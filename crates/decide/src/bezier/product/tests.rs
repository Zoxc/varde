//! Products, sums and elevation: on rationals exact against the products
//! of values; at every level each certified coefficient holds the exact
//! result of the same operation on the true coefficients, up to
//! [`MAX_DEGREE`] where the binomial weights pass `2⁵³`; reversal and the
//! swap of the operands give the same bits; the tangent and normal
//! numerators are `W²·x'` and `W³·(x_u × x_v)`.

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Exact;

use super::*;
use crate::bezier::tests::{Level, Rng, holds, q};
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

fn ints(rng: &mut Rng, n: usize) -> Poly<Q> {
    Poly::new((0..=n).map(|_| Q::from(rng.below(201) as i64 - 100))).expect("degree in range")
}

fn int_tensor(rng: &mut Rng, m: usize, n: usize) -> Tensor<Q> {
    let c = (0..(m + 1) * (n + 1)).map(|_| Q::from(rng.below(201) as i64 - 100));
    Tensor::new(m, n, c).expect("degrees in range")
}

/// A dyadic in `[0, 1]`.
fn dyadic(rng: &mut Rng) -> Q {
    Q::from(rng.below(65)) / Q::from(64u32)
}

/// A level's polynomial and its true one.
fn poly<S: Level>(rng: &mut Rng, n: usize, errs: bool) -> (Poly<S>, Poly<Q>) {
    let (p, truth) = rng.poly::<S>(n, errs);
    (p, Poly::new(truth).expect("degree in range"))
}

