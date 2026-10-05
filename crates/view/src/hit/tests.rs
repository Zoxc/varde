use varde_document::OriginPlane;

use super::*;
use crate::projection::top_camera;

/// A sketch with a line from `a` to `b`, a circle around `c`, a quarter
/// arc around `d` and a lone point, and their ids.
struct Drawn {
    sketch: Sketch,
    a: Id,
    b: Id,
    line: Id,
    c: Id,
    circle: Id,
    arc: Id,
    lone: Id,
}

fn drawn() -> Drawn {
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::ZERO).unwrap();
    let b = sketch.add_point(DVec2::new(10.0, 0.0)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let c = sketch.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center: c,
                radius: 5.0,
            },
            false,
        )
        .unwrap();
    let d = sketch.add_point(DVec2::new(0.0, 20.0)).unwrap();
    let start = sketch.add_point(DVec2::new(5.0, 20.0)).unwrap();
    let end = sketch.add_point(DVec2::new(0.0, 25.0)).unwrap();
    let arc = sketch
        .add_curve(
            Curve::Arc {
                center: d,
                start,
                end,
            },
            true,
        )
        .unwrap();
    let lone = sketch.add_point(DVec2::new(30.0, 30.0)).unwrap();
    Drawn {
        sketch,
        a,
        b,
        line,
        c,
        circle,
        arc,
        lone,
    }
}

#[test]
fn points_come_before_the_curves_they_are_on() {
    let d = drawn();
    let hit = |x, y, tolerance| hit(&d.sketch, DVec2::new(x, y), tolerance);
    assert_eq!(hit(0.2, 0.1, 0.5), Some(d.a));
    assert_eq!(hit(9.9, 0.0, 0.5), Some(d.b));
    assert_eq!(hit(5.0, 0.3, 0.5), Some(d.line));
    // The circle's centre is its point, the circle its edge.
    assert_eq!(hit(20.0, 0.0, 0.5), Some(d.c));
    assert_eq!(hit(25.2, 0.0, 0.5), Some(d.circle));
    assert_eq!(hit(20.0, -4.7, 0.5), Some(d.circle));
    assert_eq!(hit(30.0, 30.4, 0.5), Some(d.lone));
    // Nothing within the tolerance.
    assert_eq!(hit(5.0, 1.0, 0.5), None);
    assert_eq!(hit(20.0, 2.0, 0.5), None);
    assert_eq!(hit(f64::NAN, 0.0, 0.5), None);
}

#[test]
fn the_nearest_wins() {
    let d = drawn();
    // Both ends of the line are within 6, the nearer wins.
    assert_eq!(hit(&d.sketch, DVec2::new(4.0, 0.0), 6.0), Some(d.a));
    assert_eq!(hit(&d.sketch, DVec2::new(6.0, 0.0), 6.0), Some(d.b));
    // Between the line and the circle, off their points.
    assert_eq!(hit(&d.sketch, DVec2::new(8.0, 2.5), 3.0), Some(d.line));
    assert_eq!(hit(&d.sketch, DVec2::new(14.0, 2.5), 3.0), Some(d.circle));
}

#[test]
fn an_arc_is_hit_only_where_it_runs() {
    let d = drawn();
    let on = |angle: f64| DVec2::new(0.0, 20.0) + varde_sketch::angle::from_angle(angle) * 5.0;
    assert_eq!(hit(&d.sketch, on(0.8), 0.2), Some(d.arc));
    // On its circle, but past its end.
    assert_eq!(hit(&d.sketch, on(3.9), 0.2), None);
}

/// Looking straight down at the XY plane, 20 sketch units across a 200 by
/// 200 viewport with the origin in its middle.
fn top() -> Projector {
    Projector::new(&top_camera(), OriginPlane::XY.placement(), 200.0, 200.0).unwrap()
}

/// The screen position of the sketch point at `x`, `y` in [`top`].
fn screen(x: f64, y: f64) -> DVec2 {
    top().project(DVec2::new(x, y)).unwrap()
}

#[test]
fn a_box_dragged_right_selects_what_is_inside() {
    let d = drawn();
    let select = |a: DVec2, b: DVec2| {
        let mut ids = in_box(
            &d.sketch,
            &top(),
            ScreenBox::new(a, b),
            BoxMode::dragged(a, b),
        );
        ids.sort();
        ids
    };
    assert_eq!(
        BoxMode::dragged(screen(0.0, 0.0), screen(1.0, 0.0)),
        BoxMode::Inside
    );
    // Around the line and its points.
    let mut line = vec![d.a, d.b, d.line];
    line.sort();
    assert_eq!(select(screen(-1.0, 1.0), screen(11.0, -1.0)), line);
    // Half the circle isn't the circle, but its centre.
    assert_eq!(select(screen(14.0, 6.0), screen(21.0, -6.0)), [d.c]);
    // Upside down, it's the same box.
    assert_eq!(select(screen(-1.0, -1.0), screen(11.0, 1.0)), line);
}

