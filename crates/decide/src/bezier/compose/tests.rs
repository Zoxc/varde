//! Rational spans and the compositions with linear forms: on rationals
//! exact against the point's values; at every level each certified
//! coefficient holds the exact one; reversal gives the same bits, the
//! images keep the weights' bits, factors 0 and ±1 round nothing.

use malachite::Rational as Q;

use super::*;
use crate::bezier::MAX_DEGREE;
use crate::bezier::tensor::Dir;
use crate::bezier::tests::{Level, Rng, holds, poly_holds, q, tensor_holds};
use crate::num::bigf::Big;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

fn int(rng: &mut Rng) -> Q {
    Q::from(rng.below(201) as i64 - 100)
}

/// A positive weight in `[1/8, 8]`.
fn weight(rng: &mut Rng) -> Q {
    Q::from(rng.below(64) + 1) / Q::from(8u32)
}

fn dyadic(rng: &mut Rng) -> Q {
    Q::from(rng.below(65)) / Q::from(64u32)
}

fn int_curve(rng: &mut Rng, n: usize) -> HomCurve<Q, 3> {
    HomCurve::new((0..=n).map(|_| {
        let w = weight(rng);
        ([0, 1, 2].map(|_| int(rng) * &w), w)
    }))
    .expect("degree in range")
}

/// A level's span with positive weights and its true span.
fn curve<S: Level>(rng: &mut Rng, n: usize, errs: bool) -> (HomCurve<S, 3>, HomCurve<Q, 3>) {
    let mut p: Vec<(Poly<S>, Poly<Q>)> = Vec::new();
    for _ in 0..3 {
        let (x, t) = rng.poly::<S>(n, errs);
        p.push((x, Poly::new(t).expect("degree in range")));
    }
    let ws: Vec<(f64, f64)> = (0..=n)
        .map(|_| {
            let hi = rng.pow(-6, 5).abs();
            (hi, hi * rng.pow(-80, -54).abs())
        })
        .collect();
    let w = Poly::new(ws.iter().map(|&(h, l)| S::of(h, l))).expect("degree in range");
    let wq = Poly::new(w.coefs().iter().map(Level::val)).expect("degree in range");
    (
        HomCurve::from_polys(std::array::from_fn(|k| p[k].0.clone()), w).expect("one degree"),
        HomCurve::from_polys(std::array::from_fn(|k| p[k].1.clone()), wq).expect("one degree"),
    )
}

fn bits<S: Level>(p: &Poly<S>) -> Vec<String> {
    p.coefs().iter().map(Level::bits).collect()
}

/// A factor: 0, ±1 (no rounding) or a random value.
fn factor<S: Level>(rng: &mut Rng) -> (S, Q) {
    let x = match rng.below(5) {
        0 => 0.0,
        1 => 1.0,
        2 => -1.0,
        _ => rng.pow(-4, 4),
    };
    (S::of(x, 0.0), q(x))
}

/// On rationals: the span's point is the explicit quotient, the pieces
/// and the elevation the same span, the compositions the equation at the
/// point times `W`, the shadow the projected point.
#[test]
fn rational_spans_and_compositions_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..60 {
            let n = case % 6;
            let c = int_curve(&mut rng, n);
            let a = [0, 1, 2].map(|_| int(&mut rng));
            let k = int(&mut rng);
            let f = c.compose_linear(&a, &k);
            assert_eq!(f.degree(), n);
            let ex = [0, 1, 2].map(|_| int(&mut rng));
            let ey = [0, 1, 2].map(|_| int(&mut rng));
            let s = c.shadow(&ex, &ey);
            let line = [int(&mut rng), int(&mut rng)];
            let g = s.compose_linear(&line, &k);
            let e = c.elevate(2).expect("in range");
            let r = c.reversed();
            let t0 = Q::from(rng.below(63) + 1) / Q::from(64u32);
            let [l, rt] = c.split_at(&t0);
            let [lh, rh] = c.split_half();
            for _ in 0..4 {
                let t = dyadic(&mut rng);
                let x = c.eval(&t);
                let (_, w) = c.eval_hom(&t);
                let dot = |u: &[Q; 3]| &u[0] * &x[0] + &u[1] * &x[1] + &u[2] * &x[2];
                assert_eq!(f.eval(&t), (dot(&a) + &k) * &w);
                let sx = [dot(&ex), dot(&ey)];
                assert_eq!(s.eval(&t), sx);
                assert_eq!(
                    g.eval(&t),
                    (&line[0] * &sx[0] + &line[1] * &sx[1] + &k) * &w
                );
                assert_eq!(e.eval(&t), x);
                assert_eq!(r.eval(&t), c.eval(&(Q::from(1u32) - &t)));
                assert_eq!(l.eval(&t), c.eval(&(&t0 * &t)));
                let one = Q::from(1u32);
                assert_eq!(rt.eval(&t), c.eval(&(&t0 + (&one - &t0) * &t)));
                let half = Q::from(1u32) / Q::from(2u32);
                assert_eq!(lh.eval(&t), c.eval(&(&half * &t)));
                assert_eq!(rh.eval(&t), c.eval(&(&half + &half * &t)));
            }
            // A patch in a plane equation.
            let (m, pn) = (1 + case % 3, case % 3);
            let net: Vec<[Q; 4]> = (0..(m + 1) * (pn + 1))
                .map(|_| {
                    let w = weight(&mut rng);
                    let [x, y, z] = [0, 1, 2].map(|_| int(&mut rng) * &w);
                    [x, y, z, w]
                })
                .collect();
            let patch = HomPatch::new(m, pn, net).expect("degrees in range");
            let h = patch.compose_linear(&a, &k);
            assert_eq!(h.degrees(), (m, pn));
            let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
            let x = patch.eval(&u, &v);
            let w = patch.weights().eval(&u, &v);
            let side = &a[0] * &x[0] + &a[1] * &x[1] + &a[2] * &x[2] + &k;
            assert_eq!(h.eval(&u, &v), side * w);
        }
    }
}

