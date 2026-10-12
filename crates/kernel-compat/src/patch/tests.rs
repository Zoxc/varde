#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_PI_2, PI};

use glam::{DVec2, DVec3};

use varde_testkit::rng::Rng;

use super::*;
use crate::trig;

/// How many random cases each property test runs.
const CASES: usize = 400;

/// The size rounding is relative to for these points: their largest
/// coordinate, at least 1.
fn scale(points: &[DVec3]) -> f64 {
    points
        .iter()
        .map(|p| p.abs().max_element())
        .fold(1.0, f64::max)
}

fn bits3(p: DVec3) -> [u64; 3] {
    p.to_array().map(f64::to_bits)
}

/// The two curves are the same to the last bit.
fn same_conic(a: &Conic3, b: &Conic3) -> bool {
    bits3(a.p0) == bits3(b.p0)
        && bits3(a.c) == bits3(b.c)
        && a.w.to_bits() == b.w.to_bits()
        && bits3(a.p1) == bits3(b.p1)
}

/// The denominator of `conic` at `t`.
fn conic_weight<P: Point>(conic: &Conic<P>, t: f64) -> f64 {
    let s = 1.0 - t;
    s * s + 2.0 * s * t * conic.w + t * t
}

/// The parameter of `parent` that a piece split from it between `t0` and
/// `t1` has at `s`: the standard form reparametrizes each piece
/// projectively.
fn parent_param<P: Point>(parent: &Conic<P>, t0: f64, t1: f64, s: f64) -> f64 {
    let l0 = (1.0 - s) / conic_weight(parent, t0).sqrt();
    let l1 = s / conic_weight(parent, t1).sqrt();
    let (a, b) = (l0 * (1.0 - t0) + l1 * (1.0 - t1), l0 * t0 + l1 * t1);
    b / (a + b)
}

/// A random conic in space: ends and control point anywhere in a box,
/// weight on a log scale within `w`.
fn random_conic(rng: &mut Rng, w: (f64, f64)) -> Conic3 {
    Conic3::new(
        rng.point(10.0),
        rng.point(10.0),
        rng.log_range(w.0, w.1),
        rng.point(10.0),
    )
    .unwrap()
}

/// A random patch that usually doesn't fold: a random triangle, each edge
/// control point near its midpoint, weights on a log scale within `w`.
fn random_patch(rng: &mut Rng, w: (f64, f64)) -> Patch {
    let p = [rng.point(10.0), rng.point(10.0), rng.point(10.0)];
    let c = [0, 1, 2].map(|i| {
        let (a, b) = (p[i], p[(i + 1) % 3]);
        (a + b) * 0.5 + rng.point(0.3 * (b - a).length())
    });
    let w = [0, 1, 2].map(|_| rng.log_range(w.0, w.1));
    Patch::new(p, c, w).unwrap()
}

#[test]
fn curves_end_at_their_ends() {
    let conic = Conic3::new(DVec3::X, DVec3::ONE, 0.5, DVec3::Z * 3.0).unwrap();
    assert_eq!(conic.eval(0.0), conic.p0);
    assert_eq!(conic.eval(1.0), conic.p1);

    let line = Conic2::line(DVec2::ZERO, DVec2::new(2.0, 4.0)).unwrap();
    assert_eq!(line.c, DVec2::new(1.0, 2.0));
    assert_eq!(line.eval(0.25), DVec2::new(0.5, 1.0));
    assert_eq!(line.eval_deriv(0.7).1, DVec2::new(2.0, 4.0));
}

