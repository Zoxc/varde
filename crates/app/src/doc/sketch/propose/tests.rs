use std::time::Duration;

use glam::DVec2;
use iced::keyboard;
use varde_document::{Document, Editor, OriginPlane, Plane};
use varde_sketch::{Constraint, Curve};
use varde_view::{ConstraintKind, Edit, Look, Tool};

use super::*;
use crate::doc::sketch::tests::{
    Answered, at, click_at, lines, position, selection, shown, sketch, sketching, undo_to,
    with_shapes,
};
use crate::doc::{Origin, Target};
use crate::tests::{SolveLane, answer};

/// Clicks the tool in use at `x`, `y` on nothing, leaving the proposal it
/// makes unanswered.
fn click_waiting(doc: &mut Doc, x: f64, y: f64) {
    doc.update(Edit::ToolClick(click_at(x, y)));
}

/// The proposals with the lane, by the revisions they're on.
fn proposed(lane: &SolveLane) -> Vec<Revision> {
    lane.waiting()
        .iter()
        .filter_map(|request| match request {
            Request::Propose { base, .. } => Some(*base),
            _ => None,
        })
        .collect()
}

/// Selects `ids` alone.
fn select(doc: &mut Doc, ids: &[Id]) {
    doc.look(Look::SelectBox {
        ids: ids.to_vec(),
        add: false,
    });
}

