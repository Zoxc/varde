//! Plane geometry the drawing tools build shapes from, and that the
//! sketch, its dimensions and the view's snapping and glyphs share.

use std::f64::consts::TAU;

use glam::DVec2;

use crate::angle;

/// An arc by its centre and its ends, counter-clockwise from `start` to
/// `end`, as [`Curve::Arc`](crate::Curve::Arc) keeps one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcPoints {
    pub center: DVec2,
    pub start: DVec2,
    pub end: DVec2,
}

/// The arc from `a` to `b` through `through`: the circle through the
/// three, and `a` and `b` as its ends, swapped where needed so it runs
/// counter-clockwise from `start` to `end` and passes `through`. `None`
/// if the three are on one line, two of them the same point included,
/// which no circle passes through, or its centre isn't finite.
pub fn arc_through(a: DVec2, b: DVec2, through: DVec2) -> Option<ArcPoints> {
    let (u, v) = (b - a, through - a);
    // Twice the signed area of (a, b, through): positive when `through`
    // is left of a → b, so the counter-clockwise arc from `b` to `a`
    // passes it.
    let area = u.perp_dot(v);
    if area == 0.0 {
        return None;
    }
    let d = 2.0 * area;
    let (uu, vv) = (u.length_squared(), v.length_squared());
    let center = a + DVec2::new(v.y * uu - u.y * vv, u.x * vv - v.x * uu) / d;
    if !center.is_finite() {
        return None;
    }
    let (start, end) = if area > 0.0 { (b, a) } else { (a, b) };
    Some(ArcPoints { center, start, end })
}

/// The angle turned counter-clockwise from the direction `from` to the
/// direction `to`, in `(0, 2π]`: the same direction is a whole turn, as
/// an arc whose ends are is.
pub fn arc_sweep(from: DVec2, to: DVec2) -> f64 {
    TAU - (angle::to_angle(from) - angle::to_angle(to)).rem_euclid(TAU)
}

/// Where `point` is nearest on the endless line through `start` and
/// `end`. Not a number for a line of no length.
pub fn foot(point: DVec2, start: DVec2, end: DVec2) -> DVec2 {
    let along = end - start;
    let t = along.dot(point - start) / along.length_squared();
    start + along * t
}

/// Where the endless lines `p + r t` and `q + s u` cross, as `(t, u)`.
/// `None` where they're parallel, or so near it (within about 10⁻⁹
/// radians) that where they cross is far off or nowhere.
pub fn crossing(p: DVec2, r: DVec2, q: DVec2, s: DVec2) -> Option<(f64, f64)> {
    let across = r.perp_dot(s);
    if across.abs() <= 1e-9 * r.length() * s.length() {
        return None;
    }
    let (t, u) = ((q - p).perp_dot(s) / across, (q - p).perp_dot(r) / across);
    (t.is_finite() && u.is_finite()).then_some((t, u))
}

#[cfg(test)]
mod tests;
