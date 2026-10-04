//! The align session: a pin aligned into the example plate's hole by its
//! rim and the hole's, then OK; a plate aligned face to face onto another
//! with an offset; picks refused and said why; editing from the Timeline
//! and undo.

use glam::DVec3;
use varde_document::{
    Align, AlignRefs, AxisRef, BodyId, Command, DirRef, Document, Editor, Extent, FeatureKind,
    Operation, PointRef,
};
use varde_regen::Summary;
use varde_view::{
    AlignRole, AlignSide, AlignSlot, Edit, Look, MotionField, MotionKind, MotionLook, MotionPick,
    Pick, Picked, Snapped,
};

use super::{Plates, enter, near};
use crate::doc::motion::align::Taken;
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
    // Its field clicked again stops picking; once more, it picks again.
    let direction = slot(AlignSide::Target, AlignRole::Primary);
    plates.motion(MotionLook::Picking(direction));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    plates.motion(MotionLook::Picking(direction));
    assert_eq!(picking(&plates), direction);
    // A direction isn't a vertex: picking one now, a corner of the plate
    // is refused.
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

/// With the body aligned taken away by an undo, a moved side's pick on
/// another body says the body aligned is gone rather than naming it.
#[test]
fn a_pick_after_the_body_aligned_is_gone_says_so() {
    let (mut plates, _, pin) = pin_and_plate();
    let later = super::later_disc(&mut plates);
    plates.doc.look(Look::StartAlign);
    plates.click(later);
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Point));
    plates.doc.update(Edit::Undo);
    plates.answer();
    assert!(shows(&plates, "A picked body is gone"));
    let foot = rim(&plates, pin, DVec3::new(100.0, 0.0, 0.0));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(108.0, 0.0, 0.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("The body aligned is gone: pick the body to align first")
    );
}

/// With the mouse's hints on, the status bar hints what a click picks
/// next beside the session's status; the viewport's own mouse hints make
/// room for it.
#[test]
fn the_status_bar_hints_what_a_click_picks() {
    let (mut plates, _, pin) = pin_and_plate();
    plates.doc.look(Look::StartAlign);
    let bar = crate::tests::status_bar_texts(&plates.doc, true);
    assert!(bar.iter().any(|text| text == "Pick the body"), "{bar:?}");
    assert!(!bar.iter().any(|text| text == "Pan"), "{bar:?}");
    plates.click(pin);
    let bar = crate::tests::status_bar_texts(&plates.doc, true);
    assert!(bar.iter().any(|text| text == "Pick the point"), "{bar:?}");
    let bar = crate::tests::status_bar_texts(&plates.doc, false);
    assert!(!bar.iter().any(|text| text == "Pick the point"), "{bar:?}");
}

/// A pin's moved point picked again on its other rim takes that rim's
/// axis along with it, and taken out takes the axis out too, clicks going
/// back to it; a hole's rim picked at its centre's dot names what a click
/// on the rim names.
#[test]
fn a_rim_picked_again_takes_its_axis_along() {
    let (mut plates, plate, pin) = pin_and_plate();
    plates.doc.look(Look::StartAlign);
    plates.click(pin);
    let foot = rim(&plates, pin, DVec3::new(100.0, 0.0, 0.0));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(108.0, 0.0, 0.0));
    let rim_of = |plates: &Plates, side: usize| {
        let side = plates.doc.motion.as_ref().unwrap().align.sides[side];
        match (side.point, side.primary) {
            (Some(PointRef::Centre(point)), Some(DirRef::Axis(AxisRef::Edge(axis)))) => {
                assert_eq!(point, axis, "the point's rim gives the direction");
                point
            }
            other => panic!("not a rim and its axis: {other:?}"),
        }
    };
    let first = rim_of(&plates, 0);
    // Named at a point on the rim, not at its centre.
    let centre = DVec3::new(100.0, 0.0, 0.0);
    assert!((first.near - centre).length() > 7.5, "{}", first.near);

    plates.motion(MotionLook::Picking(slot(
        AlignSide::Moved,
        AlignRole::Point,
    )));
    let top = rim(&plates, pin, DVec3::new(100.0, 0.0, 25.0));
    plates.click_at(pin, Picked::Edge(top), DVec3::new(108.0, 0.0, 25.0));
    let second = rim_of(&plates, 0);
    assert_ne!(second.faces, first.faces);
    assert!((second.near.z - 25.0).abs() < 1e-3, "{}", second.near);
    assert_eq!(picking(&plates), slot(AlignSide::Target, AlignRole::Point));

    // The hole's top rim at its centre's dot, then clicked on the rim.
    let hole = rim(&plates, plate, DVec3::new(0.0, 0.0, 10.0));
    click_snapped(
        &mut plates,
        plate,
        Picked::Edge(hole),
        DVec3::new(0.0, 0.0, 10.0),
        Snapped::EdgePoint(hole),
    );
    let at_dot = rim_of(&plates, 1);
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Target,
        AlignRole::Point,
    )));
    plates.click_at(plate, Picked::Edge(hole), DVec3::new(0.0, 8.0, 10.0));
    assert_eq!(rim_of(&plates, 1), at_dot);

    // Taken out, the point takes its axis with it; clicks go back to it.
    plates.motion(MotionLook::Clear(AlignSlot::new(
        AlignSide::Moved,
        AlignRole::Point,
    )));
    let moved = plates.doc.motion.as_ref().unwrap().align.sides[0];
    assert_eq!((moved.point, moved.primary), (None, None));
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Point));
}

