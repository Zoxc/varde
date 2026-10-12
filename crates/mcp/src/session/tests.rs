use super::*;

const T0: Duration = Duration::from_secs(1000);

fn request(method: &str, headers: &[(&str, &str)], body: Value) -> Request {
    Request {
        method: method.into(),
        path: "/mcp".into(),
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).into(), (*v).into()))
            .collect(),
        body: if body.is_null() {
            Vec::new()
        } else {
            serde_json::to_vec(&body).unwrap()
        },
        close: false,
    }
}

fn response(outcome: &Outcome) -> &Response {
    match &outcome.answer {
        Answer::Respond(r) => r,
        Answer::Dispatch(d) => panic!("expected a response, got {d:?}"),
    }
}

fn body(outcome: &Outcome) -> Value {
    let r = response(outcome);
    assert_eq!(r.status, Status::Ok);
    serde_json::from_slice(&r.body).unwrap()
}

fn error_code(outcome: &Outcome) -> i64 {
    body(outcome)["error"]["code"].as_i64().unwrap()
}

fn header<'a>(r: &'a Response, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn initialize(state: &mut State, version: &str, now: Duration) -> (Outcome, String) {
    let outcome = state.handle(
        &request(
            "POST",
            &[],
            json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": { "name": "test client", "version": "1" }
            }}),
        ),
        now,
    );
    let id = header(response(&outcome), "MCP-Session-Id")
        .expect("a session id")
        .to_owned();
    (outcome, id)
}

/// A session past `notifications/initialized`, and its id.
fn ready() -> (State, String) {
    let mut state = State::new();
    let (_, id) = initialize(&mut state, "2025-11-25", T0);
    let outcome = post(
        &mut state,
        &id,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );
    assert_eq!(response(&outcome).status, Status::Accepted);
    (state, id)
}

fn post(state: &mut State, session: &str, message: Value) -> Outcome {
    post_at(state, session, message, T0)
}

fn post_at(state: &mut State, session: &str, message: Value, now: Duration) -> Outcome {
    state.handle(
        &request("POST", &[("MCP-Session-Id", session)], message),
        now,
    )
}

fn call(id: i64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

#[test]
fn initialize_starts_a_session() {
    let mut state = State::new();
    let (outcome, id) = initialize(&mut state, "2025-06-18", T0);
    assert_eq!(id.len(), 22);
    assert!(id.bytes().all(|b| b.is_ascii_graphic()));
    let result = &body(&outcome)["result"];
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert_eq!(result["capabilities"], json!({ "tools": {} }));
    assert_eq!(result["serverInfo"]["name"], "varde");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(result["instructions"], INSTRUCTIONS);
    assert_eq!(state.session().unwrap().as_str(), id);
    assert_eq!(state.client(), Some("test client"));
    assert_eq!(
        outcome.events,
        [Event::Started {
            session: state.session().unwrap().clone(),
            client: Some("test client".into()),
        }]
    );
}

#[test]
fn instructions_fit_in_2_kb() {
    assert!(INSTRUCTIONS.len() <= 2048);
}

#[test]
fn negotiates_the_version() {
    for version in VERSIONS {
        let mut state = State::new();
        let (outcome, _) = initialize(&mut state, version, T0);
        assert_eq!(body(&outcome)["result"]["protocolVersion"], version);
        assert_eq!(state.version(), Some(version));
    }
    let mut state = State::new();
    let (outcome, _) = initialize(&mut state, "2024-11-05", T0);
    assert_eq!(body(&outcome)["result"]["protocolVersion"], VERSIONS[0]);
}

#[test]
fn a_new_initialize_replaces_the_session() {
    let (mut state, old) = ready();
    let (outcome, new) = initialize(&mut state, "2025-11-25", T0);
    assert_ne!(old, new);
    let old_id = SessionId(old.clone());
    assert_eq!(
        outcome.events[0],
        Event::Ended {
            session: old_id,
            end: End::Replaced
        }
    );
    assert!(matches!(outcome.events[1], Event::Started { .. }));
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    assert_eq!(
        response(&post(&mut state, &old, ping.clone())).status,
        Status::NotFound
    );
    assert!(body(&post(&mut state, &new, ping)).get("result").is_some());
}

#[test]
fn get_and_other_methods_are_405() {
    let mut state = State::new();
    for method in ["GET", "PUT"] {
        let outcome = state.handle(&request(method, &[], Value::Null), T0);
        let r = response(&outcome);
        assert_eq!(r.status, Status::MethodNotAllowed);
        assert_eq!(header(r, "Allow"), Some("POST, DELETE"));
    }
}

#[test]
fn delete_ends_the_session() {
    let (mut state, id) = ready();
    let delete = |state: &mut State, headers: &[(&str, &str)]| {
        state.handle(&request("DELETE", headers, Value::Null), T0)
    };
    assert_eq!(
        response(&delete(&mut state, &[])).status,
        Status::BadRequest
    );
    assert_eq!(
        response(&delete(&mut state, &[("MCP-Session-Id", "other")])).status,
        Status::NotFound
    );
    let outcome = delete(&mut state, &[("MCP-Session-Id", &id)]);
    assert_eq!(response(&outcome).status, Status::NoContent);
    assert_eq!(
        outcome.events,
        [Event::Ended {
            session: SessionId(id.clone()),
            end: End::Deleted
        }]
    );
    assert!(state.session().is_none());
    assert_eq!(
        response(&delete(&mut state, &[("MCP-Session-Id", &id)])).status,
        Status::NotFound
    );
}

#[test]
fn requests_need_their_session() {
    let (mut state, id) = ready();
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    let outcome = state.handle(&request("POST", &[], ping.clone()), T0);
    assert_eq!(response(&outcome).status, Status::BadRequest);
    let outcome = post(&mut state, "nope", ping.clone());
    assert_eq!(response(&outcome).status, Status::NotFound);
    let twice = request(
        "POST",
        &[("MCP-Session-Id", &id), ("MCP-Session-Id", &id)],
        ping,
    );
    assert_eq!(
        response(&state.handle(&twice, T0)).status,
        Status::BadRequest
    );
    // With no session at all, also 404.
    let mut state = State::new();
    let outcome = post(
        &mut state,
        &id,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }),
    );
    assert_eq!(response(&outcome).status, Status::NotFound);
}

