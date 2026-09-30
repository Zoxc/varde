//! The model shown for a document, its mesh and its finished sketches' lines,
//! fed by the regeneration side.

use std::sync::Arc;

use varde_document::{BodyId, Editor, Extrude, FeatureId, Generation};
use varde_kernel::{RenderLines, RenderMesh};
use varde_regen::{Draft, Drafted, Request, Response, Transport};
use varde_view::MeshStatus;

/// The mesh and sketch lines shown for the document. They're built by the
/// regeneration side, so they may lag behind the editor: the last ones stay
/// shown until newer ones arrive.
#[derive(Default)]
pub(crate) struct MeshFeed {
    /// Builds the meshes: the document's lane, a thread natively and a Web
    /// Worker on the web. `None` until the lane has started, see
    /// [`Varde::regen_lane`]; nothing is requested until then.
    ///
    /// [`Varde::regen_lane`]: crate::Varde::regen_lane
    regen: Option<Box<dyn Transport<Request>>>,
    mesh: Arc<RenderMesh>,
    /// The visible sketches' curves, of the same generation as `mesh`.
    sketches: Arc<RenderLines>,
    /// The sketches that don't solve, of the same generation as `mesh`.
    unsolved: Vec<FeatureId>,
    /// The features that failed and why, of the same generation as
    /// `mesh`, in the document's order.
    failed_features: Vec<(FeatureId, String)>,
    /// How the draft of the model shown went, if it had one.
    drafted: Option<Drafted>,
    /// The bodies the newest draft answered that ran the touch test
    /// touches, and that draft's revision: kept across answers that
    /// didn't run it, so the panel's list doesn't empty while a draft
    /// fails before its tool exists, or while New body is picked.
    touched: Option<(u64, Vec<BodyId>)>,
    /// The first revision of the current run of drafts: those of one
    /// extrude set up without a pause. A run starts when a draft is
    /// asked for after none, or for another feature; touched bodies
    /// answered for an earlier run aren't listed.
    run: u64,
    /// What `mesh` is of, or `None` before the first one arrives.
    shown: Option<Asked>,
    /// What was asked for last.
    requested: Option<Asked>,
    /// The draft asked for last, if any: another one is given the next
    /// revision.
    draft: Option<Draft>,
    /// The revision the last draft was given: drafts are counted up
    /// across the document's life, so no two differing ones share one.
    revision: u64,
    /// What was asked for, at least as new as `shown`, whose model
    /// couldn't be built, and why.
    failed: Option<(Asked, String)>,
}

/// What a request asks for, and so what its answer is of: a generation
/// of the document, the sketch left out of the lines, and the revision
/// of the draft applied, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Asked {
    generation: Generation,
    exclude: Option<FeatureId>,
    draft: Option<u64>,
}

impl Asked {
    /// What `response` answers.
    fn of(response: &Response) -> Self {
        Self {
            generation: response.generation(),
            exclude: response.exclude(),
            draft: response.draft(),
        }
    }
}

impl MeshFeed {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Starts sending requests to `lane`, the document's regeneration
    /// lane, once it has started. What to ask for is up to the next
    /// [`MeshFeed::request_with`].
    pub(crate) fn connect(&mut self, lane: impl Transport<Request> + 'static) {
        self.regen = Some(Box::new(lane));
    }

    /// Whether the lane has started, see [`MeshFeed::connect`].
    #[cfg(test)]
    pub(crate) fn connected(&self) -> bool {
        self.regen.is_some()
    }

    /// [`MeshFeed::request_with`] without a draft.
    #[cfg(test)]
    pub(crate) fn request(&mut self, editor: &Editor, exclude: Option<FeatureId>) {
        self.request_with(editor, exclude, None);
    }

    /// Asks the lane for the document's mesh, leaving the sketch `exclude`
    /// out of the lines, with `draft` applied, the extrude being set up
    /// and the extrude it's edited from, if any: if the editor moved on
    /// since the last request, the sketch to leave out changed, as it does
    /// on entering or leaving one, or the draft did. A draft differing
    /// from the one asked for last is given the next revision; without
    /// one, the model is asked for again without the last. Nothing, not
    /// even remembering it, before the lane has started: then the first
    /// request asks for the editor's newest.
    pub(crate) fn request_with(
        &mut self,
        editor: &Editor,
        exclude: Option<FeatureId>,
        draft: Option<(Option<FeatureId>, Extrude)>,
    ) {
        let Some(regen) = &mut self.regen else {
            return;
        };
        let draft = draft.map(|(feature, extrude)| match &self.draft {
            Some(last) if last.feature == feature && last.extrude == extrude => last.clone(),
            last => {
                // One per change of a draft: a u64 won't run out.
                self.revision += 1;
                if last.as_ref().is_none_or(|last| last.feature != feature) {
                    self.run = self.revision;
                }
                Draft {
                    revision: self.revision,
                    feature,
                    extrude,
                }
            }
        });
        let asked = Asked {
            generation: editor.generation(),
            exclude,
            draft: draft.as_ref().map(|draft| draft.revision),
        };
        self.draft = draft.clone();
        if self.requested.is_none_or(|requested| {
            requested.generation < asked.generation
                || (requested.generation == asked.generation && requested != asked)
        }) {
            self.requested = Some(asked);
            regen.send(Request::Regenerate {
                generation: asked.generation,
                document: editor.snapshot(),
                exclude,
                draft,
            });
        }
    }

