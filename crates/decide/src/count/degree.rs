//! Box degrees: the winding of a box's shadow boundary about the query
//! point's shadow `q_xy`, an exact integer, from certified angle
//! increments of its sides.
//!
//! **The shadow plane.** A point `x` has the shadow coordinates `(r̂·x,
//! ν·x)` ([`R_HAT`], [`NU`]), unnormalised; `(r̂, ν, UP)` is right-handed,
//! so a winding counter-clockwise in these coordinates is positive seen
//! from above, and a box of an up-facing face (its UV boundary taken
//! counter-clockwise, the face unflipped) winds `+1` round a point under
//! it. The query point is known by certified enclosures of its two
//! coordinates ([`QShadow`]), as the splits read it.
//!
//! **Angle increments as laps.** The angle of a curve about `q_xy` is
//! measured from a ray from `q_xy` ([`Ray`]): along `(a, b)` in the shadow
//! coordinates, `a, b ∈ {−1, 0, 1}`, the four axis rays `±r̂`, `±ν` and
//! the four diagonals. A curve whose shadow avoids `q_xy` and whose ends
//! are off the closed ray turns by `2π·laps + φ(end) − φ(start)`, `φ` the
//! angle from the ray in `(0, 2π)` and `laps` the signed crossings of the
//! ray, `+1` crossing it counter-clockwise (from the side right of the
//! ray to its left): an integer, exact, and additive where curves join
//! at a point off the ray. Around a closed curve the `φ` terms cancel and
//! the laps are its winding, whatever the ray. That is the [`Turn`] of a
//! curve ([`curve_turn`]): its laps and on which side of the ray's line
//! each end lies, or that it is behind `q_xy` ([`End`]). A tangential
//! touch of the ray counts 0; no angle is ever formed in floating point.
//!
//! **Pieces.** A curve is halved, at most [`OFF_DEPTH`] times, until each
//! piece's coefficient hull is certified on one side of the ray's line
//! (left or right of it: never on the ray) or strictly ahead of or behind
//! `q_xy` along it, each a half-plane certificate against the query's
//! enclosure, which also certifies the piece off `q_xy`; positive weights
//! make the hull's sign the curve's. For an axis ray these are the four
//! tests of [`iso_off`](super::boxes::iso_off) at its depth, so a box side
//! certified off the query by the splits always has pieces here, and one
//! that isn't is reported as such ([`TurnError::Off`]). A run of pieces
//! ahead crosses the ray `(side after − side before)/2` times, the sides
//! read from the neighbouring pieces left or right of the line or from
//! the curve's ends, whose side is the certified sign of their end
//! coefficient ([`TurnError::End`] where it isn't settled: the end lies
//! on the ray's line ahead of `q_xy` as far as the level tells). A piece
//! behind `q_xy` never meets a run (they would share a point both ahead
//! and behind).
//!
//! **Sides of a box** ([`side_turn`], [`Side`]). A side is the tile
//! patch's iso line at the box's extent, cut exactly and covered by the
//! extent's aligned dyadic pieces, as [`split`](super::boxes::split)
//! certifies its lines ([`iso_turn`]); the sides run counter-clockwise in
//! UV: bottom and right along their parameter, top and left against it.
//! Tile lines are sides like any other (fixed: the splits never move
//! them), as are the box sides on a face's outer tile lines.
//!
//! **The degree of a box** ([`box_degree`]): the laps of its four sides
//! about one ray, times the face's inside winding (`−1` on a flipped face,
//! whose boundary runs clockwise in UV), so a box's degree is its part of
//! `D(q, f)` and the degrees of any partition of the face's tiles into
//! boxes sum to the winding of their outer boundary, the face's. A box
//! whose surface hull, cut from the tile's patch at its sides, has a
//! shadow certified off `q_xy` has degree 0 at once. Otherwise the rays
//! are tried in [`Ray::ALL`]'s order until one settles all four sides'
//! ends; the degree is the same from every ray (a winding), only which
//! one settles depends on the level. A side not certified off `q_xy`
//! (through it, or too close for the level) is [`DegreeError::Through`],
//! for the side records of the counting. A corner on a ray's line ahead
//! of `q_xy` stops only that ray (it is behind `q_xy` on the line's other
//! ray), so the four corners stop at most four of the eight; a box whose
//! sides are certified off `q_xy` and still not settled has a corner, or
//! a piece for a diagonal ray, too near `q_xy` for the level,
//! [`DegreeError::Unsettled`]: escalate the level.
//! The degree of a box of a core region is its contribution as is; a
//! strip or corner box's free sides are read with [`iso_turn`] over their
//! part of a side and joined with the trims' crossings.
//!
//! **Sides through the query** ([`box_degree_sided`]). A side not
//! certified off `q_xy` is read from the side record of the line it lies
//! on ([`Sides`]): the tile edge for a side on a tile line, the iso line
//! across the tile for one inside it, each the signed crossings of the
//! line by the query point's own ray ([`Ray::of_query`]), the point
//! moved, over the side's part of the line, a cut inside a crossing's
//! enclosure ordered by the cut's dyadic point record. The box's other
//! sides are then turned against that ray too, an end on its line ahead
//! of the query taking its side from its dyadic point's record (a tile
//! corner, a box corner on a tile edge or inside the tile), so all four
//! sides count the one moved configuration and the degree is the box's
//! winding about the moved point: additive with its neighbours' however
//! deep each is refined (they read the same records along the line), and
//! the winding about `q_xy` wherever that is certified. Every side of a
//! box of the face's tiles has a record key; [`SidedError::Through`] is
//! left for a line past the range of `i64`.
//!
//! Generic over the level, as the boxes are; each call reports the pieces
//! it tested.

