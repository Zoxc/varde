//! Splines: smooth free-form curves, cubic non-rational B-splines, open
//! (clamped) or closed (periodic), through fit points or by control
//! points ([`Spline`]); their math is in [`BSpline`] and
//! [`Interpolation`], and as Bézier segments for drawing, profiles and
//! the shape tools in `bezier.rs`, where they meet other curves in
//! `meet.rs`. Converting between the kinds is `convert.rs`, the
//! curvature comb `comb.rs`.

mod basis;
pub(crate) mod bezier;
mod comb;
mod convert;
pub(crate) mod meet;

pub use basis::{BSpline, Interpolation, MIN_KNOT_GAP, MIN_SPAN, chord_params, control_knots};
pub(crate) use basis::{Basis, SplineMap, curvature, handle_tip_for};
pub use comb::MAX_COMB_TEETH;
use convert::end_tips;

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

    /// Whether a spline of the kind, open or `closed`, may have `count`
    /// points: from [`SplineKind::least`] to [`MAX_SPLINE_POINTS`].
    pub fn admits(self, count: usize, closed: bool) -> bool {
        (self.least(closed)..=MAX_SPLINE_POINTS).contains(&count)
    }
}

/// A handle at a fit point: an ordinary point, its tip, whose direction
/// from the fit point sets the spline's tangent there and whose distance
/// how strongly it follows it (see [`Interpolation`]), and another, its
/// end, mirroring the tip in the fit point, held there by the solver. So
/// dragging, fixing and dimensioning either is as for any point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle {
    /// The fit point it's at.
    pub at: Id,
    pub tip: Id,
    /// As far the other side of `at` as `tip`. Defaulted to
    /// [`Id::MISSING`] for a handle from before ends were points, which
    /// [`Sketch::add_handle_ends`] gives one.
    #[serde(default = "missing")]
    pub end: Id,
}

fn missing() -> Id {
    Id::MISSING
}

impl Handle {
    /// Its tip and its end.
    pub fn arms(&self) -> [Id; 2] {
        [self.tip, self.end]
    }
}

/// What a spline's kind carries: through fit points its handles, by
/// control points its knots.
#[derive(Debug, Clone, PartialEq)]
pub enum SplineForm {
    /// Through fit points, with handles at fit points, at most one at
    /// each.
    Through { handles: Vec<Handle> },
    /// By control points, with its knots, as [`BSpline::clamped`] (open)
    /// or [`BSpline::periodic`] (closed) take them, which keep it as it
    /// was made or converted.
    Control { knots: Vec<f64> },
}

/// A spline by its points, see [`SplineKind`].
///
/// Stored flat, as `{kind, points, closed, handles, knots}`, as it was before its kind carried its data; a record
/// with handles by control points or knots through fit points doesn't
/// read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FlatSpline", into = "FlatSpline")]
pub struct Spline {
    /// Its kind, with its handles or knots.
    pub form: SplineForm,
    /// The fit or control points, in order.
    pub points: Vec<Id>,
    /// A closed spline makes a loop, from its last point on round to its
    /// first.
    pub closed: bool,
}

/// A [`Spline`] as stored: its kind beside both its handles and its
/// knots, the one its kind doesn't carry empty.
#[derive(Serialize, Deserialize)]
struct FlatSpline {
    kind: SplineKind,
    points: Vec<Id>,
    closed: bool,
    handles: Vec<Handle>,
    knots: Vec<f64>,
}

impl TryFrom<FlatSpline> for Spline {
    type Error = &'static str;

    fn try_from(flat: FlatSpline) -> Result<Spline, Self::Error> {
        let form = match flat.kind {
            SplineKind::Through if flat.knots.is_empty() => SplineForm::Through {
                handles: flat.handles,
            },
            SplineKind::Control if flat.handles.is_empty() => {
                SplineForm::Control { knots: flat.knots }
            }
            SplineKind::Through => return Err("knots on a spline through fit points"),
            SplineKind::Control => return Err("handles on a spline by control points"),
        };
        Ok(Spline {
            form,
            points: flat.points,
            closed: flat.closed,
        })
    }
}

