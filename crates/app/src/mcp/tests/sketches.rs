//! `add_sketch`, `set_sketch_plane` and `add_link` through
//! `Varde::update`: on origin planes and on faces by key, keys bad or
//! stale, a plane changed keeping the drawing, a link projecting an edge
//! through the solver, and the refusals in the UI's words. None opens
//! the sketch for the user.

use serde_json::{Value, json};
use varde_document::{FeatureKind, OriginPlane, Plane};

use super::motion::{face_facing, face_of_form, key_of, plate_made_symmetric};
use super::*;
use crate::tests::SolveLane;

/// The feature named `name`.
fn named(varde: &Varde, name: &str) -> varde_document::Feature {
    let document = varde.screen.doc().unwrap().editor.document();
    (document.features().iter())
        .find(|feature| feature.name == name)
        .unwrap()
        .clone()
}

/// The plane of the sketch named `name`.
fn plane_of(varde: &Varde, name: &str) -> Plane {
    match named(varde, name).kind {
        FeatureKind::Sketch { plane, .. } => plane,
        kind => panic!("not a sketch: {kind:?}"),
    }
}

/// Whether the user is in no sketch, nor picking a plane.
fn nothing_open(varde: &Varde) -> bool {
    let doc = varde.screen.doc().unwrap();
    doc.sketch.is_none() && doc.picking_plane.is_none()
}

/// `tool` with `arguments` failed, saying `why`, the design unchanged.
fn failed_edit(varde: &mut Varde, requests: &Lane, tool: &str, arguments: Value, why: &str) {
    let generation = varde.screen.doc().unwrap().editor.generation();
    let (reply, state) = edit(varde, requests, tool, arguments);
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.contains(why), "{}", reply.text);
    assert_eq!(state, McpCallState::Failed, "{}", reply.text);
    assert_eq!(varde.screen.doc().unwrap().editor.generation(), generation);
}

/// The key of a straight edge of the plate's top face.
fn top_edge(varde: &Varde) -> String {
    key_of(varde, "Body 1", "edges", |edge| {
        let ends = &edge["ends"];
        edge["form"] == json!("line") && ends[0][2] == ends[1][2] && number(&ends[0][2]) > 0.0
    })
}

/// `add_sketch` on each origin plane, in any case, adds an empty sketch
/// at the end, named and with its id in the result, one undo step,
/// without opening it.
#[test]
fn add_sketch_on_origin_planes() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    for (word, plane, name) in [
        ("XY", OriginPlane::XY, "Sketch 2"),
        ("xz", OriginPlane::XZ, "Sketch 3"),
        ("Yz", OriginPlane::YZ, "Sketch 4"),
    ] {
        let (reply, state) = edit(
            &mut varde,
            &requests,
            "add_sketch",
            json!({ "plane": word }),
        );
        assert_eq!(state, McpCallState::Applied, "{}", reply.text);
        let data = edit_data(reply);
        let feature = named(&varde, name);
        assert_eq!(data["sketch"], json!(name));
        assert_eq!(data["id"], json!(feature.id.get()));
        assert_eq!(data["result"], json!("applied"));
        assert_eq!(plane_of(&varde, name), Plane::Origin(plane));
        let FeatureKind::Sketch { sketch, .. } = feature.kind else {
            unreachable!()
        };
        assert!(sketch.points.is_empty() && sketch.curves.is_empty());
        assert!(nothing_open(&varde));
    }
    let log = varde.mcp.log.back().unwrap();
    assert_eq!(log.args, "yz");
    assert_eq!(log.result.as_deref(), Some("applied · Sketch 4"));
    undo(&mut varde);
    assert!(
        (varde
            .screen
            .doc()
            .unwrap()
            .editor
            .document()
            .features()
            .iter())
        .all(|feature| feature.name != "Sketch 4")
    );
}

/// A new sketch goes at the rollback marker, as the user's does.
#[test]
fn add_sketch_goes_at_the_rollback_marker() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Extrude 1" }),
    );
    edit_data(reply);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "xy" }),
    );
    edit_data(reply);
    assert_eq!(feature_name(&varde, 1), "Sketch 2");
    assert_eq!(feature_name(&varde, 2), "Extrude 1");
}

