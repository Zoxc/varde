use varde_document::OriginPlane;
use varde_sketch::Selectable;

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
    assert_eq!(hit(0.2, 0.1, 0.5), Some(Selectable::Item(d.a)));
    assert_eq!(hit(9.9, 0.0, 0.5), Some(Selectable::Item(d.b)));
    assert_eq!(hit(5.0, 0.3, 0.5), Some(Selectable::Item(d.line)));
    // The circle's centre is its point, the circle its edge.
    assert_eq!(hit(20.0, 0.0, 0.5), Some(Selectable::Item(d.c)));
    assert_eq!(hit(25.2, 0.0, 0.5), Some(Selectable::Item(d.circle)));
    assert_eq!(hit(20.0, -4.7, 0.5), Some(Selectable::Item(d.circle)));
    assert_eq!(hit(30.0, 30.4, 0.5), Some(Selectable::Item(d.lone)));
    // Nothing within the tolerance.
    assert_eq!(hit(5.0, 1.0, 0.5), None);
    assert_eq!(hit(20.0, 2.0, 0.5), None);
    assert_eq!(hit(f64::NAN, 0.0, 0.5), None);
}

#[test]
fn the_nearest_wins() {
    let d = drawn();
    // Both ends of the line are within 6, the nearer wins.
    assert_eq!(
        hit(&d.sketch, DVec2::new(4.0, 0.0), 6.0),
        Some(Selectable::Item(d.a))
    );
    assert_eq!(
        hit(&d.sketch, DVec2::new(6.0, 0.0), 6.0),
        Some(Selectable::Item(d.b))
    );
    // Between the line and the circle, off their points.
    assert_eq!(
        hit(&d.sketch, DVec2::new(8.0, 2.5), 3.0),
        Some(Selectable::Item(d.line))
    );
    assert_eq!(
        hit(&d.sketch, DVec2::new(14.0, 2.5), 3.0),
        Some(Selectable::Item(d.circle))
    );
}

#[test]
fn an_arc_is_hit_only_where_it_runs() {
    let d = drawn();
    let on = |angle: f64| DVec2::new(0.0, 20.0) + varde_sketch::angle::from_angle(angle) * 5.0;
    assert_eq!(hit(&d.sketch, on(0.8), 0.2), Some(Selectable::Item(d.arc)));
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
    let mut line = [d.a, d.b, d.line].map(Selectable::Item).to_vec();
    line.sort();
    assert_eq!(select(screen(-1.0, 1.0), screen(11.0, -1.0)), line);
    // Half the circle isn't the circle, but its centre.
    assert_eq!(
        select(screen(14.0, 6.0), screen(21.0, -6.0)),
        [Selectable::Item(d.c)]
    );
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
    assert_eq!(
        select(screen(6.0, 1.0), screen(4.0, -1.0)),
        [Selectable::Item(d.line)]
    );
    // Across the circle's edge, and its centre.
    let mut both = [d.c, d.circle].map(Selectable::Item).to_vec();
    both.sort();
    assert_eq!(select(screen(21.0, 6.0), screen(14.0, -6.0)), both);
    // Inside the circle, touching nothing.
    assert!(select(screen(23.0, 1.0), screen(21.0, -1.0)).is_empty());
    // A segment passing through without an end inside.
    assert_eq!(
        select(screen(4.0, 26.0), screen(2.0, 22.0)),
        [Selectable::Item(d.arc)]
    );
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
    assert_eq!(hit(0.1, 0.1), Some(Selectable::Item(d.a)));
    let empty = Sketch::default();
    assert_eq!(
        super::hit(&empty, DVec2::new(0.2, -0.1), 0.5),
        Some(Selectable::Item(Id::ORIGIN))
    );
    // The line along the x axis before the axis, the axis past its end.
    assert_eq!(hit(5.0, 0.3), Some(Selectable::Item(d.line)));
    assert_eq!(hit(-7.0, 0.3), Some(Selectable::Item(Id::X_AXIS)));
    assert_eq!(hit(0.4, -7.0), Some(Selectable::Item(Id::Y_AXIS)));
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
fn a_handle_is_hit_by_its_tip_along_both_arms_before_curves() {
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
        Some(Selectable::HandleLine(tip))
    );
    assert_eq!(
        hit(&sketch, DVec2::new(7.0, 9.8), 0.5),
        Some(Selectable::HandleLine(tip))
    );
    assert_eq!(
        overlaps(&sketch, DVec2::new(7.0, 9.8), 0.5),
        [Selectable::HandleLine(tip)]
    );
    // At the fit point, where the spline runs along it, the handle
    // first; off its arms, the spline.
    assert_eq!(
        hit(&sketch, DVec2::new(10.4, 10.1), 0.25),
        Some(Selectable::HandleLine(tip))
    );
    assert_eq!(
        overlaps(&sketch, DVec2::new(10.4, 10.1), 0.25),
        [Selectable::HandleLine(tip), Selectable::Item(id)]
    );
    let flat = sketch.flatten(&sketch.curve(id).unwrap().curve).unwrap();
    let along = flat[flat.len() / 4];
    assert_eq!(hit(&sketch, along, 0.25), Some(Selectable::Item(id)));
}

