#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{PI, TAU};

use glam::DVec2;

use super::bezier::{self, Path};
use super::*;
use crate::intersect::{Geom, meet};
use crate::testing::{self, DESIGN, arc, circle, geom, line, point};
use crate::{Kind, SketchError, analyse};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Fit points on a wave, not evenly spaced.
fn wave() -> Vec<DVec2> {
    vec![
        v(0.0, 0.0),
        v(2.0, 3.0),
        v(5.0, 2.5),
        v(6.0, -1.0),
        v(9.0, -2.0),
        v(12.0, 1.0),
    ]
}

/// `count` points round the circle about `center` of `radius`.
fn ring(center: DVec2, radius: f64, count: usize) -> Vec<DVec2> {
    (0..count)
        .map(|i| center + radius * DVec2::from_angle(TAU * i as f64 / count as f64))
        .collect()
}

/// The spline through `fit`, with tips for the handles at `handles`.
fn through(fit: &[DVec2], closed: bool, handles: &[(usize, DVec2)]) -> BSpline {
    let at: Vec<usize> = handles.iter().map(|&(i, _)| i).collect();
    let tips: Vec<DVec2> = handles.iter().map(|&(_, tip)| tip).collect();
    Interpolation::at_chords(fit, closed, &at)
        .unwrap()
        .spline(fit, &tips)
        .unwrap()
}

/// A sketch with the spline through `fit`, and its id.
fn sketch_with(fit: &[DVec2], closed: bool) -> (Sketch, Id) {
    let mut sketch = Sketch::default();
    let places: Vec<(f64, f64)> = fit.iter().map(|p| (p.x, p.y)).collect();
    let (spline, _) = testing::spline(&mut sketch, &places, closed);
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    (sketch, spline)
}

fn path_of(spline: &BSpline) -> Path {
    spline.path()
}

/// Parameters from 0 to 1, `count` of them past 0.
fn params(count: usize) -> impl Iterator<Item = f64> {
    (0..=count).map(move |i| i as f64 / count as f64)
}

#[test]
fn chord_params_run_from_zero_to_one_by_the_chords() {
    let params = chord_params(&[v(0.0, 0.0), v(3.0, 4.0), v(3.0, 9.0)], false);
    assert_eq!(params, vec![0.0, 0.5, 1.0]);
    let closed = chord_params(&[v(0.0, 0.0), v(3.0, 0.0), v(3.0, 4.0)], true);
    assert_eq!(closed[..2], [0.0, 0.25]);
    assert!((closed[2] - 7.0 / 12.0).abs() < 1e-15);
    // Points at one place still get distinct parameters.
    let same = chord_params(&[v(1.0, 1.0); 3], false);
    assert_eq!(same, vec![0.0, 0.5, 1.0]);
    let repeated = chord_params(&[v(0.0, 0.0), v(0.0, 0.0), v(1.0, 0.0)], false);
    assert!(repeated[1] > 0.0 && repeated[1] < 1e-5, "{repeated:?}");
}

#[test]
fn interpolation_passes_through_its_fit_points() {
    let fit = wave();
    for closed in [false, true] {
        for handles in [vec![], vec![(0, v(1.0, 1.0)), (3, v(7.0, -3.0))]] {
            let spline = through(&fit, closed, &handles);
            let params = chord_params(&fit, closed);
            for (&t, &p) in params.iter().zip(&fit) {
                assert!(spline.point(t).distance(p) < 1e-9, "{closed} {t}");
            }
        }
    }
}

#[test]
fn handles_set_the_tangent_and_its_strength() {
    let fit = wave();
    for closed in [false, true] {
        let handles = [(0, v(1.0, 2.0)), (2, v(6.0, 4.0)), (5, v(13.0, 0.0))];
        let spline = through(&fit, closed, &handles);
        let params = chord_params(&fit, closed);
        for (i, tip) in handles {
            let [_, d1, _] = spline.eval(params[i]);
            let expected = (tip - fit[i]) * handle_scale(&params, closed, i);
            assert!(
                d1.distance(expected) < 1e-8,
                "{closed} {i}: {d1} {expected}"
            );
        }
    }
    // A handle at every point still interpolates.
    let all: Vec<(usize, DVec2)> = (0..fit.len()).map(|i| (i, fit[i] + v(1.0, 0.5))).collect();
    let spline = through(&fit, false, &all);
    let params = chord_params(&fit, false);
    for (&t, &p) in params.iter().zip(&fit) {
        assert!(spline.point(t).distance(p) < 1e-9);
    }
}

#[test]
fn two_fit_points_without_handles_are_a_straight_line() {
    let (a, b) = (v(1.0, 2.0), v(7.0, -1.0));
    let spline = through(&[a, b], false, &[]);
    for t in params(10) {
        let p = spline.point(t);
        assert!((b - a).perp_dot(p - a).abs() < 1e-9);
    }
}

#[test]
fn the_interpolation_is_linear_in_the_points() {
    let fit = wave();
    let params = chord_params(&fit, false);
    let interpolation = Interpolation::new(&params, false, &[2]).unwrap();
    let tip = [v(6.0, 5.0)];
    let control = interpolation.control(&fit, &tip).unwrap();
    // Moving a point by δ moves each control point by its weight times δ.
    let delta = v(0.5, -0.25);
    let mut moved = fit.clone();
    moved[3] += delta;
    let after = interpolation.control(&moved, &tip).unwrap();
    for (c, (before, after)) in control.iter().zip(&after).enumerate() {
        let weight = interpolation.weights(c)[3];
        assert!((*after - *before - delta * weight).length() < 1e-12);
    }
    assert_eq!(interpolation.controls(), fit.len() + 2 + 1);
    assert!(interpolation.control(&fit, &[]).is_none());
}

