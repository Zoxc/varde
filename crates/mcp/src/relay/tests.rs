use std::cell::RefCell;
use std::io::Cursor;
use std::net::TcpListener;
use std::rc::Rc;

use futures::executor::{BlockingStream, block_on_stream};

use serde_json::json;

use super::*;
use crate::server::{self, Event, Events, Handle, Timeouts};
use crate::session::{self, End};

const TOKEN: [u8; Token::BYTES] = [7; Token::BYTES];
const WRONG: [u8; Token::BYTES] = [8; Token::BYTES];

fn token() -> Token {
    Token::from_bytes(TOKEN)
}

fn initialize(id: i64) -> Vec<u8> {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "test" } },
    })
    .to_string()
    .into_bytes()
}

fn initialized() -> Vec<u8> {
    br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_vec()
}

fn ping(id: i64) -> Vec<u8> {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#).into_bytes()
}

/// The lines written, parsed.
fn lines(out: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(out).unwrap();
    assert!(
        !text.contains(&token().to_string()),
        "the token was written"
    );
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// The -32000 error's message in `reply`, which answers `id`.
fn error_message(reply: &Value, id: Value) -> String {
    assert_eq!(reply["id"], id);
    assert_eq!(reply["error"]["code"], ERROR_CODE);
    reply["error"]["message"].as_str().unwrap().to_owned()
}

// The in-memory fake.

type Handler = Box<dyn FnMut(&http::Request) -> Vec<u8>>;

struct Fake {
    handler: Handler,
    requests: Vec<http::Request>,
    refuse: bool,
    connections: usize,
}

#[derive(Clone)]
struct FakeConnect(Rc<RefCell<Fake>>);

impl FakeConnect {
    fn new(handler: impl FnMut(&http::Request) -> Vec<u8> + 'static) -> Self {
        Self(Rc::new(RefCell::new(Fake {
            handler: Box::new(handler),
            requests: Vec::new(),
            refuse: false,
            connections: 0,
        })))
    }

    fn requests(&self) -> Vec<http::Request> {
        self.0.borrow().requests.clone()
    }
}

impl Connect for FakeConnect {
    type Stream = FakeStream;

    fn connect(&mut self, _port: u16) -> io::Result<FakeStream> {
        let mut fake = self.0.borrow_mut();
        if fake.refuse {
            return Err(io::ErrorKind::ConnectionRefused.into());
        }
        fake.connections += 1;
        Ok(FakeStream {
            fake: self.0.clone(),
            written: Vec::new(),
            reply: Cursor::new(Vec::new()),
        })
    }
}

struct FakeStream {
    fake: Rc<RefCell<Fake>>,
    written: Vec<u8>,
    reply: Cursor<Vec<u8>>,
}

impl Write for FakeStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.written.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for FakeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.reply.position() as usize >= self.reply.get_ref().len() && !self.written.is_empty()
        {
            let mut reader = Cursor::new(std::mem::take(&mut self.written));
            let request = http::read_request(&mut reader, &Limits::default())
                .unwrap()
                .unwrap();
            let rest = reader.get_ref()[reader.position() as usize..].to_vec();
            self.written = rest;
            let mut fake = self.fake.borrow_mut();
            let reply = (fake.handler)(&request);
            fake.requests.push(request);
            self.reply = Cursor::new(reply);
        }
        self.reply.read(buf)
    }
}

fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
    // JSON unless said otherwise, as the server's bodies are.
    if !body.is_empty()
        && !headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
    {
        head.push_str("Content-Type: application/json\r\n");
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// A fake server: `initialize` starts session `s<n>`, a request with a
/// session other than the current one gets 404, `ping` answers.
fn fake() -> FakeConnect {
    let mut sessions = 0;
    FakeConnect::new(move |request| {
        let current = format!("s{sessions}");
        if request.method == "DELETE" {
            return response("204 No Content", &[], b"");
        }
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        if body["method"] == "initialize" {
            sessions += 1;
            let reply = json!({
                "jsonrpc": "2.0", "id": body["id"],
                "result": { "protocolVersion": "2025-06-18" },
            });
            return response(
                "200 OK",
                &[("MCP-Session-Id", &format!("s{sessions}"))],
                reply.to_string().as_bytes(),
            );
        }
        if request.header("mcp-session-id") != Some(current.as_str()) {
            return response("404 Not Found", &[], b"");
        }
        if body.get("id").is_none() {
            return response("202 Accepted", &[], b"");
        }
        let reply = json!({ "jsonrpc": "2.0", "id": body["id"], "result": {} });
        response("200 OK", &[], reply.to_string().as_bytes())
    })
}

fn relay(connect: FakeConnect) -> Relay<FakeConnect, impl TokenSource> {
    Relay::new(
        connect,
        Target {
            port: 7311,
            token_source: || Ok(token()),
        },
    )
}

fn send(relay: &mut Relay<impl Connect, impl TokenSource>, line: Vec<u8>) -> Vec<Value> {
    let mut out = Vec::new();
    relay.handle(Line::Text(line), &mut out).unwrap();
    lines(&out)
}

#[test]
fn reads_bounded_lines() {
    let mut input = Cursor::new(b"ab\r\n\ncdefgh\nij".to_vec());
    assert_eq!(
        read_line(&mut input, 4).unwrap(),
        Some(Line::Text(b"ab".to_vec()))
    );
    assert_eq!(read_line(&mut input, 4).unwrap(), Some(Line::Text(vec![])));
    assert_eq!(read_line(&mut input, 4).unwrap(), Some(Line::TooLong));
    assert_eq!(
        read_line(&mut input, 4).unwrap(),
        Some(Line::Text(b"ij".to_vec()))
    );
    assert_eq!(read_line(&mut input, 4).unwrap(), None);
}

#[test]
fn forwards_with_the_session_headers() {
    let connect = fake();
    let mut relay = relay(connect.clone());
    let out = send(&mut relay, initialize(1));
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
    assert!(send(&mut relay, initialized()).is_empty());
    let out = send(&mut relay, ping(2));
    assert_eq!(out, [json!({ "jsonrpc": "2.0", "id": 2, "result": {} })]);

    let requests = connect.requests();
    assert_eq!(requests.len(), 3);
    let first = &requests[0];
    assert_eq!(first.method, "POST");
    assert_eq!(first.path, "/mcp");
    assert_eq!(first.header("host"), Some("127.0.0.1:7311"));
    let bearer = format!("Bearer {}", token());
    assert_eq!(first.header("authorization"), Some(bearer.as_str()));
    assert_eq!(first.body, initialize(1));
    assert_eq!(first.header("mcp-session-id"), None);
    assert_eq!(first.header("mcp-protocol-version"), None);
    for request in &requests[1..] {
        assert_eq!(request.header("mcp-session-id"), Some("s1"));
        assert_eq!(request.header("mcp-protocol-version"), Some("2025-06-18"));
    }
    // One connection, kept.
    assert_eq!(connect.0.borrow().connections, 1);
}

#[test]
fn garbage_and_batches_go_as_they_are() {
    let connect = FakeConnect::new(|request| {
        let reply =
            json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "x" } });
        assert!(!request.body.is_empty());
        response("200 OK", &[], reply.to_string().as_bytes())
    });
    let mut relay = relay(connect.clone());
    for line in [b"not json".to_vec(), b"[1,2]".to_vec()] {
        let out = send(&mut relay, line.clone());
        assert_eq!(out[0]["error"]["code"], -32700);
        assert_eq!(connect.requests().last().unwrap().body, line);
    }
    // A blank line isn't sent.
    assert!(send(&mut relay, b"  ".to_vec()).is_empty());
    assert_eq!(connect.requests().len(), 2);
}

#[test]
fn an_answer_goes_out_as_one_line() {
    let connect =
        FakeConnect::new(|_| response("200 OK", &[], b"{\n\"id\": 1,\r\n\"result\": \"a\\nb\"}"));
    let mut relay = relay(connect);
    let out = send(&mut relay, ping(1));
    assert_eq!(out, [json!({ "id": 1, "result": "a\nb" })]);
}

