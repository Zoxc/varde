use glam::DVec2;
use varde_sketch::{Curve, Id, Sketch};

use super::*;
use crate::testing::{at, tool};

/// A pixel's size in sketch units.
const PIXEL: f64 = 0.1;

/// A line from (2, 1) to (8, 1), a circle of radius 2 about (20, 0), an
/// arc about (0, 20) of radius 3 from its right to its top, and a lone
/// point at (-10, -10).
struct Drawn {
    sketch: Sketch,
    start: Id,
    end: Id,
    line: Id,
    center: Id,
    circle: Id,
    arc_start: Id,
    arc: Id,
    lone: Id,
}

fn drawn() -> Drawn {
    let mut sketch = Sketch::default();
    let start = sketch.add_point(at(2.0, 1.0)).unwrap();
    let end = sketch.add_point(at(8.0, 1.0)).unwrap();
    let line = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    let center = sketch.add_point(at(20.0, 0.0)).unwrap();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 2.0,
            },
            false,
        )
        .unwrap();
    let arc_center = sketch.add_point(at(0.0, 20.0)).unwrap();
    let arc_start = sketch.add_point(at(3.0, 20.0)).unwrap();
    let arc_end = sketch.add_point(at(0.0, 23.0)).unwrap();
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
    let lone = sketch.add_point(at(-10.0, -10.0)).unwrap();
    Drawn {
        sketch,
        start,
        end,
        line,
        center,
        circle,
        arc_start,
        arc,
        lone,
    }
}

/// Where `tool`'s click with the cursor at `x`, `y` snaps in `sketch`.
fn snapped(sketch: &Sketch, tool: &ActiveTool, x: f64, y: f64) -> Snap {
    snap(sketch, tool, at(x, y), PIXEL)
}

fn target(snap: Snap) -> Option<Target> {
    snap.target
}

#[test]
fn points_come_first_the_origin_among_them() {
    let d = drawn();
    let point = tool(Tool::Point, &[], &[]);
    let near_end = snapped(&d.sketch, &point, 7.6, 1.3);
    assert_eq!(near_end.at, at(8.0, 1.0));
    assert_eq!(target(near_end), Some(Target::Point(d.end)));
    assert_eq!(near_end.kinds(), [ConstraintKind::Coincident]);
    assert_eq!(
        target(snapped(&d.sketch, &point, 20.3, 0.2)),
        Some(Target::Point(d.center))
    );
    assert_eq!(
        target(snapped(&d.sketch, &point, -9.5, -10.0)),
        Some(Target::Point(d.lone))
    );
    let origin = snapped(&d.sketch, &point, 0.2, -0.3);
    assert_eq!(
        (origin.at, target(origin)),
        (DVec2::ZERO, Some(Target::Point(Id::ORIGIN)))
    );
    // Out of reach, nothing.
    let free = snapped(&d.sketch, &point, -5.0, -5.0);
    assert!(!free.snapped());
    assert_eq!(free.at, at(-5.0, -5.0));
}

