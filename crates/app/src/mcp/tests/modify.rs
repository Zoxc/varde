//! The tools `split`, `shell`, `offset_face` and `draft`, through
//! `Varde::update`: the panel's defaults, values given, keys taken back
//! or refused, values past their bounds, and what only the panel
//! refuses, said. The kernel builds none of the four yet, so each
//! applies and fails in regeneration, as for the user.

use glam::DVec3;
use serde_json::json;
use varde_document::{FeatureKind, Keep, OriginPlane, PlaneRef, Side, SplitTool};

use super::motion::{
    applied, applied_last, bodies, corner_at, edge_between, face_facing, face_of_form, moved_key,
    plate_made_symmetric, with_disc,
};
use super::*;

/// The id of a line of the example's sketch.
fn a_line(varde: &Varde) -> u32 {
    let document = varde.screen.doc().unwrap().editor.document();
    let sketch = document.features()[0].id;
    let drawn = crate::doc::sketch_of(document, sketch).unwrap();
    let line = (drawn.curves.iter())
        .find(|curve| matches!(curve.curve, varde_sketch::Curve::Line { .. }))
        .unwrap();
    line.id.get()
}

/// `split` by an origin plane keeps both pieces, the front keeping the
/// body's id, as the panel starts; by a face, a body, regions or a line
/// as each tile picks them, with Keeps and Keep given.
#[test]
fn split_takes_each_tool() {
    let (mut varde, requests) = with_disc();
    let [plate, disc] = bodies(&varde)[..] else {
        panic!("two bodies");
    };
    let feature = |varde: &mut Varde, arguments| {
        let kind = applied_last(varde, &requests, "split", arguments);
        let FeatureKind::Split(split) = kind else {
            panic!("not a split: {kind:?}");
        };
        undo(varde);
        regenerate(varde, &requests);
        split
    };
    let split = feature(&mut varde, json!({ "body": "Body 1", "tool": "XY" }));
    assert_eq!(split.body, plate);
    assert_eq!(
        split.tool,
        SplitTool::Plane(PlaneRef::Origin(OriginPlane::XY))
    );
    assert_eq!((split.original, split.keep), (Side::Front, Keep::Both));
    assert!(split.new_body.is_some());

    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let split = feature(
        &mut varde,
        json!({ "body": "Body 1", "tool": { "face": top }, "original": "back" }),
    );
    assert!(matches!(split.tool, SplitTool::Plane(PlaneRef::Face(face)) if face.body == plate));
    assert_eq!(split.original, Side::Back);

    let hole = face_of_form(&varde, "Body 1", "cylinder");
    let split = feature(
        &mut varde,
        json!({ "body": "Body 1", "tool": { "face": hole }, "keep": "front" }),
    );
    assert!(matches!(split.tool, SplitTool::Face(face) if face.body == plate));
    assert_eq!(split.keep, Keep::Front);
    assert!(split.new_body.is_none());

    let split = feature(
        &mut varde,
        json!({ "body": "Body 1", "tool": { "body": "Body 2" }, "keep": "back" }),
    );
    assert_eq!(split.tool, SplitTool::Body(disc));
    assert_eq!(varde.mcp.log.back().unwrap().args, "Body 1 by Body 2");

    let (disc_region, _) = disc_and_plate(&varde);
    let split = feature(
        &mut varde,
        json!({ "body": "Body 1", "tool": { "sketch": "Sketch 1", "profiles": [disc_region] } }),
    );
    assert!(matches!(&split.tool, SplitTool::Regions { regions, .. } if regions.len() == 1));

    let line = a_line(&varde);
    let split = feature(
        &mut varde,
        json!({ "body": "Body 1", "tool": { "sketch": "Sketch 1", "curves": [line] } }),
    );
    assert!(matches!(&split.tool, SplitTool::Chain { curves, .. }
        if curves.iter().map(|id| id.get()).eq([line])));
    assert_eq!(
        varde.mcp.log.back().unwrap().args,
        "Body 1 by Sketch 1's 1 curve"
    );
}

