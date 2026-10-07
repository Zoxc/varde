//! Splines: smooth free-form curves, cubic non-rational B-splines, open
//! (clamped) or closed (periodic), through fit points or by control
//! points ([`Spline`]); their math is in [`BSpline`] and
//! [`Interpolation`], and as Bézier segments for drawing, profiles and
//! the shape tools in `bezier.rs`.

mod basis;
pub(crate) mod bezier;

pub use basis::{
    BSpline, Interpolation, MIN_KNOT_GAP, MIN_SPAN, chord_params, control_knots, handle_scale,
};
pub(crate) use basis::{Basis, SplineMap, curvature};

use std::collections::HashSet;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::{Curve, EditError, Id, OutOfIds, Sketch};

/// The most fit or control points a spline may have. Interpolating
/// through them solves a dense system of about twice as many unknowns,
/// a millisecond or so, which a file could otherwise make as large as it
/// liked.
pub const MAX_SPLINE_POINTS: usize = 100;

/// How a spline's points shape it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SplineKind {
    /// Through its points, fit points, each at its chord-length parameter
    /// ([`chord_params`]), found again from the points each time they
    /// move (see [`Interpolation`]).
    Through,
    /// Pulled towards its points, control points, without touching them
    /// but at an open spline's ends, over knots it keeps
    /// ([`Spline::knots`]).
    Control,
}

impl SplineKind {
    /// The fewest points a spline of the kind has: a cubic by control
    /// points needs four open, three closed; one through fit points two
    /// open (a line, or a curve by its handles), three closed.
    pub fn least(self, closed: bool) -> usize {
        match (self, closed) {
            (SplineKind::Control, false) => 4,
            (SplineKind::Through, false) => 2,
            (_, true) => 3,
        }
    }
}

/// A handle at a fit point: an ordinary point, its tip, whose direction
/// from the fit point sets the spline's tangent there and whose distance
/// how strongly it follows it (see [`Interpolation`]). So dragging,
/// fixing and dimensioning one is as for any point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle {
    /// The fit point it's at.
    pub at: Id,
    pub tip: Id,
}

/// A spline by its points, see [`SplineKind`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spline {
    pub kind: SplineKind,
    /// The fit or control points, in order.
    pub points: Vec<Id>,
    /// A closed spline makes a loop, from its last point on round to its
    /// first.
    pub closed: bool,
    /// Handles at fit points, at most one at each, only through fit
    /// points.
    pub handles: Vec<Handle>,
    /// By control points, its knots, as [`BSpline::clamped`] (open) or
    /// [`BSpline::periodic`] (closed) take them, which keep it as it was
    /// made or converted; through fit points, none, the parameters being
    /// found from the points.
    pub knots: Vec<f64>,
}

impl Spline {
    /// The spline through the fit points `points`, without handles.
    pub fn through(points: Vec<Id>, closed: bool) -> Spline {
        Spline {
            kind: SplineKind::Through,
            points,
            closed,
            handles: Vec::new(),
            knots: Vec::new(),
        }
    }

