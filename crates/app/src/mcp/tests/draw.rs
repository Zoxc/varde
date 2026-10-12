//! `edit_sketch` through `Varde::update` with the solve lane: shapes by
//! label used by later operations, constraints and dimensions, moving
//! and deleting by id, a dimension that would over-constrain placed as
//! a reference and said, an unsolvable edit not applied, bad labels and
//! ids naming the operation, the user's sketch open, one undo step, and
//! the degrees of freedom in the result; the user drawing beside a
//! client's edit, their Undo or edit between its steps (of that sketch,
//! another feature, or the sketch removed), a dimension after a move,
//! and spline points the tool refuses.

use varde_document::FeatureKind;
use varde_sketch::{Curve, Sketch};

use super::*;
use crate::tests::SolveLane;

/// The example listening in "a session", with an empty sketch on XY,
/// Sketch 2, its solve lane deferred.
fn on_xy() -> (Varde, Lane, SolveLane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let lane = SolveLane::connect(varde.screen.doc_mut().unwrap());
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "xy" }),
    );
    edit_data(reply);
    (varde, requests, lane)
}

/// Sketch 2 as committed.
fn drawn(varde: &Varde) -> Sketch {
    let document = varde.screen.doc().unwrap().editor.document();
    let feature = (document.features().iter())
        .find(|feature| feature.name == "Sketch 2")
        .unwrap();
    match &feature.kind {
        FeatureKind::Sketch { sketch, .. } => sketch.clone(),
        kind => panic!("not a sketch: {kind:?}"),
    }
}

/// `edit_sketch` of Sketch 2 with `operations`, solved and regenerated:
/// its reply and its row's state.
fn draw(
    varde: &mut Varde,
    requests: &Lane,
    lane: &mut SolveLane,
    operations: Value,
) -> (ToolReply, McpCallState) {
    let id = (varde.mcp.log.back()).map_or(1, |logged| i64::try_from(logged.serial).unwrap() + 1);
    let call = tool_call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": operations }),
    );
    let reply = calling(varde, "a session", id, call);
    // Behind the user's edits it waits for the model, then for the
    // solver.
    for _ in 0..3 {
        solve(varde, lane);
        regenerate(varde, requests);
    }
    let reply = reply.try_recv().unwrap();
    let state = varde.mcp.log.back().unwrap().state;
    (reply, state)
}

/// `operations` failed, saying `why`, the sketch unchanged.
fn draw_fails(
    varde: &mut Varde,
    requests: &Lane,
    lane: &mut SolveLane,
    operations: Value,
    why: &str,
) {
    let before = drawn(varde);
    let (reply, state) = draw(varde, requests, lane, operations);
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.contains(why), "{}", reply.text);
    assert_eq!(state, McpCallState::Failed, "{}", reply.text);
    assert_eq!(drawn(varde), before);
}

/// The id `labels[label][part]` got.
fn label_id(data: &Value, label: &str, part: &str) -> u32 {
    let id = data["labels"][label][part].as_u64();
    u32::try_from(id.unwrap_or_else(|| panic!("no {label}.{part}: {data}"))).unwrap()
}

/// The length of the line numbered `id` in `sketch`.
fn length(sketch: &Sketch, id: u32) -> f64 {
    let line = (sketch.curves.iter())
        .find(|entry| entry.id.get() == id)
        .unwrap();
    let Curve::Line { start, end } = line.curve else {
        panic!("not a line: {line:?}");
    };
    sketch
        .point(start)
        .unwrap()
        .at
        .distance(sketch.point(end).unwrap().at)
}

