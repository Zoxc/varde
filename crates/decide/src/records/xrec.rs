//! The crossing records' count: how many times, with sign, the shadow of
//! an edge `e = a→b` of `A` crosses the shadow of an edge `g = g0→g1` of
//! `B`, and the boxes of their parameter square that hold the crossings.
//!
//! **The difference map.** Both edges run along their stored direction
//! (from the lower-id end), `e` with parameter `t`, `g` with `s`, so the
//! square `[0, 1]²` (per pair of spans, below) maps to the shadow plane
//! by `D(t, s) = (C_e(t) − C_g(s))_xy`. The shadows cross exactly where
//! `D` is 0, and the winding of `D` round a box's boundary about 0 (its
//! **degree**) counts the crossings inside the box with their signs.
//!
//! **The four-term identity.** The boundary of the whole square is four
//! curves, each a point of one operand against an edge of the other, so
//! its winding is read from the ray records, the four terms
//!
//! ```text
//! X(e, g) = RayRec(b, g) − RayRec(a, g) + RayRec(g0, e) − RayRec(g1, e)
//! ```
//!
//! ([`x_count`]). Counted against the ray along `+r̂` from 0, the side
//! `s = 0` (`D = C_e(t) − g0`, `t` increasing) crosses that ray where
//! `C_e` crosses the ray of `g0` (a point of `B`: its ray goes `+r̂`),
//! counter-clockwise exactly when `ν·T_e > 0`, which is the record's sign
//! `sign(ρ × T_e)`; the side `t = 1` (`D = b − C_g(s)`, `s` increasing)
//! crosses it where `C_g` crosses the ray of `b` (a point of `A`: `−r̂`),
//! the angle of `D` being that of `C_g − b` turned by `π`, so again with
//! the record's sign; the sides `s = 1` and `t = 0` run backwards
//! counter-clockwise, hence their minus signs. The records decide every
//! tie (a vertex on the other edge's shadow, a ray through a vertex)
//! under the perturbation, so the identity is exact on the moved data.
//!
//! The boxes of the square and their degrees, and the refinement that
//! cuts the square into clusters, are counting's
//! ([`crate::count::xrec`]). A box side inside the square that the level
//! can't certify off 0 reads a side record kept here: the held point of
//! one edge (a knot, or a box line's dyadic point) against the other
//! edge, `(e, g, dyadic side)`, and the corner records of the two edges'
//! points, a knot against a knot where their shadows coincide
//! ([`XSides`], module `sides`).
//!
//! **Clusters** ([`XCluster`]). A box of any degree refines until the
//! z-order of its two pieces is clear (the heights `ê·x` of the two
//! pieces certified apart over the box) or their shadow tangents are
//! certified transversal over it (so it holds at most one crossing),
//! never stopping on size; a box of non-zero degree is then one cluster,
//! of the box's degree, in the record's list ([`XRec`]) by its index
//! ([`XClusterIx`]), the list stored with the precision level that formed
//! it, which its order depends on. `s11(e, g)` is the sum of the degrees
//! of the clusters with `g` above `e` ([`XRec::s11`]).
//!
//! **The z-order key** ([`zorder_key`]). A cluster whose z-order isn't
//! clear reads a height record anchored at `g`, keyed by the cluster
//! itself, the edge–edge key `HKey::X(e, g, c)`, always: its value is the
//! edges' height at the crossing (exact on straight edges, on curved ones
//! at a level over the cluster's box), and an end of `e` lying exactly on
//! `g` (a shared corner) makes it exactly 0, so the orders decide. No
//! end's feet or near key choose it. The row it reads is fixed with the
//! cluster ([`XCluster::row`]); the decider is the height model's
//! ([`crate::records::height::model::XHeights`]).
//!
//! **Degenerate z-order** ([`ZRow`]). Where the shadows' tangents can't
//! be certified apart at the crossing (`T_e`, `T_g` and `ê` coplanar, so
//! `m = T_e × T_g` has no certified upward side), the edges don't cross
//! transversally in shadow. Two straight edges whose shadows are exactly
//! parallel cross only where the four terms' ties put a crossing; their
//! one cluster then reads the point–curve height row at the end whose
//! term counted it ([`ZRow::PointCurve`]), never an edge–edge row with an
//! undefined `m`, and only where the z-order is one sign over the whole
//! overlap of the shadows (then clear; else the pair is refused,
//! [`crate::count::xrec`]). Curved pieces in that state refine, and
//! escalate where no level certifies them.
//!
//! **Shared curves** ([`shared_clusters`]). Two edges on one curve (one
//! curve object, or bitwise-equal canonical curves: the caller knows,
//! [`XPair::shared`]) have shadows that coincide wherever the edges
//! overlap, so no box of their square ever settles: they get no
//! geometric crossing search. With `e` moved by `δ·Δ` (its rates) and
//! `g` fixed, the moved shadows meet near the overlap exactly where `h =
//! ê·(T × Δ)` changes sign along it (`T` the curve's tangent: the shadow
//! tangent parallel to the shadow of the motion;
//! [`shared_crossing`](crate::perturb::shared_crossing)), and at an
//! overlap end only where `h` is 0 there. The four terms are side terms
//! of the parameter square, never crossings: a tie at a shared end, a ray
//! through the other edge's far vertex, a cancelling `±1` pair at the two
//! ends make no cluster (such a pair reads different z-orders at its two
//! ends, so keeping it would be wrong). So a pair whose `h` is certified
//! one sign over the closed overlap has no clusters, and its four terms
//! must count `X = 0`; any other pair is refused, naming why
//! ([`SharedError`]): `h` of both signs (a crossing, whose cluster at the
//! root of `h` this rule doesn't form), `h` not settled by any level
//! (a root at or near a point of the overlap, a vertical shadow tangent,
//! `h ≡ 0`), or `X ≠ 0` (crossings away from the overlap, which no search
//! here finds). It never answers wrong.
//!
//! **The overlap.** On one line it is the exact common range of the two
//! segments on `e`'s parameter, rounded outwards (a point where they only
//! abut). On one chain it is made of whole spans: those of `e` that are
//! spans of `g` bit for bit (either way round), and the span ends of `e`
//! that are span ends of `g` (abutting edges' common end). That holds
//! where every end of either edge is a knot or end of the other or off it
//! (no feet on it); an end lying on the other edge inside a span is
//! refused ([`SharedError::Unaligned`]). `h` is evaluated at each
//! precision level over the pieces, halved breadth first until each
//! piece certifies a sign (down to [`MIN_H_PIECE`], at most
//! [`MAX_H_PIECES`] evaluations a level, then the next level), at `e`'s
//! rates there ([`XGeometry::edge_rates`]).
//!
//! **Cut points** ([`cut_point`]). The cut point `Cut(e, g, c)` is the
//! point of `e` at cluster `c` ([`XCluster::part`]). Its records (against
//! a vertex, an edge's ray, a height) are the ordinary ones of a point at
//! its stored position: the point of `e`'s span at the middle of the
//! cluster's piece, evaluated exactly and rounded to the nearest `f64`
//! (exactly the vertex or knot where the piece is a span's end), within
//! the piece and so within the band of the crossing. Against its own
//! edge `g` its height is the crossing's, `HKey::X(e, g, c)`
//! ([`HKey::x`]).

