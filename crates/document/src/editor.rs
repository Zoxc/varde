//! The [`Editor`]: a [`Document`] and its undo history.

use std::collections::VecDeque;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use varde_sketch::Budget;

use crate::{Command, Document, EditError, FeatureKind, Snapshot};

/// Names a state of an [`Editor`]'s document, see [`Editor::revision`].
/// Only equality means anything outside the editor: the state current
/// later may have an older revision (undo), so there's no ordering them;
/// which was made first, the editor says ([`Editor::made_before`]). Serializable as its number, for
/// the IO lane's requests, which answer with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

/// An editor's own come from [`Editor::revision`]; this is for tests,
/// which name one by its number.
#[cfg(any(test, feature = "testing"))]
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
#[cfg(any(test, feature = "testing"))]
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
pub const MAX_UNDO: usize = 200;

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

/// A state of an [`Editor`]'s document kept apart from it
/// ([`Editor::state`]), for [`Editor::restore`] to bring back: the
/// document, shared with the editor's history while it holds it, with
/// its revision and lineage.
#[derive(Debug, Clone)]
pub struct EditorState(State);

impl EditorState {
    pub fn document(&self) -> &Snapshot {
        &self.0.document
    }

    pub fn revision(&self) -> Revision {
        self.0.revision
    }

    pub fn lineage(&self) -> Revision {
        self.0.lineage
    }
}

