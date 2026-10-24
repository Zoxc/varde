//! The B-rep store: every entity of a body and every field it keeps, as
//! structure-of-arrays columns indexed by `u32` ids, `NONE` (`u32::MAX`)
//! for "no entity".
//!
//! **Geometry** is immutable once an entity refers to it, and may be
//! shared: tiled surfaces (with a fitted surface's [`Fit`] and its
//! [`ExactRows`]; what an intent names is a surface added before it, or a
//! curve), space curves ([`Curve3D`], control points stored affine with
//! their weights) and p-curves ([`PCurve`], one per coedge). Every stored
//! scalar is an `f64`, rounded once where it was computed at higher
//! precision.
//!
//! **Topology.**
//! - A vertex: its position `pos`, tolerance `tol`, kind (regular, or a
//!   pole: the singular point of a surface, apexes included), one coedge
//!   leaving it, its tag (`Pole(N | S)` or `Apex`; a pole snapped away
//!   keeps its tag with kind regular) and attributes.
//! - An edge: its curve, the curve's parameter range `t_range` (two
//!   knots), its vertices, **canonical**: the curve runs from the lower
//!   vertex id to the higher (`v_start ≤ v_end`); a closed edge, both ends
//!   one vertex, runs the curve's way. Its kind ([`EdgeKind`]), continuity
//!   ([`Continuity`]), origin ([`Origin`]), tolerance `tol`, deviation
//!   `dev` (from the true curve it stands for), whether its join zone is
//!   certified embedded (`embedded_join`), one coedge, its tag (`Seam`)
//!   and attributes. No edge is without vertices: a closed curve has one
//!   vertex on it.
//! - A coedge: a use of an edge by a loop of a face, running the edge's
//!   way or, `reversed`, against it; `next` and `prev` round its loop;
//!   `pair` the edge's other coedge (`NONE` on a singular edge, which has
//!   one coedge, and on a sheet's open boundary); its p-curve, over the
//!   edge's parameter; its `seam_bound`, the width of its ribbon.
//! - A loop: its face, first coedge, the face's next loop, whether outer.
//! - A face: its surface, `flipped` (its outward normal against the
//!   surface's), its two shells (the shell of its `Out` side, towards its
//!   normal, and of its `In` side, in the material), first loop, key
//!   ([`FaceKey`]) and attributes. A face's vertex UVs are not stored:
//!   they are its p-curves' end control points.
//! - A shell: a side-shell, the faces' sides it joins, its region and its
//!   kind (`Outer` or `Island` of a solid, `Open` on a sheet).
//! - A region: material (a lump) or empty (a void, or the unbounded
//!   region), its outer shell, its island shells, and the empty region a
//!   lump's empty islands lie in (its container). Regions are derived from
//!   the shells; a solid with no lumps is the empty solid.
//!
//! **Kind, `Seam` and `touch`** are stored on each edge but never carried
//! by an operation: the operation's assembly derives them for every edge
//! of its result from the edge's two faces there.
//!
//! **Records.** [`Contact`]s are the only records kept on a body from one
//! operation to the next; every other record lives for one operation.
//!
//! **Tolerances and status.** The body keeps the document's tolerance
//! with the largest edge tolerance it admits ([`Tolerances`]) and the
//! status of the operation that made it ([`BodyStatus`]).
//!
//! The store hands out ids below `NONE` in order of addition, so a kind's
//! ids are `0..count`. Adding a row checks only what it refers to below
//! itself (a surface's intent and exact rows, an edge's curve and
//! vertices, a coedge's edge and p-curve, a face's surface) and the edge's own canonical form; links within the
//! topology (`next`, `pair`, loops, shells, regions) may be set in any
//! order and are checked by the body's check.

use crate::geom::curve::{Curve3D, Span3};
use crate::geom::fit_types::IntentId;
use crate::geom::pcurve::PCurve;
use crate::geom::surface::Surface;
use crate::infra::ops::{MAX_WARNINGS, Status, Tolerance};
use crate::names::keys::TermTable;

use varde_decide::num::decide::Sign;

pub use crate::geom::fit_types::{ExactRow, ExactRows, Fit};
pub use crate::names::keys::{Digest, EdgeTag, FaceKey, FacePart, PoleEnd, VertTag};
pub use varde_decide::ids::{CoedgeId, CurveId, EdgeId, FaceId, LoopId, NONE, SurfId, VertId};

macro_rules! store_ids {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl $name {
            /// No entity.
            pub const NONE: $name = $name(NONE);

            /// The id as an index into its kind's columns.
            #[inline]
            pub fn index(self) -> usize {
                self.0 as usize
            }

            /// Whether this is [`NONE`], naming no entity.
            #[inline]
            pub fn is_none(self) -> bool {
                self.0 == NONE
            }
        }
    )*};
}

store_ids! {
    /// A p-curve.
    PCurveId;
    /// A shell.
    ShellId;
    /// A region.
    RegionId;
}

/// A set of attributes, interned in the body's name tables; `NONE` for
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttrSetId(pub u32);

impl AttrSetId {
    /// No attributes.
    pub const NONE: AttrSetId = AttrSetId(NONE);
}

impl Default for AttrSetId {
    fn default() -> Self {
        Self::NONE
    }
}

/// What a body is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BodyKind {
    /// A solid: closed shells only (outer and island).
    #[default]
    Solid,
    /// A sheet: open shells, whose boundary coedges have no pair. Never a
    /// boolean operand.
    Sheet,
}