#[test]
fn arcs_lie_on_their_circles() {
    let mut rng = Rng::new(1);
    for _ in 0..CASES {
        let center = DVec2::new(rng.range(-100.0, 100.0), rng.range(-100.0, 100.0));
        let radius = rng.log_range(1e-3, 1e3);
        let start = rng.range(-PI, PI);
        let sweep = rng.range(-FRAC_PI_2, FRAC_PI_2);
        let arc = Conic2::arc(center, radius, start, sweep).unwrap();
        assert_eq!(arc.w, trig::cos(sweep / 2.0));
        let tolerance = 1e-12 * (radius + center.abs().max_element());
        for i in 0..=16 {
            let t = i as f64 / 16.0;
            let (p, d) = arc.eval_deriv(t);
            assert!(((p - center).length() - radius).abs() <= tolerance);
            // Tangent to the circle, turning the way the sweep does.
            let r = p - center;
            assert!(r.dot(d).abs() <= 1e-9 * r.length() * d.length());
            assert!(r.perp_dot(d) * sweep > 0.0);
        }
        let end = center + DVec2::from_angle(start + sweep) * radius;
        assert!(arc.p1.distance(end) <= tolerance);
    }
}

#[test]
fn arcs_in_space_lie_on_their_circles() {
    let mut rng = Rng::new(2);
    for _ in 0..CASES {
        let center = rng.point(100.0);
        let x = rng.direction();
        let y = x.any_orthonormal_vector();
        let normal = x.cross(y);
        let radius = rng.log_range(1e-3, 1e3);
        let arc = Conic3::arc(center, x, y, radius, rng.range(-PI, PI), FRAC_PI_2).unwrap();
        let tolerance = 1e-12 * (radius + center.abs().max_element());
        for i in 0..=16 {
            let p = arc.eval(i as f64 / 16.0) - center;
            assert!((p.length() - radius).abs() <= tolerance);
            assert!(p.dot(normal).abs() <= tolerance);
        }
    }
}

