//! The tool `measure`, through `Varde::update`: a face's area, an
//! edge's length, a body's volume, two faces' distance and angle, each
//! as the measure tool shows it and saying which generation it
//! measured; a stale key refused; the call waiting on regen's measure,
//! queued beside the user's own, and timing out.

use serde_json::{Value, json};

use super::motion::{edge_between, face_facing, plate_made_symmetric};
use super::*;

fn is_measure(request: &Request) -> bool {
    matches!(
        request,
        Request::Query {
            query: varde_regen::Query::Measure(_),
            ..
        }
    )
}

/// Calls `measure` with `arguments` in "a session", answers regen's
/// measure: its reply.
fn measure(varde: &mut Varde, requests: &Lane, arguments: Value) -> ToolReply {
    let id = (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 1);
    let reply = calling(varde, "a session", id, tool_call("measure", arguments));
    respond(varde, requests, is_measure);
    reply.try_recv().unwrap()
}

/// [`measure`]'s data, checked not to be an error, and to say it
/// measured the design's generation now.
fn measured(varde: &mut Varde, requests: &Lane, arguments: Value) -> Value {
    let reply = measure(varde, requests, arguments);
    assert!(!reply.is_error, "{}", reply.text);
    let data = bare(reply).structured.unwrap();
    let generation = varde.screen.doc().unwrap().editor.generation();
    assert_eq!(
        data["model"]["generation"],
        serde_json::to_value(generation).unwrap()
    );
    assert_eq!(data["model"]["current"], json!(true));
    assert_eq!(data["units"], json!("mm"));
    data
}

fn ready() -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    (varde, requests)
}

#[test]
fn measures_a_face_s_area() {
    let (mut varde, requests) = ready();
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let data = measured(&mut varde, &requests, json!({ "a": { "face": top } }));
    let a = &data["a"];
    assert_eq!(a["kind"], json!("Planar face"), "{data}");
    // The plate's 60 × 40 top less its hole of radius 8.
    let area = 2400.0 - std::f64::consts::PI * 64.0;
    let shown = varde_expr::Shown::current();
    let units = varde_expr::LengthUnit::Mm;
    assert_eq!(
        a["values"]["area"],
        json!(shown.format_power(area, units, varde_expr::Power::Area)),
        "{data}"
    );
    assert_eq!(a["values"]["normal"], json!("0, 0, 1"));
    assert!(data.get("b").is_none() && data.get("between").is_none());
    let row = varde.mcp.log.back().unwrap();
    assert_eq!(row.state, McpCallState::Answered);
    assert!(
        row.result.as_ref().unwrap().starts_with("area: "),
        "{row:?}"
    );
}

#[test]
fn measures_an_edge_s_length() {
    let (mut varde, requests) = ready();
    let back = edge_between(&varde, "Body 1", [-30.0, 20.0, 10.0], [30.0, 20.0, 10.0]);
    let data = measured(&mut varde, &requests, json!({ "a": { "edge": back } }));
    assert_eq!(data["a"]["kind"], json!("Straight edge"), "{data}");
    assert_eq!(data["a"]["values"]["length"], json!("60 mm"), "{data}");
}

#[test]
fn measures_a_body_s_volume() {
    let (mut varde, requests) = ready();
    let data = measured(&mut varde, &requests, json!({ "a": { "body": "Body 1" } }));
    let a = &data["a"];
    assert_eq!(a["kind"], json!("body Body 1"), "{data}");
    let volume = 24000.0 - std::f64::consts::PI * 64.0 * 10.0;
    let shown = varde_expr::Shown::current();
    let units = varde_expr::LengthUnit::Mm;
    assert_eq!(
        a["values"]["volume"],
        json!(shown.format_power(volume, units, varde_expr::Power::Volume)),
        "{data}"
    );
    assert_eq!(a["values"]["box_size"], json!("60, 40, 10 mm"), "{data}");
    let refused = measure(&mut varde, &requests, json!({ "a": { "body": "Body 9" } }));
    assert!(refused.is_error);
    assert!(
        refused.text.contains("no body is named"),
        "{}",
        refused.text
    );
}

#[test]
fn measures_two_faces_distance_and_angle() {
    let (mut varde, requests) = ready();
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let bottom = face_facing(&varde, "Body 1", [0.0, 0.0, -1.0]);
    let data = measured(
        &mut varde,
        &requests,
        json!({ "a": { "face": top }, "b": { "face": bottom } }),
    );
    let between = &data["between"];
    assert_eq!(between["distance"], json!("10 mm"), "{data}");
    assert_eq!(between["dz"], json!("-10 mm"), "{data}");
    assert_eq!(between["angle"], json!("180°"), "{data}");
    assert_eq!(data["b"]["kind"], json!("Planar face"));
    let row = varde.mcp.log.back().unwrap();
    let said = row.result.as_ref().unwrap();
    assert_eq!(said, "distance 10 mm · angle 180°");
}

