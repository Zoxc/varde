//! The list of the model's overlaps in the sessions picking the model
//! for themselves: each row ticked as the session has its item (what a
//! click on it would leave or take out), never as the model's selection,
//! which the session doesn't change.

use glam::DVec3;
use varde_regen::Summary;
use varde_view::{Look, MotionLook, MotionPick, OverlapItems, Overlaps, Pick, Picked};

use super::{Plates, plates};

/// The top face of `body` in the model shown, as the cursor picks it.
fn top(plates: &Plates, body: varde_document::BodyId) -> Pick {
    let face = plates.face(
        body,
        |summary| matches!(summary, Summary::Plane { n, .. } if n[2] > 0.5),
    );
    pick(plates, body, Picked::Face(face))
}

/// The round wall of `body` in the model shown, as the cursor picks it.
fn wall(plates: &Plates, body: varde_document::BodyId) -> Pick {
    let face = plates.face(body, |summary| matches!(summary, Summary::Cylinder { .. }));
    pick(plates, body, Picked::Face(face))
}

fn pick(plates: &Plates, body: varde_document::BodyId, target: Picked) -> Pick {
    Pick {
        model: plates.doc.feed.pick_index().model(),
        target,
        body,
        at: DVec3::ZERO,
        snap: None,
    }
}

/// Opens the list of `picks` afresh.
fn list(plates: &mut Plates, picks: Vec<Pick>) {
    plates.doc.look(Look::CloseOverlaps);
    plates.doc.look(Look::OpenOverlaps(Overlaps {
        held: glam::DVec2::ZERO,
        at: glam::DVec2::ZERO,
        items: OverlapItems::Model(picks),
    }));
    assert!(plates.doc.overlaps.is_some());
}

/// The plate's top selected before a move: in the move, a row is ticked
/// as the move has its body while bodies are picked, and as the axis
/// while the axis is; the selection ticks nothing.
#[test]
fn a_move_ticks_its_bodies_and_its_axis_not_the_selection() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    let selected = top(&plates, plate);
    plates.doc.look(Look::ClickModel {
        pick: Some(selected),
        add: false,
        double: false,
    });
    assert!(!plates.doc.pick.selection.is_empty());
    plates.doc.look(Look::StartMove);
    let session = plates.doc.motion.as_ref().expect("a move");
    assert_eq!(session.bodies, [plate], "the selection's body");
    plates.click(plate);
    plates.click(right);
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [right]);
    let picks = vec![top(&plates, plate), wall(&plates, right)];
    list(&mut plates, picks.clone());
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![false, true]));
    // Chosen with its tick, the plate's top picks the plate.
    plates.doc.look(Look::ToggleOverlap(0));
    assert_eq!(plates.doc.motion.as_ref().unwrap().bodies, [plate, right]);
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, true]));

    // The axis picked: nothing's ticked till the disc's wall is it.
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    plates.answer();
    let picks = vec![top(&plates, plate), wall(&plates, right)];
    list(&mut plates, picks.clone());
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![false, false]));
    plates.doc.look(Look::CloseOverlaps);
    let disc = wall(&plates, right);
    plates.click_at(right, disc.target, DVec3::new(25.0, 0.0, 0.0));
    assert_eq!(plates.doc.notice, None);
    plates.motion(MotionLook::Picking(MotionPick::Reference));
    list(&mut plates, picks);
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![false, true]));
}

/// A combine ticks its target, and its tools while they're picked.
#[test]
fn a_combine_ticks_its_target_and_tools() {
    let mut plates = plates();
    let [plate, right, left] = plates.bodies;
    plates.doc.look(Look::StartCombine);
    plates.click(plate);
    plates.click(right);
    let session = plates.doc.combine.as_ref().expect("a combine");
    assert_eq!(
        (session.target, &session.tools[..]),
        (Some(plate), &[right][..])
    );
    let picks = vec![
        top(&plates, plate),
        wall(&plates, right),
        wall(&plates, left),
    ];
    list(&mut plates, picks);
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, true, false]));
    plates.doc.look(Look::ToggleOverlap(2));
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, true, true]));
}

/// The measure tool ticks what it has picked, A or B, once measured.
#[test]
fn the_measure_tool_ticks_its_picks() {
    let mut plates = plates();
    let [plate, right, _] = plates.bodies;
    plates.doc.look(Look::StartMeasure);
    let picked = top(&plates, plate);
    plates.doc.look(Look::ClickModel {
        pick: Some(picked),
        add: false,
        double: false,
    });
    plates.answer();
    let picks = vec![top(&plates, plate), wall(&plates, right)];
    list(&mut plates, picks);
    assert_eq!(plates.doc.overlap_ticked(), Some(vec![true, false]));
}
