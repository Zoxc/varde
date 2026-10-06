//! The sketch solver off the UI thread, behind requests and responses.
//!
//! The UI sends a [`Request`] and never waits; the [`Response`] comes back
//! later tagged with what it answers ([`Tag`]): the revision a proposal
//! was made on, a drag's session, the revision analysed. The work is
//! [`varde_sketch`]'s pure solver: proposals ([`propose`](varde_sketch::propose)),
//! drag steps ([`DragSession`]) and analyses. It runs in a [`lane`] per
//! document: natively a thread, on the web a Web Worker, which shares no
//! memory with the page, so requests and responses cross it as bytes (see
//! `src/wire.rs`). Both lanes have the same API, so the app doesn't tell
//! them apart.
//!
//! Proposals and analyses are taken in order, drag steps latest wins, the
//! two taking turns (see `src/order.rs`). A lane answers with a
//! [`Solver`], which keeps the drag in progress between its steps; tests
//! answer with one of their own.

mod clock;
mod order;
#[cfg(not(target_arch = "wasm32"))]
mod thread;
// Only the web lane uses it; it's plain Rust, so it's tested natively too.
#[cfg(any(target_arch = "wasm32", test))]
mod wire;
#[cfg(target_arch = "wasm32")]
mod worker;

/// Handles one document's requests away from the UI thread: natively a
/// thread (`src/thread.rs`), on the web a Web Worker (`src/worker.rs`).
/// Both have the same API: [`spawn`](lane::spawn) starts one and returns
/// the [`Lane`](lane::Lane) to send requests through and the
/// [`Responses`](lane::Responses) stream to read.
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

/// The solver Web Worker's side, run by the web app's `serve_worker` in
/// the role [`WORKER_ROLE`] (see `crates/web/src/lib.rs`), never by the
/// page.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use crate::worker::serve as serve_worker;

/// The role the web app starts the solver Web Worker in.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub const WORKER_ROLE: &str = "solve";

use std::sync::Arc;
use std::time::Duration;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use varde_document::{Design, LengthUnit, MAX_COORD, Revision};
use varde_sketch::{Analysis, Budget, DragSession, Id, Rejected, Sketch, SketchEdit};

/// Carries [`Request`]s to a lane without waiting for them to be handled;
/// tests stand in for one with their own and answer with a [`Solver`].
pub use varde_lane::Transport;

/// How long each solve of a proposal may take before it's refused as out
/// of time: far longer than any sketch the benchmark measured needs (see
/// `notes/SketchImpl.md`), so only a pathological one is refused.
pub const PROPOSAL_TIME: Duration = Duration::from_secs(2);

/// How long a drag step may take before it's given up, keeping the last
/// solution: a few frames, so a slow step doesn't hold up the next.
pub const DRAG_TIME: Duration = Duration::from_millis(100);

/// Work for the solver. Each carries the design's `units`, which checking
/// a sketch reads its dimensions' expressions in: a sketch sent has passed
/// [`Sketch::check`] against [`MAX_COORD`] and them ([`design`]).
#[derive(Debug, Clone)]
pub enum Request {
    /// Applies `edit` to `sketch`, the committed sketch as of revision
    /// `base`, then solves and analyses the result, see
    /// [`propose`](varde_sketch::propose). Proposals are handled in the
    /// order sent.
    Propose {
        base: Revision,
        sketch: Arc<Sketch>,
        edit: SketchEdit,
        units: LengthUnit,
    },
    /// A step of the drag `session`: drags `points` and circles' `radii`
    /// towards their targets, from the last step's solution, see
    /// [`DragSession::step`]. Latest wins: a newer step, of this session or
    /// a newer one, replaces one waiting, and a step of an older session
    /// than one sent is dropped.
    ///
    /// `sketch` is where the session starts, and is the same for all its
    /// steps: only the first step of a session uses it. The lane keeps the
    /// session, so on the web later steps cross to the worker without it.
    Drag {
        session: u64,
        sketch: Arc<Sketch>,
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
        units: LengthUnit,
    },
    /// Analyses `sketch`, the committed sketch as of `revision`, e.g. on
    /// entering it. A newer analysis replaces one still waiting.
    Analyse {
        revision: Revision,
        sketch: Arc<Sketch>,
        units: LengthUnit,
    },
}

/// What a [`Response`] answers: a request's kind and what the app tells
/// its answers by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tag {
    /// A proposal made on this revision.
    Propose(Revision),
    /// A step of this drag session.
    Drag(u64),
    /// An analysis of this revision.
    Analyse(Revision),
}

impl Request {
    pub fn tag(&self) -> Tag {
        match self {
            Request::Propose { base, .. } => Tag::Propose(*base),
            Request::Drag { session, .. } => Tag::Drag(*session),
            Request::Analyse { revision, .. } => Tag::Analyse(*revision),
        }
    }