/// On a flat face by key, the sketch is on that face of the plate, its
/// result giving the face's key; a curved face, a key that isn't one,
/// another kind's key and a stale one are refused, the first in the
/// plane pick's words.
#[test]
fn add_sketch_on_a_face_by_key() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": { "face": top } }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["sketch"], json!("Sketch 2"));
    assert!(data["plane"]["face"].is_string(), "{}", data["plane"]);
    let Plane::Face(face) = plane_of(&varde, "Sketch 2") else {
        panic!("not on a face");
    };
    let plate = varde.screen.doc().unwrap().editor.document().bodies()[0].id;
    assert_eq!(face.body, plate);
    assert!(nothing_open(&varde));
    assert_eq!(varde.mcp.log.back().unwrap().args, "a face");

    let hole = face_of_form(&varde, "Body 1", "cylinder");
    failed_edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": { "face": hole } }),
        &format!("add_sketch: plane: {}", varde_view::CURVED_FACE),
    );
    failed_edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": { "face": "nonsense" } }),
        "add_sketch: plane: that isn't a key Varde gave",
    );
    let edge = top_edge(&varde);
    failed_edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": { "face": edge } }),
        "that key names an edge, not a face",
    );
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    plate_made_symmetric(&mut varde, &requests);
    failed_edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": { "face": top } }),
        "that face isn't in the model any more",
    );
}

/// `set_sketch_plane` moves a sketch to another origin plane keeping its
/// drawing; the plane it's on changes nothing; a face made after the
/// sketch is refused in the plane pick's words, a face before it taken;
/// a feature that isn't a sketch is named.
#[test]
fn set_sketch_plane_keeps_the_drawing() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let before = named(&varde, "Sketch 1").kind;
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "set_sketch_plane",
        json!({ "sketch": "Sketch 1", "plane": "XZ" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["sketch"], json!("Sketch 1"));
    assert_eq!(data["plane"], json!("xz"));
    assert_eq!(plane_of(&varde, "Sketch 1"), Plane::Origin(OriginPlane::XZ));
    let (FeatureKind::Sketch { sketch: was, .. }, FeatureKind::Sketch { sketch: now, .. }) =
        (before, named(&varde, "Sketch 1").kind)
    else {
        unreachable!()
    };
    assert_eq!(was, now);
    assert!(nothing_open(&varde));
    assert_eq!(varde.mcp.log.back().unwrap().args, "Sketch 1 to xz");

    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_sketch_plane",
        json!({ "sketch": "Sketch 1", "plane": "xz" }),
    );
    assert_eq!(reply.text, edit::NOTHING_TO_CHANGE);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    failed_edit(
        &mut varde,
        &requests,
        "set_sketch_plane",
        json!({ "sketch": "Sketch 1", "plane": { "face": top } }),
        "set_sketch_plane: plane: Sketch 1 can only go on a face made before it",
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "xy" }),
    );
    edit_data(reply);
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "set_sketch_plane",
        json!({ "sketch": "Sketch 2", "plane": { "face": top } }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert!(matches!(plane_of(&varde, "Sketch 2"), Plane::Face(_)));
    failed_edit(
        &mut varde,
        &requests,
        "set_sketch_plane",
        json!({ "sketch": "Extrude 1", "plane": "xy" }),
        "\"Extrude 1\" isn't a sketch",
    );
}

/// The example listening in "a session" with a sketch on the plate's
/// top face, Sketch 2, its solve lane deferred.
fn on_top() -> (Varde, Lane, SolveLane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let lane = SolveLane::connect(varde.screen.doc_mut().unwrap());
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": { "face": top } }),
    );
    edit_data(reply);
    (varde, requests, lane)
}

/// The links of the sketch named `name`, with their sources.
fn links(varde: &Varde, name: &str) -> (Vec<varde_sketch::Link>, usize) {
    match named(varde, name).kind {
        FeatureKind::Sketch {
            sketch, sources, ..
        } => (sketch.links.clone(), sources.len()),
        kind => panic!("not a sketch: {kind:?}"),
    }
}

