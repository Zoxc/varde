//! Correct rounding of exact values to `f64`: rationals, and rationals
//! times a square root, to the nearest `f64`, ties to even. Forms'
//! keys and their lattices' control points are such values of stored
//! `f64` inputs, so rounding them this way gives every construction of
//! one exact value the same bits.

use std::cmp::Ordering;

use malachite::base::num::arithmetic::traits::Sign;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use malachite::{Float, Rational};

/// The finite `x` as an exact rational.
pub(crate) use varde_decide::num::exact::rat as q;

/// `x` rounded to the nearest `f64`, ties to even (`±∞` past the range).
pub(crate) fn nearest(x: &Rational) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// `a·√b` rounded to the nearest `f64`, ties to even, for `b ≥ 0`.
pub(crate) fn nearest_root(a: &Rational, b: &Rational) -> f64 {
    let s = a * a * b;
    let m = nearest_sqrt(&s);
    if a.sign() == Ordering::Less { -m } else { m }
}

/// `√s` rounded to the nearest `f64`, ties to even, for `s ≥ 0`
/// (`+∞` past the range).
///
/// A 53-bit big-float root, rounded to `f64`, is a candidate at most an
/// ulp away (two roundings, where the result is subnormal); the
/// candidate's neighbours' midpoints, squared exactly, then settle it: a
/// root on a midpoint (only possible for a rational root) goes to the
/// even neighbour.
pub(crate) fn nearest_sqrt(s: &Rational) -> f64 {
    debug_assert!(s.sign() != Ordering::Less, "a root of a negative value");
    if s.sign() == Ordering::Equal {
        return 0.0;
    }
    let (f, _) = Float::sqrt_rational_prec_round_ref(s, 53, Nearest);
    let mut c = f64::rounding_from(&f, Nearest).0;
    loop {
        if !c.is_finite() {
            return c;
        }
        let up = c.next_up();
        if up.is_finite() {
            let m = (q(c) + q(up)) / Rational::from(2u32);
            match s.cmp(&(&m * &m)) {
                Ordering::Greater => {
                    c = up;
                    continue;
                }
                Ordering::Equal => return even(c, up),
                Ordering::Less => {}
            }
        }
        if c > 0.0 {
            let down = c.next_down();
            let m = (q(c) + q(down)) / Rational::from(2u32);
            match s.cmp(&(&m * &m)) {
                Ordering::Less => {
                    c = down;
                    continue;
                }
                Ordering::Equal => return even(down, c),
                Ordering::Greater => {}
            }
        }
        return c;
    }
}

/// Of two neighbouring `f64`s, the one with an even significand.
fn even(a: f64, b: f64) -> f64 {
    if a.to_bits() & 1 == 0 { a } else { b }
}

#[cfg(test)]
mod tests {
    use malachite::base::num::arithmetic::traits::Pow;
    use malachite::base::num::basic::traits::One;
    use varde_testkit::rng::Rng;

    use super::*;

    /// `√s` from a 300-bit root: an independent reference (a double
    /// rounding there would need a root within `2⁻²⁴⁷` ulp of a
    /// midpoint, which the random cases don't hit).
    fn reference(s: &Rational) -> f64 {
        let (f, _) = Float::sqrt_rational_prec_round_ref(s, 300, Nearest);
        f64::rounding_from(&f, Nearest).0
    }

    #[test]
    fn roots_match_a_wide_reference() {
        let mut rng = Rng::new(7);
        for _ in 0..varde_testing::pick(2_000, 50_000) {
            let a = q(rng.log_range(1e-300, 1e300));
            let b = q(rng.log_range(1e-30, 1e30));
            let s = &a / &b;
            assert_eq!(nearest_sqrt(&s).to_bits(), reference(&s).to_bits(), "{s}");
        }
    }