/// Every certified coefficient of a composition and a shadow holds the
/// exact one on the true span.
fn bounds<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    let mut check = |what: &str, p: &Poly<S>, truth: &Poly<Q>| {
        assert_eq!(p.degree(), truth.degree());
        for (b, x) in p.coefs().iter().zip(truth.coefs()) {
            stats[1] += 1;
            if holds(what, b, x) {
                stats[0] += 1;
            }
        }
    };
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..100 {
            let what = format!("seed {seed} case {case}");
            let (c, cq) = curve::<S>(&mut rng, case % 5, case % 2 == 0);
            let [(a0, a0q), (a1, a1q), (a2, a2q), (k, kq)] = [0; 4].map(|_| factor::<S>(&mut rng));
            let (a, aq) = ([a0, a1, a2], [a0q, a1q, a2q]);
            check(
                &what,
                &c.compose_linear(&a, &k),
                &cq.compose_linear(&aq, &kq),
            );
            let [(b0, b0q), (b1, b1q), (b2, b2q)] = [0; 3].map(|_| factor::<S>(&mut rng));
            let (b, bq) = ([b0, b1, b2], [b0q, b1q, b2q]);
            let s = c.shadow(&a, &b);
            let sq = cq.shadow(&aq, &bq);
            for kk in 0..2 {
                check(&what, &s.coords()[kk], &sq.coords()[kk]);
            }
            check(&what, s.weights(), sq.weights());
            let (l0, l0q) = factor::<S>(&mut rng);
            let (l1, l1q) = factor::<S>(&mut rng);
            check(
                &what,
                &s.compose_linear(&[l0, l1], &k),
                &sq.compose_linear(&[l0q, l1q], &kq),
            );
        }
    }
    assert!(
        stats[0] * 10 >= stats[1] * 9,
        "{}: certified {stats:?}",
        S::NAME
    );
}

#[test]
fn compositions_hold_their_bounds() {
    bounds::<Bounded<f64>>(varde_testing::seeds(1, 16));
    bounds::<Bounded<Dd>>(varde_testing::seeds(1, 16));
    bounds::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// The composition of the reversed span is the reversed composition, to
/// the bit, and the shadow keeps the weights' bits; a factor 1 with the
/// others 0 gives the coordinate itself; the span's split at ½ and its
/// elevation commute with reversal; the patch's composition commutes with
/// reversal and transposition.
fn mirror<S: Level>() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..60 {
            let what = format!("{} seed {seed} case {case}", S::NAME);
            let (c, _) = curve::<S>(&mut rng, 1 + case % 5, case % 2 == 0);
            let r = c.reversed();
            let a = [0; 3].map(|_| factor::<S>(&mut rng).0);
            let b = [0; 3].map(|_| factor::<S>(&mut rng).0);
            let k = factor::<S>(&mut rng).0;
            assert_eq!(
                bits(&r.compose_linear(&a, &k)),
                bits(&c.compose_linear(&a, &k).reversed()),
                "{what}"
            );
            let s = c.shadow(&a, &b);
            assert_eq!(bits(s.weights()), bits(c.weights()), "{what}");
            assert_eq!(s.reversed(), r.shadow(&a, &b), "{what}");
            let one = S::of(1.0, 0.0);
            let zero = S::of(0.0, 0.0);
            let unit = [zero.clone(), one, zero.clone()];
            assert_eq!(
                bits(&c.compose_linear(&unit, &zero)),
                bits(&c.coords()[1]),
                "{what}"
            );
            let [lh, rh] = c.split_half();
            let [rl, rr] = r.split_half();
            assert_eq!(rl, rh.reversed(), "{what}");
            assert_eq!(rr, lh.reversed(), "{what}");
            assert_eq!(
                r.elevate(2).expect("in range"),
                c.elevate(2).expect("in range").reversed(),
                "{what}"
            );
            let net: Vec<[S; 4]> = (0..9)
                .map(|_| {
                    let w = rng.pow(-6, 5).abs();
                    let [x, y, z] = [0; 3].map(|_| S::of(rng.pow(-4, 4), 0.0));
                    [x, y, z, S::of(w, 0.0)]
                })
                .collect();
            let patch = HomPatch::new(2, 2, net).expect("biquadratic");
            let h = patch.compose_linear(&a, &k);
            for dir in [Dir::U, Dir::V] {
                assert_eq!(
                    patch.reversed(dir).compose_linear(&a, &k),
                    h.reversed(dir),
                    "{what}"
                );
            }
            assert_eq!(
                patch.transposed().compose_linear(&a, &k),
                h.transposed(),
                "{what}"
            );
        }
    }
}

