//! The stdio relay, native only: `varde mcp` reads newline-delimited
//! JSON-RPC on stdin and POSTs each message to the app's server on
//! `127.0.0.1`, writing each answer to stdout as one line. It holds no
//! design, only the HTTP session's id, the negotiated protocol version
//! and the client's `initialize` and `notifications/initialized`, kept
//! to start a new session when the server says the old one ended (404).
//!
//! An HTTP/1.1 client on `std::net`, one request at a time on one
//! connection (a new one if the server closed the last), the response
//! head read with `httparse` within [`http::Limits`]' head bounds and the
//! body within [`BODY_BYTES`]. Everything from stdin and the socket is
//! bounded: a line within the server's body cap, an answer within its
//! own. The token is sent only to `127.0.0.1` on the target's port and
//! never written anywhere else. Whatever listens there gets it, though:
//! while Varde isn't listening, another local process (another user's)
//! can take the port and be sent the token, an accepted risk the
//! settings state.
//!
//! A failure to reach or be served by the server answers a request (a
//! message with an `id`) with JSON-RPC error -32000 saying what, and the
//! relay keeps running, so the next message works once Varde's server is
//! started; a notification failing is dropped. Anything that isn't a
//! request or a notification (a batch, garbage) is forwarded as it is,
//! for the server to refuse, and dropped if that fails. stdin's end
//! sends DELETE for the session and returns.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::Value;

use crate::guard::{PATH, Token};
use crate::http::{self, Limits};
use crate::rpc::{self, Code, Id};
use crate::tools::ToolReply;

/// The longest line read from stdin: the server's body cap.
pub const LINE_BYTES: u64 = Limits::BODY_BYTES;

/// The longest answer body taken: a reply's image as base64 and room
/// for its text, the tools list and JSON's escapes.
pub const BODY_BYTES: u64 = (ToolReply::IMAGE_BYTES as u64).div_ceil(3) * 4 + 1024 * 1024;

/// The longest session id or protocol version put in a header.
const HEADER_VALUE: usize = 256;

/// How long connecting may take; loopback answers at once or not at all.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an answer may take: past the server's own 35 s wait on the
/// app.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// The JSON-RPC error code for the relay's own failures.
pub const ERROR_CODE: i64 = Code::Unreachable.number();

/// The answer to a request when nothing listens on the port.
pub const NOT_RUNNING: &str =
    "Varde isn't running, or its MCP server isn't started: Start it in Varde's MCP panel";

/// Where the token comes from: read when first needed and again after a
/// 401 (the token regenerated). `Err` is the words to answer a request
/// with, such as to turn MCP on in Varde's settings; it must not hold
/// the token.
pub trait TokenSource {
    fn token(&mut self) -> Result<Token, String>;
}

impl<F: FnMut() -> Result<Token, String>> TokenSource for F {
    fn token(&mut self) -> Result<Token, String> {
        self()
    }
}

/// The server to relay to.
pub struct Target<S> {
    /// The port on `127.0.0.1`.
    pub port: u16,
    pub token_source: S,
}

/// Relays `stdin` to the server at `target` and its answers to `stdout`
/// until `stdin` ends. `Err` only if reading stdin or writing stdout
/// fails: the server's failures are answers.
pub fn run(
    stdin: impl BufRead,
    stdout: impl Write,
    target: Target<impl TokenSource>,
) -> io::Result<()> {
    run_with(stdin, stdout, Relay::new(Tcp, target))
}

fn run_with<C: Connect, S: TokenSource>(
    mut stdin: impl BufRead,
    mut stdout: impl Write,
    mut relay: Relay<C, S>,
) -> io::Result<()> {
    let mut relayed = || {
        while let Some(line) = read_line(&mut stdin, LINE_BYTES)? {
            relay.handle(line, &mut stdout)?;
        }
        Ok(())
    };
    let result = relayed();
    // Also when stdout failed: the client is gone, its session with it.
    relay.end();
    result
}

/// A line read from stdin.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    /// Without its line break.
    Text(Vec<u8>),
    /// Longer than the bound: read past and dropped.
    TooLong,
}

