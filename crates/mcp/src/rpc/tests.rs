use serde_json::json;

use super::*;

fn read(body: &str) -> Result<Message, Error> {
    parse(body.as_bytes(), &Limits::default())
}

fn code(body: &str) -> Code {
    read(body).unwrap_err().code
}

#[test]
fn reads_a_request() {
    let message = read(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap();
    assert_eq!(
        message,
        Message::Request(Request {
            id: Id::Number(1),
            method: "ping".into(),
            params: None,
        })
    );
    let message =
        read(r#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"x":[1]}}"#).unwrap();
    assert_eq!(
        message,
        Message::Request(Request {
            id: Id::String("a".into()),
            method: "tools/call".into(),
            params: Some(json!({"x": [1]})),
        })
    );
}

#[test]
fn reads_a_notification() {
    let message = read(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
    assert_eq!(
        message,
        Message::Notification(Notification {
            method: "notifications/initialized".into(),
            params: None,
        })
    );
}

#[test]
fn reads_a_response() {
    for body in [
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-1,"message":"x"}}"#,
    ] {
        assert_eq!(read(body), Ok(Message::Response), "{body}");
    }
    for body in [
        r#"{"jsonrpc":"2.0","result":{}}"#,
        r#"{"jsonrpc":"2.0","id":1}"#,
        r#"{"jsonrpc":"2.0","id":1,"result":1,"error":{}}"#,
        r#"{"jsonrpc":"2.0","id":1,"error":3}"#,
    ] {
        assert_eq!(code(body), Code::InvalidRequest, "{body}");
    }
}

#[test]
fn not_json_is_a_parse_error() {
    for body in ["", "{", "nul", r#"{"a":1,}"#, "\u{ff}"] {
        assert_eq!(code(body), Code::Parse, "{body}");
    }
    assert_eq!(read("{").unwrap_err().id, None);
}

#[test]
fn batches_are_refused() {
    assert_eq!(
        code(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#),
        Code::InvalidRequest
    );
    assert_eq!(code("[]"), Code::InvalidRequest);
}

#[test]
fn other_json_is_refused() {
    for body in ["1", "\"x\"", "null", "true", "{}"] {
        assert_eq!(code(body), Code::InvalidRequest, "{body}");
    }
}

#[test]
fn jsonrpc_must_be_2_0() {
    for body in [
        r#"{"id":1,"method":"ping"}"#,
        r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
        r#"{"jsonrpc":2.0,"id":1,"method":"ping"}"#,
    ] {
        let error = read(body).unwrap_err();
        assert_eq!(error.code, Code::InvalidRequest, "{body}");
        // The id was readable, so the error goes to it.
        assert_eq!(error.id, Some(Id::Number(1)), "{body}");
    }
}

#[test]
fn ids_are_integers_or_short_strings() {
    let long = "x".repeat(Id::STRING);
    let ok = format!(r#"{{"jsonrpc":"2.0","id":"{long}","method":"ping"}}"#);
    assert!(matches!(read(&ok), Ok(Message::Request(_))));
    let id = |body: &str| match read(body).unwrap() {
        Message::Request(r) => r.id,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        id(r#"{"jsonrpc":"2.0","id":-9223372036854775808,"method":"m"}"#),
        Id::Number(i64::MIN)
    );
    assert_eq!(
        id(r#"{"jsonrpc":"2.0","id":0,"method":"m"}"#),
        Id::Number(0)
    );
    let too_long = format!(r#"{{"jsonrpc":"2.0","id":"{long}x","method":"ping"}}"#);
    for body in [
        too_long.as_str(),
        r#"{"jsonrpc":"2.0","id":1.5,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1.0,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":1e3,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":9223372036854775808,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":true,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":[1],"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":{},"method":"ping"}"#,
    ] {
        let error = read(body).unwrap_err();
        assert_eq!(error.code, Code::InvalidRequest, "{body}");
        assert_eq!(error.id, None, "{body}");
    }
}

#[test]
fn method_and_params_are_checked() {
    for body in [
        r#"{"jsonrpc":"2.0","id":1,"method":1}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":null}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"m","params":1}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"m","params":"x"}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"m","params":null}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"m","result":{}}"#,
    ] {
        let error = read(body).unwrap_err();
        assert_eq!(error.code, Code::InvalidRequest, "{body}");
        assert_eq!(error.id, Some(Id::Number(1)), "{body}");
    }
    let message = read(r#"{"jsonrpc":"2.0","method":"m","params":[1,2]}"#).unwrap();
    assert!(matches!(
        message,
        Message::Notification(Notification {
            params: Some(Value::Array(_)),
            ..
        })
    ));
}

#[test]
fn depth_is_bounded() {
    let nest = |depth: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"m","params":{}{}}}"#,
            "[".repeat(depth - 1),
            "]".repeat(depth - 1)
        )
    };
    assert!(read(&nest(Limits::DEPTH)).is_ok());
    assert_eq!(code(&nest(Limits::DEPTH + 1)), Code::InvalidRequest);
    // Far past serde_json's own limit, and never recursed into.
    let deep = "[".repeat(1_000_000);
    assert_eq!(code(&deep), Code::InvalidRequest);
    let deep = "{\"a\":".repeat(100_000);
    assert_eq!(code(&deep), Code::InvalidRequest);
}

#[test]
fn brackets_in_strings_are_not_nesting() {
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"m","params":{{"s":"{}\"\\"}}}}"#,
        "[{".repeat(1000)
    );
    assert!(read(&body).is_ok(), "{body}");
}

