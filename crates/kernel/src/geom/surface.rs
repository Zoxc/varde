//! A tiled surface: an integer rectangle of tiles on its form's lattice,
//! in the surface's **global UV**, the lattice's level units: tile
//! `(i, j)` of the rectangle's level owns `[i, i + 1] × [j, j + 1]`, and a
//! point there is the tile's at the local coordinates `(u − i, v − j)`.
//! Neighbouring tiles share their boundary rows bit for bit, so a point
//! on a tile line is the same from either side to within the evaluation's
//! certified error (and to the bit where the local coordinate is exactly
//! 0 or 1).
//!
//! So far the surface of an exact form only: its lattice and rectangle,
//! with the tiles made once.

use varde_decide::num::bounded::Bounded;

use super::lattice::{Lattice, LatticeError, Rect};
use super::tile::Tile;

/// A tiled surface on its form's lattice (module docs).
#[derive(Clone, Debug)]
pub struct Surface {
    lattice: Lattice,
    rect: Rect,
    /// Row-major in `u`, as [`Lattice::tiles`] gives them.
    tiles: Vec<Tile>,
}

impl Surface {
    /// The surface of `rect` on `lattice`, its tiles made
    /// ([`Lattice::tiles`]).
    pub fn new(lattice: Lattice, rect: Rect) -> Result<Surface, LatticeError> {
        let tiles = lattice.tiles(&rect)?;
        Ok(Surface {
            lattice,
            rect,
            tiles,
        })
    }

    /// The lattice, and with it the form's key.
    #[inline]
    pub fn lattice(&self) -> &Lattice {
        &self.lattice
    }

    /// The domain: the rectangle of tiles at its level.
    #[inline]
    pub fn rect(&self) -> &Rect {
        &self.rect
    }

    /// The tile `(i, j)`, `None` outside the rectangle.
    pub fn tile(&self, i: i64, j: i64) -> Option<&Tile> {
        let (u, v) = (&self.rect.u, &self.rect.v);
        if !u.contains(&i) || !v.contains(&j) {
            return None;
        }
        // Within the rectangle, whose tile count fits a `usize`.
        let nv = v.end.abs_diff(v.start);
        let at = i.abs_diff(u.start) * nv + j.abs_diff(v.start);
        self.tiles.get(usize::try_from(at).ok()?)
    }

    /// The point at global `uv`, certified, `None` outside the domain:
    /// on the tile `(⌊u⌋, ⌊v⌋)` (on the domain's far side the last tile),
    /// at the local coordinates `uv − (i, j)`, certified.
    pub fn eval(&self, uv: [f64; 2]) -> Option<[Bounded<f64>; 3]> {
        let (i, s) = cell(uv[0], &self.rect.u)?;
        let (j, t) = cell(uv[1], &self.rect.v)?;
        let tile = self.tile(i, j)?;
        Some(tile.patch().eval(&s, &t))
    }
}

/// The tile index holding `x` in `range` (the last one at its end) and
/// the local coordinate `x − i`, certified; `None` outside the range.
fn cell(x: f64, range: &std::ops::Range<i64>) -> Option<(i64, Bounded<f64>)> {
    // Indices are exact in `f64` up to 2⁵³; a domain past that has no
    // `f64` UV of its own.
    const EXACT: i64 = 1 << 53;
    if !(-EXACT..=EXACT).contains(&range.start) || !(-EXACT..=EXACT).contains(&range.end) {
        return None;
    }
    let (lo, hi) = (range.start as f64, range.end as f64);
    // False for a NaN.
    if !(lo <= x && x <= hi) {
        return None;
    }
    let i = (x.floor() as i64).min(range.end - 1);
    let s = Bounded::exact(x) - Bounded::exact(i as f64);
    Some((i, s))
}
