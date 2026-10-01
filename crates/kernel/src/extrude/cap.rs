//! Triangulating a profile's region for the caps.
//!
//! The caps are the constrained Delaunay triangulation of the polygon of
//! the segments' chords, with each chord's triangle taking the segment's
//! curve as its edge. The chain is already separated, so the chords don't
//! cross and each bulge lies clear of everything but its own triangle's
//! side. What is left is to keep every cap patch's corners open, measured
//! between the curves' tangents rather than the chords:
//!
//! - A triangle with two of the loop's segments meeting at a corner (an
//!   ear) whose tangents turn by about 180° or more, as two arcs in a row
//!   of a circle do, gets a Steiner point at its centroid.
//! - A corner between a curved segment and an inner edge that is too
//!   narrow or too wide gets that segment halved: the new chord leaves
//!   the corner closer to the curve's tangent, which halving keeps. For a
//!   segment bulging into the region, that keeps the bulge inside its
//!   triangle.
//! - A triangle whose corners are all open but whose patch still fails
//!   the fold check (a bulge of weight above 1 into the triangle can) gets
//!   its curved segments halved: their weights go towards 1.
//! - On a second try, when the caps made without it fail the mesh's
//!   rules: a corner at a loop vertex that is flat (obtuse, the vertex
//!   within [`FLAT`] resolutions of the opposite side, as where short
//!   segments meet nearly straight) gets a Steiner point moved into the
//!   region from that vertex. The sliver there comes within the
//!   resolution of the walls; the point, well clear of everything, takes
//!   the vertex's corner instead. A flat ear gets this point rather than
//!   its centroid, which lies as close to its sides as the ear is flat.
//!
//! - When nothing else is left to mend, a round of refinement for
//!   quality ([`quality`]): Steiner points at the circumcentres of
//!   triangles with an angle under 5°, chords (straight ones too) halved
//!   where those would encroach on them. Fine polygons' fans and ears,
//!   and a plate's fans out of a corner to holes on a common tangent,
//!   become triangles a later cut, or the walls, can't come too close to.
//!
//! A Steiner point inside the hull of a segment bulging into the region
//! could end up outside the region once that segment is halved, so that
//! segment is halved instead. Each round triangulates afresh; the rounds
//! stop when nothing changes, or fail after [`MAX_ROUNDS`] rounds of
//! mending (rounds of refinement count apart, up to
//! [`MAX_QUALITY_ROUNDS`], after which the caps stand as they are). A
//! segment to halve that already spans less than
//! [`MIN_SPLIT`](crate::mesh::MIN_SPLIT) resolutions is left to
//! refinement, whose points can take its corner apart; if they don't,
//! it fails the profile with [`ProfileError::TooFine`]: detail too small
//! for the resolution, which a finer tolerance mends. One halved
//! [`MAX_CAP_DEPTH`] times fails with [`KernelError::TooComplex`] at
//! once.
//!
//! Flat corners are the only thing the two tries do differently, and up
//! to the first round that finds one they do the same, work counted
//! included. So the first try keeps the state that round started from
//! (a [`Rounds`], its fork) and the second try resumes from it rather
//! than from the start; with no fork it would repeat the first try and
//! isn't made.

use std::collections::VecDeque;

use glam::DVec2;
use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, HierarchyHintGenerator, Point2, Triangulation};

use super::chain::{Chain, SIN_MIN, Seg, chord, next_in_loop};
use crate::budget::Work;
use crate::mesh::{Bvh, apart};
use crate::par::par_map;
use crate::patch::{Bounds3, Patch};
use crate::profile::ProfileError;
use crate::{KernelError, MAX_PATCHES};

mod quality;

use quality::MAX_QUALITY_ROUNDS;

/// The most rounds of triangulating and mending.
const MAX_ROUNDS: usize = 32;

/// How many times a segment may have been halved, all told, before the
/// caps give up halving it: mending that doesn't converge would otherwise
/// double the segments it can't mend every round.
pub(super) const MAX_CAP_DEPTH: u8 = 16;

/// Work units per vertex for one triangulation: inserting a vertex walks
/// and flips a few edges.
const TRIANGULATION_WORK: usize = 8;