use std::cmp::Ordering;
use std::collections::VecDeque;

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor, Nearest};

use crate::bezier::{Dyadic, HomCurve, Scalar};
use crate::ids::EdgeId;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::Sign;
use crate::order::{EntityOrder, first};
use crate::perturb::{Curved, NU, between, curve_point, shared_crossing};
use crate::records::height::{HeightReads, HeightRec};
use crate::records::keys::{
    Anchor, HKey, HeightKey, Operand, Oriented, Point, RayKey, RecordKey, XClusterIx, XKey,
};
use crate::records::ray::{ChainRec, Span};
use crate::records::shadow::{LEVELS, RayRec};

/// An edge's curve along its stored direction, from its lower-id end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum XCurve<'a> {
    /// A straight edge from its first point to its second.
    Line([f64; 3], [f64; 3]),
    /// A curved edge: its spans ([`CurveGeometry::spans`](super::ray::CurveGeometry::spans)).
    Spans(&'a [Span]),
}

impl XCurve<'_> {
    /// The number of spans: 1 for a line.
    pub fn span_count(&self) -> u32 {
        match self {
            XCurve::Line(..) => 1,
            // A chain longer than `u32::MAX` spans has no span keys.
            XCurve::Spans(s) => u32::try_from(s.len()).unwrap_or(u32::MAX),
        }
    }

    /// The edge's first or last point, exactly.
    pub(crate) fn end(&self, last: bool) -> Option<[f64; 3]> {
        match self {
            XCurve::Line(a, b) => Some(if last { *b } else { *a }),
            XCurve::Spans(s) => {
                let p = if last { s.last()?[2] } else { s.first()?[0] };
                Some([p[0], p[1], p[2]])
            }
        }
    }

    /// Span `k` at `like`'s level, `None` if there is none.
    pub(crate) fn span<S: Scalar>(&self, k: u32, like: &S) -> Option<HomCurve<S, 3>> {
        match self {
            XCurve::Line(a, b) if k == 0 => HomCurve::from_affine([(*a, 1.0), (*b, 1.0)], like),
            XCurve::Line(..) => None,
            XCurve::Spans(s) => {
                let sp = s.get(usize::try_from(k).ok()?)?;
                HomCurve::from_affine(sp.iter().map(|p| ([p[0], p[1], p[2]], p[3])), like)
            }
        }
    }

    /// Span `k`'s control point at its start (`at = 0`) or end (`at = 1`),
    /// exactly, if `at` is one of them.
    pub(crate) fn span_end(&self, k: u32, at: Dyadic) -> Option<[f64; 3]> {
        let last = if at == Dyadic::ZERO {
            false
        } else if at == Dyadic::ONE {
            true
        } else {
            return None;
        };
        match self {
            XCurve::Line(a, b) => (k == 0).then_some(if last { *b } else { *a }),
            XCurve::Spans(s) => {
                let sp = s.get(usize::try_from(k).ok()?)?;
                let p = sp[if last { 2 } else { 0 }];
                Some([p[0], p[1], p[2]])
            }
        }
    }
}

