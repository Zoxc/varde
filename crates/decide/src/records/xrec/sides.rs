//! The side and corner records of an edge pair's square: what a box side
//! inside the square reads where its shadow can't be certified off 0.
//!
//! **Inner sides.** A box side inside the square holds one parameter at a
//! dyadic value: on `s = s₀` it is `C_e` about the point `C_g(s₀)`, on `t
//! = t₀` it is `C_g` about `C_e(t₀)` (the box degrees,
//! [`crate::count::xrec`]). Where the other curve's shadow passes through
//! the held point's (two knots on one shadow point, a knot on the other
//! edge's shadow, the shadows overlapping), no level certifies the side;
//! it is then counted under the perturbation like a ray record: the signed
//! crossings of the other edge's shadow by the held point's ray (`−r̂` for
//! a point of `A`, `+r̂` for one of `B`), the held point moving with its
//! edge, each crossing's sign `sign(ρ × T)` with `T` the other edge's
//! tangent along its stored direction ([`XSideRec`]). Those are exactly
//! the laps of the side about 0 counted against `+r̂` in the square's
//! plane, as the four terms are.
//!
//! **One record per held point** ([`XSideKey`]). The boxes on the two sides
//! of a line `s = s₀` are refined independently, so the record is kept for
//! the maximal segment: the held point against the **whole** other edge,
//! keyed by the pair, the held operand and the point ([`EdgeAt`], one key
//! whichever span reaches it), `(e, g, dyadic side)`. Its crossings carry
//! their span and parameter enclosure; a box side counts those in its
//! part ([`XSideRec::count_over`]), so every partition counts each crossing
//! once. A curved other edge is decided span by span as a span ray record
//! is ([`super::super::ray`], module docs there), with the held point as
//! the query, a stored point where it is a vertex or knot, else the exact
//! homogeneous point of its span at the dyadic parameter; a straight one
//! as the flat ray record on a line ([`super::super::shadow`]). Like a
//! span record it keeps the tangencies with the ray's line ahead of the
//! held point whose two moved crossings cancel ([`XSideRec::touches`]): a
//! box side cut inside one isn't ordered against them.
//!
//! **Corners, half-open** ([`XTieKey`]). A box corner is a point of `e`
//! against one of `g`, `D = C_e(t) − C_g(s)`: whether `g`'s point lies to
//! the left of `e`'s point's `r̂` line, and whether it is ahead along it,
//! the flat point–vertex shadow row between the two moving points, decided
//! exactly on the moved configuration (oriented from `e`'s point, `A`'s, to
//! `g`'s, as the point–vertex records are). A side record reads its other
//! edge's vertices and knots from these, so the held point's ray through a
//! knot or vertex is counted once along the edge; a box side certified off
//! 0 but ending on its ray's line takes that end's side from its corner;
//! and a box side whose cut lies inside a crossing's enclosure orders the
//! crossing against the cut by the cut's corner (a piece's one crossing
//! lies before the cut exactly where the cut's point is on the side the
//! crossing goes to). Where both points are vertices, or one a vertex and
//! the other a knot, the fact is already the point–vertex or knot record
//! of the four terms' ray records: it is read from there, never decided
//! again, so the inner sides and the four terms meet consistently at the
//! square's sides.
//!
//! **Decided once.** The records live in two [`Table`]s owned by
//! [`XSides`]; their deciding functions run only inside [`XSides::fill`]:
//! one charged [`Pass`] for the corners asked and those the asked sides'
//! ends read, one for the sides, reading those. A reader asks for what it
//! misses one key at a time ([`XWant`]).

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor};
use smallvec::SmallVec;

