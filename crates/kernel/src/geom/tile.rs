//! A tile: one rational biquadratic patch of a tiled surface, stored as
//! its homogeneous net in `f64`, with certified evaluation, partial
//! derivatives, splits at ½, the hull box, the normal cone and the
//! regularity flag.
//!
//! **Storage and validation.** A tile is the net `Pᵢⱼ = (w·x, w·y, w·z,
//! w)`, `i, j ∈ 0..=2`, row-major in `u` (`Pᵢⱼ` at `3·i + j`, `i` along
//! `u`), exactly as stored: neighbouring tiles share a boundary row of
//! these values bit for bit, weights included, and every evaluation on a
//! boundary line reads that row alone (`varde_decide::bezier`'s end rule),
//! so neighbours agree there to the bit. [`Tile::new`] takes the net only
//! when every value is finite and every weight lies in
//! `[MIN_WEIGHT, MAX_WEIGHT]` = `[1/64, 64]`. Coordinates are not range
//! tested: a form's lattice tiles may reach past the largest coordinate
//! of a body (a plane's single tile covers the whole coordinate box), so
//! their magnitude costs precision, never validity.
//!
//! **Levels.** Every operation is certified: its result carries an error
//! bound against the same operation on the stored net. The first level
//! (`Bounded<f64>`) is the default and is cached; a caller who needs more
//! precision lifts the net exactly to any level with
//! [`Tile::patch_at`] (double-double, big-float at a width, or exact
//! rationals: the level and width are those of the value passed) and uses
//! the patch's operations there, or asks for the cone at that level with
//! [`Tile::normal_cone_at`].
//!
//! **Splits.** [`Tile::split_half`] stores the first level's halves at ½
//! as new tiles: the halves' values are rounded once, at storage, so a
//! half is a tile of its own, within the split's error bound of the
//! parent's half, not exactly that half. A finer lattice level is
//! defined this way (the base tile after its splits), and as the same
//! net always splits into the same bits, every surface using that level
//! gets the same tiles. Splits in `u` and in `v` don't commute to the
//! bit, so whoever derives a level fixes their order (the lattices,
//! [`super::lattice`]: every `u` split first, then the `v` splits).
//!
//! **Cached per tile** (computed once, on first use, from the net alone,
//! so every copy of a tile has the same bits): the first-level patch, the
//! hull box, the homogeneous partial derivatives, the normal numerator
//! and the normal cone.
//!
//! **Normal cone and regularity.** The normal numerator
//! `N = W·(P_u × P_v) + W_u·(P_v × P) + W_v·(P × P_u)` equals
//! `W³·(x_u × x_v)`, a positive multiple of the normal of the tile's
//! orientation (`x_u × x_v`) wherever it isn't zero. As a Bernstein form
//! (degrees `(5, 5)`, unreduced) every value is a non-negative combination
//! of its coefficients. If every coefficient lies in the circular cone of
//! half-angle `θ < 90°` about an axis `a` (a convex cone), so does every
//! value, and `a·N > 0` keeps it non-zero: the cone holds every unit
//! normal of the tile and the tile is **regular** (no fold, no
//! degenerate point). The cone is [`NormalCone::of_form`] of the
//! numerator: about the normalised sum of the coefficients' directions,
//! certified on their enclosures, none when one of them isn't certainly
//! in the open half-space `a·c > 0`. A pole or apex row (a singular side) makes a whole boundary
//! row of the numerator zero, so such a tile is never regular here; nor
//! is one whose normals spread over more than a half-space (it must be
//! split) or one the first level can't settle (escalate with
//! [`Tile::normal_cone_at`] or split). The numerator is translation
//! invariant, so it is computed in a frame centred on the tile's hull box,
//! each translated value `x − c·w` rounded once from its double-double
//! value: its rounding scales with the tile's size, not its distance from
//! the origin (rounding `c·w` alone would cost `u·|c·w|`, which swamps a
//! small or nearly flat tile far out).

use std::fmt;
use std::sync::OnceLock;

pub use varde_decide::bezier::NormalCone;
use varde_decide::bezier::{Dir, HomPatch, HullBox, Partials, Scalar, Tensor};
use varde_decide::num::bounded::{Bounded, add_up};
use varde_decide::num::dd::Dd;

/// The degree of a tile in each direction.
pub const DEGREE: usize = 2;

/// The least weight a tile may hold, `1/64`.
pub const MIN_WEIGHT: f64 = 1.0 / 64.0;

/// The greatest weight a tile may hold, `64`.
pub const MAX_WEIGHT: f64 = 64.0;

/// A tile's homogeneous net, `(w·x, w·y, w·z, w)` row-major in `u`:
/// `Pᵢⱼ` at `3·i + j`, `i` along `u`.
pub type Net = [[f64; 4]; 9];

