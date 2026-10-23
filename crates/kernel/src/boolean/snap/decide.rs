//! After a snap, the zone is one surface: what the records there decide,
//! and what they read.
//!
//! **The coincidence** ([`zone_labels`]). Inside the rim the two sheets
//! lie on one surface, a tie decided by the perturbation, which moves the
//! first operand `A` along its outward side times `σ_A` (`+1` for a union,
//! `−1` for an intersection or a difference):
//!
//! | cluster | `σ_A = −1` | `σ_A = +1` |
//! |---|---|---|
//! | Contact (opposed) | the gap opens: both sheets outside the other | fused: both inside |
//! | Near (co-oriented) | `A` inside `B`, `B` outside `A` | `A` outside `B`, `B` inside `A` |
//!
//! So opposed sheets always get one label and co-oriented ones
//! complementary labels.
//!
//! **Ties on the rim** ([`tie_side`]). Every tie between the rim and the
//! target `T` (a branch vertex, where the gap changes sign, and a rim
//! vertex on a target edge) is decided by the same motion, so to the same
//! side of `T`: the snapped side's label in the table (outside: the gap
//! positive).
//!
//! **Rim records** ([`RimRecords`]). A rim edge off the target edges (along
//! box sides, or along an edge of the snapped side `S`) against a target
//! face beside it reads the sign of the `S − T` gap on its stretch of the
//! rim (positive outside `T`), never a lifted value or a band: the sign
//! of the rim's one-signed cells there ([`super::rim::PieceGap`]), outside every
//! root cluster the corner rule hands to an anchor ([`Corner::spans`]).
//! The sign is constant between the branch vertices, so a stretch whose
//! cells are all in root clusters reads it along the rim from its
//! neighbours: past a branch vertex of degree `d` (`+1` from inside to
//! outside), the sign `d` gives; failing both, its own cells in clusters
//! (a proxy at the band's scale); failing those (a piece on the target
//! surface exactly), the tie side. A rim vertex off the target edges reads
//! the sign of its rim edges.
//!
//! **Corner records.** The rim's ties are its branch vertices (kept by the
//! corner rule, or an anchor carrying a root cluster of net `±1`) and its
//! vertices on target edges: the target's own vertices, rim vertices on a
//! target edge, a target edge crossing the rim or an edge of `S`, splits
//! at an anchor, `SnapAlong` bracket ends, every vertex the corner rule
//! puts on an anchor or a target edge, and every end of a rim edge that
//! runs along a target edge. Each is decided to the tie side. A rim edge
//! whose sign disagrees with it gets a **corner record** at each such end,
//! of degree the sign change along the rim there; so a branch vertex gives
//! its one record, of its degree, to the one rim edge beside it that
//! disagrees, and a rim edge between two ties disagreeing gets two
//! opposite records. A rim edge's crossing count with a target face beside
//! it, [`RimRecords::x21`], is the sum of its corner records. Signs that
//! change at a vertex that isn't a tie, or not as a branch vertex's degree
//! says, are a contradiction of the rim's certificates
//! ([`DecideError::Vertex`]).
//!
//! **Lifted reads** ([`Lifted`]). Every record involving an element on `T`
//! ([`OnT`]) reads it at its place on `T`: an edge as `S_T∘p_T`, its
//! coedge's p-curve on a snapped face carried by that target face's plane
//! (exact control points, the weights kept: a line where the p-curve is
//! one), a vertex as `S_T(uv)` at its loop corner there. Flat targets
//! only: each target face a plane chart.
//!
//! **Same-surface band** ([`Zone::height_rule`]). A height record whose
//! two elements are both on `T` is decided under the same-surface band
//! ([`Band::same_surface`]) of the larger of their bands, at every rung:
//! a flat value compared with it exactly, beyond it the value's sign,
//! within it a tie decided by the perturbation's orders ([`decide`](varde_decide::num::decide::decide)), a
//! proxy at the band's scale. Never for a rim element against a target
//! face beside it (the gap sign above).
//!
//! **Order.** Loops and pieces in the rim's numbering (from geometry),
//! rim edges along each piece by their exact place on it, surfaces by
//! their first target face; ids break exact ties only.
//!
//! **Work.** Charged on the operation's budget in passes: the lookups
//! the passes read (one item, a unit per entry and per target face
//! searched), the pieces (an item per piece, a unit per rim edge), the
//! loops (an item per loop, a unit per rim edge and surface), the lifted
//! edges and vertices (an item each, a unit per control point).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use varde_decide::ids::{FaceId, VertId};
use varde_decide::num::decide::{Band, Sign};
use varde_decide::records::keys::Operand;

use super::cluster::Tangency;
use super::corner::{Corner, Placement, larger};
use super::rewrite::{
    Chart, EdgeClass, EdgeCurve, Geometry, NewVert, Rewrite, SnapEdge, SnapFace, SnapVert,
    TargetFace, chart,
};
use super::rim::{Element, Gap, Implicit, Rim};
use super::stale::OnT;
use crate::geom::pcurve::PCurve;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

type Q = Rational;
/// A point in space, exactly.
type P3 = [Q; 3];
/// A point of a face's global UV, exactly.
type P2 = [Q; 2];

// ---------------------------------------------------------------------
// The coincidence.

/// A sheet's side of the other operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    Inside,
    Outside,
}

/// The labels of the two sheets inside a zone (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneLabels {
    /// `A`'s sheet against `B`.
    pub a: Label,
    /// `B`'s sheet against `A`.
    pub b: Label,
}

