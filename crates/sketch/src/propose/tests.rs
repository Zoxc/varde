use std::collections::BTreeSet;

use glam::DVec2;

use super::*;

use crate::testing::{
    self, DESIGN, at, circle, constrain, dimension, line, point, propose_it, value,
};
use crate::{Add, Constraint, Curve, Dimension, SketchError};

const MAX: f64 = DESIGN.max;

/// [`testing::quadrilateral`] with its bottom and top horizontal, solved:
/// its corners, its lines and the horizontals.
fn quadrilateral() -> (Sketch, [Id; 4], [Id; 4], [Id; 2]) {
    let (mut sketch, corners, lines) = testing::quadrilateral();
    let horizontal =
        [lines[0], lines[2]].map(|line| constrain(&mut sketch, Constraint::Horizontal(line)));
    let solved = propose_it(&sketch, &SketchEdit::Delete(Vec::new()))
        .unwrap()
        .sketch;
    (solved, corners, lines, horizontal)
}

#[test]
fn an_edit_is_solved_and_analysed() {
    let (sketch, _, [.., left], _) = quadrilateral();
    let edit = SketchEdit::constrain(&sketch, vec![Constraint::Vertical(left)]);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let vertical = accepted.sketch.constraints.last().unwrap();
    assert_eq!(vertical.constraint, Constraint::Vertical(left));
    assert_eq!(accepted.analysis, analyse(&accepted.sketch));
    assert_eq!(accepted.analysis.freedom, 5);
    assert!(accepted.analysis.solved);
    let Curve::Line { start, end } = accepted.sketch.curve(left).unwrap().curve else {
        unreachable!()
    };
    assert!((at(&accepted.sketch, start).x - at(&accepted.sketch, end).x).abs() < 1e-9);
}

#[test]
fn auto_constraints_are_kept_when_independent_and_dropped_when_redundant() {
    let (sketch, _, [bottom, right, top, left], horizontal) = quadrilateral();
    // Parallel restates the horizontals; vertical is new.
    let mut add = Add::new(&sketch);
    add.auto.push(Constraint::Parallel(bottom, top));
    add.auto.push(Constraint::Vertical(left));
    let accepted = propose_it(&sketch, &SketchEdit::Add(add)).unwrap();
    let constraints: Vec<_> = accepted
        .sketch
        .constraints
        .iter()
        .map(|entry| entry.constraint.clone())
        .collect();
    assert_eq!(
        constraints,
        [
            Constraint::Horizontal(bottom),
            Constraint::Horizontal(top),
            Constraint::Vertical(left),
        ]
    );
    assert!(accepted.analysis.redundant.is_empty());
    assert_eq!(accepted.analysis.freedom, 5);

    // Redundant with a constraint of the same edit, the auto one goes and
    // the edit's own stays.
    let mut add = Add::new(&sketch);
    add.constraints.push(Constraint::Vertical(right));
    add.auto.push(Constraint::Vertical(right));
    let accepted = propose_it(&sketch, &SketchEdit::Add(add)).unwrap();
    assert_eq!(accepted.sketch.constraints.len(), 3);
    // The dropped one's id isn't handed out again.
    assert_eq!(accepted.sketch.next_id, sketch.next_id + 2);

    // An edit redundant in itself is refused however many are auto.
    let mut add = Add::new(&sketch);
    add.constraints.push(Constraint::Parallel(bottom, top));
    add.auto.push(Constraint::Vertical(left));
    let rejected = propose_it(&sketch, &SketchEdit::Add(add)).unwrap_err();
    let parallel = Id(sketch.next_id);
    assert_eq!(
        rejected,
        Rejected::Redundant {
            involved: BTreeSet::from([horizontal[0], horizontal[1], parallel])
        }
    );
    assert_eq!(rejected.involved().len(), 3);
}

