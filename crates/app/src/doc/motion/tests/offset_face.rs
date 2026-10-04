//! The offset face session: the rail's Offset face on the example plate,
//! faces picked with a click (lit, listed with their kind), the panel,
//! the kernel's stand-in failing as too complex and Add anyway keeping
//! it; with regeneration moving boxes' faces
//! ([`varde_regen::testing::offset_by_boxes`]): the moved face
//! previewed, the handle standing on the face as it was with its knob at
//! the distance, a typed distance moving the knob, the handle's drag
//! through zero turning inward, Tangent faces, OK as one undo step;
//! editing from the Timeline; another body's faces refused.

use glam::{DVec2, DVec3};
use varde_document::{BodyId, Command, Document, Editor, FeatureKind, OffsetFace, Operation};
use varde_regen::Summary;
use varde_view::{
    Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, Pick, Picked, Picks,
};

use super::face_session::{add_box, boxes, click, face_pick, held, picked_faces as faces, shows};
use super::{Plates, enter, near};
use crate::tests::{key_in, screen_texts};

/// The offset the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<OffsetFace> {
    match plates.last_draft()?.1 {
        FeatureKind::OffsetFace(offset) => Some(offset),
        _ => None,
    }
}

/// The handle the panel's view has, if any.
fn handle(plates: &Plates) -> Option<varde_view::FaceHandle> {
    let state = plates.doc.motion_state()?;
    state.offset_face?.handle
}

/// The box's top, at z 10 (or wherever the model shown has it).
fn top(plates: &Plates, body: BodyId, z: f64) -> Pick {
    face_pick(plates, body, DVec3::Z, z, DVec3::new(20.0, 15.0, z))
}

/// The rail's Offset face on the example plate: no body until a face is
/// picked, the panel in the shell's style; the top clicked picked, lit
/// and listed; the preview fails as too complex (the kernel's offset
/// face isn't built), shown in the panel; OK waits, Add anyway keeps it
/// as one undo step, failing in the Timeline.
#[test]
fn offset_face_picks_faces_and_add_anyway_keeps_the_too_complex_offset() {
    let (mut plates, plate) = {
        let plates = held(Document::example());
        let plate = plates.bodies[0];
        (plates, plate)
    };
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartOffsetFace);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::OffsetFace);
    assert!(session.bodies.is_empty(), "its body is its faces'");
    assert_eq!(session.picking, MotionPick::Faces);
    assert_eq!(
        plates.doc.model_picking().expect("picking").picks,
        Picks::Faces
    );
    for text in [
        "New offset face",
        "Faces",
        "Click faces",
        "Distance",
        "Inward",
        "Tangent faces",
        "pick the faces to move",
        "Pick faces to move",
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
    plates.click_at(plate, Picked::Edge(edge), DVec3::new(0.0, -20.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a face can be moved")
    );

    let up = face_pick(&plates, plate, DVec3::Z, 10.0, DVec3::new(0.0, 15.0, 10.0));
    click(&mut plates, up);
    assert_eq!(faces(&plates).len(), 1);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate]);
    assert_eq!(plates.doc.faces_lit(), [up.target]);
    assert!(shows(&plates, "Face 1") && shows(&plates, "Planar face"));
    assert!(shows(&plates, "1 face · 1 mm outward"));
    let offset = drafted(&plates).expect("a draft");
    assert_eq!(offset.faces, faces(&plates));
    assert_eq!(offset.distance.value, 1.0);
    assert!(!offset.inward && offset.tangent);

    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
    assert!(shows(&plates, "Offset face fails"));
    assert!(shows(&plates, "Add anyway"));
    // The body is shown as it is: the handle stands on the top.
    let handle = handle(&plates).expect("a handle");
    assert!(near(handle.origin, DVec3::new(0.0, 15.0, 10.0)));
    assert!(near(handle.normal, DVec3::Z) && handle.at == 1.0);
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert!(matches!(kind, FeatureKind::OffsetFace(_)));
    assert_eq!(
        plates.doc.editor.document().feature(id).unwrap().name,
        "Offset face 1"
    );
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the offset fails"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// With regeneration moving boxes' faces: the top moved out 1 mm
/// previewed, the handle on the top as it was with its knob 1 up; a
/// typed distance moves the knob and the preview, the handle staying;
/// the handle's drag past zero turns it inward; Tangent faces drafted;
/// OK adds it as one undo step, the Timeline's row noting it.
#[test]
fn the_offset_is_previewed_the_handle_follows_and_ok_adds_one_undo_step() {
    varde_regen::testing::offset_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartOffsetFace);
    plates.answer();
    let pick = top(&plates, body, 10.0);
    click(&mut plates, pick);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(body);
    assert!(near(low, DVec3::ZERO) && near(high, DVec3::new(40.0, 30.0, 11.0)));
    let handle_now = handle(&plates).expect("a handle");
    assert!(
        near(handle_now.origin, DVec3::new(20.0, 15.0, 10.0)),
        "{}",
        handle_now.origin
    );
    assert_eq!(handle_now.at, 1.0);
    // The moved top is still the face picked: lit, and a click on it
    // takes it out.
    assert_eq!(plates.doc.faces_lit().len(), 1);

    // A typed distance moves the knob, and the preview; the handle stays
    // on the face as it was.
    plates.input(MotionField::Distance, "3");
    assert_eq!(handle(&plates).unwrap().at, 3.0);
    assert_eq!(drafted(&plates).unwrap().distance.value, 3.0);
    plates.answer();
    assert!(near(plates.bounds(body)[1], DVec3::new(40.0, 30.0, 13.0)));
    let handle_now = handle(&plates).unwrap();
    assert!(near(handle_now.origin, DVec3::new(20.0, 15.0, 10.0)));

    // The handle dragged below the face: 2 mm inward.
    plates.motion(MotionLook::OffsetBy {
        distance: "2 mm".to_owned(),
        inward: true,
    });
    let offset = drafted(&plates).unwrap();
    assert!(offset.inward && offset.distance.value == 2.0);
    assert_eq!(handle(&plates).unwrap().at, -2.0);
    assert!(shows(&plates, "1 face · 2 mm inward"));
    plates.answer();
    assert!(near(plates.bounds(body)[1], DVec3::new(40.0, 30.0, 8.0)));
    assert!(near(
        handle(&plates).unwrap().origin,
        DVec3::new(20.0, 15.0, 10.0)
    ));
    // Inward ticked off: out again.
    plates.motion(MotionLook::Flip);
    assert!(!drafted(&plates).unwrap().inward);
    assert_eq!(handle(&plates).unwrap().at, 2.0);
    plates.motion(MotionLook::Flip);
    plates.motion(MotionLook::TangentFaces);
    assert!(!drafted(&plates).unwrap().tangent);
    // A distance of nothing is refused.
    plates.input(MotionField::Distance, "0");
    assert!(!plates.doc.motion_ready());
    plates.input(MotionField::Distance, "2");
    plates.answer();
    assert!(plates.doc.motion_ready());

    let offset = (plates.doc.motion.as_ref()).and_then(|session| session.offset_face());
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert_eq!(Some(kind), offset.map(FeatureKind::OffsetFace));
    assert_eq!(plates.doc.selected_feature, Some(id));
    plates.answer();
    assert!(near(plates.bounds(body)[1], DVec3::new(40.0, 30.0, 8.0)));
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    assert!(shows(&plates, "Offset face 1") && shows(&plates, "2 mm in · 1 face"));
    assert!(shows(&plates, "1 face · 2 mm inward"));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// Editing an offset from the Timeline opens it with its faces and
