//! Extrudes: a [`Profile`] placed on a [`Frame`] and swept along the
//! frame's normal into a solid.
//!
//! The old kernel's extrude is gone; [`extrude()`] and
//! [`extrude_tapered`] fail with [`KernelError::NotImplemented`] until
//! the Varde kernel's extrude lands. [`Frame`] stays: callers place
//! profiles with it.

use glam::{DVec2, DVec3};

use crate::{Budget, Failure, KernelError, Profile, Solid, Tolerance};

/// Where a profile lies in space: its origin and the unit axes its `x`
/// and `y` run along. The normal, `x × y`, is the direction it is
/// extruded in, and counter-clockwise is seen from the side it points to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
}

impl Frame {
    /// The world's XY plane.
    pub const XY: Frame = Frame {
        origin: DVec3::ZERO,
        x: DVec3::X,
        y: DVec3::Y,
    };

    /// `x × y`.
    pub fn normal(&self) -> DVec3 {
        self.x.cross(self.y)
    }

    /// The point `p` of the profile's plane, moved `height` along the
    /// normal.
    pub fn point(&self, p: DVec2, height: f64) -> DVec3 {
        self.origin + self.x * p.x + self.y * p.y + self.normal() * height
    }
}

/// The solid swept by `profile` on `frame` from height `from` to `to`
/// along the frame's normal, its faces named for `feature`. Not built
/// yet: a profile failing [`Profile::check`] gives its
/// [`KernelError::Profile`], any other [`KernelError::NotImplemented`].
pub fn extrude(
    profile: &Profile,
    _frame: &Frame,
    _from: f64,
    _to: f64,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, Failure> {
    profile.check().map_err(KernelError::Profile)?;
    Err(KernelError::NotImplemented("extruding").into())
}

/// Why a tapered extrude gives no solid.
#[derive(Debug, Clone, PartialEq)]
pub enum TaperError {
    /// Narrowing, the walls meet before the end of the span: a wall
    /// turned past its neighbour (a narrow slot tapered in) or a round
    /// shrunk to nothing.
    Closes,
    /// A wall can't lean that far: the corners where the drafted walls
    /// meet don't meet in one point, or a wall's drafted form can't be
    /// made at that angle.
    TooSteep,
    /// Widening, the solid would reach past [`MAX_COORD`](crate::MAX_COORD).
    OutOfRange,
    /// The kernel failed otherwise: the profile, out of budget, or a
    /// check.
    Failed(Failure),
}

impl std::fmt::Display for TaperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaperError::Closes => f.write_str("the taper closes the profile before its end"),
            TaperError::TooSteep => f.write_str("the taper is too steep for this profile"),
            TaperError::OutOfRange => f.write_str("the taper takes the solid out of range"),
            TaperError::Failed(failure) => write!(f, "{}", failure.error),
        }
    }
}

impl std::error::Error for TaperError {}

impl From<Failure> for TaperError {
    fn from(failure: Failure) -> Self {
        TaperError::Failed(failure)
    }
}

/// The solid [`extrude()`] makes of `profile` on `frame` from `from` to
/// `to`, with its walls leaning by `taper` (radians). Not built yet:
/// [`TaperError::Failed`] with the profile's [`KernelError::Profile`] as
/// [`extrude()`], else with [`KernelError::NotImplemented`].
#[allow(clippy::too_many_arguments)]
pub fn extrude_tapered(
    profile: &Profile,
    _frame: &Frame,
    _from: f64,
    _to: f64,
    _taper: f64,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, TaperError> {
    profile
        .check()
        .map_err(|e| TaperError::Failed(KernelError::Profile(e).into()))?;
    Err(TaperError::Failed(
        KernelError::NotImplemented("tapered extrudes").into(),
    ))
}
