use crate::boolean::BooleanError;
use crate::mesh::CheckError;
use crate::patch::PatchError;
use crate::profile::ProfileError;

/// Why a kernel operation gives no solid. It never gives an invalid one
/// instead.
///
/// The variants are the old kernel's, which callers match on. The facade
/// itself only gives [`TooComplex`](Self::TooComplex) and
/// [`NotImplemented`](Self::NotImplemented), and the Varde kernel's own
/// [`varde_kernel::KernelError`] converts into this one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KernelError {
    /// The operation ran out of its [`Budget`](crate::Budget), or past
    /// another limit that keeps it from running on.
    TooComplex,
    /// The input breaks an invariant the operation can't restore, such
    /// as its topology, or comes out invalid for the tolerance.
    Invalid(CheckError),
    /// A parameter or a split gave geometry outside the patch bounds.
    Patch(PatchError),
    /// A profile that can't be extruded.
    Profile(ProfileError),
    /// A boolean that can't be worked out.
    Boolean(BooleanError),
    /// The operation, or this case of it, isn't implemented yet (in the
    /// Varde kernel, which replaces the old one); names what isn't.
    NotImplemented(&'static str),
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KernelError::TooComplex => f.write_str("the geometry is too complex to work out"),
            KernelError::Invalid(e) => write!(f, "the geometry is invalid: {e}"),
            KernelError::Patch(e) => write!(f, "the geometry is out of bounds: {e}"),
            KernelError::Profile(e) => write!(f, "the profile can't be extruded: {e}"),
            KernelError::Boolean(e) => write!(f, "the boolean can't be worked out: {e}"),
            KernelError::NotImplemented(what) => write!(f, "{what} isn't implemented yet"),
        }
    }
}

impl std::error::Error for KernelError {}

/// The Varde kernel's errors as the old variants: running out of the
/// budget or a limit, whatever the cause, is [`KernelError::TooComplex`],
/// and decisions that contradict each other are
/// [`BooleanError::Inconsistent`]. The facade calls no operation that
/// raises the others yet, and the old variants that would fit carry the
/// old kernel's types, so for now they come out as
/// [`KernelError::NotImplemented`], naming the error the facade can't pass on.
impl From<varde_kernel::KernelError> for KernelError {
    fn from(e: varde_kernel::KernelError) -> Self {
        use varde_kernel::KernelError as K;
        match e {
            K::OutOfBudget { .. } => KernelError::TooComplex,
            K::NotImplemented(what) => KernelError::NotImplemented(what),
            K::Unrepresentable => {
                KernelError::NotImplemented("reporting a result that can't be stored")
            }
            K::Invalid(_) => KernelError::NotImplemented("reporting an invalid operand"),
            K::Unsupported(_) => KernelError::NotImplemented("reporting unsupported input"),
            K::Internal(varde_kernel::Internal::Inconsistent) => {
                KernelError::Boolean(BooleanError::Inconsistent)
            }
            K::Feature(kind) => match kind {},
        }
    }
}

impl From<PatchError> for KernelError {
    fn from(e: PatchError) -> Self {
        KernelError::Patch(e)
    }
}

impl From<ProfileError> for KernelError {
    fn from(e: ProfileError) -> Self {
        KernelError::Profile(e)
    }
}

impl From<CheckError> for KernelError {
    fn from(e: CheckError) -> Self {
        KernelError::Invalid(e)
    }
}