/// values; another distance, OK, one undo step; Cancel leaves no trace.
#[test]
fn editing_an_offset_face_from_the_timeline() {
    varde_regen::testing::offset_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    plates.doc.look(Look::StartOffsetFace);
    plates.answer();
    let pick = top(&plates, body, 10.0);
    click(&mut plates, pick);
    plates.answer();
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    plates.answer();
    let committed = plates.doc.editor.document().clone();

    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("editing");
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.faces.refs.len(), 1);
    assert!(shows(&plates, "Offset face 1"));
    plates.answer();
    // The handle stands on the top as it was before the offset.
    assert!(near(
        handle(&plates).expect("a handle").origin,
        DVec3::new(20.0, 15.0, 10.0)
    ));
    plates.motion(MotionLook::Cancel);
    assert_eq!(*plates.doc.editor.document(), committed);

    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::Distance, "4");
    plates.answer();
    assert!(near(plates.bounds(body)[1], DVec3::new(40.0, 30.0, 14.0)));
    plates.doc.update(Edit::CommitMotion);
    let FeatureKind::OffsetFace(edited) = &plates.doc.editor.document().feature(id).unwrap().kind
    else {
        panic!("an offset face");
    };
    assert_eq!(edited.distance.value, 4.0);
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), committed);
}

