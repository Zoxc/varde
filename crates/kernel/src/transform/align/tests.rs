use glam::{DMat3, DVec3};

use super::*;
use crate::par::assert_deterministic;
use crate::test_rng::Rng;
use crate::{Budget, Solid, Tolerance};

const TOL: Tolerance = Tolerance::DEFAULT;

fn datum(point: DVec3, primary: DVec3, secondary: Option<DVec3>) -> Datum {
    Datum {
        point,
        primary: Some(primary),
        secondary,
    }
}

fn align(moved: &Datum, target: &Datum, options: &AlignOptions) -> Motion {
    Motion::align(moved, target, options).unwrap()
}

/// The linear part as a matrix, read back through `vector`.
fn matrix(m: &Motion) -> DMat3 {
    DMat3::from_cols(m.vector(DVec3::X), m.vector(DVec3::Y), m.vector(DVec3::Z))
}

/// Whether every entry of the linear part is 0 or ±1 (a signed
/// permutation), exactly.
fn signed_permutation(m: &Motion) -> bool {
    matrix(m)
        .to_cols_array()
        .iter()
        .all(|&x| x == 0.0 || x.abs() == 1.0)
}

/// A rotation to rounding: orthonormal columns, determinant 1.
fn rotation(m: &Motion) -> bool {
    let a = matrix(m);
    let gap = (a.transpose() * a - DMat3::IDENTITY)
        .to_cols_array()
        .iter()
        .fold(0.0f64, |g, x| g.max(x.abs()));
    gap <= 1e-14 && (a.determinant() - 1.0).abs() <= 1e-14 && !m.mirrors()
}

fn near(a: DVec3, b: DVec3, within: f64) -> bool {
    (a - b).abs().max_element() <= within
}

fn random_unit(rng: &mut Rng) -> DVec3 {
    loop {
        let v = DVec3::new(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        );
        let l = v.length();
        if (0.1..=1.0).contains(&l) {
            return v / l;
        }
    }
}

#[test]
fn axis_aligned_primaries_land_to_the_bit() {
    let axes = [
        DVec3::X,
        DVec3::Y,
        DVec3::Z,
        DVec3::NEG_X,
        DVec3::NEG_Y,
        DVec3::NEG_Z,
    ];
    let (pm, pt) = (DVec3::new(1.5, -2.25, 3.0), DVec3::new(10.0, -4.0, 7.125));
    for &dm in &axes {
        for &dt in &axes {
            for flip in [false, true] {
                // Any length: a direction along an axis is that axis.
                let moved = datum(pm, dm * 3.7, None);
                let target = datum(pt, dt * 0.01, None);
                let options = AlignOptions {
                    flip,
                    ..AlignOptions::default()
                };
                let m = align(&moved, &target, &options);
                let want = if flip { -dt } else { dt };
                assert_eq!(m.point(pm), pt, "{dm} onto {dt}");
                assert_eq!(m.vector(dm), want, "{dm} onto {dt}");
                assert!(signed_permutation(&m) && rotation(&m), "{dm} onto {dt}");
                // The smallest rotation: the axis square to both stays.
                let axis = dm.cross(want);
                if axis != DVec3::ZERO {
                    assert_eq!(m.vector(axis), axis);
                }
            }
        }
    }
}

#[test]
fn quarter_turned_frames_with_secondaries_are_exact() {
    let axes = [DVec3::X, DVec3::Y, DVec3::Z];
    let signs = [1.0, -1.0];
    let (pm, pt) = (DVec3::new(-3.0, 0.5, 8.0), DVec3::new(0.25, 6.0, -1.0));
    for (i, &a) in axes.iter().enumerate() {
        for &b in &axes[..i] {
            for &sa in &signs {
                for &sb in &signs {
                    let moved = datum(pm, a * sa, Some(b * sb + a * 0.5));
                    let target = datum(pt, b, Some(-a * sb));
                    let m = align(&moved, &target, &AlignOptions::default());
                    assert_eq!(m.point(pm), pt);
                    assert_eq!(m.vector(a * sa), b);
                    assert_eq!(m.vector(b * sb), -a * sb);
                    assert!(signed_permutation(&m) && rotation(&m));
                }
            }
        }
    }
}

