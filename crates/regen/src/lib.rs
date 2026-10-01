//! Regeneration: rebuilding the model from the document off the UI thread,
//! behind requests and responses.
//!
//! The UI sends a [`Request`] tagged with the editor generation it's of
//! (see [`Editor::generation`]) and never waits; the [`Response`] comes back
//! later tagged with the same one. The work is evaluating the feature
//! history into the bodies' solids ([`evaluate`], with an extrude being
//! set up applied as a [`Draft`]) and tessellating the visible ones,
//! flattening the visible sketches' curves and solving every sketch, to
//! tell those that don't solve, and, asked for an export, welding the
//! visible bodies ([`export`]). What it works out is kept per feature
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
mod picking;
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
use varde_kernel::{
    Aabb, Display, LinesError, ManifoldError, ManifoldMesh, MeshError, RenderLines, RenderMesh,
};
use varde_sketch::{Budget, Goal};

pub use cache::Cache;
pub use history::{BodySolid, Evaluation, evaluate, note_merge};
pub use picking::{PickChain, PickFace, Picking, PickingError, Summary};
pub use profile::{ProfileError, profile};

use cache::Keyer;
use picking::{Drawn, Scene};

/// Carries [`Request`]s to a lane without waiting for them to be handled;
/// tests stand in for one with their own and answer with [`handle`].
pub use varde_lane::Transport;

/// Work for the regeneration side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Builds the mesh and the sketch lines of the committed document as
    /// of `generation`, with `draft` applied if there is one. Supersedes
    /// any earlier regeneration still waiting, never an export.
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
    /// Welds the visible bodies of the committed `document` for export
    /// (see [`export`]), answered with [`Response::Exported`] tagged
    /// `export`, which the app chooses. Never replaced or dropped by a
    /// later request, unlike [`Request::Regenerate`]: exports wait in
    /// order, ahead of a regeneration waiting, see `src/newest.rs`.
    Export {
        export: u64,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
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
    /// panel lists to take out, with those taken out to put back. `None`
    /// where the touch test didn't run: for a new body, where the draft
    /// failed before its solid was made, or where the document refused
    /// it; `Some` of an empty list where it ran and touched nothing. See
    /// [`Evaluation::touched`].
    pub touched: Option<Vec<BodyId>>,
}

impl Response {
    /// The editor generation the response is of: `None` for an export's.
    pub fn generation(&self) -> Option<Generation> {
        match self {
            Response::Regenerated { generation, .. } | Response::Failed { generation, .. } => {
                Some(*generation)
            }
            Response::Exported { .. } => None,
        }
    }

    /// The sketch the request answered asked to leave out.
    pub fn exclude(&self) -> Option<FeatureId> {
        match self {
            Response::Regenerated { exclude, .. } | Response::Failed { exclude, .. } => *exclude,
            Response::Exported { .. } => None,
        }
    }

    /// The revision of the draft the request answered had, if any.
    pub fn draft(&self) -> Option<u64> {
        match self {
            Response::Regenerated { draft, .. } => draft.as_ref().map(|draft| draft.revision),
            Response::Failed { draft, .. } => *draft,
            Response::Exported { .. } => None,
        }
    }
}

impl Request {
    /// The editor generation a [`Request::Regenerate`] is of: `None` for
    /// an export.
    pub fn generation(&self) -> Option<Generation> {
        match self {
            Request::Regenerate { generation, .. } => Some(*generation),
            Request::Export { .. } => None,
        }
    }

    /// The sketch the request asks to leave out.
    pub fn exclude(&self) -> Option<FeatureId> {
        match self {
            Request::Regenerate { exclude, .. } => *exclude,
            Request::Export { .. } => None,
        }
    }

    /// The revision of the request's draft, if it has one.
    pub fn draft(&self) -> Option<u64> {
        match self {
            Request::Regenerate { draft, .. } => draft.as_ref().map(|draft| draft.revision),
            Request::Export { .. } => None,
        }
    }