/// The labels of a zone of a cluster of `kind` under the operation's
/// `σ_A` (module docs).
pub fn zone_labels(kind: Tangency, sigma_a: Sign) -> ZoneLabels {
    use Label::{Inside, Outside};
    let (a, b) = match (kind, sigma_a) {
        (Tangency::Contact, Sign::Neg) => (Outside, Outside),
        (Tangency::Contact, Sign::Pos) => (Inside, Inside),
        (Tangency::Near, Sign::Neg) => (Inside, Outside),
        (Tangency::Near, Sign::Pos) => (Outside, Inside),
    };
    ZoneLabels { a, b }
}

/// The side of the target every tie on the rim is decided to, as the sign
/// of the `S − T` gap (`Pos`: outside): the snapped side's label (module
/// docs).
pub fn tie_side(kind: Tangency, sigma_a: Sign, snapped: Operand) -> Sign {
    let l = zone_labels(kind, sigma_a);
    match (snapped, l) {
        (
            Operand::A,
            ZoneLabels {
                a: Label::Outside, ..
            },
        )
        | (
            Operand::B,
            ZoneLabels {
                b: Label::Outside, ..
            },
        ) => Sign::Pos,
        _ => Sign::Neg,
    }
}

/// An element a record in the zone reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneEl {
    /// A vertex of the rewrite.
    Vert(SnapVert),
    /// An edge of the rewrite.
    Edge(SnapEdge),
    /// A face of the rewrite.
    Face(SnapFace),
    /// An operand's edge or vertex the rewrite doesn't hold.
    Operand(Element),
    /// A face of the target operand.
    TargetFace(FaceId),
    /// Any other element: never on the target.
    Off,
}

/// How a height-type record between two elements is decided
/// ([`Zone::height_rule`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeightRule {
    /// A rim element against a target face beside it: this gap sign
    /// (`S − T`, positive outside the target), the record's order 0.
    RimGap(Sign),
    /// Both elements on the target: this same-surface band, the larger
    /// of the two elements' ([`Band::same_surface`], the band [`decide`]
    /// takes).
    ///
    /// [`decide`]: varde_decide::num::decide::decide
    SameSurface(Band),
    /// The ordinary rules.
    Ordinary,
}

// ---------------------------------------------------------------------
// Rim records.

/// Which end of a rim edge, along the rim (the region on its left).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RimEnd {
    Start,
    End,
}

/// A corner record of a rim edge at a tie (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CornerRecord {
    /// The tie.
    pub vert: SnapVert,
    /// Which end of the rim edge it is.
    pub end: RimEnd,
    /// The sign change there along the rim, `+1` from inside the target to
    /// outside.
    pub degree: i8,
}

/// A rim edge's record against the target faces of one surface beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct RimRecord {
    /// The rim edge.
    pub edge: SnapEdge,
    /// The target faces beside it on that surface, sorted.
    pub targets: Vec<FaceId>,
    /// The gap's sign on it (`Pos`: outside the target).
    pub sign: Sign,
    /// Its corner records, along the rim.
    pub corners: Vec<CornerRecord>,
}

/// A rim vertex's sign against the target faces of one surface beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct RimVertRecord {
    /// The vertex.
    pub vert: SnapVert,
    /// The target faces beside its rim edges on that surface, sorted.
    pub targets: Vec<FaceId>,
    /// The gap's sign there: the tie side for a tie.
    pub sign: Sign,
    /// Whether it is a tie (a branch vertex or on a target edge).
    pub tie: bool,
}

/// A branch vertex of the rim: where the corner record of its degree went.
#[derive(Clone, Debug, PartialEq)]
pub struct BranchRecord {
    /// The vertex: a branch vertex the corner rule keeps, or an anchor
    /// carrying a root cluster (or the rim vertex joined to it).
    pub vert: SnapVert,
    /// The target faces of its surface beside it, sorted.
    pub targets: Vec<FaceId>,
    /// Its degree, `+1` from inside the target to outside along the rim.
    pub degree: i8,
    /// The rim edge holding its corner record, if one beside it disagrees
    /// with the tie side.
    pub edge: Option<SnapEdge>,
}

/// A cluster's rim records (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct RimRecords {
    /// The tie side.
    pub side: Sign,
    /// The rim edges off the target edges, along the loops, each per
    /// surface.
    pub edges: Vec<RimRecord>,
    /// The rim vertices, along the loops, each per surface.
    pub verts: Vec<RimVertRecord>,
    /// The branch vertices, along the loops.
    pub branches: Vec<BranchRecord>,
}

impl RimRecords {
    /// Rim edge `k`'s record against target face `face`.
    pub fn edge(&self, k: SnapEdge, face: FaceId) -> Option<&RimRecord> {
        self.edges
            .iter()
            .find(|r| r.edge == k && r.targets.contains(&face))
    }

    /// Rim vertex `v`'s record against target face `face`.
    pub fn vert(&self, v: SnapVert, face: FaceId) -> Option<&RimVertRecord> {
        self.verts
            .iter()
            .find(|r| r.vert == v && r.targets.contains(&face))
    }

    /// The crossing count of rim edge `k` with target face `face`: the sum
    /// of its corner records (`0` without a record).
    pub fn x21(&self, k: SnapEdge, face: FaceId) -> i32 {
        self.edge(k, face)
            .map_or(0, |r| r.corners.iter().map(|c| i32::from(c.degree)).sum())
    }
}

// ---------------------------------------------------------------------
// Lifted reads.

/// An edge on the target read at its place there: `S_T∘p_T` (module
/// docs), over the edge's parameter as its p-curve.
#[derive(Clone, Debug, PartialEq)]
pub struct LiftedCurve {
    /// The target face.
    pub face: FaceId,
    /// The knots, as the p-curve's.
    pub knots: Vec<f64>,
    /// The spans: control points in space and their weights, exactly.
    pub spans: Vec<[(P3, Q); 3]>,
}

