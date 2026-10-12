//! The bytes that cross to the solver's Web Worker and back.
//!
//! A worker shares no memory with the page, so requests and responses are
//! encoded, posted and decoded. Plain Rust, so it's tested natively; the
//! web [`lane`](crate::lane) moves the bytes.
//!
//! ```text
//! request = postcard(Posted)
//! reply   = postcard(Reply)
//! ```
//!
//! Each is posted as one `ArrayBuffer`, transferred, not copied, and
//! copied out on the other side only if it's within `MAX_MESSAGE_BYTES`.
//! A [`Posted`] is a [`Request`] whose drag step may leave out the
//! session's sketch, which the worker has from the session's first step,
//! and whose design's parameters cross as each one's name and text, which
//! the worker resolves again ([`Params::evaluate`]). A
//! [`Reply`] is a [`Response`], or [`Reply::Unmoved`] for a drag step that
//! didn't converge, which the page must still hear of to post the next
//! request. A drag step's solution crosses as its points' places and its
//! circles' radii alone, the rest being the session's sketch.
//!
//! Both sides check what they get. The worker, a request
//! ([`decode_request`]): its parameters are at most [`MAX_PARAMS`], each
//! name at most [`MAX_NAME_LEN`] bytes and each text at most [`MAX_LEN`],
//! its sketch passes [`Sketch::check`]
//! against [`MAX_COORD`](varde_document::MAX_COORD) and the units and parameters it carries, and what the edit or the drag
//! step names is bounded in count, every place, radius and label finite
//! and within [`MAX_COORD`](varde_document::MAX_COORD), every value finite and its text within
//! [`MAX_LEN`], every new spline's counts and knots
//! as they can be ([`Spline::fits`](varde_sketch::Spline::fits)). The
//! page, a reply ([`decode_reply`]) against the request it answers: it
//! must answer that request, or it's refused, as an undecodable one is. A
//! solved sketch must pass [`Sketch::check`]; the ids an analysis or a
//! rejection names must name items of the sketch sent, or of the one the
//! edit makes from it, of the kind they're named for, and an analysis must
//! be of the revision sent (a rejection's driving
//! dimensions among those it involves); a drag step's
//! solution must have a place for each point and a radius for each circle.
//! A reply failing those answers its request with [`Response::Failed`].
//! Malformed bytes are refused, never a panic.

use std::fmt;
use std::sync::Arc;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use varde_document::{DecodeError, LengthUnit, Revision, codec};
use varde_expr::{MAX_LEN, MAX_NAME_LEN, MAX_PARAMS, Params};
use varde_sketch::{
    Analysis, BoundsError, Constraint, Curve, Id, Kind, Measure, OffsetPair, Rejected, Sketch,
    SketchEdit, SketchError, check_targets,
};

use crate::{Job, MAX, Request, Response, Tag, design};
use varde_document::Design;

/// The most bytes a request or a reply may have. A sketch at its limits
/// is far smaller; the bound keeps a broken message from being copied
/// without end.
#[cfg(target_arch = "wasm32")]
pub const MAX_MESSAGE_BYTES: usize = 1 << 28;

/// A [`Request`] as it crosses to the worker: `S` is the sketch, `E` the
/// edit, and `P` the design's parameters, borrowed to encode and owned
/// once decoded: each one's name and text as it crosses, resolved once
/// checked.
#[derive(Debug, Serialize, Deserialize)]
pub enum Posted<S, E, P> {
    Propose {
        base: Revision,
        sketch: S,
        edit: E,
        units: LengthUnit,
        params: P,
    },
    /// `sketch` and `params` are left out once the worker has the session.
    Drag {
        session: u64,
        sketch: Option<(S, P)>,
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
        units: LengthUnit,
    },
    Analyse {
        query: Option<u64>,
        revision: Revision,
        sketch: S,
        units: LengthUnit,
        params: P,
    },
}

/// The design's parameters as they cross: each one's name and text.
type Sources = Vec<(String, String)>;

/// A request as the worker decodes it, its parameters resolved.
pub type Decoded = Posted<Sketch, SketchEdit, Params>;

