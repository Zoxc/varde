//! The dyadic boxes of a face: its parameter domain cut into boxes whose
//! sides are tile lines or dyadic iso lines inside one tile, each box
//! certified *outside* the trim, *core* (inside the trim and clear of
//! every trimming curve), or *mixed* (some trimming curve may meet it), the
//! counting's pieces of the face.
//!
//! **The face as the caller gives it** ([`FaceUv`]). The counting never
//! reads the store: the caller describes the face by its tiles, an integer
//! rectangle of the surface's global UV (tile `(i, j)` owns `[i, i + 1] ×
//! [j, j + 1]`), a length per unit of `u` and of `v` that says which side
//! of a box is the longer one ([`FaceUv::new`]'s `scale`, any fixed
//! positive values taken from the stored surface, the same at every
//! precision level), and its trimming curves: per coedge of every loop
//! the p-curve's rational quadratic spans in global UV, affine control
//! points with their weights `(u, v, w)` as stored, in the edge's
//! parameter direction, with whether the coedge runs against it
//! ([`Trim`]). Its outer loop runs counter-clockwise and its holes
//! clockwise seen against the face's outward normal, which is the
//! surface's unless the face is `flipped`: so in UV the trims wind `+1`
//! round a point inside an unflipped face and `−1` round one inside a
//! flipped face, `0` round a point outside. Splits read the tile's patch,
//! which the caller passes at the level it works at.
//!
//! **Boxes and keys.** A box lies in one tile, `[i, i + 1] × [j, j + 1]`
//! cut to the dyadic extents `[lo, hi] ⊆ [0, 1]` of the tile's local
//! coordinates, in lowest terms with denominators at most
//! `2^`[`MAX_BOX_DEPTH`] (so every side, and the box's centre, is an
//! `f64` exactly). Its key ([`BoxKey`]) is the face (operand and id), the
//! tile and the two extents by value, so a box has one key however it was
//! reached and from whichever caller, and keys order by face, tile, then
//! extents. A **tile line is a fixed side**: the boxes of a face start as
//! its whole tiles ([`FaceUv::tile_boxes`]) and every split is inside one
//! box, so no box ever straddles a tile line.
//!
//! **Splits, shifted off the query** ([`split`]). A box splits across its
//! longer side (its extent times the face's scale for that direction, ties
//! across `u`) at a point of the canonical sequence of relative positions
//! `½, ⅜, ⅝, 7/16, 9/16, 5/16, 11/16` of that extent, the first whose iso
//! line over the box has a shadow certified off the query point's shadow
//! `q_xy` ([`iso_off`]); a split with no query point is at `½`. The
//! counting reads the winding of each box's shadow boundary about `q_xy`
//! from angle increments of its sides, which a side through `q_xy` has
//! none of: a moved split leaves such a side only on tile lines (fixed,
//! read through their own side record) and where no position of the
//! sequence is certified, where the split is at `½` and the [`Split`]
//! says so. Every position lies in `[5/16, 11/16]`, so each half is at
//! most `11/16` of the box and refinement shrinks every box. A position
//! is skipped where, in lowest terms, it would pass [`MAX_BOX_DEPTH`];
//! with none left the box can't split ([`BoxError::Depth`]). The split is
//! a function of the box, the tile's patch, the face's scale and the
//! query alone.
//!
//! **Shadows off the query** ([`iso_off`]). The iso line is the patch's
//! boundary row after an exact-parameter split there (the row itself on a
//! tile line), cut to the box's extent by the canonical cover of that
//! extent by aligned dyadic intervals, each reached by splits at `½`. The
//! shadow of a piece misses `q_xy` where the piece in the equation of a
//! line across the shadow plane (`r̂·x` or `ν·x`, the shadow frame's
//! axes, against either end of the query's certified enclosure) has a
//! coefficient hull of one sign, certified; a piece that doesn't is
//! halved, at most [`OFF_DEPTH`] times.
//!
//! **Classification** ([`classify`]). Each span of each trim is tested
//! against the closed box: it is *separated* when the span, cut in halves
//! as needed (at most [`SEP_DEPTH`] deep, [`SEP_PIECES`] pieces), has every
//! piece certified beyond one of the box's four side lines, its coefficient
//! hull in that line's equation (`u − u_lo`, `u_hi − u`, `v − v_lo`,
//! `v_hi − v`, times the weight, which is positive) certified negative
//! (a span whose stored control points lie beyond a side is separated at
//! once). Halving stops early at a piece certainly in the box, or one of
//! whose ends is, and at a piece spread less than a sixteenth of the
//! box's width each way: such a piece runs along or next to the box's
//! boundary, and a smaller box separates it sooner.
//! A box some span isn't separated from is *mixed*, with the coedges of
//! all such spans, by id; it may touch or cross them, or lie too close to
//! certify at this level and size (a curve along a box side, on a tile
//! line or a dyadic line, always leaves the box mixed: a core box never
//! touches a trim). A box separated from every span lies in one region
//! of the trim, and the trims' winding number about its centre `p` says
//! which: `0` *outside*, the face's inside winding *core*, anything else
//! a trim that isn't valid ([`BoxError::Winding`]).
//!
//! **The winding**, exact on the certified data. The ray from `p` along
//! `+u` is taken at the height `v_p − ε`, `ε` infinitely small, so a point
//! of a trim with `v ≥ v_p` is above it and one with `v < v_p` below (`p`
//! is off every trim, so this is the winding about `p`). Each span is
//! halved until every piece is certified above or below the line (its
//! hull in `v − v_p`), left of `p` (in `u − u_p`, never crossing the
//! ray) or right of it. Along a run of pieces right of `p` the net
//! crossing of the ray is half the change of side from the run's start to
//! its end; those ends are a span's ends, whose side is exact (their
//! points are stored control points, compared with `v_p` exactly), or a
//! neighbouring piece above or below the line, whose side is certified.
//! Joining spans share their control point bit for bit, so a trim through
//! a column of `p` is counted once. A coedge against its edge counts its
//! spans negated. Where pieces don't settle within [`WIND_DEPTH`] halvings
//! (or [`WIND_PIECES`]), the level can't decide ([`BoxError::Unsettled`]:
//! escalate the level).
//!
//! **Levels.** Classification and splits run at the level of the value
//! (or patch) passed, every hull certified there: a mixed box at one
//! level may separate at a higher one, never the other way round in
//! meaning (a core or outside answer is a certificate at any level).
//! Each call reports the pieces it tested, for the caller to charge.

