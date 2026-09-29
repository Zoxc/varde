use glam::DVec2;

use super::*;
use crate::testing::{DESIGN, arc, at, circle, constrain, line, point};
use crate::{Budget, Goal, Handle, SketchError, Spline, analyse, solve};

/// A spline through `places`, with a handle at each of `handles`, its
/// tip at the fit point's place and the offset: its id, its fit points
/// and its tips.
fn spline(
    sketch: &mut Sketch,
    places: &[(f64, f64)],
    handles: &[(usize, (f64, f64))],
) -> (Id, Vec<Id>, Vec<Id>) {
    let fit: Vec<Id> = places.iter().map(|&(x, y)| point(sketch, x, y)).collect();
    spline_through(sketch, fit, handles)
}

/// A spline through the points `fit`, with handles as for [`spline`].
fn spline_through(
    sketch: &mut Sketch,
    fit: Vec<Id>,
    handles: &[(usize, (f64, f64))],
) -> (Id, Vec<Id>, Vec<Id>) {
    let mut made = Spline::through(fit.clone(), false);
    let mut tips = Vec::new();
    for &(i, (dx, dy)) in handles {
        let from = at(sketch, fit[i]);
        let tip = point(sketch, from.x + dx, from.y + dy);
        made.handles.push(Handle { at: fit[i], tip });
        tips.push(tip);
    }
    let id = sketch.add_curve(Curve::Spline(made), false).unwrap();
    (id, fit, tips)
}

fn settle(sketch: &Sketch) -> Sketch {
    solve(sketch, &Goal::Settle, &Budget::default())
        .unwrap()
        .sketch
}

/// Whether `a` and `b` run along each other, the same way if `same`.
fn along(a: DVec2, b: DVec2, same: bool) -> bool {
    let (a, b) = (a.normalize(), b.normalize());
    a.perp_dot(b).abs() < 1e-8 && (a.dot(b) > 0.0) == same
}

#[test]
fn a_joint_is_at_a_spline_s_end() {
    let mut sketch = Sketch::default();
    let (s, fit, _) = spline(&mut sketch, &[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0)], &[]);
    let far = point(&mut sketch, -10.0, 0.0);
    let joined = line(&mut sketch, far, fit[0]);
    let center = point(&mut sketch, 0.0, -30.0);
    let round = circle(&mut sketch, center, 5.0);
    let shared = Joint::of(&sketch, joined, s, fit[0]);
    assert_eq!(
        shared,
        Some(Joint {
            spline: s,
            other: joined,
            shared: true
        })
    );
    let apart = Joint::of(&sketch, s, round, fit[2]).unwrap();
    assert!(!apart.shared && apart.spline == s && apart.other == round);
    // Not at a fit point in the middle, nor without a spline.
    assert_eq!(Joint::of(&sketch, s, joined, fit[1]), None);
    assert_eq!(Joint::of(&sketch, joined, round, fit[0]), None);
    // Two splines only where they share an end.
    let (other, _, _) = spline_through(&mut sketch, vec![fit[2], far], &[]);
    assert!(Joint::of(&sketch, s, other, fit[2]).unwrap().shared);
    assert_eq!(Joint::of(&sketch, s, other, fit[0]), None);
    // A closed spline has no end.
    let mut closed = sketch.clone();
    let Curve::Spline(made) = &mut closed.curve_mut(s).unwrap().curve else {
        unreachable!()
    };
    made.closed = true;
    assert_eq!(Joint::of(&closed, s, joined, fit[0]), None);
}

#[test]
fn a_tangent_or_smooth_join_is_made_at_the_end_nearer_and_on_its_side() {
    let mut sketch = Sketch::default();
    let (s, fit, _) = spline(&mut sketch, &[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0)], &[]);
    let (a, b) = (
        point(&mut sketch, 30.0, -1.0),
        point(&mut sketch, 40.0, -3.0),
    );
    let right = line(&mut sketch, a, b);
    // Nearer its last point; the line runs the way the spline ends.
    assert_eq!(
        sketch.tangent(s, right),
        Some(Constraint::Tangent {
            a: s,
            b: right,
            side: Side::Positive,
            at: Some(fit[2]),
        })
    );
    // A shared end is taken, and running into each other there is the
    // negative side.
    let joined = line(&mut sketch, a, fit[0]);
    assert_eq!(
        sketch.tangent(joined, s),
        Some(Constraint::Tangent {
            a: joined,
            b: s,
            side: Side::Negative,
            at: Some(fit[0]),
        })
    );
    // Smooth needs a handle at a spline's end through fit points.
    assert_eq!(sketch.smooth(joined, s), None);
    let tip = sketch.add_point(DVec2::new(3.0, 0.0)).unwrap();
    let Curve::Spline(made) = &mut sketch.curve_mut(s).unwrap().curve else {
        unreachable!()
    };
    made.handles.push(Handle { at: fit[0], tip });
    assert_eq!(
        sketch.smooth(joined, s),
        Some(Constraint::Smooth {
            a: joined,
            b: s,
            at: fit[0],
            side: Side::Negative,
        })
    );
}