/// A vertex's kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VertKind {
    /// An ordinary vertex.
    #[default]
    Regular,
    /// A singular point of a surface (a pole or an apex), the end of its
    /// singular edges.
    Pole,
}

/// An edge's kind, derived from its two faces at the assembly of the
/// operation making the body (module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// An edge between faces of different forms.
    #[default]
    Regular,
    /// A pole's or apex's side: one coedge, the curve a point, both ends
    /// the pole.
    Singular,
    /// An edge between faces on surfaces of bit-identical form keys; its
    /// p-curves iso or linear on both sides. `touch`: the faces have
    /// different keys and only touched, never overlapped.
    SameForm {
        /// Whether the faces only touched.
        touch: bool,
    },
}

impl EdgeKind {
    /// Whether this is a [`SameForm`](Self::SameForm) edge whose faces
    /// only touched.
    pub fn is_touch(self) -> bool {
        self == EdgeKind::SameForm { touch: true }
    }
}

/// The sign of a crease along its edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Convexity {
    /// Convex along the whole edge.
    Convex,
    /// Concave along the whole edge.
    Concave,
    /// Either, or not certified: the sign changes or wasn't settled. The
    /// edge is never split for it.
    Mixed,
}

/// How smoothly an edge's two faces meet, a certified field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Continuity {
    /// A crease. A zero-length edge is a mixed crease, uncertified.
    Crease(Convexity),
    /// Tangent: the angle between the faces' normals is certified within
    /// the tangent threshold along the whole edge.
    Tangent {
        /// The achieved bound on that angle (radians).
        angle: f64,
    },
    /// Curvature-continuous: an edge of the same form, or a join its
    /// construction declares so.
    Curvature,
}

impl Default for Continuity {
    fn default() -> Self {
        Continuity::Crease(Convexity::Mixed)
    }
}

/// Which leaf certificate made a cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClusterKind {
    /// Near leaves: a two-sided gap.
    Near,
    /// Contact leaves: a signed gap.
    Contact,
}

/// The origin of an edge a tangency cluster made: the cluster's kind,
/// the two immutable surfaces it ran on and `σ_A` of the operation's
/// run (not the operation, so one record serves both runs of a split).
/// The surfaces are kept as roots, so the cluster can be run again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClusterOrigin {
    /// The cluster's kind.
    pub kind: ClusterKind,
    /// The first operand's surface.
    pub sa: SurfId,
    /// The second operand's surface.
    pub sb: SurfId,
    /// `σ_A` of the run.
    pub sigma: Sign,
}

/// An origin an edge may have before its first move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlainOrigin {
    /// Constructed exactly.
    #[default]
    Exact,
    /// A simple intersection branch, fitted.
    Simple,
    /// A tangency cluster's edge (on the mid-surface): its rim or a
    /// snapped edge.
    Cluster(ClusterOrigin),
}

/// How an edge was made: what its tolerance target is, and what an
/// operation re-deriving it runs again.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// Any origin but a moved join.
    Plain(PlainOrigin),
    /// An edge a snap moved an end of, refit: its origin before its first
    /// move, never itself a moved join, and every cluster that moved it,
    /// in this and earlier operations, each once, in the order added.
    MovedJoin {
        /// The origin before the first move.
        prior: PlainOrigin,
        /// The moving clusters.
        clusters: Vec<ClusterOrigin>,
    },
}

impl Default for Origin {
    fn default() -> Self {
        Origin::Plain(PlainOrigin::Exact)
    }
}

impl From<PlainOrigin> for Origin {
    fn from(o: PlainOrigin) -> Origin {
        Origin::Plain(o)
    }
}

impl Origin {
    /// The origin before the edge's first move: its own if never moved.
    pub fn prior(&self) -> PlainOrigin {
        match self {
            Origin::Plain(p) | Origin::MovedJoin { prior: p, .. } => *p,
        }
    }

    /// The clusters that moved the edge; none if never moved.
    pub fn clusters(&self) -> &[ClusterOrigin] {
        match self {
            Origin::Plain(_) => &[],
            Origin::MovedJoin { clusters, .. } => clusters,
        }
    }

    /// The origin after `cluster` moved an end of the edge: a moved join
    /// keeping the prior origin, `cluster` added after the others unless
    /// it is among them already.
    pub fn moved(self, cluster: ClusterOrigin) -> Origin {
        match self {
            Origin::Plain(prior) => Origin::MovedJoin {
                prior,
                clusters: vec![cluster],
            },
            Origin::MovedJoin {
                prior,
                mut clusters,
            } => {
                if !clusters.contains(&cluster) {
                    clusters.push(cluster);
                }
                Origin::MovedJoin { prior, clusters }
            }
        }
    }

    /// The surfaces the origin names (a cluster's two, the prior origin's
    /// first, then each moving cluster's), roots the surface collection
    /// keeps.
    pub fn surfaces(&self) -> impl Iterator<Item = SurfId> + '_ {
        let prior = match self.prior() {
            PlainOrigin::Cluster(c) => Some(c),
            _ => None,
        };
        prior
            .into_iter()
            .chain(self.clusters().iter().copied())
            .flat_map(|c| [c.sa, c.sb])
    }
}

/// A face's side: towards its outward normal (empty) or into the
/// material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// Towards the normal.
    Out,
    /// Into the material.
    In,
}

