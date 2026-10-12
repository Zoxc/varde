//! The motion tools, `move_body`, `mirror`, `pattern`, `align` and
//! `scale`, through `Varde::update`: the panel's defaults, values given,
//! keys taken back or refused, values past their bounds, and what only
//! the panel refuses, said.

use glam::DVec3;
use serde_json::{Value, json};
use varde_document::{
    AxisRef, BodyId, Copies, DirRef, FeatureKind, PatternKind, PlaneRef, PointRef, ScaleFactor,
};

use super::*;

/// The items of `body`'s `list` (faces, edges or corners) in `varde`'s
/// committed model.
pub(super) fn items(varde: &Varde, body: &str, list: &str) -> Vec<Value> {
    let listed = data(varde, &topology_call(json!({ "body": body, "limit": 50 })));
    listed[list]["items"].as_array().unwrap().clone()
}

/// The key of the first of `body`'s `list` that `wanted` takes.
pub(super) fn key_of(
    varde: &Varde,
    body: &str,
    list: &str,
    wanted: impl Fn(&Value) -> bool,
) -> String {
    let items = items(varde, body, list);
    let item = items.iter().find(|item| wanted(item)).unwrap();
    item["key"].as_str().unwrap().to_owned()
}

/// The key of `body`'s flat face facing `normal`.
pub(super) fn face_facing(varde: &Varde, body: &str, normal: [f64; 3]) -> String {
    key_of(varde, body, "faces", |face| face["normal"] == json!(normal))
}

/// The key of `body`'s face of the form `form`.
pub(super) fn face_of_form(varde: &Varde, body: &str, form: &str) -> String {
    key_of(varde, body, "faces", |face| face["form"] == json!(form))
}

/// The key of `body`'s round rim at height `z`.
fn rim_at(varde: &Varde, body: &str, z: f64) -> String {
    key_of(varde, body, "edges", |edge| {
        edge["form"] == json!("circle") && number(&edge["centre"][2]) == z
    })
}

/// The key of `body`'s corner at `point`.
pub(super) fn corner_at(varde: &Varde, body: &str, point: [f64; 3]) -> String {
    key_of(varde, body, "corners", |corner| {
        corner["point"] == json!(point)
    })
}

/// The key of `body`'s straight edge from `from` to `to`, either way.
pub(super) fn edge_between(varde: &Varde, body: &str, from: [f64; 3], to: [f64; 3]) -> String {
    key_of(varde, body, "edges", |edge| {
        let ends = &edge["ends"];
        (ends[0] == json!(from) && ends[1] == json!(to))
            || (ends[0] == json!(to) && ends[1] == json!(from))
    })
}

/// The bodies of `varde`'s design, in order.
pub(super) fn bodies(varde: &Varde) -> Vec<BodyId> {
    let document = varde.screen.doc().unwrap().editor.document();
    document.bodies().iter().map(|body| body.id).collect()
}

/// The kind of the feature at `at`.
fn kind_at(varde: &Varde, at: usize) -> FeatureKind {
    feature_at(varde, at).kind
}

/// `tool` applied with `arguments`: its row applied, its data, the
/// feature added third, after the example's two.
pub(super) fn applied(
    varde: &mut Varde,
    requests: &Lane,
    tool: &str,
    arguments: Value,
) -> FeatureKind {
    let (reply, state) = edit(varde, requests, tool, arguments);
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    let feature = feature_at(varde, 2);
    assert_eq!(data["feature"], json!(feature.name));
    assert_eq!(data["id"], json!(feature.id.get()));
    feature.kind
}

/// `tool` applied with `arguments`: its row applied, its data, the
/// feature added last.
pub(super) fn applied_last(
    varde: &mut Varde,
    requests: &Lane,
    tool: &str,
    arguments: Value,
) -> FeatureKind {
    let (reply, state) = edit(varde, requests, tool, arguments);
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let data = edit_data(reply);
    assert_eq!(data["feature"], json!(last_feature(varde).name));
    last_feature(varde).kind
}

/// The kind of the feature added last.
pub(super) fn last_kind(varde: &Varde) -> FeatureKind {
    last_feature(varde).kind
}

/// The feature added last.
fn last_feature(varde: &Varde) -> varde_document::Feature {
    let document = varde.screen.doc().unwrap().editor.document();
    document.features().last().unwrap().clone()
}