#[test]
fn a_box_dragged_left_selects_what_it_touches() {
    let d = drawn();
    let select = |a: DVec2, b: DVec2| {
        assert_eq!(BoxMode::dragged(a, b), BoxMode::Touching);
        let mut ids = in_box(&d.sketch, &top(), ScreenBox::new(a, b), BoxMode::Touching);
        ids.sort();
        ids
    };
    // Across the middle of the line, missing its points.
    assert_eq!(select(screen(6.0, 1.0), screen(4.0, -1.0)), [d.line]);
    // Across the circle's edge, and its centre.
    let mut both = vec![d.c, d.circle];
    both.sort();
    assert_eq!(select(screen(21.0, 6.0), screen(14.0, -6.0)), both);
    // Inside the circle, touching nothing.
    assert!(select(screen(23.0, 1.0), screen(21.0, -1.0)).is_empty());
    // A segment passing through without an end inside.
    assert_eq!(select(screen(4.0, 26.0), screen(2.0, 22.0)), [d.arc]);
}

#[test]
fn a_segment_meets_a_box_it_crosses_or_ends_in() {
    let area = ScreenBox::new(DVec2::ZERO, DVec2::splat(10.0));
    assert!(area.meets(DVec2::new(-5.0, 5.0), DVec2::new(15.0, 5.0)));
    assert!(area.meets(DVec2::new(5.0, 5.0), DVec2::new(5.0, 5.0)));
    assert!(area.meets(DVec2::new(-5.0, -5.0), DVec2::new(15.0, 15.0)));
    assert!(!area.meets(DVec2::new(-5.0, 5.0), DVec2::new(-1.0, 50.0)));
    assert!(!area.meets(DVec2::new(11.0, -5.0), DVec2::new(11.0, 15.0)));
    assert!(!area.meets(DVec2::new(-5.0, 12.0), DVec2::new(12.0, 30.0)));
}

#[test]
fn a_segment_is_cut_to_the_box() {
    let area = ScreenBox::new(DVec2::ZERO, DVec2::splat(10.0));
    let (a, b) = area
        .clip(DVec2::new(-5.0, 5.0), DVec2::new(15.0, 5.0))
        .unwrap();
    assert_eq!((a, b), (DVec2::new(0.0, 5.0), DVec2::new(10.0, 5.0)));
    let inside = (DVec2::new(2.0, 3.0), DVec2::new(4.0, 1.0));
    assert_eq!(area.clip(inside.0, inside.1), Some(inside));
    assert_eq!(area.clip(DVec2::NAN, DVec2::ONE), None);
}

#[test]
fn a_box_has_its_corners_whichever_way_it_was_dragged() {
    let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)].map(DVec2::from);
    let dragged = ScreenBox::new(DVec2::new(10.0, 0.0), DVec2::new(0.0, 5.0));
    assert_eq!(dragged.corners(), corners);
}

#[test]
fn the_origin_and_axes_are_hit_after_the_sketch_s_own() {
    let d = drawn();
    let hit = |x, y| hit(&d.sketch, DVec2::new(x, y), 0.5);
    // `a` is at the origin: the sketch's point first.
    assert_eq!(hit(0.1, 0.1), Some(d.a));
    let empty = Sketch::default();
    assert_eq!(
        super::hit(&empty, DVec2::new(0.2, -0.1), 0.5),
        Some(Id::ORIGIN)
    );
    // The line along the x axis before the axis, the axis past its end.
    assert_eq!(hit(5.0, 0.3), Some(d.line));
    assert_eq!(hit(-7.0, 0.3), Some(Id::X_AXIS));
    assert_eq!(hit(0.4, -7.0), Some(Id::Y_AXIS));
}

#[test]
fn fillet_and_chamfer_hit_the_corners_where_lines_meet() {
    let mut sketch = Sketch::default();
    let corner = sketch.add_point(DVec2::ZERO).unwrap();
    let along = sketch.add_point(DVec2::new(10.0, 0.0)).unwrap();
    let up = sketch.add_point(DVec2::new(0.0, 10.0)).unwrap();
    for (start, end) in [(corner, along), (up, corner)] {
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    assert_eq!(hit_corner(&sketch, DVec2::new(0.5, 0.5), 1.0), Some(corner));
    // Nearer the lines' other ends, which make no corner, it's none.
    assert_eq!(hit_corner(&sketch, DVec2::new(9.8, 0.1), 1.0), None);
    assert_eq!(hit_corner(&sketch, DVec2::new(2.0, 2.0), 1.0), None);
    assert_eq!(hit_corner(&sketch, DVec2::NAN, 1.0), None);
}

#[test]
fn a_handle_is_hit_by_its_tip_along_both_arms_after_curves() {
    use varde_sketch::{Handle, Spline};
    let mut sketch = Sketch::default();
    let mut point = |x, y| sketch.add_point(DVec2::new(x, y)).unwrap();
    let (a, b, c, tip) = (
        point(0.0, 0.0),
        point(10.0, 10.0),
        point(20.0, 0.0),
        point(14.0, 10.0),
    );
    let mut spline = Spline::through(vec![a, b, c], false);
    spline.handles.push(Handle { at: b, tip });
    let id = sketch.add_curve(Curve::Spline(spline), false).unwrap();
    // On the tip's arm and on the mirrored one, as a line of its own.
    assert_eq!(
        hit(&sketch, DVec2::new(13.0, 10.2), 0.5),
        Some(Id::handle(tip))
    );
    assert_eq!(
        hit(&sketch, DVec2::new(7.0, 9.8), 0.5),
        Some(Id::handle(tip))
    );
    assert_eq!(
        overlaps(&sketch, DVec2::new(7.0, 9.8), 0.5),
        [Id::handle(tip)]
    );
    // At the fit point, the spline runs along it: the spline first.
    assert_eq!(hit(&sketch, DVec2::new(10.4, 10.1), 0.25), Some(id));
}
