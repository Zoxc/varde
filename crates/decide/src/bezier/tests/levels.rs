//! Every precision level of the schedule, each big-float width on its
//! own, against exact rationals at degrees 1 to 4: one computation over
//! curves, patches and their products, derivatives, splits and
//! compositions, written once over [`Scalar`] and run on rationals for
//! the truth and at each level; every certified result holds the truth
//! within its error. Then the ladder: signs of values a few `f64`s of
//! cancellation from zero are never wrong, each settles at the first
//! level whose error certifies it, and the escalation and its charges
//! are the same bits on every platform.

use std::cmp::Ordering;

use super::*;
use crate::bezier::{Dir, HomCurve, HomPatch, Implicit, Tensor};
use crate::num::level::{AtLevel, LevelScalar, ladder};
use crate::num::schedule::{LEVELS, Level as Lv, UNITS};

/// An input `a/b`: exact at every level when `b` is 1, else rounded by
/// the levels' division.
#[derive(Clone, Copy, Debug)]
struct Frac(f64, f64);

impl Frac {
    fn at<S: Scalar>(self, like: &S) -> S {
        let a = S::from_f64(self.0, like);
        if self.1 == 1.0 {
            a
        } else {
            a.div(&S::from_f64(self.1, like))
        }
    }
}

/// An implicit's degree, origin and terms (exponents, coefficient).
type Quartic = (usize, [Frac; 3], Vec<([usize; 3], Frac)>);

/// One case: degrees `(m, n)` in `1..=4`, a curve's and a patch's
/// coefficients, homogeneous points and weights, an implicit of degree
/// `1..=4`, and the parameters.
#[derive(Clone, Debug)]
struct Case {
    m: usize,
    n: usize,
    p: Vec<Frac>,
    p2: Vec<Frac>,
    f: Vec<Frac>,
    curve: Vec<([Frac; 3], Frac)>,
    net: Vec<([Frac; 3], Frac)>,
    implicit: Quartic,
    t: Frac,
    u: Frac,
    v: Frac,
}

impl Rng {
    /// A denominator: 1 (an exact input) or one the levels round by.
    fn denominator(&mut self) -> f64 {
        match self.below(4) {
            0 | 1 => 1.0,
            2 => [3.0, 5.0, 7.0, 11.0, 1023.0][self.below(5) as usize],
            _ => self.pow(-2, 2).abs(),
        }
    }

    /// A coefficient about `2^scale`, zero sometimes.
    fn frac(&mut self, scale: i32) -> Frac {
        if self.below(10) == 0 {
            return Frac(0.0, 1.0);
        }
        Frac(self.pow(scale - 8, scale), self.denominator())
    }

    /// A weight in `[1/64, 64]`.
    fn weight(&mut self) -> Frac {
        Frac(
            self.pow(-6, 5).abs(),
            [1.0, 3.0, 7.0][self.below(3) as usize],
        )
    }

    /// A parameter in `[0, 1]`: an end, ½, a dyadic, a float, or a
    /// rounded fraction `k/b`.
    fn param(&mut self) -> Frac {
        match self.below(6) {
            0..=2 => Frac(self.t(), 1.0),
            3 => Frac(1.0, 3.0),
            4 => Frac(2.0, 3.0),
            _ => {
                let b = 2.0 * self.below(500) as f64 + 3.0;
                Frac(self.below(b as u64 + 1) as f64, b)
            }
        }
    }

    /// A case at a scale picked as [`Rng::scale`] does, or a moderate
    /// one (`wide` false).
    fn case(&mut self, wide: bool) -> Case {
        let (m, n) = (1 + self.below(4) as usize, 1 + self.below(4) as usize);
        let scale = if wide {
            self.scale()
        } else {
            self.below(41) as i32 - 20
        };
        let fracs = |k: usize, r: &mut Rng| (0..k).map(|_| r.frac(scale)).collect::<Vec<_>>();
        let p = fracs(n + 1, self);
        let p2 = fracs(m + 1, self);
        let f = fracs((m + 1) * (n + 1), self);
        let point = |r: &mut Rng| ([r.frac(scale), r.frac(scale), r.frac(scale)], r.weight());
        let curve = (0..=n).map(|_| point(self)).collect();
        let net = (0..(m + 1) * (n + 1)).map(|_| point(self)).collect();
        let d = 1 + self.below(4) as usize;
        let origin = [self.frac(0), self.frac(0), self.frac(0)];
        let mut terms = Vec::new();
        for e0 in 0..=d {
            for e1 in 0..=d - e0 {
                for e2 in 0..=d - e0 - e1 {
                    if self.below(3) != 0 {
                        terms.push(([e0, e1, e2], self.frac(0)));
                    }
                }
            }
        }
        if terms.is_empty() {
            terms.push(([d, 0, 0], Frac(1.0, 1.0)));
        }
        Case {
            m,
            n,
            p,
            p2,
            f,
            curve,
            net,
            implicit: (d, origin, terms),
            t: self.param(),
            u: self.param(),
            v: self.param(),
        }
    }
}

