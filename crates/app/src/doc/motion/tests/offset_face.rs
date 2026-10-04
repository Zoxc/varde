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
use varde_document::{
    BodyId, Command, Document, Editor, Extent, Extrude, FeatureKind, OffsetFace, Operation,
    OriginPlane, Plane,
};
use varde_regen::Summary;
use varde_sketch::{Curve, Sketch};
use varde_view::{
    Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, Pick, Picked, Picks,
};

use super::{Plates, enter, near};
use crate::tests::{holding, key_in, screen_texts};

fn shows(plates: &Plates, wanted: &str) -> bool {
    screen_texts(&plates.doc)
        .iter()
        .any(|text| text.contains(wanted))
}

/// The offset the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<OffsetFace> {
    match plates.last_draft()?.1 {
        FeatureKind::OffsetFace(offset) => Some(offset),
        _ => None,
    }
}

/// The faces of the offset being set up.
fn faces(plates: &Plates) -> Vec<varde_document::FaceRef> {
    (plates.doc.motion.as_ref().expect("a session").faces.refs).clone()
}

/// The handle the panel's view has, if any.
fn handle(plates: &Plates) -> Option<varde_view::FaceHandle> {
    let state = plates.doc.motion_state()?;
    state.offset_face?.handle
}

/// The doc holding `document`, its bodies in order.
fn held(document: Document) -> Plates {
    let bodies = document.bodies();
    let at = |i: usize| bodies.get(i).or(bodies.first()).expect("a body").id;
    let bodies = [at(0), at(1), at(2)];
    let (doc, requests) = holding(document);
    Plates {
        doc,
        requests,
        bodies,
    }
}

/// Adds a sketch on XY holding the rectangle from `a` to `b`, extruded
/// 10 up as a new body.
fn add_box(editor: &mut Editor, a: DVec2, b: DVec2) {
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let mut drawn = Sketch::default();
    let corners = [(a.x, a.y), (b.x, a.y), (b.x, b.y), (a.x, b.y)]
        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let line = Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        drawn.add_curve(line, false).unwrap();
    }
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
    let extrude = Extrude {
        sketch,
        regions,
        extent: Extent::OneSide(crate::tests::length(editor.document(), "10")),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
}

/// A box from (0, 0, 0) to (40, 30, 10), "Body 1", and with `two` another
/// from (60, 0, 0) to (80, 20, 10), "Body 2".
fn boxes(two: bool) -> Plates {
    let mut editor = Editor::new(Document::default());
    add_box(&mut editor, DVec2::ZERO, DVec2::new(40.0, 30.0));
    if two {
        add_box(&mut editor, DVec2::new(60.0, 0.0), DVec2::new(80.0, 20.0));
    }
    held(editor.document().clone())
}

/// The pick of `body`'s flat face facing `normal` at `d`, at `at`.
fn face_pick(plates: &Plates, body: BodyId, normal: DVec3, d: f64, at: DVec3) -> Pick {
    let index = plates.doc.feed.pick_index();
    let face = (index.body_faces(body))
        .find(|&face| {
            matches!(index.picking().faces()[face as usize].summary,
                Summary::Plane { n, d: at } if DVec3::from(n).distance(normal) < 1e-9 && (at - d).abs() < 1e-9)
        })
        .expect("the face shown");
    Pick {
        model: index.model(),
        target: Picked::Face(face),
        body,
        at,
        snap: None,
    }
}

/// A click on `pick`.
fn click(plates: &mut Plates, pick: Pick) {
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: false,
        double: false,
    });
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
    assert!(shows(&plates, "Offset fails"));
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
        "Offset 1"
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
    assert!(shows(&plates, "Offset 1") && shows(&plates, "2 mm in · 1 face"));
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
    assert!(shows(&plates, "Offset 1"));
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

/// An offset's faces are all on one body: once one is picked, another
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
        Some("An offset's faces are all on one body: pick faces of Body 1")
    );
    plates.doc.look(Look::ClickBody {
        body: second,
        add: false,
    });
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("An offset's faces are all on one body: take them out to pick another")
    );
    // Taking the face out leaves nothing to commit.
    let face = faces(&plates)[0];
    plates.motion(MotionLook::DropFace(face));
    assert!(plates.doc.motion.as_ref().unwrap().bodies.is_empty());
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "pick the faces to move"));
}
