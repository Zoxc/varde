use super::*;

const PORT: u16 = 7311;

fn token() -> Token {
    let mut bytes = [0; Token::BYTES];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(37).wrapping_add(250);
    }
    Token::from_bytes(bytes)
}

fn guard() -> Guard {
    Guard {
        port: PORT,
        token: token(),
    }
}

fn request(method: &str, path: &str, headers: &[(&str, &str)]) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).into(), (*v).into()))
            .collect(),
        body: Vec::new(),
        close: false,
    }
}

fn bearer() -> String {
    format!("Bearer {}", token())
}

/// A request that passes, with `extra` added and the headers named in
/// `without` left out.
fn check(extra: &[(&str, &str)], without: &[&str]) -> Result<(), Refusal> {
    let bearer = bearer();
    let mut headers = vec![
        ("Host", "127.0.0.1:7311"),
        ("Authorization", bearer.as_str()),
    ];
    headers.retain(|(n, _)| !without.contains(n));
    headers.extend_from_slice(extra);
    guard().check(&request("POST", "/mcp", &headers))
}

#[test]
fn passes_a_good_request() {
    assert_eq!(check(&[], &[]), Ok(()));
    assert_eq!(check(&[("Origin", "http://127.0.0.1:7311")], &[]), Ok(()));
}

#[test]
fn host_is_required() {
    assert_eq!(check(&[], &["Host"]), Err(Refusal::NoHost));
    assert_eq!(Refusal::NoHost.response().status, Status::BadRequest);
}

#[test]
fn host_must_be_loopback_with_the_port() {
    for host in [
        "127.0.0.1:7311",
        "localhost:7311",
        "LocalHost:7311",
        "[::1]:7311",
    ] {
        assert_eq!(check(&[("Host", host)], &["Host"]), Ok(()), "{host}");
    }
    for host in [
        "127.0.0.1",
        "localhost",
        "[::1]",
        "127.0.0.1:7312",
        "127.0.0.1:07311",
        "127.0.0.1:+7311",
        "127.0.0.1:",
        "127.0.0.1:99999999999999999999",
        "evil.example:7311",
        "localhost.evil.example:7311",
        "127.0.0.2:7311",
        "::1:7311",
        "[::1]x:7311",
        "0.0.0.0:7311",
        "",
    ] {
        assert_eq!(
            check(&[("Host", host)], &["Host"]),
            Err(Refusal::Host),
            "{host}"
        );
    }
    assert_eq!(Refusal::Host.response().status, Status::Forbidden);
}

#[test]
fn origin_if_sent_must_be_loopback_http() {
    for origin in [
        "http://127.0.0.1:7311",
        "http://localhost:7311",
        "HTTP://[::1]:7311",
    ] {
        assert_eq!(check(&[("Origin", origin)], &[]), Ok(()), "{origin}");
    }
    for origin in [
        "null",
        "",
        "http://127.0.0.1",
        "http://127.0.0.1:80",
        "https://127.0.0.1:7311",
        "http://127.0.0.1:7311/",
        "http://evil.example:7311",
        "http://localhost.evil.example",
        "file://",
        "http:/",
    ] {
        assert_eq!(
            check(&[("Origin", origin)], &[]),
            Err(Refusal::Origin),
            "{origin}"
        );
    }
    assert_eq!(Refusal::Origin.response().status, Status::Forbidden);
}

#[test]
fn host_and_origin_come_before_the_token() {
    assert_eq!(
        check(&[("Host", "evil:1")], &["Host", "Authorization"]),
        Err(Refusal::Host)
    );
    assert_eq!(
        check(&[("Origin", "null")], &["Authorization"]),
        Err(Refusal::Origin)
    );
}

