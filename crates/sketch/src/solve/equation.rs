//! The equations the solver holds at zero, each written once over
//! [`Real`] and evaluated for its value and its gradient.
//!
//! Every residual is a length, so one tolerance means the same for all of
//! them: angles go through direction vectors (a cross or dot product over a
//! length), never an angle computed with `atan2` that wraps around.

use std::cell::RefCell;
use std::rc::Rc;

use glam::DVec2;

use crate::Id;
use crate::spline::{Basis, Interpolation, SplineMap};

use super::real::{Dual, MAX_INPUTS, Real, Vector};

/// Where an equation reads a number: a variable of the solver, by index,
/// or a constant (a coordinate of something fixed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Slot {
    Var(usize),
    Const(f64),
}

/// A point's coordinates.
pub(crate) type PointSlots = [Slot; 2];

/// A line by its ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LineSlots {
    pub start: PointSlots,
    pub end: PointSlots,
}

/// A circle, or an arc, whose radius is its start's distance from its
/// centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RoundSlots {
    Circle {
        center: PointSlots,
        radius: Slot,
    },
    Arc {
        center: PointSlots,
        start: PointSlots,
    },
}

impl RoundSlots {
    pub fn center(&self) -> PointSlots {
        match *self {
            RoundSlots::Circle { center, .. } | RoundSlots::Arc { center, .. } => center,
        }
    }
}

/// What an edge's gap ([`Residual::EdgeGap`]) runs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EdgeTo {
    Point(PointSlots),
    Line(LineSlots),
    Round(RoundSlots),
}

/// An offset pair (see [`Sketch::offset_pair`](crate::Sketch::offset_pair)):
/// how far the second of it lies from the first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PairSlots {
    /// The signed distance of the second line's midpoint from the line
    /// through the first, positive on its left.
    Lines { line: LineSlots, copy: LineSlots },
    /// The second's radius less the first's.
    Rounds { round: RoundSlots, copy: RoundSlots },
    /// A round join's radius about its corner.
    Join { copy: RoundSlots },
}

/// An offset pair as an equation reads it: by the slots of its curves,
/// or a point and a spline, read at a parameter of the point's own that a
/// [`Residual::Nearest`] holds at the place on it nearest the point,
/// where the point's offset is its distance, left of the way the spline
/// runs positive.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PairRead {
    Slots(PairSlots),
    Spline { point: PointSlots, at: SplineAt },
}

/// A spline as the solver reads it: its points' slots, the fit points
/// then the tips (or the control points), and its map from them to its
/// place and derivatives ([`SplineMap`]). Through fit points, the map is
/// linear in them for the parameters their places give, which it holds
/// through a step and finds anew after each ([`SplineSlots::refit`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SplineSlots {
    pub map: RefCell<SplineMap>,
    pub inputs: Vec<PointSlots>,
    /// About how long it is, at least a micrometre: its parameter, which
    /// runs from 0 to 1, is read as a length along it, from 0 to this,
    /// so it's scaled like the lengths it's solved with.
    pub length: f64,
    /// Through fit points, how its map is made from them.
    pub refit: Option<Refit>,
}

/// How a spline through fit points is mapped from them: the first `fit`
/// of its [`SplineSlots::inputs`], open or `closed`, with handles at the
/// fit points `handles` (see [`Interpolation`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Refit {
    pub fit: usize,
    pub closed: bool,
    pub handles: Vec<usize>,
}

impl SplineSlots {
    /// Whether its map changes as the solver moves its points: through
    /// fit points, some of which are variables.
    pub fn moves(&self) -> bool {
        self.refit.as_ref().is_some_and(|refit| {
            let fit = self.inputs[..refit.fit].iter().flatten();
            fit.into_iter().any(|slot| matches!(slot, Slot::Var(_)))
        })
    }

    /// Through fit points, finds its parameters anew from where the fit
    /// points are at the variables' values `x`, and its map with them.
    /// Kept as it was if they make no spline.
    pub fn refit(&self, x: &[f64]) {
        let Some(refit) = &self.refit else {
            return;
        };
        let places: Vec<DVec2> = self.inputs[..refit.fit]
            .iter()
            .map(|&[x_slot, y_slot]| DVec2::new(read(x, x_slot), read(x, y_slot)))
            .collect();
        if let Some(interpolation) = Interpolation::at_chords(&places, refit.closed, &refit.handles)
        {
            *self.map.borrow_mut() = SplineMap::through(&interpolation);
        }
    }