/// A level's tensor and its true one; `weight` keeps the coefficients
/// in `[1/64, 64]`.
fn tensor<S: Level>(
    rng: &mut Rng,
    m: usize,
    n: usize,
    errs: bool,
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
        if errs && rng.below(2) == 0 {
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

fn poly_holds<S: Level>(what: &str, p: &Poly<S>, truth: &Poly<Q>, stats: &mut [u32; 2]) {
    assert_eq!(p.degree(), truth.degree(), "{what}");
    for (b, x) in p.coefs().iter().zip(truth.coefs()) {
        stats[1] += 1;
        if holds(what, b, x) {
            stats[0] += 1;
        }
    }
}

fn tensor_holds<S: Level>(what: &str, f: &Tensor<S>, truth: &Tensor<Q>, stats: &mut [u32; 2]) {
    assert_eq!(f.degrees(), truth.degrees(), "{what}");
    for (b, x) in f.coefs().iter().zip(truth.coefs()) {
        stats[1] += 1;
        if holds(what, b, x) {
            stats[0] += 1;
        }
    }
}

fn pbits<S: Level>(p: &Poly<S>) -> Vec<String> {
    p.coefs().iter().map(Level::bits).collect()
}

fn tbits<S: Level>(f: &Tensor<S>) -> Vec<String> {
    f.coefs().iter().map(Level::bits).collect()
}

/// Each coefficient's value and errors: its bits but the signs of zero
/// parts (`x − x` is `+0` either way round, so a negated difference may
/// differ in them).
fn value_and_errs<S: Level>(c: &[S]) -> Vec<(Q, (f64, f64))> {
    c.iter().map(|x| (x.val(), x.errs())).collect()
}

#[test]
fn int_u64_is_certified() {
    fn check<S: Level>(like: &S, exact_always: bool) {
        let mut ks = vec![
            0,
            1,
            (1 << 53) - 1,
            1 << 53,
            (1 << 53) + 1,
            (1 << 60) + 3,
            u64::MAX,
            binom(64, 32),
            binom(57, 28),
        ];
        let mut rng = Rng::new(7);
        ks.extend((0..200).map(|_| rng.0.next_u64() >> rng.below(64)));
        for k in ks {
            let x = S::int_u64(k, like);
            assert!(x.certified(), "{} {k}", S::NAME);
            let (t, r) = x.errs();
            let off = q(t) + q(r);
            let d = Q::from(k) - x.val();
            assert!(d <= off && -d <= off, "{} {k}: {x:?}", S::NAME);
            if exact_always || k <= 1 << 53 {
                assert_eq!(x.val(), Q::from(k), "{} {k}", S::NAME);
                assert_eq!((t, r), (0.0, 0.0), "{} {k}", S::NAME);
            }
            if k <= u64::from(u32::MAX) {
                assert_eq!(x, S::int(u32::try_from(k).expect("small"), like));
            }
        }
    }
    check(&Bounded::exact(0.0), false);
    check(&Bounded::exact(Dd::from_f64(0.0)), true);
    check(&Bounded::exact(Big::from_f64(0.0, Width::MIN)), true);
    for k in [0, 1 << 53, u64::MAX] {
        assert_eq!(Q::int_u64(k, &Q::from(0u32)), Q::from(k));
    }
}

#[test]
fn binomials_are_exact() {
    let mut row = vec![1u64];
    for n in 1..=MAX_DEGREE {
        let mut next = vec![1u64; n + 1];
        for k in 1..n {
            next[k] = row[k - 1] + row[k];
        }
        row = next;
        for (k, &c) in row.iter().enumerate() {
            assert_eq!(binom(n, k), c, "C({n}, {k})");
        }
    }
}

/// On rationals: the product's value is the product of the values, the
/// sum's the sum, at every degree pair up to 8; the swap gives the same
/// product; the degree limit holds.
#[test]
fn rational_products_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..60 {
            let (m, n) = (case % 9, (case / 9) % 9);
            let (a, b) = (ints(&mut rng, m), ints(&mut rng, n));
            let p = a.mul(&b).expect("in range");
            assert_eq!(p.degree(), m + n);
            assert_eq!(p, b.mul(&a).expect("in range"));
            let (s, d) = (a.add(&b), a.sub(&b));
            assert_eq!(s.degree(), m.max(n));
            let k = Q::from(rng.below(21) as i64 - 10) / Q::from(3u32);
            let sc = a.scale(&k);
            for _ in 0..4 {
                let t = dyadic(&mut rng);
                let (x, y) = (a.eval(&t), b.eval(&t));
                assert_eq!(p.eval(&t), &x * &y);
                assert_eq!(s.eval(&t), &x + &y);
                assert_eq!(d.eval(&t), &x - &y);
                assert_eq!(sc.eval(&t), &k * &x);
                assert_eq!(a.neg().eval(&t), -&x);
            }
            let (m, n) = (case % 4, (case / 4) % 4);
            let (f, g) = (int_tensor(&mut rng, m, n), int_tensor(&mut rng, n, m));
            let h = f.mul(&g).expect("in range");
            assert_eq!(h.degrees(), (m + n, n + m));
            assert_eq!(h, g.mul(&f).expect("in range"));
            let (s, d) = (f.add(&g), f.sub(&g));
            let e = f.elevate_to(m + 2, n + 1).expect("in range");
            assert_eq!(e.degrees(), (m + 2, n + 1));
            for _ in 0..4 {
                let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
                let (x, y) = (f.eval(&u, &v), g.eval(&u, &v));
                assert_eq!(h.eval(&u, &v), &x * &y);
                assert_eq!(s.eval(&u, &v), &x + &y);
                assert_eq!(d.eval(&u, &v), &x - &y);
                assert_eq!(e.eval(&u, &v), x);
                assert_eq!(f.neg().eval(&u, &v), -&x);
                assert_eq!(f.scale(&k).eval(&u, &v), &k * &x);
            }
        }
    }
}

