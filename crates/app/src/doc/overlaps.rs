//! The list of what overlaps where the left button was held still in the
//! viewport (see `varde_view::Overlaps`): hovering a row highlights its
//! item, choosing one selects it as a click on it would, and closing it
//! leaves the selection as it was. A list of the model's is found again
//! on each new model shown while it's open (a session's preview of each
//! tick), its rows by their names; a row of the session's own edge or
//! face that the new preview took away (rounded off, cut out) is kept,
//! marked removed, so it can be taken out from the list. A list of the
//! model's may hold the curves and points of the sketches picked with it
//! too: outside sketches selected as the model
//! is, and in a sketch with Project or Intersect picked as that tool
//! picks.

use crate::doc::OperationSlot;
use varde_view::{
    Look, OverlapItem, OverlapItems, OverlapNote, OverlapTick, Overlaps, Pick, PickIndex, Picked,
    Selected,
};

use super::Doc;
use super::motion::HeldRef;

/// The list open, and the row of it hovered.
#[derive(Debug)]
pub(crate) struct Listed {
    pub(crate) list: Overlaps,
    hovered: Option<usize>,
    /// A list of the model's: its rows, each item as in `list.items`;
    /// empty for a list of the sketch being edited's.
    rows: Vec<Row>,
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
        Selected::Body(_) | Selected::SketchItem { .. } => return None,
    };
    Some(Pick {
        model: index.model(),
        target,
        body,
        at,
        snap: None,
    })
}

/// The items of a list of the model's overlaps, each as an
/// [`OverlapItem`]; none for a list of the sketch being edited's.
fn items(items: &OverlapItems) -> Option<Vec<OverlapItem>> {
    match items {
        OverlapItems::Sketch(_) => None,
        OverlapItems::Model(items) => Some(items.clone()),
    }
}

/// Whether `item` is of model `model`: sketches' items and origin planes
/// are of every model.
fn of_model(item: &OverlapItem, model: u64) -> bool {
    match item {
        OverlapItem::Model(pick) => pick.model == model,
        OverlapItem::Sketch(_) | OverlapItem::Origin(_) => true,
    }
}

/// A row of the list of the model's overlaps, found again on each new
/// model, see [`Doc::follow_overlaps`].
#[derive(Debug, Clone, Copy)]
struct Row {
    item: OverlapItem,
    /// Its item by its names, as on the model it was opened on, to find
    /// it again on a new one; `None` for an origin plane, which needs no
    /// finding.
    named: Option<Selected>,
    /// Its item as the session being set up had it among its own edges
    /// or faces when the row was last chosen (or the list opened), to
    /// keep the row while it has it.
    held: Option<HeldRef>,
    /// Whether its item is gone from the model shown, kept for the
    /// session's edge or face it is (`held`): not hovered, and chosen,
    /// takes that out.
    removed: bool,
}

