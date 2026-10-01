//! Assembling the result from the counts: new vertices as records, the
//! parts of edges and faces to keep, the cut edges between the two, and
//! each cut face triangulated in its parameter domain.
//!
//! Curves are kept as records by the vertices they join ([`Curves`]):
//! a curved edge kept whole keeps its own, a curved edge cut at crossings
//! is split once into pieces both faces beside it read, each cut arc's
//! chain (see [`super::chain`]) has its curves, and a cut curved
//! face's new inner edges theirs (see [`face`]). Straight edges have no
//! record.
//!
//! The faces are cut in rounds: a triangle along a cut that strays from
//! its patch by more than half the fit tolerance, or a curve on a face's
//! boundary that bulges out of the triangle it is a side of, gets that
//! curve halved (a cut's by its chain, an operand's edge by a vertex
//! added on it that both faces beside it get) and every face is cut
//! again. A triangle off its face's quadric (onto the face's copy
//! claiming no surface) by that much gets its sides on the face's
//! boundary halved the same way. What the last round keeps must be within
//! the fit tolerance, or the operation fails as too complex. Then
//! refinement's pieces that came through whole are merged back (see
//! [`merge`]).

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;

use super::chain::{self, Chain};
use super::cleanup::Soup;
use super::count::{Counts, Crossing};
use super::curved::solve::near_patch;
use super::input::{Input, Side};
use super::pairs::{Arc, first_ids};
use super::surface::{Crossed, Shape, lerp, on_curve, point, polish, straight};
use super::triangulate::Meter;
use super::{Op, Primitives, segment};
use crate::budget::Work;
use crate::mesh::{Edge, Face, MIN_SPLIT, Node, Surface};
use crate::par::par_map;
use crate::patch::{Conic3, Point};
use crate::{KernelError, Tolerance};

mod face;
mod merge;

use face::{Cut, FIRST_STEINER, Layout, cut_face, project};

/// How many rounds of halving curves the faces are cut in, at most (see
/// the [module](self) docs). The last may leave corners the
/// triangulation asked to split, for repair, and triangles straying from
/// their face by up to the fit tolerance; one straying further fails the
/// operation as too complex.
const SPLIT_ROUNDS: usize = 6;

/// Curves by the vertices they join, the lower id first.
pub(super) type Curves = BTreeMap<(u32, u32), Edge>;

/// The key of the edge between `a` and `b`.
fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

/// What an operation keeps of each operand, by winding number in the
/// other, and whether `B`'s faces turn over.
#[derive(Debug, Clone, Copy)]
struct Keep {
    /// The winding number of the parts of `A`, then `B`, kept.
    w: [i32; 2],
    flip_b: bool,
}

impl Keep {
    fn of(op: Op) -> Keep {
        match op {
            Op::Union => Keep {
                w: [0, 0],
                flip_b: false,
            },
            Op::Intersection => Keep {
                w: [1, 1],
                flip_b: false,
            },
            Op::Difference => Keep {
                w: [0, 1],
                flip_b: true,
            },
        }
    }

    fn keeps(self, side: Side, w: i32) -> bool {
        self.w[side as usize] == w
    }

    /// Whether a cut edge in a face of `side` runs from its end whose
    /// sign `s` is +1. The sign of an end, seen from `A`, is the crossing
    /// of an edge of `A` (as the face of `A` runs it) through the face of
    /// `B` there, and minus the crossing of an edge of `B` (as the face of
    /// `B` runs it) through the face of `A`. Keeping what is outside `B`,
    /// a face of `A` leaves the cut where its edge enters `B`; seen from
    /// `B` every sign is the other way round.
    fn starts_at_plus(self, side: Side) -> bool {
        let outside = self.w[side as usize] == 0;
        match side {
            Side::A => outside,
            Side::B => !outside,
        }
    }
}

/// The crossings along each edge of one operand, in order along it.
pub(super) struct Along {
    /// For each edge, where its crossings start in `verts` (one past the
    /// end is the next edge's start).
    start: Vec<u32>,
    /// The new vertices' ids, edge by edge, in order along the edge, with
    /// their parameters (never decreasing along an edge).
    verts: Vec<(u32, f64)>,
    /// Whether each piece is kept: an edge with `k` crossings has `k + 1`
    /// pieces, at `start[e] + e..`.
    kept: Vec<bool>,
    /// Each crossing's parameter as ordered, by crossing index.
    at: Vec<f64>,
}

