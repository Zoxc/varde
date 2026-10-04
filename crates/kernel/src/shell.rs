//! Shells: a solid hollowed out to walls of one thickness, opened
//! through the faces asked for; and offset faces: faces of a solid
//! moved along their normals, the faces around them extended or
//! trimmed to meet them.
//!
//! **Not built yet**: [`shell`] and [`offset_faces`] are stand-ins with
//! the planned signatures that fail with [`KernelError::TooComplex`], so
//! the shell and offset face features above the kernel (their
//! documents, regeneration and messages) are built against them. The real implementation replaces this file (and adds
//! `shell/`, the **offset solid**: a solid with each of its regions
//! offset by a distance of its own, zero keeping a face on its form, its
//! neighbours extended or trimmed until they meet, which offset face
//! and draft share). Inward, the hollow is the offset solid with the
//! kept faces by `−thickness` and the open ones outwards past the body,
//! and the result is the body less the hollow (with no open face, the
//! hollow is a void inside it). Outward, the result is the offset solid
//! with the kept faces by `+thickness` and the open ones by zero, less
//! the body with its open faces pushed outwards past it. Offset forms
//! (planes shifted, cylinders' and spheres' radii, cones' apexes, tori
//! and fitted faces fitted), corners (three planes a 3 × 3 solve, else
//! a line and a quadric, else Newton from the old corner) and chains
//! (exact where the offset surfaces meet in lines, conics or coaxial
//! circles, traced otherwise), the faces rebuilt from their loops and
//! named [`FacePart::Offset`](crate::mesh::FacePart::Offset) of the
//! key they came from; the refusals are [`ShellError`]'s.
//!
//! Offset face is that offset solid with the picked regions by their
//! distance and every other by zero, rebuilt only where a corner or
//! chain moved, no boolean; its refusals are [`OffsetError`]'s.
//!
//! Which faces are open, or moved, is the caller's: regions of the
//! solid's [`Topology`] its references resolve to.

use crate::{Budget, Failure, KernelError, Solid, Tolerance, Topology};

/// Why a shell gives no solid. Regions are named by their index in the
/// topology, corners by their vertex.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellError {
    /// The round face `region` (a cylinder, cone, sphere or torus) would
    /// shrink to nothing: its radius is at or under the thickness.
    RoundTooSmall { region: u32 },
    /// The offset faces cross each other: the walls are too thick for
    /// this body.
    TooThick,
    /// More than three faces meet at this corner, and their offsets
    /// don't meet in one point.
    Corner { vertex: u32 },
    /// The kernel failed otherwise: out of budget, or its boolean.
    Failed(Failure),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellError::RoundTooSmall { region } => {
                write!(f, "the round face {region} is thinner than the shell")
            }
            ShellError::TooThick => f.write_str("the shell is too thick for this body"),
            ShellError::Corner { vertex } => {
                write!(f, "the faces meeting at vertex {vertex} can't be offset")
            }
            ShellError::Failed(failure) => write!(f, "{}", failure.error),
        }
    }
}

impl std::error::Error for ShellError {}

impl From<Failure> for ShellError {
    fn from(failure: Failure) -> Self {
        ShellError::Failed(failure)
    }
}

/// `solid` hollowed to walls `thickness` thick (in model units,
/// positive), inside its faces or with `outward` outside them, opened
/// through the regions `open` of `topology` (made from `solid`; sorted,
/// each at most once; none for a closed hollow body), the new faces
/// named for `feature`. Not built yet: always [`ShellError::Failed`]
/// with [`KernelError::TooComplex`].
#[allow(clippy::too_many_arguments)]
pub fn shell(
    _solid: &Solid,
    _topology: &Topology,
    _open: &[u32],
    _thickness: f64,
    _outward: bool,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, ShellError> {
    Err(ShellError::Failed(KernelError::TooComplex.into()))
}

/// Why an offset of faces gives no solid. Regions are named by their
/// index in the topology, corners by their vertex. The refusals shared
/// with [`ShellError`] (a round shrinking to nothing, a corner, the
/// kernel failing) are named alike; offset face never changes the
/// solid's topology, so what would is refused here.
#[derive(Debug, Clone, PartialEq)]
pub enum OffsetError {
    /// A chain between faces moved would run the other way between its
    /// corners, or a rebuilt face's loop would turn over: the face
    /// `region` moves past a neighbouring face.
    PastNeighbour { region: u32 },
    /// The moved faces reach another part of the same solid (the
    /// result fails the hull rules).
    IntoBody,
    /// The round face `region` (a cylinder, cone, sphere or torus) would
    /// shrink to nothing: a hole closing, a boss's wall to its axis.
    RoundTooSmall { region: u32 },
    /// The face `region`, next to a moved one, has no surface to extend
    /// (its form is unknown: a canal-surface fillet).
    NoSurface { region: u32 },
    /// The face `region`, tangent to a moved one, isn't moved with it
    /// (tangent growth off): neither can be extended to meet the other.
    TangentNeighbour { region: u32 },
    /// More than three faces meet at this corner, and their new
    /// surfaces don't meet in one point.
    Corner { vertex: u32 },
    /// The result would reach past [`MAX_COORD`](crate::MAX_COORD).
    OutOfRange,
    /// The kernel failed otherwise: out of budget, or a check.
    Failed(Failure),
}

impl std::fmt::Display for OffsetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OffsetError::PastNeighbour { region } => {
                write!(f, "face {region} moves past a neighbouring face")
            }
            OffsetError::IntoBody => f.write_str("the faces run into another part of the solid"),
            OffsetError::RoundTooSmall { region } => {
                write!(f, "the round face {region} shrinks to nothing")
            }
            OffsetError::NoSurface { region } => {
                write!(f, "face {region} has no surface to extend")
            }
            OffsetError::TangentNeighbour { region } => {
                write!(
                    f,
                    "face {region} is tangent to a moved face but isn't moved"
                )
            }
            OffsetError::Corner { vertex } => {
                write!(f, "the faces meeting at vertex {vertex} can't be offset")
            }
            OffsetError::OutOfRange => f.write_str("the offset moves the solid out of range"),
            OffsetError::Failed(failure) => write!(f, "{}", failure.error),
        }
    }
}

impl std::error::Error for OffsetError {}

impl From<Failure> for OffsetError {
    fn from(failure: Failure) -> Self {
        OffsetError::Failed(failure)
    }
}

/// `solid` with the regions `faces` of `topology` (made from `solid`;
/// sorted, each at most once, at least one) moved along their normals
/// by `distance` (in model units, nonzero: positive out of the solid,
/// growing it; negative into it), grown first across tangent-continuous
/// chains when `tangent` is on, every face around them extended or
/// trimmed to meet them. It's the offset solid [`shell`] builds on,
/// with `distance` for the faces picked and zero for the rest: no
/// boolean, the topology unchanged, every face keeping its name (so
/// references to a moved face still find it); faces made new (none,
/// unless a chain must be split) are named for `feature`. The
/// refusals are [`OffsetError`]'s. Not built yet: always
/// [`OffsetError::Failed`] with [`KernelError::TooComplex`].
#[allow(clippy::too_many_arguments)]
pub fn offset_faces(
    _solid: &Solid,
    _topology: &Topology,
    _faces: &[u32],
    _distance: f64,
    _tangent: bool,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, OffsetError> {
    Err(OffsetError::Failed(KernelError::TooComplex.into()))
}