#[test]
fn other_paths_are_404_before_the_token() {
    let headers = [("Host", "127.0.0.1:7311")];
    for path in [
        "/",
        "/mcp/",
        "/MCP",
        "/.well-known/oauth-authorization-server",
        "*",
    ] {
        let refused = guard().check(&request("GET", path, &headers));
        assert_eq!(refused, Err(Refusal::Path), "{path}");
    }
    assert_eq!(Refusal::Path.response().status, Status::NotFound);
}

#[test]
fn options_is_405_before_the_token() {
    let headers = [("Host", "127.0.0.1:7311")];
    let refused = guard().check(&request("OPTIONS", "/mcp", &headers));
    assert_eq!(refused, Err(Refusal::Options));
    assert_eq!(Refusal::Options.response().status, Status::MethodNotAllowed);
}

#[test]
fn the_token_is_required() {
    assert_eq!(check(&[], &["Authorization"]), Err(Refusal::Token));
    let response = Refusal::Token.response();
    assert_eq!(response.status, Status::Unauthorized);
    assert_eq!(
        response.headers,
        vec![("WWW-Authenticate", "Bearer".to_owned())]
    );
}

#[test]
fn the_token_must_match() {
    let good = token().to_string();
    assert_eq!(
        check(
            &[("Authorization", &format!("bearer {good}"))],
            &["Authorization"]
        ),
        Ok(())
    );
    assert_eq!(
        check(
            &[("Authorization", &format!("BEARER  {good}"))],
            &["Authorization"]
        ),
        Ok(())
    );
    let mut wrong_last = good.clone().into_bytes();
    wrong_last[42] = if wrong_last[42] == b'A' { b'E' } else { b'A' };
    let wrong_last = String::from_utf8(wrong_last).unwrap();
    let other = Token::from_bytes([7; 32]).to_string();
    for value in [
        String::new(),
        "Bearer".to_owned(),
        "Bearer ".to_owned(),
        good.clone(),
        format!("Basic {good}"),
        format!("Bearer {other}"),
        format!("Bearer {wrong_last}"),
        format!("Bearer {}", &good[..42]),
        format!("Bearer {good}A"),
        format!("Bearer {good}{}", "A".repeat(10_000)),
        format!("Bearer {good} x"),
    ] {
        assert_eq!(
            check(&[("Authorization", &value)], &["Authorization"]),
            Err(Refusal::Token),
            "{value}"
        );
    }
}

#[test]
fn token_text_round_trips() {
    let token = token();
    let text = token.to_string();
    assert_eq!(text.len(), Token::TEXT);
    assert!(
        text.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    assert_eq!(text.parse::<Token>(), Ok(token));
    assert_eq!(
        Token::from_bytes([0; 32]).to_string(),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );
    assert_eq!(
        Token::from_bytes([0xff; 32]).to_string(),
        "__________________________________________8"
    );
}

#[test]
fn token_text_is_checked() {
    let good = token().to_string();
    for text in [
        String::new(),
        good[..42].to_owned(),
        format!("{good}A"),
        format!("{}=", &good[..42]),
        format!("{}+", &good[..42]),
        format!("{}/", &good[..42]),
        // The last character's two spare bits set.
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB".to_owned(),
        "__________________________________________".to_owned() + "_",
    ] {
        assert_eq!(text.parse::<Token>(), Err(BadToken), "{text}");
    }
}

#[test]
fn tokens_are_random() {
    let a = Token::generate().unwrap();
    let b = Token::generate().unwrap();
    assert_ne!(a, b);
    assert_eq!(format!("{a:?}"), "Token(..)");
}

#[test]
fn repeated_headers_are_refused() {
    let evil = [("Host", "evil.example:7311")];
    assert_eq!(check(&evil, &[]), Err(Refusal::Host));
    let origins = [
        ("Origin", "http://127.0.0.1:7311"),
        ("Origin", "http://evil.example"),
    ];
    assert_eq!(check(&origins, &[]), Err(Refusal::Origin));
    let bearer = bearer();
    let twice = [("authorization", bearer.as_str())];
    assert_eq!(check(&twice, &[]), Err(Refusal::Token));
}