/// The case's computation at `like`'s level (or exactly, on rationals):
/// a curve and a tensor evaluated, split at `t` and at ½, differentiated,
/// elevated and multiplied; a rational span's point and derivatives; a
/// rational patch's point, partials, second derivatives, normal
/// numerator and the implicit along it; the span in the implicit.
fn run<S: Scalar>(c: &Case, like: &S) -> Vec<S> {
    let x = |f: &Frac| f.at(like);
    let (t, u, v) = (x(&c.t), x(&c.u), x(&c.v));
    let mut out = Vec::new();
    let p = Poly::new(c.p.iter().map(x)).expect("degree in range");
    let p2 = Poly::new(c.p2.iter().map(x)).expect("degree in range");
    out.push(p.eval(&t));
    for half in p.split_at(&t).iter().chain(&p.split_half()) {
        out.extend_from_slice(half.coefs());
    }
    out.extend_from_slice(p.derivative().coefs());
    out.extend_from_slice(p.elevate(1).expect("in range").coefs());
    let pp = p.mul(&p2).expect("in range");
    out.extend_from_slice(pp.coefs());
    out.push(pp.eval(&t));
    let f = Tensor::new(c.m, c.n, c.f.iter().map(x)).expect("degrees in range");
    out.push(f.eval(&u, &v));
    out.extend_from_slice(f.derivative(Dir::U).coefs());
    for half in f.split_at(Dir::V, &t) {
        out.extend_from_slice(half.coefs());
    }
    let hom = |(p, w): &([Frac; 3], Frac)| {
        let w = x(w);
        (p.each_ref().map(|k| x(k).mul(&w)), w)
    };
    let curve = HomCurve::<S, 3>::new(c.curve.iter().map(hom)).expect("a span");
    out.extend(curve.eval(&t));
    for d in curve.derivatives_at(&t, 2) {
        out.extend(d);
    }
    let patch = HomPatch::new(
        c.m,
        c.n,
        c.net.iter().map(|pw| {
            let ([a, b, d], w) = hom(pw);
            [a, b, d, w]
        }),
    )
    .expect("a patch");
    out.extend(patch.eval(&u, &v));
    let partials = patch.eval_partials(&u, &v);
    out.extend(partials.p.into_iter().chain(partials.du).chain(partials.dv));
    let second = patch.derivatives_at(&u, &v, 2);
    for (a, b) in [(2, 0), (1, 1), (0, 2)] {
        out.extend(second.get(a, b).expect("order 2").clone());
    }
    let numerator = patch.normal_numerator().expect("in range");
    out.extend(numerator.iter().map(|n| n.eval(&u, &v)));
    let (d, origin, terms) = &c.implicit;
    let implicit = Implicit::new(
        *d,
        origin.each_ref().map(x),
        terms.iter().map(|(e, k)| (*e, x(k))),
    )
    .expect("an implicit");
    let along = patch.compose_implicit(&implicit).expect("in range");
    out.push(along.eval(&u, &v));
    out.extend_from_slice(&along.coefs()[..along.coefs().len().min(8)]);
    out.push(
        curve
            .compose_implicit(&implicit)
            .expect("in range")
            .eval(&t),
    );
    out
}

/// Results checked and certified, per level.
#[derive(Clone, Copy, Debug, Default)]
struct Tally {
    results: u64,
    certified: u64,
}

/// The case at `like`'s level against its truth: every certified result
/// holds it within its error (exactly) and in its enclosure. Each
/// result's error is returned for the comparison between levels.
fn check<S: Level + LevelScalar>(c: &Case, truth: &[Q], like: &S, tally: &mut Tally) -> Vec<f64> {
    let got = run(c, like);
    assert_eq!(got.len(), truth.len());
    let level = like.level();
    got.iter()
        .zip(truth)
        .enumerate()
        .map(|(i, (x, want))| {
            tally.results += 1;
            let what = format!("{level:?} result {i} of {c:?}");
            if holds(&what, x, want) {
                tally.certified += 1;
                let [lo, hi] = x.enclose();
                // Past the `f64` range an end is infinite.
                assert!(
                    (lo == f64::NEG_INFINITY || q(lo) <= *want)
                        && (hi == f64::INFINITY || *want <= q(hi)),
                    "{what}: enclosure"
                );
                let (t, r) = x.errs();
                t + r
            } else {
                f64::INFINITY
            }
        })
        .collect()
}