#[test]
fn protocol_version_header_is_checked() {
    let (mut state, id) = ready();
    let ping = |n: i64| json!({ "jsonrpc": "2.0", "id": n, "method": "ping" });
    let with = |state: &mut State, version: &str, n| {
        state.handle(
            &request(
                "POST",
                &[("MCP-Session-Id", &id), ("MCP-Protocol-Version", version)],
                ping(n),
            ),
            T0,
        )
    };
    assert_eq!(
        response(&with(&mut state, "1999-01-01", 1)).status,
        Status::BadRequest
    );
    assert!(
        body(&with(&mut state, "2025-03-26", 2))
            .get("result")
            .is_some()
    );
    assert!(
        body(&post(&mut state, &id, ping(3)))
            .get("result")
            .is_some()
    );
}

#[test]
fn the_session_is_checked_before_the_version() {
    let mut state = State::new();
    let outcome = state.handle(
        &request(
            "POST",
            &[("MCP-Session-Id", "x"), ("MCP-Protocol-Version", "bad")],
            json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }),
        ),
        T0,
    );
    assert_eq!(response(&outcome).status, Status::NotFound);
}

#[test]
fn bad_bodies_are_400_with_the_error() {
    let (mut state, id) = ready();
    let mut r = request("POST", &[("MCP-Session-Id", &id)], Value::Null);
    r.body = b"{not json".to_vec();
    let outcome = state.handle(&r, T0);
    let response = response(&outcome);
    assert_eq!(response.status, Status::BadRequest);
    let error: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(error["error"]["code"], -32700);
}

#[test]
fn only_ping_before_initialized() {
    let mut state = State::new();
    let (_, id) = initialize(&mut state, "2025-11-25", T0);
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    assert_eq!(body(&post(&mut state, &id, ping))["result"], json!({}));
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    assert_eq!(error_code(&post(&mut state, &id, list)), -32600);
    post(
        &mut state,
        &id,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );
    let list = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" });
    assert_eq!(body(&post(&mut state, &id, list))["result"], tools::list());
}

