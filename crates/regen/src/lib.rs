//! Regeneration: rebuilding the model from the document off the UI thread,
//! behind requests and responses.
//!
//! The UI sends a [`Request`] tagged with the editor generation it's of
//! (see [`Editor::generation`]) and never waits; the [`Response`] comes back
//! later tagged with the same one. Today the work is tessellating the
//! bodies, flattening the visible sketches' curves and solving every
//! sketch, to tell those that don't solve. It runs in a
//! [`lane`] per document: natively a thread, on the web a Web
//! Worker, which shares no memory with the page, so requests and responses
//! cross it as bytes (see `src/wire.rs`). Both lanes have the same API, so
//! the app doesn't tell them apart. What was asked for and the newest
//! model answered are the app's to keep.
//!
//! [`Editor::generation`]: varde_document::Editor::generation

mod newest;
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
use varde_document::{Document, FeatureId, FeatureKind, Generation, Snapshot};
use varde_kernel::{Display, LinesError, MeshError, RenderLines, RenderMesh, ShapeError};
use varde_sketch::{Budget, Goal};

/// Carries [`Request`]s to a lane without waiting for them to be handled;
/// tests stand in for one with their own and answer with [`handle`].
pub use varde_lane::Transport;

/// Work for the regeneration side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Builds the mesh and the sketch lines of the committed document as
    /// of `generation`. Supersedes any earlier request.
    Regenerate {
        generation: Generation,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
        /// A sketch left out of the lines, the one being edited, which the
        /// viewport draws over everything instead.
        exclude: Option<FeatureId>,
    },
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
}

/// The answer to a [`Request`].
#[derive(Debug, Clone)]
pub enum Response {
    Regenerated {
        generation: Generation,
        /// The sketch left out of `sketches`, as the request asked: a
        /// request can ask again for the same generation with another one.
        exclude: Option<FeatureId>,
        mesh: Arc<RenderMesh>,
        /// The visible sketches' curves, see [`flatten_sketches`].
        sketches: Arc<RenderLines>,
        /// The sketches that don't solve, see [`unsolved`], in the
        /// document's order.
        unsolved: Vec<FeatureId>,
    },
    /// The work for `generation` failed, e.g. the kernel panicked. The
    /// request leaving out `exclude` did: one of the same generation
    /// leaving out another sketch may not.
    Failed {
        generation: Generation,
        exclude: Option<FeatureId>,
        error: String,
    },
}

/// Does the work of `request`. Pure, so it can run anywhere.
pub fn handle(request: Request) -> Response {
    match request {
        Request::Regenerate {
            generation,
            document,
            exclude,
        } => match regenerate(&document, exclude) {
            Ok((mesh, sketches)) => Response::Regenerated {
                generation,
                exclude,
                mesh: Arc::new(mesh),
                sketches: Arc::new(sketches),
                unsolved: unsolved(&document),
            },
            Err(error) => Response::Failed {
                generation,
                exclude,
                error,
            },
        },
    }
}

/// The mesh of `document` and the lines of its sketches but `exclude`, or
/// why they couldn't be built.
fn regenerate(
    document: &Document,
    exclude: Option<FeatureId>,
) -> Result<(RenderMesh, RenderLines), String> {
    let mesh = tessellate(document).map_err(|error| error.to_string())?;
    let sketches = flatten_sketches(document, exclude).map_err(|error| error.to_string())?;
    Ok((mesh, sketches))
}

/// Tessellates all visible bodies of `document` into a single mesh in world
/// space. Fails if it would have more vertices, indices or edges than a
/// [`RenderMesh`] may hold, which a file with enough bodies in it can ask
/// for, or if a shape doesn't build (a checked box too thin for the
/// kernel's resolution, or for its length).
pub fn tessellate(document: &Document) -> Result<RenderMesh, TessellateError> {
    let mut mesh = RenderMesh::default();
    for body in document.bodies().iter().filter(|b| b.visible) {
        let solid = body.shape.build().map_err(TessellateError::Shape)?;
        let drawn = solid
            .tessellate(&Display::default())
            .map_err(TessellateError::Mesh)?;
        mesh.append_at(&drawn, body.position)
            .map_err(TessellateError::Mesh)?;
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
        let FeatureKind::Sketch { plane, sketch } = &feature.kind;
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
/// sketch the app committed is solved, and settles at once.
pub fn unsolved(document: &Document) -> Vec<FeatureId> {
    let budget = Budget::default();
    document
        .features()
        .iter()
        .filter(|feature| {
            let FeatureKind::Sketch { sketch, .. } = &feature.kind;
            varde_sketch::solve(sketch, &Goal::Settle, &budget).is_err()
        })
        .map(|feature| feature.id)
        .collect()
}

/// Why [`tessellate`] fails.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TessellateError {
    /// A body's shape doesn't build.
    Shape(ShapeError),
    /// The bodies' meshes don't make one [`RenderMesh`].
    Mesh(MeshError),
}

impl std::fmt::Display for TessellateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TessellateError::Shape(error) => error.fmt(f),
            TessellateError::Mesh(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for TessellateError {}

#[cfg(test)]
mod tests;
