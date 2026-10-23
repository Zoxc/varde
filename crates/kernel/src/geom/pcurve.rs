//! P-curves: a coedge's curve in its face's global UV ([`PCurve`]), a
//! chain of spans over the edge's parameter `t`, broken at every tile
//! line it crosses so that each span lies in one tile.
//!
//! **Span kinds** ([`PSpan`]). Each coordinate of a span is a rational
//! Bézier function of the span's local parameter `s ∈ [0, 1]` of degree
//! 0, 1 or 2, `Σ Bᵢ·wᵢ·xᵢ / Σ Bᵢ·wᵢ`:
//! - **Q**, rational quadratic: control points `(uᵢ, vᵢ)` with shared
//!   weights `wᵢ`, degrees `(2, 2)`;
//! - **G**, graph span: one coordinate a **Möbius** function of `s`
//!   (degree 1, `(a + b·s)/(c + d·s)`, affine for equal weights), the
//!   other rational quadratic with its own weights; degrees `(1, 2)` or
//!   `(2, 1)`. The p-curves of plane ∩ cylinder and plane ∩ cone are G
//!   spans, exactly;
//! - **linear**: a straight UV segment, two control points with shared
//!   weights (a Möbius speed along it), degrees `(1, 1)`;
//! - **iso**: one coordinate fixed, bit for bit, the other a Möbius
//!   function; degrees `(0, 1)` or `(1, 0)`.
//!
//! Every reparametrisation of `t` by a Möbius map, and every Möbius map
//! of one coordinate, keeps G, linear and iso spans of their kind.
//! [`PSpan::degrees`] gives `(k_u, k_v)`, from which a composition with a
//! tile has degree `p·k_u + q·k_v`.
//!
//! **Storage.** As for space curves (the [`curve`](super::curve) module):
//! control values **affine** with their weights, `(x, w)`, never `w·x`; a
//! G or iso span per coordinate, the coordinates' weights apart. So a
//! span passes exactly through its first control values at its start and
//! its last at its end, for any positive weights, and a chain's point at
//! a knot is its control point there ([`PCurve::knot_point`]). A chain is
//! refused unless every value is finite, every weight lies in
//! `[MIN_WEIGHT, MAX_WEIGHT]`, the knots increase strictly and
//! consecutive spans join bit for bit (the UV points, not their weights).
//!
//! **Levels.** [`PCurve::coords_at`] forms each coordinate's homogeneous
//! coefficients `(w·x, w)` at a precision level, as
//! [`HomCurve::from_affine`] does: exact from double-double up, rounded
//! once per product at the first level with the rounding carried.
//!
//! **Canonical points and splits.** [`PCurve::point_at`] is the exact
//! point of the stored span at `t`, rounded to the nearest `f64` once per
//! coordinate; [`PCurve::split`] inserts knots there, the spans between
//! the exact sub-spans (blossoms per coordinate) rounded once per value,
//! as for space curves; a span's kind is kept.
//!
//! **Tile-line splits** (the B-rep's invariant 2(c)).
//! [`PCurve::split_at_tile_lines`] breaks each span where a coordinate
//! crosses an integer, the tile lines of global UV. A crossing is found
//! on the exact span: for a Möbius coordinate (degree 1) it is the
//! inverse of the Möbius map at the line, an exact rational parameter;
//! for a quadratic one a root of the quadratic, rational where it is a
//! double root or where an end lies on the line, else isolated by exact
//! signs. The new knot is the `f64` nearest the crossing's `t` (a knot
//! within half an ulp of a span's end moves one ulp inside), and its
//! control point takes the line's integer **exactly** in the crossing
//! coordinate; the other coordinate is the exact span's there, rounded
//! once. Where a Möbius coordinate crosses, the sub-spans are the exact
//! pieces of the stored span on either side of the exact crossing,
//! rounded once per value, so the UV path is kept to that rounding and
//! only the parameter moves, by under half an ulp of `t`; at a quadratic
//! crossing the pieces meet at the span's point at the knot, moved onto
//! the line. A span whose control values, after the cuts, still don't
//! lie in one tile (a quadratic bulging over a line it doesn't cross) is
//! halved at the middle of its knots until they do, at most
//! [`MAX_TILE_DEPTH`] times. Every span then lies in one tile, and every
//! knot point on a tile line has its integer coordinate exactly.
//! Construction-time geometry, as the chain constructors: not charged;
//! a caller charges the spans gained.

use std::cmp::Ordering;

use malachite::Rational;
use malachite::base::num::arithmetic::traits::Sign;
use varde_decide::bezier::{Dir, HomCurve, Scalar};
use varde_decide::num::bounded::Bounded;