/// An offset face's faces are all on one body: once one is picked, another
/// body's face is refused with why, and a body's row does nothing.
#[test]
fn an_offset_face_s_faces_are_all_on_one_body() {
    let mut plates = boxes(true);
    let [first, second, _] = plates.bodies;
    plates.doc.look(Look::StartOffsetFace);
    assert!(plates.doc.motion.as_ref().unwrap().bodies.is_empty());
    // A body's row picks nothing: its body is its faces'.
    plates.doc.look(Look::ClickBody {
        body: second,
        add: false,
    });
    assert!(plates.doc.motion.as_ref().unwrap().bodies.is_empty());
    let pick = top(&plates, first, 10.0);
    click(&mut plates, pick);
    let theirs = face_pick(
        &plates,
        second,
        DVec3::Z,
        10.0,
        DVec3::new(70.0, 10.0, 10.0),
    );
    assert!(!plates.doc.takes_reference(theirs));
    click(&mut plates, theirs);
    assert_eq!(faces(&plates).len(), 1);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("An offset face's faces are all on one body: pick faces of Body 1")
    );
    plates.doc.look(Look::ClickBody {
        body: second,
        add: false,
    });
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("An offset face's faces are all on one body: take them out to pick another")
    );
    // Taking the face out leaves nothing to commit.
    let face = faces(&plates)[0];
    plates.motion(MotionLook::DropFace(face));
    assert!(plates.doc.motion.as_ref().unwrap().bodies.is_empty());
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "pick the faces to move"));
}

/// The box's front, at y 0 (or wherever the model shown has it).
fn front(plates: &Plates, body: BodyId, y: f64) -> Pick {
    face_pick(plates, body, -DVec3::Y, -y, DVec3::new(20.0, y, 5.0))
}

/// Answers every request but the last, which stays on its way.
fn answer_all_but_the_last(plates: &mut Plates) {
    let mut requests = plates.requests.take();
    let last = requests.pop();
    for request in requests {
        plates.doc.computed(varde_regen::handle(request));
    }
    plates.requests.borrow_mut().extend(last);
}

/// Inward ticked and the handle dragged through zero to the same
/// distance give the same draft and the same preview, out and in, in
/// millimetres and in inches.
#[test]
fn inward_ticked_or_the_handle_dragged_through_zero_give_the_same_draft() {
    varde_regen::testing::offset_by_boxes();
    for (units, typed, dragged, top_at) in [
        (varde_expr::LengthUnit::Mm, "2", "2 mm", 8.0),
        (varde_expr::LengthUnit::In, "0.1", "0.1 in", 10.0 - 2.54),
    ] {
        let mut plates = boxes(false);
        let body = plates.bodies[0];
        plates.doc.update(Edit::SetUnits(units));
        plates.answer();
        plates.doc.look(Look::StartOffsetFace);
        plates.answer();
        let pick = top(&plates, body, 10.0);
        click(&mut plates, pick);
        plates.answer();
        plates.input(MotionField::Distance, typed);
        plates.motion(MotionLook::Flip);
        let ticked = drafted(&plates).expect("a draft");
        assert_eq!(
            handle(&plates).map(|handle| handle.at),
            Some(-ticked.distance.value)
        );
        plates.answer();
        assert!(near(plates.bounds(body)[1], DVec3::new(40.0, 30.0, top_at)));
        let shown = plates.bounds(body);
        assert_eq!(handle(&plates).unwrap().at, -ticked.distance.value);

        // Out again, then dragged down through zero.
        plates.motion(MotionLook::Flip);
        assert!(!drafted(&plates).unwrap().inward);
        plates.answer();
        plates.motion(MotionLook::OffsetBy {
            distance: dragged.to_owned(),
            inward: true,
        });
        let dragged = drafted(&plates).expect("a draft");
        assert_eq!(dragged.faces, ticked.faces);
        assert_eq!(dragged.inward, ticked.inward);
        assert_eq!(dragged.tangent, ticked.tangent);
        assert!(
            (dragged.distance.value - ticked.distance.value).abs() < 1e-12,
            "{dragged:?} vs {ticked:?}"
        );
        assert_eq!(handle(&plates).unwrap().at, -dragged.distance.value);
        plates.answer();
        assert_eq!(plates.bounds(body), shown);
        // And back out by the tick: the same as dragged up.
        plates.motion(MotionLook::Flip);
        let ticked_out = drafted(&plates).unwrap();
        assert!(!ticked_out.inward && ticked_out.distance.value == dragged.distance.value);
    }
}

