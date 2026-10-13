//! Tensor forms and rational patches: on rationals every operation is
//! exact against the explicit Bernstein sums; at every level each
//! certified result holds the exact one, and the hull box holds the exact
//! patch at sampled parameters, also on pieces of pieces; the split at ½
//! gives the same bits under reversal in either direction and under
//! transposition.

use malachite::Rational as Q;

use super::*;
use crate::bezier::tests::{Level, Rng, approx, holds, q, qabs};
use crate::num::bigf::Big;
use crate::num::dd::Dd;

/// `C(n, k)`, exactly.
fn binom(n: usize, k: usize) -> Q {
    let mut r = Q::from(1u32);
    for i in 0..k {
        r = r * Q::from(n - i) / Q::from(i + 1);
    }
    r
}

/// `Bₖⁿ(t)`, zero for `k` outside `0..=n`.
fn basis(n: usize, k: usize, t: &Q) -> Q {
    if k > n {
        return Q::from(0u32);
    }
    let one = Q::from(1u32);
    let mut r = binom(n, k);
    for _ in 0..k {
        r *= t;
    }
    for _ in k..n {
        r *= &one - t;
    }
    r
}

/// `d/dt Bₖⁿ(t) = n·(Bₖ₋₁ⁿ⁻¹ − Bₖⁿ⁻¹)`.
fn basis_d(n: usize, k: usize, t: &Q) -> Q {
    if n == 0 {
        return Q::from(0u32);
    }
    let prev = if k == 0 {
        Q::from(0u32)
    } else {
        basis(n - 1, k - 1, t)
    };
    Q::from(n) * (prev - basis(n - 1, k, t))
}

/// The explicit sum `Σ bᵢⱼ·Bᵢ(u)·Bⱼ(v)`, its basis in `u` and `v` given
/// (to take derivatives).
fn sum(
    f: &Tensor<Q>,
    u: &Q,
    v: &Q,
    bu: fn(usize, usize, &Q) -> Q,
    bv: fn(usize, usize, &Q) -> Q,
) -> Q {
    let (m, n) = f.degrees();
    let mut r = Q::from(0u32);
    for i in 0..=m {
        for j in 0..=n {
            r += f.get(i, j) * bu(m, i, u) * bv(n, j, v);
        }
    }
    r
}

fn value(f: &Tensor<Q>, u: &Q, v: &Q) -> Q {
    sum(f, u, v, basis, basis)
}

/// A dyadic in `[0, 1]`.
fn dyadic(rng: &mut Rng) -> Q {
    Q::from(rng.below(65)) / Q::from(64u32)
}

/// A random rational form of degrees `(m, n)`, small integers.
fn int_tensor(rng: &mut Rng, m: usize, n: usize) -> Tensor<Q> {
    let c = (0..(m + 1) * (n + 1)).map(|_| Q::from(rng.below(201) as i64 - 100));
    Tensor::new(m, n, c).expect("degrees in range")
}

