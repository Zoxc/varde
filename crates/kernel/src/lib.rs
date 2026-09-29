//! Geometry kernel.
//!
//! Three layers: a [`Shape`] is what a document stores, a recipe checked
//! as it's read; a [`Solid`] is the geometry it [`build`](Shape::build)s,
//! which is never stored; and a [`RenderMesh`] is a solid tessellated for
//! drawing. Only analytic primitives exist so far, and no booleans. The
//! intent is to back [`Solid`] with a robust mesh-boolean kernel such as
//! [manifold](https://github.com/elalish/manifold), which the other two
//! layers wouldn't see.

mod aabb;
mod render_mesh;
mod shape;
mod solid;

pub use aabb::Aabb;
pub use render_mesh::{MeshError, MeshPart, RenderMesh};
pub use shape::{Shape, ShapeError};
pub use solid::Solid;

/// The largest coordinate or size, in model units, a shape or a position
/// may have. A file could carry any `f32`, and NaN would make a document
/// unequal to itself, while tessellation adds positions to sizes and
/// multiplies edge lengths, which overflow to infinity well within the
/// `f32` range. Within this bound they cannot, and meshes stay within
/// [`RenderMesh::MAX_POSITION`].
pub const MAX_COORD: f32 = 1e6;

/// Whether `position` is somewhere a solid may be placed: every coordinate
/// within [`MAX_COORD`] of zero, and so finite.
pub fn position_in_range(position: glam::Vec3) -> bool {
    position.abs().cmple(glam::Vec3::splat(MAX_COORD)).all()
}
