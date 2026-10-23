//! The snap's rewrite, its geometry: where each vertex of the rewrite
//! lands, and each edge's curve and its coedges' p-curves, on flat
//! targets (each target face a plane, [`TargetFace`]) with the map along
//! the common direction `ℓ` ([`Map::Along`]).
//!
//! **Edges** ([`EdgeClass`]), each from the faces using it and the
//! records:
//! - **reused target edges**: a target edge's piece in a face of `S`
//!   (inside the region, or a rim piece along it), and a piece of an `S`
//!   edge inside a `SnapAlong` piece of it along a target edge (merged
//!   into that edge). Each reuses the target edge's chain and its
//!   p-curves on the target faces **bit for bit**
//!   ([`EdgeCurve::Reused`], [`Chain`]), over the parameters its end
//!   vertices store on it; only a coedge on a face of `S` gets a p-curve of
//!   its own: the target edge carried into that face's UV by the caller
//!   (its trim or [`TargetCut`]), on the same parameter;
//! - **snapped edges** (`S` edges inside the rim) and **`S` edges along
//!   the rim**: on the target, `p_T` the straight line between the end
//!   vertices' UVs on the target face, `C` the line between their
//!   positions, fitted to `T∘p_T` within `ε_seam/2` (its bound, exactly:
//!   both are quadratics, their difference within its control points'
//!   hull; beyond it, [`RewriteError::Fit`]); on a kept face, the `S`
//!   edge's line in that face's UV;
//! - **rim edges** along box sides: the outer coedge exact on `S`, the
//!   rim piece's iso line in its face's UV; `C` the iso curve of `S`'s
//!   tile between the ends carried halfway to the target along `ℓ` (the
//!   mid-surface; exact, a rational quadratic, as the map is affine);
//!   the inner coedge that curve carried onto the target, in the target
//!   face's UV;
//! - **kept** pieces of `S` edges keep their curve: the join pass refits
//!   those whose ends moved;
//! - the **zero-length** edges between coincident rim vertices
//!   ([`Corner::zero_edges`]), their ends bitwise one point.
//!
//! **Vertices.** A vertex at an end of a reused edge **stores its
//! parameter** on that target edge ([`VertGeom::on`]) and lies at the
//! chain's canonical point there ([`Curve3D::point_at`]), on every edge
//! reusing it: a target vertex at its end of the range; else the
//! parameter the corner rule or the `SnapAlong` piece placed it at (the
//! fraction along the target edge from its start); else where it lies on
//! the target edge carried into a face of `S`; else the clamped foot of
//! its pre-snap point. The target edge's chain is split at the stored
//! parameters inside its range ([`Curve3D::split`]): its new knots carry
//! the canonical points, so a vertex's position is bitwise its knot's,
//! and the other operand's edge split from the same parameter
//! ([`Chain::splits`]) gets the same bits. Two reused edges putting one
//! vertex at different points: [`RewriteError::Shared`]. Every other
//! vertex: a target vertex stays; one the corner rule (or a bracket end)
//! pins goes there; a rim vertex (and a branch vertex) whose edges are
//! rim edges along box sides goes to the sheets' mid-point, its point of
//! `S` carried halfway along `ℓ`; one that also bounds an edge on the
//! target (a snapped edge, an `S` edge along the rim), and an `S` vertex
//! inside the rim, to its image on the target, where that edge's fit
//! holds; the rest stay. Each vertex's displacement is bounded again
//! from where it lands ([`VertGeom::displacement`]): a curve point of a
//! reused edge wins over a placement off that edge. A vertex's
//! UV on a target face is the target edge's p-curve's there where it
//! stores a parameter on one, else its position carried onto the face's
//! plane; on a face of `S`, the carried target edge's, else its exact UV
//! rounded.
//!
//! Every value is exact (rational) from the stored `f64`s and rounded to
//! the nearest `f64` once.
//!
//! **Work.** Charged on the operation's budget in passes: the edges'
//! classes (an item per edge, a unit per `SnapAlong` pair read), the
//! vertices (an item each), the chains (an item per reused target edge)
//! and the edges (an item each), a unit per value read.

use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use varde_decide::bezier::HomPatch;
use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::order::EntityOrder;
use varde_decide::records::keys::Operand;

use super::super::along::SnapAlong;
use super::super::corner::{Corner, Placement, d2, dist_up, exact3, point_on, point_seg, to_f64};
use super::super::rim::{self, ExactSpan, Rim, RimInput, RimTrim};
use super::super::target::Map;
use super::{
    NewEdge, NewVert, P2, Pre, PreImage, Q, Rewrite, RewriteError, SnapEdge, SnapFace, SnapVert,
    TargetCut, param_in, run, units,
};
use crate::geom::curve::{Curve3D, Span3};
use crate::geom::pcurve::{PCurve, Span2};
use crate::infra::budget::{Ledger, Stop, Work};

/// A point in space, exactly.
type P3 = [Q; 3];
/// A face using an edge: its key and the face whose surface it lies on.
type FaceUse = (SnapFace, (Operand, FaceId));

/// A target face's surface: its tile, a plane, in its global UV.
#[derive(Clone, Copy, Debug)]
pub struct TargetFace<'a> {
    /// The face.
    pub face: FaceId,
    /// The tile, and its patch, exact: affine (its net on the plane's
    /// grid, its weights one value).
    pub tile: [i64; 2],
    pub patch: &'a HomPatch<Rational>,
}

/// A target edge the rewrite may reuse.
#[derive(Clone, Copy, Debug)]
pub struct TargetEdge<'a> {
    /// The edge.
    pub edge: EdgeId,
    /// Its curve.
    pub curve: &'a Curve3D,
    /// Its parameter range, increasing, from its first end vertex
    /// ([`EntityOrder::ends`]) to its second: two knots of the curve.
    pub range: [f64; 2],
    /// Its coedges' p-curves on the target faces, over its range.
    pub pcurves: &'a [(FaceId, PCurve)],
}

