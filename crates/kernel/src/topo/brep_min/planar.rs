//! Bodies of planar polygon faces, and the planar subset of `check`.
//!
//! **Building** ([`Brep::from_polygons`]). A body given as vertex
//! positions and faces, each one loop of vertex indices running
//! counter-clockwise about the face's outward normal, is stored with:
//! - per face, a plane surface: the plane through its first corner with
//!   the loop's Newell normal (rounded to its key, [`PlaneKey`]), the
//!   lattice's one base tile; the face is `flipped` where the key's
//!   normal points against the loop's (the key's normal is the Newell
//!   normal normalised and sign-ruled, so their dot product is `±` its
//!   length, never near zero);
//! - per undirected corner pair, one edge on a line span from the lower
//!   vertex id to the higher (canonical), its middle control point the
//!   rounded midpoint: straight exactly where the two positions' sums
//!   are exact (vertices on a common grid), else a hair off the chord,
//!   which the seam bounds cover;
//! - per corner, its UV on the face's plane, `(x − o)·axis / CELL + ½`
//!   rounded to a multiple of `2⁻⁵²` ([`UV_GRID`]): every UV of a plane's
//!   base tile lies in `(0, 1)`, so the sum of two is exact and each
//!   coedge's p-curve, a line span between its ends' UVs with the
//!   midpoint as its middle control point, is **straight exactly**. The
//!   rounding moves a point by at most `2⁻⁵³·CELL ≈ 5·10⁻¹⁰` mm along the
//!   plane, inside the seam bound; a vertex's UV is computed once per
//!   face, so consecutive coedges join bit for bit.
//!
//! Vertex ids are the input's indices, edges are numbered as the faces'
//! corners first reach them, in input order. The planes are made in one
//! pass over the faces, each charged as one rational run of the record
//! units; the seam bounds as [`Brep::add_loop`] charges them, a pass per
//! face.
//!
//! **The planar check** ([`Brep::check_planar`]): the invariants of
//! `check` that a body of planar faces needs, in order, the first fault
//! found (ids ascending within each item) returned:
//! 1. *topology*: a coedge's pair is a coedge of the same edge running
//!    the other way whose pair is it, or `NONE` on a sheet's open
//!    boundary (a solid has none); the edge's stored coedge is one of the
//!    two; every edge has a coedge; `next` and `prev` are inverse; each
//!    loop's `next` cycle closes through coedges of that loop and face
//!    only, and every coedge lies on its loop's cycle; each face's loops
//!    are its own, every loop is on its face's list, and a face has one
//!    outer loop; the coedges ending at a vertex form **one fan** (the
//!    walk `c → pair(next(c))` round the vertex reaches them all: a cycle,
//!    or on a sheet's boundary one chain from its open coedge), and the
//!    vertex's stored coedge leaves it;
//! 2. the ribbon joins ([`Brep::check_ribbons`]);
//! 3. *domain*: every p-curve control point lies in its face's lattice
//!    rectangle (so does the p-curve, inside its control points' hull);
//! 4. *trims*, on every face whose p-curves are straight exactly (every
//!    span's weights 1, its middle control point the exact midpoint of
//!    its ends) with UVs within the exact predicates' input range: the
//!    loops, as polygons of their spans' ends, are simple and mutually
//!    disjoint (no two segments meet but consecutive ones at their shared
//!    corner, and those don't fold back), the outer loop runs
//!    counter-clockwise in the face's orientation (clockwise in UV on a
//!    flipped face) and the holes the other way, and each hole lies in
//!    the outer loop and in no other hole; all by exact flat predicates
//!    on the stored UVs. Faces with curved p-curves are listed, unchecked
//!    ([`PlanarCheck::unchecked_trims`]). Two loops of a face may not
//!    meet even at a shared vertex here (the full check admits that at a
//!    contact point);
//! 5. *seams*: each coedge's seam bound is at most its edge's tolerance,
//!    which is at most each end vertex's.
//!
//! Not checked here: shells and regions (not stored yet), the tile-line
//! joins (c) and (d), the seam bounds' recomputation, and the
//! certificates of contacts and embedding. The check is a validator and
//! charges no budget.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use varde_decide::num::exact::{MAX_INPUT, Num, Pred, value_sign};
use varde_decide::num::schedule::PredKind;
use varde_decide::num::schedule::UNITS;

