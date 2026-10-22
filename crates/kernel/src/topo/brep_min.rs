//! The minimal B-rep store: vertices, edges on rational quadratic space
//! curves, coedges with Q p-curves and their seam bounds, loops and
//! faces, as structure-of-arrays columns indexed by `u32` ids, `NONE`
//! (`u32::MAX`) for "no entity", and the body's kind: a closed `Solid`
//! or an open `Sheet` ([`BodyKind`]). No shells, regions, contacts or
//! names yet; of `check`, the joins the ribbons need
//! ([`Brep::check_ribbons`]) and the planar subset
//! ([`Brep::check_planar`], [`planar`]), which also builds bodies of
//! planar polygon faces.
//!
//! **Entities.**
//! - A vertex: its position `pos` and tolerance `tol` (raised to the
//!   tolerance of every edge at it), one coedge leaving it.
//! - An edge: its curve and the curve's parameter range `t_range` (two
//!   knots of the curve), the vertices at its ends `v_start` and `v_end`,
//!   **canonical**: the curve runs from the lower vertex id to the higher
//!   (`v_start ≤ v_end`, equal on a closed curve). Its tolerance `tol` is
//!   the greatest seam bound of its coedges, the ribbons' width.
//! - A coedge: a use of an edge by a face's loop, running the edge's way
//!   or, `reversed`, against it; `next` and `prev` round its loop; `pair`
//!   the edge's other coedge (`NONE` while it has one only). Its p-curve
//!   runs over the edge's `t_range` with the **edge's parameter**, whatever
//!   the coedge's direction, and its `seam_bound` bounds
//!   `|S_f(p(t)) − C(t)|` over it (the seam bound of
//!   [`certify`](crate::geom::certify)).
//! - A loop: its face, first coedge, the face's next loop, whether outer.
//! - A face: its surface, `flipped` (the outward normal against the
//!   surface's), its first loop.
//!
//! **Ribbons.** The counting surface of a face adds to it the ribbon of
//! each of its coedges, `(1 − λ)·S_f(p_c(t)) + λ·C_e(t)` with the same
//! `t` on both, at most the seam bound wide. For the ribbons to close the
//! face onto its edges' curves exactly, the joins must hold **bit for
//! bit** ([`check_ribbons`](Brep::check_ribbons)):
//! - (a) each edge's `t_range` ends are knots of its curve, and the
//!   curve's control points there are its vertices' `pos`; the edge is
//!   canonical;
//! - (b) each coedge ends at the vertex its next coedge starts at, and
//!   its p-curve's end control point is the next coedge's p-curve's start
//!   control point, in the loop's direction (with the edges' ends this
//!   makes neighbouring ribbons share their side at the corner);
//! - (e) each coedge's p-curve runs over its edge's `t_range`;
//! - (f) consecutive spans of a curve or p-curve share their joining
//!   control point: a chain holds it from its construction
//!   ([`crate::geom::curve`]), so it isn't tested again.
//!
//! Control points are stored affine with their weights, and a span passes
//! exactly through its end control points for any weights: so the
//! compared points are the curves' true points at the ends, and the stored
//! curve ends exactly at its vertices. They are compared bit for bit
//! (`+0` and `−0` differ).
//!
//! **Building.** Surfaces, curves, vertices, edges and faces are added
//! one by one, each refused if it names an entity not there; a loop is
//! added whole, with a p-curve per coedge: its seam bounds are computed
//! then, refined towards a target in one charged pass of the operation's
//! work over its coedges (running out names the coedge), and every coedge
//! is paired with its edge's other coedge. The
//! joins are not tested on the way in; [`check_ribbons`](Brep::check_ribbons)
//! tests them on the stored body.

use crate::geom::certify::{SeamError, seam_bound};
use crate::geom::curve::{Curve3D, PCurve, Span3};
use crate::geom::form::FormKey;
use crate::geom::surface::Surface;
use crate::infra::budget::{Halt, Pass, Work};
use crate::infra::error::KernelError;

use varde_decide::num::storage::{OpStart, OperandScan, Unrepresentable};
use varde_decide::records::near::OpBands;

pub use varde_decide::ids::{CoedgeId, CurveId, EdgeId, FaceId, LoopId, NONE, SurfId, VertId};

/// What a body is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BodyKind {
    /// A closed solid: every coedge has its pair.
    #[default]
    Solid,
    /// An open sheet: a coedge on its boundary has none (`pair = NONE`).
    /// Never a boolean operand.
    Sheet,
}

