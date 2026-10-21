//! Double-double, the second precision level: a value is the unevaluated
//! sum `hi + lo` of two `f64`s with `hi` the sum rounded to nearest, about
//! 106 bits of precision, and [`Bounded<Dd>`] its values with certified
//! error bounds. A [`Bounded<f64>`] moves up to it exactly
//! ([`Bounded::promote`]).
//!
//! **The algorithms and their bounds** are those of M. Joldes,
//! J.-M. Muller and V. Popescu, "Tight and rigorous error bounds for basic
//! building blocks of double-word arithmetic", ACM Transactions on
//! Mathematical Software 44(2), 2017, whose theorems J.-M. Muller and
//! L. Rideau checked in Coq ("Formalization of double-word arithmetic, and
//! comments on ...", ACM TOMS 48(1), 2022; the slips they found in some
//! proofs leave the bounds valid). With `u = 2⁻⁵³`, relative to the exact
//! result:
//!
//! | operation | algorithm | proven bound | constant here |
//! |---|---|---|---|
//! | `+`, `−` | AccurateDWPlusDW (Alg. 6) | `3u²/(1 − 4u)` < `3u² + 13u³` (Th. 3.1) | [`ADD_REL`] `= 4u²` |
//! | `×` | DWTimesDW1 (Alg. 10) | `7u²/(1 + u)²` (Th. 5.1) | [`MUL_REL`] `= 8u²` |
//! | `÷` | DWDivDW2 (Alg. 17, with DWTimesFP1, Alg. 7) | `15u² + 56u³` (Th. 7.1) | [`DIV_REL`] `= 16u²` |
//!
//! Each constant is the proven bound rounded up to the next multiple of
//! `u²`. The margin, over `u³`, covers taking the bound relative to the
//! computed `|hi|` rather than the exact result (`|hi + lo| ≤ (1 + u)·|hi|`,
//! and the exact result is within the bound of the computed one) and the
//! flushed low parts below. Each is a power of two, so `rel·|hi|` is exact
//! but for underflow, which [`up`]'s floor `η` covers.
//!
//! **No FMA.** Exact products `a·b = p + e` are Dekker's: a Veltkamp split
//! of each factor into 26-bit halves and the four partial products. Rust
//! never contracts `a·b + c` into a fused multiply-add, so the results are
//! the same bits with and without hardware FMA (the crate bans `mul_add`).
//!
//! **Exponent range.** The paper's bounds assume an unbounded exponent
//! range. Additions and subtractions are exact whenever their result is
//! subnormal, so on `f64` inputs IEEE addition gives the same result as
//! addition with an unbounded exponent range: the addition algorithm runs
//! exactly as the paper's model does, and its bound holds down to `η`,
//! with no absolute term. Nothing in it overflows while both operands'
//! `|hi|` is below `2¹⁰²⁰` ([`RANGE`]). From there it runs on the operands
//! scaled by `2⁻⁴`, each part within `η/2`, and its result is scaled back
//! exactly (or overflows, uncertified): within `rel·|hi| +`
//! [`SUM_SCALE_ABS`] `= 33η` of the exact sum, so sums are certified up to
//! overflow, as at the first level.
//!
//! Products and quotients round subnormal results by up to `η/2`, so they
//! run on scaled operands: each operand scaled by a power of two to `hi`
//! in `[1, 2)` (exactly), its `lo` set to zero when below `2⁻⁹⁰⁰` there
//! (a relative change below `2⁻⁸⁹⁹`, in the margin above). There every
//! partial product is exact, every rounded product and quotient is at
//! least `2⁻⁹⁵⁴` (every quantity is a multiple of `2⁻⁹⁵³`), so no
//! operation underflows or overflows, and the bound holds for the scaled
//! operands. Scaling the result back rounds to nearest once where it
//! falls below the normal range: the low part alone while the high part
//! stays normal, else the whole result to the nearest float (a subnormal
//! high part, no low part). So it is within `η/2` of the scaled-back
//! result, or overflows (`±∞`, uncertified), and a product or quotient is
//! within `rel·|hi| + η/2` of the exact one over the whole range, which
//! the floor `η` of [`up`] covers: the rounding term is `up(rel·|hi|)`,
//! as the first level's is `up(u·|v|)`.
//!
//! **Promotion never loses a certified sign near zero.** On promoted
//! operands (no low parts) a product is exact or, below the normal range,
//! the nearest float, the first level's bits; a quotient below the normal
//! range is the first level's own `fl(a/b)`. Their rounding terms are at
//! most the first level's and the other terms the same, and
//! [`Bounded::sign`] compares `|hi + lo|` with the error exactly, so a
//! sign the first level certifies within a few `η` of zero stays
//! certified.
//!
//! **Square root.** The paper has none: the root is one Newton step from
//! the `f64` root, `r = s + (x − s²)/(2s)`, bounded afterwards from its
//! certified residual `|x − r²|` by `|√x − r| = |x − r²| / (√x + r)`, at
//! most `|x − r²| / r` and `√|x − r²|`. From `hi ≥ 2¹⁰⁰⁰` it is the root
//! of `x·2⁻⁶⁴` scaled back by `2³²`, so the residual's sum stays below
//! [`RANGE`] and roots are certified over the whole range; below
//! `hi < 2⁻⁹⁰⁰` it is the root of `x·2²⁰⁰` scaled back by `2⁻¹⁰⁰`, so the
//! residual's absolute term (the floor `η`) doesn't swamp the root's own
//! rounding, and tiny roots stay as tight as at the first level.

