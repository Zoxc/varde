//! Big-float against exact rational arithmetic. The rule first: every
//! operation and conversion at every width is the exact result rounded to
//! nearest-even (by a rounding written here on rationals, independent of
//! the float code), with the range tested on the rounded result, over
//! operands anywhere in the exponent range (its edges, where products and
//! sums leave it, often), carrying fewer or more bits than the width.
//! Then the bounds: every certified [`Bounded<Big>`] result holds the
//! exact result within its error, values beyond the `f64` range included.
//! Plus promotion, and the same bits everywhere.

use std::cmp::Ordering;

use malachite::base::num::arithmetic::traits::{DivRem, DivisibleByPowerOf2, Parity, PowerOf2};
use malachite::base::num::conversion::traits::{IntegerMantissaAndExponent, RoundingFrom};
use malachite::base::num::logic::traits::{BitAccess, SignificantBits};
use malachite::base::rounding_modes::RoundingMode;
use malachite::{Float, Natural, Rational as Q};

use super::*;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn qd(x: Dd) -> Q {
    q(x.hi()) + q(x.lo())
}

fn qb(x: &Big) -> Q {
    Q::try_from(&x.x).expect("finite")
}

fn qabs(x: Q) -> Q {
    if x < 0u32 { -x } else { x }
}

fn p2(k: i64) -> Q {
    Q::power_of_2(k)
}

/// An exact dyadic value `±m·2ᵉ`, a zero when `m` is: the oracle of the
/// rounding rule. Integers only (no rational normalisation), so operands
/// a million binades apart cost a shift.
#[derive(Clone, Debug)]
struct D {
    neg: bool,
    m: Natural,
    e: i64,
}

impl D {
    /// A finite big float's value.
    fn of(b: &Big) -> D {
        let neg = b.x.is_sign_negative();
        if b.x.is_zero() {
            return D::zero(neg);
        }
        let (m, e) = (&b.x).integer_mantissa_and_exponent();
        D { neg, m, e }
    }

    fn zero(neg: bool) -> D {
        D {
            neg,
            m: Natural::from(0u32),
            e: 0,
        }
    }

    fn is_zero(&self) -> bool {
        self.m == 0u32
    }

    /// `⌊log₂|x|⌋` of a non-zero value.
    fn log2(&self) -> i64 {
        self.e + (&self.m).significant_bits() as i64 - 1
    }

    fn neg(&self) -> D {
        D {
            neg: !self.neg,
            ..self.clone()
        }
    }

    /// The exact sum (a zero sum `+0`; callers give IEEE's signs).
    fn add(&self, o: &D) -> D {
        if self.is_zero() {
            return o.clone();
        }
        if o.is_zero() {
            return self.clone();
        }
        let e = self.e.min(o.e);
        let a = &self.m << (self.e - e) as u64;
        let b = &o.m << (o.e - e) as u64;
        if self.neg == o.neg {
            return D {
                neg: self.neg,
                m: a + b,
                e,
            };
        }
        match a.cmp(&b) {
            Ordering::Equal => D::zero(false),
            Ordering::Greater => D {
                neg: self.neg,
                m: a - b,
                e,
            },
            Ordering::Less => D {
                neg: o.neg,
                m: b - a,
                e,
            },
        }
    }

    fn mul(&self, o: &D) -> D {
        D {
            neg: self.neg != o.neg,
            m: &self.m * &o.m,
            e: self.e + o.e,
        }
    }

