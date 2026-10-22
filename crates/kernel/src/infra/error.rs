//! Why a kernel operation fails: one enum for every operation.
//!
//! On valid input an operation gives a solid, [`KernelError::OutOfBudget`]
//! (it ran out of resources: more of them could help) or
//! [`KernelError::Unrepresentable`] (a value can't be stored in `f64`).
//! The other variants say the input is at fault or the kernel is. Each
//! failure names its cause in this enum only; the evidence built after the
//! outcome is fixed (the failing input entities, by name) refines it and
//! never changes it. Variants for failures no operation raises yet are
//! here already, so callers match on the final set.
//!
//! Of the failures one stage finds, it reports the one at the least key;
//! at one key, running out before a value that can't be stored before the
//! kernel's contradiction; at one key and kind, the check first in the
//! stage's fixed order ([`first_failure`]).

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
    /// A value can't be stored in `f64`, for one of the [`Storage`]
    /// causes. Never for geometry at the scale of the tolerance, nor for
    /// a sign or certificate still open.
    Unrepresentable(Storage),
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

/// Why a value can't be stored ([`KernelError::Unrepresentable`]): an
/// operation has exactly the first three causes; the fourth is only an
/// export's that refuses approximate meshes. The value or site is the
/// evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Storage {
    /// A stored value isn't finite, or a bounded position (a vertex, a
    /// curve's control point, a form's key point inside the body's box)
    /// lies beyond the largest coordinate: an operand's at the
    /// operation's start, a result's when it is stored.
    OutOfRange,
    /// The tolerance is below the least the largest coordinate allows,
    /// checked at the operation's start.
    Tolerance,
    /// Rounding the result to `f64` breaks its validity or an order it
    /// relies on, or leaves a cluster at the least bound storage allows,
    /// and repairing the rounded result failed within its rounds.
    Repair,
    /// An exported mesh has a feature (a separation, an angle, a
    /// collision) below the output precision, and the export refuses
    /// approximate meshes. No operation raises it.
    OutputPrecision,
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

/// The kernel's own failure ([`KernelError::Internal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Internal {
    /// The kernel's decisions contradict each other on every attempt, or
    /// its result fails a check: a bug, or a known gap in its rules, by
    /// its class.
    Inconsistent(Inconsistency),
}

/// Which of the five classes an [`Internal::Inconsistent`] is: three are
/// bugs, two known gaps in the kernel's rules ([`Self::is_rule_gap`]),
/// tracked and counted until shown never to occur. The evidence names
/// the site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Inconsistency {
    /// A check still failing on the last attempt where every decision it
    /// read was an exact tie (a flat sign proven zero, or a coincidence
    /// of inputs). A bug.
    ExactTie,
    /// A check still failing on the last attempt that read a decision by
    /// proxy (a value standing in for one the kernel doesn't compute),
    /// with no near tie that precision moved. A known rule gap.
    Proxy,
    /// The result fails one of the checks a valid solid passes, or a
    /// crossing or defect is certified on it. A bug.
    ResultDefect,
    /// The symbolic perturbation's guard was reached. A bug.
    SosGuard,
    /// A tangency cluster no halving, precision or tie band settles, or a
    /// result face certified too tight to embed. A known rule gap.
    ClusterRuleGap,
}

impl Inconsistency {
    /// Whether it is a known gap in the kernel's rules (tracked and
    /// counted, its replay the full one) rather than a bug.
    pub fn is_rule_gap(self) -> bool {
        matches!(self, Inconsistency::Proxy | Inconsistency::ClusterRuleGap)
    }
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
            KernelError::Unrepresentable(cause) => f.write_str(match cause {
                Storage::OutOfRange => {
                    "a value can't be stored in double precision: it is too large or not a number"
                }
                Storage::Tolerance => "the tolerance is too small for the coordinate range",
                Storage::Repair => "the result can't be rounded to double precision validly",
                Storage::OutputPrecision => "the mesh has features finer than its output precision",
            }),
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
            KernelError::Internal(Internal::Inconsistent(class)) => f.write_str(match class {
                Inconsistency::ExactTie => "the kernel's decisions are inconsistent at exact ties",
                Inconsistency::Proxy => "the kernel's decisions are inconsistent at a proxy",
                Inconsistency::ResultDefect => "the kernel's result is defective",
                Inconsistency::SosGuard => "the kernel's tie-breaking reached its guard",
                Inconsistency::ClusterRuleGap => "the kernel's rules don't settle a tangency",
            }),
            KernelError::Feature(kind) => match *kind {},
            KernelError::NotImplemented(what) => write!(f, "{what} isn't implemented yet"),
        }
    }
}

impl std::error::Error for KernelError {}

/// A storage cause an operation checks itself is
/// [`KernelError::Unrepresentable`]; its value is the evidence.
impl From<varde_decide::num::storage::Unrepresentable> for KernelError {
    fn from(e: varde_decide::num::storage::Unrepresentable) -> Self {
        use varde_decide::num::storage::Unrepresentable as U;
        KernelError::Unrepresentable(match e {
            U::OutOfRange { .. } => Storage::OutOfRange,
            U::Tolerance { .. } => Storage::Tolerance,
        })
    }
}

/// Where an error stands among the failures one stage finds at one key:
/// running out first, then a value that can't be stored, then the
/// kernel's contradiction, then the rest.
fn kind_rank(error: &KernelError) -> u8 {
    match error {
        KernelError::OutOfBudget { .. } => 0,
        KernelError::Unrepresentable(_) => 1,
        KernelError::Internal(_) => 2,
        KernelError::Invalid(_)
        | KernelError::Unsupported(_)
        | KernelError::Feature(_)
        | KernelError::NotImplemented(_) => 3,
    }
}

/// A failure a stage found: at an item's `key`, by the stage's check
/// numbered `check` in its fixed order (a check fails at most once per
/// key).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Failure<K> {
    /// The key of the item it is at.
    pub key: K,
    /// The check that found it, by its place in the stage's order.
    pub check: u32,
    /// The failure.
    pub error: KernelError,
}

/// The failure a stage reports of those it found: the one at the least
/// key; at one key, [`KernelError::OutOfBudget`] before
/// [`KernelError::Unrepresentable`] before [`KernelError::Internal`]
/// (the others after); at one key and kind, the one whose check comes
/// first. The order the failures come in doesn't matter. Stages run in
/// order, so the first stage failing ends the operation; where the work
/// summed in key order runs out ([`Work`](super::budget::Work)) is the
/// key of that `OutOfBudget`.
pub(crate) fn first_failure<K: Ord>(
    failures: impl IntoIterator<Item = Failure<K>>,
) -> Option<Failure<K>> {
    failures.into_iter().min_by(|a, b| {
        (&a.key, kind_rank(&a.error), a.check).cmp(&(&b.key, kind_rank(&b.error), b.check))
    })
}

#[cfg(test)]
mod tests;