    /// The parameter, as a length along it, of the place on it nearest
    /// `point`, near `t` (as a length too), both read at the variables'
    /// values `x`: by Newton's method on the distance from `t`, a few
    /// steps, which a place a little off the curve by `t` needs, kept
    /// within its ends if it's open.
    pub fn project(&self, x: &[f64], point: PointSlots, t: f64) -> f64 {
        let value = |slot| read(x, slot);
        let point = Vector {
            x: value(point[0]),
            y: value(point[1]),
        };
        let closed = self.map.borrow().closed();
        let mut t = t / self.length;
        for _ in 0..PROJECTION_STEPS {
            let basis = self.map.borrow().basis(t);
            let [place, first, second] = [0, 1, 2].map(|order| self.eval(&value, &basis, order));
            let off = place - point;
            let slope = off.dot(first);
            let curving = first.dot(first) + off.dot(second);
            if curving.is_nan() || curving <= 0.0 {
                break;
            }
            let next = t - slope / curving;
            let next = if closed { next } else { next.clamp(0.0, 1.0) };
            if !next.is_finite() || next == t {
                break;
            }
            t = next;
        }
        t * self.length
    }

    /// Its derivative of `order` (0, its place, to 2) where `basis` is
    /// of, its points read through `value`.
    fn eval(&self, value: &impl Fn(Slot) -> f64, basis: &Basis, order: usize) -> Vector<f64> {
        let mut sum = Vector { x: 0.0, y: 0.0 };
        self.map.borrow().weights(basis, order, |point, weight| {
            let [x, y] = self.inputs[point];
            sum.x += weight * value(x);
            sum.y += weight * value(y);
        });
        sum
    }
}

/// The most steps [`SplineSlots::project`] takes: from a place off the
/// curve by a fraction of the last error, Newton's method gets to it in a
/// few.
const PROJECTION_STEPS: usize = 4;

/// Where an equation reads a spline: at the parameter `t`, as a length
/// along it (see [`SplineSlots::length`]): a variable for a point on it,
/// or a constant at an end.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SplineAt {
    pub spline: Rc<SplineSlots>,
    pub t: Slot,
}

impl SplineAt {
    /// The spline's basis where it's read, `t` being read through
    /// `value`.
    fn basis(&self, value: &impl Fn(Slot) -> f64) -> Basis {
        self.spline
            .map
            .borrow()
            .basis(value(self.t) / self.spline.length)
    }

    /// Whether it's `other`, read the same way.
    fn same(&self, other: &SplineAt) -> bool {
        Rc::ptr_eq(&self.spline, &other.spline) && self.t == other.t
    }
}

/// Which way a curve runs where two join, and how it curves there, for
/// a tangent or a smooth join with a spline (see
/// [`Joint`](crate::Joint)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Heading {
    /// Along a line, from its start to its end, straight.
    Line(LineSlots),
    /// Round a circle or an arc counter-clockwise, across the direction
    /// of `at` from its centre.
    Round { round: RoundSlots, at: PointSlots },
    /// Along a spline as its parameter runs, at an end.
    Spline(SplineAt),
}

