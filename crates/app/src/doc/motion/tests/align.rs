//! The align session: a pin aligned into the example plate's hole by its
//! rim and the hole's, then OK; a plate aligned face to face onto another
//! with an offset; picks refused and said why; editing from the Timeline
//! and undo.

use glam::DVec3;
use varde_document::{
    Align, AxisRef, BodyId, DirRef, Document, Editor, Extent, FeatureKind, Operation, PointRef,
};
use varde_regen::Summary;
use varde_view::{
    AlignRole, AlignSide, AlignSlot, Edit, Look, MotionField, MotionKind, MotionLook, MotionPick,
    Pick, Picked, Snapped,
};

use super::{Plates, enter, near};
use crate::tests::{add_disc_of, holding, key_in, length, screen_texts, two_plates};

fn shows(plates: &Plates, wanted: &str) -> bool {
    screen_texts(&plates.doc)
        .iter()
        .any(|text| text.contains(wanted))
}

/// `editor`'s document held by a test's doc, its bodies listed.
fn plates_of(editor: &Editor) -> Plates {
    let (doc, requests) = holding(editor.document().clone());
    Plates {
        doc,
        requests,
        bodies: [BodyId::NEW; 3],
    }
}

/// The example plate, "Body 1" (its hole of radius 8 about the Z axis,
/// from z 0 to 10), and a pin of the hole's radius standing 25 tall from
/// z 0 at (100, 0), "Body 2".
fn pin_and_plate() -> (Plates, BodyId, BodyId) {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    let tall = Extent::OneSide(length(editor.document(), "25"));
    add_disc_of(
        &mut editor,
        (100.0, 0.0),
        8.0,
        tall,
        Operation::NewBody(BodyId::NEW),
    );
    let pin = editor.document().bodies()[1].id;
    (plates_of(&editor), plate, pin)
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

/// The corner of `body` in the model shown at `at`.
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

/// The flat face of `body` in the model shown along `normal`.
fn flat(plates: &Plates, body: BodyId, normal: DVec3) -> u32 {
    plates.face(body, |summary| match summary {
        Summary::Plane { n, .. } => DVec3::from(*n).distance(normal) < 1e-9,
        _ => false,
    })
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

fn picking(plates: &Plates) -> MotionPick {
    plates.doc.motion.as_ref().expect("a session").picking
}

fn slot(side: AlignSide, role: AlignRole) -> MotionPick {
    MotionPick::Align(AlignSlot::new(side, role))
}

/// The align the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Align> {
    match plates.last_draft()?.1 {
        FeatureKind::Align(align) => Some(*align),
        _ => None,
    }
}

/// A pin picked by its foot's rim and aligned onto the hole's top rim,
/// 10 down, fills the hole: two clicks give both points and both
/// directions (each rim's axis), the preview and OK put it there, one
/// undo step.
#[test]
fn a_pin_aligned_by_its_rim_into_the_hole_and_ok() {
    let (mut plates, plate, pin) = pin_and_plate();
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartAlign);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Align);
    // Two bodies and none selected: the body is picked first.
    assert_eq!(picking(&plates), MotionPick::Bodies);
    for text in [
        "New align",
        "Body",
        "From",
        "To",
        "Point",
        "Direction",
        "Second direction",
        "Flip",
        "Offset",
        "Distance",
        "pick the body to align",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    plates.click(pin);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [pin]);
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Point));
    // Points are picked at the measure tool's snap points.
    let shown = plates.doc.model_picking().expect("picking");
    assert!(shown.snaps);

    // The pin's foot rim: its centre, and its axis as the direction.
    let foot = rim(&plates, pin, DVec3::new(100.0, 0.0, 0.0));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(108.0, 0.0, 0.0));
    assert_eq!(picking(&plates), slot(AlignSide::Target, AlignRole::Point));
    assert!(shows(&plates, "Centre of an edge of Body 2"));
    assert!(shows(&plates, "pick the point to align it to"));
    assert!(
        plates.last_draft().is_none(),
        "nothing previewed while picking"
    );

    // The hole's top rim, at its snap point.
    let hole = rim(&plates, plate, DVec3::new(0.0, 0.0, 10.0));
    click_snapped(
        &mut plates,
        plate,
        Picked::Edge(hole),
        DVec3::new(0.0, 0.0, 10.0),
        Snapped::EdgePoint(hole),
    );
    assert_eq!(picking(&plates), MotionPick::Nothing);
    assert!(shows(&plates, "Centre of an edge of Body 1"));
    let session = plates.doc.motion.as_ref().unwrap();
    let [moved, target] = &session.align.sides;
    assert!(matches!(moved.point, Some(PointRef::Centre(edge)) if edge.body == pin));
    assert!(matches!(moved.primary, Some(DirRef::Axis(AxisRef::Edge(edge))) if edge.body == pin));
    assert!(matches!(target.point, Some(PointRef::Centre(edge)) if edge.body == plate));
    assert!(matches!(
        target.primary,
        Some(DirRef::Axis(AxisRef::Edge(_)))
    ));

    // Ready: it stands on the hole; 10 down it fills it.
    assert!(plates.doc.motion_ready());
    plates.input(MotionField::Distance, "-10");
    let align = drafted(&plates).expect("an align's draft");
    assert_eq!(align.body, pin);
    assert_eq!(align.offset.as_ref().map(|value| value.value), Some(-10.0));
    assert!(!align.flip);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(pin);
    assert!(near(low, DVec3::new(-8.0, -8.0, 0.0)), "{low}");
    assert!(near(high, DVec3::new(8.0, 8.0, 25.0)), "{high}");
    // The status bar says what it's aligned to.
    assert!(shows(&plates, "Body 2 to Body 1"));

    // OK: one undo step, selected and named.
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert_eq!(kind, FeatureKind::from(align));
    assert_eq!(plates.doc.selected_feature, Some(id));
    assert_eq!(
        plates.doc.editor.document().feature(id).unwrap().name,
        "Align 1"
    );
    plates.answer();
    let [low, high] = plates.bounds(pin);
    assert!(near(low, DVec3::new(-8.0, -8.0, 0.0)) && near(high, DVec3::new(8.0, 8.0, 25.0)));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// The two plates of `two_plates`, the 3 mm one under the example's
