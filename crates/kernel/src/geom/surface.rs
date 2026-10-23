//! A tiled surface: an integer rectangle of tiles on its form's lattice,
//! with the tree over their hull boxes.
//!
//! **Global UV.** A surface's points are named in its **global UV**, the
//! units of its rectangle's level: tile `(i, j)` owns `[i, i + 1] ×
//! [j, j + 1]`, and a point there is the tile's at the **local
//! coordinates** `(u − i, v − j)` ([`Surface::locate`]). A point on a
//! tile line belongs to the tile above it (the one whose side `0` the
//! line is), on the rectangle's far side to the last tile. The local
//! coordinate `x − i` is exact in `f64` (Sterbenz: `x` and `i` within a
//! factor of two, or `i = 0`) on every tile but `i = −1`, where `x` in
//! `(−½, 0)` has bits finer than `x + 1` can hold; there it is the
//! nearest `f64` with its residual as its certified rounding error. An
//! index past `2⁵³` in magnitude has no `f64` UV, so such a domain has no
//! points.
//!
//! **Shared rows** (bit-exact joins, the B-rep's invariant 2(d)). The
//! tiles are the lattice's ([`Lattice::tiles`]), so neighbours share
//! their boundary row, affine control points and weights, bit for bit,
//! corners included, by construction: each row is the rounded exact
//! sub-curve of one base tile's row. [`Surface::row`] gives the row on
//! a tile line, the same values from either side.
//!
//! **Tile-line evaluation.** Every evaluation goes through the tile the
//! point is located in, at its local coordinates. A local coordinate
//! exactly 0 or 1 makes the tile's patch evaluate its boundary row alone
//! (`varde_decide::bezier`'s end rule), so a point on a tile line is the
//! same bits whichever tile beside it is asked, and the same as the
//! row's own span evaluated there; a lattice node is its control point
//! exactly. Off the lines the surface is its tiles' patches.
//!
//! **Caches.** Each tile caches what it computes from its net alone, the
//! same bits on every copy ([`Tile`]: the first level's patch, the hull
//! box, the partials, the normal numerator and the cone with the level
//! that certified it). The surface keeps the tree over its tiles' hull
//! boxes, built with the tiles and as they are (uncharged, at
//! construction), leaf `k` the tile at position `k` of
//! [`Surface::tiles`]; a query of it is charged as the tree charges
//! every query, counted before its hits are collected.

use std::ops::Range;

use varde_decide::bezier::{Dir, HullBox, Partials};
use varde_decide::num::bounded::Bounded;
use varde_decide::num::expansion::two_sum;

use super::lattice::{Lattice, LatticeError, Rect};
use super::tile::{DEGREE, Tile};
use crate::infra::budget::{Exhausted, Ledger};
use crate::infra::bvh::{Aabb, Bvh};

/// A tile's boundary row: three affine control points `(x, y, z, w)`,
/// in the order of the line's direction.
pub type Row = [[f64; 4]; 3];

/// A tiled surface on its form's lattice (module docs).
#[derive(Clone, Debug)]
pub struct Surface {
    lattice: Lattice,
    rect: Rect,
    /// Row-major in `u`, as [`Lattice::tiles`] gives them.
    tiles: Vec<Tile>,
    /// The tree over the tiles' hull boxes, leaf `k` the tile `tiles[k]`.
    tree: Bvh<3>,
}

/// Where a point of global UV lies on a surface (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    /// The tile's index in `u`.
    pub i: i64,
    /// The tile's index in `v`.
    pub j: i64,
    /// The tile's position in [`Surface::tiles`].
    pub index: usize,
    /// The local coordinates `(u − i, v − j)`: exact but on a tile `−1`
    /// (module docs), `−0` as `+0`.
    pub local: [Bounded<f64>; 2],
}

