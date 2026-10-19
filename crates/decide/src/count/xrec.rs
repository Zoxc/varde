//! Box degrees of an edge pair's square: the boxes holding the crossings
//! that the records' four terms ([`x_count`](crate::records::xrec::x_count)) count in total.
//!
//! **Spans and boxes** ([`XBox`]). A curved edge is a chain of spans
//! ([`Span`](crate::records::ray::Span)); the square is cut into one cell per pair of spans, each
//! with its own `[0, 1]²` in the two spans' parameters, and a box is a
//! cell cut to two dyadic extents. A straight edge is one span. The
//! sides between cells are inner sides like any other.
//!
//! **Box degrees** ([`box_degree`]). A box's degree is the laps of its
//! four sides, counter-clockwise (`t` increasing along the bottom, `s`
//! along the right side), about one ray from 0:
//!
//! - **outer sides** (on the square's boundary) are parts of the four
//!   terms: the crossings of the term's record whose parameters lie in
//!   the side's part of the edge. A curved edge's
//!   crossings carry their span and parameter enclosure; a straight
//!   edge's single crossing is placed at its parameter on the stored
//!   positions, `ν·(q − p₀)/ν·(p₁ − p₀)`, enclosed. The records count
//!   against `+r̂`, so a box with an outer side is counted against that
//!   ray;
//! - **inner sides** are angle increments of `D` with one parameter held
//!   at a dyadic value: at `t = t₀`, `D = C_e(t₀) − C_g(s)` turns as
//!   `C_g` does about the point `C_e(t₀)` against the opposite ray, at
//!   `s = s₀` as `C_e` does about `C_g(s₀)` against the same ray, each by
//!   [`curve_turn`](crate::count::degree::curve_turn) with the held point known by enclosures (exact where
//!   it is a span's end control point). A box with no outer side tries
//!   every ray in [`Ray::ALL`]'s order.
//!
//! **Sides the level can't certify** (the records'). Where that doesn't
//! settle (an inner side through 0 or too near it, an end on the ray's
//! line ahead of 0, a record's crossing whose enclosure reaches over a
//! box corner), every side is counted against `+r̂` with the side
//! records of the square ([`XSides`]): an inner side not certified off 0
//! reads the record of its held point against the other edge, the `(e, g,
//! dyadic side)` record, over its part; an end on the ray's line takes
//! its side from its corner's record, the corner of the two edges' points
//! there (knot against knot where two knots' shadows coincide); a
//! crossing of an outer or inner side's record whose enclosure holds a
//! box corner is ordered against it by that corner's record. All of them
//! count the one moved configuration, so the degree is the box's winding
//! in it, as the four terms are. A cut inside a record's tangency with
//! the ray's line (two moved crossings that cancel in the record's count,
//! [`SpanRec::touches`](crate::records::ray::SpanRec::touches)) holds one
//! of them, which no record orders: [`XDegreeError::Unsettled`]. A record
//! not yet decided is asked for ([`XDegreeError::Need`]), one key at a
//! time.
//!
//! **`Σ` degrees `= X` by construction.** Every certified turn is the
//! true crossing count of its side, and the certificates (sides off 0,
//! ends off the ray's line ahead of 0) hold for the moved data too, so
//! each box's degree is the winding of `D` round it in the moved
//! configuration, where the outer sides read the records; the inner
//! sides of any partition cancel (the boxes on the two sides of a line
//! read the one record of its held point, and the one record of each
//! corner), and the degrees of a partition of the square sum to the
//! winding of its boundary: `X`. Counted per box, the records' crossings
//! are split between the boxes by their parameters, each counted once.
//!
//! **Clusters** ([`clusters`]). The cells are refined into the record's
//! clusters ([`XRec`]): a box whose two pieces' shadows have certified
//! disjoint hulls holds no crossing and is dropped. A box is a **leaf**
//! once its z-order is clear (the pieces' heights along `ê` certified
//! apart over it: its crossings, whatever their number, all read that
//! one sign) or its shadow tangents are certified transversal over it
//! (the cone of each piece's control point differences, which holds
//! every tangent of a rational span with positive weights, of one strict
//! sign of their cross product against the other's: the two shadows
//! cross at most once in it). A leaf of non-zero degree is a cluster, of
//! that degree, its z-order clear or read from its height record (a
//! transversal leaf's degree is then `±1`); a leaf of degree 0 holds no
//! crossing that counts and is dropped. Every other box splits, whatever
//! its size and degree: no band stops the refinement, since a small box
//! of degree 0 may hold a `±1` pair of crossings of opposite z-orders,
//! which counts `±1`, not 0. Only a leaf's degree is read: where the
//! level doesn't certify its sides it reads the side records, and a leaf
//! the records leave unsettled (a cut inside a record's tangency with the
//! ray's line) splits again. The search goes on past the leaves whose
//! records aren't decided yet and asks for all of them at once
//! ([`XClusterError::Need`]). A split cuts the larger piece's parameter
//! (in 3D, by the hulls' summed widths; `e`'s on a tie), at the middle or
//! the first shifted position (the face boxes' snapped positions) whose
//! new corners are certified off the `+r̂` ray from 0 (no cut is made at
//! a tangency of a record with the ray's line, which no record orders),
//! else the other parameter likewise, else at the middle; an unsettled
//! leaf's split takes the first such position whose two halves both have
//! degrees without the side records where one does. Where the shadows
//! are tangent or overlap, no box there ever
//! becomes a leaf: the search escalates at the depth limit
//! ([`XClusterError::Depth`]) or past [`MAX_BOXES`] boxes
//! ([`XClusterError::Boxes`]).
//!
//! **Two straight edges** hold at most one crossing, so their whole cell
//! is the one cluster where `X ≠ 0`. Where their shadows are exactly
//! parallel (decided on the stored points; with `X ≠ 0` they are then
//! collinear, the crossing the four terms' ties put at an end), the
//! cluster is at the end whose term counted the crossing ([`tie_point`])
//! and reads the point–curve row there ([`ZRow::PointCurve`]), valid only
//! where the z-order is one sign over the whole overlap of the two
//! shadows: decided exactly at the overlap's two ends (the heights are
//! linear along it), the cluster's z-order is then clear; else the pair
//! is refused ([`XClusterError::Collinear`]: the edges meet in 3D, or
//! cross in height, over the overlap). A pair on one curve is never
//! searched ([`XClusterError::Shared`]): its clusters come from the
//! records alone ([`shared_clusters`](crate::records::xrec::shared_clusters)).
//!
//! **Levels.** The clusters are listed in their boxes' key order, so each
//! cluster's index is a function of the pair and the precision level the
//! record was formed at, which the record stores ([`XRec::level`]): it is
//! formed at the first level that forms it ([`form`]), and checked again
//! at that level only ([`replay`]); a reader of the record never forms it
//! at another.
//!
//! Generic over the level, as the face boxes are; each call reports the
//! pieces it tested (hulls, tangent cones and sides, settled or not), for
//! the pass to charge.

use std::cmp::Ordering;

use malachite::Rational;