#[test]
fn refused_answers_requests_and_drops_notifications() {
    let connect = fake();
    connect.0.borrow_mut().refuse = true;
    let mut relay = relay(connect.clone());
    let out = send(&mut relay, initialize(1));
    assert_eq!(error_message(&out[0], json!(1)), NOT_RUNNING);
    assert!(send(&mut relay, initialized()).is_empty());
    let out = send(
        &mut relay,
        br#"{"jsonrpc":"2.0","id":"x","method":"ping"}"#.to_vec(),
    );
    assert_eq!(error_message(&out[0], json!("x")), NOT_RUNNING);

    // Started, the next message goes through.
    connect.0.borrow_mut().refuse = false;
    let out = send(&mut relay, initialize(2));
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn a_401_reads_the_token_again_once() {
    let connect = FakeConnect::new(|request| {
        let bearer = format!("Bearer {}", token());
        if request.header("authorization") == Some(bearer.as_str()) {
            response("200 OK", &[], br#"{"jsonrpc":"2.0","id":1,"result":{}}"#)
        } else {
            response("401 Unauthorized", &[("WWW-Authenticate", "Bearer")], b"")
        }
    });
    let reads = Rc::new(RefCell::new(0));
    let counted = reads.clone();
    let mut relay = Relay::new(
        connect.clone(),
        Target {
            port: 1,
            token_source: move || {
                *counted.borrow_mut() += 1;
                Ok(if *counted.borrow() == 1 {
                    Token::from_bytes(WRONG)
                } else {
                    token()
                })
            },
        },
    );
    let out = send(&mut relay, ping(1));
    assert_eq!(out[0]["result"], json!({}));
    assert_eq!(*reads.borrow(), 2);
    // Kept: not read again.
    send(&mut relay, ping(1));
    assert_eq!(*reads.borrow(), 2);
}

#[test]
fn a_401_after_the_reread_is_an_error() {
    let connect = FakeConnect::new(|_| response("401 Unauthorized", &[], b""));
    let mut relay = relay(connect.clone());
    let out = send(&mut relay, ping(3));
    assert!(error_message(&out[0], json!(3)).contains("401"));
    assert_eq!(connect.requests().len(), 2);
}

#[test]
fn a_token_that_cant_be_read_is_an_error() {
    let mut relay = Relay::new(
        fake(),
        Target {
            port: 1,
            token_source: || Err("Turn MCP on in Varde's settings".to_owned()),
        },
    );
    let out = send(&mut relay, ping(1));
    assert_eq!(
        error_message(&out[0], json!(1)),
        "Turn MCP on in Varde's settings"
    );
}

#[test]
fn a_403_says_what() {
    let connect = FakeConnect::new(|_| response("403 Forbidden", &[], b""));
    let mut relay = relay(connect);
    let out = send(&mut relay, ping(1));
    assert!(error_message(&out[0], json!(1)).contains("403 Forbidden"));
}

#[test]
fn a_404_replays_initialize() {
    let connect = fake();
    let mut relay = relay(connect.clone());
    send(&mut relay, initialize(1));
    send(&mut relay, initialized());
    // The server forgets the session.
    relay.session = Some("gone".to_owned());
    let out = send(&mut relay, ping(5));
    assert_eq!(out, [json!({ "jsonrpc": "2.0", "id": 5, "result": {} })]);
    let requests = connect.requests();
    let methods: Vec<_> = requests
        .iter()
        .map(|r| serde_json::from_slice::<Value>(&r.body).unwrap()["method"].clone())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "notifications/initialized",
            "ping",
            "initialize",
            "notifications/initialized",
            "ping"
        ]
    );
    assert_eq!(requests[5].header("mcp-session-id"), Some("s2"));
}

#[test]
fn a_404_without_initialize_is_an_error() {
    let mut relay = relay(fake());
    let out = send(&mut relay, ping(1));
    assert!(error_message(&out[0], json!(1)).contains("404"));
}

