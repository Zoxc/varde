//! What the spline commands do to what's selected: switching splines
//! between their kinds ([`conversions`]) and giving fit points handles or
//! taking them away ([`handles`]), and when the Spline tool's spline can
//! end or closes, and closing curves selected or opening them
//! ([`closings`]). Pure, shared by the keys, the toolbar and the app.

use std::collections::BTreeSet;

use glam::DVec2;
use varde_sketch::{Curve, Id, Selectable, Sketch, SketchEdit, Spline, SplineKind};

use crate::{ActiveTool, SNAP_TOLERANCE, Target, Tool};

impl ActiveTool<'_> {
    /// The kind of spline the Spline tool draws.
    pub fn spline_kind(&self) -> SplineKind {
        if self.control {
            SplineKind::Control
        } else {
            SplineKind::Through
        }
    }

    /// Whether the Spline tool has placed enough points to end its
    /// spline open: two through fit points, four control points.
    pub fn spline_ends(&self) -> bool {
        self.tool == Tool::Spline && self.placed.len() >= self.spline_kind().least(false)
    }

    /// Whether the tool can close what it's drawing without a click on
    /// its first point (the Close key or button): the Spline tool once it
    /// has enough points to close, the Line tool once its chain has two
    /// lines.
    pub fn can_close(&self) -> bool {
        match self.tool {
            Tool::Spline => self.placed.len() >= self.spline_kind().least(true),
            Tool::Line => self.chained >= 2,
            _ => false,
        }
    }

    /// Whether the Spline tool's click at `at`, a pixel being `pixel`
    /// sketch units there, closes its spline: on its first point (within
    /// [`SNAP_TOLERANCE`] pixels of it), once there are enough to close
    /// it, three.
    pub fn closes(&self, at: DVec2, pixel: f64) -> bool {
        self.placed.len() >= self.spline_kind().least(true) && self.on_first(at, pixel)
    }

    /// Whether the Spline tool's click at `at`, a pixel being `pixel`
    /// sketch units there, is on its first point (within
    /// [`SNAP_TOLERANCE`] pixels of it): closing its spline if it has
    /// enough points, else placing nothing, as a point there would close
    /// it on itself.
    pub fn on_first(&self, at: DVec2, pixel: f64) -> bool {
        self.tool == Tool::Spline
            && self
                .placed
                .first()
                .is_some_and(|&first| first.distance(at) <= SNAP_TOLERANCE * pixel)
    }
}