/// moved face to face onto its top by their corners at (30, 20), 5 above
/// it; committed. The plates, and the align's id.
fn face_to_face() -> (Plates, [BodyId; 2], varde_document::FeatureId) {
    let (editor, [top, below]) = two_plates();
    let mut plates = plates_of(&editor);
    // The body selected is the one aligned.
    plates.click(below);
    plates.doc.look(Look::StartAlign);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [below]);
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Point));
    let at = DVec3::new(30.0, 20.0, -3.0);
    let (moved_corner, face) = corner(&plates, below, at);
    click_snapped(&mut plates, below, face, at, Snapped::Corner(moved_corner));
    let at = DVec3::new(30.0, 20.0, 10.0);
    let (target_corner, face) = corner(&plates, top, at);
    click_snapped(&mut plates, top, face, at, Snapped::Corner(target_corner));
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Primary));
    assert!(shows(&plates, "Corner of Body 2") && shows(&plates, "Corner of Body 1"));
    // A point alone moves it already.
    assert!(plates.doc.motion_ready());
    // The faces: its bottom onto the other's top, opposed.
    let bottom = flat(&plates, below, DVec3::NEG_Z);
    plates.click_at(below, Picked::Face(bottom), DVec3::new(0.0, 15.0, -3.0));
    assert_eq!(
        picking(&plates),
        slot(AlignSide::Target, AlignRole::Primary)
    );
    assert!(!plates.doc.motion_ready(), "a direction on one side only");
    assert!(shows(&plates, "pick the direction to align it to"));
    let top_face = flat(&plates, top, DVec3::Z);
    plates.click_at(top, Picked::Face(top_face), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    plates.input(MotionField::Distance, "5");
    assert!(plates.doc.motion_ready());
    let align = drafted(&plates).expect("an align's draft");
    assert!(matches!(align.from.primary, Some(DirRef::Normal(face)) if face.body == below));
    assert!(matches!(align.to.primary, Some(DirRef::Normal(face)) if face.body == top));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // What regenerating found is drawn: both corners, both normals.
    let state = plates.doc.motion_state().unwrap();
    let marks = state.align.as_ref().unwrap().marks;
    assert_eq!(marks[0].point, Some(DVec3::new(30.0, 20.0, -3.0)));
    assert_eq!(marks[1].point, Some(DVec3::new(30.0, 20.0, 10.0)));
    assert_eq!(marks[1].directions[0], Some(DVec3::Z));
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, _) = plates.last_feature();
    plates.answer();
    (plates, [top, below], id)
}

/// Face to face with an offset: the plate under the example's lands on
/// its top, 5 above, its bottom against the top.
#[test]
fn a_plate_aligned_face_to_face_with_an_offset() {
    let (plates, [top, below], _) = face_to_face();
    let [low, high] = plates.bounds(below);
    assert!(near(low, DVec3::new(-30.0, -20.0, 15.0)), "{low}");
    assert!(near(high, DVec3::new(30.0, 20.0, 18.0)), "{high}");
    let [low, high] = plates.bounds(top);
    assert!(near(low, DVec3::new(-30.0, -20.0, 0.0)) && near(high, DVec3::new(30.0, 20.0, 10.0)));
}

