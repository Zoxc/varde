#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::PI;

use glam::DVec2;

use super::*;
use crate::testing::{DESIGN, arc, dimension, line, point, propose_it};
use crate::{Constraint, Measure, SketchEdit};

/// A quarter arc of radius 10 about the origin's right, from (10, 0) to
/// (0, 10): the sketch, the arc, its centre, start and end.
fn quarter() -> (Sketch, Id, [Id; 3]) {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let start = point(&mut sketch, 10.0, 0.0);
    let end = point(&mut sketch, 0.0, 10.0);
    let id = arc(&mut sketch, center, start, end);
    (sketch, id, [center, start, end])
}

/// Closed, an arc's end is its start: it has no ends, its point counts
/// once, it bounds a disc and solves, with no equation of its radius
/// left to be redundant.
#[test]
fn a_closed_arc_runs_all_the_way_round() {
    let (sketch, id, [center, start, end]) = quarter();
    assert!(sketch.closable(id));
    let mut closed = SketchEdit::CloseArc(id).apply(&sketch, &DESIGN).unwrap();
    let curve = &closed.curve(id).unwrap().curve;
    assert_eq!(
        *curve,
        Curve::Arc {
            center,
            start,
            end: start
        }
    );
    assert!(curve.closed_arc());
    assert_eq!(curve.ends(), None);
    assert_eq!(curve.points().collect::<Vec<_>>(), [center, start]);
    assert!(closed.point(end).is_none());
    assert!(!closed.closable(id));
    let profiles = closed.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 1);
    dimension(&mut closed, Measure::Radius(id), "4");
    let solved = propose_it(&closed, &SketchEdit::Add(crate::Add::new(&closed))).unwrap();
    let at = |id| solved.sketch.point(id).unwrap().at;
    assert!((at(start).distance(at(center)) - 4.0).abs() < 1e-9);
    let flat = solved
        .sketch
        .flatten(&solved.sketch.curve(id).unwrap().curve)
        .unwrap();
    let length: f64 = flat.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    assert!((length - 8.0 * PI).abs() < 0.1, "{length}");
}

/// What's on the end goes, and a line from the end is from the start
/// now.
#[test]
fn closing_hands_the_end_over_to_the_start() {
    let (mut sketch, id, [_, start, end]) = quarter();
    let far = point(&mut sketch, -5.0, 15.0);
    let spoke = line(&mut sketch, end, far);
    let other = point(&mut sketch, 0.0, 10.0);
    sketch
        .add_constraint(Constraint::Coincident(end, other))
        .unwrap();
    let kept = sketch
        .add_constraint(Constraint::Horizontal(spoke))
        .unwrap();
    let closed = SketchEdit::CloseArc(id).apply(&sketch, &DESIGN).unwrap();
    assert_eq!(
        closed.curve(spoke).unwrap().curve,
        Curve::Line { start, end: far }
    );
    assert_eq!(closed.constraints.len(), 1);
    assert_eq!(closed.constraints[0].id, kept);
}

/// A line, a circle, a closed arc or a fillet can't be closed.
#[test]
fn only_an_open_arc_closes() {
    let (sketch, id, [center, start, _]) = quarter();
    let closed = SketchEdit::CloseArc(id).apply(&sketch, &DESIGN).unwrap();
    let mut sketch = closed.clone();
    let spoke = line(&mut sketch, center, start);
    for id in [id, spoke, center] {
        assert!(!sketch.closable(id));
        assert_eq!(
            SketchEdit::CloseArc(id).apply(&sketch, &DESIGN),
            Err(EditError::Target(id))
        );
    }
}

/// Trimmed, a closed arc is cut as a circle is: the piece between the
/// cuts either side of the click goes, and it's an open arc.
#[test]
fn a_closed_arc_trims_as_a_circle() {
    let (sketch, id, [_, start, _]) = quarter();
    let mut closed = SketchEdit::CloseArc(id).apply(&sketch, &DESIGN).unwrap();
    let [a, b] = [(-20.0, 5.0), (20.0, 5.0)].map(|(x, y)| point(&mut closed, x, y));
    line(&mut closed, a, b);
    let near = DVec2::new(0.0, 10.0);
    let trimmed = SketchEdit::Trim { curve: id, near }
        .apply(&closed, &DESIGN)
        .unwrap();
    let Curve::Arc {
        start: from,
        end: to,
        ..
    } = trimmed.curve(id).unwrap().curve
    else {
        panic!("not an arc");
    };
    assert_ne!(from, to);
    let at = |id| trimmed.point(id).unwrap().at;
    assert!((at(from).y - 5.0).abs() < 1e-9 && (at(to).y - 5.0).abs() < 1e-9);
    // What's left runs counter-clockwise from the left round below.
    assert!(at(from).x < 0.0 && at(to).x > 0.0);
    // The point it ran round from is no curve's now.
    assert!(trimmed.point(start).is_none());
}