impl Along {
    #[allow(clippy::too_many_arguments)]
    fn new(
        side: Side,
        input: &Input,
        other: &Input,
        crossings: &[Crossing],
        params: &[f64],
        first_id: u32,
        windings: &[i32],
        keep: Keep,
        prims: &impl Primitives,
        resolution: f64,
    ) -> Result<Along, KernelError> {
        let ne = input.edges.len();
        let mut start = vec![0u32; ne + 1];
        for c in crossings {
            start[c.edge as usize + 1] += 1;
        }
        for e in 0..ne {
            start[e + 1] += start[e];
        }
        let mut verts = Vec::with_capacity(crossings.len());
        let mut kept = Vec::with_capacity(crossings.len() + ne);
        let mut at_of = vec![0.0; crossings.len()];
        let mut i = 0;
        for e in 0..ne as u32 {
            let from = i;
            while i < crossings.len() && crossings[i].edge == e {
                i += 1;
            }
            let [s, _] = input.edges[e as usize];
            let conic = input.conic(e);
            let near = |k1: usize, k2: usize| {
                conic.eval(params[k1]).distance(conic.eval(params[k2])) <= resolution
            };
            // Crossings a little further apart along a stretch of the edge
            // lying on both faces' surfaces (within the resolution): where
            // the edge grazes them, a tangency, the crossings' places along
            // it are as good as unknown, and may be in either order.
            let grazing = |k1: usize, k2: usize| {
                let (t1, t2) = (params[k1], params[k2]);
                let points = [t1, (t1 + t2) / 2.0, t2].map(|t| conic.eval(t));
                points[0].distance(points[2]) <= MIN_SPLIT * resolution
                    && [k1, k2].iter().all(|&k| {
                        let shape = Shape::of(other, crossings[k].face);
                        points.iter().all(|&p| shape.distance(p) <= resolution)
                    })
            };
            let together = |k1: usize, k2: usize| near(k1, k2) || grazing(k1, k2);
            // By insertion, which can't fail however the order behaves:
            // by the crossings' places where they are apart, else as the
            // primitives order them (exactly, for flat operands).
            let mut here: Vec<usize> = Vec::with_capacity(i - from);
            for k in from..i {
                let at = here
                    .iter()
                    .position(|&x| {
                        if near(k, x) {
                            prims.order(side, e, &crossings[k], &crossings[x])
                                == std::cmp::Ordering::Less
                        } else {
                            params[k] < params[x]
                        }
                    })
                    .unwrap_or(here.len());
                here.insert(at, k);
            }
            alternate(
                &mut here,
                windings[s as usize],
                |k| crossings[k].x,
                together,
            );
            let mut w = windings[s as usize];
            // The parameters follow the order: rounding may have swapped
            // two that are close. Two apart in the wrong order would put a
            // vertex off the face it crosses.
            let mut at = 0.0f64;
            let mut last = None;
            // A crossing in and the next out (or out and in) a grazing
            // stretch apart go to one place: the piece between, within the
            // resolution of the surface, has no size to speak of, and at
            // one place the clean-up collapses it. Left apart, a
            // micrometre, it left triangles along the rim with three
            // corners that far apart, and one of zero width between the
            // edge, the rim and the vertex, which no split mends. The
            // place is where the edge is nearest both surfaces: one of the
            // two, midway, or an end of the edge within the stretch's
            // reach (where the edge touches the surface at its vertex).
            let mut place: Vec<f64> = here.iter().map(|&k| params[k]).collect();
            let mut j = 0;
            while j + 1 < here.len() {
                let (k1, k2) = (here[j], here[j + 1]);
                if crossings[k1].x != crossings[k2].x && !near(k1, k2) && grazing(k1, k2) {
                    let (t1, t2) = (params[k1], params[k2]);
                    let middle = conic.eval((t1 + t2) / 2.0);
                    let reach = MIN_SPLIT * resolution;
                    let shapes = [k1, k2].map(|k| Shape::of(other, crossings[k].face));
                    let off = |t: f64| {
                        let p = conic.eval(t);
                        shapes.iter().map(|s| s.distance(p)).fold(0.0, f64::max)
                    };
                    let best = [0.0, 1.0, t1, (t1 + t2) / 2.0, t2]
                        .into_iter()
                        .filter(|&t| conic.eval(t).distance(middle) <= reach)
                        .min_by(|&x, &y| off(x).total_cmp(&off(y)))
                        .unwrap_or((t1 + t2) / 2.0);
                    (place[j], place[j + 1]) = (best, best);
                    j += 2;
                } else {
                    j += 1;
                }
            }
            for (&k, &param) in here.iter().zip(&place) {
                kept.push(keep.keeps(side, w));
                w += i32::from(crossings[k].x);
                if param < at && last.is_some_and(|l| !together(l, k)) {
                    return Err(KernelError::Boolean(super::BooleanError::Inconsistent));
                }
                at = at.max(param);
                at_of[k] = at;
                last = Some(k);
                verts.push((first_id + k as u32, at));
            }
            kept.push(keep.keeps(side, w));
        }
        Ok(Along {
            start,
            verts,
            kept,
            at: at_of,
        })
    }

    /// The same with the vertices `added` (by edge: ids and parameters,
    /// ascending) among each edge's crossings: they split a piece in two,
    /// both kept or not as it was.
    fn with(&self, added: &BTreeMap<u32, Vec<(u32, f64)>>) -> Along {
        if added.is_empty() {
            return Along {
                start: self.start.clone(),
                verts: self.verts.clone(),
                kept: self.kept.clone(),
                at: self.at.clone(),
            };
        }
        let ne = self.start.len() - 1;
        let mut start = Vec::with_capacity(ne + 1);
        let mut verts = Vec::with_capacity(self.verts.len());
        let mut kept = Vec::with_capacity(self.kept.len());
        start.push(0);
        for e in 0..ne as u32 {
            let (old, old_kept) = self.of(e);
            let new = added.get(&e).map_or(&[][..], Vec::as_slice);
            let (mut i, mut j) = (0, 0);
            kept.push(old_kept[0]);
            while i < old.len() || j < new.len() {
                if j < new.len() && (i == old.len() || new[j].1 < old[i].1) {
                    verts.push(new[j]);
                    kept.push(old_kept[i]);
                    j += 1;
                } else {
                    verts.push(old[i]);
                    kept.push(old_kept[i + 1]);
                    i += 1;
                }
            }
            start.push(verts.len() as u32);
        }
        Along {
            start,
            verts,
            kept,
            at: self.at.clone(),
        }
    }

    /// Edge `e`'s crossings, in order, and whether each of its pieces is
    /// kept.
    pub(super) fn of(&self, e: u32) -> (&[(u32, f64)], &[bool]) {
        let (a, b) = (
            self.start[e as usize] as usize,
            self.start[e as usize + 1] as usize,
        );
        (
            &self.verts[a..b],
            &self.kept[a + e as usize..b + e as usize + 1],
        )
    }
}

