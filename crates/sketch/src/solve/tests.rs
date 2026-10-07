#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use glam::DVec2;

use super::*;
use crate::testing::{at, circle, constrain, dimension, line, point, quadrilateral};
use crate::{Constraint, Curve, Measure, Selectable, Side, Spline};

fn settle(sketch: &Sketch) -> Result<Solution, Failure> {
    solve(sketch, &Goal::Settle, &Budget::default())
}

fn drag(sketch: &Sketch, id: Id, to: DVec2) -> Result<Solution, Failure> {
    let goal = Goal::Drag {
        points: vec![(id, to)],
        radii: Vec::new(),
    };
    solve(sketch, &goal, &Budget::default())
}

fn close(a: DVec2, b: DVec2) -> bool {
    a.abs_diff_eq(b, 1e-7)
}

#[test]
fn a_rectangle_loses_its_freedom_constraint_by_constraint() {
    let (mut sketch, corners, [bottom, right, top, left]) = quadrilateral();
    let fixed = analyse(&sketch);
    assert_eq!(fixed.freedom, 8);
    assert!(fixed.fixed.is_empty() && fixed.redundant.is_empty() && fixed.solved);

    let steps = [
        (Constraint::Horizontal(bottom), 7),
        (Constraint::Horizontal(top), 6),
        (Constraint::Vertical(left), 5),
        (Constraint::Vertical(right), 4),
        (Constraint::Fix(corners[0]), 2),
        (Constraint::Equal(bottom, left), 1),
    ];
    let mut pinned = DVec2::NAN;
    for (constraint, freedom) in steps {
        constrain(&mut sketch, constraint.clone());
        // Unsolved, it still counts the same.
        assert_eq!(analyse(&sketch).freedom, freedom, "{constraint:?}");
        sketch = settle(&sketch).unwrap().sketch;
        if let Constraint::Fix(corner) = constraint {
            pinned = at(&sketch, corner);
        }
        let analysis = analyse(&sketch);
        assert_eq!(analysis.freedom, freedom, "{constraint:?}");
        assert!(analysis.solved && analysis.redundant.is_empty());
    }
    let analysis = analyse(&sketch);
    assert_eq!(analysis.fixed, BTreeSet::from([corners[0]]));
    // A square, its first corner where it was fixed.
    let [a, b, c, d] = corners.map(|id| at(&sketch, id));
    assert_eq!(a, pinned);
    assert!((b.y - a.y).abs() < 1e-9 && (c.x - b.x).abs() < 1e-9);
    assert!(((b - a).length() - (d - a).length()).abs() < 1e-9);
    assert!(close(c, b + d - a));

    // The last freedom, the size, taken by a fixed point the right side
    // lines up with.
    let marker = point(&mut sketch, 8.0, -3.0);
    constrain(&mut sketch, Constraint::Fix(marker));
    constrain(&mut sketch, Constraint::VerticalPoints(corners[1], marker));
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert_eq!(analysis.freedom, 0);
    assert!(analysis.redundant.is_empty());
    let mut everything: BTreeSet<Id> = solved.points.iter().map(|point| point.id).collect();
    everything.extend(solved.curves.iter().map(|entry| entry.id));
    assert_eq!(analysis.fixed, everything);
    let side = 8.0 - pinned.x;
    assert!(close(at(&solved, corners[2]), pinned + DVec2::splat(side)));
}

/// A triangle made equilateral, its first corner and side's length
/// fixed by a fixed circle about that corner which its second corner is
/// on, its base horizontal.
fn triangle() -> (Sketch, [Id; 3]) {
    let mut sketch = Sketch::default();
    let corners = [(0.0, 0.0), (9.0, 1.0), (4.0, 7.0)].map(|(x, y)| point(&mut sketch, x, y));
    let [ab, bc, ca] = [0, 1, 2].map(|i| line(&mut sketch, corners[i], corners[(i + 1) % 3]));
    let around = circle(&mut sketch, corners[0], 10.0);
    for constraint in [
        Constraint::Fix(around),
        Constraint::PointOnCurve {
            point: corners[1],
            curve: around,
        },
        Constraint::Horizontal(ab),
        Constraint::Equal(ab, bc),
        Constraint::Equal(bc, ca),
    ] {
        constrain(&mut sketch, constraint);
    }
    (sketch, corners)
}

#[test]
fn a_triangle_is_fully_constrained() {
    let (sketch, corners) = triangle();
    let solution = settle(&sketch).unwrap();
    assert!(solution.iterations > 0);
    let solved = solution.sketch;
    let [a, b, c] = corners.map(|id| at(&solved, id));
    assert_eq!(a, DVec2::ZERO);
    assert!(close(b, DVec2::new(10.0, 0.0)), "{b}");
    // Above the base, where it was drawn, not its mirror image below.
    assert!(close(c, DVec2::new(5.0, 75f64.sqrt())), "{c}");

    let analysis = analyse(&solved);
    assert_eq!(analysis.freedom, 0);
    assert!(analysis.solved && analysis.redundant.is_empty());
    assert_eq!(
        analysis.fixed.len(),
        solved.points.len() + solved.curves.len()
    );
}

#[test]
fn a_solved_sketch_takes_no_step() {
    let (sketch, _) = triangle();
    let solved = settle(&sketch).unwrap().sketch;
    let again = settle(&solved).unwrap();
    assert_eq!(again.iterations, 0);
    assert_eq!(again.sketch, solved);
    // A drag leaving everything where it is, likewise.
    let corner = solved.points[2].id;
    let still = drag(&solved, corner, at(&solved, corner)).unwrap();
    assert_eq!((still.iterations, still.sketch), (0, solved));
}

#[test]
fn redundant_constraints_are_named() {
    let (mut sketch, _, [bottom, right, top, left]) = quadrilateral();
    let horizontal = [bottom, top].map(|line| constrain(&mut sketch, Constraint::Horizontal(line)));
    for line in [left, right] {
        constrain(&mut sketch, Constraint::Vertical(line));
    }
    let solved = settle(&sketch).unwrap().sketch;
    assert!(analyse(&solved).redundant.is_empty());

    // Parallel follows from the two horizontals: all three are named.
    let mut parallel = solved.clone();
    let id = constrain(&mut parallel, Constraint::Parallel(bottom, top));
    let analysis = analyse(&settle(&parallel).unwrap().sketch);
    assert!(analysis.solved);
    assert_eq!(
        analysis.redundant,
        BTreeSet::from([horizontal[0], horizontal[1], id])
    );
    assert_eq!(analysis.freedom, 4);

    // So does fixing a horizontal line, with its horizontal.
    let mut fixed = solved.clone();
    let fix = constrain(&mut fixed, Constraint::Fix(bottom));
    let analysis = analyse(&fixed);
    assert_eq!(analysis.redundant, BTreeSet::from([horizontal[0], fix]));
    assert_eq!(analysis.freedom, 1);
}

