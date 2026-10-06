use crate::boolean::BooleanError;
use crate::mesh::CheckError;
use crate::patch::PatchError;
use crate::profile::ProfileError;

/// Why a kernel operation gives no solid. It never gives an invalid one
/// instead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KernelError {
    /// The operation ran out of its [`Budget`](crate::Budget), or needed
    /// more than [`MAX_PATCHES`](crate::MAX_PATCHES) patches or deeper
    /// refinement than [`MAX_REFINE_DEPTH`](crate::MAX_REFINE_DEPTH), or
    /// past another limit that keeps it from running on. Never for detail
    /// too small for the tolerance: that is [`Invalid`](Self::Invalid) or
    /// [`ProfileError::TooFine`].
    TooComplex,
    /// The input breaks an invariant the operation can't restore, such
    /// as its topology, or comes out invalid for the tolerance (a solid
    /// too small for its resolution, say).
    Invalid(CheckError),
    /// A parameter or a split gave geometry outside the patch bounds.
    Patch(PatchError),
    /// A profile that can't be extruded.
    Profile(ProfileError),
    /// A boolean that can't be worked out.
    Boolean(BooleanError),
    /// The operation, or this case of it, isn't implemented yet; names
    /// what isn't.
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
