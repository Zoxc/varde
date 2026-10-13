//! The scalar of a Bernstein form: a precision level's [`Bounded`] value
//! (`f64`, double-double, big-float) or an exact `malachite` [`Rational`].

use std::cmp::Ordering;
use std::fmt;

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor};
use smallvec::SmallVec;

use crate::num::bigf::BigF;
use crate::num::bounded::{Bounded, ETA, U, add_up, down, gamma_bound, mul_up};
use crate::num::dd::Dd;

/// A coefficient of a Bernstein form: `+ − × ÷` and the midpoint, each
/// carrying its error bound (a [`Bounded`] level) or exact
/// ([`Rational`]), and a certified `f64` enclosure of the true value.
///
/// [`mid`](Scalar::mid) must be symmetric to the bit,
/// `a.mid(&b) == b.mid(&a)`, error parts included: the split at ½ relies
/// on it for the same bits under reversal.
///
/// Equality is of the representation, error parts included: a value
/// equal to [`int(k)`](Scalar::int) is exactly `k`.
pub trait Scalar: Clone + fmt::Debug + PartialEq {
    /// The integer `k` exactly, at `like`'s level (and width).
    fn int(k: u32, like: &Self) -> Self;

    /// The integer `k` at `like`'s level (and width), certified: exact
    /// at every level but the first, and there for `k` up to `2⁵³` (or
    /// with at most 53 significant bits); else rounded to nearest, the
    /// rounding (at most `2¹⁰`) its error. Binomial weights pass `2⁵³`
    /// from degree 57 on (`C(57, 28)`).
    fn int_u64(k: u64, like: &Self) -> Self;

    /// `self + o`.
    fn add(&self, o: &Self) -> Self;

    /// `self − o`.
    fn sub(&self, o: &Self) -> Self;

    /// `self · o`.
    fn mul(&self, o: &Self) -> Self;

    /// `self / o`; uncertified unless `o` is certainly non-zero (a
    /// [`Rational`] panics on zero, as its division does).
    fn div(&self, o: &Self) -> Self;

    /// `−self`, exactly.
    fn neg(&self) -> Self;

    /// `(self + o) / 2`, the same bits as `o.mid(self)`.
    fn mid(&self, o: &Self) -> Self;

    /// `[lo, hi]` with the true value in it: `[−∞, +∞]` for a value that
    /// isn't certified.
    fn enclose(&self) -> [f64; 2];

    /// The certified sign of the true value: `Some` when it is certainly
    /// non-zero (`|v| > err`, compared exactly, so also for a big-float
    /// value below the `f64` range), `None` when the error doesn't settle
    /// it, for an exact zero and for a value that isn't certified.
    fn sign(&self) -> Option<Ordering>;

    /// The finite `x` exactly, at `like`'s level (and width).
    fn from_f64(x: f64, like: &Self) -> Self;

    /// The polynomial with Bernstein coefficients `c` (non-empty) at `t`
    /// by a level's own faster method, if it has one for this `t`; `None`
    /// leaves it to de Casteljau in this type's operations.
    fn eval_fast(c: &[Self], t: &Self) -> Option<Self> {
        let _ = (c, t);
        None
    }

    /// Whether this is the integer `k` exactly, with no error (as a
    /// parameter, a certain end of `[0, 1]` for `k` 0 or 1). A zero of
    /// either sign is the integer 0.
    #[inline]
    fn is_int(&self, k: u32) -> bool {
        *self == Self::int(k, self)
    }
}

/// `e/2` rounded up, zero kept zero.
#[inline]
fn half_up(e: f64) -> f64 {
    if e == 0.0 { 0.0 } else { mul_up(e, 0.5) }
}

/// The larger of two error terms by compare and select, `+∞` for a NaN.
#[inline]
fn most(a: f64, e: f64) -> f64 {
    if e.is_nan() {
        f64::INFINITY
    } else if e > a {
        e
    } else {
        a
    }
}

/// An upper bound of the exact value `t` was rounded from (any sign),
/// the counterpart of [`down`].
#[inline]
fn up_signed(t: f64) -> f64 {
    -down(-t)
}

/// The enclosure of a value `v` within `e` of the true one, from a lower
/// and an upper `f64` bound of `v` itself.
fn widen_by(lo: f64, hi: f64, e: f64) -> [f64; 2] {
    let r = [down(lo - e), up_signed(hi + e)];
    if r[0].is_nan() || r[1].is_nan() {
        [f64::NEG_INFINITY, f64::INFINITY]
    } else {
        r
    }
}

/// `k = hi + lo` exactly: `hi` the nearest `f64`, `lo` the rest, at
/// most half an ulp of `hi` (`2¹⁰`), so itself exact.
fn split_u64(k: u64) -> (f64, f64) {
    // Rounds to nearest; `hi ≤ 2⁶⁴`, which `u128` holds.
    let hi = k as f64;
    let rest = i128::from(k) - hi as u128 as i128;
    // `|rest| ≤ 2¹⁰`: exact.
    (hi, rest as f64)
}

