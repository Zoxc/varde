//! The lane's protocol: the [`Request`]s the app sends, the [`Response`]s
//! it gets back, a client's [`Query`] and its [`Answer`], and the
//! [`Progress`] sent ahead of an answer.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use varde_document::{BodyId, FeatureId, Generation, Placement, Sketch, Snapshot};
use varde_kernel_compat::{Aabb, RenderLines, RenderMesh};
use varde_sketch::Id;

use crate::{
    Draft, Drafted, EdgeFacts, ExportedBody, FeatureFailure, Inspect, Inspected, PickTables,
    Picking, Sight,
};

/// Work for the regeneration side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Builds the mesh and the sketch lines of the committed document as
    /// of `generation`, with `draft` applied if there is one. Supersedes
    /// any earlier regeneration still waiting, never an export or a query.
    Regenerate {
        generation: Generation,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
        /// A sketch left out of the lines, the one being edited, which the
        /// viewport draws over everything instead.
        exclude: Option<FeatureId>,
        /// The Timeline rolled back to before this feature: it and those
        /// after it aren't evaluated at all ([`Document::before`](varde_document::Document::before)).
        until: Option<FeatureId>,
        /// A feature being set up and not committed yet, answered as if
        /// it were. Boxed: a feature's kind is large next to the rest.
        draft: Option<Box<Draft>>,
        /// The measure tool's picks, measured on the model answered
        /// (see [`Inspect`]). It rides on the regeneration, latest wins,
        /// as a draft does: the newest request carries the newest picks,
        /// so one it replaces only had an answer the panel would no
        /// longer show, and the picks are always measured on the very
        /// model the answer draws, whose tables say where they are.
        inspect: Option<Box<Inspect>>,
        /// How finely to draw each body for the view, or `None` for
        /// within a share of each solid's size: see [`Sight`]. Boxed, as
        /// it lists the bodies.
        sight: Option<Box<Sight>>,
    },
    /// Welds the visible bodies of the committed `document` for export
    /// (see [`export`](crate::export)), answered with [`Response::Exported`] tagged
    /// `export`, which the app chooses. Never replaced or dropped by a
    /// later request, unlike [`Request::Regenerate`]: exports wait in
    /// order, ahead of a regeneration waiting, see `src/newest.rs`.
    Export {
        export: u64,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
    },
    /// A client's question ([`Query`]) about the committed `document` as
    /// of `generation`, rolled back to before `until` as a
    /// [`Request::Regenerate`] is: answered with [`Response::Answered`]
    /// tagged `tag`, which the app chooses. The history is evaluated or
    /// found in the cache, as for a regeneration of the same document,
    /// but nothing is drawn. Queued in order beside exports, never
    /// replaced or dropped, as they are (see `src/newest.rs`): a client
    /// (the MCP queries) waits on each.
    Query {
        tag: u64,
        generation: Generation,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
        until: Option<FeatureId>,
        query: Query,
    },
}

/// What a [`Request::Query`] asks of the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Query {
    /// What each edge of the body is ([`EdgeFacts`]), answered with
    /// [`Answer::Topology`]. The answer's edges pair with the body's
    /// part's first edges in the [`Picking`] of a model of the same
    /// generation and `until`, see [`EdgeFacts`].
    Topology(BodyId),
    /// Measures the picks as the user's measure does, answered with
    /// [`Answer::Measured`], with no place in any tables ([`Probed::at`](crate::Probed::at)
    /// `None`): the regeneration's own inspect is the user's, and
    /// neither may replace the other.
    Measure(Box<Inspect>),
}

/// What a [`Response::Answered`] answers, of the [`Query`] asked.
#[derive(Debug, Clone)]
pub enum Answer {
    /// Of [`Query::Topology`] for `body`: the facts of each of the
    /// body's edges, in its topology's order, or why there are none (the
    /// body has no solid there), in words to follow a colon.
    Topology {
        body: BodyId,
        result: Result<Vec<EdgeFacts>, String>,
    },
    /// Of [`Query::Measure`]: the picks measured (each found or why not,
    /// as for the user's measure), or why nothing was (the work failed),
    /// in words to follow a colon. Checked by [`Inspected::checked`] with no scene.
    Measured(Result<Box<Inspected>, String>),
}

/// How far a [`Request::Regenerate`] has got, sent ahead of its answer
/// as [`Response::Progress`] at the start of each of its steps: each
/// feature of the history in turn, then drawing the model. A draft that
/// fails has the history gone over again without it, counted from the
/// start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// The step started, counted from 0: as many steps are done.
    pub step: u32,
    /// How many steps there are, more than `step`.
    pub steps: u32,
    pub stage: Stage,
}