/// A rectangle by its shape, labelled, its side then given a length by
/// a later dimension through the label: one undo step, solved, the
/// result mapping the label's parts to their ids and giving the degrees
/// of freedom left. The sketch isn't opened.
#[test]
fn a_rectangle_by_label_takes_a_dimension() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([
            { "op": "rectangle", "corner": [0, 0], "size": ["20 mm", 10], "label": "r" },
            { "op": "dimension", "kind": "length", "items": ["r.side1"], "value": "30 mm" }
        ]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["result"], json!("applied"));
    assert_eq!(data["sketch"], json!("Sketch 2"));
    let sketch = drawn(&varde);
    assert_eq!(sketch.points.len(), 4);
    assert_eq!(sketch.curves.len(), 4);
    // Held horizontal and vertical, as the tool holds it.
    assert_eq!(sketch.constraints.len(), 4);
    assert_eq!(sketch.dimensions.len(), 1);
    let side = label_id(&data, "r", "side1");
    assert!(
        (length(&sketch, side) - 30.0).abs() < 1e-6,
        "{}",
        length(&sketch, side)
    );
    let corner = label_id(&data, "r", "corner1");
    assert!(sketch.points.iter().any(|point| point.id.get() == corner));
    // Four points, four held by H/V, one length.
    assert_eq!(data["degrees_of_freedom"], json!(3));
    assert!(data.get("references").is_none(), "{data}");
    assert!(varde.screen.doc().unwrap().sketch.is_none());
    assert_eq!(
        varde.mcp.log.back().unwrap().args,
        "Sketch 2: rectangle, dimension"
    );
    // One undo step takes the whole call back.
    undo(&mut varde);
    let sketch = drawn(&varde);
    assert!(sketch.points.is_empty() && sketch.curves.is_empty());
}

/// Circles by label, held equal by a constraint between the labels:
/// solved, both of one radius.
#[test]
fn circles_held_equal_by_their_labels() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([
            { "op": "circle", "center": [0, 0], "diameter": 10, "label": "a" },
            { "op": "circle", "center": "origin", "diameter": "4 mm", "label": "b", "construction": true },
            { "op": "constraint", "kind": "equal", "items": ["a", "b"] },
            { "op": "dimension", "kind": "diameter", "items": ["b"], "value": "6 mm" }
        ]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    let sketch = drawn(&varde);
    let radius = |label: &str| {
        let id = label_id(&data, label, "id");
        match (sketch.curves.iter())
            .find(|entry| entry.id.get() == id)
            .unwrap()
            .curve
        {
            Curve::Circle { radius, .. } => radius,
            ref curve => panic!("not a circle: {curve:?}"),
        }
    };
    assert!((radius("a") - 3.0).abs() < 1e-6, "{}", radius("a"));
    assert!((radius("b") - 3.0).abs() < 1e-6, "{}", radius("b"));
    let b = label_id(&data, "b", "id");
    assert!((sketch.curves.iter()).any(|entry| entry.id.get() == b && entry.construction));
    // The second centre on the origin is a point of its own, tied to it.
    assert_ne!(
        label_id(&data, "a", "center"),
        label_id(&data, "b", "center")
    );
}

/// Polygons, arcs, lines and splines draw as their tools do, their
/// labels' parts named by later operations.
#[test]
fn every_shape_draws_with_its_parts() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([
            { "op": "polygon", "center": [50, 50], "sides": 6, "diameter": 20, "label": "p" },
            { "op": "constraint", "kind": "horizontal", "items": ["p.side1"] },
            { "op": "line", "start": "p.corner1", "end": [90, 50], "label": "l" },
            { "op": "arc", "start": "l.end", "end": [110, 50], "through": [100, 60], "label": "a" },
            { "op": "spline", "points": ["a.end", [120, 60], [130, 50]], "label": "s" },
            { "op": "point", "at": [-5, -5], "label": "q" },
            { "op": "rectangle", "center": "q", "size": [4, 2], "label": "r" }
        ]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    // Shared ends are one point.
    assert_eq!(
        label_id(&data, "l", "start"),
        label_id(&data, "p", "corner1")
    );
    assert_eq!(label_id(&data, "a", "start"), label_id(&data, "l", "end"));
    assert_eq!(label_id(&data, "s", "start"), label_id(&data, "a", "end"));
    assert_eq!(label_id(&data, "r", "center"), label_id(&data, "q", "id"));
    for part in ["center", "circle", "side6", "corner6"] {
        label_id(&data, "p", part);
    }
    assert!(data["labels"]["p"].get("side7").is_none());
    label_id(&data, "s", "point3");
    let sketch = drawn(&varde);
    // The polygon's six sides and circle, a line, an arc, a spline, and
    // the rectangle's four sides and its diagonal.
    assert_eq!(sketch.curves.len(), 7 + 1 + 1 + 1 + 5);
    assert!(data["degrees_of_freedom"].as_u64().unwrap() > 0, "{data}");
}

