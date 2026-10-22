//! The normal numerator of a rational patch, its reduced form, and its
//! quotient by the linear factors of singular sides: the fold
//! certificate's forms.
//!
//! **Numerator.** For the net `H = (P, W) = (X, Y, Z, W)` the spatial part
//! of `H ∧ H_u ∧ H_v` is `N = W·(P_u × P_v) + W_u·(P_v × P) + W_v·(P ×
//! P_u)`, equal to `W³·(x_u × x_v)` for the point `x = P / W`: with
//! `W > 0` a normal of the patch's orientation wherever it isn't zero,
//! and the same polynomial for the net translated (the frame a tile
//! computes it in). As computed it has degrees `(3m − 1, 3n − 1)`, `(5,
//! 5)` for a biquadratic patch ([`HomPatch::normal_numerator`]); its true
//! degrees are `(3m − 2, 3n − 2)` (the top terms of `H ∧ H_u` cancel in
//! `u`, of `H ∧ H_v` in `v`), so its coefficients are the exact elevation
//! of the reduced form's, a convex combination of them: a cone or a sign
//! test on the unreduced form is never looser. The fold check and the
//! normal cone take the unreduced form; the reduced one
//! ([`HomPatch::normal_numerator_reduced`], `(4, 4)` for a biquadratic
//! patch) is for a consumer that composes the numerator with further
//! forms and pays for every degree, such as a seam field along a p-curve
//! span (degree `k·(3m − 2) + k·(3n − 2)` for a span of degree `k`
//! instead of `k·(3m − 1) + k·(3n − 1)`). It is the degree reduction of
//! the unreduced form in `u`, then in `v`
//! ([`Tensor::reduced`](super::Tensor::reduced)): sound because the
//! unreduced form is an elevation in truth, its residual and rounding in
//! each coefficient's bound; it cancels, so it often needs a higher level
//! than the unreduced form.
//!
//! **Singular sides.** Along a side that is one point in 3D (a pole or
//! an apex row) `P` and `P_v` (on a `u` side) are multiples of one
//! homogeneous point and the numerator is zero, so it has that side's
//! linear factor (`u`, `1 − u`, `v` or `1 − v`). In floats it cancels
//! there only to `±err`, never to a certified zero. So the numerator's
//! line on the side, which depends on the side's two rows of the net
//! alone ([`HomPatch::side_numerator`]), is proven zero on the exact net
//! ([`HomPatch::singular_sides`], at the rational level), and the proof
//! divides the factor out exactly ([`SingularSides::quotient`],
//! [`Tensor::divide_sides`](super::Tensor::divide_sides)) from the
//! numerator at any level, in any frame, reduced or not: degrees one
//! lower in that direction, `(4, 5)` or `(3, 4)` for a biquadratic patch
//! with one singular side. The quotient is the normal numerator divided
//! by a factor positive inside the square, so a cone of its coefficients
//! certifies the patch regular up to the side, and its limit there is
//! the normal at the side. A cusp (a side where the numerator vanishes
//! to a higher order) is not covered: its quotient is zero on the side.
//!
//! **Same bits under reversal.** The patch reversed in either direction
//! gives the reversed numerator, reduced or not, negated, to the bit but
//! the signs of zeros; its quotient by the mirrored sides is the
//! reversed quotient negated likewise. A side's numerator line is the
//! numerator's line on that side to the bit.

use super::divide::{Side, Sides};
use super::poly::Poly;
use super::product::{Form, cross};
use super::scalar::Scalar;
use super::tensor::{Dir, HomPatch, Tensor};

/// `W·(P_u × P_v) + W_u·(P_v × P) + W_v·(P × P_u)` of forms of either
/// kind, in one order of operations for both.
fn numerator<F: Form>(
    p: &[F; 3],
    w: &F,
    pu: &[F; 3],
    wu: &F,
    pv: &[F; 3],
    wv: &F,
) -> Option<[F; 3]> {
    let term = |k: &F, a: &[F; 3], b: &[F; 3]| {
        let c = cross(a, b)?;
        Some([k.mul(&c[0])?, k.mul(&c[1])?, k.mul(&c[2])?])
    };
    let [a0, a1, a2] = term(w, pu, pv)?;
    let [b0, b1, b2] = term(wu, pv, p)?;
    let [c0, c1, c2] = term(wv, p, pu)?;
    Some([
        a0.add(&b0).add(&c0),
        a1.add(&b1).add(&c1),
        a2.add(&b2).add(&c2),
    ])
}

/// The proof that a patch's normal numerator is zero on some sides
/// ([`HomPatch::singular_sides`]): it divides their linear factors out of
/// the numerator of that net ([`quotient`](Self::quotient)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingularSides {
    degrees: (usize, usize),
    sides: Sides,
}

impl SingularSides {
    /// The proven sides.
    pub fn sides(&self) -> Sides {
        self.sides
    }

    /// The degrees of the proven patch.
    pub fn degrees(&self) -> (usize, usize) {
        self.degrees
    }

