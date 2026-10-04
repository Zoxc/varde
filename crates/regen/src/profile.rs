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
use varde_sketch::{Curve, Id, Piece, Profiles, Sketch};

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
            ProfileError::Fit => {
                f.write_str("a spline couldn't be fitted: it stops or turns back on itself")
            }
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

/// Why a split's curves give no open chain ([`chain`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ChainError {
    /// A curve isn't in the sketch any more, or names a point it doesn't
    /// have, or a spline has no shape.
    Missing,
    /// A curve is closed (a circle, a closed spline), or the curves join
    /// up into a loop.
    Closed,
    /// The curves don't join end to end into one chain: three or more
    /// ends meet at a point, or they're in several pieces.
    Branches,
    /// What turning them into conics found wrong.
    Profile(ProfileError),
}

impl From<ProfileError> for ChainError {
    fn from(e: ProfileError) -> Self {
        ChainError::Profile(e)
    }
}

/// The open chain the curves `curves` of `sketch` make, as conics, each
/// starting to the bit where the one before ends, or why they make none.
/// Each curve is whole and has two ends (a line, an arc or an open
/// spline); ends join where they're the same point or within `join` of
/// each other, two at a time; the curves must join into one chain with
/// two free ends. It runs the way its lowest curve (by id) runs. Each
/// curve's conics are made as a region's pieces are (lines, arcs of at
/// most 90°, splines fitted within `fit`), from the point where the one
/// before ends: a joint's place is where the earlier curve along the
/// chain ends. At most [`MAX_PROFILE_SEGMENTS`] conics; the work is
/// linear in the curves but for finding the joints, quadratic in them
/// (`curves` holds at most a few hundred).
pub(crate) fn chain(
    sketch: &Sketch,
    curves: &[Id],
    join: f64,
    fit: f64,
) -> Result<Vec<Segment>, ChainError> {
    walk_chain(sketch, curves, join, fit, false).map(|(segments, _)| segments)
}

/// [`chain`], or with `closed` also a closed one: a circle or a closed
/// spline alone (a circle from its point on its `+x` side round
/// counter-clockwise, a spline from its start), or curves joining into
/// one loop (from the lowest curve's start, the way it runs). Whether the
/// chain closes comes with it. For a sweep's path.
pub(crate) fn path_chain(
    sketch: &Sketch,
    curves: &[Id],
    join: f64,
    fit: f64,
) -> Result<(Vec<Segment>, bool), ChainError> {
    walk_chain(sketch, curves, join, fit, true)
}