/// What an equation says, as a residual that's zero when it holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Residual {
    /// `a - b`: two coordinates equal (coincident, horizontal, vertical).
    Equal(Slot, Slot),
    /// `point - (a + b) / 2`: a coordinate halfway between two others.
    Midpoint { point: Slot, a: Slot, b: Slot },
    /// A point's signed distance from the endless line through a line.
    OnLine { point: PointSlots, line: LineSlots },
    /// A point's distance from a circle's centre less its radius.
    OnRound {
        point: PointSlots,
        round: RoundSlots,
    },
    /// The cross product of two lines' directions over the geometric mean
    /// of their lengths: the sine of the angle between them times a
    /// length.
    Parallel(LineSlots, LineSlots),
    /// As [`Residual::Parallel`] with the dot product: the cosine.
    Perpendicular(LineSlots, LineSlots),
    /// The centre's signed distance from the line, less the radius on
    /// the side `sign` (1 or -1) says.
    TangentLine {
        line: LineSlots,
        round: RoundSlots,
        sign: f64,
    },
    /// The distance between the centres less `a`'s radius plus `b`'s
    /// times `sign`: 1 touching from outside, -1 with `b` inside `a`.
    TangentRounds {
        a: RoundSlots,
        b: RoundSlots,
        sign: f64,
    },
    /// The difference of two lines' lengths.
    EqualLength(LineSlots, LineSlots),
    /// The difference of two radii.
    EqualRadius(RoundSlots, RoundSlots),
    /// The signed distance of the midpoint of `a` and `b` from the line:
    /// half of being symmetric.
    MidpointOn {
        a: PointSlots,
        b: PointSlots,
        line: LineSlots,
    },
    /// How far `a` to `b` runs along the line: the other half.
    Across {
        a: PointSlots,
        b: PointSlots,
        line: LineSlots,
    },
    /// An arc's end's distance from its centre less its start's: the
    /// equation every arc implies.
    ArcRadius {
        center: PointSlots,
        start: PointSlots,
        end: PointSlots,
    },
    /// `slot - at`: a fixed number staying where it is. Only for the
    /// analysis: when solving, what's fixed is a constant instead.
    Fix { slot: Slot, at: f64 },
    /// The distance between two points less `value`: a distance or a
    /// line's length.
    Distance {
        a: PointSlots,
        b: PointSlots,
        value: f64,
    },
    /// `sign` (1 or -1) times `b - a`, less `value`: a horizontal or
    /// vertical distance.
    Offset {
        a: Slot,
        b: Slot,
        sign: f64,
        value: f64,
    },
    /// `sign` times the signed distance of the midpoint of `a` and `b`
    /// from the line, less `value`: of a point, `a` and `b` both being
    /// it, or of a line's middle.
    LineDistance {
        a: PointSlots,
        b: PointSlots,
        line: LineSlots,
        sign: f64,
        value: f64,
    },
    /// A radius less `value`.
    Radius { round: RoundSlots, value: f64 },
    /// The gap from a circle's edge to `to`, less `value`: of a point,
    /// `sign` times its distance from the centre less the radius (1
    /// outside, -1 inside); of a line, `sign` times the centre's signed
    /// distance from it, less the radius; of another circle, `sign` times
    /// the distance between the centres less the radius, less the other's
    /// radius (1 apart, -1 with it inside). See
    /// [`Measure::EdgeDistance`](crate::Measure::EdgeDistance).
    EdgeGap {
        round: RoundSlots,
        to: EdgeTo,
        sign: f64,
        value: f64,
    },
    /// `sign` times how far an offset pair's second lies from its first,
    /// less `value`.
    PairOffset {
        pair: PairRead,
        sign: f64,
        value: f64,
    },
    /// How far one offset pair's second lies from its first, less how far
    /// the other's does, either side: the square root of the square, so
    /// no side is stored, and a length. Each being held away from zero by
    /// the offset's dimension, neither can pass to the other side.
    EqualOffset(PairRead, PairRead),
    /// How far a point is along a spline from its place at `at`, a
    /// variable: zero where that's the place on it nearest the point (or
    /// the point is right across from it), which holds the parameter a
    /// point's offset from the spline is read at ([`PairRead::Spline`]).
    Nearest { point: PointSlots, at: SplineAt },
    /// The angle from `a`'s direction, times `sign`, to `b`'s, less the
    /// angle whose cosine and sine are `cos` and `sin`, as `2 sin(δ / 2)`
    /// of that difference `δ` times the geometric mean of the lines'
    /// lengths: the chord between the directions, a length. It's zero
    /// only where the angle is the one asked for, never at its mirror
    /// image half a turn off, where it's no number at all, so the solver
    /// can't flip a line round.
    Angle {
        a: LineSlots,
        b: LineSlots,
        sign: f64,
        cos: f64,
        sin: f64,
    },
    /// The cross product of `point - center` with the unit vector
    /// `direction`: the point's distance from the line through `center`
    /// that way. With the arc's radius equation, holds a fixed arc's end,
    /// which fixing both its coordinates would restate. Only for the
    /// analysis, as `Fix`.
    FixDirection {
        point: PointSlots,
        center: PointSlots,
        direction: [f64; 2],
    },
    /// A coordinate of a point (`axis` 0 its x, 1 its y) less that of a
    /// spline's place at `at`, a variable: with the other, the point on
    /// the spline.
    OnSpline {
        point: PointSlots,
        at: SplineAt,
        axis: usize,
    },
    /// Two curves running along each other where they join: as
    /// [`Residual::Angle`] of no angle from `a`'s way along, times `sign`,
    /// to `b`'s, so zero only where they run the same way (`sign` 1) or
    /// opposite ways (-1), and no number where they run the other.
    Along { a: Heading, b: Heading, sign: f64 },
    /// How much more `a` curves than `b` where they join, each its own
    /// way along, `b`'s times `sign` as for [`Residual::Along`], times the
    /// lengths of their [`Heading`]s' vectors (a line's, a radius, a
    /// spline's derivative), so a length.
    Curving { a: Heading, b: Heading, sign: f64 },
}

