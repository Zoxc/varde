//! The draft feature: faces of a body already made turned by an angle
//! about where they meet a neutral plane, so the part comes out of a
//! mould pulled along the plane's normal; the faces around them extended
//! or trimmed to meet them.

use std::f64::consts::FRAC_PI_2;
use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::plane::check_face_list_own;
use crate::{
    BodyId, Design, FaceListError, FaceRef, FaceSetError, FeatureId, PlaneError, PlaneRef,
};

/// The most faces a draft may turn.
pub const MAX_DRAFT_FACES: usize = 256;

/// A draft ("Draft N"; the type isn't `Draft`, which is regeneration's
/// preview): the faces `faces`, all of one body, each turned by `angle`
/// about its hinge, where it meets the neutral plane `neutral`, so that
/// its outward normal leans towards the pull: the plane's normal (a
/// face's outward normal, an origin plane's axis), reversed by `flip`.
/// The part then narrows along the pull. Grown first across
/// tangent-continuous faces when `tangent` is on. The body keeps its id
/// and every face its name, so later references to a drafted face still
/// find it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceDraft {
    /// `1..=`[`MAX_DRAFT_FACES`] faces, all on one body a feature before
    /// it makes, in [`FaceRef::order`] without repeats. Each is found on
    /// the body as the features before it leave it.
    pub faces: Vec<FaceRef>,
    /// The neutral plane: an origin plane, or a flat face of a body a
    /// feature before it makes (its own body's or another's), found as
    /// the features before it leave that body.
    pub neutral: PlaneRef,
    /// The angle each face turns by ([`FaceDraft::angle_ask`]): above 0
    /// and under 90°.
    pub angle: Value,
    /// Whether the pull runs against the neutral plane's normal.
    pub flip: bool,
    /// Whether the faces take in the faces running on smoothly from them
    /// (tangent within 1°), as regeneration grows them.
    pub tangent: bool,
}

impl FaceDraft {
    /// What its angle is checked against in `design`: above zero and
    /// under a right angle, bare numbers in degrees.
    pub fn angle_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        Ask::angle(design.units, FRAC_PI_2)
            .positive()
            .under_max()
            .with_params(design.params)
    }

    /// The body its faces are on: the first face's (all are on one).
    /// `None` only for one with no faces, which the document refuses.
    pub fn body(&self) -> Option<BodyId> {
        self.faces.first().map(|face| face.body)
    }

    /// The neutral plane's face, if it's a face.
    pub fn neutral_face(&self) -> Option<&FaceRef> {
        match &self.neutral {
            PlaneRef::Origin(_) => None,
            PlaneRef::Face(face) => Some(face),
        }
    }

    /// The bodies it names, which it depends on: its faces' body and the
    /// neutral plane's face's body. Sorted, without repeats.
    pub fn bodies(&self) -> Vec<BodyId> {
        let mut bodies: Vec<BodyId> = (self.body().into_iter())
            .chain(self.neutral_face().map(|face| face.body))
            .collect();
        bodies.sort_unstable();
        bodies.dedup();
        bodies
    }

    /// Checks what needs only the draft and `design`: the face count,
    /// order and body, each face's own parts, the neutral face's own
    /// parts, and its angle. What the bodies and the faces name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self, design: &Design) -> Result<(), FaceDraftError> {
        check_face_list_own(&self.faces, 1..=MAX_DRAFT_FACES, None)
            .map_err(FaceDraftError::Faces)?;
        if let Some(face) = self.neutral_face() {
            face.check_own().map_err(FaceDraftError::Neutral)?;
        }
        (self.angle)
            .check(&FaceDraft::angle_ask(design))
            .map_err(|_| FaceDraftError::Angle)
    }

    /// Its typed values and what each is checked against in `design`.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        vec![(&mut self.angle, FaceDraft::angle_ask(design))]
    }
}

/// What's wrong with a draft, see
/// [`CheckError::FaceDraft`](crate::CheckError::FaceDraft).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaceDraftError {
    /// Its faces fail their own check ([`FaceListError`]).
    Faces(FaceListError),
    /// Its neutral plane's face fails its own check.
    Neutral(PlaneError),
    /// Its angle's expression doesn't give its value, or the value isn't
    /// an angle [`FaceDraft::angle_ask`] takes.
    Angle,
    /// What its faces name ([`Document::check_face_set`](crate::Document::check_face_set)).
    Set(FaceSetError),
    /// Its neutral face's body, this one, isn't there or no feature
    /// before it makes it.
    NeutralBody(BodyId),
    /// Its neutral face's key names this feature, which is the draft
    /// itself or comes after it, or isn't there and has an id a feature
    /// made later could take.
    NeutralMaker(FeatureId),
}

impl fmt::Display for FaceDraftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaceDraftError::Faces(why) => write!(f, "the faces it drafts: {why}"),
            FaceDraftError::Neutral(why) => write!(f, "its neutral plane's face: {why}"),
            FaceDraftError::Angle => f.write_str(
                "its angle's expression doesn't give its value, or it isn't an angle it takes",
            ),
            FaceDraftError::Set(why) => why.fmt(f),
            FaceDraftError::NeutralBody(body) => write!(
                f,
                "its neutral plane is a face of body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            FaceDraftError::NeutralMaker(feature) => write!(
                f,
                "its neutral plane's face was made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl std::error::Error for FaceDraftError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FaceDraftError::Faces(why) => Some(why),
            FaceDraftError::Set(why) => Some(why),
            FaceDraftError::Neutral(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
