//! The sweep feature: regions of an earlier sketch moved along a path
//! into a solid, which makes a new body or joins, cuts or intersects
//! bodies already there, as an extrude's does. The path is chains of
//! other sketches' curves and of model edges, joined end to end, or a
//! helix.

use std::f64::consts::TAU;
use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};
use varde_sketch::{Id, RegionRef, RegionRefError, Sketch};

use crate::{
    AxisRef, BodyId, Design, EdgeError, EdgeRef, Extent, FeatureId, MAX_COORD, MotionError,
    Operation,
};

/// The most regions one sweep may take, as an extrude's.
pub const MAX_SWEEP_REGIONS: usize = 256;

/// The most parts a sweep's path may have.
pub const MAX_PATH_PARTS: usize = 64;

/// The most curves and edges a sweep's path may name, all its parts
/// together.
pub const MAX_PATH_CURVES: usize = 1024;

/// The fewest turns a helix may make.
pub const MIN_HELIX_TURNS: f64 = 1e-3;

/// The most turns a helix may make.
pub const MAX_HELIX_TURNS: f64 = 1000.0;

/// The most turns a sweep's twist may make, either way.
pub const MAX_TWIST_TURNS: f64 = 8.0;

/// A sweep: the regions of sketch `sketch` moved along `path`, carried
/// as `orientation` says and turned by `twist` about the path over its
/// length, and what it does with the solid it makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sweep {
    /// An earlier sketch feature: the profile's.
    pub sketch: FeatureId,
    /// The regions, `1..=`[`MAX_SWEEP_REGIONS`] of them, made as an
    /// extrude's are. Adjacent ones are merged before sweeping.
    pub regions: Vec<RegionRef>,
    pub path: PathRef,
    /// [`Orientation::FollowPath`] with a helix.
    pub orientation: Orientation,
    /// An angle the section turns about the path over its whole length
    /// ([`Sweep::twist_ask`]: within [`MAX_TWIST_TURNS`] either way); none
    /// with a helix.
    pub twist: Option<Value>,
    pub operation: Operation,
}

/// How a sweep's section is carried along its path. New kinds are
/// appended: a kind's place in the list is how the workers' bytes store
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Orientation {
    /// Turned with the path, square to it, without spinning about it.
    #[default]
    FollowPath,
    /// Only moved along the path, never turned.
    Keep,
}

/// What a sweep's path is. New kinds are appended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PathRef {
    /// `1..=`[`MAX_PATH_PARTS`] parts, which regenerating joins end to
    /// end into one chain, in the order their ends meet, from the end on
    /// the profile's plane: the order here doesn't matter.
    Chain(Vec<PathPart>),
    /// A helix, alone.
    Helix(Helix),
}

/// One part of a sweep's path. New kinds are appended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PathPart {
    /// A chain of one sketch's curves.
    Curves(CurveChain),
    /// A chain of edges of one body, as the features before the sweep
    /// leave it: `1..` of them, in [`EdgeRef::order`] without repeats,
    /// each taking in its tangent chain with `tangent`.
    Edges { edges: Vec<EdgeRef>, tangent: bool },
}

/// Curves of a sketch that make one chain, end to end: `1..` of them,
/// sorted without repeats. Regenerating orders them along the chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveChain {
    /// A sketch feature before the feature naming it.
    pub sketch: FeatureId,
    pub curves: Vec<Id>,
}

/// A helix a sweep carries its profile round: about `axis`, `pitch` a
/// turn along it, for `turns` turns, counter-clockwise seen from the tip
/// of the axis's direction unless `left_handed`. The profile's plane
/// holds the axis; each of its points runs on a helix of its own, so the
/// helix needs no radius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Helix {
    /// Found as a move's turn's axis is, as the features before the
    /// sweep leave its body.
    pub axis: AxisRef,
    /// A length as an extrude's distance ([`Sweep::pitch_ask`]).
    pub pitch: Value,
    /// [`MIN_HELIX_TURNS`]`..=`[`MAX_HELIX_TURNS`] ([`Sweep::turns_ask`]).
    pub turns: Value,
    pub left_handed: bool,
    /// Reverses the axis's direction, so the helix climbs the other way.
    pub flip: bool,
}

impl PathPart {
    /// How many curves or edges it names.
    pub fn len(&self) -> usize {
        match self {
            PathPart::Curves(chain) => chain.curves.len(),
            PathPart::Edges { edges, .. } => edges.len(),
        }
    }

