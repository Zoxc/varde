//! The shape tools: trimming a curve back to the curves cutting it
//! ([`SketchEdit::Trim`](crate::SketchEdit::Trim)), extending a line, an
//! arc or a spline to the next curve it meets
//! ([`SketchEdit::Extend`](crate::SketchEdit::Extend)) and mirroring
//! geometry about a line ([`SketchEdit::Mirror`](crate::SketchEdit::Mirror)),
//! with what the view shows of them before they're made
//! ([`Sketch::trim_piece`], [`Sketch::extension`]).
//!
//! Every curve cuts, construction curves too, as in other CAD, the origin's
//! axes not. Where curves meet is [`meet`], with the tolerance profiles
//! use, of the sketch's size. What the tools tie their new ends to is
//! `auto` (see [`Add::auto`](crate::Add::auto)): kept if it holds with the
//! rest, dropped if it restates or contradicts it.

use std::collections::BTreeSet;

use crate::intersect::{Geom, meet};
use crate::{Constraint, Corner, Curve, EditError, Id, Kind, Measure, OutOfIds, Sketch};

mod extend;
mod mirror;
#[cfg(test)]
mod tests;
mod trim;

/// Where other curves cut a curve, trimming or extending it.
#[derive(Debug, Clone, PartialEq)]
struct Cut {
    /// The parameter along the curve cut (see [`Geom`]).
    u: f64,
    /// The curves cutting it there.
    by: Vec<Id>,
    /// An end of one of those curves that's there, if one is: a curve cut
    /// there ends at that point itself.
    end: Option<Id>,
}

impl Cut {
    /// Takes in a place found within tolerance of this one: each cutting
    /// curve once, the first end found kept.
    fn absorb(&mut self, by: impl IntoIterator<Item = Id>, end: Option<Id>) {
        for by in by {
            if !self.by.contains(&by) {
                self.by.push(by);
            }
        }
        self.end = self.end.or(end);
    }
}

impl Sketch {
    /// Where the curves but `skip` meet `geom`, within `tolerance`, in
    /// order along it: each place where one or more do. `None` where a
    /// spline lies along another so closely that not every place is
    /// found ([`meet`] runs out of steps).
    fn cuts(&self, geom: &Geom, skip: Id, tolerance: f64) -> Option<Vec<Cut>> {
        let mut found = Vec::new();
        let mut places = Vec::new();
        for entry in self.curves.iter().filter(|entry| entry.id != skip) {
            let Some(other) = Geom::of(self, &entry.curve) else {
                continue;
            };
            found.clear();
            if meet(geom, &other, tolerance, &mut found) == usize::MAX {
                return None;
            }
            let ends = entry.curve.ends();
            for &(u, v) in &found {
                let end = ends.and_then(|[start, end]| {
                    if other.length(0.0, v) <= tolerance {
                        Some(start)
                    } else if other.length(v, other.last()) <= tolerance {
                        Some(end)
                    } else {
                        None
                    }
                });
                places.push((u, entry.id, end));
            }
        }
        places.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut cuts: Vec<Cut> = Vec::new();
        for (u, by, end) in places {
            match cuts.last_mut() {
                Some(cut) if geom.length(cut.u, u) <= tolerance => cut.absorb([by], end),
                _ => cuts.push(Cut {
                    u,
                    by: vec![by],
                    end,
                }),
            }
        }
        // Round a circle, the last place and the first may be one.
        if !geom.has_ends()
            && cuts.len() > 1
            && let (Some(first), Some(last)) = (cuts.first(), cuts.last())
            && geom.length(last.u, first.u + geom.last()) <= tolerance
            && let Some(last) = cuts.pop()
        {
            cuts[0].absorb(last.by, last.end);
        }
        Some(cuts)
    }

    /// The point a curve cut at `cut` of `geom` ends at: the end of a
    /// curve cutting it that's there, unless it's one of `own`, or else a
    /// new one, tied to the curves cutting it there by `ties`. A link's
    /// end is never the user's curve's own (see [`Link`](crate::Link)): a
    /// new point is tied coincident with it.
    fn cut_point(
        &mut self,
        geom: &Geom,
        cut: &Cut,
        own: &BTreeSet<Id>,
        ties: &mut Vec<Constraint>,
    ) -> Result<Id, OutOfIds> {
        if let Some(end) = cut.end.filter(|end| !own.contains(end)) {
            if !self.is_linked(end) {
                return Ok(end);
            }
            let point = self.add_point(geom.at(cut.u))?;
            ties.push(Constraint::Coincident(point, end));
            return Ok(point);
        }
        let point = self.add_point(geom.at(cut.u))?;
        ties.extend(
            cut.by
                .iter()
                .map(|&curve| Constraint::PointOnCurve { point, curve }),
        );
        Ok(point)
    }