/// How the pair decisions refined the operands, for merging the pieces
/// back where nothing cut them: each operand's tree of red splits, and
/// each refined triangle's leaf in it.
pub(super) struct Refinement<'a> {
    pub(super) tree: [&'a [Node]; 2],
    pub(super) leaf: [&'a [u32]; 2],
}

/// The result's triangles and curves, before clean-up, from the counts
/// and each pair of faces' cut `arcs`: vertices are the operands' (`A`'s,
/// then `B`'s), then the new ones on edges (`x12`'s, then `x21`'s), then
/// those along the cuts, arc by arc. With `refinement`, pieces of an
/// operand's triangle kept whole are merged back into it.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble(
    op: Op,
    a: &Input,
    b: &Input,
    counts: &Counts,
    arcs: &[Arc],
    prims: &impl Primitives,
    tol: &Tolerance,
    refinement: Option<&Refinement>,
    work: &mut Work,
) -> Result<(Soup, Vec<Face>), KernelError> {
    let keep = Keep::of(op);
    let [first12, first21] = first_ids(a, b, counts);

    // Ordering the crossings along each edge compares every two.
    for crossings in [&counts.x12, &counts.x21] {
        for run in crossings.chunk_by(|x, y| x.edge == y.edge) {
            work.spend(run.len().saturating_mul(run.len()))?;
        }
    }
    let (params12, units12) = params(a, b, &counts.x12, tol.resolution());
    let (params21, units21) = params(b, a, &counts.x21, tol.resolution());
    work.spend(units12.saturating_add(units21))?;
    let params = [params12, params21];
    let along = [
        Along::new(
            Side::A,
            a,
            b,
            &counts.x12,
            &params[0],
            first12,
            &counts.w03,
            keep,
            prims,
            tol.resolution(),
        )?,
        Along::new(
            Side::B,
            b,
            a,
            &counts.x21,
            &params[1],
            first21,
            &counts.w30,
            keep,
            prims,
            tol.resolution(),
        )?,
    ];
    let mut cutting = Cutting {
        a,
        b,
        counts,
        arcs,
        keep,
        tol,
        first: [first12, first21],
        along,
        base: Vec::new(),
    };
    cutting.base = cutting.positions();
    cutting.certify(work)?;

    // Each arc's chain.
    let chain_jobs: Vec<chain::Job> = arcs.iter().map(|arc| cutting.chain_job(arc)).collect();
    work.spend(
        chain_jobs
            .iter()
            .map(|j| {
                if j.planar[0] && j.planar[1] {
                    1
                } else {
                    crate::MAX_TRACE_STEPS / 64
                }
            })
            .sum(),
    )?;
    let mut chains: Vec<Chain> = par_map(&chain_jobs, |job| chain::chain(job, tol.fit()));
    work.spend(chains.iter().map(|c| c.curves.len()).sum())?;

    // The faces, in rounds: where a triangle along a cut strays from its
    // patch, or a curve on a face's boundary bulges out of the triangle
    // it is a side of, that curve is halved (a cut's by its chain, an
    // operand's edge by a vertex added on it, which both faces beside it
    // get) and the faces are cut again.
    let mut extras = [BTreeMap::new(), BTreeMap::new()];
    cutting.flush_extras(&chain_jobs, &chains, 0..arcs.len(), &mut extras);
    let mut round = 0;
    let last = loop {
        let cut = cutting.round(&extras, &chains, work)?;
        let done = cut.split.is_empty() && cut.more.iter().all(BTreeMap::is_empty);
        if done || round >= SPLIT_ROUNDS {
            // Halving couldn't bring every triangle within the fit
            // tolerance (a crossing off the surface keeps the bands at
            // it that far however small, and a band tree's root may have
            // no side to halve): past it, the result would be wrong by
            // the tolerance's own measure.
            if cut.stray > tol.fit() {
                return Err(KernelError::TooComplex);
            }
            break cut;
        }
        round += 1;
        work.spend(
            cut.split.values().map(Vec::len).sum::<usize>()
                + cut
                    .more
                    .iter()
                    .flat_map(|m| m.values())
                    .map(Vec::len)
                    .sum::<usize>(),
        )?;
        for (k, more) in cut.more.into_iter().enumerate() {
            for (e, ts) in more {
                let list = extras[k].entry(e).or_default();
                list.extend(ts);
                list.sort_by(f64::total_cmp);
                list.dedup();
            }
        }
        let wanted: Vec<(usize, Vec<usize>)> = cut
            .split
            .into_iter()
            .map(|(arc, mut segs)| {
                segs.sort_unstable();
                segs.dedup();
                (arc, segs)
            })
            .collect();
        let halved = par_map(&wanted, |(arc, segs)| {
            chains[*arc].split(&chain_jobs[*arc], segs, tol.fit())
        });
        for ((arc, _), chain) in wanted.iter().zip(halved) {
            chains[*arc] = chain;
        }
        // A flush rim's edge gets the halved chains' new vertices too, so
        // the two keep coming in the same pieces.
        cutting.flush_extras(
            &chain_jobs,
            &chains,
            wanted.iter().map(|(arc, _)| *arc),
            &mut extras,
        );
    };
    cutting.finish(last, refinement, work)
}

#[cfg(test)]
thread_local! {
    /// Tests only: whether, on this thread, crossings the search only
    /// placed are placed as they were before they went to a root on the
    /// patch crossed (on a plane its nearest root, on a quadric one within
    /// `1e-6`, else where the search put them; see [`params`]) and
    /// [`Cutting::certify`] is skipped, and the crossing searches of the
    /// counting made on this thread don't drop pieces by their control
    /// hulls or halve long ones (so they run out of pieces where they
    /// used to), to see what
    /// the rest makes of crossings off the surface.
    pub(super) static LOOSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The operands and what the counting made of them, which every round of
/// cutting the faces reads.
struct Cutting<'a> {
    a: &'a Input<'a>,
    b: &'a Input<'a>,
    counts: &'a Counts,
    arcs: &'a [Arc],
    keep: Keep,
    tol: &'a Tolerance,
    /// Where the crossings of `A`'s edges' ids start, then `B`'s.
    first: [u32; 2],
    /// Each operand's crossings along its edges.
    along: [Along; 2],
    /// The operands' vertices and the crossings, by id.
    base: Vec<DVec3>,
}

