//! Refining the plane faces a boolean cut for their triangles' shapes.
//!
//! Each input triangle is cut on its own and its loops ear-clipped on
//! their vertices only, so a box's cap cut by a hole is fanned from its
//! far corners to the rim: triangles 19 mm long and a few tenths wide.
//! The next hole in line with the first passes along their long sides,
//! within micrometres, where no split mends the band between. So, last
//! in the clean-up, the triangles this boolean made on plane faces whose
//! narrowest angle is under 5° are refined, a Delaunay refinement
//! (Ruppert's) on the whole face in 3D:
//!
//! - a vertex inside a plane face at such a triangle's corner (a seam's,
//!   left inside a cap where two flush caps became one) is taken out,
//!   its star triangulated again on its boundary, where that is better
//!   shaped;
//! - the made triangles' free sides are flipped towards Delaunay;
//! - then, worst first, a triangle of straight sides takes a point at its
//!   circumcentre; one with a curved side has its longest free side
//!   halved where the narrow corner is across the curve (a fan onto a
//!   rim), or the straight side at a narrow corner on the curve flipped
//!   (the rims are never split here); a point that would land beyond, or
//!   within the diametral circle of, a straight side between two plane
//!   faces (a cap's edge with a wall) halves that side instead, in both
//!   faces. After each change the sides facing the new vertex are
//!   flipped towards Delaunay.
//!
//! Every change keeps the region: a point goes strictly inside a proper
//! triangle, a side is halved at a point on it, a flip replaces two
//! triangles of one plane by two covering the same quadrilateral, a star
//! is triangulated again on its own boundary, and each new triangle is
//! proper (higher than the short length, facing along its plane's normal,
//! not folded) with its curved corners open. A curve whose corner is
//! closed may leave its triangle, bulging over those beyond: no point
//! goes near one. It is sequential, in a total order, and stops after a
//! number of points set by the triangles made: prevention for the next
//! operation, not a condition of this one.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use glam::{DVec2, DVec3};

use super::super::triangulate::{Meter, in_circle};
use super::Cleaner;
use super::seams::{REGION_STEPS, STEPS_PER_UNIT};
use crate::KernelError;
use crate::budget::Work;
use crate::mesh::{MIN_SPLIT, SIN_SHAPE, circumcentre_from};

/// The sine of the smallest angle a triangle made on a plane face may
/// have before it is refined: sin 5°, as for the shapes of cut faces on
/// curved patches.
pub(in crate::boolean) const QUALITY_SIN: f64 = SIN_SHAPE;

/// Two constrained sides meeting at an angle whose cosine is above this
/// (under 60°) exempt their corner: no point mends an angle the face's
/// boundary makes, and refining it never ends.
const SHARP_COS: f64 = 0.5;

/// How many points the pass may add: so many per triangle made, and so
/// many more.
const QUALITY_POINTS: usize = 4;
const QUALITY_MORE: usize = 64;

/// The sine of the narrowest angle a triangle the pass makes must
/// exceed: well above where rounding can't tell a sliver from a fold
/// ([`FOLD_FLOOR`](crate::patch::FOLD_FLOOR)). Halving a fan's side along
/// the common tangent of two rims made triangles of three corners on that
/// line, which failed the fold check.
const MIN_SINE: f64 = 1e-6;

/// How many triangles a walk to a point passes at most.
const WALK: usize = 4096;

/// How many sides the flips after one change may flip at most.
const FLIPS: usize = 256;

/// What the pass keeps away from: `least` is the smallest circumradius
/// refined and the smallest distance a point keeps from a side or a
/// corner, and `bulges` the boxes (grown by `least`) of the control
/// triangles of the curved sides of triangles on plane faces whose curved
/// corner is closed. Such a curve may leave its triangle, bulging over
/// those beyond, where a point could land between it and its chord,
/// outside the face: no point goes in one of these boxes.
struct Near {
    least: f64,
    bulges: Vec<(DVec3, DVec3)>,
}

impl Near {
    /// Whether `p` lies in one of the boxes of [`Near::bulges`].
    fn in_bulge(&self, p: DVec3) -> bool {
        self.bulges
            .iter()
            .any(|(lo, hi)| lo.cmple(p).all() && p.cmple(*hi).all())
    }
}

