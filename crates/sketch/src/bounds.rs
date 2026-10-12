//! Axis-aligned boxes as the curve maths pass them, `(min, max)`: their
//! overlap, a point's distance from one, their size and reach.

use glam::DVec2;

/// The box of `points`: inverted (an infinite `min`) if there are none.
pub(crate) fn of(points: impl IntoIterator<Item = DVec2>) -> (DVec2, DVec2) {
    (points.into_iter()).fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(min, max), p| {
        (min.min(p), max.max(p))
    })
}

/// The box holding both `a` and `b`.
pub(crate) fn union(a: (DVec2, DVec2), b: (DVec2, DVec2)) -> (DVec2, DVec2) {
    (a.0.min(b.0), a.1.max(b.1))
}

/// Whether the boxes `a` and `b`, each grown by `tolerance`, overlap: so
/// also where either isn't finite, which no test can rule out.
pub(crate) fn overlap(a: (DVec2, DVec2), b: (DVec2, DVec2), tolerance: f64) -> bool {
    !((a.0 - tolerance).cmpgt(b.1).any() || (b.0 - tolerance).cmpgt(a.1).any())
}

/// How far `at` is outside the box `(min, max)` along each axis: zero
/// where it's within it along that axis.
pub(crate) fn outside((min, max): (DVec2, DVec2), at: DVec2) -> DVec2 {
    (min - at).max(at - max).max(DVec2::ZERO)
}

/// The box's longest side.
pub(crate) fn size((min, max): (DVec2, DVec2)) -> f64 {
    (max - min).max_element()
}

/// The box's largest coordinate, either way from the origin.
pub(crate) fn reach((min, max): (DVec2, DVec2)) -> f64 {
    min.abs().max(max.abs()).max_element()
}
