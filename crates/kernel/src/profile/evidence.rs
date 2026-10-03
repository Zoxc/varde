//! The evidence of a profile's errors: the segments, loops and points a
//! [`ProfileError`] is about, placed in space by the operation's frame,
//! and the sketch curves they came from.
//!
//! Every profile error names what it is about by indices into the profile
//! as given, so the evidence is gathered where the public operation
//! returns, from the error it returns and the profile, after every retry:
//! it is the returned error's by construction, and gathering it can't
//! change the outcome.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};

use crate::budget::Work;
use crate::extrude::Frame;
use crate::failure::{Evidence, evidence_work};
use crate::patch::{Conic2, Conic3};
use crate::profile::{Profile, ProfileError, Segment};
use crate::{Failure, KernelError, in_range};

/// The work units finding where two segments come nearest takes: a grid
/// of [`GRID`]² pairs of points, then [`REFINE`] rounds of 25, a unit
/// each.
const NEAREST_WORK: usize = (GRID + 1) * (GRID + 1) + REFINE * 25;

/// The work units finding where a segment comes nearest the axis takes,
/// as [`NEAREST_WORK`] on one parameter.
pub(crate) const AXIS_WORK: usize = GRID + 1 + REFINE * 5;

/// The intervals each segment is cut into for the first search of where
/// two come nearest, or where one comes nearest the axis.
const GRID: usize = 32;

/// Rounds of narrowing the search round the best pair found, each
/// halving the window.
const REFINE: usize = 48;

/// Profile segments scanned per work unit where a whole profile is only
/// looked through (the axis's extent), not placed.
const SCAN_PER_UNIT: usize = 16;

/// `error`, failing `profile` placed on `frame`, with its evidence.
pub(crate) fn profile_failure(error: KernelError, profile: &Profile, frame: &Frame) -> Failure {
    let KernelError::Profile(e) = error else {
        return error.into();
    };
    let mut gather = Gather::new(profile, frame);
    gather.error(e);
    gather.failure(error)
}

/// Evidence of a profile's error being gathered from its own allowance.
pub(crate) struct Gather<'a> {
    profile: &'a Profile,
    /// The frame the profile lies on, if it is fit to place by
    /// ([`Frame::check`]): a profile is checked before its frame, so its
    /// errors may come with one that isn't. Without it only the sketch
    /// curves are given.
    frame: Option<Frame>,
    evidence: Evidence,
    work: Work,
    /// The sketch curves given so far, each given once.
    seen: BTreeSet<u64>,
}

impl<'a> Gather<'a> {
    pub(crate) fn new(profile: &'a Profile, frame: &Frame) -> Gather<'a> {
        Gather {
            profile,
            frame: frame.check().ok().map(|()| *frame),
            evidence: Evidence::default(),
            work: evidence_work(),
            seen: BTreeSet::new(),
        }
    }

    /// The failure of `error` with what was gathered.
    pub(crate) fn failure(self, error: KernelError) -> Failure {
        Failure {
            error,
            evidence: Box::new(self.evidence),
        }
    }

    /// The frame placing the profile, if it is fit to.
    pub(crate) fn frame(&self) -> Option<Frame> {
        self.frame
    }

    /// Segment `s` of loop `l`, if there is one.
    pub(crate) fn get(&self, l: usize, s: usize) -> Option<&'a Segment> {
        self.profile.loops.get(l)?.segments.get(s)
    }

    /// The segment before segment `s` of loop `l` in its loop, or after
    /// it (`after`), as `(loop, segment)`.
    fn beside(&self, l: usize, s: usize, after: bool) -> Option<(usize, usize)> {
        let n = self.profile.loops.get(l)?.segments.len();
        if s >= n {
            return None;
        }
        let other = if after { (s + 1) % n } else { (s + n - 1) % n };
        Some((l, other))
    }

    /// Takes `units` of the allowance, or says no (and marks the evidence
    /// truncated) once it is spent.
    pub(crate) fn afford(&mut self, units: usize) -> bool {
        self.evidence.afford(&mut self.work, units)
    }

