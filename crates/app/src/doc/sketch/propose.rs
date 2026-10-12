//! The solver lane's side of editing a sketch: every edit is proposed to
//! it and committed only once it's accepted, drags step through it, and
//! it analyses the sketch for its colours and the status bar. See "Only
//! valid edits are accepted" and "Dragging" in `notes/Threading.md`.
//!
//! One proposal is with the lane at a time; the rest queue behind it, each
//! proposed on the sketch the one before produced, so an edit made while
//! another waits (the next line of a chain) builds on it. Until they're
//! answered, the sketch is shown, and drawn on, with them applied
//! ([`Waiting`]). An accepted proposal commits its sketch, solved, as one
//! [`Command::SetSketch`]; a rejected one changes nothing and says why
//! until the next action, or, if its sketch was left meanwhile, in a
//! banner over the viewport until dismissed. Other changes to the document made meanwhile
//! wait behind them, in order, so what's committed keeps the order the
//! user made it in, and undo takes back the newest of what waits, dropping
//! it. A proposal answered for a revision no longer the document's
//! (something else was committed meanwhile) is proposed again.

use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;

use iced::time::Instant;
use varde_document::{Command, FeatureId, LinkSource, OutsideRef, Revision, Sketch};
use varde_expr::LengthUnit;
use varde_sketch::{Analysis, Design, Id, LinkKind, Rejected, SketchEdit};
use varde_solve::{Request, Response, Tag};
use varde_view::RefusedEdit;

use super::{Refusal, ValueEdit, Waiting};
use crate::doc::feed::SLOW;
use crate::doc::{Change, Doc, sketch_of};

/// Why the MCP client's proposal isn't applied when its sketch went
/// before its turn.
const SKETCH_GONE: &str = "the sketch was removed before the edit's turn";

/// Why the MCP client's sketch edit in steps isn't applied when the
/// sketch changed under it before it was all solved: its later steps
/// name the items the earlier ones made by the ids they were to get.
const SKETCH_CHANGED: &str =
    "the sketch changed while the edit was solved: read it again and send the edit anew";

/// Why the user's sketch edit isn't made when an MCP client's edit of
/// the sketch, queued before it and not shown when it was made, was
/// committed first: the ids it names may be the client's items now.
const TAKEN_BY_CLIENT: &str =
    "the MCP client's edit of the sketch, queued before it, was made first: draw it again";

/// How many analyses a session keeps, by revision, so undoing and redoing
/// shows them at once.
const ANALYSES: usize = 8;

/// What's asked of a document's solver lane and not answered yet.
#[derive(Debug, Default)]
pub(crate) struct Proposals {
    /// The proposal with the lane, and the revision it was proposed on.
    in_flight: Option<(Revision, Proposal)>,
    /// Proposals waiting for the one in flight, and other changes to the
    /// document waiting behind them, in the order made.
    queued: VecDeque<Pending>,
    /// Answers still to come to proposals dropped while with the lane,
    /// which are ignored: the lane answers proposals in the order sent.
    dropped: usize,
    /// When the proposals waiting started waiting.
    since: Option<Instant>,
    /// Whether they've waited [`SLOW`] or longer.
    slow: bool,
    /// The analysis asked of the lane and not answered yet, of which
    /// revision of which sketch. One at a time: the answer names only the
    /// revision, which two sketches share.
    analysing: Option<(Revision, FeatureId)>,
    /// The drag session the next drag is.
    next_drag: u64,
    /// Whether a change from the queue asks the user first (the delete
    /// prompt): nothing waiting behind it moves until they answer, see
    /// [`Doc::send_proposal`].
    asking: bool,
    /// How many MCP client edits were applied: what the user's proposal
    /// notes as it's made ([`Proposal::made_on`]), to tell a client's
    /// commit from a relink's amend once the sketch's `next_id` moved.
    client_edits: u64,
}

/// What waits for the proposal in flight, see [`Proposals::queued`].
#[derive(Debug, Clone)]
enum Pending {
    Proposal(Proposal),
    /// Made once the proposals before it are answered.
    Change(Change),
    /// An MCP client's edit, made from its call (in `Doc::mcp`, under
    /// `tag`) on the design as it is once the proposals before it are
    /// answered, and applied through the client's apply path, if the call
    /// still waits; an edit whose model isn't in yet keeps its place
    /// first, holding what's behind it, until it is. (A query queued
    /// goes on to wait apart.) It isn't shown until then,
    /// so the user's Undo passes over it ([`Doc::drop_newest`]).
    #[cfg(not(target_arch = "wasm32"))]
    Client {
        tag: u64,
    },
}

impl Pending {
    /// Whether it's the user's own, not an MCP client's.
    fn users(&self) -> bool {
        match self {
            Pending::Proposal(proposal) => proposal.client.is_none(),
            Pending::Change(_) => true,
            #[cfg(not(target_arch = "wasm32"))]
            Pending::Client { .. } => false,
        }
    }
}

