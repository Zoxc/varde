//! The curved ray records: a point's ray against one span of a curved
//! edge of the other operand ([`SpanRec`]), each decided once per
//! canonical key and read by every consumer.
//!
//! **The frame** is the flat records' ([`super::shadow`]): shadows along
//! [`UP`], the ray of a point of `A` going `−r̂`, of a
//! point of `B` `+r̂` ([`R_HAT`]), [`NU`] the normal of the `r̂` line, to
//! its left seen from above.
//!
//! **Spans.** A curved edge is a chain of rational quadratic spans, each
//! three affine control points with their weights `(x, y, z, w)`, weights
//! positive, numbered along the edge's stored direction (from its lower-id
//! end), so a record is the same for both coedges of the edge, which flip
//! its sign as they need ([`SpanKey`]). The first span starts at the
//! edge's lower-id vertex, the last ends at the other; consecutive spans
//! share their joining control point (the same point, their weights may
//! differ), the edge's inner **knot**. A span passes exactly through its
//! end control points, so its ends are its vertices' and knots' points,
//! with nothing projected or rounded. Each level forms the span's
//! homogeneous coefficients `(w·x, w)` itself
//! ([`HomCurve::from_affine`]).
//!
//! **The record.** [`SpanRec`] lists the signed crossings of the span's
//! shadow by the point `q`'s ray, in increasing parameter, each with the
//! sign `sign(ρ × T)` seen from above, `ρ` the ray's **actual** direction
//! and `T` the span's tangent in its stored direction (the flat ray
//! record's convention: the signed crossings of a closed chain are its
//! winding number about `q`, for either ray direction). It is decided
//! from two polynomials of the span's degree, its point put into the
//! equations of `q`'s `r̂` line and of the line across it through `q`:
//!
//! - `g = ν·P − (ν·q)·W`, whose sign is the side of the span's point
//!   against the `r̂` line (`W > 0`); its roots on `[0, 1]` are isolated
//!   (certified simple roots, and clusters a level can't separate);
//! - `h = r̂·P − (r̂·q)·W`, whose sign says whether the point is ahead of
//!   `q` along `r̂`: a crossing is on the ray where `h` has the sign of the
//!   ray's direction.
//!
//! **Ends, half-open.** The sides of the span's two ends are read from
//! their points' own records, never from the span: the point–vertex
//! record ([`VvRec`](super::shadow::VvRec)) of an edge end, the knot
//! record ([`KnotRec`]) of an inner knot, which the spans on both sides
//! read. A span's end is that point exactly, so its record's side is the
//! sign of `g` there wherever `g` is not 0, and where it is (the end on
//! the ray line) the side of the end moved. A root at a span's end (a
//! cluster reaching `t = 0` or `t = 1`, where the level can't certify `g`)
//! takes that record's side there, so a ray through a vertex or knot is
//! counted once along the chain, by the half-open rule of the flat
//! records.
//!
//! **Pieces.** The span's parameter splits into pieces holding all of
//! `g`'s roots: the isolation's results (between them `g` is certified
//! non-zero). A cluster beside an end record with at most one root is
//! narrowed towards that end, halving while `g` keeps its certified sign,
//! so a ray through a vertex or knot reads `q` against the span right
//! there; where `g` next to the end is certified of the side other than
//! the end's record (the end on the ray line, moved off it), the span
//! crosses at the end itself, a piece `[0, 0]` or `[1, 1]`. A piece whose
//! two ends' sides differ is crossed once (a span is quadratic, so a piece with an odd
//! sign change holds one root) with the sign that change names; one whose
//! ends agree and that holds at most one root is not crossed. Its
//! crossing is on the ray when `h` is certified of the ray's sign over
//! the piece (its coefficient hull there), off it when of the other
//! sign, and in either case only when the records of the ends the piece
//! reaches agree; a root that doesn't settle so is first bracketed
//! narrower. A piece in the span's second half is worked on the span run
//! backwards, in `u = 1 − t`, where `f64` parameters resolve a root near
//! `t = 1` as finely as one near `t = 0`, so the span's two ends are
//! decided alike. Otherwise `q` lies on or near the span's shadow there, and
//! the **vertex–curve shadow row** ([`curve_shadow`]: `q` against the
//! span at the piece, its foot, the span moving at its rates there)
//! decides, by [`decide`], the side `s` of `q` against the span:
//!
//! - a piece crossed once, with sign `c`: the crossing is on the ray
//!   exactly when `s = c` (to the left of the span for a `+` crossing, as
//!   on a line; at a tie beside an end, `s` from the row at that end,
//!   below);
//! - a cluster whose ends agree, on side `o` (two roots it can't
//!   separate: a tangency of the span's shadow with the ray line at `q`):
//!   in the moved configuration the line meets the span twice, one
//!   crossing on the ray, with sign `s`, when `q` is moved into the side
//!   the span turns to, that is when `s·sign(r̂·T) = o`; else not at all.
//!   Where `q` is away from it, both crossings are on the ray or both off
//!   it, and they cancel.
//!
//! A span lying along `q`'s ray line (`g ≡ 0`, a conic in a vertical plane
//! along `r̂`) is one cluster as wide as the span at every level, where the
//! row over it never decides: at the last level `h`'s sign over a piece is
//! also certified by halving it, and such a cluster's row is read at `q`'s
//! foot, where `h` is 0 (`q` between the span's ends along `r̂`, or at
//! one). In the moved configuration the line then misses the span, which
//! the tangency rule gives.
//!
//! **Beside an end.** An end's record is exact data decided as a flat row
//! (only an exact zero is a tie, decided by the motions), the span's row a
//! curved one (a value within the band is a tie once its error reaches
//! the target). With `q` within the band of an end, beside but not on its
//! column, the record reads where `q` really is while the row at a
//! crossing next to the end, a tie, answers for `q` moved onto the span
//! there: a span's crossing at a knot could then be counted on the ray
//! where the knot's record puts it behind `q`, its neighbour's not. So a
//! once-crossed piece whose row is a tie, with the span certified within
//! the band of `q` (in both shadow directions) from one end to past the
//! piece, and not from the other end too, takes `s` from the
//! **row at that end** instead: `q` against the span's tangent line there,
//! through the end and the middle control point (a rational quadratic
//! with positive weights leaves its end along that line), all stored
//! points, so decided as a flat row, the line moving at the end's rates
//! (as in its record); where that row has no answer (the tangent's shadow
//! 0), the crossing is on the ray as the end's record is.
//!
//! *Each crossing at a vertex or knot is then counted once along the
//! chain, consistently with its record.* (1) The side at a shared end is
//! one value, its record's, read by the spans on both sides: `g` there is
//! `W·ν·(k − q)`, `W > 0`, of the record's sign wherever certified, and the
//! record's side stands where it isn't. (2) In a span the pieces hold all
//! of `g`'s roots and `g` is certified non-zero between them, of the sides
//! the pieces' ends take, so the sides at the piece ends form one sequence
//! from one end's record to the other's; joined at the shared ends, the
//! chain's sequence has each change of side in exactly one piece of one
//! span: at a vertex or knot, the change into its record's side in the
//! span before it, the change out of it in the span after, each counted
//! with the sign of that change. (3) Whether such a crossing is on the ray
//! is decided once, for its piece: by `h` or the row by value, the truth;
//! at a tie beside an end, by the row at that end, against the same exact
//! point the record reads (by the motions at an exact zero, as the record
//! is, the first-order answer of the moved configuration). The count is
//! then that of the chain with each such span replaced, from the end to
//! past the crossing, by its tangent line there, which passes through the
//! end point as its record decides it: a change within the band, as the
//! span is within the band of `q` there. A tie elsewhere is the moved
//! configuration's, `q` on the span at the piece.
//!
//! **Precision.** A record is decided at the first level (`f64` with
//! certified errors), then double-double, then big floats of growing
//! width, each level from the stored data again, until no step asks to
//! escalate: a row not yet decided at its band's target, a tangent or a
//! side the level can't certify. Past the last level the record's
//! [`Fail::Escalate`] stands. The curved band is the caller's, the
//! current rung's.
//!
//! **Decided once.** The records live in two [`Table`]s owned by
//! [`CurvedRays`]; their deciding functions run only inside
//! [`CurvedRays::fill`]: one charged [`Pass`] for the edge ends'
//! point–vertex records ([`Shadows::fill_vv`]), one for the knot records,
//! one for the spans, reading those.