/// The plates of [`super::plates`] with a combine consuming the right
/// disc into the plate: the plates, the plate, the left disc and the
/// right disc's top rim's centre.
fn combined() -> (Plates, BodyId, BodyId, DVec3) {
    let mut plates = super::plates();
    let [plate, right, left] = plates.bodies;
    let combine = varde_document::Combine {
        target: plate,
        tools: vec![right],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    let add = plates.doc.editor.document().add_feature(combine.into());
    plates.doc.apply(add);
    plates.doc.sync();
    plates.answer();
    (plates, plate, left, DVec3::new(20.0, 0.0, 15.0))
}

/// The moved side picked on what a combine before the align merged into
/// the moved body is named on that body, as the document wants, and the
/// align works: the plate, holding the right disc, aligned by that disc's
/// top rim onto the left disc's, face to face (turned over about X).
#[test]
fn a_pick_on_what_was_merged_into_the_moved_body_is_named_on_it() {
    let (mut plates, plate, left, top) = combined();
    plates.doc.look(Look::StartAlign);
    plates.click(plate);
    let moved_rim = rim(&plates, plate, top);
    plates.click_at(plate, Picked::Edge(moved_rim), top + DVec3::X * 5.0);
    assert_eq!(plates.doc.notice, None);
    let other = DVec3::new(-20.0, 0.0, 15.0);
    let target_rim = rim(&plates, left, other);
    plates.click_at(left, Picked::Edge(target_rim), other + DVec3::X * 5.0);
    assert_eq!(picking(&plates), MotionPick::Nothing);
    let align = drafted(&plates).expect("the align previewed");
    assert!(matches!(align.from.point, PointRef::Centre(edge) if edge.body == plate));
    assert!(align.from.directions().all(|d| d.body() == Some(plate)));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // Turned over about X, (20, 0, 15) onto (−20, 0, 15): x − 40, −y,
    // 30 − z; the plate from z 0 to 10, the disc from −5 to 15.
    let [low, high] = plates.bounds(plate);
    assert!(near(low, DVec3::new(-70.0, -20.0, 15.0)), "{low}");
    assert!(near(high, DVec3::new(-10.0, 20.0, 35.0)), "{high}");
}

/// Another body picked to move takes out what's picked on the target
/// side on a body a combine before the align merged into it.
#[test]
fn a_target_merged_into_the_body_picked_to_move_is_taken_out() {
    let (mut plates, plate, left, top) = combined();
    plates.doc.look(Look::StartAlign);
    plates.click(left);
    let other = DVec3::new(-20.0, 0.0, 15.0);
    let moved_rim = rim(&plates, left, other);
    plates.click_at(left, Picked::Edge(moved_rim), other + DVec3::X * 5.0);
    let target_rim = rim(&plates, plate, top);
    plates.click_at(plate, Picked::Edge(target_rim), top + DVec3::X * 5.0);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(session.align.sides[1].point.is_some());
    plates.motion(MotionLook::Picking(MotionPick::Bodies));
    plates.click(plate);
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.bodies, [plate]);
    assert_eq!(
        session.align.sides,
        <[crate::doc::motion::align::Side; 2]>::default()
    );
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Point));
}

