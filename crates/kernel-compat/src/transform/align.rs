//! The motion that aligns one body with another ([`Motion::align`]): a
//! point onto a point, and optionally a primary direction onto a
//! primary and a secondary onto a secondary.
//!
//! Each side's directions make an orthonormal frame by Gram–Schmidt
//! (`f1` the primary, `f2` the secondary less its part along `f1`, `f3 =
//! f1 × f2`), and the rotation is `R = F_t·F_mᵀ`, which takes the moved
//! side's frame onto the target's. Without a secondary both sides take
//! the cross product of the two primaries as their secondary, the axis
//! the smallest rotation taking one primary onto the other turns about,
//! which it leaves where it is. Where the primaries are within `1e-8`
//! (the sine) of parallel or opposite that cross product is lost in
//! rounding, and a direction square to the target's primary takes its
//! place, chosen as a sketch placement's x axis is (world X less its
//! part along the primary where that is within a nanoradian of world Z,
//! else `Z × d`): parallel, the rotation is then the smallest to within
//! the angle between them; opposite, it is the half turn about that
//! direction. Frames that agree to the bit give the identity.
//!
//! The motion is `x ↦ T·R·(x − p_m) + p_t + offset·d_t`, `d_t` the
//! target's primary as given (not turned round by the flip, so a positive
//! offset is a gap whichever way the two face, and a turn goes the same
//! way round), `T` the turn about it ([`Motion::turn`]'s matrix: quarter turns
//! exact, other angles through [`trig`](crate::trig)), kept as one
//! matrix `T·R` and one offset worked out from it, so the moved point
//! lands on the target within a few ulps of their coordinates. Units are
//! made by dividing by the length (after dividing by the largest
//! coordinate), never multiplying by its inverse, so a direction along a
//! world axis is that axis exactly; then every frame is a signed
//! permutation, `R` and quarter turns have only the entries `0` and `±1`,
//! and the motion is exact wherever the offset's arithmetic is.
//!
//! Only `+ − × ÷ √` and the trig helper; the one decision on geometry is
//! a secondary within `1e-9` (the sine) of its primary, which is refused
//! ([`AlignError::Parallel`]), stated as one.

use glam::{DMat3, DVec3};

use super::{Motion, outer};
use crate::MAX_COORD;

/// How near its primary, as the sine of the angle between them, a
/// secondary direction may come: nearer is refused as parallel.
pub const PARALLEL: f64 = 1e-9;

/// How near parallel or opposite, as the sine, two primaries must be for
/// their cross product to be replaced by a fixed direction square to the
/// target's (see the [module](self) docs). Not a decision on what the
/// motion is: either way it takes the primary onto the target's.
const LOST: f64 = 1e-8;

/// One side of an alignment: a point, and the directions picked with it
/// (any length). A secondary needs a primary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Datum {
    pub point: DVec3,
    pub primary: Option<DVec3>,
    pub secondary: Option<DVec3>,
}

impl Datum {
    /// A point alone.
    pub fn point(point: DVec3) -> Datum {
        Datum {
            point,
            primary: None,
            secondary: None,
        }
    }
}

/// How the moved side meets the target, besides the references:
/// whether the moved primary goes against the target's rather than along
/// it, a distance along the target's primary and a turn about it in
/// degrees (right-handed), both along and about it as given, whatever the
/// flip. The last two only with primaries.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AlignOptions {
    pub flip: bool,
    pub offset: f64,
    pub degrees: f64,
}

/// Why [`Motion::align`] makes no motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignError {
    /// A point isn't finite or is past [`MAX_COORD`].
    Point,
    /// A direction is zero or isn't finite.
    Direction,
    /// The sides' directions don't pair: a primary or a secondary on one
    /// side only, or a secondary without a primary.
    Unpaired,
    /// A secondary is within [`PARALLEL`] of its primary.
    Parallel,
    /// The offset isn't finite or is past [`MAX_COORD`], or the turn
    /// isn't finite, or either is given without primaries.
    Options,
}

impl std::fmt::Display for AlignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AlignError::Point => "a point is out of range",
            AlignError::Direction => "a direction has no length",
            AlignError::Unpaired => "the directions don't pair up",
            AlignError::Parallel => "the second direction is parallel to the first",
            AlignError::Options => "the offset or turn is out of range or has no direction",
        })
    }
}

impl std::error::Error for AlignError {}