    /// Shows the model in `response`, or its error next to the last one,
    /// unless a response as new was applied already. Responses arriving out
    /// of order or superseded are dropped.
    pub(crate) fn apply(&mut self, response: Response) {
        if !self.wanted(&response) {
            return;
        }
        let asked = Asked::of(&response);
        match response {
            Response::Regenerated {
                mesh,
                sketches,
                unsolved,
                failed,
                draft,
                ..
            } => {
                self.mesh = mesh;
                self.sketches = sketches;
                self.unsolved = unsolved;
                self.failed_features = failed;
                if let Some(Drafted {
                    revision,
                    touched: Some(touched),
                    ..
                }) = &draft
                    && self
                        .touched
                        .as_ref()
                        .is_none_or(|(kept, _)| kept <= revision)
                {
                    self.touched = Some((*revision, touched.clone()));
                }
                self.drafted = draft;
                self.shown = Some(asked);
                self.failed = None;
            }
            Response::Failed { error, .. } => self.failed = Some((asked, error)),
        }
    }

    /// Whether `response` is newer than what was applied: of a newer
    /// generation, or of the same one answering the request asked last,
    /// which left out another sketch than the answer applied, model or
    /// failure. Once a request failed, nothing more of it is taken, but a
    /// request of the same generation leaving out another sketch, entering
    /// or leaving one, is answered as usual.
    fn wanted(&self, response: &Response) -> bool {
        let Some(answered) = self.answered() else {
            return true;
        };
        let asked = Asked::of(response);
        if asked.generation != answered {
            return asked.generation > answered;
        }
        // Of the same generation: only the answer to what was asked last,
        // the same sketch left out and the same draft applied, once.
        let failed = self
            .failed
            .as_ref()
            .is_some_and(|(failed, _)| *failed == asked);
        self.requested.is_some_and(|requested| {
            (requested.exclude, requested.draft) == (asked.exclude, asked.draft)
        }) && self.shown != Some(asked)
            && !failed
    }

    /// The newest generation a response was applied for.
    fn answered(&self) -> Option<Generation> {
        self.last_answered().map(|asked| asked.generation)
    }

    /// What the response applied last answered, model or failure.
    fn last_answered(&self) -> Option<Asked> {
        self.failed.as_ref().map(|(asked, _)| *asked).or(self.shown)
    }

    /// Why the draft asked for last fails, once its answer is shown: none
    /// if it goes, it's not answered yet, or there's none.
    pub(crate) fn draft_error(&self) -> Option<&str> {
        let revision = self.draft.as_ref()?.revision;
        let drafted = self.drafted.as_ref()?;
        (drafted.revision == revision)
            .then_some(drafted.error.as_deref())
            .flatten()
    }

    /// The bodies the draft's solid touches, as the newest answer of the
    /// current run of drafts that ran the touch test found, while a draft
    /// is asked for: kept while a changed draft is on its way, and while
    /// one fails before its tool exists or makes a new body, so the
    /// panel's list doesn't blink. Empty until the run's first such
    /// answer, so another extrude's bodies aren't listed.
    pub(crate) fn draft_touched(&self) -> &[BodyId] {
        match (&self.draft, &self.touched) {
            (Some(_), Some((revision, touched))) if *revision >= self.run => touched,
            _ => &[],
        }
    }

    /// How the mesh shown stands against the editor's document. A failure
    /// of an older generation than the editor's isn't reported: that document
    /// is gone, and the current one is still being built. Nor is the model
    /// shown current while another draft than its, or none, is asked for:
    /// a preview cancelled or changed still shows until the answer.
    pub(crate) fn status(&self, editor: &Editor) -> MeshStatus<'_> {
        let redrafted =
            self.last_answered()
                .zip(self.requested)
                .is_some_and(|(last, requested)| {
                    last.generation == requested.generation && last.draft != requested.draft
                });
        if redrafted
            || self
                .answered()
                .is_none_or(|answered| answered < editor.generation())
        {
            return MeshStatus::Regenerating;
        }
        match &self.failed {
            Some((.., error)) => MeshStatus::Failed(error),
            None => MeshStatus::Current,
        }
    }

    /// The generation of the mesh shown, if there is one yet.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> Option<Generation> {
        self.shown.map(|shown| shown.generation)
    }

    /// The sketch left out of the lines shown, if there are lines yet.
    #[cfg(test)]
    pub(crate) fn left_out(&self) -> Option<Option<FeatureId>> {
        self.shown.map(|shown| shown.exclude)
    }

    /// The revision of the draft applied to the model shown, if there's
    /// a model and it had one.
    #[cfg(test)]
    pub(crate) fn shown_draft(&self) -> Option<u64> {
        self.shown.and_then(|shown| shown.draft)
    }

    pub(crate) fn mesh(&self) -> &Arc<RenderMesh> {
        &self.mesh
    }

    pub(crate) fn sketches(&self) -> &Arc<RenderLines> {
        &self.sketches
    }

    /// The sketches that don't solve, as the model shown found.
    pub(crate) fn unsolved(&self) -> &[FeatureId] {
        &self.unsolved
    }

    /// The features that failed and why, as the model shown found: with
    /// a draft, those of the document with the draft applied.
    pub(crate) fn failed_features(&self) -> &[(FeatureId, String)] {
        &self.failed_features
    }
}

#[cfg(test)]
mod tests;
