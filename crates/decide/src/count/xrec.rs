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
//!   positions, `ν·(q − p₀)/ν·(p₁ − p₀)`, enclosed. A crossing whose
//!   enclosure reaches over an inner end of the part is
//!   [`XDegreeError::Placed`]: split elsewhere. The records count
//!   against `+r̂`, so a box with an outer side is counted against that
//!   ray;
//! - **inner sides** are angle increments of `D` with one parameter held
//!   at a dyadic value: at `t = t₀`, `D = C_e(t₀) − C_g(s)` turns as
//!   `C_g` does about the point `C_e(t₀)` against the opposite ray, at
//!   `s = s₀` as `C_e` does about `C_g(s₀)` against the same ray, each by
//!   [`curve_turn`] with the held point known by enclosures (exact where
//!   it is a span's end control point). A side not certified off 0
//!   within the halvings is [`XDegreeError::Through`] (no side record
//!   for it yet); an end on the ray's line ahead of 0 stops that ray.
//!   A box with no outer side tries every ray in [`Ray::ALL`]'s order.
//!
//! **`Σ` degrees `= X` by construction.** Every certified turn is the
//! true crossing count of its side, and the certificates (sides off 0,
//! ends off the ray's line ahead of 0) hold for the moved data too, so
//! each box's degree is the winding of `D` round it in the moved
//! configuration, where the outer sides read the records; the inner
//! sides of any partition cancel, and the degrees of a partition of the
//! square sum to the winding of its boundary: `X`. Counted per box, the
//! records' crossings are split between the boxes by their parameters,
//! each counted once.
//!
//! **Clusters** ([`clusters`]). The cells are refined into the record's
//! clusters ([`XRec`]): a box whose two pieces' shadows have certified
//! disjoint hulls holds no crossing and is dropped; a box of non-zero
//! degree whose z-order is clear (the pieces' heights along `ê` certified
//! apart) or whose two pieces both lie within their bands (`β_e`, `β_g`,
//! the hull's widths summed) is a cluster; a box of degree 0 whose
//! z-order is clear or whose pieces are both that small is dropped (any
//! crossings in it cancel and read one z-order); every other box splits,
//! as does a box larger than the bands whose degree its inner sides (at
//! the edges' knots, or a split's) don't settle at its size.
//! A split cuts the larger piece's parameter (in 3D, by the hulls' summed
//! widths; `e`'s on a tie), at the middle or the first shifted position
//! (the face boxes' snapped positions) whose two halves both have degrees, else
//! the other parameter likewise; where none does, at the middle, its
//! halves splitting again (a box within the bands whose degree doesn't
//! settle is refused, [`XClusterError::Degree`]). A cluster of curved pieces whose shadow
//! tangents aren't certified apart (the cone of the pieces' control
//! point differences, which holds every tangent of a rational span with
//! positive weights, against the other's) is a shadow tangency:
//! [`XClusterError::Degenerate`], escalate. Two straight edges hold at
//! most one crossing, so their whole cell is the one cluster where `X ≠
//! 0`; if their shadows are exactly parallel (decided on the stored
//! points) it reads the point–curve row at the end whose term counted
//! the crossing ([`tie_point`]). The clusters are listed in their boxes'
//! key order, so each cluster's index is a function of the pair and the
//! level the record was decided at. A pair on one curve is never
//! searched ([`XClusterError::Shared`]): its clusters come from the
//! records alone ([`shared_clusters`](crate::records::xrec::shared_clusters)).
//!
//! Generic over the level, as the face boxes are; each call reports the
//! pieces it tested, for the pass to charge.

use std::cmp::Ordering;

use malachite::Rational;

use crate::bezier::{Dir, Dyadic, HomCurve, Scalar};
use crate::count::boxes::{Extent, QShadow, SPLIT_SIXTEENTHS, dy};
use crate::count::degree::{Ray, TurnError, curve_turn};
use crate::count::s02::MAX_LEAVES;
use crate::num::decide::Sign;
use crate::order::EntityOrder;
use crate::perturb::{NU, R_HAT, UP};
use crate::records::keys::XKey;
use crate::records::xrec::{
    EdgeRays, XCluster, XCurve, XPair, XPart, XRec, ZRow, line_param, tie_point,
};

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
}

/// A box's degree and what it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XDegree {
    /// The signed crossings inside the box.
    pub degree: i64,
    /// The ray it was counted against.
    pub ray: Ray,
    /// The curve pieces tested.
    pub pieces: u64,
}