/// The example with its hole's region extruded 20 mm as Body 2, a disc
/// standing in the hole.
pub(super) fn with_disc() -> (Varde, Lane) {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let (disc, _) = disc_and_plate(&varde);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": [disc], "distance": "20" }),
    );
    edit_data(reply);
    (varde, requests)
}

/// The plate made symmetric about its sketch: its faces, edges and
/// corners off where keys read before have them.
pub(super) fn plate_made_symmetric(varde: &mut Varde, requests: &Lane) {
    let doc = varde.screen.doc_mut().unwrap();
    let plate = doc.editor.document().features()[1].clone();
    let mut extrude = extrude_of(&plate);
    let varde_document::Extent::OneSide(distance) = extrude.extent.clone() else {
        panic!();
    };
    extrude.extent = varde_document::Extent::Symmetric(distance);
    doc.apply(Command::SetFeature {
        feature: plate.id,
        kind: Box::new(extrude.into()),
    });
    doc.sync();
    regenerate(varde, requests);
}

/// A key re-encoded with its point `near`, at `varde`'s design's
/// generation now.
pub(super) fn moved_key(varde: &Varde, key: &str, near: DVec3) -> String {
    let doc = varde.screen.doc().unwrap();
    let item = match key::decode(key).unwrap().item {
        Ref::Face(face) => Ref::Face(varde_document::FaceRef { near, ..face }),
        Ref::Edge(edge) => Ref::Edge(varde_document::EdgeRef { near, ..edge }),
        Ref::Corner { body, faces, .. } => Ref::Corner {
            body,
            faces,
            point: near.to_array(),
        },
    };
    Key::new(doc.id, doc.editor.generation(), item).encode()
}

/// `move_body` with its bodies alone moves nothing, which the panel's OK
/// refuses; offsets and an angle about the panel's Z axis, or about a
/// round face by key, are added as the panel adds them, one undo step.
#[test]
fn move_body_takes_offsets_and_an_axis() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    wrong_edit(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 1"] }),
        "move_body: enter a distance or an angle",
    );
    let FeatureKind::Move(moved) = applied(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 1"], "x": "5", "z": "-1 cm" }),
    ) else {
        panic!("not a move");
    };
    assert_eq!(moved.bodies, vec![plate]);
    let offsets = moved.offset.each_ref().map(|value| value.value);
    assert_eq!(offsets, [5.0, 0.0, -10.0]);
    assert!(moved.turn.is_none());
    assert_eq!(varde.mcp.log.back().unwrap().args, "Body 1, by 5, 0, -1 cm");
    undo(&mut varde);
    regenerate(&mut varde, &requests);
    assert_eq!(
        varde
            .screen
            .doc()
            .unwrap()
            .editor
            .document()
            .features()
            .len(),
        2
    );

    // An angle alone turns about the Z axis, the panel's.
    let FeatureKind::Move(moved) = applied(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 1"], "angle": "90 deg" }),
    ) else {
        panic!("not a move");
    };
    let (axis, angle) = moved.turn.unwrap();
    assert_eq!(axis, AxisRef::Origin(varde_document::Axis3::Z));
    assert_eq!(angle.text, "90 deg");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    // About the hole's wall, a round face, by key.
    let hole = face_of_form(&varde, "Body 1", "cylinder");
    let FeatureKind::Move(moved) = applied(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 1"], "angle": "45 deg", "axis": { "face": hole } }),
    ) else {
        panic!("not a move");
    };
    let Some((AxisRef::Face(face), _)) = moved.turn else {
        panic!("{:?}", moved.turn);
    };
    assert_eq!(face.body, plate);
}

/// What the Move panel refuses, the call refuses saying why: a flat face
/// as the axis, a key naming another kind than the argument says, an
/// offset past the coordinate limit, a body unknown or named twice, one
/// made after the rollback marker.
#[test]
fn move_body_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let corner = corner_at(&varde, "Body 1", [30.0, 20.0, 10.0]);
    for (arguments, why) in [
        (
            json!({ "bodies": ["Body 1"], "angle": "10 deg", "axis": { "face": top } }),
            "axis: Only a straight or round edge, or a round face, can be the axis",
        ),
        (
            json!({ "bodies": ["Body 1"], "angle": "10 deg", "axis": { "edge": corner } }),
            "axis: that key names a corner, not an edge",
        ),
        (
            json!({ "bodies": ["Body 1"], "angle": "10 deg", "axis": { "edge": "nonsense" } }),
            "axis: that isn't a key Varde gave: ask body_topology again",
        ),
        (
            json!({ "bodies": ["Body 1"], "x": "2000000" }),
            "move_body: x: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "angle": "2 turn" }),
            "move_body: angle: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "x": "nothing" }),
            "move_body: x: ",
        ),
        (
            json!({ "bodies": ["Body 9"], "x": "1" }),
            "no body is named \"Body 9\"",
        ),
        (
            json!({ "bodies": ["Body 1", "Body 1"], "x": "1" }),
            "\"Body 1\" is named twice",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "move_body", arguments, why);
    }
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Extrude 1" }),
    );
    edit_data(reply);
    wrong_edit(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 1"], "x": "1" }),
        "\"Body 1\" is made after where the move goes",
    );
}

