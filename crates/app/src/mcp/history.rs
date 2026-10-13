//! The client's edits on a design's history ([`ClientHistory`]): its
//! steps, the links' amends, the client's undos and the panel's reverts
//! since, what tells the client's steps from the user's (`changes.rs`,
//! `undo.rs`, `revert.rs`).

use varde_document::{EditorState, Revision};
use varde_mcp::session::SessionId;
use varde_mcp::tools::Call;

use super::push_bounded;
use super::waiting::Caller;

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
    pub(super) fn amended(&mut self, before: Revision, now: Revision) {
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
            session: from.session.clone(),
            after,
            label: super::text::label(call),
            shown: super::text::shown(call),
            told_done: true,
            undone: false,
            feature,
        };
        push_bounded(&mut self.steps, STEPS, step);
    }
}

/// What a client's edit applied leaves for its row: kept with it while
/// it waits on regeneration, then with its answer; the design before it
/// compared by revision.
#[derive(Debug, Clone)]
pub(crate) struct Applied {
    /// The design before it, for the panel's Revert.
    pub(crate) before: EditorState,
    /// The revision it made.
    pub(crate) after: Revision,
    /// An `undo`'s: what it took back.
    pub(crate) undid: Option<Undid>,
    /// The number of the feature it made or changed, if it names one:
    /// its row's Show in Timeline.
    pub(crate) feature: Option<u64>,
}

impl PartialEq for Applied {
    fn eq(&self, other: &Applied) -> bool {
        self.before.revision() == other.before.revision()
            && self.after == other.after
            && self.undid == other.undid
            && self.feature == other.feature
    }
}

/// The most client edits [`ClientHistory::steps`] keeps: as many as the undo
/// history holds, so a step on it is known for the client's while it's
/// there.
pub(crate) const STEPS: usize = 200;

/// A client edit applied to the design.
#[derive(Debug, Clone)]
pub(crate) struct ClientStep {
    /// The session it came in.
    pub(crate) session: ClientSession,
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

/// A client's session: its id and the server it's of, as ids could
/// repeat across the servers started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientSession {
    /// The server ([`super::Serve::start`]).
    pub(crate) start: u64,
    pub(crate) id: SessionId,
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
