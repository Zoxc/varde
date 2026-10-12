//! The `undo` tool (`notes/AgentSupport.md`, "Step 4", "The `undo` tool"): the
//! session's own newest edits taken back, as the user's Undo would take
//! them back (`Editor::undo`, once per step), so a link's amend goes with
//! its edit and ids wind back as for the user.
//!
//! An edit like the others: it goes through the edits' queue, the
//! popup and the bound (`edit.rs`, `ask.rs`, `waiting.rs`), checked at its
//! turn, not on arrival. Each step it takes back must be one of the
//! session's own [`ClientStep`]s, the newest it hasn't undone the state
//! now; any that isn't (an edit of the user's, their Undo or Redo, a
//! revert, another session's edit) refuses the whole undo. All its steps
//! go in one turn, or none.
//!
//! The steps undone are marked so (`ClientStep::undone`): never undone
//! again by the session, and not told it as the user's undo
//! (`changes::since`); a Redo of them by the user is the user's, and is
//! told.

use serde_json::json;
use varde_document::Revision;
use varde_mcp::tools::Call;

use super::edit::Refused;
use super::waiting::{Before, ClientStep, From, Undid, Wait, Waiter};
use crate::doc::Doc;

/// Why an undo is refused when the design isn't as the session's last
/// edit left it.
pub(crate) const CHANGED_SINCE: &str = "the design changed since your last edit or undo (the \
    user edited, undid, redid, a step you undid included, or reverted, or another session \
    edited), so nothing was undone; edit again instead";

/// Why an undo is refused when the session has no edit of its own in
/// effect.
pub(crate) const NOTHING_OF_YOURS: &str = "this session has made no edit of its own still in \
    effect to undo (one it undid that the user redid isn't its own to undo again)";

/// What an undo's result says when the user redid what it undid before
/// regeneration answered.
pub(crate) const REDONE: &str = "undone, then redone by the user";

/// What an undo takes back: indices into `Waiting::steps`, newest first.
#[derive(Debug)]
pub(super) struct Undoing {
    steps: Vec<usize>,
}

/// What undoing `steps` steps of `doc`'s design for the session `from`
/// came in takes back, or why it can't: each of the newest `steps`
/// steps of the history must be an edit of this session it hasn't
/// undone, the newest of them its newest.
pub(super) fn plan(doc: &Doc, steps: usize, from: &From) -> Result<Undoing, Refused> {
    let mcp = &doc.mcp;
    let mine = |step: &ClientStep| step.of(from.start, &from.session) && !step.undone;
    let newest = (mcp.steps.iter())
        .rposition(mine)
        .ok_or_else(|| Refused::not_applied(NOTHING_OF_YOURS))?;
    // The current state, then the undo history's, newest first.
    let history: Vec<Revision> = doc.editor.history().rev().map(|(at, _)| at).collect();
    if history.first() != Some(&mcp.amended_to(mcp.steps[newest].after)) {
        return Err(Refused::not_applied(CHANGED_SINCE));
    }
    let mut taken = Vec::with_capacity(steps.min(history.len()));
    for at in 0..steps {
        // A step taken back leaves the state before it current: there
        // must be one.
        let (Some(&revision), true) = (history.get(at), at + 1 < history.len()) else {
            return Err(Refused::not_applied(format_args!(
                "the design's undo history reaches back only {}, so nothing was undone",
                crate::words::counted(history.len().saturating_sub(1), "step", "steps")
            )));
        };
        let step = (mcp.steps.iter())
            .rposition(|step| mine(step) && mcp.amended_to(step.after) == revision);
        match step {
            Some(step) => taken.push(step),
            None => {
                return Err(Refused::not_applied(format_args!(
                    "only {} of the design's newest steps {} this session's edits, so nothing \
                     was undone",
                    at,
                    match at {
                        1 => "is",
                        _ => "are",
                    }
                )));
            }
        }
    }
    Ok(Undoing { steps: taken })
}

/// The most edits an undo's result lists, and the most bytes of each
/// label: the rest counted, so the result stays well under the cap.
const LISTED: usize = 20;
const LABEL_BYTES: usize = 200;

/// The labels of the edits undone, newest first, for the result: at most
/// [`LISTED`], each cut to [`LABEL_BYTES`], with the total past that.
fn listed(labels: Vec<String>) -> serde_json::Value {
    let total = labels.len();
    let kept: Vec<String> = (labels.into_iter().take(LISTED))
        .map(|label| super::text::clipped(label, LABEL_BYTES))
        .collect();
    match total > LISTED {
        true => json!({ "items": kept, "total": total }),
        false => json!(kept),
    }
}

impl Doc {
    /// Takes back the steps the client's `undo` `call` under `tag`, from
    /// `from`, asks for, checked again on the design as it is now (what
    /// the user had open was closed first): all of them, then waiting
    /// for regeneration as an applied edit does, or none, refused.
    pub(super) fn client_undo(&mut self, tag: u64, from: From, call: Call) {
        let Call::Undo { steps } = call else {
            let refused = Refused::failed("internal error: not an undo");
            return self.edit_not_applied(tag, from, call, refused);
        };
        let undoing = match plan(self, steps, &from) {
            Ok(undoing) => undoing,
            Err(refused) => return self.edit_not_applied(tag, from, call, refused),
        };
        let before = self.editor.state();
        let mut undid = Undid {
            afters: Vec::with_capacity(undoing.steps.len()),
            shown: Vec::with_capacity(undoing.steps.len()),
            feature: None,
        };
        let mut labels = Vec::with_capacity(undoing.steps.len());
        for &at in &undoing.steps {
            let step = &self.mcp.steps[at];
            undid.afters.push(self.mcp.amended_to(step.after));
            undid.shown.push(step.shown.clone());
            undid.feature = undid.feature.or(step.feature);
            labels.push(step.label.clone());
        }
        for _ in &undoing.steps {
            self.editor.undo();
        }
        let now = self.editor.revision();
        for (&at, &after) in undoing.steps.iter().zip(&undid.afters) {
            let step = &mut self.mcp.steps[at];
            // Not the user's undo: told as neither undone nor, once the
            // user redoes it, anything but redone.
            step.undone = true;
            step.told_done = false;
            self.mcp.undone_to(after, now);
        }
        self.client_applied(tag, &from);
        self.mcp.add(Waiter {
            tag,
            from,
            call,
            waits: Wait::Regen {
                generation: self.editor.generation(),
                after: now,
                made: json!({ "undone": listed(labels) }),
                again: None,
                before: Before(before),
                undid: Some(undid),
                feature: None,
            },
        });
    }
}