    /// Whether it names none (which no checked part does).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Sweep {
    /// What its twist is checked against in `design`: an angle within
    /// [`MAX_TWIST_TURNS`] turns either way, bare numbers in degrees.
    pub fn twist_ask(design: &Design) -> Ask {
        Ask::angle(design.units, MAX_TWIST_TURNS * TAU)
    }

    /// What a helix's pitch is checked against in `design`: a length as
    /// an extrude's distance ([`Extent::ask`]).
    pub fn pitch_ask(design: &Design) -> Ask {
        Extent::ask(design)
    }

    /// What a helix's turns are checked against in `design`: a number
    /// from [`MIN_HELIX_TURNS`] to [`MAX_HELIX_TURNS`].
    pub fn turns_ask(design: &Design) -> Ask {
        Ask::number(design.units, MAX_HELIX_TURNS)
            .positive()
            .at_least(MIN_HELIX_TURNS)
    }

    /// The sketches its path's parts name, sorted without repeats.
    pub fn path_sketches(&self) -> Vec<FeatureId> {
        let mut sketches: Vec<FeatureId> = match &self.path {
            PathRef::Chain(parts) => (parts.iter())
                .filter_map(|part| match part {
                    PathPart::Curves(chain) => Some(chain.sketch),
                    PathPart::Edges { .. } => None,
                })
                .collect(),
            PathRef::Helix(_) => Vec::new(),
        };
        sketches.sort_unstable();
        sketches.dedup();
        sketches
    }

    /// The bodies it names, which it depends on: those its path's edges
    /// are on, and its helix's axis's, sorted without repeats.
    pub fn bodies(&self) -> Vec<BodyId> {
        let mut bodies: Vec<BodyId> = match &self.path {
            PathRef::Chain(parts) => (parts.iter())
                .filter_map(|part| match part {
                    PathPart::Edges { edges, .. } => edges.first().map(|edge| edge.body),
                    PathPart::Curves(_) => None,
                })
                .collect(),
            PathRef::Helix(helix) => helix.axis.refers().map(|r| r.body()).into_iter().collect(),
        };
        bodies.sort_unstable();
        bodies.dedup();
        bodies
    }

    /// Checks what needs only the sweep and `design`: the region count
    /// and each region, the path's parts (counts, order, each edge's own
    /// parts, one body to each edge part), a helix's axis's own parts,
    /// values and options, and the twist. What the sketches, bodies and
    /// faces name is [`Document::check`](crate::Document::check)'s, and
    /// the curves [`Sweep::check_curves`]'. Cheap, for a panel to run on
    /// every view.
    pub fn check_own(&self, design: &Design) -> Result<(), SweepError> {
        let count = self.regions.len();
        if !(1..=MAX_SWEEP_REGIONS).contains(&count) {
            return Err(SweepError::Regions(count));
        }
        for region in &self.regions {
            region
                .check(f64::from(MAX_COORD))
                .map_err(SweepError::Region)?;
        }
        match &self.path {
            PathRef::Chain(parts) => check_parts(parts)?,
            PathRef::Helix(helix) => {
                if self.orientation != Orientation::FollowPath || self.twist.is_some() {
                    return Err(SweepError::HelixOptions);
                }
                if let Some(referred) = helix.axis.refers() {
                    referred.check_own().map_err(SweepError::Axis)?;
                }
                (helix.pitch)
                    .check(&Sweep::pitch_ask(design))
                    .map_err(|_| SweepError::Pitch)?;
                (helix.turns)
                    .check(&Sweep::turns_ask(design))
                    .map_err(|_| SweepError::Turns)?;
            }
        }
        if let Some(twist) = &self.twist {
            twist
                .check(&Sweep::twist_ask(design))
                .map_err(|_| SweepError::Twist)?;
        }
        Ok(())
    }

