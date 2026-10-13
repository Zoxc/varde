//! The Bernstein forms against exact rational arithmetic, at every level
//! (`f64`, double-double, big-float), degrees 1 to 8: every certified
//! coefficient and value holds the exact result of the same operations on
//! the true coefficients (anywhere within their bounds), through chains of
//! splits and elevations; the hull encloses the exact polynomial; the
//! split at ½ gives the same bits under reversal; and on rationals every
//! operation is exact.

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::*;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::{Bounded, ETA, U};
use crate::num::dd::Dd;

pub(super) fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

pub(super) fn qabs(x: Q) -> Q {
    if x < 0u32 { -x } else { x }
}

/// A precision level's scalar, as the tests see it.
pub(super) trait Level: Scalar {
    const NAME: &'static str;

    /// The value `hi + lo` exactly, with no error (`lo` dropped at the
    /// first level).
    fn of(hi: f64, lo: f64) -> Self;

    /// The value, exactly.
    fn val(&self) -> Q;

    fn errs(&self) -> (f64, f64);

    fn with_errs(self, t: f64, r: f64) -> Self;

    fn certified(&self) -> bool;

    /// Every bit of the value and the error parts.
    fn bits(&self) -> String;
}

impl Level for Bounded<f64> {
    const NAME: &'static str = "f64";

    fn of(hi: f64, _: f64) -> Self {
        Bounded::exact(hi)
    }

    fn val(&self) -> Q {
        q(self.v)
    }

    fn errs(&self) -> (f64, f64) {
        (self.err_trunc, self.err_round)
    }

    fn with_errs(self, t: f64, r: f64) -> Self {
        Bounded::new(self.v, t, r)
    }

    fn certified(&self) -> bool {
        self.is_certified()
    }

    fn bits(&self) -> String {
        format!("{:?}", self)
    }
}

impl Level for Bounded<Dd> {
    const NAME: &'static str = "dd";

    fn of(hi: f64, lo: f64) -> Self {
        Bounded::exact(Dd::sum(hi, lo))
    }

    fn val(&self) -> Q {
        q(self.v.hi()) + q(self.v.lo())
    }

    fn errs(&self) -> (f64, f64) {
        (self.err_trunc, self.err_round)
    }

    fn with_errs(self, t: f64, r: f64) -> Self {
        Bounded::new(self.v, t, r)
    }

    fn certified(&self) -> bool {
        self.is_certified()
    }

    fn bits(&self) -> String {
        format!("{:?}", self)
    }
}

impl Level for Bounded<Big> {
    const NAME: &'static str = "big";

    fn of(hi: f64, lo: f64) -> Self {
        Bounded::exact(Big::from_dd(Dd::sum(hi, lo), Width::MIN))
    }

    fn val(&self) -> Q {
        Q::try_from(self.v.as_float()).expect("finite")
    }

    fn errs(&self) -> (f64, f64) {
        (self.err_trunc, self.err_round)
    }

    fn with_errs(self, t: f64, r: f64) -> Self {
        Bounded::new(self.v, t, r)
    }

    fn certified(&self) -> bool {
        self.is_certified()
    }

    fn bits(&self) -> String {
        format!("{:?}", self)
    }
}

pub(super) struct Rng(pub(super) varde_testkit::rng::Rng);

impl Rng {
    pub(super) fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    pub(super) fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    /// `±m·2ᵉ`, `m` in `[1, 2)` with a random mantissa, `e` in `lo..=hi`.
    pub(super) fn pow(&mut self, lo: i32, hi: i32) -> f64 {
        let span = u64::try_from(hi - lo + 1).expect("lo ≤ hi");
        let e = lo + i32::try_from(self.below(span)).expect("small");
        let m = f64::from_bits(0x3FF0_0000_0000_0000 | self.0.next_u64() >> 12);
        let x = m * pow2(e);
        if self.below(2) == 0 { x } else { -x }
    }

