//! Form-anchored lattices: the grid of tiles every surface of one form
//! lies on, fixed by the form's key alone, so coplanar faces and coaxial
//! equal forms from any feature share their tiles bit for bit: every
//! form's, the plane's, the circular and conic-chain cylinders', the
//! cone's, the sphere's, the ring and spindle torus's, the revolved
//! conic's and the affine images of all of them.
//!
//! **Frames** (a function of the stored [`FormKey`], never chosen by a
//! constructor; [`frame`]). The reference direction of a unit direction
//! `n` is `normalise(e_k × n)`, `e_k` the world axis of least `|n·e_k|`
//! (ties: the lowest `k`), each component rounded once from its exact
//! value.
//! - plane: origin the foot of the world origin, `d·n/|n|²`; axes `u` the
//!   reference direction of `n` and `v = n × u`, rounded once from the
//!   stored `n` and `u` (`u × v` is `n`);
//! - forms about an axis `a` (cylinder, conic-chain cylinder, cone,
//!   sphere, torus, revolved conic): axes `e₁` the reference direction
//!   of `a`, `e₂ = a × e₁` (rounded once), and `a`; origin the foot point
//!   (cylinder, revolved conic), the apex (cone), the centre (sphere,
//!   torus), the world origin (conic-chain cylinder, whose profile lies
//!   in the plane through it: `e₁`, `e₂` are its `x`, `y` directions to
//!   rounding);
//! - affine image: its source's frame mapped, the origin `M·o + t` and
//!   each axis `M·e`, each component rounded once from its exact value
//!   (axes not unit).
//!
//! [`Lattice::origin`] and [`Lattice::axes`] give them.
//!
//! **Cells** at the base level, cell `(k, l)` (`u` along the first index;
//! each form's constructor in [`super::forms`] gives its net):
//! - plane ([`super::forms::plane`]): `[(k − ½)·CELL, (k + ½)·CELL]`
//!   along `u` times the same in `l` along `v`: cell `(0, 0)` is centred
//!   on the origin, and as [`CELL`] is at least `4·MAX_COORD`, it holds
//!   the plane's whole part of the coordinate box (whose points lie within
//!   `√3·MAX_COORD` of the origin): every plane face is one tile, the
//!   cell's bilinear parallelogram elevated to `(2, 2)`, weights 1.
//! - cylinder ([`super::forms::cyl`]): angle `[k, k + 1]·90°` from `e₁`
//!   towards `e₂` times the axial cell `[(l − ½)·CELL, (l + ½)·CELL]`
//!   from the foot point along `a`: a quarter circle (weights `1, √½,
//!   1`) times a line.
//! - conic-chain cylinder ([`super::forms::cyl`]): the profile's span `k`
//!   (`0 ≤ k <` its number of spans) times the cylinder's axial cells from
//!   the world origin, the span's weights times 1.
//! - cone ([`super::forms::cone`]): the cylinder's angle cells times the
//!   slant cell `[l, l + 1]·CELL`, `l ≥ 0`, of the distance from the apex
//!   along the generators: cell `(k, 0)`'s first `v` row is the apex.
//! - sphere ([`super::forms::sphere`]): longitude as the cylinder's angle
//!   times latitude `[l, l + 1]·90°`, `l` −1 (south) or 0 (north): the
//!   tensor product of the longitude's quarter circle and the meridian's
//!   (weights the products of `1, √½, 1`), the pole a whole row of the
//!   tile.
//! - torus ([`super::forms::torus`]): the cylinder's angle cells times
//!   tube arcs from the outer equator, 90° cells on a ring torus, on a
//!   spindle torus the two halves `[−φ*, 0]`, `[0, φ*]` of its outer part
//!   (`cos φ* = −R/r`), whose ends on the axis, the apexes, are whole
//!   rows of the tiles; a spindle has no cell on its inner part.
//! - revolved conic ([`super::forms::revolved`]): the cylinder's angle
//!   cells times the profile's span `l` (`0 ≤ l <` its number of spans),
//!   the tensor product of the quarter circle and the span; a span's end
//!   on the axis is a whole row of the tiles (a pole or an apex), and a
//!   span tangent to the axis (a cusp) has no cell.
//! - affine image ([`super::forms::affine`]): its source's cells, each
//!   base tile the source's with its control points mapped.
//!
//! The tile's `u` runs along the first index, `v` along the second, and
//! its orientation `x_u × x_v` is `n` on a plane, points out of a
//! cylinder, cone, sphere or torus, and is the profile's direction times
//! the axis on a conic-chain cylinder. Angle cells `k` and `k + 4` are the
//! same tile, as a ring torus's tube cells `l` and `l + 4`.
//!
//! **Control points** are exact functions of the key's and the frame's
//! stored values and the cell, stored affine with their weights: each
//! coordinate of the point and each weight rounded once to the nearest
//! `f64`, ties to even (a weight `√½` rounded, `½`, 1 and a profile's
//! weights exact; a spindle torus's coordinates and middle weights are
//! quadratic irrationals, rounded once all the same). So a corner of a
//! cylinder's or sphere's tile is its rounded point, and a sphere's pole
//! row and a cone's or spindle torus's apex row are one point to the
//! bit.
//!
//! **Singular sides.** Those rows on the axis (a sphere's poles, a
//! cone's apex, a spindle torus's apexes, a revolved conic's poles and
//! apexes where a span ends on the axis) are declared singular sides of
//! their tiles ([`Lattice::singular`]; [`Tile::with_singular`]), on the
//! tiles of every level whose cell reaches the row: the fold check
//! proves them on the exact net and divides their factors out of the
//! normal numerator, so these tiles certify regular. An affine image's
//! are its source's. A boundary row is the same exact values
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
//! **Level cap.** As a cell shrinks towards the spacing of its tile's
//! coordinates, the tile can still certify regular yet no longer follow
//! the form's tangent. So each direction `d` has a deepest admitted
//! level ([`Lattice::cap`]): the largest `L` up to [`MAX_LEVEL`] with
//! `ℓ_d(L) ≥ K_CAP·δ`, at least the base level. Here [`K_CAP`] is `2⁶`
//! (the tile's tangents within `2⁻⁵` rad of the form's), `ℓ_d(L)` a lower
//! bound on the chord of a level-`L` cell along `d` (the cell's extent
//! along a line; `2ρ·sin(90°·2^−L/2)` around a circle of radius `ρ`, the
//! form's radius, on a cone the radius at the cells' far row, on a torus
//! its tube's radius along it and around the axis the least radius of a
//! ring's cells or a spindle's outer equator; along a
//! profile its spans' least chord rate times `2^−L`,
//! [`super::forms::cyl`]), and `δ` a bound on a tile's control points' distance
//! from the form's exact sub-patch over the cell, the same at every
//! level:
//! - the coordinates rounded twice (the base tile's, then the level's,
//!   which are convex combinations of the base tile's): `3·ulp(M)` with
//!   the plane's origin's rounding, `M` a bound on the base tile's
//!   coordinates;
//! - the frame's axes and the weight `√½` rounded: `2⁻⁴⁸·ρ` (an axis
//!   off by `2⁻⁵⁰` at most, a weight's relative error moving a point by
//!   `2⁻⁵¹` of its tile's spread), and `2⁻⁵⁰·T` along the lines, `T` the
//!   largest distance of a control point from the origin along an axis.
//!
//! `δ` grows with a cell's distance from the origin, so the cap is the
//! form's over the cells that hold the coordinate box (the plane's cell
//! `(0, 0)`, a cylinder's axial cell 0 and a cone's slant cell 0 with
//! every angle cell or span, every cell of a sphere, a torus's outer
//! tube cell 0 with every angle cell), and a cell further
//! out has its own, lower one. Constructors, `extend`
//! ([`super::surface::Surface::extend`]), torus domains and imports
//! coarsen their rectangle to the cap ([`Lattice::capped`]: the domain
//! need only cover the trims), so they
//! never fail on it; an explicit request beyond it ([`Lattice::tile`],
//! [`Lattice::tiles`]) is refused ([`LatticeError::LevelBeyondCap`]). A
//! cylinder of radius 1 mm is capped at level 23 around its axis (tiles
//! of about `2·10⁻⁷` mm) and 44 along it, its axial cell's far rows
//! setting the precision wherever it lies in the box.
//!
//! **Surfaces** are integer rectangles of one level ([`Rect`]); a 180°
//! surface (the canonical constructors' choice, [`Lattice::half`]) is two
//! angle cells by one axial cell on a cylinder or cone, two by two on a
//! sphere (pole to pole), the one tile of a plane; a conic-chain
//! cylinder's is its whole profile by one axial cell; a ring torus's two
//! angle cells by its tube's outer half (two cells), a spindle's two by
//! its whole outer part at tube level 1 (four arcs); a revolved conic's
//! two angle cells by its whole profile; an affine image's its source's.
//! An angle span (and
//! a ring torus's tube span) must be less than a full turn, and a
//! closed profile's less than all its spans, so no surface meets itself.