    /// Checks what's required of a sweep when it's added or edited, but
    /// not of one in a document (a later edit of a sketch may break it,
    /// which regeneration reports): every curve of a path part is a curve
    /// of its sketch, as `sketch_of` gives it (a part whose sketch isn't
    /// one is [`Document::check`](crate::Document::check)'s).
    pub fn check_curves<'a>(
        &self,
        sketch_of: impl Fn(FeatureId) -> Option<&'a Sketch>,
    ) -> Result<(), SweepError> {
        let PathRef::Chain(parts) = &self.path else {
            return Ok(());
        };
        for part in parts {
            if let PathPart::Curves(chain) = part
                && let Some(sketch) = sketch_of(chain.sketch)
                && let Some(&missing) =
                    (chain.curves.iter()).find(|&&id| sketch.curve(id).is_none())
            {
                return Err(SweepError::Curve(missing));
            }
        }
        Ok(())
    }

    /// Its typed values and what each is checked against in `design`.
    pub(crate) fn values_mut(&mut self, design: &Design) -> Vec<(&mut Value, Ask)> {
        let mut values = Vec::new();
        if let Some(twist) = &mut self.twist {
            values.push((twist, Sweep::twist_ask(design)));
        }
        if let PathRef::Helix(helix) = &mut self.path {
            values.push((&mut helix.pitch, Sweep::pitch_ask(design)));
            values.push((&mut helix.turns, Sweep::turns_ask(design)));
        }
        values
    }
}

/// Checks a chain path's parts on their own, see [`Sweep::check_own`].
fn check_parts(parts: &[PathPart]) -> Result<(), SweepError> {
    let count = parts.len();
    if !(1..=MAX_PATH_PARTS).contains(&count) {
        return Err(SweepError::Parts(count));
    }
    // Each part's length is at most what a file holds, and there are at
    // most `MAX_PATH_PARTS` of them, but the sum is taken saturating.
    let mut total: usize = 0;
    for part in parts {
        let len = part.len();
        if len == 0 {
            return Err(SweepError::EmptyPart);
        }
        total = total.saturating_add(len);
        if total > MAX_PATH_CURVES {
            return Err(SweepError::PathCurves(total));
        }
    }
    for part in parts {
        match part {
            PathPart::Curves(chain) => {
                if !(chain.curves.windows(2)).all(|pair| pair[0] < pair[1]) {
                    return Err(SweepError::CurveOrder);
                }
            }
            PathPart::Edges { edges, .. } => {
                for edge in edges {
                    edge.check_own().map_err(SweepError::Edge)?;
                }
                if !(edges.windows(2)).all(|pair| pair[0].order(&pair[1]).is_lt()) {
                    return Err(SweepError::EdgeOrder);
                }
                if edges.iter().any(|edge| edge.body != edges[0].body) {
                    return Err(SweepError::EdgeBodies);
                }
            }
        }
    }
    Ok(())
}

/// What's wrong with a sweep, see
/// [`CheckError::Sweep`](crate::CheckError::Sweep).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SweepError {
    /// Its profile's sketch, this feature, isn't a sketch feature before
    /// it.
    Sketch(FeatureId),
    /// It takes this many regions: none, or over [`MAX_SWEEP_REGIONS`].
    Regions(usize),
    /// A region reference fails its check.
    Region(RegionRefError),
    /// Its path has this many parts: none, or over [`MAX_PATH_PARTS`].
    Parts(usize),
    /// A part of its path names no curve or edge.
    EmptyPart,
    /// Its path names at least this many curves and edges, over
    /// [`MAX_PATH_CURVES`].
    PathCurves(usize),
    /// A part's curves aren't sorted, or one is repeated.
    CurveOrder,
    /// A part's edges aren't in [`EdgeRef::order`], or one is repeated.
    EdgeOrder,
    /// An edge fails its own check ([`EdgeRef::check_own`]).
    Edge(EdgeError),
    /// A part's edges are on more than one body.
    EdgeBodies,
    /// A part's sketch, this feature, isn't a sketch feature before it.
    PathSketch(FeatureId),
    /// A part is a chain of the profile's own sketch: a path in the
    /// profile's plane can't be square to it.
    OwnSketch,
    /// A part's curve, this one, isn't a curve of its sketch (when added
    /// or set, see [`Sweep::check_curves`]).
    Curve(Id),
    /// An edge part's body, this one, isn't there or no feature before it
    /// makes it.
    EdgeBody(BodyId),
    /// A key of an edge's faces names this feature, which is the sweep or
    /// comes after it, or isn't there and has an id a feature made later
    /// could take.
    EdgeMaker(FeatureId),
    /// A helix sweep keeps its orientation or has a twist.
    HelixOptions,
    /// Its helix's axis fails its own check, as a move's would.
    Axis(MotionError),
    /// Its helix's axis's body, this one, isn't there or no feature
    /// before it makes it.
    AxisBody(BodyId),
    /// A key of its helix's axis's faces names this feature, which is the
    /// sweep or comes after it, or isn't there and has an id a feature
    /// made later could take.
    AxisMaker(FeatureId),
    /// Its helix's pitch's expression doesn't give its value, or the
    /// value isn't a length [`Sweep::pitch_ask`] takes.
    Pitch,
    /// Its helix's turns' expression doesn't give its value, or the value
    /// isn't one [`Sweep::turns_ask`] takes.
    Turns,
    /// Its twist's expression doesn't give its value, or the value isn't
    /// an angle [`Sweep::twist_ask`] takes.
    Twist,
    /// The body it makes, this one, isn't there, or names another maker.
    NewBody(BodyId),
    /// It excludes this body, which no feature before it makes.
    Excluded(BodyId),
    /// Its excluded bodies aren't sorted, or one is repeated.
    ExcludedOrder,
}