#[test]
fn an_accepted_edit_is_committed_as_one_undo_step() {
    let (mut t, _, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    let before = t.editor.revision();
    click_waiting(&mut t.doc, 3.0, 4.0);

    // Shown, faded, but not committed until the solver says.
    assert_eq!(t.editor.revision(), before);
    assert!(t.proposing());
    assert_eq!(proposed(&t.lane), [before]);
    let state = t.sketch_state().unwrap();
    let [point] = &state.sketch.points[..] else {
        panic!("{:?}", state.sketch);
    };
    assert!(state.pending.contains(&point.id));
    assert!(sketch(&t).points.is_empty());

    t.lane.answer(&mut t.doc);
    assert!(!t.proposing());
    assert_eq!(sketch(&t).points.len(), 1);
    let state = t.sketch_state().unwrap();
    assert!(state.pending.is_empty());
    // The analysis comes with it: a lone point is free both ways.
    assert_eq!(state.analysis.map(|analysis| analysis.freedom), Some(2));
    t.update(Edit::Undo);
    assert_eq!(t.editor.revision(), before);
    assert!(sketch(&t).points.is_empty());
}

#[test]
fn a_refused_edit_changes_nothing_and_shows_what_it_ran_into() {
    let (mut t, [_, _, line, ..]) = with_shapes();
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    let horizontal = sketch(&t).constraints[0].id;
    let committed = sketch(&t).clone();
    let revision = t.editor.revision();

    // Again restates it.
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    assert_eq!(t.editor.revision(), revision);
    assert_eq!(*sketch(&t), committed);
    let state = t.sketch_state().unwrap();
    let Some(refused @ Rejected::Redundant { .. }) = state.refusal else {
        panic!("{:?}", state.refusal);
    };
    assert!(refused.involved().contains(&horizontal));

    // Until the next action.
    t.look(Look::ClearSelection);
    assert!(t.sketch_state().unwrap().refusal.is_none());
}

#[test]
fn edits_wait_in_order_each_proposed_on_the_last() {
    let (mut t, _, _) = sketching();
    t.look(Look::SelectTool(Tool::Line));
    click_waiting(&mut t.doc, 0.0, 0.0);
    click_waiting(&mut t.doc, 10.0, 0.0);
    // The next line goes on from the first's end, which waits too.
    click_waiting(&mut t.doc, 10.0, 10.0);
    let first = t.editor.revision();
    assert_eq!(proposed(&t.lane), [first], "one with the solver at a time");
    assert_eq!(lines(shown(&t)).len(), 2);
    assert!(sketch(&t).curves.is_empty());

    t.lane.answer_first(&mut t.doc);
    assert_eq!(lines(sketch(&t)).len(), 1);
    let second = t.editor.revision();
    assert_eq!(proposed(&t.lane), [second]);
    t.lane.answer_first(&mut t.doc);
    let drawn = lines(sketch(&t));
    let [(_, b), (c, _)] = drawn[..] else {
        panic!("{drawn:?}");
    };
    assert_eq!(b, c, "the chain shares its point");
    assert_eq!(undo_to(&mut t, &varde_document::Sketch::default()), 2);
}

#[test]
fn edits_after_a_refused_one_are_proposed_still() {
    let (mut t, [a, b, line, ..]) = with_shapes();
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    let revision = t.editor.revision();
    select(&mut t.doc, &[line]);
    t.doc.update(Edit::Constrain(ConstraintKind::Horizontal));
    select(&mut t.doc, &[a, b]);
    t.doc.update(Edit::Constrain(ConstraintKind::Coincident));
    assert_eq!(proposed(&t.lane), [revision]);
    t.lane.answer_first(&mut t.doc);
    assert!(t.sketch_state().unwrap().refusal.is_some());
    assert_eq!(proposed(&t.lane), [revision]);
    // Its ends coincident restate that they're level, which the solver
    // refuses too.
    t.lane.answer_first(&mut t.doc);
    assert_eq!(t.editor.revision(), revision);
    assert!(!t.proposing());
}

#[test]
fn undo_drops_the_newest_edit_waiting_and_its_answer() {
    let (mut t, _, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    let before = t.editor.revision();
    click_waiting(&mut t.doc, 1.0, 1.0);
    click_waiting(&mut t.doc, 2.0, 2.0);
    assert!(t.keys().is_some());
    t.doc.update(Edit::Undo);
    assert!(t.proposing());
    let [point] = &shown(&t).points[..] else {
        panic!("{:?}", shown(&t));
    };
    assert_eq!(point.at, at(1.0, 1.0));
    t.doc.update(Edit::Undo);
    assert!(!t.proposing());
    assert!(shown(&t).points.is_empty());
    assert_eq!(t.editor.revision(), before, "nothing else undone");

    // Proposed before the answer to the one dropped comes, which is
    // ignored.
    click_waiting(&mut t.doc, 3.0, 3.0);
    assert_eq!(proposed(&t.lane), [before, before]);
    t.lane.answer_first(&mut t.doc);
    assert!(sketch(&t).points.is_empty());
    assert!(t.proposing());
    t.lane.answer_first(&mut t.doc);
    let drawn = sketch(&t);
    assert_eq!(drawn.points.len(), 1);
    assert_eq!(drawn.points[0].at, at(3.0, 3.0));
}

#[test]
fn an_edit_answered_for_an_older_revision_is_proposed_again() {
    let (mut t, _, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    let before = t.editor.revision();
    click_waiting(&mut t.doc, 1.0, 1.0);
    // Committed meanwhile, not through the solver (what the user changes
    // waits behind it instead, but should anything).
    let feature = t.editor.document().features()[0].id;
    t.doc.apply(Command::SetFeatureVisible(feature, false));
    t.doc.sync();
    let after = t.editor.revision();
    t.lane.answer_first(&mut t.doc);
    assert!(sketch(&t).points.is_empty());
    assert_eq!(proposed(&t.lane), [after]);
    assert_ne!(before, after);
    t.lane.answer(&mut t.doc);
    assert_eq!(sketch(&t).points.len(), 1);
    assert!(!t.editor.document().features()[0].visible);
}

#[test]
fn waiting_long_enough_says_checking() {
    let (mut t, _, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    assert!(!t.timing());
    click_waiting(&mut t.doc, 1.0, 1.0);
    assert!(t.timing());
    let since = t.proposals.since.unwrap();
    t.tick(since + CHECKING / 2);
    assert!(!t.sketch_state().unwrap().checking);
    t.tick(since + CHECKING + Duration::from_millis(1));
    assert!(t.sketch_state().unwrap().checking);
    assert!(!t.timing(), "no more frames needed");
    t.lane.answer(&mut t.doc);
    assert!(!t.sketch_state().unwrap().checking);
}

#[test]
fn a_drag_shows_only_what_the_solver_converged_on() {
    let (mut t, [a, b, line, ..]) = with_shapes();
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    let before = sketch(&t).clone();

    t.doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(12.0, 3.0),
    });
    let waiting = t.lane.waiting();
    let [Request::Drag { session, .. }] = waiting[..] else {
        panic!("{waiting:?}");
    };
    // Nothing converged yet.
    assert_eq!(*shown(&t), before);
    t.lane.answer(&mut t.doc);
    // The line follows, staying horizontal, and nothing is committed. The
    // dragged point gives way a little: the drag weighs it a million times
    // what follows.
    let dragged = shown(&t).clone();
    assert!(
        position(&dragged, b).abs_diff_eq(at(12.0, 3.0), 1e-4),
        "{dragged:?}"
    );
    assert!((position(&dragged, a).y - position(&dragged, b).y).abs() < 1e-9);
    assert_eq!(*sketch(&t), before);

    t.update(Edit::DropGeometry);
    let dropped = sketch(&t).clone();
    assert!(position(&dropped, b).abs_diff_eq(at(12.0, 3.0), 1e-4));
    assert!((position(&dropped, a).y - position(&dropped, b).y).abs() < 1e-9);
    assert_eq!(undo_to(&mut t, &before), 1);

    // The next drag is a new session.
    t.doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(10.0, 5.0),
    });
    let waiting = t.lane.waiting();
    let [Request::Drag { session: next, .. }] = waiting[..] else {
        panic!("{waiting:?}");
    };
    assert!(next > session);
}