const UNBOUNDED: [f64; 2] = [f64::NEG_INFINITY, f64::INFINITY];

impl Scalar for Bounded<f64> {
    #[inline]
    fn int(k: u32, _: &Self) -> Self {
        Bounded::exact(f64::from(k))
    }

    fn int_u64(k: u64, _: &Self) -> Self {
        let (hi, lo) = split_u64(k);
        Bounded::new(hi, 0.0, lo.abs())
    }

    #[inline]
    fn add(&self, o: &Self) -> Self {
        *self + *o
    }

    #[inline]
    fn sub(&self, o: &Self) -> Self {
        *self - *o
    }

    #[inline]
    fn mul(&self, o: &Self) -> Self {
        *self * *o
    }

    #[inline]
    fn div(&self, o: &Self) -> Self {
        *self / *o
    }

    #[inline]
    fn neg(&self) -> Self {
        -*self
    }

    /// The sum halved: exact but below the normal range, where it rounds
    /// by at most `η/2` (the `η` added).
    fn mid(&self, o: &Self) -> Self {
        let s = *self + *o;
        Bounded::new(
            s.v * 0.5,
            half_up(s.err_trunc),
            add_up(half_up(s.err_round), ETA),
        )
    }

    fn enclose(&self) -> [f64; 2] {
        if !self.is_certified() {
            return UNBOUNDED;
        }
        let e = self.err();
        if e == 0.0 {
            return [self.v, self.v];
        }
        widen_by(self.v, self.v, e)
    }

    #[inline]
    fn sign(&self) -> Option<Ordering> {
        Bounded::<f64>::sign(self)
    }

    #[inline]
    fn from_f64(x: f64, _: &Self) -> Self {
        Bounded::exact(x)
    }

    /// For an exact `t` in `[0, 1]`: de Casteljau in plain floats with
    /// weights `s = fl(1 − t)` and `t`, bounded afterwards by
    /// [`gamma_bound`] from the same evaluation on the `|bᵢ|`. That bounds
    /// the error against the homogeneous form at the floats `(s, t)`, `2n`
    /// roundings per term. When `s` isn't `1 − t` exactly (`t < ½`), it is
    /// `(1 − t)·(1 + δ)` with `|δ| ≤ u`, which moves each term by a factor
    /// `(1 + δ)^±k`, `k ≤ n`, so `n` more roundings (`m = 3n`) bound the
    /// error against the polynomial at `t` itself. The basis at the exact
    /// `t` is non-negative and sums to 1, so the coefficients' own errors
    /// add their maximum, truncation into truncation and rounding into
    /// rounding.
    fn eval_fast(c: &[Self], t: &Self) -> Option<Self> {
        let n = c.len().checked_sub(1)?;
        let t_v = t.v;
        if t.err() != 0.0 || !(0.0..=1.0).contains(&t_v) {
            return None;
        }
        if n == 0 {
            return Some(c[0]);
        }
        let s = 1.0 - t_v;
        // `1 − s` is exact (Sterbenz) for `s ≥ ½`, and `s` is for `t ≥ ½`.
        let s_exact = 1.0 - s == t_v;
        let n32 = u32::try_from(n).ok()?;
        let m = n32.checked_mul(if s_exact { 2 } else { 3 })?;
        let mut v: SmallVec<[f64; 16]> = c.iter().map(|b| b.v).collect();
        let mut a: SmallVec<[f64; 16]> = c.iter().map(|b| b.v.abs()).collect();
        for k in 1..=n {
            for i in 0..=n - k {
                v[i] = s * v[i] + t_v * v[i + 1];
                a[i] = s * a[i] + t_v * a[i + 1];
            }
        }
        let (mut trunc, mut round) = (0.0, 0.0);
        for b in c {
            trunc = most(trunc, b.err_trunc);
            round = most(round, b.err_round);
        }
        Some(Bounded::new(
            v[0],
            trunc,
            add_up(round, gamma_bound(m, a[0], U)),
        ))
    }
}

impl Scalar for Bounded<Dd> {
    #[inline]
    fn int(k: u32, _: &Self) -> Self {
        Bounded::exact(Dd::from_f64(f64::from(k)))
    }

    fn int_u64(k: u64, _: &Self) -> Self {
        let (hi, lo) = split_u64(k);
        Bounded::exact(Dd::sum(hi, lo))
    }

    #[inline]
    fn add(&self, o: &Self) -> Self {
        *self + *o
    }

    #[inline]
    fn sub(&self, o: &Self) -> Self {
        *self - *o
    }

    #[inline]
    fn mul(&self, o: &Self) -> Self {
        *self * *o
    }

    #[inline]
    fn div(&self, o: &Self) -> Self {
        *self / *o
    }

    #[inline]
    fn neg(&self) -> Self {
        -*self
    }

