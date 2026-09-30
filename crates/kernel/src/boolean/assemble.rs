//! Assembling the result from the counts: new vertices as records, the
//! parts of edges and faces to keep, the cut edges between the two, and
//! each cut face triangulated in its parameter domain.

use glam::{DVec2, DVec3};

use super::cleanup::Soup;
use super::count::{Counts, Crossing};
use super::exact::orient2d;
use super::input::{Input, Side};
use super::triangulate::{Vert, triangulate};
use super::{BooleanError, Op, Primitives};
use crate::KernelError;
use crate::budget::Work;
use crate::mesh::{Face, Surface};
use crate::par::par_map;

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
struct Along {
    /// For each edge, where its crossings start in `verts` (one past the
    /// end is the next edge's start).
    start: Vec<u32>,
    /// The new vertices' ids, edge by edge, in order along the edge, with
    /// their parameters (never decreasing along an edge).
    verts: Vec<(u32, f64)>,
    /// Whether each piece is kept: an edge with `k` crossings has `k + 1`
    /// pieces, at `start[e] + e..`.
    kept: Vec<bool>,
}

impl Along {
    #[allow(clippy::too_many_arguments)]
    fn new(
        side: Side,
        input: &Input,
        crossings: &[Crossing],
        first_id: u32,
        params: &[f64],
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
                        prims.order(side, e, crossings[k].face, crossings[x].face)
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
                verts.push((first_id + k as u32, at));
            }
            kept.push(keep.keeps(side, w));
        }
        Along { start, verts, kept }
    }

    /// Edge `e`'s crossings, in order, and whether each of its pieces is
    /// kept.
    fn of(&self, e: u32) -> (&[(u32, f64)], &[bool]) {
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

/// The corners of the parameter domain.
const DOMAIN: [DVec2; 3] = [DVec2::ZERO, DVec2::X, DVec2::Y];

/// Everything one face needs to be cut.
struct Cut {
    side: Side,
    tri: u32,
    /// Its cut edges, as it runs them.
    cuts: Vec<[u32; 2]>,
}

/// The result's triangles, before clean-up: vertices are the operands'
/// (`A`'s, then `B`'s) and then the new ones (`x12`'s, then `x21`'s).
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble(
    op: Op,
    a: &Input,
    b: &Input,
    counts: &Counts,
    prims: &impl Primitives,
    work: &mut Work,
) -> Result<(Soup, Vec<Face>), KernelError> {
    let keep = Keep::of(op);
    let (nva, nvb) = (a.mesh.verts().len() as u32, b.mesh.verts().len() as u32);
    let first12 = nva + nvb;
    let first21 = first12 + counts.x12.len() as u32;

    // New vertices: where the edges cross, from their parameters.
    work.spend(counts.x12.len() + counts.x21.len())?;
    let t12 = par_map(&counts.x12, |c| prims.crossing(Side::A, c.edge, c.face));
    let t21 = par_map(&counts.x21, |c| prims.crossing(Side::B, c.edge, c.face));

    // Ordering the crossings along each edge compares every two.
    for crossings in [&counts.x12, &counts.x21] {
        for run in crossings.chunk_by(|x, y| x.edge == y.edge) {
            work.spend(run.len().saturating_mul(run.len()))?;
        }
    }
    let along = [
        Along::new(
            Side::A,
            a,
            &counts.x12,
            first12,
            &t12,
            &counts.w03,
            keep,
            prims,
        ),
        Along::new(
            Side::B,
            b,
            &counts.x21,
            first21,
            &t21,
            &counts.w30,
            keep,
            prims,
        ),
    ];
    // Their positions, from the parameters as ordered along the edges,
    // and exactly on the face crossed where it is square to an axis (so
    // flush faces of results fed on stay flush).
    let mut pos: Vec<DVec3> = a
        .mesh
        .verts()
        .iter()
        .chain(b.mesh.verts())
        .copied()
        .collect();
    pos.resize(first21 as usize + counts.x21.len(), DVec3::ZERO);
    for (input, other, along, crossings, first) in [
        (a, b, &along[0], &counts.x12, first12),
        (b, a, &along[1], &counts.x21, first21),
    ] {
        for e in 0..input.edges.len() as u32 {
            let [s, en] = input.edges[e as usize].map(|v| input.pos(v));
            for &(id, t) in along.of(e).0 {
                let mut p = lerp(s, en, t);
                let [c0, c1, c2] = other.corners(crossings[(id - first) as usize].face);
                for k in 0..3 {
                    if c0[k] == c1[k] && c0[k] == c2[k] {
                        p[k] = c0[k];
                    }
                }
                pos[id as usize] = p;
            }
        }
    }

    // The ends of each face pair's cut, with their signs seen from A.
    let mut ends: Vec<([u32; 2], u32, i8)> = Vec::new();
    for (i, c) in counts.x12.iter().enumerate() {
        let [forward, backward] = a.edge_tris[c.edge as usize];
        let id = first12 + i as u32;
        ends.push(([forward, c.face], id, c.x));
        ends.push(([backward, c.face], id, -c.x));
    }
    for (i, c) in counts.x21.iter().enumerate() {
        let [forward, backward] = b.edge_tris[c.edge as usize];
        let id = first21 + i as u32;
        ends.push(([c.face, forward], id, -c.x));
        ends.push(([c.face, backward], id, c.x));
    }
    ends.sort_unstable();
    let mut cuts_a: Vec<(u32, [u32; 2])> = Vec::new();
    let mut cuts_b: Vec<(u32, [u32; 2])> = Vec::new();
    for pair in ends.chunk_by(|x, y| x.0 == y.0) {
        let [(face, u, su), (_, v, sv)] = pair else {
            return Err(KernelError::Boolean(BooleanError::Inconsistent));
        };
        if su + sv != 0 {
            return Err(KernelError::Boolean(BooleanError::Inconsistent));
        }
        let (plus, minus) = if *su > 0 { (*u, *v) } else { (*v, *u) };
        let run = |side| {
            if keep.starts_at_plus(side) {
                [plus, minus]
            } else {
                [minus, plus]
            }
        };
        cuts_a.push((face[0], run(Side::A)));
        cuts_b.push((face[1], run(Side::B)));
    }
    cuts_a.sort_unstable();
    cuts_b.sort_unstable();

    // Faces with cuts are cut; the others are kept whole or not at all.
    let mut jobs = Vec::new();
    let mut tris = Vec::new();
    let mut faces = Vec::new();
    let face_offset = a.mesh.faces().len() as u32;
    for (side, input, cuts, windings, offset) in [
        (Side::A, a, &cuts_a, &counts.w03, 0),
        (Side::B, b, &cuts_b, &counts.w30, nva),
    ] {
        let mut k = 0;
        for t in 0..input.tris.len() as u32 {
            let from = k;
            while k < cuts.len() && cuts[k].0 == t {
                k += 1;
            }
            if k > from {
                jobs.push(Cut {
                    side,
                    tri: t,
                    cuts: cuts[from..k].iter().map(|c| c.1).collect(),
                });
            } else {
                let corners = input.tris[t as usize];
                if keep.keeps(side, windings[corners[0] as usize]) {
                    tris.push(corners.map(|v| v + offset));
                    faces.push(face_id(side, input, t, face_offset));
                }
            }
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
    // Each operand, and where its vertices start.
    let operand = |side| match side {
        Side::A => (a, 0),
        Side::B => (b, nva),
    };
    let cut = par_map(&jobs, |job| {
        let (input, offset) = operand(job.side);
        cut_face(input, job, &along[job.side as usize], offset, &pos)
    });
    for (job, result) in jobs.iter().zip(cut) {
        let (input, _) = operand(job.side);
        for tri in result.map_err(KernelError::Boolean)? {
            tris.push(tri);
            faces.push(face_id(job.side, input, job.tri, face_offset));
        }
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
    Ok((Soup { pos, tris, faces }, out_faces))
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

/// Domain coordinates smaller than this are zero: far below anything
/// rounding can tell, and it keeps the exact orientation tests' products
/// clear of underflow.
const TINY: f64 = 1.0 / (1u128 << 64) as f64;

/// The point at `t` from corner `from` to corner `to` of the domain,
/// exactly on that side.
fn on_side(from: DVec2, to: DVec2, t: f64) -> DVec2 {
    let t = if t < TINY { 0.0 } else { t };
    let p = from + (to - from) * t;
    if from.x + from.y == 1.0 && to.x + to.y == 1.0 {
        on_hypotenuse(p.y)
    } else {
        p
    }
}

/// The point of the side `u + v = 1` at `v` (in `[0, 1]`), with the sum
/// exact: the larger coordinate is taken as given and the other is one
/// less it, which is exact from ½ up.
fn on_hypotenuse(v: f64) -> DVec2 {
    let v = v.clamp(0.0, 1.0);
    if v >= 0.5 {
        DVec2::new(1.0 - v, v)
    } else {
        let u = 1.0 - v;
        DVec2::new(u, 1.0 - u)
    }
}

/// A vertex inside the face (it is, for the perturbed operands) as
/// placed from its position, moved onto the domain's side where rounding
/// or a tie put it on or beyond it. The triangulation then treats it as
/// inside, infinitely close.
fn into_domain(at: DVec2) -> DVec2 {
    let u = if at.x < TINY { 0.0 } else { at.x };
    let v = if at.y < TINY { 0.0 } else { at.y };
    let p = DVec2::new(u, v);
    if orient2d(DOMAIN[1], DOMAIN[2], p) > 0 {
        p
    } else {
        on_hypotenuse((v - u + 1.0) / 2.0)
    }
}

/// The triangles of the kept part of one face, as its triangle runs.
fn cut_face(
    input: &Input,
    job: &Cut,
    along: &Along,
    offset: u32,
    pos: &[DVec3],
) -> Result<Vec<[u32; 3]>, BooleanError> {
    let t = job.tri;
    let corners = input.tris[t as usize];
    let corner_ids = corners.map(|v| v + offset);
    // The domain position and sides of every vertex on the boundary.
    let mut known: Vec<Vert> = (0..3)
        .map(|i| Vert {
            id: corner_ids[i],
            at: DOMAIN[i],
            sides: (1 << i) | (1 << ((i + 2) % 3)),
        })
        .collect();
    let mut halfedges: Vec<[u32; 2]> = job.cuts.clone();
    for (i, &(e, forward)) in input.tri_edges[t as usize].iter().enumerate() {
        let (verts, kept) = along.of(e);
        // The edge's own direction, from its start corner.
        let (s, en) = if forward {
            (i, (i + 1) % 3)
        } else {
            ((i + 1) % 3, i)
        };
        let mut chain = vec![corner_ids[s]];
        for &(id, param) in verts {
            chain.push(id);
            known.push(Vert {
                id,
                at: on_side(DOMAIN[s], DOMAIN[en], param),
                sides: 1 << i,
            });
        }
        chain.push(corner_ids[en]);
        for (k, &keep) in kept.iter().enumerate() {
            if keep {
                let (u, v) = (chain[k], chain[k + 1]);
                halfedges.push(if forward { [u, v] } else { [v, u] });
            }
        }
    }
    halfedges.sort_unstable();
    if halfedges.windows(2).any(|w| w[0][0] == w[1][0]) {
        return Err(BooleanError::Inconsistent);
    }
    known.sort_by_key(|v| v.id);
    let [p0, p1, p2] = corners.map(|v| input.pos(v));
    let (d1, d2) = (p1 - p0, p2 - p0);
    let n = d1.cross(d2);
    let nn = n.length_squared();
    let vert = |id: u32| -> Vert {
        match known.binary_search_by_key(&id, |v| v.id) {
            Ok(i) => known[i],
            Err(_) => {
                let x = pos[id as usize] - p0;
                Vert {
                    id,
                    at: into_domain(DVec2::new(x.cross(d2).dot(n) / nn, d1.cross(x).dot(n) / nn)),
                    sides: 0,
                }
            }
        }
    };
    let mut used = vec![false; halfedges.len()];
    let mut loops = Vec::new();
    for i in 0..halfedges.len() {
        if used[i] {
            continue;
        }
        let first = halfedges[i][0];
        let mut l = Vec::new();
        let mut u = first;
        loop {
            let k = halfedges
                .binary_search_by_key(&u, |h| h[0])
                .map_err(|_| BooleanError::Inconsistent)?;
            if used[k] {
                return Err(BooleanError::Inconsistent);
            }
            used[k] = true;
            l.push(vert(u));
            u = halfedges[k][1];
            if u == first {
                break;
            }
        }
        loops.push(l);
    }

    triangulate(loops)
}
