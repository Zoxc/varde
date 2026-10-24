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
//! **Cached per tile** (computed once, on first use, from the net and
//! the declared singular sides alone, so every copy of a tile has the
//! same bits): the weight ratio `κ_w` ([`Tile::kappa_w`], at
//! construction), the first-level patch, the hull box, the homogeneous
//! partial derivatives, the normal numerator, the fold check's exact
//! part, and the normal cone with the level that certified it and the
//! work it took.
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
//! **Singular sides.** A side of the tile that is one point in space (a
//! sphere's pole row, a cone's or spindle torus's apex row, a revolved
//! conic's pole or apex row) makes `N` zero along it, so no cone of `N`
//! exists. Its constructor declares it ([`Tile::with_singular`]; the
//! lattice does, per form), and the **fold check** proves it on the
//! exact net (the numerator's line there exactly zero,
//! [`HomPatch::singular_sides`]) and divides its linear factor (`u`,
//! `1 − u`, `v` or `1 − v`) out of `N` at each level, exactly in
//! Bernstein form ([`SingularSides::quotient`]): the cone is that of the
//! quotient, a positive multiple of `N` inside the square whose limit on
//! the side is the normal there. A side declared but not singular, or a
//! corner where the exact numerator the check reads is zero (an
//! undeclared pole or apex row, a corner whose edges leave it parallel,
//! a cusp), **stops** the check: no cone at any level or depth, and none
//! is tried ([`Tile::fold_stops`]). The corners are read on the exact
//! numerator's side lines, which depend on the net's two rows there
//! alone: a corner on no singular side the line's end, one next to a
//! singular side the coefficient beside it on the other side's line
//! (the quotient's corner, up to a positive factor), and one between two
//! singular sides the corner of the whole exact quotient.
//!
//! **The cone is a certificate**, found in the tile's frame, then in
//! space, then in precision, on the numerator the fold check reads. At a
//! level the cone is first tried on its coefficients
//! ([`NormalCone::of_forms`] about the axis of their smallest cone);
//! where they spread too far ([`ConeFail::Space`]) the numerator is split
//! at ½ in both directions, up to [`MAX_CONE_DEPTH`] times, and the cone
//! tried on all the pieces' coefficients together (about their mean
//! direction): every value of the numerator is a combination of one
//! piece's coefficients, and the pieces' coefficients lie closer to the
//! values. Where rounding decides ([`ConeFail::Precision`]) the next
//! level of the schedule follows ([`ladder`]), from the whole numerator
//! again. A tile whose normals spread over more than a half-space at the
//! deepest split is not regular (it must be split), nor one the top
//! level leaves undecided. The cone and its level are cached;
//! [`Tile::normal_cone_at`] runs the same search at one level, so it is
//! [`Tile::normal_cone`] at the level [`Tile::normal_cone_level`] names.
//!
//! **Work.** The search counts its work ([`ConeWork`]): the exact lines
//! formed, and the pieces tested at each level. An operation reading a
//! tile's cone charges it ([`Tile::normal_cone_charged`]; the unit
//! table's fold row): the exact part as work, each level's numerator and
//! pieces as that level's step, a precision step above the first. The
//! search runs whole and is cached before it is charged: it is bounded
//! (the levels times `1 + 4 + 16 + 64` pieces), and every call charges
//! the same, cached or not, so where a pass runs out doesn't depend on
//! which of its items asked first.
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
//!
//! **The patch trait.** A tile is a [`Patch`] at every level: its net
//! formed at the level ([`Tile::patch_at`]; at the first level the cached
//! patch and numerator), a corner's point its control point exactly, the
//! split, hull box, frame, cone search and its cache the tile's own, the
//! singular sides proven on the exact net (whichever sides are asked;
//! the cone reads the declared ones), the closest point set up from the
//! stored net; the rest the patch's operations at the level.

use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::sync::OnceLock;

use malachite::Rational;
pub use varde_decide::bezier::NormalCone;
use varde_decide::bezier::{
    Axis, Closest, ConeFail, Dir, HomPatch, HullBox, Partials, Poly, Scalar, Side, Sides,
    SingularSides, Tensor,
};
use varde_decide::num::bounded::Bounded;
use varde_decide::num::expansion::two_sum;
use varde_decide::num::level::{AtLevel, LevelScalar, at_level, ladder};
use varde_decide::num::schedule::{LEVELS, Level, UNITS};
use varde_decide::num::storage::{frame_of, translate};
use varde_decide::patch::Patch;