#[test]
fn a_box_aligned_corner_to_corner_lands_to_the_bit() {
    // A 2 × 3 × 4 box's top-front-left corner onto another's bottom
    // corner, its top face against the other's bottom: every vertex
    // where it should be, to the bit.
    let solid = Solid::cuboid(DVec3::ZERO, DVec3::new(2.0, 3.0, 4.0), 1, &TOL).unwrap();
    let moved = datum(DVec3::new(0.0, 0.0, 4.0), DVec3::Z, Some(DVec3::X));
    let target = datum(DVec3::new(10.0, 20.0, 30.0), DVec3::NEG_Z, Some(DVec3::Y));
    let options = AlignOptions {
        flip: true,
        ..AlignOptions::default()
    };
    let m = align(&moved, &target, &options);
    let moved_solid = solid.transformed(&m, None, &TOL, &Budget::DEFAULT).unwrap();
    // Z stays Z (flipped onto −(−Z)), X goes to Y, so Y goes to −X.
    for (a, b) in solid.mesh().verts().iter().zip(moved_solid.mesh().verts()) {
        let want = DVec3::new(10.0 - a.y, 20.0 + a.x, 30.0 + a.z - 4.0);
        assert_eq!(*b, want);
    }
    assert!((moved_solid.volume() - solid.volume()).abs() <= 1e-12 * solid.volume());
}

#[test]
fn tilted_frames_land_to_rounding() {
    let mut rng = Rng::new(33);
    for _ in 0..500 {
        let pm = DVec3::new(
            rng.range(-1e3, 1e3),
            rng.range(-1e3, 1e3),
            rng.range(-1e3, 1e3),
        );
        let pt = DVec3::new(
            rng.range(-1e3, 1e3),
            rng.range(-1e3, 1e3),
            rng.range(-1e3, 1e3),
        );
        let (dm, dt) = (random_unit(&mut rng), random_unit(&mut rng));
        let (sm, st) = (random_unit(&mut rng), random_unit(&mut rng));
        let flip = rng.range(0.0, 1.0) < 0.5;
        let want = if flip { -dt } else { dt };
        let options = AlignOptions {
            flip,
            ..AlignOptions::default()
        };
        // Primaries only: the smallest rotation.
        let m = align(&datum(pm, dm, None), &datum(pt, dt * 2.0, None), &options);
        assert!(near(m.point(pm), pt, 1e-12));
        assert!(near(m.vector(dm), want, 1e-15));
        assert!(rotation(&m));
        // Its axis stays, to the rounding of the cross product.
        let cross = dm.cross(want);
        let axis = cross.normalize();
        assert!(near(m.vector(axis), axis, 1e-15 + 1e-15 / cross.length()));
        // With secondaries: their parts square to the primaries agree.
        let m = align(&datum(pm, dm, Some(sm)), &datum(pt, dt, Some(st)), &options);
        assert!(near(m.point(pm), pt, 1e-12));
        assert!(near(m.vector(dm), want, 1e-15));
        let square = |s: DVec3, d: DVec3| (s - d * s.dot(d)).normalize();
        assert!(near(m.vector(square(sm, dm)), square(st, dt), 1e-14));
        assert!(rotation(&m));
    }
}

#[test]
fn frames_that_agree_give_the_identity() {
    let mut rng = Rng::new(7);
    for _ in 0..100 {
        let (d, s) = (random_unit(&mut rng), random_unit(&mut rng));
        let (pm, pt) = (DVec3::new(1.0, 2.0, 3.0), DVec3::new(-4.0, 0.5, 9.0));
        for secondary in [None, Some(s)] {
            let m = align(
                &datum(pm, d, secondary),
                // Other lengths, scaled exactly: the same units.
                &datum(pt, d * 4.0, secondary.map(|s| s * 0.5)),
                &AlignOptions::default(),
            );
            assert_eq!(matrix(&m), DMat3::IDENTITY);
            assert_eq!(m.point(pm), pt);
            assert_eq!(m, Motion::translation(pt - pm).unwrap());
        }
    }
    // A point alone is a move.
    let m = align(
        &Datum::point(DVec3::new(1.0, 1.0, 1.0)),
        &Datum::point(DVec3::new(3.0, -1.0, 0.0)),
        &AlignOptions::default(),
    );
    assert_eq!(m, Motion::translation(DVec3::new(2.0, -2.0, -1.0)).unwrap());
}

