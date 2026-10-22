//! A polynomial in Bernstein form on `[0, 1]`: evaluation by de
//! Casteljau, reversal, degree elevation, the derivative and the
//! coefficient hull.

use smallvec::SmallVec;

use super::scalar::Scalar;
use crate::num::bounded::Bounded;

/// The largest degree a [`Poly`] takes, 64: far above any certificate's
/// (a seam numerator of degree 10, an implicit composed with a span of
/// degree 16), small enough that every elevation weight is a small exact
/// integer and every binomial a `u64` (a product's weights pass `2⁵³`
/// above degree 56, where the first level rounds and certifies them,
/// [`Scalar::int_u64`]).
pub const MAX_DEGREE: usize = 64;

/// `C(n, k)` for `k ≤ n ≤ MAX_DEGREE`, exactly: `C(64, 32) < 2⁶¹`.
pub(crate) fn binom(n: usize, k: usize) -> u64 {
    debug_assert!(k <= n && n <= MAX_DEGREE);
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k {
        // Exact at every step: `r·(n − i)` is `(i + 1)·C(n, i + 1)`, below
        // `2⁶⁷`.
        r = r * (n - i) as u128 / (i + 1) as u128;
    }
    u64::try_from(r).expect("C(n, k) < 2⁶¹ for n ≤ 64")
}

/// `w·x`, with no operation for `w = 1`.
pub(super) fn weigh<S: Scalar>(w: u64, x: S) -> S {
    if w == 1 { x } else { S::int_u64(w, &x).mul(&x) }
}

/// `x / d`, `d` at `x`'s level (and width), with no operation for
/// `d = 1`.
pub(super) fn divide<S: Scalar>(x: S, d: u64) -> S {
    if d == 1 { x } else { x.div(&S::int_u64(d, &x)) }
}

/// Coefficients stored inline up to this many (degree 10, a seam
/// numerator's).
pub(super) const INLINE: usize = 11;

/// The coefficient storage.
pub(super) type Coefs<S> = SmallVec<[S; INLINE]>;

/// `p(t) = Σ bᵢ·C(n, i)·(1 − t)ⁿ⁻ⁱ·tⁱ`, its degree `n` the number of
/// coefficients less one, at most [`MAX_DEGREE`].
///
/// At a [`Bounded`] level every result carries a certified bound on its
/// distance from the same operation on the true coefficients (anywhere
/// within their own bounds); with [`malachite::Rational`] coefficients
/// every operation is exact.
#[derive(Clone, Debug, PartialEq)]
pub struct Poly<S> {
    c: Coefs<S>,
}

/// A certified enclosure of a polynomial's range on `[0, 1]`: the least
/// and greatest coefficient (the curve lies in the convex hull of its
/// control values), each bounded outwards, so `lo ≤ p(t) ≤ hi` for every
/// `t` in `[0, 1]`. `±∞` where a coefficient isn't certified.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hull {
    pub lo: f64,
    pub hi: f64,
}

impl Hull {
    /// An upper bound of `|p(t)|` on `[0, 1]`, `max(−lo, hi)`.
    pub fn abs_max(&self) -> f64 {
        let (a, b) = (-self.lo, self.hi);
        if b > a { b } else { a }
    }
}

impl<S: Scalar> Poly<S> {
    /// The polynomial with these Bernstein coefficients, `None` unless
    /// there are `1..=MAX_DEGREE + 1` of them.
    pub fn new(c: impl IntoIterator<Item = S>) -> Option<Self> {
        let c: Coefs<S> = c.into_iter().take(MAX_DEGREE + 2).collect();
        (!c.is_empty() && c.len() <= MAX_DEGREE + 1).then_some(Poly { c })
    }

    /// From coefficients the caller has checked: `1..=MAX_DEGREE + 1`.
    pub(super) fn from_coefs(c: Coefs<S>) -> Self {
        debug_assert!(!c.is_empty() && c.len() <= MAX_DEGREE + 1);
        Poly { c }
    }

    /// The Bernstein coefficients, `b₀` first.
    #[inline]
    pub fn coefs(&self) -> &[S] {
        &self.c
    }

    /// The degree `n`.
    #[inline]
    pub fn degree(&self) -> usize {
        self.c.len() - 1
    }

    /// `p(t)`: at an exact end, `t` exactly 0 or 1, the end coefficient
    /// itself (`p(0) = b₀`, `p(1) = bₙ`, error parts and all); else the
    /// level's own method where it has one ([`Scalar::eval_fast`]: at the
    /// first level, for an exact `t` in `[0, 1]`), else de Casteljau in
    /// the scalar's operations, `bᵢ ← (1 − t)·bᵢ + t·bᵢ₊₁`, which holds
    /// for any `t`.
    pub fn eval(&self, t: &S) -> S {
        if let Some(b) = self.end(t) {
            return b.clone();
        }
        if let Some(v) = S::eval_fast(&self.c, t) {
            return v;
        }
        let s = S::int(1, t).sub(t);
        let mut w = self.c.clone();
        let n = self.degree();
        for k in 1..=n {
            for i in 0..=n - k {
                w[i] = s.mul(&w[i]).add(&t.mul(&w[i + 1]));
            }
        }
        w.swap_remove(0)
    }