use super::curve::{
    At, CurveError, MAX_WEIGHT, MIN_WEIGHT, affine_rounded, blossom, hom_exact, local, local_exact,
    locate,
};
use super::round::{nearest, q};

/// A Q span: three affine control points with their shared weights,
/// `(u, v, w)`.
pub type Span2 = [[f64; 3]; 3];

/// A Möbius coordinate: two control values with their weights, `(x, w)`.
pub type Mobius = [[f64; 2]; 2];

/// A rational quadratic coordinate: three control values with their
/// weights, `(x, w)`.
pub type Quadratic = [[f64; 2]; 3];

/// The most tile lines one coordinate of one span may cross
/// ([`PCurve::split_at_tile_lines`]).
pub const MAX_TILE_LINES: u64 = 1 << 16;

/// The most halvings of a span the tile-line split makes to bring its
/// control values into one tile ([`PCurve::split_at_tile_lines`]).
pub const MAX_TILE_DEPTH: u32 = 16;

/// Global UV values below this magnitude have exact integer neighbours
/// (`2⁵²`), so their tile lines are exact as `f64`s and as `i64`s.
const LIMIT: f64 = 4_503_599_627_370_496.0;

/// The kind of a span (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Rational quadratic, shared weights.
    Q,
    /// Graph span: one coordinate Möbius, the other rational quadratic.
    G,
    /// A straight UV segment.
    Linear,
    /// One coordinate fixed, the other Möbius.
    Iso,
}

/// A span of a p-curve (module docs), its values affine with their
/// weights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PSpan {
    /// Rational quadratic: `(u, v, w)` per control point.
    Q(Span2),
    /// Graph span: coordinate `mobius` the Möbius function `m`, the
    /// other the rational quadratic `q`.
    G {
        mobius: Dir,
        m: Mobius,
        q: Quadratic,
    },
    /// A straight UV segment: `(u, v, w)` at its two ends.
    Linear([[f64; 3]; 2]),
    /// Coordinate `fixed` held at `at`, the other the Möbius function
    /// `m`.
    Iso { fixed: Dir, at: f64, m: Mobius },
}

/// One coordinate of a span: its control values with their weights,
/// `(x, w)`, one to three of them (degree 0 to 2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coord {
    n: usize,
    c: [[f64; 2]; 3],
}

impl Coord {
    fn new(values: &[[f64; 2]]) -> Coord {
        debug_assert!((1..=3).contains(&values.len()));
        let mut c = [[0.0; 2]; 3];
        c[..values.len()].copy_from_slice(values);
        Coord { n: values.len(), c }
    }

    /// The control values with their weights, `(x, w)`.
    #[inline]
    pub fn values(&self) -> &[[f64; 2]] {
        &self.c[..self.n]
    }

    /// The degree: 0 (fixed), 1 (Möbius) or 2 (quadratic).
    #[inline]
    pub fn degree(&self) -> usize {
        self.n - 1
    }

    /// The least and greatest control value: the coordinate stays
    /// between them (positive weights).
    pub fn range(&self) -> [f64; 2] {
        let mut r = [self.c[0][0]; 2];
        for p in self.values() {
            if p[0] < r[0] {
                r[0] = p[0];
            }
            if p[0] > r[1] {
                r[1] = p[0];
            }
        }
        r
    }

    /// The coordinate in homogeneous form `(X, W)` at `like`'s level
    /// ([`HomCurve::from_affine`]).
    pub fn hom<S: Scalar>(&self, like: &S) -> HomCurve<S, 1> {
        HomCurve::from_affine(self.values().iter().map(|p| ([p[0]], p[1])), like)
            .expect("one to three control values")
    }

    fn first(&self) -> [f64; 2] {
        self.c[0]
    }

    fn last(&self) -> [f64; 2] {
        self.c[self.n - 1]
    }
}

/// The index of a coordinate.
fn ix(d: Dir) -> usize {
    match d {
        Dir::U => 0,
        Dir::V => 1,
    }
}

/// The two coordinates, `fixed` (or the Möbius one) at its index.
fn place<T>(d: Dir, this: T, other: T) -> [T; 2] {
    match d {
        Dir::U => [this, other],
        Dir::V => [other, this],
    }
}

impl PSpan {
    /// The span's kind.
    pub fn kind(&self) -> Kind {
        match self {
            PSpan::Q(_) => Kind::Q,
            PSpan::G { .. } => Kind::G,
            PSpan::Linear(_) => Kind::Linear,
            PSpan::Iso { .. } => Kind::Iso,
        }
    }