    /// `self` against `o`, by value.
    fn cmp(&self, o: &D) -> Ordering {
        let d = self.add(&o.neg());
        if d.is_zero() {
            Ordering::Equal
        } else if d.neg {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    /// The same value (a zero's sign aside).
    fn same(&self, o: &D) -> bool {
        self.cmp(o) == Ordering::Equal
    }

    /// The exponent of the last set bit: `x` is a multiple of `2^that`.
    fn last_bit(&self) -> i64 {
        self.e + self.m.trailing_zeros().expect("non-zero") as i64
    }
}

/// `n` rounded to a multiple of `2^shift`, ties to even, with `sticky`
/// standing for a part below `n`'s last bit (non-zero, under one unit);
/// and whether that rounded anything off.
fn round_at(n: &Natural, shift: u64, sticky: bool) -> (Natural, bool) {
    if shift == 0 {
        assert!(!sticky, "no room for the sticky part");
        return (n.clone(), false);
    }
    let q = n >> shift;
    let half = n.get_bit(shift - 1);
    let below = !n.divisible_by_power_of_2(shift - 1) || sticky;
    let up = half && (below || q.odd());
    let q = if up { q + Natural::from(1u32) } else { q };
    (q << shift, half || below)
}

/// `±n·2ᵉ` (plus a sticky part, see [`round_at`]) rounded to nearest-even
/// at `p` significant bits, with an unbounded exponent range, and whether
/// it was inexact.
fn rne_parts(neg: bool, n: &Natural, e: i64, sticky: bool, p: u32) -> (D, bool) {
    let bits = n.significant_bits();
    let shift = bits.saturating_sub(u64::from(p));
    let (m, inexact) = round_at(n, shift, sticky);
    (D { neg, m, e }, inexact)
}

fn rne(x: &D, p: u32) -> (D, bool) {
    rne_parts(x.neg, &x.m, x.e, false, p)
}

/// `a / b` rounded at `p` bits, for `b ≠ 0`, and whether inexact: the
/// quotient of the significands to at least `p + 2` bits and the rest
/// as a sticky part.
fn rne_div(a: &D, b: &D, p: u32) -> (D, bool) {
    let neg = a.neg != b.neg;
    if a.is_zero() {
        return (D::zero(neg), false);
    }
    let k = i64::from(p) + 2 + (&b.m).significant_bits() as i64 - (&a.m).significant_bits() as i64;
    let (num, den) = if k >= 0 {
        (&a.m << k as u64, b.m.clone())
    } else {
        (a.m.clone(), &b.m << (-k) as u64)
    };
    let (qt, rest) = (&num).div_rem(&den);
    rne_parts(neg, &qt, a.e - b.e - k, rest != 0u32, p)
}

/// Checks a result against the rule: `rounded` is the exact result rounded
/// at `w` (`inexact` if that rounded), `negative` the exact result's sign
/// (IEEE 754's for an exact zero).
fn conforms(
    what: &dyn Fn() -> String,
    r: &Rounded<Big>,
    rounded: &D,
    inexact: bool,
    w: Width,
    negative: bool,
) {
    let what = || format!("{}: {r:?}", what());
    assert_eq!(r.v.w, w, "{}", what());
    assert_eq!(r.v.x.is_sign_negative(), negative, "{}", what());
    if !rounded.is_zero() && rounded.log2() > BF_EXP {
        assert!(r.v.x.is_infinite(), "not ∞: {}", what());
        assert_eq!(r.rounding, Rounding::OutOfRange, "{}", what());
    } else if !rounded.is_zero() && rounded.log2() < -BF_EXP {
        assert!(r.v.x.is_zero(), "not 0: {}", what());
        assert_eq!(r.rounding, Rounding::OutOfRange, "{}", what());
    } else {
        assert!(r.v.x.is_finite(), "{}", what());
        assert!(D::of(&r.v).same(rounded), "not {rounded:?}: {}", what());
        let want = if inexact {
            Rounding::Inexact
        } else {
            Rounding::Exact
        };
        assert_eq!(r.rounding, want, "{}", what());
    }
}

/// Checks an exact result `x` rounded at `w` against the rule.
fn conforms_exact(
    what: &dyn Fn() -> String,
    r: &Rounded<Big>,
    x: &D,
    w: Width,
    zero_negative: bool,
) {
    let (rounded, inexact) = rne(x, w.bits());
    let negative = if x.is_zero() { zero_negative } else { x.neg };
    conforms(what, r, &rounded, inexact, w, negative);
}

/// Checks a root against the rule: `r` is the float of the width nearest
/// `√x` (ties to even), its midpoints with the neighbours bracketing `√x`.
fn root_conforms(r: &Rounded<Big>, x: &Big, w: Width) {
    assert_eq!(r.v.w, w);
    let dx = D::of(x);
    if dx.is_zero() {
        assert!(r.v.x.is_zero(), "{r:?}");
        assert_eq!(r.v.x.is_sign_negative(), x.x.is_sign_negative());
        assert_eq!(r.rounding, Rounding::Exact);
        return;
    }
    if dx.neg {
        assert!(r.v.x.is_nan(), "sqrt of a negative: {r:?}");
        return;
    }
    let v = D::of(&r.v);
    let ulp = v.log2() - i64::from(w.bits()) + 1;
    let pow = |k: i64| D {
        neg: false,
        m: Natural::from(1u32),
        e: k,
    };
    // Below a power of two the neighbour is half as far.
    let below = if (&v.m).significant_bits() == 1 + v.m.trailing_zeros().expect("non-zero") {
        pow(ulp - 2)
    } else {
        pow(ulp - 1)
    };
    let lo = v.add(&below.neg());
    let hi = v.add(&pow(ulp - 1));
    let (lo2, hi2) = (lo.mul(&lo), hi.mul(&hi));
    let (at_lo, at_hi) = (lo2.cmp(&dx), dx.cmp(&hi2));
    assert!(
        at_lo != Ordering::Greater && at_hi != Ordering::Greater,
        "sqrt not nearest: {r:?} of {x:?}"
    );
    if at_lo == Ordering::Equal || at_hi == Ordering::Equal {
        assert!(v.last_bit() > ulp, "sqrt tie not to even: {r:?} of {x:?}");
    }
    let want = if v.mul(&v).same(&dx) {
        Rounding::Exact
    } else {
        Rounding::Inexact
    };
    assert_eq!(r.rounding, want, "{r:?}");
}

/// `x` rounded to nearest-even into `f64` (subnormals, overflow to `±∞`).
fn f64_oracle(x: &D) -> f64 {
    let sign = if x.neg { -1.0 } else { 1.0 };
    if x.is_zero() {
        return sign * 0.0;
    }
    let s = x.log2();
    let quantum = if s < -1022 { -1074 } else { s - 52 };
    let (m, e) = if quantum > x.e {
        (
            round_at(&x.m, (quantum - x.e) as u64, false).0 >> (quantum - x.e) as u64,
            quantum,
        )
    } else {
        (&x.m << (x.e - quantum) as u64, quantum)
    };
    if (&m).significant_bits() as i64 + e > 1024 {
        return sign * f64::INFINITY;
    }
    // At most 2⁵³, exactly an `f64`; scaled exactly (the result is one).
    let m = f64::rounding_from(&m, RoundingMode::Exact).0;
    let v = if e >= -1022 {
        m * pow2(e)
    } else {
        m * pow2(e + 600) * pow2(-600)
    };
    sign * v
}

/// The kernels' seeded test generator, with draws of big floats across
/// the exponent range.
struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    /// A `bits`-bit significand (top bit set): random, all ones, a power
    /// of two, or a power of two plus one.
    fn significand(&mut self, bits: u32) -> Natural {
        let mut n = Natural::from(0u32);
        match self.below(6) {
            0 => {
                n.set_bit(u64::from(bits));
                n -= Natural::from(1u32);
            }
            1 => n.set_bit(u64::from(bits - 1)),
            2 => {
                n.set_bit(u64::from(bits - 1));
                n.set_bit(0);
            }
            _ => {
                for _ in 0..bits.div_ceil(64) {
                    n = (n << 64u32) + Natural::from(self.0.next_u64());
                }
                n >>= u64::from(bits.div_ceil(64) * 64 - bits);
                n.set_bit(u64::from(bits - 1));
            }
        }
        n
    }

    /// `⌊log₂|x|⌋` for an operand: moderate, the `f64` range, the edges of
    /// the big-float range and their halves (where products leave it), or
    /// anywhere in it.
    fn exponent(&mut self) -> i64 {
        match self.below(8) {
            0 | 1 => self.range(-60, 60),
            2 => self.range(-1080, 1030),
            3 => self.range(BF_EXP - 2, BF_EXP),
            4 => self.range(-BF_EXP, -BF_EXP + 2),
            5 => self.range(BF_EXP / 2 - 2, BF_EXP / 2 + 1),
            6 => self.range(-BF_EXP / 2 - 1, -BF_EXP / 2 + 2),
            _ => self.range(-BF_EXP, BF_EXP),
        }
    }

    /// A number of significant bits for an operand at width `w`: the width,
    /// fewer, a double's, or more (a promoted value's).
    fn precision(&mut self, w: Width) -> u32 {
        let b = w.bits();
        match self.below(5) {
            0 => b,
            1 => 1 + self.below(u64::from(b)) as u32,
            2 => 53,
            3 => 1 + self.below(8) as u32,
            _ => b + 1 + self.below(80) as u32,
        }
    }

    /// A float of `bits` significant bits with `⌊log₂|x|⌋ = e`.
    fn float(&mut self, bits: u32, e: i64) -> Float {
        let m = self.significand(bits);
        let (x, o) = Float::from_natural_prec_round(m, u64::from(bits), RoundingMode::Exact);
        assert_eq!(o, Ordering::Equal);
        let x = x << (e - i64::from(bits) + 1);
        if self.below(2) == 0 { x } else { -x }
    }

    /// An operand at width `w`: in range, occasionally a zero.
    fn big(&mut self, w: Width) -> Big {
        if self.below(40) == 0 {
            let x = if self.below(2) == 0 {
                Float::ZERO
            } else {
                Float::NEGATIVE_ZERO
            };
            return Big { x, w };
        }
        let (bits, e) = (self.precision(w), self.exponent());
        Big {
            x: self.float(bits, e),
            w,
        }
    }

    /// A second operand: drawn alike, or close to `−a` or `a` (so a sum or
    /// difference cancels).
    fn partner(&mut self, a: &Big, w: Width) -> Big {
        match (self.below(4), a.x.get_exponent()) {
            (0, Some(e)) => {
                let bits = 1 + self.below(u64::from(w.bits()) + 40) as u32;
                let (x, _) = Float::from_float_prec_round_ref(&a.x, u64::from(bits), Nearest);
                let (tb, te) = (1 + self.below(8) as u32, self.range(1, 300));
                let tweak = self.float(tb, i64::from(e) - 1 - te);
                let (x, _) = x.add_prec_round(tweak, u64::from(w.bits()) + 300, Nearest);
                // Clear of the range only by construction: keep it in.
                let x = if Big::rounded(x.clone(), Ordering::Equal, w).rounding
                    == Rounding::OutOfRange
                {
                    a.x.clone()
                } else {
                    x
                };
                let x = if self.below(2) == 0 { -x } else { x };
                Big { x, w }
            }
            _ => self.big(w),
        }
    }
}

#[test]
fn operations_are_correctly_rounded_at_every_width() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for &w in &Width::ALL {
            for _ in 0..300 {
                let a = rng.big(w);
                let b = rng.partner(&a, w);
                let (xa, xb) = (D::of(&a), D::of(&b));
                let (na, nb) = (xa.neg, xb.neg);
                let what = |op: &str| format!("{op} {a:?}, {b:?}");
                let p = w.bits();
                conforms_exact(&|| what("add"), &a.add(&b, w), &xa.add(&xb), w, na && nb);
                conforms_exact(
                    &|| what("sub"),
                    &a.sub(&b, w),
                    &xa.add(&xb.neg()),
                    w,
                    na && !nb,
                );
                conforms_exact(&|| what("mul"), &a.mul(&b, w), &xa.mul(&xb), w, na != nb);
                if !xb.is_zero() {
                    let (rounded, inexact) = rne_div(&xa, &xb, p);
                    conforms(
                        &|| what("div"),
                        &a.div(&b, w),
                        &rounded,
                        inexact,
                        w,
                        na != nb,
                    );
                }
                root_conforms(&a.sqrt(w), &a, w);
                // Between widths: to every width, narrower or wider.
                for &to in &Width::ALL {
                    conforms_exact(&|| what("to_width"), &a.to_width(to), &xa, to, na);
                }
                assert_eq!(
                    a.to_f64().to_bits(),
                    f64_oracle(&xa).to_bits(),
                    "to f64 {a:?}"
                );
            }
        }
    }
}

