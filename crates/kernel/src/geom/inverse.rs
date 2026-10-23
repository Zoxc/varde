//! The **analytic inverse** of a form's surface: from a point to its
//! global UV ([`Surface::inverse`]), in closed form, with no angle
//! function: the half-angle tangent and a Möbius map around circles,
//! barycentric ratios along conic spans, projections along lines. With
//! the point, a certified upper bound on its distance from the surface's
//! point at that UV.
//!
//! **Lattice coordinates.** The inverse works in the base level's global
//! coordinates, cell `k` owning `[k, k + 1]`, from the lattice frame's
//! stored origin `o` and axes `e₁, e₂, a` ([`super::lattice`]), `y = x −
//! o`, `ξ = e₁·y`, `η = e₂·y`, `z = a·y`, `ρ = √(ξ² + η²)`:
//! - plane: `(e₁·y, e₂·y)/CELL + ½` (cell `(0, 0)` centred on the
//!   origin);
//! - around an axis (cylinder, cone, sphere, torus, revolved conic): the
//!   angle of `(ξ, η)` in quarter turns ([`quarter_turns`]); on the axis
//!   (`ξ = η = 0`, a pole or apex row, any angle the same point) the
//!   middle of the surface's angle range;
//! - along the axis: the cylinder's `z/CELL + ½`; the cone's slant
//!   distance `(z·cos + ρ·sin)/CELL`, the foot of the point's meridian
//!   projection on the generator;
//! - meridian circles: the sphere's latitude, the angle of `(ρ, z)` in
//!   quarter turns from the equator; a ring torus's tube angle, that of
//!   `(ρ − R, z)` from the outer equator; a spindle torus's tube arcs, the
//!   conic spans of its apple (below), cell 0 where `z ≥ 0`, else −1;
//! - along a profile (conic-chain cylinder in `(ξ, η)`, revolved conic in
//!   `(ρ, z)`): span `k` plus the span's parameter (below), the span the
//!   one of the surface's whose point at that parameter (clamped to the
//!   span) lies nearest, ties to the lowest index;
//! - affine image: its source's at `M⁻¹(x − t)`.
//!
//! A level `(L_u, L_v)`'s coordinates are those times `2^L` (its tiles
//! are their base tiles' exact sub-patches over dyadic cells). A turn is
//! periodic: of the angle's values `θ + 4m`, the one nearest the middle
//! of the surface's range is taken.
//!
//! **Around a circle** (the half-angle and Möbius maps). The lattice's
//! quarter circles have the weights `1, √½, 1`: a rational quadratic arc
//! with equal end weights over the angle `2α` has the half-angle tangent
//! of its angle from the arc's middle linear in its parameter,
//! `tan((φ − α)/2) = tan(α/2)·(2s − 1)`. So with `t = tan(φ/2)`, `φ`
//! the angle from the cell's first radius, `s = t·(1 + T²)/(2T·(1 + t·T))`,
//! `T = tan(α/2)`, a Möbius map of `t`: for a quarter circle (`T = √2 −
//! 1`) `s = √2·t/(1 + (√2 − 1)·t)`. The point's coordinates `(c, s′)`
//! along the cell's two radii (`c > 0`, `s′ ≥ 0` in the cell that owns it)
//! give `t = s′/(r + c)`, `r = √(c² + s′²)`: no cancellation, no angle
//! function.
//!
//! **Along a conic span** (`P₀, P₁, P₂`, weights `w₀, w₁, w₂`): the
//! barycentric coordinates `λᵢ` of the point in the control triangle
//! (times its doubled area) satisfy `s/(1 − s) = w₀λ₁/(2w₁λ₀) =
//! 2w₁λ₂/(w₂λ₁)` on the span, so `s = (w₀λ₁ + 2w₁λ₂)/(w₀λ₁ + 2w₁λ₀ +
//! 2w₁λ₂ + w₂λ₁)`: the sum of both ratios' terms, well conditioned at
//! both ends, a rational (Möbius) function of the point. These ratios
//! are `0/0` on a straight span and lose their precision on a nearly
//! straight one, where the span's position along its chord does not: the
//! root in the parameter of the point's projection on the chord (a
//! quadratic), the one nearest `[0, 1]`. Of the two parameters, the one
//! whose point of the span lies nearer the point is taken (the ratios' on
//! a tie): no threshold, both exact on the span.
//!
//! **The bound.** The UV is computed in `f64`, deterministic (no fused
//! operations, `√` correctly rounded). Its certificate is the surface's
//! certified point there ([`Surface::eval`]): `dev` bounds `|S(uv) − x|`
//! from above, rounding included, so it is the point's distance from the
//! surface's point at that UV, and for a point on the surface the
//! rounding of the inverse (its parameter's rounding times the surface's
//! speed) and of the tile. A UV outside the surface's domain (the point
//! lies beyond it) has no tile to evaluate: `dev` is `+∞`.
//!
//! **Construction-time conditioning** (a Möbius map per tile column or
//! row) is not applied by the lattices yet: when it is, the inverse
//! applies the same map to the coordinate it reparametrises.