    /// The scale of one polynomial's coefficients: moderate mostly, near
    /// the bottom of the range (subnormal products) or in it (subnormal
    /// coefficients) sometimes.
    pub(super) fn scale(&mut self) -> i32 {
        match self.below(16) {
            0 | 1 => -1000,
            2 => -1060,
            3 | 4 => 400,
            _ => i32::try_from(self.below(81)).expect("small") - 40,
        }
    }

    /// A coefficient about `2^scale`: zero sometimes, else spread over a
    /// few binades below it, with a double-double low part.
    pub(super) fn coef(&mut self, scale: i32) -> (f64, f64) {
        if self.below(10) == 0 {
            return (0.0, 0.0);
        }
        let hi = self.pow(scale - 8, scale);
        let lo = if self.below(2) == 0 {
            0.0
        } else {
            hi * self.pow(-80, -54).abs()
        };
        (hi, lo)
    }

    /// Error parts for a value of magnitude about `m`.
    pub(super) fn errs(&mut self, m: f64) -> (f64, f64) {
        let part = |rng: &mut Rng| match rng.below(3) {
            0 => 0.0,
            1 => m.abs() * rng.pow(-55, -30).abs(),
            _ => rng.pow(-1074 + 60, -1000).abs(),
        };
        let t = part(self);
        (t, part(self))
    }

    /// A point in `[−1, 1]`: an end, the middle, or a dyadic between.
    pub(super) fn lambda(&mut self) -> Q {
        match self.below(4) {
            0 => q(-1.0),
            1 => q(1.0),
            2 => q(0.0),
            _ => q((self.below(2049) as f64 - 1024.0) / 1024.0),
        }
    }

    /// A parameter in `[0, 1]`: ½ or an end sometimes, a dyadic, a
    /// multiple of `2⁻⁵³` (`1 − t` exact), or a float below ½ with a full
    /// mantissa (`1 − t` mostly rounds).
    pub(super) fn t(&mut self) -> f64 {
        match self.below(8) {
            0 => 0.5,
            1 => 0.0,
            2 => 1.0,
            3 => self.below(1025) as f64 / 1024.0,
            4 | 5 => self.pow(-20, -2).abs(),
            _ => self.0.unit(),
        }
    }

    /// A polynomial of degree `n` and its true coefficients.
    fn poly<S: Level>(&mut self, n: usize, with_errs: bool) -> (Poly<S>, Vec<Q>) {
        let scale = self.scale();
        let mut c = Vec::new();
        let mut truth = Vec::new();
        for _ in 0..=n {
            let (hi, lo) = self.coef(scale);
            let mut b = S::of(hi, lo);
            let mut x = b.val();
            if with_errs && self.below(2) == 0 {
                let (t, r) = self.errs(hi);
                b = b.with_errs(t, r);
                x += self.lambda() * (q(t) + q(r));
            }
            c.push(b);
            truth.push(x);
        }
        (Poly::new(c).expect("degree in range"), truth)
    }
}

/// `2ᵉ`, exactly for `e` in the `f64` range.
pub(super) fn pow2(e: i32) -> f64 {
    let mut x = 1.0;
    let step = if e < 0 { 0.5 } else { 2.0 };
    for _ in 0..e.unsigned_abs() {
        x *= step;
    }
    x
}

/// De Casteljau in rationals.
fn eval_q(c: &[Q], t: &Q) -> Q {
    let s = Q::from(1u32) - t;
    let mut w = c.to_vec();
    for k in 1..w.len() {
        for i in 0..w.len() - k {
            w[i] = &s * &w[i] + t * &w[i + 1];
        }
    }
    w.swap_remove(0)
}

fn split_q(c: &[Q], t: &Q) -> [Vec<Q>; 2] {
    let s = Q::from(1u32) - t;
    let n = c.len() - 1;
    let mut w = c.to_vec();
    let mut left = vec![w[0].clone()];
    let mut right = w.clone();
    for k in 1..=n {
        for i in 0..=n - k {
            w[i] = &s * &w[i] + t * &w[i + 1];
        }
        left.push(w[0].clone());
        right[n - k] = w[n - k].clone();
    }
    [left, right]
}

