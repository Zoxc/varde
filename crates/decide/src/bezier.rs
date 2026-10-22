//! Bernstein forms: polynomials on `[0, 1]` in the Bernstein basis
//! ([`Poly`]), with degrees given at runtime, over a [`Scalar`]: a
//! precision level's [`Bounded`](crate::num::bounded::Bounded) value,
//! whose every result carries a certified error bound, or an exact
//! `malachite` rational. Evaluation, subdivision at ½ (the same bits
//! under reversal) and at any `t`, degree elevation, the derivative and
//! the coefficient hull, a certified enclosure of the range. In
//! [`tensor`], the same on `[0, 1]²` for tensor-product forms
//! ([`Tensor`]) and rational patches in homogeneous coordinates
//! ([`HomPatch`]), with partial derivatives and a certified hull box.
//! In [`curve`], rational spans in homogeneous coordinates
//! ([`HomCurve`]). In [`product`], products, sums and elevation of both
//! kinds of form with certified binomial weights and the vector products
//! of forms; in [`normal`], a rational patch's normal numerator, reduced
//! or not, and its exact quotient by the factors of singular sides; in
//! [`divide`], the exact divisions under it (by the linear factor of an
//! end or a side, and degree reduction); in [`deriv`], hodographs of any
//! order, the tangent and partial numerators by the quotient rule in
//! homogeneous form, and the derivatives of a rational form's point; in
//! [`power`], the power basis and back; in [`nd`], forms in any number
//! of variables ([`NdTensor`]) with their elevation; in [`compose`], spans
//! and patches put into line and plane equations, the shadow of a span,
//! a patch along a curve in its domain (a p-curve span) with the seam
//! numerator, and spans and patches put into an implicit equation
//! ([`Implicit`]). In [`cone`], the certified normal cone of a vector form's
//! coefficients ([`NormalCone`]). In [`closest`], the closest point of a
//! span or patch to a point ([`Closest`]): a certified distance upper
//! bound, a Krawczyk certificate for the foot point, and curvature
//! bounds. In [`roots`], root isolation on
//! `[0, 1]`: certified simple roots, and clusters for the rest; in
//! [`roots_nd`], the same for a system of forms in `d` variables on
//! `[0, 1]ᵈ`, certified by Krawczyk's operator. Every operation commutes with
//! reversal to the bit (the tangent and normal numerators change sign
//! with it, exactly but for the signs of zeros; the roots of the reversed
//! form are the mirrored roots), and the 2D products and elevation with
//! transposition too.

pub mod closest;
pub mod compose;
pub mod cone;
pub mod curve;
pub mod deriv;
pub mod divide;
pub mod nd;
pub mod normal;
pub mod poly;
pub mod power;
pub mod product;
pub mod roots;
pub mod roots_nd;
pub mod scalar;
pub mod split;
pub mod tensor;

pub use closest::{Closest, Foot, FootBox, Reached};
pub use compose::Implicit;
pub use cone::NormalCone;
pub use curve::HomCurve;
pub use deriv::Derivatives;
pub use divide::{Side, Sides};
pub use nd::NdTensor;
pub use normal::SingularSides;
pub use poly::{Hull, MAX_DEGREE, Poly};
pub use product::{Form, cross, dot};
pub use roots::{
    Cluster, Dyadic, Exact, ExactRoots, Found, Limits, Narrowed, Root, Roots, isolate,
    isolate_exact, narrow,
};
pub use roots_nd::{NdBox, NdCluster, NdFail, NdFound, NdRoot, NdRoots, isolate_nd};
pub use scalar::Scalar;
pub use tensor::{Dir, HomPatch, HullBox, Partials, Tensor};

#[cfg(test)]
mod tests;
