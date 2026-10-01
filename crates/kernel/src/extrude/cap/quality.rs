//! Refining the caps for quality: Steiner points at the circumcentres of
//! triangles with a narrow angle, Ruppert's way.
//!
//! The constrained Delaunay triangulation of a chord polygon has
//! triangles as thin as the polygon's vertices lie near each other's
//! chords: a fine polygon's caps are fans and ears whose angles at the
//! loop are its turning angle, and a plate's caps fan out of a corner to
//! every hole on a common tangent line. Both fail the mesh's rules next
//! to the walls, or leave long sides a later cut grazes. A round of
//! refinement takes the triangles whose narrowest angle is under 5°
//! (bar the exempt ones below), puts their circumcentres in order of
//! circumradius, keeps those more than half a circumradius from any kept
//! before, and for each kept one:
//!
//! - halves a segment whose control hull its way in, from the triangle's
//!   centroid, meets (it would leave the region, or come too near the
//!   loop), or whose chord's diametral circle it lies in (it encroaches
//!   on it): straight segments too, which nothing else halves, and the
//!   halves again while it encroaches on them ([`halve`]);
//! - or else adds it, if it lies in a triangle of the region, more than
//!   [`MIN_CLEAR`] resolutions from every segment's hull (a concave one
//!   it comes nearer is halved instead) and every Steiner point.
//!
//! A segment that can't be halved (too small, or halved too often) drops
//! the point. The points added stay in the region whatever is halved in
//! the same round: they keep clear of the hulls, which the halves lie
//! in. The round's caller triangulates afresh. Exempt, as refining them wouldn't
//! end: a narrowest angle at a loop corner narrower than 60° between the
//! curves' tangents (the small-input-angle rule), a circumradius under
//! [`MIN_SPLIT`] resolutions, and a shortest side that is a chord under
//! twice that, which can't be halved for long.

use glam::DVec2;

use super::MIN_CLEAR;
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

/// What a round of refinement asks: segments to halve, each with the
/// point that asked (see [`halve`]), or Steiner points to add.
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
/// halved: what rounds of refinement would do one halving a round, as
/// the triangles at a long chord near a feature keep encroaching on it,
/// each round triangulating afresh. Each pass spends the segments.
pub(super) fn halve(
    chain: &mut Chain,
    mut asks: Vec<(u32, DVec2)>,
    work: &mut Work,
) -> Result<(), KernelError> {
    while !asks.is_empty() {
        let mut ids: Vec<u32> = asks.iter().map(|&(s, _)| s).collect();
        ids.sort_unstable();
        ids.dedup();
        // Only segments that may be halved are asked.
        chain.split(&ids, super::MAX_CAP_DEPTH, true, |_, _| {
            KernelError::TooComplex
        })?;
        let (segs, _) = chain.flat();
        work.spend(segs.len())?;
        let mut next = Vec::new();
        for &(s, p) in &asks {
            // Segment `s` is now the two after those halved before it.
            let first = s + ids.partition_point(|&i| i < s) as u32;
            for h in [first, first + 1] {
                let seg = &segs[h as usize];
                let c = &seg.conic;
                if (p - c.p0).dot(p - c.p1) < 0.0
                    && chain.splittable(seg, super::MAX_CAP_DEPTH, true)
                {
                    next.push((h, p));
                }
            }
        }
        asks = next;
    }
    Ok(())
}

/// One round's caps: the chain, its segments (as [`Chain::flat`] gives
/// them) and their loops' starts, the vertices' positions, the region's
/// triangles and the Steiner points so far.
pub(super) struct Caps<'a, F> {
    pub chain: &'a Chain,
    pub segs: &'a [Seg],
    pub starts: &'a [u32],
    pub at: &'a F,
    pub tris: &'a [[u32; 3]],
    pub steiner: &'a [DVec2],
    pub margin: f64,
}

/// A point in the plane `z = 0` as a box.
fn point(p: DVec2) -> Bounds3 {
    Bounds3::point(p.extend(0.0))
}