/// The handle dragged while the previews are on their way, and the first
/// face changed before they come (the top taken out, the front picked):
/// no handle while the models shown aren't of what was asked last, then
/// the handle stands on the front as it is before the offset (y 0), its
/// knob at the distance, on the front as the preview moved it.
#[test]
fn a_drag_while_the_preview_is_on_its_way_and_the_first_face_changed() {
    varde_regen::testing::offset_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    plates.doc.look(Look::StartOffsetFace);
    plates.answer();
    let pick = top(&plates, body, 10.0);
    click(&mut plates, pick);
    plates.answer();
    assert!(near(
        handle(&plates).expect("a handle").origin,
        DVec3::new(20.0, 15.0, 10.0)
    ));
    // Dragged up, then down through zero, nothing answered.
    plates.motion(MotionLook::OffsetBy {
        distance: "3 mm".to_owned(),
        inward: false,
    });
    plates.motion(MotionLook::OffsetBy {
        distance: "2 mm".to_owned(),
        inward: true,
    });
    // The front picked on the model shown (the top moved 1 mm out), the
    // top taken out.
    let pick = front(&plates, body, 0.0);
    click(&mut plates, pick);
    let top = faces(&plates)
        .into_iter()
        .find(|face| face.near.z > 9.0)
        .expect("the top");
    plates.motion(MotionLook::DropFace(top));
    assert_eq!(faces(&plates).len(), 1);
    assert!(handle(&plates).is_none(), "{:?}", handle(&plates));
    // The older answers come: the model shown isn't of what was asked
    // last, so no handle yet.
    answer_all_but_the_last(&mut plates);
    assert!(handle(&plates).is_none(), "{:?}", handle(&plates));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // The front 2 mm in.
    assert!(near(plates.bounds(body)[0], DVec3::new(0.0, 2.0, 0.0)));
    let handle = handle(&plates).expect("a handle");
    assert!(near(handle.normal, -DVec3::Y), "{handle:?}");
    assert!(handle.origin.y.abs() < 1e-4, "{handle:?}");
    assert_eq!(handle.at, -2.0);
    let knob = handle.origin + handle.normal * handle.at;
    assert!((knob.y - 2.0).abs() < 1e-4, "{knob}");
}

/// Held open in an offset face session, the list of what overlaps ticks
/// the session's faces, picks and takes them out, and follows each
/// preview: a moved face keeps its name, so its row stays.
#[test]
fn the_overlap_list_ticks_picks_and_follows_the_offset_s_faces() {
    varde_regen::testing::offset_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    plates.doc.look(Look::StartOffsetFace);
    plates.answer();
    let list = varde_view::Overlaps {
        held: DVec2::ZERO,
        at: DVec2::ZERO,
        items: varde_view::OverlapItems::Model(vec![
            top(&plates, body, 10.0),
            front(&plates, body, 0.0),
        ]),
    };
    plates.doc.look(Look::OpenOverlaps(list));
    assert_eq!(plates.doc.overlap_ticks(), Some(vec![false, false]));
    plates.doc.look(Look::ToggleOverlap(0));
    assert_eq!(faces(&plates).len(), 1);
    assert_eq!(plates.doc.overlap_ticks(), Some(vec![true, false]));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // The top moved out: still listed, ticked.
    assert_eq!(plates.doc.overlap_ticks(), Some(vec![true, false]));
    plates.doc.look(Look::ToggleOverlap(1));
    assert_eq!(faces(&plates).len(), 2);
    plates.answer();
    assert_eq!(plates.doc.overlap_ticks(), Some(vec![true, true]));
    let listed = plates.doc.overlaps.as_ref().expect("still open");
    let varde_view::OverlapItems::Model(picks) = &listed.list.items else {
        panic!("the model's");
    };
    assert!(
        picks
            .iter()
            .all(|pick| pick.model == plates.doc.feed.model())
    );
    plates.doc.look(Look::ToggleOverlap(0));
    assert_eq!(faces(&plates).len(), 1);
    plates.doc.look(Look::ChooseOverlap {
        index: 1,
        add: false,
    });
    assert!(plates.doc.overlaps.is_none());
    assert!(faces(&plates).is_empty());
}

/// The handle found on a preview that moved the face far out stands
/// where the face was, exactly: a flat face's point and normal are its
/// plane's, not the drawn mesh's single precision (10 000 mm out, that
/// was 0.4 µm off, and stayed so as the distance came back).
#[test]
fn a_handle_found_on_a_face_moved_far_out_stands_where_it_was_exactly() {
    varde_regen::testing::offset_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    plates.doc.look(Look::StartOffsetFace);
    plates.answer();
    let pick = face_pick(&plates, body, DVec3::X, 40.0, DVec3::new(40.0, 0.1, 0.1));
    click(&mut plates, pick);
    plates.input(MotionField::Distance, "9999.9");
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let handle_now = handle(&plates).expect("a handle");
    assert_eq!(handle_now.normal, DVec3::X);
    assert_eq!(handle_now.origin.x, 40.0, "{handle_now:?}");
    plates.input(MotionField::Distance, "2.5");
    plates.answer();
    let handle_now = handle(&plates).expect("a handle");
    assert_eq!(handle_now.origin.x + handle_now.at, 42.5, "{handle_now:?}");
}

mod fuzz;
