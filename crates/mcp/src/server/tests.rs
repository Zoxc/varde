use std::io::{BufRead, Read, Write};
use std::net::Shutdown;

use futures::executor::{BlockingStream, block_on_stream};
use serde_json::{Value, json};

use super::*;

const TOKEN: [u8; Token::BYTES] = [7; Token::BYTES];

/// A server on a free port, its stream and its handle.
fn server(reply: Duration) -> (BlockingStream<Events>, Handle) {
    server_with(Timeouts {
        reply,
        ..Timeouts::default()
    })
}

fn server_with(timeouts: Timeouts) -> (BlockingStream<Events>, Handle) {
    let mut events = block_on_stream(start_with(0, Token::from_bytes(TOKEN), timeouts));
    match events.next() {
        Some(Event::Ready(handle)) => (events, handle),
        other => panic!("expected Ready, got {other:?}"),
    }
}

/// A response read off the wire.
struct Received {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        assert_eq!(self.status, 200);
        serde_json::from_slice(&self.body).unwrap()
    }
}

/// A raw HTTP client on one connection.
struct Client {
    port: u16,
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    token: String,
    session: Option<String>,
}

impl Client {
    fn new(address: SocketAddr) -> Self {
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        Self {
            port: address.port(),
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
            token: Token::from_bytes(TOKEN).to_string(),
            session: None,
        }
    }

    fn post(&mut self, body: Value) -> Received {
        let body = serde_json::to_vec(&body).unwrap();
        let mut head = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n",
            self.port,
            self.token,
            body.len()
        );
        if let Some(session) = &self.session {
            head.push_str(&format!("MCP-Session-Id: {session}\r\n"));
        }
        head.push_str("\r\n");
        self.writer.write_all(head.as_bytes()).unwrap();
        self.writer.write_all(&body).unwrap();
        self.read()
    }

    fn read(&mut self) -> Received {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        let status = line.split(' ').nth(1).unwrap().parse().unwrap();
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let (name, value) = line.split_once(':').unwrap();
            headers.push((name.to_owned(), value.trim().to_owned()));
        }
        let mut reply = Received {
            status,
            headers,
            body: Vec::new(),
        };
        let length: usize = reply
            .header("content-length")
            .map_or(0, |l| l.parse().unwrap());
        reply.body = vec![0; length];
        self.reader.read_exact(&mut reply.body).unwrap();
        reply
    }

    /// `initialize` and `notifications/initialized`.
    fn open(&mut self) {
        let reply = self.post(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "test" } },
        }));
        assert_eq!(reply.json()["result"]["protocolVersion"], "2025-06-18");
        self.session = Some(reply.header("mcp-session-id").unwrap().to_owned());
        let reply = self.post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        assert_eq!(reply.status, 202);
    }

    fn call(&mut self, id: i64) -> Received {
        self.post(json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "design_summary", "arguments": {} },
        }))
    }
}

/// The text of a `tools/call` result, and whether it's an error.
fn tool_text(reply: &Received) -> (String, bool) {
    let result = &reply.json()["result"];
    (
        result["content"][0]["text"].as_str().unwrap().to_owned(),
        result["isError"].as_bool().unwrap(),
    )
}

fn started(events: &mut BlockingStream<Events>) -> SessionId {
    match events.next() {
        Some(Event::Session(session::Event::Started { session, client })) => {
            assert_eq!(client.as_deref(), Some("test"));
            session
        }
        other => panic!("expected Started, got {other:?}"),
    }
}

#[test]
fn binds_a_free_port() {
    let (_events, handle) = server(REPLY);
    assert!(handle.address().ip().is_loopback());
    assert_ne!(handle.address().port(), 0);
}