/// `add_link` projecting an edge of the plate waits on the solver, is
/// committed once accepted as one undo step with its source, and
/// answers after regeneration found its geometry: the link's id and the
/// ids of its points and curves, as `sketch` lists them. The sketch
/// isn't opened.
#[test]
fn add_link_projects_an_edge() {
    let (mut varde, requests, mut lane) = on_top();
    // A sketch on a face has its face's link already.
    let (had, sources) = links(&varde, "Sketch 2");
    let edge = top_edge(&varde);
    let reply = calling(
        &mut varde,
        "a session",
        10,
        tool_call(
            "add_link",
            json!({ "sketch": "Sketch 2", "from": { "edge": edge } }),
        ),
    );
    regenerate(&mut varde, &requests);
    // Proposed, not committed yet.
    assert!(reply.try_recv().is_err());
    assert_eq!(links(&varde, "Sketch 2").0, had);
    assert!(varde.screen.doc().unwrap().proposing());
    solve(&mut varde, &mut lane);
    assert_eq!(links(&varde, "Sketch 2").1, sources + 1);
    regenerate(&mut varde, &requests);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(varde.mcp.log.back().unwrap().state, McpCallState::Applied);
    let (made, _) = links(&varde, "Sketch 2");
    let new: Vec<_> = (made.iter())
        .filter(|link| had.iter().all(|old| old.id != link.id))
        .collect();
    let [link] = &new[..] else {
        panic!("one new link: {made:?}");
    };
    assert_eq!(data["sketch"], json!("Sketch 2"));
    assert_eq!(data["link"], json!(link.id.get()));
    assert_eq!(data["kind"], json!("project"));
    let ids = |ids: &[varde_sketch::Id]| json!(ids.iter().map(|id| id.get()).collect::<Vec<_>>());
    assert_eq!(data["points"], ids(&link.points));
    assert_eq!(data["curves"], ids(&link.curves));
    // An edge projects to one line between two points.
    assert_eq!(link.curves.len(), 1, "{data}");
    assert_eq!(link.points.len(), 2, "{data}");
    assert!(nothing_open(&varde));
    assert_eq!(
        varde.mcp.log.back().unwrap().args,
        "Sketch 2 an edge projected"
    );
    // Asked again, it's linked already.
    let edge = top_edge(&varde);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "add_link",
        json!({ "sketch": "Sketch 2", "from": { "edge": edge } }),
    );
    assert_eq!(state, McpCallState::NotApplied, "{}", reply.text);
    assert!(
        reply.text.contains(&format!(
            "that is linked into Sketch 2 already, as link {}",
            link.id.get()
        )),
        "{}",
        reply.text
    );
    // One undo step takes the link back with its geometry.
    undo(&mut varde);
    assert_eq!(links(&varde, "Sketch 2").0, had);
}

/// Another sketch's curve is projected as the tool's click on it is.
#[test]
fn add_link_projects_another_sketchs_curve() {
    let (mut varde, requests, mut lane) = on_top();
    let (_, sources) = links(&varde, "Sketch 2");
    let document = varde.screen.doc().unwrap().editor.document();
    let sketch = document.features()[0].id;
    let circle = (crate::doc::sketch_of(document, sketch)
        .unwrap()
        .curves
        .iter())
    .find(|curve| matches!(curve.curve, varde_sketch::Curve::Circle { .. }))
    .unwrap()
    .id
    .get();
    let reply = calling(
        &mut varde,
        "a session",
        10,
        tool_call(
            "add_link",
            json!({ "sketch": "Sketch 2", "from": { "sketch": "Sketch 1", "curve": circle } }),
        ),
    );
    solve(&mut varde, &mut lane);
    regenerate(&mut varde, &requests);
    let data = edit_data(reply.try_recv().unwrap());
    assert_eq!(data["curves"].as_array().unwrap().len(), 1, "{data}");
    assert_eq!(links(&varde, "Sketch 2").1, sources + 1);
}

/// What the tools refuse, in their words: a corner for Intersect, what's
/// made at or after the sketch, a sketch's id it hasn't, not a sketch.
#[test]
fn add_link_refusals_are_the_tools() {
    let (mut varde, requests, _lane) = on_top();
    let corner = key_of(&varde, "Body 1", "corners", |_| true);
    failed_edit(
        &mut varde,
        &requests,
        "add_link",
        json!({ "sketch": "Sketch 2", "kind": "intersect", "from": { "corner": corner } }),
        "add_link: from: Intersect takes faces and edges, cut with the sketch's plane",
    );
    let edge = top_edge(&varde);
    failed_edit(
        &mut varde,
        &requests,
        "add_link",
        json!({ "sketch": "Sketch 1", "from": { "edge": edge } }),
        "add_link: from: Only what's made before Sketch 1 can be projected",
    );
    failed_edit(
        &mut varde,
        &requests,
        "add_link",
        json!({ "sketch": "Sketch 1", "from": { "sketch": "Sketch 2", "point": 999 } }),
        "add_link: from: the sketch has no point 999",
    );
    failed_edit(
        &mut varde,
        &requests,
        "add_link",
        json!({ "sketch": "Extrude 1", "from": { "edge": edge } }),
        "\"Extrude 1\" isn't a sketch",
    );
    failed_edit(
        &mut varde,
        &requests,
        "add_link",
        json!({ "sketch": "Sketch 2", "from": { "edge": "nonsense" } }),
        "add_link: from: that isn't a key Varde gave",
    );
}