/// Where a point lies, walking to it across free sides.
enum Spot {
    /// Inside this triangle, more than the least length from its sides.
    Inside(u32),
    /// Within the least length of this side (from corner `i`) of this
    /// triangle, its foot inside the side.
    OnSide(u32, usize),
    /// Beyond this constrained side of this triangle.
    Beyond(u32, usize),
    /// Near a corner, or the walk lost its way.
    Nowhere,
}

/// An entry in the queue of bad triangles: its sine's bits (positive,
/// so ordered as the sines are), its corners sorted, the triangle and its
/// corners as they were. The worst first.
type Entry = Reverse<(u64, [u32; 3], u32, [u32; 3])>;

impl Cleaner<'_> {
    /// Refines the triangles made on plane faces whose narrowest angle is
    /// under 5°: see the [module](self) docs. `least` is the smallest
    /// circumradius refined and the smallest distance a point keeps from
    /// a side or a corner: [`MIN_SPLIT`] resolutions, the smallest piece
    /// repair splits.
    pub(super) fn quality(&mut self, resolution: f64, work: &mut Work) -> Result<(), KernelError> {
        let least = MIN_SPLIT * resolution;
        work.spend(self.soup.tris.len())?;
        // Triangles with a curved corner closed: flipped open where they
        // can be, else their curves may leave them, bulging over the
        // triangles beyond, and no point may come near those curves.
        let mut closed = Vec::new();
        let mut made = 0usize;
        for t in 0..self.soup.tris.len() {
            if !self.alive[t] {
                continue;
            }
            made += usize::from(self.soup.made[t]);
            let on_plane = self.planes[self.soup.faces[t] as usize].is_some();
            if on_plane
                && !self.open(self.soup.tris[t])
                && !(self.soup.made[t] && self.open_by_flip(t as u32))
            {
                closed.push(t as u32);
            }
        }
        let bulges = closed
            .iter()
            .flat_map(|&t| self.bulges(self.soup.tris[t as usize]))
            .map(|(lo, hi)| (lo - DVec3::splat(least), hi + DVec3::splat(least)))
            .collect();
        let near = Near { least, bulges };
        // Vertices inside a plane face at a bad triangle's corner (a
        // seam's, left in a cap two flush caps made one) taken out where
        // the star triangulated again on its link is better shaped.
        let mut inner: Vec<u32> = (0..self.soup.tris.len() as u32)
            .filter(|&t| self.bad(t, least).is_some())
            .flat_map(|t| self.soup.tris[t as usize])
            .collect();
        inner.sort_unstable();
        inner.dedup();
        for v in inner {
            self.remove_inner(v, work)?;
        }
        let most = made
            .saturating_mul(QUALITY_POINTS)
            .saturating_add(QUALITY_MORE);
        let mut steps = 0usize;
        self.delaunay_made(most, &mut steps);
        let mut queue: BinaryHeap<Entry> = BinaryHeap::new();
        for t in 0..self.soup.tris.len() as u32 {
            self.judge(&mut queue, t, least);
        }
        let first = self.soup.pos.len();
        while self.soup.pos.len() - first < most {
            work.spend(std::mem::take(&mut steps))?;
            let Some(Reverse((_, _, t, tri))) = queue.pop() else {
                break;
            };
            steps += 1;
            if !self.alive[t as usize] || self.soup.tris[t as usize] != tri {
                continue;
            }
            let mut changed = Vec::new();
            self.refine(t, &near, &mut changed, &mut steps);
            changed.sort_unstable();
            changed.dedup();
            steps = steps.saturating_add(changed.len());
            for s in changed {
                self.judge(&mut queue, s, least);
            }
        }
        // The triangles across closed corners may have changed: tried again.
        for t in closed {
            if self.alive[t as usize]
                && self.soup.made[t as usize]
                && !self.open(self.soup.tris[t as usize])
            {
                self.open_by_flip(t);
            }
        }
        work.spend(steps)
    }

    /// Queues triangle `t` if it is bad: made by this boolean (or a piece
    /// of one), alive, on a plane face, proper, every side longer than
    /// the short length, its circumcircle at least `least` in radius, and
    /// the sine of its narrowest corner ([`Self::corner_sines`]) under
    /// [`QUALITY_SIN`], but not where some corner lies between two
    /// constrained sides meeting at under 60°.
    fn judge(&self, queue: &mut BinaryHeap<Entry>, t: u32, least: f64) {
        if let Some(sine) = self.bad(t, least) {
            let tri = self.soup.tris[t as usize];
            let mut sorted = tri;
            sorted.sort_unstable();
            queue.push(Reverse((sine.to_bits(), sorted, t, tri)));
        }
    }

    /// The sine of triangle `t`'s narrowest angle if it is bad (see
    /// [`Self::judge`]).
    fn bad(&self, t: u32, least: f64) -> Option<f64> {
        let i = t as usize;
        let face = self.soup.faces[i] as usize;
        if !self.alive[i] || !self.soup.made[i] {
            return None;
        }
        let (up, _) = self.planes[face]?;
        let tri = self.soup.tris[i];
        let side = |k: usize| self.p(tri[(k + 1) % 3]) - self.p(tri[k]);
        let lengths = [0, 1, 2].map(|k| side(k).length());
        let normal = self.normal(tri);
        let twice = normal.length();
        if lengths.iter().any(|&l| l.is_nan() || l <= self.small)
            || normal.dot(up) <= 0.0
            || self.height(tri).0 <= self.small
        {
            return None;
        }
        let radius = lengths[0] * lengths[1] * lengths[2] / (2.0 * twice);
        if radius.is_nan() || radius < least {
            return None;
        }
        let sine = self
            .corner_sines(tri, up)
            .into_iter()
            .fold(f64::INFINITY, f64::min);
        if sine.is_nan() || sine >= QUALITY_SIN {
            return None;
        }
        // A corner between two constrained sides meeting at under 60°: the
        // face's boundary makes the angle (where a rim runs on from a
        // straight edge at a tangent, a fraction of a degree), and the
        // points and halvings meant to mend it only step towards that
        // corner, each half the last, and spread to the faces beside.
        let constrained = |k: usize| self.free(t, tri[k], tri[(k + 1) % 3]).is_none();
        let sharp = (0..3).any(|j| {
            let (out, back) = (j, (j + 2) % 3);
            constrained(out)
                && constrained(back)
                && side(out).dot(-side(back)) > SHARP_COS * lengths[out] * lengths[back]
        });
        (!sharp).then_some(sine)
    }

    /// Takes out vertex `v`, if it lies inside a plane face (every
    /// triangle round it on that face, every edge from it straight) and
    /// triangulating its star again on the star's boundary (as the seams'
    /// regions are, [`Self::retriangulate`]) makes the star's narrowest
    /// triangle wider. The region stays as it was: the new triangles cover the star's
    /// boundary polygon once. Gives whether it did.
    fn remove_inner(&mut self, v: u32, work: &mut Work) -> Result<bool, KernelError> {
        if !self.inside_plane(v) {
            return Ok(false);
        }
        let mut star = self.around[v as usize].clone();
        star.sort_unstable();
        let face = self.soup.faces[star[0] as usize];
        let Some(plane) = self.planes[face as usize] else {
            return Ok(false);
        };
        let before = star
            .iter()
            .map(|&t| self.narrowest(self.soup.tris[t as usize]))
            .fold(f64::INFINITY, f64::min);
        // The records of the edges from `v`, which the triangulation
        // takes out, put back if the star is kept.
        let saved: Vec<_> = self
            .neighbours(v)
            .into_iter()
            .filter_map(|w| {
                let k = (v.min(w), v.max(w));
                self.soup.curves.get(&k).map(|e| (k, *e))
            })
            .collect();
        work.spend(star.len())?;
        let meter = Meter::new(work.left().min(REGION_STEPS / STEPS_PER_UNIT) * STEPS_PER_UNIT);
        let made = self.retriangulate(&star, plane, &meter);
        work.spend(usize::try_from(meter.used() / STEPS_PER_UNIT).unwrap_or(usize::MAX))?;
        let Some(made) = made else {
            return Ok(false);
        };
        let after = made
            .iter()
            .map(|&tri| self.narrowest(tri))
            .fold(f64::INFINITY, f64::min);
        if after.is_nan() || before.is_nan() || after <= before {
            self.soup.curves.extend(saved);
            return Ok(false);
        }
        self.replace(&star, made);
        Ok(true)
    }

    /// Flips a free side of triangle `t`, on a plane face, whose curved
    /// corner is closed, where both new triangles are proper with their
    /// curved corners open: a fan from a far corner whose side grazes a
    /// rim (along the common tangent of two holes) has its corner at the
    /// rim closed, which no split mends; the triangle across that side
    /// gives the rim another corner to join. Gives whether it flipped.
    fn open_by_flip(&mut self, t: u32) -> bool {
        let tri = self.soup.tris[t as usize];
        let face = self.soup.faces[t as usize];
        for k in 0..3 {
            let (u, v, a) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
            let Some(s) = self.free(t, u, v) else {
                continue;
            };
            let b = self.third(s, u, v);
            if a == b || !self.shared(a, b).is_empty() {
                continue;
            }
            let (n1, n2) = ([a, u, b], [a, b, v]);
            if !(self.proper_on(n1, face) && self.proper_on(n2, face)) {
                continue;
            }
            self.swap(t, s, [u, v], a, b);
            self.soup.curves.remove(&(a.min(b), a.max(b)));
            return true;
        }
        false
    }

    /// The triangle across side `u → v` of triangle `t` if the side is
    /// free: straight, and between `t` and one other triangle on the same
    /// face.
    fn free(&self, t: u32, u: u32, v: u32) -> Option<u32> {
        if self.curved(u, v) {
            return None;
        }
        self.pair(t, u, v)
            .filter(|&s| self.soup.faces[s as usize] == self.soup.faces[t as usize])
    }

    /// The triangle across side `u → v` of triangle `t`, where the two are
    /// the only ones on that side.
    fn pair(&self, t: u32, u: u32, v: u32) -> Option<u32> {
        let shared = self.shared(u, v);
        if shared.len() != 2 {
            return None;
        }
        self.across(t, u, v)
    }

    /// Whether `tri` is proper on face `face`: higher than the short
    /// length, facing along its plane's normal, no narrower than
    /// [`MIN_SINE`], its curved corners open.
    fn proper_on(&self, tri: [u32; 3], face: u32) -> bool {
        let Some((up, _)) = self.planes[face as usize] else {
            return false;
        };
        self.normal(tri).dot(up) > 0.0
            && self.height(tri).0 > self.small
            && self.narrowest(tri) > MIN_SINE
            && self.open(tri)
            && self
                .curved_patch(tri)
                .is_none_or(|patch| patch.fold_direction().is_some())
    }

    /// One step for the bad triangle `t`: a point at its circumcentre, or a
    /// side halved (see the [module](self) docs). The triangles it
    /// changed or made go in `changed`, and `steps` counts the work.
    fn refine(&mut self, t: u32, near: &Near, changed: &mut Vec<u32>, steps: &mut usize) {
        let least = near.least;
        let tri = self.soup.tris[t as usize];
        let face = self.soup.faces[t as usize];
        if (0..3).any(|k| self.curved(tri[k], tri[(k + 1) % 3])) {
            let Some((up, _)) = self.planes[face as usize] else {
                return;
            };
            let sines = self.corner_sines(tri, up);
            let k = (0..3)
                .min_by(|&a, &b| sines[a].total_cmp(&sines[b]))
                .expect("three corners");
            let (out, back) = (k, (k + 2) % 3);
            let curved_side = |k: usize| self.curved(tri[k], tri[(k + 1) % 3]);
            if curved_side(out) || curved_side(back) {
                // A narrow corner at a curve's end, between the curve and
                // a straight side leaving along it: no point in the
                // triangle mends that, but flipping the straight side
                // gives the curve another corner to meet.
                let straight = if curved_side(out) { back } else { out };
                let (u, v) = (tri[straight], tri[(straight + 1) % 3]);
                if let Some(s) = self.flip_better(t, u, v, up) {
                    changed.extend([t, s]);
                }
                return;
            }
            // A fan onto a curve: its longest free straight side halved,
            // which brings the far corner nearer.
            let lengths = [0, 1, 2].map(|k| self.p(tri[(k + 1) % 3]).distance(self.p(tri[k])));
            let longest = (0..3)
                .filter(|&k| self.free(t, tri[k], tri[(k + 1) % 3]).is_some())
                .max_by(|&a, &b| lengths[a].total_cmp(&lengths[b]).then(b.cmp(&a)));
            if let Some(k) = longest
                && lengths[k] > 2.0 * least
            {
                let (u, v) = (tri[k], tri[(k + 1) % 3]);
                self.halve_free(t, u, v, (self.p(u) + self.p(v)) * 0.5, near, changed, steps);
            }
            return;
        }
        let Some((up, d)) = self.planes[face as usize] else {
            return;
        };
        // The circumcentre, in the plane of the face.
        let (e1, e2) = frame(up);
        let o = self.p(tri[0]);
        let flat = |v: u32| {
            let p = self.p(v) - o;
            DVec2::new(p.dot(e1), p.dot(e2))
        };
        let (a, b, c) = (flat(tri[0]), flat(tri[1]), flat(tri[2]));
        let centre = a + circumcentre_from(a, b, c);
        let q = o + e1 * centre.x + e2 * centre.y;
        let q = q - up * (up.dot(q) - d);
        if !q.is_finite() {
            return;
        }
        match self.walk(t, q, up, least, steps) {
            Spot::Inside(h) => {
                if let Some((g, k)) = self.encroached(&[h], q) {
                    self.halve_constrained(g, k, near, changed, steps);
                } else {
                    self.insert(h, q, near, changed, steps);
                }
            }
            Spot::OnSide(h, k) => {
                let corners = self.soup.tris[h as usize];
                let (u, v) = (corners[k], corners[(k + 1) % 3]);
                if self.free(h, u, v).is_some() {
                    let (pu, pv) = (self.p(u), self.p(v));
                    let along = pv - pu;
                    let s = (q - pu).dot(along) / along.length_squared();
                    self.halve_free(h, u, v, pu + along * s, near, changed, steps);
                } else {
                    self.halve_constrained(h, k, near, changed, steps);
                }
            }
            Spot::Beyond(h, k) => self.halve_constrained(h, k, near, changed, steps),
            Spot::Nowhere => {}
        }
    }

    /// A straight constrained side of one of the triangles `tris` whose
    /// diametral circle holds `q` (as the triangle and the side's index):
    /// a point there would leave a thin triangle on that side, which only
    /// halving the side mends (Ruppert's encroachment).
    fn encroached(&self, tris: &[u32], q: DVec3) -> Option<(u32, usize)> {
        tris.iter().find_map(|&t| {
            let tri = self.soup.tris[t as usize];
            (0..3)
                .find(|&k| {
                    let (u, v) = (tri[k], tri[(k + 1) % 3]);
                    let (pu, pv) = (self.p(u), self.p(v));
                    !self.curved(u, v)
                        && self.free(t, u, v).is_none()
                        && q.distance((pu + pv) * 0.5) < pu.distance(pv) * 0.5
                })
                .map(|k| (t, k))
        })
    }

    /// Halves the free side `u → v` of triangle `t` at `at`, or where that
    /// lies within the diametral circle of a straight constrained side of
    /// either triangle beside it, halves that side instead.
    #[allow(clippy::too_many_arguments)]
    fn halve_free(
        &mut self,
        t: u32,
        u: u32,
        v: u32,
        at: DVec3,
        near: &Near,
        changed: &mut Vec<u32>,
        steps: &mut usize,
    ) {
        let Some(s) = self.free(t, u, v) else {
            return;
        };
        match self.encroached(&[t, s], at) {
            Some((g, k)) => self.halve_constrained(g, k, near, changed, steps),
            None => self.halve(t, u, v, at, near, changed, steps),
        }
    }

    /// Walks from triangle `t` towards `q`, across free sides only.
    fn walk(&self, t: u32, q: DVec3, up: DVec3, least: f64, steps: &mut usize) -> Spot {
        let mut at = t;
        for _ in 0..WALK {
            *steps += 1;
            let tri = self.soup.tris[at as usize];
            // Signed distances from each side's line, positive inside.
            let dist = [0, 1, 2].map(|k| {
                let (pu, pv) = (self.p(tri[k]), self.p(tri[(k + 1) % 3]));
                let along = pv - pu;
                along.cross(q - pu).dot(up) / along.length()
            });
            let k = (0..3)
                .min_by(|&a, &b| dist[a].total_cmp(&dist[b]))
                .expect("three sides");
            if dist[k] > least {
                return Spot::Inside(at);
            }
            if !dist[k].is_finite() {
                return Spot::Nowhere;
            }
            if dist[k] >= -least {
                // Near this side's line: on it, if its foot is inside the
                // side and away from the other sides.
                let (pu, pv) = (self.p(tri[k]), self.p(tri[(k + 1) % 3]));
                let along = pv - pu;
                let s = (q - pu).dot(along) / along.length();
                let near_others = (0..3).any(|j| j != k && dist[j] <= least);
                return if s > least && s < along.length() - least && !near_others {
                    Spot::OnSide(at, k)
                } else {
                    Spot::Nowhere
                };
            }
            let (u, v) = (tri[k], tri[(k + 1) % 3]);
            match self.free(at, u, v) {
                Some(s) => at = s,
                None => return Spot::Beyond(at, k),
            }
        }
        Spot::Nowhere
    }

    /// Halves side `k` of triangle `t`, which isn't free, at its middle:
    /// where it is straight and longer than twice `least`, with the
    /// triangle across it the only one there, on a plane face too (a
    /// cap's edge with a wall).
    fn halve_constrained(
        &mut self,
        t: u32,
        k: usize,
        near: &Near,
        changed: &mut Vec<u32>,
        steps: &mut usize,
    ) {
        let least = near.least;
        let tri = self.soup.tris[t as usize];
        let (u, v) = (tri[k], tri[(k + 1) % 3]);
        let (pu, pv) = (self.p(u), self.p(v));
        let length = pu.distance(pv);
        if self.curved(u, v) || length.is_nan() || length <= 2.0 * least {
            return;
        }
        let Some(s) = self.pair(t, u, v) else {
            return;
        };
        if self.planes[self.soup.faces[s as usize] as usize].is_none() {
            return;
        }
        self.halve(t, u, v, (pu + pv) * 0.5, near, changed, steps);
    }

    /// Splits the straight side `u → v` of triangle `t` at `at`, both
    /// triangles beside it in two, if every piece is proper on its face;
    /// then flips the sides facing the new vertex.
    #[allow(clippy::too_many_arguments)]
    fn halve(
        &mut self,
        t: u32,
        u: u32,
        v: u32,
        at: DVec3,
        near: &Near,
        changed: &mut Vec<u32>,
        steps: &mut usize,
    ) {
        let Some(s) = self.pair(t, u, v) else {
            return;
        };
        if near.in_bulge(at) {
            return;
        }
        let c = self.third(t, u, v);
        let d = self.third(s, u, v);
        let (ft, fs) = (self.soup.faces[t as usize], self.soup.faces[s as usize]);
        let m = self.add_vertex(at);
        let pieces = [
            ([u, m, c], ft),
            ([m, v, c], ft),
            ([v, m, d], fs),
            ([m, u, d], fs),
        ];
        if !pieces.iter().all(|&(tri, f)| self.proper_on(tri, f)) {
            self.drop_vertex(m);
            return;
        }
        self.set_tri(t, pieces[0].0);
        let t2 = self.add_tri(pieces[1].0, ft, t);
        self.set_tri(s, pieces[2].0);
        let s2 = self.add_tri(pieces[3].0, fs, s);
        changed.extend([t, t2, s, s2]);
        self.flip_round(m, vec![(c, u), (v, c), (d, v), (u, d)], changed, steps);
    }

    /// Puts a vertex at `q`, strictly inside triangle `h`, splitting it in
    /// three, if every piece is proper; then flips the sides facing it.
    fn insert(&mut self, h: u32, q: DVec3, near: &Near, changed: &mut Vec<u32>, steps: &mut usize) {
        if near.in_bulge(q) {
            return;
        }
        let [x, y, z] = self.soup.tris[h as usize];
        let face = self.soup.faces[h as usize];
        let m = self.add_vertex(q);
        let pieces = [[m, x, y], [m, y, z], [m, z, x]];
        if !pieces.iter().all(|&tri| self.proper_on(tri, face)) {
            self.drop_vertex(m);
            return;
        }
        self.set_tri(h, pieces[0]);
        let a = self.add_tri(pieces[1], face, h);
        let b = self.add_tri(pieces[2], face, h);
        changed.extend([h, a, b]);
        self.flip_round(m, vec![(x, y), (y, z), (z, x)], changed, steps);
    }

    /// Flips the free sides `u → w` facing the new vertex `m` (each a side
    /// of a triangle `[m, u, w]`) where the far corner across lies inside
    /// that triangle's circle and both new triangles are proper, and the
    /// sides that then face it, as an incremental Delaunay insertion does.
    fn flip_round(
        &mut self,
        m: u32,
        mut facing: Vec<(u32, u32)>,
        changed: &mut Vec<u32>,
        steps: &mut usize,
    ) {
        let mut flips = FLIPS;
        while let Some((u, w)) = facing.pop() {
            *steps += 1;
            if flips == 0 {
                break;
            }
            let Some(t) = self.running(u, w) else {
                continue;
            };
            if !self.soup.tris[t as usize].contains(&m) {
                continue;
            }
            if let Some((s, d)) = self.flip_delaunay(t, u, w) {
                flips -= 1;
                changed.extend([t, s]);
                facing.extend([(u, d), (d, w)]);
            }
        }
    }

    /// Flips the free sides of the triangles made on plane faces towards
    /// the Delaunay triangulation of their faces (constrained by the other
    /// sides), as the refinement assumes: a point at a circumcentre is
    /// then clear of the other vertices. Without it a wall's thin
    /// triangles split into pieces of the same shape, without end.
    fn delaunay_made(&mut self, most: usize, steps: &mut usize) {
        let mut sides: Vec<(u32, u32)> = Vec::new();
        for t in 0..self.soup.tris.len() {
            if self.alive[t]
                && self.soup.made[t]
                && self.planes[self.soup.faces[t] as usize].is_some()
            {
                let tri = self.soup.tris[t];
                sides.extend((0..3).map(|k| (tri[k], tri[(k + 1) % 3])));
            }
        }
        sides.sort_unstable();
        sides.reverse();
        let mut flips = most;
        while let Some((u, w)) = sides.pop() {
            *steps += 1;
            if flips == 0 {
                break;
            }
            let Some(t) = self.running(u, w) else {
                continue;
            };
            let a = self.third(t, u, w);
            if let Some((_, d)) = self.flip_delaunay(t, u, w) {
                flips -= 1;
                sides.extend([(a, u), (u, d), (d, w), (w, a)]);
            }
        }
    }

    /// The sine of each corner's angle (corner `k` between sides `k` and
    /// `k + 2`), the smaller of the corners' and, where a side is curved,
    /// the patch's, between the curve's tangent and the other side, as
    /// seen along `up`: a corner where a straight side leaves along a
    /// curve is narrow however wide the chords make it, and a cut halving
    /// the triangle there leaves it closed.
    fn corner_sines(&self, tri: [u32; 3], up: DVec3) -> [f64; 3] {
        let twice = self.normal(tri).length();
        [0, 1, 2].map(|k| {
            let (v, next, prev) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
            let (a, b) = (self.p(next) - self.p(v), self.p(prev) - self.p(v));
            let chord = twice / (a.length() * b.length());
            let dir = |w: u32, along: DVec3| {
                if self.curved(v, w) {
                    self.soup.curves[&(v.min(w), v.max(w))].ctrl - self.p(v)
                } else {
                    along
                }
            };
            let (l, r) = (dir(next, a), dir(prev, b));
            let patch = l.cross(r).dot(up) / (l.length() * r.length());
            if patch.is_nan() {
                0.0
            } else {
                chord.min(patch)
            }
        })
    }

    /// Flips the free side `u → v` of triangle `t` where both new
    /// triangles are proper and the narrowest of their corners (as
    /// [`Self::corner_sines`] measures them) is wider than the two old
    /// ones'. Gives the triangle across.
    fn flip_better(&mut self, t: u32, u: u32, v: u32, up: DVec3) -> Option<u32> {
        let s = self.free(t, u, v)?;
        let face = self.soup.faces[t as usize];
        let a = self.third(t, u, v);
        let b = self.third(s, u, v);
        if a == b || !self.shared(a, b).is_empty() {
            return None;
        }
        let worst = |tris: [[u32; 3]; 2]| {
            tris.iter()
                .flat_map(|&tri| self.corner_sines(tri, up))
                .fold(f64::INFINITY, f64::min)
        };
        let (n1, n2) = ([a, u, b], [a, b, v]);
        let old = [self.soup.tris[t as usize], self.soup.tris[s as usize]];
        if !(self.proper_on(n1, face) && self.proper_on(n2, face) && worst([n1, n2]) > worst(old)) {
            return None;
        }
        self.swap(t, s, [u, v], a, b);
        self.soup.curves.remove(&(a.min(b), a.max(b)));
        Some(s)
    }

    /// The living triangle running the side `u → w`, if any.
    fn running(&self, u: u32, w: u32) -> Option<u32> {
        self.shared(u, w).into_iter().find(|&t| {
            let tri = self.soup.tris[t as usize];
            (0..3).any(|k| tri[k] == u && tri[(k + 1) % 3] == w)
        })
    }

    /// Flips the free side `u → w` of triangle `t` (a plane face's) where
    /// the far corner across lies inside `t`'s circle and both new
    /// triangles are proper: `t` and the triangle across become `[a, u,
    /// d]` and `[a, d, w]`, `a` and `d` the far corners. Gives the triangle
    /// across and its far corner.
    fn flip_delaunay(&mut self, t: u32, u: u32, w: u32) -> Option<(u32, u32)> {
        let s = self.free(t, u, w)?;
        let face = self.soup.faces[t as usize];
        let (up, _) = self.planes[face as usize]?;
        let a = self.third(t, u, w);
        let d = self.third(s, u, w);
        if a == d || !self.shared(a, d).is_empty() {
            return None;
        }
        let (e1, e2) = frame(up);
        let o = self.p(a);
        let flat = |v: u32| {
            let p = self.p(v) - o;
            DVec2::new(p.dot(e1), p.dot(e2))
        };
        if !in_circle(flat(a), flat(u), flat(w), flat(d)) {
            return None;
        }
        let (n1, n2) = ([a, u, d], [a, d, w]);
        if !(self.proper_on(n1, face) && self.proper_on(n2, face)) {
            return None;
        }
        self.swap(t, s, [u, w], a, d);
        // The new side is straight: no record from an edge there before.
        self.soup.curves.remove(&(a.min(d), a.max(d)));
        Some((s, d))
    }

    /// The boxes of the control triangles of `tri`'s curved sides: each
    /// curve lies in its own.
    fn bulges(&self, tri: [u32; 3]) -> Vec<(DVec3, DVec3)> {
        (0..3)
            .filter_map(|k| {
                let (u, v) = (tri[k], tri[(k + 1) % 3]);
                if !self.curved(u, v) {
                    return None;
                }
                let c = self.soup.curves[&(u.min(v), u.max(v))].ctrl;
                let (p, q) = (self.p(u), self.p(v));
                Some((p.min(q).min(c), p.max(q).max(c)))
            })
            .collect()
    }

    /// The corner of triangle `t` other than `u` and `v`.
    fn third(&self, t: u32, u: u32, v: u32) -> u32 {
        *self.soup.tris[t as usize]
            .iter()
            .find(|&&w| w != u && w != v)
            .expect("a third corner")
    }

    /// A new vertex at `p`.
    fn add_vertex(&mut self, p: DVec3) -> u32 {
        let id = u32::try_from(self.soup.pos.len()).expect("fewer vertices than u32::MAX");
        self.soup.pos.push(p);
        self.around.push(Vec::new());
        id
    }

    /// Takes back the vertex [`Self::add_vertex`] just added, which no
    /// triangle has.
    fn drop_vertex(&mut self, v: u32) {
        debug_assert_eq!(v as usize + 1, self.soup.pos.len());
        debug_assert!(self.around[v as usize].is_empty());
        self.soup.pos.pop();
        self.around.pop();
    }

    /// Gives triangle `t` the corners `tri` (on its face): a piece of
    /// what it was, made by this boolean if that was.
    fn set_tri(&mut self, t: u32, tri: [u32; 3]) {
        for w in self.soup.tris[t as usize] {
            self.around[w as usize].retain(|&x| x != t);
        }
        self.soup.tris[t as usize] = tri;
        for w in tri {
            self.around[w as usize].push(t);
        }
    }

    /// A new triangle `tri` on `face`, a piece of triangle `of`: made by
    /// this boolean if that was.
    fn add_tri(&mut self, tri: [u32; 3], face: u32, of: u32) -> u32 {
        let t = u32::try_from(self.soup.tris.len()).expect("fewer triangles than u32::MAX");
        self.soup.tris.push(tri);
        self.soup.faces.push(face);
        self.soup.made.push(self.soup.made[of as usize]);
        self.alive.push(true);
        for w in tri {
            self.around[w as usize].push(t);
        }
        t
    }
}

/// Two unit vectors square to the unit normal `up` and to each other,
/// turning from the first to the second about `up` (so a triangle facing
/// along `up` runs counter-clockwise in them).
fn frame(up: DVec3) -> (DVec3, DVec3) {
    let e1 = up.any_orthonormal_vector();
    (e1, up.cross(e1))
}