/// On rationals: evaluation is the explicit sum, the partials its
/// derivatives, the pieces of either split the form on their intervals,
/// transposition and reversal what they say, the hull an enclosure.
#[test]
fn rational_tensor_operations_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..60 {
            let (m, n) = (case % 4, (case / 4) % 4);
            let f = int_tensor(&mut rng, m, n);
            let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
            let t = Q::from(rng.below(63) + 1) / Q::from(64u32);
            let one = Q::from(1u32);
            let half = &one / Q::from(2u32);
            let fv = f.eval(&u, &v);
            assert_eq!(fv, value(&f, &u, &v));
            let h = f.hull();
            assert!(q(h.lo) <= fv && fv <= q(h.hi));
            assert_eq!(f.transposed().eval(&v, &u), fv);
            assert_eq!(f.transposed().transposed(), f);
            assert_eq!(f.reversed(Dir::U).eval(&(&one - &u), &v), fv);
            assert_eq!(f.reversed(Dir::V).eval(&u, &(&one - &v)), fv);
            let du = f.derivative(Dir::U);
            let dv = f.derivative(Dir::V);
            assert_eq!(du.degrees(), (m.saturating_sub(1), n));
            assert_eq!(dv.degrees(), (m, n.saturating_sub(1)));
            assert_eq!(du.eval(&u, &v), sum(&f, &u, &v, basis_d, basis));
            assert_eq!(dv.eval(&u, &v), sum(&f, &u, &v, basis, basis_d));
            let [l, r] = f.split_half(Dir::U);
            assert_eq!(l.eval(&u, &v), f.eval(&(&u * &half), &v));
            assert_eq!(r.eval(&u, &v), f.eval(&(&half + &u * &half), &v));
            let [l, r] = f.split_half(Dir::V);
            assert_eq!(l.eval(&u, &v), f.eval(&u, &(&v * &half)));
            assert_eq!(r.eval(&u, &v), f.eval(&u, &(&half + &v * &half)));
            let [l, r] = f.split_at(Dir::U, &t);
            assert_eq!(l.eval(&u, &v), f.eval(&(&t * &u), &v));
            assert_eq!(r.eval(&u, &v), f.eval(&(&t + (&one - &t) * &u), &v));
            let [l, r] = f.split_at(Dir::V, &t);
            assert_eq!(l.eval(&u, &v), f.eval(&u, &(&t * &v)));
            assert_eq!(r.eval(&u, &v), f.eval(&u, &(&t + (&one - &t) * &v)));
        }
    }
}

/// A random rational patch with positive integer weights.
fn int_patch(rng: &mut Rng, m: usize, n: usize) -> HomPatch<Q> {
    let net: Vec<[Q; 4]> = (0..(m + 1) * (n + 1))
        .map(|_| {
            let mut c = [0; 4].map(|_| Q::from(rng.below(201) as i64 - 100));
            c[3] = Q::from(rng.below(64) + 1);
            c
        })
        .collect();
    HomPatch::new(m, n, net).expect("degrees in range")
}

/// On rationals: the point is the quotient of the explicit sums, the
/// partials the quotient rule on their derivatives, the pieces the patch
/// on their intervals; the hull box holds the point.
#[test]
fn rational_patch_operations_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..40 {
            let (m, n) = (1 + case % 3, 1 + (case / 3) % 3);
            let p = int_patch(&mut rng, m, n);
            let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
            let [x, y, z, w] = p.tensors();
            let hv = [x, y, z].map(|c| value(c, &u, &v));
            let wv = value(w, &u, &v);
            let pt = p.eval(&u, &v);
            assert_eq!(pt, hv.clone().map(|c| c / &wv));
            let d = p.eval_partials(&u, &v);
            assert_eq!(d.p, pt);
            let wu = sum(w, &u, &v, basis_d, basis);
            let wd = sum(w, &u, &v, basis, basis_d);
            for (k, c) in [x, y, z].into_iter().enumerate() {
                let (cu, cv) = (
                    sum(c, &u, &v, basis_d, basis),
                    sum(c, &u, &v, basis, basis_d),
                );
                assert_eq!(d.du[k], (&cu * &wv - &hv[k] * &wu) / (&wv * &wv));
                assert_eq!(d.dv[k], (&cv * &wv - &hv[k] * &wd) / (&wv * &wv));
            }
            let half = Q::from(1u32) / Q::from(2u32);
            let [l, r] = p.split_half(Dir::V);
            assert_eq!(l.eval(&u, &v), p.eval(&u, &(&v * &half)));
            assert_eq!(r.eval(&u, &v), p.eval(&u, &(&half + &v * &half)));
            assert_eq!(p.transposed().eval(&v, &u), pt);
            let b = p.hull_box().expect("positive weights");
            for k in 0..3 {
                assert!(q(b.lo[k]) <= pt[k] && pt[k] <= q(b.hi[k]), "{b:?}");
            }
        }
    }
}