/// One round of cutting the faces: the vertices and curves so far, the
/// faces cut and what each gave, the faces kept whole, and the curves the
/// round asks to be halved.
struct Round {
    pos: Vec<DVec3>,
    curves: Curves,
    jobs: Vec<Cut>,
    cut: Vec<face::Cutout>,
    /// The triangles of the faces kept whole, their faces, and which
    /// triangle of which operand each is (for [`merge`]).
    tris: Vec<[u32; 3]>,
    faces: Vec<u32>,
    whole: Vec<Option<(Side, u32)>>,
    /// The chain curves to halve, by arc.
    split: BTreeMap<usize, Vec<usize>>,
    /// The parameters on the operands' edges to add vertices at.
    more: [BTreeMap<u32, Vec<f64>>; 2],
    /// How far the faces' triangles held to the fit tolerance stray, at
    /// most (see [`face::Cutout::stray`]).
    stray: f64,
}

/// The cuts' vertices and edges, as a round numbers them.
struct ChainEdges {
    /// Every vertex of each arc's chain, its ends included, by arc.
    ids: Vec<Vec<u32>>,
    /// The arc and index along it of each chain edge.
    owner: BTreeMap<(u32, u32), (usize, usize)>,
    /// The chain edges that were fitted.
    fitted: BTreeSet<(u32, u32)>,
}

