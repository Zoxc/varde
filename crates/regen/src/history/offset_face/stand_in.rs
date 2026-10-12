//! The stand-in for the kernel's offset that tests run in its place
//! until the kernel's is built (see `crate::testing`).

use super::*;
use crate::history::in_place;

/// A stand-in for the kernel's offset face, for tests: the solid must be
/// a box along the world's axes ([`in_place::box_sides`]), and each face
/// picked moves along its normal by `distance`, the four around it
/// stretched to meet it: one scale along each axis and a move, so every
/// face keeps its name (as the kernel's keeps them). A face moved onto
/// or past the face opposite it (the walls between shrinking to
/// nothing) is [`OffsetError::PastNeighbour`], a box past
/// [`MAX_COORD`](varde_kernel::MAX_COORD) [`OffsetError::OutOfRange`];
/// anything else is [`KernelError::TooComplex`].
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_boxes(
    solid: &Solid,
    topology: &Topology,
    faces: &[u32],
    distance: f64,
    _tangent: bool,
    _feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, OffsetError> {
    use varde_kernel::KernelError;
    use varde_kernel::Motion;
    let too_complex = || OffsetError::Failed(KernelError::TooComplex.into());
    let (min, max, sides) = in_place::box_sides(solid, topology).ok_or_else(too_complex)?;
    let size = max - min;
    let (mut lo, mut hi) = (min, max);
    for &region in faces {
        let (axis, end) = *sides.get(region as usize).ok_or_else(too_complex)?;
        if end == 0 {
            lo[axis] = min[axis] - distance;
        } else {
            hi[axis] = max[axis] + distance;
        }
    }
    // The walls between a face and the one opposite it shrink to
    // nothing (the resolution is the least they may keep).
    let least = tol.resolution();
    for axis in 0..3 {
        if hi[axis] - lo[axis] <= least {
            let moved = (faces.iter())
                .copied()
                .find(|&region| sides[region as usize].0 == axis)
                .expect("only a moved face shrinks an axis");
            return Err(OffsetError::PastNeighbour { region: moved });
        }
    }
    let reach = f64::from(varde_kernel::MAX_COORD);
    if lo.abs().max_element() > reach || hi.abs().max_element() > reach {
        return Err(OffsetError::OutOfRange);
    }
    let factors = (hi - lo) / size;
    let motion = Motion::scale(min, factors)
        .zip(Motion::translation(lo - min))
        .map(|(scale, shift)| scale.then(&shift))
        .ok_or_else(too_complex)?;
    debug_assert!((motion.point(max) - hi).abs().max_element() <= 1e-9 * (1.0 + size.length()));
    Ok(solid.transformed(&motion, None, tol, budget)?)
}