use std::f64::consts::SQRT_2;
use std::ops::RangeInclusive;

use varde_decide::num::bounded::Bounded;

use super::form::{FormKey, Span2};
use super::lattice::{CELL, Lattice, pow2};
use super::surface::Surface;

/// A point's UV on a surface and its certified distance from the
/// surface's point there (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inverse {
    /// The global UV.
    pub uv: [f64; 2],
    /// An upper bound on `|S(uv) − x|`, certified; `+∞` where `uv` lies
    /// outside the domain or isn't finite.
    pub dev: f64,
}

/// Why a point has no inverse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InverseError {
    /// A coordinate of the point is not finite.
    NotFinite,
}

impl Surface {
    /// The global UV of `x` on the surface by its form's analytic inverse
    /// and the certified distance from the surface's point there (module
    /// docs).
    pub fn inverse(&self, x: [f64; 3]) -> Result<Inverse, InverseError> {
        if !x.iter().all(|c| c.is_finite()) {
            return Err(InverseError::NotFinite);
        }
        let rect = self.rect();
        let (lu, lv) = (rect.level.u, rect.level.v);
        let middle = |r: &std::ops::Range<i64>, l: u32| {
            (r.start as f64 + r.end as f64) * 0.5 * pow2(-(l as i32))
        };
        let base = |r: &std::ops::Range<i64>, l: u32| (r.start >> l)..=((r.end - 1) >> l);
        let within = Within {
            middle: [middle(&rect.u, lu), middle(&rect.v, lv)],
            cells: [base(&rect.u, lu), base(&rect.v, lv)],
        };
        let [bu, bv] = base_uv(self.lattice(), x, &within);
        let uv = [bu * pow2(lu as i32), bv * pow2(lv as i32)];
        let dev = match self.eval(uv) {
            Some(p) => distance(&p, &x),
            None => f64::INFINITY,
        };
        Ok(Inverse { uv, dev })
    }
}

/// What the inverse reads of the surface's rectangle, in base cells: the
/// middle of each direction's range and the cells it covers.
struct Within {
    middle: [f64; 2],
    cells: [RangeInclusive<i64>; 2],
}

/// An upper bound on `|p − x|` for the certified point `p`.
fn distance(p: &[Bounded<f64>; 3], x: &[f64; 3]) -> f64 {
    let d: [Bounded<f64>; 3] = std::array::from_fn(|k| p[k] - Bounded::exact(x[k]));
    let s = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let b = s.sqrt_nonneg();
    if b.is_certified() {
        b.abs_bound()
    } else {
        f64::INFINITY
    }
}