use smallvec::SmallVec;

use crate::bezier::roots::{Dyadic, Found, Limits, MAX_ROOT_DEPTH, isolate};
use crate::bezier::{HomCurve, Poly, Scalar};
use crate::ids::EdgeId;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::{Bounded, add_up, down, mul_up};
use crate::num::dd::Dd;
use crate::num::decide::{Band, Coeff, Fail, Row, Sign, TieKey, decide};
use crate::num::exact::ORDERS;
use crate::perturb::{
    Curved, LineShadow, Moving, MovingAt, NU, R_HAT, Rate, Seg, ShadowAxis, VvShadow, curve_point,
    curve_shadow, decide_flat,
};
use crate::records::keys::{KnotKey, Operand, Point, RecordKey, SpanKey, VvKey};
use crate::records::shadow::{
    Cost, Decided, LEVELS, LevelWork, Pass, ShadowError, ShadowGeometry, Shadows, neg, ray_ahead,
    read_vv, replay, shadow_error, table_error,
};
use crate::records::table::{Fresh, Table};

/// A span: three affine control points with their weights `(x, y, z, w)`,
/// weights positive; its point is `Σ Bᵢ·wᵢ·xᵢ / Σ Bᵢ·wᵢ`.
pub type Span = [[f64; 4]; 3];

const _: () = assert!(LEVELS == 2 + Width::ALL.len());

/// The most splits one root isolation takes; a quadratic's isolation to
/// the deepest level stays far below.
const MAX_SPLITS: u32 = 4096;

/// The most halvings that certify `h`'s sign over a piece ([`split_sign`])
/// or find its root there ([`foot`]): where the coefficient hull of a
/// piece as wide as the span (a span lying along the ray line) doesn't.
const SETTLE_SPLITS: u32 = 6;

/// One crossing of a span's shadow by a point's ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossing {
    /// `[lo, hi]`, `f64`s with the crossing's parameter on the span's
    /// `[0, 1]` between them (an end, `[0, 0]` or `[1, 1]`, for a crossing
    /// at the span's end itself).
    pub t: [f64; 2],
    /// `sign(ρ × T)` seen from above, `ρ` the ray's actual direction, `T`
    /// the span's tangent in its stored direction.
    pub sign: Sign,
}

/// A point's ray against one span of a curved edge of the other operand
/// (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct SpanRec {
    /// The crossings, in increasing parameter.
    pub crossings: SmallVec<[Crossing; 2]>,
}

impl SpanRec {
    /// The crossings summed as `±1`s.
    pub fn count(&self) -> i32 {
        self.crossings
            .iter()
            .map(|c| i32::from(c.sign.as_i8()))
            .sum()
    }
}

/// A point against an inner knot of a curved edge of the other operand,
/// oriented from the point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KnotRec {
    /// The side of the knot against the point's `r̂` line, `ν·(k − q)`:
    /// `Pos` to its left seen from above.
    pub side: Sign,
    /// Whether the knot is ahead of the point along `r̂`, `r̂·(k − q)`.
    pub ahead: Sign,
}

