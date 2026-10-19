//! Root isolation against polynomials with known roots, built exactly in
//! rationals as products of linear factors and taken to each level with
//! their rounding as the error: every root lies in exactly one result; a
//! [`Root`] holds exactly one, simple, crossing as `dir` says, inside
//! `near`; a [`Cluster`] bounds the multiplicity it holds and its end
//! signs are the true ones; results are ordered and apart. Simple roots
//! apart isolate, at split points too; tangential and end roots are
//! clusters; the reversed polynomial gives the mirrored result.

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Nearest};
use malachite::{Float, base::num::arithmetic::traits::PowerOf2};

use super::*;
use crate::bezier::tests::{Level, Rng, q, qabs};
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::{Bounded, ETA};
use crate::num::dd::Dd;

const DEEP: Limits = Limits {
    max_depth: 60,
    max_splits: 100_000,
};

fn dq(d: Dyadic) -> Q {
    Q::from(d.num()) >> d.exp()
}

/// `scale·Π (t − r)^m` in Bernstein form, exactly.
fn product(roots: &[(Q, u32)], scale: &Q) -> Poly<Q> {
    let mut p = Poly::new([scale.clone()]).expect("degree 0");
    for (r, m) in roots {
        let f = Poly::new([-r.clone(), Q::from(1u32) - r]).expect("degree 1");
        for _ in 0..*m {
            p = p.mul(&f).expect("degree in range");
        }
    }
    p
}

/// The rational `x` at a level: its nearest value there (an `f64`, or
/// two), the rest as the rounding error.
fn at_level<S: Level>(x: &Q) -> S {
    let hi = f64::rounding_from(x, Nearest).0;
    let lo = f64::rounding_from(&(x - q(hi)), Nearest).0;
    let v = S::of(hi, lo);
    let rest = qabs(x - v.val());
    if rest == 0u32 {
        v
    } else {
        v.with_errs(0.0, f64::rounding_from(&rest, Ceiling).0)
    }
}

fn level<S: Level>(p: &Poly<Q>) -> Poly<S> {
    Poly::new(p.coefs().iter().map(at_level::<S>)).expect("degree in range")
}

fn sign_of(x: &Q) -> Option<Ordering> {
    x.partial_cmp(&0u32).filter(|o| *o != Ordering::Equal)
}

/// Checks `found` against the true polynomial and its roots.
fn check(what: &str, r: &Roots, truth: &Poly<Q>, roots: &[(Q, u32)]) {
    let deriv = truth.derivative();
    for w in r.found.windows(2) {
        assert!(
            w[0].span()[1] <= w[1].span()[0],
            "{what}: out of order {w:?}"
        );
    }
    for f in &r.found {
        let [lo, hi] = f.span();
        assert!(lo < hi, "{what}: empty {f:?}");
        let inside = |x: &Q| match f {
            Found::Root(_) => *x > dq(lo) && *x < dq(hi),
            Found::Cluster(_) => *x >= dq(lo) && *x <= dq(hi),
        };
        let held: Vec<&(Q, u32)> = roots.iter().filter(|(x, _)| inside(x)).collect();
        let (ps, pe) = (sign_of(&truth.eval(&dq(lo))), sign_of(&truth.eval(&dq(hi))));
        match f {
            Found::Root(root) => {
                assert!(
                    held.len() == 1 && held[0].1 == 1,
                    "{what}: {f:?} holds {held:?}"
                );
                let x = &held[0].0;
                assert_eq!(sign_of(&deriv.eval(x)), Some(root.dir), "{what}: {f:?}");
                assert!(
                    ps.is_some() && pe == Some(root.dir) && ps != pe,
                    "{what}: {f:?}"
                );
                assert!(
                    q(root.near[0]) <= *x && *x <= q(root.near[1]),
                    "{what}: near {f:?}"
                );
                assert!(
                    q(lo.bounds()[0]) <= q(root.near[0]) && q(root.near[1]) <= q(hi.bounds()[1]),
                    "{what}: near {f:?}"
                );
            }
            Found::Cluster(c) => {
                let m: u32 = held.iter().map(|(_, m)| m).sum();
                assert!(m <= c.mult, "{what}: {f:?} holds {held:?}");
                assert!(c.start.is_some() || lo == Dyadic::ZERO, "{what}: {f:?}");
                assert!(c.end.is_some() || hi == Dyadic::ONE, "{what}: {f:?}");
                assert!(c.start.is_none() || c.start == ps, "{what}: {f:?}");
                assert!(c.end.is_none() || c.end == pe, "{what}: {f:?}");
            }
        }
    }
    for (x, _) in roots {
        if *x < 0u32 || *x > 1u32 {
            continue;
        }
        let n = r
            .found
            .iter()
            .filter(|f| {
                let [lo, hi] = f.span();
                dq(lo) <= *x && *x <= dq(hi)
            })
            .count();
        assert_eq!(n, 1, "{what}: root {x} in {n} results: {r:?}");
    }
}