use crate::bezier::{Dyadic, Scalar};
use crate::ids::EdgeId;
use crate::num::bounded::Bounded;
use crate::num::decide::{Band, Fail, Row, Sign, TieKey};
use crate::num::exact::ORDERS;
use crate::perturb::{
    Curved, HomPairShadow, HomPoint, HomPointLineShadow, HomShadow, LineShadow, Moving, NU,
    QueryPoint, Rate, Seg, decide_flat,
};
use crate::records::keys::{EdgeAt, Operand, Point, RecordKey, XSideKey, XTieKey};
use crate::records::ray::{
    CurveGeometry, CurvedRays, SpanCurve, SpanRates, bernstein, span_crossings,
};
use crate::records::shadow::{
    Cost, Decided, Pass, ShadowError, Shadows, SideAhead, neg, replay, shadow_error,
    side_and_ahead, side_and_ahead_by, table_error,
};
use crate::records::side::{Over, count_part};
use crate::records::table::{Fresh, Table};

use super::XCurve;

/// The geometry the side and corner records of an edge pair read, beyond
/// the curved ray records'.
pub trait XGeometry: CurveGeometry {
    /// Edge `g` of `operand` as the pair's square reads it
    /// ([`XPair`](super::XPair)): a straight edge's two vertex points, a
    /// curved one's spans ([`CurveGeometry::spans`]).
    fn x_curve(&self, operand: Operand, g: EdgeId) -> XCurve<'_>;

    /// The rates of edge `g` of `operand`'s dyadic point `at` for a record
    /// against a point of the other operand: at a vertex its
    /// [`point`](crate::records::shadow::ShadowGeometry::point)'s, at an
    /// inner knot its [`knot_rates`](CurveGeometry::knot_rates)', elsewhere
    /// the direction field's value there on `A`, zero on `B`.
    fn edge_point_rates(&self, operand: Operand, g: EdgeId, at: EdgeAt) -> [Rate; ORDERS];

    /// The rates of span `span` of edge `g` of `operand` (span 0 of a
    /// straight edge) at the parameter `t` (an enclosure at its level), for
    /// a record against a point of the other operand: as
    /// [`span_rates`](CurveGeometry::span_rates).
    fn edge_rates<S: Curved>(
        &self,
        operand: Operand,
        g: EdgeId,
        span: u32,
        t: &S,
    ) -> [[S; 3]; ORDERS];

    /// Straight edge `g` of `operand`, from its lower-id end to its
    /// higher, moving at its rates at the foot of the shadow of a point at
    /// `near` (as [`line`](crate::records::shadow::ShadowGeometry::line)).
    fn edge_line(&self, operand: Operand, g: EdgeId, near: DVec3) -> Seg;

    /// Edge `g` of `operand`'s near radius `r_g = 3·β_g` (finite, `≥ 0`):
    /// a held point inside the edge takes it, so a third of it is the
    /// held point's band (a vertex takes its own, `r_w`).
    fn edge_near_radius(&self, operand: Operand, g: EdgeId) -> f64;
}

/// One crossing of an edge's shadow by a held point's ray (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XSideCrossing {
    /// The span of the edge, from 0 at its lower-id end (0 on a straight
    /// edge).
    pub span: u32,
    /// `[lo, hi]`, `f64`s with the crossing's parameter on the span's
    /// `[0, 1]` between them (`[0, 0]` or `[1, 1]` at the span's end).
    pub t: [f64; 2],
    /// `sign(ρ × T)` seen from above, `ρ` the held point's ray, `T` the
    /// edge's tangent along its stored direction.
    pub sign: Sign,
    /// Whether it is the one crossing of a piece whose ends lie on either
    /// side of the ray's line: only such a crossing is ordered against a
    /// cut inside its enclosure.
    pub single: bool,
}

/// A held point's ray against the other edge of the pair (module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XSideRec {
    /// The crossings, span by span along the edge's stored direction, in
    /// increasing parameter.
    pub crossings: SmallVec<[XSideCrossing; 2]>,
    /// The spans' tangencies with the ray's line ahead of the held point,
    /// whose two moved crossings cancel, each with its span
    /// ([`SpanRec::touches`](crate::records::ray::SpanRec::touches)).
    pub touches: SmallVec<[(u32, [f64; 2]); 1]>,
}