    /// Its fit or control points, then its handles' tips.
    pub fn all_points(&self) -> impl Iterator<Item = Id> + Clone + '_ {
        let tips = self.handles.iter().map(|handle| handle.tip);
        self.points.iter().copied().chain(tips)
    }

    /// An open spline's first and last points, where it starts and ends
    /// (through fit points or by control points alike). `None` if it's
    /// closed.
    pub fn ends(&self) -> Option<[Id; 2]> {
        match (self.closed, self.points.first(), self.points.last()) {
            (false, Some(&first), Some(&last)) => Some([first, last]),
            _ => None,
        }
    }

    /// Its parameter at its end `at`, if it's open: 0 at its first point,
    /// 1 at its last. `None` if `at` isn't an end of it.
    pub(crate) fn end_param(&self, at: Id) -> Option<f64> {
        match self.ends()? {
            [first, _] if first == at => Some(0.0),
            [_, last] if last == at => Some(1.0),
            _ => None,
        }
    }

    /// Whether its counts and knots are as they can be, for
    /// [`Sketch::check`]: from [`SplineKind::least`] to
    /// [`MAX_SPLINE_POINTS`] points, handles only through fit points, on
    /// its points and at most one on each, and knots only by control
    /// points, as many as [`BSpline`] takes and increasing within `[0,
    /// 1)`. Whether its points are points, and none repeated, is
    /// [`Sketch::check`]'s to say. Its counts are bounded before anything
    /// else is looked at, so a spline from outside can be asked.
    pub fn fits(&self) -> bool {
        let count = self.points.len();
        if count < self.kind.least(self.closed) || count > MAX_SPLINE_POINTS {
            return false;
        }
        match self.kind {
            SplineKind::Through => {
                self.knots.is_empty()
                    && self.handles.len() <= count
                    && self.handles.iter().enumerate().all(|(i, handle)| {
                        self.points.contains(&handle.at)
                            && !self.handles[..i].iter().any(|other| other.at == handle.at)
                    })
            }
            SplineKind::Control => {
                let control = vec![DVec2::ZERO; count];
                self.handles.is_empty() && self.shape_of(control, &[]).is_some()
            }
        }
    }

    /// The spline its points at `points` and its handles' tips at `tips`
    /// make: `None` if they're not as many as it has, or the knots or the
    /// interpolation aren't sound.
    fn shape_of(&self, points: Vec<DVec2>, tips: &[DVec2]) -> Option<BSpline> {
        match self.kind {
            SplineKind::Control => BSpline::new(&self.knots, points, self.closed),
            SplineKind::Through => {
                let handles = self.handle_indices()?;
                Interpolation::at_chords(&points, self.closed, &handles)?.spline(&points, tips)
            }
        }
    }

    /// The spline as the solver holds it through a solve, with its points
    /// where `at` says: its [`SplineMap`], and the points it maps, its
    /// fit or control points then its handles' tips in order of their fit
    /// points. `None` if it names a point `at` doesn't know, or the
    /// points make no sound spline.
    pub(crate) fn map(&self, at: impl Fn(Id) -> Option<DVec2>) -> Option<(SplineMap, Vec<Id>)> {
        let inputs: Vec<Id> = self
            .points
            .iter()
            .copied()
            .chain(self.ordered_tips()?)
            .collect();
        let map = match self.kind {
            SplineKind::Control => SplineMap::control(&self.knots, self.closed, self.points.len())?,
            SplineKind::Through => {
                let places: Vec<DVec2> = self
                    .points
                    .iter()
                    .map(|&id| at(id))
                    .collect::<Option<_>>()?;
                let handles = self.handle_indices()?;
                SplineMap::through(&Interpolation::at_chords(&places, self.closed, &handles)?)
            }
        };
        Some((map, inputs))
    }

    /// Whether it has a handle at the point `at`.
    pub fn has_handle(&self, at: Id) -> bool {
        self.handles.iter().any(|handle| handle.at == at)
    }

    /// Its handles in order of their fit points, each as the index of its
    /// fit point among its points and its tip. `None` if one is at none
    /// of its points.
    fn ordered_handles(&self) -> Option<Vec<(usize, Id)>> {
        let mut handles: Vec<(usize, Id)> = self
            .handles
            .iter()
            .map(|handle| {
                let index = self.points.iter().position(|&id| id == handle.at)?;
                Some((index, handle.tip))
            })
            .collect::<Option<_>>()?;
        handles.sort_unstable();
        Some(handles)
    }

    /// The indices among its points of those with handles, in order (see
    /// [`Spline::ordered_handles`]).
    pub(crate) fn handle_indices(&self) -> Option<Vec<usize>> {
        let handles = self.ordered_handles()?;
        Some(handles.into_iter().map(|(index, _)| index).collect())
    }

    /// Its handles' tips in order of their fit points (see
    /// [`Spline::ordered_handles`]).
    fn ordered_tips(&self) -> Option<Vec<Id>> {
        let handles = self.ordered_handles()?;
        Some(handles.into_iter().map(|(_, tip)| tip).collect())
    }

    /// The spline without the points `deleted`, for [`Sketch::delete`]:
    /// its handles at them or with their tips among them gone, and by
    /// control points its knots found anew ([`control_knots`]) from where
    /// `at` says the rest are. With the tips of the handles that went
    /// but weren't deleted themselves. `None` if too few points are
    /// left.
    pub(crate) fn without(
        &self,
        deleted: &HashSet<Id>,
        at: impl Fn(Id) -> Option<DVec2>,
    ) -> Option<(Spline, Vec<Id>)> {
        let points: Vec<Id> = self
            .points
            .iter()
            .copied()
            .filter(|id| !deleted.contains(id))
            .collect();
        if points.len() < self.kind.least(self.closed) {
            return None;
        }
        let (handles, gone): (Vec<Handle>, Vec<Handle>) = self
            .handles
            .iter()
            .partition(|handle| !deleted.contains(&handle.at) && !deleted.contains(&handle.tip));
        let knots = match self.kind {
            SplineKind::Through => Vec::new(),
            SplineKind::Control if points.len() == self.points.len() => self.knots.clone(),
            SplineKind::Control => {
                let places: Vec<DVec2> = points.iter().map(|&id| at(id)).collect::<Option<_>>()?;
                control_knots(&places, self.closed)
            }
        };
        let dropped = gone.iter().map(|handle| handle.tip);
        let dropped = dropped.filter(|tip| !deleted.contains(tip)).collect();
        let spline = Spline {
            points,
            handles,
            knots,
            ..self.clone()
        };
        Some((spline, dropped))
    }
}