impl LiftedCurve {
    /// Its point at the first knot.
    pub fn first(&self) -> &P3 {
        &self.spans[0][0].0
    }

    /// Its point at the last knot.
    pub fn last(&self) -> &P3 {
        &self.spans[self.spans.len() - 1][2].0
    }

    /// Its point at parameter `t`, exactly; `None` outside its knots.
    pub fn point(&self, t: &Q) -> Option<P3> {
        let k = (0..self.spans.len()).find(|&k| {
            let lo = Q::try_from(self.knots[k]).ok();
            let hi = Q::try_from(self.knots[k + 1]).ok();
            matches!((lo, hi), (Some(lo), Some(hi)) if lo <= *t && *t <= hi)
        })?;
        let lo = Q::try_from(self.knots[k]).ok()?;
        let hi = Q::try_from(self.knots[k + 1]).ok()?;
        let s = (t - &lo) / (hi - lo);
        let r = Q::from(1u32) - &s;
        let b = [&r * &r, Q::from(2u32) * &r * &s, &s * &s];
        let span = &self.spans[k];
        let mut num: P3 = [Q::from(0u32), Q::from(0u32), Q::from(0u32)];
        let mut den = Q::from(0u32);
        for (bi, (p, w)) in b.iter().zip(span) {
            let f = bi * w;
            for (n, x) in num.iter_mut().zip(p) {
                *n += &f * x;
            }
            den += f;
        }
        Some(num.map(|n| n / &den))
    }
}

/// The elements on the target read at their places there (module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lifted {
    /// The edges, by key, then target face.
    pub edges: Vec<(SnapEdge, LiftedCurve)>,
    /// The vertices, by key, then target face.
    pub verts: Vec<(SnapVert, FaceId, P3)>,
}

impl Lifted {
    /// Edge `k` read on target face `face`.
    pub fn edge(&self, k: SnapEdge, face: FaceId) -> Option<&LiftedCurve> {
        self.edges
            .iter()
            .find(|(e, c)| *e == k && c.face == face)
            .map(|x| &x.1)
    }

    /// Vertex `v` read on target face `face`.
    pub fn vert(&self, v: SnapVert, face: FaceId) -> Option<&P3> {
        self.verts
            .iter()
            .find(|(w, f, _)| *w == v && *f == face)
            .map(|x| &x.2)
    }
}

// ---------------------------------------------------------------------
// The zone.

/// What [`decide_zone`] reads besides the snap's results.
#[derive(Clone, Copy)]
pub struct DecideInput<'a> {
    /// The cluster's kind.
    pub kind: Tangency,
    /// The operation's `σ_A`: `Pos` for a union, `Neg` otherwise.
    pub sigma_a: Sign,
    /// The snapped side `S`.
    pub snapped: Operand,
    /// The target faces the cluster reaches, each with its surface (the
    /// rim's, [`super::rim::RimInput::targets`]).
    pub targets: &'a [(FaceId, Implicit)],
    /// The target faces' charts, each a plane.
    pub faces: &'a [TargetFace<'a>],
}

/// What the records of a snapped zone decide and read (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// The sheets' labels inside the zone.
    pub labels: ZoneLabels,
    /// The rim's records.
    pub rim: RimRecords,
    /// The elements on the target, read there.
    pub lifted: Lifted,
    /// The elements of the rewrite on the target.
    on: OnT,
    /// The target's edges and vertices in the window, sorted.
    window: Vec<Element>,
    /// The target faces the cluster reaches, sorted.
    targets: Vec<FaceId>,
    /// The target operand.
    t_op: Operand,
}

impl Zone {
    /// The elements of the rewrite on the target.
    pub fn on(&self) -> &OnT {
        &self.on
    }

    /// Whether `x` is on the target: an element of the rewrite by
    /// [`OnT`], the target's own elements in the zone by construction.
    pub fn on_t(&self, x: ZoneEl) -> bool {
        match x {
            ZoneEl::Vert(SnapVert::Vert(o, v)) if o == self.t_op => {
                self.on.vert(SnapVert::Vert(o, v))
                    || self.window.binary_search(&Element::Vert(o, v)).is_ok()
            }
            ZoneEl::Vert(v) => self.on.vert(v),
            ZoneEl::Edge(e) => self.on.edge(e),
            ZoneEl::Face(f) => self.on.face(f),
            ZoneEl::Operand(el) => self.window.binary_search(&el).is_ok(),
            ZoneEl::TargetFace(f) => self.targets.binary_search(&f).is_ok(),
            ZoneEl::Off => false,
        }
    }

    /// The gap sign of rim element `x` (a rim edge, or a rim vertex not a
    /// tie) against target face `f` beside it.
    fn rim_gap(&self, x: ZoneEl, f: FaceId) -> Option<Sign> {
        match x {
            ZoneEl::Edge(e) => self.rim.edge(e, f).map(|r| r.sign),
            ZoneEl::Vert(v) => self.rim.vert(v, f).filter(|r| !r.tie).map(|r| r.sign),
            _ => None,
        }
    }

    /// How a height-type record between `x` and `y` is decided (module
    /// docs); `band` gives an element's band (an edge's `β_g`, a vertex's
    /// tolerance plus `ρ_op`: [`OpBands::band`]). `None` where both are on
    /// the target and the larger band isn't finite and `≥ 0`.
    ///
    /// [`OpBands::band`]: varde_decide::records::near::OpBands::band
    pub fn height_rule(
        &self,
        x: ZoneEl,
        y: ZoneEl,
        band: &dyn Fn(ZoneEl) -> f64,
    ) -> Option<HeightRule> {
        for (r, t) in [(x, y), (y, x)] {
            if let ZoneEl::TargetFace(f) = t
                && let Some(s) = self.rim_gap(r, f)
            {
                return Some(HeightRule::RimGap(s));
            }
        }
        if self.on_t(x) && self.on_t(y) {
            // A flat value is exact: no error target.
            let beta = larger(band(x), band(y));
            Band::same_surface(beta, 0.0).map(HeightRule::SameSurface)
        } else {
            Some(HeightRule::Ordinary)
        }
    }
}

