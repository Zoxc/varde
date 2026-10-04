//! The draft session: the rail's Draft on the example plate, faces
//! picked with a click (lit, listed with their kind), the panel with its
//! neutral plane (XY to begin with), the kernel's stand-in failing as
//! too complex and Add anyway keeping it; with regeneration drafting
//! boxes' sides ([`varde_regen::testing::draft_by_boxes`]): the drafted
//! face previewed, the neutral plane picked as a face or from the
//! toolbar's origin planes, Flip, the angle typed and refused, Tangent
//! faces, OK as one undo step; editing from the Timeline; the neutral
//! face an undo takes away said to be gone.

use glam::DVec3;
use varde_document::{Document, Editor, FaceDraft, FeatureKind, OriginPlane, PlaneRef};
use varde_regen::Summary;
use varde_view::{Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, Picked, Picks};

use super::offset_face::{add_box, boxes, click, face_pick, held, shows};
use super::{Plates, enter, near};
use crate::tests::key_in;

/// The draft the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<FaceDraft> {
    match plates.last_draft()?.1 {
        FeatureKind::FaceDraft(draft) => Some(draft),
        _ => None,
    }
}

fn picking(plates: &Plates) -> MotionPick {
    plates.doc.motion.as_ref().expect("a session").picking
}

/// The faces of the draft being set up.
fn faces(plates: &Plates) -> Vec<varde_document::FaceRef> {
    (plates.doc.motion.as_ref().expect("a session").faces.refs).clone()
}

/// The tangent of `degrees`.
fn tan(degrees: f64) -> f64 {
    degrees.to_radians().tan()
}

/// The rail's Draft on the example plate: no body until a face is
/// picked, the panel in the shell's style with the neutral plane XY;
/// an edge refused; a side clicked picked, lit and listed; the preview
/// fails as too complex (the kernel's draft isn't built), shown in the
/// panel; OK waits, Add anyway keeps it as one undo step, failing in the
/// Timeline.
#[test]
fn draft_picks_faces_and_add_anyway_keeps_the_too_complex_draft() {
    let mut plates = held(Document::example());
    let plate = plates.bodies[0];
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartDraft);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Draft);
    assert!(session.bodies.is_empty(), "its body is its faces'");
    assert_eq!(session.picking, MotionPick::Faces);
    assert_eq!(session.plane, Some(PlaneRef::Origin(OriginPlane::XY)));
    assert_eq!(
        plates.doc.model_picking().expect("picking").picks,
        Picks::Faces
    );
    for text in [
        "New draft",
        "Faces",
        "Click faces",
        "Neutral plane",
        "XY plane",
        "Angle",
        "Flip",
        "Tangent faces",
        "pick the faces to draft",
        "Pick faces to draft",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    assert!(!plates.doc.motion_ready());
    assert!(drafted(&plates).is_none());

    // An edge is no face.
    let index = plates.doc.feed.pick_index();
    let edge = (0..index.mesh().edge_count() as u32)
        .find(|&edge| index.body(Picked::Edge(edge)) == Some(plate))
        .expect("an edge");
    plates.click_at(plate, Picked::Edge(edge), DVec3::ZERO);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a face can be drafted")
    );

    // A side of the plate, square to Y.
    let side = plates.face(plate, |summary| {
        matches!(*summary, Summary::Plane { n, .. } if DVec3::from(n).distance(-DVec3::Y) < 1e-9)
    });
    let Summary::Plane { d, .. } =
        plates.doc.feed.pick_index().picking().faces()[side as usize].summary
    else {
        unreachable!()
    };
    plates.click_at(plate, Picked::Face(side), DVec3::new(0.0, -d, 5.0));
    assert_eq!(faces(&plates).len(), 1);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    assert_eq!(plates.doc.faces_lit(), [Picked::Face(side)]);
    assert!(shows(&plates, "Face 1") && shows(&plates, "Planar face"));
    assert!(shows(&plates, "1 face · 3° from XY"));
    let draft = drafted(&plates).expect("a draft");
    assert_eq!(draft.faces, faces(&plates));
    assert_eq!(draft.neutral, PlaneRef::Origin(OriginPlane::XY));
    assert!((draft.angle.value - 3f64.to_radians()).abs() < 1e-15);
    assert!(!draft.flip && draft.tangent);

    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
    assert!(shows(&plates, "Draft fails"));
    assert!(shows(&plates, "Add anyway"));
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert!(matches!(kind, FeatureKind::FaceDraft(_)));
    assert_eq!(
        plates.doc.editor.document().feature(id).unwrap().name,
        "Draft 1"
    );
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the draft fails"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// The front of the box, at y 0.
fn front(plates: &Plates, body: varde_document::BodyId) -> varde_view::Pick {
    face_pick(plates, body, -DVec3::Y, 0.0, DVec3::new(20.0, 0.0, 5.0))
}