use super::{BodyKind, Brep, CoedgeId, EdgeId, FaceId, LoopId, RibbonFault, StoreError, VertId};
use crate::geom::curve::{Curve3D, CurveError, PCurve, Span2, Span3};
use crate::geom::form::{FormError, FormKey, PlaneKey};
use crate::geom::lattice::{CELL, Lattice, LatticeError};
use crate::geom::surface::Surface;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

/// The grid a plane face's corner UVs are rounded to, `2⁻⁵²` (module
/// docs).
pub const UV_GRID: f64 = f64::EPSILON;

/// Why [`Brep::from_polygons`] can't build a body.
#[derive(Clone, Debug, PartialEq)]
pub enum PolygonError {
    /// Vertex `index` can't be added.
    Vert { index: usize, why: StoreError },
    /// Face `face` can't be.
    Face { face: usize, why: FaceError },
    /// The faces' planes ran out of budget ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// Why a face of [`Brep::from_polygons`] can't be built.
#[derive(Clone, Debug, PartialEq)]
pub enum FaceError {
    /// Fewer than three corners.
    Corners,
    /// A corner names no vertex.
    Index,
    /// A vertex is a corner twice.
    Repeated,
    /// Its plane has no key: a zero Newell normal (collinear corners), or
    /// a value out of range.
    Plane(FormError),
    /// Its plane's lattice or tile can't be made.
    Lattice(LatticeError),
    /// An edge curve or p-curve can't be made (a value out of range).
    Curve(CurveError),
    /// An entity can't be added: among them an edge a third face uses, or
    /// a second one the same way (`StoreError::Coedge`), or a seam bound
    /// out of budget.
    Store(StoreError),
}

/// The first fault [`Brep::check_planar`] finds (module docs, items 1–5
/// in order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckFault {
    /// 1: a solid's coedge has no pair.
    Open(CoedgeId),
    /// 1: the coedge's pair isn't its edge's other coedge, running the
    /// other way and paired back; or its edge doesn't store it or its
    /// pair.
    Pair(CoedgeId),
    /// 1: the edge has no coedge.
    Unused(EdgeId),
    /// 1: `prev(next(c)) ≠ c`.
    NextPrev(CoedgeId),
    /// 1: the loop's cycle doesn't close through its own coedges, or a
    /// coedge naming it lies off it.
    Loop(LoopId),
    /// 1: the face's loops aren't its own, a loop isn't on its face's
    /// list, or the face has no outer loop or several.
    FaceLoops(FaceId),
    /// 1: the coedges ending at the vertex form more than one fan.
    Fan(VertId),
    /// 1: the vertex's coedge doesn't leave it (or is missing, or there
    /// while no coedge reaches it).
    VertCoedge(VertId),
    /// 2: a ribbon join fails.
    Ribbon(RibbonFault),
    /// 3: the coedge's p-curve leaves its face's lattice rectangle.
    Domain(CoedgeId),
    /// 4: two segments of the face's loops meet (or one has length zero,
    /// or two consecutive ones fold back).
    Crossing(FaceId),
    /// 4: the loop runs the wrong way for its kind (outer or hole), or
    /// bounds no area.
    Orientation(LoopId),
    /// 4: the hole lies outside its face's outer loop or inside another
    /// hole.
    Nesting(LoopId),
    /// 5: the coedge's seam bound exceeds its edge's tolerance, or the
    /// edge's exceeds a vertex's.
    Seam(CoedgeId),
}

/// What [`Brep::check_planar`] passed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanarCheck {
    /// The faces whose trims weren't checked (item 4): a p-curve not
    /// straight, or a UV beyond the predicates' range.
    pub unchecked_trims: Vec<FaceId>,
}

/// A face's plane, made in the planes' pass.
struct Plane {
    surface: Surface,
    flipped: bool,
    /// Per corner, its UV.
    uv: Vec<[f64; 2]>,
}