/// Why a box's degree isn't settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum XDegreeError {
    /// An inner side isn't certified off 0 (the two curves' shadows meet
    /// on it, or too near for the level): split elsewhere or escalate.
    Through,
    /// Every ray tried stopped at an end on its line, or the pieces
    /// didn't settle: escalate the level.
    Unsettled,
    /// A record's crossing reaches over an inner end of an outer side's
    /// part (or a straight edge's crossing has no certified parameter):
    /// split elsewhere.
    Placed,
    /// The pair's data doesn't fit the box: no such span, a record of the
    /// wrong kind for its edge, a weight not certified positive.
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

/// The degree of box `b` of `pair` at `like`'s level (module docs).
pub fn box_degree<S: Scalar>(
    pair: &XPair<'_>,
    b: &XBox,
    like: &S,
) -> Result<XDegree, XDegreeError> {
    let spans = [pair.e.span_count(), pair.g.span_count()];
    if b.cell[0] >= spans[0] || b.cell[1] >= spans[1] {
        return Err(XDegreeError::Shape);
    }
    let any_outer = XSide::CCW.iter().any(|&sd| outer(b, sd, spans));
    let rays: &[Ray] = if any_outer { &[Ray::PLUS_R] } else { &Ray::ALL };
    let ce = pair.e.span(b.cell[0], like).ok_or(XDegreeError::Shape)?;
    let cg = pair.g.span(b.cell[1], like).ok_or(XDegreeError::Shape)?;
    let pe = cut(&ce, b.t, like);
    let pg = cut(&cg, b.s, like);
    // The held point of each inner side, which doesn't depend on the ray.
    let mut held = [None; 4];
    for (i, side) in XSide::CCW.into_iter().enumerate() {
        if !outer(b, side, spans) {
            held[i] = Some(held_point(pair, b, side, like).ok_or(XDegreeError::Shape)?);
        }
    }
    let mut pieces = 0u64;
    let mut through = false;
    for &ray in rays {
        let mut degree = 0i64;
        let mut stopped = false;
        for (side, q) in XSide::CCW.into_iter().zip(&held) {
            let laps = match q {
                None => side_count(pair, b, side)?,
                Some(q) => match inner_laps(side, q, &pe, &pg, ray, &mut pieces) {
                    Ok(l) => l,
                    // Off an axis ray's line and the line across it is
                    // off 0; a diagonal's pieces may need more halvings.
                    Err(TurnError::Off) if ray.is_axis() => {
                        through = true;
                        stopped = true;
                        break;
                    }
                    Err(TurnError::Off | TurnError::End | TurnError::Unsettled) => {
                        stopped = true;
                        break;
                    }
                },
            };
            degree += laps;
        }
        if through {
            // Off 0 doesn't depend on the ray.
            return Err(XDegreeError::Through);
        }
        if !stopped {
            return Ok(XDegree {
                degree,
                ray,
                pieces,
            });
        }
    }
    Err(XDegreeError::Unsettled)
}