/// The next line, `None` at the end. A last line without a break counts.
fn read_line(reader: &mut impl BufRead, limit: u64) -> io::Result<Option<Line>> {
    let mut line = Vec::new();
    let mut too_long = false;
    let mut any = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            break;
        }
        any = true;
        let (taken, ended) = match available.iter().position(|&b| b == b'\n') {
            Some(at) => (at, true),
            None => (available.len(), false),
        };
        if !too_long {
            let room = usize::try_from(limit)
                .unwrap_or(usize::MAX)
                .saturating_sub(line.len());
            if taken > room {
                too_long = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(&available[..taken]);
            }
        }
        reader.consume(if ended { taken + 1 } else { taken });
        if ended {
            break;
        }
    }
    if !any {
        return Ok(None);
    }
    if too_long {
        return Ok(Some(Line::TooLong));
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(Some(Line::Text(line)))
}

/// What a line from stdin is, as far as the relay cares.
#[derive(Debug, Default)]
struct Kind {
    /// An object with a `method` and an `id`: its failures are answered.
    request: bool,
    /// The request's id, if the server would take it: an id it refuses
    /// (past `i64`, a fraction, an object, ...) is answered to `null`, as
    /// the server answers it, rather than echoed rounded or as sent.
    id: Option<Id>,
    /// An object with a `method` and no `id`: never answered.
    notification: bool,
    initialize: bool,
    initialized: bool,
}

impl Kind {
    fn of(line: &[u8]) -> Self {
        // Bounded by the line's cap; serde_json bounds nesting itself.
        let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(line) else {
            return Kind::default();
        };
        let Some(Value::String(method)) = object.get("method") else {
            return Kind::default();
        };
        let id = object.get("id");
        Kind {
            request: id.is_some(),
            id: id.and_then(Id::from_value),
            notification: id.is_none(),
            initialize: method == "initialize" && id.is_some(),
            initialized: method == "notifications/initialized" && id.is_none(),
        }
    }
}

/// Opens connections to the server; the fake in tests.
trait Connect {
    type Stream: Read + Write;
    fn connect(&mut self, port: u16) -> io::Result<Self::Stream>;
}

/// Over TCP to `127.0.0.1`.
struct Tcp;

impl Connect for Tcp {
    type Stream = TcpStream;

    fn connect(&mut self, port: u16) -> io::Result<TcpStream> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let _ = stream.set_nodelay(true);
        Ok(stream)
    }
}

/// An answer read off the connection.
#[derive(Debug)]
struct Answer {
    status: u16,
    reason: String,
    session: Option<String>,
    /// `Content-Type` is `application/json`.
    json: bool,
    body: Vec<u8>,
}

/// Why no answer came.
#[derive(Debug)]
enum Failure {
    /// Nothing listens on the port.
    Refused,
    Io(io::Error),
    /// A head or body past its bound.
    TooLarge,
    Malformed,
    /// The token couldn't be read: the source's words.
    Token(String),
    /// A 401 even with the token read again.
    Unauthorized,
    /// The session ended and a new one couldn't be started.
    Replay(Box<Failure>),
    /// A status the relay doesn't pass on.
    Status(u16, String),
}

impl Failure {
    fn message(&self) -> String {
        match self {
            Failure::Refused => NOT_RUNNING.to_owned(),
            Failure::Io(error) => {
                format!("The connection to Varde's MCP server failed: {error}")
            }
            Failure::TooLarge => format!(
                "Varde's MCP server sent an answer larger than the relay takes \
                 ({BODY_BYTES} bytes)"
            ),
            Failure::Malformed => "Varde's MCP server sent a malformed answer".to_owned(),
            Failure::Token(words) => words.clone(),
            Failure::Unauthorized => "Varde's MCP server refused the token (401 Unauthorized), \
                 read again too: is the port Varde's?"
                .to_owned(),
            Failure::Replay(why) => format!(
                "The session with Varde ended and a new one couldn't be started: {}",
                why.message()
            ),
            Failure::Status(code, reason) => {
                format!("Varde's MCP server refused the message: {code} {reason}")
            }
        }
    }
}