/// A level's form of degrees `(m, n)` and its true coefficients (within
/// the error parts, when given); `weight` makes every coefficient a
/// weight in `[1/64, 64]`.
fn tensor<S: Level>(
    rng: &mut Rng,
    m: usize,
    n: usize,
    with_errs: bool,
    weight: bool,
) -> (Tensor<S>, Tensor<Q>) {
    let scale = rng.scale();
    let mut c = Vec::new();
    let mut truth = Vec::new();
    for _ in 0..(m + 1) * (n + 1) {
        let (hi, lo) = if weight {
            let hi = rng.pow(-6, 5).abs();
            (hi, hi * rng.pow(-80, -54).abs())
        } else {
            rng.coef(scale)
        };
        let mut b = S::of(hi, lo);
        let mut x = b.val();
        if with_errs && rng.below(2) == 0 {
            let (t, r) = rng.errs(hi);
            b = b.with_errs(t, r);
            x += rng.lambda() * (q(t) + q(r));
        }
        c.push(b);
        truth.push(x);
    }
    (
        Tensor::new(m, n, c).expect("degrees in range"),
        Tensor::new(m, n, truth).expect("degrees in range"),
    )
}

/// A level's patch with weights in `[1/64, 64]` and its true patch.
fn patch<S: Level>(
    rng: &mut Rng,
    m: usize,
    n: usize,
    with_errs: bool,
) -> (HomPatch<S>, HomPatch<Q>) {
    let [(x, xq), (y, yq), (z, zq), (w, wq)] =
        [false, false, false, true].map(|weight| tensor::<S>(rng, m, n, with_errs, weight));
    (
        HomPatch::from_tensors([x, y, z, w]).expect("same degrees"),
        HomPatch::from_tensors([xq, yq, zq, wq]).expect("same degrees"),
    )
}

/// Every coefficient of a level's form holds the true form's.
fn coefs_hold<S: Level>(what: &str, f: &Tensor<S>, truth: &Tensor<Q>) {
    assert_eq!(f.degrees(), truth.degrees(), "{what}");
    for (b, x) in f.coefs().iter().zip(truth.coefs()) {
        holds(what, b, x);
    }
}

/// A parameter of the level and its exact value: exact (`f64`) or with
/// an error of its own.
fn param<S: Level>(rng: &mut Rng) -> (S, Q) {
    let t = rng.t();
    if rng.below(4) == 0 {
        let e = rng.pow(-60, -40).abs();
        (S::of(t, 0.0).with_errs(0.0, e), q(t) + rng.lambda() * q(e))
    } else {
        (S::of(t, 0.0), q(t))
    }
}

/// The hull box holds the exact point, given as rationals.
fn box_holds(what: &str, b: &HullBox, pt: &[Q; 3]) {
    for k in 0..3 {
        assert!(
            q(b.lo[k]) <= pt[k] && pt[k] <= q(b.hi[k]),
            "{what}: {b:?} misses {k}: {}",
            approx(&pt[k])
        );
    }
}

