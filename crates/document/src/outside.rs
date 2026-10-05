//! What a sketch's links come from, outside the sketch: another sketch's
//! curve or point, or an edge, face or corner of the model, named as
//! features name them, so the reference outlives the model it was picked
//! in. The Project and Intersect tools pick them ([`OutsideRef`]); a
//! sketch feature keeps one per link ([`LinkSource`], beside the
//! [`Link`](varde_sketch::Link) in its sketch holding the geometry made
//! from it), which regenerating finds again on the model as the features
//! before the sketch leave it.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_sketch::LinkKind;

use crate::{EdgeError, EdgeRef, FaceRef, FeatureId, Id, PlaneError, PointRef};

/// Geometry outside a sketch that it projects or intersects: always of
/// features before the sketch. New kinds are appended: a kind's place in
/// the list is how the workers' bytes store it; files go by name.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OutsideRef {
    /// A curve or point of another sketch.
    Sketch { sketch: FeatureId, item: Id },
    /// An edge of a body.
    Edge(EdgeRef),
    /// A face of a body.
    Face(FaceRef),
    /// A vertex of a body, a [`PointRef::Corner`].
    Corner(PointRef),
}

impl OutsideRef {
    /// Whether a link of `kind` can take it: Project an edge, a corner
    /// or another sketch's item; Intersect a face or an edge.
    pub fn takes(&self, kind: LinkKind) -> bool {
        matches!(
            (kind, self),
            (_, OutsideRef::Edge(_))
                | (
                    LinkKind::Project,
                    OutsideRef::Sketch { .. } | OutsideRef::Corner(_)
                )
                | (LinkKind::Intersect, OutsideRef::Face(_))
        )
    }
}

/// What a sketch's link comes from: the link's id in the sketch, and
/// the reference.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LinkSource {
    pub link: Id,
    pub source: OutsideRef,
}

/// Why a sketch's links' sources fail [`Document::check`](crate::Document::check).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinkError {
    /// The sources aren't in increasing order of link, or two are of one
    /// link, or one is of no link of the sketch, or a link has none.
    Sources(Id),
    /// The link's kind can't take what it's from ([`OutsideRef::takes`]).
    Kind(Id),
    /// The link names a feature made at or after its sketch, or one
    /// that's no sketch: another sketch's item from the sketch itself, a
    /// later one, or a feature of another kind.
    Later(Id),
    /// The link's edge is wrong, see [`EdgeError`].
    Edge(Id, EdgeError),
    /// The link's face is wrong, see [`PlaneError`].
    Face(Id, PlaneError),
    /// The link's corner isn't a corner, or its keys aren't sorted and
    /// different, or its point isn't finite within the limit.
    Corner(Id),
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Sources(link) => {
                write!(f, "sketch link {link} has no source, or more than one")
            }
            LinkError::Kind(link) => {
                write!(f, "sketch link {link} can't take what it comes from")
            }
            LinkError::Later(link) => write!(
                f,
                "sketch link {link} comes from what's made at or after its sketch"
            ),
            LinkError::Edge(link, why) => write!(f, "sketch link {link}'s edge: {why}"),
            LinkError::Face(link, why) => write!(f, "sketch link {link}'s face: {why}"),
            LinkError::Corner(link) => write!(f, "sketch link {link}'s corner is wrong"),
        }
    }
}

impl std::error::Error for LinkError {}
