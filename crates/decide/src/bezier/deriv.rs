//! Derivatives: hodographs of any order, the numerators of a rational
//! form's derivatives by the quotient rule in homogeneous form, and the
//! derivatives of a rational form's point at a parameter.
//!
//! **Hodographs.** The derivative of a polynomial of degree `n` has the
//! Bernstein coefficients `n·(bᵢ₊₁ − bᵢ)` ([`Poly::derivative`]); the
//! `k`-th, of degree `n − k`, is that step `k` times
//! ([`Poly::derivative_n`], [`Tensor::derivative_n`] in one direction).
//! The derivative of the reversed form is the reversed derivative
//! negated, exactly but for the signs of zeros.
//!
//! **Numerators.** The point of a rational form is `x = H / W`. Its
//! derivative is `x' = (W·H' − W'·H) / W²`, so with `W > 0` the
//! numerator `W·H' − W'·H` points along `x'` wherever it isn't zero: the
//! tangent numerator of a span ([`HomCurve::tangent_numerator`], degree
//! `2n − 1`) and the partial numerators `W·H_u − W_u·H`, `W·H_v − W_v·H`
//! of a patch ([`HomPatch::partial_numerator`], degrees `(2m − 1, 2n)`
//! and `(2m, 2n − 1)`), the nets the graph certificate takes its vectors
//! from. Their degrees follow from the operands' (the exact numerators
//! are a degree lower, not reduced here). A polynomial form, every weight
//! exactly 1, has `W' = 0` and `W = 1`: its numerator is the hodograph
//! `H'` itself, of degree `n − 1` (`(m − 1, n)`, `(m, n − 1)`). The
//! numerator of the form reversed in its direction is the reversed
//! numerator negated (in the other direction of a patch, reversed), and
//! a patch's numerator in `v` is the transposed patch's in `u`,
//! transposed, exactly but for the signs of zeros.
//!
//! **Derivatives at a point.** From `H = W·x` by Leibniz' rule,
//! `x⁽ᵏ⁾ = (H⁽ᵏ⁾ − Σⱼ₌₁ᵏ C(k, j)·W⁽ʲ⁾·x⁽ᵏ⁻ʲ⁾) / W`, and for a patch the
//! same over both directions: every derivative up to an order
//! ([`HomCurve::derivatives_at`], [`HomPatch::derivatives_at`]), each
//! carrying its certified error at the level of the form's scalar
//! (certified only where `W` is certainly non-zero). The first order is
//! [`HomPatch::eval_partials`] to the bit.

use super::curve::HomCurve;
use super::poly::{Poly, binom, weigh};
use super::scalar::Scalar;
use super::tensor::{Dir, HomPatch, Tensor};

impl<S: Scalar> Poly<S> {
    /// The `k`-th derivative, of degree `n − k` ([`derivative`] `k`
    /// times); past the degree the zero polynomial of degree 0.
    ///
    /// [`derivative`]: Self::derivative
    pub fn derivative_n(&self, k: usize) -> Self {
        let mut p = self.clone();
        for _ in 0..k.min(self.degree() + 1) {
            p = p.derivative();
        }
        p
    }
}

impl<S: Scalar> Tensor<S> {
    /// The `k`-th partial derivative in `dir` ([`derivative`] `k` times).
    ///
    /// [`derivative`]: Self::derivative
    pub fn derivative_n(&self, dir: Dir, k: usize) -> Self {
        let mut f = self.clone();
        for _ in 0..k.min(self.degree(dir) + 1) {
            f = f.derivative(dir);
        }
        f
    }
}

/// Whether every coefficient is exactly the integer 1.
fn all_one<S: Scalar>(c: &[S]) -> bool {
    c.iter().all(|w| w.is_int(1))
}

impl<S: Scalar, const N: usize> HomCurve<S, N> {
    /// The tangent numerator `W·P' − W'·P`, equal to `W²·x'` for the
    /// point `x = P / W`: with `W > 0` the tangent's direction wherever it
    /// isn't zero. Of degree `2n − 1` for degree `n ≥ 1`, and for a
    /// polynomial span (every weight exactly 1) the hodograph `P'` itself,
    /// of degree `n − 1`; `None` past [`MAX_DEGREE`](super::MAX_DEGREE).
    /// The reversed span gives the reversed numerator negated, to the bit
    /// but the signs of zeros.
    pub fn tangent_numerator(&self) -> Option<[Poly<S>; N]> {
        let d = self.derivative();
        if all_one(self.weights().coefs()) {
            return Some(d.coords().clone());
        }
        let (w, dw) = (self.weights(), d.weights());
        let mut out: [Option<Poly<S>>; N] = std::array::from_fn(|_| None);
        for (k, o) in out.iter_mut().enumerate() {
            *o = Some(w.mul(&d.coords()[k])?.sub(&dw.mul(&self.coords()[k])?));
        }
        Some(out.map(|p| p.expect("filled")))
    }

