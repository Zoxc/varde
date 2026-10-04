//! Geometry kernel.
//!
//! A [`Solid`] is a closed mesh of rational quadratic triangles that
//! passes the mesh's checks, and never stored; a [`RenderMesh`] is a solid
//! tessellated for drawing ([`Solid::tessellate`], within a [`Display`]'s
//! targets); a [`ManifoldMesh`] is one welded into a closed, oriented
//! manifold of triangles for export ([`Solid::manifold_mesh`]). Curves drawn with the model, such as sketches, are
//! [`RenderLines`]. Documents store no solids: they store the features
//! that build them.
//!
//! [`extrude()`] sweeps a [`Profile`], closed loops of conics placed on a
//! [`Frame`], into an exact solid; [`Solid::volume`] and [`Solid::area`]
//! measure one. [`sweep`] makes exact strips on cones and quadrics of
//! revolution, and fitted ones (tori, other surfaces of revolution, caps
//! round poles and apexes) within half the fit tolerance, of which
//! [`revolve()`] makes a [`Profile`] turned about an axis in its plane,
//! all the way round or through a part turn.
//!
//! [`boolean()`] unites, subtracts and intersects solids the way
//! [manifold](https://github.com/elalish/manifold) does for flat
//! triangles, by counting from primitives each worked out once, so the
//! result is always a closed manifold, with curved patches cut along
//! exact conics where planes meet quadrics and along fitted ones
//! elsewhere.
//! [`touches`] says whether two solids meet. [`Solid::transformed`]
//! moves, turns or mirrors a solid by a [`Motion`], and [`assemble`]
//! makes one solid of copies. [`measure`] measures a
//! solid as built: lengths, areas, volumes, centres of mass
//! ([`Solid::moments`]), tight boxes ([`Solid::tight_bounds`]), forms,
//! points, directions and angles of what is picked of it. The math of one curve or
//! triangle is in [`patch`]; closed
//! meshes of them, the check of their invariants, the BVH over them,
//! their refinement and repair, and box and cylinder meshes are in
//! [`mesh`]. Operations on them are bounded by a [`Budget`] and the limits
//! here, and fail with a [`KernelError`]; the public operations making or
//! combining solids fail with a [`Failure`], the error and the
//! [`Evidence`] of where. Angles go through [`trig`],
//! whose bits are the same on every platform.

mod aabb;
pub mod blend;
mod boolean;
mod budget;
mod error;
mod extrude;
mod failure;
mod manifold;
pub mod measure;
pub mod mesh;
mod par;
pub mod patch;
mod profile;
mod quadrature;
mod render_lines;
mod render_mesh;
mod revolve;
pub mod shell;
mod solid;
pub mod sweep;
mod tessellate;
#[cfg(test)]
mod test_rng;
mod tolerance;
pub mod topology;
pub mod transform;
pub mod trig;

pub use aabb::Aabb;
pub use blend::{BlendError, ChamferChain, ChamferCut, FilletChain, chamfer, fillet};
pub use boolean::{
    BooleanError, Op, ToolError, boolean, chain_tool, half_space, split, surface_tool, touches,
};
pub use budget::Budget;
pub use error::KernelError;
pub use extrude::{Frame, extrude};
pub use failure::{EVIDENCE_WORK, Evidence, EvidenceCaps, Failure, MAX_EVIDENCE, Operand};
pub use manifold::{ManifoldError, ManifoldMesh};
pub use profile::{Loop, MAX_PROFILE_SEGMENTS, Profile, ProfileError, Segment};
pub use render_lines::{LinesError, LinesPart, RenderLines};
pub use render_mesh::{MeshError, MeshPart, MeshParts, RenderMesh, RenderPart};
pub use revolve::{Sweep, revolve};
pub use shell::{ShellError, shell};
pub use solid::Solid;
pub use tessellate::{Display, PatchSamples};
pub use tolerance::Tolerance;
pub use topology::Topology;
pub use transform::{AlignError, AlignOptions, Datum, Instance, Motion, assemble};

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

/// The most steps tracing one cut of a boolean may take, marching along
/// where two curved patches meet. A cut that needs more (it wanders, or
/// runs along a tangency) falls back to a simpler curve between its
/// ends: only its geometry suffers, never the topology.
pub const MAX_TRACE_STEPS: usize = 4096;

/// The most work one operation may do, in the units of [`Budget`]: about
/// one patch or pair of patches tested or split each, about half a
/// microsecond on one thread (0.2 to 0.7 µs measured across booleans that
/// work and fail). So an operation stops after about two seconds of work
/// at most on one thread, less on several. The heaviest booleans
/// measured, a plate with 144 holes joined to a boss across them or two
/// flat tori of 36 864 patches each, take about 2 million units.
pub const MAX_WORK: u64 = 1 << 22;

/// A public operation's result with its [`Failure`]'s evidence stripped,
/// for tests comparing errors.
#[cfg(test)]
pub(crate) trait Stripped<T> {
    fn stripped(self) -> Result<T, KernelError>;
}

#[cfg(test)]
impl<T> Stripped<T> for Result<T, Failure> {
    fn stripped(self) -> Result<T, KernelError> {
        self.map_err(|f| {
            assert!(f.evidence.within_caps(), "{:?}", f.error);
            f.error
        })
    }
}