#[test]
fn check_refuses_a_join_at_no_spline_s_end_or_a_straight_end_smooth() {
    let mut sketch = Sketch::default();
    let (s, fit, _) = spline(&mut sketch, &[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0)], &[]);
    let far = point(&mut sketch, -10.0, 0.0);
    let joined = line(&mut sketch, far, fit[0]);
    let refused = |constraint: Constraint| {
        let mut sketch = sketch.clone();
        let id = sketch.add_constraint(constraint).unwrap();
        sketch.check(&DESIGN).err().map(|error| (error, id))
    };
    let tangent = |at| Constraint::Tangent {
        a: joined,
        b: s,
        side: Side::Positive,
        at,
    };
    assert_eq!(refused(tangent(Some(fit[0]))), None);
    let middle = refused(tangent(Some(fit[1]))).unwrap();
    assert_eq!(middle.0, SketchError::Unfit(middle.1));
    // A spline needs where it touches named, and a line and a circle
    // mustn't have it.
    assert!(matches!(
        refused(tangent(None)),
        Some((SketchError::Reference { .. }, _))
    ));
    let straight = refused(Constraint::Smooth {
        a: joined,
        b: s,
        at: fit[0],
        side: Side::Positive,
    })
    .unwrap();
    assert_eq!(straight.0, SketchError::Unfit(straight.1));
}

#[test]
fn a_line_tangent_at_a_spline_s_end_turns_it_along() {
    let mut sketch = Sketch::default();
    let (s, fit, _) = spline(&mut sketch, &[(0.0, 0.0), (10.0, 5.0), (20.0, 0.0)], &[]);
    let far = point(&mut sketch, -10.0, 0.0);
    let joined = line(&mut sketch, far, fit[0]);
    constrain(&mut sketch, Constraint::Fix(joined));
    assert_eq!(analyse(&sketch).freedom, 4);
    let tangent = sketch.tangent(joined, s).unwrap();
    constrain(&mut sketch, tangent);
    let solved = settle(&sketch);
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    assert_eq!(analysis.freedom, 3);
    let (way, _) = solved.heading(s, fit[0]).unwrap();
    assert!(along(way, DVec2::X, true), "{way}");

    // Apart, its end is held on the line too.
    let mut sketch = Sketch::default();
    let (s, fit, _) = spline(&mut sketch, &[(1.0, 1.0), (10.0, 5.0), (20.0, 8.0)], &[]);
    let (a, b) = (
        point(&mut sketch, -10.0, 0.0),
        point(&mut sketch, 10.0, 0.0),
    );
    let under = line(&mut sketch, a, b);
    constrain(&mut sketch, Constraint::Fix(under));
    let tangent = sketch.tangent(s, under).unwrap();
    constrain(&mut sketch, tangent);
    let solved = settle(&sketch);
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    assert_eq!(analysis.freedom, 4);
    assert!(at(&solved, fit[0]).y.abs() < 1e-8);
    let (way, _) = solved.heading(s, fit[0]).unwrap();
    assert!(along(way, DVec2::X, true), "{way}");
}

#[test]
fn a_spline_tangent_to_a_circle_touches_it_at_its_end() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, -5.0);
    let round = circle(&mut sketch, center, 5.0);
    constrain(&mut sketch, Constraint::Fix(round));
    let (s, fit, _) = spline(&mut sketch, &[(0.5, 0.3), (10.0, 5.0), (20.0, 0.0)], &[]);
    let tangent = sketch.tangent(round, s).unwrap();
    constrain(&mut sketch, tangent);
    let solved = settle(&sketch);
    assert!(analyse(&solved).solved);
    let end = at(&solved, fit[0]);
    assert!((end.distance(DVec2::new(0.0, -5.0)) - 5.0).abs() < 1e-9);
    let (way, _) = solved.heading(s, fit[0]).unwrap();
    assert!(way.dot(end - DVec2::new(0.0, -5.0)).abs() < 1e-8 * way.length() * 5.0);
}