/// The relay's state between lines.
struct Relay<C: Connect, S> {
    connect: C,
    port: u16,
    source: S,
    token: Option<Token>,
    /// The open connection, if the last answer left it open.
    connection: Option<BufReader<C::Stream>>,
    session: Option<String>,
    version: Option<String>,
    /// The client's `initialize` and `notifications/initialized`, as
    /// sent, kept for a new session.
    initialize: Option<Vec<u8>>,
    initialized: Option<Vec<u8>>,
}

impl<C: Connect, S: TokenSource> Relay<C, S> {
    fn new(connect: C, target: Target<S>) -> Self {
        Self {
            connect,
            port: target.port,
            source: target.token_source,
            token: None,
            connection: None,
            session: None,
            version: None,
            initialize: None,
            initialized: None,
        }
    }

    /// Relays one line, writing its answer, if any, to `out`.
    fn handle(&mut self, line: Line, out: &mut impl Write) -> io::Result<()> {
        let line = match line {
            Line::Text(line) => line,
            Line::TooLong => {
                // The id can't be read without the line; null is
                // JSON-RPC's id for a message that couldn't be read.
                let words =
                    format!("The message is longer than the relay takes ({LINE_BYTES} bytes)");
                return write_error(out, None, &words);
            }
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        let kind = Kind::of(&line);
        let result = self.send(&line, kind.initialize, true);
        if kind.notification {
            // Never answered, whatever came back.
            if kind.initialized && result.is_ok_and(|a| a.status == 202) {
                self.initialized = Some(line);
            }
            return Ok(());
        }
        match result {
            Ok(answer) if answer.status == 200 => {
                if answer.body.is_empty() || !answer.json {
                    return self.fail(&kind, &Failure::Malformed, out);
                }
                if kind.initialize && is_result(&answer.body) {
                    self.started(&answer, line);
                }
                write_line(out, answer.body)
            }
            Ok(answer) if answer.status == 202 => Ok(()),
            // A JSON-RPC error of the server's own, such as -32700 with
            // its 400, goes to the client as it is.
            Ok(answer) if is_error_reply(&answer.body) => write_line(out, answer.body),
            Ok(answer) => self.fail(&kind, &Failure::Status(answer.status, answer.reason), out),
            Err(failure) => self.fail(&kind, &failure, out),
        }
    }

    /// A request's failure answered; anything else's dropped.
    fn fail(&self, kind: &Kind, failure: &Failure, out: &mut impl Write) -> io::Result<()> {
        if !kind.request {
            return Ok(());
        }
        write_error(out, kind.id.as_ref(), &failure.message())
    }

    /// Takes the session and version from `initialize`'s answer and
    /// keeps the line for a new session.
    fn started(&mut self, answer: &Answer, line: Vec<u8>) {
        self.session = answer.session.clone();
        self.version = serde_json::from_slice::<Value>(&answer.body)
            .ok()
            .and_then(|v| v["result"]["protocolVersion"].as_str().map(str::to_owned))
            .filter(|v| header_value(v));
        self.initialize = Some(line);
    }

    /// POSTs `body`, reading the token again after a 401 and, if
    /// `replay`, starting a new session after a 404, each once. Not
    /// `replay` within a replay, which would start one after another
    /// for as long as the server 404s.
    fn send(&mut self, body: &[u8], initialize: bool, replay: bool) -> Result<Answer, Failure> {
        let mut reread = false;
        let mut replayed = false;
        loop {
            let answer = self.post(body, initialize)?;
            match answer.status {
                401 if reread => return Err(Failure::Unauthorized),
                401 => {
                    reread = true;
                    self.token = None;
                }
                404 if replay && !replayed && !initialize && self.initialize.is_some() => {
                    replayed = true;
                    self.replay().map_err(|f| Failure::Replay(Box::new(f)))?;
                }
                _ => return Ok(answer),
            }
        }
    }

    /// Starts a new session with the kept `initialize` and
    /// `notifications/initialized`.
    fn replay(&mut self) -> Result<(), Failure> {
        let Some(initialize) = self.initialize.clone() else {
            return Err(Failure::Malformed);
        };
        self.session = None;
        self.version = None;
        let answer = self.send(&initialize, true, false)?;
        if answer.status != 200 {
            return Err(Failure::Status(answer.status, answer.reason));
        }
        if answer.session.is_none() {
            return Err(Failure::Malformed);
        }
        self.started(&answer, initialize);
        if let Some(initialized) = self.initialized.clone() {
            let answer = self.send(&initialized, false, false)?;
            if answer.status != 202 {
                return Err(Failure::Status(answer.status, answer.reason));
            }
        }
        Ok(())
    }

    fn post(&mut self, body: &[u8], initialize: bool) -> Result<Answer, Failure> {
        let token = match &self.token {
            Some(token) => token.clone(),
            None => {
                let token = self.source.token().map_err(Failure::Token)?;
                self.token = Some(token.clone());
                token
            }
        };
        let length = body.len().to_string();
        let mut headers = vec![
            ("Content-Type", "application/json"),
            ("Accept", "application/json, text/event-stream"),
            ("Content-Length", length.as_str()),
        ];
        // No session on `initialize`: it starts one.
        if !initialize {
            if let Some(session) = &self.session {
                headers.push(("MCP-Session-Id", session));
            }
            if let Some(version) = &self.version {
                headers.push(("MCP-Protocol-Version", version));
            }
        }
        let head = request_head("POST", self.port, &token, &headers);
        self.exchange(head.as_bytes(), body)
    }

    /// Sends DELETE for the session, if there is one; its answer doesn't
    /// matter.
    fn end(&mut self) {
        let (Some(session), Some(token)) = (self.session.take(), self.token.clone()) else {
            return;
        };
        let head = request_head(
            "DELETE",
            self.port,
            &token,
            &[("MCP-Session-Id", &session), ("Connection", "close")],
        );
        let _ = self.exchange(head.as_bytes(), &[]);
        self.connection = None;
    }

    /// Writes a request and reads its answer, on a new connection if the
    /// kept one turns out closed before answering.
    fn exchange(&mut self, head: &[u8], body: &[u8]) -> Result<Answer, Failure> {
        let reused = self.connection.is_some();
        match self.try_exchange(head, body) {
            Err(Attempt::Closed) if reused => {
                self.connection = None;
                self.try_exchange(head, body).map_err(Attempt::failure)
            }
            result => result.map_err(Attempt::failure),
        }
    }

    fn try_exchange(&mut self, head: &[u8], body: &[u8]) -> Result<Answer, Attempt> {
        let mut connection = match self.connection.take() {
            Some(connection) => connection,
            None => match self.connect.connect(self.port) {
                Ok(stream) => BufReader::new(stream),
                Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                    return Err(Attempt::Failed(Failure::Refused));
                }
                Err(e) => return Err(Attempt::Failed(Failure::Io(e))),
            },
        };
        let written = (|| {
            let stream = connection.get_mut();
            stream.write_all(head)?;
            stream.write_all(body)?;
            stream.flush()
        })();
        if written.is_err() {
            return Err(Attempt::Closed);
        }
        let (answer, close) = read_answer(&mut connection)?;
        if !close {
            self.connection = Some(connection);
        }
        Ok(answer)
    }
}

