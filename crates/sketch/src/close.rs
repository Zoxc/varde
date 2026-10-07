//! Closing an arc: its end made its start, so it runs all the way round,
//! an arc still but a circle with a point on it. Closing a spline: it
//! runs on from its last point round to its first, its ends one point
//! where they meet; and opening a closed one at one of its points.

use std::collections::HashSet;

use glam::DVec2;

use crate::{Constraint, Curve, EditError, Id, Sketch, Spline, SplineKind, control_knots};

impl Sketch {
    /// Whether the arc `arc` can be closed: an open arc of the sketch's
    /// own, no fillet, whose end is no link's.
    pub fn closable(&self, arc: Id) -> bool {
        self.curve(arc).is_some_and(|entry| {
            entry.corner.is_none()
                && matches!(entry.curve, Curve::Arc { start, end, .. } if start != end)
        }) && !self.is_linked(arc)
    }

    /// Closes the arc `arc`, see the module: its end is its start, which
    /// the other curves made from the end are made from too, and the end
    /// goes, with the constraints and dimensions on it and those that no
    /// longer fit (a tangent at the end, say). [`EditError::Target`] if it
    /// isn't [`closable`](Sketch::closable).
    pub fn close_arc(&mut self, arc: Id) -> Result<(), EditError> {
        if !self.closable(arc) {
            return Err(EditError::Target(arc));
        }
        let Some(&Curve::Arc { start, end, .. }) = self.curve(arc).map(|entry| &entry.curve) else {
            return Err(EditError::Target(arc));
        };
        self.merge_into(end, start)?;
        Ok(())
    }

    /// Makes the point `gone` the point `kept` in every curve made from
    /// it, `gone` going with the constraints and dimensions on it, then
    /// those that no longer fit (a tangent at an end that's no longer
    /// one, say).
    fn merge_into(&mut self, gone: Id, kept: Id) -> Result<(), EditError> {
        let on_end: Vec<Id> = (self.constraints.iter())
            .filter(|entry| entry.constraint.items().any(|(id, _)| id == gone))
            .map(|entry| entry.id)
            .chain(
                (self.dimensions.iter())
                    .filter(|entry| entry.dimension.measure.items().any(|(id, _)| id == gone))
                    .map(|entry| entry.id),
            )
            .collect();
        self.delete(&on_end);
        let swap = |id| Ok::<_, EditError>(if id == gone { kept } else { id });
        for entry in &mut self.curves {
            if entry.curve.points().any(|id| id == gone) {
                entry.curve = entry.curve.map_points(swap)?;
            }
        }
        self.points.retain(|point| point.id != gone);
        self.drop_unfit();
        Ok(())
    }

    /// Deletes the constraints and dimensions that no longer fit.
    fn drop_unfit(&mut self) {
        let tips = self.tips();
        let unfit: Vec<Id> = (self.constraints.iter())
            .filter(|entry| !entry.constraint.fits(self))
            .map(|entry| entry.id)
            .chain(
                (self.dimensions.iter())
                    .filter(|entry| !entry.dimension.measure.fits(self, &tips))
                    .map(|entry| entry.id),
            )
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        self.delete(&unfit);
    }

    /// Whether the open spline `curve`'s ends meet, so closing it makes
    /// them one point: a coincident ties them, or they're at the same
    /// place.
    fn ends_meet(&self, curve: Id) -> bool {
        let Some([first, last]) = self.spline(curve).and_then(Spline::ends) else {
            return false;
        };
        let tied = self.constraints.iter().any(|entry| {
            matches!(entry.constraint, Constraint::Coincident(a, b)
                if (a == first && b == last) || (a == last && b == first))
        });
        tied || (self.point(first).zip(self.point(last)))
            .is_some_and(|(a, b)| same_place(a.at, b.at))
    }