/// An edit of a sketch to propose.
#[derive(Debug, Clone)]
struct Proposal {
    feature: FeatureId,
    edit: SketchEdit,
    /// The sketch to propose the edit on in place of the one committed,
    /// while the document is at the revision given: a drag's last
    /// solution, which the move goes on from.
    from: Option<(Revision, Arc<Sketch>)>,
    /// The design's units when it was made, which the values in it were
    /// read in and shown in: new units waiting before it may be set by
    /// the time it's proposed.
    units: LengthUnit,
    /// What the link the edit adds comes from, for a
    /// [`SketchEdit::AddLink`]: committed with it by
    /// [`Command::AddLink`].
    source: Option<OutsideRef>,
    /// The value field to open in place of committing the edit, if it's
    /// accepted: the edit only asks whether a dimension can be driving.
    probe: Option<Box<ValueEdit>>,
    /// Whether the edit's dimensions were made references, refused as
    /// driving, which a toast tells once it's committed.
    forced: bool,
    /// The MCP client's call it's for, by its tag in `Doc::mcp`, if it's
    /// a client's (`add_link`, `edit_sketch`): shown waiting, as the
    /// user works on the sketch with it ([`Proposals::waiting`]), but not
    /// theirs, so passed over by the user's Undo, and committed or refused through the client's
    /// apply path, answering the call ([`Doc::client_proposal_settled`]).
    /// Always `None` on the web, which serves no MCP.
    client: Option<u64>,
    /// The steps of the MCP client's sketch edit (`edit_sketch`) after
    /// this one, which go on from its solution, committed together once
    /// the last is accepted: one undo step.
    steps: Option<Box<Steps>>,
    /// The `next_id` of the sketch the user's edit was made on, as they
    /// worked on it (committed, with what waited applied), and
    /// [`Proposals::client_edits`] then; `None` for an MCP client's. Its
    /// ids name what that sketch had, the user's own edits waiting
    /// before it by the ids they were to get: if the sketch committed
    /// has gone past it by its turn and a client edit was applied since,
    /// that edit, queued before it, which they didn't see, took those
    /// ids. Gone past with none, it was a relink's amend (`relink.rs`,
    /// new ids for a link's new shape), which names nothing the user's
    /// edit does: it's proposed on the sketch relinked.
    made_on: Option<(u32, u64)>,
}

/// A step of the MCP client's sketch edit (`edit_sketch`): the edit,
/// and the operations of its call it was made from, the first and the
/// last.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub(crate) edit: SketchEdit,
    pub(crate) ops: (usize, usize),
    /// For a step placing a dimension, whether its value is what it
    /// measures (none was given): made on the sketch the operations
    /// before it leave applied, not solved, its side, and such a value,
    /// are taken again on the solution it's proposed on
    /// ([`remeasured`]).
    pub(crate) measured: Option<bool>,
}

/// Where the MCP client's sketch edit in steps is ([`Proposal::steps`]).
#[derive(Debug, Clone)]
struct Steps {
    /// The operations the step proposed was made from.
    ops: (usize, usize),
    /// The step proposed's [`Step::measured`].
    measured: Option<bool>,
    /// The steps after it, in order.
    rest: VecDeque<Step>,
    /// The operations whose dimensions were placed as references,
    /// refused as driving.
    referenced: Vec<usize>,
    /// The `next_id` of the sketch the steps were made on: on another,
    /// they'd name other items than they mean.
    next_id: u32,
    /// The committed sketch the first step was proposed on, once it is:
    /// the later steps go on from its solutions, and the last commits as
    /// one `SetSketch` replacing it, so they go on only while it's what's
    /// committed. Anything else in the design may change meanwhile.
    on: Option<Arc<Sketch>>,
    /// Whether the step proposed is past the first, going on from the
    /// solution before it (the proposal's `from`).
    later: bool,
}

impl Steps {
    /// Whether the sketch changed under the steps, `committed` now, the
    /// step proposed going on `from`: a later step goes on only from the
    /// solution of the step before, while the sketch committed is still
    /// the one the first was proposed on (a relinking that moved its
    /// linked geometry counts, the solutions having the old); the first
    /// only on the sketch the steps were made on.
    fn changed(&self, committed: &Sketch, from: &Option<(Revision, Arc<Sketch>)>) -> bool {
        match (from, &self.on, self.later) {
            (Some(_), Some(on), true) => **on != *committed,
            (_, _, true) => true,
            (_, _, false) => committed.next_id != self.next_id,
        }
    }

    /// Whether a later step goes on from its `from` with `committed` as
    /// it is now, whatever the revision.
    fn going_on(&self, committed: &Sketch) -> bool {
        self.later && self.on.as_deref().is_some_and(|on| *on == *committed)
    }

    /// The operations of the step proposed, for a refusal of it.
    fn said(&self) -> String {
        match self.ops {
            (first, last) if first == last => format!("operations[{first}]"),
            (first, last) => format!("operations[{first}] to [{last}]"),
        }
    }
}

/// An MCP client's sketch edit accepted: the command committing it, the
/// degrees of freedom the sketch has then, and the operations whose
/// dimensions were placed as references (an `edit_sketch`'s).
#[derive(Debug, Clone)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct Settled {
    pub(crate) command: Command,
    pub(crate) freedom: usize,
    pub(crate) referenced: Vec<usize>,
}

impl Proposals {
    /// Whether edits are waiting on the solver, or changes behind them,
    /// or the user's answer to a change from the queue.
    pub(crate) fn any(&self) -> bool {
        self.in_flight.is_some() || !self.queued.is_empty() || self.asking
    }

    /// Whether edits have waited on the solver long enough to say so, see
    /// [`SLOW`].
    pub(crate) fn slow(&self) -> bool {
        self.slow
    }

    /// Whether frames are wanted to tell when edits have waited long
    /// enough to say so.
    pub(crate) fn timing(&self) -> bool {
        self.any() && !self.slow && !self.asking
    }

