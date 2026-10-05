//! Detaching a point curves share: each curve but the first gets a point
//! of its own at the same place, held there by a coincident constraint,
//! so the joint is one that can be seen and taken off.

use crate::{Constraint, EditError, Id, Sketch};

impl Sketch {
    /// The curves made from the point `point`, in the curves' order.
    fn users(&self, point: Id) -> Vec<Id> {
        (self.curves.iter())
            .filter(|entry| entry.curve.points().any(|id| id == point))
            .map(|entry| entry.id)
            .collect()
    }

    /// Whether the point `point` can be detached: one of the sketch's own
    /// (not the origin nor a link's) that two curves or more are made
    /// from.
    pub fn detachable(&self, point: Id) -> bool {
        self.point(point).is_some()
            && !point.is_builtin()
            && !self.is_linked(point)
            && self.users(point).len() >= 2
    }

    /// Detaches the point `point`, see the module: the curves after the
    /// first made from it each get a new point at its place, coincident
    /// with it; its constraints and dimensions stay on it, with the first
    /// curve. [`EditError::Target`] if it isn't [`detachable`].
    ///
    /// [`detachable`]: Sketch::detachable
    pub(crate) fn detach(&mut self, point: Id) -> Result<(), EditError> {
        if !self.detachable(point) {
            return Err(EditError::Target(point));
        }
        let at = self.point(point).ok_or(EditError::Target(point))?.at;
        for curve in self.users(point).into_iter().skip(1) {
            let own = self.add_point(at)?;
            let entry = self.curve_mut(curve).ok_or(EditError::Target(curve))?;
            let swap = |id| Ok::<_, EditError>(if id == point { own } else { id });
            entry.curve = entry.curve.map_points(swap)?;
            self.add_constraint(Constraint::Coincident(point, own))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
