use std::f64::consts::{FRAC_PI_2, PI};

use super::*;

const TOLERANCE: f64 = 1e-7;

fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Geom {
    Geom::Segment {
        start: DVec2::new(x0, y0),
        end: DVec2::new(x1, y1),
    }
}

fn circle(x: f64, y: f64, radius: f64) -> Geom {
    Geom::Round {
        center: DVec2::new(x, y),
        radius,
        begin: 0.0,
        sweep: TAU,
    }
}

fn arc(x: f64, y: f64, radius: f64, begin: f64, sweep: f64) -> Geom {
    Geom::Round {
        center: DVec2::new(x, y),
        radius,
        begin,
        sweep,
    }
}

/// Where `a` and `b` meet, as places on `a` checked to be the same on
/// `b`, sorted and with repeats dropped.
fn places(a: &Geom, b: &Geom) -> Vec<DVec2> {
    let mut found = Vec::new();
    meet(a, b, TOLERANCE, &mut found);
    let mut places: Vec<DVec2> = Vec::new();
    for (ua, ub) in found {
        let (pa, pb) = (a.at(ua), b.at(ub));
        assert!(pa.distance(pb) < 1e-6, "{pa} {pb}");
        if !places.iter().any(|place| place.distance(pa) < 1e-6) {
            places.push(pa);
        }
    }
    places.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)));
    places
}

fn close(a: &[DVec2], b: &[(f64, f64)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(p, &(x, y))| p.distance(DVec2::new(x, y)) < 1e-6)
}

#[test]
fn lines_cross_within_both() {
    let a = segment(0.0, 0.0, 4.0, 4.0);
    assert!(close(
        &places(&a, &segment(0.0, 4.0, 4.0, 0.0)),
        &[(2.0, 2.0)]
    ));
    // The endless lines cross past one's end: not a meeting.
    assert!(places(&a, &segment(5.0, 0.0, 5.0, 3.0)).is_empty());
    // Parallel apart.
    assert!(places(&a, &segment(1.0, 0.0, 5.0, 4.0)).is_empty());
}

#[test]
fn a_line_ending_on_another_meets_it_there() {
    let a = segment(0.0, 0.0, 10.0, 0.0);
    // Ending in its middle, a T.
    assert!(close(
        &places(&a, &segment(5.0, 3.0, 5.0, 0.0)),
        &[(5.0, 0.0)]
    ));
    // Short of it by less than the tolerance.
    let b = segment(5.0, 3.0, 5.0, TOLERANCE / 2.0);
    assert!(close(&places(&a, &b), &[(5.0, 0.0)]));
    // Short by more: apart.
    assert!(places(&a, &segment(5.0, 3.0, 5.0, 1e-4)).is_empty());
    // Past its end by less than the tolerance.
    let b = segment(10.0 + TOLERANCE / 2.0, -1.0, 10.0 + TOLERANCE / 2.0, 1.0);
    assert!(close(&places(&a, &b), &[(10.0, 0.0)]));
}

#[test]
fn lines_on_one_line_meet_where_they_overlap() {
    let a = segment(0.0, 0.0, 10.0, 0.0);
    let overlaps = places(&a, &segment(4.0, 0.0, 14.0, 0.0));
    assert!(close(&overlaps, &[(4.0, 0.0), (10.0, 0.0)]), "{overlaps:?}");
    let inside = places(&a, &segment(8.0, 0.0, 2.0, 0.0));
    assert!(close(&inside, &[(2.0, 0.0), (8.0, 0.0)]), "{inside:?}");
    // End to end.
    let touching = places(&a, &segment(10.0, 0.0, 20.0, 0.0));
    assert!(close(&touching, &[(10.0, 0.0)]), "{touching:?}");
    assert!(places(&a, &segment(11.0, 0.0, 20.0, 0.0)).is_empty());
}