/// A point moved by its id goes there; deleted by id it's gone with what
/// depends on it.
#[test]
fn an_existing_point_moves_and_is_deleted() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, _) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([{ "op": "line", "start": [0, 0], "end": [10, 0], "label": "l" }]),
    );
    let data = edit_data(reply);
    let end = label_id(&data, "l", "end");
    let line = label_id(&data, "l", "id");
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([{ "op": "move", "point": end, "to": ["5 mm", "5 mm"] }]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let sketch = drawn(&varde);
    let moved = (sketch.points.iter())
        .find(|point| point.id.get() == end)
        .unwrap();
    assert!(
        moved.at.distance(glam::DVec2::new(5.0, 5.0)) < 1e-6,
        "{:?}",
        moved.at
    );
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([{ "op": "delete", "items": [line] }]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let sketch = drawn(&varde);
    assert!(
        sketch.curves.is_empty() && sketch.points.is_empty(),
        "{sketch:?}"
    );
}

/// A dimension that would over-constrain the sketch is placed as a
/// reference, as the user's is, and the result says so; the rest
/// applies.
#[test]
fn an_over_constraining_dimension_is_placed_as_a_reference() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([
            { "op": "rectangle", "corner": [0, 0], "size": [20, 10], "label": "r" },
            { "op": "dimension", "kind": "length", "items": ["r.side1"], "value": "25 mm" },
            { "op": "dimension", "kind": "horizontal_distance", "items": ["r.corner1", "r.corner2"] }
        ]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["references"], json!([2]), "{data}");
    assert!(
        data["note"].as_str().unwrap().contains("operations[2]"),
        "{data}"
    );
    let sketch = drawn(&varde);
    let driving: Vec<bool> = (sketch.dimensions.iter())
        .map(|entry| entry.dimension.driving)
        .collect();
    assert_eq!(driving, [true, false]);
    assert!(varde.toast.message().is_none());
}

/// An edit the solver refuses (points fixed apart, then made one) isn't
/// applied, saying which operations, the sketch unchanged.
#[test]
fn an_unsolvable_edit_is_not_applied() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([
            { "op": "point", "at": [0, 0], "label": "p" },
            { "op": "point", "at": [10, 0], "label": "q" },
            { "op": "constraint", "kind": "fix", "items": ["p", "q"] },
            { "op": "constraint", "kind": "coincident", "items": ["p", "q"] }
        ]),
    );
    assert!(reply.is_error, "{}", reply.text);
    assert!(
        reply
            .text
            .starts_with("Not applied: operations[0] to [3]: "),
        "{}",
        reply.text
    );
    assert_eq!(state, McpCallState::NotApplied);
    assert!(drawn(&varde).points.is_empty());
    assert!(!varde.screen.doc().unwrap().proposing());
}

