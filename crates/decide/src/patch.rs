//! The patch trait the decision layer and the certificates are written
//! against ([`Patch`]): a rational patch on `[0, 1]²` of degrees given at
//! runtime, with its certified operations at a precision level.
//!
//! **One trait, a level per use.** `Patch<S>` is the patch at the level
//! of the scalar `S` (a [`Bounded`](crate::num::bounded::Bounded) level of
//! [`num::level`](crate::num::level), or exact rationals): every value it
//! returns carries a certified error bound at that level. A member that
//! has no scalar argument to take the level from (and width, for
//! big-float) takes a `like` value of it, as [`Scalar`]'s constructors
//! do. A [`HomPatch<S>`] is the patch at its own level and ignores
//! `like`; a type that stores an exact net (the kernel's tile) implements
//! `Patch<S>` for every level, forming its coefficients there.
//!
//! **The members** are the degree-generic layer's list: evaluation and
//! derivatives ([`eval`](Patch::eval), [`eval_partials`](Patch::eval_partials),
//! [`derivatives`](Patch::derivatives)); the split at ½
//! ([`split_half`](Patch::split_half), deterministic and the same bits
//! under reversal and transposition); the hull box and the normal cone
//! ([`hull_box`](Patch::hull_box), [`normal_cone_at`](Patch::normal_cone_at),
//! [`normal_cone`](Patch::normal_cone)); the fold certificate (the normal
//! numerator [`normal_numerator`](Patch::normal_numerator), the proof of
//! singular sides [`singular_sides`](Patch::singular_sides) and the
//! numerator divided by their factors
//! [`fold_numerator`](Patch::fold_numerator)); Bernstein composition with
//! an implicit ([`compose_implicit`](Patch::compose_implicit)) and with a
//! p-curve span ([`compose_span`](Patch::compose_span), with the seam
//! numerator [`seam_numerator`](Patch::seam_numerator)); and the closest
//! point ([`closest`](Patch::closest), the [`Closest`] set-up whose
//! Newton search, distance bound and foot certificate follow).
//!
//! **Defaults.** Every member but the degrees, the net at the level
//! ([`hom`](Patch::hom)), the split, the hull, the certified cone and the
//! closest point has a default: the [`HomPatch`] operation of the same
//! name on the net at the level. An implementor overrides a default where
//! it computes the same thing better (exact corners, a frame, a cache),
//! with the same meaning.

use std::borrow::Cow;
use std::convert::Infallible;

use crate::bezier::{
    Axis, Closest, ConeFail, Derivatives, Dir, HomCurve, HomPatch, HullBox, Implicit, NormalCone,
    Partials, Poly, Scalar, Sides, SingularSides, Tensor,
};

/// A rational patch on `[0, 1]²` at the level of `S`, of degrees given at
/// runtime, with its certified operations (module docs).
pub trait Patch<S: Scalar> {
    /// Why a split at ½ may fail (a stored net whose new values round
    /// past its range).
    type SplitError;

    /// The degrees `(m, n)`.
    fn degrees(&self) -> (usize, usize);

    /// The homogeneous net `(w·x, w·y, w·z, w)` at `like`'s level.
    fn hom(&self, like: &S) -> Cow<'_, HomPatch<S>>;

    /// The point at `(u, v)`, certified ([`HomPatch::eval`]).
    fn eval(&self, u: &S, v: &S) -> [S; 3] {
        self.hom(u).eval(u, v)
    }

    /// The point and its first partials at `(u, v)`, certified
    /// ([`HomPatch::eval_partials`]).
    fn eval_partials(&self, u: &S, v: &S) -> Partials<S> {
        self.hom(u).eval_partials(u, v)
    }

    /// The point's partial derivatives up to total order `order` at
    /// `(u, v)`, certified ([`HomPatch::derivatives_at`]).
    fn derivatives(&self, u: &S, v: &S, order: usize) -> Derivatives<S> {
        self.hom(u).derivatives_at(u, v, order)
    }

    /// The halves on `[0, ½]` and `[½, 1]` in `dir`, deterministic and the
    /// same bits under reversal and transposition.
    fn split_half(&self, dir: Dir) -> Result<[Self; 2], Self::SplitError>
    where
        Self: Sized;

    /// A certified box holding every point of the patch; `None` where
    /// none is certified (a weight not certified positive).
    fn hull_box(&self) -> Option<HullBox>;

    /// The normal numerator `W·(P_u × P_v) + W_u·(P_v × P) + W_v·(P × P_u)`
    /// at `like`'s level, degrees `(3m − 1, 3n − 1)` unreduced
    /// ([`HomPatch::normal_numerator`]; an implementor may compute it in a
    /// translated frame, the numerator being translation invariant).
    /// `None` past [`MAX_DEGREE`](crate::bezier::MAX_DEGREE).
    fn normal_numerator(&self, like: &S) -> Option<[Tensor<S>; 3]> {
        self.hom(like).normal_numerator()
    }

