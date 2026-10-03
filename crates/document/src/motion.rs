//! The move and mirror features: bodies already made moved (turned about
//! an axis, then shifted) or mirrored in a plane, as steps of the history,
//! and the axes and planes they name.

use std::f64::consts::TAU;
use std::fmt;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::{
    BodyId, Design, EdgeError, EdgeRef, FaceRef, FeatureId, MAX_COORD, MAX_FEATURE_BODIES,
    OriginPlane,
};

/// A move: the bodies it moves, each turned about `turn`'s axis by its
/// angle (if it has one), then shifted by `offset`. Each body keeps its
/// id and its faces their names: later features find them where they
/// went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Move {
    /// `1..=`[`MAX_FEATURE_BODIES`] bodies features before it make,
    /// sorted without repeats.
    pub bodies: Vec<BodyId>,
    /// Along world X, Y and Z: lengths within [`MAX_COORD`] of zero
    /// ([`Move::offset_ask`]), zero or negative too.
    pub offset: [Value; 3],
    /// The axis turned about, as the features before the move leave its
    /// body, and the angle, right-handed about the axis's direction,
    /// within a turn either way ([`Move::angle_ask`]). Turned first, then
    /// shifted.
    pub turn: Option<(AxisRef, Value)>,
}

/// A mirror: the bodies it mirrors in `plane`, each keeping its id. With
/// `keep_original` a body is itself and its mirror image together (one
/// body: the two united where they meet, side by side where they don't);
/// without, it's only the image, its faces keeping their names as a
/// move's do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mirror {
    /// As a move's: `1..=`[`MAX_FEATURE_BODIES`] bodies features before
    /// it make, sorted without repeats.
    pub bodies: Vec<BodyId>,
    pub plane: PlaneRef,
    pub keep_original: bool,
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

    /// The axis as the user sees it: "X".
    pub fn name(self) -> &'static str {
        match self {
            Axis3::X => "X",
            Axis3::Y => "Y",
            Axis3::Z => "Z",
        }
    }
}

/// A plane in the world, for a mirror: an origin plane, or a flat face of
/// a body as the features before the mirror leave it. New kinds are
/// appended, as [`AxisRef`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PlaneRef {
    Origin(OriginPlane),
    Face(FaceRef),
}

impl PlaneRef {
    /// The plane as the user sees it: "XY", or "a face".
    pub fn name(self) -> &'static str {
        match self {
            PlaneRef::Origin(plane) => plane.name(),
            PlaneRef::Face(_) => "a face",
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

impl PlaneRef {
    /// The face it names on a body, if it names one.
    pub(crate) fn refers(&self) -> Option<Referred<'_>> {
        match self {
            PlaneRef::Origin(_) => None,
            PlaneRef::Face(face) => Some(Referred::Face(face)),
        }
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
    pub(crate) fn check_own(&self) -> Result<(), MotionError> {
        match self {
            Referred::Edge(edge) => edge.check_own().map_err(MotionError::Edge),
            Referred::Face(face) => face.check_own().map_err(|_| MotionError::Near(face.near)),
        }
    }

    /// The body it's on.
    pub(crate) fn body(&self) -> BodyId {
        match self {
            Referred::Edge(edge) => edge.body,
            Referred::Face(face) => face.body,
        }
    }

    /// The features its keys name as its faces' makers.
    pub(crate) fn makers(&self) -> Vec<FeatureId> {
        match self {
            Referred::Edge(edge) => edge.makers().to_vec(),
            Referred::Face(face) => vec![face.maker()],
        }
    }
}

/// Checks a feature's body list: `1..=`[`MAX_FEATURE_BODIES`], sorted
/// without repeats.
pub(crate) fn check_bodies(bodies: &[BodyId]) -> Result<(), MotionError> {
    let count = bodies.len();
    if !(1..=MAX_FEATURE_BODIES).contains(&count) {
        return Err(MotionError::Bodies(count));
    }
    if !bodies.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(MotionError::BodyOrder);
    }
    Ok(())
}

impl Move {
    /// What an offset is checked against in `design`: a length within
    /// [`MAX_COORD`] of zero, bare numbers in its units. Zero and
    /// negative offsets are offsets too.
    pub fn offset_ask(design: &Design) -> Ask {
        Ask::length(design.units, f64::from(MAX_COORD))
    }

    /// What its turn's angle is checked against in `design`: within a
    /// turn either way, bare numbers in degrees.
    pub fn angle_ask(design: &Design) -> Ask {
        Ask::angle(design.units, TAU)
    }

    /// Its offsets as a vector, in millimetres.
    pub fn offset_vector(&self) -> DVec3 {
        let [x, y, z] = &self.offset;
        DVec3::new(x.value, y.value, z.value)
    }

