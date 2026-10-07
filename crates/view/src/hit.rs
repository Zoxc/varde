//! Hit testing the sketch being edited: what's under the cursor, and what
//! a box dragged on the screen selects. Pure functions, so they're tested
//! without a window.

use glam::DVec2;
use varde_sketch::{Curve, Id, Selectable, Sketch};

use crate::projection::Projector;

/// What's under the cursor at `at`, in sketch coordinates, within
/// `tolerance` sketch units: the nearest point, the origin included,
/// or failing one, the nearest handle end ([`handle_ends`]: its tip, or
/// its mirrored end, [`Selectable::HandleEnd`]), over an arm within
/// [`END_REACH`] times the tolerance,
/// or failing one, the nearest spline handle as drawn
/// ([`Selectable::HandleLine`], see [`handles`]), so a handle along its
/// spline is the handle, or
/// failing one, the nearest curve as drawn, or failing one, the nearest
/// axis. A point of the sketch's goes before the origin where
/// they're as near.
pub(crate) fn hit(sketch: &Sketch, at: DVec2, tolerance: f64) -> Option<Selectable> {
    if !at.is_finite() {
        return None;
    }
    let points = sketch
        .points
        .iter()
        .map(|point| (point.id.into(), point.at.distance(at)))
        .chain([(Id::ORIGIN.into(), at.length())]);
    let curves = sketch.curves.iter().filter_map(|entry| {
        let distance = curve_distance(sketch, &entry.curve, at)?;
        Some((entry.id.into(), distance))
    });
    let axes = [
        (Id::X_AXIS.into(), at.y.abs()),
        (Id::Y_AXIS.into(), at.x.abs()),
    ];
    nearest(points, tolerance)
        .or_else(|| nearest(handle_ends(sketch, at).into_iter(), tolerance))
        .or_else(|| {
            // Over an arm, an end reaches farther.
            let arm = nearest(handles(sketch, at).into_iter(), tolerance)?;
            nearest(handle_ends(sketch, at).into_iter(), END_REACH * tolerance).or(Some(arm))
        })
        .or_else(|| nearest(curves, tolerance))
        .or_else(|| nearest(axes.into_iter(), tolerance))
}