    /// The `u` and `v` coordinates.
    pub fn coords(&self) -> [Coord; 2] {
        match self {
            PSpan::Q(s) => [0, 1].map(|k| Coord::new(&s.map(|p| [p[k], p[2]]))),
            PSpan::G { mobius, m, q } => place(*mobius, Coord::new(m), Coord::new(q)),
            PSpan::Linear(s) => [0, 1].map(|k| Coord::new(&s.map(|p| [p[k], p[2]]))),
            PSpan::Iso { fixed, at, m } => place(*fixed, Coord::new(&[[*at, 1.0]]), Coord::new(m)),
        }
    }

    /// The coordinate `d`.
    pub fn coord(&self, d: Dir) -> Coord {
        self.coords()[ix(d)]
    }

    /// The degrees `(k_u, k_v)` of the coordinates.
    pub fn degrees(&self) -> [usize; 2] {
        self.coords().map(|c| c.degree())
    }

    /// The UV point at the span's start: its first control values.
    pub fn start(&self) -> [f64; 2] {
        self.coords().map(|c| c.first()[0])
    }

    /// The UV point at the span's end: its last control values.
    pub fn end(&self) -> [f64; 2] {
        self.coords().map(|c| c.last()[0])
    }

    /// The least and greatest control value of each coordinate,
    /// `[[u_lo, v_lo], [u_hi, v_hi]]`: a box holding the span (positive
    /// weights).
    pub fn hull(&self) -> [[f64; 2]; 2] {
        let [u, v] = self.coords().map(|c| c.range());
        [[u[0], v[0]], [u[1], v[1]]]
    }

    /// The Q span's control points, `None` for another kind.
    pub fn as_q(&self) -> Option<&Span2> {
        match self {
            PSpan::Q(s) => Some(s),
            _ => None,
        }
    }

    /// The span run the other way: its control values reversed.
    pub fn reversed(&self) -> PSpan {
        let r = |c: Coord| {
            let mut v = c.values().to_vec();
            v.reverse();
            Coord::new(&v)
        };
        self.with_coords(self.coords().map(r))
    }

    /// The tile `(i, j)` holding every control value of the span:
    /// `i = ⌊least u⌋` and `j = ⌊least v⌋`, the tile above a line the span
    /// runs on; `None` where the values don't lie in one tile, or lie at
    /// `2⁵²` or beyond in magnitude.
    pub fn tile(&self) -> Option<[i64; 2]> {
        let [u, v] = self.coords().map(|c| {
            let [lo, hi] = c.range();
            // False for a NaN.
            if !(lo.abs() < LIMIT && hi.abs() < LIMIT) {
                return None;
            }
            let i = lo.floor();
            (hi <= i + 1.0).then_some(i as i64)
        });
        Some([u?, v?])
    }

    /// The span of this kind with the coordinates `c`, which have this
    /// span's degrees (and, for a Q or linear span, equal weights).
    fn with_coords(&self, c: [Coord; 2]) -> PSpan {
        let shared = |n: usize| -> Vec<[f64; 3]> {
            debug_assert!(
                (0..n).all(|i| c[0].c[i][1].to_bits() == c[1].c[i][1].to_bits()),
                "shared weights"
            );
            (0..n)
                .map(|i| [c[0].c[i][0], c[1].c[i][0], c[0].c[i][1]])
                .collect()
        };
        match self {
            PSpan::Q(_) => {
                let p = shared(3);
                PSpan::Q([p[0], p[1], p[2]])
            }
            PSpan::Linear(_) => {
                let p = shared(2);
                PSpan::Linear([p[0], p[1]])
            }
            PSpan::G { mobius, .. } => {
                let (m, q) = (c[ix(*mobius)], c[1 - ix(*mobius)]);
                PSpan::G {
                    mobius: *mobius,
                    m: [m.c[0], m.c[1]],
                    q: [q.c[0], q.c[1], q.c[2]],
                }
            }
            PSpan::Iso { fixed, at, .. } => {
                let m = c[1 - ix(*fixed)];
                PSpan::Iso {
                    fixed: *fixed,
                    at: *at,
                    m: [m.c[0], m.c[1]],
                }
            }
        }
    }

    /// The span in homogeneous form over one denominator at `like`'s
    /// level: a Q, linear or iso span's shared weights (an iso span's
    /// fixed value times them), a G span's two denominators multiplied
    /// (degree 3).
    fn hom<S: Scalar>(&self, like: &S) -> HomCurve<S, 2> {
        let joint = |pts: &[([f64; 2], f64)]| {
            HomCurve::from_affine(pts.iter().copied(), like).expect("two or three control points")
        };
        match self {
            PSpan::Q(s) => joint(&s.map(|p| ([p[0], p[1]], p[2]))),
            PSpan::Linear(s) => joint(&s.map(|p| ([p[0], p[1]], p[2]))),
            PSpan::Iso { fixed, at, m } => joint(&m.map(|[x, w]| (place(*fixed, *at, x), w))),
            PSpan::G { .. } => {
                let [u, v] = self.coords().map(|c| c.hom(like));
                let (wu, wv) = (u.weights(), v.weights());
                HomCurve::from_polys(
                    [
                        u.coords()[0].mul(wv).expect("degree 3"),
                        v.coords()[0].mul(wu).expect("degree 3"),
                    ],
                    wu.mul(wv).expect("degree 3"),
                )
                .expect("one degree")
            }
        }
    }

