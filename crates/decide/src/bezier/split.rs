//! Subdivision by de Casteljau: at ½, symmetric to the bit, and at any
//! `t`.
//!
//! **Same bits under reversal.** At ½ every step is a midpoint
//! ([`Scalar::mid`]), symmetric in its two operands to the bit, so the
//! reversed polynomial's halves are this one's, swapped and reversed:
//! `p.reversed().split_half() == [right.reversed(), left.reversed()]`.
//! A curve shared by two patches that run it opposite ways splits into
//! the same pieces from either side, which a split at a general `t`
//! (where `1 − t` rounds) doesn't give.

use super::poly::{Coefs, Poly};
use super::scalar::Scalar;

impl<S: Scalar> Poly<S> {
    /// The halves on `[0, ½]` and `[½, 1]`, each as a polynomial on
    /// `[0, 1]`, by midpoints. The left half starts with `b₀` and the
    /// right ends with `bₙ`, to the bit, and they share the middle value.
    pub fn split_half(&self) -> [Poly<S>; 2] {
        self.split_by(|a, b| a.mid(b))
    }

    /// The pieces on `[0, t]` and `[t, 1]`, each as a polynomial on
    /// `[0, 1]`, by `bᵢ ← (1 − t)·bᵢ + t·bᵢ₊₁` (any `t`; outside `(0, 1)`
    /// they extend the polynomial). The ends as in
    /// [`split_half`](Self::split_half); for the same bits under reversal
    /// split at ½ by that.
    pub fn split_at(&self, t: &S) -> [Poly<S>; 2] {
        let s = S::int(1, t).sub(t);
        self.split_by(|a, b| s.mul(a).add(&t.mul(b)))
    }

    /// De Casteljau's triangle by the step `f(bᵢ, bᵢ₊₁)`: its left side
    /// is the left piece, its right side read upwards the right piece.
    fn split_by(&self, f: impl Fn(&S, &S) -> S) -> [Poly<S>; 2] {
        let n = self.degree();
        let mut w: Coefs<S> = self.coefs().iter().cloned().collect();
        let mut left = Coefs::with_capacity(n + 1);
        let mut right = w.clone();
        left.push(w[0].clone());
        for k in 1..=n {
            for i in 0..=n - k {
                w[i] = f(&w[i], &w[i + 1]);
            }
            left.push(w[0].clone());
            right[n - k] = w[n - k].clone();
        }
        [Poly::from_coefs(left), Poly::from_coefs(right)]
    }
}