/// From `f64` and double-double, exactly: every float, subnormals and
/// signed zeros included, and double-doubles whose low part lies far
/// below the high one (more bits than any width).
#[test]
fn conversions_in_are_exact() {
    let mut rng = Rng::new(0xC0DE);
    let mut floats = vec![0.0, -0.0, ETA, f64::MIN_POSITIVE, f64::MAX, 1.0, 1.0 / 3.0];
    for _ in 0..2000 {
        let bits = rng.0.next_u64();
        let x = f64::from_bits(bits);
        if x.is_finite() {
            floats.push(x);
        }
    }
    for &x in &floats {
        for &w in &Width::ALL {
            let b = Big::from_f64(x, w);
            assert_eq!(b.w, w);
            assert!(qb(&b) == q(x), "{x:e}");
            assert_eq!(b.x.is_sign_negative(), x.is_sign_negative());
            // And back, exactly.
            assert_eq!(b.to_f64().to_bits(), x.to_bits());
        }
    }
    for (i, &hi) in floats.iter().enumerate() {
        let lo = floats[(i * 7 + 3) % floats.len()];
        let x = Dd::sum(hi, lo);
        if !x.is_finite() {
            continue;
        }
        let b = Big::from_dd(x, Width::MIN);
        assert!(qb(&b) == qd(x), "{x:?}");
        assert_eq!(b.to_f64().to_bits(), x.hi().to_bits(), "{x:?}");
    }
    let x = Dd::sum(f64::MAX, f64::MAX.next_down() * pow2(-60));
    let x = Dd::sum(x.hi(), ETA);
    let b = Big::from_dd(Dd::sum(pow2(1000), ETA), Width::MIN);
    assert!(qb(&b) == q(pow2(1000)) + q(ETA));
    assert!(b.x.get_prec().expect("finite") > 2000);
    assert!(qb(&Big::from_dd(x, Width::MIN)) == qd(x));
    // Specials.
    assert!(Big::from_f64(f64::NAN, Width::MIN).x.is_nan());
    assert!(!Big::from_f64(f64::INFINITY, Width::MIN).is_finite());
    assert_eq!(
        Big::from_f64(f64::NEG_INFINITY, Width::MIN).to_f64(),
        f64::NEG_INFINITY
    );
}

