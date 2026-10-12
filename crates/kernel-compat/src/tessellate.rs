//! How solids are sampled for drawing ([`Display`]), and the drawing of
//! loose curves and patches, such as a failure's evidence.
//!
//! The old kernel's tessellation is gone: the facade's solids are empty,
//! so they tessellate to the empty mesh ([`Solid::tessellate`]), and loose
//! geometry is drawn as simply as it can be until the Varde kernel's
//! tessellation lands: a curve as its chord, a patch not at all.
//!
//! [`Solid::tessellate`]: crate::Solid::tessellate

use glam::DVec3;

use crate::Tolerance;
use crate::patch::{Conic3, Patch};

/// How finely [`Solid::tessellate`](crate::Solid::tessellate) samples:
/// each edge curve is cut into segments whose chords are at most
/// [`Display::chord`] from the curve and turn at most
/// [`Display::MAX_TURN_DEGREES`] from one to the next, with at most
/// [`Display::MAX_SEGMENTS`] segments to an edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    fit: f64,
    /// The chord error asked for by the view, in mm, if it's drawn for
    /// one: taken in place of the share of the diagonal.
    view: Option<f64>,
}

impl Display {
    /// The chord error allowed relative to the solid's diagonal.
    pub const RELATIVE_CHORD: f64 = 1e-3;
    /// The most one segment of an edge may turn, in degrees.
    pub const MAX_TURN_DEGREES: f64 = 10.0;
    /// The most segments an edge is cut into.
    pub const MAX_SEGMENTS: u32 = 64;
    /// The most two patches' normals may differ along an edge they share
    /// for the edge to be drawn smooth, in degrees.
    pub const SMOOTH_DEGREES: f64 = 1.0;

    /// Sampling to within the fit tolerance of `tol` at best.
    pub fn new(tol: &Tolerance) -> Display {
        Display {
            fit: tol.fit(),
            view: None,
        }
    }

    /// Sampling for a view: chords within `chord` mm of their curves
    /// whatever the solid's size, but no closer than the fit tolerance of
    /// `tol`. The turn per segment still bounds how coarse it gets, so a
    /// circle keeps at least 36 segments however coarse `chord` is. A
    /// `chord` that isn't positive and finite is ignored.
    pub fn for_view(tol: &Tolerance, chord: f64) -> Display {
        Display {
            fit: tol.fit(),
            view: (chord.is_finite() && chord > 0.0).then_some(chord),
        }
    }

    /// The chord error allowed on a solid `diagonal` across: its
    /// [`Display::RELATIVE_CHORD`] (or the view's chord, for
    /// [`Display::for_view`]), but no less than the fit tolerance.
    pub fn chord(&self, diagonal: f64) -> f64 {
        let chord = self.view.unwrap_or(Self::RELATIVE_CHORD * diagonal);
        self.fit.max(chord)
    }
}

/// A loose patch sampled for drawing ([`Display::sample_patch`]): its
/// points, their unit normals, and its triangles as indices into them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatchSamples {
    pub points: Vec<DVec3>,
    pub normals: Vec<DVec3>,
    pub indices: Vec<u32>,
}

impl Display {
    /// `curve` as drawn: for now its two ends, the chord.
    pub fn flatten(&self, curve: &Conic3, _diagonal: f64) -> Vec<DVec3> {
        vec![curve.p0, curve.p1]
    }

    /// `patch`, on its own, sampled for drawing: for now nothing.
    pub fn sample_patch(&self, _patch: &Patch, _diagonal: f64) -> PatchSamples {
        PatchSamples::default()
    }
}

impl Default for Display {
    /// At the default [`Tolerance`].
    fn default() -> Self {
        Display::new(&Tolerance::DEFAULT)
    }
}

#[cfg(test)]
mod tests;
