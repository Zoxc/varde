use std::f64::consts::{FRAC_PI_2, PI};

use glam::{DVec2, DVec3};

use super::fold::{fold_direction, smallest_cone};
use super::*;
use crate::test_rng::Rng;

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

/// The point of `parent` that the sub-patch over `domain` has at `b`.
fn parent_point(parent: &Patch, domain: [DVec3; 3], b: DVec3) -> DVec3 {
    let q: DVec3 = (0..3)
        .map(|i| domain[i] * (b[i] / parent.weight_at(domain[i]).sqrt()))
        .sum();
    parent.eval(q)
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

/// A random barycentric triangle inside the domain, counter-clockwise.
fn random_domain(rng: &mut Rng) -> [DVec3; 3] {
    loop {
        let d = [rng.bary(), rng.bary(), rng.bary()];
        let area = (d[1] - d[0]).cross(d[2] - d[0]).dot(DVec3::ONE);
        if area > 0.01 {
            return d;
        }
    }
}

/// Every sampled point of `child` is the matching point of `parent` over
/// `domain`.
fn assert_reproduces(parent: &Patch, domain: [DVec3; 3], child: &Patch, rng: &mut Rng) {
    let tolerance = 1e-12 * scale(&parent.hull());
    for _ in 0..20 {
        let b = rng.bary();
        let error = child.eval(b).distance(parent_point(parent, domain, b));
        assert!(error <= tolerance, "{error} off the parent");
    }
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
        assert_eq!(arc.w, (sweep / 2.0).cos());
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
fn a_quarter_circle_splits_into_equal_quarters() {
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
            assert_eq!(patch.weight_at(corners[i]), 1.0);
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
fn patch_derivatives_and_normals_agree() {
    let mut rng = Rng::new(8);
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (0.1, 10.0));
        let b = rng.bary() * 0.8 + DVec3::splat(0.2 / 3.0);
        let [p, du, dv] = patch.eval_derivs(b);
        assert!(p.distance(patch.eval(b)) <= 1e-12 * scale(&patch.hull()));
        let h = 1e-6;
        let at = |du: f64, dv: f64| patch.eval(b + DVec3::new(du, dv, -du - dv));
        let fu = (at(h, 0.0) - at(-h, 0.0)) / (2.0 * h);
        let fv = (at(0.0, h) - at(0.0, -h)) / (2.0 * h);
        assert!(du.distance(fu) <= 1e-6 * du.length().max(1.0));
        assert!(dv.distance(fv) <= 1e-6 * dv.length().max(1.0));

        // The normal is the cross product times the cube of the weight.
        let normal = patch.normal(b);
        let expected = du.cross(dv) * patch.weight_at(b).powi(3);
        assert!(normal.distance(expected) <= 1e-9 * expected.length().max(1.0));

        // And the Bernstein coefficients sum to it.
        let coeffs = patch.normal_coeffs();
        let mut sum = DVec3::ZERO;
        for (index, c) in fold::NORMAL_INDEX.iter().zip(coeffs) {
            let fact = |n: u8| [1.0, 1.0, 2.0, 6.0][n as usize];
            let multinomial = 6.0 / (fact(index[0]) * fact(index[1]) * fact(index[2]));
            let monomial =
                b.x.powi(index[0] as i32) * b.y.powi(index[1] as i32) * b.z.powi(index[2] as i32);
            sum += c * (multinomial * monomial);
        }
        assert!(sum.distance(normal) <= 1e-9 * normal.length().max(1.0));
    }
}

#[test]
fn the_corner_coefficient_is_the_corner_cross_product() {
    let mut rng = Rng::new(9);
    let patch = random_patch(&mut rng, (0.5, 2.0));
    let coeffs = patch.normal_coeffs();
    // At corner 0 the derivatives are 2·w01·(c01 − p0) and 2·w20·(c20 − p0).
    let expected = ((patch.c[0] - patch.p[0]) * (2.0 * patch.w[0]))
        .cross((patch.c[2] - patch.p[0]) * (2.0 * patch.w[2]));
    assert!(coeffs[0].distance(expected) <= 1e-9 * expected.length());
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
    for c in patch.normal_coeffs() {
        assert!(c.distance(flat) < 1e-12);
    }
    assert_eq!(patch.fold_direction(), Some(flat.normalize()));
    let cone = patch.normal_cone();
    assert!(cone.axis.distance(flat.normalize()) < 1e-12);
    assert!(cone.angle < 1e-6);
}

#[test]
fn folded_and_degenerate_patches_fail_the_fold_check() {
    let p = [DVec3::ZERO, DVec3::X, DVec3::Y];
    // Edge 01 pulled past the opposite corner: the patch folds over.
    let folded = Patch::new(
        p,
        [
            DVec3::new(0.5, 2.0, 0.0),
            DVec3::new(0.5, 0.5, 0.0),
            DVec3::new(0.0, 0.5, 0.0),
        ],
        [1.0; 3],
    )
    .unwrap();
    let signs: Vec<f64> = (0..=10)
        .flat_map(|i| (0..=10 - i).map(move |j| (i, j)))
        .map(|(i, j)| {
            let (u, v) = (i as f64 / 10.0, j as f64 / 10.0);
            folded.normal(DVec3::new(u, v, 1.0 - u - v)).z
        })
        .collect();
    assert!(signs.iter().any(|&z| z > 0.0) && signs.iter().any(|&z| z < 0.0));
    assert_eq!(folded.fold_direction(), None);

    // Edge 01 leaves corner 0 along edge 20: a corner of 0°.
    let p = [DVec3::ZERO, DVec3::new(2.0, 0.0, 0.0), DVec3::ONE];
    let cusp = Patch::new(
        p,
        [
            DVec3::new(1.0, 1.0, 1.0),
            (p[1] + p[2]) * 0.5,
            (p[2] + p[0]) * 0.5,
        ],
        [1.0; 3],
    )
    .unwrap();
    assert_eq!(cusp.fold_direction(), None);

    // All corners on a line.
    let line = Patch::flat([DVec3::ZERO, DVec3::X, DVec3::X * 2.0]).unwrap();
    assert_eq!(line.fold_direction(), None);
}

#[test]
fn the_fold_check_finds_the_exact_direction_when_the_quick_ones_fail() {
    let near = DVec3::new(1.0, 0.0, 0.05).normalize();
    let far = DVec3::new(-0.3, 0.0, 1.0).normalize();
    let mut dirs = vec![near; 9];
    dirs.push(far);
    let hint = DVec3::X;
    let mean: DVec3 = dirs.iter().copied().sum::<DVec3>().normalize();
    assert!(hint.dot(far) < 0.0 && mean.dot(far) < 0.0);
    let d = fold_direction(&dirs, hint).unwrap();
    assert!(dirs.iter().all(|c| c.dot(d) > FOLD_MARGIN));
    // The two ends of the spread sit on the cone's rim.
    assert!((d.dot(near) - d.dot(far)).abs() < 1e-12);

    // Spread over more than a half-space: nothing passes.
    dirs.push(-DVec3::Z);
    dirs.push(DVec3::new(-1.0, 0.0, -0.2).normalize());
    assert_eq!(fold_direction(&dirs, hint), None);
}

#[test]
fn the_smallest_cone_beats_every_other_axis() {
    let mut rng = Rng::new(10);
    for _ in 0..100 {
        let axis = rng.direction();
        let spread = rng.range(0.05, 1.4);
        let dirs: Vec<DVec3> = (0..10)
            .map(|_| {
                loop {
                    let d = rng.direction();
                    if d.dot(axis) >= spread.cos() {
                        break d;
                    }
                }
            })
            .collect();
        let (best, least) = smallest_cone(&dirs);
        assert!((best.length() - 1.0).abs() < 1e-12);
        let score = |d: DVec3| dirs.iter().map(|c| c.dot(d)).fold(f64::INFINITY, f64::min);
        assert!((score(best) - least).abs() < 1e-15);
        assert!(least >= score(axis) - 1e-12);
        for _ in 0..200 {
            let other = (best + rng.direction() * rng.range(0.0, 0.2)).normalize();
            assert!(least >= score(other) - 1e-12);
        }
    }
}

#[test]
fn normal_cones_hold_the_normals() {
    let mut rng = Rng::new(11);
    let mut narrow = 0;
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (0.2, 5.0));
        let cone = patch.normal_cone();
        if cone.angle >= PI {
            continue;
        }
        narrow += 1;
        for _ in 0..40 {
            let n = patch.normal(rng.bary()).normalize();
            assert!(n.dot(cone.axis).clamp(-1.0, 1.0).acos() <= cone.angle + 1e-9);
        }
        // A patch that passes the fold check has a cone narrower than a
        // half-space.
        if patch.fold_direction().is_some() {
            assert!(cone.angle < FRAC_PI_2);
        }
    }
    assert!(narrow > CASES / 2);
}

