//! Tapered extrudes: a profile swept along its frame's normal with its
//! walls leaning by an angle.
//!
//! **Not built yet**: [`extrude_tapered`] is a stand-in with the planned
//! signature that fails with [`KernelError::NotImplemented`] for any taper
//! but zero (which is [`extrude()`](super::extrude) exactly), so the
//! extrude's taper above the kernel (its document, regeneration,
//! messages and panel) is built against it. The plan: the walls are the
//! untapered extrude's, drafted ([`draft_faces`](crate::draft_faces)'s
//! drafted forms) with the neutral plane the profile's plane (height 0)
//! and the pull along the normal on each side of it, so a positive angle
//! narrows the profile away from the plane on both sides: lines give
//! planes, circular arcs exact cones, other conics fitted walls. For a
//! span across height 0 each wall is built in two pieces split there (a
//! ring of vertices on the plane, the walls behind it named
//! `BackSide { curve, segment }`, faces of their own as they meet the
//! front walls at a crease); a span not reaching height 0 turns its walls
//! about the hinge on the plane all the same, outside the solid. The
//! refusals are [`TaperError`]'s.

use super::{Frame, extrude};
use crate::{Budget, Failure, KernelError, Profile, Solid, Tolerance};

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

/// The solid [`extrude()`](super::extrude) makes of `profile` on `frame`
/// from `from` to `to`, with its walls leaning by `taper` (radians,
/// under a right angle either way; its sine and cosine from
/// [`trig`](crate::trig)): at height `h` the profile is offset inward by
/// `|h| tan taper` (outward for a negative taper), the walls turning
/// about where they meet height 0. Faces named as the extrude's; for a
/// span across height 0, the walls below it `BackSide`. A zero taper is
/// the extrude's solid exactly.
///
/// Not built yet: any other taper is [`TaperError::Failed`] with
/// [`KernelError::NotImplemented`].
#[allow(clippy::too_many_arguments)]
pub fn extrude_tapered(
    profile: &Profile,
    frame: &Frame,
    from: f64,
    to: f64,
    taper: f64,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, TaperError> {
    if taper == 0.0 {
        return Ok(extrude(profile, frame, from, to, feature, tol, budget)?);
    }
    Err(TaperError::Failed(
        KernelError::NotImplemented("a tapered extrude").into(),
    ))
}

#[cfg(test)]
mod tests;
