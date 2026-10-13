//! Bernstein forms: polynomials on `[0, 1]` in the Bernstein basis
//! ([`Poly`]), with degrees given at runtime, over a [`Scalar`]: a
//! precision level's [`Bounded`](crate::num::bounded::Bounded) value,
//! whose every result carries a certified error bound, or an exact
//! `malachite` rational. Evaluation, subdivision at ½ (the same bits
//! under reversal) and at any `t`, degree elevation and the coefficient
//! hull, a certified enclosure of the range.

pub mod poly;
pub mod scalar;
pub mod split;

pub use poly::{Hull, MAX_DEGREE, Poly};
pub use scalar::Scalar;

#[cfg(test)]
mod tests;
