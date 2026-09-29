//! The extrude feature: regions of an earlier sketch swept along its
//! plane's normal into a solid, which makes a new body or, once booleans
//! come, joins, cuts or intersects bodies already there.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};
use varde_sketch::{MIN_LENGTH, RegionRef, RegionRefError};

use crate::{BodyId, Design, FeatureId, MAX_COORD};

/// The most regions one extrude may take. They're merged into one
/// profile first, so this bounds that work, and what a file can ask for.
pub const MAX_EXTRUDE_REGIONS: usize = 256;

/// An extrude: the regions of sketch `sketch` it takes, how far it goes,
/// and what it does with the solid it makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extrude {
    /// An earlier sketch feature.
    pub sketch: FeatureId,
    /// The regions, `1..=`[`MAX_EXTRUDE_REGIONS`] of them, as made by
    /// `varde_sketch::Profiles::reference` from the sketch as it was when
    /// they were picked. Adjacent ones are merged before extruding.
    pub regions: Vec<RegionRef>,
    pub extent: Extent,
    /// Swaps the direction of [`Extent::OneSide`] and the two sides of
    /// [`Extent::TwoSides`]; the others ignore it.
    pub flip: bool,
    pub operation: Operation,
}

/// How far an extrude goes along its sketch plane's normal. Distances are
/// lengths in millimetres, typed as expressions, each at least
/// [`MIN_LENGTH`] and at most [`MAX_COORD`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Extent {
    /// From the sketch plane this far along its normal.
    OneSide(Value),
    /// This far in all, half each side of the sketch plane.
    Symmetric(Value),
    /// The first this far along the normal, the second this far against
    /// it; together at most [`MAX_COORD`].
    TwoSides(Value, Value),
    /// Through every body it cuts: only for [`Operation::Cut`], its span
    /// worked out when regenerating from the bodies it touches.
    ThroughAll,
}

/// What an extrude does with the solid it makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    /// Makes a body of its own, this one, which names the extrude as its
    /// maker. A command adding one gives it a new id whatever it holds:
    /// [`BodyId::NEW`] stands for it until then.
    NewBody(BodyId),
    Join(Targets),
    Cut(Targets),
    Intersect(Targets),
}

/// The bodies a join, cut or intersect works on: every body the extrude
/// touches but those the user took out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Targets {
    /// Sorted without repeats, each a body an earlier feature makes.
    pub excluded: Vec<BodyId>,
}

impl Operation {
    /// The bodies taken out of a join, cut or intersect's targets.
    pub fn excluded(&self) -> &[BodyId] {
        match self {
            Operation::NewBody(_) => &[],
            Operation::Join(targets) | Operation::Cut(targets) | Operation::Intersect(targets) => {
                &targets.excluded
            }
        }
    }

    /// The same, to change.
    pub(crate) fn excluded_mut(&mut self) -> Option<&mut Vec<BodyId>> {
        match self {
            Operation::NewBody(_) => None,
            Operation::Join(targets) | Operation::Cut(targets) | Operation::Intersect(targets) => {
                Some(&mut targets.excluded)
            }
        }
    }

    /// The body it makes, for [`Operation::NewBody`].
    pub fn new_body(&self) -> Option<BodyId> {
        match *self {
            Operation::NewBody(body) => Some(body),
            _ => None,
        }
    }
}

impl Extent {
    /// What a distance is checked against in `design`: a length at least
    /// [`MIN_LENGTH`] and at most [`MAX_COORD`], bare numbers in its units.
    pub fn ask(design: &Design) -> Ask {
        Ask::length(design.units, f64::from(MAX_COORD))
            .positive()
            .at_least(MIN_LENGTH)
    }

    /// Its distances.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        let (a, b) = match self {
            Extent::OneSide(a) | Extent::Symmetric(a) => (Some(a), None),
            Extent::TwoSides(a, b) => (Some(a), Some(b)),
            Extent::ThroughAll => (None, None),
        };
        a.into_iter().chain(b)
    }

    /// The same, to change.
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        let (a, b) = match self {
            Extent::OneSide(a) | Extent::Symmetric(a) => (Some(a), None),
            Extent::TwoSides(a, b) => (Some(a), Some(b)),
            Extent::ThroughAll => (None, None),
        };
        a.into_iter().chain(b)
    }
}

