//! Big-float, the precision levels above double-double: a value is a
//! binary float rounded to one of the schedule's widths, 212, 424 and 848
//! bits ([`Width`]), and [`Bounded<B>`] its values with
//! certified error bounds. A [`Bounded<Dd>`] moves up to the first width
//! exactly ([`Bounded::promote`]), and a value to a wider width exactly
//! ([`Bounded::widen`]).
//!
//! **Defined by a rule, not by a crate.** The [`BigF`] trait states what
//! every implementation must give, bit for bit:
//!
//! - every operation (`+ − × ÷ √`) and every conversion (from `f64` and
//!   double-double, to `f64`, between widths) is the exact result rounded
//!   to nearest, ties to even, at the operation's width, as with an
//!   unbounded exponent range, and with the signs of IEEE 754 (as MPFR):
//!   an exact zero sum is `+0` unless both terms are `−0`, products and
//!   quotients take the operands' signs, `√−0 = −0`, and `0/0`, `∞ − ∞`
//!   and the roots of negatives are NaN;
//! - the exponent range is fixed: magnitudes in `[2^−BF_EXP, 2^(BF_EXP+1))`
//!   ([`BF_EXP`] `= 2²⁰`), with no subnormals. The range is tested on the
//!   **rounded** result: one at or above `2^(BF_EXP+1)` becomes `±∞`, one
//!   below `2^−BF_EXP` becomes `±0`, both with the exact result's sign and
//!   reported out of range ([`Rounding::OutOfRange`]), which makes the
//!   item's error infinite (uncertified, but deterministic). A result just
//!   below `2^−BF_EXP` that rounds up to it is in range;
//! - conversion to `f64` rounds to nearest-even into `f64`'s own format
//!   (subnormals, overflow to `±∞`).
//!
//! An operation's inputs may carry more bits than its width (a promoted
//! double-double carries up to about 2100, exactly): only the result is
//! rounded. Any implementation meeting the rule gives the same bits, so
//! swapping the crate behind it changes no result, and the bits are the
//! same natively and on wasm. [`Big`] implements it over `malachite`'s
//! `Float` (correctly rounded, exponents to `±(2³⁰ − 1)`, so no result of
//! in-range operands leaves its range and the range test above is the only
//! one).
//!
//! **Error terms.** Both error parts stay `f64` upper bounds, rounded up
//! as at the other levels ([`up`]). No operation multiplies by the width's
//! unit roundoff `2^−bits`: an inexact result's own rounding is
//! half its ulp, `2^(⌊log₂|v|⌋ − bits)`, a power of two taken from the
//! result's exponent ([`pow2_up`]): exact when it is an `f64`, `η` (above
//! it) when it falls below `η`, `+∞` (uncertified) above the `f64` range.
//! An exact result adds no rounding of its own. Magnitudes that leave the
//! `f64` range enter products of error terms, and divisors and roots their
//! quotients, as mantissa and exponent ([`BigF::mag_up`],
//! [`BigF::mag_down`]), so a value beyond `2¹⁰²⁴` with a small error stays
//! certified, and so does a quotient over an exact divisor below `η`. For
//! algorithms bounding afterwards, [`Width::unit_roundoff`] gives an `f64`
//! at least `2^−bits` (the power itself at every width of the schedule).
//!
//! Since every error term is at least `η = 2⁻¹⁰⁷⁴`, a width's own rounding
//! shows only for magnitudes above about `2^(bits − 1074)`: `2⁻⁸⁶²` at 212
//! bits, `2⁻²²⁶` at 848. Below that it is the floor `η`, which is why the
//! schedule ends at 848 bits: a wider width would bound no smaller for
//! values of ordinary size.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use malachite::Float;
use malachite::base::num::basic::traits::{Infinity, NegativeInfinity, NegativeZero, Zero};
use malachite::base::num::comparison::traits::PartialOrdAbs;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Exact, Floor, Nearest};

use super::bounded::{Bounded, ETA, Level, add_up, div_up, down, least, mul_up, up};
use super::dd::Dd;

/// The exponent bound of the big-float levels, `2²⁰`: magnitudes lie in
/// `[2^−BF_EXP, 2^(BF_EXP+1))`, far beyond any value of a computation on
/// coordinates up to `1e6`.
pub const BF_EXP: i64 = 1 << 20;

