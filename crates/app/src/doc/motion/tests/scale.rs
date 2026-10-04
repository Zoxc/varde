//! The scale session: the example plate scaled × 2 about the origin and
//! OK, about a corner picked, per axis, to an edge's length (its length
//! shown, along its axis only offered for a straight edge along X and not
//! for the hole's rim), the refusals shown (a length too far, the edge
//! drawn), a torus's fitted faces noted, editing from the Timeline,
//! cancelling and undo, a point an undo takes away said to be gone.

use glam::DVec3;
use varde_document::{
    AxisLine, BodyId, Command, Document, EdgeRef, Editor, FeatureKind, Operation, OriginPlane,
    Plane, PointRef, Revolve, Scale, ScaleFactor, Turn,
};
use varde_regen::Request;
use varde_view::{
    Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, Pick, Picked, ScaleMode, Snapped,
};

use super::{Plates, enter, near};
use crate::tests::{holding, key_in, screen_texts};

fn shows(plates: &Plates, wanted: &str) -> bool {
    screen_texts(&plates.doc)
        .iter()
        .any(|text| text.contains(wanted))
}

/// The example's plate alone, "Body 1": 60 × 40 × 10 about the Z axis
/// from z 0 up, a hole of radius 8 through it about the Z axis.
fn plate() -> (Plates, BodyId) {
    let document = Document::example();
    let plate = document.bodies()[0].id;
    let (doc, requests) = holding(document);
    let plates = Plates {
        doc,
        requests,
        bodies: [plate; 3],
    };
    (plates, plate)
}

fn picking(plates: &Plates) -> MotionPick {
    plates.doc.motion.as_ref().expect("a session").picking
}

/// The scale the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Scale> {
    match plates.last_draft()?.1 {
        FeatureKind::Scale(scale) => Some(scale),
        _ => None,
    }
}

/// Whether the box of `body` in the model shown is `low` to `high`.
fn boxed(plates: &Plates, body: BodyId, low: [f64; 3], high: [f64; 3]) -> bool {
    let [l, h] = plates.bounds(body);
    near(l, DVec3::from(low)) && near(h, DVec3::from(high))
}

/// The straight edge of `body` in the model shown from `a` to `b`
/// (either way round).
fn straight(plates: &Plates, body: BodyId, a: DVec3, b: DVec3) -> u32 {
    let index = plates.doc.feed.pick_index();
    (0..index.mesh().edge_count() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(body)
                && (index.chain_keys(edge))
                    .and_then(|keys| index.edge_ends(edge, &keys))
                    .is_some_and(|[from, to]| {
                        (near(from, a) && near(to, b)) || (near(from, b) && near(to, a))
                    })
        })
        .expect("such an edge")
}

/// The round edge of `body` in the model shown whose centre is `centre`.
fn rim(plates: &Plates, body: BodyId, centre: DVec3) -> u32 {
    let index = plates.doc.feed.pick_index();
    let snaps = index.picking().snaps();
    (0..snaps.len() as u32)
        .find(|&edge| {
            index.body(Picked::Edge(edge)) == Some(body)
                && snaps[edge as usize].is_some_and(|at| DVec3::from(at).distance(centre) < 1e-9)
        })
        .expect("such a rim")
}

/// The corner of `body` in the model shown at `at`, and a face there.
fn corner(plates: &Plates, body: BodyId, at: DVec3) -> (u32, Picked) {
    let index = plates.doc.feed.pick_index();
    let corners = index.picking().corners();
    let found = (0..corners.len() as u32)
        .find(|&corner| {
            let face = Picked::Face(corners[corner as usize].faces[0]);
            index.body(face) == Some(body)
                && DVec3::from(corners[corner as usize].point).distance(at) < 1e-9
        })
        .expect("such a corner");
    (found, Picked::Face(corners[found as usize].faces[0]))
}

/// A click on `target` of `body` at `at`, snapped to `snap`.
fn click_snapped(plates: &mut Plates, body: BodyId, target: Picked, at: DVec3, snap: Snapped) {
    let pick = Pick {
        model: plates.doc.feed.pick_index().model(),
        target,
        body,
        at,
        snap: Some(snap),
    };
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
}

/// The plate's bottom edge along X at the front, from (-30, -20, 0) to
/// (30, -20, 0): 60 long.
fn front_edge(plates: &Plates, plate: BodyId) -> u32 {
    straight(
        plates,
        plate,
        DVec3::new(-30.0, -20.0, 0.0),
        DVec3::new(30.0, -20.0, 0.0),
    )
}