use std::cell::OnceCell;
use std::ops::Range;

use malachite::Rational;
use smallvec::SmallVec;

use crate::bezier::{Dir, Dyadic, HomCurve, HomPatch, Scalar};
use crate::ids::{CoedgeId, FaceId};
use crate::num::bounded::Bounded;
use crate::perturb::{NU, R_HAT};
use crate::records::keys::Operand;

/// The finest box: extents in lowest terms with denominators at most
/// `2⁵²`, so a side and the centre between two are `f64`s exactly.
pub const MAX_BOX_DEPTH: u32 = 52;

/// The most halvings of a span against one box: past the finest box, so a
/// box at any depth as near a span as its own width separates from it.
pub const SEP_DEPTH: u32 = MAX_BOX_DEPTH + 8;

/// The most pieces of a span tested against one box; past it the span
/// counts as meeting the box (mixed), and refining the box goes on.
pub const SEP_PIECES: u32 = 256;

/// The most halvings of a span settling the winding about a box's centre:
/// past the finest box, as [`SEP_DEPTH`].
pub const WIND_DEPTH: u32 = MAX_BOX_DEPTH + 8;

/// The most pieces of a span settling the winding.
pub const WIND_PIECES: u32 = 4096;

/// The most halvings of an iso line's piece certifying its shadow off the
/// query.
pub const OFF_DEPTH: u32 = 6;

/// Tile indices lie in `(−2⁵², 2⁵²)`, exact in `f64`; a global side or
/// centre is the index plus the local dyadic, formed at the level.
const MAX_TILE: i64 = 1 << 52;

/// The relative split positions in order, `num/2^exp`: the middle, then
/// shifted by `⅛`, then by `1/16` and `3/16`, the lesser first.
const SHIFTS: [(u64, u32); 7] = [(1, 1), (3, 3), (5, 3), (7, 4), (9, 4), (5, 4), (11, 4)];

/// A trim's span: three affine control points with their weights,
/// `(u, v, w)`, in global UV.
pub type UvSpan = [[f64; 3]; 3];

/// A dyadic extent `[lo, hi]` of a tile's local coordinate, `lo < hi`, in
/// lowest terms with denominators at most `2^`[`MAX_BOX_DEPTH`]; ordered
/// by `lo`, then `hi`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Extent {
    lo: Dyadic,
    hi: Dyadic,
}

impl Extent {
    /// The whole tile, `[0, 1]`.
    pub const WHOLE: Extent = Extent {
        lo: Dyadic::ZERO,
        hi: Dyadic::ONE,
    };

    /// `[lo, hi]`, `None` unless `lo < hi` and both denominators are at
    /// most `2^`[`MAX_BOX_DEPTH`].
    pub fn new(lo: Dyadic, hi: Dyadic) -> Option<Extent> {
        (lo < hi && lo.exp() <= MAX_BOX_DEPTH && hi.exp() <= MAX_BOX_DEPTH)
            .then_some(Extent { lo, hi })
    }

    /// The lower end.
    #[inline]
    pub fn lo(self) -> Dyadic {
        self.lo
    }

    /// The upper end.
    #[inline]
    pub fn hi(self) -> Dyadic {
        self.hi
    }

