//! From a sketch's region to the kernel's [`Profile`]: its pieces as
//! conics.
//!
//! [`Profiles::merge`] gives the loops of the regions an extrude takes as
//! pieces of the sketch's curves. Each piece becomes one or more
//! [`Segment`]s named by its curve's id:
//!
//! - a line piece, the straight conic between its ends;
//! - a piece of a circle or an arc, exact conics of at most 90° each,
//!   halving it until they are (at most four);
//! - a piece of a spline, a chain of conics fitted to it within the
//!   tolerance, each conic over the longest run of the spline's Bézier
//!   segments it fits, a line wherever a run is straight within the
//!   tolerance, curved conics meeting along the same tangent; fitted in
//!   the spline's own direction, so a piece run backwards gives the same
//!   conics to the bit, reversed ([`fit`]).
//!
//! Every piece's ends are put at its vertices ([`Profiles::vertices`]),
//! which the pieces meeting there share, so the loops close to the bit as
//! the kernel wants. Arcs are built from their ends and centre without
//! `cos` or `sin`, so no platform's maths library decides a bit.

use std::f64::consts::{FRAC_PI_2, PI};
use std::fmt;

use glam::DVec2;
use varde_kernel::patch::{Conic2, PatchError};
use varde_kernel::{Loop, MAX_PROFILE_SEGMENTS, Profile, Segment};
use varde_sketch::{Curve, Piece, Profiles, Sketch};

mod fit;

/// Why a region's pieces couldn't be turned into a profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileError {
    /// A piece names a curve or a vertex the sketch doesn't have, or a
    /// spline without a shape: profiles not found from this sketch.
    Missing,
    /// More segments than a profile may have
    /// ([`MAX_PROFILE_SEGMENTS`]).
    TooManySegments,
    /// A spline couldn't be fitted within the tolerance: it stops or
    /// turns back on itself somewhere.
    Fit,
    /// A segment came out outside the patch bounds.
    Patch(PatchError),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::Missing => f.write_str("the region's curves aren't in the sketch"),
            ProfileError::TooManySegments => write!(
                f,
                "the region needs more than {MAX_PROFILE_SEGMENTS} segments"
            ),
            ProfileError::Fit => f.write_str("a spline couldn't be fitted within the tolerance"),
            ProfileError::Patch(e) => write!(f, "a curve is out of bounds: {e}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl From<PatchError> for ProfileError {
    fn from(e: PatchError) -> Self {
        ProfileError::Patch(e)
    }
}

/// The profile of `loops` (as [`Profiles::merge`] gives them) of
/// `profiles`, found from `sketch`, splines fitted within `fit` (in
/// sketch units, millimetres).
pub fn profile(
    sketch: &Sketch,
    profiles: &Profiles,
    loops: &[Vec<Piece>],
    fit: f64,
) -> Result<Profile, ProfileError> {
    let mut out = Segments {
        count: 0,
        fit,
        segments: Vec::new(),
    };
    let mut profile = Profile::default();
    for pieces in loops {
        for piece in pieces {
            piece_segments(sketch, profiles, piece, &mut out)?;
        }
        profile.loops.push(Loop {
            segments: std::mem::take(&mut out.segments),
        });
    }
    Ok(profile)
}

/// Where segments go, counted against [`MAX_PROFILE_SEGMENTS`] as
/// they're made.
struct Segments {
    /// Segments made so far, all loops together.
    count: usize,
    /// The spline fitting tolerance.
    fit: f64,
    /// The loop being made.
    segments: Vec<Segment>,
}

impl Segments {
    fn push(&mut self, conic: Conic2, curve: u64) -> Result<(), ProfileError> {
        self.count += 1;
        if self.count > MAX_PROFILE_SEGMENTS {
            return Err(ProfileError::TooManySegments);
        }
        self.segments.push(Segment { conic, curve });
        Ok(())
    }
}