/// What [`geometry`] reads besides the rewrite and its inputs.
#[derive(Clone, Copy)]
pub struct GeomInput<'a, O> {
    /// The operands' vertices: their positions (their keys') and the
    /// edges' ends.
    pub order: &'a O,
    /// The cluster's map ([`Map::Along`]).
    pub map: &'a Map,
    /// The target faces the cluster reaches.
    pub faces: &'a [TargetFace<'a>],
    /// The target edges the rewrite may reuse.
    pub edges: &'a [TargetEdge<'a>],
    /// The seam target `ε_seam`: a fitted curve is within half of it.
    pub eps_seam: f64,
}

/// What an edge of the rewrite is (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeClass {
    /// A piece of an `S` edge outside the rim: its curve kept.
    Kept,
    /// A rim edge along box sides.
    Rim,
    /// An `S` edge inside the rim.
    Snapped,
    /// An `S` edge along the rim.
    AlongRim,
    /// A target edge's piece.
    Target,
    /// A piece of an `S` edge merged into this target edge (a `SnapAlong`
    /// piece).
    Merged(EdgeId),
    /// A zero-length edge.
    Zero,
}

/// An edge's curve.
#[derive(Clone, Debug, PartialEq)]
pub enum EdgeCurve {
    /// Its `S` edge's own, over its pre-image's range.
    Kept,
    /// The chain of this target edge ([`Geometry::chains`]), from `t[0]` at
    /// the edge's start to `t[1]` at its end.
    Reused { edge: EdgeId, t: [f64; 2] },
    /// A new chain over `[0, 1]`, from the edge's start to its end.
    New(Curve3D),
    /// None: the edge has zero length.
    Point,
}

/// A coedge's p-curve: on the face's surface, over its edge's parameter
/// (a reused edge's increasing range, else `[0, 1]`), whichever way the
/// face uses it.
#[derive(Clone, Debug, PartialEq)]
pub struct Coedge {
    /// The face using the edge.
    pub face: SnapFace,
    /// The p-curve, in that face's global UV.
    pub pcurve: PCurve,
}

/// An edge's geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeGeom {
    /// Its key.
    pub key: SnapEdge,
    /// What it is.
    pub class: EdgeClass,
    /// Its curve.
    pub curve: EdgeCurve,
    /// Its coedges' p-curves, in the order of the faces' keys (none for a
    /// kept or zero-length edge).
    pub coedges: Vec<Coedge>,
    /// A fitted curve's bound from the target composed with its p-curves
    /// there (mm); `0` otherwise.
    pub fit: f64,
}

/// A vertex's geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct VertGeom {
    /// Its key.
    pub key: SnapVert,
    /// Its position.
    pub pos: [f64; 3],
    /// The parameters it stores on the target edges it lies on.
    pub on: Vec<(EdgeId, f64)>,
    /// An upper bound of its distance from its pre-snap position to
    /// [`pos`](Self::pos) (`0` for a target vertex): where a reused
    /// edge's curve point wins over the corner rule's placement, this, not
    /// [`Moved::displacement`](crate::boolean::snap::corner::Moved::displacement), bounds the move.
    pub displacement: f64,
}

/// A reused target edge's chain, split at the parameters the vertices
/// store on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    /// The target edge.
    pub edge: EdgeId,
    /// Its curve, split.
    pub curve: Curve3D,
    /// Its p-curves on the target faces, split there too.
    pub pcurves: Vec<(FaceId, PCurve)>,
    /// The parameters inside its range where it is split, increasing,
    /// each with the vertex there: the other operand's edge is split there
    /// too, from the same parameter.
    pub splits: Vec<(f64, SnapVert)>,
}

/// The rewrite's geometry (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    /// The vertices, by key.
    pub verts: Vec<VertGeom>,
    /// The edges, by key.
    pub edges: Vec<EdgeGeom>,
    /// The reused target edges' chains, by id.
    pub chains: Vec<Chain>,
}

