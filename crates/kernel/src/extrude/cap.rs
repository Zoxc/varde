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
//! - When nothing else is left to mend, a run of refinement for
//!   quality ([`quality`]): Steiner points at the circumcentres of
//!   triangles with an angle under 5°, inserted one by one, chords
//!   (straight ones too) halved where those would encroach on them. Fine
//!   polygons' fans and ears, and a plate's fans out of a corner to holes
//!   on a common tangent, become triangles a later cut, or the walls,
//!   can't come too close to.
//! - Last, caps whose triangles' boxes still crowd each other (a fan the
//!   exemptions left, as of a polygon whose sides are too short to
//!   refine at, or the plain caps of the last try) are refined once more
//!   for that, without exemptions or halving ([`crowded`]): repair would
//!   count every pair of those boxes first and run out of budget.
//!
//! A Steiner point inside the hull of a segment bulging into the region
//! could end up outside the region once that segment is halved, so that
//! segment is halved instead. The first round triangulates the chain;
//! each round after adds what the one before asked for to that
//! triangulation ([`Live`]), points inserted and halved segments' chords
//! replaced, so a round costs what it changes and the region's flood
//! rather than a triangulation made afresh; caps under [`FRESH`] vertices
//! are triangulated afresh all the same. The rounds stop when nothing
//! changes, or fail after [`MAX_ROUNDS`] rounds of mending (runs of
//! refinement count apart, up to [`MAX_QUALITY_ROUNDS`], after which the
//! caps stand as they are). A segment to halve that already spans less
//! than [`MIN_SPLIT`](crate::mesh::MIN_SPLIT) resolutions, or, refining,
//! was halved [`MAX_MEND_DEPTH`] times, is left to refinement, whose
//! points can take its corner apart; if they don't,
//! it fails the profile with [`ProfileError::TooFine`]: detail too small
//! for the resolution, which a finer tolerance mends. One halved
//! [`MAX_CAP_DEPTH`] times fails with [`KernelError::TooComplex`] at
//! once.
//!
//! Flat corners are the only thing the two tries do differently, and up
//! to the first round that finds one they do the same, work counted
//! included. So the first try keeps the state that round started from
//! (a [`Rounds`], its fork, with a copy of the triangulation: one made
//! afresh from the same points can differ where four lie on a circle)
//! and the second try resumes from it rather than from the start; with
//! no fork it would repeat the first try and isn't made.

use std::collections::VecDeque;

use glam::DVec2;
use spade::handles::{FixedFaceHandle, FixedVertexHandle, InnerTag};
use spade::{
    ConstrainedDelaunayTriangulation, HierarchyHintGenerator, Point2, PositionInTriangulation,
    Triangulation,
};

use super::chain::{Chain, SIN_MIN, Seg, chord, next_in_loop};
use crate::budget::Work;
use crate::mesh::{Bvh, apart};
use crate::par::par_map;
use crate::patch::{Bounds3, Patch};
use crate::profile::ProfileError;
use crate::{KernelError, MAX_PATCHES};

mod quality;

use quality::{Bound, MAX_QUALITY_ROUNDS};

/// The most rounds of triangulating and mending.
const MAX_ROUNDS: usize = 32;

/// How many times a segment may have been halved, all told, before the
/// caps give up halving it: mending that doesn't converge would otherwise
/// double the segments it can't mend every round.
pub(super) const MAX_CAP_DEPTH: u8 = 16;

/// How many times, all told, a segment may have been halved before the
/// caps, refining for quality, leave a corner it makes too narrow to
/// refinement rather than halve it again: a corner between an arc and an
/// inner edge along its tangent to a far vertex (a row of holes close to
/// a plate's straight side, which has no vertices near) stays narrow
/// however often the arc is halved, while refinement's points, or its
/// halvings of the side they encroach on, give the arc a vertex near.
/// Halving on to [`MAX_CAP_DEPTH`] spent the rounds and the budget first.
const MAX_MEND_DEPTH: u8 = 6;

