//! The stand-in for the kernel's loft that tests run in its place
//! until the kernel's is built (see [`crate::testing`]).

use varde_kernel_compat::loft::{self as kernel_loft, LoftError, Rail};
use varde_kernel_compat::{Budget, Solid, Tolerance};

/// A stand-in for the kernel's loft, for tests: two open sections, no
/// rails, each a loop of straight segments, the second the first moved
/// along its plane's normal (vertex for vertex within the resolution,
/// either way round) is the first extruded to the second, its faces
/// named as an extrude's. The second's start (its given one, or the
/// vertex nearest the first's start) must be the first's moved: another
/// is [`LoftError::Twists`], as the kernel's would twist. Anything else
/// is [`KernelError::TooComplex`].
///
/// [`KernelError::TooComplex`]: varde_kernel_compat::KernelError::TooComplex
pub(crate) fn by_extrude(
    sections: &[kernel_loft::Section],
    _mode: kernel_loft::LoftMode,
    closed: bool,
    rails: &[Rail],
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, LoftError> {
    use glam::DVec3;
    use varde_kernel_compat::{KernelError, Profile};
    let too_complex = || LoftError::Failed(KernelError::TooComplex.into());
    let resolution = tol.resolution();
    let [first, second] = sections else {
        return Err(too_complex());
    };
    let (
        kernel_loft::Section::Loop {
            outline,
            frame,
            start: a_start,
        },
        kernel_loft::Section::Loop { start: b_start, .. },
    ) = (first, second)
    else {
        return Err(too_complex());
    };
    if closed || !rails.is_empty() {
        return Err(too_complex());
    }
    let straight = |section: &kernel_loft::Section| match section {
        kernel_loft::Section::Loop { outline, .. } => outline.segments.iter().all(|segment| {
            let c = segment.conic;
            let chord = c.p1 - c.p0;
            (c.c - c.p0).perp_dot(chord).abs() <= resolution * chord.length()
        }),
        kernel_loft::Section::Point(_) => false,
    };
    let (a, b) = (first.vertices(), second.vertices());
    let n = a.len();
    if !straight(first) || !straight(second) || b.len() != n || n < 3 {
        return Err(too_complex());
    }
    let normal = frame.normal();
    let height = (b[0] - frame.origin).dot(normal);
    if (b.iter()).any(|&p| ((p - frame.origin).dot(normal) - height).abs() > resolution) {
        return Err(too_complex());
    }
    let shift = normal * height;
    let a_start = a_start.unwrap_or(0) % n;
    let nearest = |to: DVec3| {
        (0..n)
            .min_by(|&i, &j| {
                b[i].distance_squared(to)
                    .total_cmp(&b[j].distance_squared(to))
            })
            .unwrap_or(0)
    };
    let b_start = b_start.map_or_else(|| nearest(a[a_start]), |start| start % n);
    // Whether `b` from `at`, `step` (1 or n − 1) a vertex, is `a` from
    // its start moved.
    let matches = |at: usize, step: usize| {
        (0..n).all(|i| {
            let moved = a[(a_start + i) % n] + shift;
            b[(at + i * step) % n].distance(moved) <= resolution
        })
    };
    if !(matches(b_start, 1) || matches(b_start, n - 1)) {
        let elsewhere = (0..n).any(|at| matches(at, 1) || matches(at, n - 1));
        return Err(if elsewhere {
            LoftError::Twists
        } else {
            too_complex()
        });
    }
    let profile = Profile {
        loops: vec![outline.clone()],
    };
    let (from, to) = if height > 0.0 {
        (0.0, height)
    } else {
        (height, 0.0)
    };
    Ok(varde_kernel_compat::extrude(
        &profile, frame, from, to, feature, tol, budget,
    )?)
}