/// A label no earlier operation gave, a part it hasn't, an id the
/// sketch hasn't, a line where a point goes and a constraint that
/// doesn't fit fail at once, naming the operation, the sketch unchanged.
#[test]
fn bad_labels_and_ids_name_the_operation() {
    let (mut varde, requests, mut lane) = on_xy();
    let line = json!({ "op": "line", "start": [0, 0], "end": [10, 0], "label": "l" });
    for (second, why) in [
        (
            json!({ "op": "constraint", "kind": "horizontal", "items": ["m"] }),
            "edit_sketch: operations[1]: no earlier operation is labelled \"m\"",
        ),
        (
            json!({ "op": "constraint", "kind": "horizontal", "items": ["l.middle"] }),
            "operations[1]: \"l.middle\": a line has no such part; its parts are start, end",
        ),
        (
            json!({ "op": "delete", "items": [9999] }),
            "operations[1]: the sketch has no item 9999",
        ),
        (
            json!({ "op": "circle", "center": "l", "diameter": 3 }),
            "operations[1]: that's a line, not a point",
        ),
        (
            json!({ "op": "constraint", "kind": "concentric", "items": ["l"] }),
            "operations[1]: a concentric constraint doesn't fit those items",
        ),
        (
            json!({ "op": "dimension", "kind": "length", "items": ["l"], "value": "-3 mm" }),
            "operations[1]: value: ",
        ),
    ] {
        draw_fails(&mut varde, &requests, &mut lane, json!([line, second]), why);
    }
    // A label deleted by an earlier operation is gone.
    draw_fails(
        &mut varde,
        &requests,
        &mut lane,
        json!([
            line,
            { "op": "delete", "items": ["l"] },
            { "op": "move", "point": "l.start", "to": [1, 1] }
        ]),
        "operations[2]: \"l.start\" was deleted by an earlier operation",
    );
    // A shape too small for its tool is refused.
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([{ "op": "rectangle", "corner": [0, 0], "size": [0, 10] }]),
    );
    assert_eq!(state, McpCallState::NotApplied, "{}", reply.text);
    assert!(
        reply
            .text
            .contains("operations[0]: a rectangle is at least"),
        "{}",
        reply.text
    );
}

/// The user's sketch open: closed for the edit when nothing in it is
/// unfinished, else the popup asks, as for any edit.
#[test]
fn the_users_open_sketch_is_closed_or_asked() {
    let (mut varde, requests, mut lane) = on_xy();
    let sketch = feature_id(&varde, 2);
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    assert!(varde.screen.doc().unwrap().sketch.is_some());
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([{ "op": "point", "at": [1, 2] }]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert!(varde.screen.doc().unwrap().sketch.is_none());
    assert_eq!(drawn(&varde).points.len(), 1);

    // A line half drawn: the popup asks, the call waiting.
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    solve(&mut varde, &mut lane);
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Line),
    );
    place_point(&mut varde, 3.0, 3.0);
    let call = tool_call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": [{ "op": "point", "at": [4, 4] }] }),
    );
    let reply = calling(&mut varde, "a session", 99, call);
    assert!(ask(&varde).is_some());
    assert!(reply.try_recv().is_err());
    assert_eq!(drawn(&varde).points.len(), 1);
}

/// The edit waits behind the user's sketch edit on the solver and is
/// checked as it comes on the sketch with it applied: the user's point,
/// not committed yet, is found by the id it's to get, and the edit
/// applies after it.
#[test]
fn an_edit_queued_behind_the_users_is_checked_on_their_sketch() {
    let (mut varde, requests, mut lane) = on_xy();
    let sketch = feature_id(&varde, 2);
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    solve(&mut varde, &mut lane);
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Point),
    );
    let theirs = drawn(&varde).next_id;
    place_point(&mut varde, 7.0, 7.0);
    assert!(varde.screen.doc().unwrap().proposing());
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([{ "op": "line", "start": theirs, "end": [0, 0], "label": "l" }]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(label_id(&data, "l", "start"), theirs);
    assert_eq!(drawn(&varde).points.len(), 2);
    // An id no one has is refused as it comes.
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    solve(&mut varde, &mut lane);
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Point),
    );
    place_point(&mut varde, 8.0, 8.0);
    let call = tool_call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": [{ "op": "delete", "items": [12345] }] }),
    );
    let reply = calling(&mut varde, "a session", 77, call)
        .try_recv()
        .unwrap();
    assert!(
        reply.text.contains("the sketch has no item 12345"),
        "{}",
        reply.text
    );
}

/// The bounds `varde-mcp` states in `edit_sketch`'s schema, mirrored
/// there as it has no sketch or view dependency, are the app's.
#[test]
fn schema_bounds_are_the_tools() {
    use varde_mcp::tools;
    assert_eq!(tools::SPLINE_POINTS, varde_sketch::MAX_SPLINE_POINTS);
    assert_eq!(tools::MIN_SIDES, varde_view::typed::MIN_SIDES);
    assert_eq!(tools::MAX_SIDES, varde_view::typed::MAX_SIDES);
}

