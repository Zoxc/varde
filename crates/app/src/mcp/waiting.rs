//! The MCP calls waiting in a [`Doc`]: those whose
//! answer needs something from the design's lanes first: a query or an
//! edit waiting for the model to catch up with the design (`Wait::Model`),
//! `body_topology` waiting on its edge facts, `measure` on regen's
//! measure, `sketch` on the solve
//! lane's analysis of its sketch, and the edits (`edit.rs`),
//! queued behind the user's sketch edits or applied and waiting on
//! regeneration. Each is answered within
//! [`BOUND`] of its arrival, or with an error saying the model took too
//! long; one ended otherwise (a cancel, the session's end, the server
//! stopping, the design closing) is answered with why.
//!
//! The table doesn't send replies itself: a call done is put by with
//! its reply ([`Waiting::finished`]) for [`Mcp::sync`](super::Mcp::sync)
//! to send and to mark in the log, after each message.

use std::sync::Arc;
use std::time::Duration;

use iced::futures::channel::oneshot;
use varde_render::PreviewImage;
use varde_view::PreviewRequest;

use iced::time::Instant;
use serde_json::{Value, json};
use varde_document::{EditorState, FeatureId, Generation, Revision};
use varde_mcp::rpc::Id;
use varde_mcp::server::Reply;
use varde_mcp::session::SessionId;
use varde_mcp::tools::{Call, ToolReply};
use varde_view::McpCallState;

use varde_regen::{Answer, Response};
use varde_sketch::Analysis;

use super::edit::{Made, Refused};
use super::push_bounded;
use super::query::{self, Asked};
use crate::doc::Doc;

/// How long a call may wait, from its arrival: clients give up on theirs
/// after 60 s (Codex, the TypeScript SDK), so Varde answers well before.
pub(crate) const BOUND: Duration = Duration::from_secs(30);

/// The answer to a call that waited [`BOUND`].
pub(crate) const TOOK_TOO_LONG: &str =
    "The model took too long to answer (over 30 s): ask again later";

/// The answer to a `body_topology` call whose model moved on twice
/// while its edges were read.
pub(crate) const MOVED_ON: &str =
    "The model changed while its edges were read: ask again once it settles";

/// The answer to an edit that waited [`BOUND`] behind the user's sketch
/// edits, never applied: why, for [`Refused::NotApplied`].
pub(crate) const NOT_APPLIED_BUSY: &str =
    "the design was busy with the user's sketch edits for over 30 s";

/// What an edit's result says when the app took it up past
/// [`BOUND`], its time queued before then counted: why, for
/// [`Refused::NotApplied`].
pub(crate) const NOT_APPLIED_LATE: &str = "Varde took over 30 s to take the call up";

/// The answer to an edit that waited [`BOUND`] for the model to catch
/// up with the design, never applied: why, for [`Refused::NotApplied`].
pub(crate) const NOT_APPLIED_MODEL: &str = "the model took over 30 s to catch up with the design";

/// Why an edit proposed to the sketch solver (`add_link`) isn't applied
/// when the solver hasn't answered within [`BOUND`], for
/// [`Refused::NotApplied`].
pub(crate) const NOT_APPLIED_SOLVER: &str = "the sketch solver took over 30 s with the edit";

/// What an edit's result says once applied, when regeneration hasn't
/// answered within [`BOUND`].
pub(crate) const STILL_RUNNING: &str = "applied, regeneration still running; see design_summary";

/// The answer to a `sketch` call while the design's solve lane hasn't
/// started yet.
pub(crate) const NO_SOLVER: &str = "the sketch solver isn't running yet: ask again shortly";

/// The answer to a `sketch` call whose analysis went with a solve lane
/// replaced.
pub(crate) const SOLVER_RESTARTED: &str = "The sketch solver restarted: ask again";

/// The answer to a `render` call no frame was drawn for within [`BOUND`].
pub(crate) const NOT_DRAWN: &str = "No frame was drawn within 30 s, so there's no picture: the \
    window may be minimized, or there's no GPU to draw with";

/// The answer to a `render` call whose picture the viewport couldn't
/// draw.
pub(crate) const DRAW_FAILED: &str =
    "Varde couldn't draw the picture: the GPU refused it (see Varde's log)";

/// The answer to the calls of a server stopped since they came.
pub(crate) const SERVER_STOPPED: &str = "The server stopped";

/// The answer to the calls waiting as the design closes.
pub(crate) const DESIGN_CLOSED: &str = "The design was closed";

/// The answer to the calls of a session that ended.
pub(crate) const SESSION_ENDED: &str = "The session ended";

/// Why waiting calls end other than answered: the cause picks how each
/// wait is answered ([`Wait::ended`]), [`Ended::text`] the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// It waited [`BOUND`].
    Bound,
    /// Its client cancelled it.
    Cancelled,
    /// Its session ended.
    Session,
    /// The server it came by stopped.
    ServerStopped,
    /// The design closed.
    DesignClosed,
    /// Varde is quitting.
    Quitting,
    /// The solve lane its analysis went to was replaced.
    SolverRestarted,
    /// The user's sketch edits it was queued behind were dropped.
    QueueDropped,
}

impl Ended {
    /// The answer's words for a call ended so.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Ended::Bound => TOOK_TOO_LONG,
            Ended::Cancelled => "The call was cancelled",
            Ended::Session => SESSION_ENDED,
            Ended::ServerStopped => SERVER_STOPPED,
            Ended::DesignClosed => DESIGN_CLOSED,
            Ended::Quitting => varde_mcp::server::CLOSING,
            Ended::SolverRestarted => SOLVER_RESTARTED,
            Ended::QueueDropped => "The edits waiting on the design were dropped",
        }
    }
}

/// How many edits of a session may wait behind the one it has in flight
/// (`notes/AgentSupport.md`, "Edits one at a time"). Enough for a client to
/// send the next few steps of a plan (a sketch's extrude, its fillet,
/// a rename) without waiting for each answer, as one waiting on the
/// popup may hold them for a while; past that a client is sending
/// faster than the design can take edits, and each queued edit only
/// grows the batch one refusal fails together.
pub(crate) const QUEUED_EDITS: usize = 8;

/// Why an edit past [`QUEUED_EDITS`] isn't applied, for
/// [`Refused::NotApplied`].
pub(crate) const TOO_MANY_QUEUED: &str = "8 edits of this session already wait behind the \
    one in flight; wait for their answers first";

// The answers above spell out `BOUND` ("30 s") and `QUEUED_EDITS` ("8"):
// changing either means rewording them.
const _: () = assert!(BOUND.as_secs() == 30 && QUEUED_EDITS == 8);

/// The calls waiting, keyed by an app-side tag.
#[derive(Debug, Default)]
pub(crate) struct Waiting {
    calls: Vec<Waiter>,
    /// The calls done, with their replies, for `Mcp::sync` to send.
    finished: Vec<Done>,
    /// The tag the next call takes.
    tags: u64,
    /// The popup asking the user to finish what they have open before an
    /// edit, while it's up (`ask.rs`).
    pub(crate) ask: Option<super::ask::Ask>,
    /// The client's edits on the design's history, and the links' amends,
    /// the client's undos and the panel's reverts since.
    pub(crate) history: ClientHistory,
    /// The pictures `render` calls wait for the viewport to draw, as the
    /// table had them after the last message ([`Waiting::refresh_renders`]).
    renders: Vec<Arc<PreviewRequest>>,
    /// The answers to the renders asked for, under their calls' tags,
    /// until the app takes them to wait for ([`Waiting::take_renders`]).
    render_answers: Vec<(u64, oneshot::Receiver<Option<PreviewImage>>)>,
    /// The dialog asking before a revert from the panel that undoes
    /// later changes, while it's up (`revert.rs`).
    pub(crate) revert_ask: Option<super::revert::RevertAsk>,
    /// Where the Timeline is to scroll to, a share of the way down, for
    /// the panel's Show in Timeline: taken by the app after the message.
    pub(crate) scroll: Option<f32>,
}