/// A value at each level: `f64`, double-double, each big-float width.
fn likes() -> (Bounded<f64>, Bounded<Dd>, [Bounded<Big>; 3]) {
    (
        Bounded::exact(0.0),
        Bounded::exact(Dd::ZERO),
        Width::ALL.map(|w| Bounded::exact(Big::from_f64(0.0, w))),
    )
}

/// Every level on random cases at degrees 1 to 4 against the exact
/// result: no certified bound is violated, almost everything certifies
/// (all of it at moderate scales; the rest are values far past the `f64`
/// range, whose rounding error is past it too below the top width), and
/// at moderate scales a level never bounds a result looser than the one
/// below it beyond a few roundings of the result.
#[test]
fn every_level_holds_its_bounds_at_degrees_one_to_four() {
    let (l0, l1, big) = likes();
    let mut tally = [Tally::default(); LEVELS];
    let mut moderate = [Tally::default(); LEVELS];
    for seed in varde_testing::seeds(24, 1024) {
        let mut rng = Rng::new(seed);
        let wide = seed % 4 == 3;
        let c = rng.case(wide);
        let truth = run(&c, &Q::from(0u32));
        let mut errs = Vec::with_capacity(LEVELS);
        let mut each = [Tally::default(); LEVELS];
        errs.push(check(&c, &truth, &l0, &mut each[0]));
        errs.push(check(&c, &truth, &l1, &mut each[1]));
        for (k, like) in big.iter().enumerate() {
            errs.push(check(&c, &truth, like, &mut each[2 + k]));
        }
        for (k, e) in each.iter().enumerate() {
            tally[k].results += e.results;
            tally[k].certified += e.certified;
            if !wide {
                assert_eq!(e.certified, e.results, "{:?} on {c:?}", Lv::ALL[k]);
                moderate[k].results += e.results;
                moderate[k].certified += e.certified;
            }
        }
        // At moderate scales each level at least as tight as the one
        // below (near the bottom of the range the floors differ), allowing for
        // the slack of different evaluation orders: within 4 roundings
        // of the result's size at the lower level's unit, and `2⁻¹⁰⁰⁰`
        // for the floors of the underflows a result near zero takes,
        // which differ by level.
        for k in (1..LEVELS).filter(|_| !wide) {
            for (i, (lo, hi)) in errs[k - 1].iter().zip(&errs[k]).enumerate() {
                let size = approx(&qabs(truth[i].clone()));
                let slack = lo + 4.0 * U * size + pow2(-1000);
                assert!(
                    *hi <= slack || !lo.is_finite(),
                    "{:?} looser than {:?} on result {i}: {hi:e} > {lo:e} of {c:?}",
                    Lv::ALL[k],
                    Lv::ALL[k - 1]
                );
            }
        }
    }
    println!("certified per level (all, moderate): {tally:?} {moderate:?}");
    for (k, t) in tally.iter().enumerate() {
        assert!(
            t.certified * 100 >= t.results * 95,
            "{:?}: {t:?}",
            Lv::ALL[k]
        );
    }
}

/// The sign of `p(t) − K₁ − … − K_k`, the `Kⱼ` the first `k` terms of the
/// exact `p(t)` as a sum of `f64`s, so the value is `p(t)` past `53·k`
/// bits of cancellation, or exactly zero. Each run charges one
/// evaluation's units at its level.
struct Cancelled<'a> {
    case: &'a Case,
    ks: &'a [f64],
}

impl Cancelled<'_> {
    fn value<S: Scalar>(&self, like: &S) -> S {
        let p = Poly::new(self.case.p.iter().map(|f| f.at(like))).expect("degree in range");
        let mut v = p.eval(&self.case.t.at(like));
        for k in self.ks {
            v = v.sub(&S::from_f64(*k, like));
        }
        v
    }
}

impl AtLevel<u64> for Cancelled<'_> {
    type Out = (Lv, Option<Ordering>, [u64; 2]);

    fn at<S: LevelScalar>(&mut self, like: &S, units: &mut u64) -> Self::Out {
        let level = like.level();
        *units += UNITS.record.eval[level.index()];
        let v = self.value(like);
        let [lo, hi] = v.enclose();
        (level, v.sign(), [lo.to_bits(), hi.to_bits()])
    }

    fn escalates(out: &Self::Out) -> bool {
        out.1.is_none()
    }
}

