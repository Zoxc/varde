//! The moved-vertex joins: one pass after **every** cluster of the
//! operation is snapped. A snap moves the snapped side's vertices inside
//! and on its rim onto the target (or to the sheets' mid-point, or where
//! the corner rule or a `SnapAlong` bracket end pins them); each cluster
//! only records the vertices it moved ([`ClusterMoves`]). The edges at
//! those vertices that lie outside every cluster keep their p-curves (a
//! moved vertex keeps its UV on the faces it isn't snapped with) and get
//! their curve refit here, once, from their geometry before the operation
//! with all moved ends known. So the clusters' order is work order only:
//! the outcome reads the set of clusters and their moves, never the order
//! they come in.
//!
//! **Refit intervals.** Per edge, the stretch next to a moved end runs
//! from that end's knot to the first knot of the curve inside the edge
//! that is also a knot of every p-curve of every edge on the curve there
//! (else to the edge's other end). Where both ends moved and no such knot
//! lies between them, the whole edge is one interval. On an interval the
//! curve's knot points are displaced linearly in `t` between its ends'
//! displacements (the moved vertex's new position less the curve's point
//! there, `0` at an end that stays), exactly, and rounded once; a span's
//! middle control point by the mean of its ends' displacements, its
//! weights kept, so the span moves by a convex combination of its control
//! points' moves. The moved end's control point **is** the vertex's new
//! position, bit for bit; the spans outside the intervals keep their bits,
//! and the refit spans start exactly where they end.
//!
//! **Shared curves.** Edges on one curve object ([`JoinEdge::curve`]:
//! slit members, coincident edges) are refit once per curve: their
//! intervals must be one interval or apart, the ends of all its edges at
//! one knot moved alike (or all kept), and no edge may run through a
//! moved knot or end inside another edge's interval
//! ([`JoinError::Shared`]).
//!
//! **Pieces.** Where a refit interval meets the untouched rest of its
//! edge the edge is split there, at a new vertex ([`Junction`], at the
//! curve's knot point, so both pieces' curves hold it bit for bit), and
//! each piece keeps the edge's name; origin and tolerance are per piece.
//! A refit piece's origin is [`Origin::MovedJoin`]: the edge's prior
//! origin (never itself a moved join: an edge moved before keeps its
//! prior and adds the new clusters) and every cluster that moved one of
//! its ends, in the clusters' order. The rest keeps its origin, curve,
//! p-curves and seam bounds.
//!
//! **Certificate.** Each refit interval's seam bound is certified on
//! every coedge of its edge ([`seam_bound`]) against its target:
//! `max(prior tol, b_K + ε_seam)`, `b_K` the largest over this
//! operation's clusters that moved the piece's ends, or, at an end a
//! corner-rule placement or bracket end put there, `R_on + ε_seam` of
//! that placement when larger (vertex-local). A bound above it fails the
//! pass ([`JoinError::Fit`], a validity failure).
//!
//! **Order.** Clusters by their keys `C` (the caller's geometric order),
//! vertices, edges and curves by id, knots by parameter: the outcome is
//! the same for any order of the inputs.
//!
//! **Work.** Charged on the operation's budget in passes: the clusters'
//! moves (an item per cluster, a unit per vertex), the edges (an item
//! each, a unit per coedge), the curves (an item per curve, a unit per
//! edge on it and per span, and the seam bounds' cells).

use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use varde_decide::ids::{CurveId, EdgeId, FaceId, VertId};
use varde_decide::num::bounded::add_up;
use varde_decide::num::exact::rat as q;

use super::corner::{exact3, sub3, to_f64};

use crate::geom::certify::{SeamError, seam_bound};
use crate::geom::curve::{Curve3D, Span3};
use crate::geom::pcurve::PCurve;
use crate::geom::surface::Surface;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

type Q = Rational;
/// A point in space, exactly.
type P3 = [Q; 3];

/// A vertex a cluster moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovedVert {
    /// The vertex.
    pub vert: VertId,
    /// Its new position.
    pub pos: [f64; 3],
    /// `R_on` of the corner-rule placement or bracket end that put it
    /// there; `None` for a vertex the snap map moves (by at most `b_K`).
    pub r_on: Option<f64>,
}

