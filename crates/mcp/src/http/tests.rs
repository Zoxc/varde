use std::io::{BufReader, Cursor};

use super::*;

fn read(bytes: &[u8]) -> Result<Option<Request>, Error> {
    read_request(&mut Cursor::new(bytes), &Limits::default())
}

fn status(bytes: &[u8]) -> Option<Status> {
    read(bytes).unwrap_err().status()
}

fn post(body: &str) -> String {
    format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn reads_a_post() {
    let request = read(post("{}").as_bytes()).unwrap().unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/mcp");
    assert_eq!(request.body, b"{}");
    assert_eq!(request.header("HOST"), Some("127.0.0.1"));
    assert_eq!(request.header("content-length"), Some("2"));
    assert_eq!(request.header("Origin"), None);
    assert!(!request.close);
}

#[test]
fn once_refuses_a_repeated_header() {
    let request =
        read(b"POST /mcp HTTP/1.1\r\nHost: a\r\nX: 1\r\nx: 2\r\nContent-Length: 0\r\n\r\n")
            .unwrap()
            .unwrap();
    assert_eq!(request.headers.once("host"), Ok(Some("a")));
    assert_eq!(request.headers.once("origin"), Ok(None));
    assert_eq!(request.headers.once("x"), Err(Repeated));
}

#[test]
fn nothing_sent_is_none() {
    assert!(read(b"").unwrap().is_none());
}

#[test]
fn ending_within_a_request_is_io() {
    assert!(matches!(read(b"POST /mcp HTTP/1.1\r\n"), Err(Error::Io(_))));
    let mut cut = post("{\"a\":1}");
    cut.truncate(cut.len() - 2);
    assert!(matches!(read(cut.as_bytes()), Err(Error::Io(_))));
}

#[test]
fn two_requests_back_to_back() {
    let bytes = format!(
        "{}{}GET /mcp HTTP/1.1\r\nConnection: close\r\n\r\n",
        post("[1]"),
        post("[22]")
    );
    // A small buffer, so heads and bodies straddle refills.
    let mut reader = BufReader::with_capacity(5, Cursor::new(bytes.into_bytes()));
    let limits = Limits::default();
    let first = read_request(&mut reader, &limits).unwrap().unwrap();
    let second = read_request(&mut reader, &limits).unwrap().unwrap();
    let third = read_request(&mut reader, &limits).unwrap().unwrap();
    assert_eq!(first.body, b"[1]");
    assert_eq!(second.body, b"[22]");
    assert_eq!(third.method, "GET");
    assert!(third.body.is_empty());
    assert!(!first.close && !second.close && third.close);
    assert!(read_request(&mut reader, &limits).unwrap().is_none());
}

#[test]
fn connection_close() {
    let close = |head: &str| read(head.as_bytes()).unwrap().unwrap().close;
    assert!(close(
        "GET / HTTP/1.1\r\nConnection: keep-alive, Close\r\n\r\n"
    ));
    assert!(!close("GET / HTTP/1.1\r\n\r\n"));
    assert!(close("GET / HTTP/1.0\r\n\r\n"));
    assert!(!close("GET / HTTP/1.0\r\nConnection: keep-alive\r\n\r\n"));
}

#[test]
fn url_limit() {
    let limits = Limits::default();
    let at = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(limits.url - 1));
    assert!(read(at.as_bytes()).is_ok());
    let over = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(limits.url));
    assert!(matches!(read(over.as_bytes()), Err(Error::UrlTooLong)));
    assert_eq!(status(over.as_bytes()), Some(Status::BadRequest));
}

#[test]
fn header_count_limit() {
    let head = |count: usize| {
        let mut head = String::from("GET / HTTP/1.1\r\n");
        for i in 0..count {
            head.push_str(&format!("X-{i}: x\r\n"));
        }
        head + "\r\n"
    };
    assert!(read(head(Limits::HEADERS).as_bytes()).is_ok());
    assert_eq!(
        status(head(Limits::HEADERS + 1).as_bytes()),
        Some(Status::HeadersTooLarge)
    );
}

#[test]
fn head_bytes_limit() {
    let limits = Limits {
        head_bytes: 64,
        ..Limits::default()
    };
    let head = |pad: usize| format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(pad));
    // 16 + 3 + 2 + 2 bytes around the padding.
    let fits = head(64 - 23);
    assert_eq!(fits.len(), 64);
    assert!(read_request(&mut Cursor::new(fits), &limits).is_ok());
    // Endless, so only the bound stops it.
    let endless = format!("GET / HTTP/1.1\r\nX: {}", "a".repeat(1 << 20));
    let error = read_request(&mut Cursor::new(endless), &limits).unwrap_err();
    assert_eq!(error.status(), Some(Status::HeadersTooLarge));
    let error = read_request(&mut Cursor::new(head(64 - 22)), &limits).unwrap_err();
    assert!(matches!(error, Error::HeadTooLarge));
}

