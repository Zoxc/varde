//! After a snap: which elements of the rewrite lie **on the target**, the
//! leaves a snap made **stale** and their recertification, and the
//! **merges** that grow an earlier cluster when a recertified leaf joins
//! it.
//!
//! **On the target** ([`on_t`]), per element, never per origin. An
//! element is on `T` only where it lies wholly in the rewritten window
//! (the snapped faces, the snapped edges, the rim edges, the snapped
//! side's edges along the rim and their vertices; every edge of the
//! rewrite ends at the rim, so none lies partly inside) and is certified
//! within its own band of `T`: of every target face's plane the faces
//! using it lie on, exactly, its curve's control points (a curve with
//! positive weights lies in their hull) or its position compared with
//! the band (`(n·(p − o))² ≤ β²·|n|²`, no square root). The target's own
//! elements in the window (its vertices, the target edges reused, the
//! snapped side's pieces merged into them) are on `T` by construction,
//! the snapped faces lie on `T`'s surface, and a zero-length edge is on
//! `T` where its ends are. Kept pieces and the join pass's refit pieces
//! (which aren't in the rewrite) are never on `T`; a vertex moved by the
//! snap is on `T` only inside or on the rim, where it ends an edge of the
//! window. An element not certified so is simply not on `T`.
//!
//! **Windows** ([`Window`]): where a snap, or the join pass after every
//! snap, changed an operand, as closed rectangles in its faces' global UV
//! (exact): the cluster's boxes on the snapped side (its rim, snapped and
//! inner elements lie over them), and the boxes of the refit pieces'
//! p-curves ([`join_windows`]; a curve with positive weights lies in its
//! control points' hull). A leaf **meets** a window when one of its two
//! boxes shares a point with a rectangle on its face.
//!
//! **Stale leaves** ([`snap_clusters`]). The clusters are snapped in the
//! geometric order. After each snap only the leaves meeting its window
//! are recertified on the current operands (by the caller,
//! [`Snapper::recertify`], charged), before the next cluster's
//! certificates; the leaves of clusters already snapped never are. A
//! recertified leaf may come back as several (its boxes split), each:
//! - a pair sharing a curve along a target edge the rim runs on
//!   ([`Fresh::SharedCurve`]): coincident there, no leaf and no cluster;
//! - Empty or Simple: used as is (a free leaf);
//! - Near or Contact of the kind of a snapped cluster `K` whose window
//!   the stale leaf met: it **merges into `K`**, every such cluster of
//!   that kind and the not yet snapped cluster the stale leaf belonged
//!   to (if of that kind) made one with it, the least in the order
//!   keeping its place; the merged cluster re-runs from the operands it
//!   started from ([`Run::replaces`]), in the geometric order, before any
//!   later cluster. A leaf whose boxes the cluster already holds adds
//!   nothing;
//! - else, of the kind of the not yet snapped cluster it belonged to: it
//!   stays there;
//! - else (Contact at a Near window, or Near at a Contact one): an
//!   [`Abutment`], left free for the escalation's abutment rule.
//!
//! Once every cluster is snapped the join pass runs ([`Snapper::join`]);
//! the leaves meeting its windows are recertified as after a snap, and
//! where that merges a leaf into a cluster, the cluster re-runs and the
//! join pass runs again.
//!
//! **Termination.** A re-run happens only where a merge strictly grows
//! a cluster's set of leaves (by their boxes): by a leaf it didn't hold,
//! or by another cluster. Clusters are never created, so their count
//! never grows; leaves are pairs of dyadic boxes of bounded depth, so the
//! sets are finite and the merges end; and every recertification, merge
//! and run is charged on the operation's budget, which ends them sooner
//! ([`StaleError::OutOfBudget`]).
//!
//! **Order.** Clusters by their place in the order given (the geometric
//! order), leaves by position, a recertified leaf's pieces appended in
//! the order the caller gives them: the outcome doesn't depend on ids or
//! threads.
//!
//! **Work.** Charged on the operation's budget in passes: on the target,
//! an item per edge and per vertex, a unit per point compared; per snap or
//! join window, the leaves met (an item per candidate leaf, a unit per
//! rectangle compared), their recertification (an item per stale leaf, a
//! unit and a unit per leaf it gives, besides the caller's charges) and the
//! clusters rebuilt (one item, a unit per leaf).

use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use varde_decide::count::boxes::{BoxKey, MAX_BOX_DEPTH};
use varde_decide::ids::FaceId;
use varde_decide::records::keys::Operand;

use super::cluster::{self, Cluster, ClusterError, ClusterLeaf, Tangency, uv_box};
use super::join::{Joined, Origin};
use super::rewrite::{
    EdgeClass, EdgeCurve, Geometry, Rewrite, SnapEdge, SnapFace, SnapVert, TargetFace, plane,
};
use crate::geom::pcurve::PCurve;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