/// Why a zone can't be decided.
#[derive(Clone, Debug, PartialEq)]
pub enum DecideError {
    /// The rewrite's edges along this rim piece don't run along it end to
    /// end, or don't join the next piece along its loop.
    Loop(u32),
    /// This edge has no geometry.
    Edge(SnapEdge),
    /// This target face has no surface given.
    Surface(FaceId),
    /// This rim edge's one-signed cells (outside the anchors' root
    /// clusters) disagree: a sign change without a branch vertex.
    Inconsistent(SnapEdge),
    /// The gap's signs beside this rim vertex change where it isn't a tie,
    /// or not as its degree says.
    Vertex(SnapVert),
    /// The corner rule's branch vertex at this index isn't a vertex of the
    /// rim, or shares one with another.
    Branch(u32),
    /// This anchor carries a branch but no rim vertex is it or joins it.
    Anchor(Operand, VertId),
    /// This target face isn't described, or isn't a plane.
    Chart(FaceId),
    /// This edge on the target has no p-curve on a snapped face, or a value
    /// isn't finite.
    Lift(SnapEdge),
    /// This vertex on the target has no loop corner on a snapped face, or
    /// two that differ.
    LiftVert(SnapVert),
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

impl From<KernelError> for DecideError {
    fn from(e: KernelError) -> Self {
        DecideError::OutOfBudget(e)
    }
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, DecideError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<DecideError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => DecideError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<DecideError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

fn fail<T>(e: DecideError) -> Result<T, Stop<DecideError>> {
    Err(Stop::Failed(e))
}

/// The zone of a snapped cluster (module docs): `rim`, `corner`, `rewrite`
/// and its `geometry` the snap's, `on` the rewrite's elements on the
/// target ([`super::stale::on_t`]), kept by the zone; charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn decide_zone(
    rim: &Rim,
    corner: &Corner,
    rewrite: &Rewrite,
    geometry: &Geometry,
    on: OnT,
    input: &DecideInput<'_>,
    work: &mut Work,
) -> Result<Zone, DecideError> {
    let side = tie_side(input.kind, input.sigma_a, input.snapped);
    let t_op = input.snapped.other();
    // The lookups the passes below build before their items: the rim's
    // (surfaces by target face, gaps, edge classes, vertices, rim edges by
    // piece), the lifted reads' (edges by key, ends, a chart per target
    // face reached by an on-`T` coedge, found among the target faces) and
    // the zone's own (window, targets).
    let coedges: usize = geometry.edges.iter().map(|e| e.coedges.len()).sum();
    let setup = [
        2 * input.targets.len(),
        rim.gaps.len(),
        2 * geometry.edges.len(),
        rewrite.verts.len(),
        2 * rewrite.edges.len(),
        coedges,
        input.faces.len().saturating_mul(input.faces.len()),
        corner.window.len(),
    ]
    .iter()
    .fold(0u64, |acc, &n| acc.saturating_add(n as u64));
    run(work, "snap decide: setup", &[()], |_, l| units(l, setup))?;
    let rim_records = rim_records(rim, corner, rewrite, geometry, input, side, work)?;
    let lifted = lift(rewrite, geometry, &on, input, work)?;
    let mut window: Vec<Element> = corner
        .window
        .iter()
        .copied()
        .filter(|e| match e {
            Element::Edge(o, _) | Element::Vert(o, _) => *o == t_op,
        })
        .collect();
    window.sort_unstable();
    window.dedup();
    let mut targets: Vec<FaceId> = input.targets.iter().map(|t| t.0).collect();
    targets.sort_unstable();
    targets.dedup();
    Ok(Zone {
        labels: zone_labels(input.kind, input.sigma_a),
        rim: rim_records,
        lifted,
        on,
        window,
        targets,
        t_op,
    })
}

// ---------------------------------------------------------------------
// Walking a loop.

/// A rim edge as a loop's walk reads it against one surface.
#[derive(Clone, Debug)]
struct WalkPart {
    /// Whether it is a rim edge off the target edges with a gap against the
    /// surface.
    rim: bool,
    /// Its sign from its own cells outside every anchor's root cluster.
    own: Option<Sign>,
    /// Its sign from any of its one-signed cells: the proxy.
    fallback: Option<Sign>,
}

/// A rim vertex as a loop's walk reads it: the start of the part of its
/// index, the end of the one before.
#[derive(Clone, Copy, Debug)]
struct WalkVert {
    /// Whether it is a tie.
    tie: bool,
    /// Its degree against the surface, a branch vertex's.
    degree: Option<i8>,
}

/// A walk's result: each part's sign and the corner records, `(part, end,
/// degree)`.
#[derive(Debug, PartialEq)]
struct Walk {
    signs: Vec<Option<Sign>>,
    records: Vec<(usize, RimEnd, i8)>,
}

/// The sign after a change of degree `d`.
fn after(d: i8) -> Sign {
    if d > 0 { Sign::Pos } else { Sign::Neg }
}

/// The degree of a change from the opposite of `to` to `to`.
fn degree_to(to: Sign) -> i8 {
    to.as_i8()
}

/// The signs and corner records along a closed loop of `parts` (module
/// docs), every tie decided to `side`; `Err(i)` where the signs beside
/// vertex `i` contradict it.
fn walk(parts: &[WalkPart], verts: &[WalkVert], side: Sign) -> Result<Walk, usize> {
    let n = parts.len();
    debug_assert_eq!(n, verts.len());
    let mut sign: Vec<Option<Sign>> = parts
        .iter()
        .map(|p| if p.rim { p.own } else { None })
        .collect();
    // Along the loop, then back, twice round each way: across a vertex
    // the sign carries, a branch vertex sets it.
    for _ in 0..2 {
        for i in 0..n {
            if !parts[i].rim || sign[i].is_some() {
                continue;
            }
            let p = (i + n - 1) % n;
            if let Some(d) = verts[i].degree {
                sign[i] = Some(after(d));
            } else if p != i && parts[p].rim {
                sign[i] = sign[p];
            }
        }
    }
    for _ in 0..2 {
        for i in (0..n).rev() {
            if !parts[i].rim || sign[i].is_some() {
                continue;
            }
            let e = (i + 1) % n;
            if let Some(d) = verts[e].degree {
                sign[i] = Some(after(-d));
            } else if e != i && parts[e].rim {
                sign[i] = sign[e];
            }
        }
    }
    for (s, p) in sign.iter_mut().zip(parts) {
        if p.rim && s.is_none() {
            *s = Some(p.fallback.unwrap_or(side));
        }
    }
    let at = |i: usize| if parts[i].rim { sign[i] } else { None };
    for (i, v) in verts.iter().enumerate() {
        let (a, b) = (at((i + n - 1) % n), at(i));
        let ok = match v.degree {
            Some(d) => a.is_none_or(|a| a == after(-d)) && b.is_none_or(|b| b == after(d)),
            None => v.tie || a.is_none() || b.is_none() || a == b,
        };
        if !ok {
            return Err(i);
        }
    }
    let mut records = Vec::new();
    for i in 0..n {
        let Some(s) = at(i) else { continue };
        if s == side {
            continue;
        }
        if verts[i].tie {
            records.push((i, RimEnd::Start, degree_to(s)));
        }
        if verts[(i + 1) % n].tie {
            records.push((i, RimEnd::End, degree_to(side)));
        }
    }
    Ok(Walk {
        signs: sign,
        records,
    })
}

// ---------------------------------------------------------------------
// The rim's records.

/// A rim edge along its piece.
#[derive(Clone, Debug)]
struct Part {
    edge: SnapEdge,
    piece: u32,
    /// Its stretch of the piece's parameter, increasing.
    lo: Q,
    hi: Q,
    /// Its vertices at `lo` and at `hi`.
    first: SnapVert,
    last: SnapVert,
    class: EdgeClass,
}

impl Part {
    /// Whether it is a rim element off the target edges.
    fn off_target(&self) -> bool {
        matches!(self.class, EdgeClass::Rim | EdgeClass::AlongRim)
    }
}

fn sub2(a: &P2, b: &P2) -> P2 {
    [&a[0] - &b[0], &a[1] - &b[1]]
}

fn dot2(a: &P2, b: &P2) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1]
}