impl Sketch {
    /// The sketch's splines, with their ids.
    pub fn splines(&self) -> impl Iterator<Item = (Id, &Spline)> {
        self.curves.iter().filter_map(|entry| match &entry.curve {
            Curve::Spline(spline) => Some((entry.id, spline)),
            _ => None,
        })
    }

    /// The handle whose tip is the point `tip`, and the spline it's on,
    /// if there's one: the first spline's with it.
    pub fn handle(&self, tip: Id) -> Option<(Id, Handle)> {
        self.splines().find_map(|(id, spline)| {
            let handle = spline.handles.iter().find(|handle| handle.tip == tip)?;
            Some((id, *handle))
        })
    }

    /// A line's start and end, an axis's as [`Sketch::line`] gives them,
    /// or a handle's fit point and tip, named by its tip: what an angle
    /// is measured between ([`Measure::Angle`](crate::Measure::Angle)).
    /// `None` for anything else, or a point missing.
    pub fn direction(&self, id: Id) -> Option<(DVec2, DVec2)> {
        if let Some(line) = self.line(id) {
            return Some(line);
        }
        let (_, handle) = self.handle(id)?;
        Some((self.point(handle.at)?.at, self.point(handle.tip)?.at))
    }

    /// A line's start and end, or a handle's whole line, named by its
    /// tip: from its mirrored end, across its fit point, to its tip. What
    /// a length is measured along ([`Measure::Length`](crate::Measure::Length)).
    /// `None` for anything else, or a point missing.
    pub fn span(&self, id: Id) -> Option<(DVec2, DVec2)> {
        if let Some(line) = self.line(id) {
            return Some(line);
        }
        let (_, handle) = self.handle(id)?;
        let (at, tip) = (self.point(handle.at)?.at, self.point(handle.tip)?.at);
        Some((2.0 * at - tip, tip))
    }

    /// The shape of `spline`, where the sketch has its points: `None` if
    /// it names a point the sketch doesn't have, or its points make no
    /// sound spline (as [`Sketch::check`] rules out for its knots and
    /// counts).
    pub fn spline_shape(&self, spline: &Spline) -> Option<BSpline> {
        let points = self.places(&spline.points)?;
        let tips = self.places(&spline.ordered_tips()?)?;
        spline.shape_of(points, &tips)
    }

