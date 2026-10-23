//! Conics and their spans: ellipses (circles among them), parabolas and
//! hyperbolas in a plane of space or of UV, and the rational quadratic
//! spans that are exactly their arcs, stored as [`super::curve`] stores
//! them: three control points **affine** with their weights.
//!
//! **Conics.** A [`Conic`] is its kind, a centre `c` (a parabola's
//! vertex), two linearly independent axes `e₁`, `e₂` and two positive
//! scales `ρ₁`, `ρ₂`, all stored `f64`s; with `a = ρ₁·e₁` and
//! `b = ρ₂·e₂`, exactly:
//! - ellipse: `c + cos θ·a + sin θ·b` (`a`, `b` conjugate semi-diameters;
//!   a circle of radius `ρ` for orthonormal axes and `ρ₁ = ρ₂ = ρ`);
//! - parabola: `c + t·a + t²·b`, `t` real ([`Conic::parabola`] takes a
//!   focal length `f` as `ρ₁ = 2f`, `ρ₂ = f`: `y = x²/4f` in an
//!   orthonormal frame);
//! - hyperbola: `c + ½(s + 1/s)·a + ½(s − 1/s)·b`, `s > 0` (`cosh u` and
//!   `sinh u` at `s = eᵘ`): the branch through `c + a`, `b` along it
//!   there, its asymptotes `a ± b`.
//!
//! **Spans.** Every value below is an exact function of the stored
//! inputs, rounded once to the nearest `f64`, ties to even: each
//! coordinate of an affine control point, and each weight.
//! - An ellipse's **90° cells** ([`Conic::quarter`]): cell `k` runs from
//!   angle `k·90°` to `(k + 1)·90°`, its control points the radii's ends
//!   `c + r_k`, `c + r_{k+1}` (`r` cycling through `a`, `b`, `−a`, `−b`)
//!   and their tangents' corner `c + r_k + r_{k+1}`, weights `1, √½, 1`
//!   (`√½` rounded). Cells `k` and `k + 4` are the same bits, and
//!   neighbouring cells share their end point, weight 1. On a circle in
//!   a cylinder's or sphere's frame these are the lattice's quarter
//!   circles, bit for bit ([`super::lattice`]).
//! - A parabola's or hyperbola's arc over a parameter interval
//!   ([`Conic::arc`]): the span is the exact rational quadratic over
//!   `[t₀, t₁]`, its Bézier parameter linear in `t` (or `s`). A
//!   parabola's weights are 1. A hyperbola's are the blossom of its
//!   denominator `s`, `s₀, (s₀ + s₁)/2, s₁`, divided by the power of two
//!   that puts the exact middle weight in `[1, 2)` (a homogeneous scale:
//!   the same curve and parametrisation, the points' bits unchanged);
//!   they must lie in `[MIN_WEIGHT, MAX_WEIGHT]`, so `s₁/s₀` is at most
//!   128 to 256.
//!
//! **½ splits.** The span `j` of level `L` of a stored span is its exact
//! sub-span over `[j, j + 1]·2^−L` ([`sub_span`]): the blossom of the
//! stored span's exact homogeneous control points, rounded once. It is
//! not reparametrised: its end weights are the stored span's
//! denominator at its ends, `W(a)` and `W(b)`, not 1. So neighbours at
//! one level share their end point and weight bit for bit, and their
//! knots carry the same bits as [`Curve3D::split`](super::curve::Curve3D::split)
//! of the stored span there. Weights stay in the stored span's range (a
//! sub-span's are convex combinations of its), so a split never fails.
//! [`Conic::quarter_at`] gives an ellipse's cells' splits.
//!
//! **Implicits.** [`Conic::implicit`] is the conic's quadratic equation
//! in the conic's own coordinates `(α, β)` of `x − c` along `a`, `b`
//! (`α² + β² = 1`, `β = α²`, `α² − β² = 1`), scaled by the Gram
//! determinant `g` of `a`, `b` to stay polynomial; in space its zero set
//! is the right cylinder over the conic, cut by the conic's plane
//! ([`Conic::plane`]) in the conic. The stored spans lie on it to within
//! their rounding (the module's tests check that, exactly).
//!
//! Rational quadratic conic arcs: the NURBS Book (Piegl and Tiller,
//! 2nd ed.), §7.2–7.3 and A7.1.