/// One equation, from the constraint (or the arc, for the equation it
/// implies) named by `source`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Equation {
    pub residual: Residual,
    pub source: Id,
    /// Whether it restates what may be held otherwise, so that it counts
    /// only where the rest don't hold it: never redundant, and left out
    /// of the analysis where it depends on the rest. The solver takes it
    /// as any other, as it holds wherever the rest do.
    pub implied: bool,
}

/// A slot's value among the variables' values `x`.
fn read(x: &[f64], slot: Slot) -> f64 {
    match slot {
        Slot::Var(index) => x[index],
        Slot::Const(value) => value,
    }
}

impl Heading {
    /// Calls `f` with every slot it reads but a spline's.
    fn slots(&self, f: &mut dyn FnMut(Slot)) {
        match self {
            Heading::Line(line) => line.start.into_iter().chain(line.end).for_each(f),
            Heading::Round { round, at } => {
                round_slots(*round, f);
                at.iter().copied().for_each(f);
            }
            Heading::Spline(_) => {}
        }
    }

    /// Its spline read at an end, if it's a spline's.
    fn spline(&self) -> Option<&SplineAt> {
        match self {
            Heading::Spline(at) => Some(at),
            _ => None,
        }
    }
}

/// Calls `f` with the slots of a circle or an arc.
fn round_slots(round: RoundSlots, f: &mut dyn FnMut(Slot)) {
    match round {
        RoundSlots::Circle { center, radius } => center.into_iter().chain([radius]).for_each(f),
        RoundSlots::Arc { center, start } => center.into_iter().chain(start).for_each(f),
    }
}

/// The residual of [`Residual::Angle`]: the angle from `u`, times
/// `sign`, to `w` less the angle whose cosine and sine are `cos` and
/// `sin`, as `2 sin(δ / 2)` of the difference `δ` times the geometric
/// mean of the vectors' lengths.
fn angle_off<R: Real>(u: Vector<R>, w: Vector<R>, sign: f64, cos: f64, sin: f64) -> R {
    let lengths = u.length() * w.length();
    let (cross, dot) = (u.cross(w) * R::constant(sign), u.dot(w) * R::constant(sign));
    // The sine and cosine of the difference, times the lengths.
    let (cos, sin) = (R::constant(cos), R::constant(sin));
    let sine = cross * cos - dot * sin;
    let cosine = dot * cos + cross * sin;
    // 2 sin(δ / 2) = sin δ / cos(δ / 2), cos(δ / 2) being
    // √((1 + cos δ) / 2).
    let half = ((lengths + cosine) / (lengths * R::constant(2.0))).sqrt();
    sine / (lengths.sqrt() * half)
}

impl Residual {
    /// Calls `f` with every slot the equation reads, some perhaps more
    /// than once: at most [`MAX_INPUTS`] of them but for a spline's, whose
    /// points may be many (see [`Residual::gradient`]).
    pub fn slots(&self, mut f: impl FnMut(Slot)) {
        let f: &mut dyn FnMut(Slot) = &mut f;
        self.direct_slots(f);
        for (at, _) in self.reads() {
            f(at.t);
            at.spline.inputs.iter().flatten().copied().for_each(&mut *f);
        }
    }

