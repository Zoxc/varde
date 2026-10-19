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

    /// The inputs the property tests draw are the same bits natively and
    /// on wasm (tools/ci.sh runs this test on both): every draw of one
    /// seed, pinned.
    #[test]
    fn draws_are_the_same_bits_everywhere() {
        let mut rng = Rng::new(0x5eed);
        let mut got = vec![rng.next_u64(), rng.next_u64()];
        got.push(rng.unit().to_bits());
        got.push(rng.range(-3.0, 5.0).to_bits());
        got.push(rng.range(-f64::MAX, f64::MAX).to_bits());
        got.push(rng.log_range(1e-3, 1e3).to_bits());
        for v in [rng.point(10.0), rng.direction(), rng.bary()] {
            got.extend([v.x, v.y, v.z].map(f64::to_bits));
        }
        let want: [u64; 15] = [
            0x09f1_fd9d_03f0_a9b4,
            0x5532_7416_1bbf_8475,
            0x3fd7_56f2_91a5_acd0,
            0x3fe0_d29b_6c7d_2250,
            0xffed_0352_3a41_9bab,
            0x3fc5_3dbc_a962_b6de,
            0xc01b_0dfe_b423_56ca,
            0xc021_1a7d_f83f_19d3,
            0x401c_52c7_e9c4_5fa0,
            0xbfe9_d4ba_ec78_462c,
            0xbfdf_3016_4dc4_0343,
            0xbfd5_50d4_ce5b_989b,
            0x3fc1_57b5_7554_4744,
            0x3fc6_95d7_3232_055c,
            0x3fe6_049c_d61e_6cd8,
        ];
        assert_eq!(got, want, "{got:#018x?}");
    }
}