fn roots_of(rs: &[(f64, u32)]) -> Vec<(Q, u32)> {
    rs.iter().map(|&(x, m)| (q(x), m)).collect()
}

fn run<S: Level>(rs: &[(Q, u32)], scale: &Q, limits: Limits) -> Roots {
    let truth = product(rs, scale);
    let r = isolate(&level::<S>(&truth), limits).expect("within the work cap");
    check(S::NAME, &r, &truth, rs);
    r
}

fn kinds(r: &Roots) -> String {
    r.found
        .iter()
        .map(|f| match f {
            Found::Root(_) => 'r',
            Found::Cluster(_) => 'c',
        })
        .collect()
}

fn every_level(rs: &[(Q, u32)], scale: &Q, limits: Limits) -> [Roots; 4] {
    let truth = product(rs, scale);
    let r = isolate(&truth, limits).expect("within the work cap");
    check("rational", &r, &truth, rs);
    [
        run::<Bounded<f64>>(rs, scale, limits),
        run::<Bounded<Dd>>(rs, scale, limits),
        run::<Bounded<Big>>(rs, scale, limits),
        r,
    ]
}

#[test]
fn simple_roots_isolate_at_split_points_too() {
    // ½, ¼ and ¾ are split points, the others aren't.
    let rs = roots_of(&[(0.1, 1), (0.25, 1), (0.5, 1), (0.6, 1), (0.75, 1)]);
    for r in every_level(&rs, &q(3.0), DEEP) {
        assert_eq!(kinds(&r), "rrrrr", "{r:?}");
        let Found::Root(mid) = r.found[2] else {
            unreachable!()
        };
        // Once, by the two intervals beside ½.
        assert!(mid.lo < Dyadic::new(1, 1).unwrap() && Dyadic::new(1, 1).unwrap() < mid.hi);
    }
}

#[test]
fn krawczyk_narrows_an_isolated_root() {
    let rs = roots_of(&[(0.3, 1)]);
    for r in &every_level(&rs, &q(1.0), DEEP)[..2] {
        let Found::Root(root) = r.found[0] else {
            panic!("{r:?}")
        };
        assert!(root.near[1] - root.near[0] < 1e-9, "{root:?}");
    }
}

#[test]
fn tangential_roots_are_clusters() {
    // A double root at ⅓ (no sign change) and a triple one at 0.7 (a
    // sign change), between simple roots.
    let rs = vec![
        (q(0.125), 1),
        (Q::from(1u32) / Q::from(3u32), 2),
        (q(0.7), 3),
        (q(0.9), 1),
    ];
    for r in &every_level(&rs, &q(1.0), DEEP) {
        assert_eq!(kinds(r), "rccr", "{r:?}");
        let (Found::Cluster(two), Found::Cluster(three)) = (r.found[1], r.found[2]) else {
            unreachable!()
        };
        assert_eq!(two.start, two.end);
        assert_ne!(three.start, three.end);
        assert!(two.mult >= 2 && three.mult >= 3);
    }
}

#[test]
fn a_double_root_on_a_split_point_is_one_cluster() {
    let rs = roots_of(&[(0.5, 2), (0.25, 1)]);
    for r in every_level(&rs, &q(1.0), DEEP) {
        assert_eq!(kinds(&r), "rc", "{r:?}");
    }
}

#[test]
fn roots_at_the_ends_are_clusters() {
    let rs = roots_of(&[(0.0, 1), (0.5, 1), (1.0, 1)]);
    for r in every_level(&rs, &q(2.0), DEEP) {
        assert_eq!(kinds(&r), "crc", "{r:?}");
        let (Found::Cluster(a), Found::Cluster(b)) = (r.found[0], r.found[2]) else {
            unreachable!()
        };
        assert_eq!((a.lo, a.start, a.mult), (Dyadic::ZERO, None, 1));
        assert_eq!((b.hi, b.end, b.mult), (Dyadic::ONE, None, 1));
    }
}