impl Surface {
    /// The surface of `rect` on `lattice`, its tiles made
    /// ([`Lattice::tiles`]) and the tree over them built.
    pub fn new(lattice: Lattice, rect: Rect) -> Result<Surface, LatticeError> {
        let tiles = lattice.tiles(&rect)?;
        let boxes: Vec<Aabb<3>> = tiles.iter().map(|t| t.hull_box().into()).collect();
        let tree = Bvh::sorted_tree(&boxes);
        let s = Surface {
            lattice,
            rect,
            tiles,
            tree,
        };
        debug_assert!(s.rows_shared(), "neighbouring tiles share their rows");
        Ok(s)
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

    /// Every tile, row-major in `u`: tile `(i, j)` at
    /// [`index`](Self::index)`(i, j)`.
    #[inline]
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// The position of tile `(i, j)` in [`tiles`](Self::tiles),
    /// `(i − u.start)·v.len() + (j − v.start)`; `None` outside the
    /// rectangle.
    pub fn index(&self, i: i64, j: i64) -> Option<usize> {
        let (u, v) = (&self.rect.u, &self.rect.v);
        if !u.contains(&i) || !v.contains(&j) {
            return None;
        }
        // Within the rectangle, whose tile count fits a `usize`.
        let nv = v.end.abs_diff(v.start);
        let at = i.abs_diff(u.start) * nv + j.abs_diff(v.start);
        usize::try_from(at).ok()
    }

    /// The indices `(i, j)` of the tile at position `k` of
    /// [`tiles`](Self::tiles); `None` past the last.
    pub fn indices(&self, k: usize) -> Option<(i64, i64)> {
        if k >= self.tiles.len() {
            return None;
        }
        let (u, v) = (&self.rect.u, &self.rect.v);
        // Positions and offsets within the rectangle fit an `i64`.
        let nv = v.end.abs_diff(v.start);
        let k = k as u64;
        Some((u.start + (k / nv) as i64, v.start + (k % nv) as i64))
    }

    /// The tile `(i, j)`, `None` outside the rectangle.
    pub fn tile(&self, i: i64, j: i64) -> Option<&Tile> {
        self.tiles.get(self.index(i, j)?)
    }

    /// The tile holding global `uv` and the local coordinates there
    /// (module docs); `None` outside the domain or for a NaN.
    pub fn locate(&self, uv: [f64; 2]) -> Option<Located> {
        let (i, s) = cell(uv[0], &self.rect.u)?;
        let (j, t) = cell(uv[1], &self.rect.v)?;
        Some(Located {
            i,
            j,
            index: self.index(i, j)?,
            local: [s, t],
        })
    }

    /// The point at global `uv`, certified, `None` outside the domain:
    /// on the tile [`locate`](Self::locate) names, at its local
    /// coordinates; on a tile line from the row there, at a node its
    /// control point exactly (module docs).
    pub fn eval(&self, uv: [f64; 2]) -> Option<[Bounded<f64>; 3]> {
        let at = self.locate(uv)?;
        let tile = &self.tiles[at.index];
        let [s, t] = at.local;
        Some(if exact(&s) && exact(&t) {
            tile.eval(s.v, t.v)
        } else {
            tile.patch().eval(&s, &t)
        })
    }

    /// The point and its partials in global `u` and `v` at `uv`,
    /// certified, `None` outside the domain: the tile's
    /// ([`Tile::eval_partials`]) at its local coordinates, a unit step of
    /// which is one of global UV. On a tile line the point is the row's;
    /// the partials are the located tile's.
    pub fn eval_partials(&self, uv: [f64; 2]) -> Option<Partials<Bounded<f64>>> {
        let at = self.locate(uv)?;
        let tile = &self.tiles[at.index];
        let [s, t] = at.local;
        Some(if exact(&s) && exact(&t) {
            tile.eval_partials(s.v, t.v)
        } else {
            tile.patch().eval_partials(&s, &t)
        })
    }

    /// The boundary row on a tile line across one tile: `along` [`Dir::V`],
    /// the line `u = at` over `v ∈ [cell, cell + 1]` (tile `(at, cell)`'s
    /// control points `(0, ·)`, or tile `(at − 1, cell)`'s `(2, ·)`), in
    /// increasing `v`; `along` [`Dir::U`], the line `v = at` over
    /// `u ∈ [cell, cell + 1]` (tile `(cell, at)`'s `(·, 0)`, or tile
    /// `(cell, at − 1)`'s `(·, 2)`), in increasing `u`. The same values
    /// from either tile (module docs); `None` where neither is in the
    /// rectangle.
    pub fn row(&self, along: Dir, at: i64, cell: i64) -> Option<Row> {
        let (first, last) = match along {
            Dir::V => (
                self.tile(at, cell),
                at.checked_sub(1).and_then(|i| self.tile(i, cell)),
            ),
            Dir::U => (
                self.tile(cell, at),
                at.checked_sub(1).and_then(|j| self.tile(cell, j)),
            ),
        };
        match (first, last) {
            (Some(t), _) => Some(row_of(t, along, 0)),
            (None, Some(t)) => Some(row_of(t, along, DEGREE)),
            (None, None) => None,
        }
    }

    /// A certified box holding the whole surface: the least box holding
    /// its tiles' hull boxes.
    pub fn hull_box(&self) -> HullBox {
        let mut b = HullBox {
            lo: [f64::INFINITY; 3],
            hi: [f64::NEG_INFINITY; 3],
        };
        for t in &self.tiles {
            let h = t.hull_box();
            for k in 0..3 {
                if h.lo[k] < b.lo[k] {
                    b.lo[k] = h.lo[k];
                }
                if h.hi[k] > b.hi[k] {
                    b.hi[k] = h.hi[k];
                }
            }
        }
        b
    }

    /// The tree over the tiles' hull boxes, leaf `k` the tile at position
    /// `k` of [`tiles`](Self::tiles) (module docs).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the broad phase reads it as it lands")
    )]
    #[inline]
    pub(crate) fn tree(&self) -> &Bvh<3> {
        &self.tree
    }

    /// The positions in [`tiles`](Self::tiles) of the tiles whose hull
    /// boxes meet `query`, ascending: the tree's query
    /// ([`Bvh::hits`]), each box test charged to `ledger` as it is made
    /// and the hits before they are collected.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the broad phase calls it as it lands")
    )]
    pub(crate) fn tiles_meeting(
        &self,
        query: &Aabb<3>,
        ledger: &mut Ledger<'_>,
    ) -> Result<Vec<u32>, Exhausted> {
        self.tree.hits(query, ledger)
    }

    /// Whether every two neighbouring tiles share their row bit for bit.
    fn rows_shared(&self) -> bool {
        let bits = |r: Row| r.map(|p| p.map(f64::to_bits));
        (0..self.tiles.len()).all(|k| {
            let Some((i, j)) = self.indices(k) else {
                return false;
            };
            let t = &self.tiles[k];
            let up = self
                .tile(i + 1, j)
                .is_none_or(|n| bits(row_of(t, Dir::V, DEGREE)) == bits(row_of(n, Dir::V, 0)));
            let right = self
                .tile(i, j + 1)
                .is_none_or(|n| bits(row_of(t, Dir::U, DEGREE)) == bits(row_of(n, Dir::U, 0)));
            up && right
        })
    }
}

/// The row of `t` running `along` at index `at` of the other direction.
fn row_of(t: &Tile, along: Dir, at: usize) -> Row {
    let net = t.net();
    std::array::from_fn(|k| match along {
        Dir::V => net[3 * at + k],
        Dir::U => net[3 * k + at],
    })
}

/// Whether `b` is exact.
fn exact(b: &Bounded<f64>) -> bool {
    b.err_trunc == 0.0 && b.err_round == 0.0
}

/// The tile index holding `x` in `range` (the last one at its end) and
/// the local coordinate `x − i`, certified (module docs); `None` outside
/// the range.
fn cell(x: f64, range: &Range<i64>) -> Option<(i64, Bounded<f64>)> {
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
    // `x − i` with its residual, exact; `+ 0.0` makes `−0` `+0`.
    let (s, r) = two_sum(x, -(i as f64));
    let s = s + 0.0;
    Some((i, Bounded::new(s, 0.0, r.abs())))
}

#[cfg(test)]
mod tests;