/// The geometry of `rewrite`, the rewrite of the cluster of `rim` (made
/// from `rim_input`) with the corner rule's decisions `corner`, the
/// `SnapAlong` records `along` and the target edges `cuts` (module docs),
/// charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
#[expect(clippy::too_many_arguments, reason = "the snap's decisions so far")]
pub(crate) fn geometry<O: EntityOrder + Sync>(
    rewrite: &Rewrite,
    rim: &Rim,
    rim_input: &RimInput<'_>,
    corner: &Corner,
    along: &SnapAlong,
    cuts: &[TargetCut<'_>],
    input: &GeomInput<'_, O>,
    work: &mut Work,
) -> Result<Geometry, RewriteError> {
    let Map::Along(ell) = input.map else {
        return Err(RewriteError::Map);
    };
    if !(input.eps_seam.is_finite() && input.eps_seam >= 0.0) {
        return Err(RewriteError::Map);
    }
    let s_op = rim_input.snapped;
    let mut charts = BTreeMap::new();
    for f in input.faces {
        charts.insert(f.face, chart(f).ok_or(RewriteError::Chart(f.face))?);
    }
    let mut targets = BTreeMap::new();
    for e in input.edges {
        if !(e.range.iter().all(|t| t.is_finite())
            && e.range[0].partial_cmp(&e.range[1]) == Some(std::cmp::Ordering::Less))
        {
            return Err(RewriteError::Reuse(e.edge));
        }
        targets.insert(e.edge, *e);
    }
    // The target edges carried into each face of `S`, exactly.
    let mut carried: BTreeMap<(FaceId, EdgeId), Carried<'_>> = BTreeMap::new();
    for f in rim_input.faces {
        let trims: Vec<RimTrim<'_>> = f
            .target
            .iter()
            .copied()
            .chain(cuts.iter().filter(|c| c.face == f.face).map(|c| c.trim()))
            .collect();
        let exact = rim::exact_spans(f.face, &trims).map_err(|_| RewriteError::Trim(f.face))?;
        for (t, ex) in trims.into_iter().zip(exact) {
            carried
                .entry((f.face, t.edge))
                .or_insert(Carried { trim: t, exact: ex });
        }
    }
    // The faces using each edge, and the edges at each vertex.
    let mut uses: BTreeMap<SnapEdge, Vec<FaceUse>> = BTreeMap::new();
    for f in &rewrite.faces {
        for l in &f.loops {
            for u in l {
                let v = uses.entry(u.edge).or_default();
                if !v.iter().any(|x| x.0 == f.key) {
                    v.push((f.key, f.on));
                }
            }
        }
    }
    for v in uses.values_mut() {
        v.sort_by_key(|x| x.0);
    }
    let mut at_vert: BTreeMap<SnapVert, Vec<SnapEdge>> = BTreeMap::new();
    for e in &rewrite.edges {
        for v in e.ends {
            at_vert.entry(v).or_default().push(e.key);
        }
    }
    let ctx = Geo {
        rewrite,
        rim,
        rim_input,
        corner,
        along,
        input,
        ell,
        s_op,
        charts,
        targets,
        carried,
        uses,
        at_vert,
    };
    // The edges' classes.
    let classes: BTreeMap<SnapEdge, EdgeClass> = run(
        work,
        "snap rewrite: edge classes",
        &rewrite.edges,
        |e, l| {
            units(l, 1 + along.pairs.len() as u64)?;
            Ok((e.key, ctx.class(e)))
        },
    )?
    .into_iter()
    .collect();

    // The vertices.
    let verts = run(
        work,
        "snap rewrite: vertex geometry",
        &rewrite.verts,
        |v, l| ctx.vertex(v, &classes, 0, l),
    )?;
    let by_key: BTreeMap<SnapVert, &VertGeom> = verts.iter().map(|v| (v.key, v)).collect();

    // The chains, split at the parameters stored inside their ranges.
    let mut inner: BTreeMap<EdgeId, BTreeMap<u64, (f64, SnapVert)>> = BTreeMap::new();
    for e in &rewrite.edges {
        if let Some(g) = reused(classes[&e.key], e.key) {
            inner.entry(g).or_default();
        }
    }
    for v in &verts {
        for &(g, t) in &v.on {
            let Some(r) = ctx.targets.get(&g).map(|e| e.range) else {
                continue;
            };
            if let Some(ts) = inner.get_mut(&g)
                && r[0] < t
                && t < r[1]
            {
                ts.entry(t.to_bits()).or_insert((t, v.key));
            }
        }
    }
    let jobs: Vec<(EdgeId, Vec<(f64, SnapVert)>)> = inner
        .into_iter()
        .map(|(g, ts)| {
            let mut v: Vec<(f64, SnapVert)> = ts.into_values().collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            (g, v)
        })
        .collect();
    let chains = run(work, "snap rewrite: chains", &jobs, |(g, splits), l| {
        ctx.chain(*g, splits, l)
    })?;
    let chain_of: BTreeMap<EdgeId, &Chain> = chains.iter().map(|c| (c.edge, c)).collect();

    // The edges.
    let edges = run(
        work,
        "snap rewrite: edge geometry",
        &rewrite.edges,
        |e, l| ctx.edge(e, classes[&e.key], &by_key, &chain_of, l),
    )?;
    Ok(Geometry {
        verts,
        edges,
        chains,
    })
}

/// The target edge a reused edge of class `class` reuses.
fn reused(class: EdgeClass, key: SnapEdge) -> Option<EdgeId> {
    match (class, key) {
        (EdgeClass::Merged(g), _) => Some(g),
        (EdgeClass::Target, SnapEdge::Cut { edge, .. }) => Some(edge),
        _ => None,
    }
}

/// A target face's plane: `T(u, v) = o + (u − i)·a + (v − j)·b` at global
/// `(u, v)`, `(i, j)` its tile.
pub(in crate::boolean::snap) struct Chart {
    tile: [i64; 2],
    o: P3,
    a: P3,
    b: P3,
}

/// The chart of `f`, if its patch is affine with one weight.
pub(in crate::boolean::snap) fn chart(f: &TargetFace<'_>) -> Option<Chart> {
    let h = f.patch.tensors();
    let (m, n) = f.patch.degrees();
    if m == 0 || n == 0 {
        return None;
    }
    let w = h[3].get(0, 0).clone();
    if w <= 0u32 {
        return None;
    }
    let pt = |i: usize, j: usize| -> Option<P3> {
        (*h[3].get(i, j) == w).then(|| [0, 1, 2].map(|k| h[k].get(i, j) / &w))
    };
    let o = pt(0, 0)?;
    let a = sub3q(&pt(m, 0)?, &o);
    let b = sub3q(&pt(0, n)?, &o);
    for i in 0..=m {
        for j in 0..=n {
            let (s, t) = (Q::from(i) / Q::from(m), Q::from(j) / Q::from(n));
            let want: P3 = [0, 1, 2].map(|k| &o[k] + &s * &a[k] + &t * &b[k]);
            if pt(i, j)? != want {
                return None;
            }
        }
    }
    (cross3q(&a, &b) != [Q::from(0u32), Q::from(0u32), Q::from(0u32)]).then_some(Chart {
        tile: f.tile,
        o,
        a,
        b,
    })
}

/// The plane of target face `f`, exactly: a point of it and a normal
/// (nonzero), if its patch is affine with one weight.
pub(in crate::boolean::snap) fn plane(f: &TargetFace<'_>) -> Option<([Q; 3], [Q; 3])> {
    let c = chart(f)?;
    let n = cross3q(&c.a, &c.b);
    Some((c.o, n))
}

impl Chart {
    /// The point at global `uv`.
    pub(in crate::boolean::snap) fn point(&self, uv: &[Q; 2]) -> P3 {
        let u = &uv[0] - Q::from(self.tile[0]);
        let v = &uv[1] - Q::from(self.tile[1]);
        [0, 1, 2].map(|k| &self.o[k] + &u * &self.a[k] + &v * &self.b[k])
    }

    /// `p` carried along `ell` onto the plane: its global UV there and
    /// the multiple `λ` of `ell` it moves by; `None` where `ell` lies in
    /// the plane.
    fn along(&self, p: &P3, ell: &P3) -> Option<([Q; 2], Q)> {
        // u·a + v·b − λ·ℓ = p − o, by Cramer's rule.
        let r = sub3q(p, &self.o);
        let m = |x: &P3, y: &P3, z: &P3| dot3q(x, &cross3q(y, z));
        let nl = ell.clone().map(|x| -x);
        let det = m(&self.a, &self.b, &nl);
        if det == 0u32 {
            return None;
        }
        let u = m(&r, &self.b, &nl) / &det;
        let v = m(&self.a, &r, &nl) / &det;
        let lambda = m(&self.a, &self.b, &r) / &det;
        Some((
            [u + Q::from(self.tile[0]), v + Q::from(self.tile[1])],
            lambda,
        ))
    }
}

fn sub3q(a: &P3, b: &P3) -> P3 {
    [&a[0] - &b[0], &a[1] - &b[1], &a[2] - &b[2]]
}

fn dot3q(a: &P3, b: &P3) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn cross3q(a: &P3, b: &P3) -> P3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

/// `p + s·ℓ`.
fn moved(p: &P3, ell: &P3, s: &Q) -> P3 {
    [0, 1, 2].map(|k| &p[k] + s * &ell[k])
}

fn round3(p: &P3) -> [f64; 3] {
    p.each_ref().map(to_f64)
}

fn round2(p: &[Q; 2]) -> [f64; 2] {
    p.each_ref().map(to_f64)
}

/// The rounded midpoint of `a` and `b`, if both are finite.
fn midpoint<const N: usize>(a: [f64; N], b: [f64; N]) -> Option<[f64; N]> {
    let mut m = [0.0; N];
    for k in 0..N {
        let s = Q::try_from(a[k]).ok()? + Q::try_from(b[k]).ok()?;
        m[k] = to_f64(&(s / Q::from(2u32)));
    }
    Some(m)
}

/// The line span from `a` to `b`, its middle control point the rounded
/// midpoint, weights 1; `None` unless both are finite.
fn line(a: [f64; 3], b: [f64; 3]) -> Option<Span3> {
    let m = midpoint(a, b)?;
    Some([
        [a[0], a[1], a[2], 1.0],
        [m[0], m[1], m[2], 1.0],
        [b[0], b[1], b[2], 1.0],
    ])
}

/// The UV line span from `a` to `b`, as [`line()`].
fn uv_line(a: [f64; 2], b: [f64; 2]) -> Option<Span2> {
    let m = midpoint(a, b)?;
    Some([[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]])
}

/// The exact value of a finite `f64` (one checked on the way in).
fn exact1(x: f64) -> Q {
    Q::try_from(x).expect("a finite value")
}

/// A target edge carried into a face of `S`.
struct Carried<'a> {
    trim: RimTrim<'a>,
    exact: Vec<ExactSpan>,
}