impl Side {
    /// The side's place in a face's pair of shells: `Out` 0, `In` 1.
    pub fn index(self) -> usize {
        match self {
            Side::Out => 0,
            Side::In => 1,
        }
    }
}

/// A shell's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellKind {
    /// The outer shell of the region it faces (negative signed volume).
    Outer,
    /// An island inside the region of the innermost outer shell of its
    /// kind enclosing it (positive signed volume).
    Island,
    /// A sheet's open shell; part of no region.
    Open,
}

/// A region's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegionKind {
    /// A lump.
    Material,
    /// A void, or the unbounded region.
    Empty,
}

/// What a contact record joins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContactKind {
    /// Along edges.
    Edge,
    /// At a point.
    Vertex,
    /// Over a region: an import's contact leaves, consumed by the load's
    /// snap and never kept on a loaded body or an operand.
    Region,
}

/// One side of a contact record's member pair: an entity of the body, or
/// a point on an edge or a face where a vertex rests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Member {
    /// A vertex.
    Vert(VertId),
    /// An edge (zero-length edges included).
    Edge(EdgeId),
    /// The point of an edge at parameter `t`.
    EdgeAt {
        /// The edge.
        edge: EdgeId,
        /// The parameter on its curve.
        t: f64,
    },
    /// The point of a face at `uv`.
    FaceAt {
        /// The face.
        face: FaceId,
        /// The point in the face's global UV.
        uv: [f64; 2],
    },
}

/// A record of entities of the body that touch without crossing,
/// certified, kept from operation to operation and moved with the
/// entities by each operation's history. Both members of each pair are
/// entities of the body.
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    /// What it joins.
    pub kind: ContactKind,
    /// The member pairs.
    pub members: Vec<[Member; 2]>,
    /// Its certificate.
    pub cert: ContactCert,
}

/// A contact's certificate: per face pair the separators or one
/// separating cone, the orientation class and the ball or tube radius.
/// The store keeps its place; its content comes with the certificate.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactCert {}

/// The body's provenance of its faces, edges and vertices. The store
/// keeps its place; its content comes with the names.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Provenance {}

/// The body's name tables: aliases per key, the digests' term table and
/// the interned attribute sets. The store keeps their place; so far the
/// term table is filled in, the rest comes with the names.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameTables {
    pub(crate) terms: TermTable,
}

impl NameTables {
    /// What the digests the body's keys hold stand for.
    pub fn terms(&self) -> &TermTable {
        &self.terms
    }
}

/// The body's tolerances: the document's tolerance `ε`, passed to every
/// operation on it, and `max_tol`, the largest edge tolerance the body
/// admits. An edge whose achieved tolerance exceeds `max_tol` raises it,
/// so every edge of the body is within it and every gap of the body
/// within twice it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerances {
    eps: Tolerance,
    max_tol: f64,
}

impl Tolerances {
    /// The tolerances at `eps`: `max_tol` the largest tolerance an edge
    /// may have without being wide, `ε/2` (exact).
    pub fn new(eps: Tolerance) -> Tolerances {
        Tolerances {
            eps,
            max_tol: eps.eps() * 0.5,
        }
    }

    /// The document's tolerance.
    pub fn eps(&self) -> Tolerance {
        self.eps
    }

    /// The largest edge tolerance the body admits (mm).
    pub fn max_tol(&self) -> f64 {
        self.max_tol
    }

    /// Whether an edge of tolerance `tol` is within `max_tol`.
    pub fn admits(&self, tol: f64) -> bool {
        tol <= self.max_tol
    }

    /// Whether an edge of tolerance `tol` is wide: above `ε/2`, and so
    /// listed in its operation's status.
    pub fn is_wide(&self, tol: f64) -> bool {
        tol > self.eps.eps() * 0.5
    }

    /// Raises `max_tol` to `tol` if `tol` exceeds it, whether it did.
    /// Tolerances are finite; a value that isn't changes nothing.
    pub fn raise(&mut self, tol: f64) -> bool {
        debug_assert!(tol.is_finite(), "a tolerance is finite: {tol}");
        let raised = tol.is_finite() && tol > self.max_tol;
        if raised {
            self.max_tol = tol;
        }
        raised
    }
}

/// The face pairs an operation newly joined across touching edges, by
/// key: each pair lesser key first, the first [`MAX_WARNINGS`] listed in
/// the order added and all counted. The operation adds one pair per pair
/// of regions, in leaf order, after its outcome is fixed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Touching {
    pairs: Vec<(FaceKey, FaceKey)>,
    count: u64,
}

impl Touching {
    /// Adds the pair of `a` and `b`: listed if fewer than
    /// [`MAX_WARNINGS`] are, counted either way.
    pub fn add(&mut self, a: FaceKey, b: FaceKey) {
        if self.pairs.len() < MAX_WARNINGS {
            self.pairs.push(if b < a { (b, a) } else { (a, b) });
        }
        self.count = self.count.saturating_add(1);
    }

    /// The listed pairs.
    pub fn pairs(&self) -> &[(FaceKey, FaceKey)] {
        &self.pairs
    }

    /// How many pairs were added, listed or not.
    pub fn count(&self) -> u64 {
        self.count
    }
}

/// A region changed where an operation settled a tangent contact. The
/// status keeps its place; its content comes with the operations that
/// report one.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Zone {}

/// An edge or vertex whose tolerance is above the contract. The status
/// keeps its place; its content comes with the operations that report
/// one.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bound {}