    /// The normal cone certified at `like`'s level, else why not: the
    /// cone of the numerator's coefficients ([`NormalCone::of_forms`]
    /// about [`Axis::Smallest`]; an implementor may also split in space).
    /// A numerator past the degree limit is [`ConeFail::Space`] (no level
    /// helps).
    fn normal_cone_at(&self, like: &S) -> Result<NormalCone, ConeFail> {
        let n = self.normal_numerator(like).ok_or(ConeFail::Space)?;
        NormalCone::of_forms([&n], Axis::Smallest)
    }

    /// The certified normal cone: [`normal_cone_at`](Self::normal_cone_at)
    /// at this level, or at the levels above where the implementor
    /// escalates; `None` where none certifies one.
    fn normal_cone(&self) -> Option<NormalCone>;

    /// The proof that the normal numerator is zero on every side in
    /// `sides` ([`HomPatch::singular_sides`] at `like`'s level, which
    /// needs the zeros exact; an implementor with an exact net proves it
    /// there). `None` where it isn't proven.
    fn singular_sides(&self, sides: Sides, like: &S) -> Option<SingularSides> {
        self.hom(like).singular_sides(sides)
    }

    /// The numerator the fold check reads: the normal numerator at
    /// `like`'s level divided by the linear factors of `sides`, once
    /// they are proven singular ([`singular_sides`](Self::singular_sides),
    /// [`SingularSides::quotient`]); the numerator itself for no sides.
    fn fold_numerator(&self, sides: Sides, like: &S) -> Option<[Tensor<S>; 3]> {
        let proof = self.singular_sides(sides, like)?;
        proof.quotient(&self.normal_numerator(like)?)
    }

    /// The patch in the implicit equation `f`, `W^d·F(x)` over the patch
    /// ([`HomPatch::compose_implicit`]), at `f`'s level. `None` past the
    /// degree limit.
    fn compose_implicit(&self, f: &Implicit<S>) -> Option<Tensor<S>> {
        self.hom(&f.origin()[0]).compose_implicit(f)
    }

    /// The patch along the p-curve span `p` in its local coordinates
    /// ([`HomPatch::compose_span`]), at `p`'s level. `None` past the
    /// degree limit.
    fn compose_span(&self, p: &HomCurve<S, 2>) -> Option<HomCurve<S, 3>> {
        self.hom(&p.weights().coefs()[0]).compose_span(p)
    }

    /// The seam numerator of the patch along the p-curve span `p` against
    /// the space curve `c` with the same parameter
    /// ([`compose_span`](Self::compose_span), then
    /// [`HomCurve::seam_numerator`]). `None` past the degree limit.
    fn seam_numerator(&self, p: &HomCurve<S, 2>, c: &HomCurve<S, 3>) -> Option<[Poly<S>; 3]> {
        self.compose_span(p)?.seam_numerator(c)
    }

    /// The closest point of the patch to `x` set up at `like`'s level
    /// ([`Closest`]: the distance bound, Newton and the foot certificate
    /// on it). `None` for a point or net that isn't finite, a weight not
    /// certified positive, or a gradient past the degree limit.
    fn closest(&self, x: [f64; 3], like: &S) -> Option<Closest<S>>;
}

/// A rational patch at its own level, by its inherent operations; `like`
/// is not read.
impl<S: Scalar> Patch<S> for HomPatch<S> {
    type SplitError = Infallible;

    fn degrees(&self) -> (usize, usize) {
        HomPatch::degrees(self)
    }

    fn hom(&self, _: &S) -> Cow<'_, HomPatch<S>> {
        Cow::Borrowed(self)
    }

    fn split_half(&self, dir: Dir) -> Result<[Self; 2], Infallible> {
        Ok(HomPatch::split_half(self, dir))
    }

    fn hull_box(&self) -> Option<HullBox> {
        HomPatch::hull_box(self)
    }

    /// The cone at this level ([`Patch::normal_cone_at`]): no escalation.
    fn normal_cone(&self) -> Option<NormalCone> {
        let like = &self.weights().coefs()[0];
        Patch::normal_cone_at(self, like).ok()
    }

    /// The patch's net held in the frame centred at `x`, `(X − x·W, W)`
    /// at this level, then [`Closest::from_offset`].
    fn closest(&self, x: [f64; 3], _: &S) -> Option<Closest<S>> {
        if !x.iter().all(|c| c.is_finite()) || !self.weights_positive() {
            return None;
        }
        let [hx, hy, hz, w] = self.tensors().each_ref().map(Tensor::coefs);
        let (m, n) = HomPatch::degrees(self);
        let at = |h: &S, w: &S, k: usize| h.sub(&S::from_f64(x[k], w).mul(w));
        let net = (0..w.len()).map(|i| {
            [
                at(&hx[i], &w[i], 0),
                at(&hy[i], &w[i], 1),
                at(&hz[i], &w[i], 2),
                w[i].clone(),
            ]
        });
        Closest::from_offset(HomPatch::new(m, n, net)?, x)
    }
}

#[cfg(test)]
mod tests;