    /// The evidence of `error`, the variants only a revolve gives as far
    /// as they go without its axis and turn: the segment named, or the
    /// whole profile.
    pub(crate) fn error(&mut self, error: ProfileError) {
        match error {
            ProfileError::Empty => {}
            ProfileError::TooManySegments(_)
            | ProfileError::Triangulation
            | ProfileError::NearlyFullTurn => self.all(),
            ProfileError::Short(l) | ProfileError::Area(l) | ProfileError::Nesting(l) => {
                self.whole_loop(l);
            }
            ProfileError::Segment(l, s, _)
            | ProfileError::Degenerate(l, s)
            | ProfileError::TooFine(l, s)
            | ProfileError::CrossesAxis(l, s) => {
                self.segment(l, s);
            }
            ProfileError::Open(l, s) => {
                let next = self.beside(l, s, true);
                if let (Some(a), Some((ln, sn))) = (self.get(l, s), next) {
                    let b = self.get(ln, sn).expect("in its loop");
                    let (end, start) = (a.conic.p1, b.conic.p0);
                    self.points([end, start]);
                }
                self.segment(l, s);
                if let Some((ln, sn)) = next {
                    self.segment(ln, sn);
                }
            }
            ProfileError::Cusp(l, s) | ProfileError::TouchesAxis(l, s) => self.vertex(l, s),
            ProfileError::Touching([(la, sa), (lb, sb)]) => self.touching((la, sa), (lb, sb)),
        }
    }

    /// The vertex where segment `s` of loop `l` starts, and the segments
    /// either side of it.
    pub(crate) fn vertex(&mut self, l: usize, s: usize) {
        let before = self.beside(l, s, false);
        if let Some(seg) = self.get(l, s) {
            self.points([seg.conic.p0]);
        }
        if let Some((lb, sb)) = before {
            self.segment(lb, sb);
        }
        self.segment(l, s);
    }

    /// Two segments that touch or cross, and where they come nearest:
    /// one point if that is one point (to within about a billionth of
    /// its distance from the origin), else both.
    fn touching(&mut self, a: (usize, usize), b: (usize, usize)) {
        if a != b
            && let (Some(sa), Some(sb)) = (self.get(a.0, a.1), self.get(b.0, b.1))
            && self.afford(NEAREST_WORK)
        {
            let (pa, pb) = nearest(&sa.conic, &sb.conic);
            let scale = pa.abs().max(pb.abs()).max_element().max(1.0);
            if pa.distance(pb) <= 1e-9 * scale {
                self.points([(pa + pb) * 0.5]);
            } else {
                self.points([pa, pb]);
            }
        }
        self.segment(a.0, a.1);
        if a != b {
            self.segment(b.0, b.1);
        }
    }

    /// Every segment of loop `l`, in order.
    pub(crate) fn whole_loop(&mut self, l: usize) {
        let n = self.profile.loops.get(l).map_or(0, |lp| lp.segments.len());
        for s in 0..n {
            if !self.segment(l, s) {
                return;
            }
        }
    }

    /// Every segment of the profile, loop by loop.
    pub(crate) fn all(&mut self) {
        let frame = self.frame;
        self.all_on(frame.as_ref());
    }

    /// Every segment of the profile placed on `frame` (none: only their
    /// sketch curves), loop by loop, as far as the allowance and the caps
    /// go.
    pub(crate) fn all_on(&mut self, frame: Option<&Frame>) {
        let profile = self.profile;
        for (l, lp) in profile.loops.iter().enumerate() {
            for s in 0..lp.segments.len() {
                if !self.segment_on(frame, l, s) {
                    return;
                }
            }
        }
    }

    /// Segment `s` of loop `l`: its curve placed by the frame, if it has
    /// one and the placed curve is fit to draw (in range, passing
    /// [`Conic3::new`]), and its sketch curve. Whether to go on: false
    /// once the allowance is spent or the curves and sketch curves are
    /// both full.
    pub(crate) fn segment(&mut self, l: usize, s: usize) -> bool {
        let frame = self.frame;
        self.segment_on(frame.as_ref(), l, s)
    }

    fn segment_on(&mut self, frame: Option<&Frame>, l: usize, s: usize) -> bool {
        let caps = crate::failure::MAX_EVIDENCE;
        let full = self.evidence.curves.len() >= caps.curves
            && self.evidence.sketch_curves.len() >= caps.sketch_curves;
        if full {
            self.evidence.truncated = true;
            return false;
        }
        if !self.afford(1) {
            return false;
        }
        let Some(seg) = self.get(l, s) else {
            return true;
        };
        if let Some(curve) = frame.and_then(|f| place(f, &seg.conic)) {
            self.evidence.add_curves([curve]);
        }
        if self.seen.insert(seg.curve) {
            self.evidence.add_sketch_curves([seg.curve]);
        }
        true
    }