#[test]
fn four_quarter_arcs_close_the_circle() {
    // Four quarters around: the ends meet, and the full turn closes.
    let arcs: Vec<Conic2> = (0..4)
        .map(|i| Conic2::arc(DVec2::ZERO, 2.0, i as f64 * FRAC_PI_2, FRAC_PI_2).unwrap())
        .collect();
    for i in 0..4 {
        assert!(arcs[i].p1.distance(arcs[(i + 1) % 4].p0) < 1e-15);
    }
    assert!((arcs[0].w - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-16);
    assert!(arcs[0].c.distance(DVec2::new(2.0, 2.0)) < 1e-14);
}

#[test]
fn arcs_between_their_ends_match_arcs_by_angle() {
    let center = DVec2::new(3.0, -1.5);
    for (radius, start, sweep) in [
        (2.0, 0.3, FRAC_PI_2),
        (2.0, 0.3, -FRAC_PI_2),
        (0.5, -2.0, 0.1),
        (1e4, 1.0, -1.2),
    ] {
        let by_angle = Conic2::arc(center, radius, start, sweep).unwrap();
        let between = Conic2::arc_between(center, radius, by_angle.p0, by_angle.p1).unwrap();
        let near = 1e-12 * (radius + center.length());
        assert!(between.c.distance(by_angle.c) <= near, "{radius} {sweep}");
        assert!((between.w - by_angle.w).abs() <= 1e-12);
        assert_eq!((between.p0, between.p1), (by_angle.p0, by_angle.p1));
    }
    // A half turn has no control point.
    let (a, b) = (center + DVec2::X, center - DVec2::X);
    assert!(Conic2::arc_between(center, 1.0, a, b).is_err());
}

#[test]
fn arcs_by_angle_take_their_bits_from_libm() {
    // The same bits on every platform: `libm`'s `cos` and `sin`, never
    // the platform's maths library.
    let at = |angle: f64, r: f64| DVec2::new(libm::cos(angle), libm::sin(angle)) * r;
    let mut rng = Rng::new(7);
    for _ in 0..CASES {
        let center = DVec2::new(rng.range(-100.0, 100.0), rng.range(-100.0, 100.0));
        let radius = rng.log_range(1e-3, 1e3);
        let start = rng.range(-4.0 * PI, 4.0 * PI);
        let sweep = rng.range(-FRAC_PI_2, FRAC_PI_2);
        if sweep == 0.0 {
            continue;
        }
        let half = sweep * 0.5;
        let w = libm::cos(half);
        let points = [
            at(start, radius),
            at(start + half, radius / w),
            at(start + sweep, radius),
        ];
        let arc = Conic2::arc(center, radius, start, sweep).unwrap();
        assert_eq!(
            [arc.p0, arc.c, arc.p1],
            points.map(|p| center + p),
            "{start} {sweep}"
        );
        assert_eq!(arc.w.to_bits(), w.to_bits());

        let x = rng.direction();
        let y = x.any_orthonormal_vector();
        let center = rng.point(100.0);
        let place = |p: DVec2| center + x * p.x + y * p.y;
        let arc = Conic3::arc(center, x, y, radius, start, sweep).unwrap();
        assert_eq!([arc.p0, arc.c, arc.p1], points.map(place));
        assert_eq!(arc.w.to_bits(), w.to_bits());
    }
}

#[test]
fn bad_arcs_are_refused() {
    let arc = |radius, start, sweep| Conic2::arc(DVec2::ZERO, radius, start, sweep);
    assert!(arc(1.0, 0.0, FRAC_PI_2).is_ok());
    assert!(arc(1.0, 0.0, -FRAC_PI_2).is_ok());
    assert!(arc(1.0, 0.0, 3.0 * FRAC_PI_2 / 3.0).is_ok());
    for (radius, start, sweep) in [
        (1.0, 0.0, 0.0),
        (1.0, 0.0, FRAC_PI_2 * 1.001),
        (1.0, 0.0, -PI),
        (0.0, 0.0, 1.0),
        (-1.0, 0.0, 1.0),
        (f64::NAN, 0.0, 1.0),
        (1.0, f64::NAN, 1.0),
        (1.0, 0.0, f64::NAN),
        (1.0, f64::INFINITY, 1.0),
        (1e9, 0.0, 1.0),
    ] {
        assert!(
            matches!(arc(radius, start, sweep), Err(PatchError::Parameter(_))),
            "{radius} {start} {sweep}"
        );
    }
}

#[test]
fn bad_weights_are_refused() {
    let conic = |w| Conic3::new(DVec3::ZERO, DVec3::Y, w, DVec3::X);
    let patch = |w| {
        Patch::new(
            [DVec3::ZERO, DVec3::X, DVec3::Y],
            [DVec3::ZERO; 3],
            [1.0, w, 1.0],
        )
    };
    assert!(conic(W_MIN).is_ok() && conic(W_MAX).is_ok());
    assert!(patch(W_MIN).is_ok() && patch(W_MAX).is_ok());
    for w in [
        0.0,
        -0.5,
        W_MIN * 0.99,
        W_MAX * 1.01,
        f64::NAN,
        f64::INFINITY,
        -f64::INFINITY,
    ] {
        assert!(matches!(conic(w), Err(PatchError::Weight(_))), "{w}");
        assert!(matches!(patch(w), Err(PatchError::Weight(_))), "{w}");
        let mut c = conic(1.0).unwrap();
        c.w = w;
        assert!(c.check().is_err());
    }
}

#[test]
fn non_positive_homogeneous_weights_are_refused() {
    let conic = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    for w in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        for slot in 0..3 {
            let mut h = conic.hom();
            h[slot].w = w;
            assert!(matches!(Conic3::from_hom(h), Err(PatchError::Weight(_))));
        }
    }
    // Scaling a whole end leaves the curve as it was.
    let h = conic.hom();
    let same = Conic3::from_hom([h[0] * 4.0, h[1] * 2.0, h[2]]).unwrap();
    assert!(same_conic(&same, &conic));
}