/// A point's ray record against a whole edge of the other operand: a
/// straight edge's [`RayRec`], a curved one's [`ChainRec`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeRays<'a> {
    /// Against a straight edge.
    Line(RayRec),
    /// Against a curved edge.
    Chain(&'a ChainRec),
}

impl EdgeRays<'_> {
    /// The signed crossings over the whole edge.
    pub fn count(&self) -> i64 {
        match self {
            EdgeRays::Line(r) => i64::from(r.count()),
            EdgeRays::Chain(c) => i64::from(c.count()),
        }
    }
}

/// The four ray records the square's boundary reads (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XTerms<'a> {
    /// `RayRec(a, g)`: `e`'s start against `g`.
    pub a_g: EdgeRays<'a>,
    /// `RayRec(b, g)`: `e`'s end against `g`.
    pub b_g: EdgeRays<'a>,
    /// `RayRec(g0, e)`: `g`'s start against `e`.
    pub g0_e: EdgeRays<'a>,
    /// `RayRec(g1, e)`: `g`'s end against `e`.
    pub g1_e: EdgeRays<'a>,
}

/// `X(e, g)`, the signed crossings of the two edges' shadows, from the
/// four terms (module docs).
pub fn x_count(terms: &XTerms<'_>) -> i64 {
    terms.b_g.count() - terms.a_g.count() + terms.g0_e.count() - terms.g1_e.count()
}