    /// The exponent of the ends' common denominator.
    #[inline]
    pub fn depth(self) -> u32 {
        let (a, b) = (self.lo.exp(), self.hi.exp());
        if a > b { a } else { b }
    }

    /// The ends' numerators over `2^depth`.
    fn nums(self) -> (u64, u64, u32) {
        let e = self.depth();
        (
            self.lo.num() << (e - self.lo.exp()),
            self.hi.num() << (e - self.hi.exp()),
            e,
        )
    }

    /// The width as an `f64`, exactly (a numerator below `2⁵³`).
    fn width(self) -> f64 {
        let (a, b, e) = self.nums();
        float(b - a, e)
    }

    /// The point at `num/2^exp` of the way from `lo` to `hi` (`exp ≤ 4`),
    /// `None` unless its denominator in lowest terms is at most
    /// `2^`[`MAX_BOX_DEPTH`].
    fn at(self, num: u64, exp: u32) -> Option<Dyadic> {
        let (a, b, e) = self.nums();
        // Below `2^(e + exp) ≤ 2⁵⁶`: no overflow, and within the dyadics'
        // depth before reduction.
        let n = a * ((1 << exp) - num) + b * num;
        Dyadic::new(n, e + exp).filter(|t| t.exp() <= MAX_BOX_DEPTH)
    }

    /// The middle, exactly (denominator at most `2⁵³`).
    fn mid(self) -> Dyadic {
        let (a, b, e) = self.nums();
        Dyadic::new(a + b, e + 1).expect("the middle of [0, 1] is in it")
    }

    /// Whether `t` lies strictly inside.
    fn inside(self, t: Dyadic) -> bool {
        self.lo < t && t < self.hi
    }
}

/// `num/2^exp` as an `f64`, exactly: `num < 2⁵³`, `exp ≤ 53`.
fn float(num: u64, exp: u32) -> f64 {
    debug_assert!(num < 1 << 53 && exp <= 53);
    num as f64 * f64::from_bits((1023 - u64::from(exp)) << 52)
}

/// A dyadic point of `[0, 1]` as an `f64`, exactly (a box's sides and
/// centre have denominators at most `2⁵³`).
fn dy(t: Dyadic) -> f64 {
    float(t.num(), t.exp())
}

/// A box of a face: one tile cut to two dyadic extents (module docs). Its
/// value is its key: equal boxes are equal keys, ordered by operand,
/// face, tile (`u` index first), then the `u` extent and the `v` extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoxKey {
    operand: Operand,
    face: FaceId,
    tile: [i64; 2],
    u: Extent,
    v: Extent,
}

impl BoxKey {
    /// The box of `face` of `operand` in tile `tile` over the extents `u`
    /// and `v` of its local coordinates.
    pub fn new(operand: Operand, face: FaceId, tile: [i64; 2], u: Extent, v: Extent) -> BoxKey {
        BoxKey {
            operand,
            face,
            tile,
            u,
            v,
        }
    }

    /// The operand of the face.
    #[inline]
    pub fn operand(&self) -> Operand {
        self.operand
    }

    /// The face.
    #[inline]
    pub fn face(&self) -> FaceId {
        self.face
    }

    /// The tile `(i, j)`.
    #[inline]
    pub fn tile(&self) -> [i64; 2] {
        self.tile
    }

    /// The extent in `dir`.
    #[inline]
    pub fn extent(&self, dir: Dir) -> Extent {
        match dir {
            Dir::U => self.u,
            Dir::V => self.v,
        }
    }

    /// The deeper of the two extents' depths.
    pub fn depth(&self) -> u32 {
        let (a, b) = (self.u.depth(), self.v.depth());
        if a > b { a } else { b }
    }

    /// The two boxes either side of the iso line where the local
    /// coordinate in `dir` is `at`, lower first; `None` unless `at` lies
    /// strictly inside the extent and within [`MAX_BOX_DEPTH`].
    pub fn split_at(&self, dir: Dir, at: Dyadic) -> Option<[BoxKey; 2]> {
        let e = self.extent(dir);
        if !e.inside(at) || at.exp() > MAX_BOX_DEPTH {
            return None;
        }
        let (lo, hi) = (Extent { lo: e.lo, hi: at }, Extent { lo: at, hi: e.hi });
        let mut a = *self;
        let mut b = *self;
        match dir {
            Dir::U => (a.u, b.u) = (lo, hi),
            Dir::V => (a.v, b.v) = (lo, hi),
        }
        Some([a, b])
    }

    /// The box's sides in global UV, `[[u_lo, u_hi], [v_lo, v_hi]]`, at
    /// `like`'s level: each the tile index plus the local end, exact from
    /// double-double up, certified at the first level.
    pub fn sides<S: Scalar>(&self, like: &S) -> [[S; 2]; 2] {
        let g = |i: i64, t: Dyadic| {
            // `|i| < 2⁵²`, checked by the face: exact.
            S::from_f64(i as f64, like).add(&S::from_f64(dy(t), like))
        };
        [
            [g(self.tile[0], self.u.lo), g(self.tile[0], self.u.hi)],
            [g(self.tile[1], self.v.lo), g(self.tile[1], self.v.hi)],
        ]
    }

