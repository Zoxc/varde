//! Form-anchored lattices: the grid of tiles every surface of one form
//! lies on, fixed by the form's key alone, so coplanar faces and coaxial
//! equal forms from any feature share their tiles bit for bit. So far
//! the plane's, the circular cylinder's and the sphere's.
//!
//! **Frames** (a function of the stored [`FormKey`], never chosen by a
//! constructor). The reference direction of a unit direction `n` is
//! `normalise(e_k × n)`, `e_k` the world axis of least `|n·e_k|` (ties:
//! the lowest `k`), each component rounded once from its exact value.
//! - plane: origin the foot of the world origin, `d·n/|n|²`; axes `u` the
//!   reference direction of `n` and `v = n × u`, rounded once from the
//!   stored `n` and `u` (`u × v` is `n`);
//! - cylinder and sphere: origin the foot point or centre; axes `e₁` the
//!   reference direction of the axis `a`, `e₂ = a × e₁` (rounded once),
//!   and `a`.
//!
//! [`Lattice::origin`] and [`Lattice::axes`] give them.
//!
//! **Cells** at the base level, cell `(k, l)` (`u` along the first index):
//! - plane: `[(k − ½)·CELL, (k + ½)·CELL]` along `u` times the same in `l`
//!   along `v`: cell `(0, 0)` is centred on the origin, and as [`CELL`]
//!   is at least `4·MAX_COORD`, it holds the plane's whole part of the
//!   coordinate box (whose points lie within `√3·MAX_COORD` of the
//!   origin): every plane face is one tile. Its tile is the cell's
//!   bilinear parallelogram elevated to `(2, 2)`, weights 1.
//! - cylinder: angle `[k, k + 1]·90°` from `e₁` towards `e₂` times the
//!   axial cell `[(l − ½)·CELL, (l + ½)·CELL]` from the foot point along
//!   `a`: a quarter circle (control points the cell's two radii's ends
//!   and their corner, weights `1, √½, 1`) times a line.
//! - sphere: longitude as the cylinder's angle times latitude
//!   `[l, l + 1]·90°`, `l` −1 (south) or 0 (north): the tensor product of
//!   the longitude's quarter circle and the meridian's (weights the
//!   products of `1, √½, 1`), the pole a whole row of the tile.
//!
//! The tile's `u` runs along the first index, `v` along the second, and
//! its orientation `x_u × x_v` is `n` on a plane and points out of a
//! cylinder or sphere. Angle cells `k` and `k + 4` are the same tile.
//!
//! **Control points** are exact functions of the frame's stored values
//! and the cell, stored affine with their weights: each coordinate of
//! the point and each weight rounded once to the nearest `f64`, ties to
//! even (a weight `√½` rounded, `½` and 1 exact). So a corner of a
//! cylinder's or sphere's tile is its rounded point, and a sphere's pole
//! row is one point to the bit. A boundary row is the same exact values
//! in both cells beside it, so neighbours share it bit for bit, weights
//! included, and every cell of one key is the same bits on every surface.
//!
//! **Levels.** A surface may use a finer level `(L_u, L_v)`: each base
//! cell split `L_u` times at ½ in `u` and `L_v` times in `v`, tile
//! `(i, j)` lying in base cell `(i >> L_u, j >> L_v)`. A level's tile is
//! **its stored base tile's exact sub-patch, each control point and
//! weight rounded once** to the nearest `f64`, ties to even: the exact
//! splits ([`Tile::split_half`] without its rounding) on the base tile's
//! exact homogeneous net, the higher halves where `i`'s (and `j`'s) bits
//! from the top say 1, rounded at the end. Exact splits in `u` and `v`
//! commute, so the tile has no route to fix, and its precision is that of
//! its own coordinates, not of the base tile's: a fine tile near a
//! cylinder's foot point keeps its shape though its base cell reaches
//! `CELL/2` along the axis. Neighbours at one level share their boundary
//! rows bit for bit: the exact sub-patches of one base net, or of two
//! sharing a row, have the same exact control points on a shared line,
//! which round alike.
//!
//! **Surfaces** are integer rectangles of one level ([`Rect`]); a 180°
//! surface (the canonical constructors' choice, [`Lattice::half`]) is two
//! angle cells by one axial cell on a cylinder, two by two on a sphere
//! (pole to pole), the one tile of a plane. An angle span must be less
//! than a full turn, so no surface meets itself.

use std::ops::Range;

use malachite::Rational;
use malachite::base::num::basic::traits::One;
use varde_decide::bezier::{Dir, HomPatch};

use super::form::{FormKey, dot};
use super::round::{nearest, nearest_root, nearest_sqrt, q};
use super::tile::{Net, Tile, TileError};

/// A tile's exact homogeneous patch.
type Exact = HomPatch<Rational>;