/// `1 ± 2^−k` with `k` bits, `⌊log₂⌋ = e`, as a big float.
fn near_one(k: i64, plus: bool, e: i64, w: Width) -> Big {
    let one = p2(e);
    let x = if plus {
        one.clone() + one * p2(-k)
    } else {
        one.clone() - one * p2(-k)
    };
    let (x, o) = Float::from_rational_prec_round(x, 4000, RoundingMode::Exact);
    assert_eq!(o, Ordering::Equal);
    Big { x, w }
}

/// The range is tested on the rounded result: an exact result just below
/// `2^−BF_EXP` that rounds up to it (also on a tie, to the even
/// `2^−BF_EXP`) is in range, one rounding below it is `±0`; one rounding
/// up to `2^(BF_EXP+1)` is `±∞`, while the top float of the range stays.
#[test]
fn the_range_is_tested_on_the_rounded_result() {
    let bottom = p2(-BF_EXP);
    for &w in &Width::ALL {
        let p = i64::from(w.bits());
        let half = Big::from_f64(0.5, w);
        for neg in [false, true] {
            let sign = |b: Big| if neg { b.neg() } else { b };
            // Two operands of the width: (1 + 2^(1−p))/2 · 2^(1−BF_EXP)·(1 − 2^(1−p))
            // = 2^−BF_EXP·(1 − 2^(2−2p)).
            let a = near_one(p - 1, true, -1, w);
            let b = sign(near_one(p - 1, false, -BF_EXP + 1, w));
            let r = a.mul(&b, w);
            conforms_exact(
                &|| "into range".into(),
                &r,
                &D::of(&a).mul(&D::of(&b)),
                w,
                neg,
            );
            assert_eq!(r.rounding, Rounding::Inexact, "{r:?}");
            assert!(qabs(qb(&r.v)) == bottom, "{r:?}");
            // A tie: 2^−BF_EXP·(1 − 2^−(p+1)) rounds to the even 2^−BF_EXP.
            let x = sign(near_one(p + 1, false, -BF_EXP + 1, w));
            let r = x.mul(&half, w);
            assert_eq!(r.rounding, Rounding::Inexact, "{r:?}");
            assert!(qabs(qb(&r.v)) == bottom, "{r:?}");
            // 2^−BF_EXP·(1 − 2^−p) is a float of the width, below the range.
            let x = sign(near_one(p, false, -BF_EXP + 1, w));
            let r = x.mul(&half, w);
            assert_eq!(r.rounding, Rounding::OutOfRange, "{r:?}");
            assert!(r.v.x.is_zero() && r.v.x.is_sign_negative() == neg, "{r:?}");
            // So is a difference cancelling below it.
            let a = sign(near_one(p - 1, true, -BF_EXP, w));
            let b = sign(Big {
                x: Float::power_of_2(-BF_EXP),
                w,
            });
            let r = a.sub(&b, w);
            assert_eq!(r.rounding, Rounding::OutOfRange, "{r:?}");
            assert!(r.v.x.is_zero() && r.v.x.is_sign_negative() == neg, "{r:?}");
            // At the top: 2^BF_EXP·(1 + 2^(1−p)) · (2 − 2^(2−p)) rounds to 2^(BF_EXP+1).
            let a = sign(near_one(p - 1, true, BF_EXP, w));
            let b = near_one(p - 1, false, 1, w);
            let r = a.mul(&b, w);
            assert_eq!(r.rounding, Rounding::OutOfRange, "{r:?}");
            assert!(
                r.v.x.is_infinite() && r.v.x.is_sign_negative() == neg,
                "{r:?}"
            );
            conforms_exact(&|| "to ∞".into(), &r, &D::of(&a).mul(&D::of(&b)), w, neg);
            // The top float of the range stays.
            let top = sign(near_one(p, false, BF_EXP + 1, w));
            let r = top.mul(&Big::from_f64(1.0, w), w);
            assert_eq!(r.rounding, Rounding::Exact, "{r:?}");
            // Narrowing a value just under 2^(BF_EXP+1) rounds it out.
            let x = sign(near_one(p + 1, false, BF_EXP + 1, w));
            let r = x.to_width(w);
            assert_eq!(r.rounding, Rounding::OutOfRange, "{r:?}");
            assert!(r.v.x.is_infinite(), "{r:?}");
            // Out of range is uncertified at the bounded level.
            let r = Bounded::exact(a) * Bounded::exact(b);
            assert!(!r.is_certified() && r.err_round == f64::INFINITY, "{r:?}");
            let bottom = Bounded::exact(sign(Big {
                x: Float::power_of_2(-BF_EXP),
                w,
            }));
            let r = &bottom * &bottom;
            assert!(!r.is_certified() && r.v.is_zero(), "{r:?}");
        }
    }
}

