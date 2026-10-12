use glam::DVec3;

use super::*;

/// Quarter turns about a coordinate axis are exact whatever the axis's
/// length, and so are steps along one: the axis is made unit without
/// rounding, as a direction along an axis must be.
#[test]
fn quarter_turns_and_steps_along_an_axis_of_any_length_are_exact() {
    for length in [1.0, 3.0, 49.0, 0.1, 1e-200, 1e200, f64::MAX] {
        let axis = DVec3::new(0.0, 0.0, length);
        let turn = Motion::turn(DVec3::ZERO, axis, 90.0).expect("an axis");
        assert_eq!(turn.vector(DVec3::X), DVec3::Y, "{length}");
        assert_eq!(turn.vector(DVec3::Z), DVec3::Z, "{length}");
        let step = Motion::pattern_step(axis, 2.0, 3).expect("a direction");
        assert_eq!(
            step.point(DVec3::ZERO),
            DVec3::new(0.0, 0.0, 6.0),
            "{length}"
        );
    }
    for axis in [DVec3::ZERO, DVec3::NAN, DVec3::new(0.0, f64::INFINITY, 1.0)] {
        assert_eq!(Motion::turn(DVec3::ZERO, axis, 90.0), None);
        assert_eq!(Motion::pattern_step(axis, 1.0, 1), None);
    }
}

/// The corners of a 2 × 3 × 4 box, then points at coordinates rounding
/// would move if a motion weren't exact.
fn points() -> Vec<DVec3> {
    let mut points: Vec<DVec3> = (0..8)
        .map(|c| {
            DVec3::new(
                if c & 1 == 0 { 0.0 } else { 2.0 },
                if c & 2 == 0 { 0.0 } else { 3.0 },
                if c & 4 == 0 { 0.0 } else { 4.0 },
            )
        })
        .collect();
    points.extend([
        DVec3::new(0.1, 0.7, 1.3),
        DVec3::new(1.0 / 3.0, 2.0 / 3.0, 3.9),
        DVec3::new(-1e-7, 2.5e3, 17.25),
    ]);
    points
}

#[test]
fn quarter_turns_are_exact() {
    let points = points();
    for axis in [DVec3::X, DVec3::Y, DVec3::Z, DVec3::NEG_Z * 3.0] {
        let quarter = Motion::turn(DVec3::ZERO, axis, 90.0).unwrap();
        // Four quarter turns are the points again, to the bit.
        for &p in &points {
            let turned = (0..4).fold(p, |q, _| quarter.point(q));
            assert_eq!(turned, p, "about {axis}");
        }
        // A quarter turn about z maps (x, y, z) to (−y, x, z) exactly.
        if axis == DVec3::Z {
            for &a in &points {
                assert_eq!(quarter.point(a), DVec3::new(-a.y, a.x, a.z));
            }
        }
    }
    // Multiples of 90° of either sign and past a turn.
    for (degrees, sin, cos) in [
        (0.0, 0.0, 1.0),
        (90.0, 1.0, 0.0),
        (180.0, 0.0, -1.0),
        (-90.0, -1.0, 0.0),
        (270.0, -1.0, 0.0),
        (-270.0, 1.0, 0.0),
        (450.0, 1.0, 0.0),
        (-180.0, 0.0, -1.0),
        (720.0, 0.0, 1.0),
    ] {
        assert_eq!(sin_cos_degrees(degrees), (sin, cos), "{degrees}");
    }
}

#[test]
fn mirrors_in_planes_square_to_an_axis_are_exact_for_any_normal_length() {
    // Normals of any length along an axis: the coordinate along it
    // negated, about the plane's (exact for 0 and halves of powers of
    // two, so `x ↦ 2·p − x` takes one rounding at most, none here).
    let points = points();
    for (scale, along) in [(3.0, 2), (0.1, 0), (-7.0, 1), (1e-300, 2), (1e300, 0)] {
        let mut normal = DVec3::ZERO;
        normal[along] = scale;
        let mirror = Motion::mirror(DVec3::ZERO, normal).unwrap();
        assert!(mirror.mirrors());
        for &a in &points {
            let mut want = a;
            want[along] = -want[along];
            assert_eq!(mirror.point(a), want, "{scale} along {along}");
        }
        let through = Motion::mirror(DVec3::splat(2.0), normal).unwrap();
        for &p in &points[..8] {
            let mut want = p;
            want[along] = 4.0 - want[along];
            assert_eq!(through.point(p), want);
        }
    }
    // A slanted normal of any length is the same mirror, to rounding.
    let unit = Motion::mirror(DVec3::X, DVec3::new(1.0, 2.0, -0.5)).unwrap();
    for scale in [1e-300, 3.0, 1e300] {
        let mirror = Motion::mirror(DVec3::X, DVec3::new(1.0, 2.0, -0.5) * scale).unwrap();
        for &p in &points[..8] {
            assert!((mirror.point(p) - unit.point(p)).length() < 1e-14 * 8.0);
        }
    }
}

/// Equal motions have equal bits, however they're built; any change of
/// a number or of whether it mirrors changes them.
#[test]
fn bits_tell_motions_apart() {
    let shift = |x: f64| Motion::translation(DVec3::new(x, 0.0, 0.0)).unwrap();
    assert_eq!(shift(1.0).bits(), shift(1.0).bits());
    assert_eq!(Motion::IDENTITY.then(&shift(2.0)).bits(), shift(2.0).bits());
    assert_ne!(shift(1.0).bits(), shift(1.0 + f64::EPSILON).bits());
    let turn = Motion::turn(DVec3::ZERO, DVec3::Z, 90.0).unwrap();
    let mirror = Motion::mirror(DVec3::ZERO, DVec3::X).unwrap();
    assert_ne!(turn.bits(), Motion::IDENTITY.bits());
    assert_ne!(turn.bits(), mirror.bits());
    assert_eq!(mirror.bits()[22], 1);
    assert_eq!(turn.bits()[22], 0);
    assert_eq!(turn.then(&mirror).bits()[22], 1);
}