impl Brep {
    /// The body of kind `kind` with the vertices `verts` and the planar
    /// polygon faces `faces` (module docs), seam bounds refined towards
    /// `target` on `work`; or the first input it can't take: a vertex, a
    /// face's corners (in face order), a plane (first in face order, or
    /// running out of budget), then an entity of a face, in face order.
    /// The edges a face's corners share with earlier faces are those
    /// faces', so a closed body's faces must use each edge once each way,
    /// a sheet's once or once each way.
    #[allow(dead_code, reason = "moved ahead of the operations that build bodies")]
    pub(crate) fn from_polygons(
        kind: BodyKind,
        verts: &[[f64; 3]],
        faces: &[Vec<u32>],
        target: f64,
        work: &mut Work,
    ) -> Result<Brep, PolygonError> {
        let mut b = Brep::with_kind(kind);
        let ids = verts
            .iter()
            .enumerate()
            .map(|(index, p)| {
                b.add_vert(*p)
                    .map_err(|why| PolygonError::Vert { index, why })
            })
            .collect::<Result<Vec<VertId>, _>>()?;
        for (face, f) in faces.iter().enumerate() {
            let why = |why| PolygonError::Face { face, why };
            if f.len() < 3 {
                return Err(why(FaceError::Corners));
            }
            if f.iter().any(|&v| v as usize >= verts.len()) {
                return Err(why(FaceError::Index));
            }
            let mut sorted = f.clone();
            sorted.sort_unstable();
            if sorted.windows(2).any(|w| w[0] == w[1]) {
                return Err(why(FaceError::Repeated));
            }
        }
        let pass = Pass {
            stage: "planes",
            rung: 0,
        };
        let planes = work
            .pass(pass, faces, |f, ledger| plane_of(verts, f, ledger))
            .map_err(|halt| match halt {
                Halt::Failed { item, error } => PolygonError::Face {
                    face: item,
                    why: error,
                },
                Halt::OutOfBudget(e) => PolygonError::OutOfBudget(e),
            })?;
        let mut edges: BTreeMap<[u32; 2], EdgeId> = BTreeMap::new();
        for (face, (f, plane)) in faces.iter().zip(planes).enumerate() {
            let fail = |why| PolygonError::Face { face, why };
            let store = |why| fail(FaceError::Store(why));
            let s = b.add_surface(plane.surface).map_err(store)?;
            let id = b.add_face(s, plane.flipped).map_err(store)?;
            let n = f.len();
            let mut uses = Vec::with_capacity(n);
            for k in 0..n {
                let (from, to) = (f[k], f[(k + 1) % n]);
                let (uv_from, uv_to) = (plane.uv[k], plane.uv[(k + 1) % n]);
                let reversed = from > to;
                let (lo, hi, uv_lo, uv_hi) = if reversed {
                    (to, from, uv_to, uv_from)
                } else {
                    (from, to, uv_from, uv_to)
                };
                let e = match edges.get(&[lo, hi]) {
                    Some(&e) => e,
                    None => {
                        let (a, z) = (verts[lo as usize], verts[hi as usize]);
                        let c = Curve3D::new(vec![line(a, z)], vec![0.0, 1.0])
                            .map_err(|e| fail(FaceError::Curve(e)))?;
                        let c = b.add_curve(c).map_err(store)?;
                        let e = b
                            .add_edge(c, [0.0, 1.0], ids[lo as usize], ids[hi as usize])
                            .map_err(store)?;
                        edges.insert([lo, hi], e);
                        e
                    }
                };
                let p = PCurve::new(vec![uv_line(uv_lo, uv_hi)], vec![0.0, 1.0])
                    .map_err(|e| fail(FaceError::Curve(e)))?;
                uses.push((e, reversed, p));
            }
            b.add_loop(id, true, uses, target, work).map_err(store)?;
        }
        Ok(b)
    }

    /// The planar subset of `check` (module docs): what it passed, or the
    /// first fault.
    pub fn check_planar(&self) -> Result<PlanarCheck, CheckFault> {
        self.check_topology()?;
        self.check_ribbons().map_err(CheckFault::Ribbon)?;
        self.check_domain()?;
        let unchecked_trims = self.check_trims()?;
        self.check_seams()?;
        Ok(PlanarCheck { unchecked_trims })
    }

