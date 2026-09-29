//! Offsetting a spline alone: its exact offset isn't a spline, so the copy
//! is a spline through fit points along it, as many as it takes to follow
//! it within [`SPLINE_FIT`] of its size, each held as far from the spline
//! as the rest by an offset pair of its own
//! ([`OffsetPair::Spline`](super::OffsetPair::Spline)), so one dimension
//! drives it as it does a chain's copy. Between its fit points the copy
//! is as the interpolation makes it, which is where it strays from the
//! exact offset once the spline is reshaped.

use glam::DVec2;
use varde_expr::Value;

use crate::{
    BSpline, Constraint, Curve, Dimension, EditError, Id, MAX_SPLINE_POINTS, Measure, Side, Sketch,
    Spline, SplineKind,
};

/// How closely a spline's copy follows its exact offset where it's made,
/// as a share of the copy's size: fit points are added until it's within
/// this, or there are [`MAX_SPLINE_POINTS`].
pub(super) const SPLINE_FIT: f64 = 1e-4;

/// How far past bending to a point the copy may come: where the spline
/// turns towards it tighter than the distance, it would fold over itself.
const FOLD_MARGIN: f64 = 1e-3;

/// How many places in each of the spline's segments are looked at for
/// folding, and drawn for the preview.
const SAMPLES: usize = 16;

/// Where in each span between fit points the copy is compared with the
/// exact offset, as shares of the span.
const CHECKS: [f64; 3] = [0.25, 0.5, 0.75];

/// The most rounds of adding fit points: each halves the spans too far
/// off, so from the spline's own segments it gets well past
/// [`MAX_SPLINE_POINTS`] within this many.
const ROUNDS: usize = 12;

/// Where `shape` is at `t`, moved `shift` to the left of the way it runs
/// (right, below zero). `None` where it doesn't move, and has no way.
fn offset_at(shape: &BSpline, t: f64, shift: f64) -> Option<DVec2> {
    let [place, first, _] = shape.eval(t);
    let normal = first.perp().try_normalize()?;
    let at = place + normal * shift;
    at.is_finite().then_some(at)
}

impl Sketch {
    /// The spline `ids` is, if it's a spline alone (perhaps named more
    /// than once): what Offset copies as a spline.
    pub(super) fn lone_spline(&self, ids: &[Id]) -> Option<Id> {
        let &first = ids.first()?;
        let alone = ids.iter().all(|&id| id == first);
        (alone && self.spline(first).is_some()).then_some(first)
    }

    /// The shape of the spline `curve` and how far to the left of it (to
    /// the right, below zero) its copy `distance` to `side` is, once it's
    /// sure the copy doesn't fold: `TooTight` where the spline turns
    /// towards the copy tighter than `distance`, `NothingLeft` for no
    /// distance, `Target` for no spline with a shape.
    fn offset_shape(
        &self,
        curve: Id,
        distance: f64,
        side: Side,
    ) -> Result<(BSpline, f64), EditError> {
        if !(distance > 0.0 && distance.is_finite()) {
            return Err(EditError::NothingLeft);
        }
        let target = EditError::Target(curve);
        let shape = self
            .spline_shape(self.spline(curve).ok_or(target)?)
            .ok_or(target)?;
        let shift = distance * side.sign();
        for t in shape.samples(SAMPLES) {
            let curvature = shape.curvature(t);
            if curvature.is_finite() && curvature * shift >= 1.0 - FOLD_MARGIN {
                return Err(EditError::TooTight);
            }
        }
        Ok((shape, shift))
    }