/// A cap triangle's corner at a loop vertex is flat when it is obtuse and
/// the vertex lies within this many resolutions of the opposite side.
const FLAT: f64 = 64.0;

/// The distances a flat corner's Steiner point is tried at, as fractions
/// of half the shorter chord meeting there.
const DISPLACE: [f64; 3] = [1.0, 0.25, 0.0625];

/// The least clearance, in resolutions, a displaced Steiner point must
/// keep.
const MIN_CLEAR: f64 = 4.0;

/// The caps' triangles.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Cap {
    /// Points added inside the region. Vertex ids run through the chain's
    /// vertices, then these.
    pub steiner: Vec<DVec2>,
    /// Counter-clockwise in the profile's plane.
    pub tris: Vec<[u32; 3]>,
}

/// What a try of the caps does besides the mending every try does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Mode {
    /// Refine for quality ([`quality`]).
    pub quality: bool,
    /// Move Steiner points in from loop vertices with flat corners.
    pub flat_corners: bool,
}

impl Mode {
    /// The first try: refined.
    pub const QUALITY: Mode = Mode {
        quality: true,
        flat_corners: false,
    };
    /// The second: refined, with flat corners.
    pub const FLAT_CORNERS: Mode = Mode {
        quality: true,
        flat_corners: true,
    };
    /// The last: neither.
    pub const PLAIN: Mode = Mode {
        quality: false,
        flat_corners: false,
    };
}

/// Where the rounds start from: the chain as halved so far, the Steiner
/// points so far, the round's number, counting towards [`MAX_ROUNDS`]
/// from the first try's start, and the rounds of refinement so far,
/// towards [`MAX_QUALITY_ROUNDS`].
#[derive(Debug, Clone)]
pub(super) struct Rounds {
    chain: Chain,
    steiner: Vec<DVec2>,
    round: usize,
    quality: usize,
}

impl Rounds {
    /// The first round, on `chain` (separated).
    pub(super) fn new(chain: Chain) -> Rounds {
        Rounds {
            chain,
            steiner: Vec::new(),
            round: 0,
            quality: 0,
        }
    }

    /// The round these start.
    #[cfg(test)]
    pub(super) fn round(&self) -> usize {
        self.round
    }
}