/// The largest coordinate magnitude of a body's bounded positions (mm).
pub const MAX_COORD: f64 = 1e6;

/// The edge of a plane's cell and of a cylinder's axial cell,
/// `2^⌈log₂(4·MAX_COORD)⌉ = 2²²`.
pub const CELL: f64 = 4_194_304.0;

// The least power of two at least `4·MAX_COORD`.
const _: () = assert!(CELL / 2.0 < 4.0 * MAX_COORD && 4.0 * MAX_COORD <= CELL);

/// The finest level in each direction: a level's index within its base
/// cell, times `2^−L`, is exact in `f64`.
pub const MAX_LEVEL: u32 = 52;

/// A lattice level: the number of ½ splits of the base cells in `u` and
/// in `v`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Level {
    pub u: u32,
    pub v: u32,
}

impl Level {
    /// The base level, `(0, 0)`.
    pub const BASE: Level = Level { u: 0, v: 0 };
}

/// A surface's domain on its lattice: the tiles `(i, j)` of `level`
/// with `i` in `u` and `j` in `v` (half-open).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rect {
    pub level: Level,
    pub u: Range<i64>,
    pub v: Range<i64>,
}

/// Why a lattice, tile or rectangle can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatticeError {
    /// A level above [`MAX_LEVEL`].
    Level,
    /// A cell outside the lattice (a sphere's latitude beyond the poles),
    /// an index whose arithmetic overflows, or a rectangle with more
    /// tiles than memory holds.
    Index,
    /// An empty rectangle, or one whose angle span is a full turn or more.
    Span,
    /// A frame value, rounded, is not finite.
    Range,
    /// A tile's net is refused: a coordinate overflows.
    Tile(TileError),
}

impl From<TileError> for LatticeError {
    fn from(e: TileError) -> Self {
        LatticeError::Tile(e)
    }
}

/// A form's lattice: its key and the frame fixed by it (module docs).
#[derive(Clone, Debug)]
pub struct Lattice {
    key: FormKey,
    origin: [f64; 3],
    axes: [[f64; 3]; 3],
}

/// A weight of the lattices' tiles: `1`, `√½` or `½`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Weight {
    One,
    Root,
    Half,
}

impl Weight {
    fn times(self, o: Weight) -> Weight {
        match (self, o) {
            (Weight::One, w) | (w, Weight::One) => w,
            (Weight::Root, Weight::Root) => Weight::Half,
            _ => unreachable!("lattice weights multiply at most two roots"),
        }
    }
}

/// The weights of a quarter circle's control points.
const ARC: [Weight; 3] = [Weight::One, Weight::Root, Weight::One];

impl Lattice {
    /// The lattice of `key`'s form, its frame computed from the stored
    /// key.
    pub fn new(key: FormKey) -> Result<Lattice, LatticeError> {
        let (origin, axes) = match &key {
            FormKey::Plane(k) => {
                let n = k.normal();
                let nq = n.map(q);
                let t = q(k.offset()) / dot(&nq, &nq);
                let origin = nq.clone().map(|x| nearest(&(&t * x)));
                let u = reference(&n);
                let v = cross(&nq, &u.map(q)).map(|x| nearest(&x));
                (origin, [u, v, n])
            }
            FormKey::Cylinder(k) => (k.foot(), revolution(&k.axis())),
            FormKey::Sphere(k) => (k.centre(), revolution(&k.axis())),
        };
        if !origin
            .iter()
            .chain(axes.iter().flatten())
            .all(|x| x.is_finite())
        {
            return Err(LatticeError::Range);
        }
        Ok(Lattice { key, origin, axes })
    }

    /// The key the lattice is anchored to.
    #[inline]
    pub fn key(&self) -> &FormKey {
        &self.key
    }

    /// The frame's origin: the plane's foot of the world origin, the
    /// cylinder's foot point, the sphere's centre.
    #[inline]
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// The frame's axes: `[u, v, n]` on a plane, `[e₁, e₂, a]` on a
    /// cylinder or sphere (module docs).
    #[inline]
    pub fn axes(&self) -> [[f64; 3]; 3] {
        self.axes
    }

