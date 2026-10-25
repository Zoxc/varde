//! Building and editing a [`Brep`]: the constructions and the operations'
//! assembly add a body's entities directly, row by row, and an operation
//! that edits a body removes and adds whole loops and faces; there are no
//! Euler operators, and nothing here decides geometry.
//!
//! **Adding.** A [`Builder`] holds a body being built (an empty one, or a
//! body given to [`Builder::edit`]). Geometry, vertices, edges and faces
//! are added one by one, each refused if it names an entity that isn't
//! there or was removed. A loop is added whole ([`Builder::add_loop`]), a
//! coedge per [`Use`] (edge, direction, p-curve, seam bound), in the
//! loop's order:
//! - each use must start at the vertex the previous one ends at (the
//!   last's end the first's start), and its p-curve run over its edge's
//!   `t_range` exactly (its first and last knots the range's ends, bit for
//!   bit);
//! - its coedge is paired with the edge's other coedge, which must run
//!   the other way; an edge takes at most two coedges, a
//!   [`Singular`](EdgeKind::Singular) one one;
//! - the edge's tolerance is raised to the coedge's seam bound, its
//!   vertices' tolerances to the edge's, and the body's `max_tol` to it
//!   ([`Tolerances::raise`](super::brep::Tolerances::raise));
//! - the loop goes after the face's other loops, and a vertex with no
//!   coedge leaving it takes the one starting there.
//!
//! Nothing of a loop is added unless all of it can be. Seam bounds are
//! the caller's: [`Builder::seam_bounds`] certifies a loop's in one
//! charged pass. Shells join faces' sides ([`Builder::add_shell`]) and
//! regions shells ([`Builder::add_region`]); a side or a shell takes one
//! of each. A solid's shells and regions are derived data:
//! [`Builder::fill_regions`] sets them from its faces and edges.
//!
//! **Removing.** [`Builder::remove_loop`] and [`Builder::remove_face`]
//! remove what they own (a loop's coedges; a face's loops and its sides in
//! shells) and unpair the coedges left across their edges.
//! [`Builder::remove_edge`] and [`Builder::remove_vert`] remove the one
//! row; [`Builder::clear_shells`] removes every shell and region, which
//! an operation derives again. A removed entity's id names nothing from
//! then on.
//!
//! **Geometry collection.** [`Builder::collect_geometry`] removes every
//! surface, curve and p-curve no remaining entity refers to. Its roots: a
//! face's surface, an edge's curve and the surfaces of its origin's
//! clusters (kept so the cluster can run again), a coedge's p-curve, and
//! what a kept surface's [`Fit`] intent and exact rows name (the surfaces
//! and curves an intent is built on, which a refit or a measure reads).
//!
//! **Finishing.** [`Builder::finish`] compacts the columns: the remaining
//! rows keep their order, so their ids only move down, and an edge stays
//! canonical. Every id stored is mapped to its row's new id, and the map
//! returned ([`Remap`]) for the caller's own tables (history, names). An
//! id naming a removed row is refused there ([`Dangling`]): an editor
//! removes the references to what it removes, or the rows holding them. A
//! vertex or edge whose stored coedge was removed takes the first
//! remaining coedge leaving it (using it), or none. In debug builds the
//! finished store is checked for dangling ids ([`Brep::dangling`]).

use super::brep::{
    AttrSetId, BodyKind, BodyStatus, Brep, CoedgeId, CoedgeRow, Contact, Continuity, CurveId,
    EdgeId, EdgeKind, EdgeRow, EdgeTag, ExactRows, FaceId, FaceKey, FaceRow, Fit, LoopId, LoopRow,
    Member, NONE, Origin, PCurveId, RegionId, RegionKind, RegionRow, ShellId, ShellKind, ShellRow,
    Side, StoreError, SurfId, SurfaceRow, VertId, VertKind, VertRow, VertTag,
};
use super::regions::{RegionError, derive};
use crate::geom::certify::{SeamError, seam_bound};
use crate::geom::curve::Curve3D;
use crate::geom::fit_types::IntentId;
use crate::geom::pcurve::PCurve;
use crate::geom::surface::Surface;
use crate::infra::budget::{Halt, Pass, Work};
use crate::infra::error::KernelError;
use crate::infra::ops::Status;

/// A row of the store, by its kind and id (a contact by its place).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Row {
    /// A surface.
    Surface(SurfId),
    /// A space curve.
    Curve(CurveId),
    /// A p-curve.
    PCurve(PCurveId),
    /// A vertex.
    Vert(VertId),
    /// An edge.
    Edge(EdgeId),
    /// A coedge.
    Coedge(CoedgeId),
    /// A loop.
    Loop(LoopId),
    /// A face.
    Face(FaceId),
    /// A shell.
    Shell(ShellId),
    /// A region.
    Region(RegionId),
    /// A contact record, by its place in the body's list.
    Contact(usize),
}

/// A stored id that names no row: past its kind's rows, removed, or
/// `NONE` where the field must name one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dangling {
    /// The row holding it.
    pub row: Row,
    /// The field holding it.
    pub field: &'static str,
}

/// Why a coedge of a loop can't be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseError {
    /// Its edge or p-curve isn't there.
    Id,
    /// It doesn't start at the vertex the previous use ends at.
    Chain,
    /// Its p-curve doesn't run over its edge's `t_range` exactly.
    SameT,
    /// Its seam bound is negative or not finite.
    SeamBound,
    /// Its edge has a coedge running the same way.
    SameWay,
    /// Its edge has two coedges already, or a singular edge one.
    Third,
}

/// Why the builder can't do what it was asked.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildError {
    /// A row can't be added ([`StoreError`]); an id naming a removed row
    /// is [`StoreError::Id`].
    Store(StoreError),
    /// A loop without coedges.
    EmptyLoop,
    /// Use `index` of a loop can't be added.
    Use {
        /// Its place in the loop.
        index: usize,
        /// Why not.
        why: UseError,
    },
    /// A face's side is in a shell already, or listed twice.
    Side(FaceId, Side),
    /// A shell is in a region already, or listed twice.
    Shell(ShellId),
    /// Use `index`'s seam bound can't be given.
    Seam {
        /// Its place in the loop.
        index: usize,
        /// Why not.
        why: SeamError,
    },
    /// The seam bounds ran out of budget or reached a fixed limit
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
    /// A stored id names a removed row ([`Builder::finish`]).
    Dangling(Dangling),
    /// The body's regions can't be derived ([`Builder::fill_regions`]).
    Regions(RegionError),
}

impl From<StoreError> for BuildError {
    fn from(e: StoreError) -> BuildError {
        BuildError::Store(e)
    }
}