#[test]
fn compositions_commute_with_reversal() {
    mirror::<Bounded<f64>>();
    mirror::<Bounded<Dd>>();
    mirror::<Bounded<Big>>();
}

/// Factors all zero give the zero polynomial, at the level of the span.
#[test]
fn all_zero_factors_give_zero() {
    let mut rng = Rng::new(5);
    let (c, _) = curve::<Bounded<f64>>(&mut rng, 2, false);
    let z = Bounded::exact(0.0);
    let f = c.compose_linear(&[z, z, z], &z);
    assert!(f.coefs().iter().all(|b| b.is_int(0)));
}

#[test]
fn span_constructors_check_degrees() {
    let p = |n: usize| Poly::new((0..=n).map(|_| Q::from(1u32))).expect("degree in range");
    assert!(HomCurve::<Q, 2>::from_polys([p(2), p(2)], p(1)).is_none());
    assert!(HomCurve::<Q, 2>::from_polys([p(2), p(2)], p(2)).is_some());
    assert!(HomCurve::<Q, 3>::new(std::iter::empty()).is_none());
    let pt = || ([Q::from(1u32), Q::from(2u32)], Q::from(1u32));
    assert!(HomCurve::<Q, 2>::new((0..=MAX_DEGREE).map(|_| pt())).is_some());
    assert!(HomCurve::<Q, 2>::new((0..=MAX_DEGREE + 1).map(|_| pt())).is_none());
    let c = HomCurve::<Q, 2>::new((0..=MAX_DEGREE).map(|_| pt())).expect("at the limit");
    assert!(c.elevate(1).is_none());
    assert!(c.weights_positive());
    // At an exact end the point is the end control point's.
    let (s, _) = curve::<Bounded<f64>>(&mut Rng::new(1), 3, true);
    let (p0, w0) = s.eval_hom(&Bounded::exact(0.0));
    assert_eq!(w0, s.weights().coefs()[0]);
    assert_eq!(p0[2], s.coords()[2].coefs()[0]);
}

// ---------------------------------------------------------------------
// A patch along a curve, the seam numerator, implicits.

/// A positive integer polynomial: a denominator.
fn den_ints(rng: &mut Rng, n: usize) -> Poly<Q> {
    Poly::new((0..=n).map(|_| Q::from(rng.below(9) + 1))).expect("degree in range")
}

fn int_poly(rng: &mut Rng, n: usize) -> Poly<Q> {
    Poly::new((0..=n).map(|_| int(rng))).expect("degree in range")
}

fn int_patch(rng: &mut Rng, m: usize, n: usize) -> HomPatch<Q> {
    let net: Vec<[Q; 4]> = (0..(m + 1) * (n + 1))
        .map(|_| {
            let w = weight(rng);
            let [x, y, z] = [0, 1, 2].map(|_| int(rng) * &w);
            [x, y, z, w]
        })
        .collect();
    HomPatch::new(m, n, net).expect("degrees in range")
}

/// An implicit of degree `d` with integer terms (some zero) and origin.
fn int_implicit(rng: &mut Rng, d: usize) -> Implicit<Q> {
    let mut terms = Vec::new();
    for a in 0..=d {
        for b in 0..=d - a {
            for c in 0..=d - a - b {
                let k = if rng.below(4) == 0 {
                    Q::from(0u32)
                } else {
                    int(rng)
                };
                terms.push(([a, b, c], k));
            }
        }
    }
    // The leading terms non-zero, so the degree is `d`.
    terms[0].1 = Q::from(rng.below(9) + 1);
    let o = [0, 1, 2].map(|_| int(rng));
    Implicit::new(d, o, terms).expect("terms within the degree")
}

