//! Extending a line, an arc or a spline to the next curve it meets
//! ([`SketchEdit::Extend`](crate::SketchEdit::Extend)).

use std::f64::consts::TAU;

use glam::DVec2;

use super::*;
use crate::intersect::Geom;
use crate::{
    BSpline, Constraint, Curve, EditError, Id, Sketch, SplineForm, SplineKind, control_knots,
};

/// Where extending a line or an arc takes an end: the curve it runs on
/// from the end on (see [`Sketch::ahead`]) and the cut there.
#[derive(Debug, Clone, PartialEq)]
struct Reached {
    ahead: Geom,
    /// The parameter of the end on `ahead`.
    from: f64,
    cut: Cut,
}

impl Sketch {
    /// The end of the line, arc or open spline `curve` nearer `at`, its
    /// end where they're as near. `None` for anything else.
    pub fn nearer_end(&self, curve: Id, at: DVec2) -> Option<Id> {
        let [start, end] = self.curve(curve)?.curve.ends()?;
        let distance = |id| self.point(id).map(|point| point.at.distance(at));
        Some(if distance(start)? < distance(end)? {
            start
        } else {
            end
        })
    }

    /// The curve the line, arc or open spline `curve` runs on past its
    /// end `end`, and the parameter of `end` on it: a line on for `reach`,
    /// an arc round the rest of its circle, which runs counter-clockwise
    /// from its end to its start, a spline along its tangent there for
    /// `reach`. `Target` if `curve` is none of those, has no size, or `end`
    /// is no end of it.
    fn ahead(&self, curve: Id, end: Id, reach: f64) -> Result<(Geom, f64), EditError> {
        let entry = self.curve(curve).ok_or(EditError::Target(curve))?;
        let geom = Geom::of(self, &entry.curve).ok_or(EditError::Target(curve))?;
        let [first, last] = entry.curve.ends().ok_or(EditError::Target(curve))?;
        if end != first && end != last {
            return Err(EditError::Target(end));
        }
        Ok(match geom {
            Geom::Segment { start, end: finish } => {
                let (from, to) = if end == last {
                    (start, finish)
                } else {
                    (finish, start)
                };
                let way = (to - from).normalize();
                let ahead = Geom::Segment {
                    start: to,
                    end: to + way * reach,
                };
                (ahead, 0.0)
            }
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                let ahead = Geom::Round {
                    center,
                    radius,
                    begin: begin + sweep,
                    sweep: TAU - sweep,
                };
                (ahead, if end == last { 0.0 } else { TAU - sweep })
            }
            // On along its tangent at the end, which the spline then
            // curves into (see `Sketch::extend_spline`).
            Geom::Spline(path) => {
                let (u, forward) = if end == last {
                    (path.last(), true)
                } else {
                    (0.0, false)
                };
                let from = path.at(u);
                let ahead = Geom::Segment {
                    start: from,
                    end: from + path.heading(u, forward).0 * reach,
                };
                (ahead, 0.0)
            }
        })
    }

    /// Where extending `end` of `curve` takes it, see [`Sketch::ahead`]:
    /// the first place past it where another curve meets it.
    /// `NothingAhead` if none does.
    fn reached(&self, curve: Id, end: Id, reach: f64) -> Result<Reached, EditError> {
        let (ahead, from) = self.ahead(curve, end, reach)?;
        let tolerance = self.curve_tolerance();
        let cuts = (self.cuts(&ahead, curve, tolerance)).ok_or(EditError::TooComplex)?;
        // Not where it is, nor, round an arc, back at its other end.
        let far = if from == 0.0 { ahead.last() } else { 0.0 };
        let cut = cuts
            .into_iter()
            .filter(|cut| {
                ahead.length(from, cut.u) > tolerance && ahead.length(cut.u, far) > tolerance
            })
            .min_by(|a, b| {
                let (a, b) = (ahead.length(from, a.u), ahead.length(from, b.u));
                a.total_cmp(&b)
            })
            .ok_or(EditError::NothingAhead)?;
        Ok(Reached { ahead, from, cut })
    }

    /// What extending `end` of the line, arc or spline `curve` adds to it
    /// (see [`SketchEdit::Extend`](crate::SketchEdit::Extend)), as a
    /// polyline from the end to where it would reach, a line or a spline
    /// looking at most `reach` ahead: the spline as it would be past its
    /// old end. `None` if it can't be extended.
    pub fn extension(&self, curve: Id, end: Id, reach: f64) -> Option<Vec<DVec2>> {
        if self.spline(curve).is_some() {
            let [_, last] = self.curve(curve)?.curve.ends()?;
            let old = self.point(end)?.at;
            let mut extended = self.clone();
            extended.extend(curve, end, reach).ok()?;
            let geom = Geom::of(&extended, &extended.curve(curve)?.curve)?;
            // Past its end, or before its start.
            let far = if end == last { geom.last() } else { 0.0 };
            return Some(geom.polyline(geom.closest(old), far));
        }
        let reached = self.reached(curve, end, reach).ok()?;
        Some(reached.ahead.polyline(reached.from, reached.cut.u))
    }

    /// Extends `end` of the line, arc or spline `curve` to the next curve
    /// it meets, a line or a spline looking at most `reach` ahead, see
    /// [`SketchEdit::Extend`](crate::SketchEdit::Extend), giving the ids
    /// of the constraints tying its new end, which are `auto`. The sketch
    /// may be left part way on failure.
    pub(crate) fn extend(&mut self, curve: Id, end: Id, reach: f64) -> Result<Vec<Id>, EditError> {
        if self.is_corner(curve) {
            return Err(EditError::Target(curve));
        }
        let reached = self.reached(curve, end, reach)?;
        let own = self.corner_points(curve);
        let mut ties = Vec::new();
        let new = self.cut_point(&reached.ahead, &reached.cut, &own, &mut ties)?;
        if self.spline(curve).is_some() {
            self.extend_spline(curve, end, new)?;
            return self.finish_shape(curve, &[], ties);
        }
        self.set_end(curve, end, new)?;
        self.finish_shape(curve, &[end], ties)
    }

    /// Makes the spline `curve` end at `new` past its end `end`, which it
    /// then runs on through, curving into the way to `new` smoothly:
    /// through fit points `new` a fit point more, by control points a
    /// control point more, its knots squeezed into the part of its
    /// parameter the old polygon's length takes and one more where the
    /// extension starts. What held it tangent or smooth at `end` goes, as
    /// it no longer ends there.
    fn extend_spline(&mut self, curve: Id, end: Id, new: Id) -> Result<(), EditError> {
        let target = EditError::Target(curve);
        let mut spline = self.spline(curve).ok_or(target)?.clone();
        let [_, last] = spline.ends().ok_or(target)?;
        let at_end = end == last;
        if at_end {
            spline.points.push(new);
        } else {
            spline.points.insert(0, new);
        }
        if spline.kind() == SplineKind::Control {
            let places = self.places(&spline.points).ok_or(target)?;
            let (old, reach) = if at_end {
                let old = &places[..places.len() - 1];
                (old, places[places.len() - 1].distance(old[old.len() - 1]))
            } else {
                (&places[1..], places[0].distance(places[1]))
            };
            let length: f64 = old.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
            let share = length / (length + reach);
            let knots: Vec<f64> = if at_end {
                let old = spline.knots().iter().map(|&k| k * share);
                old.chain([share]).collect()
            } else {
                let start = 1.0 - share;
                let old = spline.knots().iter().map(|&k| start + k * share);
                std::iter::once(start).chain(old).collect()
            };
            let sound = BSpline::clamped(&knots, places.clone()).is_some();
            let knots = if sound {
                knots
            } else {
                control_knots(&places, false)
            };
            spline.form = SplineForm::Control { knots };
        }
        self.curve_mut(curve).ok_or(target)?.curve = Curve::Spline(spline);
        self.constraints.retain(|entry| match entry.constraint {
            Constraint::Tangent {
                a, b, at: Some(at), ..
            }
            | Constraint::Smooth { a, b, at, .. } => at != end || (a != curve && b != curve),
            _ => true,
        });
        Ok(())
    }
}