fn elevate_q(c: &[Q]) -> Vec<Q> {
    let n1 = Q::from(c.len());
    let mut out = vec![c[0].clone()];
    for i in 1..c.len() {
        let w = Q::from(i) / &n1;
        out.push(&w * &c[i - 1] + (Q::from(1u32) - &w) * &c[i]);
    }
    out.push(c[c.len() - 1].clone());
    out
}

/// Checks a certified result against its exact value; returns whether it
/// was certified.
pub(super) fn holds<S: Level>(what: &str, r: &S, exact: &Q) -> bool {
    if !r.certified() {
        return false;
    }
    let (t, e) = r.errs();
    assert!(
        qabs(exact - r.val()) <= q(t) + q(e),
        "{} {what}: bound violated: {r:?}, exact {}",
        S::NAME,
        approx(exact)
    );
    true
}

/// A rational rounded to an `f64`, for messages.
pub(super) fn approx(x: &Q) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// The hull holds every true coefficient, and so the true polynomial.
fn hull_holds<S: Level>(what: &str, p: &Poly<S>, truth: &[Q]) {
    let h = p.hull();
    for x in truth {
        if h.lo > f64::NEG_INFINITY {
            assert!(q(h.lo) <= *x, "{} {what}: hull {h:?} above {x}", S::NAME);
        }
        if h.hi < f64::INFINITY {
            assert!(*x <= q(h.hi), "{} {what}: hull {h:?} below {x}", S::NAME);
        }
    }
}

/// Eval at random parameters, exact in `[0, 1]` (the first level's fast
/// path), beyond it, or with an error of their own.
fn eval_case<S: Level>(rng: &mut Rng, p: &Poly<S>, truth: &[Q], stats: &mut [u32; 2]) {
    let (t, tq) = match rng.below(4) {
        0 => {
            let t = rng.0.range(-0.5, 1.5);
            (S::of(t, 0.0), q(t))
        }
        1 => {
            let t = rng.t();
            let e = rng.pow(-60, -40).abs();
            let tq = q(t) + rng.lambda() * q(e);
            (S::of(t, 0.0).with_errs(0.0, e), tq)
        }
        _ => {
            let t = rng.t();
            (S::of(t, 0.0), q(t))
        }
    };
    let v = p.eval(&t);
    stats[1] += 1;
    if holds("eval", &v, &eval_q(truth, &tq)) {
        stats[0] += 1;
    }
    let [lo, hi] = v.enclose();
    let exact = eval_q(truth, &tq);
    if v.certified() {
        assert!(
            q(lo) <= exact && exact <= q(hi),
            "{} eval enclosure",
            S::NAME
        );
    }
}

/// Chains of splits (at ½ and at `t`) and elevations, every coefficient
/// checked at every step, and evaluations and the hull along the way.
fn chains<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..300 {
            let n = 1 + case % 8;
            let (mut p, mut truth) = rng.poly::<S>(n, case % 3 != 0);
            for step in 0..5 {
                hull_holds("hull", &p, &truth);
                eval_case(&mut rng, &p, &truth, &mut stats);
                let side = usize::from(rng.below(2) == 0);
                let (next, exact) = match rng.below(5) {
                    0 | 1 => (
                        p.split_half()[side].clone(),
                        split_q(&truth, &q(0.5))[side].clone(),
                    ),
                    2 | 3 => {
                        // Exact, or with an error of its own: the pieces
                        // then hold the split at the true `t`.
                        let t = rng.t();
                        let (ts, tq) = if rng.below(3) == 0 {
                            let e = rng.pow(-60, -40).abs();
                            (S::of(t, 0.0).with_errs(0.0, e), q(t) + rng.lambda() * q(e))
                        } else {
                            (S::of(t, 0.0), q(t))
                        };
                        (
                            p.split_at(&ts)[side].clone(),
                            split_q(&truth, &tq)[side].clone(),
                        )
                    }
                    _ if p.degree() < 10 => (p.elevate(1).expect("in range"), elevate_q(&truth)),
                    _ => (p.reversed(), truth.iter().rev().cloned().collect()),
                };
                assert_eq!(next.coefs().len(), exact.len());
                for (b, x) in next.coefs().iter().zip(&exact) {
                    stats[1] += 1;
                    if holds(&format!("seed {seed} case {case} step {step}"), b, x) {
                        stats[0] += 1;
                    }
                }
                (p, truth) = (next, exact);
            }
        }
    }
    // Not vacuous: nearly everything certifies (the uncertified are
    // overflows at the top of the range).
    assert!(
        stats[0] * 10 >= stats[1] * 9,
        "{}: certified {stats:?}",
        S::NAME
    );
}

