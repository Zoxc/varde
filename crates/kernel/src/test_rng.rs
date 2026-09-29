//! A small seeded generator for the kernel's property tests, so they need
//! no dependency and every run sees the same inputs.

use glam::DVec3;

/// SplitMix64: fast, and good enough for picking test inputs.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub(crate) fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    pub(crate) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Uniform in `[lo, hi)` on a log scale: `lo` and `hi` positive.
    pub(crate) fn log_range(&mut self, lo: f64, hi: f64) -> f64 {
        (lo.ln() + (hi.ln() - lo.ln()) * self.unit()).exp()
    }

    /// Each coordinate uniform in `[-extent, extent)`.
    pub(crate) fn point(&mut self, extent: f64) -> DVec3 {
        DVec3::new(
            self.range(-extent, extent),
            self.range(-extent, extent),
            self.range(-extent, extent),
        )
    }

    /// A uniformly random unit vector.
    pub(crate) fn direction(&mut self) -> DVec3 {
        loop {
            let p = self.point(1.0);
            let length = p.length();
            if length > 0.1 && length <= 1.0 {
                return p / length;
            }
        }
    }

    /// A uniformly random barycentric point of the closed triangle.
    pub(crate) fn bary(&mut self) -> DVec3 {
        let (mut a, mut b) = (self.unit(), self.unit());
        if a + b > 1.0 {
            (a, b) = (1.0 - a, 1.0 - b);
        }
        DVec3::new(a, b, 1.0 - a - b)
    }
}