use std::f64::consts::FRAC_1_SQRT_2;

use malachite::Rational;
use malachite::base::num::basic::traits::{One, Zero};
use varde_decide::bezier::Scalar;

use super::curve::{MAX_WEIGHT, MIN_WEIGHT, affine_rounded, blossom, hom_exact};
use super::lattice::MAX_LEVEL;
use super::round::{nearest, q};

/// A conic's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConicKind {
    Ellipse,
    Parabola,
    Hyperbola,
}

/// Why a conic or span can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConicError {
    /// An input is not finite.
    NotFinite,
    /// A scale is not positive.
    Scale,
    /// The two axes are linearly dependent (or one is zero).
    Axes,
    /// The method doesn't apply to the conic's kind (cells of a parabola
    /// or hyperbola, an arc of an ellipse).
    Kind,
    /// An arc's interval is not increasing, or a hyperbola's starts at
    /// or below 0.
    Parameter,
    /// A hyperbola arc's weights don't fit `[MIN_WEIGHT, MAX_WEIGHT]`.
    Weight,
    /// A control point's coordinate, rounded, is not finite.
    Range,
    /// A level above [`MAX_LEVEL`], or a sub-span index outside
    /// `[0, 2^L)`.
    Level,
}

/// A conic in `ℝᴺ` (module docs): its kind, centre (or vertex), axes and
/// scales, as given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conic<const N: usize> {
    kind: ConicKind,
    centre: [f64; N],
    axes: [[f64; N]; 2],
    scales: [f64; 2],
}

/// A span of a conic in `ℝᴺ`, `D = N + 1`: three affine control points
/// with their weights ([`super::curve::Span3`], [`super::pcurve::Span2`]).
pub type Span<const D: usize> = [[f64; D]; 3];

impl<const N: usize> Conic<N> {
    /// The conic of `kind` with centre (a parabola's vertex) `centre`,
    /// axes `e₁`, `e₂` and scales `ρ₁`, `ρ₂` (module docs). Refused unless
    /// every input is finite, both scales are positive and the axes are
    /// linearly independent, exactly.
    pub fn new(
        kind: ConicKind,
        centre: [f64; N],
        axes: [[f64; N]; 2],
        scales: [f64; 2],
    ) -> Result<Conic<N>, ConicError> {
        const { assert!(N >= 2, "a conic lies in a plane") };
        if !centre
            .iter()
            .chain(axes.iter().flatten())
            .chain(&scales)
            .all(|x| x.is_finite())
        {
            return Err(ConicError::NotFinite);
        }
        if !scales.iter().all(|&r| r > 0.0) {
            return Err(ConicError::Scale);
        }
        let [e1, e2] = axes.each_ref().map(|e| e.map(q));
        let (aa, ab, bb) = (dot(&e1, &e1), dot(&e1, &e2), dot(&e2, &e2));
        if aa * bb - &ab * &ab <= Rational::ZERO {
            return Err(ConicError::Axes);
        }
        Ok(Conic {
            kind,
            centre,
            axes,
            scales,
        })
    }

    /// The ellipse `c + cos θ·ρ₁e₁ + sin θ·ρ₂e₂`.
    pub fn ellipse(
        centre: [f64; N],
        axes: [[f64; N]; 2],
        scales: [f64; 2],
    ) -> Result<Conic<N>, ConicError> {
        Conic::new(ConicKind::Ellipse, centre, axes, scales)
    }

