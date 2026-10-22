//! Derivatives: on rationals the point's derivatives are the numerators'
//! quotients `A / W^(k + 1)` (an independent recurrence on forms), the
//! numerators `W²·x'` and the hodographs of polynomial forms; at every
//! level each certified value holds the exact one; the first order is
//! `eval_partials` to the bit; the numerators change sign with reversal
//! and follow transposition, to the bit but the signs of zeros.

use malachite::Rational as Q;

use super::*;
use crate::bezier::tests::{
    Level, Rng, dyadic, holds, poly, tbits, tensor, tensor_holds, value_and_errs,
};
use crate::num::bigf::Big;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

/// The orders `(a, b)` of a mixed partial.
type Order = (usize, usize);

/// A rational patch of degrees `(m, n)`, positive weights in `[1/8, 8]`.
fn rational_patch(rng: &mut Rng, m: usize, n: usize, polynomial: bool) -> HomPatch<Q> {
    let net: Vec<[Q; 4]> = (0..(m + 1) * (n + 1))
        .map(|_| {
            let w = if polynomial {
                Q::from(1u32)
            } else {
                Q::from(rng.below(64) + 1) / Q::from(8u32)
            };
            let [x, y, z] = [0, 1, 2].map(|_| Q::from(rng.below(201) as i64 - 100) * &w);
            [x, y, z, w]
        })
        .collect();
    HomPatch::new(m, n, net).expect("degrees in range")
}

fn rational_curve(rng: &mut Rng, d: usize, polynomial: bool) -> HomCurve<Q, 3> {
    HomCurve::new((0..=d).map(|_| {
        let w = if polynomial {
            Q::from(1u32)
        } else {
            Q::from(rng.below(64) + 1) / Q::from(8u32)
        };
        (
            [0, 1, 2].map(|_| Q::from(rng.below(201) as i64 - 100) * &w),
            w,
        )
    }))
    .expect("degree in range")
}

/// `A·W − k·W'·A`, the next numerator of the recurrence `x⁽ᵏ⁾ =
/// Aₖ / Wᵏ⁺¹`, `A_(k+1) = Aₖ'·W − (k + 1)·W'·Aₖ`.
fn next_poly(a: &Poly<Q>, w: &Poly<Q>, k: u32) -> Poly<Q> {
    let dw = w.derivative();
    a.derivative()
        .mul(w)
        .expect("in range")
        .sub(&dw.mul(a).expect("in range").scale(&Q::from(k)))
}

#[test]
fn curve_derivatives_are_the_numerators_quotients() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..12 {
            let d = 1 + case % 4;
            let curve = rational_curve(&mut rng, d, false);
            let w = curve.weights().clone();
            // `A₁ = W·P' − W'·P`, then the recurrence.
            let mut a: Vec<[Poly<Q>; 3]> = vec![curve.tangent_numerator().expect("in range")];
            for k in 2..=4u32 {
                let last = a.last().expect("one");
                a.push(std::array::from_fn(|c| next_poly(&last[c], &w, k)));
            }
            for _ in 0..3 {
                let t = dyadic(&mut rng);
                let x = curve.derivatives_at(&t, 4);
                assert_eq!(x.len(), 5);
                assert_eq!(x[0], curve.eval(&t));
                let wt = w.eval(&t);
                let mut wk = wt.clone();
                for k in 1..=4 {
                    wk *= &wt;
                    for c in 0..3 {
                        assert_eq!(
                            x[k][c],
                            a[k - 1][c].eval(&t) / &wk,
                            "seed {seed} case {case}"
                        );
                    }
                }
            }
            // A polynomial span: the hodographs.
            let poly_curve = rational_curve(&mut rng, d, true);
            let t = dyadic(&mut rng);
            let x = poly_curve.derivatives_at(&t, 3);
            for (k, xk) in x.iter().enumerate() {
                for (xc, pc) in xk.iter().zip(poly_curve.coords()) {
                    assert_eq!(*xc, pc.derivative_n(k).eval(&t));
                }
            }
            let tn = poly_curve.tangent_numerator().expect("in range");
            assert_eq!(tn[0], poly_curve.coords()[0].derivative());
            assert_eq!(tn[0].degree(), d - 1);
        }
    }
}

