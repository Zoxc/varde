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
//! session's sketch, which the worker has from the session's first step. A
//! [`Reply`] is a [`Response`], or [`Reply::Unmoved`] for a drag step that
//! didn't converge, which the page must still hear of to post the next
//! request. A drag step's solution crosses as its points' places and its
//! circles' radii alone, the rest being the session's sketch.
//!
//! Both sides check what they get. The worker, a request
//! ([`decode_request`]): its sketch passes [`Sketch::check`] against
//! [`MAX_COORD`] and the units it carries, and what the edit or the drag
//! step names is bounded in count, every place, radius and label finite
//! and within [`MAX_COORD`], every value finite and its text within
//! [`MAX_LEN`](varde_expr::MAX_LEN), every new spline's counts and knots
//! as they can be ([`Spline::fits`](varde_sketch::Spline::fits)). The
//! page, a reply ([`decode_reply`]) against the request it answers: it
//! must answer that request, or it's refused, as an undecodable one is. A
//! solved sketch must pass [`Sketch::check`]; the ids an analysis or a
//! rejection names must name items of the sketch sent, or of the one the
//! edit makes from it, of the kind they're named for (a rejection's driving
//! dimensions among those it involves); a drag step's
//! solution must have a place for each point and a radius for each circle.
//! A reply failing those answers its request with [`Response::Failed`].
//! Malformed bytes are refused, never a panic.

use std::fmt;
use std::sync::Arc;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use varde_document::{DecodeError, LengthUnit, MAX_COORD, Revision, codec};
use varde_expr::Value;
use varde_sketch::{
    Analysis, Constraint, Curve, Id, Kind, MAX_CONSTRAINTS, MAX_CURVES, MAX_DIMENSIONS, MAX_POINTS,
    Measure, OffsetPair, Rejected, Setback, Sketch, SketchEdit, SketchError,
};

use crate::{MAX, Request, Response, Solver, Tag, design};

/// The most bytes a request or a reply may have. A sketch at its limits
/// is far smaller; the bound keeps a broken message from being copied
/// without end.
#[cfg(target_arch = "wasm32")]
pub const MAX_MESSAGE_BYTES: usize = 1 << 28;

/// A [`Request`] as it crosses to the worker: `S` is the sketch, and `E`
/// the edit, borrowed to encode and owned once decoded.
#[derive(Debug, Serialize, Deserialize)]
pub enum Posted<S, E> {
    Propose {
        base: Revision,
        sketch: S,
        edit: E,
        units: LengthUnit,
    },
    /// `sketch` is left out once the worker has the session.
    Drag {
        session: u64,
        sketch: Option<S>,
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
        units: LengthUnit,
    },
    Analyse {
        revision: Revision,
        sketch: S,
        units: LengthUnit,
    },
}

/// A request as the worker decodes it.
pub type Decoded = Posted<Sketch, SketchEdit>;

impl<S, E> Posted<S, E> {
    pub fn tag(&self) -> Tag {
        match self {
            Posted::Propose { base, .. } => Tag::Propose(*base),
            Posted::Drag { session, .. } => Tag::Drag(*session),
            Posted::Analyse { revision, .. } => Tag::Analyse(*revision),
        }
    }
}

impl Decoded {
    /// Does the work, as [`Solver::handle`] does for a [`Request`].
    pub fn answer(self, solver: &mut Solver) -> Option<Response> {
        match self {
            Posted::Propose {
                base,
                sketch,
                edit,
                units,
            } => Some(crate::propose(base, &sketch, &edit, units)),
            Posted::Drag {
                session,
                sketch,
                points,
                radii,
                units,
            } => solver.drag(session, sketch.as_ref(), points, radii, units),
            Posted::Analyse {
                revision, sketch, ..
            } => Some(crate::analyse(revision, &sketch)),
        }
    }
}

/// Encodes `request` to post to the worker, leaving out a drag step's
/// sketch unless `with_sketch`.
pub fn encode_request(request: &Request, with_sketch: bool) -> Vec<u8> {
    let posted: Posted<&Sketch, &SketchEdit> = match request {
        Request::Propose {
            base,
            sketch,
            edit,
            units,
        } => Posted::Propose {
            base: *base,
            sketch,
            edit,
            units: *units,
        },
        Request::Drag {
            session,
            sketch,
            points,
            radii,
            units,
        } => Posted::Drag {
            session: *session,
            sketch: with_sketch.then_some(&**sketch),
            points: points.clone(),
            radii: radii.clone(),
            units: *units,
        },
        Request::Analyse {
            revision,
            sketch,
            units,
        } => Posted::Analyse {
            revision: *revision,
            sketch,
            units: *units,
        },
    };
    postcard::to_stdvec(&posted).expect("requests always serialize")
}