/// At a level, through chains of splits in random directions: every
/// coefficient, the point, its partials and the derivative forms hold the
/// exact ones, and the hull box holds the exact patch at sampled
/// parameters (the box always certified: weights in `[1/64, 64]` with
/// small errors).
fn patch_chains<S: Level>(seeds: std::ops::Range<u64>) {
    let mut certified = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..60 {
            let (m, n) = (1 + case % 3, 1 + (case / 3) % 3);
            let (mut p, mut truth) = patch::<S>(&mut rng, m, n, case % 2 == 0);
            for step in 0..4 {
                let what = format!("{} seed {seed} case {case} step {step}", S::NAME);
                // The corners are control points, where the box is
                // tight; dyadics inside.
                let b = p.hull_box().expect("weights certified positive");
                let corners = [0u32, 1].map(|u| [0u32, 1].map(|v| (Q::from(u), Q::from(v))));
                let inner = [0; 4].map(|_| (dyadic(&mut rng), dyadic(&mut rng)));
                for (u, v) in corners.into_iter().flatten().chain(inner) {
                    box_holds(&what, &b, &truth.eval(&u, &v));
                }
                let ((u, uq), (v, vq)) = (param::<S>(&mut rng), param::<S>(&mut rng));
                let d = p.eval_partials(&u, &v);
                let e = truth.eval_partials(&uq, &vq);
                for k in 0..3 {
                    for (r, x) in [
                        (&d.p[k], &e.p[k]),
                        (&d.du[k], &e.du[k]),
                        (&d.dv[k], &e.dv[k]),
                    ] {
                        certified[1] += 1;
                        if holds(&what, r, x) {
                            certified[0] += 1;
                        }
                    }
                }
                for dir in [Dir::U, Dir::V] {
                    let (dp, dq) = (p.derivative(dir), truth.derivative(dir));
                    for k in 0..4 {
                        coefs_hold(&what, &dp.tensors()[k], &dq.tensors()[k]);
                    }
                }
                let dir = if rng.below(2) == 0 { Dir::U } else { Dir::V };
                let side = usize::from(rng.below(2) == 0);
                (p, truth) = if rng.below(2) == 0 {
                    (
                        p.split_half(dir)[side].clone(),
                        truth.split_half(dir)[side].clone(),
                    )
                } else {
                    let t = rng.t();
                    (
                        p.split_at(dir, &S::of(t, 0.0))[side].clone(),
                        truth.split_at(dir, &q(t))[side].clone(),
                    )
                };
                for k in 0..4 {
                    coefs_hold(&what, &p.tensors()[k], &truth.tensors()[k]);
                }
            }
        }
    }
    // Not vacuous: the patch's values certify nearly always (the misses
    // overflow at the top of the range).
    assert!(
        certified[0] * 10 >= certified[1] * 9,
        "{}: certified {certified:?}",
        S::NAME
    );
}

#[test]
fn patch_bounds_hold_f64() {
    patch_chains::<Bounded<f64>>(varde_testing::seeds(1, 16));
}

#[test]
fn patch_bounds_hold_dd() {
    patch_chains::<Bounded<Dd>>(varde_testing::seeds(1, 16));
}

#[test]
fn patch_bounds_hold_big() {
    patch_chains::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// A scalar form's evaluation and hull hold the exact form at a level.
fn tensor_bounds<S: Level>() {
    for seed in varde_testing::seeds(1, 8) {
        let mut rng = Rng::new(seed);
        for case in 0..100 {
            let (m, n) = (case % 4, (case / 4) % 4);
            let (f, truth) = tensor::<S>(&mut rng, m, n, true, false);
            let ((u, uq), (v, vq)) = (param::<S>(&mut rng), param::<S>(&mut rng));
            holds(S::NAME, &f.eval(&u, &v), &truth.eval(&uq, &vq));
            let h = f.hull();
            for x in truth.coefs() {
                assert!(h.lo == f64::NEG_INFINITY || q(h.lo) <= *x, "{h:?}");
                assert!(h.hi == f64::INFINITY || *x <= q(h.hi), "{h:?}");
            }
        }
    }
}

#[test]
fn tensor_bounds_hold() {
    tensor_bounds::<Bounded<f64>>();
    tensor_bounds::<Bounded<Dd>>();
    tensor_bounds::<Bounded<Big>>();
}

/// Every bit of a patch.
fn bits<S: Level>(p: &HomPatch<S>) -> Vec<String> {
    p.tensors()
        .iter()
        .flat_map(|t| t.coefs().iter().map(Level::bits))
        .collect()
}

/// The split at ½ of the patch reversed in `dir` is the halves swapped
/// and reversed in `dir`, to the bit; the split in `v` of the transposed
/// patch is the split in `u` transposed, to the bit; through three
/// levels of splits in random directions.
fn mirror<S: Level>() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..40 {
            let (m, n) = (1 + case % 3, 1 + (case / 3) % 3);
            let (p, _) = patch::<S>(&mut rng, m, n, case % 2 == 0);
            let mut pieces = vec![p];
            for level in 0..3 {
                let what = format!("{} seed {seed} case {case} level {level}", S::NAME);
                let dir = if rng.below(2) == 0 { Dir::U } else { Dir::V };
                let other = if dir == Dir::U { Dir::V } else { Dir::U };
                let mut next = Vec::new();
                for x in &pieces {
                    let [l, r] = x.split_half(dir);
                    let [rl, rr] = x.reversed(dir).split_half(dir);
                    assert_eq!(bits(&l), bits(&rr.reversed(dir)), "{what}");
                    assert_eq!(bits(&r), bits(&rl.reversed(dir)), "{what}");
                    let [tl, tr] = x.transposed().split_half(other);
                    assert_eq!(bits(&l), bits(&tl.transposed()), "{what}");
                    assert_eq!(bits(&r), bits(&tr.transposed()), "{what}");
                    // The halves share their middle line, to the bit.
                    let deg = l.degrees();
                    for t in 0..4 {
                        let (a, b) = (&l.tensors()[t], &r.tensors()[t]);
                        match dir {
                            Dir::U => (0..=deg.1).for_each(|j| {
                                assert_eq!(a.get(deg.0, j).bits(), b.get(0, j).bits(), "{what}");
                            }),
                            Dir::V => (0..=deg.0).for_each(|i| {
                                assert_eq!(a.get(i, deg.1).bits(), b.get(i, 0).bits(), "{what}");
                            }),
                        }
                    }
                    next.extend([l, r]);
                }
                pieces = next;
            }
        }
    }
}