/// The client's edits on the design's history and what moved them since:
/// what tells the client's steps from the user's (`changes.rs`), each list
/// oldest first and at most [`STEPS`] long.
#[derive(Debug, Default)]
pub(crate) struct ClientHistory {
    /// The client edits applied to the design.
    pub(crate) steps: std::collections::VecDeque<ClientStep>,
    /// The states the links' amend replaced, each with the one it
    /// replaced it with: what follows a state a session last saw that's
    /// since been amended to where it is on the history.
    pub(crate) amends: std::collections::VecDeque<(Revision, Revision)>,
    /// The states a client's `undo` took back, each with the state it
    /// went back to: what follows a state a session last saw past its
    /// own undo.
    pub(crate) undos: std::collections::VecDeque<(Revision, Revision)>,
    /// The reverts made from the panel: for the client's next result to
    /// name them, and for what's taken back (`Doc::in_effect`).
    pub(crate) reverted: std::collections::VecDeque<super::revert::Made>,
}

impl ClientHistory {
    /// A client's `undo` took the state `from` back to `to`: a session
    /// that last saw `from` is to see the client's undo as its own, not
    /// the user's (`changes.rs`).
    pub(crate) fn undone_to(&mut self, from: Revision, to: Revision) {
        push_bounded(&mut self.undos, STEPS, (from, to));
    }

    /// The state `revision` became through the links' amends since.
    pub(crate) fn amended_to(&self, mut revision: Revision) -> Revision {
        while let Some(&(_, next)) = (self.amends.iter()).find(|(before, _)| *before == revision) {
            revision = next;
        }
        revision
    }

    /// The links' amend replaced the state `before` with another, `now`:
    /// a step that had made `before` has made `now`.
    fn amended(&mut self, before: Revision, now: Revision) {
        push_bounded(&mut self.amends, STEPS, (before, now));
        for step in &mut self.steps {
            if step.after == before {
                step.after = now;
            }
        }
    }

    /// The client's edit `call`, from `from`, was applied, making
    /// `after`, its result naming the feature numbered `feature`, if
    /// any: kept as the client's step.
    pub(crate) fn applied(
        &mut self,
        from: &Caller,
        call: &Call,
        after: Revision,
        feature: Option<u64>,
    ) {
        let step = ClientStep {
            start: from.start,
            session: from.session.clone(),
            after,
            label: label(call),
            shown: super::text::tool(call).with(&super::text::plain(&super::text::args(call))),
            told_done: true,
            undone: false,
            feature,
        };
        push_bounded(&mut self.steps, STEPS, step);
    }
}

/// The design before a client's edit, kept with it while it waits on
/// regeneration; compared by revision.
#[derive(Debug, Clone)]
pub(crate) struct Before(pub(crate) EditorState);

impl PartialEq for Before {
    fn eq(&self, other: &Before) -> bool {
        self.0.revision() == other.0.revision()
    }
}

/// The most client edits [`ClientHistory::steps`] keeps: as many as the undo
/// history holds, so a step on it is known for the client's while it's
/// there.
pub(crate) const STEPS: usize = 200;

/// A client edit applied to the design.
#[derive(Debug, Clone)]
pub(crate) struct ClientStep {
    /// The server and session it came in.
    pub(crate) start: u64,
    pub(crate) session: SessionId,
    /// The revision it made (the links' amend moving it along).
    pub(crate) after: Revision,
    /// Its tool and arguments, as the client named them: "rename
    /// feature Extrude 1 to Base".
    pub(crate) label: String,
    /// The same as its row in the panel says it: "Renamed feature
    /// Extrude 1 to Base".
    pub(crate) shown: String,
    /// Whether it was in effect (not undone) when its session was last
    /// told of the user's changes.
    pub(crate) told_done: bool,
    /// Whether its own session took it back with `undo` (`undo.rs`):
    /// never its own to undo again, and not the user's undo to tell of.
    pub(crate) undone: bool,
    /// The number of the feature its result named, if any: what an
    /// undo's row shows in the Timeline.
    pub(crate) feature: Option<u64>,
}

impl ClientStep {
    /// Whether it came in the session `session` of the server `start`.
    pub(crate) fn of(&self, start: u64, session: &SessionId) -> bool {
        (self.start, &self.session) == (start, session)
    }
}

/// What a client's `undo` took back, newest first: kept with it while it
/// waits on regeneration and on its row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Undid {
    /// The revisions the edits undone had made.
    pub(crate) afters: Vec<Revision>,
    /// Each as its row said it: "Renamed feature Extrude 1 to Base".
    pub(crate) shown: Vec<String>,
    /// The feature the newest that named one named, for Show in Timeline.
    pub(crate) feature: Option<u64>,
}

/// A call waiting.
#[derive(Debug)]
pub(crate) struct Waiter {
    /// The app's own key for it: what a lane's answer comes tagged with.
    pub(crate) tag: u64,
    pub(crate) from: Caller,
    pub(crate) call: Call,
    /// What it waits for.
    pub(crate) waits: Wait,
}

/// What a call waits for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Wait {
    /// `body_topology`'s edge facts, `asked` of regen tagged with the
    /// call's tag; `again` once it was asked a second time, the model
    /// having moved on from the first.
    Topology { asked: Asked, again: bool },
    /// `measure`'s answer: regen's [`Query::Measure`] tagged with the
    /// call's tag, of the committed model of `generation` and `until`;
    /// `held`, the bodies holding the bodies named that joins merged.
    ///
    /// [`Query::Measure`]: varde_regen::Query::Measure
    Measure {
        generation: Generation,
        until: Option<FeatureId>,
        held: query::Held,
    },
    /// `sketch`'s degrees of freedom: the solve lane's client analysis
    /// (`Request::Analyse`, its `query` the call's tag) of the sketch `data`
    /// reads, which completes it ([`query::analysed`]).
    Analysis { data: Value },
    /// `render`'s picture, for the viewport's next frame to draw
    /// (`Doc::mcp_rendered`); `data` is the answer, the image to follow.
    Render { drawn: Drawn, data: Value },
    /// `render`'s picture drawn, its PNG with the IO lane
    /// (`Doc::mcp_encoded`).
    Encoding { data: Value },
    /// A query or an edit queued behind the user's sketch edits waiting
    /// on the solver (`Pending::Client`, under the call's tag): the
    /// design it's about is the one once they're committed.
    Queued,
    /// A query, or an edit not taken up yet, waiting for the model to
    /// catch up with the design it's about: for regen to have answered
    /// (model, draft or failure) at generation `at` or past, with no
    /// earlier client edit it waits behind still to be applied
    /// ([`Waiting::ready`]).
    Model { at: Generation },
    /// An edit waiting on the popup (`ask.rs`): the one it asks about,
    /// or one come while it's up, behind it in order. `editing` is what
    /// the user was doing, after "is": "editing Sketch 2".
    Asking { editing: String },
    /// An edit made as a sketch edit (`add_link`), proposed to the solve
    /// lane as the user's sketch edits are (a client's `Proposal`, under
    /// the call's tag), waiting for it to be accepted and committed, or
    /// refused (`Doc::client_proposed`). An `edit_sketch`'s holds what
    /// it's to make (`Made::Drawn`), the solver's answer completing it.
    Solving(Option<Made>),
    /// An edit applied, waiting for the first regeneration answer at
    /// `generation` or past it. `after` is the revision it made, `made`
    /// what its result says it made; `again`, if any, what's made of it
    /// again once regeneration answered, over `made`: a link's points and
    /// curves come with its relinking.
    Regen {
        generation: Generation,
        after: Revision,
        made: Value,
        again: Option<Made>,
        /// The design before it, for the panel's Revert.
        before: Before,
        /// An `undo`'s: what it took back.
        undid: Option<Undid>,
        /// The number of the feature it made or changed, if it names
        /// one: its row's Show in Timeline.
        feature: Option<u64>,
    },
}

