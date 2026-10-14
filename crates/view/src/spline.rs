//! What the spline commands do to what's selected: switching splines
//! between their kinds ([`conversions`]) and giving fit points handles or
//! taking them away ([`handles`]), closing curves selected or opening
//! them ([`closings`]), and taking spline points selected out of their
//! splines ([`removals`]). Pure, shared by the keys, the toolbar and the
//! app. When the Spline tool's spline can end or closes is
//! [`Drawing`](crate::Drawing)'s to say.

use std::collections::BTreeSet;

use varde_sketch::{Curve, Id, Selectable, Sketch, SketchEdit, Spline, SplineKind};

use crate::Target;

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

/// The edit dropping the point `point` of `sketch`, dragged and snapped
/// to `target`, makes of its curve, if it closes one: on the other end of
/// an arc it's an end of ([`crate::closing`]), the arc closed
/// ([`SketchEdit::CloseArc`]), as two coincident ends of one would be
/// redundant; likewise an open spline's, not linked, closed with its ends
/// made one ([`SketchEdit::CloseSpline`]) where it keeps enough points to
/// close. `None` for a drop closing nothing.
pub fn close_on_drop(sketch: &Sketch, point: Id, target: Option<Target>) -> Option<SketchEdit> {
    let Some(Target::Point(other)) = target else {
        return None;
    };
    (sketch.curves.iter())
        .filter(|entry| crate::closing(sketch, entry.id, point) == Some(other))
        .find_map(|entry| match &entry.curve {
            Curve::Arc { .. } => Some(SketchEdit::CloseArc(entry.id)),
            // Its ends made one, it must keep enough to close.
            Curve::Spline(spline)
                if spline.points.len() > spline.kind.least(true) && !sketch.is_linked(entry.id) =>
            {
                Some(SketchEdit::CloseSpline(entry.id))
            }
            _ => None,
        })
}

/// The sketch as dropping its point `point`, dragged and snapped to
/// `target`, leaves it where that closes a curve ([`close_on_drop`]): the
/// point at the other end, the curve closed there, with the closed
/// curve's id. For the drag's preview; `None` if the drop closes nothing.
pub(crate) fn closed_by_drop(
    sketch: &Sketch,
    point: Id,
    target: Option<Target>,
) -> Option<(Id, Sketch)> {
    let Some(Target::Point(other)) = target else {
        return None;
    };
    let edit = close_on_drop(sketch, point, target)?;
    let mut closed = sketch.clone();
    closed.point_mut(point)?.at = sketch.point(other)?.at;
    let id = match edit {
        SketchEdit::CloseArc(id) => closed.close_arc(id).map(|()| id),
        SketchEdit::CloseSpline(id) => closed.close_spline(id).map(|()| id),
        _ => return None,
    };
    Some((id.ok()?, closed))
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

/// Taking the spline points among `selection` in `sketch` out of their
/// splines, as one [`SketchEdit::Delete`] of them: `None` if none are
/// selected, else why it can't be, where a point is on a curve that
/// isn't one of the splines it's a point of (deleting it would take that
/// curve too), or a spline would be left with fewer points than its kind
/// needs ([`SplineKind::least`]), which would delete it whole.
pub fn removals(
    sketch: &Sketch,
    selection: &BTreeSet<Selectable>,
) -> Option<Result<SketchEdit, String>> {
    let on_spline = |point: Id| {
        sketch.curves.iter().any(|entry| match &entry.curve {
            Curve::Spline(spline) => spline.points.contains(&point),
            _ => false,
        })
    };
    let points: BTreeSet<Id> = (selection.iter())
        .filter_map(|target| target.item())
        .filter(|&id| on_spline(id))
        .collect();
    if points.is_empty() {
        return None;
    }
    for entry in &sketch.curves {
        let lost = entry
            .curve
            .points()
            .filter(|id| points.contains(id))
            .count();
        if lost == 0 {
            continue;
        }
        let Curve::Spline(spline) = &entry.curve else {
            return Some(Err(
                "A point selected is on a curve that isn't a spline".into()
            ));
        };
        let own = spline
            .points
            .iter()
            .filter(|id| points.contains(id))
            .count();
        if own != lost {
            return Some(Err("A point selected is a spline's handle".into()));
        }
        let least = spline.kind.least(spline.closed);
        if spline.points.len().saturating_sub(lost) < least {
            return Some(Err(match (spline.kind, spline.closed) {
                (_, true) => format!("A closed spline needs {least} points"),
                (SplineKind::Through, false) => {
                    format!("A spline through fit points needs {least}")
                }
                (SplineKind::Control, false) => format!("A spline by control points needs {least}"),
            }));
        }
    }
    Some(Ok(SketchEdit::Delete(points.into_iter().collect())))
}

#[cfg(test)]
mod tests;