/// The first `k` terms of `x` as a sum of `f64`s (each the nearest to the
/// rest).
fn terms(x: &Q, k: usize) -> Vec<f64> {
    let mut rest = x.clone();
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        let t = approx(&rest);
        rest -= q(t);
        out.push(t);
    }
    out
}

/// The ladder on one case cancelled by `k` terms: its answer, which must
/// be the exact sign where it has one, reached at the first level that
/// certifies one (or the top, without); the units charged per level.
fn climb(c: &Case, k: usize) -> ((Lv, Option<Ordering>, [u64; 2]), [u64; LEVELS]) {
    let truth = Cancelled { case: c, ks: &[] }.value(&Q::from(0u32));
    let ks = terms(&truth, k);
    let exact = Cancelled { case: c, ks: &ks }.value(&Q::from(0u32));
    let mut f = Cancelled { case: c, ks: &ks };
    let mut units = [0u64; LEVELS];
    let out = ladder(&mut f, Lv::L0, &mut units);
    let (level, sign, _) = out;
    if let Some(s) = sign {
        assert_eq!(s, exact.cmp(&Q::from(0u32)), "{k} terms of {c:?}");
    }
    // The levels below the one reached certified no sign, each charged
    // once at its row; none above it ran.
    for l in Lv::ALL {
        let ran = l <= level;
        let want = if ran { UNITS.record.eval[l.index()] } else { 0 };
        assert_eq!(units[l.index()], want, "{l:?}, {k} terms of {c:?}");
        if l < level {
            let (_, s, _) = crate::num::level::at_level(&mut f, l, &mut 0);
            assert_eq!(s, None, "{l:?} settles below {level:?}");
        }
    }
    if sign.is_none() {
        assert_eq!(level, Lv::TOP);
    }
    (out, units)
}

/// Signs past `53·k` bits of cancellation, `k` from 0 to 17, at degrees
/// 1 to 4: never wrong; the ladder climbs one level at a time and stops
/// at the first that certifies, each level charged at its row; every
/// level is where some sign settles, and exact zeros and cancellation
/// past the top width stay open at the top.
#[test]
fn signs_settle_at_the_first_level_that_certifies_them() {
    let mut settled = [0u32; LEVELS];
    let mut open = 0u32;
    for seed in varde_testing::seeds(16, 512) {
        let mut rng = Rng::new(seed);
        let c = rng.case(false);
        for k in [0, 1, 2, 3, 5, 9, 17] {
            let ((level, sign, _), _) = climb(&c, k);
            match sign {
                Some(_) => settled[level.index()] += 1,
                None => open += 1,
            }
        }
    }
    println!("settled per level {settled:?}, open at the top {open}");
    assert!(settled.iter().all(|&n| n > 0), "{settled:?}");
    assert!(open > 0);
}

/// FNV-1a.
struct Hash(u64);

impl Hash {
    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01B3);
        }
    }

    fn u64(&mut self, x: u64) {
        self.bytes(&x.to_le_bytes());
    }

    fn all<S: Level>(&mut self, xs: &[S]) {
        for x in xs {
            self.bytes(x.bits().as_bytes());
        }
    }
}

/// The hash of every level's results on a fixed set of cases (seeded,
/// independent of `VARDE_TESTS`) and of the ladder's escalation on them:
/// the level each sign settles at, the sign, its enclosure and the units
/// charged per level. The same bits on every platform and thread count:
/// `tools/ci.sh` runs it natively, with hardware FMA, and on wasm
/// (`wasm32-wasip1`). A change of algorithm, schedule or unit table
/// changes the hash, which then needs updating here.
#[test]
fn levels_and_escalations_are_the_same_bits_everywhere() {
    let (l0, l1, big) = likes();
    let mut h = Hash(0xCBF2_9CE4_8422_2325);
    for seed in 0..12 {
        let mut rng = Rng::new(0x1E7E_0000 + seed);
        let c = rng.case(seed % 4 == 3);
        h.all(&run(&c, &l0));
        h.all(&run(&c, &l1));
        for like in &big {
            h.all(&run(&c, like));
        }
        for k in [0, 1, 2, 3, 5, 9, 17] {
            let ((level, sign, enclosure), units) = climb(&c, k);
            h.u64(level.index() as u64);
            h.u64(sign.map_or(3, |s| s as i8 as u64 & 3));
            enclosure.iter().chain(&units).for_each(|&x| h.u64(x));
        }
    }
    println!("hash of the results: {:#018x}", h.0);
    assert_eq!(
        h.0, 0x976a_1475_c8ff_df12,
        "results differ from the reference bits"
    );
}