#[test]
fn close_roots_separate_with_depth_or_cluster_at_it() {
    let (a, b) = (q(0.4), q(0.4) + (Q::from(1u32) >> 40u32));
    let rs = vec![(a, 1), (b, 1)];
    let shallow = Limits {
        max_depth: 30,
        ..DEEP
    };
    for r in every_level(&rs, &q(1.0), shallow) {
        let [Found::Cluster(c)] = r.found[..] else {
            panic!("{r:?}")
        };
        assert!(c.at_depth && c.mult >= 2 && c.start == c.end, "{c:?}");
    }
    // The levels above `f64` hold the coefficients closely enough.
    for r in &every_level(&rs, &q(1.0), DEEP)[1..] {
        assert_eq!(kinds(r), "rr", "{r:?}");
    }
}

#[test]
fn far_from_zero_has_no_roots_and_zero_is_one_cluster() {
    let rs = roots_of(&[(-0.5, 2), (1.5, 1)]);
    for r in every_level(&rs, &q(1.0), DEEP) {
        assert!(r.found.is_empty(), "{r:?}");
        assert_eq!(r.work.splits, 0);
    }
    let zero = Poly::new((0..4).map(|_| Q::from(0u32))).expect("degree 3");
    let r = isolate(&zero, DEEP).expect("no splits");
    assert_eq!(
        r.found,
        vec![Found::Cluster(Cluster {
            lo: Dyadic::ZERO,
            hi: Dyadic::ONE,
            mult: 3,
            start: None,
            end: None,
            at_depth: false,
        })]
    );
}

#[test]
fn the_work_cap_stops_it() {
    let truth = product(&roots_of(&[(0.3, 1), (0.31, 1)]), &q(1.0));
    let p = level::<Bounded<f64>>(&truth);
    let all = isolate(&p, DEEP).expect("within the cap");
    assert!(all.work.splits > 2);
    let capped = Limits {
        max_splits: all.work.splits - 1,
        ..DEEP
    };
    let stop = isolate(&p, capped).expect_err("past the cap");
    assert_eq!(stop.work.splits, all.work.splits - 1);
    assert!(
        isolate(
            &p,
            Limits {
                max_splits: all.work.splits,
                ..DEEP
            }
        )
        .is_ok()
    );
}

/// The reversed polynomial's results, mirrored: the same intervals,
/// crossings and counts (`near` aside, which is narrowed from the left).
fn mirrored(r: &Roots) -> Vec<Found> {
    let flip = |d: Dyadic| {
        let k = d.exp();
        Dyadic::new((1u64 << k) - d.num(), k).expect("in [0, 1]")
    };
    r.found
        .iter()
        .rev()
        .map(|f| match *f {
            Found::Root(x) => Found::Root(Root {
                lo: flip(x.hi),
                hi: flip(x.lo),
                dir: x.dir.reverse(),
                near: [0.0, 0.0],
            }),
            Found::Cluster(c) => Found::Cluster(Cluster {
                lo: flip(c.hi),
                hi: flip(c.lo),
                start: c.end,
                end: c.start,
                ..c
            }),
        })
        .collect()
}

fn without_near(r: &Roots) -> Vec<Found> {
    r.found
        .iter()
        .map(|f| match *f {
            Found::Root(x) => Found::Root(Root {
                near: [0.0, 0.0],
                ..x
            }),
            c => c,
        })
        .collect()
}

fn mirror_case<S: Level>(truth: &Poly<Q>) {
    let p = level::<S>(truth);
    let (a, b) = (isolate(&p, DEEP), isolate(&p.reversed(), DEEP));
    let (Ok(a), Ok(b)) = (a, b) else {
        panic!("{}: work cap", S::NAME)
    };
    assert_eq!(mirrored(&a), without_near(&b), "{}", S::NAME);
    assert_eq!(a.work, b.work, "{}", S::NAME);
}