    /// The centre's global UV, exactly, as rationals.
    #[cfg(test)]
    fn centre_exact(&self) -> [Rational; 2] {
        [
            self.centre_coord_exact(Dir::U),
            self.centre_coord_exact(Dir::V),
        ]
    }

    /// The centre's global coordinate in `dir`, exactly.
    fn centre_coord_exact(&self, dir: Dir) -> Rational {
        let k = usize::from(dir == Dir::V);
        let t = self.extent(dir).mid();
        Rational::from(self.tile[k]) + Rational::try_from(dy(t)).expect("a finite dyadic")
    }
}

/// One coedge's trimming curve in its face's global UV (module docs).
#[derive(Clone, Copy, Debug)]
pub struct Trim<'a> {
    /// The coedge.
    pub coedge: CoedgeId,
    /// Whether the coedge runs against its edge, and so against the
    /// spans' direction.
    pub reversed: bool,
    /// The p-curve's spans along the edge's parameter.
    pub spans: &'a [UvSpan],
}

/// Why a face or a box is refused, or a box can't be split or classified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxError {
    /// The face's tile ranges are empty or reach `±2⁵²`, or a scale isn't
    /// finite and positive.
    Grid,
    /// Span `span` of `coedge`'s trim has a value that isn't finite or a
    /// weight that isn't positive.
    Span { coedge: CoedgeId, span: u32 },
    /// The box isn't of this face, or not in its tiles.
    Foreign,
    /// The box is too deep to split ([`MAX_BOX_DEPTH`]).
    Depth,
    /// The winding about the box isn't settled at this level from
    /// `coedge`'s trim: escalate.
    Unsettled { coedge: CoedgeId },
    /// The trims wind this many times round a box clear of them: neither
    /// 0 nor the face's inside winding, so they aren't a valid trim.
    Winding(i64),
}

/// A face as the counting reads it (module docs), validated.
#[derive(Clone, Debug)]
pub struct FaceUv<'a> {
    operand: Operand,
    face: FaceId,
    tiles: [Range<i64>; 2],
    scale: [f64; 2],
    trims: &'a [Trim<'a>],
    flipped: bool,
}

impl<'a> FaceUv<'a> {
    /// Face `face` of `operand`: its tiles `tiles[0] × tiles[1]` (`u`
    /// indices, `v` indices), the length per unit of `u` and of `v` its
    /// splits compare (`scale`), its trims (every coedge of every loop)
    /// and whether it is flipped. Refused unless the ranges are non-empty
    /// within `(−2⁵², 2⁵²)`, the scales finite and positive, and every
    /// span's values finite with positive weights.
    pub fn new(
        operand: Operand,
        face: FaceId,
        tiles: [Range<i64>; 2],
        scale: [f64; 2],
        trims: &'a [Trim<'a>],
        flipped: bool,
    ) -> Result<FaceUv<'a>, BoxError> {
        let ok_range = |r: &Range<i64>| r.start < r.end && -MAX_TILE < r.start && r.end < MAX_TILE;
        let ok_scale = |s: f64| s.is_finite() && s > 0.0;
        if !tiles.iter().all(ok_range) || !scale.into_iter().all(ok_scale) {
            return Err(BoxError::Grid);
        }
        for t in trims {
            for (k, s) in t.spans.iter().enumerate() {
                let bad = s
                    .iter()
                    .any(|p| !p.iter().all(|x| x.is_finite()) || p[2] <= 0.0);
                if bad {
                    return Err(BoxError::Span {
                        coedge: t.coedge,
                        span: u32::try_from(k).unwrap_or(u32::MAX),
                    });
                }
            }
        }
        Ok(FaceUv {
            operand,
            face,
            tiles,
            scale,
            trims,
            flipped,
        })
    }

    /// The trims.
    #[inline]
    pub fn trims(&self) -> &'a [Trim<'a>] {
        self.trims
    }

    /// The winding of the trims round a point inside: `−1` on a flipped
    /// face, else `+1`.
    #[inline]
    pub fn inside_winding(&self) -> i64 {
        if self.flipped { -1 } else { 1 }
    }

    /// The whole tiles as boxes, in key order (`u` index first): where
    /// refinement starts.
    pub fn tile_boxes(&self) -> impl Iterator<Item = BoxKey> + '_ {
        let [us, vs] = self.tiles.clone();
        us.flat_map(move |i| {
            vs.clone().map(move |j| {
                BoxKey::new(
                    self.operand,
                    self.face,
                    [i, j],
                    Extent::WHOLE,
                    Extent::WHOLE,
                )
            })
        })
    }

    /// Whether `b` is a box of this face in its tiles.
    pub fn owns(&self, b: &BoxKey) -> bool {
        b.operand == self.operand
            && b.face == self.face
            && self.tiles[0].contains(&b.tile[0])
            && self.tiles[1].contains(&b.tile[1])
    }
}

