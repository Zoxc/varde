//! The Varde geometry kernel, replacing the old kernel now in
//! `varde-kernel-compat`. So far its infrastructure: the [`Budget`] and
//! the charging of work against it, the errors ([`KernelError`]), the
//! parallel map and quadrature; and the first of its geometry: the tile
//! ([`geom::tile`]), the forms' keys ([`geom::form`]) and their
//! lattices of tiles ([`geom::lattice`]: plane, circular and conic-chain
//! cylinder, cone, sphere, ring and spindle torus, revolved conic, affine
//! images; the constructors in [`geom::forms`], ruled surfaces' there
//! too), tiled surfaces, edge and coedge curves and their
//! seam bounds; and the minimal B-rep store counting reads
//! ([`topo::brep_min`]); and the
//! boolean's first step, its tangency clusters ([`boolean::snap`]).
//! The quadrature rules are public ([`measure::quadrature`]): the facade
//! measures profiles' areas with them.

pub mod boolean;
pub mod geom;
// Moved ahead of the code that calls them: the kernel's operations and
// measures, as they land, are their first callers.
#[allow(dead_code, reason = "moved ahead of its callers")]
mod infra;
#[allow(dead_code, reason = "moved ahead of its callers")]
pub mod measure;
pub mod topo;

#[cfg(test)]
mod oracle;

pub use infra::budget::{
    Budget, BudgetProfile, Cap, HOTTEST, ItemAt, ItemName, Limit, MAX_SUBDIV_DEPTH, MAX_WORK, Step,
    Stopped,
};
/// The broad phase's tree uncharged, for the calibration benchmark
/// (`benches/calibrate.rs`). Not part of the kernel's interface.
#[doc(hidden)]
pub use infra::bvh::calibration as bvh_calibration;
#[cfg(feature = "staging")]
pub use infra::error::Stage;
pub use infra::error::{
    Cause, FeatureKind, Import, Inconsistency, Internal, Invalid, KernelError, Storage, Unsupported,
};
pub use infra::ops::{
    FeatureId, InstanceId, MAX_REPAIR_ROUNDS, MAX_WARNINGS, OpCtx, Repair, Status, Tolerance,
    ToleranceOutOfRange, Warning, WarningKind,
};