impl Motion {
    /// The rigid motion taking `moved`'s point onto `target`'s, its
    /// primary along the target's (against it with `options.flip`), its
    /// secondary's part square to its primary along the target's, then
    /// moved `options.offset` along the target's primary and turned
    /// `options.degrees` about it (right-handed), both as the target's
    /// primary is given, unflipped: see the
    /// `transform/align.rs` docs. Without a secondary the rotation is the
    /// smallest taking one primary onto the other; without primaries the
    /// motion is a move.
    pub fn align(
        moved: &Datum,
        target: &Datum,
        options: &AlignOptions,
    ) -> Result<Motion, AlignError> {
        let limit = f64::from(MAX_COORD);
        let in_range = |p: DVec3| p.is_finite() && p.abs().max_element() <= limit;
        if !in_range(moved.point) || !in_range(target.point) {
            return Err(AlignError::Point);
        }
        let AlignOptions {
            flip,
            offset,
            degrees,
        } = *options;
        if !(offset.is_finite() && offset.abs() <= limit && degrees.is_finite()) {
            return Err(AlignError::Options);
        }
        let (primaries, secondaries) = match (moved, target) {
            (
                Datum {
                    primary: Some(pm),
                    secondary: sm,
                    ..
                },
                Datum {
                    primary: Some(pt),
                    secondary: st,
                    ..
                },
            ) => match (sm, st) {
                (Some(sm), Some(st)) => ([*pm, *pt], Some([*sm, *st])),
                (None, None) => ([*pm, *pt], None),
                _ => return Err(AlignError::Unpaired),
            },
            (
                Datum {
                    primary: None,
                    secondary: None,
                    ..
                },
                Datum {
                    primary: None,
                    secondary: None,
                    ..
                },
            ) => {
                if offset != 0.0 || degrees != 0.0 {
                    return Err(AlignError::Options);
                }
                let shift = target.point - moved.point;
                return Ok(Motion::translation(shift + DVec3::ZERO).expect("within range"));
            }
            _ => return Err(AlignError::Unpaired),
        };
        let fm = unit(primaries[0]).ok_or(AlignError::Direction)?;
        // The target's own primary, which the offset and turn go along
        // and about whatever the flip.
        let own = unit(primaries[1]).ok_or(AlignError::Direction)?;
        let ft = if flip { -own } else { own };
        let [sm, st] = match secondaries {
            Some([sm, st]) => [
                unit(sm).ok_or(AlignError::Direction)?,
                unit(st).ok_or(AlignError::Direction)?,
            ],
            None => {
                let axis = fm.cross(ft);
                let axis = if axis.length_squared() >= LOST * LOST {
                    axis
                } else {
                    square_to(ft)
                };
                [axis, axis]
            }
        };
        let from = frame(fm, sm).ok_or(AlignError::Parallel)?;
        let to = frame(ft, st).ok_or(AlignError::Parallel)?;
        let rotation = if from == to {
            DMat3::IDENTITY
        } else {
            outer(to[0], from[0]) + outer(to[1], from[1]) + outer(to[2], from[2])
        };
        let linear = if degrees == 0.0 {
            rotation
        } else {
            let turn = Motion::turn(DVec3::ZERO, own, degrees).ok_or(AlignError::Options)?;
            turn.linear * rotation
        };
        // Adding zero turns −0 into +0, so equal motions have equal bits.
        let linear = DMat3::from_cols(
            linear.x_axis + DVec3::ZERO,
            linear.y_axis + DVec3::ZERO,
            linear.z_axis + DVec3::ZERO,
        );
        let lands = if offset == 0.0 {
            target.point
        } else {
            target.point + own * offset
        };
        let shift = lands - linear * moved.point + DVec3::ZERO;
        if !shift.is_finite() {
            return Err(AlignError::Point);
        }
        Ok(Motion {
            linear,
            normal: linear,
            offset: shift,
            ..Motion::IDENTITY
        })
    }
}

/// `v` scaled to unit length by dividing by its length, after dividing by
/// its largest coordinate (so its square neither overflows nor
/// underflows, and a vector along a world axis is that axis exactly);
/// `None` for a zero or non-finite vector.
fn unit(v: DVec3) -> Option<DVec3> {
    let largest = v.abs().max_element();
    if !v.is_finite() || largest == 0.0 {
        return None;
    }
    let v = v / largest;
    Some(v / v.length())
}

/// A direction square to the unit `d`, as a sketch placement on a plane
/// of normal `d` chooses its x axis: world X less its part along `d`
/// where `d` is within a nanoradian of world Z (`d_x² + d_y² ≤ 1e-18`),
/// else `Z × d`. Not unit.
fn square_to(d: DVec3) -> DVec3 {
    let tilt = d.x * d.x + d.y * d.y;
    if tilt <= 1e-18 {
        DVec3::new(d.y * d.y + d.z * d.z, 0.0 - d.x * d.y, 0.0 - d.x * d.z)
    } else {
        DVec3::new(0.0 - d.y, d.x, 0.0)
    }
}

/// The right-handed orthonormal frame of the unit `f1` and `s`'s part
/// square to it (taken off twice, so what's left is square to rounding),
/// or `None` where that part is within [`PARALLEL`] of nothing (the sine
/// of the angle between `s` and `f1`).
fn frame(f1: DVec3, s: DVec3) -> Option<[DVec3; 3]> {
    let s = unit(s)?;
    let rest = s - f1 * s.dot(f1);
    if rest.length_squared() <= PARALLEL * PARALLEL {
        return None;
    }
    let rest = rest - f1 * rest.dot(f1);
    let f2 = unit(rest)?;
    Some([f1, f2, f1.cross(f2)])
}

#[cfg(test)]
mod tests;
