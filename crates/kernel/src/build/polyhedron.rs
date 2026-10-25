//! A body from a closed triangle mesh, for tests: the corpora of flat
//! bodies (meshes as other kernels' tests keep them) read as planar
//! B-reps, checked as any constructed body.
//!
//! **The mesh.** Vertices are `f64` points taken exactly as given, each
//! a distinct position; triangles are vertex index triples turning
//! counter-clockwise seen from outside. It must be a closed oriented
//! surface: every directed edge of a triangle met once, by the reverse
//! edge of one other triangle. It may be in several pieces (the triangles
//! reached from each other across edges), such as a cavity's walls
//! turned inwards, but no vertex may join two pieces: the body would
//! touch itself there, a contact this constructor doesn't record. A
//! triangle whose corners lie on one line is refused, decided exactly.
//!
//! **Faces.** Triangles across an edge are merged into one face where
//! they lie on one plane, facing one way: the far corner of the one on
//! the other's plane by the exact `orient3d`, their exact normals'
//! product positive, and their planes' keys equal bit for bit
//! ([`PlaneKey::through`], the exact plane of the triangle's corners
//! rounded once, so triangles of one exact plane give one key). Never by
//! a distance: triangles a rounding apart stay apart, joined at a crease.
//! A face is the union of its triangles: the edges between two of them
//! are dropped, the vertices inside it with them, so its boundary is its
//! loops: the outer loop counter-clockwise about its outward normal, the
//! holes the other way, told apart by their exact areas. A boundary that
//! meets itself at a vertex, or a face with other than one outer loop, is
//! refused. A vertex on a straight run between two faces stays, with an
//! edge on each side.
//!
//! **Geometry.** A face lies on its plane's surface (the key's lattice,
//! its one tile), flipped where its outward normal is against the key's;
//! its edges are the lines between their vertices, their p-curves the
//! lines between the vertices' UV on the plane (each rounded once), and
//! the seam bounds are certified as a primitive's
//! ([`super::primitives::seam_target`]), so a ribbon is the rounding of a
//! vertex off its face's stored plane. Edges are creases, convex or
//! concave by the exact sign of their faces' normals' cross product
//! along them; faces whose outward normals are opposite (a knife edge)
//! are refused.
//!
//! **Names.** Face `k` (faces in the order of their least triangle) is
//! keyed `Primitive(k)` of the caller's [`OpCtx`]. Shells and regions
//! are derived from the faces ([`Builder::fill_regions`]): each piece's
//! sides make two shells, the lumps, voids and their islands follow from
//! the shells' signed volumes, and an island's placement must be settled
//! by the boxes the regions' derivation reads (`topo::regions`), else
//! the body is refused there.
//!
//! **Work.** The planes' keys, one per triangle, and the coplanarity
//! tests, one per edge, run in charged passes; then the seam bounds.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::convert::Infallible;

use glam::DVec3;
use malachite::Rational;
use varde_decide::num::exact::{rat as q, value_sign};
use varde_decide::num::preds::Orient3d;
use varde_decide::num::schedule::{UNIT_PARTS, UNITS};
use varde_decide::num::storage::OperandScan;

use super::primitives::{built, defect, lattice, line, plane_uv, seam_target};
use crate::geom::form::{FormError, FormKey, PlaneKey};
use crate::geom::formkey::KeyMeter;
use crate::geom::lattice::Lattice;
use crate::geom::pcurve::{PCurve, PSpan};
use crate::geom::surface::Surface;
use crate::infra::budget::{Budget, Halt, Pass, Step, Stop, Work};
use crate::infra::error::{KernelError, Storage};
use crate::infra::ops::{OpCtx, Status, Tolerance, Warning, WarningKind};
use crate::topo::brep::{
    BodyKind, Brep, Continuity, Convexity, EdgeId, EdgeKind, ExactRows, FaceId, FaceKey, FacePart,
    VertId,
};
use crate::topo::build::{Builder, Use};
use crate::topo::check::{Outcome, Role, Scope};

#[cfg(test)]
mod tests;

