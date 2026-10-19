//! Bounded evaluation: a value with a certified bound on its error.
//!
//! A [`Bounded<T>`] carries a value `v` in a precision level's format
//! (`f64` at the first level; double-double and big-float above it) and
//! two error parts, both `f64` upper bounds whatever the level (a bound
//! needs no more precision than that):
//!
//! - `err_trunc`, the truncation error: what the method itself leaves out
//!   (a box not yet small enough, a series cut short, a Newton step not
//!   taken). Subdividing reduces it; more precision doesn't.
//! - `err_round`, the rounding error of the arithmetic, which more
//!   precision reduces.
//!
//! The true value lies within `err = err_trunc + err_round` of `v`. An item
//! escalates in precision when its rounding error dominates
//! ([`Bounded::rounding_dominates`]), else in space; the comparison is of
//! certified values, so the choice is deterministic.
//!
//! **Upward rounding without directed rounding.** The error terms are
//! computed in round-to-nearest like everything else, then rounded up by
//! [`up`]: `up(t) = fl(fl(t·(1 + 2u)) + η)` with `u = 2⁻⁵³` ([`U`]) and
//! `η = 2⁻¹⁰⁷⁴` ([`ETA`]), the least subnormal. For a normal `t` the
//! inflation alone reaches at least the next float above `t`; for a
//! subnormal `t` the floor `η` adds one step; an underflowed `t = 0`
//! (whose true value is at most `η/2`) becomes `η`. So the bounds stay
//! sound with subnormals on, and every computed error term is at least
//! `η`, far below any target. An overflow gives `+∞`: the item is
//! uncertified (and escalates), never wrong. So does anything that would
//! be a NaN in an error term (`∞ · 0`).
//!
//! Certified ([`Bounded::is_certified`]) means a finite value within a
//! finite error of the true one: a value that isn't finite counts as
//! uncertified whatever its error parts say ([`Level::is_finite`]), and
//! every operation's result is uncertified when an operand is.
//!
//! The operations of the first level are `+ − × ÷`, `√` (also of a value
//! non-negative by construction, [`Bounded::sqrt_nonneg`]), negation and
//! `abs` on `Bounded<f64>`: each rounds its value once, adds its own
//! rounding (at most `u·|v|`, or `η/2` below the normal range) to
//! `err_round` and carries the operands' errors through, truncation into
//! truncation and rounding into rounding (a product's cross terms with any
//! rounding error into rounding).
//!
//! For algorithms that evaluate in plain floats and bound the result
//! afterwards (de Casteljau, Bernstein evaluation), [`gamma`] and
//! [`gamma_bound`] give the classical `γₘ = m·u / (1 − m·u)` bounds for a
//! level's unit roundoff `u`, rounded up.
//!
//! The second level, double-double, and the promotion of a value to it are
//! in [`super::dd`].

use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// The unit roundoff of `f64`, `2⁻⁵³`: round-to-nearest is within `u·|x|`
/// of `x` in the normal range.
pub const U: f64 = f64::EPSILON / 2.0;

/// The least positive subnormal `f64`, `2⁻¹⁰⁷⁴`: the floor of every
/// computed error term.
pub const ETA: f64 = f64::from_bits(1);

/// `1 + 2u`, exactly.
const INFLATE: f64 = 1.0 + 2.0 * U;

/// `1 − 2u`, exactly.
const DEFLATE: f64 = 1.0 - 2.0 * U;

/// An upper bound of an error term `t ≥ 0` computed in round-to-nearest:
/// `fl(fl(t·(1 + 2u)) + η)`, at least the float after `t` (so above the
/// exact result `t` was rounded from), and at least [`ETA`]. `+∞` on
/// overflow, and for a NaN.
#[inline]
pub fn up(t: f64) -> f64 {
    let r = t * INFLATE + ETA;
    if r.is_nan() { f64::INFINITY } else { r }
}

/// A lower bound of a quantity `t` computed in round-to-nearest, the
/// counterpart of [`up`]: at most the float before `t`, so below the
/// exact result `t` was rounded from. For a positive `t`
/// `fl(fl(t·(1 − 2u)) − η)`, the top float for `+∞` (an overflow);
/// otherwise `−up(−t)`. `−∞` for a NaN.
#[inline]
pub fn down(t: f64) -> f64 {
    if t > 0.0 {
        least(t * DEFLATE - ETA, f64::MAX)
    } else {
        -up(-t)
    }
}

