//! Revolves: a [`Profile`] turned about an axis in its plane.
//!
//! The old kernel's revolve is gone; [`revolve()`] fails with
//! [`KernelError::NotImplemented`] until the Varde kernel's revolve lands.

use crate::{Budget, Failure, Frame, KernelError, Profile, Solid, Tolerance};

/// How far a revolve turns: all the way round, or from the angle `from`
/// to `to` (radians, turning the frame's `x` towards `x × y`, `0` at `x`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sweep {
    Full,
    Part { from: f64, to: f64 },
}

/// The solid `profile` on `frame` sweeps turning about the frame's `y`
/// axis through `sweep`, its faces named for `feature`. Not built yet:
/// a profile failing [`Profile::check`] gives its
/// [`KernelError::Profile`], any other [`KernelError::NotImplemented`].
pub fn revolve(
    profile: &Profile,
    _frame: &Frame,
    _sweep: Sweep,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, Failure> {
    profile.check().map_err(KernelError::Profile)?;
    Err(KernelError::NotImplemented("revolving").into())
}