impl Decoded {
    pub fn tag(&self) -> Tag {
        self.job().tag()
    }

    /// The work it asks for, as [`Request::job`] is a [`Request`]'s.
    pub fn job(&self) -> Job<'_> {
        match self {
            Posted::Propose {
                base,
                sketch,
                edit,
                units,
                params,
            } => Job::Propose {
                base: *base,
                sketch,
                edit,
                design: design(*units, params),
            },
            Posted::Drag {
                session,
                sketch,
                points,
                radii,
                units,
            } => Job::Drag {
                session: *session,
                start: (sketch.as_ref()).map(|(sketch, params)| (sketch, design(*units, params))),
                points,
                radii,
            },
            Posted::Analyse {
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
}

/// Encodes `request` to post to the worker, leaving out a drag step's
/// sketch unless `with_sketch`.
pub fn encode_request(request: &Request, with_sketch: bool) -> Vec<u8> {
    let posted: Posted<&Sketch, &SketchEdit, Vec<(&str, &str)>> = match request {
        Request::Propose {
            base,
            sketch,
            edit,
            units,
            params,
        } => Posted::Propose {
            base: *base,
            sketch,
            edit,
            units: *units,
            params: sources(params),
        },
        Request::Drag {
            session,
            sketch,
            points,
            radii,
            units,
            params,
        } => Posted::Drag {
            session: *session,
            sketch: with_sketch.then(|| (&**sketch, sources(params))),
            points: points.clone(),
            radii: radii.clone(),
            units: *units,
        },
        Request::Analyse {
            query,
            revision,
            sketch,
            units,
            params,
        } => Posted::Analyse {
            query: *query,
            revision: *revision,
            sketch,
            units: *units,
            params: sources(params),
        },
    };
    postcard::to_stdvec(&posted).expect("requests always serialize")
}

/// Each of `params`' names and texts, as they cross.
fn sources(params: &Params) -> Vec<(&str, &str)> {
    params.sources().collect()
}

/// Decodes a request, checking it (see the module docs) and refusing
/// bytes after its end. The page encodes requests from checked sketches,
/// so a request refused here is a bug: the worker throws, and the page
/// answers the request as it does for any worker that stops.
pub fn decode_request(bytes: &[u8]) -> Result<Decoded, Error> {
    let posted: Posted<Sketch, SketchEdit, Sources> =
        codec::from_postcard_exact(bytes).map_err(Error::Request)?;
    Ok(match posted {
        Posted::Propose {
            base,
            sketch,
            edit,
            units,
            params,
        } => {
            let params = resolve(&params, units)?;
            check_sketch(&sketch, &design(units, &params))?;
            edit.check_bounds(MAX).map_err(Error::Bounds)?;
            Posted::Propose {
                base,
                sketch,
                edit,
                units,
                params,
            }
        }
        Posted::Drag {
            session,
            sketch,
            points,
            radii,
            units,
        } => {
            let sketch = match sketch {
                Some((sketch, params)) => {
                    let params = resolve(&params, units)?;
                    check_sketch(&sketch, &design(units, &params))?;
                    Some((sketch, params))
                }
                None => None,
            };
            check_targets(&points, &radii, MAX).map_err(Error::Bounds)?;
            Posted::Drag {
                session,
                sketch,
                points,
                radii,
                units,
            }
        }
        Posted::Analyse {
            query,
            revision,
            sketch,
            units,
            params,
        } => {
            let params = resolve(&params, units)?;
            check_sketch(&sketch, &design(units, &params))?;
            Posted::Analyse {
                query,
                revision,
                sketch,
                units,
                params,
            }
        }
    })
}

/// The parameters `sources` name, resolved in `units`, once their counts
/// and lengths are checked.
fn resolve(sources: &Sources, units: LengthUnit) -> Result<Params, Error> {
    check_count("parameters", sources.len(), MAX_PARAMS)?;
    for (name, text) in sources {
        check_count("bytes of a parameter's name", name.len(), MAX_NAME_LEN)?;
        check_count("bytes of a parameter's expression", text.len(), MAX_LEN)?;
    }
    Ok(Params::evaluate(
        sources
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str())),
        units,
    ))
}

