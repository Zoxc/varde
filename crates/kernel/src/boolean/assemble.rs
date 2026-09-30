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
//! again. Then refinement's pieces that came through whole are merged
//! back (see [`merge`]).

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;

use super::chain::{self, Chain};
use super::cleanup::Soup;
use super::count::{Counts, Crossing};
use super::input::{Input, Side};
use super::pairs::{Arc, first_ids};
use super::surface::{Shape, polish};
use super::{Op, Primitives};
use crate::budget::Work;
use crate::mesh::{Edge, Face, Node, Surface};
use crate::par::par_map;
use crate::patch::Conic3;
use crate::{KernelError, Tolerance};

mod face;
mod merge;

use face::{Cut, FIRST_STEINER, Layout, cut_face, project};

/// How many rounds of halving curves the faces are cut in, at most (see
/// the [module](self) docs); what the last leaves is for repair.
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
        crossings: &[Crossing],
        params: &[f64],
        first_id: u32,
        windings: &[i32],
        keep: Keep,
        prims: &impl Primitives,
    ) -> Along {
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
            // By insertion, which can't fail however the order behaves.
            let mut here: Vec<usize> = Vec::with_capacity(i - from);
            for k in from..i {
                let at = here
                    .iter()
                    .position(|&x| {
                        prims.order(side, e, &crossings[k], &crossings[x])
                            == std::cmp::Ordering::Less
                    })
                    .unwrap_or(here.len());
                here.insert(at, k);
            }
            let [s, _] = input.edges[e as usize];
            let mut w = windings[s as usize];
            // The parameters follow the order: rounding may have swapped
            // two that are close.
            let mut at = 0.0f64;
            for &k in &here {
                kept.push(keep.keeps(side, w));
                w += i32::from(crossings[k].x);
                at = at.max(params[k]);
                at_of[k] = at;
                verts.push((first_id + k as u32, at));
            }
            kept.push(keep.keeps(side, w));
        }
        Along {
            start,
            verts,
            kept,
            at: at_of,
        }
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
    let nva = a.mesh.verts().len() as u32;
    let [first12, first21] = first_ids(a, b, counts);

    // Ordering the crossings along each edge compares every two.
    for crossings in [&counts.x12, &counts.x21] {
        for run in crossings.chunk_by(|x, y| x.edge == y.edge) {
            work.spend(run.len().saturating_mul(run.len()))?;
        }
    }
    let params = [params(a, b, &counts.x12), params(b, a, &counts.x21)];
    let along = [
        Along::new(
            Side::A,
            a,
            &counts.x12,
            &params[0],
            first12,
            &counts.w03,
            keep,
            prims,
        ),
        Along::new(
            Side::B,
            b,
            &counts.x21,
            &params[1],
            first21,
            &counts.w30,
            keep,
            prims,
        ),
    ];
    // Their positions, from the parameters as ordered along the edges,
    // and exactly on the face crossed where it is square to an axis (so
    // flush faces of results fed on stay flush).
    let mut base: Vec<DVec3> = a
        .mesh
        .verts()
        .iter()
        .chain(b.mesh.verts())
        .copied()
        .collect();
    base.resize(first21 as usize + counts.x21.len(), DVec3::ZERO);
    for (input, other, along, crossings, first) in [
        (a, b, &along[0], &counts.x12, first12),
        (b, a, &along[1], &counts.x21, first21),
    ] {
        for e in 0..input.edges.len() as u32 {
            let [s, en] = input.edges[e as usize];
            let snap = |mut p: DVec3, id: u32| {
                let [c0, c1, c2] = other.corners(crossings[(id - first) as usize].face);
                for k in 0..3 {
                    if c0[k] == c1[k] && c0[k] == c2[k] {
                        p[k] = c0[k];
                    }
                }
                p
            };
            let conic = input.conic(e);
            let [ps, pe] = [s, en].map(|v| input.pos(v));
            for &(id, t) in along.of(e).0 {
                let p = if lined(input, e) {
                    lerp(ps, pe, t)
                } else {
                    point(&conic, t)
                };
                base[id as usize] = snap(p, id);
            }
        }
    }

    // Each arc's chain.
    let chain_jobs: Vec<chain::Job> = arcs
        .iter()
        .map(|arc| {
            let [p, q] = arc.tris;
            let ends = [arc.plus, arc.minus];
            let dom = ends.map(|id| {
                [
                    place(
                        id,
                        Side::A,
                        p,
                        a,
                        b,
                        counts,
                        &along,
                        &base,
                        [first12, first21],
                    ),
                    place(
                        id,
                        Side::B,
                        q,
                        a,
                        b,
                        counts,
                        &along,
                        &base,
                        [first12, first21],
                    ),
                ]
            });
            chain::Job {
                p: &a.patches[p as usize],
                q: &b.patches[q as usize],
                shapes: [Shape::of(a, p), Shape::of(b, q)],
                planar: [a.planar[p as usize], b.planar[q as usize]],
                ends: ends.map(|id| base[id as usize]),
                dom,
                on_p: ends.map(|id| id < first21),
                flip_q: keep.flip_b,
            }
        })
        .collect();
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
    let face_offset = a.mesh.faces().len() as u32;
    // Each operand, and where its vertices start.
    let operand = |side| match side {
        Side::A => (a, 0),
        Side::B => (b, nva),
    };
    let inputs = [a, b];
    let windings = [&counts.w03, &counts.w30];

    // The faces, in rounds: where a triangle along a cut strays from its
    // patch, or a curve on a face's boundary bulges out of the triangle
    // it is a side of, that curve is halved (a cut's by its chain, an
    // operand's edge by a vertex added on it, which both faces beside it
    // get) and the faces are cut again.
    let mut extras: [BTreeMap<u32, Vec<f64>>; 2] = [BTreeMap::new(), BTreeMap::new()];
    let mut round = 0;
    let (mut pos, mut curves, jobs, cut, mut tris, mut faces, mut whole) = loop {
        let mut pos = base.clone();
        let mut curves = Curves::new();
        // The added vertices on the operands' edges, and each edge's stops
        // with them.
        let mut stops: Vec<Along> = Vec::with_capacity(2);
        for side in [Side::A, Side::B] {
            let k = side as usize;
            let mut added: BTreeMap<u32, Vec<(u32, f64)>> = BTreeMap::new();
            for (&e, ts) in &extras[k] {
                let conic = inputs[k].conic(e);
                for &t in ts {
                    let id = u32::try_from(pos.len()).map_err(|_| KernelError::TooComplex)?;
                    pos.push(point(&conic, t));
                    added.entry(e).or_default().push((id, t));
                }
            }
            stops.push(along[k].with(&added));
        }
        // The curved edges' pieces, and which edge and part of it each is.
        let mut pieces: BTreeMap<(u32, u32), (Side, u32, f64, f64)> = BTreeMap::new();
        for side in [Side::A, Side::B] {
            let (input, offset) = operand(side);
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
        // The chains' vertices and curves, and each chain edge's arc and
        // index along it.
        let mut fitted = BTreeSet::new();
        let mut owner: BTreeMap<(u32, u32), (usize, usize)> = BTreeMap::new();
        let mut chain_ids: Vec<Vec<u32>> = Vec::with_capacity(arcs.len());
        for (i, (arc, chain)) in arcs.iter().zip(&chains).enumerate() {
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
                    curves.insert(
                        k,
                        Edge {
                            ctrl: c.c,
                            weight: c.w,
                        },
                    );
                }
                if !chain.exact {
                    fitted.insert(k);
                }
            }
            chain_ids.push(ids);
        }
        // Each face's cut edges, as it runs them, and where the cuts'
        // vertices are in it. Keeping the outside of `B`, a face of `A`
        // runs its cut from the +1 end to the −1 end; faces of `B` the
        // other way round from that.
        let mut cuts: [Vec<(u32, [u32; 2])>; 2] = [Vec::new(), Vec::new()];
        let mut inside: [Vec<(u32, u32, DVec3, u32)>; 2] = [Vec::new(), Vec::new()];
        for (n, ((arc, chain), ids)) in arcs.iter().zip(&chains).zip(&chain_ids).enumerate() {
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
        // Faces with cuts, or with vertices added on their edges, are cut
        // (those unless the other solid's winding drops them); the
        // others are kept whole or not at all.
        let mut jobs = Vec::new();
        let mut tris = Vec::new();
        let mut faces = Vec::new();
        // Which triangle of which operand each kept whole one is.
        let mut whole: Vec<Option<(Side, u32)>> = Vec::new();
        for side in [Side::A, Side::B] {
            let k = side as usize;
            let (input, offset) = operand(side);
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
        // Ear clipping looks at every vertex for every ear, and more for
        // large faces.
        work.spend(
            jobs.iter()
                .map(|j| {
                    let n = j.cuts.len() + 6;
                    n.saturating_mul(n).saturating_mul(1 + n / 64)
                })
                .fold(0, usize::saturating_add),
        )?;
        let cut = par_map(&jobs, |job| {
            let (input, offset) = operand(job.side);
            cut_face(
                input,
                job,
                &stops[job.side as usize],
                offset,
                &pos,
                &curves,
                &fitted,
                tol,
            )
        });
        let cut: Vec<face::Cutout> = cut
            .into_iter()
            .collect::<Result<_, _>>()
            .map_err(KernelError::Boolean)?;
        // The chain edges to halve, by arc, and the operands' edges to add
        // vertices on.
        let mut split: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut more: [BTreeMap<u32, Vec<f64>>; 2] = [BTreeMap::new(), BTreeMap::new()];
        for k in cut.iter().flat_map(|c| &c.split) {
            if let Some(&(arc, seg)) = owner.get(k) {
                split.entry(arc).or_default().push(seg);
            } else if let Some(&(side, e, t0, t1)) = pieces.get(k) {
                more[side as usize]
                    .entry(e)
                    .or_default()
                    .push((t0 + t1) / 2.0);
            }
        }
        let done = split.is_empty() && more.iter().all(BTreeMap::is_empty);
        if done || round >= SPLIT_ROUNDS {
            break (pos, curves, jobs, cut, tris, faces, whole);
        }
        round += 1;
        work.spend(
            split.values().map(Vec::len).sum::<usize>()
                + more
                    .iter()
                    .flat_map(|m| m.values())
                    .map(Vec::len)
                    .sum::<usize>(),
        )?;
        for (k, more) in more.into_iter().enumerate() {
            for (e, ts) in more {
                let list = extras[k].entry(e).or_default();
                list.extend(ts);
                list.sort_by(f64::total_cmp);
                list.dedup();
            }
        }
        let wanted: Vec<(usize, Vec<usize>)> = split
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
    };

    // Faces some of whose triangles are off their surface: those go on a
    // copy of it claiming none.
    let mut off: Vec<bool> = vec![false; tris.len()];
    let mut inner = Vec::new();
    for (job, result) in jobs.iter().zip(cut) {
        let (input, _) = operand(job.side);
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
        let offsets = [0, nva];
        merge::merge(
            &mut tris,
            &mut faces,
            &mut whole,
            &mut off,
            &mut curves,
            refinement,
            offsets,
        );
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
    // Copies claiming no surface, for the faces with triangles off theirs.
    let mut copies: BTreeMap<u32, u32> = BTreeMap::new();
    for (&face, _) in faces.iter().zip(&off).filter(|x| *x.1) {
        copies.entry(face).or_insert(0);
    }
    for (face, copy) in &mut copies {
        *copy = out_faces.len() as u32;
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
        },
        out_faces,
    ))
}

