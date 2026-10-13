//! The revolve feature: regions of an earlier sketch turned about a line
//! in its plane into a solid, which makes a new body or joins, cuts or
//! intersects bodies already there, as an extrude's does.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};
use varde_sketch::{Curve, Id, RegionRef, RegionRefError, Sketch};

use crate::{BodyId, Design, EdgeError, EdgeRef, FeatureId, Operation, OperationError};

/// The most regions one revolve may take, as [`MAX_EXTRUDE_REGIONS`]
/// for an extrude.
///
/// [`MAX_EXTRUDE_REGIONS`]: crate::MAX_EXTRUDE_REGIONS
pub const MAX_REVOLVE_REGIONS: usize = 256;

/// How far two sides' angles may add up past a turn, or a turn's span
/// fall short of one, and still be a whole turn: the rounding of angles
/// typed in degrees. "0.5" and "359.5" come to a little over `TAU` in
/// radians, "180.1" and "179.9" a little under; both are a turn.
pub(crate) const TURN_ROUNDING: f64 = 8.0 * f64::EPSILON * TAU;

/// A revolve: the regions of sketch `sketch` it takes, the line they turn
/// about, how far they turn, and what it does with the solid it makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revolve {
    /// An earlier sketch feature.
    pub sketch: FeatureId,
    /// The regions, `1..=`[`MAX_REVOLVE_REGIONS`] of them, made as an
    /// extrude's are. Adjacent ones are merged before revolving.
    pub regions: Vec<RegionRef>,
    pub axis: AxisLine,
    pub extent: Turn,
    /// Swaps the direction of [`Turn::OneSide`] and the two sides of
    /// [`Turn::TwoSides`]; the others ignore it.
    pub flip: bool,
    pub operation: Operation,
}

/// The line a revolve turns about, in its sketch's plane, and its
/// direction, which says which way positive angles turn (right-handed
/// about it). New kinds are appended: a kind's place in the list is how
/// files store it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AxisLine {
    /// A line of the sketch, construction or not, directed from its start
    /// point to its end. The sketch may lose it or the line may stop
    /// being straight in later edits, which regenerating reports, as a
    /// region that's no longer there; only adding or setting the revolve
    /// requires it to be a line of the sketch then (see
    /// [`Revolve::check_axis`]).
    Curve(Id),
    /// The sketch's own x axis, directed along +x.
    SketchX,
    /// The sketch's own y axis, directed along +y.
    SketchY,
    /// A straight edge of a body, as the features before the revolve
    /// leave it, directed as [`EdgeRef`] says. Regenerating finds it on
    /// the body (or the body a join merged it into) when the history
    /// reaches the revolve, and requires it straight and in the sketch's
    /// plane: the axis is then the line through its two ends, mapped into
    /// the sketch. The document requires only that it names a body and
    /// features before the revolve ([`Document::check`](crate::Document::check)).
    Edge(EdgeRef),
}

/// How far a revolve turns about its axis. Angles are in radians, typed as
/// expressions (bare numbers in degrees), each above zero and at most a
/// turn ([`Turn::ask`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Turn {
    /// A whole turn.
    Full,
    /// From the sketch plane this far, right-handed about the axis.
    OneSide(Value),
    /// This far in all, half each side of the sketch plane.
    Symmetric(Value),
    /// The first this far right-handed about the axis, the second this
    /// far the other way; together at most a turn.
    TwoSides(Value, Value),
}

impl Turn {
    /// What an angle is checked against in `design`: above zero and at
    /// most a turn, bare numbers in degrees.
    pub fn ask<'p>(design: &Design<'p>) -> Ask<'p> {
        design.angle(TAU).positive()
    }

    /// Its angles.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        let (a, b) = match self {
            Turn::OneSide(a) | Turn::Symmetric(a) => (Some(a), None),
            Turn::TwoSides(a, b) => (Some(a), Some(b)),
            Turn::Full => (None, None),
        };
        a.into_iter().chain(b)
    }

    /// The same, to change.
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        let (a, b) = match self {
            Turn::OneSide(a) | Turn::Symmetric(a) => (Some(a), None),
            Turn::TwoSides(a, b) => (Some(a), Some(b)),
            Turn::Full => (None, None),
        };
        a.into_iter().chain(b)
    }
}

