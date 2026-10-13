//! Curves: an edge's space curve ([`Curve3D`]) and a coedge's curve in
//! its face's global UV ([`PCurve`]), each a chain of rational quadratic
//! spans over a parameter `t` that runs through the chain's knots.
//!
//! **Storage.** A span is its three homogeneous control points, `(w·x,
//! w·y, w·z, w)` in space and `(w·u, w·v, w)` in UV, exactly as stored.
//! Span `k` covers `[knots[k], knots[k + 1]]` of `t`, mapped linearly to
//! its own `[0, 1]`. A chain is refused unless every value is finite,
//! every weight lies in `[MIN_WEIGHT, MAX_WEIGHT]` = `[1/64, 64]`, each
//! span's end points projected (below) are finite too, the knots (one
//! more than the spans) increase strictly, and consecutive
//! spans join **bit for bit**: the end point of span `k` and the start
//! point of span `k + 1` are the same `f64`s.
//!
//! **Points at knots.** The point of a span's end is its end control
//! point projected, `w·x / w` rounded once per coordinate (exact for a
//! weight of 1). Those values are the chain's points at its knots
//! ([`Curve3D::knot_point`], [`PCurve::knot_point`]), which joins compare
//! bit for bit: between spans here, between an edge's curve and its
//! vertices, between consecutive coedges' p-curves. Certified evaluation
//! at an end gives the same value, the projection's rounding its error.
//!
//! A p-curve so far is Q spans only, rational quadratics in global UV;
//! linear and iso spans are Q spans with their middle control point on
//! the chord.

use varde_decide::bezier::HomCurve;
use varde_decide::num::bounded::Bounded;

/// The least weight of a span's control point.
pub const MIN_WEIGHT: f64 = 1.0 / 64.0;

/// The greatest weight of a span's control point.
pub const MAX_WEIGHT: f64 = 64.0;

/// A space span: three homogeneous control points `(w·x, w·y, w·z, w)`.
pub type Span3 = [[f64; 4]; 3];

/// A UV span: three homogeneous control points `(w·u, w·v, w)`.
pub type Span2 = [[f64; 3]; 3];

/// Why a chain can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveError {
    /// No spans, or not one knot more than spans.
    Shape,
    /// A value of span `span`, or one of its end points projected, is
    /// not finite.
    NotFinite { span: usize },
    /// A weight of span `span` lies outside `[MIN_WEIGHT, MAX_WEIGHT]`.
    Weight { span: usize },
    /// A knot is not finite, or knot `knot` is not above the one before.
    Knots { knot: usize },
    /// Span `span` doesn't start where span `span − 1` ends, bit for bit.
    Join { span: usize },
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

    /// The point at knot `k` (module docs), `None` past the last knot.
    pub fn knot_point(&self, k: usize) -> Option<[f64; 3]> {
        let p = knot_hom(&self.spans, k)?;
        Some([p[0] / p[3], p[1] / p[3], p[2] / p[3]])
    }

    /// Span `k` at the first level, the stored values exact.
    pub fn span(&self, k: usize) -> Option<HomCurve<Bounded<f64>, 3>> {
        self.spans.get(k).map(hom3)
    }

    /// The point at `t`, certified, `None` outside the knots' range: on
    /// the span holding `t` (the lower one at a knot), at the local
    /// parameter `(t − knots[k]) / (knots[k + 1] − knots[k])`, exactly 0
    /// or 1 at the span's ends.
    pub fn eval(&self, t: f64) -> Option<[Bounded<f64>; 3]> {
        let (k, s) = locate(&self.knots, t)?;
        Some(hom3(&self.spans[k]).eval(&s))
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
    /// `offsets` entry in UV, `(w·u + du·w, w·v + dv·w, w)` in `f64`, the
    /// end control points kept: the curve moves off
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
                let w = s[1][2];
                s[1][0] += du * w;
                s[1][1] += dv * w;
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

    /// The UV point at knot `k` (module docs), `None` past the last knot.
    pub fn knot_point(&self, k: usize) -> Option<[f64; 2]> {
        let p = knot_hom(&self.spans, k)?;
        Some([p[0] / p[2], p[1] / p[2]])
    }

    /// Span `k` at the first level, the stored values exact.
    pub fn span(&self, k: usize) -> Option<HomCurve<Bounded<f64>, 2>> {
        self.spans.get(k).map(hom2)
    }

    /// The UV point at `t`, certified, `None` outside the knots' range
    /// (as [`Curve3D::eval`]).
    pub fn eval(&self, t: f64) -> Option<[Bounded<f64>; 2]> {
        let (k, s) = locate(&self.knots, t)?;
        Some(hom2(&self.spans[k]).eval(&s))
    }
}

fn hom3(s: &Span3) -> HomCurve<Bounded<f64>, 3> {
    HomCurve::new(
        s.iter()
            .map(|p| ([p[0], p[1], p[2]].map(Bounded::exact), Bounded::exact(p[3]))),
    )
    .expect("three control points")
}

fn hom2(s: &Span2) -> HomCurve<Bounded<f64>, 2> {
    HomCurve::new(
        s.iter()
            .map(|p| ([p[0], p[1]].map(Bounded::exact), Bounded::exact(p[2]))),
    )
    .expect("three control points")
}

/// The homogeneous control point at knot `k`: span `k`'s first, or the
/// last span's last.
fn knot_hom<const D: usize>(spans: &[[[f64; D]; 3]], k: usize) -> Option<[f64; D]> {
    match spans.get(k) {
        Some(s) => Some(s[0]),
        None if k == spans.len() => spans.last().map(|s| s[2]),
        None => None,
    }
}

/// The projected point of a homogeneous control point, its weight last.
fn project<const D: usize>(p: &[f64; D]) -> [f64; D] {
    let w = p[D - 1];
    std::array::from_fn(|k| if k + 1 == D { 1.0 } else { p[k] / w })
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
        // An end point projected past the range would be `±∞`, and two
        // different ones would compare equal.
        if ![s[0], s[2]].iter().flat_map(project).all(f64::is_finite) {
            return Err(CurveError::NotFinite { span });
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
        let a = project(&spans[span - 1][2]);
        let b = project(&spans[span][0]);
        if a.iter().zip(&b).any(|(x, y)| x.to_bits() != y.to_bits()) {
            return Err(CurveError::Join { span });
        }
    }
    Ok(())
}

/// The span holding `t` (the lower one at an inner knot) and the local
/// parameter, exactly 0 or 1 at the span's ends; `None` outside the
/// knots' range.
fn locate(knots: &[f64], t: f64) -> Option<(usize, Bounded<f64>)> {
    let (first, last) = (knots[0], knots[knots.len() - 1]);
    // False for a NaN.
    if !(first <= t && t <= last) {
        return None;
    }
    // The first knot at or above `t`, at least 1.
    let hi = knots.partition_point(|&k| k < t).max(1);
    Some((hi - 1, local(t, knots[hi - 1], knots[hi])))
}

/// The local parameter of `t` in `[a, b]`, exactly 0 or 1 at the ends,
/// certified elsewhere.
pub(crate) fn local(t: f64, a: f64, b: f64) -> Bounded<f64> {
    if t == a {
        Bounded::exact(0.0)
    } else if t == b {
        Bounded::exact(1.0)
    } else {
        (Bounded::exact(t) - Bounded::exact(a)) / (Bounded::exact(b) - Bounded::exact(a))
    }
}
