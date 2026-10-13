//! HTTP/1.1, as much of it as the MCP server needs: reading one request
//! from a stream and writing one response to it. The caller loops to keep
//! a connection alive, until a request asks to close or the stream ends.
//!
//! Everything read comes off the network, so it's read within [`Limits`]:
//! the head no longer than its bound before it ends, the body's length
//! checked before anything is allocated for it. A request breaking one is
//! an [`Error`] with the [`Status`] to answer it with. A URL too long is
//! 400 rather than 414, and a head too long or with too many headers 431,
//! so the statuses stay the few the server sends.

use std::io::{self, BufRead, Write};

/// The bounds a request is read within.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The request target's length, in bytes.
    pub url: usize,
    /// How many headers.
    pub headers: usize,
    /// The whole head's length, request line and blank line included.
    pub head_bytes: usize,
    /// The body's length.
    pub body_bytes: u64,
}

impl Limits {
    /// The default [`Limits::url`]: `/mcp` and room to spare.
    pub const URL: usize = 1024;
    /// The default [`Limits::headers`].
    pub const HEADERS: usize = 64;
    /// The default [`Limits::head_bytes`].
    pub const HEAD_BYTES: usize = 16 * 1024;
    /// The default [`Limits::body_bytes`]: a tool call's arguments, with
    /// room for a long one.
    pub const BODY_BYTES: u64 = 4 * 1024 * 1024;
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            url: Self::URL,
            headers: Self::HEADERS,
            head_bytes: Self::HEAD_BYTES,
            body_bytes: Self::BODY_BYTES,
        }
    }
}

/// A request read from a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// As sent, such as `POST`.
    pub method: String,
    /// The request target, such as `/mcp`.
    pub path: String,
    /// In the order sent, names as sent: see [`Request::header`].
    pub headers: Vec<(String, String)>,
    /// Empty unless `Content-Length` said otherwise.
    pub body: Vec<u8>,
    /// The client asked to close the connection after this request
    /// (`Connection: close`, or HTTP/1.0 without `keep-alive`).
    pub close: bool,
}

impl Request {
    /// The first header named `name`, in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.all(name).next()
    }

    /// Every header named `name`, in any case, in the order sent.
    pub(crate) fn all<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> {
        named(&self.headers, name)
    }

    /// The header named `name` if sent at most once, `Err` if more often:
    /// with two, one check and whatever reads the request after could
    /// each look at a different one.
    pub(crate) fn once(&self, name: &str) -> Result<Option<&str>, Repeated> {
        once(&self.headers, name)
    }
}

/// Every header in `headers` named `name`, in any case, in the order
/// sent. Shared by requests and the relay's reading of answers.
pub(crate) fn named<'a>(
    headers: &'a [(String, String)],
    name: &str,
) -> impl Iterator<Item = &'a str> {
    headers
        .iter()
        .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The header named `name` if in `headers` at most once, as
/// [`Request::once`].
pub(crate) fn once<'a>(
    headers: &'a [(String, String)],
    name: &str,
) -> Result<Option<&'a str>, Repeated> {
    let mut all = named(headers, name);
    let first = all.next();
    match all.next() {
        None => Ok(first),
        Some(_) => Err(Repeated),
    }
}

/// Whether a comma-separated header named `name`, such as
/// `Connection`, lists `token`, in any case.
pub(crate) fn has_token(headers: &[(String, String)], name: &str, token: &str) -> bool {
    named(headers, name)
        .flat_map(|v| v.split(','))
        .any(|t| t.trim().eq_ignore_ascii_case(token))
}