impl Wait {
    /// How a call waiting so is answered when it ends otherwise (the
    /// bound, a cancel, the session's end, the server stopping, the
    /// design closing), `why`: a query and an edit not applied fail
    /// saying why; an edit applied stays so, and says regeneration
    /// hadn't answered, or that the user took it back (`done` says
    /// whether a revision is still done). `edits`: whether the call is
    /// an edit. Its answer if applied, else why not.
    fn ended(
        self,
        edits: bool,
        why: Ended,
        done: &impl Fn(Revision) -> bool,
    ) -> Result<ToolReply, Refused> {
        let text = why.text();
        match self {
            Wait::Render { .. } if why == Ended::Bound => Err(Refused::failed(NOT_DRAWN)),
            Wait::Topology { .. }
            | Wait::Measure { .. }
            | Wait::Analysis { .. }
            | Wait::Render { .. }
            | Wait::Encoding { .. } => Err(Refused::failed(text)),
            Wait::Queued | Wait::Model { .. } | Wait::Solving(_) if !edits => {
                Err(Refused::failed(text))
            }
            Wait::Solving(_) => Err(Refused::not_applied(match why {
                Ended::Bound => NOT_APPLIED_SOLVER,
                _ => text,
            })),
            Wait::Queued | Wait::Model { .. } => Err(Refused::not_applied(match (why, self) {
                (Ended::Bound, Wait::Queued) => NOT_APPLIED_BUSY,
                (Ended::Bound, _) => NOT_APPLIED_MODEL,
                _ => text,
            })),
            Wait::Asking { editing } => Err(match why {
                Ended::DesignClosed | Ended::ServerStopped | Ended::Quitting => {
                    Refused::not_applied(text)
                }
                // The bound, a cancel or the session's end: the user
                // was editing.
                Ended::Bound
                | Ended::Cancelled
                | Ended::Session
                | Ended::SolverRestarted
                | Ended::QueueDropped => super::ask::user_editing(&editing),
            }),
            Wait::Regen {
                mut made,
                after,
                undid,
                ..
            } => {
                made["result"] = match undid {
                    // An undo isn't taken back by the user's Undo of
                    // another step, only by their Redo of what it undid.
                    Some(undid) if undid.afters.iter().any(|after| done(*after)) => {
                        json!(super::undo::REDONE)
                    }
                    Some(_) => json!(STILL_RUNNING),
                    None if done(after) => json!(STILL_RUNNING),
                    None => json!(super::edit::UNDONE),
                };
                Ok(ToolReply::data(made))
            }
        }
    }
}

/// A render for the viewport to draw, compared by its `Arc`.
#[derive(Debug, Clone)]
pub(crate) struct Drawn(pub(crate) Arc<PreviewRequest>);

impl PartialEq for Drawn {
    fn eq(&self, other: &Drawn) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Where a call came from and when.
#[derive(Debug)]
pub(crate) struct Caller {
    /// Its row in the MCP log, if it's still there.
    pub(crate) serial: u64,
    /// The server it came from ([`super::Serve::start`]).
    pub(crate) start: u64,
    pub(crate) session: SessionId,
    pub(crate) id: Id,
    pub(crate) reply: Reply,
    /// When it came whole to the server ([`Reply::came`]): the [`BOUND`]
    /// runs from here, the time it queued for the app included.
    pub(crate) came: Instant,
}

impl Caller {
    /// Whether the call has waited [`BOUND`] by `now`.
    pub(crate) fn late(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.came) >= BOUND
    }

    /// Whether `other` came in the same session as this.
    fn same_session(&self, other: &Caller) -> bool {
        (self.start, &self.session) == (other.start, &other.session)
    }
}

impl Waiter {
    /// Whether it's a client edit still to be applied that the calls
    /// after it wait behind: queued behind the user's sketch edits or
    /// waiting for the model. One waiting on the popup holds none.
    fn holds(&self) -> bool {
        self.call.edits()
            && matches!(
                self.waits,
                Wait::Queued | Wait::Model { .. } | Wait::Solving(_)
            )
    }

    /// Whether it's a client edit still to be applied: one that
    /// [holds](Waiter::holds) those after it, or one waiting on the
    /// popup. What the bound on a session's edits counts, and what an
    /// edit of its session ending not applied fails.
    fn unapplied(&self) -> bool {
        self.call.edits()
            && matches!(
                self.waits,
                Wait::Queued | Wait::Model { .. } | Wait::Asking { .. } | Wait::Solving(_)
            )
    }

    /// Whether `self` waits behind `earlier`: an edit behind every
    /// earlier client edit still to be applied, a query behind its own
    /// session's (tags go up in arrival order).
    fn behind(&self, earlier: &Waiter) -> bool {
        earlier.tag < self.tag
            && earlier.holds()
            && (self.call.edits() || earlier.from.same_session(&self.from))
    }
}

/// The most bytes of a query's `pending_edit` ([`with_pending_edit`])
/// as JSON, the edit and what the user is doing each cut to fit: kept under the
/// cap beside the answer by [`query::BUDGET`].
pub(crate) const PENDING_BYTES: usize = 512;

/// A query's `reply`, answered while an edit of its session waits on
/// the popup, saying so: `pending_edit` in its structured content (and
/// so its text), or a line after its text.
fn with_pending_edit(reply: ToolReply, pending: String) -> ToolReply {
    reply.with_fields(vec![("pending_edit", json!(pending))])
}

/// `call`'s tool and arguments: "rename feature Extrude 1 to Base".
pub(crate) fn label(call: &Call) -> String {
    let args = super::text::plain(&super::text::args(call));
    match args.is_empty() {
        true => call.tool().to_owned(),
        false => format!("{} {args}", call.tool()),
    }
}

/// A call done, its reply still to send.
#[derive(Debug)]
pub(crate) struct Done {
    pub(crate) from: Caller,
    pub(crate) call: Call,
    pub(crate) reply: ToolReply,
    /// What its row is to say.
    pub(crate) state: McpCallState,
    /// An edit applied: the design before it and the revision it made,
    /// for the panel's Revert.
    pub(crate) edit: Option<(EditorState, Revision)>,
    /// An `undo` applied: what it took back, for its row.
    pub(crate) undid: Option<Undid>,
    /// The number of the feature an edit applied made or changed, if it
    /// names one, for its row.
    pub(crate) feature: Option<u64>,
}