/// The laps of an inner side about `ray`, counter-clockwise round the
/// box, `q` its held point; `pe`, `pg` the box's pieces of the two spans.
fn inner_laps<S: Scalar>(
    side: XSide,
    q: &QShadow,
    pe: &HomCurve<S, 3>,
    pg: &HomCurve<S, 3>,
    ray: Ray,
    pieces: &mut u64,
) -> Result<i64, TurnError> {
    let opposite = Ray::new(-ray.dir()[0], -ray.dir()[1]).expect("a ray's opposite");
    Ok(match side {
        // `D = C_e(t) − C_g(s₀)`: `C_e` about `C_g(s₀)`, same ray.
        XSide::Bottom => curve_turn(pe, q, ray, pieces)?.laps,
        XSide::Top => -curve_turn(pe, q, ray, pieces)?.laps,
        // `D = C_e(t₀) − C_g(s)`: `C_g` about `C_e(t₀)`, opposite ray.
        XSide::Right => curve_turn(pg, q, opposite, pieces)?.laps,
        XSide::Left => -curve_turn(pg, q, opposite, pieces)?.laps,
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

/// The part of `c` over the extent `x`, cut at its exact dyadic ends.
fn cut<S: Scalar>(c: &HomCurve<S, 3>, x: Extent, like: &S) -> HomCurve<S, 3> {
    let (a, b) = (dy(x.lo()), dy(x.hi()));
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
/// of the edge (module docs).
fn side_count(pair: &XPair<'_>, b: &XBox, side: XSide) -> Result<i64, XDegreeError> {
    let t = &pair.terms;
    // The record, the edge it counts along, the asking point, the part
    // (span and extent) and the side's direction against the edge's.
    let (rays, edge, q, span, ext, sign) = match side {
        XSide::Bottom => (t.g0_e, &pair.e, pair.g.end(false), b.cell[0], b.t, 1),
        XSide::Right => (t.b_g, &pair.g, pair.e.end(true), b.cell[1], b.s, 1),
        XSide::Top => (t.g1_e, &pair.e, pair.g.end(true), b.cell[0], b.t, -1),
        XSide::Left => (t.a_g, &pair.g, pair.e.end(false), b.cell[1], b.s, -1),
    };
    let q = q.ok_or(XDegreeError::Shape)?;
    let mut count = 0i64;
    match (rays, edge) {
        (EdgeRays::Line(r), XCurve::Line(p0, p1)) => {
            if let Some(c) = r.crossing {
                if ext != Extent::WHOLE {
                    let at = line_param(q, *p0, *p1).ok_or(XDegreeError::Placed)?;
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
        return Err(XDegreeError::Placed);
    };
    let before_hi = if ext.hi() == Dyadic::ONE || at[1] < hi {
        true
    } else if at[0] > hi {
        false
    } else {
        return Err(XDegreeError::Placed);
    };
    Ok(past_lo && before_hi)
}

/// Why an edge pair's clusters aren't formed at a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum XClusterError {
    /// A box's degree isn't settled, and no split of it gives both
    /// halves one (or a cell's own sides don't): escalate, or the sides
    /// need records not there yet.
    Degree(XBox, XDegreeError),
    /// A cluster's shadow tangents aren't certified apart: a shadow
    /// tangency of curved pieces, escalate (module docs).
    Degenerate(XBox),
    /// A box can't split further within the depth limit: escalate.
    Depth(XBox),
    /// More than [`MAX_LEAVES`] boxes at once.
    SizeLimit,
    /// The pair's edges lie on one curve: no geometric search, its
    /// clusters are the records'
    /// ([`shared_clusters`](crate::records::xrec::shared_clusters)).
    Shared,
}

/// An edge pair's clusters and what they took.
#[derive(Clone, Debug, PartialEq)]
pub struct XClusters {
    /// The record's clusters.
    pub rec: XRec,
    /// Each cluster's box, by the same index.
    pub boxes: Vec<XBox>,
    /// The curve pieces tested.
    pub pieces: u64,
}

/// The clusters of pair `x` (`pair` its curves and terms) at `like`'s
/// level, `beta` the bands `[β_e, β_g]` (module docs).
pub fn clusters<S: Scalar, O: EntityOrder>(
    order: &O,
    x: XKey,
    pair: &XPair<'_>,
    beta: [f64; 2],
    like: &S,
) -> Result<XClusters, XClusterError> {
    if pair.shared {
        return Err(XClusterError::Shared);
    }
    let spans = [pair.e.span_count(), pair.g.span_count()];
    let mut pieces = 0u64;
    if let (XCurve::Line(a, b), XCurve::Line(g0, g1)) = (pair.e, pair.g) {
        let whole = XBox {
            cell: [0, 0],
            t: Extent::WHOLE,
            s: Extent::WHOLE,
        };
        let d = box_degree(pair, &whole, like).map_err(|e| XClusterError::Degree(whole, e))?;
        pieces += d.pieces;
        let mut out = XClusters {
            rec: XRec::default(),
            boxes: Vec::new(),
            pieces,
        };
        if d.degree == 0 {
            return Ok(out);
        }
        let row = if parallel([a, b], [g0, g1]) {
            match tie_point(order, x, &pair.terms) {
                Some(p) => ZRow::PointCurve(p),
                None => return Ok(out),
            }
        } else {
            ZRow::EdgeEdge
        };
        let pe = pair
            .e
            .span(0, like)
            .ok_or(XClusterError::Degree(whole, XDegreeError::Shape))?;
        let pg = pair
            .g
            .span(0, like)
            .ok_or(XClusterError::Degree(whole, XDegreeError::Shape))?;
        let clear = hulls(&pe, &pg, like).and_then(|h| h.z_order());
        out.rec.clusters.push(cluster(&whole, d.degree, clear, row));
        out.boxes.push(whole);
        return Ok(out);
    }
    let mut work: Vec<(XBox, Option<i64>)> = XBox::cells(spans).map(|b| (b, None)).collect();
    work.reverse();
    let mut found: Vec<(XBox, XCluster)> = Vec::new();
    while let Some((b, degree)) = work.pop() {
        if work.len() + found.len() >= MAX_LEAVES {
            return Err(XClusterError::SizeLimit);
        }
        let shape = XClusterError::Degree(b, XDegreeError::Shape);
        let ce = pair.e.span(b.cell[0], like).ok_or(shape)?;
        let cg = pair.g.span(b.cell[1], like).ok_or(shape)?;
        let (pe, pg) = (cut(&ce, b.t, like), cut(&cg, b.s, like));
        let h = hulls(&pe, &pg, like).ok_or(shape)?;
        if h.apart() {
            continue;
        }
        let clear = h.z_order();
        let small = h.size[0] <= beta[0] && h.size[1] <= beta[1];
        let degree = match degree {
            Some(d) => d,
            None => match box_degree(pair, &b, like) {
                Ok(d) => {
                    pieces += d.pieces;
                    d.degree
                }
                // A box whose inner sides (at the edges' knots, or a
                // split's) aren't settled at its size: its halves' are,
                // or they split again, down to the bands.
                Err(XDegreeError::Through | XDegreeError::Placed | XDegreeError::Unsettled)
                    if !small =>
                {
                    let first = if h.size[1] > h.size[0] {
                        Dir::V
                    } else {
                        Dir::U
                    };
                    let halves = split_box(pair, &b, first, like, &mut pieces)?;
                    work.push(halves[1]);
                    work.push(halves[0]);
                    continue;
                }
                Err(e) => return Err(XClusterError::Degree(b, e)),
            },
        };
        if degree == 0 && (clear.is_some() || small) {
            continue;
        }
        if clear.is_some() || small {
            if clear.is_none() && !transversal(&pe, &pg, like) {
                return Err(XClusterError::Degenerate(b));
            }
            found.push((b, cluster(&b, degree, clear, ZRow::EdgeEdge)));
            continue;
        }
        let first = if h.size[1] > h.size[0] {
            Dir::V
        } else {
            Dir::U
        };
        let halves = split_box(pair, &b, first, like, &mut pieces)?;
        // Lower half processed first.
        work.push(halves[1]);
        work.push(halves[0]);
    }
    found.sort_by_key(|x| x.0);
    let (boxes, clusters) = found.into_iter().unzip();
    Ok(XClusters {
        rec: XRec { clusters },
        boxes,
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

/// Splits `b` (module docs): `first`'s parameter first, at the middle or
/// the first shifted position whose halves both have degrees, else the
/// other parameter likewise; where no position does, at the middle of
/// `first`'s parameter, the halves' degrees left to find (each splits
/// again until its own degree settles or it is within the bands).
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
            if fallback.is_none() {
                fallback = Some(halves);
            }
            let mut out = [(halves[0], None); 2];
            let mut ok = true;
            for (o, h) in out.iter_mut().zip(halves) {
                match box_degree(pair, &h, like) {
                    Ok(d) => {
                        *pieces += d.pieces;
                        *o = (h, Some(d.degree));
                    }
                    // A ray stopped at an end on its line may be a split
                    // position's own: another may not be.
                    Err(XDegreeError::Through | XDegreeError::Placed | XDegreeError::Unsettled) => {
                        ok = false;
                        break;
                    }
                    Err(e) => return Err(XClusterError::Degree(h, e)),
                }
            }
            if ok {
                return Ok(out);
            }
        }
    }
    match fallback {
        Some([lo, hi]) => Ok([(lo, None), (hi, None)]),
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

/// The hulls of two pieces; `None` where a weight isn't certified
/// positive.
fn hulls<S: Scalar>(pe: &HomCurve<S, 3>, pg: &HomCurve<S, 3>, like: &S) -> Option<Hulls> {
    let v = |d: glam::DVec3| [d.x, d.y, d.z].map(|c| S::from_f64(c, like));
    let rows = [v(R_HAT), v(NU), v(UP)];
    let one = |c: &HomCurve<S, 3>| -> Option<([[f64; 2]; 3], f64)> {
        let f = c.map_linear(&rows).control_box()?;
        let b = c.control_box()?;
        let size = b.iter().fold(0.0f64, |s, [lo, hi]| s + (hi - lo));
        // Rounded up: a size within the band is certified.
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
/// one strict sign of their cross product (module docs).
fn transversal<S: Scalar>(pe: &HomCurve<S, 3>, pg: &HomCurve<S, 3>, like: &S) -> bool {
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