use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Neg, Sub};

use super::bounded::{Bounded, ETA, Level, add_up, div_up, down, least, mul_up, up};
use super::expansion::Exp;

/// A double-double: the unevaluated sum `hi + lo`, `hi` the sum rounded to
/// nearest (so `|lo| ≤ ulp(hi)/2`). Built from `f64`s ([`Dd::from_f64`],
/// [`Dd::sum`]) or by [`Bounded<Dd>`]'s operations, which keep that form.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dd {
    hi: f64,
    lo: f64,
}

/// [`Bounded<Dd>`]'s relative rounding bound for `+` and `−`: `4u²`, above
/// AccurateDWPlusDW's proven `3u² + 13u³` (see the [module docs](self)).
pub const ADD_REL: f64 = pow2(-104);

/// [`Bounded<Dd>`]'s relative rounding bound for `×`: `8u²`, above
/// DWTimesDW1's proven `7u²`.
pub const MUL_REL: f64 = pow2(-103);

/// [`Bounded<Dd>`]'s relative rounding bound for `÷`: `16u²`, above
/// DWDivDW2's proven `15u² + 56u³`.
pub const DIV_REL: f64 = pow2(-102);

/// The magnitude below which both operands' `hi` must be for a sum to run
/// unscaled, `2¹⁰²⁰`: then nothing in the addition overflows.
pub const RANGE: f64 = pow2(1020);

/// [`Bounded<Dd>`]'s absolute rounding term for a sum run on operands
/// scaled by `2⁻⁴` (one at or past [`RANGE`]), `33η`: the four parts
/// scaled within `η/2` each, `16·2η` scaled back, and the relative bound's
/// share of it.
pub const SUM_SCALE_ABS: f64 = 33.0 * ETA;

/// Low parts below this, relative to `hi` in `[1, 2)`, are dropped before
/// a product or quotient, `2⁻⁹⁰⁰`.
const FLUSH: f64 = pow2(-900);

/// Veltkamp's splitting constant for `f64`, `2²⁷ + 1`.
const SPLITTER: f64 = 134_217_729.0;

/// `2ᵏ` for `k` in the normal range, `−1022..=1023`.
const fn pow2(k: i32) -> f64 {
    debug_assert!(-1022 <= k && k <= 1023);
    f64::from_bits(((1023 + k) as u64) << 52)
}

impl Dd {
    /// Zero.
    pub const ZERO: Dd = Dd { hi: 0.0, lo: 0.0 };

    /// A float, exactly.
    #[inline]
    pub const fn from_f64(x: f64) -> Dd {
        Dd { hi: x, lo: 0.0 }
    }