/// The region of the chain of `start` (separated) as triangles, from the
/// round `start` holds on, halving segments where a cap patch needs it,
/// and as `mode` says, refined for quality ([`quality`]) and moving
/// Steiner points in from loop vertices with flat corners. Returns the
/// chain as halved, with the caps; `was_refined` is set if a round of
/// refinement asked for anything.
///
/// A round with nothing to halve and no flat corners to mend is a round
/// of refinement, if refinement asks for anything, and then places no
/// ears' centroids. Halvings the chain refuses as too small don't count:
/// refinement's points can take those corners apart, and when it has
/// nothing more to add and such halvings are left, they fail the caps as
/// [`refused`] says; a segment halved too often fails them at once.
///
/// Without flat corners, `fork` is set to the state of the first round
/// that found a flat corner, taken before the round changed anything:
/// running from there with flat corners gives what running from `start`
/// with them would, as no round before differs. That holds only as long
/// as the flat corners found are all the flag changes in a round.
pub(super) fn triangulate(
    start: Rounds,
    margin: f64,
    mode: Mode,
    fork: &mut Option<Rounds>,
    was_refined: &mut bool,
    work: &mut Work,
) -> Result<(Chain, Cap), KernelError> {
    let Rounds {
        mut chain,
        mut steiner,
        mut round,
        mut quality,
    } = start;
    loop {
        if round >= MAX_ROUNDS {
            return Err(KernelError::TooComplex);
        }
        let (segs, starts) = chain.flat();
        let n = segs.len();
        if n.saturating_add(steiner.len()) > MAX_PATCHES / 4 {
            return Err(KernelError::TooComplex);
        }
        work.spend((n + steiner.len()).saturating_mul(TRIANGULATION_WORK))?;
        let tris = region(&segs, &starts, &steiner)?;
        work.spend(tris.len())?;
        let at = |v: u32| {
            let v = v as usize;
            if v < n {
                segs[v].conic.p0
            } else {
                steiner[v - n]
            }
        };
        let (mut split, ears, mut flat) = mend(&segs, &starts, &at, &tris, margin);
        if !mode.flat_corners && !flat.is_empty() {
            if fork.is_none() {
                *fork = Some(Rounds {
                    chain: chain.clone(),
                    steiner: steiner.clone(),
                    round,
                    quality,
                });
            }
            flat.clear();
        }
        // Pieces halved too often are mending that doesn't converge, and
        // fail; pieces too small to halve are left to refinement.
        split.sort_unstable();
        split.dedup();
        let splittable = |&s: &u32| chain.splittable(&segs[s as usize], MAX_CAP_DEPTH, false);
        let stuck = |split: &[u32]| -> Vec<Seg> {
            split
                .iter()
                .filter(|s| !splittable(s))
                .map(|&s| segs[s as usize])
                .collect()
        };
        if split
            .iter()
            .any(|&s| !splittable(&s) && !chain.too_small(&segs[s as usize]))
        {
            return Err(refused(&chain, &stuck(&split)));
        }
        // With nothing to halve and no flat corners, refinement goes before
        // the ears' centroids: its points break up ears too (a fine
        // circle's ears all share one circumcircle, whose centre takes
        // them all), where a centroid, as close to the loop as the ear is
        // thin, would make every triangle at it thin, and refinement then
        // halve the chords near it over and over.
        if mode.quality
            && !split.iter().any(splittable)
            && flat.is_empty()
            && quality < MAX_QUALITY_ROUNDS
        {
            let caps = quality::Caps {
                chain: &chain,
                segs: &segs,
                starts: &starts,
                at: &at,
                tris: &tris,
                steiner: &steiner,
                margin,
            };
            let refined = caps.refine(work)?;
            if !refined.is_empty() {
                *was_refined = true;
                quality += 1;
                quality::halve(&mut chain, refined.split, work)?;
                steiner.extend(refined.added);
                continue;
            }
        }
        let places = Places {
            segs: &segs,
            starts: &starts,
            at: &at,
            tris: &tris,
            steiner: &steiner,
            margin,
        };
        let added = places.place(&ears, flat, &mut split, work)?;
        split.sort_unstable();
        split.dedup();
        let stuck = stuck(&split);
        split.retain(splittable);
        if split.is_empty() && added.is_empty() {
            if !stuck.is_empty() {
                return Err(refused(&chain, &stuck));
            }
            return Ok((chain, Cap { steiner, tris }));
        }
        // Only segments that may be halved are left.
        chain.split(&split, MAX_CAP_DEPTH, false, refused)?;
        steiner.extend(added);
        round += 1;
    }
}

/// Why the caps can't halve the curved segments `unsplittable` (in the
/// chain's order): [`ProfileError::TooFine`] naming the input segment of
/// the first that is [`too_small`](Chain::too_small), detail too small
/// for the resolution, or [`KernelError::TooComplex`] if each was halved
/// [`MAX_CAP_DEPTH`] times, mending that doesn't converge. A small one
/// wins wherever it comes among them.
pub(super) fn refused(chain: &Chain, unsplittable: &[Seg]) -> KernelError {
    match unsplittable.iter().find(|s| chain.too_small(s)) {
        Some(s) => {
            let (l, s) = chain.sides[s.side as usize].at;
            ProfileError::TooFine(l, s).into()
        }
        None => KernelError::TooComplex,
    }
}

/// The triangulation: its point location walks down a hierarchy of
/// coarser ones, so it takes about `log n` steps wherever the points go.
type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>, (), (), (), HierarchyHintGenerator<f64>>;

/// The order the points `0..n` are inserted in: a fixed shuffle, the
/// same on every platform. In the loops' order, a hole's vertices after
/// the outline's around it, each insertion would take apart most of the
/// triangles the last one made; in a random order an insertion takes
/// about a constant number, whatever the input.
fn insertion_order(n: usize) -> Vec<u32> {
    let mut order: Vec<u32> = (0..n as u32).collect();
    // SplitMix64.
    let mut state = 0u64;
    for i in (1..n).rev() {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        order.swap(i, (z % (i as u64 + 1)) as usize);
    }
    order
}