    #[test]
    fn exact_roots_and_ties() {
        // Exact squares round to their roots, subnormal ones included.
        for x in [1.0, 0.6, 3.0e-310, f64::MIN_POSITIVE, 1.5e300] {
            assert_eq!(nearest_sqrt(&(q(x) * q(x))), x);
        }
        // A root on the midpoint of 1 and the next f64 goes to 1 (even);
        // one on the midpoint above that goes up to the even neighbour.
        let half_ulp = q(f64::EPSILON) / Rational::from(2u32);
        let m = Rational::ONE + &half_ulp;
        assert_eq!(nearest_sqrt(&(&m * &m)), 1.0);
        let up = 1.0f64.next_up();
        let m = q(up) + &half_ulp;
        assert_eq!(nearest_sqrt(&(&m * &m)), up.next_up());
        assert_eq!(nearest_sqrt(&Rational::from(0u32)), 0.0);
    }

    #[test]
    fn scaled_roots_keep_the_sign() {
        let half = Rational::from_signeds(1, 2);
        let r = nearest_root(&Rational::ONE, &half);
        assert_eq!(r, std::f64::consts::FRAC_1_SQRT_2);
        assert_eq!(nearest_root(&-Rational::ONE, &half), -r);
        assert_eq!(
            nearest_root(&Rational::from(3u32), &Rational::from(0u32)),
            0.0
        );
    }

    #[test]
    fn subnormal_ties_and_the_range_edges() {
        let tiny = q(5e-324);
        let two = Rational::from(2u32);
        for k in 0u32..6 {
            // `(2k + 1)/2` units of the least subnormal: a tie, to even.
            let m = &tiny * Rational::from(2 * k + 1) / &two;
            let lo = 5e-324 * f64::from(k);
            let want = if k % 2 == 0 { lo } else { lo + 5e-324 };
            assert_eq!(nearest(&m).to_bits(), want.to_bits(), "{k}");
            assert_eq!(nearest(&-m.clone()).to_bits(), (-want).to_bits(), "{k}");
            assert_eq!(nearest_sqrt(&(&m * &m)).to_bits(), want.to_bits(), "{k}");
            // Roots just off the tie.
            let e = &tiny / Rational::from(1u64 << 40);
            let (up, down) = (&m + &e, &m - &e);
            assert_eq!(nearest_sqrt(&(&up * &up)), lo + 5e-324, "{k}");
            assert_eq!(nearest_sqrt(&(&down * &down)), lo, "{k}");
        }
        // The overflow edge: `MAX` plus half an ulp is a tie, to `∞`.
        let max = q(f64::MAX);
        let mid = &max + (&max - q(f64::MAX.next_down())) / &two;
        let below = &mid - Rational::ONE;
        assert_eq!(nearest(&mid), f64::INFINITY);
        assert_eq!(nearest(&below), f64::MAX);
        assert_eq!(nearest_sqrt(&(&mid * &mid)), f64::INFINITY);
        assert_eq!(nearest_sqrt(&(&below * &below)), f64::MAX);
        // Radicands past both ends of the range.
        let p = Rational::from(2u32).pow(3000u64);
        assert_eq!(nearest_sqrt(&p), f64::INFINITY);
        assert_eq!(nearest_sqrt(&(Rational::ONE / &p)), 0.0);
        let s = Rational::ONE / Rational::from(2u32).pow(2148u64);
        assert_eq!(nearest_sqrt(&s), 5e-324);
        assert_eq!(nearest_sqrt(&(&s / Rational::from(4u32))), 0.0);
        // Subnormal roots against the wide reference.
        let mut rng = Rng::new(9);
        for _ in 0..varde_testing::pick(300, 5_000) {
            let a = q(rng.log_range(1e-320, 1e-300));
            let b = q(rng.log_range(1e-30, 1e30));
            let s = &a * &a / &b;
            assert_eq!(nearest_sqrt(&s).to_bits(), reference(&s).to_bits(), "{s}");
        }
    }
}