use crate::bezier::{Dir, Dyadic, HomCurve, Scalar};
use crate::count::boxes::{Extent, QShadow, SPLIT_SIXTEENTHS, dy};
use crate::count::degree::{Ray, TurnError, pieces_turn};
use crate::count::s02::MAX_LEAVES;
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::Sign;
use crate::order::EntityOrder;
use crate::perturb::{NU, R_HAT, UP};
use crate::records::keys::{EdgeAt, Operand, XKey, XSideKey, XTieKey};
use crate::records::shadow::{LEVELS, SideAhead};
use crate::records::side::Over;
use crate::records::xrec::{
    EdgeRays, XCluster, XCurve, XPair, XPart, XRec, XSides, XWant, ZRow, from_held, line_param,
    tie_point,
};

/// The most boxes one search of an edge pair's clusters tests at a level
/// ([`XClusterError::Boxes`]).
pub const MAX_BOXES: usize = 1 << 16;

/// The most boxes of one search of an edge pair's clusters that read the
/// side records ([`XClusterError::Sided`]): a knot's or a crossing's
/// neighbourhood takes a few at each depth; shadows overlapping along a
/// stretch would take one per box along it.
pub const MAX_SIDED: usize = 64;

/// A box of the square of an edge pair: the cell of span `cell[0]` of `e`
/// and span `cell[1]` of `g`, cut to the extents `t` (of `e`'s span) and
/// `s` (of `g`'s). Its value is its key, ordered by cell, then `t`, `s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XBox {
    /// The spans of `e` and of `g`.
    pub cell: [u32; 2],
    /// The extent in `e`'s span parameter.
    pub t: Extent,
    /// The extent in `g`'s span parameter.
    pub s: Extent,
}

impl XBox {
    /// The whole cells of a pair whose edges have `spans` spans, `e`'s
    /// span first in the order.
    pub fn cells(spans: [u32; 2]) -> impl Iterator<Item = XBox> {
        (0..spans[0]).flat_map(move |i| {
            (0..spans[1]).map(move |j| XBox {
                cell: [i, j],
                t: Extent::WHOLE,
                s: Extent::WHOLE,
            })
        })
    }

    /// The box cut at `at` in `e`'s parameter (`Dir::U`) or `g`'s
    /// (`Dir::V`), the lower first; `None` unless `at` lies strictly
    /// inside that extent with the box depth's limit kept.
    pub fn split_at(&self, dir: Dir, at: Dyadic) -> Option<[XBox; 2]> {
        let ext = match dir {
            Dir::U => self.t,
            Dir::V => self.s,
        };
        let lo = Extent::new(ext.lo(), at)?;
        let hi = Extent::new(at, ext.hi())?;
        let with = |x: Extent| match dir {
            Dir::U => XBox { t: x, ..*self },
            Dir::V => XBox { s: x, ..*self },
        };
        Some([with(lo), with(hi)])
    }

    /// The corner at `e`'s parameter `t` and `g`'s `s`, ends of the box's
    /// extents: the two edges' dyadic points there; `None` past the range
    /// of span indices.
    fn corner(&self, t: Dyadic, s: Dyadic) -> Option<[EdgeAt; 2]> {
        Some([EdgeAt::new(self.cell[0], t)?, EdgeAt::new(self.cell[1], s)?])
    }
}

/// A box's degree and what it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XDegree {
    /// The signed crossings inside the box.
    pub degree: i64,
    /// The ray it was counted against.
    pub ray: Ray,
    /// The curve pieces tested, including those of attempts that didn't
    /// settle.
    pub pieces: u64,
}

/// Why a box's degree isn't settled.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum XDegreeError {
    /// The degree reads this side or corner record, not yet decided: fill
    /// it and ask again.
    Need(Box<XWant>),
    /// The sides don't settle at this level even with the side records
    /// (a tangency's pair of crossings over a box corner, an inner side's
    /// pieces contradicting each other): escalate the level.
    Unsettled,
    /// The pair's data doesn't fit the box: no such span, a record of the
    /// wrong kind for its edge, a weight not certified positive, a key
    /// past the range of span indices.
    Shape,
}

/// Why a box's degree isn't certified without the side records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plain {
    /// An inner side isn't certified off 0, or a ray stopped at an end on
    /// its line, or a record's crossing reaches over a box corner.
    Records,
    /// The pair's data doesn't fit the box.
    Shape,
}

/// The sides of a box in counter-clockwise order, with whether each is
/// on the square's boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XSide {
    /// `s = s.lo`, `t` increasing.
    Bottom,
    /// `t = t.hi`, `s` increasing.
    Right,
    /// `s = s.hi`, `t` decreasing.
    Top,
    /// `t = t.lo`, `s` decreasing.
    Left,
}

impl XSide {
    const CCW: [XSide; 4] = [XSide::Bottom, XSide::Right, XSide::Top, XSide::Left];
}

/// A side of a box as the records read it: the edge whose point is held
/// (its operand), the held point, the part of the other edge it runs
/// over (span, extent), the sign of its laps counter-clockwise round the
/// box against the part's direction, and its two corners in the part's
/// increasing direction.
struct SideLine {
    held: Operand,
    at: EdgeAt,
    span: u32,
    part: Extent,
    sign: i64,
    corners: [[EdgeAt; 2]; 2],
}

impl SideLine {
    /// Side `side` of `b`, `spans` the pair's span counts; `None` past the
    /// range of span indices.
    fn of(b: &XBox, side: XSide) -> Option<SideLine> {
        let (t, s) = (b.t, b.s);
        let along_t = |at: Dyadic, sign| -> Option<SideLine> {
            Some(SideLine {
                held: Operand::B,
                at: EdgeAt::new(b.cell[1], at)?,
                span: b.cell[0],
                part: t,
                sign,
                corners: [b.corner(t.lo(), at)?, b.corner(t.hi(), at)?],
            })
        };
        let along_s = |at: Dyadic, sign| -> Option<SideLine> {
            Some(SideLine {
                held: Operand::A,
                at: EdgeAt::new(b.cell[0], at)?,
                span: b.cell[1],
                part: s,
                sign,
                corners: [b.corner(at, s.lo())?, b.corner(at, s.hi())?],
            })
        };
        match side {
            XSide::Bottom => along_t(s.lo(), 1),
            XSide::Top => along_t(s.hi(), -1),
            XSide::Right => along_s(t.hi(), 1),
            XSide::Left => along_s(t.lo(), -1),
        }
    }
}

/// Whether `b`'s `side` lies on the square's boundary of a pair with
/// `spans` spans.
fn outer(b: &XBox, side: XSide, spans: [u32; 2]) -> bool {
    match side {
        XSide::Bottom => b.cell[1] == 0 && b.s.lo() == Dyadic::ZERO,
        XSide::Top => b.cell[1] + 1 == spans[1] && b.s.hi() == Dyadic::ONE,
        XSide::Left => b.cell[0] == 0 && b.t.lo() == Dyadic::ZERO,
        XSide::Right => b.cell[0] + 1 == spans[0] && b.t.hi() == Dyadic::ONE,
    }
}

/// The degree of box `b` of the pair `x` (`pair` its curves and terms) at
/// `like`'s level (module docs): certified where the level settles it,
/// else every side against `+r̂` reading `sides`.
pub fn box_degree<S: Scalar>(
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    b: &XBox,
    like: &S,
) -> Result<XDegree, XDegreeError> {
    let mut pieces = 0u64;
    let r = degree_of(x, pair, sides, b, like, &mut pieces);
    r.map(|(degree, ray)| XDegree {
        degree,
        ray,
        pieces,
    })
}

