//! The tool rail's open set, see [`varde_view::RailLook`]. A list the
//! cursor opened only peeks: it closes as the cursor leaves the head. One
//! opened by a click on its head or the set's key is held: it stays till
//! a tool is picked, a click elsewhere, `Esc` or the key again closes it.

use varde_view::{RailLook, RailOpen};

use super::Doc;

/// The rail's open set and what the cursor is over of it.
#[derive(Debug, Default)]
pub(crate) struct Rail {
    /// The index of the set whose list is open, if one is.
    pub(crate) open: Option<usize>,
    /// The row of the open list the keys are on.
    pub(crate) row: usize,
    /// Whether the open list was opened by a click or a key, and stays
    /// as the cursor leaves its head.
    held: bool,
    /// Where to scroll the open list to, from its top (0) to its end (1),
    /// to show the row the keys moved to, once the app asks: see
    /// [`Rail::take_scroll`].
    scroll: Option<f32>,
}

impl Rail {
    /// Closes the open set's list, if there is one.
    pub(crate) fn close(&mut self) {
        self.open = None;
        self.held = false;
    }

    /// Opens the list of the set `set`, the keys on its first row and
    /// scrolled to its top, unless it's open already.
    fn open(&mut self, set: usize) {
        if self.open != Some(set) {
            self.open = Some(set);
            self.row = 0;
            self.scroll = Some(0.0);
        }
    }

    /// The open set and the row the keys are on, if a set is open.
    pub(crate) fn state(&self) -> Option<RailOpen> {
        Some(RailOpen {
            set: self.open?,
            row: self.row,
            held: self.held,
        })
    }

    /// Where to scroll the open list to, once: see [`Rail::scroll`].
    pub(crate) fn take_scroll(&mut self) -> Option<f32> {
        self.scroll.take().filter(|_| self.open.is_some())
    }

    /// Moves the keys to the row `step` rows down the open list of
    /// `rows` rows (up if negative), round from one end to the other, and
    /// scrolls it to show it.
    fn step(&mut self, step: isize, rows: usize) {
        if self.open.is_none() || rows == 0 {
            return;
        }
        let row = self.row.min(rows - 1);
        self.row = (row as isize + step).rem_euclid(rows as isize) as usize;
        // The list's scroll at this share of its end shows the row,
        // however tall the list shows.
        self.scroll = Some(if rows > 1 {
            self.row as f32 / (rows - 1) as f32
        } else {
            0.0
        });
    }

    /// Takes `message`, with `sets` sets on the rail, the list of
    /// set `i` having `rows(i)` rows.
    pub(crate) fn update(&mut self, message: RailLook, sets: usize, rows: impl Fn(usize) -> usize) {
        let open_rows = self.open.map_or(0, &rows);
        match message {
            RailLook::Open(set) if set < sets => {
                self.open(set);
                self.held = true;
            }
            // A list the cursor only peeks at, its key holds.
            RailLook::Toggle(set) if set < sets => {
                if self.open == Some(set) && self.held {
                    self.close();
                } else {
                    self.open(set);
                    self.held = true;
                }
            }
            RailLook::Open(_) | RailLook::Toggle(_) => {}
            RailLook::Close => self.close(),
            RailLook::Up => self.step(-1, open_rows),
            RailLook::Down => self.step(1, open_rows),
            RailLook::Row(row) => {
                if row < open_rows {
                    self.row = row;
                }
            }
            // A held list stays: only a click or a key changes it.
            RailLook::Hover(..) if self.held => {}
            RailLook::Hover(set, true) if set < sets => self.open(set),
            // Off a head: entering the next may come before or after, so
            // only leaving the head of the set peeked at closes it.
            RailLook::Hover(set, false) if self.open == Some(set) => self.close(),
            RailLook::Hover(..) => {}
        }
    }
}

impl Doc {
    /// Takes `message`, about the rail.
    pub(crate) fn rail_look(&mut self, message: RailLook) {
        let sketching = self.sketch.is_some();
        let sets = varde_view::rail_sets(sketching);
        let rows = |set| varde_view::rail_rows(sketching, set);
        self.rail.update(message, sets, rows);
    }
}

#[cfg(test)]
mod tests;