    /// Item 1.
    fn check_topology(&self) -> Result<(), CheckFault> {
        let co = &self.coedges;
        let n = co.edge.len();
        let ne = self.edges.curve.len();
        let valid = |c: CoedgeId| c.index() < n;
        for c in 0..n {
            let id = CoedgeId(c as u32);
            let e = co.edge[c];
            let p = co.pair[c];
            if p == CoedgeId::NONE {
                if self.kind == BodyKind::Solid {
                    return Err(CheckFault::Open(id));
                }
            } else if !valid(p)
                || p == id
                || co.pair[p.index()] != id
                || co.edge[p.index()] != e
                || co.reversed[p.index()] == co.reversed[c]
            {
                return Err(CheckFault::Pair(id));
            }
            let stored = match self.edges.coedge.get(e.index()) {
                Some(&s) if e.index() < ne => s,
                _ => return Err(CheckFault::Pair(id)),
            };
            if stored == CoedgeId::NONE || (stored != id && stored != p) {
                return Err(CheckFault::Pair(id));
            }
        }
        if let Some(e) = self.edges.coedge.iter().position(|&c| c == CoedgeId::NONE) {
            return Err(CheckFault::Unused(EdgeId(e as u32)));
        }
        for c in 0..n {
            let next = co.next[c];
            if !valid(next) || co.prev[next.index()].index() != c {
                return Err(CheckFault::NextPrev(CoedgeId(c as u32)));
            }
        }
        // Each loop's cycle.
        let nl = self.loops.face.len();
        let mut on_cycle = vec![false; n];
        for l in 0..nl {
            let id = LoopId(l as u32);
            let face = self.loops.face[l];
            let first = self.loops.first_coedge[l];
            let mut at = first;
            let mut steps = 0;
            loop {
                if !valid(at) || on_cycle[at.index()] || steps >= n {
                    return Err(CheckFault::Loop(id));
                }
                if co.lp[at.index()] != id || co.face[at.index()] != face {
                    return Err(CheckFault::Loop(id));
                }
                on_cycle[at.index()] = true;
                steps += 1;
                at = co.next[at.index()];
                if at == first {
                    break;
                }
            }
        }
        if let Some(c) = on_cycle.iter().position(|&s| !s) {
            return Err(CheckFault::Loop(co.lp[c]));
        }
        // Each face's loops.
        let mut listed = vec![false; nl];
        for f in 0..self.faces.surface.len() {
            let id = FaceId(f as u32);
            let mut at = self.faces.first_loop[f];
            let mut outer = 0;
            let mut steps = 0;
            while at != LoopId::NONE {
                if at.index() >= nl
                    || listed[at.index()]
                    || steps >= nl
                    || self.loops.face[at.index()] != id
                {
                    return Err(CheckFault::FaceLoops(id));
                }
                listed[at.index()] = true;
                outer += usize::from(self.loops.is_outer[at.index()]);
                steps += 1;
                at = self.loops.next_loop[at.index()];
            }
            if outer != 1 {
                return Err(CheckFault::FaceLoops(id));
            }
        }
        if let Some(l) = listed.iter().position(|&s| !s) {
            return Err(CheckFault::FaceLoops(self.loops.face[l]));
        }
        // Each vertex's fan.
        let nv = self.verts.pos.len();
        let mut ending: Vec<Vec<CoedgeId>> = vec![Vec::new(); nv];
        for c in 0..n {
            let id = CoedgeId(c as u32);
            if let Some(v) = self.coedge_vert(id, true) {
                ending[v.index()].push(id);
            }
        }
        for (v, ends) in ending.iter().enumerate() {
            let id = VertId(v as u32);
            let leaving = self.verts.coedge[v];
            let leaves = valid(leaving) && self.coedge_vert(leaving, false) == Some(id);
            if ends.is_empty() != (leaving == CoedgeId::NONE) || !(ends.is_empty() || leaves) {
                return Err(CheckFault::VertCoedge(id));
            }
            let Some(&any) = ends.first() else { continue };
            let mut open = ends
                .iter()
                .filter(|&&c| co.pair[c.index()] == CoedgeId::NONE);
            let start = open.next().copied().unwrap_or(any);
            if open.next().is_some() {
                return Err(CheckFault::Fan(id));
            }
            let mut at = start;
            let mut reached = 1;
            loop {
                let pair = co.pair[co.next[at.index()].index()];
                if pair == CoedgeId::NONE || pair == start {
                    break;
                }
                reached += 1;
                if reached > ends.len() {
                    return Err(CheckFault::Fan(id));
                }
                at = pair;
            }
            if reached != ends.len() {
                return Err(CheckFault::Fan(id));
            }
        }
        Ok(())
    }