/// A tile's patch at the first precision level.
pub type Patch = HomPatch<Bounded<f64>>;

/// Why [`Tile::new`] refuses a net. `index` is the first offending
/// control point's place in the [`Net`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileError {
    /// A coordinate or weight is not finite.
    NotFinite { index: usize },
    /// A weight lies outside `[MIN_WEIGHT, MAX_WEIGHT]`.
    Weight { index: usize },
}

/// A rational biquadratic patch on `[0, 1]²` stored as its homogeneous
/// `f64` net, validated, with certified operations (module docs).
#[derive(Clone)]
pub struct Tile {
    net: Net,
    patch: Patch,
    hull: HullBox,
    /// The homogeneous partials in `u` and `v`.
    partials: OnceLock<[Patch; 2]>,
    numerator: OnceLock<[Tensor<Bounded<f64>>; 3]>,
    cone: OnceLock<Option<NormalCone>>,
}

impl Tile {
    /// The tile with the homogeneous net `net` ([`Net`]'s order): every
    /// value finite and every weight in `[MIN_WEIGHT, MAX_WEIGHT]`, else
    /// the first offence (non-finite values checked first).
    pub fn new(net: Net) -> Result<Tile, TileError> {
        if let Some(index) = net.iter().position(|p| !p.iter().all(|x| x.is_finite())) {
            return Err(TileError::NotFinite { index });
        }
        if let Some(index) = net
            .iter()
            .position(|p| !(MIN_WEIGHT..=MAX_WEIGHT).contains(&p[3]))
        {
            return Err(TileError::Weight { index });
        }
        let patch = HomPatch::from_f64(DEGREE, DEGREE, &net).expect("a biquadratic net");
        let hull = patch.hull_box().expect("every weight is positive");
        Ok(Tile {
            net,
            patch,
            hull,
            partials: OnceLock::new(),
            numerator: OnceLock::new(),
            cone: OnceLock::new(),
        })
    }

    /// The stored net.
    #[inline]
    pub fn net(&self) -> &Net {
        &self.net
    }

    /// The patch at the first level, the stored values exact.
    #[inline]
    pub fn patch(&self) -> &Patch {
        &self.patch
    }

    /// The patch at `like`'s level (and width), the stored values exact:
    /// for operations at a higher level than the first.
    pub fn patch_at<S: Scalar>(&self, like: &S) -> HomPatch<S> {
        HomPatch::new(
            DEGREE,
            DEGREE,
            self.net.iter().map(|p| p.map(|x| S::from_f64(x, like))),
        )
        .expect("a biquadratic net")
    }

    /// A certified box holding the whole tile.
    #[inline]
    pub fn hull_box(&self) -> HullBox {
        self.hull
    }

    /// The point at `(u, v)`, certified ([`HomPatch::eval`]).
    pub fn eval(&self, u: f64, v: f64) -> [Bounded<f64>; 3] {
        self.patch.eval(&Bounded::exact(u), &Bounded::exact(v))
    }

    /// The homogeneous partial derivative in `dir`, of degree 1 in it.
    pub fn derivative(&self, dir: Dir) -> &Patch {
        let [du, dv] = self
            .partials
            .get_or_init(|| [self.patch.derivative(Dir::U), self.patch.derivative(Dir::V)]);
        match dir {
            Dir::U => du,
            Dir::V => dv,
        }
    }

    /// The point and its partials at `(u, v)`, certified: the same values
    /// as [`HomPatch::eval_partials`] (the quotient rule
    /// `x_u = (X_u − x·W_u) / W`), from the cached derivatives.
    pub fn eval_partials(&self, u: f64, v: f64) -> Partials<Bounded<f64>> {
        let (u, v) = (Bounded::exact(u), Bounded::exact(v));
        let [x, y, z, w] = self.patch.eval_hom(&u, &v);
        let p = [x / w, y / w, z / w];
        let partial = |dir| {
            let d = self.derivative(dir).eval_hom(&u, &v);
            [0, 1, 2].map(|k| (d[k] - p[k] * d[3]) / w)
        };
        let du = partial(Dir::U);
        let dv = partial(Dir::V);
        Partials { p, du, dv }
    }

    /// The tiles on the halves `[0, ½]` and `[½, 1]` in `dir`: the first
    /// level's split at ½ ([`HomPatch::split_half`]) stored to its values.
    /// Its rounding makes each half a tile of its own, within the split's
    /// error bound of the parent's half, and the same bits wherever the
    /// same net is split: halves share their middle row, and neighbours
    /// sharing a boundary row and split in the same direction share the
    /// halves of that row. Weights stay in range; an error only where a
    /// sum overflows.
    pub fn split_half(&self, dir: Dir) -> Result<[Tile; 2], TileError> {
        let [a, b] = self.patch.split_half(dir);
        Ok([Tile::new(values(&a))?, Tile::new(values(&b))?])
    }