    /// The UV point at local `s` (not an end) at `like`'s level: a Q or
    /// linear span over its shared weights, else per coordinate.
    fn eval<S: Scalar>(&self, s: &S, like: &S) -> [S; 2] {
        match self {
            PSpan::Q(_) | PSpan::Linear(_) => self.hom(like).eval(s),
            _ => self.coords().map(|c| c.hom(like).eval(s)[0].clone()),
        }
    }
}

/// A coedge's curve in its face's global UV: a chain of spans of any kind
/// (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct PCurve {
    spans: Vec<PSpan>,
    knots: Vec<f64>,
}

impl PCurve {
    /// The chain of Q spans `spans`, span `k` over `[knots[k],
    /// knots[k + 1]]` ([`from_spans`](Self::from_spans)).
    pub fn new(spans: Vec<Span2>, knots: Vec<f64>) -> Result<PCurve, CurveError> {
        PCurve::from_spans(spans.into_iter().map(PSpan::Q).collect(), knots)
    }

    /// The chain of `spans`, span `k` over `[knots[k], knots[k + 1]]`,
    /// validated (module docs); else the first offence.
    pub fn from_spans(spans: Vec<PSpan>, knots: Vec<f64>) -> Result<PCurve, CurveError> {
        validate(&spans, &knots)?;
        Ok(PCurve { spans, knots })
    }

    /// This p-curve with each Q span's middle control point moved by its
    /// `offsets` entry in UV, `(u + du, v + dv, w)` in `f64`, the end
    /// control points kept: the curve moves off its image by up to about
    /// the offsets times the surface's speed, while its ends, and so its
    /// joins, stay to the bit. For tests that need ribbons of a given
    /// width. `Shape` unless every span is a Q span and there is one
    /// offset per span; the moved chain validated as by
    /// [`from_spans`](Self::from_spans).
    pub fn perturbed(&self, offsets: &[[f64; 2]]) -> Result<PCurve, CurveError> {
        if offsets.len() != self.spans.len() {
            return Err(CurveError::Shape);
        }
        let spans = self
            .spans
            .iter()
            .zip(offsets)
            .map(|(s, [du, dv])| {
                let mut s = *s.as_q().ok_or(CurveError::Shape)?;
                s[1][0] += du;
                s[1][1] += dv;
                Ok(PSpan::Q(s))
            })
            .collect::<Result<_, _>>()?;
        PCurve::from_spans(spans, self.knots.clone())
    }

    /// The spans.
    #[inline]
    pub fn spans(&self) -> &[PSpan] {
        &self.spans
    }

    /// The spans' control points if every span is a Q span, else `None`.
    pub fn q_spans(&self) -> Option<Vec<Span2>> {
        self.spans.iter().map(|s| s.as_q().copied()).collect()
    }

    /// The knots, one more than the spans.
    #[inline]
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The UV point at knot `k`: the control point there (module docs),
    /// `None` past the last knot.
    pub fn knot_point(&self, k: usize) -> Option<[f64; 2]> {
        match self.spans.get(k) {
            Some(s) => Some(s.start()),
            None if k == self.spans.len() => self.spans.last().map(PSpan::end),
            None => None,
        }
    }

    /// Span `k` at the first level ([`span_at`](Self::span_at)).
    pub fn span(&self, k: usize) -> Option<HomCurve<Bounded<f64>, 2>> {
        self.span_at(k, &Bounded::exact(0.0))
    }

    /// Span `k` in homogeneous form over one denominator at `like`'s
    /// level (and width), its coefficients formed there
    /// ([`HomCurve::from_affine`]): a Q, linear or iso span's
    /// `(w·u, w·v, w)`, a G span's coordinates over the product of their
    /// denominators, of degree 3.
    pub fn span_at<S: Scalar>(&self, k: usize, like: &S) -> Option<HomCurve<S, 2>> {
        self.spans.get(k).map(|s| s.hom(like))
    }