#[test]
fn conflicts_name_the_constraints_in_them() {
    let mut sketch = Sketch::default();
    // Two fixed circles apart, and a point asked to be on both.
    let centers = [(0.0, 0.0), (20.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let circles = centers.map(|center| circle(&mut sketch, center, 5.0));
    for circle in circles {
        constrain(&mut sketch, Constraint::Fix(circle));
    }
    let p = point(&mut sketch, 10.0, 3.0);
    let on =
        circles.map(|curve| constrain(&mut sketch, Constraint::PointOnCurve { point: p, curve }));
    // Something else, solved and not part of it.
    let (a, b) = (point(&mut sketch, 0.0, 30.0), point(&mut sketch, 5.0, 31.0));
    let other = line(&mut sketch, a, b);
    let horizontal = constrain(&mut sketch, Constraint::Horizontal(other));

    let Err(Failure::NotConverged { involved }) = settle(&sketch) else {
        panic!("solved a conflict");
    };
    assert_eq!(involved, BTreeSet::from(on));
    assert!(!involved.contains(&horizontal));

    // Between fixed things alone, the constraint is named at once.
    let mut fixed = Sketch::default();
    let (a, b) = (point(&mut fixed, 0.0, 0.0), point(&mut fixed, 5.0, 1.0));
    let both = line(&mut fixed, a, b);
    constrain(&mut fixed, Constraint::Fix(both));
    let horizontal = constrain(&mut fixed, Constraint::Horizontal(both));
    assert_eq!(
        settle(&fixed),
        Err(Failure::NotConverged {
            involved: BTreeSet::from([horizontal])
        })
    );
    let analysis = analyse(&fixed);
    assert!(!analysis.solved);
    assert!(analysis.redundant.contains(&horizontal));
}

#[test]
fn a_drag_the_constraints_forbid_stops_at_the_nearest_place() {
    // A point on a fixed circle, dragged far outside it.
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let around = circle(&mut sketch, center, 5.0);
    constrain(&mut sketch, Constraint::Fix(around));
    let p = point(&mut sketch, 5.0, 0.0);
    constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: around,
        },
    );
    let target = DVec2::new(20.0, 15.0);
    let dragged = drag(&sketch, p, target).unwrap().sketch;
    assert!(
        close(at(&dragged, p), target.normalize() * 5.0),
        "{}",
        at(&dragged, p)
    );

    // A line fixed at one end and horizontal: its other end slides along.
    let mut sketch = Sketch::default();
    let (a, b) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 10.0, 0.0));
    let flat = line(&mut sketch, a, b);
    constrain(&mut sketch, Constraint::Fix(a));
    constrain(&mut sketch, Constraint::Horizontal(flat));
    let dragged = drag(&sketch, b, DVec2::new(12.0, 5.0)).unwrap().sketch;
    assert!(close(at(&dragged, b), DVec2::new(12.0, 0.0)));

    // A fixed point doesn't move at all.
    let still = drag(&sketch, a, DVec2::new(3.0, 3.0)).unwrap();
    assert_eq!(still.sketch, sketch);
}

#[test]
fn a_drag_moves_the_rest_as_little_as_it_can() {
    // A free horizontal line: dragging one end up takes the other along,
    // and leaves the dragged end at its target.
    let mut sketch = Sketch::default();
    let (a, b) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 10.0, 0.0));
    let flat = line(&mut sketch, a, b);
    constrain(&mut sketch, Constraint::Horizontal(flat));
    let target = DVec2::new(11.0, 4.0);
    let dragged = drag(&sketch, b, target).unwrap().sketch;
    assert!(
        at(&dragged, b).abs_diff_eq(target, 1e-4),
        "{}",
        at(&dragged, b)
    );
    // The other end moved up only, nowhere else.
    assert!(at(&dragged, a).abs_diff_eq(DVec2::new(0.0, 4.0), 1e-4));
    assert!((at(&dragged, b).y - at(&dragged, a).y).abs() < 1e-9);

    // A point nothing holds goes where it's dragged; a target that isn't
    // a number goes nowhere.
    let lone = point(&mut sketch, 5.0, 5.0);
    let dragged = drag(&sketch, lone, DVec2::new(-7.0, 2.0)).unwrap();
    assert_eq!(at(&dragged.sketch, lone), DVec2::new(-7.0, 2.0));
    assert_eq!(dragged.iterations, 0);
    assert_eq!(
        drag(&sketch, lone, DVec2::new(f64::NAN, 0.0)),
        Err(Failure::Degenerate {
            involved: BTreeSet::from([lone])
        })
    );

    // A circle's radius dragged, with a point on it following.
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let around = circle(&mut sketch, center, 5.0);
    let p = point(&mut sketch, 0.0, 5.0);
    constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: around,
        },
    );
    let goal = Goal::Drag {
        points: Vec::new(),
        radii: vec![(around, 8.0)],
    };
    let dragged = solve(&sketch, &goal, &Budget::default()).unwrap().sketch;
    let (middle, radius) = dragged.round(around).unwrap();
    assert!((radius - 8.0).abs() < 1e-4);
    assert!(((at(&dragged, p) - middle).length() - radius).abs() < 1e-9);
}

/// A circle tangent to a line, on its left: above it, the line running
/// along +x.
fn tangent() -> (Sketch, Id, Id, Id) {
    let mut sketch = Sketch::default();
    let (a, b) = (
        point(&mut sketch, -10.0, 0.0),
        point(&mut sketch, 10.0, 0.0),
    );
    let flat = line(&mut sketch, a, b);
    let center = point(&mut sketch, 0.0, 3.0);
    let round = circle(&mut sketch, center, 2.5);
    let tangent = sketch.tangent(flat, round).unwrap();
    assert_eq!(
        tangent,
        Constraint::Tangent {
            a: flat,
            b: round,
            side: Side::Positive,
            at: None,
        }
    );
    constrain(&mut sketch, tangent);
    let sketch = settle(&sketch).unwrap().sketch;
    (sketch, flat, round, center)
}

