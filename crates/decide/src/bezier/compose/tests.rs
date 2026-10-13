//! Rational spans and the compositions with linear forms: on rationals
//! exact against the point's values; at every level each certified
//! coefficient holds the exact one; reversal gives the same bits, the
//! images keep the weights' bits, factors 0 and ±1 round nothing.

use malachite::Rational as Q;

use super::*;
use crate::bezier::MAX_DEGREE;
use crate::bezier::tensor::Dir;
use crate::bezier::tests::{Level, Rng, holds, q};
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
