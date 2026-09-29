use std::{collections::VecDeque, sync::Arc};

use serde::{Deserialize, Serialize};
use varde_expr::LengthUnit;
use varde_sketch::Sketch;

use crate::{BodyId, Document, EditError, FeatureId, FeatureKind, Plane, Snapshot};

/// An edit to a [`Document`]. [`Editor::apply`] refuses one that would
/// leave the document failing [`Document::check`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    RemoveBody(BodyId),
    SetVisible(BodyId, bool),
    /// Adds a feature holding an empty sketch on `plane`.
    AddSketch {
        name: String,
        plane: Plane,
    },
    /// Replaces a sketch feature's sketch whole: how every edit inside a
    /// sketch is committed, as one undoable change.
    SetSketch {
        feature: FeatureId,
        sketch: Box<Sketch>,
    },
    /// Removes a feature and the bodies it makes.
    RemoveFeature(FeatureId),
    SetFeatureVisible(FeatureId, bool),
    /// Changes the design's units. Every dimension's expression first has
    /// the old units written in after its bare numbers
    /// ([`Sketch::pin_units`]), so it means what it did, and no value or
    /// geometry changes.
    SetUnits(LengthUnit),
    /// Replaces the whole document, e.g. with unsaved changes recovered
    /// after a crash.
    Replace(Box<Document>),
}

impl Document {
    /// The command adding a new sketch on `plane`, named one past the
    /// highest "Sketch N" in the document, so that the numbering holds
    /// across sessions and undo.
    pub fn add_sketch(&self, plane: Plane) -> Command {
        let names = self.features.iter().map(|feature| feature.name.as_str());
        Command::AddSketch {
            name: format!("Sketch {}", next_number(names, "Sketch")),
            plane,
        }
    }
}

/// One past the highest `N` of the `names` that are "`kind` N", or 1. A
/// file could hold the highest number there is, which is then reused: a
/// repeated name is harmless.
fn next_number<'a>(names: impl Iterator<Item = &'a str>, kind: &str) -> u64 {
    names
        .filter_map(|name| {
            name.strip_prefix(kind)?
                .strip_prefix(' ')?
                .parse::<u64>()
                .ok()
        })
        .max()
        .map_or(1, |highest| highest.saturating_add(1))
}

/// Names a state of an [`Editor`]'s document, see [`Editor::revision`].
/// Only equality means anything: a newer state may have an older
/// revision, so there's no ordering them. Serializable as its number, for
/// the IO lane's requests, which answer with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

/// An editor's own come from [`Editor::revision`]; this is for tests,
/// which name one by its number.
impl From<u64> for Revision {
    fn from(number: u64) -> Self {
        Self(number)
    }
}

impl From<Revision> for u64 {
    fn from(revision: Revision) -> Self {
        revision.0
    }
}

/// Orders the changes of an [`Editor`]'s document, see
/// [`Editor::generation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Generation(u64);

/// An editor's own come from [`Editor::generation`]; this is for tests,
/// which name one by its number.
impl From<u64> for Generation {
    fn from(number: u64) -> Self {
        Self(number)
    }
}

impl From<Generation> for u64 {
    fn from(generation: Generation) -> Self {
        generation.0
    }
}

/// How many edits can be undone. Older ones are forgotten, so history holds
/// at most this many copies of the document.
const MAX_UNDO: usize = 200;

/// Owns a [`Document`] and its edit history.
///
/// History is snapshot based for simplicity, and capped at `MAX_UNDO`
/// steps. Once documents get large this should switch to storing inverse
/// commands or a persistent data structure.
///
/// The document and its history are behind `Arc`s, so a [`snapshot`] is a
/// refcount bump and undo/redo only swap pointers. An edit clones the
/// document once, and one that would change nothing isn't cloned at all.
///
/// [`snapshot`]: Editor::snapshot
#[derive(Debug, Clone)]
pub struct Editor {
    current: State,
    undo: VecDeque<State>,
    redo: Vec<State>,
    /// The revision the next edit gets.
    next_revision: Revision,
    generation: Generation,
}

/// A document in an editor's history, with the revision naming it, which
/// stays with it through undo and redo, and its lineage, see
/// [`Editor::lineage`].
#[derive(Debug, Clone)]
struct State {
    document: Arc<Document>,
    revision: Revision,
    lineage: Revision,
}

impl Editor {
    /// Starts editing `document`, which passes [`Document::check`] as
    /// every [`Document`] does (see there).
    pub fn new(document: Document) -> Self {
        debug_assert_eq!(document.check(), Ok(()));
        Self {
            current: State {
                document: Arc::new(document),
                revision: Revision(0),
                lineage: Revision(0),
            },
            undo: VecDeque::new(),
            redo: Vec::new(),
            next_revision: Revision(1),
            generation: Generation(0),
        }
    }