#[test]
fn a_tangency_never_reaches_its_mirror_image() {
    let (sketch, flat, _, center) = tangent();
    let side = |sketch: &Sketch| {
        let Curve::Line { start, end } = sketch.curve(flat).unwrap().curve else {
            unreachable!()
        };
        let (start, end) = (at(sketch, start), at(sketch, end));
        (end - start).perp_dot(at(sketch, center) - start)
    };
    assert!(side(&sketch) > 0.0);
    // Dragged further up, the circle grows or the line follows, and the
    // centre stays on the line's left.
    let dragged = drag(&sketch, center, DVec2::new(1.0, 6.0)).unwrap().sketch;
    assert!(side(&dragged) > 0.0);
    // Dragged below the line, only a radius below zero would do, which is
    // no solution: the drag fails, as it does with the line fixed. An
    // unsigned tangency would have taken a radius of three below the
    // line.
    let below = DVec2::new(0.0, -3.0);
    assert!(matches!(
        drag(&sketch, center, below),
        Err(Failure::Degenerate { .. })
    ));
    let mut fixed = sketch.clone();
    constrain(&mut fixed, Constraint::Fix(flat));
    assert!(matches!(
        drag(&fixed, center, below),
        Err(Failure::Degenerate { .. })
    ));
}

#[test]
fn tangent_circles_touch_outside_or_the_smaller_inside() {
    let mut sketch = Sketch::default();
    let centers = [(0.0, 0.0), (7.0, 0.0), (1.0, 0.5)].map(|(x, y)| point(&mut sketch, x, y));
    let [big, apart, inner] = [(centers[0], 5.0), (centers[1], 1.5), (centers[2], 2.0)]
        .map(|(center, radius)| circle(&mut sketch, center, radius));
    let outside = sketch.tangent(big, apart).unwrap();
    assert_eq!(
        outside,
        Constraint::Tangent {
            a: big,
            b: apart,
            side: Side::Positive,
            at: None,
        }
    );
    // Asked the other way round, the larger comes first.
    let inside = sketch.tangent(inner, big).unwrap();
    assert_eq!(
        inside,
        Constraint::Tangent {
            a: big,
            b: inner,
            side: Side::Negative,
            at: None,
        }
    );
    constrain(&mut sketch, outside);
    constrain(&mut sketch, inside);
    let solved = settle(&sketch).unwrap().sketch;
    let round = |id| solved.round(id).unwrap();
    let ((c0, r0), (c1, r1), (c2, r2)) = (round(big), round(apart), round(inner));
    assert!((c0.distance(c1) - (r0 + r1)).abs() < 1e-9);
    assert!((c0.distance(c2) - (r0 - r2)).abs() < 1e-9);
}

#[test]
fn solving_leaves_other_components_alone() {
    let (triangle, corners) = triangle();
    let mut sketch = settle(&triangle).unwrap().sketch;
    // A second shape, unsolved: a line asked to be vertical.
    let (a, b) = (point(&mut sketch, 50.0, 0.0), point(&mut sketch, 52.0, 9.0));
    let leaning = line(&mut sketch, a, b);
    constrain(&mut sketch, Constraint::Vertical(leaning));
    let unchanged = |solved: &Sketch| {
        corners
            .iter()
            .all(|&corner| at(solved, corner) == at(&sketch, corner))
    };

    // Settling solves the line alone: the triangle is exactly as it was.
    let settled = settle(&sketch).unwrap();
    assert!(unchanged(&settled.sketch));
    let vertical = |solved: &Sketch| (at(solved, a).x - at(solved, b).x).abs() < 1e-8;
    assert!(vertical(&settled.sketch));

    // Dragging the triangle leaves the unsolved line alone, and dragging
    // the line leaves the triangle.
    let (_, (x, y)) = (a, (at(&sketch, a), at(&sketch, b)));
    let dragged = drag(&sketch, corners[2], DVec2::new(3.0, 3.0))
        .unwrap()
        .sketch;
    assert_eq!((at(&dragged, a), at(&dragged, b)), (x, y));
    let dragged = drag(&sketch, a, DVec2::new(49.0, 1.0)).unwrap().sketch;
    assert!(unchanged(&dragged));
    assert!(vertical(&dragged));
}

#[test]
fn arcs_keep_their_ends_at_one_radius() {
    let mut sketch = Sketch::default();
    let [center, start, end] =
        [(0.0, 0.0), (5.0, 0.0), (0.0, 6.0)].map(|(x, y)| point(&mut sketch, x, y));
    let arc = sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    let analysis = analyse(&sketch);
    assert_eq!(analysis.freedom, 5);
    assert!(!analysis.solved);
    let solved = settle(&sketch).unwrap().sketch;
    let (middle, radius) = solved.round(arc).unwrap();
    assert!((at(&solved, end).distance(middle) - radius).abs() < 1e-9);
    assert!(analyse(&solved).solved);
}

#[test]
fn a_fixed_arc_is_fixed_without_restating_its_radius() {
    let mut sketch = Sketch::default();
    let [center, start, end] =
        [(0.0, 0.0), (5.0, 0.0), (0.0, 5.0)].map(|(x, y)| point(&mut sketch, x, y));
    let arc = sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    constrain(&mut sketch, Constraint::Fix(arc));
    let analysis = analyse(&sketch);
    assert_eq!(analysis.freedom, 0);
    assert!(analysis.redundant.is_empty(), "{analysis:?}");
    assert_eq!(analysis.fixed, BTreeSet::from([center, start, end, arc]));
    // Fixing its end as well restates it.
    let again = constrain(&mut sketch, Constraint::Fix(end));
    assert!(analyse(&sketch).redundant.contains(&again));
}

#[test]
fn the_budget_bounds_the_solve() {
    let (sketch, _) = triangle();
    let none = Budget {
        iterations: 0,
        ..Budget::default()
    };
    assert!(matches!(
        solve(&sketch, &Goal::Settle, &none),
        Err(Failure::NotConverged { .. })
    ));
    let expired = || true;
    let late = Budget {
        expired: &expired,
        ..Budget::default()
    };
    assert_eq!(
        solve(&sketch, &Goal::Settle, &late),
        Err(Failure::OutOfTime)
    );
    // Nothing to do costs nothing, however late.
    let solved = settle(&sketch).unwrap().sketch;
    assert!(solve(&solved, &Goal::Settle, &late).is_ok());
}