impl XSideRec {
    /// The crossings summed as `±1`s.
    pub fn count(&self) -> i64 {
        self.crossings
            .iter()
            .map(|c| i64::from(c.sign.as_i8()))
            .sum()
    }

    /// The count over the part `[lo, hi]` of span `span`'s parameter (`0 ≤
    /// lo < hi ≤ 1`), for the ray of a held point of `of`: the crossings
    /// in it, a cut inside the span inside a crossing's enclosure ordered
    /// by `sides`, the side of the cut's point against the ray's line
    /// ([`SideRec::count_over`](crate::records::side::SideRec::count_over)).
    pub fn count_over(
        &self,
        of: Operand,
        span: u32,
        part: [f64; 2],
        sides: [Option<Sign>; 2],
    ) -> Result<i64, Over> {
        let touches: SmallVec<[[f64; 2]; 1]> = self
            .touches
            .iter()
            .filter(|t| t.0 == span)
            .map(|t| t.1)
            .collect();
        count_part(
            self.crossings
                .iter()
                .filter(|c| c.span == span)
                .map(|c| (c.t, c.sign, c.single)),
            &touches,
            of,
            part,
            sides,
        )
    }
}

/// What a reader of the side records misses: a side record (and the
/// corners its ends read), or a corner's alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum XWant {
    /// A held point's record.
    Side(XSideKey),
    /// A corner's record.
    Tie(XTieKey),
}

impl From<XSideKey> for XWant {
    fn from(k: XSideKey) -> XWant {
        XWant::Side(k)
    }
}

impl From<XTieKey> for XWant {
    fn from(k: XTieKey) -> XWant {
        XWant::Tie(k)
    }
}

/// The side and corner records of a run's edge pairs, decided once per
/// key (module docs).
#[derive(Debug, Default)]
pub struct XSides {
    side: Table<XSideKey, XSideRec>,
    tie: Table<XTieKey, SideAhead>,
}

/// The decided records a corner of two vertices, or of a vertex and a
/// knot, is read from.
#[derive(Clone, Copy)]
pub struct XBase<'a> {
    /// The point–vertex records.
    pub shadows: &'a Shadows,
    /// The curved ray records, with their knot records.
    pub curved: &'a CurvedRays,
}

impl XSides {
    /// No records.
    pub fn new() -> XSides {
        XSides::default()
    }

    /// Decides the records of `wanted` not yet stored: first the corners
    /// asked and those the asked sides read (the held point against each
    /// vertex and knot of the other edge), in one pass, then the sides, in
    /// another, at the curved band `band`. A corner of two vertices, or of
    /// a vertex and a knot, is read from `base` (module docs), which must
    /// hold it: the four terms' records. A key the geometry doesn't fit (a
    /// point past its edge, a span whose end isn't its vertex's or the next
    /// span's start exactly) fails its pass ([`Fail::Internal`]).
    pub fn fill<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = impl Into<XWant>>,
        base: XBase<'_>,
        geom: &G,
        band: Band,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: XGeometry + Sync,
        P: Pass,
    {
        let (mut sides, mut ties) = (Vec::new(), Vec::new());
        for w in wanted {
            match w.into() {
                XWant::Side(k) => sides.push(k),
                XWant::Tie(k) => ties.push(k),
            }
        }
        let batch = self.side.missing(sides);
        for k in batch.keys() {
            let k = *k.key();
            let n = geom.x_curve(k.held.other(), other_edge(&k)).span_count();
            ties.extend((0..=n).map(|span| {
                tie_key(
                    &k,
                    EdgeAt {
                        span,
                        at: Dyadic::ZERO,
                    },
                )
            }));
        }
        self.tie
            .fill(ties, |keys| {
                pass.run(keys, &|k: &Fresh<XTieKey>| decide_tie(k, base, geom))
            })
            .map_err(|e| shadow_error(e, RecordKey::XTie))?;
        if batch.is_empty() {
            return Ok(());
        }
        let tie = &self.tie;
        let values = pass
            .run(batch.keys(), &|k: &Fresh<XSideKey>| {
                decide_side(k, tie, geom, band)
            })
            .map_err(ShadowError::Pass)?;
        self.side
            .insert(batch, values)
            .map_err(|e| ShadowError::Table(table_error(e, RecordKey::XSide)))
    }