/// On rationals: the patch along a curve is the patch's point at the
/// curve's times `D_u^m·D_v^n`, of degree `m·k_u + n·k_v`; the span form
/// shares its weight; the seam numerator is the difference's numerator.
#[test]
fn patches_along_curves_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..40 {
            let (m, n) = (case % 4, (case / 4) % 3);
            let (ku, kv) = (rng.below(3) as usize, rng.below(3) as usize);
            let patch = int_patch(&mut rng, m, n);
            let (uu, du) = (int_poly(&mut rng, ku), den_ints(&mut rng, ku));
            let (vv, dv) = (int_poly(&mut rng, kv), den_ints(&mut rng, kv));
            let a = patch
                .compose_params([&uu, &du], [&vv, &dv])
                .expect("in range");
            assert_eq!(a.degree(), m * ku + n * kv, "seed {seed} case {case}");
            let x = patch.tensors()[1].compose_params([&uu, &du], [&vv, &dv]);
            assert_eq!(x.as_ref(), Some(&a.coords()[1]));
            let c = int_curve(&mut rng, case % 3);
            let nn = a.seam_numerator(&c).expect("in range");
            assert_eq!(nn[0].degree(), a.degree() + c.degree());
            for _ in 0..3 {
                let t = dyadic(&mut rng);
                let (u, v) = (uu.eval(&t) / du.eval(&t), vv.eval(&t) / dv.eval(&t));
                let h = patch.eval_hom(&u, &v);
                let s = pow(&du.eval(&t), m) * pow(&dv.eval(&t), n);
                let (ap, aw) = a.eval_hom(&t);
                assert_eq!(aw, &h[3] * &s);
                for k in 0..3 {
                    assert_eq!(ap[k], &h[k] * &s);
                }
                let (cp, cw) = c.eval_hom(&t);
                for k in 0..3 {
                    assert_eq!(nn[k].eval(&t), &ap[k] * &cw - &cp[k] * &aw);
                }
            }
            // A span: both parameters over one weight, moved to a corner.
            let k = ku.max(kv);
            let up = |x: &Poly<Q>| x.elevate(k - x.degree()).expect("low");
            let w = den_ints(&mut rng, k);
            let p = HomCurve::from_polys([up(&uu), up(&vv)], w).expect("one degree");
            let corner = [int(&mut rng), int(&mut rng)];
            let local = p.translated(&corner);
            let s = patch.compose_span(&local).expect("in range");
            assert_eq!(s.degree(), (m + n) * p.degree());
            let t = dyadic(&mut rng);
            let (pp, pw) = p.eval_hom(&t);
            let (u, v) = (&pp[0] / &pw - &corner[0], &pp[1] / &pw - &corner[1]);
            let h = patch.eval_hom(&u, &v);
            let (sp, sw) = s.eval_hom(&t);
            let f = pow(&pw, m + n);
            assert_eq!(sw, &h[3] * &f);
            assert_eq!(sp[2], &h[2] * &f);
        }
    }
}

fn pow(x: &Q, n: usize) -> Q {
    (0..n).fold(Q::from(1u32), |a, _| a * x)
}

fn int_tensor(rng: &mut Rng, m: usize, n: usize) -> Tensor<Q> {
    Tensor::new(m, n, (0..(m + 1) * (n + 1)).map(|_| int(rng))).expect("degrees in range")
}

fn den_tensor(rng: &mut Rng, m: usize, n: usize) -> Tensor<Q> {
    Tensor::new(
        m,
        n,
        (0..(m + 1) * (n + 1)).map(|_| Q::from(rng.below(9) + 1)),
    )
    .expect("degrees in range")
}

