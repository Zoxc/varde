//! Cylinders' tiles: the circular cylinder's and the conic-chain
//! cylinder's, each a profile in the frame's `e₁`, `e₂` plane times a
//! line along the axis `a`.
//!
//! **Circular cylinder.** Cell `(k, l)` is the angle `[k, k + 1]·90°`
//! from `e₁` towards `e₂` times the axial cell `[(l − ½)·CELL,
//! (l + ½)·CELL]` from the foot point along `a`: a quarter circle (its
//! control points the cell's two radii's ends and their corner, weights
//! `1, √½, 1`) times a line. Angle cells `k` and `k + 4` are the same
//! tile; the orientation `x_u × x_v` points out of the cylinder.
//!
//! **Conic-chain cylinder.** Cell `(k, l)` is the profile's span `k`
//! (`0 ≤ k <` the number of spans, in the key's canonical direction)
//! times the same axial cells from the world origin, the frame's origin
//! (the profile's plane passes through it): control point `(i, j)` is
//! `x_i·e₁ + y_i·e₂ + t_j·a` with the span's weight `w_i`, `(x_i, y_i,
//! w_i)` the span's stored control points and `t_j` the axial cell's
//! rows. The orientation `x_u × x_v` is the profile's direction times
//! the axis, not an inside or outside (a face's `flipped` says which).
//! Neighbouring spans share their end point bit for bit (the key's
//! chain), so neighbouring tiles share their row where the spans' end
//! weights there are equal too ([`joints`]; a lattice refuses a profile
//! where they differ). Where the profile closes (its last span ends at
//! its first span's start), a surface may not take every span ([`closed`]).
//!
//! **The level cap's chord along a profile.** A rational quadratic span
//! `C` with control points `P₀, P₁, P₂` and weights `w₀, w₁, w₂` has the
//! derivative `C′ = 2·[w₀w₁(1−s)²·d₁ + w₀w₂·s(1−s)·(d₁ + d₂) + w₁w₂·s²·d₂]/W²`
//! (`d₁ = P₁ − P₀`, `d₂ = P₂ − P₁`, `W` the denominator). With `b` the unit
//! bisector of `d₁` and `d₂`, `b·d₁ = |d₁|·c` and `b·d₂ = |d₂|·c` for
//! `c = cos(α/2) = |d₁/|d₁| + d₂/|d₂||/2` (`α` the angle between them), so
//! `b·C′ ≥ 2c·min(|d₁|, |d₂|)·min(wᵢwⱼ)/max(w)²` everywhere on the span: a
//! sub-span of parameter length `h` has a chord at least that rate times
//! `h` ([`chord_rate`]). A span whose control polygon turns back (`c` 0)
//! has no positive rate and caps its lattice's `u` at the base level.

use std::ops::Range;

use malachite::Rational;

use super::super::form::{ConicCylinderKey, CylinderKey, Profile, Span2};
use super::super::lattice::{
    CELL, Cells, ExactFrame, Frame, Lattice, LatticeError, Level, Reach, Rect, Run, affine, arc,
    halves, larger, line_reach, pow2,
};
use super::super::round::q;
use super::super::surface::Surface;
use super::super::tile::Net;

/// The circular cylinder's base tile's net of cell `(k, l)` (module
/// docs); `Index` where an index's arithmetic overflows.
pub(in crate::geom) fn net(
    f: &ExactFrame,
    key: &CylinderKey,
    k: i64,
    l: i64,
) -> Result<Net, LatticeError> {
    let (o, [a0, a1, a2]) = (&f.origin, &f.axes);
    let around = arc(a0, a1, k);
    let rho = q(key.radius());
    let g = halves(l)?;
    let half = q(CELL) / Rational::from(2u32);
    Ok(std::array::from_fn(|n| {
        let (dir, w) = &around[n / 3];
        let t = Rational::from(g[n % 3]) * &half;
        let p = [0, 1, 2].map(|c| &o[c] + &rho * &dir[c] + &t * &a2[c]);
        affine(&p, w.value())
    }))
}

/// What the level cap reads of the circular cylinder's axial cells `v`
/// (any angle cells): coordinates to the radius around the axis and the
/// cells' reach along it; `u` around a circle of the radius, `v` a line.
pub(in crate::geom) fn reach(f: &Frame, key: &CylinderKey, v: &Cells) -> Reach {
    let (o, [a0, a1, a2]) = (f.origin, f.axes);
    let r = key.radius();
    let t = line_reach(v);
    let m = (0..3)
        .map(|c| o[c].abs() + r * (a0[c].abs() + a1[c].abs()) + t * a2[c].abs())
        .fold(0.0, larger);
    Reach {
        m,
        rho: r,
        t,
        runs: [Run::Turn(r), Run::Line],
    }
}

/// The conic-chain cylinder's base tile's net of cell `(k, l)` (module
/// docs); `Index` for a span index outside the profile or where an
/// index's arithmetic overflows.
pub(in crate::geom) fn chain_net(
    f: &ExactFrame,
    key: &ConicCylinderKey,
    k: i64,
    l: i64,
) -> Result<Net, LatticeError> {
    let span = usize::try_from(k)
        .ok()
        .and_then(|k| key.profile().spans().get(k))
        .ok_or(LatticeError::Index)?;
    let (o, [a0, a1, a2]) = (&f.origin, &f.axes);
    let g = halves(l)?;
    let half = q(CELL) / Rational::from(2u32);
    Ok(std::array::from_fn(|n| {
        let [x, y, w] = span[n / 3];
        let (x, y) = (q(x), q(y));
        let t = Rational::from(g[n % 3]) * &half;
        let p = [0, 1, 2].map(|c| &o[c] + &x * &a0[c] + &y * &a1[c] + &t * &a2[c]);
        affine(&p, w)
    }))
}