    /// Calls `f` with every slot the equation reads but through a
    /// spline: at most [`MAX_INPUTS`] of them, with two for each spline
    /// read ([`Residual::reads`]).
    fn direct_slots(&self, f: &mut dyn FnMut(Slot)) {
        let point = |p: PointSlots, f: &mut dyn FnMut(Slot)| p.into_iter().for_each(f);
        let line =
            |l: LineSlots, f: &mut dyn FnMut(Slot)| l.start.into_iter().chain(l.end).for_each(f);
        let round = |r: RoundSlots, f: &mut dyn FnMut(Slot)| round_slots(r, f);
        let pair = |p: &PairRead, f: &mut dyn FnMut(Slot)| match *p {
            PairRead::Slots(PairSlots::Lines { line: a, copy: b }) => {
                line(a, f);
                line(b, f);
            }
            PairRead::Slots(PairSlots::Rounds { round: a, copy: b }) => {
                round(a, f);
                round(b, f);
            }
            PairRead::Slots(PairSlots::Join { copy }) => round(copy, f),
            // Its spline is read at `at`, see `reads`.
            PairRead::Spline { point: p, .. } => point(p, f),
        };
        match *self {
            Residual::Equal(a, b) => [a, b].into_iter().for_each(f),
            Residual::Midpoint { point, a, b } => [point, a, b].into_iter().for_each(f),
            Residual::OnLine { point: p, line: l } => {
                point(p, f);
                line(l, f);
            }
            Residual::OnRound { point: p, round: r } => {
                point(p, f);
                round(r, f);
            }
            Residual::Parallel(a, b)
            | Residual::Perpendicular(a, b)
            | Residual::EqualLength(a, b) => {
                line(a, f);
                line(b, f);
            }
            Residual::TangentLine {
                line: l, round: r, ..
            } => {
                line(l, f);
                round(r, f);
            }
            Residual::TangentRounds { a, b, .. } | Residual::EqualRadius(a, b) => {
                round(a, f);
                round(b, f);
            }
            Residual::MidpointOn { a, b, line: l } | Residual::Across { a, b, line: l } => {
                point(a, f);
                point(b, f);
                line(l, f);
            }
            Residual::ArcRadius { center, start, end } => {
                point(center, f);
                point(start, f);
                point(end, f);
            }
            Residual::Fix { slot, .. } => f(slot),
            Residual::Distance { a, b, .. } => {
                point(a, f);
                point(b, f);
            }
            Residual::Offset { a, b, .. } => [a, b].into_iter().for_each(f),
            Residual::LineDistance { a, b, line: l, .. } => {
                point(a, f);
                point(b, f);
                line(l, f);
            }
            Residual::Radius { round: r, .. } => round(r, f),
            Residual::EdgeGap { round: r, to, .. } => {
                round(r, f);
                match to {
                    EdgeTo::Point(p) => point(p, f),
                    EdgeTo::Line(l) => line(l, f),
                    EdgeTo::Round(other) => round(other, f),
                }
            }
            Residual::PairOffset { pair: ref p, .. } => pair(p, f),
            Residual::EqualOffset(ref a, ref b) => {
                pair(a, f);
                pair(b, f);
            }
            Residual::Nearest { point: p, .. } => point(p, f),
            Residual::Angle { a, b, .. } => {
                line(a, f);
                line(b, f);
            }
            Residual::FixDirection {
                point: p, center, ..
            } => {
                point(p, f);
                point(center, f);
            }
            Residual::OnSpline { point: p, .. } => point(p, f),
            Residual::Along { ref a, ref b, .. } | Residual::Curving { ref a, ref b, .. } => {
                a.slots(f);
                b.slots(f);
            }
        }
    }

    /// The splines the equation reads, perhaps more than once.
    pub fn splines(&self) -> impl Iterator<Item = &Rc<SplineSlots>> {
        self.reads().into_iter().map(|(at, _)| &at.spline)
    }

    /// If it holds a point on a spline at a variable parameter, or that
    /// parameter at the place on the spline nearest the point, the point,
    /// the variable and the spline, once of a point on one's two (its x):
    /// a parameter found anew where the spline has moved.
    pub fn on_spline(&self) -> Option<(PointSlots, usize, &Rc<SplineSlots>)> {
        match self {
            Residual::OnSpline {
                point,
                at:
                    SplineAt {
                        spline,
                        t: Slot::Var(var),
                    },
                axis: 0,
            }
            | Residual::Nearest {
                point,
                at:
                    SplineAt {
                        spline,
                        t: Slot::Var(var),
                    },
            } => Some((*point, *var, spline)),
            _ => None,
        }
    }