impl Cutting<'_> {
    /// Each operand, and where its vertices start.
    fn operand(&self, side: Side) -> (&Input<'_>, u32) {
        match side {
            Side::A => (self.a, 0),
            Side::B => (self.b, self.a.mesh.verts().len() as u32),
        }
    }

    /// Where the operands' vertices and the crossings are: the crossings
    /// from their parameters as ordered along the edges, and exactly on
    /// the face crossed where it lies in a plane square to an axis (so
    /// flush faces of results fed on stay flush): each coordinate all six
    /// control points of the crossed patch share is set to theirs, which
    /// only ever takes off rounding. The patch's corners alone won't do:
    /// a wall over an arc whose ends are level has three corners at one
    /// height while it bulges off it.
    fn positions(&self) -> Vec<DVec3> {
        let (a, b, counts) = (self.a, self.b, self.counts);
        let mut base: Vec<DVec3> = a
            .mesh
            .verts()
            .iter()
            .chain(b.mesh.verts())
            .copied()
            .collect();
        base.resize(self.first[1] as usize + counts.x21.len(), DVec3::ZERO);
        for (input, other, along, crossings, first) in [
            (a, b, &self.along[0], &counts.x12, self.first[0]),
            (b, a, &self.along[1], &counts.x21, self.first[1]),
        ] {
            for e in 0..input.edges.len() as u32 {
                let snap = |p: DVec3, id: u32| {
                    let face = crossings[(id - first) as usize].face;
                    let q = p.shared(&other.patches[face as usize].hull());
                    debug_assert!(
                        (q - p).abs().max_element() <= self.tol.resolution(),
                        "a crossing moved from {p} to {q}"
                    );
                    q
                };
                let conic = input.conic(e);
                for &(id, t) in along.of(e).0 {
                    base[id as usize] = snap(point(&conic, t), id);
                }
            }
        }
        base
    }

    /// Checks that each crossing the search didn't solve, only placed
    /// for the count (where it found the two meeting but kept nothing,
    /// else where they came closest), lies on the other operand's
    /// surface: within the resolution of the patch it crosses, or of
    /// another of that operand's patches near it (a crossing through the
    /// side two patches share lands on either), by a certified distance.
    /// Its vertex is on its edge, and on the other surface where it went
    /// to a root on the patch crossed ([`params`]), but not on a face
    /// claiming no surface or where no root of its sign is on the patch.
    /// One farther, or past the search's cap, fails the operation as
    /// `Inconsistent`.
    fn certify(&self, work: &mut Work) -> Result<(), KernelError> {
        #[cfg(test)]
        if LOOSE.get() {
            return Ok(());
        }
        let mut asked = Vec::new();
        for (crossings, other, first) in [
            (&self.counts.x12, self.b, self.first[0]),
            (&self.counts.x21, self.a, self.first[1]),
        ] {
            for (k, c) in crossings.iter().enumerate() {
                if !c.solved {
                    asked.push((other, c.face, first + k as u32));
                }
            }
        }
        let resolution = self.tol.resolution();
        let near = par_map(&asked, |&(other, face, id)| {
            let x = self.base[id as usize];
            let (d, mut cost) = near_patch(x, &other.patches[face as usize], resolution);
            if d.is_some() {
                return (true, cost.div_ceil(NEAR_NODES_PER_UNIT));
            }
            let mut units = cost.div_ceil(NEAR_NODES_PER_UNIT) + other.boxes.len() / 64;
            for (t, b) in other.boxes.iter().enumerate() {
                if t == face as usize || (b.min - x).max(x - b.max).max_element() > resolution {
                    continue;
                }
                let d;
                (d, cost) = near_patch(x, &other.patches[t], resolution);
                units += cost.div_ceil(NEAR_NODES_PER_UNIT);
                if d.is_some() {
                    return (true, units);
                }
            }
            (false, units)
        });
        work.spend(
            near.iter()
                .map(|&(_, units)| units)
                .fold(0, usize::saturating_add),
        )?;
        if near.iter().any(|&(on, _)| !on) {
            return Err(KernelError::Boolean(super::BooleanError::Inconsistent));
        }
        Ok(())
    }

    /// What tracing and fitting `arc`'s chain needs.
    fn chain_job(&self, arc: &Arc) -> chain::Job<'_> {
        let (a, b) = (self.a, self.b);
        let [p, q] = arc.tris;
        let ends = [arc.plus, arc.minus];
        let dom = ends.map(|id| [self.place(id, Side::A, p), self.place(id, Side::B, q)]);
        chain::Job {
            p: &a.patches[p as usize],
            q: &b.patches[q as usize],
            shapes: [Shape::of(a, p), Shape::of(b, q)],
            planar: [a.planar[p as usize], b.planar[q as usize]],
            ends: ends.map(|id| self.base[id as usize]),
            dom,
            on_p: ends.map(|id| id < self.first[1]),
            flip_q: self.keep.flip_b,
        }
    }

    /// Vertices to add on the operands' curved edges that a cut runs
    /// along, where a plane meets a quadric flush with an edge on both (a
    /// cap's rim on the other's cap): at the cut's vertices, so the edge
    /// and the cut come in the same pieces, which lie on each other and
    /// which the clean-up merges. Halving the pieces' curves as the rounds
    /// do would never make the two meet.
    fn flush_extras(
        &self,
        jobs: &[chain::Job],
        chains: &[Chain],
        which: impl Iterator<Item = usize>,
        extras: &mut [BTreeMap<u32, Vec<f64>>; 2],
    ) {
        let resolution = self.tol.resolution();
        for i in which {
            let (arc, job, chain) = (&self.arcs[i], &jobs[i], &chains[i]);
            let (plane, k) = match job.shapes {
                [Shape::Plane { n, d }, Shape::Quadric(_)] => ((n, d), 1),
                [Shape::Quadric(_), Shape::Plane { n, d }] => ((n, d), 0),
                _ => continue,
            };
            if !chain.exact {
                continue;
            }
            let side = if k == 0 { Side::A } else { Side::B };
            let (input, _) = self.operand(side);
            let on_plane = |x: DVec3| (plane.0.dot(x) - plane.1).abs() <= resolution;
            let verts: Vec<DVec3> = std::iter::once(job.ends[0])
                .chain(chain.points.iter().copied())
                .chain(std::iter::once(job.ends[1]))
                .collect();
            for &(e, _) in &input.tri_edges[arc.tris[k] as usize] {
                if lined(input, e) {
                    continue;
                }
                let conic = input.conic(e);
                if ![0.25, 0.5, 0.75]
                    .into_iter()
                    .all(|t| on_plane(conic.eval(t)))
                {
                    continue;
                }
                for &x in &verts {
                    if x == conic.p0 || x == conic.p1 {
                        continue;
                    }
                    if let Some(t) = param_on(&conic, x, resolution)
                        && t > 1e-9
                        && t < 1.0 - 1e-9
                    {
                        extras[k].entry(e).or_default().push(t);
                    }
                }
            }
        }
        for list in extras.iter_mut().flat_map(|m| m.values_mut()) {
            list.sort_by(f64::total_cmp);
            list.dedup();
        }
    }

    /// Where the crossing vertex `id` is in triangle `t` of `side`
    /// (barycentric): on its side at its parameter, if its edge is one of
    /// the triangle's, else where the patch inverts its position.
    fn place(&self, id: u32, side: Side, t: u32) -> DVec3 {
        let [first12, first21] = self.first;
        let (own, k) = if id < first21 {
            (Side::A, (id - first12) as usize)
        } else {
            (Side::B, (id - first21) as usize)
        };
        let (input, crossing) = match side {
            Side::A => (self.a, &self.counts.x12),
            Side::B => (self.b, &self.counts.x21),
        };
        if own == side {
            let e = crossing[k].edge;
            let at = self.along[side as usize].at[k];
            if let Some(i) = input.tri_edges[t as usize].iter().position(|x| x.0 == e) {
                let forward = input.tri_edges[t as usize][i].1;
                let (s, en) = if forward {
                    (i, (i + 1) % 3)
                } else {
                    ((i + 1) % 3, i)
                };
                return DVec3::AXES[s] * (1.0 - at) + DVec3::AXES[en] * at;
            }
        }
        let x = self.base[id as usize];
        let at = project(input.corners(t), x);
        let guess = DVec3::new(1.0 - at.x - at.y, at.x, at.y);
        if Layout::of(input, t) == Layout::Flat {
            return guess;
        }
        let guess = guess.max(DVec3::ZERO);
        let guess = guess / guess.element_sum().max(f64::MIN_POSITIVE);
        chain::trace::invert(&input.patches[t as usize], x, guess)
    }

    /// Every face cut once, with the vertices `extras` added on the
    /// operands' edges and the cuts along `chains`.
    fn round(
        &self,
        extras: &[BTreeMap<u32, Vec<f64>>; 2],
        chains: &[Chain],
        work: &mut Work,
    ) -> Result<Round, KernelError> {
        let mut pos = self.base.clone();
        let mut curves = Curves::new();
        let stops = self.stops(extras, &mut pos)?;
        let pieces = self.edge_pieces(&stops, &mut curves)?;
        let edges = self.chain_edges(chains, &mut pos, &mut curves)?;
        let (jobs, tris, faces, whole) = self.face_jobs(chains, &edges.ids, extras);
        // A unit a face before, and its triangulation's steps after (up to
        // what is left, past which they all stop).
        work.spend(jobs.len())?;
        let meter = Meter::new(work.left().saturating_mul(STEPS_PER_UNIT));
        let cut = par_map(&jobs, |job| {
            let (input, offset) = self.operand(job.side);
            cut_face(
                input,
                job,
                &stops[job.side as usize],
                offset,
                &pos,
                &curves,
                &edges.fitted,
                &meter,
                self.tol,
            )
        });
        if meter.over() {
            return Err(KernelError::TooComplex);
        }
        work.spend(usize::try_from(meter.used() / STEPS_PER_UNIT).unwrap_or(usize::MAX))?;
        let cut: Vec<face::Cutout> = cut
            .into_iter()
            .collect::<Result<_, _>>()
            .map_err(KernelError::Boolean)?;
        let stray = cut.iter().map(|c| c.stray).fold(0.0, f64::max);
        // The chain edges to halve, by arc, and the operands' edges to add
        // vertices on.
        let mut split: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut more: [BTreeMap<u32, Vec<f64>>; 2] = [BTreeMap::new(), BTreeMap::new()];
        for k in cut.iter().flat_map(|c| &c.split) {
            if let Some(&(arc, seg)) = edges.owner.get(k) {
                split.entry(arc).or_default().push(seg);
            } else if let Some(&(side, e, t0, t1)) = pieces.get(k) {
                more[side as usize]
                    .entry(e)
                    .or_default()
                    .push((t0 + t1) / 2.0);
            }
        }
        Ok(Round {
            pos,
            curves,
            jobs,
            cut,
            tris,
            faces,
            whole,
            split,
            more,
            stray,
        })
    }

    /// Each operand's edges' stops: their crossings and the vertices
    /// `extras` adds on them (given ids after `pos`, and put there).
    fn stops(
        &self,
        extras: &[BTreeMap<u32, Vec<f64>>; 2],
        pos: &mut Vec<DVec3>,
    ) -> Result<Vec<Along>, KernelError> {
        let mut stops = Vec::with_capacity(2);
        for side in [Side::A, Side::B] {
            let k = side as usize;
            let (input, _) = self.operand(side);
            let mut added: BTreeMap<u32, Vec<(u32, f64)>> = BTreeMap::new();
            for (&e, ts) in &extras[k] {
                let conic = input.conic(e);
                for &t in ts {
                    let id = u32::try_from(pos.len()).map_err(|_| KernelError::TooComplex)?;
                    pos.push(on_curve(&conic, t));
                    added.entry(e).or_default().push((id, t));
                }
            }
            stops.push(self.along[k].with(&added));
        }
        Ok(stops)
    }

    /// The kept pieces of the operands' curved edges between their stops:
    /// their curves (into `curves`), and which edge and part of it each
    /// is, by the vertices it joins.
    #[allow(clippy::type_complexity)]
    fn edge_pieces(
        &self,
        stops: &[Along],
        curves: &mut Curves,
    ) -> Result<BTreeMap<(u32, u32), (Side, u32, f64, f64)>, KernelError> {
        let mut pieces = BTreeMap::new();
        for side in [Side::A, Side::B] {
            let (input, offset) = self.operand(side);
            for e in 0..input.edges.len() as u32 {
                if lined(input, e) {
                    continue;
                }
                let [s, en] = input.edges[e as usize];
                let (verts, kept) = stops[side as usize].of(e);
                let conic = input.conic(e);
                let mut ids = vec![s + offset];
                let mut ts = vec![0.0];
                for &(id, t) in verts {
                    ids.push(id);
                    ts.push(t);
                }
                ids.push(en + offset);
                ts.push(1.0);
                for (j, &kept) in kept.iter().enumerate() {
                    if kept {
                        let k = key(ids[j], ids[j + 1]);
                        curves.insert(k, piece(&conic, ts[j], ts[j + 1])?);
                        pieces.insert(k, (side, e, ts[j], ts[j + 1]));
                    }
                }
            }
        }
        Ok(pieces)
    }

    /// The chains' vertices (given ids after `pos`, and put there) and
    /// curves (into `curves`, where not straight).
    fn chain_edges(
        &self,
        chains: &[Chain],
        pos: &mut Vec<DVec3>,
        curves: &mut Curves,
    ) -> Result<ChainEdges, KernelError> {
        let mut fitted = BTreeSet::new();
        let mut owner = BTreeMap::new();
        let mut all = Vec::with_capacity(self.arcs.len());
        for (i, (arc, chain)) in self.arcs.iter().zip(chains).enumerate() {
            let mut ids = vec![arc.plus];
            for &p in &chain.points {
                ids.push(u32::try_from(pos.len()).map_err(|_| KernelError::TooComplex)?);
                pos.push(p);
            }
            ids.push(arc.minus);
            for (j, (w, c)) in ids.windows(2).zip(&chain.curves).enumerate() {
                let k = key(w[0], w[1]);
                owner.insert(k, (i, j));
                if c.w != 1.0 || c.c != (c.p0 + c.p1) * 0.5 {
                    curves.insert(k, Edge::of(c));
                }
                if !chain.exact {
                    fitted.insert(k);
                }
            }
            all.push(ids);
        }
        Ok(ChainEdges {
            ids: all,
            owner,
            fitted,
        })
    }

    /// The faces to cut, and the triangles of those kept whole (with
    /// their faces and which triangle each is). A face is cut if it has
    /// cuts, or vertices `extras` adds on its edges (unless the other
    /// solid's winding drops it); the others are kept whole or not at
    /// all.
    #[allow(clippy::type_complexity)]
    fn face_jobs(
        &self,
        chains: &[Chain],
        chain_ids: &[Vec<u32>],
        extras: &[BTreeMap<u32, Vec<f64>>; 2],
    ) -> (Vec<Cut>, Vec<[u32; 3]>, Vec<u32>, Vec<Option<(Side, u32)>>) {
        let keep = self.keep;
        let face_offset = self.a.mesh.faces().len() as u32;
        let windings = [&self.counts.w03, &self.counts.w30];
        // Each face's cut edges, as it runs them, and where the cuts'
        // vertices are in it. Keeping the outside of `B`, a face of `A`
        // runs its cut from the +1 end to the −1 end; faces of `B` the
        // other way round from that.
        let mut cuts: [Vec<(u32, [u32; 2])>; 2] = [Vec::new(), Vec::new()];
        let mut inside: [Vec<(u32, u32, DVec3, u32)>; 2] = [Vec::new(), Vec::new()];
        for (n, ((arc, chain), ids)) in self.arcs.iter().zip(chains).zip(chain_ids).enumerate() {
            for side in [Side::A, Side::B] {
                let k = side as usize;
                let tri = arc.tris[k];
                let mut run = ids.clone();
                if !keep.starts_at_plus(side) {
                    run.reverse();
                }
                cuts[k].extend(run.windows(2).map(|w| (tri, [w[0], w[1]])));
                inside[k].extend(
                    ids.iter()
                        .zip(&chain.dom[k])
                        .map(|(&id, &d)| (tri, id, d, n as u32)),
                );
            }
        }
        for list in &mut cuts {
            list.sort_unstable();
        }
        for list in &mut inside {
            list.sort_by_key(|x| (x.0, x.1));
        }
        let mut jobs = Vec::new();
        let mut tris = Vec::new();
        let mut faces = Vec::new();
        let mut whole = Vec::new();
        for side in [Side::A, Side::B] {
            let k = side as usize;
            let (input, offset) = self.operand(side);
            let (cuts, inside) = (&cuts[k], &inside[k]);
            let (mut c, mut j) = (0, 0);
            for t in 0..input.tris.len() as u32 {
                let from = c;
                while c < cuts.len() && cuts[c].0 == t {
                    c += 1;
                }
                let at = j;
                while j < inside.len() && inside[j].0 == t {
                    j += 1;
                }
                let corners = input.tris[t as usize];
                let kept = keep.keeps(side, windings[k][corners[0] as usize]);
                let added = input.tri_edges[t as usize]
                    .iter()
                    .any(|(e, _)| extras[k].contains_key(e));
                if c == from && !(kept && added) {
                    if kept {
                        tris.push(corners.map(|v| v + offset));
                        faces.push(face_id(side, input, t, face_offset));
                        whole.push(Some((side, t)));
                    }
                    continue;
                }
                let mut here: Vec<(u32, DVec3)> =
                    inside[at..j].iter().map(|x| (x.1, x.2)).collect();
                here.dedup_by_key(|x| x.0);
                let on_cut = if side == Side::B {
                    let mut on: Vec<(u32, u32)> =
                        inside[at..j].iter().map(|x| (x.1, x.3)).collect();
                    on.sort_unstable();
                    on.dedup();
                    on
                } else {
                    Vec::new()
                };
                jobs.push(Cut {
                    side,
                    tri: t,
                    cuts: cuts[from..c].iter().map(|c| c.1).collect(),
                    inside: here,
                    on_cut,
                });
            }
        }
        (jobs, tris, faces, whole)
    }

    /// The soup of the last round's triangles: the cut faces' triangles
    /// after those kept whole, the points they added given ids, pieces of
    /// the refinement kept whole merged back, `B`'s faces turned over for
    /// a difference, and triangles off their face's surface put on a copy
    /// of it claiming none.
    fn finish(
        &self,
        last: Round,
        refinement: Option<&Refinement>,
        work: &mut Work,
    ) -> Result<(Soup, Vec<Face>), KernelError> {
        let (a, b, keep) = (self.a, self.b, self.keep);
        let face_offset = a.mesh.faces().len() as u32;
        let Round {
            mut pos,
            mut curves,
            jobs,
            cut,
            mut tris,
            mut faces,
            mut whole,
            ..
        } = last;
        let mut off: Vec<bool> = vec![false; tris.len()];
        let mut inner = Vec::new();
        for (job, result) in jobs.iter().zip(cut) {
            let (input, _) = self.operand(job.side);
            // The points added inside the face get their ids.
            let first = u32::try_from(pos.len()).map_err(|_| KernelError::TooComplex)?;
            pos.extend(&result.steiner);
            let id = |v: u32| {
                if v >= FIRST_STEINER {
                    first + (v - FIRST_STEINER)
                } else {
                    v
                }
            };
            for (tri, is_off) in result.tris.into_iter().zip(result.off) {
                tris.push(tri.map(id));
                faces.push(face_id(job.side, input, job.tri, face_offset));
                whole.push(None);
                off.push(is_off);
            }
            inner.extend(
                result
                    .curves
                    .into_iter()
                    .map(|((u, v), e)| (key(id(u), id(v)), e)),
            );
        }
        curves.extend(inner);

        if let Some(refinement) = refinement {
            let offsets = [0, self.operand(Side::B).1];
            merge::merge(
                &mut tris,
                &mut faces,
                &mut whole,
                &mut off,
                &mut curves,
                refinement,
                offsets,
                work,
            )?;
        }

        if keep.flip_b {
            for (tri, &face) in tris.iter_mut().zip(&faces) {
                if face >= face_offset {
                    tri.swap(1, 2);
                }
            }
        }
        let mut out_faces: Vec<Face> = a.mesh.faces().to_vec();
        out_faces.extend(
            b.mesh
                .faces()
                .iter()
                .map(|&f| if keep.flip_b { flipped(f) } else { f }),
        );
        // Copies claiming no surface, for the faces with triangles off
        // theirs.
        let mut copies: BTreeMap<u32, u32> = BTreeMap::new();
        for (&face, _) in faces.iter().zip(&off).filter(|x| *x.1) {
            copies.entry(face).or_insert(0);
        }
        let mut sources: Vec<u32> = (0..out_faces.len() as u32).collect();
        for (face, copy) in &mut copies {
            *copy = out_faces.len() as u32;
            sources.push(*face);
            out_faces.push(Face {
                surface: Surface::Free,
                ..out_faces[*face as usize]
            });
        }
        for (face, &is_off) in faces.iter_mut().zip(&off) {
            if is_off {
                *face = copies[face];
            }
        }
        Ok((
            Soup {
                pos,
                tris,
                faces,
                curves,
                sources,
            },
            out_faces,
        ))
    }
}