/// A target picked on another body, then merged into the body aligned
/// by a combine added after it was picked, is said to be gone, as
/// regenerating would refuse it: the session isn't ready until the
/// combine is undone.
#[test]
fn a_target_a_later_combine_merges_into_the_moved_body_is_gone() {
    let mut plates = super::plates();
    let [_, right, left] = plates.bodies;
    plates.doc.look(Look::StartAlign);
    plates.click(left);
    let other = DVec3::new(-20.0, 0.0, 15.0);
    let moved_rim = rim(&plates, left, other);
    plates.click_at(left, Picked::Edge(moved_rim), other + DVec3::X * 5.0);
    let top = DVec3::new(20.0, 0.0, 15.0);
    let target_rim = rim(&plates, right, top);
    plates.click_at(right, Picked::Edge(target_rim), top + DVec3::X * 5.0);
    assert!(plates.doc.motion_ready());
    let combine = varde_document::Combine {
        target: left,
        tools: vec![right],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    let add = plates.doc.editor.document().add_feature(combine.into());
    plates.doc.apply(add);
    plates.doc.sync();
    plates.answer();
    assert!(plates.doc.motion.is_some());
    assert!(!plates.doc.motion_ready());
    assert!(shows(
        &plates,
        "The point it's aligned to is in the body aligned now: pick another"
    ));
    plates.doc.update(Edit::Undo);
    plates.answer();
    assert!(plates.doc.motion_ready());
}

/// An edited align's references, picked again, are lit and drawn on the
/// model as of the align, found by their names: its faces in each
/// side's colour, its corners as dots.
#[test]
fn an_edited_aligns_references_are_found_on_the_model_shown() {
    let (mut plates, [top, below], id) = face_to_face();
    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Target,
        AlignRole::Secondary,
    )));
    plates.answer();
    let bottom = flat(&plates, below, DVec3::NEG_Z);
    let top_face = flat(&plates, top, DVec3::Z);
    assert_eq!(
        plates.doc.align_lit(),
        [vec![Picked::Face(bottom)], vec![Picked::Face(top_face)]]
    );
    let state = plates.doc.motion_state().unwrap();
    let marks = state.align.as_ref().unwrap().marks;
    let point = |k: usize| marks[k].point.expect("drawn");
    assert!(near(point(0), DVec3::new(30.0, 20.0, -3.0)), "{}", point(0));
    assert!(near(point(1), DVec3::new(30.0, 20.0, 10.0)), "{}", point(1));
}

/// A pin's foot rim picked as its point, its axis taken out, then the
/// same rim clicked by hand for the direction (anywhere along it): it's
/// named as the point's rim, so it's that rim's axis and goes with the
/// point, as the axis the point brought would. Deliberate: a direction on
/// the point's own rim is the point's axis however it was picked, so the
/// two never part (the point picked again elsewhere would leave an axis
/// on a rim no longer picked).
#[test]
fn a_direction_picked_by_hand_on_the_points_rim_goes_with_it() {
    let (mut plates, _, pin) = pin_and_plate();
    plates.doc.look(Look::StartAlign);
    plates.click(pin);
    let foot = rim(&plates, pin, DVec3::new(100.0, 0.0, 0.0));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(108.0, 0.0, 0.0));
    let primary = AlignSlot::new(AlignSide::Moved, AlignRole::Primary);
    plates.motion(MotionLook::Clear(primary));
    let moved = plates.doc.motion.as_ref().unwrap().align.sides[0];
    assert!(moved.point.is_some() && moved.primary.is_none());
    plates.motion(MotionLook::Picking(MotionPick::Align(primary)));
    // Clicked across the rim from where the point was named.
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(92.0, 0.0, 0.0));
    let moved = plates.doc.motion.as_ref().unwrap().align.sides[0];
    let (Some(PointRef::Centre(point)), Some(DirRef::Axis(AxisRef::Edge(axis)))) =
        (moved.point, moved.primary)
    else {
        panic!("{moved:?}");
    };
    assert_eq!(point, axis, "named alike");
    // The point picked again on the top rim takes it, and brings the top
    // rim's axis.
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Moved,
        AlignRole::Point,
    )));
    let top = rim(&plates, pin, DVec3::new(100.0, 0.0, 25.0));
    plates.click_at(pin, Picked::Edge(top), DVec3::new(108.0, 0.0, 25.0));
    let moved = plates.doc.motion.as_ref().unwrap().align.sides[0];
    let (Some(PointRef::Centre(point)), Some(DirRef::Axis(AxisRef::Edge(axis)))) =
        (moved.point, moved.primary)
    else {
        panic!("{moved:?}");
    };
    assert_eq!(point, axis);
    assert!((point.near.z - 25.0).abs() < 1e-3, "{}", point.near);
    // A direction picked by hand on another edge stays when the point
    // goes.
    plates.motion(MotionLook::Picking(MotionPick::Align(primary)));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(92.0, 0.0, 0.0));
    plates.motion(MotionLook::Clear(AlignSlot::new(
        AlignSide::Moved,
        AlignRole::Point,
    )));
    let moved = plates.doc.motion.as_ref().unwrap().align.sides[0];
    assert!(moved.point.is_none());
    assert!(
        matches!(moved.primary, Some(DirRef::Axis(AxisRef::Edge(edge))) if edge.near.z.abs() < 1e-3)
    );
}