/// IEEE specials go through as IEEE 754 has them, uncertified.
#[test]
fn specials_follow_ieee() {
    let w = Width::MIN;
    let f = |x: f64| Big::from_f64(x, w);
    let (inf, zero, one) = (f(f64::INFINITY), f(0.0), f(1.0));
    assert!(inf.sub(&inf, w).v.x.is_nan());
    assert!(zero.div(&zero, w).v.x.is_nan());
    assert!(f(-1.0).sqrt(w).v.x.is_nan());
    assert_eq!(one.div(&zero, w).v.to_f64(), f64::INFINITY);
    assert_eq!(one.div(&f(-0.0), w).v.to_f64(), f64::NEG_INFINITY);
    assert!(f(-0.0).sqrt(w).v.x.is_sign_negative());
    assert!(!one.add(&one.neg(), w).v.x.is_sign_negative());
    assert!(f(-0.0).add(&f(-0.0), w).v.x.is_sign_negative());
    for x in [f64::INFINITY, f64::NAN] {
        let b = Bounded::exact(f(x));
        assert!(!b.is_certified() && b.sign().is_none());
        let one = Bounded::exact(one.clone());
        for r in [&b + &one, &b * &one, &one / &b, b.sqrt(), -&b] {
            assert!(!r.is_certified(), "{r:?}");
        }
    }
}

fn check(what: &str, r: &Bounded<Big>, exact: Option<&Q>) -> bool {
    assert!(
        r.err_trunc >= ETA && r.err_round >= ETA,
        "{what}: an error term below η: {r:?}"
    );
    holds(what, r, exact)
}

/// Checks a result against its exact value (`None` where the operation is
/// undefined at the true operands); returns whether it was certified.
fn holds(what: &str, r: &Bounded<Big>, exact: Option<&Q>) -> bool {
    if !r.is_certified() {
        return false;
    }
    let x = exact.unwrap_or_else(|| panic!("{what}: certified an undefined value {r:?}"));
    let dist = qabs(x.clone() - qb(&r.v));
    assert!(
        dist <= q(r.err_trunc) + q(r.err_round),
        "{what}: bound violated: {r:?}"
    );
    assert!(dist <= q(r.err()), "{what}: err() below the parts: {r:?}");
    if let Some(s) = r.sign() {
        assert_eq!(
            x.cmp(&Q::from(0u32)),
            s,
            "{what}: wrong certified sign {r:?}"
        );
    }
    assert!(
        r.abs_bound() == f64::INFINITY || q(r.abs_bound()) >= qabs(x.clone()),
        "{what}: abs_bound below the value {r:?}"
    );
    true
}

fn check_sqrt(r: &Bounded<Big>, x: &Q) -> bool {
    assert!(r.err_trunc >= ETA && r.err_round >= ETA, "sqrt: {r:?}");
    if !r.is_certified() {
        return false;
    }
    assert!(*x >= 0u32, "sqrt: certified the root of a negative {r:?}");
    let e = q(r.err_trunc) + q(r.err_round);
    let (lo, hi) = (qb(&r.v) - e.clone(), qb(&r.v) + e);
    assert!(*x <= hi.clone() * hi, "sqrt: root above the bound {r:?}");
    if lo >= 0u32 {
        assert!(lo.clone() * lo <= *x, "sqrt: root below the bound {r:?}");
    }
    true
}

fn quotient(a: &Q, b: &Q) -> Option<Q> {
    (*b != 0u32).then(|| a.clone() / b.clone())
}

/// How far the operands of the bound checks spread.
#[derive(Clone, Copy)]
enum Spread {
    /// Within `2^±40`.
    Moderate,
    /// Past the `f64` range both ways, to `2^±BEYOND`: every way an error
    /// term leaves the `f64` range shows there, while the exact values stay
    /// a few thousand bits. Operands at the big-float range's edges would
    /// cost rationals of a million bits each; [`Spread::Full`] is for the
    /// cheap checks, and the range is tested on its own.
    Beyond,
    /// Anywhere in the big-float range, its edges often ([`Rng::big`]).
    Full,
}

/// The exponent bound of [`Spread::Beyond`]: past `2^(1024 + 848)`, where
/// half an ulp at the widest width is past the `f64` range.
const BEYOND: i64 = 5000;

impl Rng {
    /// An error part for a value: zero, relative to it, tiny or wide.
    fn error(&mut self, v: &Big, wide: bool) -> f64 {
        let m = mag_up(v);
        let m = if m < f64::INFINITY { m } else { f64::MAX };
        match self.below(if wide { 6 } else { 5 }) {
            0 => 0.0,
            1 => m * pow2(-(self.range(150, 250))),
            2 | 3 => m * pow2(-(self.range(20, 60))),
            4 => ETA * self.below(8) as f64,
            _ => pow2(self.range(-1000, 1000)),
        }
    }