use std::ops::{Range, RangeInclusive};

use malachite::Rational;
use malachite::base::num::basic::traits::One;
use varde_decide::bezier::{Dir, HomPatch, Side, Sides};

use super::form::{FormKey, dot};
use super::forms::{affine as image, cone, cyl, plane, revolved, sphere, torus};
use super::round::{nearest, nearest_root, nearest_sqrt, q};
use super::tile::{Net, Tile, TileError};
use varde_decide::num::storage::ulp;

/// A tile's exact homogeneous patch.
type Exact = HomPatch<Rational>;

/// The largest coordinate magnitude of a body's bounded positions (mm).
pub use varde_decide::num::storage::MAX_COORD;

/// The edge of a plane's cell and of a cylinder's axial cell,
/// `2^⌈log₂(4·MAX_COORD)⌉ = 2²²`.
pub const CELL: f64 = 4_194_304.0;

// The least power of two at least `4·MAX_COORD`.
const _: () = assert!(CELL / 2.0 < 4.0 * MAX_COORD && 4.0 * MAX_COORD <= CELL);

/// The finest level in each direction: a level's index within its base
/// cell, times `2^−L`, is exact in `f64`.
pub const MAX_LEVEL: u32 = 52;

/// The margin of the level cap: a level's chord at least `K_CAP` times
/// its tiles' deviation from the form (module docs).
pub const K_CAP: f64 = 64.0;

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LatticeError {
    /// A level above [`MAX_LEVEL`].
    Level,
    /// A level beyond the form's cap in some direction (module docs):
    /// the form's key, the level asked for and the cap of the cells
    /// asked for.
    LevelBeyondCap {
        form: Box<FormKey>,
        level: Level,
        cap: Level,
    },
    /// A cell outside the lattice (a sphere's latitude beyond the poles,
    /// a cone's axial cell on the other nappe, a conic-chain cylinder's
    /// or revolved conic's span index outside its profile, a spindle
    /// torus's tube cell on its inner part), an index whose arithmetic
    /// overflows, or a rectangle with more tiles than memory holds.
    Index,
    /// An empty rectangle, or one whose angle span is a full turn or more
    /// (on a conic-chain cylinder: every span of a closed profile, on a
    /// revolved conic also; on a ring torus also its tube span).
    Span,
    /// A frame value, rounded, is not finite.
    Range,
    /// A tile's net is refused: a coordinate overflows, or a weight lies
    /// outside the tiles' range (a conic-chain cylinder's profile's, a
    /// spindle torus's too close to a horn torus).
    Tile(TileError),
    /// A conic-chain cylinder's or revolved conic's profile whose
    /// consecutive spans meet with different weights at their shared
    /// point: the tiles beside the joint would not share their row bit
    /// for bit.
    Joint,
    /// A revolved conic's cell whose span is tangent to the axis: the
    /// surface has a cusp there, which no tile holds
    /// ([`super::forms::revolved`]). A feature operation needing that
    /// point refuses it as its own `Feature`.
    Cusp,
    /// An extension's rectangle at another level than the surface's, or
    /// not covering its rectangle ([`Surface::extend`]).
    ///
    /// [`Surface::extend`]: super::surface::Surface::extend
    Extend,
}