/// The degree limit: products and elevations up to [`MAX_DEGREE`], none
/// past it; at the limit still exact (weights up to `C(64, 32)`).
#[test]
fn degrees_are_limited() {
    let mut rng = Rng::new(3);
    let (a, b) = (ints(&mut rng, 32), ints(&mut rng, 32));
    let p = a.mul(&b).expect("at the limit");
    assert_eq!(p.degree(), MAX_DEGREE);
    let t = Q::from(5u32) / Q::from(7u32);
    assert_eq!(p.eval(&t), a.eval(&t) * b.eval(&t));
    assert!(p.mul(&ints(&mut rng, 1)).is_none());
    assert!(ints(&mut rng, 33).mul(&b).is_none());
    let f = int_tensor(&mut rng, 30, 2);
    let g = int_tensor(&mut rng, 34, 3);
    let h = f.mul(&g).expect("at the limit");
    assert_eq!(h.degrees(), (64, 5));
    let (u, v) = (Q::from(1u32) / Q::from(3u32), Q::from(3u32) / Q::from(5u32));
    assert_eq!(h.eval(&u, &v), f.eval(&u, &v) * g.eval(&u, &v));
    assert!(h.mul(&int_tensor(&mut rng, 1, 0)).is_none());
    assert!(h.transposed().mul(&int_tensor(&mut rng, 0, 1)).is_none());
    assert!(h.elevate(Dir::U, 1).is_none());
    assert!(h.elevate(Dir::V, usize::MAX).is_none());
    assert_eq!(
        h.elevate(Dir::V, MAX_DEGREE - 5)
            .expect("to the limit")
            .degrees(),
        (64, 64)
    );
    assert!(f.elevate_to(29, 2).is_none(), "below its degree");
    assert!(f.elevate_to(30, 65).is_none());
    assert_eq!(f.elevate(Dir::U, 0).expect("by zero"), f);
}

/// Every certified coefficient of a product, sum, elevation, tangent and
/// normal numerator holds the exact result on the true coefficients.
fn bounds<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..60 {
            let what = format!("seed {seed} case {case}");
            let (m, n) = (case % 9, (case / 9) % 7);
            let (a, aq) = poly::<S>(&mut rng, m, case % 2 == 0);
            let (b, bq) = poly::<S>(&mut rng, n, case % 3 == 0);
            let pq = aq.mul(&bq).expect("in range");
            poly_holds(&what, &a.mul(&b).expect("in range"), &pq, &mut stats);
            poly_holds(&what, &a.add(&b), &aq.add(&bq), &mut stats);
            poly_holds(&what, &a.sub(&b), &aq.sub(&bq), &mut stats);
            let (k, kq) = poly::<S>(&mut rng, 0, true);
            poly_holds(
                &what,
                &a.scale(&k.coefs()[0]),
                &aq.scale(&kq.coefs()[0]),
                &mut stats,
            );

            let (m, n) = (case % 4, (case / 4) % 3);
            let (f, fq) = tensor::<S>(&mut rng, m, n, case % 2 == 0, false);
            let (g, gq) = tensor::<S>(&mut rng, n + 1, m, case % 3 == 0, false);
            let hq = fq.mul(&gq).expect("in range");
            tensor_holds(&what, &f.mul(&g).expect("in range"), &hq, &mut stats);
            tensor_holds(&what, &f.add(&g), &fq.add(&gq), &mut stats);
            tensor_holds(&what, &f.sub(&g), &fq.sub(&gq), &mut stats);
            tensor_holds(
                &what,
                &f.elevate_to(m + 3, n + 2).expect("in range"),
                &fq.elevate_to(m + 3, n + 2).expect("in range"),
                &mut stats,
            );

            // A rational biquadratic (or other low-degree) patch: its
            // normal numerator, and a rational span's tangent numerator.
            let (pm, pn) = (1 + case % 2, 1 + (case / 2) % 2);
            let parts: Vec<_> = [false, false, false, true]
                .iter()
                .map(|&w| tensor::<S>(&mut rng, pm, pn, case % 2 == 1, w))
                .collect();
            let patch = HomPatch::from_tensors(std::array::from_fn(|k| parts[k].0.clone()))
                .expect("one pair of degrees");
            let truth = HomPatch::from_tensors(std::array::from_fn(|k| parts[k].1.clone()))
                .expect("one pair of degrees");
            let nn = patch.normal_numerator().expect("in range");
            let nq = truth.normal_numerator().expect("in range");
            for k in 0..3 {
                assert_eq!(nn[k].degrees(), (3 * pm - 1, 3 * pn - 1));
                tensor_holds(&what, &nn[k], &nq[k], &mut stats);
            }
            let cd = 1 + case % 3;
            let cs: Vec<_> = (0..4)
                .map(|_| poly::<S>(&mut rng, cd, case % 2 == 0))
                .collect();
            let curve: HomCurve<S, 3> =
                HomCurve::from_polys(std::array::from_fn(|k| cs[k].0.clone()), cs[3].0.clone())
                    .expect("one degree");
            let cq: HomCurve<Q, 3> =
                HomCurve::from_polys(std::array::from_fn(|k| cs[k].1.clone()), cs[3].1.clone())
                    .expect("one degree");
            let tn = curve.tangent_numerator().expect("in range");
            let tq: [Poly<Q>; 3] = cq.tangent_numerator().expect("in range");
            for k in 0..3 {
                assert_eq!(tn[k].degree(), 2 * cd - 1);
                poly_holds(&what, &tn[k], &tq[k], &mut stats);
            }
        }
    }
    // Not vacuous: nearly everything certifies (the rest overflow).
    assert!(
        stats[0] * 10 >= stats[1] * 9,
        "{}: certified {stats:?}",
        S::NAME
    );
}

