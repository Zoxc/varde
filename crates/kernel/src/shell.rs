//! Shells: a solid hollowed out to walls of one thickness, opened
//! through the faces asked for.
//!
//! **Not built yet**: [`shell`] is a stand-in with the planned signature
//! that fails with [`KernelError::TooComplex`], so the shell feature
//! above the kernel (its document, regeneration and messages) is built
//! against it. The real implementation replaces this file (and adds
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
//! Which faces are open is the caller's: regions of the solid's
//! [`Topology`] its references resolve to.

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