/// Decodes a request, checking it (see the module docs) and refusing
/// bytes after its end. The page encodes requests from checked sketches,
/// so a request refused here is a bug: the worker throws, and the page
/// answers the request as it does for any worker that stops.
pub fn decode_request(bytes: &[u8]) -> Result<Decoded, Error> {
    let posted: Decoded = codec::from_postcard_exact(bytes).map_err(Error::Request)?;
    match &posted {
        Posted::Propose {
            sketch,
            edit,
            units,
            ..
        } => {
            check_sketch(sketch, *units)?;
            check_edit(edit)?;
        }
        Posted::Drag {
            sketch,
            points,
            radii,
            units,
            ..
        } => {
            if let Some(sketch) = sketch {
                check_sketch(sketch, *units)?;
            }
            check_targets(points, radii)?;
        }
        Posted::Analyse { sketch, units, .. } => check_sketch(sketch, *units)?,
    }
    Ok(posted)
}

fn check_sketch(sketch: &Sketch, units: LengthUnit) -> Result<(), Error> {
    sketch.check(&design(units)).map_err(Error::Sketch)
}

/// Checks the counts and the numbers of `edit`. Whether what it names is
/// there is applying it's to say.
fn check_edit(edit: &SketchEdit) -> Result<(), Error> {
    match edit {
        SketchEdit::Add(add) => {
            check_count(add.points.len(), MAX_POINTS)?;
            check_count(add.curves.len(), MAX_CURVES)?;
            check_count(add.constraints.len(), MAX_CONSTRAINTS)?;
            check_count(add.auto.len(), MAX_CONSTRAINTS)?;
            check_count(add.dimensions.len(), MAX_DIMENSIONS)?;
            add.points.iter().try_for_each(|&(_, at)| check_place(at))?;
            add.curves.iter().try_for_each(|new| match &new.curve {
                &Curve::Circle { radius, .. } => check_value(radius),
                Curve::Spline(spline) if !spline.fits() => {
                    Err(Error::Sketch(SketchError::Spline(new.id)))
                }
                Curve::Line { .. } | Curve::Arc { .. } | Curve::Spline(_) => Ok(()),
            })?;
            add.dimensions.iter().try_for_each(|dimension| {
                check_place(dimension.label)?;
                check_dimension_value(&dimension.value)
            })
        }
        SketchEdit::Delete(ids) => {
            // Checked, though the limits are far from overflowing.
            let items = MAX_POINTS
                .checked_add(MAX_CURVES)
                .and_then(|n| n.checked_add(MAX_CONSTRAINTS))
                .expect("the limits add up");
            check_count(ids.len(), items)
        }
        SketchEdit::Move { points, radii } => check_targets(points, radii),
        SketchEdit::SetConstruction { ids, .. } => check_count(ids.len(), MAX_CURVES),
        SketchEdit::SetDimension { value, .. } => check_dimension_value(value),
        SketchEdit::SetDriving { .. } => Ok(()),
        SketchEdit::MoveLabel { label, .. } => check_place(*label),
        SketchEdit::Trim { near, .. } => check_place(*near),
        SketchEdit::Extend { .. } => Ok(()),
        SketchEdit::Mirror { ids, .. } => {
            let items = MAX_POINTS
                .checked_add(MAX_CURVES)
                .expect("the limits add up");
            check_count(ids.len(), items)
        }
        SketchEdit::Offset {
            chain, distance, ..
        } => {
            check_count(chain.len(), MAX_CURVES)?;
            check_dimension_value(distance)
        }
        SketchEdit::Fillet { radius, .. } => check_dimension_value(radius),
        SketchEdit::Chamfer { setback, .. } => match setback {
            Setback::Equal(d) => check_dimension_value(d),
            Setback::Two(a, b) | Setback::Angle(a, b) => {
                check_dimension_value(a)?;
                check_dimension_value(b)
            }
        },
        SketchEdit::Convert { .. } => Ok(()),
        SketchEdit::AddHandles(points) => check_count(points.len(), MAX_POINTS),
        SketchEdit::InsertPoint { near, .. } => check_place(*near),
        SketchEdit::AddLink { .. } | SketchEdit::SetLinkProfiles { .. } => Ok(()),
        SketchEdit::Relink(found) => {
            check_count(found.len(), varde_sketch::MAX_LINKS)?;
            match (found.iter()).find(|(_, shape)| !shape.fits(MAX)) {
                Some(&(link, _)) => Err(Error::Sketch(SketchError::Link(link))),
                None => Ok(()),
            }
        }
    }
}