#[test]
fn split_half_is_bit_symmetric_in_both_directions() {
    mirror::<Bounded<f64>>();
    mirror::<Bounded<Dd>>();
    mirror::<Bounded<Big>>();
}

/// Two tiles sharing a line (the halves of a split, errors in the net)
/// evaluate on it to the same bits, error parts included, from either
/// side and transposed, as the line itself does; a corner is its
/// coefficient.
fn shared_lines<S: Level>() {
    for seed in varde_testing::seeds(1, 8) {
        let mut rng = Rng::new(seed);
        for case in 0..40 {
            let (m, n) = (1 + case % 3, 1 + (case / 3) % 3);
            let (f, _) = tensor::<S>(&mut rng, m, n, true, false);
            let what = format!("{} seed {seed} case {case}", S::NAME);
            let (zero, one) = (S::of(0.0, 0.0), S::of(1.0, 0.0));
            let (t, _) = param::<S>(&mut rng);
            for dir in [Dir::U, Dir::V] {
                let [l, r] = f.split_half(dir);
                let (a, b, line) = match dir {
                    Dir::U => (
                        l.eval(&one, &t),
                        r.eval(&zero, &t),
                        (0..=n).map(|j| r.get(0, j).clone()).collect::<Vec<_>>(),
                    ),
                    Dir::V => (
                        l.eval(&t, &one),
                        r.eval(&t, &zero),
                        (0..=m).map(|i| r.get(i, 0).clone()).collect(),
                    ),
                };
                let own = Poly::new(line).expect("degree in range").eval(&t);
                assert_eq!(a.bits(), b.bits(), "{what}");
                assert_eq!(a.bits(), own.bits(), "{what}");
                let tr = match dir {
                    Dir::U => r.transposed().eval(&t, &zero),
                    Dir::V => r.transposed().eval(&zero, &t),
                };
                assert_eq!(a.bits(), tr.bits(), "{what}");
            }
            for (i, u) in [(0, &zero), (m, &one)] {
                for (j, v) in [(0, &zero), (n, &one)] {
                    assert_eq!(f.eval(u, v).bits(), f.get(i, j).bits(), "{what}");
                }
            }
        }
    }
}

#[test]
fn shared_lines_evaluate_to_the_same_bits() {
    shared_lines::<Bounded<f64>>();
    shared_lines::<Bounded<Dd>>();
    shared_lines::<Bounded<Big>>();
}