/// The splines among `selection` in `sketch`, and what they are.
fn selected_splines<'a>(
    sketch: &'a Sketch,
    selection: &'a BTreeSet<Selectable>,
) -> impl Iterator<Item = (Id, &'a Spline)> + 'a {
    selection.iter().filter_map(|&target| {
        let id = target.item()?;
        Some((id, sketch.spline(id)?))
    })
}

/// Whether any splines are among `selection` in `sketch`: what the
/// spline commands (converting, handles, the comb) are offered for.
pub fn any_selected(sketch: &Sketch, selection: &BTreeSet<Selectable>) -> bool {
    selected_splines(sketch, selection).next().is_some()
}

/// Switching each spline among `selection` in `sketch` to the other kind:
/// through fit points to by control points, and back. Empty if none are
/// selected.
pub fn conversions(sketch: &Sketch, selection: &BTreeSet<Selectable>) -> Vec<SketchEdit> {
    selected_splines(sketch, selection)
        .map(|(spline, shape)| SketchEdit::Convert {
            spline,
            to: match shape.kind {
                SplineKind::Through => SplineKind::Control,
                SplineKind::Control => SplineKind::Through,
            },
        })
        .collect()
}

/// What giving handles to what's selected in `sketch` does: the fit
/// points among `selection` (of splines through fit points), or failing
/// any, all the fit points of the splines through fit points selected. Those
/// without a handle get one ([`SketchEdit::AddHandles`]); if they all
/// have, their handles go, by deleting the tips. `None` for nothing to
/// give handles to.
pub fn handles(sketch: &Sketch, selection: &BTreeSet<Selectable>) -> Option<SketchEdit> {
    let through = || {
        sketch.curves.iter().filter_map(|entry| match &entry.curve {
            Curve::Spline(spline) if spline.kind == SplineKind::Through => Some((entry.id, spline)),
            _ => None,
        })
    };
    // Each fit point with the splines it's taken on.
    let mut points: Vec<(Id, Vec<Id>)> = selection
        .iter()
        .filter_map(|&target| {
            let point = target.item()?;
            let on: Vec<Id> = through()
                .filter(|(_, spline)| spline.points.contains(&point))
                .map(|(id, _)| id)
                .collect();
            (!on.is_empty()).then_some((point, on))
        })
        .collect();
    if points.is_empty() {
        points = selected_splines(sketch, selection)
            .filter(|(_, spline)| spline.kind == SplineKind::Through)
            .flat_map(|(id, spline)| spline.points.iter().map(move |&point| (point, vec![id])))
            .collect();
    }
    if points.is_empty() {
        return None;
    }
    let handle = |point: Id, spline: Id| {
        let handles = &sketch.spline(spline)?.handles;
        handles.iter().find(|handle| handle.at == point)
    };
    // Each once, as two splines selected may share an end.
    let mut bare: Vec<Id> = Vec::new();
    for (point, on) in &points {
        if on.iter().any(|&spline| handle(*point, spline).is_none()) && !bare.contains(point) {
            bare.push(*point);
        }
    }
    if !bare.is_empty() {
        return Some(SketchEdit::AddHandles(bare));
    }
    let tips: BTreeSet<Id> = points
        .iter()
        .flat_map(|(point, on)| on.iter().filter_map(|&spline| handle(*point, spline)))
        .map(|handle| handle.tip)
        .collect();
    Some(SketchEdit::Delete(tips.into_iter().collect()))
}

/// The sketch as dropping its point `point`, dragged and snapped to
/// `target`, the other end of its arc or spline (see
/// [`crate::closing`]), leaves it: the point at the other end, the curve
/// closed there ([`Sketch::close_arc`], [`Sketch::close_spline`], an
/// open spline only with enough points left to close), with the closed
/// curve's id. For the drag's preview; `None` if the drop closes nothing.
pub fn closed_by_drop(sketch: &Sketch, point: Id, target: Option<Target>) -> Option<(Id, Sketch)> {
    let Some(Target::Point(other)) = target else {
        return None;
    };
    let entry = (sketch.curves.iter())
        .find(|entry| crate::closing(sketch, entry.id, point) == Some(other))?;
    let mut closed = sketch.clone();
    closed.point_mut(point)?.at = sketch.point(other)?.at;
    match &entry.curve {
        Curve::Arc { .. } => closed.close_arc(entry.id).ok()?,
        Curve::Spline(spline) if spline.points.len() > spline.kind.least(true) => {
            closed.close_spline(entry.id).ok()?
        }
        _ => return None,
    }
    Some((entry.id, closed))
}

/// What closing or opening the curves selected in `sketch` does, and
/// whether it closes them: where any open spline or arc among
/// `selection` can be closed, closing each that can
/// ([`SketchEdit::CloseSpline`], [`SketchEdit::CloseArc`]); else opening
/// each closed spline selected, or a closed spline one of whose points is
/// selected, at its point selected or else its first
/// ([`SketchEdit::OpenSpline`]), and each closed arc selected
/// ([`SketchEdit::Detach`] of its point). `None` for nothing to close or
/// open.
pub fn closings(
    sketch: &Sketch,
    selection: &BTreeSet<Selectable>,
) -> Option<(bool, Vec<SketchEdit>)> {
    let items: Vec<Id> = selection
        .iter()
        .filter_map(|target| target.item())
        .collect();
    let closing: Vec<SketchEdit> = (items.iter())
        .filter_map(|&id| match &sketch.curve(id)?.curve {
            Curve::Spline(_) if sketch.spline_closable(id) => Some(SketchEdit::CloseSpline(id)),
            Curve::Arc { .. } if sketch.closable(id) => Some(SketchEdit::CloseArc(id)),
            _ => None,
        })
        .collect();
    if !closing.is_empty() {
        return Some((true, closing));
    }
    let mut opening = Vec::new();
    for (id, spline) in sketch.splines().filter(|(_, spline)| spline.closed) {
        let picked = items.iter().find(|item| spline.points.contains(item));
        let at = match (items.contains(&id), picked) {
            (_, Some(&at)) => at,
            (true, None) => match spline.points.first() {
                Some(&first) => first,
                None => continue,
            },
            (false, None) => continue,
        };
        if sketch.spline_openable(id, at) {
            opening.push(SketchEdit::OpenSpline { spline: id, at });
        }
    }
    for &id in &items {
        if let Some(Curve::Arc { start, end, .. }) = sketch.curve(id).map(|entry| &entry.curve)
            && start == end
            && sketch.detachable(*start)
        {
            opening.push(SketchEdit::Detach(*start));
        }
    }
    (!opening.is_empty()).then_some((false, opening))
}

#[cfg(test)]
mod tests;
