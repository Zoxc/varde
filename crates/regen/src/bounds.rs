//! The numbers an answer's tables and measures may hold: coordinates
//! within [`Picking::MAX_VALUE`] (so finite), directions unit vectors
//! within [`Picking::UNIT`]. Each check on a [`Summary`](crate::Summary),
//! a measure or an edge's facts is built of these.

use glam::DVec3;

use crate::Picking;

/// Whether `x` is a coordinate the answer may hold.
pub(crate) fn value(x: f64) -> bool {
    x.abs() <= Picking::MAX_VALUE
}

/// Whether `p`'s coordinates are ones the answer may hold.
pub(crate) fn point(p: [f64; 3]) -> bool {
    p.into_iter().all(value)
}

/// [`point`] of a [`DVec3`].
pub(crate) fn point_dvec(p: DVec3) -> bool {
    point(p.to_array())
}

/// Whether `v` is a unit vector, within [`Picking::UNIT`].
pub(crate) fn unit(v: [f64; 3]) -> bool {
    (DVec3::from(v).length() - 1.0).abs() <= Picking::UNIT
}

/// Whether `x` is a size that may be zero (a length, an area, a volume).
pub(crate) fn non_negative(x: f64) -> bool {
    x >= 0.0 && value(x)
}

/// Whether `x` is a size that may not be zero (a radius).
pub(crate) fn positive(x: f64) -> bool {
    x > 0.0 && value(x)
}

/// Whether `x` is within `(0, 1]` (a cosine or sine of an acute angle).
pub(crate) fn ratio(x: f64) -> bool {
    x > 0.0 && x <= 1.0
}