/// Each crossing's parameter along its edge, solved again exactly where
/// the face crossed is a plane or a quadric and the edge curved, or the
/// face curved (a straight edge through a planar patch is exact already).
fn params(input: &Input, other: &Input, crossings: &[Crossing]) -> Vec<f64> {
    par_map(crossings, |c| {
        let straight = input.straight[c.edge as usize];
        if straight && other.planar[c.face as usize] {
            return c.t;
        }
        let conic = if straight {
            let [s, e] = input.edges[c.edge as usize].map(|v| input.pos(v));
            Conic3 {
                p0: s,
                c: (s + e) * 0.5,
                w: 1.0,
                p1: e,
            }
        } else {
            input.conic(c.edge)
        };
        polish(&conic, c.t, &Shape::of(other, c.face))
    })
}

/// Whether edge `e` of `input` is exactly the segment between its ends
/// (as flat solids' edges are): its crossings are placed and its pieces
/// cut as such. An edge only straight within the resolution (a short arc)
/// keeps its curve.
fn lined(input: &Input, e: u32) -> bool {
    let edge = input.mesh.edges()[e as usize];
    let [s, t] = input.edges[e as usize].map(|v| input.pos(v));
    edge.weight == 1.0 && edge.ctrl == (s + t) * 0.5
}