/// With regeneration drafting boxes' sides: the front drafted 3° from
/// XY previewed (leaning in as it rises); the neutral plane picked as
/// the top (leaning out below it), its row toggling the picking; Flip;
/// an origin plane from the toolbar facing the front refused by
/// regeneration; an edge refused as the neutral plane; the angle typed
/// and one of 90° refused; Tangent faces; OK adds it as one undo step,
/// the Timeline's row noting it.
#[test]
fn the_draft_is_previewed_its_neutral_plane_picked_and_ok_adds_one_undo_step() {
    varde_regen::testing::draft_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartDraft);
    plates.answer();
    let pick = front(&plates, body);
    click(&mut plates, pick);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(body);
    assert!(near(low, DVec3::ZERO) && near(high, DVec3::new(40.0, 30.0, 10.0)));
    // The plane drawn by the body, the pull up.
    let state = plates.doc.motion_state().unwrap();
    let [at, pull] = state.line.expect("the neutral plane drawn");
    assert!(near(at, DVec3::new(20.0, 15.0, 0.0)) && near(pull, DVec3::Z));

    // The neutral plane's row picks it; clicked again, faces again.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    assert_eq!(picking(&plates), MotionPick::Reference);
    assert!(shows(&plates, "Pick the neutral plane"));
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    assert_eq!(picking(&plates), MotionPick::Faces);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    // An edge isn't a plane.
    let index = plates.doc.feed.pick_index();
    let edge = (0..index.mesh().edge_count() as u32)
        .find(|&edge| index.body(Picked::Edge(edge)) == Some(body))
        .expect("an edge");
    plates.click_at(body, Picked::Edge(edge), DVec3::ZERO);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a flat face can be the neutral plane")
    );
    // The top as the neutral plane: the front leans out below it.
    let top = face_pick(&plates, body, DVec3::Z, 10.0, DVec3::new(20.0, 15.0, 10.0));
    click(&mut plates, top);
    assert_eq!(picking(&plates), MotionPick::Faces);
    assert_eq!(faces(&plates).len(), 1, "the top isn't a face drafted");
    let draft = drafted(&plates).unwrap();
    assert!(matches!(draft.neutral, PlaneRef::Face(_)));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(near(
        plates.bounds(body)[0],
        DVec3::new(0.0, -10.0 * tan(3.0), 0.0)
    ));
    let [at, pull] = plates.doc.motion_state().unwrap().line.expect("drawn");
    // On the top, by the middle of the drafted box.
    assert!(
        (at.z - 10.0).abs() < 1e-9 && near(pull, DVec3::Z),
        "{at} {pull}"
    );
    // Flipped: the pull down, the front leaning in below the top.
    plates.motion(MotionLook::Flip);
    assert!(drafted(&plates).unwrap().flip);
    assert!(shows(&plates, ", flipped"));
    plates.answer();
    assert!(near(plates.bounds(body)[0], DVec3::ZERO));
    let [_, pull] = plates.doc.motion_state().unwrap().line.expect("drawn");
    assert!(near(pull, -DVec3::Z));
    plates.motion(MotionLook::Flip);

    // XZ from the toolbar: the front faces the pull, which regeneration
    // refuses.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.motion(MotionLook::OriginPlane(OriginPlane::XZ));
    assert_eq!(picking(&plates), MotionPick::Faces);
    assert!(shows(&plates, "XZ plane"));
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("refused");
    assert!(error.contains("faces the pull direction"), "{error}");
    // An origin plane only while the plane picks.
    plates.motion(MotionLook::OriginPlane(OriginPlane::XY));
    let session = plates.doc.motion.as_ref().unwrap();
    assert_eq!(session.plane, Some(PlaneRef::Origin(OriginPlane::XZ)));
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.motion(MotionLook::OriginPlane(OriginPlane::XY));

    // Another angle; one of 90° refused.
    plates.input(MotionField::Angle, "10");
    assert!((drafted(&plates).unwrap().angle.value - 10f64.to_radians()).abs() < 1e-15);
    plates.answer();
    let [_, high] = plates.bounds(body);
    assert!(near(high, DVec3::new(40.0, 30.0, 10.0)));
    plates.input(MotionField::Angle, "90");
    assert!(!plates.doc.motion_ready());
    plates.input(MotionField::Angle, "10");
    plates.motion(MotionLook::TangentFaces);
    assert!(!drafted(&plates).unwrap().tangent);
    plates.motion(MotionLook::TangentFaces);
    plates.answer();
    assert!(plates.doc.motion_ready());

    let draft = (plates.doc.motion.as_ref()).and_then(|session| session.face_draft());
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert_eq!(Some(kind), draft.map(FeatureKind::FaceDraft));
    assert_eq!(plates.doc.selected_feature, Some(id));
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    assert!(shows(&plates, "Draft 1") && shows(&plates, "10° · 1 face"));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// Editing a draft from the Timeline opens it with its faces, plane and
/// values; Cancel leaves no trace; another angle, OK, one undo step.
#[test]
fn editing_a_draft_from_the_timeline() {
    varde_regen::testing::draft_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    plates.doc.look(Look::StartDraft);
    plates.answer();
    let pick = front(&plates, body);
    click(&mut plates, pick);
    plates.motion(MotionLook::Flip);
    plates.answer();
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    plates.answer();
    let committed = plates.doc.editor.document().clone();

    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing");
    assert_eq!(session.kind, MotionKind::Draft);
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.faces.refs.len(), 1);
    assert!(session.flip);
    assert!(shows(&plates, "Draft 1") && shows(&plates, "XY plane"));
    plates.motion(MotionLook::Cancel);
    assert_eq!(*plates.doc.editor.document(), committed);

    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::Angle, "5");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    plates.doc.update(Edit::CommitMotion);
    let FeatureKind::FaceDraft(edited) = &plates.doc.editor.document().feature(id).unwrap().kind
    else {
        panic!("a draft");
    };
    assert!((edited.angle.value - 5f64.to_radians()).abs() < 1e-15);
    assert!(edited.flip);
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), committed);
}