    /// The splines the equation reads, each with the order of the
    /// derivative read (0, the place, to 2), each once. At most two
    /// splines, four reads.
    fn reads(&self) -> Vec<(&SplineAt, usize)> {
        let mut reads = Vec::new();
        fn pair(pair: &PairRead) -> Option<&SplineAt> {
            match pair {
                PairRead::Spline { at, .. } => Some(at),
                PairRead::Slots(_) => None,
            }
        }
        match self {
            Residual::OnSpline { at, .. } => reads.push((at, 0)),
            Residual::Nearest { at, .. } => reads.extend([(at, 0), (at, 1)]),
            Residual::PairOffset { pair: a, .. } => {
                reads.extend(pair(a).into_iter().flat_map(|at| [(at, 0), (at, 1)]));
            }
            Residual::EqualOffset(a, b) => {
                let ats = pair(a).into_iter().chain(pair(b));
                reads.extend(ats.flat_map(|at| [(at, 0), (at, 1)]));
            }
            Residual::Along { a, b, .. } => {
                for at in [a, b].into_iter().filter_map(Heading::spline) {
                    reads.push((at, 1));
                }
            }
            Residual::Curving { a, b, .. } => {
                for at in [a, b].into_iter().filter_map(Heading::spline) {
                    reads.extend([(at, 1), (at, 2)]);
                }
            }
            _ => {}
        }
        reads
    }