    /// The circle of radius `radius` about `centre` in the plane of the
    /// orthonormal axes `e₁`, `e₂` (orthonormal as the caller's frame
    /// stores them: the conic is the ellipse on the stored axes).
    pub fn circle(
        centre: [f64; N],
        axes: [[f64; N]; 2],
        radius: f64,
    ) -> Result<Conic<N>, ConicError> {
        Conic::ellipse(centre, axes, [radius, radius])
    }

    /// The parabola with vertex `vertex`, tangent there along `e₁` and
    /// opening along `e₂`, focal length `focal`: `vertex + 2f·t·e₁ +
    /// f·t²·e₂` (`2f` exact).
    pub fn parabola(
        vertex: [f64; N],
        axes: [[f64; N]; 2],
        focal: f64,
    ) -> Result<Conic<N>, ConicError> {
        Conic::new(ConicKind::Parabola, vertex, axes, [2.0 * focal, focal])
    }

    /// The hyperbola branch `c + cosh u·ρ₁e₁ + sinh u·ρ₂e₂`, parameter
    /// `s = eᵘ`.
    pub fn hyperbola(
        centre: [f64; N],
        axes: [[f64; N]; 2],
        scales: [f64; 2],
    ) -> Result<Conic<N>, ConicError> {
        Conic::new(ConicKind::Hyperbola, centre, axes, scales)
    }

    /// The kind.
    #[inline]
    pub fn kind(&self) -> ConicKind {
        self.kind
    }

    /// The centre (a parabola's vertex).
    #[inline]
    pub fn centre(&self) -> [f64; N] {
        self.centre
    }

    /// The axes `e₁`, `e₂`.
    #[inline]
    pub fn axes(&self) -> [[f64; N]; 2] {
        self.axes
    }

    /// The scales `ρ₁`, `ρ₂`.
    #[inline]
    pub fn scales(&self) -> [f64; 2] {
        self.scales
    }

    /// An ellipse's 90° cell `k` (module docs); `Kind` for another conic,
    /// `Range` where a coordinate rounds past the range.
    pub fn quarter<const D: usize>(&self, k: i64) -> Result<Span<D>, ConicError> {
        const { assert!(D == N + 1, "a span holds a point and its weight") };
        if self.kind != ConicKind::Ellipse {
            return Err(ConicError::Kind);
        }
        let (c, [a, b]) = self.exact();
        let neg = |x: &[Rational; N]| x.clone().map(|v| -v);
        let radii = [a.clone(), b.clone(), neg(&a), neg(&b)];
        let m = k.rem_euclid(4) as usize;
        let (r0, r1) = (&radii[m], &radii[(m + 1) % 4]);
        let at = |r: &[Rational; N]| -> [Rational; N] { std::array::from_fn(|i| &c[i] + &r[i]) };
        let corner: [Rational; N] = std::array::from_fn(|i| &c[i] + &r0[i] + &r1[i]);
        checked([
            stored(&at(r0), 1.0),
            stored(&corner, FRAC_1_SQRT_2),
            stored(&at(r1), 1.0),
        ])
    }

    /// Span `i` of an ellipse's cells at level `L`: the sub-span
    /// `i mod 2^L` ([`sub_span`]) of cell `⌊i / 2^L⌋` (so `i` and
    /// `i + 4·2^L` are the same bits). Errors as [`quarter`](Self::quarter)
    /// and `Level` past [`MAX_LEVEL`].
    pub fn quarter_at<const D: usize>(&self, level: u32, i: i64) -> Result<Span<D>, ConicError> {
        if level > MAX_LEVEL {
            return Err(ConicError::Level);
        }
        let cell = self.quarter::<D>(i >> level)?;
        // In `[0, 2^L)`: the low `L` bits.
        let j = (i & ((1i64 << level) - 1)) as u64;
        sub_span(&cell, level, j)
    }