/// A note of an operation (a region the label audit skipped, a move's
/// rounding). The status keeps its place; its content comes with the
/// operations that report one.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {}

/// The fate of each entity through an operation. The status keeps its
/// place; its content comes with the names.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {}

/// The status of the operation that made the body: the operation's
/// [`Status`] (approximate, largest deviation, warnings) with the
/// touching faces it joined, its zones, bounds, notes and history.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyStatus {
    op: Status,
    touching: Touching,
    zones: Vec<Zone>,
    bounds: Vec<Bound>,
    notes: Vec<Note>,
    history: History,
}

impl BodyStatus {
    /// The status `op`, nothing else reported.
    pub fn new(op: Status) -> BodyStatus {
        BodyStatus {
            op,
            touching: Touching::default(),
            zones: Vec::new(),
            bounds: Vec::new(),
            notes: Vec::new(),
            history: History::default(),
        }
    }

    /// The operation's status: approximate, deviation, warnings.
    pub fn op(&self) -> &Status {
        &self.op
    }

    /// The operation's status, to add warnings to.
    pub fn op_mut(&mut self) -> &mut Status {
        &mut self.op
    }

    /// The touching face pairs newly joined.
    pub fn touching(&self) -> &Touching {
        &self.touching
    }

    /// The touching face pairs, to add to.
    pub fn touching_mut(&mut self) -> &mut Touching {
        &mut self.touching
    }

    /// The zones.
    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    /// The bounds.
    pub fn bounds(&self) -> &[Bound] {
        &self.bounds
    }

    /// The notes.
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// The history.
    pub fn history(&self) -> &History {
        &self.history
    }
}

/// Why a row can't be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

/// The next id of a kind with `len` entities: below `NONE`.
fn next_id(len: usize) -> Result<u32, StoreError> {
    u32::try_from(len)
        .ok()
        .filter(|&i| i != NONE)
        .ok_or(StoreError::Full)
}

/// A surface's row.
#[derive(Clone, Debug)]
pub(crate) struct SurfaceRow {
    pub(crate) surface: Surface,
    pub(crate) fit: Option<Fit>,
    pub(crate) exact_rows: ExactRows,
}

/// A vertex's row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VertRow {
    pub(crate) pos: [f64; 3],
    pub(crate) tol: f64,
    pub(crate) kind: VertKind,
    pub(crate) coedge: CoedgeId,
    pub(crate) tag: Option<VertTag>,
    pub(crate) attrs: AttrSetId,
}

/// An edge's row.
#[derive(Clone, Debug)]
pub(crate) struct EdgeRow {
    pub(crate) curve: CurveId,
    pub(crate) t_range: [f64; 2],
    pub(crate) v_start: VertId,
    pub(crate) v_end: VertId,
    pub(crate) kind: EdgeKind,
    pub(crate) cont: Continuity,
    pub(crate) origin: Origin,
    pub(crate) tol: f64,
    pub(crate) dev: f64,
    pub(crate) embedded_join: bool,
    pub(crate) coedge: CoedgeId,
    pub(crate) tag: Option<EdgeTag>,
    pub(crate) attrs: AttrSetId,
}

/// A coedge's row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CoedgeRow {
    pub(crate) edge: EdgeId,
    pub(crate) reversed: bool,
    pub(crate) face: FaceId,
    pub(crate) lp: LoopId,
    pub(crate) next: CoedgeId,
    pub(crate) prev: CoedgeId,
    pub(crate) pair: CoedgeId,
    pub(crate) pcurve: PCurveId,
    pub(crate) seam_bound: f64,
}

/// A loop's row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoopRow {
    pub(crate) face: FaceId,
    pub(crate) first_coedge: CoedgeId,
    pub(crate) next_loop: LoopId,
    pub(crate) is_outer: bool,
}

/// A face's row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FaceRow {
    pub(crate) surface: SurfId,
    pub(crate) flipped: bool,
    pub(crate) shells: [ShellId; 2],
    pub(crate) first_loop: LoopId,
    pub(crate) key: FaceKey,
    pub(crate) attrs: AttrSetId,
}

/// A shell's row.
#[derive(Clone, Debug)]
pub(crate) struct ShellRow {
    pub(crate) sides: Vec<(FaceId, Side)>,
    pub(crate) region: RegionId,
    pub(crate) kind: ShellKind,
}

/// A region's row.
#[derive(Clone, Debug)]
pub(crate) struct RegionRow {
    pub(crate) kind: RegionKind,
    pub(crate) outer: ShellId,
    pub(crate) islands: Vec<ShellId>,
    pub(crate) container: RegionId,
}

/// The surfaces' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Surfaces {
    pub(crate) surface: Vec<Surface>,
    pub(crate) fit: Vec<Option<Fit>>,
    pub(crate) exact_rows: Vec<ExactRows>,
}

/// The vertices' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Verts {
    pub(crate) pos: Vec<[f64; 3]>,
    pub(crate) tol: Vec<f64>,
    pub(crate) kind: Vec<VertKind>,
    pub(crate) coedge: Vec<CoedgeId>,
    pub(crate) tag: Vec<Option<VertTag>>,
    pub(crate) attrs: Vec<AttrSetId>,
}

