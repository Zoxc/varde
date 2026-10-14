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
//! Generic over the level, as the face boxes are; each call reports the
//! pieces it tested, for the pass to charge.

use std::cmp::Ordering;

use crate::bezier::{Dir, Dyadic, HomCurve, Scalar};
use crate::count::boxes::{Extent, QShadow, dy};
use crate::count::degree::{Ray, TurnError, curve_turn};
use crate::num::bounded::Bounded;
use crate::perturb::NU;
use crate::records::xrec::{EdgeRays, XCurve, XPair};

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

/// The enclosure of the parameter where `q`'s `r̂` line meets the line
/// from `p0` to `p1`, `ν·(q − p₀)/ν·(p₁ − p₀)`; `None` unless the
/// denominator is certainly non-zero.
fn line_param(q: [f64; 3], p0: [f64; 3], p1: [f64; 3]) -> Option<[f64; 2]> {
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

#[cfg(test)]
mod tests;
