//! Curves: an edge's space curve ([`Curve3D`]) and a coedge's curve in
//! its face's global UV ([`PCurve`]), each a chain of rational quadratic
//! spans over a parameter `t` that runs through the chain's knots.
//!
//! **Storage.** A span is its three control points **affine** with their
//! weights, `(x, y, z, w)` in space and `(u, v, w)` in UV (a point and
//! its weight, the NURBS layout), never the products `w·x`. The span is
//! `Σ Bᵢ·wᵢ·xᵢ / Σ Bᵢ·wᵢ`. Span `k` covers `[knots[k], knots[k + 1]]` of
//! `t`, mapped linearly to its own `[0, 1]`. A chain is refused unless
//! every value is finite, every weight lies in `[MIN_WEIGHT, MAX_WEIGHT]`
//! = `[1/64, 64]`, the knots (one more than the spans) increase strictly,
//! and consecutive spans join **bit for bit**: the last control point of
//! span `k` and the first of span `k + 1` are the same point, the same
//! `f64`s (their weights may differ).
//!
//! **Points at knots.** For any positive weights a span passes exactly
//! through its first control point at its start and its last at its
//! end. So the chain's point at a knot is a control point, exactly, with
//! nothing projected or rounded ([`Curve3D::knot_point`],
//! [`PCurve::knot_point`]): joins compare those bit for bit, between
//! spans here, between an edge's curve and its vertices, between
//! consecutive coedges' p-curves. Certified evaluation at a knot gives
//! that point with no error.
//!
//! **Levels.** A computation forms a span's homogeneous coefficients
//! `(w·x, w)` at its precision level ([`Curve3D::span_at`],
//! [`HomCurve::from_affine`]): exact from double-double up; at the first
//! level rounded once per coefficient, the rounding carried, and exact
//! where the product is a float (a weight of 1, as on lines).
//!
//! A p-curve so far is Q spans only, rational quadratics in global UV;
//! linear and iso spans are Q spans with their middle control point on
//! the chord.
//!
//! **Canonical points and splits.** Where two edges share a chain and one
//! of them ends inside it, both must see the same point there, to the
//! bit. [`Curve3D::point_at`] is the one evaluator for that: the exact
//! point of the stored span at `t` (rational arithmetic on the stored
//! `f64`s), rounded to the nearest `f64` once per coordinate.
//! [`Curve3D::split`] inserts knots: each new knot's control point is that
//! point of the chain it is called on, so the bits there don't depend on
//! which other knots are inserted with it, or by whom; the spans between
//! are the exact sub-spans of the stored ones (their blossoms), rounded
//! once per value, so a split moves the curve by at most that rounding,
//! and the spans it doesn't cut keep their bits. [`Curve3D::slice`] takes
//! the spans between two knots. The same for p-curves.
//!
//! **Bounded control points.** Every control point of a space curve, the
//! middle ones included, is a bounded position: within the `MAX_COORD`
//! box `[−MAX_COORD, MAX_COORD]³` ([`varde_decide::num::storage`]). A
//! middle control point may lie far off its curve (about `|chord|/w₁`,
//! 64 times the chord at the least weight), so a curve well inside the
//! box can have one outside it. [`Curve3D::bounded`] is the chain
//! constructors' rule for that: a span whose middle control point, as
//! stored, would leave the box is split at the middle of its knots
//! (the `f64` midpoint), and again, until every middle point is inside;
//! the split takes the middle weight towards the ends' and the middle
//! point towards the curve. The cuts are those of [`Curve3D::split`] of
//! the given chain (its canonical points, its exact sub-spans rounded
//! once), so the result is that split, bit for bit. A chain is refused
//! (`Range`) where a point of the curve itself is outside the box (an
//! end, or a cut's knot point), and where a span still fails after
//! [`MAX_RANGE_DEPTH`] halvings (a curve touching the box's boundary
//! from inside). The number of splits made is the spans gained. A split
//! of a bounded chain stays bounded: its control points are convex
//! combinations of the stored ones, rounded to nearest, and the box's
//! bound is an `f64`.
//!
//! **Conic chains.** [`Curve3D::ellipse`] is the chain of an ellipse's
//! 90° cells at a level ([`Conic::quarter_at`]; knots in quarter turns,
//! `i·2^−L` at the start of span `i`, so a closed ellipse ends on its
//! first point, bit for bit); [`Curve3D::conic_arcs`] the chain of a
//! parabola's or hyperbola's arcs ([`Conic::arc`]) between consecutive
//! parameters, the parameters its knots. Both are bounded as above, the
//! cells' range splits the cells' own finer levels, the same bits as
//! the lattice's.

