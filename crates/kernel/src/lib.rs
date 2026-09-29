//! Geometry kernel.
//!
//! Three layers: a [`Shape`] is what a document stores, a recipe checked
//! as it's read; a [`Solid`] is the geometry it [`build`](Shape::build)s,
//! which is never stored; and a [`RenderMesh`] is a solid tessellated for
//! drawing. Curves drawn with the model, such as sketches, are
//! [`RenderLines`]. Only analytic primitives exist so far, and no booleans.
//!
//! Solids are to become closed meshes of rational quadratic triangles,
//! with booleans built the way [manifold](https://github.com/elalish/manifold)
//! builds them for flat triangles. The math of one curve or triangle is in
//! [`patch`]; closed meshes of them, the check of their invariants and the
//! BVH over them are in [`mesh`].

mod aabb;
pub mod mesh;
mod par;
pub mod patch;
mod render_lines;
mod render_mesh;
mod shape;
mod solid;
#[cfg(test)]
mod test_rng;
mod tolerance;

pub use aabb::Aabb;
pub use render_lines::{LinesError, LinesPart, RenderLines};
pub use render_mesh::{MeshError, MeshPart, RenderMesh};
pub use shape::{Shape, ShapeError};
pub use solid::Solid;
pub use tolerance::Tolerance;

/// The largest coordinate or size, in model units, a shape or a position
/// may have. A file could carry any `f32`, and NaN would make a document
/// unequal to itself, while tessellation adds positions to sizes and
/// multiplies edge lengths, which overflow to infinity well within the
/// `f32` range. Within this bound they cannot, and meshes stay within
/// [`RenderMesh::MAX_POSITION`].
pub const MAX_COORD: f32 = 1e6;

/// The most patches a mesh may have. Halfedge ids (three per patch) and
/// every count derived from them then fit a `u32` with room to spare.
pub const MAX_PATCHES: usize = 1 << 22;

/// Whether `position` is somewhere a solid may be placed: every coordinate
/// within [`MAX_COORD`] of zero, and so finite.
pub fn position_in_range(position: glam::Vec3) -> bool {
    position.abs().cmple(glam::Vec3::splat(MAX_COORD)).all()
}