impl Waiting {
    /// Whether a call waits.
    pub(crate) fn any(&self) -> bool {
        !self.calls.is_empty()
    }

    /// The tag the next call is to wait under.
    pub(crate) fn next_tag(&mut self) -> u64 {
        let tag = self.tags;
        // One per call: a u64 won't run out.
        self.tags = self.tags.wrapping_add(1);
        tag
    }

    /// Puts `waiter` waiting.
    pub(crate) fn add(&mut self, waiter: Waiter) {
        self.calls.push(waiter);
    }

    /// Takes the call waiting under `tag` out of the table, if one is.
    pub(crate) fn take(&mut self, tag: u64) -> Option<Waiter> {
        let at = self.calls.iter().position(|call| call.tag == tag)?;
        Some(self.calls.remove(at))
    }

    /// Puts `call`, from `from`, by as done, answered `reply`: its row
    /// answered, or failed for an error.
    /// A query's says so when an edit of its session waits on the
    /// popup ([`Waiting::pending_edit`]).
    pub(crate) fn answer(&mut self, from: Caller, call: Call, mut reply: ToolReply) {
        if !call.edits()
            && let Some(pending) = self.pending_edit(&from)
        {
            reply = with_pending_edit(reply, pending);
        }
        let state = match reply.is_error {
            true => McpCallState::Failed,
            false => McpCallState::Answered,
        };
        self.answer_as(from, call, reply, state);
    }

    /// Puts `call`, from `from`, by as done, answered `reply`, its row
    /// saying `state`.
    pub(crate) fn answer_as(
        &mut self,
        from: Caller,
        call: Call,
        reply: ToolReply,
        state: McpCallState,
    ) {
        self.finished.push(Done {
            from,
            call,
            reply,
            state,
            edit: None,
            undid: None,
            feature: None,
        });
    }

    /// Puts the edit `call`, from `from`, by as done and applied,
    /// answered `reply`, with the design `before` it and the revision it
    /// made, `after`; an `undo`'s with what it took back, `undid`; the
    /// number of the feature it made or changed, `feature`.
    pub(crate) fn answer_applied(
        &mut self,
        from: Caller,
        call: Call,
        reply: ToolReply,
        (before, after): (EditorState, Revision),
        undid: Option<Undid>,
        feature: Option<u64>,
    ) {
        self.finished.push(Done {
            from,
            call,
            reply,
            state: McpCallState::Applied,
            edit: Some((before, after)),
            undid,
            feature,
        });
    }

    /// Whether a client's `undo` waits, applied or not: the panel's
    /// Revert is refused until it ends.
    pub(crate) fn undoing(&self) -> bool {
        (self.calls.iter()).any(|waiter| matches!(waiter.call, Call::Undo { .. }))
    }

    /// What the first edit of `from`'s session waiting on the popup is,
    /// if one does: its tool and arguments, and whom it waits on.
    fn pending_edit(&self, from: &Caller) -> Option<String> {
        let (waiter, editing) = (self.calls.iter())
            .filter(|waiter| waiter.from.same_session(from))
            .find_map(|waiter| match &waiter.waits {
                Wait::Asking { editing } => Some((waiter, editing)),
                _ => None,
            })?;
        // Each part cut to its share of [`PENDING_BYTES`] as JSON (quotes
        // escaped), with an ellipsis where it was: the arguments may be
        // kilobytes, a name in `editing` one.
        let short = |text: &str, bytes: usize| {
            let json = |c: char| serde_json::to_string(&c).map_or(6, |c| c.len() - 2);
            if text.chars().map(json).sum::<usize>() <= bytes {
                return text.to_owned();
            }
            let mut room = bytes - '…'.len_utf8();
            let mut kept: String = (text.chars())
                .take_while(|&c| match room.checked_sub(json(c)) {
                    Some(left) => {
                        room = left;
                        true
                    }
                    None => false,
                })
                .collect();
            kept.push('…');
            kept
        };
        let edit = short(&label(&waiter.call), PENDING_BYTES / 2);
        let editing = short(editing, PENDING_BYTES / 2 - 64);
        Some(format!(
            "{edit}: waiting for the user to let it apply, as they are {editing}"
        ))
    }

    /// Whether `from`'s session has [`QUEUED_EDITS`] edits waiting behind
    /// the one in flight: edits still to be applied
    /// ([`Waiter::unapplied`]) past the first.
    pub(crate) fn edits_full(&self, from: &Caller) -> bool {
        let unapplied = (self.calls.iter())
            .filter(|waiter| waiter.from.same_session(from) && waiter.unapplied())
            .count();
        unapplied > QUEUED_EDITS
    }

    /// Whether a client edit, of any session, is still to be applied
    /// ([`Waiter::unapplied`]): one coming now may be written assuming
    /// it.
    pub(crate) fn any_unapplied(&self) -> bool {
        self.calls.iter().any(Waiter::unapplied)
    }

    /// Puts the edit `call` under `tag`, from `from`, by as `refused`,
    /// and fails the edits of its session after it still to be applied
    /// ([`Waiter::unapplied`]): they were written assuming it. Those go
    /// not applied with its reason. The caller takes them out of the
    /// proposals' queue (`Doc::drop_ended_edits`).
    pub(crate) fn not_applied(&mut self, tag: u64, from: Caller, call: Call, refused: Refused) {
        let why = format!(
            "an earlier edit of this session wasn't: {}",
            refused.reason()
        );
        let (failed, kept) =
            std::mem::take(&mut self.calls)
                .into_iter()
                .partition(|waiter: &Waiter| {
                    waiter.tag > tag && waiter.from.same_session(&from) && waiter.unapplied()
                });
        self.calls = kept;
        let (reply, state) = refused.answer();
        self.answer_as(from, call, reply, state);
        for waiter in failed {
            self.cancel_ask(waiter.tag);
            let (reply, state) = Refused::not_applied(&why).answer();
            self.answer_as(waiter.from, waiter.call, reply, state);
        }
    }

    /// The popup's call, if it's `tag`'s, ended: the popup stays, saying
    /// so.
    fn cancel_ask(&mut self, tag: u64) {
        if let Some(ask) = &mut self.ask
            && ask.tag == tag
        {
            ask.cancelled = true;
        }
    }

    /// Ends the calls `which` picks, answered as their wait has them
    /// answered given `why` ([`Wait::ended`]), `done` saying whether a
    /// revision an edit made is still done.
    pub(crate) fn end(
        &mut self,
        which: impl Fn(&Caller) -> bool,
        why: Ended,
        done: impl Fn(Revision) -> bool,
    ) {
        self.end_where(|waiter| which(&waiter.from), why, done);
    }

    /// Ends the calls `which` picks, as [`Waiting::end`] does.
    fn end_where(
        &mut self,
        which: impl Fn(&Waiter) -> bool,
        why: Ended,
        done: impl Fn(Revision) -> bool,
    ) {
        // At the bound, each ends on its own: those of its session
        // behind one ending not applied fail with its reason rather than
        // their own ([`Waiting::not_applied`] takes them out first). The
        // other ends (a cancel, the session's end, the server stopping,
        // the design closing) are every call's own reason.
        // They end in arrival order (tags go up in it), not the table's:
        // one taken out and put back (the popup, the proposals' queue)
        // sits at its end.
        if why == Ended::Bound {
            while let Some((at, _)) = (self.calls.iter().enumerate())
                .filter(|(_, waiter)| which(waiter))
                .min_by_key(|(_, waiter)| waiter.tag)
            {
                let waiter = self.calls.remove(at);
                self.cancel_ask(waiter.tag);
                self.end_one(waiter, why, &done);
            }
            return;
        }
        let (ended, kept) = std::mem::take(&mut self.calls)
            .into_iter()
            .partition(|waiter| which(waiter));
        self.calls = kept;
        for waiter in ended {
            self.cancel_ask(waiter.tag);
            self.end_one(waiter, why, &done);
        }
    }