/// Makes an edge's crossings, `order`ed along it, go in and out of the
/// other solid in turn from its start's winding number `w`, as they do
/// on any path through a solid: where one would take the winding number
/// out of `0..=1`, the next one along with the sign wanted is brought
/// forward, if it is `near` (at one place, as far as the resolution
/// tells: a tie, whose order its positions can't give). Exact orders
/// already alternate and stay as they are; crossings apart that don't
/// alternate are left, and the result fails.
fn alternate(
    order: &mut [usize],
    mut w: i32,
    sign: impl Fn(usize) -> i8,
    near: impl Fn(usize, usize) -> bool,
) {
    for i in 0..order.len() {
        let x = i32::from(sign(order[i]));
        if (0..=1).contains(&(w + x)) {
            w += x;
            continue;
        }
        let Some(j) = (i + 1..order.len()).find(|&j| i32::from(sign(order[j])) == -x) else {
            return;
        };
        if !near(order[i], order[j]) {
            return;
        }
        order[i..=j].rotate_right(1);
        w -= x;
    }
}

/// Each crossing's parameter along its edge, solved again exactly where
/// the face crossed is a plane or a quadric and the edge curved, or the
/// face curved (a straight edge through a planar patch is exact already):
/// at a root of the edge against the surface, a crossing the search only
/// placed at one with its sign on the patch crossed (see [`polish`]).
/// Also the work checking roots on the patches took.
///
/// A straight edge's crossings are found along the segment between its
/// ends, in its parameter; but where the edge isn't exactly that segment
/// ([`lined`]) its vertices are its conic's points, whose parameter runs
/// at another pace (a nearly straight cut whose control point is off its
/// chord's middle): that parameter is solved again on the conic, from the
/// conic's point nearest the segment's. Taken as it was, it put a vertex
/// `2e-3` along the edge from the plane it crossed.
fn params(
    input: &Input,
    other: &Input,
    crossings: &[Crossing],
    resolution: f64,
) -> (Vec<f64>, usize) {
    // Tests only: a negative distance no root's point is within, so
    // `polish` falls back on what it did before for crossings only placed.
    #[cfg(test)]
    let loose = LOOSE.get();
    #[cfg(not(test))]
    let loose = false;
    let placed = par_map(crossings, |c| {
        let straight = input.straight[c.edge as usize];
        let [s, e] = input.edges[c.edge as usize].map(|v| input.pos(v));
        let shape = Shape::of(other, c.face);
        let crossed = Crossed {
            patch: &other.patches[c.face as usize],
            x: c.x,
            solved: c.solved,
            resolution: if loose && !c.solved { -1.0 } else { resolution },
        };
        let (t, nodes) = if straight && other.planar[c.face as usize] {
            (c.t, 0)
        } else if straight {
            polish(&segment(s, e), c.t, &shape, &crossed)
        } else {
            return polish(&input.conic(c.edge), c.t, &shape, &crossed);
        };
        if lined(input, c.edge) {
            return (t, nodes);
        }
        let conic = input.conic(c.edge);
        let guess = param_on(&conic, lerp(s, e, t), f64::INFINITY).unwrap_or(t);
        let (t, more) = polish(&conic, guess, &shape, &crossed);
        (t, nodes.saturating_add(more))
    });
    let units = placed
        .iter()
        .map(|&(_, nodes)| nodes.div_ceil(NEAR_NODES_PER_UNIT))
        .fold(0, usize::saturating_add);
    (placed.into_iter().map(|(t, _)| t).collect(), units)
}