/// A width of the precision schedule, `212·2ᵏ` bits for `k = 0..=2`: 212,
/// 424, 848 ([`Width::MAX`], the schedule's last).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Width(u8);

impl Width {
    /// The first big-float width, 212 bits (four times double's 53).
    pub const MIN: Width = Width(0);

    /// The last width of the schedule, 848 bits.
    pub const MAX: Width = Width(2);

    /// Every width, narrowest first.
    pub const ALL: [Width; 3] = [Width(0), Width(1), Width(2)];

    /// The number of significant bits.
    #[inline]
    pub const fn bits(self) -> u32 {
        212 << self.0
    }

    /// The next width up, `None` at [`Width::MAX`].
    #[inline]
    pub const fn next(self) -> Option<Width> {
        if self.0 < Width::MAX.0 {
            Some(Width(self.0 + 1))
        } else {
            None
        }
    }

    /// The width of a number of bits, if it is one of the schedule's.
    pub fn from_bits(bits: u32) -> Option<Width> {
        Width::ALL.into_iter().find(|w| w.bits() == bits)
    }

    /// An `f64` at least the unit roundoff `2^−bits`: the power itself at
    /// every width of the schedule.
    #[inline]
    pub fn unit_roundoff(self) -> f64 {
        pow2_up(-i64::from(self.bits()))
    }

    /// The wider of two widths.
    #[inline]
    fn wider(self, o: Width) -> Width {
        if o > self { o } else { self }
    }
}

/// How a big-float result came about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// The exact result (or an IEEE special: a NaN, an infinity from an
    /// infinite operand or a zero divisor).
    Exact,
    /// Rounded to nearest at the width, in range.
    Inexact,
    /// The rounded result left the exponent range: `±∞` or `±0` with the
    /// exact result's sign. Its error is infinite.
    OutOfRange,
}

/// A big-float result and how it was rounded.
#[derive(Clone, Debug, PartialEq)]
pub struct Rounded<B> {
    /// The result.
    pub v: B,
    /// Whether it is exact, rounded, or out of range.
    pub rounding: Rounding,
}

/// A big-float format meeting the rule in the [module docs](self): every
/// operation and conversion correctly rounded to nearest-even at its width
/// over the fixed exponent range `±BF_EXP`, the range tested on the
/// rounded result. A value knows its width; an operation's width is given
/// ([`Bounded<B>`]'s take the wider operand's).
pub trait BigF: Clone + fmt::Debug + PartialEq + Level + Sized {
    /// A float at width `w`, exactly.
    fn from_f64(x: f64, w: Width) -> Self;

    /// A double-double's value `hi + lo` at width `w`, exactly (it may
    /// carry more bits than `w`).
    fn from_dd(x: Dd, w: Width) -> Self;

    /// The width the value belongs to.
    fn width(&self) -> Width;

    /// The value at width `w`: rounded to `w` bits when it carries more,
    /// else exactly.
    fn to_width(&self, w: Width) -> Rounded<Self>;

    /// The value at a width `w` at least its own, exactly: its bits kept
    /// (all of them, even past `w`'s, as a promoted value's).
    fn widen(&self, w: Width) -> Self;

    /// The value rounded to the nearest `f64`, ties to even (subnormals
    /// included; `±∞` past the top float).
    fn to_f64(&self) -> f64;

    /// `−x`, exactly.
    fn neg(&self) -> Self;

    /// `|x|`, exactly.
    fn abs(&self) -> Self;

    /// `x + y` rounded at `w`.
    fn add(&self, y: &Self, w: Width) -> Rounded<Self>;

    /// `x − y` rounded at `w`.
    fn sub(&self, y: &Self, w: Width) -> Rounded<Self>;

    /// `x · y` rounded at `w`.
    fn mul(&self, y: &Self, w: Width) -> Rounded<Self>;

    /// `x / y` rounded at `w`.
    fn div(&self, y: &Self, w: Width) -> Rounded<Self>;

    /// `√x` rounded at `w`.
    fn sqrt(&self, w: Width) -> Rounded<Self>;

    /// Whether the value is `±0`.
    fn is_zero(&self) -> bool;

    /// Whether the sign bit is set (`−0` included).
    fn is_sign_negative(&self) -> bool;

    /// `⌊log₂|x|⌋` for a finite non-zero value.
    fn exponent(&self) -> Option<i64>;

