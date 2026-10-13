//! Never under the user's hands (`agents/mcp.md`, "Edits"): a client
//! edit, at its turn, first closes the sketch or the
//! extrude, revolve, combine or motion session the user has open, new or
//! on a feature, and with it picking a plane, the Timeline marker's drag
//! and the measure tool, as Esc would. A panel keeps a working copy its
//! OK writes whole, which would undo the edit unsaid; a sketch rolls the
//! model back to it.
//!
//! With something unfinished there ([`Doc::unfinished`]: a sketch with a
//! value, label or drag open or its tool started, a panel building
//! another feature than it opened with, the rename field open or a
//! parameter being typed), a popup asks first ([`Ask`]): *Apply yours*
//! finishes the user's work as its OK would, or leaving the sketch would,
//! then applies the edit; refused, the popup says why and the call goes on
//! waiting. *Keep editing* refuses the call. One ended meanwhile (the
//! bound, a cancel, its session's end) answers that the user was editing,
//! and the popup stays, saying it was cancelled, with only Close. One
//! popup at a time: an edit coming while it's up waits behind it
//! ([`Wait::Asking`]), in order; but one coming while it shows cancelled
//! takes it over, asking about that edit, its buttons back (or, with
//! nothing unfinished any more, just applies).

use varde_mcp::tools::Call;
use varde_view::McpTool;

use super::edit::Refused;
use super::waiting::{Caller, Wait, Waiter};
use crate::doc::Doc;

/// The popup asking the user to finish what they have open before an
/// edit: [`Waiting::ask`](super::waiting::Waiting::ask).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ask {
    /// The call it asks about.
    pub(crate) tag: u64,
    /// What the user is doing, after "you're": "editing Sketch 2".
    pub(crate) editing: String,
    /// The edit asked about: its tool and its arguments, as typed.
    pub(crate) tool: McpTool,
    pub(crate) args: String,
    /// Why the user's work wasn't applied as Apply yours was tried.
    pub(crate) refused: Option<String>,
    /// Whether its call ended unapplied: the popup only says so.
    pub(crate) cancelled: bool,
}

impl Ask {
    /// The popup as the view shows it, from `client`.
    pub(crate) fn shown<'a>(&'a self, client: Option<&'a str>) -> varde_view::McpAsk<'a> {
        varde_view::McpAsk {
            client,
            editing: &self.editing,
            tool: self.tool,
            args: &self.args,
            refused: self.refused.as_deref(),
            cancelled: self.cancelled,
        }
    }
}

/// The refusal of an edit as the user was `editing` ("editing Sketch
/// 2").
pub(super) fn user_editing(editing: &str) -> Refused {
    Refused::not_applied(format_args!("the user is {editing}"))
}

impl Doc {
    /// What the user has open with something unfinished, if anything,
    /// after "you're": "renaming Body 1", "editing Sketch 2".
    pub(crate) fn unfinished(&self) -> Option<String> {
        let document = self.editor.document();
        if let Some(renaming) = &self.renaming {
            let name = document.name_of(renaming.target).unwrap_or_default();
            return Some(format!("renaming {name}"));
        }
        if let Some((index, _)) = self.param_typing() {
            return Some(match document.params().get(index) {
                Some(param) => format!("editing the parameter {}", param.name),
                None => "editing the parameters".to_owned(),
            });
        }
        if let Some(session) = self.mode.sketch() {
            let open = session.value.is_some()
                || session.label.is_some()
                || session.drag.is_some()
                || (session.tool.as_ref()).is_some_and(|tool| tool.started());
            if open {
                let name = (document.feature(session.feature))
                    .map_or("the sketch", |feature| feature.name.as_str());
                return Some(format!("editing {name}"));
            }
        }
        if self.operation_changed() {
            return Some(format!("editing {}", self.operation_name()?));
        }
        None
    }

    /// The operation being set up, by the name of the feature it edits or
    /// as a new one ("a new extrude"), if one is.
    fn operation_name(&self) -> Option<String> {
        let (feature, noun) = if let Some(session) = self.mode.extrude() {
            (session.feature, "extrude")
        } else if let Some(session) = self.mode.revolve() {
            (session.feature, "revolve")
        } else if let Some(session) = self.mode.combine() {
            (session.feature, "combine")
        } else {
            let session = self.mode.motion()?;
            (session.feature, session.kind.noun())
        };
        let document = self.editor.document();
        Some(match feature.and_then(|id| document.feature(id)) {
            Some(feature) => feature.name.clone(),
            None => format!("a new {}", noun.to_lowercase()),
        })
    }

    /// Closes what the user has open, as Esc would: the operation being
    /// set up, the sketch (left without asking for the model again, for
    /// `sync` to), picking a plane, the Timeline marker's drag and the
    /// measure tool. For a client's edit, with nothing unfinished.
    fn close_for_client(&mut self) {
        self.mode.take_picking_plane();
        self.rolling = None;
        self.mode.take_measure();
        self.close_operation();
        if self.mode.sketch().is_some() {
            self.end_session();
        }
    }

