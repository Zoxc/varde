//! Certified bounds: the **seam bound** of a coedge, an upper bound of
//! `|S(p(t)) − C(t)|` over the coedge, with the **same `t`** on the face's
//! surface `S` composed with its p-curve `p` and on the edge's curve `C`.
//! It is a parametric deviation, so it also bounds the distance between
//! the two images; it is the width of the coedge's ribbon, the surface
//! between them.
//!
//! **Pieces.** The coedge is cut at every knot of `p` and of `C` inside
//! it, so each piece lies in one span of each. A p-curve span lies in
//! one tile, `(i, j)`, decided exactly: each stored control point's `u`
//! within `[i, i + 1]` and `v` within `[j, j + 1]`, compared as floats
//! (the span lies in its control points' hull, as its weights are
//! positive); the lower candidate where a span runs on a tile line at
//! the domain's far side.
//!
//! **The bound on a piece.** The spans' homogeneous coefficients are
//! formed at the first level from their affine control points, each
//! product `w·x` rounded once with its rounding carried
//! ([`Curve3D::span`]). With the span's local homogeneous
//! coordinates `U' = U − i·W`, `V' = V − j·W`, the tile's net `Pₖₗ`
//! composed with it is `H = Σₖₗ Pₖₗ·Bₖ(U', W)·Bₗ(V', W)`, `Bₖ` the
//! homogeneous quadratic Bernstein basis (`(W − U')²`, `2U'(W − U')`,
//! `U'²`): degree 8, the tile's point `(H₀, H₁, H₂) / H₃`. The difference
//! with `C = (X, W_C)` is `N / D`, `Nₘ = Hₘ·W_C − Xₘ·H₃` of degree 10 and
//! `D = H₃·W_C`. Its length is at most `Σₘ max|Nₘ| / min D`, the maxima and
//! the minimum certified from the Bernstein coefficients (the forms lie
//! in their coefficients' hull) at the first level, every error carried,
//! rounded upwards. Where the denominator isn't certified positive, the
//! piece is split at ½, up to [`MAX_SPLITS`] times.
//!
//! This is the simple form of the bound: one Bernstein bound per piece
//! (no subdivision to a target, no higher precision level), biquadratic
//! tiles and Q spans only. Its rounding scales with the coordinates'
//! magnitude: on a plane, whose one tile spans the coordinate box, about
//! `1e-9` mm for weights near 1. It grows with the spread of the weights
//! along a span, as the bound divides the largest coefficients' rounding
//! by the least denominator: on that tile a 90° arc with end weights 3
//! and ⅓ gets about `2·10⁻⁴` mm, with end weights 64 and 1/64 about
//! `3·10⁹` mm, though the arc lies exactly on the plane.

use varde_decide::bezier::{HomCurve, Poly, Scalar};
use varde_decide::num::bounded::{Bounded, add_up, div_up};

use super::curve::{Curve3D, PCurve, Span2, local};
use super::surface::Surface;
use super::tile::Tile;

/// The most ½ splits of a piece whose denominator isn't certified
/// positive.
pub const MAX_SPLITS: u32 = 6;

/// Why a seam bound can't be given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeamError {
    /// The p-curve's knots run outside the curve's.
    Range,
    /// Span `span` of the p-curve doesn't lie in one tile of the domain.
    Tile { span: usize },
    /// The bound on a piece within span `span` of the p-curve isn't
    /// certified after [`MAX_SPLITS`] splits.
    Uncertified { span: usize },
}

type B = Bounded<f64>;

/// The seam bound of `pcurve` on `surface` against `curve` over the
/// p-curve's knots' range (module docs).
pub fn seam_bound(surface: &Surface, curve: &Curve3D, pcurve: &PCurve) -> Result<f64, SeamError> {
    let (pk, ck) = (pcurve.knots(), curve.knots());
    let (t0, t1) = (pk[0], pk[pk.len() - 1]);
    if t0 < ck[0] || t1 > ck[ck.len() - 1] {
        return Err(SeamError::Range);
    }
    let cells = pcurve
        .spans()
        .iter()
        .enumerate()
        .map(|(span, s)| tile_of(surface, s).ok_or(SeamError::Tile { span }))
        .collect::<Result<Vec<_>, _>>()?;
    // The cuts: the p-curve's knots and the curve's inside them, merged.
    let mut cuts: Vec<f64> = pk
        .iter()
        .chain(ck.iter().filter(|&&t| t0 < t && t < t1))
        .copied()
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut bound = 0.0;
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        // The spans holding the piece: the last starting at or below `a`.
        let sp = pk.partition_point(|&k| k <= a) - 1;
        let sc = ck.partition_point(|&k| k <= a) - 1;
        let p = restrict(
            &pcurve.span(sp).expect("a span"),
            local(a, pk[sp], pk[sp + 1]),
            local(b, pk[sp], pk[sp + 1]),
        );
        let c = restrict(
            &curve.span(sc).expect("a span"),
            local(a, ck[sc], ck[sc + 1]),
            local(b, ck[sc], ck[sc + 1]),
        );
        let (tile, i, j) = cells[sp];
        let piece = piece_bound(tile, i, j, &p, &c, MAX_SPLITS)
            .ok_or(SeamError::Uncertified { span: sp })?;
        if piece > bound {
            bound = piece;
        }
    }
    Ok(bound)
}