#[test]
fn a_new_point_snapped_onto_a_line_keeps_the_snap() {
    // A lone point added on a free line, with its snap: kept, as it's
    // what holds the point there.
    let (sketch, _, [bottom, ..], _) = quadrilateral();
    let mut add = Add::new(&sketch);
    let on = add.point(DVec2::new(4.0, 0.0)).unwrap();
    add.auto.push(Constraint::PointOnCurve {
        point: on,
        curve: bottom,
    });
    let accepted = propose_it(&sketch, &SketchEdit::Add(add)).unwrap();
    assert_eq!(accepted.sketch.constraints.len(), 3);
    assert_eq!(accepted.analysis.freedom, 6 + 1);
}

#[test]
fn a_conflicting_constraint_is_refused_naming_the_conflict() {
    let mut sketch = Sketch::default();
    // Two fixed circles apart, a point on the first, and something else.
    let centers = [(0.0, 0.0), (20.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let circles = centers.map(|center| circle(&mut sketch, center, 5.0));
    for circle in circles {
        constrain(&mut sketch, Constraint::Fix(circle));
    }
    let p = point(&mut sketch, 5.0, 0.0);
    let first = constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: circles[0],
        },
    );
    let (a, b) = (point(&mut sketch, 0.0, 30.0), point(&mut sketch, 5.0, 30.0));
    let other = line(&mut sketch, a, b);
    let horizontal = constrain(&mut sketch, Constraint::Horizontal(other));
    assert!(propose_it(&sketch, &SketchEdit::Delete(Vec::new())).is_ok());

    // The point on the second too can't be.
    let edit = SketchEdit::constrain(
        &sketch,
        vec![Constraint::PointOnCurve {
            point: p,
            curve: circles[1],
        }],
    );
    let second = Id(sketch.next_id);
    let rejected = propose_it(&sketch, &edit).unwrap_err();
    assert!(matches!(
        rejected,
        Rejected::Unsolved(Failure::NotConverged { .. })
    ));
    assert_eq!(rejected.involved(), &BTreeSet::from([first, second]));
    assert!(!rejected.involved().contains(&horizontal));

    // As an auto constraint, it's dropped instead.
    let mut add = Add::new(&sketch);
    add.auto.push(Constraint::PointOnCurve {
        point: p,
        curve: circles[1],
    });
    let accepted = propose_it(&sketch, &SketchEdit::Add(add)).unwrap();
    assert_eq!(accepted.sketch.constraints, sketch.constraints);
}

#[test]
fn a_move_keeps_the_constraints_holding() {
    let (sketch, corners, _, _) = quadrilateral();
    // Moving a corner of the bottom up takes the other along, and the rest
    // stays as far as it can.
    let target = DVec2::new(10.0, 3.0);
    let edit = SketchEdit::Move {
        points: vec![(corners[1], target)],
        radii: Vec::new(),
    };
    let moved = propose_it(&sketch, &edit).unwrap().sketch;
    assert!(at(&moved, corners[1]).abs_diff_eq(target, 1e-4));
    assert!((at(&moved, corners[0]).y - at(&moved, corners[1]).y).abs() < 1e-9);
    assert_eq!(at(&moved, corners[2]), at(&sketch, corners[2]));

    // With the other end fixed, the moved one slides along instead.
    let mut fixed = sketch.clone();
    constrain(&mut fixed, Constraint::Fix(corners[0]));
    let moved = propose_it(&fixed, &edit).unwrap().sketch;
    assert_eq!(at(&moved, corners[0]), at(&sketch, corners[0]));
    assert!((at(&moved, corners[1]).y - at(&sketch, corners[0]).y).abs() < 1e-9);
    assert!((at(&moved, corners[1]).x - 10.0).abs() < 1e-4);
    // Moving what's fixed moves nothing.
    let still = SketchEdit::Move {
        points: vec![(corners[0], DVec2::new(-4.0, -4.0))],
        radii: Vec::new(),
    };
    assert_eq!(propose_it(&fixed, &still).unwrap().sketch, fixed);
}