/// Work units per vertex for one triangulation: inserting a vertex walks
/// and flips a few edges.
pub(super) const TRIANGULATION_WORK: usize = 8;

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
/// points so far, their triangulation ([`Live`], none before the first
/// round), the round's number, counting towards [`MAX_ROUNDS`] from the
/// first try's start, the rounds of refinement so far, towards
/// [`MAX_QUALITY_ROUNDS`], whether refinement has nothing more to add to
/// that triangulation, and whether the caps were looked at for crowding.
#[derive(Debug, Clone)]
pub(super) struct Rounds {
    chain: Chain,
    steiner: Vec<DVec2>,
    live: Option<Live>,
    round: usize,
    quality: usize,
    settled: bool,
    gated: bool,
}

impl Rounds {
    /// The first round, on `chain` (separated).
    pub(super) fn new(chain: Chain) -> Rounds {
        Rounds {
            chain,
            steiner: Vec::new(),
            live: None,
            round: 0,
            quality: 0,
            settled: false,
            gated: false,
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
/// The first round triangulates the chain and the Steiner points; the
/// rounds after it add what the round before asked for to that
/// triangulation ([`Live`]): Steiner points inserted, segments halved
/// with their chords replaced.
///
/// A round with nothing to halve and no flat corners to mend is a round
/// of refinement, if refinement asks for anything, and then places no
/// ears' centroids. Halvings the chain refuses as too small (or, refining,
/// as halved [`MAX_MEND_DEPTH`] times) don't count:
/// refinement's points can take those corners apart, and when it has
/// nothing more to add and such halvings are left, they fail the caps as
/// [`refused`] says; a segment halved too often fails them at once. When
/// nothing is left to do, caps whose triangles' boxes crowd each other
/// ([`crowded`]) are refined once more for that, every narrow triangle
/// alike, and mended again.
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
        live: mut kept,
        mut round,
        mut quality,
        mut settled,
        mut gated,
    } = start;
    loop {
        if round >= MAX_ROUNDS {
            return Err(KernelError::TooComplex);
        }
        let (segs, starts) = chain.flat();
        let n = segs.len();
        let vertices = n.saturating_add(steiner.len());
        if vertices > MAX_PATCHES / 4 {
            return Err(KernelError::TooComplex);
        }
        // Small caps are triangulated afresh whenever a round changed
        // them, as cheaply as kept.
        let fresh = vertices < FRESH;
        let live = match &mut kept {
            Some(live) => live,
            None => {
                work.spend(vertices.saturating_mul(TRIANGULATION_WORK))?;
                kept.insert(Live::new(&segs, &starts, &steiner)?)
            }
        };
        let (tris, faces) = live.region(&segs, &starts)?;
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
                    live: Some(live.clone()),
                    round,
                    quality,
                    settled,
                    gated,
                });
            }
            flat.clear();
        }
        // Pieces halved too often are mending that doesn't converge, and
        // fail; pieces too small to halve are left to refinement, and so,
        // when refining, are pieces halved `MAX_MEND_DEPTH` times.
        split.sort_unstable();
        split.dedup();
        let depth = if mode.quality {
            MAX_MEND_DEPTH
        } else {
            MAX_CAP_DEPTH
        };
        let splittable = |&s: &u32| chain.splittable(&segs[s as usize], depth, false);
        let stuck = |split: &[u32]| -> Vec<Seg> {
            split
                .iter()
                .filter(|s| !splittable(s))
                .map(|&s| segs[s as usize])
                .collect()
        };
        let left = |s: &Seg| chain.too_small(s) || (mode.quality && s.depth < MAX_CAP_DEPTH);
        if split
            .iter()
            .any(|&s| !splittable(&s) && !left(&segs[s as usize]))
        {
            return Err(refused(&chain, &stuck(&split)));
        }
        let caps = quality::Caps {
            chain: &chain,
            segs: &segs,
            starts: &starts,
            tris: &tris,
            faces: &faces,
            steiner: &steiner,
            margin,
        };
        // With nothing to halve and no flat corners, refinement goes before
        // the ears' centroids: its points break up ears too (a fine
        // circle's ears all share one circumcircle, whose centre takes
        // them all), where a centroid, as close to the loop as the ear is
        // thin, would make every triangle at it thin, and refinement then
        // halve the chords near it over and over. Once it has run to the
        // end with nothing to halve, it has nothing more to add until a
        // round of mending changes the triangles.
        if mode.quality
            && !split.iter().any(splittable)
            && flat.is_empty()
            && !settled
            && quality < MAX_QUALITY_ROUNDS
        {
            let refined = caps.refine(live, Bound::Quality, work)?;
            settled = refined.split.is_empty();
            if !refined.is_empty() {
                *was_refined = true;
                quality += 1;
                steiner.extend(refined.added);
                quality::halve(&mut chain, live, refined.split, work)?;
                if fresh {
                    // Made afresh, the triangles may differ from those
                    // refinement settled.
                    kept = None;
                    settled = false;
                }
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
            if !gated {
                gated = true;
                if crowded(&tris, &at, margin, work)? {
                    let refined = caps.refine(live, Bound::Crowded, work)?;
                    if !refined.added.is_empty() {
                        steiner.extend(refined.added);
                        if fresh {
                            kept = None;
                        }
                        continue;
                    }
                }
            }
            return Ok((chain, Cap { steiner, tris }));
        }
        for &p in &added {
            live.insert(p)?;
        }
        work.spend(added.len().saturating_mul(TRIANGULATION_WORK))?;
        steiner.extend(added);
        // Only segments that may be halved are left.
        let pieces = chain.halved(&split, depth, false, refused)?;
        replace(&mut chain, live, &starts, &pieces, work)?;
        if fresh {
            kept = None;
        }
        settled = false;
        round += 1;
    }
}

