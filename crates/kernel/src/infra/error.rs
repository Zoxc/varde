//! Why a kernel operation fails: one enum for every operation.
//!
//! On valid input an operation gives a solid, [`KernelError::OutOfBudget`]
//! (it ran out of resources: more of them could help) or
//! [`KernelError::Unrepresentable`] (the result can't be stored in `f64`).
//! The other variants say the input is at fault or the kernel is. Each
//! failure names its cause in this enum only; the evidence built after the
//! outcome is fixed (the failing input entities, by name) refines it and
//! never changes it. Variants for failures no operation raises yet are
//! here already, so callers match on the final set.

use super::budget::BudgetProfile;

/// Why a kernel operation gives no result. It never gives an invalid one
/// instead.
#[derive(Debug, Clone, PartialEq)]
pub enum KernelError {
    /// The operation ran out of its [`Budget`](crate::Budget), or reached
    /// a fixed limit, while a decision or certificate was still open.
    /// Resources only: a settled fact is never this.
    OutOfBudget {
        /// What ran out.
        cause: Cause,
        /// Where the work went, up to the point it ran out.
        profile: Box<BudgetProfile>,
    },
    /// A value of the result can't be stored: a stored value is not
    /// finite or a position lies beyond the largest coordinate, the
    /// tolerance is below the least the coordinate range allows, or
    /// rounding to `f64` breaks validity and repairing the rounded result
    /// failed (and, for exports that refuse approximate meshes, a mesh
    /// feature below the output precision). Never for geometry at the
    /// scale of the tolerance.
    Unrepresentable,
    /// An operand is not a valid solid.
    Invalid(Invalid),
    /// The input is of a kind the kernel does not take.
    Unsupported(Unsupported),
    /// The kernel contradicted itself: a bug, or a known gap in its rules
    /// (tracked and counted).
    Internal(Internal),
    /// A feature operation can't build this geometry: by definition, with
    /// a certified margin, or because its input breaks the operation's
    /// stated contract.
    Feature(FeatureKind),
    /// The operation, or this case of it, isn't implemented yet; names
    /// what isn't.
    NotImplemented(&'static str),
}

/// What an operation ran out of ([`KernelError::OutOfBudget`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cause {
    /// The budget, spent at a step that is not a precision step.
    Work,
    /// The budget, spent at a precision step; or the highest precision
    /// reached with a sign or certificate still open.
    Precision,
    /// A fixed size limit, such as the subdivision depth. A larger budget
    /// does not help.
    SizeLimit,
}

/// How an operand fails to be a valid solid ([`KernelError::Invalid`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Invalid {
    /// It breaks the solid invariant numbered `inv`.
    Check {
        /// The invariant broken first.
        inv: u8,
    },
    /// It is not closed: a sheet with open boundary loops.
    NotClosed,
    /// It intersects itself: an imported solid, or a constructed face
    /// whose surroundings are certified too tight to embed it.
    SelfIntersection,
}

/// What kind of input the kernel does not take
/// ([`KernelError::Unsupported`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unsupported {
    /// An imported entity the kernel can't represent.
    Import(Import),
    /// A case the kernel's current stage of development leaves out.
    /// Development builds only (the `staging` feature): a release build
    /// has no such variant.
    #[cfg(feature = "staging")]
    Stage(Stage),
}

/// Why an imported entity is refused ([`Unsupported::Import`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Import {
    /// A surface of higher degree than the kernel represents.
    SurfaceDegree,
}

/// The case left out at this stage of development ([`Unsupported::Stage`]).
#[cfg(feature = "staging")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// A check the stage doesn't run.
    CheckStage,
    /// Tangent contact between operands.
    Tangency,
    /// More than one pair of surfaces where one is handled.
    MultiPair,
    /// Splicing a refitted curve into its neighbours.
    SpliceRefit,
    /// Bodies touching along a vertex or an edge.
    Contact,
}

/// The kernel's own failure ([`KernelError::Internal`]). Its evidence
/// names which of the five classes it is: an exact tie, a proxy rule gap,
/// a result defect, the symbolic perturbation's guard, or a cluster rule
/// gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Internal {
    /// The kernel's decisions contradict each other on every attempt.
    Inconsistent,
}

/// What a feature operation can't build ([`KernelError::Feature`]). It
/// gains a variant for each refusal as the feature operations land; until
/// then none can be raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureKind {}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KernelError::OutOfBudget { cause, .. } => match cause {
                Cause::Work => f.write_str("the geometry is too complex to work out"),
                Cause::Precision => {
                    f.write_str("the geometry needs more precision than the kernel has")
                }
                Cause::SizeLimit => f.write_str("the geometry goes past a size limit"),
            },
            KernelError::Unrepresentable => {
                f.write_str("the result can't be stored in double precision")
            }
            KernelError::Invalid(Invalid::Check { inv }) => {
                write!(f, "the solid is invalid: it breaks invariant {inv}")
            }
            KernelError::Invalid(Invalid::NotClosed) => f.write_str("the solid isn't closed"),
            KernelError::Invalid(Invalid::SelfIntersection) => {
                f.write_str("the solid intersects itself")
            }
            KernelError::Unsupported(Unsupported::Import(Import::SurfaceDegree)) => {
                f.write_str("a surface's degree is too high to import")
            }
            #[cfg(feature = "staging")]
            KernelError::Unsupported(Unsupported::Stage(stage)) => {
                write!(f, "this case ({stage:?}) isn't supported yet")
            }
            KernelError::Internal(Internal::Inconsistent) => {
                f.write_str("the kernel's decisions are inconsistent")
            }
            KernelError::Feature(kind) => match *kind {},
            KernelError::NotImplemented(what) => write!(f, "{what} isn't implemented yet"),
        }
    }
}

impl std::error::Error for KernelError {}

/// A storage cause is [`KernelError::Unrepresentable`]; its value is the
/// evidence.
impl From<varde_decide::num::storage::Unrepresentable> for KernelError {
    fn from(_: varde_decide::num::storage::Unrepresentable) -> Self {
        KernelError::Unrepresentable
    }
}
