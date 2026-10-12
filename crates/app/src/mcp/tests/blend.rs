//! The tools `chamfer` and `fillet`, through `Varde::update`: the
//! panel's defaults, values given, edges and faces by key taken back or
//! refused, tangent chains grown as the panel's tick grows a click,
//! values past their bounds, and what only the panel refuses, said. The
//! kernel blends nothing yet, so each applies and fails in
//! regeneration, as for the user.

use glam::DVec2;
use serde_json::json;
use varde_document::{ChamferSize, Command, FeatureKind, OriginPlane, Plane};
use varde_sketch::{Curve, Sketch};

use super::motion::{
    applied, bodies, corner_at, edge_between, face_facing, key_of, last_kind, moved_key,
    plate_made_symmetric, with_disc,
};
use super::*;

/// The plate's top edge along its back, at y 20.
const BACK: ([f64; 3], [f64; 3]) = ([-30.0, 20.0, 10.0], [30.0, 20.0, 10.0]);

/// The plate's top edge along its right, at x 30.
const RIGHT: ([f64; 3], [f64; 3]) = ([30.0, 20.0, 10.0], [30.0, -20.0, 10.0]);

/// Adds a sketch on `plane` drawn by `draw`, as the user would: its
/// Timeline name.
pub(super) fn sketched(
    varde: &mut Varde,
    requests: &Lane,
    plane: OriginPlane,
    draw: impl FnOnce(&mut Sketch),
) -> String {
    let doc = varde.screen.doc_mut().unwrap();
    doc.apply(doc.editor.document().add_sketch(Plane::Origin(plane)));
    let feature = doc.editor.document().features().last().unwrap().clone();
    let mut sketch = Sketch::default();
    draw(&mut sketch);
    doc.apply(Command::SetSketch {
        feature: feature.id,
        sketch: Box::new(sketch),
    });
    doc.sync();
    regenerate(varde, requests);
    feature.name
}

/// The example with a slot beside the plate (x 40 to 50, 6 wide, its
/// lines running on into half circles), 5 tall, as Body 2.
fn with_slot() -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let name = sketched(&mut varde, &requests, OriginPlane::XY, |sketch| {
        let mut at = |x: f64, y: f64| sketch.add_point(DVec2::new(x, y)).unwrap();
        let [a, b, c, d] =
            [(40.0, -3.0), (50.0, -3.0), (50.0, 3.0), (40.0, 3.0)].map(|(x, y)| at(x, y));
        let [left, right] = [(40.0, 0.0), (50.0, 0.0)].map(|(x, y)| at(x, y));
        for curve in [
            Curve::Line { start: a, end: b },
            Curve::Arc {
                center: right,
                start: b,
                end: c,
            },
            Curve::Line { start: c, end: d },
            Curve::Arc {
                center: left,
                start: d,
                end: a,
            },
        ] {
            sketch.add_curve(curve, false).unwrap();
        }
    });
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": name, "profiles": "all", "distance": "5" }),
    );
    edit_data(reply);
    (varde, requests)
}

/// The slot's top front line, and its top right half circle.
fn slot_top(varde: &Varde) -> (String, String) {
    let line = edge_between(varde, "Body 2", [40.0, -3.0, 5.0], [50.0, -3.0, 5.0]);
    let arc = key_of(varde, "Body 2", "edges", |edge| {
        edge["form"] == json!("circle") && edge["centre"] == json!([50.0, 0.0, 5.0])
    });
    (line, arc)
}