#[test]
fn a_drag_cancelled_or_cut_short_commits_nothing() {
    let (mut t, [_, b, ..]) = with_shapes();
    let before = sketch(&t).clone();
    let drag = |t: &mut crate::doc::sketch::tests::Answered| {
        t.look(Look::DragGeometry {
            id: b,
            from: at(10.0, 0.0),
            to: at(10.0, 4.0),
        });
    };
    drag(&mut t);
    assert_ne!(*shown(&t), before);
    t.look(Look::CancelDrag);
    assert_eq!(*shown(&t), before);
    t.update(Edit::DropGeometry);
    assert_eq!(*sketch(&t), before);
    assert!(proposed(&t.lane).is_empty());

    // Another edit ends it, as a tool taken up does.
    drag(&mut t);
    let feature = t.editor.document().features()[0].id;
    t.update(Edit::ToggleFeatureVisible(feature));
    assert!(t.sketch.as_ref().unwrap().drag.is_none());
    drag(&mut t);
    t.look(Look::SelectTool(Tool::Line));
    assert!(t.sketch.as_ref().unwrap().drag.is_none());
}

#[test]
fn the_constrain_tool_offers_what_fits_the_selection() {
    let (mut t, [a, _, line, _, circle]) = with_shapes();
    let k = || keyboard::Key::Character("k".into());
    t.key(k());
    assert!(t.sketch.as_ref().unwrap().constraining);
    let fitting = |t: &Doc| t.keys().unwrap().constraints;
    assert!(!fitting(&t).contains(ConstraintKind::Fix));

    select(&mut t, &[line]);
    let keys = fitting(&t);
    assert!(keys.contains(ConstraintKind::Horizontal) && keys.contains(ConstraintKind::Fix));
    assert!(!keys.contains(ConstraintKind::Tangent));
    // A kind that doesn't fit does nothing.
    t.update(Edit::Constrain(ConstraintKind::Tangent));
    assert!(sketch(&t).constraints.is_empty());

    select(&mut t, &[line, circle]);
    assert!(fitting(&t).contains(ConstraintKind::Tangent));
    t.update(Edit::Constrain(ConstraintKind::Tangent));
    let drawn = sketch(&t);
    assert!(matches!(
        drawn.constraints[..],
        [varde_sketch::ConstraintEntry {
            constraint: Constraint::Tangent { .. },
            ..
        }]
    ));
    // The tool clears the selection for the next.
    assert!(selection(&t).is_empty());

    // Outside the tool, the keys apply what fits and keep the selection.
    t.key(k());
    assert!(!t.sketch.as_ref().unwrap().constraining);
    select(&mut t, &[a]);
    t.update(Edit::Constrain(ConstraintKind::Fix));
    assert_eq!(selection(&t), [a]);
    assert!(
        sketch(&t)
            .constraints
            .iter()
            .any(|entry| entry.constraint == Constraint::Fix(a))
    );

    // Escape, or a drawing tool, puts the Constrain tool down.
    t.key(k());
    t.look(Look::Escape);
    assert!(!t.sketch.as_ref().unwrap().constraining);
    assert!(t.sketch.is_some());
    t.key(k());
    t.look(Look::SelectTool(Tool::Line));
    assert!(!t.sketch.as_ref().unwrap().constraining);
}