/// The rail's Scale with the example's only body: × 2 about the origin
/// previews twice as large, the status bar names it, and OK adds "Scale
/// 1" as one undo step.
#[test]
fn a_plate_scaled_by_two_about_the_origin_and_ok() {
    let (mut plates, plate) = plate();
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartScale);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Scale);
    // The only body is the one scaled, about the origin to begin with.
    assert_eq!(session.bodies, [plate]);
    assert_eq!(session.scale.about, PointRef::Origin);
    assert_eq!(picking(&plates), MotionPick::Bodies);
    for text in [
        "New scale",
        "Bodies",
        "Point",
        "Origin",
        "Uniform",
        "Per axis",
        "Edge length",
        "Factor",
        "enter a factor other than 1",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    assert!(!plates.doc.motion_ready());
    assert!(plates.last_draft().is_none(), "nothing scales yet");

    plates.input(MotionField::Factor, "2");
    assert!(plates.doc.motion_ready());
    let scale = drafted(&plates).expect("a scale's draft");
    assert_eq!(scale.bodies, [plate]);
    assert_eq!(scale.about, PointRef::Origin);
    assert!(matches!(&scale.factor, ScaleFactor::Uniform(f) if f.value == 2.0));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(boxed(
        &plates,
        plate,
        [-60.0, -40.0, 0.0],
        [60.0, 40.0, 20.0]
    ));
    // The point is drawn where regenerating found it, and named.
    let state = plates.doc.motion_state().unwrap();
    assert_eq!(state.scale.as_ref().unwrap().at, Some(DVec3::ZERO));
    assert!(shows(&plates, "Body 1 ×2"));
    // No fitted faces: no note.
    assert!(state.warning.is_none());

    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert_eq!(kind, FeatureKind::Scale(scale));
    assert_eq!(plates.doc.selected_feature, Some(id));
    let name = &plates.doc.editor.document().feature(id).unwrap().name;
    assert_eq!(name, "Scale 1");
    plates.answer();
    assert!(boxed(
        &plates,
        plate,
        [-60.0, -40.0, 0.0],
        [60.0, 40.0, 20.0]
    ));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// The point picked as an align's, at a corner's snap dot (the origin
/// offered on the toolbar meanwhile): × 2 about the plate's corner at
/// (-30, -20, 0) keeps that corner where it is.
#[test]
fn a_plate_scaled_about_a_corner_picked() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartScale);
    plates.motion(MotionLook::Picking(MotionPick::Point));
    assert_eq!(picking(&plates), MotionPick::Point);
    let shown = plates.doc.model_picking().expect("picking");
    assert!(shown.snaps, "points at the measure tool's snap dots");
    assert!(shows(&plates, "Origin"), "the toolbar's origin");
    // Nothing is previewed while the point is picked: the model is the
    // history as of the scale.
    assert!(plates.last_draft().is_none());
    let at = DVec3::new(-30.0, -20.0, 0.0);
    let (found, face) = corner(&plates, plate, at);
    click_snapped(&mut plates, plate, face, at, Snapped::Corner(found));
    assert_eq!(picking(&plates), MotionPick::Bodies);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(matches!(session.scale.about, PointRef::Corner { body, .. } if body == plate));
    assert!(shows(&plates, "Corner of Body 1"));
    plates.input(MotionField::Factor, "2");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(boxed(
        &plates,
        plate,
        [-30.0, -20.0, 0.0],
        [90.0, 60.0, 20.0]
    ));
    let state = plates.doc.motion_state().unwrap();
    let drawn = state.scale.as_ref().unwrap().at.expect("the point drawn");
    assert!(near(drawn, at), "{drawn}");

    // A face is no point; the origin from the toolbar is one again.
    plates.motion(MotionLook::Picking(MotionPick::Point));
    plates.answer();
    let top = plates.face(
        plate,
        |summary| matches!(summary, varde_regen::Summary::Plane { n, .. } if n[2] == 1.0),
    );
    plates.click_at(plate, Picked::Face(top), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a corner, a straight edge's middle or a round edge's centre can be the point")
    );
    plates.motion(MotionLook::OriginPoint);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.scale.about, PointRef::Origin);
    assert_eq!(picking(&plates), MotionPick::Bodies);
}

/// Per axis: × 3 along Z alone triples the plate's height.
#[test]
fn a_plate_scaled_per_axis() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartScale);
    plates.motion(MotionLook::ScaleMode(ScaleMode::PerAxis));
    assert!(shows(&plates, "X") && shows(&plates, "Z"));
    assert!(shows(&plates, "enter a factor other than 1"));
    plates.input(MotionField::AxisFactor(varde_document::Axis3::Z), "3");
    let scale = drafted(&plates).expect("a scale's draft");
    assert!(
        matches!(&scale.factor, ScaleFactor::PerAxis(f) if f.each_ref().map(|f| f.value) == [1.0, 1.0, 3.0])
    );
    plates.answer();
    assert!(boxed(
        &plates,
        plate,
        [-30.0, -20.0, 0.0],
        [30.0, 20.0, 30.0]
    ));
    assert!(shows(&plates, "Body 1 ×1 · 1 · 3"));
    // A factor past a thousand is refused under its field.
    plates.input(MotionField::AxisFactor(varde_document::Axis3::X), "2000");
    assert!(!plates.doc.motion_ready());
}