impl From<TileError> for LatticeError {
    fn from(e: TileError) -> Self {
        LatticeError::Tile(e)
    }
}

/// A lattice's frame: its origin and axes, a function of the form's key
/// (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: [f64; 3],
    pub axes: [[f64; 3]; 3],
}

/// The frame of `key`'s form (module docs), computed from the stored
/// key: `Range` where a value rounds past the range.
pub fn frame(key: &FormKey) -> Result<Frame, LatticeError> {
    let (origin, axes) = match key {
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
        FormKey::ConicCylinder(k) => ([0.0; 3], revolution(&k.axis())),
        FormKey::Cone(k) => (k.apex(), revolution(&k.axis())),
        FormKey::Sphere(k) => (k.centre(), revolution(&k.axis())),
        FormKey::Torus(k) => (k.centre(), revolution(&k.axis())),
        FormKey::Revolved(k) => (k.foot(), revolution(&k.axis())),
        FormKey::Affine(k) => {
            let f = frame(k.source())?;
            let m = k.linear().map(|row| row.map(q));
            let apply = |x: &[f64; 3]| {
                let x = x.map(q);
                [0, 1, 2].map(|r| dot(&m[r], &x))
            };
            let t = k.shift().map(q);
            let o = apply(&f.origin);
            let origin = [0, 1, 2].map(|c| nearest(&(&o[c] + &t[c])));
            let axes = f.axes.map(|a| apply(&a).map(|x| nearest(&x)));
            (origin, axes)
        }
    };
    if !origin
        .iter()
        .chain(axes.iter().flatten())
        .all(|x| x.is_finite())
    {
        return Err(LatticeError::Range);
    }
    Ok(Frame { origin, axes })
}

