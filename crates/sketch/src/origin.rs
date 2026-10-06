//! The sketch's origin and its two axes: built-in items every sketch has,
//! fixed, to snap and constrain to. They aren't stored: they're known by
//! ids of their own ([`Id::ORIGIN`], [`Id::X_AXIS`], [`Id::Y_AXIS`]) at
//! the top of the ids, past any a sketch gives out.
//!
//! [`Sketch::kind`] and [`Sketch::name`] know them, [`Sketch::point`]
//! gives the origin (at zero) and [`Sketch::line`] each axis (from the
//! origin a unit along it), so constraints and dimensions can name them
//! like any point or line; [`Sketch::curve`] doesn't give the axes, which
//! are made from no points of the sketch's. The solver takes them as
//! constants. They're never in the sketch's lists, so they're never
//! deleted, moved, listed or counted, and no curve is made from the
//! origin: a curve's point snapped there is a point of its own, coincident
//! with it.

use glam::DVec2;

use crate::{Constraint, Curve, Id, Measure, Point, Sketch, SplineKind};

/// The lowest of the ids the built-in items take.
pub(crate) const FIRST_BUILTIN: u32 = u32::MAX - 2;

/// The lowest of the ids a sketch never gives out: from here to the
/// built-in items', [`Id::handle`]'s, and the built-in items' own.
pub(crate) const FIRST_RESERVED: u32 = 1 << 31;

/// The ids a sketch gives out end before here (`OutOfIds`,
/// `NextIdReserved` past it): further on, a tip's
/// [`Id::handle`] would be a built-in item's.
pub(crate) const LAST_ID: u32 = FIRST_BUILTIN & !FIRST_RESERVED;

impl Id {
    /// The sketch's origin, a point at zero.
    pub const ORIGIN: Id = Id(u32::MAX);
    /// The sketch's x axis: the endless line through the origin along x.
    pub const X_AXIS: Id = Id(u32::MAX - 1);
    /// The sketch's y axis.
    pub const Y_AXIS: Id = Id(u32::MAX - 2);

    /// Whether it names the origin or an axis, which every sketch has.
    pub fn is_builtin(self) -> bool {
        self.0 >= FIRST_BUILTIN
    }

    /// The id naming the handle whose tip is `tip` as a line, apart from
    /// its tip: for selecting it in the view. No item has it, and nothing
    /// stored names it; what's made of a handle names its tip
    /// ([`Sketch::direction`](crate::Sketch::direction)).
    pub fn handle(tip: Id) -> Id {
        Id(tip.0 | FIRST_RESERVED)
    }

    /// The tip of the handle the id names as a line ([`Id::handle`]), if
    /// it's such an id.
    pub fn handle_tip(self) -> Option<Id> {
        (self.0 >= FIRST_RESERVED && !self.is_builtin()).then_some(Id(self.0 & !FIRST_RESERVED))
    }

    /// Whether it names an axis: an endless line, where a line of the
    /// sketch's ends.
    pub fn is_axis(self) -> bool {
        axis(self).is_some()
    }
}

/// The origin, as [`Sketch::point`] gives it.
pub(crate) static ORIGIN: Point = Point {
    id: Id::ORIGIN,
    number: 0,
    at: DVec2::ZERO,
};

/// The axis `id` names, as a line from the origin a unit along it, if it
/// names one.
pub(crate) fn axis(id: Id) -> Option<(DVec2, DVec2)> {
    match id {
        Id::X_AXIS => Some((DVec2::ZERO, DVec2::X)),
        Id::Y_AXIS => Some((DVec2::ZERO, DVec2::Y)),
        _ => None,
    }
}

impl Sketch {
    /// The name of the point or curve `id`, as the user sees it: "Point 3",
    /// "Arc 1", "Origin", "X axis", or a point by its role in a curve,
    /// see [`Sketch::point_name`].
    pub fn name(&self, id: Id) -> Option<String> {
        match id {
            Id::X_AXIS => Some("X axis".into()),
            Id::Y_AXIS => Some("Y axis".into()),
            _ if let Some(tip) = id.handle_tip() => {
                let (curve, _) = self.handle(tip)?;
                Some(format!("Handle of {}", self.curve(curve)?.name()))
            }
            _ => match self.point(id) {
                Some(point) => Some(self.point_name(point)),
                None => self.curve(id).map(|entry| entry.name()),
            },
        }
    }

    /// The name of `point` as the user sees it: by its role in the first
    /// circle, arc or spline by control points it's one of ("Centre of
    /// Arc 1", "Start of Arc 1", "End of Arc 1", "Control point 2 of
    /// Spline 1"), else [`Point::name`] ("Point 3", "Origin").
    pub fn point_name(&self, point: &Point) -> String {
        let id = point.id;
        let role = self.curves.iter().find_map(|entry| {
            let role = match &entry.curve {
                &Curve::Circle { center, .. } if center == id => "Centre".to_owned(),
                &Curve::Arc { center, .. } if center == id => "Centre".to_owned(),
                &Curve::Arc { start, .. } if start == id => "Start".to_owned(),
                &Curve::Arc { end, .. } if end == id => "End".to_owned(),
                Curve::Spline(spline) if spline.kind == SplineKind::Control => {
                    let at = spline.points.iter().position(|&p| p == id)?;
                    format!("Control point {}", at + 1)
                }
                _ => return None,
            };
            Some(format!("{role} of {}", entry.name()))
        });
        role.unwrap_or_else(|| point.name())
    }
}

impl Constraint {
    /// Whether it uses the built-in items it names as they can be: it
    /// names something of the sketch's own too (a constraint between
    /// built-ins alone, a fix of one included, holds or doesn't whatever
    /// moves), and takes an axis as the endless line it is, not as a
    /// segment: no midpoint of one, nor its length equal to another.
    pub fn fits_builtins(&self) -> bool {
        let own = self.items().any(|(id, _)| !id.is_builtin());
        own && match *self {
            Constraint::Midpoint { line, .. } => !line.is_axis(),
            Constraint::Equal(a, b) => !(a.is_axis() || b.is_axis()),
            _ => true,
        }
    }
}

impl Measure {
    /// Whether it measures the built-in items it names as they can be, as
    /// [`Constraint::fits_builtins`]: not built-ins alone, not an axis's
    /// length, and not from an axis's midpoint (the first of two lines,
    /// see [`Measure::Distance`]) of `sketch`'s.
    pub fn fits_builtins(&self, sketch: &Sketch) -> bool {
        let own = self.items().any(|(id, _)| !id.is_builtin());
        own && match *self {
            Measure::Length(line) => !line.is_axis(),
            Measure::Distance(a, b) => !a.is_axis() || sketch.point(b).is_some(),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests;
