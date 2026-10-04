//! The shell session: the rail's Shell on the example plate, faces
//! picked and toggled with a click (lit, sorted, all on one body:
//! another body's neither lit nor taken), none for a closed one with the
//! mock's warning, the kernel's stand-in failing as too complex in the
//! panel and Add anyway keeping it; with regeneration shelling boxes
//! ([`varde_regen::testing`]), the hollow previewed open and closed, the
//! thickness and direction drafted, too thick refused, OK as one undo
//! step; editing from the Timeline, Cancel and undo; a face an undo takes
//! away said to be gone; the face selected taken in (on two bodies, the
//! first one's); faces and the body following a merge before the shell;
//! the toolbar fitting at 1280 px.

use glam::{DVec2, DVec3};
use varde_document::{BodyId, Command, Document, Editor, FeatureId, FeatureKind, Operation, Shell};
use varde_regen::Summary;
use varde_view::{
    Edit, Look, MotionField, MotionKind, MotionLook, MotionPick, PanelHover, Pick, Picked, Picks,
    Selection, SelectionMode, ShellDirection,
};

use super::face_session::{
    add_box, boxes, click, face_pick, flat, held, picked_faces as faces, picking, shows,
};
use super::{Plates, enter, near};
use crate::tests::{add_join, key_in, screen_texts};

/// The shell the last request previews, if it previews one.
fn drafted(plates: &Plates) -> Option<Shell> {
    match plates.last_draft()?.1 {
        FeatureKind::Shell(shell) => Some(shell),
        _ => None,
    }
}

/// The example's plate alone, "Body 1": 60 × 40 × 10 about the Z axis
/// from z 0 up, a hole of radius 8 through it about the Z axis.
fn plate() -> (Plates, BodyId) {
    let plates = held(Document::example());
    let plate = plates.bodies[0];
    (plates, plate)
}

/// The box's top, at z 10.
fn top(plates: &Plates, body: BodyId) -> Pick {
    face_pick(plates, body, DVec3::Z, 10.0, DVec3::new(20.0, 15.0, 10.0))
}

/// The box's front, at y 0.
fn front(plates: &Plates, body: BodyId) -> Pick {
    face_pick(plates, body, -DVec3::Y, 0.0, DVec3::new(20.0, 0.0, 5.0))
}