use super::condition::kappa;
use super::formkey::Charge;
use super::round::nearest;
use crate::infra::budget::{Step, level_step};

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
pub type FirstLevelPatch = HomPatch<Bounded<f64>>;

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
    /// The declared singular sides.
    sides: Sides,
    patch: FirstLevelPatch,
    hull: HullBox,
    /// The largest weight over the least.
    kappa_w: f64,
    /// The homogeneous partials in `u` and `v`.
    partials: OnceLock<[FirstLevelPatch; 2]>,
    numerator: OnceLock<[Tensor<Bounded<f64>>; 3]>,
    cone: OnceLock<Cone>,
    /// The fold check's exact part.
    fold: OnceLock<Fold>,
}

/// The fold check's exact part, on the stored net (module docs).
#[derive(Clone, Copy, Debug)]
struct Fold {
    /// The proof of the declared sides (`Some(None)` for no sides);
    /// `None` where a declared side isn't singular or the numerator the
    /// check reads is zero at a corner: no cone at any level.
    proof: Option<Option<SingularSides>>,
    /// The exact numerator's lines formed and whether it was formed in
    /// full.
    lines: u32,
    full: bool,
}

/// The cone search's outcome and its work, computed once.
#[derive(Clone, Copy, Debug)]
struct Cone {
    found: Option<(NormalCone, Level)>,
    work: ConeWork,
}