/// The plates of [`two_plates`] with the align started on the lower one
/// and both its corners at (30, 20) picked: what's picked next is the
/// moved body's direction.
fn corners_picked() -> (Plates, [BodyId; 2]) {
    let (editor, [top, below]) = two_plates();
    let mut plates = plates_of(&editor);
    plates.click(below);
    plates.doc.look(Look::StartAlign);
    let at = DVec3::new(30.0, 20.0, -3.0);
    let (moved_corner, face) = corner(&plates, below, at);
    click_snapped(&mut plates, below, face, at, Snapped::Corner(moved_corner));
    let at = DVec3::new(30.0, 20.0, 10.0);
    let (target_corner, face) = corner(&plates, top, at);
    click_snapped(&mut plates, top, face, at, Snapped::Corner(target_corner));
    (plates, [top, below])
}

/// An align of points alone: picking stopped where it would go on to the
/// directions, it's whole and previewed as the move between the points,
/// stored with no flip even if Flip was clicked; a distance typed without
/// directions holds it back, said why.
#[test]
fn an_align_of_points_alone() {
    let (mut plates, [_, below]) = corners_picked();
    let direction = slot(AlignSide::Moved, AlignRole::Primary);
    assert_eq!(picking(&plates), direction);
    // Whole already: the status bar names it.
    assert!(plates.doc.motion_ready());
    assert!(shows(&plates, "Body 2 to Body 1"));
    plates.motion(MotionLook::Picking(direction));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    plates.motion(MotionLook::Flip);
    let align = drafted(&plates).expect("previewed");
    assert_eq!((align.from.primary, align.to.primary), (None, None));
    assert!(!align.flip && align.offset.is_none() && align.turn.is_none());
    plates.input(MotionField::Distance, "4");
    assert!(!plates.doc.motion_ready());
    assert!(shows(
        &plates,
        "pick a direction on each side to offset or turn along"
    ));
    plates.input(MotionField::Distance, "0");
    assert!(plates.doc.motion_ready());
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (_, kind) = plates.last_feature();
    assert_eq!(kind, FeatureKind::from(align));
    plates.answer();
    // Moved 13 up, corner onto corner: from z −3..0 to 10..13.
    let [low, high] = plates.bounds(below);
    assert!(near(low, DVec3::new(-30.0, -20.0, 10.0)), "{low}");
    assert!(near(high, DVec3::new(30.0, 20.0, 13.0)), "{high}");
}

/// The directions are asked for in the order clicks go on to them: the
/// moved body's first, then the target's, then a second direction where
/// the other side has one; a second direction left without first ones
/// (those taken out) asks for the first ones first, as clicks go.
#[test]
fn directions_are_asked_for_in_the_order_picked() {
    let (mut plates, [top, below]) = corners_picked();
    // The target's direction picked first: the body's is asked for next.
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Target,
        AlignRole::Primary,
    )));
    let top_face = flat(&plates, top, DVec3::Z);
    plates.click_at(top, Picked::Face(top_face), DVec3::new(0.0, 15.0, 10.0));
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Primary));
    assert!(shows(&plates, "pick a direction on the body"));
    let bottom = flat(&plates, below, DVec3::NEG_Z);
    plates.click_at(below, Picked::Face(bottom), DVec3::new(0.0, 15.0, -3.0));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    assert!(plates.doc.motion_ready());
    // A second direction on the body: the target's is asked for next.
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Moved,
        AlignRole::Secondary,
    )));
    let side = flat(&plates, below, DVec3::X);
    plates.click_at(below, Picked::Face(side), DVec3::new(30.0, 0.0, -1.5));
    assert_eq!(
        picking(&plates),
        slot(AlignSide::Target, AlignRole::Secondary)
    );
    assert!(shows(&plates, "pick the second direction to align it to"));
    assert!(!plates.doc.motion_ready());
    // The first directions taken out: they're asked for first, where
    // clicks go.
    for side in [AlignSide::Moved, AlignSide::Target] {
        plates.motion(MotionLook::Clear(AlignSlot::new(side, AlignRole::Primary)));
    }
    assert_eq!(picking(&plates), slot(AlignSide::Moved, AlignRole::Primary));
    assert!(shows(
        &plates,
        "pick a direction on each side before a second one"
    ));
    assert!(!plates.doc.motion_ready());
}