/// One cluster's record for the join pass: its key, its `b_K` and the
/// vertices it moved.
#[derive(Clone, Copy, Debug)]
pub struct ClusterMoves<'a, C> {
    /// The cluster, as edges record it in [`Origin::MovedJoin`]; ordered
    /// by the caller's geometric order.
    pub cluster: C,
    /// Its gap bound `b_K`.
    pub b_k: f64,
    /// The vertices it moved.
    pub moved: &'a [MovedVert],
}

/// An edge's origin as far as the join pass reads it: `P` the caller's
/// origins other than a moved join, `C` its clusters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin<P, C> {
    /// Any origin but a moved join.
    Plain(P),
    /// A moved join: its origin before its first move and every cluster
    /// that moved one of its ends, in their order.
    MovedJoin { prior: P, clusters: Vec<C> },
}

/// A coedge of an edge the pass may refit.
#[derive(Clone, Copy, Debug)]
pub struct JoinCoedge<'a> {
    /// Its face.
    pub face: FaceId,
    /// The face's surface.
    pub surface: &'a Surface,
    /// Its p-curve, over its edge's range, on the edge's parameter.
    pub pcurve: &'a PCurve,
    /// Its seam bound.
    pub seam: f64,
}

/// An edge outside the clusters, as the pass reads it.
#[derive(Clone, Debug)]
pub struct JoinEdge<'a, P, C> {
    /// The edge.
    pub edge: EdgeId,
    /// Its curve object; edges sharing one are refit together.
    pub curve: CurveId,
    /// Its parameter range on the curve, increasing: two knots.
    pub range: [f64; 2],
    /// Its vertices at `range[0]` and `range[1]`.
    pub ends: [VertId; 2],
    /// Its tolerance before the operation.
    pub tol: f64,
    /// Its origin before the operation.
    pub origin: Origin<P, C>,
    /// Its coedges.
    pub coedges: &'a [JoinCoedge<'a>],
}

/// A curve object the edges use.
#[derive(Clone, Copy, Debug)]
pub struct JoinCurve<'a> {
    pub id: CurveId,
    pub curve: &'a Curve3D,
}

/// What [`join`] reads besides the clusters' moves.
#[derive(Clone, Copy, Debug)]
pub struct JoinInput<'a, P, C> {
    /// The curves the edges use.
    pub curves: &'a [JoinCurve<'a>],
    /// The edges outside the clusters, with every edge on a curve any of
    /// them uses; edges with no moved end are read for the sharing rules
    /// only.
    pub edges: &'a [JoinEdge<'a, P, C>],
    /// The seam target `ε_seam`.
    pub eps_seam: f64,
}

/// Which end of its edge a junction lies nearer to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Near {
    Start,
    End,
}

/// A junction's key: the edge split there and the end whose refit
/// interval ends there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Junction {
    pub edge: EdgeId,
    pub near: Near,
}

/// A vertex where a refit interval meets its edge's untouched rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JunctionVert {
    /// Its key.
    pub key: Junction,
    /// Its position: the curve's knot point there.
    pub pos: [f64; 3],
    /// The curve and the knot it lies at.
    pub on: (CurveId, f64),
}

/// An end of a piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PieceEnd {
    /// An end of the edge.
    Vert(VertId),
    /// A junction.
    Junction(Junction),
}

/// A coedge of a piece: its p-curve over the piece's range and its seam
/// bound.
#[derive(Clone, Debug, PartialEq)]
pub struct PieceCoedge {
    pub face: FaceId,
    pub pcurve: PCurve,
    pub seam: f64,
}

/// A piece of an edge the pass touched: its name is the edge's.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece<P, C> {
    /// The edge.
    pub edge: EdgeId,
    /// Its place along the edge from its start.
    pub part: u32,
    /// Its range on the curve ([`Joined::curves`] if refit there, else
    /// the input's).
    pub t: [f64; 2],
    /// Its ends at `t[0]` and `t[1]`.
    pub ends: [PieceEnd; 2],
    /// Whether its curve was refit.
    pub refit: bool,
    /// Its origin.
    pub origin: Origin<P, C>,
    /// Its tolerance: its coedges' greatest seam bound (the prior
    /// tolerance on the untouched rest).
    pub tol: f64,
    /// The target its tolerance is tested against.
    pub target: f64,
    /// Its coedges, in the input's order.
    pub coedges: Vec<PieceCoedge>,
}

