//! Why a kernel operation fails. Only the variants of Varde's
//! `KernelError` that don't name the old kernel's types moved here; C55
//! grows this into the plan's error enum (§2.4).

/// Why a kernel operation gives no solid. It never gives an invalid one
/// instead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KernelError {
    /// The operation ran out of its [`Budget`](crate::Budget), or past
    /// another limit that keeps it from running on.
    TooComplex,
    /// The operation, or this case of it, isn't implemented yet; names
    /// what isn't.
    NotImplemented(&'static str),
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KernelError::TooComplex => f.write_str("the geometry is too complex to work out"),
            KernelError::NotImplemented(what) => write!(f, "{what} isn't implemented yet"),
        }
    }
}

impl std::error::Error for KernelError {}
