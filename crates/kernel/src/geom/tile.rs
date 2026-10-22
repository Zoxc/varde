//! A tile: one rational biquadratic patch of a tiled surface, stored as
//! its affine net in `f64`, with certified evaluation, partial
//! derivatives, splits at ½, the hull box, the normal cone and the
//! regularity flag.
//!
//! **Storage and validation.** A tile is the net of its control points
//! with their weights, `(x, y, z, w)`, `i, j ∈ 0..=2`, row-major in `u`
//! (`(i, j)` at `3·i + j`, `i` along `u`): a point and its weight, never
//! `w·x`, so a corner of the tile is its corner control point exactly,
//! and a boundary row is a span through its end points for any weights.
//! Neighbouring tiles share a boundary row of these values bit for bit,
//! weights included, and every evaluation on a boundary line reads that
//! row alone (`varde_decide::bezier`'s end rule), so neighbours agree
//! there to the bit. [`Tile::new`] takes the net only when every value
//! is finite and every weight lies in `[MIN_WEIGHT, MAX_WEIGHT]` =
//! `[1/64, 64]`. Coordinates are not range tested: a form's lattice
//! tiles may reach past the largest coordinate of a body (a plane's
//! single tile covers the whole coordinate box), so their magnitude
//! costs precision, never validity.
//!
//! **Levels.** Every operation is certified: its result carries an error
//! bound against the same operation on the stored net. The homogeneous
//! coefficients `w·x` are formed at the level they are used at
//! ([`HomPatch::from_affine`]: rounded once each, the rounding carried,
//! at the first level; exact from double-double up). The first level
//! (`Bounded<f64>`) is the default and is cached; a caller who needs
//! more precision forms the patch at any level with [`Tile::patch_at`]
//! (double-double, big-float at a width, or exact rationals: the level
//! and width are those of the value passed) and uses the patch's
//! operations there, or asks for the cone at that level with
//! [`Tile::normal_cone_at`].
//!
//! **Splits.** [`Tile::split_half`] runs de Casteljau at ½ on the exact
//! homogeneous net and stores each new control point and weight as its
//! exact value rounded once to the nearest `f64`, ties to even; the kept
//! corners and boundary points round to themselves, so they are copied.
//! A half is a tile of its own, within half an ulp per value of the
//! parent's exact half. As a split is a function of the stored net alone
//! and each of its values depends only on the line it lies on, halves
//! share their middle row, and neighbours sharing a boundary row and
//! split in the same direction share the halves of that row.
//!
//! **Cached per tile** (computed once, on first use, from the net alone,
//! so every copy of a tile has the same bits): the first-level patch, the
//! hull box, the homogeneous partial derivatives, the normal numerator,
//! the normal cone with the level that certified it, and whether a corner
//! of the exact numerator is zero.
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
//! degenerate point). The cone's margin is its certified `cos`.
//!
//! **The cone is a certificate**, found in the tile's frame, then in
//! space, then in precision. At a level the cone is first tried on the
//! numerator's coefficients ([`NormalCone::of_forms`] about the axis of
//! their smallest cone); where they spread too far
//! ([`ConeFail::Space`]) the numerator is split at ½ in both directions,
//! up to [`MAX_CONE_DEPTH`] times, and the cone tried on all the pieces'
//! coefficients together (about their mean direction): every value of
//! the numerator is a combination of one piece's coefficients, and the
//! pieces' coefficients lie closer to the values. Where rounding decides
//! ([`ConeFail::Precision`]) the next level of the schedule follows
//! ([`ladder`]), from the whole numerator again. A tile whose exact
//! numerator is zero at a corner (a pole or apex row, a singular side,
//! makes a whole boundary row zero; a corner whose two edges leave it
//! parallel makes the corner zero) has no cone at any level or depth and
//! is never regular here (no escalation is tried); nor is one whose
//! normals spread over more than a half-space at the deepest split (the
//! tile must be split) or that the top level leaves undecided. The cone
//! and its level are cached; [`Tile::normal_cone_at`] runs the same
//! search at one level, so it is [`Tile::normal_cone`] at the level
//! [`Tile::normal_cone_level`] names.
//!
//! **One frame.** The numerator is translation invariant, so at every
//! level it is computed in one frame centred on the tile's hull box
//! ([`Tile::frame`]: the centre on a power-of-two grid): each
//! homogeneous coefficient `w·(x − c)`, the affine point translated
//! first, so its rounding scales with the tile's size, not its distance
//! from the origin (rounding `w·x` and `c·w` alone would cost `u·|c·w|`,
//! which swamps a small or nearly flat tile far out). The translation is
//! formed exactly and rounded once, `x − c = t + r` with `t` the nearest
//! `f64` and the residual `r` exact (two-sum, as
//! [`storage::translate`](varde_decide::num::storage::translate)), and
//! the coefficient is `w·t + w·r` at the level: at the first the products
//! rounded once each and the sum once more, the translation's rounding
//! carried, from double-double up exact but for the sum's rounding.
//! Every caller of the cone, at every level, uses this frame.