/// An edge of `A` against an edge of `B`: their curves and the four
/// terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XPair<'a> {
    /// The edge `e` of `A`.
    pub e: XCurve<'a>,
    /// The edge `g` of `B`.
    pub g: XCurve<'a>,
    /// The ray records of the square's boundary.
    pub terms: XTerms<'a>,
    /// The edges lie on one curve (module docs, "Shared curves"): the
    /// pair gets no geometric crossing search.
    pub shared: bool,
}

/// A piece of an edge: an extent of one of its spans' parameters, along
/// its stored direction, its ends exact dyadic values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XPart {
    /// The span, from 0 at the edge's lower-id end (0 on a straight
    /// edge).
    pub span: u32,
    /// `[lo, hi]` on the span's `[0, 1]`.
    pub t: [f64; 2],
}

impl XCluster {
    /// The piece of the edge of `of`: `e`'s for `A`, `g`'s for `B`.
    pub fn part(&self, of: Operand) -> &XPart {
        match of {
            Operand::A => &self.e,
            Operand::B => &self.g,
        }
    }
}

/// Which row a cluster's edge–edge key reads (module docs, "Degenerate
/// z-order").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZRow {
    /// The edge–edge height `m·(C_g(s) − C_e(t))`, `m = unit(T_e × T_g)`
    /// turned so `m·ê > 0`.
    EdgeEdge,
    /// The shadows are exactly parallel: the point–curve height of this
    /// end of one edge against the other edge, the end whose term counted
    /// the crossing.
    PointCurve(Point),
}

/// One cluster of an edge pair's crossing record (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XCluster {
    /// The piece of `e`.
    pub e: XPart,
    /// The piece of `g`.
    pub g: XPart,
    /// The signed crossings in it, non-zero.
    pub degree: i64,
    /// The z-order where it is clear over the cluster's box: `Pos` for
    /// `g` above `e`; `None` where the cluster reads its height record.
    pub clear: Option<Sign>,
    /// The row its edge–edge key reads.
    pub row: ZRow,
}

/// The crossing record of an edge pair: its clusters, in the order their
/// boxes were keyed ([`XClusterIx`] is the index here), and the precision
/// level they were formed at, on which that order depends: the record is
/// checked again at that level only
/// ([`replay`](crate::count::xrec::replay)). A shared-curve pair's
/// record has no clusters, level 0.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XRec {
    /// The clusters.
    pub clusters: Vec<XCluster>,
    /// The level the clusters were formed at (`0` for `f64`).
    pub level: usize,
}

impl XRec {
    /// Cluster `c`, if there is one.
    pub fn get(&self, c: XClusterIx) -> Option<&XCluster> {
        usize::try_from(c.0).ok().and_then(|i| self.clusters.get(i))
    }

    /// The clusters with their indices.
    pub fn iter(&self) -> impl Iterator<Item = (XClusterIx, &XCluster)> {
        (0u32..)
            .zip(&self.clusters)
            .map(|(i, c)| (XClusterIx(i), c))
    }

    /// The sum of the degrees: `X(e, g)` when the clusters are all of
    /// the square's.
    pub fn degree(&self) -> i64 {
        self.clusters.iter().map(|c| c.degree).sum()
    }

    /// `s11(e, g)`: the sum of the degrees of the clusters with `g` above
    /// `e`, a clear cluster by its own z-order, the others by `above`
    /// (from their height records, [`ZKey::above`]).
    pub fn s11<E>(
        &self,
        mut above: impl FnMut(XClusterIx, &XCluster) -> Result<Sign, E>,
    ) -> Result<i64, E> {
        let mut s = 0i64;
        for (c, cl) in self.iter() {
            let z = match cl.clear {
                Some(z) => z,
                None => above(c, cl)?,
            };
            if z == Sign::Pos {
                s += cl.degree;
            }
        }
        Ok(s)
    }
}