    /// The sum halved part by part, then renormalised (exactly, by
    /// 2Sum): exact but below the normal range, where each part rounds by
    /// at most `η/2` (the `η` added).
    fn mid(&self, o: &Self) -> Self {
        let s = *self + *o;
        Bounded::new(
            Dd::sum(s.v.hi() * 0.5, s.v.lo() * 0.5),
            half_up(s.err_trunc),
            add_up(half_up(s.err_round), ETA),
        )
    }

    /// `hi + lo` bounded each way in two rounded steps, each pushed out.
    fn enclose(&self) -> [f64; 2] {
        if !self.is_certified() {
            return UNBOUNDED;
        }
        let (hi, lo, e) = (self.v.hi(), self.v.lo(), self.err());
        let r = [down(hi + down(lo - e)), up_signed(hi + up_signed(lo + e))];
        if r[0].is_nan() || r[1].is_nan() {
            UNBOUNDED
        } else {
            r
        }
    }

    #[inline]
    fn sign(&self) -> Option<Ordering> {
        Bounded::<Dd>::sign(self)
    }

    #[inline]
    fn from_f64(x: f64, _: &Self) -> Self {
        Bounded::exact(Dd::from_f64(x))
    }
}

impl<B: BigF> Scalar for Bounded<B> {
    #[inline]
    fn int(k: u32, like: &Self) -> Self {
        Bounded::exact(B::from_f64(f64::from(k), like.v.width()))
    }

    fn int_u64(k: u64, like: &Self) -> Self {
        let (hi, lo) = split_u64(k);
        Bounded::exact(B::from_dd(Dd::sum(hi, lo), like.v.width()))
    }

    #[inline]
    fn add(&self, o: &Self) -> Self {
        self + o
    }

    #[inline]
    fn sub(&self, o: &Self) -> Self {
        self - o
    }

    #[inline]
    fn mul(&self, o: &Self) -> Self {
        self * o
    }

    #[inline]
    fn div(&self, o: &Self) -> Self {
        self / o
    }

    #[inline]
    fn neg(&self) -> Self {
        -self
    }

    /// As the other levels': exactly `k` with no error, `−0` the integer
    /// 0 (the representation's equality tells the zeros apart).
    fn is_int(&self, k: u32) -> bool {
        self.err_trunc == 0.0
            && self.err_round == 0.0
            && if k == 0 {
                self.v.is_zero()
            } else {
                self.v == B::from_f64(f64::from(k), self.v.width())
            }
    }

    /// The sum times an exact ½ (exact itself unless it leaves the
    /// exponent range, then uncertified).
    fn mid(&self, o: &Self) -> Self {
        let s = self + o;
        let half = Bounded::exact(B::from_f64(0.5, s.v.width()));
        &s * &half
    }

    /// The value rounded to the nearest `f64`, `v_f`: the true value lies
    /// beyond the floats either side of `v_f`, so those, pushed out by
    /// the error, bound it. A value past the `f64` range is unbounded.
    fn enclose(&self) -> [f64; 2] {
        let f = self.v.to_f64();
        if !self.is_certified() || !f.is_finite() {
            return UNBOUNDED;
        }
        widen_by(down(f), up_signed(f), self.err())
    }

    #[inline]
    fn sign(&self) -> Option<Ordering> {
        Bounded::<B>::sign(self)
    }

    /// Exact: every width holds an `f64`.
    #[inline]
    fn from_f64(x: f64, like: &Self) -> Self {
        Bounded::exact(B::from_f64(x, like.v.width()))
    }
}

impl Scalar for Rational {
    #[inline]
    fn int(k: u32, _: &Self) -> Self {
        Rational::from(k)
    }

    #[inline]
    fn int_u64(k: u64, _: &Self) -> Self {
        Rational::from(k)
    }

    #[inline]
    fn add(&self, o: &Self) -> Self {
        self + o
    }

    #[inline]
    fn sub(&self, o: &Self) -> Self {
        self - o
    }

    #[inline]
    fn mul(&self, o: &Self) -> Self {
        self * o
    }

    #[inline]
    fn div(&self, o: &Self) -> Self {
        self / o
    }

    #[inline]
    fn neg(&self) -> Self {
        -self
    }

    fn mid(&self, o: &Self) -> Self {
        (self + o) >> 1u32
    }

    /// Rounded down and up into `f64` (past its range, `±∞` or the top
    /// float on the inner side).
    fn enclose(&self) -> [f64; 2] {
        [
            f64::rounding_from(self, Floor).0,
            f64::rounding_from(self, Ceiling).0,
        ]
    }

    fn sign(&self) -> Option<Ordering> {
        match self.partial_cmp(&0u32) {
            Some(Ordering::Equal) | None => None,
            o => o,
        }
    }

    /// Panics for a value that isn't finite.
    fn from_f64(x: f64, _: &Self) -> Self {
        Rational::try_from(x).expect("a finite f64")
    }
}