    /// `(m, k)` with `|x| ≤ m·2ᵏ`, `m` an `f64` in `[1, 2)` (the mantissa
    /// rounded up), for a finite non-zero value.
    fn mag_up(&self) -> Option<(f64, i64)>;

    /// `(m, k)` with `|x| ≥ m·2ᵏ`, `m` an `f64` in `[1, 2)` (the mantissa
    /// rounded down), for a finite non-zero value.
    fn mag_down(&self) -> Option<(f64, i64)>;

    /// `|x|` against `e ≥ 0`, exactly; `None` for a NaN.
    fn cmp_abs(&self, e: f64) -> Option<Ordering>;
}

/// The big-float level over `malachite::Float`: a value and its width.
#[derive(Clone)]
pub struct Big {
    x: Float,
    w: Width,
}

impl Big {
    /// The value, as a `malachite` float.
    #[inline]
    pub fn as_float(&self) -> &Float {
        &self.x
    }

    /// A `malachite` float at width `w`, rounded to it ([`BigF::to_width`]).
    pub fn from_float(x: Float, w: Width) -> Rounded<Big> {
        Big { x, w }.to_width(w)
    }

    /// The range test on a rounded result `x` of the exact result's sign:
    /// see the [module docs](self).
    fn rounded(x: Float, o: Ordering, w: Width) -> Rounded<Big> {
        let rounding = match x.get_exponent() {
            Some(e) => {
                let e = i64::from(e) - 1;
                if e > BF_EXP {
                    let inf = if x.is_sign_negative() {
                        Float::NEGATIVE_INFINITY
                    } else {
                        Float::INFINITY
                    };
                    return out_of_range(inf, w);
                }
                if e < -BF_EXP {
                    let zero = if x.is_sign_negative() {
                        Float::NEGATIVE_ZERO
                    } else {
                        Float::ZERO
                    };
                    return out_of_range(zero, w);
                }
                if o == Ordering::Equal {
                    Rounding::Exact
                } else {
                    Rounding::Inexact
                }
            }
            None => Rounding::Exact,
        };
        Rounded {
            v: Big { x, w },
            rounding,
        }
    }
}

fn out_of_range(x: Float, w: Width) -> Rounded<Big> {
    Rounded {
        v: Big { x, w },
        rounding: Rounding::OutOfRange,
    }
}

impl fmt::Debug for Big {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Big({:#x} @ {})", self.x, self.w.bits())
    }
}

/// The same value (the sign of a zero included, any NaN equal to any
/// other) at the same width; the number of bits carried doesn't count.
impl PartialEq for Big {
    fn eq(&self, o: &Big) -> bool {
        self.w == o.w
            && if self.x.is_nan() || o.x.is_nan() {
                self.x.is_nan() && o.x.is_nan()
            } else {
                self.x == o.x && self.x.is_sign_negative() == o.x.is_sign_negative()
            }
    }
}

impl Level for Big {
    #[inline]
    fn is_finite(&self) -> bool {
        self.x.is_finite()
    }
}

impl BigF for Big {
    fn from_f64(x: f64, w: Width) -> Big {
        Big {
            x: Float::from(x),
            w,
        }
    }

    fn from_dd(x: Dd, w: Width) -> Big {
        let (hi, lo) = (Float::from(x.hi()), Float::from(x.lo()));
        let x = if !x.is_finite() {
            // IEEE's sum of the parts, so a value that isn't finite stays
            // so (and uncertified) whichever part carries it.
            Float::from(x.hi() + x.lo())
        } else if x.lo() == 0.0 {
            hi
        } else {
            // The exact sum: its bits span at most from 2¹⁰²³ down to
            // 2⁻¹⁰⁷⁴, then trimmed to those it needs.
            let (s, _) = hi.add_prec_round(lo, 2200, Exact);
            let prec = s.get_min_prec().unwrap_or(1);
            Float::from_float_prec_round(s, prec, Exact).0
        };
        Big { x, w }
    }

    #[inline]
    fn width(&self) -> Width {
        self.w
    }

    fn to_width(&self, w: Width) -> Rounded<Big> {
        let (x, o) = Float::from_float_prec_round_ref(&self.x, u64::from(w.bits()), Nearest);
        Big::rounded(x, o, w)
    }

