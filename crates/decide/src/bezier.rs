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
//! kinds of form with certified binomial weights, the vector products of
//! forms, and the tangent and normal numerators; in [`compose`], spans
//! and patches put into line and plane equations, and the shadow of a
//! span. In [`roots`], root isolation on `[0, 1]`: certified simple
//! roots, and clusters for the rest. Every operation commutes with
//! reversal to the bit (the tangent and normal numerators change sign
//! with it, exactly but for the signs of zeros; the roots of the reversed
//! form are the mirrored roots).

pub mod compose;
pub mod curve;
pub mod poly;
pub mod product;
pub mod roots;
pub mod scalar;
pub mod split;
pub mod tensor;

pub use curve::HomCurve;
pub use poly::{Hull, MAX_DEGREE, Poly};
pub use product::{Form, cross, dot};
pub use roots::{Cluster, Dyadic, Found, Limits, Root, Roots, isolate};
pub use scalar::Scalar;
pub use tensor::{Dir, HomPatch, HullBox, Partials, Tensor};

#[cfg(test)]
mod tests;