/// The work of a tile's fold check and cone search, as it ran: what
/// [`Tile::normal_cone_charged`] charges, and the counts per level that
/// calibrate it. A function of the tile alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConeWork {
    /// The exact numerator's lines formed on the stored net: the
    /// singular sides' proof and the corner test.
    pub exact_lines: u32,
    /// Whether the exact numerator was formed in full (a corner between
    /// two singular sides).
    pub exact_full: bool,
    /// The pieces whose cone was tested at each level, 0 at a level the
    /// search didn't run at.
    pub pieces: [u64; LEVELS],
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
            sides: Sides::NONE,
            patch,
            hull: hull_of(&net),
            kappa_w: kappa(net.iter().map(|p| p[3])),
            partials: OnceLock::new(),
            numerator: OnceLock::new(),
            cone: OnceLock::new(),
            fold: OnceLock::new(),
        })
    }

    /// The tile with `sides` declared singular (a pole or an apex row
    /// each; module docs): its fold check proves them on the exact net
    /// and divides their factors out of the numerator. A side declared
    /// but not singular leaves the tile not regular.
    pub fn with_singular(mut self, sides: Sides) -> Tile {
        if sides != self.sides {
            self.sides = sides;
            self.cone = OnceLock::new();
            self.fold = OnceLock::new();
        }
        self
    }

    /// The declared singular sides.
    #[inline]
    pub fn singular(&self) -> Sides {
        self.sides
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

    /// The weight ratio `κ_w`: the largest weight over the least,
    /// rounded to the nearest `f64` ([`kappa`]), at least 1 and at most
    /// `MAX_WEIGHT/MIN_WEIGHT`. The factor by which the weights' rounding
    /// grows in the tile's evaluations; the lattices' weight conditioning
    /// keeps it least ([`super::condition`]).
    #[inline]
    pub fn kappa_w(&self) -> f64 {
        self.kappa_w
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
    pub fn patch(&self) -> &FirstLevelPatch {
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
    pub fn derivative(&self, dir: Dir) -> &FirstLevelPatch {
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
    /// error only where a value rounds past the range. A declared
    /// singular side stays declared on the halves it bounds.
    pub fn split_half(&self, dir: Dir) -> Result<[Tile; 2], TileError> {
        let [a, b] = self.exact().split_half(dir);
        let (lo, hi) = match dir {
            Dir::U => (Side::U1, Side::U0),
            Dir::V => (Side::V1, Side::V0),
        };
        let keep = |off: Side| self.sides.iter().filter(|&s| s != off).collect();
        Ok([
            Tile::rounded(&a)?.with_singular(keep(lo)),
            Tile::rounded(&b)?.with_singular(keep(hi)),
        ])
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
    /// at the levels above in turn (module docs), of the numerator the
    /// fold check reads (divided by the singular sides); `None` where no
    /// level certifies one. Computed once.
    pub fn normal_cone(&self) -> Option<NormalCone> {
        self.cone_certificate().found.map(|(c, _)| c)
    }

    /// The level that certified [`normal_cone`](Self::normal_cone),
    /// `None` where none did.
    pub fn normal_cone_level(&self) -> Option<Level> {
        self.cone_certificate().found.map(|(_, l)| l)
    }

    /// [`normal_cone`](Self::normal_cone), its work charged to `c`
    /// ([`ConeWork`]: the exact part's lines, then each level's
    /// numerator and pieces, a level above the first as a precision
    /// step). The search runs whole and is cached, then charged: it is
    /// bounded (the levels times [`MAX_CONE_DEPTH`]'s pieces), and every
    /// call charges the same, cached or not, so where a pass runs out
    /// doesn't depend on which item asked first.
    pub(crate) fn normal_cone_charged<C: Charge>(
        &self,
        c: &mut C,
    ) -> Result<Option<NormalCone>, C::Err> {
        let cone = self.cone_certificate();
        let u = UNITS.fold;
        let w = &cone.work;
        let exact = u64::from(w.exact_lines) * u.line + if w.exact_full { u.full } else { 0 };
        if exact > 0 {
            c.charge(Step::Work, exact)?;
        }
        for (l, &pieces) in w.pieces.iter().enumerate() {
            if pieces > 0 {
                let units = u.level[l].saturating_add(pieces.saturating_mul(u.piece[l]));
                c.charge(level_step(l as u8), units)?;
            }
        }
        Ok(cone.found.map(|(n, _)| n))
    }

    /// The work [`normal_cone`](Self::normal_cone) did (computing it if
    /// it hasn't run): the counts per level.
    pub fn cone_work(&self) -> ConeWork {
        self.cone_certificate().work
    }

    /// The certified normal cone with the numerator computed at `like`'s
    /// level (and width), in the tile's frame, divided by the singular
    /// sides, split in space as at any level but not escalated past it
    /// (module docs): at the level
    /// [`normal_cone_level`](Self::normal_cone_level) names it is
    /// [`normal_cone`](Self::normal_cone). `None` where it isn't
    /// certified at this level.
    pub fn normal_cone_at<S: Scalar>(&self, like: &S) -> Option<NormalCone> {
        let n = self.fold_numerator_at(like)?;
        cone_in_space(n).0.ok()
    }

    /// Whether the tile is certified regular: the numerator the fold
    /// check reads certified non-zero, by a normal cone at some level
    /// (module docs). Up to its singular sides, which the cone's
    /// quotient covers in the limit.
    pub fn is_regular(&self) -> bool {
        self.normal_cone().is_some()
    }

    /// The proof of the declared singular sides on the exact net
    /// ([`HomPatch::singular_sides`]); `None` for no sides and where the
    /// fold check stops ([`fold_stops`](Self::fold_stops)). Computed once.
    pub fn singular_proof(&self) -> Option<SingularSides> {
        self.fold().proof.flatten()
    }

    /// Whether the fold check stops before any level: a declared side
    /// isn't singular, or the numerator the check reads is zero at a
    /// corner of the exact net (module docs). Then no cone is certified
    /// at any level or depth, and none is tried. Computed once.
    pub fn fold_stops(&self) -> bool {
        self.fold().proof.is_none()
    }

    /// The numerator the fold check reads at `like`'s level (and width):
    /// the normal numerator in the tile's frame (as
    /// [`normal_numerator`](Self::normal_numerator) at the first level),
    /// divided by the linear factors of the singular sides
    /// ([`SingularSides::quotient`]); of degrees `(5, 5)` less one per
    /// singular side in its direction. `None` where the fold check stops
    /// ([`fold_stops`](Self::fold_stops)).
    pub fn fold_numerator_at<S: Scalar>(&self, like: &S) -> Option<[Tensor<S>; 3]> {
        self.fold_of(numerator(&self.framed(like)))
    }

    /// The normal numerator `n` divided by the singular sides' factors;
    /// `None` where the fold check stops.
    fn fold_of<S: Scalar>(&self, n: [Tensor<S>; 3]) -> Option<[Tensor<S>; 3]> {
        match self.fold().proof? {
            None => Some(n),
            Some(p) => p.quotient(&n),
        }
    }

    /// The cone, the level that certified it and the work, computed
    /// once: the first level from the cached numerator, then the
    /// schedule's levels in turn while rounding decides.
    fn cone_certificate(&self) -> &Cone {
        self.cone.get_or_init(|| {
            let fold = self.fold();
            let mut work = ConeWork {
                exact_lines: fold.lines,
                exact_full: fold.full,
                pieces: [0; LEVELS],
            };
            let Some(n) = self.fold_of(self.normal_numerator().clone()) else {
                return Cone { found: None, work };
            };
            let (out, pieces) = cone_in_space(n);
            work.pieces[0] = pieces;
            let found = match out {
                Ok(c) => Some((c, Level::L0)),
                Err(ConeFail::Space) => None,
                Err(ConeFail::Precision) => match Level::L0.next() {
                    Some(start) => ladder(&mut ConeStep(self), start, &mut work.pieces).ok(),
                    None => None,
                },
            };
            Cone { found, work }
        })
    }

    /// The fold check's exact part (module docs), computed once: the
    /// declared sides proven on the exact net, and the numerator the
    /// check reads tested for an exact zero at each corner. A corner on
    /// no singular side reads the end of a side's numerator line; next
    /// to one singular side, the coefficient beside it on the other
    /// side's line (the quotient's corner, up to a positive factor);
    /// between two, the quotient of the whole exact numerator.
    fn fold(&self) -> &Fold {
        self.fold
            .get_or_init(|| check_fold(&self.exact(), self.sides))
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

/// The tile at the level of `S` (module docs).
impl<S: Scalar + 'static> Patch<S> for Tile {
    type SplitError = TileError;

    fn degrees(&self) -> (usize, usize) {
        (DEGREE, DEGREE)
    }

    /// [`Tile::patch_at`]; at the first level the cached [`Tile::patch`].
    fn hom(&self, like: &S) -> Cow<'_, HomPatch<S>> {
        match first_level(&self.patch) {
            Some(p) => Cow::Borrowed(p),
            None => Cow::Owned(self.patch_at(like)),
        }
    }

    /// The patch's point at the level ([`HomPatch::eval`]), at a corner
    /// its control point exactly: [`Tile::eval`] at every level.
    fn eval(&self, u: &S, v: &S) -> [S; 3] {
        match corner_at(u, v) {
            Some((i, j)) => self.point(i, j).map(|x| S::from_f64(x, u)),
            None => self.hom(u).eval(u, v),
        }
    }

    /// [`HomPatch::eval_partials`] at the level, the point at a corner its
    /// control point exactly: [`Tile::eval_partials`] at every level.
    fn eval_partials(&self, u: &S, v: &S) -> Partials<S> {
        let mut d = self.hom(u).eval_partials(u, v);
        if let Some((i, j)) = corner_at(u, v) {
            d.p = self.point(i, j).map(|x| S::from_f64(x, u));
        }
        d
    }

    /// [`Tile::split_half`].
    fn split_half(&self, dir: Dir) -> Result<[Tile; 2], TileError> {
        Tile::split_half(self, dir)
    }

    /// [`Tile::hull_box`], at every level.
    fn hull_box(&self) -> Option<HullBox> {
        Some(self.hull)
    }

    /// In the tile's frame ([`Tile::normal_numerator`] at the first
    /// level, cached).
    fn normal_numerator(&self, like: &S) -> Option<[Tensor<S>; 3]> {
        Some(match first_level(Tile::normal_numerator(self)) {
            Some(n) => Clone::clone(n),
            None => numerator(&self.framed(like)),
        })
    }

    /// The search of [`Tile::normal_cone_at`] at the level, on the
    /// numerator the fold check reads, with why it fails:
    /// [`ConeFail::Space`] where the fold check stops (no level or depth
    /// helps, [`Tile::fold_stops`]) or at the deepest split.
    fn normal_cone_at(&self, like: &S) -> Result<NormalCone, ConeFail> {
        let n = Patch::normal_numerator(self, like).expect("a biquadratic numerator");
        cone_in_space(self.fold_of(n).ok_or(ConeFail::Space)?).0
    }

    /// [`Tile::normal_cone`]: escalated and cached.
    fn normal_cone(&self) -> Option<NormalCone> {
        Tile::normal_cone(self)
    }

    /// Proven on the exact net ([`Tile::exact`]), whatever the level.
    fn singular_sides(&self, sides: Sides, _: &S) -> Option<SingularSides> {
        self.exact().singular_sides(sides)
    }

    /// [`Closest::patch`] on the stored net.
    fn closest(&self, x: [f64; 3], like: &S) -> Option<Closest<S>> {
        Closest::patch(DEGREE, DEGREE, &self.net, x, like)
    }
}

/// The fold check's exact part on the exact net `exact` with the
/// declared singular sides `sides` ([`Tile::singular_proof`]).
fn check_fold(exact: &HomPatch<Rational>, sides: Sides) -> Fold {
    let mut lines = sides.iter().count() as u32;
    let proof = if sides.is_empty() {
        None
    } else {
        match exact.singular_sides(sides) {
            Some(p) => Some(p),
            None => {
                return Fold {
                    proof: None,
                    lines,
                    full: false,
                };
            }
        }
    };
    let mut line: [Option<[Poly<Rational>; 3]>; 4] = Default::default();
    let mut zero = false;
    let mut full = false;
    for su in [Side::U0, Side::U1] {
        for sv in [Side::V0, Side::V1] {
            let (du, dv) = (sides.contains(su), sides.contains(sv));
            // The line read and the place on it, counted from the
            // end at the other side.
            let (on, from, at) = match (du, dv) {
                (false, false) => (su, sv, 0),
                (false, true) => (su, sv, 1),
                (true, false) => (sv, su, 1),
                (true, true) => {
                    full = true;
                    continue;
                }
            };
            let l = line[on as usize].get_or_insert_with(|| {
                lines += 1;
                exact
                    .side_numerator(on)
                    .expect("a biquadratic patch's numerator is within the degree limit")
            });
            zero |= l.iter().all(|p| {
                let c = p.coefs();
                let k = if from.at_one() { c.len() - 1 - at } else { at };
                c[k] == 0u32
            });
        }
    }
    if full {
        let n = exact
            .normal_numerator()
            .expect("a biquadratic patch's numerator is within the degree limit");
        let q = proof
            .expect("two singular sides")
            .quotient(&n)
            .expect("the proven patch's numerator");
        let (m, k) = q[0].degrees();
        zero |= [(0, 0), (0, k), (m, 0), (m, k)]
            .into_iter()
            .any(|(i, j)| q.iter().all(|f| *f.get(i, j) == 0u32));
    }
    Fold {
        proof: if zero { None } else { Some(proof) },
        lines,
        full,
    }
}

/// `x` as a `T` where it is one: the first level's caches read through
/// the level-generic [`Patch`].
fn first_level<A: 'static, T: 'static>(x: &A) -> Option<&T> {
    (x as &dyn Any).downcast_ref()
}

/// The corner `(i, j)` at `(u, v)` if both are exactly 0 or 1, at any
/// level ([`corner`]'s rule).
fn corner_at<S: Scalar>(u: &S, v: &S) -> Option<(usize, usize)> {
    let end = |t: &S| {
        if t.is_int(0) {
            Some(0)
        } else if t.is_int(1) {
            Some(DEGREE)
        } else {
            None
        }
    };
    Some((end(u)?, end(v)?))
}

impl fmt::Debug for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tile")
            .field("net", &self.net)
            .field("sides", &self.sides)
            .finish_non_exhaustive()
    }
}

/// Tiles are equal when their nets are, bit for bit, and their declared
/// singular sides.
impl PartialEq for Tile {
    fn eq(&self, o: &Self) -> bool {
        self.sides == o.sides
            && self
                .net
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

/// One level of the cone search: the cone in space at the level of the
/// numerator the fold check reads, asking for the next while rounding
/// decides; the pieces tested in the level's slot.
struct ConeStep<'a>(&'a Tile);

impl AtLevel<u64> for ConeStep<'_> {
    type Out = Result<(NormalCone, Level), ConeFail>;

    fn at<S: LevelScalar>(&mut self, like: &S, pieces: &mut u64) -> Self::Out {
        let n = self.0.fold_numerator_at(like).ok_or(ConeFail::Space)?;
        let (out, tested) = cone_in_space(n);
        *pieces = tested;
        out.map(|c| (c, like.level()))
    }

    fn escalates(out: &Self::Out) -> bool {
        *out == Err(ConeFail::Precision)
    }
}

/// The cone of the numerator `n` at its level, split in space while the
/// coefficients spread too far (module docs): the whole numerator about
/// the axis of the coefficients' smallest cone, then all pieces of each
/// depth about their mean direction, up to [`MAX_CONE_DEPTH`]. Stops at
/// the first failure rounding decides. With the pieces tested.
fn cone_in_space<S: Scalar>(n: [Tensor<S>; 3]) -> (Result<NormalCone, ConeFail>, u64) {
    let mut pieces = vec![n];
    let mut depth = 0;
    let mut tested = 0u64;
    loop {
        let axis = if depth == 0 {
            Axis::Smallest
        } else {
            Axis::Mean
        };
        tested += pieces.len() as u64;
        match NormalCone::of_forms(&pieces, axis) {
            Err(ConeFail::Space) if depth < MAX_CONE_DEPTH => {}
            out => return (out, tested),
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

/// Hooks for the unit table's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times the fold check's
/// exact part and the cone search per precision level against the units
/// charged for them ([`ConeWork`]), on fixtures it shares with this
/// module's tests. Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;

    /// An octant of the unit sphere, its pole row at `v = 0` declared
    /// singular: the exact part forms three lines, the search divides
    /// the numerator by `v`.
    pub fn pole() -> Tile {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let arc = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0)];
        let net = std::array::from_fn(|k| {
            let ((cu, su, wu), (cv, sv, wv)) = (arc[k / 3], arc[k % 3]);
            [sv * cu, sv * su, cv, wu * wv]
        });
        Tile::new(net)
            .expect("a valid net")
            .with_singular(Side::V0.into())
    }

    /// A plane tile folding over in `u`: no cone at any depth, every
    /// piece of every depth tested (`1 + 4 + 16 + 64`).
    pub fn folded() -> Tile {
        let xs = [0.0, 2.0, 1.0];
        Tile::new(std::array::from_fn(|k| {
            [xs[k / 3], (k % 3) as f64, 0.0, 1.0]
        }))
        .expect("a valid net")
    }

    /// `x = (u·v, u·v², u²·v)` with its sides `u = 0` and `v = 0`
    /// declared singular: the exact part forms the whole numerator.
    pub fn corner() -> Tile {
        let (a, b) = ([0.0, 0.5, 1.0], [0.0, 0.0, 1.0]);
        let net = std::array::from_fn(|k| {
            let (i, j) = (k / 3, k % 3);
            [a[i] * a[j], a[i] * b[j], b[i] * a[j], 1.0]
        });
        Tile::new(net)
            .expect("a valid net")
            .with_singular(Sides::NONE.with(Side::U0).with(Side::V0))
    }

    /// The fold check's exact part run afresh on `tile`: the lines
    /// formed and whether the whole numerator was.
    pub fn fold_check(tile: &Tile) -> (u32, bool) {
        let f = check_fold(&tile.exact(), tile.sides);
        (f.lines, f.full)
    }

    /// The cone search at precision level `level` (from 0) run afresh on
    /// `tile`, its numerator formed there: the pieces tested; `None` for
    /// a level the schedule doesn't have or where the fold check stops.
    pub fn search_at(tile: &Tile, level: usize) -> Option<u64> {
        let level = Level::from_index(level)?;
        let mut pieces = 0;
        at_level(&mut Search(tile), level, &mut pieces)?;
        Some(pieces)
    }

    struct Search<'a>(&'a Tile);

    impl AtLevel<u64> for Search<'_> {
        type Out = Option<()>;

        fn at<S: LevelScalar>(&mut self, like: &S, pieces: &mut u64) -> Option<()> {
            let n = self.0.fold_numerator_at(like)?;
            *pieces = cone_in_space(n).1;
            Some(())
        }

        fn escalates(_: &Option<()>) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests;
