//! Turning meridians about an axis: the stations of a surface of
//! revolution, fitted bands, and the caps at poles and apexes.

use std::f64::consts::{FRAC_PI_2, TAU};

use glam::{DVec2, DVec3};

use super::fit::{deviation, fitted_strip};
use crate::budget::Work;
use crate::mesh::{Form, edge_neighbours_apart};
use crate::patch::{Conic3, Patch, PatchError};
use crate::{Budget, KernelError, Tolerance, trig};

/// Turns about an axis in equal pieces: the stations of a surface of
/// revolution. Station `k` is `k` pieces round from station 0, turning
/// the way the right hand does about `axis` (from `x` towards `axis ×
/// x`); a full turn's last station is station 0 again.
///
/// Angles go through [`trig`], with exact cosines and sines at whole
/// quarter turns (a part turn's within a few roundings of one: its sweep
/// was meant to be); station 0 is the point itself, to the bit.
#[derive(Debug, Clone, PartialEq)]
pub struct Lathe {
    origin: DVec3,
    axis: DVec3,
    sweep: Option<f64>,
    pieces: usize,
    /// The cosine and sine of each station's angle, `0..=pieces`.
    turns: Vec<DVec2>,
}

impl Lathe {
    /// The most pieces a lathe may have.
    pub const MAX_PIECES: usize = 1 << 12;

    /// The lathe about the line through `origin` along `axis`, over
    /// `sweep` radians (`None` for a full turn), in `pieces` equal pieces
    /// of at most a quarter turn each (up to `1e-12` relative, room for
    /// rounding of equal parts).
    ///
    /// Refuses a zero or non-finite axis or origin, a sweep outside `(0,
    /// 2π)`, or pieces too few, too many (past [`Self::MAX_PIECES`]) or
    /// too wide, with [`PatchError::Parameter`] (or
    /// [`PatchError::Coordinate`]).
    pub fn new(
        origin: DVec3,
        axis: DVec3,
        sweep: Option<f64>,
        pieces: usize,
    ) -> Result<Lathe, PatchError> {
        crate::in_range(origin)?;
        let axis = axis
            .try_normalize()
            .ok_or(PatchError::Parameter(axis.length()))?;
        let total = match sweep {
            None => TAU,
            Some(s) if s > 0.0 && s < TAU => s,
            Some(s) => return Err(PatchError::Parameter(s)),
        };
        if pieces == 0 || pieces > Self::MAX_PIECES {
            return Err(PatchError::Parameter(pieces as f64));
        }
        let piece = total / pieces as f64;
        if piece > FRAC_PI_2 * (1.0 + 1e-12) {
            return Err(PatchError::Parameter(piece));
        }
        let turns = (0..=pieces)
            .map(|k| {
                let angle = if k == pieces {
                    total
                } else {
                    total * k as f64 / pieces as f64
                };
                // Whole quarter turns: for a full turn by counting (`TAU ·
                // k / pieces` rounds off one for, say, 11 of 44 pieces);
                // for a part, within the few roundings of the angle, so
                // a sweep given as 270° has its quarter stations too.
                let quarters = match sweep {
                    None => (4 * k).is_multiple_of(pieces).then_some(4 * k / pieces),
                    Some(_) => {
                        let q = angle / FRAC_PI_2;
                        let whole = q.round();
                        // Never a whole turn: a part stays one.
                        ((q - whole).abs() <= 8.0 * f64::EPSILON * whole.max(1.0) && whole < 4.0)
                            .then_some(whole as usize)
                    }
                };
                match quarters.map(|q| q % 4) {
                    Some(0) => DVec2::new(1.0, 0.0),
                    Some(1) => DVec2::new(0.0, 1.0),
                    Some(2) => DVec2::new(-1.0, 0.0),
                    Some(_) => DVec2::new(0.0, -1.0),
                    None => trig::unit(angle),
                }
            })
            .collect();
        Ok(Lathe {
            origin,
            axis,
            sweep,
            pieces,
            turns,
        })
    }

    /// The same lathe in twice the pieces (every station kept, a new one
    /// between each two).
    pub fn halved(&self) -> Result<Lathe, PatchError> {
        let pieces = self
            .pieces
            .checked_mul(2)
            .ok_or(PatchError::Parameter(f64::INFINITY))?;
        Lathe::new(self.origin, self.axis, self.sweep, pieces)
    }