    /// The places of the fit points of the copy of the spline `curve`
    /// `distance` to `side` of it: on its exact offset, first where its
    /// segments meet, then halfway between those the copy through them
    /// strays further than [`SPLINE_FIT`] of its size from it, until none
    /// do or there are [`MAX_SPLINE_POINTS`]. Refused as
    /// [`Sketch::offset_shape`] refuses.
    pub(super) fn spline_offset(
        &self,
        curve: Id,
        distance: f64,
        side: Side,
    ) -> Result<(Vec<DVec2>, bool), EditError> {
        let (shape, shift) = self.offset_shape(curve, distance, side)?;
        let closed = shape.closed();
        let mut params = shape.breaks();
        if closed {
            params.pop();
        }
        let place = |t: f64| offset_at(&shape, t, shift).ok_or(EditError::NothingLeft);
        let least = SplineKind::Through.least(closed);
        for _ in 0..ROUNDS {
            let places: Vec<DVec2> = params.iter().map(|&t| place(t)).collect::<Result<_, _>>()?;
            if places.len() < least {
                return Err(EditError::NothingLeft);
            }
            let fitted = BSpline::through(&places, closed)
                .ok_or(EditError::NothingLeft)?
                .path();
            let (low, high) = places
                .iter()
                .fold((places[0], places[0]), |(low, high), &p| {
                    (low.min(p), high.max(p))
                });
            let within = SPLINE_FIT * (high - low).max_element().max(distance);
            // The spans the copy strays too far in, worst first, each
            // from a fit point's parameter to the next's, a closed one's
            // last round to 1.
            let spans = if closed {
                params.len()
            } else {
                params.len() - 1
            };
            let span_ends =
                |span: usize| (params[span], params.get(span + 1).copied().unwrap_or(1.0));
            let mut off: Vec<(f64, usize)> = Vec::new();
            for span in 0..spans {
                let (from, to) = span_ends(span);
                let mut worst: f64 = 0.0;
                for share in CHECKS {
                    let exact = place(from + (to - from) * share)?;
                    let fit = fitted.at(fitted.closest(exact));
                    worst = worst.max(fit.distance(exact));
                }
                if worst > within {
                    off.push((worst, span));
                }
            }
            let room = MAX_SPLINE_POINTS.saturating_sub(params.len());
            if off.is_empty() || room == 0 {
                return Ok((places, closed));
            }
            off.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut added: Vec<f64> = off
                .iter()
                .take(room)
                .map(|&(_, span)| {
                    let (from, to) = span_ends(span);
                    (from + to) / 2.0
                })
                .collect();
            params.append(&mut added);
            params.sort_by(f64::total_cmp);
        }
        let places = params.iter().map(|&t| place(t)).collect::<Result<_, _>>()?;
        Ok((places, closed))
    }

    /// The copy of the spline `curve` `distance` to `side`, as a
    /// polyline: its exact offset, as the preview shows it.
    pub(super) fn spline_offset_preview(
        &self,
        curve: Id,
        distance: f64,
        side: Side,
    ) -> Result<Vec<DVec2>, EditError> {
        let (shape, shift) = self.offset_shape(curve, distance, side)?;
        let mut polyline: Vec<DVec2> = shape
            .samples(SAMPLES)
            .into_iter()
            .filter_map(|t| offset_at(&shape, t, shift))
            .collect();
        if shape.closed()
            && let Some(&first) = polyline.first()
        {
            polyline.push(first);
        }
        Ok(polyline)
    }

    /// How far `at` is from the spline `curve` and on which side, left of
    /// the way it runs [`Side::Positive`]: from the place on it nearest.
    pub(super) fn spline_side(&self, curve: Id, at: DVec2) -> Option<(f64, Side)> {
        let shape = self.spline_shape(self.spline(curve)?)?;
        let t = shape.path().closest(at);
        let [place, first, _] = shape.eval(t);
        let distance = place.distance(at);
        distance
            .is_finite()
            .then(|| (distance, Side::of(first.perp_dot(at - place))))
    }

    /// Offsets the spline `curve` alone, see
    /// [`SketchEdit::Offset`](crate::SketchEdit::Offset): its copy through
    /// fit points along its exact offset ([`Sketch::spline_offset`]),
    /// construction if it is, the first held by a driving
    /// [`Measure::Offset`] of `distance`, the rest each by a
    /// [`Constraint::EqualOffset`] to it.
    pub(super) fn offset_spline(
        &mut self,
        curve: Id,
        distance: &Value,
        side: Side,
    ) -> Result<(), EditError> {
        let (places, closed) = self.spline_offset(curve, distance.value, side)?;
        let construction = self
            .curve(curve)
            .ok_or(EditError::Target(curve))?
            .construction;
        let mut points = Vec::with_capacity(places.len());
        for place in places {
            points.push(self.add_point(place)?);
        }
        let copy = Spline::through(points.clone(), closed);
        self.add_curve(Curve::Spline(copy), construction)?;
        let lead = [curve, points[0]];
        for &point in &points[1..] {
            self.add_constraint(Constraint::EqualOffset {
                a: lead,
                b: [curve, point],
            })?;
        }
        let measure = Measure::Offset(curve, points[0]);
        let side = self.side(&measure);
        self.add_dimension(Dimension {
            measure,
            value: distance.clone(),
            driving: true,
            label: DVec2::ZERO,
            side,
        })?;
        Ok(())
    }
}