#[test]
fn every_constraint_holds_once_solved() {
    let mut sketch = Sketch::default();
    let p = |sketch: &mut Sketch, x, y| point(sketch, x, y);
    let [a, b, c, d] =
        [(0.0, 0.0), (10.0, 1.0), (1.0, 5.0), (8.0, 9.0)].map(|(x, y)| p(&mut sketch, x, y));
    let first = line(&mut sketch, a, b);
    let second = line(&mut sketch, c, d);
    let [e, f] = [(3.0, -4.0), (4.0, 6.0)].map(|(x, y)| p(&mut sketch, x, y));
    let third = line(&mut sketch, e, f);
    let middle = p(&mut sketch, 4.0, 1.5);
    let [m, n] = [(2.0, 3.0), (6.0, 2.0)].map(|(x, y)| p(&mut sketch, x, y));
    let hub = p(&mut sketch, 20.0, 20.0);
    let round = circle(&mut sketch, hub, 3.0);
    let [arc_center, arc_start, arc_end] =
        [(21.0, 19.0), (25.0, 19.0), (21.0, 23.0)].map(|(x, y)| p(&mut sketch, x, y));
    let arc = sketch
        .add_curve(
            Curve::Arc {
                center: arc_center,
                start: arc_start,
                end: arc_end,
            },
            false,
        )
        .unwrap();
    let on = p(&mut sketch, 24.0, 21.0);
    let lone = p(&mut sketch, 30.0, 30.0);
    for constraint in [
        Constraint::Parallel(first, second),
        Constraint::Perpendicular(first, third),
        Constraint::Midpoint {
            point: middle,
            line: first,
        },
        Constraint::Symmetric {
            a: m,
            b: n,
            about: third,
        },
        Constraint::Equal(first, third),
        Constraint::Concentric(round, arc),
        Constraint::Equal(round, arc),
        Constraint::PointOnCurve {
            point: on,
            curve: round,
        },
        Constraint::PointOnCurve {
            point: m,
            curve: second,
        },
        Constraint::Concentric(lone, round),
        Constraint::HorizontalPoints(on, arc_start),
    ] {
        constrain(&mut sketch, constraint);
    }
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(analysis.solved, "{analysis:?}");
    assert!(analysis.redundant.is_empty(), "{analysis:?}");
    // Checked by hand for a few.
    let [a, b, e, f, middle, m, n] = [a, b, e, f, middle, m, n].map(|id| at(&solved, id));
    assert!(close(middle, (a + b) / 2.0));
    assert!((b - a).dot(f - e).abs() < 1e-7);
    let (axis, mirror) = ((f - e).normalize(), (m + n) / 2.0 - e);
    assert!(axis.perp_dot(mirror).abs() < 1e-7 && axis.dot(n - m).abs() < 1e-7);
    assert!(close(at(&solved, lone), at(&solved, hub)));
}

#[test]
fn driving_dimensions_fully_constrain_a_triangle() {
    let mut sketch = Sketch::default();
    let corners = [(0.0, 0.0), (9.0, 1.0), (4.0, 7.0)].map(|(x, y)| point(&mut sketch, x, y));
    let [ab, bc, ca] = [0, 1, 2].map(|i| line(&mut sketch, corners[i], corners[(i + 1) % 3]));
    constrain(&mut sketch, Constraint::Fix(corners[0]));
    constrain(&mut sketch, Constraint::Horizontal(ab));
    let mut freedom = 3;
    for (measure, text) in [
        (Measure::Length(ab), "10"),
        (Measure::Length(bc), "8"),
        (Measure::Length(ca), "6"),
    ] {
        dimension(&mut sketch, measure, text);
        freedom -= 1;
        assert_eq!(analyse(&sketch).freedom, freedom);
    }
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    assert_eq!(analysis.freedom, 0);
    let [a, b, c] = corners.map(|id| at(&solved, id));
    assert!(close(b, DVec2::new(10.0, 0.0)), "{b}");
    // A right angle at c, above the base where it was drawn: c is 3.6
    // along it and 4.8 up.
    assert!(close(c, DVec2::new(3.6, 4.8)), "{c}");
    assert_eq!(a, DVec2::ZERO);
    for entry in &solved.dimensions {
        let dimension = &entry.dimension;
        let measured = solved.measure(&dimension.measure, dimension.side).unwrap();
        assert!((measured - dimension.value.value).abs() < 1e-9);
    }
}

#[test]
fn driving_dimensions_fully_constrain_a_rectangle() {
    let (mut sketch, corners, [bottom, right, top, left]) = quadrilateral();
    for constraint in [
        Constraint::Horizontal(bottom),
        Constraint::Horizontal(top),
        Constraint::Vertical(left),
        Constraint::Vertical(right),
    ] {
        constrain(&mut sketch, constraint);
    }
    // Placed from the origin by its distances from two lines through it.
    let origin = point(&mut sketch, 0.0, 0.0);
    let x = point(&mut sketch, 1.0, 0.0);
    let y = point(&mut sketch, 0.0, 1.0);
    let x_axis = line(&mut sketch, origin, x);
    let y_axis = line(&mut sketch, origin, y);
    constrain(&mut sketch, Constraint::Fix(x_axis));
    constrain(&mut sketch, Constraint::Fix(y));
    // Off the axes, on the sides of them its dimensions keep it.
    sketch.point_mut(corners[0]).unwrap().at = DVec2::new(1.5, 1.0);
    for (measure, text) in [
        (Measure::HorizontalDistance(corners[0], corners[1]), "20"),
        (Measure::Distance(top, bottom), "(20 / 2) mm"),
        (Measure::Distance(corners[0], y_axis), "2"),
        (Measure::Distance(x_axis, bottom), "3"),
    ] {
        dimension(&mut sketch, measure, text);
    }
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(
        analysis.solved && analysis.redundant.is_empty(),
        "{analysis:?}"
    );
    assert_eq!(analysis.freedom, 0);
    let corners = corners.map(|id| at(&solved, id));
    let expected = [(2.0, 3.0), (22.0, 3.0), (22.0, 13.0), (2.0, 13.0)];
    for (corner, (x, y)) in corners.into_iter().zip(expected) {
        assert!(close(corner, DVec2::new(x, y)), "{corners:?}");
    }
}

