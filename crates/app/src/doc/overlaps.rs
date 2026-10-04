//! The list of what overlaps where the left button was held still in the
//! viewport (see `varde_view::Overlaps`): hovering a row highlights its
//! item, choosing one selects it as a click on it would, and closing it
//! leaves the selection as it was. A list of the model's is found again
//! on each new model shown while it's open (a session's preview of each
//! tick), its rows by their names.

use varde_view::{Look, OverlapItems, Overlaps, Pick, PickIndex, Picked, Selected};

use super::Doc;

/// The list open, and the row of it hovered.
#[derive(Debug)]
pub(crate) struct Listed {
    pub(crate) list: Overlaps,
    hovered: Option<usize>,
    /// A list of the model's: each row's item by its names, as on the
    /// model it was opened on, to find it again on a new one.
    names: Vec<Option<Selected>>,
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
        let names = match &list.items {
            OverlapItems::Sketch(_) => Vec::new(),
            OverlapItems::Model(picks) => {
                let index = self.feed.pick_index();
                picks.iter().map(|pick| name(index, pick)).collect()
            }
        };
        self.overlaps = Some(Listed {
            list,
            hovered: None,
            names,
        });
    }

    /// Finds the rows of the list of the model's overlaps again on the
    /// model shown, if it's another than theirs, by their names, each on
    /// the body drawing its body there: those not found are dropped, and
    /// the list closes once none is left (or the cursor doesn't pick).
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
        let mut rows: Vec<(Pick, Option<Selected>)> = Vec::new();
        for named in listed.names.iter().flatten() {
            let Some(pick) = drawn(named.body()).and_then(|body| found(index, named, body)) else {
                continue;
            };
            if rows.iter().all(|(other, _)| other.target != pick.target) {
                rows.push((pick, Some(*named)));
            }
        }
        if rows.is_empty() {
            return self.close_overlaps();
        }
        self.unhover_overlap();
        if let Some(listed) = &mut self.overlaps {
            let (picks, names) = rows.into_iter().unzip();
            listed.list.items = OverlapItems::Model(picks);
            listed.names = names;
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
