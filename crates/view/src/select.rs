//! What's selected in the model shown: faces, edges, vertices and bodies,
//! picked with the cursor or (bodies) in Objects, and the curves and
//! points of the finished sketches shown.
//!
//! A [`Selection`] keeps what's selected by name, as a reference would: a
//! face by its body and key, an edge by its body and the keys of the
//! faces either side, a vertex by its body and the keys of three faces
//! meeting there, each with the point it was picked at, and a body by
//! its id. So it outlives the model it was picked in: when another model
//! shows (an edit, an undo, a new tolerance), [`Selection::resolve`]
//! finds each face, edge and vertex again by its keys and aliases
//! ([`PickIndex::find_face`], [`PickIndex::find_edge`],
//! [`PickIndex::find_vertex`]), the nearest to its point among several of
//! one name. What isn't there any more (a face an edit removed, a hidden
//! body's, a body merged into another) stops being selected, but it's
//! kept and looked for in each later model until the selection changes,
//! so an undo brings it back. A face, edge or vertex of a body merged
//! into another is looked for in that one.
//!
//! How a click selects depends on the [`SelectionMode`]: outside the
//! sessions a click selects the face, edge or vertex under the cursor and
//! a double-click its body; a face session takes only faces, an edge
//! session only edges, with their tangent chains if it asks, and a body
//! session bodies, whatever of them is clicked. A click alone selects
//! what it's on, or nothing; with `add` (Shift or Ctrl held) it adds that
//! or, if it's all selected already, takes it out.
//!
//! A sketch's curve or point ([`Selected::SketchItem`]) is selected by
//! its sketch and id, outside the sessions only ([`SelectionMode::Any`]).
//! It doesn't depend on the model, so a new model keeps it; the app drops
//! it once its sketch or the item is gone, or hidden
//! ([`Selection::retain_sketch_items`]).

use glam::DVec3;
use varde_document::{BodyId, FaceRef, FeatureId};
use varde_kernel::mesh::FaceKey;
use varde_sketch::Id;

use crate::pick::{ModelHighlight, Pick, PickIndex, Picked, Picks};

/// What a click in the model selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// The face, edge or vertex clicked, the body of any on a
    /// double-click: outside the sessions.
    #[default]
    Any,
    Faces,
    /// Edges, with all of the tangent chain clicked if `tangent`.
    Edges {
        tangent: bool,
    },
    /// The body of whatever is clicked.
    Bodies,
}

impl SelectionMode {
    /// What the cursor picks in it.
    pub fn picks(self) -> Picks {
        match self {
            SelectionMode::Any | SelectionMode::Bodies => Picks::All,
            SelectionMode::Faces => Picks::Faces,
            SelectionMode::Edges { .. } => Picks::Edges,
        }
    }

    /// Whether bodies are selected in it, in the viewport or in Objects.
    pub fn takes_bodies(self) -> bool {
        matches!(self, SelectionMode::Any | SelectionMode::Bodies)
    }

    /// Whether the curves and points of the sketches shown are selected
    /// in it: outside the sessions.
    pub fn takes_sketch_items(self) -> bool {
        self == SelectionMode::Any
    }
}

/// A curve or point of a finished sketch, as the viewport picks it in the
/// model: the sketch feature and the item's id in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SketchItem {
    pub sketch: FeatureId,
    pub item: Id,
}

/// Something selected, by name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Selected {
    /// A face of `body` named `key`, picked at `near`.
    Face {
        body: BodyId,
        key: FaceKey,
        near: DVec3,
    },
    /// An edge of `body` between faces named `faces` (sorted), picked at
    /// `near`.
    Edge {
        body: BodyId,
        faces: [FaceKey; 2],
        near: DVec3,
    },
    /// A vertex of `body` where faces named `faces` (sorted) meet, at
    /// `near`.
    Vertex {
        body: BodyId,
        faces: [FaceKey; 3],
        near: DVec3,
    },
    Body(BodyId),
    /// The curve or point `item` of the sketch `sketch`.
    SketchItem {
        sketch: FeatureId,
        item: Id,
    },
}

impl Selected {
    /// The body it's of, or is: none for a sketch's item.
    pub fn body(&self) -> Option<BodyId> {
        match *self {
            Selected::Face { body, .. }
            | Selected::Edge { body, .. }
            | Selected::Vertex { body, .. }
            | Selected::Body(body) => Some(body),
            Selected::SketchItem { .. } => None,
        }
    }

    /// The sketch's item it is, if it's one.
    pub fn sketch_item(&self) -> Option<SketchItem> {
        match *self {
            Selected::SketchItem { sketch, item } => Some(SketchItem { sketch, item }),
            _ => None,
        }
    }
}