/// Caps are crowded when their triangles' boxes come within the
/// resolution of each other in more pairs than this many times the
/// triangles, and than [`MIN_CROWDED`]: a fan of long thin triangles out
/// of one vertex, every two of whose boxes overlap, or a strip of them
/// between two fine outlines. Repair counts every such pair (of both caps,
/// and more with the walls) before anything else; refined, caps have a
/// few per triangle.
const CROWDED: usize = 32;

/// The fewest pairs of boxes that make caps [`CROWDED`].
const MIN_CROWDED: usize = 1 << 16;

/// Whether the triangles `tris`, with their vertices `at` their points,
/// are [`CROWDED`]: their boxes' pairs within `margin` counted, and
/// spent, up to the limit, along with the boxes.
fn crowded(
    tris: &[[u32; 3]],
    at: &(impl Fn(u32) -> DVec2 + Sync),
    margin: f64,
    work: &mut Work,
) -> Result<bool, KernelError> {
    work.spend(tris.len())?;
    let bvh = Bvh::new(
        tris.iter()
            .map(|t| Bounds3::around(&t.map(|v| at(v).extend(0.0))).expect("three points"))
            .collect(),
    );
    let most = tris.len().saturating_mul(CROWDED).max(MIN_CROWDED);
    let ids: Vec<u32> = (0..tris.len() as u32).collect();
    let pairs = bvh.count_pairs_up_to(&ids, margin, |i, j| j > i, most);
    work.spend(pairs)?;
    Ok(pairs > most)
}

/// Caps with fewer vertices than this at a round's start are
/// triangulated afresh in the next round if the round changed them,
/// rather than kept ([`Live`]): it costs at most 512 units a round, and
/// their triangulations stay the ones made afresh in a fixed order, even
/// where four points lie on a circle (a rectangle, a circle's arcs), so
/// the patches of small solids, and the booleans on them, are as they
/// were when every round triangulated afresh.
const FRESH: usize = 64;