    fn widen(&self, w: Width) -> Big {
        Big {
            x: self.x.clone(),
            w,
        }
    }

    fn to_f64(&self) -> f64 {
        f64::rounding_from(&self.x, Nearest).0
    }

    fn neg(&self) -> Big {
        Big {
            x: -&self.x,
            w: self.w,
        }
    }

    fn abs(&self) -> Big {
        if self.x.is_sign_negative() {
            self.neg()
        } else {
            self.clone()
        }
    }

    fn add(&self, y: &Big, w: Width) -> Rounded<Big> {
        let (x, o) = self.x.add_prec_round_ref_ref(&y.x, prec(w), Nearest);
        Big::rounded(x, o, w)
    }

    fn sub(&self, y: &Big, w: Width) -> Rounded<Big> {
        let (x, o) = self.x.sub_prec_round_ref_ref(&y.x, prec(w), Nearest);
        Big::rounded(x, o, w)
    }

    fn mul(&self, y: &Big, w: Width) -> Rounded<Big> {
        let (x, o) = self.x.mul_prec_round_ref_ref(&y.x, prec(w), Nearest);
        Big::rounded(x, o, w)
    }

    fn div(&self, y: &Big, w: Width) -> Rounded<Big> {
        let (x, o) = self.x.div_prec_round_ref_ref(&y.x, prec(w), Nearest);
        Big::rounded(x, o, w)
    }

    fn sqrt(&self, w: Width) -> Rounded<Big> {
        let (x, o) = self.x.sqrt_prec_round_ref(prec(w), Nearest);
        Big::rounded(x, o, w)
    }

    #[inline]
    fn is_zero(&self) -> bool {
        self.x.is_zero()
    }

    #[inline]
    fn is_sign_negative(&self) -> bool {
        self.x.is_sign_negative()
    }

    fn exponent(&self) -> Option<i64> {
        self.x.get_exponent().map(|e| i64::from(e) - 1)
    }

    fn mag_up(&self) -> Option<(f64, i64)> {
        self.x
            .sci_mantissa_and_exponent_round::<f64>(Ceiling)
            .map(|(m, e, _)| (m, i64::from(e)))
    }

    fn mag_down(&self) -> Option<(f64, i64)> {
        self.x
            .sci_mantissa_and_exponent_round::<f64>(Floor)
            .map(|(m, e, _)| (m, i64::from(e)))
    }

