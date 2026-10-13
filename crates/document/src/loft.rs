//! The loft feature: a solid through two or more sections in order, each
//! a region of one loop of an earlier sketch or (first or last only) a
//! point of one, ruled or smooth, open or closed, following up to four
//! rails; it makes a new body or joins, cuts or intersects bodies as an
//! extrude does.

use serde::{Deserialize, Serialize};
use varde_sketch::{Id, RegionRef, RegionRefError, Sketch};

use crate::sorted_unique;
use crate::{CurveChain, FeatureId, MAX_COORD, Operation, OperationError};

/// The most sections one loft may have.
pub const MAX_LOFT_SECTIONS: usize = 64;

/// The most rails one loft may follow.
pub const MAX_LOFT_RAILS: usize = 4;

/// The most curves one rail may hold.
pub const MAX_RAIL_CURVES: usize = 256;

/// A loft: its sections in order, how it runs between them, and what it
/// does with the solid it makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loft {
    /// `2..=`[`MAX_LOFT_SECTIONS`], in the order the loft runs through
    /// them; a point only first or last, and not every one a point.
    pub sections: Vec<Section>,
    pub mode: LoftMode,
    /// The last section lofts back to the first, with no caps: three or
    /// more sections, none a point, no rails.
    pub closed: bool,
    /// Up to [`MAX_LOFT_RAILS`] chains of sketch curves the sections'
    /// vertices follow, each through the matching vertex of every
    /// section.
    pub rails: Vec<CurveChain>,
    pub operation: Operation,
}

/// One section of a loft. New kinds are appended: a kind's place in the
/// list is how the workers' bytes store it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Section {
    /// A region of a sketch before the loft, as an extrude's regions are
    /// named, of one loop (no holes); it starts at the sketch point
    /// `start`, one of its vertices, or with `None` at its vertex nearest
    /// the previous section's start.
    Region {
        sketch: FeatureId,
        region: RegionRef,
        start: Option<Id>,
    },
    /// A point of a sketch before the loft: only the first or the last
    /// section.
    Point { sketch: FeatureId, point: Id },
}

/// How a loft runs between its sections.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LoftMode {
    /// Through the sections smoothly. A loft of two sections is ruled
    /// whatever its mode.
    #[default]
    Smooth,
    /// Straight between consecutive sections.
    Ruled,
}

impl Section {
    /// The sketch it's of.
    pub fn sketch(&self) -> FeatureId {
        match self {
            Section::Region { sketch, .. } | Section::Point { sketch, .. } => *sketch,
        }
    }

    /// Whether it's a point.
    pub fn is_point(&self) -> bool {
        matches!(self, Section::Point { .. })
    }
}

impl Loft {
    /// The sketches its sections and rails are of, sorted without
    /// repeats: what it uses.
    pub fn sketches(&self) -> Vec<FeatureId> {
        sorted_unique(
            (self.sections.iter().map(Section::sketch))
                .chain(self.rails.iter().map(|rail| rail.sketch)),
        )
    }

    /// The sketches its sections are of, sorted without repeats: those
    /// adding it hides.
    pub fn section_sketches(&self) -> Vec<FeatureId> {
        sorted_unique(self.sections.iter().map(Section::sketch))
    }

    /// Checks what needs only the loft: the section count, each region
    /// and that it has no holes, points only first or last and not every
    /// section one, a closed loft's three or more sections with no point
    /// and no rails, the rail count and each rail's curves' count and
    /// order, and no rail twice. What the sections and rails name is
    /// [`Document::check`](crate::Document::check)'s; their points and
    /// curves in their sketches are [`Loft::check_names`]'. Cheap, for a
    /// panel to run on every view.
    pub fn check_own(&self) -> Result<(), LoftError> {
        let count = self.sections.len();
        if !(2..=MAX_LOFT_SECTIONS).contains(&count) {
            return Err(LoftError::Sections(count));
        }
        for (index, section) in self.sections.iter().enumerate() {
            match section {
                Section::Region { region, .. } => {
                    region
                        .check(f64::from(MAX_COORD))
                        .map_err(LoftError::Region)?;
                    if !region.holes.is_empty() {
                        return Err(LoftError::Holes(index));
                    }
                }
                Section::Point { .. } if index != 0 && index + 1 != count => {
                    return Err(LoftError::PointInside(index));
                }
                Section::Point { .. } => {}
            }
        }
        if self.sections.iter().all(Section::is_point) {
            return Err(LoftError::Points);
        }
        if self.closed {
            if count < 3 {
                return Err(LoftError::ClosedSections(count));
            }
            if self.sections.iter().any(Section::is_point) {
                return Err(LoftError::ClosedPoint);
            }
            if !self.rails.is_empty() {
                return Err(LoftError::ClosedRails);
            }
        }
        if self.rails.len() > MAX_LOFT_RAILS {
            return Err(LoftError::Rails(self.rails.len()));
        }
        for (index, rail) in self.rails.iter().enumerate() {
            let curves = rail.curves.len();
            if !(1..=MAX_RAIL_CURVES).contains(&curves) {
                return Err(LoftError::RailCurves(curves));
            }
            if !rail.sorted() {
                return Err(LoftError::RailOrder);
            }
            if self.rails[..index].contains(rail) {
                return Err(LoftError::RailRepeated);
            }
        }
        Ok(())
    }