    /// How many pieces it turns in.
    pub fn pieces(&self) -> usize {
        self.pieces
    }

    /// Whether it turns all the way round.
    pub fn is_full(&self) -> bool {
        self.sweep.is_none()
    }

    /// The unit axis.
    pub fn axis(&self) -> DVec3 {
        self.axis
    }

    /// A point on the axis.
    pub fn origin(&self) -> DVec3 {
        self.origin
    }

    /// How many distinct stations a point off the axis has: the pieces
    /// for a full turn, one more for a part.
    pub fn stations(&self) -> usize {
        self.pieces + usize::from(self.sweep.is_some())
    }

    /// `p` turned to station `k` (`0..=pieces`, panicking past it).
    /// Station 0, a full turn's last, and a point on the axis are `p`
    /// itself, to the bit.
    pub fn turned(&self, p: DVec3, k: usize) -> DVec3 {
        let turn = self.turns[k];
        if (turn.x == 1.0 && turn.y == 0.0) || k == 0 {
            return p;
        }
        let v = p - self.origin;
        let along = self.axis * v.dot(self.axis);
        let across = v - along;
        if across == DVec3::ZERO {
            return p;
        }
        self.origin + along + across * turn.x + self.axis.cross(across) * turn.y
    }

    /// Whether `p` lies on the axis, to a few roundings of its
    /// coordinates (a point put there by turning the axis's frame).
    pub fn on_axis(&self, p: DVec3) -> bool {
        let v = p - self.origin;
        let across = v - self.axis * v.dot(self.axis);
        let scale = p.abs().max(self.origin.abs()).max_element();
        across.length() <= 64.0 * f64::EPSILON * scale
    }

    /// The ring of `p`: `p` at each of its [`Self::stations`].
    pub fn ring(&self, p: DVec3) -> Vec<DVec3> {
        (0..self.stations()).map(|k| self.turned(p, k)).collect()
    }

    /// The parallel of `p` from station `k` to station `k + 1` (`k` below
    /// the pieces): an exact
    /// arc about the axis, from its ends (see
    /// [`Conic::arc_between`](crate::patch::Conic::arc_between)).
    /// Refuses `p` on the axis with [`PatchError::Parameter`].
    pub fn parallel(&self, p: DVec3, k: usize) -> Result<Conic3, PatchError> {
        let v = p - self.origin;
        let foot = self.origin + self.axis * v.dot(self.axis);
        let radius = (p - foot).length();
        if radius.is_nan() || radius <= 0.0 {
            return Err(PatchError::Parameter(radius));
        }
        Conic3::arc_between(foot, radius, self.turned(p, k), self.turned(p, k + 1))
    }

    /// `meridian` (drawn at station 0) turned to station `k`: its ends
    /// and control point turned, its weight kept. So strips either side
    /// of a station share it to the bit.
    pub fn meridian(&self, meridian: &Conic3, k: usize) -> Result<Conic3, PatchError> {
        Conic3::new(
            self.turned(meridian.p0, k),
            self.turned(meridian.c, k),
            meridian.w,
            self.turned(meridian.p1, k),
        )
    }

    /// How far `p` is from the axis.
    fn radius(&self, p: DVec3) -> f64 {
        let v = p - self.origin;
        (v - self.axis * v.dot(self.axis)).length()
    }

    /// The height of `p` along the axis.
    fn height(&self, p: DVec3) -> f64 {
        (p - self.origin).dot(self.axis)
    }

    /// The height along the axis of `meridian`'s point at `t`.
    fn height_at(&self, meridian: &Conic3, t: f64) -> f64 {
        let [h0, hc, h1] = meridian.hull().map(|p| self.height(p));
        let (s, w) = (1.0 - t, meridian.w);
        let (b0, b1, b2) = (s * s, 2.0 * s * t, t * t);
        (h0 * b0 + hc * w * b1 + h1 * b2) / (b0 + w * b1 + b2)
    }