    /// Answers `waiter`, taken out of the table, as its wait has it
    /// answered given `why` ([`Wait::ended`]): an edit not applied fails
    /// the edits of its session after it ([`Waiting::not_applied`]).
    fn end_one(&mut self, waiter: Waiter, why: Ended, done: &impl Fn(Revision) -> bool) {
        let Waiter {
            tag,
            from,
            call,
            waits,
        } = waiter;
        let edits = call.edits();
        let mut waits = waits;
        let edit = match &mut waits {
            Wait::Regen {
                before,
                after,
                undid,
                feature,
                ..
            } => {
                // What an undo took back, through the links' amends since
                // (a Redo brings back the state they amended).
                for undone in undid.iter_mut().flat_map(|undid| &mut undid.afters) {
                    *undone = self.history.amended_to(*undone);
                }
                Some(((before.0.clone(), *after), undid.clone(), *feature))
            }
            _ => None,
        };
        match waits.ended(edits, why, done) {
            Ok(reply) => match edit {
                Some((edit, undid, feature)) => {
                    self.answer_applied(from, call, reply, edit, undid, feature)
                }
                None => self.answer_as(from, call, reply, McpCallState::Applied),
            },
            Err(refused) if edits => self.not_applied(tag, from, call, refused),
            Err(refused) => {
                let (reply, state) = refused.answer();
                self.answer_as(from, call, reply, state);
            }
        }
    }

    /// The tags of the edits waiting on the popup, in the order they
    /// came. Taken up one at a time ([`Waiting::take_asking`]), each
    /// stays in the table until its turn, so one ending not applied
    /// fails those of its session behind it ([`Waiting::not_applied`]).
    pub(crate) fn asking(&self) -> Vec<u64> {
        (self.calls.iter())
            .filter(|waiter| matches!(waiter.waits, Wait::Asking { .. }))
            .map(|waiter| waiter.tag)
            .collect()
    }

    /// Takes the edit under `tag` out of the table if it still waits on
    /// the popup: not ended meanwhile, nor asking again.
    pub(crate) fn take_asking(&mut self, tag: u64) -> Option<Waiter> {
        let at = (self.calls.iter())
            .position(|waiter| waiter.tag == tag && matches!(waiter.waits, Wait::Asking { .. }))?;
        Some(self.calls.remove(at))
    }

    /// Whether the call under `tag` waits queued behind the user's
    /// sketch edits (not ended).
    pub(crate) fn queued(&self, tag: u64) -> bool {
        (self.calls.iter()).any(|waiter| waiter.tag == tag && waiter.waits == Wait::Queued)
    }

    /// Whether the call under `tag` waits on the solve lane with its
    /// sketch edit (not ended).
    pub(crate) fn solving(&self, tag: u64) -> bool {
        (self.calls.iter())
            .any(|waiter| waiter.tag == tag && matches!(waiter.waits, Wait::Solving(_)))
    }

    /// Whether `waiter` (waiting or not) waits behind an earlier client
    /// edit still to be applied ([`Waiter::behind`]).
    fn blocked(&self, waiter: &Waiter) -> bool {
        self.calls.iter().any(|earlier| waiter.behind(earlier))
    }

    /// The first call, in arrival order, waiting for the model that
    /// `answered` (regen's newest answer) has caught up with and no
    /// earlier edit holds ([`Waiting::blocked`]).
    fn ready(&self, answered: Option<Generation>) -> Option<u64> {
        (self.calls.iter())
            .filter(|waiter| match waiter.waits {
                Wait::Model { at } => answered.is_some_and(|answered| answered >= at),
                _ => false,
            })
            .filter(|waiter| !self.blocked(waiter))
            .map(|waiter| waiter.tag)
            .min()
    }

    /// The client's edit under `tag`, from `from`, was applied, making
    /// `generation`: the calls of its session that came after it and
    /// wait for the model are to see it.
    fn raise_after(&mut self, tag: u64, from: &Caller, generation: Generation) {
        for waiter in &mut self.calls {
            if let Wait::Model { at } = &mut waiter.waits
                && waiter.tag > tag
                && waiter.from.same_session(from)
            {
                *at = (*at).max(generation);
            }
        }
    }

    /// Takes the edits waiting on regeneration that it has answered,
    /// at `answered` or past: the generation of the response applied
    /// last.
    pub(crate) fn regenerated(&mut self, answered: Generation) -> Vec<Waiter> {
        let (done, kept) = std::mem::take(&mut self.calls)
            .into_iter()
            .partition(|waiter| {
                matches!(waiter.waits, Wait::Regen { generation, .. } if generation <= answered)
            });
        self.calls = kept;
        done
    }

    /// The links' amend replaced the state `before` with `now`: an edit
    /// waiting that had made `before` has made `now`.
    pub(crate) fn amended(&mut self, before: Revision, now: Revision) {
        if before == now {
            return;
        }
        self.history.amended(before, now);
        for waiter in &mut self.calls {
            if let Wait::Regen { after, .. } = &mut waiter.waits
                && *after == before
            {
                *after = now;
            }
        }
    }

    /// Takes the pictures waited for into [`Waiting::renders`]: what the
    /// viewport draws next.
    fn refresh_renders(&mut self) {
        self.renders = (self.calls.iter())
            .filter_map(|waiter| match &waiter.waits {
                Wait::Render { drawn, .. } => Some(drawn.0.clone()),
                _ => None,
            })
            .collect();
    }

    /// The pictures for the viewport to draw on its next frame.
    pub(crate) fn renders(&self) -> &[Arc<PreviewRequest>] {
        &self.renders
    }

    /// The answers to the renders asked for since last taken, under their
    /// calls' tags, for the app to wait for and hand back to
    /// [`Doc::mcp_rendered`].
    pub(crate) fn take_renders(&mut self) -> Vec<(u64, oneshot::Receiver<Option<PreviewImage>>)> {
        std::mem::take(&mut self.render_answers)
    }

    /// The renders waiting to be drawn, under their calls' tags.
    #[cfg(test)]
    pub(crate) fn rendering(&self) -> Vec<(u64, Arc<PreviewRequest>)> {
        (self.calls.iter())
            .filter_map(|waiter| match &waiter.waits {
                Wait::Render { drawn, .. } => Some((waiter.tag, drawn.0.clone())),
                _ => None,
            })
            .collect()
    }

    /// Whether a call waits for its PNG from the IO lane, under `tag`.
    #[cfg(test)]
    pub(crate) fn encoding(&self, tag: u64) -> bool {
        (self.calls.iter())
            .any(|waiter| waiter.tag == tag && matches!(waiter.waits, Wait::Encoding { .. }))
    }

    /// The calls done since last asked, with their replies.
    pub(crate) fn finished(&mut self) -> Vec<Done> {
        std::mem::take(&mut self.finished)
    }
}

impl Doc {
    /// Whether an MCP call waits on this design: what the tick that
    /// bounds the wait runs for.
    pub(crate) fn mcp_waiting(&self) -> bool {
        self.mcp.any()
    }

