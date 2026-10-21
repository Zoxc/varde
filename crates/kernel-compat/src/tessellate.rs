//! How solids are sampled for drawing ([`Display`]), and the drawing of
//! loose curves and patches, such as a failure's evidence.
//!
//! The old kernel's tessellation is gone: the facade's solids are empty,
//! so they tessellate to the empty mesh ([`Solid::tessellate`]). A loose
//! curve is cut as an edge of a solid would be ([`Display::flatten`]); a
//! loose patch isn't drawn until the Varde kernel's tessellation lands.
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
    /// `curve` cut as an edge of a solid `diagonal` across is drawn
    /// ([`Display::chord`], [`Display::MAX_TURN_DEGREES`],
    /// [`Display::MAX_SEGMENTS`]): its points at equal parameter steps,
    /// both ends included. Geometry that isn't a solid's, such as a
    /// failure's [`Evidence`](crate::Evidence), is drawn this way.
    pub fn flatten(&self, curve: &Conic3, diagonal: f64) -> Vec<DVec3> {
        let n = segments(curve, self.chord(diagonal));
        (0..=n)
            .map(|s| curve.eval(f64::from(s) / f64::from(n)))
            .collect()
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

/// `cos` of [`Display::MAX_TURN_DEGREES`], written out so no platform's
/// `cos` decides a segment count.
const COS_TURN: f64 = 0.984807753012208;
/// [`Display::MAX_TURN_DEGREES`] in radians.
const TURN: f64 = 0.17453292519943295;

/// How many equal parameter steps `curve` needs: chords within `chord` of
/// the curve at their middles, and the tangent turning at most
/// [`Display::MAX_TURN_DEGREES`] along each, up to
/// [`Display::MAX_SEGMENTS`]. Uses only correctly rounded arithmetic, so
/// every platform gets the same count.
pub(crate) fn segments(curve: &Conic3, chord: f64) -> u32 {
    let max = Display::MAX_SEGMENTS;
    // The whole turn is the angle between the end tangents; the chord
    // between their unit vectors is shorter, so this underestimates.
    let d0 = (curve.c - curve.p0).normalize_or_zero();
    let d1 = (curve.p1 - curve.c).normalize_or_zero();
    let mut n = steps((d0 - d1).length() / TURN);
    loop {
        let (error, turned) = measure(curve, n);
        if (error <= chord && !turned) || n >= max {
            return n;
        }
        // The chord error of a smooth curve falls with the square of the
        // step.
        let grow = if error > chord {
            steps(f64::from(n) * (error / chord).sqrt())
        } else {
            1
        };
        n = grow.max(n + 1).min(max);
    }
}

/// `x` rounded up to a segment count within `1..=MAX_SEGMENTS` (NaN to
/// the most).
fn steps(x: f64) -> u32 {
    let max = Display::MAX_SEGMENTS;
    if x.is_nan() {
        max
    } else {
        // Clamped first, so the cast is exact.
        x.ceil().clamp(1.0, f64::from(max)) as u32
    }
}

/// The largest distance of `curve`'s points halfway along each of `n`
/// equal steps from that step's chord, and whether its tangent turns by
/// more than [`Display::MAX_TURN_DEGREES`] along any step.
fn measure(curve: &Conic3, n: u32) -> (f64, bool) {
    let at = |s: u32| f64::from(s) / f64::from(n);
    let mut error = 0.0f64;
    let mut turned = false;
    let mut prev = curve.eval_deriv(0.0);
    for s in 0..n {
        let next = curve.eval_deriv(at(s + 1));
        let mid = curve.eval((f64::from(s) + 0.5) / f64::from(n));
        let d = distance_to_line(mid, prev.0, next.0);
        // NaN counts as too far.
        error = if d.is_nan() {
            f64::INFINITY
        } else {
            error.max(d)
        };
        let (a, b) = (prev.1.normalize_or_zero(), next.1.normalize_or_zero());
        if a != DVec3::ZERO && b != DVec3::ZERO && a.dot(b) < COS_TURN {
            turned = true;
        }
        prev = next;
    }
    (error, turned)
}

/// The distance from `x` to the line through `a` and `b` (to `a` if they
/// are the same point).
fn distance_to_line(x: DVec3, a: DVec3, b: DVec3) -> f64 {
    let (ab, ax) = (b - a, x - a);
    let len2 = ab.length_squared();
    if len2 > 0.0 {
        (ax - ab * (ax.dot(ab) / len2)).length()
    } else {
        ax.length()
    }
}

#[cfg(test)]
mod tests;
