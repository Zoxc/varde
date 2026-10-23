//! Infrastructure shared by every operation: the budget, the errors, the
//! tolerance, context and status of an operation, the one parallel
//! primitive, and the bounding volume hierarchy of the broad phases.

pub(crate) mod budget;
pub(crate) mod bvh;
pub(crate) mod error;
pub(crate) mod ops;
pub(crate) mod par;