/// A random root set: dyadics (split points), thirds and sevenths,
/// multiplicities 1 to 3, some outside `[0, 1]`, some at its ends.
fn random_roots(rng: &mut Rng) -> Vec<(Q, u32)> {
    let k = 1 + rng.below(5) as usize;
    (0..k)
        .map(|_| {
            let x = match rng.below(4) {
                0 => Q::from(rng.below(17)) / Q::from(16u32),
                1 => Q::from(rng.below(4)) / Q::from(3u32),
                2 => Q::from(rng.below(10) as i64 - 1) / Q::from(7u32),
                _ => Q::from(rng.below(1001)) / Q::from(1000u32),
            };
            let m = match rng.below(6) {
                0 => 2,
                1 => 3,
                _ => 1,
            };
            (x, m)
        })
        .collect()
}

/// Merges equal roots, adding their multiplicities.
fn merged(mut rs: Vec<(Q, u32)>) -> Vec<(Q, u32)> {
    rs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out: Vec<(Q, u32)> = Vec::new();
    for (x, m) in rs {
        match out.last_mut() {
            Some(l) if l.0 == x => l.1 += m,
            _ => out.push((x, m)),
        }
    }
    out
}

#[test]
fn random_roots_hold_and_mirror() {
    let limits = Limits {
        max_depth: 40,
        max_splits: 100_000,
    };
    let mut stats = [0usize; 2];
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..40 {
            let rs = merged(random_roots(&mut rng));
            let scale = Q::from(rng.below(19) as i64 - 9) / Q::from(4u32);
            if scale == 0u32 {
                continue;
            }
            let what = format!("seed {seed} case {case}");
            let truth = product(&rs, &scale);
            let r = isolate(&truth, limits).expect("within the cap");
            check(&format!("{what} rational"), &r, &truth, &rs);
            let r = isolate(&level::<Bounded<f64>>(&truth), limits).expect("within the cap");
            check(&format!("{what} f64"), &r, &truth, &rs);
            for f in &r.found {
                stats[usize::from(matches!(f, Found::Cluster(_)))] += 1;
                // Simple roots inside `(0, 1)` are at least `1/7000`
                // apart: they isolate.
                if let Found::Cluster(c) = f {
                    let simple = |(x, m): &&(Q, u32)| *m == 1 && *x > 0u32 && *x < 1u32;
                    let held: Vec<_> = rs
                        .iter()
                        .filter(|(x, _)| *x >= dq(c.lo) && *x <= dq(c.hi))
                        .collect();
                    assert!(held.is_empty() || !held.iter().all(simple), "{what}: {c:?}");
                }
            }
            let r = isolate(&level::<Bounded<Dd>>(&truth), limits).expect("within the cap");
            check(&format!("{what} dd"), &r, &truth, &rs);
            if case % 4 == 0 {
                let r = isolate(&level::<Bounded<Big>>(&truth), limits).expect("within the cap");
                check(&format!("{what} big"), &r, &truth, &rs);
            }
            mirror_case::<Bounded<f64>>(&truth);
            mirror_case::<Bounded<Dd>>(&truth);
        }
    }
    // Not vacuous: roots and clusters both.
    assert!(stats[0] > 0 && stats[1] > 0, "{stats:?}");
}

#[test]
fn big_floats_below_the_f64_range_still_have_signs() {
    let big = |x: Float| Big::from_float(x, Width::MIN).v;
    // `1.5·η` within `η`: certainly positive, which its `f64` enclosure
    // (the value rounds to a float) can't tell.
    let x = Bounded::new(big(Float::from(3u32) >> 1075u32), 0.0, ETA);
    assert_eq!(Scalar::sign(&x), Some(Ordering::Greater));
    assert!(x.enclose()[0] <= 0.0);
    // Coefficients far into the subnormal range, their errors at its
    // floor: the same roots as unscaled.
    let tiny = Bounded::exact(big(Float::power_of_2(-1040i64)));
    let p = Poly::new(
        [2.0, -3.0, 1.0, 4.0, -2.0]
            .iter()
            .map(|&x| Bounded::exact(Big::from_f64(x, Width::MIN))),
    )
    .expect("degree 4");
    let low = Poly::new(p.coefs().iter().map(|c| c.mul(&tiny))).expect("degree 4");
    let (a, b) = (isolate(&p, DEEP).unwrap(), isolate(&low, DEEP).unwrap());
    assert_eq!(kinds(&b), "rrr");
    assert_eq!(without_near(&a), without_near(&b));
}

