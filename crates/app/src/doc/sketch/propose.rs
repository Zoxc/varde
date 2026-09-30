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
//! until the next action. Other changes to the document made meanwhile
//! wait behind them, in order, so what's committed keeps the order the
//! user made it in, and undo takes back the newest of what waits, dropping
//! it. A proposal answered for a revision no longer the document's
//! (something else was committed meanwhile) is proposed again.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use iced::time::Instant;
use varde_document::{Command, Document, FeatureId, FeatureKind, Revision, Sketch};
use varde_sketch::{Analysis, Id, Rejected, SketchEdit};
use varde_solve::{Request, Response, Tag};

use super::{Refusal, Waiting};
use crate::doc::{Change, Doc};

/// How long edits wait on the solver before the status bar says it's
/// checking them: most are answered well within it.
pub(crate) const CHECKING: Duration = Duration::from_millis(100);

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
    /// Whether they've waited [`CHECKING`] or longer.
    slow: bool,
    /// The analysis asked of the lane and not answered yet, of which
    /// revision of which sketch. One at a time: the answer names only the
    /// revision, which two sketches share.
    analysing: Option<(Revision, FeatureId)>,
    /// The drag session the next drag is.
    next_drag: u64,
}

/// What waits for the proposal in flight, see [`Proposals::queued`].
#[derive(Debug, Clone)]
enum Pending {
    Proposal(Proposal),
    /// Made once the proposals before it are answered.
    Change(Change),
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
}

impl Proposals {
    /// Whether edits are waiting on the solver.
    pub(crate) fn any(&self) -> bool {
        self.in_flight.is_some() || !self.queued.is_empty()
    }

    /// Whether edits have waited on the solver long enough to say so, see
    /// [`CHECKING`].
    pub(crate) fn slow(&self) -> bool {
        self.slow
    }

    /// Whether frames are wanted to tell when edits have waited long
    /// enough to say so.
    pub(crate) fn timing(&self) -> bool {
        self.any() && !self.slow
    }

    /// The proposals waiting, the one in flight first.
    fn waiting(&self) -> impl Iterator<Item = &Proposal> {
        let in_flight = self.in_flight.as_ref().map(|(_, proposal)| proposal);
        let queued = self.queued.iter().filter_map(|pending| match pending {
            Pending::Proposal(proposal) => Some(proposal),
            Pending::Change(_) => None,
        });
        in_flight.into_iter().chain(queued)
    }

    /// Has `change` wait behind the proposals waiting.
    pub(crate) fn wait(&mut self, change: Change) {
        self.queued.push_back(Pending::Change(change));
    }