use super::boxes::{
    BoxError, BoxKey, Extent, FaceUv, OFF_DEPTH, QShadow, cover, dy, iso_line, sub_patch,
};
use crate::bezier::{Dir, Dyadic, HomCurve, HomPatch, Scalar};
use crate::ids::FaceId;
use crate::num::bounded::Bounded;
use crate::perturb::{NU, R_HAT};
use crate::records::keys::{CornerKey, Operand, Point, SideKey, SideLine};
use crate::records::side::{CornerRec, Over, Sides, Want};

/// A ray from the query's shadow along `(a, b)` in the shadow coordinates
/// `(r̂·x, ν·x)`, `a, b ∈ {−1, 0, 1}` not both 0 (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ray {
    a: i8,
    b: i8,
}

impl Ray {
    /// Along `+r̂`: the ray of a query point on `B`.
    pub const PLUS_R: Ray = Ray { a: 1, b: 0 };
    /// Along `−r̂`: the ray of a query point on `A`.
    pub const MINUS_R: Ray = Ray { a: -1, b: 0 };
    /// Along `+ν`.
    pub const PLUS_NU: Ray = Ray { a: 0, b: 1 };
    /// Along `−ν`.
    pub const MINUS_NU: Ray = Ray { a: 0, b: -1 };

    /// Every ray, in the order [`box_degree`] tries them: the four axes,
    /// then the four diagonals counter-clockwise from `(1, 1)`.
    pub const ALL: [Ray; 8] = [
        Ray::PLUS_R,
        Ray::MINUS_R,
        Ray::PLUS_NU,
        Ray::MINUS_NU,
        Ray { a: 1, b: 1 },
        Ray { a: -1, b: 1 },
        Ray { a: -1, b: -1 },
        Ray { a: 1, b: -1 },
    ];

    /// The ray `(a, b)`, `None` unless both are in `{−1, 0, 1}` and not
    /// both 0.
    pub fn new(a: i8, b: i8) -> Option<Ray> {
        ((-1..=1).contains(&a) && (-1..=1).contains(&b) && (a, b) != (0, 0)).then_some(Ray { a, b })
    }

    /// The ray of a query point on `operand`: from `A` points `−r̂`, from
    /// `B` points `+r̂`, as the ray records count.
    pub fn of_query(operand: Operand) -> Ray {
        match operand {
            Operand::A => Ray::MINUS_R,
            Operand::B => Ray::PLUS_R,
        }
    }

    /// The direction `(a, b)`.
    #[inline]
    pub fn dir(self) -> [i8; 2] {
        [self.a, self.b]
    }

