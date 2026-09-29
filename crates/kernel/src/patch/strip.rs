//! Exact cylinder patches.
//!
//! A rational quadratic triangle lies on a quadric when its three boundary
//! conics lie on the quadric and their planes meet in one point `O` on it:
//! the patch is then the image of a flat triangle under the inverse of the
//! projection from `O`, which is quadratic. A straight ruling of a
//! cylinder, whose plane through `O` cuts the cylinder in two parallel
//! rulings, comes out parametrized linearly (control point at the
//! midpoint, weight 1) whatever `O` is, so strips next to each other can
//! share their rulings.

use glam::DVec3;

use super::{Conic3, Patch, PatchError};

/// How far `offset` must turn away from a straight bottom, or out of a
/// curved bottom's plane, as the sine of the angle between them.
const MIN_TILT: f64 = 1e-9;

/// The strip swept by moving `bottom` along `offset`, as two patches on
/// the (general) cylinder over the conic, exactly.
///
/// With `bottom` running from `a0` to `a1`, and the top edge
/// `bottom.translated(offset)` from `b0` to `b1`, the patches are
/// `(a0, a1, b1)` and `(a0, b1, b0)`. The rulings `a1–b1` and `b0–a0` are
/// straight. The diagonal from `a0` to `b1` is the bottom sheared along
/// the offset, `x ↦ x + h(x)·offset` with `h` affine and `0`, `½` and `1`
/// at `a0`, the control point and `a1`: control point `c + offset/2` and
/// the bottom's weight. It lies on the cylinder, and its plane meets the
/// bottom's plane in the line where `h = 0`, which cuts the bottom's conic
/// again at a point `O` away from the arc. So the three boundary planes of
/// the first patch meet at `O`, on the cylinder, and those of the second
/// at `O + offset`. Strips over the pieces of a split conic share their
/// rulings, to the bit. For a circular arc running counter-clockwise seen
/// from where `offset` points, the normals point out of the cylinder.
///
/// A straight `bottom` gives the two flat triangles of a parallelogram.
/// Refuses an `offset` along a straight bottom or in a curved bottom's
/// plane.
pub fn cylinder_strip(bottom: &Conic3, offset: DVec3) -> Result<[Patch; 2], PatchError> {
    bottom.check()?;
    let top = bottom.translated(offset);
    top.check()?;
    let (a0, ca, wa, a1) = (bottom.p0, bottom.c, bottom.w, bottom.p1);
    let (b0, b1) = (top.p0, top.p1);

    let chord = a1 - a0;
    // Also refuses a zero offset or chord.
    if chord.cross(offset).length() <= MIN_TILT * chord.length() * offset.length() {
        return Err(PatchError::Degenerate);
    }
    let plane = (ca - a0).cross(chord);
    if plane.dot(offset).abs() <= MIN_TILT * plane.length() * offset.length()
        && plane != DVec3::ZERO
    {
        return Err(PatchError::Degenerate);
    }

    let cd = ca + offset * 0.5;
    Ok([
        Patch::new([a0, a1, b1], [ca, (a1 + b1) * 0.5, cd], [wa, 1.0, wa])?,
        Patch::new([a0, b1, b0], [cd, top.c, (b0 + a0) * 0.5], [wa, wa, 1.0])?,
    ])
}