/// The edges' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Edges {
    pub(crate) curve: Vec<CurveId>,
    pub(crate) t_range: Vec<[f64; 2]>,
    pub(crate) v_start: Vec<VertId>,
    pub(crate) v_end: Vec<VertId>,
    pub(crate) kind: Vec<EdgeKind>,
    pub(crate) cont: Vec<Continuity>,
    pub(crate) origin: Vec<Origin>,
    pub(crate) tol: Vec<f64>,
    pub(crate) dev: Vec<f64>,
    pub(crate) embedded_join: Vec<bool>,
    pub(crate) coedge: Vec<CoedgeId>,
    pub(crate) tag: Vec<Option<EdgeTag>>,
    pub(crate) attrs: Vec<AttrSetId>,
}

/// The coedges' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Coedges {
    pub(crate) edge: Vec<EdgeId>,
    pub(crate) reversed: Vec<bool>,
    pub(crate) face: Vec<FaceId>,
    pub(crate) lp: Vec<LoopId>,
    pub(crate) next: Vec<CoedgeId>,
    pub(crate) prev: Vec<CoedgeId>,
    pub(crate) pair: Vec<CoedgeId>,
    pub(crate) pcurve: Vec<PCurveId>,
    pub(crate) seam_bound: Vec<f64>,
}

/// The loops' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Loops {
    pub(crate) face: Vec<FaceId>,
    pub(crate) first_coedge: Vec<CoedgeId>,
    pub(crate) next_loop: Vec<LoopId>,
    pub(crate) is_outer: Vec<bool>,
}

/// The faces' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Faces {
    pub(crate) surface: Vec<SurfId>,
    pub(crate) flipped: Vec<bool>,
    pub(crate) shells: Vec<[ShellId; 2]>,
    pub(crate) first_loop: Vec<LoopId>,
    pub(crate) key: Vec<FaceKey>,
    pub(crate) attrs: Vec<AttrSetId>,
}

/// The shells' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Shells {
    pub(crate) sides: Vec<Vec<(FaceId, Side)>>,
    pub(crate) region: Vec<RegionId>,
    pub(crate) kind: Vec<ShellKind>,
}

/// The regions' columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Regions {
    pub(crate) kind: Vec<RegionKind>,
    pub(crate) outer: Vec<ShellId>,
    pub(crate) islands: Vec<Vec<ShellId>>,
    pub(crate) container: Vec<RegionId>,
}

/// The B-rep store (module docs).
#[derive(Clone, Debug)]
pub struct Brep {
    pub(crate) kind: BodyKind,
    pub(crate) surfaces: Surfaces,
    pub(crate) curves3: Vec<Curve3D>,
    pub(crate) pcurves: Vec<PCurve>,
    pub(crate) verts: Verts,
    pub(crate) edges: Edges,
    pub(crate) coedges: Coedges,
    pub(crate) loops: Loops,
    pub(crate) faces: Faces,
    pub(crate) shells: Shells,
    pub(crate) regions: Regions,
    pub(crate) contacts: Vec<Contact>,
    pub(crate) prov: Provenance,
    pub(crate) names: NameTables,
    pub(crate) eps: Tolerances,
    pub(crate) status: BodyStatus,
}

/// Whether `t` is a knot of `c`, bit for bit.
fn is_knot(c: &Curve3D, t: f64) -> bool {
    c.knots().iter().any(|k| k.to_bits() == t.to_bits())
}

impl Brep {
    /// An empty body of kind `kind` made by an operation of status
    /// `status`, at the operation's tolerance; `max_tol` at its least
    /// ([`Tolerances::new`]).
    pub fn new(kind: BodyKind, status: Status) -> Brep {
        Brep {
            kind,
            surfaces: Surfaces::default(),
            curves3: Vec::new(),
            pcurves: Vec::new(),
            verts: Verts::default(),
            edges: Edges::default(),
            coedges: Coedges::default(),
            loops: Loops::default(),
            faces: Faces::default(),
            shells: Shells::default(),
            regions: Regions::default(),
            contacts: Vec::new(),
            prov: Provenance::default(),
            names: NameTables::default(),
            eps: Tolerances::new(status.eps()),
            status: BodyStatus::new(status),
        }
    }

    /// Adds a surface; what its fit's intent and exact rows name must be
    /// below it (a surface added before, a curve).
    pub(crate) fn push_surface(&mut self, row: SurfaceRow) -> Result<SurfId, StoreError> {
        let id = next_id(self.surfaces.surface.len())?;
        if !self.surface_refs_below(row.fit.as_ref(), &row.exact_rows, id as usize) {
            return Err(StoreError::Id);
        }
        let s = &mut self.surfaces;
        s.surface.push(row.surface);
        s.fit.push(row.fit);
        s.exact_rows.push(row.exact_rows);
        Ok(SurfId(id))
    }

    /// Whether every id `fit` and `rows` name is a surface below `below`
    /// or a curve of the body.
    pub(crate) fn surface_refs_below(
        &self,
        fit: Option<&Fit>,
        rows: &ExactRows,
        below: usize,
    ) -> bool {
        let curves = self.curves3.len();
        let ids = fit.map(|f| f.intent.ids()).unwrap_or_default();
        ids.iter().all(|id| match *id {
            IntentId::Surface(s) => s.index() < below,
            IntentId::Curve(c) => c.index() < curves,
        }) && rows.rows().iter().all(|r| r.curve.index() < curves)
    }

    /// Adds a space curve.
    pub(crate) fn push_curve(&mut self, c: Curve3D) -> Result<CurveId, StoreError> {
        let id = next_id(self.curves3.len())?;
        self.curves3.push(c);
        Ok(CurveId(id))
    }

