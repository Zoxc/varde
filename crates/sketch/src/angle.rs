//! Angles, logarithms and the like with the same bits on every platform.
//!
//! std's `sin`, `cos`, `atan2`, `ln`, ... (and glam's `DVec2::from_angle`
//! and `to_angle`, built on them) come from the platform's maths library,
//! whose last bits differ between platforms; the sketch's arcs are placed
//! by them, and so the vertices of its profiles and the solids built from
//! them. These wrap the `libm` crate instead: pure Rust, pinned by
//! `Cargo.lock`, the same code natively and on wasm (where std's are
//! already this code). The kernel's `varde_kernel::trig` wraps the same
//! functions, so the two agree to the bit.
//!
//! The crate's `clippy.toml` refuses std's versions, so the sketch keeps
//! to these; tests that use std as an independent reference allow it in
//! their module. [`from_angle`] is `cos` and `sin` taken apart, not a
//! joint `sincos`, as std's `sin_cos` is on wasm.

use glam::DVec2;

/// The sine of `x` radians.
#[inline]
pub(crate) fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// The cosine of `x` radians.
#[inline]
pub(crate) fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// The tangent of `x` radians.
#[inline]
pub(crate) fn tan(x: f64) -> f64 {
    libm::tan(x)
}

/// The angle in `[−π, π]` of the direction `(x, y)`, from +x towards +y.
#[inline]
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// The angle in `[0, π]` whose cosine is `x`; not a number outside
/// `[−1, 1]`.
#[inline]
pub(crate) fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// `e` to the power `x`.
#[inline]
pub(crate) fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// The natural logarithm of `x`.
#[inline]
pub(crate) fn ln(x: f64) -> f64 {
    libm::log(x)
}

/// `√(x² + y²)`, without overflow or underflow on the way.
#[inline]
pub(crate) fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

/// The unit vector at `angle` radians from +x towards +y, `(cos, sin)`:
/// glam's `DVec2::from_angle` with these bits.
#[inline]
pub(crate) fn from_angle(angle: f64) -> DVec2 {
    DVec2::new(cos(angle), sin(angle))
}

/// The angle in `[0, π]` between `a` and `b`, by `acos` of their cosine:
/// glam's `DVec2::angle_to` without its sign, with these bits.
#[inline]
pub(crate) fn between(a: DVec2, b: DVec2) -> f64 {
    let cos = a.dot(b) / (a.length_squared() * b.length_squared()).sqrt();
    acos(cos.clamp(-1.0, 1.0))
}

/// The angle in `[−π, π]` of `v` from +x towards +y: glam's
/// `DVec2::to_angle` with these bits.
#[inline]
pub(crate) fn to_angle(v: DVec2) -> f64 {
    atan2(v.y, v.x)
}