#[test]
fn derivatives_match_finite_differences() {
    let fit = wave();
    let splines = [
        through(&fit, false, &[]),
        through(&fit, true, &[]),
        through(&fit, false, &[(2, v(6.0, 4.0))]),
        through(&fit, true, &[(0, v(-1.0, 1.0))]),
    ];
    let h = 1e-6;
    for spline in &splines {
        for t in params(37).map(|t| t.clamp(0.01, 0.99)) {
            let [_, d1, d2] = spline.eval(t);
            let fd1 = (spline.point(t + h) - spline.point(t - h)) / (2.0 * h);
            let fd2 = (spline.eval(t + h)[1] - spline.eval(t - h)[1]) / (2.0 * h);
            assert!(d1.distance(fd1) < 1e-4 * d1.length().max(1.0), "{t}");
            assert!(d2.distance(fd2) < 1e-3 * d2.length().max(1.0), "{t}");
        }
    }
}

#[test]
fn a_spline_through_points_on_a_circle_curves_as_the_circle() {
    let radius = 5.0;
    let fit = ring(v(3.0, -2.0), radius, 16);
    let spline = through(&fit, true, &[]);
    for t in params(64) {
        let curvature = spline.curvature(t);
        assert!(
            (curvature - 1.0 / radius).abs() < 0.02 / radius,
            "{t}: {curvature}"
        );
        assert!((spline.point(t).distance(v(3.0, -2.0)) - radius).abs() < 1e-3 * radius);
    }
}

#[test]
fn a_closed_spline_is_twice_continuous_round_and_at_its_knots() {
    let fit = wave();
    for handles in [vec![], vec![(0, v(1.0, -1.0)), (4, v(10.0, -3.0))]] {
        let spline = through(&fit, true, &handles);
        let (start, end) = (spline.eval(0.0), spline.eval(1.0 - 1e-12));
        for order in 0..3 {
            let scale = start[order].length().max(1.0);
            assert!(start[order].distance(end[order]) < 1e-6 * scale, "{order}");
        }
        // Either side of every knot.
        for &k in spline.knots() {
            let (before, after) = (spline.eval(k - 1e-10), spline.eval(k + 1e-10));
            for order in 0..3 {
                let scale = before[order].length().max(1.0);
                assert!(before[order].distance(after[order]) < 1e-5 * scale);
            }
        }
    }
}

#[test]
fn bezier_segments_are_the_spline() {
    let fit = wave();
    for spline in [
        through(&fit, false, &[]),
        through(&fit, true, &[(0, v(-1.0, 1.0)), (3, v(7.0, 0.0))]),
    ] {
        let path = path_of(&spline);
        assert_eq!(path.last(), 1.0);
        for t in params(101) {
            assert!(path.at(t).distance(spline.point(t)) < 1e-9, "{t}");
            let (d1, d2) = path.derivatives(t);
            let [_, e1, e2] = spline.eval(t);
            assert!(d1.distance(e1) < 1e-7 * e1.length().max(1.0), "{t}");
            assert!(d2.distance(e2) < 1e-6 * e2.length().max(1.0), "{t}");
        }
    }
}

#[test]
fn flattening_stays_close_with_bounded_segments() {
    let fit = wave();
    let spline = through(&fit, false, &[]);
    let path = path_of(&spline);
    let polyline = path.flatten();
    let (min, max) = path.bounds();
    let tolerance = (max - min).max_element() * 5e-4;
    assert!(polyline[0].distance(fit[0]) < 1e-9);
    assert!(polyline.last().unwrap().distance(fit[5]) < 1e-9);
    assert!(polyline.len() > 10);
    assert!(polyline.len() <= 1 + crate::CIRCLE_SEGMENTS * (fit.len() - 1));
    for pair in polyline.windows(2) {
        for k in 1..4 {
            let at = pair[0].lerp(pair[1], k as f64 / 4.0);
            let near = path.at(path.closest(at));
            assert!(near.distance(at) <= tolerance * 1.01, "{at}");
        }
    }
    // Closed, it ends where it starts.
    let closed = path_of(&through(&fit, true, &[])).flatten();
    assert_eq!(closed.first(), closed.last());
}

#[test]
fn the_closest_place_is_the_nearest_of_all() {
    let path = path_of(&through(&wave(), false, &[]));
    let dense: Vec<DVec2> = params(20_000).map(|t| path.at(t)).collect();
    for at in [
        v(3.0, 0.0),
        v(6.0, 4.0),
        v(-3.0, -1.0),
        v(10.0, -0.5),
        v(4.0, 2.7),
    ] {
        let found = path.at(path.closest(at)).distance(at);
        let brute = dense
            .iter()
            .map(|p| p.distance(at))
            .fold(f64::INFINITY, f64::min);
        assert!(found <= brute + 1e-9, "{at}: {found} {brute}");
        assert!(found >= brute - 1e-3);
    }
}

#[test]
fn span_bounds_and_the_bulge_are_exact() {
    let path = path_of(&through(&ring(DVec2::ZERO, 2.0, 12), true, &[]));
    let (min, max) = path.span_bounds(0.0, 1.0);
    let dense: Vec<DVec2> = params(20_000).map(|t| path.at(t)).collect();
    let (dmin, dmax) = dense
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(a, b), &p| {
            (a.min(p), b.max(p))
        });
    assert!(min.distance(dmin) < 1e-6 && max.distance(dmax) < 1e-6);
    // The whole loop's area, its bulge over a chord of nothing.
    let area = path.bulge(0.0, 1.0);
    assert!((area - PI * 4.0).abs() < 0.01 * PI * 4.0, "{area}");
    assert!((path.bulge(1.0, 0.0) + area).abs() < 1e-9);
    // Round the other way it winds once, not round a place outside.
    let mut work = 0;
    assert!((path.winding(0.0, 1.0, v(0.3, -0.2), &mut work) - TAU).abs() < 1e-9);
    assert!(path.winding(0.0, 1.0, v(3.0, 0.0), &mut work).abs() < 1e-9);
    assert!(work > 0);
    let length = path.length(0.0, 1.0);
    assert!((length - TAU * 2.0).abs() < 0.01 * TAU * 2.0, "{length}");
}

