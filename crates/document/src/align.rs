//! The align feature: one body moved rigidly, in place, so a point and
//! directions picked on it meet a point and directions picked on other
//! bodies or the origin, and the points and directions it names.

use std::fmt;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_expr::Value;
use varde_kernel::mesh::FaceKey;

use crate::motion::Referred;
use crate::{Axis3, AxisRef, BodyId, Design, EdgeError, EdgeRef, FaceRef, FeatureId, Move};

/// An align: `body` moved so `from`, picked on it, meets `to`, picked on
/// bodies features before it make (not `body`) or the origin. The point
/// pair fixes where it goes; a primary direction pair, its tilt (the
/// smallest turn taking one onto the other); a secondary pair, the turn
/// about the primary. The body keeps its id and its faces their names,
/// as a move's do. Its motion is worked out from the references at every
/// regeneration, so the body follows when either side is edited before
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Align {
    /// The body moved, made by a feature before it.
    pub body: BodyId,
    /// On `body`: no origin reference.
    pub from: AlignRefs,
    /// On bodies other than `body` made before it, or the origin's.
    /// Shaped as `from`: a primary on both or neither, a secondary
    /// likewise.
    pub to: AlignRefs,
    /// Against the default way the primaries meet: opposed where each is
    /// a flat face's normal or a round edge's axis (a rim's: so faces
    /// meet face to face and a pin goes into a hole), the same way
    /// otherwise. Only with primaries.
    pub flip: bool,
    /// A gap along the target's primary direction as it's found (not as
    /// flipped): a length within [`MAX_COORD`](crate::MAX_COORD) of zero
    /// ([`Move::offset_ask`]). Only with primaries.
    pub offset: Option<Value>,
    /// A turn about the target's primary direction, right-handed, within a
    /// turn either way ([`Move::angle_ask`]), stored in radians. Only
    /// with primaries.
    pub turn: Option<Value>,
}

/// One side of an align: a point, and directions picked with it. A
/// secondary needs a primary.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AlignRefs {
    pub point: PointRef,
    pub primary: Option<DirRef>,
    pub secondary: Option<DirRef>,
}

/// A point on a body, as picked, or the origin. Found on its body as the
/// features before the one naming it leave it. New kinds are appended:
/// a kind's place in the list is how the workers' bytes store it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PointRef {
    Origin,
    /// A vertex where faces of the three keys meet (by name or alias),
    /// the nearest to `near` among several. The keys are sorted and
    /// different: a vertex is named by its faces, never by which end of
    /// an edge it is.
    Corner {
        body: BodyId,
        faces: [FaceKey; 3],
        /// Finite and within [`MAX_COORD`](crate::MAX_COORD).
        near: DVec3,
    },
    /// The middle of a straight edge: the mean of its ends.
    Middle(EdgeRef),
    /// The centre of a round edge (a hole's or a boss's rim, whole or an
    /// arc), from its conics.
    Centre(EdgeRef),
}

/// A direction on a body, as picked, or an origin axis. Signs follow the
/// faces' names: a flat face's normal points out of the body; a round
/// face's axis runs as its form's (an extrude's walls along the extrude);
/// a straight edge runs with the face of its first key on its left seen
/// from outside ([`EdgeRef`]); a round edge's axis is the outward normal
/// of a flat face beside it, else the axis of a round face beside it.
/// New kinds are appended, as [`PointRef`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DirRef {
    Origin(Axis3),
    /// A flat face's outward normal.
    Normal(FaceRef),
    /// A straight edge's direction, a round edge's axis or a round face's
    /// axis; an origin axis as [`DirRef::Origin`].
    Axis(AxisRef),
}

impl PointRef {
    /// The body it's on, if it's on one.
    pub fn body(&self) -> Option<BodyId> {
        match self {
            PointRef::Origin => None,
            PointRef::Corner { body, .. } => Some(*body),
            PointRef::Middle(edge) | PointRef::Centre(edge) => Some(edge.body),
        }
    }