/// The user opens the sketch while the client's edit of it is with the
/// solver and draws a chain of lines: the second line goes on from the
/// first's end, as drawn, though the client's items commit first.
#[test]
fn the_users_chain_drawn_while_the_clients_edit_solves_holds() {
    let (mut varde, requests, mut lane) = on_xy();
    let sketch = feature_id(&varde, 2);
    let call = tool_call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": [
            { "op": "point", "at": [50, 50] },
            { "op": "point", "at": [60, 50] }
        ] }),
    );
    let reply = calling(&mut varde, "a session", 50, call);
    regenerate(&mut varde, &requests);
    assert!(varde.screen.doc().unwrap().proposing());
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Line),
    );
    place_point(&mut varde, 0.0, 0.0);
    place_point(&mut varde, 10.0, 0.0);
    place_point(&mut varde, 10.0, 10.0);
    for _ in 0..3 {
        solve(&mut varde, &mut lane);
        regenerate(&mut varde, &requests);
    }
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    let sketch = drawn(&varde);
    assert_eq!(sketch.points.len(), 5, "{sketch:#?}");
    assert_eq!(sketch.curves.len(), 2, "{sketch:#?}");
    let ends: Vec<_> = (sketch.curves.iter())
        .map(|entry| match entry.curve {
            Curve::Line { start, end } => (start, end),
            _ => panic!("not a line"),
        })
        .collect();
    // The second starts where the first ends.
    assert_eq!(ends[0].1, ends[1].0, "{sketch:#?}");
    let at = |id| sketch.point(id).unwrap().at;
    assert!(at(ends[1].0).distance(glam::DVec2::new(10.0, 0.0)) < 1e-6);
}

/// The client's edit queued behind the user's point, the user reopens
/// the sketch, places a point and draws a line from it, snapped, behind
/// the client's edit: committed first, the client's items take the ids
/// the user's were to get, so theirs aren't made, saying why, rather than
/// the line starting at the client's point.
#[test]
fn the_users_line_from_their_waiting_point_behind_the_clients_queued_edit_is_refused() {
    let (mut varde, requests, mut lane) = on_xy();
    let sketch = feature_id(&varde, 2);
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    solve(&mut varde, &mut lane);
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Point),
    );
    place_point(&mut varde, 7.0, 7.0);
    let call = tool_call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": [
            { "op": "point", "at": [50, 50] },
            { "op": "point", "at": [60, 50] }
        ] }),
    );
    let reply = calling(&mut varde, "a session", 50, call);
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    assert!(varde.screen.doc().unwrap().sketch.is_some());
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Point),
    );
    // Their point, waiting, is the second (the first is with the lane).
    place_point(&mut varde, 0.0, 0.0);
    let theirs = {
        let doc = varde.screen.doc().unwrap();
        let waiting = doc.sketch.as_ref().unwrap().waiting.as_ref().unwrap();
        (waiting.sketch.points.iter())
            .find(|point| point.at == glam::DVec2::ZERO)
            .unwrap()
            .id
    };
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Line),
    );
    let click = |at: glam::DVec2, target| {
        varde_view::Message::Edit(varde_view::Edit::ToolClick(varde_view::ToolClick {
            at,
            target,
            inference: None,
            pixel: 0.1,
            double: false,
            hit: None,
            reference: false,
        }))
    };
    ui(
        &mut varde,
        click(glam::DVec2::ZERO, Some(varde_view::Target::Point(theirs))),
    );
    ui(&mut varde, click(glam::DVec2::new(10.0, 0.0), None));
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Point),
    );
    for _ in 0..4 {
        solve(&mut varde, &mut lane);
        regenerate(&mut varde, &requests);
    }
    let reply = reply.try_recv().unwrap();
    assert!(!reply.is_error, "{}", reply.text);
    // Made on a sketch without the client's items, the user's edits name
    // ids the client's took: not made, saying so, rather than joined to
    // the client's point.
    let sketch = drawn(&varde);
    assert_eq!(sketch.points.len(), 3, "{sketch:#?}");
    assert!(sketch.curves.is_empty(), "{sketch:#?}");
    let doc = varde.screen.doc().unwrap();
    let refused = (doc.sketch.as_ref())
        .and_then(|session| session.refusal.as_ref())
        .or(doc.refused_edit.as_ref().map(|(_, refusal)| refusal));
    assert!(
        matches!(refused, Some(crate::doc::Refusal::Failed(why)) if why.contains("MCP client")),
        "{refused:?}"
    );
}

