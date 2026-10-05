//! Where the constraints' glyphs go: beside the geometry each ties
//! together, at the place it's about (the point, the corner, where the
//! curves touch). Pure, tested headless.

use glam::DVec2;
use varde_sketch::{Constraint, Curve, Id, Sketch, crossing, foot};

/// The places in sketch coordinates to show a glyph of `constraint` at:
/// one, or for a constraint between two things apart, parallel or equal,
/// one on each; a tangent or a smooth join with a spline where it joins.
/// None if what it names is missing.
pub(crate) fn anchors(sketch: &Sketch, constraint: &Constraint) -> Vec<DVec2> {
    let at = |id| sketch.point(id).map(|point| point.at);
    let between = |a, b| Some((at(a)? + at(b)?) / 2.0);
    let found = match *constraint {
        Constraint::Coincident(point, _)
        | Constraint::PointOnCurve { point, .. }
        | Constraint::Midpoint { point, .. } => at(point),
        Constraint::HorizontalPoints(a, b)
        | Constraint::VerticalPoints(a, b)
        | Constraint::Symmetric { a, b, .. } => between(a, b),
        Constraint::Horizontal(item) | Constraint::Vertical(item) => line_anchor(sketch, item),
        Constraint::Fix(item) => anchor(sketch, item),
        Constraint::Parallel(a, b) => {
            return [line_anchor(sketch, a), line_anchor(sketch, b)]
                .into_iter()
                .flatten()
                .collect();
        }
        Constraint::Equal(a, b) => {
            return [anchor(sketch, a), anchor(sketch, b)]
                .into_iter()
                .flatten()
                .collect();
        }
        Constraint::Perpendicular(a, b) => corner(sketch, a, b),
        // With a spline, where it's named.
        Constraint::Tangent {
            at: Some(point), ..
        }
        | Constraint::Smooth { at: point, .. } => at(point),
        Constraint::Tangent { a, b, .. } => touching(sketch, a, b),
        // On the copy it ties, beside those of its parallel: the one
        // offset every copy is tied to would gather them all.
        Constraint::EqualOffset { b: [_, copy], .. } => anchor(sketch, copy),
        Constraint::Concentric(a, b) => sketch
            .round(a)
            .or_else(|| sketch.round(b))
            .map(|(center, _)| center)
            .or_else(|| at(a)),
    };
    found.into_iter().collect()
}

/// Where a glyph goes on what's named as a line ([`Sketch::direction`]):
/// a handle, by its tip, at its middle, else as [`anchor`].
fn line_anchor(sketch: &Sketch, id: Id) -> Option<DVec2> {
    match sketch.handle(id) {
        Some(_) => {
            let (at, tip) = sketch.direction(id)?;
            Some((at + tip) / 2.0)
        }
        None => anchor(sketch, id),
    }
}

/// Where a glyph goes on the item `id`: a point's place, a line's middle,
/// a circle's upper right, an arc's or a spline's middle.
fn anchor(sketch: &Sketch, id: Id) -> Option<DVec2> {
    if let Some(point) = sketch.point(id) {
        return Some(point.at);
    }
    let entry = sketch.curve(id)?;
    match entry.curve {
        Curve::Line { .. } | Curve::Arc { .. } | Curve::Spline(_) => {
            let polyline = sketch.flatten(&entry.curve)?;
            match polyline[..] {
                [start, end] => Some((start + end) / 2.0),
                _ => polyline.get(polyline.len() / 2).copied(),
            }
        }
        Curve::Circle { .. } => {
            let (center, radius) = sketch.round(id)?;
            Some(center + DVec2::splat(std::f64::consts::FRAC_1_SQRT_2 * radius))
        }
    }
}

/// Where the lines (or handles) `a` and `b` meet, if they do near them (a corner),
/// else halfway between their middles. An axis, which has no middle, is
/// near anywhere, and failing a corner the glyph is at the other's
/// middle.
fn corner(sketch: &Sketch, a: Id, b: Id) -> Option<DVec2> {
    let halfway = match (line_anchor(sketch, a), line_anchor(sketch, b)) {
        (Some(a), Some(b)) => (a + b) / 2.0,
        (Some(one), None) | (None, Some(one)) => one,
        (None, None) => return None,
    };
    let ((p, p_end), (q, q_end)) = (sketch.direction(a)?, sketch.direction(b)?);
    let r = p_end - p;
    let Some((t, _)) = crossing(p, r, q, q_end - q) else {
        return Some(halfway);
    };
    let meet = p + r * t;
    // Near both: within a line's length of its middle.
    let near = |line: Id, start: DVec2, end: DVec2| {
        line.is_axis() || meet.distance((start + end) / 2.0) <= start.distance(end)
    };
    Some(
        if meet.is_finite() && near(a, p, p_end) && near(b, q, q_end) {
            meet
        } else {
            halfway
        },
    )
}

/// Where the curves `a` and `b`, one of them a circle or an arc, touch
/// once tangent: nearest a round's centre on a line, or on the line
/// between two centres at the first's radius.
fn touching(sketch: &Sketch, a: Id, b: Id) -> Option<DVec2> {
    let on_line = |line: Id, (center, _): (DVec2, f64)| {
        let (start, end) = sketch.line(line)?;
        Some(foot(center, start, end)).filter(|foot| foot.is_finite())
    };
    match (sketch.round(a), sketch.round(b)) {
        (Some((ca, ra)), Some((cb, _))) => {
            let direction = (cb - ca).try_normalize().unwrap_or(DVec2::X);
            Some(ca + direction * ra)
        }
        (Some(round), None) => on_line(b, round),
        (None, Some(round)) => on_line(a, round),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests;