/// What the Split panel refuses, the call refuses saying why: the body
/// itself as the tool, Keeps for a trim, an edge's key as a face, a
/// line's curve not in the sketch or named twice, a region past the
/// sketch's, a body made after the rollback marker.
#[test]
fn split_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_disc();
    let edge = edge_between(&varde, "Body 1", [30.0, 20.0, 10.0], [30.0, 20.0, 0.0]);
    let line = a_line(&varde);
    for (arguments, why) in [
        (
            json!({ "body": "Body 1", "tool": { "body": "Body 1" } }),
            "split: tool: That's the body being split: pick another body to split with",
        ),
        (
            json!({ "body": "Body 1", "tool": "xy", "keep": "back", "original": "front" }),
            "split: original goes with keep both",
        ),
        (
            json!({ "body": "Body 1", "tool": { "face": edge } }),
            "split: tool: that key names an edge, not a face",
        ),
        (
            json!({ "body": "Body 1", "tool": { "sketch": "Sketch 1", "curves": [9999] } }),
            "split: tool.curves: the sketch has no curve 9999",
        ),
        (
            json!({ "body": "Body 1", "tool": { "sketch": "Sketch 1", "curves": [line, line] } }),
            &format!("split: tool.curves: curve {line} is named twice"),
        ),
        (
            json!({ "body": "Body 1", "tool": { "sketch": "Sketch 1", "profiles": [99] } }),
            "split: tool.profiles: the sketch has 2 regions: index 99 is past them",
        ),
        (
            json!({ "body": "Body 1", "tool": { "sketch": "Extrude 1", "profiles": "all" } }),
            "\"Extrude 1\" isn't a sketch",
        ),
        (
            json!({ "body": "Body 9", "tool": "xy" }),
            "no body is named \"Body 9\"",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "split", arguments, why);
    }
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
        "split",
        json!({ "body": "Body 1", "tool": { "body": "Body 2" } }),
        "\"Body 2\" is made after where the split goes",
    );
}

/// `shell` with nothing given hollows the model's only body closed, 2 mm
/// inward (the panel's thickness follows the view); faces by key open it,
/// their body its body, with a thickness and direction given.
#[test]
fn shell_takes_its_defaults_or_faces() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    let FeatureKind::Shell(shell) = applied(&mut varde, &requests, "shell", json!({})) else {
        panic!("not a shell");
    };
    assert_eq!(shell.body, plate);
    assert!(shell.open.is_empty());
    assert_eq!(shell.thickness.value, 2.0);
    assert!(!shell.outward);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let side = face_facing(&varde, "Body 1", [1.0, 0.0, 0.0]);
    let FeatureKind::Shell(shell) = applied(
        &mut varde,
        &requests,
        "shell",
        json!({ "faces": [top, side], "thickness": "1 mm", "direction": "outward" }),
    ) else {
        panic!("not a shell");
    };
    assert_eq!(shell.body, plate);
    assert_eq!(shell.open.len(), 2);
    assert!(shell.open.iter().all(|face| face.body == plate));
    assert_eq!(shell.thickness.value, 1.0);
    assert!(shell.outward);
    assert_eq!(varde.mcp.log.back().unwrap().args, "2 faces, 1 mm");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::Shell(shell) = applied(
        &mut varde,
        &requests,
        "shell",
        json!({ "body": "Body 1", "thickness": "0.5" }),
    ) else {
        panic!("not a shell");
    };
    assert_eq!(shell.body, plate);
    assert_eq!(shell.thickness.value, 0.5);
}

/// What the Shell panel refuses, the call refuses saying why: a body
/// beside faces, no body to take among two, faces of two bodies, a face
/// named twice, a key of another kind or stale, a thickness its field
/// refuses.
#[test]
fn shell_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_disc();
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let disc_top = face_facing(&varde, "Body 2", [0.0, 0.0, 1.0]);
    let corner = corner_at(&varde, "Body 1", [30.0, 20.0, 10.0]);
    for (arguments, why) in [
        (
            json!({ "body": "Body 1", "faces": [top.clone()] }),
            "shell: body goes with no faces",
        ),
        (
            json!({}),
            "shell: pick faces to remove, or the body to hollow",
        ),
        (
            json!({ "faces": [top.clone(), disc_top] }),
            "shell: faces[1]: A shell's faces are all on one body: pick faces of Body 1",
        ),
        (
            json!({ "faces": [top.clone(), top.clone()] }),
            "shell: faces[1]: that face is named twice",
        ),
        (
            json!({ "faces": [corner] }),
            "shell: faces[0]: that key names a corner, not a face",
        ),
        (
            json!({ "faces": [top.clone()], "thickness": "-1" }),
            "shell: thickness: ",
        ),
        (
            json!({ "faces": [top.clone()], "thickness": "nothing" }),
            "shell: thickness: ",
        ),
        (json!({ "body": "Body 9" }), "no body is named \"Body 9\""),
    ] {
        wrong_edit(&mut varde, &requests, "shell", arguments, why);
    }
    let off = moved_key(&varde, &top, DVec3::new(0.0, 0.0, 50.0));
    wrong_edit(
        &mut varde,
        &requests,
        "shell",
        json!({ "faces": [off] }),
        "shell: faces[0]: that face isn't in the model any more: ask body_topology again",
    );
}