    /// The base level's tile of cell `(k, l)` (module docs).
    pub fn base_tile(&self, k: i64, l: i64) -> Result<Tile, LatticeError> {
        let o = self.origin.map(q);
        let [a0, a1, a2] = self.axes.map(|a| a.map(q));
        let net: Net = match &self.key {
            FormKey::Plane(_) => {
                let (h, g) = (halves(k)?, halves(l)?);
                let half = q(CELL) / Rational::from(2u32);
                std::array::from_fn(|n| {
                    let s = Rational::from(h[n / 3]) * &half;
                    let t = Rational::from(g[n % 3]) * &half;
                    let p = [0, 1, 2].map(|c| &o[c] + &s * &a0[c] + &t * &a1[c]);
                    affine(&p, Weight::One)
                })
            }
            FormKey::Cylinder(c) => {
                let around = arc(&a0, &a1, k);
                let rho = q(c.radius());
                let g = halves(l)?;
                let half = q(CELL) / Rational::from(2u32);
                std::array::from_fn(|n| {
                    let (dir, w) = &around[n / 3];
                    let t = Rational::from(g[n % 3]) * &half;
                    let p = [0, 1, 2].map(|c| &o[c] + &rho * &dir[c] + &t * &a2[c]);
                    affine(&p, *w)
                })
            }
            FormKey::Sphere(s) => {
                // (ρ, z) on the unit meridian, and the weights.
                let meridian: [(i32, i32); 3] = match l {
                    -1 => [(0, -1), (1, -1), (1, 0)],
                    0 => [(1, 0), (1, 1), (0, 1)],
                    _ => return Err(LatticeError::Index),
                };
                let around = arc(&a0, &a1, k);
                let r = q(s.radius());
                std::array::from_fn(|n| {
                    let (dir, wu) = &around[n / 3];
                    let (rho, z) = meridian[n % 3];
                    let (rho, z) = (Rational::from(rho), Rational::from(z));
                    let p = [0, 1, 2].map(|c| &o[c] + &r * (&rho * &dir[c] + &z * &a2[c]));
                    affine(&p, wu.times(ARC[n % 3]))
                })
            }
        };
        Ok(Tile::new(net)?)
    }

    /// The tile `(i, j)` of `level`: its base tile's exact sub-patch,
    /// rounded once (module docs).
    pub fn tile(&self, level: Level, i: i64, j: i64) -> Result<Tile, LatticeError> {
        if level.u > MAX_LEVEL || level.v > MAX_LEVEL {
            return Err(LatticeError::Level);
        }
        let base = self.base_tile(i >> level.u, j >> level.v)?;
        if level == Level::BASE {
            return Ok(base);
        }
        let t = descend(base.exact(), Dir::U, level.u, i);
        Ok(Tile::rounded(&descend(t, Dir::V, level.v, j))?)
    }

    /// The tiles of `rect`, row-major in `u` (tile `(i, j)` at
    /// `(i − u.start)·v.len() + (j − v.start)`): the same bits as
    /// [`tile`](Self::tile) gives each, a base tile's splits shared.
    pub fn tiles(&self, rect: &Rect) -> Result<Vec<Tile>, LatticeError> {
        let Level { u: lu, v: lv } = rect.level;
        if lu > MAX_LEVEL || lv > MAX_LEVEL {
            return Err(LatticeError::Level);
        }
        let (nu, nv) = (span(&rect.u)?, span(&rect.v)?);
        if !matches!(self.key, FormKey::Plane(_)) && nu >= 4u64 << lu {
            return Err(LatticeError::Span);
        }
        let count = usize::try_from(nu)
            .ok()
            .and_then(|a| usize::try_from(nv).ok().and_then(|b| a.checked_mul(b)))
            .ok_or(LatticeError::Index)?;
        let mut out: Vec<Option<Tile>> = Vec::new();
        out.try_reserve_exact(count)
            .map_err(|_| LatticeError::Index)?;
        out.resize(count, None);
        for k in (rect.u.start >> lu)..=((rect.u.end - 1) >> lu) {
            for l in (rect.v.start >> lv)..=((rect.v.end - 1) >> lv) {
                let base = self.base_tile(k, l)?;
                if rect.level == Level::BASE {
                    // Within the rectangle, as below.
                    let at = k.abs_diff(rect.u.start) * nv + l.abs_diff(rect.v.start);
                    out[at as usize] = Some(base);
                    continue;
                }
                for (i, column) in split_all(base.exact(), Dir::U, lu, k << lu, &rect.u) {
                    for (j, t) in split_all(column, Dir::V, lv, l << lv, &rect.v) {
                        // Below `count` by construction: i and j lie in
                        // the rectangle.
                        let at = i.abs_diff(rect.u.start) * nv + j.abs_diff(rect.v.start);
                        out[at as usize] = Some(Tile::rounded(&t)?);
                    }
                }
            }
        }
        Ok(out
            .into_iter()
            .map(|t| t.expect("every tile of the rectangle is made"))
            .collect())
    }

