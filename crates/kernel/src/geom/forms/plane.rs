//! The plane's tiles: cell `(k, l)` is `[(k − ½)·CELL, (k + ½)·CELL]`
//! along the frame's `u` times the same in `l` along `v`, its tile the
//! cell's bilinear parallelogram elevated to `(2, 2)`, weights 1, the
//! orientation `x_u × x_v` the key's normal. Cell `(0, 0)` holds the
//! plane's whole part of the coordinate box, so every plane face is one
//! tile ([`surface`]).

use malachite::Rational;

use super::super::form::PlaneKey;
use super::super::lattice::{
    CELL, Cells, ExactFrame, Frame, Lattice, LatticeError, Reach, Run, affine, halves, larger,
    line_reach,
};
use super::super::round::q;
use super::super::surface::Surface;
use super::super::tile::Net;

/// The base tile's net of cell `(k, l)` (module docs); `Index` where an
/// index's arithmetic overflows.
pub(in crate::geom) fn net(f: &ExactFrame, k: i64, l: i64) -> Result<Net, LatticeError> {
    let (h, g) = (halves(k)?, halves(l)?);
    let (o, [a0, a1, _]) = (&f.origin, &f.axes);
    let half = q(CELL) / Rational::from(2u32);
    Ok(std::array::from_fn(|n| {
        let s = Rational::from(h[n / 3]) * &half;
        let t = Rational::from(g[n % 3]) * &half;
        let p = [0, 1, 2].map(|c| &o[c] + &s * &a0[c] + &t * &a1[c]);
        affine(&p, 1.0)
    }))
}

/// What the level cap reads of the base cells `u` × `v`: coordinates
/// to the cells' reach along both axes, no radius, both directions
/// lines.
pub(in crate::geom) fn reach(f: &Frame, u: &Cells, v: &Cells) -> Reach {
    let (o, [a0, a1, _]) = (f.origin, f.axes);
    let (ru, rv) = (line_reach(u), line_reach(v));
    let m = (0..3)
        .map(|c| o[c].abs() + ru * a0[c].abs() + rv * a1[c].abs())
        .fold(0.0, larger);
    Reach {
        m,
        rho: 0.0,
        t: larger(ru, rv),
        runs: [Run::Line, Run::Line],
    }
}

/// The plane's surface: its one tile, cell `(0, 0)` (module docs).
pub fn surface(key: PlaneKey) -> Result<Surface, LatticeError> {
    let lattice = Lattice::new(key.into())?;
    let rect = lattice.half(0)?;
    Surface::new(lattice, rect)
}

#[cfg(test)]
mod tests;