/// Adds a sketch on XY holding the rectangle from `a` to `b`, cut 15 up
/// and 5 down through what it touches.
fn cut_rectangle(editor: &mut Editor, a: (f64, f64), b: (f64, f64)) {
    let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features().last().unwrap().id;
    let mut sketch = varde_sketch::Sketch::default();
    let corners = [(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1)]
        .map(|(x, y)| sketch.add_point(glam::DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let line = varde_sketch::Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        sketch.add_curve(line, false).unwrap();
    }
    let profiles = sketch.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let extrude = varde_document::Extrude {
        sketch: feature,
        regions,
        extent: crate::tests::two_sides(editor.document(), "15", "5"),
        flip: false,
        operation: Operation::Cut(varde_document::Targets::default()),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
}

/// The example plate's hole split by a slot across it: its top rim two
/// arcs between the same two faces (the plate's top and the hole's
/// wall). The far arc's centre picked lights that arc, named at a point
/// on it so it's found again there (not on the other arc) on the next
/// model; the align then finds the arcs' centre.
#[test]
fn two_arcs_with_the_same_keys() {
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().bodies()[0].id;
    cut_rectangle(&mut editor, (-2.0, -12.0), (2.0, 12.0));
    let tall = Extent::OneSide(length(editor.document(), "25"));
    add_disc_of(
        &mut editor,
        (100.0, 0.0),
        8.0,
        tall,
        Operation::NewBody(BodyId::NEW),
    );
    let pin = editor.document().bodies()[1].id;
    let mut plates = plates_of(&editor);
    // The hole's top arcs: two edges of the plate whose snap point is the
    // hole's centre, on either side of the slot.
    let index = plates.doc.feed.pick_index();
    let snaps = index.picking().snaps();
    let arcs: Vec<u32> = (0..snaps.len() as u32)
        .filter(|&edge| {
            index.body(Picked::Edge(edge)) == Some(plate)
                && snaps[edge as usize]
                    .is_some_and(|at| DVec3::from(at).distance(DVec3::new(0.0, 0.0, 10.0)) < 1e-9)
        })
        .collect();
    let [a, b] = arcs[..] else {
        panic!("two arcs: {arcs:?}");
    };
    assert_eq!(index.chain_keys(a), index.chain_keys(b), "the same keys");
    // The one on the −x side.
    let far = if index.chain_point(a).unwrap().x < 0.0 {
        a
    } else {
        b
    };
    plates.doc.look(Look::StartAlign);
    plates.click(pin);
    let foot = rim(&plates, pin, DVec3::new(100.0, 0.0, 0.0));
    plates.click_at(pin, Picked::Edge(foot), DVec3::new(108.0, 0.0, 0.0));
    click_snapped(
        &mut plates,
        plate,
        Picked::Edge(far),
        DVec3::new(0.0, 0.0, 10.0),
        Snapped::EdgePoint(far),
    );
    let target = plates.doc.motion.as_ref().unwrap().align.sides[1];
    let Some(PointRef::Centre(named)) = target.point else {
        panic!("{target:?}");
    };
    assert!(named.near.x < -2.0, "named on the far arc: {}", named.near);
    assert_eq!(plates.doc.align_lit()[1], [Picked::Edge(far)]);
    // Previewed and answered: a new model, the arc found again.
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    plates.motion(MotionLook::Picking(slot(
        AlignSide::Target,
        AlignRole::Point,
    )));
    plates.answer();
    let index = plates.doc.feed.pick_index();
    let lit = plates.doc.align_lit()[1].clone();
    let [Picked::Edge(found)] = lit[..] else {
        panic!("{lit:?}");
    };
    assert!(
        index.chain_point(found).unwrap().x < -2.0,
        "the far arc lit"
    );
    let state = plates.doc.motion_state().unwrap();
    let point = state.align.as_ref().unwrap().marks[1].point.expect("drawn");
    assert!(near(point, DVec3::new(0.0, 0.0, 10.0)), "{point}");
    // Done: the pin in the hole, its foot at the top.
    plates.motion(MotionLook::Picking(MotionPick::Nothing));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(pin);
    assert!(near(low, DVec3::new(-8.0, -8.0, 10.0)), "{low}");
    assert!(near(high, DVec3::new(8.0, 8.0, 35.0)), "{high}");
}

mod fuzz;
