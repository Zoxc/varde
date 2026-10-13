//! Mirroring geometry about a line
//! ([`SketchEdit::Mirror`](crate::SketchEdit::Mirror)).

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec2;

use crate::{Constraint, Corner, Curve, EditError, Id, Sketch, foot};

impl Sketch {
    /// Mirrors the points and curves among `ids` about the line `about`,
    /// see [`SketchEdit::Mirror`](crate::SketchEdit::Mirror), giving the
    /// ids of the constraints holding points on the line there, which are
    /// `auto`. The sketch may be left part way on failure.
    pub(crate) fn mirror(&mut self, ids: &[Id], about: Id) -> Result<Vec<Id>, EditError> {
        let (a, b) = self.line(about).ok_or(EditError::Target(about))?;
        let along = (b - a).try_normalize().ok_or(EditError::Target(about))?;
        let reflect = |p: DVec2| 2.0 * foot(p, a, b) - p;
        let tolerance = self.curve_tolerance();
        let on_line = |p: DVec2| (p - a).perp_dot(along).abs() <= tolerance;
        let own: BTreeSet<Id> = self
            .curve(about)
            .map(|entry| entry.curve.points().collect())
            .unwrap_or_default();

        let ids: BTreeSet<Id> = ids.iter().copied().filter(|&id| id != about).collect();
        let at = |id: Id| self.place(id);
        let arc_points: BTreeSet<Id> = self
            .curves
            .iter()
            .filter(|entry| ids.contains(&entry.id) && matches!(entry.curve, Curve::Arc { .. }))
            .flat_map(|entry| entry.curve.points())
            .collect();
        // A point on the line is its own mirror image, and a curve's there
        // is shared with its copy, but for an arc's, whose copy's radius
        // follows from the symmetry of all three of its points (see the
        // solver's system), and a link's, which no curve of the user's
        // may be made of: those get images, held symmetric. A curve all of
        // whose points are on the line is its own mirror image, and isn't
        // copied.
        let on_axis = |id: Id| !arc_points.contains(&id) && at(id).is_some_and(on_line);
        let shared = |id: Id| on_axis(id) && !self.is_linked(id);
        let (curves, own_images): (Vec<_>, Vec<_>) = self
            .curves
            .iter()
            .filter(|entry| ids.contains(&entry.id))
            .cloned()
            .partition(|entry| !entry.curve.points().all(on_axis));
        let own_images: BTreeSet<Id> = own_images.iter().map(|entry| entry.id).collect();
        let used: BTreeSet<Id> = curves
            .iter()
            .flat_map(|entry| entry.curve.points())
            .collect();
        // Lone points selected off the line, and the curves' points, each
        // once.
        let lone = ids
            .iter()
            .copied()
            .filter(|&id| !id.is_builtin() && self.point(id).is_some() && !shared(id));
        let points: BTreeSet<Id> = lone.chain(used.iter().copied()).collect();

        let placed = points
            .iter()
            .map(|&point| {
                Ok((
                    point,
                    at(point).ok_or(EditError::Target(point))?,
                    shared(point),
                ))
            })
            .collect::<Result<Vec<_>, EditError>>()?;

        let mut images = BTreeMap::new();
        let mut symmetric = Vec::new();
        let mut ties = Vec::new();
        for (point, place, shared) in placed {
            if used.contains(&point) && on_line(place) && !own.contains(&point) {
                ties.push(Constraint::PointOnCurve {
                    point,
                    curve: about,
                });
            }
            let image = if shared {
                point
            } else {
                let image = self.add_point(reflect(place))?;
                symmetric.push(Constraint::Symmetric {
                    a: point,
                    b: image,
                    about,
                });
                image
            };
            images.insert(point, image);
        }
        let image = |id: Id| images.get(&id).copied().ok_or(EditError::Target(id));
        let mut equal = Vec::new();
        let mut copies = BTreeMap::new();
        for entry in &curves {
            let curve = match entry.curve {
                // Mirrored, counter-clockwise runs the other way.
                Curve::Arc { center, start, end } => Curve::Arc {
                    center: image(center)?,
                    start: image(end)?,
                    end: image(start)?,
                },
                // A spline's mirror image is the spline of its points'.
                ref curve => curve.map_points(image)?,
            };
            let copy = self.add_curve(curve, entry.construction)?;
            if let Curve::Circle { .. } = entry.curve {
                equal.push(Constraint::Equal(entry.id, copy));
            }
            copies.insert(entry.id, copy);
        }
        // A fillet or chamfer copied with its lines is a corner of theirs,
        // its equations restating the symmetry, which the solver leaves
        // out (see its system). A line that's its own image, as the line
        // mirrored about is, is its own copy.
        for entry in &curves {
            let Some(corner) = entry.corner else {
                continue;
            };
            let line = |id: Id| {
                copies
                    .get(&id)
                    .copied()
                    .or_else(|| (own_images.contains(&id) || id == about).then_some(id))
            };
            let (Some(a), Some(b), Some(&at), Some(&copy)) = (
                line(corner.a),
                line(corner.b),
                images.get(&corner.at),
                copies.get(&entry.id),
            ) else {
                continue;
            };
            // A mirrored arc runs the other way, from the image of its end,
            // on `b`'s image.
            let (a, b) = match entry.curve {
                Curve::Arc { .. } => (b, a),
                _ => (a, b),
            };
            self.set_corner(copy, Corner { a, b, at, ..corner });
        }
        if curves.is_empty() && symmetric.is_empty() {
            return Err(EditError::NothingToMirror);
        }
        for constraint in symmetric.into_iter().chain(equal) {
            self.add_constraint(constraint)?;
        }
        let mut added = Vec::with_capacity(ties.len());
        for tie in ties {
            if tie.fits_builtins() {
                added.push(self.add_constraint(tie)?);
            }
        }
        Ok(added)
    }
}