/// On rationals: the patch over a map of a square is the patch's point
/// at the map's times `D_u^m·D_v^n`, of degrees `m·(p_u, q_u) +
/// n·(p_v, q_v)`; a coordinate tensor's own composition is the patch's
/// coordinate.
#[test]
fn patches_over_maps_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..24 {
            let (m, n) = (case % 3, (case / 3) % 3);
            let (pu, qu) = (rng.below(3) as usize, rng.below(2) as usize);
            let (pv, qv) = (rng.below(3) as usize, rng.below(2) as usize);
            let patch = int_patch(&mut rng, m, n);
            let (uu, du) = (int_tensor(&mut rng, pu, qu), den_tensor(&mut rng, pu, 0));
            let (vv, dv) = (int_tensor(&mut rng, pv, qv), den_tensor(&mut rng, pv, 0));
            let a = patch.compose_map([&uu, &du], [&vv, &dv]).expect("in range");
            assert_eq!(
                a.degrees(),
                (m * pu + n * pv, m * qu + n * qv),
                "seed {seed} case {case}"
            );
            let y = patch.tensors()[1].compose_map([&uu, &du], [&vv, &dv]);
            assert_eq!(y.as_ref(), Some(&a.tensors()[1]));
            for _ in 0..3 {
                let (t, r) = (dyadic(&mut rng), dyadic(&mut rng));
                let (du_t, dv_t) = (du.eval(&t, &r), dv.eval(&t, &r));
                let (u, v) = (uu.eval(&t, &r) / &du_t, vv.eval(&t, &r) / &dv_t);
                let h = patch.eval_hom(&u, &v);
                let f = pow(&du_t, m) * pow(&dv_t, n);
                let x = a.eval_hom(&t, &r);
                for k in 0..4 {
                    assert_eq!(x[k], &h[k] * &f, "seed {seed} case {case}");
                }
            }
        }
    }
}

/// The seam degrees of §3.4's biquadratic table: Q 10 over 8 + 2, G 8
/// over 6 + 2, a straight UV line 6 over 4 + 2, an iso line 4 over 2 + 2.
#[test]
fn seam_degrees_are_computed_from_the_operands() {
    let mut rng = Rng::new(3);
    let patch = int_patch(&mut rng, 2, 2);
    let c = int_curve(&mut rng, 2);
    for ((ku, kv), deg) in [((2, 2), 10), ((1, 2), 8), ((1, 1), 6), ((0, 1), 4)] {
        let (uu, du) = (int_poly(&mut rng, ku), den_ints(&mut rng, ku));
        let (vv, dv) = (int_poly(&mut rng, kv), den_ints(&mut rng, kv));
        let a = patch.compose_params([&uu, &du], [&vv, &dv]).expect("low");
        assert_eq!(a.degree() + 2, deg);
        let n = a.seam_numerator(&c).expect("low");
        assert!(n.iter().all(|p| p.degree() == deg));
    }
    // Past the largest degree.
    let big = int_patch(&mut rng, 40, 1);
    let (uu, du) = (int_poly(&mut rng, 2), den_ints(&mut rng, 2));
    assert!(big.compose_params([&uu, &du], [&uu, &du]).is_none());
}

/// On rationals: a patch or span in an implicit equation is `W^d·F(x)`
/// at every point, of degrees `d` times the operand's.
#[test]
fn implicit_compositions_are_exact() {
    for seed in varde_testing::seeds(1, 8) {
        let mut rng = Rng::new(seed);
        for case in 0..16 {
            let d = 1 + case % 4;
            let f = int_implicit(&mut rng, d);
            let (m, n) = (1 + case % 2, 1 + (case / 2) % 2);
            let patch = int_patch(&mut rng, m, n);
            let g = patch.compose_implicit(&f).expect("in range");
            assert_eq!(g.degrees(), (d * m, d * n));
            let c = int_curve(&mut rng, 1 + case % 3);
            let gc = c.compose_implicit(&f).expect("in range");
            assert_eq!(gc.degree(), d * c.degree());
            for _ in 0..3 {
                let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
                let h = patch.eval_hom(&u, &v);
                let x = [h[0].clone(), h[1].clone(), h[2].clone()];
                assert_eq!(g.eval(&u, &v), f.eval_hom(&x, &h[3]));
                let (p, w) = c.eval_hom(&u);
                assert_eq!(gc.eval(&u), f.eval_hom(&p, &w));
                // `W^d·F(x)` from the affine point.
                let pt = [0, 1, 2].map(|k| &p[k] / &w);
                let one = Q::from(1u32);
                assert_eq!(f.eval_hom(&pt, &one) * pow(&w, d), f.eval_hom(&p, &w));
            }
        }
    }
}