#[test]
fn a_flip_turns_the_primary_round() {
    // Exactly opposite: the half turn about the direction a sketch
    // placement would take as x (world X for a vertical primary).
    let m = align(
        &datum(DVec3::ZERO, DVec3::Z, None),
        &datum(DVec3::ZERO, DVec3::Z, None),
        &AlignOptions {
            flip: true,
            ..AlignOptions::default()
        },
    );
    assert_eq!(m.vector(DVec3::Z), DVec3::NEG_Z);
    assert_eq!(m.vector(DVec3::X), DVec3::X);
    assert_eq!(m.vector(DVec3::Y), DVec3::NEG_Y);
    // About Z × d otherwise.
    let m = align(
        &datum(DVec3::ZERO, DVec3::X, None),
        &datum(DVec3::ZERO, DVec3::NEG_X, None),
        &AlignOptions::default(),
    );
    assert_eq!(m.vector(DVec3::X), DVec3::NEG_X);
    assert_eq!(m.vector(DVec3::NEG_Y), DVec3::NEG_Y);
    assert_eq!(m.vector(DVec3::Z), DVec3::NEG_Z);
    // Tilted and nearly opposite or parallel: still onto the target, a
    // rotation, and next to no turn about the primary when parallel.
    let mut rng = Rng::new(11);
    for _ in 0..200 {
        let d = random_unit(&mut rng);
        let wobble = random_unit(&mut rng) * rng.range(0.0, 1e-9);
        let dt = (d + wobble).normalize();
        for flip in [false, true] {
            let m = align(
                &datum(DVec3::ZERO, d, None),
                &datum(DVec3::ZERO, dt, None),
                &AlignOptions {
                    flip,
                    ..AlignOptions::default()
                },
            );
            let want = if flip { -dt } else { dt };
            assert!(near(m.vector(d), want, 1e-15));
            assert!(rotation(&m));
            if !flip {
                let gap = (matrix(&m) - DMat3::IDENTITY)
                    .to_cols_array()
                    .iter()
                    .fold(0.0f64, |g, x| g.max(x.abs()));
                assert!(gap <= 1e-8, "{gap}");
            }
        }
    }
}

#[test]
fn an_offset_and_a_turn_follow_the_target_s_primary() {
    let (pm, pt) = (DVec3::new(1.0, 0.0, 0.0), DVec3::new(0.0, 0.0, 10.0));
    let moved = datum(pm, DVec3::X, Some(DVec3::Y));
    let target = datum(pt, DVec3::Z, Some(DVec3::X));
    let plain = align(&moved, &target, &AlignOptions::default());
    let m = align(
        &moved,
        &target,
        &AlignOptions {
            flip: false,
            offset: 2.5,
            degrees: 90.0,
        },
    );
    // The point 2.5 along Z, the primary still Z, the secondary turned
    // a quarter about it: X to Y. Exact.
    assert_eq!(m.point(pm), DVec3::new(0.0, 0.0, 12.5));
    assert_eq!(m.vector(DVec3::X), DVec3::Z);
    assert_eq!(m.vector(DVec3::Y), DVec3::Y);
    assert_eq!(plain.vector(DVec3::Y), DVec3::X);
    // A point off the axis turns about it.
    let off = pm + DVec3::Y;
    assert_eq!(plain.point(off), pt + DVec3::X);
    assert_eq!(m.point(off), pt + DVec3::new(0.0, 1.0, 2.5));
    // Other angles to rounding; flipping turns the other way round.
    for flip in [false, true] {
        let m = align(
            &moved,
            &target,
            &AlignOptions {
                flip,
                offset: -1.0,
                degrees: 30.0,
            },
        );
        // About and along the target's primary as given, flipped or not.
        let axis = DVec3::Z;
        let turn = Motion::turn(pt, axis, 30.0).unwrap();
        let base = align(
            &moved,
            &target,
            &AlignOptions {
                flip,
                ..AlignOptions::default()
            },
        );
        let want = turn.point(base.point(off)) - axis;
        assert!(near(m.point(off), want, 1e-14), "{} {want}", m.point(off));
        assert!(near(m.point(pm), pt - axis, 1e-15));
    }
}

