//! The references features share to what's on bodies: axes, points and
//! directions, and what each names on a body for the check.

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_kernel_compat::mesh::FaceKey;

use crate::{BodyId, EdgeError, EdgeRef, FaceRef, FeatureId, NearError, Plane};

/// What a reference names on a body: the body and the features its keys
/// name as its faces' makers, for [`Document::refers_before`](crate::Document::refers_before).
pub(crate) struct OnBody {
    pub(crate) body: BodyId,
    pub(crate) makers: Vec<FeatureId>,
}

/// A line in the world, for a move's turn: an origin axis, a straight
/// model edge (the line through it, directed as [`EdgeRef`] says) or a
/// round one (its circle's axis through its centre, turning the way the
/// edge runs), or a round face's axis (a cylinder's, cone's, torus's or
/// other surface of revolution's, directed as the face's form has it).
/// Edges and faces are found on their bodies as the features before the
/// one naming them leave them. New kinds are appended: a kind's place in
/// the list is how the workers' bytes store it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AxisRef {
    Origin(Axis3),
    Edge(EdgeRef),
    Face(FaceRef),
}

/// One of the world's axes through the origin, directed along its
/// positive side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Axis3 {
    X,
    Y,
    Z,
}

impl Axis3 {
    pub const ALL: [Axis3; 3] = [Axis3::X, Axis3::Y, Axis3::Z];

    /// Its unit direction.
    pub fn direction(self) -> DVec3 {
        match self {
            Axis3::X => DVec3::X,
            Axis3::Y => DVec3::Y,
            Axis3::Z => DVec3::Z,
        }
    }

    /// Its place in [`Axis3::ALL`], for arrays kept per axis.
    pub fn index(self) -> usize {
        match self {
            Axis3::X => 0,
            Axis3::Y => 1,
            Axis3::Z => 2,
        }
    }

    /// Two world axes spanning the plane square to it, right-handed: the
    /// first crossed with the second is [`Axis3::direction`].
    pub fn plane_axes(self) -> (DVec3, DVec3) {
        match self {
            Axis3::X => (DVec3::Y, DVec3::Z),
            Axis3::Y => (DVec3::Z, DVec3::X),
            Axis3::Z => (DVec3::X, DVec3::Y),
        }
    }

    /// The axis as the user sees it: "X".
    pub fn name(self) -> &'static str {
        match self {
            Axis3::X => "X",
            Axis3::Y => "Y",
            Axis3::Z => "Z",
        }
    }
}

impl AxisRef {
    /// The edge or face it names on a body, if it names one.
    pub(crate) fn refers(&self) -> Option<Referred<'_>> {
        match self {
            AxisRef::Origin(_) => None,
            AxisRef::Edge(edge) => Some(Referred::Edge(edge)),
            AxisRef::Face(face) => Some(Referred::Face(face)),
        }
    }
}

impl Plane {
    /// The face it names on a body, if it names one.
    pub(crate) fn refers(&self) -> Option<Referred<'_>> {
        self.face().map(Referred::Face)
    }
}

/// An edge or a face a move's axis or a mirror's plane names.
pub(crate) enum Referred<'a> {
    Edge(&'a EdgeRef),
    Face(&'a FaceRef),
}

impl Referred<'_> {
    /// Checks what needs only the reference: an edge's
    /// [`EdgeRef::check_own`], a face's point in bounds.
    pub(crate) fn check_own(&self) -> Result<(), RefError> {
        match self {
            Referred::Edge(edge) => edge.check_own().map_err(RefError::Edge),
            Referred::Face(face) => face
                .check_own()
                .map_err(|NearError(near)| RefError::Near(near)),
        }
    }

    /// The body it's on.
    pub(crate) fn body(&self) -> BodyId {
        match self {
            Referred::Edge(edge) => edge.body,
            Referred::Face(face) => face.body,
        }
    }

    /// What it names on its body.
    pub(crate) fn on_body(&self) -> OnBody {
        match self {
            Referred::Edge(edge) => edge.on_body(),
            Referred::Face(face) => face.on_body(),
        }
    }
}

/// What's wrong with an axis's or plane's edge or face on its own, see
/// `Referred::check_own`.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum RefError {
    /// Its edge fails its own check ([`EdgeRef::check_own`]).
    #[error("its axis: {0}")]
    Edge(#[source] EdgeError),
    /// Its face's point isn't finite, or is further from zero than
    /// [`MAX_COORD`](crate::MAX_COORD).
    #[error("its face's point {0} is out of bounds")]
    Near(DVec3),
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
    pub fn check_own(&self) -> Result<(), PointRefError> {
        match self {
            PointRef::Origin => Ok(()),
            PointRef::Corner { faces, near, .. } => {
                let [a, b, c] = faces;
                if !(a < b && b < c) {
                    return Err(PointRefError::Corner);
                }
                check_near(*near)
            }
            PointRef::Middle(edge) | PointRef::Centre(edge) => {
                edge.check_own().map_err(PointRefError::Edge)
            }
        }
    }

    /// What it names on a body, if it's on one.
    pub(crate) fn on_body(&self) -> Option<OnBody> {
        match self {
            PointRef::Origin => None,
            PointRef::Corner { body, faces, .. } => Some(OnBody {
                body: *body,
                makers: faces.map(|key| FeatureId(key.feature)).to_vec(),
            }),
            PointRef::Middle(edge) | PointRef::Centre(edge) => Some(edge.on_body()),
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
    pub fn check_own(&self) -> Result<(), PointRefError> {
        let Some(referred) = self.refers() else {
            return Ok(());
        };
        referred.check_own().map_err(|why| match why {
            RefError::Edge(why) => PointRefError::Edge(why),
            RefError::Near(near) => PointRefError::Near(near),
        })
    }
}

/// Checks a reference's point: finite and within [`MAX_COORD`](crate::MAX_COORD).
fn check_near(near: DVec3) -> Result<(), PointRefError> {
    if crate::in_bounds(near) {
        Ok(())
    } else {
        Err(PointRefError::Near(near))
    }
}

/// What's wrong with a point's or a direction's reference on its own,
/// see [`PointRef::check_own`] and [`DirRef::check_own`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum PointRefError {
    /// A corner's keys aren't sorted, or two are the same.
    #[error("a corner's faces are out of order or repeated")]
    Corner,
    /// An edge fails its own check ([`EdgeRef::check_own`]).
    #[error("{0}")]
    Edge(#[source] EdgeError),
    /// A corner's or face's point isn't finite, or is further from zero
    /// than [`MAX_COORD`](crate::MAX_COORD).
    #[error("a reference's point {0} is out of bounds")]
    Near(DVec3),
}