/// Puts `pieces` (as [`Chain::halved`] gives them) in the place of the
/// segments they name, in the chain and in the triangulation, whose
/// chords run between `starts`' loops (the chain's before), spending
/// [`TRIANGULATION_WORK`] for each vertex they add.
fn replace(
    chain: &mut Chain,
    live: &mut Live,
    starts: &[u32],
    pieces: &[(u32, Vec<Seg>)],
    work: &mut Work,
) -> Result<(), KernelError> {
    let added = pieces
        .iter()
        .map(|(_, these)| these.len().saturating_sub(1))
        .fold(0, usize::saturating_add);
    work.spend(added.saturating_mul(TRIANGULATION_WORK))?;
    live.replace(starts, pieces)?;
    chain.replace(pieces);
    Ok(())
}

/// The region's triangles of the triangulation made afresh from
/// `chain`'s chords and `cap`'s Steiner points, and `cap`'s own, each
/// starting at its lowest corner, sorted: the same unless four of the
/// points lie on a circle.
#[cfg(test)]
pub(super) fn afresh(chain: &Chain, cap: &Cap) -> Result<[Vec<[u32; 3]>; 2], KernelError> {
    let (segs, starts) = chain.flat();
    let live = Live::new(&segs, &starts, &cap.steiner)?;
    let (fresh, _) = live.region(&segs, &starts)?;
    Ok([fresh, cap.tris.clone()].map(|mut tris| {
        for t in &mut tris {
            let k = (0..3).min_by_key(|&k| t[k]).expect("three corners");
            t.rotate_left(k);
        }
        tris.sort_unstable();
        tris
    }))
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

/// The constrained Delaunay triangulation of a round's chords and Steiner
/// points, kept from round to round: the points a round adds are
/// inserted, and a segment it halves has its chord's constraint removed,
/// the new vertices inserted and the pieces' chords made constraints.
/// Without points on four circles that is the triangulation made afresh;
/// with them, as on fine regular polygons, it is one of theirs. Spade's
/// vertices are mapped to ours both ways.
#[derive(Clone)]
pub(super) struct Live {
    cdt: Cdt,
    /// Spade's vertex for each of ours: the chain's, in [`Chain::flat`]'s
    /// order, then the Steiner points.
    handles: Vec<FixedVertexHandle>,
    /// Ours for each of spade's.
    ours: Vec<u32>,
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Live({} vertices)", self.handles.len())
    }
}

/// Spade couldn't make the triangulation asked for.
fn triangulation() -> KernelError {
    KernelError::Profile(ProfileError::Triangulation)
}

impl Live {
    /// The triangulation of the chords of `segs` (in loops from `starts`)
    /// and the `steiner` points.
    fn new(segs: &[Seg], starts: &[u32], steiner: &[DVec2]) -> Result<Live, KernelError> {
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
        let mut live = Live { cdt, handles, ours };
        let next = next_in_loop(starts);
        for i in 0..segs.len() as u32 {
            let (a, b) = (live.handles[i as usize], live.handles[next(i) as usize]);
            live.constrain(a, b)?;
        }
        Ok(live)
    }

    /// Makes the side `a`–`b` a constraint. A point exactly on it would
    /// split it; one crossing a constraint isn't added.
    fn constrain(&mut self, a: FixedVertexHandle, b: FixedVertexHandle) -> Result<(), KernelError> {
        if self.cdt.try_add_constraint(a, b).is_empty() || !self.cdt.exists_constraint(a, b) {
            return Err(triangulation());
        }
        Ok(())
    }

    /// Inserts `p` as a Steiner point, our next vertex, whose number it
    /// returns. It must lie in a triangle of the region, clear of every
    /// chord.
    pub(super) fn insert(&mut self, p: DVec2) -> Result<u32, KernelError> {
        let next = self.cdt.num_vertices();
        let handle = self.cdt.insert(point(p)).map_err(|_| triangulation())?;
        // A repeated point comes back as the vertex already there.
        if handle.index() != next {
            return Err(triangulation());
        }
        let ours = u32::try_from(self.handles.len()).map_err(|_| KernelError::TooComplex)?;
        self.handles.push(handle);
        self.ours.push(ours);
        Ok(ours)
    }