    /// The exact sum of two floats (unless it overflows).
    #[inline]
    pub fn sum(a: f64, b: f64) -> Dd {
        let (hi, lo) = two_sum(a, b);
        Dd { hi, lo }
    }

    /// The high part: the value rounded to the nearest `f64`.
    #[inline]
    pub fn hi(self) -> f64 {
        self.hi
    }

    /// The low part, `value − hi` exactly.
    #[inline]
    pub fn lo(self) -> f64 {
        self.lo
    }

    /// An upper bound of `|hi + lo|`, `+∞` unless finite: `|hi|` when `lo`
    /// is zero or of the other sign, else the float after `|hi|` (as
    /// `|lo| ≤ ulp(hi)/2`).
    #[inline]
    fn mag_up(self) -> f64 {
        if !self.is_finite() {
            f64::INFINITY
        } else if self.lo == 0.0 || (self.lo < 0.0) != (self.hi < 0.0) {
            self.hi.abs()
        } else {
            self.hi.abs().next_up()
        }
    }

    /// An upper bound of `|hi + lo|·e` for an error term `e ≥ 0`, through
    /// [`mag_up`](Self::mag_up); past the top float (`hi = ±MAX` with a
    /// low part of its sign, where that bound is `+∞`) through
    /// `|hi + lo| < 2¹⁰²⁴ = 2·2¹⁰²³`, so such a value stays certified.
    #[inline]
    fn mag_mul_up(self, e: f64) -> f64 {
        let m = self.mag_up();
        if m == f64::INFINITY && self.is_finite() {
            mul_up(2.0, mul_up(pow2(1023), e))
        } else {
            mul_up(m, e)
        }
    }

    /// A lower bound of `|hi + lo|` for a finite value: `|hi|` when `lo` is
    /// zero or of the same sign, else the float before `|hi|` (which is
    /// `|hi| − ulp(hi)/2` or less, also for a power of two).
    #[inline]
    fn mag_down(self) -> f64 {
        if self.lo == 0.0 || (self.lo < 0.0) == (self.hi < 0.0) {
            self.hi.abs()
        } else {
            self.hi.abs().next_down()
        }
    }
}

impl Level for Dd {
    #[inline]
    fn is_finite(&self) -> bool {
        self.hi.is_finite() && self.lo.is_finite()
    }
}

impl From<f64> for Dd {
    #[inline]
    fn from(x: f64) -> Dd {
        Dd::from_f64(x)
    }
}

impl Neg for Dd {
    type Output = Dd;

    /// Exact.
    #[inline]
    fn neg(self) -> Dd {
        Dd {
            hi: -self.hi,
            lo: -self.lo,
        }
    }
}

/// `s + t = a + b` exactly, `s = fl(a + b)` (2Sum; Møller, Knuth). Exact
/// unless something overflows.
#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let a1 = s - b;
    let b1 = s - a1;
    (s, (a - a1) + (b - b1))
}

/// `s + t = a + b` exactly, `s = fl(a + b)`, for `|a| ≥ |b|` or either
/// zero (Fast2Sum; Dekker).
#[inline]
fn fast_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    (s, b - (s - a))
}

/// Veltkamp's split of `a` into `hi + lo` exactly, each of at most 26
/// significant bits. For `|a|` well inside the range (here in `[½, 4)`).
#[inline]
fn split(a: f64) -> (f64, f64) {
    let c = SPLITTER * a;
    let hi = c - (c - a);
    (hi, a - hi)
}

/// `p + e = a·b` exactly, `p = fl(a·b)`, by Dekker's product, without FMA.
/// For factors in `[½, 4)`, where no partial product underflows.
#[inline]
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    let e = ((ah * bh - p) + ah * bl + al * bh) + al * bl;
    (p, e)
}

/// `⌊log₂|x|⌋` for a finite non-zero `x`, subnormals included.
fn exponent(x: f64) -> i32 {
    let bits = x.to_bits() & !(1 << 63);
    let field = (bits >> 52) as i32;
    if field == 0 {
        // Subnormal: the mantissa's leading one.
        -1011 - bits.leading_zeros() as i32
    } else {
        field - 1023
    }
}