#[test]
fn signs_are_certified_at_every_level() {
    use Ordering::{Greater, Less};
    assert_eq!(Scalar::sign(&Bounded::new(1.0, 0.5, 0.25)), Some(Greater));
    assert_eq!(Scalar::sign(&Bounded::new(-1.0, 0.5, 0.5)), None);
    assert_eq!(Scalar::sign(&Bounded::exact(0.0)), None);
    let dd = Bounded::exact(Dd::sum(-1.0, 1e-20));
    assert_eq!(Scalar::sign(&dd), Some(Less));
    assert_eq!(Scalar::sign(&Q::from(0u32)), None);
    assert_eq!(Scalar::sign(&(Q::from(-1i32) >> 5000u32)), Some(Less));
    let like = Bounded::exact(Big::from_f64(1.0, Width::MIN));
    let x = <Bounded<Big> as Scalar>::from_f64(0.3, &like);
    assert_eq!(x.v.to_f64(), 0.3);
    assert_eq!(
        <Q as Scalar>::from_f64(0.75, &Q::from(0u32)),
        Q::from(3u32) >> 2u32
    );
}

#[test]
fn dyadics_are_in_lowest_terms_and_ordered() {
    assert_eq!(Dyadic::new(4, 3), Dyadic::new(1, 1));
    assert_eq!(Dyadic::new(0, 7), Some(Dyadic::ZERO));
    assert_eq!(Dyadic::new(8, 3), Some(Dyadic::ONE));
    assert_eq!(Dyadic::new(9, 3), None);
    assert_eq!(Dyadic::new(1, 64), None);
    let a = Dyadic::new(3, 2).unwrap();
    let b = Dyadic::new((1 << 62) + 1, 63).unwrap();
    assert!(b < a && Dyadic::new(1, 1).unwrap() < b);
    assert_eq!(a.bounds(), [0.75, 0.75]);
    let [lo, hi] = b.bounds();
    assert!(lo < hi && q(lo) <= dq(b) && dq(b) <= q(hi));
}

#[test]
fn wilkinson_like_roots_hold_and_clustered_ones_separate_exactly() {
    // Twenty evenly spaced roots, degree 20.
    let rs: Vec<(Q, u32)> = (1..=20u32)
        .map(|k| (Q::from(k) / Q::from(21u32), 1))
        .collect();
    let [.., exact] = every_level(&rs, &q(1.0), DEEP);
    assert_eq!(kinds(&exact), "r".repeat(20), "{exact:?}");
    // Four roots `2⁻²⁴` apart and one far off: exactly, five roots; at
    // each level, whatever it can separate, every root held once.
    let mut rs: Vec<(Q, u32)> = (0..4u32)
        .map(|i| (q(0.3) + (Q::from(i) >> 24u32), 1))
        .collect();
    rs.push((q(0.8), 1));
    let [.., exact] = every_level(&rs, &q(1.0), DEEP);
    assert_eq!(kinds(&exact), "rrrrr", "{exact:?}");
}

#[test]
fn brackets_below_the_normal_range_scale_outwards() {
    // A local bracket on a first leaf `[0, 2⁻ᵏ]` scaled into `t`: below
    // the normal range the scaling rounds, so it is taken outwards. Plain
    // scaling of `down(7η) = 6η` by ⅛ gives `0.75η`, rounded up to `η`,
    // above `7η/8`.
    for (k, a, b) in [
        (3, 7.0 * ETA, 8.0 * ETA),
        (5, 0.0, 3.0 * ETA),
        (1, 0.25, 0.5),
    ] {
        let [lo, hi] = to_t(k, 0, [a, b]);
        let s = Q::from(1u32) >> k;
        assert!(q(lo) <= q(a) * &s && q(b) * &s <= q(hi), "{k} {a} {b}");
        assert!(0.0 <= lo && hi <= pow2_neg(k), "{k} {a} {b}");
    }
}

#[test]
fn roots_finer_than_floats_have_near_within_the_bounds_of_their_ends() {
    // Roots `2⁻⁵³` apart at ½ separate at depth 54, where the ends
    // `(2⁵³ ± 1)/2⁵⁴` aren't floats: `near` reaches the floats around
    // them.
    let half = q(0.5);
    for rs in [
        vec![
            (half.clone(), 1),
            (half.clone() + (Q::from(1u32) >> 53u32), 1),
        ],
        vec![(half.clone() - (Q::from(1u32) >> 53u32), 1), (half, 1)],
    ] {
        let [.., exact] = every_level(&rs, &q(1.0), DEEP);
        assert_eq!(kinds(&exact), "rr", "{exact:?}");
    }
}

