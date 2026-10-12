//! The facade between the app and the Varde kernel (`varde-kernel`).
//!
//! The old kernel that lived here, a closed mesh of rational quadratic
//! triangles with its booleans, extrudes, revolves, blends, sweeps and
//! tessellation, is gone (it stays reachable at the git tag
//! `varde-kernel-legacy`). What is left is the public API the other
//! crates call, so they build unchanged while the Varde kernel grows in
//! `crates/kernel`; as its operations land, this facade forwards to them,
//! and once every caller uses the Varde kernel's types directly it can go.
//!
//! Until then:
//!
//! - The value types stay as they were, with their fields and pure
//!   helpers: [`Profile`], [`Frame`], [`Tolerance`], [`Aabb`], [`Motion`]
//!   and [`Datum`] with [`Motion::align`], the curves and triangles of
//!   [`patch`], the names and forms of [`mesh`], [`Evidence`] and
//!   [`Failure`], [`RenderMesh`], [`RenderLines`], [`MeshUpload`] and
//!   [`ManifoldMesh`] with their checks, and the errors callers match on.
//! - Every [`Solid`] is empty: [`Solid::empty`] is the one solid there
//!   is. Operations that would make geometry ([`extrude()`],
//!   [`revolve()`], [`Solid::cuboid`], [`Solid::new`] of a mesh with
//!   triangles, [`boolean()`] of two non-empty solids, [`assemble`], the
//!   blends, shells, sweeps, lofts and split tools) fail with
//!   [`KernelError::NotImplemented`], naming what isn't there yet.
//! - Queries of the empty solid answer for an empty one: no bounds, no
//!   volume, an empty [`Topology`] and [`RenderMesh`], nothing found.
//!
//! Nothing here panics on a caller's data: the stubs return errors or
//! empty data. Angles go through [`trig`], whose bits are the same on
//! every platform.

mod aabb;
pub mod blend;
mod boolean;
mod error;
mod extrude;
mod failure;
pub mod loft;
mod manifold;
pub mod measure;
pub mod mesh;
mod mesh_upload;
pub mod patch;
mod profile;
mod render_lines;
mod render_mesh;
mod revolve;
pub mod section;
pub mod shell;
mod solid;
pub mod sweep;
mod tessellate;
mod tolerance;
pub mod topology;
pub mod transform;

pub use aabb::Aabb;
pub use blend::{BlendError, ChamferChain, ChamferCut, FilletChain, chamfer, fillet};
pub use boolean::{
    BooleanError, Op, ToolError, boolean, chain_tool, half_space, split, surface_tool, touches,
};
pub use error::KernelError;
pub use extrude::{Frame, TaperError, extrude, extrude_tapered};
pub use failure::{EVIDENCE_WORK, Evidence, EvidenceCaps, Failure, MAX_EVIDENCE, Operand};
pub use manifold::{ManifoldError, ManifoldMesh};
pub use mesh_upload::{
    CREASE, EdgePoint, EdgeStream, MeshUpload, NEIGHBOUR_ONLY, NO_EDGE, UploadError,
};
pub use profile::{Loop, MAX_PROFILE_SEGMENTS, Profile, ProfileError, Segment};
pub use render_lines::{LinesError, LinesPart, RenderLines};
pub use render_mesh::{MeshError, MeshPart, MeshParts, RenderMesh, RenderPart};
pub use revolve::{Sweep, revolve};
pub use shell::{DraftError, OffsetError, ShellError, draft_faces, offset_faces, shell};
pub use solid::Solid;
pub use tessellate::{Display, PatchSamples};
pub use tolerance::Tolerance;
pub use topology::Topology;
pub use transform::{AlignError, AlignOptions, Datum, Instance, Motion, assemble};
pub use varde_decide::num::trig;
pub use varde_kernel::{Budget, MAX_WORK};

/// The largest coordinate or size, in model units, a design may have: its
/// sketches' coordinates and lengths, and so the solids built from them. A
/// file could carry any number, and NaN would make a document unequal to
/// itself, while building and drawing solids adds sizes to coordinates
/// and multiplies lengths, which would overflow to infinity well within
/// the `f32` range. Within this bound they cannot, and meshes stay within
/// [`RenderMesh::MAX_POSITION`].
pub const MAX_COORD: f32 = 1e6;

/// The most patches a mesh may have. Halfedge ids (three per patch) and
/// every count derived from them then fit a `u32` with room to spare.
pub const MAX_PATCHES: usize = 1 << 22;

#[cfg(test)]
mod tests;