impl KnotRec {
    /// Whether the knot lies the way the ray of the point (of `of`) goes.
    pub fn on_ray(&self, of: Operand) -> bool {
        self.ahead == ray_ahead(of)
    }
}

/// The geometry the curved ray records read, beyond the flat records'.
pub trait CurveGeometry: ShadowGeometry {
    /// The spans of curved edge `g` of `operand`, along its stored
    /// direction (module docs): the first starting at `ends(g)[0]`, the
    /// last ending at `ends(g)[1]`, those end control points being the
    /// vertices' positions exactly, and each inner knot the control point
    /// two spans share.
    fn spans(&self, operand: Operand, g: EdgeId) -> &[Span];

    /// The rates of curved edge `g` (of the operand other than `q`'s) at
    /// its inner knot `k`, for `q`'s record: the direction field there on
    /// `A`, zero on `B`.
    fn knot_rates(&self, q: Point, g: EdgeId, k: u32) -> [Rate; ORDERS];

    /// The rates of span `span` of `g` at the parameter `t` (an enclosure
    /// at its level), for `q`'s record: the direction field at the span's
    /// point there on `A`, zero on `B`.
    fn span_rates<S: Curved>(&self, q: Point, g: EdgeId, span: u32, t: &S) -> [[S; 3]; ORDERS];
}

/// A precision level the curved ray records are decided at.
pub trait RayLevel: Curved + Coeff {
    /// The same value with its error widened by `by ≥ 0`.
    fn widened(&self, by: f64) -> Self;
}

impl RayLevel for Bounded<f64> {
    fn widened(&self, by: f64) -> Self {
        self.with_trunc(by)
    }
}

impl RayLevel for Bounded<Dd> {
    fn widened(&self, by: f64) -> Self {
        self.with_trunc(by)
    }
}

impl<B: BigF> RayLevel for Bounded<B> {
    fn widened(&self, by: f64) -> Self {
        self.clone().with_trunc(by)
    }
}

/// The curved ray records of a run and the knot records they read,
/// decided once per key (module docs).
#[derive(Debug, Default)]
pub struct CurvedRays {
    span: Table<SpanKey, SpanRec>,
    knot: Table<KnotKey, KnotRec>,
}

impl CurvedRays {
    /// No records.
    pub fn new() -> CurvedRays {
        CurvedRays::default()
    }

    /// Decides the span records of `wanted` not yet stored: first the
    /// point–vertex records of the edge ends they reach, in one pass (into
    /// `shadows`), then the knot records they reach, in another, then the
    /// spans, in a third, at the curved band `band`. A key whose span the
    /// edge doesn't have fails its pass ([`Fail::Internal`]), as does a
    /// span whose end isn't exactly its vertex's point or the next span's
    /// start ([`CurveGeometry::spans`]).
    pub fn fill<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = SpanKey>,
        shadows: &mut Shadows,
        geom: &G,
        band: Band,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: CurveGeometry + Sync,
        P: Pass,
    {
        let batch = self.span.missing(wanted);
        if batch.is_empty() {
            return Ok(());
        }
        let mut vv = Vec::new();
        let mut knots = Vec::new();
        for k in batch.keys() {
            let op = k.q.operand.other();
            let n = geom.spans(op, k.g).len();
            let Ok(span) = usize::try_from(k.span) else {
                continue;
            };
            if span >= n {
                continue;
            }
            let ends = geom.ends(op, k.g);
            for (at, knot, end) in [(span, k.span, ends[0]), (span + 1, k.span + 1, ends[1])] {
                if at == 0 || at == n {
                    vv.push(VvKey::of(k.q, end).key);
                } else {
                    knots.push(KnotKey {
                        q: k.q,
                        g: k.g,
                        knot,
                    });
                }
            }
        }
        shadows.fill_vv(vv, geom, pass)?;
        self.knot
            .fill(knots, |keys| {
                pass.run(keys, &|k: &Fresh<KnotKey>| decide_knot(k, geom))
            })
            .map_err(|e| shadow_error(e, RecordKey::Knot))?;
        let (vv, knot) = (shadows.vv_table(), &self.knot);
        let values = pass
            .run(batch.keys(), &|k: &Fresh<SpanKey>| {
                decide_span(k, vv, knot, geom, band)
            })
            .map_err(ShadowError::Pass)?;
        self.span
            .insert(batch, values)
            .map_err(|e| ShadowError::Table(table_error(e, RecordKey::RaySpan)))
    }

    /// The record of point `q`'s ray against span `span` of curved edge
    /// `g` of the other operand; `None` if it isn't decided.
    pub fn span(&self, q: Point, g: EdgeId, span: u32) -> Option<&SpanRec> {
        self.span.get(&SpanKey { q, g, span })
    }

    /// The record of point `q` against inner knot `knot` of curved edge
    /// `g` of the other operand; `None` if it isn't decided.
    pub fn knot(&self, q: Point, g: EdgeId, knot: u32) -> Option<KnotRec> {
        self.knot.get(&KnotKey { q, g, knot }).copied()
    }

    /// The span table, for its reads and keys.
    pub fn span_table(&self) -> &Table<SpanKey, SpanRec> {
        &self.span
    }

    /// The knot table, for its reads and keys.
    pub fn knot_table(&self) -> &Table<KnotKey, KnotRec> {
        &self.knot
    }

    /// Decides every stored record again, on `geom` at `band`, and gives
    /// the key of the first whose answer differs (knot records first,
    /// each table in key order), reading the end records from `shadows`:
    /// a check that the answers are functions of their keys.
    pub fn replay<G: CurveGeometry>(
        &self,
        shadows: &Shadows,
        geom: &G,
        band: Band,
    ) -> Result<(), RecordKey> {
        replay(&self.knot, |k| decide_knot(k, geom), RecordKey::Knot)?;
        let vv = shadows.vv_table();
        replay(
            &self.span,
            |k| decide_span(k, vv, &self.knot, geom, band),
            RecordKey::RaySpan,
        )
    }
}