    /// An input and a true value inside its bounds: an end, the middle or
    /// a point between. Its value spread as `spread` says.
    fn input(&mut self, w: Width, spread: Spread) -> (Bounded<Big>, Q) {
        let moderate = |rng: &mut Rng, lo: i64, hi: i64| {
            let bits = rng.precision(w);
            let e = rng.range(lo, hi);
            Big {
                x: rng.float(bits, e),
                w,
            }
        };
        let v = match spread {
            Spread::Moderate => moderate(self, -40, 40),
            Spread::Beyond => match self.below(4) {
                0 => moderate(self, -60, 60),
                1 => moderate(self, -1080, 1030),
                _ => moderate(self, -BEYOND, BEYOND),
            },
            Spread::Full => self.big(w),
        };
        let wide = !matches!(spread, Spread::Moderate);
        let (t, r) = (self.error(&v, wide), self.error(&v, wide));
        let b = Bounded::new(v, t, r);
        let e = q(b.err_trunc) + q(b.err_round);
        let lambda = match self.below(4) {
            0 => q(-1.0),
            1 => q(1.0),
            2 => q(0.0),
            _ => q((self.below(2049) as f64 - 1024.0) / 1024.0),
        };
        let x = qb(&b.v) + lambda * e;
        (b, x)
    }
}

#[test]
fn operations_hold_their_bounds() {
    let mut certified = [0u32; 2];
    let mut total = [0u32; 2];
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..300 {
            let wide = case % 2 == 0;
            let spread = if wide {
                Spread::Beyond
            } else {
                Spread::Moderate
            };
            let wa = Width::ALL[rng.below(Width::ALL.len() as u64) as usize];
            let wb = if rng.below(3) == 0 {
                Width::ALL[rng.below(Width::ALL.len() as u64) as usize]
            } else {
                wa
            };
            let ((a, xa), (b, xb)) = (rng.input(wa, spread), rng.input(wb, spread));
            let results = [
                check("add", &(&a + &b), Some(&(xa.clone() + xb.clone()))),
                check("sub", &(&a - &b), Some(&(xa.clone() - xb.clone()))),
                check("mul", &(&a * &b), Some(&(xa.clone() * xb.clone()))),
                check("div", &(&a / &b), quotient(&xa, &xb).as_ref()),
                holds("neg", &-&a, Some(&-xa.clone())),
                holds("abs", &a.abs(), Some(&qabs(xa.clone()))),
                check_sqrt(&a.abs().sqrt(), &qabs(xa.clone())),
                check_sqrt(&a.sqrt(), &xa),
                // A conversion: the truncation error unchanged.
                holds("narrow", &a.to_width(Width::MIN), Some(&xa)),
            ];
            // Commutative, bit for bit.
            assert_eq!(&a + &b, &b + &a, "add: {a:?}, {b:?}");
            assert_eq!(&a * &b, &b * &a, "mul: {a:?}, {b:?}");
            let w = usize::from(wide);
            total[w] += results.len() as u32;
            certified[w] += results.iter().filter(|&&c| c).count() as u32;
        }
    }
    // Not vacuous.
    assert!(
        certified[0] * 10 > total[0] * 7,
        "{certified:?} of {total:?}"
    );
    assert!(
        certified[1] * 10 > total[1] * 3,
        "{certified:?} of {total:?}"
    );
}

/// The depth of [`chain`]'s expressions.
const CHAIN_DEPTH: u32 = 4;

/// A random expression of `+ − × ÷ √` over inputs at one width, with its
/// exact value (`None` once a true divisor is zero).
fn chain(rng: &mut Rng, depth: u32, w: Width, spread: Spread) -> (Bounded<Big>, Option<Q>) {
    if depth < CHAIN_DEPTH && (depth == 0 || rng.below(4) == 0) {
        let (b, x) = rng.input(w, spread);
        return (b, Some(x));
    }
    let (a, xa) = chain(rng, depth - 1, w, spread);
    let (b, xb) = chain(rng, depth - 1, w, spread);
    let both = xa.clone().zip(xb);
    let r = match rng.below(5) {
        0 => (&a + &b, both.map(|(x, y)| x + y)),
        1 => (&a - &b, both.map(|(x, y)| x - y)),
        2 => (&a * &b, both.map(|(x, y)| x * y)),
        3 => (&a / &b, both.and_then(|(x, y)| quotient(&x, &y))),
        _ => ((&a * &a).sqrt(), xa.map(qabs)),
    };
    check("chain", &r.0, r.1.as_ref());
    r
}

#[test]
fn chains_hold_their_bounds() {
    let mut certified = 0;
    let mut total = 0;
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..100 {
            let w = Width::ALL[case % Width::ALL.len()];
            let spread = if case % 3 == 0 {
                Spread::Beyond
            } else {
                Spread::Moderate
            };
            let (r, x) = chain(&mut rng, CHAIN_DEPTH, w, spread);
            total += 1;
            if check("chain", &r, x.as_ref()) {
                certified += 1;
            }
        }
    }
    assert!(certified * 2 > total, "{certified} of {total}");
}

/// Values beyond the `f64` range stay certified with small errors (while
/// half their ulp at the width is an `f64`, below `2^(1024 + bits)`):
/// their magnitudes enter the error products as mantissa and exponent.
/// Past that, a rounded result is uncertified.
#[test]
fn values_beyond_the_f64_range_stay_certified() {
    for &w in &Width::ALL {
        let mut rng = Rng::new(0xFA2);
        for _ in 0..50 {
            let e = rng.range(1030, 1010 + i64::from(w.bits()));
            let bits = rng.precision(w);
            let a = Big {
                x: rng.float(bits, e),
                w,
            };
            let b = Big {
                x: {
                    let eb = -e + rng.range(-20, 20);
                    rng.float(bits, eb)
                },
                w,
            };
            let c = Big {
                x: {
                    let ec = e - rng.range(20, 40);
                    rng.float(bits, ec)
                },
                w,
            };
            let (a, b, c) = (
                Bounded::new(a, 0.0, 1.0),
                Bounded::exact(b),
                Bounded::exact(c),
            );
            let (xa, xb, xc) = (qb(&a.v) + q(1.0), qb(&b.v), qb(&c.v));
            for (r, x) in [
                (&a * &b, xa.clone() * xb.clone()),
                (&a / &c, xa.clone() / xc.clone()),
                (&a - &a, Q::from(0u32)),
                (&a + &c, xa.clone() + xc.clone()),
            ] {
                assert!(r.is_certified(), "{r:?}");
                check("beyond", &r, Some(&x));
            }
        }
        // Half an ulp of an inexact sum at 2^(1024 + bits) is 2¹⁰²⁴: past
        // the top float.
        let big = Bounded::exact(Big {
            x: Float::power_of_2(1024 + i64::from(w.bits())),
            w,
        });
        let third = Bounded::exact(Big::from_f64(1.0 / 3.0, w));
        assert!(!(&big + &third).is_certified());
        assert!((&big + &big).is_certified(), "exact, so certified");
    }
}