fn check_sketch(sketch: &Sketch, design: &Design) -> Result<(), Error> {
    sketch.check(design).map_err(Error::Sketch)
}

/// `count` of `what` (a plural, such as "points"), at most `limit`.
fn check_count(what: &'static str, count: usize, limit: usize) -> Result<(), Error> {
    if count > limit {
        return Err(Error::TooMany { what, count, limit });
    }
    Ok(())
}

/// A [`Response`] as it crosses back to the page: `S` is a sketch and `A`
/// an analysis, borrowed to encode and owned once decoded.
#[derive(Debug, Serialize, Deserialize)]
pub enum Reply<S, A> {
    Accepted {
        base: Revision,
        sketch: S,
        analysis: A,
    },
    Rejected {
        base: Revision,
        why: Rejected,
    },
    /// The solution's points' places and circles' radii, in the order
    /// the session's sketch has them.
    Dragged {
        session: u64,
        points: Vec<DVec2>,
        radii: Vec<f64>,
    },
    /// A drag step of `session` that didn't converge: nothing to show.
    Unmoved {
        session: u64,
    },
    Analysed {
        query: Option<u64>,
        revision: Revision,
        analysis: A,
    },
    Failed {
        tag: Tag,
        error: String,
    },
}

impl<S, A> Reply<S, A> {
    /// What the reply answers.
    fn tag(&self) -> Tag {
        match self {
            Reply::Accepted { base, .. } | Reply::Rejected { base, .. } => Tag::Propose(*base),
            Reply::Dragged { session, .. } | Reply::Unmoved { session } => Tag::Drag(*session),
            Reply::Analysed {
                revision, query, ..
            } => Tag::analysis(*revision, *query),
            Reply::Failed { tag, .. } => *tag,
        }
    }
}

/// Encodes the reply to the request `tag` names: `response`, or, for a
/// drag step that didn't converge, none. The mirror of [`decode_reply`].
pub fn encode_reply(tag: Tag, response: Option<&Response>) -> Vec<u8> {
    let reply: Reply<&Sketch, &Analysis> = match (tag, response) {
        (_, Some(response)) => match response {
            Response::Accepted {
                base,
                sketch,
                analysis,
            } => Reply::Accepted {
                base: *base,
                sketch,
                analysis,
            },
            Response::Rejected { base, why } => Reply::Rejected {
                base: *base,
                why: why.clone(),
            },
            Response::Dragged { session, solution } => Reply::Dragged {
                session: *session,
                points: solution.points.iter().map(|point| point.at).collect(),
                radii: solution.curves.iter().filter_map(radius).collect(),
            },
            Response::Analysed {
                query,
                revision,
                analysis,
            } => Reply::Analysed {
                query: *query,
                revision: *revision,
                analysis,
            },
            Response::Failed { tag, error } => Reply::Failed {
                tag: *tag,
                error: error.clone(),
            },
        },
        (Tag::Drag(session), None) => Reply::Unmoved { session },
        (Tag::Propose(_) | Tag::Analyse(_) | Tag::Query(_), None) => {
            unreachable!("only a drag step goes unanswered")
        }
    };
    postcard::to_stdvec(&reply).expect("replies always serialize")
}

/// The radius of a circle, the curves a drag solution gives radii for.
fn radius(entry: &varde_sketch::CurveEntry) -> Option<f64> {
    match entry.curve {
        Curve::Circle { radius, .. } => Some(radius),
        Curve::Line { .. } | Curve::Arc { .. } | Curve::Spline(_) => None,
    }
}

/// Decodes the worker's reply to `asked`, the request it had, refusing
/// bytes after its end: the response, or none for a drag step that didn't
/// converge. A reply that doesn't decode or answers another request is an
/// error; one whose contents fail their checks (see the module docs)
/// answers `asked` with [`Response::Failed`].
pub fn decode_reply(bytes: &[u8], asked: &Request) -> Result<Option<Response>, Error> {
    let reply: Reply<Sketch, Analysis> = codec::from_postcard_exact(bytes).map_err(Error::Reply)?;
    let tag = asked.tag();
    if reply.tag() != tag {
        return Err(Error::Answers(reply.tag()));
    }
    Ok(check_reply(reply, asked).unwrap_or_else(|error| {
        Some(Response::Failed {
            tag,
            error: error.to_string(),
        })
    }))
}