use std::fmt;
use std::sync::OnceLock;

use malachite::Rational;
pub use varde_decide::bezier::NormalCone;
use varde_decide::bezier::{
    Axis, ConeFail, Dir, HomPatch, HullBox, Partials, Scalar, Side, Tensor,
};
use varde_decide::num::bounded::Bounded;
use varde_decide::num::expansion::two_sum;
use varde_decide::num::level::{AtLevel, LevelScalar, ladder};
use varde_decide::num::schedule::{LEVELS, Level};
use varde_decide::num::storage::{frame_of, translate};

use super::round::nearest;

/// The degree of a tile in each direction.
pub const DEGREE: usize = 2;

/// The least weight a tile may hold, `1/64`.
pub const MIN_WEIGHT: f64 = 1.0 / 64.0;

/// The greatest weight a tile may hold, `64`.
pub const MAX_WEIGHT: f64 = 64.0;

/// A tile's affine net, `(x, y, z, w)` row-major in `u`: control point
/// `(i, j)` at `3·i + j`, `i` along `u`.
pub type Net = [[f64; 4]; 9];

/// The most times the normal numerator is split at ½ in each direction
/// in the search for its cone at a level (module docs): up to `4³` = 64
/// pieces.
pub const MAX_CONE_DEPTH: u32 = 3;

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

/// A rational biquadratic patch on `[0, 1]²` stored as its affine `f64`
/// net, validated, with certified operations (module docs).
#[derive(Clone)]
pub struct Tile {
    net: Net,
    patch: Patch,
    hull: HullBox,
    /// The homogeneous partials in `u` and `v`.
    partials: OnceLock<[Patch; 2]>,
    numerator: OnceLock<[Tensor<Bounded<f64>>; 3]>,
    cone: OnceLock<Option<(NormalCone, Level)>>,
    /// Whether the exact numerator is zero at a corner.
    degenerate: OnceLock<bool>,
}

