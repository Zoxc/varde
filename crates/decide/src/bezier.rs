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

pub mod poly;
pub mod scalar;
pub mod split;
pub mod tensor;

pub use poly::{Hull, MAX_DEGREE, Poly};
pub use scalar::Scalar;
pub use tensor::{Dir, HomPatch, HullBox, Partials, Tensor};

#[cfg(test)]
mod tests;