    fn cmp_abs(&self, e: f64) -> Option<Ordering> {
        self.x.partial_cmp_abs(&e)
    }
}

#[inline]
fn prec(w: Width) -> u64 {
    u64::from(w.bits())
}

/// `2ᵏ` for `k` in the normal range, `−1022..=1023`.
fn pow2(k: i64) -> f64 {
    debug_assert!((-1022..=1023).contains(&k));
    f64::from_bits(((1023 + k) as u64) << 52)
}

/// An upper bound of `2ᵏ` as an `f64`: exact from `2⁻¹⁰⁷⁴` to `2¹⁰²³`,
/// `η` below, `+∞` above.
pub fn pow2_up(k: i64) -> f64 {
    if k > 1023 {
        f64::INFINITY
    } else if k >= -1022 {
        pow2(k)
    } else if k >= -1074 {
        f64::from_bits(1 << (k + 1074))
    } else {
        ETA
    }
}

/// `(m, e)` with `t = m·2ᵉ` exactly, `m` in `[1, 2)`, for a finite
/// `t > 0`.
fn frexp(t: f64) -> (f64, i64) {
    let bits = t.to_bits();
    let field = (bits >> 52) & 0x7FF;
    if field == 0 {
        // Subnormal: scaled up exactly into the normal range.
        let (m, e) = frexp(t * pow2(64));
        return (m, e - 64);
    }
    let m = f64::from_bits((bits & ((1 << 52) - 1)) | (1023 << 52));
    (m, field as i64 - 1023)
}

/// `m·2ᵉ` for `m` in `[1, 4)`, `e` in `−1080..=1023`: exact in the normal
/// range, rounded once (within `η/2`) below it; may overflow to `+∞`.
fn ldexp(m: f64, e: i64) -> f64 {
    if e >= -1022 {
        m * pow2(e)
    } else {
        // Exact into the normal range, then one rounding.
        (m * pow2(e + 600)) * pow2(-600)
    }
}

/// An upper bound of `t·2ᵏ` for `t ≥ 0`, at least `η`; `+∞` for an
/// infinite `t` or past the `f64` range.
fn scale_up(t: f64, k: i64) -> f64 {
    if t == 0.0 {
        return ETA;
    }
    if t.is_nan() || t == f64::INFINITY {
        return f64::INFINITY;
    }
    let (m, e) = frexp(t);
    let e = e.saturating_add(k);
    if e > 1023 {
        f64::INFINITY
    } else if e < -1080 {
        // Below 2⁻¹⁰⁷⁸.
        ETA
    } else {
        up(ldexp(m, e))
    }
}

/// An upper bound of `|x|`: `0` for a zero, `+∞` unless finite or past
/// the `f64` range.
fn mag_up<B: BigF>(x: &B) -> f64 {
    match x.mag_up() {
        Some((m, k)) => scale_up(m, k),
        None if x.is_zero() => 0.0,
        None => f64::INFINITY,
    }
}

/// A positive lower bound `m·2ᵏ` (`m` in `[1, 2)`) of `|x| − d` for an
/// error term `d ≥ 0`, through `x`'s mantissa and exponent, so for a
/// magnitude beyond the `f64` range or below it as well; `None` unless
/// `x` is finite and non-zero and the bound positive.
fn mag_less_down<B: BigF>(x: &B, d: f64) -> Option<(f64, i64)> {
    let (m, k) = x.mag_down()?;
    if d == 0.0 {
        return Some((m, k));
    }
    // `d·2^−k`, rounded up: `|x| − d ≥ (m − s)·2ᵏ`.
    let s = scale_up(d, -k);
    let t = down(m - s);
    if t > 0.0 {
        let (m, e) = frexp(t);
        Some((m, k + e))
    } else {
        None
    }
}

/// An upper bound of `e / (m·2ᵏ)` for an error term `e ≥ 0` and `m` in
/// `[1, 2)`, at least `η`; `+∞` past the `f64` range.
fn div_mag_up(e: f64, (m, k): (f64, i64)) -> f64 {
    if e == 0.0 {
        ETA
    } else {
        scale_up(div_up(e, m), -k)
    }
}

/// An upper bound of `|x|·e` for an error term `e ≥ 0`, at least `η`
/// (as [`mul_up`]): through `x`'s mantissa and exponent, so a magnitude
/// beyond the `f64` range times a small error stays finite.
fn mag_mul_up<B: BigF>(x: &B, e: f64) -> f64 {
    if e.is_nan() || e == f64::INFINITY {
        return f64::INFINITY;
    }
    if e == 0.0 || x.is_zero() {
        return ETA;
    }
    match x.mag_up() {
        Some((m, k)) => {
            let (me, ke) = frexp(e);
            scale_up(mul_up(m, me), k + ke)
        }
        None => f64::INFINITY,
    }
}

/// A result's own rounding error: zero when exact, half its ulp at its
/// width when rounded, `+∞` out of range or for a value that isn't
/// finite.
fn own<B: BigF>(r: &Rounded<B>) -> f64 {
    if !r.v.is_finite() {
        return f64::INFINITY;
    }
    match r.rounding {
        Rounding::Exact => 0.0,
        Rounding::Inexact => match r.v.exponent() {
            Some(e) => pow2_up(e - i64::from(r.v.width().bits())),
            None => 0.0,
        },
        Rounding::OutOfRange => f64::INFINITY,
    }
}

impl Bounded<Dd> {
    /// The same value and errors at the first big-float width, exactly
    /// (promotion is exact: the value keeps all its bits).
    pub fn promote(self) -> Bounded<Big> {
        Bounded::new(
            Big::from_dd(self.v, Width::MIN),
            self.err_trunc,
            self.err_round,
        )
    }
}

impl From<Bounded<Dd>> for Bounded<Big> {
    #[inline]
    fn from(b: Bounded<Dd>) -> Self {
        b.promote()
    }
}

impl<B: BigF> Bounded<B> {
    /// The same value and errors at a width `w` at least the value's,
    /// exactly (promotion is exact); a narrower `w` is ignored.
    pub fn widen(self, w: Width) -> Self {
        let w = self.v.width().wider(w);
        Bounded {
            v: self.v.widen(w),
            ..self
        }
    }