#[test]
fn notifications_and_responses_are_202() {
    let (mut state, id) = ready();
    for message in [
        json!({ "jsonrpc": "2.0", "method": "notifications/whatever" }),
        json!({ "jsonrpc": "2.0", "id": 9, "result": {} }),
    ] {
        let outcome = post(&mut state, &id, message);
        assert_eq!(response(&outcome).status, Status::Accepted);
        assert!(outcome.events.is_empty());
    }
}

#[test]
fn cancelled_is_passed_on_once_for_a_call_in_flight() {
    let (mut state, id) = ready();
    let cancel = |request: Value| {
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": { "requestId": request, "reason": "user" } })
    };
    // Nothing in flight: dropped.
    let outcome = post(&mut state, &id, cancel(json!("r7")));
    assert_eq!(response(&outcome).status, Status::Accepted);
    assert!(outcome.events.is_empty());
    let dispatched = post(
        &mut state,
        &id,
        json!({ "jsonrpc": "2.0", "id": "r7", "method": "tools/call",
                "params": { "name": "design_summary", "arguments": {} } }),
    );
    assert!(matches!(dispatched.answer, Answer::Dispatch(_)));
    let outcome = post(&mut state, &id, cancel(json!("r7")));
    assert_eq!(
        outcome.events,
        [Event::Cancelled {
            session: SessionId(id.clone()),
            request: Id::String("r7".into()),
        }]
    );
    // Again, or for another id: dropped.
    for request in [json!("r7"), json!("r8")] {
        assert!(post(&mut state, &id, cancel(request)).events.is_empty());
    }
    // Finished: nothing left to cancel.
    state.finish(&SessionId(id.clone()), &Id::String("r7".into()), T0);
    assert!(post(&mut state, &id, cancel(json!("r7"))).events.is_empty());
}

#[test]
fn reused_ids_are_refused() {
    let (mut state, id) = ready();
    let ping = |id: Value| json!({ "jsonrpc": "2.0", "id": id, "method": "ping" });
    assert!(
        body(&post(&mut state, &id, ping(json!(1))))
            .get("result")
            .is_some()
    );
    assert_eq!(error_code(&post(&mut state, &id, ping(json!(1)))), -32600);
    // initialize's own id counts.
    assert_eq!(error_code(&post(&mut state, &id, ping(json!(0)))), -32600);
    // A string id is another id.
    assert!(
        body(&post(&mut state, &id, ping(json!("1"))))
            .get("result")
            .is_some()
    );
}

#[test]
fn the_oldest_ids_are_forgotten() {
    let (mut state, id) = ready();
    let ping = |n: usize| json!({ "jsonrpc": "2.0", "id": n, "method": "ping" });
    // initialize used 0; 1..=USED_IDS pushes it out.
    for n in 1..=USED_IDS {
        assert!(
            body(&post(&mut state, &id, ping(n)))
                .get("result")
                .is_some()
        );
    }
    assert!(
        body(&post(&mut state, &id, ping(0)))
            .get("result")
            .is_some()
    );
    assert_eq!(error_code(&post(&mut state, &id, ping(USED_IDS))), -32600);
}

#[test]
fn unknown_methods_are_32601() {
    let (mut state, id) = ready();
    for (n, method) in ["logging/setLevel", "resources/list", "nope"]
        .iter()
        .enumerate()
    {
        let message = json!({ "jsonrpc": "2.0", "id": n + 10, "method": method, "params": {} });
        assert_eq!(error_code(&post(&mut state, &id, message)), -32601);
    }
}

#[test]
fn tool_calls_go_to_the_app() {
    let (mut state, id) = ready();
    let outcome = post(
        &mut state,
        &id,
        call(5, "feature", json!({ "name": "Sketch 1" })),
    );
    let Answer::Dispatch(dispatch) = &outcome.answer else {
        panic!("expected a dispatch");
    };
    assert_eq!(dispatch.session.as_str(), id);
    assert_eq!(dispatch.id, Id::Number(5));
    assert_eq!(
        dispatch.call,
        Call::Feature {
            name: "Sketch 1".into(),
            page: crate::tools::Page::FEATURE,
        }
    );
    let reply = dispatch.respond(&ToolReply::text("done"));
    let reply: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(reply["id"], 5);
    assert_eq!(reply["result"]["content"][0]["text"], "done");
    assert_eq!(reply["result"]["isError"], false);
}