#[test]
fn patch_derivatives_are_the_numerators_quotients() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..8 {
            let (m, n) = (1 + case % 3, 1 + (case / 3) % 2);
            let patch = rational_patch(&mut rng, m, n, false);
            let w = patch.weights().clone();
            let (wu, wv) = (w.derivative(Dir::U), w.derivative(Dir::V));
            // `x^(a, b) = A_ab / W^(a + b + 1)`: `A₁₀`, `A₀₁` the partial
            // numerators, `A_(a+1),b = ∂_u A_ab·W − (a + b + 1)·W_u·A_ab`
            // (and in `v`).
            let step = |a: &Tensor<Q>, dir: Dir, k: u32| -> Tensor<Q> {
                let dw = if dir == Dir::U { &wu } else { &wv };
                a.derivative(dir)
                    .mul(&w)
                    .expect("in range")
                    .sub(&dw.mul(a).expect("in range").scale(&Q::from(k)))
            };
            let nu = patch.partial_numerator(Dir::U).expect("in range");
            let nv = patch.partial_numerator(Dir::V).expect("in range");
            assert_eq!(nu[0].degrees(), (2 * m - 1, 2 * n));
            assert_eq!(nv[0].degrees(), (2 * m, 2 * n - 1));
            let nums: Vec<(Order, [Tensor<Q>; 3])> = vec![
                ((1, 0), nu.clone()),
                ((0, 1), nv.clone()),
                ((2, 0), nu.clone().map(|a| step(&a, Dir::U, 2))),
                ((1, 1), nu.clone().map(|a| step(&a, Dir::V, 2))),
                ((0, 2), nv.clone().map(|a| step(&a, Dir::V, 2))),
                (
                    (2, 1),
                    nu.clone().map(|a| step(&step(&a, Dir::U, 2), Dir::V, 3)),
                ),
                (
                    (0, 3),
                    nv.clone().map(|a| step(&step(&a, Dir::V, 2), Dir::V, 3)),
                ),
            ];
            for _ in 0..2 {
                let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
                let d = patch.derivatives_at(&u, &v, 3);
                assert_eq!(d.order(), 3);
                assert_eq!(d.get(0, 0).expect("the point"), &patch.eval(&u, &v));
                assert!(d.get(2, 2).is_none());
                let wt = w.eval(&u, &v);
                for ((a, b), num) in &nums {
                    let mut wk = wt.clone();
                    for _ in 0..a + b {
                        wk *= &wt;
                    }
                    let x = d.get(*a, *b).expect("within the order");
                    for c in 0..3 {
                        assert_eq!(
                            x[c],
                            num[c].eval(&u, &v) / &wk,
                            "seed {seed} case {case} ({a}, {b})"
                        );
                    }
                }
            }
            // A polynomial patch: the hodographs.
            let poly_patch = rational_patch(&mut rng, m, n, true);
            let nu = poly_patch.partial_numerator(Dir::U).expect("in range");
            assert_eq!(nu[1], poly_patch.tensors()[1].derivative(Dir::U));
            let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
            let d = poly_patch.derivatives_at(&u, &v, 2);
            let x = &poly_patch.tensors()[2];
            assert_eq!(
                d.get(1, 1).expect("order 2")[2],
                x.derivative(Dir::U).derivative(Dir::V).eval(&u, &v)
            );
            assert_eq!(
                d.get(0, 2).expect("order 2")[2],
                x.derivative_n(Dir::V, 2).eval(&u, &v)
            );
        }
    }
}

/// A level's rational patch with positive weights and its true one.
fn patch<S: Level>(rng: &mut Rng, m: usize, n: usize, errs: bool) -> (HomPatch<S>, HomPatch<Q>) {
    let parts: Vec<_> = (0..4)
        .map(|k| tensor::<S>(rng, m, n, errs, k == 3))
        .collect();
    (
        HomPatch::from_tensors(std::array::from_fn(|k| parts[k].0.clone())).expect("degrees"),
        HomPatch::from_tensors(std::array::from_fn(|k| parts[k].1.clone())).expect("degrees"),
    )
}

