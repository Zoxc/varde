//! What the spline commands do to what's selected: switching splines
//! between their kinds ([`conversions`]) and giving fit points handles or
//! taking them away ([`handles`]), and when the Spline tool's spline can
//! end or closes. Pure, shared by the keys, the toolbar and the app.

use std::collections::BTreeSet;

use glam::DVec2;
use varde_sketch::{Curve, Id, Sketch, SketchEdit, Spline, SplineKind};

use crate::{ActiveTool, SNAP_TOLERANCE, Tool};

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
    selection: &'a BTreeSet<Id>,
) -> impl Iterator<Item = (Id, &'a Spline)> + 'a {
    selection
        .iter()
        .filter_map(|&id| Some((id, sketch.spline(id)?)))
}

/// Whether any splines are among `selection` in `sketch`: what the
/// spline commands (converting, handles, the comb) are offered for.
pub fn any_selected(sketch: &Sketch, selection: &BTreeSet<Id>) -> bool {
    selected_splines(sketch, selection).next().is_some()
}

/// Switching each spline among `selection` in `sketch` to the other kind:
/// through fit points to by control points, and back. Empty if none are
/// selected.
pub fn conversions(sketch: &Sketch, selection: &BTreeSet<Id>) -> Vec<SketchEdit> {
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
pub fn handles(sketch: &Sketch, selection: &BTreeSet<Id>) -> Option<SketchEdit> {
    let through = || {
        sketch.curves.iter().filter_map(|entry| match &entry.curve {
            Curve::Spline(spline) if spline.kind == SplineKind::Through => Some((entry.id, spline)),
            _ => None,
        })
    };
    // Each fit point with the splines it's taken on.
    let mut points: Vec<(Id, Vec<Id>)> = selection
        .iter()
        .filter_map(|&point| {
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

#[cfg(test)]
mod tests;
