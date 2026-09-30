//! Regeneration: rebuilding the model from the document off the UI thread,
//! behind requests and responses.
//!
//! The UI sends a [`Request`] tagged with the editor generation it's of
//! (see [`Editor::generation`]) and never waits; the [`Response`] comes back
//! later tagged with the same one. The work is evaluating the feature
//! history into the bodies' solids ([`evaluate`], with an extrude being
//! set up applied as a [`Draft`]) and tessellating the visible ones,
//! flattening the visible sketches' curves and solving every sketch, to
//! tell those that don't solve. What it works out is kept per feature
//! ([`Cache`]), so an edit reruns only what it changes. It runs in a
//! [`lane`] per document: natively a thread, on the web a Web
//! Worker, which shares no memory with the page, so requests and responses
//! cross it as bytes (see `src/wire.rs`). Both lanes have the same API, so
//! the app doesn't tell them apart. What was asked for and the newest
//! model answered are the app's to keep.
//!
//! [`Editor::generation`]: varde_document::Editor::generation

mod cache;
mod history;
mod message;
mod newest;
mod profile;
#[cfg(not(target_arch = "wasm32"))]
mod thread;
// Only the web lane uses it; it's plain Rust, so it's tested natively too.
#[cfg(any(target_arch = "wasm32", test))]
mod wire;
#[cfg(target_arch = "wasm32")]
mod worker;

/// Handles one document's requests away from the UI thread, latest wins:
/// natively a thread (`src/thread.rs`), on the web a Web Worker
/// (`src/worker.rs`). Both have the same API: [`spawn`](lane::spawn) starts
/// one and returns the [`Lane`](lane::Lane) to send requests through and
/// the [`Responses`](lane::Responses) stream to read.
pub mod lane {
    use crate::{Request, Response};

    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::thread::spawn;
    #[cfg(target_arch = "wasm32")]
    pub use crate::worker::spawn;

    /// Sends requests to a lane. Cheap to clone; all clones feed the same
    /// thread or worker.
    pub type Lane = varde_lane::Lane<Request>;

    /// The responses of a lane, as a stream. Dropping it ends the lane:
    /// natively once the job running, if any, is done, on the web at once,
    /// terminating the worker even mid-job.
    pub type Responses = varde_lane::Responses<Request, Response>;
}

/// The regeneration Web Worker's side, run by its `main` (see
/// `src/bin/varde-regen-worker.rs`), never by the page.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use crate::worker::serve as serve_worker;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use varde_document::{
    BodyId, Command, Document, Editor, Extrude, FeatureId, FeatureKind, Generation, Snapshot,
};
use varde_kernel::{Aabb, Display, LinesError, MeshError, RenderLines, RenderMesh};
use varde_sketch::{Budget, Goal};

pub use cache::Cache;
pub use history::{BodySolid, Evaluation, evaluate};
pub use profile::{ProfileError, profile};

use cache::Keyer;

/// Carries [`Request`]s to a lane without waiting for them to be handled;
/// tests stand in for one with their own and answer with [`handle`].
pub use varde_lane::Transport;

/// Work for the regeneration side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Builds the mesh and the sketch lines of the committed document as
    /// of `generation`, with `draft` applied if there is one. Supersedes
    /// any earlier request.
    Regenerate {
        generation: Generation,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
        /// A sketch left out of the lines, the one being edited, which the
        /// viewport draws over everything instead.
        exclude: Option<FeatureId>,
        /// An extrude being set up and not committed yet, answered as if
        /// it were.
        draft: Option<Draft>,
    },
}

/// An extrude being set up, new or edited, that isn't committed: a
/// request answers with it applied, as [`Command::AddExtrude`] (with
/// [`Operation::NewBody`] holding [`BodyId::NEW`]) or
/// [`Command::SetExtrude`] would apply it, for a preview.
///
/// [`Operation::NewBody`]: varde_document::Operation::NewBody
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    /// Counted up by the app with every change to the draft, so the
    /// newest answer can be told apart from older ones of the same
    /// generation.
    pub revision: u64,
    /// The extrude being edited, or `None` for a new one.
    pub feature: Option<FeatureId>,
    pub extrude: Extrude,
}

