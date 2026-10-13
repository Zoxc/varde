//! The edges a chamfer or a fillet runs along: how many there may be and
//! the checks both kinds share.
//!
//! A blend names edges and faces. A face stands for the edges around it;
//! an edge named that is also around a face named is left out (an
//! exclusion), any other edge named is taken.

use crate::{BodyId, Document, EdgeError, EdgeRef, FaceRef, FeatureId, NearError};

/// The most edges, and the most faces, a chamfer or a fillet may name.
pub const MAX_BLEND_EDGES: usize = 256;

/// Checks what needs only a chamfer's or fillet's `edges` and `faces`:
/// at least one of either and at most [`MAX_BLEND_EDGES`] of each, each
/// one's own parts ([`EdgeRef::check_own`], [`FaceRef::check_own`]),
/// [`EdgeRef::order`] and [`FaceRef::order`] without repeats, and one
/// body. What they name is [`Document::check_blend_edges`]'s.
pub fn check_blend_edges_own(edges: &[EdgeRef], faces: &[FaceRef]) -> Result<(), BlendEdgesError> {
    let count = edges.len().saturating_add(faces.len());
    if count == 0 || edges.len() > MAX_BLEND_EDGES || faces.len() > MAX_BLEND_EDGES {
        return Err(BlendEdgesError::Count(count));
    }
    for edge in edges {
        edge.check_own().map_err(BlendEdgesError::Edge)?;
    }
    for face in faces {
        face.check_own().map_err(BlendEdgesError::Face)?;
    }
    if !(edges.windows(2)).all(|pair| pair[0].order(&pair[1]).is_lt())
        || !(faces.windows(2)).all(|pair| pair[0].order(&pair[1]).is_lt())
    {
        return Err(BlendEdgesError::Order);
    }
    let body = blend_body(edges, faces);
    if (edges.iter().map(|edge| edge.body))
        .chain(faces.iter().map(|face| face.body))
        .any(|other| Some(other) != body)
    {
        return Err(BlendEdgesError::Bodies);
    }
    Ok(())
}

/// The body a blend's `edges` and `faces` are on: the first edge's, else
/// the first face's (all are on one in a checked document). `None` with
/// neither.
pub fn blend_body(edges: &[EdgeRef], faces: &[FaceRef]) -> Option<BodyId> {
    (edges.first().map(|edge| edge.body)).or_else(|| faces.first().map(|face| face.body))
}

/// Whether `edge` is around one of `faces`, by their keys: an edge a
/// blend names that is also around a face it names is left out.
pub fn blend_excludes(edge: &EdgeRef, faces: &[FaceRef]) -> bool {
    (faces.iter()).any(|face| face.body == edge.body && edge.faces.contains(&face.key))
}

impl Document {
    /// Checks what `edges` and `faces` name as the edges of a chamfer or a fillet at
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
        faces: &[FaceRef],
    ) -> Result<(), BlendEdgesError> {
        let named = (edges.iter().map(EdgeRef::on_body)).chain(faces.iter().map(FaceRef::on_body));
        (self.all_refer_before(index, named))
            .map_err(|late| late.or(BlendEdgesError::Body, BlendEdgesError::RefMaker))
    }
}

/// What's wrong with the edges of a chamfer or a fillet, see
/// [`check_blend_edges_own`] and [`Document::check_blend_edges`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum BlendEdgesError {
    /// It names this many edges and faces: none, or over
    /// [`MAX_BLEND_EDGES`] edges or faces.
    #[error("names {0} edges and faces, not 1 to {MAX_BLEND_EDGES} of each")]
    Count(usize),
    /// Its edges aren't in [`EdgeRef::order`], or its faces in
    /// [`FaceRef::order`], or one is repeated.
    #[error("its edges or faces are out of order or repeated")]
    Order,
    /// An edge fails its own check ([`EdgeRef::check_own`]).
    #[error("{0}")]
    Edge(#[source] EdgeError),
    /// A face fails its own check ([`FaceRef::check_own`]).
    #[error("{0}")]
    Face(#[source] NearError),
    /// Its edges are on more than one body.
    #[error("its edges are on more than one body")]
    Bodies,
    /// Its edges are on this body, which isn't there or which no feature
    /// before it makes.
    #[error("its edges are on body {}, which isn't there or no earlier feature makes", .0.0)]
    Body(BodyId),
    /// A key of an edge's faces names this feature, which is the feature
    /// itself or comes after it, or isn't there and has an id a feature
    /// made later could take.
    #[error("an edge is on a face made by feature {}, which doesn't come before it", .0.0)]
    RefMaker(FeatureId),
}