/// To an edge's length: Edge length picks an edge next; the plate's 60
/// front edge along X shows its length and offers Along its axis only;
/// 120 doubles the plate, along its axis only stretches X alone.
#[test]
fn a_plate_scaled_so_an_edge_has_a_length() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartScale);
    plates.motion(MotionLook::ScaleMode(ScaleMode::EdgeLength));
    assert_eq!(picking(&plates), MotionPick::Edge);
    assert!(shows(&plates, "pick the edge to give a length"));
    let picks = plates.doc.model_picking().expect("picking");
    assert_eq!(picks.picks, varde_view::Picks::Edges);
    // A face is no edge.
    let top = plates.face(
        plate,
        |summary| matches!(summary, varde_regen::Summary::Plane { n, .. } if n[2] == 1.0),
    );
    plates.click_at(plate, Picked::Face(top), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only an edge can be scaled to a length")
    );
    let front = front_edge(&plates, plate);
    plates.click_at(plate, Picked::Edge(front), DVec3::new(10.0, -20.0, 0.0));
    assert_eq!(picking(&plates), MotionPick::Bodies);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(matches!(session.scale.edge, Some(EdgeRef { body, .. }) if body == plate));
    assert!(shows(&plates, "Edge of Body 1"));
    assert!(shows(&plates, "enter the length the edge is to have"));
    // The edge is lit in the second colour.
    assert_eq!(plates.doc.scale_lit(), [Picked::Edge(front)]);
    // Its length is measured with the model: shown, and as it's along X,
    // Along its axis only is offered.
    assert!(!shows(&plates, "Along its axis only"), "not measured yet");
    let Some(Request::Regenerate { inspect, .. }) = plates.requests.borrow().last().cloned() else {
        panic!("a request");
    };
    assert!(inspect.is_some(), "the edge measured");
    plates.answer();
    assert!(shows(&plates, "60 mm"));
    assert!(shows(&plates, "Along its axis only"));

    plates.input(MotionField::Length, "120");
    let scale = drafted(&plates).expect("a scale's draft");
    assert!(matches!(
        &scale.factor,
        ScaleFactor::EdgeLength { length, axis_only: false, .. } if length.value == 120.0
    ));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(boxed(
        &plates,
        plate,
        [-60.0, -40.0, 0.0],
        [60.0, 40.0, 20.0]
    ));
    // The length shown is the edge's before the scale.
    assert!(shows(&plates, "60 mm"));
    assert!(shows(&plates, "Body 1 edge → 120 mm"));

    plates.motion(MotionLook::AxisOnly);
    let scale = drafted(&plates).expect("a scale's draft");
    assert!(matches!(
        &scale.factor,
        ScaleFactor::EdgeLength {
            axis_only: true,
            ..
        }
    ));
    plates.answer();
    assert!(boxed(
        &plates,
        plate,
        [-60.0, -20.0, 0.0],
        [60.0, 20.0, 10.0]
    ));
    key_in(&mut plates.doc, enter());
    let (_, kind) = plates.last_feature();
    assert_eq!(kind, FeatureKind::Scale(scale));
}

/// The hole's rim is round: Along its axis only isn't offered; a length
/// giving a factor past a thousand is refused, the panel saying so, and
/// the edge drawn where it fails.
#[test]
fn a_round_edge_offers_no_axis_and_a_length_too_far_is_refused() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartScale);
    plates.motion(MotionLook::ScaleMode(ScaleMode::EdgeLength));
    let hole = rim(&plates, plate, DVec3::new(0.0, 0.0, 10.0));
    plates.click_at(plate, Picked::Edge(hole), DVec3::new(8.0, 0.0, 10.0));
    plates.answer();
    // 2π·8 round.
    let round = varde_expr::format(
        2.0 * std::f64::consts::PI * 8.0,
        Some(varde_expr::Unit::Length(varde_expr::LengthUnit::Mm)),
    );
    assert!(shows(&plates, &round), "{round}");
    assert!(!shows(&plates, "Along its axis only"));
    // A thousand times and more.
    plates.input(MotionField::Length, "60000");
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("refused");
    assert!(error.contains("more than a thousand times"), "{error}");
    assert!(shows(&plates, "Scale fails"));
    assert!(shows(&plates, "more than a thousand times"));
    let drawn = plates.doc.feed.draft_geometry().expect("the edge drawn");
    assert!(drawn.lines().segment_count() > 0);
    // The length shown stays the edge's, measured on the model shown.
    assert!(shows(&plates, &round), "{round}");
}

