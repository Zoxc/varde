//! A profile's loops as the segments the solid is built on: classified,
//! checked for cusps, and split until no two come near each other.

use std::collections::BTreeMap;

use glam::{DVec2, DVec3};

use crate::budget::Work;
use crate::mesh::{Bvh, apart};
use crate::par::par_map;
use crate::patch::{Bounds3, Conic2};
use crate::profile::{Profile, ProfileError};
use crate::{KernelError, MAX_PATCHES};

/// Corners sharper than this, as the sine of the angle between the
/// segments' tangents, are cusps; cap patch corners must be wider.
pub(super) const SIN_MIN: f64 = 1e-3;

/// How many times a segment of the profile may be halved.
pub(super) const MAX_SPLIT_DEPTH: u8 = 24;

/// The smallest segment halved, in resolutions across its control
/// points: pieces a few resolutions long can't keep the margin from their
/// own neighbours, so halving them only makes more that fail.
const MIN_SPLIT: f64 = 64.0;

/// A piece of a loop.
#[derive(Debug, Clone, Copy)]
pub(super) struct Seg {
    pub conic: Conic2,
    /// Whether its control point is off its chord by more than the
    /// resolution. A straight one is [`Conic2::line`].
    pub curved: bool,
    /// The side face it sweeps: the input segment it is a piece of.
    pub side: u32,
    /// How many halvings made it from the input segment.
    pub depth: u8,
}

impl Seg {
    /// The direction the segment leaves its start (`start`) or its end
    /// along the curve: towards the control point.
    pub fn tangent(&self, start: bool) -> DVec2 {
        let c = &self.conic;
        if start { c.c - c.p0 } else { c.c - c.p1 }
    }

    /// Whether it is curved and bulges into the region, on its left: its
    /// chord then cuts across material that isn't the region's.
    pub fn concave(&self) -> bool {
        let c = &self.conic;
        self.curved && (c.p1 - c.p0).perp_dot(c.c - c.p0) > 0.0
    }

    /// Its control points in the plane `z = 0`.
    pub fn hull(&self) -> [DVec3; 3] {
        let c = &self.conic;
        [c.p0, c.c, c.p1].map(|p| p.extend(0.0))
    }

    fn halves(&self) -> Result<[Seg; 2], KernelError> {
        let [a, b] = self.conic.split_half()?;
        let depth = self.depth + 1;
        let piece = |conic| Seg {
            conic,
            depth,
            ..*self
        };
        Ok([piece(a), piece(b)])
    }
}

/// An input segment: the wall it sweeps is one face.
#[derive(Debug, Clone, Copy)]
pub(super) struct Side {
    pub curve: u64,
    /// Its number among the segments of `curve`, in profile order.
    pub segment: u32,
    /// The input conic, straightened if it was straight.
    pub conic: Conic2,
    pub curved: bool,
    /// Its loop and index in the profile.
    pub at: (usize, usize),
}

/// The loops as segments. Vertex `i` is the start of segment `i`,
/// counted through the loops in order.
#[derive(Debug, Clone)]
pub(super) struct Chain {
    pub loops: Vec<Vec<Seg>>,
    pub sides: Vec<Side>,
    /// The resolution: the margin segments keep from each other.
    margin: f64,
}