#[test]
fn sizes_are_bounded() {
    let limits = Limits {
        depth: 8,
        string_bytes: 7,
        array: 3,
        keys: 6,
    };
    let read = |params: &str| {
        let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"m","params":{params}}}"#);
        parse(body.as_bytes(), &limits)
    };
    for params in [
        r#"["abcdefg"]"#,
        "[1,2,3]",
        r#"{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6}"#,
        r#"{"abcdefg":1}"#,
    ] {
        assert!(read(params).is_ok(), "{params}");
    }
    for params in [
        r#"["abcdefgh"]"#,
        r#"[[["abcdefgh"]]]"#,
        r#"["\u0041\u0041\u0041\u0041\u0041\u0041\u0041\u0041"]"#,
        "[1,2,3,4]",
        "[[1,2,3,4]]",
        r#"{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7}"#,
        r#"{"abcdefgh":1}"#,
        r#"{"x":{"abcdefgh":1}}"#,
    ] {
        assert_eq!(
            read(params).unwrap_err().code,
            Code::InvalidRequest,
            "{params}"
        );
    }
    // The method is a string like any other.
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"abcdefgh"}"#;
    assert_eq!(
        parse(body.as_bytes(), &limits).unwrap_err().code,
        Code::InvalidRequest
    );
}

#[test]
fn writes_replies() {
    let reply = success(&Id::Number(7), json!({"ok": true}));
    let value: Value = serde_json::from_slice(&reply).unwrap();
    assert_eq!(
        value,
        json!({"jsonrpc": "2.0", "id": 7, "result": {"ok": true}})
    );

    let reply = success(&Id::String("a".into()), json!(null));
    let value: Value = serde_json::from_slice(&reply).unwrap();
    assert_eq!(value, json!({"jsonrpc": "2.0", "id": "a", "result": null}));

    for (c, n) in [
        (Code::Parse, -32700),
        (Code::InvalidRequest, -32600),
        (Code::MethodNotFound, -32601),
        (Code::InvalidParams, -32602),
        (Code::Internal, -32603),
    ] {
        let reply = error(Some(&Id::Number(1)), c, "no");
        let value: Value = serde_json::from_slice(&reply).unwrap();
        assert_eq!(
            value,
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": n, "message": "no"}})
        );
    }
    let value: Value = serde_json::from_slice(&read("x").unwrap_err().reply()).unwrap();
    assert_eq!(
        value,
        json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}})
    );
}