impl Extrude {
    /// Where the extrude runs along its sketch plane's normal, as
    /// `(from, to)` with `from < to`, the plane at 0: flipped, one side
    /// runs back from the plane and two sides swap. None for
    /// [`Extent::ThroughAll`], whose span depends on the bodies.
    pub fn span(&self) -> Option<(f64, f64)> {
        let (from, to) = match &self.extent {
            Extent::OneSide(d) => (0.0, d.value),
            Extent::Symmetric(d) => (-d.value / 2.0, d.value / 2.0),
            Extent::TwoSides(a, b) => (-b.value, a.value),
            Extent::ThroughAll => return None,
        };
        let flips = matches!(self.extent, Extent::OneSide(_) | Extent::TwoSides(..));
        Some(if self.flip && flips {
            (-to, -from)
        } else {
            (from, to)
        })
    }

    /// Checks what needs only the extrude and `design`: the region count
    /// and each region, the distances and their sum, and that through all
    /// only cuts. The references to other features and bodies are
    /// [`Document::check`](crate::Document::check)'s.
    pub(crate) fn check_own(&self, design: &Design) -> Result<(), ExtrudeError> {
        let count = self.regions.len();
        if !(1..=MAX_EXTRUDE_REGIONS).contains(&count) {
            return Err(ExtrudeError::Regions(count));
        }
        for region in &self.regions {
            region
                .check(f64::from(MAX_COORD))
                .map_err(ExtrudeError::Region)?;
        }
        let ask = Extent::ask(design);
        for value in self.extent.values() {
            value.check(&ask).map_err(|_| ExtrudeError::Distance)?;
        }
        if let Extent::TwoSides(a, b) = &self.extent
            // Both finite and positive, as checked above.
            && a.value + b.value > f64::from(MAX_COORD)
        {
            return Err(ExtrudeError::Length);
        }
        if self.extent == Extent::ThroughAll && !matches!(self.operation, Operation::Cut(_)) {
            return Err(ExtrudeError::ThroughAll);
        }
        Ok(())
    }
}

/// What's wrong with an extrude, see
/// [`CheckError::Extrude`](crate::CheckError::Extrude).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExtrudeError {
    /// Its sketch, this feature, isn't a sketch feature before it.
    Sketch(FeatureId),
    /// It takes this many regions: none, or over [`MAX_EXTRUDE_REGIONS`].
    Regions(usize),
    /// A region reference fails its check.
    Region(RegionRefError),
    /// A distance's expression doesn't give its value in the design's
    /// units, or the value isn't a length [`Extent::ask`] takes.
    Distance,
    /// Its two sides come to more than [`MAX_COORD`].
    Length,
    /// It goes through all, but doesn't cut.
    ThroughAll,
    /// The body it makes, this one, isn't there, or names another maker.
    NewBody(BodyId),
    /// It excludes this body, which no feature before it makes.
    Excluded(BodyId),
    /// Its excluded bodies aren't sorted, or one is repeated.
    ExcludedOrder,
}

impl fmt::Display for ExtrudeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtrudeError::Sketch(sketch) => write!(
                f,
                "extrudes feature {}, which isn't a sketch before it",
                sketch.0
            ),
            ExtrudeError::Regions(count) => write!(
                f,
                "extrudes {count} regions, not 1 to {MAX_EXTRUDE_REGIONS}"
            ),
            ExtrudeError::Region(why) => why.fmt(f),
            ExtrudeError::Distance => f.write_str("a distance's expression doesn't give its value"),
            ExtrudeError::Length => write!(f, "its two sides come to over {MAX_COORD} mm"),
            ExtrudeError::ThroughAll => f.write_str("only a cut can go through all"),
            ExtrudeError::NewBody(body) => write!(
                f,
                "makes body {}, which isn't there or has another maker",
                body.0
            ),
            ExtrudeError::Excluded(body) => write!(
                f,
                "excludes body {}, which no earlier feature makes",
                body.0
            ),
            ExtrudeError::ExcludedOrder => {
                f.write_str("its excluded bodies are out of order or repeated")
            }
        }
    }
}

impl std::error::Error for ExtrudeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExtrudeError::Region(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