/// Whether vertex `v` is on a target edge (module docs).
fn on_target_edge(v: &NewVert, t_op: Operand) -> bool {
    let by_key = match v.key {
        SnapVert::Vert(o, _) => o == t_op,
        SnapVert::RimTarget { .. }
        | SnapVert::Branch { .. }
        | SnapVert::Bracket { .. }
        | SnapVert::Cross { .. }
        | SnapVert::CutRim { .. } => true,
        SnapVert::Split { at, .. } => at.0 == t_op,
        SnapVert::Rim { .. } | SnapVert::RimBox { .. } => false,
    };
    by_key
        || matches!(
            v.moved.to,
            Placement::Vert(..) | Placement::Edge { .. } | Placement::Branch(_)
        )
}

/// A position along a loop: the piece's place in it plus the parameter.
fn loop_x(place: usize, t: &Q) -> Q {
    Q::from(place) + t
}

/// The stretches of the loop of `n` pieces from `a` to `b` (cyclic).
fn stretches(a: Q, b: Q, n: usize) -> Vec<[Q; 2]> {
    if b > a {
        vec![[a, b]]
    } else {
        vec![[a, Q::from(n)], [Q::from(0u32), b]]
    }
}

/// The rim's records (module docs).
fn rim_records(
    rim: &Rim,
    corner: &Corner,
    rewrite: &Rewrite,
    geometry: &Geometry,
    input: &DecideInput<'_>,
    side: Sign,
    work: &mut Work,
) -> Result<RimRecords, DecideError> {
    let t_op = input.snapped.other();
    // Surfaces: the target faces' implicits, equal ones one surface, in
    // the order of their first face.
    let mut by_face: Vec<(FaceId, Implicit)> = input.targets.to_vec();
    by_face.sort_by_key(|t| t.0);
    let mut surfs: Vec<Implicit> = Vec::new();
    let mut face_surf: BTreeMap<FaceId, usize> = BTreeMap::new();
    for (f, imp) in &by_face {
        let s = surfs.iter().position(|x| x == imp).unwrap_or_else(|| {
            surfs.push(*imp);
            surfs.len() - 1
        });
        face_surf.insert(*f, s);
    }
    let surface_of = |ts: &[FaceId]| -> Result<Option<usize>, DecideError> {
        match ts.first() {
            None => Ok(None),
            Some(f) => face_surf
                .get(f)
                .copied()
                .map(Some)
                .ok_or(DecideError::Surface(*f)),
        }
    };
    let mut gap_at: BTreeMap<(u32, usize), usize> = BTreeMap::new();
    for (i, g) in rim.gaps.iter().enumerate() {
        if let Some(s) = surface_of(&g.targets)? {
            gap_at.insert((g.piece, s), i);
        }
    }
    let class: BTreeMap<SnapEdge, EdgeClass> =
        geometry.edges.iter().map(|e| (e.key, e.class)).collect();
    let verts: BTreeMap<SnapVert, &NewVert> = rewrite.verts.iter().map(|v| (v.key, v)).collect();

    // Each piece's rim edges along it.
    let mut on_piece: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, e) in rewrite.edges.iter().enumerate() {
        if let Some(k) = e.rim {
            on_piece.entry(k).or_default().push(i);
        }
    }
    let pieces: Vec<u32> = (0..rim.pieces.len() as u32).collect();
    let parts = run(work, "snap decide: pieces", &pieces, |&k, l| {
        let mine = on_piece.get(&k).map_or(&[][..], |v| &v[..]);
        units(l, 1 + mine.len() as u64)?;
        let piece = &rim.pieces[k as usize];
        let d = sub2(&piece.ends[1], &piece.ends[0]);
        let dd = dot2(&d, &d);
        if dd == 0u32 {
            return fail(DecideError::Loop(k));
        }
        let frac = |v: SnapVert| -> Option<Q> {
            let p = &verts.get(&v)?.at.iter().find(|x| x.0 == piece.face)?.1;
            Some(dot2(&sub2(p, &piece.ends[0]), &d) / &dd)
        };
        let mut out: Vec<Part> = Vec::new();
        for &i in mine {
            let e = &rewrite.edges[i];
            let c = *class
                .get(&e.key)
                .ok_or(Stop::Failed(DecideError::Edge(e.key)))?;
            if c == EdgeClass::Zero {
                continue;
            }
            let (Some(ta), Some(tb)) = (frac(e.ends[0]), frac(e.ends[1])) else {
                return fail(DecideError::Loop(k));
            };
            let part = match ta.cmp(&tb) {
                Ordering::Less => (ta, tb, e.ends[0], e.ends[1]),
                Ordering::Greater => (tb, ta, e.ends[1], e.ends[0]),
                Ordering::Equal => return fail(DecideError::Loop(k)),
            };
            out.push(Part {
                edge: e.key,
                piece: k,
                lo: part.0,
                hi: part.1,
                first: part.2,
                last: part.3,
                class: c,
            });
        }
        out.sort_by(|a, b| a.lo.cmp(&b.lo));
        // End to end along the piece.
        let joined = out
            .windows(2)
            .all(|w| w[0].hi == w[1].lo && w[0].last == w[1].first);
        let ends =
            out.first().is_some_and(|p| p.lo == 0u32) && out.last().is_some_and(|p| p.hi == 1u32);
        if !(joined && ends) {
            return fail(DecideError::Loop(k));
        }
        Ok(out)
    })?;

    let loops: Vec<usize> = (0..rim.loops.len()).collect();
    let per_loop = run(work, "snap decide: loops", &loops, |&li, l| {
        let lp = &rim.loops[li];
        let n_pieces = lp.len();
        let place: BTreeMap<u32, usize> = lp.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let ps: Vec<&Part> = lp.iter().flat_map(|&k| &parts[k as usize]).collect();
        let n = ps.len();
        for i in 0..n {
            if ps[i].last != ps[(i + 1) % n].first {
                return fail(DecideError::Loop(ps[i].piece));
            }
        }
        let xs: Vec<Q> = ps.iter().map(|p| loop_x(place[&p.piece], &p.lo)).collect();
        let vert_at = |x: &Q| -> Option<usize> {
            let x = if *x == n_pieces {
                Q::from(0u32)
            } else {
                x.clone()
            };
            xs.iter().position(|y| *y == x)
        };
        let tie: Vec<bool> = (0..n)
            .map(|i| {
                let v = ps[i].first;
                let p = (i + n - 1) % n;
                !ps[i].off_target()
                    || !ps[p].off_target()
                    || verts.get(&v).is_some_and(|nv| on_target_edge(nv, t_op))
            })
            .collect();
        let mut surfaces: BTreeSet<usize> = BTreeSet::new();
        for p in &ps {
            surfaces.extend(
                gap_at
                    .range((p.piece, 0)..=(p.piece, usize::MAX))
                    .map(|x| x.0.1),
            );
        }
        let mut out = LoopOut::default();
        for s in surfaces {
            units(l, n as u64)?;
            let mut degree: Vec<Option<(i8, Vec<FaceId>)>> = vec![None; n];
            let mut set = |i: usize, d: i8, ts: &[FaceId], err: DecideError| {
                if degree[i].is_some() {
                    return Err(Stop::Failed(err));
                }
                let mut ts = ts.to_vec();
                ts.sort_unstable();
                degree[i] = Some((d, ts));
                Ok(())
            };
            // The branch vertices the corner rule keeps.
            for (bi, b) in corner.branches.iter().enumerate() {
                let Some(&pl) = place.get(&b.piece) else {
                    continue;
                };
                if surface_of(&b.targets).map_err(Stop::Failed)? != Some(s) {
                    continue;
                }
                let i = vert_at(&loop_x(pl, &b.at))
                    .ok_or(Stop::Failed(DecideError::Branch(bi as u32)))?;
                let d = if b.rising { 1 } else { -1 };
                set(i, d, &b.targets, DecideError::Branch(bi as u32))?;
            }
            // The anchors' root clusters on this surface: every one hides its
            // cells; one of net `±1` makes its anchor a branch vertex.
            let mut hidden: Vec<[Q; 2]> = Vec::new();
            for sp in &corner.spans {
                let (Some(&pa), Some(&pb)) = (place.get(&sp.from.0), place.get(&sp.to.0)) else {
                    continue;
                };
                if surface_of(&sp.targets).map_err(Stop::Failed)? != Some(s) {
                    continue;
                }
                let a = loop_x(pa, &sp.from.1.rational());
                let b = loop_x(pb, &sp.to.1.rational());
                let st = stretches(a.clone(), b, n_pieces);
                if let Some(d @ (1 | -1)) = sp.net {
                    let (o, w) = sp.anchor;
                    let is_anchor = |v: SnapVert| {
                        v == SnapVert::Vert(o, w)
                            || verts
                                .get(&v)
                                .is_some_and(|nv| nv.moved.to == Placement::Vert(o, w))
                    };
                    let dist = |x: &Q| {
                        let d = x - &a;
                        if d < 0u32 { d + Q::from(n_pieces) } else { d }
                    };
                    let i = (0..n)
                        .filter(|&i| {
                            is_anchor(ps[i].first)
                                && st.iter().any(|[lo, hi]| *lo <= xs[i] && xs[i] <= *hi)
                        })
                        .min_by(|&i, &j| dist(&xs[i]).cmp(&dist(&xs[j])))
                        .ok_or(Stop::Failed(DecideError::Anchor(o, w)))?;
                    set(i, d, &sp.targets, DecideError::Vertex(ps[i].first))?;
                }
                hidden.extend(st);
            }
            // The parts against this surface.
            let mut wp = Vec::with_capacity(n);
            for p in &ps {
                let g = gap_at.get(&(p.piece, s)).map(|&g| &rim.gaps[g]);
                let Some(g) = g.filter(|_| p.off_target()) else {
                    wp.push(WalkPart {
                        rim: false,
                        own: None,
                        fallback: None,
                    });
                    continue;
                };
                let pl = place[&p.piece];
                let (mut own, mut fallback, mut zero) = (BTreeSet::new(), None, false);
                for ([c0, c1], gap) in &g.cells {
                    let (c0, c1) = (c0.rational(), c1.rational());
                    if !(c1 > p.lo && c0 < p.hi) {
                        continue;
                    }
                    let sign = match gap {
                        Gap::Positive => Sign::Pos,
                        Gap::Negative => Sign::Neg,
                        Gap::Zero => {
                            zero = true;
                            continue;
                        }
                        Gap::Cluster => continue,
                    };
                    fallback.get_or_insert(sign);
                    let (x0, x1) = (loop_x(pl, &c0), loop_x(pl, &c1));
                    if !hidden.iter().any(|[a, b]| x1 > *a && x0 < *b) {
                        own.insert(sign);
                    }
                }
                if own.len() > 1 {
                    return fail(DecideError::Inconsistent(p.edge));
                }
                let own = if zero {
                    Some(side)
                } else {
                    own.first().copied()
                };
                wp.push(WalkPart {
                    rim: true,
                    own,
                    fallback,
                });
            }
            let wv: Vec<WalkVert> = (0..n)
                .map(|i| WalkVert {
                    tie: tie[i] || degree[i].is_some(),
                    degree: degree[i].as_ref().map(|d| d.0),
                })
                .collect();
            let w =
                walk(&wp, &wv, side).map_err(|i| Stop::Failed(DecideError::Vertex(ps[i].first)))?;
            let targets_of = |i: usize| -> Vec<FaceId> {
                gap_at
                    .get(&(ps[i].piece, s))
                    .map(|&g| rim.gaps[g].targets.clone())
                    .unwrap_or_default()
            };
            for i in 0..n {
                let Some(sign) = w.signs[i] else { continue };
                let corners = w
                    .records
                    .iter()
                    .filter(|r| r.0 == i)
                    .map(|&(_, end, degree)| CornerRecord {
                        vert: match end {
                            RimEnd::Start => ps[i].first,
                            RimEnd::End => ps[i].last,
                        },
                        end,
                        degree,
                    })
                    .collect();
                out.edges.push(RimRecord {
                    edge: ps[i].edge,
                    targets: targets_of(i),
                    sign,
                    corners,
                });
            }
            for i in 0..n {
                let p = (i + n - 1) % n;
                if !(wp[p].rim || wp[i].rim) {
                    continue;
                }
                let tie = wv[i].tie;
                let beside = |j: usize| if wp[j].rim { w.signs[j] } else { None };
                let Some(sign) = (if tie {
                    Some(side)
                } else {
                    beside(i).or(beside(p))
                }) else {
                    continue;
                };
                let mut ts = if wp[p].rim { targets_of(p) } else { Vec::new() };
                if wp[i].rim {
                    ts.extend(targets_of(i));
                }
                ts.sort_unstable();
                ts.dedup();
                out.verts.push(RimVertRecord {
                    vert: ps[i].first,
                    targets: ts,
                    sign,
                    tie,
                });
                if let Some((d, bt)) = &degree[i] {
                    let p_rec = w.records.iter().any(|r| r.0 == p && r.1 == RimEnd::End);
                    let i_rec = w.records.iter().any(|r| r.0 == i && r.1 == RimEnd::Start);
                    let edge = if i_rec {
                        Some(ps[i].edge)
                    } else if p_rec {
                        Some(ps[p].edge)
                    } else {
                        None
                    };
                    out.branches.push(BranchRecord {
                        vert: ps[i].first,
                        targets: bt.clone(),
                        degree: *d,
                        edge,
                    });
                }
            }
        }
        Ok(out)
    })?;
    let mut out = RimRecords {
        side,
        edges: Vec::new(),
        verts: Vec::new(),
        branches: Vec::new(),
    };
    for o in per_loop {
        out.edges.extend(o.edges);
        out.verts.extend(o.verts);
        out.branches.extend(o.branches);
    }
    Ok(out)
}