#[test]
fn answers_a_session_and_a_call() {
    let (mut events, handle) = server(REPLY);
    let address = handle.address();
    let client = thread::spawn(move || {
        let mut client = Client::new(address);
        client.open();
        let list = client.post(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
        let names: Vec<_> = list.json()["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        assert!(names.contains(&"design_summary".to_owned()));
        tool_text(&client.call(3))
    });
    let session = started(&mut events);
    match events.next() {
        Some(Event::Call {
            session: s,
            id,
            call,
            reply,
        }) => {
            assert_eq!(s, session);
            assert_eq!(id, Id::Number(3));
            assert_eq!(call, Call::DesignSummary(crate::tools::Page::SUMMARY));
            // A clone answers as well as the original; the second is
            // ignored.
            reply.clone().send(ToolReply::text("one box"));
            reply.send(ToolReply::text("ignored"));
        }
        other => panic!("expected a call, got {other:?}"),
    }
    assert_eq!(client.join().unwrap(), ("one box".to_owned(), false));
}

#[test]
fn a_dropped_reply_says_closing() {
    let (mut events, handle) = server(REPLY);
    let address = handle.address();
    let client = thread::spawn(move || {
        let mut client = Client::new(address);
        client.open();
        tool_text(&client.call(2))
    });
    started(&mut events);
    match events.next() {
        Some(Event::Call { reply, .. }) => drop(reply),
        other => panic!("expected a call, got {other:?}"),
    }
    assert_eq!(client.join().unwrap(), (CLOSING.to_owned(), true));
}

#[test]
fn an_unanswered_call_times_out() {
    let (mut events, handle) = server(Duration::from_millis(100));
    let address = handle.address();
    let client = thread::spawn(move || {
        let mut client = Client::new(address);
        client.open();
        let first = tool_text(&client.call(2));
        // The call finished: the session takes another.
        let ping = client.post(json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }));
        assert_eq!(ping.json()["result"], json!({}));
        first
    });
    started(&mut events);
    let Some(Event::Call { reply, .. }) = events.next() else {
        panic!("expected a call");
    };
    assert_eq!(client.join().unwrap(), (TIMED_OUT.to_owned(), true));
    // Too late: nothing to answer, and the app's told it wasn't taken.
    assert!(!reply.send(ToolReply::text("late")));
}

#[test]
fn a_wrong_token_gets_401() {
    let (_events, handle) = server(REPLY);
    let mut client = Client::new(handle.address());
    client.token = Token::from_bytes([8; Token::BYTES]).to_string();
    let reply = client.post(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }));
    assert_eq!(reply.status, 401);
    assert_eq!(reply.header("www-authenticate"), Some("Bearer"));
}

#[test]
fn a_malformed_request_is_answered_and_closed() {
    let (_events, handle) = server(REPLY);
    let mut client = Client::new(handle.address());
    client
        .writer
        .write_all(b"POST /mcp HTTP/1.1\r\n\r\n")
        .unwrap();
    let reply = client.read();
    assert_eq!(reply.status, 411);
    assert_eq!(reply.header("connection"), Some("close"));
    let mut rest = Vec::new();
    client.reader.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
}

#[test]
fn ending_the_session_is_an_event() {
    let (mut events, handle) = server(REPLY);
    let mut client = Client::new(handle.address());
    client.open();
    let session = started(&mut events);
    handle.end_session();
    match events.next() {
        Some(Event::Session(session::Event::Ended { session: s, end })) => {
            assert_eq!(s, session);
            assert_eq!(end, session::End::Disconnected);
        }
        other => panic!("expected Ended, got {other:?}"),
    }
    let reply = client.post(json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }));
    assert_eq!(reply.status, 404);
}

#[test]
fn dropping_the_stream_stops_the_server() {
    let (events, handle) = server(REPLY);
    let address = handle.address();
    drop(events);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match TcpStream::connect(address) {
            Err(_) => break,
            Ok(stream) => {
                let _ = stream.shutdown(Shutdown::Both);
                assert!(Instant::now() < deadline, "still listening");
                thread::sleep(POLL);
            }
        }
    }
}

#[test]
fn a_taken_port_fails_to_bind() {
    let (_events, handle) = server(REPLY);
    let mut second = block_on_stream(start(handle.address().port(), Token::from_bytes(TOKEN)));
    assert!(matches!(second.next(), Some(Event::BindFailed(_))));
    assert!(second.next().is_none());
}

#[test]
fn a_cancel_names_the_waiting_call() {
    let (mut events, handle) = server(REPLY);
    let address = handle.address();
    let mut client = Client::new(address);
    client.open();
    let session = started(&mut events);
    let waiting = thread::spawn(move || tool_text(&client.call(2)));
    let Some(Event::Call { id, reply, .. }) = events.next() else {
        panic!("expected a call");
    };
    // The cancel comes on another connection while the call waits.
    let mut canceller = Client::new(address);
    canceller.session = Some(session.as_str().to_owned());
    let accepted = canceller.post(json!({
        "jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": { "requestId": 2 },
    }));
    assert_eq!(accepted.status, 202);
    match events.next() {
        Some(Event::Session(session::Event::Cancelled {
            session: s,
            request,
        })) => {
            assert_eq!(s, session);
            assert_eq!(request, id);
        }
        other => panic!("expected Cancelled, got {other:?}"),
    }
    // The app answers the cancelled call itself.
    reply.send(ToolReply::error("cancelled"));
    assert_eq!(waiting.join().unwrap(), ("cancelled".to_owned(), true));
}