#[test]
fn a_smooth_join_to_an_arc_curves_as_the_arc_does() {
    // An arc a quarter turn round (0, -10) from (10, -10) to (0, 0),
    // fixed, and a spline on from its end.
    let mut sketch = Sketch::default();
    let [center, start, end] =
        [(0.0, -10.0), (10.0, -10.0), (0.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let quarter = arc(&mut sketch, center, start, end);
    constrain(&mut sketch, Constraint::Fix(quarter));
    let fit = vec![
        end,
        point(&mut sketch, -10.0, -2.0),
        point(&mut sketch, -20.0, -8.0),
    ];
    let (s, _, _) = spline_through(&mut sketch, fit, &[(0, (-3.0, 0.5))]);
    assert_eq!(analyse(&sketch).freedom, 6);
    let smooth = sketch.smooth(quarter, s).unwrap();
    assert!(matches!(
        smooth,
        Constraint::Smooth {
            side: Side::Positive,
            ..
        }
    ));
    constrain(&mut sketch, smooth);
    let solved = settle(&sketch);
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    assert_eq!(analysis.freedom, 4);
    let (way, curving) = solved.heading(s, end).unwrap();
    let (arc_way, arc_curving) = solved.heading(quarter, end).unwrap();
    assert!(along(way, arc_way, true), "{way} {arc_way}");
    assert!(
        (curving - arc_curving).abs() < 1e-8,
        "{curving} {arc_curving}"
    );
    assert!((arc_curving - 0.1).abs() < 1e-12);
}

#[test]
fn a_smooth_join_of_two_splines_curves_alike() {
    let mut sketch = Sketch::default();
    let (first, fit, _) = spline(
        &mut sketch,
        &[(-20.0, 0.0), (-10.0, 3.0), (0.0, 0.0)],
        &[(2, (3.0, -1.0))],
    );
    constrain(&mut sketch, Constraint::Fix(first));
    let joint = fit[2];
    let rest = vec![
        joint,
        point(&mut sketch, 10.0, -4.0),
        point(&mut sketch, 20.0, 1.0),
    ];
    let (second, _, _) = spline_through(&mut sketch, rest, &[(0, (3.0, -2.0))]);
    let smooth = sketch.smooth(first, second).unwrap();
    constrain(&mut sketch, smooth);
    let solved = settle(&sketch);
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    // The second's two fit points and tip, less two.
    assert_eq!(analysis.freedom, 4);
    let (way, curving) = solved.heading(first, joint).unwrap();
    let (next, next_curving) = solved.heading(second, joint).unwrap();
    assert!(along(way, next, true), "{way} {next}");
    assert!(
        (curving - next_curving).abs() < 1e-8,
        "{curving} {next_curving}"
    );
    // The first didn't move: it's fixed.
    assert_eq!(at(&solved, fit[1]), DVec2::new(-10.0, 3.0));

    // Made again the same way, it's redundant.
    let mut again = solved.clone();
    let twice = solved.smooth(second, first).unwrap();
    let id = constrain(&mut again, twice);
    assert!(analyse(&again).redundant.contains(&id));
}

#[test]
fn a_tangent_with_a_spline_can_t_turn_round() {
    // A line from the spline's start runs back over it: the tangent,
    // made running on, never reaches the way back.
    let mut sketch = Sketch::default();
    let (s, fit, _) = spline(&mut sketch, &[(0.0, 0.0), (10.0, 1.0), (20.0, 0.0)], &[]);
    let far = point(&mut sketch, -10.0, 0.0);
    let joined = line(&mut sketch, far, fit[0]);
    constrain(&mut sketch, Constraint::Fix(joined));
    constrain(&mut sketch, Constraint::Fix(fit[2]));
    let tangent = sketch.tangent(joined, s).unwrap();
    constrain(&mut sketch, tangent);
    let solved = settle(&sketch);
    let goal = Goal::Drag {
        points: vec![(fit[1], DVec2::new(-8.0, 0.5))],
        radii: Vec::new(),
    };
    if let Ok(dragged) = solve(&solved, &goal, &Budget::default()) {
        let (way, _) = dragged.sketch.heading(s, fit[0]).unwrap();
        assert!(way.x > 0.0, "{way}");
    }
}

#[test]
fn a_smooth_join_goes_with_the_handle_it_needs() {
    let mut sketch = Sketch::default();
    let far = point(&mut sketch, -10.0, 0.0);
    let start = point(&mut sketch, 0.0, 0.0);
    let joined = line(&mut sketch, far, start);
    let fit = vec![
        start,
        point(&mut sketch, 10.0, 5.0),
        point(&mut sketch, 20.0, 0.0),
    ];
    let (s, _, tips) = spline_through(&mut sketch, fit, &[(0, (3.0, 0.5))]);
    let smooth = sketch.smooth(joined, s).unwrap();
    let id = constrain(&mut sketch, smooth);
    let tangent = sketch.tangent(joined, s).unwrap();
    let kept = constrain(&mut sketch, tangent);
    sketch.delete(&[tips[0]]);
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    assert!(sketch.constraint(id).is_none());
    assert!(sketch.constraint(kept).is_some());
    // Its end, the line's, gone, the tangent goes too.
    sketch.delete(&[start]);
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    assert!(sketch.constraints.is_empty());
}