/// [`box_degree`]'s degree and ray, adding the pieces tested to `pieces`.
fn degree_of<S: Scalar>(
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    b: &XBox,
    like: &S,
    pieces: &mut u64,
) -> Result<(i64, Ray), XDegreeError> {
    match plain_degree(pair, b, like, pieces) {
        Ok(d) => Ok(d),
        Err(Plain::Shape) => Err(XDegreeError::Shape),
        Err(Plain::Records) => {
            sided_degree(x, pair, sides, b, like, pieces, &mut Vec::new()).map(|d| (d, Ray::PLUS_R))
        }
    }
}

/// The degree of `b` from certified turns and the four terms alone
/// (module docs), and the ray it was counted against.
fn plain_degree<S: Scalar>(
    pair: &XPair<'_>,
    b: &XBox,
    like: &S,
    pieces: &mut u64,
) -> Result<(i64, Ray), Plain> {
    let spans = [pair.e.span_count(), pair.g.span_count()];
    if b.cell[0] >= spans[0] || b.cell[1] >= spans[1] {
        return Err(Plain::Shape);
    }
    let any_outer = XSide::CCW.iter().any(|&sd| outer(b, sd, spans));
    let rays: &[Ray] = if any_outer { &[Ray::PLUS_R] } else { &Ray::ALL };
    let ce = pair.e.span(b.cell[0], like).ok_or(Plain::Shape)?;
    let cg = pair.g.span(b.cell[1], like).ok_or(Plain::Shape)?;
    let pe = cut(&ce, b.t, like);
    let pg = cut(&cg, b.s, like);
    // The held point of each inner side, which doesn't depend on the ray.
    let mut held = [None; 4];
    for (i, side) in XSide::CCW.into_iter().enumerate() {
        if !outer(b, side, spans) {
            held[i] = Some(held_point(pair, b, side, like).ok_or(Plain::Shape)?);
        }
    }
    for &ray in rays {
        let mut degree = 0i64;
        let mut stopped = false;
        for (side, q) in XSide::CCW.into_iter().zip(&held) {
            let laps = match q {
                None => match side_count(pair, b, side) {
                    Ok(n) => n,
                    Err(XDegreeError::Shape) => return Err(Plain::Shape),
                    // A crossing over a box corner: the corner's record.
                    Err(_) => return Err(Plain::Records),
                },
                Some(q) => match inner_laps(side, q, &pe, &pg, ray, [None; 2], pieces) {
                    Ok(l) => l,
                    // Off an axis ray's line and the line across it is
                    // off 0, whatever the ray: the side's record.
                    Err(TurnError::Off) if ray.is_axis() => return Err(Plain::Records),
                    Err(TurnError::Off | TurnError::End | TurnError::Unsettled) => {
                        stopped = true;
                        break;
                    }
                },
            };
            degree += laps;
        }
        if !stopped {
            return Ok((degree, ray));
        }
    }
    Err(Plain::Records)
}

/// The degree of `b` with every side against `+r̂` reading the side and
/// corner records where the level doesn't certify it (module docs). Every
/// record the box misses is added to `wants`, and the box's degree is
/// then [`XDegreeError::Need`] of one of them.
#[allow(clippy::too_many_arguments)]
fn sided_degree<S: Scalar>(
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    b: &XBox,
    like: &S,
    pieces: &mut u64,
    wants: &mut Vec<XWant>,
) -> Result<i64, XDegreeError> {
    let spans = [pair.e.span_count(), pair.g.span_count()];
    if b.cell[0] >= spans[0] || b.cell[1] >= spans[1] {
        return Err(XDegreeError::Shape);
    }
    let ce = pair.e.span(b.cell[0], like).ok_or(XDegreeError::Shape)?;
    let cg = pair.g.span(b.cell[1], like).ok_or(XDegreeError::Shape)?;
    let (pe, pg) = (cut(&ce, b.t, like), cut(&cg, b.s, like));
    let mut degree = 0i64;
    let mut missing = None;
    for side in XSide::CCW {
        let line = SideLine::of(b, side).ok_or(XDegreeError::Shape)?;
        let n = if outer(b, side, spans) {
            outer_sided(x, pair, sides, side, &line)
        } else {
            inner_sided(
                x,
                sides,
                b,
                side,
                &line,
                (pair, &pe, &pg),
                like,
                pieces,
                wants,
            )
        };
        match n {
            Ok(n) => degree += n * line.sign,
            Err(XDegreeError::Need(k)) => {
                wants.push(*k);
                missing = Some(*k);
            }
            Err(e) => return Err(e),
        }
    }
    match missing {
        Some(k) => Err(XDegreeError::Need(Box::new(k))),
        None => Ok(degree),
    }
}

/// The corner record of the two edges' points `c`, oriented from `e`'s
/// point to `g`'s.
fn tie(x: XKey, sides: &XSides, c: [EdgeAt; 2]) -> Result<SideAhead, XDegreeError> {
    let k = XTieKey {
        x,
        e: c[0],
        g: c[1],
    };
    sides
        .tie(&k)
        .ok_or_else(|| XDegreeError::Need(Box::new(XWant::Tie(k))))
}

/// An outer side's signed crossings along its part's increasing
/// parameter, from its term's record, a crossing over a box corner
/// ordered by the corner's record.
fn outer_sided(
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    side: XSide,
    line: &SideLine,
) -> Result<i64, XDegreeError> {
    let (rays, edge, q) = term(pair, side);
    let q = q.ok_or(XDegreeError::Shape)?;
    let (crossings, touches) = term_crossings(rays, edge, q, line.span)?;
    over(&crossings, &touches, line.held, line.part, |j| {
        Ok(from_held(line.held, tie(x, sides, line.corners[j])?).side)
    })
}