    /// The proposals waiting, the one in flight first: the user's, and
    /// an MCP client's at its turn (its edit made, with the lane or its
    /// steps going on). The user's sketch is worked on with the client's
    /// applied, though it isn't theirs to undo: an edit they make behind
    /// it builds on it, as their next line builds on the one waiting,
    /// rather than naming by the ids it was to get what the client's
    /// commit takes first.
    fn waiting(&self) -> impl Iterator<Item = &Proposal> {
        let in_flight = self.in_flight.as_ref().map(|(_, proposal)| proposal);
        let queued = self.queued.iter().filter_map(|pending| match pending {
            Pending::Proposal(proposal) => Some(proposal),
            _ => None,
        });
        in_flight.into_iter().chain(queued)
    }

    /// Whether the proposal with the lane is the user's own.
    fn users_in_flight(&self) -> bool {
        (self.in_flight.as_ref()).is_some_and(|(_, proposal)| proposal.client.is_none())
    }

    /// Has `change` wait behind the proposals waiting.
    pub(crate) fn wait(&mut self, change: Change) {
        self.queued.push_back(Pending::Change(change));
    }

    /// Notes an MCP client edit applied ([`Proposals::client_edits`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn client_applied(&mut self) {
        // One per edit: a u64 won't run out.
        self.client_edits = self.client_edits.wrapping_add(1);
    }

    /// Whether something of the user's own waits: a proposal, a change,
    /// or their answer to one. What Undo and Redo go to the queue for:
    /// an MCP client's edit waiting isn't their action, so isn't theirs
    /// to drop.
    pub(crate) fn users_waiting(&self) -> bool {
        self.users_in_flight() || self.asking || self.queued.iter().any(Pending::users)
    }

    /// Has an MCP client's edit, the call waiting under `tag`, wait
    /// behind what waits: its commands are made in its turn.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn wait_client(&mut self, tag: u64) {
        self.queued.push_back(Pending::Client { tag });
    }

    /// Has an MCP client's edit, the call waiting under `tag`, wait
    /// first among what's queued, behind only the proposal with the lane:
    /// one that was at its turn already, waiting for the model.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn wait_client_first(&mut self, tag: u64) {
        self.queued.push_front(Pending::Client { tag });
    }

    /// The tag of the MCP client's call first in the queue, if it's at
    /// its turn: nothing with the lane, nor the user asked.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn client_first(&self) -> Option<u64> {
        match self.queued.front() {
            Some(Pending::Client { tag }) if self.in_flight.is_none() && !self.asking => Some(*tag),
            _ => None,
        }
    }

    /// Takes the MCP client's edits waiting out of the queue but those
    /// `kept` picks by their call's tag: those whose call ended, a
    /// client's proposal with the lane too (its answer, still to come, is
    /// ignored). Whether any went.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn retain_clients(&mut self, kept: impl Fn(u64) -> bool) -> bool {
        let before = self.queued.len();
        self.queued.retain(|pending| match pending {
            Pending::Client { tag, .. } => kept(*tag),
            Pending::Proposal(Proposal {
                client: Some(tag), ..
            }) => kept(*tag),
            _ => true,
        });
        let mut went = self.queued.len() != before;
        let ended = (self.in_flight.as_ref())
            .and_then(|(_, proposal)| proposal.client)
            .is_some_and(|tag| !kept(tag));
        if ended {
            self.in_flight = None;
            self.dropped = self.dropped.saturating_add(1);
            went = true;
        }
        self.settle();
        went
    }

    /// Has the MCP client's proposal of `edit` of the sketch `feature`,
    /// a new link's `source` with it, for the call under `tag`, wait
    /// first among what's queued: it's at its turn. Made in the design's
    /// `units`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn propose_client(
        &mut self,
        tag: u64,
        feature: FeatureId,
        edit: SketchEdit,
        source: Option<OutsideRef>,
        units: LengthUnit,
    ) {
        if !self.any() {
            self.since = Some(Instant::now());
            self.slow = false;
        }
        self.queued.push_front(Pending::Proposal(Proposal {
            feature,
            edit,
            from: None,
            units,
            source,
            probe: None,
            forced: false,
            client: Some(tag),
            steps: None,
            made_on: None,
        }));
    }

    /// Has the MCP client's sketch edit in `steps`, of the sketch
    /// `feature`, for the call under `tag`, wait first among what's
    /// queued, as [`Proposals::propose_client`]: each step proposed on
    /// the solution of the one before, all committed together once the
    /// last is accepted. Made on a sketch whose `next_id` was `next_id`,
    /// in the design's `units`. Nothing for no steps.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn propose_steps(
        &mut self,
        tag: u64,
        feature: FeatureId,
        steps: Vec<Step>,
        units: LengthUnit,
        next_id: u32,
    ) {
        let mut rest: VecDeque<Step> = steps.into();
        let Some(first) = rest.pop_front() else {
            return;
        };
        self.propose_client(tag, feature, first.edit, None, units);
        if let Some(Pending::Proposal(proposal)) = self.queued.front_mut() {
            proposal.steps = Some(Box::new(Steps {
                ops: first.ops,
                measured: first.measured,
                rest,
                referenced: Vec::new(),
                next_id,
                on: None,
                later: false,
            }));
        }
    }

    /// Stops timing the wait once nothing is waiting, or while the user
    /// is asked.
    fn settle(&mut self) {
        if !self.any() || self.asking {
            self.since = None;
            self.slow = false;
        }
    }

    /// Whether a change from the queue was asking the user, which no
    /// longer is: its answer is made in its turn.
    pub(crate) fn take_asking(&mut self) -> bool {
        let asking = self.asking;
        self.answered();
        asking
    }

    /// The user answered the change from the queue that asked: what
    /// waits behind it goes on, timed afresh.
    fn answered(&mut self) {
        if self.asking {
            self.asking = false;
            if self.any() {
                self.since = Some(Instant::now());
            }
        }
    }
}