    /// The value rounded to width `w`, its rounding added to the rounding
    /// error.
    pub fn to_width(&self, w: Width) -> Self {
        let r = self.v.to_width(w);
        let e = own(&r);
        Bounded {
            v: r.v,
            err_trunc: self.err_trunc,
            err_round: add_up(self.err_round, e),
        }
    }

    /// The certified sign of the true value: `Some` when `|v| > err`
    /// (compared exactly), `None` when the error doesn't settle it (and for
    /// a value that isn't finite).
    pub fn sign(&self) -> Option<Ordering> {
        let e = self.err();
        let settled = self.v.is_finite()
            && if e == 0.0 {
                !self.v.is_zero()
            } else {
                self.v.cmp_abs(e) == Some(Ordering::Greater)
            };
        settled.then(|| {
            if self.v.is_sign_negative() {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        })
    }

    /// An upper bound of the true value's magnitude, `|v| + err` (`+∞`
    /// past the `f64` range).
    #[inline]
    pub fn abs_bound(&self) -> f64 {
        add_up(mag_up(&self.v), self.err())
    }

    /// The absolute value: exact, the errors unchanged.
    pub fn abs(&self) -> Self {
        Bounded {
            v: self.v.abs(),
            ..self.clone()
        }
    }

    /// The square root at the value's width. Uncertified unless the true
    /// value is certainly `≥ 0` (`v ≥ err`, compared exactly). The true
    /// root is within `e / (√x + √v) ≤ min(e / √v, √e)` of `√v` for an
    /// error `e`, with `√v` bounded below by the rounded root less its
    /// rounding (as mantissa and exponent); each part takes that bound of
    /// its own error.
    pub fn sqrt(&self) -> Self {
        let x = &self.v;
        let e = self.err();
        let non_negative = x.is_finite()
            && if x.is_zero() {
                e == 0.0
            } else {
                !x.is_sign_negative() && x.cmp_abs(e) != Some(Ordering::Less)
            };
        if !non_negative {
            let r = x.sqrt(x.width());
            return Bounded::new(r.v, f64::INFINITY, f64::INFINITY);
        }
        self.root()
    }

    /// The square root of a value whose true value is `≥ 0` by
    /// construction (a sum of squares), certified whatever its error, as
    /// at the first level ([`Bounded::<f64>::sqrt_nonneg`]): a value at or
    /// below zero is taken as `+0`. Uncertified only for a value or error
    /// that isn't finite.
    pub fn sqrt_nonneg(&self) -> Self {
        let x = &self.v;
        if !x.is_finite() {
            let r = x.sqrt(x.width());
            return Bounded::new(r.v, f64::INFINITY, f64::INFINITY);
        }
        if x.is_zero() || x.is_sign_negative() {
            let zero = Bounded {
                v: B::from_f64(0.0, x.width()),
                ..self.clone()
            };
            return zero.root();
        }
        self.root()
    }

    /// The root of a finite `v ≥ 0` (see [`sqrt`](Self::sqrt)).
    fn root(&self) -> Self {
        let x = &self.v;
        let r = x.sqrt(x.width());
        let own = own(&r);
        let root_lo = mag_less_down(&r.v, own);
        let part = |e: f64| {
            let by_root = up(e.sqrt());
            match root_lo {
                Some(lo) => least(div_mag_up(e, lo), by_root),
                None => by_root,
            }
        };
        Bounded {
            v: r.v,
            err_trunc: part(self.err_trunc),
            err_round: add_up(part(self.err_round), own),
        }
    }
}

impl<B: BigF> Neg for &Bounded<B> {
    type Output = Bounded<B>;

    /// Exact: the errors unchanged.
    fn neg(self) -> Bounded<B> {
        Bounded {
            v: self.v.neg(),
            ..self.clone()
        }
    }
}

impl<B: BigF> Neg for Bounded<B> {
    type Output = Bounded<B>;

    #[inline]
    fn neg(self) -> Bounded<B> {
        -&self
    }
}

impl<B: BigF> Add for &Bounded<B> {
    type Output = Bounded<B>;

    /// Rounded at the wider operand's width.
    fn add(self, o: &Bounded<B>) -> Bounded<B> {
        let r = self.v.add(&o.v, self.v.width().wider(o.v.width()));
        let own = own(&r);
        Bounded {
            v: r.v,
            err_trunc: add_up(self.err_trunc, o.err_trunc),
            err_round: add_up(add_up(self.err_round, o.err_round), own),
        }
    }
}

impl<B: BigF> Sub for &Bounded<B> {
    type Output = Bounded<B>;