    /// Puts `pieces` (as [`Chain::halved`] gives them) in the place of the
    /// segments they name, whose chords run between `starts`' loops: each
    /// chord's constraint removed, the vertices between its pieces
    /// inserted, the pieces' chords made constraints, and ours renumbered
    /// as the chain will be. The pieces lie in their segment's control
    /// hull, which every other segment and every Steiner point keeps clear
    /// of, so their chords cross nothing.
    fn replace(&mut self, starts: &[u32], pieces: &[(u32, Vec<Seg>)]) -> Result<(), KernelError> {
        let n = *starts.last().expect("the end") as usize;
        let next = next_in_loop(starts);
        let mut middles = Vec::with_capacity(pieces.len());
        for (s, these) in pieces {
            let (a, b) = (self.handles[*s as usize], self.handles[next(*s) as usize]);
            let edge = self
                .cdt
                .get_edge_from_neighbors(a, b)
                .ok_or_else(triangulation)?
                .fix()
                .as_undirected();
            if !self.cdt.remove_constraint_edge(edge) {
                return Err(triangulation());
            }
            let mut chord = vec![a];
            for piece in &these[1..] {
                let next = self.cdt.num_vertices();
                let handle = self
                    .cdt
                    .insert_with_hint(point(piece.conic.p0), a)
                    .map_err(|_| triangulation())?;
                if handle.index() != next {
                    return Err(triangulation());
                }
                chord.push(handle);
            }
            chord.push(b);
            for side in chord.windows(2) {
                self.constrain(side[0], side[1])?;
            }
            middles.push(chord[1..chord.len() - 1].to_vec());
        }
        let mut handles = Vec::with_capacity(self.cdt.num_vertices());
        let mut middles = pieces.iter().map(|&(s, _)| s).zip(middles).peekable();
        for (i, &handle) in self.handles[..n].iter().enumerate() {
            handles.push(handle);
            if let Some((_, these)) = middles.next_if(|&(s, _)| s as usize == i) {
                handles.extend(these);
            }
        }
        handles.extend_from_slice(&self.handles[n..]);
        self.handles = handles;
        self.ours = vec![u32::MAX; self.cdt.num_vertices()];
        for (i, handle) in self.handles.iter().enumerate() {
            self.ours[handle.index()] = i as u32;
        }
        Ok(())
    }

    /// Face `face`'s corners, counter-clockwise, as our vertices.
    fn corners(&self, face: FixedFaceHandle<InnerTag>) -> [u32; 3] {
        self.cdt
            .face(face)
            .vertices()
            .map(|v| self.ours[v.fix().index()])
    }

    /// Whether `face` is still the triangle `tri`: a face spade changes
    /// gets a new corner.
    pub(super) fn is(&self, face: FixedFaceHandle<InnerTag>, tri: [u32; 3]) -> bool {
        let corners = self.corners(face);
        (0..3).any(|k| corners == [tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]])
    }

    /// The faces around our vertex `v`, with their corners.
    pub(super) fn around(&self, v: u32) -> Vec<(FixedFaceHandle<InnerTag>, [u32; 3])> {
        self.cdt
            .vertex(self.handles[v as usize])
            .out_edges()
            .filter_map(|e| e.face().as_inner())
            .map(|f| (f.fix(), self.corners(f.fix())))
            .collect()
    }

    /// The corners of the face `p` lies inside, if it lies inside one
    /// rather than on an edge or a vertex.
    pub(super) fn locate(&self, p: DVec2) -> Option<[u32; 3]> {
        match self.cdt.locate(point(p)) {
            PositionInTriangulation::OnFace(face) => Some(self.corners(face)),
            _ => None,
        }
    }

    /// The triangles of the region, the faces where the loops of `segs`
    /// (from `starts`) wind once, with spade's faces. Loops that wind
    /// anywhere other than 0 or 1 times don't nest.
    #[allow(clippy::type_complexity)]
    fn region(
        &self,
        segs: &[Seg],
        starts: &[u32],
    ) -> Result<(Vec<[u32; 3]>, Vec<FixedFaceHandle<InnerTag>>), KernelError> {
        let cdt = &self.cdt;
        let ours = |v: FixedVertexHandle| self.ours[v.index()];
        debug_assert_eq!(self.handles.len(), cdt.num_vertices());
        debug_assert!(segs.len() <= self.handles.len());

        // Each inner face's corners and, per side, the face across it and
        // the side's ends, counter-clockwise.
        let outer = cdt.outer_face().fix().index();
        let nf = cdt.num_all_faces();
        let mut corners = vec![None; nf];
        let mut across = vec![[(0usize, 0u32, 0u32); 3]; nf];
        for face in cdt.inner_faces() {
            let f = face.fix().index();
            corners[f] = Some((face.fix(), face.vertices().map(|v| ours(v.fix()))));
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
        let (mut tris, mut faces) = (Vec::new(), Vec::new());
        for (f, corners) in corners.iter().enumerate() {
            if let Some((face, corners)) = corners {
                match winding[f] {
                    Some(0) => {}
                    Some(1) => {
                        tris.push(*corners);
                        faces.push(*face);
                    }
                    _ => return Err(nesting()),
                }
            }
        }
        if tris.is_empty() {
            return Err(nesting());
        }
        Ok((tris, faces))
    }
}