    /// Converts the spline `curve` to `to`, keeping its shape: through
    /// fit points to by control points exactly (the interpolation's
    /// control points and knots), the other way through the places its
    /// knots are at, which it then passes through but between them
    /// strays a hair, with handles at its ends or at every one, whichever
    /// keeps nearer its shape ([`through_tips`]). Knots a spline with a
    /// handle at every fit point would have go back to those fit points
    /// ([`handled_params`]), not one at each knot, so converting round
    /// and back doesn't grow it. An open spline keeps its first and last points,
    /// the rest are new, and those only it used go with what's on them,
    /// as do its handles. Nothing changes if it's `to` already.
    /// `Target` if `curve` is no spline or has no shape.
    pub fn convert_spline(&mut self, curve: Id, to: SplineKind) -> Result<(), EditError> {
        let target = EditError::Target(curve);
        let spline = self.spline(curve).ok_or(target)?;
        if spline.kind == to {
            return Ok(());
        }
        let spline = spline.clone();
        let shape = self.spline_shape(&spline).ok_or(target)?;
        let mut breaks = Vec::new();
        let (places, knots) = match to {
            SplineKind::Control => (shape.control().to_vec(), shape.knots().to_vec()),
            SplineKind::Through => {
                // A closed spline's knots are where its pieces meet,
                // round from the first; 0 needn't be one.
                breaks = if let Some(params) = handled_params(&shape) {
                    params
                } else if spline.closed {
                    shape.knots().to_vec()
                } else {
                    shape.breaks()
                };
                let places: Vec<DVec2> = breaks.iter().map(|&t| shape.point(t)).collect();
                (places, Vec::new())
            }
        };
        if places.len() < to.least(spline.closed) || places.len() > MAX_SPLINE_POINTS {
            return Err(target);
        }
        // Through fit points, the handles, each its fit point's index and
        // its tip.
        let tips = match to {
            SplineKind::Control => Vec::new(),
            SplineKind::Through => through_tips(&shape, &breaks, &places).ok_or(target)?,
        };
        let last = places.len() - 1;
        let mut points = Vec::with_capacity(places.len());
        for (i, &place) in places.iter().enumerate() {
            let kept = match spline.ends() {
                Some([first, _]) if i == 0 => Some(first),
                Some([_, end]) if i == last => Some(end),
                _ => None,
            };
            points.push(match kept {
                Some(id) => id,
                None => self.add_point(place)?,
            });
        }
        let mut handles = Vec::new();
        for (i, tip) in tips {
            let tip = self.add_point(tip)?;
            handles.push(Handle { at: points[i], tip });
        }
        let old: Vec<Id> = spline.all_points().collect();
        let entry = self.curve_mut(curve).ok_or(target)?;
        entry.curve = Curve::Spline(Spline {
            kind: to,
            points,
            closed: spline.closed,
            handles,
            knots,
        });
        self.delete_unused(&old);
        Ok(())
    }
}

/// The most teeth a spline's curvature comb has
/// ([`Sketch::curvature_comb`]), however many segments it has.
pub const MAX_COMB_TEETH: usize = 1000;

/// The most teeth a curvature comb has in each of a spline's segments.
const COMB_PER_SEGMENT: usize = 16;

/// The curvature times a spline's size below which its curvature comb
/// counts it straight: rounding's noise, not a turn.
const COMB_FLAT: f64 = 1e-9;

impl Sketch {
    /// The spline `curve`, if it's one of the sketch's.
    pub fn spline(&self, curve: Id) -> Option<&Spline> {
        match &self.curve(curve)?.curve {
            Curve::Spline(spline) => Some(spline),
            _ => None,
        }
    }

    /// Where the tip of a handle at the fit point `at` of the spline
    /// `curve` through fit points would be for the spline to keep the
    /// tangent it has there: its derivative there, as [`Interpolation`]
    /// reads a handle. Drawn where an end has none, and where
    /// [`SketchEdit::AddHandles`](crate::SketchEdit::AddHandles) puts one.
    /// `None` if `curve` is no spline through fit points, `at` none of
    /// them, or the spline has no shape.
    pub fn handle_tip(&self, curve: Id, at: Id) -> Option<DVec2> {
        let spline = self.spline(curve)?;
        if spline.kind != SplineKind::Through {
            return None;
        }
        let index = spline.points.iter().position(|&id| id == at)?;
        let places = self.places(&spline.points)?;
        let params = chord_params(&places, spline.closed);
        let derivative = self.spline_shape(spline)?.eval(params[index])[1];
        let tip = places[index] + derivative / handle_scale(&params, spline.closed, index);
        tip.is_finite().then_some(tip)
    }

    /// Gives the fit points `points` handles, each on every spline
    /// through fit points it's a fit point of without one, its tip where
    /// the spline keeps its tangent ([`Sketch::handle_tip`]). `Target`
    /// for a point that's no such fit point.
    pub(crate) fn add_handles(&mut self, points: &[Id]) -> Result<(), EditError> {
        let mut added = Vec::new();
        for (i, &at) in points.iter().enumerate() {
            // Named twice, handled once.
            if points[..i].contains(&at) {
                continue;
            }
            let before = added.len();
            for (curve, spline) in self.splines() {
                if spline.kind == SplineKind::Through
                    && spline.points.contains(&at)
                    && !spline.has_handle(at)
                {
                    let tip = self.handle_tip(curve, at).ok_or(EditError::Target(at))?;
                    added.push((curve, at, tip));
                }
            }
            if added.len() == before {
                return Err(EditError::Target(at));
            }
        }
        for (curve, at, place) in added {
            let tip = self.add_point(place)?;
            let Some(Curve::Spline(spline)) = self.curve_mut(curve).map(|entry| &mut entry.curve)
            else {
                return Err(EditError::Target(curve));
            };
            spline.handles.push(Handle { at, tip });
        }
        Ok(())
    }