#[test]
fn the_analysis_says_what_is_fixed_and_how_free_the_rest_is() {
    let (mut t, [a, b, ..]) = with_shapes();
    // Asked on entering: three points and a radius.
    let freedom = |t: &Doc| t.sketch_state().unwrap().analysis.map(|a| a.freedom);
    assert_eq!(freedom(&t), Some(7));
    select(&mut t, &[a]);
    t.update(Edit::Constrain(ConstraintKind::Fix));
    let state = t.sketch_state().unwrap();
    let analysis = state.analysis.unwrap();
    assert_eq!(analysis.freedom, 5);
    assert!(analysis.fixed.contains(&a) && !analysis.fixed.contains(&b));
    // Undone, it's known again without asking.
    t.doc.update(Edit::Undo);
    assert_eq!(freedom(&t), Some(7));
    assert!(
        !t.lane
            .waiting()
            .iter()
            .any(|request| matches!(request, Request::Analyse { .. }))
    );
}

#[test]
fn a_sketch_that_does_not_solve_is_marked_and_can_be_mended() {
    // As a file could hold it: a line between fixed points held level,
    // though they aren't.
    let mut sketch_in_file = varde_document::Sketch::default();
    let a = sketch_in_file.add_point(DVec2::ZERO).unwrap();
    let b = sketch_in_file.add_point(DVec2::new(4.0, 1.0)).unwrap();
    sketch_in_file
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let level = [
        Constraint::Fix(a),
        Constraint::Fix(b),
        Constraint::HorizontalPoints(a, b),
    ]
    .map(|constraint| sketch_in_file.add_constraint(constraint).unwrap())[2];
    let mut editor = Editor::new(Document::default());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(OriginPlane::XY)))
        .unwrap();
    let feature = editor.document().features()[0].id;
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch_in_file),
        })
        .unwrap();
    let requests = std::rc::Rc::default();
    let mut doc = Doc::new(
        editor.document().clone(),
        Origin::new(Target::None, varde_io::Access::Edit, "Old".to_owned()),
    );
    doc.feed
        .connect(crate::tests::Deferred(std::rc::Rc::clone(&requests)));
    doc.sync();
    let mut lane = SolveLane::connect(&mut doc);
    answer(&mut doc, &requests);
    // Marked in the Timeline.
    assert_eq!(doc.feed.unsolved(), [feature]);

    doc.look(Look::EditFeature(feature));
    lane.answer(&mut doc);
    let state = doc.sketch_state().unwrap();
    assert!(state.unsolved);
    let analysis = state.analysis.unwrap();
    assert!(!analysis.solved);
    assert!(analysis.redundant.contains(&level));

    // Edits are proposed as usual, and one that mends it is accepted.
    select(&mut doc, &[level]);
    doc.update(Edit::DeleteSelection);
    lane.answer(&mut doc);
    let state = doc.sketch_state().unwrap();
    assert!(state.refusal.is_none());
    assert!(sketch(&doc).constraint(level).is_none());
    let analysis = state.analysis.unwrap();
    assert!(analysis.solved && analysis.redundant.is_empty());
    answer(&mut doc, &requests);
    assert!(doc.feed.unsolved().is_empty());
}

#[test]
fn hovering_a_glyph_or_row_and_clicking_it() {
    let (mut t, [_, _, line, ..]) = with_shapes();
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    let horizontal = sketch(&t).constraints[0].id;
    t.look(Look::HoverItem(Some(horizontal)));
    assert_eq!(t.sketch_state().unwrap().hovered, Some(horizontal));
    t.look(Look::ClickRow(horizontal));
    assert_eq!(selection(&t), [horizontal]);
    t.update(Edit::DeleteSelection);
    assert!(sketch(&t).constraints.is_empty());
    // What's gone isn't hovered.
    assert_eq!(t.sketch_state().unwrap().hovered, None);
    t.look(Look::ToggleGlyphs);
    assert!(!t.sketch_state().unwrap().glyphs);
}