/// A control point's point, exactly.
fn point_of(p: &[f64; 4]) -> glam::DVec3 {
    glam::DVec3::new(p[0], p[1], p[2])
}

/// Decides a knot record, by the flat vertex–vertex shadow row against
/// the knot's point moving at the edge's rates there.
fn decide_knot<G: CurveGeometry>(k: &Fresh<KnotKey>, geom: &G) -> Decided<KnotRec> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::VertexVertexShadow,
        record: RecordKey::Knot(key),
    };
    let spans = geom.spans(key.q.operand.other(), key.g);
    let span = usize::try_from(key.knot)
        .ok()
        .filter(|&i| i >= 1)
        .and_then(|i| spans.get(i));
    let Some(span) = span else {
        return (Err(Fail::Internal { key: tie }), Cost::default());
    };
    let w = Moving {
        at: point_of(&span[0]),
        rates: geom.knot_rates(key.q, key.g, key.knot),
    };
    let q = geom.point(key.q);
    let row = |axis| VvShadow {
        q: q.clone(),
        w: w.clone(),
        axis,
    };
    let (side, w1) = decide_flat(&row(ShadowAxis::Side), tie);
    let (ahead, w2) = decide_flat(&row(ShadowAxis::Along), tie);
    let rec = side.and_then(|side| {
        Ok(KnotRec {
            side,
            ahead: ahead?,
        })
    });
    (rec, Cost::exact(w1.and(w2)))
}

/// A span record's inputs, read once for every level.
struct SpanCtx<'a, G> {
    key: SpanKey,
    span: &'a Span,
    /// The side of each end and whether it is on the ray, from its record.
    ends: [(Sign, bool); 2],
    /// At each end `q` is within the band of (in both shadow directions,
    /// or not certainly beyond), the row there ([`EndRow`]).
    near: [Option<EndRow>; 2],
    q: Moving,
    geom: &'a G,
    band: Band,
    tie: TieKey,
}

/// A stretch of the span holding some of `g`'s roots (module docs).
struct Piece {
    /// `[lo, hi]`, `f64`s, on the span, or on the span run backwards
    /// (`u = 1 − t`) if `rev`.
    t: [f64; 2],
    /// Whether `t` is on the span run backwards: so for a piece in the
    /// span's second half, where `f64` parameters resolve `u` as finely
    /// as they resolve `t` near `t = 0`.
    rev: bool,
    /// The side at the piece's lower and higher end on the span.
    sides: [Sign; 2],
    /// At an end the piece takes from a record, whether that record is on
    /// the ray.
    rec: [Option<bool>; 2],
    /// Whether it holds at most one root (else at most two: a cluster).
    one: bool,
}

/// The vertex–curve shadow row at an end of the span `q` is near: `q`
/// against the span's tangent line there, through the end and the middle
/// control point, stored points, so decided as a flat row (module docs,
/// "Beside an end").
struct EndRow {
    /// The side of `q` against the span there, `None` where the row has
    /// no answer (the tangent's shadow 0: the guard).
    side: Option<Sign>,
    /// Whether the end is on the ray, from its record.
    on: bool,
}

/// The vertex–curve shadow row at a piece ([`SpanCtx::row`]).
struct RowAt {
    /// The side of `q` against the span there, decided.
    s: Sign,
    /// `sign(r̂·T)` of the span's tangent there, `T` in the span's
    /// direction, if certified.
    along: Option<Sign>,
    /// Whether the row's value is within the band, so `s` is the
    /// perturbation's answer.
    tie: bool,
}

/// Decides a span record, at each level in turn until one decides it.
fn decide_span<G: CurveGeometry>(
    k: &Fresh<SpanKey>,
    vv: &Table<VvKey, super::shadow::VvRec>,
    knot: &Table<KnotKey, KnotRec>,
    geom: &G,
    band: Band,
) -> Decided<SpanRec> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::VertexCurveShadow,
        record: RecordKey::RaySpan(key),
    };
    let mut cost = Cost::default();
    let internal = Err(Fail::Internal { key: tie });
    let op = key.q.operand.other();
    let spans = geom.spans(op, key.g);
    let Some(i) = usize::try_from(key.span).ok().filter(|&i| i < spans.len()) else {
        return (internal, cost);
    };
    let edge_ends = geom.ends(op, key.g);
    // Each end is the point its record reads: the vertex's, or the next
    // span's start (the knot record's point); else the geometry breaks
    // its contract.
    let vertex_at = |v| geom.point(Point::vert(op, v)).at;
    let start = (i == 0).then(|| vertex_at(edge_ends[0]));
    let end_at = spans
        .get(i + 1)
        .map_or_else(|| vertex_at(edge_ends[1]), |s| point_of(&s[0]));
    if start.is_some_and(|p| p != point_of(&spans[i][0])) || end_at != point_of(&spans[i][2]) {
        return (internal, cost);
    }
    let end = |at: usize, knot_ix: u32, vertex| {
        if at == 0 || at == spans.len() {
            read_vv(vv, key.q, vertex).map(|r| (r.side, r.on_ray(key.q.operand)))
        } else {
            let r = knot.get(&KnotKey {
                q: key.q,
                g: key.g,
                knot: knot_ix,
            })?;
            Some((r.side, r.on_ray(key.q.operand)))
        }
    };
    let (Some(e0), Some(e1)) = (
        end(i, key.span, edge_ends[0]),
        end(i + 1, key.span + 1, edge_ends[1]),
    ) else {
        // The batch's ends are filled first: a missing one is a bug.
        return (internal, cost);
    };
    let span = &spans[i];
    let q = geom.point(key.q);
    // The rows at the ends `q` is near, decided once for every level.
    let near = [0, 1].map(|j| {
        let at = point_of(&span[2 * j]);
        if !near_end(q.at, at, band) {
            return None;
        }
        let rates = if (i + j == 0) || (i + j == spans.len()) {
            geom.point(Point::vert(op, edge_ends[j])).rates
        } else {
            geom.knot_rates(key.q, key.g, key.span + j as u32)
        };
        let line = Seg {
            a: point_of(&span[j]),
            b: point_of(&span[j + 1]),
            rates,
        };
        let (side, w) = decide_flat(&LineShadow { q: q.clone(), line }, tie);
        cost.exact = cost.exact.and(w);
        Some(EndRow {
            side: side.ok(),
            on: [e0, e1][j].1,
        })
    });
    let ctx = SpanCtx {
        key,
        span,
        ends: [e0, e1],
        near,
        q,
        geom,
        band,
        tie,
    };
    for level in 0..LEVELS {
        let work = &mut cost.levels[level];
        let r = match level {
            0 => ctx.at(&Bounded::exact(0.0), work, false),
            1 => ctx.at(&Bounded::exact(Dd::from_f64(0.0)), work, false),
            _ => {
                let w = Width::ALL[level - 2];
                let last = level + 1 == LEVELS;
                ctx.at(&Bounded::exact(Big::from_f64(0.0, w)), work, last)
            }
        };
        match r {
            Err(Fail::Escalate { .. }) if level + 1 < LEVELS => {}
            r => return (r, cost),
        }
    }
    unreachable!("the last level returns")
}