#[test]
fn reference_dimensions_only_measure() {
    let (mut sketch, _, [bottom, right, top, left]) = quadrilateral();
    let free = analyse(&sketch).freedom;
    let length = dimension(&mut sketch, Measure::Length(bottom), "3");
    let angle = dimension(&mut sketch, Measure::Angle(bottom, right), "30");
    for id in [length, angle] {
        sketch.dimension_mut(id).unwrap().dimension.driving = false;
    }
    let analysis = analyse(&sketch);
    assert_eq!(analysis.freedom, free);
    assert!(analysis.solved);
    assert_eq!(settle(&sketch).unwrap().iterations, 0);

    // It measures what's there, not its value.
    constrain(&mut sketch, Constraint::Horizontal(bottom));
    constrain(&mut sketch, Constraint::Perpendicular(bottom, right));
    let solved = settle(&sketch).unwrap().sketch;
    let measured = |id| {
        let dimension = &solved.dimension(id).unwrap().dimension;
        solved.measure(&dimension.measure, dimension.side).unwrap()
    };
    let (start, end) = solved.line(bottom).unwrap();
    assert!((measured(length) - start.distance(end)).abs() < 1e-12);
    assert!((measured(angle) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    let _ = (top, left);
}

#[test]
fn every_dimension_holds_once_solved() {
    let mut sketch = Sketch::default();
    let p = |sketch: &mut Sketch, x, y| point(sketch, x, y);
    let [a, b, c, d] =
        [(0.0, 0.0), (10.0, 1.0), (1.0, 5.0), (8.0, 9.0)].map(|(x, y)| p(&mut sketch, x, y));
    let first = line(&mut sketch, a, b);
    let second = line(&mut sketch, c, d);
    let lone = p(&mut sketch, 4.0, -3.0);
    let hub = p(&mut sketch, 20.0, 20.0);
    let round = circle(&mut sketch, hub, 3.0);
    let [arc_center, arc_start, arc_end] =
        [(21.0, 5.0), (25.0, 5.0), (21.0, 9.0)].map(|(x, y)| p(&mut sketch, x, y));
    let arc = sketch
        .add_curve(
            Curve::Arc {
                center: arc_center,
                start: arc_start,
                end: arc_end,
            },
            false,
        )
        .unwrap();
    for (measure, text) in [
        (Measure::Distance(a, lone), "6"),
        (Measure::Distance(lone, first), "2.5"),
        (Measure::Distance(second, first), "4"),
        (Measure::HorizontalDistance(hub, arc_center), "3"),
        (Measure::VerticalDistance(hub, arc_center), "12"),
        (Measure::Length(second), "7"),
        (Measure::Angle(first, second), "20"),
        (Measure::Radius(round), "1.5"),
        (Measure::Diameter(arc), "5"),
    ] {
        dimension(&mut sketch, measure, text);
    }
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(
        analysis.solved && analysis.redundant.is_empty(),
        "{analysis:?}"
    );
    for entry in &solved.dimensions {
        let dimension = &entry.dimension;
        let measured = solved.measure(&dimension.measure, dimension.side).unwrap();
        assert!(
            (measured - dimension.value.value).abs() < 1e-9,
            "{:?}: {measured}",
            dimension.measure
        );
        // On the side it was made on.
        assert_eq!(solved.side(&dimension.measure), dimension.side);
    }
}

/// A circle's edge dimensioned from a point outside and one inside, a
/// line, a circle apart and one inside it: each holds, on its side.
#[test]
fn edge_distances_hold_once_solved() {
    let mut sketch = Sketch::default();
    let p = |sketch: &mut Sketch, x, y| point(sketch, x, y);
    let hub = p(&mut sketch, 0.0, 0.0);
    let big = circle(&mut sketch, hub, 10.0);
    let outside = p(&mut sketch, 15.0, 1.0);
    let inside = p(&mut sketch, 2.0, 1.0);
    let [a, b] = [(-20.0, -14.0), (20.0, -13.0)].map(|(x, y)| p(&mut sketch, x, y));
    let below = line(&mut sketch, a, b);
    let far_hub = p(&mut sketch, 0.0, 25.0);
    let apart = circle(&mut sketch, far_hub, 3.0);
    let near_hub = p(&mut sketch, -4.0, 0.0);
    let held = circle(&mut sketch, near_hub, 2.0);
    for (measure, text) in [
        (Measure::EdgeDistance(big, outside), "4"),
        (Measure::EdgeDistance(big, inside), "6"),
        (Measure::EdgeDistance(big, below), "5"),
        (Measure::EdgeDistance(big, apart), "8"),
        (Measure::EdgeDistance(big, held), "3"),
    ] {
        dimension(&mut sketch, measure, text);
    }
    let sides: Vec<_> = (sketch.dimensions.iter())
        .map(|entry| entry.dimension.side)
        .collect();
    use crate::Side::{Negative, Positive};
    assert_eq!(sides, [Positive, Negative, Positive, Positive, Negative]);
    let solved = settle(&sketch).unwrap().sketch;
    for entry in &solved.dimensions {
        let dimension = &entry.dimension;
        let measured = solved.measure(&dimension.measure, dimension.side).unwrap();
        assert!(
            (measured - dimension.value.value).abs() < 1e-9,
            "{:?}: {measured}",
            dimension.measure
        );
        assert_eq!(solved.side(&dimension.measure), dimension.side);
    }
    // The gap runs from the edge: a point outside sits 4 past it.
    let (edge, to) = (solved.edge_ends(big, outside, Positive)).unwrap();
    let (center, radius) = solved.round(big).unwrap();
    assert!((edge.distance(center) - radius).abs() < 1e-9);
    assert!((edge.distance(to) - 4.0).abs() < 1e-9);
}

/// An edge distance takes no spline, nor the circle's own points.
#[test]
fn an_edge_distance_takes_no_spline_nor_its_own_points() {
    let mut sketch = Sketch::default();
    let hub = point(&mut sketch, 0.0, 0.0);
    let round = circle(&mut sketch, hub, 2.0);
    let tips = std::collections::HashSet::new();
    assert!(!Measure::EdgeDistance(round, hub).fits(&sketch, &tips));
    assert!(!Measure::EdgeDistance(round, round).fits(&sketch, &tips));
    let lone = point(&mut sketch, 5.0, 0.0);
    assert!(Measure::EdgeDistance(round, lone).fits(&sketch, &tips));
}

#[test]
fn a_distance_from_a_line_never_reaches_its_mirror_image() {
    let mut sketch = Sketch::default();
    let [a, b] = [(0.0, 0.0), (10.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let base = line(&mut sketch, a, b);
    constrain(&mut sketch, Constraint::Fix(base));
    let p = point(&mut sketch, 5.0, 1.0);
    constrain(&mut sketch, Constraint::VerticalPoints(p, a));
    let id = dimension(&mut sketch, Measure::Distance(p, base), "1");
    // Its equation is the signed distance less the value, which is above
    // zero: four below the line isn't a solution.
    sketch.dimension_mut(id).unwrap().dimension.value.value = 4.0;
    let solved = settle(&sketch).unwrap().sketch;
    assert!(close(at(&solved, p), DVec2::new(0.0, 4.0)));
    let mut below = solved.clone();
    below.point_mut(p).unwrap().at.y = -4.0;
    assert!(!analyse(&below).solved);
}

#[test]
fn a_tangent_where_curves_meet_at_their_ends_is_an_equation_of_its_own() {
    // A line and an arc from its end, and a second arc on from the first's
    // other end, all a little off tangent.
    let mut sketch = Sketch::default();
    let a = point(&mut sketch, 0.0, 0.0);
    let b = point(&mut sketch, 8.0, 0.0);
    let straight = line(&mut sketch, a, b);
    let center = point(&mut sketch, 8.3, 4.0);
    let top = point(&mut sketch, 8.0, 8.0);
    let first = sketch
        .add_curve(
            Curve::Arc {
                center,
                start: b,
                end: top,
            },
            false,
        )
        .unwrap();
    let far = point(&mut sketch, 7.7, 12.0);
    let second_center = point(&mut sketch, 8.0, 10.2);
    let second = sketch
        .add_curve(
            Curve::Arc {
                center: second_center,
                start: far,
                end: top,
            },
            false,
        )
        .unwrap();
    let before = analyse(&sketch).freedom;
    for tangent in [
        sketch.tangent(straight, first).unwrap(),
        sketch.tangent(first, second).unwrap(),
    ] {
        constrain(&mut sketch, tangent);
    }
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    // Each takes one freedom, restating nothing.
    assert!(analysis.redundant.is_empty() && analysis.solved);
    assert_eq!(analysis.freedom, before - 2);
    // The first arc's centre is straight above the line's end, and the
    // second's on the line through the first's and their shared end.
    let (c1, _) = solved.round(first).unwrap();
    let (c2, _) = solved.round(second).unwrap();
    let (end, shared) = (at(&solved, b), at(&solved, top));
    let (start, _) = solved.line(straight).unwrap();
    assert!((c1 - end).dot(end - start).abs() < 1e-7);
    assert!((c1 - shared).perp_dot(c2 - shared).abs() < 1e-7);
}

/// An L of lines from (0, 0) to (20, 1) and from (-1, 10) back to it,
/// its corner off a right angle, filleted and chamfered as the edits make
/// them, on corners of its own: `(filleted, chamfered)`.
fn cornered() -> (Sketch, Sketch) {
    let mut sketch = Sketch::default();
    let corner = point(&mut sketch, 0.0, 0.0);
    let (along, up) = (
        point(&mut sketch, 20.0, 1.0),
        point(&mut sketch, -1.0, 10.0),
    );
    let a = line(&mut sketch, corner, along);
    let b = line(&mut sketch, up, corner);
    let design = crate::testing::DESIGN;
    let length = |text| crate::testing::value(text, &Measure::Length(a));
    let fillet = crate::SketchEdit::Fillet {
        at: corner,
        lines: [a, b],
        radius: length("3"),
    };
    let chamfer = crate::SketchEdit::Chamfer {
        at: corner,
        lines: [a, b],
        setback: crate::Setback::Equal(length("2")),
    };
    (
        fillet.apply(&sketch, &design).unwrap(),
        chamfer.apply(&sketch, &design).unwrap(),
    )
}

#[test]
fn a_corner_s_equations_hold_and_their_gradients_match_finite_differences() {
    let (filleted, chamfered) = cornered();
    for (sketch, count) in [(filleted, 4), (chamfered, 3)] {
        let corner = sketch.curves.last().unwrap().id;
        let system = system::System::new(&sketch, system::Fixing::Constants);
        let equations: Vec<_> = system
            .equations
            .iter()
            .filter(|equation| equation.source == corner)
            .collect();
        // A fillet's radius equation, as an arc's, comes first.
        let own = equations.len() - usize::from(count == 4);
        assert_eq!(own, count, "{:?}", sketch.curves.last());
        let x = &system.values;
        for equation in equations {
            let residual = &equation.residual;
            // Made where they hold.
            assert!(residual.value(x).abs() < 1e-9, "{residual:?}");
            let mut gradient = vec![0.0; x.len()];
            residual.gradient(x, |var, derivative| gradient[var] += derivative);
            for var in 0..x.len() {
                let h = 1e-6;
                let at = |offset: f64| {
                    let mut x = x.clone();
                    x[var] += offset;
                    residual.value(&x)
                };
                let expected = (at(h) - at(-h)) / (2.0 * h);
                assert!(
                    (gradient[var] - expected).abs() <= 1e-6 * expected.abs().max(1.0),
                    "{residual:?} by {var}: {} against {expected}",
                    gradient[var]
                );
            }
        }
    }
}

#[test]
fn a_fillet_never_turns_inside_out() {
    let (mut filleted, _) = cornered();
    let entry = filleted.curves.last().unwrap().clone();
    let Curve::Arc { center, start, .. } = entry.curve else {
        panic!("a fillet is an arc");
    };
    // The centre across its start's line, outside the corner: the
    // equation holding it at right angles there is no number, or far from
    // zero.
    let (from, to) = (at(&filleted, start), at(&filleted, center));
    filleted.point_mut(center).unwrap().at = 2.0 * from - to;
    let system = system::System::new(&filleted, system::Fixing::Constants);
    let inside_out = system
        .equations
        .iter()
        .filter(|equation| equation.source == entry.id)
        .map(|equation| equation.residual.value(&system.values))
        .any(|value| !value.is_finite() || value.abs() > 1e6);
    assert!(inside_out);
    // Dragging the centre across the line it's tangent to doesn't flip it.
    let (mut filleted, _) = cornered();
    for id in [0, 1, 2].map(|index| filleted.points[index].id) {
        constrain(&mut filleted, Constraint::Fix(id));
    }
    let dragged = drag(&filleted, center, DVec2::new(2.0, -3.0));
    if let Ok(solution) = dragged {
        let at = at(&solution.sketch, center);
        assert!(at.y > 0.0, "{at}");
    }
}

/// The spline through fit points on a wave, open or `closed`, with a
/// handle at each of the fit points `handles` pointing on along it:
/// the sketch, its fit points, its tips by the order of `handles`, and
/// its id.
fn wave(closed: bool, handles: &[usize]) -> (Sketch, Vec<Id>, Vec<Id>, Id) {
    let mut sketch = Sketch::default();
    let places = [
        (0.0, 0.0),
        (10.0, 6.0),
        (20.0, 4.0),
        (30.0, -3.0),
        (40.0, 2.0),
    ];
    let fit: Vec<Id> = places
        .iter()
        .map(|&(x, y)| point(&mut sketch, x, y))
        .collect();
    let mut spline = Spline::through(fit.clone(), closed);
    let tips: Vec<Id> = handles
        .iter()
        .map(|&i| {
            let (x, y) = places[i];
            let tip = point(&mut sketch, x + 3.0, y + 1.0);
            spline
                .handles
                .push(crate::testing::handle(&mut sketch, fit[i], tip));
            tip
        })
        .collect();
    let id = sketch.add_curve(Curve::Spline(spline), false).unwrap();
    (sketch, fit, tips, id)
}

/// How far `place` is from the spline `id` of `sketch`.
fn off_spline(sketch: &Sketch, id: Id, place: DVec2) -> f64 {
    sketch.nearest_on(id, place).unwrap().distance(place)
}

#[test]
fn a_point_on_a_spline_stays_on_it_as_it_s_dragged() {
    let (mut sketch, fit, _, spline) = wave(false, &[]);
    let p = point(&mut sketch, 14.0, 7.5);
    constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: spline,
        },
    );
    let solved = settle(&sketch).unwrap().sketch;
    assert!(off_spline(&solved, spline, at(&solved, p)) < 1e-8);
    // A fit point dragged, the point goes with the curve.
    let mut current = solved.clone();
    for step in 1..=10 {
        let to = at(&solved, fit[2]) + DVec2::new(0.0, step as f64);
        current = drag(&current, fit[2], to).unwrap().sketch;
        // As near as the drag's weight holds it.
        assert!(at(&current, fit[2]).distance(to) < 1e-5, "{step}");
        assert!(
            off_spline(&current, spline, at(&current, p)) < 1e-8,
            "{step}"
        );
    }
    // The point dragged along a fixed spline slides along it, and past
    // its end stops there.
    let mut fixed = solved.clone();
    constrain(&mut fixed, Constraint::Fix(spline));
    let slid = drag(&fixed, p, DVec2::new(25.0, 3.0)).unwrap().sketch;
    assert!(off_spline(&slid, spline, at(&slid, p)) < 1e-8);
    assert!(at(&slid, p).x > 20.0);
    let past = drag(&fixed, p, DVec2::new(50.0, 2.0)).unwrap().sketch;
    assert!(close(at(&past, p), at(&past, fit[4])), "{}", at(&past, p));
}

#[test]
fn a_spline_s_freedom_is_its_points_and_a_point_on_it_slides() {
    let (mut sketch, _, tips, spline) = wave(false, &[0, 2]);
    // Five fit points and two tips.
    assert_eq!(analyse(&sketch).freedom, 14);
    let p = point(&mut sketch, 14.0, 7.5);
    let on = constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: spline,
        },
    );
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    assert_eq!(analysis.freedom, 15);

    // Fixed, the spline is, with its tips, and the point on it slides.
    let mut fixed = solved.clone();
    constrain(&mut fixed, Constraint::Fix(spline));
    let analysis = analyse(&fixed);
    assert_eq!(analysis.freedom, 1);
    assert!(analysis.fixed.contains(&spline) && analysis.fixed.contains(&tips[1]));
    assert!(!analysis.fixed.contains(&p));
    // Held at a distance from a fixed point too, it's fixed.
    let from = point(&mut fixed, 20.0, -10.0);
    constrain(&mut fixed, Constraint::Fix(from));
    let distance = at(&fixed, p).distance(at(&fixed, from));
    dimension(
        &mut fixed,
        Measure::Distance(from, p),
        &format!("{distance}"),
    );
    let analysis = analyse(&fixed);
    assert_eq!(analysis.freedom, 0);
    assert!(analysis.fixed.contains(&p) && analysis.redundant.is_empty());

    // A second point where the first is, on it too, restates it.
    let mut twice = solved.clone();
    let q = point(&mut twice, at(&solved, p).x, at(&solved, p).y);
    let same = constrain(&mut twice, Constraint::Coincident(p, q));
    let again = constrain(
        &mut twice,
        Constraint::PointOnCurve {
            point: q,
            curve: spline,
        },
    );
    let redundant = analyse(&twice).redundant;
    assert!(
        [on, same, again].iter().all(|id| redundant.contains(id)),
        "{redundant:?}"
    );
}