    /// The canonical 180° surface's rectangle at the base level, its
    /// angle cells `first` and `first + 1`: the angle cells times axial
    /// cell 0 on a cylinder, times latitudes −1 and 0 on a sphere; the
    /// plane's one cell `(0, 0)` (`first` unused).
    pub fn half(&self, first: i64) -> Result<Rect, LatticeError> {
        let next = first.checked_add(2).ok_or(LatticeError::Index)?;
        Ok(match self.key {
            FormKey::Plane(_) => Rect {
                level: Level::BASE,
                u: 0..1,
                v: 0..1,
            },
            FormKey::Cylinder(_) => Rect {
                level: Level::BASE,
                u: first..next,
                v: 0..1,
            },
            FormKey::Sphere(_) => Rect {
                level: Level::BASE,
                u: first..next,
                v: -1..1,
            },
        })
    }
}

/// The node indices, in half cells, of cell `k`'s three control rows:
/// `2k − 1`, `2k`, `2k + 1`.
fn halves(k: i64) -> Result<[i64; 3], LatticeError> {
    let m = k.checked_mul(2).ok_or(LatticeError::Index)?;
    let lo = m.checked_sub(1).ok_or(LatticeError::Index)?;
    let hi = m.checked_add(1).ok_or(LatticeError::Index)?;
    Ok([lo, m, hi])
}

/// The control directions and weights of angle cell `k`'s quarter
/// circle in the unit frame `e₁, e₂`: the radii at `k·90°` and
/// `(k + 1)·90°` and their sum (the tangents' corner), exact. Period 4.
fn arc(e1: &[Rational; 3], e2: &[Rational; 3], k: i64) -> [([Rational; 3], Weight); 3] {
    let neg = |x: &[Rational; 3]| x.clone().map(|c| -c);
    let radii = [e1.clone(), e2.clone(), neg(e1), neg(e2)];
    let m = k.rem_euclid(4) as usize;
    let (a, b) = (&radii[m], &radii[(m + 1) % 4]);
    let corner = [0, 1, 2].map(|c| &a[c] + &b[c]);
    [
        (a.clone(), Weight::One),
        (corner, Weight::Root),
        (b.clone(), Weight::One),
    ]
}

/// The affine control point `(p, w)`, each value rounded once.
fn affine(p: &[Rational; 3], w: Weight) -> [f64; 4] {
    let [x, y, z] = p.each_ref().map(nearest);
    let w = match w {
        Weight::One => 1.0,
        Weight::Root => nearest_sqrt(&Rational::from_signeds(1, 2)),
        Weight::Half => 0.5,
    };
    [x, y, z, w]
}

/// The reference direction of the unit `n` (module docs).
fn reference(n: &[f64; 3]) -> [f64; 3] {
    let mut k = 0;
    for c in 1..3 {
        if n[c].abs() < n[k].abs() {
            k = c;
        }
    }
    let mut e = [0.0; 3];
    e[k] = 1.0;
    let c = cross(&e.map(q), &n.map(q));
    let inv = Rational::ONE / dot(&c, &c);
    c.each_ref().map(|x| nearest_root(x, &inv))
}

/// `[e₁, e₂, a]` of a cylinder's or sphere's unit axis `a`.
fn revolution(a: &[f64; 3]) -> [[f64; 3]; 3] {
    let e1 = reference(a);
    let e2 = cross(&a.map(q), &e1.map(q)).map(|x| nearest(&x));
    [e1, e2, *a]
}

fn cross(a: &[Rational; 3], b: &[Rational; 3]) -> [Rational; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

/// The number of indices in `r`, non-zero.
fn span(r: &Range<i64>) -> Result<u64, LatticeError> {
    if r.start >= r.end {
        return Err(LatticeError::Span);
    }
    Ok(r.end.abs_diff(r.start))
}

/// `t` split exactly `levels` times in `dir` towards index `i`'s
/// sub-cell: the higher half where `i`'s low `levels` bits, from the
/// top, are 1.
fn descend(mut t: Exact, dir: Dir, levels: u32, i: i64) -> Exact {
    for s in (0..levels).rev() {
        let [lo, hi] = t.split_half(dir);
        t = if (i >> s) & 1 == 1 { hi } else { lo };
    }
    t
}

/// The exact sub-patches of `t` (whose first index at `levels` below is
/// `first`) after `levels` splits in `dir`, those with indices in
/// `want`, in order, each with its index: the values [`descend`] gives
/// each.
fn split_all(t: Exact, dir: Dir, levels: u32, first: i64, want: &Range<i64>) -> Vec<(i64, Exact)> {
    let mut out = Vec::new();
    // (tile, its first index, its width in indices)
    let mut stack = vec![(t, first, 1i64 << levels)];
    while let Some((t, at, width)) = stack.pop() {
        let end = at.saturating_add(width);
        if end <= want.start || at >= want.end {
            continue;
        }
        if width == 1 {
            out.push((at, t));
            continue;
        }
        let [lo, hi] = t.split_half(dir);
        let w = width / 2;
        // The lower half last, so it is popped first.
        stack.push((hi, at + w, w));
        stack.push((lo, at, w));
    }
    out
}

#[cfg(test)]
mod tests;