/// A curve the pass refit.
#[derive(Clone, Debug, PartialEq)]
pub struct RefitCurve {
    pub id: CurveId,
    pub curve: Curve3D,
}

/// The join pass's outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined<P, C> {
    /// The refit curves, by id; the edges on them with no moved end
    /// keep their spans' bits.
    pub curves: Vec<RefitCurve>,
    /// The pieces of the edges with a moved end, by edge and part.
    pub pieces: Vec<Piece<P, C>>,
    /// The junctions, by key.
    pub junctions: Vec<JunctionVert>,
}

/// Why the join pass can't be made.
#[derive(Clone, Debug, PartialEq)]
pub enum JoinError {
    /// `ε_seam` or a `b_K` isn't finite and `≥ 0`, or two clusters have
    /// one key.
    Input,
    /// A moved vertex's position or `R_on` isn't finite, or two clusters
    /// move it to different points.
    Moved(VertId),
    /// This curve isn't described, or is described twice.
    Curve(CurveId),
    /// This edge is listed twice, or its range isn't two increasing knots
    /// of its curve, or a p-curve doesn't run over the range, or it has no
    /// coedges, or its tolerance or a seam bound isn't finite and `≥ 0`.
    Edge(EdgeId),
    /// The edges on this curve can't be refit once for all of them
    /// (module docs).
    Shared(CurveId),
    /// This curve's refit chain is refused (a value not finite).
    Refit(CurveId),
    /// A coedge of this edge can't be bounded (a p-curve span in no one
    /// tile, or running outside the curve).
    Seam(EdgeId),
    /// The refit of this piece of this edge lies beyond its target (a
    /// validity failure).
    Fit { edge: EdgeId, part: u32 },
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// The moved-vertex join pass over the moves of `clusters`, on the edges
/// of `input` (module docs), charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn join<P, C>(
    clusters: &[ClusterMoves<'_, C>],
    input: &JoinInput<'_, P, C>,
    work: &mut Work,
) -> Result<Joined<P, C>, JoinError>
where
    P: Clone + Send + Sync,
    C: Ord + Clone + Send + Sync,
{
    if !finite_nonneg(input.eps_seam) {
        return Err(JoinError::Input);
    }
    // The clusters in their order.
    let mut order: Vec<&ClusterMoves<'_, C>> = clusters.iter().collect();
    order.sort_by(|a, b| a.cluster.cmp(&b.cluster));
    if order.windows(2).any(|w| w[0].cluster == w[1].cluster)
        || order.iter().any(|c| !finite_nonneg(c.b_k))
    {
        return Err(JoinError::Input);
    }
    // Their moves, checked per cluster, then merged by vertex.
    let checked = run(work, "snap join: moves", &order, |c, l| {
        units(l, 1 + c.moved.len() as u64)?;
        for m in c.moved {
            if !(m.pos.iter().all(|x| x.is_finite()) && m.r_on.is_none_or(finite_nonneg)) {
                return Err(Stop::Failed(JoinError::Moved(m.vert)));
            }
        }
        Ok(())
    })?;
    debug_assert_eq!(checked.len(), order.len());
    let mut moves: BTreeMap<VertId, Move> = BTreeMap::new();
    for (k, c) in order.iter().enumerate() {
        for m in c.moved {
            match moves.get_mut(&m.vert) {
                Some(have) => {
                    if bits3(have.pos) != bits3(m.pos) {
                        return Err(JoinError::Moved(m.vert));
                    }
                    have.r_on = larger_opt(have.r_on, m.r_on);
                    have.clusters.insert(k);
                }
                None => {
                    moves.insert(
                        m.vert,
                        Move {
                            pos: m.pos,
                            r_on: m.r_on,
                            clusters: BTreeSet::from([k]),
                        },
                    );
                }
            }
        }
    }

    // The curves and the edges on each, by id.
    let mut curves: BTreeMap<CurveId, &Curve3D> = BTreeMap::new();
    for c in input.curves {
        if curves.insert(c.id, c.curve).is_some() {
            return Err(JoinError::Curve(c.id));
        }
    }
    let mut edges: Vec<&JoinEdge<'_, P, C>> = input.edges.iter().collect();
    edges.sort_by_key(|e| e.edge);
    if let Some(w) = edges.windows(2).find(|w| w[0].edge == w[1].edge) {
        return Err(JoinError::Edge(w[0].edge));
    }
    let knots = run(work, "snap join: edges", &edges, |e, l| {
        units(l, 1 + e.coedges.len() as u64)?;
        let curve = curves
            .get(&e.curve)
            .ok_or(Stop::Failed(JoinError::Curve(e.curve)))?;
        edge_knots(e, curve).ok_or(Stop::Failed(JoinError::Edge(e.edge)))
    })?;
    let mut on_curve: BTreeMap<CurveId, Vec<OnCurve<'_, P, C>>> = BTreeMap::new();
    for (e, k) in edges.iter().zip(knots) {
        on_curve
            .entry(e.curve)
            .or_default()
            .push(OnCurve { edge: e, knots: k });
    }
    // The curves with an edge with a moved end.
    let jobs: Vec<CurveJob<'_, '_, P, C>> = on_curve
        .into_iter()
        .filter(|(_, es)| {
            es.iter()
                .any(|e| e.edge.ends.iter().any(|v| moves.contains_key(v)))
        })
        .map(|(id, es)| (id, curves[&id], es))
        .collect();
    let ctx = Ctx {
        moves: &moves,
        order: &order,
        eps_seam: input.eps_seam,
    };
    let done = run(work, "snap join: refits", &jobs, |(id, curve, es), l| {
        ctx.curve(*id, curve, es, l)
    })?;
    let mut out = Joined {
        curves: Vec::new(),
        pieces: Vec::new(),
        junctions: Vec::new(),
    };
    for d in done {
        out.curves.push(d.curve);
        out.pieces.extend(d.pieces);
        out.junctions.extend(d.junctions);
    }
    out.pieces.sort_by_key(|p| (p.edge, p.part));
    out.junctions.sort_by_key(|j| j.key);
    Ok(out)
}

/// A vertex's move, merged over the clusters.
struct Move {
    pos: [f64; 3],
    r_on: Option<f64>,
    /// The clusters moving it, as positions in their order.
    clusters: BTreeSet<usize>,
}

/// An edge on its curve: the knots of its range's ends.
struct OnCurve<'e, P, C> {
    edge: &'e JoinEdge<'e, P, C>,
    knots: [usize; 2],
}