/// [`chain`] and [`path_chain`]: a closed chain only with `closed`.
fn walk_chain(
    sketch: &Sketch,
    curves: &[Id],
    join: f64,
    fit: f64,
    closed: bool,
) -> Result<(Vec<Segment>, bool), ChainError> {
    let at = |id| sketch.point(id).map(|point| point.at);
    if closed && let [only] = curves {
        let entry = sketch.curve(*only).ok_or(ChainError::Missing)?;
        if entry.curve.ends().is_none() {
            return closed_curve(sketch, *only, fit).map(|segments| (segments, true));
        }
    }
    // Each curve's end points and where they are.
    let mut ends: Vec<[(Id, DVec2); 2]> = Vec::with_capacity(curves.len());
    for &id in curves {
        let entry = sketch.curve(id).ok_or(ChainError::Missing)?;
        let [a, b] = entry.curve.ends().ok_or(ChainError::Closed)?;
        let (Some(pa), Some(pb)) = (at(a), at(b)) else {
            return Err(ChainError::Missing);
        };
        ends.push([(a, pa), (b, pb)]);
    }
    // The end each end joins, as (curve, end).
    let n = ends.len();
    let mut partner: Vec<[Option<(usize, usize)>; 2]> = vec![[None; 2]; n];
    for i in 0..n {
        for e in 0..2 {
            for j in (i + 1)..n {
                for f in 0..2 {
                    let (p, q) = (ends[i][e], ends[j][f]);
                    if p.0 != q.0 && p.1.distance(q.1) > join {
                        continue;
                    }
                    if partner[i][e].is_some() || partner[j][f].is_some() {
                        return Err(ChainError::Branches);
                    }
                    partner[i][e] = Some((j, f));
                    partner[j][f] = Some((i, e));
                }
            }
        }
    }
    // A curve whose own two ends meet is a loop of its own.
    if (0..n).any(|i| ends[i][0].0 == ends[i][1].0 || ends[i][0].1.distance(ends[i][1].1) <= join) {
        return Err(ChainError::Closed);
    }
    let mut free = (0..n).flat_map(|i| (0..2).map(move |e| (i, e)));
    let (start, loops) = match free.find(|&(i, e)| partner[i][e].is_none()) {
        Some(start) => (start, false),
        // Every end joined: a loop, walked from the lowest curve's start.
        None if closed => ((0, 0), true),
        None => return Err(ChainError::Closed),
    };
    // Walked from a free end (or round the loop): each step a curve and
    // whether it runs backwards (from its end to its start).
    let mut walk: Vec<(usize, bool)> = Vec::with_capacity(n);
    let (mut curve, mut from) = start;
    loop {
        walk.push((curve, from == 1));
        match partner[curve][1 - from] {
            Some(next) if loops && next == start => break,
            Some((next, end)) if walk.len() < n => (curve, from) = (next, end),
            Some(_) => return Err(ChainError::Branches),
            None => break,
        }
    }
    if walk.len() != n {
        return Err(ChainError::Branches);
    }
    // The way the lowest curve runs: `curves` is sorted, so it's the
    // first.
    if walk
        .iter()
        .any(|&(curve, backwards)| curve == 0 && backwards)
    {
        walk.reverse();
        for step in &mut walk {
            step.1 = !step.1;
        }
    }
    let mut out = Segments {
        count: 0,
        fit,
        segments: Vec::new(),
    };
    let mut joint: Option<DVec2> = None;
    for &(index, backwards) in &walk {
        let [first, last] = ends[index];
        let (start, end) = if backwards {
            (last, first)
        } else {
            (first, last)
        };
        let a = joint.unwrap_or(start.1);
        let b = end.1;
        let id = curves[index];
        let entry = sketch.curve(id).ok_or(ChainError::Missing)?;
        let curve = u64::from(id.get());
        match &entry.curve {
            Curve::Line { .. } => {
                let line = Conic2::line(a, b).map_err(ProfileError::from)?;
                out.push(line, curve)?;
            }
            &Curve::Arc { center, .. } => {
                let center = at(center).ok_or(ChainError::Missing)?;
                // Counter-clockwise from its start to its end.
                let radius = first.1.distance(center);
                let turn = |p: DVec2| varde_kernel::trig::angle(p - center);
                let mut sweep = turn(last.1) - turn(first.1);
                if sweep <= 0.0 {
                    sweep += 2.0 * PI;
                }
                arc(center, radius, a, b, sweep, !backwards, curve, &mut out)?;
            }
            Curve::Spline(spline) => {
                let shape = sketch.spline_shape(spline).ok_or(ChainError::Missing)?;
                fit::spline(&shape, backwards, a, b, curve, &mut out)?;
            }
            Curve::Circle { .. } => return Err(ChainError::Closed),
        }
        joint = Some(b);
    }
    if loops
        && let (Some(first), Some(last)) = (out.segments.first().copied(), out.segments.last_mut())
    {
        // Round to where it started, to the bit.
        last.conic.p1 = first.conic.p0;
    }
    Ok((out.segments, loops))
}

/// The conics of the closed curve `id` of `sketch` (a circle or a closed
/// spline), as [`path_chain`] makes them.
fn closed_curve(sketch: &Sketch, id: Id, fit: f64) -> Result<Vec<Segment>, ChainError> {
    let entry = sketch.curve(id).ok_or(ChainError::Missing)?;
    let mut out = Segments {
        count: 0,
        fit,
        segments: Vec::new(),
    };
    let curve = u64::from(id.get());
    match &entry.curve {
        &Curve::Circle { center, radius } => {
            let center = (sketch.point(center).map(|point| point.at)).ok_or(ChainError::Missing)?;
            let start = center + DVec2::new(radius, 0.0);
            arc(
                center,
                radius,
                start,
                start,
                2.0 * PI,
                true,
                curve,
                &mut out,
            )?;
        }
        Curve::Spline(spline) => {
            let shape = sketch.spline_shape(spline).ok_or(ChainError::Missing)?;
            let t = shape.breaks().first().copied().ok_or(ChainError::Missing)?;
            let start = shape.eval(t)[0];
            fit::spline(&shape, false, start, start, curve, &mut out)?;
        }
        Curve::Line { .. } | Curve::Arc { .. } => return Err(ChainError::Closed),
    }
    Ok(out.segments)
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
