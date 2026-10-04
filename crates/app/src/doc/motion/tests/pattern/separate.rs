//! Join to original: the tick, the mock's overlap warning, a body per
//! copy committed and listed in Objects, editing the tick, and an edit
//! held back by a later feature naming a copy body.

use varde_document::{BodyOp, Combine, Copies};
use varde_view::Picked;

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

/// The mock's overlap warning, for each copy against the copies of its
/// own body: two discs 40 apart, each 10 wide, copies 12 apart, don't
/// overlap, though the two together are 50 long.
#[test]
fn bodies_apart_whose_copies_miss_get_no_overlap_warning() {
    let mut plates = plates();
    let [_, right, left] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.click(left);
    plates.input(MotionField::Count, "3");
    plates.input(MotionField::Spread, "12");
    plates.motion(MotionLook::Join);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    assert!(!shows(&plates, OVERLAP), "{:?}", screen_texts(&plates.doc));
    plates.input(MotionField::Spread, "8");
    plates.answer();
    assert!(
        shows(&plates, "The copies overlap (10 mm long this way)"),
        "{:?}",
        screen_texts(&plates.doc)
    );
}

/// In inches: the length said in inches, the spacing compared in the
/// model's millimetres; flipped the same.
#[test]
fn the_overlap_warning_in_inches_and_flipped() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates
        .doc
        .update(Edit::SetUnits(varde_expr::LengthUnit::In));
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "3");
    plates.motion(MotionLook::Join);
    // 0.5 in is 12.7 mm: apart.
    plates.input(MotionField::Spread, "0.5");
    plates.answer();
    assert!(!shows(&plates, OVERLAP), "{:?}", screen_texts(&plates.doc));
    // 0.3 in is 7.62 mm: they overlap; 10 mm is 0.394 in.
    plates.input(MotionField::Spread, "0.3");
    plates.answer();
    assert!(
        shows(&plates, "The copies overlap (0.3937 in long this way)"),
        "{:?}",
        screen_texts(&plates.doc)
    );
    plates.motion(MotionLook::Flip);
    plates.answer();
    assert!(shows(&plates, OVERLAP), "{:?}", screen_texts(&plates.doc));
    plates.input(MotionField::Spread, "0.5");
    plates.answer();
    assert!(!shows(&plates, OVERLAP), "{:?}", screen_texts(&plates.doc));
}

/// Along an edge of the plate (60 long in X, 40 in Y): copies 50 apart
/// overlap along X but not along Y, flipped or not.
#[test]
fn the_overlap_warning_along_an_edge() {
    let mut plates = plates();
    let [plate, _, _] = plates.bodies;
    plates.click(plate);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "2");
    plates.input(MotionField::Spread, "50");
    plates.motion(MotionLook::Join);
    for (wanted, overlaps) in [(DVec3::X, true), (DVec3::Y, false)] {
        plates.motion(MotionLook::Picking(MotionPick::Reference));
        plates.answer();
        let index = plates.doc.feed.pick_index();
        let (edge, ends) = (0..index.mesh().edge_count() as u32)
            .filter(|&edge| index.body(Picked::Edge(edge)) == Some(plate))
            .find_map(|edge| {
                let keys = index.chain_keys(edge)?;
                let ends = index.edge_ends(edge, &keys)?;
                let along = (ends[1] - ends[0]).normalize();
                (along.dot(wanted).abs() > 1.0 - 1e-9).then_some((edge, ends))
            })
            .expect("a straight edge of the plate");
        plates.click_at(plate, Picked::Edge(edge), (ends[0] + ends[1]) / 2.0);
        assert!(matches!(
            plates.doc.motion.as_ref().unwrap().axis,
            Some(AxisRef::Edge(_))
        ));
        for _ in 0..2 {
            plates.answer();
            assert_eq!(plates.doc.feed.draft_error(), None);
            assert_eq!(
                shows(&plates, OVERLAP),
                overlaps,
                "{wanted}: {:?}",
                screen_texts(&plates.doc)
            );
            plates.motion(MotionLook::Flip);
        }
    }
}