/// A curve with an edge with a moved end, and the edges on it.
type CurveJob<'c, 'e, P, C> = (CurveId, &'c Curve3D, Vec<OnCurve<'e, P, C>>);

/// The pass's shared reads.
struct Ctx<'c, 'm, C> {
    moves: &'c BTreeMap<VertId, Move>,
    order: &'c [&'c ClusterMoves<'m, C>],
    eps_seam: f64,
}

/// A curve's refit and its edges' pieces.
struct CurveDone<P, C> {
    curve: RefitCurve,
    pieces: Vec<Piece<P, C>>,
    junctions: Vec<JunctionVert>,
}

/// An edge's plan on its curve: its pieces as knot ranges, and the refit
/// intervals in each.
struct Plan {
    /// Per piece: its knots, whether refit, its intervals (knots).
    pieces: Vec<([usize; 2], bool, Vec<[usize; 2]>)>,
}

type Res<T> = Result<T, Stop<JoinError>>;

impl<C: Ord + Clone> Ctx<'_, '_, C> {
    /// The refit of curve `id` and its edges `es` (module docs).
    fn curve<P: Clone>(
        &self,
        id: CurveId,
        curve: &Curve3D,
        es: &[OnCurve<'_, P, C>],
        ledger: &mut Ledger<'_>,
    ) -> Res<CurveDone<P, C>> {
        units(ledger, 1 + (es.len() + curve.spans().len()) as u64)?;
        let shared = || Stop::Failed(JoinError::Shared(id));
        let ts = curve.knots();
        // The move at each knot an edge ends at, alike on all of them.
        let mut at: BTreeMap<usize, Option<&Move>> = BTreeMap::new();
        for e in es {
            for (k, v) in e.knots.iter().zip(e.edge.ends) {
                let m = self.moves.get(&v);
                match at.get(k) {
                    Some(have) if !same_move(*have, m) => return Err(shared()),
                    Some(_) => {}
                    None => {
                        at.insert(*k, m);
                    }
                }
            }
        }
        // No edge runs through a moved knot.
        for e in es {
            let [a, b] = e.knots;
            if at.range(a + 1..b).any(|(_, m)| m.is_some()) {
                return Err(shared());
            }
        }
        // A knot where every edge holding it has every p-curve's knot.
        let good = |k: usize| {
            let t = ts[k];
            es.iter()
                .filter(|e| e.knots[0] <= k && k <= e.knots[1])
                .all(|e| e.edge.coedges.iter().all(|c| has_knot(c.pcurve.knots(), t)))
        };
        // Each moved edge's plan.
        let mut plans: Vec<Option<Plan>> = Vec::with_capacity(es.len());
        let mut intervals: BTreeSet<[usize; 2]> = BTreeSet::new();
        for e in es {
            let [a, b] = e.knots;
            let (ms, me) = (at[&a].is_some(), at[&b].is_some());
            if !(ms || me) {
                plans.push(None);
                continue;
            }
            let goods: Vec<usize> = (a + 1..b).filter(|&k| good(k)).collect();
            let plan = match (ms, me, goods.first(), goods.last()) {
                (true, true, None, _) => Plan {
                    pieces: vec![([a, b], true, vec![[a, b]])],
                },
                (true, true, Some(&f), Some(&l)) if f == l => Plan {
                    pieces: vec![([a, b], true, vec![[a, f], [f, b]])],
                },
                (true, true, Some(&f), Some(&l)) => Plan {
                    pieces: vec![
                        ([a, f], true, vec![[a, f]]),
                        ([f, l], false, Vec::new()),
                        ([l, b], true, vec![[l, b]]),
                    ],
                },
                (true, false, f, _) => {
                    let f = f.copied().unwrap_or(b);
                    let mut p = vec![([a, f], true, vec![[a, f]])];
                    if f < b {
                        p.push(([f, b], false, Vec::new()));
                    }
                    Plan { pieces: p }
                }
                (false, true, _, l) => {
                    let l = l.copied().unwrap_or(a);
                    let mut p = Vec::new();
                    if a < l {
                        p.push(([a, l], false, Vec::new()));
                    }
                    p.push(([l, b], true, vec![[l, b]]));
                    Plan { pieces: p }
                }
                _ => return Err(shared()),
            };
            for (_, _, ivs) in &plan.pieces {
                intervals.extend(ivs.iter().copied());
            }
            plans.push(Some(plan));
        }
        // No edge ends inside an interval, and every interval
        // overlapping an edge is one of its own.
        if intervals
            .iter()
            .any(|iv| at.range(iv[0] + 1..iv[1]).next().is_some())
        {
            return Err(shared());
        }
        for (e, plan) in es.iter().zip(&plans) {
            let [a, b] = e.knots;
            let own: BTreeSet<[usize; 2]> = plan
                .iter()
                .flat_map(|p| p.pieces.iter().flat_map(|x| x.2.iter().copied()))
                .collect();
            for iv in &intervals {
                let overlaps = iv[0] < b && a < iv[1];
                if overlaps && !own.contains(iv) {
                    return Err(shared());
                }
            }
        }

        // The new knot points and spans.
        let old_point = |k: usize| curve.knot_point(k).ok_or_else(shared);
        let mut points: BTreeMap<usize, [f64; 3]> = BTreeMap::new();
        let mut spans: Vec<Span3> = curve.spans().to_vec();
        for &[lo, hi] in &intervals {
            units(ledger, (hi - lo) as u64)?;
            let disp = |k: usize| -> Res<P3> {
                Ok(match at.get(&k).copied().flatten() {
                    Some(m) => sub3(&exact3(m.pos), &exact3(old_point(k)?)),
                    None => zero3(),
                })
            };
            let (d_lo, d_hi) = (disp(lo)?, disp(hi)?);
            let (t_lo, t_hi) = (q(ts[lo]), q(ts[hi]));
            let width = &t_hi - &t_lo;
            // The displacement at knot `k`, linear in `t`.
            let d_at = |k: usize| -> P3 {
                let s = (q(ts[k]) - &t_lo) / &width;
                std::array::from_fn(|c| &d_lo[c] + &s * (&d_hi[c] - &d_lo[c]))
            };
            let ds: Vec<P3> = (lo..=hi).map(d_at).collect();
            for k in lo..=hi {
                let p = match at.get(&k).copied().flatten() {
                    Some(m) => m.pos,
                    None if k == lo || k == hi => old_point(k)?,
                    None => round3(&add3(&exact3(old_point(k)?), &ds[k - lo])),
                };
                match points.get(&k) {
                    Some(have) if bits3(*have) != bits3(p) => return Err(shared()),
                    _ => {
                        points.insert(k, p);
                    }
                }
            }
            for k in lo..hi {
                let s = &curve.spans()[k];
                let half = Q::from_signeds(1, 2);
                let mid: P3 =
                    std::array::from_fn(|c| (&ds[k - lo][c] + &ds[k + 1 - lo][c]) * &half);
                let m = round3(&add3(&exact3([s[1][0], s[1][1], s[1][2]]), &mid));
                let (p0, p2) = (points[&k], points[&(k + 1)]);
                spans[k] = [
                    [p0[0], p0[1], p0[2], s[0][3]],
                    [m[0], m[1], m[2], s[1][3]],
                    [p2[0], p2[1], p2[2], s[2][3]],
                ];
            }
        }
        let refit =
            Curve3D::new(spans, ts.to_vec()).map_err(|_| Stop::Failed(JoinError::Refit(id)))?;

        // The pieces.
        let mut pieces = Vec::new();
        let mut junctions = Vec::new();
        for (e, plan) in es.iter().zip(plans) {
            let Some(plan) = plan else {
                continue;
            };
            let edge = e.edge;
            let [a, b] = e.knots;
            // A junction is nearer the start where the refit piece
            // before it is the first.
            let end_at = |k: usize| -> PieceEnd {
                if k == a {
                    PieceEnd::Vert(edge.ends[0])
                } else if k == b {
                    PieceEnd::Vert(edge.ends[1])
                } else {
                    let first = &plan.pieces[0];
                    let near = if first.1 && first.0[1] == k {
                        Near::Start
                    } else {
                        Near::End
                    };
                    PieceEnd::Junction(Junction {
                        edge: edge.edge,
                        near,
                    })
                }
            };
            for (part, (range, is_refit, ivs)) in plan.pieces.iter().enumerate() {
                let part = part as u32;
                let [lo, hi] = *range;
                let t = [ts[lo], ts[hi]];
                for k in [lo, hi] {
                    if let PieceEnd::Junction(key) = end_at(k)
                        && !junctions.iter().any(|j: &JunctionVert| j.key == key)
                    {
                        junctions.push(JunctionVert {
                            key,
                            pos: refit.knot_point(k).ok_or_else(shared)?,
                            on: (id, ts[k]),
                        });
                    }
                }
                let ends = [end_at(lo), end_at(hi)];
                let mut coedges = Vec::with_capacity(edge.coedges.len());
                if !*is_refit {
                    for c in edge.coedges {
                        let pcurve = c
                            .pcurve
                            .slice(t[0], t[1])
                            .ok_or(Stop::Failed(JoinError::Edge(edge.edge)))?;
                        coedges.push(PieceCoedge {
                            face: c.face,
                            pcurve,
                            seam: c.seam,
                        });
                    }
                    pieces.push(Piece {
                        edge: edge.edge,
                        part,
                        t,
                        ends,
                        refit: false,
                        origin: edge.origin.clone(),
                        tol: edge.tol,
                        target: edge.tol,
                        coedges,
                    });
                    continue;
                }
                // The clusters moving its ends, and their largest `b_K`.
                let mut ks: BTreeSet<usize> = BTreeSet::new();
                for k in [lo, hi] {
                    if let Some(m) = at.get(&k).copied().flatten() {
                        ks.extend(m.clusters.iter().copied());
                    }
                }
                let b_k = ks.iter().map(|&k| self.order[k].b_k).fold(0.0, larger);
                // Each interval's target.
                let targets: Vec<f64> = ivs
                    .iter()
                    .map(|iv| {
                        let term = iv
                            .iter()
                            .filter_map(|k| at.get(k).copied().flatten())
                            .filter_map(|m| m.r_on)
                            .fold(b_k, larger);
                        larger(edge.tol, add_up(term, self.eps_seam))
                    })
                    .collect();
                let mut tol = 0.0;
                for c in edge.coedges {
                    let mut seam = 0.0;
                    for (iv, &target) in ivs.iter().zip(&targets) {
                        let (t0, t1) = (ts[iv[0]], ts[iv[1]]);
                        let (Some(cv), Some(pc)) = (refit.slice(t0, t1), c.pcurve.slice(t0, t1))
                        else {
                            return Err(Stop::Failed(JoinError::Edge(edge.edge)));
                        };
                        let bound = seam_bound(c.surface, &cv, &pc, target, ledger)
                            .map_err(|s| seam_stop(s, edge.edge))?
                            .bound;
                        // Not `>`: a NaN bound fails too.
                        if !matches!(
                            bound.partial_cmp(&target),
                            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                        ) {
                            return Err(Stop::Failed(JoinError::Fit {
                                edge: edge.edge,
                                part,
                            }));
                        }
                        seam = larger(seam, bound);
                    }
                    tol = larger(tol, seam);
                    coedges.push(PieceCoedge {
                        face: c.face,
                        pcurve: c
                            .pcurve
                            .slice(t[0], t[1])
                            .ok_or(Stop::Failed(JoinError::Edge(edge.edge)))?,
                        seam,
                    });
                }
                let new: Vec<C> = ks.iter().map(|&k| self.order[k].cluster.clone()).collect();
                let origin = match &edge.origin {
                    Origin::Plain(p) => Origin::MovedJoin {
                        prior: p.clone(),
                        clusters: new,
                    },
                    Origin::MovedJoin { prior, clusters } => {
                        let all: BTreeSet<C> = clusters.iter().cloned().chain(new).collect();
                        Origin::MovedJoin {
                            prior: prior.clone(),
                            clusters: all.into_iter().collect(),
                        }
                    }
                };
                pieces.push(Piece {
                    edge: edge.edge,
                    part,
                    t,
                    ends,
                    refit: true,
                    origin,
                    tol,
                    target: targets.iter().copied().fold(0.0, larger),
                    coedges,
                });
            }
        }
        Ok(CurveDone {
            curve: RefitCurve { id, curve: refit },
            pieces,
            junctions,
        })
    }
}

/// The knots of edge `e`'s range on `curve`, if it is two increasing
/// knots, every p-curve runs over it, it has a coedge, and its values are
/// finite and `≥ 0`.
fn edge_knots<P, C>(e: &JoinEdge<'_, P, C>, curve: &Curve3D) -> Option<[usize; 2]> {
    let ks = curve.knots();
    let find = |t: f64| ks.iter().position(|k| k.to_bits() == t.to_bits());
    let (a, b) = (find(e.range[0])?, find(e.range[1])?);
    let fits = |c: &JoinCoedge<'_>| {
        let pk = c.pcurve.knots();
        finite_nonneg(c.seam)
            && pk.first().map(|t| t.to_bits()) == Some(e.range[0].to_bits())
            && pk.last().map(|t| t.to_bits()) == Some(e.range[1].to_bits())
    };
    (a < b && finite_nonneg(e.tol) && !e.coedges.is_empty() && e.coedges.iter().all(fits))
        .then_some([a, b])
}