use malachite::Rational;
use varde_decide::bezier::{HomCurve, Scalar};
use varde_decide::num::bounded::Bounded;

use varde_decide::num::storage::MAX_COORD;

use super::conic::{Conic, ConicError, ConicKind};
use super::lattice::MAX_LEVEL;
use super::round::{nearest, q};

/// The least weight of a span's control point.
pub const MIN_WEIGHT: f64 = 1.0 / 64.0;

/// The greatest weight of a span's control point.
pub const MAX_WEIGHT: f64 = 64.0;

/// The most halvings of one span the range split makes
/// ([`Curve3D::bounded`]).
pub const MAX_RANGE_DEPTH: u32 = 16;

/// A space span: three affine control points with their weights,
/// `(x, y, z, w)`.
pub type Span3 = [[f64; 4]; 3];

/// A UV span: three affine control points with their weights,
/// `(u, v, w)`.
pub type Span2 = [[f64; 3]; 3];

/// Why a chain can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveError {
    /// No spans, or not one knot more than spans.
    Shape,
    /// A value of span `span` is not finite.
    NotFinite { span: usize },
    /// A weight of span `span` lies outside `[MIN_WEIGHT, MAX_WEIGHT]`.
    Weight { span: usize },
    /// A knot is not finite, or knot `knot` is not above the one before.
    Knots { knot: usize },
    /// Span `span`'s first control point isn't span `span − 1`'s last,
    /// bit for bit.
    Join { span: usize },
    /// A point of span `span`'s curve lies outside the `MAX_COORD` box,
    /// or its middle control point can't be brought inside it by
    /// [`MAX_RANGE_DEPTH`] halvings ([`Curve3D::bounded`]).
    Range { span: usize },
    /// A conic chain's conic refused a span.
    Conic(ConicError),
}

/// An edge's curve: a chain of rational quadratic spans in space (module
/// docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Curve3D {
    spans: Vec<Span3>,
    knots: Vec<f64>,
}

/// A coedge's curve in its face's global UV: a chain of Q spans (module
/// docs).
#[derive(Clone, Debug, PartialEq)]
pub struct PCurve {
    spans: Vec<Span2>,
    knots: Vec<f64>,
}

impl Curve3D {
    /// The chain of `spans`, span `k` over `[knots[k], knots[k + 1]]`,
    /// validated (module docs); else the first offence.
    pub fn new(spans: Vec<Span3>, knots: Vec<f64>) -> Result<Curve3D, CurveError> {
        validate(&spans, &knots)?;
        Ok(Curve3D { spans, knots })
    }

    /// The chain of `spans` over `knots` as [`new`](Self::new) checks
    /// it, every span whose middle control point would leave the
    /// `MAX_COORD` box split at the middle of its knots until none does
    /// (module docs): the same bits as [`split`](Self::split) at the
    /// knots it adds. `Range` naming the given span where that fails.
    pub fn bounded(spans: Vec<Span3>, knots: Vec<f64>) -> Result<Curve3D, CurveError> {
        validate(&spans, &knots)?;
        let pieces: Vec<Piece> = spans
            .iter()
            .zip(knots.windows(2))
            .map(|(parent, k)| Piece {
                parent: *parent,
                a: k[0],
                b: k[1],
                t0: k[0],
                t1: k[1],
            })
            .collect();
        let (spans, knots) = bound_pieces(&pieces)?;
        Curve3D::new(spans, knots)
    }