/// The headers httparse found, values as text: [`Error::Malformed`] if
/// one isn't UTF-8.
pub(crate) fn owned_headers(parsed: &[httparse::Header]) -> Result<Vec<(String, String)>, Error> {
    parsed
        .iter()
        .map(|h| {
            let value = std::str::from_utf8(h.value).map_err(|_| Error::Malformed)?;
            Ok((h.name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// The body's length by `headers`, if they give one:
/// [`Error::LengthRequired`] with any `Transfer-Encoding`, as framing
/// by both could be read two ways (request smuggling), else as
/// [`content_length`].
pub(crate) fn body_length(headers: &[(String, String)]) -> Result<Option<u64>, Error> {
    if named(headers, "transfer-encoding").next().is_some() {
        return Err(Error::LengthRequired);
    }
    content_length(named(headers, "content-length"))
}

/// A header sent more than once where only one is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Repeated;

/// Why a request couldn't be read.
#[derive(Debug)]
pub enum Error {
    /// The stream failed, or ended within a request: nothing to answer.
    Io(io::Error),
    /// Not HTTP/1.x as it should be, or a `Content-Length` that isn't one.
    Malformed,
    /// The request target is longer than [`Limits::url`].
    UrlTooLong,
    /// The head is longer than [`Limits::head_bytes`] or has more than
    /// [`Limits::headers`] headers.
    HeadTooLarge,
    /// A body with no `Content-Length`, or sent with `Transfer-Encoding`.
    LengthRequired,
    /// `Content-Length` over [`Limits::body_bytes`].
    BodyTooLarge,
}

impl Error {
    /// The status to answer with, none if the stream is gone. The
    /// connection is closed after it either way: what's left of the
    /// request can't be told from the next one.
    pub fn status(&self) -> Option<Status> {
        match self {
            Error::Io(_) => None,
            Error::Malformed | Error::UrlTooLong => Some(Status::BadRequest),
            Error::HeadTooLarge => Some(Status::HeadersTooLarge),
            Error::LengthRequired => Some(Status::LengthRequired),
            Error::BodyTooLarge => Some(Status::PayloadTooLarge),
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}

/// Reads one request, `None` if the stream ends before its first byte.
pub fn read_request(reader: &mut impl BufRead, limits: &Limits) -> Result<Option<Request>, Error> {
    let Some((mut request, length)) = read_request_head(reader, limits)? else {
        return Ok(None);
    };
    read_body(reader, &mut request, length)?;
    Ok(Some(request))
}

/// Reads one request's head, `None` if the stream ends before its first
/// byte: the request with its body still empty, and the body's length,
/// checked against [`Limits::body_bytes`] but not yet read, so the
/// caller can refuse the request first. Read it with [`read_body`], or
/// close the connection, as what's left can't be told from the next
/// request.
pub fn read_request_head(
    reader: &mut impl BufRead,
    limits: &Limits,
) -> Result<Option<(Request, usize)>, Error> {
    let Some(head) = read_head(reader, limits.head_bytes)? else {
        return Ok(None);
    };
    let mut headers = vec![httparse::EMPTY_HEADER; limits.headers];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(&head) {
        Ok(httparse::Status::Complete(_)) => {}
        // `read_head` stopped at the blank line, so the head is whole.
        Ok(httparse::Status::Partial) => return Err(Error::Malformed),
        Err(httparse::Error::TooManyHeaders) => return Err(Error::HeadTooLarge),
        Err(_) => return Err(Error::Malformed),
    }
    let (Some(method), Some(path), Some(version)) = (parsed.method, parsed.path, parsed.version)
    else {
        return Err(Error::Malformed);
    };
    if path.len() > limits.url {
        return Err(Error::UrlTooLong);
    }
    let headers = owned_headers(parsed.headers)?;
    let mut request = Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body: Vec::new(),
        close: false,
    };

    let connection = |token| has_token(&request.headers, "connection", token);
    request.close = connection("close") || (version == 0 && !connection("keep-alive"));

    let Some(length) = body_length(&request.headers)? else {
        if request.method == "POST" {
            return Err(Error::LengthRequired);
        }
        return Ok(Some((request, 0)));
    };
    if length > limits.body_bytes {
        return Err(Error::BodyTooLarge);
    }
    let length = usize::try_from(length).map_err(|_| Error::BodyTooLarge)?;
    Ok(Some((request, length)))
}

/// Reads `request`'s body of `length` bytes, as [`read_request_head`]
/// gave it.
pub fn read_body(
    reader: &mut impl BufRead,
    request: &mut Request,
    length: usize,
) -> Result<(), Error> {
    request.body = vec![0; length];
    reader.read_exact(&mut request.body)?;
    Ok(())
}

/// The head up to and with its blank line, read a byte at a time out of
/// the buffer so nothing past it is taken, and never more than `limit`
/// bytes. `None` if the stream ends first thing.
pub(crate) fn read_head(reader: &mut impl BufRead, limit: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut head = Vec::new();
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if available.is_empty() {
            if head.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }
        let mut taken = 0;
        let mut ended = false;
        for &byte in available {
            if head.len() >= limit {
                break;
            }
            head.push(byte);
            taken += 1;
            if head.ends_with(b"\n\r\n") || head.ends_with(b"\n\n") {
                ended = true;
                break;
            }
        }
        reader.consume(taken);
        if ended {
            return Ok(Some(head));
        }
        if head.len() >= limit {
            return Err(Error::HeadTooLarge);
        }
    }
}

/// `Content-Length` from its `values`, one per copy sent, if any: digits
/// alone, and the same in every copy. [`Error::BodyTooLarge`] if it
/// overflows `u64`.
pub(crate) fn content_length<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> Result<Option<u64>, Error> {
    let mut length = None;
    for value in values {
        // Only HTTP's own whitespace: `str::trim` would also take
        // Unicode spaces off a value that isn't digits alone.
        let value = value.trim_matches([' ', '\t']);
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Malformed);
        }
        // All digits, so it can only fail by overflowing: too large.
        let parsed = value.parse::<u64>().map_err(|_| Error::BodyTooLarge)?;
        if length.is_some_and(|l| l != parsed) {
            return Err(Error::Malformed);
        }
        length = Some(parsed);
    }
    Ok(length)
}

/// The statuses the server answers with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Accepted,
    NoContent,
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    LengthRequired,
    PayloadTooLarge,
    HeadersTooLarge,
}

impl Status {
    /// The status code, such as 200.
    pub fn code(self) -> u16 {
        match self {
            Status::Ok => 200,
            Status::Accepted => 202,
            Status::NoContent => 204,
            Status::BadRequest => 400,
            Status::Unauthorized => 401,
            Status::Forbidden => 403,
            Status::NotFound => 404,
            Status::MethodNotAllowed => 405,
            Status::LengthRequired => 411,
            Status::PayloadTooLarge => 413,
            Status::HeadersTooLarge => 431,
        }
    }

    /// The reason phrase, such as `OK`.
    pub fn reason(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Accepted => "Accepted",
            Status::NoContent => "No Content",
            Status::BadRequest => "Bad Request",
            Status::Unauthorized => "Unauthorized",
            Status::Forbidden => "Forbidden",
            Status::NotFound => "Not Found",
            Status::MethodNotAllowed => "Method Not Allowed",
            Status::LengthRequired => "Length Required",
            Status::PayloadTooLarge => "Payload Too Large",
            Status::HeadersTooLarge => "Request Header Fields Too Large",
        }
    }
}

/// A response to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: Status,
    /// `Content-Type`, sent if there's a body.
    pub content_type: &'static str,
    /// Others, such as `WWW-Authenticate`, `MCP-Session-Id` or `Allow`.
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// `status` with no body.
    pub fn empty(status: Status) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// 200 with a JSON body.
    pub fn json(body: Vec<u8>) -> Self {
        Self {
            status: Status::Ok,
            content_type: "application/json",
            headers: Vec::new(),
            body,
        }
    }

    /// 405, naming the methods `/mcp` takes (`POST` and `DELETE`): the
    /// session's answer to any other, and the guard's to `OPTIONS`.
    pub(crate) fn method_not_allowed() -> Self {
        Self::empty(Status::MethodNotAllowed).with_header("Allow", "POST, DELETE")
    }

    /// With the header `name: value` added.
    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

/// Writes `response`, with `Connection: close` if `close`, and flushes.
/// A header value with a line break in it is refused before anything is
/// written, as it would forge headers of its own.
pub fn write_response(writer: &mut impl Write, response: &Response, close: bool) -> io::Result<()> {
    if response
        .headers
        .iter()
        .any(|(n, v)| breaks_line(n) || breaks_line(v))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "header with a line break",
        ));
    }
    let status = response.status;
    let mut head = format!("HTTP/1.1 {} {}\r\n", status.code(), status.reason());
    // 204 carries neither a body nor its length.
    if status != Status::NoContent {
        if !response.body.is_empty() {
            head.push_str(&format!("Content-Type: {}\r\n", response.content_type));
        }
        head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if close {
        head.push_str("Connection: close\r\n");
    }
    head.push_str("\r\n");
    writer.write_all(head.as_bytes())?;
    if status != Status::NoContent {
        writer.write_all(&response.body)?;
    }
    writer.flush()
}

fn breaks_line(text: &str) -> bool {
    text.bytes().any(|b| b == b'\r' || b == b'\n')
}

#[cfg(test)]
mod tests;