/// A parameter of `−0` is the end 0 at every level: the end coefficient
/// or the boundary line alone, the same bits as at `+0`. A value carried
/// up from the first level keeps its sign, and there `−0 == 0`.
fn negative_zero_is_an_end<S: Level>() {
    let mut rng = Rng::new(7);
    let (f, _) = tensor::<S>(&mut rng, 2, 1, true, false);
    let (zero, t) = (S::of(0.0, 0.0), S::of(0.375, 0.0));
    let neg = zero.neg();
    assert!(neg.is_int(0), "{}: {neg:?}", S::NAME);
    let what = S::NAME;
    assert_eq!(f.eval(&neg, &t).bits(), f.eval(&zero, &t).bits(), "{what}");
    assert_eq!(f.eval(&t, &neg).bits(), f.eval(&t, &zero).bits(), "{what}");
    assert_eq!(f.eval(&neg, &neg).bits(), f.get(0, 0).bits(), "{what}");
    let line = Poly::new((0..=2).map(|i| f.get(i, 0).clone())).expect("quadratic");
    assert_eq!(line.eval(&neg).bits(), f.get(0, 0).bits(), "{what}");
}

#[test]
fn negative_zero_is_an_end_at_every_level() {
    negative_zero_is_an_end::<Bounded<f64>>();
    negative_zero_is_an_end::<Bounded<Dd>>();
    negative_zero_is_an_end::<Bounded<Big>>();
}

/// Where `W` may be zero the point and its partials are uncertified.
#[test]
fn partials_need_a_nonzero_weight() {
    let p = HomPatch::from_f64(
        1,
        1,
        &[
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, -1.0],
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, -1.0],
        ],
    )
    .expect("bilinear");
    let half = Bounded::exact(0.5);
    let d = p.eval_partials(&half, &half);
    for x in d.p.iter().chain(&d.du).chain(&d.dv) {
        assert!(!x.is_certified(), "{x:?}");
    }
    let ok = p.eval_partials(&half, &Bounded::exact(0.25));
    assert!(
        ok.p.iter()
            .chain(&ok.du)
            .chain(&ok.dv)
            .all(Bounded::is_certified)
    );
}

/// No box unless every weight is certified positive: a zero, a negative
/// or an uncertified weight, or one whose error reaches zero.
#[test]
fn hull_box_needs_positive_weights() {
    let net = |w: Bounded<f64>| {
        let mut n = [
            [1.0, 2.0, 3.0, 1.0],
            [2.0, 0.0, 1.0, 2.0],
            [0.5, 1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 0.5],
        ]
        .map(|p| p.map(Bounded::exact));
        n[3][3] = w;
        HomPatch::new(1, 1, n).expect("bilinear")
    };
    let ok = net(Bounded::exact(0.5)).hull_box().expect("positive");
    assert!(ok.contains([1.0, 2.0, 3.0]) && ok.contains([2.0, 2.0, 2.0]));
    assert!(!ok.contains([3.0, 0.0, 0.0]));
    for w in [
        Bounded::exact(0.0),
        Bounded::exact(-0.5),
        Bounded::new(0.5, 0.0, 0.5),
        Bounded::new(0.5, f64::INFINITY, 0.0),
        Bounded::exact(f64::NAN),
    ] {
        let p = net(w);
        assert!(!p.weights_positive(), "{w:?}");
        assert!(p.hull_box().is_none(), "{w:?}");
    }
}

/// The box of the projected control points is tight on exact data: here
/// each side is within a few ulps of the extreme control point.
#[test]
fn hull_box_is_tight() {
    let p = HomPatch::from_f64(
        2,
        2,
        &[
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.25, 0.5],
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 0.5, 0.0, 0.5],
            [0.5, 0.5, 1.5, 0.5],
            [1.0, 0.5, 0.0, 0.5],
            [0.0, 2.0, 0.0, 1.0],
            [0.5, 1.0, 0.0, 0.5],
            [6.0, 6.0, 0.0, 3.0],
        ],
    )
    .expect("biquadratic");
    let b = p.hull_box().expect("positive weights");
    let near = |a: f64, x: f64| (a - x).abs() <= 4.0 * f64::EPSILON * (1.0 + x.abs());
    for (k, (lo, hi)) in [(0.0, 2.0), (0.0, 2.0), (0.0, 3.0)].into_iter().enumerate() {
        assert!(near(b.lo[k], lo) && b.lo[k] <= lo, "{b:?}");
        assert!(near(b.hi[k], hi) && b.hi[k] >= hi, "{b:?}");
    }
}