#[test]
fn narrowing_takes_the_same_steps_under_reversal() {
    // A root crossing downwards is narrowed on the mirrored interval, as
    // the reversed polynomial's crossing upwards: the same steps, and
    // `near` mirrored but for the rounding of `1 − t`.
    fn case<S: Level>(truth: &Poly<Q>) {
        let p = level::<S>(truth);
        let (a, b) = (
            isolate(&p, DEEP).unwrap(),
            isolate(&p.reversed(), DEEP).unwrap(),
        );
        assert_eq!(a.work, b.work, "{}", S::NAME);
        for (x, y) in a.found.iter().zip(b.found.iter().rev()) {
            let (Found::Root(x), Found::Root(y)) = (x, y) else {
                panic!("{}: {a:?}", S::NAME)
            };
            let one = Q::from(1u32);
            let width = |n: [f64; 2]| q(n[1]) - q(n[0]);
            let (mx, my) = (width(x.near), width(y.near));
            assert!(
                q(x.near[0]) <= one.clone() - q(y.near[1])
                    && one.clone() - q(y.near[0]) <= q(x.near[1])
                    || q(y.near[0]) <= one.clone() - q(x.near[1])
                        && one - q(x.near[0]) <= q(y.near[1]),
                "{}: {x:?} {y:?} {mx} {my}",
                S::NAME
            );
        }
    }
    let third = Q::from(1u32) / Q::from(3u32);
    for r in [
        q(0.3),
        third,
        Q::from(1u32) / Q::from(10u32),
        q(0.5),
        q(0.375),
    ] {
        let truth = product(&[(r, 1)], &q(1.0));
        let w = isolate(&truth, DEEP).unwrap().work;
        assert_eq!(w, isolate(&truth.reversed(), DEEP).unwrap().work);
        case::<Bounded<f64>>(&truth);
        case::<Bounded<Dd>>(&truth);
        case::<Bounded<Big>>(&truth);
    }
}

#[test]
fn the_work_cap_stops_it_the_same_way_reversed() {
    // Depth first from the left, the tests run before the cap depend on
    // the side the splits are on; the work reported doesn't.
    let truth = product(&roots_of(&[(0.1, 1), (0.7, 1), (0.71, 1)]), &q(1.0));
    let p = level::<Bounded<f64>>(&truth);
    let all = isolate(&p, DEEP).expect("within the cap");
    for max_splits in 0..all.work.splits {
        let capped = Limits { max_splits, ..DEEP };
        let a = isolate(&p, capped).expect_err("past the cap");
        let b = isolate(&p.reversed(), capped).expect_err("past the cap");
        assert_eq!(a, b, "{max_splits}");
    }
}

