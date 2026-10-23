//! An affine image's tiles: its source form's lattice mapped by `x ↦
//! M·x + t`. Cell `(k, l)` is the source's cell `(k, l)`, and its base
//! tile the source's base tile with each control point mapped, `(M·x +
//! t, w)`: each coordinate the exact value of the stored point, map and
//! shift rounded once to the nearest `f64`, ties to even, each weight
//! kept (the motion bake's rule for points, applied to the net). A
//! finer level's tile is that base tile's exact sub-patch rounded once,
//! as on every lattice, so the image's tiles are a function of its key
//! and the cell alone; the source's cells, their validity, the turns
//! that may not close and the canonical rectangle are the source's.
//!
//! **Shared rows.** A row both cells beside a line share in the source
//! is the same stored values, mapped by one formula, so the image's
//! neighbours share it bit for bit, weights included; a pole or apex
//! row, one point in the source, stays one point. A cusp the source
//! refuses stays refused.
//!
//! **Orientation.** `x_u × x_v` is the source's turned by the map: the
//! same side of the image where `det M > 0`, the other where it is
//! negative.
//!
//! **Level cap.** The image's chords are at least `σ` times the
//! source's, `σ ≥ |det M|/‖adj M‖_F` a lower bound on `M`'s least
//! singular value, and its tiles' deviation from the image at most `n =
//! ‖M‖_∞ + ‖M‖_F` times the source's plus the map's rounding: the cap
//! reads the source's cells with their radii and lengths scaled by `n`,
//! their chord bounds by `σ`, and coordinates up to `4·(|t| + n·m)` for
//! the source's bound `m` (whose rounding term, `3·ulp`, then covers the
//! source's two roundings scaled and the map's; [`reach`]).

use malachite::Rational;

use super::super::form::{AffineKey, dot};
use super::super::lattice::{CELL, Lattice, LatticeError, Reach, Run, larger, pow2};
use super::super::round::{nearest, q};
use super::super::surface::Surface;
use super::super::tile::Net;

/// The source's net `net` mapped by the key's map (module docs).
pub(in crate::geom) fn net(key: &AffineKey, net: &Net) -> Net {
    let m = key.linear().map(|row| row.map(q));
    let t = key.shift().map(q);
    net.map(|p| {
        let x = [p[0], p[1], p[2]].map(q);
        let [a, b, c] = [0, 1, 2].map(|r| nearest(&(dot(&m[r], &x) + &t[r])));
        [a, b, c, p[3]]
    })
}

/// What the level cap reads of the image's cells, from what it reads of
/// the source's, `source` (module docs).
pub(in crate::geom) fn reach(key: &AffineKey, source: Reach) -> Reach {
    let m = key.linear();
    let up = 1.0 + pow2(-40);
    let inf = m
        .iter()
        .map(|row| row.iter().map(|x| x.abs()).sum::<f64>())
        .fold(0.0, larger);
    let fro = m.as_flattened().iter().map(|x| x * x).sum::<f64>().sqrt();
    let n = (inf + fro) * up;
    let shift = key.shift().iter().map(|x| x.abs()).fold(0.0, larger);
    let s = least_stretch(key);
    let scale = |run: Run| match run {
        Run::Line => Run::Span(CELL * s),
        Run::Turn(r) => Run::Turn(r * s),
        Run::Span(rate) => Run::Span(rate * s),
    };
    let finite = |x: f64| if x.is_finite() { x } else { f64::INFINITY };
    Reach {
        m: finite(4.0 * (shift + n * source.m) * up),
        rho: finite(n * source.rho),
        t: finite(n * source.t),
        runs: source.runs.map(scale),
    }
}

/// A lower bound on the map's least singular value, `|det M|/‖adj M‖_F`
/// (`‖M⁻¹‖₂ ≤ ‖M⁻¹‖_F`), rounded down; 0 where the determinant is
/// below the normal range or a value overflows.
fn least_stretch(key: &AffineKey) -> f64 {
    let m = key.linear().map(|row| row.map(q));
    let minor = |r0: usize, r1: usize, c0: usize, c1: usize| {
        &m[r0][c0] * &m[r1][c1] - &m[r0][c1] * &m[r1][c0]
    };
    let adj: [Rational; 9] = std::array::from_fn(|n| {
        let (r, c) = (n / 3, n % 3);
        let rows = [(r + 1) % 3, (r + 2) % 3];
        let cols = [(c + 1) % 3, (c + 2) % 3];
        minor(rows[0], rows[1], cols[0], cols[1])
    });
    // The cofactors along row 0 (cyclic minors carry their signs).
    let det = (0..3).fold(Rational::from(0u32), |d, c| d + &m[0][c] * &adj[c]);
    let norm2 = adj.iter().fold(Rational::from(0u32), |s, x| s + x * x);
    let det = nearest(&det).abs();
    if det < f64::MIN_POSITIVE {
        // Subnormal or zero: no relative bound on its rounding.
        return 0.0;
    }
    let norm = nearest(&norm2).sqrt();
    let s = det / norm * (1.0 - pow2(-40));
    if s.is_finite() { s } else { 0.0 }
}

/// The affine image's surface of its source's canonical rectangle
/// ([`Lattice::half`] of the source at `first`; module docs).
pub fn affine_image(key: AffineKey, first: i64) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let rect = lattice.half(first)?;
    Surface::new(lattice, rect)
}

#[cfg(test)]
mod tests;
