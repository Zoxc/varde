// SPDX-License-Identifier: AGPL-3.0-or-later AND MIT
// SPDX-FileCopyrightText: 2017 The Spade Developers
// SPDX-FileCopyrightText: 2020 The GeoRust Project Developers
//
// Derived from robust (https://github.com/georust/robust), crates.io release 1.2.0
//   (robust-1.2.0.crate, sha256 4e27ee8bb91ca0adcf0ecb116293afa12d393f9c2b9b9cd54d33e8078fe19839),
//   file src/lib.rs, lines 53, 1966-2030, 2061-2074 and 2176-2186 (`SPLITTER`,
//   `scale_expansion_zeroelim`, `two_product`, `two_product_tail`, `split`,
//   `two_product_presplit`, `two_sum`, `two_sum_tail`, `fast_two_sum_tail`,
//   `fast_two_sum`), itself a transcript of J. R. Shewchuk's public-domain predicates.c,
//   Copyright (c) 2017 The Spade Developers, Copyright (c) 2020 The GeoRust Project
//   Developers, licensed under the MIT license (LICENSES/MIT.txt; the crate is
//   MIT OR Apache-2.0, used here under MIT).
// Translated to Rust and modified: an `Exp` value over a smallvec instead of caller
//   slices and lengths, zero as the empty expansion, empty operands handled, the
//   two-sum tails inlined; the floating-point operations and their order unchanged.
// See THIRD_PARTY.md, row L4.
//
// Derived from Shewchuk's Robust Predicates (https://www.cs.cmu.edu/~quake/robust.html),
//   predicates.c of May 18, 1996, retrieved 2026-10-13, sha256
//   f8662c3f407d1c1c5dcd4dd49ea8b8ddd801a71827d65734206ddc3741792029,
//   function `linear_expansion_sum_zeroelim` (with its macros `Two_Sum`, `Fast_Two_Sum`),
//   by Jonathan Richard Shewchuk, placed in the public domain by its author.
// Translated to Rust and modified: an `Exp` value instead of caller arrays and
//   lengths, the merge read without reading past an operand's end, zero as the
//   empty expansion, empty operands handled; the floating-point operations and
//   their order unchanged.
// See THIRD_PARTY.md, row L5.