/// `x·2ᵏ`: exact unless it overflows (`±∞`) or falls below the normal
/// range, where it is within `η` of `x·2ᵏ` (each step rounds by at most
/// `η/2`, and multiplies the error of the steps before by at most ½, or
/// by 1 when itself exact).
fn scale(mut x: f64, mut k: i32) -> f64 {
    while k > 1023 {
        x *= pow2(1023);
        k -= 1023;
    }
    while k < -1022 {
        x *= pow2(-1022);
        k += 1022;
    }
    x * pow2(k)
}

/// `x` scaled by `2⁻ᵉ` to `hi` in `[1, 2)`, and `e`; `lo` dropped when
/// below [`FLUSH`] after scaling. For a finite `x` with `hi ≠ 0`.
fn normalise(x: Dd) -> (Dd, i32) {
    let e = exponent(x.hi);
    // Exact: the result is normal (a downward scaling of `lo` that rounds
    // falls below `FLUSH` and is dropped).
    let hi = scale(x.hi, -e);
    let lo = scale(x.lo, -e);
    let lo = if lo.abs() < FLUSH { 0.0 } else { lo };
    (Dd { hi, lo }, e)
}

/// `x·2ᵏ` rounded to nearest once (for a finite `x`): every step but the
/// last is exact, so it is within `η/2` of `x·2ᵏ`, or overflows.
fn scale_rn(x: f64, k: i32) -> f64 {
    if k >= -1022 || x == 0.0 {
        // Steps up are exact; a single step down rounds once.
        return scale(x, k);
    }
    if exponent(x) + k < -1076 {
        // Below `2⁻¹⁰⁷⁶ < η/2`: the nearest float is zero.
        return x * 0.0;
    }
    // `x·2^(k + 1022)` is normal (exact), then one rounding step.
    x * pow2(k + 1022) * pow2(-1022)
}

/// `(h + l)·2ᵏ` as a double-double, for `h ≠ 0` and `|l| ≤ ulp(h)/2` (a
/// scaled product or quotient), within `η/2` of it or overflowing. While
/// `h·2ᵏ` is normal it scales exactly and only `l` rounds; below that the
/// result is the float nearest to `(h + l)·2ᵏ`, ties to even, with no
/// low part.
fn rescale(h: f64, l: f64, k: i32) -> Dd {
    let e = exponent(h);
    if e + k >= -1022 {
        // `|h| ≥ |l|` stays so, so Fast2Sum is exact and restores the form.
        let (hi, lo) = fast_two_sum(scale(h, k), scale_rn(l, k));
        return Dd { hi, lo };
    }
    if e + k < -1077 {
        // `|h + l| < 2^(e + 1)`: below `2⁻¹⁰⁷⁶ < η/2`.
        return Dd::from_f64(h * 0.0);
    }
    let hs = scale_rn(h, k);
    // The rest at `h`'s scale, exactly: `hs·2⁻ᵏ` is within `η/2·2⁻ᵏ` of
    // `h`, so within a factor 2 of it (Sterbenz) or zero; `two_sum` keeps
    // the sum with `l` exact. `|rest| < η·2⁻ᵏ`: at most one step of `η`.
    let r = h - scale(hs, -k);
    let (rh, rl) = two_sum(r, l);
    let half = pow2(-1075 - k);
    let past = match rh.abs().partial_cmp(&half) {
        Some(Ordering::Less) => Ordering::Less,
        Some(Ordering::Greater) => Ordering::Greater,
        _ if rl == 0.0 => Ordering::Equal,
        _ if (rl < 0.0) == (rh < 0.0) => Ordering::Greater,
        _ => Ordering::Less,
    };
    let step = if rh < 0.0 { -ETA } else { ETA };
    let hi = match past {
        Ordering::Less => hs,
        Ordering::Greater => hs + step,
        // A tie: the even one.
        Ordering::Equal if hs.to_bits() & 1 == 0 => hs,
        Ordering::Equal => hs + step,
    };
    Dd::from_f64(hi)
}