    /// The ellipse's chain of spans `from..to` of level `level`
    /// ([`Conic::quarter_at`]), span `i` over `[i, i + 1]·2^−level` in
    /// quarter turns, bounded as [`bounded`](Self::bounded) does (its
    /// splits the cells' finer levels). `Shape` unless `from < to`, at
    /// most a full turn (`to − from ≤ 4·2^level`) and both within
    /// `±2⁵²`; `Conic(Kind)` for another conic, `Conic(Level)` past
    /// [`MAX_LEVEL`]; `Range` (span index from `from`) as for
    /// [`bounded`](Self::bounded).
    pub fn ellipse(
        conic: &Conic<3>,
        level: u32,
        from: i64,
        to: i64,
    ) -> Result<Curve3D, CurveError> {
        if conic.kind() != ConicKind::Ellipse {
            return Err(CurveError::Conic(ConicError::Kind));
        }
        if level > MAX_LEVEL {
            return Err(CurveError::Conic(ConicError::Level));
        }
        const LIMIT: i64 = 1 << 52;
        let turn = 4i64 << level;
        if !(-LIMIT..=LIMIT).contains(&from)
            || !(-LIMIT..=LIMIT).contains(&to)
            || from >= to
            || to - from > turn
        {
            return Err(CurveError::Shape);
        }
        // Exact: `|i| ≤ 2⁵²` and a power of two.
        let scale = 1.0 / (1u64 << level) as f64;
        let mut pieces = Vec::new();
        let mut cell: Option<(i64, Span3)> = None;
        for i in from..to {
            let k = i >> level;
            let parent = match cell {
                Some((c, s)) if c == k => s,
                _ => {
                    let s = conic.quarter::<4>(k).map_err(CurveError::Conic)?;
                    cell = Some((k, s));
                    s
                }
            };
            pieces.push(Piece {
                parent,
                a: k as f64,
                b: (k + 1) as f64,
                t0: i as f64 * scale,
                t1: (i + 1) as f64 * scale,
            });
        }
        let (spans, knots) = bound_pieces(&pieces)?;
        Curve3D::new(spans, knots)
    }

    /// A parabola's or hyperbola's chain of arcs ([`Conic::arc`]) between
    /// consecutive `params` (`s` on a hyperbola), the params its knots,
    /// bounded as [`bounded`](Self::bounded) does. `Shape` for fewer than
    /// two params; the arcs' refusals as `Conic(..)`; then as
    /// [`bounded`](Self::bounded).
    pub fn conic_arcs(conic: &Conic<3>, params: &[f64]) -> Result<Curve3D, CurveError> {
        if params.len() < 2 {
            return Err(CurveError::Shape);
        }
        let spans = params
            .windows(2)
            .map(|p| conic.arc::<4>(p[0], p[1]))
            .collect::<Result<Vec<_>, _>>()
            .map_err(CurveError::Conic)?;
        Curve3D::bounded(spans, params.to_vec())
    }

    /// The spans.
    #[inline]
    pub fn spans(&self) -> &[Span3] {
        &self.spans
    }

    /// The knots, one more than the spans.
    #[inline]
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The point at knot `k`: the control point there (module docs),
    /// `None` past the last knot.
    pub fn knot_point(&self, k: usize) -> Option<[f64; 3]> {
        let p = knot_control(&self.spans, k)?;
        Some([p[0], p[1], p[2]])
    }

    /// Span `k` at the first level ([`span_at`](Self::span_at)).
    pub fn span(&self, k: usize) -> Option<HomCurve<Bounded<f64>, 3>> {
        self.span_at(k, &Bounded::exact(0.0))
    }

    /// Span `k` in homogeneous form at `like`'s level (and width), its
    /// coefficients `(w·x, w)` formed there ([`HomCurve::from_affine`]).
    pub fn span_at<S: Scalar>(&self, k: usize, like: &S) -> Option<HomCurve<S, 3>> {
        self.spans.get(k).map(|s| hom(s, like))
    }

    /// The point at `t`, certified, at the first level
    /// ([`eval_at`](Self::eval_at)).
    pub fn eval(&self, t: f64) -> Option<[Bounded<f64>; 3]> {
        self.eval_at(t, &Bounded::exact(0.0))
    }

    /// The point at `t` at `like`'s level, certified, `None` outside the
    /// knots' range: at a knot its control point exactly (the lower span's
    /// end at an inner knot, the same point); else on the span holding
    /// `t`, at the local parameter `(t − knots[k]) / (knots[k + 1] −
    /// knots[k])`.
    pub fn eval_at<S: Scalar>(&self, t: f64, like: &S) -> Option<[S; 3]> {
        eval_chain(&self.spans, &self.knots, t, like)
    }