/// Exact coefficients: roots at the ends and on split points are points,
/// reported once, the intervals beside them apart; others isolate as
/// single roots, which the bounded levels' rule would join into clusters.
#[test]
fn exact_zeros_are_points() {
    let rs = [
        (q(0.0), 1),
        (q(0.25), 1),
        (Q::from(1u32) / Q::from(3u32), 1),
        (q(0.5), 1),
        (q(1.0), 1),
    ];
    let p = product(&rs, &Q::from(3u32));
    let r = isolate_exact(&p, DEEP).unwrap();
    let points: Vec<Q> = r
        .found
        .iter()
        .filter_map(|f| match f {
            Exact::Point(d) => Some(dq(*d)),
            _ => None,
        })
        .collect();
    assert_eq!(points, [q(0.0), q(0.25), q(0.5), q(1.0)]);
    let third = Q::from(1u32) / Q::from(3u32);
    let inner: Vec<_> = r
        .found
        .iter()
        .filter_map(|f| match f {
            Exact::Root { lo, hi, dir } => Some((dq(*lo), dq(*hi), *dir)),
            Exact::Cluster { .. } => panic!("{f:?}"),
            Exact::Point(_) => None,
        })
        .collect();
    assert_eq!(inner.len(), 1);
    let (lo, hi, dir) = &inner[0];
    assert!(*lo < third && third < *hi);
    // `p` falls through `⅓` (an odd number of roots above it in `(0, 1]`
    // and a positive scale): negative past it.
    assert_eq!(
        *dir,
        if p.eval(&hi.clone()) > 0u32 {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    );
    // In increasing order, every result apart.
    let key = |f: &Exact| match f {
        Exact::Point(d) => [*d, *d],
        Exact::Root { lo, hi, .. } | Exact::Cluster { lo, hi, .. } => [*lo, *hi],
    };
    for w in r.found.windows(2) {
        assert!(key(&w[0])[1] <= key(&w[1])[0], "{:?}", r.found);
    }
    // The bounded rule over the same rationals joins them at the zeros.
    let b = isolate(&p, DEEP).unwrap();
    assert!(b.found.iter().any(|f| matches!(f, Found::Cluster(_))));
}

/// Roots closer than the depth limit stay one cluster, with the number
/// of roots it may hold.
#[test]
fn exact_roots_closer_than_the_depth_cluster() {
    let a = Q::from(1u32) / Q::from(3u32);
    let b = &a + (Q::from(1u32) >> 70u64);
    let p = product(&[(a, 1), (b, 1)], &Q::from(1u32));
    let r = isolate_exact(&p, DEEP).unwrap();
    assert!(
        matches!(r.found.as_slice(), [Exact::Cluster { mult: 2, .. }]),
        "{:?}",
        r.found
    );
}

/// Narrowing to the caller's width: the root stays inside, each halving
/// counted; a caller error stops it; past the floats' resolution it
/// stops unsettled.
#[test]
fn narrowing_to_a_width_keeps_the_root() {
    let third = Q::from(1u32) / Q::from(3u32);
    let p = product(&[(third.clone(), 1), (q(0.75), 1)], &Q::from(1u32));
    let r = isolate(&level::<Bounded<f64>>(&p), DEEP).unwrap();
    let Found::Root(root) = r.found[0] else {
        panic!("{:?}", r.found)
    };
    let pl = level::<Bounded<f64>>(&p);
    let n = narrow(&pl, [0.0, 0.5], root.dir, |s| {
        Ok::<_, ()>(s[1] - s[0] > 1e-9)
    })
    .unwrap();
    assert!(n.settled && n.span[1] - n.span[0] <= 1e-9);
    assert!(q(n.span[0]) < third && third < q(n.span[1]));
    assert_eq!(n.halvings, 29);
    // Already narrow enough: no halving.
    let n0 = narrow(&pl, [0.0, 0.5], root.dir, |_| Ok::<_, ()>(false)).unwrap();
    assert_eq!((n0.halvings, n0.settled), (0, true));
    // The caller's error.
    let mut calls = 0;
    let e = narrow(&pl, [0.0, 0.5], root.dir, |_| {
        calls += 1;
        if calls > 3 { Err("stop") } else { Ok(true) }
    });
    assert_eq!(e, Err("stop"));
    // Never wide enough: stops where the floats run out, unsettled,
    // still holding the root.
    let n = narrow(&pl, [0.0, 0.5], root.dir, |_| Ok::<_, ()>(true)).unwrap();
    assert!(!n.settled && n.halvings <= ROOT_HALVINGS);
    assert!(q(n.span[0]) < third && third < q(n.span[1]));
}

/// A root on a split point is found by the run of the intervals beside
/// it, and narrowed as one inside an interval is, at every level (exact
/// coefficients too, where the split point's value is an exact zero):
/// from runs at least `2⁻³` wide to below `10⁻⁹`, as single intervals' roots.
#[test]
fn roots_on_split_points_are_narrowed() {
    let rs = roots_of(&[(0.125, 1), (0.375, 1), (0.5, 1), (0.6, 1), (0.75, 1)]);
    for r in every_level(&rs, &q(3.0), DEEP) {
        assert_eq!(kinds(&r), "rrrrr", "{r:?}");
        for f in &r.found {
            let Found::Root(root) = f else { unreachable!() };
            assert!(root.near[1] - root.near[0] < 1e-9, "{root:?}");
        }
    }
    // Roots `k/65` of a degree-12 polynomial, a split point among them.
    let rs: Vec<(Q, u32)> = [13u32, 20, 26, 32, 39]
        .iter()
        .map(|&k| (Q::from(k) / Q::from(65u32), 1))
        .chain([(q(0.5), 1)])
        .collect();
    let [.., exact] = every_level(&rs, &q(1.0), DEEP);
    for f in &exact.found {
        let Found::Root(root) = f else {
            panic!("{exact:?}")
        };
        assert!(root.near[1] - root.near[0] < 1e-9, "{root:?}");
    }
}