/// Why a mesh makes no body.
#[derive(Clone, Debug, PartialEq)]
pub enum PolyhedronError {
    /// Triangle `tri` names a vertex that isn't there.
    Index {
        /// The triangle.
        tri: usize,
    },
    /// Two vertices at one position.
    Duplicate {
        /// The two, the lesser first.
        verts: [usize; 2],
    },
    /// Triangle `tri`'s corners lie on one line.
    Degenerate {
        /// The triangle.
        tri: usize,
    },
    /// The directed edge `edge` is used twice, or its reverse by no
    /// triangle: the mesh isn't a closed oriented surface.
    Open {
        /// The edge's vertices, in its direction.
        edge: [usize; 2],
    },
    /// Vertex `vert` joins two pieces of the mesh.
    Pieces {
        /// The vertex.
        vert: usize,
    },
    /// A face's boundary meets itself at vertex `vert`, or the face has
    /// other than one outer loop (then `vert` is its least vertex).
    Pinched {
        /// The vertex.
        vert: usize,
    },
    /// The faces at the edge `edge` have opposite outward normals.
    Knife {
        /// The edge's vertices, the lesser first.
        edge: [usize; 2],
    },
    /// The kernel refused the body: the input's range, the budget, or its
    /// check ([`CheckError::outcome`](crate::topo::check::CheckError::outcome)
    /// for a constructed body).
    Kernel(KernelError),
}

impl From<KernelError> for PolyhedronError {
    fn from(e: KernelError) -> Self {
        PolyhedronError::Kernel(e)
    }
}

/// The body of the closed mesh `verts`, `tris` (module docs), its faces
/// keyed for `ctx`, on tolerance `eps` and a budget of `budget`, checked
/// as a constructed body.
pub fn from_polyhedron(
    verts: &[[f64; 3]],
    tris: &[[usize; 3]],
    ctx: OpCtx,
    eps: Tolerance,
    budget: &Budget,
) -> Result<Brep, PolyhedronError> {
    let mut work = Work::new(budget);
    let body = build(verts, tris, ctx, eps, &mut work)?;
    match body.check_in(Scope::Full, None, &mut work, 0) {
        Ok(()) => Ok(body),
        Err(e) => Err(PolyhedronError::Kernel(
            match e.outcome(Role::Constructed) {
                Outcome::Error(e) => e,
                Outcome::Accuracy | Outcome::LadderSignal => defect(),
            },
        )),
    }
}

/// [`from_polyhedron`] unchecked, its work charged to `work`.
pub(crate) fn build(
    verts: &[[f64; 3]],
    tris: &[[usize; 3]],
    ctx: OpCtx,
    eps: Tolerance,
    work: &mut Work,
) -> Result<Brep, PolyhedronError> {
    let mut scan = OperandScan::new();
    for p in verts {
        scan.bounded(p).map_err(KernelError::from)?;
    }
    let start = scan.start(eps.eps()).map_err(KernelError::from)?;
    let mesh = Mesh::new(verts, tris)?;
    let faces = mesh.faces(work)?;
    let mut m = Make {
        b: Builder::new(BodyKind::Solid, Status::new(eps, &start)),
        mesh: &mesh,
        target: seam_target(start.max_coord),
        verts: vec![VertId::NONE; verts.len()],
        edges: BTreeMap::new(),
    };
    m.edges(&faces)?;
    let mut ids = Vec::with_capacity(faces.len());
    for (k, f) in faces.iter().enumerate() {
        let k = u32::try_from(k).map_err(|_| KernelError::Unrepresentable(Storage::OutOfRange))?;
        ids.push(m.face(f, FaceKey::new(ctx, FacePart::Primitive(k)), work)?);
    }
    let b = &mut m.b;
    b.fill_regions(work).map_err(built)?;
    let accuracy = eps.eps() / 4.0;
    let tols: Vec<f64> = (0..b.brep().edge_count())
        .map(|i| b.brep().edge_tol(EdgeId(i as u32)))
        .collect();
    for tol in tols {
        if tol > accuracy {
            let w = Warning::new(WarningKind::Accuracy, tol).ok_or_else(defect)?;
            b.status_mut().op_mut().warn(w);
        }
    }
    Ok(m.b.finish().map_err(built)?.0)
}

/// An exact vector.
type Q3 = [Rational; 3];

fn sub(a: &[f64; 3], b: &[f64; 3]) -> Q3 {
    std::array::from_fn(|k| q(a[k]) - q(b[k]))
}

fn cross(a: &Q3, b: &Q3) -> Q3 {
    let x = |i: usize, j: usize| &a[i] * &b[j] - &a[j] * &b[i];
    [x(1, 2), x(2, 0), x(0, 1)]
}

fn dot(a: &Q3, b: &Q3) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