    /// Item 3.
    fn check_domain(&self) -> Result<(), CheckFault> {
        for c in 0..self.coedges.edge.len() {
            let id = CoedgeId(c as u32);
            let rect = self.surface(self.face(id)).rect();
            let (u0, u1) = (rect.u.start as f64, rect.u.end as f64);
            let (v0, v1) = (rect.v.start as f64, rect.v.end as f64);
            let inside = self.coedges.pcurve[c]
                .spans()
                .iter()
                .flatten()
                .all(|p| u0 <= p[0] && p[0] <= u1 && v0 <= p[1] && p[1] <= v1);
            if !inside {
                return Err(CheckFault::Domain(id));
            }
        }
        Ok(())
    }

    /// Item 4: the faces left unchecked, or the first fault.
    fn check_trims(&self) -> Result<Vec<FaceId>, CheckFault> {
        let mut unchecked = Vec::new();
        for f in 0..self.faces.surface.len() {
            let id = FaceId(f as u32);
            let mut loops: Vec<(LoopId, Vec<[f64; 2]>)> = Vec::new();
            let mut straight = true;
            for l in self.loops(id) {
                let mut corners = Vec::new();
                for c in self.loop_coedges(l) {
                    let p = self.pcurve(c);
                    straight &= p.spans().iter().all(is_uv_line);
                    let k = p.spans().len();
                    let at = |i: usize| p.knot_point(i).unwrap_or([f64::NAN; 2]);
                    if self.reversed(c) {
                        corners.extend((1..=k).rev().map(at));
                    } else {
                        corners.extend((0..k).map(at));
                    }
                }
                loops.push((l, corners));
            }
            let in_range = loops
                .iter()
                .flat_map(|(_, c)| c.iter().flatten())
                .all(|x| x.abs() <= MAX_INPUT);
            if !(straight && in_range) {
                unchecked.push(id);
                continue;
            }
            self.check_face_trims(id, &loops)?;
        }
        Ok(unchecked)
    }

    /// Item 4 on one face's loops, as polygons of their corners.
    fn check_face_trims(
        &self,
        f: FaceId,
        loops: &[(LoopId, Vec<[f64; 2]>)],
    ) -> Result<(), CheckFault> {
        let crossing = Err(CheckFault::Crossing(f));
        // (loop, index in it, start, end)
        let segs: Vec<(usize, usize, [f64; 2], [f64; 2])> = loops
            .iter()
            .enumerate()
            .flat_map(|(l, (_, c))| (0..c.len()).map(move |i| (l, i, c[i], c[(i + 1) % c.len()])))
            .collect();
        if segs.iter().any(|s| same_point(s.2, s.3)) {
            return crossing;
        }
        for (k, s) in segs.iter().enumerate() {
            for t in &segs[k + 1..] {
                let len = loops[s.0].1.len();
                let meets = if s.0 == t.0 && t.1 == s.1 + 1 {
                    folds(s.2, s.3, t.3)
                } else if s.0 == t.0 && s.1 == 0 && t.1 == len - 1 {
                    folds(t.2, t.3, s.3)
                } else {
                    segments_meet(s.2, s.3, t.2, t.3)
                };
                if meets {
                    return crossing;
                }
            }
        }
        let ccw = if self.flipped(f) {
            Ordering::Less
        } else {
            Ordering::Greater
        };
        for (l, c) in loops {
            let want = if self.is_outer(*l) {
                ccw
            } else {
                ccw.reverse()
            };
            if sign(&Area(c)) != Some(want) {
                return Err(CheckFault::Orientation(*l));
            }
        }
        let Some(outer) = loops.iter().position(|(l, _)| self.is_outer(*l)) else {
            return Err(CheckFault::FaceLoops(f));
        };
        for (h, (l, c)) in loops.iter().enumerate() {
            if h == outer {
                continue;
            }
            let p = c[0];
            let in_others = loops
                .iter()
                .enumerate()
                .any(|(o, (_, other))| o != h && o != outer && inside(p, other));
            if !inside(p, &loops[outer].1) || in_others {
                return Err(CheckFault::Nesting(*l));
            }
        }
        Ok(())
    }