/// What's selected in the model, in the order it was, kept by name
/// ([`Selected`]) so it's found again in each new model
/// ([`Selection::resolve`]); clicks select as its [`SelectionMode`] says
/// ([`Selection::click`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    mode: SelectionMode,
    /// Each item, in the order selected, with its face or edge in the
    /// model `model` (none for a body or a sketch's item), no two found
    /// of one target, body or sketch item; and what was selected but
    /// isn't found there, looked for again in each later model until the
    /// selection changes.
    items: Vec<Entry>,
    /// The model the items' targets are of, see [`Pick::model`].
    model: Option<u64>,
}

/// An item of a [`Selection`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    item: Selected,
    /// Its face or edge, none for a body or a sketch's item.
    target: Option<Picked>,
    /// Whether it was found in the model last looked in: selected, or
    /// only looked for.
    found: bool,
}

impl Selection {
    /// Nothing selected, clicks selecting as `mode` says.
    pub fn new(mode: SelectionMode) -> Self {
        Selection {
            mode,
            ..Selection::default()
        }
    }

    pub fn mode(&self) -> SelectionMode {
        self.mode
    }

    /// Whether nothing is selected, though what was may be looked for
    /// still (see [`Selection::holds_nothing`]).
    pub fn is_empty(&self) -> bool {
        !self.items.iter().any(|entry| entry.found)
    }

    /// Whether nothing is selected or looked for in later models.
    pub fn holds_nothing(&self) -> bool {
        self.items.is_empty()
    }

    /// What's selected, in the order it was.
    pub fn items(&self) -> impl Iterator<Item = &Selected> {
        self.found().map(|entry| &entry.item)
    }