/// A divisor or a root beyond the `f64` range, or an exact non-zero divisor
/// below it, bounds the quotient's and the root's errors through its
/// mantissa and exponent too: `2³⁰⁰⁰·(1 + 2⁻⁵⁰)` with an error of `2¹⁰⁰⁰`
/// over `2³⁰⁰⁰` is `1 + 2⁻⁵⁰` within about `2⁻²⁰⁰⁰`, and the root of
/// `(2⁶⁰⁰ + 2⁵⁰⁰)²` with an error of `2⁴⁰⁰` is within `2⁻¹⁵⁰`; dividing by an exact
/// `2^−BF_EXP` is exact.
#[test]
fn divisors_and_roots_beyond_the_f64_range_stay_certified() {
    let exact = |x: Q, w: Width| {
        let (x, o) = Float::from_rational_prec_round(x, u64::from(w.bits()), RoundingMode::Exact);
        assert_eq!(o, Ordering::Equal);
        Big { x, w }
    };
    let w = Width::MIN;
    let a = Bounded::new(exact(p2(3000) + p2(2950), w), 0.0, pow2(1000));
    let b = Bounded::exact(exact(p2(3000), w));
    let one = Bounded::exact(Big::from_f64(1.0, w));
    let x = (qb(&a.v) + q(pow2(1000))) / qb(&b.v);
    let r = &a / &b;
    check("quotient", &r, Some(&x));
    assert!(r.err() < pow2(-1000), "{r:?}");
    let d = &r - &one;
    check("quotient less one", &d, Some(&(x - Q::from(1u32))));
    assert_eq!(d.sign(), Some(Ordering::Greater), "{d:?}");

    let w = Width::MAX;
    let root = p2(600) + p2(500);
    let a = Bounded::new(exact(root.clone() * root.clone(), w), pow2(400), 0.0);
    let r = a.sqrt();
    check_sqrt(&r, &(qb(&a.v) + q(pow2(400))));
    assert!(r.err() < pow2(-150), "{r:?}");
    let near = root - p2(-30);
    let d = &r - &Bounded::exact(exact(near, w));
    assert_eq!(d.sign(), Some(Ordering::Greater), "{d:?}");

    for &w in &Width::ALL {
        let bottom = Bounded::exact(Big {
            x: Float::power_of_2(-BF_EXP),
            w,
        });
        let tiny = Bounded::exact(Big {
            x: Float::power_of_2(-2000i64),
            w,
        });
        for (b, x) in [(&bottom, p2(BF_EXP)), (&tiny, p2(2000))] {
            let r = &Bounded::exact(Big::from_f64(1.0, w)) / b;
            assert!(r.is_certified(), "{r:?}");
            check("over a tiny divisor", &r, Some(&x));
            let r = &Bounded::new(Big::from_f64(-1.0, w), 0.0, ETA) / b;
            assert!(
                !r.is_certified() || r.sign() == Some(Ordering::Less),
                "{r:?}"
            );
        }
    }
}

/// Promotion from double-double keeps the value, all its bits, and both
/// error parts, bit for bit; widening likewise. On exact moderate operands
/// each width's rounding error is about `2^−bits` of the result.
#[test]
fn promotion_is_exact_and_each_width_is_tighter() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for _ in 0..100 {
            let hi = f64::from_bits(rng.0.next_u64() >> 2 | 0x3000_0000_0000_0000);
            let lo = hi * pow2(-60 - rng.range(0, 900));
            let v = Dd::sum(hi, lo);
            let a = Bounded::new(v, ETA * 3.0, pow2(-rng.range(0, 1000)));
            let p = a.promote();
            assert!(qb(&p.v) == qd(v), "{v:?}");
            assert_eq!(p.v.w, Width::MIN);
            assert_eq!((p.err_trunc, p.err_round), (a.err_trunc, a.err_round));
            assert_eq!(Bounded::<Big>::from(a), p);
            for &w in &Width::ALL {
                let q2 = p.clone().widen(w);
                assert_eq!(q2.v.w, w);
                assert!(qb(&q2.v) == qd(v));
                assert_eq!((q2.err_trunc, q2.err_round), (a.err_trunc, a.err_round));
            }
            let x = Bounded::exact(v).promote();
            let y = Bounded::exact(Dd::from_f64(rng.range(1, 1000) as f64 / 7.0)).promote();
            let mut last = f64::INFINITY;
            for &w in &Width::ALL {
                let (x, y) = (x.clone().widen(w), y.clone().widen(w));
                for r in [&x + &y, &x * &y, &x / &y, x.abs().sqrt()] {
                    // Only floors of η, divided by at most 7.
                    assert!(r.err_trunc < 1e-300, "{r:?}");
                    let mag = mag_up(&r.v);
                    let bound = up(mag * w.unit_roundoff()) + 1e-300;
                    assert!(r.err_round <= bound, "{} bits: {r:?}", w.bits());
                }
                let r = &x / &y;
                assert!(r.err_round <= last, "{r:?}");
                last = r.err_round;
            }
        }
    }
}