    /// The canonical point at `t` (module docs): exact, rounded to the
    /// nearest `f64`s; at a knot its control point. `None` outside the
    /// knots' range.
    pub fn point_at(&self, t: f64) -> Option<[f64; 3]> {
        point_chain(&self.spans, &self.knots, t)
    }

    /// The chain with knots inserted at `ts` (module docs): those inside
    /// its range and not knots already; the rest are ignored. The new
    /// chain validated as by [`new`](Self::new).
    pub fn split(&self, ts: &[f64]) -> Result<Curve3D, CurveError> {
        let (spans, knots) = split_chain(&self.spans, &self.knots, ts);
        Curve3D::new(spans, knots)
    }

    /// The spans between knots `t0 < t1`, `None` unless both are knots.
    pub fn slice(&self, t0: f64, t1: f64) -> Option<Curve3D> {
        let (spans, knots) = slice_chain(&self.spans, &self.knots, t0, t1)?;
        Some(Curve3D { spans, knots })
    }
}

impl PCurve {
    /// The chain of `spans`, span `k` over `[knots[k], knots[k + 1]]`,
    /// validated (module docs); else the first offence.
    pub fn new(spans: Vec<Span2>, knots: Vec<f64>) -> Result<PCurve, CurveError> {
        validate(&spans, &knots)?;
        Ok(PCurve { spans, knots })
    }

    /// This p-curve with each span's middle control point moved by its
    /// `offsets` entry in UV, `(u + du, v + dv, w)` in `f64`, the end
    /// control points kept: the curve moves off
    /// its image by up to about the offsets times the surface's speed,
    /// while its ends, and so its joins, stay to the bit. For tests that
    /// need ribbons of a given width. `Shape` unless there is one offset
    /// per span; the moved chain validated as by [`new`](Self::new).
    pub fn perturbed(&self, offsets: &[[f64; 2]]) -> Result<PCurve, CurveError> {
        if offsets.len() != self.spans.len() {
            return Err(CurveError::Shape);
        }
        let spans = self
            .spans
            .iter()
            .zip(offsets)
            .map(|(s, [du, dv])| {
                let mut s = *s;
                s[1][0] += du;
                s[1][1] += dv;
                s
            })
            .collect();
        PCurve::new(spans, self.knots.clone())
    }

    /// The spans.
    #[inline]
    pub fn spans(&self) -> &[Span2] {
        &self.spans
    }

    /// The knots, one more than the spans.
    #[inline]
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The UV point at knot `k`: the control point there (module docs),
    /// `None` past the last knot.
    pub fn knot_point(&self, k: usize) -> Option<[f64; 2]> {
        let p = knot_control(&self.spans, k)?;
        Some([p[0], p[1]])
    }

    /// Span `k` at the first level ([`span_at`](Self::span_at)).
    pub fn span(&self, k: usize) -> Option<HomCurve<Bounded<f64>, 2>> {
        self.span_at(k, &Bounded::exact(0.0))
    }

    /// Span `k` in homogeneous form at `like`'s level (and width), its
    /// coefficients `(w·u, w·v, w)` formed there
    /// ([`HomCurve::from_affine`]).
    pub fn span_at<S: Scalar>(&self, k: usize, like: &S) -> Option<HomCurve<S, 2>> {
        self.spans.get(k).map(|s| hom(s, like))
    }

    /// The UV point at `t`, certified, at the first level (as
    /// [`Curve3D::eval_at`]).
    pub fn eval(&self, t: f64) -> Option<[Bounded<f64>; 2]> {
        self.eval_at(t, &Bounded::exact(0.0))
    }

    /// The UV point at `t` at `like`'s level, certified, `None` outside
    /// the knots' range (as [`Curve3D::eval_at`]).
    pub fn eval_at<S: Scalar>(&self, t: f64, like: &S) -> Option<[S; 2]> {
        eval_chain(&self.spans, &self.knots, t, like)
    }

    /// The canonical UV point at `t` (as [`Curve3D::point_at`]).
    pub fn point_at(&self, t: f64) -> Option<[f64; 2]> {
        point_chain(&self.spans, &self.knots, t)
    }

