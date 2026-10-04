//! The list of what overlaps where the left button was held still in the
//! viewport (see `varde_view::Overlaps`): hovering a row highlights its
//! item, choosing one selects it as a click on it would, and closing it
//! leaves the selection as it was.

use varde_view::{Look, OverlapItems, Overlaps};

use super::Doc;

/// The list open, and the row of it hovered.
#[derive(Debug)]
pub(crate) struct Listed {
    pub(crate) list: Overlaps,
    hovered: Option<usize>,
}

impl Doc {
    /// Opens `list`, in a sketch only of its items, outside one only of
    /// the model shown while the cursor picks it. Nothing's hovered till
    /// a row is.
    pub(crate) fn open_overlaps(&mut self, list: Overlaps) {
        let fits = match &list.items {
            OverlapItems::Sketch(_) => self.sketch.is_some(),
            OverlapItems::Model(picks) => {
                self.picks() && picks.iter().all(|pick| pick.model == self.feed.model())
            }
        };
        if !fits || list.items.is_empty() {
            return;
        }
        self.unhover_overlap();
        self.overlaps = Some(Listed {
            list,
            hovered: None,
        });
    }

    /// Hovers the list's row `row`, or none: its item is highlighted as
    /// one hovered in the viewport is.
    pub(crate) fn hover_overlap(&mut self, row: Option<usize>) {
        let Some(listed) = &mut self.overlaps else {
            return;
        };
        let row = row.filter(|&row| row < listed.list.items.len());
        listed.hovered = row;
        let (sketch, model) = match (&listed.list.items, row) {
            (OverlapItems::Sketch(ids), Some(row)) => (Some(ids[row]), None),
            (OverlapItems::Model(picks), Some(row)) => (None, Some(picks[row])),
            (_, None) => return self.unhover_overlap(),
        };
        if sketch.is_some() {
            self.hover_item(sketch);
        } else {
            self.hover(model);
        }
    }

    /// The cursor left the list's row `row`: nothing's hovered, unless
    /// another row already is.
    pub(crate) fn leave_overlap(&mut self, row: usize) {
        if self
            .overlaps
            .as_ref()
            .is_some_and(|listed| listed.hovered == Some(row))
        {
            self.hover_overlap(None);
        }
    }

    /// Selects the item of the list's row `row` as a click on it would:
    /// alone, closing the list, or with `add` added or taken out, the list
    /// kept open to pick more.
    pub(crate) fn choose_overlap(&mut self, row: usize, add: bool) {
        let Some(listed) = self.overlaps.take() else {
            return;
        };
        if !add {
            self.unhover_overlap();
        }
        let click = match &listed.list.items {
            OverlapItems::Sketch(ids) => ids
                .get(row)
                .map(|&id| Look::ClickGeometry { hit: Some(id), add }),
            OverlapItems::Model(picks) => picks.get(row).map(|&pick| Look::ClickModel {
                pick: Some(pick),
                add,
                double: false,
            }),
        };
        // Taken out meanwhile, so the click doesn't close it.
        if let Some(click) = click {
            self.look_at(click);
        }
        if add {
            self.overlaps = Some(listed);
        }
    }

    /// Which rows of the list of the model's overlaps are ticked while a
    /// session picks edges or faces of its own (a chamfer's, a fillet's,
    /// a shell's), where a row chosen picks its item or takes it out:
    /// those it has. `None` elsewhere, where the model's selection ticks
    /// them.
    pub(crate) fn overlap_ticks(&self) -> Option<Vec<bool>> {
        let OverlapItems::Model(picks) = &self.overlaps.as_ref()?.list.items else {
            return None;
        };
        (picks.iter()).map(|&pick| self.motion_has(pick)).collect()
    }

    /// Whether what's hovered in the model is drawn over what hides it
    /// too: while a row of the list of the model's overlaps is hovered,
    /// as what's listed is often hidden.
    pub(crate) fn hovers_through(&self) -> bool {
        self.overlaps.as_ref().is_some_and(|listed| {
            listed.hovered.is_some() && matches!(listed.list.items, OverlapItems::Model(_))
        })
    }

    /// Closes the list, changing nothing selected.
    pub(crate) fn close_overlaps(&mut self) {
        if self.overlaps.take().is_some() {
            self.unhover_overlap();
        }
    }

    /// Lets go of what a row of the list hovered: the viewport says what
    /// it's over again as the cursor moves.
    fn unhover_overlap(&mut self) {
        if self.sketch.is_some() {
            self.hover_item(None);
        } else {
            self.hover(None);
        }
    }
}