    /// A parabola's or hyperbola's arc over `[t0, t1]` (`s` for a
    /// hyperbola; module docs). `Kind` for an ellipse; `Parameter` unless
    /// `t0 < t1`, both finite (and `t0 > 0` on a hyperbola); `Weight`
    /// where a hyperbola's weights don't fit; `Range` where a coordinate
    /// rounds past the range.
    pub fn arc<const D: usize>(&self, t0: f64, t1: f64) -> Result<Span<D>, ConicError> {
        const { assert!(D == N + 1, "a span holds a point and its weight") };
        // False for a NaN.
        if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
            return Err(ConicError::Parameter);
        }
        let (c, [a, b]) = self.exact();
        let (x, y) = (q(t0), q(t1));
        let half = Rational::from_signeds(1, 2);
        // The blossom's arguments of the three control points.
        let args = [(&x, &x), (&x, &y), (&y, &y)];
        match self.kind {
            ConicKind::Ellipse => Err(ConicError::Kind),
            ConicKind::Parabola => {
                // c + ((u + v)/2)·a + uv·b.
                checked(args.map(|(u, v)| {
                    let m = (u + v) * &half;
                    let uv = u * v;
                    let p: [Rational; N] =
                        std::array::from_fn(|i| &c[i] + &m * &a[i] + &uv * &b[i]);
                    stored(&p, 1.0)
                }))
            }
            ConicKind::Hyperbola => {
                if t0 <= 0.0 {
                    return Err(ConicError::Parameter);
                }
                // Homogeneous: W = (u + v)/2, numerator
                // c·W + ½(uv + 1)·a + ½(uv − 1)·b.
                let hom = args.map(|(u, v)| {
                    let w = (u + v) * &half;
                    let uv = u * v;
                    let (ka, kb) = ((&uv + Rational::ONE) * &half, (uv - Rational::ONE) * &half);
                    let p: [Rational; N] =
                        std::array::from_fn(|i| (&c[i] * &w + &ka * &a[i] + &kb * &b[i]) / &w);
                    (p, w)
                });
                // The power of two that puts the exact middle weight in
                // [1, 2): exact on every weight.
                let scale = pow2(-hom[1].1.floor_log_base_2_abs());
                let span = hom.map(|(p, w)| stored(&p, nearest(&(w * &scale))));
                if !span
                    .iter()
                    .all(|p| (MIN_WEIGHT..=MAX_WEIGHT).contains(&p[D - 1]))
                {
                    return Err(ConicError::Weight);
                }
                checked(span)
            }
        }
    }

    /// The conic's implicit polynomial at `p`, at `p`'s level (exactly
    /// for exact `p`), in the stored values: with `d = p − c`,
    /// `g = |a|²|b|² − (a·b)²`, `α' = |b|²(a·d) − (a·b)(b·d)` and
    /// `β' = |a|²(b·d) − (a·b)(a·d)` (`g` times the coordinates of `d`'s
    /// projection on the conic's plane along `a`, `b`):
    /// - ellipse: `α'² + β'² − g²`;
    /// - parabola: `α'² − g·β'`;
    /// - hyperbola: `α'² − β'² − g²`.
    ///
    /// Degree 2 in `p`. Zero on the conic (in space: on the right
    /// cylinder over it); negative inside an ellipse, on a parabola's
    /// focus side and between a hyperbola's branches.
    pub fn implicit<S: Scalar>(&self, p: &[S; N]) -> S {
        let like = &p[0];
        let [a, b] = [0, 1].map(|k| self.axes[k].map(|x| S::product(self.scales[k], x, like)));
        let d: [S; N] = std::array::from_fn(|i| p[i].sub(&S::from_f64(self.centre[i], like)));
        let (aa, ab, bb) = (sdot(&a, &a), sdot(&a, &b), sdot(&b, &b));
        let (ad, bd) = (sdot(&a, &d), sdot(&b, &d));
        let g = aa.mul(&bb).sub(&ab.mul(&ab));
        let alpha = bb.mul(&ad).sub(&ab.mul(&bd));
        let beta = aa.mul(&bd).sub(&ab.mul(&ad));
        let a2 = alpha.mul(&alpha);
        match self.kind {
            ConicKind::Ellipse => a2.add(&beta.mul(&beta)).sub(&g.mul(&g)),
            ConicKind::Parabola => a2.sub(&g.mul(&beta)),
            ConicKind::Hyperbola => a2.sub(&beta.mul(&beta)).sub(&g.mul(&g)),
        }
    }

    /// The degree of [`implicit`](Self::implicit): 2.
    #[inline]
    pub fn implicit_degree(&self) -> u32 {
        2
    }

    /// The centre and `a = ρ₁e₁`, `b = ρ₂e₂`, exactly.
    fn exact(&self) -> ([Rational; N], [[Rational; N]; 2]) {
        let c = self.centre.map(q);
        let ab = [0, 1].map(|k| {
            let r = q(self.scales[k]);
            self.axes[k].map(|x| &r * q(x))
        });
        (c, ab)
    }
}