    /// Takes the MCP `call`, from `from`: answered now, or put waiting
    /// for what it needs.
    pub(crate) fn mcp_call(&mut self, call: Call, from: Caller) {
        if call.edits() {
            return self.mcp_edit(call, from);
        }
        let tag = self.mcp.next_tag();
        self.client_take_up(tag, from, call);
    }

    // How a call reaches its answer, the one place for the order rules:
    // `client_take_up` queues it behind the user's sketch edits on the
    // solver if any wait; at its turn there (`client_queued_turn`), or
    // at once with none, `client_turn` puts it waiting for the model and
    // behind earlier client edits unless both hold, and `mcp_ready` takes
    // it up once they do; then it's answered or, an edit, made and
    // applied (`client_edit_now`). An edit coming while the popup is up
    // waits on it first (`ask.rs`), then is taken up here.

    /// Takes up the client's query or edit `call` under `tag`: queued
    /// behind the user's sketch edits waiting on the solver if any do
    /// (`Wait::Queued`), else as [`Doc::client_turn`].
    pub(super) fn client_take_up(&mut self, tag: u64, from: Caller, call: Call) {
        if self.proposing() {
            self.proposals.wait_client(tag);
            return self.mcp.add(Waiter {
                tag,
                from,
                call,
                waits: Wait::Queued,
            });
        }
        self.client_turn(tag, from, call);
    }

    /// The MCP call queued under `tag` has come to its turn in the
    /// proposals' queue, those before it committed: taken up
    /// ([`Doc::client_turn`]) if its call still waits and hasn't waited
    /// [`BOUND`] (the tick that ends it may not have come yet). An edit
    /// whose model isn't in yet, with no earlier client edit it waits
    /// behind, is held there instead, so the user's edits queued after
    /// it can't go first ([`Doc::client_holds_queue`]): whether it is,
    /// for the queue to keep it first and stop.
    pub(crate) fn client_queued_turn(&mut self, tag: u64) -> bool {
        if !self.mcp.queued(tag) {
            return false;
        }
        if self.client_holds_queue(tag) {
            return true;
        }
        let Some(mut waiter) = self.mcp.take(tag) else {
            return false;
        };
        // Late with its model not in, it was holding the queue for it.
        let at = self.editor.generation();
        if waiter.call.edits() && !self.model_ready(at) {
            waiter.waits = Wait::Model { at };
        }
        let Some(waiter) = self.unless_late(waiter) else {
            return false;
        };
        self.client_turn(waiter.tag, waiter.from, waiter.call);
        false
    }

    /// Whether the client's edit queued under `tag`, at its turn, keeps
    /// its place first in the proposals' queue: its model isn't in, it
    /// waits behind no earlier client edit (which goes first, see
    /// [`Doc::mcp_ready`]) and it hasn't waited [`BOUND`]. Taken up
    /// otherwise, it would wait for the model apart and be queued again
    /// behind whatever the user queued meanwhile, at every turn while
    /// they sketch on, out of the order things happened.
    fn client_holds_queue(&self, tag: u64) -> bool {
        let Some(waiter) = self.mcp.calls.iter().find(|waiter| waiter.tag == tag) else {
            return false;
        };
        waiter.call.edits()
            && !self.model_ready(self.editor.generation())
            && !self.mcp.blocked(waiter)
            && !waiter.from.late(Instant::now())
    }

    /// The client's query or edit `call` under `tag`, at its turn after
    /// the user's sketch edits: answered on a model of the design as it
    /// is now, so it waits for one (`Wait::Model`), and behind the
    /// earlier client edits it waits behind ([`Waiter::behind`]), unless
    /// both hold; then a query is answered, an edit made and applied
    /// ([`Doc::client_edit_now`]).
    pub(super) fn client_turn(&mut self, tag: u64, from: Caller, call: Call) {
        let at = self.editor.generation();
        let waiter = Waiter {
            tag,
            from,
            call,
            waits: Wait::Model { at },
        };
        if !self.model_ready(at) || self.mcp.blocked(&waiter) {
            return self.mcp.add(waiter);
        }
        self.client_now(waiter);
    }

    /// Answers the query `waiter` waits with, or makes and applies its
    /// edit: what it waited for holds.
    fn client_now(&mut self, waiter: Waiter) {
        let Waiter {
            tag, from, call, ..
        } = waiter;
        match call.edits() {
            true => self.client_edit_now(tag, from, call),
            false => self.mcp_query(from, call),
        }
    }