    /// The answer to this request should handling it fail with `error`,
    /// e.g. by a panic: every request has one. Keeps only what the
    /// answer needs, not the document.
    pub fn failure(&self) -> impl FnOnce(String) -> Response + Send + use<> {
        let failed = match self {
            Request::Regenerate {
                generation,
                exclude,
                draft,
                ..
            } => Err((*generation, *exclude, draft.as_ref().map(|d| d.revision))),
            Request::Export { export, .. } => Ok(*export),
        };
        move |error| match failed {
            Err((generation, exclude, draft)) => Response::Failed {
                generation,
                exclude,
                draft,
                error,
            },
            Ok(export) => Response::Exported {
                export,
                result: Err(error),
            },
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
        /// Which face and edge each triangle and edge of `mesh` draws,
        /// and the faces' and edges' tables, see [`Picking`].
        picking: Arc<Picking>,
        /// The visible sketches' curves, see [`flatten_sketches`].
        sketches: Arc<RenderLines>,
        /// The sketches that don't solve, see [`unsolved`], in the
        /// document's order.
        unsolved: Vec<FeatureId>,
        /// The features that failed and why, in the document's order,
        /// see [`evaluate`].
        failed: Vec<(FeatureId, String)>,
        /// Each join, cut or intersect that got as far as its tool, with
        /// the bodies it touches, see [`Evaluation::touched`]: with a
        /// draft that goes, as the document with it applied found.
        touched: Vec<(FeatureId, Vec<BodyId>)>,
        /// Each body a join merged into another and the body holding it
        /// now, see [`Evaluation::merged`]: a consumed body has no solid
        /// and isn't in `bodies`.
        merged: Vec<(BodyId, BodyId)>,
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
    /// Answers [`Request::Export`] tagged `export`: the visible bodies
    /// welded, in the order the history made them, or why they can't be
    /// (see [`ExportError`]), in words to follow a colon.
    Exported {
        export: u64,
        result: Result<Vec<ExportedBody>, String>,
    },
}

/// Answers requests, keeping what it worked out for the next ones (see
/// [`Cache`]): what a lane runs.
#[derive(Default)]
pub struct Regenerator {
    cache: Cache,
}

impl Regenerator {
    /// One whose cache holds `budget` bytes (see [`Cache`]), for tests.
    #[cfg(test)]
    pub(crate) fn with_budget(budget: usize) -> Regenerator {
        Regenerator {
            cache: Cache::with_budget(budget),
        }
    }

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
                        mesh: model.scene.mesh,
                        picking: model.scene.picking,
                        sketches: Arc::new(model.sketches),
                        unsolved: model.unsolved,
                        failed: model.failed,
                        touched: model.touched,
                        merged: model.merged,
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
            // Not a request of its own to the cache: `Cache::begin` would
            // let go of the meshes the regeneration before used and an
            // export doesn't, which the next one would draw again.
            Request::Export { export, document } => {
                let evaluation = evaluate(&document, &mut self.cache);
                let result = crate::export(&document, &evaluation).map_err(|e| e.to_string());
                Response::Exported { export, result }
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
                    .map(|(_, touched)| touched.clone());
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
            Err(error) => (error, None),
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
        // Only a draft that worked is drawn: one that failed is answered
        // with the committed model.
        let drafted = draft.as_ref().is_some_and(|draft| draft.error.is_none());
        let scene = tessellate_scene(document, &evaluation, drafted, &mut self.cache)
            .map_err(|error| error.to_string())?;
        let sketches = flatten_sketches(document, exclude).map_err(|error| error.to_string())?;
        let bodies = evaluation
            .bodies
            .iter()
            .filter_map(|made| Some((made.body, made.solid.bounds()?)))
            .collect();
        Ok(Model {
            draft,
            scene,
            sketches,
            unsolved: unsolved(document, &mut self.cache),
            failed: evaluation.failed,
            touched: evaluation.touched,
            merged: evaluation.merged,
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
    scene: Scene,
    sketches: RenderLines,
    unsolved: Vec<FeatureId>,
    failed: Vec<(FeatureId, String)>,
    touched: Vec<(FeatureId, Vec<BodyId>)>,
    merged: Vec<(BodyId, BodyId)>,
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
/// and kept in `cache`. The joined mesh is kept too, filed by the shown
/// bodies and their mesh keys in order (which hold the tolerance): a
/// scene that didn't change gives the same `Arc` without joining again
/// (see [`Cache`]). Fails if it would have more vertices, indices or
/// edges than a [`RenderMesh`] may hold, which a file with enough bodies
/// in it can ask for.
pub fn tessellate(
    document: &Document,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<Arc<RenderMesh>, MeshError> {
    tessellate_scene(document, evaluation, false, cache).map(|scene| scene.mesh)
}

/// [`tessellate`], with the scene's [`Picking`] tables.
pub fn tessellate_picking(
    document: &Document,
    evaluation: &Evaluation,
    cache: &mut Cache,
) -> Result<(Arc<RenderMesh>, Arc<Picking>), MeshError> {
    tessellate_scene(document, evaluation, false, cache).map(|scene| (scene.mesh, scene.picking))
}

/// [`tessellate_picking`], for a draft's answer if `drafted`, whose scene
/// doesn't become the committed one the cache never evicts (see
/// [`Cache`]).
fn tessellate_scene(
    document: &Document,
    evaluation: &Evaluation,
    drafted: bool,
    cache: &mut Cache,
) -> Result<Scene, MeshError> {
    let fit = document.tolerance().fit().to_bits();
    let shown: Vec<_> = evaluation
        .bodies
        .iter()
        .filter(|made| document.body(made.body).is_some_and(|body| body.visible))
        .map(|made| (made, Keyer::new("mesh").key(made.key).number(fit).finish()))
        .collect();
    // The bodies too: the picking tables name them.
    let mut scene = Keyer::new("scene");
    for (made, key) in &shown {
        scene.value(&made.body).key(*key);
    }
    let filed = scene.number(shown.len() as u64).finish();
    let mut found = true;
    let scene = cache.scene(filed, drafted, |cache| {
        found = false;
        let display = Display::new(&document.tolerance());
        let mut mesh = RenderMesh::default();
        let mut picking = Picking::default();
        for (made, key) in &shown {
            let drawn = cache.mesh(*key, || Drawn::new(&made.solid, &display))?;
            mesh.append(&drawn.mesh)?;
            picking.append(made.body, &drawn)?;
        }
        Ok(Scene {
            mesh: Arc::new(mesh),
            picking: Arc::new(picking),
        })
    })?;
    if found {
        // The bodies' meshes stay for the next scene that changes one.
        for (_, key) in &shown {
            cache.keep(*key);
        }
    }
    Ok(scene)
}

/// A visible body welded for export: see [`export`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportedBody {
    pub body: BodyId,
    /// The body's name in the document.
    pub name: String,
    pub mesh: ManifoldMesh,
}

/// Why the visible bodies can't be exported: the first body, in the
/// order the history made them, whose solid gives no [`ManifoldMesh`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportError {
    pub body: BodyId,
    pub name: String,
    /// What [`ManifoldError`] the mesh failed with, in words.
    pub error: String,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} can't be exported: {}", self.name, self.error)
    }
}

impl std::error::Error for ExportError {}

/// The solids of the visible bodies of `document` in `evaluation`, in the
/// order the history made them, each welded into a closed, oriented
/// [`ManifoldMesh`] at the [`Display`] of the document's tolerance (the
/// samples it is drawn with, see [`varde_kernel::Solid::manifold_mesh`]),
/// for a file such as 3MF. None visible gives none. Fails with the first
/// body whose mesh fails its check or is too large: a body is never
/// written as something that isn't a manifold.
pub fn export(
    document: &Document,
    evaluation: &Evaluation,
) -> Result<Vec<ExportedBody>, ExportError> {
    let display = Display::new(&document.tolerance());
    let shown = (evaluation.bodies.iter())
        .filter_map(|made| Some((made, document.body(made.body).filter(|body| body.visible)?)));
    let mut out = Vec::new();
    for (made, body) in shown {
        let failed = |error: ManifoldError| ExportError {
            body: body.id,
            name: body.name.clone(),
            error: error.to_string(),
        };
        out.push(ExportedBody {
            body: body.id,
            name: body.name.clone(),
            mesh: made.solid.manifold_mesh(&display).map_err(failed)?,
        });
    }
    Ok(out)
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