/// The rail's Shell with the example's only body: a shell of it picking
/// faces, the mock's panel, closed to begin with and said so; an edge
/// refused; faces clicked picked, lit and listed sorted with their kind,
/// a second click taking one out; the preview fails as too complex (the
/// kernel's shell isn't built), shown in the panel; OK waits, Add anyway
/// keeps it as one undo step.
#[test]
fn shell_picks_and_toggles_faces_and_add_anyway_keeps_the_too_complex_shell() {
    let (mut plates, plate) = plate();
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartShell);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Shell);
    assert_eq!(session.bodies, [plate], "the only body");
    assert_eq!(picking(&plates), MotionPick::Faces);
    assert_eq!(
        plates.doc.model_picking().expect("picking").picks,
        Picks::Faces
    );
    for text in [
        "New shell",
        "Remove",
        "Click faces",
        "Thickness",
        "Direction",
        "Inward",
        "Outward",
        "Closed · 2 mm inward",
    ] {
        assert!(shows(&plates, text), "{text}");
    }
    // Whole with no faces: a closed shell, previewed, and the mock's
    // warning while nothing else is said.
    assert!(plates.doc.motion_ready());
    let shell = drafted(&plates).expect("a closed shell's draft");
    assert!(shell.open.is_empty() && !shell.outward && shell.body == plate);
    assert_eq!(shell.thickness.value, 2.0);
    assert!(shows(
        &plates,
        "No faces removed: the body becomes closed and hollow"
    ));

    // An edge is no face.
    let index = plates.doc.feed.pick_index();
    let edge = (0..index.mesh().edge_count() as u32)
        .find(|&edge| index.body(Picked::Edge(edge)) == Some(plate))
        .expect("an edge");
    plates.click_at(plate, Picked::Edge(edge), DVec3::new(0.0, -20.0, 10.0));
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("Only a face can be removed")
    );

    // The top hovered lights; clicked, it's picked, lit as selected and
    // listed with its kind.
    let up = face_pick(&plates, plate, DVec3::Z, 10.0, DVec3::new(0.0, 15.0, 10.0));
    plates.doc.look(Look::Hover(Some(up)));
    assert!(!plates.doc.motion_highlight().expect("lit").is_empty());
    click(&mut plates, up);
    assert_eq!(faces(&plates).len(), 1);
    assert_eq!(plates.doc.faces_lit(), [up.target]);
    assert!(shows(&plates, "Face 1") && shows(&plates, "Planar face"));
    assert!(shows(&plates, "1 face removed · 2 mm inward"));
    assert!(!shows(&plates, "No faces removed"));
    assert_eq!(drafted(&plates).unwrap().open, faces(&plates));
    // Its row hovered in the panel lights it as hovered too.
    plates.doc.look(Look::Hover(None));
    let picked = plates.doc.motion_highlight().expect("lit").clone();
    plates.doc.look(Look::HoverPanel(Some(PanelHover::Face(0))));
    let hovered = plates.doc.motion_highlight().expect("lit").clone();
    assert_ne!(picked, hovered);
    plates.doc.look(Look::LeavePanel(PanelHover::Face(0)));
    assert_eq!(plates.doc.motion_highlight(), Some(&picked));

    // Two more, at once, sorted as the feature keeps them; the hole's
    // wall listed as cylindrical; a second click on the underside takes
    // it out again.
    let under = face_pick(&plates, plate, -DVec3::Z, 0.0, DVec3::new(0.0, 15.0, 0.0));
    let hole = plates.face(plate, |s| matches!(s, Summary::Cylinder { .. }));
    click(&mut plates, under);
    plates.click_at(plate, Picked::Face(hole), DVec3::new(8.0, 0.0, 5.0));
    let picked = faces(&plates);
    assert_eq!(picked.len(), 3);
    assert!(
        picked
            .windows(2)
            .all(|pair| pair[0].order(&pair[1]).is_lt())
    );
    assert!(shows(&plates, "Face 3") && shows(&plates, "Cylindrical face"));
    click(&mut plates, under);
    assert_eq!(faces(&plates).len(), 2);
    assert!(!shows(&plates, "Face 3"));
    assert_eq!(drafted(&plates).unwrap().open, faces(&plates));

    // The stand-in fails as too complex: the panel says so, and the body
    // is shown whole, its faces there to pick.
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("the stand-in fails");
    assert!(error.contains("too complex"), "{error}");
    assert!(shows(&plates, "Shell fails"));
    assert!(shows(&plates, "Add anyway"));
    let state = plates.doc.motion_state().unwrap();
    assert!(!state.ready && state.accept);
    assert_eq!(plates.doc.faces_lit().len(), 2);
    let features = plates.doc.editor.document().features().len();
    key_in(&mut plates.doc, enter());
    assert_eq!(plates.doc.editor.document().features().len(), features);

    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    let FeatureKind::Shell(stored) = &kind else {
        panic!("a shell");
    };
    assert_eq!(stored.open.len(), 2);
    let document = plates.doc.editor.document();
    assert_eq!(document.feature(id).unwrap().name, "Shell 1");
    assert_eq!(plates.doc.selected_feature, Some(id));
    plates.answer();
    assert!(
        (plates.doc.feed.failed_features().iter()).any(|failed| failed.feature == id),
        "the shell fails"
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// With regeneration shelling boxes: a closed hollow previewed, the box
/// as it was outside; its top removed, the floor 2 up inside, what's
/// left of the top lit as the face removed; another
/// thickness, outward (the box grown but at its top), too thick refused
/// by regeneration in the panel; OK adds it as one undo step.
#[test]
fn the_hollow_is_previewed_and_ok_adds_one_undo_step() {
    varde_regen::testing::shell_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    let before = plates.doc.editor.document().clone();
    plates.doc.look(Look::StartShell);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(body);
    assert!(near(low, DVec3::ZERO) && near(high, DVec3::new(40.0, 30.0, 10.0)));
    // Closed: the void's ceiling, 2 under the top, faces down into it.
    assert!(flat(&plates, body, -DVec3::Z, -8.0).is_some(), "closed");

    let pick = top(&plates, body);
    click(&mut plates, pick);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(flat(&plates, body, -DVec3::Z, -8.0).is_none(), "opened");
    assert!(flat(&plates, body, DVec3::Z, 2.0).is_some(), "the floor");
    // What's left of the top, the walls' ends, keeps its name: lit as
    // the face removed.
    let ring = flat(&plates, body, DVec3::Z, 10.0).expect("the walls' ends");
    assert_eq!(plates.doc.faces_lit(), [Picked::Face(ring)]);
    assert!(shows(&plates, "Face 1") && shows(&plates, "Planar face"));
    // The inner faces, made by the shell itself, aren't taken.
    let floor = face_pick(&plates, body, DVec3::Z, 2.0, DVec3::new(20.0, 15.0, 2.0));
    assert!(!plates.doc.takes_reference(floor));
    click(&mut plates, floor);
    assert_eq!(faces(&plates).len(), 1);
    assert!(
        flat(&plates, body, DVec3::Y, 2.0).is_some(),
        "the front wall"
    );
    // Another face of the box, on the preview, is.
    let pick = front(&plates, body);
    click(&mut plates, pick);
    assert_eq!(faces(&plates).len(), 2);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // The front wall's inside, 2 behind it, is gone with it.
    assert!(flat(&plates, body, DVec3::Y, 2.0).is_none());
    // Its row's cross takes it out.
    let front_face = faces(&plates)
        .into_iter()
        .find(|face| face.near.y == 0.0)
        .expect("the front");
    plates.motion(MotionLook::DropFace(front_face));
    assert_eq!(faces(&plates).len(), 1);

    plates.input(MotionField::Thickness, "3");
    let shell = drafted(&plates).expect("a draft");
    assert_eq!(shell.thickness.value, 3.0);
    plates.answer();
    assert!(flat(&plates, body, DVec3::Z, 3.0).is_some());
    plates.motion(MotionLook::ShellDirection(ShellDirection::Outward));
    assert!(drafted(&plates).unwrap().outward);
    assert!(shows(&plates, "1 face removed · 3 mm outward"));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    let [low, high] = plates.bounds(body);
    assert!(
        near(low, DVec3::splat(-3.0)) && near(high, DVec3::new(43.0, 33.0, 10.0)),
        "{low} {high}"
    );
    plates.motion(MotionLook::ShellDirection(ShellDirection::Inward));
    // Too thick: the walls would meet.
    plates.input(MotionField::Thickness, "20");
    plates.answer();
    let error = plates.doc.feed.draft_error().expect("too thick");
    assert!(error.contains("too thick"), "{error}");
    assert!(shows(&plates, "Shell fails"));
    plates.input(MotionField::Thickness, "0");
    assert!(!plates.doc.motion_ready(), "a thickness of nothing");
    plates.input(MotionField::Thickness, "2");
    plates.answer();
    assert!(plates.doc.motion_ready());

    let shell = (plates.doc.motion.as_ref()).and_then(|session| session.shell());
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, kind) = plates.last_feature();
    assert_eq!(Some(kind), shell.map(FeatureKind::Shell));
    assert_eq!(plates.doc.selected_feature, Some(id));
    plates.answer();
    plates
        .doc
        .look(Look::SelectPanel(varde_view::Panel::Timeline));
    assert!(shows(&plates, "Shell 1") && shows(&plates, "1 face removed · 2 mm inward"));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// A shell's faces are all on one body: once one is picked, another
/// body's faces neither light nor are taken, nor is a body's row; with
/// none, a body's row picks the body for a closed one, and a face of
/// another body moves it there. The Remove field clicked stops picking,
/// and again starts.
#[test]
fn a_shell_s_faces_are_all_on_one_body() {
    let mut plates = boxes(true);
    let [first, second, _] = plates.bodies;
    plates.doc.look(Look::StartShell);
    let session = plates.doc.motion.as_ref().expect("a session");
    assert!(session.bodies.is_empty(), "two bodies: none to begin with");
    assert!(shows(
        &plates,
        "pick faces to remove, or the body to hollow"
    ));
    assert!(drafted(&plates).is_none());
    assert!(!plates.doc.motion_ready());
    // A body's row picks it: a closed shell of it.
    plates.doc.look(Look::ClickBody {
        body: second,
        add: false,
    });
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [second]);
    assert_eq!(drafted(&plates).unwrap().body, second);
    // A face of the other body moves it there.
    let pick = top(&plates, first);
    click(&mut plates, pick);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [first]);
    assert_eq!(faces(&plates).len(), 1);
    let theirs = face_pick(
        &plates,
        second,
        DVec3::Z,
        10.0,
        DVec3::new(70.0, 10.0, 10.0),
    );
    assert!(!plates.doc.takes_reference(theirs), "not lit");
    click(&mut plates, theirs);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("A shell's faces are all on one body: pick faces of Body 1")
    );
    assert_eq!(faces(&plates).len(), 1);
    plates.doc.look(Look::ClickBody {
        body: second,
        add: false,
    });
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [first]);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("A shell's faces are all on one body: take them out to pick another")
    );
    // Its face out, the body stays: a closed shell of it.
    let face = faces(&plates)[0];
    plates.motion(MotionLook::DropFace(face));
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [first]);
    assert!(drafted(&plates).unwrap().open.is_empty());
    assert!(plates.doc.takes_reference(theirs));
    click(&mut plates, theirs);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [second]);

    plates.motion(MotionLook::Picking(MotionPick::Faces));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    plates.motion(MotionLook::Picking(MotionPick::Edges));
    assert_eq!(picking(&plates), MotionPick::Nothing);
    plates.motion(MotionLook::Picking(MotionPick::Faces));
    assert_eq!(picking(&plates), MotionPick::Faces);
}

