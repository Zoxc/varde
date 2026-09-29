use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::{MAX_COORD, Solid};

/// What a body is made of, as a document stores it: a recipe that
/// [`build`](Shape::build)s a [`Solid`].
///
/// Only primitives for now. Shapes that combine others will refer to them
/// by id rather than nest, so a file can't make decoding recurse without
/// bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// An axis-aligned box with one corner at the origin.
    Cuboid { size: Vec3 },
}

impl Shape {
    pub fn cuboid(size: Vec3) -> Self {
        Shape::Cuboid { size }
    }

    /// Checks what a file could get wrong: every size is above zero and
    /// at most [`MAX_COORD`], so the solid it builds has
    /// [`bounds`](Solid::bounds) within [`MAX_COORD`] of the origin, and
    /// tessellating it, and adding a position within [`MAX_COORD`] to
    /// them, stays finite.
    pub fn check(&self) -> Result<(), ShapeError> {
        match self {
            Shape::Cuboid { size } => {
                if size.cmpgt(Vec3::ZERO).all() && size.cmple(Vec3::splat(MAX_COORD)).all() {
                    Ok(())
                } else {
                    Err(ShapeError::Size(*size))
                }
            }
        }
    }

    /// The solid this makes, once it passes [`Shape::check`], so that
    /// every [`RenderMesh`](crate::RenderMesh) tessellated from it stays
    /// within [`RenderMesh::MAX_POSITION`](crate::RenderMesh::MAX_POSITION).
    pub fn build(&self) -> Result<Solid, ShapeError> {
        self.check()?;
        Ok(Solid::new(self.clone()))
    }
}

/// Why a [`Shape`] fails [`Shape::check`], and so [`Shape::build`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeError {
    /// A cuboid size not above zero, or past [`MAX_COORD`].
    Size(Vec3),
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShapeError::Size(size) => write!(
                f,
                "the shape has a size of {size}, not above zero and within {MAX_COORD}"
            ),
        }
    }
}

impl std::error::Error for ShapeError {}

#[cfg(test)]
mod tests;
