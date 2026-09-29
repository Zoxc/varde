//! The model shown for a document, its mesh and its finished sketches' lines,
//! fed by the regeneration side.

use std::sync::Arc;

use varde_document::{Editor, FeatureId, Generation};
use varde_kernel::{RenderLines, RenderMesh};
use varde_regen::{Request, Response, Transport};
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
    /// The generation `mesh` is of and the sketch left out of `sketches`,
    /// or `None` before the first one arrives.
    shown: Option<(Generation, Option<FeatureId>)>,
    /// The newest generation asked for, and the sketch it asked to leave
    /// out.
    requested: Option<(Generation, Option<FeatureId>)>,
    /// A generation at least as new as `shown` whose model couldn't be
    /// built leaving out the sketch given, and why.
    failed: Option<(Generation, Option<FeatureId>, String)>,
}

impl MeshFeed {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Starts sending requests to `lane`, the document's regeneration
    /// lane, once it has started. What to ask for is up to the next
    /// [`MeshFeed::request`].
    pub(crate) fn connect(&mut self, lane: impl Transport<Request> + 'static) {
        self.regen = Some(Box::new(lane));
    }

    /// Whether the lane has started, see [`MeshFeed::connect`].
    #[cfg(test)]
    pub(crate) fn connected(&self) -> bool {
        self.regen.is_some()
    }

    /// Asks the lane for the document's mesh, leaving the sketch `exclude`
    /// out of the lines, if the editor moved on since the last request or
    /// the sketch to leave out changed, as it does on entering or leaving
    /// one. Nothing, not even remembering it, before the lane has started:
    /// then the first request asks for the editor's newest.
    pub(crate) fn request(&mut self, editor: &Editor, exclude: Option<FeatureId>) {
        let Some(regen) = &mut self.regen else {
            return;
        };
        let generation = editor.generation();
        if self.requested.is_none_or(|(requested, left_out)| {
            requested < generation || (requested == generation && left_out != exclude)
        }) {
            self.requested = Some((generation, exclude));
            regen.send(Request::Regenerate {
                generation,
                document: editor.snapshot(),
                exclude,
                draft: None,
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
        match response {
            Response::Regenerated {
                generation,
                exclude,
                mesh,
                sketches,
                unsolved,
                ..
            } => {
                self.mesh = mesh;
                self.sketches = sketches;
                self.unsolved = unsolved;
                self.shown = Some((generation, exclude));
                self.failed = None;
            }
            Response::Failed {
                generation,
                exclude,
                error,
                ..
            } => self.failed = Some((generation, exclude, error)),
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
        let generation = response.generation();
        if generation != answered {
            return generation > answered;
        }
        let exclude = response.exclude();
        let failed = self
            .failed
            .as_ref()
            .is_some_and(|(failed, left_out, _)| (*failed, *left_out) == (generation, exclude));
        self.requested
            .is_some_and(|(_, left_out)| left_out == exclude)
            && self.shown != Some((generation, exclude))
            && !failed
    }

    /// The newest generation a response was applied for.
    fn answered(&self) -> Option<Generation> {
        self.failed
            .as_ref()
            .map(|(generation, ..)| *generation)
            .or(self.shown.map(|(generation, _)| generation))
    }

    /// How the mesh shown stands against the editor's document. A failure
    /// of an older generation than the editor's isn't reported: that document
    /// is gone, and the current one is still being built.
    pub(crate) fn status(&self, editor: &Editor) -> MeshStatus<'_> {
        if self
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
        self.shown.map(|(generation, _)| generation)
    }

    /// The sketch left out of the lines shown, if there are lines yet.
    #[cfg(test)]
    pub(crate) fn left_out(&self) -> Option<Option<FeatureId>> {
        self.shown.map(|(_, exclude)| exclude)
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
}

#[cfg(test)]
mod tests;