    /// Item 5.
    fn check_seams(&self) -> Result<(), CheckFault> {
        for c in 0..self.coedges.edge.len() {
            let id = CoedgeId(c as u32);
            let e = self.coedges.edge[c];
            let tol = self.edge_tol(e);
            let [vs, ve] = self.ends(e);
            let held = self.coedges.seam_bound[c] <= tol
                && tol <= self.vert_tol(vs)
                && tol <= self.vert_tol(ve);
            if !held {
                return Err(CheckFault::Seam(id));
            }
        }
        Ok(())
    }
}

/// The plane of face `f` (module docs), charged as one rational run.
fn plane_of(
    verts: &[[f64; 3]],
    f: &[u32],
    ledger: &mut Ledger<'_>,
) -> Result<Plane, Stop<FaceError>> {
    ledger.charge(Step::Work, UNITS.record.rational)?;
    let pts: Vec<[f64; 3]> = f.iter().map(|&v| verts[v as usize]).collect();
    let n = newell(&pts);
    let key = PlaneKey::new(n, pts[0]).map_err(|e| Stop::Failed(FaceError::Plane(e)))?;
    let lattice =
        Lattice::new(FormKey::Plane(key)).map_err(|e| Stop::Failed(FaceError::Lattice(e)))?;
    let rect = lattice
        .half(0)
        .map_err(|e| Stop::Failed(FaceError::Lattice(e)))?;
    let k = key.normal();
    let flipped = k[0] * n[0] + k[1] * n[1] + k[2] * n[2] < 0.0;
    let uv = pts.iter().map(|p| plane_uv(&lattice, *p)).collect();
    let surface = Surface::new(lattice, rect).map_err(|e| Stop::Failed(FaceError::Lattice(e)))?;
    Ok(Plane {
        surface,
        flipped,
        uv,
    })
}

/// The Newell normal of a polygon (twice its vector area): along the
/// side it runs counter-clockwise about.
fn newell(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for (k, a) in pts.iter().enumerate() {
        let b = pts[(k + 1) % pts.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// `p`'s UV on a plane's base tile, on [`UV_GRID`] (module docs).
pub(super) fn plane_uv(lattice: &Lattice, p: [f64; 3]) -> [f64; 2] {
    let (o, [u, v, _]) = (lattice.origin(), lattice.axes());
    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let along = |a: [f64; 3]| {
        let x = (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
        (x / UV_GRID).round() * UV_GRID
    };
    [along(u), along(v)]
}

/// The line span from `a` to `b`, its middle control point the rounded
/// midpoint, weights 1.
pub(super) fn line(a: [f64; 3], b: [f64; 3]) -> Span3 {
    let m = [0, 1, 2].map(|k| 0.5 * (a[k] + b[k]));
    [
        [a[0], a[1], a[2], 1.0],
        [m[0], m[1], m[2], 1.0],
        [b[0], b[1], b[2], 1.0],
    ]
}

/// The UV line span from `a` to `b`, as [`line`].
pub(super) fn uv_line(a: [f64; 2], b: [f64; 2]) -> Span2 {
    let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

/// Whether a UV span is straight exactly: weights 1, the middle control
/// point the exact midpoint of the ends (their sum exact).
fn is_uv_line(s: &Span2) -> bool {
    s.iter().all(|p| p[2] == 1.0)
        && (0..2).all(|k| {
            let (a, b) = (s[0][k], s[2][k]);
            let sum = a + b;
            // The rounding error of `a + b` (Knuth's two-sum).
            let bb = sum - a;
            let err = (a - (sum - bb)) + (b - bb);
            err == 0.0 && (0.5 * sum).to_bits() == s[1][k].to_bits() && 2.0 * s[1][k] == sum
        })
}

fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    a[0] == b[0] && a[1] == b[1]
}

/// The exact sign of a predicate on stored UVs; `None` for an input that
/// isn't finite.
fn sign(p: &impl Pred) -> Option<Ordering> {
    value_sign(p).0.ok()
}

/// `orient(a, b, c)`: positive when `c` lies left of `a → b`. Also the
/// face arrangements' one flat predicate ([`crate::topo::arrange2d`]).
pub(crate) struct Orient(pub(crate) [[f64; 2]; 3]);

impl Pred for Orient {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c] = self.0.map(|p| p.map(N::lit));
        let (bx, by) = (b[0].sub(&a[0]), b[1].sub(&a[1]));
        let (cx, cy) = (c[0].sub(&a[0]), c[1].sub(&a[1]));
        bx.mul(&cy).sub(&by.mul(&cx))
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient2
    }
}

/// `(b − a)·(d − c)`.
struct Dot([[f64; 2]; 4]);

impl Pred for Dot {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c, d] = self.0.map(|p| p.map(N::lit));
        let (x, y) = (b[0].sub(&a[0]), b[1].sub(&a[1]));
        let (z, w) = (d[0].sub(&c[0]), d[1].sub(&c[1]));
        x.mul(&z).add(&y.mul(&w))
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient2
    }
}

