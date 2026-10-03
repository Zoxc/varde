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

use crate::budget::{Budget, Work};
use crate::extrude::Frame;
use crate::failure::{Evidence, MAX_EVIDENCE, evidence_work};
use crate::measure::{curves_distance, turns};
use crate::patch::{Conic2, Conic3};
use crate::profile::{Profile, ProfileError, Segment};
use crate::{Failure, KernelError, Tolerance, in_range};

/// The work units, out of the allowance, of finding where two segments
/// come nearest ([`curves_distance`]): well over the hundred or so that
/// arcs crossing, touching or nested take.
const NEAREST_WORK: usize = 1 << 12;

/// Profile segments scanned per work unit where a whole profile is only
/// looked through (the axis's extent: up to four evaluations each), not
/// placed.
const SCAN_PER_UNIT: usize = 4;

/// `failure`, of `profile` placed on `frame` at `tol`, with its
/// evidence: a profile's error with what it names of the profile (the
/// steps raising one hand up no evidence), any other as it came (with
/// the evidence of the step that raised it, if any).
pub(crate) fn profile_failure(
    failure: Failure,
    profile: &Profile,
    frame: &Frame,
    tol: &Tolerance,
) -> Failure {
    let error = failure.error;
    let KernelError::Profile(e) = error else {
        return failure;
    };
    let mut gather = Gather::new(profile, frame, tol);
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
    /// The operation's resolution: two segments nearer are one point.
    resolution: f64,
    evidence: Evidence,
    work: Work,
    /// The sketch curves given so far, each given once.
    seen: BTreeSet<u64>,
}

impl<'a> Gather<'a> {
    pub(crate) fn new(profile: &'a Profile, frame: &Frame, tol: &Tolerance) -> Gather<'a> {
        Gather {
            profile,
            frame: frame.check().ok().map(|()| *frame),
            resolution: tol.resolution(),
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

    /// Two segments that touch or cross, and where they come nearest
    /// ([`curves_distance`], from [`NEAREST_WORK`] of the allowance):
    /// one point half way between where that is within the resolution
    /// (they touch, as far as the operation can tell), else the point on
    /// each.
    fn touching(&mut self, a: (usize, usize), b: (usize, usize)) {
        let placed = |at: (usize, usize)| place(self.frame.as_ref()?, &self.get(at.0, at.1)?.conic);
        if a != b
            && let (Some(ca), Some(cb)) = (placed(a), placed(b))
            && self.afford(NEAREST_WORK)
        {
            let mut work = Work::new(&Budget::new(NEAREST_WORK as u64));
            match curves_distance(&ca, &cb, self.resolution, &mut work) {
                Ok(d) if d.distance <= self.resolution => {
                    let [p, q] = d.points;
                    self.evidence.add_points([(p + q) * 0.5]);
                }
                Ok(d) => self.evidence.add_points(d.points),
                Err(_) => self.evidence.truncated = true,
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
        let caps = MAX_EVIDENCE;
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

    /// The least and greatest `y` of the profile, if it has any and the
    /// allowance covers looking: the extent along a revolve's axis. Each
    /// segment's own, at its ends and where its `y` turns ([`turns`]):
    /// its control points may reach past it.
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
            .flat_map(|seg| {
                let c = &seg.conic;
                [0.0, 1.0]
                    .into_iter()
                    .chain(turns([c.p0.y, c.c.y, c.p1.y], c.w))
                    .map(|t| c.eval(t).y)
            })
            .filter(|y| y.is_finite());
        ys.fold(None, |range, y| match range {
            None => Some((y, y)),
            Some((lo, hi)) => Some((f64::min(lo, y), f64::max(hi, y))),
        })
    }
}

/// `conic`, of a profile's plane, placed on `frame` at height 0 (in the
/// sketch's own plane, where its curves are mended), if every control
/// point lands in range and it is a fit conic ([`Conic3::check`]).
fn place(frame: &Frame, conic: &Conic2) -> Option<Conic3> {
    let placed = frame.conic(conic, 0.0);
    let in_range = placed.hull().iter().all(|&p| in_range(p).is_ok());
    (in_range && placed.check().is_ok()).then_some(placed)
}

/// Where `conic`, not crossing the line `x = 0`, comes nearest it: an
/// end, or where its `x` turns ([`turns`]), whichever is nearest (the
/// first of those as near).
pub(crate) fn nearest_axis(conic: &Conic2) -> DVec2 {
    let x = [conic.p0.x, conic.c.x, conic.p1.x];
    [0.0, 1.0]
        .into_iter()
        .chain(turns(x, conic.w))
        .map(|t| conic.eval(t))
        .min_by(|p, q| p.x.abs().total_cmp(&q.x.abs()))
        .expect("the ends")
}

#[cfg(test)]
mod tests;