/// Analyses of the sketch being edited, by revision, the newest last.
#[derive(Debug, Default)]
pub(crate) struct Analyses {
    by_revision: VecDeque<(Revision, Arc<Analysis>)>,
    /// A revision whose analysis failed, so it's not asked for again.
    failed: Option<Revision>,
}

impl Analyses {
    pub(crate) fn get(&self, revision: Revision) -> Option<&Arc<Analysis>> {
        self.by_revision
            .iter()
            .find(|(analysed, _)| *analysed == revision)
            .map(|(_, analysis)| analysis)
    }

    fn insert(&mut self, revision: Revision, analysis: Arc<Analysis>) {
        self.by_revision
            .retain(|(analysed, _)| *analysed != revision);
        if self.by_revision.len() == ANALYSES {
            self.by_revision.pop_front();
        }
        self.by_revision.push_back((revision, analysis));
    }

    /// Whether the analysis of `revision` is to be asked for.
    fn wanted(&self, revision: Revision) -> bool {
        self.get(revision).is_none() && self.failed != Some(revision)
    }
}

/// The next step of the MCP client's sketch edit in steps `proposal`,
/// accepted on `base` as `sketch`: proposed on that, if there's one.
fn next_step(proposal: &Proposal, base: Revision, sketch: &Arc<Sketch>) -> Option<Proposal> {
    let mut steps = proposal.steps.clone()?;
    let step = steps.rest.pop_front()?;
    steps.ops = step.ops;
    steps.measured = step.measured;
    steps.later = true;
    Some(Proposal {
        edit: step.edit,
        from: Some((base, sketch.clone())),
        forced: false,
        steps: Some(steps),
        ..proposal.clone()
    })
}

/// `edit`, a step of the MCP client's edit placing dimensions, with
/// their sides, and their values if they're `measured`, taken on
/// `sketch`, which it's proposed on, in `design`: the step was made on
/// the sketch the operations before it left applied, not solved, where
/// a point moved hadn't taken the rest with it. One that can't be
/// measured there is left as made.
fn remeasured(edit: &mut SketchEdit, measured: bool, sketch: &Sketch, design: &Design) {
    let SketchEdit::Add(add) = edit else {
        return;
    };
    for dimension in &mut add.dimensions {
        if let Ok((value, side)) = sketch.held(&dimension.measure, dimension.side, design) {
            dimension.side = side;
            if measured {
                dimension.value = value;
            }
        }
    }
}

/// `committed` at `revision` with `proposals` applied in order, the
/// steps of a client's edit in steps after each, in `design` but each in
/// its units, and the sources of the links they add. One that doesn't
/// apply (it depends on one refused) is left out; one going on from a
/// solution at `revision` (a drop's move, a later step) is applied to
/// that.
fn applied<'a>(
    committed: &Sketch,
    proposals: impl Iterator<Item = &'a Proposal>,
    revision: Revision,
    design: &Design,
) -> (Sketch, Vec<(Id, OutsideRef)>) {
    let mut sketch = committed.clone();
    let mut sources = Vec::new();
    for proposal in proposals {
        let design = Design {
            units: proposal.units,
            ..*design
        };
        let on = match &proposal.from {
            Some((at, from))
                if *at == revision
                    || (proposal.steps.as_ref()).is_some_and(|steps| steps.going_on(committed)) =>
            {
                from
            }
            _ => &sketch,
        };
        if let Ok(next) = proposal.edit.apply(on, &design) {
            // An added link goes last, with the highest id.
            if let (Some(source), Some(link)) = (proposal.source, next.links.last()) {
                sources.push((link.id, source));
            }
            sketch = next;
        }
        let rest = proposal.steps.iter().flat_map(|steps| &steps.rest);
        for step in rest {
            if let Ok(next) = step.edit.apply(&sketch, &design) {
                sketch = next;
            }
        }
    }
    (sketch, sources)
}

/// What the solver said of a proposal.
enum Answer {
    Accepted(Arc<Sketch>, Arc<Analysis>),
    Rejected(Rejected),
    Failed(String),
}

impl Doc {
    /// Whether edits are waiting on the solver.
    pub(crate) fn proposing(&self) -> bool {
        self.proposals.any()
    }

    /// Whether something of the user's own waits on the solver
    /// ([`Proposals::users_waiting`]): what the user's other changes
    /// wait behind, and what holds the panels' OK. An MCP client's edit
    /// waiting alone holds neither: it isn't applied yet, and is made
    /// at its turn on the design as it is then, so the user's change
    /// made now simply comes before it in the undo history, as it came
    /// first; a client's sketch proposal with the lane is proposed again
    /// on the revision the change made, or refused if its sketch went
    /// or changed under its steps.
    pub(crate) fn users_proposing(&self) -> bool {
        self.proposals.users_waiting()
    }

    /// Proposes `edit` of the sketch being edited, if it can be changed:
    /// on the sketch as it's worked on, with the edits waiting applied.
    /// One that can't be applied to that is refused at once, saying why.
    /// Puts back a drag, and ends what the last refusal showed. Whether it
    /// was proposed.
    pub(crate) fn propose(&mut self, edit: SketchEdit) -> bool {
        self.propose_from(edit, None)
    }

    /// Proposes a new link of `kind` from `source`, as [`Doc::propose`]
    /// does an edit, committed with its source.
    pub(crate) fn propose_link(&mut self, kind: LinkKind, source: OutsideRef) -> bool {
        self.propose_with(SketchEdit::AddLink { kind }, None, Some(source), None)
    }