    /// Adds a point to the spline `curve` where it passes nearest `near`,
    /// see [`SketchEdit::InsertPoint`](crate::SketchEdit::InsertPoint).
    /// `Target` if `curve` is no spline with a shape, or a point of it is
    /// there already.
    pub(crate) fn insert_spline_point(&mut self, curve: Id, near: DVec2) -> Result<(), EditError> {
        let target = EditError::Target(curve);
        let spline = self.spline(curve).ok_or(target)?.clone();
        let shape = self.spline_shape(&spline).ok_or(target)?;
        let u = shape.path().closest(near);
        let (points, knots) = match spline.kind {
            SplineKind::Through => {
                let places = self.places(&spline.points).ok_or(target)?;
                let params = chord_params(&places, spline.closed);
                if params.iter().any(|&t| (t - u).abs() < MIN_SPAN) || (u >= 1.0 - MIN_SPAN) {
                    return Err(target);
                }
                let index = params.partition_point(|&t| t < u);
                let mut points = spline.points.clone();
                points.insert(index, self.add_point(shape.point(u))?);
                (points, Vec::new())
            }
            SplineKind::Control => {
                let inserted = shape.with_knot(u).ok_or(target)?;
                let points = self.reuse_points(&spline.points, inserted.control())?;
                (points, inserted.knots().to_vec())
            }
        };
        let old: Vec<Id> = spline.points.clone();
        let entry = self.curve_mut(curve).ok_or(target)?;
        entry.curve = Curve::Spline(Spline {
            points,
            knots,
            ..spline
        });
        self.delete_unused(&old);
        Ok(())
    }

    /// The piece of the spline `curve` from its parameter `from` to `to`
    /// (past 1, round a closed one's start), as an open spline of its
    /// kind from the point `start` to the point `end`, which are where it
    /// is at `from` and `to`, keeping its shape as trimming it does: by
    /// control points exactly ([`BSpline::piece`]), its control points
    /// kept where they stay and new ones where they don't; through fit
    /// points through those between, with their handles, and at an end
    /// that's none of its own a handle giving it the tangent it had there
    /// (straying a little between). Fit points within `tolerance` of
    /// either end are left out. `Target` if the piece has no shape.
    pub(crate) fn spline_piece(
        &mut self,
        curve: Id,
        [from, to]: [f64; 2],
        [start, end]: [Id; 2],
        tolerance: f64,
    ) -> Result<Spline, EditError> {
        let target = EditError::Target(curve);
        let spline = self.spline(curve).ok_or(target)?.clone();
        let shape = self.spline_shape(&spline).ok_or(target)?;
        if spline.kind == SplineKind::Control {
            let piece = shape.piece(from, to).ok_or(target)?;
            let control = piece.control();
            let inner = &control[1..control.len() - 1];
            let mut points = vec![start];
            points.extend(self.reuse_points(&spline.points, inner)?);
            points.push(end);
            return Ok(Spline {
                kind: SplineKind::Control,
                points,
                closed: false,
                handles: Vec::new(),
                knots: piece.knots().to_vec(),
            });
        }
        let places = self.places(&spline.points).ok_or(target)?;
        let params = chord_params(&places, spline.closed);
        let at = |id| self.point(id).map(|point| point.at).ok_or(target);
        let (start_at, end_at) = (at(start)?, at(end)?);
        // The fit points between, by where they are along it.
        let mut between: Vec<(f64, Id)> = Vec::new();
        for ((&id, &place), &t) in spline.points.iter().zip(&places).zip(&params) {
            let t = if spline.closed && t <= from {
                t + 1.0
            } else {
                t
            };
            let clear = place.distance(start_at) > tolerance && place.distance(end_at) > tolerance;
            if t > from && t < to && id != start && id != end && clear {
                between.push((t, id));
            }
        }
        between.sort_by(|a, b| a.0.total_cmp(&b.0));
        let ids: Vec<Id> = std::iter::once(start)
            .chain(between.iter().map(|&(_, id)| id))
            .chain([end])
            .collect();
        let along: Vec<f64> = std::iter::once(from)
            .chain(between.iter().map(|&(t, _)| t))
            .chain([to])
            .collect();
        let new_places = self.places(&ids).ok_or(target)?;
        let mut handles: Vec<Handle> = spline
            .handles
            .iter()
            .filter(|handle| ids.contains(&handle.at))
            .copied()
            .collect();
        let ends = [ids[0], ids[ids.len() - 1]];
        for (at, tip) in ends.into_iter().zip(end_tips(&shape, &along, &new_places)) {
            if let Some(tip) = tip
                && !spline.points.contains(&at)
            {
                let tip = self.add_point(tip)?;
                handles.push(Handle { at, tip });
            }
        }
        Ok(Spline {
            kind: SplineKind::Through,
            points: ids,
            closed: false,
            handles,
            knots: Vec::new(),
        })
    }

