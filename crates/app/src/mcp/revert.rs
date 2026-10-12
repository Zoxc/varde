//! The MCP panel's Revert (`notes/AgentSupport.md`, "Revert"): each applied
//! edit's row keeps the design before and after it ([`Edited`]), and its
//! context menu reverts to either through [`Editor::restore`], one
//! undoable step. A revert that would undo later changes (anyone's)
//! asks first ([`RevertAsk`]), listing them as the differences of the
//! state reverted to from the design now (`changes::differences`).
//!
//! Refused, saying so in a toast, while a client edit is between its
//! arrival and applied or ended, while the user's sketch edits wait on
//! the solver, on a read-only design, and for a state of another
//! lineage (from before a `Replace`).
//!
//! [`Editor::restore`]: varde_document::Editor::restore

use std::sync::Arc;

use varde_document::{Document, EditorState, RestoreError, Revision};

use super::{Mcp, changes, push_bounded};
use crate::doc::Doc;

/// The most distinct documents the log's rows keep for Revert, the
/// oldest rows letting theirs go first. Each is a whole document: those
/// still on the undo history cost nothing more, but one the history let
/// go of (or undid past and branched from) is held by the row alone. An
/// edit's after is usually the next one's before, so 50 keeps Revert on
/// about the last 50 edits, far more than a session looks back over, at
/// most 50 designs' worth of memory beyond the history's own.
pub(crate) const SNAPSHOTS: usize = 50;

/// What an applied edit's row keeps for its menu.
#[derive(Debug, Clone)]
pub(crate) struct Edited {
    /// The design before it and after it (once regeneration answered, a
    /// link's amend in it), while kept ([`SNAPSHOTS`]).
    pub(crate) before: Option<EditorState>,
    pub(crate) after: Option<EditorState>,
    /// The revision it made.
    pub(crate) after_revision: Revision,
    /// The number of the feature it made or changed, if it names one:
    /// what Show in Timeline selects.
    pub(crate) feature: Option<u64>,
    /// Whether its reply reached no one: the session gone, or the call
    /// given up on.
    pub(crate) not_told: bool,
    /// An `undo`'s row: the revisions the edits it undid had made, newest
    /// first (none for any other edit). Taken back when the user redoes
    /// any; it keeps no states, so offers no Revert.
    pub(crate) undid: Vec<Revision>,
}

impl PartialEq for Edited {
    fn eq(&self, other: &Edited) -> bool {
        let revision = |state: &Option<EditorState>| state.as_ref().map(EditorState::revision);
        revision(&self.before) == revision(&other.before)
            && revision(&self.after) == revision(&other.after)
            && (self.after_revision, self.feature, self.not_told)
                == (other.after_revision, other.feature, other.not_told)
            && self.undid == other.undid
    }
}

/// A revert made from the panel, a row of its own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reverted {
    /// Its place among the log's rows: after those of lower serials.
    pub(crate) serial: u64,
    /// The revision it made.
    pub(crate) after: Revision,
    /// What it went back to: "before “Renamed feature Extrude 1 to Base”".
    pub(crate) to: String,
    /// Its row's tooltip: "You: reverted to ..." and what it changed, at
    /// most [`TIP_CHANGES`] of them, the rest counted.
    pub(crate) tip: String,
}

/// The most of a revert's changes its row's tooltip lists.
pub(crate) const TIP_CHANGES: usize = 10;

/// A revert's row's tooltip: "You: reverted to `to`", then what it
/// changed, `made`, a line each, at most [`TIP_CHANGES`], then "and N
/// more".
fn tip(to: &str, made: &[String]) -> String {
    let mut tip = super::text::plain(&format!("You: reverted to {to}"));
    for line in made.iter().take(TIP_CHANGES) {
        tip.push('\n');
        tip.push_str(&super::text::plain(line));
    }
    let more = made.len().saturating_sub(TIP_CHANGES);
    if more > 0 {
        tip.push_str(&format!("\nand {more} more"));
    }
    tip
}

/// A revert made in a design, kept with it (`Waiting::reverted`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Made {
    /// The revision it made.
    pub(crate) after: Revision,
    /// The revision of the state it brought back.
    pub(crate) target: Revision,
    /// What it went back to, as its row says.
    pub(crate) to: String,
}

/// The dialog asking before a revert that undoes later changes.
#[derive(Debug, Clone)]
pub(crate) struct RevertAsk {
    /// The design's revision the differences were worked out on.
    pub(crate) at: Revision,
    pub(crate) state: EditorState,
    pub(crate) to: String,
    pub(crate) differences: Vec<String>,
}

/// What a revert's row and dialog say it goes back to, for an edit whose
/// row says `shown`.
fn to(shown: &str, to_before: bool) -> String {
    match to_before {
        true => format!("before “{shown}”"),
        false => format!("after “{shown}”"),
    }
}

