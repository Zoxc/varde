//! What the view can hover and select in a sketch: its items, and the
//! parts of a spline's handle that aren't items of their own. Never
//! stored: what's made of a handle names its tip
//! ([`Sketch::direction`]).

use glam::DVec2;

use crate::{Id, Sketch, Spline};

/// Something of a sketch's the view can hover and select: an item,
/// built-ins included ([`Id::ORIGIN`], ...), or a part of the handle
/// whose tip is the id that has no id of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Selectable {
    /// A point, curve, constraint, dimension or link, by its id.
    Item(Id),
    /// The handle whose tip this is, as a line: from its tip through its
    /// fit point to as far the other side.
    HandleLine(Id),
    /// The end of the handle whose tip this is the other side of its fit
    /// point, mirroring the tip: no point, but grabbed and shown as one.
    HandleEnd(Id),
}

impl Selectable {
    /// The id it's stored by: the item's, or for a handle's part its
    /// tip's, as what's made of a handle names it.
    pub fn id(self) -> Id {
        match self {
            Selectable::Item(id) | Selectable::HandleLine(id) | Selectable::HandleEnd(id) => id,
        }
    }

    /// The item, if it's one.
    pub fn item(self) -> Option<Id> {
        match self {
            Selectable::Item(id) => Some(id),
            _ => None,
        }
    }

    /// The tip of the handle it's a part of, if it's one.
    pub fn handle_tip(self) -> Option<Id> {
        match self {
            Selectable::Item(_) => None,
            Selectable::HandleLine(tip) | Selectable::HandleEnd(tip) => Some(tip),
        }
    }
}

impl From<Id> for Selectable {
    fn from(id: Id) -> Self {
        Selectable::Item(id)
    }
}

/// An item is its id; a handle's part is none.
impl PartialEq<Id> for Selectable {
    fn eq(&self, id: &Id) -> bool {
        *self == Selectable::Item(*id)
    }
}

impl Sketch {
    /// Whether `target` names something of the sketch's: an item
    /// ([`Sketch::kind`]), or a part of a handle it has.
    pub fn selectable(&self, target: Selectable) -> bool {
        match target {
            Selectable::Item(id) => self.kind(id).is_some(),
            Selectable::HandleLine(tip) | Selectable::HandleEnd(tip) => self.handle(tip).is_some(),
        }
    }

    /// The name of `target` as the user sees it: an item's
    /// ([`Sketch::name`]), "Handle of Spline 1" for a handle as a line,
    /// "End 2 of Handle 2 of Spline 1" for its mirrored end, the
    /// handle numbered among its spline's by its fit point's place
    /// ([`Spline::handle_number`]), as its tip's name has it ("End 1 of
    /// Handle 2 of Spline 1", see [`Sketch::point_name`]).
    pub fn selectable_name(&self, target: Selectable) -> Option<String> {
        match target {
            Selectable::Item(id) => self.name(id),
            Selectable::HandleLine(tip) => {
                let (curve, _) = self.handle(tip)?;
                Some(format!("Handle of {}", self.curve(curve)?.name()))
            }
            Selectable::HandleEnd(tip) => {
                let (curve, _) = self.handle(tip)?;
                let number = self.spline(curve)?.handle_number(tip)?;
                Some(format!(
                    "End 2 of Handle {number} of {}",
                    self.curve(curve)?.name()
                ))
            }
        }
    }

    /// Where the mirrored end of the handle whose tip is `tip` is: as far
    /// the other side of its fit point as the tip.
    pub fn handle_end(&self, tip: Id) -> Option<DVec2> {
        let (at, tip_at) = self.direction(tip)?;
        self.handle(tip)?;
        Some(2.0 * at - tip_at)
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
