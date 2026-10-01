//! Refining the caps for quality: Steiner points at the circumcentres of
//! triangles with a narrow angle, Ruppert's way.
//!
//! The constrained Delaunay triangulation of a chord polygon has
//! triangles as thin as the polygon's vertices lie near each other's
//! chords: a fine polygon's caps are fans and ears whose angles at the
//! loop are its turning angle, and a plate's caps fan out of a corner to
//! every hole on a common tangent line. Both fail the mesh's rules next
//! to the walls, or leave long sides a later cut grazes. A run of
//! refinement queues the triangles whose narrowest angle is under 5°
//! (bar the exempt ones below), smallest circumradius first, and takes
//! each in turn while the triangulation still has it. Its circumcentre:
//!
//! - asks for a segment to be halved if the segment's control hull meets
//!   the way in from the triangle's centroid (it would leave the region,
//!   or come too near the loop), or if it lies in the segment's chord's
//!   diametral circle (it encroaches on it): straight segments too, which
//!   nothing else halves, and the halves again while it encroaches on
//!   them ([`halve`]), unless a point that asked before lies within half
//!   its circumradius: that halving may take its triangle too;
//! - or else is inserted, if it lies in a triangle of the region, more
//!   than [`MIN_CLEAR`] resolutions from every segment's hull (a concave
//!   one it comes nearer is asked to be halved instead) and every
//!   Steiner point, and the bad triangles around it join the queue.
//!
//! A segment that can't be halved (too small, or halved too often) drops
//! the point. The halvings asked for are made when the queue is empty,
//! and the next round runs refinement again. The points added stay in
//! the region whatever is halved: they keep clear of the hulls, which
//! the halves lie in. Exempt, as refining them wouldn't end: a narrowest
//! angle at a loop corner narrower than 60° between the curves' tangents
//! (the small-input-angle rule), a circumradius under [`MIN_SPLIT`]
//! resolutions, and a shortest side that is a chord under twice that,
//! which can't be halved for long.
//!
//! A circumcentre lies at least its circumradius from every vertex the
//! triangle sees, so the points space themselves; the queue's order, the
//! triangulation's and the tests are all sequential and total, so a run
//! is the same at any thread count.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use glam::DVec2;
use spade::handles::{FixedFaceHandle, InnerTag};

use super::{Live, MIN_CLEAR};
use crate::KernelError;
use crate::budget::Work;
use crate::extrude::chain::{Chain, Seg, chord};
use crate::mesh::{Bvh, MIN_SPLIT, apart};
use crate::par::par_map;
use crate::patch::Bounds3;

/// The sine of the narrowest angle a cap triangle keeps: sin 5°.
const SIN_BOUND: f64 = 0.087_155_742_747_658_17;

/// A loop corner whose tangents are less than 60° apart, its cosine
/// above this, exempts the triangle whose narrowest angle it holds.
const COS_SMALL_INPUT: f64 = 0.5;

/// The most rounds of refinement, apart from the rounds of mending.
pub(super) const MAX_QUALITY_ROUNDS: usize = 64;

/// What a run of refinement holds a triangle's narrowest angle to, and
/// what it may do about one narrower.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bound {
    /// For quality: every triangle but the exempt ones, segments halved
    /// where the points would leave the region or encroach on them.
    Quality,
    /// For crowded caps: every triangle, no segment halved, and points
    /// kept half their circumradius from every segment's hull. A point
    /// then makes no side shorter than the shortest there is (at 5° a
    /// circumradius is over 5.7 times the triangle's shortest side), so
    /// it ends without the exemptions.
    Crowded,
}

/// What a run of refinement did and asks: segments to halve, each with
/// the point that asked (see [`halve`]), and the Steiner points it added
/// to the triangulation, in order.
#[derive(Debug, Default)]
pub(super) struct Refined {
    pub split: Vec<(u32, DVec2)>,
    pub added: Vec<DVec2>,
}

impl Refined {
    pub fn is_empty(&self) -> bool {
        self.split.is_empty() && self.added.is_empty()
    }
}

