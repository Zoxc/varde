use varde_sketch::Curve;

use super::*;
use crate::testing::{at, handled_spline, line, point};

/// Whether `a` and `b` are the same polylines, within a rounding.
fn same(a: &[Vec<DVec2>], b: &[Vec<DVec2>]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.distance(*q) < 1e-9))
}

#[test]
fn a_length_has_extension_lines_out_to_its_dimension_line_through_the_label() {
    let mut sketch = Sketch::default();
    let [a, b] = [(0.0, 0.0), (10.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let id = line(&mut sketch, a, b);
    let length = Measure::Length(id);
    let drawn = lines(&sketch, &length, Side::Positive, at(4.0, 3.0)).unwrap();
    let expected = [
        vec![at(0.0, 0.0), at(0.0, 3.0)],
        vec![at(10.0, 0.0), at(10.0, 3.0)],
        vec![at(0.0, 3.0), at(10.0, 3.0)],
    ];
    assert!(same(&drawn.lines, &expected), "{drawn:?}");
    assert_eq!(
        drawn.arrows,
        [[at(0.0, 3.0), at(10.0, 3.0)], [at(10.0, 3.0), at(0.0, 3.0)]]
    );
    // Off the end, the dimension line reaches the label.
    let drawn = lines(&sketch, &length, Side::Positive, at(14.0, -2.0)).unwrap();
    assert!(same(
        &drawn.lines[2..],
        &[vec![at(0.0, -2.0), at(10.0, -2.0), at(14.0, -2.0)]]
    ));
}

#[test]
fn horizontal_and_vertical_distances_run_along_their_axes() {
    let mut sketch = Sketch::default();
    let [a, b] = [(0.0, 0.0), (10.0, 5.0)].map(|(x, y)| point(&mut sketch, x, y));
    let horizontal = Measure::HorizontalDistance(a, b);
    let drawn = lines(&sketch, &horizontal, Side::Positive, at(4.0, -3.0)).unwrap();
    let expected = [
        vec![at(0.0, 0.0), at(0.0, -3.0)],
        vec![at(10.0, 5.0), at(10.0, -3.0)],
        vec![at(0.0, -3.0), at(10.0, -3.0)],
    ];
    assert!(same(&drawn.lines, &expected), "{drawn:?}");
    let vertical = Measure::VerticalDistance(a, b);
    let drawn = lines(&sketch, &vertical, Side::Positive, at(12.0, 1.0)).unwrap();
    let expected = [
        vec![at(0.0, 0.0), at(12.0, 0.0)],
        vec![at(10.0, 5.0), at(12.0, 5.0)],
        vec![at(12.0, 0.0), at(12.0, 5.0)],
    ];
    assert!(same(&drawn.lines, &expected), "{drawn:?}");
}

#[test]
fn a_point_s_distance_from_a_line_runs_to_its_foot_the_line_extended() {
    let mut sketch = Sketch::default();
    let [a, b, p] = [(0.0, 0.0), (10.0, 0.0), (15.0, 5.0)].map(|(x, y)| point(&mut sketch, x, y));
    let id = line(&mut sketch, a, b);
    let distance = Measure::Distance(p, id);
    let drawn = lines(&sketch, &distance, Side::Positive, at(17.0, 2.0)).unwrap();
    let expected = [
        vec![at(15.0, 5.0), at(17.0, 5.0)],
        vec![at(15.0, 0.0), at(17.0, 0.0)],
        vec![at(17.0, 5.0), at(17.0, 0.0)],
        // The line on to the foot, past its end.
        vec![at(10.0, 0.0), at(15.0, 0.0)],
    ];
    assert!(same(&drawn.lines, &expected), "{drawn:?}");
}

#[test]
fn an_angle_is_an_arc_through_the_label_in_its_corner_or_across() {
    let mut sketch = Sketch::default();
    let [o, x, y] = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)].map(|(x, y)| point(&mut sketch, x, y));
    let (a, b) = (line(&mut sketch, o, x), line(&mut sketch, o, y));
    let angle = Measure::Angle(a, b);
    let arc = |label| {
        let drawn = lines(&sketch, &angle, Side::Positive, label).unwrap();
        let arc = drawn.lines.last().unwrap().clone();
        (arc, drawn)
    };
    let (quarter, drawn) = arc(at(3.0, 4.0));
    assert!(quarter.first().unwrap().distance(at(5.0, 0.0)) < 1e-9);
    assert!(quarter.last().unwrap().distance(at(0.0, 5.0)) < 1e-9);
    assert!(quarter.iter().all(|p| (p.length() - 5.0).abs() < 1e-9));
    assert_eq!(drawn.lines.len(), 1, "within the lines, nothing extended");
    assert_eq!(drawn.arrows.len(), 2);
    assert_eq!(drawn.arrows[0][0], quarter[0]);
    // Across the corner.
    let (across, _) = arc(at(-3.0, -4.0));
    assert!(across.first().unwrap().distance(at(-5.0, 0.0)) < 1e-9);
    assert!(across.last().unwrap().distance(at(0.0, -5.0)) < 1e-9);
    // Past the lines' ends, they're extended to it.
    let (_, drawn) = arc(at(12.0, 16.0));
    let expected = [
        vec![at(10.0, 0.0), at(20.0, 0.0)],
        vec![at(0.0, 10.0), at(0.0, 20.0)],
    ];
    assert!(same(&drawn.lines[..2], &expected), "{drawn:?}");
}