    /// The residual, reading each slot through `value` and each spline's
    /// derivative of an order at where it's read through `spline`.
    fn eval<R: Real>(
        &self,
        value: impl Fn(Slot) -> R,
        spline: impl Fn(&SplineAt, usize) -> Vector<R>,
    ) -> R {
        let point = |p: PointSlots| Vector {
            x: value(p[0]),
            y: value(p[1]),
        };
        let direction = |l: LineSlots| point(l.end) - point(l.start);
        let radius = |r: RoundSlots| match r {
            RoundSlots::Circle { radius, .. } => value(radius),
            RoundSlots::Arc { center, start } => (point(start) - point(center)).length(),
        };
        // The signed distance of `p` from the line, positive on its left.
        let distance = |p: Vector<R>, l: LineSlots| {
            let along = direction(l);
            along.cross(p - point(l.start)) / along.length()
        };
        let mean_length = |a: Vector<R>, b: Vector<R>| (a.length() * b.length()).sqrt();
        let offset = |p: &PairRead| match *p {
            PairRead::Slots(PairSlots::Lines { line, copy }) => {
                distance(point(copy.start).midpoint(point(copy.end)), line)
            }
            PairRead::Slots(PairSlots::Rounds { round, copy }) => radius(copy) - radius(round),
            PairRead::Slots(PairSlots::Join { copy }) => radius(copy),
            PairRead::Spline { point: p, ref at } => {
                let way = spline(at, 1);
                way.cross(point(p) - spline(at, 0)) / way.length()
            }
        };
        // A heading's way along, and how it curves.
        let way = |h: &Heading| match h {
            Heading::Line(line) => direction(*line),
            Heading::Round { round, at } => {
                let from = point(*at) - point(round.center());
                Vector {
                    x: -from.y,
                    y: from.x,
                }
            }
            Heading::Spline(at) => spline(at, 1),
        };
        let curving = |h: &Heading| match h {
            Heading::Line(_) => R::constant(0.0),
            Heading::Round { round, .. } => R::constant(1.0) / radius(*round),
            Heading::Spline(at) => {
                let (first, second) = (spline(at, 1), spline(at, 2));
                let speed = first.length();
                first.cross(second) / (speed * speed * speed)
            }
        };
        match *self {
            Residual::Equal(a, b) => value(a) - value(b),
            Residual::Midpoint { point, a, b } => {
                value(point) - (value(a) + value(b)) * R::constant(0.5)
            }
            Residual::OnLine { point: p, line } => distance(point(p), line),
            Residual::OnRound { point: p, round } => {
                (point(p) - point(round.center())).length() - radius(round)
            }
            Residual::Parallel(a, b) => {
                let (a, b) = (direction(a), direction(b));
                a.cross(b) / mean_length(a, b)
            }
            Residual::Perpendicular(a, b) => {
                let (a, b) = (direction(a), direction(b));
                a.dot(b) / mean_length(a, b)
            }
            Residual::TangentLine { line, round, sign } => {
                distance(point(round.center()), line) - R::constant(sign) * radius(round)
            }
            Residual::TangentRounds { a, b, sign } => {
                let between = (point(a.center()) - point(b.center())).length();
                between - (radius(a) + R::constant(sign) * radius(b))
            }
            Residual::EqualLength(a, b) => direction(a).length() - direction(b).length(),
            Residual::EqualRadius(a, b) => radius(a) - radius(b),
            Residual::MidpointOn { a, b, line } => distance(point(a).midpoint(point(b)), line),
            Residual::Across { a, b, line } => {
                let along = direction(line);
                along.dot(point(b) - point(a)) / along.length()
            }
            Residual::ArcRadius { center, start, end } => {
                let center = point(center);
                (point(end) - center).length() - (point(start) - center).length()
            }
            Residual::Fix { slot, at } => value(slot) - R::constant(at),
            Residual::Distance { a, b, value } => {
                (point(b) - point(a)).length() - R::constant(value)
            }
            Residual::Offset {
                a,
                b,
                sign,
                value: v,
            } => (value(b) - value(a)) * R::constant(sign) - R::constant(v),
            Residual::LineDistance {
                a,
                b,
                line,
                sign,
                value,
            } => {
                distance(point(a).midpoint(point(b)), line) * R::constant(sign) - R::constant(value)
            }
            Residual::Radius { round, value } => radius(round) - R::constant(value),
            Residual::EdgeGap {
                round,
                to,
                sign,
                value,
            } => {
                let (center, sign) = (point(round.center()), R::constant(sign));
                let gap = match to {
                    EdgeTo::Point(p) => sign * ((point(p) - center).length() - radius(round)),
                    EdgeTo::Line(line) => sign * distance(center, line) - radius(round),
                    EdgeTo::Round(other) => {
                        let between = (point(other.center()) - center).length();
                        sign * (between - radius(round)) - radius(other)
                    }
                };
                gap - R::constant(value)
            }
            Residual::PairOffset {
                ref pair,
                sign,
                value,
            } => offset(pair) * R::constant(sign) - R::constant(value),
            Residual::EqualOffset(ref a, ref b) => {
                let (a, b) = (offset(a), offset(b));
                (a * a).sqrt() - (b * b).sqrt()
            }
            Residual::Nearest { point: p, ref at } => {
                let way = spline(at, 1);
                way.dot(point(p) - spline(at, 0)) / way.length()
            }
            Residual::Angle {
                a,
                b,
                sign,
                cos,
                sin,
            } => angle_off(direction(a), direction(b), sign, cos, sin),
            Residual::FixDirection {
                point: p,
                center,
                direction: [x, y],
            } => {
                let direction = Vector {
                    x: R::constant(x),
                    y: R::constant(y),
                };
                (point(p) - point(center)).cross(direction)
            }
            Residual::OnSpline {
                point: p,
                ref at,
                axis,
            } => {
                let on = spline(at, 0);
                value(p[axis]) - if axis == 0 { on.x } else { on.y }
            }
            Residual::Along { ref a, ref b, sign } => angle_off(way(a), way(b), sign, 1.0, 0.0),
            Residual::Curving { ref a, ref b, sign } => {
                let lengths = way(a).length() * way(b).length();
                (curving(a) - curving(b) * R::constant(sign)) * lengths
            }
        }
    }

    /// The residual at the variables' values `x`.
    pub fn value(&self, x: &[f64]) -> f64 {
        let value = |slot| read(x, slot);
        self.eval(value, |at, order| {
            at.spline.eval(&value, &at.basis(&value), order)
        })
    }