    /// Whether the ray runs along an axis.
    #[inline]
    pub fn is_axis(self) -> bool {
        self.a == 0 || self.b == 0
    }

    /// The direction's coefficients of `(r̂·x, ν·x)` along the ray, and
    /// across it to its left (the direction turned a quarter
    /// counter-clockwise).
    fn axes(self) -> [[i8; 2]; 2] {
        [[self.a, self.b], [-self.b, self.a]]
    }
}

/// Where an end of a curve lies against a ray (module docs): left of the
/// ray's line (`Left`, the side the ray turns to counter-clockwise),
/// right of it, or behind the query's shadow along it; never on the ray.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum End {
    /// Certified left of the ray's line.
    Left,
    /// Certified right of the ray's line.
    Right,
    /// Certified behind the query's shadow along the ray.
    Behind,
}

impl End {
    fn of_side(s: i8) -> End {
        if s > 0 { End::Left } else { End::Right }
    }
}

/// A curve's angle increment about the query's shadow against a ray:
/// `2π·laps + φ(end) − φ(start)` (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Turn {
    /// The signed crossings of the ray, `+1` counter-clockwise.
    pub laps: i64,
    /// Where the curve starts.
    pub start: End,
    /// Where it ends.
    pub end: End,
}

impl Turn {
    /// The curve run backwards: the laps negated, the ends swapped.
    pub fn reversed(self) -> Turn {
        Turn {
            laps: -self.laps,
            start: self.end,
            end: self.start,
        }
    }

    /// This curve followed by `next`, which starts where this one ends
    /// (the caller's to know): the laps added. Laps are bounded by the
    /// pieces tested, so the sum stays far inside `i64`.
    pub fn then(self, next: Turn) -> Turn {
        Turn {
            laps: self.laps + next.laps,
            start: self.start,
            end: next.end,
        }
    }
}

/// Why a curve's turn about a ray isn't certified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TurnError {
    /// A piece isn't certified off the ray's line or the line across it
    /// through the query's shadow within [`OFF_DEPTH`] halvings (for an
    /// axis ray: the curve isn't certified off `q_xy`), or a weight isn't
    /// certified positive. Reported before the other two: every piece is
    /// classified even after an end or the pieces failed to settle.
    Off,
    /// An end lies on the ray's line ahead of the query's shadow, as far
    /// as the level tells: another ray, or a higher level.
    End,
    /// The pieces' certificates contradict each other (a piece ahead next
    /// to one behind): not certified at this level.
    Unsettled,
}

/// A piece of a curve against a ray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Certified left (`+1`) or right (`−1`) of the ray's line.
    Side(i8),
    /// Certified ahead of the query's shadow along the ray.
    Ahead,
    /// Certified behind it.
    Behind,
}

/// A classified piece with the certified sides of its two ends against
/// the ray's line, where settled.
#[derive(Clone, Copy, Debug)]
struct Leaf {
    class: Class,
    ends: [Option<i8>; 2],
}

/// A ray at a level: the 3D axes along and across it and the negated ends
/// of the query's enclosures in each.
struct Frame<S> {
    /// Along the ray, then across it to the left.
    axes: [[S; 3]; 2],
    /// Per axis, `[−lo, −hi]` of the query's enclosure there.
    neg_q: [[S; 2]; 2],
}

impl<S: Scalar> Frame<S> {
    fn new(q: &QShadow, ray: Ray, like: &S) -> Frame<S> {
        let k = ray.axes();
        let axes = k.map(|[a, b]| {
            let (a, b) = (f64::from(a), f64::from(b));
            // Small integers: exact.
            [
                a * R_HAT.x + b * NU.x,
                a * R_HAT.y + b * NU.y,
                a * R_HAT.z + b * NU.z,
            ]
            .map(|x| S::from_f64(x, like))
        });
        let neg_q = k.map(|c| combine(c, q).map(|x| S::from_f64(-x, like)));
        Frame { axes, neg_q }
    }