/// Where `a` and `b` meet, as places, merged within `1e-6`.
fn places(a: &Geom, b: &Geom) -> Vec<DVec2> {
    let mut found = Vec::new();
    meet(a, b, 1e-9, &mut found);
    let mut places: Vec<DVec2> = Vec::new();
    for (ua, ub) in found {
        let (pa, pb) = (a.at(ua), b.at(ub));
        assert!(pa.distance(pb) < 1e-7, "{pa} {pb}");
        if !places.iter().any(|p| p.distance(pa) < 1e-6) {
            places.push(pa);
        }
    }
    places
}

#[test]
fn a_spline_meets_lines_circles_and_arcs() {
    let (sketch, spline) = sketch_with(&wave(), false);
    let wave = geom(&sketch, spline);
    let segment = |x0: f64, y0: f64, x1: f64, y1: f64| Geom::Segment {
        start: v(x0, y0),
        end: v(x1, y1),
    };
    // The x axis, crossed twice between its ends (and at its start).
    let found = places(&wave, &segment(-1.0, 0.0, 13.0, 0.0));
    assert_eq!(found.len(), 3, "{found:?}");
    for p in &found {
        assert!(p.y.abs() < 1e-9);
    }
    // Only as far as the segment goes.
    assert_eq!(places(&segment(1.0, 0.0, 13.0, 0.0), &wave).len(), 2);
    assert!(places(&wave, &segment(0.0, 10.0, 12.0, 10.0)).is_empty());
    // A circle round its middle.
    let round = |x: f64, y: f64, radius: f64, begin: f64, sweep: f64| Geom::Round {
        center: v(x, y),
        radius,
        begin,
        sweep,
    };
    let found = places(&wave, &round(6.0, 0.0, 3.0, 0.0, TAU));
    assert_eq!(found.len(), 2, "{found:?}");
    for p in &found {
        assert!((p.distance(v(6.0, 0.0)) - 3.0).abs() < 1e-9);
    }
    // Its upper half only.
    assert_eq!(places(&round(6.0, 0.0, 3.0, 0.0, PI), &wave).len(), 1);
}

#[test]
fn a_spline_touching_a_line_meets_it_once() {
    // Symmetric about x = 5, its top at y = 4.
    let (sketch, spline) = sketch_with(&[v(0.0, 0.0), v(5.0, 4.0), v(10.0, 0.0)], false);
    let hump = geom(&sketch, spline);
    let top = hump.at(0.5).y;
    let found = places(
        &hump,
        &Geom::Segment {
            start: v(0.0, top),
            end: v(10.0, top),
        },
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].distance(v(5.0, top)) < 1e-4);
}

#[test]
fn splines_meet_each_other() {
    let (mut sketch, a) = sketch_with(&wave(), false);
    let points = [v(1.0, -3.0), v(4.0, 4.0), v(8.0, -4.0), v(11.0, 3.0)]
        .map(|p| point(&mut sketch, p.x, p.y))
        .to_vec();
    let b = sketch
        .add_curve(Curve::Spline(Spline::through(points, false)), false)
        .unwrap();
    let (ga, gb) = (geom(&sketch, a), geom(&sketch, b));
    let found = places(&ga, &gb);
    let reverse = places(&gb, &ga);
    assert_eq!(found.len(), reverse.len());
    // Checked against a dense polyline of each.
    let polyline = |g: &Geom| g.polyline(0.0, 1.0);
    let (pa, pb) = (polyline(&ga), polyline(&gb));
    let mut crossings = 0;
    for p in pa.windows(2) {
        for q in pb.windows(2) {
            if let Some((t, u)) = crate::crossing(p[0], p[1] - p[0], q[0], q[1] - q[0])
                && (0.0..1.0).contains(&t)
                && (0.0..1.0).contains(&u)
            {
                crossings += 1;
            }
        }
    }
    assert!(crossings >= 3);
    assert_eq!(found.len(), crossings, "{found:?}");
}

#[test]
fn a_straight_spline_meets_a_curved_one_in_a_few_steps() {
    // A straight piece is flat at once, but its box is the whole line's:
    // were only the curved one halved, every piece of it in that box
    // would be halved down to the tolerance, which runs out of steps.
    let (mut sketch, a) = sketch_with(&wave(), false);
    let points = [v(-1.0, 1.0), v(13.0, -0.5)]
        .map(|p| point(&mut sketch, p.x, p.y))
        .to_vec();
    let b = sketch
        .add_curve(Curve::Spline(Spline::through(points, false)), false)
        .unwrap();
    let (ga, gb) = (geom(&sketch, a), geom(&sketch, b));
    for (a, b) in [(&ga, &gb), (&gb, &ga)] {
        let mut found = Vec::new();
        let work = bezier::crossings(a, b, 1e-9, &mut found);
        assert!(work < bezier::MAX_MEET_STEPS, "{work}");
    }
    // Crossing the wave three times, where the dense polylines do.
    let found = places(&ga, &gb);
    let (pa, pb) = (ga.polyline(0.0, 1.0), gb.polyline(0.0, 1.0));
    let mut crossings = 0;
    for p in pa.windows(2) {
        for q in pb.windows(2) {
            if let Some((t, u)) = crate::crossing(p[0], p[1] - p[0], q[0], q[1] - q[0])
                && (0.0..1.0).contains(&t)
                && (0.0..1.0).contains(&u)
            {
                crossings += 1;
            }
        }
    }
    assert!(crossings >= 3, "{crossings}");
    assert_eq!(found.len(), crossings, "{found:?}");
}

