//! A small seeded generator for the kernels' property tests, so every
//! run, natively and on the web, sees the same inputs.

use glam::DVec3;

/// SplitMix64: fast, and good enough for picking test inputs.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`, `lo` itself if they are equal. Finite for
    /// any finite ends, even when `hi - lo` overflows.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = self.unit();
        let span = hi - lo;
        let x = if span.is_finite() {
            lo + span * u
        } else {
            // Halved, the span fits; doubling back can't overflow as the
            // halved point lies between the halved ends.
            (lo * 0.5 + (hi * 0.5 - lo * 0.5) * u) * 2.0
        };
        within(x, lo, hi)
    }

    /// Uniform in `[lo, hi)` on a log scale: `lo` and `hi` positive. The
    /// logarithms come from `libm`, so the inputs have the same bits on
    /// every platform.
    pub fn log_range(&mut self, lo: f64, hi: f64) -> f64 {
        let (log_lo, log_hi) = (libm::log(lo), libm::log(hi));
        within(libm::exp(log_lo + (log_hi - log_lo) * self.unit()), lo, hi)
    }

    /// Each coordinate uniform in `[-extent, extent)`.
    pub fn point(&mut self, extent: f64) -> DVec3 {
        DVec3::new(
            self.range(-extent, extent),
            self.range(-extent, extent),
            self.range(-extent, extent),
        )
    }

    /// A uniformly random unit vector.
    pub fn direction(&mut self) -> DVec3 {
        loop {
            let p = self.point(1.0);
            let length = p.length();
            if length > 0.1 && length <= 1.0 {
                return p / length;
            }
        }
    }

    /// A uniformly random barycentric point of the closed triangle.
    pub fn bary(&mut self) -> DVec3 {
        let (mut a, mut b) = (self.unit(), self.unit());
        if a + b > 1.0 {
            (a, b) = (1.0 - a, 1.0 - b);
        }
        DVec3::new(a, b, 1.0 - a - b)
    }
}

/// `x` moved into `[lo, hi)` (onto `lo` if they are equal): rounding
/// can carry a draw onto `hi`, or past either end on a log scale, when
/// the range is a few ulps wide.
fn within(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo || hi <= lo {
        lo
    } else if x >= hi {
        hi.next_down()
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_a_few_ulps_wide_stay_in_bounds() {
        // Near ties are a few ulps apart: `lo + (hi - lo) * u` rounds to
        // `hi` for a quarter of the draws in `[1, 1 + 4ε)`, and the log
        // scale's `exp(log(..))` lands on either side of its ends.
        let mut rng = Rng::new(7);
        for (lo, hi) in [
            (1.0, 1.0 + 4.0 * f64::EPSILON),
            (3.0, 3.000_000_000_000_01),
            (-1.0e-300, -0.999_999_999_999_9e-300),
            (1.0e-300, 1.000_000_000_000_1e-300),
        ] {
            for _ in 0..10_000 {
                let x = rng.range(lo, hi);
                assert!(lo <= x && x < hi, "range({lo:e}, {hi:e}) gave {x:e}");
                if lo > 0.0 {
                    let x = rng.log_range(lo, hi);
                    assert!(lo <= x && x < hi, "log_range({lo:e}, {hi:e}) gave {x:e}");
                }
            }
        }
    }

    #[test]
    fn the_widest_range_is_finite() {
        // `hi - lo` overflows to infinity past `f64::MAX`.
        let mut rng = Rng::new(1);
        for _ in 0..1000 {
            let x = rng.range(-f64::MAX, f64::MAX);
            assert!(x.is_finite(), "{x}");
            assert!(rng.point(f64::MAX).is_finite());
        }
        assert_eq!(Rng::new(2).range(5.0, 5.0), 5.0);
    }
}