/// A double-double that isn't finite, for an operation with an operand
/// that isn't.
const NAN: Dd = Dd {
    hi: f64::NAN,
    lo: f64::NAN,
};

/// `x + y` by AccurateDWPlusDW (JMP 2017, Alg. 6): within `3u² + 13u³`
/// relative unless something overflows (operands below [`RANGE`]).
fn add(x: Dd, y: Dd) -> Dd {
    let (sh, sl) = two_sum(x.hi, y.hi);
    let (th, tl) = two_sum(x.lo, y.lo);
    let c = sl + th;
    let (vh, vl) = fast_two_sum(sh, c);
    let w = tl + vl;
    let (hi, lo) = fast_two_sum(vh, w);
    Dd { hi, lo }
}

/// `x·y` by DWTimesDW1 (JMP 2017, Alg. 10) on the scaled operands: within
/// `7u²` relative plus `η/2` (exact, or the nearest float, for operands
/// without low parts).
fn mul(x: Dd, y: Dd) -> Dd {
    if !(x.is_finite() && y.is_finite()) {
        return NAN;
    }
    if x.hi == 0.0 || y.hi == 0.0 {
        return Dd::from_f64(x.hi * y.hi);
    }
    let (x, ex) = normalise(x);
    let (y, ey) = normalise(y);
    let (ch, c1) = two_prod(x.hi, y.hi);
    let t1 = x.hi * y.lo;
    let t2 = x.lo * y.hi;
    let c2 = t1 + t2;
    let c3 = c1 + c2;
    let (zh, zl) = fast_two_sum(ch, c3);
    rescale(zh, zl, ex + ey)
}

/// `x·y` for a float `y` by DWTimesFP1 (JMP 2017, Alg. 7), as a pair.
fn mul_f64(x: Dd, y: f64) -> (f64, f64) {
    let (ch, cl1) = two_prod(x.hi, y);
    let cl2 = x.lo * y;
    let (th, tl1) = fast_two_sum(ch, cl2);
    let tl2 = tl1 + cl1;
    fast_two_sum(th, tl2)
}

/// `x / y` by DWDivDW2 (JMP 2017, Alg. 17) on the scaled operands: within
/// `15u² + 56u³` relative plus `η/2`. For `y ≠ 0`. Of two floats with a
/// quotient below the normal range, `fl(x/y)` itself (the first level's
/// bits).
fn div(x: Dd, y: Dd) -> Dd {
    if !(x.is_finite() && y.is_finite()) || y.hi == 0.0 {
        return NAN;
    }
    if x.hi == 0.0 {
        return Dd::from_f64(x.hi / y.hi);
    }
    if x.lo == 0.0 && y.lo == 0.0 {
        let q = x.hi / y.hi;
        if q.abs() < f64::MIN_POSITIVE {
            return Dd::from_f64(q);
        }
    }
    let (x, ex) = normalise(x);
    let (y, ey) = normalise(y);
    let th = x.hi / y.hi;
    let (rh, rl) = mul_f64(y, th);
    // Exact (Sterbenz).
    let pi = x.hi - rh;
    let dl = x.lo - rl;
    let d = pi + dl;
    let tl = d / y.hi;
    let (zh, zl) = fast_two_sum(th, tl);
    rescale(zh, zl, ex - ey)
}

/// A candidate square root of a finite `x ≥ 0`: one Newton step from the
/// `f64` root of `hi`.
fn sqrt_candidate(x: Dd) -> Dd {
    if x.hi == 0.0 {
        return x;
    }
    let s = x.hi.sqrt();
    let sq = mul(Dd::from_f64(s), Dd::from_f64(s));
    let t = add(x, -sq);
    let (hi, lo) = fast_two_sum(s, t.hi / (2.0 * s));
    Dd { hi, lo }
}

/// `hi` from which a square root is taken of the value scaled by `2⁻⁶⁴`,
/// `2¹⁰⁰⁰`: its residual stays below [`RANGE`].
const SQRT_SCALE_FROM: f64 = pow2(1000);