/// Whether edge `e` of `input` is exactly the segment between its ends
/// (as flat solids' edges are): its crossings are placed and its pieces
/// cut as such. An edge only straight within the resolution (a short arc)
/// keeps its curve.
fn lined(input: &Input, e: u32) -> bool {
    straight(&input.conic(e))
}

/// Pieces of a patch looked at certifying an unsolved crossing's distance
/// to it, to a unit of work (as the crossings' searches count theirs).
const NEAR_NODES_PER_UNIT: usize = 4;

/// Steps of triangulating a face (a vertex tested against a diagonal,
/// say) to a unit of work.
const STEPS_PER_UNIT: u64 = 16;

/// Where along `conic` the point `x` is, if it lies on it within
/// `within`: the nearest of samples, then Newton's method on the
/// distance.
fn param_on(conic: &Conic3, x: DVec3, within: f64) -> Option<f64> {
    let samples = 64;
    let mut t = (0..=samples)
        .map(|i| f64::from(i) / f64::from(samples))
        .min_by(|&a, &b| {
            conic
                .eval(a)
                .distance_squared(x)
                .total_cmp(&conic.eval(b).distance_squared(x))
        })
        .expect("samples");
    for _ in 0..16 {
        let (p, d) = conic.eval_deriv(t);
        let dd = d.length_squared();
        if dd.is_nan() || dd <= 0.0 {
            break;
        }
        let step = (x - p).dot(d) / dd;
        t = (t + step).clamp(0.0, 1.0);
        if step.abs() <= 1e-15 {
            break;
        }
    }
    (conic.eval(t).distance(x) <= within).then_some(t)
}