/// Whether `t` is one of `knots`, bit for bit.
fn has_knot(knots: &[f64], t: f64) -> bool {
    knots.iter().any(|k| k.to_bits() == t.to_bits())
}

/// Whether two moves at one knot agree: both none, or one point.
fn same_move(a: Option<&Move>, b: Option<&Move>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => bits3(x.pos) == bits3(y.pos),
        _ => false,
    }
}

/// A seam bound's stop as the pass's.
fn seam_stop(s: Stop<SeamError>, edge: EdgeId) -> Stop<JoinError> {
    match s {
        Stop::Exhausted(e) => Stop::Exhausted(e),
        Stop::Cap(c) => Stop::Cap(c),
        Stop::Failed(_) => Stop::Failed(JoinError::Seam(edge)),
    }
}

fn finite_nonneg(x: f64) -> bool {
    x.is_finite() && x >= 0.0
}

/// The larger of `a` and `b`, by comparison (no `f64::max`).
fn larger(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

fn larger_opt(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(larger(x, y)),
        (x, None) => x,
        (None, y) => y,
    }
}

fn bits3(p: [f64; 3]) -> [u64; 3] {
    p.map(f64::to_bits)
}

fn zero3() -> P3 {
    std::array::from_fn(|_| Q::from(0u32))
}

fn add3(a: &P3, b: &P3) -> P3 {
    std::array::from_fn(|k| &a[k] + &b[k])
}

fn round3(p: &P3) -> [f64; 3] {
    p.each_ref().map(to_f64)
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, JoinError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<JoinError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => JoinError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<JoinError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

#[cfg(test)]
mod tests;