    /// Where `meridian`'s height along the axis turns, its tangent square
    /// to the axis: the roots in `(0, 1)` (kept off the ends by `1e-6`;
    /// nearer, the ring is at the turn) of the derivative's numerator,
    /// whose Bernstein coefficients for a conic of weight `w` and
    /// heights `h0`, `hc`, `h1` are `w·(hc − h0)`, `(h1 − h0)/2` and
    /// `w·(h1 − hc)`. In order.
    fn turns(&self, meridian: &Conic3) -> Vec<f64> {
        let [h0, hc, h1] = meridian.hull().map(|p| self.height(p));
        let w = meridian.w;
        let (d0, d1, d2) = (w * (hc - h0), (h1 - h0) * 0.5, w * (h1 - hc));
        // d0·(1 − t)² + 2·d1·t(1 − t) + d2·t² = a·t² + b·t + c.
        let (a, b, c) = (d0 - 2.0 * d1 + d2, 2.0 * (d1 - d0), d0);
        let scale = d0.abs().max(d1.abs()).max(d2.abs());
        let mut roots = Vec::new();
        if scale.is_nan() || scale <= 0.0 {
            return roots;
        }
        if a.abs() <= 1e-12 * scale {
            if b != 0.0 {
                roots.push(-c / b);
            }
        } else {
            let disc = b * b - 4.0 * a * c;
            if disc >= 0.0 {
                // The stable pair: no cancellation in either.
                let q = -0.5 * (b + b.signum() * disc.sqrt());
                roots.push(q / a);
                if q != 0.0 {
                    roots.push(c / q);
                }
            }
        }
        roots.retain(|t| (1e-6..=1.0 - 1e-6).contains(t));
        roots.sort_by(f64::total_cmp);
        roots.dedup();
        roots
    }

    /// [`Self::turns`] of a band's piece, leaving out those within
    /// [`TURN_NEAR_END`] of its ends: the ring there is near enough the
    /// turn to be parted by the cylinder over it (see `check`'s edge
    /// rule), and the piece over the turn that balancing them would cut
    /// is a sliver a band can't fit.
    fn band_turns(&self, piece: &Conic3) -> Vec<f64> {
        let mut turns = self.turns(piece);
        turns.retain(|t| (TURN_NEAR_END..=1.0 - TURN_NEAR_END).contains(t));
        turns
    }

