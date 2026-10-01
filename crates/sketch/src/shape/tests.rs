#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::collections::BTreeSet;

use glam::DVec2;

use super::*;
use crate::testing::{
    self, DESIGN, arc, at, circle, constrain, dimension, geom, line, near, point, propose_it,
};
use crate::{Rejected, SketchEdit, analyse};

fn apply(sketch: &Sketch, edit: &SketchEdit) -> Sketch {
    edit.apply(sketch, &DESIGN).unwrap()
}

/// The ends of the line or arc `id`.
fn ends(sketch: &Sketch, id: Id) -> (Id, Id) {
    let [start, end] = sketch.curve(id).unwrap().curve.ends().unwrap();
    (start, end)
}

fn constraints(sketch: &Sketch) -> Vec<Constraint> {
    let listed = sketch.constraints.iter();
    listed.map(|entry| entry.constraint.clone()).collect()
}

/// A line from (0, 0) to (30, 0) crossed by vertical lines at x = 10 and
/// x = 20: the line, its ends, and the two crossing it.
fn crossed() -> (Sketch, Id, [Id; 2], [Id; 2]) {
    let mut sketch = Sketch::default();
    let (start, end) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 30.0, 0.0));
    let across = line(&mut sketch, start, end);
    let cutters = [10.0, 20.0].map(|x| {
        let (low, high) = (point(&mut sketch, x, -5.0), point(&mut sketch, x, 5.0));
        line(&mut sketch, low, high)
    });
    (sketch, across, [start, end], cutters)
}

#[test]
fn trimming_the_middle_of_a_line_splits_it_on_one_line() {
    let (sketch, across, [start, end], [left, right]) = crossed();
    let trim = SketchEdit::Trim {
        curve: across,
        near: DVec2::new(14.0, 0.3),
    };
    let piece = sketch.trim_piece(across, DVec2::new(14.0, 0.3)).unwrap();
    assert!(near(piece[0], DVec2::new(10.0, 0.0)) && near(piece[1], DVec2::new(20.0, 0.0)));

    let trimmed = apply(&sketch, &trim);
    // The part before keeps the id and its start; the part after is new,
    // keeping the end.
    let (kept_start, kept_end) = ends(&trimmed, across);
    assert_eq!(kept_start, start);
    assert!(near(at(&trimmed, kept_end), DVec2::new(10.0, 0.0)));
    let rest = trimmed.curves.last().unwrap();
    assert_eq!(rest.name(), "Line 4");
    let (rest_start, rest_end) = ends(&trimmed, rest.id);
    assert!(near(at(&trimmed, rest_start), DVec2::new(20.0, 0.0)));
    assert_eq!(rest_end, end);
    // Each new end on the line cutting it, and the rest on the first.
    assert_eq!(
        constraints(&trimmed),
        [
            Constraint::PointOnCurve {
                point: kept_end,
                curve: left
            },
            Constraint::PointOnCurve {
                point: rest_start,
                curve: right
            },
            Constraint::PointOnCurve {
                point: rest_start,
                curve: across
            },
            Constraint::PointOnCurve {
                point: end,
                curve: across
            },
        ]
    );

    let accepted = propose_it(&sketch, &trim).unwrap();
    assert_eq!(accepted.sketch.constraints.len(), 4);
    // Four points more, as many equations.
    assert_eq!(analyse(&sketch).freedom, 12);
    assert_eq!(accepted.analysis.freedom, 12);
}

#[test]
fn trimming_an_end_moves_it_to_the_crossing() {
    let (sketch, across, [start, end], [left, _]) = crossed();
    let trimmed = apply(
        &sketch,
        &SketchEdit::Trim {
            curve: across,
            near: DVec2::new(3.0, 0.0),
        },
    );
    let (new_start, kept_end) = ends(&trimmed, across);
    assert_eq!(kept_end, end);
    assert!(near(at(&trimmed, new_start), DVec2::new(10.0, 0.0)));
    // The start no curve is made from any more goes.
    assert!(trimmed.point(start).is_none());
    assert_eq!(trimmed.curves.len(), 3);
    assert_eq!(
        constraints(&trimmed),
        [Constraint::PointOnCurve {
            point: new_start,
            curve: left
        }]
    );
}