/// `mirror` keeps the original unless told, across an origin plane or a
/// flat face by key; a round face, or a face key whose face moved, is
/// refused.
#[test]
fn mirror_takes_a_plane_or_a_flat_face() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    let FeatureKind::Mirror(mirror) = applied(
        &mut varde,
        &requests,
        "mirror",
        json!({ "bodies": ["Body 1"], "plane": "YZ" }),
    ) else {
        panic!("not a mirror");
    };
    assert_eq!(mirror.bodies, vec![plate]);
    assert_eq!(
        mirror.plane,
        PlaneRef::Origin(varde_document::OriginPlane::YZ)
    );
    assert!(mirror.keep_original);
    assert_eq!(varde.mcp.log.back().unwrap().args, "Body 1 across yz");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let top = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    let FeatureKind::Mirror(mirror) = applied(
        &mut varde,
        &requests,
        "mirror",
        json!({ "bodies": ["Body 1"], "plane": { "face": top.clone() }, "keep_original": false }),
    ) else {
        panic!("not a mirror");
    };
    let PlaneRef::Face(face) = mirror.plane else {
        panic!("{:?}", mirror.plane);
    };
    assert_eq!(face.body, plate);
    assert!(!mirror.keep_original);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let hole = face_of_form(&varde, "Body 1", "cylinder");
    wrong_edit(
        &mut varde,
        &requests,
        "mirror",
        json!({ "bodies": ["Body 1"], "plane": { "face": hole } }),
        "plane: Only a flat face can be the mirror plane",
    );
    // A face key's point off its face, or its face moved since.
    let off = moved_key(&varde, &top, DVec3::new(0.0, 0.0, 50.0));
    for bad in [
        off,
        moved_key(&varde, &top, DVec3::NAN),
        moved_key(&varde, &top, DVec3::new(1e30, 0.0, 0.0)),
    ] {
        wrong_edit(
            &mut varde,
            &requests,
            "mirror",
            json!({ "bodies": ["Body 1"], "plane": { "face": bad } }),
            "plane: that face isn't in the model any more: ask body_topology again",
        );
    }
    plate_made_symmetric(&mut varde, &requests);
    wrong_edit(
        &mut varde,
        &requests,
        "mirror",
        json!({ "bodies": ["Body 1"], "plane": { "face": top } }),
        "plane: that face isn't in the model any more",
    );
}

/// `pattern` with its bodies alone is the panel's: a linear one along X,
/// 3 copies 10 mm apart (the panel's spacing follows the view), joined;
/// a circular one about Z, 4 round a full turn.
#[test]
fn pattern_adds_with_the_panels_defaults() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    let FeatureKind::Pattern(pattern) = applied(
        &mut varde,
        &requests,
        "pattern",
        json!({ "bodies": ["Body 1"] }),
    ) else {
        panic!("not a pattern");
    };
    assert_eq!(pattern.bodies, vec![plate]);
    assert_eq!(pattern.copies, Copies::Joined);
    let PatternKind::Linear {
        along,
        count,
        spacing,
    } = &pattern.kind
    else {
        panic!("{:?}", pattern.kind);
    };
    assert_eq!(*along, AxisRef::Origin(varde_document::Axis3::X));
    assert_eq!(count.value, 3.0);
    assert_eq!(spacing.value, 10.0);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let FeatureKind::Pattern(pattern) = applied(
        &mut varde,
        &requests,
        "pattern",
        json!({ "kind": "circular", "bodies": ["Body 1"] }),
    ) else {
        panic!("not a pattern");
    };
    let PatternKind::Circular { about, count, .. } = &pattern.kind else {
        panic!("{:?}", pattern.kind);
    };
    assert_eq!(*about, AxisRef::Origin(varde_document::Axis3::Z));
    assert_eq!(count.value, 4.0);
    assert!(pattern.full_turn());
}

