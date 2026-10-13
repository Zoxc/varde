//! The move and mirror features: bodies already made moved (turned about
//! an axis, then shifted) or mirrored in a plane, as steps of the history,
//! naming their axes and planes as `refs` has them.

use std::f64::consts::TAU;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::bodies::check_bodies;
use crate::refs::Referred;
use crate::{AxisRef, BodiesError, BodyId, Design, FeatureId, MAX_COORD, Plane, RefError};

/// A move: the bodies it moves, each turned about `turn`'s axis by its
/// angle (if it has one), then shifted by `offset`. Each body keeps its
/// id and its faces their names: later features find them where they
/// went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Move {
    /// `1..=`[`MAX_FEATURE_BODIES`](crate::MAX_FEATURE_BODIES) bodies features before it make,
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
    /// As a move's: `1..=`[`MAX_FEATURE_BODIES`](crate::MAX_FEATURE_BODIES) bodies features before
    /// it make, sorted without repeats.
    pub bodies: Vec<BodyId>,
    pub plane: Plane,
    pub keep_original: bool,
}

impl Move {
    /// What an offset is checked against in `design`: a length within
    /// [`MAX_COORD`] of zero, bare numbers in its units. Zero and
    /// negative offsets are offsets too.
    pub fn offset_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        design.length(f64::from(MAX_COORD))
    }

    /// What its turn's angle is checked against in `design`: within a
    /// turn either way, bare numbers in degrees.
    pub fn angle_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        design.angle(TAU)
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
        check_bodies(&self.bodies).map_err(MotionError::Bodies)?;
        let ask = Move::offset_ask(design);
        for value in &self.offset {
            value.check(&ask).map_err(|_| MotionError::Offset)?;
        }
        if let Some((axis, angle)) = &self.turn {
            angle
                .check(&Move::angle_ask(design))
                .map_err(|_| MotionError::Angle)?;
            if let Some(referred) = axis.refers() {
                referred.check_own().map_err(MotionError::Ref)?;
            }
        }
        Ok(())
    }

    /// The edge or face its axis names, if it names one.
    pub(crate) fn referred(&self) -> Option<Referred<'_>> {
        self.turn.as_ref().and_then(|(axis, _)| axis.refers())
    }

    /// Its values: the offsets and the angle, with the ask each is
    /// checked against in `design`.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        let offset = Move::offset_ask(design);
        let angle = (self.turn.as_mut()).map(|(_, angle)| (angle, Move::angle_ask(design)));
        (self.offset.iter_mut().map(|v| (v, offset)))
            .chain(angle)
            .collect()
    }
}

impl Mirror {
    /// Checks what needs only the mirror: the body count and order and a
    /// face plane's point. What the bodies and the face name is
    /// [`Document::check`](crate::Document::check)'s.
    pub fn check_own(&self) -> Result<(), MotionError> {
        check_bodies(&self.bodies).map_err(MotionError::Bodies)?;
        if let Some(referred) = self.plane.refers() {
            referred.check_own().map_err(MotionError::Ref)?;
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
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum MotionError {
    /// Its body list is wrong (`check_bodies`).
    #[error("{0}")]
    Bodies(#[source] BodiesError),
    /// It names this body, which isn't there or which no feature before
    /// it makes.
    #[error("names body {}, which isn't there or no earlier feature makes", .0.0)]
    Body(BodyId),
    /// An offset's expression doesn't give its value, or the value isn't
    /// a length [`Move::offset_ask`] takes.
    #[error("an offset's expression doesn't give its value")]
    Offset,
    /// Its turn's expression doesn't give its value, or the value isn't
    /// an angle [`Move::angle_ask`] takes; for a circular pattern, one
    /// [`Pattern::angle_ask`](crate::Pattern::angle_ask) takes.
    #[error("its angle's expression doesn't give its value")]
    Angle,
    /// Its axis's edge or face, or its plane's face, fails its own check.
    #[error("{0}")]
    Ref(#[source] RefError),
    /// Its axis's or plane's body, this one, is made by the feature or one
    /// after it, or isn't there and has an id a body made later could
    /// take.
    #[error("its axis or plane is on body {}, which isn't made before it", .0.0)]
    RefBody(BodyId),
    /// A key of its axis's or plane's faces names this feature, which is
    /// the feature itself or comes after it, or isn't there and has an id
    /// a feature made later could take.
    #[error(
        "its axis or plane is on a face made by feature {}, which doesn't come before it",
        .0.0
    )]
    RefMaker(FeatureId),
}

#[cfg(test)]
mod tests;