/// At every level: each certified derivative holds the exact one, the
/// numerators hold theirs, and the first order is `eval_partials`.
fn bounds<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..20 {
            let what = format!("seed {seed} case {case}");
            let (m, n) = (1 + case % 2, 1 + (case / 2) % 3);
            let (p, pq) = patch::<S>(&mut rng, m, n, case % 2 == 0);
            for dir in [Dir::U, Dir::V] {
                let nn = p.partial_numerator(dir).expect("in range");
                let nq = pq.partial_numerator(dir).expect("in range");
                for c in 0..3 {
                    tensor_holds(&what, &nn[c], &nq[c], &mut stats);
                }
            }
            let (u, v) = (rng.t(), rng.t());
            let like = p.weights().coefs()[0].clone();
            let (us, vs) = (S::from_f64(u, &like), S::from_f64(v, &like));
            let d = p.derivatives_at(&us, &vs, 3);
            let dq = pq.derivatives_at(
                &Q::try_from(u).expect("finite"),
                &Q::try_from(v).expect("finite"),
                3,
            );
            for a in 0..=3 {
                for b in 0..=3 - a {
                    let (x, xq) = (d.get(a, b).expect("in"), dq.get(a, b).expect("in"));
                    for c in 0..3 {
                        stats[1] += 1;
                        if holds(&format!("{what} ({a}, {b})"), &x[c], &xq[c]) {
                            stats[0] += 1;
                        }
                    }
                }
            }
            let first = p.eval_partials(&us, &vs);
            let d1 = p.derivatives_at(&us, &vs, 1);
            let bits = |x: &[S; 3]| x.iter().map(Level::bits).collect::<Vec<_>>();
            assert_eq!(bits(d1.get(0, 0).expect("point")), bits(&first.p), "{what}");
            assert_eq!(bits(d1.get(1, 0).expect("u")), bits(&first.du), "{what}");
            assert_eq!(bits(d1.get(0, 1).expect("v")), bits(&first.dv), "{what}");

            let cd = 1 + case % 3;
            let cs: Vec<_> = (0..3)
                .map(|_| poly::<S>(&mut rng, cd, case % 3 == 0))
                .collect();
            // Positive weights: a line of a weight tensor.
            let (wt, wq) = tensor::<S>(&mut rng, cd, 0, case % 3 == 1, true);
            let w = Poly::new(wt.coefs().to_vec()).expect("a line");
            let wq = Poly::new(wq.coefs().to_vec()).expect("a line");
            let curve: HomCurve<S, 3> =
                HomCurve::from_polys(std::array::from_fn(|k| cs[k].0.clone()), w)
                    .expect("one degree");
            let cq: HomCurve<Q, 3> =
                HomCurve::from_polys(std::array::from_fn(|k| cs[k].1.clone()), wq)
                    .expect("one degree");
            let t = rng.t();
            let x = curve.derivatives_at(&S::from_f64(t, &like), 3);
            let xq = cq.derivatives_at(&Q::try_from(t).expect("finite"), 3);
            for (xk, xqk) in x.iter().zip(&xq) {
                for c in 0..3 {
                    stats[1] += 1;
                    if holds(&format!("{what} curve"), &xk[c], &xqk[c]) {
                        stats[0] += 1;
                    }
                }
            }
        }
    }
    assert!(stats[0] * 2 >= stats[1], "{}: certified {stats:?}", S::NAME);
}

#[test]
fn derivatives_hold_their_bounds_f64() {
    bounds::<Bounded<f64>>(varde_testing::seeds(1, 16));
}

#[test]
fn derivatives_hold_their_bounds_dd() {
    bounds::<Bounded<Dd>>(varde_testing::seeds(1, 16));
}

#[test]
fn derivatives_hold_their_bounds_big() {
    bounds::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// The partial numerators reversed and transposed, at every level.
fn mirror<S: Level>() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..20 {
            let what = format!("{} seed {seed} case {case}", S::NAME);
            let (m, n) = (1 + case % 3, 1 + (case / 3) % 2);
            let (p, _) = patch::<S>(&mut rng, m, n, case % 2 == 1);
            for dir in [Dir::U, Dir::V] {
                let other = if dir == Dir::U { Dir::V } else { Dir::U };
                let nn = p.partial_numerator(dir).expect("in range");
                let along = p.reversed(dir).partial_numerator(dir).expect("in range");
                let across = p.reversed(other).partial_numerator(dir).expect("in range");
                for c in 0..3 {
                    assert_eq!(
                        value_and_errs(along[c].coefs()),
                        value_and_errs(nn[c].reversed(dir).neg().coefs()),
                        "{what} along {dir:?}"
                    );
                    assert_eq!(
                        tbits(&across[c]),
                        tbits(&nn[c].reversed(other)),
                        "{what} across {dir:?}"
                    );
                }
            }
            let nu = p.partial_numerator(Dir::U).expect("in range");
            let nv = p.transposed().partial_numerator(Dir::V).expect("in range");
            for c in 0..3 {
                assert_eq!(
                    tbits(&nv[c]),
                    tbits(&nu[c].transposed()),
                    "{what} transposed"
                );
            }
        }
    }
}

#[test]
fn partial_numerators_commute_with_reversal_and_transposition() {
    mirror::<Bounded<f64>>();
    mirror::<Bounded<Dd>>();
    mirror::<Bounded<Big>>();
}

#[test]
fn derivatives_past_the_degree_vanish() {
    let p = Poly::new([1, 4, 9, 16].map(Q::from)).expect("cubic");
    assert_eq!(p.derivative_n(0), p);
    assert_eq!(p.derivative_n(2), p.derivative().derivative());
    assert_eq!(p.derivative_n(3).degree(), 0);
    let zero = Poly::new([Q::from(0u32)]).expect("constant");
    assert_eq!(p.derivative_n(4), zero);
    assert_eq!(p.derivative_n(usize::MAX), zero);
    let f = Tensor::new(1, 2, (0..6).map(Q::from)).expect("degrees");
    assert_eq!(f.derivative_n(Dir::V, 2).degrees(), (1, 0));
    assert_eq!(f.derivative_n(Dir::U, 5).coefs(), &[0, 0, 0].map(Q::from));
}