    /// Checks what needs only the reference: a corner's keys sorted and
    /// different, an edge's as [`EdgeRef::check_own`], its point finite
    /// and within [`MAX_COORD`](crate::MAX_COORD).
    pub fn check_own(&self) -> Result<(), AlignError> {
        match self {
            PointRef::Origin => Ok(()),
            PointRef::Corner { faces, near, .. } => {
                let [a, b, c] = faces;
                if !(a < b && b < c) {
                    return Err(AlignError::Corner);
                }
                check_near(*near)
            }
            PointRef::Middle(edge) | PointRef::Centre(edge) => {
                edge.check_own().map_err(AlignError::Edge)
            }
        }
    }

    /// The features its keys name as its faces' makers.
    fn makers(&self) -> Vec<FeatureId> {
        match self {
            PointRef::Origin => Vec::new(),
            PointRef::Corner { faces, .. } => faces.map(|key| FeatureId(key.feature)).to_vec(),
            PointRef::Middle(edge) | PointRef::Centre(edge) => edge.makers().to_vec(),
        }
    }
}

impl DirRef {
    /// The edge or face it names on a body, if it names one.
    pub(crate) fn refers(&self) -> Option<Referred<'_>> {
        match self {
            DirRef::Origin(_) => None,
            DirRef::Normal(face) => Some(Referred::Face(face)),
            DirRef::Axis(axis) => axis.refers(),
        }
    }

    /// The body it's on, if it's on one.
    pub fn body(&self) -> Option<BodyId> {
        self.refers().map(|referred| referred.body())
    }

    /// Checks what needs only the reference, as [`PointRef::check_own`].
    pub fn check_own(&self) -> Result<(), AlignError> {
        match self.refers() {
            Some(Referred::Edge(edge)) => edge.check_own().map_err(AlignError::Edge),
            Some(Referred::Face(face)) => {
                (face.check_own()).map_err(|_| AlignError::Near(face.near))
            }
            None => Ok(()),
        }
    }
}

/// Checks a reference's point: finite and within [`MAX_COORD`](crate::MAX_COORD).
fn check_near(near: DVec3) -> Result<(), AlignError> {
    if crate::in_bounds(near) {
        Ok(())
    } else {
        Err(AlignError::Near(near))
    }
}

/// What a reference of an align names on a body: the body and the
/// features its keys name as its faces' makers.
pub(crate) struct Named {
    pub(crate) body: BodyId,
    pub(crate) makers: Vec<FeatureId>,
}

impl AlignRefs {
    /// Its directions, primary first.
    pub fn directions(&self) -> impl Iterator<Item = &DirRef> {
        self.primary.iter().chain(&self.secondary)
    }

    /// Checks each reference's own parts, and that a secondary comes
    /// with a primary.
    fn check_own(&self) -> Result<(), AlignError> {
        if self.secondary.is_some() && self.primary.is_none() {
            return Err(AlignError::Unpaired);
        }
        self.point.check_own()?;
        for direction in self.directions() {
            direction.check_own()?;
        }
        Ok(())
    }

    /// What each of its references names on a body, if it names one.
    pub(crate) fn named(&self) -> Vec<Named> {
        let point = self.point.body().map(|body| Named {
            body,
            makers: self.point.makers(),
        });
        let directions = self.directions().filter_map(|direction| {
            let referred = direction.refers()?;
            Some(Named {
                body: referred.body(),
                makers: referred.makers(),
            })
        });
        point.into_iter().chain(directions).collect()
    }

    /// Whether any of its references is the origin's.
    fn has_origin(&self) -> bool {
        self.point == PointRef::Origin
            || (self.directions()).any(|direction| direction.refers().is_none())
    }
}