#[test]
fn products_hold_their_bounds_f64() {
    bounds::<Bounded<f64>>(varde_testing::seeds(1, 16));
}

#[test]
fn products_hold_their_bounds_dd() {
    bounds::<Bounded<Dd>>(varde_testing::seeds(1, 16));
}

#[test]
fn products_hold_their_bounds_big() {
    bounds::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// Up to [`MAX_DEGREE`], where the weights pass `2⁵³` (the first level's
/// then rounded and certified): integer and random coefficients, every
/// coefficient certified and holding the exact product.
fn high_degree<S: Level>(seeds: std::ops::Range<u64>) {
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for (m, n) in [(32, 32), (28, 36), (40, 17), (57, 7)] {
            let what = format!("seed {seed} degrees {m}, {n}");
            let ai = ints(&mut rng, m);
            let bi = ints(&mut rng, n);
            let lift = |p: &Poly<Q>| {
                Poly::new(
                    p.coefs()
                        .iter()
                        .map(|x| S::of(f64::rounding_from(x, Exact).0, 0.0)),
                )
                .expect("degree in range")
            };
            let mut stats = [0u32; 2];
            let p = lift(&ai).mul(&lift(&bi)).expect("in range");
            poly_holds(&what, &p, &ai.mul(&bi).expect("in range"), &mut stats);
            let (a, aq) = poly::<S>(&mut rng, m, true);
            let (b, bq) = poly::<S>(&mut rng, n, false);
            poly_holds(
                &what,
                &a.mul(&b).expect("in range"),
                &aq.mul(&bq).expect("in range"),
                &mut stats,
            );
            assert!(stats[0] * 10 >= stats[1] * 9, "{what}: {stats:?}");
        }
    }
}

#[test]
fn bounds_hold_up_to_max_degree() {
    high_degree::<Bounded<f64>>(varde_testing::seeds(1, 8));
    high_degree::<Bounded<Dd>>(varde_testing::seeds(1, 8));
    high_degree::<Bounded<Big>>(varde_testing::seeds(1, 2));
}