/// What a box is against the trims.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Class {
    /// Certified outside the trim and clear of every trimming curve.
    Outside,
    /// Certified inside the trim and clear of every trimming curve.
    Core,
    /// Some trimming curves may meet the box: these coedges', ascending.
    Mixed(SmallVec<[CoedgeId; 4]>),
}

/// A box classified, with the work it took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    /// The class.
    pub class: Class,
    /// The span pieces tested.
    pub pieces: u64,
}

/// The box `b` of `face` against its trims at `like`'s level (module
/// docs).
pub fn classify<S: Scalar>(
    face: &FaceUv<'_>,
    b: &BoxKey,
    like: &S,
) -> Result<Classified, BoxError> {
    if !face.owns(b) {
        return Err(BoxError::Foreign);
    }
    let sides = b.sides(like);
    let outer = sides
        .each_ref()
        .map(|[lo, hi]| [lo.enclose()[0], hi.enclose()[1]]);
    let mut pieces = 0u64;
    let mut meets: SmallVec<[CoedgeId; 4]> = SmallVec::new();
    for t in face.trims {
        for (k, s) in t.spans.iter().enumerate() {
            let hull = control_box(s);
            // The control points beyond a side's certified enclosure: the
            // span, in their hull, misses the box.
            if (0..2).any(|k| hull[k][1] < outer[k][0] || hull[k][0] > outer[k][1]) {
                continue;
            }
            let c = uv_curve(s, like).ok_or(span_error(t, k))?;
            let mut budget = SEP_PIECES;
            if !separated(&c, &sides, SEP_DEPTH, &mut budget) {
                meets.push(t.coedge);
            }
            pieces = pieces.saturating_add(u64::from(SEP_PIECES - budget));
        }
    }
    if !meets.is_empty() {
        meets.sort_unstable();
        meets.dedup();
        return Ok(Classified {
            class: Class::Mixed(meets),
            pieces,
        });
    }
    let p = b.centre(like);
    let pe = p.each_ref().map(|x| x.enclose());
    let ray = RayLine {
        enclosure: pe[1],
        b,
        exact: OnceCell::new(),
    };
    let mut w = 0i64;
    for t in face.trims {
        for (k, s) in t.spans.iter().enumerate() {
            let hull = control_box(s);
            // Above or below the ray's line, or left of the centre, as a
            // whole: no crossing.
            if hull[1][0] > pe[1][1] || hull[1][1] < pe[1][0] || hull[0][1] < pe[0][0] {
                continue;
            }
            let n = if hull[0][0] > pe[0][1] {
                // Right of the centre as a whole: one run.
                let (first, last) = (ray.side(s[0][1]), ray.side(s[2][1]));
                i64::from((last - first) / 2)
            } else {
                let c = uv_curve(s, like).ok_or(span_error(t, k))?;
                let mut budget = WIND_PIECES;
                let n = crossings(&c, s, &p, &ray, &mut budget)
                    .ok_or(BoxError::Unsettled { coedge: t.coedge })?;
                pieces = pieces.saturating_add(u64::from(WIND_PIECES - budget));
                n
            };
            let n = if t.reversed { -n } else { n };
            w = w.checked_add(n).ok_or(BoxError::Winding(i64::MAX))?;
        }
    }
    let class = if w == 0 {
        Class::Outside
    } else if w == face.inside_winding() {
        Class::Core
    } else {
        return Err(BoxError::Winding(w));
    };
    Ok(Classified { class, pieces })
}

impl BoxKey {
    /// The centre's global UV at `like`'s level (exact sums at every level
    /// but the first, certified there).
    fn centre<S: Scalar>(&self, like: &S) -> [S; 2] {
        let g = |i: i64, t: Dyadic| S::from_f64(i as f64, like).add(&S::from_f64(dy(t), like));
        [g(self.tile[0], self.u.mid()), g(self.tile[1], self.v.mid())]
    }
}

/// The error naming span `k` of `t`.
fn span_error(t: &Trim<'_>, k: usize) -> BoxError {
    BoxError::Span {
        coedge: t.coedge,
        span: u32::try_from(k).unwrap_or(u32::MAX),
    }
}

/// The box of a span's control points, `[[u_lo, u_hi], [v_lo, v_hi]]`.
fn control_box(s: &UvSpan) -> [[f64; 2]; 2] {
    std::array::from_fn(|k| {
        s.iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], p| {
                [
                    if p[k] < lo { p[k] } else { lo },
                    if p[k] > hi { p[k] } else { hi },
                ]
            })
    })
}

/// The span at `like`'s level, `None` unless its weights are certified
/// positive.
fn uv_curve<S: Scalar>(s: &UvSpan, like: &S) -> Option<HomCurve<S, 2>> {
    let c = HomCurve::from_affine(s.iter().map(|p| ([p[0], p[1]], p[2])), like)?;
    c.weights_positive().then_some(c)
}

