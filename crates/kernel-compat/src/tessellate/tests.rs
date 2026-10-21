use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::*;

#[test]
fn the_chord_follows_the_size() {
    let display = Display::default();
    assert_eq!(display.chord(0.0), 1e-3);
    assert_eq!(display.chord(1.0), 1e-3);
    assert_eq!(display.chord(1e3), 1.0);
    let coarse = Display::new(&Tolerance::new(0.1).unwrap());
    assert_eq!(coarse.chord(10.0), 0.1);
}

/// The farthest any point of `curve` lies from the chord of its step, and
/// the least cosine of its tangent's turn along one, over `n` equal
/// steps, sampled finely.
fn deviation(curve: &Conic3, n: u32) -> (f64, f64) {
    let (mut far, mut turn) = (0.0f64, 1.0f64);
    for s in 0..n {
        let (t0, t1) = (f64::from(s) / f64::from(n), f64::from(s + 1) / f64::from(n));
        let (a, b) = (curve.eval(t0), curve.eval(t1));
        for k in 1..64 {
            let x = curve.eval(t0 + (t1 - t0) * f64::from(k) / 64.0);
            far = far.max(distance_to_line(x, a, b));
        }
        let da = curve.eval_deriv(t0).1.normalize();
        let db = curve.eval_deriv(t1).1.normalize();
        turn = turn.min(da.dot(db));
    }
    (far, turn)
}

#[test]
fn curves_get_enough_segments() {
    let straight = Conic3::line(DVec3::ZERO, DVec3::new(100.0, 3.0, -2.0)).unwrap();
    assert_eq!(segments(&straight, 1e-3), 1);

    for radius in [1e-3, 1.0, 10.0, 1e4] {
        let quarter = Conic3::new(
            DVec3::X * radius,
            DVec3::new(1.0, 1.0, 0.0) * radius,
            FRAC_1_SQRT_2,
            DVec3::Y * radius,
        )
        .unwrap();
        for chord in [1e-3, 1e-2, 1.0] {
            let n = segments(&quarter, chord);
            let (far, turn) = deviation(&quarter, n);
            // The count follows the middles of the steps; the curve's
            // farthest point is close by.
            assert!(
                n == 64 || far <= chord * 1.05,
                "{radius} {chord}: {n} {far}"
            );
            assert!(
                n == 64 || turn >= COS_TURN - 1e-12,
                "{radius} {chord}: {n} {turn}"
            );
            // Not many more than needed: one fewer would do neither.
            if n > 1 && n < 64 {
                let (far, turn) = deviation(&quarter, n - 1);
                // cos 9°.
                assert!(
                    far > chord * 0.8 || turn < 0.987_688_340_595_137_7,
                    "{radius} {chord}: {n}"
                );
            }
            // A quarter circle turns 90°.
            assert!(n >= 9, "{n}");
        }
    }

    // Nearly the control polygon: the most segments, no more.
    let sharp = Conic3::new(DVec3::X, DVec3::ZERO, 64.0, DVec3::Y).unwrap();
    assert_eq!(segments(&sharp, 1e-6), Display::MAX_SEGMENTS);
}

/// A loose curve is flattened from end to end, as an edge would be cut,
/// within the chord of the curve.
#[test]
fn a_loose_curve_is_flattened_as_an_edge_is() {
    let arc = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 10.0, 0.0, PI / 2.0).unwrap();
    let display = Display::default();
    let points = display.flatten(&arc, 20.0);
    assert_eq!(points.len() as u32, segments(&arc, display.chord(20.0)) + 1);
    assert!(points.len() > 2);
    assert_eq!(points[0], arc.p0);
    assert_eq!(*points.last().unwrap(), arc.p1);
    for p in &points {
        assert!((p.length() - 10.0).abs() < 1e-9, "{p}");
    }
    let line = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    assert_eq!(display.flatten(&line, 1.0), [DVec3::ZERO, DVec3::X]);
}