    /// Proposes `edit` as [`Doc::propose`] does, on `from` in place of the
    /// sketch committed while the document is at the revision given.
    pub(super) fn propose_from(
        &mut self,
        edit: SketchEdit,
        from: Option<(Revision, Arc<Sketch>)>,
    ) -> bool {
        self.propose_with(edit, from, None, None)
    }

    /// Proposes `edit`, of driving dimensions alone, as [`Doc::propose`]
    /// does, opening `field` if it's accepted in place of committing it,
    /// or placing them as references, saying so, if they over-constrain
    /// the sketch.
    pub(super) fn probe(&mut self, edit: SketchEdit, field: ValueEdit) -> bool {
        self.propose_with(edit, None, None, Some(Box::new(field)))
    }

    /// Proposes `edit` as [`Doc::propose_from`] does, a new link's
    /// `source` with it.
    fn propose_with(
        &mut self,
        edit: SketchEdit,
        from: Option<(Revision, Arc<Sketch>)>,
        source: Option<OutsideRef>,
        probe: Option<Box<ValueEdit>>,
    ) -> bool {
        let Some(sketch) = self.editable_sketch() else {
            return false;
        };
        let made_on = sketch.next_id;
        if let Err(why) = edit.apply(sketch, &self.editor.document().design()) {
            self.refuse(why);
            return false;
        }
        let Some(session) = &mut self.sketch else {
            return false;
        };
        session.refusal = None;
        session.drag = None;
        let feature = session.feature;
        let units = self.editor.document().units();
        let proposals = &mut self.proposals;
        if !proposals.any() {
            proposals.since = Some(Instant::now());
            proposals.slow = false;
        }
        proposals.queued.push_back(Pending::Proposal(Proposal {
            feature,
            edit,
            from,
            units,
            source,
            probe,
            forced: false,
            client: None,
            steps: None,
            made_on: Some((made_on, proposals.client_edits)),
        }));
        self.send_proposal();
        self.refresh_waiting();
        true
    }

    /// Sends the lane the next proposal, if none is with it, on the sketch
    /// committed now, making the changes waiting before it first. One
    /// whose sketch is gone is dropped. Nothing moves while the delete
    /// prompt is up: one a change from the queue asks is answered before
    /// what waits behind it is made, and what's made while it's up comes
    /// after it anyway.
    pub(crate) fn send_proposal(&mut self) {
        if self.delete_asked() {
            self.proposals.settle();
            return;
        }
        self.proposals.answered();
        while self.proposals.in_flight.is_none() {
            let Some(pending) = self.proposals.queued.pop_front() else {
                break;
            };
            let proposal = match pending {
                Pending::Change(change) => {
                    self.make(change);
                    if self.delete_asked() {
                        self.proposals.asking = true;
                        break;
                    }
                    continue;
                }
                #[cfg(not(target_arch = "wasm32"))]
                Pending::Client { tag } => {
                    // An edit waiting for the model keeps its place, and
                    // what's behind it waits.
                    if self.client_queued_turn(tag) {
                        self.proposals.queued.push_front(Pending::Client { tag });
                        break;
                    }
                    continue;
                }
                Pending::Proposal(proposal) => proposal,
            };
            let revision = self.editor.revision();
            let sketch = match sketch_of(self.editor.document(), proposal.feature) {
                None => Err(SKETCH_GONE),
                Some(committed)
                    if (proposal.steps.as_ref())
                        .is_some_and(|steps| steps.changed(committed, &proposal.from)) =>
                {
                    Err(SKETCH_CHANGED)
                }
                Some(committed)
                    if proposal.made_on.is_some_and(|(made, edits)| {
                        committed.next_id > made && self.proposals.client_edits != edits
                    }) =>
                {
                    Err(TAKEN_BY_CLIENT)
                }
                Some(committed) => Ok(match &proposal.from {
                    Some((at, from))
                        if *at == revision
                            || (proposal.steps.as_ref())
                                .is_some_and(|steps| steps.going_on(committed)) =>
                    {
                        from.clone()
                    }
                    _ => Arc::new(committed.clone()),
                }),
            };
            let sketch = match sketch {
                Ok(sketch) => sketch,
                // The client's call says so; the user's sketch is gone
                // from under them, which they see.
                Err(why) => {
                    if let Some(tag) = proposal.client {
                        self.client_proposal_settled(tag, Err(why.to_owned()));
                    } else if why == TAKEN_BY_CLIENT {
                        self.refused(proposal.feature, Refusal::Failed(why.to_owned()));
                    }
                    continue;
                }
            };
            let mut proposal = proposal;
            if let Some(steps) = proposal.steps.as_mut().filter(|steps| !steps.later) {
                steps.on = Some(sketch.clone());
            }
            if let Some(measured) = proposal.steps.as_ref().and_then(|steps| steps.measured) {
                let design = Design {
                    units: proposal.units,
                    ..self.editor.document().design()
                };
                remeasured(&mut proposal.edit, measured, &sketch, &design);
            }
            let Some(solver) = &mut self.solver else {
                self.proposals
                    .queued
                    .push_front(Pending::Proposal(proposal));
                break;
            };
            solver.send(Request::Propose {
                base: revision,
                sketch,
                edit: proposal.edit.clone(),
                units: proposal.units,
                // Parameter changes wait behind proposals, so these are
                // the ones the edit was made with.
                params: self.editor.document().params_shared(),
            });
            self.proposals.in_flight = Some((revision, proposal));
        }
        self.proposals.settle();
    }