#[test]
fn chains_hold_their_bounds_f64() {
    chains::<Bounded<f64>>(varde_testing::seeds(1, 16));
}

#[test]
fn chains_hold_their_bounds_dd() {
    chains::<Bounded<Dd>>(varde_testing::seeds(1, 16));
}

#[test]
fn chains_hold_their_bounds_big() {
    chains::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// The bounds are tight enough to use: on exact inputs of moderate size,
/// within a small multiple of `n·u` of the largest coefficient at the
/// first level, for the fast evaluation, the general one and splits.
#[test]
fn first_level_bounds_are_tight() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..500 {
            let n = 1 + case % 8;
            let c: Vec<f64> = (0..=n).map(|_| rng.pow(-4, 4)).collect();
            let p = Poly::from_f64(&c).expect("degree in range");
            let big = c
                .iter()
                .fold(0.0, |m: f64, x| if x.abs() > m { x.abs() } else { m });
            let allowed = 8.0 * (n as f64) * U * big;
            let t = rng.0.unit();
            let fast = p.eval(&Bounded::exact(t));
            let general = p.eval(&Bounded::new(t, 0.0, f64::from_bits(1)));
            assert!(fast.err() <= allowed, "fast {fast:?} > {allowed}");
            assert!(
                general.err() <= 2.0 * allowed,
                "general {general:?} > {allowed}"
            );
            for piece in p.split_half().iter().chain(&p.split_at(&Bounded::exact(t))) {
                for b in piece.coefs() {
                    assert!(b.err() <= allowed, "split {b:?} > {allowed}");
                }
            }
        }
    }
}

/// The split at ½ of the reversed polynomial is the halves swapped and
/// reversed, to the bit (error parts included), down four levels.
fn mirror<S: Level>() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..200 {
            let (p, _) = rng.poly::<S>(1 + case % 8, case % 2 == 0);
            let mut pieces = vec![p.clone()];
            let mut mirrored = vec![p.reversed()];
            for _ in 0..4 {
                pieces = pieces.iter().flat_map(|x| x.split_half()).collect();
                mirrored = mirrored.iter().flat_map(|x| x.split_half()).collect();
            }
            let n = pieces.len();
            for (i, a) in pieces.iter().enumerate() {
                let b = mirrored[n - 1 - i].reversed();
                let bits = |x: &Poly<S>| x.coefs().iter().map(Level::bits).collect::<Vec<_>>();
                assert_eq!(
                    bits(a),
                    bits(&b),
                    "{} seed {seed} case {case} piece {i}",
                    S::NAME
                );
            }
            // The halves meet in the same bits, and keep the ends.
            let [l, r] = p.split_half();
            let (ln, rn) = (l.coefs().len() - 1, p.degree());
            assert_eq!(l.coefs()[ln].bits(), r.coefs()[0].bits());
            assert_eq!(l.coefs()[0].bits(), p.coefs()[0].bits());
            assert_eq!(r.coefs()[rn].bits(), p.coefs()[rn].bits());
        }
    }
}