#[test]
fn a_trickled_request_is_cut_off() {
    let (_events, handle) = server_with(Timeouts {
        head: Duration::from_millis(300),
        ..Timeouts::default()
    });
    let mut client = Client::new(handle.address());
    // A byte every 100 ms, each well inside the read timeout, for 2 s:
    // the request as a whole isn't.
    let mut writer = client.writer.try_clone().unwrap();
    let trickler = thread::spawn(move || {
        for _ in 0..20 {
            if writer.write_all(b"P").is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let begun = Instant::now();
    let mut rest = Vec::new();
    let _ = client.reader.read_to_end(&mut rest);
    let took = begun.elapsed();
    assert!(took < Duration::from_millis(1500), "held for {took:?}");
    trickler.join().unwrap();
}

#[test]
fn a_trickled_head_is_cut_off_at_the_head_timeout() {
    // The read timeout long, the head's short: a head trickled a byte
    // every 100 ms, each read well inside both, is cut off by the head's.
    let (_events, handle) = server_with(Timeouts {
        head: Duration::from_millis(400),
        ..Timeouts::default()
    });
    let mut client = Client::new(handle.address());
    let mut writer = client.writer.try_clone().unwrap();
    let trickler = thread::spawn(move || {
        for byte in b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:1\r\nX-Slow: aaaaaaaaaaaaaaaa" {
            if writer.write_all(&[*byte]).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let begun = Instant::now();
    let mut rest = Vec::new();
    let _ = client.reader.read_to_end(&mut rest);
    let took = begun.elapsed();
    assert!(took < Duration::from_millis(2000), "held for {took:?}");
    trickler.join().unwrap();
}

#[test]
fn an_idle_untrusted_connection_is_closed() {
    let (_events, handle) = server_with(Timeouts {
        head: Duration::from_millis(300),
        ..Timeouts::default()
    });
    let mut client = Client::new(handle.address());
    let begun = Instant::now();
    let mut rest = Vec::new();
    let _ = client.reader.read_to_end(&mut rest);
    assert!(rest.is_empty());
    let took = begun.elapsed();
    assert!(took < Duration::from_millis(2000), "held for {took:?}");
}

#[test]
fn a_trusted_connection_may_idle_past_the_head_timeout() {
    let (_events, handle) = server_with(Timeouts {
        head: Duration::from_millis(200),
        ..Timeouts::default()
    });
    let mut client = Client::new(handle.address());
    client.open();
    thread::sleep(Duration::from_millis(500));
    let reply = client.post(json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }));
    assert_eq!(reply.status, 200);
}

#[test]
fn a_bad_token_is_refused_before_the_body() {
    let (_events, handle) = server(REPLY);
    let mut client = Client::new(handle.address());
    // A large body declared, none of it sent: the 401 comes anyway.
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer wrong\r\n\
         Content-Length: {}\r\n\r\n",
        handle.address().port(),
        http::Limits::BODY_BYTES
    );
    client.writer.write_all(head.as_bytes()).unwrap();
    let reply = client.read();
    assert_eq!(reply.status, 401);
    assert_eq!(reply.header("connection"), Some("close"));
}

#[test]
fn restarts_on_the_same_port_at_once() {
    let (events, handle) = server(REPLY);
    let port = handle.address().port();
    drop(handle);
    drop(events);
    for _ in 0..20 {
        let mut events = block_on_stream(start(port, Token::from_bytes(TOKEN)));
        match events.next() {
            Some(Event::Ready(handle)) => assert_eq!(handle.address().port(), port),
            other => panic!("expected Ready, got {other:?}"),
        }
    }
}

#[test]
fn cancels_for_no_waiting_call_send_nothing() {
    let (mut events, handle) = server(REPLY);
    let address = handle.address();
    let mut client = Client::new(address);
    client.open();
    let session = started(&mut events);
    let waiting = thread::spawn(move || tool_text(&client.call(2)));
    let Some(Event::Call { reply, .. }) = events.next() else {
        panic!("expected a call");
    };
    let mut canceller = Client::new(address);
    canceller.session = Some(session.as_str().to_owned());
    // Many cancels: for ids never sent, and the waiting call's again
    // and again. Only the first for the waiting call is passed on.
    for request in [5, 2, 6, 2, 2, 7] {
        let accepted = canceller.post(json!({
            "jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": { "requestId": request },
        }));
        assert_eq!(accepted.status, 202);
    }
    reply.send(ToolReply::text("done"));
    waiting.join().unwrap();
    handle.end_session();
    let mut seen = Vec::new();
    for event in events.by_ref() {
        match event {
            Event::Session(session::Event::Ended { .. }) => break,
            other => seen.push(other),
        }
    }
    assert!(
        matches!(
            seen.as_slice(),
            [Event::Session(session::Event::Cancelled {
                request: Id::Number(2),
                ..
            })]
        ),
        "{seen:?}"
    );
}