/// The point of `conic` at `t`, from its blossom, so the ends of pieces
/// split there are it to the bit.
fn point(conic: &Conic3, t: f64) -> DVec3 {
    let h = conic.blossom(t, t);
    h.truncate() / h.w
}

/// The piece of `conic` from `s` to `t`, exact by blossoming.
fn piece(conic: &Conic3, s: f64, t: f64) -> Result<Edge, KernelError> {
    if s == 0.0 && t == 1.0 {
        return Ok(Edge {
            ctrl: conic.c,
            weight: conic.w,
        });
    }
    let part = Conic3::from_hom([
        conic.blossom(s, s),
        conic.blossom(s, t),
        conic.blossom(t, t),
    ])?;
    Ok(Edge {
        ctrl: part.c,
        weight: part.w,
    })
}

/// Where the crossing vertex `id` is in triangle `t` of `side`
/// (barycentric): on its side at its parameter, if its edge is one of
/// the triangle's, else where the patch inverts its position.
#[allow(clippy::too_many_arguments)]
fn place(
    id: u32,
    side: Side,
    t: u32,
    a: &Input,
    b: &Input,
    counts: &Counts,
    along: &[Along; 2],
    pos: &[DVec3],
    [first12, first21]: [u32; 2],
) -> DVec3 {
    let (own, k) = if id < first21 {
        (Side::A, (id - first12) as usize)
    } else {
        (Side::B, (id - first21) as usize)
    };
    let (input, crossing) = match side {
        Side::A => (a, &counts.x12),
        Side::B => (b, &counts.x21),
    };
    if own == side {
        let e = crossing[k].edge;
        let at = along[side as usize].at[k];
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
    let x = pos[id as usize];
    let at = project(input.corners(t), x);
    let guess = DVec3::new(1.0 - at.x - at.y, at.x, at.y);
    if Layout::of(input, t) == Layout::Flat {
        return guess;
    }
    let guess = guess.max(DVec3::ZERO);
    let guess = guess / guess.element_sum().max(f64::MIN_POSITIVE);
    chain::trace::invert(&input.patches[t as usize], x, guess)
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

/// The point at `t` from `s` to `e`, exactly `s` at 0 and `e` at 1.
fn lerp(s: DVec3, e: DVec3, t: f64) -> DVec3 {
    if t <= 0.5 {
        s + (e - s) * t
    } else {
        e + (s - e) * (1.0 - t)
    }
}