type Q = Rational;
/// A point in space, exactly.
type P3 = [Q; 3];
/// A closed rectangle of a face's global UV, exactly: `[[u_lo, u_hi],
/// [v_lo, v_hi]]`.
type Rect = [[Q; 2]; 2];

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, E, F>(work: &mut Work, stage: &'static str, items: &[T], f: F) -> Result<Vec<R>, E>
where
    T: Sync,
    R: Send,
    E: Send + From<KernelError>,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<E>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => E::from(e),
        })
}

/// Charges `units` of plain work.
fn units<E>(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<E>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

fn exact(x: f64) -> Option<Q> {
    Q::try_from(x).ok()
}

fn exact3(p: [f64; 3]) -> Option<P3> {
    Some([exact(p[0])?, exact(p[1])?, exact(p[2])?])
}

// ---------------------------------------------------------------------
// On the target.

/// What [`on_t`] reads besides the rewrite and its geometry.
#[derive(Clone, Copy)]
pub struct OnTInput<'a> {
    /// The snapped side `S`.
    pub snapped: Operand,
    /// The target faces the cluster reaches, each a plane.
    pub faces: &'a [TargetFace<'a>],
    /// The band of an edge of the rewrite (finite, `≥ 0`).
    pub edge_band: &'a (dyn Fn(SnapEdge) -> f64 + Sync),
    /// The band of a vertex of the rewrite: its tolerance plus the
    /// operation's rounding (finite, `≥ 0`).
    pub vert_band: &'a (dyn Fn(SnapVert) -> f64 + Sync),
}

/// The elements of a rewrite on the target, each list sorted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnT {
    pub faces: Vec<SnapFace>,
    pub edges: Vec<SnapEdge>,
    pub verts: Vec<SnapVert>,
}

impl OnT {
    /// Whether face `k` is on the target.
    pub fn face(&self, k: SnapFace) -> bool {
        self.faces.binary_search(&k).is_ok()
    }

    /// Whether edge `k` is on the target.
    pub fn edge(&self, k: SnapEdge) -> bool {
        self.edges.binary_search(&k).is_ok()
    }

    /// Whether vertex `k` is on the target.
    pub fn vert(&self, k: SnapVert) -> bool {
        self.verts.binary_search(&k).is_ok()
    }
}

