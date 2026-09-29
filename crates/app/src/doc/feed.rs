//! The mesh shown for a document, fed by the regeneration side.

use std::sync::Arc;

use varde_document::{Editor, Generation};
use varde_kernel::RenderMesh;
use varde_regen::{Request, Response, Transport};
use varde_view::MeshStatus;

/// The mesh shown for the document. It's built by the regeneration side, so it
/// may lag behind the editor: the last mesh stays shown until a newer one
/// arrives.
#[derive(Default)]
pub(crate) struct MeshFeed {
    /// Builds the meshes: the document's lane, a thread natively and a Web
    /// Worker on the web. `None` until the lane has started, see
    /// [`Varde::regen_lane`]; nothing is requested until then.
    ///
    /// [`Varde::regen_lane`]: crate::Varde::regen_lane
    regen: Option<Box<dyn Transport<Request>>>,
    mesh: Arc<RenderMesh>,
    /// The generation `mesh` is of, or `None` before the first one arrives.
    shown: Option<Generation>,
    /// The newest generation asked for.
    requested: Option<Generation>,
    /// A generation newer than `shown` whose mesh couldn't be built, and why.
    failed: Option<(Generation, String)>,
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

    /// Asks the lane for the document's mesh if the editor moved on since
    /// the last request. Nothing, not even remembering it, before the lane
    /// has started: then the first request asks for the editor's newest.
    pub(crate) fn request(&mut self, editor: &Editor) {
        let Some(regen) = &mut self.regen else {
            return;
        };
        let generation = editor.generation();
        if self
            .requested
            .is_none_or(|requested| requested < generation)
        {
            self.requested = Some(generation);
            regen.send(Request::Regenerate {
                generation,
                document: editor.snapshot(),
            });
        }
    }

    /// Shows the mesh in `response`, or its error next to the last mesh,
    /// unless a response as new was applied already. Responses arriving out
    /// of order or superseded are dropped.
    pub(crate) fn apply(&mut self, response: Response) {
        if self
            .answered()
            .is_some_and(|answered| answered >= response.generation())
        {
            return;
        }
        match response {
            Response::Regenerated { generation, mesh } => {
                self.mesh = mesh;
                self.shown = Some(generation);
                self.failed = None;
            }
            Response::Failed { generation, error } => self.failed = Some((generation, error)),
        }
    }

    /// The newest generation a response was applied for.
    fn answered(&self) -> Option<Generation> {
        self.failed
            .as_ref()
            .map(|(generation, _)| *generation)
            .or(self.shown)
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
            Some((_, error)) => MeshStatus::Failed(error),
            None => MeshStatus::Current,
        }
    }

    /// The generation of the mesh shown, if there is one yet.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> Option<Generation> {
        self.shown
    }

    pub(crate) fn mesh(&self) -> &Arc<RenderMesh> {
        &self.mesh
    }
}

#[cfg(test)]
mod tests;