/// Products of the reversed operands are the reversed product, the swap
/// of the operands the same product, to the bit; for tensors in either
/// direction, and their boundary lines are the 1D products of the
/// operands' boundary lines. Elevation of tensors commutes with
/// reversal.
fn mirror<S: Level>() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..60 {
            let (a, _) = rng.poly::<S>(case % 9, case % 2 == 0);
            let (b, _) = rng.poly::<S>((case / 3) % 7, case % 3 == 0);
            let p = a.mul(&b).expect("in range");
            let what = format!("{} seed {seed} case {case}", S::NAME);
            assert_eq!(
                pbits(&a.reversed().mul(&b.reversed()).expect("in range")),
                pbits(&p.reversed()),
                "{what}"
            );
            assert_eq!(pbits(&b.mul(&a).expect("in range")), pbits(&p), "{what}");
            assert_eq!(
                pbits(&a.reversed().sub(&b.reversed())),
                pbits(&a.sub(&b).reversed()),
                "{what}"
            );
            let (m, n) = (case % 4, (case / 4) % 4);
            let (f, _) = tensor::<S>(&mut rng, m, n, case % 2 == 0, false);
            let (g, _) = tensor::<S>(&mut rng, (case / 2) % 3, (case / 5) % 3, true, false);
            let h = f.mul(&g).expect("in range");
            for dir in [Dir::U, Dir::V] {
                assert_eq!(
                    tbits(&f.reversed(dir).mul(&g.reversed(dir)).expect("in range")),
                    tbits(&h.reversed(dir)),
                    "{what} {dir:?}"
                );
                assert_eq!(
                    tbits(&f.reversed(dir).elevate(dir, 2).expect("in range")),
                    tbits(&f.elevate(dir, 2).expect("in range").reversed(dir)),
                    "{what} {dir:?}"
                );
                let other = if dir == Dir::U { Dir::V } else { Dir::U };
                assert_eq!(
                    tbits(&f.reversed(other).elevate(dir, 1).expect("in range")),
                    tbits(&f.elevate(dir, 1).expect("in range").reversed(other)),
                    "{what} {dir:?}"
                );
            }
            assert_eq!(tbits(&g.mul(&f).expect("in range")), tbits(&h), "{what}");
            // The numerators change sign with the direction, exactly (but
            // for the signs of zero parts).
            let (pm, pn) = (1 + case % 2, 1 + (case / 2) % 2);
            let patch = HomPatch::from_tensors(std::array::from_fn(|k| {
                tensor::<S>(&mut rng, pm, pn, case % 2 == 1, k == 3).0
            }))
            .expect("one pair of degrees");
            let nn = patch.normal_numerator().expect("in range");
            for dir in [Dir::U, Dir::V] {
                let nr = patch.reversed(dir).normal_numerator().expect("in range");
                for k in 0..3 {
                    assert_eq!(
                        value_and_errs(nr[k].coefs()),
                        value_and_errs(nn[k].reversed(dir).neg().coefs()),
                        "{what} normal {dir:?}"
                    );
                }
            }
            let cs: [Poly<S>; 4] = std::array::from_fn(|_| rng.poly::<S>(1 + case % 3, true).0);
            let [x, y, z, w] = cs;
            let curve = HomCurve::from_polys([x, y, z], w).expect("one degree");
            let tn = curve.tangent_numerator().expect("in range");
            let tr = curve.reversed().tangent_numerator().expect("in range");
            for k in 0..3 {
                assert_eq!(
                    value_and_errs(tr[k].coefs()),
                    value_and_errs(tn[k].reversed().neg().coefs()),
                    "{what} tangent"
                );
            }
            // Boundary lines: row 0 and m, column 0 and n.
            let (hm, hn) = h.degrees();
            let line = |t: &Tensor<S>, dir: Dir, k: usize| -> Poly<S> {
                let (tm, tn) = t.degrees();
                match dir {
                    Dir::V => Poly::new((0..=tn).map(|j| t.get(k, j).clone())),
                    Dir::U => Poly::new((0..=tm).map(|i| t.get(i, k).clone())),
                }
                .expect("degree in range")
            };
            let (fm, fn_) = f.degrees();
            let (gm, gn) = g.degrees();
            for (dir, hk, fk, gk) in [
                (Dir::V, 0, 0, 0),
                (Dir::V, hm, fm, gm),
                (Dir::U, 0, 0, 0),
                (Dir::U, hn, fn_, gn),
            ] {
                assert_eq!(
                    pbits(&line(&h, dir, hk)),
                    pbits(&line(&f, dir, fk).mul(&line(&g, dir, gk)).expect("in range")),
                    "{what} boundary {dir:?} {hk}"
                );
            }
        }
    }
}

#[test]
fn products_commute_with_reversal_and_swap() {
    mirror::<Bounded<f64>>();
    mirror::<Bounded<Dd>>();
    mirror::<Bounded<Big>>();
}

/// With big-float coefficients of mixed widths too: each quotient at its
/// numerator's width.
#[test]
fn products_of_mixed_widths_commute_with_reversal() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..40 {
            let mut big = |n: usize| {
                Poly::new((0..=n).map(|_| {
                    let (hi, lo) = rng.coef(0);
                    let w = Width::ALL[rng.below(3) as usize];
                    Bounded::exact(Big::from_dd(Dd::sum(hi, lo), w))
                }))
                .expect("degree in range")
            };
            let (a, b) = (big(1 + case % 6), big(case % 5));
            let p = a.mul(&b).expect("in range");
            assert_eq!(
                pbits(&a.reversed().mul(&b.reversed()).expect("in range")),
                pbits(&p.reversed())
            );
            assert_eq!(pbits(&b.mul(&a).expect("in range")), pbits(&p));
        }
    }
}

