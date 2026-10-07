//! The offset face feature: faces of a body already made moved along
//! their normals by a distance, the faces around them extended or
//! trimmed to meet them.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::{BodyId, Design, Extent, FaceRef, FaceSetError, FeatureId, PlaneError};

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
        let count = self.faces.len();
        if count == 0 {
            return Err(OffsetFaceError::NoFaces);
        }
        if count > MAX_OFFSET_FACES {
            return Err(OffsetFaceError::Faces(count));
        }
        for face in &self.faces {
            face.check_own().map_err(OffsetFaceError::Face)?;
        }
        if !(self.faces.windows(2)).all(|pair| pair[0].order(&pair[1]).is_lt()) {
            return Err(OffsetFaceError::FaceOrder);
        }
        if (self.faces.windows(2)).any(|pair| pair[0].body != pair[1].body) {
            return Err(OffsetFaceError::Bodies);
        }
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
    /// It moves no face.
    NoFaces,
    /// It moves this many faces, over [`MAX_OFFSET_FACES`].
    Faces(usize),
    /// Its faces aren't in [`FaceRef::order`], or one is repeated.
    FaceOrder,
    /// A face fails its own check ([`FaceRef::check_own`]).
    Face(PlaneError),
    /// Its faces aren't all on one body.
    Bodies,
    /// Its distance's expression doesn't give its value, or the value
    /// isn't a length [`OffsetFace::distance_ask`] takes.
    Distance,
    /// Its faces' body, this one, isn't there or no feature before it
    /// makes it.
    Body(BodyId),
    /// A face's key names this feature, which is the offset face itself
    /// or comes after it, or isn't there and has an id a feature made
    /// later could take.
    RefMaker(FeatureId),
}

impl fmt::Display for OffsetFaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OffsetFaceError::NoFaces => f.write_str("it moves no face"),
            OffsetFaceError::Faces(count) => {
                write!(f, "moves {count} faces, more than {MAX_OFFSET_FACES}")
            }
            OffsetFaceError::FaceOrder => f.write_str("its faces are out of order or repeated"),
            OffsetFaceError::Face(why) => why.fmt(f),
            OffsetFaceError::Bodies => f.write_str("its faces aren't all on one body"),
            OffsetFaceError::Distance => f.write_str(
                "its distance's expression doesn't give its value, or it isn't a length it takes",
            ),
            OffsetFaceError::Body(body) => write!(
                f,
                "moves faces of body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            OffsetFaceError::RefMaker(feature) => write!(
                f,
                "a face it moves was made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl From<FaceSetError> for OffsetFaceError {
    fn from(why: FaceSetError) -> Self {
        match why {
            FaceSetError::Body(body) => OffsetFaceError::Body(body),
            FaceSetError::RefMaker(feature) => OffsetFaceError::RefMaker(feature),
        }
    }
}

impl std::error::Error for OffsetFaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OffsetFaceError::Face(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