    /// The points at `places`, in order: of `own`, the first not taken
    /// yet exactly at each place, where one is, else a new one there. As
    /// inserting a knot leaves most control points where they were.
    pub(crate) fn reuse_points(
        &mut self,
        own: &[Id],
        places: &[DVec2],
    ) -> Result<Vec<Id>, OutOfIds> {
        let mut taken = HashSet::new();
        let mut points = Vec::with_capacity(places.len());
        for &place in places {
            let found = own.iter().copied().find(|id| {
                !taken.contains(id) && self.point(*id).is_some_and(|point| point.at == place)
            });
            let id = match found {
                Some(id) => id,
                None => self.add_point(place)?,
            };
            taken.insert(id);
            points.push(id);
        }
        Ok(points)
    }

    /// Deletes those of `points` no curve is made from any more, with
    /// what's on them.
    pub(crate) fn delete_unused(&mut self, points: &[Id]) {
        let used: HashSet<Id> = self
            .curves
            .iter()
            .flat_map(|entry| entry.curve.points())
            .collect();
        let unused: Vec<Id> = points
            .iter()
            .copied()
            .filter(|id| !used.contains(id))
            .collect();
        self.delete(&unused);
    }

    /// The curvature comb of the spline `curve`: places along it, evenly
    /// by its parameter within each of its segments, at most
    /// [`MAX_COMB_TEETH`] of them, each with its curvature times the unit
    /// normal on its left, which points to where it turns and is as long
    /// as one over the radius it turns by. Where it doesn't move the
    /// curvature is left at zero. `None` if `curve` is no spline with a
    /// shape.
    pub fn curvature_comb(&self, curve: Id) -> Option<Vec<[DVec2; 2]>> {
        let shape = self.spline_shape(self.spline(curve)?)?;
        let segments = shape.breaks().len() - 1;
        let per = (MAX_COMB_TEETH.saturating_sub(1) / segments.max(1)).clamp(1, COMB_PER_SEGMENT);
        let mut params = shape.samples(per);
        params.truncate(MAX_COMB_TEETH);
        let evals: Vec<[DVec2; 3]> = params.into_iter().map(|t| shape.eval(t)).collect();
        // Curving below rounding's reach of the spline's size reads as
        // straight, so a flat spline (or a flat stretch) has no noise of
        // teeth flipping side.
        let (low, high) = evals.iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(low, high), [place, ..]| (low.min(*place), high.max(*place)),
        );
        let least = COMB_FLAT / (high - low).max_element();
        let teeth = evals
            .into_iter()
            .map(|[place, first, second]| {
                let curvature = curvature(first, second);
                let normal = first.perp().normalize_or_zero();
                let curvature = if curvature.is_finite() && curvature.abs() > least {
                    curvature
                } else {
                    0.0
                };
                [place, normal * curvature]
            })
            .collect();
        Some(teeth)
    }
}

/// Where the tips of handles at each of `points` would be for the spline
/// through them, open or `closed`, to keep the shape it has without
/// handles (as [`Sketch::handle_tip`] gives them): what the Spline tool
/// puts at each fit point. `None` for points that make no spline.
pub fn handle_tips(points: &[DVec2], closed: bool) -> Option<Vec<DVec2>> {
    if points.len() < SplineKind::Through.least(closed) || points.len() > MAX_SPLINE_POINTS {
        return None;
    }
    let shape = BSpline::through(points, closed)?;
    let params = chord_params(points, closed);
    points
        .iter()
        .enumerate()
        .map(|(i, &at)| {
            let tip = at + shape.eval(params[i])[1] / handle_scale(&params, closed, i);
            tip.is_finite().then_some(tip)
        })
        .collect()
}