/// The box with "Shell 1" opening its top, 2 mm inward: the editor and
/// the shell.
fn shelled() -> (Editor, FeatureId) {
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    plates.doc.look(Look::StartShell);
    let pick = top(&plates, body);
    click(&mut plates, pick);
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    let (id, _) = plates.last_feature();
    (Editor::new(plates.doc.editor.document().clone()), id)
}

/// Editing a shell from the Timeline opens it with its faces and values,
/// its preview the shell; Cancel leaves no trace; another thickness and
/// outward, OK sets it as one undo step; its faces all taken out, it's a
/// closed shell of its body.
#[test]
fn editing_a_shell_from_the_timeline_cancel_and_undo() {
    let (editor, id) = shelled();
    let mut plates = held(editor.document().clone());
    let body = plates.bodies[0];
    let before = plates.doc.editor.document().clone();
    let FeatureKind::Shell(stored) = before.feature(id).unwrap().kind.clone() else {
        panic!("a shell");
    };
    assert_eq!(stored.open.len(), 1);
    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert_eq!(session.kind, MotionKind::Shell);
    assert_eq!(session.feature, Some(id));
    assert_eq!(session.faces.refs, stored.open);
    assert_eq!(session.bodies, [body]);
    assert_eq!(picking(&plates), MotionPick::Faces);
    assert!(shows(&plates, "Shell 1") && shows(&plates, "Face 1"));
    assert_eq!(session.shell(), Some(stored.clone()));
    let (feature, kind) = plates.last_draft().expect("a draft");
    assert_eq!(
        (feature, kind),
        (Some(id), FeatureKind::Shell(stored.clone()))
    );
    plates.input(MotionField::Thickness, "1");
    plates.motion(MotionLook::Cancel);
    assert!(plates.doc.motion.is_none());
    assert_eq!(*plates.doc.editor.document(), before);

    plates.doc.look(Look::EditFeature(id));
    plates.input(MotionField::Thickness, "1");
    plates.motion(MotionLook::ShellDirection(ShellDirection::Outward));
    // OK waits on the stand-in's failure; Add anyway sets it.
    plates.answer();
    plates.doc.update(Edit::AcceptError);
    assert!(plates.doc.motion.is_none());
    let FeatureKind::Shell(set) = &plates.doc.editor.document().feature(id).unwrap().kind else {
        panic!("a shell");
    };
    assert!(set.thickness.value == 1.0 && set.outward && set.open == stored.open);
    assert_eq!(
        plates.doc.editor.document().features().len(),
        before.features().len()
    );
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);

    // Its faces taken out: a closed shell of its body.
    plates.doc.look(Look::EditFeature(id));
    plates.motion(MotionLook::DropFace(stored.open[0]));
    let (feature, kind) = plates.last_draft().expect("a draft");
    assert_eq!(feature, Some(id));
    let FeatureKind::Shell(closed) = kind else {
        panic!("a closed shell: {kind:?}");
    };
    assert!(closed.open.is_empty() && closed.body == body);
    assert!(plates.doc.motion_ready());
}