    /// Checks what's required of a loft when it's added or edited, but
    /// not of one in a document (a later edit of a sketch may break it,
    /// which regeneration reports): each section's start point or point
    /// is a point of its sketch, and each rail's curves curves of its
    /// sketch, `sketch_of` giving a sketch feature's sketch (`None` for
    /// one the document check refuses anyway).
    pub fn check_names<'a>(
        &self,
        sketch_of: impl Fn(FeatureId) -> Option<&'a Sketch>,
    ) -> Result<(), LoftError> {
        for section in &self.sections {
            let Some(sketch) = sketch_of(section.sketch()) else {
                continue;
            };
            match *section {
                Section::Region {
                    start: Some(start), ..
                } if sketch.point(start).is_none() => return Err(LoftError::Start(start)),
                Section::Point { point, .. } if sketch.point(point).is_none() => {
                    return Err(LoftError::Point(point));
                }
                _ => {}
            }
        }
        for rail in &self.rails {
            if let Some(sketch) = sketch_of(rail.sketch)
                && let Some(missing) = rail.missing(sketch)
            {
                return Err(LoftError::RailCurve(missing));
            }
        }
        Ok(())
    }
}

/// What's wrong with a loft, see
/// [`CheckError::Loft`](crate::CheckError::Loft).
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum LoftError {
    /// It has this many sections: fewer than 2, or over
    /// [`MAX_LOFT_SECTIONS`].
    #[error("lofts {0} sections, not 2 to {MAX_LOFT_SECTIONS}")]
    Sections(usize),
    /// A section's region reference fails its check.
    #[error("{0}")]
    Region(#[source] RegionRefError),
    /// Section `index` (from 0) is a region with holes.
    #[error("its section {} has holes, but a section is one loop", .0.saturating_add(1))]
    Holes(usize),
    /// Section `index` (from 0), neither the first nor the last, is a
    /// point.
    #[error("its section {} is a point, which only the first or last may be", .0.saturating_add(1))]
    PointInside(usize),
    /// Every section is a point.
    #[error("all its sections are points")]
    Points,
    /// It's closed with this many sections, fewer than 3.
    #[error("it's closed with {0} sections, fewer than 3")]
    ClosedSections(usize),
    /// It's closed, and a section is a point.
    #[error("it's closed, but a section is a point")]
    ClosedPoint,
    /// It's closed, and has rails.
    #[error("it's closed, but has rails")]
    ClosedRails,
    /// It has this many rails, over [`MAX_LOFT_RAILS`].
    #[error("it has {0} rails, more than {MAX_LOFT_RAILS}")]
    Rails(usize),
    /// A rail has this many curves: none, or over [`MAX_RAIL_CURVES`].
    #[error("a rail has {0} curves, not 1 to {MAX_RAIL_CURVES}")]
    RailCurves(usize),
    /// A rail's curves aren't sorted, or one is repeated.
    #[error("a rail's curves are out of order or repeated")]
    RailOrder,
    /// A rail is there twice.
    #[error("a rail is there twice")]
    RailRepeated,
    /// A section is of this feature, which isn't a sketch before it.
    #[error("lofts feature {}, which isn't a sketch before it", .0.0)]
    Sketch(FeatureId),
    /// A rail is of this feature, which isn't a sketch before it.
    #[error("a rail is on feature {}, which isn't a sketch before it", .0.0)]
    RailSketch(FeatureId),
    /// A section starts at this point, which its sketch doesn't have.
    #[error("a section starts at point {}, which its sketch doesn't have", .0.get())]
    Start(Id),
    /// A section is this point, which its sketch doesn't have.
    #[error("a section is point {}, which its sketch doesn't have", .0.get())]
    Point(Id),
    /// A rail names this curve, which its sketch doesn't have.
    #[error("a rail names curve {}, which its sketch doesn't have", .0.get())]
    RailCurve(Id),
    /// Its operation's new or excluded bodies are wrong.
    #[error("{0}")]
    Operation(#[source] OperationError),
}

#[cfg(test)]
mod tests;