/// What the level cap reads of the conic-chain cylinder's base cells
/// `u` (spans) × `v` (axial cells): coordinates to the spans' control
/// points and the axial cells' reach, the radius the spans' largest
/// `|x| + |y|`; `u` along the profile at the spans' least chord rate
/// ([`chord_rate`]), `v` a line. Spans outside the profile are left out.
pub(in crate::geom) fn chain_reach(
    f: &Frame,
    key: &ConicCylinderKey,
    u: &Cells,
    v: &Cells,
) -> Reach {
    let (o, [a0, a1, a2]) = (f.origin, f.axes);
    let spans = key.profile().spans();
    let from = usize::try_from(*u.start()).unwrap_or(0);
    let to = usize::try_from(*u.end()).map_or(0, |e| e.saturating_add(1).min(spans.len()));
    let t = line_reach(v);
    let (mut m, mut rho, mut rate) = (0.0, 0.0, f64::INFINITY);
    for s in spans.get(from..to).unwrap_or(&[]) {
        for p in s {
            let (x, y) = (p[0].abs(), p[1].abs());
            rho = larger(rho, x + y);
            for c in 0..3 {
                m = larger(
                    m,
                    o[c].abs() + x * a0[c].abs() + y * a1[c].abs() + t * a2[c].abs(),
                );
            }
        }
        let r = chord_rate(s);
        rate = if r < rate { r } else { rate };
    }
    Reach {
        m,
        rho,
        t,
        runs: [
            Run::Span(if rate.is_finite() { rate } else { 0.0 }),
            Run::Line,
        ],
    }
}

/// A lower bound on the chord of every sub-span of `span` per unit of
/// its parameter (module docs), rounded down; 0 where there is none (a
/// zero leg, a polygon turning back, values past the range).
pub(in crate::geom) fn chord_rate(span: &Span2) -> f64 {
    let leg = |a: &[f64; 3], b: &[f64; 3]| [b[0] - a[0], b[1] - a[1]];
    let (d1, d2) = (leg(&span[0], &span[1]), leg(&span[1], &span[2]));
    let norm = |d: &[f64; 2]| (d[0] * d[0] + d[1] * d[1]).sqrt();
    let (n1, n2) = (norm(&d1), norm(&d2));
    if !(n1.is_finite() && n2.is_finite() && n1 > 0.0 && n2 > 0.0) {
        return 0.0;
    }
    let b = [d1[0] / n1 + d2[0] / n2, d1[1] / n1 + d2[1] / n2];
    // `cos(α/2)` to a few ulps of 1, less a margin.
    let c = norm(&b) / 2.0 - pow2(-48);
    if c <= 0.0 {
        return 0.0;
    }
    let [w0, w1, w2] = span.map(|p| p[2]);
    let low = [w0 * w1, w0 * w2, w1 * w2]
        .into_iter()
        .fold(f64::INFINITY, |m, x| if x < m { x } else { m });
    let high = larger(larger(w0, w1), w2);
    let short = if n1 < n2 { n1 } else { n2 };
    // Products and quotients of positive values, each rounded: a
    // relative `2⁻⁴⁰` covers their rounding and the frame's (`e₁`, `e₂`
    // orthonormal to rounding).
    let rate = 2.0 * c * short * low / (high * high) * (1.0 - pow2(-40));
    if rate.is_finite() { rate } else { 0.0 }
}

/// `Joint` unless every two consecutive spans of the profile have the
/// same weight, bit for bit, at their shared point (module docs).
pub(in crate::geom) fn joints(key: &ConicCylinderKey) -> Result<(), LatticeError> {
    profile_joints(key.profile())
}

/// `Joint` unless every two consecutive spans of `profile` have the same
/// weight, bit for bit, at their shared point.
pub(in crate::geom) fn profile_joints(profile: &Profile) -> Result<(), LatticeError> {
    let spans = profile.spans();
    if spans
        .windows(2)
        .all(|p| p[0][2][2].to_bits() == p[1][0][2].to_bits())
    {
        Ok(())
    } else {
        Err(LatticeError::Joint)
    }
}

/// Whether the profile closes: its last span ends at its first span's
/// start (equal values, as the key's chain compares its joints).
pub(in crate::geom) fn closed(key: &ConicCylinderKey) -> bool {
    profile_closed(key.profile())
}

/// Whether `profile` closes: its last span ends at its first span's
/// start.
pub(in crate::geom) fn profile_closed(profile: &Profile) -> bool {
    let spans = profile.spans();
    let (first, last) = (spans[0][0], spans[spans.len() - 1][2]);
    first[0] == last[0] && first[1] == last[1]
}

/// The circular cylinder's 180° surface of angle cells `first` and
/// `first + 1` times axial cell 0 (module docs).
pub fn cylinder(key: CylinderKey, first: i64) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let rect = lattice.half(first)?;
    Surface::new(lattice, rect)
}

/// The conic-chain cylinder's surface of the profile's spans `spans`
/// (indices in the key's canonical direction) times axial cell 0 (module
/// docs); `Index` for spans outside the profile, `Span` for every span of
/// a closed one.
pub fn conic_chain(key: ConicCylinderKey, spans: Range<i64>) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let rect = Rect {
        level: Level::BASE,
        u: spans,
        v: 0..1,
    };
    Surface::new(lattice, rect)
}

#[cfg(test)]
mod tests;