#[test]
fn homogeneous_points_at_any_scale_give_the_same_curve() {
    // Powers of two scale exactly, so the results must match to the bit;
    // the end weights' product would overflow or go subnormal.
    let scales = [2f64.powi(530), 2f64.powi(-530)];
    let mut rng = Rng::new(21);
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (W_MIN, W_MAX));
        let conic = patch.edge(0);
        let (a, b) = (rng.range(0.0, 0.5), rng.range(0.5, 1.0));
        let h = [
            conic.blossom(a, a),
            conic.blossom(a, b),
            conic.blossom(b, b),
        ];
        let Ok(piece) = Conic3::from_hom(h) else {
            continue;
        };
        for s in scales {
            let scaled = Conic3::from_hom(h.map(|x| x * s)).unwrap();
            assert!(same_conic(&scaled, &piece), "{scaled:?} {piece:?}");
        }
    }
}

#[test]
fn bad_coordinates_are_refused() {
    for x in [f64::NAN, f64::INFINITY, MAX_CONTROL * 1.5] {
        let p = DVec3::new(0.0, x, 0.0);
        assert!(matches!(
            Conic3::new(DVec3::ZERO, p, 1.0, DVec3::X),
            Err(PatchError::Coordinate(_))
        ));
        assert!(matches!(
            Patch::flat([DVec3::ZERO, DVec3::X, p]),
            Err(PatchError::Coordinate(_))
        ));
    }
    assert!(Conic2::line(DVec2::ZERO, DVec2::splat(MAX_CONTROL)).is_ok());
}

#[test]
fn curve_derivatives_match_differences() {
    let mut rng = Rng::new(3);
    for _ in 0..CASES {
        let conic = random_conic(&mut rng, (0.1, 10.0));
        let t = rng.range(0.1, 0.9);
        let h = 1e-6;
        let (_, d) = conic.eval_deriv(t);
        let diff = (conic.eval(t + h) - conic.eval(t - h)) / (2.0 * h);
        assert!(d.distance(diff) <= 1e-6 * d.length().max(1.0), "{d} {diff}");
    }
}

#[test]
fn curve_blossom_gives_points_and_controls() {
    let mut rng = Rng::new(4);
    for _ in 0..CASES {
        let conic = random_conic(&mut rng, (W_MIN, W_MAX));
        let t = rng.unit();
        let h = conic.blossom(t, t);
        let p = h.truncate() / h.w;
        assert!(p.distance(conic.eval(t)) <= 1e-12 * scale(&conic.hull()));
        assert_eq!(conic.blossom(0.0, 1.0), conic.hom()[1]);
        assert_eq!(conic.blossom(0.3, 0.8), conic.blossom(0.8, 0.3));
    }
}

#[test]
fn splitting_a_curve_reproduces_it() {
    let mut rng = Rng::new(5);
    for _ in 0..CASES {
        let conic = random_conic(&mut rng, (W_MIN, W_MAX));
        let t = if rng.unit() < 0.2 {
            0.5
        } else {
            rng.range(0.01, 0.99)
        };
        let [left, right] = conic.split(t).unwrap();
        assert_eq!(left.p0, conic.p0);
        assert_eq!(right.p1, conic.p1);
        assert_eq!(left.p1, right.p0);
        let tolerance = 1e-12 * scale(&conic.hull());
        for i in 0..=8 {
            let s = i as f64 / 8.0;
            let l = conic.eval(parent_param(&conic, 0.0, t, s));
            let r = conic.eval(parent_param(&conic, t, 1.0, s));
            assert!(left.eval(s).distance(l) <= tolerance);
            assert!(right.eval(s).distance(r) <= tolerance);
        }
        // Weights move towards 1, so they stay within bounds.
        for half in [left, right] {
            assert!(half.w >= conic.w.min(1.0) - 1e-15 && half.w <= conic.w.max(1.0) + 1e-15);
        }
    }
}

#[test]
fn halving_is_symmetric_to_the_bit() {
    let mut rng = Rng::new(6);
    for _ in 0..CASES {
        let conic = random_conic(&mut rng, (W_MIN, W_MAX));
        let [l, r] = conic.split_half().unwrap();
        let [rl, rr] = conic.reversed().split_half().unwrap();
        assert!(same_conic(&rl, &r.reversed()));
        assert!(same_conic(&rr, &l.reversed()));
        let [sl, sr] = conic.split(0.5).unwrap();
        assert!(same_conic(&sl, &l) && same_conic(&sr, &r));
    }
}