//! Floating-point expansions: exact values as unevaluated sums of `f64`
//! components, the arithmetic of J. R. Shewchuk, "Adaptive Precision
//! Floating-Point Arithmetic and Fast Robust Geometric Predicates",
//! Discrete & Computational Geometry 18(3), 1997 (theorem numbers below
//! are that article's). The exact flat predicates evaluate in [`Exp`]
//! once their floating-point filter fails.
//!
//! **An [`Exp`]** holds its components in increasing magnitude, none zero
//! (zero is the empty expansion), and nonoverlapping: each component's
//! highest set bit is below the next one's lowest. So the sign of the
//! value is that of its largest component. Its operations, exact:
//!
//! - [`Exp::from_f64`], [`Exp::of_sum`] (`two_sum`), [`Exp::of_product`]
//!   (`two_product`): one or two components from `f64`s;
//! - `&e + &f`, `&e - &f`: `linear_expansion_sum_zeroelim`;
//! - `&e * b` for an `f64` `b`: `scale_expansion_zeroelim`;
//! - `&e * &f`: the sum of the scalings of the longer operand by each
//!   component of the shorter one;
//! - `-e`, exact by flipping signs.
//!
//! **What is proven.** Every operation takes nonoverlapping operands to
//! an exact nonoverlapping result, in increasing magnitude, so by
//! induction every [`Exp`] is nonoverlapping and its sign is its largest
//! component's (below):
//!
//! - `two_sum` and `two_product` give an exact pair whose error is at
//!   most half an ulp of the rounded value, so the two don't overlap;
//! - a sum, Theorem 24 (Appendix A; Shewchuk's `linear_expansion_sum`):
//!   the merged components are added smallest first to a running total
//!   kept as two terms, and the output is nonoverlapping under any
//!   tiebreaking rule. Dropping zero components keeps the form (zero
//!   overlaps nothing). A difference is the sum with the negation;
//! - a scaling, Theorem 19 (`scale_expansion`);
//! - a product of expansions: scalings (Theorem 19) summed one by one
//!   (Theorem 24);
//! - a negation flips signs, which keeps the bits of every component.
//!
//! Debug builds check each sum's operands and every result's form, and
//! that each `fast_two_sum` gets `|a| ≥ |b|` (Shewchuk's Theorem 6).
//! Shewchuk's faster `fast_expansion_sum` (Theorem 13), which `robust`
//! and his own predicates use, is not used: its claim that strongly
//! nonoverlapping operands give a strongly nonoverlapping sum fails on a
//! tie rounded to even, and merely nonoverlapping operands can give an
//! overlapping sum (both cases are in the tests).
//!
//! **Sign.** Any nonoverlapping expansion has the sign of its largest
//! component, so [`Exp::sign`] reads the last one. By induction, the sum
//! of the components up to `h_k` is below `2^(hi(h_k)+1)` in magnitude
//! (`hi`, `lo`: the highest and lowest set bit). Nonoverlap gives
//! `hi(h_k) < lo(h_{k+1})`, so the components below the largest one sum
//! to less than `2^lo(h_n) ≤ |h_n|`. Strong nonoverlap isn't needed.
//!
//! **Range.** Shewchuk's model has no overflow or underflow. Every
//! operation here rounds an exact value that is a multiple of `2⁻¹⁰⁷⁴` (any
//! sum of `f64`s; products within the range below), and IEEE arithmetic
//! rounds such a value as that model does: exactly below `2⁻¹⁰²²`, in the
//! same format above. So the theorems hold down to the subnormals.
//! `two_sum(a, b)` is exact for `|a|, |b| < 2¹⁰²³` and a finite sum: at the
//! top, its `x − a` can overflow although `x` doesn't
//! (`two_sum(−6.837409925219451e306, f64::MAX)` gives a NaN error). A sum
//! of expansions is exact and nonoverlapping when every component of both
//! operands is below `2¹⁰²¹` in magnitude: each operand's components then
//! add up to less than `2¹⁰²¹` (the sign proof's bound), so no partial sum
//! reaches `2¹⁰²²` and every `two_sum` stays in its range. Above, a partial
//! sum can overflow although the result is finite: `[1.5·2¹⁰²³] − [2¹⁰²³ −
//! 2⁹⁷⁰, 2¹⁰²³]` gives NaNs, not `−2¹⁰²² + 2⁹⁷⁰`. A product `a·b` is exact
//! when `|a|, |b| < 2⁹⁹⁶` (so the Dekker split doesn't overflow), the
//! product is finite, and the exponents satisfy `e_a + e_b ≥ −970` (`a =
//! m·2^e_a`, `1 ≤ |m| < 2`), as `ulp(a)·ulp(b) ≥ 2⁻¹⁰⁷⁴` then and every
//! partial product and error term is a representable multiple of it; `|a·b|
//! ≥ 2⁻⁹⁶⁸` is enough. Scaling forms the product of every component with
//! `b` and sums them, and a product of expansions sums its scalings: those
//! products must be below `2¹⁰²¹` too. The callers keep their inputs in
//! such a range (stored coordinates are bounded): a caller's guarantee, not
//! checked in release builds. Debug builds check the split's bound and that
//! every component is finite. A product that underflows can't be told from
//! an exact one afterwards.
//!
//! **No FMA.** Products are Dekker's: a Veltkamp split of each factor into
//! 26-bit halves and the four partial products, in Shewchuk's order. Rust
//! never contracts `a·b + c` into a fused multiply-add (and the crate bans
//! `mul_add`), so the components are the same bits with and without
//! hardware FMA.

use std::cmp::Ordering;
use std::ops::{Add, Mul, Neg, Sub};

use smallvec::SmallVec;

/// Components kept inline: enough for the products and 2×2 determinants
/// of `f64` inputs, the common case of a predicate's exact stage.
const INLINE: usize = 8;

/// `2²⁷ + 1`: multiplying by it splits an `f64` into two halves of at most
/// 26 bits each ([`split`]).
const SPLITTER: f64 = 134_217_729.0;

/// An exact value as a floating-point expansion: the sum of its
/// components, in increasing magnitude, none zero, nonoverlapping (module
/// docs). Zero is the empty expansion. One value has many expansions,
/// so there is no `==`: compare values by the sign of a difference.
#[derive(Clone, Debug, Default)]
pub struct Exp(SmallVec<[f64; INLINE]>);

impl Exp {
    /// Zero, the empty expansion.
    pub fn zero() -> Exp {
        Exp(SmallVec::new())
    }

    /// `x`, exactly: one component, or none for a zero of either sign.
    pub fn from_f64(x: f64) -> Exp {
        let mut h = SmallVec::new();
        if x != 0.0 {
            h.push(x);
        }
        Exp::checked(h)
    }

    /// `a + b`, exactly ([`two_sum`]), for `|a|, |b| < 2¹⁰²³` and a finite
    /// sum.
    pub fn of_sum(a: f64, b: f64) -> Exp {
        let (x, y) = two_sum(a, b);
        Exp::from_pair(x, y)
    }

    /// `a · b`, exactly ([`two_product`]), within the module's range.
    pub fn of_product(a: f64, b: f64) -> Exp {
        let (x, y) = two_product(a, b);
        Exp::from_pair(x, y)
    }

