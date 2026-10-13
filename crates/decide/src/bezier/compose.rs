//! Compositions with linear forms: a rational span or patch put into a
//! line or plane equation, a polynomial (a tensor) whose sign is the
//! side of the equation, and the linear image of a span, such as its
//! shadow on a plane.
//!
//! In homogeneous coordinates the equation `a·x + c = 0` at the point
//! `x = P / W` is `a·P + c·W = 0`: a combination of the coordinate
//! polynomials, of the span's own degree, coefficient by coefficient.
//! Each coefficient is the sum `a₁·P₁ + a₂·P₂ + … + c·W` in that order,
//! a factor exactly 0 left out, exactly 1 or −1 a copy or negation (no
//! rounding); so the composition of the reversed span is the reversed
//! composition, to the bit, and an image keeps the weights' bits.

use super::curve::HomCurve;
use super::poly::{Coefs, Poly};
use super::scalar::Scalar;
use super::tensor::{HomPatch, Tensor};

/// `Σ fₖ·xₖ` in order, a factor exactly 0 left out, ±1 no rounding;
/// zero (at the level of the last `x`) when every factor is 0.
fn combine<'a, S: Scalar + 'a>(terms: impl IntoIterator<Item = (&'a S, &'a S)>) -> S {
    let mut acc: Option<S> = None;
    let mut last = None;
    for (f, x) in terms {
        last = Some(x);
        let t = if f.is_int(0) {
            continue;
        } else if f.is_int(1) {
            x.clone()
        } else if f.neg().is_int(1) {
            x.neg()
        } else {
            f.mul(x)
        };
        acc = Some(match acc {
            None => t,
            Some(a) => a.add(&t),
        });
    }
    acc.unwrap_or_else(|| S::int(0, last.expect("at least one term")))
}

impl<S: Scalar, const N: usize> HomCurve<S, N> {
    /// The span in the equation `a·x + c`: the polynomial
    /// `a·P + c·W`, of the span's degree, whose sign at `t` is the side
    /// of `x(t)` where `W(t) > 0` (a plane `n·x = d` for `N = 3` with
    /// `a = n`, `c = −d`; a line in the plane for `N = 2`). Commutes with
    /// reversal to the bit; exact on rationals.
    pub fn compose_linear(&self, a: &[S; N], c: &S) -> Poly<S> {
        let n = self.degree();
        let coefs: Coefs<S> = (0..=n)
            .map(|i| {
                let xs = self.coords().iter().map(|p| &p.coefs()[i]);
                combine(
                    a.iter()
                        .zip(xs)
                        .chain(std::iter::once((c, &self.weights().coefs()[i]))),
                )
            })
            .collect();
        Poly::from_coefs(coefs)
    }

    /// The image under the linear map `x ↦ A·x` (`rows` the rows of `A`):
    /// the span `(A·P, W)`, each row by [`compose_linear`] with `c = 0`,
    /// the weights the same bits.
    ///
    /// [`compose_linear`]: Self::compose_linear
    pub fn map_linear<const M: usize>(&self, rows: &[[S; N]; M]) -> HomCurve<S, M> {
        let zero = S::int(0, &self.weights().coefs()[0]);
        let p = std::array::from_fn(|r| self.compose_linear(&rows[r], &zero));
        HomCurve::from_polys(p, self.weights().clone()).expect("one degree")
    }
}

impl<S: Scalar> HomCurve<S, 3> {
    /// The shadow on a plane with the in-plane axes `ex` and `ey` (any
    /// vectors; perpendicular to the projection direction for a shadow
    /// along it): the plane span `(ex·P, ey·P, W)`, its point
    /// `(ex·x, ey·x)`. Exact on rationals; the weights the same bits.
    pub fn shadow(&self, ex: &[S; 3], ey: &[S; 3]) -> HomCurve<S, 2> {
        self.map_linear(&[ex.clone(), ey.clone()])
    }
}

impl<S: Scalar> HomPatch<S> {
    /// The patch in the plane equation `a·x + c`: the tensor
    /// `a·(X, Y, Z) + c·W`, of the patch's degrees, the side of the plane
    /// where `W > 0`. Commutes with reversal and transposition to the
    /// bit; exact on rationals.
    pub fn compose_linear(&self, a: &[S; 3], c: &S) -> Tensor<S> {
        let [x, y, z, w] = self.tensors();
        let (m, n) = self.degrees();
        let coefs = (0..x.coefs().len()).map(|i| {
            combine([
                (&a[0], &x.coefs()[i]),
                (&a[1], &y.coefs()[i]),
                (&a[2], &z.coefs()[i]),
                (c, &w.coefs()[i]),
            ])
        });
        Tensor::new(m, n, coefs).expect("the patch's degrees")
    }
}

#[cfg(test)]
mod tests;