    /// The points of the fillets and chamfers on the line `line`.
    fn corner_points(&self, line: Id) -> BTreeSet<Id> {
        let on = |corner: &Corner| corner.a == line || corner.b == line;
        self.curves
            .iter()
            .filter(|entry| entry.corner.as_ref().is_some_and(on))
            .flat_map(|entry| entry.curve.points())
            .collect()
    }

    /// Makes the line or arc `curve` end at `new` where it ended at `old`.
    fn set_end(&mut self, curve: Id, old: Id, new: Id) -> Result<(), EditError> {
        let entry = self.curve_mut(curve).ok_or(EditError::Target(curve))?;
        match &mut entry.curve {
            Curve::Line { start, end } | Curve::Arc { start, end, .. } => {
                for point in [start, end] {
                    if *point == old {
                        *point = new;
                        return Ok(());
                    }
                }
                Err(EditError::Target(old))
            }
            Curve::Circle { .. } | Curve::Spline(_) => Err(EditError::Target(curve)),
        }
    }

    /// Hands what's on `curve` at its end `end`, now `rest`'s, to `rest`:
    /// the tangents with a curve `end` is an end of too, which touch there
    /// (a spline's, and smooth joins, where they're at `end`), and the
    /// fillets and chamfers on the corner there.
    fn hand_over(&mut self, curve: Id, rest: Id, end: Id) {
        for entry in &mut self.curves {
            if let Some(corner) = &mut entry.corner
                && corner.at == end
            {
                for line in [&mut corner.a, &mut corner.b] {
                    if *line == curve {
                        *line = rest;
                    }
                }
            }
        }
        let ends_at = |sketch: &Sketch, other: Id| {
            let ends = sketch.curve(other).and_then(|entry| entry.curve.ends());
            ends.is_some_and(|ends| ends.contains(&end))
        };
        let moved: Vec<(usize, Constraint)> = self
            .constraints
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let mut constraint = entry.constraint.clone();
                // With a spline, where it touches is named.
                let (a, b, at) = match &mut constraint {
                    Constraint::Tangent { a, b, at, .. } => (a, b, *at),
                    Constraint::Smooth { a, b, at, .. } => (a, b, Some(*at)),
                    _ => return None,
                };
                let touches = |other: Id| at.map_or_else(|| ends_at(self, other), |at| at == end);
                if *a == curve && touches(*b) {
                    *a = rest;
                } else if *b == curve && touches(*a) {
                    *b = rest;
                } else {
                    return None;
                }
                Some((index, constraint))
            })
            .collect();
        for (index, constraint) in moved {
            self.constraints[index].constraint = constraint;
        }
    }

    /// Drops what held the length of `line`, which trimming or extending
    /// changes: an equal length, a midpoint, a driving length, or
    /// distance from its midpoint to another line. What's on the endless
    /// line through it (a point on it, parallel, an angle) still holds.
    fn reshaped(&mut self, line: Id) {
        self.constraints.retain(|entry| match entry.constraint {
            Constraint::Equal(a, b) => a != line && b != line,
            Constraint::Midpoint { line: on, .. } => on != line,
            _ => true,
        });
        let lines: BTreeSet<Id> = self
            .dimensions
            .iter()
            .filter_map(|entry| match entry.dimension.measure {
                Measure::Distance(a, b) if a == line => Some(b),
                _ => None,
            })
            .filter(|&b| self.line(b).is_some())
            .collect();
        self.dimensions.retain(|entry| {
            !entry.dimension.driving
                || match entry.dimension.measure {
                    Measure::Length(of) => of != line,
                    Measure::Distance(a, b) => a != line || !lines.contains(&b),
                    _ => true,
                }
        });
    }

    /// Ends a trim or an extension of `curve`: what held its length goes if
    /// it's a line ([`Sketch::reshaped`]), and the fillets and chamfers on
    /// corners it no longer makes; the points of `gone` no curve is made
    /// from any more are deleted, with what's on them; constraints and
    /// dimensions naming a curve and one of its own points, which the
    /// shape's new ends can make of a point on it, are dropped; and `ties`
    /// are added, giving their ids.
    fn finish_shape(
        &mut self,
        curve: Id,
        gone: &[Id],
        ties: Vec<Constraint>,
    ) -> Result<Vec<Id>, EditError> {
        if self.kind(curve) == Some(Kind::Line) {
            self.reshaped(curve);
        }
        self.drop_broken_corners();
        self.delete_unused(gone);
        self.drop_annotations(|sketch, annotation| annotation.own_point(sketch).is_some());
        let mut added = Vec::with_capacity(ties.len());
        for tie in ties {
            // A tie with a curve's own point, joining its end to itself,
            // or with a fillet or chamfer gone with its corner.
            let there = tie.items().all(|(id, _)| self.kind(id).is_some());
            if there && tie.own_point(self).is_none() {
                added.push(self.add_constraint(tie)?);
            }
        }
        Ok(added)
    }
}