/// A dimension's value: its text no longer than an expression may be,
/// and its value a number. Whether they agree is the sketch's check's to
/// say, once the edit is applied.
fn check_dimension_value(value: &Value) -> Result<(), Error> {
    if value.text.len() > varde_expr::MAX_LEN {
        return Err(Error::TooMany {
            count: value.text.len(),
            limit: varde_expr::MAX_LEN,
        });
    }
    if !value.value.is_finite() {
        return Err(Error::Value(value.value));
    }
    Ok(())
}

/// Checks a drag's (or a move's) targets: no more than the points and
/// circles a sketch may have, every number finite and within the limit.
fn check_targets(points: &[(Id, DVec2)], radii: &[(Id, f64)]) -> Result<(), Error> {
    check_count(points.len(), MAX_POINTS)?;
    check_count(radii.len(), MAX_CURVES)?;
    points.iter().try_for_each(|&(_, at)| check_place(at))?;
    radii
        .iter()
        .try_for_each(|&(_, radius)| check_value(radius))
}

fn check_count(count: usize, limit: usize) -> Result<(), Error> {
    if count > limit {
        return Err(Error::TooMany { count, limit });
    }
    Ok(())
}

fn check_place(at: DVec2) -> Result<(), Error> {
    check_value(at.x)?;
    check_value(at.y)
}

/// A coordinate or radius: finite and within the limit (not a number
/// isn't).
fn check_value(value: f64) -> Result<(), Error> {
    if value.abs() <= MAX {
        Ok(())
    } else {
        Err(Error::Value(value))
    }
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
            Reply::Analysed { revision, .. } => Tag::Analyse(*revision),
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
            Response::Analysed { revision, analysis } => Reply::Analysed {
                revision: *revision,
                analysis,
            },
            Response::Failed { tag, error } => Reply::Failed {
                tag: *tag,
                error: error.clone(),
            },
        },
        (Tag::Drag(session), None) => Reply::Unmoved { session },
        (Tag::Propose(_) | Tag::Analyse(_), None) => {
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
            check_sketch(&sketch, asked.units())?;
            check_analysis(&analysis, &sketch)?;
            Response::Accepted {
                base,
                sketch: Arc::new(sketch),
                analysis: Arc::new(analysis),
            }
        }
        (
            Reply::Rejected { base, why },
            Request::Propose {
                sketch,
                edit,
                units,
                ..
            },
        ) => {
            check_rejected(&why, sketch, edit, *units)?;
            Response::Rejected { base, why }
        }
        (
            Reply::Dragged {
                session,
                points,
                radii,
            },
            Request::Drag { sketch, units, .. },
        ) => Response::Dragged {
            session,
            solution: Arc::new(solution(sketch, points, radii, *units)?),
        },
        (Reply::Unmoved { .. }, _) => return Ok(None),
        (Reply::Analysed { revision, analysis }, Request::Analyse { sketch, .. }) => {
            check_analysis(&analysis, sketch)?;
            Response::Analysed {
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
/// what's fixed its points and curves, and what's redundant its
/// constraints and arcs.
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
    units: LengthUnit,
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
    let applied = edit
        .apply(sketch, &design(units))
        .map_err(|_| Error::Items)?;
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
/// order, if there's one for each and the result passes its checks in a
/// design of `units`.
fn solution(
    sketch: &Sketch,
    points: Vec<DVec2>,
    radii: Vec<f64>,
    units: LengthUnit,
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
    check_sketch(&solution, units)?;
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
    /// A sketch fails its check.
    Sketch(SketchError),
    /// More of something than a sketch may have.
    TooMany { count: usize, limit: usize },
    /// A place or radius past [`MAX_COORD`], or not a number.
    Value(f64),
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
            Error::Sketch(e) => e.fmt(f),
            Error::TooMany { count, limit } => {
                write!(f, "{count} sketch items, over the limit of {limit}")
            }
            Error::Value(value) => write!(
                f,
                "a sketch value of {value}, outside the limit of {MAX_COORD}"
            ),
            Error::Items => f.write_str("a reply naming items the sketch doesn't have"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Request(e) | Error::Reply(e) => Some(e),
            Error::Sketch(e) => Some(e),
            Error::Answers(_) | Error::TooMany { .. } | Error::Value(_) | Error::Items => None,
        }
    }
}

#[cfg(test)]
mod tests;