    /// The origin of the frame the normal numerator is computed in: the
    /// hull box's centre, each coordinate 0 where it isn't finite.
    pub fn frame(&self) -> [f64; 3] {
        [0, 1, 2].map(|k| {
            let c = self.hull.lo[k] * 0.5 + self.hull.hi[k] * 0.5;
            if c.is_finite() { c } else { 0.0 }
        })
    }

    /// The normal numerator at the first level, of degrees `(5, 5)`
    /// ([`HomPatch::normal_numerator`]), computed on the net translated to
    /// [`frame`](Self::frame) (the numerator is translation invariant, so
    /// its value is the same; its rounding relative to the tile's size,
    /// module docs).
    pub fn normal_numerator(&self) -> &[Tensor<Bounded<f64>>; 3] {
        self.numerator
            .get_or_init(|| numerator(&self.framed_first()))
    }

    /// The certified normal cone at the first level, `None` where it
    /// can't be certified there (module docs).
    pub fn normal_cone(&self) -> Option<NormalCone> {
        *self
            .cone
            .get_or_init(|| NormalCone::of_form(self.normal_numerator()))
    }

    /// The certified normal cone with the numerator computed at `like`'s
    /// level (and width): for a tile the first level leaves uncertified.
    pub fn normal_cone_at<S: Scalar>(&self, like: &S) -> Option<NormalCone> {
        NormalCone::of_form(&numerator(&self.framed(like)))
    }

    /// Whether the tile is certified regular at the first level: its
    /// normal numerator certified non-zero, by a normal cone (module
    /// docs). A tile with a singular side never is.
    pub fn is_regular(&self) -> bool {
        self.normal_cone().is_some()
    }

    /// The first level's patch translated by `−frame`: each `x − c·w`
    /// computed in double-double and rounded once, so its error is
    /// relative to the translated value, not to `c·w`.
    fn framed_first(&self) -> Patch {
        let c = self.frame();
        let net = self.net.iter().map(|p| {
            let w = p[3];
            let at = |k: usize| {
                if c[k] == 0.0 {
                    Bounded::exact(p[k])
                } else {
                    shifted(p[k], c[k], w)
                }
            };
            [at(0), at(1), at(2), Bounded::exact(w)]
        });
        HomPatch::new(DEGREE, DEGREE, net).expect("a biquadratic net")
    }

    /// The patch at `like`'s level translated by `−frame`: the points
    /// `P − c·W`, the weights unchanged.
    fn framed<S: Scalar>(&self, like: &S) -> HomPatch<S> {
        let c = self.frame();
        let net = self.net.iter().map(|p| {
            let w = S::from_f64(p[3], like);
            let at = |k: usize| {
                let x = S::from_f64(p[k], like);
                if c[k] == 0.0 {
                    x
                } else {
                    x.sub(&S::from_f64(c[k], like).mul(&w))
                }
            };
            [at(0), at(1), at(2), w]
        });
        HomPatch::new(DEGREE, DEGREE, net).expect("a biquadratic net")
    }
}

impl fmt::Debug for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tile")
            .field("net", &self.net)
            .finish_non_exhaustive()
    }
}

/// Tiles are equal when their nets are, bit for bit.
impl PartialEq for Tile {
    fn eq(&self, o: &Self) -> bool {
        self.net
            .iter()
            .flatten()
            .zip(o.net.iter().flatten())
            .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

impl Eq for Tile {}

/// `x − c·w` at the first level: its double-double value rounded to the
/// nearest `f64`, that rounding (the low part) and the double-double's
/// own error bound its error.
fn shifted(x: f64, c: f64, w: f64) -> Bounded<f64> {
    let dd = |v: f64| Bounded::exact(Dd::from_f64(v));
    let d = dd(x) - dd(c) * dd(w);
    let v = d.v.hi();
    if !d.is_certified() {
        return Bounded::new(v, f64::INFINITY, f64::INFINITY);
    }
    Bounded::new(v, 0.0, add_up(d.v.lo().abs(), d.err()))
}

/// The values of a first-level biquadratic patch's net.
fn values(p: &Patch) -> Net {
    let h = p.tensors();
    std::array::from_fn(|i| [0, 1, 2, 3].map(|k| h[k].coefs()[i].v))
}

fn numerator<S: Scalar>(p: &HomPatch<S>) -> [Tensor<S>; 3] {
    p.normal_numerator()
        .expect("a biquadratic patch's numerator is within the degree limit")
}

#[cfg(test)]
mod tests;