#[test]
fn trimming_a_circle_leaves_an_arc() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let round = circle(&mut sketch, center, 10.0);
    let (left, right) = (
        point(&mut sketch, -20.0, 0.0),
        point(&mut sketch, 20.0, 0.0),
    );
    let across = line(&mut sketch, left, right);
    let diameter = dimension(&mut sketch, Measure::Diameter(round), "20");
    let trim = SketchEdit::Trim {
        curve: round,
        near: DVec2::new(1.0, 9.0),
    };
    let trimmed = apply(&sketch, &trim);
    let entry = trimmed.curve(round).unwrap();
    // The circle's id, numbered as an arc, the bottom half left: from the
    // left crossing counter-clockwise to the right one.
    assert_eq!(entry.name(), "Arc 1");
    let Curve::Arc {
        center: arc_center,
        start,
        end,
    } = entry.curve
    else {
        panic!("not an arc: {entry:?}");
    };
    assert_eq!(arc_center, center);
    assert!(near(at(&trimmed, start), DVec2::new(-10.0, 0.0)));
    assert!(near(at(&trimmed, end), DVec2::new(10.0, 0.0)));
    // Its diameter still means something.
    assert!(trimmed.dimension(diameter).is_some());
    assert_eq!(
        constraints(&trimmed),
        [start, end].map(|point| Constraint::PointOnCurve {
            point,
            curve: across
        })
    );
    let accepted = propose_it(&sketch, &trim).unwrap();
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn trimming_a_circle_cut_once_or_a_curve_cut_nowhere_deletes_it() {
    // A line tangent to a circle cuts it once, which leaves no piece.
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let round = circle(&mut sketch, center, 5.0);
    let (a, b) = (
        point(&mut sketch, -10.0, 5.0),
        point(&mut sketch, 10.0, 5.0),
    );
    let tangent = line(&mut sketch, a, b);
    let touching = sketch.tangent(round, tangent).unwrap();
    constrain(&mut sketch, touching);
    let lone = {
        let (c, d) = (
            point(&mut sketch, 0.0, 20.0),
            point(&mut sketch, 10.0, 20.0),
        );
        line(&mut sketch, c, d)
    };
    let horizontal = constrain(&mut sketch, Constraint::Horizontal(lone));

    let trimmed = apply(
        &sketch,
        &SketchEdit::Trim {
            curve: round,
            near: DVec2::new(0.0, -5.0),
        },
    );
    assert!(trimmed.curve(round).is_none() && trimmed.point(center).is_none());
    assert!(trimmed.constraints.iter().all(|e| e.id == horizontal));
    let piece = sketch.trim_piece(lone, DVec2::new(5.0, 20.0)).unwrap();
    assert_eq!(piece, [DVec2::new(0.0, 20.0), DVec2::new(10.0, 20.0)]);
    let trimmed = apply(
        &trimmed,
        &SketchEdit::Trim {
            curve: lone,
            near: DVec2::new(5.0, 20.0),
        },
    );
    assert!(trimmed.curve(lone).is_none());
    assert_eq!(trimmed.points.len(), 2);
    assert!(trimmed.constraints.is_empty());
}

#[test]
fn a_spline_along_another_is_not_trimmed_by_some_of_its_crossings() {
    // Rings wobbling alike but for their phases, 0.017 apart past a whole
    // turn, cross at angles so shallow that finding every place runs out
    // of steps: trimmed between those found, a crossing missed would take
    // away more than the piece pointed at.
    let mut sketch = Sketch::default();
    let ring = |phase: f64| -> Vec<(f64, f64)> {
        (0..20)
            .map(|i| {
                let angle = f64::from(i) / 20.0 * std::f64::consts::TAU;
                let r = 10.0 * (1.0 + 0.2 * (7.0 * angle + phase).sin());
                (r * angle.cos(), r * angle.sin())
            })
            .collect()
    };
    let (first, _) = testing::spline(&mut sketch, &ring(0.7), true);
    testing::spline(&mut sketch, &ring(7.0), true);
    let near = DVec2::new(10.0, 0.0);
    assert_eq!(sketch.trim_piece(first, near), None);
    let trim = SketchEdit::Trim { curve: first, near };
    assert!(trim.apply(&sketch, &DESIGN).is_err());
}