/// `hi` below which a square root is taken of the value scaled by `2²⁰⁰`,
/// `2⁻⁹⁰⁰`: the products' absolute rounding term (`η`, [`ETA`]) in its
/// residual would otherwise swamp the root's own rounding (`√η` would be
/// within about `5·√η` only).
const SQRT_SCALE_BELOW: f64 = pow2(-900);

/// The candidate root `r` of a finite `x ≥ 0` and an upper bound of
/// `|√x − r|`. Near either end of the range it is the root of `x·2²ᵏ`
/// scaled back by `2⁻ᵏ` (`k = −32` from [`SQRT_SCALE_FROM`], `k = 100`
/// below [`SQRT_SCALE_BELOW`]): `hi` scales exactly both ways, `lo` within
/// `η/2` (which moves the scaled root by far less than `η`) and the root's
/// `lo` within `η/2`, each covered by an `η` in the bound.
fn root(x: Dd) -> (Dd, f64) {
    let k = if x.hi >= SQRT_SCALE_FROM {
        -32
    } else if x.hi < SQRT_SCALE_BELOW && x.hi != 0.0 {
        100
    } else {
        return root_and_distance(x);
    };
    let xs = Dd {
        hi: x.hi * pow2(2 * k),
        lo: x.lo * pow2(2 * k),
    };
    let (r, dist) = root_and_distance(xs);
    let r = Dd {
        hi: r.hi * pow2(-k),
        lo: r.lo * pow2(-k),
    };
    (r, add_up(mul_up(add_up(dist, ETA), pow2(-k)), ETA))
}

/// The candidate root `r` of a finite `x ≥ 0` with `x.hi` zero or from
/// [`SQRT_SCALE_BELOW`] up to [`SQRT_SCALE_FROM`] (excluded), and an upper
/// bound of `|√x − r|`:
/// `|√x − r| = |x − r²| / (√x + r)`, at most `|x − r²| / r` and
/// `√|x − r²|`, from the certified residual; zero for the root of zero.
fn root_and_distance(x: Dd) -> (Dd, f64) {
    let r = sqrt_candidate(x);
    if x.hi == 0.0 {
        return (r, 0.0);
    }
    let r_lo = r.mag_down();
    let rr = Bounded::<Dd>::exact(r);
    let resid = (Bounded::<Dd>::exact(x) - rr * rr).abs_bound();
    let by_root = up(resid.sqrt());
    let dist = if r_lo > 0.0 {
        least(div_up(resid, r_lo), by_root)
    } else {
        by_root
    };
    (r, dist)
}

/// The rounding term of an operation: `rel·|hi| + abs`, rounded up; `+∞`
/// unless the result is finite. With `abs = 0` it also covers a further
/// `η/2` (the rounding of a result scaled back below the normal range):
/// `up(t) ≥ t + η` for a subnormal `t`, `t + ulp(t)` for a normal one,
/// and `rel·|hi|` loses at most `η/2` by underflow and nothing else.
#[inline]
fn rounding(z: Dd, rel: f64, abs: f64) -> f64 {
    if !z.is_finite() {
        f64::INFINITY
    } else if abs == 0.0 {
        up(rel * z.hi.abs())
    } else {
        add_up(mul_up(rel, z.hi.abs()), abs)
    }
}

impl Bounded<f64> {
    /// The same value and errors at the double-double level, exactly
    /// (promotion is exact).
    #[inline]
    pub fn promote(self) -> Bounded<Dd> {
        Bounded::new(Dd::from_f64(self.v), self.err_trunc, self.err_round)
    }
}

impl From<Bounded<f64>> for Bounded<Dd> {
    #[inline]
    fn from(b: Bounded<f64>) -> Self {
        b.promote()
    }
}

impl From<Dd> for Bounded<Dd> {
    #[inline]
    fn from(v: Dd) -> Self {
        Bounded::exact(v)
    }
}