/// `pattern` with its values given, stored as the panel stores them (a
/// total divided, Flip a negative spacing; a circular total as typed),
/// unjoined copies each a body; and what only the panel refuses said: a
/// mode its kind doesn't offer, a spread with full, Flip for a circular
/// one, a count or spread past its bounds.
#[test]
fn pattern_takes_its_values_and_refuses_what_the_panel_does() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let FeatureKind::Pattern(pattern) = applied(
        &mut varde,
        &requests,
        "pattern",
        json!({ "bodies": ["Body 1"], "axis": "y", "count": 4, "mode": "total",
                "spread": "90", "flip": true, "join": false }),
    ) else {
        panic!("not a pattern");
    };
    let PatternKind::Linear {
        along,
        count,
        spacing,
    } = &pattern.kind
    else {
        panic!("{:?}", pattern.kind);
    };
    assert_eq!(*along, AxisRef::Origin(varde_document::Axis3::Y));
    assert_eq!(count.value, 4.0);
    assert_eq!(spacing.value, -30.0);
    // Each of the three copies a body of its own.
    let Copies::Separate(copies) = &pattern.copies else {
        panic!("{:?}", pattern.copies);
    };
    assert_eq!(copies.len(), 3);
    assert_eq!(bodies(&varde).len(), 4);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let rim = rim_at(&varde, "Body 1", 10.0);
    let FeatureKind::Pattern(pattern) = applied(
        &mut varde,
        &requests,
        "pattern",
        json!({ "kind": "circular", "bodies": ["Body 1"], "axis": { "edge": rim },
                "count": "3", "mode": "total", "spread": "90 deg" }),
    ) else {
        panic!("not a pattern");
    };
    let PatternKind::Circular { about, angle, .. } = &pattern.kind else {
        panic!("{:?}", pattern.kind);
    };
    assert!(matches!(about, AxisRef::Edge(_)), "{about:?}");
    assert_eq!(angle.text, "90 deg");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    for (arguments, why) in [
        (
            json!({ "bodies": ["Body 1"], "mode": "full" }),
            "pattern: mode full goes with kind circular only",
        ),
        (
            json!({ "kind": "circular", "bodies": ["Body 1"], "spread": "30 deg" }),
            "pattern: spread goes with mode spacing or total",
        ),
        (
            json!({ "kind": "circular", "bodies": ["Body 1"], "flip": true }),
            "pattern: flip goes with kind linear only",
        ),
        (
            json!({ "bodies": ["Body 1"], "count": 1 }),
            "pattern: count: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "count": "2000" }),
            "pattern: count: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "count": "2.5" }),
            "pattern: count: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "spread": "-5" }),
            "pattern: spread: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "count": 1000, "spread": "1000 m" }),
            "pattern: The pattern runs past",
        ),
        (
            json!({ "kind": "circular", "bodies": ["Body 1"], "mode": "spacing",
                    "count": 5, "spread": "90 deg" }),
            "pattern: 5 copies 90° apart go past a full turn",
        ),
        (
            json!({ "kind": "circular", "bodies": ["Body 1"], "mode": "total",
                    "spread": "360 deg" }),
            "pattern: A whole turn puts the last copy on the first",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "pattern", arguments, why);
    }
}