/// The numerators against their definitions on rationals: `W²·x'` from
/// the quotient rule, `W³·(x_u × x_v)` from the patch's partials.
#[test]
fn numerators_are_the_scaled_tangent_and_normal() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..20 {
            let (m, n) = (1 + case % 3, 1 + (case / 3) % 3);
            let pos = |rng: &mut Rng| Q::from(rng.below(64) + 1) / Q::from(8u32);
            let net: Vec<[Q; 4]> = (0..(m + 1) * (n + 1))
                .map(|_| {
                    let w = pos(&mut rng);
                    let [x, y, z] = [0, 1, 2].map(|_| Q::from(rng.below(201) as i64 - 100) * &w);
                    [x, y, z, w]
                })
                .collect();
            let patch = HomPatch::new(m, n, net).expect("degrees in range");
            let nn = patch.normal_numerator().expect("in range");
            for _ in 0..3 {
                let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
                let p = patch.eval_partials(&u, &v);
                let w = patch.weights().eval(&u, &v);
                let w3 = &w * &w * &w;
                let c = [
                    &p.du[1] * &p.dv[2] - &p.du[2] * &p.dv[1],
                    &p.du[2] * &p.dv[0] - &p.du[0] * &p.dv[2],
                    &p.du[0] * &p.dv[1] - &p.du[1] * &p.dv[0],
                ];
                for k in 0..3 {
                    assert_eq!(nn[k].eval(&u, &v), &w3 * &c[k], "seed {seed} case {case}");
                }
            }
            let d = 1 + case % 4;
            let curve = HomCurve::new((0..=d).map(|_| {
                let w = pos(&mut rng);
                (
                    [0, 1, 2].map(|_| Q::from(rng.below(201) as i64 - 100) * &w),
                    w,
                )
            }))
            .expect("degree in range");
            let tn = curve.tangent_numerator().expect("in range");
            for _ in 0..3 {
                let t = dyadic(&mut rng);
                let (p, w) = curve.eval_hom(&t);
                let (dp, dw) = curve.derivative().eval_hom(&t);
                for k in 0..3 {
                    // x' = (P'·W − P·W') / W².
                    assert_eq!(tn[k].eval(&t), &dp[k] * &w - &p[k] * &dw);
                }
            }
        }
    }
}

/// The vector products against their components, on rationals.
#[test]
fn cross_and_dot_of_forms() {
    let mut rng = Rng::new(11);
    for case in 0..30 {
        let a: [Poly<Q>; 3] = std::array::from_fn(|k| ints(&mut rng, (case + k) % 4));
        let b: [Poly<Q>; 3] = std::array::from_fn(|k| ints(&mut rng, (case * k) % 3));
        let c = cross(&a, &b).expect("in range");
        let d = dot(&a, &b).expect("in range");
        let t = dyadic(&mut rng);
        let x = a.clone().map(|p| p.eval(&t));
        let y = b.clone().map(|p| p.eval(&t));
        assert_eq!(c[0].eval(&t), &x[1] * &y[2] - &x[2] * &y[1]);
        assert_eq!(c[1].eval(&t), &x[2] * &y[0] - &x[0] * &y[2]);
        assert_eq!(c[2].eval(&t), &x[0] * &y[1] - &x[1] * &y[0]);
        assert_eq!(d.eval(&t), &x[0] * &y[0] + &x[1] * &y[1] + &x[2] * &y[2]);
    }
}