/// An inner side's signed crossings along its part's increasing
/// parameter: certified against `+r̂` where the level settles it (an end
/// on the ray's line taking its side from its corner's record), else
/// from its held point's side record, a crossing over a box corner
/// ordered by the corner's record. Both corners an end on the ray's
/// line misses are added to `wants`.
#[allow(clippy::too_many_arguments)]
fn inner_sided<S: Scalar>(
    x: XKey,
    sides: &XSides,
    b: &XBox,
    side: XSide,
    line: &SideLine,
    (pair, pe, pg): (&XPair<'_>, &HomCurve<S, 3>, &HomCurve<S, 3>),
    like: &S,
    pieces: &mut u64,
    wants: &mut Vec<XWant>,
) -> Result<i64, XDegreeError> {
    let q = held_point(pair, b, side, like).ok_or(XDegreeError::Shape)?;
    let mut turn = |given| inner_laps(side, &q, pe, pg, Ray::PLUS_R, given, pieces);
    let mut t = turn([None; 2]);
    if let Err(TurnError::End) = t {
        // A corner on the ray's line ahead of 0: its side from its record.
        // Left of the ray is the corner's `D` to `+ν`: `g`'s point to the
        // right of `e`'s `r̂` line.
        let mut given = [None; 2];
        let mut need = None;
        for (j, c) in line.corners.iter().enumerate() {
            match tie(x, sides, *c) {
                Ok(r) => given[j] = Some(-r.side.as_i8()),
                Err(XDegreeError::Need(k)) => {
                    if let Some(k0) = need.replace(*k) {
                        wants.push(k0);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        if let Some(k) = need {
            return Err(XDegreeError::Need(Box::new(k)));
        }
        t = turn(given);
    }
    match t {
        // `inner_laps` turns the side counter-clockwise: back to the
        // part's direction.
        Ok(laps) => Ok(laps * line.sign),
        Err(TurnError::Off) => {
            let k = XSideKey {
                x,
                held: line.held,
                at: line.at,
            };
            let rec = sides
                .side(&k)
                .ok_or_else(|| XDegreeError::Need(Box::new(XWant::Side(k))))?;
            let crossings: Vec<_> = rec
                .crossings
                .iter()
                .filter(|c| c.span == line.span)
                .map(|c| (c.t, c.sign, c.single))
                .collect();
            let touches: Vec<_> = rec
                .touches
                .iter()
                .filter(|t| t.0 == line.span)
                .map(|t| t.1)
                .collect();
            over(&crossings, &touches, line.held, line.part, |j| {
                Ok(from_held(line.held, tie(x, sides, line.corners[j])?).side)
            })
        }
        Err(TurnError::End | TurnError::Unsettled) => Err(XDegreeError::Unsettled),
    }
}

/// The count of `crossings` (of one span: `(t, sign, single)`) over
/// `part`, for the ray of a point of `held`, a crossing over a cut
/// ordered by `cut_side(j)` at the part's end `j`; a cut inside one of
/// the span's `touches` isn't ordered ([`XDegreeError::Unsettled`]).
fn over(
    crossings: &[([f64; 2], Sign, bool)],
    touches: &[[f64; 2]],
    held: Operand,
    part: Extent,
    cut_side: impl Fn(usize) -> Result<Sign, XDegreeError>,
) -> Result<i64, XDegreeError> {
    let x = [dy(part.lo()), dy(part.hi())];
    let mut sides = [None, None];
    loop {
        match crate::records::side::count_part(crossings.iter().copied(), touches, held, x, sides) {
            Ok(n) => return Ok(n),
            Err(Over::Side(j)) => sides[j] = Some(cut_side(j)?),
            Err(Over::Unordered) => return Err(XDegreeError::Unsettled),
        }
    }
}

/// The term an outer side reads: its record, the edge it counts along
/// and the asking point (an end of the other edge).
fn term<'a>(pair: &XPair<'a>, side: XSide) -> (EdgeRays<'a>, XCurve<'a>, Option<[f64; 3]>) {
    let t = &pair.terms;
    match side {
        XSide::Bottom => (t.g0_e, pair.e, pair.g.end(false)),
        XSide::Right => (t.b_g, pair.g, pair.e.end(true)),
        XSide::Top => (t.g1_e, pair.e, pair.g.end(true)),
        XSide::Left => (t.a_g, pair.g, pair.e.end(false)),
    }
}

/// The crossings of a term on span `span` of its edge, a crossing
/// `(t, sign, single)` (a straight edge's one at its parameter enclosed,
/// `[0, 1]` where the edge's shadow runs along the ray's line, then
/// ordered against any cut by the corners' records alone), and the
/// span's tangencies whose crossings cancel.
type Crossings = (Vec<([f64; 2], Sign, bool)>, Vec<[f64; 2]>);

/// The term's [`Crossings`] on span `span` of its edge.
fn term_crossings(
    rays: EdgeRays<'_>,
    edge: XCurve<'_>,
    q: [f64; 3],
    span: u32,
) -> Result<Crossings, XDegreeError> {
    Ok(match (rays, edge) {
        (EdgeRays::Line(r), XCurve::Line(p0, p1)) => (
            r.crossing
                .map(|c| (line_param(q, p0, p1).unwrap_or([0.0, 1.0]), c, true))
                .into_iter()
                .collect(),
            Vec::new(),
        ),
        (EdgeRays::Chain(r), XCurve::Spans(_)) => (
            r.crossings
                .iter()
                .filter(|c| c.span == span)
                .map(|c| (c.crossing.t, c.crossing.sign, c.crossing.single))
                .collect(),
            r.touches
                .iter()
                .filter(|t| t.0 == span)
                .map(|t| t.1)
                .collect(),
        ),
        _ => return Err(XDegreeError::Shape),
    })
}

/// The laps of an inner side about `ray`, counter-clockwise round the
/// box, `q` its held point; `pe`, `pg` the box's pieces of the two spans;
/// `given` the sides (`+1` left of the ray) of the turned curve's start
/// and end where the level doesn't settle them.
fn inner_laps<S: Scalar>(
    side: XSide,
    q: &QShadow,
    pe: &HomCurve<S, 3>,
    pg: &HomCurve<S, 3>,
    ray: Ray,
    given: [Option<i8>; 2],
    pieces: &mut u64,
) -> Result<i64, TurnError> {
    let opposite = Ray::new(-ray.dir()[0], -ray.dir()[1]).expect("a ray's opposite");
    let turn = |c: &HomCurve<S, 3>, ray, pieces: &mut u64| {
        if !c.weights_positive() {
            return Err(TurnError::Off);
        }
        pieces_turn(std::slice::from_ref(c), q, ray, given, pieces)
    };
    Ok(match side {
        // `D = C_e(t) − C_g(s₀)`: `C_e` about `C_g(s₀)`, same ray.
        XSide::Bottom => turn(pe, ray, pieces)?.laps,
        XSide::Top => -turn(pe, ray, pieces)?.laps,
        // `D = C_e(t₀) − C_g(s)`: `C_g` about `C_e(t₀)`, opposite ray.
        XSide::Right => turn(pg, opposite, pieces)?.laps,
        XSide::Left => -turn(pg, opposite, pieces)?.laps,
    })
}

/// The held point of `b`'s inner `side`: the shadow of `g` at `s₀` for
/// the bottom and top, of `e` at `t₀` for the right and left; `None` if
/// it can't be enclosed (no such span, a weight not certified non-zero).
fn held_point<S: Scalar>(pair: &XPair<'_>, b: &XBox, side: XSide, like: &S) -> Option<QShadow> {
    let (c, k, at) = match side {
        XSide::Bottom => (&pair.g, b.cell[1], b.s.lo()),
        XSide::Top => (&pair.g, b.cell[1], b.s.hi()),
        XSide::Right => (&pair.e, b.cell[0], b.t.hi()),
        XSide::Left => (&pair.e, b.cell[0], b.t.lo()),
    };
    point_shadow(c, k, at, like)
}

/// The shadow of span `k` of `c` at `at`, enclosed (exact where it is the
/// span's end control point); `None` if it can't be.
fn point_shadow<S: Scalar>(c: &XCurve<'_>, k: u32, at: Dyadic, like: &S) -> Option<QShadow> {
    if let Some(p) = c.span_end(k, at) {
        return QShadow::of(p);
    }
    let span = c.span(k, like)?;
    let (x, w) = span.eval_hom(&S::from_f64(dy(at), like));
    QShadow::from_dot(|a| {
        let s = x.iter().zip(a).fold(S::int(0, like), |s, (x, a)| {
            s.add(&x.mul(&S::from_f64(a, like)))
        });
        s.div(&w).enclose()
    })
}

/// Whether the corner of cell `cell` at `e`'s parameter `t` and `g`'s `s`
/// is certified off the `+r̂` ray from 0: `D = C_e(t) − C_g(s)` certified
/// off the ray's line or behind 0 along it.
fn corner_off<S: Scalar>(pair: &XPair<'_>, cell: [u32; 2], t: Dyadic, s: Dyadic, like: &S) -> bool {
    let (Some(e), Some(g)) = (
        point_shadow(&pair.e, cell[0], t, like),
        point_shadow(&pair.g, cell[1], s, like),
    ) else {
        return false;
    };
    let diff = |a: [f64; 2], b: [f64; 2]| {
        let lo = Bounded::exact(a[0]) - Bounded::exact(b[1]);
        let hi = Bounded::exact(a[1]) - Bounded::exact(b[0]);
        [lo.enclose()[0], hi.enclose()[1]]
    };
    let (r, n) = (diff(e.r, g.r), diff(e.n, g.n));
    n[0] > 0.0 || n[1] < 0.0 || r[1] < 0.0
}

/// The part of `c` over the extent `x`, cut at its exact dyadic ends.
fn cut<S: Scalar>(c: &HomCurve<S, 3>, x: Extent, like: &S) -> HomCurve<S, 3> {
    cut_part(c, [dy(x.lo()), dy(x.hi())], like)
}

/// The part of `c` over `[a, b]` of its parameter, `0 ≤ a ≤ b ≤ 1`, cut
/// at those values exactly.
pub(crate) fn cut_part<S: Scalar>(
    c: &HomCurve<S, 3>,
    [a, b]: [f64; 2],
    like: &S,
) -> HomCurve<S, 3> {
    let fa = S::from_f64(a, like);
    let c = if a > 0.0 {
        let [_, r] = c.split_at(&fa);
        r
    } else {
        c.clone()
    };
    if b < 1.0 {
        let one = S::int(1, like);
        let s = S::from_f64(b, like).sub(&fa).div(&one.sub(&fa));
        let [l, _] = c.split_at(&s);
        l
    } else {
        c
    }
}

/// The laps of an outer side of `b`, counter-clockwise round the box,
/// against `+r̂`: the crossings of its term's record over the side's part
/// of the edge (module docs); [`XDegreeError::Unsettled`] where a
/// crossing's enclosure reaches over an inner end of the part.
fn side_count(pair: &XPair<'_>, b: &XBox, side: XSide) -> Result<i64, XDegreeError> {
    let (rays, edge, q) = term(pair, side);
    let (span, ext, sign) = match side {
        XSide::Bottom => (b.cell[0], b.t, 1),
        XSide::Right => (b.cell[1], b.s, 1),
        XSide::Top => (b.cell[0], b.t, -1),
        XSide::Left => (b.cell[1], b.s, -1),
    };
    let q = q.ok_or(XDegreeError::Shape)?;
    let mut count = 0i64;
    match (rays, edge) {
        (EdgeRays::Line(r), XCurve::Line(p0, p1)) => {
            if let Some(c) = r.crossing {
                if ext != Extent::WHOLE {
                    let at = line_param(q, p0, p1).ok_or(XDegreeError::Unsettled)?;
                    if !within(at, ext)? {
                        return Ok(0);
                    }
                }
                count += i64::from(c.as_i8());
            }
        }
        (EdgeRays::Chain(r), XCurve::Spans(_)) => {
            for c in &r.crossings {
                if c.span == span && within(c.crossing.t, ext)? {
                    count += i64::from(c.crossing.sign.as_i8());
                }
            }
            // A cut inside a tangency whose crossings cancel holds one of
            // them.
            for t in r.touches.iter().filter(|t| t.0 == span) {
                within(t.1, ext)?;
            }
        }
        _ => return Err(XDegreeError::Shape),
    }
    Ok(sign * count)
}

/// Whether a crossing with the parameter enclosure `at` lies in the
/// extent: a span's end `0` or `1` belongs to it whole, an inner end must
/// be certified on one side.
fn within(at: [f64; 2], ext: Extent) -> Result<bool, XDegreeError> {
    let (lo, hi) = (dy(ext.lo()), dy(ext.hi()));
    let past_lo = if ext.lo() == Dyadic::ZERO || at[0] > lo {
        true
    } else if at[1] < lo {
        false
    } else {
        return Err(XDegreeError::Unsettled);
    };
    let before_hi = if ext.hi() == Dyadic::ONE || at[1] < hi {
        true
    } else if at[0] > hi {
        false
    } else {
        return Err(XDegreeError::Unsettled);
    };
    Ok(past_lo && before_hi)
}

/// Why an edge pair's clusters aren't formed at a level.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum XClusterError {
    /// Leaves read these side and corner records, not yet decided (every one the search met, in key order): fill them
    /// and form the clusters again.
    Need(Vec<XWant>),
    /// A box's degree isn't settled at this level, even with the side
    /// records: escalate.
    Degree(XBox, XDegreeError),
    /// A box can't split further within the depth limit: escalate (a
    /// shadow tangency, or shadows overlapping along a stretch; module
    /// docs).
    Depth(XBox),
    /// The search tested more than [`MAX_BOXES`] boxes at this level
    /// (shadows tangent or overlapping, where no box there is a leaf):
    /// escalate.
    Boxes,
    /// More than [`MAX_SIDED`] boxes needed the side records (this one
    /// the next): the shadows overlap or nearly along a stretch, where no
    /// box settles without them; escalate.
    Sided(XBox),
    /// More than [`MAX_LEAVES`] boxes at once.
    SizeLimit,
    /// Two straight edges with collinear shadows whose z-order isn't one
    /// sign over the overlap of their shadows (they meet in 3D there, or
    /// cross in height): no cluster reads it until the crossings along
    /// the overlap are found (module docs, "Two straight edges"). Exact,
    /// so no level does better.
    Collinear,
    /// The pair's edges lie on one curve: no geometric search, its
    /// clusters are the records'
    /// ([`shared_clusters`](crate::records::xrec::shared_clusters)).
    Shared,
}

impl XClusterError {
    /// Whether the next precision level may form the clusters.
    pub fn escalates(&self) -> bool {
        matches!(
            self,
            XClusterError::Degree(..)
                | XClusterError::Depth(_)
                | XClusterError::Boxes
                | XClusterError::Sided(_)
        )
    }
}

/// An edge pair's clusters and what they took.
#[derive(Clone, Debug, PartialEq)]
pub struct XClusters {
    /// The record's clusters, with the level they were formed at.
    pub rec: XRec,
    /// Each cluster's box, by the same index.
    pub boxes: Vec<XBox>,
    /// The curve pieces tested (hulls, tangent cones and box sides,
    /// settled or not).
    pub pieces: u64,
}

/// A box's degree for the clusters (module docs).
enum Found {
    /// Its degree.
    Degree(i64),
    /// The side records leave it unsettled: split it.
    Split,
    /// It reads side records not yet decided, added to the wants.
    Wants,
}

/// A leaf's degree for the clusters: certified without the side records
/// where it is, else reading them, adding those it misses to `wants`.
fn cluster_degree<S: Scalar>(
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    b: &XBox,
    like: &S,
    (pieces, read): (&mut u64, &mut usize),
    wants: &mut Vec<XWant>,
) -> Result<Found, XClusterError> {
    match plain_degree(pair, b, like, pieces) {
        Ok((d, _)) => Ok(Found::Degree(d)),
        Err(Plain::Shape) => Err(XClusterError::Degree(*b, XDegreeError::Shape)),
        Err(Plain::Records) if *read >= MAX_SIDED => Err(XClusterError::Sided(*b)),
        Err(Plain::Records) => {
            *read += 1;
            match sided_degree(x, pair, sides, b, like, pieces, wants) {
                Ok(d) => Ok(Found::Degree(d)),
                Err(XDegreeError::Need(_)) => Ok(Found::Wants),
                // A leaf whose records don't settle it (a cut inside a
                // tangency): its halves' may.
                Err(XDegreeError::Unsettled) => Ok(Found::Split),
                Err(e) => Err(XClusterError::Degree(*b, e)),
            }
        }
    }
}

/// The wants gathered, each once, in key order, as the error asking for
/// them.
fn need(mut wants: Vec<XWant>) -> XClusterError {
    wants.sort_unstable();
    wants.dedup();
    XClusterError::Need(wants)
}

/// The clusters of pair `x` (`pair` its curves and terms, `sides` the
/// square's side records) at precision level `level` ([`LEVELS`]: `f64`,
/// double-double, then the big-float widths); the record stores the
/// level.
pub fn clusters_at<O: EntityOrder>(
    order: &O,
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    level: usize,
) -> Result<XClusters, XClusterError> {
    let mut out = match level {
        0 => clusters(order, x, pair, sides, &Bounded::exact(0.0)),
        1 => clusters(order, x, pair, sides, &Bounded::exact(Dd::from_f64(0.0))),
        _ => {
            let w = Width::ALL
                .get(level - 2)
                .copied()
                .ok_or(XClusterError::SizeLimit)?;
            clusters(
                order,
                x,
                pair,
                sides,
                &Bounded::exact(Big::from_f64(0.0, w)),
            )
        }
    }?;
    out.rec.level = level;
    Ok(out)
}

/// The clusters of pair `x` at the first level that forms them
/// ([`clusters_at`] from level 0, escalating where
/// [`XClusterError::escalates`]); past the last level its error stands.
pub fn form<O: EntityOrder>(
    order: &O,
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
) -> Result<XClusters, XClusterError> {
    let mut last = XClusterError::SizeLimit;
    for level in 0..LEVELS {
        match clusters_at(order, x, pair, sides, level) {
            Ok(c) => return Ok(c),
            Err(e) if e.escalates() => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Why a stored crossing record doesn't replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XReplayError {
    /// Forming the clusters again at the record's level failed so.
    Form(XClusterError),
    /// Formed again at the record's level, the clusters differ.
    Differs,
}

/// Forms the clusters of pair `x` again at the level stored with `rec`,
/// never another (its cluster indices are that level's), and checks they
/// are `rec`'s: a check that the record is a function of its key.
pub fn replay<O: EntityOrder>(
    order: &O,
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    rec: &XRec,
) -> Result<(), XReplayError> {
    let again = clusters_at(order, x, pair, sides, rec.level).map_err(XReplayError::Form)?;
    if again.rec == *rec {
        Ok(())
    } else {
        Err(XReplayError::Differs)
    }
}

/// The clusters of pair `x` (`pair` its curves and terms, `sides` the
/// square's side records) at `like`'s level (module docs). The record's
/// level is the caller's to set ([`clusters_at`]).
pub fn clusters<S: Scalar, O: EntityOrder>(
    order: &O,
    x: XKey,
    pair: &XPair<'_>,
    sides: &XSides,
    like: &S,
) -> Result<XClusters, XClusterError> {
    if pair.shared {
        return Err(XClusterError::Shared);
    }
    let spans = [pair.e.span_count(), pair.g.span_count()];
    let mut pieces = 0u64;
    if let (XCurve::Line(a, b), XCurve::Line(g0, g1)) = (pair.e, pair.g) {
        return straight_clusters(order, x, pair, [[a, b], [g0, g1]], sides, like);
    }
    let mut work: Vec<(XBox, Option<i64>)> = XBox::cells(spans).map(|b| (b, None)).collect();
    work.reverse();
    let mut found: Vec<(XBox, XCluster)> = Vec::new();
    // The records the leaves miss: such a box is a leaf whatever its
    // degree, so the search goes on without it and asks for them all at
    // once.
    let mut wants = Vec::new();
    // The boxes that read the side records.
    let mut read = 0usize;
    let mut seen = 0usize;
    while let Some((b, degree)) = work.pop() {
        seen += 1;
        if seen > MAX_BOXES {
            return Err(XClusterError::Boxes);
        }
        if work.len() + found.len() >= MAX_LEAVES {
            return Err(XClusterError::SizeLimit);
        }
        let shape = || XClusterError::Degree(b, XDegreeError::Shape);
        let ce = pair.e.span(b.cell[0], like).ok_or_else(shape)?;
        let cg = pair.g.span(b.cell[1], like).ok_or_else(shape)?;
        let (pe, pg) = (cut(&ce, b.t, like), cut(&cg, b.s, like));
        let h = hulls(&pe, &pg, like, &mut pieces).ok_or_else(shape)?;
        if h.apart() {
            continue;
        }
        let clear = h.z_order();
        // A leaf: one z-order over the box, or at most one crossing in it.
        let leaf = clear.is_some() || transversal(&pe, &pg, like, &mut pieces);
        let first = if h.size[1] > h.size[0] {
            Dir::V
        } else {
            Dir::U
        };
        if !leaf {
            // Its degree isn't read: it splits whatever it is, where no
            // record would be read at the new corners.
            let halves = split_off(pair, &b, first, like)?;
            // Lower half processed first.
            work.push((halves[1], None));
            work.push((halves[0], None));
            continue;
        }
        let degree = match degree {
            Some(d) => d,
            // A leaf is one once its degree is known: the side records
            // settle it, or its halves' may.
            None => match cluster_degree(
                x,
                pair,
                sides,
                &b,
                like,
                (&mut pieces, &mut read),
                &mut wants,
            )? {
                Found::Degree(d) => d,
                Found::Wants => continue,
                Found::Split => {
                    let halves = split_box(pair, &b, first, like, &mut pieces)?;
                    work.push(halves[1]);
                    work.push(halves[0]);
                    continue;
                }
            },
        };
        if degree == 0 {
            continue;
        }
        if clear.is_none() && degree.abs() != 1 {
            // Transversal shadows cross at most once: the sides contradict
            // the cone at this level.
            return Err(XClusterError::Degree(b, XDegreeError::Unsettled));
        }
        found.push((b, cluster(&b, degree, clear, ZRow::EdgeEdge)));
    }
    if !wants.is_empty() {
        return Err(need(wants));
    }
    found.sort_by_key(|x| x.0);
    let (boxes, clusters) = found.into_iter().unzip();
    Ok(XClusters {
        rec: XRec { clusters, level: 0 },
        boxes,
        pieces,
    })
}

/// The clusters of two straight edges `ends` (module docs, "Two straight
/// edges"): their one cell, the one cluster where its degree isn't 0.
fn straight_clusters<S: Scalar, O: EntityOrder>(
    order: &O,
    x: XKey,
    pair: &XPair<'_>,
    ends: [[[f64; 3]; 2]; 2],
    sides: &XSides,
    like: &S,
) -> Result<XClusters, XClusterError> {
    let mut pieces = 0u64;
    let whole = XBox {
        cell: [0, 0],
        t: Extent::WHOLE,
        s: Extent::WHOLE,
    };
    let none = |pieces| XClusters {
        rec: XRec::default(),
        boxes: Vec::new(),
        pieces,
    };
    let mut wants = Vec::new();
    let degree = match cluster_degree(
        x,
        pair,
        sides,
        &whole,
        like,
        (&mut pieces, &mut 0),
        &mut wants,
    )? {
        Found::Degree(d) => d,
        Found::Wants => return Err(need(wants)),
        Found::Split => return Err(XClusterError::Degree(whole, XDegreeError::Unsettled)),
    };
    if degree == 0 {
        return Ok(none(pieces));
    }
    let [e, g] = ends;
    let (row, clear) = if parallel(e, g) {
        let Some(p) = tie_point(order, x, &pair.terms) else {
            return Ok(none(pieces));
        };
        let z = collinear_z(e, g).ok_or(XClusterError::Collinear)?;
        (ZRow::PointCurve(p), Some(z))
    } else {
        let shape = || XClusterError::Degree(whole, XDegreeError::Shape);
        let pe = pair.e.span(0, like).ok_or_else(shape)?;
        let pg = pair.g.span(0, like).ok_or_else(shape)?;
        let clear = hulls(&pe, &pg, like, &mut pieces).and_then(|h| h.z_order());
        (ZRow::EdgeEdge, clear)
    };
    Ok(XClusters {
        rec: XRec {
            clusters: vec![cluster(&whole, degree, clear, row)],
            level: 0,
        },
        boxes: vec![whole],
        pieces,
    })
}

/// The cluster of box `b`.
fn cluster(b: &XBox, degree: i64, clear: Option<Sign>, row: ZRow) -> XCluster {
    let part = |span: u32, x: Extent| XPart {
        span,
        t: [dy(x.lo()), dy(x.hi())],
    };
    XCluster {
        e: part(b.cell[0], b.t),
        g: part(b.cell[1], b.s),
        degree,
        clear,
        row,
    }
}

/// Splits a box that isn't a leaf (module docs), its halves' degrees not
/// read: `first`'s parameter first, at the middle or the first shifted
/// position whose new corners are certified off the `+r̂` ray from 0,
/// else the other parameter likewise; where none is, at the middle of
/// `first`'s parameter.
fn split_off<S: Scalar>(
    pair: &XPair<'_>,
    b: &XBox,
    first: Dir,
    like: &S,
) -> Result<[XBox; 2], XClusterError> {
    let mut fallback = None;
    for dir in [first, crate::count::boxes::other(first)] {
        let (ext, across) = match dir {
            Dir::U => (b.t, b.s),
            Dir::V => (b.s, b.t),
        };
        for k in SPLIT_SIXTEENTHS {
            let Some(at) = ext.snapped(k) else {
                continue;
            };
            let Some(halves) = b.split_at(dir, at) else {
                continue;
            };
            let off = [across.lo(), across.hi()].into_iter().all(|o| match dir {
                Dir::U => corner_off(pair, b.cell, at, o, like),
                Dir::V => corner_off(pair, b.cell, o, at, like),
            });
            if off {
                return Ok(halves);
            }
            fallback.get_or_insert(halves);
        }
    }
    fallback.ok_or(XClusterError::Depth(*b))
}

/// Splits `b` (module docs): `first`'s parameter first, at the middle or
/// the first shifted position whose halves both have degrees without the
/// side records, else the other parameter likewise; where no position
/// does, at the first position whose new corners are certified off the
/// `+r̂` ray from 0, else the middle, the halves' degrees left to find.
fn split_box<S: Scalar>(
    pair: &XPair<'_>,
    b: &XBox,
    first: Dir,
    like: &S,
    pieces: &mut u64,
) -> Result<[(XBox, Option<i64>); 2], XClusterError> {
    let mut fallback = None;
    for dir in [first, crate::count::boxes::other(first)] {
        let ext = match dir {
            Dir::U => b.t,
            Dir::V => b.s,
        };
        for k in SPLIT_SIXTEENTHS {
            let Some(at) = ext.snapped(k) else {
                continue;
            };
            let Some(halves) = b.split_at(dir, at) else {
                continue;
            };
            // Where no position settles, the first whose new corners are
            // off the ray (a cut at a point of a record's tangency with
            // the ray's line is never read; module docs), else the
            // middle.
            let off = || {
                let ext = match dir {
                    Dir::U => b.s,
                    Dir::V => b.t,
                };
                [ext.lo(), ext.hi()].into_iter().all(|o| match dir {
                    Dir::U => corner_off(pair, b.cell, at, o, like),
                    Dir::V => corner_off(pair, b.cell, o, at, like),
                })
            };
            match fallback {
                None => fallback = Some((halves, off())),
                Some((_, false)) if off() => fallback = Some((halves, true)),
                _ => {}
            }
            let mut out = [(halves[0], None); 2];
            let mut ok = true;
            for (o, h) in out.iter_mut().zip(halves) {
                match plain_degree(pair, &h, like, pieces) {
                    Ok((d, _)) => *o = (h, Some(d)),
                    // A split position's own side or corner may be the
                    // trouble: another may not be.
                    Err(Plain::Records) => {
                        ok = false;
                        break;
                    }
                    Err(Plain::Shape) => {
                        return Err(XClusterError::Degree(h, XDegreeError::Shape));
                    }
                }
            }
            if ok {
                return Ok(out);
            }
        }
    }
    match fallback {
        Some(([lo, hi], _)) => Ok([(lo, None), (hi, None)]),
        None => Err(XClusterError::Depth(*b)),
    }
}

/// Certified hulls of a box's two pieces in the frame `(r̂·x, ν·x, ê·x)`,
/// and their sizes in 3D (the summed widths of their boxes).
struct Hulls {
    frame: [[[f64; 2]; 3]; 2],
    size: [f64; 2],
}

impl Hulls {
    /// Whether the shadows' hulls are certified apart.
    fn apart(&self) -> bool {
        let [e, g] = &self.frame;
        (0..2).any(|k| e[k][1] < g[k][0] || g[k][1] < e[k][0])
    }

    /// `Pos` where `g`'s piece lies certainly above `e`'s along `ê`,
    /// `Neg` certainly below.
    fn z_order(&self) -> Option<Sign> {
        let [e, g] = &self.frame;
        if g[2][0] > e[2][1] {
            Some(Sign::Pos)
        } else if g[2][1] < e[2][0] {
            Some(Sign::Neg)
        } else {
            None
        }
    }
}

/// The hulls of two pieces, adding them to the pieces tested; `None`
/// where a weight isn't certified positive.
fn hulls<S: Scalar>(
    pe: &HomCurve<S, 3>,
    pg: &HomCurve<S, 3>,
    like: &S,
    pieces: &mut u64,
) -> Option<Hulls> {
    *pieces = pieces.saturating_add(2);
    let v = |d: glam::DVec3| [d.x, d.y, d.z].map(|c| S::from_f64(c, like));
    let rows = [v(R_HAT), v(NU), v(UP)];
    let one = |c: &HomCurve<S, 3>| -> Option<([[f64; 2]; 3], f64)> {
        let f = c.map_linear(&rows).control_box()?;
        let b = c.control_box()?;
        let size = b.iter().fold(0.0f64, |s, [lo, hi]| s + (hi - lo));
        // Rounded up: the larger piece is the one split.
        Some((f, crate::num::bounded::up(size)))
    };
    let (fe, se) = one(pe)?;
    let (fg, sg) = one(pg)?;
    Some(Hulls {
        frame: [fe, fg],
        size: [se, sg],
    })
}

/// Whether every shadow tangent of `pe` is certified off every one of
/// `pg`: each pair of control point differences, one of each piece, has
/// one strict sign of their cross product (module docs). Adds the two
/// pieces' shadows and each product tested to the pieces tested.
fn transversal<S: Scalar>(
    pe: &HomCurve<S, 3>,
    pg: &HomCurve<S, 3>,
    like: &S,
    pieces: &mut u64,
) -> bool {
    *pieces = pieces.saturating_add(2);
    let v = |d: glam::DVec3| [d.x, d.y, d.z].map(|c| S::from_f64(c, like));
    let (ex, ey) = (v(R_HAT), v(NU));
    let diffs = |c: &HomCurve<S, 3>| -> Vec<[S; 2]> {
        let sh = c.shadow(&ex, &ey);
        let w = sh.weights().coefs();
        let pts: Vec<[S; 2]> = (0..w.len())
            .map(|i| [0, 1].map(|k| sh.coords()[k].coefs()[i].div(&w[i])))
            .collect();
        let mut out = Vec::new();
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                out.push([pts[j][0].sub(&pts[i][0]), pts[j][1].sub(&pts[i][1])]);
            }
        }
        out
    };
    let mut sign = None;
    for de in diffs(pe) {
        for dg in diffs(pg) {
            *pieces = pieces.saturating_add(1);
            let c = de[0].mul(&dg[1]).sub(&de[1].mul(&dg[0]));
            match (c.sign(), sign) {
                (None | Some(Ordering::Equal), _) => return false,
                (Some(o), None) => sign = Some(o),
                (Some(o), Some(s)) if o != s => return false,
                _ => {}
            }
        }
    }
    sign.is_some()
}

/// The z-order of two straight edges whose shadows are exactly parallel,
/// over the overlap of their shadows (module docs, "Two straight edges"),
/// exactly on the stored points: `Pos` where `g` is above `e` at both of
/// the overlap's ends (so all along it: the heights are linear there),
/// `Neg` where below at both. An edge whose shadow is a point (an edge
/// along `ê`) is all its heights at that point. `None` where it isn't
/// one strict sign (the edges meet in 3D over the overlap, or cross in
/// height), where the shadows aren't collinear or don't overlap, or a
/// coordinate isn't finite.
fn collinear_z(e: [[f64; 3]; 2], g: [[f64; 3]; 2]) -> Option<Sign> {
    let q = |p: [f64; 3]| -> Option<[Rational; 3]> {
        Some([
            Rational::try_from(p[0]).ok()?,
            Rational::try_from(p[1]).ok()?,
            Rational::try_from(p[2]).ok()?,
        ])
    };
    let axis = |v: glam::DVec3| [v.x, v.y, v.z].map(|c| Rational::try_from(c).expect("finite"));
    let (r, n, up) = (axis(R_HAT), axis(NU), axis(UP));
    let dot = |a: &[Rational; 3], b: &[Rational; 3]| -> Rational {
        &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
    };
    let pts = [q(e[0])?, q(e[1])?, q(g[0])?, q(g[1])?];
    // Shadow coordinates and heights.
    let sh: Vec<[Rational; 2]> = pts.iter().map(|p| [dot(&r, p), dot(&n, p)]).collect();
    let ht: Vec<Rational> = pts.iter().map(|p| dot(&up, p)).collect();
    let diff = |i: usize, j: usize| [&sh[j][0] - &sh[i][0], &sh[j][1] - &sh[i][1]];
    let zero = Rational::from(0u32);
    let dir = [diff(0, 1), diff(2, 3)]
        .into_iter()
        .find(|d| d[0] != zero || d[1] != zero)?;
    let cross = |a: &[Rational; 2], b: &[Rational; 2]| &a[0] * &b[1] - &a[1] * &b[0];
    if [diff(0, 1), diff(0, 2), diff(0, 3)]
        .iter()
        .any(|d| cross(&dir, d) != zero)
    {
        return None;
    }
    let pos: Vec<Rational> = sh
        .iter()
        .map(|s| &dir[0] * &s[0] + &dir[1] * &s[1])
        .collect();
    let span = |i: usize| {
        if pos[i] <= pos[i + 1] {
            [pos[i].clone(), pos[i + 1].clone()]
        } else {
            [pos[i + 1].clone(), pos[i].clone()]
        }
    };
    let ([e0, e1], [g0, g1]) = (span(0), span(2));
    let lo = if e0 > g0 { e0 } else { g0 };
    let hi = if e1 < g1 { e1 } else { g1 };
    if lo > hi {
        return None;
    }
    // The heights of edge `i` (its ends `i`, `i + 1`) at position `x` in
    // its range.
    let at = |i: usize, x: &Rational| -> [Rational; 2] {
        let (pa, pb) = (&pos[i], &pos[i + 1]);
        let (ha, hb) = (&ht[i], &ht[i + 1]);
        if pa == pb {
            if ha <= hb {
                [ha.clone(), hb.clone()]
            } else {
                [hb.clone(), ha.clone()]
            }
        } else {
            let v = ha + (x - pa) / (pb - pa) * (hb - ha);
            [v.clone(), v]
        }
    };
    let mut sign = None;
    for x in [&lo, &hi] {
        let ([el, eh], [gl, gh]) = (at(0, x), at(2, x));
        let here = if gl > eh {
            Sign::Pos
        } else if gh < el {
            Sign::Neg
        } else {
            return None;
        };
        if sign.is_some_and(|s| s != here) {
            return None;
        }
        sign = Some(here);
    }
    sign
}

/// Whether two straight edges' shadows are exactly parallel: `(T_e ×
/// T_g)·ê = 0` on the stored points, exactly.
fn parallel(e: [[f64; 3]; 2], g: [[f64; 3]; 2]) -> bool {
    let q = |x: f64| Rational::try_from(x).ok();
    let d = |p: [[f64; 3]; 2]| -> Option<[Rational; 3]> {
        Some([
            q(p[1][0])? - q(p[0][0])?,
            q(p[1][1])? - q(p[0][1])?,
            q(p[1][2])? - q(p[0][2])?,
        ])
    };
    let (Some(te), Some(tg), Some(up)) = (d(e), d(g), d([[0.0; 3], [UP.x, UP.y, UP.z]])) else {
        return false;
    };
    let c = [
        &te[1] * &tg[2] - &te[2] * &tg[1],
        &te[2] * &tg[0] - &te[0] * &tg[2],
        &te[0] * &tg[1] - &te[1] * &tg[0],
    ];
    let dot = &c[0] * &up[0] + &c[1] * &up[1] + &c[2] * &up[2];
    dot == 0u32
}

#[cfg(test)]
mod tests;