#[test]
fn a_point_on_a_fixed_spline_off_it_conflicts() {
    let (mut sketch, _, _, spline) = wave(false, &[]);
    constrain(&mut sketch, Constraint::Fix(spline));
    let p = point(&mut sketch, 20.0, 20.0);
    constrain(&mut sketch, Constraint::Fix(p));
    let on = constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: spline,
        },
    );
    assert_eq!(
        settle(&sketch),
        Err(Failure::NotConverged {
            involved: BTreeSet::from([on])
        })
    );
    let analysis = analyse(&sketch);
    assert!(!analysis.solved && analysis.redundant.contains(&on));
}

#[test]
fn a_point_on_a_closed_spline_goes_round_past_where_it_starts() {
    let (mut sketch, fit, _, spline) = wave(true, &[1]);
    constrain(&mut sketch, Constraint::Fix(spline));
    let made = sketch.spline(spline).unwrap();
    let shape = sketch.spline_shape(made).unwrap();
    let start = shape.point(0.9) + DVec2::splat(0.1);
    let p = point(&mut sketch, start.x, start.y);
    constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: spline,
        },
    );
    let mut current = settle(&sketch).unwrap().sketch;
    // Across its first point, where its parameter starts again, and back.
    let there = (0..=20).map(|i| 0.9 + 0.01 * i as f64);
    for t in there.clone().chain(there.rev()) {
        let place = shape.point(t);
        current = drag(&current, p, place)
            .unwrap_or_else(|e| panic!("{t}: {e:?}"))
            .sketch;
        // Where it's put, as near as the drag's weight holds it.
        assert!(
            at(&current, p).distance(place) < 1e-5,
            "{t}: {}",
            at(&current, p)
        );
        assert!(off_spline(&current, spline, at(&current, p)) < 1e-8, "{t}");
    }
    assert_eq!(at(&current, fit[0]), DVec2::new(0.0, 0.0));
}