#[test]
fn a_radius_runs_from_the_centre_and_a_diameter_across_out_to_the_label() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 5.0,
            },
            false,
        )
        .unwrap();
    let radius = lines(
        &sketch,
        &Measure::Radius(circle),
        Side::Positive,
        at(10.0, 0.0),
    )
    .unwrap();
    let expected = [
        vec![at(0.0, 0.0), at(5.0, 0.0)],
        vec![at(5.0, 0.0), at(10.0, 0.0)],
    ];
    assert!(same(&radius.lines, &expected), "{radius:?}");
    assert_eq!(radius.arrows, [[at(5.0, 0.0), at(0.0, 0.0)]]);
    // Inside, it only reaches the curve.
    let diameter = lines(
        &sketch,
        &Measure::Diameter(circle),
        Side::Positive,
        at(0.0, 2.0),
    )
    .unwrap();
    assert!(same(&diameter.lines, &[vec![at(0.0, -5.0), at(0.0, 5.0)]]));
    assert_eq!(diameter.arrows.len(), 2);
    // What isn't a circle draws nothing.
    let gone = Measure::Radius(center);
    assert_eq!(lines(&sketch, &gone, Side::Positive, at(0.0, 0.0)), None);
}

#[test]
fn an_offset_runs_across_from_the_original_to_its_copy() {
    let mut sketch = Sketch::default();
    // Lines 2 apart.
    let [a, b, c, d] =
        [(0.0, 0.0), (10.0, 0.0), (0.0, 2.0), (10.0, 2.0)].map(|(x, y)| point(&mut sketch, x, y));
    let (low, high) = (line(&mut sketch, a, b), line(&mut sketch, c, d));
    let drawn = lines(
        &sketch,
        &Measure::Offset(low, high),
        Side::Positive,
        at(5.0, 1.0),
    )
    .unwrap();
    let expected = [
        vec![at(5.0, 2.0), at(5.0, 2.0)],
        vec![at(5.0, 0.0), at(5.0, 0.0)],
        vec![at(5.0, 2.0), at(5.0, 0.0)],
    ];
    assert!(same(&drawn.lines, &expected), "{drawn:?}");
    // Circles 1 and 3 about the origin's right, and a round join about its
    // corner, towards the label.
    let center = point(&mut sketch, 20.0, 0.0);
    let [inner, outer] = [1.0, 3.0].map(|radius| {
        sketch
            .add_curve(Curve::Circle { center, radius }, false)
            .unwrap()
    });
    let label = at(20.0, 2.0);
    let drawn = lines(
        &sketch,
        &Measure::Offset(inner, outer),
        Side::Positive,
        label,
    )
    .unwrap();
    assert!(
        same(&drawn.lines, &[vec![at(20.0, 1.0), at(20.0, 3.0)]]),
        "{drawn:?}"
    );
    assert_eq!(drawn.arrows.len(), 2);
    let drawn = lines(
        &sketch,
        &Measure::Offset(center, outer),
        Side::Positive,
        label,
    )
    .unwrap();
    assert!(
        same(&drawn.lines, &[vec![at(20.0, 0.0), at(20.0, 3.0)]]),
        "{drawn:?}"
    );
}

#[test]
fn a_handle_s_angle_is_an_arc_from_the_axis_and_its_offset_a_line_from_the_spline() {
    let mut sketch = Sketch::default();
    let (spline, _, tip) = handled_spline(&mut sketch);
    // From the X axis round to the handle, about where they meet.
    let angle = Measure::Angle(Id::X_AXIS, tip);
    let drawn = lines(&sketch, &angle, Side::Positive, at(16.0, 4.0)).unwrap();
    let arc = drawn.lines.last().unwrap();
    let meet = sketch.anchor(&angle).unwrap();
    assert!(meet.distance(at(6.0, 0.0)) < 1e-9, "{meet}");
    assert!((arc[0].y).abs() < 1e-9 && arc.len() > 2);
    assert_eq!(drawn.arrows.len(), 2);
    // A point's offset from a spline, from the place on it nearest.
    let off = point(&mut sketch, 10.0, 7.0);
    let offset = Measure::Offset(spline, off);
    let drawn = lines(&sketch, &offset, Side::Positive, at(12.0, 6.0)).unwrap();
    let foot = sketch.nearest_on(spline, at(10.0, 7.0)).unwrap();
    assert!(drawn.lines[0][0].distance(foot) < 1e-9);
    assert!(drawn.lines[1][0].distance(at(10.0, 7.0)) < 1e-9);
}