    /// Clears the way for the client's edit `call` under `tag`, at its
    /// turn: with the popup up it waits behind it; with something
    /// unfinished open the popup asks about it; else what's open closes
    /// and the edit goes on, given back. A popup whose call was
    /// cancelled doesn't hold it: see [`Doc::drop_cancelled_ask`].
    pub(super) fn clear_for_client(
        &mut self,
        tag: u64,
        from: Caller,
        call: Call,
    ) -> Option<(Caller, Call)> {
        if let Some(ask) = &self.mcp.ask {
            let editing = ask.editing.clone();
            self.mcp.add(Waiter {
                tag,
                from,
                call,
                waits: Wait::Asking { editing },
            });
            return None;
        }
        if let Some(editing) = self.unfinished() {
            self.mcp.ask = Some(Ask {
                tag,
                editing: editing.clone(),
                tool: super::text::tool(&call),
                args: super::text::plain(&super::text::args(&call)),
                refused: None,
                cancelled: false,
            });
            self.mcp.add(Waiter {
                tag,
                from,
                call,
                waits: Wait::Asking { editing },
            });
            return None;
        }
        self.close_for_client();
        Some((from, call))
    }

    /// Takes down the popup if its call was cancelled, for an edit at
    /// its turn, which doesn't wait on it: those waiting behind it go on
    /// first, in order (and may ask again). Whether any did, so the
    /// edit's commands are made again on the design they left.
    pub(super) fn drop_cancelled_ask(&mut self) -> bool {
        if !self.mcp.ask.as_ref().is_some_and(|ask| ask.cancelled) {
            return false;
        }
        self.mcp.ask = None;
        let behind = self.mcp.asking();
        let any = !behind.is_empty();
        self.release(behind);
        any
    }

    /// The popup's Apply yours: finishes the user's work as its OK would
    /// (or leaving the sketch would), then applies the edit asked about if
    /// its call still waits, and lets those behind it go on. Refused, the
    /// popup stays saying why, the call waiting.
    pub(crate) fn mcp_apply_yours(&mut self) {
        let Some(tag) = self.mcp.ask.as_ref().map(|ask| ask.tag) else {
            return;
        };
        if let Err(why) = self.finish_users() {
            if let Some(ask) = &mut self.mcp.ask {
                ask.refused = Some(why);
            }
            self.sync();
            return;
        }
        self.mcp.ask = None;
        if let Some(waiter) = self.mcp.take(tag) {
            self.edit_turn(waiter);
        }
        self.release_asking();
        self.sync();
    }

    /// The popup's Keep editing, or `Esc`: the edit asked about is
    /// refused, saying the user is editing, and those behind it go on
    /// (asking again). Also its Close, once the edit was cancelled: then
    /// only those behind it go on.
    pub(crate) fn mcp_keep_editing(&mut self) {
        let Some(ask) = self.mcp.ask.take() else {
            return;
        };
        if let Some(waiter) = self.mcp.take(ask.tag) {
            let refused = user_editing(&ask.editing);
            self.edit_not_applied(waiter.tag, waiter.from, waiter.call, refused);
        }
        self.release_asking();
        self.sync();
    }

    /// Finishes what the user has open, as Apply yours: the rename field
    /// and a parameter typed committed, the sketch left, the operation
    /// committed as its OK would if it changed (else closed), the rest
    /// closed. Why, if the operation's OK or the parameter was refused.
    fn finish_users(&mut self) -> Result<(), String> {
        self.commit_rename();
        if let Some((index, field)) = self.param_typing() {
            self.commit_param_typing();
            if let Some(why) = self.param_refusal(index, field) {
                return Err(format!("Your parameter wasn't changed: {why}"));
            }
        }
        if self.operation_changed() {
            let name = self.operation_name().unwrap_or_default();
            let name = name.strip_prefix("a ").unwrap_or(&name).to_owned();
            // Why it's refused is the editor's only if it says so now.
            self.edit_error = None;
            self.commit_operation(false);
            if self.operating() {
                let why = if let Some(error) = &self.edit_error {
                    error.to_string()
                } else if let Some(error) = self.feed.draft_error() {
                    format!("its preview fails ({error})")
                } else if self.proposing() {
                    "the sketch edits before it are still being solved".to_owned()
                } else {
                    "it isn't complete, or its preview hasn't come yet".to_owned()
                };
                return Err(format!(
                    "Your {name} wasn't applied: {why}. Fix it, or keep editing."
                ));
            }
        }
        self.close_for_client();
        Ok(())
    }

    /// Takes up the edit `waiter` waited with on the popup
    /// ([`Doc::client_take_up`]), unless it has waited the bound
    /// ([`Doc::unless_late`]: answered that the user was editing).
    fn edit_turn(&mut self, waiter: Waiter) {
        if let Some(Waiter {
            tag, from, call, ..
        }) = self.unless_late(waiter)
        {
            self.client_take_up(tag, from, call);
        }
    }

    /// Lets the edits waiting behind the popup go on, in order: the first
    /// that finds something unfinished asks again, and the rest wait
    /// behind it.
    fn release_asking(&mut self) {
        let behind = self.mcp.asking();
        self.release(behind);
    }

    /// Takes up the edits under `tags` that still wait on the popup, in
    /// order: one taken up and refused fails those of its session behind
    /// it, which are still in the table.
    fn release(&mut self, tags: Vec<u64>) {
        for tag in tags {
            if let Some(waiter) = self.mcp.take_asking(tag) {
                self.edit_turn(waiter);
            }
        }
    }
}
