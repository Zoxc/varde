/// The design's tolerances, in model units (mm): the **fit** tolerance,
/// how far a fitted curve or patch may be from the true surfaces, and the
/// **resolution**, a thousandth of it, the size below which the kernel
/// stops refining and the margin its hull rules keep.
///
/// Neither ever decides that two things are the same vertex or edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    fit: f64,
}

impl Tolerance {
    /// The smallest fit tolerance: 0.01 µm.
    pub const MIN_FIT: f64 = 1e-5;
    /// The largest fit tolerance: 0.1 mm.
    pub const MAX_FIT: f64 = 1e-1;
    /// A fit tolerance of 1 µm.
    pub const DEFAULT: Tolerance = Tolerance { fit: 1e-3 };

    /// The tolerance with fit tolerance `fit`, if it lies within
    /// [`Self::MIN_FIT`]`..=`[`Self::MAX_FIT`].
    pub fn new(fit: f64) -> Option<Tolerance> {
        (Self::MIN_FIT..=Self::MAX_FIT)
            .contains(&fit)
            .then_some(Tolerance { fit })
    }

    /// How far a fitted curve or patch may be from the true surfaces.
    pub fn fit(self) -> f64 {
        self.fit
    }

    /// A thousandth of the fit tolerance: where refinement stops, and the
    /// margin hulls keep from each other.
    pub fn resolution(self) -> f64 {
        self.fit / 1000.0
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_kept() {
        assert_eq!(Tolerance::new(1e-3), Some(Tolerance::DEFAULT));
        assert_eq!(Tolerance::default().resolution(), 1e-6);
        assert!(Tolerance::new(Tolerance::MIN_FIT).is_some());
        assert!(Tolerance::new(Tolerance::MAX_FIT).is_some());
        for fit in [0.0, -1e-3, 1e-6, 0.2, f64::NAN, f64::INFINITY] {
            assert_eq!(Tolerance::new(fit), None, "{fit}");
        }
    }
}