impl Conic<3> {
    /// The conic's plane at `p`: `(p − c)·(e₁ × e₂)`, at `p`'s level, in
    /// the stored values. Zero on the plane; with
    /// [`implicit`](Self::implicit) zero, on the conic.
    pub fn plane<S: Scalar>(&self, p: &[S; 3]) -> S {
        let like = &p[0];
        let [e1, e2] = self.axes;
        let n = [
            S::product(e1[1], e2[2], like).sub(&S::product(e1[2], e2[1], like)),
            S::product(e1[2], e2[0], like).sub(&S::product(e1[0], e2[2], like)),
            S::product(e1[0], e2[1], like).sub(&S::product(e1[1], e2[0], like)),
        ];
        let d: [S; 3] = std::array::from_fn(|i| p[i].sub(&S::from_f64(self.centre[i], like)));
        sdot(&n, &d)
    }
}

/// Span `j` of level `level` of the stored `span`: its exact sub-span
/// over `[j, j + 1]·2^−level`, rounded once (module docs). `Level` past
/// [`MAX_LEVEL`] or for `j ≥ 2^level`. A span with finite values and
/// weights in `[MIN_WEIGHT, MAX_WEIGHT]` gives one, every value a convex
/// combination of its.
pub fn sub_span<const D: usize>(span: &Span<D>, level: u32, j: u64) -> Result<Span<D>, ConicError> {
    if level > MAX_LEVEL || j >> level != 0 {
        return Err(ConicError::Level);
    }
    if level == 0 {
        return Ok(*span);
    }
    let h = span.each_ref().map(hom_exact);
    let den = pow2(-i64::from(level));
    let s0 = Rational::from(j) * &den;
    let s1 = Rational::from(j + 1) * &den;
    Ok([
        affine_rounded(&blossom(&h, &s0, &s0)),
        affine_rounded(&blossom(&h, &s0, &s1)),
        affine_rounded(&blossom(&h, &s1, &s1)),
    ])
}

/// The affine point `p` rounded once, with the weight `w`.
fn stored<const N: usize, const D: usize>(p: &[Rational; N], w: f64) -> [f64; D] {
    std::array::from_fn(|i| if i < N { nearest(&p[i]) } else { w })
}

/// `span`, unless a value rounded past the range.
fn checked<const D: usize>(span: Span<D>) -> Result<Span<D>, ConicError> {
    if span.iter().flatten().all(|x| x.is_finite()) {
        Ok(span)
    } else {
        Err(ConicError::Range)
    }
}

/// `2^e` exactly.
fn pow2(e: i64) -> Rational {
    Rational::ONE << e
}

fn dot<const N: usize>(a: &[Rational; N], b: &[Rational; N]) -> Rational {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn sdot<S: Scalar, const N: usize>(a: &[S; N], b: &[S; N]) -> S {
    let mut s = a[0].mul(&b[0]);
    for i in 1..N {
        s = s.add(&a[i].mul(&b[i]));
    }
    s
}

#[cfg(test)]
mod tests;