/// A face whose maker an undo takes away is kept, said to be gone,
/// nothing previewed or committed, until a redo brings it back; taken
/// out, the shell goes on closed.
#[test]
fn a_face_an_undo_takes_away_is_said_to_be_gone() {
    let (mut plates, plate) = plate();
    // A boss joined to the plate, 15 up about (20, 0).
    add_join(&mut plates.doc.editor);
    plates.doc.sync();
    plates.answer();
    plates.doc.look(Look::StartShell);
    let boss = face_pick(&plates, plate, DVec3::Z, 15.0, DVec3::new(20.0, 0.0, 15.0));
    click(&mut plates, boss);
    assert_eq!(faces(&plates).len(), 1);
    assert!(plates.doc.motion_ready());
    plates.doc.update(Edit::Undo);
    assert!(plates.doc.editor.document().body(plate).is_some());
    assert_eq!(faces(&plates).len(), 1, "kept");
    assert!(!plates.doc.motion_ready());
    assert!(shows(&plates, "A picked face is gone"));
    assert!(drafted(&plates).is_none());
    plates.doc.update(Edit::Redo);
    assert!(plates.doc.motion_ready());
    assert!(drafted(&plates).is_some());
    plates.doc.update(Edit::Undo);
    let face = faces(&plates)[0];
    plates.motion(MotionLook::DropFace(face));
    assert!(plates.doc.motion_ready());
    assert!(drafted(&plates).unwrap().open.is_empty());
}