#[test]
fn trimming_a_line_at_a_tangent_keeps_the_tangency() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let round = circle(&mut sketch, center, 5.0);
    let (a, b) = (
        point(&mut sketch, -10.0, 5.0),
        point(&mut sketch, 10.0, 5.0),
    );
    let tangent = line(&mut sketch, a, b);
    let touching = sketch.tangent(round, tangent).unwrap();
    let touching = constrain(&mut sketch, touching);
    let trim = SketchEdit::Trim {
        curve: tangent,
        near: DVec2::new(-6.0, 5.0),
    };
    let accepted = propose_it(&sketch, &trim).unwrap();
    let trimmed = &accepted.sketch;
    let (start, end) = ends(trimmed, tangent);
    assert_eq!(end, b);
    assert!(near(at(trimmed, start), DVec2::new(0.0, 5.0)));
    assert!(trimmed.constraint(touching).is_some());
    // Its new end on the circle restates the tangency there, to the
    // solver, so the tie is dropped: tangent, it can only be there.
    let applied = apply(&sketch, &trim);
    let on = Constraint::PointOnCurve {
        point: start,
        curve: round,
    };
    assert!(constraints(&applied).contains(&on));
    assert!(!constraints(trimmed).contains(&on));
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn trimming_the_middle_of_an_arc_keeps_both_on_one_circle() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let (start, end) = (
        point(&mut sketch, 10.0, 0.0),
        point(&mut sketch, -10.0, 0.0),
    );
    let top = arc(&mut sketch, center, start, end);
    for x in [5.0, -5.0] {
        let (low, high) = (point(&mut sketch, x, 5.0), point(&mut sketch, x, 15.0));
        line(&mut sketch, low, high);
    }
    let radius = dimension(&mut sketch, Measure::Radius(top), "10");
    let trim = SketchEdit::Trim {
        curve: top,
        near: DVec2::new(0.0, 10.0),
    };
    let piece = sketch.trim_piece(top, DVec2::new(0.0, 10.0)).unwrap();
    let x = 5.0;
    let y = (100.0f64 - x * x).sqrt();
    assert!(near(piece[0], DVec2::new(x, y)));
    assert!(near(*piece.last().unwrap(), DVec2::new(-x, y)));

    let accepted = propose_it(&sketch, &trim).unwrap();
    let trimmed = &accepted.sketch;
    let rest = trimmed.curves.last().unwrap();
    let Curve::Arc {
        center: rest_center,
        end: rest_end,
        ..
    } = rest.curve
    else {
        panic!("not an arc: {rest:?}");
    };
    assert_eq!((rest_center, rest_end), (center, end));
    assert_eq!(ends(trimmed, top).0, start);
    assert!(constraints(trimmed).contains(&Constraint::Equal(top, rest.id)));
    assert!(trimmed.dimension(radius).is_some());
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn trimming_carries_what_still_means_something() {
    let (mut sketch, across, [start, end], [left, _]) = crossed();
    let horizontal = constrain(&mut sketch, Constraint::Horizontal(across));
    let lone = point(&mut sketch, 40.0, 0.0);
    let on = constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: lone,
            curve: across,
        },
    );
    let middle = point(&mut sketch, 15.0, 0.0);
    constrain(
        &mut sketch,
        Constraint::Midpoint {
            point: middle,
            line: across,
        },
    );
    constrain(&mut sketch, Constraint::Equal(across, left));
    let fix = constrain(&mut sketch, Constraint::Fix(start));
    dimension(&mut sketch, Measure::Length(across), "30");
    dimension(&mut sketch, Measure::HorizontalDistance(start, end), "30");
    let angle = dimension(&mut sketch, Measure::Angle(across, left), "90 deg");
    let from = dimension(&mut sketch, Measure::Distance(lone, left), "30");

    let trim = SketchEdit::Trim {
        curve: across,
        near: DVec2::new(25.0, 0.0),
    };
    let trimmed = apply(&sketch, &trim);
    // The end is gone and what's on it; the length, its midpoint and an
    // equal length don't hold any more; what's on the endless line and
    // the start stay.
    assert!(trimmed.point(end).is_none());
    let kept: BTreeSet<Id> = trimmed
        .constraints
        .iter()
        .map(|entry| entry.id)
        .chain(trimmed.dimensions.iter().map(|entry| entry.id))
        .filter(|&id| id.0 < sketch.next_id)
        .collect();
    assert_eq!(kept, BTreeSet::from([horizontal, on, fix, angle, from]));
    // The new end, on the line cutting it: the solver drops what that
    // restates.
    let accepted = propose_it(&sketch, &trim).unwrap();
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn trimming_at_an_end_of_a_curve_ends_there_too() {
    let mut sketch = Sketch::default();
    let (start, end) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 20.0, 0.0));
    let across = line(&mut sketch, start, end);
    // A T: a line from the middle, its end snapped on.
    let (foot, top) = (point(&mut sketch, 10.0, 0.0), point(&mut sketch, 10.0, 8.0));
    let stem = line(&mut sketch, foot, top);
    constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: foot,
            curve: across,
        },
    );
    let trim = SketchEdit::Trim {
        curve: across,
        near: DVec2::new(15.0, 0.0),
    };
    let accepted = propose_it(&sketch, &trim).unwrap();
    let trimmed = &accepted.sketch;
    // The two now share the point, and the point on the line is the
    // line's own.
    assert_eq!(ends(trimmed, across), (start, foot));
    assert!(trimmed.curve(stem).is_some());
    assert!(trimmed.constraints.is_empty());
}