/// Halves the segments `asks` names, and each half again, as long as the
/// point that asked lies in its chord's diametral circle and it may be
/// halved: what runs of refinement would do one halving a run, as the
/// triangles at a long chord near a feature keep encroaching on it. The
/// chords in `live` are replaced with the pieces'. Spends the segments,
/// the pieces made and the vertices added.
pub(super) fn halve(
    chain: &mut Chain,
    live: &mut Live,
    mut asks: Vec<(u32, DVec2)>,
    work: &mut Work,
) -> Result<(), KernelError> {
    if asks.is_empty() {
        return Ok(());
    }
    // Only segments that may be halved are asked.
    asks.sort_by_key(|&(s, _)| s);
    let (segs, starts) = chain.flat();
    work.spend(segs.len())?;
    let mut pieces = Vec::new();
    for group in asks.chunk_by(|a, b| a.0 == b.0) {
        let s = group[0].0;
        let points: Vec<DVec2> = group.iter().map(|&(_, p)| p).collect();
        let mut these = Vec::new();
        cut(chain, &segs[s as usize], &points, &mut these)?;
        work.spend(these.len())?;
        pieces.push((s, these));
    }
    super::replace(chain, live, &starts, &pieces, work)
}

/// `seg`'s halves, each halved again in turn while one of `points` lies
/// in its chord's diametral circle and it may be halved, appended to
/// `out` in order.
fn cut(chain: &Chain, seg: &Seg, points: &[DVec2], out: &mut Vec<Seg>) -> Result<(), KernelError> {
    for half in seg.halves()? {
        let c = &half.conic;
        let inside: Vec<DVec2> = points
            .iter()
            .copied()
            .filter(|&p| (p - c.p0).dot(p - c.p1) < 0.0)
            .collect();
        if !inside.is_empty() && chain.splittable(&half, super::MAX_CAP_DEPTH, true) {
            cut(chain, &half, &inside, out)?;
        } else {
            out.push(half);
        }
    }
    Ok(())
}

/// One round's caps: the chain, its segments (as [`Chain::flat`] gives
/// them) and their loops' starts, the region's triangles and their faces
/// in the triangulation, the Steiner points so far and the resolution.
pub(super) struct Caps<'a> {
    pub chain: &'a Chain,
    pub segs: &'a [Seg],
    pub starts: &'a [u32],
    pub tris: &'a [[u32; 3]],
    pub faces: &'a [FixedFaceHandle<InnerTag>],
    pub steiner: &'a [DVec2],
    pub margin: f64,
}

/// A point in the plane `z = 0` as a box.
fn point(p: DVec2) -> Bounds3 {
    Bounds3::point(p.extend(0.0))
}

/// Triangle `tri`, rotated to start at its lowest corner: one triangle
/// one way.
fn lowest_first(tri: [u32; 3]) -> [u32; 3] {
    let k = (0..3).min_by_key(|&k| tri[k]).expect("three corners");
    [tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]]
}

/// A triangle waiting in refinement's queue, with its circumradius and
/// circumcentre and its face. Ordered by circumradius, then circumcentre,
/// then corners, the smallest last, so a [`BinaryHeap`] hands it out
/// first: a total order on triangles, each in the queue once.
struct Bad {
    r: f64,
    p: DVec2,
    tri: [u32; 3],
    face: FixedFaceHandle<InnerTag>,
}

impl Bad {
    fn key(&self) -> impl Ord + use<> {
        (
            Reverse(Total(self.r)),
            Reverse(Total(self.p.x)),
            Reverse(Total(self.p.y)),
            Reverse(self.tri),
        )
    }
}

impl PartialEq for Bad {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Bad {}

impl PartialOrd for Bad {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Bad {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

/// A float in its total order.
#[derive(PartialEq)]
struct Total(f64);

impl Eq for Total {}

impl PartialOrd for Total {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Total {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Caps<'_> {
    /// Vertex `v`'s point, with `added` the Steiner points this run added.
    fn at(&self, added: &[DVec2], v: u32) -> DVec2 {
        let (n, s) = (self.segs.len(), self.steiner.len());
        let v = v as usize;
        if v < n {
            self.segs[v].conic.p0
        } else if v < n + s {
            self.steiner[v - n]
        } else {
            added[v - n - s]
        }
    }