#[test]
fn chunked_is_refused() {
    let bytes = b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n";
    assert_eq!(status(bytes), Some(Status::LengthRequired));
}

#[test]
fn post_needs_a_length() {
    assert_eq!(
        status(b"POST /mcp HTTP/1.1\r\n\r\n{}"),
        Some(Status::LengthRequired)
    );
    // Other methods don't.
    let request = read(b"DELETE /mcp HTTP/1.1\r\n\r\n").unwrap().unwrap();
    assert!(request.body.is_empty());
}

#[test]
fn body_limit() {
    let limits = Limits {
        body_bytes: 4,
        ..Limits::default()
    };
    let read = |body: &str| read_request(&mut Cursor::new(post(body)), &limits);
    assert_eq!(read("1234").unwrap().unwrap().body, b"1234");
    assert_eq!(
        read("12345").unwrap_err().status(),
        Some(Status::PayloadTooLarge)
    );
    // Refused on the length alone, nothing allocated for it.
    let huge = format!(
        "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        Limits::BODY_BYTES + 1
    );
    assert_eq!(status(huge.as_bytes()), Some(Status::PayloadTooLarge));
    let overflowing = "POST / HTTP/1.1\r\nContent-Length: 99999999999999999999999\r\n\r\n";
    assert_eq!(
        status(overflowing.as_bytes()),
        Some(Status::PayloadTooLarge)
    );
}

#[test]
fn malformed() {
    let bad = |bytes: &[u8]| assert_eq!(status(bytes), Some(Status::BadRequest), "{bytes:?}");
    bad(b"nonsense\r\n\r\n");
    bad(b"GET / HTTP/2.0\r\n\r\n");
    bad(b"GET / HTTP/1.1\r\nNo colon\r\n\r\n");
    bad(b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n");
    bad(b"POST / HTTP/1.1\r\nContent-Length: +1\r\n\r\nx");
    bad(b"POST / HTTP/1.1\r\nContent-Length:\r\n\r\n");
    bad(b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nxx");
    bad(b"GET / HTTP/1.1\r\nX: \xff\r\n\r\n");
}

#[test]
fn repeated_equal_lengths_are_one() {
    let bytes = b"POST / HTTP/1.1\r\nContent-Length: 1\r\ncontent-length: 1\r\n\r\nx";
    assert_eq!(read(bytes).unwrap().unwrap().body, b"x");
}

fn written(response: &Response, close: bool) -> String {
    let mut out = Vec::new();
    write_response(&mut out, response, close).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn writes_json() {
    let response = Response::json(b"{}".to_vec()).with_header("MCP-Session-Id", "abc");
    assert_eq!(
        written(&response, false),
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\
         MCP-Session-Id: abc\r\n\r\n{}"
    );
}

#[test]
fn writes_empty_responses() {
    assert_eq!(
        written(&Response::empty(Status::Accepted), false),
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n"
    );
    assert_eq!(
        written(&Response::empty(Status::NoContent), true),
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"
    );
    let unauthorized =
        Response::empty(Status::Unauthorized).with_header("WWW-Authenticate", "Bearer");
    assert_eq!(
        written(&unauthorized, false),
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nWWW-Authenticate: Bearer\r\n\r\n"
    );
}

#[test]
fn every_status_has_a_line() {
    use Status::*;
    let all = [
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
    ];
    let codes = all.map(Status::code);
    assert_eq!(
        codes,
        [200, 202, 204, 400, 401, 403, 404, 405, 411, 413, 431]
    );
    for status in all {
        let line = written(&Response::empty(status), false);
        assert!(line.starts_with(&format!(
            "HTTP/1.1 {} {}\r\n",
            status.code(),
            status.reason()
        )));
    }
}

#[test]
fn line_breaks_in_headers_are_refused() {
    let response = Response::empty(Status::Ok).with_header("MCP-Session-Id", "a\r\nX: y");
    let mut out = Vec::new();
    assert!(write_response(&mut out, &response, false).is_err());
    assert!(out.is_empty());
}

#[test]
fn length_with_unicode_space_is_malformed() {
    // `str::trim` would take these off; only digits make a length.
    let bad = |bytes: &[u8]| assert_eq!(status(bytes), Some(Status::BadRequest), "{bytes:?}");
    bad("POST / HTTP/1.1\r\nContent-Length: 1\u{2003}\r\n\r\nx".as_bytes());
    bad("POST / HTTP/1.1\r\nContent-Length: \u{a0}1\r\n\r\nx".as_bytes());
}