    /// Span `k`'s coordinates in homogeneous form, each `(X, W)` over its
    /// own weights at `like`'s level ([`Coord::hom`]), of the span's
    /// [`degrees`](PSpan::degrees).
    pub fn coords_at<S: Scalar>(&self, k: usize, like: &S) -> Option<[HomCurve<S, 1>; 2]> {
        self.spans.get(k).map(|s| s.coords().map(|c| c.hom(like)))
    }

    /// The UV point at `t`, certified, at the first level
    /// ([`eval_at`](Self::eval_at)).
    pub fn eval(&self, t: f64) -> Option<[Bounded<f64>; 2]> {
        self.eval_at(t, &Bounded::exact(0.0))
    }

    /// The UV point at `t` at `like`'s level, certified, `None` outside
    /// the knots' range: at a knot its control point exactly (the lower
    /// span's end at an inner knot, the same point); else on the span
    /// holding `t`, at the local parameter `(t − knots[k]) / (knots[k + 1]
    /// − knots[k])`.
    pub fn eval_at<S: Scalar>(&self, t: f64, like: &S) -> Option<[S; 2]> {
        let (k, at) = locate(&self.knots, t)?;
        let span = &self.spans[k];
        let end = match at {
            At::Start => span.start(),
            At::End => span.end(),
            At::Inside => {
                let s = local(t, self.knots[k], self.knots[k + 1], like);
                return Some(span.eval(&s, like));
            }
        };
        Some(end.map(|x| S::from_f64(x, like)))
    }

    /// The canonical UV point at `t`: the exact point of the stored span,
    /// rounded to the nearest `f64` per coordinate; at a knot its control
    /// point. `None` outside the knots' range.
    pub fn point_at(&self, t: f64) -> Option<[f64; 2]> {
        let (k, at) = locate(&self.knots, t)?;
        let span = &self.spans[k];
        Some(match at {
            At::Start => span.start(),
            At::End => span.end(),
            At::Inside => {
                let s = local_exact(t, self.knots[k], self.knots[k + 1]);
                span.coords().map(|c| point(&hom_coord(&c), &s)[0])
            }
        })
    }

    /// The chain with knots inserted at `ts` (module docs): those inside
    /// its range and not knots already; the rest are ignored. Each new
    /// knot's control point is the canonical point there
    /// ([`point_at`](Self::point_at)); spans not cut keep their bits. The
    /// new chain validated as by [`from_spans`](Self::from_spans).
    pub fn split(&self, ts: &[f64]) -> Result<PCurve, CurveError> {
        let mut spans = Vec::with_capacity(self.spans.len() + ts.len());
        let mut knots = Vec::with_capacity(self.knots.len() + ts.len());
        knots.push(self.knots[0]);
        for (k, span) in self.spans.iter().enumerate() {
            let (a, b) = (self.knots[k], self.knots[k + 1]);
            let mut inner: Vec<f64> = ts.iter().copied().filter(|&t| a < t && t < b).collect();
            inner.sort_by(f64::total_cmp);
            inner.dedup();
            let cuts: Vec<Cut> = inner
                .iter()
                .map(|&t| Cut {
                    t,
                    s: local_exact(t, a, b),
                    snap: [None; 2],
                })
                .collect();
            spans.extend(cut_span(span, &cuts));
            knots.extend(inner);
            knots.push(b);
        }
        PCurve::from_spans(spans, knots)
    }

    /// The spans between knots `t0 < t1`, `None` unless both are knots.
    pub fn slice(&self, t0: f64, t1: f64) -> Option<PCurve> {
        let i = self.knots.iter().position(|&k| k == t0)?;
        let j = self.knots.iter().position(|&k| k == t1)?;
        (i < j).then(|| PCurve {
            spans: self.spans[i..j].to_vec(),
            knots: self.knots[i..=j].to_vec(),
        })
    }

    /// The chain broken at every tile line it crosses (module docs): each
    /// span in one tile ([`PSpan::tile`]), every knot point on a tile
    /// line with its integer coordinate exactly; spans already in one
    /// tile, and that no line crosses, keep their bits. `Tile` naming
    /// the given span where that can't be done: a value at `2⁵²` or
    /// beyond, more than [`MAX_TILE_LINES`] lines crossed by one
    /// coordinate, two crossings at one `f64` knot that are not one
    /// point, or a span still not in one tile after [`MAX_TILE_DEPTH`]
    /// halvings.
    pub fn split_at_tile_lines(&self) -> Result<PCurve, CurveError> {
        let mut spans = Vec::with_capacity(self.spans.len());
        let mut knots = Vec::with_capacity(self.knots.len());
        knots.push(self.knots[0]);
        for (k, span) in self.spans.iter().enumerate() {
            let fail = CurveError::Tile { span: k };
            let (a, b) = (self.knots[k], self.knots[k + 1]);
            let cuts = tile_cuts(span, a, b).ok_or(fail)?;
            let mut ends = Vec::with_capacity(cuts.len() + 2);
            ends.push(a);
            ends.extend(cuts.iter().map(|c| c.t));
            ends.push(b);
            for (piece, w) in cut_span(span, &cuts).into_iter().zip(ends.windows(2)) {
                into_tiles(piece, w[0], w[1], 0, &mut spans, &mut knots).ok_or(fail)?;
            }
        }
        PCurve::from_spans(spans, knots)
    }