/// The response `reply` to `asked` makes, which answers it, if it passes
/// its checks.
fn check_reply(reply: Reply<Sketch, Analysis>, asked: &Request) -> Result<Option<Response>, Error> {
    Ok(Some(match (reply, asked) {
        (
            Reply::Accepted {
                base,
                sketch,
                analysis,
            },
            _,
        ) => {
            check_sketch(&sketch, &asked.design())?;
            check_analysis(&analysis, &sketch)?;
            Response::Accepted {
                base,
                sketch: Arc::new(sketch),
                analysis: Arc::new(analysis),
            }
        }
        (Reply::Rejected { base, why }, Request::Propose { sketch, edit, .. }) => {
            check_rejected(&why, sketch, edit, &asked.design())?;
            Response::Rejected { base, why }
        }
        (
            Reply::Dragged {
                session,
                points,
                radii,
            },
            Request::Drag { sketch, .. },
        ) => Response::Dragged {
            session,
            solution: Arc::new(solution(sketch, points, radii, &asked.design())?),
        },
        (Reply::Unmoved { .. }, _) => return Ok(None),
        (
            Reply::Analysed {
                query,
                revision,
                analysis,
            },
            Request::Analyse {
                revision: asked_revision,
                sketch,
                ..
            },
        ) => {
            // The tag names a query by its id alone: its revision must be
            // the one sent too.
            if revision != *asked_revision {
                return Err(Error::Revision(revision));
            }
            check_analysis(&analysis, sketch)?;
            Response::Analysed {
                query,
                revision,
                analysis: Arc::new(analysis),
            }
        }
        (Reply::Failed { tag, error }, _) => Response::Failed { tag, error },
        // The tags matched, so the kinds do.
        (Reply::Rejected { .. } | Reply::Dragged { .. } | Reply::Analysed { .. }, _) => {
            unreachable!("a reply of another kind than its request")
        }
    }))
}

/// Checks that `analysis` is of `sketch`: at most as many degrees of
/// freedom as it has variables (its points' coordinates, circles' radii
/// and points' parameters on splines, or where they're offset from one),
/// what's fixed its points and curves, what's sized its lines, circles and
/// arcs, and what's redundant its constraints and arcs.
fn check_analysis(analysis: &Analysis, sketch: &Sketch) -> Result<(), Error> {
    let circles = sketch.curves.iter().filter_map(radius).count();
    let from_spline = |pair: [Id; 2]| sketch.offset_pair(pair) == Some(OffsetPair::Spline);
    let offsets = sketch.dimensions.iter().filter(
        |entry| matches!(entry.dimension.measure, Measure::Offset(a, b) if from_spline([a, b])),
    );
    let on_splines = sketch
        .constraints
        .iter()
        .map(|entry| match entry.constraint {
            Constraint::PointOnCurve { curve, .. } => {
                usize::from(sketch.kind(curve) == Some(Kind::Spline))
            }
            Constraint::EqualOffset { a, b } => {
                [a, b].into_iter().filter(|&p| from_spline(p)).count()
            }
            _ => 0,
        })
        .try_fold(offsets.count(), usize::checked_add);
    let variables = sketch
        .points
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_add(circles))
        .and_then(|n| n.checked_add(on_splines?))
        .ok_or(Error::Items)?;
    let fixed = |id| matches!(sketch.kind(id), Some(kind) if kind != Kind::Constraint);
    if analysis.freedom > variables
        || !analysis.fixed.iter().all(|&id| fixed(id))
        || !(analysis.sized.iter())
            .all(|&id| matches!(sketch.kind(id), Some(Kind::Line | Kind::Circle | Kind::Arc)))
        || !check_involved(&analysis.redundant, sketch)
    {
        return Err(Error::Items);
    }
    Ok(())
}