    /// The residual at `x` and its derivative by each variable it reads,
    /// through `f(variable, derivative)`, each variable once.
    ///
    /// Dual numbers carry a derivative per variable read, at most
    /// [`MAX_INPUTS`], which a spline's points may be many more than (a
    /// spline through fit points is a combination of all of them). So an
    /// equation reading a spline is differentiated by the chain rule: by
    /// dual numbers over its own variables and the spline's derivatives it
    /// reads, which the spline's map ([`SplineMap::weights`]) takes on to
    /// the spline's points, and its derivative a step higher to the
    /// parameter, where that's a variable.
    pub fn gradient(&self, x: &[f64], mut f: impl FnMut(usize, f64)) -> f64 {
        let mut vars = [0; MAX_INPUTS];
        let mut count = 0;
        self.direct_slots(&mut |slot| {
            if let Slot::Var(index) = slot
                && !vars[..count].contains(&index)
            {
                // No equation reads more than `MAX_INPUTS`: see there.
                vars[count] = index;
                count += 1;
            }
        });
        let (vars, first) = (&vars[..count], count);
        let value = |slot| read(x, slot);
        // Each read with the spline's basis there, and what it reads.
        let reads: Vec<(&SplineAt, usize, Basis, Vector<f64>)> = self
            .reads()
            .into_iter()
            .map(|(at, order)| {
                let basis = at.basis(&value);
                (at, order, basis, at.spline.eval(&value, &basis, order))
            })
            .collect();
        // What `eval` reads is what `direct_slots` and `reads` say, as
        // the finite-difference tests show of every kind; were it not, a
        // read would be taken as a constant, not added to another's
        // derivative.
        let dual = self.eval(
            |slot| match slot {
                Slot::Var(index) => match vars.iter().position(|&v| v == index) {
                    Some(position) => Dual::variable(x[index], position),
                    None => {
                        debug_assert!(false, "{self:?} reads {index} unlisted");
                        Dual::constant(x[index])
                    }
                },
                Slot::Const(value) => Dual::constant(value),
            },
            |at, order| {
                let read = reads
                    .iter()
                    .position(|&(other, o, ..)| o == order && other.same(at));
                match read {
                    Some(r) => {
                        let (position, place) = (first + 2 * r, reads[r].3);
                        Vector {
                            x: Dual::variable(place.x, position),
                            y: Dual::variable(place.y, position + 1),
                        }
                    }
                    None => {
                        debug_assert!(false, "{self:?} reads its spline unlisted");
                        let place = at.spline.eval(&value, &at.basis(&value), order);
                        Vector {
                            x: Dual::constant(place.x),
                            y: Dual::constant(place.y),
                        }
                    }
                }
            },
        );
        if reads.is_empty() {
            for (&var, &derivative) in vars.iter().zip(&dual.gradient) {
                f(var, derivative);
            }
            return dual.value;
        }
        let mut gradient: Vec<(usize, f64)> = vars.iter().copied().zip(dual.gradient).collect();
        let mut weights = Vec::new();
        for (r, (at, order, basis, _)) in reads.iter().enumerate() {
            let (order, basis) = (*order, basis);
            let [by_x, by_y] = [
                dual.gradient[first + 2 * r],
                dual.gradient[first + 2 * r + 1],
            ];
            // Each point's weight, summed over the control points.
            weights.clear();
            weights.resize(at.spline.inputs.len(), 0.0);
            at.spline
                .map
                .borrow()
                .weights(basis, order, |point, weight| {
                    weights[point] += weight;
                });
            for (&[x, y], &weight) in at.spline.inputs.iter().zip(&weights) {
                if weight == 0.0 {
                    continue;
                }
                for (slot, by) in [(x, by_x), (y, by_y)] {
                    if let Slot::Var(var) = slot {
                        gradient.push((var, by * weight));
                    }
                }
            }
            // Read at a variable parameter: only places are, so the next
            // derivative up is there to be had.
            if let Slot::Var(var) = at.t
                && order < 2
            {
                let along = at.spline.eval(&value, basis, order + 1);
                gradient.push((var, (by_x * along.x + by_y * along.y) / at.spline.length));
            }
        }
        gradient.sort_unstable_by_key(|&(var, _)| var);
        let mut merged = gradient.into_iter().peekable();
        while let Some((var, mut derivative)) = merged.next() {
            while let Some(&(next, more)) = merged.peek()
                && next == var
            {
                derivative += more;
                merged.next();
            }
            f(var, derivative);
        }
        dual.value
    }
}

#[cfg(test)]
mod tests;