/// Why an entity can't be added.
#[derive(Clone, Debug, PartialEq)]
pub enum StoreError {
    /// An id names no entity of its kind.
    Id,
    /// The ids of a kind are used up.
    Full,
    /// A vertex position is not finite.
    NotFinite,
    /// An edge's vertices are not canonical (`v_start > v_end`).
    NotCanonical,
    /// An edge's `t_range` is not two increasing knots of its curve.
    TRange,
    /// A loop without coedges.
    EmptyLoop,
    /// The loop's coedge `index` can't be added.
    Coedge { index: usize, why: CoedgeError },
}

/// Why a coedge can't be added.
#[derive(Clone, Debug, PartialEq)]
pub enum CoedgeError {
    /// Its edge has two coedges already.
    Third,
    /// Its edge's coedge runs the same way.
    SameWay,
    /// Its p-curve doesn't run over the edge's `t_range` exactly.
    SameT,
    /// Its seam bound can't be given.
    Seam(SeamError),
    /// Its seam bound ran out of budget or reached a fixed limit
    /// ([`KernelError::OutOfBudget`]); the coedges before it in the loop
    /// had theirs.
    OutOfBudget(KernelError),
}

/// The first failed ribbon join (module docs), in the order (a) over the
/// edges, (b) over the coedges (the vertex before the p-curve), (e) over
/// the coedges, ids ascending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RibbonFault {
    /// (a): the edge's curve's control points at its `t_range` ends
    /// aren't its vertices' positions, or the edge isn't canonical.
    EdgeEnds(EdgeId),
    /// (b): the coedge doesn't end at the vertex its next starts at.
    Corner(CoedgeId),
    /// (b): the coedge's p-curve's end control point isn't its next's
    /// start control point.
    Join(CoedgeId),
    /// (e): the coedge's p-curve doesn't run over its edge's `t_range`.
    SameT(CoedgeId),
}

#[derive(Clone, Debug, Default)]
struct Verts {
    pos: Vec<[f64; 3]>,
    tol: Vec<f64>,
    coedge: Vec<CoedgeId>,
}

#[derive(Clone, Debug, Default)]
struct Edges {
    curve: Vec<CurveId>,
    t_range: Vec<[f64; 2]>,
    v_start: Vec<VertId>,
    v_end: Vec<VertId>,
    tol: Vec<f64>,
    coedge: Vec<CoedgeId>,
}

#[derive(Clone, Debug, Default)]
struct Coedges {
    edge: Vec<EdgeId>,
    reversed: Vec<bool>,
    face: Vec<FaceId>,
    lp: Vec<LoopId>,
    next: Vec<CoedgeId>,
    prev: Vec<CoedgeId>,
    pair: Vec<CoedgeId>,
    pcurve: Vec<PCurve>,
    seam_bound: Vec<f64>,
}

#[derive(Clone, Debug, Default)]
struct Loops {
    face: Vec<FaceId>,
    first_coedge: Vec<CoedgeId>,
    next_loop: Vec<LoopId>,
    is_outer: Vec<bool>,
}

#[derive(Clone, Debug, Default)]
struct Faces {
    surface: Vec<SurfId>,
    flipped: Vec<bool>,
    first_loop: Vec<LoopId>,
}

/// The minimal B-rep store (module docs).
#[derive(Clone, Debug, Default)]
pub struct Brep {
    kind: BodyKind,
    surfaces: Vec<Surface>,
    curves: Vec<Curve3D>,
    verts: Verts,
    edges: Edges,
    coedges: Coedges,
    loops: Loops,
    faces: Faces,
}

/// The next id of a kind with `len` entities: below `NONE`.
fn next_id(len: usize) -> Result<u32, StoreError> {
    u32::try_from(len)
        .ok()
        .filter(|&i| i != NONE)
        .ok_or(StoreError::Full)
}

