//! References to the edges of bodies, as features that build on a model
//! edge store them.

use std::fmt;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use varde_kernel::mesh::FaceKey;

use crate::{BodyId, FeatureId, MAX_COORD};

/// An edge of a body, as picked: the body, the keys of the faces either
/// side of it (the kernel's names for them, from the features that made
/// them), the lower first, and the picked point, which chooses among
/// several edges between faces of those keys (see
/// `varde_kernel::topology::Topology::edge`). The body may since have
/// been removed, or the faces renamed, gone or no longer meeting: the
/// reference then doesn't resolve, which regenerating reports.
///
/// Its direction, where a feature needs one (a revolve's axis), is the
/// way the edge runs with the face of the first key on its left seen
/// from outside the body: a face's own edges run round it that way, so
/// this follows the faces, never the mesh, and an edit that keeps them
/// keeps it ([`EdgeRef::runs_with`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub body: BodyId,
    /// Sorted, and two different keys.
    pub faces: [FaceKey; 2],
    /// Finite and within [`MAX_COORD`].
    pub near: DVec3,
}

impl EdgeRef {
    /// The features the two keys name as the faces' makers, which may not
    /// be there.
    pub fn makers(&self) -> [FeatureId; 2] {
        self.faces.map(|key| FeatureId(key.feature))
    }

    /// Whether an edge between faces of the keys `faces` (a reference's,
    /// sorted) runs the way it does with the face `left` on its left seen
    /// from outside (as a face's own boundary runs, the halfedges on its
    /// triangles), `right` the face on its other side, each given by its
    /// key and its aliases, sorted: `Some(true)` if `left` is the first
    /// key's face, `Some(false)` if `right` is. A face is the first key's
    /// if that key names it (its key or an alias) and the second key
    /// names the other. Where that holds both ways round (aliases naming
    /// each face by both keys), the face whose own key is the first, or
    /// whose other's own key is the second, is; `None` if that doesn't
    /// tell, or the keys name the faces neither way. The model shown and
    /// regenerating both direct an edge by this, so the arrow drawn on
    /// it is the way the revolve turns about it.
    pub fn runs_with(
        faces: &[FaceKey; 2],
        left: (&FaceKey, &[FaceKey]),
        right: (&FaceKey, &[FaceKey]),
    ) -> Option<bool> {
        let [a, b] = faces;
        let named = |(key, aliases): (&FaceKey, &[FaceKey]), by: &FaceKey| {
            key == by || aliases.binary_search(by).is_ok()
        };
        let along = named(left, a) && named(right, b);
        let against = named(right, a) && named(left, b);
        match (along, against) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            (false, false) => None,
            (true, true) => {
                let along = left.0 == a || right.0 == b;
                let against = right.0 == a || left.0 == b;
                (along != against).then_some(along)
            }
        }
    }

    /// Checks what needs only the reference: its keys sorted and
    /// different, its point finite and within [`MAX_COORD`]. What it
    /// names is [`Document::check`](crate::Document::check)'s.
    pub fn check_own(&self) -> Result<(), EdgeError> {
        let [a, b] = &self.faces;
        if a >= b {
            return Err(EdgeError::Faces);
        }
        let near = self.near;
        if near.is_finite() && near.abs().max_element() <= f64::from(MAX_COORD) {
            Ok(())
        } else {
            Err(EdgeError::Near(near))
        }
    }
}

/// What's wrong with an [`EdgeRef`] on its own ([`EdgeRef::check_own`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeError {
    /// Its faces' keys aren't sorted, or are the same.
    Faces,
    /// Its point isn't finite, or is further from zero than
    /// [`MAX_COORD`].
    Near(DVec3),
}

impl fmt::Display for EdgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EdgeError::Faces => f.write_str("its edge's faces are out of order or the same"),
            EdgeError::Near(at) => write!(f, "its edge's point {at} is out of bounds"),
        }
    }
}

impl std::error::Error for EdgeError {}

#[cfg(test)]
mod tests;