/// The unit vector of axis `k` in the plane, at `like`'s level.
fn axis<S: Scalar>(k: usize, like: &S) -> [S; 2] {
    let (one, zero) = (S::int(1, like), S::int(0, like));
    if k == 0 { [one, zero] } else { [zero, one] }
}

/// Whether every piece of `c` is certified beyond one of the closed box's
/// side lines (module docs), halving at most `depth` more times and
/// `budget` pieces in all; `false` at once where the piece, or one of its
/// ends, lies certainly in the box.
fn separated<S: Scalar>(
    c: &HomCurve<S, 2>,
    sides: &[[S; 2]; 2],
    depth: u32,
    budget: &mut u32,
) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    let like = &c.weights().coefs()[0];
    // Certainly in the box: the whole piece, its start, its end.
    let mut inside = [true; 3];
    for (k, [lo, hi]) in sides.iter().enumerate() {
        let a = axis(k, like);
        let below = c.compose_linear(&a, &lo.neg());
        let hb = below.hull();
        if hb.hi < 0.0 {
            return true;
        }
        let above = c.compose_linear(&a, &hi.neg());
        let ha = above.hull();
        if ha.lo > 0.0 {
            return true;
        }
        inside[0] &= hb.lo >= 0.0 && ha.hi <= 0.0;
        let (b, a) = (below.coefs(), above.coefs());
        for (end, at) in [(1, 0), (2, b.len() - 1)] {
            inside[end] &= b[at].enclose()[0] >= 0.0 && a[at].enclose()[1] <= 0.0;
        }
    }
    if inside.contains(&true) || depth == 0 || small(c, sides) {
        return false;
    }
    let [l, r] = c.split_half();
    separated(&l, sides, depth - 1, budget) && separated(&r, sides, depth - 1, budget)
}

/// Whether the piece's control points spread less than a sixteenth of the
/// box's width in each direction: a piece that small and still not
/// separated lies along or next to the box's boundary, which a smaller
/// box separates sooner. A stopping rule only, read from the level's
/// enclosures.
fn small<S: Scalar>(c: &HomCurve<S, 2>, sides: &[[S; 2]; 2]) -> bool {
    let w = c.weights().coefs();
    c.coords().iter().zip(sides).all(|(p, [lo, hi])| {
        let width = hi.enclose()[0] - lo.enclose()[1];
        let mut range = [f64::INFINITY, f64::NEG_INFINITY];
        for (x, w) in p.coefs().iter().zip(w) {
            let ([xl, xh], [wl, _]) = (x.enclose(), w.enclose());
            if wl.is_nan() || wl <= 0.0 {
                return false;
            }
            let (a, b) = (xl / wl, xh / wl);
            for v in [a, b] {
                if v < range[0] {
                    range[0] = v;
                }
                if v > range[1] {
                    range[1] = v;
                }
            }
        }
        range[1] - range[0] < width * 0.0625
    })
}

/// Where a piece of a span lies against the ray from the box's centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    /// Certified above (`+1`) or below (`−1`) the ray's line.
    Off(i8),
    /// Certified left of the centre: never on the ray.
    Left,
    /// Certified right of the centre.
    Right,
}

/// The pieces of `c` in order, each settled against the ray from `p`
/// (module docs), halving at most `depth` more times; `None` where one
/// doesn't settle.
fn pieces<S: Scalar>(
    c: &HomCurve<S, 2>,
    p: &[S; 2],
    depth: u32,
    budget: &mut u32,
    out: &mut Vec<Piece>,
) -> Option<()> {
    if *budget == 0 {
        return None;
    }
    *budget -= 1;
    let like = &c.weights().coefs()[0];
    let v = c.compose_linear(&axis(1, like), &p[1].neg()).hull();
    if v.lo > 0.0 {
        out.push(Piece::Off(1));
        return Some(());
    }
    if v.hi < 0.0 {
        out.push(Piece::Off(-1));
        return Some(());
    }
    let u = c.compose_linear(&axis(0, like), &p[0].neg()).hull();
    if u.hi < 0.0 {
        out.push(Piece::Left);
        return Some(());
    }
    if u.lo > 0.0 {
        out.push(Piece::Right);
        return Some(());
    }
    if depth == 0 {
        return None;
    }
    let [l, r] = c.split_half();
    pieces(&l, p, depth - 1, budget, out)?;
    pieces(&r, p, depth - 1, budget, out)
}

/// The ray's line at `v_p − ε` through a box's centre: `v_p`'s certified
/// enclosure at the level, and `v_p` exactly when that doesn't decide.
struct RayLine<'b> {
    enclosure: [f64; 2],
    b: &'b BoxKey,
    exact: OnceCell<Rational>,
}