impl<G: CurveGeometry> SpanCtx<'_, G> {
    /// The record at the level of `like`, or why this level doesn't
    /// decide it; `last` at the last level, where a piece's sign along
    /// the ray and `q`'s foot on the span are also sought by halving.
    fn at<S: RayLevel>(&self, like: &S, work: &mut LevelWork, last: bool) -> Result<SpanRec, Fail> {
        let f = |x: f64| S::from_f64(x, like);
        let v3 = |v: glam::DVec3| [f(v.x), f(v.y), f(v.z)];
        let pts = self.span.iter().map(|p| ([p[0], p[1], p[2]], p[3]));
        let c = HomCurve::from_affine(pts, like).expect("three control points");
        if !c.weights_positive() {
            return Err(Fail::Internal { key: self.tie });
        }
        let q = v3(self.q.at);
        let line = |a: glam::DVec3| {
            let a = v3(a);
            let off = dot3(&a, &q).neg();
            c.compose_linear(&a, &off)
        };
        let (g, h) = (line(NU), line(R_HAT));
        // The span and its polynomials run backwards, `u = 1 − t`, for
        // the pieces in its second half.
        let (cr, gr, hr) = (c.reversed(), g.reversed(), h.reversed());
        let polys = [[&g, &h, c.weights()], [&gr, &hr, cr.weights()]];
        let limits = Limits {
            max_depth: MAX_ROOT_DEPTH,
            max_splits: MAX_SPLITS,
        };
        let roots = match isolate(&g, limits) {
            Ok(r) => r,
            Err(out) => {
                work.roots = work.roots.and(out.work);
                return Err(Fail::Escalate { order: 0 });
            }
        };
        work.roots = work.roots.and(roots.work);

        let [(s0, on0), (s1, on1)] = self.ends;
        let mut pieces: SmallVec<[Piece; 4]> = SmallVec::new();
        for found in &roots.found {
            // A result in the span's second half (a root's narrowed
            // bounds there) on the span run backwards: its ends mirrored
            // exactly as dyadics.
            let [lo, hi] = found.span();
            let rev = match found {
                Found::Root(r) => r.near[0] >= 0.5,
                Found::Cluster(_) => lo.bounds()[0] >= 0.5,
            };
            let t = if rev {
                [mirror(hi).bounds()[0], mirror(lo).bounds()[1]]
            } else {
                [lo.bounds()[0], hi.bounds()[1]]
            };
            match found {
                Found::Root(r) => {
                    // Krawczyk's bounds, kept where they are inside.
                    let n = if rev { mirrored(r.near) } else { r.near };
                    pieces.push(Piece {
                        t: [
                            if n[0] > t[0] { n[0] } else { t[0] },
                            if n[1] < t[1] { n[1] } else { t[1] },
                        ],
                        rev,
                        sides: [r.dir.reverse(), r.dir].map(|d| Sign::of(d).expect("a root's dir")),
                        rec: [None, None],
                        one: true,
                    });
                }
                Found::Cluster(cl) => {
                    let (start, end) = (cl.start.and_then(Sign::of), cl.end.and_then(Sign::of));
                    match (start, end) {
                        // At most one root, beside a record end: narrowed
                        // towards that end, on the span run backwards for
                        // the end at `t = 1`.
                        (None, Some(e)) if cl.mult <= 1 => {
                            let b = [lo.bounds()[0], hi.bounds()[1]][1];
                            let (b, split) = narrow(&g, b, e, like, work);
                            end_pieces(&mut pieces, b, split, (s0, on0), e, false);
                        }
                        (Some(e), None) if cl.mult <= 1 => {
                            let d = mirror(lo).bounds()[1];
                            let (b, split) = narrow(&gr, d, e, like, work);
                            let mut at_end = SmallVec::<[Piece; 2]>::new();
                            end_pieces(&mut at_end, b, split, (s1, on1), e, true);
                            pieces.extend(at_end.into_iter().rev());
                        }
                        _ => pieces.push(Piece {
                            t,
                            rev,
                            sides: [start.unwrap_or(s0), end.unwrap_or(s1)],
                            rec: [start.is_none().then_some(on0), end.is_none().then_some(on1)],
                            one: cl.mult <= 1,
                        }),
                    }
                }
            }
        }

        let ahead = ray_ahead(self.key.q.operand);
        let mut crossings = SmallVec::new();
        for p in &pieces {
            // `ρ × T` seen from above: `ν·T` along `+r̂`, the sign of the
            // side crossed to; negated along `−r̂`.
            let net = (p.sides[0] != p.sides[1]).then(|| match self.key.q.operand {
                Operand::A => neg(p.sides[1]),
                Operand::B => p.sides[1],
            });
            if net.is_none() && p.one {
                // Ends on one side and at most one root: none.
                continue;
            }
            let (pg, ph) = if p.rev { (&gr, &hr) } else { (&g, &h) };
            let settle = |t: [f64; 2], work: &mut LevelWork| {
                split_sign(ph, t, like, work, if last { SETTLE_SPLITS } else { 0 })
                    .map(|a| a == ahead)
                    .filter(|&on| p.rec.iter().flatten().all(|&r| r == on))
            };
            let mut t = p.t;
            let mut settled = settle(t, work);
            if settled.is_none() && net.is_some() && p.rec == [None, None] && t[0] < t[1] {
                // One root, bracketed: narrowed to where the level
                // can't tell its side, so the row reads `q` against the
                // span there.
                let start = if p.rev { p.sides[1] } else { p.sides[0] };
                t = bracket(pg, t, start, like, work);
                settled = settle(t, work);
            }
            let sign = match (settled, net) {
                (Some(on), Some(c)) => on.then_some(c),
                (Some(_), None) => None,
                (None, net) => {
                    let pc = if p.rev { &cr } else { &c };
                    let mut row = self.row(pc, t, p.rev, like, work);
                    if last && net.is_none() && matches!(row, Err(Fail::Escalate { .. })) {
                        // A cluster no level narrows, as wide as a span
                        // lying along the ray line: the row at `q`'s foot
                        // on the span, where `h` is 0 (`q` between its
                        // ends along `r̂` or at one).
                        let hs = if p.rev { [&hr, &h] } else { [&h, &hr] };
                        let switched;
                        (t, switched) = foot(hs, t, like, work);
                        let rev = p.rev != switched;
                        row = self.row(if rev { &cr } else { &c }, t, rev, like, work);
                        if switched {
                            // Back on the piece's parameter.
                            t = mirrored(t);
                        }
                    }
                    let RowAt { s, along, tie } = row?;
                    match net {
                        // A tie beside an end: the row at that end, flat
                        // (module docs, "Beside an end").
                        Some(c) => match tie.then(|| self.at_end(p, c, polys, like, work)) {
                            Some(Some(on)) => on.then_some(c),
                            _ => (s == c).then_some(c),
                        },
                        None => {
                            let along = along.ok_or(Fail::Escalate { order: 0 })?;
                            (times(s, along) == p.sides[0]).then_some(s)
                        }
                    }
                }
            };
            if let Some(sign) = sign {
                let t = if p.rev { mirrored(t) } else { t };
                crossings.push(Crossing { t, sign });
            }
        }
        Ok(SpanRec { crossings })
    }

    /// Whether the crossing of the piece `p`, of sign `c`, is on the ray,
    /// by the row at the end the span is within the band of `q` from to
    /// past the piece (module docs, "Beside an end"): `s = c` for
    /// the row's side `s`, or as the end's record where that row has no
    /// answer. `None` if there is no such end, or two. `polys` are `g`,
    /// `h` and the weight `W` on the span and on the span run backwards.
    fn at_end<S: RayLevel>(
        &self,
        p: &Piece,
        c: Sign,
        polys: [[&Poly<S>; 3]; 2],
        like: &S,
        work: &mut LevelWork,
    ) -> Option<bool> {
        let [lo, hi] = if p.rev { mirrored(p.t) } else { p.t };
        // From each end to past the piece, on the parameter that starts
        // at that end.
        let reach = [hi, one_minus(lo, true)];
        let mut ends = (0..2).filter(|&j| {
            self.near[j].is_some() && {
                work.evals = work.evals.saturating_add(4);
                within(polys[j], reach[j], self.band, like)
            }
        });
        let j = ends.next()?;
        if ends.next().is_some() {
            return None;
        }
        let end = self.near[j].as_ref()?;
        Some(match end.side {
            Some(s) => s == c,
            None => end.on,
        })
    }

    /// The vertex–curve shadow row at the piece `t` (on `c`, the span, or
    /// the span run backwards if `rev`): the side of `q` against the span
    /// there, decided, and `sign(r̂·T)` of the span's tangent there if
    /// certified, `T` in the span's direction.
    fn row<S: RayLevel>(
        &self,
        c: &HomCurve<S, 3>,
        t: [f64; 2],
        rev: bool,
        like: &S,
        work: &mut LevelWork,
    ) -> Result<RowAt, Fail> {
        work.evals = work.evals.saturating_add(1);
        let f = |x: f64| S::from_f64(x, like);
        let ts = if t[0] == t[1] {
            f(t[0])
        } else {
            f(t[0])
                .mid(&f(t[1]))
                .widened(mul_up(add_up(t[1], -t[0]), 0.5))
        };
        let (at, tangent) = curve_point(c, &ts);
        let (tangent, ts) = if rev {
            (tangent.map(|x| x.neg()), f(1.0).sub(&ts))
        } else {
            (tangent, ts)
        };
        let rates = self
            .geom
            .span_rates(self.key.q, self.key.g, self.key.span, &ts);
        let foot = MovingAt { at, rates };
        let q = MovingAt::from_flat(&self.q, like);
        let row = curve_shadow(&q, &foot, &tangent);
        // Within the band: a tie if `decide` answers (at its target).
        let tie = row.g.beyond(self.band.tau()).is_none();
        let s = decide(row.g, row.ord, self.band, self.tie)?;
        let r = [f(R_HAT.x), f(R_HAT.y), f(R_HAT.z)];
        let along = dot3(&tangent, &r).sign().and_then(Sign::of);
        Ok(RowAt { s, along, tie })
    }
}