/// Whether `involved` are constraints, arcs, fillets and chamfers and
/// dimensions of `sketch`, the items equations come from.
fn check_involved<'a>(involved: impl IntoIterator<Item = &'a Id>, sketch: &Sketch) -> bool {
    involved.into_iter().all(|&id| {
        matches!(
            sketch.kind(id),
            Some(Kind::Constraint | Kind::Arc | Kind::Dimension)
        ) || sketch.curve(id).is_some_and(|entry| entry.corner.is_some())
    })
}

/// Checks that what `why` names is of the sketch `edit` makes of
/// `sketch`, which is only made if it names anything, and that driving
/// dimensions it names are among those involved.
fn check_rejected(
    why: &Rejected,
    sketch: &Sketch,
    edit: &SketchEdit,
    design: &Design,
) -> Result<(), Error> {
    if let Rejected::Driving {
        dimensions,
        involved,
    } = why
        && (dimensions.is_empty() || !dimensions.is_subset(involved))
    {
        return Err(Error::Items);
    }
    let involved = why.involved();
    if involved.is_empty() {
        return Ok(());
    }
    let applied = edit.apply(sketch, design).map_err(|_| Error::Items)?;
    if !check_involved(involved, &applied) {
        return Err(Error::Items);
    }
    if let Rejected::Driving { dimensions, .. } = why
        && !dimensions
            .iter()
            .all(|&id| applied.kind(id) == Some(Kind::Dimension))
    {
        return Err(Error::Items);
    }
    Ok(())
}

/// `sketch` with its points at `points` and its circles' radii `radii`, in
/// order, if there's one for each and the result passes its checks in
/// `design`.
fn solution(
    sketch: &Sketch,
    points: Vec<DVec2>,
    radii: Vec<f64>,
    design: &Design,
) -> Result<Sketch, Error> {
    let mut solution = sketch.clone();
    let mut circles: Vec<_> = solution
        .curves
        .iter_mut()
        .filter_map(|entry| match &mut entry.curve {
            Curve::Circle { radius, .. } => Some(radius),
            Curve::Line { .. } | Curve::Arc { .. } | Curve::Spline(_) => None,
        })
        .collect();
    if points.len() != solution.points.len() || radii.len() != circles.len() {
        return Err(Error::Items);
    }
    for (radius, new) in circles.iter_mut().zip(radii) {
        **radius = new;
    }
    for (point, at) in solution.points.iter_mut().zip(points) {
        point.at = at;
    }
    check_sketch(&solution, design)?;
    Ok(solution)
}

/// Why bytes from the other side were refused, or what they hold is.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A request couldn't be decoded.
    Request(DecodeError),
    /// A reply couldn't be decoded.
    Reply(DecodeError),
    /// A reply answers the request this tag names, not the one asked.
    Answers(Tag),
    /// An analysis is of this revision, not the one sent.
    Revision(Revision),
    /// A sketch fails its check.
    Sketch(SketchError),
    /// More of something than a request may hold: `what` names it, as a
    /// plural ("points", "bytes of a parameter's name").
    TooMany {
        what: &'static str,
        count: usize,
        limit: usize,
    },
    /// An edit or a drag's targets out of bounds, past [`MAX_COORD`](varde_document::MAX_COORD)
    /// where it's a place or radius.
    Bounds(BoundsError),
    /// A reply names items the sketch doesn't have, or not as many as it
    /// has.
    Items,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Request(e) => write!(f, "couldn't decode the request: {e}"),
            Error::Reply(e) => write!(f, "couldn't decode the reply: {e}"),
            Error::Answers(tag) => write!(f, "a reply to another request: {tag:?}"),
            Error::Revision(revision) => {
                write!(f, "an answer of another revision: {revision:?}")
            }
            Error::Sketch(e) => e.fmt(f),
            Error::TooMany { what, count, limit } => {
                write!(f, "{count} {what}, over the limit of {limit}")
            }
            Error::Bounds(e) => e.fmt(f),
            Error::Items => f.write_str("a reply naming items the sketch doesn't have"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Request(e) | Error::Reply(e) => Some(e),
            Error::Sketch(e) => Some(e),
            Error::Bounds(e) => Some(e),
            Error::Answers(_) | Error::Revision(_) | Error::TooMany { .. } | Error::Items => None,
        }
    }
}

#[cfg(test)]
mod tests;