#[test]
fn split_half_is_bit_symmetric() {
    mirror::<Bounded<f64>>();
    mirror::<Bounded<Dd>>();
    mirror::<Bounded<Big>>();
}

/// On rationals every operation is exact: the pieces are the polynomial
/// on their intervals, elevation keeps it, reversal mirrors it.
#[test]
fn rational_operations_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..100 {
            let n = 1 + case % 8;
            let p = Poly::new((0..=n).map(|_| Q::from(rng.below(201) as i64 - 100)))
                .expect("degree in range");
            let t = Q::from(rng.below(1023) + 1) / Q::from(1024u32);
            let u = Q::from(rng.below(1025)) / Q::from(1024u32);
            let one = Q::from(1u32);
            let half = Q::from(1u32) / Q::from(2u32);
            let [l, r] = p.split_half();
            assert_eq!(l.eval(&u), p.eval(&(&u * &half)));
            assert_eq!(r.eval(&u), p.eval(&(&half + &u * &half)));
            let [l, r] = p.split_at(&t);
            assert_eq!(l.eval(&u), p.eval(&(&t * &u)));
            assert_eq!(r.eval(&u), p.eval(&(&t + (&one - &t) * &u)));
            let e = p.elevate(3).expect("in range");
            assert_eq!(e.degree(), n + 3);
            assert_eq!(e.eval(&u), p.eval(&u));
            assert_eq!(p.reversed().eval(&u), p.eval(&(&one - &u)));
            let h = p.hull();
            let v = p.eval(&u);
            assert!(q(h.lo) <= v && v <= q(h.hi));
            assert!(q(h.abs_max()) >= qabs(v));
        }
    }
}

#[test]
fn degrees_are_limited() {
    assert!(Poly::<Q>::new([]).is_none());
    let max = Poly::new((0..=MAX_DEGREE).map(|_| Q::from(1u32))).expect("at the limit");
    assert_eq!(max.degree(), MAX_DEGREE);
    assert!(Poly::new((0..=MAX_DEGREE + 1).map(|_| Q::from(1u32))).is_none());
    assert!(max.elevate(1).is_none());
    assert!(max.elevate(usize::MAX).is_none());
    let p = Poly::from_f64(&[1.0, 2.0]).expect("degree 1");
    assert_eq!(
        p.elevate(MAX_DEGREE - 1).expect("to the limit").degree(),
        MAX_DEGREE
    );
}

/// The fast evaluation's `m = 3n` when `1 − t` rounds is needed: at this
/// `t` below ½, `s = fl(1 − t)` is `1 − t` less half its ulp, and the
/// product `s·b₀` and the sum each round by half an ulp the same way, so
/// the degree-1 value is off by nearly `3u` of the absolute evaluation,
/// past the `2n` roundings' bound, within the `3n` one.
#[test]
fn fast_eval_needs_3n_when_one_minus_t_rounds() {
    let t = f64::from_bits(0x3FDF_FFFF_FFFF_FFFB); // ½ − 5·2⁻⁵⁴
    let (b0, b1) = (f64::from_bits(0x3FFF_FFFF_FFFF_FFFF), pow2(-52)); // 2 − 2⁻⁵², 2⁻⁵²
    let s = 1.0 - t;
    assert!(q(s) != Q::from(1u32) - q(t), "1 − t rounds");
    let p = Poly::from_f64(&[b0, b1]).expect("degree 1");
    let v = p.eval(&Bounded::exact(t));
    let off = qabs(eval_q(&[q(b0), q(b1)], &q(t)) - q(v.v));
    assert!(off <= q(v.err()), "{v:?} off by {}", approx(&off));
    let a_hat = s * b0.abs() + t * b1.abs();
    let two_n = crate::num::bounded::gamma_bound(2, a_hat, U);
    assert!(off > q(two_n), "2n would hold: {two_n:e}");
}