/// What the triangles need mended: the segments to halve, the triangles
/// (by index) to put a Steiner point in, and the loop vertices with a
/// flat corner, where a Steiner point can be moved in from the vertex:
/// flat for a resolution of `margin`. Only the last depends on `margin`.
/// Each triangle is looked at alone, in parallel, and what they ask for
/// is gathered in their order.
fn mend(
    segs: &[Seg],
    starts: &[u32],
    at: &(impl Fn(u32) -> DVec2 + Sync),
    tris: &[[u32; 3]],
    margin: f64,
) -> (Vec<u32>, Vec<usize>, Vec<u32>) {
    let asks = par_map(tris, |tri| mend_one(segs, starts, at, tri, margin));
    let mut split = Vec::new();
    let mut ears = Vec::new();
    let mut flat = Vec::new();
    for (t, ask) in asks.into_iter().enumerate() {
        split.extend_from_slice(&ask.split[..ask.splits]);
        flat.extend_from_slice(&ask.flat[..ask.flats]);
        if ask.ear {
            ears.push(t);
        }
    }
    flat.sort_unstable();
    flat.dedup();
    (split, ears, flat)
}

/// What one triangle asks [`mend`] for: at most three segments to
/// halve (one per corner, or its curved sides), whether it is an ear to
/// put a Steiner point in, and at most three loop vertices with a flat
/// corner, each in the order found.
#[derive(Default)]
struct Ask {
    split: [u32; 3],
    splits: usize,
    ear: bool,
    flat: [u32; 3],
    flats: usize,
}

/// [`mend`] for the triangle `tri`.
fn mend_one(
    segs: &[Seg],
    starts: &[u32],
    at: &impl Fn(u32) -> DVec2,
    tri: &[u32; 3],
    margin: f64,
) -> Ask {
    let curved = |e: Option<(u32, bool)>| e.is_some_and(|(s, _)| segs[s as usize].curved);
    let mut ask = Ask::default();
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
            ask.flat[ask.flats] = v;
            ask.flats += 1;
        }
    }
    let sides = [0, 1, 2].map(|k| chord(starts, tri[k], tri[(k + 1) % 3]));
    if !sides.iter().any(|&e| curved(e)) {
        return ask;
    }
    let mut narrow = false;
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
            ask.ear = true;
        } else {
            narrow = true;
            let (s, _) = if curved(ea) { ea } else { eb }.expect("a curved chord");
            ask.split[ask.splits] = s;
            ask.splits += 1;
        }
    }
    if !ask.ear && !narrow && patch(segs, at, tri, &sides).fold_direction().is_none() {
        // Open corners but a fold inside (a bulge of weight above 1
        // into the triangle can): halve its curves.
        for e in sides.iter().filter(|&&e| curved(e)) {
            ask.split[ask.splits] = e.expect("a curved chord").0;
            ask.splits += 1;
        }
    }
    ask
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