impl Carried<'_> {
    /// Its p-curve in the face's UV, on the edge's parameter.
    fn pcurve(&self) -> Option<PCurve> {
        PCurve::new(self.trim.spans.to_vec(), self.trim.knots.to_vec()).ok()
    }
}

/// A vertex's position and the parameters it stores, while it is found.
struct At {
    pos: [f64; 3],
    on: Vec<(EdgeId, f64)>,
}

struct Geo<'a, O> {
    rewrite: &'a Rewrite,
    rim: &'a Rim,
    rim_input: &'a RimInput<'a>,
    corner: &'a Corner,
    along: &'a SnapAlong,
    input: &'a GeomInput<'a, O>,
    ell: &'a P3,
    s_op: Operand,
    charts: BTreeMap<FaceId, Chart>,
    targets: BTreeMap<EdgeId, TargetEdge<'a>>,
    carried: BTreeMap<(FaceId, EdgeId), Carried<'a>>,
    uses: BTreeMap<SnapEdge, Vec<FaceUse>>,
    at_vert: BTreeMap<SnapVert, Vec<SnapEdge>>,
}

type Res<T> = Result<T, Stop<RewriteError>>;

fn fail<T>(e: RewriteError) -> Res<T> {
    Err(Stop::Failed(e))
}

impl<O: EntityOrder> Geo<'_, O> {
    /// The class of edge `e` (module docs).
    fn class(&self, e: &NewEdge) -> EdgeClass {
        match e.key {
            SnapEdge::Zero { .. } => EdgeClass::Zero,
            SnapEdge::Cut { .. } => EdgeClass::Target,
            SnapEdge::Rim { .. } => EdgeClass::Rim,
            SnapEdge::Piece { edge: h, .. } => {
                if let Some(g) = self.merged_into(e.key, h) {
                    return EdgeClass::Merged(g);
                }
                if e.rim.is_some() {
                    return EdgeClass::AlongRim;
                }
                let snapped = self.uses.get(&e.key).is_some_and(|us| {
                    us.iter()
                        .any(|(f, _)| matches!(f, SnapFace::Snapped { .. }))
                });
                if snapped {
                    EdgeClass::Snapped
                } else {
                    EdgeClass::Kept
                }
            }
        }
    }

    /// The target edge a `SnapAlong` piece of `h` holding piece `key` of
    /// `h` merges it into, the first pair's in key order.
    fn merged_into(&self, key: SnapEdge, h: EdgeId) -> Option<EdgeId> {
        let Some(Pre {
            at: PreImage::Range([t0, t1]),
            ..
        }) = self.rewrite.map.edges.get(&key)
        else {
            return None;
        };
        let trim = self
            .rim_input
            .faces
            .iter()
            .flat_map(|f| f.own.iter())
            .find(|t| t.edge == h)?;
        let k0 = Q::try_from(*trim.knots.first()?).ok()?;
        let k1 = Q::try_from(*trim.knots.last()?).ok()?;
        let frac = |t: &Q| (t - &k0) / (&k1 - &k0);
        let (s0, s1) = (frac(t0), frac(t1));
        let (lo, hi) = if s0 <= s1 { (s0, s1) } else { (s1, s0) };
        self.along
            .pairs
            .iter()
            .filter(|p| p.h == h)
            .find(|p| {
                p.pieces
                    .iter()
                    .any(|piece| piece.span[0] <= lo && hi <= piece.span[1])
            })
            .map(|p| p.e)
    }

    fn new_vert(&self, key: SnapVert) -> Option<&NewVert> {
        self.rewrite
            .verts
            .binary_search_by_key(&key, |v| v.key)
            .ok()
            .map(|i| &self.rewrite.verts[i])
    }

    /// The point of face `face` of `S` at global `uv`, exactly: on the
    /// first of its tiles holding `uv`.
    fn s_point(&self, face: FaceId, uv: &P2) -> Option<P3> {
        let f = self.rim_input.faces.iter().find(|f| f.face == face)?;
        let (tile, _) = f.tiles.iter().find(|(k, _)| {
            (0..2).all(|c| uv[c] >= k[c] && uv[c] <= Q::from(k[c]) + Q::from(1u32))
        })?;
        point_on(self.rim_input, face, *tile, uv)
    }

    /// Vertex `v`'s pre-snap point.
    fn pre_snap(&self, v: &NewVert) -> Option<P3> {
        if let SnapVert::Vert(o, w) = v.key {
            return Some(exact3(self.input.order.vert_key(o, w).pos().get()));
        }
        let (face, uv) = v.at.first()?;
        self.s_point(*face, uv)
    }

    /// The target faces of the snapped faces at vertex `key`.
    fn target_faces(&self, key: SnapVert) -> BTreeSet<FaceId> {
        self.at_vert
            .get(&key)
            .into_iter()
            .flatten()
            .flat_map(|e| self.uses.get(e).into_iter().flatten())
            .filter(|(_, on)| on.0 != self.s_op)
            .map(|(_, on)| on.1)
            .collect()
    }

    /// `p`, the point of vertex `key`, carried along `ℓ` by the fraction
    /// `part` of the way onto the target faces `faces`, one point on all
    /// of them.
    fn carry(
        &self,
        key: SnapVert,
        p: &P3,
        faces: &BTreeSet<FaceId>,
        part: &Q,
    ) -> Result<P3, RewriteError> {
        let mut out: Option<P3> = None;
        for f in faces {
            let chart = self.charts.get(f).ok_or(RewriteError::Chart(*f))?;
            let (_, lambda) = chart.along(p, self.ell).ok_or(RewriteError::Map)?;
            let q = moved(p, self.ell, &(lambda * part));
            match &out {
                Some(o) if *o != q => return Err(RewriteError::Position(key)),
                Some(_) => {}
                None => out = Some(q),
            }
        }
        out.ok_or(RewriteError::Position(key))
    }

    /// Whether vertex `key` goes to the sheets' mid-point: a vertex of
    /// the rim whose edges are rim edges along box sides (or kept, or of
    /// zero length); one that also bounds an edge on the target goes onto
    /// the target, where that edge's fit holds.
    fn at_mid(&self, key: SnapVert, classes: &BTreeMap<SnapEdge, EdgeClass>) -> bool {
        self.on_rim(key)
            && self.at_vert.get(&key).into_iter().flatten().all(|e| {
                matches!(
                    classes[e],
                    EdgeClass::Rim | EdgeClass::Kept | EdgeClass::Zero
                )
            })
    }

    /// Whether vertex `key` is a vertex of the rim.
    fn on_rim(&self, key: SnapVert) -> bool {
        match key {
            SnapVert::Rim { .. }
            | SnapVert::RimTarget { .. }
            | SnapVert::RimBox { .. }
            | SnapVert::Branch { .. } => true,
            SnapVert::Vert(o, v) if o == self.s_op => self
                .corner
                .rim_verts
                .iter()
                .any(|r| r.kind == super::RimVertKind::Vert(o, v)),
            _ => false,
        }
    }

    /// Branch vertex `i`'s position, for vertex `key` pinned there: its
    /// point of `S` at the mid-point.
    fn branch_pos(&self, key: SnapVert, i: u32) -> Result<[f64; 3], RewriteError> {
        let bad = RewriteError::Position(key);
        let b = self.corner.branches.get(i as usize).ok_or(bad.clone())?;
        let piece = self.rim.pieces.get(b.piece as usize).ok_or(bad.clone())?;
        let p = point_on(self.rim_input, piece.face, piece.tile, &b.uv).ok_or(bad)?;
        let faces: BTreeSet<FaceId> = b.targets.iter().copied().collect();
        Ok(round3(&self.carry(
            key,
            &p,
            &faces,
            &Q::from_signeds(1, 2),
        )?))
    }

    /// The vertex at the start of rim piece `j`.
    fn rim_start(&self, j: u32) -> Option<&NewVert> {
        let piece = self.rim.pieces.get(j as usize)?;
        self.rewrite.verts.iter().find(|v| {
            v.at.iter()
                .any(|(f, p)| *f == piece.face && *p == piece.ends[0])
        })
    }

    /// The exact parameter vertex `v` stores on target edge `g` (module
    /// docs).
    fn stored_t(&self, v: &NewVert, g: &TargetEdge<'_>) -> Option<Q> {
        let t_op = self.s_op.other();
        let [w0, w1] = self.input.order.ends(t_op, g.edge);
        let (r0, r1) = (exact1(g.range[0]), exact1(g.range[1]));
        let end_t = |w: VertId| {
            if w == w0 {
                Some(r0.clone())
            } else if w == w1 {
                Some(r1.clone())
            } else {
                None
            }
        };
        if let SnapVert::Vert(o, w) = v.key
            && o == t_op
            && let Some(t) = end_t(w)
        {
            return Some(t);
        }
        // Where the corner rule or the bracket end put it.
        let mut m = v.moved;
        for _ in 0..4 {
            match m.to {
                Placement::Edge { edge, s } if edge == g.edge && s.is_finite() => {
                    return Some(&r0 + exact1(s) * (&r1 - &r0));
                }
                Placement::Vert(o, w) if o == t_op => {
                    if let Some(t) = end_t(w) {
                        return Some(t);
                    }
                    break;
                }
                Placement::Vert(_, w) => match self.corner.placed.iter().find(|p| p.vert == w) {
                    Some(p) => m = p.moved,
                    None => break,
                },
                Placement::With(j) => match self.corner.rim_verts.get(j as usize) {
                    Some(r) => m = r.moved,
                    None => break,
                },
                _ => break,
            }
        }
        // Where it lies on the target edge carried into a face of `S`.
        for (face, uv) in &v.at {
            if let Some(c) = self.carried.get(&(*face, g.edge))
                && let Some(t) = param_in(&[c.trim], std::slice::from_ref(&c.exact), g.edge, uv)
            {
                return Some(t);
            }
        }
        // The clamped foot of its pre-snap point.
        let p = self.pre_snap(v)?;
        let a = exact3(g.curve.point_at(g.range[0])?);
        let b = exact3(g.curve.point_at(g.range[1])?);
        let (_, s) = point_seg(&p, &a, &b);
        Some(&r0 + s * (r1 - &r0))
    }

    /// Vertex `v`'s geometry (module docs); `depth` bounds the chase of
    /// coincident rim vertices.
    fn vertex(
        &self,
        v: &NewVert,
        classes: &BTreeMap<SnapEdge, EdgeClass>,
        depth: u32,
        ledger: &mut Ledger<'_>,
    ) -> Res<VertGeom> {
        // Its position and its displacement.
        units(ledger, 2)?;
        let key = v.key;
        let bad = || Stop::Failed(RewriteError::Position(key));
        // The target edges it ends a reused edge of.
        let curves: BTreeSet<EdgeId> = self
            .at_vert
            .get(&key)
            .into_iter()
            .flatten()
            .filter_map(|e| reused(classes[e], *e))
            .collect();
        let mut at: Option<At> = None;
        for g in curves {
            units(ledger, 1)?;
            let Some(te) = self.targets.get(&g) else {
                return fail(RewriteError::Reuse(g));
            };
            let t = to_f64(&self.stored_t(v, te).ok_or_else(bad)?);
            let t = if t < te.range[0] {
                te.range[0]
            } else if t > te.range[1] {
                te.range[1]
            } else {
                t
            };
            let pos = te.curve.point_at(t).ok_or_else(bad)?;
            match &mut at {
                Some(a) if a.pos.map(f64::to_bits) != pos.map(f64::to_bits) => {
                    return fail(RewriteError::Shared(key));
                }
                Some(a) => a.on.push((g, t)),
                None => {
                    at = Some(At {
                        pos,
                        on: vec![(g, t)],
                    })
                }
            }
        }
        if let Some(a) = at {
            let displacement = self.displacement(v, a.pos).ok_or_else(bad)?;
            return Ok(VertGeom {
                key,
                pos: a.pos,
                on: a.on,
                displacement,
            });
        }
        let pos = match key {
            SnapVert::Vert(o, w) if o != self.s_op => self.input.order.vert_key(o, w).pos().get(),
            _ => match self.pinned(v, classes, depth, ledger)? {
                Some(p) => p,
                None => {
                    let p = self.pre_snap(v).ok_or_else(bad)?;
                    let faces = self.target_faces(key);
                    if faces.is_empty() {
                        round3(&p)
                    } else {
                        let part = if self.at_mid(key, classes) {
                            Q::from_signeds(1, 2)
                        } else {
                            Q::from(1u32)
                        };
                        round3(&self.carry(key, &p, &faces, &part).map_err(Stop::Failed)?)
                    }
                }
            },
        };
        let displacement = self.displacement(v, pos).ok_or_else(bad)?;
        Ok(VertGeom {
            key,
            pos,
            on: Vec::new(),
            displacement,
        })
    }

    /// An upper bound of the distance from vertex `v`'s pre-snap point to
    /// `pos`, exactly; `0` for a target vertex.
    fn displacement(&self, v: &NewVert, pos: [f64; 3]) -> Option<f64> {
        if matches!(v.key, SnapVert::Vert(o, _) if o != self.s_op) {
            return Some(0.0);
        }
        let p = self.pre_snap(v)?;
        Some(dist_up(&d2(&p, &exact3(pos))))
    }

    /// Where the corner rule (or a bracket end) pins vertex `v`, if it
    /// does.
    fn pinned(
        &self,
        v: &NewVert,
        classes: &BTreeMap<SnapEdge, EdgeClass>,
        depth: u32,
        ledger: &mut Ledger<'_>,
    ) -> Res<Option<[f64; 3]>> {
        let bad = || Stop::Failed(RewriteError::Position(v.key));
        if let Some(p) = v.moved.pos {
            return Ok(Some(p));
        }
        Ok(match v.moved.to {
            Placement::Stays | Placement::Edge { .. } => None,
            Placement::Vert(o, w) => Some(self.input.order.vert_key(o, w).pos().get()),
            Placement::Branch(i) => Some(self.branch_pos(v.key, i).map_err(Stop::Failed)?),
            Placement::With(j) => {
                let lead = self.rim_start(j).ok_or_else(bad)?;
                if depth > 2 || lead.key == v.key {
                    return fail(RewriteError::Position(v.key));
                }
                Some(self.vertex(lead, classes, depth + 1, ledger)?.pos)
            }
        })
    }

    /// The chain of target edge `g` split at `splits`.
    fn chain(&self, g: EdgeId, splits: &[(f64, SnapVert)], ledger: &mut Ledger<'_>) -> Res<Chain> {
        let Some(te) = self.targets.get(&g) else {
            return fail(RewriteError::Reuse(g));
        };
        units(ledger, 1 + (splits.len() * (1 + te.pcurves.len())) as u64)?;
        let ts: Vec<f64> = splits.iter().map(|s| s.0).collect();
        let curve = te
            .curve
            .split(&ts)
            .map_err(|_| Stop::Failed(RewriteError::Reuse(g)))?;
        let mut pcurves = Vec::with_capacity(te.pcurves.len());
        for (f, p) in te.pcurves {
            pcurves.push((
                *f,
                p.split(&ts)
                    .map_err(|_| Stop::Failed(RewriteError::Reuse(g)))?,
            ));
        }
        Ok(Chain {
            edge: g,
            curve,
            pcurves,
            splits: splits.to_vec(),
        })
    }

    /// Vertex `v`'s UV on target face `f` (module docs).
    fn t_uv(&self, v: &VertGeom, f: FaceId) -> Option<[f64; 2]> {
        for (g, t) in &v.on {
            if let Some(p) = self
                .targets
                .get(g)
                .and_then(|te| te.pcurves.iter().find(|x| x.0 == f))
            {
                return p.1.point_at(*t);
            }
        }
        let (uv, _) = self.charts.get(&f)?.along(&exact3(v.pos), self.ell)?;
        Some(round2(&uv))
    }

    /// Vertex `v`'s UV on face `face` of `S` (module docs).
    fn s_uv(&self, v: &VertGeom, face: FaceId) -> Option<[f64; 2]> {
        for (g, t) in &v.on {
            if let Some(c) = self.carried.get(&(face, *g)) {
                return c.pcurve()?.point_at(*t);
            }
        }
        let nv = self.new_vert(v.key)?;
        let (_, uv) = nv.at.iter().find(|x| x.0 == face)?;
        Some(round2(uv))
    }

    /// Edge `e`'s geometry (module docs).
    fn edge(
        &self,
        e: &NewEdge,
        class: EdgeClass,
        verts: &BTreeMap<SnapVert, &VertGeom>,
        chains: &BTreeMap<EdgeId, &Chain>,
        ledger: &mut Ledger<'_>,
    ) -> Res<EdgeGeom> {
        let key = e.key;
        let uses: &[FaceUse] = self.uses.get(&key).map(Vec::as_slice).unwrap_or_default();
        units(ledger, 1 + uses.len() as u64)?;
        let bad = || Stop::Failed(RewriteError::Curve(key));
        let [a, b] = e.ends.map(|k| verts.get(&k).copied());
        let (Some(a), Some(b)) = (a, b) else {
            return fail(RewriteError::Curve(key));
        };
        let mut out = EdgeGeom {
            key,
            class,
            curve: EdgeCurve::Kept,
            coedges: Vec::new(),
            fit: 0.0,
        };
        match class {
            EdgeClass::Kept => {}
            EdgeClass::Zero => {
                if a.pos.map(f64::to_bits) != b.pos.map(f64::to_bits) {
                    return fail(RewriteError::Position(b.key));
                }
                out.curve = EdgeCurve::Point;
            }
            EdgeClass::Target | EdgeClass::Merged(_) => {
                let g = reused(class, key).ok_or_else(bad)?;
                let chain = chains.get(&g).ok_or(Stop::Failed(RewriteError::Reuse(g)))?;
                let t_of = |v: &VertGeom| v.on.iter().find(|x| x.0 == g).map(|x| x.1);
                let (Some(ta), Some(tb)) = (t_of(a), t_of(b)) else {
                    return fail(RewriteError::Curve(key));
                };
                if ta == tb {
                    return fail(RewriteError::Curve(key));
                }
                let (lo, hi) = if ta < tb { (ta, tb) } else { (tb, ta) };
                for (face, on) in uses {
                    let pcurve = if on.0 == self.s_op {
                        let c = self
                            .carried
                            .get(&(on.1, g))
                            .ok_or(Stop::Failed(RewriteError::Reuse(g)))?;
                        let ts: Vec<f64> = chain.splits.iter().map(|s| s.0).collect();
                        c.pcurve()
                            .and_then(|p| p.split(&ts).ok())
                            .and_then(|p| p.slice(lo, hi))
                    } else {
                        chain
                            .pcurves
                            .iter()
                            .find(|x| x.0 == on.1)
                            .and_then(|p| p.1.slice(lo, hi))
                    };
                    out.coedges.push(Coedge {
                        face: *face,
                        pcurve: pcurve.ok_or(Stop::Failed(RewriteError::Reuse(g)))?,
                    });
                }
                out.curve = EdgeCurve::Reused {
                    edge: g,
                    t: [ta, tb],
                };
            }
            EdgeClass::Snapped | EdgeClass::AlongRim => {
                let c = Curve3D::new(vec![line(a.pos, b.pos).ok_or_else(bad)?], vec![0.0, 1.0])
                    .map_err(|_| bad())?;
                for (face, on) in uses {
                    let span = if on.0 == self.s_op {
                        let (Some(ua), Some(ub)) = (self.s_uv(a, on.1), self.s_uv(b, on.1)) else {
                            return fail(RewriteError::Curve(key));
                        };
                        uv_line(ua, ub).ok_or_else(bad)?
                    } else {
                        let chart = self
                            .charts
                            .get(&on.1)
                            .ok_or(Stop::Failed(RewriteError::Chart(on.1)))?;
                        let (Some(ua), Some(ub)) = (self.t_uv(a, on.1), self.t_uv(b, on.1)) else {
                            return fail(RewriteError::Curve(key));
                        };
                        let span = uv_line(ua, ub).ok_or_else(bad)?;
                        let fit = fit_bound(chart, &span, &c.spans()[0]);
                        if fit > out.fit {
                            out.fit = fit;
                        }
                        span
                    };
                    out.coedges.push(Coedge {
                        face: *face,
                        pcurve: PCurve::new(vec![span], vec![0.0, 1.0]).map_err(|_| bad())?,
                    });
                }
                // Not `>`: a NaN bound fails too.
                let most = 0.5 * self.input.eps_seam;
                if !matches!(
                    out.fit.partial_cmp(&most),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ) {
                    return fail(RewriteError::Fit(key));
                }
                out.curve = EdgeCurve::New(c);
            }
            EdgeClass::Rim => {
                let (curve, coedges) = self.rim_edge(e, a, b, uses).map_err(Stop::Failed)?;
                out.curve = EdgeCurve::New(curve);
                out.coedges = coedges;
            }
        }
        Ok(out)
    }

    /// A rim edge along box sides: its curve on the mid-surface and its
    /// coedges (module docs).
    fn rim_edge(
        &self,
        e: &NewEdge,
        a: &VertGeom,
        b: &VertGeom,
        uses: &[FaceUse],
    ) -> Result<(Curve3D, Vec<Coedge>), RewriteError> {
        let key = e.key;
        let SnapEdge::Rim { face, piece, .. } = key else {
            return Err(RewriteError::Curve(key));
        };
        let tile = self
            .rim
            .pieces
            .get(piece as usize)
            .ok_or(RewriteError::Corner)?
            .tile;
        let exact_uv = |k: SnapVert| -> Option<P2> {
            let nv = self.new_vert(k)?;
            nv.at.iter().find(|x| x.0 == face).map(|x| x.1.clone())
        };
        let (Some(ua), Some(ub)) = (exact_uv(a.key), exact_uv(b.key)) else {
            return Err(RewriteError::Curve(key));
        };
        let targets: Vec<FaceId> = uses
            .iter()
            .filter(|(_, on)| on.0 != self.s_op)
            .map(|(_, on)| on.1)
            .collect();
        let [f] = targets[..] else {
            return Err(RewriteError::TargetFace(face));
        };
        let chart = self.charts.get(&f).ok_or(RewriteError::Chart(f))?;
        // The iso curve of the tile between the ends, homogeneous.
        let patch = self
            .rim_input
            .faces
            .iter()
            .find(|g| g.face == face)
            .and_then(|g| g.tiles.iter().find(|(k, _)| *k == tile))
            .map(|x| &x.1)
            .ok_or(RewriteError::Face(face))?;
        let local = |p: &P2| [&p[0] - Q::from(tile[0]), &p[1] - Q::from(tile[1])];
        let (la, lb) = (local(&ua), local(&ub));
        let h = iso(patch, &la, &lb).ok_or(RewriteError::Trim(face))?;
        // Carried halfway onto the target along `ℓ`, and onto it.
        let half = Q::from_signeds(1, 2);
        let mut mid: Vec<[Q; 4]> = Vec::with_capacity(3);
        let mut on_t: Vec<[Q; 3]> = Vec::with_capacity(3);
        for c in &h {
            let w = &c[3];
            let x: P3 = [0, 1, 2].map(|k| &c[k] / w);
            let (uv, lambda) = chart.along(&x, self.ell).ok_or(RewriteError::Map)?;
            let m = moved(&x, self.ell, &(lambda * &half));
            let w = w / &h[0][3];
            mid.push([m[0].clone(), m[1].clone(), m[2].clone(), w.clone()]);
            on_t.push([uv[0].clone(), uv[1].clone(), w]);
        }
        let weight = |k: usize| to_f64(&mid[k][3]);
        let p1 = round3(&[mid[1][0].clone(), mid[1][1].clone(), mid[1][2].clone()]);
        let curve = Curve3D::new(
            vec![[
                [a.pos[0], a.pos[1], a.pos[2], weight(0)],
                [p1[0], p1[1], p1[2], weight(1)],
                [b.pos[0], b.pos[1], b.pos[2], weight(2)],
            ]],
            vec![0.0, 1.0],
        )
        .map_err(|_| RewriteError::Curve(key))?;
        let mut coedges = Vec::with_capacity(uses.len());
        for (sf, on) in uses {
            let span: Span2 = if on.0 == self.s_op {
                let (Some(sa), Some(sb)) = (self.s_uv(a, on.1), self.s_uv(b, on.1)) else {
                    return Err(RewriteError::Curve(key));
                };
                uv_line(sa, sb).ok_or(RewriteError::Curve(key))?
            } else {
                let (Some(ta), Some(tb)) = (self.t_uv(a, on.1), self.t_uv(b, on.1)) else {
                    return Err(RewriteError::Curve(key));
                };
                let m = round2(&[on_t[1][0].clone(), on_t[1][1].clone()]);
                [
                    [ta[0], ta[1], weight(0)],
                    [m[0], m[1], weight(1)],
                    [tb[0], tb[1], weight(2)],
                ]
            };
            coedges.push(Coedge {
                face: *sf,
                pcurve: PCurve::new(vec![span], vec![0.0, 1.0])
                    .map_err(|_| RewriteError::Curve(key))?,
            });
        }
        Ok((curve, coedges))
    }
}