#[test]
fn a_figure_of_eight_crosses_itself_once() {
    let fit = [v(0.0, 0.0), v(4.0, 3.0), v(8.0, 0.0), v(4.0, -3.0)];
    // Crossing at the middle: round the left lobe one way, the right the
    // other.
    let eight = [
        v(-4.0, 0.0),
        v(-2.0, 2.0),
        v(2.0, -2.0),
        v(4.0, 0.0),
        v(2.0, 2.0),
        v(-2.0, -2.0),
    ];
    let mut found = Vec::new();
    bezier::self_crossings(&path_of(&through(&eight, true, &[])), 1e-9, &mut found);
    let path = path_of(&through(&eight, true, &[]));
    let mut places: Vec<DVec2> = Vec::new();
    for (u, w) in found {
        assert!(path.at(u).distance(path.at(w)) < 1e-7);
        if !places.iter().any(|p| p.distance(path.at(u)) < 1e-6) {
            places.push(path.at(u));
        }
    }
    assert_eq!(places.len(), 1, "{places:?}");
    assert!(places[0].length() < 1e-6);
    // A simple loop doesn't.
    let mut none = Vec::new();
    bezier::self_crossings(&path_of(&through(&fit, true, &[])), 1e-9, &mut none);
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn converting_keeps_the_shape() {
    for closed in [false, true] {
        let (mut sketch, id) = sketch_with(&wave(), closed);
        let before = geom(&sketch, id);
        let fit: Vec<Id> = sketch.curve(id).unwrap().curve.points().collect();
        sketch.convert_spline(id, SplineKind::Control).unwrap();
        assert_eq!(sketch.check(&DESIGN), Ok(()));
        let Curve::Spline(control) = &sketch.curve(id).unwrap().curve else {
            panic!("still a spline");
        };
        assert_eq!(control.kind, SplineKind::Control);
        assert_eq!(control.points.len(), if closed { 6 } else { 8 });
        // An open spline keeps its ends, the rest of its fit points go.
        if !closed {
            assert_eq!(control.ends(), Some([fit[0], fit[5]]));
        }
        assert_eq!(sketch.points.len(), control.points.len());
        let after = geom(&sketch, id);
        for t in params(50) {
            assert!(before.at(t).distance(after.at(t)) < 1e-9, "{closed} {t}");
        }
        // And back through the places at its knots: those of the fit
        // points it had, so the same spline, an open one with handles at
        // its ends giving the tangents it had there.
        sketch.convert_spline(id, SplineKind::Through).unwrap();
        assert_eq!(sketch.check(&DESIGN), Ok(()));
        let again = geom(&sketch, id);
        assert_eq!(sketch.points.len(), if closed { 6 } else { 8 });
        for t in params(50) {
            assert!(before.at(t).distance(again.at(t)) < 1e-8, "{closed} {t}");
        }
    }
}

#[test]
fn converting_by_control_points_to_through_strays_a_little() {
    let mut sketch = Sketch::default();
    let places = [
        v(0.0, 0.0),
        v(1.0, 4.0),
        v(4.0, 5.0),
        v(7.0, 2.0),
        v(9.0, -2.0),
        v(12.0, -1.0),
        v(14.0, 3.0),
        v(17.0, 4.0),
    ];
    let points: Vec<Id> = places
        .iter()
        .map(|p| point(&mut sketch, p.x, p.y))
        .collect();
    let spline = Spline {
        kind: SplineKind::Control,
        knots: control_knots(&places, false),
        points,
        closed: false,
        handles: Vec::new(),
    };
    let id = sketch.add_curve(Curve::Spline(spline), false).unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    let before = geom(&sketch, id);
    sketch.convert_spline(id, SplineKind::Through).unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    let after = geom(&sketch, id);
    let Curve::Spline(fit) = &sketch.curve(id).unwrap().curve else {
        panic!("still a spline");
    };
    assert_eq!(fit.points.len(), 6);
    // Through the places at its knots, a little off between them.
    let mut most: f64 = 0.0;
    for t in params(200) {
        let near = after.at(after.closest(before.at(t)));
        most = most.max(near.distance(before.at(t)));
    }
    assert!(most < 0.03 * 17.0, "{most}");
    assert_eq!(fit.handles.len(), 2);
}

#[test]
fn a_spline_is_named_and_counted_as_one() {
    let (sketch, id) = sketch_with(&wave(), false);
    let entry = sketch.curve(id).unwrap();
    assert_eq!(entry.name(), "Spline 1");
    assert_eq!(sketch.kind(id), Some(Kind::Spline));
    assert_eq!(
        entry.curve.ends(),
        Some([sketch.points[0].id, sketch.points[5].id])
    );
    let (closed, id) = sketch_with(&wave(), true);
    assert_eq!(closed.curve(id).unwrap().curve.ends(), None);
}

#[test]
fn its_points_solve_as_free_points() {
    let (sketch, _) = sketch_with(&wave(), false);
    let analysis = analyse(&sketch);
    assert_eq!(analysis.freedom, 12);
    let solved = crate::solve(&sketch, &crate::Goal::Settle, &crate::Budget::default());
    assert!(solved.is_ok());
}

/// A spline of `kind` and `closed` on `count` new points, with `knots`.
fn spline_of(sketch: &mut Sketch, count: usize, kind: SplineKind, closed: bool) -> Spline {
    let places: Vec<DVec2> = ring(DVec2::ZERO, 10.0, count);
    let points = places.iter().map(|p| point(sketch, p.x, p.y)).collect();
    let knots = match kind {
        SplineKind::Through => Vec::new(),
        SplineKind::Control => control_knots(&places, closed),
    };
    Spline {
        kind,
        points,
        closed,
        handles: Vec::new(),
        knots,
    }
}

/// What checking a sketch with just `spline` gives, as the curve it adds.
fn checked(mut sketch: Sketch, spline: Spline) -> Result<(), SketchError> {
    sketch.add_curve(Curve::Spline(spline), false).unwrap();
    sketch.check(&DESIGN)
}

#[test]
fn check_refuses_splines_that_cant_be() {
    let refused = |sketch: &Sketch, spline: Spline| {
        let id = Id(sketch.next_id);
        assert_eq!(
            checked(sketch.clone(), spline),
            Err(SketchError::Spline(id))
        );
    };
    for (kind, closed, least) in [
        (SplineKind::Through, false, 2),
        (SplineKind::Through, true, 3),
        (SplineKind::Control, false, 4),
        (SplineKind::Control, true, 3),
    ] {
        let mut sketch = Sketch::default();
        let fine = spline_of(&mut sketch, least, kind, closed);
        assert_eq!(checked(sketch.clone(), fine), Ok(()), "{kind:?} {closed}");
        let mut sketch = Sketch::default();
        let few = spline_of(&mut sketch, least - 1, kind, closed);
        refused(&sketch, few);
        let mut sketch = Sketch::default();
        let many = spline_of(&mut sketch, MAX_SPLINE_POINTS + 1, kind, closed);
        refused(&sketch, many);
    }

    let mut sketch = Sketch::default();
    let base = spline_of(&mut sketch, 5, SplineKind::Through, false);
    let tip = point(&mut sketch, 3.0, 3.0);
    let other = point(&mut sketch, 4.0, 4.0);
    let handle = |at| Handle { at, tip };
    // A handle is at one of its points, once.
    let good = Spline {
        handles: vec![handle(base.points[1])],
        ..base.clone()
    };
    assert_eq!(checked(sketch.clone(), good), Ok(()));
    refused(
        &sketch,
        Spline {
            handles: vec![handle(other)],
            ..base.clone()
        },
    );
    refused(
        &sketch,
        Spline {
            handles: vec![
                handle(base.points[1]),
                Handle {
                    at: base.points[1],
                    tip: other,
                },
            ],
            ..base.clone()
        },
    );
    // Through fit points, no knots.
    refused(
        &sketch,
        Spline {
            knots: vec![0.5],
            ..base.clone()
        },
    );
    // A tip that's one of its points, or a point twice.
    let id = Id(sketch.next_id);
    let own_tip = Spline {
        handles: vec![Handle {
            at: base.points[0],
            tip: base.points[2],
        }],
        ..base.clone()
    };
    assert_eq!(
        checked(sketch.clone(), own_tip),
        Err(SketchError::Repeated {
            from: id,
            to: base.points[2]
        })
    );
    let mut twice = base.clone();
    twice.points[3] = twice.points[1];
    assert!(matches!(
        checked(sketch.clone(), twice),
        Err(SketchError::Repeated { .. })
    ));
    // A point it doesn't have.
    let mut missing = base.clone();
    missing.points[0] = Id(sketch.next_id + 5);
    assert!(checked(sketch.clone(), missing).is_err());

    // By control points: no handles, and knots as it takes them.
    let mut sketch = Sketch::default();
    let control = spline_of(&mut sketch, 6, SplineKind::Control, false);
    let tip = point(&mut sketch, 1.0, 1.0);
    for knots in [
        vec![0.5],
        vec![0.6, 0.4],
        vec![0.0, 0.5],
        vec![0.5, 1.0],
        vec![0.5, f64::NAN],
        vec![0.5, 0.5 + 1e-9],
    ] {
        refused(
            &sketch,
            Spline {
                knots,
                ..control.clone()
            },
        );
    }
    refused(
        &sketch,
        Spline {
            handles: vec![Handle {
                at: control.points[0],
                tip,
            }],
            ..control.clone()
        },
    );
    let mut sketch = Sketch::default();
    let periodic = spline_of(&mut sketch, 4, SplineKind::Control, true);
    for knots in [
        vec![0.0, 0.25, 0.5],
        vec![0.0, 0.25, 0.5, 1.0],
        vec![-0.1, 0.25, 0.5, 0.75],
        vec![0.1, 0.25, 0.5, 1.1 - 1e-9],
    ] {
        refused(
            &sketch,
            Spline {
                knots,
                ..periodic.clone()
            },
        );
    }
}

#[test]
fn deleting_a_fit_point_keeps_the_spline_while_it_can() {
    let (mut sketch, id) = sketch_with(&wave(), false);
    let tip = point(&mut sketch, 3.0, 5.0);
    let second = sketch.points[1].id;
    if let Curve::Spline(spline) = &mut sketch.curve_mut(id).unwrap().curve {
        spline.handles.push(Handle { at: second, tip });
    }
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    // Its handle's point goes, and the handle with its tip.
    sketch.delete(&[second]);
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    let points = |sketch: &Sketch| match &sketch.curve(id).unwrap().curve {
        Curve::Spline(spline) => (spline.points.len(), spline.handles.len()),
        _ => panic!("a spline"),
    };
    assert_eq!(points(&sketch), (5, 0));
    assert!(sketch.point(tip).is_none());
    // Down to two, a line through them, and then gone with its points.
    let ids: Vec<Id> = sketch.points.iter().map(|p| p.id).collect();
    sketch.delete(&ids[1..4]);
    assert_eq!(points(&sketch), (2, 0));
    sketch.delete(&[ids[0]]);
    assert!(sketch.curve(id).is_none());
    assert!(sketch.points.is_empty());
    assert_eq!(sketch.check(&DESIGN), Ok(()));
}

#[test]
fn deleting_a_tip_takes_the_handle_and_a_control_point_new_knots() {
    let (mut sketch, id) = sketch_with(&wave(), false);
    let tip = point(&mut sketch, 3.0, 5.0);
    let second = sketch.points[1].id;
    if let Curve::Spline(spline) = &mut sketch.curve_mut(id).unwrap().curve {
        spline.handles.push(Handle { at: second, tip });
    }
    sketch.delete(&[tip]);
    let Curve::Spline(spline) = &sketch.curve(id).unwrap().curve else {
        panic!("a spline");
    };
    assert!(spline.handles.is_empty());
    assert_eq!(spline.points.len(), 6);

    sketch.convert_spline(id, SplineKind::Control).unwrap();
    let first_inner = match &sketch.curve(id).unwrap().curve {
        Curve::Spline(spline) => spline.points[2],
        _ => panic!("a spline"),
    };
    sketch.delete(&[first_inner]);
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    let Curve::Spline(spline) = &sketch.curve(id).unwrap().curve else {
        panic!("a spline");
    };
    assert_eq!(spline.points.len(), 7);
    assert_eq!(spline.knots.len(), 3);
}

#[test]
fn a_closed_spline_is_a_profile_of_its_own() {
    let mut sketch = Sketch::default();
    let fit = ring(v(20.0, 5.0), 10.0, 8);
    let points = fit.iter().map(|p| point(&mut sketch, p.x, p.y)).collect();
    sketch
        .add_curve(Curve::Spline(Spline::through(points, true)), false)
        .unwrap();
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 1);
    let region = &profiles.regions[0];
    assert!(
        (region.area - PI * 100.0).abs() < 0.02 * PI * 100.0,
        "{}",
        region.area
    );
    assert_eq!(region.outer.len(), 1);
    assert!(region.holes.is_empty());
    assert!(profiles.region_at(v(20.0, 5.0)).is_some());
    assert!(profiles.open_ends.is_empty());

    // A circle inside it is a hole, and a region of its own.
    let center = point(&mut sketch, 20.0, 5.0);
    circle(&mut sketch, center, 3.0);
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 2);
    let ring_region = profiles
        .regions
        .iter()
        .find(|region| region.holes.len() == 1)
        .unwrap();
    assert!((ring_region.area - PI * 91.0).abs() < 0.02 * PI * 100.0);
}