    /// The side record of key `k`; `None` if it isn't decided.
    pub fn side(&self, k: &XSideKey) -> Option<&XSideRec> {
        self.side.get(k)
    }

    /// The corner record of key `k`, oriented from `e`'s point to `g`'s;
    /// `None` if it isn't decided.
    pub fn tie(&self, k: &XTieKey) -> Option<SideAhead> {
        self.tie.get(k).copied()
    }

    /// The side table, for its reads and keys.
    pub fn side_table(&self) -> &Table<XSideKey, XSideRec> {
        &self.side
    }

    /// The corner table, for its reads and keys.
    pub fn tie_table(&self) -> &Table<XTieKey, SideAhead> {
        &self.tie
    }

    /// Decides every stored record again, on `geom` at `band`, reading
    /// `base`, and gives the key of the first whose answer differs
    /// (corners first, then sides, each table in key order): a check that
    /// the answers are functions of their keys.
    pub fn replay<G: XGeometry>(
        &self,
        base: XBase<'_>,
        geom: &G,
        band: Band,
    ) -> Result<(), RecordKey> {
        replay(&self.tie, |k| decide_tie(k, base, geom), RecordKey::XTie)?;
        replay(
            &self.side,
            |k| decide_side(k, &self.tie, geom, band),
            RecordKey::XSide,
        )
    }
}

/// The edge a side record's held point is on.
fn held_edge(k: &XSideKey) -> EdgeId {
    match k.held {
        Operand::A => k.x.e,
        Operand::B => k.x.g,
    }
}

/// The edge a side record counts the crossings of.
fn other_edge(k: &XSideKey) -> EdgeId {
    match k.held {
        Operand::A => k.x.g,
        Operand::B => k.x.e,
    }
}

/// The corner of a side record's held point and the other edge's point
/// `o`.
pub(crate) fn tie_key(k: &XSideKey, o: EdgeAt) -> XTieKey {
    match k.held {
        Operand::A => XTieKey {
            x: k.x,
            e: k.at,
            g: o,
        },
        Operand::B => XTieKey {
            x: k.x,
            e: o,
            g: k.at,
        },
    }
}

/// The other edge's point of a corner against the held point's `r̂` line,
/// oriented from the held point, from the corner's record (oriented from
/// `e`'s point to `g`'s).
pub(crate) fn from_held(held: Operand, r: SideAhead) -> SideAhead {
    match held {
        Operand::A => r,
        Operand::B => SideAhead {
            side: neg(r.side),
            ahead: neg(r.ahead),
        },
    }
}

/// Edge `g` of `operand`'s dyadic point `at` on `curve`, moving at its
/// rates: a stored point at a vertex or knot, else the exact homogeneous
/// point of its span; `None` past the edge or with a weight not positive.
fn edge_point<G: XGeometry>(
    geom: &G,
    operand: Operand,
    g: EdgeId,
    curve: &XCurve<'_>,
    at: EdgeAt,
) -> Option<QueryPoint> {
    let n = curve.span_count();
    if at.span > n || (at.span == n && at.at != Dyadic::ZERO) {
        return None;
    }
    let rates = geom.edge_point_rates(operand, g, at);
    if at.stored() {
        let p = if at.span < n {
            curve.span_end(at.span, Dyadic::ZERO)?
        } else {
            curve.span_end(n - 1, Dyadic::ONE)?
        };
        return Some(QueryPoint::Flat(Moving {
            at: DVec3::from_array(p),
            rates,
        }));
    }
    let t = at.at.rational();
    let terms = match curve {
        XCurve::Line(a, b) => {
            let one = Rational::from(1u32);
            vec![
                (DVec3::from_array(*a), &one - &t),
                (DVec3::from_array(*b), t),
            ]
        }
        XCurve::Spans(spans) => {
            let sp = spans.get(usize::try_from(at.span).ok()?)?;
            let mut terms = Vec::with_capacity(3);
            for (p, b) in sp.iter().zip(bernstein(&t)) {
                if p[3].partial_cmp(&0.0) != Some(Ordering::Greater) {
                    return None;
                }
                terms.push((
                    DVec3::new(p[0], p[1], p[2]),
                    b * Rational::try_from(p[3]).ok()?,
                ));
            }
            terms
        }
    };
    Some(QueryPoint::Hom(HomPoint { terms, rates }))
}