impl Chain {
    /// The profile's segments, a segment whose control point is within
    /// `margin` of its chord made straight. Refuses straight segments that
    /// run back on themselves and cusps between segments. Loops of two
    /// segments have their curved ones halved, so no two segments share
    /// both ends.
    pub fn new(profile: &Profile, margin: f64) -> Result<Chain, KernelError> {
        let mut numbers: BTreeMap<u64, u32> = BTreeMap::new();
        let mut sides = Vec::with_capacity(profile.segment_count());
        let mut loops = Vec::with_capacity(profile.loops.len());
        for (l, lp) in profile.loops.iter().enumerate() {
            let mut segs = Vec::with_capacity(lp.segments.len());
            for (s, seg) in lp.segments.iter().enumerate() {
                let (conic, curved) = classify(&seg.conic, margin)
                    .ok_or(KernelError::Profile(ProfileError::Degenerate(l, s)))?;
                let number = numbers.entry(seg.curve).or_insert(0);
                let side = sides.len() as u32;
                sides.push(Side {
                    curve: seg.curve,
                    segment: *number,
                    conic,
                    curved,
                    at: (l, s),
                });
                *number += 1;
                segs.push(Seg {
                    conic,
                    curved,
                    side,
                    depth: 0,
                });
            }
            for s in 0..segs.len() {
                let prev = &segs[(s + segs.len() - 1) % segs.len()];
                let (t_out, t_in) = (segs[s].tangent(true), prev.tangent(false));
                let cross = t_out.perp_dot(t_in).abs();
                if t_out.dot(t_in) > 0.0 && cross <= SIN_MIN * t_out.length() * t_in.length() {
                    return Err(ProfileError::Cusp(l, s).into());
                }
            }
            if segs.len() == 2 {
                let mut pieces = Vec::with_capacity(4);
                for seg in segs {
                    if seg.curved {
                        pieces.extend(seg.halves()?);
                    } else {
                        pieces.push(seg);
                    }
                }
                segs = pieces;
            }
            loops.push(segs);
        }
        Ok(Chain {
            loops,
            sides,
            margin,
        })
    }

    /// The number of segments, which is the number of vertices.
    pub fn len(&self) -> usize {
        self.loops.iter().map(Vec::len).sum()
    }

    /// The segments in order, and for each loop the index of its first.
    pub fn flat(&self) -> (Vec<Seg>, Vec<u32>) {
        let mut segs = Vec::with_capacity(self.len());
        let mut starts = Vec::with_capacity(self.loops.len() + 1);
        for lp in &self.loops {
            starts.push(segs.len() as u32);
            segs.extend_from_slice(lp);
        }
        starts.push(segs.len() as u32);
        (segs, starts)
    }

    /// Whether `seg` may be halved: it is curved, was halved fewer than
    /// `max_depth` times, and spans at least [`MIN_SPLIT`] resolutions.
    pub fn splittable(&self, seg: &Seg, max_depth: u8) -> bool {
        let b = Bounds3::around(&seg.hull()).expect("three points");
        seg.curved
            && seg.depth < max_depth
            && (b.max - b.min).max_element() >= MIN_SPLIT * self.margin
    }

    /// Halves the segments `ids` (indices into [`Self::flat`], sorted),
    /// or fails with `refused` if one isn't
    /// [`splittable`](Self::splittable) within `max_depth`.
    pub fn split(
        &mut self,
        ids: &[u32],
        max_depth: u8,
        refused: KernelError,
    ) -> Result<(), KernelError> {
        let (segs, _) = self.flat();
        if ids
            .iter()
            .any(|&i| !self.splittable(&segs[i as usize], max_depth))
        {
            return Err(refused);
        }
        let mut ids = ids.iter().copied().peekable();
        let mut i = 0u32;
        for lp in &mut self.loops {
            let mut out = Vec::with_capacity(lp.len());
            for seg in lp.iter() {
                if ids.next_if_eq(&i).is_some() {
                    out.extend(seg.halves()?);
                } else {
                    out.push(*seg);
                }
                i += 1;
            }
            *lp = out;
        }
        Ok(())
    }

