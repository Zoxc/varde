//! What the view can hover and select in a sketch: its items, and a
//! spline's handle as a line, which isn't an item of its own. Never
//! stored: what's made of a handle names its tip
//! ([`Sketch::direction`]).

use crate::{Handle, Id, Sketch, Spline};

/// Something of a sketch's the view can hover and select: an item,
/// built-ins included ([`Id::ORIGIN`], ...), or the handle whose tip is
/// the id as a line, which has no id of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Selectable {
    /// A point, curve, constraint, dimension or link, by its id.
    Item(Id),
    /// The handle whose tip this is, as a line: from its tip through its
    /// fit point to its end.
    HandleLine(Id),
}

impl Selectable {
    /// The id it's stored by: the item's, or for a handle its tip's, as
    /// what's made of a handle names it.
    pub fn id(self) -> Id {
        match self {
            Selectable::Item(id) | Selectable::HandleLine(id) => id,
        }
    }

    /// The item, if it's one.
    pub fn item(self) -> Option<Id> {
        match self {
            Selectable::Item(id) => Some(id),
            _ => None,
        }
    }

    /// The tip of the handle it is, if it's one.
    pub fn handle_tip(self) -> Option<Id> {
        match self {
            Selectable::Item(_) => None,
            Selectable::HandleLine(tip) => Some(tip),
        }
    }
}

impl From<Id> for Selectable {
    fn from(id: Id) -> Self {
        Selectable::Item(id)
    }
}

/// An item is its id; a handle is none.
impl PartialEq<Id> for Selectable {
    fn eq(&self, id: &Id) -> bool {
        *self == Selectable::Item(*id)
    }
}

impl Sketch {
    /// Whether `target` names something of the sketch's: an item
    /// ([`Sketch::kind`]), or a handle it has.
    pub fn selectable(&self, target: Selectable) -> bool {
        match target {
            Selectable::Item(id) => self.kind(id).is_some(),
            Selectable::HandleLine(tip) => self.handle(tip).is_some(),
        }
    }

    /// The name of `target` as the user sees it: an item's
    /// ([`Sketch::name`]), or "Handle of Spline 1" for a handle as a
    /// line. Its tip and end are points, "End 1 of Handle 2 of Spline 1"
    /// and "End 2 of ..." ([`Sketch::point_name`]), the handle numbered
    /// among its spline's by its fit point's place
    /// ([`Spline::handle_number`]).
    pub fn selectable_name(&self, target: Selectable) -> Option<String> {
        match target {
            Selectable::Item(id) => self.name(id),
            Selectable::HandleLine(tip) => {
                let (curve, _) = self.handle(tip)?;
                Some(format!("Handle of {}", self.curve(curve)?.name()))
            }
        }
    }

    /// The handle whose end (not its tip) is `end`, with its spline.
    pub fn handle_by_end(&self, end: Id) -> Option<(Id, Handle)> {
        self.splines().find_map(|(id, spline)| {
            let handle = spline.handles.iter().find(|handle| handle.end == end)?;
            Some((id, *handle))
        })
    }
}

impl Spline {
    /// The number of its handle whose tip is `tip`, from 1, in the order
    /// of their fit points along it.
    pub fn handle_number(&self, tip: Id) -> Option<usize> {
        let handle = self.handles.iter().find(|handle| handle.tip == tip)?;
        let before = (self.points.iter())
            .take_while(|&&point| point != handle.at)
            .filter(|&&point| self.has_handle(point))
            .count();
        Some(before + 1)
    }
}
