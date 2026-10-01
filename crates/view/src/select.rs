//! What's selected in the model shown: faces, edges and bodies, picked
//! with the cursor or (bodies) in Objects.
//!
//! A [`Selection`] keeps what's selected by name, as a reference would: a
//! face by its body and key, an edge by its body and the keys of the
//! faces either side, each with the point it was picked at, and a body by
//! its id. So it outlives the model it was picked in: when another model
//! shows (an edit, an undo, a new tolerance), [`Selection::resolve`]
//! finds each face and edge again by its keys and aliases
//! ([`PickIndex::find_face`], [`PickIndex::find_edge`]), the nearest to
//! its point among several of one name, and drops what isn't there any
//! more.
//!
//! How a click selects depends on the [`SelectionMode`]: outside the
//! sessions a click selects the face or edge under the cursor and a
//! double-click its body; a face session takes only faces, an edge
//! session only edges, with their tangent chains if it asks, and a body
//! session bodies, whatever of them is clicked. A click alone selects
//! what it's on, or nothing; with `add` (Shift or Ctrl held) it adds that
//! or, if it's all selected already, takes it out.

use glam::DVec3;
use varde_document::BodyId;
use varde_kernel::mesh::FaceKey;
use varde_render::{Emphasis, Highlight};

use crate::pick::{Pick, PickIndex, Picked, Picks};

/// What a click in the model selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// The face or edge clicked, the body of either on a double-click:
    /// outside the sessions.
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
            SelectionMode::Any | SelectionMode::Bodies => Picks::FacesAndEdges,
            SelectionMode::Faces => Picks::Faces,
            SelectionMode::Edges { .. } => Picks::Edges,
        }
    }

    /// Whether bodies are selected in it, in the viewport or in Objects.
    pub fn takes_bodies(self) -> bool {
        matches!(self, SelectionMode::Any | SelectionMode::Bodies)
    }
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
    Body(BodyId),
}

impl Selected {
    /// The body it's of, or is.
    pub fn body(&self) -> BodyId {
        match *self {
            Selected::Face { body, .. } | Selected::Edge { body, .. } | Selected::Body(body) => {
                body
            }
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
    /// Each item with its face or edge in the model `model` (none for a
    /// body), no two of one target or body.
    items: Vec<(Selected, Option<Picked>)>,
    /// The model the items' targets are of, see [`Pick::model`].
    model: Option<u64>,
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

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// What's selected, in the order it was.
    pub fn items(&self) -> impl Iterator<Item = &Selected> {
        self.items.iter().map(|(item, _)| item)
    }

    /// The faces and edges selected in the model [`Selection::model`].
    pub fn targets(&self) -> impl Iterator<Item = Picked> + '_ {
        self.items.iter().filter_map(|&(_, target)| target)
    }

    /// The bodies selected, as bodies.
    pub fn bodies(&self) -> impl Iterator<Item = BodyId> + '_ {
        self.items.iter().filter_map(|(item, _)| match item {
            Selected::Body(body) => Some(*body),
            _ => None,
        })
    }

    /// The model its faces and edges were last found in, if any.
    pub fn model(&self) -> Option<u64> {
        self.model
    }

    /// Selects nothing. Whether anything was.
    pub fn clear(&mut self) -> bool {
        let was = !self.items.is_empty();
        self.items.clear();
        was
    }