    /// The piece's class, `None` where no certificate holds at this size.
    fn classify(&self, c: &HomCurve<S, 3>) -> Option<Leaf> {
        let [along, across] = &self.axes;
        let [[a_lo, a_hi], [l_lo, l_hi]] = &self.neg_q;
        let left = c.compose_linear(across, l_hi);
        let right = c.compose_linear(across, l_lo);
        let side_at = |i: usize| {
            if left.coefs()[i].enclose()[0] > 0.0 {
                Some(1)
            } else if right.coefs()[i].enclose()[1] < 0.0 {
                Some(-1)
            } else {
                None
            }
        };
        let last = left.coefs().len() - 1;
        let ends = [side_at(0), side_at(last)];
        let class = if left.hull().lo > 0.0 {
            Class::Side(1)
        } else if right.hull().hi < 0.0 {
            Class::Side(-1)
        } else if c.compose_linear(along, a_hi).hull().lo > 0.0 {
            Class::Ahead
        } else if c.compose_linear(along, a_lo).hull().hi < 0.0 {
            Class::Behind
        } else {
            return None;
        };
        Some(Leaf { class, ends })
    }

    /// Classifies `c`'s pieces in order into `tally`, halving at most
    /// `depth` more times.
    fn walk(
        &self,
        c: &HomCurve<S, 3>,
        depth: u32,
        pieces: &mut u64,
        tally: &mut Tally,
    ) -> Result<(), TurnError> {
        *pieces = pieces.saturating_add(1);
        if let Some(leaf) = self.classify(c) {
            tally.push(leaf);
            return Ok(());
        }
        if depth == 0 {
            return Err(TurnError::Off);
        }
        let [l, r] = c.split_half();
        self.walk(&l, depth - 1, pieces, tally)?;
        self.walk(&r, depth - 1, pieces, tally)
    }
}

/// The enclosure of `c₀·(r̂·q) + c₁·(ν·q)`, `c` in `{−1, 0, 1}`, from the
/// query's enclosures: one term as it is (negated exactly), two summed
/// and rounded outwards.
fn combine(c: [i8; 2], q: &QShadow) -> [f64; 2] {
    let term = |k: i8, [lo, hi]: [f64; 2]| if k > 0 { [lo, hi] } else { [-hi, -lo] };
    match c {
        [0, k] => term(k, q.n),
        [k, 0] => term(k, q.r),
        [i, j] => {
            let (x, y) = (term(i, q.r), term(j, q.n));
            let lo = Bounded::exact(x[0]) + Bounded::exact(y[0]);
            let hi = Bounded::exact(x[1]) + Bounded::exact(y[1]);
            [lo.enclose()[0], hi.enclose()[1]]
        }
    }
}

/// The laps of a curve's pieces as they come, in order (module docs).
#[derive(Debug, Default)]
struct Tally {
    laps: i64,
    /// Where the current run of pieces ahead started, if one is open.
    run: Option<i8>,
    /// The certified side at the boundary before the next piece.
    before: Option<i8>,
    start: Option<End>,
    last: Option<Leaf>,
    /// The first failure of the tally: the walk still classifies every
    /// piece, so a curve not certified off the query is
    /// [`TurnError::Off`] whatever else failed first.
    failed: Option<TurnError>,
    /// The sides of the curve's start and end against the ray's line
    /// where the level doesn't certify them: an end on the line ahead of
    /// the query, its side from its record.
    given: [Option<i8>; 2],
}

impl Tally {
    fn push(&mut self, leaf: Leaf) {
        if self.failed.is_none()
            && let Err(e) = self.step(leaf)
        {
            self.failed = Some(e);
        }
    }

    fn step(&mut self, leaf: Leaf) -> Result<(), TurnError> {
        if self.start.is_none() {
            self.start = Some(match leaf.class {
                Class::Side(s) => End::of_side(s),
                Class::Behind => End::Behind,
                Class::Ahead => {
                    let s = leaf.ends[0].or(self.given[0]).ok_or(TurnError::End)?;
                    self.before = Some(s);
                    End::of_side(s)
                }
            });
        }
        match leaf.class {
            Class::Side(s) => {
                if let Some(from) = self.run.take() {
                    // Each run adds -1, 0 or 1; runs are at most the
                    // pieces tested.
                    self.laps += i64::from((s - from) / 2);
                }
                self.before = Some(s);
            }
            Class::Behind => {
                if self.run.is_some() {
                    return Err(TurnError::Unsettled);
                }
                self.before = None;
            }
            Class::Ahead => {
                if self.run.is_none() {
                    self.run = Some(self.before.ok_or(TurnError::Unsettled)?);
                }
                self.before = None;
            }
        }
        self.last = Some(leaf);
        Ok(())
    }