    pub fn document(&self) -> &Document {
        &self.current.document
    }

    /// The current document, shared rather than copied. Later edits don't
    /// change it.
    pub fn snapshot(&self) -> Snapshot {
        self.current.document.clone()
    }

    /// Names the document's state, for asking whether it's one seen before,
    /// e.g. the one saved. Each edit makes a new one, and undo and redo
    /// give a state back its own.
    pub fn revision(&self) -> Revision {
        self.current.revision
    }

    /// Names the line of edits the document comes from: the revision of
    /// the document it began with, the first or one that replaced it
    /// whole ([`Command::Replace`]). It changes only when an edit, undo or
    /// redo crosses such a replacement, when ids in the document, which
    /// otherwise name the same things from one state to the next, may name
    /// other things than before.
    pub fn lineage(&self) -> Revision {
        self.current.lineage
    }

    /// Grows on every change, undo and redo included: for ordering work
    /// about the document, such as regenerating it, where the newest wins.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Records a change: a new generation.
    fn changed(&mut self) {
        self.generation = Generation(self.generation.0 + 1);
    }

    /// Applies `command` as one undoable change. Refuses one that runs out
    /// of ids or would leave the document failing [`Document::check`],
    /// leaving the document, its history, revision and generation as they
    /// were. One that would change nothing leaves them as they were too.
    pub fn apply(&mut self, command: Command) -> Result<(), EditError> {
        // Each edit works on a private copy, so a refused one leaves `self`
        // as it was, and one that would change nothing returns before
        // copying.
        let document = &self.current.document;
        let replacing = matches!(command, Command::Replace(_));
        let next = match command {
            Command::RemoveBody(id) => {
                let Some(index) = document.body_index(id) else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                next.bodies.remove(index);
                next
            }
            Command::SetVisible(id, visible) => {
                let Some(index) = document
                    .body_index(id)
                    .filter(|&index| document.bodies[index].visible != visible)
                else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                next.bodies[index].visible = visible;
                next
            }
            Command::AddSketch { name, plane } => {
                let mut next = Document::clone(document);
                let sketch = Sketch::default();
                next.add_feature(name, FeatureKind::Sketch { plane, sketch })?;
                next
            }
            Command::SetSketch { feature, sketch } => {
                let Some(index) = document
                    .feature_index(feature)
                    .filter(|&index| match &document.features[index].kind {
                        FeatureKind::Sketch { sketch: old, .. } => *old != *sketch,
                    })
                else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                match &mut next.features[index].kind {
                    FeatureKind::Sketch { sketch: old, .. } => *old = *sketch,
                }
                next
            }
            Command::RemoveFeature(id) => {
                let Some(index) = document.feature_index(id) else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                next.features.remove(index);
                next.bodies.retain(|body| body.created_by != id);
                next
            }
            Command::SetFeatureVisible(id, visible) => {
                let Some(index) = document
                    .feature_index(id)
                    .filter(|&index| document.features[index].visible != visible)
                else {
                    return Ok(());
                };
                let mut next = Document::clone(document);
                next.features[index].visible = visible;
                next
            }
            Command::SetUnits(units) => {
                if units == document.units {
                    return Ok(());
                }
                let mut next = Document::clone(document);
                let before = document.design();
                for feature in &mut next.features {
                    match &mut feature.kind {
                        FeatureKind::Sketch { sketch, .. } => sketch.pin_units(&before),
                    }
                }
                next.units = units;
                next
            }
            Command::Replace(replacement) => {
                if *replacement == **document {
                    return Ok(());
                }
                *replacement
            }
        };
        next.check().map_err(EditError::Invalid)?;

        let revision = self.next_revision;
        // One per edit: a u64 won't run out.
        self.next_revision = Revision(revision.0 + 1);
        let lineage = if replacing {
            revision
        } else {
            self.current.lineage
        };
        let before = std::mem::replace(
            &mut self.current,
            State {
                document: Arc::new(next),
                revision,
                lineage,
            },
        );
        self.push_undo(before);
        self.redo.clear();
        self.changed();
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop_back() {
            self.redo
                .push(std::mem::replace(&mut self.current, previous));
            self.changed();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let current = std::mem::replace(&mut self.current, next);
            self.push_undo(current);
            self.changed();
        }
    }

    fn push_undo(&mut self, state: State) {
        self.undo.push_back(state);
        if self.undo.len() > MAX_UNDO {
            self.undo.pop_front();
        }
    }
}

#[cfg(test)]
mod tests;