    /// Adds a p-curve.
    pub(crate) fn push_pcurve(&mut self, p: PCurve) -> Result<PCurveId, StoreError> {
        let id = next_id(self.pcurves.len())?;
        self.pcurves.push(p);
        Ok(PCurveId(id))
    }

    /// Adds a vertex; its position must be finite.
    pub(crate) fn push_vert(&mut self, row: VertRow) -> Result<VertId, StoreError> {
        if !row.pos.iter().all(|x| x.is_finite()) {
            return Err(StoreError::NotFinite);
        }
        let id = next_id(self.verts.pos.len())?;
        let v = &mut self.verts;
        v.pos.push(row.pos);
        v.tol.push(row.tol);
        v.kind.push(row.kind);
        v.coedge.push(row.coedge);
        v.tag.push(row.tag);
        v.attrs.push(row.attrs);
        Ok(VertId(id))
    }

    /// Adds an edge: its curve and vertices must be there, its vertices
    /// canonical (`v_start ≤ v_end`) and its `t_range` two increasing
    /// knots of the curve.
    pub(crate) fn push_edge(&mut self, row: EdgeRow) -> Result<EdgeId, StoreError> {
        let c = self.curves3.get(row.curve.index()).ok_or(StoreError::Id)?;
        let verts = self.verts.pos.len();
        if row.v_start.index() >= verts || row.v_end.index() >= verts {
            return Err(StoreError::Id);
        }
        if row.v_start > row.v_end {
            return Err(StoreError::NotCanonical);
        }
        let [t0, t1] = row.t_range;
        if !(t0 < t1 && is_knot(c, t0) && is_knot(c, t1)) {
            return Err(StoreError::TRange);
        }
        let id = next_id(self.edges.curve.len())?;
        let e = &mut self.edges;
        e.curve.push(row.curve);
        e.t_range.push(row.t_range);
        e.v_start.push(row.v_start);
        e.v_end.push(row.v_end);
        e.kind.push(row.kind);
        e.cont.push(row.cont);
        e.origin.push(row.origin);
        e.tol.push(row.tol);
        e.dev.push(row.dev);
        e.embedded_join.push(row.embedded_join);
        e.coedge.push(row.coedge);
        e.tag.push(row.tag);
        e.attrs.push(row.attrs);
        Ok(EdgeId(id))
    }

    /// Adds a coedge: its edge and p-curve must be there.
    pub(crate) fn push_coedge(&mut self, row: CoedgeRow) -> Result<CoedgeId, StoreError> {
        if row.edge.index() >= self.edges.curve.len() || row.pcurve.index() >= self.pcurves.len() {
            return Err(StoreError::Id);
        }
        let id = next_id(self.coedges.edge.len())?;
        let c = &mut self.coedges;
        c.edge.push(row.edge);
        c.reversed.push(row.reversed);
        c.face.push(row.face);
        c.lp.push(row.lp);
        c.next.push(row.next);
        c.prev.push(row.prev);
        c.pair.push(row.pair);
        c.pcurve.push(row.pcurve);
        c.seam_bound.push(row.seam_bound);
        Ok(CoedgeId(id))
    }

    /// Adds a loop.
    pub(crate) fn push_loop(&mut self, row: LoopRow) -> Result<LoopId, StoreError> {
        let id = next_id(self.loops.face.len())?;
        let l = &mut self.loops;
        l.face.push(row.face);
        l.first_coedge.push(row.first_coedge);
        l.next_loop.push(row.next_loop);
        l.is_outer.push(row.is_outer);
        Ok(LoopId(id))
    }

    /// Adds a face: its surface must be there.
    pub(crate) fn push_face(&mut self, row: FaceRow) -> Result<FaceId, StoreError> {
        if row.surface.index() >= self.surfaces.surface.len() {
            return Err(StoreError::Id);
        }
        let id = next_id(self.faces.surface.len())?;
        let f = &mut self.faces;
        f.surface.push(row.surface);
        f.flipped.push(row.flipped);
        f.shells.push(row.shells);
        f.first_loop.push(row.first_loop);
        f.key.push(row.key);
        f.attrs.push(row.attrs);
        Ok(FaceId(id))
    }

    /// Adds a shell.
    pub(crate) fn push_shell(&mut self, row: ShellRow) -> Result<ShellId, StoreError> {
        let id = next_id(self.shells.kind.len())?;
        let s = &mut self.shells;
        s.sides.push(row.sides);
        s.region.push(row.region);
        s.kind.push(row.kind);
        Ok(ShellId(id))
    }

    /// Adds a region.
    pub(crate) fn push_region(&mut self, row: RegionRow) -> Result<RegionId, StoreError> {
        let id = next_id(self.regions.kind.len())?;
        let r = &mut self.regions;
        r.kind.push(row.kind);
        r.outer.push(row.outer);
        r.islands.push(row.islands);
        r.container.push(row.container);
        Ok(RegionId(id))
    }

    /// The body's kind.
    pub fn kind(&self) -> BodyKind {
        self.kind
    }

    /// The number of surfaces; their ids are `0..n`.
    pub fn surface_count(&self) -> usize {
        self.surfaces.surface.len()
    }

    /// The number of space curves; their ids are `0..n`.
    pub fn curve_count(&self) -> usize {
        self.curves3.len()
    }