/// A key whose face a later edit moved is refused at once, saying to
/// ask again; regen isn't asked.
#[test]
fn a_stale_key_is_refused() {
    let (mut varde, requests) = ready();
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    plate_made_symmetric(&mut varde, &requests);
    let reply = measure(&mut varde, &requests, json!({ "a": { "face": top } }));
    assert!(reply.is_error);
    assert!(
        reply
            .text
            .contains("a: that face isn't in the model any more"),
        "{}",
        reply.text
    );
    let reply = measure(
        &mut varde,
        &requests,
        json!({ "a": { "edge": "nonsense" } }),
    );
    assert!(
        reply.text.contains("isn't a key Varde gave"),
        "{}",
        reply.text
    );
    assert!(!requests.borrow().iter().any(is_measure));
}

/// The client's measure is a request of its own, queued beside the
/// user's measure riding on the regeneration: neither replaces the
/// other, and the user's answer is still theirs.
#[test]
fn the_client_s_measure_leaves_the_user_s_alone() {
    let (mut varde, requests) = ready();
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let body = varde.screen.doc().unwrap().editor.document().bodies()[0].id;
    let doc = varde.screen.doc_mut().unwrap();
    doc.start_measure();
    doc.measure_body(body, false);
    doc.sync();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        tool_call("measure", json!({ "a": { "face": top } })),
    );
    let sent = requests.borrow();
    let user = sent
        .iter()
        .filter(|r| r.asked().and_then(|asked| asked.inspect).is_some())
        .count();
    let client = sent.iter().filter(|r| is_measure(r)).count();
    drop(sent);
    assert_eq!((user, client), (1, 1));
    respond(&mut varde, &requests, |_| true);
    let data = bare(reply.try_recv().unwrap()).structured.unwrap();
    assert_eq!(data["a"]["kind"], json!("Planar face"));
    let doc = varde.screen.doc().unwrap();
    let shown = doc.measure_state().unwrap();
    let first = shown.picks[0].as_ref().map(|picked| picked.outcome);
    assert!(
        matches!(first, Some(varde_view::Outcome::Measured(Ok(m))) if matches!(m, varde_regen::Measure::Body { .. })),
        "{first:?}"
    );
}

/// A measure waiting 30 s is answered that the model took too long; an
/// answer coming after is dropped.
#[test]
fn a_measure_waiting_30_s_times_out() {
    let (mut varde, requests) = ready();
    let came = iced::time::Instant::now();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        tool_call("measure", json!({ "a": { "body": "Body 1" } })),
    );
    assert!(reply.try_recv().is_err());
    assert_eq!(varde.mcp.log[0].state, McpCallState::Waiting);
    let late = came + waiting::BOUND + std::time::Duration::from_secs(1);
    let _ = varde.update(Message::Tick(late));
    assert_eq!(
        reply.try_recv().unwrap(),
        ToolReply::error(waiting::TOOK_TOO_LONG)
    );
    respond(&mut varde, &requests, is_measure);
    assert_eq!(varde.mcp.log[0].state, McpCallState::Failed);
    assert!(!varde.screen.doc().unwrap().mcp_waiting());
}

/// A body a join merged into another is measured as the body holding
/// it, whole: the answer says so rather than giving the holder's values
/// under the merged body's name alone.
#[test]
fn a_merged_body_says_it_is_measured_as_its_holder() {
    let (mut varde, requests) = super::motion::with_disc();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 1", "tools": ["Body 2"] }),
    );
    edit_data(reply);
    let data = measured(&mut varde, &requests, json!({ "a": { "body": "Body 2" } }));
    let a = &data["a"];
    assert_eq!(a["kind"], json!("body Body 2"), "{data}");
    assert_eq!(a["merged_into"], json!("Body 1"), "{data}");
    assert_eq!(a["values"]["box_size"], json!("60, 40, 20 mm"), "{data}");
    let data = measured(&mut varde, &requests, json!({ "a": { "body": "Body 1" } }));
    assert!(data["a"].get("merged_into").is_none(), "{data}");
}

/// A measure answered after the user edited on is of an earlier state:
/// the model says so in a measure's terms, not a summary's failures.
#[test]
fn a_measure_of_an_earlier_state_says_its_values_may_be_out_of_date() {
    let (mut varde, requests) = ready();
    let reply = calling(
        &mut varde,
        "a session",
        1,
        tool_call("measure", json!({ "a": { "body": "Body 1" } })),
    );
    let doc = varde.screen.doc_mut().unwrap();
    let body = doc.editor.document().bodies()[0].id;
    doc.apply(Command::SetVisible(body, false));
    doc.sync();
    respond(&mut varde, &requests, is_measure);
    let data = bare(reply.try_recv().unwrap()).structured.unwrap();
    let model = &data["model"];
    assert_eq!(model["current"], json!(false), "{data}");
    let note = model["note"].as_str().unwrap();
    assert!(note.contains("values may be out of date"), "{note}");
    assert!(!note.contains("failures"), "{note}");
}
