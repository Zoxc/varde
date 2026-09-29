use std::sync::Arc;

use glam::DVec2;
use varde_sketch::{Sketch, SketchEdit};

use super::*;
use crate::{LengthUnit, Tag};

fn propose(base: u64) -> Request {
    Request::Propose {
        base: base.into(),
        sketch: Arc::new(Sketch::default()),
        edit: SketchEdit::Delete(Vec::new()),
        units: LengthUnit::Mm,
    }
}

fn analyse(revision: u64) -> Request {
    Request::Analyse {
        revision: revision.into(),
        sketch: Arc::new(Sketch::default()),
        units: LengthUnit::Mm,
    }
}

/// A step of `session`, told apart from the session's others by how many
/// points it drags.
fn drag(session: u64, step: usize) -> Request {
    let mut sketch = Sketch::default();
    let point = sketch.add_point(DVec2::ZERO).unwrap();
    Request::Drag {
        session,
        sketch: Arc::new(sketch),
        points: vec![(point, DVec2::ZERO); step],
        radii: Vec::new(),
        units: LengthUnit::Mm,
    }
}

/// What `request` is: its tag, and a drag step's number.
fn tag(request: Option<Request>) -> Option<(Tag, usize)> {
    request.map(|request| match &request {
        Request::Drag { points, .. } => (request.tag(), points.len()),
        _ => (request.tag(), 0),
    })
}

fn proposed(base: u64) -> Option<(Tag, usize)> {
    Some((Tag::Propose(base.into()), 0))
}

fn analysed(revision: u64) -> Option<(Tag, usize)> {
    Some((Tag::Analyse(revision.into()), 0))
}

fn dragged(session: u64, step: usize) -> Option<(Tag, usize)> {
    Some((Tag::Drag(session), step))
}

/// Everything `order` holds, in the order it gives it.
fn drain(order: &mut Order) -> Vec<Option<(Tag, usize)>> {
    std::iter::from_fn(|| order.pop().map(Some))
        .map(tag)
        .collect()
}

#[test]
fn proposals_are_taken_in_order_and_none_is_dropped() {
    let mut order = Order::default();
    for base in 0..5 {
        assert_eq!(tag(order.push(propose(base))), None);
    }
    assert_eq!(drain(&mut order), (0..5).map(proposed).collect::<Vec<_>>());
    assert!(order.is_empty());
}

#[test]
fn a_newer_drag_step_replaces_the_one_waiting() {
    let mut order = Order::default();
    assert_eq!(tag(order.push(drag(1, 1))), None);
    assert_eq!(tag(order.push(drag(1, 2))), dragged(1, 1));
    assert_eq!(tag(order.push(drag(1, 3))), dragged(1, 2));
    assert_eq!(drain(&mut order), [dragged(1, 3)]);
}

#[test]
fn a_new_session_replaces_the_old_one() {
    let mut order = Order::default();
    order.push(drag(1, 1));
    assert_eq!(tag(order.push(drag(2, 1))), dragged(1, 1));
    // The old session is over: its steps are refused, waiting or not.
    assert_eq!(tag(order.push(drag(1, 2))), dragged(1, 2));
    assert_eq!(drain(&mut order), [dragged(2, 1)]);
    assert_eq!(tag(order.push(drag(1, 3))), dragged(1, 3));
    assert!(order.is_empty());
}

#[test]
fn a_newer_analysis_replaces_the_one_waiting() {
    let mut order = Order::default();
    order.push(analyse(1));
    order.push(propose(1));
    assert_eq!(tag(order.push(analyse(2))), analysed(1));
    assert_eq!(drain(&mut order), [proposed(1), analysed(2)]);
}

#[test]
fn proposals_are_not_stuck_behind_drags() {
    let mut order = Order::default();
    order.push(propose(1));
    order.push(propose(2));
    // However fast drag steps come, each proposal waits for one at most.
    let mut taken = Vec::new();
    for step in 1..=4 {
        order.push(drag(7, step));
        taken.push(tag(order.pop()));
    }
    assert_eq!(
        taken,
        [dragged(7, 1), proposed(1), dragged(7, 3), proposed(2)]
    );
    assert_eq!(drain(&mut order), [dragged(7, 4)]);
}

#[test]
fn drags_are_not_stuck_behind_a_queue() {
    let mut order = Order::default();
    for base in 1..=3 {
        order.push(propose(base));
    }
    assert_eq!(tag(order.pop()), proposed(1));
    // A drag step waits for the proposal running, not the queue.
    order.push(drag(1, 1));
    assert_eq!(tag(order.pop()), dragged(1, 1));
    assert_eq!(tag(order.pop()), proposed(2));
    order.push(drag(1, 2));
    assert_eq!(drain(&mut order), [dragged(1, 2), proposed(3)]);
}
