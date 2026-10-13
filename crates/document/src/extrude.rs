//! The extrude feature: regions of an earlier sketch swept along its
//! plane's normal into a solid, which makes a new body or, once booleans
//! come, joins, cuts or intersects bodies already there.

use std::f64::consts::FRAC_PI_2;

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
    /// The angle its walls lean by ([`Extrude::taper_ask`]: under 90°
    /// either way), if they do: positive narrows the profile away from
    /// the sketch's plane, on both sides of it for two sides and
    /// symmetric, negative widens it. Each wall turns about where it
    /// meets the sketch's plane, also where the extrude doesn't reach the
    /// plane (through all). None: straight walls, as every extrude made
    /// before tapers came reads (`#[serde(default)]`). The panel stores a
    /// taper of zero as none; one read from a file is taken as none too
    /// ([`Extrude::tapered`]).
    #[serde(default)]
    pub taper: Option<Value>,
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
    /// The id of the body the feature made when it was a new body, if it
    /// was one, which the document holds for it (no other body or feature
    /// gets it), so that making a new body again brings the body back
    /// with it, and what was named on it (a sketch on its face) finds it
    /// again. The commands fill it in, whatever it holds: none when
    /// adding, the feature's body or held id when setting.
    /// `#[serde(default)]`: none, as every operation written before ids
    /// were held reads.
    #[serde(default)]
    pub held: Option<BodyId>,
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

    /// The id it holds for the body it made as a new body before, for a
    /// join, cut or intersect ([`Targets::held`]).
    pub fn held_body(&self) -> Option<BodyId> {
        match self {
            Operation::NewBody(_) => None,
            Operation::Join(targets) | Operation::Cut(targets) | Operation::Intersect(targets) => {
                targets.held
            }
        }
    }
}

impl Extent {
    /// What a distance is checked against in `design`: a length at least
    /// [`MIN_LENGTH`] and at most [`MAX_COORD`], bare numbers in its units.
    pub fn ask<'p>(design: &Design<'p>) -> Ask<'p> {
        design
            .length(f64::from(MAX_COORD))
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
    /// What a taper is checked against in `design`: an angle under a
    /// right angle either way, bare numbers in degrees.
    pub fn taper_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        design.angle(FRAC_PI_2).under_max()
    }

    /// Its taper, if its walls lean: none for no taper or one of zero
    /// (either sign), which is the untapered extrude exactly.
    pub fn tapered(&self) -> Option<&Value> {
        self.taper.as_ref().filter(|taper| taper.value != 0.0)
    }

    /// Its typed values and what each is checked against in `design`:
    /// its distances and its taper.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> Vec<(&mut Value, Ask<'p>)> {
        let length = Extent::ask(design);
        let mut values: Vec<(&mut Value, Ask<'p>)> = self
            .extent
            .values_mut()
            .map(|value| (value, length))
            .collect();
        if let Some(taper) = &mut self.taper {
            values.push((taper, Extrude::taper_ask(design)));
        }
        values
    }

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
    /// and each region, the distances and their sum, the taper, and that
    /// through all only cuts. The references to other features and bodies are
    /// [`Document::check`](crate::Document::check)'s.
    ///
    /// It's the part of [`Document::check`](crate::Document::check) that
    /// looks at the extrude alone, for callers who want to know before
    /// applying it: the extrude panel runs it on every view to tell
    /// whether OK can be pressed, so it has to stay cheap and depend on
    /// nothing more than the extrude and the design.
    pub fn check_own(&self, design: &Design) -> Result<(), ExtrudeError> {
        crate::extrude::check_regions(
            &self.regions,
            MAX_EXTRUDE_REGIONS,
            ExtrudeError::Regions,
            ExtrudeError::Region,
        )?;
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
        if let Some(taper) = &self.taper {
            taper
                .check(&Extrude::taper_ask(design))
                .map_err(|_| ExtrudeError::Taper)?;
        }
        if self.extent == Extent::ThroughAll && !matches!(self.operation, Operation::Cut(_)) {
            return Err(ExtrudeError::ThroughAll);
        }
        Ok(())
    }
}

/// Checks the regions a profile feature (extrude, revolve, sweep, split)
/// takes: `1..=max` of them, each passing its own check, saying what's
/// wrong by `count` or `region`.
pub(crate) fn check_regions<E>(
    regions: &[RegionRef],
    max: usize,
    count: impl FnOnce(usize) -> E,
    region: impl Fn(RegionRefError) -> E,
) -> Result<(), E> {
    if !(1..=max).contains(&regions.len()) {
        return Err(count(regions.len()));
    }
    for each in regions {
        each.check(f64::from(MAX_COORD)).map_err(&region)?;
    }
    Ok(())
}

/// What's wrong with an extrude, see
/// [`CheckError::Extrude`](crate::CheckError::Extrude).
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum ExtrudeError {
    /// Its sketch, this feature, isn't a sketch feature before it.
    #[error("extrudes feature {}, which isn't a sketch before it", .0.0)]
    Sketch(FeatureId),
    /// It takes this many regions: none, or over [`MAX_EXTRUDE_REGIONS`].
    #[error("extrudes {0} regions, not 1 to {MAX_EXTRUDE_REGIONS}")]
    Regions(usize),
    /// A region reference fails its check.
    #[error("{0}")]
    Region(#[source] RegionRefError),
    /// A distance's expression doesn't give its value in the design's
    /// units, or the value isn't a length [`Extent::ask`] takes.
    #[error("a distance's expression doesn't give its value")]
    Distance,
    /// Its two sides come to more than [`MAX_COORD`].
    #[error("its two sides come to over {MAX_COORD} mm")]
    Length,
    /// Its taper's expression doesn't give its value, or the value isn't
    /// an angle [`Extrude::taper_ask`] takes.
    #[error("its taper isn't an angle under 90° either way")]
    Taper,
    /// It goes through all, but doesn't cut.
    #[error("only a cut can go through all")]
    ThroughAll,
    /// Its operation's new or excluded bodies are wrong.
    #[error("{0}")]
    Operation(#[source] OperationError),
}

/// What's wrong with the bodies an [`Operation`] names, which the
/// kinds that take one (extrude, revolve, sweep, loft) share.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum OperationError {
    /// The body it makes, this one, isn't there, or names another maker.
    #[error("makes body {}, which isn't there or has another maker", .0.0)]
    NewBody(BodyId),
    /// It excludes this body, which no feature before it makes.
    #[error("excludes body {}, which no earlier feature makes", .0.0)]
    Excluded(BodyId),
    /// Its excluded bodies aren't sorted, or one is repeated.
    #[error("its excluded bodies are out of order or repeated")]
    ExcludedOrder,
}

#[cfg(test)]
mod tests;