    /// The chain with knots inserted at `ts` (as [`Curve3D::split`]).
    pub fn split(&self, ts: &[f64]) -> Result<PCurve, CurveError> {
        let (spans, knots) = split_chain(&self.spans, &self.knots, ts);
        PCurve::new(spans, knots)
    }

    /// The spans between knots `t0 < t1` (as [`Curve3D::slice`]).
    pub fn slice(&self, t0: f64, t1: f64) -> Option<PCurve> {
        let (spans, knots) = slice_chain(&self.spans, &self.knots, t0, t1)?;
        Some(PCurve { spans, knots })
    }
}

/// A chain's spans and knots.
type Chain<const D: usize> = (Vec<[[f64; D]; 3]>, Vec<f64>);

/// The part over `[t0, t1]` of a stored span `parent` over `[a, b]`
/// (`a ≤ t0 < t1 ≤ b`), a span of a chain [`bound_pieces`] builds.
struct Piece {
    parent: Span3,
    a: f64,
    b: f64,
    t0: f64,
    t1: f64,
}

/// Whether a space control point lies in the `MAX_COORD` box (false for
/// a non-finite one).
fn in_box(p: &[f64; 4]) -> bool {
    p[..3].iter().all(|x| x.abs() <= MAX_COORD)
}

/// The chain of `pieces`, each the exact sub-span of its parent rounded
/// once, its ends the parent's control points or its canonical points,
/// as [`split_chain`] cuts; a piece whose middle control point is
/// outside the box halved at the `f64` midpoint of its knots until none
/// is (as [`Curve3D::bounded`]). Unchecked otherwise.
fn bound_pieces(pieces: &[Piece]) -> Result<Chain<4>, CurveError> {
    let mut spans = Vec::with_capacity(pieces.len());
    let mut knots = Vec::with_capacity(pieces.len() + 1);
    knots.extend(pieces.first().map(|p| p.t0));
    for (span, piece) in pieces.iter().enumerate() {
        let range = CurveError::Range { span };
        let Piece { parent, a, b, .. } = *piece;
        let h = parent.each_ref().map(hom_exact);
        let point = |t: f64| -> [f64; 4] {
            if t == a {
                parent[0]
            } else if t == b {
                parent[2]
            } else {
                let s = local_exact(t, a, b);
                affine_rounded(&blossom(&h, &s, &s))
            }
        };
        let (p0, p1) = (point(piece.t0), point(piece.t1));
        if !(in_box(&p0) && in_box(&p1)) {
            return Err(range);
        }
        // Depth first, the lower half first, so spans come in order.
        let mut todo = vec![(piece.t0, p0, piece.t1, p1, 0u32)];
        while let Some((t0, p0, t1, p1, depth)) = todo.pop() {
            let mid = affine_rounded(&blossom(&h, &local_exact(t0, a, b), &local_exact(t1, a, b)));
            if in_box(&mid) {
                spans.push([p0, mid, p1]);
                knots.push(t1);
                continue;
            }
            // Not `(t0 + t1) / 2`, which may overflow.
            let tm = t0 / 2.0 + t1 / 2.0;
            if depth == MAX_RANGE_DEPTH || !(t0 < tm && tm < t1) {
                return Err(range);
            }
            let pm = point(tm);
            if !in_box(&pm) {
                return Err(range);
            }
            todo.push((tm, pm, t1, p1, depth + 1));
            todo.push((t0, p0, tm, pm, depth + 1));
        }
    }
    Ok((spans, knots))
}

/// A span's control point in homogeneous form, exactly: `(w·x, w)`.
pub(super) fn hom_exact<const D: usize>(p: &[f64; D]) -> [Rational; D] {
    let w = q(p[D - 1]);
    std::array::from_fn(|k| if k + 1 == D { w.clone() } else { q(p[k]) * &w })
}

