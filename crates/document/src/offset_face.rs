//! The offset face feature: faces of a body already made moved along
//! their normals by a distance, the faces around them extended or
//! trimmed to meet them.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::plane::check_face_list_own;
use crate::{BodyId, Design, Extent, FaceListError, FaceRef, FaceSetError};

/// The most faces an offset face may move.
pub const MAX_OFFSET_FACES: usize = 256;

/// An offset face: the faces `faces`, all of one body, moved along their
/// normals by `distance`, out of the body (growing it) or with `inward`
/// into it, grown first across tangent-continuous faces when `tangent`
/// is on. The body keeps its id and every face its name, so later
/// references to a moved face still find it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetFace {
    /// `1..=`[`MAX_OFFSET_FACES`] faces, all on one body a feature
    /// before it makes, in [`FaceRef::order`] without repeats. Each is
    /// found on the body as the features before it leave it.
    pub faces: Vec<FaceRef>,
    /// A length as an extrude's distance ([`OffsetFace::distance_ask`]):
    /// above zero, the side given by `inward`, as an extrude's flip, so
    /// a handle can drag it through zero.
    pub distance: Value,
    /// Whether the faces move into the body (shrinking it) rather than
    /// out of it.
    pub inward: bool,
    /// Whether the faces take in the faces running on smoothly from them
    /// (tangent within 1°), as regeneration grows them.
    pub tangent: bool,
}

impl OffsetFace {
    /// What its distance is checked against in `design`: a length as an
    /// extrude's distance ([`Extent::ask`]).
    pub fn distance_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        Extent::ask(design)
    }

    /// The body its faces are on: the first face's (all are on one).
    /// `None` only for one with no faces, which the document refuses.
    pub fn body(&self) -> Option<BodyId> {
        self.faces.first().map(|face| face.body)
    }

    /// The bodies it names, which it depends on: its faces' body.
    pub fn bodies(&self) -> Vec<BodyId> {
        self.body().into_iter().collect()
    }

    /// The distance the faces move along their outward normals, in model
    /// units: negative inward.
    pub fn signed_distance(&self) -> f64 {
        if self.inward {
            -self.distance.value
        } else {
            self.distance.value
        }
    }

    /// Checks what needs only the offset face and `design`: the face
    /// count, order and body, each face's own parts, and its distance.
    /// What the body and the faces name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self, design: &Design) -> Result<(), OffsetFaceError> {
        check_face_list_own(&self.faces, 1..=MAX_OFFSET_FACES, None)
            .map_err(OffsetFaceError::Faces)?;
        (self.distance)
            .check(&OffsetFace::distance_ask(design))
            .map_err(|_| OffsetFaceError::Distance)
    }

    /// Its typed values and what each is checked against in `design`.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        vec![(&mut self.distance, OffsetFace::distance_ask(design))]
    }
}

/// What's wrong with an offset face, see
/// [`CheckError::OffsetFace`](crate::CheckError::OffsetFace).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OffsetFaceError {
    /// Its faces fail their own check ([`FaceListError`]).
    Faces(FaceListError),
    /// Its distance's expression doesn't give its value, or the value
    /// isn't a length [`OffsetFace::distance_ask`] takes.
    Distance,
    /// What its faces name ([`Document::check_face_set`](crate::Document::check_face_set)).
    Set(FaceSetError),
}

impl fmt::Display for OffsetFaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OffsetFaceError::Faces(why) => write!(f, "the faces it moves: {why}"),
            OffsetFaceError::Distance => f.write_str(
                "its distance's expression doesn't give its value, or it isn't a length it takes",
            ),
            OffsetFaceError::Set(why) => why.fmt(f),
        }
    }
}

impl std::error::Error for OffsetFaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OffsetFaceError::Faces(why) => Some(why),
            OffsetFaceError::Set(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