#[test]
fn an_oversize_line_is_refused_and_the_next_goes() {
    let connect = fake();
    let mut input = vec![b'x'; LINE_BYTES as usize + 1];
    input.push(b'\n');
    input.extend(initialize(1));
    let mut out = Vec::new();
    run_with(Cursor::new(input), &mut out, relay(connect.clone())).unwrap();
    let out = lines(&out);
    assert!(error_message(&out[0], Value::Null).contains("longer"));
    assert_eq!(out[1]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(connect.requests()[0].body, initialize(1));
}

#[test]
fn an_oversize_answer_is_refused() {
    let connect = FakeConnect::new(|_| {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            BODY_BYTES + 1
        )
        .into_bytes()
    });
    let mut relay = relay(connect.clone());
    let out = send(&mut relay, ping(1));
    assert!(error_message(&out[0], json!(1)).contains("larger"));
    // An oversize head too.
    let connect = FakeConnect::new(|_| {
        let mut head = b"HTTP/1.1 200 OK\r\nX: ".to_vec();
        head.extend(vec![b'a'; Limits::HEAD_BYTES]);
        head
    });
    let mut relay = super::tests::relay(connect);
    let out = send(&mut relay, ping(2));
    assert!(error_message(&out[0], json!(2)).contains("larger"));
}