/// Everything under the cursor at `at` within `tolerance` sketch units,
/// as [`hit`] would find each on its own: the points, the origin
/// included, then the handles' mirrored ends, then the handles, then the
/// curves, then the axes, each nearest first.
pub(crate) fn overlaps(sketch: &Sketch, at: DVec2, tolerance: f64) -> Vec<Selectable> {
    if !at.is_finite() {
        return Vec::new();
    }
    let points: Vec<_> = (sketch.points.iter())
        .map(|point| (point.id.into(), point.at.distance(at)))
        .chain([(Id::ORIGIN.into(), at.length())])
        .collect();
    let mut ends = handle_ends(sketch, at);
    // The tips are among the points.
    ends.retain(|(target, _)| target.item().is_none());
    let curves: Vec<_> = (sketch.curves.iter())
        .filter_map(|entry| Some((entry.id.into(), curve_distance(sketch, &entry.curve, at)?)))
        .collect();
    let axes = vec![
        (Id::X_AXIS.into(), at.y.abs()),
        (Id::Y_AXIS.into(), at.x.abs()),
    ];
    [points, ends, handles(sketch, at), curves, axes]
        .into_iter()
        .flat_map(|mut kind| {
            kind.retain(|&(_, distance)| distance <= tolerance);
            kind.sort_by(|a, b| a.1.total_cmp(&b.1));
            kind.into_iter().map(|(id, _)| id)
        })
        .collect()
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

/// The line or axis under the cursor at `at`, the nearest within
/// `tolerance`, construction lines included: what a revolve turns about.
/// Unlike [`hit_line`], an axis counts only within `reach` of the origin,
/// as far as it's drawn, and a line with both ends at one point not at
/// all: it has no direction to turn about.
pub(crate) fn hit_axis(sketch: &Sketch, at: DVec2, tolerance: f64, reach: f64) -> Option<Id> {
    if !at.is_finite() {
        return None;
    }
    let lines = sketch.curves.iter().filter_map(|entry| match entry.curve {
        Curve::Line { start, end } if sketch.point(start)?.at != sketch.point(end)?.at => {
            Some((entry.id, curve_distance(sketch, &entry.curve, at)?))
        }
        _ => None,
    });
    let axes = [(Id::X_AXIS, DVec2::X), (Id::Y_AXIS, DVec2::Y)]
        .map(|(id, along)| (id, segment_distance(at, -along * reach, along * reach)));
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
pub(crate) fn nearest<T: Copy>(items: impl Iterator<Item = (T, f64)>, tolerance: f64) -> Option<T> {
    items
        .filter(|&(_, distance)| distance <= tolerance)
        .fold(None, |best: Option<(T, f64)>, item| match best {
            Some(best) if best.1 <= item.1 => Some(best),
            _ => Some(item),
        })
        .map(|(id, _)| id)
}

/// How far `at` is from `curve` of `sketch`, as it's drawn: exactly for a
/// line and a circle, from the polyline for an arc, whose radius may
/// change along it (see [`Sketch::flatten`]).
pub(crate) fn curve_distance(sketch: &Sketch, curve: &Curve, at: DVec2) -> Option<f64> {
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

/// The splines' handles, each as a line ([`Selectable::HandleLine`]), and how far
/// `at` is from it as drawn: from its tip through its fit point to as
/// far the other side.
fn handles(sketch: &Sketch, at: DVec2) -> Vec<(Selectable, f64)> {
    let mut found = Vec::new();
    for (_, spline) in sketch.splines() {
        for handle in &spline.handles {
            if let (Some(from), Some(tip)) = (sketch.point(handle.at), sketch.point(handle.tip)) {
                let (from, tip) = (from.at, tip.at);
                let distance = segment_distance(at, 2.0 * from - tip, tip);
                found.push((Selectable::HandleLine(handle.tip), distance));
            }
        }
    }
    found
}

/// How many times the tolerance a handle's end reaches, before its arm:
/// along the arm the cursor nears the end, so with the arm's reach alone
/// the arm is hit until the cursor is all but on the end.
const END_REACH: f64 = 2.0;

/// The ends of the splines' handles, and how far `at` is from each: its tip,
/// as the item it is, and as far the other side of its fit point, its
/// mirrored end ([`Selectable::HandleEnd`]), which is no point.
fn handle_ends(sketch: &Sketch, at: DVec2) -> Vec<(Selectable, f64)> {
    let mut found = Vec::new();
    for (_, spline) in sketch.splines() {
        for handle in &spline.handles {
            if let (Some(from), Some(tip)) = (sketch.point(handle.at), sketch.point(handle.tip)) {
                let (from, tip) = (from.at, tip.at);
                found.push((handle.tip.into(), at.distance(tip)));
                found.push((
                    Selectable::HandleEnd(handle.tip),
                    at.distance(2.0 * from - tip),
                ));
            }
        }
    }
    found
}

/// How far `p` is from the segment from `a` to `b`.
pub(crate) fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
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
/// screen selects in `mode`: points inside it, handles' mirrored ends as
/// the points they're shown as, and curves as drawn, wholly inside it or
/// touching it. What's behind the eye is never inside.
pub(crate) fn in_box(
    sketch: &Sketch,
    projector: &Projector,
    area: ScreenBox,
    mode: BoxMode,
) -> Vec<Selectable> {
    let points = sketch.points.iter().filter_map(|point| {
        let shown = projector.project(point.at)?;
        area.contains(shown).then_some(point.id.into())
    });
    let ends = (sketch.splines())
        .flat_map(|(_, spline)| &spline.handles)
        .filter_map(|handle| {
            let shown = projector.project(sketch.handle_end(handle.tip)?)?;
            area.contains(shown)
                .then_some(Selectable::HandleEnd(handle.tip))
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
        selected.then_some(entry.id.into())
    });
    points.chain(ends).chain(curves).collect()
}

#[cfg(test)]
mod tests;
