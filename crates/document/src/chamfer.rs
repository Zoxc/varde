//! The chamfer feature: edges of a body already made cut off by a flat
//! (or, round a rim, conical) face, set back equal distances along the
//! two faces beside each edge, two different ones, or one distance and
//! the cut's angle.

use std::f64::consts::FRAC_PI_2;
use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::{BodyId, Design, EdgeError, EdgeRef, Extent, FeatureId};

/// The most edges a chamfer (or a fillet) may name.
pub const MAX_BLEND_EDGES: usize = 256;

/// A chamfer: the edges `edges` of one body cut off as `distances` says.
/// The body keeps its id; the new faces are named after the chamfer and
/// the edge each runs along.
///
/// An edge's **first face** is the face of its reference's first key
/// (the lower, [`EdgeRef::faces`]), or with `flip` the second: what
/// [`ChamferSize::Two`]'s first distance and [`ChamferSize::Angle`]'s
/// distance and angle are taken along. An edge grown into along a
/// tangent chain takes the side that shares a face with the picked
/// edge's first (regeneration's to work out).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chamfer {
    /// `1..=`[`MAX_BLEND_EDGES`] edges, all on one body a feature before
    /// it makes, in [`EdgeRef::order`] without repeats. Each is found
    /// on the body as the features before the chamfer leave it.
    pub edges: Vec<EdgeRef>,
    pub distances: ChamferSize,
    /// Whether each edge takes in the edges running on smoothly from it
    /// (its tangent chain, within 1°).
    pub chains: bool,
    /// Whether the edges' first faces are their second keys' faces.
    #[serde(default)]
    pub flip: bool,
}

/// How far a chamfer cuts. New kinds are appended: a kind's place in the
/// list is how the workers' bytes store it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChamferSize {
    /// This far along both faces from the edge, a length as an
    /// extrude's distance ([`Chamfer::distance_ask`]).
    Equal(Value),
    /// The first distance along the first face, the second along the
    /// other, each as [`ChamferSize::Equal`]'s.
    Two(Value, Value),
    /// The distance along the first face (as [`ChamferSize::Equal`]'s),
    /// and the cut's angle to that face, above 0 and under 90°
    /// ([`Chamfer::angle_ask`]).
    Angle(Value, Value),
}

impl ChamferSize {
    /// What it's called, as the panel's choices name it: "Equal".
    pub fn name(&self) -> &'static str {
        match self {
            ChamferSize::Equal(_) => "Equal",
            ChamferSize::Two(..) => "Two distances",
            ChamferSize::Angle(..) => "Distance and angle",
        }
    }
}

impl Chamfer {
    /// What a distance is checked against in `design`: a length as an
    /// extrude's distance ([`Extent::ask`]).
    pub fn distance_ask(design: &Design) -> Ask {
        Extent::ask(design)
    }

    /// What an angle is checked against in `design`: above zero and
    /// under a right angle, bare numbers in degrees.
    pub fn angle_ask(design: &Design) -> Ask {
        Ask::angle(design.units, FRAC_PI_2).positive().under_max()
    }

    /// The body its edges are on: the first edge's (all are on one in a
    /// checked document). `None` with no edges.
    pub fn body(&self) -> Option<BodyId> {
        self.edges.first().map(|edge| edge.body)
    }

    /// The bodies it names, which it depends on: its body.
    pub fn bodies(&self) -> Vec<BodyId> {
        self.body().into_iter().collect()
    }

    /// Checks what needs only the chamfer and `design`: the edge count,
    /// order and body, each edge's own parts, and its values. What the
    /// body and the faces name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self, design: &Design) -> Result<(), ChamferError> {
        let count = self.edges.len();
        if !(1..=MAX_BLEND_EDGES).contains(&count) {
            return Err(ChamferError::Edges(count));
        }
        for edge in &self.edges {
            edge.check_own().map_err(ChamferError::Edge)?;
        }
        if !(self.edges.windows(2)).all(|pair| pair[0].order(&pair[1]).is_lt()) {
            return Err(ChamferError::EdgeOrder);
        }
        if self
            .edges
            .iter()
            .any(|edge| edge.body != self.edges[0].body)
        {
            return Err(ChamferError::Bodies);
        }
        let distance = Chamfer::distance_ask(design);
        let angle = Chamfer::angle_ask(design);
        let (first, second) = match &self.distances {
            ChamferSize::Equal(d) => (d, None),
            ChamferSize::Two(a, b) => (a, Some((b, false))),
            ChamferSize::Angle(d, a) => (d, Some((a, true))),
        };
        first.check(&distance).map_err(|_| ChamferError::Distance)?;
        match second {
            Some((value, false)) => value.check(&distance).map_err(|_| ChamferError::Distance)?,
            Some((value, true)) => value.check(&angle).map_err(|_| ChamferError::Angle)?,
            None => {}
        }
        Ok(())
    }

    /// Its typed values and what each is checked against in `design`.
    pub(crate) fn values_mut(&mut self, design: &Design) -> Vec<(&mut Value, Ask)> {
        let distance = Chamfer::distance_ask(design);
        match &mut self.distances {
            ChamferSize::Equal(d) => vec![(d, distance)],
            ChamferSize::Two(a, b) => vec![(a, distance), (b, distance)],
            ChamferSize::Angle(d, a) => vec![(d, distance), (a, Chamfer::angle_ask(design))],
        }
    }
}

/// What's wrong with a chamfer, see
/// [`CheckError::Chamfer`](crate::CheckError::Chamfer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChamferError {
    /// It names this many edges: none, or over [`MAX_BLEND_EDGES`].
    Edges(usize),
    /// Its edges aren't in [`EdgeRef::order`], or one is repeated.
    EdgeOrder,
    /// An edge fails its own check ([`EdgeRef::check_own`]).
    Edge(EdgeError),
    /// Its edges are on more than one body.
    Bodies,
    /// A distance's expression doesn't give its value, or the value
    /// isn't a length [`Chamfer::distance_ask`] takes.
    Distance,
    /// Its angle's expression doesn't give its value, or the value isn't
    /// an angle [`Chamfer::angle_ask`] takes.
    Angle,
    /// Its edges are on this body, which isn't there or which no feature
    /// before it makes.
    Body(BodyId),
    /// A key of an edge's faces names this feature, which is the chamfer
    /// itself or comes after it, or isn't there and has an id a feature
    /// made later could take.
    RefMaker(FeatureId),
}

impl fmt::Display for ChamferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChamferError::Edges(count) => {
                write!(f, "names {count} edges, not 1 to {MAX_BLEND_EDGES}")
            }
            ChamferError::EdgeOrder => f.write_str("its edges are out of order or repeated"),
            ChamferError::Edge(why) => why.fmt(f),
            ChamferError::Bodies => f.write_str("its edges are on more than one body"),
            ChamferError::Distance => f.write_str(
                "a distance's expression doesn't give its value, or it isn't a length it takes",
            ),
            ChamferError::Angle => f.write_str(
                "its angle's expression doesn't give its value, or it isn't above 0° and under \
                 90°",
            ),
            ChamferError::Body(body) => write!(
                f,
                "chamfers body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            ChamferError::RefMaker(feature) => write!(
                f,
                "an edge is on a face made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl std::error::Error for ChamferError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ChamferError::Edge(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