    /// Checks what needs only the move and `design`: the body count and
    /// order, every offset and the angle, and an axis edge's or face's own
    /// parts. What the bodies and references name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view, as [`Extrude::check_own`](crate::Extrude::check_own).
    pub fn check_own(&self, design: &Design) -> Result<(), MotionError> {
        check_bodies(&self.bodies)?;
        let ask = Move::offset_ask(design);
        for value in &self.offset {
            value.check(&ask).map_err(|_| MotionError::Offset)?;
        }
        if let Some((axis, angle)) = &self.turn {
            angle
                .check(&Move::angle_ask(design))
                .map_err(|_| MotionError::Angle)?;
            if let Some(referred) = axis.refers() {
                referred.check_own()?;
            }
        }
        Ok(())
    }

    /// The edge or face its axis names, if it names one.
    pub(crate) fn referred(&self) -> Option<Referred<'_>> {
        self.turn.as_ref().and_then(|(axis, _)| axis.refers())
    }

    /// Its values: the offsets and the angle.
    pub(crate) fn values_mut(&mut self) -> (&mut [Value; 3], Option<&mut Value>) {
        (&mut self.offset, self.turn.as_mut().map(|(_, angle)| angle))
    }
}

impl Mirror {
    /// Checks what needs only the mirror: the body count and order and a
    /// face plane's point. What the bodies and the face name is
    /// [`Document::check`](crate::Document::check)'s.
    pub fn check_own(&self) -> Result<(), MotionError> {
        check_bodies(&self.bodies)?;
        if let Some(referred) = self.plane.refers() {
            referred.check_own()?;
        }
        Ok(())
    }

    /// The face its plane names, if it names one.
    pub(crate) fn referred(&self) -> Option<Referred<'_>> {
        self.plane.refers()
    }
}

/// What's wrong with a move, a mirror or a pattern, see
/// [`CheckError::Move`](crate::CheckError::Move),
/// [`CheckError::Mirror`](crate::CheckError::Mirror) and
/// [`CheckError::Pattern`](crate::CheckError::Pattern).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MotionError {
    /// It names this many bodies: none, or over [`MAX_FEATURE_BODIES`].
    Bodies(usize),
    /// Its bodies aren't sorted, or one is repeated.
    BodyOrder,
    /// It names this body, which isn't there or which no feature before
    /// it makes.
    Body(BodyId),
    /// An offset's expression doesn't give its value, or the value isn't
    /// a length [`Move::offset_ask`] takes.
    Offset,
    /// Its turn's expression doesn't give its value, or the value isn't
    /// an angle [`Move::angle_ask`] takes; for a circular pattern, one
    /// [`Pattern::angle_ask`](crate::Pattern::angle_ask) takes.
    Angle,
    /// A pattern's count's expression doesn't give its value, or the
    /// value isn't a count [`Pattern::count_ask`](crate::Pattern::count_ask)
    /// takes.
    Count,
    /// A linear pattern's spacing's expression doesn't give its value, or
    /// the value isn't a length
    /// [`Pattern::spacing_ask`](crate::Pattern::spacing_ask) takes, or is
    /// zero.
    Spacing,
    /// Its axis edge fails its own check ([`EdgeRef::check_own`]).
    Edge(EdgeError),
    /// Its axis's or plane's face's point isn't finite, or is further
    /// from zero than [`MAX_COORD`].
    Near(DVec3),
    /// Its axis's or plane's body, this one, is made by the feature or one
    /// after it, or isn't there and has an id a body made later could
    /// take.
    RefBody(BodyId),
    /// A key of its axis's or plane's faces names this feature, which is
    /// the feature itself or comes after it, or isn't there and has an id
    /// a feature made later could take.
    RefMaker(FeatureId),
}

impl fmt::Display for MotionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MotionError::Bodies(count) => {
                write!(f, "names {count} bodies, not 1 to {MAX_FEATURE_BODIES}")
            }
            MotionError::BodyOrder => f.write_str("its bodies are out of order or repeated"),
            MotionError::Body(body) => write!(
                f,
                "names body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            MotionError::Offset => f.write_str("an offset's expression doesn't give its value"),
            MotionError::Angle => f.write_str("its angle's expression doesn't give its value"),
            MotionError::Count => f.write_str("its count isn't a whole number from 2 to 1024"),
            MotionError::Spacing => {
                f.write_str("its spacing's expression doesn't give its value, or it's zero")
            }
            MotionError::Edge(why) => write!(f, "its axis: {why}"),
            MotionError::Near(at) => write!(f, "its face's point {at} is out of bounds"),
            MotionError::RefBody(body) => write!(
                f,
                "its axis or plane is on body {}, which isn't made before it",
                body.0
            ),
            MotionError::RefMaker(feature) => write!(
                f,
                "its axis or plane is on a face made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl std::error::Error for MotionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MotionError::Edge(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