/// `(R + ρ·cos φ)·(cos θ, sin θ) , ρ·sin φ` on a quarter of each circle,
/// exact in rationals: `((1 − s²), 2s) / (1 + s²)`, the Bernstein net
/// `(1, 0, 1), (1, 1, 1), (0, 2, 2)`; and the torus's quartic
/// `(|r|² + R² − ρ²)² − 4R²·(x² + y²)` about the centre `o`.
fn torus(o: [Q; 3], big_r: u32, rho: u32) -> (HomPatch<Q>, Implicit<Q>) {
    let circle = [[1, 0, 1], [1, 1, 1], [0, 2, 2]].map(|p| p.map(Q::from));
    let (r, p) = (Q::from(big_r), Q::from(rho));
    let mut net = Vec::new();
    for tube in &circle {
        for turn in &circle {
            // Tube circle (cos, sin, w) at φ, the turn circle at θ.
            let radial = &r * &tube[2] + &p * &tube[0];
            let w = &tube[2] * &turn[2];
            net.push([
                &radial * &turn[0] + &o[0] * &w,
                &radial * &turn[1] + &o[1] * &w,
                &p * &tube[1] * &turn[2] + &o[2] * &w,
                w,
            ]);
        }
    }
    let patch = HomPatch::new(2, 2, net).expect("biquadratic");
    let k = &r * &r - &p * &p;
    let i = |x: i64| Q::from(x);
    // (x² + y² + z² + k)² − 4R²(x² + y²), expanded.
    let terms = [
        ([4, 0, 0], i(1)),
        ([0, 4, 0], i(1)),
        ([0, 0, 4], i(1)),
        ([2, 2, 0], i(2)),
        ([2, 0, 2], i(2)),
        ([0, 2, 2], i(2)),
        ([2, 0, 0], i(2) * &k - i(4) * &r * &r),
        ([0, 2, 0], i(2) * &k - i(4) * &r * &r),
        ([0, 0, 2], i(2) * &k),
        ([0, 0, 0], &k * &k),
    ];
    (patch, Implicit::new(4, o, terms).expect("a quartic"))
}

/// A torus tile in its quartic: degrees (8, 8), every coefficient exactly
/// zero; off the torus not zero, of the sign of the point's side.
#[test]
fn a_torus_tile_in_its_quartic_is_exactly_zero() {
    let o = [Q::from(3u32), Q::from(-5i32), Q::from(7u32)];
    let (patch, f) = torus(o.clone(), 5, 2);
    let g = patch.compose_implicit(&f).expect("degrees (8, 8)");
    assert_eq!(g.degrees(), (8, 8));
    assert!(g.coefs().iter().all(|c| *c == 0u32));
    // The same tile on a larger tube lies inside it: `F < 0` everywhere.
    let (_, fat) = torus(o, 5, 3);
    let g = patch.compose_implicit(&fat).expect("degrees (8, 8)");
    assert!(g.coefs().iter().all(|c| *c < 0u32));
}

/// An implicit of degree 1 at the origin 0 is the linear composition, to
/// the bit; factors ±1 round nothing.
fn plane_bits<S: Level>() {
    let mut rng = Rng::new(11);
    for case in 0..30 {
        let what = format!("{} case {case}", S::NAME);
        let (c, _) = curve::<S>(&mut rng, 1 + case % 4, case % 2 == 0);
        let a = [0; 3].map(|_| factor::<S>(&mut rng).0);
        let k = factor::<S>(&mut rng).0;
        let z = S::of(0.0, 0.0);
        let terms = [
            ([1, 0, 0], a[0].clone()),
            ([0, 1, 0], a[1].clone()),
            ([0, 0, 1], a[2].clone()),
            ([0, 0, 0], k.clone()),
        ];
        let f = Implicit::new(1, [z.clone(), z.clone(), z.clone()], terms).expect("a plane");
        assert_eq!(
            bits(&c.compose_implicit(&f).expect("degree 1")),
            bits(&c.compose_linear(&a, &k)),
            "{what}"
        );
        let unit = Implicit::new(
            2,
            [z.clone(), z.clone(), z],
            [([0, 2, 0], S::of(-1.0, 0.0))],
        )
        .expect("one term");
        let sq = c.coords()[1].mul(&c.coords()[1]).expect("low").neg();
        assert_eq!(
            bits(&c.compose_implicit(&unit).expect("degree 2")),
            bits(&sq),
            "{what}"
        );
    }
}

#[test]
fn plane_implicits_are_the_linear_compositions() {
    plane_bits::<Bounded<f64>>();
    plane_bits::<Bounded<Dd>>();
    plane_bits::<Bounded<Big>>();
}

/// A level's value `±m·2ᵉ` of moderate size, with error parts sometimes,
/// and its true value.
fn moderate<S: Level>(rng: &mut Rng, errs: bool) -> (S, Q) {
    let x = match rng.below(6) {
        0 => 0.0,
        1 => 1.0,
        _ => rng.pow(-4, 4),
    };
    let b = S::of(x, 0.0);
    if errs && rng.below(2) == 0 {
        let (t, r) = rng.errs(x);
        let lam = rng.lambda();
        (b.with_errs(t, r), q(x) + lam * (q(t) + q(r)))
    } else {
        (b, q(x))
    }
}

fn moderate_poly<S: Level>(
    rng: &mut Rng,
    n: usize,
    errs: bool,
    positive: bool,
) -> (Poly<S>, Poly<Q>) {
    let (c, t): (Vec<S>, Vec<Q>) = (0..=n)
        .map(|_| {
            if positive {
                let w = rng.pow(-3, 3).abs();
                (S::of(w, 0.0), q(w))
            } else {
                moderate::<S>(rng, errs)
            }
        })
        .unzip();
    (
        Poly::new(c).expect("in range"),
        Poly::new(t).expect("in range"),
    )
}