/// The blossom of a quadratic span's homogeneous control points at
/// `(a, b)`: `(1 − a)(1 − b)·H₀ + ((1 − a)b + a(1 − b))·H₁ + ab·H₂`.
pub(super) fn blossom<const D: usize>(
    h: &[[Rational; D]; 3],
    a: &Rational,
    b: &Rational,
) -> [Rational; D] {
    let one = Rational::from(1u32);
    let (ma, mb) = (&one - a, &one - b);
    let c0 = &ma * &mb;
    let c1 = &ma * b + a * &mb;
    let c2 = a * b;
    std::array::from_fn(|k| &c0 * &h[0][k] + &c1 * &h[1][k] + &c2 * &h[2][k])
}

/// A homogeneous point as a stored control point, its point and weight
/// rounded to the nearest `f64`s.
pub(super) fn affine_rounded<const D: usize>(h: &[Rational; D]) -> [f64; D] {
    let w = &h[D - 1];
    std::array::from_fn(|k| {
        if k + 1 == D {
            nearest(w)
        } else {
            nearest(&(&h[k] / w))
        }
    })
}

/// The local parameter of `t` on `[a, b]`, exactly.
fn local_exact(t: f64, a: f64, b: f64) -> Rational {
    (q(t) - q(a)) / (q(b) - q(a))
}

/// The canonical point of a chain at `t` (as [`Curve3D::point_at`]).
fn point_chain<const D: usize, const N: usize>(
    spans: &[[[f64; D]; 3]],
    knots: &[f64],
    t: f64,
) -> Option<[f64; N]> {
    debug_assert_eq!(N + 1, D);
    let (k, at) = locate(knots, t)?;
    let p = match at {
        At::Start => spans[k][0],
        At::End => spans[k][2],
        At::Inside => {
            let s = local_exact(t, knots[k], knots[k + 1]);
            affine_rounded(&blossom(&spans[k].each_ref().map(hom_exact), &s, &s))
        }
    };
    Some(std::array::from_fn(|c| p[c]))
}

/// A chain with knots inserted at `ts` (as [`Curve3D::split`]), unchecked.
fn split_chain<const D: usize>(spans: &[[[f64; D]; 3]], knots: &[f64], ts: &[f64]) -> Chain<D> {
    let mut out_spans = Vec::with_capacity(spans.len() + ts.len());
    let mut out_knots = Vec::with_capacity(knots.len() + ts.len());
    out_knots.push(knots[0]);
    for (k, span) in spans.iter().enumerate() {
        let (a, b) = (knots[k], knots[k + 1]);
        let mut inner: Vec<f64> = ts.iter().copied().filter(|&t| a < t && t < b).collect();
        inner.sort_by(f64::total_cmp);
        inner.dedup();
        if inner.is_empty() {
            out_spans.push(*span);
            out_knots.push(b);
            continue;
        }
        let h = span.each_ref().map(hom_exact);
        // The stored ends, and between them each new knot's canonical
        // point, the same bits as `point_chain` gives there.
        let mut cuts: Vec<(Rational, [f64; D])> = vec![(Rational::from(0u32), span[0])];
        for &t in &inner {
            let s = local_exact(t, a, b);
            let p = affine_rounded(&blossom(&h, &s, &s));
            cuts.push((s, p));
        }
        cuts.push((Rational::from(1u32), span[2]));
        // The sub-span over `[s₀, s₁]`: the blossom's `(s₀, s₀)`, `(s₀, s₁)`
        // and `(s₁, s₁)`, its ends the points (and weights) above.
        for w in cuts.windows(2) {
            let ((s0, p0), (s1, p1)) = (&w[0], &w[1]);
            out_spans.push([*p0, affine_rounded(&blossom(&h, s0, s1)), *p1]);
        }
        out_knots.extend(inner);
        out_knots.push(b);
    }
    (out_spans, out_knots)
}

/// The spans of a chain between its knots `t0 < t1`.
fn slice_chain<const D: usize>(
    spans: &[[[f64; D]; 3]],
    knots: &[f64],
    t0: f64,
    t1: f64,
) -> Option<Chain<D>> {
    let i = knots.iter().position(|&k| k == t0)?;
    let j = knots.iter().position(|&k| k == t1)?;
    (i < j).then(|| (spans[i..j].to_vec(), knots[i..=j].to_vec()))
}

