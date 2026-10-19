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

/// Golden bits: what each function gives for fixed inputs, edge cases
/// among them, to the bit. Determinism is the point, so these are
/// `libm`'s bits, and a change to its code (a version bump) shows here.
/// They were checked against the correctly rounded values, computed with
/// mpmath at 400 and 3000 bits: all agree but the three marked, where
/// `libm` is one unit in the last place off.
#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_4, PI, TAU};

    /// The least positive `f64`, a subnormal.
    const MIN_SUB: f64 = 5e-324;

    /// 3π/2 rounded to `f64`, where the cosine is near zero and the
    /// tangent large.
    const THREE_HALVES_PI: f64 = 4.71238898038469;

    /// A function's output bits for each input.
    fn check(name: &str, f: fn(f64) -> f64, golden: &[(f64, u64)]) {
        for &(x, bits) in golden {
            let got = f(x).to_bits();
            assert_eq!(got, bits, "{name}({x:e}): {got:#018x}, not {bits:#018x}");
        }
    }

    const SIN: &[(f64, u64)] = &[
        (0.0, 0x0000_0000_0000_0000),
        (-0.0, 0x8000_0000_0000_0000),
        (MIN_SUB, 0x0000_0000_0000_0001),
        (1e-300, 0x01a5_6e1f_c2f8_f359),
        (7e-9, 0x3e3e_1094_d643_f784),
        (0.3, 0x3fd2_e9cd_95ba_ba33),
        (0.5, 0x3fde_aee8_744b_05f0),
        (1.0, 0x3fea_ed54_8f09_0cee),
        // Correctly rounded: 0xbfe3_26af_0dcf_cab1.
        (-2.5, 0xbfe3_26af_0dcf_cab0),
        (FRAC_PI_4, 0x3fe6_a09e_667f_3bcc),
        (FRAC_PI_2, 0x3ff0_0000_0000_0000),
        (PI, 0x3ca1_a626_3314_5c07),
        (THREE_HALVES_PI, 0xbff0_0000_0000_0000),
        (TAU, 0xbcb1_a626_3314_5c07),
        (1000.0, 0x3fea_75cc_150a_206b),
        (1e22, 0xbfeb_453a_b76b_f397),
        (1e300, 0xbfea_2c16_b010_e385),
        (f64::MAX, 0x3f74_52fc_98b3_4e97),
    ];

    const COS: &[(f64, u64)] = &[
        (0.0, 0x3ff0_0000_0000_0000),
        (-0.0, 0x3ff0_0000_0000_0000),
        (MIN_SUB, 0x3ff0_0000_0000_0000),
        (1e-300, 0x3ff0_0000_0000_0000),
        (7e-9, 0x3ff0_0000_0000_0000),
        (0.3, 0x3fee_921d_d42f_09ba),
        (0.5, 0x3fec_1528_065b_7d50),
        (1.0, 0x3fe1_4a28_0fb5_068c),
        (-2.5, 0xbfe9_a2f7_ef85_8b7d),
        (FRAC_PI_4, 0x3fe6_a09e_667f_3bcd),
        (FRAC_PI_2, 0x3c91_a626_3314_5c07),
        (PI, 0xbff0_0000_0000_0000),
        (THREE_HALVES_PI, 0xbcaa_7939_4c9e_8a0a),
        (TAU, 0x3ff0_0000_0000_0000),
        // Correctly rounded: 0x3fe1_ff02_6793_f1bb.
        (1000.0, 0x3fe1_ff02_6793_f1bc),
        (1e22, 0x3fe0_be2c_ef01_c8f4),
        (1e300, 0xbfe2_6990_22ad_c4c1),
        (f64::MAX, 0xbfef_ffe6_2ecf_ab75),
    ];

    const TAN: &[(f64, u64)] = &[
        (0.0, 0x0000_0000_0000_0000),
        (-0.0, 0x8000_0000_0000_0000),
        (MIN_SUB, 0x0000_0000_0000_0001),
        (1e-300, 0x01a5_6e1f_c2f8_f359),
        (7e-9, 0x3e3e_1094_d643_f784),
        (0.3, 0x3fd3_cc2a_44e2_9998),
        (0.5, 0x3fe1_7b4f_5bf3_474a),
        (1.0, 0x3ff8_eb24_5cbe_e3a6),
        (-2.5, 0x3fe7_e79b_4e00_bb15),
        (FRAC_PI_4, 0x3fef_ffff_ffff_ffff),
        (FRAC_PI_2, 0x434d_0296_7c31_cdb5),
        (PI, 0xbca1_a626_3314_5c07),
        (THREE_HALVES_PI, 0x4333_570e_fd76_8923),
        (TAU, 0xbcb1_a626_3314_5c07),
        (1000.0, 0x3ff7_8672_9f34_311a),
        // Correctly rounded: 0xbffa_0f79_c1b6_b257.
        (1e22, 0xbffa_0f79_c1b6_b258),
        (1e300, 0x3ff6_be41_1f37_ac77),
        (f64::MAX, 0xbf74_530c_fe72_9484),
    ];

    const ASIN: &[(f64, u64)] = &[
        (0.0, 0x0000_0000_0000_0000),
        (-0.0, 0x8000_0000_0000_0000),
        (MIN_SUB, 0x0000_0000_0000_0001),
        (1e-300, 0x01a5_6e1f_c2f8_f359),
        (1e-8, 0x3e45_798e_e230_8c3a),
        (0.3, 0x3fd3_8015_9e14_f6ff),
        (0.5, 0x3fe0_c152_382d_7366),
        (-0.5, 0xbfe0_c152_382d_7366),
        (FRAC_1_SQRT_2, 0x3fe9_21fb_5444_2d19),
        (0.9999999999999999, 0x3ff9_21fb_5044_2d18),
        (-0.9999999999999999, 0xbff9_21fb_5044_2d18),
        (1.0, 0x3ff9_21fb_5444_2d18),
        (-1.0, 0xbff9_21fb_5444_2d18),
    ];

    const ACOS: &[(f64, u64)] = &[
        (0.0, 0x3ff9_21fb_5444_2d18),
        (-0.0, 0x3ff9_21fb_5444_2d18),
        (MIN_SUB, 0x3ff9_21fb_5444_2d18),
        (1e-300, 0x3ff9_21fb_5444_2d18),
        (1e-8, 0x3ff9_21fb_5194_fb3c),
        (0.3, 0x3ff4_41f5_ecbe_ef59),
        (0.5, 0x3ff0_c152_382d_7366),
        (-0.5, 0x4000_c152_382d_7366),
        (FRAC_1_SQRT_2, 0x3fe9_21fb_5444_2d18),
        (0.9999999999999999, 0x3e50_0000_0000_0000),
        (-0.9999999999999999, 0x4009_21fb_5244_2d18),
        (1.0, 0x0000_0000_0000_0000),
        (-1.0, 0x4009_21fb_5444_2d18),
    ];

    /// `(y, x, bits of atan2(y, x))`. The signed zeros and infinities
    /// are exact by IEEE 754's definition of `atan2`.
    const ATAN2: &[(f64, f64, u64)] = &[
        (1.0, 2.0, 0x3fdd_ac67_0561_bb4f),
        (1.0, -1.0, 0x4002_d97c_7f33_21d2),
        (-1.0, -1.0, 0xc002_d97c_7f33_21d2),
        (3.0, -4.0, 0x4003_fc17_6b7a_8560),
        (-4.0, 3.0, 0xbfed_ac67_0561_bb4f),
        (0.5, 0.8660254037844386, 0x3fe0_c152_382d_7366),
        (1.0, 0.0, 0x3ff9_21fb_5444_2d18),
        (-1.0, 0.0, 0xbff9_21fb_5444_2d18),
        (0.0, -1.0, 0x4009_21fb_5444_2d18),
        (-0.0, -1.0, 0xc009_21fb_5444_2d18),
        (MIN_SUB, 1.0, 0x0000_0000_0000_0001),
        (1e-300, 1e300, 0x0000_0000_0000_0000),
        (1e300, 1e-300, 0x3ff9_21fb_5444_2d18),
        (0.0, 0.0, 0x0000_0000_0000_0000),
        (-0.0, 0.0, 0x8000_0000_0000_0000),
        (0.0, -0.0, 0x4009_21fb_5444_2d18),
        (-0.0, -0.0, 0xc009_21fb_5444_2d18),
        (f64::INFINITY, f64::INFINITY, 0x3fe9_21fb_5444_2d18),
        (f64::NEG_INFINITY, f64::NEG_INFINITY, 0xc002_d97c_7f33_21d2),
        (1.0, f64::INFINITY, 0x0000_0000_0000_0000),
        (-1.0, f64::NEG_INFINITY, 0xc009_21fb_5444_2d18),
    ];

    #[test]
    fn sin_cos_tan_are_the_same_bits_everywhere() {
        check("sin", sin, SIN);
        check("cos", cos, COS);
        check("tan", tan, TAN);
    }

    #[test]
    fn asin_acos_are_the_same_bits_everywhere() {
        check("asin", asin, ASIN);
        check("acos", acos, ACOS);
    }

    #[test]
    fn atan2_is_the_same_bits_everywhere() {
        for &(y, x, bits) in ATAN2 {
            let got = atan2(y, x).to_bits();
            assert_eq!(
                got, bits,
                "atan2({y:e}, {x:e}): {got:#018x}, not {bits:#018x}"
            );
        }
    }

    /// [`sin_cos`] and [`unit`] give the bits of [`sin`] and [`cos`],
    /// and [`angle`] those of [`atan2`].
    #[test]
    fn sin_cos_unit_angle_are_the_same_bits_everywhere() {
        for (&(x, sin_bits), &(x_cos, cos_bits)) in SIN.iter().zip(COS) {
            assert_eq!(x.to_bits(), x_cos.to_bits());
            let (s, c) = sin_cos(x);
            assert_eq!(
                (s.to_bits(), c.to_bits()),
                (sin_bits, cos_bits),
                "sin_cos({x:e})"
            );
            let u = unit(x);
            assert_eq!(
                (u.x.to_bits(), u.y.to_bits()),
                (cos_bits, sin_bits),
                "unit({x:e})"
            );
        }
        for &(y, x, bits) in ATAN2 {
            assert_eq!(
                angle(DVec2::new(x, y)).to_bits(),
                bits,
                "angle(({x:e}, {y:e}))"
            );
        }
    }

    /// Outside their domains, the functions give NaN.
    #[test]
    fn outside_the_domain_is_nan() {
        for x in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(sin(x).is_nan() && cos(x).is_nan() && tan(x).is_nan(), "{x}");
            assert!(asin(x).is_nan() && acos(x).is_nan(), "{x}");
            assert!(atan2(x, 1.0).is_nan() == x.is_nan(), "{x}");
        }
        for x in [1.0000000000000002, -1.0000000000000002, 2.0] {
            assert!(asin(x).is_nan() && acos(x).is_nan(), "{x}");
        }
        assert!(atan2(f64::NAN, 1.0).is_nan() && atan2(1.0, f64::NAN).is_nan());
    }
}
