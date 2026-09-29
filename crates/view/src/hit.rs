//! Hit testing the sketch being edited: what's under the cursor, and what
//! a box dragged on the screen selects. Pure functions, so they're tested
//! without a window.

use glam::DVec2;
use varde_sketch::{Curve, Id, Sketch};

use crate::projection::Projector;

/// What's under the cursor at `at`, in sketch coordinates, within
/// `tolerance` sketch units: the nearest point, the origin included,
/// or failing one, the nearest curve as drawn, or failing one, the
/// nearest axis. A point of the sketch's goes before the origin where
/// they're as near.
pub(crate) fn hit(sketch: &Sketch, at: DVec2, tolerance: f64) -> Option<Id> {
    if !at.is_finite() {
        return None;
    }
    let points = sketch
        .points
        .iter()
        .map(|point| (point.id, point.at.distance(at)))
        .chain([(Id::ORIGIN, at.length())]);
    let curves = sketch.curves.iter().filter_map(|entry| {
        let distance = curve_distance(sketch, &entry.curve, at)?;
        Some((entry.id, distance))
    });
    let axes = [(Id::X_AXIS, at.y.abs()), (Id::Y_AXIS, at.x.abs())];
    nearest(points, tolerance)
        .or_else(|| nearest(curves, tolerance))
        .or_else(|| nearest(axes.into_iter(), tolerance))
}

/// The curve under the cursor at `at`, as [`hit`] finds one, points and
/// axes aside: what Trim and Extend take.
pub(crate) fn hit_curve(sketch: &Sketch, at: DVec2, tolerance: f64) -> Option<Id> {
    if !at.is_finite() {
        return None;
    }
    let curves = sketch.curves.iter().filter_map(|entry| {
        let distance = curve_distance(sketch, &entry.curve, at)?;
        Some((entry.id, distance))
    });
    nearest(curves, tolerance)
}

/// The line or axis under the cursor at `at`, as [`hit`] finds one,
/// points and circles and arcs aside: what Mirror mirrors about.
pub(crate) fn hit_line(sketch: &Sketch, at: DVec2, tolerance: f64) -> Option<Id> {
    if !at.is_finite() {
        return None;
    }
    let lines = sketch.curves.iter().filter_map(|entry| match entry.curve {
        Curve::Line { .. } => Some((entry.id, curve_distance(sketch, &entry.curve, at)?)),
        _ => None,
    });
    let axes = [(Id::X_AXIS, at.y.abs()), (Id::Y_AXIS, at.x.abs())];
    nearest(lines, tolerance).or_else(|| nearest(axes.into_iter(), tolerance))
}

/// The point under the cursor at `at` where lines make a corner a fillet
/// or a chamfer can go on ([`Sketch::corner_lines`]), the nearest within
/// `tolerance`: what Fillet and Chamfer take.
pub(crate) fn hit_corner(sketch: &Sketch, at: DVec2, tolerance: f64) -> Option<Id> {
    if !at.is_finite() {
        return None;
    }
    // Only points near enough are asked about their lines.
    let corners = sketch
        .points
        .iter()
        .map(|point| (point.id, point.at.distance(at)))
        .filter(|&(_, distance)| distance <= tolerance)
        .filter(|&(id, _)| sketch.corner_lines(id, at).is_some());
    nearest(corners, tolerance)
}

/// The item of `items`, each with its distance, nearest and within
/// `tolerance`; the first of those equally near.
pub(crate) fn nearest(items: impl Iterator<Item = (Id, f64)>, tolerance: f64) -> Option<Id> {
    items
        .filter(|&(_, distance)| distance <= tolerance)
        .fold(None, |best: Option<(Id, f64)>, item| match best {
            Some(best) if best.1 <= item.1 => Some(best),
            _ => Some(item),
        })
        .map(|(id, _)| id)
}