    /// The expansion `hi + lo` of a pair with `lo` the rounding error of
    /// `hi`, its zeros dropped.
    fn from_pair(hi: f64, lo: f64) -> Exp {
        let mut h = SmallVec::new();
        if lo != 0.0 {
            h.push(lo);
        }
        if hi != 0.0 {
            h.push(hi);
        }
        Exp::checked(h)
    }

    /// `h` as an expansion; debug builds check its form (module docs).
    fn checked(h: SmallVec<[f64; INLINE]>) -> Exp {
        debug_assert!(is_expansion(&h), "not an expansion: {h:?}");
        Exp(h)
    }

    /// The components, in increasing magnitude, none zero.
    pub fn components(&self) -> &[f64] {
        &self.0
    }

    /// Whether the value is zero.
    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    /// The sign of the exact value, that of its largest component.
    pub fn sign(&self) -> Ordering {
        match self.0.last() {
            Some(&x) if x > 0.0 => Ordering::Greater,
            Some(_) => Ordering::Less,
            None => Ordering::Equal,
        }
    }

    /// `e · b`, exactly, within the module's range:
    /// `scale_expansion_zeroelim`, nonoverlapping for a nonoverlapping `e`
    /// (Shewchuk's Theorem 19).
    pub fn scale(&self, b: f64) -> Exp {
        let e = &self.0;
        let Some((&first, rest)) = e.split_first() else {
            return Exp::zero();
        };
        let mut h = SmallVec::with_capacity(2 * e.len());
        let (bhi, blo) = split(b);
        let (mut q, hh) = two_product_presplit(first, b, bhi, blo);
        if hh != 0.0 {
            h.push(hh);
        }
        for &enow in rest {
            let (product1, product0) = two_product_presplit(enow, b, bhi, blo);
            let (sum, hh) = two_sum(q, product0);
            if hh != 0.0 {
                h.push(hh);
            }
            let (new_q, hh) = fast_two_sum(product1, sum);
            q = new_q;
            if hh != 0.0 {
                h.push(hh);
            }
        }
        // Shewchuk keeps a lone zero component; here zero is empty.
        if q != 0.0 {
            h.push(q);
        }
        Exp::checked(h)
    }

    /// `e + f`, exactly and nonoverlapping, for components below `2¹⁰²¹`
    /// (module docs):
    /// `linear_expansion_sum_zeroelim` (Shewchuk's Theorem 24). It merges
    /// the components by magnitude and adds each, smallest first, to a
    /// running total kept as two terms `Q + q`: the next component plus
    /// `q` by `fast_two_sum`, whose error is an output component, and the
    /// rounded sum into `Q` by `two_sum`.
    pub fn sum(&self, other: &Exp) -> Exp {
        let (e, f) = (&self.0, &other.0);
        // The theorem's hypothesis: both operands nonoverlapping, in
        // increasing magnitude.
        debug_assert!(is_expansion(e), "not an expansion: {e:?}");
        debug_assert!(is_expansion(f), "not an expansion: {f:?}");
        if e.is_empty() {
            return other.clone();
        }
        if f.is_empty() {
            return self.clone();
        }
        let (mut eindex, mut findex) = (0, 0);
        // The next component of the merge. `(fnow > enow) == (fnow >
        // -enow)`: `|enow| < |fnow|`, or a tie with `fnow` positive;
        // Shewchuk's comparison, kept as written.
        let mut next = || {
            let take_e = eindex < e.len() && {
                findex >= f.len() || {
                    let (enow, fnow) = (e[eindex], f[findex]);
                    (fnow > enow) == (fnow > -enow)
                }
            };
            if take_e {
                eindex += 1;
                e[eindex - 1]
            } else {
                findex += 1;
                f[findex - 1]
            }
        };
        let mut h = SmallVec::with_capacity(e.len() + f.len());
        let g0 = next();
        let g1 = next();
        let (mut big_q, mut q) = fast_two_sum(g1, g0);
        for _ in 2..e.len() + f.len() {
            let g = next();
            let (r, hh) = fast_two_sum(g, q);
            (big_q, q) = two_sum(big_q, r);
            if hh != 0.0 {
                h.push(hh);
            }
        }
        if q != 0.0 {
            h.push(q);
        }
        // Shewchuk keeps a lone zero component; here zero is empty.
        if big_q != 0.0 {
            h.push(big_q);
        }
        Exp::checked(h)
    }
}

impl Neg for Exp {
    type Output = Exp;

    /// `−e`, exactly: every component's sign flipped.
    fn neg(mut self) -> Exp {
        for x in &mut self.0 {
            *x = -*x;
        }
        self
    }
}

impl Neg for &Exp {
    type Output = Exp;

    fn neg(self) -> Exp {
        -self.clone()
    }
}