    fn finish(mut self) -> Result<Turn, TurnError> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        let (Some(start), Some(last)) = (self.start, self.last) else {
            return Err(TurnError::Unsettled);
        };
        let end = match last.class {
            Class::Side(s) => End::of_side(s),
            Class::Behind => End::Behind,
            Class::Ahead => End::of_side(last.ends[1].or(self.given[1]).ok_or(TurnError::End)?),
        };
        if let (Some(from), End::Left | End::Right) = (self.run.take(), end) {
            let to = if end == End::Left { 1 } else { -1 };
            self.laps += i64::from((to - from) / 2);
        }
        Ok(Turn {
            laps: self.laps,
            start,
            end,
        })
    }
}

/// The turn of the curve `c` about the query's shadow `q` against `ray`
/// (module docs), its pieces halved at most [`OFF_DEPTH`] times. Adds the
/// pieces tested to `pieces`.
pub fn curve_turn<S: Scalar>(
    c: &HomCurve<S, 3>,
    q: &QShadow,
    ray: Ray,
    pieces: &mut u64,
) -> Result<Turn, TurnError> {
    if !c.weights_positive() {
        return Err(TurnError::Off);
    }
    let frame = Frame::new(q, ray, &c.weights().coefs()[0]);
    let mut tally = Tally::default();
    frame.walk(c, OFF_DEPTH, pieces, &mut tally)?;
    tally.finish()
}

/// The turn of `patch`'s iso line where the local coordinate in `dir` is
/// `at` (0 or 1: a tile line), over `along` of the other coordinate in
/// its increasing direction, about `q` against `ray`: the line cut
/// exactly and covered by `along`'s aligned dyadic pieces, as
/// [`iso_off`](super::boxes::iso_off) certifies it, each piece then
/// halved as [`curve_turn`]'s. Adds the pieces tested to `pieces`.
pub fn iso_turn<S: Scalar>(
    patch: &HomPatch<S>,
    dir: Dir,
    at: Dyadic,
    along: Extent,
    q: &QShadow,
    ray: Ray,
    pieces: &mut u64,
) -> Result<Turn, TurnError> {
    iso_turn_given(patch, dir, at, along, q, ray, [None, None], pieces)
}

/// [`iso_turn`], with the sides against the ray's line (`+1` left, `−1`
/// right) of the line's start and end over `along` where known from
/// their records: used where the level doesn't certify an end's side (an
/// end on the ray's line ahead of the query), so the turn is that of the
/// curve with such an end moved off the line to its record's side.
#[allow(clippy::too_many_arguments)]
pub(crate) fn iso_turn_given<S: Scalar>(
    patch: &HomPatch<S>,
    dir: Dir,
    at: Dyadic,
    along: Extent,
    q: &QShadow,
    ray: Ray,
    given: [Option<i8>; 2],
    pieces: &mut u64,
) -> Result<Turn, TurnError> {
    if !patch.weights_positive() {
        return Err(TurnError::Off);
    }
    pieces_turn(&iso_pieces(patch, dir, at, along), q, ray, given, pieces)
}

/// The aligned dyadic pieces of `patch`'s iso line where the local
/// coordinate in `dir` is `at` that cover `along` exactly ([`cover`]),
/// left to right: cut once and turned against every ray that reads the
/// side ([`pieces_turn`]).
pub(crate) fn iso_pieces<S: Scalar>(
    patch: &HomPatch<S>,
    dir: Dir,
    at: Dyadic,
    along: Extent,
) -> Vec<HomCurve<S, 3>> {
    let iso = iso_line(patch, dir, at);
    let mut out = Vec::new();
    cover(&iso, 0, 0, along.nums(), &mut |piece| {
        out.push(piece.clone());
        true
    });
    out
}