/// A cluster's z-order key (module docs): the edge–edge key as the
/// cluster's `A` edge asks it. The record is anchored at `g`
/// ([`ZKey::anchor`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ZKey {
    /// The height key, oriented as its point asked it.
    pub key: Oriented<HKey>,
}

impl ZKey {
    /// The anchor every z-order record of the pair reads: `g`, of `B`.
    pub fn anchor(g: EdgeId) -> Anchor {
        Anchor::Edge(Operand::B, g)
    }

    /// The height record's key at `g`.
    pub fn height_key(&self, g: EdgeId) -> HeightKey {
        HeightKey {
            h: self.key.key,
            anchor: ZKey::anchor(g),
        }
    }

    /// The z-order from the record (read through
    /// [`Heights::get`](super::height::Heights::get) with [`key`](Self::key)):
    /// `Pos` for `g` above `e`.
    pub fn above(&self, rec: &HeightRec) -> Sign {
        rec.above
    }
}

/// The z-order key of cluster `c` of the pair `x` (module docs): the
/// cluster's own edge–edge key.
pub fn zorder_key(x: XKey, c: XClusterIx) -> ZKey {
    ZKey {
        key: Oriented {
            key: HKey::X { e: x.e, g: x.g, c },
            swapped: false,
        },
    }
}

/// For two straight edges whose shadows are exactly parallel (module
/// docs, "Degenerate z-order"): the end whose term counted a crossing,
/// the geometrically lesser of those whose term has the sign of the
/// pair's count `X` (each term's contribution to `X` as the four-term
/// identity signs it); `None` where `X` is 0.
pub fn tie_point<O: EntityOrder>(order: &O, x: XKey, terms: &XTerms<'_>) -> Option<Point> {
    let total = x_count(terms).signum();
    if total == 0 {
        return None;
    }
    let [a, b] = order.ends(Operand::A, x.e);
    let [g0, g1] = order.ends(Operand::B, x.g);
    let parts = [
        (Operand::A, a, -terms.a_g.count()),
        (Operand::A, b, terms.b_g.count()),
        (Operand::B, g0, terms.g0_e.count()),
        (Operand::B, g1, -terms.g1_e.count()),
    ];
    first(
        parts
            .into_iter()
            .filter(|p| p.2.signum() == total)
            .map(|(op, v, _)| ((order.vert_key(op, v), op), Point::vert(op, v))),
    )
}

/// Why a shared-curve pair isn't answered (module docs, "Shared
/// curves"): never a wrong record, a refusal naming what it met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedError {
    /// An end's feet on the other edge aren't decided.
    Missing(RecordKey),
    /// An edge has no spans, one is straight and the other curved, or
    /// two straight edges aren't on one line.
    Shape,
    /// This end of one edge lies on the other edge inside one of its
    /// spans, not at a knot or vertex of it: the overlap isn't made of
    /// whole spans of both, which the rule reads it as.
    Unaligned(Point),
    /// `h` is certified of both signs over the overlap: the moved edges
    /// cross there.
    Crossed,
    /// No level certifies one sign of `h` over the overlap: a root of `h`
    /// at or too near a point of it (the shadow tangent along the motion,
    /// or vertical), or `h ≡ 0` there.
    Unsettled,
    /// `h` is one sign over the overlap, but the four terms count this
    /// many crossings: crossings away from the overlap (the curve's
    /// shadow crossing itself), which no search here finds.
    Away(i64),
}

/// The narrowest piece of a span's parameter `h` is halved to: below
/// it, a level that hasn't certified `h` leaves it to the next (`2⁻⁴⁰`).
pub const MIN_H_PIECE: f64 = 1.0 / 1_099_511_627_776.0;

/// A piece of the overlap of a shared-curve pair, on `e`: a span of `e`
/// and a parameter range of it (a point where its ends are equal).
type Piece = (u32, [f64; 2]);

/// The most evaluations of `h` one level makes over a pair's overlap
/// before it is left to the next level.
pub const MAX_H_PIECES: usize = 4096;