/// Why [`on_t`] can't decide.
#[derive(Clone, Debug, PartialEq)]
pub enum OnTError {
    /// A band isn't finite and `≥ 0`.
    Band,
    /// This target face isn't described, or isn't a plane.
    Chart(FaceId),
    /// This edge of the rewrite has no geometry.
    Edge(SnapEdge),
    /// This vertex of the rewrite has no geometry.
    Vert(SnapVert),
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

impl From<KernelError> for OnTError {
    fn from(e: KernelError) -> Self {
        OnTError::OutOfBudget(e)
    }
}

/// A target face's plane: a point, a normal and its squared length.
struct Plane {
    o: P3,
    n: P3,
    nn: Q,
}

impl Plane {
    /// Whether `p` is within `beta` of the plane, exactly.
    fn within(&self, p: &P3, beta: &Q) -> bool {
        let d: Q = (0..3).map(|k| &self.n[k] * (&p[k] - &self.o[k])).sum();
        &d * &d <= beta * beta * &self.nn
    }
}

/// The band `b` exactly, if finite and `≥ 0`.
fn band(b: f64) -> Result<Q, OnTError> {
    if b.is_finite() && b >= 0.0 {
        exact(b).ok_or(OnTError::Band)
    } else {
        Err(OnTError::Band)
    }
}

/// The elements of `rewrite` (with its `geometry`) on the target (module
/// docs), charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn on_t(
    rewrite: &Rewrite,
    geometry: &Geometry,
    input: &OnTInput<'_>,
    work: &mut Work,
) -> Result<OnT, OnTError> {
    let t_op = input.snapped.other();
    // The target faces each edge's snapped faces lie on.
    let mut targets: BTreeMap<SnapEdge, BTreeSet<FaceId>> = BTreeMap::new();
    let mut faces = Vec::new();
    for f in &rewrite.faces {
        if let SnapFace::Snapped { target, .. } = f.key {
            faces.push(f.key);
            for u in f.loops.iter().flatten() {
                targets.entry(u.edge).or_default().insert(target);
            }
        }
    }
    faces.sort_unstable();
    let mut planes: BTreeMap<FaceId, Plane> = BTreeMap::new();
    for t in targets.values().flatten() {
        if planes.contains_key(t) {
            continue;
        }
        let (o, n) = input
            .faces
            .iter()
            .find(|f| f.face == *t)
            .and_then(plane)
            .ok_or(OnTError::Chart(*t))?;
        let nn = (0..3).map(|k| &n[k] * &n[k]).sum();
        planes.insert(*t, Plane { o, n, nn });
    }
    let geo_edges: BTreeMap<SnapEdge, usize> = geometry
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| (e.key, i))
        .collect();
    for e in &rewrite.edges {
        if !geo_edges.contains_key(&e.key) {
            return Err(OnTError::Edge(e.key));
        }
    }
    let none = BTreeSet::new();
    let planes_of =
        |ts: &BTreeSet<FaceId>| -> Vec<&Plane> { ts.iter().map(|t| &planes[t]).collect() };

    // The edges: `Some(on)` where decided, `None` for a zero-length edge.
    let edges = run(work, "snap on-T: edges", &rewrite.edges, |e, l| {
        units(l, 1)?;
        let g = &geometry.edges[geo_edges[&e.key]];
        Ok(match g.class {
            EdgeClass::Kept => Some(false),
            EdgeClass::Target | EdgeClass::Merged(_) => Some(true),
            EdgeClass::Zero => None,
            EdgeClass::Snapped | EdgeClass::AlongRim | EdgeClass::Rim => {
                let beta = band((input.edge_band)(e.key)).map_err(Stop::Failed)?;
                let ps = planes_of(targets.get(&e.key).unwrap_or(&none));
                let EdgeCurve::New(c) = &g.curve else {
                    return Ok(Some(false));
                };
                let mut on = !ps.is_empty();
                'all: for s in c.spans() {
                    for p in s {
                        units(l, ps.len() as u64)?;
                        let Some(x) = exact3([p[0], p[1], p[2]]) else {
                            on = false;
                            break 'all;
                        };
                        if !ps.iter().all(|pl| pl.within(&x, &beta)) {
                            on = false;
                            break 'all;
                        }
                    }
                }
                Some(on)
            }
        })
    })?;

    // The vertices of the window: the ends of its edges, with the target
    // faces round them.
    let mut window: BTreeMap<SnapVert, BTreeSet<FaceId>> = BTreeMap::new();
    for e in &rewrite.edges {
        if geometry.edges[geo_edges[&e.key]].class == EdgeClass::Kept {
            continue;
        }
        for v in e.ends {
            let w = window.entry(v).or_default();
            w.extend(targets.get(&e.key).into_iter().flatten().copied());
        }
    }
    let geo_verts: BTreeMap<SnapVert, usize> = geometry
        .verts
        .iter()
        .enumerate()
        .map(|(i, v)| (v.key, i))
        .collect();
    let cands: Vec<(SnapVert, &BTreeSet<FaceId>)> = window.iter().map(|(v, t)| (*v, t)).collect();
    for (v, _) in &cands {
        if !geo_verts.contains_key(v) {
            return Err(OnTError::Vert(*v));
        }
    }
    let verts = run(work, "snap on-T: vertices", &cands, |(v, ts), l| {
        units(l, 1 + ts.len() as u64)?;
        if matches!(v, SnapVert::Vert(o, _) if *o == t_op) {
            return Ok(true);
        }
        let beta = band((input.vert_band)(*v)).map_err(Stop::Failed)?;
        let Some(p) = exact3(geometry.verts[geo_verts[v]].pos) else {
            return Ok(false);
        };
        let ps = planes_of(ts);
        Ok(!ps.is_empty() && ps.iter().all(|pl| pl.within(&p, &beta)))
    })?;
    let mut out_verts: Vec<SnapVert> = cands
        .iter()
        .zip(&verts)
        .filter(|(_, on)| **on)
        .map(|((v, _), _)| *v)
        .collect();
    out_verts.sort_unstable();
    let mut out_edges: Vec<SnapEdge> = rewrite
        .edges
        .iter()
        .zip(&edges)
        .filter(|(e, on)| match on {
            Some(on) => *on,
            None => e.ends.iter().all(|v| out_verts.binary_search(v).is_ok()),
        })
        .map(|(e, _)| e.key)
        .collect();
    out_edges.sort_unstable();
    Ok(OnT {
        faces,
        edges: out_edges,
        verts: out_verts,
    })
}

// ---------------------------------------------------------------------
// Windows.

/// Where a snap or the join pass changed the operands (module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    rects: BTreeMap<(Operand, FaceId), Vec<Rect>>,
}

/// Whether the closed rectangles `a` and `b` share a point.
fn meet(a: &Rect, b: &Rect) -> bool {
    (0..2).all(|k| a[k][0] <= b[k][1] && b[k][0] <= a[k][1])
}

/// Box `b`'s rectangle in its face's global UV, exactly.
fn box_rect(b: &BoxKey) -> Rect {
    let scale = Q::from(1u64 << MAX_BOX_DEPTH);
    uv_box(b).map(|side| side.map(|x| Q::from(x) / &scale))
}