/// A form's lattice: its key, the frame fixed by it and the level cap
/// of the cells holding the coordinate box (module docs).
#[derive(Clone, Debug)]
pub struct Lattice {
    key: FormKey,
    origin: [f64; 3],
    axes: [[f64; 3]; 3],
    cap: Level,
    /// An affine image's source's lattice, whose cells it maps.
    source: Option<Box<Lattice>>,
}

/// How a lattice direction runs: along a line (its cells `CELL` long at
/// the base level), around a circle of the radius given, or along a
/// profile whose base spans' chords are at least the rate given times
/// their parameter length.
#[derive(Clone, Copy, Debug)]
pub(in crate::geom) enum Run {
    Line,
    Turn(f64),
    Span(f64),
}

/// What a form's level cap reads of the base cells asked for (module
/// docs): the bound `m` on the tiles' coordinates, the radius `rho` and
/// the distance `t` along the lines that the rounded frame scales, and
/// how `u` and `v` run.
#[derive(Clone, Copy, Debug)]
pub(in crate::geom) struct Reach {
    pub(in crate::geom) m: f64,
    pub(in crate::geom) rho: f64,
    pub(in crate::geom) t: f64,
    pub(in crate::geom) runs: [Run; 2],
}

/// A range of base cells in one direction, ends included.
pub(in crate::geom) type Cells = RangeInclusive<i64>;

/// The frame's stored values, exactly.
pub(in crate::geom) struct ExactFrame {
    pub(in crate::geom) origin: [Rational; 3],
    pub(in crate::geom) axes: [[Rational; 3]; 3],
}

/// A weight of the lattices' tiles: `1`, `√½` or `½`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::geom) enum Weight {
    One,
    Root,
    Half,
}

impl Weight {
    pub(in crate::geom) fn times(self, o: Weight) -> Weight {
        match (self, o) {
            (Weight::One, w) | (w, Weight::One) => w,
            (Weight::Root, Weight::Root) => Weight::Half,
            _ => unreachable!("lattice weights multiply at most two roots"),
        }
    }

    /// The weight rounded once (`√½` to the nearest `f64`).
    pub(in crate::geom) fn value(self) -> f64 {
        match self {
            Weight::One => 1.0,
            Weight::Root => nearest_sqrt(&Rational::from_signeds(1, 2)),
            Weight::Half => 0.5,
        }
    }
}

/// The weights of a quarter circle's control points.
pub(in crate::geom) const ARC: [Weight; 3] = [Weight::One, Weight::Root, Weight::One];

impl Lattice {
    /// The lattice of `key`'s form, its frame computed from the stored
    /// key ([`frame`]); `Joint` for a conic-chain cylinder or revolved
    /// conic whose profile's spans meet with different weights; an
    /// affine image's errors those of its source's lattice, else its
    /// own.
    pub fn new(key: FormKey) -> Result<Lattice, LatticeError> {
        let source = match &key {
            FormKey::Plane(_)
            | FormKey::Cylinder(_)
            | FormKey::Cone(_)
            | FormKey::Sphere(_)
            | FormKey::Torus(_) => None,
            FormKey::ConicCylinder(c) => {
                cyl::joints(c)?;
                None
            }
            FormKey::Revolved(r) => {
                cyl::profile_joints(r.profile())?;
                None
            }
            FormKey::Affine(a) => Some(Box::new(Lattice::new(a.source().clone())?)),
        };
        let Frame { origin, axes } = frame(&key)?;
        let mut l = Lattice {
            key,
            origin,
            axes,
            cap: Level::BASE,
            source,
        };
        let (u, v) = l.box_cells();
        l.cap = l.cap_for(&u, &v);
        Ok(l)
    }

    /// The level cap of the cells holding the coordinate box: the
    /// deepest level admitted in each direction (module docs).
    #[inline]
    pub fn cap(&self) -> Level {
        self.cap
    }