#[test]
fn a_line_crosses_a_circle_twice_or_touches_it() {
    let round = circle(0.0, 0.0, 5.0);
    let across = places(&segment(-10.0, 3.0, 10.0, 3.0), &round);
    assert!(close(&across, &[(-4.0, 3.0), (4.0, 3.0)]), "{across:?}");
    // Into it and stopping.
    let into = places(&segment(-10.0, 3.0, 0.0, 3.0), &round);
    assert!(close(&into, &[(-4.0, 3.0)]), "{into:?}");
    // Touching the top, and touching within the tolerance: once.
    for y in [5.0, 5.0 + TOLERANCE / 2.0] {
        let touch = places(&segment(-10.0, y, 10.0, y), &round);
        assert!(close(&touch, &[(0.0, y)]), "{y}: {touch:?}");
    }
    // Into it by less than the tolerance, it still crosses twice, where
    // it does, further apart than the tolerance.
    let y = 5.0 - TOLERANCE / 2.0;
    let x = (25.0 - y * y).sqrt();
    assert!(2.0 * x > 1e3 * TOLERANCE);
    let barely = places(&segment(-10.0, y, 10.0, y), &round);
    assert!(close(&barely, &[(-x, y), (x, y)]), "{barely:?}");
    assert!(places(&segment(-10.0, 5.1, 10.0, 5.1), &round).is_empty());
}

#[test]
fn an_arc_meets_only_along_its_sweep() {
    // The upper half of a circle.
    let upper = arc(0.0, 0.0, 5.0, 0.0, PI);
    let across = places(&segment(-10.0, 3.0, 10.0, 3.0), &upper);
    assert!(close(&across, &[(-4.0, 3.0), (4.0, 3.0)]));
    assert!(places(&segment(-10.0, -3.0, 10.0, -3.0), &upper).is_empty());
    // From the lower left, across its start: the arc begins at (5, 0).
    let quarter = arc(0.0, 0.0, 5.0, -FRAC_PI_2, FRAC_PI_2);
    let at_start = places(&segment(-10.0, -5.0, 10.0, -5.0), &quarter);
    assert!(close(&at_start, &[(0.0, -5.0)]), "{at_start:?}");
}

#[test]
fn circles_cross_touch_or_overlap() {
    let a = circle(0.0, 0.0, 5.0);
    let crossing = places(&a, &circle(8.0, 0.0, 5.0));
    assert!(close(&crossing, &[(4.0, -3.0), (4.0, 3.0)]), "{crossing:?}");
    // Outside each other, touching, also a hair apart.
    for x in [10.0, 10.0 + TOLERANCE / 2.0] {
        let touch = places(&a, &circle(x, 0.0, 5.0));
        assert!(close(&touch, &[(5.0, 0.0)]), "{x}: {touch:?}");
    }
    // Into each other by less than the tolerance, they cross twice, and
    // `a` starts between.
    let x = 10.0 - TOLERANCE / 2.0;
    let y = (25.0 - x * x / 4.0).sqrt();
    let barely = places(&a, &circle(x, 0.0, 5.0));
    assert!(
        close(&barely, &[(x / 2.0, -y), (x / 2.0, y), (5.0, 0.0)]),
        "{barely:?}"
    );
    assert!(places(&a, &circle(10.1, 0.0, 5.0)).is_empty());
    // One inside touching.
    let inside = places(&a, &circle(3.0, 0.0, 2.0));
    assert!(close(&inside, &[(5.0, 0.0)]), "{inside:?}");
    let inside = places(&circle(3.0, 0.0, 2.0), &a);
    assert!(close(&inside, &[(5.0, 0.0)]), "{inside:?}");
    assert!(places(&a, &circle(2.0, 0.0, 2.0)).is_empty());
    // Concentric: whole circles only meet where one starts, so one is cut
    // where the other is; arcs on one circle meet where they start and
    // stop overlapping.
    assert!(close(&places(&a, &circle(0.0, 0.0, 5.0)), &[(5.0, 0.0)]));
    assert!(places(&a, &circle(0.0, 0.0, 3.0)).is_empty());
    let (upper, right) = (
        arc(0.0, 0.0, 5.0, 0.0, PI),
        arc(0.0, 0.0, 5.0, -FRAC_PI_2, PI),
    );
    let overlap = places(&upper, &right);
    assert!(close(&overlap, &[(0.0, 5.0), (5.0, 0.0)]), "{overlap:?}");
    let on_circle = places(&upper, &a);
    assert!(
        close(&on_circle, &[(-5.0, 0.0), (5.0, 0.0)]),
        "{on_circle:?}"
    );
}