impl Mcp {
    /// Lets the oldest rows' documents go until the log keeps at most
    /// [`SNAPSHOTS`] distinct ones.
    pub(super) fn bound_snapshots(&mut self) {
        loop {
            let mut kept: Vec<*const Document> = Vec::new();
            for edit in self.log.iter().filter_map(|logged| logged.edit.as_ref()) {
                for state in [&edit.before, &edit.after].into_iter().flatten() {
                    let at = Arc::as_ptr(state.document());
                    if !kept.contains(&at) {
                        kept.push(at);
                    }
                }
            }
            if kept.len() <= SNAPSHOTS {
                return;
            }
            let Some(edit) = (self.log.iter_mut())
                .filter_map(|logged| logged.edit.as_mut())
                .find(|edit| edit.before.is_some() || edit.after.is_some())
            else {
                return;
            };
            if edit.before.take().is_none() {
                edit.after = None;
            }
        }
    }

    /// Whether a row standing for the state `at` is taken back in `doc`'s
    /// design ([`Doc::in_effect`]).
    pub(super) fn taken_back(doc: &Doc, at: Option<Revision>) -> bool {
        at.is_some_and(|at| !doc.in_effect(at))
    }

    /// The row menu's and the revert dialog's messages, about `doc`.
    pub(crate) fn panel_message(&mut self, message: varde_view::McpPanel, doc: Option<&mut Doc>) {
        use varde_view::McpPanel;
        let Some(doc) = doc else {
            self.menu = None;
            return;
        };
        match message {
            McpPanel::OpenMenu(serial) => self.menu = Some(serial),
            McpPanel::CloseMenu => self.menu = None,
            McpPanel::ShowInTimeline(serial) => {
                self.menu = None;
                let feature = (self.log.iter())
                    .find(|logged| logged.serial == serial)
                    .and_then(|logged| logged.edit.as_ref()?.feature);
                if let Some(number) = feature {
                    doc.mcp_show_feature(number);
                }
            }
            McpPanel::RevertToHere(serial) => self.revert(doc, serial, false),
            McpPanel::RevertBefore(serial) => self.revert(doc, serial, true),
            McpPanel::ConfirmRevert => {
                let Some(mut ask) = doc.mcp.revert_ask.take() else {
                    return;
                };
                // The design changed while it was up (the client's edits
                // go on): what it listed is stale, so it asks again.
                if ask.at != doc.editor.revision() {
                    if doc.editor.restores_nothing(&ask.state) {
                        doc.toast = Some(AS_IT_WAS.to_owned());
                        return;
                    }
                    ask.differences = changes::differences(doc, ask.state.document());
                    ask.at = doc.editor.revision();
                    doc.mcp.revert_ask = Some(ask);
                    return;
                }
                self.restore(doc, &ask.state, ask.to);
            }
            McpPanel::CancelRevert => doc.mcp.revert_ask = None,
            _ => {}
        }
    }

    /// Reverts `doc`'s design to before the edit of the row `serial`, or
    /// after it: refused saying why, asked first if it would undo later
    /// changes, else made.
    fn revert(&mut self, doc: &mut Doc, serial: u64, to_before: bool) {
        self.menu = None;
        let Some(logged) = self.log.iter().find(|logged| logged.serial == serial) else {
            return;
        };
        let Some(edit) = &logged.edit else {
            return;
        };
        let state = match to_before {
            true => edit.before.clone(),
            false => edit.after.clone(),
        };
        let Some(state) = state else {
            return;
        };
        let to = to(&logged.tool.with(&logged.args), to_before);
        // Before it, with nothing since, undoes it alone.
        let after = doc.mcp.amended_to(edit.after_revision);
        let later = !to_before || doc.editor.revision() != after;
        if let Some(why) = doc.revert_refusal(&state) {
            doc.toast = Some(why);
            return;
        }
        if doc.editor.restores_nothing(&state) {
            doc.toast = Some(AS_IT_WAS.to_owned());
            return;
        }
        if later {
            let differences = changes::differences(doc, state.document());
            doc.mcp.revert_ask = Some(RevertAsk {
                at: doc.editor.revision(),
                state,
                to,
                differences,
            });
            return;
        }
        self.restore(doc, &state, to);
    }

    /// Restores `state` in `doc`'s design, the revert saying `to`: a row
    /// of its own if it changed the design; refused saying why.
    fn restore(&mut self, doc: &mut Doc, state: &EditorState, to: String) {
        if let Some(why) = doc.revert_refusal(state) {
            doc.toast = Some(why);
            return;
        }
        let made = changes::differences(doc, state.document());
        let revision = doc.editor.revision();
        if let Err(why) = doc.editor.restore(state) {
            doc.toast = Some(match why {
                RestoreError::OtherLineage => REPLACED.to_owned(),
                RestoreError::Edit(why) => format!("Couldn't revert: {why}"),
            });
            return;
        }
        if doc.editor.revision() == revision {
            return;
        }
        doc.edit_error = None;
        doc.sync();
        let after = doc.editor.revision();
        let made_revert = Made {
            after,
            target: state.revision(),
            to: to.clone(),
        };
        push_bounded(&mut doc.mcp.reverted, super::waiting::STEPS, made_revert);
        let serial = self.next_serial();
        let tip = tip(&to, &made);
        let reverted = Reverted {
            serial,
            after,
            to,
            tip,
        };
        push_bounded(&mut self.reverts, super::LOG_CAP, reverted);
    }
}