    /// Whether every span lies in one tile ([`PSpan::tile`]), as
    /// [`split_at_tile_lines`](Self::split_at_tile_lines) leaves them.
    pub fn in_tiles(&self) -> bool {
        self.spans.iter().all(|s| s.tile().is_some())
    }
}

/// A cut of a span: the new knot `t`, the span's local parameter `s`
/// whose point it carries, and the integers its coordinates are set to.
struct Cut {
    t: f64,
    s: Rational,
    snap: [Option<i64>; 2],
}

/// A coordinate's control values in homogeneous form, exactly.
fn hom_coord(c: &Coord) -> Vec<[Rational; 2]> {
    c.values().iter().map(hom_exact).collect()
}

/// `(1 − s)·a + s·b`, exactly.
fn lerp(a: &[Rational; 2], b: &[Rational; 2], s: &Rational) -> [Rational; 2] {
    let ms = Rational::from(1u32) - s;
    std::array::from_fn(|k| &ms * &a[k] + s * &b[k])
}

/// The point of a coordinate (homogeneous values `h`) at local `s`,
/// rounded to its stored form `(x, w)`.
fn point(h: &[[Rational; 2]], s: &Rational) -> [f64; 2] {
    match h {
        [p] => affine_rounded(p),
        [a, b] => affine_rounded(&lerp(a, b, s)),
        _ => affine_rounded(&blossom(&[h[0].clone(), h[1].clone(), h[2].clone()], s, s)),
    }
}

/// The span cut at `cuts` (sorted, strictly inside, distinct): the exact
/// sub-spans between the cuts' parameters, rounded once per value, each
/// cut's point the span's there with the snapped coordinates set to
/// their integers; the stored ends kept.
fn cut_span(span: &PSpan, cuts: &[Cut]) -> Vec<PSpan> {
    if cuts.is_empty() {
        return vec![*span];
    }
    let coords = span.coords();
    let h = coords.each_ref().map(hom_coord);
    let mut at: Vec<(Rational, [[f64; 2]; 2])> = Vec::with_capacity(cuts.len() + 2);
    at.push((Rational::from(0u32), coords.map(|c| c.first())));
    for cut in cuts {
        let p = std::array::from_fn(|k| {
            let mut x = point(&h[k], &cut.s);
            if let Some(line) = cut.snap[k] {
                // Below 2⁵² in magnitude: exact.
                x[0] = line as f64;
            }
            x
        });
        at.push((cut.s.clone(), p));
    }
    at.push((Rational::from(1u32), coords.map(|c| c.last())));
    at.windows(2)
        .map(|w| {
            let ((s0, p0), (s1, p1)) = (&w[0], &w[1]);
            let c = std::array::from_fn(|k| match coords[k].degree() {
                0 => coords[k],
                1 => Coord::new(&[p0[k], p1[k]]),
                _ => {
                    let hk = [h[k][0].clone(), h[k][1].clone(), h[k][2].clone()];
                    Coord::new(&[p0[k], affine_rounded(&blossom(&hk, s0, s1)), p1[k]])
                }
            });
            span.with_coords(c)
        })
        .collect()
}

/// `piece` over `[t0, t1]` pushed with its end knot, halved at the middle
/// of its knots while its control values aren't in one tile; `None`
/// past [`MAX_TILE_DEPTH`] halvings or where the knots have no `f64`
/// between them.
fn into_tiles(
    piece: PSpan,
    t0: f64,
    t1: f64,
    depth: u32,
    spans: &mut Vec<PSpan>,
    knots: &mut Vec<f64>,
) -> Option<()> {
    if piece.tile().is_some() {
        spans.push(piece);
        knots.push(t1);
        return Some(());
    }
    // Not `(t0 + t1) / 2`, which may overflow.
    let tm = t0 / 2.0 + t1 / 2.0;
    if depth == MAX_TILE_DEPTH || !(t0 < tm && tm < t1) {
        return None;
    }
    let cut = Cut {
        t: tm,
        s: local_exact(tm, t0, t1),
        snap: [None; 2],
    };
    let halves = cut_span(&piece, &[cut]);
    into_tiles(halves[0], t0, tm, depth + 1, spans, knots)?;
    into_tiles(halves[1], tm, t1, depth + 1, spans, knots)
}