#[test]
fn then_midpoints_and_quadrants_then_curves_then_axes() {
    let d = drawn();
    let point = tool(Tool::Point, &[], &[]);
    let middle = snapped(&d.sketch, &point, 5.2, 1.4);
    assert_eq!(
        (middle.at, target(middle)),
        (at(5.0, 1.0), Some(Target::Midpoint(d.line)))
    );
    assert_eq!(middle.kinds(), [ConstraintKind::Midpoint]);
    let top = snapped(&d.sketch, &point, 20.3, 1.8);
    assert_eq!(top.at, at(20.0, 2.0));
    assert_eq!(
        target(top),
        Some(Target::Quadrant {
            round: d.circle,
            level: Level::Vertical
        })
    );
    let left = snapped(&d.sketch, &point, 18.2, 0.2);
    assert_eq!(
        target(left),
        Some(Target::Quadrant {
            round: d.circle,
            level: Level::Horizontal
        })
    );
    // An arc's quadrants only where it runs: its left isn't.
    let arc_left = snapped(&d.sketch, &point, -2.9, 20.3);
    assert_ne!(
        target(arc_left),
        Some(Target::Quadrant {
            round: d.arc,
            level: Level::Horizontal
        })
    );
    let on_top = snapped(&d.sketch, &point, 0.3, 22.8);
    assert_eq!(on_top.at, at(0.0, 23.0));

    // The nearest place on a line, within its ends.
    let on_line = snapped(&d.sketch, &point, 3.0, 1.5);
    assert_eq!(
        (on_line.at, target(on_line)),
        (at(3.0, 1.0), Some(Target::On(d.line)))
    );
    // Past its ends is off it, but on the x axis.
    let past = snapped(&d.sketch, &point, 9.5, 0.4);
    assert_eq!(
        (past.at, target(past)),
        (at(9.5, 0.0), Some(Target::On(Id::X_AXIS)))
    );
    // On a circle.
    let on_circle = snapped(&d.sketch, &point, 21.5, 1.5);
    assert_eq!(target(on_circle), Some(Target::On(d.circle)));
    assert!((on_circle.at.distance(at(20.0, 0.0)) - 2.0).abs() < 1e-12);
    // Only along an arc, not on the rest of its circle.
    let off_arc = snapped(&d.sketch, &point, -2.3, 18.2);
    assert_eq!(target(off_arc), None);
    // The sketch's curves before the axes: a line near the x axis wins.
    let near_both = snapped(&d.sketch, &point, 6.3, 0.6);
    assert_eq!(target(near_both), Some(Target::On(d.line)));
    let y_axis = snapped(&d.sketch, &point, 0.4, -6.0);
    assert_eq!(
        (y_axis.at, target(y_axis)),
        (at(0.0, -6.0), Some(Target::On(Id::Y_AXIS)))
    );
}

#[test]
fn a_line_infers_horizontal_and_vertical_from_its_start() {
    let d = drawn();
    let placed = [at(-10.0, 5.0)];
    let line = tool(Tool::Line, &placed, &[None]);
    let level = snapped(&d.sketch, &line, -4.0, 5.4);
    assert_eq!(level.at, at(-4.0, 5.0));
    assert_eq!(
        (level.target, level.inference),
        (None, Some(Inference::Horizontal))
    );
    assert_eq!(level.kinds(), [ConstraintKind::Horizontal]);
    // Its guide runs from the start on past the cursor.
    let guide = level.guide(&d.sketch, &line, PIXEL).unwrap();
    assert_eq!(guide[0], placed[0]);
    assert!(guide[1].x > -4.0 && guide[1].y == 5.0);
    let upright = snapped(&d.sketch, &line, -10.5, 12.0);
    assert_eq!(upright.at, at(-10.0, 12.0));
    assert_eq!(upright.inference, Some(Inference::Vertical));
    // Not too near the start to tell, nor too far off.
    assert!(!snapped(&d.sketch, &line, -9.9, 5.05).snapped());
    assert!(!snapped(&d.sketch, &line, -4.0, 7.0).snapped());
    // Where a curve crosses the direction: here the y axis.
    let crossing = snapped(&d.sketch, &line, 0.5, 5.3);
    assert_eq!(crossing.at, at(0.0, 5.0));
    assert_eq!(crossing.target, Some(Target::On(Id::Y_AXIS)));
    assert_eq!(crossing.inference, Some(Inference::Horizontal));
    assert_eq!(
        crossing.kinds(),
        [ConstraintKind::Coincident, ConstraintKind::Horizontal]
    );
}