    /// The number of p-curves; their ids are `0..n`.
    pub fn pcurve_count(&self) -> usize {
        self.pcurves.len()
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

    /// The number of loops; their ids are `0..n`.
    pub fn loop_count(&self) -> usize {
        self.loops.face.len()
    }

    /// The number of faces; their ids are `0..n`.
    pub fn face_count(&self) -> usize {
        self.faces.surface.len()
    }

    /// The number of shells; their ids are `0..n`.
    pub fn shell_count(&self) -> usize {
        self.shells.kind.len()
    }

    /// The number of regions; their ids are `0..n`.
    pub fn region_count(&self) -> usize {
        self.regions.kind.len()
    }

    // Geometry.

    /// A surface.
    pub fn surface(&self, s: SurfId) -> &Surface {
        &self.surfaces.surface[s.index()]
    }

    /// A surface's fit: `None` for an exact form.
    pub fn fit(&self, s: SurfId) -> Option<&Fit> {
        self.surfaces.fit[s.index()].as_ref()
    }

    /// A surface's rows equal to a space curve.
    pub fn exact_rows(&self, s: SurfId) -> &ExactRows {
        &self.surfaces.exact_rows[s.index()]
    }

    /// A space curve.
    pub fn curve(&self, c: CurveId) -> &Curve3D {
        &self.curves3[c.index()]
    }

    /// A p-curve.
    pub fn pcurve(&self, p: PCurveId) -> &PCurve {
        &self.pcurves[p.index()]
    }

    // Vertices.

    /// A vertex's position.
    pub fn pos(&self, v: VertId) -> [f64; 3] {
        self.verts.pos[v.index()]
    }

    /// A vertex's tolerance.
    pub fn vert_tol(&self, v: VertId) -> f64 {
        self.verts.tol[v.index()]
    }

    /// A vertex's kind.
    pub fn vert_kind(&self, v: VertId) -> VertKind {
        self.verts.kind[v.index()]
    }

    /// A coedge leaving the vertex, `NONE` if none.
    pub fn vert_coedge(&self, v: VertId) -> CoedgeId {
        self.verts.coedge[v.index()]
    }

    /// A vertex's tag.
    pub fn vert_tag(&self, v: VertId) -> Option<VertTag> {
        self.verts.tag[v.index()]
    }

    /// A vertex's attributes.
    pub fn vert_attrs(&self, v: VertId) -> AttrSetId {
        self.verts.attrs[v.index()]
    }

    // Edges.

    /// The curve object an edge lies on (edges may share one).
    pub fn edge_curve_id(&self, e: EdgeId) -> CurveId {
        self.edges.curve[e.index()]
    }

    /// An edge's curve.
    pub fn edge_curve(&self, e: EdgeId) -> &Curve3D {
        self.curve(self.edge_curve_id(e))
    }

    /// An edge's parameter range on its curve.
    pub fn t_range(&self, e: EdgeId) -> [f64; 2] {
        self.edges.t_range[e.index()]
    }

    /// An edge's vertices, `[v_start, v_end]`.
    pub fn ends(&self, e: EdgeId) -> [VertId; 2] {
        [self.edges.v_start[e.index()], self.edges.v_end[e.index()]]
    }

    /// Whether an edge is closed: both ends one vertex.
    pub fn is_closed(&self, e: EdgeId) -> bool {
        self.edges.v_start[e.index()] == self.edges.v_end[e.index()]
    }

    /// The spans of an edge's curve inside its `t_range`, in the curve's
    /// direction.
    pub fn edge_spans(&self, e: EdgeId) -> &[Span3] {
        let c = self.edge_curve(e);
        let [t0, t1] = self.t_range(e);
        let at = |t: f64| c.knots().iter().position(|k| k.to_bits() == t.to_bits());
        match (at(t0), at(t1)) {
            (Some(i), Some(j)) if i < j => &c.spans()[i..j],
            _ => &[],
        }
    }

    /// An edge's kind.
    pub fn edge_kind(&self, e: EdgeId) -> EdgeKind {
        self.edges.kind[e.index()]
    }

    /// An edge's continuity.
    pub fn cont(&self, e: EdgeId) -> Continuity {
        self.edges.cont[e.index()]
    }

    /// An edge's origin.
    pub fn origin(&self, e: EdgeId) -> &Origin {
        &self.edges.origin[e.index()]
    }

    /// An edge's tolerance: the greatest seam bound of its coedges.
    pub fn edge_tol(&self, e: EdgeId) -> f64 {
        self.edges.tol[e.index()]
    }

    /// An edge's deviation from the true curve it stands for.
    pub fn dev(&self, e: EdgeId) -> f64 {
        self.edges.dev[e.index()]
    }

    /// Whether an edge's join zone is certified embedded.
    pub fn embedded_join(&self, e: EdgeId) -> bool {
        self.edges.embedded_join[e.index()]
    }

    /// One of an edge's coedges, `NONE` if none.
    pub fn edge_coedge(&self, e: EdgeId) -> CoedgeId {
        self.edges.coedge[e.index()]
    }

    /// An edge's tag.
    pub fn edge_tag(&self, e: EdgeId) -> Option<EdgeTag> {
        self.edges.tag[e.index()]
    }

    /// An edge's attributes.
    pub fn edge_attrs(&self, e: EdgeId) -> AttrSetId {
        self.edges.attrs[e.index()]
    }

    // Coedges.

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

    /// A coedge's loop.
    pub fn coedge_loop(&self, c: CoedgeId) -> LoopId {
        self.coedges.lp[c.index()]
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

    /// A coedge's p-curve object.
    pub fn pcurve_id(&self, c: CoedgeId) -> PCurveId {
        self.coedges.pcurve[c.index()]
    }

    /// A coedge's p-curve, over its edge's parameter.
    pub fn coedge_pcurve(&self, c: CoedgeId) -> &PCurve {
        self.pcurve(self.pcurve_id(c))
    }

    /// A coedge's seam bound: its ribbon's width.
    pub fn seam_bound(&self, c: CoedgeId) -> f64 {
        self.coedges.seam_bound[c.index()]
    }

    /// The vertex where a coedge ends (`end`) or starts, in its own
    /// direction.
    pub fn coedge_vert(&self, c: CoedgeId, end: bool) -> VertId {
        let [vs, ve] = self.ends(self.edge(c));
        if end != self.reversed(c) { ve } else { vs }
    }

    // Loops.

    /// A loop's face.
    pub fn loop_face(&self, l: LoopId) -> FaceId {
        self.loops.face[l.index()]
    }

    /// A loop's first coedge.
    pub fn first_coedge(&self, l: LoopId) -> CoedgeId {
        self.loops.first_coedge[l.index()]
    }

    /// The face's loop after this one, `NONE` after the last.
    pub fn next_loop(&self, l: LoopId) -> LoopId {
        self.loops.next_loop[l.index()]
    }

    /// Whether a loop is its face's outer loop.
    pub fn is_outer(&self, l: LoopId) -> bool {
        self.loops.is_outer[l.index()]
    }

    /// A loop's coedges, in order from its first, once round.
    pub fn loop_coedges(&self, l: LoopId) -> impl Iterator<Item = CoedgeId> + '_ {
        let first = self.first_coedge(l);
        std::iter::successors((!first.is_none()).then_some(first), move |&c| {
            let n = self.next(c);
            (n != first).then_some(n)
        })
    }

    // Faces.

    /// A face's surface object.
    pub fn face_surface_id(&self, f: FaceId) -> SurfId {
        self.faces.surface[f.index()]
    }

    /// A face's surface.
    pub fn face_surface(&self, f: FaceId) -> &Surface {
        self.surface(self.face_surface_id(f))
    }

    /// Whether a face's outward normal is against its surface's.
    pub fn flipped(&self, f: FaceId) -> bool {
        self.faces.flipped[f.index()]
    }

    /// The shell of a face's side.
    pub fn face_shell(&self, f: FaceId, side: Side) -> ShellId {
        self.faces.shells[f.index()][side.index()]
    }

    /// A face's first loop, `NONE` if none.
    pub fn first_loop(&self, f: FaceId) -> LoopId {
        self.faces.first_loop[f.index()]
    }

    /// A face's loops, from its first.
    pub fn loops(&self, f: FaceId) -> impl Iterator<Item = LoopId> + '_ {
        let first = self.first_loop(f);
        std::iter::successors((!first.is_none()).then_some(first), |&l| {
            let n = self.next_loop(l);
            (!n.is_none()).then_some(n)
        })
    }