#[test]
fn split_positions_outside_the_curve_are_refused() {
    let conic = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    for t in [0.0, 1.0, -0.5, 2.0, f64::NAN] {
        assert!(matches!(conic.split(t), Err(PatchError::Parameter(_))));
    }
}

#[test]
fn patches_meet_their_corners_and_edges() {
    let mut rng = Rng::new(7);
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (W_MIN, W_MAX));
        let corners = [DVec3::X, DVec3::Y, DVec3::Z];
        for i in 0..3 {
            assert_eq!(patch.eval(corners[i]), patch.p[i]);
        }
        let tolerance = 1e-12 * scale(&patch.hull());
        for i in 0..3 {
            let t = rng.unit();
            let mut u = DVec3::ZERO;
            u[i] = 1.0 - t;
            u[(i + 1) % 3] = t;
            assert!(patch.eval(u).distance(patch.edge(i).eval(t)) <= tolerance);
        }
    }
}

#[test]
fn flat_triangles_have_the_flat_normal() {
    let p = [
        DVec3::ZERO,
        DVec3::new(2.0, 0.0, 0.0),
        DVec3::new(0.0, 3.0, 1.0),
    ];
    let patch = Patch::flat(p).unwrap();
    let flat = (p[1] - p[0]).cross(p[2] - p[0]);
    let mut rng = Rng::new(25);
    for b in [DVec3::X, DVec3::Y, DVec3::Z, DVec3::splat(1.0 / 3.0)]
        .into_iter()
        .chain((0..20).map(|_| rng.bary()))
    {
        assert!(patch.normal(b).distance(flat) < 1e-12, "{b}");
    }
}

#[test]
fn bounds_hold_the_patch() {
    let mut rng = Rng::new(18);
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (W_MIN, W_MAX));
        let bounds = patch.bounds();
        assert_eq!(Some(bounds), Bounds::around(&patch.hull()));
        for _ in 0..20 {
            let p = patch.eval(rng.bary());
            assert!(p.cmpge(bounds.min - 1e-12).all() && p.cmple(bounds.max + 1e-12).all());
        }
        let conic = patch.edge(0).bounds();
        assert_eq!(bounds.union(conic), bounds);
    }
}

#[test]
fn pieces_keep_the_coordinates_their_curve_shares() {
    // Arcs in planes square to the axes, at coordinates rounding would
    // move: their halves, their points and the points of a patch on them
    // stay in the plane to the bit, as a cap refined or cut must.
    let mut rng = Rng::new(91);
    for _ in 0..200 {
        let z = rng.range(-100.0, 100.0);
        let (c, r) = (
            DVec2::new(rng.range(-5.0, 5.0), rng.range(-5.0, 5.0)),
            rng.range(0.1, 3.0),
        );
        let a = rng.range(0.0, 6.0);
        let arc = Conic2::arc(c, r, a, rng.range(0.2, 1.5)).unwrap();
        let lift = |p: DVec2| DVec3::new(p.x, p.y, z);
        let arc3 = Conic3::new(lift(arc.p0), lift(arc.c), arc.w, lift(arc.p1)).unwrap();
        for half in arc3
            .split_half()
            .unwrap()
            .into_iter()
            .chain(arc3.split(rng.range(0.1, 0.9)).unwrap())
        {
            assert!(half.hull().iter().all(|p| p.z == z));
        }
        assert_eq!(arc3.eval(rng.unit()).z, z);
        let patch = Patch::new(
            [arc3.p0, arc3.p1, lift(c)],
            [arc3.c, (arc3.p1 + lift(c)) * 0.5, (lift(c) + arc3.p0) * 0.5],
            [arc3.w, 1.0, 1.0],
        )
        .unwrap();
        assert!(patch.hull().iter().all(|p| p.z == z));
        assert_eq!(patch.eval(rng.bary()).z, z);
    }
}
