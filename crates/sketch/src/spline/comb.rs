//! A spline's curvature comb.

use glam::DVec2;

use super::curvature;
use crate::bounds;
use crate::{Id, Sketch};

/// The most teeth a spline's curvature comb has
/// ([`Sketch::curvature_comb`]), however many segments it has.
pub const MAX_COMB_TEETH: usize = 1000;

/// The most teeth a curvature comb has in each of a spline's segments.
const COMB_PER_SEGMENT: usize = 16;

/// The curvature times a spline's size below which its curvature comb
/// counts it straight: rounding's noise, not a turn.
const COMB_FLAT: f64 = 1e-9;

impl Sketch {
    /// The curvature comb of the spline `curve`: places along it, evenly
    /// by its parameter within each of its segments, at most
    /// [`MAX_COMB_TEETH`] of them, each with its curvature times the unit
    /// normal on its left, which points to where it turns and is as long
    /// as one over the radius it turns by. Where it doesn't move the
    /// curvature is left at zero. `None` if `curve` is no spline with a
    /// shape.
    pub fn curvature_comb(&self, curve: Id) -> Option<Vec<[DVec2; 2]>> {
        let shape = self.curve_shape(curve)?;
        let segments = shape.breaks().len() - 1;
        let per = (MAX_COMB_TEETH.saturating_sub(1) / segments.max(1)).clamp(1, COMB_PER_SEGMENT);
        let mut params = shape.samples(per);
        params.truncate(MAX_COMB_TEETH);
        let evals: Vec<[DVec2; 3]> = params.into_iter().map(|t| shape.eval(t)).collect();
        // Curving below rounding's reach of the spline's size reads as
        // straight, so a flat spline (or a flat stretch) has no noise of
        // teeth flipping side.
        let least = COMB_FLAT / bounds::size(bounds::of(evals.iter().map(|[place, ..]| *place)));
        let teeth = evals
            .into_iter()
            .map(|[place, first, second]| {
                let curvature = curvature(first, second);
                let normal = first.perp().normalize_or_zero();
                let curvature = if curvature.is_finite() && curvature.abs() > least {
                    curvature
                } else {
                    0.0
                };
                [place, normal * curvature]
            })
            .collect();
        Some(teeth)
    }
}