/// Decides a corner's record (module docs): read from the point–vertex or
/// knot record where both points are vertices or one a vertex and the
/// other a knot, else by the flat point–vertex shadow row between the two
/// moving points.
fn decide_tie<G: XGeometry>(k: &Fresh<XTieKey>, base: XBase<'_>, geom: &G) -> Decided<SideAhead> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::VertexVertexShadow,
        record: RecordKey::XTie(key),
    };
    let internal = (Err(Fail::Internal { key: tie }), Cost::default());
    let (ce, cg) = (
        geom.x_curve(Operand::A, key.x.e),
        geom.x_curve(Operand::B, key.x.g),
    );
    let (ne, ng) = (ce.span_count(), cg.span_count());
    let (ends_e, ends_g) = (
        geom.ends(Operand::A, key.x.e),
        geom.ends(Operand::B, key.x.g),
    );
    let read = match (key.e.end(ne), key.e.knot(ne), key.g.end(ng), key.g.knot(ng)) {
        (Some(i), _, Some(j), _) => Some(
            base.shadows
                .vv(Point::vert(Operand::A, ends_e[i]), ends_g[j])
                .map(|r| r.place),
        ),
        (Some(i), _, _, Some(kg)) => Some(base.curved.knot(
            Point::vert(Operand::A, ends_e[i]),
            key.x.g,
            kg,
        )),
        (_, Some(ke), Some(j), _) => Some(
            base.curved
                .knot(Point::vert(Operand::B, ends_g[j]), key.x.e, ke)
                .map(|r| from_held(Operand::B, r)),
        ),
        _ => None,
    };
    if let Some(r) = read {
        // The four terms' records are decided before any corner: a missing
        // one is the caller's bug.
        return match r {
            Some(r) => (Ok(r), Cost::default()),
            None => internal,
        };
    }
    let (Some(pe), Some(pg)) = (
        edge_point(geom, Operand::A, key.x.e, &ce, key.e),
        edge_point(geom, Operand::B, key.x.g, &cg, key.g),
    ) else {
        return internal;
    };
    let (rec, w) = match (&pe, &pg) {
        (QueryPoint::Flat(q), QueryPoint::Flat(w)) => side_and_ahead(q, w, tie),
        (QueryPoint::Flat(q), QueryPoint::Hom(w)) => side_and_ahead_by(
            |axis| HomShadow {
                q: q.clone(),
                w: w.clone(),
                axis,
            },
            tie,
        ),
        (q, w) => {
            let (q, w) = (q.hom(), w.hom());
            side_and_ahead_by(
                |axis| HomPairShadow {
                    q: q.clone(),
                    w: w.clone(),
                    axis,
                },
                tie,
            )
        }
    };
    (rec, Cost::exact(w))
}

/// A span of the other edge, for its rates.
struct EdgeRates<'a, G> {
    geom: &'a G,
    operand: Operand,
    g: EdgeId,
    span: u32,
}