#[test]
fn a_line_from_a_line_s_end_infers_perpendicular_and_parallel() {
    let mut d = drawn();
    // Tilt the line, so its directions aren't the axes'.
    d.sketch.point_mut(d.end).unwrap().at = at(8.0, 7.0);
    let start = at(8.0, 7.0);
    let placed = [start];
    let targets = [Some(Target::Point(d.end))];
    let line = tool(Tool::Line, &placed, &targets);
    // Perpendicular: along (-1, 1) from the end.
    let across = snapped(&d.sketch, &line, 3.1, 12.0);
    assert!(across.at.abs_diff_eq(at(3.05, 11.95), 1e-9), "{across:?}");
    assert_eq!(across.inference, Some(Inference::Perpendicular(d.line)));
    // Parallel: on along (1, 1).
    let on = snapped(&d.sketch, &line, 12.1, 11.0);
    assert!(on.at.abs_diff_eq(at(12.05, 11.05), 1e-9), "{on:?}");
    assert_eq!(on.inference, Some(Inference::Parallel(d.line)));
}

#[test]
fn a_line_from_an_arc_s_end_infers_the_tangent_and_touches_circles() {
    let d = drawn();
    let start = at(3.0, 20.0);
    let placed = [start];
    let targets = [Some(Target::Point(d.arc_start))];
    let line = tool(Tool::Line, &placed, &targets);
    // Straight down from the arc's start is its tangent there.
    let down = snapped(&d.sketch, &line, 3.3, 14.0);
    assert_eq!(down.at, at(3.0, 14.0));
    // Vertical too, and preferred.
    assert_eq!(down.inference, Some(Inference::Vertical));

    // From (20, 4), the line touches the circle about (20, 0) of radius 2
    // 60° either side of straight down to its centre.
    let placed = [at(20.0, 4.0)];
    let line = tool(Tool::Line, &placed, &[None]);
    let touch = at(20.0, 0.0) + 2.0 * varde_sketch::angle::from_angle(30f64.to_radians());
    let tangent = snapped(&d.sketch, &line, touch.x + 0.1, touch.y);
    assert!(tangent.at.abs_diff_eq(touch, 1e-12), "{tangent:?}");
    // With libm's bits, the same natively and on the web: the tangent
    // point is saved with the line.
    let turn = libm::atan2(4.0, 0.0) - libm::acos(2.0 / 4.0);
    let touch = at(20.0, 0.0) + 2.0 * DVec2::new(libm::cos(turn), libm::sin(turn));
    assert_eq!(tangent.at, touch);
    assert_eq!(tangent.target, Some(Target::On(d.circle)));
    assert_eq!(tangent.inference, Some(Inference::Tangent(d.circle)));
}

#[test]
fn an_arc_s_last_point_snaps_to_points_and_the_tangent_arc() {
    let d = drawn();
    // From the line's end (8, 1) to (8, 9): tangent to the line there, the
    // arc's centre is straight above the end, at (8, 5), radius 4.
    let placed = [at(8.0, 1.0), at(8.0, 9.0)];
    let targets = [Some(Target::Point(d.end)), None];
    let arc = tool(Tool::Arc, &placed, &targets);
    let through = snapped(&d.sketch, &arc, 12.3, 5.0);
    assert!(through.at.abs_diff_eq(at(12.0, 5.0), 1e-9), "{through:?}");
    assert_eq!(through.inference, Some(Inference::Tangent(d.line)));
    assert_eq!(through.target, None);
    assert_eq!(through.kinds(), [ConstraintKind::Tangent]);
    // Nothing else of a curve, not being one of the arc's points.
    assert!(!snapped(&d.sketch, &arc, 5.0, 1.2).snapped());
    // But a point, which it passes through.
    let on_start = snapped(&d.sketch, &arc, 2.1, 1.1);
    assert_eq!(on_start.target, Some(Target::Point(d.start)));

    // A circle's rim the same, without the tangent.
    let placed = [at(-3.0, 1.0)];
    let circle = tool(Tool::Circle, &placed, &[None]);
    assert_eq!(
        snapped(&d.sketch, &circle, 2.2, 1.1).target,
        Some(Target::Point(d.start))
    );
    assert!(!snapped(&d.sketch, &circle, 5.0, 1.2).snapped());
}

#[test]
fn the_dimension_tool_snaps_to_nothing() {
    let d = drawn();
    let dimension = tool(Tool::Dimension, &[], &[]);
    assert!(!snapped(&d.sketch, &dimension, 8.0, 1.0).snapped());
}

