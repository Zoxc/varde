//! The fillet feature: edges of a body already made rounded off by a
//! face of one radius, tangent to the two faces beside each edge.

use std::fmt;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::{BlendEdgesError, BodyId, Design, EdgeRef, Extent, check_blend_edges_own};

/// A fillet: the edges `edges` of one body rounded off to `radius`. The
/// body keeps its id; the new faces are named after the fillet and the
/// edge each runs along. Convex edges lose material, concave ones gain
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fillet {
    /// `1..=`[`MAX_BLEND_EDGES`](crate::MAX_BLEND_EDGES) edges, all on
    /// one body a feature before it makes, in [`EdgeRef::order`] without
    /// repeats. Each is found on the body as the features before the
    /// fillet leave it.
    pub edges: Vec<EdgeRef>,
    /// A length as an extrude's distance ([`Fillet::radius_ask`]).
    pub radius: Value,
    /// Whether each edge takes in the edges running on smoothly from it
    /// (its tangent chain, within 1°).
    pub chains: bool,
}

impl Fillet {
    /// What its radius is checked against in `design`: a length as an
    /// extrude's distance ([`Extent::ask`]).
    pub fn radius_ask(design: &Design) -> Ask {
        Extent::ask(design)
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

    /// Checks what needs only the fillet and `design`: its edges'
    /// count, order, body and own parts ([`check_blend_edges_own`]), and
    /// its radius. What the body and the faces name is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self, design: &Design) -> Result<(), FilletError> {
        check_blend_edges_own(&self.edges).map_err(FilletError::Edges)?;
        (self.radius)
            .check(&Fillet::radius_ask(design))
            .map_err(|_| FilletError::Radius)
    }

    /// Its typed values and what each is checked against in `design`.
    pub(crate) fn values_mut(&mut self, design: &Design) -> Vec<(&mut Value, Ask)> {
        vec![(&mut self.radius, Fillet::radius_ask(design))]
    }
}

/// What's wrong with a fillet, see
/// [`CheckError::Fillet`](crate::CheckError::Fillet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilletError {
    /// Its edges are wrong, see [`BlendEdgesError`].
    Edges(BlendEdgesError),
    /// Its radius's expression doesn't give its value, or the value
    /// isn't a length [`Fillet::radius_ask`] takes.
    Radius,
}

impl fmt::Display for FilletError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilletError::Edges(why) => why.fmt(f),
            FilletError::Radius => f.write_str(
                "its radius's expression doesn't give its value, or it isn't a length it takes",
            ),
        }
    }
}

impl std::error::Error for FilletError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FilletError::Edges(why) => Some(why),
            FilletError::Radius => None,
        }
    }
}

#[cfg(test)]
mod tests;