fn same_bits<const N: usize>(a: &[f64; N], b: &[f64; N]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

impl Brep {
    /// An empty store of a solid.
    pub fn new() -> Brep {
        Brep::default()
    }

    /// An empty store of a body of kind `kind`.
    pub fn with_kind(kind: BodyKind) -> Brep {
        Brep {
            kind,
            ..Brep::default()
        }
    }

    /// The body's kind.
    pub fn kind(&self) -> BodyKind {
        self.kind
    }

    /// Adds a surface.
    pub fn add_surface(&mut self, s: Surface) -> Result<SurfId, StoreError> {
        let id = next_id(self.surfaces.len())?;
        self.surfaces.push(s);
        Ok(SurfId(id))
    }

    /// Adds a space curve.
    pub fn add_curve(&mut self, c: Curve3D) -> Result<CurveId, StoreError> {
        let id = next_id(self.curves.len())?;
        self.curves.push(c);
        Ok(CurveId(id))
    }

    /// Adds a vertex at `pos`, its tolerance 0 until edges reach it.
    pub fn add_vert(&mut self, pos: [f64; 3]) -> Result<VertId, StoreError> {
        if !pos.iter().all(|x| x.is_finite()) {
            return Err(StoreError::NotFinite);
        }
        let id = next_id(self.verts.pos.len())?;
        self.verts.pos.push(pos);
        self.verts.tol.push(0.0);
        self.verts.coedge.push(CoedgeId::NONE);
        Ok(VertId(id))
    }

    /// Adds an edge on `curve` over `t_range`, two knots of the curve
    /// with `t_range[0] < t_range[1]`, from `v_start` to `v_end`
    /// (canonical: `v_start ≤ v_end`). Its tolerance is 0 until coedges
    /// use it.
    pub fn add_edge(
        &mut self,
        curve: CurveId,
        t_range: [f64; 2],
        v_start: VertId,
        v_end: VertId,
    ) -> Result<EdgeId, StoreError> {
        let c = self.curves.get(curve.index()).ok_or(StoreError::Id)?;
        if v_start.index() >= self.verts.pos.len() || v_end.index() >= self.verts.pos.len() {
            return Err(StoreError::Id);
        }
        if v_start > v_end {
            return Err(StoreError::NotCanonical);
        }
        let knot = |t: f64| c.knots().iter().any(|k| k.to_bits() == t.to_bits());
        if !(t_range[0] < t_range[1] && knot(t_range[0]) && knot(t_range[1])) {
            return Err(StoreError::TRange);
        }
        let id = next_id(self.edges.curve.len())?;
        let e = &mut self.edges;
        e.curve.push(curve);
        e.t_range.push(t_range);
        e.v_start.push(v_start);
        e.v_end.push(v_end);
        e.tol.push(0.0);
        e.coedge.push(CoedgeId::NONE);
        Ok(EdgeId(id))
    }

    /// Adds a face on `surface`, without loops.
    pub fn add_face(&mut self, surface: SurfId, flipped: bool) -> Result<FaceId, StoreError> {
        if surface.index() >= self.surfaces.len() {
            return Err(StoreError::Id);
        }
        let id = next_id(self.faces.surface.len())?;
        self.faces.surface.push(surface);
        self.faces.flipped.push(flipped);
        self.faces.first_loop.push(LoopId::NONE);
        Ok(FaceId(id))
    }

    /// Adds a loop to `face`, after its other loops: a coedge per entry
    /// `(edge, reversed, p-curve)`, in the loop's order, each coedge's
    /// `next` the following entry's (the last's the first's). Each
    /// coedge's seam bound is computed on the face's surface, refined
    /// towards `target` ([`seam_bound`]), in one pass of `work` over the
    /// loop's coedges in order; its edge's tolerance, and its vertices',
    /// are raised to it; it is paired with the edge's coedge if there is
    /// one, which must run the other way. Nothing is added unless every
    /// coedge can be.
    #[allow(dead_code, reason = "moved ahead of the operations that build bodies")]
    pub(crate) fn add_loop(
        &mut self,
        face: FaceId,
        is_outer: bool,
        uses: Vec<(EdgeId, bool, PCurve)>,
        target: f64,
        work: &mut Work,
    ) -> Result<LoopId, StoreError> {
        let surface = *self.faces.surface.get(face.index()).ok_or(StoreError::Id)?;
        if uses.is_empty() {
            return Err(StoreError::EmptyLoop);
        }
        let lp = LoopId(next_id(self.loops.face.len())?);
        let first = self.coedges.edge.len();
        let n = uses.len();
        // The ids of the coedges, checked before any is added.
        next_id(first.checked_add(n - 1).ok_or(StoreError::Full)?)?;
        let mut curves = Vec::with_capacity(n);
        for (index, (e, reversed, p)) in uses.iter().enumerate() {
            let why = |why| StoreError::Coedge { index, why };
            let c = *self.edges.curve.get(e.index()).ok_or(StoreError::Id)?;
            // Earlier coedges of this loop count as the edge's too.
            let mut users = self.edge_coedges(*e);
            users.extend(
                uses[..index]
                    .iter()
                    .filter(|(o, _, _)| o == e)
                    .map(|(_, r, _)| *r),
            );
            match users[..] {
                [] => {}
                [r] if r != *reversed => {}
                [_] => return Err(why(CoedgeError::SameWay)),
                _ => return Err(why(CoedgeError::Third)),
            }
            let [t0, t1] = self.edges.t_range[e.index()];
            let k = p.knots();
            if k[0].to_bits() != t0.to_bits() || k[k.len() - 1].to_bits() != t1.to_bits() {
                return Err(why(CoedgeError::SameT));
            }
            curves.push(c);
        }
        let surf = &self.surfaces[surface.index()];
        let items: Vec<(&Curve3D, &PCurve)> = curves
            .iter()
            .zip(&uses)
            .map(|(c, (_, _, p))| (&self.curves[c.index()], p))
            .collect();
        let pass = Pass {
            stage: "seam",
            rung: 0,
        };
        let bounds = work
            .pass(pass, &items, |(c, p), ledger| {
                seam_bound(surf, c, p, target, ledger).map(|b| b.bound)
            })
            .map_err(|halt| match halt {
                Halt::Failed { item, error } => StoreError::Coedge {
                    index: item,
                    why: CoedgeError::Seam(error),
                },
                Halt::OutOfBudget(error) => StoreError::Coedge {
                    index: match &error {
                        KernelError::OutOfBudget { profile, .. } => profile.stopped.at.item,
                        _ => 0,
                    },
                    why: CoedgeError::OutOfBudget(error),
                },
            })?;
        let co = &mut self.coedges;
        for (index, ((e, reversed, p), b)) in uses.into_iter().zip(bounds).enumerate() {
            // Below `first + n`, whose ids were checked above.
            let id = CoedgeId((first + index) as u32);
            let next = CoedgeId((first + (index + 1) % n) as u32);
            let prev = CoedgeId((first + (index + n - 1) % n) as u32);
            let other = self.edges.coedge[e.index()];
            co.edge.push(e);
            co.reversed.push(reversed);
            co.face.push(face);
            co.lp.push(lp);
            co.next.push(next);
            co.prev.push(prev);
            co.pair.push(other);
            co.pcurve.push(p);
            co.seam_bound.push(b);
            if other == CoedgeId::NONE {
                self.edges.coedge[e.index()] = id;
            } else {
                co.pair[other.index()] = id;
            }
            let tol = &mut self.edges.tol[e.index()];
            if b > *tol {
                *tol = b;
            }
            let tol = *tol;
            let (vs, ve) = (self.edges.v_start[e.index()], self.edges.v_end[e.index()]);
            for v in [vs, ve] {
                let t = &mut self.verts.tol[v.index()];
                if tol > *t {
                    *t = tol;
                }
            }
            let start = if reversed { ve } else { vs };
            if self.verts.coedge[start.index()] == CoedgeId::NONE {
                self.verts.coedge[start.index()] = id;
            }
        }
        let l = &mut self.loops;
        l.face.push(face);
        l.first_coedge.push(CoedgeId(first as u32));
        l.next_loop.push(LoopId::NONE);
        l.is_outer.push(is_outer);
        // Appended after the face's last loop.
        match self.faces.first_loop[face.index()] {
            f if f == LoopId::NONE => self.faces.first_loop[face.index()] = lp,
            mut at => {
                while self.loops.next_loop[at.index()] != LoopId::NONE {
                    at = self.loops.next_loop[at.index()];
                }
                self.loops.next_loop[at.index()] = lp;
            }
        }
        Ok(lp)
    }

    /// The directions (`reversed`) of the edge's coedges so far.
    fn edge_coedges(&self, e: EdgeId) -> Vec<bool> {
        let first = self.edges.coedge[e.index()];
        if first == CoedgeId::NONE {
            return Vec::new();
        }
        let mut out = vec![self.coedges.reversed[first.index()]];
        let pair = self.coedges.pair[first.index()];
        if pair != CoedgeId::NONE {
            out.push(self.coedges.reversed[pair.index()]);
        }
        out
    }

    /// The first failed ribbon join (module docs), or none.
    pub fn check_ribbons(&self) -> Result<(), RibbonFault> {
        for e in 0..self.edges.curve.len() {
            let id = EdgeId(e as u32);
            if !self.edge_ends_hold(id) {
                return Err(RibbonFault::EdgeEnds(id));
            }
        }
        for c in 0..self.coedges.edge.len() {
            let id = CoedgeId(c as u32);
            let next = self.coedges.next[c];
            if self.coedge_vert(id, true) != self.coedge_vert(next, false) {
                return Err(RibbonFault::Corner(id));
            }
            let joined = match (self.coedge_uv(id, true), self.coedge_uv(next, false)) {
                (Some(a), Some(b)) => same_bits(&a, &b),
                _ => false,
            };
            if !joined {
                return Err(RibbonFault::Join(id));
            }
        }
        for c in 0..self.coedges.edge.len() {
            let id = CoedgeId(c as u32);
            let [t0, t1] = self.edges.t_range[self.coedges.edge[c].index()];
            let k = self.coedges.pcurve[c].knots();
            if k[0].to_bits() != t0.to_bits() || k[k.len() - 1].to_bits() != t1.to_bits() {
                return Err(RibbonFault::SameT(id));
            }
        }
        Ok(())
    }

    /// Whether (a) holds for `e`.
    fn edge_ends_hold(&self, e: EdgeId) -> bool {
        let (vs, ve) = (self.edges.v_start[e.index()], self.edges.v_end[e.index()]);
        if vs > ve {
            return false;
        }
        let c = &self.curves[self.edges.curve[e.index()].index()];
        let [t0, t1] = self.edges.t_range[e.index()];
        let at = |t: f64| {
            c.knots()
                .iter()
                .position(|k| k.to_bits() == t.to_bits())
                .and_then(|k| c.knot_point(k))
        };
        match (at(t0), at(t1)) {
            (Some(a), Some(b)) => {
                same_bits(&a, &self.verts.pos[vs.index()])
                    && same_bits(&b, &self.verts.pos[ve.index()])
            }
            _ => false,
        }
    }

    /// The vertex where coedge `c` ends (`end`) or starts, in its own
    /// direction; `None` for an id naming no coedge.
    fn coedge_vert(&self, c: CoedgeId, end: bool) -> Option<VertId> {
        let e = self.coedges.edge.get(c.index())?.index();
        let last = end != self.coedges.reversed[c.index()];
        Some(if last {
            self.edges.v_end[e]
        } else {
            self.edges.v_start[e]
        })
    }

    /// The UV point where coedge `c` ends (`end`) or starts, in its own
    /// direction: its p-curve's last or first control point (the true
    /// p-curve's end point), swapped when reversed.
    pub fn coedge_uv(&self, c: CoedgeId, end: bool) -> Option<[f64; 2]> {
        let p = self.coedges.pcurve.get(c.index())?;
        let last = end != self.coedges.reversed[c.index()];
        p.knot_point(if last { p.spans().len() } else { 0 })
    }

    /// The number of vertices; their ids are `0..n`.
    pub fn vert_count(&self) -> usize {
        self.verts.pos.len()
    }

    /// The number of edges; their ids are `0..n`.
    pub fn edge_count(&self) -> usize {
        self.edges.curve.len()
    }

    /// The number of coedges; their ids are `0..n`.
    pub fn coedge_count(&self) -> usize {
        self.coedges.edge.len()
    }

    /// The number of faces; their ids are `0..n`.
    pub fn face_count(&self) -> usize {
        self.faces.surface.len()
    }

    /// The number of loops; their ids are `0..n`.
    pub fn loop_count(&self) -> usize {
        self.loops.face.len()
    }

    /// A vertex's position.
    pub fn pos(&self, v: VertId) -> [f64; 3] {
        self.verts.pos[v.index()]
    }

    /// A vertex's tolerance.
    pub fn vert_tol(&self, v: VertId) -> f64 {
        self.verts.tol[v.index()]
    }

    /// An edge's curve.
    pub fn curve(&self, e: EdgeId) -> &Curve3D {
        &self.curves[self.edges.curve[e.index()].index()]
    }

    /// The curve object an edge lies on (edges may share one).
    pub fn curve_id(&self, e: EdgeId) -> CurveId {
        self.edges.curve[e.index()]
    }

    /// An edge's parameter range on its curve.
    pub fn t_range(&self, e: EdgeId) -> [f64; 2] {
        self.edges.t_range[e.index()]
    }

    /// An edge's vertices, `[v_start, v_end]`.
    pub fn ends(&self, e: EdgeId) -> [VertId; 2] {
        [self.edges.v_start[e.index()], self.edges.v_end[e.index()]]
    }

    /// The spans of an edge's curve inside its `t_range`, in the curve's
    /// (the edge's stored) direction: the first starts at `v_start`'s
    /// point, the last ends at `v_end`'s, where (a) holds
    /// ([`check_ribbons`](Self::check_ribbons)). Empty if a `t_range` end
    /// isn't a knot (which [`add_edge`](Self::add_edge) refuses).
    pub fn edge_spans(&self, e: EdgeId) -> &[Span3] {
        let c = self.curve(e);
        let [t0, t1] = self.t_range(e);
        let at = |t: f64| c.knots().iter().position(|k| k.to_bits() == t.to_bits());
        match (at(t0), at(t1)) {
            (Some(i), Some(j)) if i < j => &c.spans()[i..j],
            _ => &[],
        }
    }

    /// The largest coordinate magnitude of the stored geometry: vertex
    /// positions and the control points of the edges' curves (weights
    /// aside). Its operands' greatest is an operation's magnitude, which
    /// fixes its rounding bound
    /// ([`OpBands`]).
    pub fn max_coord(&self) -> f64 {
        let verts = self.verts.pos.iter().flatten();
        let curves = self
            .curves
            .iter()
            .flat_map(|c| c.spans().iter().flatten().flat_map(|p| &p[..3]));
        verts
            .chain(curves)
            .map(|x| x.abs())
            .fold(0.0, |m, x| if x > m { x } else { m })
    }

    /// The operation-start scan of the body's stored values, in a fixed
    /// order ([`OperandScan`]): vertex positions and the control points of
    /// the edges' curves are bounded positions (at most `MAX_COORD`); the
    /// forms' key points (a lattice's origin, a cylinder's foot point, a
    /// sphere's centre) are where they lie inside the box of those; every
    /// other value (tolerances, curve weights and knots, p-curves, seam
    /// bounds, the forms' directions and radii, the tiles' nets) need
    /// only be finite. The first failing value is the error.
    pub fn scan_storable(&self, scan: &mut OperandScan) -> Result<(), Unrepresentable> {
        let mut body = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        let mut grow = |p: &[f64]| {
            for k in 0..3 {
                if p[k] < body[0][k] {
                    body[0][k] = p[k];
                }
                if p[k] > body[1][k] {
                    body[1][k] = p[k];
                }
            }
        };
        for (p, tol) in self.verts.pos.iter().zip(&self.verts.tol) {
            scan.bounded(p)?;
            scan.finite(&[*tol])?;
            grow(p);
        }
        for c in &self.curves {
            for p in c.spans().iter().flatten() {
                scan.bounded(&p[..3])?;
                scan.finite(&p[3..])?;
                grow(&p[..3]);
            }
            scan.finite(c.knots())?;
        }
        for (t, tol) in self.edges.t_range.iter().zip(&self.edges.tol) {
            scan.finite(t)?;
            scan.finite(&[*tol])?;
        }
        for (p, b) in self.coedges.pcurve.iter().zip(&self.coedges.seam_bound) {
            for q in p.spans().iter().flatten() {
                scan.finite(q)?;
            }
            scan.finite(p.knots())?;
            scan.finite(&[*b])?;
        }
        for s in &self.surfaces {
            let l = s.lattice();
            scan.key_point(&l.origin(), &body)?;
            scan.finite(l.axes().as_flattened())?;
            match l.key() {
                FormKey::Plane(k) => {
                    scan.finite(&k.normal())?;
                    scan.finite(&[k.offset()])?;
                }
                FormKey::Cylinder(k) => {
                    scan.key_point(&k.foot(), &body)?;
                    scan.finite(&k.axis())?;
                    scan.finite(&[k.radius()])?;
                }
                FormKey::Sphere(k) => {
                    scan.key_point(&k.centre(), &body)?;
                    scan.finite(&k.axis())?;
                    scan.finite(&[k.radius()])?;
                }
            }
            for t in s.tiles() {
                scan.finite(t.net().as_flattened())?;
            }
        }
        Ok(())
    }

    /// An edge's near radius `r_g = 3·(tol + ρ_op)` under the operation's
    /// bands; `None` if the bands refuse its tolerance.
    pub fn edge_near_radius(&self, e: EdgeId, bands: &OpBands) -> Option<f64> {
        bands.near_radius(self.edge_tol(e))
    }

    /// Every vertex's near radius, by id: the greatest of its edges'
    /// ([`edge_near_radius`](Self::edge_near_radius)), 0 at a vertex
    /// without edges; `None` if the bands refuse an edge's tolerance.
    pub fn vert_near_radii(&self, bands: &OpBands) -> Option<Vec<f64>> {
        let mut out = vec![0.0; self.verts.pos.len()];
        for e in 0..self.edges.curve.len() {
            let r = self.edge_near_radius(EdgeId(e as u32), bands)?;
            for v in [self.edges.v_start[e], self.edges.v_end[e]] {
                let m = &mut out[v.index()];
                if r > *m {
                    *m = r;
                }
            }
        }
        Some(out)
    }

    /// An edge's tolerance: the greatest seam bound of its coedges.
    pub fn edge_tol(&self, e: EdgeId) -> f64 {
        self.edges.tol[e.index()]
    }

    /// One of an edge's coedges, `NONE` while it has none.
    pub fn edge_coedge(&self, e: EdgeId) -> CoedgeId {
        self.edges.coedge[e.index()]
    }

    /// A coedge's edge.
    pub fn edge(&self, c: CoedgeId) -> EdgeId {
        self.coedges.edge[c.index()]
    }

    /// Whether a coedge runs against its edge.
    pub fn reversed(&self, c: CoedgeId) -> bool {
        self.coedges.reversed[c.index()]
    }

    /// A coedge's face.
    pub fn face(&self, c: CoedgeId) -> FaceId {
        self.coedges.face[c.index()]
    }

    /// The next coedge round a coedge's loop.
    pub fn next(&self, c: CoedgeId) -> CoedgeId {
        self.coedges.next[c.index()]
    }

    /// The previous coedge round a coedge's loop.
    pub fn prev(&self, c: CoedgeId) -> CoedgeId {
        self.coedges.prev[c.index()]
    }

    /// The other coedge of a coedge's edge, `NONE` if none.
    pub fn pair(&self, c: CoedgeId) -> CoedgeId {
        self.coedges.pair[c.index()]
    }

    /// A coedge's p-curve, over its edge's parameter.
    pub fn pcurve(&self, c: CoedgeId) -> &PCurve {
        &self.coedges.pcurve[c.index()]
    }

    /// A coedge's seam bound: its ribbon's width.
    pub fn seam_bound(&self, c: CoedgeId) -> f64 {
        self.coedges.seam_bound[c.index()]
    }

    /// A face's surface.
    pub fn surface(&self, f: FaceId) -> &Surface {
        &self.surfaces[self.faces.surface[f.index()].index()]
    }

    /// Whether a face's outward normal is against its surface's.
    pub fn flipped(&self, f: FaceId) -> bool {
        self.faces.flipped[f.index()]
    }

    /// A face's loops, in the order added.
    pub fn loops(&self, f: FaceId) -> impl Iterator<Item = LoopId> + '_ {
        let first = self.faces.first_loop[f.index()];
        std::iter::successors((first != LoopId::NONE).then_some(first), |l| {
            let n = self.loops.next_loop[l.index()];
            (n != LoopId::NONE).then_some(n)
        })
    }

    /// Whether a loop is its face's outer loop.
    pub fn is_outer(&self, l: LoopId) -> bool {
        self.loops.is_outer[l.index()]
    }

    /// A loop's coedges, in order from its first.
    pub fn loop_coedges(&self, l: LoopId) -> impl Iterator<Item = CoedgeId> + '_ {
        let first = self.loops.first_coedge[l.index()];
        std::iter::successors(Some(first), move |&c| {
            let n = self.coedges.next[c.index()];
            (n != first).then_some(n)
        })
    }
}

/// An operation's start on its operands at tolerance `eps`: each
/// operand scanned in turn ([`Brep::scan_storable`]), then the tolerance
/// checked and `ρ_op` fixed at the operands' largest bounded magnitude
/// ([`OperandScan::start`]).
pub fn op_start(operands: &[&Brep], eps: f64) -> Result<OpStart, Unrepresentable> {
    let mut scan = OperandScan::new();
    for b in operands {
        b.scan_storable(&mut scan)?;
    }
    scan.start(eps)
}

pub mod planar;

#[cfg(test)]
pub(crate) mod sheets;
#[cfg(test)]
mod tests;