/// Why a revert of a state of another lineage is refused.
pub(crate) const REPLACED: &str = "Can't revert: the design was replaced since";

/// What a revert that would change nothing says.
pub(crate) const AS_IT_WAS: &str = "The design is already as it was then";

/// Why a revert is refused while a client edit is being applied.
pub(crate) const EDIT_IN_FLIGHT: &str =
    "Can't revert while the client's edit is being applied: try again once it is";

impl Doc {
    /// Whether the change that made the state `revision` (an edit's
    /// after, the state a call's told changes were worked out on) is in
    /// effect in the design now: not undone, not gone with a branch
    /// undone past it ([`Editor::is_gone`]), nor taken back by a revert
    /// that is.
    ///
    /// Worked out by revision, as the editor gives them out in order
    /// ([`Editor::made_before`]): the newest revert in effect made after
    /// the state decides. If it brought back the state itself, the state
    /// is; one from before it, it isn't; one from after it, the state is
    /// if it was in that one, asked the same way of the reverts made up
    /// to it. With no revert after it, it is unless undone or gone.
    /// A state let go of by the history as too old counts as in effect.
    ///
    /// [`Editor::is_gone`]: varde_document::Editor::is_gone
    /// [`Editor::made_before`]: varde_document::Editor::made_before
    pub(crate) fn in_effect(&self, revision: Revision) -> bool {
        let editor = &self.editor;
        let revision = self.mcp.amended_to(revision);
        // The state looked from: the design now, then the states the
        // reverts brought back.
        let mut from: Option<Revision> = None;
        loop {
            let revert = (self.mcp.reverted.iter())
                .filter(|made| editor.made_before(revision, made.after))
                .filter(|made| match from {
                    None => editor.is_done(made.after),
                    Some(from) => {
                        !editor.made_before(from, made.after)
                            && !editor.is_undone(made.after)
                            && !editor.is_gone(made.after)
                    }
                })
                .max_by(|a, b| match editor.made_before(a.after, b.after) {
                    true => std::cmp::Ordering::Less,
                    false => std::cmp::Ordering::Greater,
                });
            match (revert, from) {
                (Some(made), _) if made.target == revision => return true,
                (Some(made), _) if editor.made_before(made.target, revision) => return false,
                (Some(made), _) => from = Some(made.target),
                (None, None) => return !editor.is_undone(revision) && !editor.is_gone(revision),
                // Whether `revision` is an ancestor of `from`: on the
                // history, `from` has those of the history before it,
                // and the undone after the undone; off it, it's taken to.
                (None, Some(from)) => {
                    return !editor.on_history(from)
                        || !(editor.is_gone(revision)
                            || (editor.is_done(from) && editor.is_undone(revision)));
                }
            }
        }
    }

    /// Whether the design now is `state`'s: its revision (through the
    /// links' amends), its document, or what a revert to it with nothing
    /// since made.
    pub(crate) fn shows_state(&self, state: &EditorState) -> bool {
        let now = self.editor.revision();
        self.mcp.amended_to(state.revision()) == now
            || Arc::ptr_eq(state.document(), &self.editor.snapshot())
            || (self.mcp.reverted.iter())
                .any(|made| made.after == now && made.target == state.revision())
    }

    /// Why reverting to `state` is refused now, if it is.
    pub(crate) fn revert_refusal(&self, state: &EditorState) -> Option<String> {
        if let Some(why) = &self.read_only {
            return Some(format!("Can't revert: the design is read-only ({why})"));
        }
        // A client's undo until it ends, applied or not.
        if self.mcp.any_unapplied() || self.mcp.undoing() {
            return Some(EDIT_IN_FLIGHT.to_owned());
        }
        if self.proposing() {
            return Some("Can't revert while the sketch's edits are being solved".to_owned());
        }
        if state.lineage() != self.editor.lineage() {
            return Some(REPLACED.to_owned());
        }
        None
    }

    /// Selects the feature numbered `number` in the Timeline, scrolled to
    /// it, if the design still has it.
    pub(crate) fn mcp_show_feature(&mut self, number: u64) {
        let features = self.editor.document().features();
        let Some(at) = features
            .iter()
            .position(|feature| feature.id.get() == number)
        else {
            self.toast = Some("It's no longer in the Timeline".to_owned());
            return;
        };
        let id = features[at].id;
        let last = features.len().saturating_sub(1).max(1);
        self.panel = varde_view::Panel::Timeline;
        self.look(varde_view::Look::SelectFeature(id));
        self.mcp.scroll = Some(at as f32 / last as f32);
    }
}
