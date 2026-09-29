use varde_sketch::Side;

use super::*;

fn near(found: &[DVec2], expected: &[DVec2]) -> bool {
    found.len() == expected.len()
        && found
            .iter()
            .zip(expected)
            .all(|(a, b)| a.distance(*b) < 1e-9)
}

/// Two lines meeting at right angles at (10, 0), a circle of radius 2
/// about (20, 5) and a lone point.
fn drawn() -> (Sketch, [Id; 4], Id) {
    let mut sketch = Sketch::default();
    let mut point = |x, y| sketch.add_point(DVec2::new(x, y)).unwrap();
    let (a, b, c) = (point(0.0, 0.0), point(10.0, 0.0), point(10.0, 8.0));
    let center = point(20.0, 5.0);
    let lone = point(3.0, 3.0);
    let across = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let up = sketch
        .add_curve(Curve::Line { start: b, end: c }, false)
        .unwrap();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 2.0,
            },
            false,
        )
        .unwrap();
    (sketch, [across, up, circle, a], lone)
}

#[test]
fn glyphs_go_where_their_constraint_is_about() {
    let (sketch, [across, up, circle, a], lone) = drawn();
    let glyphs = |constraint| anchors(&sketch, &constraint);
    // On a point, and midway between two.
    assert!(near(
        &glyphs(Constraint::Coincident(lone, a)),
        &[DVec2::new(3.0, 3.0)]
    ));
    assert!(near(
        &glyphs(Constraint::HorizontalPoints(a, lone)),
        &[DVec2::new(1.5, 1.5)]
    ));
    // A line's middle, a circle's upper right.
    assert!(near(
        &glyphs(Constraint::Horizontal(across)),
        &[DVec2::new(5.0, 0.0)]
    ));
    let upper_right = DVec2::new(20.0, 5.0) + DVec2::splat(2f64.sqrt());
    assert!(near(&glyphs(Constraint::Fix(circle)), &[upper_right]));
    // At the corner, and on each of two parallel.
    assert!(near(
        &glyphs(Constraint::Perpendicular(across, up)),
        &[DVec2::new(10.0, 0.0)]
    ));
    assert!(near(
        &glyphs(Constraint::Parallel(across, up)),
        &[DVec2::new(5.0, 0.0), DVec2::new(10.0, 4.0)]
    ));
    // Where a line would touch the circle: the foot of its centre.
    let tangent = Constraint::Tangent {
        a: up,
        b: circle,
        side: Side::Negative,
        at: None,
    };
    assert!(near(&glyphs(tangent), &[DVec2::new(10.0, 5.0)]));
    // With a spline, where it's named as touching.
    let smooth = Constraint::Smooth {
        a: across,
        b: circle,
        at: lone,
        side: Side::Positive,
    };
    assert!(near(&glyphs(smooth), &[DVec2::new(3.0, 3.0)]));
    // At the centre.
    assert!(near(
        &glyphs(Constraint::Concentric(lone, circle)),
        &[DVec2::new(20.0, 5.0)]
    ));
    // On the copy an equal offset ties, not the lead's.
    let equal = Constraint::EqualOffset {
        a: [across, up],
        b: [up, across],
    };
    assert!(near(&glyphs(equal), &[DVec2::new(5.0, 0.0)]));
}

#[test]
fn lines_far_apart_get_a_glyph_between_them() {
    let mut sketch = Sketch::default();
    let mut point = |x, y| sketch.add_point(DVec2::new(x, y)).unwrap();
    let (a, b) = (point(0.0, 0.0), point(1.0, 0.0));
    let (c, d) = (point(100.0, 50.0), point(100.0, 51.0));
    let first = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let second = sketch
        .add_curve(Curve::Line { start: c, end: d }, false)
        .unwrap();
    let found = anchors(&sketch, &Constraint::Perpendicular(first, second));
    assert!(near(&found, &[DVec2::new(50.25, 25.25)]), "{found:?}");
}

#[test]
fn glyphs_on_the_origin_and_axes_go_by_the_sketch_s_own() {
    let (sketch, [across, up, circle, a], lone) = drawn();
    let anchors = |constraint| anchors(&sketch, &constraint);
    // On the line alone, the axis having no middle.
    assert!(near(
        &anchors(Constraint::Parallel(across, Id::X_AXIS)),
        &[DVec2::new(5.0, 0.0)]
    ));
    // Where the line meets the axis, however far along it.
    assert!(near(
        &anchors(Constraint::Perpendicular(up, Id::X_AXIS)),
        &[DVec2::new(10.0, 0.0)]
    ));
    assert!(near(
        &anchors(Constraint::Coincident(lone, Id::ORIGIN)),
        &[DVec2::new(3.0, 3.0)]
    ));
    assert!(near(
        &anchors(Constraint::PointOnCurve {
            point: a,
            curve: Id::Y_AXIS
        }),
        &[DVec2::ZERO]
    ));
    // Where the circle touches the axis.
    assert!(near(
        &anchors(sketch.tangent(Id::X_AXIS, circle).unwrap()),
        &[DVec2::new(20.0, 0.0)]
    ));
}