impl From<Spline> for FlatSpline {
    fn from(spline: Spline) -> FlatSpline {
        let kind = spline.kind();
        let (handles, knots) = match spline.form {
            SplineForm::Through { handles } => (handles, Vec::new()),
            SplineForm::Control { knots } => (Vec::new(), knots),
        };
        FlatSpline {
            kind,
            points: spline.points,
            closed: spline.closed,
            handles,
            knots,
        }
    }
}

impl Spline {
    /// The spline through the fit points `points`, without handles.
    pub fn through(points: Vec<Id>, closed: bool) -> Spline {
        Spline {
            form: SplineForm::Through {
                handles: Vec::new(),
            },
            points,
            closed,
        }
    }

    /// The spline by the control points `points` and `knots`, as
    /// [`SplineForm::Control`] holds them.
    pub fn control(points: Vec<Id>, knots: Vec<f64>, closed: bool) -> Spline {
        Spline {
            form: SplineForm::Control { knots },
            points,
            closed,
        }
    }

    /// How its points shape it.
    pub fn kind(&self) -> SplineKind {
        match self.form {
            SplineForm::Through { .. } => SplineKind::Through,
            SplineForm::Control { .. } => SplineKind::Control,
        }
    }

    /// Its handles: none by control points.
    pub fn handles(&self) -> &[Handle] {
        match &self.form {
            SplineForm::Through { handles } => handles,
            SplineForm::Control { .. } => &[],
        }
    }

    /// Its handles, to change: `None` by control points, which have
    /// none.
    pub fn handles_mut(&mut self) -> Option<&mut Vec<Handle>> {
        match &mut self.form {
            SplineForm::Through { handles } => Some(handles),
            SplineForm::Control { .. } => None,
        }
    }

    /// Its knots: none through fit points, the parameters being found
    /// from the points.
    pub fn knots(&self) -> &[f64] {
        match &self.form {
            SplineForm::Through { .. } => &[],
            SplineForm::Control { knots } => knots,
        }
    }

