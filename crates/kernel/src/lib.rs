//! The Varde geometry kernel, replacing the old kernel now in
//! `varde-kernel-compat`. So far only its infrastructure: the [`Budget`]
//! and the charging of work against it, the errors ([`KernelError`]), the
//! parallel map and quadrature.
//! The quadrature rules are public ([`measure::quadrature`]): the facade
//! measures profiles' areas with them.

// Moved ahead of the code that calls them: the kernel's operations and
// measures, as they land, are their first callers.
#[allow(dead_code, reason = "moved ahead of its callers")]
mod infra;
#[allow(dead_code, reason = "moved ahead of its callers")]
pub mod measure;

pub use infra::budget::{
    Budget, BudgetProfile, Cap, HOTTEST, ItemAt, Limit, MAX_WORK, Step, Stopped,
};
#[cfg(feature = "staging")]
pub use infra::error::Stage;
pub use infra::error::{Cause, FeatureKind, Import, Internal, Invalid, KernelError, Unsupported};
