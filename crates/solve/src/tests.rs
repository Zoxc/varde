use glam::DVec2;
use varde_sketch::{Constraint, Curve};

use super::*;

/// A solved sketch: a horizontal line from (0, 0) to (10, 0) and a circle
/// of radius 3 around (20, 0). Its ends, the line, the circle and the
/// horizontal.
pub(crate) struct Drawn {
    pub sketch: Arc<Sketch>,
    pub start: Id,
    pub end: Id,
    pub line: Id,
    pub circle: Id,
    pub horizontal: Id,
}

pub(crate) fn drawn() -> Drawn {
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::new(10.0, 0.0)).unwrap();
    let line = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    let center = sketch.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 3.0,
            },
            false,
        )
        .unwrap();
    let horizontal = sketch.add_constraint(Constraint::Horizontal(line)).unwrap();
    assert_eq!(sketch.check(&design(LengthUnit::Mm)), Ok(()));
    Drawn {
        sketch: Arc::new(sketch),
        start,
        end,
        line,
        circle,
        horizontal,
    }
}

pub(crate) fn propose_on(base: u64, sketch: &Arc<Sketch>, edit: SketchEdit) -> Request {
    Request::Propose {
        base: base.into(),
        sketch: Arc::clone(sketch),
        edit,
        units: LengthUnit::Mm,
    }
}

pub(crate) fn drag(session: u64, sketch: &Arc<Sketch>, point: Id, to: DVec2) -> Request {
    Request::Drag {
        session,
        sketch: Arc::clone(sketch),
        points: vec![(point, to)],
        radii: Vec::new(),
        units: LengthUnit::Mm,
    }
}

pub(crate) fn analyse_at(revision: u64, sketch: &Arc<Sketch>) -> Request {
    Request::Analyse {
        revision: revision.into(),
        sketch: Arc::clone(sketch),
        units: LengthUnit::Mm,
    }
}

/// How near a dragged point ends to its target: the solver weighs it a
/// million times the rest, so it gives way by about a millionth of how
/// far the rest had to move.
const NEAR: f64 = 1e-4;

fn at(sketch: &Sketch, id: Id) -> DVec2 {
    sketch.point(id).unwrap().at
}

#[test]
fn an_accepted_proposal_is_solved_analysed_and_tagged() {
    let drawn = drawn();
    let edit = SketchEdit::constrain(&drawn.sketch, vec![Constraint::Fix(drawn.start)]);
    let response = Solver::default().handle(propose_on(3, &drawn.sketch, edit));
    let Some(Response::Accepted {
        base,
        sketch,
        analysis,
    }) = response
    else {
        panic!("not accepted: {response:?}");
    };
    assert_eq!(base, Revision::from(3));
    assert_eq!(sketch.constraints.len(), 2);
    assert_eq!(*analysis, varde_sketch::analyse(&sketch));
    // The line's start is fixed; its end moves along it, the circle freely.
    assert_eq!(analysis.freedom, 4);
    assert!(analysis.fixed.contains(&drawn.start));
}

#[test]
fn a_redundant_proposal_is_rejected_naming_what_is_involved() {
    let drawn = drawn();
    let edit = SketchEdit::constrain(&drawn.sketch, vec![Constraint::Horizontal(drawn.line)]);
    let response = Solver::default().handle(propose_on(5, &drawn.sketch, edit));
    let Some(Response::Rejected { base, why }) = &response else {
        panic!("not rejected: {response:?}");
    };
    assert_eq!(*base, Revision::from(5));
    assert!(matches!(why, Rejected::Redundant { .. }));
    assert!(why.involved().contains(&drawn.horizontal));
    assert_eq!(response.unwrap().tag(), Tag::Propose(5.into()));
}

#[test]
fn an_edit_that_does_not_apply_is_rejected() {
    let drawn = drawn();
    let edit = SketchEdit::SetConstruction {
        ids: vec![drawn.start],
        construction: true,
    };
    let response = Solver::default().handle(propose_on(0, &drawn.sketch, edit));
    assert!(matches!(
        response,
        Some(Response::Rejected {
            why: Rejected::Edit(_),
            ..
        })
    ));
}

#[test]
fn an_analysis_is_of_the_sketch_sent() {
    let drawn = drawn();
    let response = Solver::default().handle(analyse_at(7, &drawn.sketch));
    let Some(Response::Analysed { revision, analysis }) = response else {
        panic!("not analysed: {response:?}");
    };
    assert_eq!(revision, Revision::from(7));
    assert_eq!(*analysis, varde_sketch::analyse(&drawn.sketch));
    // Two points of the line less its horizontal, the circle's centre and
    // radius.
    assert_eq!(analysis.freedom, 6);
}