/// `up(a + b)`: an upper bound of the sum of two error terms.
#[inline]
pub fn add_up(a: f64, b: f64) -> f64 {
    up(a + b)
}

/// `up(a · b)`: an upper bound of the product of two error terms.
#[inline]
pub fn mul_up(a: f64, b: f64) -> f64 {
    up(a * b)
}

/// `up(a / b)`: an upper bound of an error term `a` over a positive lower
/// bound `b`; `+∞` unless `b > 0`.
#[inline]
pub fn div_up(a: f64, b: f64) -> f64 {
    if b > 0.0 { up(a / b) } else { f64::INFINITY }
}

/// The smaller of two error terms (neither a NaN), by compare and select.
#[inline]
pub(crate) fn least(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// An upper bound of `γₙ = n·u / (1 − n·u)` for the unit roundoff `u`
/// (`U` here, a higher level's own above it): the relative error of `n`
/// roundings in sequence, `|Π(1 + δᵢ) − 1| ≤ γₙ` with `|δᵢ| ≤ u`; `u`
/// given as an `f64` at least the true unit roundoff. `+∞` unless
/// `n·u < 1`.
pub fn gamma(n: u32, u: f64) -> f64 {
    let nu = mul_up(f64::from(n), u);
    div_up(nu, down(1.0 - nu))
}

/// An upper bound of the rounding error of a sum of products evaluated in
/// floats, from the same evaluation with every input replaced by its
/// absolute value, `a_hat` (computed alongside, or a bound of it): if
/// every term passes through at most `m` roundings of unit roundoff `u`,
/// the error is at most `γₘ·A`, `A` the exact absolute evaluation, and
/// `a_hat ≥ (1 − γₘ)·A`, so the result is `γₘ·a_hat / (1 − γₘ)`, rounded
/// up. Its absolute part `m·η` covers underflow: products that fall below
/// the normal range lose up to `η/2` each, and a convex-weight scheme
/// (every step's weights summing to at most `1 + u`) never amplifies them.
///
/// De Casteljau at a parameter `t` with weights `s` and `t` (both floats
/// `≥ 0`, `s + t ≤ 1 + u`, such as `s = fl(1 − t)`) does two roundings per
/// level, so a degree-`n` evaluation takes `m = 2n`; the bound is against
/// the polynomial at the floats `(s, t)` (the homogeneous form
/// `Σ bᵢ·C(n, i)·sⁿ⁻ⁱ·tⁱ`), and `a_hat` is de Casteljau of the `|bᵢ|`.
/// `+∞` unless `γₘ < 1`.
pub fn gamma_bound(m: u32, a_hat: f64, u: f64) -> f64 {
    let g = gamma(m, u);
    let rel = div_up(mul_up(g, a_hat), down(1.0 - g));
    add_up(rel, mul_up(f64::from(m), ETA))
}

/// A precision level's value format, as far as certification needs it.
/// Not `Copy`: a big-float level's values own their digits.
pub trait Level {
    /// Whether the value is finite. A value that isn't is never
    /// certified, whatever its error parts say.
    fn is_finite(&self) -> bool;
}

impl Level for f64 {
    #[inline]
    fn is_finite(&self) -> bool {
        f64::is_finite(*self)
    }
}

/// A value at a precision level with certified bounds on its truncation and
/// rounding errors (see the [module docs](self)). `T` is the level's
/// format: `f64` at the first level, whose operations are here.
///
/// Every operation's error terms are at least [`ETA`]; [`Bounded::exact`]
/// alone has zero errors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounded<T = f64> {
    /// The value, in the level's format.
    pub v: T,
    /// An upper bound of the truncation error.
    pub err_trunc: f64,
    /// An upper bound of the rounding error.
    pub err_round: f64,
}

impl<T> Bounded<T> {
    /// A value with the given error bounds (both `≥ 0`, possibly `+∞`; a
    /// NaN part leaves the value uncertified). A negative part would
    /// certify a value no bound covers, so debug builds reject it.
    #[inline]
    pub const fn new(v: T, err_trunc: f64, err_round: f64) -> Self {
        debug_assert!(
            (err_trunc >= 0.0 || err_trunc.is_nan()) && (err_round >= 0.0 || err_round.is_nan()),
            "Bounded error parts must be non-negative"
        );
        Bounded {
            v,
            err_trunc,
            err_round,
        }
    }