    /// The level cap of `rect`'s base cells (module docs): the form's
    /// [`cap`](Self::cap) where they hold the coordinate box, lower
    /// further out. Errors as [`tiles`](Self::tiles) on its level, span
    /// and cells.
    pub fn cap_of(&self, rect: &Rect) -> Result<Level, LatticeError> {
        let (u, v) = self.cells(rect)?;
        Ok(self.cap_for(&u, &v))
    }

    /// `rect` coarsened to the cap of its cells: at each direction's
    /// level or the cap, whichever is coarser, the least rectangle
    /// covering it (the same rectangle where it is within the cap). What
    /// constructors, `extend`, torus domains and imports use instead of
    /// a level beyond the cap (module docs). Errors as
    /// [`tiles`](Self::tiles) on its level, span and cells (a span of
    /// less than a turn may cover a full one coarsened, which `tiles`
    /// refuses).
    pub fn capped(&self, rect: &Rect) -> Result<Rect, LatticeError> {
        let cap = self.cap_of(rect)?;
        let level = Level {
            u: rect.level.u.min(cap.u),
            v: rect.level.v.min(cap.v),
        };
        let coarse = |r: &Range<i64>, from: u32, to: u32| {
            let s = from - to;
            // `end − 1 ≥ start` (a span checked), so `+ 1` stays in range
            // once shifted (`s ≥ 1`), and is the same range at `s = 0`.
            (r.start >> s)..(((r.end - 1) >> s) + 1)
        };
        Ok(Rect {
            level,
            u: coarse(&rect.u, rect.level.u, level.u),
            v: coarse(&rect.v, rect.level.v, level.v),
        })
    }

    /// The key the lattice is anchored to.
    #[inline]
    pub fn key(&self) -> &FormKey {
        &self.key
    }

    /// The frame's origin: the plane's foot of the world origin, the
    /// cylinder's or revolved conic's foot point, the world origin of a
    /// conic-chain cylinder, the cone's apex, the sphere's or torus's
    /// centre, an affine image's source's mapped.
    #[inline]
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// The frame's axes: `[u, v, n]` on a plane, `[e₁, e₂, a]` on a
    /// form about an axis (module docs).
    #[inline]
    pub fn axes(&self) -> [[f64; 3]; 3] {
        self.axes
    }

    /// An affine image's source's lattice, whose cells it maps; `None`
    /// for every other form.
    #[inline]
    pub fn source(&self) -> Option<&Lattice> {
        self.source.as_deref()
    }

    /// The base level's tile of cell `(k, l)` (module docs).
    pub fn base_tile(&self, k: i64, l: i64) -> Result<Tile, LatticeError> {
        let f = ExactFrame {
            origin: self.origin.map(q),
            axes: self.axes.map(|a| a.map(q)),
        };
        let net: Net = match &self.key {
            FormKey::Plane(_) => plane::net(&f, k, l)?,
            FormKey::Cylinder(c) => cyl::net(&f, c, k, l)?,
            FormKey::ConicCylinder(c) => cyl::chain_net(&f, c, k, l)?,
            FormKey::Cone(c) => cone::net(&f, c, k, l)?,
            FormKey::Sphere(s) => sphere::net(&f, s, k, l)?,
            FormKey::Torus(t) => torus::net(&f, t, k, l)?,
            FormKey::Revolved(r) => revolved::net(&f, r, k, l)?,
            FormKey::Affine(a) => match &self.source {
                Some(src) => image::net(a, src.base_tile(k, l)?.net()),
                None => unreachable!("an affine image's lattice has its source's"),
            },
        };
        Ok(Tile::new(net)?.with_singular(self.singular(l)))
    }

    /// The declared singular sides of the base cells `(k, l)`, the same
    /// for every `k` (module docs): a sphere's latitude −1's first `v`
    /// row and latitude 0's last, a cone's slant cell 0's first, a
    /// spindle torus's tube cell −1's first and 0's last, a revolved
    /// conic's span's end rows where the span ends on the axis (`ρ = 0`),
    /// an affine image's its source's; none on other forms and cells.
    pub fn singular(&self, l: i64) -> Sides {
        if let Some(src) = &self.source {
            return src.singular(l);
        }
        let none = Sides::NONE;
        let ends = match &self.key {
            FormKey::Cone(_) => return if l == 0 { Side::V0.into() } else { none },
            FormKey::Sphere(_) => true,
            FormKey::Torus(t) => torus::spindle(t),
            FormKey::Revolved(r) => {
                let Some(span) = usize::try_from(l)
                    .ok()
                    .and_then(|l| r.profile().spans().get(l))
                else {
                    return none;
                };
                let mut s = none;
                if span[0][0] == 0.0 {
                    s = s.with(Side::V0);
                }
                if span[2][0] == 0.0 {
                    s = s.with(Side::V1);
                }
                return s;
            }
            _ => false,
        };
        match l {
            -1 if ends => Side::V0.into(),
            0 if ends => Side::V1.into(),
            _ => none,
        }
    }