    /// Takes back the newest of the user's own that waits, as undo does:
    /// the last change or proposal of theirs queued, or else the question
    /// a change from the queue asks (cancelling it), or else the proposal
    /// with the lane, which is answered still, and ignored. Those before
    /// it don't depend on it. An MCP client's edit waiting is passed
    /// over: it's not the user's last action, and it's applied in its
    /// turn.
    pub(crate) fn drop_newest(&mut self) {
        let proposals = &mut self.proposals;
        let newest = proposals.queued.iter().rposition(Pending::users);
        if newest.and_then(|at| proposals.queued.remove(at)).is_none() {
            if mem::take(&mut proposals.asking) {
                self.deleting = None;
            } else if proposals.users_in_flight() {
                proposals.in_flight = None;
                proposals.dropped = proposals.dropped.saturating_add(1);
            }
        }
        proposals.settle();
        self.refresh_waiting();
    }

    /// Drops everything waiting on the solver, and the changes waiting
    /// behind it, as restoring recovered changes does: the proposal with
    /// the lane is answered still, and ignored.
    pub(crate) fn drop_proposals(&mut self) {
        let proposals = &mut self.proposals;
        if proposals.in_flight.take().is_some() {
            proposals.dropped = proposals.dropped.saturating_add(1);
        }
        proposals.queued.clear();
        if mem::take(&mut proposals.asking) {
            self.deleting = None;
        }
        self.proposals.settle();
        self.refresh_waiting();
        // The client's edits dropped with them: their calls say so.
        #[cfg(not(target_arch = "wasm32"))]
        self.mcp_end_queued();
    }

    /// Takes `response`, the solver's answer for the document: commits an
    /// accepted proposal, shows why one was refused, shows a drag's step,
    /// keeps an analysis, and answers a client's `sketch` call with its.
    pub(crate) fn solved(&mut self, response: Response) {
        match response {
            Response::Accepted {
                base,
                sketch,
                analysis,
            } => self.proposal_answered(base, Answer::Accepted(sketch, analysis)),
            Response::Rejected { base, why } => self.proposal_answered(base, Answer::Rejected(why)),
            Response::Failed {
                tag: Tag::Propose(base),
                error,
            } => {
                log::error!("The solver failed on an edit: {error}");
                self.proposal_answered(base, Answer::Failed(error));
            }
            Response::Dragged { session, solution } => {
                let drag = self.sketch.as_mut().and_then(|sketch| sketch.drag.as_mut());
                if let Some(drag) = drag.filter(|drag| drag.session == session) {
                    drag.solution = Some(solution);
                }
            }
            Response::Failed {
                tag: Tag::Drag(_),
                error,
            } => log::error!("The solver failed on a drag: {error}"),
            Response::Analysed {
                query: None,
                revision,
                analysis,
            } => self.analysed(revision, Some(analysis)),
            Response::Failed {
                tag: Tag::Analyse(revision),
                error,
            } => {
                log::error!("The solver failed to analyse a sketch: {error}");
                self.analysed(revision, None);
            }
            // A client's analysis, for an MCP `sketch` call: natively only.
            Response::Analysed {
                query: Some(id),
                analysis,
                ..
            } => self.client_analysed(id, Ok(&analysis)),
            Response::Failed {
                tag: Tag::Query(id),
                error,
            } => self.client_analysed(id, Err(error)),
        }
        self.sync();
    }

    /// Takes the solve lane's answer to the client's analysis `id`, or
    /// why it failed: an MCP `sketch` call's, natively only.
    fn client_analysed(&mut self, id: u64, analysis: Result<&Analysis, String>) {
        #[cfg(not(target_arch = "wasm32"))]
        self.sketch_analysed(id, analysis);
        #[cfg(target_arch = "wasm32")]
        let _ = (id, analysis);
    }

    /// Takes the `answer` to the proposal on `base` with the lane, unless
    /// it was dropped: commits it, or says why not, and sends the next.
    fn proposal_answered(&mut self, base: Revision, answer: Answer) {
        let proposals = &mut self.proposals;
        if proposals.dropped > 0 {
            proposals.dropped -= 1;
            return;
        }
        let Some((sent, proposal)) = proposals.in_flight.take() else {
            return;
        };
        if base != sent || base != self.editor.revision() {
            // Something else was committed since it was proposed: it's
            // proposed again on what's committed now.
            proposals.queued.push_front(Pending::Proposal(proposal));
            self.send_proposal();
            return;
        }
        let feature = proposal.feature;
        if let Some(field) = proposal.probe {
            match answer {
                Answer::Accepted(..) => {
                    if self.sketch.as_ref().is_some_and(|s| s.feature == feature) {
                        self.open_value(*field);
                    }
                    self.send_proposal();
                    return;
                }
                answer => {
                    let proposal = Proposal {
                        probe: None,
                        ..proposal
                    };
                    return self.proposal_settled(base, proposal, answer);
                }
            }
        }
        self.proposal_settled(base, proposal, answer);
    }