/// `chamfer` with an edge alone is the panel's: equal, 1 mm (its own
/// follows the view), stored unflipped; two distances, a distance and
/// angle, Flip sides and faces given are taken, one undo step each.
#[test]
fn chamfer_takes_its_defaults_and_values() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    let back = edge_between(&varde, "Body 1", BACK.0, BACK.1);
    let right = edge_between(&varde, "Body 1", RIGHT.0, RIGHT.1);
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let FeatureKind::Chamfer(chamfer) = applied(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [back.clone()] }),
    ) else {
        panic!("not a chamfer");
    };
    assert_eq!(chamfer.edges.len(), 1);
    assert_eq!(chamfer.edges[0].body, plate);
    assert!(chamfer.faces.is_empty());
    assert!(matches!(&chamfer.distances, ChamferSize::Equal(d) if d.value == 1.0));
    assert!(!chamfer.chains && !chamfer.flip);
    assert_eq!(varde.mcp.log.back().unwrap().args, "1 edge");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::Chamfer(chamfer) = applied(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [back.clone(), right.clone()], "type": "two_distances" }),
    ) else {
        panic!("not a chamfer");
    };
    assert_eq!(chamfer.edges.len(), 2);
    assert!(
        matches!(&chamfer.distances, ChamferSize::Two(a, b) if a.value == 1.0 && b.value == 2.0)
    );
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::Chamfer(chamfer) = applied(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [back.clone()], "type": "distance_angle", "distance": "1.5",
                "angle": "30", "flip": true }),
    ) else {
        panic!("not a chamfer");
    };
    assert!(matches!(&chamfer.distances, ChamferSize::Angle(d, a)
        if d.value == 1.5 && (a.value - 30f64.to_radians()).abs() < 1e-12));
    assert!(chamfer.flip);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::Chamfer(chamfer) = applied(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "faces": [top.clone()], "distance": "0.5 mm" }),
    ) else {
        panic!("not a chamfer");
    };
    assert!(chamfer.edges.is_empty());
    assert_eq!(chamfer.faces.len(), 1);
    assert_eq!(chamfer.faces[0].body, plate);
    assert_eq!(varde.mcp.log.back().unwrap().args, "1 face, 0.5 mm");
    // Applied, and failed in regeneration as the user's would.
    let document = varde.screen.doc().unwrap().editor.document();
    assert!(matches!(
        document.features().last().unwrap().kind,
        FeatureKind::Chamfer(_)
    ));
}

/// What the Chamfer panel refuses, the call refuses saying why: a value
/// of another type, Flip for equal, edges of two bodies, an edge named
/// twice, a key of another kind, off its edge or stale, values past
/// their bounds, an edge of a body made after the rollback marker.
#[test]
fn chamfer_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_disc();
    let back = edge_between(&varde, "Body 1", BACK.0, BACK.1);
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let disc_top = face_facing(&varde, "Body 2", [0.0, 0.0, 1.0]);
    let disc_rim = key_of(&varde, "Body 2", "edges", |edge| {
        edge["form"] == json!("circle")
    });
    let corner = corner_at(&varde, "Body 1", [30.0, 20.0, 10.0]);
    for (arguments, why) in [
        (
            json!({ "edges": [back.clone()], "second_distance": "2" }),
            "chamfer: second_distance goes with type two_distances",
        ),
        (
            json!({ "edges": [back.clone()], "type": "two_distances", "angle": "20" }),
            "chamfer: angle goes with type distance_angle",
        ),
        (
            json!({ "edges": [back.clone()], "flip": true }),
            "chamfer: flip goes with type two_distances or distance_angle",
        ),
        (
            json!({ "edges": [back.clone(), disc_rim.clone()] }),
            "chamfer: edges[1]: A chamfer's edges are all on one body: pick edges of Body 1",
        ),
        (
            json!({ "edges": [back.clone()], "faces": [disc_top] }),
            "chamfer: faces[0]: A chamfer's faces are all on one body: pick faces of Body 1",
        ),
        (
            json!({ "edges": [back.clone(), back.clone()] }),
            "chamfer: edges[1]: that edge is named twice",
        ),
        (
            json!({ "faces": [top.clone(), top.clone()] }),
            "chamfer: faces[1]: that face is named twice",
        ),
        (
            json!({ "edges": [top.clone()] }),
            "chamfer: edges[0]: that key names a face, not an edge",
        ),
        (
            json!({ "edges": [corner] }),
            "chamfer: edges[0]: that key names a corner, not an edge",
        ),
        (
            json!({ "edges": [back.clone()], "distance": "0" }),
            "chamfer: distance: ",
        ),
        (
            json!({ "edges": [back.clone()], "distance": "2000000" }),
            "chamfer: distance: ",
        ),
        (
            json!({ "edges": [back.clone()], "type": "distance_angle", "angle": "90" }),
            "chamfer: angle: ",
        ),
        (
            json!({ "edges": [back.clone()], "type": "two_distances", "second_distance": "x" }),
            "chamfer: second_distance: ",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "chamfer", arguments, why);
    }
    let off = moved_key(&varde, &back, glam::DVec3::new(0.0, 5.0, 10.0));
    wrong_edit(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [off] }),
        "chamfer: edges[0]: ",
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Extrude 2" }),
    );
    edit_data(reply);
    wrong_edit(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [disc_rim] }),
        "chamfer: edges[0]: ",
    );
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": null }),
    );
    edit_data(reply);
    plate_made_symmetric(&mut varde, &requests);
    wrong_edit(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [back] }),
        "chamfer: edges[0]: that edge isn't in the model any more",
    );
}