impl RayLine<'_> {
    /// The side of a stored point with `v`: `+1` from `v_p` up, else `−1`.
    fn side(&self, v: f64) -> i8 {
        let [lo, hi] = self.enclosure;
        if v > hi {
            return 1;
        }
        if v < lo {
            return -1;
        }
        let vp = self.exact.get_or_init(|| self.b.centre_coord_exact(Dir::V));
        if Rational::try_from(v).expect("finite, checked by the face") >= *vp {
            1
        } else {
            -1
        }
    }
}

/// The signed crossings of the ray from `p` (at `v_p − ε`, along `+u`)
/// by span `s` (`c` at the level), `+1` upwards (module docs); `None`
/// where the level doesn't settle them.
fn crossings<S: Scalar>(
    c: &HomCurve<S, 2>,
    s: &UvSpan,
    p: &[S; 2],
    ray: &RayLine<'_>,
    budget: &mut u32,
) -> Option<i64> {
    let mut out = Vec::new();
    pieces(c, p, WIND_DEPTH, budget, &mut out)?;
    let (first, last) = (ray.side(s[0][1]), ray.side(s[2][1]));
    let mut n = 0i64;
    // The side where the current run of right pieces started, and the
    // side at the boundary before the next piece where it is known.
    let mut run: Option<i8> = None;
    let mut before: Option<i8> = Some(first);
    for piece in out {
        match piece {
            Piece::Off(side) => {
                if let Some(start) = run.take() {
                    n += i64::from((side - start) / 2);
                }
                before = Some(side);
            }
            Piece::Left => {
                // A left piece never shares a point with a right one.
                if run.is_some() {
                    return None;
                }
                before = None;
            }
            Piece::Right => {
                if run.is_none() {
                    run = Some(before?);
                }
                before = None;
            }
        }
    }
    if let Some(start) = run {
        n += i64::from((last - start) / 2);
    }
    Some(n)
}

/// The query point's shadow: certified enclosures of `r̂·q` and `ν·q`
/// (the shadow frame's axes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QShadow {
    r: [f64; 2],
    n: [f64; 2],
}

impl QShadow {
    /// The shadow of the stored point `q`, `None` unless it is finite.
    pub fn of(q: [f64; 3]) -> Option<QShadow> {
        let dot = |a: [f64; 3]| {
            a.iter()
                .zip(q)
                .fold(Bounded::exact(0.0), |s, (&a, x)| {
                    s + Bounded::exact(a) * Bounded::exact(x)
                })
                .enclose()
        };
        QShadow::enclosing(dot([R_HAT.x, R_HAT.y, R_HAT.z]), dot([NU.x, NU.y, NU.z]))
    }

    /// The shadow of a point known by enclosures `r` of `r̂·q` and `n` of
    /// `ν·q`, each `[lo, hi]`; `None` unless finite and ordered.
    pub fn enclosing(r: [f64; 2], n: [f64; 2]) -> Option<QShadow> {
        let ok = |e: [f64; 2]| e[0].is_finite() && e[1].is_finite() && e[0] <= e[1];
        (ok(r) && ok(n)).then_some(QShadow { r, n })
    }
}

/// How a box splits (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    /// The direction whose extent is cut: the split line is the iso line
    /// where that local coordinate is `at`.
    pub dir: Dir,
    /// Where, in the tile's local coordinate.
    pub at: Dyadic,
    /// The two boxes, the lower first.
    pub halves: [BoxKey; 2],
    /// With a query point, whether the split line's shadow is certified
    /// off it (`false`: no position of the sequence was, the split is at
    /// the middle); `None` without one.
    pub off_q: Option<bool>,
    /// The iso line pieces tested.
    pub pieces: u64,
}

/// The split of `b` (module docs), `patch` its tile's patch at the level
/// worked at, `q` the query point's shadow if there is one.
pub fn split<S: Scalar>(
    face: &FaceUv<'_>,
    b: &BoxKey,
    patch: &HomPatch<S>,
    q: Option<&QShadow>,
) -> Result<Split, BoxError> {
    if !face.owns(b) {
        return Err(BoxError::Foreign);
    }
    let (wu, wv) = (b.u.width() * face.scale[0], b.v.width() * face.scale[1]);
    let first = if wv > wu { Dir::V } else { Dir::U };
    let mut pieces = 0u64;
    for dir in [first, other(first)] {
        let ext = b.extent(dir);
        let along = b.extent(other(dir));
        let mut middle = None;
        for (num, exp) in SHIFTS {
            let Some(at) = ext.at(num, exp) else {
                continue;
            };
            let halves = b.split_at(dir, at).expect("strictly inside, within depth");
            let Some(q) = q else {
                return Ok(Split {
                    dir,
                    at,
                    halves,
                    off_q: None,
                    pieces,
                });
            };
            if middle.is_none() {
                middle = Some((at, halves));
            }
            if iso_off(patch, dir, at, along, q, &mut pieces) {
                return Ok(Split {
                    dir,
                    at,
                    halves,
                    off_q: Some(true),
                    pieces,
                });
            }
        }
        if let Some((at, halves)) = middle {
            return Ok(Split {
                dir,
                at,
                halves,
                off_q: Some(false),
                pieces,
            });
        }
    }
    Err(BoxError::Depth)
}

