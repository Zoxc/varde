//! Regeneration: rebuilding the model from the document off the UI thread,
//! behind requests and responses.
//!
//! The UI sends a [`Request`] tagged with the editor generation it's of
//! (see [`Editor::generation`]) and never waits; the [`Response`] comes back
//! later tagged with the same one. The work is evaluating the feature
//! history into the bodies' solids ([`evaluate`], with a feature being
//! set up applied as a [`Draft`]) and tessellating the visible ones,
//! flattening the visible sketches' curves and solving every sketch, to
//! tell those that don't solve, and, asked for an export, welding the
//! visible bodies ([`export`]), or, asked by a client ([`Query`]), what
//! each of a body's edges is ([`EdgeFacts`]) or a measure. What it works out is kept per feature
//! ([`Cache`]), so an edit reruns only what it changes. It runs in a
//! [`lane`] per document: natively a thread, on the web a Web
//! Worker, which shares no memory with the page, so requests and responses
//! cross it as bytes (see `src/wire.rs`). Both lanes have the same API, so
//! the app doesn't tell them apart. What was asked for and the newest
//! model answered are the app's to keep.
//!
//! [`Editor::generation`]: varde_document::Editor::generation

mod bounded_seq;
mod bounds;
mod cache;
mod drafted;
mod draw;
mod edge_facts;
mod error_geometry;
mod history;
mod inspect;
mod naming;
mod newest;
mod pick_tables;
mod picking;
mod profile;
mod protocol;
mod regenerator;
mod sight;
/// What other crates' tests set up of regeneration.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(not(target_arch = "wasm32"))]
mod thread;
mod why;
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

/// The regeneration Web Worker: the role the web app's `serve_worker` starts it
/// in (see `crates/web/src/lib.rs`), its name in errors and its side,
/// never run by the page.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub const WORKER: varde_lane::page::Role = varde_lane::page::Role {
    id: "regen",
    name: "the regeneration worker",
    serve: crate::worker::serve,
};

pub use cache::Cache;
pub use drafted::{AlignDatums, AlignFound, Draft, Drafted, MAX_REFERENCE, ScaleFound, SweepFound};
pub use draw::{
    ExportError, ExportedBody, export, flatten_sketches, push_sketch_lines, tessellate,
    tessellate_picking, unsolved,
};
pub use edge_facts::{Convexity, EdgeFacts};
pub use error_geometry::{ErrorGeometry, FeatureFailure, GeometryError, GeometryParts};
pub use history::scale::{AXIS_SINE, along_axis};
pub use history::{BodySolid, Evaluation, evaluate, merged_into, note_merge};
pub use inspect::{
    At, Between, EdgeForm, Entity, Gap, Inspect, InspectPick, Inspected, Measure, Probed,
};
pub use naming::{Naming, Shown, Unnamed};
pub use pick_tables::{Bvh, BvhNode, PickTables, PickTablesError, PickTablesParts, vertex_runs};
pub use picking::{
    FaceEdgeTables, PickCorner, PickFace, Picking, PickingError, PickingParts, Summary,
};
pub use profile::{ProfileError, chain_closes, loft_corner, loft_corners, profile};
pub use protocol::{Answer, Asked, Progress, Query, Regenerated, Request, Response, Stage};
pub use regenerator::{Regenerator, handle};
pub use sight::{Detail, Sight};

/// Carries [`Request`]s to a lane without waiting for them to be handled;
/// tests stand in for one with their own and answer with [`handle`].
pub use varde_lane::Transport;

#[cfg(test)]
mod tests;