/// The piece of `conic` from `s` to `t`, exact by blossoming.
fn piece(conic: &Conic3, s: f64, t: f64) -> Result<Edge, KernelError> {
    if s == 0.0 && t == 1.0 {
        return Ok(Edge::of(conic));
    }
    let part = Conic3::from_hom([
        conic.blossom(s, s),
        conic.blossom(s, t),
        conic.blossom(t, t),
    ])?;
    Ok(Edge {
        ctrl: part.c.shared(&conic.hull()),
        weight: part.w,
    })
}

fn face_id(side: Side, input: &Input, t: u32, offset: u32) -> u32 {
    match side {
        Side::A => input.face(t),
        Side::B => offset + input.face(t),
    }
}

/// The face facing the other way.
fn flipped(f: Face) -> Face {
    let surface = match f.surface {
        Surface::Plane { n, d } => Surface::Plane { n: -n, d: -d },
        s => s,
    };
    Face { surface, ..f }
}

#[cfg(test)]
mod tests {
    use super::alternate;

    #[test]
    fn crossings_at_one_place_go_in_and_out_in_turn() {
        // Signs by index; all at one place.
        let signs = [1i8, 1, -1, -1];
        let run = |order: &mut [usize], w: i32| {
            alternate(order, w, |k| signs[k], |_, _| true);
            let mut w = w;
            for &k in order.iter() {
                w += i32::from(signs[k]);
                assert!((0..=1).contains(&w), "{order:?}");
            }
        };
        let mut order = [0, 1, 2, 3];
        run(&mut order, 0);
        assert_eq!(order, [0, 2, 1, 3]);
        let mut order = [0, 2, 1, 3];
        run(&mut order, 0);
        assert_eq!(order, [0, 2, 1, 3]);
        let mut order = [2, 0, 3, 1];
        run(&mut order, 1);
        assert_eq!(order, [2, 0, 3, 1]);
        // Apart, they stay as they are, and the result will fail.
        let mut order = [0, 1, 2, 3];
        alternate(&mut order, 0, |k| signs[k], |_, _| false);
        assert_eq!(order, [0, 1, 2, 3]);
    }
}