impl Align {
    /// Checks what needs only the align and `design`: its sides shaped
    /// alike (a primary on both or neither, a secondary likewise, a
    /// secondary only with a primary), the flip, offset and turn only
    /// with primaries, the values, no origin reference on the moved side,
    /// and each reference's own parts. What the body and references name
    /// is [`Document::check`](crate::Document::check)'s.
    pub fn check_own(&self, design: &Design) -> Result<(), AlignError> {
        self.from.check_own()?;
        self.to.check_own()?;
        let pair = |a: Option<&DirRef>, b: Option<&DirRef>| a.is_some() == b.is_some();
        if !pair(self.from.primary.as_ref(), self.to.primary.as_ref())
            || !pair(self.from.secondary.as_ref(), self.to.secondary.as_ref())
        {
            return Err(AlignError::Unpaired);
        }
        let primaries = self.from.primary.is_some();
        if !primaries && (self.flip || self.offset.is_some() || self.turn.is_some()) {
            return Err(AlignError::Options);
        }
        if let Some(offset) = &self.offset {
            (offset.check(&Move::offset_ask(design))).map_err(|_| AlignError::Offset)?;
        }
        if let Some(turn) = &self.turn {
            (turn.check(&Move::angle_ask(design))).map_err(|_| AlignError::Angle)?;
        }
        if self.from.has_origin() {
            return Err(AlignError::FromOrigin);
        }
        Ok(())
    }
}

/// What's wrong with an align, see
/// [`CheckError::Align`](crate::CheckError::Align).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlignError {
    /// It moves this body, which isn't there or which no feature before
    /// it makes.
    Body(BodyId),
    /// Its sides' directions don't pair up: a primary or a secondary on
    /// one side only, or a secondary without a primary.
    Unpaired,
    /// It flips, offsets or turns without primary directions to do it
    /// along.
    Options,
    /// Its offset's expression doesn't give its value, or the value isn't
    /// a length [`Move::offset_ask`] takes.
    Offset,
    /// Its turn's expression doesn't give its value, or the value isn't
    /// an angle [`Move::angle_ask`] takes.
    Angle,
    /// A corner's keys aren't sorted, or two are the same.
    Corner,
    /// An edge fails its own check ([`EdgeRef::check_own`]).
    Edge(EdgeError),
    /// A corner's or face's point isn't finite, or is further from zero
    /// than [`MAX_COORD`](crate::MAX_COORD).
    Near(DVec3),
    /// A reference on the moved side is the origin's.
    FromOrigin,
    /// A reference on the moved side is on this body, not the one moved.
    FromBody(BodyId),
    /// A reference on the target side is on the body moved.
    OnMoved,
    /// A reference on the target side is on this body, which is made by
    /// the feature or one after it, or isn't there and has an id a body
    /// made later could take.
    RefBody(BodyId),
    /// A key of a reference's faces names this feature, which is the
    /// feature itself or comes after it, or isn't there and has an id a
    /// feature made later could take.
    RefMaker(FeatureId),
}

impl fmt::Display for AlignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlignError::Body(body) => write!(
                f,
                "moves body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            AlignError::Unpaired => f.write_str("its directions don't pair up"),
            AlignError::Options => {
                f.write_str("it flips, offsets or turns without directions to do it along")
            }
            AlignError::Offset => f.write_str("its offset's expression doesn't give its value"),
            AlignError::Angle => f.write_str("its turn's expression doesn't give its value"),
            AlignError::Corner => f.write_str("a corner's faces are out of order or repeated"),
            AlignError::Edge(why) => why.fmt(f),
            AlignError::Near(at) => write!(f, "a reference's point {at} is out of bounds"),
            AlignError::FromOrigin => f.write_str("the moved side names the origin"),
            AlignError::FromBody(body) => write!(
                f,
                "the moved side names body {}, not the body moved",
                body.0
            ),
            AlignError::OnMoved => f.write_str("the target side names the body moved"),
            AlignError::RefBody(body) => write!(
                f,
                "a reference is on body {}, which isn't made before it",
                body.0
            ),
            AlignError::RefMaker(feature) => write!(
                f,
                "a reference is on a face made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl std::error::Error for AlignError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AlignError::Edge(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