#[test]
fn cones_apart_are_neither_parallel_nor_opposite() {
    let cone = |axis: DVec3, angle| NormalCone { axis, angle };
    let up = cone(DVec3::Z, 0.3);
    assert!(up.apart(&cone(DVec3::X, 0.3)));
    assert!(!up.apart(&cone(DVec3::Z, 0.1)));
    assert!(!up.apart(&cone(-DVec3::Z, 0.1)));
    assert!(!up.apart(&cone(DVec3::new(1.0, 0.0, 1.0).normalize(), 0.6)));
    assert!(!up.apart(&cone(DVec3::X, PI)));
}

#[test]
fn sub_patches_reproduce_the_parent() {
    let mut rng = Rng::new(12);
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (0.2, 5.0));
        let domain = random_domain(&mut rng);
        let Ok(child) = patch.sub(domain) else {
            continue;
        };
        assert_reproduces(&patch, domain, &child, &mut rng);
    }
}

#[test]
fn splitting_a_patch_reproduces_it() {
    let mut rng = Rng::new(13);
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (0.2, 5.0));
        let children = patch.split4().unwrap();
        for (child, domain) in children.iter().zip(Patch::SPLIT4_DOMAINS) {
            assert_reproduces(&patch, domain, child, &mut rng);
        }

        let edge = (rng.next_u64() % 3) as usize;
        let t = rng.range(0.05, 0.95);
        let children = patch.bisect(edge, t).unwrap();
        let (a, b, o) = (edge, (edge + 1) % 3, (edge + 2) % 3);
        let e = |i: usize| DVec3::from_array(std::array::from_fn(|j| (i == j) as u8 as f64));
        let m = e(a) * (1.0 - t) + e(b) * t;
        assert_reproduces(&patch, [e(a), m, e(o)], &children[0], &mut rng);
        assert_reproduces(&patch, [m, e(b), e(o)], &children[1], &mut rng);
    }
}