/// A crossing of a coordinate with a tile line, in the span's local
/// parameter.
enum Root {
    /// At this exact parameter.
    Exact(Rational),
    /// The one simple root between these parameters, where the form's
    /// signs are nonzero and opposite.
    Between(Rational, Rational),
}

/// The sign of `x`.
fn sgn(x: &Rational) -> Ordering {
    <Rational as Sign>::sign(x)
}

/// The value of the Bernstein form `f` (degree 0 to 2) at `s`, exactly.
fn bern(f: &[Rational], s: &Rational) -> Rational {
    let ms = Rational::from(1u32) - s;
    match f {
        [a] => a.clone(),
        [a, b] => &ms * a + s * b,
        [a, b, c] => &ms * &ms * a + Rational::from(2u32) * &ms * s * b + s * s * c,
        _ => unreachable!("degree 0 to 2"),
    }
}

/// The roots in `(0, 1)` of the Bernstein form `f` (degree 1 or 2), at
/// which it changes sign or touches zero; none for a form identically
/// zero (a span along the line).
fn roots(f: &[Rational]) -> Vec<Root> {
    let zero = Rational::from(0u32);
    let within = |s: Rational| (s > zero && s < 1u32).then_some(Root::Exact(s));
    match f {
        [a, b] => {
            if sgn(a) == Ordering::Equal || sgn(a) == sgn(b) {
                return Vec::new();
            }
            within(a / (a - b)).into_iter().collect()
        }
        [a, b, c] => {
            let two = Rational::from(2u32);
            let (sa, sc) = (sgn(a), sgn(c));
            let lead = a - &two * b + c;
            if sa == Ordering::Equal && sc == Ordering::Equal {
                // `2s(1 − s)·b`: zero at the ends only, or everywhere.
                return Vec::new();
            }
            if sa == Ordering::Equal {
                // `s·(2b(1 − s) + c·s)`.
                let d = &two * b - c;
                if sgn(&d) == Ordering::Equal {
                    return Vec::new();
                }
                return within(&two * b / d).into_iter().collect();
            }
            if sc == Ordering::Equal {
                // `(1 − s)·(a(1 − s) + 2b·s)`.
                let d = a - &two * b;
                if sgn(&d) == Ordering::Equal {
                    return Vec::new();
                }
                return within(a / d).into_iter().collect();
            }
            if sgn(&lead) == Ordering::Equal {
                // Linear in power form: `a + 2(b − a)·s`.
                let d = &two * (a - b);
                if sgn(&d) == Ordering::Equal {
                    return Vec::new();
                }
                return within(a / d).into_iter().collect();
            }
            if sa != sc {
                return vec![Root::Between(zero, Rational::from(1u32))];
            }
            // The same sign at both ends: none or two roots, about the
            // vertex.
            let sv = (a - b) / &lead;
            if !(sv > zero && sv < 1u32) {
                return Vec::new();
            }
            match sgn(&bern(f, &sv)) {
                Ordering::Equal => vec![Root::Exact(sv)],
                s if s == sa => Vec::new(),
                _ => vec![
                    Root::Between(zero, sv.clone()),
                    Root::Between(sv, Rational::from(1u32)),
                ],
            }
        }
        _ => Vec::new(),
    }
}

/// `t` moved one ulp inside `(a, b)` if it is an end; `None` if there is
/// no `f64` strictly between.
fn inside(t: f64, a: f64, b: f64) -> Option<f64> {
    let t = if t <= a {
        a.next_up()
    } else if t >= b {
        b.next_down()
    } else {
        t
    };
    (a < t && t < b).then_some(t)
}

/// `x`'s position in the order of the finite `f64`s, `−0` as `+0`.
fn order_key(x: f64) -> i64 {
    let b = x.to_bits() as i64;
    if b < 0 { i64::MIN - b } else { b }
}

/// The `f64` at position `k` of [`order_key`]'s order.
fn from_key(k: i64) -> f64 {
    if k < 0 {
        f64::from_bits((i64::MIN - k) as u64)
    } else {
        f64::from_bits(k as u64)
    }
}