/// A loop's records.
#[derive(Default)]
struct LoopOut {
    edges: Vec<RimRecord>,
    verts: Vec<RimVertRecord>,
    branches: Vec<BranchRecord>,
}

// ---------------------------------------------------------------------
// Lifting.

/// `pc` carried by `chart` onto `face` (module docs).
fn lift_pcurve(chart: &Chart, face: FaceId, pc: &PCurve) -> Option<LiftedCurve> {
    let mut spans = Vec::with_capacity(pc.spans().len());
    for s in pc.spans() {
        let s = s.as_q()?;
        let mut out: Vec<(P3, Q)> = Vec::with_capacity(3);
        for c in s {
            let uv = [Q::try_from(c[0]).ok()?, Q::try_from(c[1]).ok()?];
            out.push((chart.point(&uv), Q::try_from(c[2]).ok()?));
        }
        spans.push(out.try_into().ok()?);
    }
    Some(LiftedCurve {
        face,
        knots: pc.knots().to_vec(),
        spans,
    })
}

/// The target face a coedge's face lies on, for a snapped face.
fn target_of(f: SnapFace) -> Option<FaceId> {
    match f {
        SnapFace::Snapped { target, .. } => Some(target),
        SnapFace::Kept { .. } => None,
    }
}

/// The elements on the target, read there (module docs).
fn lift(
    rewrite: &Rewrite,
    geometry: &Geometry,
    on: &OnT,
    input: &DecideInput<'_>,
    work: &mut Work,
) -> Result<Lifted, DecideError> {
    let mut charts: BTreeMap<FaceId, Chart> = BTreeMap::new();
    let geo: BTreeMap<SnapEdge, usize> = geometry
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| (e.key, i))
        .collect();
    let ends: BTreeMap<SnapEdge, [SnapVert; 2]> =
        rewrite.edges.iter().map(|e| (e.key, e.ends)).collect();
    for e in &on.edges {
        let g = &geometry.edges[*geo.get(e).ok_or(DecideError::Edge(*e))?];
        for c in &g.coedges {
            if let Some(t) = target_of(c.face)
                && !charts.contains_key(&t)
            {
                let ch = input
                    .faces
                    .iter()
                    .find(|f| f.face == t)
                    .and_then(chart)
                    .ok_or(DecideError::Chart(t))?;
                charts.insert(t, ch);
            }
        }
    }
    // Each edge, and its ends' corners on the target faces.
    type Corners = Vec<(SnapVert, FaceId, [f64; 2])>;
    let lifted = run(work, "snap decide: lifted edges", &on.edges, |e, l| {
        let g = &geometry.edges[geo[e]];
        if g.class == EdgeClass::Zero {
            units(l, 1)?;
            return Ok((Vec::new(), Corners::new()));
        }
        let ts = match g.curve {
            EdgeCurve::Reused { t, .. } => Some(t),
            _ => None,
        };
        let mut curves = Vec::new();
        let mut corners = Corners::new();
        for c in &g.coedges {
            let Some(t) = target_of(c.face) else { continue };
            if curves.iter().any(|x: &LiftedCurve| x.face == t) {
                continue;
            }
            units(l, 3 * c.pcurve.spans().len() as u64)?;
            let lc = lift_pcurve(&charts[&t], t, &c.pcurve)
                .ok_or(Stop::Failed(DecideError::Lift(*e)))?;
            let ks = c.pcurve.knots();
            let span = ts.unwrap_or([ks[0], ks[ks.len() - 1]]);
            for (v, at) in ends[e].iter().zip(span) {
                let k = ks
                    .iter()
                    .position(|&x| x == at)
                    .ok_or(Stop::Failed(DecideError::Lift(*e)))?;
                let uv = c
                    .pcurve
                    .knot_point(k)
                    .ok_or(Stop::Failed(DecideError::Lift(*e)))?;
                corners.push((*v, t, uv));
            }
            curves.push(lc);
        }
        if curves.is_empty() {
            return fail(DecideError::Lift(*e));
        }
        Ok((curves, corners))
    })?;
    let mut out = Lifted::default();
    let mut uvs: BTreeMap<(SnapVert, FaceId), [f64; 2]> = BTreeMap::new();
    for (e, (curves, corners)) in on.edges.iter().zip(lifted) {
        for c in curves {
            out.edges.push((*e, c));
        }
        for (v, t, uv) in corners {
            if !on.vert(v) {
                continue;
            }
            match uvs.get(&(v, t)) {
                Some(w) if w.map(f64::to_bits) != uv.map(f64::to_bits) => {
                    return Err(DecideError::LiftVert(v));
                }
                _ => {
                    uvs.insert((v, t), uv);
                }
            }
        }
    }
    out.edges.sort_by_key(|(e, c)| (*e, c.face));
    let items: Vec<((SnapVert, FaceId), [f64; 2])> = uvs.into_iter().collect();
    let verts = run(
        work,
        "snap decide: lifted vertices",
        &items,
        |((v, t), uv), l| {
            units(l, 1)?;
            let (Ok(u), Ok(w)) = (Q::try_from(uv[0]), Q::try_from(uv[1])) else {
                return fail(DecideError::LiftVert(*v));
            };
            Ok((*v, *t, charts[t].point(&[u, w])))
        },
    )?;
    // Every vertex on the target has a corner there.
    for v in &on.verts {
        if !verts.iter().any(|x| x.0 == *v) {
            return Err(DecideError::LiftVert(*v));
        }
    }
    out.verts = verts;
    Ok(out)
}

#[cfg(test)]
mod tests;