/// The mesh, validated: its directed edges and the triangle using each.
struct Mesh<'a> {
    verts: &'a [[f64; 3]],
    tris: &'a [[usize; 3]],
    /// Each directed edge `(from, to)`: its triangle.
    half: BTreeMap<(usize, usize), usize>,
}

/// A face: its triangles' plane, the exact outward normal of its least
/// triangle, and its loops (vertex indices in order, the outer first,
/// each from its least vertex).
struct Face {
    key: PlaneKey,
    normal: Q3,
    loops: Vec<Vec<usize>>,
}

impl<'a> Mesh<'a> {
    fn new(verts: &'a [[f64; 3]], tris: &'a [[usize; 3]]) -> Result<Mesh<'a>, PolyhedronError> {
        // A position by its coordinates' bits, `−0` as `+0`.
        let mut at: Vec<([u64; 3], usize)> = verts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.map(|x| (x + 0.0).to_bits()), i))
            .collect();
        at.sort_unstable();
        if let Some(w) = at.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(PolyhedronError::Duplicate {
                verts: [w[0].1, w[1].1],
            });
        }
        let mut half = BTreeMap::new();
        for (t, tri) in tris.iter().enumerate() {
            if tri.iter().any(|&i| i >= verts.len()) {
                return Err(PolyhedronError::Index { tri: t });
            }
            let [a, b, c] = tri.map(|i| verts[i]);
            if cross(&sub(&b, &a), &sub(&c, &a)).iter().all(|x| *x == 0u32) {
                return Err(PolyhedronError::Degenerate { tri: t });
            }
            for k in 0..3 {
                let e = (tri[k], tri[(k + 1) % 3]);
                if half.insert(e, t).is_some() {
                    return Err(PolyhedronError::Open { edge: [e.0, e.1] });
                }
            }
        }
        if let Some((&(a, b), _)) = half
            .iter()
            .find(|&(&(a, b), _)| !half.contains_key(&(b, a)))
        {
            return Err(PolyhedronError::Open { edge: [a, b] });
        }
        let mesh = Mesh { verts, tris, half };
        let mut parts = Parts::new(tris.len());
        for (&(a, b), &t) in &mesh.half {
            parts.join(t, mesh.half[&(b, a)]);
        }
        // Each vertex's triangles in one piece.
        let mut piece = vec![usize::MAX; verts.len()];
        for (t, tri) in tris.iter().enumerate() {
            let r = parts.root(t);
            for &v in tri {
                if piece[v] == usize::MAX {
                    piece[v] = r;
                } else if piece[v] != r {
                    return Err(PolyhedronError::Pieces { vert: v });
                }
            }
        }
        Ok(mesh)
    }

    fn corners(&self, t: usize) -> [[f64; 3]; 3] {
        self.tris[t].map(|i| self.verts[i])
    }

    /// The exact outward normal of triangle `t` (unnormalised).
    fn normal(&self, t: usize) -> Q3 {
        let [a, b, c] = self.corners(t);
        cross(&sub(&b, &a), &sub(&c, &a))
    }

    /// The faces (module docs): the triangles' keys and the coplanarity
    /// across each edge in charged passes, then the triangles merged.
    fn faces(&self, work: &mut Work) -> Result<Vec<Face>, PolyhedronError> {
        let ts: Vec<usize> = (0..self.tris.len()).collect();
        let pass = |stage| Pass { stage, rung: 0 };
        let keys = work
            .pass(pass("polyhedron keys"), &ts, |&t, ledger| {
                let mut m = KeyMeter::new(ledger);
                PlaneKey::through_metered(self.corners(t), &mut m).map_err(Stop::<FormError>::from)
            })
            .map_err(|h| self.halted(h))?;
        // Each edge once, by its lesser vertex first.
        let edges: Vec<(usize, usize)> = self.half.keys().copied().filter(|(a, b)| a < b).collect();
        let coplanar = work
            .pass(pass("polyhedron planes"), &edges, |&(a, b), ledger| {
                let (t, u) = (self.half[&(a, b)], self.half[&(b, a)]);
                ledger.charge(Step::Work, 1)?;
                if FormKey::from(keys[t]) != FormKey::from(keys[u]) {
                    return Ok(false);
                }
                // Facing one way: a fold onto the plane is a knife edge.
                if dot(&self.normal(t), &self.normal(u)) <= 0u32 {
                    return Ok(false);
                }
                let far = self.tris[u]
                    .into_iter()
                    .find(|&i| i != a && i != b)
                    .expect("a triangle's third corner");
                let p = |i: usize| DVec3::from_array(self.verts[i]);
                let [x, y, z] = self.tris[t].map(p);
                let (s, w) = value_sign(&Orient3d([x, y, z, p(far)]));
                let parts = w.parts(&UNITS.record);
                ledger.charge(Step::Work, parts.div_ceil(UNIT_PARTS))?;
                Ok::<_, Stop<Infallible>>(s == Ok(Ordering::Equal))
            })
            .map_err(|h| self.halted(h))?;
        let mut parts = Parts::new(self.tris.len());
        for (&(a, b), &same) in edges.iter().zip(&coplanar) {
            if same {
                parts.join(self.half[&(a, b)], self.half[&(b, a)]);
            }
        }
        // Faces by their least triangle.
        let mut face_of: BTreeMap<usize, usize> = BTreeMap::new();
        let mut first = Vec::new();
        for t in 0..self.tris.len() {
            let r = parts.root(t);
            if let std::collections::btree_map::Entry::Vacant(e) = face_of.entry(r) {
                e.insert(first.len());
                first.push(t);
            }
        }
        let face = |t: usize| face_of[&parts.root(t)];
        // Each face's boundary: the directed edges whose reverse is
        // another face's, by their start.
        let mut out: Vec<BTreeMap<usize, Vec<usize>>> = vec![BTreeMap::new(); first.len()];
        for (&(a, b), &t) in &self.half {
            let f = face(t);
            if face(self.half[&(b, a)]) != f {
                out[f].entry(a).or_default().push(b);
            }
        }
        let mut faces = Vec::with_capacity(first.len());
        for (f, starts) in out.into_iter().enumerate() {
            let t = first[f];
            let normal = self.normal(t);
            let loops = self.loops(starts, &normal)?;
            faces.push(Face {
                key: keys[t],
                normal,
                loops,
            });
        }
        Ok(faces)
    }

    /// A face's loops from its boundary's edges `next` (by start), its
    /// outward normal `n`: the outer loop first, then the holes by their
    /// least vertex.
    fn loops(
        &self,
        mut next: BTreeMap<usize, Vec<usize>>,
        n: &Q3,
    ) -> Result<Vec<Vec<usize>>, PolyhedronError> {
        if let Some((&v, _)) = next.iter().find(|(_, to)| to.len() != 1) {
            return Err(PolyhedronError::Pinched { vert: v });
        }
        let least = next.keys().next().copied().unwrap_or(0);
        let mut outer = Vec::new();
        let mut holes = Vec::new();
        while let Some((&start, _)) = next.iter().next() {
            let mut ring = vec![start];
            let mut v = next.remove(&start).expect("a start")[0];
            while v != start {
                ring.push(v);
                v = match next.remove(&v) {
                    Some(to) => to[0],
                    None => return Err(PolyhedronError::Pinched { vert: v }),
                };
            }
            // Twice the exact area vector, against the normal.
            let o = self.verts[ring[0]];
            let mut area = [0, 1, 2].map(|_| Rational::from(0u32));
            for k in 1..ring.len().saturating_sub(1) {
                let c = cross(
                    &sub(&self.verts[ring[k]], &o),
                    &sub(&self.verts[ring[k + 1]], &o),
                );
                for (a, c) in area.iter_mut().zip(c) {
                    *a += c;
                }
            }
            if dot(&area, n) > 0u32 {
                outer.push(ring);
            } else {
                holes.push(ring);
            }
        }
        if outer.len() != 1 {
            return Err(PolyhedronError::Pinched { vert: least });
        }
        outer.extend(holes);
        Ok(outer)
    }

    /// A pass's halt: running out as it is; a key's refusal is the
    /// input's range (the corners are finite, bounded and not on a line).
    fn halted(&self, h: Halt<impl Sized>) -> PolyhedronError {
        match h {
            Halt::OutOfBudget(e) => e.into(),
            Halt::Failed { .. } => {
                PolyhedronError::Kernel(KernelError::Unrepresentable(Storage::OutOfRange))
            }
        }
    }
}