    /// Takes the `answer` to `proposal` on `base`, no probe, as
    /// [`Doc::proposal_answered`] does: commits it, or says why not.
    fn proposal_settled(&mut self, base: Revision, proposal: Proposal, answer: Answer) {
        let feature = proposal.feature;
        if let Answer::Rejected(Rejected::Driving { .. }) = &answer
            && let Some(mut reference) = self.as_reference(&proposal)
        {
            // A dimension placed that over-constrains the sketch is placed
            // as a reference instead, as the refusal would suggest; the
            // MCP client's call says which operations.
            if let Some(steps) = &mut reference.steps {
                let (first, last) = steps.ops;
                steps.referenced.extend(first..=last);
            }
            self.proposals
                .queued
                .push_front(Pending::Proposal(reference));
            self.send_proposal();
            return;
        }
        // The next step of a client's edit in steps goes on from this
        // one's solution.
        if let Answer::Accepted(sketch, _) = &answer
            && let Some(next) = next_step(&proposal, base, sketch)
        {
            self.proposals.queued.push_front(Pending::Proposal(next));
            self.send_proposal();
            return;
        }
        // The client's commits through its apply path, or answers its call
        // why not: never the session's refusal, the banner or a toast.
        if let Some(tag) = proposal.client {
            let said = |why: String| match &proposal.steps {
                Some(steps) => format!("{}: {why}", steps.said()),
                None => why,
            };
            let settled = match answer {
                Answer::Accepted(sketch, analysis) => Ok(Settled {
                    referenced: (proposal.steps.as_ref())
                        .map(|steps| steps.referenced.clone())
                        .unwrap_or_default(),
                    command: self.committing(&proposal, sketch),
                    freedom: analysis.freedom,
                }),
                Answer::Rejected(why) => Err(said(why.to_string())),
                Answer::Failed(error) => Err(said(format!("the solver failed on it: {error}"))),
            };
            self.client_proposal_settled(tag, settled);
            self.send_proposal();
            return;
        }
        let refusal = match answer {
            Answer::Accepted(sketch, analysis) => {
                if proposal.forced {
                    self.toast = Some(
                        "The dimension would over-constrain the sketch: placed as a reference"
                            .into(),
                    );
                }
                self.apply(self.committing(&proposal, sketch));
                // Not committed (read-only since, say), the analysis isn't
                // of what's committed.
                let revision = self.editor.revision();
                if revision != base
                    && let Some(session) = self.sketch.as_mut().filter(|s| s.feature == feature)
                {
                    session.analyses.insert(revision, analysis);
                }
                None
            }
            Answer::Rejected(why) => Some(Refusal::Rejected(why)),
            Answer::Failed(error) => Some(Refusal::Failed(error)),
        };
        if let Some(refusal) = refusal {
            self.refused(feature, refusal);
        }
        self.send_proposal();
    }

    /// Says why the user's edit of the sketch `feature` wasn't made: in
    /// the sketch's status bar, or the banner if it was left.
    fn refused(&mut self, feature: FeatureId, refusal: Refusal) {
        match self.sketch.as_mut().filter(|s| s.feature == feature) {
            Some(session) => session.refusal = Some(refusal),
            // Its sketch was left, so its status bar doesn't say: the
            // banner does, or the edit would go without a word.
            None => self.refused_edit = Some((feature, refusal)),
        }
    }

    /// The command committing `proposal`, accepted as `sketch`: a new
    /// link with what it comes from ([`Command::AddLink`]), else
    /// [`Command::SetSketch`]. The user's and the MCP client's share it.
    fn committing(&self, proposal: &Proposal, sketch: Arc<Sketch>) -> Command {
        let feature = proposal.feature;
        let mut sketch = Arc::unwrap_or_clone(sketch);
        // Read in other units than the design's now: its values keep what
        // they came to, as setting the units does.
        let design = self.editor.document().design();
        if proposal.units != design.units {
            sketch.pin_units(&Design {
                units: proposal.units,
                ..design
            });
        }
        // A new link is committed with what it comes from: the link the
        // committed sketch hasn't.
        let committed = sketch_of(self.editor.document(), feature);
        let link = (sketch.links.iter())
            .map(|link| link.id)
            .find(|&id| committed.is_some_and(|committed| committed.link(id).is_none()));
        match (proposal.source, link) {
            (Some(source), Some(link)) => Command::AddLink {
                feature,
                sketch: Box::new(sketch),
                source: LinkSource { link, source },
            },
            _ => Command::SetSketch {
                feature,
                sketch: Box::new(sketch),
            },
        }
    }

    /// Answers the MCP client's proposal for the call under `tag`:
    /// `settled` the command committing it, or why it wasn't accepted.
    /// Natively only (the web has no client, and no client proposals).
    fn client_proposal_settled(&mut self, tag: u64, settled: Result<Settled, String>) {
        #[cfg(not(target_arch = "wasm32"))]
        self.client_proposed(tag, settled);
        #[cfg(target_arch = "wasm32")]
        let _ = (tag, settled);
    }