#[test]
fn running_out_of_budget_refuses_rather_than_half_solves() {
    // An edit needing a step, with an auto constraint to tell apart.
    let (sketch, _, [bottom, _, top, left], _) = quadrilateral();
    let mut add = Add::new(&sketch);
    add.constraints.push(Constraint::Vertical(left));
    add.auto.push(Constraint::Parallel(bottom, top));
    let edit = SketchEdit::Add(add);
    let none = Budget {
        iterations: 0,
        ..Budget::default()
    };
    assert!(matches!(
        propose(&sketch, &edit, &DESIGN, &none),
        Err(Rejected::Unsolved(Failure::NotConverged { .. }))
    ));
    let expired = || true;
    let late = Budget {
        expired: &expired,
        ..Budget::default()
    };
    assert_eq!(
        propose(&sketch, &edit, &DESIGN, &late),
        Err(Rejected::Unsolved(Failure::OutOfTime))
    );
}

#[test]
fn a_solution_past_the_limit_is_refused() {
    // A short line near the limit made as long as a fixed one: its end
    // goes past the limit, the nearer of the two places it could be.
    let mut sketch = Sketch::default();
    let (a, b) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 10.0, 0.0));
    let long = line(&mut sketch, a, b);
    constrain(&mut sketch, Constraint::Fix(long));
    let (start, p) = (
        point(&mut sketch, MAX - 5.0, 0.0),
        point(&mut sketch, MAX - 4.0, 0.0),
    );
    let short = line(&mut sketch, start, p);
    for constraint in [Constraint::Fix(start), Constraint::Horizontal(short)] {
        constrain(&mut sketch, constraint);
    }
    let edit = SketchEdit::constrain(&sketch, vec![Constraint::Equal(long, short)]);
    assert!(matches!(
        propose_it(&sketch, &edit),
        Err(Rejected::Edit(EditError::Sketch(SketchError::Coordinate { id, .. }))) if id == p
    ));
    // An edit that doesn't apply is refused as it is.
    let edit = SketchEdit::Move {
        points: vec![(p, DVec2::new(2.0 * MAX, 0.0))],
        radii: Vec::new(),
    };
    assert!(matches!(
        propose_it(&sketch, &edit),
        Err(Rejected::Edit(EditError::Sketch(
            SketchError::Coordinate { .. }
        )))
    ));
}

#[test]
fn a_drag_steps_from_one_solution_to_the_next() {
    // A point on a fixed circle, dragged around and far outside it.
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
    let mut session = DragSession::new(sketch.clone(), DESIGN);
    let budget = Budget::default();
    let target = DVec2::new(0.0, 20.0);
    let reached = at(
        session
            .step(vec![(p, target)], Vec::new(), &budget)
            .unwrap(),
        p,
    );
    // The nearest place it can be.
    assert!(reached.abs_diff_eq(DVec2::new(0.0, 5.0), 1e-7), "{reached}");
    let reached = at(
        session
            .step(vec![(p, DVec2::new(-30.0, 0.1))], Vec::new(), &budget)
            .unwrap(),
        p,
    );
    assert!((reached.length() - 5.0).abs() < 1e-9);
    assert!(reached.x < -4.9);
    let last = session.sketch().clone();

    // A target that isn't a number, past the limit, or out of time moves
    // nothing, keeping the last solution.
    let lone = point(&mut sketch, 1.0, 1.0);
    let mut free = DragSession::new(sketch, DESIGN);
    for (to, budget) in [
        (DVec2::new(f64::NAN, 0.0), Budget::default()),
        (DVec2::new(2.0 * MAX, 0.0), Budget::default()),
    ] {
        assert!(free.step(vec![(lone, to)], Vec::new(), &budget).is_err());
        assert_eq!(at(free.sketch(), lone), DVec2::new(1.0, 1.0));
    }
    let expired = || true;
    let late = Budget {
        expired: &expired,
        ..Budget::default()
    };
    assert_eq!(
        session.step(vec![(p, DVec2::new(3.0, 4.5))], Vec::new(), &late),
        Err(Rejected::Unsolved(Failure::OutOfTime))
    );
    assert_eq!(session.sketch(), &last);
}