    /// Whether each revision the applied edits among `waiters` ask
    /// after (an edit's after, what an undo took back through the links'
    /// amends) is in effect ([`Doc::in_effect`]), worked out before they
    /// end, as [`Wait::ended`] takes it: a revert from the panel takes an
    /// edit back as an undo does.
    fn in_effect_of<'a>(
        &self,
        waiters: impl Iterator<Item = &'a Waiter>,
    ) -> std::collections::HashMap<Revision, bool> {
        let mut known = std::collections::HashMap::new();
        for waiter in waiters {
            if let Wait::Regen { after, undid, .. } = &waiter.waits {
                known.insert(*after, self.in_effect(*after));
                for undone in undid.iter().flat_map(|undid| &undid.afters) {
                    let undone = self.mcp.history.amended_to(*undone);
                    known.insert(undone, self.in_effect(undone));
                }
            }
        }
        known
    }

    /// `waiter`, taken out of the table, unless it has waited [`BOUND`]
    /// (its tick not come yet): then it's ended as the tick would end it.
    pub(super) fn unless_late(&mut self, waiter: Waiter) -> Option<Waiter> {
        if !waiter.from.late(Instant::now()) {
            return Some(waiter);
        }
        let in_effect = self.in_effect_of(std::iter::once(&waiter));
        let done = |after| in_effect.get(&after).copied().unwrap_or(true);
        (self.mcp).end_one(waiter, Ended::Bound, &done);
        let _ = self.drop_ended_edits();
        None
    }

    /// Answers the client's edit `call` under `tag`, from `from`, as
    /// `refused`, failing the edits of its session after it still to be
    /// applied with its reason ([`Waiting::not_applied`]), and takes
    /// those out of the proposals' queue. How an edit not in the table
    /// ends unapplied; one in it ends so through [`Waiting::end_one`].
    pub(super) fn edit_not_applied(
        &mut self,
        tag: u64,
        from: Caller,
        call: Call,
        refused: Refused,
    ) {
        self.mcp.not_applied(tag, from, call, refused);
        let _ = self.drop_ended_edits();
    }

    /// Whether regen has answered (model, draft or failure) at
    /// generation `at` or past it: what a call waiting for the model
    /// waits for.
    pub(crate) fn model_ready(&self, at: Generation) -> bool {
        self.feed.answered().is_some_and(|answered| answered >= at)
    }

    /// The client's edit under `tag`, from `from`, was applied: the
    /// calls of its session after it waiting for the model are to see it.
    pub(super) fn client_applied(&mut self, tag: u64, from: &Caller) {
        (self.mcp).raise_after(tag, from, self.editor.generation());
    }

    /// Answers the query `call`, from `from`, from the design as it is:
    /// `body_topology` goes on to wait for its edge facts, `measure` for
    /// regen's measure, `sketch` for its analysis.
    fn mcp_query(&mut self, from: Caller, call: Call) {
        match &call {
            Call::BodyTopology(args) => match query::body_topology(self, args) {
                Ok(asked) => self.ask_topology(from, call, asked, false),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            Call::Sketch(args) => match query::sketch(self, args) {
                Ok(read) => self.ask_analysis(from, call, read),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            Call::Measure(args) => match query::measure(self, args) {
                Ok((inspect, held)) => self.ask_measure(from, call, inspect, held),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            Call::Render(args) => match query::render(self, args) {
                Ok(shot) => self.ask_render(from, call, shot),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            _ => {
                let reply = answer_now(self, &call);
                self.mcp.answer(from, call, reply);
            }
        }
    }

    /// Ends the MCP calls waiting that `which` picks, `why`, as
    /// [`Waiting::end`] does, taking the edits of those queued out of the
    /// proposals' queue.
    ///
    /// Then takes up the calls waiting for the model that are ready
    /// ([`Doc::mcp_ready`]): run after every message (`Mcp::sync`), it's
    /// what answers them once regen or the solver has.
    pub(crate) fn mcp_end(&mut self, which: impl Fn(&Caller) -> bool, why: Ended) {
        // An edit holding the proposals' queue waits for its model
        // (`Doc::client_holds_queue`): ended, it says so.
        let at = self.editor.generation();
        if let Some(tag) = self.proposals.client_first()
            && !self.model_ready(at)
            && let Some(waiter) = (self.mcp.calls.iter_mut())
                .find(|waiter| waiter.tag == tag && waiter.waits == Wait::Queued)
            && which(&waiter.from)
        {
            waiter.waits = Wait::Model { at };
        }
        let in_effect =
            self.in_effect_of((self.mcp.calls.iter()).filter(|waiter| which(&waiter.from)));
        let done = |after| in_effect.get(&after).copied().unwrap_or(true);
        self.mcp.end(which, why, done);
        // One dropped may have held the queue: what's behind goes on.
        let dropped = self.drop_ended_edits();
        let taken = self.mcp_ready();
        if dropped || taken {
            self.sync();
        }
        self.mcp.refresh_renders();
    }

    /// Whether a picture waits for the viewport's next frame: frames are
    /// drawn meanwhile, as for a thumbnail.
    pub(crate) fn mcp_drawing(&self) -> bool {
        !self.mcp.renders.is_empty()
    }

    /// Takes up the calls waiting for the model that are ready, one at a
    /// time in arrival order ([`Waiting::ready`]), unless late
    /// ([`Doc::unless_late`]): a query is answered on the model it
    /// waited for, though the user edited on since; an edit is taken up
    /// again ([`Doc::client_take_up`]), as it's made on the design as it
    /// is now, first among what's queued on the solver if anything is
    /// (it was at its turn already). Then an edit holding the proposals'
    /// queue whose model is in goes on. Whether an edit was taken up,
    /// for the caller to sync: applied, or closing what the user had
    /// open, the model is to be asked for.
    pub(crate) fn mcp_ready(&mut self) -> bool {
        let mut taken = false;
        while let Some(tag) = self.mcp.ready(self.feed.answered()) {
            let Some(waiter) = self
                .mcp
                .take(tag)
                .and_then(|waiter| self.unless_late(waiter))
            else {
                continue;
            };
            match waiter.call.edits() {
                true if self.proposing() => {
                    self.proposals.wait_client_first(waiter.tag);
                    self.mcp.add(Waiter {
                        waits: Wait::Queued,
                        ..waiter
                    });
                    taken = true;
                }
                true => {
                    self.client_turn(waiter.tag, waiter.from, waiter.call);
                    taken = true;
                }
                false => self.client_now(waiter),
            }
        }
        if self.proposals.client_first().is_some() && self.model_ready(self.editor.generation()) {
            taken = true;
        }
        taken
    }

    /// Ends the MCP edits queued behind the user's sketch edits, `why`:
    /// what they waited behind was dropped. The queries queued there
    /// are about the design as it is now.
    pub(crate) fn mcp_end_queued(&mut self) {
        // None applied: no revision to ask after.
        let queued = |waiter: &Waiter| {
            waiter.call.edits() && matches!(waiter.waits, Wait::Queued | Wait::Solving(_))
        };
        (self.mcp).end_where(queued, Ended::QueueDropped, |_| true);
        let at = self.editor.generation();
        for waiter in &mut self.mcp.calls {
            if waiter.waits == Wait::Queued {
                waiter.waits = Wait::Model { at };
            }
        }
        let _ = self.drop_ended_edits();
    }

    /// Takes the client's edits whose calls ended out of the proposals'
    /// queue: never applied. Whether any went.
    fn drop_ended_edits(&mut self) -> bool {
        let mcp = &self.mcp;
        self.proposals
            .retain_clients(|tag| mcp.queued(tag) || mcp.solving(tag))
    }

    /// Asks regen for the edge facts `call` waits on, `asked`, and puts
    /// it waiting; `again` if it's the second time.
    fn ask_topology(&mut self, from: Caller, call: Call, asked: Asked, again: bool) {
        let query = varde_regen::Query::Topology(asked.body);
        self.ask_regen(from, call, query, |sent| {
            (sent == (asked.generation, asked.until)).then_some(Wait::Topology { asked, again })
        });
    }

    /// Asks regen to measure `inspect` on the committed model for the
    /// `measure` call `call` from `from`, and puts it waiting
    /// (`Wait::Measure`).
    fn ask_measure(
        &mut self,
        from: Caller,
        call: Call,
        inspect: varde_regen::Inspect,
        held: query::Held,
    ) {
        let query = varde_regen::Query::Measure(Box::new(inspect));
        self.ask_regen(from, call, query, |(generation, until)| {
            Some(Wait::Measure {
                generation,
                until,
                held,
            })
        });
    }

    /// Asks the viewport to draw `shot` for the `render` call `call` from
    /// `from` on its next frame, and puts it waiting (`Wait::Render`).
    fn ask_render(&mut self, from: Caller, call: Call, shot: query::Shot) {
        let tag = self.mcp.next_tag();
        let (send, answer) = oneshot::channel();
        let request = PreviewRequest::render(
            shot.mesh,
            shot.opacity,
            shot.tints,
            shot.shot,
            shot.selected_faces,
            shot.highlights,
            move |image| {
                // Unless the call ended meanwhile.
                let _ = send.send(image);
            },
        );
        self.mcp.render_answers.push((tag, answer));
        self.mcp.add(Waiter {
            tag,
            from,
            call,
            waits: Wait::Render {
                drawn: Drawn(request),
                data: shot.data,
            },
        });
        self.mcp.refresh_renders();
    }

    /// The picture of the `render` call under `tag` came back as `image`,
    /// or `None` if it couldn't be drawn: its PNG is asked of the IO lane
    /// (`Wait::Encoding`), or the call fails. One for no call waiting (it
    /// ended meanwhile) is dropped.
    pub(crate) fn mcp_rendered(
        &mut self,
        io: &mut crate::io::Io,
        tag: u64,
        image: Option<varde_io::thumbnail::Image>,
    ) {
        let Some(mut waiter) = self.mcp.take(tag) else {
            return;
        };
        let Wait::Render { data, .. } = waiter.waits else {
            self.mcp.add(waiter);
            return;
        };
        let Some(image) = image else {
            log::error!("Couldn't draw the picture an MCP client asked for");
            let reply = ToolReply::error(DRAW_FAILED);
            self.mcp.answer(waiter.from, waiter.call, reply);
            return self.mcp.refresh_renders();
        };
        io.send(varde_io::Request::EncodePng {
            owner: self.id.number(),
            tag,
            image,
        });
        waiter.waits = Wait::Encoding { data };
        self.mcp.add(waiter);
        self.mcp.refresh_renders();
    }

    /// The PNG of the `render` call under `tag` came back from the IO
    /// lane, or why it didn't: the call is answered with it as image
    /// content, unless it's past [`ToolReply::IMAGE_BYTES`]. One for no
    /// call waiting (it ended meanwhile) is dropped.
    pub(crate) fn mcp_encoded(&mut self, tag: u64, result: Result<Vec<u8>, String>) {
        let Some(waiter) = self.mcp.take(tag) else {
            return;
        };
        let Wait::Encoding { data } = waiter.waits else {
            return self.mcp.add(waiter);
        };
        let reply = match result {
            Ok(png) if png.len() <= ToolReply::IMAGE_BYTES => ToolReply::data(data).with_png(png),
            Ok(png) => ToolReply::error(format!(
                "the picture is {} KiB as a PNG, past the bound of {} KiB: ask for a \
                 smaller size",
                png.len() / 1024,
                ToolReply::IMAGE_BYTES / 1024
            )),
            Err(error) => {
                log::error!("Couldn't encode the picture an MCP client asked for: {error}");
                ToolReply::error(format!("Varde couldn't encode the picture: {error}"))
            }
        };
        self.mcp.answer(waiter.from, waiter.call, reply);
    }

    /// Asks regen `query` of the committed model for `call` from `from`
    /// ([`Request::Query`], queued in order beside exports: it neither
    /// replaces the user's measure riding on the regeneration nor is
    /// replaced by it), and puts it waiting as `waits` says of the
    /// generation and `until` sent. Without a committed model, or one
    /// other than `waits` wants, the call is answered saying so.
    ///
    /// [`Request::Query`]: varde_regen::Request::Query
    fn ask_regen(
        &mut self,
        from: Caller,
        call: Call,
        query: varde_regen::Query,
        waits: impl FnOnce((Generation, Option<FeatureId>)) -> Option<Wait>,
    ) {
        let tag = self.mcp.next_tag();
        match self.feed.request_query(tag, query).and_then(waits) {
            Some(waits) => self.mcp.add(Waiter {
                tag,
                from,
                call,
                waits,
            }),
            None => self
                .mcp
                .answer(from, call, ToolReply::error(query::REBUILDING)),
        }
    }

    /// Asks the solve lane to analyse the sketch `read` reads, for the
    /// `sketch` call `call` from `from`, and puts it waiting
    /// (`Wait::Analysis`): queued apart from the user's analysis, it
    /// neither replaces it nor is replaced by it. Without a lane yet (it
    /// starts with the design), the call fails saying so.
    fn ask_analysis(&mut self, from: Caller, call: Call, read: query::Read) {
        let Some(solver) = &mut self.solver else {
            let reply = ToolReply::error(NO_SOLVER);
            return self.mcp.answer(from, call, reply);
        };
        let tag = self.mcp.next_tag();
        let document = self.editor.document();
        solver.send(varde_solve::Request::Analyse {
            query: Some(tag),
            revision: self.editor.revision(),
            sketch: read.sketch,
            units: document.units(),
            params: document.params_shared(),
        });
        self.mcp.add(Waiter {
            tag,
            from,
            call,
            waits: Wait::Analysis { data: read.data },
        });
    }

    /// Ends the `sketch` calls waiting on the solve lane's analysis: the
    /// lane was replaced, and their requests with it.
    pub(crate) fn mcp_end_analyses(&mut self) {
        let analysing = |waiter: &Waiter| matches!(waiter.waits, Wait::Analysis { .. });
        (self.mcp).end_where(analysing, Ended::SolverRestarted, |_| true);
    }

    /// Takes the solve lane's answer to the client's analysis `id`, or
    /// why it failed: the `sketch` call waiting under that tag is
    /// answered with its degrees of freedom, or fails saying why. An
    /// answer for no call waiting (one ended meanwhile) is dropped.
    pub(crate) fn sketch_analysed(&mut self, id: u64, analysis: Result<&Analysis, String>) {
        let Some(waiter) = self.mcp.take(id) else {
            return;
        };
        let Waiter {
            from, call, waits, ..
        } = waiter;
        let reply = match (waits, analysis) {
            (Wait::Analysis { data }, Ok(analysis)) => query::analysed(data, analysis),
            (Wait::Analysis { .. }, Err(error)) => {
                log::error!("The solver failed to analyse a sketch for a client: {error}");
                ToolReply::error(format!("the solver failed to analyse the sketch: {error}"))
            }
            _ => ToolReply::error("internal error: only sketch waits on the solver's analysis"),
        };
        self.mcp.answer(from, call, reply);
    }

    /// Takes regen's answer to a [`Request::Query`] of a call waiting,
    /// by its tag, and answers the call: an answer for no call waiting
    /// (one ended meanwhile) is dropped, one of another generation,
    /// `until` or body than asked is an internal error.
    ///
    /// `body_topology` is answered from the committed model if it's
    /// still the one asked about; if it has moved on, the facts are
    /// asked for once more, of the model now, and past that the call is
    /// answered saying so. `measure` is answered with what was
    /// measured, saying of which model.
    ///
    /// [`Request::Query`]: varde_regen::Request::Query
    pub(crate) fn query_answered(&mut self, response: Response) {
        let Response::Answered {
            tag,
            generation,
            until,
            answer,
        } = response
        else {
            return;
        };
        let Some(waiter) = self.mcp.take(tag) else {
            return;
        };
        let Waiter {
            from, call, waits, ..
        } = waiter;
        const OTHER: &str = "internal error: regen answered another question than asked";
        let reply = match (&call, waits, answer) {
            (
                Call::BodyTopology(args),
                Wait::Topology { asked, again },
                Answer::Topology { body, result },
            ) => {
                let reply =
                    if (body, generation, until) == (asked.body, asked.generation, asked.until) {
                        let facts = result.as_deref().map_err(String::as_str);
                        query::listed(self, args, asked, facts)
                    } else {
                        Some(ToolReply::error(OTHER))
                    };
                match reply {
                    Some(reply) => reply,
                    None if !again => match query::body_topology(self, args) {
                        Ok(asked) => return self.ask_topology(from, call, asked, true),
                        Err(reply) => reply,
                    },
                    None => ToolReply::error(MOVED_ON),
                }
            }
            (
                Call::Measure(args),
                Wait::Measure {
                    generation: g,
                    until: u,
                    held,
                },
                Answer::Measured(result),
            ) if (g, u) == (generation, until) => {
                let inspected = result.as_deref().map_err(String::as_str);
                query::measured(self, args, (generation, until), held, inspected)
            }
            (Call::Measure(_), Wait::Measure { .. }, Answer::Measured(_)) => {
                ToolReply::error(OTHER)
            }
            _ => ToolReply::error(
                "internal error: only body_topology and measure wait on regen's answers",
            ),
        };
        self.mcp.answer(from, call, reply);
    }
}

/// The answer to `call` from `doc`, the open design, for the queries
/// answered at once; the others wait ([`Doc::mcp_query`]).
pub(super) fn answer_now(doc: &Doc, call: &Call) -> ToolReply {
    match call {
        Call::DesignSummary(page) => query::design_summary(doc, *page),
        Call::Feature { name, page } => query::feature(doc, name, *page),
        _ => ToolReply::error("internal error: not a query answered at once"),
    }
}