    /// An upper bound of the whole error, `err_trunc + err_round`: the
    /// other part itself, exactly, when one part is zero, so an exact value
    /// has `err() == 0`.
    #[inline]
    pub fn err(&self) -> f64 {
        if self.err_trunc == 0.0 {
            self.err_round
        } else if self.err_round == 0.0 {
            self.err_trunc
        } else {
            add_up(self.err_trunc, self.err_round)
        }
    }

    /// Whether the rounding error dominates, `err_round > err_trunc`: the
    /// item escalates in precision, else in space.
    #[inline]
    pub fn rounding_dominates(&self) -> bool {
        self.err_round > self.err_trunc
    }

    /// The same value with a further truncation error `t ≥ 0` (a remainder
    /// the method leaves out; a NaN leaves the value uncertified). A
    /// negative `t` would shrink the bound below the error, so debug
    /// builds reject it, as [`new`](Self::new) does.
    #[inline]
    pub fn with_trunc(self, t: f64) -> Self {
        debug_assert!(
            t >= 0.0 || t.is_nan(),
            "Bounded error parts must be non-negative"
        );
        Bounded {
            err_trunc: add_up(self.err_trunc, t),
            ..self
        }
    }
}

impl<T: Level> Bounded<T> {
    /// An exact value: zero errors, unless `v` isn't finite (then the
    /// rounding error is `+∞`).
    #[inline]
    pub fn exact(v: T) -> Self {
        let err_round = if v.is_finite() { 0.0 } else { f64::INFINITY };
        Bounded {
            v,
            err_trunc: 0.0,
            err_round,
        }
    }

    /// Whether the value is finite and the error too: then the true value
    /// is within [`err`](Self::err) of the value. Each level's operations
    /// also make the error infinite when the value isn't finite, but a
    /// value built by hand may not.
    #[inline]
    pub fn is_certified(&self) -> bool {
        self.v.is_finite() && self.err() < f64::INFINITY
    }
}

impl Bounded<f64> {
    /// The certified sign of the true value: `Some` when `|v| > err`, which
    /// also leaves it non-zero, `None` when the error doesn't settle it
    /// (and for a value that isn't finite).
    pub fn sign(&self) -> Option<Ordering> {
        if self.v.is_finite() && self.v.abs() > self.err() {
            Some(if self.v > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Less
            })
        } else {
            None
        }
    }

    /// An upper bound of the true value's magnitude, `|v| + err`: `|v|`
    /// itself for an exact value (so the top float is its own bound),
    /// else rounded up (`+∞` past the top float).
    #[inline]
    pub fn abs_bound(&self) -> f64 {
        let e = self.err();
        if e == 0.0 {
            self.v.abs()
        } else {
            add_up(self.v.abs(), e)
        }
    }

    /// The absolute value: exact, the errors unchanged (`|·|` moves no
    /// value further than its argument moves).
    #[inline]
    pub fn abs(self) -> Self {
        Bounded {
            v: self.v.abs(),
            ..self
        }
    }

    /// The square root. Uncertified unless the true value is certainly
    /// `≥ 0` (`v ≥ err`, compared exactly); for a value non-negative by
    /// construction see [`sqrt_nonneg`](Self::sqrt_nonneg).
    pub fn sqrt(self) -> Self {
        // False for a NaN too.
        let non_negative = self.v >= self.err();
        if !non_negative {
            return Bounded::new(self.v.sqrt(), f64::INFINITY, f64::INFINITY);
        }
        self.root()
    }

    /// The square root of a value whose true value is `≥ 0` by
    /// construction (a sum of squares, a squared length), certified
    /// whatever its error: a value below zero is taken as `+0`, which is
    /// no further from the true value than the value itself, so the error
    /// parts carry over (and `√` of `0 ± e` is `0 ± √e`). Uncertified only
    /// for a value or error that isn't finite. The caller vouches for the
    /// sign: of a value that may truly be negative the result is wrong.
    pub fn sqrt_nonneg(self) -> Self {
        // A value that isn't finite stays (uncertified); `−0` becomes `+0`.
        let v = if self.v.is_finite() && self.v <= 0.0 {
            0.0
        } else {
            self.v
        };
        Bounded { v, ..self }.root()
    }

    /// The root of `v ≥ 0` (or a NaN): the true root is within
    /// `e / (√x + √v) ≤ min(e / √v, √e)` of `√v` for an error `e` and a
    /// true value `x ≥ 0`; each part takes that bound of its own error
    /// (the bound is subadditive in `e`, so the parts still sum to a
    /// bound).
    fn root(self) -> Self {
        let v = self.v.sqrt();
        let root_lo = down(v);
        let part = |e: f64| {
            let by_root = up(e.sqrt());
            if root_lo > 0.0 {
                least(div_up(e, root_lo), by_root)
            } else {
                by_root
            }
        };
        Bounded {
            v,
            err_trunc: part(self.err_trunc),
            err_round: add_up(part(self.err_round), rounding(v)),
        }
    }
}