/// The other direction.
fn other(d: Dir) -> Dir {
    match d {
        Dir::U => Dir::V,
        Dir::V => Dir::U,
    }
}

/// Whether the shadow of `patch`'s iso line where the local coordinate in
/// `dir` is `at`, over `along` of the other coordinate, is certified off
/// `q`'s (module docs); `at` 0 or 1 is a tile line. Adds the pieces
/// tested to `pieces`.
pub fn iso_off<S: Scalar>(
    patch: &HomPatch<S>,
    dir: Dir,
    at: Dyadic,
    along: Extent,
    q: &QShadow,
    pieces: &mut u64,
) -> bool {
    if !patch.weights_positive() {
        return false;
    }
    let iso = iso_line(patch, dir, at);
    let like = iso.weights().coefs()[0].clone();
    let axes = [R_HAT, NU].map(|a| [a.x, a.y, a.z].map(|x| S::from_f64(x, &like)));
    let ends = [q.r, q.n].map(|e| e.map(|x| S::from_f64(-x, &like)));
    let (lo, hi, e) = along.nums();
    cover(&iso, 0, 0, (lo, hi, e), &mut |piece| {
        off(piece, &axes, &ends, OFF_DEPTH, pieces)
    })
}

/// The iso line of `patch` where the coordinate in `dir` is `at`, running
/// along the other coordinate, in space.
fn iso_line<S: Scalar>(patch: &HomPatch<S>, dir: Dir, at: Dyadic) -> HomCurve<S, 3> {
    let (m, n) = patch.degrees();
    let like = patch.weights().coefs()[0].clone();
    // The patch whose first row (in `dir`) is the line, and that row's index.
    let (cut, row) = if at == Dyadic::ZERO {
        (patch.clone(), 0)
    } else if at == Dyadic::ONE {
        (patch.clone(), if dir == Dir::U { m } else { n })
    } else {
        let [_, r] = patch.split_at(dir, &S::from_f64(dy(at), &like));
        (r, 0)
    };
    let [x, y, z, w] = cut.tensors();
    let len = if dir == Dir::U { n } else { m };
    let at_k = |t: &crate::bezier::Tensor<S>, k: usize| match dir {
        Dir::U => t.get(row, k).clone(),
        Dir::V => t.get(k, row).clone(),
    };
    HomCurve::new((0..=len).map(|k| ([at_k(x, k), at_k(y, k), at_k(z, k)], at_k(w, k))))
        .expect("a patch's line")
}

/// Visits the pieces of `c` (over the aligned dyadic interval
/// `[k/2^d, (k + 1)/2^d]` of the line) that cover `[lo, hi]/2^e`
/// exactly, each reached by halvings, left to right; `true` when `f` is
/// for every one.
fn cover<S: Scalar>(
    c: &HomCurve<S, 3>,
    k: u64,
    d: u32,
    along: (u64, u64, u32),
    f: &mut impl FnMut(&HomCurve<S, 3>) -> bool,
) -> bool {
    let (lo, hi, e) = along;
    // The node's ends over `2^e` (`d ≤ e`: the extent's ends are on that
    // grid, so the cover never goes deeper).
    let s = e - d;
    let (a, b) = (u128::from(k) << s, u128::from(k + 1) << s);
    let (lo, hi) = (u128::from(lo), u128::from(hi));
    if b <= lo || a >= hi {
        return true;
    }
    if lo <= a && b <= hi {
        return f(c);
    }
    let [l, r] = c.split_half();
    cover(&l, 2 * k, d + 1, along, f) && cover(&r, 2 * k + 1, d + 1, along, f)
}

/// Whether the shadow of `c` misses the query's enclosure: some piece's
/// hull in `axis·x − end` of one sign beyond the enclosure, halving at
/// most `depth` more times.
fn off<S: Scalar>(
    c: &HomCurve<S, 3>,
    axes: &[[S; 3]; 2],
    ends: &[[S; 2]; 2],
    depth: u32,
    pieces: &mut u64,
) -> bool {
    *pieces = pieces.saturating_add(1);
    for (a, [neg_lo, neg_hi]) in axes.iter().zip(ends) {
        if c.compose_linear(a, neg_hi).hull().lo > 0.0 {
            return true;
        }
        if c.compose_linear(a, neg_lo).hull().hi < 0.0 {
            return true;
        }
    }
    if depth == 0 {
        return false;
    }
    let [l, r] = c.split_half();
    off(&l, axes, ends, depth - 1, pieces) && off(&r, axes, ends, depth - 1, pieces)
}

#[cfg(test)]
mod tests;