/// What a [`Progress`] step works on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// The feature of this name.
    Feature(String),
    /// The model: tessellating the bodies, flattening the sketches' lines
    /// and solving the sketches.
    Drawing,
}

impl Progress {
    /// Starting the feature at `index` of `count` in the history, of
    /// this `name`.
    pub(crate) fn feature(index: usize, count: usize, name: &str) -> Self {
        Self {
            step: saturating_u32(index),
            steps: Self::steps(count),
            stage: Stage::Feature(name.to_owned()),
        }
    }

    /// Starting to draw the model of a history of `count` features.
    pub(crate) fn drawing(count: usize) -> Self {
        let steps = Self::steps(count);
        Self {
            step: steps - 1,
            steps,
            stage: Stage::Drawing,
        }
    }

    /// The steps of a history of `count` features, drawing the model
    /// last.
    fn steps(count: usize) -> u32 {
        saturating_u32(count).saturating_add(1)
    }
}

pub(crate) fn saturating_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// What a [`Request::Regenerate`] asks for, which its answer echoes
/// back and so is of: a generation of the document, the sketch left out
/// of the lines, the feature rolled back to before, the revisions of the
/// draft applied and the measure taken, if any, and of the [`Sight`] it's
/// drawn by, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asked {
    pub generation: Generation,
    /// The sketch left out of the lines: a request can ask again for the
    /// same generation with another one.
    pub exclude: Option<FeatureId>,
    /// The feature the model is rolled back to before.
    pub until: Option<FeatureId>,
    /// The revision of the draft, if there is one.
    pub draft: Option<u64>,
    /// The revision of the measure, if there is one.
    pub inspect: Option<u64>,
    /// The revision of the [`Sight`], if there is one.
    pub sight: Option<u32>,
}

impl Response {
    /// What the model answered was asked for: `None` for an export's, a
    /// [`Response::Answered`] (whose generation is its own, not a
    /// model's) or a [`Response::Progress`].
    pub fn asked(&self) -> Option<Asked> {
        match self {
            Response::Regenerated(model) => Some(model.asked),
            Response::Failed { asked, .. } => Some(*asked),
            Response::Exported { .. } | Response::Answered { .. } | Response::Progress(_) => None,
        }
    }
}

impl Request {
    /// What a [`Request::Regenerate`] asks for: `None` for an export or a
    /// [`Request::Query`], which are queued in order.
    pub fn asked(&self) -> Option<Asked> {
        match self {
            Request::Regenerate {
                generation,
                exclude,
                until,
                draft,
                inspect,
                sight,
                ..
            } => Some(Asked {
                generation: *generation,
                exclude: *exclude,
                until: *until,
                draft: draft.as_ref().map(|draft| draft.revision),
                inspect: inspect.as_ref().map(|inspect| inspect.revision),
                sight: sight.as_ref().map(|sight| sight.revision),
            }),
            Request::Export { .. } | Request::Query { .. } => None,
        }
    }

    /// The answer to this request should handling it fail with `error`,
    /// e.g. by a panic: every request has one. Keeps only what the
    /// answer needs, not the document.
    pub fn failure(&self) -> impl FnOnce(String) -> Response + Send + use<> {
        /// What the answer needs of the request.
        enum Of {
            Regenerate(Asked),
            Export(u64),
            /// A query, with the body if it asks for a topology.
            Query {
                tag: u64,
                generation: Generation,
                until: Option<FeatureId>,
                topology: Option<BodyId>,
            },
        }
        let of = match self {
            Request::Regenerate { .. } => {
                Of::Regenerate(self.asked().expect("a regeneration asks"))
            }
            Request::Export { export, .. } => Of::Export(*export),
            Request::Query {
                tag,
                generation,
                until,
                query,
                ..
            } => Of::Query {
                tag: *tag,
                generation: *generation,
                until: *until,
                topology: match query {
                    Query::Topology(body) => Some(*body),
                    Query::Measure(_) => None,
                },
            },
        };
        move |why| match of {
            Of::Regenerate(asked) => Response::Failed { asked, error: why },
            Of::Export(export) => Response::Exported {
                export,
                result: Err(why),
            },
            Of::Query {
                tag,
                generation,
                until,
                topology,
            } => Response::Answered {
                tag,
                generation,
                until,
                answer: match topology {
                    Some(body) => Answer::Topology {
                        body,
                        result: Err(why),
                    },
                    None => Answer::Measured(Err(why)),
                },
            },
        }
    }
}

