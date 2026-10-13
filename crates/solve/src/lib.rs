//! The sketch solver off the UI thread, behind requests and responses.
//!
//! The UI sends a [`Request`] and never waits; the [`Response`] comes back
//! later tagged with what it answers ([`Tag`]): the revision a proposal
//! was made on, a drag's session, the revision analysed. The work is
//! [`varde_sketch`]'s pure solver: proposals ([`propose`](varde_sketch::propose)),
//! drag steps ([`DragSession`]) and analyses, the user's and a client's
//! queries'. It runs in a [`lane`] per
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

/// The solver Web Worker: the role the web app's `serve_worker` starts it
/// in (see `crates/web/src/lib.rs`), its name in errors and its side,
/// never run by the page.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub const WORKER: varde_lane::page::Role = varde_lane::page::Role {
    id: "solve",
    name: "the solver worker",
    serve: crate::worker::serve,
};

use std::sync::Arc;
use std::time::Duration;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use varde_document::{Design, LengthUnit, MAX_COORD, Params, Revision};
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

/// Work for the solver. Each carries the design's `units` and `params`,
/// resolved, which checking a sketch reads its dimensions' expressions
/// with: a sketch sent has passed [`Sketch::check`] against [`MAX_COORD`]
/// and them ([`design`]).
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
        params: Arc<Params>,
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
        params: Arc<Params>,
    },
    /// Analyses `sketch`, the committed sketch as of `revision`: the
    /// user's, e.g. on entering it, with `query` `None`, or for a client's
    /// query of that id (the MCP `sketch` tool), any sketch, open or not.
    /// A newer analysis of the user's replaces one of theirs still
    /// waiting; a client's queues in order like a proposal, never
    /// replacing the user's waiting one nor replaced by it or another.
    Analyse {
        query: Option<u64>,
        revision: Revision,
        sketch: Arc<Sketch>,
        units: LengthUnit,
        params: Arc<Params>,
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
    /// The user's analysis of this revision.
    Analyse(Revision),
    /// The analysis for a client's query of this id.
    Query(u64),
}

impl Tag {
    /// An analysis's: of `revision` for the user, or for the client's
    /// `query`.
    pub fn analysis(revision: Revision, query: Option<u64>) -> Tag {
        match query {
            Some(id) => Tag::Query(id),
            None => Tag::Analyse(revision),
        }
    }
}

impl Request {
    pub fn tag(&self) -> Tag {
        self.job().tag()
    }

    /// The work it asks for, see [`Solver::answer`].
    pub fn job(&self) -> Job<'_> {
        let design = self.design();
        match self {
            Request::Propose {
                base, sketch, edit, ..
            } => Job::Propose {
                base: *base,
                sketch,
                edit,
                design,
            },
            Request::Drag {
                session,
                sketch,
                points,
                radii,
                ..
            } => Job::Drag {
                session: *session,
                start: Some((sketch, design)),
                points,
                radii,
            },
            Request::Analyse {
                query,
                revision,
                sketch,
                ..
            } => Job::Analyse {
                query: *query,
                revision: *revision,
                sketch,
            },
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

    /// The design's parameters it carries.
    pub fn params(&self) -> &Params {
        match self {
            Request::Propose { params, .. }
            | Request::Drag { params, .. }
            | Request::Analyse { params, .. } => params,
        }
    }

    /// What its sketch is checked against, see [`design`].
    pub fn design(&self) -> Design<'_> {
        design(self.units(), self.params())
    }
}

/// The work a request asks for, borrowed from a [`Request`] or from one
/// decoded on the worker (`wire::Decoded`): what
/// [`Solver::answer`] does, so both lanes answer alike.
#[derive(Debug, Clone, Copy)]
pub enum Job<'a> {
    Propose {
        base: Revision,
        sketch: &'a Sketch,
        edit: &'a SketchEdit,
        design: Design<'a>,
    },
    /// `start` is the sketch a new session starts from, in its design:
    /// `None` once the worker has the session.
    Drag {
        session: u64,
        start: Option<(&'a Sketch, Design<'a>)>,
        points: &'a [(Id, DVec2)],
        radii: &'a [(Id, f64)],
    },
    Analyse {
        query: Option<u64>,
        revision: Revision,
        sketch: &'a Sketch,
    },
}

impl Job<'_> {
    pub fn tag(&self) -> Tag {
        match self {
            Job::Propose { base, .. } => Tag::Propose(*base),
            Job::Drag { session, .. } => Tag::Drag(*session),
            Job::Analyse {
                revision, query, ..
            } => Tag::analysis(*revision, *query),
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
    /// The sketch of `revision` analysed, for the user or the client's
    /// `query`, as asked.
    Analysed {
        query: Option<u64>,
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
            Response::Analysed {
                revision, query, ..
            } => Tag::analysis(*revision, *query),
            Response::Failed { tag, .. } => *tag,
        }
    }
}

/// The coordinate limit solutions keep within, the document's.
const MAX: f64 = MAX_COORD as f64;

/// What sketches are checked against in a design of `units` and
/// `params`, as the document does
/// ([`Document::design`](varde_document::Document::design)).
pub fn design(units: LengthUnit, params: &Params) -> Design<'_> {
    Design {
        max: MAX,
        units,
        params,
    }
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
        self.answer(request.job())
    }

    /// Does `job`, the one dispatch both a [`Request`] and a request
    /// decoded on the worker go through: `None` for a drag step that
    /// didn't converge, which isn't answered.
    pub fn answer(&mut self, job: Job) -> Option<Response> {
        match job {
            Job::Propose {
                base,
                sketch,
                edit,
                design,
            } => Some(propose(base, sketch, edit, &design)),
            Job::Drag {
                session,
                start,
                points,
                radii,
            } => self.drag(session, start, points.to_vec(), radii.to_vec()),
            Job::Analyse {
                query,
                revision,
                sketch,
            } => Some(analyse(query, revision, sketch)),
        }
    }

    /// A step of the drag `session`, from where its last step left off,
    /// or from `sketch` in its design if the session is new: a session
    /// other than the one in progress replaces it. Without a sketch to start a new
    /// session from, or if the step doesn't converge within [`DRAG_TIME`],
    /// `None`.
    pub(crate) fn drag(
        &mut self,
        session: u64,
        sketch: Option<(&Sketch, Design)>,
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
    ) -> Option<Response> {
        if self.drag.as_ref().is_none_or(|(id, _)| *id != session) {
            let (sketch, design) = sketch?;
            let started = DragSession::new(sketch.clone(), &design);
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
fn propose(base: Revision, sketch: &Sketch, edit: &SketchEdit, design: &Design) -> Response {
    let expired = clock::deadline(PROPOSAL_TIME);
    match varde_sketch::propose(sketch, edit, design, &budget(&expired)) {
        Ok(accepted) => Response::Accepted {
            base,
            sketch: Arc::new(accepted.sketch),
            analysis: Arc::new(accepted.analysis),
        },
        Err(why) => Response::Rejected { base, why },
    }
}

/// The analysis of `sketch`, the committed sketch of `revision`, for the
/// user or the client's `query`.
fn analyse(query: Option<u64>, revision: Revision, sketch: &Sketch) -> Response {
    Response::Analysed {
        query,
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