/// The tile holding span `s` and its indices: `(⌊u⌋, ⌊v⌋)` of the least
/// control point, or the one below in either direction, whichever holds
/// every control point and lies in the domain.
fn tile_of<'a>(surface: &'a Surface, s: &Span2) -> Option<(&'a Tile, i64, i64)> {
    let pick = |k: usize, range: &std::ops::Range<i64>| -> Option<i64> {
        let least = s
            .iter()
            .map(|p| p[k])
            .fold(f64::INFINITY, |a, x| if x < a { x } else { a });
        // Below 2⁵² in magnitude, so the candidates and the tile lines
        // about them are exact as `i64`s and as `f64`s.
        if least.is_nan() || least.abs() >= 4_503_599_627_370_496.0 {
            return None;
        }
        let c = least.floor() as i64;
        [c, c - 1].into_iter().find(|&i| {
            if !range.contains(&i) {
                return false;
            }
            let (lo, hi) = (i as f64, (i + 1) as f64);
            s.iter().all(|p| lo <= p[k] && p[k] <= hi)
        })
    };
    let rect = surface.rect();
    let (i, j) = (pick(0, &rect.u)?, pick(1, &rect.v)?);
    let tile = surface.tile(i, j)?;
    Some((tile, i, j))
}

/// The span `c` restricted to `[sa, sb]`, as a span on `[0, 1]`.
fn restrict<const N: usize>(c: &HomCurve<B, N>, sa: B, sb: B) -> HomCurve<B, N> {
    let left = if sb.is_int(1) {
        c.clone()
    } else {
        let [l, _] = c.split_at(&sb);
        l
    };
    if sa.is_int(0) {
        left
    } else {
        let [_, r] = left.split_at(&(sa / sb));
        r
    }
}

/// The bound of `|S(p) − C|` on one piece (module docs), splitting at ½
/// up to `splits` more times; `None` if not certified.
fn piece_bound(
    tile: &Tile,
    i: i64,
    j: i64,
    p: &HomCurve<B, 2>,
    c: &HomCurve<B, 3>,
    splits: u32,
) -> Option<f64> {
    if let Some(b) = direct(tile, i, j, p, c) {
        return Some(b);
    }
    if splits == 0 {
        return None;
    }
    let ([p0, p1], [c0, c1]) = (p.split_half(), c.split_half());
    let a = piece_bound(tile, i, j, &p0, &c0, splits - 1)?;
    let b = piece_bound(tile, i, j, &p1, &c1, splits - 1)?;
    Some(if b > a { b } else { a })
}

/// One Bernstein bound of `|S(p) − C|` (module docs), `None` unless the
/// denominator is certified positive and the numerators finite.
fn direct(tile: &Tile, i: i64, j: i64, p: &HomCurve<B, 2>, c: &HomCurve<B, 3>) -> Option<f64> {
    let [pu, pv] = p.coords();
    let w = p.weights();
    // Tile indices below 2⁵² in magnitude (`tile_of`): exact in `f64`.
    let shift = |x: &Poly<B>, k: i64| x.sub(&w.scale(&B::exact(k as f64)));
    let (u, v) = (shift(pu, i), shift(pv, j));
    let basis = |x: &Poly<B>| -> [Poly<B>; 3] {
        let y = w.sub(x);
        let two = B::exact(2.0);
        [
            y.mul(&y).expect("degree 4"),
            x.mul(&y).expect("degree 4").scale(&two),
            x.mul(x).expect("degree 4"),
        ]
    };
    let (bu, bv) = (basis(&u), basis(&v));
    let net = tile.net();
    let h: [Poly<B>; 4] = std::array::from_fn(|m| {
        let mut sum: Option<Poly<B>> = None;
        for (k, a) in bu.iter().enumerate() {
            for (l, b) in bv.iter().enumerate() {
                let term = a
                    .mul(b)
                    .expect("degree 8")
                    .scale(&B::exact(net[3 * k + l][m]));
                sum = Some(match sum {
                    Some(s) => s.add(&term),
                    None => term,
                });
            }
        }
        sum.expect("nine terms")
    });
    let wc = c.weights();
    let den = h[3].mul(wc).expect("degree 10").hull().lo;
    if den.is_nan() || den <= 0.0 {
        return None;
    }
    let mut num = 0.0;
    for (m, x) in c.coords().iter().enumerate() {
        let n = h[m]
            .mul(wc)
            .expect("degree 10")
            .sub(&x.mul(&h[3]).expect("degree 10"));
        num = add_up(num, n.hull().abs_max());
    }
    let b = div_up(num, den);
    b.is_finite().then_some(b)
}

#[cfg(test)]
mod tests;