/// [`iso_turn_given`] over the line's pieces from [`iso_pieces`] (of a
/// patch with positive weights): each halved as [`curve_turn`]'s.
pub(crate) fn pieces_turn<S: Scalar>(
    cut: &[HomCurve<S, 3>],
    q: &QShadow,
    ray: Ray,
    given: [Option<i8>; 2],
    pieces: &mut u64,
) -> Result<Turn, TurnError> {
    let Some(first) = cut.first() else {
        return Err(TurnError::Unsettled);
    };
    let frame = Frame::new(q, ray, &first.weights().coefs()[0]);
    let mut tally = Tally {
        given,
        ..Tally::default()
    };
    for piece in cut {
        frame.walk(piece, OFF_DEPTH, pieces, &mut tally)?;
    }
    tally.finish()
}

/// A side of a box, in counter-clockwise order in UV.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// `v = v_lo`, along increasing `u`.
    Bottom,
    /// `u = u_hi`, along increasing `v`.
    Right,
    /// `v = v_hi`, along decreasing `u`.
    Top,
    /// `u = u_lo`, along decreasing `v`.
    Left,
}

impl Side {
    /// The four, counter-clockwise from the bottom.
    pub const CCW: [Side; 4] = [Side::Bottom, Side::Right, Side::Top, Side::Left];

    /// The side's iso line on box `b`: the direction whose local
    /// coordinate is fixed, that coordinate, the extent of the other one,
    /// and whether the side runs against that coordinate.
    pub fn line(self, b: &BoxKey) -> (Dir, Dyadic, Extent, bool) {
        let (u, v) = (b.extent(Dir::U), b.extent(Dir::V));
        match self {
            Side::Bottom => (Dir::V, v.lo(), u, false),
            Side::Right => (Dir::U, u.hi(), v, false),
            Side::Top => (Dir::V, v.hi(), u, true),
            Side::Left => (Dir::U, u.lo(), v, true),
        }
    }

    /// Whether the side lies on a tile line (a fixed side).
    pub fn on_tile_line(self, b: &BoxKey) -> bool {
        let (_, at, _, _) = self.line(b);
        at == Dyadic::ZERO || at == Dyadic::ONE
    }
}

/// Whether the line of axis `k` (0 for `u`) at the local `at` in tile
/// index `i` is the low edge of `face`'s tiles.
pub(crate) fn low_edge(face: &FaceUv<'_>, k: usize, i: i64, at: Dyadic) -> bool {
    i == face.tiles()[k].start && at == Dyadic::ZERO
}

/// The turn of side `side` of box `b`, oriented counter-clockwise in UV,
/// on `patch`, the box's tile's patch at the level worked at
/// ([`iso_turn`]). Adds the pieces tested to `pieces`.
pub fn side_turn<S: Scalar>(
    b: &BoxKey,
    patch: &HomPatch<S>,
    side: Side,
    q: &QShadow,
    ray: Ray,
    pieces: &mut u64,
) -> Result<Turn, TurnError> {
    let (dir, at, along, back) = side.line(b);
    let t = iso_turn(patch, dir, at, along, q, ray, pieces)?;
    Ok(if back { t.reversed() } else { t })
}

/// A box's degree (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Degree {
    /// The winding of the box's shadow boundary about `q_xy`, oriented as
    /// the face's boundary.
    pub degree: i64,
    /// The ray it was counted against; `None` where the box's hull has a
    /// shadow certified off `q_xy`.
    pub ray: Option<Ray>,
    /// The pieces tested.
    pub pieces: u64,
}

/// Why a box's degree isn't certified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegreeError {
    /// The box isn't of this face, or not in its tiles.
    Box(BoxError),
    /// This side's shadow isn't certified off `q_xy` (the first such side
    /// counter-clockwise): its side record decides.
    Through(Side),
    /// No ray settles the sides' ends, or the patch's weights aren't
    /// certified positive, at this level: escalate.
    Unsettled,
}

