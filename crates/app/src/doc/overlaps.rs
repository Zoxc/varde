//! The list of what overlaps where the left button was held still in the
//! viewport (see `varde_view::Overlaps`): hovering a row highlights its
//! item, choosing one selects it as a click on it would, and closing it
//! leaves the selection as it was. A list of the model's is found again
//! on each new model shown while it's open (a session's preview of each
//! tick), its rows by their names; a row of the session's own edge or
//! face that the new preview took away (rounded off, cut out) is kept,
//! marked removed, so it can be taken out from the list.

use varde_view::{
    Look, OverlapItems, OverlapNote, OverlapTick, Overlaps, Pick, PickIndex, Picked, Selected,
};

use super::Doc;
use super::motion::HeldRef;

/// The list open, and the row of it hovered.
#[derive(Debug)]
pub(crate) struct Listed {
    pub(crate) list: Overlaps,
    hovered: Option<usize>,
    /// A list of the model's: each row's item by its names, as on the
    /// model it was opened on, to find it again on a new one.
    names: Vec<Option<Selected>>,
    /// A list of the model's: each row's item as the session being set
    /// up had it among its own edges or faces when the row was last
    /// chosen (or the list opened), to keep the row while it has it.
    held: Vec<Option<HeldRef>>,
    /// A list of the model's: whether each row's item is gone from the
    /// model shown, kept for the session's edge or face it is
    /// (`held`): not hovered, and chosen, takes that out.
    removed: Vec<bool>,
}

impl Listed {
    /// Whether row `row` is one whose item the model shown no longer has.
    fn removed(&self, row: usize) -> bool {
        self.removed.get(row).copied().unwrap_or(false)
    }
}

/// The item `pick` is of on `index`'s model by its names: its face's key,
/// its edge's faces' or its vertex's, and its body as drawn there.
fn name(index: &PickIndex, pick: &Pick) -> Option<Selected> {
    let (body, near) = (pick.body, pick.at);
    Some(match pick.target {
        Picked::Face(face) => Selected::Face {
            body,
            key: index.picking().faces().get(face as usize)?.key,
            near,
        },
        Picked::Edge(chain) => Selected::Edge {
            body,
            faces: index.chain_keys(chain)?,
            near,
        },
        Picked::Vertex(corner) => Selected::Vertex {
            body,
            faces: index.vertex_keys(corner)?,
            near,
        },
    })
}

/// The item `named` on `index`'s model, drawn on `body` there, if it's
/// there: a pick of it where it was named.
fn found(index: &PickIndex, named: &Selected, body: varde_document::BodyId) -> Option<Pick> {
    let (target, at) = match *named {
        Selected::Face { key, near, .. } => {
            (Picked::Face(index.find_face(body, &key, near)?), near)
        }
        Selected::Edge { faces, near, .. } => {
            (Picked::Edge(index.find_edge(body, faces, near)?), near)
        }
        Selected::Vertex { faces, near, .. } => {
            (Picked::Vertex(index.find_vertex(body, faces, near)?), near)
        }
        Selected::Body(_) => return None,
    };
    Some(Pick {
        model: index.model(),
        target,
        body,
        at,
        snap: None,
    })
}

