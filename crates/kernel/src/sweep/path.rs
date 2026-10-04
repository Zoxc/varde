//! Path sweeps: a profile moved along a path of pieces in order, or
//! carried round a helix.
//!
//! **Not built yet**: [`sweep`] is a stand-in with the planned signature
//! that fails with [`KernelError::TooComplex`], so the sweep feature
//! above the kernel (its document, regeneration and messages) is built
//! against it. The real implementation replaces this file (and adds
//! what it needs beside it, under `sweep/`): each piece built from the
//! section the piece before ended with (lines as extrude walls, exact;
//! circular arcs as revolve pieces about the arc's axis moved to the
//! section; curve pieces by stations, halved until the strips fit, the
//! frame rotation-minimizing: a plane curve's `T`, the plane's normal and
//! `T × N` with the roll carried in, a curve out of any one plane by
//! double reflection), the sections exact copies of the profile's conics
//! and the rails and diagonals fitted against the swept surface within
//! half the fit tolerance; closed paths built round to the first section
//! (a turn left out of one plane spread back as a twist); keep
//! orientation (straight profile edges sweeping exact cylinders over the
//! path's conics); a twist; a helix by its screw motion, every station
//! the profile moved directly from its angle; caps and names; and the
//! refusals [`SweepError`] lists.
//!
//! The path is in the world, built by the caller (regeneration) from
//! what the feature names: sketch chains placed by their sketches, model
//! edges found on their bodies, joined end to end and ordered from their
//! end on the profile's plane, or a helix's axis. The kernel knows
//! nothing of sketches or bodies.

use glam::DVec3;

use crate::patch::Conic3;
use crate::{Budget, Failure, Frame, KernelError, Profile, Solid, Tolerance};

/// One piece of a path, in the world, running from its start to its end.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    /// A straight run from `from` to `to`.
    Line { from: DVec3, to: DVec3 },
    /// A circular arc: its `conics` end to end (each at most a quarter
    /// turn), about `centre`, turning right-handed about the unit `axis`.
    Arc {
        conics: Vec<Conic3>,
        centre: DVec3,
        axis: DVec3,
    },
    /// Any other curve: its `conics` end to end (a spline's fit, an
    /// ellipse, a chain a boolean traced), with the unit normal of the
    /// plane it lies in where that's known (a sketch's), `None` where it
    /// isn't (a model edge).
    Curve {
        conics: Vec<Conic3>,
        normal: Option<DVec3>,
    },
}

impl Piece {
    /// Where it starts. `None` for an arc or a curve of no conics.
    pub fn start(&self) -> Option<DVec3> {
        match self {
            Piece::Line { from, .. } => Some(*from),
            Piece::Arc { conics, .. } | Piece::Curve { conics, .. } => conics.first().map(|c| c.p0),
        }
    }

    /// Where it ends. `None` for an arc or a curve of no conics.
    pub fn end(&self) -> Option<DVec3> {
        match self {
            Piece::Line { to, .. } => Some(*to),
            Piece::Arc { conics, .. } | Piece::Curve { conics, .. } => conics.last().map(|c| c.p1),
        }
    }
}

/// A helix: the axis through `point` along the unit `axis`, climbing
/// `pitch` (in model units, positive) a turn along it for `turns` turns
/// (positive), counter-clockwise seen from the tip of `axis` (right-handed)
/// unless `left`. It needs no radius: each point of the profile runs on
/// a helix of its own about the axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Helix {
    pub point: DVec3,
    pub axis: DVec3,
    pub pitch: f64,
    pub turns: f64,
    pub left: bool,
}

/// What a profile is swept along.
#[derive(Debug, Clone, PartialEq)]
pub enum Path {
    /// Pieces end to end, in order, tangent-continuous at their joints,
    /// the first starting on the profile's plane, square to it. A closed
    /// chain's last piece ends where the first starts; it starts anywhere
    /// (the kernel finds where the profile's plane crosses it).
    Chain { pieces: Vec<Piece>, closed: bool },
    /// A helix, alone; the profile's plane holds its axis.
    Helix(Helix),
}

/// How the section is carried along a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Turned with the path, by rotation-minimizing frames: the section
    /// stays square to it and doesn't spin about it.
    Follow,
    /// Only moved along it, never turned.
    Keep,
}

/// Why a sweep gives no solid. Pieces are named by their index in the
/// path's chain.
#[derive(Debug, Clone, PartialEq)]
pub enum SweepError {
    /// Two pieces meet at `at` with tangents more than a sine of `1e-6`
    /// apart, or one doesn't start where the one before ends.
    Corner { at: DVec3 },
    /// The path doesn't start on the profile's plane.
    OffStart,
    /// The profile's plane isn't square to the path at its start.
    NotSquare,
    /// Piece `piece` bends tighter than the profile: the section would
    /// reach its arc's axis or its curve's centre of curvature.
    TooTight { piece: u32 },
    /// With the orientation kept, the path turns parallel to the
    /// profile's plane.
    Parallel,
    /// The profile's plane doesn't hold the helix's axis.
    HelixPlane,
    /// The profile reaches the helix's axis.
    ReachesAxis,
    /// The profile is longer along the helix's axis than its pitch:
    /// neighbouring turns would meet.
    Pitch,
    /// The swept volume runs into itself.
    IntoItself,
    /// The kernel failed otherwise: out of budget, or its checks.
    Failed(Failure),
}

impl std::fmt::Display for SweepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SweepError::Corner { at } => write!(f, "the path has a corner at {at}"),
            SweepError::OffStart => f.write_str("the path doesn't start on the profile's plane"),
            SweepError::NotSquare => f.write_str("the profile isn't square to the path"),
            SweepError::TooTight { piece } => {
                write!(f, "piece {piece} bends tighter than the profile")
            }
            SweepError::Parallel => f.write_str("the path turns parallel to the profile"),
            SweepError::HelixPlane => f.write_str("the profile's plane doesn't hold the axis"),
            SweepError::ReachesAxis => f.write_str("the profile reaches the helix's axis"),
            SweepError::Pitch => f.write_str("the pitch is smaller than the profile"),
            SweepError::IntoItself => f.write_str("the sweep runs into itself"),
            SweepError::Failed(failure) => write!(f, "{}", failure.error),
        }
    }
}

impl std::error::Error for SweepError {}

impl From<Failure> for SweepError {
    fn from(failure: Failure) -> Self {
        SweepError::Failed(failure)
    }
}

/// `profile`, on the plane of `frame`, swept along `path`, carried as
/// `orientation` says and turned by `twist` radians about the path over
/// its length (none with a helix), the faces named for `feature`. Not
/// built yet: always [`SweepError::Failed`] with
/// [`KernelError::TooComplex`].
#[allow(clippy::too_many_arguments)]
pub fn sweep(
    _profile: &Profile,
    _frame: &Frame,
    _path: &Path,
    _orientation: Orientation,
    _twist: f64,
    _feature: u64,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, SweepError> {
    Err(SweepError::Failed(KernelError::TooComplex.into()))
}

#[cfg(test)]
mod tests;
