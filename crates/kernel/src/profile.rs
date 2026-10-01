//! Profiles: the plane regions an extrude sweeps, as closed loops of
//! conic segments.
//!
//! A [`Profile`] is what [`extrude`](crate::extrude()) takes. It is neutral:
//! the kernel knows no sketch, and whoever builds one (the regeneration
//! lane, from a sketch's region) turns lines into straight conics, arcs
//! into exact ones of at most 90° and splines into fitted chains. Outer
//! loops run counter-clockwise and holes clockwise, so the region is on
//! the left of every segment, and loops that share curves are already
//! merged.

use glam::DVec2;

use crate::MAX_COORD;
use crate::patch::{Conic2, PatchError};
use crate::quadrature::GAUSS8;

/// The most segments a profile may have, all loops together.
pub const MAX_PROFILE_SEGMENTS: usize = 1 << 16;

/// A plane region: its outer loops, counter-clockwise, and its holes,
/// clockwise, none crossing or touching another. The region is where the
/// loops wind once.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Profile {
    pub loops: Vec<Loop>,
}

/// A closed loop: each segment starts, to the bit, where the one before it
/// ends, and the last ends where the first starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loop {
    pub segments: Vec<Segment>,
}

/// One piece of a loop, and the curve it came from, whose id names the
/// wall it sweeps ([`FacePart::Side`](crate::mesh::FacePart::Side)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub conic: Conic2,
    pub curve: u64,
}

impl Segment {
    /// The straight segment from `a` to `b` of `curve`.
    pub fn line(a: DVec2, b: DVec2, curve: u64) -> Result<Segment, PatchError> {
        Ok(Segment {
            conic: Conic2::line(a, b)?,
            curve,
        })
    }
}

/// Why a profile can't be extruded or revolved. Loops and segments are
/// named by their indices in the profile as given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileError {
    /// No loops.
    Empty,
    /// More than [`MAX_PROFILE_SEGMENTS`] segments.
    TooManySegments(usize),
    /// A loop of fewer than two segments.
    Short(usize),
    /// A segment's ends past [`MAX_COORD`], or its control point or weight
    /// outside the patch bounds.
    Segment(usize, usize, PatchError),
    /// A segment whose ends are the same point, or that runs back on
    /// itself (a straight segment whose control point lies past an end).
    Degenerate(usize, usize),
    /// Segment `(loop, segment)` doesn't end where the next one starts.
    Open(usize, usize),
    /// A loop that encloses no area.
    Area(usize),
    /// The segments of a loop meet at the start of segment `(loop,
    /// segment)` at an angle of about 0° or 360°: a cusp.
    Cusp(usize, usize),
    /// Two segments `(loop, segment)` cross, touch, or come within the
    /// resolution of each other.
    Touching([(usize, usize); 2]),
    /// The loops don't nest as outer loops and holes: a hole outside
    /// every outer loop, an outer loop inside another's material, or a
    /// loop running the wrong way.
    Nesting,
    /// The region's caps couldn't be triangulated.
    Triangulation,
    /// A curved segment `(loop, segment)` needs halving for the caps
    /// below the size the resolution allows: detail too small for the
    /// tolerance.
    TooFine(usize, usize),
    /// Revolving: segment `(loop, segment)` reaches across the axis.
    CrossesAxis(usize, usize),
    /// Revolving: the region touches the axis at a single point, where
    /// segment `(loop, segment)` starts (a vertex on the axis with no
    /// edge along it, in a full turn) or inside that segment (in any
    /// turn): the solid would pinch to a point there.
    TouchesAxis(usize, usize),
    /// Revolving: a part turn so close to a full one that its two ends
    /// come within the resolution of each other.
    NearlyFullTurn,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Empty => f.write_str("the profile has no loops"),
            ProfileError::TooManySegments(n) => write!(
                f,
                "the profile has {n} segments, more than {MAX_PROFILE_SEGMENTS}"
            ),
            ProfileError::Short(l) => write!(f, "loop {l} has fewer than two segments"),
            ProfileError::Segment(l, s, e) => write!(f, "segment {s} of loop {l}: {e}"),
            ProfileError::Degenerate(l, s) => write!(f, "segment {s} of loop {l} is degenerate"),
            ProfileError::Open(l, s) => {
                write!(
                    f,
                    "segment {s} of loop {l} doesn't end where the next starts"
                )
            }
            ProfileError::Area(l) => write!(f, "loop {l} encloses no area"),
            ProfileError::Cusp(l, s) => {
                write!(f, "loop {l} has a cusp where segment {s} starts")
            }
            ProfileError::Touching([(la, sa), (lb, sb)]) => write!(
                f,
                "segment {sa} of loop {la} and segment {sb} of loop {lb} touch or cross"
            ),
            ProfileError::Nesting => f.write_str("the loops don't nest as outer loops and holes"),
            ProfileError::Triangulation => f.write_str("the profile couldn't be triangulated"),
            ProfileError::TooFine(l, s) => write!(
                f,
                "segment {s} of loop {l} is too small or sharply bent for the resolution"
            ),
            ProfileError::CrossesAxis(l, s) => {
                write!(f, "segment {s} of loop {l} crosses the axis")
            }
            ProfileError::TouchesAxis(l, s) => {
                write!(f, "loop {l} touches the axis at a point at segment {s}")
            }
            ProfileError::NearlyFullTurn => {
                f.write_str("the turn is so nearly full that its ends touch")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// The number of segments in all loops.
    pub fn segment_count(&self) -> usize {
        self.loops
            .iter()
            .fold(0usize, |n, l| n.saturating_add(l.segments.len()))
    }

    /// The checks that need no tolerance: at least one loop, at most
    /// [`MAX_PROFILE_SEGMENTS`] segments, each loop of two or more
    /// segments, closed to the bit, and enclosing some area, each segment
    /// within the patch bounds with its ends within [`MAX_COORD`] and
    /// apart. Whether loops touch, cross or nest is for the extrude to
    /// find, with the design's resolution.
    pub fn check(&self) -> Result<(), ProfileError> {
        if self.loops.is_empty() {
            return Err(ProfileError::Empty);
        }
        let n = self.segment_count();
        if n > MAX_PROFILE_SEGMENTS {
            return Err(ProfileError::TooManySegments(n));
        }
        for (l, lp) in self.loops.iter().enumerate() {
            if lp.segments.len() < 2 {
                return Err(ProfileError::Short(l));
            }
            for (s, seg) in lp.segments.iter().enumerate() {
                let conic = &seg.conic;
                conic.check().map_err(|e| ProfileError::Segment(l, s, e))?;
                for end in [conic.p0, conic.p1] {
                    let m = end.abs().max_element();
                    if m > f64::from(MAX_COORD) {
                        return Err(ProfileError::Segment(l, s, PatchError::Coordinate(m)));
                    }
                }
                if conic.p0 == conic.p1 {
                    return Err(ProfileError::Degenerate(l, s));
                }
            }
            let segs = &lp.segments;
            for s in 0..segs.len() {
                if segs[s].conic.p1 != segs[(s + 1) % segs.len()].conic.p0 {
                    return Err(ProfileError::Open(l, s));
                }
            }
            let area = lp.area();
            if !(area != 0.0 && area.is_finite()) {
                return Err(ProfileError::Area(l));
            }
        }
        Ok(())
    }

    /// The region's area: the loops' signed areas summed, positive for a
    /// valid profile.
    pub fn area(&self) -> f64 {
        self.loops.iter().map(Loop::area).sum()
    }
}