    /// The tile `(i, j)` of `level`: its base tile's exact sub-patch,
    /// rounded once (module docs);
    /// `LevelBeyondCap` beyond the cell's cap.
    pub fn tile(&self, level: Level, i: i64, j: i64) -> Result<Tile, LatticeError> {
        if level.u > MAX_LEVEL || level.v > MAX_LEVEL {
            return Err(LatticeError::Level);
        }
        let (k, l) = (i >> level.u, j >> level.v);
        self.valid(&(k..=k), &(l..=l))?;
        self.admit(level, &(k..=k), &(l..=l))?;
        let base = self.base_tile(k, l)?;
        if level == Level::BASE {
            return Ok(base);
        }
        let t = descend(base.exact(), Dir::U, level.u, i);
        let sides = within(base.singular(), level, i, j);
        Ok(Tile::rounded(&descend(t, Dir::V, level.v, j))?.with_singular(sides))
    }

    /// The tiles of `rect`, row-major in `u` (tile `(i, j)` at
    /// `(i − u.start)·v.len() + (j − v.start)`): the same bits as
    /// [`tile`](Self::tile) gives each, a base tile's splits shared;
    /// `LevelBeyondCap` beyond the cap of its cells
    /// ([`cap_of`](Self::cap_of)).
    pub fn tiles(&self, rect: &Rect) -> Result<Vec<Tile>, LatticeError> {
        let Level { u: lu, v: lv } = rect.level;
        let (cu, cv) = self.cells(rect)?;
        self.admit(rect.level, &cu, &cv)?;
        let (nu, nv) = (span(&rect.u)?, span(&rect.v)?);
        let count = usize::try_from(nu)
            .ok()
            .and_then(|a| usize::try_from(nv).ok().and_then(|b| a.checked_mul(b)))
            .ok_or(LatticeError::Index)?;
        let mut out: Vec<Option<Tile>> = Vec::new();
        out.try_reserve_exact(count)
            .map_err(|_| LatticeError::Index)?;
        out.resize(count, None);
        for k in cu {
            for l in cv.clone() {
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
                        let sides = within(base.singular(), rect.level, i, j);
                        out[at as usize] = Some(Tile::rounded(&t)?.with_singular(sides));
                    }
                }
            }
        }
        Ok(out
            .into_iter()
            .map(|t| t.expect("every tile of the rectangle is made"))
            .collect())
    }

    /// `rect`'s base cells in `u` and `v`, after checking its level, its
    /// span, a turn's span less than full (every span of a closed
    /// profile not taken), and its cells in the lattice (an affine
    /// image's those of its source).
    fn cells(&self, rect: &Rect) -> Result<(Cells, Cells), LatticeError> {
        if let Some(src) = &self.source {
            return src.cells(rect);
        }
        let Level { u: lu, v: lv } = rect.level;
        if lu > MAX_LEVEL || lv > MAX_LEVEL {
            return Err(LatticeError::Level);
        }
        let nu = span(&rect.u)?;
        let nv = span(&rect.v)?;
        let turn = |n: u64, l: u32| u128::from(n) >= 4u128 << l;
        let full = match &self.key {
            FormKey::Plane(_) => false,
            FormKey::ConicCylinder(c) => {
                cyl::closed(c) && u128::from(nu) >= (c.profile().spans().len() as u128) << lu
            }
            FormKey::Torus(t) => turn(nu, lu) || torus::full_tube(t, nv, lv),
            FormKey::Revolved(r) => revolved::full(r, (nu, lu), (nv, lv)),
            _ => turn(nu, lu),
        };
        if full {
            return Err(LatticeError::Span);
        }
        let base = |r: &Range<i64>, l: u32| (r.start >> l)..=((r.end - 1) >> l);
        let (u, v) = (base(&rect.u, lu), base(&rect.v, lv));
        self.valid(&u, &v)?;
        Ok((u, v))
    }

    /// `Index` unless the base cells `u` × `v` lie in the lattice: a
    /// cone's axial cells from the apex on, a conic-chain cylinder's or
    /// revolved conic's spans those of its profile, a sphere's latitudes
    /// −1 and 0, a spindle torus's tube cells −1 and 0, an affine
    /// image's its source's.
    fn valid(&self, u: &Cells, v: &Cells) -> Result<(), LatticeError> {
        if let Some(src) = &self.source {
            return src.valid(u, v);
        }
        let ok = match &self.key {
            FormKey::Cone(_) => *v.start() >= 0,
            FormKey::Sphere(_) => sphere::valid(v),
            FormKey::Torus(t) => torus::valid(t, v),
            FormKey::ConicCylinder(c) => {
                *u.start() >= 0 && (*u.end() as u64) < c.profile().spans().len() as u64
            }
            FormKey::Revolved(r) => revolved::valid(r, v),
            _ => true,
        };
        if ok { Ok(()) } else { Err(LatticeError::Index) }
    }

    /// `LevelBeyondCap` if `level` is beyond the cap of the base cells
    /// `u` × `v`.
    fn admit(&self, level: Level, u: &Cells, v: &Cells) -> Result<(), LatticeError> {
        let cap = self.cap_for(u, v);
        if level.u > cap.u || level.v > cap.v {
            return Err(LatticeError::LevelBeyondCap {
                form: Box::new(self.key.clone()),
                level,
                cap,
            });
        }
        Ok(())
    }

    /// The base cells holding the coordinate box (module docs), in `u`
    /// and `v`: cell `(0, 0)` of a plane, axial cell 0 of a cylinder or
    /// cone, a torus's outer tube cell 0, any angle cell, every span of
    /// a profile, an affine image's source's.
    fn box_cells(&self) -> (Cells, Cells) {
        if let Some(src) = &self.source {
            return src.box_cells();
        }
        match &self.key {
            FormKey::ConicCylinder(c) => (0..=(c.profile().spans().len() as i64 - 1), 0..=0),
            FormKey::Revolved(r) => revolved::box_cells(r),
            _ => (0..=0, 0..=0),
        }
    }

    /// What the level cap reads of the base cells `u` × `v`: each form's
    /// constructor's reading, an affine image's from its source's.
    fn reach(&self, u: &Cells, v: &Cells) -> Reach {
        let frame = Frame {
            origin: self.origin,
            axes: self.axes,
        };
        match (&self.key, &self.source) {
            (FormKey::Plane(_), _) => plane::reach(&frame, u, v),
            (FormKey::Cylinder(c), _) => cyl::reach(&frame, c, v),
            (FormKey::ConicCylinder(c), _) => cyl::chain_reach(&frame, c, u, v),
            (FormKey::Cone(c), _) => cone::reach(&frame, c, v),
            (FormKey::Sphere(s), _) => sphere::reach(&frame, s),
            (FormKey::Torus(t), _) => torus::reach(&frame, t, v),
            (FormKey::Revolved(r), _) => revolved::reach(&frame, r, v),
            (FormKey::Affine(a), Some(src)) => image::reach(a, src.reach(u, v)),
            (FormKey::Affine(_), None) => {
                unreachable!("an affine image's lattice has its source's")
            }
        }
    }

    /// The level cap of the base cells `u` × `v` (module docs), lattice
    /// cells there.
    fn cap_for(&self, u: &Cells, v: &Cells) -> Level {
        let Reach { m, rho, t, runs } = self.reach(u, v);
        // Sums and products of positive values, each rounded: a relative
        // `2⁻⁴⁰` covers their rounding.
        let up = 1.0 + pow2(-40);
        let m = m * up;
        let delta = match ulp(m) {
            Some(ulp) => (3.0 * ulp + pow2(-48) * rho + pow2(-50) * t) * up,
            None => f64::INFINITY,
        };
        if !delta.is_finite() {
            // Coordinates past the range: no tile is made at any level.
            return Level::BASE;
        }
        let [u, v] = runs.map(|run| deepest(run, K_CAP * delta));
        Level { u, v }
    }

    /// The canonical surface's rectangle at the base level: the 180°
    /// surface of angle cells `first` and `first + 1` times axial cell
    /// 0 on a cylinder or cone (the cone's apex row its first `v` row),
    /// times latitudes −1 and 0 on a sphere, times a ring torus's tube
    /// cells −1 and 0 (its outer half); a spindle torus's at level `(0,
    /// 1)`, its outer part's four tube arcs from apex to apex; the
    /// plane's one cell `(0, 0)`; every span of a conic-chain cylinder's
    /// profile times axial cell 0 (`first` unused on these two); angle
    /// cells `first` and `first + 1` times every span of a revolved
    /// conic's profile (a closed profile's rectangle is refused by
    /// [`tiles`](Self::tiles)); an affine image's source's.
    pub fn half(&self, first: i64) -> Result<Rect, LatticeError> {
        if let Some(src) = &self.source {
            return src.half(first);
        }
        let next = first.checked_add(2).ok_or(LatticeError::Index)?;
        Ok(match &self.key {
            FormKey::Plane(_) => Rect {
                level: Level::BASE,
                u: 0..1,
                v: 0..1,
            },
            FormKey::Cylinder(_) | FormKey::Cone(_) => Rect {
                level: Level::BASE,
                u: first..next,
                v: 0..1,
            },
            FormKey::ConicCylinder(c) => Rect {
                level: Level::BASE,
                u: 0..c.profile().spans().len() as i64,
                v: 0..1,
            },
            FormKey::Sphere(_) => Rect {
                level: Level::BASE,
                u: first..next,
                v: -1..1,
            },
            FormKey::Torus(t) => torus::half(t, first, next),
            FormKey::Revolved(r) => revolved::half(r, first, next),
            FormKey::Affine(_) => unreachable!("an affine image's lattice has its source's"),
        })
    }
}