#[test]
fn a_line_across_a_closed_spline_cuts_it_in_two() {
    let mut sketch = Sketch::default();
    let fit = ring(DVec2::ZERO, 10.0, 8);
    let points = fit.iter().map(|p| point(&mut sketch, p.x, p.y)).collect();
    sketch
        .add_curve(Curve::Spline(Spline::through(points, true)), false)
        .unwrap();
    let (a, b) = (
        point(&mut sketch, -15.0, 1.0),
        point(&mut sketch, 15.0, 1.0),
    );
    line(&mut sketch, a, b);
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 2);
    let total: f64 = profiles.regions.iter().map(|region| region.area).sum();
    assert!((total - PI * 100.0).abs() < 0.02 * PI * 100.0);
}

#[test]
fn a_figure_of_eight_is_two_profiles() {
    let eight = [
        v(-4.0, 0.0),
        v(-2.0, 2.0),
        v(2.0, -2.0),
        v(4.0, 0.0),
        v(2.0, 2.0),
        v(-2.0, -2.0),
    ];
    let (sketch, _) = sketch_with(&eight, true);
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 2);
    for region in &profiles.regions {
        assert!(region.area > 1.0, "{}", region.area);
    }
}

#[test]
fn an_open_spline_closes_a_profile_with_a_line_and_an_arc() {
    let mut sketch = Sketch::default();
    let places = [v(0.0, 0.0), v(3.0, 2.0), v(7.0, 1.0), v(10.0, 0.0)];
    let points: Vec<Id> = places
        .iter()
        .map(|p| point(&mut sketch, p.x, p.y))
        .collect();
    let (first, last) = (points[0], points[3]);
    sketch
        .add_curve(Curve::Spline(Spline::through(points, false)), false)
        .unwrap();
    // Back below it by a half circle.
    let center = point(&mut sketch, 5.0, 0.0);
    arc(&mut sketch, center, first, last);
    let profiles = sketch.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 1, "{:?}", profiles.open_ends);
    let area = profiles.regions[0].area;
    assert!(area > PI * 25.0 / 2.0, "{area}");
}