    /// The parameter in `[lo, hi]`, over which `meridian`'s height is
    /// monotonic, where its height is `level`, by bisection to the bit.
    fn level(&self, meridian: &Conic3, lo: f64, hi: f64, level: f64) -> f64 {
        let rising = self.height_at(meridian, hi) > self.height_at(meridian, lo);
        let (mut lo, mut hi) = (lo, hi);
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if mid <= lo || mid >= hi {
                break;
            }
            if (self.height_at(meridian, mid) < level) == rising {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// A piece whose height along the axis turns inside it, as pieces
    /// that either don't turn or turn once between ends at one height
    /// (within a quarter of `margin`), or `None` if it is one already.
    ///
    /// A ring's parallel is the shared edge of the strips either side of
    /// it, and the hull rule between them takes the plane through its
    /// control points, square to the axis: one strip must clear it, the
    /// other not cross it by more than the margin. Where the meridian's
    /// tangent is square to the axis (a torus's top and bottom) the
    /// surface touches that plane along the parallel, and only the
    /// cylinder over the ring parts the strips (`check`'s edge rule tries
    /// it after the plane). Bands keep their rings off turns all the same,
    /// as it is cheaper: the strip over the turn ends at one height on
    /// both sides, so it stays on its side of both its rings' planes, and
    /// its neighbours, falling away from the turn, clear them.
    ///
    /// A turn within [`TURN_NEAR_END`] of the piece's end (in its
    /// parameter) is left to the ring there ([`Self::band_turns`]), which
    /// the cylinder parts: one strip leaves it inwards, the other
    /// outwards.
    fn balance(&self, piece: &Conic3, margin: f64) -> Result<Option<Vec<Conic3>>, PatchError> {
        let turns = self.band_turns(piece);
        match turns[..] {
            [] => Ok(None),
            [turn] => {
                let (h0, h1) = (self.height(piece.p0), self.height(piece.p1));
                if (h0 - h1).abs() <= 0.25 * margin {
                    return Ok(None);
                }
                let top = self.height_at(piece, turn);
                // The end nearer the turn's height sets the level; the
                // other side is cut where it reaches it.
                Ok(Some(if (h0 - top).abs() < (h1 - top).abs() {
                    let t = self.level(piece, turn, 1.0, h0);
                    vec![piece.piece(0.0, t)?, piece.piece(t, 1.0)?]
                } else {
                    let t = self.level(piece, 0.0, turn, h1);
                    vec![piece.piece(0.0, t)?, piece.piece(t, 1.0)?]
                }))
            }
            [first, second, ..] => {
                let t = 0.5 * (first + second);
                Ok(Some(vec![piece.piece(0.0, t)?, piece.piece(t, 1.0)?]))
            }
        }
    }

    /// A balanced piece (see [`Self::balance`]) halved along the
    /// meridian: in halves, or if it turns, in three with the middle one,
    /// about half as long, over the turn and ending at one height.
    fn halve(&self, piece: &Conic3) -> Result<Vec<Conic3>, PatchError> {
        match self.band_turns(piece)[..] {
            [turn] => {
                let t0 = 0.5 * turn;
                let t1 = self.level(piece, turn, 1.0, self.height_at(piece, t0));
                Ok(vec![
                    piece.piece(0.0, t0)?,
                    piece.piece(t0, t1)?,
                    piece.piece(t1, 1.0)?,
                ])
            }
            _ => Ok(piece.split_half()?.to_vec()),
        }
    }

    /// The strip of `meridian` (from `a0` to `b0` at station 0) from
    /// station `k` to `k + 1`, its diagonal fitted to `form` (see
    /// [`fitted_strip`]), if it is sound, with its error: `None` if no
    /// diagonal could be fitted, a patch folds, or the two come within
    /// `margin` of each other off their diagonal, all of which halving
    /// may cure.
    ///
    /// "Within `margin`" is the plane rule ([`edge_neighbours_apart`])
    /// alone, not `check`'s cylinder after it: the plane's failure on the
    /// fitted diagonal is what tells a band a strip is too coarse. With
    /// the cylinder too, bands chose coarser strips whose patches then
    /// failed the vertex rule.
    fn strip(
        &self,
        meridian: &Conic3,
        k: usize,
        form: &Form,
        margin: f64,
    ) -> Result<Option<([Patch; 2], f64)>, PatchError> {
        let bottom = self.parallel(meridian.p0, k)?;
        let top = self.parallel(meridian.p1, k)?;
        let left = self.meridian(meridian, k)?;
        let right = self.meridian(meridian, k + 1)?;
        let Ok(fitted) = fitted_strip(&bottom, &top, &left, &right, form) else {
            return Ok(None);
        };
        let [first, second] = &fitted.patches;
        let sound = first.fold_direction().is_some()
            && second.fold_direction().is_some()
            && edge_neighbours_apart(first, 2, second, 0, margin);
        Ok(sound.then_some((fitted.patches, fitted.error)))
    }

    /// The error of [`Self::strip`], infinite for one that isn't sound.
    fn strip_error(
        &self,
        meridian: &Conic3,
        k: usize,
        form: &Form,
        margin: f64,
    ) -> Result<f64, PatchError> {
        Ok(self
            .strip(meridian, k, form, margin)?
            .map_or(f64::INFINITY, |s| s.1))
    }
}

/// Work units per strip fitted and measured (about a hundred
/// microseconds: [`Budget`]'s units are about half a microsecond).
const STRIP_UNITS: usize = 256;
/// Work units per cap triangle measured.
const CAP_UNITS: usize = 96;
/// How many times a band's meridian may be halved.
const MAX_BAND_DEPTH: u32 = 16;

/// How near a band's piece's end, in its parameter, a turn of its height
/// is left to the ring there rather than balanced ([`Lathe::balance`]).
/// Balancing a turn `t` from the end cuts a piece about `2t` long over
/// it, whose strips are slivers: a quarter arc turning `1e-5` rad past its
/// end gave pieces of `1e-5` of it, which the band halved round the axis
/// until `TooComplex` (or whose solid then failed `check`), from `3e-6`
/// to `3e-4` rad at fits `1e-2` to `1e-4`. Left to the ring, every such
/// profile measured passes; `1e-3` did too, `1e-4` still failed.
const TURN_NEAR_END: f64 = 1e-2;
/// How many times a cap's meridian may be halved.
const MAX_CAP_HALVINGS: u32 = 40;
/// How far under half the fit tolerance a strip's or cap's measured
/// error must be, as a part of it: [`deviation`] is the largest error its
/// climbs found, not a certified bound, and this keeps what they could
/// leave short of a maximum (on the tests' tori, nothing measurable)
/// inside half the fit tolerance.
const MEASURE_MARGIN: f64 = 1.0 / 64.0;
/// The most a cap's rest grows from one ring to the next, in its
/// meridian's parameter.
const REST_RATIO: f64 = 16.0;

/// What a fitted strip's or cap's measured error must not pass: half the
/// fit tolerance, less [`MEASURE_MARGIN`] of it.
fn limit(tol: &Tolerance) -> f64 {
    tol.fit() * 0.5 * (1.0 - MEASURE_MARGIN)
}

/// A band of a surface of revolution fitted by [`fitted_band`].
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    /// The meridian's pieces at station 0, in its order: the band's rings
    /// are at their ends.
    pub pieces: Vec<Conic3>,
    /// The strips, piece by piece and within a piece station by station
    /// (`strips[j · pieces + k]` for piece `j` from station `k`), laid out
    /// as the strips of the [module](super) (`a` on the piece's start,
    /// `b` on its end).
    pub strips: Vec<[Patch; 2]>,
    /// The farthest a patch is from the form ([`deviation`]).
    pub error: f64,
}

/// The band `meridian` (at station 0) sweeps on the lathe, in fitted
/// strips (see [`fitted_strip`]) within half `tol`'s fit tolerance of
/// `form` (measured by [`deviation`], with a 64th of that to spare): a surface of revolution with exact parallels and meridians
/// whose strips aren't exact (a torus, another conic about the axis, or
/// any of them past where its exact strips are made).
///
/// Strips too far off are halved in either direction, whichever the
/// strip at station 0 says gains more: the meridian (each half on its
/// own, so only where needed), or round the axis. Halving round the axis
/// changes every face of the lathe, so it is the caller's: `Ok(None)`
/// asks for the band again on [`Lathe::halved`]. Every strip is measured
/// ([`deviation`]), and passes the fold check and the hull rule between
/// its two patches, or is halved.
///
/// [`KernelError::TooComplex`] past `budget`, or for a meridian halved
/// more than 16 times; a meridian with an end on the axis (a cap's, see
/// [`pole_cap`]) is a [`KernelError::Patch`].
pub fn fitted_band(
    lathe: &Lathe,
    meridian: &Conic3,
    form: &Form,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Option<Band>, KernelError> {
    fitted_band_with(lathe, meridian, form, tol, &mut Work::new(budget))
}

/// [`fitted_band`] charging `work`.
pub(crate) fn fitted_band_with(
    lathe: &Lathe,
    meridian: &Conic3,
    form: &Form,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Option<Band>, KernelError> {
    let limit = limit(tol);
    let margin = tol.resolution();
    let mut band = Band {
        pieces: Vec::new(),
        strips: Vec::new(),
        error: 0.0,
    };
    // Pieces still to fit, the next one last, with their depth.
    let mut todo = vec![(*meridian, 0)];
    let stations: Vec<usize> = (0..lathe.pieces()).collect();
    while let Some((piece, depth)) = todo.pop() {
        if depth > MAX_BAND_DEPTH {
            return Err(KernelError::TooComplex);
        }
        // A piece whose height turns must end at one height first.
        if let Some(parts) = lathe.balance(&piece, margin)? {
            for part in parts.into_iter().rev() {
                todo.push((part, depth + 1));
            }
            continue;
        }
        // Station 0 first: the others are the same strip turned, so a
        // piece that fails there is halved without them.
        work.spend(STRIP_UNITS)?;
        if lathe.strip_error(&piece, 0, form, margin)? <= limit {
            work.spend(STRIP_UNITS.saturating_mul(lathe.pieces()))?;
            let strips = crate::par::par_map(&stations, |&k| lathe.strip(&piece, k, form, margin));
            let strips = strips
                .into_iter()
                .collect::<Result<Option<Vec<_>>, _>>()?
                .filter(|strips| strips.iter().all(|s| s.1 <= limit));
            // Every strip is measured: one past the limit by its
            // rounding is halved as the first would have been.
            if let Some(strips) = strips {
                for (patches, error) in strips {
                    band.error = band.error.max(error);
                    band.strips.push(patches);
                }
                band.pieces.push(piece);
                continue;
            }
        }
        // Which way gains more, from station 0.
        let parts = lathe.halve(&piece)?;
        work.spend(STRIP_UNITS.saturating_mul(parts.len() + 1))?;
        let mut along: f64 = 0.0;
        for part in &parts {
            along = along.max(lathe.strip_error(part, 0, form, margin)?);
        }
        if let Ok(finer) = lathe.halved() {
            let round = finer.strip_error(&piece, 0, form, margin)?;
            // Neither sound yet: the longer way first.
            let longer_round = || {
                let across = (lathe.turned(piece.p0, 1) - piece.p0)
                    .length()
                    .max((lathe.turned(piece.p1, 1) - piece.p1).length());
                across > (piece.p1 - piece.p0).length()
            };
            if round < along || (round.is_infinite() && along.is_infinite() && longer_round()) {
                return Ok(None);
            }
        }
        for part in parts.into_iter().rev() {
            todo.push((part, depth + 1));
        }
    }
    Ok(Some(band))
}

/// Which end of a meridian is on the axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pole {
    Start,
    End,
}

/// The cap round a pole (or apex) of a surface of revolution, from
/// [`pole_cap`].
#[derive(Debug, Clone, PartialEq)]
pub struct Cap {
    /// The piece of the meridian the cap is made of, in the meridian's
    /// direction, at station 0.
    pub meridian: Conic3,
    /// The rest of the meridian, in its direction, in pieces whose rings
    /// lie at their ends: each at most 16 times as long (in the
    /// meridian's parameter) as the one nearer the pole, so the strips
    /// the caller makes of them don't thin out, up to half way to where
    /// the meridian's height first turns, and then one piece to its end
    /// (over the turn: a band's to balance, see [`fitted_band`]). Empty
    /// if the cap is the whole meridian.
    pub rest: Vec<Conic3>,
    /// The cap's triangles, one per piece of the lathe: the strips of the
    /// [module](super) with the pole's side collapsed, `(pole, b1, b0)`
    /// for a pole at the start, `(a0, a1, pole)` at the end, so they face
    /// the way the strips beside them do.
    pub patches: Vec<Patch>,
    /// The farthest a triangle is from the form ([`deviation`]).
    pub error: f64,
}

/// The cap round the pole of `meridian` (drawn at station 0, the end
/// `pole` says on the axis): triangles of two meridians from the pole and
/// the parallel between them. No triangle with two meridians meeting on
/// the axis lies on a surface of revolution (their planes meet in the
/// axis, which holds no third point of the surface; on a cone the exact
/// rulings would have their control points on the apex, a corner the fold
/// check can't pass), so the cap is fitted: its meridians are `meridian`'s
/// own piece turned (exact on spheres; on a cone pass a straight
/// meridian, `Conic::line`, whose rulings are then the linear ones), and
/// the piece is halved toward the pole until each triangle is within half
/// `tol`'s fit tolerance of `form` (as a band's strips) and passes the
/// fold check, and the edge rule's plane with its neighbour (alone, as
/// bands judge their strips: see `Lathe::strip`). Measured errors: a
/// sphere's cap of angle `δ` in sectors of `φ` (radians) about
/// `R·δ²·φ²/64` off, so each halving takes a quarter; a cone's in
/// proportion to its length, so a half.
///
/// The cap starts no further than half way to where the meridian's
/// height first turns (its tangent square to the axis, as an apple's
/// below its dimple): a cap over the turn would lie on both sides of
/// its rim's plane with the strips beyond it on one, which the hull rule
/// between them refuses.
///
/// [`KernelError::TooComplex`] past `budget`, after 40 halvings, or once
/// the rim comes within the resolution of the axis (no triangles that
/// small pass the hull rules); the pole off the axis or a meridian that
/// can't be split is a [`KernelError::Patch`].
pub fn pole_cap(
    lathe: &Lathe,
    meridian: &Conic3,
    pole: Pole,
    form: &Form,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Cap, KernelError> {
    pole_cap_with(lathe, meridian, pole, form, tol, &mut Work::new(budget))
}

/// [`pole_cap`] charging `work`.
pub(crate) fn pole_cap_with(
    lathe: &Lathe,
    meridian: &Conic3,
    pole: Pole,
    form: &Form,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Cap, KernelError> {
    meridian.check()?;
    let limit = limit(tol);
    let margin = tol.resolution();
    let tip = match pole {
        Pole::Start => meridian.p0,
        Pole::End => meridian.p1,
    };
    if !lathe.on_axis(tip) {
        return Err(PatchError::Parameter(0.0).into());
    }
    // `t` from the pole, as the meridian's own parameter.
    let at = |t: f64| match pole {
        Pole::Start => t,
        Pole::End => 1.0 - t,
    };
    let stations: Vec<usize> = (0..lathe.pieces()).collect();
    // A cap whose height turns between the pole and its rim lies on
    // both sides of its rim's plane, and the strips beyond it on one, so
    // the hull rule between them can't pass: the cap stops half way to
    // the first turn, and the rest, over it, is a band's to balance.
    // The rest's rings stay at or below that size too (a ring at the turn
    // would touch its plane), and the piece beyond it goes up to the
    // meridian's end.
    let reach = match (pole, &lathe.turns(meridian)[..]) {
        (_, []) => 1.0,
        (Pole::Start, [first, ..]) => 0.5 * first,
        (Pole::End, [.., last]) => 0.5 * (1.0 - last),
    };
    let mut size = reach;
    for _ in 0..=MAX_CAP_HALVINGS {
        let cap = if size == 1.0 {
            *meridian
        } else {
            meridian.piece(at(0.0), at(size))?
        };
        // A rim within the resolution of the axis makes triangles no
        // hull rule can pass (and its parallel no arc): halving further
        // can't help.
        let rim = match pole {
            Pole::Start => cap.p1,
            Pole::End => cap.p0,
        };
        if lathe.radius(rim) <= margin {
            return Err(KernelError::TooComplex);
        }
        work.spend(CAP_UNITS.saturating_mul(lathe.pieces()))?;
        let triangles = crate::par::par_map(&stations, |&k| cap_triangle(lathe, &cap, pole, k));
        let patches = triangles.into_iter().collect::<Result<Vec<_>, _>>()?;
        let errors = crate::par::par_map(&patches, |p| {
            if p.fold_direction().is_some() {
                deviation(p, form)
            } else {
                f64::INFINITY
            }
        });
        let mut error = errors.iter().copied().fold(0.0, f64::max);
        // Neighbours share the meridian between them: the first's edge 0
        // (pole to its far rim corner) and the second's edge 2 for a pole
        // at the start, the first's edge 1 and the second's edge 2 at
        // the end. By the plane rule alone, as bands judge their strips
        // (see `Lathe::strip`).
        if patches.len() > 1 {
            let first = match pole {
                Pole::Start => 0,
                Pole::End => 1,
            };
            if !edge_neighbours_apart(&patches[0], first, &patches[1], 2, margin) {
                error = f64::INFINITY;
            }
        }
        if error <= limit {
            let mut rest = Vec::new();
            let mut from = size;
            while from < 1.0 {
                let to = if from < reach {
                    (from * REST_RATIO).min(reach)
                } else {
                    1.0
                };
                rest.push(meridian.piece(at(from), at(to))?);
                from = to;
            }
            if pole == Pole::End {
                rest.reverse();
            }
            return Ok(Cap {
                meridian: cap,
                rest,
                patches,
                error,
            });
        }
        size *= 0.5;
    }
    Err(KernelError::TooComplex)
}

/// The cap's triangle from station `k` to `k + 1` (see [`Cap`]).
fn cap_triangle(lathe: &Lathe, cap: &Conic3, pole: Pole, k: usize) -> Result<Patch, PatchError> {
    let (left, right) = (lathe.meridian(cap, k)?, lathe.meridian(cap, k + 1)?);
    match pole {
        Pole::Start => {
            // (pole, b1, b0): edges pole → b1 (right), b1 → b0 (the
            // parallel reversed), b0 → pole (left reversed).
            let parallel = lathe.parallel(cap.p1, k)?;
            Patch::new(
                [cap.p0, right.p1, left.p1],
                [right.c, parallel.c, left.c],
                [right.w, parallel.w, left.w],
            )
        }
        Pole::End => {
            // (a0, a1, pole): edges a0 → a1 (the parallel), a1 → pole
            // (right), pole → a0 (left reversed).
            let parallel = lathe.parallel(cap.p0, k)?;
            Patch::new(
                [left.p0, right.p0, cap.p1],
                [parallel.c, right.c, left.c],
                [parallel.w, right.w, left.w],
            )
        }
    }
}

#[cfg(test)]
mod tests;