/// A [`Response::Regenerated`]: the model a regeneration made.
#[derive(Debug, Clone)]
pub struct Regenerated {
    /// What the request asked for: `sketches` leaves out its
    /// `exclude`, and the mesh may be drawn otherwise than by its
    /// [`Sight`], if too large by it. Its draft and inspect revisions
    /// are `draft`'s and `inspected`'s.
    pub asked: Asked,
    /// How the request's draft went, if it had one. Boxed, as the
    /// struct is large enough.
    pub draft: Option<Box<Drafted>>,
    pub mesh: Arc<RenderMesh>,
    /// The body of each of `mesh`'s parts, and its faces' and edges'
    /// tables, see [`Picking`].
    pub picking: Arc<Picking>,
    /// The hierarchies the viewport picks `mesh` with, built here so
    /// the UI thread doesn't, see [`PickTables`]. `mesh`'s
    /// [`RenderMesh::upload`] is built too.
    pub tables: Arc<PickTables>,
    /// The visible sketches' curves, see [`flatten_sketches`](crate::flatten_sketches).
    pub sketches: Arc<RenderLines>,
    /// The sketches that don't solve, see [`unsolved`](crate::unsolved), in the
    /// document's order.
    pub unsolved: Vec<FeatureId>,
    /// The features that failed, why and where, in the document's
    /// order, see [`evaluate`](crate::evaluate); their geometry's faces are this
    /// model's.
    pub failed: Vec<FeatureFailure>,
    /// Each join, cut or intersect that got as far as its tool, with
    /// the bodies it touches, see [`Evaluation::touched`](crate::Evaluation::touched): with a
    /// draft that goes, as the document with it applied found.
    pub touched: Vec<(FeatureId, Vec<BodyId>)>,
    /// Each body a join merged into another and the body holding it
    /// now, see [`Evaluation::merged`](crate::Evaluation::merged): a consumed body has no solid
    /// and isn't in `bodies`.
    pub merged: Vec<(BodyId, BodyId)>,
    /// Each sketch on a face that was placed, and where, in the
    /// document's order, see [`Evaluation::placements`](crate::Evaluation::placements): with a draft
    /// that goes, as the document with it applied placed them. A
    /// sketch on a face that isn't listed isn't placed (it failed).
    pub placements: Vec<(FeatureId, Placement)>,
    /// The box around each body that has a solid, shown or not, in
    /// the order they were made.
    pub bodies: Vec<(BodyId, Aabb)>,
    /// The answer to the request's measure, if it had one, measured
    /// on this model.
    pub inspected: Option<Box<Inspected>>,
    /// Each sketch whose links found other geometry than they hold,
    /// relinked and solved, in the document's order (see
    /// [`Evaluation::relinked`](crate::Evaluation::relinked)): for the app to fold into the change
    /// it's of. None with a draft: only the committed document's
    /// links are followed.
    pub relinked: Vec<(FeatureId, Arc<Sketch>)>,
    /// Each link that found nothing, its sketch, its id and why, in
    /// the document's order (see [`Evaluation::broken`](crate::Evaluation::broken)).
    pub broken: Vec<(FeatureId, Id, String)>,
}

/// The answer to a [`Request`].
#[derive(Debug, Clone)]
// On wasm32 the variants' pointers are smaller and the lint doesn't fire.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        clippy::large_enum_variant,
        reason = "a model is the usual answer, sent once and taken apart at once"
    )
)]
pub enum Response {
    Regenerated(Regenerated),
    /// The work `asked` failed, e.g. the kernel panicked. The request
    /// leaving out its `exclude` did: one of the same generation leaving
    /// out another sketch may not.
    Failed {
        asked: Asked,
        error: String,
    },
    /// Answers [`Request::Export`] tagged `export`: the visible bodies
    /// welded, in the order the history made them, or why they can't be
    /// (see [`ExportError`](crate::ExportError)), in words to follow a colon.
    Exported {
        export: u64,
        result: Result<Vec<ExportedBody>, String>,
    },
    /// Answers [`Request::Query`] tagged `tag`, of `generation` and
    /// `until` as asked.
    Answered {
        tag: u64,
        generation: Generation,
        until: Option<FeatureId>,
        answer: Answer,
    },
    /// How far the [`Request::Regenerate`] being worked on has got, sent
    /// while it's worked on, before its answer. Not an answer: every
    /// regeneration started is still answered.
    Progress(Progress),
}