impl Bounded<Dd> {
    /// The certified sign of the true value: `Some` when `|v| > err`,
    /// compared exactly (`|hi + lo|` against the error, by an exact sum
    /// where the low part decides), `None` when the error doesn't settle
    /// it (and for a value that isn't finite).
    pub fn sign(&self) -> Option<Ordering> {
        let e = self.err();
        let (hi, lo) = (self.v.hi, self.v.lo);
        let settled = self.v.is_finite()
            && if e == 0.0 {
                hi != 0.0
            } else if self.v.mag_down() > e {
                true
            } else if lo == 0.0 || self.v.mag_up() <= e {
                false
            } else {
                // `|hi| + lo·sign(hi) − e`, exactly.
                let lo = if hi < 0.0 { -lo } else { lo };
                let d = Exp::of_sum(hi.abs(), -e).sum(&Exp::from_f64(lo));
                d.sign() == Ordering::Greater
            };
        settled.then_some({
            if self.v.hi > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        })
    }

    /// An upper bound of the true value's magnitude, `|v| + err`: `|hi|`
    /// itself for an exact float (so the top float is its own bound),
    /// else rounded up. `+∞` past the top float (`hi = ±MAX` with a low
    /// part of its sign): no `f64` bounds that magnitude.
    #[inline]
    pub fn abs_bound(&self) -> f64 {
        let e = self.err();
        if e == 0.0 && self.v.lo == 0.0 && self.v.is_finite() {
            self.v.hi.abs()
        } else {
            add_up(self.v.mag_up(), e)
        }
    }

    /// The absolute value: exact, the errors unchanged; `+0` (both parts)
    /// for a zero of either sign, as at the first level.
    #[inline]
    pub fn abs(self) -> Self {
        if self.v.hi == 0.0 {
            Bounded {
                v: Dd::ZERO,
                ..self
            }
        } else if self.v.hi < 0.0 {
            -self
        } else {
            self
        }
    }

    /// The square root. Uncertified unless the true value is certainly
    /// `≥ 0` (`v ≥ err`, with `v` bounded below as in
    /// [`sign`](Self::sign)); for a value non-negative by construction see
    /// [`sqrt_nonneg`](Self::sqrt_nonneg). The rounding part adds the
    /// candidate root's certified distance from `√v`; each error part `e`
    /// takes `min(e / √v, √e)`, as at the first level.
    pub fn sqrt(self) -> Self {
        let x = self.v;
        let e = self.err();
        let non_negative = x.is_finite()
            && if e == 0.0 {
                x.hi >= 0.0
            } else {
                x.hi > 0.0 && x.mag_down() >= e
            };
        if !non_negative {
            return Bounded::new(NAN, f64::INFINITY, f64::INFINITY);
        }
        self.root()
    }

    /// The square root of a value whose true value is `≥ 0` by
    /// construction (a sum of squares), certified whatever its error, as
    /// at the first level ([`Bounded::<f64>::sqrt_nonneg`]): a value at or
    /// below zero is taken as `+0`. Uncertified only for a value or error
    /// that isn't finite.
    pub fn sqrt_nonneg(self) -> Self {
        if !self.v.is_finite() {
            return Bounded::new(NAN, f64::INFINITY, f64::INFINITY);
        }
        let v = if self.v.hi > 0.0 { self.v } else { Dd::ZERO };
        Bounded { v, ..self }.root()
    }

    /// The root of a finite `v ≥ 0` (see [`sqrt`](Self::sqrt)).
    fn root(self) -> Self {
        let (r, dist) = root(self.v);
        let r_lo = r.mag_down();
        let root_lo = down(r_lo - dist);
        let part = |e: f64| {
            let by_root = up(e.sqrt());
            if root_lo > 0.0 {
                least(div_up(e, root_lo), by_root)
            } else {
                by_root
            }
        };
        Bounded {
            v: r,
            err_trunc: part(self.err_trunc),
            err_round: add_up(part(self.err_round), dist),
        }
    }
}

impl Neg for Bounded<Dd> {
    type Output = Self;

    /// Exact: the errors unchanged.
    #[inline]
    fn neg(self) -> Self {
        Bounded { v: -self.v, ..self }
    }
}