/// Picks it can't take are refused, the status bar saying why, and
/// nothing changes: a face for a point, a vertex for a direction, the
/// target picked on the body aligned, the moved side's on another body.
#[test]
fn a_refused_pick_is_shown() {
    let (mut plates, plate, pin) = pin_and_plate();
    plates.doc.look(Look::StartAlign);
    plates.click(pin);
    let before = plates.doc.motion.as_ref().unwrap().align.clone();
    // A face is no point.
    let side = plates.face(pin, |summary| matches!(summary, Summary::Cylinder { .. }));
    plates.click_at(pin, Picked::Face(side), DVec3::new(108.0, 0.0, 5.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a corner, a straight edge's middle or a round edge's centre can be the point")
    );
    assert!(shows(
        &plates,
        "Only a corner, a straight edge's middle or a round edge's centre can be the point"
    ));
    // The moved side's point on another body.
    let hole = rim(&plates, plate, DVec3::new(0.0, 0.0, 10.0));
    plates.click_at(plate, Picked::Edge(hole), DVec3::new(8.0, 0.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Pick it on Body 2, the body aligned")
    );
    assert_eq!(plates.doc.motion.as_ref().unwrap().align, before);
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Point));
    // The target on the body aligned.
    let foot = rim(&plates, pin, DVec3::new(100.0, 0.0, 0.0));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(108.0, 0.0, 0.0));
    let top = rim(&plates, pin, DVec3::new(100.0, 0.0, 25.0));
    plates.click_at(pin, Picked::Edge(top), DVec3::new(108.0, 0.0, 25.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Pick what it's aligned to on another body than the one aligned")
    );
    assert_eq!(picking(&plates), slot(AlignSide::Target, AlignRole::Point));
    // The origin from the toolbar is a target point.
    assert!(shows(&plates, "Origin"));
    plates.motion(MotionLook::OriginPoint);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.align.sides[1].point, Some(PointRef::Origin));
    assert_eq!(
        picking(&plates),
        slot(AlignSide::Target, AlignRole::Primary)
    );
    // A direction isn't a vertex: picking one now, a corner of the plate
    // is refused.
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Target,
        AlignRole::Primary,
    )));
    let index = plates.doc.feed.pick_index();
    let vertex = (0..index.mesh().positions().len() as u32)
        .find(|&v| index.body(Picked::Vertex(v)) == Some(plate))
        .expect("a vertex of the plate");
    plates.click_at(plate, Picked::Vertex(vertex), DVec3::ZERO);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a flat or round face, or a straight or round edge, can give the direction")
    );
}

/// An align edited from the Timeline opens with its references and
/// values; another distance previews and commits it, one undo step back
/// to what it was.
#[test]
fn an_align_edited_from_the_timeline_and_undone() {
    let (mut plates, [_, below], id) = face_to_face();
    let committed = plates.doc.editor.document().clone();
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    assert!(shows(&plates, "to Body 1"), "the Timeline's note");
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Align);
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.picking, MotionPick::Nothing);
    for text in [
        "Align 1",
        "Corner of Body 2",
        "Corner of Body 1",
        "Normal of",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    assert!(plates.doc.motion_ready());
    // Picking a reference again previews the history as of the align:
    // a move of the body by nothing.
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Target,
        AlignRole::Point,
    )));
    let (feature, draft) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    assert!(matches!(draft, FeatureKind::Move(moved) if moved.bodies == [below]));
    plates.answer();
    let [low, _] = plates.bounds(below);
    assert!(near(low, DVec3::new(-30.0, -20.0, -3.0)), "{low}");
    plates.motion(MotionLook::Picking(MotionPick::Nothing));
    plates.input(MotionField::Distance, "2");
    let align = drafted(&plates).expect("the align previewed");
    assert_eq!(align.offset.as_ref().map(|value| value.value), Some(2.0));
    plates.answer();
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    plates.answer();
    let [low, high] = plates.bounds(below);
    assert!(near(low, DVec3::new(-30.0, -20.0, 12.0)), "{low}");
    assert!(near(high, DVec3::new(30.0, 20.0, 15.0)), "{high}");
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), committed);
    plates.answer();
    let [low, _] = plates.bounds(below);
    assert!(near(low, DVec3::new(-30.0, -20.0, 15.0)), "{low}");
}

/// A reference picked on a body an undo takes away stays, said to be
/// gone as the mock says a move's axis is: nothing previewed or
/// committed until a redo brings it back.
#[test]
fn a_reference_an_undo_takes_away_is_said_to_be_gone() {
    let (mut plates, _, pin) = pin_and_plate();
    let later = super::later_disc(&mut plates);
    plates.doc.look(Look::StartAlign);
    plates.click(pin);
    let foot = rim(&plates, pin, DVec3::new(100.0, 0.0, 0.0));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(108.0, 0.0, 0.0));
    // The later disc's top rim, about (0, 30) at z 5.
    let disc = rim(&plates, later, DVec3::new(0.0, 30.0, 5.0));
    plates.click_at(later, Picked::Edge(disc), DVec3::new(5.0, 30.0, 5.0));
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::Undo);
    assert!(plates.last_draft().is_none(), "nothing previewed");
    plates.answer();
    assert!(plates.doc.motion.is_some());
    assert!(!plates.doc.motion_ready());
    assert!(shows(
        &plates,
        "The point it's aligned to is gone: pick another"
    ));
    plates.doc.update(Edit::Redo);
    assert!(drafted(&plates).is_some());
    plates.answer();
    assert!(plates.doc.motion_ready());
}