#[test]
fn a_drag_never_flips_a_tangency() {
    // A circle tangent to a line above it, dragged below it: no solution,
    // so the drag stays where it last converged.
    let mut sketch = Sketch::default();
    let (a, b) = (
        point(&mut sketch, -10.0, 0.0),
        point(&mut sketch, 10.0, 0.0),
    );
    let flat = line(&mut sketch, a, b);
    let center = point(&mut sketch, 0.0, 3.0);
    let round = circle(&mut sketch, center, 3.0);
    let tangent = sketch.tangent(flat, round).unwrap();
    constrain(&mut sketch, tangent);
    constrain(&mut sketch, Constraint::Fix(flat));
    let mut session = DragSession::new(sketch, DESIGN);
    let budget = Budget::default();
    let up = session
        .step(vec![(center, DVec2::new(1.0, 4.0))], Vec::new(), &budget)
        .unwrap();
    assert!(at(up, center).y > 0.0);
    let last = session.sketch().clone();
    assert!(matches!(
        session.step(vec![(center, DVec2::new(1.0, -3.0))], Vec::new(), &budget),
        Err(Rejected::Unsolved(Failure::Degenerate { .. }))
    ));
    assert_eq!(session.sketch(), &last);
}

/// A triangle with its first corner fixed, its base horizontal, and its
/// sides' lengths driving 10, 8 and 6, solved: its corners and sides.
fn dimensioned() -> (Sketch, [Id; 3], [Id; 3]) {
    let mut sketch = Sketch::default();
    let corners = [(0.0, 0.0), (9.0, 1.0), (4.0, 7.0)].map(|(x, y)| point(&mut sketch, x, y));
    let sides = [0, 1, 2].map(|i| line(&mut sketch, corners[i], corners[(i + 1) % 3]));
    constrain(&mut sketch, Constraint::Fix(corners[0]));
    constrain(&mut sketch, Constraint::Horizontal(sides[0]));
    for (side, text) in sides.into_iter().zip(["10", "8", "6"]) {
        dimension(&mut sketch, Measure::Length(side), text);
    }
    let sketch = propose_it(&sketch, &SketchEdit::Delete(Vec::new()))
        .unwrap()
        .sketch;
    (sketch, corners, sides)
}

/// Adding a dimension of `measure`, `text` millimetres or degrees.
fn add_dimension(sketch: &Sketch, measure: Measure, text: &str, driving: bool) -> SketchEdit {
    let value = value(text, &measure);
    let side = sketch.side(&measure);
    let mut add = Add::new(sketch);
    add.dimensions.push(Dimension {
        measure,
        value,
        driving,
        label: DVec2::ZERO,
        side,
    });
    SketchEdit::Add(add)
}

