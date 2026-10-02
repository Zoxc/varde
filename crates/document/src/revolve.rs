//! The revolve feature: regions of an earlier sketch turned about a line
//! in its plane into a solid, which makes a new body or joins, cuts or
//! intersects bodies already there, as an extrude's does.

use std::f64::consts::TAU;
use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};
use varde_sketch::{Curve, Id, RegionRef, RegionRefError, Sketch};

use crate::{BodyId, Design, EdgeError, EdgeRef, FeatureId, MAX_COORD, Operation};

/// The most regions one revolve may take, as [`MAX_EXTRUDE_REGIONS`]
/// for an extrude.
///
/// [`MAX_EXTRUDE_REGIONS`]: crate::MAX_EXTRUDE_REGIONS
pub const MAX_REVOLVE_REGIONS: usize = 256;

/// How far two sides' angles may add up past a turn, or a turn's span
/// fall short of one, and still be a whole turn: the rounding of angles
/// typed in degrees. "0.5" and "359.5" come to a little over `TAU` in
/// radians, "180.1" and "179.9" a little under; both are a turn.
const TURN_ROUNDING: f64 = 8.0 * f64::EPSILON * TAU;

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
    pub fn ask(design: &Design) -> Ask {
        Ask::angle(design.units, TAU).positive()
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
        let count = self.regions.len();
        if !(1..=MAX_REVOLVE_REGIONS).contains(&count) {
            return Err(RevolveError::Regions(count));
        }
        for region in &self.regions {
            region
                .check(f64::from(MAX_COORD))
                .map_err(RevolveError::Region)?;
        }
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RevolveError {
    /// Its sketch, this feature, isn't a sketch feature before it.
    Sketch(FeatureId),
    /// It takes this many regions: none, or over [`MAX_REVOLVE_REGIONS`].
    Regions(usize),
    /// A region reference fails its check.
    Region(RegionRefError),
    /// An angle's expression doesn't give its value, or the value isn't
    /// an angle [`Turn::ask`] takes.
    Angle,
    /// Its two sides come to more than a turn.
    Turn,
    /// Its axis, this curve, isn't a line of its sketch (when added or
    /// set, see [`Revolve::check_axis`]).
    Axis(Id),
    /// The body it makes, this one, isn't there, or names another maker.
    NewBody(BodyId),
    /// It excludes this body, which no feature before it makes.
    Excluded(BodyId),
    /// Its excluded bodies aren't sorted, or one is repeated.
    ExcludedOrder,
    /// Its axis edge's reference fails its own check
    /// ([`EdgeRef::check_own`]).
    Edge(EdgeError),
    /// Its axis edge's body, this one, is made by the revolve or a
    /// feature after it, or isn't there and has an id a body made later
    /// could take.
    EdgeBody(BodyId),
    /// A key of its axis edge's faces names this feature, which is the
    /// revolve or comes after it, or isn't there and has an id a feature
    /// made later could take.
    EdgeMaker(FeatureId),
}

impl fmt::Display for RevolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RevolveError::Sketch(sketch) => write!(
                f,
                "revolves feature {}, which isn't a sketch before it",
                sketch.0
            ),
            RevolveError::Regions(count) => write!(
                f,
                "revolves {count} regions, not 1 to {MAX_REVOLVE_REGIONS}"
            ),
            RevolveError::Region(why) => why.fmt(f),
            RevolveError::Angle => f.write_str("an angle's expression doesn't give its value"),
            RevolveError::Turn => f.write_str("its two sides come to over a turn"),
            RevolveError::Axis(id) => write!(f, "its axis, curve {id}, isn't a line of its sketch"),
            RevolveError::NewBody(body) => write!(
                f,
                "makes body {}, which isn't there or has another maker",
                body.0
            ),
            RevolveError::Excluded(body) => write!(
                f,
                "excludes body {}, which no earlier feature makes",
                body.0
            ),
            RevolveError::ExcludedOrder => {
                f.write_str("its excluded bodies are out of order or repeated")
            }
            RevolveError::Edge(why) => write!(f, "its axis: {why}"),
            RevolveError::EdgeBody(body) => write!(
                f,
                "its axis is an edge of body {}, which isn't made before it",
                body.0
            ),
            RevolveError::EdgeMaker(feature) => write!(
                f,
                "its axis is an edge of a face made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl std::error::Error for RevolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RevolveError::Region(why) => Some(why),
            RevolveError::Edge(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