    /// A run of refinement to `bound`, inserting its points into `live`
    /// (this round's triangulation) as it goes: a queue of the bad
    /// triangles, smallest circumradius first, each taken while it is
    /// still a face, its circumcentre tested as the module says (for
    /// [`Bound::Crowded`]: in the region, half its circumradius clear of
    /// every hull) and inserted, and the bad triangles around the new
    /// vertex queued. A point that would leave the region or encroach on
    /// a chord asks for that segment to be halved instead, which is left
    /// to the caller. A triangle the triangulation makes when a point is
    /// inserted has that point for a corner, and lies in the region with
    /// it, so the region's triangles are those at the start and those
    /// with a corner added since.
    pub fn refine(
        &self,
        live: &mut Live,
        bound: Bound,
        work: &mut Work,
    ) -> Result<Refined, KernelError> {
        let (segs, margin) = (self.segs, self.margin);
        let base = segs.len() + self.steiner.len();
        work.spend(self.tris.len())?;
        let found = par_map(self.tris, |&tri| self.bad(tri, &[], bound));
        let mut queue: BinaryHeap<Bad> = self
            .tris
            .iter()
            .zip(self.faces)
            .zip(found)
            .filter_map(|((&tri, &face), found)| found.map(|(r, p)| Bad { r, p, tri, face }))
            .collect();
        if queue.is_empty() {
            return Ok(Refined::default());
        }
        work.spend(queue.len())?;

        let hulls = Bvh::new(
            segs.iter()
                .map(|s| Bounds3::around(&s.hull()).expect("three points"))
                .collect(),
        );
        // Each chord's diametral circle's box.
        let circles = Bvh::new(
            segs.iter()
                .map(|s| {
                    let (a, b) = (s.conic.p0, s.conic.p1);
                    let (m, h) = ((a + b) * 0.5, 0.5 * a.distance(b));
                    Bounds3 {
                        min: (m - DVec2::splat(h)).extend(0.0),
                        max: (m + DVec2::splat(h)).extend(0.0),
                    }
                })
                .collect(),
        );
        let old = Bvh::new(self.steiner.iter().map(|&p| point(p)).collect());
        let mut region: Vec<[u32; 3]> = self.tris.iter().map(|&t| lowest_first(t)).collect();
        region.sort_unstable();
        work.spend(
            segs.len()
                .saturating_mul(2)
                .saturating_add(self.steiner.len())
                .saturating_add(self.tris.len()),
        )?;

        let halvable = |s: u32| {
            self.chain
                .splittable(&segs[s as usize], super::MAX_CAP_DEPTH, true)
        };
        let clear = MIN_CLEAR * margin;
        let mut refined = Refined::default();
        let mut added = Vec::new();
        let (mut kept, mut asked) = (Kept::default(), Kept::default());
        let mut near = Vec::new();
        while let Some(Bad { r, p, tri, face }) = queue.pop() {
            if !live.is(face, tri) {
                continue;
            }
            // A halving asked for by a point within half its circumradius
            // (which is no larger) may well take this triangle too, as
            // the next round will tell: it doesn't ask again.
            let mut tried = 0usize;
            let pending = asked.within(p, 0.5 * r, &mut tried);
            work.spend(tried)?;
            let mut ask = |s: u32, work: &mut Work| -> Result<(), KernelError> {
                if !pending && halvable(s) {
                    refined.split.push((s, p));
                    work.spend(asked.push(p))?;
                }
                Ok(())
            };
            let corners = tri.map(|v| self.at(&added, v));
            let centroid = (corners[0] + corners[1] + corners[2]) / 3.0;
            let p3 = [p.extend(0.0)];

            if bound == Bound::Quality {
                // Its way in from the triangle meets a hull: at margin 0,
                // as a thin ear's own centroid lies within the resolution
                // of its chords.
                let way = [centroid.extend(0.0), p.extend(0.0)];
                near.clear();
                hulls.query(&Bounds3::around(&way).expect("two points"), 0.0, &mut near);
                work.spend(near.len())?;
                if let Some(&s) = near
                    .iter()
                    .find(|&&s| !apart(&segs[s as usize].hull(), &way, 0.0))
                {
                    ask(s, work)?;
                    continue;
                }

                // It encroaches on a chord.
                near.clear();
                circles.query(&point(p), 0.0, &mut near);
                work.spend(near.len())?;
                if let Some(&s) = near.iter().find(|&&s| {
                    let c = &segs[s as usize].conic;
                    (p - c.p0).dot(p - c.p1) < 0.0
                }) {
                    ask(s, work)?;
                    continue;
                }
            }

            // In the region.
            let inside = live.locate(p).is_some_and(|f| {
                f.iter().any(|&v| v as usize >= base)
                    || region.binary_search(&lowest_first(f)).is_ok()
            });
            if !inside {
                continue;
            }

            // Clear of the hulls, or a concave one halved.
            let clear_of_hulls = match bound {
                Bound::Quality => clear,
                Bound::Crowded => clear.max(0.5 * r),
            };
            near.clear();
            hulls.query(&point(p), clear_of_hulls, &mut near);
            work.spend(near.len())?;
            if let Some(&s) = near
                .iter()
                .find(|&&s| !apart(&segs[s as usize].hull(), &p3, clear_of_hulls))
            {
                if bound == Bound::Quality && segs[s as usize].concave() {
                    ask(s, work)?;
                }
                continue;
            }

            // Clear of the Steiner points.
            near.clear();
            old.query(&point(p), clear, &mut near);
            work.spend(near.len())?;
            let mut tried = 0usize;
            let close = near
                .iter()
                .any(|&i| self.steiner[i as usize].distance(p) <= clear)
                || kept.within(p, clear, &mut tried);
            work.spend(tried)?;
            if close {
                continue;
            }

            let v = live.insert(p)?;
            work.spend(super::TRIANGULATION_WORK)?;
            work.spend(kept.push(p))?;
            added.push(p);
            let around = live.around(v);
            work.spend(around.len())?;
            for (face, tri) in around {
                if let Some((r, p)) = self.bad(tri, &added, bound) {
                    queue.push(Bad { r, p, tri, face });
                }
            }
        }
        refined.added = added;
        Ok(refined)
    }