/// Whether the span is within the curved band (`τ + τ_sign`) of `q` in
/// both shadow directions over `[0, to]`, certified: `R·W ± g` and
/// `R'·W ± h` of one sign over it, for `g`, `h`, `W` (`[g, h, W]`), `R`,
/// `R'` the band times the 1-norms of [`NU`], [`R_HAT`] (at least their
/// lengths).
fn within<S: Scalar>([g, h, w]: [&Poly<S>; 3], to: f64, band: Band, like: &S) -> bool {
    let reach = add_up(band.tau(), band.tau_sign());
    [(g, NU), (h, R_HAT)].iter().all(|(p, v)| {
        let r = S::from_f64(mul_up(reach, v.x.abs() + v.y.abs() + v.z.abs()), like);
        [false, true].iter().all(|&minus| {
            let c = w.coefs().iter().zip(p.coefs()).map(|(w, p)| {
                let rw = w.mul(&r);
                if minus { rw.sub(p) } else { rw.add(p) }
            });
            Poly::new(c).and_then(|b| piece_sign(&b, [0.0, to], like)) == Some(Sign::Pos)
        })
    })
}

/// Whether `q` is within the curved band (`τ + τ_sign`) of the point `k`
/// in both shadow directions, or not certainly beyond it: `|ν·(k − q)|`
/// and `|r̂·(k − q)|` against the band times each axis's 1-norm (at least
/// its length), at `f64` with certified errors.
fn near_end(q: glam::DVec3, k: glam::DVec3, band: Band) -> bool {
    let reach = add_up(band.tau(), band.tau_sign());
    let b = |x: f64| Bounded::<f64>::from(x);
    let d = [b(k.x) - b(q.x), b(k.y) - b(q.y), b(k.z) - b(q.z)];
    [NU, R_HAT].iter().all(|v| {
        let dot = b(v.x) * d[0] + b(v.y) * d[1] + b(v.z) * d[2];
        let norm = v.x.abs() + v.y.abs() + v.z.abs();
        dot.beyond(mul_up(reach, norm)).is_none()
    })
}