/// The singular sides `base` of a base cell that tile `(i, j)` of
/// `level` in it reaches: a side at 0 where the index's low bits are all
/// 0, at 1 where they are all 1.
fn within(base: Sides, level: Level, i: i64, j: i64) -> Sides {
    let low = |x: i64, l: u32| x & ((1i64 << l) - 1);
    let full = |l: u32| (1i64 << l) - 1;
    base.iter()
        .filter(|&s| {
            let (x, l) = match s.dir() {
                Dir::U => (i, level.u),
                Dir::V => (j, level.v),
            };
            if s.at_one() {
                low(x, l) == full(l)
            } else {
                low(x, l) == 0
            }
        })
        .collect()
}

/// The deepest level up to [`MAX_LEVEL`] whose cells' chord along `run`
/// is at least `min`, by a lower bound on the chord; the base level if
/// none is.
fn deepest(run: Run, min: f64) -> u32 {
    let down = 1.0 - pow2(-40);
    let chord = |level: u32| {
        // `2^−L`, exact.
        let s = pow2(-(level as i32));
        match run {
            Run::Line => CELL * s * down,
            Run::Span(rate) => rate * s * down,
            Run::Turn(r) => {
                // Half the cell's turn, `x ≤ π/4·2^−L`, and `sin x ≥ x −
                // x³/6`.
                let x = 0.785 * s;
                r * (2.0 * x * (1.0 - x * x / 6.0) * down)
            }
        }
    };
    (1..=MAX_LEVEL)
        .take_while(|&level| chord(level) >= min)
        .last()
        .unwrap_or(0)
}