/// A row of the list of the model's overlaps as it's found again on a
/// new model, see [`Doc::follow_overlaps`].
struct Row {
    pick: Pick,
    named: Selected,
    held: Option<HeldRef>,
    removed: bool,
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
        let (names, held) = match &list.items {
            OverlapItems::Sketch(_) => (Vec::new(), Vec::new()),
            OverlapItems::Model(picks) => {
                let index = self.feed.pick_index();
                let names = picks.iter().map(|pick| name(index, pick)).collect();
                let held = picks.iter().map(|&pick| self.motion_ref_at(pick)).collect();
                (names, held)
            }
        };
        let removed = vec![false; held.len()];
        self.overlaps = Some(Listed {
            list,
            hovered: None,
            names,
            held,
            removed,
        });
    }

    /// Finds the rows of the list of the model's overlaps again on the
    /// model shown, if it's another than theirs, by their names, each on
    /// the body drawing its body there: those not found are dropped,
    /// but for one of the session's own edges or faces it still has
    /// (its preview took it away), kept marked removed; the list closes
    /// once none is left (or the cursor doesn't pick).
    pub(crate) fn follow_overlaps(&mut self) {
        let model = self.feed.model();
        let Some(listed) = &self.overlaps else {
            return;
        };
        let OverlapItems::Model(picks) = &listed.list.items else {
            return;
        };
        if picks.iter().all(|pick| pick.model == model) {
            return;
        }
        if !self.picks() {
            return self.close_overlaps();
        }
        let document = self.editor.document();
        let merged = self.feed.merged_bodies();
        let drawn = |body| {
            document.body(body)?;
            let holder = merged.iter().find(|(merged, _)| *merged == body);
            Some(holder.map_or(body, |&(_, holder)| holder))
        };
        let index = self.feed.pick_index();
        let mut rows: Vec<Row> = Vec::new();
        for (row, &pick) in picks.iter().enumerate() {
            let Some(named) = listed.names.get(row).copied().flatten() else {
                continue;
            };
            let held = listed.held.get(row).copied().flatten();
            let found = drawn(named.body()).and_then(|body| found(index, &named, body));
            match found {
                Some(found) => {
                    let again =
                        (rows.iter()).any(|row| !row.removed && row.pick.target == found.target);
                    if !again {
                        rows.push(Row {
                            pick: found,
                            named,
                            held,
                            removed: false,
                        });
                    }
                }
                // Its item is on no model shown now: the pick kept, as
                // of this model, never hovered nor clicked.
                None if held.is_some_and(|held| self.motion_holds(&held)) => rows.push(Row {
                    pick: Pick { model, ..pick },
                    named,
                    held,
                    removed: true,
                }),
                None => {}
            }
        }
        if rows.is_empty() {
            return self.close_overlaps();
        }
        self.unhover_overlap();
        if let Some(listed) = &mut self.overlaps {
            listed.list.items = OverlapItems::Model(rows.iter().map(|row| row.pick).collect());
            listed.names = rows.iter().map(|row| Some(row.named)).collect();
            listed.held = rows.iter().map(|row| row.held).collect();
            listed.removed = rows.iter().map(|row| row.removed).collect();
            listed.hovered = None;
        }
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
            // Removed, it's on no model shown to highlight.
            (OverlapItems::Model(picks), Some(row)) => {
                (None, (!listed.removed(row)).then_some(picks[row]))
            }
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
    /// kept open to pick more. A row removed takes the session's edge or
    /// face it is out, as a click on it would have.
    pub(crate) fn choose_overlap(&mut self, row: usize, add: bool) {
        let Some(mut listed) = self.overlaps.take() else {
            return;
        };
        if !add {
            self.unhover_overlap();
        }
        if listed.removed(row) {
            if let Some(held) = listed.held.get(row).copied().flatten() {
                self.drop_motion_ref(&held);
            }
            if add {
                self.overlaps = Some(listed);
            }
            return;
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
            // What the session has of each row now, to keep a row its
            // next preview takes away.
            if let OverlapItems::Model(picks) = &listed.list.items {
                for (row, &pick) in picks.iter().enumerate() {
                    if !listed.removed(row) {
                        listed.held[row] = self.motion_ref_at(pick);
                    }
                }
            }
            self.overlaps = Some(listed);
        }
    }

    /// How the rows of the list of the model's overlaps show while a
    /// session picks the model for itself (a combine, a move or other
    /// motion session, the measure tool, picking a plane), as the session
    /// a click goes to has their items: ticked where the click would leave
    /// it as it is or take it out, see [`Doc::motion_tick`]; a row removed
    /// ticked while the session still has it. `None` elsewhere, where the
    /// model's selection ticks them.
    pub(crate) fn overlap_ticks(&self) -> Option<Vec<OverlapTick>> {
        let listed = self.overlaps.as_ref()?;
        let OverlapItems::Model(picks) = &listed.list.items else {
            return None;
        };
        let own = self.combine.is_some()
            || self.motion.is_some()
            || self.measure.is_some()
            || self.picking_plane.is_some();
        if !own {
            return None;
        }
        let tick = |row: usize, pick: Pick| {
            if listed.removed(row) {
                let held = listed.held.get(row).copied().flatten();
                return OverlapTick {
                    ticked: held.is_some_and(|held| self.motion_holds(&held)),
                    note: OverlapNote::Removed,
                };
            }
            // As `look_at` hands a click on.
            let ticked = if self.combine.is_some() {
                self.combine_has(pick)
            } else if self.motion.is_some() {
                return self.motion_tick(pick);
            } else if self.measure.is_some() {
                self.measure_has(pick)
            } else {
                // Picking a plane, a click on a face takes it: none is.
                false
            };
            OverlapTick {
                ticked,
                note: OverlapNote::None,
            }
        };
        Some(
            picks
                .iter()
                .enumerate()
                .map(|(row, &pick)| tick(row, pick))
                .collect(),
        )
    }

    /// Which rows [`Doc::overlap_ticks`] ticks.
    #[cfg(test)]
    pub(crate) fn overlap_ticked(&self) -> Option<Vec<bool>> {
        let ticks = self.overlap_ticks()?;
        Some(ticks.iter().map(|tick| tick.ticked).collect())
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