    /// The point and its derivatives at `t` up to `order`: `[x, x', …,
    /// x⁽ᵒʳᵈᵉʳ⁾]`, by Leibniz' rule from the homogeneous derivatives
    /// (module docs).
    pub fn derivatives_at(&self, t: &S, order: usize) -> Vec<[S; N]> {
        let mut h = self.clone();
        let mut hom = Vec::with_capacity(order + 1);
        for k in 0..=order {
            if k > 0 {
                h = h.derivative();
            }
            hom.push(h.eval_hom(t));
        }
        let w = hom[0].1.clone();
        let mut x: Vec<[S; N]> = Vec::with_capacity(order + 1);
        for k in 0..=order {
            let mut acc = hom[k].0.clone();
            for j in 1..=k {
                let c = binom(k, j);
                for (a, xk) in acc.iter_mut().zip(&x[k - j]) {
                    *a = a.sub(&weigh(c, hom[j].1.mul(xk)));
                }
            }
            x.push(acc.map(|a| a.div(&w)));
        }
        x
    }
}

impl<S: Scalar> HomPatch<S> {
    /// The partial numerator in `dir`, `W·H_u − W_u·H` (or in `v`), for
    /// `H = (X, Y, Z)`: equal to `W²·x_u`, so with `W > 0` along the
    /// point's partial wherever it isn't zero. Of degrees `(2m − 1, 2n)`
    /// in `u` and `(2m, 2n − 1)` in `v` for degrees `(m, n) ≥ (1, 1)`,
    /// and for a polynomial patch (every weight exactly 1) the hodograph
    /// `H_u` itself, of degrees `(m − 1, n)` (or `(m, n − 1)`); `None`
    /// past [`MAX_DEGREE`](super::MAX_DEGREE). Reversal and transposition
    /// as in the module docs.
    pub fn partial_numerator(&self, dir: Dir) -> Option<[Tensor<S>; 3]> {
        let d = self.derivative(dir);
        let [x, y, z, w] = self.tensors();
        let [dx, dy, dz, dw] = d.tensors();
        if all_one(w.coefs()) {
            return Some([dx.clone(), dy.clone(), dz.clone()]);
        }
        let num = |h: &Tensor<S>, dh: &Tensor<S>| Some(w.mul(dh)?.sub(&dw.mul(h)?));
        Some([num(x, dx)?, num(y, dy)?, num(z, dz)?])
    }

    /// The point and its partial derivatives at `(u, v)` up to `order`
    /// (total), by Leibniz' rule from the homogeneous ones (module docs):
    /// `x^(a, b) = (H^(a, b) − Σ C(a, i)·C(b, j)·W^(i, j)·x^(a − i, b − j))
    /// / W`, `(i, j) ≠ (0, 0)` in increasing `i`, then `j`.
    pub fn derivatives_at(&self, u: &S, v: &S, order: usize) -> Derivatives<S> {
        let mut hom: Vec<[S; 4]> = Vec::with_capacity(tri(order + 1, order + 1));
        let mut along_u = self.clone();
        for a in 0..=order {
            if a > 0 {
                along_u = along_u.derivative(Dir::U);
            }
            let mut h = along_u.clone();
            for b in 0..=order - a {
                if b > 0 {
                    h = h.derivative(Dir::V);
                }
                hom.push(h.eval_hom(u, v));
            }
        }
        let at = |a: usize, b: usize| tri_index(order, a, b);
        let w = hom[0][3].clone();
        let mut x: Vec<[S; 3]> = Vec::with_capacity(hom.len());
        for a in 0..=order {
            for b in 0..=order - a {
                let h = &hom[at(a, b)];
                let mut acc = [h[0].clone(), h[1].clone(), h[2].clone()];
                for i in 0..=a {
                    for j in 0..=b {
                        if i == 0 && j == 0 {
                            continue;
                        }
                        let c = binom(a, i) * binom(b, j);
                        let wij = &hom[at(i, j)][3];
                        let xr = &x[at(a - i, b - j)];
                        for (s, xk) in acc.iter_mut().zip(xr) {
                            *s = s.sub(&weigh(c, xk.mul(wij)));
                        }
                    }
                }
                x.push(acc.map(|s| s.div(&w)));
            }
        }
        Derivatives { order, x }
    }
}

/// The derivatives of a patch's point at a parameter up to an order
/// ([`HomPatch::derivatives_at`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Derivatives<S> {
    order: usize,
    /// `x^(a, b)` for `a + b ≤ order`, by `a`, then `b`.
    x: Vec<[S; 3]>,
}

impl<S> Derivatives<S> {
    /// The highest total order held.
    pub fn order(&self) -> usize {
        self.order
    }

    /// `∂^(a + b) x / ∂uᵃ ∂vᵇ`, `None` past the order.
    pub fn get(&self, a: usize, b: usize) -> Option<&[S; 3]> {
        (a.checked_add(b)? <= self.order).then(|| &self.x[tri_index(self.order, a, b)])
    }
}

/// The number of entries `(a', b)` with `a' < a` and `a' + b < len`.
fn tri(len: usize, a: usize) -> usize {
    // Σ_{a' < a} (len − a').
    a * len - a * a.saturating_sub(1) / 2
}

/// The index of `(a, b)`, `a + b ≤ order`, in the order `a`, then `b`.
fn tri_index(order: usize, a: usize, b: usize) -> usize {
    tri(order + 1, a) + b
}

#[cfg(test)]
mod tests;