    /// Stops timing the wait once nothing is waiting.
    fn settle(&mut self) {
        if !self.any() {
            self.since = None;
            self.slow = false;
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

/// The sketch of the sketch feature `feature` of `document`, if it holds
/// one.
pub(super) fn sketch_of(document: &Document, feature: FeatureId) -> Option<&Sketch> {
    match &document.feature(feature)?.kind {
        FeatureKind::Sketch { sketch, .. } => Some(sketch),
        FeatureKind::Extrude(_) => None,
    }
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

    /// Proposes `edit` of the sketch being edited, if it can be changed:
    /// on the sketch as it's worked on, with the edits waiting applied.
    /// One that can't be applied to that is refused at once, saying why.
    /// Puts back a drag, and ends what the last refusal showed. Whether it
    /// was proposed.
    pub(crate) fn propose(&mut self, edit: SketchEdit) -> bool {
        self.propose_from(edit, None)
    }

    /// Proposes `edit` as [`Doc::propose`] does, on `from` in place of the
    /// sketch committed while the document is at the revision given.
    pub(super) fn propose_from(
        &mut self,
        edit: SketchEdit,
        from: Option<(Revision, Arc<Sketch>)>,
    ) -> bool {
        let Some(sketch) = self.editable_sketch() else {
            return false;
        };
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
        let proposals = &mut self.proposals;
        if !proposals.any() {
            proposals.since = Some(Instant::now());
            proposals.slow = false;
        }
        proposals.queued.push_back(Pending::Proposal(Proposal {
            feature,
            edit,
            from,
        }));
        self.send_proposal();
        self.refresh_waiting();
        true
    }

    /// Sends the lane the next proposal, if none is with it, on the sketch
    /// committed now, making the changes waiting before it first. One
    /// whose sketch is gone is dropped.
    pub(crate) fn send_proposal(&mut self) {
        while self.proposals.in_flight.is_none() {
            match self.proposals.queued.front() {
                None => break,
                Some(Pending::Change(_)) => {
                    if let Some(Pending::Change(change)) = self.proposals.queued.pop_front() {
                        self.make(change);
                    }
                }
                Some(Pending::Proposal(_)) => {
                    let Some(solver) = &mut self.solver else {
                        break;
                    };
                    let Some(Pending::Proposal(proposal)) = self.proposals.queued.pop_front()
                    else {
                        break;
                    };
                    let revision = self.editor.revision();
                    let Some(committed) = sketch_of(self.editor.document(), proposal.feature)
                    else {
                        continue;
                    };
                    let sketch = match &proposal.from {
                        Some((at, from)) if *at == revision => from.clone(),
                        _ => Arc::new(committed.clone()),
                    };
                    solver.send(Request::Propose {
                        base: revision,
                        sketch,
                        edit: proposal.edit.clone(),
                        units: self.editor.document().units(),
                    });
                    self.proposals.in_flight = Some((revision, proposal));
                }
            }
        }
        self.proposals.settle();
    }

    /// Takes back the newest of what waits, as undo does: the last change
    /// or proposal queued, or else the proposal with the lane, which is
    /// answered still, and ignored. Those before it don't depend on it.
    pub(crate) fn drop_newest(&mut self) {
        let proposals = &mut self.proposals;
        if proposals.queued.pop_back().is_none() && proposals.in_flight.take().is_some() {
            proposals.dropped = proposals.dropped.saturating_add(1);
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
        proposals.settle();
        self.refresh_waiting();
    }

    /// Takes `response`, the solver's answer for the document: commits an
    /// accepted proposal, shows why one was refused, shows a drag's step
    /// and keeps an analysis.
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
            Response::Analysed { revision, analysis } => self.analysed(revision, Some(analysis)),
            Response::Failed {
                tag: Tag::Analyse(revision),
                error,
            } => {
                log::error!("The solver failed to analyse a sketch: {error}");
                self.analysed(revision, None);
            }
        }
        self.sync();
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
        let refusal = match answer {
            Answer::Accepted(sketch, analysis) => {
                self.apply(Command::SetSketch {
                    feature,
                    sketch: Box::new(Arc::unwrap_or_clone(sketch)),
                });
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
        if let Some(refusal) = refusal
            && let Some(session) = self.sketch.as_mut().filter(|s| s.feature == feature)
        {
            session.refusal = Some(refusal);
        }
        self.send_proposal();
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
            revision,
            sketch: Arc::new(sketch.clone()),
            units: self.editor.document().units(),
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
            .is_some_and(|since| now.saturating_duration_since(since) >= CHECKING)
        {
            proposals.slow = true;
        }
    }

    /// Whether frames are wanted to tell when edits have waited long
    /// enough to say so, see [`Doc::tick`].
    pub(crate) fn timing(&self) -> bool {
        self.proposals.timing()
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
            let mut sketch = committed.clone();
            for proposal in edits {
                // A drop's move goes on from the drag's last solution.
                let on = match &proposal.from {
                    Some((at, from)) if *at == revision => from,
                    _ => &sketch,
                };
                if let Ok(next) = proposal.edit.apply(on, &design) {
                    sketch = next;
                }
            }
            let ids = sketch.points.iter().map(|point| point.id);
            let ids = ids
                .chain(sketch.curves.iter().map(|entry| entry.id))
                .chain(sketch.constraints.iter().map(|entry| entry.id))
                .chain(sketch.dimensions.iter().map(|entry| entry.id));
            let added = ids
                .filter(|&id: &Id| committed.kind(id).is_none())
                .collect();
            Some(Waiting { sketch, added })
        });
        if let Some(session) = &mut self.sketch {
            session.waiting = waiting;
        }
    }
}

#[cfg(test)]
mod tests;