/// The span of `D − 1` coordinates and a weight in homogeneous form at
/// `like`'s level.
fn hom<S: Scalar, const D: usize, const N: usize>(s: &[[f64; D]; 3], like: &S) -> HomCurve<S, N> {
    debug_assert_eq!(N + 1, D);
    HomCurve::from_affine(
        s.iter().map(|p| (std::array::from_fn(|k| p[k]), p[D - 1])),
        like,
    )
    .expect("three control points")
}

/// The point of a chain at `t` at `like`'s level (as
/// [`Curve3D::eval_at`]).
fn eval_chain<S: Scalar, const D: usize, const N: usize>(
    spans: &[[[f64; D]; 3]],
    knots: &[f64],
    t: f64,
    like: &S,
) -> Option<[S; N]> {
    let (k, at) = locate(knots, t)?;
    let end = match at {
        At::Start => Some(spans[k][0]),
        At::End => Some(spans[k][2]),
        At::Inside => None,
    };
    if let Some(p) = end {
        return Some(std::array::from_fn(|c| S::from_f64(p[c], like)));
    }
    let (a, b) = (knots[k], knots[k + 1]);
    let s = S::from_f64(t, like)
        .sub(&S::from_f64(a, like))
        .div(&S::from_f64(b, like).sub(&S::from_f64(a, like)));
    Some(hom(&spans[k], like).eval(&s))
}

/// The control point at knot `k`: span `k`'s first, or the last span's
/// last.
fn knot_control<const D: usize>(spans: &[[[f64; D]; 3]], k: usize) -> Option<[f64; D]> {
    match spans.get(k) {
        Some(s) => Some(s[0]),
        None if k == spans.len() => spans.last().map(|s| s[2]),
        None => None,
    }
}

fn validate<const D: usize>(spans: &[[[f64; D]; 3]], knots: &[f64]) -> Result<(), CurveError> {
    if spans.is_empty() || knots.len() != spans.len() + 1 {
        return Err(CurveError::Shape);
    }
    for (span, s) in spans.iter().enumerate() {
        if !s.iter().flatten().all(|x| x.is_finite()) {
            return Err(CurveError::NotFinite { span });
        }
        if !s
            .iter()
            .all(|p| (MIN_WEIGHT..=MAX_WEIGHT).contains(&p[D - 1]))
        {
            return Err(CurveError::Weight { span });
        }
    }
    if !knots[0].is_finite() {
        return Err(CurveError::Knots { knot: 0 });
    }
    for knot in 1..knots.len() {
        // False for a NaN.
        if !(knots[knot] > knots[knot - 1] && knots[knot].is_finite()) {
            return Err(CurveError::Knots { knot });
        }
    }
    for span in 1..spans.len() {
        // The points, not their weights.
        let a = &spans[span - 1][2][..D - 1];
        let b = &spans[span][0][..D - 1];
        if a.iter().zip(b).any(|(x, y)| x.to_bits() != y.to_bits()) {
            return Err(CurveError::Join { span });
        }
    }
    Ok(())
}

/// Where `t` lies on the span [`locate`] gives.
enum At {
    Start,
    End,
    Inside,
}

/// The span holding `t` (the lower one at an inner knot) and where on it;
/// `None` outside the knots' range.
fn locate(knots: &[f64], t: f64) -> Option<(usize, At)> {
    let (first, last) = (knots[0], knots[knots.len() - 1]);
    // False for a NaN.
    if !(first <= t && t <= last) {
        return None;
    }
    // The first knot at or above `t`, at least 1.
    let hi = knots.partition_point(|&k| k < t).max(1);
    let at = if t == knots[hi - 1] {
        At::Start
    } else if t == knots[hi] {
        At::End
    } else {
        At::Inside
    };
    Some((hi - 1, at))
}

/// The local parameter of `t` in `[a, b]` at `like`'s level, exactly 0
/// or 1 at the ends, certified elsewhere.
pub(crate) fn local<S: Scalar>(t: f64, a: f64, b: f64, like: &S) -> S {
    if t == a {
        S::int(0, like)
    } else if t == b {
        S::int(1, like)
    } else {
        let a_s = S::from_f64(a, like);
        S::from_f64(t, like)
            .sub(&a_s)
            .div(&S::from_f64(b, like).sub(&a_s))
    }
}

#[cfg(test)]
mod tests;