/// A use of an edge by a loop: one coedge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Use {
    /// The edge.
    pub edge: EdgeId,
    /// Whether the coedge runs against the edge.
    pub reversed: bool,
    /// Its p-curve, over the edge's parameter.
    pub pcurve: PCurveId,
    /// Its seam bound.
    pub seam_bound: f64,
}

/// The ids of a body's rows before [`Builder::finish`] mapped to theirs
/// after; `NONE` for a removed row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remap {
    surfaces: Vec<u32>,
    curves: Vec<u32>,
    pcurves: Vec<u32>,
    verts: Vec<u32>,
    edges: Vec<u32>,
    coedges: Vec<u32>,
    loops: Vec<u32>,
    faces: Vec<u32>,
    shells: Vec<u32>,
    regions: Vec<u32>,
}

/// The new id of `id` in `map`: `NONE` for `NONE`, a removed row or one
/// past the map.
fn look(map: &[u32], id: u32) -> u32 {
    map.get(id as usize).copied().unwrap_or(NONE)
}

macro_rules! remap_getters {
    ($($(#[$doc:meta])* $name:ident: $id:ident;)*) => {
        impl Remap {
            $(
                $(#[$doc])*
                pub fn $name(&self, id: $id) -> $id {
                    $id(look(&self.$name, id.0))
                }
            )*
        }
    };
}

remap_getters! {
    /// A surface's new id.
    surfaces: SurfId;
    /// A curve's new id.
    curves: CurveId;
    /// A p-curve's new id.
    pcurves: PCurveId;
    /// A vertex's new id.
    verts: VertId;
    /// An edge's new id.
    edges: EdgeId;
    /// A coedge's new id.
    coedges: CoedgeId;
    /// A loop's new id.
    loops: LoopId;
    /// A face's new id.
    faces: FaceId;
    /// A shell's new id.
    shells: ShellId;
    /// A region's new id.
    regions: RegionId;
}

/// The removed rows, per kind; a row past a kind's marks is there.
#[derive(Clone, Debug, Default)]
struct Removed {
    surfaces: Vec<bool>,
    curves: Vec<bool>,
    pcurves: Vec<bool>,
    verts: Vec<bool>,
    edges: Vec<bool>,
    coedges: Vec<bool>,
    loops: Vec<bool>,
    faces: Vec<bool>,
    shells: Vec<bool>,
    regions: Vec<bool>,
}

/// Whether row `i` is marked in `marks`.
fn marked(marks: &[bool], i: u32) -> bool {
    marks.get(i as usize).copied().unwrap_or(false)
}

/// Marks row `i` of a kind with `len` rows.
fn mark(marks: &mut Vec<bool>, len: usize, i: u32) {
    if marks.len() < len {
        marks.resize(len, false);
    }
    marks[i as usize] = true;
}

/// The map of a kind's `len` rows to their ids once the marked ones are
/// gone.
fn compaction(marks: &[bool], len: usize) -> Vec<u32> {
    let mut next = 0u32;
    (0..len)
        .map(|i| {
            if marks.get(i).copied().unwrap_or(false) {
                NONE
            } else {
                // Below `len`, itself a count of ids below `NONE`.
                let id = next;
                next += 1;
                id
            }
        })
        .collect()
}

/// Keeps the entries of `v` whose rows `map` keeps, in order.
fn compact<T>(v: &mut Vec<T>, map: &[u32]) {
    let mut i = 0;
    v.retain(|_| {
        let keep = map[i] != NONE;
        i += 1;
        keep
    });
}

/// A body being built or edited (module docs).
#[derive(Clone, Debug)]
pub struct Builder {
    brep: Brep,
    removed: Removed,
}

impl Builder {
    /// An empty body of kind `kind`, made by an operation of status
    /// `status` ([`Brep::new`]).
    pub fn new(kind: BodyKind, status: Status) -> Builder {
        Builder::edit(Brep::new(kind, status))
    }

    /// The body `brep`, to edit.
    pub fn edit(brep: Brep) -> Builder {
        Builder {
            brep,
            removed: Removed::default(),
        }
    }

    /// The body as built so far; removed rows are still in it.
    pub fn brep(&self) -> &Brep {
        &self.brep
    }

    /// The status of the operation making the body, to report into.
    pub fn status_mut(&mut self) -> &mut BodyStatus {
        self.brep.status_mut()
    }

    // Whether a row is there: added and not removed.

    fn has_surface(&self, s: SurfId) -> bool {
        s.index() < self.brep.surface_count() && !marked(&self.removed.surfaces, s.0)
    }

    fn has_curve(&self, c: CurveId) -> bool {
        c.index() < self.brep.curve_count() && !marked(&self.removed.curves, c.0)
    }

    fn has_pcurve(&self, p: PCurveId) -> bool {
        p.index() < self.brep.pcurve_count() && !marked(&self.removed.pcurves, p.0)
    }

    fn has_vert(&self, v: VertId) -> bool {
        v.index() < self.brep.vert_count() && !marked(&self.removed.verts, v.0)
    }

    fn has_edge(&self, e: EdgeId) -> bool {
        e.index() < self.brep.edge_count() && !marked(&self.removed.edges, e.0)
    }

    fn has_coedge(&self, c: CoedgeId) -> bool {
        c.index() < self.brep.coedge_count() && !marked(&self.removed.coedges, c.0)
    }

    fn has_loop(&self, l: LoopId) -> bool {
        l.index() < self.brep.loop_count() && !marked(&self.removed.loops, l.0)
    }

    fn has_face(&self, f: FaceId) -> bool {
        f.index() < self.brep.face_count() && !marked(&self.removed.faces, f.0)
    }

    fn has_shell(&self, s: ShellId) -> bool {
        s.index() < self.brep.shell_count() && !marked(&self.removed.shells, s.0)
    }

    fn has_region(&self, r: RegionId) -> bool {
        r.index() < self.brep.region_count() && !marked(&self.removed.regions, r.0)
    }

    // Geometry.

    /// Adds a surface, with its fit (`None` for an exact form) and exact
    /// rows.
    pub fn add_surface(
        &mut self,
        surface: Surface,
        fit: Option<Fit>,
        exact_rows: ExactRows,
    ) -> Result<SurfId, BuildError> {
        Ok(self.brep.push_surface(SurfaceRow {
            surface,
            fit,
            exact_rows,
        })?)
    }

    /// Adds a space curve.
    pub fn add_curve(&mut self, c: Curve3D) -> Result<CurveId, BuildError> {
        Ok(self.brep.push_curve(c)?)
    }

    /// Adds a p-curve.
    pub fn add_pcurve(&mut self, p: PCurve) -> Result<PCurveId, BuildError> {
        Ok(self.brep.push_pcurve(p)?)
    }

    // Topology.

    /// Adds a regular vertex at `pos`, its tolerance 0 until edges reach
    /// it.
    pub fn add_vert(&mut self, pos: [f64; 3]) -> Result<VertId, BuildError> {
        self.push_vert(pos, VertKind::Regular, None)
    }

    /// Adds a pole or apex vertex at `pos`, tagged `tag`.
    pub fn add_pole(&mut self, pos: [f64; 3], tag: VertTag) -> Result<VertId, BuildError> {
        self.push_vert(pos, VertKind::Pole, Some(tag))
    }

    fn push_vert(
        &mut self,
        pos: [f64; 3],
        kind: VertKind,
        tag: Option<VertTag>,
    ) -> Result<VertId, BuildError> {
        Ok(self.brep.push_vert(VertRow {
            pos,
            tol: 0.0,
            kind,
            coedge: CoedgeId::NONE,
            tag,
            attrs: AttrSetId::NONE,
        })?)
    }

    /// Adds an edge on `curve` over `t_range` (two increasing knots of
    /// the curve) from `ends[0]` to `ends[1]`, canonical (`ends[0] ≤
    /// ends[1]`): a regular `Exact` edge, a mixed crease, tolerance and
    /// deviation 0, its join not certified embedded, no tag, until set.
    pub fn add_edge(
        &mut self,
        curve: CurveId,
        t_range: [f64; 2],
        ends: [VertId; 2],
    ) -> Result<EdgeId, BuildError> {
        if !self.has_curve(curve) || !ends.iter().all(|&v| self.has_vert(v)) {
            return Err(StoreError::Id.into());
        }
        Ok(self.brep.push_edge(EdgeRow {
            curve,
            t_range,
            v_start: ends[0],
            v_end: ends[1],
            kind: EdgeKind::Regular,
            cont: Continuity::default(),
            origin: Origin::default(),
            tol: 0.0,
            dev: 0.0,
            embedded_join: false,
            coedge: CoedgeId::NONE,
            tag: None,
            attrs: AttrSetId::NONE,
        })?)
    }

    /// Adds a face on `surface`, `flipped` if its outward normal is
    /// against the surface's, keyed `key`; without loops or shells.
    pub fn add_face(
        &mut self,
        surface: SurfId,
        flipped: bool,
        key: FaceKey,
    ) -> Result<FaceId, BuildError> {
        if !self.has_surface(surface) {
            return Err(StoreError::Id.into());
        }
        Ok(self.brep.push_face(FaceRow {
            surface,
            flipped,
            shells: [ShellId::NONE; 2],
            first_loop: LoopId::NONE,
            key,
            attrs: AttrSetId::NONE,
        })?)
    }

    /// The seam bounds of the coedges `uses` (edge, p-curve) would add to
    /// a face on `surface`, each refined towards `target` ([`seam_bound`]),
    /// in one pass of `work` over them in order; or the first that can't
    /// be given, or the budget running out.
    pub fn seam_bounds(
        &self,
        surface: SurfId,
        uses: &[(EdgeId, PCurveId)],
        target: f64,
        work: &mut Work,
    ) -> Result<Vec<f64>, BuildError> {
        if !self.has_surface(surface) {
            return Err(StoreError::Id.into());
        }
        let b = &self.brep;
        let mut items = Vec::with_capacity(uses.len());
        for (index, &(e, p)) in uses.iter().enumerate() {
            if !self.has_edge(e) || !self.has_pcurve(p) {
                return Err(BuildError::Use {
                    index,
                    why: UseError::Id,
                });
            }
            items.push((b.edge_curve(e), b.pcurve(p)));
        }
        let surf = b.surface(surface);
        let pass = Pass {
            stage: "seam",
            rung: 0,
        };
        work.pass(pass, &items, |(c, p), ledger| {
            seam_bound(surf, c, p, target, ledger).map(|s| s.bound)
        })
        .map_err(|halt| match halt {
            Halt::Failed { item, error } => BuildError::Seam {
                index: item,
                why: error,
            },
            Halt::OutOfBudget(e) => BuildError::OutOfBudget(e),
        })
    }

    /// Adds a loop to `face`, after its other loops: a coedge per use, in
    /// the loop's order (module docs, "Adding"). Nothing is added unless
    /// every coedge can be.
    pub fn add_loop(
        &mut self,
        face: FaceId,
        is_outer: bool,
        uses: &[Use],
    ) -> Result<LoopId, BuildError> {
        if !self.has_face(face) {
            return Err(StoreError::Id.into());
        }
        let n = uses.len();
        if n == 0 {
            return Err(BuildError::EmptyLoop);
        }
        let b = &self.brep;
        let start = |u: &Use| b.coedge_vert_of(u.edge, u.reversed, false);
        let end = |u: &Use| b.coedge_vert_of(u.edge, u.reversed, true);
        for (index, u) in uses.iter().enumerate() {
            let why = |why| BuildError::Use { index, why };
            if !self.has_edge(u.edge) || !self.has_pcurve(u.pcurve) {
                return Err(why(UseError::Id));
            }
            if !(u.seam_bound >= 0.0 && u.seam_bound.is_finite()) {
                return Err(why(UseError::SeamBound));
            }
            let [t0, t1] = b.t_range(u.edge);
            let k = b.pcurve(u.pcurve).knots();
            if k[0].to_bits() != t0.to_bits() || k[k.len() - 1].to_bits() != t1.to_bits() {
                return Err(why(UseError::SameT));
            }
            // The edge's coedges so far, this loop's earlier ones too.
            let mut ways: Vec<bool> = self
                .edge_coedges(u.edge)
                .map(|c| b.reversed(c))
                .chain(
                    uses[..index]
                        .iter()
                        .filter(|o| o.edge == u.edge)
                        .map(|o| o.reversed),
                )
                .collect();
            ways.push(u.reversed);
            let most = if b.edge_kind(u.edge) == EdgeKind::Singular {
                1
            } else {
                2
            };
            match ways[..] {
                _ if ways.len() > most => return Err(why(UseError::Third)),
                [a, z] if a == z => return Err(why(UseError::SameWay)),
                _ => {}
            }
        }
        for index in 0..n {
            let next = &uses[(index + 1) % n];
            if end(&uses[index]) != start(next) {
                return Err(BuildError::Use {
                    index: (index + 1) % n,
                    why: UseError::Chain,
                });
            }
        }
        // The coedges' ids, below `NONE`: checked before any is added.
        let first = self.brep.coedge_count();
        let last = first.checked_add(n - 1).and_then(|l| u32::try_from(l).ok());
        if last.is_none_or(|l| l == NONE) {
            return Err(StoreError::Full.into());
        }
        let lp = self.brep.push_loop(LoopRow {
            face,
            first_coedge: CoedgeId::NONE,
            next_loop: LoopId::NONE,
            is_outer,
        })?;
        let id = |k: usize| CoedgeId((first + k % n) as u32);
        for (k, u) in uses.iter().enumerate() {
            let pair = self.edge_coedges(u.edge).next().unwrap_or(CoedgeId::NONE);
            let c = self.brep.push_coedge(CoedgeRow {
                edge: u.edge,
                reversed: u.reversed,
                face,
                lp,
                next: id(k + 1),
                prev: id(k + n - 1),
                pair,
                pcurve: u.pcurve,
                seam_bound: u.seam_bound,
            })?;
            let b = &mut self.brep;
            if pair.is_none() {
                b.edges.coedge[u.edge.index()] = c;
            } else {
                b.coedges.pair[pair.index()] = c;
            }
            let tol = &mut b.edges.tol[u.edge.index()];
            if u.seam_bound > *tol {
                *tol = u.seam_bound;
            }
            let tol = *tol;
            for v in b.ends(u.edge) {
                let t = &mut b.verts.tol[v.index()];
                if tol > *t {
                    *t = tol;
                }
            }
            b.eps_mut().raise(tol);
            let v = b.coedge_vert(c, false);
            let held = b.verts.coedge[v.index()];
            if !self.has_coedge(held) {
                self.brep.verts.coedge[v.index()] = c;
            }
        }
        let b = &mut self.brep;
        b.loops.first_coedge[lp.index()] = id(0);
        match b.faces.first_loop[face.index()] {
            f if f.is_none() => b.faces.first_loop[face.index()] = lp,
            mut at => {
                while !b.loops.next_loop[at.index()].is_none() {
                    at = b.loops.next_loop[at.index()];
                }
                b.loops.next_loop[at.index()] = lp;
            }
        }
        Ok(lp)
    }

    /// The edge's remaining coedges: the one it holds, then its pair.
    fn edge_coedges(&self, e: EdgeId) -> impl Iterator<Item = CoedgeId> + '_ {
        let held = self.brep.edge_coedge(e);
        let first = self.has_coedge(held).then_some(held);
        let pair = first
            .map(|c| self.brep.pair(c))
            .filter(|&p| self.has_coedge(p));
        first.into_iter().chain(pair)
    }

    /// Adds a shell of kind `kind` joining the faces' sides `sides`, in no
    /// region yet. Each side must be in no shell yet.
    pub fn add_shell(
        &mut self,
        kind: ShellKind,
        sides: Vec<(FaceId, Side)>,
    ) -> Result<ShellId, BuildError> {
        for (k, &(f, side)) in sides.iter().enumerate() {
            if !self.has_face(f) {
                return Err(StoreError::Id.into());
            }
            if self.has_shell(self.brep.face_shell(f, side)) || sides[..k].contains(&(f, side)) {
                return Err(BuildError::Side(f, side));
            }
        }
        let s = self.brep.push_shell(ShellRow {
            sides,
            region: RegionId::NONE,
            kind,
        })?;
        let b = &mut self.brep;
        for &(f, side) in &b.shells.sides[s.index()] {
            b.faces.shells[f.index()][side.index()] = s;
        }
        Ok(s)
    }

    /// Adds a region of kind `kind`: its outer shell (`NONE` for the
    /// unbounded region), its island shells, each in no region yet, and
    /// the region its empty islands lie in (`NONE` if none).
    pub fn add_region(
        &mut self,
        kind: RegionKind,
        outer: ShellId,
        islands: Vec<ShellId>,
        container: RegionId,
    ) -> Result<RegionId, BuildError> {
        if !(container.is_none() || self.has_region(container)) {
            return Err(StoreError::Id.into());
        }
        let shells = std::iter::once(outer)
            .filter(|s| !s.is_none())
            .chain(islands.iter().copied());
        for (k, s) in shells.clone().enumerate() {
            if !self.has_shell(s) {
                return Err(StoreError::Id.into());
            }
            if self.has_region(self.brep.shell_region(s)) || shells.clone().take(k).any(|o| o == s)
            {
                return Err(BuildError::Shell(s));
            }
        }
        let shells: Vec<ShellId> = shells.collect();
        let r = self.brep.push_region(RegionRow {
            kind,
            outer,
            islands,
            container,
        })?;
        for s in shells {
            self.brep.shells.region[s.index()] = r;
        }
        Ok(r)
    }

    /// Adds a contact record. Its members are checked when the body is
    /// finished.
    pub fn add_contact(&mut self, c: Contact) {
        self.brep.contacts.push(c);
    }

    /// Keeps the contact records `keep` holds to, in order.
    pub fn retain_contacts(&mut self, keep: impl FnMut(&Contact) -> bool) {
        self.brep.contacts.retain(keep);
    }

    // An edge's facts. The setters take an edge that is there.

    /// Sets an edge's kind.
    pub fn set_edge_kind(&mut self, e: EdgeId, kind: EdgeKind) {
        debug_assert!(self.has_edge(e), "edge {e:?} is there");
        self.brep.edges.kind[e.index()] = kind;
    }

    /// Sets an edge's tag.
    pub fn set_edge_tag(&mut self, e: EdgeId, tag: Option<EdgeTag>) {
        debug_assert!(self.has_edge(e), "edge {e:?} is there");
        self.brep.edges.tag[e.index()] = tag;
    }

    /// Sets an edge's continuity.
    pub fn set_cont(&mut self, e: EdgeId, cont: Continuity) {
        debug_assert!(self.has_edge(e), "edge {e:?} is there");
        self.brep.edges.cont[e.index()] = cont;
    }

    /// Sets an edge's origin.
    pub fn set_origin(&mut self, e: EdgeId, origin: Origin) {
        debug_assert!(self.has_edge(e), "edge {e:?} is there");
        self.brep.edges.origin[e.index()] = origin;
    }

    /// Sets an edge's deviation from the true curve it stands for.
    pub fn set_dev(&mut self, e: EdgeId, dev: f64) {
        debug_assert!(self.has_edge(e), "edge {e:?} is there");
        self.brep.edges.dev[e.index()] = dev;
    }

    /// Sets whether an edge's join zone is certified embedded.
    pub fn set_embedded_join(&mut self, e: EdgeId, embedded: bool) {
        debug_assert!(self.has_edge(e), "edge {e:?} is there");
        self.brep.edges.embedded_join[e.index()] = embedded;
    }

    // Removing.

    /// Removes a loop and its coedges: unlinked from its face's loops,
    /// each coedge's pair left without one and holding its edge.
    pub fn remove_loop(&mut self, l: LoopId) -> Result<(), BuildError> {
        if !self.has_loop(l) {
            return Err(StoreError::Id.into());
        }
        let coedges: Vec<CoedgeId> = self.brep.loop_coedges(l).collect();
        let n = self.brep.coedge_count();
        for &c in &coedges {
            mark(&mut self.removed.coedges, n, c.0);
        }
        for &c in &coedges {
            let e = self.brep.edge(c);
            let pair = self.brep.pair(c);
            let held = self.brep.edge_coedge(e);
            if self.has_coedge(pair) {
                self.brep.coedges.pair[pair.index()] = CoedgeId::NONE;
                self.brep.edges.coedge[e.index()] = pair;
            } else if !self.has_coedge(held) {
                self.brep.edges.coedge[e.index()] = CoedgeId::NONE;
            }
        }
        let face = self.brep.loop_face(l);
        let next = self.brep.next_loop(l);
        let b = &mut self.brep;
        if b.faces.first_loop[face.index()] == l {
            b.faces.first_loop[face.index()] = next;
        } else {
            let mut at = b.faces.first_loop[face.index()];
            while !at.is_none() && b.loops.next_loop[at.index()] != l {
                at = b.loops.next_loop[at.index()];
            }
            if !at.is_none() {
                b.loops.next_loop[at.index()] = next;
            }
        }
        b.loops.next_loop[l.index()] = LoopId::NONE;
        let len = b.loop_count();
        mark(&mut self.removed.loops, len, l.0);
        Ok(())
    }

    /// Removes a face, its loops and its sides in shells.
    pub fn remove_face(&mut self, f: FaceId) -> Result<(), BuildError> {
        if !self.has_face(f) {
            return Err(StoreError::Id.into());
        }
        while let Some(l) = Some(self.brep.first_loop(f)).filter(|l| !l.is_none()) {
            self.remove_loop(l)?;
        }
        for side in [Side::Out, Side::In] {
            let s = self.brep.face_shell(f, side);
            if self.has_shell(s) {
                self.brep.shells.sides[s.index()].retain(|&x| x != (f, side));
            }
        }
        let b = &mut self.brep;
        b.faces.shells[f.index()] = [ShellId::NONE; 2];
        let len = b.face_count();
        mark(&mut self.removed.faces, len, f.0);
        Ok(())
    }

    /// Removes an edge. Its coedges, and anything else naming it, must be
    /// removed too by [`Self::finish`].
    pub fn remove_edge(&mut self, e: EdgeId) -> Result<(), BuildError> {
        if !self.has_edge(e) {
            return Err(StoreError::Id.into());
        }
        let len = self.brep.edge_count();
        mark(&mut self.removed.edges, len, e.0);
        Ok(())
    }

    /// Removes a vertex. Its edges, and anything else naming it, must be
    /// removed too by [`Self::finish`].
    pub fn remove_vert(&mut self, v: VertId) -> Result<(), BuildError> {
        if !self.has_vert(v) {
            return Err(StoreError::Id.into());
        }
        let len = self.brep.vert_count();
        mark(&mut self.removed.verts, len, v.0);
        Ok(())
    }

    /// Removes every shell and region; the faces' sides are in none.
    pub fn clear_shells(&mut self) {
        let b = &mut self.brep;
        for s in b.faces.shells.iter_mut() {
            *s = [ShellId::NONE; 2];
        }
        self.removed.shells = vec![true; b.shell_count()];
        self.removed.regions = vec![true; b.region_count()];
    }

    /// Replaces a solid's shells and regions by those derived from its
    /// faces, edges and contact records ([`super::regions`]), the
    /// derivation's passes charged to `work`. The body must have no
    /// removed vertex, edge, coedge, loop or face (a construction's, or
    /// an edited body after [`Self::finish`]): a removed one is
    /// [`StoreError::Id`].
    pub fn fill_regions(&mut self, work: &mut Work) -> Result<(), BuildError> {
        let r = &self.removed;
        if [&r.verts, &r.edges, &r.coedges, &r.loops, &r.faces]
            .iter()
            .any(|m| m.iter().any(|&x| x))
        {
            return Err(StoreError::Id.into());
        }
        self.clear_shells();
        let d = derive(&self.brep, work).map_err(|h| match h {
            Halt::OutOfBudget(e) => BuildError::OutOfBudget(e),
            Halt::Failed { error, .. } => BuildError::Regions(error),
        })?;
        let mut shells = Vec::with_capacity(d.shells.len());
        for (sides, kind) in d.shells {
            shells.push(self.add_shell(kind, sides)?);
        }
        let mut regions: Vec<RegionId> = Vec::with_capacity(d.regions.len());
        for r in &d.regions {
            let outer = r.outer.map_or(ShellId::NONE, |s| shells[s]);
            let islands = r.islands.iter().map(|&s| shells[s]).collect();
            regions.push(self.add_region(r.kind, outer, islands, RegionId::NONE)?);
        }
        // A container may come after its lump.
        for (r, at) in d.regions.iter().zip(&regions) {
            if let Some(c) = r.container {
                self.brep.regions.container[at.index()] = regions[c];
            }
        }
        Ok(())
    }

    /// Removes every surface, curve and p-curve no remaining entity
    /// refers to (module docs, "Geometry collection").
    pub fn collect_geometry(&mut self) {
        let b = &self.brep;
        let mut surfaces = vec![true; b.surface_count()];
        let mut curves = vec![true; b.curve_count()];
        let mut pcurves = vec![true; b.pcurve_count()];
        let keep = |marks: &mut Vec<bool>, i: usize| {
            if let Some(m) = marks.get_mut(i) {
                *m = false;
            }
        };
        for f in 0..b.face_count() {
            let f = FaceId(f as u32);
            if self.has_face(f) {
                keep(&mut surfaces, b.face_surface_id(f).index());
            }
        }
        for e in 0..b.edge_count() {
            let e = EdgeId(e as u32);
            if self.has_edge(e) {
                keep(&mut curves, b.edge_curve_id(e).index());
                for s in b.origin(e).surfaces() {
                    keep(&mut surfaces, s.index());
                }
            }
        }
        for c in 0..b.coedge_count() {
            let c = CoedgeId(c as u32);
            if self.has_coedge(c) {
                keep(&mut pcurves, b.pcurve_id(c).index());
            }
        }
        // An intent names only surfaces below its own, so one pass from
        // the top keeps every surface a kept one leads to.
        for i in (0..b.surface_count()).rev() {
            if surfaces[i] {
                continue;
            }
            if let Some(fit) = &b.surfaces.fit[i] {
                for id in fit.intent.ids() {
                    match id {
                        IntentId::Surface(s) => keep(&mut surfaces, s.index()),
                        IntentId::Curve(c) => keep(&mut curves, c.index()),
                    }
                }
            }
            for r in b.surfaces.exact_rows[i].rows() {
                keep(&mut curves, r.curve.index());
            }
        }
        let or = |old: &[bool], new: Vec<bool>| -> Vec<bool> {
            new.into_iter()
                .enumerate()
                .map(|(i, m)| m || old.get(i).copied().unwrap_or(false))
                .collect()
        };
        self.removed.surfaces = or(&self.removed.surfaces, surfaces);
        self.removed.curves = or(&self.removed.curves, curves);
        self.removed.pcurves = or(&self.removed.pcurves, pcurves);
    }

    // Finishing.

    /// The body with the removed rows gone and every id mapped to its
    /// row's new one, and the map; or the first id naming a removed row
    /// (module docs, "Finishing").
    pub fn finish(mut self) -> Result<(Brep, Remap), BuildError> {
        self.rehold();
        let b = &self.brep;
        let r = &self.removed;
        let map = Remap {
            surfaces: compaction(&r.surfaces, b.surface_count()),
            curves: compaction(&r.curves, b.curve_count()),
            pcurves: compaction(&r.pcurves, b.pcurve_count()),
            verts: compaction(&r.verts, b.vert_count()),
            edges: compaction(&r.edges, b.edge_count()),
            coedges: compaction(&r.coedges, b.coedge_count()),
            loops: compaction(&r.loops, b.loop_count()),
            faces: compaction(&r.faces, b.face_count()),
            shells: compaction(&r.shells, b.shell_count()),
            regions: compaction(&r.regions, b.region_count()),
        };
        let mut brep = self.brep;
        remap_rows(&mut brep, &map).map_err(BuildError::Dangling)?;
        debug_assert_eq!(brep.dangling(), None, "a finished body names only its rows");
        Ok((brep, map))
    }

    /// Gives each remaining vertex and edge whose stored coedge was
    /// removed the first remaining coedge leaving (using) it, or none.
    fn rehold(&mut self) {
        let b = &self.brep;
        let lost_v: Vec<usize> = (0..b.vert_count())
            .filter(|&v| {
                let c = b.verts.coedge[v];
                !c.is_none() && !self.has_coedge(c)
            })
            .collect();
        let lost_e: Vec<usize> = (0..b.edge_count())
            .filter(|&e| {
                let c = b.edges.coedge[e];
                !c.is_none() && !self.has_coedge(c)
            })
            .collect();
        if lost_v.is_empty() && lost_e.is_empty() {
            return;
        }
        let b = &mut self.brep;
        for &v in &lost_v {
            b.verts.coedge[v] = CoedgeId::NONE;
        }
        for &e in &lost_e {
            b.edges.coedge[e] = CoedgeId::NONE;
        }
        for c in 0..b.coedge_count() {
            let c = CoedgeId(c as u32);
            if marked(&self.removed.coedges, c.0) {
                continue;
            }
            let e = b.edge(c);
            if e.index() >= b.edge_count() {
                continue;
            }
            if lost_e.binary_search(&e.index()).is_ok() && b.edges.coedge[e.index()].is_none() {
                b.edges.coedge[e.index()] = c;
            }
            let v = b.coedge_vert(c, false);
            if lost_v.binary_search(&v.index()).is_ok() && b.verts.coedge[v.index()].is_none() {
                b.verts.coedge[v.index()] = c;
            }
        }
    }
}

/// Maps every id of the rows `map` keeps, then drops the others; or the
/// first id naming a dropped row or none past its kind, in the order of
/// [`Brep::dangling`].
fn remap_rows(b: &mut Brep, map: &Remap) -> Result<(), Dangling> {
    // An id that may be `NONE`, and one that must name a row.
    fn opt(m: &[u32], id: u32, row: Row, field: &'static str) -> Result<u32, Dangling> {
        match look(m, id) {
            NONE if id != NONE => Err(Dangling { row, field }),
            new => Ok(new),
        }
    }
    fn req(m: &[u32], id: u32, row: Row, field: &'static str) -> Result<u32, Dangling> {
        match look(m, id) {
            NONE => Err(Dangling { row, field }),
            new => Ok(new),
        }
    }
    let kept = |m: &[u32], i: usize| m[i] != NONE;

    for i in (0..b.surface_count()).filter(|&i| kept(&map.surfaces, i)) {
        let row = Row::Surface(SurfId(i as u32));
        let s = &mut b.surfaces;
        let mut surface = |x: SurfId| SurfId(look(&map.surfaces, x.0)).filter_some();
        let mut curve = |x: CurveId| CurveId(look(&map.curves, x.0)).filter_some();
        if let Some(fit) = &mut s.fit[i] {
            fit.intent = fit
                .intent
                .remap(&mut surface, &mut curve)
                .ok_or(Dangling { row, field: "fit" })?;
        }
        s.exact_rows[i] = s.exact_rows[i].remap(&mut curve).ok_or(Dangling {
            row,
            field: "exact_rows",
        })?;
    }
    for i in (0..b.vert_count()).filter(|&i| kept(&map.verts, i)) {
        let row = Row::Vert(VertId(i as u32));
        let c = &mut b.verts.coedge[i];
        c.0 = opt(&map.coedges, c.0, row, "coedge")?;
    }
    for i in (0..b.edge_count()).filter(|&i| kept(&map.edges, i)) {
        let row = Row::Edge(EdgeId(i as u32));
        let e = &mut b.edges;
        e.curve[i].0 = req(&map.curves, e.curve[i].0, row, "curve")?;
        e.v_start[i].0 = req(&map.verts, e.v_start[i].0, row, "v_start")?;
        e.v_end[i].0 = req(&map.verts, e.v_end[i].0, row, "v_end")?;
        e.coedge[i].0 = opt(&map.coedges, e.coedge[i].0, row, "coedge")?;
        let origin = std::mem::take(&mut e.origin[i]);
        e.origin[i] = remap_origin(origin, &map.surfaces).ok_or(Dangling {
            row,
            field: "origin",
        })?;
    }
    for i in (0..b.coedge_count()).filter(|&i| kept(&map.coedges, i)) {
        let row = Row::Coedge(CoedgeId(i as u32));
        let c = &mut b.coedges;
        c.edge[i].0 = req(&map.edges, c.edge[i].0, row, "edge")?;
        c.face[i].0 = req(&map.faces, c.face[i].0, row, "face")?;
        c.lp[i].0 = req(&map.loops, c.lp[i].0, row, "loop")?;
        c.next[i].0 = req(&map.coedges, c.next[i].0, row, "next")?;
        c.prev[i].0 = req(&map.coedges, c.prev[i].0, row, "prev")?;
        c.pair[i].0 = opt(&map.coedges, c.pair[i].0, row, "pair")?;
        c.pcurve[i].0 = req(&map.pcurves, c.pcurve[i].0, row, "pcurve")?;
    }
    for i in (0..b.loop_count()).filter(|&i| kept(&map.loops, i)) {
        let row = Row::Loop(LoopId(i as u32));
        let l = &mut b.loops;
        l.face[i].0 = req(&map.faces, l.face[i].0, row, "face")?;
        l.first_coedge[i].0 = req(&map.coedges, l.first_coedge[i].0, row, "first_coedge")?;
        l.next_loop[i].0 = opt(&map.loops, l.next_loop[i].0, row, "next_loop")?;
    }
    for i in (0..b.face_count()).filter(|&i| kept(&map.faces, i)) {
        let row = Row::Face(FaceId(i as u32));
        let f = &mut b.faces;
        f.surface[i].0 = req(&map.surfaces, f.surface[i].0, row, "surface")?;
        for s in f.shells[i].iter_mut() {
            s.0 = opt(&map.shells, s.0, row, "shells")?;
        }
        f.first_loop[i].0 = opt(&map.loops, f.first_loop[i].0, row, "first_loop")?;
    }
    for i in (0..b.shell_count()).filter(|&i| kept(&map.shells, i)) {
        let row = Row::Shell(ShellId(i as u32));
        let s = &mut b.shells;
        for (f, _) in s.sides[i].iter_mut() {
            f.0 = req(&map.faces, f.0, row, "sides")?;
        }
        s.region[i].0 = opt(&map.regions, s.region[i].0, row, "region")?;
    }
    for i in (0..b.region_count()).filter(|&i| kept(&map.regions, i)) {
        let row = Row::Region(RegionId(i as u32));
        let r = &mut b.regions;
        r.outer[i].0 = opt(&map.shells, r.outer[i].0, row, "outer")?;
        for s in r.islands[i].iter_mut() {
            s.0 = req(&map.shells, s.0, row, "islands")?;
        }
        r.container[i].0 = opt(&map.regions, r.container[i].0, row, "container")?;
    }
    for (i, c) in b.contacts.iter_mut().enumerate() {
        let row = Row::Contact(i);
        for m in c.members.iter_mut().flatten() {
            *m = remap_member(*m, map).ok_or(Dangling {
                row,
                field: "members",
            })?;
        }
    }

    let s = &mut b.surfaces;
    compact(&mut s.surface, &map.surfaces);
    compact(&mut s.fit, &map.surfaces);
    compact(&mut s.exact_rows, &map.surfaces);
    compact(&mut b.curves3, &map.curves);
    compact(&mut b.pcurves, &map.pcurves);
    let v = &mut b.verts;
    let m = &map.verts;
    compact(&mut v.pos, m);
    compact(&mut v.tol, m);
    compact(&mut v.kind, m);
    compact(&mut v.coedge, m);
    compact(&mut v.tag, m);
    compact(&mut v.attrs, m);
    let e = &mut b.edges;
    let m = &map.edges;
    compact(&mut e.curve, m);
    compact(&mut e.t_range, m);
    compact(&mut e.v_start, m);
    compact(&mut e.v_end, m);
    compact(&mut e.kind, m);
    compact(&mut e.cont, m);
    compact(&mut e.origin, m);
    compact(&mut e.tol, m);
    compact(&mut e.dev, m);
    compact(&mut e.embedded_join, m);
    compact(&mut e.coedge, m);
    compact(&mut e.tag, m);
    compact(&mut e.attrs, m);
    let c = &mut b.coedges;
    let m = &map.coedges;
    compact(&mut c.edge, m);
    compact(&mut c.reversed, m);
    compact(&mut c.face, m);
    compact(&mut c.lp, m);
    compact(&mut c.next, m);
    compact(&mut c.prev, m);
    compact(&mut c.pair, m);
    compact(&mut c.pcurve, m);
    compact(&mut c.seam_bound, m);
    let l = &mut b.loops;
    let m = &map.loops;
    compact(&mut l.face, m);
    compact(&mut l.first_coedge, m);
    compact(&mut l.next_loop, m);
    compact(&mut l.is_outer, m);
    let f = &mut b.faces;
    let m = &map.faces;
    compact(&mut f.surface, m);
    compact(&mut f.flipped, m);
    compact(&mut f.shells, m);
    compact(&mut f.first_loop, m);
    compact(&mut f.key, m);
    compact(&mut f.attrs, m);
    let s = &mut b.shells;
    let m = &map.shells;
    compact(&mut s.sides, m);
    compact(&mut s.region, m);
    compact(&mut s.kind, m);
    let r = &mut b.regions;
    let m = &map.regions;
    compact(&mut r.kind, m);
    compact(&mut r.outer, m);
    compact(&mut r.islands, m);
    compact(&mut r.container, m);
    Ok(())
}

/// `origin` with its surfaces mapped by `map`; `None` if one is dropped.
fn remap_origin(origin: Origin, map: &[u32]) -> Option<Origin> {
    use super::brep::{ClusterOrigin, PlainOrigin};
    let cluster = |c: ClusterOrigin| -> Option<ClusterOrigin> {
        let sa = SurfId(look(map, c.sa.0)).filter_some()?;
        let sb = SurfId(look(map, c.sb.0)).filter_some()?;
        Some(ClusterOrigin { sa, sb, ..c })
    };
    let plain = |p: PlainOrigin| match p {
        PlainOrigin::Cluster(c) => cluster(c).map(PlainOrigin::Cluster),
        p => Some(p),
    };
    Some(match origin {
        Origin::Plain(p) => Origin::Plain(plain(p)?),
        Origin::MovedJoin { prior, clusters } => Origin::MovedJoin {
            prior: plain(prior)?,
            clusters: clusters
                .into_iter()
                .map(cluster)
                .collect::<Option<Vec<_>>>()?,
        },
    })
}

/// `m` with its entity mapped by `map`; `None` if it is dropped.
fn remap_member(m: Member, map: &Remap) -> Option<Member> {
    Some(match m {
        Member::Vert(v) => Member::Vert(map.verts(v).filter_some()?),
        Member::Edge(e) => Member::Edge(map.edges(e).filter_some()?),
        Member::EdgeAt { edge, t } => Member::EdgeAt {
            edge: map.edges(edge).filter_some()?,
            t,
        },
        Member::FaceAt { face, uv } => Member::FaceAt {
            face: map.faces(face).filter_some()?,
            uv,
        },
    })
}

/// An id, `None` for `NONE`.
trait FilterSome: Sized {
    fn filter_some(self) -> Option<Self>;
}

macro_rules! filter_some {
    ($($id:ident),*) => {$(
        impl FilterSome for $id {
            fn filter_some(self) -> Option<$id> {
                (!self.is_none()).then_some(self)
            }
        }
    )*};
}

filter_some!(SurfId, CurveId, VertId, EdgeId, FaceId);

impl Brep {
    /// The vertex a use of `e` (`reversed` or not) ends at (`end`) or
    /// starts at.
    fn coedge_vert_of(&self, e: EdgeId, reversed: bool, end: bool) -> VertId {
        let [vs, ve] = self.ends(e);
        if end != reversed { ve } else { vs }
    }

    /// The first stored id that names no row (past its kind's rows, or
    /// `NONE` where the field must name one, or a surface not below the
    /// surface whose intent names it), by kind (surfaces, vertices,
    /// edges, coedges, loops, faces, shells, regions, contacts), then row,
    /// then field; none if every id names a row. A store sanity check: what
    /// the rows say of each other is the body's check's.
    pub(crate) fn dangling(&self) -> Option<Dangling> {
        // Whether `id` names a row of a kind with `n`, or is `NONE` where
        // that is allowed.
        let ok = |id: u32, n: usize, none: bool| (id as usize) < n || (none && id == NONE);
        let (nv, ne, nc) = (self.vert_count(), self.edge_count(), self.coedge_count());
        let (nl, nf, ns, nr) = (
            self.loop_count(),
            self.face_count(),
            self.shell_count(),
            self.region_count(),
        );
        let first = |row: Row, fields: &[(&'static str, bool)]| {
            fields
                .iter()
                .find(|(_, good)| !good)
                .map(|&(field, _)| Dangling { row, field })
        };
        for i in 0..self.surface_count() {
            let s = &self.surfaces;
            let fit = s.fit[i]
                .as_ref()
                .is_none_or(|f| self.surface_refs_below(Some(f), &ExactRows::default(), i));
            let rows = self.surface_refs_below(None, &s.exact_rows[i], i);
            let found = first(
                Row::Surface(SurfId(i as u32)),
                &[("fit", fit), ("exact_rows", rows)],
            );
            if found.is_some() {
                return found;
            }
        }
        for (i, c) in self.verts.coedge.iter().enumerate() {
            let row = Row::Vert(VertId(i as u32));
            let found = first(row, &[("coedge", ok(c.0, nc, true))]);
            if found.is_some() {
                return found;
            }
        }
        for i in 0..ne {
            let row = Row::Edge(EdgeId(i as u32));
            let e = &self.edges;
            let origin = e.origin[i]
                .surfaces()
                .all(|s| ok(s.0, self.surface_count(), false));
            let found = first(
                row,
                &[
                    ("curve", ok(e.curve[i].0, self.curve_count(), false)),
                    ("v_start", ok(e.v_start[i].0, nv, false)),
                    ("v_end", ok(e.v_end[i].0, nv, false)),
                    ("coedge", ok(e.coedge[i].0, nc, true)),
                    ("origin", origin),
                ],
            );
            if found.is_some() {
                return found;
            }
        }
        for i in 0..nc {
            let row = Row::Coedge(CoedgeId(i as u32));
            let c = &self.coedges;
            let found = first(
                row,
                &[
                    ("edge", ok(c.edge[i].0, ne, false)),
                    ("face", ok(c.face[i].0, nf, false)),
                    ("loop", ok(c.lp[i].0, nl, false)),
                    ("next", ok(c.next[i].0, nc, false)),
                    ("prev", ok(c.prev[i].0, nc, false)),
                    ("pair", ok(c.pair[i].0, nc, true)),
                    ("pcurve", ok(c.pcurve[i].0, self.pcurve_count(), false)),
                ],
            );
            if found.is_some() {
                return found;
            }
        }
        for i in 0..nl {
            let row = Row::Loop(LoopId(i as u32));
            let l = &self.loops;
            let found = first(
                row,
                &[
                    ("face", ok(l.face[i].0, nf, false)),
                    ("first_coedge", ok(l.first_coedge[i].0, nc, false)),
                    ("next_loop", ok(l.next_loop[i].0, nl, true)),
                ],
            );
            if found.is_some() {
                return found;
            }
        }
        for i in 0..nf {
            let row = Row::Face(FaceId(i as u32));
            let f = &self.faces;
            let found = first(
                row,
                &[
                    ("surface", ok(f.surface[i].0, self.surface_count(), false)),
                    ("shells", f.shells[i].iter().all(|s| ok(s.0, ns, true))),
                    ("first_loop", ok(f.first_loop[i].0, nl, true)),
                ],
            );
            if found.is_some() {
                return found;
            }
        }
        for i in 0..ns {
            let row = Row::Shell(ShellId(i as u32));
            let s = &self.shells;
            let found = first(
                row,
                &[
                    ("sides", s.sides[i].iter().all(|(f, _)| ok(f.0, nf, false))),
                    ("region", ok(s.region[i].0, nr, true)),
                ],
            );
            if found.is_some() {
                return found;
            }
        }
        for i in 0..nr {
            let row = Row::Region(RegionId(i as u32));
            let r = &self.regions;
            let found = first(
                row,
                &[
                    ("outer", ok(r.outer[i].0, ns, true)),
                    ("islands", r.islands[i].iter().all(|s| ok(s.0, ns, false))),
                    ("container", ok(r.container[i].0, nr, true)),
                ],
            );
            if found.is_some() {
                return found;
            }
        }
        for (i, c) in self.contacts.iter().enumerate() {
            let good = c.members.iter().flatten().all(|m| match *m {
                Member::Vert(v) => ok(v.0, nv, false),
                Member::Edge(e) | Member::EdgeAt { edge: e, .. } => ok(e.0, ne, false),
                Member::FaceAt { face, .. } => ok(face.0, nf, false),
            });
            if !good {
                return Some(Dangling {
                    row: Row::Contact(i),
                    field: "members",
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;