/// How a [`Draft`] went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drafted {
    /// The draft's [`Draft::revision`].
    pub revision: u64,
    /// Why the draft gives no solid, or can't be applied: the model
    /// answered is then the committed one, without it.
    pub error: Option<String>,
    /// For a join, cut or intersect, the bodies its solid touches, less
    /// those taken out of it, in the order they were made: those the
    /// panel lists to take out, with those taken out to put back. Empty
    /// for a new body, or where the draft failed before its solid was
    /// made. See [`Evaluation::touched`].
    pub touched: Vec<BodyId>,
}

impl Response {
    /// The editor generation the response is of.
    pub fn generation(&self) -> Generation {
        match self {
            Response::Regenerated { generation, .. } | Response::Failed { generation, .. } => {
                *generation
            }
        }
    }

    /// The sketch the request answered asked to leave out.
    pub fn exclude(&self) -> Option<FeatureId> {
        match self {
            Response::Regenerated { exclude, .. } | Response::Failed { exclude, .. } => *exclude,
        }
    }

    /// The revision of the draft the request answered had, if any.
    pub fn draft(&self) -> Option<u64> {
        match self {
            Response::Regenerated { draft, .. } => draft.as_ref().map(|draft| draft.revision),
            Response::Failed { draft, .. } => *draft,
        }
    }
}

impl Request {
    /// The editor generation the request is of.
    pub fn generation(&self) -> Generation {
        match self {
            Request::Regenerate { generation, .. } => *generation,
        }
    }

    /// The sketch the request asks to leave out.
    pub fn exclude(&self) -> Option<FeatureId> {
        match self {
            Request::Regenerate { exclude, .. } => *exclude,
        }
    }

    /// The revision of the request's draft, if it has one.
    pub fn draft(&self) -> Option<u64> {
        match self {
            Request::Regenerate { draft, .. } => draft.as_ref().map(|draft| draft.revision),
        }
    }
}

/// The answer to a [`Request`].
#[derive(Debug, Clone)]
pub enum Response {
    Regenerated {
        generation: Generation,
        /// The sketch left out of `sketches`, as the request asked: a
        /// request can ask again for the same generation with another one.
        exclude: Option<FeatureId>,
        /// How the request's draft went, if it had one.
        draft: Option<Drafted>,
        mesh: Arc<RenderMesh>,
        /// The visible sketches' curves, see [`flatten_sketches`].
        sketches: Arc<RenderLines>,
        /// The sketches that don't solve, see [`unsolved`], in the
        /// document's order.
        unsolved: Vec<FeatureId>,
        /// The features that failed and why, in the document's order,
        /// see [`evaluate`].
        failed: Vec<(FeatureId, String)>,
        /// The box around each body that has a solid, shown or not, in
        /// the order they were made.
        bodies: Vec<(BodyId, Aabb)>,
    },
    /// The work for `generation` failed, e.g. the kernel panicked. The
    /// request leaving out `exclude` did: one of the same generation
    /// leaving out another sketch may not.
    Failed {
        generation: Generation,
        exclude: Option<FeatureId>,
        /// The revision of the request's draft, if it had one.
        draft: Option<u64>,
        error: String,
    },
}

/// Answers requests, keeping what it worked out for the next ones (see
/// [`Cache`]): what a lane runs.
#[derive(Default)]
pub struct Regenerator {
    cache: Cache,
}