/// The neutral plane a face of another body, which an undo takes away:
/// said to be gone, nothing committed; back on redo.
#[test]
fn a_neutral_face_an_undo_takes_away_is_gone() {
    let mut editor = Editor::new(Document::default());
    add_box(&mut editor, glam::DVec2::ZERO, glam::DVec2::new(40.0, 30.0));
    let mut plates = held(editor.document().clone());
    let body = plates.bodies[0];
    // A second box, as edits the session's undo takes away.
    add_box(
        &mut plates.doc.editor,
        glam::DVec2::new(60.0, 0.0),
        glam::DVec2::new(80.0, 20.0),
    );
    plates.doc.sync();
    let second = plates.doc.editor.document().bodies()[1].id;
    plates.answer();

    plates.doc.look(Look::StartDraft);
    plates.answer();
    let pick = front(&plates, body);
    click(&mut plates, pick);
    plates.answer();
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    let other = face_pick(
        &plates,
        second,
        DVec3::Z,
        10.0,
        DVec3::new(70.0, 10.0, 10.0),
    );
    click(&mut plates, other);
    let draft = drafted(&plates).expect("a draft");
    assert!(matches!(draft.neutral, PlaneRef::Face(face) if face.body == second));
    assert!(shows(&plates, "1 face · 3° from Extrude 2's end"));

    plates.doc.update(Edit::Undo);
    assert!(plates.doc.motion.is_some(), "the session stays");
    assert!(shows(&plates, "The neutral plane is gone: pick another"));
    assert!(!plates.doc.motion_ready());
    assert!(drafted(&plates).is_none());
    plates.doc.update(Edit::Redo);
    assert!(!shows(&plates, "The neutral plane is gone"));
    assert!(drafted(&plates).is_some());
}