impl Window {
    /// The empty window.
    pub fn new() -> Window {
        Window::default()
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Its number of rectangles.
    pub fn len(&self) -> usize {
        self.rects.values().map(Vec::len).sum()
    }

    fn push(&mut self, at: (Operand, FaceId), r: Rect) {
        let rs = self.rects.entry(at).or_default();
        if !rs.contains(&r) {
            rs.push(r);
        }
    }

    /// Adds box `b`, on its face.
    pub fn add_box(&mut self, b: &BoxKey) {
        self.push((b.operand(), b.face()), box_rect(b));
    }

    /// Adds the box of p-curve `pc`'s control values ([`PSpan::hull`]) on
    /// face `face` of `operand`.
    ///
    /// [`PSpan::hull`]: crate::geom::pcurve::PSpan::hull
    pub fn add_pcurve(&mut self, operand: Operand, face: FaceId, pc: &PCurve) {
        let mut lo: Option<[Q; 2]> = None;
        let mut hi: Option<[Q; 2]> = None;
        for p in pc.spans().iter().flat_map(|s| s.hull()) {
            // Finite: a p-curve's values are.
            let (Some(u), Some(v)) = (exact(p[0]), exact(p[1])) else {
                continue;
            };
            let x = [u, v];
            match (&mut lo, &mut hi) {
                (Some(l), Some(h)) => {
                    for k in 0..2 {
                        if x[k] < l[k] {
                            l[k] = x[k].clone();
                        }
                        if x[k] > h[k] {
                            h[k] = x[k].clone();
                        }
                    }
                }
                _ => {
                    lo = Some(x.clone());
                    hi = Some(x);
                }
            }
        }
        if let (Some([u0, v0]), Some([u1, v1])) = (lo, hi) {
            self.push((operand, face), [[u0, u1], [v0, v1]]);
        }
    }

    /// Adds every rectangle of `other`.
    pub fn add_window(&mut self, other: &Window) {
        for (at, rs) in &other.rects {
            for r in rs {
                self.push(*at, r.clone());
            }
        }
    }

    /// The number of its rectangles on box `b`'s face.
    fn on_face(&self, b: &BoxKey) -> usize {
        self.rects.get(&(b.operand(), b.face())).map_or(0, Vec::len)
    }

    /// Whether box `b` shares a point with one of its rectangles.
    pub fn meets_box(&self, b: &BoxKey) -> bool {
        let Some(rs) = self.rects.get(&(b.operand(), b.face())) else {
            return false;
        };
        let r = box_rect(b);
        rs.iter().any(|x| meet(x, &r))
    }

    /// Whether leaf `l` meets it: one of its boxes does.
    pub fn meets<L: ClusterLeaf + ?Sized>(&self, l: &L) -> bool {
        l.boxes().iter().any(|b| self.meets_box(b))
    }
}

/// The window of the cluster `cluster` (of the leaves `leaves`): its
/// boxes on the snapped side `snapped`.
pub fn cluster_window<L: ClusterLeaf, K: Ord + Clone>(
    cluster: &Cluster<K>,
    leaves: &[L],
    snapped: Operand,
) -> Window {
    let mut w = Window::new();
    for i in cluster.leaves() {
        if let Some(l) = leaves.get(i as usize) {
            for b in l.boxes().iter().filter(|b| b.operand() == snapped) {
                w.add_box(b);
            }
        }
    }
    w
}

/// Per cluster, the window of the join pass's refit pieces with an end
/// it moved: their coedges' p-curves, on the faces of `operand`.
pub fn join_windows<P, C: Ord + Clone>(
    operand: Operand,
    joined: &Joined<P, C>,
) -> BTreeMap<C, Window> {
    let mut out: BTreeMap<C, Window> = BTreeMap::new();
    for p in joined.pieces.iter().filter(|p| p.refit) {
        let Origin::MovedJoin { clusters, .. } = &p.origin else {
            continue;
        };
        for c in clusters {
            let w = out.entry(c.clone()).or_default();
            for ce in &p.coedges {
                w.add_pcurve(operand, ce.face, &ce.pcurve);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// Stale leaves and merges.

/// A leaf recertified on the current operands.
#[derive(Clone, Debug, PartialEq)]
pub enum Fresh<L> {
    /// A pair sharing a curve along a target edge the rim runs on:
    /// coincident there, no leaf.
    SharedCurve,
    /// A leaf, of any kind.
    Leaf(L),
}

/// A run of a cluster's snap.
#[derive(Clone, Copy, Debug)]
pub struct Run<'a, K> {
    /// Its place: the least among the clusters given of those it is made
    /// of.
    pub slot: u32,
    /// The cluster.
    pub cluster: &'a Cluster<K>,
    /// The clusters (by place, ascending) whose rewrites are in place and
    /// this run replaces: it starts from the operands they started from
    /// (their entities mapped back through their snap maps), and from the
    /// operands before the join pass. Empty on a first run.
    pub replaces: &'a [u32],
}

/// The snap's steps [`snap_clusters`] drives.
pub(crate) trait Snapper<L, K> {
    /// A step's own failure.
    type Error: Send;

    /// The geometric key of face `face` of `operand` (its form key and
    /// name, never its id).
    fn face_key(&self, operand: Operand, face: FaceId) -> K;

    /// Snaps `run`'s cluster (its leaves are positions of `leaves`),
    /// charged to `work`, and gives its window (e.g. [`cluster_window`]).
    fn snap(
        &mut self,
        run: &Run<'_, K>,
        leaves: &[L],
        work: &mut Work,
    ) -> Result<Window, Self::Error>;

    /// The moved-vertex join pass over the clusters `snapped` (by place,
    /// in order), from the operands before any join pass, charged to
    /// `work`; its windows, each with the place of a cluster that moved an
    /// end of the pieces in it (e.g. [`join_windows`]).
    fn join(
        &mut self,
        snapped: &[(u32, &Cluster<K>)],
        leaves: &[L],
        work: &mut Work,
    ) -> Result<Vec<(u32, Window)>, Self::Error>;

    /// `leaf` recertified on the current operands, charged to `ledger`:
    /// one leaf or its pieces, the first operand's box first in each.
    fn recertify(
        &self,
        leaf: &L,
        ledger: &mut Ledger<'_>,
    ) -> Result<Vec<Fresh<L>>, Stop<Self::Error>>;
}

/// A recertified leaf of one kind meeting only windows of clusters of the
/// other: the escalation's abutment case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abutment {
    /// The leaf's position.
    pub leaf: u32,
    /// The first of those clusters, by place.
    pub cluster: u32,
}

/// A snapped cluster.
#[derive(Clone, Debug, PartialEq)]
pub struct Done<K> {
    /// Its place.
    pub slot: u32,
    /// The cluster, with the leaves merged into it.
    pub cluster: Cluster<K>,
    /// The places of the clusters it was made of, ascending.
    pub parts: Vec<u32>,
    /// Its window: its last snap's, and the last join pass's for it.
    pub window: Window,
}

/// The outcome of [`snap_clusters`].
#[derive(Clone, Debug, PartialEq)]
pub struct Snapped<L, K> {
    /// The leaves given, then the recertified ones in the order made:
    /// clusters and the lists below hold positions here.
    pub leaves: Vec<L>,
    /// The snapped clusters, by place.
    pub clusters: Vec<Done<K>>,
    /// The leaves in no cluster and not replaced: used as they are.
    pub free: Vec<u32>,
    /// The stale leaves that came back sharing a curve, ascending.
    pub exempt: Vec<u32>,
    /// The abutting leaves, in the order found.
    pub abutments: Vec<Abutment>,
    /// The snaps run, first runs and re-runs.
    pub runs: u32,
    /// The merges that grew a cluster.
    pub merges: u32,
}

/// Why [`snap_clusters`] stopped.
#[derive(Clone, Debug, PartialEq)]
pub enum StaleError<E> {
    /// The leaves or clusters given don't fit: too many, a leaf in two
    /// clusters, or a leaf (given or recertified) with its boxes not of
    /// the first operand then the second, or a NaN.
    Cluster(ClusterError),
    /// The join pass named a cluster that isn't there.
    Slot(u32),
    /// A snap, the join pass or a recertification failed.
    Step(E),
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

impl<E> From<KernelError> for StaleError<E> {
    fn from(e: KernelError) -> Self {
        StaleError::OutOfBudget(e)
    }
}

/// Who a leaf belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    /// No cluster: used as it is.
    Free,
    /// The cluster at this place.
    In(u32),
    /// Replaced by its recertification, or a repeat of a cluster's leaf.
    Gone,
}

/// A cluster while the snaps run.
struct Slot<K> {
    cluster: Cluster<K>,
    /// Its leaves' positions, ascending.
    members: Vec<u32>,
    parts: Vec<u32>,
    /// The places whose rewrites are in place for it, ascending.
    in_place: Vec<u32>,
    /// Its last snap's window, and the join pass's for it.
    window: Window,
    join: Window,
}

struct State<L, K> {
    leaves: Vec<L>,
    owner: Vec<Owner>,
    slots: Vec<Option<Slot<K>>>,
    /// Each place's place after merges (a forest with roots at the live
    /// places).
    parent: Vec<u32>,
    pending: BTreeSet<u32>,
    exempt: Vec<u32>,
    abutments: Vec<Abutment>,
    runs: u32,
    merges: u32,
}

impl<L, K> State<L, K>
where
    L: ClusterLeaf + Send,
    K: Ord + Clone + Send + Sync,
{
    fn find(&self, mut k: u32) -> u32 {
        while self.parent[k as usize] != k {
            k = self.parent[k as usize];
        }
        k
    }

    fn slot(&self, k: u32) -> Option<&Slot<K>> {
        self.slots.get(k as usize).and_then(Option::as_ref)
    }

    /// The live places with a rewrite in place, with their kinds and
    /// windows.
    fn snapped(&self) -> Vec<(u32, Tangency, &Window, &Window)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(k, s)| {
                let s = s.as_ref()?;
                // Fits: places are `u32`.
                (!s.in_place.is_empty()).then_some((k as u32, s.cluster.kind(), &s.window, &s.join))
            })
            .collect()
    }

    /// Recertifies the leaves meeting window `w` (a snapped cluster's)
    /// and routes them (module docs).
    fn recheck<S>(
        &mut self,
        w: &Window,
        snapper: &S,
        work: &mut Work,
    ) -> Result<(), StaleError<S::Error>>
    where
        S: Snapper<L, K> + Sync,
    {
        if w.is_empty() {
            return Ok(());
        }
        // The candidates: free leaves and those of clusters not snapped.
        let cands: Vec<u32> = (0..self.owner.len() as u32)
            .filter(|&i| match self.owner[i as usize] {
                Owner::Free => true,
                Owner::In(j) => self.slot(j).is_some_and(|s| s.in_place.is_empty()),
                Owner::Gone => false,
            })
            .collect();
        let snapped = self.snapped();
        let leaves = &self.leaves;
        // Per candidate meeting `w`: the snapped clusters whose windows it
        // meets, by place.
        type Err<S, L, K> = StaleError<<S as Snapper<L, K>>::Error>;
        let met: Vec<Option<Vec<u32>>> =
            run::<_, _, Err<S, L, K>, _>(work, "snap stale: meet", &cands, |&i, l| {
                let leaf = &leaves[i as usize];
                let boxes = leaf.boxes();
                let count = |w: &Window| boxes.iter().map(|b| w.on_face(b) as u64).sum::<u64>();
                units(l, 1 + count(w))?;
                if !w.meets(leaf) {
                    return Ok(None);
                }
                let mut by = Vec::new();
                for (s, _, win, join) in &snapped {
                    units(l, 1 + count(win) + count(join))?;
                    if win.meets(leaf) || join.meets(leaf) {
                        by.push(*s);
                    }
                }
                Ok(Some(by))
            })?;
        let stale: Vec<(u32, Vec<u32>)> = cands
            .into_iter()
            .zip(met)
            .filter_map(|(i, m)| m.map(|m| (i, m)))
            .collect();
        if stale.is_empty() {
            return Ok(());
        }
        let fresh: Vec<Vec<Fresh<L>>> = run(work, "snap stale: recertify", &stale, |(i, _), l| {
            units(l, 1)?;
            let out = snapper
                .recertify(&leaves[*i as usize], l)
                .map_err(|s| match s {
                    Stop::Failed(e) => Stop::Failed(StaleError::Step(e)),
                    Stop::Exhausted(x) => Stop::Exhausted(x),
                    Stop::Cap(c) => Stop::Cap(c),
                })?;
            units(l, out.len() as u64)?;
            Ok(out)
        })?;

        // Route each piece.
        let kind_of = |st: &Self, s: u32| st.slot(s).map(|x| x.cluster.kind());
        // The places each leaf merges, and its position.
        let mut routes: Vec<(Vec<u32>, u32)> = Vec::new();
        let mut changed: BTreeSet<u32> = BTreeSet::new();
        for ((i, met), pieces) in stale.into_iter().zip(fresh) {
            let old = match self.owner[i as usize] {
                Owner::In(j) => Some(j),
                _ => None,
            };
            self.owner[i as usize] = Owner::Gone;
            if let Some(j) = old
                && let Some(s) = self.slots[j as usize].as_mut()
            {
                s.members.retain(|&m| m != i);
                changed.insert(j);
            }
            for f in pieces {
                let l = match f {
                    Fresh::SharedCurve => {
                        if self.exempt.last() != Some(&i) {
                            self.exempt.push(i);
                        }
                        continue;
                    }
                    Fresh::Leaf(l) => l,
                };
                let p = u32::try_from(self.leaves.len())
                    .map_err(|_| StaleError::Cluster(ClusterError::TooMany))?;
                let [x, y] = l.boxes();
                if x.operand() != Operand::A || y.operand() != Operand::B {
                    return Err(StaleError::Cluster(ClusterError::Operands(p)));
                }
                let t = l.tangency();
                self.leaves.push(l);
                self.owner.push(Owner::Free);
                let Some(t) = t else {
                    continue;
                };
                let roots: BTreeSet<u32> = met.iter().map(|&s| self.find(s)).collect();
                let mut same: BTreeSet<u32> = roots
                    .iter()
                    .copied()
                    .filter(|&s| kind_of(self, s) == Some(t))
                    .collect();
                let home = old.filter(|&j| kind_of(self, j) == Some(t));
                if !same.is_empty() {
                    same.extend(home);
                    routes.push((same.into_iter().collect(), p));
                } else if let Some(j) = home {
                    self.owner[p as usize] = Owner::In(j);
                    if let Some(s) = self.slots[j as usize].as_mut() {
                        s.members.push(p);
                    }
                    changed.insert(j);
                } else if let Some(&s) = roots.first() {
                    self.abutments.push(Abutment {
                        leaf: p,
                        cluster: s,
                    });
                }
            }
        }

        // Merge, then rebuild the clusters changed.
        let mut before: BTreeMap<u32, BTreeSet<[BoxKey; 2]>> = BTreeMap::new();
        for (group, p) in routes {
            let g = group.iter().map(|&s| self.find(s)).min().expect("a group");
            before.entry(g).or_insert_with(|| self.boxes_of(g));
            for s in group {
                let s = self.find(s);
                if s == g {
                    continue;
                }
                let Some(part) = self.slots[s as usize].take() else {
                    continue;
                };
                self.parent[s as usize] = g;
                self.pending.remove(&s);
                changed.remove(&s);
                let into = self.slots[g as usize].as_mut().expect("a live place");
                for &m in &part.members {
                    self.owner[m as usize] = Owner::In(g);
                }
                into.members.extend(part.members);
                into.parts.extend(part.parts);
                into.in_place.extend(part.in_place);
                into.window.add_window(&part.window);
                into.join.add_window(&part.join);
            }
            self.owner[p as usize] = Owner::In(g);
            let into = self.slots[g as usize].as_mut().expect("a live place");
            into.members.push(p);
            changed.insert(g);
        }
        self.rebuild(&changed, snapper, work)?;
        // A merge target is a snapped cluster (one not yet snapped comes
        // after every one that has run), whose leaves only grow: by another
        // cluster's, or by a leaf it didn't hold.
        for (g, was) in before {
            if self.boxes_of(g).len() > was.len() {
                self.merges = self.merges.saturating_add(1);
                self.pending.insert(g);
            }
        }
        Ok(())
    }

    /// The boxes of the leaves of the cluster at place `g`.
    fn boxes_of(&self, g: u32) -> BTreeSet<[BoxKey; 2]> {
        self.slot(g)
            .map(|s| {
                s.members
                    .iter()
                    .map(|&m| self.leaves[m as usize].boxes())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Rebuilds the clusters at the places `changed`: their members
    /// sorted, repeated boxes dropped (the first kept), each cluster made
    /// again; a cluster left without leaves dissolves.
    fn rebuild<S>(
        &mut self,
        changed: &BTreeSet<u32>,
        snapper: &S,
        work: &mut Work,
    ) -> Result<(), StaleError<S::Error>>
    where
        S: Snapper<L, K> + Sync,
    {
        let mut jobs: Vec<(u32, Tangency, Vec<u32>)> = Vec::new();
        for &g in changed {
            let Some(s) = self.slots[g as usize].as_mut() else {
                continue;
            };
            s.members.sort_unstable();
            s.members.dedup();
            s.parts.sort_unstable();
            s.in_place.sort_unstable();
            s.in_place.dedup();
            let mut seen: BTreeSet<[BoxKey; 2]> = BTreeSet::new();
            let mut keep = Vec::with_capacity(s.members.len());
            for &m in &s.members {
                if seen.insert(self.leaves[m as usize].boxes()) {
                    keep.push(m);
                } else {
                    self.owner[m as usize] = Owner::Gone;
                }
            }
            s.members.clone_from(&keep);
            jobs.push((g, s.cluster.kind(), keep));
        }
        if jobs.is_empty() {
            return Ok(());
        }
        let leaves = &self.leaves;
        let total: u64 = jobs.iter().map(|j| j.2.len() as u64).sum();
        let built = run(work, "snap stale: merge", &[()], |_, l| {
            units(l, 1 + total)?;
            let face_key = |o: Operand, f: FaceId| snapper.face_key(o, f);
            jobs.iter()
                .map(|(_, kind, members)| {
                    cluster::of_leaves(*kind, members, leaves, &face_key)
                        .map_err(|e| Stop::Failed(StaleError::Cluster(e)))
                })
                .collect::<Result<Vec<_>, _>>()
        })?;
        let built = built.into_iter().next().expect("one item");
        for ((g, _, _), c) in jobs.into_iter().zip(built) {
            match c {
                Some(c) => {
                    if let Some(s) = self.slots[g as usize].as_mut() {
                        s.cluster = c;
                    }
                }
                None => {
                    self.slots[g as usize] = None;
                    self.pending.remove(&g);
                }
            }
        }
        Ok(())
    }
}

/// Snaps `clusters` (in the geometric order) of `leaves` with `snapper`,
/// recertifying the stale leaves after each snap and after the join pass,
/// and merging and re-running clusters until no merge grows one (module
/// docs); charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn snap_clusters<L, K, S>(
    clusters: Vec<Cluster<K>>,
    leaves: Vec<L>,
    snapper: &mut S,
    work: &mut Work,
) -> Result<Snapped<L, K>, StaleError<S::Error>>
where
    L: ClusterLeaf + Send,
    K: Ord + Clone + Send + Sync,
    S: Snapper<L, K> + Sync,
{
    let too_many = || StaleError::Cluster(ClusterError::TooMany);
    let n = u32::try_from(leaves.len()).map_err(|_| too_many())?;
    let m = u32::try_from(clusters.len()).map_err(|_| too_many())?;
    let mut owner = vec![Owner::Free; n as usize];
    let mut slots = Vec::with_capacity(clusters.len());
    for (k, c) in clusters.into_iter().enumerate() {
        // Fits: checked above.
        let k = k as u32;
        let mut members: Vec<u32> = c.leaves().collect();
        for &i in &members {
            match owner.get(i as usize) {
                Some(Owner::Free) => owner[i as usize] = Owner::In(k),
                _ => return Err(StaleError::Cluster(ClusterError::Kind(i))),
            }
        }
        members.sort_unstable();
        slots.push(Some(Slot {
            cluster: c,
            members,
            parts: vec![k],
            in_place: Vec::new(),
            window: Window::new(),
            join: Window::new(),
        }));
    }
    let mut st = State {
        leaves,
        owner,
        slots,
        parent: (0..m).collect(),
        pending: (0..m).collect(),
        exempt: Vec::new(),
        abutments: Vec::new(),
        runs: 0,
        merges: 0,
    };
    loop {
        while let Some(k) = st.pending.pop_first() {
            let Some(slot) = st.slot(k) else {
                continue;
            };
            let r = Run {
                slot: k,
                cluster: &slot.cluster,
                replaces: &slot.in_place,
            };
            let window = snapper
                .snap(&r, &st.leaves, work)
                .map_err(StaleError::Step)?;
            st.runs = st.runs.saturating_add(1);
            // A run starts before the join pass: its windows go.
            for s in st.slots.iter_mut().flatten() {
                s.join = Window::new();
            }
            let s = st.slots[k as usize].as_mut().expect("a live place");
            s.in_place = vec![k];
            s.window = window.clone();
            st.recheck(&window, snapper, work)?;
        }
        let done: Vec<(u32, &Cluster<K>)> = st
            .slots
            .iter()
            .enumerate()
            .filter_map(|(k, s)| s.as_ref().map(|s| (k as u32, &s.cluster)))
            .collect();
        if done.is_empty() {
            break;
        }
        let mut joins = snapper
            .join(&done, &st.leaves, work)
            .map_err(StaleError::Step)?;
        joins.sort_by_key(|(k, _)| *k);
        for (k, w) in &joins {
            let g = st.find(*k);
            let Some(s) = st.slots.get_mut(g as usize).and_then(Option::as_mut) else {
                return Err(StaleError::Slot(*k));
            };
            s.join.add_window(w);
        }
        for (k, w) in &joins {
            let g = st.find(*k);
            if st.slot(g).is_some() {
                st.recheck(w, snapper, work)?;
            }
        }
        if st.pending.is_empty() {
            break;
        }
    }
    let mut clusters = Vec::new();
    let mut slots = std::mem::take(&mut st.slots);
    for (k, s) in slots.iter_mut().enumerate() {
        if let Some(s) = s.take() {
            let mut window = s.window;
            window.add_window(&s.join);
            clusters.push(Done {
                slot: k as u32,
                cluster: s.cluster,
                parts: s.parts,
                window,
            });
        }
    }
    let free = st
        .owner
        .iter()
        .enumerate()
        .filter(|(_, o)| **o == Owner::Free)
        .map(|(i, _)| i as u32)
        .collect();
    st.exempt.sort_unstable();
    Ok(Snapped {
        leaves: st.leaves,
        clusters,
        free,
        exempt: st.exempt,
        abutments: st.abutments,
        runs: st.runs,
        merges: st.merges,
    })
}

#[cfg(test)]
mod tests;