/// `align` of the disc by its bottom rim to the plate's top rim: each
/// rim's centre gives its side the rim's axis, as the panel's picks do,
/// so the two meet axis to axis; a corner to the origin is a move
/// between the points.
#[test]
fn align_meets_rims_or_points() {
    let (mut varde, requests) = with_disc();
    let [plate, disc] = bodies(&varde)[..] else {
        panic!();
    };
    let foot = rim_at(&varde, "Body 2", 0.0);
    let top = rim_at(&varde, "Body 1", 10.0);
    let feature = feature_at(&varde, 2);
    assert!(matches!(feature.kind, FeatureKind::Extrude(_)));
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "align",
        json!({ "from": { "point": { "edge": foot } }, "to": { "point": { "edge": top } } }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    assert_eq!(edit_data(reply)["feature"], json!("Align 1"));
    let FeatureKind::Align(align) = kind_at(&varde, 3) else {
        panic!("not an align");
    };
    assert_eq!(align.body, disc);
    let (PointRef::Centre(from), Some(DirRef::Axis(AxisRef::Edge(from_axis)))) =
        (align.from.point, align.from.primary)
    else {
        panic!("{:?}", align.from);
    };
    assert_eq!((from.body, from_axis), (disc, from));
    let (PointRef::Centre(to), Some(DirRef::Axis(AxisRef::Edge(to_axis)))) =
        (align.to.point, align.to.primary)
    else {
        panic!("{:?}", align.to);
    };
    assert_eq!((to.body, to_axis), (plate, to));
    assert!(!align.flip && align.offset.is_none() && align.turn.is_none());
    assert_eq!(varde.mcp.log.back().unwrap().args, "an edge to an edge");
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    // A plate corner to the origin, the body the corner's; then with
    // directions, a flip and an offset.
    let corner = corner_at(&varde, "Body 1", [30.0, 20.0, 10.0]);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "align",
        json!({ "from": { "point": { "corner": corner.clone() } }, "to": { "point": "origin" } }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Align(align) = kind_at(&varde, 3) else {
        panic!("not an align");
    };
    assert_eq!(align.body, plate);
    assert!(matches!(align.from.point, PointRef::Corner { body, .. } if body == plate));
    assert_eq!(align.to.point, PointRef::Origin);
    assert!(align.from.primary.is_none());
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let side = face_facing(&varde, "Body 1", [1.0, 0.0, 0.0]);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "align",
        json!({ "body": "Body 1",
                "from": { "point": { "corner": corner }, "direction": { "face": side } },
                "to": { "point": "origin", "direction": "x" },
                "flip": true, "distance": "5", "angle": "30 deg" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Align(align) = kind_at(&varde, 3) else {
        panic!("not an align");
    };
    assert!(matches!(align.from.primary, Some(DirRef::Normal(face)) if face.body == plate));
    assert_eq!(
        align.to.primary,
        Some(DirRef::Origin(varde_document::Axis3::X))
    );
    assert!(align.flip);
    assert_eq!(align.offset.unwrap().value, 5.0);
    assert_eq!(align.turn.unwrap().text, "30 deg");
}

/// What the Align panel refuses, the call refuses saying why: the origin
/// on the body's side, Flip or an offset without directions, a direction
/// on one side alone, what it's aligned to on the body aligned, a point
/// off the body given, a face's key as a corner, a corner key's point off
/// its corner.
#[test]
fn align_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_disc();
    let foot = rim_at(&varde, "Body 2", 0.0);
    let top = rim_at(&varde, "Body 1", 10.0);
    let corner = corner_at(&varde, "Body 1", [30.0, 20.0, 10.0]);
    let other = corner_at(&varde, "Body 1", [-30.0, -20.0, 0.0]);
    let face = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    for (arguments, why) in [
        (
            json!({ "from": { "point": "origin" }, "to": { "point": { "corner": corner } } }),
            "align: from.point: the origin and its axes can only be what it's aligned to",
        ),
        (
            json!({ "from": { "point": { "corner": corner } }, "to": { "point": "origin" },
                    "flip": true }),
            "align: flip goes with a direction on each side",
        ),
        (
            json!({ "from": { "point": { "corner": corner } }, "to": { "point": "origin" },
                    "distance": "5" }),
            "align: pick a direction on each side to offset or turn along",
        ),
        (
            json!({ "from": { "point": { "edge": foot } }, "to": { "point": "origin" } }),
            "align: pick the direction to align it to",
        ),
        (
            json!({ "from": { "point": { "corner": corner } },
                    "to": { "point": { "corner": other } } }),
            "align: to.point: Pick what it's aligned to on another body than the one aligned",
        ),
        (
            json!({ "body": "Body 2", "from": { "point": { "corner": corner } },
                    "to": { "point": { "edge": top } } }),
            "align: from.point: Pick it on Body 2, the body aligned",
        ),
        (
            json!({ "from": { "point": { "corner": face } }, "to": { "point": "origin" } }),
            "align: from.point: that key names a face, not a corner",
        ),
        (
            json!({ "from": { "point": { "corner": corner } }, "to": { "point": "origin" },
                    "distance": "1e9" }),
            "align: distance: ",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "align", arguments, why);
    }
    for near in [
        DVec3::new(30.0, 20.0, 11.0),
        DVec3::NAN,
        DVec3::new(f64::INFINITY, 0.0, 0.0),
    ] {
        let bad = moved_key(&varde, &corner, near);
        wrong_edit(
            &mut varde,
            &requests,
            "align",
            json!({ "from": { "point": { "corner": bad } }, "to": { "point": "origin" } }),
            "align: from.point: that corner isn't in the model any more",
        );
    }
}

/// `scale` with its bodies alone scales by 1, which the panel's OK
/// refuses; a factor about the origin or a corner by key, per axis, or to
/// an edge's length (along its axis only for an edge along one) are added
/// as the panel adds them.
#[test]
fn scale_takes_a_factor_a_point_or_an_edge() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let plate = bodies(&varde)[0];
    wrong_edit(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 1"] }),
        "scale: enter a factor other than 1",
    );
    let FeatureKind::Scale(scale) = applied(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 1"], "factor": "2" }),
    ) else {
        panic!("not a scale");
    };
    assert_eq!(scale.bodies, vec![plate]);
    assert_eq!(scale.about, PointRef::Origin);
    assert!(matches!(&scale.factor, ScaleFactor::Uniform(factor) if factor.value == 2.0));
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let corner = corner_at(&varde, "Body 1", [30.0, 20.0, 10.0]);
    let FeatureKind::Scale(scale) = applied(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 1"], "point": { "corner": corner }, "mode": "per_axis",
                "z": "3" }),
    ) else {
        panic!("not a scale");
    };
    assert!(matches!(scale.about, PointRef::Corner { body, .. } if body == plate));
    let ScaleFactor::PerAxis(factors) = &scale.factor else {
        panic!("{:?}", scale.factor);
    };
    assert_eq!(factors.each_ref().map(|f| f.value), [1.0, 1.0, 3.0]);
    undo(&mut varde);
    regenerate(&mut varde, &requests);

    let long = edge_between(&varde, "Body 1", [-30.0, -20.0, 0.0], [30.0, -20.0, 0.0]);
    let FeatureKind::Scale(scale) = applied(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 1"], "mode": "edge_length", "edge": long,
                "length": "120", "axis_only": true }),
    ) else {
        panic!("not a scale");
    };
    let ScaleFactor::EdgeLength {
        edge,
        length,
        axis_only,
    } = &scale.factor
    else {
        panic!("{:?}", scale.factor);
    };
    assert_eq!(edge.body, plate);
    assert_eq!(length.value, 120.0);
    assert!(axis_only);
}