/// A dimension given no value after a move holds what it measures in
/// the solution the move left, not in the sketch with the point merely
/// put there: the side the drag stretched keeps its length.
#[test]
fn a_dimension_after_a_move_holds_the_solved_length() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([
            { "op": "rectangle", "corner": [0, 0], "size": [20, 10], "label": "r" },
            { "op": "move", "point": "r.corner3", "to": [40, 30] },
            { "op": "dimension", "kind": "length", "items": ["r.side1"] }
        ]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    let sketch = drawn(&varde);
    let side = label_id(&data, "r", "side1");
    let length = length(&sketch, side);
    let held = sketch.dimensions[0].dimension.value.value;
    assert!((length - held).abs() < 1e-6, "{length} {held} {sketch:#?}");
    assert!(
        (length - 20.0).abs() > 1.0,
        "the drag stretched it: {length}"
    );
}

/// The user's Undo between the steps of the client's edit (a point of
/// theirs drawn before it taken back): nothing of the edit is committed,
/// its call answered not applied.
#[test]
fn undo_between_the_steps_commits_nothing() {
    let (mut varde, requests, mut lane) = on_xy();
    let sketch = feature_id(&varde, 2);
    look(&mut varde, varde_view::Look::EditFeature(sketch));
    solve(&mut varde, &mut lane);
    look(
        &mut varde,
        varde_view::Look::SelectTool(varde_view::Tool::Point),
    );
    place_point(&mut varde, 7.0, 7.0);
    solve(&mut varde, &mut lane);
    look(&mut varde, varde_view::Look::Escape);
    let before = drawn(&varde);
    assert_eq!(before.points.len(), 1);
    let call = tool_call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": [
            { "op": "line", "start": [0, 0], "end": [10, 0], "label": "l" },
            { "op": "dimension", "kind": "length", "items": ["l"], "value": "20" },
            { "op": "point", "at": [3, 3] }
        ] }),
    );
    let reply = calling(&mut varde, "a session", 60, call);
    regenerate(&mut varde, &requests);
    // The first step answered, the second with the lane.
    let id = varde.screen.doc().unwrap().id;
    let response = lane.respond().unwrap();
    let _ = varde.update(Message::Doc(id, crate::message::ForDoc::Solved(response)));
    assert!(varde.screen.doc().unwrap().proposing());
    undo(&mut varde);
    for _ in 0..3 {
        solve(&mut varde, &mut lane);
        regenerate(&mut varde, &requests);
    }
    let reply = reply.try_recv().unwrap();
    assert!(reply.is_error, "{}", reply.text);
    assert!(drawn(&varde).curves.is_empty(), "{:?}", drawn(&varde));
}

