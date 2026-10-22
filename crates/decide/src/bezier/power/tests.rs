//! The power basis: exact and inverse on rationals (the evaluation of
//! the power form is the Bernstein form's), certified at every level, and
//! the exact instance `crate::num::poly` uses.

use malachite::Rational as Q;

use super::*;
use crate::bezier::tests::{Level, Rng, dyadic, holds, int_tensor, ints, poly, tensor};
use crate::num::bigf::Big;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::poly as exact;

/// `Σ aₖ·tᵏ` by Horner.
fn horner(a: &[Q], t: &Q) -> Q {
    a.iter().rev().fold(Q::from(0u32), |s, c| s * t + c)
}

#[test]
fn rational_conversions_are_exact_and_inverse() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..30 {
            let n = case % 12;
            let p = ints(&mut rng, n);
            let a = p.to_power();
            assert_eq!(a.len(), n + 1);
            for _ in 0..4 {
                let t = dyadic(&mut rng);
                assert_eq!(horner(&a, &t), p.eval(&t), "seed {seed} case {case}");
            }
            let like = Q::from(0u32);
            assert_eq!(Poly::from_power(&a, n, &like).expect("degree n"), p);
            // At a higher degree, the elevated form.
            let up = Poly::from_power(&a, n + 3, &like).expect("in range");
            assert_eq!(up, p.elevate(3).expect("in range"));
            assert_eq!(exact::from_bernstein(p.coefs()), exact::trim(a.clone()));
            assert_eq!(exact::to_bernstein(&a, n + 4), up.coefs());

            let (m, k) = (case % 4, (case / 4) % 4);
            let f = int_tensor(&mut rng, m, k);
            let fa = f.to_power();
            let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
            let mut s = Q::from(0u32);
            for i in (0..=m).rev() {
                let row = &fa[i * (k + 1)..(i + 1) * (k + 1)];
                s = s * &u + horner(row, &v);
            }
            assert_eq!(s, f.eval(&u, &v));
            assert_eq!(Tensor::from_power(m, k, &fa).expect("degrees"), f);
        }
    }
    let like = Q::from(0u32);
    assert_eq!(
        Poly::from_power(&[], 2, &like).expect("zero").coefs(),
        &[0, 0, 0].map(Q::from)
    );
    assert!(Poly::from_power(&[like.clone(), like.clone()], 0, &like).is_none());
    assert!(Poly::<Q>::from_power(&[], MAX_DEGREE + 1, &like).is_none());
    assert!(Tensor::<Q>::from_power(1, 1, std::slice::from_ref(&like)).is_none());
}

fn bounds<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    let mut check = |what: &str, x: &S, xq: &Q| {
        stats[1] += 1;
        if holds(what, x, xq) {
            stats[0] += 1;
        }
    };
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..40 {
            let what = format!("seed {seed} case {case}");
            let n = case % 10;
            let (p, truth) = poly::<S>(&mut rng, n, case % 2 == 0);
            let a = p.to_power();
            let aq = truth.to_power();
            for (x, xq) in a.iter().zip(&aq) {
                check(&what, x, xq);
            }
            let like = p.coefs()[0].clone();
            let b = Poly::from_power(&a, n + case % 3, &like).expect("in range");
            let bq = Poly::from_power(
                &a.iter().map(Level::val).collect::<Vec<_>>(),
                n + case % 3,
                &Q::from(0u32),
            )
            .expect("in range");
            for (x, xq) in b.coefs().iter().zip(bq.coefs()) {
                check(&what, x, xq);
            }
            let (f, fq) = tensor::<S>(&mut rng, case % 3, (case / 3) % 3, case % 3 == 0, false);
            for (x, xq) in f.to_power().iter().zip(&fq.to_power()) {
                check(&what, x, xq);
            }
        }
    }
    assert!(
        stats[0] * 10 >= stats[1] * 9,
        "{}: certified {stats:?}",
        S::NAME
    );
}

#[test]
fn conversions_hold_their_bounds() {
    bounds::<Bounded<f64>>(varde_testing::seeds(1, 16));
    bounds::<Bounded<Dd>>(varde_testing::seeds(1, 16));
    bounds::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// The first level's power coefficients of a quadratic are the plain
/// float expressions `b₀`, `2·(b₁ − b₀)`, `b₀ − 2·b₁ + b₂`.
#[test]
fn quadratic_power_coefficients_are_the_float_expressions() {
    let mut rng = Rng::new(5);
    for _ in 0..200 {
        let b = [0, 1, 2].map(|_| rng.pow(-20, 20));
        let a = Poly::from_f64(&b).expect("quadratic").to_power();
        let [b0, b1, b2] = b;
        let want = [b0, 2.0 * (b1 - b0), b0 - 2.0 * b1 + b2];
        for (x, w) in a.iter().zip(want) {
            assert_eq!(x.v.to_bits(), w.to_bits());
        }
    }
}