#[test]
fn a_spline_draws_and_is_near_where_it_is() {
    let (sketch, id) = sketch_with(&wave(), false);
    let polyline = sketch.flatten(&sketch.curve(id).unwrap().curve).unwrap();
    assert!(polyline.len() > 10);
    let near = sketch.nearest_on(id, v(5.0, 5.0)).unwrap();
    assert!(near.y > 2.0 && near.y < 3.5, "{near}");
    // All at one place, it has no shape but still draws.
    let (sketch, id) = sketch_with(&[v(1.0, 1.0); 3], false);
    assert!(Geom::of(&sketch, &sketch.curve(id).unwrap().curve).is_none());
    assert!(sketch.flatten(&sketch.curve(id).unwrap().curve).is_some());
}

#[test]
fn a_spline_is_mirrored_point_for_point() {
    let (mut sketch, id) = sketch_with(&wave(), false);
    let (a, b) = (
        point(&mut sketch, 0.0, -10.0),
        point(&mut sketch, 1.0, -10.0),
    );
    let about = line(&mut sketch, a, b);
    let edit = crate::SketchEdit::Mirror {
        ids: vec![id],
        about,
    };
    let mirrored = crate::testing::propose_it(&sketch, &edit).unwrap().sketch;
    let copy = mirrored.curves.last().unwrap();
    assert_eq!(copy.curve.kind(), Kind::Spline);
    let (before, after) = (geom(&mirrored, id), geom(&mirrored, copy.id));
    for t in params(20) {
        let p = before.at(t);
        assert!(after.at(t).distance(v(p.x, -20.0 - p.y)) < 1e-6, "{t}");
    }
}

#[test]
fn trimming_a_line_at_a_spline_ends_it_there() {
    let (mut sketch, _) = sketch_with(&wave(), false);
    let (a, b) = (point(&mut sketch, 3.0, -5.0), point(&mut sketch, 3.0, 8.0));
    let cut = line(&mut sketch, a, b);
    let edit = crate::SketchEdit::Trim {
        curve: cut,
        near: v(3.0, 7.0),
    };
    let trimmed = crate::testing::propose_it(&sketch, &edit).unwrap().sketch;
    let (start, end) = trimmed.line(cut).unwrap();
    assert!(start.distance(v(3.0, -5.0)) < 1e-9);
    let spline = geom(&trimmed, sketch.curves[0].id);
    assert!(spline.at(spline.closest(end)).distance(end) < 1e-6, "{end}");
}