    /// Triangle `tri`'s circumradius and circumcentre, if its narrowest
    /// angle is under the bound and, for [`Bound::Quality`], it isn't
    /// exempt; `added` holds the points this run added.
    fn bad(&self, tri: [u32; 3], added: &[DVec2], bound: Bound) -> Option<(f64, DVec2)> {
        let margin = self.margin;
        let p = tri.map(|v| self.at(added, v));
        // Side `k` runs from corner `k` to the next; the angle across it
        // is at the corner after that.
        let lengths = [0, 1, 2].map(|k| (p[(k + 1) % 3] - p[k]).length_squared());
        let k = (0..3)
            .min_by(|&a, &b| lengths[a].total_cmp(&lengths[b]))
            .expect("three sides");
        let o = (k + 2) % 3;
        let (ea, eb) = (p[k] - p[o], p[(k + 1) % 3] - p[o]);
        if ea.perp_dot(eb).abs() >= SIN_BOUND * ea.length() * eb.length() {
            return None;
        }
        let (d, e) = (p[1] - p[0], p[2] - p[0]);
        let den = 2.0 * d.perp_dot(e);
        let (dd, ee) = (d.length_squared(), e.length_squared());
        let u = DVec2::new(e.y * dd - d.y * ee, d.x * ee - e.x * dd) / den;
        let r = u.length();
        if bound == Bound::Quality {
            if (tri[o] as usize) < self.segs.len() && self.small_input_angle(tri[o]) {
                return None;
            }
            let short = 2.0 * MIN_SPLIT * margin;
            if chord(self.starts, tri[k], tri[(k + 1) % 3]).is_some() && lengths[k] < short * short
            {
                return None;
            }
            if r < MIN_SPLIT * margin {
                return None;
            }
        }
        (u.is_finite() && r.is_finite()).then_some((r, p[0] + u))
    }

    /// Whether the region's corner at loop vertex `v` is under 60°
    /// between the curves' tangents.
    fn small_input_angle(&self, v: u32) -> bool {
        let starts = self.starts;
        let l = starts.partition_point(|&s| s <= v) - 1;
        let before = if v == starts[l] {
            starts[l + 1] - 1
        } else {
            v - 1
        };
        // The region lies counter-clockwise from the way out to the way
        // back.
        let out = self.segs[v as usize].tangent(true);
        let back = self.segs[before as usize].tangent(false);
        out.perp_dot(back) > 0.0 && out.dot(back) > COS_SMALL_INPUT * out.length() * back.length()
    }
}

/// The points kept so far, in groups of falling power-of-two sizes, each
/// with its hierarchy: adding one merges the groups of its size, so it
/// is rebuilt about `log n` times.
#[derive(Default)]
struct Kept {
    groups: Vec<(Vec<DVec2>, Bvh)>,
}

impl Kept {
    /// Adds `p`, returning how many points it rebuilt a hierarchy over.
    fn push(&mut self, p: DVec2) -> usize {
        let mut points = vec![p];
        while self
            .groups
            .last()
            .is_some_and(|(group, _)| group.len() == points.len())
        {
            let (group, _) = self.groups.pop().expect("a group");
            points.extend(group);
        }
        let bvh = Bvh::new(points.iter().map(|&p| point(p)).collect());
        let built = points.len();
        self.groups.push((points, bvh));
        built
    }

    /// Whether a kept point is within `d` of `p`, adding to `tried` the
    /// points it looked at.
    fn within(&self, p: DVec2, d: f64, tried: &mut usize) -> bool {
        self.groups.iter().any(|(points, bvh)| {
            bvh.any(&point(p), d, |i| {
                *tried += 1;
                points[i as usize].distance(p) <= d
            })
        })
    }
}
