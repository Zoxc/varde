//! Join to original: the tick, the mock's overlap warning, a body per
//! copy committed and listed in Objects, editing the tick, and an edit
//! held back by a later feature naming a copy body.

use varde_document::{BodyOp, Combine, Copies};

use super::*;

/// The copy bodies `pattern` lists.
fn copy_bodies(pattern: &Pattern) -> Vec<varde_document::BodyId> {
    pattern.copy_bodies().map(|(_, _, body)| body).collect()
}

const OVERLAP: &str = "tick Join to original to merge them";

#[test]
fn unticked_a_pattern_makes_a_body_per_copy_and_warns_of_overlaps() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    let before = plates.doc.editor.document().clone();
    plates.click(right);
    key_in(&mut plates.doc, character("p"));
    // Ticked to begin with: the copies join the original.
    assert!(shows(&plates, "Join to original"));
    assert!(plates.doc.motion.as_ref().unwrap().join);
    plates.input(MotionField::Count, "3");
    plates.input(MotionField::Spread, "12");
    assert!(drafted(&plates).unwrap().joins());
    assert!(shows(&plates, "Body 2 · 3 × 12 mm along X axis · joined"));
    plates.answer();
    // Unticked, each copy is a body of its own; 12 apart, the discs (10
    // wide) don't overlap.
    plates.motion(MotionLook::Join);
    let pattern = drafted(&plates).expect("a draft");
    assert_eq!(pattern.copies, Copies::Separate(Vec::new()));
    assert!(shows(&plates, "Body 2 · 3 × 12 mm along X axis"));
    assert!(!shows(&plates, "· joined"));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(!shows(&plates, OVERLAP));
    // 8 apart they do: the mock's warning, which doesn't stop OK.
    plates.input(MotionField::Spread, "8");
    plates.answer();
    assert!(
        shows(&plates, "The copies overlap (10 mm long this way)"),
        "{:?}",
        screen_texts(&plates.doc)
    );
    assert!(shows(&plates, OVERLAP));
    assert!(plates.doc.motion_ready());
    // Ticked again, no warning: the copies are united.
    plates.motion(MotionLook::Join);
    assert!(!shows(&plates, OVERLAP));
    plates.motion(MotionLook::Join);
    key_in(&mut plates.doc, enter());
    assert!(plates.doc.motion.is_none());
    let (id, _) = plates.last_feature();
    let stored = last_pattern(&plates);
    let made = copy_bodies(&stored);
    assert_eq!(made.len(), 2);
    let document = plates.doc.editor.document();
    for (&body, name) in made.iter().zip(["Body 4", "Body 5"]) {
        let body = document.body(body).unwrap();
        assert_eq!((body.name.as_str(), body.created_by), (name, id));
    }
    // Objects lists them; the model shows each, 8 apart.
    plates.answer();
    assert!(shows(&plates, "Body 4") && shows(&plates, "Body 5"));
    let [low, _] = plates.bounds(right);
    let [copy_low, _] = plates.bounds(made[1]);
    assert!((copy_low.x - low.x - 16.0).abs() < 1e-3, "{low} {copy_low}");
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
}

/// Editing an unjoined pattern opens unticked; ticking it joins the
/// copies, dropping their bodies, one undo step. A later combine using a
/// copy body holds that back, the panel saying why.
#[test]
fn editing_the_tick_and_a_copy_body_a_later_feature_uses() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "3");
    plates.input(MotionField::Spread, "40");
    plates.motion(MotionLook::Join);
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    let made = copy_bodies(&last_pattern(&plates));
    plates.answer();
    let unjoined = plates.doc.editor.document().clone();

    plates.doc.look(Look::EditFeature(id));
    let session = plates.doc.motion.as_ref().expect("a session");
    assert!(!session.join);
    plates.motion(MotionLook::Join);
    assert!(drafted(&plates).unwrap().joins());
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_none());
    assert!(last_pattern(&plates).joins());
    for body in &made {
        assert!(plates.doc.editor.document().body(*body).is_none());
    }
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), unjoined);

    // The plate combined with the second copy's body.
    let combine = Combine {
        target: plate,
        tools: vec![made[1]],
        op: BodyOp::Union,
        keep_tools: true,
    };
    let add = plates.doc.editor.document().add_feature(combine.into());
    plates.doc.editor.apply(add).unwrap();
    plates.doc.sync();
    plates.answer();
    plates.doc.look(Look::EditFeature(id));
    // Fewer copies, or joined, would drop it.
    plates.input(MotionField::Count, "2");
    let name = &plates
        .doc
        .editor
        .document()
        .body(made[1])
        .unwrap()
        .name
        .clone();
    let held = format!(
        "Combine 1 uses {name}, a copy this pattern would no longer make: take {name} out of Combine 1 or delete it first"
    );
    assert_eq!(plates.doc.motion_held(), Some(held.clone()));
    assert!(!plates.doc.motion_ready());
    plates.input(MotionField::Count, "4");
    assert_eq!(plates.doc.motion_held(), None);
    assert!(plates.doc.motion_ready());
    plates.motion(MotionLook::Join);
    assert_eq!(plates.doc.motion_held(), Some(held));
    assert!(!plates.doc.motion_ready());
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_some(), "not committed");
}

/// Picking another direction for an unjoined pattern whose copy body a
/// later feature uses: the preview leaving the bodies where they are (a
/// move by nothing) would drop the copy bodies, which the document
/// refuses, so there's no draft, and no failure shown, the model as
/// committed.
#[test]
fn picking_the_direction_of_a_pattern_whose_copy_a_later_feature_uses() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "3");
    plates.input(MotionField::Spread, "40");
    plates.motion(MotionLook::Join);
    plates.doc.update(Edit::CommitMotion);
    let (id, _) = plates.last_feature();
    let made = copy_bodies(&last_pattern(&plates));
    let combine = Combine {
        target: plate,
        tools: vec![made[1]],
        op: BodyOp::Union,
        keep_tools: true,
    };
    let add = plates.doc.editor.document().add_feature(combine.into());
    plates.doc.editor.apply(add).unwrap();
    plates.doc.sync();
    plates.answer();
    plates.doc.look(Look::EditFeature(id));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    assert_eq!(plates.doc.motion_draft(), None);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(
        !shows(&plates, "Pattern fails"),
        "{:?}",
        screen_texts(&plates.doc)
    );
    // The copy bodies are still there to see.
    let [low, _] = plates.bounds(made[1]);
    let [right_low, _] = plates.bounds(right);
    assert!(
        (low.x - right_low.x - 80.0).abs() < 1e-3,
        "{low} {right_low}"
    );
}

/// Unticked, a pattern making more bodies than a document may have of
/// one is refused in the panel, OK waiting; ticked, it's fine.
#[test]
fn too_many_copy_bodies_are_refused_in_the_panel() {
    let mut plates = plates();
    let [_, right, left] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.click(left);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies.len(), 2);
    plates.input(MotionField::Count, "1024");
    plates.input(MotionField::Spread, "0.1");
    assert!(plates.doc.motion_ready());
    plates.motion(MotionLook::Join);
    assert!(
        shows(&plates, "2046 bodies"),
        "{:?}",
        screen_texts(&plates.doc)
    );
    assert!(!plates.doc.motion_ready());
}