#[test]
fn bad_tool_calls() {
    let (mut state, id) = ready();
    assert_eq!(
        error_code(&post(&mut state, &id, call(1, "nope", json!({})))),
        -32602
    );
    let outcome = post(&mut state, &id, call(2, "feature", json!({})));
    let result = &body(&outcome)["result"];
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("name")
    );
}

#[test]
fn idle_sessions_end() {
    let (mut state, id) = ready();
    let ping = |n: i64| json!({ "jsonrpc": "2.0", "id": n, "method": "ping" });
    // Activity restarts the clock.
    let later = T0 + IDLE - Duration::from_secs(1);
    assert!(
        body(&post_at(&mut state, &id, ping(1), later))
            .get("result")
            .is_some()
    );
    assert_eq!(state.expire(later + IDLE - Duration::from_secs(1)), None);
    let outcome = post_at(&mut state, &id, ping(2), later + IDLE);
    assert_eq!(response(&outcome).status, Status::NotFound);
    assert_eq!(
        outcome.events,
        [Event::Ended {
            session: SessionId(id),
            end: End::Idle
        }]
    );
    assert!(state.session().is_none());
}

#[test]
fn a_call_in_flight_keeps_the_session() {
    let (mut state, id) = ready();
    let outcome = post(&mut state, &id, call(1, "design_summary", json!({})));
    assert!(matches!(outcome.answer, Answer::Dispatch(_)));
    let late = T0 + IDLE + IDLE;
    assert_eq!(state.expire(late), None);
    state.finish(&SessionId(id.clone()), &Id::Number(1), late);
    assert_eq!(state.expire(late + IDLE - Duration::from_secs(1)), None);
    assert!(state.expire(late + IDLE).is_some());
    // A clock gone backwards doesn't end it, nor overflow.
    let (mut state, _) = ready();
    assert_eq!(state.expire(Duration::ZERO), None);
}

#[test]
fn finish_for_an_old_session_does_nothing() {
    let (mut state, id) = ready();
    let outcome = post(&mut state, &id, call(1, "design_summary", json!({})));
    assert!(matches!(outcome.answer, Answer::Dispatch(_)));
    state.finish(&SessionId("old".into()), &Id::Number(1), T0);
    assert_eq!(state.expire(T0 + IDLE + IDLE), None);
}

#[test]
fn end_disconnects() {
    let (mut state, id) = ready();
    assert_eq!(
        state.end(),
        Some(Event::Ended {
            session: SessionId(id),
            end: End::Disconnected
        })
    );
    assert_eq!(state.end(), None);
}

#[test]
fn finishing_a_call_twice_keeps_the_other_in_flight() {
    let (mut state, id) = ready();
    let session = SessionId(id.clone());
    for n in [1, 2] {
        let outcome = post(&mut state, &id, call(n, "design_summary", json!({})));
        assert!(matches!(outcome.answer, Answer::Dispatch(_)));
    }
    // Call 1 answered, and then given up on too: call 2 still waits.
    state.finish(&session, &Id::Number(1), T0);
    state.finish(&session, &Id::Number(1), T0);
    assert_eq!(state.expire(T0 + IDLE + IDLE), None);
    state.finish(&session, &Id::Number(2), T0);
    assert!(state.expire(T0 + IDLE).is_some());
}

#[test]
fn an_id_in_flight_is_refused_even_once_forgotten() {
    let (mut state, id) = ready();
    let outcome = post(&mut state, &id, call(-1, "design_summary", json!({})));
    assert!(matches!(outcome.answer, Answer::Dispatch(_)));
    for n in 0..USED_IDS as i64 {
        post(
            &mut state,
            &id,
            json!({ "jsonrpc": "2.0", "id": n, "method": "ping" }),
        );
    }
    let outcome = post(&mut state, &id, call(-1, "design_summary", json!({})));
    assert_eq!(error_code(&outcome), -32600);
}

#[test]
fn initialize_over_an_idle_session_ends_it_as_idle() {
    let (mut state, id) = ready();
    let (outcome, new) = initialize(&mut state, "2025-11-25", T0 + IDLE);
    assert_eq!(
        outcome.events,
        [
            Event::Ended {
                session: SessionId(id),
                end: End::Idle
            },
            Event::Started {
                session: SessionId(new),
                client: Some("test client".into())
            }
        ]
    );
}