impl Add for &Exp {
    type Output = Exp;

    /// [`Exp::sum`].
    fn add(self, other: &Exp) -> Exp {
        self.sum(other)
    }
}

impl Sub for &Exp {
    type Output = Exp;

    /// `e − f` as `e + (−f)`, exactly.
    fn sub(self, other: &Exp) -> Exp {
        self.sum(&-other)
    }
}

impl Mul<&Exp> for &Exp {
    type Output = Exp;

    /// `e · f`, exactly, within the module's range: the longer operand
    /// scaled by each component of the shorter one, the products summed
    /// from the smallest component up.
    fn mul(self, other: &Exp) -> Exp {
        let (long, short) = if self.0.len() >= other.0.len() {
            (self, other)
        } else {
            (other, self)
        };
        short
            .0
            .iter()
            .fold(Exp::zero(), |acc, &x| acc.sum(&long.scale(x)))
    }
}

impl Mul<f64> for &Exp {
    type Output = Exp;

    /// [`Exp::scale`].
    fn mul(self, b: f64) -> Exp {
        self.scale(b)
    }
}

/// Whether `c` has an expansion's form: components finite, none zero, in
/// increasing magnitude and nonoverlapping (each one's highest set bit
/// below the next one's lowest).
fn is_expansion(c: &[f64]) -> bool {
    c.iter().all(|x| x.is_finite() && *x != 0.0)
        && c.windows(2).all(|w| bit_span(w[0]).1 < bit_span(w[1]).0)
}

/// The exponents of the lowest and highest set bits of a finite nonzero
/// `x`.
fn bit_span(x: f64) -> (i32, i32) {
    let bits = x.to_bits();
    let field = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1 << 52) - 1);
    let (m, e) = if field == 0 {
        (frac, -1074)
    } else {
        (frac | 1 << 52, field - 1075)
    };
    (
        e + m.trailing_zeros() as i32,
        e + 63 - m.leading_zeros() as i32,
    )
}

/// `(x, y)` with `x = fl(a + b)` and `x + y = a + b` exactly (Knuth's
/// two-sum), for `|a|, |b| < 2¹⁰²³` and a finite `x`; at the top `x − a`
/// can overflow (module docs).
#[inline]
pub fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    let bvirt = x - a;
    let avirt = x - bvirt;
    let bround = b - bvirt;
    let around = a - avirt;
    (x, around + bround)
}

/// [`two_sum`] for `|a| ≥ |b|` (or `a` zero), in three operations
/// (Dekker's fast two-sum).
#[inline]
pub fn fast_two_sum(a: f64, b: f64) -> (f64, f64) {
    // NaN passes: the result's form check reports it.
    debug_assert!(
        a.abs().partial_cmp(&b.abs()) != Some(Ordering::Less),
        "fast_two_sum({a:e}, {b:e})"
    );
    let x = a + b;
    let bvirt = x - a;
    (x, b - bvirt)
}

/// Veltkamp's split: `(hi, lo)` with `hi + lo = a` exactly, each
/// representable in 26 bits (Shewchuk's Theorem 17), so products of halves
/// are exact; for `|a| < 2⁹⁹⁶`.
#[inline]
pub fn split(a: f64) -> (f64, f64) {
    // 2⁹⁹⁶; NaN fails too.
    debug_assert!(a.abs() < f64::from_bits(2019 << 52), "split of {a:e}");
    let c = SPLITTER * a;
    let abig = c - a;
    let ahi = c - abig;
    let alo = a - ahi;
    (ahi, alo)
}

/// `(x, y)` with `x = fl(a·b)` and `x + y = a·b` exactly, within the
/// module's range (Dekker's two-product, without FMA).
#[inline]
pub fn two_product(a: f64, b: f64) -> (f64, f64) {
    let x = a * b;
    (x, two_product_tail(a, b, x))
}

/// The error `a·b − x` of the rounded product `x = fl(a·b)`.
#[inline]
fn two_product_tail(a: f64, b: f64, x: f64) -> f64 {
    let (ahi, alo) = split(a);
    let (bhi, blo) = split(b);
    let err1 = x - (ahi * bhi);
    let err2 = err1 - (alo * bhi);
    let err3 = err2 - (ahi * blo);
    (alo * blo) - err3
}

/// [`two_product`] with `b` already split into `(bhi, blo)`.
#[inline]
fn two_product_presplit(a: f64, b: f64, bhi: f64, blo: f64) -> (f64, f64) {
    let x = a * b;
    let (ahi, alo) = split(a);
    let err1 = x - ahi * bhi;
    let err2 = err1 - alo * bhi;
    let err3 = err2 - ahi * blo;
    let y = alo * blo - err3;
    (x, y)
}

#[cfg(test)]
mod tests;