#[test]
fn an_over_constraining_driving_dimension_is_refused_as_one_to_add_as_a_reference() {
    let (sketch, _, [_, right, left]) = dimensioned();
    let new = Id(sketch.next_id);
    // The right angle the lengths make, restated or contradicted.
    for angle in ["90", "80"] {
        let edit = add_dimension(&sketch, Measure::Angle(right, left), angle, true);
        let Err(Rejected::Driving {
            dimensions,
            involved,
        }) = propose_it(&sketch, &edit)
        else {
            panic!("{angle}° accepted");
        };
        assert_eq!(dimensions, BTreeSet::from([new]));
        assert!(involved.contains(&new));
        assert!(
            Rejected::Driving {
                dimensions,
                involved
            }
            .to_string()
            .contains("reference")
        );
    }
    // As a reference, it's accepted and measures the right angle.
    let edit = add_dimension(&sketch, Measure::Angle(right, left), "80", false);
    let accepted = propose_it(&sketch, &edit).unwrap();
    let added = &accepted.sketch.dimension(new).unwrap().dimension;
    let measured = accepted.sketch.measure(&added.measure, added.side).unwrap();
    assert!((measured - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert_eq!(accepted.analysis.freedom, 0);

    // Made driving, it's refused the same way.
    let driving = SketchEdit::SetDriving {
        id: new,
        driving: true,
    };
    assert!(matches!(
        propose_it(&accepted.sketch, &driving),
        Err(Rejected::Driving { dimensions, .. }) if dimensions == BTreeSet::from([new])
    ));

    // A constraint restating the dimensions is refused as ever.
    let edit = SketchEdit::constrain(&sketch, vec![Constraint::Perpendicular(right, left)]);
    assert!(matches!(
        propose_it(&sketch, &edit),
        Err(Rejected::Redundant { .. })
    ));
    // As is a value the rest can't reach: no triangle has sides 10, 8, 1.
    let edit = SketchEdit::SetDimension {
        id: sketch.dimensions[2].id,
        value: value("1", &Measure::Length(left)),
    };
    assert!(matches!(
        propose_it(&sketch, &edit),
        Err(Rejected::Unsolved(Failure::NotConverged { .. }))
    ));
}

#[test]
fn a_dimension_set_moves_the_geometry_and_a_reference_s_moves_nothing() {
    let (sketch, corners, [base, ..]) = dimensioned();
    let id = sketch.dimensions[0].id;
    let edit = SketchEdit::SetDimension {
        id,
        value: value("12", &Measure::Length(base)),
    };
    let accepted = propose_it(&sketch, &edit).unwrap();
    assert!((at(&accepted.sketch, corners[1]) - DVec2::new(12.0, 0.0)).length() < 1e-9);
    assert_eq!(accepted.analysis.freedom, 0);

    let mut reference = sketch.clone();
    reference.dimensions[0].dimension.driving = false;
    let accepted = propose_it(&reference, &edit).unwrap();
    assert_eq!(accepted.sketch.points, reference.points);
    assert_eq!(accepted.sketch.dimensions[0].dimension.value.value, 12.0);
    assert_eq!(accepted.analysis.freedom, 1);
}

/// A four-bar linkage: ground from (0, 0) to (10, 0), fixed, a crank of 2
/// from its start at `angle` degrees, a coupler of 12 and a rocker of 5
/// from ground's end, the coupler's far end above the line from the
/// crank's end to ground's end: the crank's angle, the joint between the
/// coupler and the rocker, and which side of that line it's on.
fn linkage(angle: f64) -> (Sketch, Id, Id, impl Fn(&Sketch) -> f64) {
    let (crank, coupler, rocker) = (2.0, 12.0, 5.0);
    let ground_end = DVec2::new(10.0, 0.0);
    let crank_end = DVec2::from_angle(angle.to_radians()) * crank;
    let across = ground_end - crank_end;
    let along =
        (coupler * coupler - rocker * rocker + across.length_squared()) / (2.0 * across.length());
    let up = (coupler * coupler - along * along).sqrt();
    let joint = crank_end + across.normalize() * along + across.normalize().perp() * up;
    let mut sketch = Sketch::default();
    let [a, d, b, c] =
        [DVec2::ZERO, ground_end, crank_end, joint].map(|at| point(&mut sketch, at.x, at.y));
    let ground = line(&mut sketch, a, d);
    let links = [(a, b), (b, c), (d, c)].map(|(start, end)| line(&mut sketch, start, end));
    constrain(&mut sketch, Constraint::Fix(ground));
    for (link, length) in links.into_iter().zip([crank, coupler, rocker]) {
        dimension(&mut sketch, Measure::Length(link), &length.to_string());
    }
    let turn = dimension(
        &mut sketch,
        Measure::Angle(ground, links[0]),
        &angle.to_string(),
    );
    let side = move |sketch: &Sketch| {
        let (b, c, d) = (at(sketch, b), at(sketch, c), at(sketch, d));
        (d - b).perp_dot(c - b)
    };
    (sketch, turn, c, side)
}

#[test]
fn a_big_change_goes_in_steps_keeping_the_geometry_on_its_branch() {
    let (sketch, turn, joint, side) = linkage(60.0);
    assert!(side(&sketch) > 0.0);
    let measure = &sketch.dimension(turn).unwrap().dimension.measure;
    let value = value("300", measure);

    // Solved in one go, the joint flips to the other side.
    let mut jumped = sketch.clone();
    jumped.dimension_mut(turn).unwrap().dimension.value = value.clone();
    let jumped = solve(&jumped, &Goal::Settle, &Budget::default())
        .unwrap()
        .sketch;
    assert!(side(&jumped) < 0.0);

    // In steps it follows the crank round, as the linkage would move.
    let edit = SketchEdit::SetDimension { id: turn, value };
    let accepted = propose_it(&sketch, &edit).unwrap();
    assert!(side(&accepted.sketch) > 0.0);
    let (expected, _, _, _) = linkage(300.0);
    assert!((at(&accepted.sketch, joint) - at(&expected, joint)).length() < 1e-6);
    assert!(accepted.analysis.solved && accepted.analysis.freedom == 0);
}

#[test]
fn a_dimension_placed_at_another_value_goes_there_in_steps() {
    let (mut sketch, turn, joint, side) = linkage(60.0);
    let index = sketch.dimensions.iter().position(|entry| entry.id == turn);
    let mut placed = sketch.dimensions.remove(index.unwrap()).dimension;
    placed.value = value("300", &placed.measure);
    let mut add = Add::new(&sketch);
    add.dimensions.push(placed);
    let accepted = propose_it(&sketch, &SketchEdit::Add(add)).unwrap();
    assert!(side(&accepted.sketch) > 0.0);
    let (expected, _, _, _) = linkage(300.0);
    assert!((at(&accepted.sketch, joint) - at(&expected, joint)).length() < 1e-6);
    assert!(accepted.analysis.solved && accepted.analysis.freedom == 0);
}

#[test]
fn an_angle_turned_through_its_mirror_image_goes_in_steps() {
    let mut sketch = Sketch::default();
    let [o, x, e] = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0 * 10f64.to_radians().tan()),
    ]
    .map(|(x, y)| point(&mut sketch, x, y));
    let base = line(&mut sketch, o, x);
    let arm = line(&mut sketch, o, e);
    constrain(&mut sketch, Constraint::Fix(base));
    dimension(&mut sketch, Measure::Length(arm), "10");
    let turn = dimension(&mut sketch, Measure::Angle(base, arm), "10");
    let sketch = propose_it(&sketch, &SketchEdit::Delete(Vec::new()))
        .unwrap()
        .sketch;
    let value = value("190", &Measure::Angle(base, arm));
    // Half a turn away, the equation isn't a number.
    let mut jumped = sketch.clone();
    jumped.dimension_mut(turn).unwrap().dimension.value = value.clone();
    assert!(matches!(
        solve(&jumped, &Goal::Settle, &Budget::default()),
        Err(Failure::Degenerate { .. })
    ));
    let edit = SketchEdit::SetDimension { id: turn, value };
    let turned = propose_it(&sketch, &edit).unwrap().sketch;
    let expected = DVec2::from_angle(190f64.to_radians()) * 10.0;
    assert!((at(&turned, e) - expected).length() < 1e-9);
}

#[test]
fn continuation_takes_bounded_steps() {
    let length = Measure::Length(Id(0));
    let angle = Measure::Angle(Id(0), Id(1));
    assert_eq!(steps(&length, 10.0, 12.0), 1);
    assert_eq!(steps(&length, 10.0, 12.4), 1);
    assert_eq!(steps(&length, 10.0, 12.6), 2);
    assert_eq!(steps(&length, 12.6, 10.0), 2);
    assert_eq!(steps(&length, 1.0, 1e6), CONTINUATION_STEPS);
    assert_eq!(steps(&length, 1e-300, 1e300), CONTINUATION_STEPS);
    assert_eq!(steps(&length, 0.0, 1.0), 1);
    assert_eq!(steps(&length, f64::NAN, 1.0), 1);
    assert_eq!(steps(&angle, 0.1, 0.1 + 0.9 * ANGLE_STEP), 1);
    assert_eq!(steps(&angle, 0.1 + 1.1 * ANGLE_STEP, 0.1), 2);
    assert_eq!(steps(&angle, 0.1, 6.2), CONTINUATION_STEPS);
}
