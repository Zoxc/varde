//! The Varde geometry kernel (plan §11), replacing the old kernel now in
//! `varde-kernel-compat`. So far only the infrastructure moved over from
//! it: the [`Budget`], [`KernelError`], the parallel map and quadrature.

// Moved ahead of the code that calls them (V-02); A-02 and the measure
// tasks are their first callers.
#[allow(dead_code, reason = "moved ahead of its callers")]
mod infra;
#[allow(dead_code, reason = "moved ahead of its callers")]
mod measure;

pub use infra::budget::{Budget, MAX_WORK};
pub use infra::error::KernelError;