/// The degree of box `b` of `face` about the query's shadow `q` (module
/// docs), `patch` the box's tile's patch at the level worked at.
pub fn box_degree<S: Scalar>(
    face: &FaceUv<'_>,
    b: &BoxKey,
    patch: &HomPatch<S>,
    q: &QShadow,
) -> Result<Degree, DegreeError> {
    if !face.owns(b) {
        return Err(DegreeError::Box(BoxError::Foreign));
    }
    if !patch.weights_positive() {
        return Err(DegreeError::Unsettled);
    }
    let mut pieces = 0u64;
    if hull_off(&sub_patch(patch, b), q) {
        return Ok(Degree {
            degree: 0,
            ray: None,
            pieces: 1,
        });
    }
    let sign = face.inside_winding();
    // Each side's pieces, cut once for every ray.
    let mut cuts: [Option<Vec<HomCurve<S, 3>>>; 4] = Default::default();
    for (k, ray) in Ray::ALL.into_iter().enumerate() {
        let mut laps = Some(0i64);
        for (cut, side) in cuts.iter_mut().zip(Side::CCW) {
            let (dir, at, along, back) = side.line(b);
            let cut = cut.get_or_insert_with(|| iso_pieces(patch, dir, at, along));
            let turn = pieces_turn(cut, q, ray, [None, None], &mut pieces);
            match turn.map(|t| if back { t.reversed() } else { t }) {
                Ok(t) => laps = laps.map(|l| l + t.laps),
                // An axis ray's pieces are the splits' certificate: a side
                // they don't cover isn't certified off the query.
                Err(TurnError::Off) if k == 0 => return Err(DegreeError::Through(side)),
                Err(_) => {
                    laps = None;
                    // The first ray reads every side, so that a side
                    // through the query is reported whatever comes first.
                    if k > 0 {
                        break;
                    }
                }
            }
        }
        if let Some(l) = laps {
            return Ok(Degree {
                degree: sign * l,
                ray: Some(ray),
                pieces,
            });
        }
    }
    Err(DegreeError::Unsettled)
}

/// Why a side's turn isn't certified with its side records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecTurnError {
    /// This record isn't decided yet.
    Need(Want),
    /// The side isn't certified off `q_xy` and has no record key: its
    /// line is past the range of `i64`.
    Through,
    /// Not settled at this level.
    Unsettled,
}

/// The laps of the side `dir = at` of tile `tile` of face `of`, on
/// `patch` (that tile's at the level worked at), over `along` (local, in
/// the side's own direction), its ends' sides where `given`, about the
/// ray of `point`: [`iso_turn_given`] where certified, an end on the
/// ray's line ahead of the query taking its side from its dyadic point's
/// record; on a side not certified off `q_xy`, the record of the line it
/// lies on (a tile edge, or an iso line inside the tile) over `along`,
/// a cut inside a crossing's enclosure ordered by the cut's record
/// (module docs). The patch's weights are positive.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rec_turn<S: Scalar>(
    patch: &HomPatch<S>,
    of: (Operand, FaceId),
    tile: [i64; 2],
    dir: Dir,
    at: Dyadic,
    along: Extent,
    q: &QShadow,
    point: Point,
    sides: &Sides,
    given: [Option<i8>; 2],
    pieces: &mut u64,
) -> Result<i64, RecTurnError> {
    let ray = Ray::of_query(point.operand);
    let line = SideLine::of(of.0, of.1, tile, dir, at);
    let ends = line.map(|l| [l.point(along.lo()), l.point(along.hi())]);
    let corner = |j: usize| -> Result<CornerRec, RecTurnError> {
        let p = ends.and_then(|e| e[j]).ok_or(RecTurnError::Unsettled)?;
        sides
            .corner(point, p)
            .ok_or(RecTurnError::Need(Want::Corner(CornerKey {
                q: point,
                corner: p,
            })))
    };
    // The line's pieces, cut once for both turns below.
    let cut = patch
        .weights_positive()
        .then(|| iso_pieces(patch, dir, at, along));
    let mut turn = |g: [Option<i8>; 2]| match &cut {
        Some(c) => pieces_turn(c, q, ray, g, pieces),
        None => Err(TurnError::Off),
    };
    let mut t = turn(given);
    if let Err(TurnError::End) = t {
        // Left of `+r̂` is `+ν`, the record's `Pos`; of `−r̂` it is `−ν`.
        let flip: i8 = match point.operand {
            Operand::A => -1,
            Operand::B => 1,
        };
        let mut g = given;
        for (j, slot) in g.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(flip * corner(j)?.side.as_i8());
            }
        }
        if g != given {
            t = turn(g);
        }
    }
    match (t, line) {
        (Ok(t), _) => Ok(t.laps),
        (Err(TurnError::Off), Some(line)) => {
            let rec = sides
                .side(point, line)
                .ok_or(RecTurnError::Need(Want::Side(SideKey { q: point, line })))?;
            let x = [dy(along.lo()), dy(along.hi())];
            let mut cut = [None, None];
            loop {
                match rec.count_over(point.operand, x, cut) {
                    Ok(n) => return Ok(n),
                    Err(Over::Side(j)) => cut[j] = Some(corner(j)?.side),
                    Err(Over::Unordered) => return Err(RecTurnError::Unsettled),
                }
            }
        }
        (Err(TurnError::Off), None) => Err(RecTurnError::Through),
        (Err(_), _) => Err(RecTurnError::Unsettled),
    }
}