impl Add for Bounded<Dd> {
    type Output = Self;

    /// From [`RANGE`] on the operands scaled by `2⁻⁴` (see the
    /// [module docs](self)); uncertified on overflow.
    fn add(self, o: Self) -> Self {
        let (x, y) = (self.v, o.v);
        // False for a NaN too.
        let in_range = x.hi.abs() < RANGE && y.hi.abs() < RANGE;
        let (v, own) = if in_range {
            let v = add(x, y);
            (v, rounding(v, ADD_REL, 0.0))
        } else if x.is_finite() && y.is_finite() {
            let down = |z: Dd| Dd {
                hi: z.hi * pow2(-4),
                lo: z.lo * pow2(-4),
            };
            let s = add(down(x), down(y));
            let v = Dd {
                hi: s.hi * pow2(4),
                lo: s.lo * pow2(4),
            };
            (v, rounding(v, ADD_REL, SUM_SCALE_ABS))
        } else {
            (add(x, y), f64::INFINITY)
        };
        Bounded {
            v,
            err_trunc: add_up(self.err_trunc, o.err_trunc),
            err_round: add_up(add_up(self.err_round, o.err_round), own),
        }
    }
}

impl Sub for Bounded<Dd> {
    type Output = Self;

    /// `self + (−o)`.
    #[inline]
    fn sub(self, o: Self) -> Self {
        self + -o
    }
}

impl Mul for Bounded<Dd> {
    type Output = Self;

    /// As at the first level, with `|va|`, `|vb|` bounded above:
    /// `|xa·xb − va·vb| ≤ |va|·eb + |vb|·ea + ea·eb`, the terms of
    /// truncation alone truncation, the rest rounding. Commutative, bit for
    /// bit.
    fn mul(self, o: Self) -> Self {
        let v = mul(self.v, o.v);
        let (a, b) = (self, o);
        let err_trunc = add_up(
            add_up(a.v.mag_mul_up(b.err_trunc), b.v.mag_mul_up(a.err_trunc)),
            mul_up(a.err_trunc, b.err_trunc),
        );
        let linear = add_up(a.v.mag_mul_up(b.err_round), b.v.mag_mul_up(a.err_round));
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
            err_round: add_up(add_up(linear, cross), rounding(v, MUL_REL, 0.0)),
        }
    }
}

impl Div for Bounded<Dd> {
    type Output = Self;

    /// As at the first level: `|xa/xb − va/vb| ≤ (ea + |va/vb|·eb) / |xb|`
    /// with `|xb| ≥ |vb| − eb`, each part over that lower bound.
    /// Uncertified unless the divisor is finite and certainly non-zero.
    fn div(self, o: Self) -> Self {
        let v = div(self.v, o.v);
        let (a, b) = (self, o);
        let den = down(b.v.mag_down() - b.err());
        // False for a NaN too.
        let non_zero = b.v.is_finite() && den > 0.0;
        if !non_zero {
            return Bounded::new(v, f64::INFINITY, f64::INFINITY);
        }
        let own = rounding(v, DIV_REL, 0.0);
        // `|va/vb|·eb`, `|va/vb|` the exact quotient of the values, at most
        // `up(|hi|)` as at the first level: past `|hi + lo|` by at least
        // `ulp(hi)/2`, which holds `rel·|hi| + η/2` (there is a low part
        // only where `ulp(hi) ≥ 2η`); past the top float `|v| + own`, term
        // by term.
        let q = up(v.hi.abs());
        let q_times = |eb: f64| {
            if q < f64::INFINITY {
                mul_up(q, eb)
            } else {
                add_up(v.mag_mul_up(eb), mul_up(own, eb))
            }
        };
        let part = |ea: f64, eb: f64| div_up(add_up(ea, q_times(eb)), den);
        Bounded {
            v,
            err_trunc: part(a.err_trunc, b.err_trunc),
            err_round: add_up(part(a.err_round, b.err_round), own),
        }
    }
}

#[cfg(test)]
mod tests;