#[test]
fn sign_needs_the_value_past_the_error() {
    let f = |x: f64| Big::from_f64(x, Width::MIN);
    assert_eq!(Bounded::exact(f(2.0)).sign(), Some(Ordering::Greater));
    assert_eq!(Bounded::exact(f(-ETA)).sign(), Some(Ordering::Less));
    assert_eq!(Bounded::exact(f(0.0)).sign(), None);
    assert_eq!(Bounded::new(f(1.0), 0.5, 0.5).sign(), None);
    assert_eq!(
        Bounded::new(f(-1.0), 0.25, 0.5).sign(),
        Some(Ordering::Less)
    );
    // A value below the `f64` range, exact, has a sign.
    let tiny = Big {
        x: Float::power_of_2(-5000i64),
        w: Width::MIN,
    };
    assert_eq!(Bounded::exact(tiny.clone()).sign(), Some(Ordering::Greater));
    assert_eq!(Bounded::new(tiny, 0.0, ETA).sign(), None);
    // The root of an exact zero is certified.
    let r = Bounded::exact(f(0.0)).sqrt();
    assert!(check_sqrt(&r, &Q::from(0u32)), "{r:?}");
    assert_eq!(Width::MAX.bits(), 848);
    assert_eq!(Width::from_bits(848), Some(Width::MAX));
    assert_eq!(Width::from_bits(2 * 848), None);
    assert_eq!(Width::MAX.next(), None);
    assert_eq!(Width::MAX.unit_roundoff(), pow2(-848));
    assert_eq!(Width::MIN.unit_roundoff(), pow2(-212));
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

    /// A value: its class and sign, then its integer mantissa (in hex,
    /// whatever the limb size) and exponent.
    fn big(&mut self, b: &Big) {
        let x = &b.x;
        self.u64(u64::from(b.w.bits()));
        self.u64(u64::from(x.is_sign_negative()));
        if x.is_nan() {
            self.u64(1);
        } else if x.is_infinite() {
            self.u64(2);
        } else if x.is_zero() {
            self.u64(3);
        } else {
            let (m, e) = x.integer_mantissa_and_exponent();
            self.bytes(format!("{m:x}").as_bytes());
            self.u64(e as u64);
        }
    }

    fn rounded(&mut self, r: &Rounded<Big>) {
        self.big(&r.v);
        self.u64(r.rounding as u64);
    }

    fn bounded(&mut self, r: &Bounded<Big>) {
        self.big(&r.v);
        self.u64(r.err_trunc.to_bits());
        self.u64(r.err_round.to_bits());
    }
}

/// The hash of every operation's bits at every width over a fixed probe
/// set (seeded, so independent of `VARDE_TESTS`), the bounded level's
/// error terms included. The same bits on every platform: `tools/ci.sh`
/// runs it natively and on wasm (`wasm32-wasip1`). A change of crate that
/// still meets the rule keeps the hash; a change of the error bounds
/// changes it, which then needs updating here.
#[test]
fn results_are_the_same_bits_everywhere() {
    let mut h = Hash(0xCBF2_9CE4_8422_2325);
    let mut rng = Rng::new(0x5EED);
    for &w in &Width::ALL {
        for case in 0..40 {
            let a = rng.big(w);
            let b = rng.partner(&a, w);
            for r in [
                a.add(&b, w),
                a.sub(&b, w),
                a.mul(&b, w),
                a.div(&b, w),
                a.sqrt(w),
                a.to_width(Width::MIN),
            ] {
                h.rounded(&r);
            }
            h.u64(a.to_f64().to_bits());
            let spread = if case % 2 == 0 {
                Spread::Full
            } else {
                Spread::Moderate
            };
            let ((x, _), (y, _)) = (rng.input(w, spread), rng.input(w, spread));
            for r in [
                &x + &y,
                &x - &y,
                &x * &y,
                &x / &y,
                x.sqrt(),
                x.to_width(Width::MIN),
            ] {
                h.bounded(&r);
            }
        }
    }
    let d = Bounded::new(Dd::sum(1.0 / 3.0, pow2(-80)), ETA, pow2(-110)).promote();
    h.bounded(&(&d * &d));
    println!("hash of the results: {:#018x}", h.0);
    assert_eq!(
        h.0, 0x74a2_c6af_935c_4323,
        "results differ from the reference bits"
    );
}

/// The root of a value non-negative by construction is certified however
/// near zero the value rounds, as at the first level, at every width.
#[test]
fn roots_of_values_non_negative_by_construction_are_certified() {
    for &w in &Width::ALL {
        let a = Bounded::new(Big::from_f64(1e-170, w), 0.0, 1e-170);
        let n2 = &(&a * &a) + &(&a * &a);
        let r = n2.sqrt_nonneg();
        for x in [q(0.0), q(2e-170) * q(2e-170) * q(2.0)] {
            assert!(check_sqrt(&r, &x), "{w:?} {x}: {r:?}");
        }
        for (v, e) in [(0.0, ETA), (-0.0, ETA), (-ETA, 2.0 * ETA), (-1e-20, 3e-20)] {
            let x = Bounded::new(Big::from_f64(v, w), 0.0, e);
            assert!(!x.sqrt().is_certified(), "{x:?}");
            let root = x.sqrt_nonneg();
            let hi = q(v) + q(e);
            for t in [q(0.0), qabs(q(v)), hi] {
                assert!(check_sqrt(&root, &t), "{w:?} {v} ± {e}: {root:?}");
            }
        }
        let x = Bounded::new(Big::from_f64(2.0, w), 0.0, 1.0);
        assert_eq!(x.sqrt_nonneg(), x.sqrt());
        let inf = Bounded::new(Big::from_f64(f64::INFINITY, w), 0.0, 0.0);
        assert!(!inf.sqrt_nonneg().is_certified());
    }
}
