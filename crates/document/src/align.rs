//! The align feature: one body moved rigidly, in place, so a point and
//! directions picked on it meet a point and directions picked on other
//! bodies or the origin, as `refs` names them.

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::refs::OnBody;
use crate::{BodyId, Design, DirRef, FeatureId, Move, PointRef, PointRefError};

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
    pub(crate) fn named(&self) -> impl Iterator<Item = OnBody> {
        let directions =
            (self.directions()).filter_map(|direction| Some(direction.refers()?.on_body()));
        self.point.on_body().into_iter().chain(directions)
    }

    /// Whether any of its references is the origin's.
    fn has_origin(&self) -> bool {
        self.point == PointRef::Origin
            || (self.directions()).any(|direction| direction.refers().is_none())
    }
}

impl Align {
    /// Its values: the offset and the turn, with the ask each is checked
    /// against in `design`, a move's.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        let offset = (self.offset.as_mut()).map(|v| (v, Move::offset_ask(design)));
        let turn = (self.turn.as_mut()).map(|v| (v, Move::angle_ask(design)));
        offset.into_iter().chain(turn).collect()
    }

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
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum AlignError {
    /// It moves this body, which isn't there or which no feature before
    /// it makes.
    #[error("moves body {}, which isn't there or no earlier feature makes", .0.0)]
    Body(BodyId),
    /// Its sides' directions don't pair up: a primary or a secondary on
    /// one side only, or a secondary without a primary.
    #[error("its directions don't pair up")]
    Unpaired,
    /// It flips, offsets or turns without primary directions to do it
    /// along.
    #[error("it flips, offsets or turns without directions to do it along")]
    Options,
    /// Its offset's expression doesn't give its value, or the value isn't
    /// a length [`Move::offset_ask`] takes.
    #[error("its offset's expression doesn't give its value")]
    Offset,
    /// Its turn's expression doesn't give its value, or the value isn't
    /// an angle [`Move::angle_ask`] takes.
    #[error("its turn's expression doesn't give its value")]
    Angle,
    /// A reference fails its own check ([`PointRef::check_own`],
    /// [`DirRef::check_own`]).
    #[error("{0}")]
    Ref(#[from] PointRefError),
    /// A reference on the moved side is the origin's.
    #[error("the moved side names the origin")]
    FromOrigin,
    /// A reference on the moved side is on this body, not the one moved.
    #[error("the moved side names body {}, not the body moved", .0.0)]
    FromBody(BodyId),
    /// A reference on the target side is on the body moved.
    #[error("the target side names the body moved")]
    OnMoved,
    /// A reference on the target side is on this body, which is made by
    /// the feature or one after it, or isn't there and has an id a body
    /// made later could take.
    #[error("a reference is on body {}, which isn't made before it", .0.0)]
    RefBody(BodyId),
    /// A key of a reference's faces names this feature, which is the
    /// feature itself or comes after it, or isn't there and has an id a
    /// feature made later could take.
    #[error("a reference is on a face made by feature {}, which doesn't come before it", .0.0)]
    RefMaker(FeatureId),
}

#[cfg(test)]
mod tests;