#[test]
fn degrees_are_limited() {
    let one = || Q::from(1u32);
    assert!(Tensor::new(1, 1, (0..3).map(|_| one())).is_none());
    assert!(Tensor::new(1, 1, (0..5).map(|_| one())).is_none());
    let max = Tensor::new(MAX_DEGREE, 0, (0..=MAX_DEGREE).map(|_| one())).expect("at the limit");
    assert_eq!(max.transposed().degrees(), (0, MAX_DEGREE));
    assert!(Tensor::new(MAX_DEGREE + 1, 0, (0..=MAX_DEGREE + 1).map(|_| one())).is_none());
    assert!(HomPatch::<Q>::new(usize::MAX, usize::MAX, []).is_none());
    let t = Tensor::from_f64(0, 0, &[3.0]).expect("constant");
    assert_eq!(t.derivative(Dir::U).coefs(), &[Bounded::exact(0.0)]);
    let a = Tensor::from_f64(1, 0, &[1.0, 2.0]).expect("linear");
    let b = Tensor::from_f64(0, 1, &[1.0, 2.0]).expect("linear");
    let w = Tensor::from_f64(1, 0, &[1.0, 1.0]).expect("linear");
    assert!(HomPatch::from_tensors([a.clone(), a.clone(), b, w.clone()]).is_none());
    assert!(HomPatch::from_tensors([a.clone(), a.clone(), a, w]).is_some());
}

/// A patch from affine control points `(x, y, z, w)` at level `S`: each
/// coefficient holds the true `w·x` (exactly from double-double up) and
/// the weights are the stored ones; the exact patch's corners are the
/// corner control points.
fn affine_corners<S: Level>(exact_level: bool) {
    let like = S::of(0.0, 0.0);
    let mut rng = Rng::new(11);
    for _ in 0..100 {
        let net: Vec<[f64; 4]> = (0..9)
            .map(|_| {
                let [x, y, z] = [(); 3].map(|_| rng.pow(-20, 20));
                [x, y, z, rng.pow(-6, 6).abs()]
            })
            .collect();
        let h = HomPatch::<S>::from_affine(2, 2, net.iter().copied(), &like).unwrap();
        let e = HomPatch::<Q>::from_affine(2, 2, net.iter().copied(), &Q::from(0u32)).unwrap();
        for (i, p) in net.iter().enumerate() {
            assert_eq!(h.weights().coefs()[i].val(), q(p[3]));
            assert_eq!(h.weights().coefs()[i].errs(), (0.0, 0.0));
            for k in 0..3 {
                let c = &h.tensors()[k].coefs()[i];
                let (t, r) = c.errs();
                let truth = q(p[k]) * q(p[3]);
                assert!(qabs(c.val() - &truth) <= q(t) + q(r));
                if exact_level {
                    assert_eq!(c.val(), truth);
                    assert_eq!((t, r), (0.0, 0.0));
                }
            }
        }
        for (i, j) in [(0, 0), (0, 2), (2, 0), (2, 2)] {
            let (u, v) = (Q::from(i / 2), Q::from(j / 2));
            let p = net[3 * i + j];
            assert_eq!(e.eval(&u, &v), [p[0], p[1], p[2]].map(q));
        }
    }
}

#[test]
fn affine_patches_hold_their_products() {
    affine_corners::<Bounded<f64>>(false);
    affine_corners::<Bounded<Dd>>(true);
    affine_corners::<Bounded<Big>>(true);
}