/// The record of a shared-curve pair `x` (module docs, "Shared curves"):
/// no clusters where `h = ê·(T × Δ)` (`T` the curve's tangent, `Δ` `e`'s
/// rates, [`shared_crossing`]) is certified one sign over the edges'
/// overlap and the four terms count no crossing; else refused. `geom`
/// gives `e`'s rates along it; `reads` its ends and their feet.
pub fn shared_clusters<R: HeightReads, G: XGeometry>(
    reads: &R,
    geom: &G,
    x: XKey,
    pair: &XPair<'_>,
) -> Result<XRec, SharedError> {
    if pair.e.span_count() == 0 || pair.g.span_count() == 0 {
        return Err(SharedError::Shape);
    }
    let pieces = overlap(reads, x, pair)?;
    let mut settled = false;
    for level in 0..LEVELS {
        let over = match level {
            0 => h_over(geom, x.e, &pair.e, &pieces, &Bounded::exact(0.0)),
            1 => h_over(
                geom,
                x.e,
                &pair.e,
                &pieces,
                &Bounded::exact(Dd::from_f64(0.0)),
            ),
            _ => match Width::ALL.get(level - 2) {
                Some(&w) => h_over(
                    geom,
                    x.e,
                    &pair.e,
                    &pieces,
                    &Bounded::exact(Big::from_f64(0.0, w)),
                ),
                None => break,
            },
        };
        match over {
            Ok(_) => {
                settled = true;
                break;
            }
            Err(SharedError::Unsettled) => {}
            Err(e) => return Err(e),
        }
    }
    if !settled {
        return Err(SharedError::Unsettled);
    }
    let n = x_count(&pair.terms);
    if n != 0 {
        return Err(SharedError::Away(n));
    }
    Ok(XRec::default())
}

/// The overlap of a shared-curve pair as pieces of `e` (module docs,
/// "Shared curves"): on one line, the exact common range, rounded
/// outwards to `f64`; on one chain, the spans of `e` that are spans of
/// `g` (either way round) and the span ends of `e` that are span ends of
/// `g` (a common end of abutting edges). Each end of either edge must be
/// a span end of the other or have no feet on it.
fn overlap<R: HeightReads>(
    reads: &R,
    x: XKey,
    pair: &XPair<'_>,
) -> Result<Vec<Piece>, SharedError> {
    match (pair.e, pair.g) {
        (XCurve::Line(a, b), XCurve::Line(g0, g1)) => line_overlap(a, b, g0, g1),
        (XCurve::Spans(es), XCurve::Spans(gs)) => {
            let ends = |s: &[Span]| -> Vec<[f64; 3]> {
                s.iter()
                    .flat_map(|sp| [sp[0], sp[2]])
                    .map(|p| [p[0], p[1], p[2]])
                    .collect()
            };
            let (e_pts, g_pts) = (ends(es), ends(gs));
            for (of, edge, other, curve, theirs) in [
                (Operand::A, x.e, x.g, &pair.e, &g_pts),
                (Operand::B, x.g, x.e, &pair.g, &e_pts),
            ] {
                for (i, v) in reads.ends(of, edge).into_iter().enumerate() {
                    let p = curve.end(i == 1).ok_or(SharedError::Shape)?;
                    if theirs.contains(&p) {
                        continue;
                    }
                    let q = Point::vert(of, v);
                    let feet = reads
                        .feet(q, other)
                        .ok_or(SharedError::Missing(RecordKey::Ray(RayKey { q, g: other })))?;
                    if !feet.feet.is_empty() {
                        return Err(SharedError::Unaligned(q));
                    }
                }
            }
            let mut out = Vec::new();
            for (k, sp) in (0u32..).zip(es) {
                let back = [sp[2], sp[1], sp[0]];
                if gs.iter().any(|g| *g == *sp || *g == back) {
                    out.push((k, [0.0, 1.0]));
                    continue;
                }
                for (end, at) in [(0, 0.0), (2, 1.0)] {
                    let p = [sp[end][0], sp[end][1], sp[end][2]];
                    if g_pts.contains(&p) {
                        out.push((k, [at, at]));
                    }
                }
            }
            Ok(out)
        }
        _ => Err(SharedError::Shape),
    }
}