impl Regenerator {
    /// Does the work of `request`.
    pub fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Regenerate {
                generation,
                document,
                exclude,
                draft,
            } => {
                self.cache.begin();
                match self.regenerate(&document, exclude, draft.as_ref()) {
                    Ok(model) => Response::Regenerated {
                        generation,
                        exclude,
                        draft: model.draft,
                        mesh: Arc::new(model.mesh),
                        sketches: Arc::new(model.sketches),
                        unsolved: model.unsolved,
                        failed: model.failed,
                        bodies: model.bodies,
                    },
                    Err(error) => Response::Failed {
                        generation,
                        exclude,
                        draft: draft.map(|draft| draft.revision),
                        error,
                    },
                }
            }
        }
    }

    /// The model of `document` with `draft` applied, or without it if it
    /// fails, leaving out the lines of the sketch `exclude`.
    fn regenerate(
        &mut self,
        document: &Document,
        exclude: Option<FeatureId>,
        draft: Option<&Draft>,
    ) -> Result<Model, String> {
        let Some(draft) = draft else {
            return self.model(document, exclude, None);
        };
        let (error, touched) = match applied(document, draft) {
            Ok((drafted, feature)) => {
                let evaluation = evaluate(&drafted, &mut self.cache);
                let touched = (evaluation.touched.iter())
                    .find(|(id, _)| *id == feature)
                    .map(|(_, touched)| touched.clone())
                    .unwrap_or_default();
                match evaluation.failed.iter().find(|(id, _)| *id == feature) {
                    Some((_, error)) => (error.clone(), touched),
                    None => {
                        let done = Drafted {
                            revision: draft.revision,
                            error: None,
                            touched,
                        };
                        return self.draw(&drafted, evaluation, exclude, Some(done));
                    }
                }
            }
            Err(error) => (error, Vec::new()),
        };
        let failed = Drafted {
            revision: draft.revision,
            error: Some(error),
            touched,
        };
        self.model(document, exclude, Some(failed))
    }

    /// The model of `document`.
    fn model(
        &mut self,
        document: &Document,
        exclude: Option<FeatureId>,
        draft: Option<Drafted>,
    ) -> Result<Model, String> {
        let evaluation = evaluate(document, &mut self.cache);
        self.draw(document, evaluation, exclude, draft)
    }

    /// The model of `document`, whose history gave `evaluation`.
    fn draw(
        &mut self,
        document: &Document,
        evaluation: Evaluation,
        exclude: Option<FeatureId>,
        draft: Option<Drafted>,
    ) -> Result<Model, String> {
        let mesh = tessellate(document, &evaluation, &mut self.cache)
            .map_err(|error| error.to_string())?;
        let sketches = flatten_sketches(document, exclude).map_err(|error| error.to_string())?;
        let bodies = evaluation
            .bodies
            .iter()
            .filter_map(|made| Some((made.body, made.solid.bounds()?)))
            .collect();
        Ok(Model {
            draft,
            mesh,
            sketches,
            unsolved: unsolved(document, &mut self.cache),
            failed: evaluation.failed,
            bodies,
        })
    }

    /// What the regenerator's cache has found and worked out so far.
    pub fn cache(&self) -> &Cache {
        &self.cache
    }
}

/// A [`Response::Regenerated`]'s model.
struct Model {
    draft: Option<Drafted>,
    mesh: RenderMesh,
    sketches: RenderLines,
    unsolved: Vec<FeatureId>,
    failed: Vec<(FeatureId, String)>,
    bodies: Vec<(BodyId, Aabb)>,
}

/// `document` with `draft` applied, and the draft's feature, or why it
/// can't be applied.
fn applied(document: &Document, draft: &Draft) -> Result<(Document, FeatureId), String> {
    let extrude = Box::new(draft.extrude.clone());
    let command = match draft.feature {
        None => document.add_extrude(draft.extrude.clone()),
        Some(feature) => {
            // `SetExtrude` leaves anything else as it is, which would
            // answer the draft as applied.
            let is_extrude = document
                .feature(feature)
                .is_some_and(|feature| matches!(feature.kind, FeatureKind::Extrude(_)));
            if !is_extrude {
                return Err("the draft's feature isn't an extrude".to_owned());
            }
            Command::SetExtrude { feature, extrude }
        }
    };
    let mut editor = Editor::new(document.clone());
    editor.apply(command).map_err(|error| error.to_string())?;
    let drafted = editor.document();
    let feature = match draft.feature {
        Some(feature) => feature,
        None => {
            drafted
                .features()
                .last()
                .ok_or("the draft wasn't added")?
                .id
        }
    };
    Ok((drafted.clone(), feature))
}