impl<G: XGeometry> SpanRates for EdgeRates<'_, G> {
    fn rates_at<S: Curved>(&self, t: &S) -> [[S; 3]; ORDERS] {
        self.geom.edge_rates(self.operand, self.g, self.span, t)
    }
}

/// Decides a side record (module docs): span by span as a span ray record
/// with the held point as the query, or on a straight edge as the flat
/// ray record; the ends' sides from the corners.
fn decide_side<G: XGeometry>(
    k: &Fresh<XSideKey>,
    ties: &Table<XTieKey, SideAhead>,
    geom: &G,
    band: Band,
) -> Decided<XSideRec> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::VertexCurveShadow,
        record: RecordKey::XSide(key),
    };
    let internal = (Err(Fail::Internal { key: tie }), Cost::default());
    let (held, other) = (key.held, key.held.other());
    let (he, oe) = (held_edge(&key), other_edge(&key));
    let (hc, oc) = (geom.x_curve(held, he), geom.x_curve(other, oe));
    let Some(q) = edge_point(geom, held, he, &hc, key.at) else {
        return internal;
    };
    // The held point's band: a third of its near radius, its vertex's at
    // an end of its edge, else its edge's.
    let held_near = match key.at.end(hc.span_count()) {
        Some(i) => geom.near_radius(Point::vert(held, geom.ends(held, he)[i])),
        None => geom.edge_near_radius(held, he),
    } / 3.0;
    let n = oc.span_count();
    // The other edge's point `span` (a vertex or knot) against the held
    // point's ray: its side and whether it is on the ray.
    let end = |span: u32| -> Option<(Sign, bool)> {
        let r = ties.get(&tie_key(
            &key,
            EdgeAt {
                span,
                at: Dyadic::ZERO,
            },
        ))?;
        let r = from_held(held, *r);
        Some((r.side, r.on_ray(held)))
    };
    // Each end must be its vertex's point (the corner's record reads it
    // there), and each knot both spans' joining point.
    let vertex = |j: usize| geom.point(Point::vert(other, geom.ends(other, oe)[j])).at;
    let mut rec = XSideRec::default();
    match oc {
        XCurve::Line(a, b) => {
            if vertex(0) != DVec3::from_array(a) || vertex(1) != DVec3::from_array(b) {
                return internal;
            }
            let (Some((s0, on0)), Some((s1, on1))) = (end(0), end(1)) else {
                // The batch's corners are filled first: a missing one is a bug.
                return internal;
            };
            if s0 == s1 {
                return (Ok(rec), Cost::default());
            }
            // `ρ × T` seen from above: `ν·T` along `+r̂`, of the sign of
            // `s1` once the ends are on either side.
            let sign = match held {
                Operand::A => neg(s1),
                Operand::B => s1,
            };
            let (on, work) = if on0 == on1 {
                (on0, Default::default())
            } else {
                let near = match &q {
                    QueryPoint::Flat(m) => m.at,
                    QueryPoint::Hom(h) => {
                        let m = h.at_level(&Bounded::exact(0.0)).at;
                        let mid = |x: &Bounded<f64>| {
                            let [lo, hi] = x.enclose();
                            lo + (hi - lo) * 0.5
                        };
                        DVec3::new(mid(&m[0]), mid(&m[1]), mid(&m[2]))
                    }
                };
                let line = geom.edge_line(other, oe, near);
                let (left, work) = match &q {
                    QueryPoint::Flat(m) => decide_flat(&LineShadow { q: m.clone(), line }, tie),
                    QueryPoint::Hom(h) => {
                        let end = |p: DVec3| {
                            HomPoint::of_moving(&Moving {
                                at: p,
                                rates: line.rates.clone(),
                            })
                        };
                        decide_flat(
                            &HomPointLineShadow {
                                q: h.clone(),
                                a: end(line.a),
                                b: end(line.b),
                            },
                            tie,
                        )
                    }
                };
                match left {
                    Ok(left) => (left == sign, work),
                    Err(e) => return (Err(e), Cost::exact(work)),
                }
            };
            if on {
                rec.crossings.push(XSideCrossing {
                    span: 0,
                    t: line_at(&q, a, b).unwrap_or([0.0, 1.0]),
                    sign,
                    single: true,
                });
            }
            (Ok(rec), Cost::exact(work))
        }
        XCurve::Spans(spans) => {
            let mut cost = Cost::default();
            let last = spans.len();
            for (i, span) in spans.iter().enumerate() {
                let p = |k: usize| DVec3::new(span[k][0], span[k][1], span[k][2]);
                let start_ok = if i == 0 {
                    vertex(0) == p(0)
                } else {
                    let s = spans[i - 1][2];
                    DVec3::new(s[0], s[1], s[2]) == p(0)
                };
                if !start_ok || (i + 1 == last && vertex(1) != p(2)) {
                    return internal;
                }
                let Ok(i) = u32::try_from(i) else {
                    return internal;
                };
                let (Some(e0), Some(e1)) = (end(i), end(i + 1)) else {
                    return internal;
                };
                let end_rates = |j: usize| {
                    geom.edge_point_rates(
                        other,
                        oe,
                        EdgeAt {
                            span: i + j as u32,
                            at: Dyadic::ZERO,
                        },
                    )
                };
                let rates = EdgeRates {
                    geom,
                    operand: other,
                    g: oe,
                    span: i,
                };
                let (r, c) = span_crossings(
                    SpanCurve::Stored(span),
                    [e0, e1],
                    end_rates,
                    q.clone(),
                    held,
                    &rates,
                    band,
                    held_near,
                    tie,
                );
                cost = cost.and(c);
                match r {
                    Ok(r) => {
                        rec.crossings
                            .extend(r.crossings.iter().map(|c| XSideCrossing {
                                span: i,
                                t: c.t,
                                sign: c.sign,
                                single: c.single,
                            }));
                        rec.touches.extend(r.touches.iter().map(|&t| (i, t)));
                    }
                    Err(e) => return (Err(e), cost),
                }
            }
            debug_assert!(n as usize == last);
            (Ok(rec), cost)
        }
    }
}

