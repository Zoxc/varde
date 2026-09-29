//! Regeneration: rebuilding the model from the document off the UI thread,
//! behind requests and responses.
//!
//! The UI sends a [`Request`] tagged with the editor generation it's of
//! (see [`Editor::generation`]) and never waits; the [`Response`] comes back
//! later tagged with the same one. Today the only work is tessellation. It
//! runs in a [`lane`] per document: natively a thread, on the web a Web
//! Worker, which shares no memory with the page, so requests and responses
//! cross it as bytes (see `src/wire.rs`). Both lanes have the same API, so
//! the app doesn't tell them apart. What was asked for and the newest mesh
//! answered are the app's to keep.
//!
//! [`Editor::generation`]: varde_document::Editor::generation

// Only the web lane uses it; it's plain Rust, so it's tested natively too.
#[cfg(any(target_arch = "wasm32", test))]
mod mailbox;
mod newest;
#[cfg(not(target_arch = "wasm32"))]
mod thread;
// Likewise.
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
use varde_document::{Document, Generation, Snapshot};
use varde_kernel::{MeshError, RenderMesh, ShapeError};

/// Carries [`Request`]s to a lane without waiting for them to be handled;
/// tests stand in for one with their own and answer with [`handle`].
pub use varde_lane::Transport;

/// Work for the regeneration side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Builds the mesh of the committed document as of `generation`.
    /// Supersedes any earlier request.
    Regenerate {
        generation: Generation,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
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
}

impl Request {
    /// The editor generation the request is of.
    pub fn generation(&self) -> Generation {
        match self {
            Request::Regenerate { generation, .. } => *generation,
        }
    }
}

/// The answer to a [`Request`].
#[derive(Debug, Clone)]
pub enum Response {
    Regenerated {
        generation: Generation,
        mesh: Arc<RenderMesh>,
    },
    /// The work for `generation` failed, e.g. the kernel panicked.
    Failed {
        generation: Generation,
        error: String,
    },
}

/// Does the work of `request`. Pure, so it can run anywhere.
pub fn handle(request: Request) -> Response {
    match request {
        Request::Regenerate {
            generation,
            document,
        } => match tessellate(&document) {
            Ok(mesh) => Response::Regenerated {
                generation,
                mesh: Arc::new(mesh),
            },
            Err(error) => Response::Failed {
                generation,
                error: error.to_string(),
            },
        },
    }
}

/// Tessellates all visible bodies of `document` into a single mesh in world
/// space. Fails if it would have more vertices, indices or edges than a
/// [`RenderMesh`] may hold, which a file with enough bodies in it can ask
/// for, or, which a checked document can't, if a shape doesn't build.
pub fn tessellate(document: &Document) -> Result<RenderMesh, TessellateError> {
    let mut mesh = RenderMesh::default();
    for body in document.bodies().iter().filter(|b| b.visible) {
        let solid = body.shape.build().map_err(TessellateError::Shape)?;
        mesh.append_at(&solid.tessellate(), body.position)
            .map_err(TessellateError::Mesh)?;
    }
    Ok(mesh)
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