    /// Rounded at the wider operand's width; the same bits as
    /// `self + (−o)`.
    fn sub(self, o: &Bounded<B>) -> Bounded<B> {
        let r = self.v.sub(&o.v, self.v.width().wider(o.v.width()));
        let own = own(&r);
        Bounded {
            v: r.v,
            err_trunc: add_up(self.err_trunc, o.err_trunc),
            err_round: add_up(add_up(self.err_round, o.err_round), own),
        }
    }
}

impl<B: BigF> Mul for &Bounded<B> {
    type Output = Bounded<B>;

    /// Rounded at the wider operand's width. As at the other levels:
    /// `|xa·xb − va·vb| ≤ |va|·eb + |vb|·ea + ea·eb`, the terms of
    /// truncation alone truncation, the rest rounding; `|v|·e` through
    /// `v`'s mantissa and exponent. Commutative, bit for bit.
    fn mul(self, o: &Bounded<B>) -> Bounded<B> {
        let (a, b) = (self, o);
        let r = a.v.mul(&b.v, a.v.width().wider(b.v.width()));
        let err_trunc = add_up(
            add_up(mag_mul_up(&a.v, b.err_trunc), mag_mul_up(&b.v, a.err_trunc)),
            mul_up(a.err_trunc, b.err_trunc),
        );
        let linear = add_up(mag_mul_up(&a.v, b.err_round), mag_mul_up(&b.v, a.err_round));
        let cross = add_up(
            mul_up(a.err_round, b.err_round),
            add_up(
                mul_up(a.err_round, b.err_trunc),
                mul_up(a.err_trunc, b.err_round),
            ),
        );
        let own = own(&r);
        Bounded {
            v: r.v,
            err_trunc,
            err_round: add_up(add_up(linear, cross), own),
        }
    }
}

impl<B: BigF> Div for &Bounded<B> {
    type Output = Bounded<B>;

    /// Rounded at the wider operand's width. As at the other levels:
    /// `|xa/xb − va/vb| ≤ (ea + |va/vb|·eb) / |xb|` with
    /// `|xb| ≥ |vb| − eb`, each part over that lower bound, and
    /// `|va/vb| ≤ |v| + own`. Uncertified unless the divisor is finite and
    /// certainly non-zero, `|vb| − eb > 0`. The lower bound is a mantissa
    /// and an exponent, so a divisor beyond the `f64` range, or an exact
    /// one below it, keeps the quotient's errors small.
    fn div(self, o: &Bounded<B>) -> Bounded<B> {
        let (a, b) = (self, o);
        let r = a.v.div(&b.v, a.v.width().wider(b.v.width()));
        // False for a NaN too.
        let den = match mag_less_down(&b.v, b.err()) {
            Some(den) if b.v.is_finite() => den,
            _ => return Bounded::new(r.v, f64::INFINITY, f64::INFINITY),
        };
        let own = own(&r);
        // `ea + |va/vb|·eb`, the exact quotient of the values at most
        // `|v| + own`, term by term; just `ea` for an exact divisor (no `η`
        // added, which over a divisor below the `f64` range is large).
        let num = |ea: f64, eb: f64| {
            if eb == 0.0 {
                ea
            } else {
                add_up(ea, add_up(mag_mul_up(&r.v, eb), mul_up(own, eb)))
            }
        };
        let part = |ea: f64, eb: f64| div_mag_up(num(ea, eb), den);
        Bounded {
            err_trunc: part(a.err_trunc, b.err_trunc),
            err_round: add_up(part(a.err_round, b.err_round), own),
            v: r.v,
        }
    }
}

macro_rules! by_value {
    ($tr:ident, $f:ident) => {
        impl<B: BigF> $tr for Bounded<B> {
            type Output = Bounded<B>;

            #[inline]
            fn $f(self, o: Bounded<B>) -> Bounded<B> {
                (&self).$f(&o)
            }
        }
    };
}

by_value!(Add, add);
by_value!(Sub, sub);
by_value!(Mul, mul);
by_value!(Div, div);

#[cfg(test)]
mod tests;