    /// The sketch `feature` as committed, with every edit of it waiting
    /// on the solver applied (the user's and the MCP client's, those
    /// that apply), as an edit queued behind them will find it, near
    /// enough: before solving.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn sketch_as_queued(&self, feature: FeatureId) -> Option<Sketch> {
        let document = self.editor.document();
        let committed = sketch_of(document, feature)?;
        let proposals = (self.proposals.waiting()).filter(|p| p.feature == feature);
        let (sketch, _) = applied(
            committed,
            proposals,
            self.editor.revision(),
            &document.design(),
        );
        Some(sketch)
    }

    /// `proposal` placing driving dimensions alone, with them placed as
    /// references holding the geometry where it's at, if they can be.
    fn as_reference(&self, proposal: &Proposal) -> Option<Proposal> {
        let SketchEdit::Add(add) = &proposal.edit else {
            return None;
        };
        let alone = add.points.is_empty()
            && add.curves.is_empty()
            && add.constraints.is_empty()
            && add.auto.is_empty();
        if !alone || add.dimensions.is_empty() || add.dimensions.iter().any(|d| !d.driving) {
            return None;
        }
        let document = self.editor.document();
        let sketch = match &proposal.from {
            Some((_, from)) => &**from,
            None => sketch_of(document, proposal.feature)?,
        };
        let design = document.design();
        let mut add = add.clone();
        for dimension in &mut add.dimensions {
            let (value, side) = sketch
                .held(&dimension.measure, dimension.side, &design)
                .ok()?;
            dimension.value = value;
            dimension.side = side;
            dimension.driving = false;
        }
        Some(Proposal {
            edit: SketchEdit::Add(add),
            forced: true,
            ..proposal.clone()
        })
    }

    /// The sketch edit the solver refused after its sketch was left, for
    /// the banner over the viewport, while its sketch is there.
    pub(crate) fn refused_edit(&self) -> Option<RefusedEdit<'_>> {
        let (feature, refusal) = self.refused_edit.as_ref()?;
        let name = &self.editor.document().feature(*feature)?.name;
        let sketch = sketch_of(self.editor.document(), *feature)?;
        let (why, error) = match refusal {
            Refusal::Rejected(why) => (Some(why), None),
            Refusal::Failed(error) => (None, Some(error.as_str())),
        };
        Some(RefusedEdit {
            name,
            sketch,
            why,
            error,
        })
    }

    /// Asks the lane to analyse the sketch being edited as committed, if
    /// it hasn't been, and nothing else is being analysed.
    pub(crate) fn request_analysis(&mut self) {
        if self.proposals.analysing.is_some() {
            return;
        }
        let revision = self.editor.revision();
        let (Some(solver), Some(session)) = (&mut self.solver, &self.sketch) else {
            return;
        };
        if !session.analyses.wanted(revision) {
            return;
        }
        let Some(sketch) = sketch_of(self.editor.document(), session.feature) else {
            return;
        };
        solver.send(Request::Analyse {
            query: None,
            revision,
            sketch: Arc::new(sketch.clone()),
            units: self.editor.document().units(),
            params: self.editor.document().params_shared(),
        });
        self.proposals.analysing = Some((revision, session.feature));
    }

    /// Takes the analysis of `revision`, or that it failed, for the sketch
    /// it was asked for, if that's still being edited.
    fn analysed(&mut self, revision: Revision, analysis: Option<Arc<Analysis>>) {
        let Some((asked, feature)) = self.proposals.analysing.take() else {
            return;
        };
        let Some(session) = self.sketch.as_mut() else {
            return;
        };
        if (asked, feature) != (revision, session.feature) {
            return;
        }
        match analysis {
            Some(analysis) => session.analyses.insert(revision, analysis),
            None => session.analyses.failed = Some(revision),
        }
    }

    /// Notes the time `now`, telling whether edits have waited on the
    /// solver long enough to say so.
    pub(crate) fn tick(&mut self, now: Instant) {
        let proposals = &mut self.proposals;
        if proposals
            .since
            .is_some_and(|since| now.saturating_duration_since(since) >= SLOW)
        {
            proposals.slow = true;
        }
    }

    /// Whether frames are wanted to tell when edits have waited long
    /// enough to say so, see [`Doc::tick`].
    pub(crate) fn timing(&self) -> bool {
        self.proposals.timing()
    }

    /// Takes a new solver lane in place of one that went (its subscription
    /// started again): the proposal and analysis it had are never answered,
    /// so they're asked of the new one, and no answers to dropped ones
    /// are to come.
    pub(crate) fn lane_replaced(&mut self) {
        let proposals = &mut self.proposals;
        if let Some((_, proposal)) = proposals.in_flight.take() {
            proposals.queued.push_front(Pending::Proposal(proposal));
        }
        proposals.dropped = 0;
        proposals.analysing = None;
        // The client's analyses went with the old lane: their calls say so.
        #[cfg(not(target_arch = "wasm32"))]
        self.mcp_end_analyses();
    }

    /// A new drag session's id.
    pub(super) fn next_drag(&mut self) -> u64 {
        let session = self.proposals.next_drag;
        self.proposals.next_drag = session.wrapping_add(1);
        session
    }

    /// Makes the sketch being edited as it's worked on: as committed, with
    /// the edits waiting on the solver applied, or none while nothing
    /// waits. One that doesn't apply (it depends on one refused) is left
    /// out; a drop's move is applied to the drag's last solution, which
    /// it's proposed on.
    pub(crate) fn refresh_waiting(&mut self) {
        let Some(session) = &self.sketch else {
            return;
        };
        let feature = session.feature;
        let revision = self.editor.revision();
        let design = self.editor.document().design();
        let waiting = sketch_of(self.editor.document(), feature).and_then(|committed| {
            let mut edits = self
                .proposals
                .waiting()
                .filter(|proposal| proposal.feature == feature)
                .peekable();
            edits.peek()?;
            let (sketch, sources) = applied(committed, edits, revision, &design);
            let ids = sketch.points.iter().map(|point| point.id);
            let ids = ids
                .chain(sketch.curves.iter().map(|entry| entry.id))
                .chain(sketch.constraints.iter().map(|entry| entry.id))
                .chain(sketch.dimensions.iter().map(|entry| entry.id));
            let added = ids
                .filter(|&id: &Id| committed.kind(id).is_none())
                .collect();
            Some(Waiting {
                sketch,
                added,
                sources,
            })
        });
        if let Some(session) = &mut self.sketch {
            session.waiting = waiting;
        }
    }
}

#[cfg(test)]
mod tests;