impl Doc {
    /// Opens `list`, in a sketch only of its items, outside one only of
    /// the model shown while the cursor picks it. Nothing's hovered till
    /// a row is.
    pub(crate) fn open_overlaps(&mut self, list: Overlaps) {
        let model = self.feed.model();
        let items = items(&list.items);
        let fits = match &items {
            None => self.sketch.is_some(),
            Some(items) => self.picks() && items.iter().all(|item| of_model(item, model)),
        };
        if !fits || list.items.is_empty() {
            return;
        }
        self.unhover_overlap();
        let index = self.feed.pick_index();
        let rows = (items.into_iter().flatten())
            .map(|item| {
                let (named, held) = match item {
                    OverlapItem::Model(pick) => (name(index, &pick), self.motion_ref_at(pick)),
                    OverlapItem::Sketch(varde_view::SketchItem { sketch, item }) => {
                        (Some(Selected::SketchItem { sketch, item }), None)
                    }
                    OverlapItem::Origin(_) => (None, None),
                };
                Row {
                    item,
                    named,
                    held,
                    removed: false,
                }
            })
            .collect();
        self.overlaps = Some(Listed {
            list,
            hovered: None,
            rows,
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
        if listed.rows.is_empty() {
            return;
        }
        if listed.rows.iter().all(|row| of_model(&row.item, model)) {
            return;
        }
        if !self.picks() {
            return self.close_overlaps();
        }
        let document = self.editor.document();
        let drawn = |body| document.body(body).map(|_| self.feed.shown_body(body));
        let index = self.feed.pick_index();
        let mut rows: Vec<Row> = Vec::new();
        for &Row {
            item, named, held, ..
        } in &listed.rows
        {
            // An origin plane is on every model.
            if let OverlapItem::Origin(_) = item {
                rows.push(Row {
                    item,
                    named: None,
                    held: None,
                    removed: false,
                });
                continue;
            }
            let Some(named) = named else {
                continue;
            };
            // A sketch's item isn't the model's: kept while it's there.
            let OverlapItem::Model(pick) = item else {
                if (named.sketch_item())
                    .is_some_and(|item| super::pick::sketch_holds(document, item))
                {
                    rows.push(Row {
                        item,
                        named: Some(named),
                        held: None,
                        removed: false,
                    });
                }
                continue;
            };
            let found = named
                .body()
                .and_then(drawn)
                .and_then(|body| found(index, &named, body));
            match found {
                Some(found) => {
                    let again = (rows.iter()).any(|row| {
                        !row.removed
                            && matches!(row.item, OverlapItem::Model(pick) if pick.target == found.target)
                    });
                    if !again {
                        rows.push(Row {
                            item: OverlapItem::Model(found),
                            named: Some(named),
                            held,
                            removed: false,
                        });
                    }
                }
                // Its item is on no model shown now: the pick kept, as
                // of this model, never hovered nor clicked.
                None if held.is_some_and(|held| self.motion_holds(&held)) => rows.push(Row {
                    item: OverlapItem::Model(Pick { model, ..pick }),
                    named: Some(named),
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
            listed.list.items = OverlapItems::Model(rows.iter().map(|row| row.item).collect());
            listed.rows = rows;
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
        let Some(row) = row else {
            return self.unhover_overlap();
        };
        if let OverlapItems::Sketch(ids) = &listed.list.items {
            let id = ids[row];
            return self.hover_item(Some(id));
        }
        let removed = listed.rows.get(row).is_some_and(|row| row.removed);
        match listed.rows.get(row).map(|row| row.item) {
            Some(OverlapItem::Sketch(item)) => self.hover_sketch(Some(item)),
            // In the model's place, as the viewport hovers it.
            Some(OverlapItem::Origin(plane)) => {
                self.hover(None);
                self.plane_hover = Some(plane);
            }
            // Removed, it's on no model shown to highlight.
            Some(OverlapItem::Model(pick)) => self.hover((!removed).then_some(pick)),
            None => self.unhover_overlap(),
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
        let add = add && !self.picks_only(listed.rows.get(row).map(|row| row.item));
        if !add {
            self.unhover_overlap();
        }
        if let Some(&Row {
            removed: true,
            held,
            ..
        }) = listed.rows.get(row)
        {
            if let Some(held) = held {
                self.drop_motion_ref(&held);
            }
            if add {
                self.overlaps = Some(listed);
            }
            return;
        }
        // Picking a plane, a face or origin plane chosen is picked for the
        // sketch, which closes the list.
        if let Some(picking) = &self.picking_plane {
            let picked = (listed.rows.get(row)).and_then(|row| match row.item {
                OverlapItem::Model(Pick {
                    target: Picked::Face(face),
                    at,
                    ..
                }) => (picking.pick.face_ref(self.feed.pick_index(), face, at))
                    .map(varde_view::Edit::FacePicked),
                OverlapItem::Origin(plane) => Some(varde_view::Edit::PlanePicked(plane)),
                _ => None,
            });
            if let Some(picked) = picked {
                self.unhover_overlap();
                self.update(picked);
            } else if add {
                self.overlaps = Some(listed);
            }
            return;
        }
        let click = match &listed.list.items {
            OverlapItems::Sketch(ids) => ids
                .get(row)
                .map(|&id| Look::ClickGeometry { hit: Some(id), add }),
            OverlapItems::Model(_) => {
                listed.rows.get(row).map(|row| match row.item {
                    OverlapItem::Model(pick) => Look::ClickModel {
                        pick: Some(pick),
                        add,
                        double: false,
                    },
                    OverlapItem::Sketch(item) => Look::ClickSketch { item, add },
                    // As its toolbar button picks it.
                    OverlapItem::Origin(plane) => {
                        Look::Motion(varde_view::MotionLook::OriginPlane(plane))
                    }
                })
            }
        };
        // Taken out meanwhile, so the click doesn't close it.
        if let Some(click) = click {
            self.look_at(click);
        }
        if add {
            // What the session has of each row now, to keep a row its
            // next preview takes away.
            for row in &mut listed.rows {
                if let OverlapItem::Model(pick) = row.item
                    && !row.removed
                {
                    row.held = self.motion_ref_at(pick);
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
    /// ticked while the session still has it. With Project or Intersect
    /// in a sketch, ticked as the tool has picked its item. Elsewhere the
    /// model's selection ticks them.
    pub(crate) fn overlap_ticks(&self) -> Option<Vec<OverlapTick>> {
        let listed = self.overlaps.as_ref()?;
        let rows = &listed.rows;
        if rows.is_empty() {
            return None;
        }
        let plain = |ticked| OverlapTick {
            ticked,
            ..OverlapTick::default()
        };
        if self.picks_outside() {
            let marked = self.outside_marked();
            let ticks = rows.iter().map(|row| match &row.item {
                OverlapItem::Model(pick) => plain(self.outside_has(*pick)),
                OverlapItem::Sketch(item) => plain(marked.contains(item)),
                OverlapItem::Origin(_) => plain(false),
            });
            return Some(ticks.collect());
        }
        let own = self.operation.combine().is_some()
            || self.operation.motion().is_some()
            || self.measure.is_some()
            || self.picking_plane.is_some();
        if !own {
            let OverlapItems::Model(items) = &listed.list.items else {
                return None;
            };
            let selection = &self.pick.selection;
            let targets: Vec<Picked> = selection.targets().collect();
            let items_selected: Vec<_> = selection.sketch_items().collect();
            let ticks = items.iter().map(|item| match item {
                OverlapItem::Model(pick) => {
                    plain(selection.model() == Some(pick.model) && targets.contains(&pick.target))
                }
                OverlapItem::Sketch(item) => plain(items_selected.contains(item)),
                OverlapItem::Origin(_) => plain(false),
            });
            return Some(ticks.collect());
        }
        let tick = |row: &Row| {
            // Sessions don't pick sketches' items with the model.
            let OverlapItem::Model(pick) = row.item else {
                return plain(false);
            };
            if row.removed {
                return OverlapTick {
                    ticked: row.held.is_some_and(|held| self.motion_holds(&held)),
                    note: OverlapNote::Removed,
                    only: false,
                };
            }
            // As `look_at` hands a click on.
            let ticked = if self.operation.combine().is_some() {
                self.combine_has(pick)
            } else if self.operation.motion().is_some() {
                return self.motion_tick(pick);
            } else if self.measure.is_some() {
                self.measure_has(pick)
            } else {
                // Picking a plane, a click on a face takes it: none is.
                false
            };
            OverlapTick {
                ticked,
                ..OverlapTick::default()
            }
        };
        Some(
            (rows.iter())
                .map(|row| OverlapTick {
                    only: self.picks_only(Some(row.item)),
                    ..tick(row)
                })
                .collect(),
        )
    }

    /// Whether a click on a row of `item` (`None` in a list of the
    /// sketch's) picks it alone, never adding it to others: picking a
    /// plane, and an origin plane anywhere ([`OverlapTick::only`]).
    fn picks_only(&self, item: Option<OverlapItem>) -> bool {
        self.picking_plane.is_some() || matches!(item, Some(OverlapItem::Origin(_)))
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
        }
        // With Project or Intersect in a sketch, the model's hover too.
        if self.sketch.is_none() || self.picks_outside() {
            self.hover(None);
        }
        self.plane_hover = None;
    }
}