#[test]
fn trimming_what_is_no_curve_is_refused() {
    let (sketch, _, [start, _], _) = crossed();
    let trim = SketchEdit::Trim {
        curve: start,
        near: DVec2::ZERO,
    };
    assert_eq!(trim.apply(&sketch, &DESIGN), Err(EditError::Target(start)));
}

/// A line from (0, 0) to (10, 0).
fn short() -> (Sketch, Id, [Id; 2]) {
    let mut sketch = Sketch::default();
    let (start, end) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 10.0, 0.0));
    let id = line(&mut sketch, start, end);
    (sketch, id, [start, end])
}

#[test]
fn extending_a_line_reaches_a_line() {
    let (mut sketch, id, [start, end]) = short();
    let (low, high) = (
        point(&mut sketch, 20.0, -5.0),
        point(&mut sketch, 20.0, 5.0),
    );
    let wall = line(&mut sketch, low, high);
    // Past it, and before the line's start.
    let (low, high) = (
        point(&mut sketch, 30.0, -5.0),
        point(&mut sketch, 30.0, 5.0),
    );
    line(&mut sketch, low, high);
    let (low, high) = (
        point(&mut sketch, -8.0, -5.0),
        point(&mut sketch, -8.0, 5.0),
    );
    let behind = line(&mut sketch, low, high);
    let length = dimension(&mut sketch, Measure::Length(id), "10");

    let extension = sketch.extension(id, end, DESIGN.max).unwrap();
    assert!(near(extension[0], DVec2::new(10.0, 0.0)));
    assert!(near(extension[1], DVec2::new(20.0, 0.0)));

    let extend = SketchEdit::Extend { curve: id, end };
    let accepted = propose_it(&sketch, &extend).unwrap();
    let extended = &accepted.sketch;
    let (kept, new) = ends(extended, id);
    assert_eq!(kept, start);
    assert!(near(at(extended, new), DVec2::new(20.0, 0.0)));
    assert!(extended.point(end).is_none());
    // Its length would pull it back.
    assert!(extended.dimension(length).is_none());
    assert_eq!(
        constraints(extended),
        [Constraint::PointOnCurve {
            point: new,
            curve: wall
        }]
    );

    let back = SketchEdit::Extend {
        curve: id,
        end: start,
    };
    let extended = apply(&sketch, &back);
    let (new, _) = ends(&extended, id);
    assert!(near(at(&extended, new), DVec2::new(-8.0, 0.0)));
    assert!(constraints(&extended).contains(&Constraint::PointOnCurve {
        point: new,
        curve: behind
    }));
}

#[test]
fn extending_a_line_reaches_an_arc_and_a_circle() {
    for rim in [false, true] {
        let (mut sketch, id, [_, end]) = short();
        let center = point(&mut sketch, 30.0, 0.0);
        let round = if rim {
            circle(&mut sketch, center, 5.0)
        } else {
            // The left half, counter-clockwise from the top.
            let (top, bottom) = (
                point(&mut sketch, 30.0, 5.0),
                point(&mut sketch, 30.0, -5.0),
            );
            arc(&mut sketch, center, top, bottom)
        };
        let accepted = propose_it(&sketch, &SketchEdit::Extend { curve: id, end }).unwrap();
        let (_, new) = ends(&accepted.sketch, id);
        assert!(near(at(&accepted.sketch, new), DVec2::new(25.0, 0.0)));
        assert!(
            constraints(&accepted.sketch).contains(&Constraint::PointOnCurve {
                point: new,
                curve: round
            })
        );
    }
}

#[test]
fn extending_an_arc_follows_its_circle() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let (start, end) = (point(&mut sketch, 10.0, 0.0), point(&mut sketch, 0.0, 10.0));
    let quarter = arc(&mut sketch, center, start, end);
    let (low, high) = (
        point(&mut sketch, -5.0, -20.0),
        point(&mut sketch, -5.0, 20.0),
    );
    let wall = line(&mut sketch, low, high);
    // Round from the end, counter-clockwise, it meets the line at 120°;
    // round from the start, clockwise, at 240°.
    let reach = |end| {
        let extended = apply(
            &sketch,
            &SketchEdit::Extend {
                curve: quarter,
                end,
            },
        );
        let (new_start, new_end) = ends(&extended, quarter);
        let new = if end == start { new_start } else { new_end };
        (at(&extended, new), constraints(&extended))
    };
    let (place, ties) = reach(end);
    assert!(near(place, DVec2::from_angle(120f64.to_radians()) * 10.0));
    assert_eq!(ties.len(), 1);
    let (place, _) = reach(start);
    assert!(near(place, DVec2::from_angle(240f64.to_radians()) * 10.0));
    let accepted = propose_it(
        &sketch,
        &SketchEdit::Extend {
            curve: quarter,
            end,
        },
    )
    .unwrap();
    assert!(accepted.analysis.redundant.is_empty());
    assert!(accepted.sketch.curve(wall).is_some());
}