impl Loop {
    /// The signed area the loop encloses, positive when it runs
    /// counter-clockwise: the polygon of its segments' ends, measured from
    /// its first point, plus the area each segment bulges out of its chord
    /// (`½∫ (P − p0) × P' dt`, by Gauss–Legendre over pieces of weights
    /// near 1, which is accurate to about rounding).
    pub fn area(&self) -> f64 {
        let Some(first) = self.segments.first() else {
            return 0.0;
        };
        let origin = first.conic.p0;
        let mut sum = 0.0;
        for seg in &self.segments {
            let c = &seg.conic;
            sum += (c.p0 - origin).perp_dot(c.p1 - origin) * 0.5;
            sum += bulge_area(c);
        }
        sum
    }
}

/// The signed area between a conic and its chord, `½∫ (P − p0) × P' dt`:
/// positive when the curve bulges to the right of its direction, out of a
/// counter-clockwise loop.
///
/// By Gauss–Legendre over pieces split off exactly until their weights
/// are within [`WELL_SHAPED`] (a weight far from 1 crowds the curve's
/// points towards its ends or its middle, which the rule can't follow):
/// each piece's own bulge, plus the triangle it makes with `p0`.
fn bulge_area(c: &Conic2) -> f64 {
    if c.w == 1.0 && c.c == (c.p0 + c.p1) * 0.5 {
        return 0.0;
    }
    let mut sum = 0.0;
    pieces(c, 0, &mut |piece| {
        sum += (piece.p0 - c.p0).perp_dot(piece.p1 - c.p0) * 0.5;
        let mut bulge = 0.0;
        for &(t, w) in &GAUSS8 {
            let (p, d) = piece.eval_deriv(t);
            bulge += w * (p - piece.p0).perp_dot(d);
        }
        sum += bulge * 0.5;
    });
    sum
}

/// The weights within which a conic is integrated as it is: further from
/// 1, it is split first.
const WELL_SHAPED: std::ops::RangeInclusive<f64> = 0.9..=1.1;

/// Calls `f` on `c`, or on its halves, split at `½` until each piece's
/// weight is [`WELL_SHAPED`] (at most 8 times: halving takes a weight
/// `w` to `√((1 + w)/2)`, so even the bounds get there in 8), in order.
fn pieces(c: &Conic2, depth: u32, f: &mut impl FnMut(&Conic2)) {
    if depth < 8
        && !WELL_SHAPED.contains(&c.w)
        && let Ok([a, b]) = c.split_half()
    {
        pieces(&a, depth + 1, f);
        pieces(&b, depth + 1, f);
        return;
    }
    f(c);
}

#[cfg(test)]
pub(crate) mod tests;