/// `offset_face` moves its faces 1 mm out with tangent faces taken in
/// unless told (the panel's distance follows the view); a distance,
/// Inward and Tangent faces given are taken; what its fields and keys
/// refuse is said.
#[test]
fn offset_face_takes_its_faces_and_values() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let FeatureKind::OffsetFace(offset) = applied(
        &mut varde,
        &requests,
        "offset_face",
        json!({ "faces": [top.clone()] }),
    ) else {
        panic!("not an offset face");
    };
    assert_eq!(offset.faces.len(), 1);
    assert_eq!(offset.faces[0].body, plate);
    assert_eq!(offset.distance.value, 1.0);
    assert!(!offset.inward);
    assert!(offset.tangent);
    assert_eq!(varde.mcp.log.back().unwrap().args, "1 face");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::OffsetFace(offset) = applied(
        &mut varde,
        &requests,
        "offset_face",
        json!({ "faces": [top.clone()], "distance": "3", "inward": true, "tangent": false }),
    ) else {
        panic!("not an offset face");
    };
    assert_eq!(offset.distance.value, 3.0);
    assert!(offset.inward);
    assert!(!offset.tangent);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    for (arguments, why) in [
        (
            json!({ "faces": [top.clone()], "distance": "0" }),
            "offset_face: distance: ",
        ),
        (
            json!({ "faces": [top.clone()], "distance": "2000000" }),
            "offset_face: distance: ",
        ),
        (
            json!({ "faces": ["nonsense"] }),
            "offset_face: faces[0]: that isn't a key Varde gave",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "offset_face", arguments, why);
    }
    plate_made_symmetric(&mut varde, &requests);
    wrong_edit(
        &mut varde,
        &requests,
        "offset_face",
        json!({ "faces": [top] }),
        "offset_face: faces[0]: that face isn't in the model any more",
    );
}

/// `draft` tilts its faces 3° from XY with tangent faces taken in unless
/// told; a neutral plane by word or flat face by key, an angle, Flip and
/// Tangent faces given are taken; one of its faces as the neutral plane,
/// a round face, or an angle its field refuses is said.
#[test]
fn draft_takes_its_faces_plane_and_values() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    let side = face_facing(&varde, "Body 1", [1.0, 0.0, 0.0]);
    let bottom = face_facing(&varde, "Body 1", [0.0, 0.0, -1.0]);
    let FeatureKind::FaceDraft(draft) = applied(
        &mut varde,
        &requests,
        "draft",
        json!({ "faces": [side.clone()] }),
    ) else {
        panic!("not a draft");
    };
    assert_eq!(draft.faces[0].body, plate);
    assert_eq!(draft.neutral, PlaneRef::Origin(OriginPlane::XY));
    assert!((draft.angle.value - 3f64.to_radians()).abs() < 1e-12);
    assert!(!draft.flip);
    assert!(draft.tangent);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::FaceDraft(draft) = applied(
        &mut varde,
        &requests,
        "draft",
        json!({ "faces": [side.clone()], "neutral": { "face": bottom.clone() }, "angle": "5",
                "flip": true, "tangent": false }),
    ) else {
        panic!("not a draft");
    };
    assert!(matches!(draft.neutral, PlaneRef::Face(face) if face.body == plate));
    assert!((draft.angle.value - 5f64.to_radians()).abs() < 1e-12);
    assert!(draft.flip);
    assert!(!draft.tangent);
    assert_eq!(varde.mcp.log.back().unwrap().args, "1 face, from a face, 5");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::FaceDraft(draft) = applied(
        &mut varde,
        &requests,
        "draft",
        json!({ "faces": [side.clone()], "neutral": "yz" }),
    ) else {
        panic!("not a draft");
    };
    assert_eq!(draft.neutral, PlaneRef::Origin(OriginPlane::YZ));
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let hole = face_of_form(&varde, "Body 1", "cylinder");
    for (arguments, why) in [
        (
            json!({ "faces": [side.clone(), bottom.clone()], "neutral": { "face": bottom.clone() } }),
            "draft: neutral: That face is one the draft tilts",
        ),
        (
            json!({ "faces": [side.clone()], "neutral": { "face": hole } }),
            "draft: neutral: Only a flat face can be the neutral plane",
        ),
        (
            json!({ "faces": [side.clone()], "angle": "90" }),
            "draft: angle: ",
        ),
        (
            json!({ "faces": [side.clone()], "angle": "0" }),
            "draft: angle: ",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "draft", arguments, why);
    }
}