/// A revolved ring (its faces fitted) scaled × 25.4: kept, the panel
/// noting its fitted faces' error grows.
#[test]
fn fitted_faces_scaled_up_are_noted() {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let mut drawn = varde_sketch::Sketch::default();
    let center = drawn.add_point(glam::DVec2::new(20.0, 0.0)).unwrap();
    let circle = varde_sketch::Curve::Circle {
        center,
        radius: 5.0,
    };
    drawn.add_curve(circle, false).unwrap();
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let revolve = Revolve {
        sketch,
        regions,
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let ring = editor.document().bodies()[0].id;
    let (doc, requests) = holding(editor.document().clone());
    let mut plates = Plates {
        doc,
        requests,
        bodies: [ring; 3],
    };
    plates.doc.look(Look::StartScale);
    plates.input(MotionField::Factor, "25.4");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let state = plates.doc.motion_state().unwrap();
    let note = state.warning.clone().expect("the note");
    // A ring turned whole is one fitted face.
    assert_eq!(note, "1 fitted face: its error grows × 25.4");
    assert!(shows(&plates, &note));
    // Scaled down, nothing grows.
    plates.input(MotionField::Factor, "0.5");
    plates.answer();
    assert!(plates.doc.motion_state().unwrap().warning.is_none());
}

/// A scale edited from the Timeline opens with its values, previews a
/// move by nothing while its point is picked, takes another factor and
/// OK as one undo step; Esc leaves it unchanged.
#[test]
fn a_scale_edited_from_the_timeline_and_cancelled() {
    let (mut plates, plate) = plate();
    plates.doc.look(Look::StartScale);
    plates.input(MotionField::Factor, "2");
    key_in(&mut plates.doc, enter());
    plates.answer();
    let (id, kind) = plates.last_feature();
    let committed = plates.doc.editor.document().clone();

    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Scale);
    assert_eq!(session.feature, Some(id));
    assert!(shows(&plates, "Scale 1"));
    // OK with nothing changed writes nothing.
    assert_eq!(drafted(&plates).map(FeatureKind::Scale), Some(kind.clone()));
    // While the point is picked: the bodies by nothing, as of the scale.
    plates.motion(MotionLook::Picking(MotionPick::Point));
    let (feature, draft) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    assert!(matches!(draft, FeatureKind::Move(moved) if moved.bodies == [plate]));
    plates.answer();
    assert!(boxed(
        &plates,
        plate,
        [-30.0, -20.0, 0.0],
        [30.0, 20.0, 10.0]
    ));
    // Clicking the field again hands the clicks back.
    plates.motion(MotionLook::Picking(MotionPick::Point));
    assert_eq!(picking(&plates), MotionPick::Bodies);

    // Esc: nothing changes.
    plates.doc.look(Look::Escape);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), committed);

    // Edited again: × 3, OK, one undo step back to × 2.
    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::Factor, "3");
    plates.answer();
    assert!(boxed(
        &plates,
        plate,
        [-90.0, -60.0, 0.0],
        [90.0, 60.0, 30.0]
    ));
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    assert!(matches!(
        &plates.doc.editor.document().feature(id).unwrap().kind,
        FeatureKind::Scale(Scale { factor: ScaleFactor::Uniform(f), .. }) if f.value == 3.0
    ));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), committed);
}

/// A point on a body an undo takes away is kept and said to be gone;
/// nothing is previewed or committed until it's picked again or back.
#[test]
fn a_point_an_undo_takes_away_is_gone() {
    let mut plates = super::plates();
    let [plate, ..] = plates.bodies;
    let later = super::later_disc(&mut plates);
    plates.click(plate);
    plates.doc.look(Look::StartScale);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    // The point at the later disc's top rim's centre.
    plates.motion(MotionLook::Picking(MotionPick::Point));
    let top = rim(&plates, later, DVec3::new(0.0, 30.0, 5.0));
    plates.click_at(later, Picked::Edge(top), DVec3::new(5.0, 30.0, 5.0));
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(matches!(session.scale.about, PointRef::Centre(edge) if edge.body == later));
    plates.input(MotionField::Factor, "2");
    assert!(plates.doc.motion_ready());
    // The later disc's extrude undone: its body is gone.
    plates.doc.update(Edit::Undo);
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert!(matches!(session.scale.about, PointRef::Centre(_)), "kept");
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "The point is gone: pick another"));
    assert!(drafted(&plates).is_none(), "nothing previewed");
    // Back on redo.
    plates.doc.update(Edit::Redo);
    assert!(plates.doc.motion_ready());
    // The origin picked instead goes on without it.
    plates.doc.update(Edit::Undo);
    plates.motion(MotionLook::Picking(MotionPick::Point));
    plates.motion(MotionLook::OriginPoint);
    assert!(plates.doc.motion_ready());
}