/// A request's head to [`PATH`] on `127.0.0.1:port` with the token and
/// `headers`, each value already one of [`header_value`]'s or the
/// relay's own.
fn request_head(method: &str, port: u16, token: &Token, headers: &[(&str, &str)]) -> String {
    let mut head = format!(
        "{method} {PATH} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n"
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    head
}

/// One try at an exchange.
#[derive(Debug)]
enum Attempt {
    /// The connection was closed before an answer began: worth one more
    /// try on a new one if it was a kept one.
    Closed,
    Failed(Failure),
}

impl Attempt {
    fn failure(self) -> Failure {
        match self {
            Attempt::Closed => Failure::Io(io::Error::from(io::ErrorKind::ConnectionAborted)),
            Attempt::Failed(failure) => failure,
        }
    }
}

/// Reads one answer, and whether the server closes the connection after
/// it.
fn read_answer(reader: &mut impl BufRead) -> Result<(Answer, bool), Attempt> {
    let limits = Limits::default();
    let head = match http::read_head(reader, limits.head_bytes) {
        Ok(Some(head)) => head,
        Ok(None) => return Err(Attempt::Closed),
        Err(http::Error::Io(e))
            if matches!(
                e.kind(),
                io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
            ) =>
        {
            return Err(Attempt::Closed);
        }
        Err(http::Error::Io(e)) => return Err(Attempt::Failed(Failure::Io(e))),
        Err(http::Error::HeadTooLarge) => return Err(Attempt::Failed(Failure::TooLarge)),
        Err(_) => return Err(Attempt::Failed(Failure::Malformed)),
    };
    let malformed = || Attempt::Failed(Failure::Malformed);
    let mut headers = vec![httparse::EMPTY_HEADER; limits.headers];
    let mut parsed = httparse::Response::new(&mut headers);
    match parsed.parse(&head) {
        Ok(httparse::Status::Complete(_)) => {}
        Err(httparse::Error::TooManyHeaders) => return Err(Attempt::Failed(Failure::TooLarge)),
        _ => return Err(malformed()),
    }
    let (Some(status), Some(version)) = (parsed.code, parsed.version) else {
        return Err(malformed());
    };
    let reason = http_text(parsed.reason.unwrap_or(""));
    let headers = http::owned_headers(parsed.headers).map_err(|_| malformed())?;
    let length = http::body_length(&headers).map_err(|e| {
        Attempt::Failed(match e {
            http::Error::BodyTooLarge => Failure::TooLarge,
            _ => Failure::Malformed,
        })
    })?;
    let session = match http::once(&headers, "mcp-session-id") {
        Ok(None) => None,
        Ok(Some(value)) => match value.trim_matches([' ', '\t']) {
            value if header_value(value) => Some(value.to_owned()),
            _ => return Err(malformed()),
        },
        Err(_) => return Err(malformed()),
    };
    let json = http::named(&headers, "content-type")
        .next()
        .is_some_and(|t| {
            t.split(';')
                .next()
                .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
        });
    // Without a length the body runs to the connection's end (an event
    // stream, say): none is read, and the connection is dropped so the
    // rest isn't read as the next answer.
    let close =
        version == 0 || length.is_none() || http::has_token(&headers, "connection", "close");
    let length = length.unwrap_or(0);
    if length > BODY_BYTES {
        return Err(Attempt::Failed(Failure::TooLarge));
    }
    let length = usize::try_from(length).map_err(|_| Attempt::Failed(Failure::TooLarge))?;
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| Attempt::Failed(Failure::Io(e)))?;
    Ok((
        Answer {
            status,
            reason,
            session,
            json,
            body,
        },
        close,
    ))
}