#[test]
fn a_dragged_point_snaps_but_not_to_its_own_curves() {
    let d = drawn();
    // The line's end, by the lone point, takes it.
    let lone = snap_drag(&d.sketch, d.end, at(-9.6, -10.2), PIXEL);
    assert_eq!(
        (lone.at, target(lone)),
        (at(-10.0, -10.0), Some(Target::Point(d.lone)))
    );
    // By its own line's other end or middle, it stays free...
    assert!(!snap_drag(&d.sketch, d.end, at(2.3, 1.2), PIXEL).snapped());
    assert!(!snap_drag(&d.sketch, d.end, at(5.0, 1.3), PIXEL).snapped());
    // ...but another point by them snaps to them.
    let other = snap_drag(&d.sketch, d.lone, at(5.2, 1.3), PIXEL);
    assert_eq!(target(other), Some(Target::Midpoint(d.line)));
    // A circle's quadrant, then its rim, then an axis.
    let top = snap_drag(&d.sketch, d.lone, at(20.3, 1.8), PIXEL);
    assert_eq!(top.at, at(20.0, 2.0));
    let rim = snap_drag(&d.sketch, d.lone, at(21.5, 1.5), PIXEL);
    assert_eq!(target(rim), Some(Target::On(d.circle)));
    let axis = snap_drag(&d.sketch, d.lone, at(-5.0, 0.3), PIXEL);
    assert_eq!(
        (axis.at, target(axis)),
        (at(-5.0, 0.0), Some(Target::On(Id::X_AXIS)))
    );
    // The arc's start isn't snapped onto its own arc.
    assert!(!snap_drag(&d.sketch, d.arc_start, at(2.6, 21.6), PIXEL).snapped());
}

/// An arc's end dragged snaps to its other end, closing it, and a point
/// of a spline of three points or more to any other of its points; a
/// line's end, or a spline's of two points, doesn't.
#[test]
fn a_dragged_end_snaps_to_its_own_curves_other_end() {
    let d = drawn();
    let Some([_, arc_end]) = d.sketch.curve(d.arc).unwrap().curve.ends() else {
        panic!("the arc has no ends");
    };
    let closed = snap_drag(&d.sketch, d.arc_start, at(0.2, 22.8), PIXEL);
    assert_eq!(
        (closed.at, target(closed)),
        (at(0.0, 23.0), Some(Target::Point(arc_end)))
    );
    assert_eq!(closing(&d.sketch, d.arc, d.arc_start), Some(arc_end));
    assert_eq!(closing(&d.sketch, d.line, d.start), None);
    let mut sketch = d.sketch.clone();
    let mut spline = |places: &[(f64, f64)]| {
        let points: Vec<Id> = (places.iter())
            .map(|&(x, y)| sketch.add_point(at(x, y)).unwrap())
            .collect();
        let spline = varde_sketch::Spline::through(points.clone(), false);
        let id = sketch.add_curve(Curve::Spline(spline), false).unwrap();
        (id, points)
    };
    let (long, fit) = spline(&[(30.0, 30.0), (40.0, 35.0), (30.0, 40.0)]);
    let (short, two) = spline(&[(50.0, 50.0), (55.0, 50.0)]);
    let ends = snap_drag(&sketch, fit[2], at(30.2, 30.3), PIXEL);
    assert_eq!(target(ends), Some(Target::Point(fit[0])));
    assert_eq!(closing(&sketch, long, fit[0]), Some(fit[2]));
    assert_eq!(closing(&sketch, long, fit[1]), None);
    assert_eq!(closing(&sketch, short, two[0]), None);
    assert!(!snap_drag(&sketch, two[1], at(50.2, 50.1), PIXEL).snapped());
    // Any of a spline's points to any other of its own, not only its ends.
    let middle = snap_drag(&sketch, fit[1], at(30.1, 39.8), PIXEL);
    assert_eq!(target(middle), Some(Target::Point(fit[2])));
    let first = snap_drag(&sketch, fit[0], at(39.9, 35.2), PIXEL);
    assert_eq!(target(first), Some(Target::Point(fit[1])));
}