impl Tile {
    /// The tile with the affine net `net` ([`Net`]'s order): every
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
        let patch = HomPatch::from_affine(DEGREE, DEGREE, net, &Bounded::exact(0.0))
            .expect("a biquadratic net");
        Ok(Tile {
            net,
            patch,
            hull: hull_of(&net),
            partials: OnceLock::new(),
            numerator: OnceLock::new(),
            cone: OnceLock::new(),
            degenerate: OnceLock::new(),
        })
    }

    /// The tile whose net is the exact homogeneous `patch`'s rounded:
    /// each control point `(X/W, Y/W, Z/W)` and weight `W` rounded once
    /// to the nearest `f64`, ties to even. Errors as [`new`](Self::new),
    /// where a value rounds past the range or a weight lies out of it
    /// (one not positive refused before dividing by it); panics unless
    /// the degrees are `(2, 2)`.
    pub fn rounded(patch: &HomPatch<Rational>) -> Result<Tile, TileError> {
        assert_eq!(patch.degrees(), (DEGREE, DEGREE), "a biquadratic patch");
        let [x, y, z, w] = patch.tensors().each_ref().map(Tensor::coefs);
        if let Some(index) = w.iter().position(|w| *w <= 0u32) {
            return Err(TileError::Weight { index });
        }
        Tile::new(std::array::from_fn(|i| {
            let at = |c: &[Rational]| nearest(&(&c[i] / &w[i]));
            [at(x), at(y), at(z), nearest(&w[i])]
        }))
    }

    /// The stored net.
    #[inline]
    pub fn net(&self) -> &Net {
        &self.net
    }

    /// The control point `(i, j)` (`i` along `u`) without its weight;
    /// panics past 2. A corner is the tile's point there exactly.
    #[inline]
    pub fn point(&self, i: usize, j: usize) -> [f64; 3] {
        assert!(i <= DEGREE && j <= DEGREE, "index past the degrees");
        let [x, y, z, _] = self.net[3 * i + j];
        [x, y, z]
    }

    /// The patch at the first level, its coefficients `w·x` rounded once
    /// with the rounding carried ([`HomPatch::from_affine`]).
    #[inline]
    pub fn patch(&self) -> &Patch {
        &self.patch
    }

    /// The patch at `like`'s level (and width), its coefficients formed
    /// there ([`HomPatch::from_affine`]: exact from double-double up):
    /// for operations at a higher level than the first.
    pub fn patch_at<S: Scalar>(&self, like: &S) -> HomPatch<S> {
        HomPatch::from_affine(DEGREE, DEGREE, self.net, like).expect("a biquadratic net")
    }

    /// A certified box holding the whole tile: the box of its control
    /// points, exactly (positive weights keep the tile in their convex
    /// hull).
    #[inline]
    pub fn hull_box(&self) -> HullBox {
        self.hull
    }

    /// The point at `(u, v)`, certified ([`HomPatch::eval`]); at a corner
    /// its control point exactly.
    pub fn eval(&self, u: f64, v: f64) -> [Bounded<f64>; 3] {
        match corner(u, v) {
            Some((i, j)) => self.point(i, j).map(Bounded::exact),
            None => self.patch.eval(&Bounded::exact(u), &Bounded::exact(v)),
        }
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

    /// The point and its partials at `(u, v)`, certified: the values of
    /// [`HomPatch::eval_partials`] (the quotient rule
    /// `x_u = (X_u − x·W_u) / W`), from the cached derivatives, except
    /// that the point at a corner is its control point exactly.
    pub fn eval_partials(&self, u: f64, v: f64) -> Partials<Bounded<f64>> {
        let at = corner(u, v);
        let (u, v) = (Bounded::exact(u), Bounded::exact(v));
        let [x, y, z, w] = self.patch.eval_hom(&u, &v);
        let p = [x / w, y / w, z / w];
        let partial = |dir| {
            let d = self.derivative(dir).eval_hom(&u, &v);
            [0, 1, 2].map(|k| (d[k] - p[k] * d[3]) / w)
        };
        let du = partial(Dir::U);
        let dv = partial(Dir::V);
        let p = match at {
            Some((i, j)) => self.point(i, j).map(Bounded::exact),
            None => p,
        };
        Partials { p, du, dv }
    }

    /// The tiles on the halves `[0, ½]` and `[½, 1]` in `dir`: the exact
    /// split at ½ of the homogeneous net, each new control point and
    /// weight rounded once to the nearest `f64` ([`rounded`](Self::rounded);
    /// the kept corners and boundary points round to themselves). The same
    /// bits wherever the same net is split: halves share their middle
    /// row, and neighbours sharing a boundary row and split in the same
    /// direction share the halves of that row. Weights stay in range; an
    /// error only where a value rounds past the range.
    pub fn split_half(&self, dir: Dir) -> Result<[Tile; 2], TileError> {
        let [a, b] = self.exact().split_half(dir);
        Ok([Tile::rounded(&a)?, Tile::rounded(&b)?])
    }

    /// The exact homogeneous patch of the stored net.
    pub fn exact(&self) -> HomPatch<Rational> {
        self.patch_at(&Rational::from(0u32))
    }

    /// The origin of the frame the normal numerator is computed in, at
    /// every level and for every caller: the hull box's centre on a
    /// power-of-two grid inside the box ([`frame_of`]), each coordinate 0
    /// where it isn't finite or where a control point's translation to it
    /// isn't certified ([`translate`]: overflows).
    pub fn frame(&self) -> [f64; 3] {
        let mut c = frame_of(&self.hull.lo, &self.hull.hi);
        for k in 0..3 {
            if !self
                .net
                .iter()
                .all(|p| translate(p[k], c[k]).is_certified())
            {
                c[k] = 0.0;
            }
        }
        c
    }

    /// The normal numerator at the first level, of degrees `(5, 5)`
    /// ([`HomPatch::normal_numerator`]), computed on the net translated to
    /// [`frame`](Self::frame) (the numerator is translation invariant, so
    /// its value is the same; its rounding relative to the tile's size,
    /// module docs).
    pub fn normal_numerator(&self) -> &[Tensor<Bounded<f64>>; 3] {
        self.numerator
            .get_or_init(|| numerator(&self.framed(&Bounded::exact(0.0))))
    }

    /// The certified normal cone: the first level's, else in space, else
    /// at the levels above in turn (module docs); `None` where no level
    /// certifies one. Computed once.
    pub fn normal_cone(&self) -> Option<NormalCone> {
        self.cone_certificate().map(|(c, _)| c)
    }

    /// The level that certified [`normal_cone`](Self::normal_cone),
    /// `None` where none did.
    pub fn normal_cone_level(&self) -> Option<Level> {
        self.cone_certificate().map(|(_, l)| l)
    }

    /// The certified normal cone with the numerator computed at `like`'s
    /// level (and width), in the tile's frame, split in space as at any
    /// level but not escalated past it (module docs): at the level
    /// [`normal_cone_level`](Self::normal_cone_level) names it is
    /// [`normal_cone`](Self::normal_cone). `None` where it isn't
    /// certified at this level.
    pub fn normal_cone_at<S: Scalar>(&self, like: &S) -> Option<NormalCone> {
        if self.degenerate() {
            return None;
        }
        cone_in_space(numerator(&self.framed(like))).ok()
    }

    /// Whether the tile is certified regular: its normal numerator
    /// certified non-zero, by a normal cone at some level (module docs).
    /// A tile with a singular side never is.
    pub fn is_regular(&self) -> bool {
        self.normal_cone().is_some()
    }

    /// The cone and the level that certified it, computed once: the
    /// first level from the cached numerator, then the schedule's levels
    /// in turn while rounding decides.
    fn cone_certificate(&self) -> Option<(NormalCone, Level)> {
        *self.cone.get_or_init(|| {
            if self.degenerate() {
                return None;
            }
            let start = match cone_in_space(self.normal_numerator().clone()) {
                Ok(c) => return Some((c, Level::L0)),
                Err(ConeFail::Space) => return None,
                Err(ConeFail::Precision) => Level::L0.next()?,
            };
            ladder(&mut ConeStep(self), start, &mut [(); LEVELS]).ok()
        })
    }

    /// Whether the exact normal numerator is zero at a corner: then no
    /// cone is certified at any level or depth (module docs). The corners
    /// are the ends of the numerator's lines on the sides `u = 0` and
    /// `u = 1`, which depend on the net's two rows there alone
    /// ([`HomPatch::side_numerator`]). Computed once.
    fn degenerate(&self) -> bool {
        *self.degenerate.get_or_init(|| {
            let exact = self.exact();
            [Side::U0, Side::U1].into_iter().any(|side| {
                let line = exact
                    .side_numerator(side)
                    .expect("a biquadratic patch's numerator is within the degree limit");
                let zero = |end: fn(&[Rational]) -> &Rational| {
                    line.iter().all(|p| *end(p.coefs()) == 0u32)
                };
                zero(|c| &c[0]) || zero(|c| &c[c.len() - 1])
            })
        })
    }

    /// The patch at `like`'s level translated by `−frame`: the
    /// coefficients `w·(x − c)` as `w·t + w·r` for the translation
    /// `x − c = t + r` rounded once (module docs), and the weights `w`.
    fn framed<S: Scalar>(&self, like: &S) -> HomPatch<S> {
        let c = self.frame();
        let net = self.net.iter().map(|p| {
            let w = p[3];
            let at = |k: usize| {
                // Exact: `frame` keeps only certified translations.
                let (t, r) = two_sum(p[k], -c[k]);
                let wt = S::product(t, w, like);
                if r == 0.0 {
                    wt
                } else {
                    wt.add(&S::product(r, w, like))
                }
            };
            [at(0), at(1), at(2), S::from_f64(w, like)]
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

/// The box of the net's points, exactly.
fn hull_of(net: &Net) -> HullBox {
    let mut b = HullBox {
        lo: [f64::INFINITY; 3],
        hi: [f64::NEG_INFINITY; 3],
    };
    for p in net {
        for k in 0..3 {
            if p[k] < b.lo[k] {
                b.lo[k] = p[k];
            }
            if p[k] > b.hi[k] {
                b.hi[k] = p[k];
            }
        }
    }
    b
}

/// The corner `(i, j)` at `(u, v)` if both are exactly 0 or 1 (either
/// zero).
fn corner(u: f64, v: f64) -> Option<(usize, usize)> {
    let end = |t: f64| {
        if t == 0.0 {
            Some(0)
        } else if t == 1.0 {
            Some(DEGREE)
        } else {
            None
        }
    };
    Some((end(u)?, end(v)?))
}

/// One level of the cone search: the cone in space at the level, asking
/// for the next while rounding decides.
struct ConeStep<'a>(&'a Tile);

impl AtLevel<()> for ConeStep<'_> {
    type Out = Result<(NormalCone, Level), ConeFail>;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Self::Out {
        cone_in_space(numerator(&self.0.framed(like))).map(|c| (c, like.level()))
    }

    fn escalates(out: &Self::Out) -> bool {
        *out == Err(ConeFail::Precision)
    }
}

/// The cone of the numerator `n` at its level, split in space while the
/// coefficients spread too far (module docs): the whole numerator about
/// the axis of the coefficients' smallest cone, then all pieces of each
/// depth about their mean direction, up to [`MAX_CONE_DEPTH`]. Stops at
/// the first failure rounding decides.
fn cone_in_space<S: Scalar>(n: [Tensor<S>; 3]) -> Result<NormalCone, ConeFail> {
    let mut pieces = vec![n];
    let mut depth = 0;
    loop {
        let axis = if depth == 0 {
            Axis::Smallest
        } else {
            Axis::Mean
        };
        match NormalCone::of_forms(&pieces, axis) {
            Err(ConeFail::Space) if depth < MAX_CONE_DEPTH => {}
            out => return out,
        }
        depth += 1;
        pieces = pieces.iter().flat_map(|p| split_both(p)).collect();
    }
}

/// The four quarters of `n` split at ½ in `u`, then each half in `v`.
fn split_both<S: Scalar>(n: &[Tensor<S>; 3]) -> [[Tensor<S>; 3]; 4] {
    let halves = |n: &[Tensor<S>; 3], dir| {
        let [a, b, c] = n.each_ref().map(|f| f.split_half(dir));
        let [a0, a1] = a;
        let [b0, b1] = b;
        let [c0, c1] = c;
        [[a0, b0, c0], [a1, b1, c1]]
    };
    let [lo, hi] = halves(n, Dir::U);
    let [q0, q1] = halves(&lo, Dir::V);
    let [q2, q3] = halves(&hi, Dir::V);
    [q0, q1, q2, q3]
}

fn numerator<S: Scalar>(p: &HomPatch<S>) -> [Tensor<S>; 3] {
    p.normal_numerator()
        .expect("a biquadratic patch's numerator is within the degree limit")
}

#[cfg(test)]
mod tests;
