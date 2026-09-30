//! Geometry kernel.
//!
//! A [`Solid`] is a closed mesh of rational quadratic triangles that
//! passes the mesh's checks, and never stored; a [`RenderMesh`] is a solid
//! tessellated for drawing ([`Solid::tessellate`], within a [`Display`]'s
//! targets). Curves drawn with the model, such as sketches, are
//! [`RenderLines`]. Documents store no solids: they store the features
//! that build them.
//!
//! [`extrude()`] sweeps a [`Profile`], closed loops of conics placed on a
//! [`Frame`], into an exact solid; [`Solid::volume`] and [`Solid::area`]
//! measure one.
//!
//! [`boolean()`] unites, subtracts and intersects solids the way
//! [manifold](https://github.com/elalish/manifold) does for flat
//! triangles, by counting from primitives each worked out once, so the
//! result is always a closed manifold; so far for flat patches only.
//! [`touches`] says whether two solids meet. The math of one curve or
//! triangle is in [`patch`]; closed
//! meshes of them, the check of their invariants, the BVH over them,
//! their refinement and repair, and box and cylinder meshes are in
//! [`mesh`]. Operations on them are bounded by a [`Budget`] and the limits
//! here, and fail with a [`KernelError`].

mod aabb;
mod boolean;
mod budget;
mod error;
mod extrude;
pub mod mesh;
mod par;
pub mod patch;
mod profile;
mod quadrature;
mod render_lines;
mod render_mesh;
mod solid;
mod tessellate;
#[cfg(test)]
mod test_rng;
mod tolerance;

pub use aabb::Aabb;
pub use boolean::{BooleanError, Op, boolean, touches};
pub use budget::Budget;
pub use error::KernelError;
pub use extrude::{Frame, extrude};
pub use profile::{Loop, MAX_PROFILE_SEGMENTS, Profile, ProfileError, Segment};
pub use render_lines::{LinesError, LinesPart, RenderLines};
pub use render_mesh::{MeshError, MeshPart, RenderMesh};
pub use solid::Solid;
pub use tessellate::Display;
pub use tolerance::Tolerance;

/// The largest coordinate or size, in model units, a design may have: its
/// sketches' coordinates and lengths, and so the solids built from them. A
/// file could carry any number, and NaN would make a document unequal to
/// itself, while building and drawing solids adds sizes to coordinates
/// and multiplies lengths, which would overflow to infinity well within
/// the `f32` range. Within this bound they cannot, and meshes stay within
/// [`RenderMesh::MAX_POSITION`].
pub const MAX_COORD: f32 = 1e6;

/// Refuses a point that isn't finite or has a coordinate past
/// [`MAX_COORD`], NaN included.
pub(crate) fn in_range(p: glam::DVec3) -> Result<(), patch::PatchError> {
    let m = p.abs().max_element();
    if p.is_finite() && m <= f64::from(MAX_COORD) {
        Ok(())
    } else {
        Err(patch::PatchError::Coordinate(m))
    }
}

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