    /// The normal numerator `n` divided by the linear factors of the
    /// proven sides ([`Tensor::divide_sides`]), the dropped lines not
    /// read; `None` unless `n` has the degrees of the proven patch's
    /// numerator, unreduced or reduced in each direction
    /// ([`HomPatch::normal_numerator`],
    /// [`HomPatch::normal_numerator_reduced`]), and is above degree 1 in
    /// a direction with both its sides proven. `n` must be the numerator
    /// of the proven net: at any level and in any frame (a translation of
    /// the net leaves the polynomial as it is), its coefficients' bounds
    /// holding the true ones, which the proof makes zero on the sides.
    pub fn quotient<S: Scalar>(&self, n: &[Tensor<S>; 3]) -> Option<[Tensor<S>; 3]> {
        let (m, k) = self.degrees;
        let fits = |d: usize, of: usize| {
            d == (3 * of).saturating_sub(1) || d == (3 * of).saturating_sub(2)
        };
        let d = n[0].degrees();
        if !(n.iter().all(|f| f.degrees() == d) && fits(d.0, m) && fits(d.1, k)) {
            return None;
        }
        Some([
            n[0].divide_sides(self.sides)?,
            n[1].divide_sides(self.sides)?,
            n[2].divide_sides(self.sides)?,
        ])
    }
}

impl<S: Scalar> HomPatch<S> {
    /// The normal numerator `N = W·(P_u × P_v) + W_u·(P_v × P) +
    /// W_v·(P × P_u)` from the net `(P, W) = (X, Y, Z, W)`, equal to
    /// `W³·(x_u × x_v)` (module docs). Of degrees `(3m − 1, 3n − 1)` for
    /// degrees `(m, n) ≥ (1, 1)`, `(5, 5)` for a biquadratic patch (whose
    /// exact numerator is of degree `(4, 4)`, not reduced here); `None`
    /// past [`MAX_DEGREE`](super::MAX_DEGREE). The patch reversed in
    /// either direction gives the reversed numerator negated, to the bit
    /// but the signs of zeros.
    pub fn normal_numerator(&self) -> Option<[Tensor<S>; 3]> {
        let [x, y, z, w] = self.tensors();
        let [xu, yu, zu, wu] = self.derivative(Dir::U).tensors().clone();
        let [xv, yv, zv, wv] = self.derivative(Dir::V).tensors().clone();
        let p = [x.clone(), y.clone(), z.clone()];
        numerator(&p, w, &[xu, yu, zu], &wu, &[xv, yv, zv], &wv)
    }

    /// The normal numerator at its true degrees `(3m − 2, 3n − 2)`, `(4,
    /// 4)` for a biquadratic patch: [`normal_numerator`] reduced in `u`,
    /// then in `v` ([`Tensor::reduced`]), each bound holding the true
    /// coefficient (module docs). `None` for a patch of degree 0 in either
    /// direction (its numerator is zero), past
    /// [`MAX_DEGREE`](super::MAX_DEGREE), and where the bounds certainly
    /// leave no elevation (never for a numerator of this patch's net).
    /// Reversal as for [`normal_numerator`].
    ///
    /// [`normal_numerator`]: Self::normal_numerator
    pub fn normal_numerator_reduced(&self) -> Option<[Tensor<S>; 3]> {
        let (m, n) = self.degrees();
        if m == 0 || n == 0 {
            return None;
        }
        let reduce = |f: &Tensor<S>| f.reduced(Dir::U)?.reduced(Dir::V);
        let [a, b, c] = self.normal_numerator()?;
        Some([reduce(&a)?, reduce(&b)?, reduce(&c)?])
    }

    /// The normal numerator's line on `side`, from the net's two lines
    /// there alone (the side's and the next, through the derivative across
    /// the side): the same bits as that line of
    /// [`normal_numerator`](Self::normal_numerator), as the products' and
    /// sums' boundary lines are their operands' 1D ones. `None` past
    /// [`MAX_DEGREE`](super::MAX_DEGREE).
    pub fn side_numerator(&self, side: Side) -> Option<[Poly<S>; 3]> {
        let lines = |h: &HomPatch<S>| h.tensors().clone().map(|t| t.side(side));
        let [x, y, z, w] = lines(self);
        let [xu, yu, zu, wu] = lines(&self.derivative(Dir::U));
        let [xv, yv, zv, wv] = lines(&self.derivative(Dir::V));
        numerator(&[x, y, z], &w, &[xu, yu, zu], &wu, &[xv, yv, zv], &wv)
    }

    /// The proof that the normal numerator is zero on every side in
    /// `sides`: each side's numerator line ([`side_numerator`]) exactly
    /// zero, every coefficient the integer 0 with no error. On the exact
    /// net (the rational level) this decides it; at a [`Bounded`] level
    /// rounding makes it fail nearly always, so a tile proves it on its
    /// exact net and divides its numerator at a level. `None` where a
    /// side isn't proven.
    ///
    /// [`side_numerator`]: Self::side_numerator
    /// [`Bounded`]: crate::num::bounded::Bounded
    pub fn singular_sides(&self, sides: Sides) -> Option<SingularSides> {
        for side in sides.iter() {
            let line = self.side_numerator(side)?;
            if !line.iter().all(|p| p.coefs().iter().all(|c| c.is_int(0))) {
                return None;
            }
        }
        Some(SingularSides {
            degrees: self.degrees(),
            sides,
        })
    }
}

#[cfg(test)]
mod tests;