impl<F: Fn(u32) -> DVec2 + Sync> Caps<'_, F> {
    /// One round of refinement.
    pub fn refine(&self, work: &mut Work) -> Result<Refined, KernelError> {
        let (segs, at, margin) = (self.segs, self.at, self.margin);
        work.spend(self.tris.len())?;
        let ids: Vec<u32> = (0..self.tris.len() as u32).collect();
        let found = par_map(&ids, |&t| self.bad(t as usize));
        let mut candidates: Vec<(f64, DVec2, u32)> = ids
            .iter()
            .zip(found)
            .filter_map(|(&t, c)| c.map(|(r, p)| (r, p, t)))
            .collect();
        if candidates.is_empty() {
            return Ok(Refined::default());
        }
        candidates.sort_unstable_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.x.total_cmp(&b.1.x))
                .then(a.1.y.total_cmp(&b.1.y))
                .then(a.2.cmp(&b.2))
        });
        work.spend(candidates.len())?;

        // Greedily, each more than half its circumradius from those kept
        // before (whose circumradii are no larger).
        let mut kept = Kept::default();
        let mut chosen = Vec::new();
        for &(r, p, t) in &candidates {
            let mut tried = 0usize;
            let near = kept.within(p, 0.5 * r, &mut tried);
            work.spend(tried)?;
            if !near {
                work.spend(kept.push(p))?;
                chosen.push((p, t));
            }
        }

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
        let triangles = Bvh::new(
            self.tris
                .iter()
                .map(|t| Bounds3::around(&t.map(|v| at(v).extend(0.0))).expect("three points"))
                .collect(),
        );
        let old = Bvh::new(self.steiner.iter().map(|&p| point(p)).collect());
        let built = segs.len().saturating_mul(2);
        work.spend(
            built
                .saturating_add(self.tris.len())
                .saturating_add(self.steiner.len()),
        )?;

        let halvable = |s: u32| {
            self.chain
                .splittable(&segs[s as usize], super::MAX_CAP_DEPTH, true)
        };
        let clear = MIN_CLEAR * margin;
        let mut refined = Refined::default();
        let mut near = Vec::new();
        for (p, t) in chosen {
            let tri = self.tris[t as usize];
            let centroid = (at(tri[0]) + at(tri[1]) + at(tri[2])) / 3.0;
            let p3 = [p.extend(0.0)];

            // Its way in from the triangle meets a hull: at margin 0, as a
            // thin ear's own centroid lies within the resolution of its
            // chords.
            let way = [centroid.extend(0.0), p.extend(0.0)];
            near.clear();
            hulls.query(&Bounds3::around(&way).expect("two points"), 0.0, &mut near);
            work.spend(near.len())?;
            if let Some(&s) = near
                .iter()
                .find(|&&s| !apart(&segs[s as usize].hull(), &way, 0.0))
            {
                if halvable(s) {
                    refined.split.push((s, p));
                }
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
                if halvable(s) {
                    refined.split.push((s, p));
                }
                continue;
            }

            // In the region.
            near.clear();
            triangles.query(&point(p), 0.0, &mut near);
            work.spend(near.len())?;
            let inside = near.iter().any(|&j| {
                let q = self.tris[j as usize].map(at);
                (0..3).all(|k| (q[(k + 1) % 3] - q[k]).perp_dot(p - q[k]) >= 0.0)
            });
            if !inside {
                continue;
            }

            // Clear of the hulls, or a concave one halved.
            near.clear();
            hulls.query(&point(p), clear, &mut near);
            work.spend(near.len())?;
            if let Some(&s) = near
                .iter()
                .find(|&&s| !apart(&segs[s as usize].hull(), &p3, clear))
            {
                if segs[s as usize].concave() && halvable(s) {
                    refined.split.push((s, p));
                }
                continue;
            }

            // Clear of the Steiner points.
            near.clear();
            old.query(&point(p), clear, &mut near);
            work.spend(near.len())?;
            if near
                .iter()
                .any(|&i| self.steiner[i as usize].distance(p) <= clear)
            {
                continue;
            }
            refined.added.push(p);
        }
        Ok(refined)
    }

    /// Triangle `t`'s circumradius and circumcentre, if its narrowest
    /// angle is under the bound and it isn't exempt.
    fn bad(&self, t: usize) -> Option<(f64, DVec2)> {
        let (at, margin) = (self.at, self.margin);
        let tri = self.tris[t];
        let p = tri.map(at);
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
        if (tri[o] as usize) < self.segs.len() && self.small_input_angle(tri[o]) {
            return None;
        }
        let short = 2.0 * MIN_SPLIT * margin;
        if chord(self.starts, tri[k], tri[(k + 1) % 3]).is_some() && lengths[k] < short * short {
            return None;
        }
        let (d, e) = (p[1] - p[0], p[2] - p[0]);
        let den = 2.0 * d.perp_dot(e);
        let (dd, ee) = (d.length_squared(), e.length_squared());
        let u = DVec2::new(e.y * dd - d.y * ee, d.x * ee - e.x * dd) / den;
        let r = u.length();
        (u.is_finite() && r >= MIN_SPLIT * margin).then_some((r, p[0] + u))
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