/// A link the solver hasn't answered within the bound isn't applied,
/// and its proposal leaves the queue.
#[test]
fn add_link_waiting_on_the_solver_times_out() {
    let (mut varde, _requests, lane) = on_top();
    let (had, _) = links(&varde, "Sketch 2");
    let edge = top_edge(&varde);
    let reply = calling(
        &mut varde,
        "a session",
        10,
        tool_call(
            "add_link",
            json!({ "sketch": "Sketch 2", "from": { "edge": edge } }),
        ),
    );
    assert_eq!(lane.waiting().len(), 1);
    let later = iced::time::Instant::now() + waiting::BOUND;
    let _ = varde.update(Message::Tick(later));
    let reply = reply.try_recv().unwrap();
    assert!(
        reply.text.contains(waiting::NOT_APPLIED_SOLVER),
        "{}",
        reply.text
    );
    assert!(!varde.screen.doc().unwrap().proposing());
    assert_eq!(links(&varde, "Sketch 2").0, had);
}

/// The user's Undo passes over the client's proposal waiting (it isn't
/// theirs, nor shown): it undoes the history, here the sketch, and the
/// link proposed again in its turn finds the sketch gone, which the call
/// says.
#[test]
fn undo_passes_over_a_link_waiting_on_the_solver() {
    let (mut varde, _requests, mut lane) = on_top();
    let edge = top_edge(&varde);
    let reply = calling(
        &mut varde,
        "a session",
        10,
        tool_call(
            "add_link",
            json!({ "sketch": "Sketch 2", "from": { "edge": edge } }),
        ),
    );
    undo(&mut varde);
    let document = varde.screen.doc().unwrap().editor.document();
    assert!((document.features().iter()).all(|feature| feature.name != "Sketch 2"));
    solve(&mut varde, &mut lane);
    let reply = reply.try_recv().unwrap();
    assert!(
        reply
            .text
            .contains("the sketch was removed before the edit's turn"),
        "{}",
        reply.text
    );
    assert_eq!(
        varde.mcp.log.back().unwrap().state,
        McpCallState::NotApplied
    );
    assert!(!varde.screen.doc().unwrap().proposing());
}

/// The user's point placed in the sketch, opened again behind the
/// client's link waiting on the solver (shown with it), the link timing
/// out: the point is still placed, on the sketch without the link.
#[test]
fn the_users_edit_behind_a_link_timed_out() {
    let (mut varde, requests, mut lane) = on_top();
    regenerate(&mut varde, &requests);
    let id = named(&varde, "Sketch 2").id;
    let before = points(&varde);
    let edge = top_edge(&varde);
    let reply = calling(
        &mut varde,
        "a session",
        10,
        tool_call(
            "add_link",
            json!({ "sketch": "Sketch 2", "from": { "edge": edge } }),
        ),
    );
    ui(
        &mut varde,
        varde_view::Message::Look(varde_view::Look::EditFeature(id)),
    );
    ui(
        &mut varde,
        varde_view::Message::Look(varde_view::Look::SelectTool(varde_view::Tool::Point)),
    );
    assert!(varde.screen.doc().unwrap().sketch.is_some());
    place_point(&mut varde, 1.0, 2.0);
    let later = iced::time::Instant::now() + waiting::BOUND;
    let _ = varde.update(Message::Tick(later));
    let reply = reply.try_recv().unwrap();
    assert!(
        reply.text.contains(waiting::NOT_APPLIED_SOLVER),
        "{}",
        reply.text
    );
    solve(&mut varde, &mut lane);
    let session = varde.screen.doc().unwrap().sketch.as_ref().unwrap();
    assert_eq!(session.refusal, None);
    assert_eq!(points(&varde), before + 1);
}