/// A spade point, with coordinates below `1e-30` taken as zero: spade
/// refuses non-zero ones below about `1e-43`. The triangulation only
/// decides which triangles there are, and moving points by less than
/// `1e-30` changes only triangles with corners that close to a line:
/// slivers along the convex hull, outside the region, as the chain's
/// separation keeps every vertex far from the chords it doesn't end.
fn point(p: DVec2) -> Point2<f64> {
    let flush = |x: f64| if x.abs() < 1e-30 { 0.0 } else { x };
    Point2::new(flush(p.x), flush(p.y))
}

/// The triangles of the region: the constrained Delaunay triangulation of
/// the chords and the Steiner points, the triangles where the loops wind
/// once. Loops that wind anywhere other than 0 or 1 times don't nest.
fn region(segs: &[Seg], starts: &[u32], steiner: &[DVec2]) -> Result<Vec<[u32; 3]>, KernelError> {
    let triangulation = || KernelError::Profile(ProfileError::Triangulation);
    let mut cdt = Cdt::new();
    let points: Vec<DVec2> = segs
        .iter()
        .map(|s| s.conic.p0)
        .chain(steiner.iter().copied())
        .collect();
    // Spade numbers its vertices as they come: `ours` maps them back.
    let mut handles = vec![FixedVertexHandle::from_index(0); points.len()];
    let mut ours = vec![u32::MAX; points.len()];
    for i in insertion_order(points.len()) {
        let handle = cdt
            .insert(point(points[i as usize]))
            .map_err(|_| triangulation())?;
        // A repeated point comes back as the vertex already there.
        match ours.get_mut(handle.index()) {
            Some(slot) if *slot == u32::MAX => *slot = i,
            _ => return Err(triangulation()),
        }
        handles[i as usize] = handle;
    }
    let next = next_in_loop(starts);
    let v = |i: u32| handles[i as usize];
    for i in 0..segs.len() as u32 {
        if cdt.try_add_constraint(v(i), v(next(i))).is_empty() {
            return Err(triangulation());
        }
    }
    // A point exactly on a chord would have split it.
    for i in 0..segs.len() as u32 {
        if !cdt.exists_constraint(v(i), v(next(i))) {
            return Err(triangulation());
        }
    }
    let ours = |v: FixedVertexHandle| ours[v.index()];

    // Each inner face's corners and, per side, the face across it and
    // the side's ends, counter-clockwise.
    let outer = cdt.outer_face().fix().index();
    let nf = cdt.num_all_faces();
    let mut corners = vec![None; nf];
    let mut across = vec![[(0usize, 0u32, 0u32); 3]; nf];
    for face in cdt.inner_faces() {
        let f = face.fix().index();
        corners[f] = Some(face.vertices().map(|v| ours(v.fix())));
        for (k, e) in face.adjacent_edges().into_iter().enumerate() {
            let (a, b) = (ours(e.from().fix()), ours(e.to().fix()));
            across[f][k] = (e.rev().face().fix().index(), a, b);
        }
    }

    // How the winding number changes from the right of the side `a → b`
    // to its left: by one across a chord run forwards, back by one across
    // a chord run backwards.
    let step = |a: u32, b: u32| -> i32 {
        match chord(starts, a, b) {
            Some((_, true)) => 1,
            Some((_, false)) => -1,
            None => 0,
        }
    };
    let nesting = || KernelError::Profile(ProfileError::Nesting);
    let mut winding: Vec<Option<i32>> = vec![None; nf];
    let mut queue = VecDeque::new();
    let set = |winding: &mut [Option<i32>], queue: &mut VecDeque<usize>, f: usize, value| {
        match winding[f] {
            None => {
                winding[f] = Some(value);
                queue.push_back(f);
                Ok(())
            }
            Some(w) if w == value => Ok(()),
            Some(_) => Err(nesting()),
        }
    };
    // The outer face, around the hull, winds 0 times.
    for f in 0..nf {
        if corners[f].is_some() {
            for &(g, a, b) in &across[f] {
                if g == outer {
                    set(&mut winding, &mut queue, f, step(a, b))?;
                }
            }
        }
    }
    while let Some(f) = queue.pop_front() {
        let w = winding[f].expect("queued faces have a winding");
        for &(g, a, b) in &across[f] {
            if g != outer {
                set(&mut winding, &mut queue, g, w - step(a, b))?;
            }
        }
    }
    let mut tris = Vec::new();
    for (f, corners) in corners.iter().enumerate() {
        if let Some(corners) = corners {
            match winding[f] {
                Some(0) => {}
                Some(1) => tris.push(*corners),
                _ => return Err(nesting()),
            }
        }
    }
    if tris.is_empty() {
        return Err(nesting());
    }
    Ok(tris)
}