    /// The design's units it carries.
    pub fn units(&self) -> LengthUnit {
        match self {
            Request::Propose { units, .. }
            | Request::Drag { units, .. }
            | Request::Analyse { units, .. } => *units,
        }
    }
}

/// The answer to a [`Request`].
#[derive(Debug, Clone)]
pub enum Response {
    /// The proposal on `base` can be committed: the sketch it makes,
    /// solved, and its analysis.
    Accepted {
        base: Revision,
        sketch: Arc<Sketch>,
        analysis: Arc<Analysis>,
    },
    /// The proposal on `base` is refused, see [`Rejected`].
    Rejected { base: Revision, why: Rejected },
    /// A drag step of `session` solved to `solution`. A step that doesn't
    /// converge isn't answered: what's shown stays at the last solution.
    Dragged { session: u64, solution: Arc<Sketch> },
    /// The sketch of `revision` analysed.
    Analysed {
        revision: Revision,
        analysis: Arc<Analysis>,
    },
    /// The request `tag` names failed: the solver panicked, or on the web
    /// its worker stopped or answered with something broken.
    Failed { tag: Tag, error: String },
}

impl Response {
    pub fn tag(&self) -> Tag {
        match self {
            Response::Accepted { base, .. } | Response::Rejected { base, .. } => {
                Tag::Propose(*base)
            }
            Response::Dragged { session, .. } => Tag::Drag(*session),
            Response::Analysed { revision, .. } => Tag::Analyse(*revision),
            Response::Failed { tag, .. } => *tag,
        }
    }
}

/// The coordinate limit solutions keep within, the document's.
const MAX: f64 = MAX_COORD as f64;

/// What sketches are checked against in a design of `units`, as the
/// document does ([`Document::design`](varde_document::Document::design)).
pub fn design(units: LengthUnit) -> Design {
    Design { max: MAX, units }
}

/// Answers requests, keeping the drag in progress between its steps: a
/// lane has one, and so can a test standing in for a lane.
#[derive(Debug, Default)]
pub struct Solver {
    /// The drag session in progress, if any, and its id.
    drag: Option<(u64, DragSession)>,
}

impl Solver {
    /// Does the work of `request`: `None` for a drag step that didn't
    /// converge, which isn't answered.
    pub fn handle(&mut self, request: Request) -> Option<Response> {
        match request {
            Request::Propose {
                base,
                sketch,
                edit,
                units,
            } => Some(propose(base, &sketch, &edit, units)),
            Request::Drag {
                session,
                sketch,
                points,
                radii,
                units,
            } => self.drag(session, Some(&sketch), points, radii, units),
            Request::Analyse {
                revision, sketch, ..
            } => Some(analyse(revision, &sketch)),
        }
    }

    /// A step of the drag `session`, from where its last step left off,
    /// or from `sketch` if the session is new: a session other than the
    /// one in progress replaces it. Without a sketch to start a new
    /// session from, or if the step doesn't converge within [`DRAG_TIME`],
    /// `None`.
    pub(crate) fn drag(
        &mut self,
        session: u64,
        sketch: Option<&Sketch>,
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
        units: LengthUnit,
    ) -> Option<Response> {
        if self.drag.as_ref().is_none_or(|(id, _)| *id != session) {
            let started = DragSession::new(sketch?.clone(), design(units));
            self.drag = Some((session, started));
        }
        let (_, drag) = self.drag.as_mut()?;
        let expired = clock::deadline(DRAG_TIME);
        let solution = drag.step(points, radii, &budget(&expired)).ok()?;
        Some(Response::Dragged {
            session,
            solution: Arc::new(solution.clone()),
        })
    }
}

/// The proposal of `edit` on `sketch`, the committed sketch of `base`.
fn propose(base: Revision, sketch: &Sketch, edit: &SketchEdit, units: LengthUnit) -> Response {
    let expired = clock::deadline(PROPOSAL_TIME);
    match varde_sketch::propose(sketch, edit, &design(units), &budget(&expired)) {
        Ok(accepted) => Response::Accepted {
            base,
            sketch: Arc::new(accepted.sketch),
            analysis: Arc::new(accepted.analysis),
        },
        Err(why) => Response::Rejected { base, why },
    }
}

/// The analysis of `sketch`, the committed sketch of `revision`.
fn analyse(revision: Revision, sketch: &Sketch) -> Response {
    Response::Analysed {
        revision,
        analysis: Arc::new(varde_sketch::analyse(sketch)),
    }
}

/// The solver's default iterations, and time until `expired` says so.
fn budget(expired: &dyn Fn() -> bool) -> Budget<'_> {
    Budget {
        expired,
        ..Budget::default()
    }
}

#[cfg(test)]
mod tests;