/// The certified sign of `p` over `[lo, hi] ⊆ [0, 1]`, if one: at the
/// point for `lo = hi`, else by the coefficient hull of its piece on
/// `[s·hi, hi] ⊇ [lo, hi]`, `s ≤ lo/hi` (split at exact parameters).
fn piece_sign<S: Scalar>(p: &Poly<S>, [lo, hi]: [f64; 2], like: &S) -> Option<Sign> {
    let f = |x: f64| S::from_f64(x, like);
    let sign = if lo == hi {
        p.eval(&f(lo)).sign()
    } else {
        let mut piece = p.clone();
        if hi < 1.0 {
            piece = piece.split_at(&f(hi))[0].clone();
        }
        if lo > 0.0 {
            let s = down(lo / hi);
            if s > 0.0 {
                piece = piece.split_at(&f(s))[1].clone();
            }
        }
        let hull = piece.hull();
        if hull.lo > 0.0 {
            Some(std::cmp::Ordering::Greater)
        } else if hull.hi < 0.0 {
            Some(std::cmp::Ordering::Less)
        } else {
            None
        }
    };
    sign.and_then(Sign::of)
}

/// The certified sign of `p` over `[lo, hi]` by [`piece_sign`], or by
/// the two halves' agreeing, halving at most `depth` times.
fn split_sign<S: Scalar>(
    p: &Poly<S>,
    [lo, hi]: [f64; 2],
    like: &S,
    work: &mut LevelWork,
    depth: u32,
) -> Option<Sign> {
    work.evals = work.evals.saturating_add(1);
    if let Some(s) = piece_sign(p, [lo, hi], like) {
        return Some(s);
    }
    let m = lo + (hi - lo) * 0.5;
    if depth == 0 || !(lo < m && m < hi) {
        return None;
    }
    let l = split_sign(p, [lo, m], like, work, depth - 1)?;
    let r = split_sign(p, [m, hi], like, work, depth - 1)?;
    (l == r).then_some(l)
}

/// A cluster `[0, b]` of `g` holding at most one root, `g` certified of
/// sign `e` at `b`, narrowed towards 0 by halving `b` while `g` there is
/// certified of sign `e`. Gives the narrowed `b`, and, where `g` at
/// `m = b/2` is certified of the other sign `x`, `(m, x)`: the root is
/// then in `(m, b)`, and `g` is of sign `x` on `[0, m]`.
fn narrow<S: Scalar>(
    g: &Poly<S>,
    mut b: f64,
    e: Sign,
    like: &S,
    work: &mut LevelWork,
) -> (f64, Option<(f64, Sign)>) {
    loop {
        let m = b * 0.5;
        if m == 0.0 {
            return (b, None);
        }
        work.evals = work.evals.saturating_add(1);
        match g.eval(&S::from_f64(m, like)).sign().and_then(Sign::of) {
            Some(x) if x == e => b = m,
            Some(x) => return (b, Some((m, x))),
            None => return (b, None),
        }
    }
}