/// Why [`Editor::restore`] refused a state.
#[derive(Debug, Clone, PartialEq)]
pub enum RestoreError {
    /// The state is of another lineage than the document now (from
    /// before a [`Command::Replace`], or after one undone): its ids may
    /// name other things.
    OtherLineage,
    /// The document restored would fail [`Document::check`].
    Edit(EditError),
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
        self.change(command, true, &Budget::default())
    }

    /// Applies `command` as [`Editor::apply`] does, sketches it solves
    /// again (as a parameter's change does) solved within `budget`: one
    /// that runs out refuses the command as one that won't solve does,
    /// [`EditError::Unsolved`] with [`Failure::OutOfTime`](varde_sketch::Failure::OutOfTime). For the UI
    /// thread, which must not wait on the solver unbounded; the caller
    /// keeps the clock, as the web has no `std::time::Instant`.
    pub fn apply_within(&mut self, command: Command, budget: &Budget) -> Result<(), EditError> {
        self.change(command, true, budget)
    }

    /// Applies `command` as [`Editor::apply`] does, but folded into the
    /// change before it rather than as one of its own: undo takes both
    /// back at once, and the redo history stays. For what follows from a
    /// change rather than being one: a sketch's links found again on the
    /// model the change made. The document gets a new revision and
    /// generation as with any change. Refused as [`Editor::apply`]
    /// refuses, leaving everything as it was.
    pub fn amend(&mut self, command: Command) -> Result<(), EditError> {
        self.change(command, false, &Budget::default())
    }

    /// Applies `command` as [`Editor::amend`] does, sketches it solves
    /// again solved within `budget`, as [`Editor::apply_within`] does.
    pub fn amend_within(&mut self, command: Command, budget: &Budget) -> Result<(), EditError> {
        self.change(command, false, budget)
    }

    /// Whether the document `revision` names is current or on the undo
    /// history: whether the edit that made it is still in effect, not
    /// undone (on the redo history) or gone with a branch undone past it.
    pub fn is_done(&self, revision: Revision) -> bool {
        self.current.revision == revision
            || self.undo.iter().any(|state| state.revision == revision)
    }

    /// Whether the document `revision` names is anywhere on the history:
    /// current, on the undo history or on the redo history.
    pub fn on_history(&self, revision: Revision) -> bool {
        self.is_done(revision) || self.is_undone(revision)
    }

    /// Whether the document `revision` names is on the redo history:
    /// the edit that made it undone.
    pub fn is_undone(&self, revision: Revision) -> bool {
        self.redo.iter().any(|state| state.revision == revision)
    }

    /// Whether the state `revision` names is gone from the history with a
    /// branch: the user undid past it, then edited. Told apart from one
    /// the history let go of as too old (`MAX_UNDO`, 200) by being made after
    /// the oldest state it still has; one let go of isn't gone.
    pub fn is_gone(&self, revision: Revision) -> bool {
        let oldest = self.undo.front().unwrap_or(&self.current).revision;
        !self.on_history(revision) && oldest.0 < revision.0
    }

    /// Whether the state `a` names was made before the one `b` names:
    /// revisions are given out in order, so a state made from another
    /// (by an edit, a restore, an amend) has a later one than it, though
    /// undo can make an older one current again.
    pub fn made_before(&self, a: Revision, b: Revision) -> bool {
        a.0 < b.0
    }

    /// The current state, to [restore](Editor::restore) later.
    pub fn state(&self) -> EditorState {
        EditorState(self.current.clone())
    }

    /// The state `revision` names, if it's on the history
    /// (current, undone or redone).
    pub fn state_of(&self, revision: Revision) -> Option<EditorState> {
        (std::iter::once(&self.current))
            .chain(&self.undo)
            .chain(&self.redo)
            .find(|state| state.revision == revision)
            .map(|state| EditorState(state.clone()))
    }

    /// Brings `state`'s document back, as one undoable change with a
    /// revision of its own. Undo winds the id counters back, so the ids
    /// a state kept from before an undo and a new edit may name other
    /// things than now: the restored document keeps the largest of each
    /// counter, its own and each sketch's, over it and every state of the
    /// lineage on the history (a sketch removed since took its counter
    /// out of the document now). The lineage
    /// stays only while `state`'s revision is still on the history
    /// (current, undone or redone); otherwise the change starts a new one, as
    /// [`Command::Replace`] does. A state of another lineage than now is
    /// refused; one whose document is the current one changes nothing.
    pub fn restore(&mut self, state: &EditorState) -> Result<(), RestoreError> {
        let Some(next) = self.restored(state)? else {
            return Ok(());
        };
        next.check()
            .map_err(|why| RestoreError::Edit(EditError::Invalid(why)))?;
        let new_lineage = !self.on_history(state.0.revision);
        self.commit(next, new_lineage, true);
        Ok(())
    }

    /// Whether [restoring](Editor::restore) `state` would change nothing:
    /// its document is the current one, but for the id counters restore
    /// keeps the larger of. A state of another lineage changes nothing
    /// too (it's refused).
    pub fn restores_nothing(&self, state: &EditorState) -> bool {
        matches!(self.restored(state), Ok(None) | Err(_))
    }

    /// The document [`Editor::restore`] would make current, unchecked:
    /// `None` if it's the current one.
    fn restored(&self, state: &EditorState) -> Result<Option<Document>, RestoreError> {
        let state = &state.0;
        if state.lineage != self.current.lineage {
            return Err(RestoreError::OtherLineage);
        }
        if Arc::ptr_eq(&state.document, &self.current.document) {
            return Ok(None);
        }
        let now = &self.current.document;
        let mut next = Document::clone(&state.document);
        // Every state of the lineage on the history, not only now: a
        // sketch removed since took its counter with it, and the states
        // from before the removal stay on the history to be restored.
        let lineage = self.current.lineage;
        let states = (std::iter::once(&self.current))
            .chain(&self.undo)
            .chain(&self.redo)
            .filter(|state| state.lineage == lineage);
        for state in states {
            let there = &state.document;
            next.next_id = next.next_id.max(there.next_id);
            for feature in &mut next.features {
                let FeatureKind::Sketch { sketch, .. } = &mut feature.kind else {
                    continue;
                };
                if let Some(FeatureKind::Sketch { sketch: there, .. }) =
                    there.feature(feature.id).map(|feature| &feature.kind)
                {
                    sketch.next_id = sketch.next_id.max(there.next_id);
                }
            }
        }
        Ok((next != **now).then_some(next))
    }

    /// The documents of the undo history, then the current one, oldest
    /// first, each with its revision: the states the document went
    /// through to now, each made from the one before by an edit (whose
    /// after-revision is its revision).
    pub fn history(&self) -> impl DoubleEndedIterator<Item = (Revision, &Snapshot)> {
        (self.undo.iter())
            .chain(std::iter::once(&self.current))
            .map(|state| (state.revision, &state.document))
    }

    /// [`Editor::apply`], or, unless `own_step`, [`Editor::amend`], with
    /// sketches solved within `budget`.
    fn change(
        &mut self,
        command: Command,
        own_step: bool,
        budget: &Budget,
    ) -> Result<(), EditError> {
        // A refused edit leaves `self` as it was.
        let replacing = matches!(command, Command::Replace(_));
        let Some(next) = self.current.document.after(command, budget)? else {
            return Ok(());
        };
        next.check().map_err(EditError::Invalid)?;
        self.commit(next, replacing, own_step);
        Ok(())
    }

    /// Makes `next`, which passed [`Document::check`], the current
    /// document with a revision of its own: of a new lineage if
    /// `new_lineage`, and its own undo step if `own_step` (amended, the
    /// state replaced is dropped: undo goes back past it).
    fn commit(&mut self, next: Document, new_lineage: bool, own_step: bool) {
        let revision = self.next_revision;
        // One per edit: a u64 won't run out.
        self.next_revision = Revision(revision.0 + 1);
        let lineage = match new_lineage {
            true => revision,
            false => self.current.lineage,
        };
        let before = std::mem::replace(
            &mut self.current,
            State {
                document: Arc::new(next),
                revision,
                lineage,
            },
        );
        if own_step {
            self.push_undo(before);
            self.redo.clear();
        }
        self.changed();
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