#[test]
fn a_dropped_drag_shows_the_move_while_it_waits() {
    let (mut t, [_, b, ..]) = with_shapes();
    t.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(12.0, 3.0),
    });
    // The last step isn't answered before the drop.
    t.doc.look(Look::DragGeometry {
        id: b,
        from: at(10.0, 0.0),
        to: at(15.0, 3.0),
    });
    t.doc.update(Edit::DropGeometry);
    assert!(t.proposing());
    // Shown where it was dropped, not where the solver last had it.
    assert_eq!(position(shown(&t), b), at(15.0, 3.0));
    t.lane.answer(&mut t.doc);
    assert!(position(sketch(&t), b).abs_diff_eq(at(15.0, 3.0), 1e-4));
}

#[test]
fn an_accepted_edit_that_cannot_be_committed_leaves_the_analysis() {
    let (mut t, _, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    let before = t.editor.revision();
    click_waiting(&mut t.doc, 3.0, 4.0);
    // A Save As answered meanwhile left it read-only.
    t.doc.read_only = Some("read-only".to_owned());
    t.lane.answer(&mut t.doc);
    assert_eq!(t.editor.revision(), before);
    assert!(sketch(&t).points.is_empty());
    // The analysis shown is still the empty sketch's, not the point's.
    let state = t.sketch_state().unwrap();
    assert_eq!(state.analysis.map(|analysis| analysis.freedom), Some(0));
}

#[test]
fn redo_while_edits_wait_does_nothing() {
    let (mut t, _, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    click_waiting(&mut t.doc, 1.0, 1.0);
    t.lane.answer(&mut t.doc);
    t.update(Edit::Undo);
    assert!(t.editor.can_redo());
    let before = t.editor.revision();

    // A chain whose second line builds on the first, both waiting.
    t.look(Look::SelectTool(Tool::Line));
    click_waiting(&mut t.doc, 0.0, 0.0);
    click_waiting(&mut t.doc, 10.0, 0.0);
    click_waiting(&mut t.doc, 10.0, 10.0);
    // Redone under them, the ids they name would name other items.
    t.doc.update(Edit::Redo);
    assert_eq!(t.editor.revision(), before);
    t.lane.answer(&mut t.doc);
    let drawn = sketch(&t);
    let [(a, b), (c, d)] = lines(drawn)[..] else {
        panic!("{drawn:?}");
    };
    assert_eq!(b, c, "the chain shares its point");
    assert_eq!(
        [a, b, d].map(|id| position(drawn, id)),
        [at(0.0, 0.0), at(10.0, 0.0), at(10.0, 10.0)]
    );
}

#[test]
fn undo_after_an_edit_made_while_others_wait_takes_back_that_edit() {
    let (mut t, feature, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    click_waiting(&mut t.doc, 1.0, 1.0);
    t.doc.look(Look::FinishSketch);
    assert!(t.proposing());
    // Deleted before the solver answers, then taken back.
    t.doc.update(Edit::RemoveFeature(feature));
    t.doc.update(Edit::Undo);
    t.lane.answer(&mut t.doc);
    assert!(!t.proposing());
    let points =
        |t: &Answered| sketch_of(t.editor.document(), feature).map(|sketch| sketch.points.len());
    assert_eq!(points(&t), Some(1), "the deletion undone, the point kept");
    // The point is the next step back, and forward again; the deletion
    // taken back while it waited isn't redone.
    t.update(Edit::Undo);
    assert_eq!(points(&t), Some(0));
    t.update(Edit::Redo);
    assert_eq!(points(&t), Some(1));
    assert!(!t.editor.can_redo());
}

#[test]
fn changes_made_while_edits_wait_are_made_after_them_in_order() {
    let (mut t, feature, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    let before = t.editor.revision();
    click_waiting(&mut t.doc, 1.0, 1.0);
    // Hidden and shown again, each on the document as it is by then.
    t.doc.update(Edit::ToggleFeatureVisible(feature));
    t.doc.update(Edit::ToggleFeatureVisible(feature));
    click_waiting(&mut t.doc, 2.0, 2.0);
    assert_eq!(t.editor.revision(), before);
    assert_eq!(shown(&t).points.len(), 2);
    t.lane.answer(&mut t.doc);
    assert!(!t.proposing());
    assert_eq!(sketch(&t).points.len(), 2);
    let visible = |t: &Answered| t.editor.document().feature(feature).unwrap().visible;
    assert!(visible(&t));
    // Undone newest first: the second point, shown, hidden, the first.
    t.update(Edit::Undo);
    assert_eq!(sketch(&t).points.len(), 1);
    assert!(visible(&t));
    t.update(Edit::Undo);
    assert!(!visible(&t));
    t.update(Edit::Undo);
    assert!(visible(&t));
    assert_eq!(sketch(&t).points.len(), 1);
    t.update(Edit::Undo);
    assert_eq!(t.editor.revision(), before);
    // And redone in the same order.
    t.update(Edit::Redo);
    assert_eq!(sketch(&t).points.len(), 1);
    t.update(Edit::Redo);
    assert!(!visible(&t));
    t.update(Edit::Redo);
    t.update(Edit::Redo);
    assert!(visible(&t));
    assert_eq!(sketch(&t).points.len(), 2);
    assert!(!t.editor.can_redo());
}

#[test]
fn undo_takes_back_a_change_waiting_before_the_edit_it_waits_for() {
    let (mut t, feature, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    click_waiting(&mut t.doc, 1.0, 1.0);
    t.doc.update(Edit::ToggleFeatureVisible(feature));
    // The newest is the change: it goes, the point stays waiting.
    t.doc.update(Edit::Undo);
    assert!(t.proposing());
    assert_eq!(shown(&t).points.len(), 1);
    // Redo has nothing to bring back while the point waits.
    t.doc.update(Edit::Redo);
    t.lane.answer(&mut t.doc);
    assert_eq!(sketch(&t).points.len(), 1);
    assert!(t.editor.document().feature(feature).unwrap().visible);
    assert!(!t.editor.can_redo());
}

#[test]
fn undo_and_redo_storms_while_edits_wait_keep_the_order() {
    let (mut t, feature, _) = sketching();
    t.look(Look::SelectTool(Tool::Point));
    click_waiting(&mut t.doc, 1.0, 1.0);
    t.lane.answer(&mut t.doc);
    let one = t.editor.revision();
    click_waiting(&mut t.doc, 2.0, 2.0);
    t.doc.update(Edit::ToggleFeatureVisible(feature));
    click_waiting(&mut t.doc, 3.0, 3.0);
    t.doc.update(Edit::ToggleFeatureVisible(feature));
    // Redo brings nothing back, however often.
    for _ in 0..5 {
        t.doc.update(Edit::Redo);
    }
    assert_eq!(shown(&t).points.len(), 3);
    // Five undos: the four waiting newest first, then the point committed.
    for _ in 0..5 {
        t.doc.update(Edit::Undo);
    }
    assert!(!t.proposing());
    assert!(sketch(&t).points.is_empty());
    assert_ne!(t.editor.revision(), one);
    // The answer to the point dropped while with the lane is ignored, and
    // edits go on: redone, the first point; a new one after it.
    t.doc.update(Edit::Redo);
    assert_eq!(t.editor.revision(), one);
    click_waiting(&mut t.doc, 4.0, 4.0);
    for _ in 0..3 {
        t.doc.update(Edit::Redo);
    }
    t.lane.answer(&mut t.doc);
    assert!(!t.proposing());
    let points: Vec<_> = sketch(&t).points.iter().map(|point| point.at).collect();
    assert_eq!(points, [at(1.0, 1.0), at(4.0, 4.0)]);
    assert!(t.editor.document().feature(feature).unwrap().visible);
    assert!(!t.editor.can_redo());
}

/// [`with_shapes`] with its line made horizontal, then made horizontal
/// again, which the solver refuses, left with the solver, and the sketch
/// left: the sketch's id and the sketch as committed.
fn refused_after_leaving() -> (Answered, FeatureId, varde_document::Sketch) {
    let (mut t, [_, _, line, ..]) = with_shapes();
    let feature = t.sketch.as_ref().unwrap().feature;
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    let committed = sketch(&t).clone();
    select(&mut t.doc, &[line]);
    t.doc.update(Edit::Constrain(ConstraintKind::Horizontal));
    assert!(t.proposing());
    t.doc.look(Look::FinishSketch);
    assert!(t.sketch.is_none());
    assert!(t.refused_edit().is_none());
    (t, feature, committed)
}

#[test]
fn an_edit_refused_after_the_sketch_was_left_shows_a_banner_until_dismissed() {
    let (mut t, feature, committed) = refused_after_leaving();
    let revision = t.editor.revision();
    t.lane.answer(&mut t.doc);
    assert_eq!(t.editor.revision(), revision);
    assert!(t.edited_sketch().is_none());
    let Some(FeatureKind::Sketch { sketch, .. }) =
        t.editor.document().feature(feature).map(|f| &f.kind)
    else {
        panic!("no sketch");
    };
    assert_eq!(*sketch, committed);
    let refused = t.refused_edit().expect("a banner says so");
    assert_eq!(refused.name, "Sketch 1");
    assert!(matches!(refused.why, Some(Rejected::Redundant { .. })));
    assert_eq!(refused.error, None);
    let texts = crate::tests::screen_texts(&t);
    assert!(
        texts.contains(&"An edit of Sketch 1 wasn't kept".to_owned()),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"— Would over-constrain the sketch".to_owned()),
        "{texts:?}"
    );
    assert!(texts.contains(&"Dismiss".to_owned()), "{texts:?}");

    // It stays through other actions, and goes when dismissed.
    t.update(Edit::ToggleFeatureVisible(feature));
    t.look(Look::SelectFeature(feature));
    assert!(t.refused_edit().is_some());
    t.update(Edit::DismissRefusedEdit);
    assert!(t.refused_edit().is_none());
    let texts = crate::tests::screen_texts(&t);
    assert!(
        !texts.iter().any(|text| text.contains("wasn't kept")),
        "{texts:?}"
    );
}

#[test]
fn an_edit_refused_in_the_sketch_shows_no_banner() {
    let (mut t, [_, _, line, ..]) = with_shapes();
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    select(&mut t, &[line]);
    t.update(Edit::Constrain(ConstraintKind::Horizontal));
    assert!(t.sketch_state().unwrap().refusal.is_some());
    assert!(t.refused_edit().is_none());
    // Nor once the sketch is left: the status bar said so.
    t.look(Look::FinishSketch);
    assert!(t.refused_edit().is_none());
}

#[test]
fn the_banner_goes_back_into_the_sketch_or_with_it() {
    // Back into the sketch, the edit is seen to be missing.
    let (mut t, feature, _) = refused_after_leaving();
    t.lane.answer(&mut t.doc);
    assert!(t.refused_edit().is_some());
    t.look(Look::EditFeature(feature));
    assert!(t.refused_edit().is_none());

    // Deleting the sketch takes it along.
    let (mut t, feature, _) = refused_after_leaving();
    t.lane.answer(&mut t.doc);
    t.update(Edit::RemoveFeature(feature));
    assert!(t.editor.document().feature(feature).is_none());
    assert!(t.refused_edit().is_none());
    assert!(t.doc.refused_edit.is_none());

    // A solver that fails to answer is said to have, and how.
    let (mut t, _, _) = refused_after_leaving();
    let Some(Request::Propose { base, .. }) = t.lane.waiting().first().cloned() else {
        panic!("a proposal waits");
    };
    let _ = t.lane.respond();
    t.doc.solved(varde_solve::Response::Failed {
        tag: Tag::Propose(base),
        error: "worker stopped".to_owned(),
    });
    let refused = t.refused_edit().expect("a banner says so");
    assert!(refused.why.is_none());
    assert_eq!(refused.error, Some("worker stopped"));
    let texts = crate::tests::screen_texts(&t);
    assert!(
        texts.contains(&"— Couldn't check the edit: worker stopped".to_owned()),
        "{texts:?}"
    );
}