    /// Points of the profile's plane, placed by its frame; those that
    /// land out of range are left out.
    pub(crate) fn points(&mut self, points: impl IntoIterator<Item = DVec2>) {
        let Some(frame) = self.frame else {
            return;
        };
        let placed: Vec<DVec3> = points
            .into_iter()
            .map(|p| frame.point(p, 0.0))
            .filter(|&p| in_range(p).is_ok())
            .collect();
        self.evidence.add_points(placed);
    }

    /// The straight curve from `a` to `b` of the profile's plane, placed
    /// by its frame, if it is fit to draw.
    pub(crate) fn line(&mut self, a: DVec2, b: DVec2) {
        let Some(frame) = self.frame else {
            return;
        };
        if let Some(line) = Conic2::line(a, b).ok().and_then(|c| place(&frame, &c)) {
            self.evidence.add_curves([line]);
        }
    }

    /// The least and greatest `y` of the profile's control points, if it
    /// has any and the allowance covers looking: the extent along a
    /// revolve's axis.
    pub(crate) fn extent_along_y(&mut self) -> Option<(f64, f64)> {
        let n = self.profile.segment_count();
        if !self.afford(n.div_ceil(SCAN_PER_UNIT)) {
            return None;
        }
        let ys = self
            .profile
            .loops
            .iter()
            .flat_map(|lp| &lp.segments)
            .flat_map(|seg| [seg.conic.p0.y, seg.conic.c.y, seg.conic.p1.y])
            .filter(|y| y.is_finite());
        ys.fold(None, |range, y| match range {
            None => Some((y, y)),
            Some((lo, hi)) => Some((f64::min(lo, y), f64::max(hi, y))),
        })
    }
}

/// `conic`, of a profile's plane, placed on `frame` at height 0, if every
/// control point lands in range and it passes [`Conic3::new`].
fn place(frame: &Frame, conic: &Conic2) -> Option<Conic3> {
    let [p0, c, p1] = [conic.p0, conic.c, conic.p1].map(|p| frame.point(p, 0.0));
    if [p0, c, p1].iter().any(|&p| in_range(p).is_err()) {
        return None;
    }
    Conic3::new(p0, c, conic.w, p1).ok()
}

/// About where `a` and `b` come nearest: the closest pair of points on a
/// grid of each one's parameter, then that pair's window narrowed round
/// the best pair, [`REFINE`] times. Not exact (two segments may come near
/// in more than one place, and the grid may pick the wrong one), but
/// deterministic and on the segments.
pub(crate) fn nearest(a: &Conic2, b: &Conic2) -> (DVec2, DVec2) {
    let d = |s: f64, t: f64| a.eval(s).distance_squared(b.eval(t));
    let step = 1.0 / GRID as f64;
    let mut best = (0.0, 0.0, f64::INFINITY);
    for i in 0..=GRID {
        let s = i as f64 * step;
        for j in 0..=GRID {
            let t = j as f64 * step;
            let e = d(s, t);
            if e < best.2 {
                best = (s, t, e);
            }
        }
    }
    let mut h = step;
    for _ in 0..REFINE {
        let (s0, t0, _) = best;
        for i in -2..=2 {
            let s = (s0 + f64::from(i) * h * 0.5).clamp(0.0, 1.0);
            for j in -2..=2 {
                let t = (t0 + f64::from(j) * h * 0.5).clamp(0.0, 1.0);
                let e = d(s, t);
                if e < best.2 {
                    best = (s, t, e);
                }
            }
        }
        h *= 0.5;
    }
    (a.eval(best.0), b.eval(best.1))
}

/// About where `conic` comes nearest the line `x = 0`, its `x` being at
/// least 0: as [`nearest`], on one parameter.
pub(crate) fn nearest_axis(conic: &Conic2) -> DVec2 {
    let x = |t: f64| conic.eval(t).x.abs();
    let step = 1.0 / GRID as f64;
    let mut best = (0.0, f64::INFINITY);
    for i in 0..=GRID {
        let t = i as f64 * step;
        let e = x(t);
        if e < best.1 {
            best = (t, e);
        }
    }
    let mut h = step;
    for _ in 0..REFINE {
        let t0 = best.0;
        for i in -2..=2 {
            let t = (t0 + f64::from(i) * h * 0.5).clamp(0.0, 1.0);
            let e = x(t);
            if e < best.1 {
                best = (t, e);
            }
        }
        h *= 0.5;
    }
    conic.eval(best.0)
}

#[cfg(test)]
mod tests;