/// Why a box's degree isn't certified with its side records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidedError {
    /// The box isn't of this face, or not in its tiles, or the query
    /// point is of the face's operand.
    Box(BoxError),
    /// The degree reads this record, not yet decided: fill it and ask
    /// again.
    Need(Want),
    /// This side isn't certified off `q_xy` and no record counts it (its
    /// line past the range of `i64`): never from tiles a face can have.
    Through(Side),
    /// The sides don't settle against the query point's ray at this
    /// level (an end not certified, a tangency's crossing over a box
    /// corner, the patch's weights): escalate.
    Unsettled,
}

/// The degree of box `b` of `face` about point `point`, whose shadow is
/// `q` (module docs, "Sides through the query"): [`box_degree`] where it
/// certifies the degree, else the four sides against `point`'s ray, a
/// side not certified off `q_xy` read from `sides`.
pub fn box_degree_sided<S: Scalar>(
    face: &FaceUv<'_>,
    b: &BoxKey,
    patch: &HomPatch<S>,
    q: &QShadow,
    point: Point,
    sides: &Sides,
) -> Result<Degree, SidedError> {
    if point.operand == b.operand() {
        return Err(SidedError::Box(BoxError::Foreign));
    }
    // The plain degree's preconditions first: the box is the face's.
    match box_degree(face, b, patch, q) {
        Ok(d) => return Ok(d),
        Err(DegreeError::Box(e)) => return Err(SidedError::Box(e)),
        Err(DegreeError::Through(_) | DegreeError::Unsettled) => (),
    }
    // Every side reads `TurnError::Off` on such a patch: no record may
    // stand in for them.
    if !patch.weights_positive() {
        return Err(SidedError::Unsettled);
    }
    let ray = Ray::of_query(point.operand);
    let mut pieces = 0u64;
    let mut laps = 0i64;
    for side in Side::CCW {
        let (dir, at, along, back) = side.line(b);
        let n = rec_turn(
            patch,
            (b.operand(), b.face()),
            b.tile(),
            dir,
            at,
            along,
            q,
            point,
            sides,
            [None, None],
            &mut pieces,
        )
        .map_err(|e| match e {
            RecTurnError::Need(k) => SidedError::Need(k),
            RecTurnError::Through => SidedError::Through(side),
            RecTurnError::Unsettled => SidedError::Unsettled,
        })?;
        laps += if back { -n } else { n };
    }
    Ok(Degree {
        degree: face.inside_winding() * laps,
        ray: Some(ray),
        pieces,
    })
}

/// Whether the shadow of the patch's coefficient hull is certified off
/// the query's: beyond its enclosure in `r̂·x` or `ν·x`.
pub(crate) fn hull_off<S: Scalar>(p: &HomPatch<S>, q: &QShadow) -> bool {
    if !p.weights_positive() {
        return false;
    }
    let like = p.weights().coefs()[0].clone();
    [(R_HAT, q.r), (NU, q.n)].into_iter().any(|(a, [lo, hi])| {
        let a = [a.x, a.y, a.z].map(|x| S::from_f64(x, &like));
        p.compose_linear(&a, &S::from_f64(-hi, &like)).hull().lo > 0.0
            || p.compose_linear(&a, &S::from_f64(-lo, &like)).hull().hi < 0.0
    })
}

#[cfg(test)]
mod tests;