fn moderate_patch<S: Level>(
    rng: &mut Rng,
    m: usize,
    n: usize,
    errs: bool,
) -> (HomPatch<S>, HomPatch<Q>) {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for _ in 0..(m + 1) * (n + 1) {
        let (x, xq) = moderate::<S>(rng, errs);
        let (y, yq) = moderate::<S>(rng, errs);
        let (z, zq) = moderate::<S>(rng, errs);
        let w = rng.pow(-3, 3).abs();
        a.push([x, y, z, S::of(w, 0.0)]);
        b.push([xq, yq, zq, q(w)]);
    }
    (
        HomPatch::new(m, n, a).expect("in range"),
        HomPatch::new(m, n, b).expect("in range"),
    )
}

fn moderate_implicit<S: Level>(rng: &mut Rng, d: usize, errs: bool) -> (Implicit<S>, Implicit<Q>) {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for x in 0..=d {
        for y in 0..=d - x {
            for z in 0..=d - x - y {
                let (c, cq) = moderate::<S>(rng, errs);
                a.push(([x, y, z], c));
                b.push(([x, y, z], cq));
            }
        }
    }
    let o: [(S, Q); 3] = [0; 3].map(|_| moderate::<S>(rng, errs));
    let (oa, ob) = (o.clone().map(|x| x.0), o.map(|x| x.1));
    (
        Implicit::new(d, oa, a).expect("in range"),
        Implicit::new(d, ob, b).expect("in range"),
    )
}

/// Every certified coefficient of a patch along a curve, of a seam
/// numerator and of an implicit composition (degrees 1 to 4) holds the
/// exact one.
fn along_bounds<S: Level>(seeds: std::ops::Range<u64>, cases: usize) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..cases {
            let what = format!("seed {seed} case {case}");
            let errs = case % 2 == 0;
            let (m, n) = (1 + case % 2, 1 + (case / 2) % 2);
            let (patch, pq) = moderate_patch::<S>(&mut rng, m, n, errs);
            let (ku, kv) = (rng.below(3) as usize, 1 + rng.below(2) as usize);
            let (uu, uq) = moderate_poly::<S>(&mut rng, ku, errs, false);
            let (du, duq) = moderate_poly::<S>(&mut rng, ku, errs, true);
            let (vv, vq) = moderate_poly::<S>(&mut rng, kv, errs, false);
            let (dv, dvq) = moderate_poly::<S>(&mut rng, kv, errs, true);
            let a = patch.compose_params([&uu, &du], [&vv, &dv]).expect("low");
            let aq = pq.compose_params([&uq, &duq], [&vq, &dvq]).expect("low");
            for k in 0..3 {
                poly_holds(&what, &a.coords()[k], &aq.coords()[k], &mut stats);
            }
            poly_holds(&what, a.weights(), aq.weights(), &mut stats);
            let (c, cq) = curve::<S>(&mut rng, case % 3, errs);
            let nn = a.seam_numerator(&c).expect("low");
            let nq = aq.seam_numerator(&cq).expect("low");
            for k in 0..3 {
                poly_holds(&what, &nn[k], &nq[k], &mut stats);
            }
            let d = 1 + case % 4;
            let (f, fq) = moderate_implicit::<S>(&mut rng, d, errs);
            let g = patch.compose_implicit(&f).expect("low");
            tensor_holds(
                &what,
                &g,
                &pq.compose_implicit(&fq).expect("low"),
                &mut stats,
            );
            poly_holds(
                &what,
                &a.compose_implicit(&f).expect("low"),
                &aq.compose_implicit(&fq).expect("low"),
                &mut stats,
            );
        }
    }
    assert!(
        stats[0] * 10 >= stats[1] * 9,
        "{}: certified {stats:?}",
        S::NAME
    );
}

#[test]
fn compositions_along_curves_hold_their_bounds() {
    along_bounds::<Bounded<f64>>(varde_testing::seeds(1, 8), 24);
    along_bounds::<Bounded<Dd>>(varde_testing::seeds(1, 8), 24);
    along_bounds::<Bounded<Big>>(varde_testing::seeds(1, 4), 12);
}

/// The bits of a form (`NaN`s alike).
fn dbg<T: std::fmt::Debug>(x: &T) -> String {
    format!("{x:?}")
}

