use super::*;
use crate::flatten_arc;

fn close(a: DVec2, b: DVec2) -> bool {
    a.abs_diff_eq(b, 1e-9)
}

#[test]
fn the_arc_passes_its_three_points() {
    let (a, b) = (DVec2::new(1.0, 0.0), DVec2::new(-1.0, 0.0));
    // Over the top, and under the bottom: the ends swap so both run
    // counter-clockwise.
    for (through, start, end) in [(DVec2::Y, a, b), (DVec2::NEG_Y, b, a)] {
        let arc = arc_through(a, b, through).unwrap();
        assert!(close(arc.center, DVec2::ZERO), "{arc:?}");
        assert_eq!((arc.start, arc.end), (start, end));
        let polyline = flatten_arc(arc.center, arc.start, arc.end);
        let middle = polyline[polyline.len() / 2];
        assert!(close(middle, through), "{middle}");
    }
}

#[test]
fn an_off_centre_arc_has_its_circle_s_centre() {
    let center = DVec2::new(3.0, -2.0);
    let on = |angle: f64| center + DVec2::from_angle(angle) * 5.0;
    let arc = arc_through(on(0.3), on(2.0), on(1.1)).unwrap();
    assert!(close(arc.center, center), "{arc:?}");
    assert_eq!((arc.start, arc.end), (on(0.3), on(2.0)));
    // Most of a turn the other way round.
    let arc = arc_through(on(0.3), on(2.0), on(4.0)).unwrap();
    assert!(close(arc.center, center), "{arc:?}");
    assert_eq!((arc.start, arc.end), (on(2.0), on(0.3)));
}

#[test]
fn points_on_a_line_make_no_arc() {
    let (a, b) = (DVec2::ZERO, DVec2::new(4.0, 2.0));
    assert_eq!(arc_through(a, b, DVec2::new(2.0, 1.0)), None);
    assert_eq!(arc_through(a, b, DVec2::new(8.0, 4.0)), None);
    assert_eq!(arc_through(a, a, b), None);
    assert_eq!(arc_through(a, b, b), None);
    assert_eq!(arc_through(a, b, DVec2::NAN), None);
    // Nearly on a line puts the centre past any bound, but finite.
    let max = 1e6;
    let far = arc_through(
        DVec2::splat(-max),
        DVec2::splat(max),
        DVec2::new(0.0, 1e-300),
    );
    assert!(far.is_none_or(|arc| arc.center.is_finite()));
}

#[test]
fn an_arc_sweeps_counter_clockwise_a_whole_turn_at_most() {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let sweep = |from: f64, to: f64| arc_sweep(DVec2::from_angle(from), DVec2::from_angle(to));
    assert!((sweep(0.0, FRAC_PI_2) - FRAC_PI_2).abs() < 1e-12);
    assert!((sweep(FRAC_PI_2, 0.0) - 1.5 * PI).abs() < 1e-12);
    assert!((sweep(-3.0, 3.0) - 6.0).abs() < 1e-12);
    // The same direction is a whole turn, never none.
    assert_eq!(sweep(1.0, 1.0), TAU);
}

#[test]
fn a_foot_is_on_the_endless_line() {
    let (start, end) = (DVec2::new(1.0, 1.0), DVec2::new(3.0, 1.0));
    assert!(close(
        foot(DVec2::new(2.0, 5.0), start, end),
        DVec2::new(2.0, 1.0)
    ));
    // Past the ends too.
    assert!(close(
        foot(DVec2::new(-4.0, 0.0), start, end),
        DVec2::new(-4.0, 1.0)
    ));
    assert!(foot(DVec2::ZERO, start, start).is_nan());
}

#[test]
fn lines_cross_unless_parallel() {
    let (p, r) = (DVec2::new(0.0, 1.0), DVec2::new(2.0, 0.0));
    let (q, s) = (DVec2::new(3.0, 0.0), DVec2::new(0.0, 4.0));
    let (t, u) = crossing(p, r, q, s).unwrap();
    assert!(close(p + r * t, DVec2::new(3.0, 1.0)));
    assert!(close(q + s * u, DVec2::new(3.0, 1.0)));
    assert_eq!(crossing(p, r, q, r * -3.0), None);
    assert_eq!(crossing(p, r, q, r + DVec2::new(0.0, 1e-12)), None);
    assert_eq!(crossing(p, DVec2::ZERO, q, s), None);
}