#[test]
fn a_closed_connection_is_opened_again() {
    let connect = FakeConnect::new(|_| {
        response(
            "200 OK",
            &[("Connection", "close")],
            br#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        )
    });
    let mut relay = relay(connect.clone());
    send(&mut relay, ping(1));
    send(&mut relay, ping(2));
    assert_eq!(connect.0.borrow().connections, 2);
}

#[test]
fn the_end_sends_delete() {
    let connect = fake();
    let mut input = initialize(1);
    input.push(b'\n');
    input.extend(initialized());
    let mut out = Vec::new();
    run_with(Cursor::new(input), &mut out, relay(connect.clone())).unwrap();
    let requests = connect.requests();
    let last = requests.last().unwrap();
    assert_eq!(last.method, "DELETE");
    assert_eq!(last.header("mcp-session-id"), Some("s1"));
    // No session, no DELETE.
    let connect = fake();
    run_with(Cursor::new(Vec::new()), Vec::new(), relay(connect.clone())).unwrap();
    assert!(connect.requests().is_empty());
}

// Against the real server on loopback.

fn server() -> (BlockingStream<Events>, Handle) {
    server_on(0)
}

fn server_on(port: u16) -> (BlockingStream<Events>, Handle) {
    let mut events = block_on_stream(server::start_with(port, token(), Timeouts::default()));
    match events.next() {
        Some(Event::Ready(handle)) => (events, handle),
        other => panic!("expected Ready, got {other:?}"),
    }
}

fn tcp(port: u16) -> Relay<Tcp, impl TokenSource> {
    Relay::new(
        Tcp,
        Target {
            port,
            token_source: || Ok(token()),
        },
    )
}

fn ended(events: &mut BlockingStream<Events>) -> End {
    loop {
        match events.next() {
            Some(Event::Session(session::Event::Ended { end, .. })) => return end,
            Some(_) => {}
            None => panic!("the stream ended"),
        }
    }
}

#[test]
fn relays_to_the_server() {
    let (mut events, handle) = server();
    let mut relay = tcp(handle.address().port());
    let out = send(&mut relay, initialize(1));
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
    assert!(relay.session.is_some());
    assert!(send(&mut relay, initialized()).is_empty());
    let out = send(&mut relay, ping(2));
    assert_eq!(out, [json!({ "jsonrpc": "2.0", "id": 2, "result": {} })]);
    let out = send(
        &mut relay,
        br#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#.to_vec(),
    );
    assert!(out[0]["result"]["tools"].as_array().unwrap().len() > 1);
    // A batch is the server's to refuse.
    let out = send(&mut relay, b"[]".to_vec());
    assert_eq!(out[0]["error"]["code"], -32600);
    relay.end();
    assert_eq!(ended(&mut events), End::Deleted);
}

#[test]
fn replays_after_the_server_ends_the_session() {
    let (mut events, handle) = server();
    let mut relay = tcp(handle.address().port());
    send(&mut relay, initialize(1));
    send(&mut relay, initialized());
    let first = relay.session.clone();
    handle.end_session();
    assert_eq!(ended(&mut events), End::Disconnected);
    let out = send(&mut relay, ping(2));
    assert_eq!(out, [json!({ "jsonrpc": "2.0", "id": 2, "result": {} })]);
    assert_ne!(relay.session, first);
}

#[test]
fn rereads_a_changed_token() {
    let (_events, handle) = server();
    let reads = Rc::new(RefCell::new(0));
    let counted = reads.clone();
    let mut relay = Relay::new(
        Tcp,
        Target {
            port: handle.address().port(),
            token_source: move || {
                *counted.borrow_mut() += 1;
                Ok(if *counted.borrow() == 1 {
                    Token::from_bytes(WRONG)
                } else {
                    token()
                })
            },
        },
    );
    let out = send(&mut relay, initialize(1));
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(*reads.borrow(), 2);
}

#[test]
fn refused_then_started() {
    let port = {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.local_addr().unwrap().port()
    };
    let mut relay = tcp(port);
    let out = send(&mut relay, initialize(1));
    assert_eq!(error_message(&out[0], json!(1)), NOT_RUNNING);
    assert!(send(&mut relay, initialized()).is_empty());
    let (_events, _handle) = server_on(port);
    let out = send(&mut relay, initialize(2));
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn the_tools_list_fits_the_body_bound() {
    let (_events, handle) = server();
    let mut relay = tcp(handle.address().port());
    send(&mut relay, initialize(1));
    send(&mut relay, initialized());
    let mut out = Vec::new();
    relay
        .handle(
            Line::Text(br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_vec()),
            &mut out,
        )
        .unwrap();
    // Well within: room left for a reply's text beside its image.
    assert!((out.len() as u64) < BODY_BYTES - (ToolReply::IMAGE_BYTES as u64) / 3 * 4 - 256 * 1024);
}

// Bugs found in review.

#[test]
fn odd_ids_are_answered_as_the_server_would() {
    let connect = fake();
    connect.0.borrow_mut().refuse = true;
    let mut relay = relay(connect);
    let line = |id: &str| format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#).into_bytes();
    let out = send(&mut relay, line(r#""a\"b""#));
    assert_eq!(error_message(&out[0], json!("a\"b")), NOT_RUNNING);
    let out = send(&mut relay, line("9223372036854775807"));
    assert_eq!(error_message(&out[0], json!(i64::MAX)), NOT_RUNNING);
    // Ids the server takes for none (past i64, fractions, objects, a
    // string past `Id::STRING`) are answered to null, as it answers
    // them, never rounded.
    let long = format!("\"{}\"", "x".repeat(crate::rpc::Id::STRING + 1));
    for id in ["123456789012345678901234567890", "1.5", "{}", "true", &long] {
        let out = send(&mut relay, line(id));
        assert_eq!(error_message(&out[0], Value::Null), NOT_RUNNING, "{id}");
    }
}

#[test]
fn a_notification_never_gets_a_reply() {
    let connect = FakeConnect::new(|_| {
        let reply = br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"x"}}"#;
        response("200 OK", &[], reply)
    });
    let mut relay = relay(connect.clone());
    assert!(send(&mut relay, initialized()).is_empty());
    let connect = FakeConnect::new(|_| {
        let reply = br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"x"}}"#;
        response("400 Bad Request", &[], reply)
    });
    let mut relay = super::tests::relay(connect);
    assert!(send(&mut relay, initialized()).is_empty());
}