#[test]
fn split_children_share_their_inner_edges() {
    let mut rng = Rng::new(14);
    let patch = random_patch(&mut rng, (0.2, 5.0));
    let [c0, c1, c2, c3] = patch.split4().unwrap();
    // Each corner child's inner edge is the middle child's, reversed.
    assert!(same_conic(&c0.edge(1), &c3.edge(2).reversed()));
    assert!(same_conic(&c1.edge(2), &c3.edge(0).reversed()));
    assert!(same_conic(&c2.edge(0), &c3.edge(1).reversed()));
    // And neighbouring corner children share the halves' midpoints.
    assert_eq!(c0.p[1], c1.p[0]);
    assert_eq!(c1.p[2], c2.p[1]);
    assert_eq!(c2.p[0], c0.p[2]);

    let [a, b] = patch.bisect(1, 0.3).unwrap();
    assert!(same_conic(&a.edge(1), &b.edge(2).reversed()));
    assert_eq!(a.p[1], b.p[0]);
}

#[test]
fn neighbours_split_a_shared_edge_to_the_same_bits() {
    let mut rng = Rng::new(15);
    for _ in 0..CASES {
        let a = random_patch(&mut rng, (0.2, 5.0));
        // `b` shares a's edge 0, running the other way as its edge 2.
        let other = rng.point(10.0);
        let b = Patch::new(
            [other, a.p[1], a.p[0]],
            [(other + a.p[1]) * 0.5, a.c[0], (a.p[0] + other) * 0.5],
            [1.0, a.w[0], 1.0],
        )
        .unwrap();
        let [a0, a1, ..] = a.split4().unwrap();
        // a's halves: a0's edge 0 runs p0 → m, a1's edge 0 runs m → p1.
        let (a_first, a_second) = (a0.edge(0), a1.edge(0));

        for b_children in [
            b.split4().unwrap().to_vec(),
            b.bisect(1, 0.5).unwrap().to_vec(),
        ] {
            // b's halves of its edge 1, from a.p1 through m to a.p0.
            let halves: Vec<Conic3> = b_children
                .iter()
                .flat_map(|c| (0..3).map(|i| c.edge(i)))
                .filter(|e| {
                    (e.p0 == a.p[1] || e.p1 == a.p[0]) && (e.p0 == a_first.p1 || e.p1 == a_first.p1)
                })
                .collect();
            assert_eq!(halves.len(), 2);
            let b_first = halves.iter().find(|e| e.p0 == a.p[1]).unwrap();
            let b_second = halves.iter().find(|e| e.p1 == a.p[0]).unwrap();
            assert!(same_conic(b_first, &a_second.reversed()));
            assert!(same_conic(b_second, &a_first.reversed()));
        }
    }
}