    /// The faces, edges and vertices selected in the model
    /// [`Selection::model`].
    pub fn targets(&self) -> impl Iterator<Item = Picked> + '_ {
        self.found().filter_map(|entry| entry.target)
    }

    /// The face selected, as a sketch plane stores it, if a face and
    /// nothing else is: what a new sketch is put on (`S`).
    pub fn single_face(&self) -> Option<FaceRef> {
        let mut items = self.items();
        match (items.next(), items.next()) {
            (Some(&Selected::Face { body, key, near }), None) => Some(FaceRef { body, key, near }),
            _ => None,
        }
    }

    /// The bodies selected, as bodies.
    pub fn bodies(&self) -> impl Iterator<Item = BodyId> + '_ {
        self.found().filter_map(|entry| match entry.item {
            Selected::Body(body) => Some(body),
            _ => None,
        })
    }

    /// The curves and points of sketches selected.
    pub fn sketch_items(&self) -> impl Iterator<Item = SketchItem> + '_ {
        self.found().filter_map(|entry| entry.item.sketch_item())
    }

    /// Keeps only the sketches' items `keep` says are still there to
    /// select: the app's, by the document.
    pub fn retain_sketch_items(&mut self, keep: impl Fn(SketchItem) -> bool) {
        (self.items).retain(|entry| entry.item.sketch_item().is_none_or(&keep));
    }

    /// The entries selected, not those only looked for.
    fn found(&self) -> impl Iterator<Item = &Entry> {
        self.items.iter().filter(|entry| entry.found)
    }

    /// Forgets what's only looked for: the selection changes.
    fn forget_missing(&mut self) {
        self.items.retain(|entry| entry.found);
    }

    /// The model its faces and edges were last found in, if any.
    pub fn model(&self) -> Option<u64> {
        self.model
    }

    /// Selects nothing, and forgets what was but isn't found. Whether
    /// anything was selected.
    pub fn clear(&mut self) -> bool {
        let was = !self.is_empty();
        self.items.clear();
        was
    }

    /// Finds the faces and edges selected in `index`'s model by their
    /// names, if they were last found in another, and those that weren't
    /// found before; what isn't there stops being selected but is kept
    /// to look for in later models. `drawn` says which body of the model
    /// draws a body: itself, the body a join merged it into, or none if
    /// the document doesn't hold it. A face or edge is looked for in that
    /// body; a body is selected only where it's drawn as itself. Whether
    /// anything changed, the model they're found in included.
    pub fn resolve(&mut self, index: &PickIndex, drawn: impl Fn(BodyId) -> Option<BodyId>) -> bool {
        let before = self.items.clone();
        let fresh = self.model == Some(index.model());
        let items = std::mem::take(&mut self.items);
        for Entry {
            item,
            target,
            found,
        } in items
        {
            let found = match item {
                Selected::Body(body) => (drawn(body) == Some(body)).then_some(None),
                // Not the model's: kept as it is.
                Selected::SketchItem { .. } => Some(None),
                _ if fresh && found => Some(target),
                // Not found in this model before, so not now either.
                _ if fresh => None,
                Selected::Face { body, key, near } => drawn(body)
                    .and_then(|body| index.find_face(body, &key, near))
                    .map(|face| Some(Picked::Face(face))),
                Selected::Edge { body, faces, near } => drawn(body)
                    .and_then(|body| index.find_edge(body, faces, near))
                    .map(|chain| Some(Picked::Edge(chain))),
                Selected::Vertex { body, faces, near } => drawn(body)
                    .and_then(|body| index.find_vertex(body, faces, near))
                    .map(|corner| Some(Picked::Vertex(corner))),
            };
            match found {
                Some(target) => self.push(item, target),
                None => self.items.push(Entry {
                    item,
                    target: None,
                    found: false,
                }),
            }
        }
        self.model = Some(index.model());
        !fresh || self.items != before
    }

    /// Adds `item`, whose target is `target`, unless something of that
    /// target, or that body or sketch item, is selected.
    fn push(&mut self, item: Selected, target: Option<Picked>) {
        let taken = self.found().any(|other| match target {
            Some(_) => other.target == target,
            None => other.item == item,
        });
        if !taken {
            self.items.push(Entry {
                item,
                target,
                found: true,
            });
        }
    }

    /// Takes a click on `pick`, of `index`'s model, or on nothing: alone it
    /// selects what the mode makes of `pick` (nothing clears), with `add`
    /// it adds that or, if all of it is selected, takes it out (a tangent
    /// chain as one). `double` is the second click of a
    /// double-click, which outside the sessions selects the body instead
    /// of what the first click did. A pick of another model is ignored.
    /// Whether anything changed.
    pub fn click(
        &mut self,
        index: &PickIndex,
        pick: Option<Pick>,
        add: bool,
        double: bool,
    ) -> bool {
        if pick.is_some_and(|pick| pick.model != index.model()) {
            return false;
        }
        // What isn't found in this model can't be compared with what's
        // clicked; and once the selection is changed, what wasn't found
        // isn't looked for any more.
        let mut changed = self.resolve_in(index);
        self.forget_missing();
        let Some(pick) = pick else {
            return if add {
                changed
            } else {
                self.clear() || changed
            };
        };
        let body = double && self.mode == SelectionMode::Any;
        if body && add {
            // Undo the first click's toggle.
            changed |= self.toggle(self.unit(index, pick, false));
        }
        let unit = self.unit(index, pick, body);
        if unit.is_empty() {
            return changed;
        }
        if add {
            return self.toggle(unit) || changed;
        }
        let before = std::mem::take(&mut self.items);
        for (item, target) in unit {
            self.push(item, target);
        }
        self.items != before || changed
    }

    /// Takes a click on `body`'s row in Objects: selects it alone, or with
    /// `add` adds it or takes it out. Only where bodies are selected.
    /// Whether anything changed.
    pub fn click_body(&mut self, body: BodyId, add: bool) -> bool {
        if !self.mode.takes_bodies() {
            return false;
        }
        self.forget_missing();
        let unit = vec![(Selected::Body(body), None)];
        if add {
            return self.toggle(unit);
        }
        let before = std::mem::take(&mut self.items);
        self.push(Selected::Body(body), None);
        self.items != before
    }

    /// Takes a click on a sketch's curve or point `item` in the model:
    /// selects it alone, or with `add` adds it or takes it out. Only where
    /// they're selected ([`SelectionMode::takes_sketch_items`]). Whether
    /// anything changed.
    pub fn click_sketch(&mut self, item: SketchItem, add: bool) -> bool {
        if !self.mode.takes_sketch_items() {
            return false;
        }
        self.forget_missing();
        let SketchItem { sketch, item } = item;
        let item = Selected::SketchItem { sketch, item };
        if add {
            return self.toggle(vec![(item, None)]);
        }
        let before = std::mem::take(&mut self.items);
        self.push(item, None);
        self.items != before
    }

    /// Finds what's selected in `index`'s model if it was last found in
    /// another, keeping all bodies selected. Whether that changed what's
    /// selected or where.
    fn resolve_in(&mut self, index: &PickIndex) -> bool {
        let before = self.items.clone();
        if self.model != Some(index.model()) {
            self.resolve(index, Some);
        }
        self.items != before
    }

    /// What a click on `pick` selects or toggles as one in the mode, or
    /// its body if `body`: an edge's tangent chain where it's asked for.
    fn unit(&self, index: &PickIndex, pick: Pick, body: bool) -> Vec<(Selected, Option<Picked>)> {
        let edge = |chain: u32, near: DVec3| {
            let faces = index.chain_keys(chain)?;
            let item = Selected::Edge {
                body: pick.body,
                faces,
                near,
            };
            Some((item, Some(Picked::Edge(chain))))
        };
        match (self.mode, pick.target) {
            _ if body => vec![(Selected::Body(pick.body), None)],
            (SelectionMode::Bodies, _) => vec![(Selected::Body(pick.body), None)],
            (SelectionMode::Any | SelectionMode::Faces, Picked::Face(face)) => {
                let Some(key) = index.picking().faces().get(face as usize).map(|f| f.key) else {
                    return Vec::new();
                };
                let item = Selected::Face {
                    body: pick.body,
                    key,
                    near: pick.at,
                };
                vec![(item, Some(pick.target))]
            }
            (SelectionMode::Any | SelectionMode::Edges { tangent: false }, Picked::Edge(chain)) => {
                edge(chain, pick.at).into_iter().collect()
            }
            (SelectionMode::Edges { tangent: true }, Picked::Edge(chain)) => {
                (index.tangent_chain(chain).iter())
                    .filter_map(|&other| {
                        let near = if other == chain {
                            Some(pick.at)
                        } else {
                            index.chain_point(other)
                        };
                        edge(other, near?)
                    })
                    .collect()
            }
            (SelectionMode::Any, Picked::Vertex(corner)) => {
                let Some(faces) = index.vertex_keys(corner) else {
                    return Vec::new();
                };
                let item = Selected::Vertex {
                    body: pick.body,
                    faces,
                    near: pick.at,
                };
                vec![(item, Some(pick.target))]
            }
            (SelectionMode::Faces, Picked::Edge(_) | Picked::Vertex(_))
            | (SelectionMode::Edges { .. }, Picked::Face(_) | Picked::Vertex(_)) => Vec::new(),
        }
    }

    /// Takes `unit` out if all of it is selected, else adds what of it
    /// isn't. Whether anything changed.
    fn toggle(&mut self, unit: Vec<(Selected, Option<Picked>)>) -> bool {
        // Whether `entry` is `item`, whose target is `target`.
        let is = |entry: &Entry, &(item, target): &(Selected, Option<Picked>)| match target {
            Some(_) => entry.target == target,
            None => entry.item == item,
        };
        if unit.is_empty() {
            return false;
        }
        if unit
            .iter()
            .all(|part| self.found().any(|entry| is(entry, part)))
        {
            self.items
                .retain(|entry| !unit.iter().any(|part| is(entry, part)));
        } else {
            for (item, target) in unit {
                self.push(item, target);
            }
        }
        true
    }

    /// What hovering `pick` highlights in the mode: the face or edge, its
    /// tangent chain where edges take theirs, or its body's faces where
    /// clicks select bodies.
    pub fn hovered(&self, index: &PickIndex, pick: Pick) -> Vec<Picked> {
        match (self.mode, pick.target) {
            (SelectionMode::Bodies, _) => index.body_faces(pick.body).map(Picked::Face).collect(),
            (SelectionMode::Edges { tangent: true }, Picked::Edge(chain)) => {
                (index.tangent_chain(chain).iter())
                    .map(|&c| Picked::Edge(c))
                    .collect()
            }
            (SelectionMode::Faces, Picked::Edge(_) | Picked::Vertex(_))
            | (SelectionMode::Edges { .. }, Picked::Face(_) | Picked::Vertex(_)) => Vec::new(),
            (_, target) => vec![target],
        }
    }

    /// What's drawn of what's selected and of `hovered`, in `index`'s
    /// model ([`PickIndex::highlight`]): a body selected as all of its
    /// faces. The faces, edges and vertices are those last found, so only
    /// of the model they were found in.
    pub fn highlight(&self, index: &PickIndex, hovered: Option<Pick>) -> ModelHighlight {
        let hovered = hovered
            .filter(|pick| pick.model == index.model())
            .map(|pick| self.hovered(index, pick))
            .unwrap_or_default();
        self.highlight_hovering(index, hovered)
    }

    /// Like [`Self::highlight`], with `hovered` hovered (a body's faces,
    /// its row in Objects hovered).
    pub fn highlight_hovering(&self, index: &PickIndex, hovered: Vec<Picked>) -> ModelHighlight {
        let fresh = self.model == Some(index.model());
        let mut selected: Vec<Picked> = if fresh {
            self.targets().collect()
        } else {
            Vec::new()
        };
        for body in self.bodies() {
            selected.extend(index.body_faces(body).map(Picked::Face));
        }
        selected.sort_unstable();
        selected.dedup();
        index.highlight(&hovered, &selected)
    }
}

#[cfg(test)]
mod tests;
