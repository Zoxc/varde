//! Infrastructure shared by every operation: the budget, the errors, the
//! tolerance, context and status of an operation, and the one parallel
//! primitive.

pub(crate) mod budget;
pub(crate) mod error;
pub(crate) mod ops;
pub(crate) mod par;
