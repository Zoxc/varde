//! The stand-in for the kernel's shell that tests run in its place
//! until the kernel's is built (see `crate::testing`).

use super::*;
use crate::history::in_place;

/// A stand-in for the kernel's shell, for tests: the solid must be a
/// box along the world's axes ([`in_place::box_sides`]), and is hollowed by one boolean with another box:
/// inward, the body less its box shrunk by the thickness (pushed out
/// past the open faces); outward, its box grown by the thickness (but
/// at the open faces) less the body's box pushed out past the open
/// faces. Anything else is [`KernelError::TooComplex`]; inward walls
/// meeting inside, or one as thick as the body is across (no hollow
/// left), is [`ShellError::TooThick`].
///
/// [`KernelError::TooComplex`]: varde_kernel_compat::KernelError::TooComplex
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_boxes(
    solid: &Solid,
    topology: &Topology,
    open: &[u32],
    thickness: f64,
    outward: bool,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, ShellError> {
    use glam::{DVec2, DVec3};
    use varde_kernel_compat::{Frame, KernelError, Loop, Op, Profile, Segment};
    let too_complex = || ShellError::Failed(KernelError::TooComplex.into());
    let (min, max, sides) = in_place::box_sides(solid, topology).ok_or_else(too_complex)?;
    let mut opened = [[false; 2]; 3];
    for &region in open {
        let (axis, end) = *sides.get(region as usize).ok_or_else(too_complex)?;
        opened[axis][end] = true;
    }
    let margin = thickness + 1.0;
    // The box from `lo` to `hi`, an extrude along z.
    let block = |lo: DVec3, hi: DVec3| -> Result<Solid, ShellError> {
        let corners = [
            DVec2::new(lo.x, lo.y),
            DVec2::new(hi.x, lo.y),
            DVec2::new(hi.x, hi.y),
            DVec2::new(lo.x, hi.y),
        ];
        let segments = (0..4)
            .map(|i| Segment::line(corners[i], corners[(i + 1) % 4], i as u64))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| too_complex())?;
        let profile = Profile {
            loops: vec![Loop { segments }],
        };
        Ok(varde_kernel_compat::extrude(
            &profile,
            &Frame::XY,
            lo.z,
            hi.z,
            feature,
            tol,
            budget,
        )?)
    };
    // The body's box moved by `by` at each kept side and pushed out past
    // each open one.
    let moved = |by: f64| {
        let (mut lo, mut hi) = (min, max);
        for axis in 0..3 {
            lo[axis] = if opened[axis][0] {
                min[axis] - margin
            } else {
                min[axis] - by
            };
            hi[axis] = if opened[axis][1] {
                max[axis] + margin
            } else {
                max[axis] + by
            };
        }
        (lo, hi)
    };
    let result = if outward {
        let (mut lo, mut hi) = moved(thickness);
        // The wall stops at the open faces.
        for axis in 0..3 {
            if opened[axis][0] {
                lo[axis] = min[axis];
            }
            if opened[axis][1] {
                hi[axis] = max[axis];
            }
        }
        let (in_lo, in_hi) = moved(0.0);
        let outer = block(lo, hi)?;
        varde_kernel_compat::boolean(&outer, &block(in_lo, in_hi)?, Op::Difference, tol, budget)?
    } else {
        let (lo, hi) = moved(-thickness);
        // The hollow must reach into the body on every axis: a wall as
        // thick as the body along one (a floor under an open top as
        // thick as the box is high) leaves no hollow, the walls run
        // into the open face.
        if (0..3).any(|axis| lo[axis].max(min[axis]) >= hi[axis].min(max[axis])) {
            return Err(ShellError::TooThick);
        }
        varde_kernel_compat::boolean(solid, &block(lo, hi)?, Op::Difference, tol, budget)?
    };
    Ok(result)
}