    /// A face's key.
    pub fn key(&self, f: FaceId) -> &FaceKey {
        &self.faces.key[f.index()]
    }

    /// A face's attributes.
    pub fn face_attrs(&self, f: FaceId) -> AttrSetId {
        self.faces.attrs[f.index()]
    }

    // Shells and regions.

    /// The faces' sides a shell joins.
    pub fn shell_sides(&self, s: ShellId) -> &[(FaceId, Side)] {
        &self.shells.sides[s.index()]
    }

    /// A shell's region, `NONE` for an open shell.
    pub fn shell_region(&self, s: ShellId) -> RegionId {
        self.shells.region[s.index()]
    }

    /// A shell's kind.
    pub fn shell_kind(&self, s: ShellId) -> ShellKind {
        self.shells.kind[s.index()]
    }

    /// A region's kind.
    pub fn region_kind(&self, r: RegionId) -> RegionKind {
        self.regions.kind[r.index()]
    }

    /// A region's outer shell, `NONE` for the unbounded region.
    pub fn outer_shell(&self, r: RegionId) -> ShellId {
        self.regions.outer[r.index()]
    }

    /// A region's island shells.
    pub fn island_shells(&self, r: RegionId) -> &[ShellId] {
        &self.regions.islands[r.index()]
    }

    /// The empty region a lump's empty islands lie in, `NONE` if none.
    pub fn container(&self, r: RegionId) -> RegionId {
        self.regions.container[r.index()]
    }

    // Records, names, tolerances and status.

    /// The contact records.
    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    /// The provenance.
    pub fn prov(&self) -> &Provenance {
        &self.prov
    }

    /// The name tables.
    pub fn names(&self) -> &NameTables {
        &self.names
    }

    /// The tolerances.
    pub fn eps(&self) -> &Tolerances {
        &self.eps
    }

    /// The status of the operation that made the body.
    pub fn status(&self) -> &BodyStatus {
        &self.status
    }

    /// The tolerances, to raise `max_tol`.
    pub(crate) fn eps_mut(&mut self) -> &mut Tolerances {
        &mut self.eps
    }

    /// The status, to report into.
    pub(crate) fn status_mut(&mut self) -> &mut BodyStatus {
        &mut self.status
    }
}

#[cfg(test)]
mod tests;