/// Operands at the edges: degree 0 and a result of degree exactly
/// [`MAX_DEGREE`], zero polynomials, alternating signs that cancel,
/// coefficients near the overflow edge (uncertified, never wrong),
/// subnormal ones and products that underflow, weights above `2⁵³`.
/// Every certified coefficient holds the exact product and difference,
/// and the swap gives the same bits.
fn edges<S: Level>() {
    let lift = |c: &[f64]| Poly::new(c.iter().map(|&x| S::of(x, 0.0))).expect("degree in range");
    let exact = |c: &[f64]| Poly::new(c.iter().map(|&x| q(x))).expect("degree in range");
    let tiny = f64::from_bits(1);
    let cases: [(Vec<f64>, Vec<f64>); 13] = [
        (vec![1.0; 33], vec![1.0; 32]),
        (vec![1.0; 65], vec![2.0]),
        (vec![3.0], vec![-1.5; 65]),
        (vec![0.0; 5], vec![1.0, 2.0]),
        (vec![f64::MAX, -f64::MAX, f64::MAX], vec![2.0, 1.0]),
        (vec![1e300; 33], vec![1e300; 33]),
        (vec![1e154; 33], vec![1e154; 33]),
        (vec![tiny, 3.0 * tiny, -7.0 * tiny], vec![0.5, 0.25, 3.0]),
        (vec![1e-160; 30], vec![1e-160; 35]),
        (vec![1e-300; 33], vec![1e-20; 32]),
        (vec![1.0, -1.0, 1.0, -1.0, 1.0], vec![1.0; 4]),
        (
            vec![1.0 + f64::EPSILON, 1.0, -1.0],
            vec![1.0, -1.0 - f64::EPSILON, 1.0],
        ),
        (vec![4_503_599_627_370_497.0; 40], vec![3.0; 25]),
    ];
    let mut stats = [0u32; 2];
    for (k, (a, b)) in cases.iter().enumerate() {
        let what = format!("case {k}");
        let p = lift(a).mul(&lift(b)).expect("in range");
        poly_holds(
            &what,
            &p,
            &exact(a).mul(&exact(b)).expect("in range"),
            &mut stats,
        );
        poly_holds(
            &what,
            &lift(a).sub(&lift(b)),
            &exact(a).sub(&exact(b)),
            &mut stats,
        );
        assert_eq!(
            pbits(&lift(b).mul(&lift(a)).expect("in range")),
            pbits(&p),
            "{what}"
        );
    }
    // At degree 64 the weights pass `2⁵³`: ones times ones, certified.
    let p = lift(&[1.0; 33])
        .mul(&lift(&[1.0; 33]))
        .expect("at the limit");
    for c in p.coefs() {
        assert!(holds("ones", c, &Q::from(1u32)), "{} {c:?}", S::NAME);
    }
}

#[test]
fn edge_operands_hold_their_bounds() {
    edges::<Bounded<f64>>();
    edges::<Bounded<Dd>>();
    edges::<Bounded<Big>>();
}

/// The numerators' degrees at the limit: a span of degree 32 has a
/// tangent numerator of degree 63, one of 33 none; a patch of degrees
/// `(21, 1)` a normal numerator of `(62, 2)`, one of `(22, 1)` none; a
/// constant span or a patch of degree 0 in `u` numerators of degree 0
/// there.
#[test]
fn numerator_degrees_at_the_limit() {
    let one = Q::from(1u32);
    let span = |n: u32| {
        HomCurve::<Q, 2>::new((0..=n).map(|i| ([Q::from(i), one.clone()], one.clone())))
            .expect("degree in range")
    };
    assert_eq!(
        span(32).tangent_numerator().expect("in range")[0].degree(),
        63
    );
    assert!(span(33).tangent_numerator().is_none());
    assert_eq!(
        span(0).tangent_numerator().expect("in range")[0].degree(),
        0
    );
    let patch = |m: usize, n: usize| {
        let net = (0..(m + 1) * (n + 1)).map(|i| {
            let i = u32::try_from(i).expect("small");
            [Q::from(i), one.clone(), Q::from(i * i), one.clone()]
        });
        HomPatch::new(m, n, net).expect("degrees in range")
    };
    let nn = patch(21, 1).normal_numerator().expect("in range");
    assert_eq!(nn[0].degrees(), (62, 2));
    assert!(patch(22, 1).normal_numerator().is_none());
    let nn = patch(0, 2).normal_numerator().expect("in range");
    assert_eq!(nn[0].degrees(), (0, 5));
}