#[test]
fn halves_split_once_serve_both_sides() {
    let mut rng = Rng::new(16);
    let a = random_patch(&mut rng, (0.2, 5.0));
    let other = rng.point(10.0);
    let b = Patch::new(
        [a.p[1], a.p[0], other],
        [a.c[0], (a.p[0] + other) * 0.5, (other + a.p[1]) * 0.5],
        [a.w[0], 1.0, 1.0],
    )
    .unwrap();
    let t = 0.3;
    let [l, r] = a.edge(0).split(t).unwrap();
    let [a0, a1] = a.bisect_with(0, t, [l, r]).unwrap();
    let [b0, b1] = b
        .bisect_with(0, 1.0 - t, [r.reversed(), l.reversed()])
        .unwrap();
    assert!(same_conic(&a0.edge(0), &b1.edge(0).reversed()));
    assert!(same_conic(&a1.edge(0), &b0.edge(0).reversed()));

    // Halves of another edge, or that don't meet, are refused.
    assert_eq!(b.bisect_with(0, t, [l, r]), Err(PatchError::Mismatch));
    let [l2, r2] = a.edge(0).split(0.6).unwrap();
    assert_eq!(a.bisect_with(0, t, [l, r2]), Err(PatchError::Mismatch));
    assert!(a.bisect_with(0, t, [l2, r2]).is_ok());
    assert!(matches!(a.bisect(3, 0.5), Err(PatchError::Parameter(_))));
    assert!(matches!(a.bisect(0, 1.0), Err(PatchError::Parameter(_))));
}

#[test]
fn splits_refuse_weights_out_of_bounds() {
    // Edge 01 nearly straight, the others bulging hard: the inner edge of
    // a bisection gets a weight past W_MAX.
    let p = [DVec3::ZERO, DVec3::X, DVec3::Y];
    let patch = Patch::new(
        p,
        [
            DVec3::new(0.5, 0.0, 0.0),
            DVec3::new(0.6, 0.6, 0.0),
            DVec3::new(-0.1, 0.5, 0.0),
        ],
        [W_MIN, W_MAX, W_MAX],
    )
    .unwrap();
    assert!(matches!(patch.bisect(0, 0.5), Err(PatchError::Weight(_))));
}

#[test]
fn folds_stay_away_from_split_pieces() {
    let mut rng = Rng::new(17);
    let mut passed = 0;
    for _ in 0..CASES {
        let patch = random_patch(&mut rng, (0.2, 5.0));
        let Some(d) = patch.fold_direction() else {
            continue;
        };
        passed += 1;
        let mut children = patch.split4().unwrap().to_vec();
        let edge = (rng.next_u64() % 3) as usize;
        children.extend(patch.bisect(edge, rng.range(0.05, 0.95)).unwrap());
        if let Ok(child) = patch.sub(random_domain(&mut rng)) {
            children.push(child);
        }
        for child in children {
            assert!(child.fold_direction().is_some(), "{patch:?} → {child:?}");
            // The parent's direction still works for the child.
            for c in child.normal_coeffs() {
                assert!(c.dot(d) > FOLD_MARGIN * c.length());
            }
        }
    }
    assert!(passed > CASES / 2, "{passed}");
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

/// A random frame: an origin and orthonormal axes, `z` last.
fn random_frame(rng: &mut Rng) -> (DVec3, DVec3, DVec3, DVec3) {
    let z = rng.direction();
    let x = z.any_orthonormal_vector();
    let angle = rng.range(0.0, 2.0 * PI);
    let x = x * angle.cos() + z.cross(x) * angle.sin();
    (rng.point(100.0), x, z.cross(x), z)
}

#[test]
fn cylinder_strips_lie_on_the_cylinder() {
    let mut rng = Rng::new(19);
    for _ in 0..CASES {
        let (origin, x, y, z) = random_frame(&mut rng);
        let radius = rng.log_range(0.01, 100.0);
        let height = radius * rng.log_range(1e-3, 1e3);
        let sweep = rng.log_range(1e-3, FRAC_PI_2);
        let start = rng.range(-PI, PI);
        let bottom = Conic3::arc(origin, x, y, radius, start, sweep).unwrap();
        let strip = cylinder_strip(&bottom, z * height).unwrap();
        assert_eq!(strip[0].w, [bottom.w, 1.0, bottom.w]);

        let tolerance = 1e-12 * (radius + height + origin.abs().max_element());
        for patch in &strip {
            assert!(patch.fold_direction().is_some(), "{patch:?}");
            for _ in 0..40 {
                let p = patch.eval(rng.bary()) - origin;
                let along = p.dot(z);
                let off = (p - z * along).length() - radius;
                assert!(off.abs() <= tolerance, "{off}");
                assert!(along >= -tolerance && along <= height + tolerance);
                // Within the arc's angles.
                let angle = (p.dot(y)).atan2(p.dot(x)) - start;
                let angle = (angle + PI).rem_euclid(2.0 * PI) - PI;
                assert!(angle >= -1e-9 && angle <= sweep + 1e-9, "{angle}");
            }
            // Normals point out of the cylinder.
            let b = DVec3::splat(1.0 / 3.0);
            let p = patch.eval(b) - origin;
            assert!(patch.normal(b).dot(p - z * p.dot(z)) > 0.0);
        }
    }
}

#[test]
fn cylinder_strips_share_their_edges() {
    let bottom = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 2.0, 0.3, 1.2).unwrap();
    let offset = DVec3::new(0.0, 0.0, 3.0);
    let [t1, t2] = cylinder_strip(&bottom, offset).unwrap();
    let top = bottom.translated(offset);
    assert!(same_conic(&t1.edge(0), &bottom));
    assert!(same_conic(&t2.edge(1), &top.reversed()));
    assert!(same_conic(&t1.edge(2), &t2.edge(0).reversed()));
    assert!(same_conic(
        &t1.edge(1),
        &Conic3::line(bottom.p1, top.p1).unwrap()
    ));
    assert!(same_conic(
        &t2.edge(2),
        &Conic3::line(top.p0, bottom.p0).unwrap()
    ));

    // The next strip round shares the ruling between them.
    let next = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 2.0, 1.5, 0.9).unwrap();
    let next = Conic3 {
        p0: bottom.p1,
        ..next
    };
    let [_, n2] = cylinder_strip(&next, offset).unwrap();
    assert!(same_conic(&n2.edge(2), &t1.edge(1).reversed()));
}

