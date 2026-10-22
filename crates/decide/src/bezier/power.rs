//! The power basis: a Bernstein form's coefficients in powers of the
//! parameter, and back, at every level and exactly on rationals.
//!
//! **To powers.** The polynomial of degree `n` with Bernstein
//! coefficients `bᵢ` is `Σₖ aₖ·tᵏ` with
//! `aₖ = C(n, k)·Σᵢ₌₀ᵏ (−1)ᵏ⁻ⁱ·C(k, i)·bᵢ`, the inner sum in increasing
//! `i` ([`Poly::to_power`]). **From powers.** On `[0, 1]` at degree
//! `n`, `bₖ = Σᵢ₌₀ᵏ C(k, i)/C(n, i)·aᵢ` ([`Poly::from_power`]), each
//! term weighted, then divided. The binomials are exact `u64`s
//! ([`MAX_DEGREE`]), their products formed as in the products' weights;
//! a weight of 1 is no operation. At a [`Bounded`] level each result
//! carries its certified bound (the alternating sums cancel: the bound
//! grows with the degree); with [`malachite::Rational`] coefficients
//! both are exact, the instance `crate::num::poly` uses.
//!
//! A tensor converts line by line, in `u`, then in `v`
//! ([`Tensor::to_power`], [`Tensor::from_power`]); its power
//! coefficients `a_ij` of `uⁱ·vʲ` are in the net's order.
//!
//! [`Bounded`]: crate::num::bounded::Bounded

use super::poly::{Coefs, MAX_DEGREE, Poly, binom, divide, weigh};
use super::scalar::Scalar;
use super::tensor::{Dir, Tensor};

/// The power coefficients of the Bernstein coefficients `b` (non-empty,
/// at most [`MAX_DEGREE`] + 1).
pub(super) fn to_power<S: Scalar>(b: &[S]) -> Coefs<S> {
    let n = b.len() - 1;
    (0..=n)
        .map(|k| {
            let mut acc: Option<S> = None;
            for (i, bi) in b.iter().enumerate().take(k + 1) {
                let t = weigh(binom(k, i), bi.clone());
                let minus = (k - i) % 2 == 1;
                acc = Some(match acc {
                    None if minus => t.neg(),
                    None => t,
                    Some(a) if minus => a.sub(&t),
                    Some(a) => a.add(&t),
                });
            }
            weigh(binom(n, k), acc.expect("i = 0 at least"))
        })
        .collect()
}

/// The Bernstein coefficients of degree `n` (at most [`MAX_DEGREE`]) of
/// the power coefficients `a` (at most `n + 1`; none is the zero
/// polynomial, at `like`'s level).
pub(super) fn from_power<S: Scalar>(a: &[S], n: usize, like: &S) -> Coefs<S> {
    debug_assert!(a.len() <= n + 1 && n <= MAX_DEGREE);
    (0..=n)
        .map(|k| {
            let mut acc: Option<S> = None;
            for (i, ai) in a.iter().enumerate().take(k + 1) {
                let t = divide(weigh(binom(k, i), ai.clone()), binom(n, i));
                acc = Some(match acc {
                    None => t,
                    Some(s) => s.add(&t),
                });
            }
            acc.unwrap_or_else(|| S::int(0, like))
        })
        .collect()
}

impl<S: Scalar> Poly<S> {
    /// The coefficients `a₀, …, aₙ` of `p(t) = Σ aₖ·tᵏ` (module docs),
    /// of the polynomial's degree (trailing zeros kept).
    pub fn to_power(&self) -> Vec<S> {
        to_power(self.coefs()).into_vec()
    }

    /// The polynomial `Σ aₖ·tᵏ` in Bernstein form of degree `n` (module
    /// docs), `None` unless `a.len() ≤ n + 1 ≤ MAX_DEGREE + 1`; no
    /// coefficients is the zero polynomial, at `like`'s level.
    pub fn from_power(a: &[S], n: usize, like: &S) -> Option<Poly<S>> {
        (n <= MAX_DEGREE && a.len() <= n + 1).then(|| Poly::from_coefs(from_power(a, n, like)))
    }
}

impl<S: Scalar> Tensor<S> {
    /// The coefficients `a_ij` of `f(u, v) = Σ a_ij·uⁱ·vʲ`, row-major in
    /// `u` as the net: each line in `u` to powers, then each in `v`.
    pub fn to_power(&self) -> Vec<S> {
        self.map_lines(Dir::U, |p| to_power(p.coefs()))
            .map_lines(Dir::V, |p| to_power(p.coefs()))
            .coefs()
            .to_vec()
    }

    /// The form of degrees `(m, n)` with the power coefficients `a`
    /// (row-major in `u`, `(m + 1)·(n + 1)` of them): each line in `u`
    /// from powers, then each in `v`. `None` past [`MAX_DEGREE`] or for
    /// another count.
    pub fn from_power(m: usize, n: usize, a: &[S]) -> Option<Tensor<S>> {
        let like = a.first()?.clone();
        let f = Tensor::new(m, n, a.iter().cloned())?;
        Some(
            f.map_lines(Dir::U, |p| from_power(p.coefs(), m, &like))
                .map_lines(Dir::V, |p| from_power(p.coefs(), n, &like)),
        )
    }
}

#[cfg(test)]
mod tests;