/// A biquadratic patch with a pole (row 0 one point, its weights `1/64`,
/// `64`, `3`): the exact normal numerator is zero along that side; every
/// certified coefficient holds the exact one.
fn pole<S: Level>() {
    let w = [1.0 / 64.0, 64.0, 3.0, 0.5, 2.0, 7.0, 1.0, 1.0, 1.0];
    let x = [
        [1.0, 2.0, 3.0],
        [1.0, 2.0, 3.0],
        [1.0, 2.0, 3.0],
        [0.0, 1.0, 5.0],
        [4.0, -1.0, 2.0],
        [0.375, 0.75, -2.0],
        [1.0, 1.5, 0.25],
        [9.0, 2.0, 1.0],
        [3.0, 3.0, 3.0],
    ];
    let point = |k: usize| [x[k][0] * w[k], x[k][1] * w[k], x[k][2] * w[k], w[k]];
    let net = (0..9).map(|k| point(k).map(|c| S::of(c, 0.0)));
    let patch = HomPatch::new(2, 2, net).expect("degrees in range");
    let truth = HomPatch::new(2, 2, (0..9).map(|k| point(k).map(q))).expect("degrees in range");
    let nn = patch.normal_numerator().expect("in range");
    let nq = truth.normal_numerator().expect("in range");
    let mut stats = [0u32; 2];
    for k in 0..3 {
        for j in 0..=5 {
            assert_eq!(*nq[k].get(0, j), 0u32, "the pole's side");
        }
        tensor_holds(S::NAME, &nn[k], &nq[k], &mut stats);
    }
    assert_eq!(stats[0], stats[1], "{}: certified {stats:?}", S::NAME);
}

#[test]
fn normal_numerator_at_a_pole() {
    pole::<Bounded<f64>>();
    pole::<Bounded<Dd>>();
    pole::<Bounded<Big>>();
}

/// With big-float coefficients of all widths: tensor products commute
/// with reversal and swap to the bit, and the numerators of the reversed
/// patch and span are the reversed ones negated.
#[test]
fn numerators_of_mixed_widths_commute_with_reversal() {
    for seed in varde_testing::seeds(16, 64) {
        let mut rng = Rng::new(seed);
        let mut big = || {
            let (hi, lo) = rng.coef(0);
            let w = Width::ALL[rng.below(Width::ALL.len() as u64) as usize];
            Bounded::exact(Big::from_dd(Dd::sum(hi, lo), w))
        };
        let what = format!("seed {seed}");
        let degs: [usize; 7] = std::array::from_fn(|k| (seed as usize >> (2 * k)) % 4);
        let mut tensor = |m: usize, n: usize| {
            Tensor::new(m, n, (0..(m + 1) * (n + 1)).map(|_| big())).expect("degrees in range")
        };
        let (f, g) = (tensor(degs[0], degs[1]), tensor(degs[2], degs[3]));
        let h = f.mul(&g).expect("in range");
        assert_eq!(tbits(&g.mul(&f).expect("in range")), tbits(&h), "{what}");
        for dir in [Dir::U, Dir::V] {
            assert_eq!(
                tbits(&f.reversed(dir).mul(&g.reversed(dir)).expect("in range")),
                tbits(&h.reversed(dir)),
                "{what} {dir:?}"
            );
        }
        let (pm, pn) = (1 + degs[4] % 2, 1 + degs[5] % 2);
        let parts: [Tensor<_>; 4] = std::array::from_fn(|_| tensor(pm, pn));
        let patch = HomPatch::from_tensors(parts).expect("one pair of degrees");
        let nn = patch.normal_numerator().expect("in range");
        for dir in [Dir::U, Dir::V] {
            let nr = patch.reversed(dir).normal_numerator().expect("in range");
            for k in 0..3 {
                assert_eq!(
                    value_and_errs(nr[k].coefs()),
                    value_and_errs(nn[k].reversed(dir).neg().coefs()),
                    "{what} normal {dir:?}"
                );
            }
        }
        let d = degs[6];
        let polys: [Poly<_>; 4] =
            std::array::from_fn(|_| Poly::new((0..=d).map(|_| big())).expect("degree in range"));
        let [x, y, z, w] = polys;
        let curve = HomCurve::from_polys([x, y, z], w).expect("one degree");
        let tn = curve.tangent_numerator().expect("in range");
        let tr = curve.reversed().tangent_numerator().expect("in range");
        for k in 0..3 {
            assert_eq!(
                value_and_errs(tr[k].coefs()),
                value_and_errs(tn[k].reversed().neg().coefs()),
                "{what} tangent"
            );
        }
    }
}