#[test]
fn strips_over_any_conic_lie_on_its_cylinder() {
    let mut rng = Rng::new(20);
    let mut built = 0;
    for i in 0..CASES {
        let (origin, x, y, z) = random_frame(&mut rng);
        let local = |u: f64, v: f64| origin + x * u + y * v;
        let w = if i % 10 == 0 {
            1.0
        } else {
            rng.log_range(0.3, 3.0)
        };
        // The ends on the x axis, the control point above.
        let bottom = Conic3::new(
            local(-1.0, 0.0),
            local(rng.range(-0.5, 0.5), rng.range(0.3, 2.0)),
            w,
            local(1.0, 0.0),
        )
        .unwrap();
        let offset = z * rng.log_range(0.1, 10.0) + x * rng.range(-0.5, 0.5);
        let Ok(strip) = cylinder_strip(&bottom, offset) else {
            continue;
        };
        built += 1;
        // A point is on the cylinder if moving it along the offset into
        // the bottom's plane lands on the conic, whose barycentric
        // coordinates in the control triangle have βc² = 4w²·β0·β1.
        for patch in &strip {
            for _ in 0..20 {
                let p = patch.eval(rng.bary());
                let p = p - offset * ((p - origin).dot(z) / offset.dot(z));
                let (u, v) = ((p - origin).dot(x), (p - origin).dot(y));
                let (cu, cv) = ((bottom.c - origin).dot(x), (bottom.c - origin).dot(y));
                let beta_c = v / cv;
                let beta_1 = (u + 1.0 - beta_c * (cu + 1.0)) / 2.0;
                let beta_0 = 1.0 - beta_c - beta_1;
                let residual = beta_c * beta_c - 4.0 * w * w * beta_0 * beta_1;
                assert!(residual.abs() <= 1e-10, "{residual}");
            }
        }
    }
    assert!(built > CASES * 9 / 10, "{built}");
}

#[test]
fn strips_over_lines_are_flat() {
    let line = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    let [t1, t2] = cylinder_strip(&line, DVec3::Z).unwrap();
    for patch in [t1, t2] {
        assert!(patch.hull().iter().all(|p| p.y == 0.0));
        assert!(same_conic(
            &patch.edge(0),
            &Conic3::line(patch.p[0], patch.p[1]).unwrap()
        ));
        assert!(patch.fold_direction().is_some());
    }
}

#[test]
fn strips_refuse_offsets_that_stay_flat() {
    let line = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    assert_eq!(
        cylinder_strip(&line, DVec3::X * -2.0),
        Err(PatchError::Degenerate)
    );
    let arc = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 1.0, 0.0, 1.0).unwrap();
    assert_eq!(cylinder_strip(&arc, DVec3::X), Err(PatchError::Degenerate));
    assert_eq!(
        cylinder_strip(&arc, DVec3::ZERO),
        Err(PatchError::Degenerate)
    );
    assert!(cylinder_strip(&arc, DVec3::Z).is_ok());
    assert!(cylinder_strip(&arc, DVec3::new(1.0, 0.0, 1e-6)).is_ok());
}