/// The iso curve of `patch` (tile-local) from `a` to `b`, which share a
/// coordinate: its homogeneous control points `(w·x, w·y, w·z, w)` as a
/// quadratic. `None` unless the ends share a coordinate and the patch's
/// degree along the curve is at most 2.
fn iso(patch: &HomPatch<Rational>, a: &[Q; 2], b: &[Q; 2]) -> Option<[[Q; 4]; 3]> {
    let h = patch.tensors();
    let (m, n) = patch.degrees();
    let bern = |d: usize, i: usize, x: &Q| -> Q {
        let c = (0..i).fold(Q::from(1u32), |c, k| c * Q::from(d - k) / Q::from(k + 1));
        let one = Q::from(1u32);
        let mut out = c;
        for _ in 0..i {
            out *= x;
        }
        for _ in i..d {
            out *= &one - x;
        }
        out
    };
    // The curve's control points along the varying coordinate, and the
    // ends' values of it.
    let (ctl, s0, s1): (Vec<[Q; 4]>, &Q, &Q) = if a[0] == b[0] {
        let pts = (0..=n)
            .map(|j| {
                std::array::from_fn(|k| {
                    (0..=m).fold(Q::from(0u32), |s, i| s + bern(m, i, &a[0]) * h[k].get(i, j))
                })
            })
            .collect();
        (pts, &a[1], &b[1])
    } else if a[1] == b[1] {
        let pts = (0..=m)
            .map(|i| {
                std::array::from_fn(|k| {
                    (0..=n).fold(Q::from(0u32), |s, j| s + bern(n, j, &a[1]) * h[k].get(i, j))
                })
            })
            .collect();
        (pts, &a[0], &b[0])
    } else {
        return None;
    };
    let q2: [[Q; 4]; 3] = match ctl.len() {
        2 => {
            let half = Q::from_signeds(1, 2);
            [
                ctl[0].clone(),
                std::array::from_fn(|k| (&ctl[0][k] + &ctl[1][k]) * &half),
                ctl[1].clone(),
            ]
        }
        3 => [ctl[0].clone(), ctl[1].clone(), ctl[2].clone()],
        _ => return None,
    };
    // Restricted to `[s₀, s₁]` by its blossom.
    let one = Q::from(1u32);
    let blossom = |x: &Q, y: &Q| -> [Q; 4] {
        let (mx, my) = (&one - x, &one - y);
        let c0 = &mx * &my;
        let c1 = &mx * y + x * &my;
        let c2 = x * y;
        std::array::from_fn(|k| &c0 * &q2[0][k] + &c1 * &q2[1][k] + &c2 * &q2[2][k])
    };
    let out = [blossom(s0, s0), blossom(s0, s1), blossom(s1, s1)];
    out.iter().all(|c| c[3] > 0u32).then_some(out)
}

/// An upper bound of `|T(p(t)) − C(t)|` for the line spans `p` (UV) and
/// `C` on the plane `chart`: both quadratics with weights 1, their
/// difference within its control points' hull.
fn fit_bound(chart: &Chart, p: &Span2, c: &Span3) -> f64 {
    let mut worst = Q::from(0u32);
    for k in 0..3 {
        let at = chart.point(&[exact1(p[k][0]), exact1(p[k][1])]);
        let dd = d2(&at, &exact3([c[k][0], c[k][1], c[k][2]]));
        if dd > worst {
            worst = dd;
        }
    }
    dist_up(&worst)
}