#[test]
fn the_largest_spline_interpolates_with_a_handle_at_every_point() {
    for closed in [false, true] {
        let fit: Vec<DVec2> = (0..MAX_SPLINE_POINTS)
            .map(|i| {
                let t = i as f64 / MAX_SPLINE_POINTS as f64;
                v(100.0 * t, (TAU * 7.0 * t).sin() * 10.0)
            })
            .collect();
        let handles: Vec<(usize, DVec2)> =
            (0..fit.len()).map(|i| (i, fit[i] + v(0.3, 0.1))).collect();
        let spline = through(&fit, closed, &handles);
        let params = chord_params(&fit, closed);
        for (&t, &p) in params.iter().zip(&fit) {
            assert!(spline.point(t).distance(p) < 1e-8, "{closed} {t}");
        }
    }
}

#[test]
fn the_closest_place_is_found_on_a_long_segment_passing_by_twice() {
    // Closed, the way back from the last point to the first is one long
    // segment, which turns back on itself: a place on it has samples on
    // the other pass nearer than any on its own, or has the distance
    // grow before it falls to it.
    let fit = [
        v(0.0, 0.0),
        v(10.0, 6.0),
        v(20.0, 4.0),
        v(30.0, -3.0),
        v(40.0, 2.0),
    ];
    let spline = through(&fit, true, &[(1, v(13.0, 7.0))]);
    let path = spline.path();
    for i in 0..=1000 {
        let t = i as f64 / 1000.0;
        let place = spline.point(t);
        let found = path.closest(place);
        assert!(spline.point(found).distance(place) < 1e-9, "{t}: {found}");
    }
}

/// Control points round a wave, and the knots a spline by them is made
/// with, open or `closed`.
fn by_control(closed: bool) -> BSpline {
    let control = wave();
    BSpline::new(&control_knots(&control, closed), control, closed).unwrap()
}

#[test]
fn a_knot_inserted_leaves_the_spline_as_it_was() {
    for closed in [false, true] {
        let spline = by_control(closed);
        for t in [0.05, 0.31, 0.5, 0.77, 0.999, if closed { 1.2 } else { 0.6 }] {
            let more = spline.with_knot(t).unwrap();
            assert_eq!(more.control().len(), spline.control().len() + 1);
            assert_eq!(more.knots().len(), spline.knots().len() + 1);
            for u in params(100) {
                let (a, b) = (spline.point(u), more.point(u));
                assert!(a.distance(b) < 1e-9, "{closed} {t} at {u}: {a} {b}");
            }
        }
        // Not at a knot it has, nor an open one's ends.
        let knot = spline.knots()[1];
        assert!(spline.with_knot(knot).is_none());
        assert!(spline.with_knot(knot + MIN_KNOT_GAP / 2.0).is_none());
        if !closed {
            assert!(spline.with_knot(0.0).is_none());
            assert!(spline.with_knot(1.0).is_none());
        }
    }
}

#[test]
fn a_piece_runs_as_the_spline_does_between_its_ends() {
    for closed in [false, true] {
        let spline = by_control(closed);
        let mut pieces = vec![(0.0, 0.4), (0.25, 0.8), (0.6, 1.0), (0.0, 1.0)];
        if closed {
            // Round its start.
            pieces.extend([(0.7, 1.3), (0.9, 1.05)]);
        }
        for (a, b) in pieces {
            let piece = spline.piece(a, b).unwrap();
            assert!(!piece.closed());
            for s in params(100) {
                let (on, off) = (piece.point(s), spline.point(a + (b - a) * s));
                assert!(on.distance(off) < 1e-9, "{closed} {a}..{b} at {s}");
            }
        }
        // A cut a hair from a knot is at the knot.
        let knot = spline.knots()[1];
        let piece = spline.piece(knot - MIN_KNOT_GAP / 4.0, 0.95).unwrap();
        assert!(piece.point(0.0).distance(spline.point(knot)) < 1e-6);
        assert!(spline.piece(0.5, 0.5).is_none());
        assert!(spline.piece(0.2, 1.5).is_none());
    }
}