#[test]
fn an_event_stream_is_refused_and_the_connection_dropped() {
    let calls = Rc::new(RefCell::new(0));
    let counted = calls.clone();
    let connect = FakeConnect::new(move |_| {
        *counted.borrow_mut() += 1;
        if *counted.borrow() == 1 {
            // No length: the stream runs until the server closes it.
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
              data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n"
                .to_vec()
        } else {
            response("200 OK", &[], br#"{"jsonrpc":"2.0","id":2,"result":{}}"#)
        }
    });
    let mut relay = relay(connect.clone());
    let out = send(&mut relay, ping(1));
    error_message(&out[0], json!(1));
    let out = send(&mut relay, ping(2));
    assert_eq!(out[0]["result"], json!({}));
    assert_eq!(connect.0.borrow().connections, 2);
    // With a length, still not JSON: refused.
    let connect = FakeConnect::new(|_| {
        response(
            "200 OK",
            &[("Content-Type", "text/event-stream")],
            br#"data: {"jsonrpc":"2.0","id":1,"result":{}}"#,
        )
    });
    let mut relay = super::tests::relay(connect);
    let out = send(&mut relay, ping(3));
    error_message(&out[0], json!(3));
}

#[test]
fn a_redirect_isnt_followed() {
    let connect = FakeConnect::new(|_| {
        response(
            "307 Temporary Redirect",
            &[("Location", "http://evil.example/mcp")],
            b"",
        )
    });
    let mut relay = relay(connect.clone());
    let out = send(&mut relay, ping(1));
    assert!(error_message(&out[0], json!(1)).contains("307"));
    assert_eq!(connect.requests().len(), 1);
}

#[test]
fn a_404_replay_doesnt_loop() {
    // Starts sessions but takes nothing else.
    let connect = FakeConnect::new(|request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        if body["method"] == "initialize" {
            let reply = json!({ "jsonrpc": "2.0", "id": body["id"], "result": {} });
            return response(
                "200 OK",
                &[("MCP-Session-Id", "s")],
                reply.to_string().as_bytes(),
            );
        }
        response("404 Not Found", &[], b"")
    });
    let mut relay = relay(connect.clone());
    send(&mut relay, initialize(1));
    relay.initialized = Some(initialized());
    let out = send(&mut relay, ping(2));
    assert!(error_message(&out[0], json!(2)).contains("404"));
    // The ping, initialize, initialized: then given up.
    assert_eq!(connect.requests().len(), 4);
}

struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_stdout_failure_still_ends_the_session() {
    let connect = fake();
    let mut input = initialize(1);
    input.extend(b"\r\n");
    input.extend(ping(2));
    let result = run_with(Cursor::new(input), Broken, relay(connect.clone()));
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    let requests = connect.requests();
    // The CR isn't forwarded; the ping isn't sent once stdout's gone.
    assert_eq!(requests[0].body, initialize(1));
    assert_eq!(requests.last().unwrap().method, "DELETE");
    assert_eq!(requests.len(), 2);
}

#[test]
fn a_failed_second_initialize_keeps_the_session() {
    let connect = fake();
    let mut relay = relay(connect.clone());
    send(&mut relay, initialize(1));
    // A server's own error to `initialize`, at 200, without a session.
    let failing = FakeConnect::new(|_| {
        let reply = br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"x"}}"#;
        response("200 OK", &[], reply)
    });
    let mut other = super::tests::relay(failing);
    other.session = relay.session.clone();
    other.version = relay.version.clone();
    other.initialize = relay.initialize.clone();
    let out = send(&mut other, initialize(2));
    assert_eq!(out[0]["error"]["code"], -32602);
    assert_eq!(other.session.as_deref(), Some("s1"));
    assert_eq!(other.version.as_deref(), Some("2025-06-18"));
    assert_eq!(other.initialize, Some(initialize(1)));
}

#[test]
fn a_session_id_on_a_later_answer_is_ignored() {
    let connect = FakeConnect::new(|request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        let reply = json!({ "jsonrpc": "2.0", "id": body["id"], "result": {} });
        let session = if body["method"] == "initialize" {
            "s1"
        } else {
            "other"
        };
        response(
            "200 OK",
            &[("MCP-Session-Id", session)],
            reply.to_string().as_bytes(),
        )
    });
    let mut relay = relay(connect.clone());
    send(&mut relay, initialize(1));
    send(&mut relay, ping(2));
    send(&mut relay, ping(3));
    assert_eq!(connect.requests()[2].header("mcp-session-id"), Some("s1"));
}