/// The common range of the lines `a→b` and `g0→g1` on `a→b`'s parameter,
/// exactly, rounded outwards: none where they don't meet, a point where
/// they meet at an end. [`SharedError::Shape`] unless the four points are
/// on one line and `a ≠ b`.
fn line_overlap(
    a: [f64; 3],
    b: [f64; 3],
    g0: [f64; 3],
    g1: [f64; 3],
) -> Result<Vec<Piece>, SharedError> {
    let pt = |p: [f64; 3]| -> Result<[Rational; 3], SharedError> {
        let [x, y, z] = p.map(|c| Rational::try_from(c).map_err(|_| SharedError::Shape));
        Ok([x?, y?, z?])
    };
    let sub = |p: &[Rational; 3], o: &[Rational; 3]| -> [Rational; 3] {
        [0, 1, 2].map(|i| &p[i] - &o[i])
    };
    let dot = |u: &[Rational; 3], v: &[Rational; 3]| -> Rational {
        (0..3).fold(Rational::from(0u32), |s, i| s + &u[i] * &v[i])
    };
    let zero = Rational::from(0u32);
    let a = pt(a)?;
    let u = sub(&pt(b)?, &a);
    let uu = dot(&u, &u);
    if uu == zero {
        return Err(SharedError::Shape);
    }
    let mut ts = Vec::new();
    for g in [g0, g1] {
        let w = sub(&pt(g)?, &a);
        let c = [
            &u[1] * &w[2] - &u[2] * &w[1],
            &u[2] * &w[0] - &u[0] * &w[2],
            &u[0] * &w[1] - &u[1] * &w[0],
        ];
        if c.iter().any(|x| *x != zero) {
            return Err(SharedError::Shape);
        }
        ts.push(dot(&u, &w) / &uu);
    }
    let one = Rational::from(1u32);
    let (lo, hi) = if ts[0] <= ts[1] {
        (&ts[0], &ts[1])
    } else {
        (&ts[1], &ts[0])
    };
    let lo = if *lo > zero { lo.clone() } else { zero };
    let hi = if *hi < one { hi.clone() } else { one };
    if lo > hi {
        return Ok(Vec::new());
    }
    let lo_f = f64::rounding_from(&lo, Floor).0;
    let hi_f = f64::rounding_from(&hi, Ceiling).0;
    Ok(vec![(0, [lo_f, hi_f])])
}

/// The sign of `h₁` over the pieces at `like`'s level: `Ok(None)` for no
/// pieces, [`SharedError::Crossed`] where two pieces certify opposite
/// signs, [`SharedError::Unsettled`] where a piece isn't certified within
/// [`MAX_H_PIECES`] evaluations, halving each piece breadth first down
/// to [`MIN_H_PIECE`].
fn h_over<S: Curved, G: XGeometry>(
    geom: &G,
    e: EdgeId,
    curve: &XCurve<'_>,
    pieces: &[Piece],
    like: &S,
) -> Result<Option<Sign>, SharedError> {
    // Breadth first, so both signs show over the whole overlap before any
    // piece is refined far.
    let mut work: VecDeque<Piece> = pieces.iter().copied().collect();
    let mut sign = None;
    let mut unsettled = false;
    let mut evals = 0usize;
    while let Some((k, [lo, hi])) = work.pop_front() {
        evals += 1;
        if evals > MAX_H_PIECES {
            return Err(SharedError::Unsettled);
        }
        let c = curve.span(k, like).ok_or(SharedError::Shape)?;
        let t = if lo == hi {
            S::from_f64(lo, like)
        } else {
            between(lo, hi, like)
        };
        let (_, tangent) = curve_point(&c, &t);
        let rates = geom.edge_rates(Operand::A, e, k, &t);
        let h = shared_crossing(&tangent, &rates).ord[0].sign();
        match h {
            Some(o) if o != Ordering::Equal => {
                let s = if o == Ordering::Greater {
                    Sign::Pos
                } else {
                    Sign::Neg
                };
                if sign.is_some_and(|x| x != s) {
                    return Err(SharedError::Crossed);
                }
                sign = Some(s);
            }
            _ => {
                let m = lo + (hi - lo) * 0.5;
                if hi - lo > MIN_H_PIECE && lo < m && m < hi {
                    work.push_back((k, [lo, m]));
                    work.push_back((k, [m, hi]));
                } else {
                    unsettled = true;
                }
            }
        }
    }
    if unsettled {
        Err(SharedError::Unsettled)
    } else {
        Ok(sign)
    }
}

