//! The stand-in for the kernel's sweep that tests run in its place
//! until the kernel's is built (see [`crate::testing`]).

use varde_kernel_compat::sweep::{self as path_sweep, Path, Piece, SweepError};
use varde_kernel_compat::{Budget, Frame, Profile, Solid, Tolerance};

/// A stand-in for the kernel's sweep, for tests: a path of straight
/// pieces all along one line, starting on the profile's plane and
/// square to it, untwisted, is the profile extruded along it, as far as
/// the pieces reach. Anything else is [`KernelError::TooComplex`].
///
/// [`KernelError::TooComplex`]: varde_kernel_compat::KernelError::TooComplex
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_extrude(
    profile: &Profile,
    frame: &Frame,
    path: &Path,
    _orientation: path_sweep::Orientation,
    twist: f64,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, SweepError> {
    use varde_kernel_compat::KernelError;
    let too_complex = || SweepError::Failed(KernelError::TooComplex.into());
    let Path::Chain {
        pieces,
        closed: false,
    } = path
    else {
        return Err(too_complex());
    };
    if twist != 0.0 {
        return Err(too_complex());
    }
    let mut ends = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let Piece::Line { from, to } = *piece else {
            return Err(too_complex());
        };
        ends.push((from, to));
    }
    let (Some(&(start, _)), Some(&(_, end))) = (ends.first(), ends.last()) else {
        return Err(too_complex());
    };
    let along = (end - start).try_normalize().ok_or_else(too_complex)?;
    let resolution = tol.resolution();
    for (k, &(from, to)) in ends.iter().enumerate() {
        let way = (to - from).try_normalize().ok_or_else(too_complex)?;
        if way.cross(along).length() > 1e-9 || way.dot(along) <= 0.0 {
            return Err(too_complex());
        }
        if k > 0 && from.distance(ends[k - 1].1) > resolution {
            return Err(too_complex());
        }
    }
    let normal = frame.normal();
    if (start - frame.origin).dot(normal).abs() > resolution {
        return Err(SweepError::OffStart);
    }
    if along.cross(normal).length() > path_sweep::JOINT_SINE {
        return Err(SweepError::NotSquare);
    }
    let length = (end - start).dot(normal);
    let (from, to) = if length > 0.0 {
        (0.0, length)
    } else {
        (length, 0.0)
    };
    Ok(varde_kernel_compat::extrude(
        profile, frame, from, to, feature, tol, budget,
    )?)
}