    /// Its fit or control points, then its handles' tips and ends.
    pub fn all_points(&self) -> impl Iterator<Item = Id> + Clone + '_ {
        let arms = self.handles().iter().flat_map(Handle::arms);
        self.points.iter().copied().chain(arms)
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
        if !self.kind().admits(count, self.closed) {
            return false;
        }
        match &self.form {
            SplineForm::Through { handles } => {
                handles.len() <= count
                    && handles.iter().enumerate().all(|(i, handle)| {
                        self.points.contains(&handle.at)
                            && !handles[..i].iter().any(|other| other.at == handle.at)
                    })
            }
            SplineForm::Control { knots } => basis::knots_fit(knots, self.closed, count),
        }
    }

    /// The spline its points at `points` and its handles' tips at `tips`
    /// make: `None` if they're not as many as it has, or the knots or the
    /// interpolation aren't sound.
    fn shape_of(&self, points: Vec<DVec2>, tips: &[DVec2]) -> Option<BSpline> {
        match &self.form {
            SplineForm::Control { knots } => BSpline::new(knots, points, self.closed),
            SplineForm::Through { .. } => {
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
        let map = match &self.form {
            SplineForm::Control { knots } => {
                SplineMap::control(knots, self.closed, self.points.len())?
            }
            SplineForm::Through { .. } => {
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
        self.handles().iter().any(|handle| handle.at == at)
    }

    /// Its handles in order of their fit points, each as the index of its
    /// fit point among its points and its tip. `None` if one is at none
    /// of its points.
    fn ordered_handles(&self) -> Option<Vec<(usize, Id)>> {
        let mut handles: Vec<(usize, Id)> = self
            .handles()
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
    /// its handles at them or with their tips or ends among them gone,
    /// and by control points its knots found anew ([`control_knots`])
    /// from where `at` says the rest are. With the tips and ends of the
    /// handles that went but weren't deleted themselves. `None` if too few points are
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
        if points.len() < self.kind().least(self.closed) {
            return None;
        }
        let (form, gone) = match &self.form {
            SplineForm::Through { handles } => {
                let (handles, gone): (Vec<Handle>, Vec<Handle>) =
                    handles.iter().partition(|handle| {
                        !deleted.contains(&handle.at)
                            && handle.arms().iter().all(|id| !deleted.contains(id))
                    });
                (SplineForm::Through { handles }, gone)
            }
            SplineForm::Control { knots } if points.len() == self.points.len() => {
                let knots = knots.clone();
                (SplineForm::Control { knots }, Vec::new())
            }
            SplineForm::Control { .. } => {
                let places: Vec<DVec2> = points.iter().map(|&id| at(id)).collect::<Option<_>>()?;
                let knots = control_knots(&places, self.closed);
                (SplineForm::Control { knots }, Vec::new())
            }
        };
        let dropped = gone.iter().flat_map(Handle::arms);
        let dropped = dropped.filter(|tip| !deleted.contains(tip)).collect();
        let spline = Spline {
            form,
            points,
            closed: self.closed,
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
            let handle = spline.handles().iter().find(|handle| handle.tip == tip)?;
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
        Some((self.point(handle.end)?.at, self.point(handle.tip)?.at))
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

    /// The [`Sketch::spline_shape`] of the spline `curve`: `None` if
    /// there's no such spline, too.
    pub fn curve_shape(&self, curve: Id) -> Option<BSpline> {
        self.spline_shape(self.spline(curve)?)
    }

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
        if spline.kind() != SplineKind::Through {
            return None;
        }
        let index = spline.points.iter().position(|&id| id == at)?;
        let places = self.places(&spline.points)?;
        let params = chord_params(&places, spline.closed);
        let derivative = self.spline_shape(spline)?.eval(params[index])[1];
        handle_tip_for(&params, spline.closed, index, places[index], derivative)
    }

    /// Gives each handle read without an end ([`Id::MISSING`], from
    /// before ends were points) one, a new point mirroring its tip in its
    /// fit point. One whose fit point or tip is missing is left for
    /// [`Sketch::check`] to refuse.
    pub fn add_handle_ends(&mut self) -> Result<(), OutOfIds> {
        let at = |id| self.place(id);
        // Each handle without an end, by its curve's index and its own,
        // and where its end goes.
        let mut missing = Vec::new();
        for (index, entry) in self.curves.iter().enumerate() {
            let Curve::Spline(spline) = &entry.curve else {
                continue;
            };
            for (i, handle) in spline.handles().iter().enumerate() {
                if handle.end != Id::MISSING {
                    continue;
                }
                if let (Some(from), Some(tip)) = (at(handle.at), at(handle.tip)) {
                    missing.push((index, i, 2.0 * from - tip));
                }
            }
        }
        // Adding points leaves the curves where they are.
        for (index, i, place) in missing {
            let end = self.add_point(place)?;
            if let Curve::Spline(spline) = &mut self.curves[index].curve
                && let Some(handles) = spline.handles_mut()
            {
                handles[i].end = end;
            }
        }
        Ok(())
    }

    /// A new handle at the fit point `at`, which is at `from`, its tip
    /// a new point at `tip` and its end one mirroring it.
    pub fn new_handle(&mut self, at: Id, from: DVec2, tip: DVec2) -> Result<Handle, OutOfIds> {
        let tip_id = self.add_point(tip)?;
        let end = self.add_point(2.0 * from - tip)?;
        Ok(Handle {
            at,
            tip: tip_id,
            end,
        })
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
                if spline.kind() == SplineKind::Through
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
            let from = self.point(at).ok_or(EditError::Target(at))?.at;
            let handle = self.new_handle(at, from, place)?;
            let Some(Curve::Spline(spline)) = self.curve_mut(curve).map(|entry| &mut entry.curve)
            else {
                return Err(EditError::Target(curve));
            };
            if let Some(handles) = spline.handles_mut() {
                handles.push(handle);
            }
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
        let (points, form) = match &spline.form {
            SplineForm::Through { .. } => {
                let places = self.places(&spline.points).ok_or(target)?;
                let params = chord_params(&places, spline.closed);
                if params.iter().any(|&t| (t - u).abs() < MIN_SPAN) || (u >= 1.0 - MIN_SPAN) {
                    return Err(target);
                }
                let index = params.partition_point(|&t| t < u);
                let mut points = spline.points.clone();
                points.insert(index, self.add_point(shape.point(u))?);
                (points, spline.form.clone())
            }
            SplineForm::Control { .. } => {
                let inserted = shape.with_knot(u).ok_or(target)?;
                let points = self.reuse_points(&spline.points, inserted.control())?;
                let knots = inserted.knots().to_vec();
                (points, SplineForm::Control { knots })
            }
        };
        self.replace_spline(
            curve,
            Spline {
                form,
                points,
                closed: spline.closed,
            },
        )
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
        if spline.kind() == SplineKind::Control {
            let piece = shape.piece(from, to).ok_or(target)?;
            let control = piece.control();
            let inner = &control[1..control.len() - 1];
            let mut points = vec![start];
            points.extend(self.reuse_points(&spline.points, inner)?);
            points.push(end);
            return Ok(Spline::control(points, piece.knots().to_vec(), false));
        }
        let places = self.places(&spline.points).ok_or(target)?;
        let params = chord_params(&places, spline.closed);
        let at = |id| self.place(id).ok_or(target);
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
            .handles()
            .iter()
            .filter(|handle| ids.contains(&handle.at))
            .copied()
            .collect();
        let ends = [ids[0], ids[ids.len() - 1]];
        let end_places = [new_places[0], new_places[new_places.len() - 1]];
        let tips = end_tips(&shape, &along, &new_places);
        for ((at, from), tip) in ends.into_iter().zip(end_places).zip(tips) {
            if let Some(tip) = tip
                && !spline.points.contains(&at)
            {
                handles.push(self.new_handle(at, from, tip)?);
            }
        }
        Ok(Spline {
            form: SplineForm::Through { handles },
            points: ids,
            closed: false,
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

    /// Makes the spline `curve` `spline`, deleting the points of the one
    /// it was that no curve is made from any more. `Target` if `curve` is
    /// no spline.
    pub(super) fn replace_spline(&mut self, curve: Id, spline: Spline) -> Result<(), EditError> {
        let old: Vec<Id> = self
            .spline(curve)
            .ok_or(EditError::Target(curve))?
            .all_points()
            .collect();
        let entry = self.curve_mut(curve).ok_or(EditError::Target(curve))?;
        entry.curve = Curve::Spline(spline);
        self.delete_unused(&old);
        Ok(())
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
}

/// Where the tips of handles at each of `points` would be for the spline
/// through them, open or `closed`, to keep the shape it has without
/// handles (as [`Sketch::handle_tip`] gives them): what the Spline tool
/// puts at each fit point. `None` for points that make no spline.
pub fn handle_tips(points: &[DVec2], closed: bool) -> Option<Vec<DVec2>> {
    if !SplineKind::Through.admits(points.len(), closed) {
        return None;
    }
    let shape = BSpline::through(points, closed)?;
    let params = chord_params(points, closed);
    points
        .iter()
        .enumerate()
        .map(|(i, &at)| handle_tip_for(&params, closed, i, at, shape.eval(params[i])[1]))
        .collect()
}

/// The spline of `kind` through or by `points`, open or `closed`, as a
/// polyline, as [`Sketch::flatten`] draws one: what the Spline tool
/// shows while it's drawn. `None` for too few points, or points that make
/// no spline (by control points, its knots are [`control_knots`]).
pub fn flatten_spline(points: &[DVec2], kind: SplineKind, closed: bool) -> Option<Vec<DVec2>> {
    if !kind.admits(points.len(), closed) {
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

#[cfg(test)]
mod tests;