/// What the triangles need mended: the segments to halve, the triangles
/// (by index) to put a Steiner point in, and the loop vertices with a
/// flat corner, where a Steiner point can be moved in from the vertex:
/// flat for a resolution of `margin`. Only the last depends on `margin`.
fn mend(
    segs: &[Seg],
    starts: &[u32],
    at: &impl Fn(u32) -> DVec2,
    tris: &[[u32; 3]],
    margin: f64,
) -> (Vec<u32>, Vec<usize>, Vec<u32>) {
    let curved = |e: Option<(u32, bool)>| e.is_some_and(|(s, _)| segs[s as usize].curved);
    let mut split = Vec::new();
    let mut ears = Vec::new();
    let mut flat = Vec::new();
    for (t, tri) in tris.iter().enumerate() {
        for k in 0..3 {
            let v = tri[k];
            if v as usize >= segs.len() {
                continue;
            }
            let o = at(v);
            let (a, b) = (at(tri[(k + 1) % 3]), at(tri[(k + 2) % 3]));
            // Obtuse at `v`, whose distance from the side `a–b` is the
            // cross product over that side's length.
            let (ea, eb) = (a - o, b - o);
            if ea.dot(eb) < 0.0 && ea.perp_dot(eb).abs() < FLAT * margin * (b - a).length() {
                flat.push(v);
            }
        }
        let sides = [0, 1, 2].map(|k| chord(starts, tri[k], tri[(k + 1) % 3]));
        if !sides.iter().any(|&e| curved(e)) {
            continue;
        }
        let (mut ear, mut narrow) = (false, false);
        for k in 0..3 {
            let (v, a, b) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
            // The side into `v` is stored from `b`: flip which end is `v`.
            let (ea, eb) = (sides[k], sides[(k + 2) % 3].map(|(s, start)| (s, !start)));
            if !curved(ea) && !curved(eb) {
                continue;
            }
            // Along the side from `v` to `w`: its curve's tangent there.
            let direction = |e: Option<(u32, bool)>, w: u32| match e {
                None => at(w) - at(v),
                Some((s, start)) => segs[s as usize].tangent(start),
            };
            let (da, db) = (direction(ea, a), direction(eb, b));
            if da.perp_dot(db) > SIN_MIN * da.length() * db.length() {
                continue;
            }
            if ea.is_some() && eb.is_some() {
                ear = true;
            } else {
                narrow = true;
                let (s, _) = if curved(ea) { ea } else { eb }.expect("a curved chord");
                split.push(s);
            }
        }
        if ear {
            ears.push(t);
        } else if !narrow && patch(segs, at, tri, &sides).fold_direction().is_none() {
            // Open corners but a fold inside (a bulge of weight above 1
            // into the triangle can): halve its curves.
            split.extend(
                sides
                    .iter()
                    .filter(|&&e| curved(e))
                    .map(|e| e.expect("a curved chord").0),
            );
        }
    }
    flat.sort_unstable();
    flat.dedup();
    (split, ears, flat)
}

/// The cap patch on triangle `tri`, in the plane `z = 0`, with its
/// `sides` (the segment along each, if any) as its edges.
fn patch(
    segs: &[Seg],
    at: &impl Fn(u32) -> DVec2,
    tri: &[u32; 3],
    sides: &[Option<(u32, bool)>; 3],
) -> Patch {
    let p = tri.map(|v| at(v).extend(0.0));
    let mut c = [0, 1, 2].map(|k| (p[k] + p[(k + 1) % 3]) * 0.5);
    let mut w = [1.0; 3];
    for k in 0..3 {
        if let Some((s, _)) = sides[k] {
            let seg = &segs[s as usize];
            if seg.curved {
                c[k] = seg.conic.c.extend(0.0);
                w[k] = seg.conic.w;
            }
        }
    }
    Patch { p, c, w }
}