/// What the Scale panel refuses, the call refuses saying why: a value of
/// another mode, a factor past its bounds, edge length without its edge
/// or length, an edge of a body it doesn't scale, along its axis only for
/// a round edge, a face's key as the point.
#[test]
fn scale_refuses_what_the_panel_would_not_take() {
    let (mut varde, requests) = with_disc();
    let rim = rim_at(&varde, "Body 1", 10.0);
    let foot = rim_at(&varde, "Body 2", 0.0);
    let face = face_facing(&varde, "Body 1", [0.0, 0.0, 1.0]);
    for (arguments, why) in [
        (
            json!({ "bodies": ["Body 1"], "x": "2" }),
            "scale: x goes with mode per_axis",
        ),
        (
            json!({ "bodies": ["Body 1"], "mode": "per_axis", "factor": "2" }),
            "scale: factor goes with mode uniform",
        ),
        (
            json!({ "bodies": ["Body 1"], "length": "5" }),
            "scale: length goes with mode edge_length",
        ),
        (
            json!({ "bodies": ["Body 1"], "factor": "2000" }),
            "scale: factor: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "factor": "0" }),
            "scale: factor: ",
        ),
        (
            json!({ "bodies": ["Body 1"], "mode": "edge_length", "length": "5" }),
            "scale: pick the edge to give a length",
        ),
        (
            json!({ "bodies": ["Body 1"], "mode": "edge_length", "edge": rim.clone() }),
            "scale: enter the length the edge is to have",
        ),
        (
            json!({ "bodies": ["Body 1"], "mode": "edge_length", "edge": foot,
                    "length": "5" }),
            "scale: edge: Pick an edge of a body it scales",
        ),
        (
            json!({ "bodies": ["Body 1"], "mode": "edge_length", "edge": rim,
                    "length": "100", "axis_only": true }),
            "scale: axis_only goes with a straight edge along a world axis",
        ),
        (
            json!({ "bodies": ["Body 1"], "factor": "2", "point": { "corner": face } }),
            "scale: point: that key names a face, not a corner",
        ),
    ] {
        wrong_edit(&mut varde, &requests, "scale", arguments, why);
    }
}