/// The pieces at a record end from [`narrow`]'s answer, in order away
/// from that end, on the span, or at `t = 1` on the span run backwards
/// (`flip`, each piece's sides and record then put in the span's order):
/// with no root split off, the piece `[0, b]` from the record's side
/// `rec.0` to `e`; else the end itself crossed from `rec.0` to `x` where
/// they differ (the end on the ray line), and the root on `[m, b]` from
/// `x` to `e`.
fn end_pieces(
    out: &mut impl Extend<Piece>,
    b: f64,
    split: Option<(f64, Sign)>,
    (side, on): (Sign, bool),
    e: Sign,
    flip: bool,
) {
    let piece = |t: [f64; 2], sides: [Sign; 2], rec: [Option<bool>; 2]| {
        let (sides, rec) = if flip {
            ([sides[1], sides[0]], [rec[1], rec[0]])
        } else {
            (sides, rec)
        };
        Piece {
            t,
            rev: flip,
            sides,
            rec,
            one: true,
        }
    };
    match split {
        None => out.extend([piece([0.0, b], [side, e], [Some(on), None])]),
        Some((m, x)) => {
            if x != side {
                out.extend([piece([0.0, 0.0], [side, x], [Some(on), None])]);
            }
            out.extend([piece([m, b], [x, e], [None, None])]);
        }
    }
}

/// `[a, b]` on `h[0]`'s parameter narrowed to the root of `h` in it, and
/// whether it is now on the other parameter (`h[1]`, `h[0]` reversed):
/// by [`bracket`] where `h` is certified of opposite signs at `a` and
/// `b`; where it is certified at one only and the other is an end of the
/// span, by [`narrow`] towards that end on the parameter that starts
/// there, then [`bracket`]; else `[a, b]`.
fn foot<S: Scalar>(
    h: [&Poly<S>; 2],
    [a, b]: [f64; 2],
    like: &S,
    work: &mut LevelWork,
) -> ([f64; 2], bool) {
    if a >= b {
        return ([a, b], false);
    }
    work.evals = work.evals.saturating_add(2);
    let at = |x: f64| h[0].eval(&S::from_f64(x, like)).sign().and_then(Sign::of);
    let near_zero =
        |p: &Poly<S>, b: f64, e: Sign, work: &mut LevelWork| match narrow(p, b, e, like, work) {
            (b, None) => [0.0, b],
            (b, Some((m, x))) => bracket(p, [m, b], x, like, work),
        };
    match (at(a), at(b)) {
        (Some(x), Some(y)) if x != y => (bracket(h[0], [a, b], x, like, work), false),
        (None, Some(y)) if a == 0.0 => (near_zero(h[0], b, y, work), false),
        (Some(x), None) if b == 1.0 => (near_zero(h[1], one_minus(a, true), x, work), true),
        (Some(x), Some(_)) => {
            // Of one sign at both ends: the first of evenly spaced points
            // where `h` isn't of that sign, from the last one where it is
            // to the next one certified.
            let n = 1u32 << SETTLE_SPLITS;
            let point = |k: u32| a + (b - a) * (f64::from(k) / f64::from(n));
            let mut from = a;
            for k in 1..n {
                let m = point(k);
                work.evals = work.evals.saturating_add(1);
                match at(m) {
                    Some(z) if z == x => from = m,
                    Some(_) => return (bracket(h[0], [from, m], x, like, work), false),
                    None => {
                        let to = (k + 1..n)
                            .map(point)
                            .find(|&m| {
                                work.evals = work.evals.saturating_add(1);
                                at(m).is_some()
                            })
                            .unwrap_or(b);
                        return ([from, to], false);
                    }
                }
            }
            ([a, b], false)
        }
        _ => ([a, b], false),
    }
}

/// `[a, b]` holding the one root of `g` there, `g` of sign `start` at
/// `a` and the other at `b`, narrowed: each step evaluates `g` at the
/// points `3/8` and `5/8` of the way and keeps the part between those
/// whose sign is certified, until neither is (the root is then within
/// the level's error of both) or the interval stops shrinking.
fn bracket<S: Scalar>(
    g: &Poly<S>,
    [mut a, mut b]: [f64; 2],
    start: Sign,
    like: &S,
    work: &mut LevelWork,
) -> [f64; 2] {
    loop {
        let w = b - a;
        let mut moved = false;
        for f in [0.375, 0.625] {
            let m = a + w * f;
            if !(a < m && m < b) {
                continue;
            }
            work.evals = work.evals.saturating_add(1);
            match g.eval(&S::from_f64(m, like)).sign().and_then(Sign::of) {
                Some(x) if x == start => {
                    a = m;
                    moved = true;
                }
                Some(_) => {
                    b = m;
                    moved = true;
                    break;
                }
                None => {}
            }
        }
        if !moved {
            return [a, b];
        }
    }
}

/// `[a, b]` on the span run backwards as bounds on the span, or the other
/// way, `[1 − b, 1 − a]` rounded outwards.
fn mirrored([a, b]: [f64; 2]) -> [f64; 2] {
    [one_minus(b, false), one_minus(a, true)]
}

/// The dyadic point `1 − d`, exactly.
fn mirror(d: Dyadic) -> Dyadic {
    // `d ≤ 1`, so `2^exp − num` is in range; `exp ≤ 63`.
    Dyadic::new((1u64 << d.exp()) - d.num(), d.exp()).expect("in [0, 1]")
}

/// `1 − x` for `x ∈ [0, 1]`, rounded up or down, in `[0, 1]`: `fl(1 − x)`
/// is exact iff `1 − fl(1 − x)` gives `x` back.
fn one_minus(x: f64, up: bool) -> f64 {
    let r = 1.0 - x;
    if 1.0 - r == x {
        r
    } else if up {
        let u = r.next_up();
        if u > 1.0 { 1.0 } else { u }
    } else {
        let d = r.next_down();
        if d < 0.0 { 0.0 } else { d }
    }
}

/// The product of two signs.
fn times(a: Sign, b: Sign) -> Sign {
    if a == b { Sign::Pos } else { Sign::Neg }
}

fn dot3<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> S {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

#[cfg(test)]
mod tests;