/// The spline of `kind` through or by `points`, open or `closed`, as a
/// polyline, as [`Sketch::flatten`] draws one: what the Spline tool
/// shows while it's drawn. `None` for too few points, or points that make
/// no spline (by control points, its knots are [`control_knots`]).
pub fn flatten_spline(points: &[DVec2], kind: SplineKind, closed: bool) -> Option<Vec<DVec2>> {
    if points.len() < kind.least(closed) || points.len() > MAX_SPLINE_POINTS {
        return None;
    }
    let shape = match kind {
        SplineKind::Through => BSpline::through(points, closed)?,
        SplineKind::Control => {
            BSpline::new(&control_knots(points, closed), points.to_vec(), closed)?
        }
    };
    let path = shape.path();
    path.is_sound().then(|| path.flatten())
}

/// How many places in each piece of a spline [`through_tips`] weighs a
/// spline through its breaks against it at.
const FIT_PER_PIECE: usize = 8;

/// How many times [`through_tips`] fits the handles' tips, each to
/// where the spline the last made came nearest the targets.
const FIT_ROUNDS: usize = 8;

/// The parameter near `u` where `spline` comes nearest `want`, by a few
/// steps of Newton's method from `u`; `u` if they go astray.
fn nearest_by_newton(spline: &BSpline, mut u: f64, want: DVec2) -> f64 {
    let start = u;
    for _ in 0..4 {
        let [p, d, dd] = spline.eval(u);
        let slope = d.length_squared() + (p - want).dot(dd);
        if slope.is_nan() || slope <= 0.0 {
            break;
        }
        u -= (p - want).dot(d) / slope;
    }
    if u.is_finite()
        && spline.point(u).distance_squared(want) <= spline.point(start).distance_squared(want)
    {
        u
    } else {
        start
    }
}

/// The handles, each its fit point's index and tip, for the spline
/// through `places`, which `shape` is at its parameters `along` (where
/// its pieces meet: an open one's breaks, a closed one's knots), to keep
/// to `shape` best: of two, the one nearer [`FIT_PER_PIECE`] places of
/// `shape` in each piece, each matched first by the parameter, piece for
/// piece, then where the spline comes nearest it. One has handles at an open
/// spline's ends only ([`end_tips`]), which keeps the shape exactly when
/// the breaks are at the chord-length parameters of `places`, as they are
/// for a spline through fit points without handles converted to control
/// points. The other has a handle at every fit point, as the Spline tool
/// draws them, their tips by least squares
/// ([`Interpolation::nearest_tips`]): the knots a handle brings can't be
/// kept to otherwise, so a spline drawn with handles converted to control
/// points and back strays visibly without them. `None` if neither makes
/// a spline.
fn through_tips(shape: &BSpline, along: &[f64], places: &[DVec2]) -> Option<Vec<(usize, DVec2)>> {
    let closed = shape.closed();
    let n = places.len();
    let params = chord_params(places, closed);
    let pieces = if closed { n } else { n - 1 };
    let mut targets = Vec::with_capacity(pieces * FIT_PER_PIECE);
    for i in 0..pieces {
        let (u0, s0) = (params[i], along[i]);
        let (u1, s1) = match (i + 1 < n, closed) {
            (true, _) => (params[i + 1], along[i + 1]),
            // A closed spline's last piece, round to its first point.
            _ => (1.0, along[0] + 1.0),
        };
        for k in 0..FIT_PER_PIECE {
            let f = (k as f64 + 0.5) / FIT_PER_PIECE as f64;
            let u = u0 + (u1 - u0) * f;
            let [at, d, _] = shape.eval(s0 + (s1 - s0) * f);
            targets.push((u, at, d.normalize_or_zero()));
        }
    }
    // How far the spline through `places` with `tips` at `handles` is
    // from the targets, each its distance from the nearest place near
    // where it's matched, and the targets matched there instead.
    let off = |handles: &[usize], tips: &[DVec2], targets: &[(f64, DVec2, DVec2)]| {
        let spline = Interpolation::at_chords(places, closed, handles)?.spline(places, tips)?;
        let mut off = 0.0;
        let mut nearer = Vec::with_capacity(targets.len());
        for &(u, want, along) in targets {
            let u = nearest_by_newton(&spline, u, want);
            off += spline.point(u).distance_squared(want);
            nearer.push((u, want, along));
        }
        off.is_finite().then_some((off, nearer))
    };
    let mut ends: Vec<(usize, DVec2)> = Vec::new();
    if !closed {
        for (i, tip) in [0, n - 1].into_iter().zip(end_tips(shape, along, places)) {
            if let Some(tip) = tip {
                ends.push((i, tip));
            }
        }
    }
    let (indices, tips): (Vec<usize>, Vec<DVec2>) = ends.iter().copied().unzip();
    let ends_off = off(&indices, &tips, &targets).map(|(off, _)| off);
    // A handle at every fit point, the tips fitted, then fitted again to
    // where the spline they make comes nearest the targets, keeping the
    // best.
    let every: Vec<usize> = (0..n).collect();
    let all = Interpolation::at_chords(places, closed, &every).and_then(|interpolation| {
        let mut targets = targets.clone();
        let mut best: Option<(f64, Vec<DVec2>)> = None;
        for _ in 0..FIT_ROUNDS {
            let Some(tips) = interpolation.nearest_tips(places, &targets) else {
                break;
            };
            let Some((off, nearer)) = off(&every, &tips, &targets) else {
                break;
            };
            if best.as_ref().is_none_or(|(least, _)| off < *least) {
                best = Some((off, tips));
            }
            targets = nearer;
        }
        best
    });
    match (ends_off, all) {
        (Some(ends_off), Some((all_off, tips))) if all_off < ends_off => {
            Some(every.into_iter().zip(tips).collect())
        }
        (None, Some((_, tips))) => Some(every.into_iter().zip(tips).collect()),
        (Some(_), _) => Some(ends),
        (None, None) => None,
    }
}