/// A key read before an edit is taken while what it names is still
/// where it says; once the plate is made symmetric, its corner is off
/// where the key has it, and refused saying to ask again.
#[test]
fn a_corner_key_from_another_generation_is_found_again_or_refused() {
    let (mut varde, requests) = listening_with();
    started(&mut varde, "a session", None);
    let corner = corner_at(&varde, "Body 1", [30.0, 20.0, 10.0]);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_opacity",
        json!({ "body": "Body 1", "opacity": 0.5 }),
    );
    edit_data(reply);
    let FeatureKind::Scale(scale) = applied(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 1"], "factor": "2", "point": { "corner": corner.clone() } }),
    ) else {
        panic!("not a scale");
    };
    assert!(matches!(scale.about, PointRef::Corner { near, .. }
        if near == DVec3::new(30.0, 20.0, 10.0)));
    undo(&mut varde);
    regenerate(&mut varde, &requests);
    plate_made_symmetric(&mut varde, &requests);
    wrong_edit(
        &mut varde,
        &requests,
        "scale",
        json!({ "bodies": ["Body 1"], "factor": "2", "point": { "corner": corner } }),
        "scale: point: that corner isn't in the model any more: ask body_topology again",
    );
}

/// A body a combine past the rollback marker uses up is still its own
/// where a new feature goes: `move_body` moves it, not the combine's
/// target, which holds it only after the marker.
#[test]
fn a_body_merged_past_the_rollback_marker_is_its_own() {
    let (mut varde, requests) = with_disc();
    let [plate, disc] = bodies(&varde)[..] else {
        panic!("not two bodies");
    };
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 1", "tools": ["Body 2"] }),
    );
    edit_data(reply);
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "set_rollback",
        json!({ "before": "Combine 1" }),
    );
    edit_data(reply);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 2"], "x": "5" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    // At the marker, before the combine.
    let FeatureKind::Move(moved) = kind_at(&varde, 3) else {
        panic!("not a move");
    };
    assert_eq!(moved.bodies, vec![disc]);
    assert_ne!(moved.bodies, vec![plate]);
}

/// A body merged into another is excluded as the body holding it, as
/// the panel's click on it would.
#[test]
fn a_merged_body_is_excluded_as_its_holder() {
    let (mut varde, requests) = with_disc();
    let [plate, _] = bodies(&varde)[..] else {
        panic!("not two bodies");
    };
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 1", "tools": ["Body 2"] }),
    );
    edit_data(reply);
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "extrude",
        json!({ "sketch": "Sketch 1", "profiles": "all", "extent": "through_all",
                "operation": "cut", "exclude": ["Body 2"] }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Extrude(extrude) = kind_at(&varde, 4) else {
        panic!("not an extrude");
    };
    assert_eq!(extrude.operation.excluded(), &[plate]);
}

/// Right after a client edit closes the user's edit of an earlier
/// combine, the model shown is still that panel's preview until
/// regeneration answers: the client's body names are resolved on the
/// committed model, where the combine holds the body it uses up, not on
/// the preview, where it failed.
#[test]
fn a_closed_panels_preview_doesnt_name_the_clients_bodies() {
    let (mut varde, requests) = with_disc();
    let [plate, _] = bodies(&varde)[..] else {
        panic!("not two bodies");
    };
    let (reply, _) = edit(
        &mut varde,
        &requests,
        "combine",
        json!({ "target": "Body 1", "tools": ["Body 2"] }),
    );
    edit_data(reply);
    let combine = feature_id(&varde, 3);
    look(&mut varde, varde_view::Look::EditFeature(combine));
    let doc = varde.screen.doc_mut().unwrap();
    answer_failing(doc, &requests, combine);
    assert!(doc.feed.shows_draft(), "the combine's preview isn't shown");
    let (reply, state) = edit(
        &mut varde,
        &requests,
        "move_body",
        json!({ "bodies": ["Body 2"], "x": "5" }),
    );
    assert_eq!(state, McpCallState::Applied, "{}", reply.text);
    let FeatureKind::Move(moved) = kind_at(&varde, 4) else {
        panic!("not a move");
    };
    assert_eq!(moved.bodies, vec![plate]);
}