/// The enclosure of the parameter where the `r̂` line of `q` meets the line
/// from `p0` to `p1`, `ν·(q − p₀)/ν·(p₁ − p₀)`; `None` unless the
/// denominator is non-zero.
fn line_at(q: &QueryPoint, p0: [f64; 3], p1: [f64; 3]) -> Option<[f64; 2]> {
    let h = q.hom();
    let r = |x: f64| Rational::try_from(x).ok();
    let nu = [NU.x, NU.y, NU.z].map(r);
    let dot = |v: [Rational; 3]| -> Option<Rational> {
        let mut s = Rational::from(0u32);
        for (n, x) in nu.iter().zip(v) {
            s += n.as_ref()? * x;
        }
        Some(s)
    };
    let mut x = [0, 1, 2].map(|_| Rational::from(0u32));
    let mut w = Rational::from(0u32);
    for (p, c) in &h.terms {
        for k in 0..3 {
            x[k] += r(p[k])? * c;
        }
        w += c;
    }
    if w == 0u32 {
        return None;
    }
    let a = [0, 1, 2].map(|k| r(p0[k]));
    let b = [0, 1, 2].map(|k| r(p1[k]));
    let (Some(a), Some(b)) = (
        a.iter().cloned().collect::<Option<Vec<_>>>(),
        b.iter().cloned().collect::<Option<Vec<_>>>(),
    ) else {
        return None;
    };
    let num = dot([0, 1, 2].map(|k| &x[k] / &w - &a[k]))?;
    let den = dot([0, 1, 2].map(|k| &b[k] - &a[k]))?;
    if den == 0u32 {
        return None;
    }
    let t = num / den;
    Some([
        f64::rounding_from(&t, Floor).0,
        f64::rounding_from(&t, Ceiling).0,
    ])
}
