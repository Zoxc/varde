use glam::DVec2;
use varde_lane::thread::testing::{join_in_time, next};
use varde_sketch::{Constraint, SketchEdit};

use super::*;
use crate::Transport;
use crate::tests::{analyse_at, drag, drawn, propose_on};

#[test]
fn round_trip() {
    let drawn = drawn();
    let (mut lane, mut responses) = spawn();
    let edit = SketchEdit::constrain(&drawn.sketch, vec![Constraint::Fix(drawn.start)]);
    lane.send(propose_on(1, &drawn.sketch, edit));
    let Response::Accepted { base, sketch, .. } = next(&mut responses) else {
        panic!("not accepted");
    };
    assert_eq!(u64::from(base), 1);
    assert_eq!(sketch.constraints.len(), 2);

    lane.send(analyse_at(2, &drawn.sketch));
    assert!(matches!(
        next(&mut responses),
        Response::Analysed { revision, .. } if u64::from(revision) == 2
    ));

    let to = DVec2::new(12.0, 4.0);
    lane.send(drag(3, &drawn.sketch, drawn.end, to));
    let Response::Dragged { session, solution } = next(&mut responses) else {
        panic!("the drag didn't move");
    };
    assert_eq!(session, 3);
    assert!(solution.point(drawn.end).unwrap().at.distance(to) < 1e-3);
}

#[test]
fn proposals_are_answered_in_order_among_drags() {
    let drawn = drawn();
    let (mut lane, mut responses) = spawn();
    let edit = SketchEdit::Delete(Vec::new());
    for base in 0..10 {
        lane.send(propose_on(base, &drawn.sketch, edit.clone()));
        for step in 0..10 {
            let to = DVec2::new(10.0 + f64::from(step), 1.0);
            lane.send(drag(1, &drawn.sketch, drawn.end, to));
        }
    }
    // Every proposal is answered, in order, and the drag's last step is
    // answered after all of them were sent; some steps may be skipped.
    let mut bases = Vec::new();
    let mut last_drag = None;
    while bases.len() < 10 || last_drag.is_none_or(|x: f64| (x - 19.0).abs() > 1e-3) {
        match next(&mut responses) {
            Response::Accepted { base, .. } => bases.push(u64::from(base)),
            Response::Dragged { solution, .. } => {
                last_drag = Some(solution.point(drawn.end).unwrap().at.x);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(bases, (0..10).collect::<Vec<_>>());
}

#[test]
fn thread_ends_when_responses_are_dropped() {
    let drawn = drawn();
    let (mut lane, responses) = spawn();
    let join = responses.close();
    // The lane handle outliving the stream doesn't keep the thread alive.
    lane.send(analyse_at(1, &drawn.sketch));
    join_in_time(join);
}

#[test]
fn lane_keeps_going_after_a_job_panics() {
    let drawn = drawn();
    let mut solver = Solver::default();
    let (mut lane, mut responses) = spawn_on(move |request| {
        assert!(
            !matches!(request, Request::Drag { session: 1, .. }),
            "the solver failed"
        );
        solver.handle(request)
    });
    lane.send(drag(1, &drawn.sketch, drawn.end, DVec2::ZERO));
    let Response::Failed { tag, error } = next(&mut responses) else {
        panic!("the panic wasn't answered");
    };
    assert_eq!(tag, crate::Tag::Drag(1));
    assert_eq!(error, "internal error: the solver failed");

    lane.send(analyse_at(2, &drawn.sketch));
    assert!(matches!(next(&mut responses), Response::Analysed { .. }));
}