/// Twice a polygon's signed area: positive when it runs
/// counter-clockwise.
struct Area<'a>(&'a [[f64; 2]]);

impl Pred for Area<'_> {
    fn eval<N: Num>(&self) -> N {
        let c = self.0;
        let mut sum = N::lit(0.0);
        for (k, a) in c.iter().enumerate() {
            let b = c[(k + 1) % c.len()];
            let term = N::lit(a[0])
                .mul(&N::lit(b[1]))
                .sub(&N::lit(b[0]).mul(&N::lit(a[1])));
            sum = sum.add(&term);
        }
        sum
    }

    /// Of any length, so no kind's price fits it: the check charges no
    /// budget.
    fn kind(&self) -> PredKind {
        PredKind::Orient2
    }
}

/// Whether the segments `a → b` and `b → c`, consecutive, fold back:
/// collinear with `c` not ahead of `b`.
fn folds(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    match sign(&Orient([a, b, c])) {
        Some(Ordering::Equal) => sign(&Dot([a, b, b, c])) != Some(Ordering::Greater),
        Some(_) => false,
        None => true,
    }
}

/// Whether the closed segments `a b` and `c d` share a point, exactly.
fn segments_meet(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let o = [
        sign(&Orient([a, b, c])),
        sign(&Orient([a, b, d])),
        sign(&Orient([c, d, a])),
        sign(&Orient([c, d, b])),
    ];
    let [Some(o1), Some(o2), Some(o3), Some(o4)] = o else {
        return true;
    };
    let opposite = |x: Ordering, y: Ordering| x != Ordering::Equal && x == y.reverse();
    if opposite(o1, o2) && opposite(o3, o4) {
        return true;
    }
    let eq = Ordering::Equal;
    (o1 == eq && in_box(a, b, c))
        || (o2 == eq && in_box(a, b, d))
        || (o3 == eq && in_box(c, d, a))
        || (o4 == eq && in_box(c, d, b))
}

/// Whether `p` lies in the box of `a` and `b` (closed).
fn in_box(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> bool {
    (0..2).all(|k| {
        let (lo, hi) = if a[k] <= b[k] {
            (a[k], b[k])
        } else {
            (b[k], a[k])
        };
        lo <= p[k] && p[k] <= hi
    })
}

/// Whether `p`, on no segment of the polygon, lies inside it: the parity
/// of its crossings with the ray from `p` along `+u`, each decided
/// exactly (a segment counts where one end lies strictly above `p`'s
/// line and the other not).
fn inside(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut odd = false;
    for (k, a) in poly.iter().enumerate() {
        let b = poly[(k + 1) % poly.len()];
        if (a[1] > p[1]) == (b[1] > p[1]) {
            continue;
        }
        // The crossing lies right of `p` where `p` is left of the
        // upward-directed segment.
        let o = sign(&Orient([*a, b, p]));
        let up = b[1] > a[1];
        if (up && o == Some(Ordering::Greater)) || (!up && o == Some(Ordering::Less)) {
            odd = !odd;
        }
    }
    odd
}