#[test]
fn degenerate_inputs_are_refused() {
    let p = DVec3::new(1.0, 2.0, 3.0);
    let ok = AlignOptions::default();
    let refused = |moved: Datum, target: Datum, options: AlignOptions| {
        Motion::align(&moved, &target, &options).unwrap_err()
    };
    // A secondary along its primary, either way, on either side.
    for s in [DVec3::Z, DVec3::NEG_Z * 4.0] {
        let good = datum(p, DVec3::Z, Some(DVec3::X));
        let bad = datum(p, DVec3::Z, Some(s));
        assert_eq!(refused(bad, good, ok), AlignError::Parallel);
        assert_eq!(refused(good, bad, ok), AlignError::Parallel);
    }
    // Within 1e-9 (the sine) is parallel; a little more isn't.
    let tilted = |sine: f64| datum(p, DVec3::Z, Some(DVec3::new(sine, 0.0, 1.0)));
    let good = datum(p, DVec3::Z, Some(DVec3::X));
    assert_eq!(refused(tilted(0.5e-9), good, ok), AlignError::Parallel);
    let m = align(&tilted(2e-9), &good, &ok);
    assert_eq!(m.vector(DVec3::X), DVec3::X);
    // Directions of no length or not finite.
    for d in [DVec3::ZERO, DVec3::new(f64::NAN, 0.0, 1.0), DVec3::INFINITY] {
        assert_eq!(
            refused(datum(p, d, None), datum(p, DVec3::Z, None), ok),
            AlignError::Direction
        );
        assert_eq!(
            refused(
                datum(p, DVec3::Z, Some(d)),
                datum(p, DVec3::Z, Some(DVec3::X)),
                ok
            ),
            AlignError::Direction
        );
    }
    // Points out of range.
    for q in [DVec3::new(0.0, 0.0, 1.000_001e6), DVec3::NAN] {
        assert_eq!(
            refused(Datum::point(q), Datum::point(p), ok),
            AlignError::Point
        );
        assert_eq!(
            refused(datum(p, DVec3::Z, None), datum(q, DVec3::Z, None), ok),
            AlignError::Point
        );
    }
    // At the limit is fine.
    let far = DVec3::splat(1e6);
    assert_eq!(
        align(&Datum::point(-far), &Datum::point(far), &ok).point(-far),
        far
    );
    // Unpaired directions.
    let z = datum(p, DVec3::Z, None);
    let zx = datum(p, DVec3::Z, Some(DVec3::X));
    let lone = Datum {
        point: p,
        primary: None,
        secondary: Some(DVec3::X),
    };
    assert_eq!(refused(z, zx, ok), AlignError::Unpaired);
    assert_eq!(refused(zx, z, ok), AlignError::Unpaired);
    assert_eq!(refused(Datum::point(p), z, ok), AlignError::Unpaired);
    assert_eq!(refused(lone, lone, ok), AlignError::Unpaired);
    // An offset or a turn needs primaries, and must be in range.
    let options = |offset, degrees| AlignOptions {
        flip: false,
        offset,
        degrees,
    };
    let pt = Datum::point(p);
    assert_eq!(refused(pt, pt, options(1.0, 0.0)), AlignError::Options);
    assert_eq!(refused(pt, pt, options(0.0, 10.0)), AlignError::Options);
    assert_eq!(refused(z, z, options(2e6, 0.0)), AlignError::Options);
    assert_eq!(refused(z, z, options(f64::NAN, 0.0)), AlignError::Options);
    assert_eq!(
        refused(z, z, options(0.0, f64::INFINITY)),
        AlignError::Options
    );
    // A flip without primaries changes nothing.
    let flipped = AlignOptions { flip: true, ..ok };
    assert_eq!(align(&pt, &pt, &flipped), Motion::IDENTITY);
}

#[test]
fn aligning_is_deterministic() {
    let run = || {
        let mut rng = Rng::new(5);
        (0..50)
            .map(|_| {
                let moved = datum(
                    DVec3::ONE,
                    random_unit(&mut rng),
                    Some(random_unit(&mut rng)),
                );
                let target = datum(DVec3::ZERO, random_unit(&mut rng), None);
                let target = Datum {
                    secondary: Some(random_unit(&mut rng)),
                    ..target
                };
                let options = AlignOptions {
                    flip: true,
                    offset: 0.3,
                    degrees: 17.0,
                };
                align(&moved, &target, &options).bits()
            })
            .collect::<Vec<_>>()
    };
    let first = assert_deterministic(run);
    assert_eq!(first, run());
}
