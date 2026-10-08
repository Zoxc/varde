//! The edges a chamfer or a fillet runs along: how many there may be and
//! the checks both kinds share.

use std::fmt;

use crate::{BodyId, Document, EdgeError, EdgeRef, FeatureId};

/// The most edges a chamfer or a fillet may name.
pub const MAX_BLEND_EDGES: usize = 256;

/// Checks what needs only a chamfer's or fillet's `edges`: their count
/// (`1..=`[`MAX_BLEND_EDGES`]), each edge's own parts
/// ([`EdgeRef::check_own`]), [`EdgeRef::order`] without repeats, and one
/// body. What they name is [`Document::check_blend_edges`]'s.
pub fn check_blend_edges_own(edges: &[EdgeRef]) -> Result<(), BlendEdgesError> {
    let count = edges.len();
    if !(1..=MAX_BLEND_EDGES).contains(&count) {
        return Err(BlendEdgesError::Count(count));
    }
    for edge in edges {
        edge.check_own().map_err(BlendEdgesError::Edge)?;
    }
    if !(edges.windows(2)).all(|pair| pair[0].order(&pair[1]).is_lt()) {
        return Err(BlendEdgesError::Order);
    }
    if edges.iter().any(|edge| edge.body != edges[0].body) {
        return Err(BlendEdgesError::Bodies);
    }
    Ok(())
}

impl Document {
    /// Checks what `edges` name as the edges of a chamfer or a fillet at
    /// feature `index` (at the end for a new one, the count of features),
    /// as [`Document::check`] has it: each edge's body there and made by
    /// a feature before it (depended on, as a combine's bodies), and its
    /// faces' makers before it, or not there with ids no later feature
    /// can take, as a sketch's face's. For a panel keeping what it sets
    /// up one the document takes; their own parts are
    /// [`check_blend_edges_own`]'s.
    pub fn check_blend_edges(
        &self,
        index: usize,
        edges: &[EdgeRef],
    ) -> Result<(), BlendEdgesError> {
        for edge in edges {
            if !self.body_before(index, edge.body) {
                return Err(BlendEdgesError::Body(edge.body));
            }
            if let Some(&maker) = (edge.makers().iter()).find(|&&m| !self.maker_before(index, m)) {
                return Err(BlendEdgesError::RefMaker(maker));
            }
        }
        Ok(())
    }
}

/// What's wrong with the edges of a chamfer or a fillet, see
/// [`check_blend_edges_own`] and [`Document::check_blend_edges`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlendEdgesError {
    /// It names this many edges: none, or over [`MAX_BLEND_EDGES`].
    Count(usize),
    /// Its edges aren't in [`EdgeRef::order`], or one is repeated.
    Order,
    /// An edge fails its own check ([`EdgeRef::check_own`]).
    Edge(EdgeError),
    /// Its edges are on more than one body.
    Bodies,
    /// Its edges are on this body, which isn't there or which no feature
    /// before it makes.
    Body(BodyId),
    /// A key of an edge's faces names this feature, which is the feature
    /// itself or comes after it, or isn't there and has an id a feature
    /// made later could take.
    RefMaker(FeatureId),
}

impl fmt::Display for BlendEdgesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlendEdgesError::Count(count) => {
                write!(f, "names {count} edges, not 1 to {MAX_BLEND_EDGES}")
            }
            BlendEdgesError::Order => f.write_str("its edges are out of order or repeated"),
            BlendEdgesError::Edge(why) => why.fmt(f),
            BlendEdgesError::Bodies => f.write_str("its edges are on more than one body"),
            BlendEdgesError::Body(body) => write!(
                f,
                "its edges are on body {}, which isn't there or no earlier feature makes",
                body.0
            ),
            BlendEdgesError::RefMaker(feature) => write!(
                f,
                "an edge is on a face made by feature {}, which doesn't come before it",
                feature.0
            ),
        }
    }
}

impl std::error::Error for BlendEdgesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BlendEdgesError::Edge(why) => Some(why),
            _ => None,
        }
    }
}
