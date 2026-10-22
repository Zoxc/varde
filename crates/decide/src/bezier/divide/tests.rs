//! Divisions by linear factors and degree reduction: on rationals exact,
//! at every level each certified coefficient holds the exact quotient or
//! reduced form, the dropped lines of a division are not read, and both
//! commute with reversal (and the division with transposition) to the
//! bit.

use malachite::Rational as Q;

use super::*;
use crate::bezier::tests::{
    Level, Rng, at_level, int_tensor, ints, pbits, poly_holds, tbits, tensor, tensor_holds,
};
use crate::num::bigf::Big;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

/// Every subset of the four sides.
fn subsets() -> impl Iterator<Item = Sides> {
    (0u8..16).map(|bits| {
        Side::ALL
            .into_iter()
            .enumerate()
            .filter(|&(k, _)| bits >> k & 1 == 1)
            .map(|(_, s)| s)
            .collect()
    })
}

/// The linear factor of `side`: `u`, `1 − u`, `v` or `1 − v`.
fn factor(side: Side) -> Tensor<Q> {
    let (a, b) = if side.at_one() { (1u32, 0u32) } else { (0, 1) };
    let c = [Q::from(a), Q::from(b)];
    match side.dir() {
        Dir::U => Tensor::new(1, 0, c),
        Dir::V => Tensor::new(0, 1, c),
    }
    .expect("degree 1")
}

/// `g` times the linear factors of `sides`.
fn times_factors(g: &Tensor<Q>, sides: Sides) -> Tensor<Q> {
    sides
        .iter()
        .fold(g.clone(), |f, s| f.mul(&factor(s)).expect("in range"))
}

#[test]
fn quotients_on_rationals_are_exact() {
    let mut rng = Rng::new(3);
    for k in 0..7 {
        let q = ints(&mut rng, k);
        let t = Poly::new([Q::from(0u32), Q::from(1u32)]).expect("degree 1");
        let s = t.reversed();
        for (a, b) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut p = q.clone();
            if a {
                p = p.mul(&t).expect("in range");
            }
            if b {
                p = p.mul(&s).expect("in range");
            }
            assert_eq!(p.divide_ends(a, b), Some(q.clone()), "{k} {a} {b}");
        }
    }
    let c = Poly::new([Q::from(0u32)]).expect("degree 0");
    assert!(c.divide_ends(true, false).is_none());
    assert_eq!(c.divide_ends(false, false), Some(c.clone()));
    let line = Poly::new([Q::from(0u32), Q::from(0u32)]).expect("degree 1");
    assert!(line.divide_ends(true, true).is_none());

    for case in 0..40 {
        let (mk, nk) = (case % 4, (case / 4) % 4);
        let g = int_tensor(&mut rng, mk, nk);
        for sides in subsets() {
            let f = times_factors(&g, sides);
            for s in sides.iter() {
                assert!(f.side(s).coefs().iter().all(|c| *c == 0u32), "{case}");
            }
            assert_eq!(f.divide_sides(sides), Some(g.clone()), "{case} {sides:?}");
        }
    }
    let (m, n) = (1, 3);
    let f = int_tensor(&mut rng, m, n);
    let both_u = Sides::from(Side::U0).with(Side::U1);
    assert!(f.divide_sides(both_u).is_none());
    assert!(f.transposed().divide_sides(both_u.transposed()).is_none());
    assert_eq!(
        f.divide_sides(Side::V0.into())
            .expect("degree 3 in v")
            .degrees(),
        (1, 2)
    );
}

