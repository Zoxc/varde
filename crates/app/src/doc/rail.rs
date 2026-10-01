//! The tool rail's open set, and its list closing as the cursor leaves
//! it, see [`varde_view::RailLook`].

use std::time::Duration;

use iced::time::Instant;
use varde_view::{RailLook, RailOpen, RailSpot};

use super::Doc;

/// How long the open set's list stays once the cursor has left its head
/// and the list, so the cursor can cross the gap from the card to the
/// list.
pub(crate) const RAIL_CLOSE_DELAY: Duration = Duration::from_millis(250);

/// The rail's open set and what the cursor is over of it.
#[derive(Debug, Default)]
pub(crate) struct Rail {
    /// The index of the set whose list is open, if one is.
    pub(crate) open: Option<usize>,
    /// The row of the open list the keys are on.
    pub(crate) row: usize,
    /// Where to scroll the open list to, from its top (0) to its end (1),
    /// to show the row the keys moved to, once the app asks: see
    /// [`Rail::take_scroll`].
    scroll: Option<f32>,
    /// What the cursor is over of the rail, if anything.
    over: Option<RailSpot>,
    /// When the cursor left the head and the list while the list was
    /// open: it closes [`RAIL_CLOSE_DELAY`] after, unless the cursor is
    /// back by then.
    left: Option<Instant>,
}

impl Rail {
    /// Closes the open set's list, if there is one.
    pub(crate) fn close(&mut self) {
        self.open = None;
        self.left = None;
    }

    /// Opens the list of the set `set`, the keys on its first row and
    /// scrolled to its top, unless it's open already.
    fn open(&mut self, set: usize) {
        self.left = None;
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

    /// Takes `message` at `now`, with `sets` sets on the rail, the list of
    /// set `i` having `rows(i)` rows.
    pub(crate) fn update(
        &mut self,
        message: RailLook,
        sets: usize,
        rows: impl Fn(usize) -> usize,
        now: Instant,
    ) {
        let open_rows = self.open.map_or(0, &rows);
        match message {
            RailLook::Open(set) if set < sets => self.open(set),
            RailLook::Toggle(set) if set < sets => {
                if self.open == Some(set) {
                    self.close();
                } else {
                    self.open(set);
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
            RailLook::Hover(spot, true) => {
                self.over = Some(spot);
                match spot {
                    RailSpot::Head(set) if set < sets => self.open(set),
                    RailSpot::Head(_) => {}
                    RailSpot::Tool => self.close(),
                    RailSpot::List => self.left = None,
                }
            }
            // Left of what the cursor was last over: entering the next
            // thing may come before or after.
            RailLook::Hover(spot, false) => {
                if self.over == Some(spot) {
                    self.over = None;
                    if self.open.is_some() {
                        self.left = Some(now);
                    }
                }
            }
        }
    }

    /// Closes the list once the cursor has been off it and the heads for
    /// [`RAIL_CLOSE_DELAY`] at `now`.
    pub(crate) fn tick(&mut self, now: Instant) {
        if self
            .left
            .is_some_and(|left| now.saturating_duration_since(left) >= RAIL_CLOSE_DELAY)
        {
            self.close();
        }
    }

    /// Whether the list waits to close, which takes frames to see.
    pub(crate) fn closing(&self) -> bool {
        self.left.is_some()
    }
}

impl Doc {
    /// Takes `message`, about the rail, at `now`.
    pub(crate) fn rail_look(&mut self, message: RailLook, now: Instant) {
        let sketching = self.sketch.is_some();
        let sets = varde_view::rail_sets(sketching);
        let rows = |set| varde_view::rail_rows(sketching, set);
        self.rail.update(message, sets, rows, now);
    }
}

#[cfg(test)]
mod tests;
