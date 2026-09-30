//! Deterministic trigonometry: the only angle functions the kernel's
//! results may depend on.
//!
//! `+ − × ÷ √` are correctly rounded, so they give the same bits on every
//! platform; `sin`, `cos`, `atan2` and the rest are not, and std's come
//! from the platform's maths library (glibc natively on Linux, its own on
//! macOS and Windows, a port of musl's on wasm), whose last bits differ.
//! These wrap the `libm` crate instead: pure Rust, pinned by `Cargo.lock`,
//! and the same code natively and on wasm, where std's are already this
//! code. So an angle gives the same bits everywhere, and the web's bits
//! natively.
//!
//! Every angle the kernel turns into a place or a decision goes through
//! here. The crate's `clippy.toml` refuses std's trigonometry, logarithms
//! and powers (and glam's methods built on them, such as
//! `DVec2::from_angle`), so it stays that way; tests that use std as an
//! independent reference allow it in their module.
//!
//! [`sin_cos`] is two calls, `sin` then `cos`, not a joint `sincos`, whose
//! bits may differ from them.

use glam::DVec2;

/// The sine of `x` radians.
#[inline]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// The cosine of `x` radians.
#[inline]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// The sine and the cosine of `x` radians, as [`sin`] and [`cos`] give
/// them.
#[inline]
pub fn sin_cos(x: f64) -> (f64, f64) {
    (sin(x), cos(x))
}

/// The tangent of `x` radians.
#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

/// The angle in `[−π/2, π/2]` whose sine is `x`; not a number outside
/// `[−1, 1]`.
#[inline]
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

/// The angle in `[0, π]` whose cosine is `x`; not a number outside
/// `[−1, 1]`. Loses half the digits near 0 and π: an angle between two
/// directions is better [`atan2`] of their cross and dot products.
#[inline]
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// The angle in `[−π, π]` of the direction `(x, y)`, from +x towards +y.
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// The unit vector at `angle` radians from +x towards +y: `(cos, sin)`.
#[inline]
pub fn unit(angle: f64) -> DVec2 {
    DVec2::new(cos(angle), sin(angle))
}

/// The angle in `[−π, π]` of `v` from +x towards +y: [`atan2`] of its
/// coordinates.
#[inline]
pub fn angle(v: DVec2) -> f64 {
    atan2(v.y, v.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_cos_is_sin_and_cos() {
        for x in [0.0, 0.3, -2.5, 1e3, 7.0e-9] {
            let (s, c) = sin_cos(x);
            assert_eq!(
                (s.to_bits(), c.to_bits()),
                (sin(x).to_bits(), cos(x).to_bits())
            );
            assert_eq!(unit(x), DVec2::new(c, s));
        }
        assert_eq!(angle(DVec2::new(-1.0, 0.0)), std::f64::consts::PI);
        assert_eq!(angle(DVec2::new(0.0, 2.0)), std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn known_values() {
        // Some bits `libm` gives, so a change of its code shows here.
        assert_eq!(sin(0.5).to_bits(), 0x3fde_aee8_744b_05f0);
        assert_eq!(cos(0.5).to_bits(), 0x3fec_1528_065b_7d50);
        assert_eq!(atan2(1.0, 2.0).to_bits(), 0x3fdd_ac67_0561_bb4f);
    }
}