/// Adds the segments of `piece`.
fn piece_segments(
    sketch: &Sketch,
    profiles: &Profiles,
    piece: &Piece,
    out: &mut Segments,
) -> Result<(), ProfileError> {
    let vertex = |index: usize| profiles.vertices.get(index).copied();
    let (Some(a), Some(b)) = (vertex(piece.start), vertex(piece.end)) else {
        return Err(ProfileError::Missing);
    };
    let entry = sketch.curve(piece.curve).ok_or(ProfileError::Missing)?;
    let at = |id| sketch.point(id).map(|point| point.at);
    let curve = u64::from(piece.curve.get());
    let sweep = (piece.to - piece.from).abs();
    let ccw = piece.to > piece.from;
    match &entry.curve {
        Curve::Line { .. } => out.push(Conic2::line(a, b)?, curve),
        &Curve::Circle { center, radius } => {
            let center = at(center).ok_or(ProfileError::Missing)?;
            arc(center, radius, a, b, sweep, ccw, curve, out)
        }
        &Curve::Arc { center, start, .. } => {
            let center = at(center).ok_or(ProfileError::Missing)?;
            let radius = at(start).ok_or(ProfileError::Missing)?.distance(center);
            arc(center, radius, a, b, sweep, ccw, curve, out)
        }
        Curve::Spline(spline) => {
            let shape = sketch.spline_shape(spline).ok_or(ProfileError::Missing)?;
            let (lo, hi) = (piece.from.min(piece.to), piece.from.max(piece.to));
            let part = shape.piece(lo, hi).ok_or(ProfileError::Missing)?;
            fit::spline(&part, !ccw, a, b, curve, out)
        }
    }
}

/// Adds the arc of the circle about `center` of `radius` from `a` to `b`,
/// turning by `sweep` radians, counter-clockwise if `ccw`: halved until
/// each part is at most 90°, each part exact
/// ([`Conic2::arc_between`]).
#[allow(clippy::too_many_arguments)]
fn arc(
    center: DVec2,
    radius: f64,
    a: DVec2,
    b: DVec2,
    sweep: f64,
    ccw: bool,
    curve: u64,
    out: &mut Segments,
) -> Result<(), ProfileError> {
    // A little over 90° from rounding stays one part; a whole turn is four.
    let mut halvings = 0;
    while halvings < 2 && sweep / f64::from(1 << halvings) > FRAC_PI_2 * (1.0 + 1e-9) {
        halvings += 1;
    }
    arc_parts(center, radius, a, b, sweep, ccw, halvings, curve, out)
}

/// [`arc`], halved `halvings` times.
#[allow(clippy::too_many_arguments)]
fn arc_parts(
    center: DVec2,
    radius: f64,
    a: DVec2,
    b: DVec2,
    sweep: f64,
    ccw: bool,
    halvings: u32,
    curve: u64,
    out: &mut Segments,
) -> Result<(), ProfileError> {
    if halvings == 0 {
        return out.push(Conic2::arc_between(center, radius, a, b)?, curve);
    }
    let middle = arc_middle(center, radius, a, b, sweep, ccw);
    let half = sweep / 2.0;
    arc_parts(
        center,
        radius,
        a,
        middle,
        half,
        ccw,
        halvings - 1,
        curve,
        out,
    )?;
    arc_parts(
        center,
        radius,
        middle,
        b,
        half,
        ccw,
        halvings - 1,
        curve,
        out,
    )
}

/// The point halfway round the arc about `center` of `radius` from `a` to
/// `b`, turning by `sweep`, counter-clockwise if `ccw`. Square roots only:
/// it lies off the chord's middle, square to it on the side the arc turns
/// away from (the right going counter-clockwise), or for an arc of nearly
/// a whole turn, opposite the ends' middle, better conditioned there.
fn arc_middle(center: DVec2, radius: f64, a: DVec2, b: DVec2, sweep: f64, ccw: bool) -> DVec2 {
    let (u, v) = (a - center, b - center);
    let direction = if sweep > 1.5 * PI {
        -(u + v)
    } else {
        let chord = v - u;
        if ccw {
            DVec2::new(chord.y, -chord.x)
        } else {
            DVec2::new(-chord.y, chord.x)
        }
    };
    center + direction * (radius / direction.length())
}

#[cfg(test)]
mod tests;