#[test]
fn drag_steps_go_on_from_the_last_solution() {
    let drawn = drawn();
    let mut solver = Solver::default();
    let to = DVec2::new(12.0, 4.0);
    let Some(Response::Dragged { session, solution }) =
        solver.handle(drag(1, &drawn.sketch, drawn.end, to))
    else {
        panic!("the drag didn't move");
    };
    assert_eq!(session, 1);
    // The end is about where it was dragged, the line still horizontal.
    assert!(at(&solution, drawn.end).distance(to) < NEAR);
    assert!((at(&solution, drawn.start).y - at(&solution, drawn.end).y).abs() < 1e-9);

    // The session's own sketch is only where it starts: the next step
    // goes on from the last solution, whatever sketch it's sent with.
    let to = DVec2::new(14.0, 4.0);
    let Some(Response::Dragged { solution, .. }) =
        solver.handle(drag(1, &Arc::new(Sketch::default()), drawn.end, to))
    else {
        panic!("the drag didn't move");
    };
    assert!(at(&solution, drawn.end).distance(to) < NEAR);
    assert_eq!(solution.points.len(), drawn.sketch.points.len());
}

#[test]
fn a_new_session_replaces_the_old_one() {
    let drawn = drawn();
    let mut solver = Solver::default();
    solver.handle(drag(1, &drawn.sketch, drawn.end, DVec2::new(12.0, 4.0)));
    // Session 2 starts from its own sketch, not where session 1 left off.
    let Some(Response::Dragged { session, solution }) =
        solver.handle(drag(2, &drawn.sketch, drawn.start, DVec2::new(-1.0, 0.0)))
    else {
        panic!("the drag didn't move");
    };
    assert_eq!(session, 2);
    assert!(at(&solution, drawn.end).distance(DVec2::new(10.0, 0.0)) < 1e-6);
    assert!(at(&solution, drawn.start).distance(DVec2::new(-1.0, 0.0)) < 1e-6);
}

#[test]
fn a_drag_step_that_does_not_converge_is_not_answered() {
    let drawn = drawn();
    let mut solver = Solver::default();
    let to = DVec2::new(12.0, 4.0);
    solver.handle(drag(1, &drawn.sketch, drawn.end, to));
    // Past the coordinate limit: refused, keeping the last solution.
    let far = DVec2::new(2.0 * MAX, 0.0);
    assert!(
        solver
            .handle(drag(1, &drawn.sketch, drawn.end, far))
            .is_none()
    );
    let back = DVec2::new(11.0, 4.0);
    let Some(Response::Dragged { solution, .. }) =
        solver.handle(drag(1, &drawn.sketch, drawn.end, back))
    else {
        panic!("the drag didn't move");
    };
    assert!(at(&solution, drawn.end).distance(back) < NEAR);
    assert!((at(&solution, drawn.start).y - 4.0).abs() < NEAR);
}

#[test]
fn a_step_of_an_unknown_session_without_its_sketch_is_not_answered() {
    let drawn = drawn();
    let mut solver = Solver::default();
    let to = DVec2::new(12.0, 4.0);
    assert!(
        solver
            .drag(1, None, vec![(drawn.end, to)], Vec::new(), LengthUnit::Mm)
            .is_none()
    );
    // Nor once another session is in progress.
    solver.handle(drag(1, &drawn.sketch, drawn.end, to));
    assert!(
        solver
            .drag(2, None, vec![(drawn.end, to)], Vec::new(), LengthUnit::Mm)
            .is_none()
    );
    // Which goes on.
    assert!(
        solver
            .drag(1, None, vec![(drawn.end, to)], Vec::new(), LengthUnit::Mm)
            .is_some()
    );
}

#[test]
fn drags_leave_proposals_alone() {
    // A drag in progress changes nothing a proposal is made on.
    let drawn = drawn();
    let mut solver = Solver::default();
    solver.handle(drag(1, &drawn.sketch, drawn.end, DVec2::new(12.0, 4.0)));
    let edit = SketchEdit::Delete(Vec::new());
    let Some(Response::Accepted { sketch, .. }) = solver.handle(propose_on(1, &drawn.sketch, edit))
    else {
        panic!("not accepted");
    };
    assert_eq!(*sketch, *drawn.sketch);
}

#[test]
fn circles_are_dragged_by_their_radius() {
    let drawn = drawn();
    let Some(Response::Dragged { solution, .. }) = Solver::default().handle(Request::Drag {
        session: 1,
        sketch: Arc::clone(&drawn.sketch),
        points: Vec::new(),
        radii: vec![(drawn.circle, 5.0)],
        units: LengthUnit::Mm,
    }) else {
        panic!("the drag didn't move");
    };
    let Curve::Circle { radius, .. } = solution.curve(drawn.circle).unwrap().curve else {
        unreachable!()
    };
    assert!((radius - 5.0).abs() < 1e-6);
}

#[test]
fn failures_name_their_request() {
    let drawn = drawn();
    let response = Response::Failed {
        tag: drag(4, &drawn.sketch, drawn.end, DVec2::ZERO).tag(),
        error: "internal error".to_owned(),
    };
    assert_eq!(response.tag(), Tag::Drag(4));
    assert_eq!(
        analyse_at(2, &drawn.sketch).tag(),
        Tag::Analyse(Revision::from(2))
    );
}