/// Disjoint sets of triangles, each named by its least member.
struct Parts(Vec<usize>);

impl Parts {
    fn new(n: usize) -> Parts {
        Parts((0..n).collect())
    }

    fn root(&self, mut t: usize) -> usize {
        while self.0[t] != t {
            t = self.0[t];
        }
        t
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        let (lo, hi) = (a.min(b), a.max(b));
        self.0[hi] = lo;
    }
}

/// The body being built.
struct Make<'m> {
    b: Builder,
    mesh: &'m Mesh<'m>,
    target: f64,
    /// Each mesh vertex's, once added.
    verts: Vec<VertId>,
    /// Each boundary edge, by its vertices (the lesser first).
    edges: BTreeMap<(usize, usize), EdgeId>,
}

impl Make<'_> {
    /// The vertices and edges of the faces' loops, in the order of the
    /// mesh's vertices, then of the edges' ends; each edge's crease
    /// convex or concave (module docs).
    fn edges(&mut self, faces: &[Face]) -> Result<(), PolyhedronError> {
        // Each boundary edge, in its direction along its face's loop:
        // that face's normal.
        let mut along: BTreeMap<(usize, usize), &Q3> = BTreeMap::new();
        for f in faces {
            for ring in &f.loops {
                for k in 0..ring.len() {
                    along.insert((ring[k], ring[(k + 1) % ring.len()]), &f.normal);
                }
            }
        }
        let mut used: Vec<usize> = along.keys().map(|&(a, _)| a).collect();
        used.sort_unstable();
        used.dedup();
        for v in used {
            self.verts[v] = self.b.add_vert(self.mesh.verts[v]).map_err(built)?;
        }
        let pos = |i: usize| self.mesh.verts[i];
        for (&(a, b), n) in &along {
            if a > b {
                continue;
            }
            let m = along[&(b, a)];
            let c = dot(&cross(n, m), &sub(&pos(b), &pos(a)));
            let convexity = match c.partial_cmp(&0u32) {
                Some(Ordering::Greater) => Convexity::Convex,
                Some(Ordering::Less) => Convexity::Concave,
                _ => return Err(PolyhedronError::Knife { edge: [a, b] }),
            };
            let curve = self.b.add_curve(line(pos(a), pos(b))).map_err(built)?;
            let e = self
                .b
                .add_edge(curve, [0.0, 1.0], [self.verts[a], self.verts[b]])
                .map_err(built)?;
            self.b.set_edge_kind(e, EdgeKind::Regular);
            self.b.set_cont(e, Continuity::Crease(convexity));
            self.edges.insert((a, b), e);
        }
        Ok(())
    }

    /// Face `f` keyed `key`: its surface, its loops' p-curves and their
    /// seam bounds.
    fn face(&mut self, f: &Face, key: FaceKey, work: &mut Work) -> Result<FaceId, PolyhedronError> {
        let l = lattice(Lattice::new(f.key.into()))?;
        let surface = lattice(Surface::new(l.clone(), lattice(l.half(0))?))?;
        let flipped = dot(&f.key.normal().map(q), &f.normal) < 0u32;
        let s = self
            .b
            .add_surface(surface, None, ExactRows::default())
            .map_err(built)?;
        let id = self.b.add_face(s, flipped, key).map_err(built)?;
        for (k, ring) in f.loops.iter().enumerate() {
            let mut pairs = Vec::with_capacity(ring.len());
            let mut reversed = Vec::with_capacity(ring.len());
            for i in 0..ring.len() {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                let (lo, hi) = (a.min(b), a.max(b));
                let [p, r] = [lo, hi].map(|v| plane_uv(&l, &self.mesh.verts[v]));
                let pcurve = PCurve::from_spans(
                    vec![PSpan::Linear([[p[0], p[1], 1.0], [r[0], r[1], 1.0]])],
                    vec![0.0, 1.0],
                )
                .map_err(|_| defect())?;
                let pcurve = pcurve.split_at_tile_lines().map_err(|_| defect())?;
                let pc = self.b.add_pcurve(pcurve).map_err(built)?;
                pairs.push((self.edges[&(lo, hi)], pc));
                reversed.push(a > b);
            }
            let bounds = self
                .b
                .seam_bounds(s, &pairs, self.target, work)
                .map_err(built)?;
            let uses: Vec<Use> = pairs
                .iter()
                .zip(reversed)
                .zip(bounds)
                .map(|((&(edge, pcurve), reversed), seam_bound)| Use {
                    edge,
                    reversed,
                    pcurve,
                    seam_bound,
                })
                .collect();
            self.b.add_loop(id, k == 0, &uses).map_err(built)?;
        }
        Ok(id)
    }
}