    /// Halves curved segments until every two are apart: the control
    /// hulls of segments that share no end more than the resolution
    /// apart, and those of two in a row split by a line through their
    /// shared end with the resolution to spare. Then no two cross or touch, the polygon of
    /// the chords is as simple as the loops, and each curved segment's
    /// bulge off its chord lies in its own hull, clear of everything else.
    ///
    /// Two segments that aren't apart and can't be halved (straight, or
    /// curved but halved [`MAX_SPLIT_DEPTH`] times or shorter than
    /// [`MIN_SPLIT`] resolutions) make the profile
    /// [`ProfileError::Touching`]. Halving only shrinks hulls, so pairs
    /// that were apart stay apart.
    pub fn separate(&mut self, work: &mut Work) -> Result<(), KernelError> {
        let margin = self.margin;
        loop {
            let (segs, starts) = self.flat();
            if segs.len() > MAX_PATCHES / 4 {
                return Err(KernelError::TooComplex);
            }
            let bvh = Bvh::new(
                segs.iter()
                    .map(|s| Bounds3::around(&s.hull()).expect("three points"))
                    .collect(),
            );
            let pairs = bvh.self_pairs(margin);
            work.spend(segs.len().saturating_add(pairs.len()))?;
            let next = next_in_loop(&starts);
            let tests = par_map(&pairs, |&[i, j]| {
                let (a, b) = (&segs[i as usize], &segs[j as usize]);
                if next(i) == j {
                    apart_at_joint(a, b, margin)
                } else if next(j) == i {
                    apart_at_joint(b, a, margin)
                } else {
                    apart(&a.hull(), &b.hull(), margin)
                }
            });
            let mut split = Vec::new();
            let mut done = true;
            for (&[i, j], passes) in pairs.iter().zip(tests) {
                if passes {
                    continue;
                }
                done = false;
                let (a, b) = (&segs[i as usize], &segs[j as usize]);
                let halve = [(a, i), (b, j)]
                    .into_iter()
                    .filter(|(s, _)| self.splittable(s, MAX_SPLIT_DEPTH))
                    .map(|(_, i)| i);
                let before = split.len();
                split.extend(halve);
                if split.len() == before {
                    return Err(self.touching(a, b));
                }
            }
            if done {
                return Ok(());
            }
            split.sort_unstable();
            split.dedup();
            self.split(&split, MAX_SPLIT_DEPTH, KernelError::TooComplex)?;
        }
    }

    fn touching(&self, a: &Seg, b: &Seg) -> KernelError {
        let at = |s: &Seg| self.sides[s.side as usize].at;
        ProfileError::Touching([at(a), at(b)]).into()
    }
}

/// The segment after segment `i` in its loop, for the loops starting at
/// `starts` (and the end last, as [`Chain::flat`] gives them).
pub(super) fn next_in_loop(starts: &[u32]) -> impl Fn(u32) -> u32 + '_ {
    move |i| {
        let l = starts.partition_point(|&s| s <= i) - 1;
        if i + 1 == starts[l + 1] {
            starts[l]
        } else {
            i + 1
        }
    }
}

/// The segment running between vertices `a` and `b` (either way round),
/// for the loops starting at `starts`, and whether it starts at `a`;
/// `None` for an inner edge.
pub(super) fn chord(starts: &[u32], a: u32, b: u32) -> Option<(u32, bool)> {
    let n = *starts.last().expect("the end");
    if a >= n || b >= n {
        return None;
    }
    let next = next_in_loop(starts);
    if next(a) == b {
        Some((a, true))
    } else if next(b) == a {
        Some((b, false))
    } else {
        None
    }
}

/// Whether `a`, ending where `b` starts, and `b` are split by a line
/// through that point with `margin` to spare: the hull of `a`'s other
/// control points and of `b`'s mirrored through it is more than `margin`
/// from it.
fn apart_at_joint(a: &Seg, b: &Seg, margin: f64) -> bool {
    let v = a.conic.p1;
    let points =
        [a.conic.p0 - v, a.conic.c - v, v - b.conic.c, v - b.conic.p1].map(|p| p.extend(0.0));
    apart(&points, &[DVec3::ZERO], margin)
}

/// The conic as a segment of the solid: straight ([`Conic2::line`]) when
/// its control point is within `margin` of the chord and between its
/// ends, curved when further off; `None` when it lies on the chord's line
/// outside the ends, where the curve would run back on itself.
fn classify(conic: &Conic2, margin: f64) -> Option<(Conic2, bool)> {
    let chord = conic.p1 - conic.p0;
    let off = conic.c - conic.p0;
    let length = chord.length();
    if chord.perp_dot(off).abs() > margin * length {
        return Some((*conic, true));
    }
    let along = chord.dot(off) / (length * length);
    if along > 0.0 && along < 1.0 {
        Some((Conic2::line(conic.p0, conic.p1).ok()?, false))
    } else {
        None
    }
}
