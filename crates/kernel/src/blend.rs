//! Chamfers and fillets along a solid's edges: chains, spines, rails,
//! the tools cut along them, the chamfer, the fillet and corners.
//!
//! **Not built yet**: [`chamfer`] and [`fillet`] are stand-ins with the
//! planned signatures that fail with [`KernelError::TooComplex`], so the
//! chamfer and fillet features above the kernel (their documents,
//! regeneration and messages) are built against them. The real
//! implementation replaces this file (and adds `blend/`): per chain,
//! the dihedral's sign the same all along and the faces neither within
//! 1° of flat nor of folded ([`BlendError`]); a spine and two rails,
//! exact where the faces' forms allow (two planes, a plane and a quadric
//! of revolution round a parallel, parallel cylinders) and traced and
//! fitted otherwise; the tool bounded by the chord surface through the
//! rails (a plane, an exact cone round a parallel, fitted ruled strips),
//! extended past the faces and the open ends; then one boolean, body
//! less the tools for convex edges or with them for concave ones, and
//! the new faces named [`FacePart::Blend`](crate::mesh::FacePart::Blend)
//! by each chain's `name`. A fillet's tool is the notch bounded by the
//! two surfaces through the rails square to each face, its faces then
//! replaced by the round strip tangent to both (sharing the rails'
//! records) and a sector at each open end on a planar end face; chains
//! of one fillet meeting at a corner are mitred, three of them at a
//! convex corner of three faces closed by a vertex blend.
//!
//! Which edges are chamfered or filleted is the caller's: the chains of
//! the solid's [`Topology`] its references resolve to, grown along
//! tangent chains ([`Topology::tangent_chains`]) where asked.

use crate::{Budget, Failure, KernelError, Solid, Tolerance, Topology};

/// One chain of the solid's topology to chamfer, how, and the name of
/// the faces made along it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChamferChain {
    /// Its index in the topology's [`Topology::chains`].
    pub chain: u32,
    /// The `edge` of the [`FacePart::Blend`](crate::mesh::FacePart::Blend)
    /// faces made along it (see [`crate::topology::blend_edge`]).
    pub name: u64,
    pub cut: ChamferCut,
}

/// How far a chamfer cuts into the two faces beside its chain, `0` and
/// `1` naming them as the chain's
/// [`regions`](crate::topology::Chain::regions) do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChamferCut {
    /// The rails this far from the edge along face 0 and face 1, in
    /// model units, each positive.
    Distances([f64; 2]),
    /// The rail on face `on` (0 or 1) `distance` from the edge along it,
    /// the chamfer's face at `angle` (radians, above 0 and under a right
    /// angle) to that face, inside the material.
    Angle {
        on: usize,
        distance: f64,
        angle: f64,
    },
}

/// One chain of the solid's topology to fillet, and the name of the
/// faces made along it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilletChain {
    /// Its index in the topology's [`Topology::chains`].
    pub chain: u32,
    /// The `edge` of the [`FacePart::Blend`](crate::mesh::FacePart::Blend)
    /// faces made along it (see [`crate::topology::blend_edge`]).
    pub name: u64,
}

/// Why a chamfer or fillet gives no solid. Chains are named by their
/// index in the topology, corners by their vertex.
#[derive(Debug, Clone, PartialEq)]
pub enum BlendError {
    /// The faces either side of the chain meet within 1° of flat:
    /// nothing to cut.
    Flat { chain: u32 },
    /// The faces either side of the chain meet within 1° of folded onto
    /// each other.
    Folded { chain: u32 },
    /// The chain turns from convex to concave (or back) along its
    /// length.
    Mixed { chain: u32 },
    /// The cut doesn't fit: a rail runs past the face it's on.
    TooBig { chain: u32 },
    /// A fillet's notch runs into another face at an end of the chain
    /// rather than ending on one end face.
    End { chain: u32 },
    /// Chains meeting at this corner can't be blended together
    /// (unequal sizes, more than three edges, convex and concave
    /// meeting).
    Corner { vertex: u32 },
    /// The kernel failed otherwise: out of budget, or its boolean.
    Failed(Failure),
}

impl std::fmt::Display for BlendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlendError::Flat { chain } => write!(f, "the faces at edge {chain} are nearly flat"),
            BlendError::Folded { chain } => {
                write!(f, "the faces at edge {chain} fold onto each other")
            }
            BlendError::Mixed { chain } => {
                write!(f, "edge {chain} turns from convex to concave")
            }
            BlendError::TooBig { chain } => write!(f, "the cut doesn't fit along edge {chain}"),
            BlendError::End { chain } => {
                write!(
                    f,
                    "the fillet along edge {chain} runs into another face at its end"
                )
            }
            BlendError::Corner { vertex } => {
                write!(
                    f,
                    "the edges meeting at vertex {vertex} can't be blended together"
                )
            }
            BlendError::Failed(failure) => write!(f, "{}", failure.error),
        }
    }
}

impl std::error::Error for BlendError {}

impl From<Failure> for BlendError {
    fn from(failure: Failure) -> Self {
        BlendError::Failed(failure)
    }
}

/// `solid` with the chains `chains` of `topology` (made from `solid`,
/// each chain at most once) chamfered as each says, the new faces named
/// for `feature` and the chain's name. Not built yet: always
/// [`BlendError::Failed`] with [`KernelError::TooComplex`].
pub fn chamfer(
    _solid: &Solid,
    _topology: &Topology,
    _chains: &[ChamferChain],
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, BlendError> {
    Err(BlendError::Failed(KernelError::TooComplex.into()))
}

/// `solid` with the chains `chains` of `topology` (made from `solid`,
/// each chain at most once) rounded off to `radius` (in model units,
/// positive), convex edges losing material and concave ones gaining it,
/// the new faces named for `feature` and the chain's name. Not built
/// yet: always [`BlendError::Failed`] with [`KernelError::TooComplex`].
pub fn fillet(
    _solid: &Solid,
    _topology: &Topology,
    _chains: &[FilletChain],
    _radius: f64,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, BlendError> {
    Err(BlendError::Failed(KernelError::TooComplex.into()))
}
