//! Geometry kernel.
//!
//! A [`Solid`] is a closed mesh of rational quadratic triangles that
//! passes the mesh's checks, and never stored; a [`RenderMesh`] is a solid
//! tessellated for drawing ([`Solid::tessellate`], within a [`Display`]'s
//! targets). Curves drawn with the model, such as sketches, are
//! [`RenderLines`]. A [`Shape`] is the recipe of a box a document stores
//! for now, checked as it's read, that [`build`](Shape::build)s a solid.
//!
//! Booleans are to be built the way
//! [manifold](https://github.com/elalish/manifold) builds them for flat
//! triangles. The math of one curve or triangle is in [`patch`]; closed
//! meshes of them, the check of their invariants, the BVH over them,
//! their refinement and repair, and box and cylinder meshes are in
//! [`mesh`]. Operations on them are bounded by a [`Budget`] and the limits
//! here, and fail with a [`KernelError`].

mod aabb;
mod budget;
mod error;
pub mod mesh;
mod par;
pub mod patch;
mod render_lines;
mod render_mesh;
mod shape;
mod solid;
mod tessellate;
#[cfg(test)]
mod test_rng;
mod tolerance;

pub use aabb::Aabb;
pub use budget::Budget;
pub use error::KernelError;
pub use render_lines::{LinesError, LinesPart, RenderLines};
pub use render_mesh::{MeshError, MeshPart, RenderMesh};
pub use shape::{Shape, ShapeError};
pub use solid::Solid;
pub use tessellate::Display;
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

/// How many times refinement may split a patch of an operation's input,
/// one level at a time: a piece at this depth is `2^24` (about 1.7e7)
/// times smaller than the patch it came from. A patch that still fails the
/// invariants there, such as one with a cusp, makes the operation
/// [`KernelError::TooComplex`].
pub const MAX_REFINE_DEPTH: u32 = 24;

/// The most work one operation may do, in the units of [`Budget`]: about
/// one patch or pair of patches tested or split each, which repair does
/// in about half a microsecond on one thread. So an operation stops after
/// about half a minute of work at most.
pub const MAX_WORK: u64 = 1 << 26;

/// Whether `position` is somewhere a solid may be placed: every coordinate
/// within [`MAX_COORD`] of zero, and so finite.
pub fn position_in_range(position: glam::Vec3) -> bool {
    position.abs().cmple(glam::Vec3::splat(MAX_COORD)).all()
}
