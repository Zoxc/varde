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
//! The boxes of the square and their degrees are counting's
//! ([`crate::count::xrec`]).

use crate::bezier::{Dyadic, HomCurve, Scalar};
use crate::records::ray::{ChainRec, Span};
use crate::records::shadow::RayRec;

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
}
