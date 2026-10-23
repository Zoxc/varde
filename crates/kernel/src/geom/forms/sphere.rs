//! The sphere's tiles: cell `(k, l)` is the longitude `[k, k + 1]·90°`
//! from `e₁` towards `e₂` times the latitude `[l, l + 1]·90°`, `l` −1
//! (south) or 0 (north), from the key's axis `a` and its reference
//! direction: the tensor product of the longitude's quarter circle and
//! the meridian's. Control point `(i, j)` is `centre + r·(ρ_j·d_i +
//! z_j·a)`, `d_i` the longitude cell's control directions (its two radii
//! and their corner) and `(ρ_j, z_j)` the unit meridian's (`(0, −1)`,
//! `(1, −1)`, `(1, 0)` south, `(1, 0)`, `(1, 1)`, `(0, 1)` north), its
//! weight the product of the two quarter circles' `1, √½, 1` (OCCT's
//! `Convert_SphereToBSplineSurface` has the same net over a cell).
//!
//! **The pole rows.** Cell `(k, −1)`'s first `v` row and cell `(k, 0)`'s
//! last are `ρ = 0`: their three control points are the pole, to the
//! bit: singular sides of the tile, which the fold check divides out.
//! Longitude cells `k` and `k + 4` are the same tile; the orientation
//! `x_u × x_v` points out of the sphere.
//!
//! **Level cap.** Both directions run around circles of the sphere's
//! radius: the longitude chord is taken at the equator, as a meridian's
//! (towards a pole a cell's longitude chord shrinks, which the pole
//! rows' singular sides, not the cap, account for).

use malachite::Rational;

use super::super::form::SphereKey;
use super::super::lattice::{
    ARC, Cells, ExactFrame, Frame, Lattice, LatticeError, Reach, Run, affine, arc, larger,
};
use super::super::round::q;
use super::super::surface::Surface;
use super::super::tile::Net;

/// The base tile's net of cell `(k, l)` (module docs); `Index` for a
/// latitude `l` other than −1 or 0.
pub(in crate::geom) fn net(
    f: &ExactFrame,
    key: &SphereKey,
    k: i64,
    l: i64,
) -> Result<Net, LatticeError> {
    // (ρ, z) on the unit meridian.
    let meridian: [(i32, i32); 3] = match l {
        -1 => [(0, -1), (1, -1), (1, 0)],
        0 => [(1, 0), (1, 1), (0, 1)],
        _ => return Err(LatticeError::Index),
    };
    let (o, [a0, a1, a2]) = (&f.origin, &f.axes);
    let around = arc(a0, a1, k);
    let r = q(key.radius());
    Ok(std::array::from_fn(|n| {
        let (dir, wu) = &around[n / 3];
        let (rho, z) = meridian[n % 3];
        let (rho, z) = (Rational::from(rho), Rational::from(z));
        let p = [0, 1, 2].map(|c| &o[c] + &r * (&rho * &dir[c] + &z * &a2[c]));
        affine(&p, wu.times(ARC[n % 3]).value())
    }))
}

/// Whether the latitude cells `v` are the sphere's (−1 and 0).
pub(in crate::geom) fn valid(v: &Cells) -> bool {
    *v.start() >= -1 && *v.end() <= 0
}

/// What the level cap reads of any cells: coordinates to the radius
/// along every axis, both directions around a circle of the radius
/// (module docs).
pub(in crate::geom) fn reach(f: &Frame, key: &SphereKey) -> Reach {
    let (o, [a0, a1, a2]) = (f.origin, f.axes);
    let r = key.radius();
    let m = (0..3)
        .map(|c| o[c].abs() + r * (a0[c].abs() + a1[c].abs() + a2[c].abs()))
        .fold(0.0, larger);
    Reach {
        m,
        rho: r,
        t: 0.0,
        runs: [Run::Turn(r), Run::Turn(r)],
    }
}

/// The sphere's 180° surface of longitude cells `first` and `first + 1`
/// from pole to pole (latitudes −1 and 0), its pole rows the tiles'
/// first and last `v` rows (module docs).
pub fn sphere(key: SphereKey, first: i64) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let rect = lattice.half(first)?;
    Surface::new(lattice, rect)
}

#[cfg(test)]
mod tests;
