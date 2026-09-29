use std::{collections::VecDeque, sync::Arc};

use glam::Vec3;
use serde::{Deserialize, Serialize};
use varde_kernel::Shape;

use crate::{BodyId, Document, EditError, Snapshot};

/// An edit to a [`Document`]. [`Editor::apply`] refuses one that would
/// leave the document failing [`Document::check`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    AddBody {
        name: String,
        shape: Shape,
        position: Vec3,
    },
    RemoveBody(BodyId),
    SetVisible(BodyId, bool),
    /// Replaces the whole document, e.g. with unsaved changes recovered
    /// after a crash.
    Replace(Box<Document>),
}

/// The edge length of a cube made by [`Document::add_cube`].
pub const CUBE_SIZE: f32 = 2.0;

/// The gap [`Document::add_cube`] leaves between a new cube and the bodies
/// before it.
const CUBE_GAP: f32 = 1.0;

impl Document {
    /// The command adding the next cube, named and placed from the
    /// document so that holds across sessions and undo: numbered one past
    /// the highest "Cube N" in it, and placed past the body reaching
    /// farthest along x. Once that is past the coordinate limit, the
    /// editor refuses it.
    pub fn add_cube(&self) -> Command {
        let number = self
            .bodies
            .iter()
            .filter_map(|body| body.name.strip_prefix("Cube ")?.parse::<u64>().ok())
            .max()
            .map_or(1, |highest| highest.saturating_add(1));
        // A document's positions and shapes' bounds are within
        // `MAX_COORD`, which `Document::check` holds, so its shapes build
        // and these sums stay finite.
        let x = self
            .bodies
            .iter()
            .map(|body| {
                let solid = body.shape.build().expect("a document's shapes are checked");
                body.position.x + solid.bounds().max.x + CUBE_GAP
            })
            .fold(0.0, f32::max);
        Command::AddBody {
            name: format!("Cube {number}"),
            shape: Shape::cuboid(Vec3::splat(CUBE_SIZE)),
            position: Vec3::new(x, 0.0, 0.0),
        }
    }
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
/// stays with it through undo and redo.
#[derive(Debug, Clone)]
struct State {
    document: Arc<Document>,
    revision: Revision,
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
        let next = match command {
            Command::AddBody {
                name,
                shape,
                position,
            } => {
                let mut next = Document::clone(document);
                next.add_body(name, shape, position)?;
                next
            }
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
        let before = std::mem::replace(
            &mut self.current,
            State {
                document: Arc::new(next),
                revision,
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