/// The enclosure of the parameter where `q`'s `r̂` line meets the line
/// from `p0` to `p1`, `ν·(q − p₀)/ν·(p₁ − p₀)`; `None` unless the
/// denominator is certainly non-zero.
pub(crate) fn line_param(q: [f64; 3], p0: [f64; 3], p1: [f64; 3]) -> Option<[f64; 2]> {
    let nu = [NU.x, NU.y, NU.z];
    let dot = |u: [f64; 3], v: [f64; 3]| {
        (0..3).fold(Bounded::exact(0.0), |s, i| {
            s + Bounded::exact(nu[i]) * (Bounded::exact(u[i]) - Bounded::exact(v[i]))
        })
    };
    let den = dot(p1, p0);
    if den.sign().is_none_or(|o| o == Ordering::Equal) {
        return None;
    }
    let at = Scalar::enclose(&(dot(q, p0) / den));
    (at[0].is_finite() && at[1].is_finite()).then_some(at)
}

/// The stored position of a cut point (module docs, "Cut points"): the
/// point of `curve` (the cut edge, along its stored direction) at the
/// middle of `part`, evaluated exactly and rounded to the nearest `f64`
/// per coordinate; exactly the span's end control point where the piece
/// is that end. `None` if the curve has no such span, a parameter or
/// weight isn't finite or the weights sum to 0 there.
pub fn cut_point(curve: &XCurve<'_>, part: &XPart) -> Option<[f64; 3]> {
    let [lo, hi] = part.t;
    if lo == hi && (lo == 0.0 || lo == 1.0) {
        let at = if lo == 0.0 { Dyadic::ZERO } else { Dyadic::ONE };
        return curve.span_end(part.span, at);
    }
    let q = |x: f64| Rational::try_from(x).ok();
    let t = (q(lo)? + q(hi)?) / Rational::from(2u32);
    let one = Rational::from(1u32);
    let u = &one - &t;
    let (pts, basis): (Vec<[f64; 4]>, Vec<Rational>) = match curve {
        XCurve::Line(a, b) if part.span == 0 => (
            vec![[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 1.0]],
            vec![u, t],
        ),
        XCurve::Line(..) => return None,
        XCurve::Spans(s) => {
            let sp = s.get(usize::try_from(part.span).ok()?)?;
            let b = vec![&u * &u, Rational::from(2u32) * &u * &t, &t * &t];
            (sp.to_vec(), b)
        }
    };
    let mut num = [0, 1, 2].map(|_| Rational::from(0u32));
    let mut den = Rational::from(0u32);
    for (p, b) in pts.iter().zip(&basis) {
        let wb = q(p[3])? * b;
        for k in 0..3 {
            num[k] += q(p[k])? * &wb;
        }
        den += wb;
    }
    if den == 0u32 {
        return None;
    }
    let out = num.map(|n| f64::rounding_from(&(n / &den), Nearest).0);
    out.iter().all(|x| x.is_finite()).then_some(out)
}

mod sides;

pub(crate) use sides::from_held;
pub use sides::{XBase, XGeometry, XSideCrossing, XSideRec, XSides, XWant};

#[cfg(test)]
mod tests;