#[test]
fn extending_to_an_end_joins_it() {
    let (mut sketch, id, [_, end]) = short();
    let (corner, top) = (
        point(&mut sketch, 20.0, 0.0),
        point(&mut sketch, 20.0, 10.0),
    );
    line(&mut sketch, corner, top);
    let accepted = propose_it(&sketch, &SketchEdit::Extend { curve: id, end }).unwrap();
    assert_eq!(ends(&accepted.sketch, id).1, corner);
    assert!(accepted.sketch.constraints.is_empty());
}

#[test]
fn extending_with_nothing_ahead_is_refused() {
    let (mut sketch, id, [start, end]) = short();
    // Beside it, and behind the end extended.
    let (low, high) = (point(&mut sketch, 5.0, 2.0), point(&mut sketch, 5.0, 8.0));
    line(&mut sketch, low, high);
    let (low, high) = (
        point(&mut sketch, -5.0, -2.0),
        point(&mut sketch, -5.0, 2.0),
    );
    line(&mut sketch, low, high);
    let extend = SketchEdit::Extend { curve: id, end };
    assert_eq!(extend.apply(&sketch, &DESIGN), Err(EditError::NothingAhead));
    assert_eq!(
        propose_it(&sketch, &extend),
        Err(Rejected::Edit(EditError::NothingAhead))
    );
    assert!(sketch.extension(id, end, DESIGN.max).is_none());
    assert!(sketch.extension(id, start, DESIGN.max).is_some());

    // A circle has no end, and a point of another curve is none of this
    // one's.
    let center = point(&mut sketch, 0.0, 30.0);
    let round = circle(&mut sketch, center, 1.0);
    let circle_end = SketchEdit::Extend {
        curve: round,
        end: center,
    };
    assert_eq!(
        circle_end.apply(&sketch, &DESIGN),
        Err(EditError::Target(round))
    );
    let other = SketchEdit::Extend {
        curve: id,
        end: low,
    };
    assert_eq!(other.apply(&sketch, &DESIGN), Err(EditError::Target(low)));
}

#[test]
fn an_end_extended_leaves_a_shared_point_with_the_other_curve() {
    let (mut sketch, id, [_, end]) = short();
    let up = point(&mut sketch, 10.0, 10.0);
    let chained = line(&mut sketch, end, up);
    let (low, high) = (
        point(&mut sketch, 20.0, -5.0),
        point(&mut sketch, 20.0, 5.0),
    );
    line(&mut sketch, low, high);
    let extended = apply(&sketch, &SketchEdit::Extend { curve: id, end });
    assert_ne!(ends(&extended, id).1, end);
    assert_eq!(ends(&extended, chained), (end, up));
}

#[test]
fn nearer_end_is_by_the_ends() {
    let (sketch, id, [start, end]) = short();
    assert_eq!(sketch.nearer_end(id, DVec2::new(2.0, 1.0)), Some(start));
    assert_eq!(sketch.nearer_end(id, DVec2::new(5.0, 1.0)), Some(end));
    assert_eq!(sketch.nearer_end(start, DVec2::ZERO), None);
}

