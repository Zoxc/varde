//! Closing an arc: its end made its start, so it runs all the way round,
//! an arc still but a circle with a point on it.

use std::collections::HashSet;

use crate::{Curve, EditError, Id, Sketch};

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
    pub(crate) fn close_arc(&mut self, arc: Id) -> Result<(), EditError> {
        if !self.closable(arc) {
            return Err(EditError::Target(arc));
        }
        let Some(&Curve::Arc { start, end, .. }) = self.curve(arc).map(|entry| &entry.curve) else {
            return Err(EditError::Target(arc));
        };
        let on_end: Vec<Id> = (self.constraints.iter())
            .filter(|entry| entry.constraint.items().any(|(id, _)| id == end))
            .map(|entry| entry.id)
            .chain(
                (self.dimensions.iter())
                    .filter(|entry| entry.dimension.measure.items().any(|(id, _)| id == end))
                    .map(|entry| entry.id),
            )
            .collect();
        self.delete(&on_end);
        let swap = |id| Ok::<_, EditError>(if id == end { start } else { id });
        for entry in &mut self.curves {
            if entry.curve.points().any(|id| id == end) {
                entry.curve = entry.curve.map_points(swap)?;
            }
        }
        self.points.retain(|point| point.id != end);
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
        Ok(())
    }
}

#[cfg(test)]
mod tests;