/// The furthest the spline of `id` in `after` is from that in `before`,
/// over places along the one before.
fn strayed(before: &Sketch, after: &Sketch, id: Id) -> f64 {
    let (was, is) = (geom(before, id), geom(after, id));
    params(400)
        .map(|t| {
            let place = was.at(t);
            is.at(is.closest(place)).distance(place)
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_point_inserted_keeps_the_shape() {
    for closed in [false, true] {
        // Through fit points, a fit point on it where asked, the rest as
        // they were, the curve through them straying little.
        let (sketch, id) = sketch_with(&wave(), closed);
        let near = v(3.5, 3.4);
        let edit = crate::SketchEdit::InsertPoint { spline: id, near };
        let inserted = edit.apply(&sketch, &DESIGN).unwrap();
        let spline = inserted.spline(id).unwrap();
        assert_eq!(spline.points.len(), 7);
        let new = spline.points[2];
        assert_eq!(inserted.point(new).unwrap().number, 7);
        let at = inserted.point(new).unwrap().at;
        let was = geom(&sketch, id);
        assert!(was.at(was.closest(near)).distance(at) < 1e-9);
        let most = strayed(&sketch, &inserted, id);
        assert!(most < 0.01 * 12.0, "{closed}: {most}");
        // Where it has one, none.
        let at_point = crate::SketchEdit::InsertPoint {
            spline: id,
            near: wave()[3],
        };
        assert_eq!(at_point.apply(&sketch, &DESIGN), Err(EditError::Target(id)));

        // By control points, a knot there: a control point more, the
        // shape just as it was.
        let mut control = sketch.clone();
        control.convert_spline(id, SplineKind::Control).unwrap();
        let count = control.spline(id).unwrap().points.len();
        let inserted = edit.apply(&control, &DESIGN).unwrap();
        assert_eq!(inserted.spline(id).unwrap().points.len(), count + 1);
        assert!(strayed(&control, &inserted, id) < 1e-9, "{closed}");
        // Most control points stay, with what's on them.
        let kept = control
            .spline(id)
            .unwrap()
            .points
            .iter()
            .filter(|p| inserted.point(**p).is_some())
            .count();
        assert!(kept >= count - 2, "{kept} of {count}");
    }
}

#[test]
fn handles_added_keep_the_tangent() {
    let (sketch, id) = sketch_with(&wave(), false);
    let (first, third) = (sketch.points[0].id, sketch.points[2].id);
    let edit = crate::SketchEdit::AddHandles(vec![first, third, first]);
    let handled = edit.apply(&sketch, &DESIGN).unwrap();
    let spline = handled.spline(id).unwrap();
    assert_eq!(spline.handles.len(), 2);
    assert!(spline.has_handle(first) && spline.has_handle(third));
    // The tangent where each is, as it was.
    let params = chord_params(&wave(), false);
    let (was, is) = (
        sketch.spline_shape(sketch.spline(id).unwrap()).unwrap(),
        handled.spline_shape(spline).unwrap(),
    );
    for i in [0, 2] {
        let (a, b) = (was.eval(params[i])[1], is.eval(params[i])[1]);
        assert!(a.distance(b) < 1e-9 * a.length(), "{i}: {a} {b}");
    }
    assert!(strayed(&sketch, &handled, id) < 0.01 * 12.0);
    // Where one's drawn before it's there.
    let tip = handled.point(spline.handles[1].tip).unwrap().at;
    assert!(sketch.handle_tip(id, third).unwrap().distance(tip) < 1e-12);
    // Not twice, nor on a point that's no fit point.
    let again = crate::SketchEdit::AddHandles(vec![third]);
    assert_eq!(
        again.apply(&handled, &DESIGN),
        Err(EditError::Target(third))
    );
    let tip = spline.handles[0].tip;
    let on_tip = crate::SketchEdit::AddHandles(vec![tip]);
    assert_eq!(on_tip.apply(&handled, &DESIGN), Err(EditError::Target(tip)));
    // Deleting its tip takes one away.
    let deleted = crate::SketchEdit::Delete(vec![tip])
        .apply(&handled, &DESIGN)
        .unwrap();
    assert_eq!(deleted.spline(id).unwrap().handles.len(), 1);
}

#[test]
fn converting_round_and_back_as_an_edit() {
    let (sketch, id) = sketch_with(&wave(), false);
    let to = |kind| crate::SketchEdit::Convert {
        spline: id,
        to: kind,
    };
    let control = crate::testing::propose_it(&sketch, &to(SplineKind::Control))
        .unwrap()
        .sketch;
    assert_eq!(control.spline(id).unwrap().kind, SplineKind::Control);
    assert!(strayed(&sketch, &control, id) < 1e-9);
    let back = crate::testing::propose_it(&control, &to(SplineKind::Through))
        .unwrap()
        .sketch;
    assert_eq!(back.spline(id).unwrap().kind, SplineKind::Through);
    assert!(strayed(&sketch, &back, id) < 1e-8);
    // Of a line, nothing.
    let (mut lined, _) = sketch_with(&wave(), false);
    let (a, b) = (point(&mut lined, 0.0, 9.0), point(&mut lined, 5.0, 9.0));
    let other = line(&mut lined, a, b);
    let refused = crate::SketchEdit::Convert {
        spline: other,
        to: SplineKind::Control,
    };
    assert_eq!(
        refused.apply(&lined, &DESIGN),
        Err(EditError::Target(other))
    );
}

#[test]
fn the_curvature_comb_turns_as_the_spline_and_stays_bounded() {
    // Round a circle: the curvature a tenth, towards its centre.
    let mut sketch = Sketch::default();
    let fit = ring(v(20.0, 5.0), 10.0, 12);
    let points = fit.iter().map(|p| point(&mut sketch, p.x, p.y)).collect();
    let id = sketch
        .add_curve(Curve::Spline(Spline::through(points, true)), false)
        .unwrap();
    let comb = sketch.curvature_comb(id).unwrap();
    assert!(comb.len() > 12 && comb.len() <= MAX_COMB_TEETH);
    for [place, curving] in comb {
        let inward = (v(20.0, 5.0) - place).normalize();
        assert!((curving.length() - 0.1).abs() < 0.005, "{curving}");
        assert!(curving.normalize().dot(inward) > 0.999);
    }
    // The largest spline, a handle at every point: still bounded.
    let mut sketch = Sketch::default();
    let fit: Vec<Id> = (0..MAX_SPLINE_POINTS)
        .map(|i| point(&mut sketch, i as f64, (i as f64 * 0.7).sin()))
        .collect();
    let handles = fit
        .iter()
        .map(|&at| {
            let tip = sketch.point(at).unwrap().at + v(0.3, 0.1);
            Handle {
                at,
                tip: point(&mut sketch, tip.x, tip.y),
            }
        })
        .collect();
    let spline = Spline {
        handles,
        ..Spline::through(fit, false)
    };
    let id = sketch.add_curve(Curve::Spline(spline), false).unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    let comb = sketch.curvature_comb(id).unwrap();
    assert!(comb.len() <= MAX_COMB_TEETH, "{}", comb.len());
    assert!(comb.len() >= 2 * MAX_SPLINE_POINTS);
    // A line has none.
    let (a, b) = (point(&mut sketch, 0.0, 9.0), point(&mut sketch, 5.0, 9.0));
    let other = line(&mut sketch, a, b);
    assert!(sketch.curvature_comb(other).is_none());
}

#[test]
fn a_spline_being_drawn_is_flattened_as_it_would_be() {
    let (sketch, id) = sketch_with(&wave(), false);
    let drawn = flatten_spline(&wave(), SplineKind::Through, false).unwrap();
    assert_eq!(
        Some(drawn),
        sketch.flatten(&sketch.curve(id).unwrap().curve)
    );
    // Too few for its kind, none.
    assert!(flatten_spline(&wave()[..3], SplineKind::Control, false).is_none());
    assert!(flatten_spline(&wave()[..3], SplineKind::Control, true).is_some());
    assert!(flatten_spline(&wave()[..1], SplineKind::Through, false).is_none());
}
