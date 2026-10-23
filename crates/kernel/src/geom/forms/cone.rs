//! The cone's tiles: cell `(k, l)` is the angle `[k, k + 1]·90°` from
//! `e₁` towards `e₂` times the slant cell `[l, l + 1]·CELL`, `l ≥ 0`, of
//! the distance `t` from the apex along the generators, anchored at the
//! apex: with the key's cosine `cos` and sine `sin` of the half-angle
//! and its axis `a` (into the nappe used), the point at angle `θ` and
//! distance `t` is `apex + t·(cos·a + sin·(cos θ·e₁ + sin θ·e₂))` (OCCT's
//! `Convert_ConeToBSplineSurface` parametrises its `V` the same way, from
//! its reference circle). Control point `(i, j)` is
//! `apex + t_j·(cos·a + sin·r_i)`, `r_i` the cell's quarter circle's
//! control directions (the two radii and their corner, weights `1, √½,
//! 1`) and `t_j` the slant cell's rows `l·CELL`, `(l + ½)·CELL`,
//! `(l + 1)·CELL`.
//!
//! **The apex row.** Cell `(k, 0)`'s first `v` row is `t = 0`: its three
//! control points are the apex, to the bit: a singular side of the tile,
//! which the fold check divides out; every other row is a circle.
//! As `CELL ≥ 4·MAX_COORD`, slant cell 0 holds the cone's whole part of
//! the coordinate box where the apex lies in it; the cells past it hold
//! the parts of cones whose apex lies further out. Angle cells `k` and
//! `k + 4` are the same tile; the orientation `x_u × x_v` points out of
//! the cone (away from its axis).
//!
//! **Level cap.** Along `v` the cells are lines (their generators'
//! direction is unit to rounding). Around the axis the chord is taken at
//! the radius `sin·t` of the cells' far row, the largest of the cells
//! asked for: near the apex a cell's chord shrinks to 0 at any level,
//! which the apex row's singular side, not the cap, accounts for.

use malachite::Rational;

use super::super::form::ConeKey;
use super::super::lattice::{
    ARC, CELL, Cells, ExactFrame, Frame, Lattice, LatticeError, Reach, Run, affine, arc, far,
    larger,
};
use super::super::round::q;
use super::super::surface::Surface;
use super::super::tile::Net;

/// The base tile's net of cell `(k, l)` (module docs); `Index` for
/// `l < 0` (the other nappe) or where an index's arithmetic overflows.
pub(in crate::geom) fn net(
    f: &ExactFrame,
    key: &ConeKey,
    k: i64,
    l: i64,
) -> Result<Net, LatticeError> {
    if l < 0 {
        return Err(LatticeError::Index);
    }
    let rows = slant_rows(l)?;
    let (o, [a0, a1, a2]) = (&f.origin, &f.axes);
    let around = arc(a0, a1, k);
    let (cos, sin) = (q(key.cos()), q(key.sin()));
    let half = q(CELL) / Rational::from(2u32);
    Ok(std::array::from_fn(|n| {
        let (dir, _) = &around[n / 3];
        let t = Rational::from(rows[n % 3]) * &half;
        let (along, across) = (&t * &cos, &t * &sin);
        let p = [0, 1, 2].map(|c| &o[c] + &along * &a2[c] + &across * &dir[c]);
        affine(&p, ARC[n / 3].value())
    }))
}

/// The rows, in half cells, of slant cell `l`: `2l`, `2l + 1`, `2l + 2`.
fn slant_rows(l: i64) -> Result<[i64; 3], LatticeError> {
    let m = l.checked_mul(2).ok_or(LatticeError::Index)?;
    let mid = m.checked_add(1).ok_or(LatticeError::Index)?;
    let hi = m.checked_add(2).ok_or(LatticeError::Index)?;
    Ok([m, mid, hi])
}

/// What the level cap reads of the slant cells `v` (any angle cells):
/// coordinates to the far row `t` of the cells, the radius `sin·t`
/// there, `t` along the lines; `u` around a circle of that radius, `v` a
/// line (module docs).
pub(in crate::geom) fn reach(f: &Frame, key: &ConeKey, v: &Cells) -> Reach {
    let (o, [a0, a1, a2]) = (f.origin, f.axes);
    // The far row of the cells: `(|l| + 1)·CELL`.
    let t = (far(v) as f64 + 1.0) * CELL;
    let (cos, sin) = (key.cos(), key.sin());
    let m = (0..3)
        .map(|c| o[c].abs() + t * (cos * a2[c].abs() + sin * (a0[c].abs() + a1[c].abs())))
        .fold(0.0, larger);
    Reach {
        m,
        rho: t * sin,
        t,
        runs: [Run::Turn(t * sin), Run::Line],
    }
}

/// The cone's 180° surface of angle cells `first` and `first + 1` times
/// slant cell 0, its apex row the tiles' first `v` row (module docs).
pub fn cone(key: ConeKey, first: i64) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let rect = lattice.half(first)?;
    Surface::new(lattice, rect)
}

#[cfg(test)]
mod tests;