/// A line doesn't snap to the point it starts from, whether a point of
/// the sketch's or the end it goes on from, but still to others.
#[test]
fn a_line_doesnt_snap_to_its_own_start() {
    let d = drawn();
    let placed = [at(8.0, 1.0)];
    let targets = [Some(Target::Point(d.end))];
    let line = tool(Tool::Line, &placed, &targets);
    let back = snapped(&d.sketch, &line, 7.8, 1.2);
    assert_ne!(target(back), Some(Target::Point(d.end)), "{back:?}");
    assert_eq!(
        target(snapped(&d.sketch, &line, 2.2, 1.1)),
        Some(Target::Point(d.start))
    );
    // From the origin, not back to it.
    let (origin_placed, origin_targets) = ([DVec2::ZERO], [Some(Target::Point(Id::ORIGIN))]);
    let from_origin = tool(Tool::Line, &origin_placed, &origin_targets);
    let origin = snapped(&d.sketch, &from_origin, 0.1, 0.2);
    assert_ne!(
        target(origin),
        Some(Target::Point(Id::ORIGIN)),
        "{origin:?}"
    );
}

/// A circle's rim dragged snaps to points, not its own centre, nor to
/// curves.
#[test]
fn a_dragged_rim_snaps_to_points() {
    let d = drawn();
    let near = snap_rim(&d.sketch, d.circle, at(8.3, 0.8), PIXEL);
    assert_eq!(
        (near.at, target(near)),
        (at(8.0, 1.0), Some(Target::Point(d.end)))
    );
    let origin = snap_rim(&d.sketch, d.circle, at(0.2, -0.3), PIXEL);
    assert_eq!(target(origin), Some(Target::Point(Id::ORIGIN)));
    assert!(!snap_rim(&d.sketch, d.circle, at(20.2, 0.1), PIXEL).snapped());
    assert!(!snap_rim(&d.sketch, d.circle, at(5.0, 1.3), PIXEL).snapped());
    assert!(!snap_rim(&d.sketch, d.line, at(8.3, 0.8), PIXEL).snapped());
}

/// A spline being drawn snaps to its own points placed, all but the last
/// and the first only once a click there closes it, before the sketch's
/// points, and to its curve through them before the sketch's curves.
#[test]
fn a_spline_snaps_to_itself() {
    let d = drawn();
    let placed = [at(7.8, 1.3), at(12.0, 5.0), at(4.0, 8.0)];
    let targets = [None; 3];
    let three = tool(Tool::Spline, &placed, &targets);
    let own = |snap: Snap| (snap.at, target(snap));
    // (8, 1), the line's end, is nearer the cursor than the first point.
    assert_eq!(
        own(snapped(&d.sketch, &three, 8.0, 1.1)),
        (placed[0], Some(Target::Own(Own::Point(0))))
    );
    assert_eq!(
        own(snapped(&d.sketch, &three, 12.1, 5.1)),
        (placed[1], Some(Target::Own(Own::Point(1))))
    );
    assert_ne!(
        target(snapped(&d.sketch, &three, 4.1, 8.1)),
        Some(Target::Own(Own::Point(2)))
    );
    let two = tool(Tool::Spline, &placed[..2], &targets[..2]);
    assert_eq!(
        target(snapped(&d.sketch, &two, 8.0, 1.1)),
        Some(Target::Point(d.end))
    );
    // Straight between two, on the way.
    let on = snapped(&d.sketch, &two, 9.9, 3.3);
    assert_eq!(target(on), Some(Target::Own(Own::Curve)));
    assert!(on.at.distance(at(9.9, 3.15)) < 0.2, "{on:?}");
    // Not with another tool.
    let line = tool(Tool::Line, &placed[..1], &targets[..1]);
    assert_eq!(
        target(snapped(&d.sketch, &line, 8.0, 1.1)),
        Some(Target::Point(d.end))
    );
}