#[test]
fn a_handle_s_ends_go_before_arms() {
    use varde_sketch::{Handle, Spline};
    let mut sketch = Sketch::default();
    let mut point = |x, y| sketch.add_point(DVec2::new(x, y)).unwrap();
    let (a, b, c, tip) = (
        point(0.0, 0.0),
        point(10.0, 10.0),
        point(20.0, 0.0),
        point(14.0, 10.0),
    );
    let (d, e, f, across) = (
        point(0.0, 20.0),
        point(6.3, 9.0),
        point(12.0, 20.0),
        point(6.3, 13.0),
    );
    for (ends, at, tip) in [([a, b, c], b, tip), ([d, e, f], e, across)] {
        let mut spline = Spline::through(ends.to_vec(), false);
        spline.handles.push(Handle { at, tip });
        sketch.add_curve(Curve::Spline(spline), false).unwrap();
    }
    // By the first handle's mirrored end at (6, 10), nearer the second's
    // arm from (6.3, 5) to (6.3, 13): the end, as an end of its own.
    assert_eq!(
        hit(&sketch, DVec2::new(6.2, 10.2), 0.5),
        Some(Selectable::HandleEnd(tip))
    );
    // Along the second's arm, away from any end, the second.
    assert_eq!(
        hit(&sketch, DVec2::new(6.2, 11.5), 0.5),
        Some(Selectable::HandleLine(across))
    );
    // Along the first's arm, nearing its tip at (14, 10) but farther than
    // the tolerance, the tip: an end reaches farther than an arm.
    assert_eq!(
        hit(&sketch, DVec2::new(13.3, 10.1), 0.5),
        Some(Selectable::Item(tip))
    );
    // Farther still, the arm.
    assert_eq!(
        hit(&sketch, DVec2::new(12.5, 10.1), 0.5),
        Some(Selectable::HandleLine(tip))
    );
}

#[test]
fn a_handle_s_mirrored_end_is_hit_listed_and_boxed_as_a_point() {
    let mut sketch = Sketch::default();
    let (_, fit, tip) = crate::testing::handled_spline(&mut sketch);
    // The tip at (13, 7) about the fit point at (10, 4): the end at (7, 1).
    let end = DVec2::new(7.0, 1.0);
    assert_eq!(sketch.handle_end(tip), Some(end));
    assert_eq!(
        hit(&sketch, end + DVec2::new(0.1, 0.0), 0.5),
        Some(Selectable::HandleEnd(tip))
    );
    // The tip is the point it is.
    assert_eq!(
        hit(&sketch, DVec2::new(13.1, 7.0), 0.5),
        Some(Selectable::Item(tip))
    );
    // Listed before the handle as a line.
    assert_eq!(
        overlaps(&sketch, end, 0.1),
        [Selectable::HandleEnd(tip), Selectable::HandleLine(tip)]
    );
    // A box round it holds it; one round its fit point alone doesn't.
    let boxed = |a: DVec2, b: DVec2| {
        let (a, b) = (screen(a.x, a.y), screen(b.x, b.y));
        in_box(&sketch, &top(), ScreenBox::new(a, b), BoxMode::Inside)
    };
    assert_eq!(
        boxed(DVec2::new(6.0, 2.0), DVec2::new(8.0, 0.0)),
        [Selectable::HandleEnd(tip)]
    );
    assert_eq!(
        boxed(DVec2::new(9.0, 5.0), DVec2::new(11.0, 3.0)),
        [Selectable::Item(fit[1])]
    );
}