/// With Tangent chain on, as the panel starts, an edge takes in its
/// tangent chain, each edge of its own (the chamfer stores them, chains
/// off); off, the edge alone. Another edge of a chain named is refused,
/// as a click on it would take it out.
#[test]
fn a_blend_takes_in_tangent_chains_as_the_panel_does() {
    let (mut varde, requests) = with_slot();
    let slot = bodies(&varde)[1];
    let (line, arc) = slot_top(&varde);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "chamfer",
        json!({ "edges": [line.clone()] }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Chamfer(chamfer) = last_kind(&varde) else {
        panic!("not a chamfer");
    };
    // The slot's top rim: two lines and two half circles.
    assert_eq!(chamfer.edges.len(), 4);
    assert!(chamfer.edges.iter().all(|edge| edge.body == slot));
    assert!(!chamfer.chains);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let (reply, state) = edit(
        &mut varde,
        &requests,
        "fillet",
        json!({ "edges": [line.clone()], "tangent": false }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Fillet(fillet) = last_kind(&varde) else {
        panic!("not a fillet");
    };
    assert_eq!(fillet.edges.len(), 1);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let (reply, state) = edit(
        &mut varde,
        &requests,
        "fillet",
        json!({ "edges": [line.clone(), arc.clone()], "tangent": false }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Fillet(fillet) = last_kind(&varde) else {
        panic!("not a fillet");
    };
    assert_eq!(fillet.edges.len(), 2);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    wrong_edit(
        &mut varde,
        &requests,
        "fillet",
        json!({ "edges": [line, arc] }),
        "fillet: edges[1]: that edge is named twice, or is in the tangent chain of one named",
    );
}

/// `fillet` with an edge alone rounds it 2 mm (the panel's radius follows
/// the view); a radius and faces given are taken; what its field and
/// keys refuse is said.
#[test]
fn fillet_takes_its_edges_and_radius() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    let back = edge_between(&varde, "Body 1", BACK.0, BACK.1);
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let FeatureKind::Fillet(fillet) = applied(
        &mut varde,
        &requests,
        "fillet",
        json!({ "edges": [back.clone()] }),
    ) else {
        panic!("not a fillet");
    };
    assert_eq!(fillet.edges[0].body, plate);
    assert_eq!(fillet.radius.value, 2.0);
    assert!(!fillet.chains);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::Fillet(fillet) = applied(
        &mut varde,
        &requests,
        "fillet",
        json!({ "edges": [back.clone()], "faces": [top.clone()], "radius": "3" }),
    ) else {
        panic!("not a fillet");
    };
    assert_eq!((fillet.edges.len(), fillet.faces.len()), (1, 1));
    assert_eq!(fillet.radius.value, 3.0);
    assert_eq!(varde.mcp.log.back().unwrap().args, "1 edge, 1 face, 3");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    for (arguments, why) in [
        (
            json!({ "edges": [back.clone()], "radius": "-1" }),
            "fillet: radius: ",
        ),
        (
            json!({ "edges": [back.clone()], "radius": "nothing" }),
            "fillet: radius: ",
        ),
        (
            json!({ "edges": ["nonsense"] }),
            "fillet: edges[0]: that isn't a key Varde gave",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "fillet", arguments, why);
    }
}
