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
/// keeps it.
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