/// How far `at` is from `curve` of `sketch`, as it's drawn: exactly for a
/// line and a circle, from the polyline for an arc, whose radius may
/// change along it (see [`Sketch::flatten`]).
fn curve_distance(sketch: &Sketch, curve: &Curve, at: DVec2) -> Option<f64> {
    let point = |id| sketch.point(id).map(|point| point.at);
    Some(match *curve {
        Curve::Line { start, end } => segment_distance(at, point(start)?, point(end)?),
        Curve::Circle { center, radius } => (at.distance(point(center)?) - radius).abs(),
        Curve::Arc { .. } | Curve::Spline(_) => sketch
            .flatten(curve)?
            .windows(2)
            .map(|pair| segment_distance(at, pair[0], pair[1]))
            .fold(f64::INFINITY, f64::min),
    })
}

/// How far `p` is from the segment from `a` to `b`.
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let length = ab.length_squared();
    let t = if length > 0.0 {
        ((p - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// What a box dragged on the screen selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxMode {
    /// What's wholly inside it: dragged from left to right.
    Inside,
    /// What it touches: dragged from right to left.
    Touching,
}

impl BoxMode {
    /// How a box dragged from `from` to `to` on the screen selects.
    pub(crate) fn dragged(from: DVec2, to: DVec2) -> Self {
        if to.x >= from.x {
            BoxMode::Inside
        } else {
            BoxMode::Touching
        }
    }
}

/// A box on the screen, in the viewport's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScreenBox {
    min: DVec2,
    max: DVec2,
}

impl ScreenBox {
    /// The box with corners `a` and `b`, in either order.
    pub(crate) fn new(a: DVec2, b: DVec2) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// Its corners, around it clockwise on the screen from the top left.
    pub(crate) fn corners(self) -> [DVec2; 4] {
        let (min, max) = (self.min, self.max);
        [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)]
    }

    pub(crate) fn contains(self, p: DVec2) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    /// Whether the segment from `a` to `b` passes through the box.
    fn meets(self, a: DVec2, b: DVec2) -> bool {
        self.clip(a, b).is_some()
    }

    /// The part of the segment from `a` to `b` inside the box, if any, by
    /// cutting it to the box's slabs in turn (Liang-Barsky). An end inside
    /// the box is kept as it is, so segments meeting there still meet.
    fn clip(self, a: DVec2, b: DVec2) -> Option<(DVec2, DVec2)> {
        if !(a.is_finite() && b.is_finite()) {
            return None;
        }
        let d = b - a;
        let (mut enter, mut leave) = (0.0f64, 1.0f64);
        for axis in 0..2 {
            let (low, high) = (self.min[axis] - a[axis], self.max[axis] - a[axis]);
            if d[axis] == 0.0 {
                if low > 0.0 || high < 0.0 {
                    return None;
                }
                continue;
            }
            let (t0, t1) = (low / d[axis], high / d[axis]);
            enter = enter.max(t0.min(t1));
            leave = leave.min(t0.max(t1));
        }
        let at = |t: f64| match t {
            0.0 => a,
            1.0 => b,
            t => a + d * t,
        };
        (enter <= leave).then(|| (at(enter), at(leave)))
    }
}

/// The items of `sketch`, shown by `projector`, that the box `area` on the
/// screen selects in `mode`: points inside it, and curves as drawn, wholly
/// inside it or touching it. What's behind the eye is never inside.
pub(crate) fn in_box(
    sketch: &Sketch,
    projector: &Projector,
    area: ScreenBox,
    mode: BoxMode,
) -> Vec<Id> {
    let points = sketch.points.iter().filter_map(|point| {
        let shown = projector.project(point.at)?;
        area.contains(shown).then_some(point.id)
    });
    let curves = sketch.curves.iter().filter_map(|entry| {
        let polyline = sketch.flatten(&entry.curve)?;
        let mut segments = polyline
            .windows(2)
            .map(|pair| projector.segment(pair[0], pair[1]));
        let selected = match mode {
            BoxMode::Inside => segments
                .all(|segment| segment.is_some_and(|(a, b)| area.contains(a) && area.contains(b))),
            BoxMode::Touching => {
                segments.any(|segment| segment.is_some_and(|(a, b)| area.meets(a, b)))
            }
        };
        selected.then_some(entry.id)
    });
    points.chain(curves).collect()
}

#[cfg(test)]
mod tests;