/// `2^k`, for `k` in the normal range.
pub(in crate::geom) const fn pow2(k: i32) -> f64 {
    f64::from_bits(((1023 + k) as u64) << 52)
}

/// The larger of two values not NaN.
pub(in crate::geom) fn larger(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// The node indices, in half cells, of cell `k`'s three control rows:
/// `2k − 1`, `2k`, `2k + 1`.
pub(in crate::geom) fn halves(k: i64) -> Result<[i64; 3], LatticeError> {
    let m = k.checked_mul(2).ok_or(LatticeError::Index)?;
    let lo = m.checked_sub(1).ok_or(LatticeError::Index)?;
    let hi = m.checked_add(1).ok_or(LatticeError::Index)?;
    Ok([lo, m, hi])
}

/// The control directions and weights of angle cell `k`'s quarter
/// circle in the unit frame `e₁, e₂`: the radii at `k·90°` and
/// `(k + 1)·90°` and their sum (the tangents' corner), exact. Period 4.
pub(in crate::geom) fn arc(
    e1: &[Rational; 3],
    e2: &[Rational; 3],
    k: i64,
) -> [([Rational; 3], Weight); 3] {
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

/// The affine control point `(p, w)`, each coordinate rounded once.
pub(in crate::geom) fn affine(p: &[Rational; 3], w: f64) -> [f64; 4] {
    let [x, y, z] = p.each_ref().map(nearest);
    [x, y, z, w]
}

/// A bound on the distance from the origin along a line of the base
/// cells `cells`: `(n + ½)·CELL`, `n` the largest `|index|`.
pub(in crate::geom) fn line_reach(cells: &Cells) -> f64 {
    (far(cells) as f64 + 0.5) * CELL
}

/// The largest `|index|` of `cells`.
pub(in crate::geom) fn far(cells: &Cells) -> u64 {
    cells.start().unsigned_abs().max(cells.end().unsigned_abs())
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