impl Revolve {
    /// Its values: its extent's angles, with the ask they're checked
    /// against in `design`.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        let angle = Turn::ask(design);
        self.extent.values_mut().map(|v| (v, angle)).collect()
    }

    /// Where the revolve turns about its axis, as `(from, to)` in radians
    /// with `from < to`, the sketch plane at 0 and positive angles
    /// right-handed about the axis: flipped, one side turns back from the
    /// plane and two sides swap. None for a whole turn: [`Turn::Full`],
    /// or any other that comes to a turn (one side or symmetric of a
    /// turn, two sides adding up to one, to the rounding of angles typed
    /// in degrees), which is the same solid.
    ///
    /// Of a checked revolve, `0 < to - from < TAU` when it's some, both
    /// ends within a turn of 0.
    pub fn span(&self) -> Option<(f64, f64)> {
        let (from, to) = match &self.extent {
            Turn::Full => return None,
            Turn::OneSide(a) => (0.0, a.value),
            Turn::Symmetric(a) => (-a.value / 2.0, a.value / 2.0),
            Turn::TwoSides(a, b) => (-b.value, a.value),
        };
        // Within a turn each, as checked, so no overflow.
        if to - from >= TAU - TURN_ROUNDING {
            return None;
        }
        let flips = matches!(self.extent, Turn::OneSide(_) | Turn::TwoSides(..));
        Some(if self.flip && flips {
            (-to, -from)
        } else {
            (from, to)
        })
    }

    /// Checks what needs only the revolve and `design`: the region count
    /// and each region, the angles and their sum. The references to other
    /// features and bodies are [`Document::check`](crate::Document::check)'s,
    /// and the axis line [`Revolve::check_axis`]'s. Cheap, for a panel to
    /// run on every view, as [`Extrude::check_own`](crate::Extrude::check_own).
    pub fn check_own(&self, design: &Design) -> Result<(), RevolveError> {
        crate::extrude::check_regions(
            &self.regions,
            MAX_REVOLVE_REGIONS,
            RevolveError::Regions,
            RevolveError::Region,
        )?;
        let ask = Turn::ask(design);
        for value in self.extent.values() {
            value.check(&ask).map_err(|_| RevolveError::Angle)?;
        }
        if let Turn::TwoSides(a, b) = &self.extent
            // Both finite and within a turn, as checked above.
            && a.value + b.value > TAU + TURN_ROUNDING
        {
            return Err(RevolveError::Turn);
        }
        Ok(())
    }

    /// Checks that the axis is a line of `sketch`, the revolve's sketch as
    /// it is now: what adding or setting a revolve requires
    /// ([`Command::AddFeature`](crate::Command::AddFeature),
    /// [`Command::SetFeature`](crate::Command::SetFeature)). A document
    /// doesn't require it of a revolve already there, since a later edit
    /// of the sketch may remove the line.
    pub fn check_axis(&self, sketch: &Sketch) -> Result<(), RevolveError> {
        match self.axis {
            AxisLine::Curve(id) => match sketch.curve(id) {
                Some(entry) if matches!(entry.curve, Curve::Line { .. }) => Ok(()),
                _ => Err(RevolveError::Axis(id)),
            },
            AxisLine::SketchX | AxisLine::SketchY | AxisLine::Edge(_) => Ok(()),
        }
    }
}

/// What's wrong with a revolve, see
/// [`CheckError::Revolve`](crate::CheckError::Revolve).
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum RevolveError {
    /// Its sketch, this feature, isn't a sketch feature before it.
    #[error("revolves feature {}, which isn't a sketch before it", .0.0)]
    Sketch(FeatureId),
    /// It takes this many regions: none, or over [`MAX_REVOLVE_REGIONS`].
    #[error("revolves {0} regions, not 1 to {MAX_REVOLVE_REGIONS}")]
    Regions(usize),
    /// A region reference fails its check.
    #[error("{0}")]
    Region(#[source] RegionRefError),
    /// An angle's expression doesn't give its value, or the value isn't
    /// an angle [`Turn::ask`] takes.
    #[error("an angle's expression doesn't give its value")]
    Angle,
    /// Its two sides come to more than a turn.
    #[error("its two sides come to over a turn")]
    Turn,
    /// Its axis, this curve, isn't a line of its sketch (when added or
    /// set, see [`Revolve::check_axis`]).
    #[error("its axis, curve {0}, isn't a line of its sketch")]
    Axis(Id),
    /// Its operation's new or excluded bodies are wrong.
    #[error("{0}")]
    Operation(#[source] OperationError),
    /// Its axis edge's reference fails its own check
    /// ([`EdgeRef::check_own`]).
    #[error("its axis: {0}")]
    Edge(#[source] EdgeError),
    /// Its axis edge's body, this one, is made by the revolve or a
    /// feature after it, or isn't there and has an id a body made later
    /// could take.
    #[error("its axis is an edge of body {}, which isn't made before it", .0.0)]
    EdgeBody(BodyId),
    /// A key of its axis edge's faces names this feature, which is the
    /// revolve or comes after it, or isn't there and has an id a feature
    /// made later could take.
    #[error("its axis is an edge of a face made by feature {}, which doesn't come before it", .0.0)]
    EdgeMaker(FeatureId),
}

#[cfg(test)]
mod tests;