    /// The end coefficient at `t` when `t` is exactly 0 or 1.
    fn end(&self, t: &S) -> Option<&S> {
        if t.is_int(0) {
            self.c.first()
        } else if t.is_int(1) {
            self.c.last()
        } else {
            None
        }
    }

    /// `p(1 − t)`: the coefficients in reverse order, exactly.
    pub fn reversed(&self) -> Self {
        Poly {
            c: self.c.iter().rev().cloned().collect(),
        }
    }

    /// The same polynomial at degree `n + by`, `None` past
    /// [`MAX_DEGREE`]. One degree at a time,
    /// `b'ᵢ = (i·bᵢ₋₁ + (n + 1 − i)·bᵢ) / (n + 1)`, the end coefficients
    /// kept to the bit; the elevation of the reversed polynomial is this
    /// one's reversed, to the bit.
    pub fn elevate(&self, by: usize) -> Option<Self> {
        let target = self.degree().checked_add(by)?;
        if target > MAX_DEGREE {
            return None;
        }
        let mut c = self.c.clone();
        while c.len() <= target {
            c = elevate_once(&c);
        }
        Some(Poly { c })
    }

    /// The derivative `p'`, of degree `n − 1`: `n·(bᵢ₊₁ − bᵢ)` (the
    /// hodograph). A constant's is the zero polynomial of degree 0.
    pub fn derivative(&self) -> Self {
        Poly { c: derive(&self.c) }
    }

    /// The coefficient hull: a certified enclosure of `p` on `[0, 1]`.
    pub fn hull(&self) -> Hull {
        hull_of(&self.c)
    }
}

/// The coefficients of the derivative of the polynomial with Bernstein
/// coefficients `c` (non-empty, at most [`MAX_DEGREE`] + 1 of them).
pub(super) fn derive<S: Scalar>(c: &[S]) -> Coefs<S> {
    let n = u32::try_from(c.len() - 1).expect("degree at most MAX_DEGREE");
    if n == 0 {
        return smallvec::smallvec![S::int(0, &c[0])];
    }
    c.windows(2)
        .map(|w| S::int(n, &w[1]).mul(&w[1].sub(&w[0])))
        .collect()
}

/// The least and greatest of the coefficients' enclosures.
pub(super) fn hull_of<S: Scalar>(c: &[S]) -> Hull {
    let mut h = Hull {
        lo: f64::INFINITY,
        hi: f64::NEG_INFINITY,
    };
    for b in c {
        let [lo, hi] = b.enclose();
        if lo < h.lo {
            h.lo = lo;
        }
        if hi > h.hi {
            h.hi = hi;
        }
    }
    h
}

/// One degree up (degree `n` from `c.len() = n + 1`, at most
/// [`MAX_DEGREE`] − 1, so every weight is a small integer). Each quotient
/// takes its divisor at its numerator's level, so a coefficient depends
/// on its two neighbours alone (symmetrically): the elevation of the
/// reversed polynomial is the reversed elevation, to the bit, also with
/// big-float coefficients of mixed widths.
///
/// Where the numerator `i·bᵢ₋₁ + (n + 1 − i)·bᵢ` isn't bounded in `f64`
/// (it overflows for coefficients near `MAX/i`), it is taken of the
/// coefficients scaled by `2⁻⁷` (below `1/(n + 1)`) and the quotient
/// scaled back, exactly but for subnormal coefficients (whose rounding
/// the level carries): the result, a convex combination, is in range.
pub(super) fn elevate_once<S: Scalar>(c: &[S]) -> Coefs<S> {
    let n1 = u32::try_from(c.len()).expect("degree at most MAX_DEGREE");
    let unbounded = |x: &S| x.enclose().iter().any(|e| !e.is_finite());
    let mut out = Coefs::with_capacity(c.len() + 1);
    out.push(c[0].clone());
    for i in 1..n1 {
        let (a, b) = (&c[i as usize - 1], &c[i as usize]);
        let num = S::int(i, a).mul(a).add(&S::int(n1 - i, b).mul(b));
        let q = if unbounded(&num) {
            let down = |x: &S| x.mul(&S::from_f64(ELEVATE_DOWN, x));
            let (a, b) = (down(a), down(b));
            let num = S::int(i, &a).mul(&a).add(&S::int(n1 - i, &b).mul(&b));
            let q = num.div(&S::int(n1, &num));
            q.mul(&S::from_f64(ELEVATE_UP, &q))
        } else {
            num.div(&S::int(n1, &num))
        };
        out.push(q);
    }
    out.push(c[c.len() - 1].clone());
    out
}

/// `2⁻⁷`, below `1/(MAX_DEGREE + 1)`: an elevation's numerator of
/// coefficients scaled by it stays below the top of the range.
pub(super) const ELEVATE_DOWN: f64 = 1.0 / 128.0;

/// `2⁷`, scaling an elevated coefficient back.
pub(super) const ELEVATE_UP: f64 = 128.0;

impl Poly<Bounded<f64>> {
    /// From exact `f64` coefficients ([`Poly::new`]'s limits).
    pub fn from_f64(c: &[f64]) -> Option<Self> {
        Poly::new(c.iter().map(|&x| Bounded::exact(x)))
    }
}