impl fmt::Display for SweepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SweepError::Sketch(sketch) => write!(
                f,
                "sweeps feature {}, which isn't a sketch before it",
                sketch.0
            ),
            SweepError::Regions(count) => {
                write!(f, "sweeps {count} regions, not 1 to {MAX_SWEEP_REGIONS}")
            }
            SweepError::Region(why) => why.fmt(f),
            SweepError::Parts(count) => {
                write!(f, "its path has {count} parts, not 1 to {MAX_PATH_PARTS}")
            }
            SweepError::EmptyPart => f.write_str("a part of its path names no curve or edge"),
            SweepError::PathCurves(count) => write!(
                f,
                "its path names {count} curves and edges, more than {MAX_PATH_CURVES}"
            ),
            SweepError::CurveOrder => {
                f.write_str("a part of its path has its curves out of order or repeated")
            }
            SweepError::EdgeOrder => {
                f.write_str("a part of its path has its edges out of order or repeated")
            }
            SweepError::Edge(why) => write!(f, "its path: {why}"),
            SweepError::EdgeBodies => f.write_str("a part of its path has edges on several bodies"),
            SweepError::PathSketch(sketch) => write!(
                f,
                "its path runs along feature {}, which isn't a sketch before it",
                sketch.0
            ),
            SweepError::OwnSketch => {
                f.write_str("its path runs along curves of its profile's own sketch")
            }
            SweepError::Curve(id) => {
                write!(f, "its path's curve {id} isn't a curve of its sketch")
            }
            SweepError::EdgeBody(body) => write!(
                f,
                "its path runs along edges of body {}, which isn't there or no earlier \
                 feature makes",
                body.0
            ),
            SweepError::EdgeMaker(feature) => write!(
                f,
                "its path runs along an edge of a face made by feature {}, which doesn't \
                 come before it",
                feature.0
            ),
            SweepError::HelixOptions => {
                f.write_str("a helix sweep follows its path and has no twist")
            }
            SweepError::Axis(why) => write!(f, "its helix: {why}"),
            SweepError::AxisBody(body) => write!(
                f,
                "its helix's axis is on body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            SweepError::AxisMaker(feature) => write!(
                f,
                "its helix's axis is on a face made by feature {}, which doesn't come before it",
                feature.0
            ),
            SweepError::Pitch => f.write_str(
                "its pitch's expression doesn't give its value, or it isn't a length it takes",
            ),
            SweepError::Turns => write!(
                f,
                "its turns' expression doesn't give its value, or it isn't from \
                 {MIN_HELIX_TURNS} to {MAX_HELIX_TURNS}"
            ),
            SweepError::Twist => write!(
                f,
                "its twist's expression doesn't give its value, or it isn't an angle within \
                 {MAX_TWIST_TURNS} turns"
            ),
            SweepError::NewBody(body) => write!(
                f,
                "makes body {}, which isn't there or has another maker",
                body.0
            ),
            SweepError::Excluded(body) => write!(
                f,
                "excludes body {}, which no earlier feature makes",
                body.0
            ),
            SweepError::ExcludedOrder => {
                f.write_str("its excluded bodies are out of order or repeated")
            }
        }
    }
}

impl std::error::Error for SweepError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SweepError::Region(why) => Some(why),
            SweepError::Edge(why) => Some(why),
            SweepError::Axis(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