/// Deleting one copy body takes its pattern and the other copy bodies
/// with it, so the user is asked first, the prompt listing them; deleting
/// the pattern itself takes only its own bodies, without asking.
#[test]
fn deleting_a_copy_body_asks_first() {
    let mut plates = plates();
    let [_, right, _] = plates.bodies;
    plates.click(right);
    plates.doc.look(Look::StartPattern);
    plates.input(MotionField::Count, "3");
    plates.input(MotionField::Spread, "40");
    plates.motion(MotionLook::Join);
    plates.doc.update(Edit::CommitMotion);
    plates.answer();
    let (id, _) = plates.last_feature();
    let made = copy_bodies(&last_pattern(&plates));
    let before = plates.doc.editor.document().clone();
    plates.doc.update(Edit::RemoveBody(made[0]));
    assert_eq!(*plates.doc.editor.document(), before, "asked first");
    let prompt = plates.doc.delete_prompt().expect("the prompt shows");
    assert_eq!(prompt.name, "Body 4");
    let features: Vec<&str> = prompt.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(features, ["Pattern 1"]);
    let bodies: Vec<&str> = prompt.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(bodies, ["Body 4", "Body 5"]);
    plates.doc.update(Edit::ConfirmDelete);
    let document = plates.doc.editor.document();
    assert!(document.feature(id).is_none());
    assert!(made.iter().all(|&body| document.body(body).is_none()));
    plates.doc.update(Edit::Undo);
    assert_eq!(*plates.doc.editor.document(), before);
    // The pattern itself goes with its bodies at once.
    plates.doc.update(Edit::RemoveFeature(id));
    assert!(plates.doc.delete_prompt().is_none());
    assert!(plates.doc.editor.document().feature(id).is_none());
}

/// Held back by a later combine naming a copy body, an unjoined
/// pattern's axis is picked again, an edge of the plate: taken, nothing
/// previewed nor failing; a count that keeps the copy lets it through,
/// previewed along the edge, and committed the copy bodies are those
/// the combine names.
#[test]
fn a_held_pattern_s_axis_picked_again_commits_once_let_through() {
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
    plates.input(MotionField::Count, "2");
    assert!(plates.doc.motion_held().is_some());
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();
    let index = plates.doc.feed.pick_index();
    let (edge, ends) = (0..index.mesh().edge_count() as u32)
        .filter(|&edge| index.body(Picked::Edge(edge)) == Some(plate))
        .find_map(|edge| {
            let keys = index.chain_keys(edge)?;
            let ends = index.edge_ends(edge, &keys)?;
            let along = (ends[1] - ends[0]).normalize();
            (along.dot(DVec3::Y).abs() > 1.0 - 1e-9).then_some((edge, ends))
        })
        .expect("an edge of the plate along Y");
    plates.click_at(plate, Picked::Edge(edge), (ends[0] + ends[1]) / 2.0);
    let session = plates.doc.motion.as_ref().unwrap();
    assert!(
        matches!(session.axis, Some(AxisRef::Edge(_))),
        "{:?}",
        session.axis
    );
    assert!(plates.doc.motion_held().is_some());
    assert_eq!(plates.doc.motion_draft(), None);
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    // The panel says why, not a failure of the model.
    assert!(shows(&plates, "Combine 1 uses Body 5"));
    assert!(!plates.doc.motion_ready());
    // Three again: let through, along the edge.
    plates.input(MotionField::Count, "3");
    assert_eq!(plates.doc.motion_held(), None);
    let drafted = drafted(&plates).expect("a draft");
    assert!(matches!(drafted.kind.axis(), AxisRef::Edge(_)));
    plates.answer();
    assert_eq!(plates.doc.feed.draft_error(), None);
    plates.doc.update(Edit::CommitMotion);
    assert!(plates.doc.motion.is_none());
    let stored = last_pattern_of(&plates, id);
    assert_eq!(copy_bodies(&stored), made);
    plates.answer();
    // Copy 2 is 80 along Y from the disc, which is in X where it was.
    let [low, _] = plates.bounds(right);
    let [copy_low, _] = plates.bounds(made[1]);
    assert!((copy_low.x - low.x).abs() < 1e-3, "{low} {copy_low}");
    assert!(
        ((copy_low.y - low.y).abs() - 80.0).abs() < 1e-3,
        "{low} {copy_low}"
    );
}

/// The pattern feature `id` of `plates`' document.
fn last_pattern_of(plates: &Plates, id: FeatureId) -> Pattern {
    match &plates.doc.editor.document().feature(id).unwrap().kind {
        FeatureKind::Pattern(pattern) => pattern.clone(),
        other => panic!("not a pattern: {other:?}"),
    }
}