#[test]
fn parameters_near_an_end_are_put_on_it() {
    let upper = arc(0.0, 0.0, 5.0, 0.0, PI);
    assert_eq!(upper.param(DVec2::new(5.0, -1e-9), TOLERANCE), Some(0.0));
    assert_eq!(upper.param(DVec2::new(-5.0, -1e-9), TOLERANCE), Some(PI));
    assert_eq!(upper.param(DVec2::new(0.0, -5.0), TOLERANCE), None);
    let line = segment(0.0, 0.0, 10.0, 0.0);
    assert_eq!(line.param(DVec2::new(-1e-8, 0.0), TOLERANCE), Some(0.0));
    assert_eq!(
        line.param(DVec2::new(10.0 + 1e-8, 0.0), TOLERANCE),
        Some(1.0)
    );
    assert_eq!(line.param(DVec2::new(11.0, 0.0), TOLERANCE), None);
}

#[test]
fn winding_and_bulge_add_up_round_loops() {
    let round = circle(1.0, 2.0, 3.0);
    for (point, turns) in [
        (DVec2::new(1.0, 2.0), 1.0),
        (DVec2::new(3.9, 2.0), 1.0),
        (DVec2::new(4.1, 2.0), 0.0),
        (DVec2::new(-5.0, 8.0), 0.0),
    ] {
        // In pieces, either way round.
        let forward = round.winding(0.0, 1.0, point)
            + round.winding(1.0, 4.0, point)
            + round.winding(4.0, TAU, point);
        assert!((forward - turns * TAU).abs() < 1e-9, "{point}: {forward}");
        let back = round.winding(TAU, 0.0, point);
        assert!((back + turns * TAU).abs() < 1e-9, "{point}: {back}");
    }
    // A half disc: the arc's bulge over its chord is the whole of it.
    let bulge = round.bulge(0.0, PI);
    assert!((bulge - PI * 9.0 / 2.0).abs() < 1e-9);
    assert!((round.bulge(PI, 0.0) + bulge).abs() < 1e-9);
    assert_eq!(segment(0.0, 0.0, 1.0, 1.0).bulge(0.0, 1.0), 0.0);
}

#[test]
fn heading_turns_left_counter_clockwise() {
    let round = circle(0.0, 0.0, 2.0);
    let (direction, curvature) = round.heading(0.0, true);
    assert!(direction.abs_diff_eq(DVec2::Y, 1e-12));
    assert_eq!(curvature, 0.5);
    let (direction, curvature) = round.heading(FRAC_PI_2, false);
    assert!(direction.abs_diff_eq(DVec2::X, 1e-12));
    assert_eq!(curvature, -0.5);
    let (direction, curvature) = segment(0.0, 0.0, 0.0, 3.0).heading(0.5, false);
    assert!(direction.abs_diff_eq(DVec2::NEG_Y, 1e-12));
    assert_eq!(curvature, 0.0);
}

#[test]
fn a_span_s_box_reaches_as_far_as_the_arc() {
    let near = |(min, max): (DVec2, DVec2), expected: [f64; 4]| {
        let found = [min.x, min.y, max.x, max.y];
        for (found, expected) in found.iter().zip(expected) {
            assert!((found - expected).abs() < 1e-12, "{found:?} {expected:?}");
        }
    };
    // From the bottom left round past the right to the top left, either
    // way along it: as far as x = 2.
    let round = arc(0.0, 0.0, 2.0, -3.0 * PI / 4.0, 3.0 * FRAC_PI_2);
    let corner = 2f64.sqrt();
    let expected = [-corner, -2.0, 2.0, 2.0];
    near(round.span_bounds(0.0, 3.0 * FRAC_PI_2), expected);
    near(round.span_bounds(3.0 * FRAC_PI_2, 0.0), expected);
    // Past the start of a circle's angles, and a line's ends.
    let whole = circle(1.0, 0.0, 1.0);
    near(whole.span_bounds(PI, TAU), [0.0, -1.0, 2.0, 0.0]);
    near(
        segment(3.0, 1.0, 0.0, 2.0).span_bounds(0.0, 1.0),
        [0.0, 1.0, 3.0, 2.0],
    );
}