/// An upper bound of the rounding error of a result `v` rounded to nearest:
/// `u·|v|` in the normal range, `η/2` below it (where `up`'s floor covers
/// it), `+∞` if `v` isn't finite.
#[inline]
fn rounding(v: f64) -> f64 {
    up(U * v.abs())
}

impl From<f64> for Bounded<f64> {
    #[inline]
    fn from(v: f64) -> Self {
        Bounded::exact(v)
    }
}

impl Neg for Bounded<f64> {
    type Output = Self;

    /// Exact: the errors unchanged.
    #[inline]
    fn neg(self) -> Self {
        Bounded { v: -self.v, ..self }
    }
}

impl Add for Bounded<f64> {
    type Output = Self;

    #[inline]
    fn add(self, o: Self) -> Self {
        let v = self.v + o.v;
        Bounded {
            v,
            err_trunc: add_up(self.err_trunc, o.err_trunc),
            err_round: add_up(add_up(self.err_round, o.err_round), rounding(v)),
        }
    }
}

impl Sub for Bounded<f64> {
    type Output = Self;

    /// `self + (−o)`, the same bits as `self.v − o.v`.
    #[inline]
    fn sub(self, o: Self) -> Self {
        self + -o
    }
}

impl Mul for Bounded<f64> {
    type Output = Self;

    /// `|xa·xb − va·vb| ≤ |va|·eb + |vb|·ea + ea·eb` with `e = t + r`: the
    /// terms of truncation alone (`|va|·tb + |vb|·ta + ta·tb`) are
    /// truncation, the rest rounding. Commutative, bit for bit.
    fn mul(self, o: Self) -> Self {
        let v = self.v * o.v;
        let (a, b) = (self, o);
        let (va, vb) = (a.v.abs(), b.v.abs());
        let err_trunc = add_up(
            add_up(mul_up(va, b.err_trunc), mul_up(vb, a.err_trunc)),
            mul_up(a.err_trunc, b.err_trunc),
        );
        let linear = add_up(mul_up(va, b.err_round), mul_up(vb, a.err_round));
        let cross = add_up(
            mul_up(a.err_round, b.err_round),
            add_up(
                mul_up(a.err_round, b.err_trunc),
                mul_up(a.err_trunc, b.err_round),
            ),
        );
        Bounded {
            v,
            err_trunc,
            err_round: add_up(add_up(linear, cross), rounding(v)),
        }
    }
}

impl Div for Bounded<f64> {
    type Output = Self;

    /// `|xa/xb − va/vb| ≤ (ea + |va/vb|·eb) / |xb|` and
    /// `|xb| ≥ |vb| − eb`; each part takes its own errors over that lower
    /// bound. Uncertified unless the divisor is finite and certainly
    /// non-zero (`|vb| > eb`).
    fn div(self, o: Self) -> Self {
        let v = self.v / o.v;
        let (a, b) = (self, o);
        let den = down(b.v.abs() - b.err());
        // False for a NaN too.
        let non_zero = b.v.is_finite() && den > 0.0;
        if !non_zero {
            return Bounded::new(v, f64::INFINITY, f64::INFINITY);
        }
        let q = up(v.abs());
        let part = |ea: f64, eb: f64| div_up(add_up(ea, mul_up(q, eb)), den);
        Bounded {
            v,
            err_trunc: part(a.err_trunc, b.err_trunc),
            err_round: add_up(part(a.err_round, b.err_round), rounding(v)),
        }
    }
}

#[cfg(test)]
mod tests;