/// At a level: the coefficients of `f = g·factors` within their bounds
/// (the rounding of `g`'s true product), the dropped lines anything.
fn quotients_hold<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..24 {
            let what = format!("seed {seed} case {case}");
            let (mk, nk) = (case % 4, (case / 4) % 3);
            let (_, gq) = tensor::<S>(&mut rng, mk, nk, false, false);
            let sides = subsets()
                .nth(rng.below(16) as usize)
                .expect("sixteen subsets");
            let fq = times_factors(&gq, sides);
            let (m, n) = fq.degrees();
            let f = Tensor::new(m, n, fq.coefs().iter().map(at_level::<S>)).expect("same degrees");
            let quotient = f.divide_sides(sides).expect("degrees in range");
            tensor_holds(&what, &quotient, &gq, &mut stats);
            // The dropped lines are not read: in floats they are only
            // near zero.
            let (junk, _) = tensor::<S>(&mut rng, m, n, true, false);
            let (fu, fv) = (sides.factors(Dir::U), sides.factors(Dir::V));
            let mixed = (0..=m)
                .flat_map(|i| (0..=n).map(move |j| (i, j)))
                .map(|(i, j)| {
                    let kept = i >= fu.0 && i + fu.1 <= m && j >= fv.0 && j + fv.1 <= n;
                    if kept { f.get(i, j) } else { junk.get(i, j) }.clone()
                });
            let mixed = Tensor::new(m, n, mixed).expect("same degrees");
            assert_eq!(
                tbits(&mixed.divide_sides(sides).expect("degrees in range")),
                tbits(&quotient),
                "{what}"
            );
            // The 1D division on a line of `[0, 1]`.
            let p = f.side(Side::U0);
            let pq = fq.side(Side::U0);
            let (a, b) = (sides.contains(Side::V0), sides.contains(Side::V1));
            poly_holds(
                &what,
                &p.divide_ends(a, b).expect("degree in range"),
                &pq.divide_ends(a, b).expect("degree in range"),
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
fn quotients_hold_their_bounds_f64() {
    quotients_hold::<Bounded<f64>>(varde_testing::seeds(1, 16));
}

#[test]
fn quotients_hold_their_bounds_dd() {
    quotients_hold::<Bounded<Dd>>(varde_testing::seeds(1, 16));
}

#[test]
fn quotients_hold_their_bounds_big() {
    quotients_hold::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// Reversal and transposition give the same bits; a kept boundary line
/// is the 1D quotient of the form's.
fn quotients_commute<S: Level>() {
    let mut rng = Rng::new(11);
    for case in 0..48 {
        let what = format!("case {case}");
        let (m, n) = (2 + case % 4, 2 + (case / 4) % 3);
        let (f, _) = tensor::<S>(&mut rng, m, n, case % 2 == 0, false);
        for sides in subsets() {
            let q = f.divide_sides(sides).expect("degrees at least 2");
            for dir in [Dir::U, Dir::V] {
                let r = f
                    .reversed(dir)
                    .divide_sides(sides.reversed(dir))
                    .expect("degrees at least 2");
                assert_eq!(
                    tbits(&r),
                    tbits(&q.reversed(dir)),
                    "{what} {sides:?} {dir:?}"
                );
            }
            let t = f
                .transposed()
                .divide_sides(sides.transposed())
                .expect("degrees at least 2");
            assert_eq!(tbits(&t), tbits(&q.transposed()), "{what} {sides:?}");
            for s in Side::ALL.into_iter().filter(|&s| !sides.contains(s)) {
                let (a, b) = match s.dir() {
                    Dir::U => (sides.contains(Side::V0), sides.contains(Side::V1)),
                    Dir::V => (sides.contains(Side::U0), sides.contains(Side::U1)),
                };
                let line = f.side(s).divide_ends(a, b).expect("degree at least 2");
                assert_eq!(pbits(&q.side(s)), pbits(&line), "{what} {sides:?} {s:?}");
            }
        }
        let (p, _) = crate::bezier::tests::poly::<S>(&mut rng, m, true);
        for (a, b) in [(true, false), (false, true), (true, true)] {
            assert_eq!(
                pbits(&p.reversed().divide_ends(b, a).expect("degree at least 2")),
                pbits(&p.divide_ends(a, b).expect("degree at least 2").reversed()),
                "{what}"
            );
        }
    }
}

#[test]
fn quotients_commute_with_reversal_and_transposition() {
    quotients_commute::<Bounded<f64>>();
    quotients_commute::<Bounded<Dd>>();
    quotients_commute::<Bounded<Big>>();
}

#[test]
fn reduction_on_rationals_is_exact() {
    let mut rng = Rng::new(5);
    for d in 0..9 {
        let c = ints(&mut rng, d);
        let b = c.elevate(1).expect("in range");
        assert_eq!(b.reduced(), Some(c.clone()), "{d}");
        // Not an elevation: the `d`-th difference of a unit vector is
        // never zero.
        for k in 0..=d + 1 {
            let mut off: Vec<Q> = b.coefs().to_vec();
            off[k] += Q::from(1u32);
            assert!(
                Poly::new(off).expect("degree in range").reduced().is_none(),
                "{d} {k}"
            );
        }
    }
    assert!(ints(&mut rng, 0).reduced().is_none());
    for case in 0..24 {
        let (m, n) = (case % 4, (case / 4) % 3);
        let g = int_tensor(&mut rng, m, n);
        let up = g.elevate(Dir::U, 1).expect("in range");
        assert_eq!(up.reduced(Dir::U), Some(g.clone()), "{case}");
        let up = g.elevate(Dir::V, 1).expect("in range");
        assert_eq!(up.reduced(Dir::V), Some(g.clone()), "{case}");
        let both = g.elevate_to(m + 1, n + 1).expect("in range");
        let back = both.reduced(Dir::U).and_then(|f| f.reduced(Dir::V));
        assert_eq!(back, Some(g.clone()), "{case}");
    }
    assert!(int_tensor(&mut rng, 0, 2).reduced(Dir::U).is_none());
}

/// At a level: the reduced form of an elevation's coefficients within
/// their bounds holds the true one; reversal gives the same bits.
fn reduction_holds<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..24 {
            let what = format!("seed {seed} case {case}");
            let (m, n) = (case % 6, (case / 6) % 4);
            let (_, cq) = tensor::<S>(&mut rng, m, n, false, false);
            let bq = cq.elevate_to(m + 1, n + 1).expect("in range");
            let b = Tensor::new(m + 1, n + 1, bq.coefs().iter().map(at_level::<S>))
                .expect("same degrees");
            let r = b
                .reduced(Dir::U)
                .and_then(|f| f.reduced(Dir::V))
                .expect("an elevation in truth");
            tensor_holds(&what, &r, &cq, &mut stats);
            for dir in [Dir::U, Dir::V] {
                let one = b.reduced(dir).expect("an elevation in truth");
                let rev = b.reversed(dir).reduced(dir).expect("an elevation in truth");
                assert_eq!(tbits(&rev), tbits(&one.reversed(dir)), "{what} {dir:?}");
            }
            let line = b.side(Side::U0);
            poly_holds(
                &what,
                &line.reduced().expect("an elevation in truth"),
                &cq.side(Side::U0),
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
fn reduction_holds_its_bounds_f64() {
    reduction_holds::<Bounded<f64>>(varde_testing::seeds(1, 16));
}

#[test]
fn reduction_holds_its_bounds_dd() {
    reduction_holds::<Bounded<Dd>>(varde_testing::seeds(1, 16));
}

#[test]
fn reduction_holds_its_bounds_big() {
    reduction_holds::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

#[test]
fn side_sets() {
    let s = Sides::from(Side::U0).with(Side::V1);
    assert!(s.contains(Side::U0) && s.contains(Side::V1) && !s.contains(Side::U1));
    assert_eq!(s.iter().collect::<Vec<_>>(), [Side::U0, Side::V1]);
    assert_eq!(s.reversed(Dir::U), Sides::from(Side::U1).with(Side::V1));
    assert_eq!(s.transposed(), Sides::from(Side::V0).with(Side::U1));
    assert!(Sides::NONE.is_empty() && !s.is_empty());
    assert_eq!(subsets().count(), 16);
}