/// Does the work of `request` without a cache. Pure, so it can run
/// anywhere.
pub fn handle(request: Request) -> Response {
    Regenerator::default().handle(request)
}

/// Tessellates the solids of the visible bodies of `document` in
/// `evaluation` into a single mesh in world space, within the [`Display`]
/// of the document's tolerance ([`Document::tolerance`]), each drawn once
/// and kept in `cache`. Fails if it would have more vertices, indices or
/// edges than a [`RenderMesh`] may hold, which a file with enough bodies
/// in it can ask for.
pub fn tessellate(
    document: &Document,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<RenderMesh, MeshError> {
    let display = Display::new(&document.tolerance());
    let mut mesh = RenderMesh::default();
    let shown = evaluation
        .bodies
        .iter()
        .filter(|made| document.body(made.body).is_some_and(|body| body.visible));
    for made in shown {
        let key = Keyer::new("mesh")
            .key(made.key)
            .number(document.tolerance().fit().to_bits())
            .finish();
        let drawn = cache.mesh(key, || made.solid.tessellate(&display))?;
        mesh.append(&drawn)?;
    }
    Ok(mesh)
}

/// The non-construction curves of the visible sketches of `document`,
/// except `exclude`, flattened (see [`Sketch::flatten`]), lines without
/// the ends fillets and chamfers cut off ([`Sketch::cut_back`]), and placed on
/// their planes in world space, a polyline each. Fails if there would be
/// more points than [`RenderLines`] may hold, which a file with enough
/// sketches in it can ask for.
///
/// [`Sketch::flatten`]: varde_document::Sketch::flatten
/// [`Sketch::cut_back`]: varde_document::Sketch::cut_back
pub fn flatten_sketches(
    document: &Document,
    exclude: Option<FeatureId>,
) -> Result<RenderLines, LinesError> {
    let mut lines = RenderLines::default();
    let shown = document
        .features()
        .iter()
        .filter(|feature| feature.visible && Some(feature.id) != exclude);
    for feature in shown {
        let FeatureKind::Sketch { plane, sketch } = &feature.kind else {
            continue;
        };
        let placement = plane.placement();
        let cut_back = sketch.cut_back();
        for entry in sketch.curves.iter().filter(|entry| !entry.construction) {
            // A checked sketch's curves name its own points.
            let Some(mut polyline) = sketch.flatten(&entry.curve) else {
                continue;
            };
            // What a fillet or chamfer cuts off a line isn't drawn.
            if let (Some(kept), &[start, end]) = (cut_back.get(&entry.id), &polyline[..]) {
                match varde_sketch::cut_line(start, end, *kept).0 {
                    Some(kept) => polyline = kept.to_vec(),
                    None => continue,
                }
            }
            lines.push(
                polyline
                    .into_iter()
                    .map(|at| placement.to_world(at).as_vec3()),
            )?;
        }
    }
    Ok(lines)
}

/// The sketches of `document` that don't solve, warm started from where
/// their points are, within the default [`Budget`]: those a file holds
/// that a different build's solver solved, or that were never solved. A
/// sketch the app committed is solved, and settles at once. Each sketch
/// is solved once and kept in `cache`.
pub fn unsolved(document: &Document, cache: &mut Cache) -> Vec<FeatureId> {
    let budget = Budget::default();
    document
        .features()
        .iter()
        .filter(|feature| {
            let FeatureKind::Sketch { sketch, .. } = &feature.kind else {
                return false;
            };
            let key = Keyer::new("solves").value(sketch).finish();
            !cache.solves(key, || {
                varde_sketch::solve(sketch, &Goal::Settle, &budget).is_ok()
            })
        })
        .map(|feature| feature.id)
        .collect()
}

#[cfg(test)]
mod tests;