/// The fast evaluation's absolute `m·η` covers underflow: on subnormal
/// coefficients every product rounds by up to `η/2`, which the relative
/// `γₘ` term (of a value itself near `η`) doesn't cover; here the error
/// is over `6η`.
#[test]
fn fast_eval_covers_underflow() {
    let t = f64::from_bits(0x3FA4_E250_8E60_74A0);
    let c = [37u32, 60, 63, 17, 21, 3, 58, 30, 29].map(|k| f64::from(k) * ETA);
    let p = Poly::from_f64(&c).expect("degree 8");
    let v = p.eval(&Bounded::exact(t));
    let off = qabs(eval_q(&c.map(q), &q(t)) - q(v.v));
    assert!(off <= q(v.err()), "{v:?} off by {}", approx(&off));
    assert!(off > q(6.0 * ETA));
}

/// Elevation commutes with reversal to the bit: a curve run either way
/// elevates to the same coefficients. With big-float coefficients of
/// mixed widths each quotient rounds at its own numerator's width, never
/// at a width taken from one end (here `2/3`, at 424 bits one way and 212
/// the other).
#[test]
fn elevation_commutes_with_reversal() {
    fn same<S: Level>(p: &Poly<S>, by: usize) {
        let bits = |x: &Poly<S>| x.coefs().iter().map(Level::bits).collect::<Vec<_>>();
        let a = p.elevate(by).expect("in range").reversed();
        let b = p.reversed().elevate(by).expect("in range");
        assert_eq!(bits(&a), bits(&b), "{}", S::NAME);
    }
    let big = |x: f64, w: usize| Bounded::exact(Big::from_f64(x, Width::ALL[w]));
    let p = Poly::new([big(1.0, 1), big(1.0, 0), big(0.0, 0)]).expect("degree 2");
    same(&p, 1);
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..50 {
            let n = 1 + case % 8;
            let by = 1 + case % 5;
            same(&rng.poly::<Bounded<f64>>(n, case % 2 == 0).0, by);
            same(&rng.poly::<Bounded<Dd>>(n, case % 2 == 0).0, by);
            let p = Poly::new((0..=n).map(|_| {
                let (hi, lo) = rng.coef(0);
                let w = Width::ALL[rng.below(3) as usize];
                Bounded::exact(Big::from_dd(Dd::sum(hi, lo), w))
            }))
            .expect("degree in range");
            same(&p, by);
        }
    }
}

/// The bounds hold up to [`MAX_DEGREE`]: elevation one degree at a time
/// from low degrees to the limit, every coefficient checked, then the
/// evaluation and both halves at the limit.
fn up_to_max_degree<S: Level>(seeds: std::ops::Range<u64>) {
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..3 {
            let (mut p, mut truth) = rng.poly::<S>(1 + case, true);
            while p.degree() < MAX_DEGREE {
                p = p.elevate(1).expect("in range");
                truth = elevate_q(&truth);
                for (b, x) in p.coefs().iter().zip(&truth) {
                    holds(&format!("seed {seed} elevation"), b, x);
                }
            }
            hull_holds("hull at the limit", &p, &truth);
            let t = rng.below(1025) as f64 / 1024.0;
            for t in [0.5, t] {
                holds(
                    "eval at the limit",
                    &p.eval(&S::of(t, 0.0)),
                    &eval_q(&truth, &q(t)),
                );
            }
            for (piece, exact) in p.split_half().iter().zip(split_q(&truth, &q(0.5))) {
                for (b, x) in piece.coefs().iter().zip(&exact) {
                    holds("half at the limit", b, x);
                }
            }
        }
    }
}

#[test]
fn bounds_hold_up_to_max_degree() {
    up_to_max_degree::<Bounded<f64>>(varde_testing::seeds(1, 8));
    up_to_max_degree::<Bounded<Dd>>(varde_testing::seeds(1, 8));
    up_to_max_degree::<Bounded<Big>>(varde_testing::seeds(1, 4));
}
