//! The loft feature: a solid through two or more sections in order, each
//! a region of one loop of an earlier sketch or (first or last only) a
//! point of one, ruled or smooth, open or closed, following up to four
//! rails; it makes a new body or joins, cuts or intersects bodies as an
//! extrude does.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_sketch::{Id, RegionRef, RegionRefError, Sketch};

use crate::{BodyId, CurveChain, FeatureId, MAX_COORD, Operation};

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
        let mut sketches: Vec<FeatureId> = (self.sections.iter().map(Section::sketch))
            .chain(self.rails.iter().map(|rail| rail.sketch))
            .collect();
        sketches.sort_unstable();
        sketches.dedup();
        sketches
    }

    /// The sketches its sections are of, sorted without repeats: those
    /// adding it hides.
    pub fn section_sketches(&self) -> Vec<FeatureId> {
        let mut sketches: Vec<FeatureId> = self.sections.iter().map(Section::sketch).collect();
        sketches.sort_unstable();
        sketches.dedup();
        sketches
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoftError {
    /// It has this many sections: fewer than 2, or over
    /// [`MAX_LOFT_SECTIONS`].
    Sections(usize),
    /// A section's region reference fails its check.
    Region(RegionRefError),
    /// Section `index` (from 0) is a region with holes.
    Holes(usize),
    /// Section `index` (from 0), neither the first nor the last, is a
    /// point.
    PointInside(usize),
    /// Every section is a point.
    Points,
    /// It's closed with this many sections, fewer than 3.
    ClosedSections(usize),
    /// It's closed, and a section is a point.
    ClosedPoint,
    /// It's closed, and has rails.
    ClosedRails,
    /// It has this many rails, over [`MAX_LOFT_RAILS`].
    Rails(usize),
    /// A rail has this many curves: none, or over [`MAX_RAIL_CURVES`].
    RailCurves(usize),
    /// A rail's curves aren't sorted, or one is repeated.
    RailOrder,
    /// A rail is there twice.
    RailRepeated,
    /// A section is of this feature, which isn't a sketch before it.
    Sketch(FeatureId),
    /// A rail is of this feature, which isn't a sketch before it.
    RailSketch(FeatureId),
    /// A section starts at this point, which its sketch doesn't have.
    Start(Id),
    /// A section is this point, which its sketch doesn't have.
    Point(Id),
    /// A rail names this curve, which its sketch doesn't have.
    RailCurve(Id),
    /// The body it makes, this one, isn't there, or names another maker.
    NewBody(BodyId),
    /// It excludes this body, which no feature before it makes.
    Excluded(BodyId),
    /// Its excluded bodies aren't sorted, or one is repeated.
    ExcludedOrder,
}

impl fmt::Display for LoftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ordinal = |index: &usize| index.saturating_add(1);
        match self {
            LoftError::Sections(count) => {
                write!(f, "lofts {count} sections, not 2 to {MAX_LOFT_SECTIONS}")
            }
            LoftError::Region(why) => why.fmt(f),
            LoftError::Holes(index) => write!(
                f,
                "its section {} has holes, but a section is one loop",
                ordinal(index)
            ),
            LoftError::PointInside(index) => write!(
                f,
                "its section {} is a point, which only the first or last may be",
                ordinal(index)
            ),
            LoftError::Points => f.write_str("all its sections are points"),
            LoftError::ClosedSections(count) => {
                write!(f, "it's closed with {count} sections, fewer than 3")
            }
            LoftError::ClosedPoint => f.write_str("it's closed, but a section is a point"),
            LoftError::ClosedRails => f.write_str("it's closed, but has rails"),
            LoftError::Rails(count) => {
                write!(f, "it has {count} rails, more than {MAX_LOFT_RAILS}")
            }
            LoftError::RailCurves(count) => {
                write!(f, "a rail has {count} curves, not 1 to {MAX_RAIL_CURVES}")
            }
            LoftError::RailOrder => f.write_str("a rail's curves are out of order or repeated"),
            LoftError::RailRepeated => f.write_str("a rail is there twice"),
            LoftError::Sketch(sketch) => write!(
                f,
                "lofts feature {}, which isn't a sketch before it",
                sketch.0
            ),
            LoftError::RailSketch(sketch) => write!(
                f,
                "a rail is on feature {}, which isn't a sketch before it",
                sketch.0
            ),
            LoftError::Start(id) => write!(
                f,
                "a section starts at point {}, which its sketch doesn't have",
                id.get()
            ),
            LoftError::Point(id) => write!(
                f,
                "a section is point {}, which its sketch doesn't have",
                id.get()
            ),
            LoftError::RailCurve(id) => write!(
                f,
                "a rail names curve {}, which its sketch doesn't have",
                id.get()
            ),
            LoftError::NewBody(body) => write!(
                f,
                "makes body {}, which isn't there or has another maker",
                body.0
            ),
            LoftError::Excluded(body) => write!(
                f,
                "excludes body {}, which no earlier feature makes",
                body.0
            ),
            LoftError::ExcludedOrder => {
                f.write_str("its excluded bodies are out of order or repeated")
            }
        }
    }
}

impl std::error::Error for LoftError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoftError::Region(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
