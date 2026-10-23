//! Ruled surfaces: the homogeneous linear interpolation of two rails over
//! one parameter `t`, `H(t, s) = (1 − s)·H₀(t) + s·H₁(t)`, `H₀`, `H₁` the
//! rails' homogeneous points `(w·x, w)`, exact given its rails. A ruled
//! surface has no form key: its identity is the surface itself, and its
//! grid its rails' spans.
//!
//! **Tiles.** Tile `k` is the rails' span `k` along `u` (over `[t_k,
//! t_{k+1}]`, the rails' common knots) times the whole ruling along `v`:
//! the `(2, 1)` patch with rows `H₀` and `H₁`, elevated to `(2, 2)`. Its
//! first `v` row is rail 0's span and its last rail 1's, their stored
//! values to the bit; its middle row is the homogeneous mean `(H₀ +
//! H₁)/2` of the spans' control points `i`: the affine point `(w₀x₀ +
//! w₁x₁)/(w₀ + w₁)` with the weight `(w₀ + w₁)/2`, each value the exact
//! one rounded once to the nearest `f64`, ties to even (a weight within
//! `[w₀, w₁]`, so within the tiles' range).
//!
//! **Shared rows.** Consecutive spans of a rail share their joint's
//! point bit for bit (the chain's rule); where each rail's spans meet
//! with equal weights there too, tile `k`'s last `u` row and tile `k +
//! 1`'s first are the same values, so they are the same bits. A rail
//! whose spans meet with different weights is refused (`Joint`), as a
//! profile's; so are rails over different knots (`Knots`: not one `t`).
//!
//! **Orientation** `x_u × x_v`: along rail 0 crossed with the ruling
//! towards rail 1. A ruled surface is regular only where its rails and
//! rulings keep apart (the fold check decides that per tile).

use super::super::curve::{Curve3D, Span3};
use super::super::round::{nearest, q};
use super::super::tile::{Net, Tile, TileError};

/// Why a ruled surface can't be made of its rails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuledError {
    /// The rails' knots differ: they don't share one parameter.
    Knots,
    /// A rail's spans meet at joint `joint` (between spans `joint − 1`
    /// and `joint`) with different weights.
    Joint { joint: usize },
    /// Tile `span`'s net is refused (a middle row's value past the
    /// range).
    Tile { span: usize, why: TileError },
}

/// The ruled surface's tiles between `rail0` and `rail1` (module docs),
/// tile `k` over the rails' span `k`.
pub fn ruled(rail0: &Curve3D, rail1: &Curve3D) -> Result<Vec<Tile>, RuledError> {
    let same = |a: &[f64], b: &[f64]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
    };
    if !same(rail0.knots(), rail1.knots()) {
        return Err(RuledError::Knots);
    }
    for rail in [rail0, rail1] {
        if let Some(k) = rail
            .spans()
            .windows(2)
            .position(|p| p[0][2][3].to_bits() != p[1][0][3].to_bits())
        {
            return Err(RuledError::Joint { joint: k + 1 });
        }
    }
    rail0
        .spans()
        .iter()
        .zip(rail1.spans())
        .enumerate()
        .map(|(span, (a, b))| Tile::new(net(a, b)).map_err(|why| RuledError::Tile { span, why }))
        .collect()
}

/// The tile's net between the rails' spans `a` and `b` (module docs).
fn net(a: &Span3, b: &Span3) -> Net {
    std::array::from_fn(|n| {
        let (i, j) = (n / 3, n % 3);
        match j {
            0 => a[i],
            2 => b[i],
            _ => mean(&a[i], &b[i]),
        }
    })
}

/// The affine point and weight of `(H₀ + H₁)/2` for the affine `p` and
/// `r` (module docs), each value rounded once.
fn mean(p: &[f64; 4], r: &[f64; 4]) -> [f64; 4] {
    let (w0, w1) = (q(p[3]), q(r[3]));
    let sum = &w0 + &w1;
    let [x, y, z] = [0, 1, 2].map(|c| nearest(&((&w0 * q(p[c]) + &w1 * q(r[c])) / &sum)));
    [x, y, z, nearest(&(sum / malachite::Rational::from(2u32)))]
}

#[cfg(test)]
mod tests;
