//! Curves as polylines, for drawing.

use std::f64::consts::TAU;

use glam::DVec2;

use crate::angle;

use crate::intersect::Geom;
use crate::{Curve, Sketch, arc_sweep};

/// How many segments a circle is flattened into. An arc gets its share of
/// them by the angle it sweeps, at least one, so no curve flattens to more
/// than this many segments, whatever a file holds. Off the true circle by
/// about a thousandth of the radius, under a pixel for a circle filling
/// the screen.
pub const CIRCLE_SEGMENTS: usize = 64;

impl Sketch {
    /// `curve` as a polyline in sketch coordinates, through its ends: a
    /// line as its two points, a circle closed, starting and ending at the
    /// same point, and an arc counter-clockwise from its start to its end,
    /// a whole turn if they're in the same direction from its centre. At
    /// most [`CIRCLE_SEGMENTS`] segments, and at least one. A spline's
    /// Bézier segments each in as many as keep it within a small share of
    /// its size, at most [`CIRCLE_SEGMENTS`] each, closed if it is; one
    /// with no shape (its points all at one place) as its points joined
    /// up.
    ///
    /// An arc's radius is its start's distance from its centre, which its
    /// end's is to equal; until the solver holds that, the radius changes
    /// evenly from one to the other along the arc, so the polyline still
    /// meets both ends.
    ///
    /// `None` if the curve names a point the sketch doesn't have, which
    /// [`Sketch::check`] rules out.
    pub fn flatten(&self, curve: &Curve) -> Option<Vec<DVec2>> {
        let at = |id| self.point(id).map(|point| point.at);
        Some(match curve {
            &Curve::Line { start, end } => vec![at(start)?, at(end)?],
            &Curve::Circle { center, radius } => flatten_circle(at(center)?, radius),
            &Curve::Arc { center, start, end } => flatten_arc(at(center)?, at(start)?, at(end)?),
            Curve::Spline(spline) => match Geom::of(self, curve) {
                Some(Geom::Spline(path)) => path.flatten(),
                _ => {
                    let mut points = self.places(&spline.points)?;
                    if spline.closed {
                        points.push(points[0]);
                    }
                    points
                }
            },
        })
    }
}

/// The line from `start` to `end` in parts, by what a corner's fillet or
/// chamfer keeps of it, `kept` (see [`Sketch::cut_back`]): the part kept,
/// if anything is, and the ends cut off, each from the kept part out.
pub fn cut_line(
    start: DVec2,
    end: DVec2,
    [from, to]: [f64; 2],
) -> (Option<[DVec2; 2]>, Vec<[DVec2; 2]>) {
    let at = |u: f64| start.lerp(end, u);
    if from >= to {
        return (None, vec![[start, end]]);
    }
    let kept = [at(from), at(to)];
    let mut cut = Vec::new();
    if from > 0.0 {
        cut.push([kept[0], start]);
    }
    if to < 1.0 {
        cut.push([kept[1], end]);
    }
    (Some(kept), cut)
}

/// The circle around `center` of `radius` as a closed polyline of
/// [`CIRCLE_SEGMENTS`] segments, counter-clockwise from its rightmost
/// point, as [`Sketch::flatten`] draws a circle.
pub fn flatten_circle(center: DVec2, radius: f64) -> Vec<DVec2> {
    let mut points: Vec<_> = (0..CIRCLE_SEGMENTS)
        .map(|i| center + radius * angle::from_angle(TAU * i as f64 / CIRCLE_SEGMENTS as f64))
        .collect();
    points.push(points[0]);
    points
}

/// The arc around `center` counter-clockwise from `start` to `end` as a
/// polyline through both, as [`Sketch::flatten`] draws an arc: a whole
/// turn if they're in the same direction from `center`, the radius
/// changing evenly from one end's to the other's.
pub fn flatten_arc(center: DVec2, start: DVec2, end: DVec2) -> Vec<DVec2> {
    let (from, to) = (start - center, end - center);
    let begin = angle::to_angle(from);
    let sweep = arc_sweep(from, to);
    let segments = arc_segments(sweep);
    let (r0, r1) = (from.length(), to.length());
    let mut points = vec![start];
    points.extend((1..segments).map(|i| {
        let t = i as f64 / segments as f64;
        center + (r0 + (r1 - r0) * t) * angle::from_angle(begin + sweep * t)
    }));
    points.push(end);
    points
}

/// How many segments an arc sweeping `sweep` radians, at most a turn, is
/// flattened into: its share of [`CIRCLE_SEGMENTS`], at least one.
pub(crate) fn arc_segments(sweep: f64) -> usize {
    let share = (sweep / TAU * CIRCLE_SEGMENTS as f64).ceil();
    // Within 1..=CIRCLE_SEGMENTS for any sweep, NaN included, which
    // `as` turns into 0.
    (share as usize).clamp(1, CIRCLE_SEGMENTS)
}

#[cfg(test)]
mod tests;