#[test]
fn a_point_on_a_spline_by_control_points_stays_on_it() {
    let (mut sketch, _, _, spline) = wave(false, &[]);
    sketch
        .convert_spline(spline, crate::SplineKind::Control)
        .unwrap();
    let made = sketch.spline(spline).unwrap();
    let control = made.points.clone();
    let p = point(&mut sketch, 22.0, 5.0);
    constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: spline,
        },
    );
    let solved = settle(&sketch).unwrap();
    // Its knots don't move with its points: one round.
    let mut current = solved.sketch;
    assert!(off_spline(&current, spline, at(&current, p)) < 1e-8);
    for step in 1..=5 {
        let to = at(&current, control[3]) + DVec2::new(0.5, step as f64 * 0.5);
        current = drag(&current, control[3], to).unwrap().sketch;
        assert!(
            off_spline(&current, spline, at(&current, p)) < 1e-8,
            "{step}"
        );
    }
    assert_eq!(analyse(&current).freedom, 2 * control.len() + 1);
}

#[test]
fn a_handle_s_length_and_angle_are_dimensioned() {
    let (mut sketch, fit, tips, spline) = wave(false, &[0]);
    let length = Measure::Distance(fit[0], tips[0]);
    let angle = Measure::Angle(Id::X_AXIS, tips[0]);
    dimension(&mut sketch, length.clone(), "5");
    dimension(&mut sketch, angle.clone(), "30 deg");
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    // Five fit points and a tip, less the two.
    assert_eq!(analysis.freedom, 10);
    let handle = at(&solved, tips[0]) - at(&solved, fit[0]);
    assert!(
        close(handle, 5.0 * DVec2::from_angle(30f64.to_radians())),
        "{handle}"
    );
    let measured = solved.measure(&angle, Side::Positive).unwrap();
    assert!((measured - 30f64.to_radians()).abs() < 1e-9);
    // The spline leaves its first point the way the handle points.
    let (way, _) = solved.heading(spline, fit[0]).unwrap();
    assert!(way.normalize().perp_dot(handle.normalize()).abs() < 1e-12);
}