/// Along the reversed curve the composition is the reversed one; the
/// transposed patch along the curve with its parameters swapped is the
/// same; an implicit composition commutes with reversal and
/// transposition; a translation keeps the weights' bits. All to the bit.
fn along_mirror<S: Level>() {
    let mut rng = Rng::new(17);
    for case in 0..24 {
        let what = format!("{} case {case}", S::NAME);
        let errs = case % 2 == 0;
        let (m, n) = (1 + case % 3, 1 + (case / 3) % 2);
        let (patch, _) = moderate_patch::<S>(&mut rng, m, n, errs);
        let k = 1 + case % 2;
        let (uu, _) = moderate_poly::<S>(&mut rng, k, errs, false);
        let (vv, _) = moderate_poly::<S>(&mut rng, k, errs, false);
        let (w, _) = moderate_poly::<S>(&mut rng, k, errs, true);
        let p = HomCurve::from_polys([uu, vv], w).expect("one degree");
        let corner = [
            moderate::<S>(&mut rng, false).0,
            moderate::<S>(&mut rng, false).0,
        ];
        let local = p.translated(&corner);
        assert_eq!(bits(local.weights()), bits(p.weights()), "{what}");
        assert_eq!(
            dbg(&local.reversed()),
            dbg(&p.reversed().translated(&corner)),
            "{what}"
        );
        let a = patch.compose_span(&local).expect("low");
        let r = local.reversed();
        assert_eq!(
            dbg(&patch.compose_span(&r).expect("low")),
            dbg(&a.reversed()),
            "{what}"
        );
        let [pu, pv] = local.coords();
        let swapped = HomCurve::from_polys([pv.clone(), pu.clone()], local.weights().clone())
            .expect("one degree");
        assert_eq!(
            dbg(&patch.transposed().compose_span(&swapped).expect("low")),
            dbg(&a),
            "{what}"
        );
        let (c, _) = curve::<S>(&mut rng, k, errs);
        assert_eq!(
            dbg(&a.reversed().seam_numerator(&c.reversed()).expect("low")),
            dbg(&a.seam_numerator(&c).expect("low").map(|x| x.reversed())),
            "{what}"
        );
        let (f, _) = moderate_implicit::<S>(&mut rng, 1 + case % 4, errs);
        let g = patch.compose_implicit(&f).expect("low");
        for dir in [Dir::U, Dir::V] {
            assert_eq!(
                dbg(&patch.reversed(dir).compose_implicit(&f).expect("low")),
                dbg(&g.reversed(dir)),
                "{what}"
            );
        }
        assert_eq!(
            dbg(&patch.transposed().compose_implicit(&f).expect("low")),
            dbg(&g.transposed()),
            "{what}"
        );
        assert_eq!(
            dbg(&c.reversed().compose_implicit(&f).expect("low")),
            dbg(&c.compose_implicit(&f).expect("low").reversed()),
            "{what}"
        );
    }
}

#[test]
fn compositions_along_curves_commute_with_reversal_and_swap() {
    along_mirror::<Bounded<f64>>();
    along_mirror::<Bounded<Dd>>();
    along_mirror::<Bounded<Big>>();
}

#[test]
fn implicits_check_their_terms() {
    let z = || [Q::from(0u32), Q::from(0u32), Q::from(0u32)];
    let one = || Q::from(1u32);
    assert!(Implicit::new(2, z(), [([1, 1, 1], one())]).is_none());
    assert!(Implicit::new(2, z(), [([1, 0, 0], one()), ([1, 0, 0], one())]).is_none());
    assert!(Implicit::<Q>::new(2, z(), []).is_none());
    assert!(Implicit::new(MAX_DEGREE + 1, z(), [([0, 0, 0], one())]).is_none());
    let f = Implicit::new(
        2,
        z(),
        [
            ([0, 0, 0], one()),
            ([0, 1, 0], Q::from(0u32)),
            ([2, 0, 0], one()),
        ],
    )
    .expect("terms within the degree");
    // Zero terms dropped, the rest in descending order of exponents.
    assert_eq!(f.terms().len(), 2);
    assert_eq!(f.terms()[0].0, [2, 0, 0, 0]);
    assert_eq!(f.terms()[1].0, [0, 0, 0, 2]);
    // Every term zero: the zero form at the composition's degrees.
    let zero = Implicit::new(3, z(), [([1, 0, 0], Q::from(0u32))]).expect("one term");
    let g = int_patch(&mut Rng::new(2), 2, 1)
        .compose_implicit(&zero)
        .expect("low");
    assert_eq!(g.degrees(), (6, 3));
    assert!(g.coefs().iter().all(|c| *c == 0u32));
    // Past the largest degree.
    let big = int_patch(&mut Rng::new(2), 20, 1);
    assert!(
        big.compose_implicit(&int_implicit(&mut Rng::new(4), 4))
            .is_none()
    );
}