/// What placing Steiner points looks at: one round's segments, their
/// loops' starts, the vertices' positions, the region's triangles and
/// the Steiner points so far.
struct Places<'a, F> {
    segs: &'a [Seg],
    starts: &'a [u32],
    at: &'a F,
    tris: &'a [[u32; 3]],
    steiner: &'a [DVec2],
    margin: f64,
}

impl<F: Fn(u32) -> DVec2 + Sync> Places<'_, F> {
    /// The Steiner points to add: for the loop vertices with a flat
    /// corner, points moved in from them ([`Self::displaced`]), and for
    /// the `ears` none of whose corners got one, their centroids, except
    /// where one lies within the resolution of the hull of a segment
    /// bulging into the region: that segment goes into `split` instead,
    /// to be halved.
    fn place(
        &self,
        ears: &[usize],
        flat: Vec<u32>,
        split: &mut Vec<u32>,
        work: &mut Work,
    ) -> Result<Vec<DVec2>, KernelError> {
        let (segs, at, margin) = (self.segs, self.at, self.margin);
        let (mut added, got) = self.displaced(flat, work)?;
        let concave: Vec<u32> = (0..segs.len() as u32)
            .filter(|&s| segs[s as usize].concave())
            .collect();
        let bvh = Bvh::new(
            concave
                .iter()
                .map(|&s| Bounds3::around(&segs[s as usize].hull()).expect("three points"))
                .collect(),
        );
        let mut near = Vec::new();
        for &t in ears {
            let [a, b, c] = self.tris[t];
            if [a, b, c].iter().any(|v| got.binary_search(v).is_ok()) {
                continue;
            }
            let centroid = (at(a) + at(b) + at(c)) / 3.0;
            let p = centroid.extend(0.0);
            near.clear();
            bvh.query(&Bounds3::point(p), margin, &mut near);
            work.spend(near.len())?;
            let inside = near
                .iter()
                .map(|&i| concave[i as usize])
                .find(|&s| !apart(&segs[s as usize].hull(), &[p], margin));
            match inside {
                Some(s) => split.push(s),
                None => added.push(centroid),
            }
        }
        Ok(added)
    }

    /// Steiner points for the loop vertices `flat` (sorted): each moved
    /// into the region from its vertex, along the bisector of the
    /// tangents there, by half the shorter chord meeting there (or by
    /// [`DISPLACE`]'s smaller fractions of that), where its way in
    /// crosses no segment's hull, so it lies in the region, and it is more
    /// than half that distance, and [`MIN_CLEAR`] resolutions, from every
    /// segment's hull and every other Steiner point, which halving
    /// segments keeps. A vertex with a Steiner point
    /// already within its distance gets none, so the rounds don't add
    /// points at the same corner for ever. Of two new points too close to
    /// each other, the one for the later vertex is dropped.
    ///
    /// Returns the points and the vertices they are for, in order.
    fn displaced(
        &self,
        flat: Vec<u32>,
        work: &mut Work,
    ) -> Result<(Vec<DVec2>, Vec<u32>), KernelError> {
        if flat.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let segs = self.segs;
        let flat3 = |p: DVec2| Bounds3::point(p.extend(0.0));
        let hulls = Bvh::new(
            segs.iter()
                .map(|s| Bounds3::around(&s.hull()).expect("three points"))
                .collect(),
        );
        let old = Bvh::new(self.steiner.iter().map(|&p| flat3(p)).collect());
        work.spend((segs.len() + self.steiner.len()).saturating_add(flat.len()))?;
        let found = par_map(&flat, |&v| self.candidate(v, &hulls, &old));
        let candidates: Vec<(u32, DVec2, f64)> = flat
            .iter()
            .zip(found)
            .filter_map(|(&v, c)| c.map(|(p, clear)| (v, p, clear)))
            .collect();

        // The candidates each within the other's clearance, found from
        // both sides.
        let boxes = Bvh::new(candidates.iter().map(|&(_, p, _)| flat3(p)).collect());
        let ids: Vec<u32> = (0..candidates.len() as u32).collect();
        let close = par_map(&ids, |&i| {
            let (_, p, clear) = candidates[i as usize];
            let mut near = Vec::new();
            boxes.query(&flat3(p), clear, &mut near);
            near.retain(|&j| j != i && candidates[j as usize].1.distance(p) <= clear);
            near
        });
        work.spend(close.iter().map(Vec::len).fold(0, usize::saturating_add))?;
        let mut blocked = vec![false; candidates.len()];
        let mut accepted = vec![false; candidates.len()];
        let (mut points, mut vertices) = (Vec::new(), Vec::new());
        for (i, close) in close.iter().enumerate() {
            if blocked[i]
                || close
                    .iter()
                    .any(|&j| (j as usize) < i && accepted[j as usize])
            {
                continue;
            }
            accepted[i] = true;
            for &j in close {
                if j as usize > i {
                    blocked[j as usize] = true;
                }
            }
            let (v, p, _) = candidates[i];
            points.push(p);
            vertices.push(v);
        }
        Ok((points, vertices))
    }

    /// The point moved in from loop vertex `v` for [`Self::displaced`],
    /// and its clearance, if one of [`DISPLACE`]'s distances gives one.
    /// `hulls` holds the segments' boxes, `old` the Steiner points'.
    ///
    /// The way in from `v` starts into the region, between the two
    /// segments' tangents; the point is in the region when that way
    /// crosses no segment's hull. The two segments at `v` have it as a
    /// corner of their hulls, which a line from `v` enters at once or
    /// never.
    fn candidate(&self, v: u32, hulls: &Bvh, old: &Bvh) -> Option<(DVec2, f64)> {
        let (segs, at, margin) = (self.segs, self.at, self.margin);
        let l = self.starts.partition_point(|&s| s <= v) - 1;
        let before = if v == self.starts[l] {
            self.starts[l + 1] - 1
        } else {
            v - 1
        };
        let (prev, seg) = (&segs[before as usize], &segs[v as usize]);
        let chord = |s: &Seg| (s.conic.p1 - s.conic.p0).length();
        let half = 0.5 * chord(prev).min(chord(seg));
        let o = at(v);
        let point = |p: DVec2| Bounds3::point(p.extend(0.0));
        let mut near = Vec::new();
        old.query(&point(o), half, &mut near);
        if near
            .iter()
            .any(|&i| self.steiner[i as usize].distance(o) <= half)
        {
            return None;
        }
        // The region lies counter-clockwise from the way out along `seg`
        // to the way back along `prev`: into it along their bisector.
        let (out, back) = (
            seg.tangent(true).normalize(),
            prev.tangent(false).normalize(),
        );
        let inward = (out - back).perp().normalize();
        if !inward.is_finite() {
            return None;
        }
        DISPLACE.iter().find_map(|&f| {
            let distance = half * f;
            let clear = distance * 0.5;
            if clear <= MIN_CLEAR * margin {
                return None;
            }
            let p = o + inward * distance;
            let (p3, way) = ([p.extend(0.0)], [o.extend(0.0), p.extend(0.0)]);
            near.clear();
            hulls.query(&point(o).include(p.extend(0.0)), clear, &mut near);
            let clear_of_hulls = near.iter().all(|&s| {
                let hull = segs[s as usize].hull();
                if s == v || s == before {
                    // Not entered at `v`: `inward` isn't strictly between
                    // the ways to its other two control points.
                    let [a, b] =
                        [hull[1], hull[2 - 2 * usize::from(s == before)]].map(|q| q.truncate() - o);
                    let (ab, ai, ib) = (a.perp_dot(b), a.perp_dot(inward), inward.perp_dot(b));
                    let enters = ab != 0.0 && ai * ab > 0.0 && ib * ab > 0.0;
                    !enters && apart(&hull, &p3, clear)
                } else {
                    apart(&hull, &way, self.margin) && apart(&hull, &p3, clear)
                }
            });
            near.clear();
            old.query(&point(p), clear, &mut near);
            let clear_of_old = near
                .iter()
                .all(|&i| self.steiner[i as usize].distance(p) > clear);
            (clear_of_hulls && clear_of_old).then_some((p, clear))
        })
    }
}