/// It starts with the face selected that it takes; Shell again backs
/// out.
#[test]
fn shell_takes_the_face_selected_and_backs_out() {
    let mut plates = boxes(true);
    let second = plates.bodies[1];
    plates.doc.pick.selection = Selection::new(SelectionMode::Faces);
    let pick = face_pick(
        &plates,
        second,
        DVec3::Z,
        10.0,
        DVec3::new(70.0, 10.0, 10.0),
    );
    plates.doc.look(Look::ClickModel {
        pick: Some(pick),
        add: true,
        double: false,
    });
    plates.doc.look(Look::StartShell);
    assert_eq!(faces(&plates).len(), 1);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [second]);
    assert!(drafted(&plates).is_some());
    plates.doc.look(Look::StartShell);
    assert!(plates.doc.motion.is_none());
}

/// Faces selected on two bodies: the shell takes those on the first
/// one's body, leaving the other's out.
#[test]
fn faces_selected_on_two_bodies_give_the_first_one_s_body() {
    let mut plates = boxes(true);
    let [first, second, _] = plates.bodies;
    plates.doc.pick.selection = Selection::new(SelectionMode::Faces);
    let theirs = face_pick(
        &plates,
        second,
        DVec3::Z,
        10.0,
        DVec3::new(70.0, 10.0, 10.0),
    );
    for pick in [theirs, top(&plates, first), front(&plates, second)] {
        plates.doc.look(Look::ClickModel {
            pick: Some(pick),
            add: true,
            double: false,
        });
    }
    plates.doc.look(Look::StartShell);
    let picked = faces(&plates);
    assert_eq!(picked.len(), 2);
    assert!(picked.iter().all(|face| face.body == second));
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [second]);
    assert_eq!(plates.doc.faces_lit().len(), 2);
}