    /// Whether the spline `curve` can be closed: an open spline of the
    /// sketch's own with points enough to close, after its ends are made
    /// one where they meet.
    pub fn spline_closable(&self, curve: Id) -> bool {
        let Some(spline) = self.spline(curve) else {
            return false;
        };
        let count = spline.points.len() - usize::from(self.ends_meet(curve));
        !spline.closed && count >= spline.kind.least(true) && !self.is_linked(curve)
    }

    /// Closes the spline `curve`, see the module: it runs on from its
    /// last point round to its first. Where its ends meet
    /// ([`Sketch::ends_meet`]) the last is merged into the first, as an
    /// arc's end is, its handle going to the first if it has none. By
    /// control points its knots are found anew ([`control_knots`]).
    /// [`EditError::Target`] if it isn't
    /// [`spline_closable`](Sketch::spline_closable).
    pub fn close_spline(&mut self, curve: Id) -> Result<(), EditError> {
        if !self.spline_closable(curve) {
            return Err(EditError::Target(curve));
        }
        let merge = self.ends_meet(curve);
        let mut spline = self.spline(curve).ok_or(EditError::Target(curve))?.clone();
        let [first, last] = spline.ends().ok_or(EditError::Target(curve))?;
        let mut unused = Vec::new();
        if merge {
            spline.points.pop();
            let first_handled = spline.has_handle(first);
            if let Some(handle) = spline.handles.iter_mut().find(|handle| handle.at == last) {
                if first_handled {
                    unused.extend(handle.arms());
                } else {
                    handle.at = first;
                }
            }
            spline.handles.retain(|handle| handle.at != last);
        }
        spline.closed = true;
        if spline.kind == SplineKind::Control {
            let places = self
                .places(&spline.points)
                .ok_or(EditError::Target(curve))?;
            spline.knots = control_knots(&places, true);
        }
        self.curve_mut(curve).ok_or(EditError::Target(curve))?.curve = Curve::Spline(spline);
        self.delete(&unused);
        if merge {
            self.merge_into(last, first)?;
        } else {
            self.drop_unfit();
        }
        Ok(())
    }

    /// Whether the spline `curve` can be opened at its point `at`: a
    /// closed spline of the sketch's own, one of whose fit or control
    /// points `at` is.
    pub fn spline_openable(&self, curve: Id, at: Id) -> bool {
        self.spline(curve)
            .is_some_and(|spline| spline.closed && spline.points.contains(&at))
            && !self.is_linked(curve)
    }

    /// Opens the closed spline `curve` at its point `at`: it starts
    /// there, and ends at a new point at the same place, tied to
    /// nothing. By control points its knots are found anew
    /// ([`control_knots`]). [`EditError::Target`] if it isn't
    /// [`spline_openable`](Sketch::spline_openable) there.
    pub(crate) fn open_spline(&mut self, curve: Id, at: Id) -> Result<(), EditError> {
        if !self.spline_openable(curve, at) {
            return Err(EditError::Target(curve));
        }
        let place = self.point(at).ok_or(EditError::Target(at))?.at;
        let mut spline = self.spline(curve).ok_or(EditError::Target(curve))?.clone();
        let index = (spline.points.iter())
            .position(|&id| id == at)
            .ok_or(EditError::Target(at))?;
        spline.points.rotate_left(index);
        spline.points.push(self.add_point(place)?);
        spline.closed = false;
        if spline.kind == SplineKind::Control {
            let places = self
                .places(&spline.points)
                .ok_or(EditError::Target(curve))?;
            spline.knots = control_knots(&places, false);
        }
        self.curve_mut(curve).ok_or(EditError::Target(curve))?.curve = Curve::Spline(spline);
        self.drop_unfit();
        Ok(())
    }
}

/// Whether two places are the same but for rounding, or a drag's solve
/// that reached one from the other: within a millionth of how far they
/// are from the origin (or of a unit).
fn same_place(a: DVec2, b: DVec2) -> bool {
    a.distance(b) <= 1e-6 * (1.0 + a.abs().max_element().max(b.abs().max_element()))
}

#[cfg(test)]
mod tests;