#[test]
fn a_handle_s_length_is_its_whole_line_s() {
    let (mut sketch, fit, tips, _) = wave(false, &[0]);
    dimension(&mut sketch, Measure::Length(tips[0]), "8");
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    let half = at(&solved, tips[0]).distance(at(&solved, fit[0]));
    assert!((half - 4.0).abs() < 1e-9, "{half}");
    let measured = solved.measure(&Measure::Length(tips[0]), Side::Positive);
    assert!((measured.unwrap() - 8.0).abs() < 1e-9);
}

#[test]
fn handles_are_held_as_lines_are() {
    let (mut sketch, fit, tips, _) = wave(false, &[0, 2, 4]);
    let (a, b) = (point(&mut sketch, 0.0, -9.0), point(&mut sketch, 4.0, -6.0));
    let slanted = line(&mut sketch, a, b);
    constrain(&mut sketch, Constraint::Horizontal(tips[0]));
    constrain(&mut sketch, Constraint::Parallel(slanted, tips[1]));
    constrain(&mut sketch, Constraint::Perpendicular(tips[1], tips[2]));
    let line = Selectable::HandleLine(tips[0]);
    let named = sketch.selectable_name(line).unwrap();
    assert!(named.starts_with("Handle of Spline"), "{named}");
    let (_, first) = sketch.handle(tips[0]).unwrap();
    let end = sketch.name(first.end).unwrap();
    assert!(end.starts_with("End 2 of Handle 1 of Spline"), "{end}");
    let tip = sketch.name(tips[1]).unwrap();
    assert!(tip.starts_with("End 1 of Handle 2 of Spline"), "{tip}");
    assert!(sketch.selectable(line) && !sketch.selectable(Selectable::HandleLine(fit[1])));
    let solved = settle(&sketch).unwrap().sketch;
    let analysis = analyse(&solved);
    assert!(analysis.solved && analysis.redundant.is_empty());
    let handle = |i: usize| at(&solved, tips[i]) - at(&solved, fit[2 * i]);
    let along = at(&solved, b) - at(&solved, a);
    assert!(handle(0).y.abs() < 1e-9, "{}", handle(0));
    assert!(handle(1).normalize().perp_dot(along.normalize()).abs() < 1e-9);
    assert!(handle(1).normalize().dot(handle(2).normalize()).abs() < 1e-9);
}

#[test]
fn a_point_on_a_spline_slides_past_its_knots_in_one_solve() {
    // By control points, a row of them, fixed; a point on it held above a
    // fixed point far along, so its parameter passes several knots.
    let mut sketch = Sketch::default();
    let control: Vec<Id> = (0..10)
        .map(|i| {
            point(
                &mut sketch,
                10.0 * i as f64,
                if i % 2 == 0 { 0.0 } else { 4.0 },
            )
        })
        .collect();
    let places: Vec<DVec2> = control.iter().map(|&id| at(&sketch, id)).collect();
    let knots = crate::control_knots(&places, false);
    let spline = Spline {
        kind: crate::SplineKind::Control,
        points: control,
        closed: false,
        handles: Vec::new(),
        knots,
    };
    let spline = sketch.add_curve(Curve::Spline(spline), false).unwrap();
    constrain(&mut sketch, Constraint::Fix(spline));
    let p = point(&mut sketch, 5.0, 1.0);
    constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: spline,
        },
    );
    let q = point(&mut sketch, 71.0, -10.0);
    constrain(&mut sketch, Constraint::Fix(q));
    constrain(&mut sketch, Constraint::VerticalPoints(p, q));
    let solved = settle(&sketch).unwrap().sketch;
    assert!((at(&solved, p).x - 71.0).abs() < 1e-9);
    assert!(off_spline(&solved, spline, at(&solved, p)) < 1e-8);
}