/// A JSON-RPC success reply.
fn is_result(body: &[u8]) -> bool {
    matches!(
        serde_json::from_slice::<Value>(body),
        Ok(Value::Object(object)) if object.contains_key("result")
    )
}

/// A JSON-RPC error reply.
fn is_error_reply(body: &[u8]) -> bool {
    matches!(
        serde_json::from_slice::<Value>(body),
        Ok(Value::Object(object)) if object.contains_key("error")
    )
}

/// Text from the server fit to quote: printable ASCII, cut short.
fn http_text(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(64)
        .collect()
}

/// A value safe to send back in a header: short, visible ASCII.
fn header_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= HEADER_VALUE && value.bytes().all(|b| b.is_ascii_graphic())
}

/// Writes `body` as one line. Line breaks in JSON are only ever
/// whitespace between tokens (strings escape theirs), so they become
/// spaces.
fn write_line(out: &mut impl Write, mut body: Vec<u8>) -> io::Result<()> {
    for byte in &mut body {
        if *byte == b'\n' || *byte == b'\r' {
            *byte = b' ';
        }
    }
    body.push(b'\n');
    out.write_all(&body)?;
    out.flush()
}

/// Answers `id`, or `null`, with the relay's own error.
fn write_error(out: &mut impl Write, id: Option<&Id>, message: &str) -> io::Result<()> {
    write_line(out, rpc::error(id, Code::Unreachable, message))
}

#[cfg(test)]
mod tests;