/// A spline's points as the Spline tool takes clicks: two in a row at
/// one place, a point given twice, or the first again on an open one are
/// refused, as the tool refuses the click, rather than a spline with a
/// piece of no length.
#[test]
fn spline_points_the_tool_refuses_are_not_applied() {
    let (mut varde, requests, mut lane) = on_xy();
    for operations in [
        json!([{ "op": "spline", "points": [[0, 0], [0, 0], [10, 0]] }]),
        json!([{ "op": "spline", "points": [[0, 0], [5, 5], [0, 0]] }]),
        json!([
            { "op": "point", "at": [1, 1], "label": "p" },
            { "op": "spline", "points": ["p", [10, 0], "p", [5, 5]] }
        ]),
    ] {
        let before = drawn(&varde);
        let (reply, state) = draw(&mut varde, &requests, &mut lane, operations);
        assert_eq!(state, McpCallState::NotApplied, "{}", reply.text);
        assert!(reply.text.contains("Spline tool"), "{}", reply.text);
        assert_eq!(drawn(&varde), before);
    }
    let (reply, state) = draw(
        &mut varde,
        &requests,
        &mut lane,
        json!([{ "op": "spline", "points": [[0, 0], [10, 0], [10, 10]], "closed": true }]),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
}

/// The client's edit `operations` called, its first step answered and
/// the second with the lane, then `between` done, then all solved: the
/// call's reply.
fn between_the_steps(
    varde: &mut Varde,
    requests: &Lane,
    lane: &mut SolveLane,
    between: impl FnOnce(&mut Varde),
) -> ToolReply {
    let call = tool_call(
        "edit_sketch",
        json!({ "sketch": "Sketch 2", "operations": [
            { "op": "line", "start": [0, 0], "end": [10, 0], "label": "l" },
            { "op": "dimension", "kind": "length", "items": ["l"], "value": "20" },
            { "op": "point", "at": [3, 3] }
        ] }),
    );
    let reply = calling(varde, "a session", 60, call);
    regenerate(varde, requests);
    let id = varde.screen.doc().unwrap().id;
    let response = lane.respond().unwrap();
    let _ = varde.update(Message::Doc(id, crate::message::ForDoc::Solved(response)));
    assert!(varde.screen.doc().unwrap().proposing());
    between(varde);
    for _ in 0..3 {
        solve(varde, lane);
        regenerate(varde, requests);
    }
    reply.try_recv().unwrap()
}

/// The user's Undo of another feature between the steps of the client's
/// edit leaves the edit's sketch as it was: the edit is committed.
#[test]
fn undo_of_another_feature_between_the_steps_commits_the_edit() {
    let (mut varde, requests, mut lane) = on_xy();
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "add_sketch",
        json!({ "plane": "yz" }),
    );
    edit_data(reply);
    let features = |varde: &Varde| {
        varde
            .screen
            .doc()
            .unwrap()
            .editor
            .document()
            .features()
            .len()
    };
    let with = features(&varde);
    let reply = between_the_steps(&mut varde, &requests, &mut lane, undo);
    assert!(!reply.is_error, "{}", reply.text);
    assert_eq!(features(&varde), with - 1);
    let sketch = drawn(&varde);
    assert_eq!(sketch.curves.len(), 1, "{sketch:?}");
    assert_eq!(sketch.points.len(), 3, "{sketch:?}");
    let line = sketch.curves[0].id.get();
    assert!((length(&sketch, line) - 20.0).abs() < 1e-6);
}

/// The user's edit of the edit's own sketch between its steps: nothing
/// of the edit is committed, its call answered that the sketch changed.
#[test]
fn an_edit_of_the_same_sketch_between_the_steps_fails() {
    let (mut varde, requests, mut lane) = on_xy();
    let sketch = feature_id(&varde, 2);
    let reply = between_the_steps(&mut varde, &requests, &mut lane, |varde| {
        let mut changed = drawn(varde);
        changed.next_id += 1;
        let doc = varde.screen.doc_mut().unwrap();
        doc.editor
            .apply(varde_document::Command::SetSketch {
                feature: sketch,
                sketch: Box::new(changed),
            })
            .unwrap();
    });
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.contains("the sketch changed"), "{}", reply.text);
    assert!(drawn(&varde).curves.is_empty(), "{:?}", drawn(&varde));
}

/// The edit's sketch removed between its steps (the user's Undo of
/// adding it): the call is answered that it was removed.
#[test]
fn the_sketch_removed_between_the_steps_fails() {
    let (mut varde, requests, mut lane) = on_xy();
    let reply = between_the_steps(&mut varde, &requests, &mut lane, undo);
    assert!(reply.is_error, "{}", reply.text);
    assert!(reply.text.contains("removed"), "{}", reply.text);
    let document = varde.screen.doc().unwrap().editor.document();
    assert!(document.features().iter().all(|f| f.name != "Sketch 2"));
}