#[test]
fn mirroring_lines_ties_each_point_symmetric() {
    let (sketch, id, [start, end]) = {
        let mut sketch = Sketch::default();
        let (start, end) = (point(&mut sketch, 1.0, 0.0), point(&mut sketch, 5.0, 3.0));
        let id = line(&mut sketch, start, end);
        (sketch, id, [start, end])
    };
    let mirror = SketchEdit::Mirror {
        ids: vec![id, start],
        about: Id::Y_AXIS,
    };
    let accepted = propose_it(&sketch, &mirror).unwrap();
    let mirrored = &accepted.sketch;
    let copy = mirrored.curves.last().unwrap();
    assert_eq!(copy.name(), "Line 2");
    let (copy_start, copy_end) = ends(mirrored, copy.id);
    assert!(near(at(mirrored, copy_start), DVec2::new(-1.0, 0.0)));
    assert!(near(at(mirrored, copy_end), DVec2::new(-5.0, 3.0)));
    assert_eq!(
        constraints(mirrored),
        [(start, copy_start), (end, copy_end)].map(|(a, b)| Constraint::Symmetric {
            a,
            b,
            about: Id::Y_AXIS
        })
    );
    // The copy follows the original: no more freedom than it had.
    assert_eq!(accepted.analysis.freedom, 4);
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn mirroring_shares_points_on_the_line() {
    let mut sketch = Sketch::default();
    // A half outline about a construction line: its ends on it.
    let (low, high) = (
        point(&mut sketch, 0.0, -10.0),
        point(&mut sketch, 0.0, 10.0),
    );
    let axis = sketch
        .add_curve(
            Curve::Line {
                start: low,
                end: high,
            },
            true,
        )
        .unwrap();
    let corners =
        [(0.0, 2.0), (4.0, 2.0), (4.0, -2.0), (0.0, -2.0)].map(|(x, y)| point(&mut sketch, x, y));
    let lines = [0, 1, 2].map(|i| line(&mut sketch, corners[i], corners[i + 1]));
    let before = analyse(&sketch).freedom;
    let mirror = SketchEdit::Mirror {
        ids: lines.to_vec(),
        about: axis,
    };
    let accepted = propose_it(&sketch, &mirror).unwrap();
    let mirrored = &accepted.sketch;
    // Two new points, three new lines, the first and last from the points
    // on the line.
    assert_eq!(mirrored.points.len(), sketch.points.len() + 2);
    let copies: Vec<_> = mirrored.curves[sketch.curves.len()..].to_vec();
    assert_eq!(copies.len(), 3);
    assert_eq!(ends(mirrored, copies[0].id).0, corners[0]);
    assert_eq!(ends(mirrored, copies[2].id).1, corners[3]);
    // Held on the line: two fewer freedoms, the copies adding none.
    let on: Vec<_> = constraints(mirrored)
        .into_iter()
        .filter(|constraint| matches!(constraint, Constraint::PointOnCurve { .. }))
        .collect();
    assert_eq!(on.len(), 2);
    assert_eq!(accepted.analysis.freedom, before - 2);
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn mirroring_circles_and_arcs_keeps_their_radii() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 3.0, 1.0);
    let round = circle(&mut sketch, center, 2.0);
    let arc_center = point(&mut sketch, 3.0, 8.0);
    let (start, end) = (point(&mut sketch, 5.0, 8.0), point(&mut sketch, 3.0, 10.0));
    let quarter = arc(&mut sketch, arc_center, start, end);
    // An arc ending on the line: its point there isn't shared.
    let half_center = point(&mut sketch, 3.0, 20.0);
    let (half_start, half_end) = (point(&mut sketch, 6.0, 20.0), point(&mut sketch, 0.0, 20.0));
    let half = arc(&mut sketch, half_center, half_start, half_end);
    let before = analyse(&sketch).freedom;
    assert_eq!(before, 3 + 5 + 5);

    let mirror = SketchEdit::Mirror {
        ids: vec![round, quarter, half],
        about: Id::Y_AXIS,
    };
    let accepted = propose_it(&sketch, &mirror).unwrap();
    let mirrored = &accepted.sketch;
    let copies = &mirrored.curves[3..];
    let Curve::Circle { center: c, radius } = copies[0].curve else {
        panic!("not a circle: {:?}", copies[0]);
    };
    assert!(near(at(mirrored, c), DVec2::new(-3.0, 1.0)) && radius == 2.0);
    assert!(constraints(mirrored).contains(&Constraint::Equal(round, copies[0].id)));
    // Counter-clockwise from the image of the end to that of the start.
    let Curve::Arc {
        center: c,
        start: s,
        end: e,
    } = copies[1].curve
    else {
        panic!("not an arc: {:?}", copies[1]);
    };
    assert!(near(at(mirrored, c), DVec2::new(-3.0, 8.0)));
    assert!(near(at(mirrored, s), DVec2::new(-3.0, 10.0)));
    assert!(near(at(mirrored, e), DVec2::new(-5.0, 8.0)));
    let Curve::Arc { start: s, .. } = copies[2].curve else {
        panic!("not an arc: {:?}", copies[2]);
    };
    assert_ne!(s, half_end);
    assert!(near(at(mirrored, s), DVec2::new(0.0, 20.0)));
    // The copies add nothing but the half's end held on the line; the
    // copied arcs' radius equations follow from the symmetry.
    assert_eq!(accepted.analysis.freedom, before - 1);
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn mirroring_nothing_but_the_line_is_refused() {
    let (sketch, id, [start, _]) = short();
    let about = SketchEdit::Mirror {
        ids: vec![id],
        about: id,
    };
    assert_eq!(
        about.apply(&sketch, &DESIGN),
        Err(EditError::NothingToMirror)
    );
    let point_about = SketchEdit::Mirror {
        ids: vec![id],
        about: start,
    };
    assert_eq!(
        point_about.apply(&sketch, &DESIGN),
        Err(EditError::Target(start))
    );
    // A line on the line is its own image.
    let on = SketchEdit::Mirror {
        ids: vec![id],
        about: Id::X_AXIS,
    };
    assert_eq!(on.apply(&sketch, &DESIGN), Err(EditError::NothingToMirror));
}

/// A spline through a wave from (0, 0) to (30, 0), by fit points or by
/// control points (converted, keeping its shape), open, crossed by
/// vertical lines at x = 10 and x = 20: the spline, its ends, and the two
/// crossing it.
fn crossed_spline(kind: crate::SplineKind) -> (Sketch, Id, [Id; 2], [Id; 2]) {
    let mut sketch = Sketch::default();
    let places = [
        (0.0, 0.0),
        (6.0, 3.0),
        (15.0, -2.0),
        (24.0, 2.0),
        (30.0, 0.0),
    ];
    let (spline, points) = testing::spline(&mut sketch, &places, false);
    let (first, last) = (points[0], points[4]);
    sketch.convert_spline(spline, kind).unwrap();
    let cutters = [10.0, 20.0].map(|x| {
        let (low, high) = (point(&mut sketch, x, -6.0), point(&mut sketch, x, 6.0));
        line(&mut sketch, low, high)
    });
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    (sketch, spline, [first, last], cutters)
}

/// The furthest places along `geom` from `from` to `to` are from `other`.
fn apart(geom: &Geom, from: f64, to: f64, other: &Geom) -> f64 {
    (0..=100)
        .map(|i| {
            let place = geom.at(from + (to - from) * i as f64 / 100.0);
            other.at(other.closest(place)).distance(place)
        })
        .fold(0.0, f64::max)
}

#[test]
fn trimming_a_spline_between_cuts_leaves_two_keeping_its_shape() {
    for kind in [crate::SplineKind::Through, crate::SplineKind::Control] {
        let (mut sketch, spline, [first, last], [left, right]) = crossed_spline(kind);
        let before = geom(&sketch, spline);
        // A point held on each part, and on the part taken away.
        let held: Vec<Id> = [5.0, 15.0, 25.0]
            .map(|x| {
                let place = sketch.nearest_on(spline, DVec2::new(x, 0.0)).unwrap();
                let point = point(&mut sketch, place.x, place.y);
                constrain(
                    &mut sketch,
                    Constraint::PointOnCurve {
                        point,
                        curve: spline,
                    },
                );
                point
            })
            .to_vec();
        let near_middle = before.at(before.closest(DVec2::new(15.0, -2.0)));
        let piece = sketch.trim_piece(spline, near_middle).unwrap();
        assert!((piece[0].x - 10.0).abs() < 1e-6 && (piece.last().unwrap().x - 20.0).abs() < 1e-6);
        let trim = SketchEdit::Trim {
            curve: spline,
            near: near_middle,
        };
        let trimmed = propose_it(&sketch, &trim).unwrap().sketch;
        // The part before keeps the id and its start, the part after is a
        // spline of its own keeping the end, each of the kind it was.
        let rest = trimmed.curves.last().unwrap();
        assert_eq!(rest.name(), "Spline 2");
        let (kept, rest) = (
            trimmed.spline(spline).unwrap(),
            trimmed.spline(rest.id).unwrap(),
        );
        assert_eq!((kept.kind, rest.kind), (kind, kind));
        let ([start, cut], [cut_rest, end]) = (kept.ends().unwrap(), rest.ends().unwrap());
        assert_eq!((start, end), (first, last));
        // Through fit points, the solver moves what's held on it onto
        // the piece that strays a little, and what follows with it.
        let within = match kind {
            crate::SplineKind::Control => 1e-9,
            crate::SplineKind::Through => 0.01 * 30.0,
        };
        assert!((at(&trimmed, cut).x - 10.0).abs() < within, "{kind:?}");
        assert!((at(&trimmed, cut_rest).x - 20.0).abs() < within, "{kind:?}");
        let rest_id = trimmed.curves.last().unwrap().id;
        // Each new end on the line cutting it.
        for (point, curve) in [(cut, left), (cut_rest, right)] {
            assert!(
                constraints(&trimmed).contains(&Constraint::PointOnCurve { point, curve }),
                "{kind:?}"
            );
        }
        // What's on each part is on it, what was on the part taken away
        // is free.
        let on = |point, curve| {
            constraints(&trimmed).contains(&Constraint::PointOnCurve { point, curve })
        };
        assert!(on(held[0], spline) && on(held[2], rest_id), "{kind:?}");
        assert!(!on(held[1], spline) && !on(held[1], rest_id));
        // Both where it was: exactly by control points, closely through
        // fit points.
        for id in [spline, rest_id] {
            let part = geom(&trimmed, id);
            let most = apart(&part, 0.0, part.last(), &before);
            assert!(most < within, "{kind:?}: {most}");
        }
        assert!(analyse(&trimmed).redundant.is_empty());
    }
}

#[test]
fn trimming_a_closed_spline_leaves_it_open_round_the_rest() {
    for kind in [crate::SplineKind::Through, crate::SplineKind::Control] {
        let mut sketch = Sketch::default();
        let points: Vec<Id> = (0..8)
            .map(|i| {
                let angle = std::f64::consts::TAU * i as f64 / 8.0;
                point(&mut sketch, 10.0 * angle.cos(), 10.0 * angle.sin())
            })
            .collect();
        let spline = sketch
            .add_curve(Curve::Spline(crate::Spline::through(points, true)), false)
            .unwrap();
        sketch.convert_spline(spline, kind).unwrap();
        let before = geom(&sketch, spline);
        let (low, high) = (
            point(&mut sketch, 5.0, -15.0),
            point(&mut sketch, 5.0, 15.0),
        );
        line(&mut sketch, low, high);
        let trimmed = propose_it(
            &sketch,
            &SketchEdit::Trim {
                curve: spline,
                near: DVec2::new(10.0, 0.0),
            },
        )
        .unwrap()
        .sketch;
        let kept = trimmed.spline(spline).unwrap();
        assert!(!kept.closed);
        let [start, end] = kept.ends().unwrap();
        assert!((at(&trimmed, start).x - 5.0).abs() < 1e-6);
        assert!((at(&trimmed, end).x - 5.0).abs() < 1e-6);
        let part = geom(&trimmed, spline);
        // Round the left, as it was.
        assert!(part.at(0.5).x < -9.0, "{kind:?}");
        let within = match kind {
            crate::SplineKind::Control => 1e-9,
            crate::SplineKind::Through => 0.01 * 20.0,
        };
        assert!(apart(&part, 0.0, part.last(), &before) < within, "{kind:?}");
    }
}

#[test]
fn extending_a_spline_runs_on_to_what_it_meets() {
    for kind in [crate::SplineKind::Through, crate::SplineKind::Control] {
        let (mut sketch, spline, [first, last], _) = crossed_spline(kind);
        let (low, high) = (
            point(&mut sketch, 40.0, -20.0),
            point(&mut sketch, 40.0, 20.0),
        );
        let wall = line(&mut sketch, low, high);
        let preview = sketch.extension(spline, last, 1e3).unwrap();
        let extend = SketchEdit::Extend {
            curve: spline,
            end: last,
        };
        let extended = propose_it(&sketch, &extend).unwrap().sketch;
        let grown = extended.spline(spline).unwrap();
        let [start, end] = grown.ends().unwrap();
        assert_eq!(start, first);
        assert!(end != last && grown.points.contains(&last), "{kind:?}");
        assert!((at(&extended, end).x - 40.0).abs() < 1e-6, "{kind:?}");
        assert!(constraints(&extended).contains(&Constraint::PointOnCurve {
            point: end,
            curve: wall
        }));
        // The preview is what's added, from the old end on.
        assert!(near(*preview.last().unwrap(), at(&extended, end)));
        let old = at(&sketch, last);
        assert!(preview[0].distance(old) < 0.5, "{kind:?}: {}", preview[0]);
        // Its start, looking back along it, meets nothing.
        let back = SketchEdit::Extend {
            curve: spline,
            end: first,
        };
        assert_eq!(back.apply(&sketch, &DESIGN), Err(EditError::NothingAhead));
    }
}

#[test]
fn a_spline_extended_leaves_what_held_its_end() {
    let (mut sketch, spline, [_, last], _) = crossed_spline(crate::SplineKind::Through);
    let (low, high) = (
        point(&mut sketch, 40.0, -20.0),
        point(&mut sketch, 40.0, 20.0),
    );
    line(&mut sketch, low, high);
    // A line drawn on from its end, tangent there.
    let beyond = point(&mut sketch, 35.0, -3.0);
    let on = line(&mut sketch, last, beyond);
    sketch.add_handles(&[last]).expect("a handle at its end");
    let tangent = sketch.tangent(spline, on).unwrap();
    constrain(&mut sketch, tangent);
    let extended = apply(
        &sketch,
        &SketchEdit::Extend {
            curve: spline,
            end: last,
        },
    );
    assert!(
        !constraints(&extended)
            .iter()
            .any(|c| matches!(c, Constraint::Tangent { .. }))
    );
    // The line still ends there, on the spline's way.
    assert_eq!(extended.line(on).map(|_| ()), Some(()));
    assert!(extended.spline(spline).unwrap().has_handle(last));
}