/// The base level's global coordinates of `x` on `lattice` (module
/// docs).
fn base_uv(lattice: &Lattice, x: [f64; 3], within: &Within) -> [f64; 2] {
    let o = lattice.origin();
    let [e1, e2, a] = lattice.axes();
    let y = [x[0] - o[0], x[1] - o[1], x[2] - o[2]];
    let (xi, eta, z) = (dot(&e1, &y), dot(&e2, &y), dot(&a, &y));
    let [mu, mv] = within.middle;
    let angle = || match quarter_turns(xi, eta) {
        Some(t) => nearest_turn(t, mu),
        None => mu,
    };
    let rho = || hypot(xi, eta);
    match lattice.key() {
        FormKey::Plane(_) => [xi / CELL + 0.5, eta / CELL + 0.5],
        FormKey::Cylinder(_) => [angle(), z / CELL + 0.5],
        FormKey::Cone(k) => [angle(), (z * k.cos() + rho() * k.sin()) / CELL],
        FormKey::Sphere(_) => {
            let lat = quarter_turns(rho(), z).map_or(mv, |t| nearest_turn(t, mv));
            [angle(), lat]
        }
        FormKey::Torus(k) => {
            let (big, small) = (k.major(), k.minor());
            let r = rho();
            let tube = if big < small {
                spindle_tube(big, small, r, z)
            } else {
                quarter_turns(r - big, z).map_or(mv, |t| nearest_turn(t, mv))
            };
            [angle(), tube]
        }
        FormKey::ConicCylinder(k) => [
            along(k.profile().spans(), &within.cells[0], [xi, eta]),
            z / CELL + 0.5,
        ],
        FormKey::Revolved(k) => [
            angle(),
            along(k.profile().spans(), &within.cells[1], [rho(), z]),
        ],
        FormKey::Affine(k) => {
            let src = lattice
                .source()
                .expect("an affine image's lattice has its source's");
            base_uv(src, preimage(&k.linear(), &k.shift(), &x), within)
        }
    }
}

/// `M⁻¹(x − t)` by the adjugate, in `f64`.
fn preimage(m: &[[f64; 3]; 3], t: &[f64; 3], x: &[f64; 3]) -> [f64; 3] {
    let minor =
        |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    // `adj M`, row `r` the cofactors of column `r`.
    let adj: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| minor((c + 1) % 3, (c + 2) % 3, (r + 1) % 3, (r + 2) % 3))
    });
    let det = m[0][0] * adj[0][0] + m[0][1] * adj[1][0] + m[0][2] * adj[2][0];
    let d = [x[0] - t[0], x[1] - t[1], x[2] - t[2]];
    std::array::from_fn(|r| dot(&adj[r], &d) / det)
}

/// The angle of `(x, y)` from the first axis towards the second, in
/// quarter turns in `[0, 4]`, by the half-angle tangent and the quarter
/// circle's Möbius map (module docs); `None` at the origin (or for a
/// NaN).
pub(in crate::geom) fn quarter_turns(x: f64, y: f64) -> Option<f64> {
    // The cell owning the point and its coordinates `(c, s)` along the
    // cell's radii: `c > 0`, `s ≥ 0`.
    let (k, c, s) = if x > 0.0 && y >= 0.0 {
        (0.0, x, y)
    } else if y > 0.0 && x <= 0.0 {
        (1.0, y, -x)
    } else if x < 0.0 && y <= 0.0 {
        (2.0, -x, -y)
    } else if y < 0.0 && x >= 0.0 {
        (3.0, -y, x)
    } else {
        return None;
    };
    let t = s / (hypot(c, s) + c);
    Some(k + SQRT_2 * t / (1.0 + (SQRT_2 - 1.0) * t))
}

/// Of `t + 4m`, the value nearest `near`.
fn nearest_turn(t: f64, near: f64) -> f64 {
    t + 4.0 * ((near - t) / 4.0 + 0.5).floor()
}

/// `√(a² + b²)`, scaled by the larger magnitude so it neither overflows
/// nor underflows.
fn hypot(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    let m = if a > b { a } else { b };
    if m == 0.0 || !m.is_finite() {
        return m;
    }
    let (p, q) = (a / m, b / m);
    m * (p * p + q * q).sqrt()
}

fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A spindle torus's tube coordinate of the meridian point `(ρ, z)`: on
/// its apple's arc in `z`'s half, cell 0 from the outer equator up to the
/// apex `(0, √(r² − R²))`, cell −1 from the lower apex to the equator
/// (the lattice's spans, [`super::forms::torus`]).
fn spindle_tube(big: f64, small: f64, rho: f64, z: f64) -> f64 {
    let d = ((small - big) * (small + big)).sqrt();
    let corner = small * d / (small - big);
    let w = ((small - big) / (2.0 * small)).sqrt();
    let outer = big + small;
    if z >= 0.0 {
        let span = [[outer, 0.0, 1.0], [outer, corner, w], [0.0, d, 1.0]];
        span_param(&span, [rho, z])
    } else {
        let span = [[0.0, -d, 1.0], [outer, -corner, w], [outer, 0.0, 1.0]];
        span_param(&span, [rho, z]) - 1.0
    }
}