/// The cuts where span `span` over `[a, b]` crosses tile lines (module
/// docs), sorted, those at one parameter merged; `None` where they can't
/// be made (as [`PCurve::split_at_tile_lines`]).
fn tile_cuts(span: &PSpan, a: f64, b: f64) -> Option<Vec<Cut>> {
    let mut cuts: Vec<Cut> = Vec::new();
    for (k, c) in span.coords().iter().enumerate() {
        if c.degree() == 0 {
            continue;
        }
        let [lo, hi] = c.range();
        // False for a NaN.
        if !(lo.abs() < LIMIT && hi.abs() < LIMIT) {
            return None;
        }
        // The lines strictly between the least and greatest value: one at
        // either can only be touched where the coordinate is constant.
        let (first, last) = (lo.floor() as i64 + 1, hi.ceil() as i64 - 1);
        if first > last {
            continue;
        }
        if last.abs_diff(first) >= MAX_TILE_LINES {
            return None;
        }
        for line in first..=last {
            let l = Rational::from(line);
            let f: Vec<Rational> = c
                .values()
                .iter()
                .map(|p| (q(p[0]) - &l) * q(p[1]))
                .collect();
            for root in roots(&f) {
                let mut snap = [None; 2];
                snap[k] = Some(line);
                cuts.push(cut_at(&root, &f, a, b, snap)?);
            }
        }
    }
    cuts.sort_by(|x, y| x.s.cmp(&y.s));
    let mut merged: Vec<Cut> = Vec::with_capacity(cuts.len());
    for cut in cuts {
        match merged.last_mut() {
            Some(m) if m.s == cut.s => {
                for k in 0..2 {
                    m.snap[k] = m.snap[k].or(cut.snap[k]);
                }
            }
            Some(m) if m.t >= cut.t => return None,
            _ => merged.push(cut),
        }
    }
    Some(merged)
}

/// The cut at a root of `f` (a coordinate less its line, times the
/// weights) on a span over `[a, b]` (module docs); `None` where its knot
/// can't be placed strictly inside.
fn cut_at(root: &Root, f: &[Rational], a: f64, b: f64, snap: [Option<i64>; 2]) -> Option<Cut> {
    let (qa, qb) = (q(a), q(b));
    let t_of = |s: &Rational| &qa + s * (&qb - &qa);
    match root {
        Root::Exact(s) => Some(Cut {
            t: inside(nearest(&t_of(s)), a, b)?,
            s: s.clone(),
            snap,
        }),
        Root::Between(s0, s1) => {
            let sign = |t: f64| sgn(&bern(f, &local_exact(t, a, b)));
            let end = |s: &Rational, t: f64| {
                if *s == 0u32 {
                    a
                } else if *s == 1u32 {
                    b
                } else {
                    t
                }
            };
            let (lo_s, hi_s) = (sgn(&bern(f, s0)), sgn(&bern(f, s1)));
            let mut lo = end(s0, nearest(&t_of(s0)));
            let mut hi = end(s1, nearest(&t_of(s1)));
            // A rounded bracket end on the wrong side: the two roots
            // within its rounding.
            if !(lo < hi && sign(lo) == lo_s && sign(hi) == hi_s) {
                return None;
            }
            // Bisect in the order of the `f64`s to adjacent ones.
            let mut exact = None;
            loop {
                let (kl, kh) = (i128::from(order_key(lo)), i128::from(order_key(hi)));
                if kh - kl <= 1 {
                    break;
                }
                // Between `kl` and `kh`, so an `i64`.
                let m = from_key(((kl + kh) >> 1) as i64);
                match sign(m) {
                    Ordering::Equal => {
                        exact = Some(m);
                        break;
                    }
                    s if s == lo_s => lo = m,
                    _ => hi = m,
                }
            }
            let t = exact.unwrap_or_else(|| {
                // The nearer of the two to the root, by the sign at their
                // midpoint; a tie to the even one.
                let m = (q(lo) + q(hi)) / Rational::from(2u32);
                match sgn(&bern(f, &((m - &qa) / (&qb - &qa)))) {
                    Ordering::Equal if lo.to_bits() & 1 == 0 => lo,
                    Ordering::Equal => hi,
                    s if s == lo_s => hi,
                    _ => lo,
                }
            });
            let t = inside(t, a, b)?;
            Some(Cut {
                t,
                s: local_exact(t, a, b),
                snap,
            })
        }
    }
}

fn validate(spans: &[PSpan], knots: &[f64]) -> Result<(), CurveError> {
    if spans.is_empty() || knots.len() != spans.len() + 1 {
        return Err(CurveError::Shape);
    }
    for (span, s) in spans.iter().enumerate() {
        let coords = s.coords();
        let values = || coords.iter().flat_map(|c| c.values());
        if !values().flatten().all(|x| x.is_finite()) {
            return Err(CurveError::NotFinite { span });
        }
        if !values().all(|p| (MIN_WEIGHT..=MAX_WEIGHT).contains(&p[1])) {
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
        let (a, b) = (spans[span - 1].end(), spans[span].start());
        if a.iter().zip(&b).any(|(x, y)| x.to_bits() != y.to_bits()) {
            return Err(CurveError::Join { span });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