/// A face picked on a body that a combine, brought back by a redo,
/// merges into another follows its body on to the holder, as does the
/// body picked for a closed shell; the faces stay lit there.
#[test]
fn faces_follow_their_body_a_redone_combine_merges() {
    let mut plates = boxes(true);
    let [first, second, _] = plates.bodies;
    let combine = varde_document::Combine {
        target: first,
        tools: vec![second],
        op: varde_document::BodyOp::Union,
        keep_tools: false,
    };
    let add = plates.doc.editor.document().add_feature(combine.into());
    plates.doc.apply(add);
    plates.doc.sync();
    plates.answer();
    plates.doc.update(Edit::Undo);
    plates.answer();

    plates.doc.look(Look::StartShell);
    plates.doc.look(Look::ClickBody {
        body: second,
        add: false,
    });
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [second]);
    plates.doc.update(Edit::Redo);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [first]);
    assert_eq!(drafted(&plates).expect("a closed shell").body, first);
    plates.answer();
    plates.doc.update(Edit::Undo);
    plates.answer();

    let theirs = face_pick(
        &plates,
        second,
        DVec3::Z,
        10.0,
        DVec3::new(70.0, 10.0, 10.0),
    );
    click(&mut plates, theirs);
    assert!(faces(&plates).iter().all(|face| face.body == second));
    plates.doc.update(Edit::Redo);
    let session = plates.doc.motion.as_ref().expect("still set up");
    assert_eq!(session.bodies, [first], "the box followed on to the other");
    assert!(faces(&plates).iter().all(|face| face.body == first));
    assert!(plates.doc.motion_ready());
    assert_eq!(drafted(&plates).expect("a draft").open, faces(&plates));
    plates.answer();
    assert_eq!(plates.doc.faces_lit().len(), 1, "lit on the holder");
}

/// At 1280 px wide the toolbar in a shell session shows whole: Cancel
/// and the origins, as a chamfer's.
#[test]
fn a_shell_s_toolbar_fits_at_1280_px() {
    use crate::tests::{shown, texts};
    use varde_view::Mode;
    let (mut plates, _) = plate();
    plates.doc.look(Look::StartShell);
    let mut renderer = varde_view::probe::renderer();
    let size = iced::Size::new(1280.0, 800.0);
    let mut ui = shown(plates.doc.view_in(Mode::Light), size, &mut renderer);
    let mut on: Vec<_> = (texts(&mut ui, &renderer).into_iter())
        .filter(|t| t.bounds.y < 40.0)
        .collect();
    on.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
    let mut end = 0.0;
    for t in &on {
        assert!(t.bounds.width > 2.0, "{} squeezed: {on:?}", t.text);
        assert!(t.bounds.x >= end, "{} overlaps: {on:?}", t.text);
        end = t.bounds.x + t.bounds.width;
    }
    assert!(end < 1280.0 - 120.0, "{on:?}");
    assert!(on.iter().any(|t| t.text == "New shell"));
    assert!(on.iter().any(|t| t.text == "Cancel"));
}