/// The profile coordinate of the point `p` on the spans `cells` of
/// `spans` (module docs): the span index plus its parameter.
fn along(spans: &[Span2], cells: &RangeInclusive<i64>, p: [f64; 2]) -> f64 {
    let mut best: Option<(f64, f64)> = None;
    for k in cells.clone() {
        let Some(span) = usize::try_from(k).ok().and_then(|k| spans.get(k)) else {
            continue;
        };
        let s = span_param(span, p);
        let at = span_point(span, s.clamp(0.0, 1.0));
        let d = (at[0] - p[0]) * (at[0] - p[0]) + (at[1] - p[1]) * (at[1] - p[1]);
        // A NaN distance is never nearer.
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, k as f64 + s));
        }
    }
    best.map_or(f64::NAN, |(_, s)| s)
}

/// The parameter of the point `p` on the span `span` (module docs): of
/// the barycentric ratios' and the chord's, the one whose point lies
/// nearer `p`, the ratios' on a tie (a NaN is never nearer).
fn span_param(span: &Span2, p: [f64; 2]) -> f64 {
    let off = |s: f64| {
        let q = span_point(span, s);
        (q[0] - p[0]) * (q[0] - p[0]) + (q[1] - p[1]) * (q[1] - p[1])
    };
    let (ratios, chord) = (conic_param(span, p), line_param(span, p));
    if off(ratios) <= off(chord) {
        ratios
    } else {
        chord
    }
}

/// The parameter of the point `p` on the conic span `span` by its
/// barycentric ratios (module docs).
fn conic_param(span: &Span2, p: [f64; 2]) -> f64 {
    let rel = |k: usize| [span[k][0] - p[0], span[k][1] - p[1]];
    let cr = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let (r0, r1, r2) = (rel(0), rel(1), rel(2));
    let (l0, l1, l2) = (cr(r1, r2), cr(r2, r0), cr(r0, r1));
    let [w0, w1, w2] = span.map(|x| x[2]);
    let num = w0 * l1 + 2.0 * w1 * l2;
    num / (num + 2.0 * w1 * l0 + w2 * l1)
}

/// The parameter of the point `p` on the span `span` by its chord: the
/// root nearest `[0, 1]` of `Σ Bᵢ·wᵢ·(xᵢ − x) = 0`, `xᵢ` and `x` the
/// positions along the chord from `P₀` (exact on a straight span, whose
/// ratios are `0/0`).
fn line_param(span: &Span2, p: [f64; 2]) -> f64 {
    let d = [span[2][0] - span[0][0], span[2][1] - span[0][1]];
    let pos = |q: [f64; 2]| (q[0] - span[0][0]) * d[0] + (q[1] - span[0][1]) * d[1];
    let x = pos(p);
    let [a, b, c] = [0, 1, 2].map(|k| span[k][2] * (pos([span[k][0], span[k][1]]) - x));
    // `(a − 2b + c)·s² + 2(b − a)·s + a = 0`.
    let (qa, qb, qc) = (a - 2.0 * b + c, b - a, a);
    let roots = if qa == 0.0 {
        [qc / (-2.0 * qb), f64::NAN]
    } else {
        let disc = qb * qb - qa * qc;
        if disc < 0.0 {
            [-qb / qa, f64::NAN]
        } else {
            let r = disc.sqrt();
            let m = -(qb + if qb < 0.0 { -r } else { r });
            if m == 0.0 {
                [0.0, f64::NAN]
            } else {
                [m / qa, qc / m]
            }
        }
    };
    let off = |s: f64| {
        if s.is_nan() {
            f64::INFINITY
        } else if s < 0.0 {
            -s
        } else if s > 1.0 {
            s - 1.0
        } else {
            0.0
        }
    };
    if off(roots[1]) < off(roots[0]) {
        roots[1]
    } else {
        roots[0]
    }
}

/// The span's point at `s`, in `f64`.
fn span_point(span: &Span2, s: f64) -> [f64; 2] {
    let b = [(1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s), s * s];
    let wb: [f64; 3] = std::array::from_fn(|k| b[k] * span[k][2]);
    let w = wb[0] + wb[1] + wb[2];
    [0, 1].map(|c| (wb[0] * span[0][c] + wb[1] * span[1][c] + wb[2] * span[2][c]) / w)
}

#[cfg(test)]
mod tests;