/// The parameters of the fit points of a spline with a handle at every
/// one whose knots are `shape`'s, if they're such a spline's: each such
/// fit point has two knots, a third of the way to its neighbours
/// ([`Interpolation`]), which a fit point at each would double. Read back
/// pair by pair, then checked by making the knots again from them.
fn handled_params(shape: &BSpline) -> Option<Vec<f64>> {
    const NEAR: f64 = 1e-9;
    let closed = shape.closed();
    let knots: Vec<f64> = if closed {
        shape.knots().to_vec()
    } else {
        let inner = shape.knots().iter().copied();
        inner.filter(|&k| k > 0.0 && k < 1.0).collect()
    };
    let m = knots.len();
    if m == 0 || !m.is_multiple_of(2) {
        return None;
    }
    let matches = |params: &[f64]| {
        let every: Vec<usize> = (0..params.len()).collect();
        let Some(interpolation) = Interpolation::new(params, closed, &every) else {
            return false;
        };
        let mut again: Vec<f64> = interpolation.knots().to_vec();
        if !closed {
            again.retain(|&k| k > 0.0 && k < 1.0);
        }
        again.len() == m && again.iter().zip(&knots).all(|(a, b)| (a - b).abs() < NEAR)
    };
    if closed {
        // The pairs start at the first knot or the second.
        (0..2).find_map(|offset| {
            // Knot `j` on from `offset`, counting round.
            let at = |j: usize| {
                let j = offset + j;
                knots[j % m] + (j / m) as f64
            };
            // Fit point `i` at `a`, its first knot, and its neighbour
            // before's second, `b`: `3a = 2t + t'`, `3b = 2t' + t`.
            let mut params: Vec<f64> = (0..m / 2)
                .map(|i| (2.0 * at(2 * i + m) - at(2 * i + m - 1)).rem_euclid(1.0))
                .collect();
            params.sort_by(f64::total_cmp);
            matches(&params).then_some(params)
        })
    } else {
        let mut params = vec![0.0];
        for pair in knots.chunks(2) {
            let before = *params.last()?;
            params.push((3.0 * pair[0] - before) / 2.0);
        }
        params.push(1.0);
        matches(&params).then_some(params)
    }
}

/// The tips of handles at the first and last of `places`, which `shape`
/// is at its parameters `along`, that give the open spline through
/// `places` the tangents `shape` has there: its derivative, by its own
/// parameter, scaled to the chord-length parameters of `places` by the
/// spans beside each end, as [`Interpolation`] reads a handle. Without
/// them, the spline's ends would have no second derivative, which
/// `shape`'s needn't, and it would stray further. `None` for a tip that
/// isn't finite.
fn end_tips(shape: &BSpline, along: &[f64], places: &[DVec2]) -> [Option<DVec2>; 2] {
    let params = chord_params(places, false);
    let last = places.len() - 1;
    [(0, 1), (last, last - 1)].map(|(i, next)| {
        let ratio = (along[next] - along[i]) / (params[next] - params[i]);
        let derivative = shape.eval(along[i])[1];
        let tip = places[i] + derivative * ratio / handle_scale(&params, false, i);
        tip.is_finite().then_some(tip)
    })
}

#[cfg(test)]
mod tests;