/// The box's top opened with its left and right: what's left of the top
/// on the preview is two strips, front and back, both lit as the top;
/// a click on either takes the top out, and again puts it back once.
#[test]
fn a_removed_face_left_in_pieces_is_lit_and_taken_out_from_any() {
    varde_regen::testing::shell_by_boxes();
    let mut plates = boxes(false);
    let body = plates.bodies[0];
    plates.doc.look(Look::StartShell);
    let up = top(&plates, body);
    let picks = [
        up,
        face_pick(&plates, body, -DVec3::X, 0.0, DVec3::new(0.0, 15.0, 5.0)),
        face_pick(&plates, body, DVec3::X, 40.0, DVec3::new(40.0, 15.0, 5.0)),
    ];
    for pick in picks {
        click(&mut plates, pick);
    }
    let key = (faces(&plates).into_iter())
        .find(|face| face.near.z == 10.0)
        .expect("the top")
        .key;
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    for y in [1.0, 29.0, 1.0, 29.0] {
        let index = plates.doc.feed.pick_index();
        let at = DVec3::new(20.0, y, 10.0);
        let strip = index.find_face(body, &key, at).expect("a strip");
        let other = index
            .find_face(body, &key, DVec3::new(20.0, 30.0 - y, 10.0))
            .expect("a strip");
        assert_ne!(strip, other, "the top in two strips");
        let lit = plates.doc.faces_lit();
        assert!(lit.contains(&Picked::Face(strip)), "{lit:?}");
        assert!(lit.contains(&Picked::Face(other)), "{lit:?}");
        let pick = Pick {
            model: index.model(),
            target: Picked::Face(strip),
            body,
            at,
            snap: None,
        };
        assert!(plates.doc.takes_reference(pick));
        click(&mut plates, pick);
        assert_eq!(faces(&plates).len(), 2, "the top out");
        assert!(plates.doc.takes_reference(pick));
        click(&mut plates, pick);
        assert_eq!(faces(&plates).len(), 3, "the top back");
        plates.answer();
    }
}

/// A shell at the limit of faces (one face named at 256 points, as a
/// file may hold): edited, another face clicked is refused with why,
/// the panel lists them all; one taken out, the other is taken.
#[test]
fn a_shell_takes_at_most_256_faces() {
    use varde_document::MAX_SHELL_FACES;
    let (mut editor, id) = shelled();
    let FeatureKind::Shell(mut shell) = editor.document().feature(id).unwrap().kind.clone() else {
        panic!("a shell");
    };
    let up = shell.open[0];
    shell.open = (0..MAX_SHELL_FACES)
        .map(|k| varde_document::FaceRef {
            near: DVec3::new(0.5 + k as f64 * 0.15, 15.0, 10.0),
            ..up
        })
        .collect();
    shell.open.sort_by(varde_document::FaceRef::order);
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(shell.clone().into()),
        })
        .unwrap();
    let mut plates = held(editor.document().clone());
    let body = plates.bodies[0];
    plates.doc.look(Look::EditFeature(id));
    plates.answer();
    assert_eq!(faces(&plates).len(), MAX_SHELL_FACES);
    assert!(shows(&plates, "Face 256"));
    let pick = front(&plates, body);
    click(&mut plates, pick);
    assert_eq!(
        plates.doc.notice.as_deref(),
        Some("A shell takes at most 256 faces")
    );
    assert_eq!(faces(&plates).len(), MAX_SHELL_FACES);
    plates.motion(MotionLook::DropFace(shell.open[7]));
    click(&mut plates, pick);
    assert_eq!(faces(&plates).len(), MAX_SHELL_FACES);
    assert!(plates.doc.motion_ready());
    let mut over = shell.clone();
    over.open.push(varde_document::FaceRef {
        near: DVec3::new(39.9, 29.9, 10.0),
        ..up
    });
    let design = plates.doc.editor.document().design();
    assert_eq!(
        over.check_own(&design).unwrap_err().to_string(),
        "opens 257 faces, more than 256"
    );
}

pub(super) mod fuzz;