    /// Finds the faces and edges selected in `index`'s model by their
    /// names, if they were last found in another, dropping those that
    /// aren't there any more, and the bodies selected for which `exists`
    /// says no. Whether anything changed, the model they're found in
    /// included.
    pub fn resolve(&mut self, index: &PickIndex, exists: impl Fn(BodyId) -> bool) -> bool {
        let before = self.items.clone();
        let fresh = self.model == Some(index.model());
        let items = std::mem::take(&mut self.items);
        for (item, target) in items {
            let target = match item {
                Selected::Body(body) => {
                    if exists(body) {
                        None
                    } else {
                        continue;
                    }
                }
                _ if fresh => target,
                Selected::Face { body, key, near } => {
                    let Some(face) = index.find_face(body, &key, near) else {
                        continue;
                    };
                    Some(Picked::Face(face))
                }
                Selected::Edge { body, faces, near } => {
                    let Some(chain) = index.find_edge(body, faces, near) else {
                        continue;
                    };
                    Some(Picked::Edge(chain))
                }
            };
            self.push(item, target);
        }
        self.model = Some(index.model());
        !fresh || self.items != before
    }

    /// Adds `item`, whose target is `target`, unless something of that
    /// target or body is selected.
    fn push(&mut self, item: Selected, target: Option<Picked>) {
        let taken = self
            .items
            .iter()
            .any(|&(other, other_target)| match target {
                Some(_) => other_target == target,
                None => matches!(other, Selected::Body(body) if body == item.body()),
            });
        if !taken {
            self.items.push((item, target));
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
        // clicked.
        let mut changed = self.resolve_in(index);
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
        let unit = vec![(Selected::Body(body), None)];
        if add {
            return self.toggle(unit);
        }
        let before = std::mem::take(&mut self.items);
        self.items = unit;
        self.items != before
    }

    /// Finds what's selected in `index`'s model if it was last found in
    /// another, keeping all bodies. Whether that changed what's selected
    /// or where.
    fn resolve_in(&mut self, index: &PickIndex) -> bool {
        let before = self.items.clone();
        if self.model != Some(index.model()) {
            self.resolve(index, |_| true);
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
            (SelectionMode::Faces, Picked::Edge(_))
            | (SelectionMode::Edges { .. }, Picked::Face(_)) => Vec::new(),
        }
    }

    /// Takes `unit` out if all of it is selected, else adds what of it
    /// isn't. Whether anything changed.
    fn toggle(&mut self, unit: Vec<(Selected, Option<Picked>)>) -> bool {
        let selected = |&(item, target): &(Selected, Option<Picked>)| {
            self.items
                .iter()
                .any(|&(other, other_target)| match target {
                    Some(_) => other_target == target,
                    None => other == item,
                })
        };
        if unit.is_empty() {
            return false;
        }
        if unit.iter().all(selected) {
            self.items.retain(|&(other, other_target)| {
                !unit.iter().any(|&(item, target)| match target {
                    Some(_) => other_target == target,
                    None => other == item,
                })
            });
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
            (SelectionMode::Faces, Picked::Edge(_))
            | (SelectionMode::Edges { .. }, Picked::Face(_)) => Vec::new(),
            (_, target) => vec![target],
        }
    }

    /// The highlight of what's selected and of `hovered`, in `index`'s
    /// model: what's selected in the selection's colour (a body as all of
    /// its faces), what's hovered that isn't in the hover's. The faces
    /// and edges are those last found, so only of the model they were
    /// found in.
    pub fn highlight(&self, index: &PickIndex, hovered: Option<Pick>) -> Highlight {
        let fresh = self.model == Some(index.model());
        let mut selected: Vec<Picked> = if fresh {
            self.targets().collect()
        } else {
            Vec::new()
        };
        for body in self.bodies() {
            selected.extend(index.body_faces(body).map(Picked::Face));
        }
        let hovered = hovered
            .filter(|pick| pick.model == index.model())
            .map(|pick| self.hovered(index, pick))
            .unwrap_or_default();
        selected.sort_unstable();
        selected.dedup();
        let hovered: Vec<Picked> = hovered
            .into_iter()
            .filter(|target| selected.binary_search(target).is_err())
            .collect();
        let selected = selected.into_iter().map(|t| (t, Emphasis::Selected));
        let hovered = hovered.into_iter().map(|t| (t, Emphasis::Hovered));
        index.highlight(selected.chain(hovered))
    }
}

#[cfg(test)]
mod tests;
