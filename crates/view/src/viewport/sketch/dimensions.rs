//! How a dimension is drawn: its extension lines, its dimension line (or
//! arc, for an angle) through the label, and where its arrows point, in
//! sketch coordinates. The renderer draws the lines; the arrows, a fixed
//! size on screen, are made where they show. Pure, tested headless.

use glam::DVec2;
use varde_sketch::{Id, Measure, OffsetPair, Side, Sketch, flatten_arc};

use crate::dimension::sector_holds;

/// A dimension's lines and arrows, in sketch coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Lines {
    /// Polylines: extension lines, then the dimension line or arc.
    pub(crate) lines: Vec<Vec<DVec2>>,
    /// Arrowheads, each as its tip and a point it points away from.
    pub(crate) arrows: Vec<[DVec2; 2]>,
}

/// How a dimension of `measure` on `side` of `sketch` is drawn with its
/// label at `label`, if what it measures is there:
///
/// - between two places (points, a point and its foot on a line, a line's
///   middle and its foot on another, a line's ends), a dimension line
///   through the label parallel to the line between them, or along an
///   axis for a horizontal or vertical distance, reaching to the label if
///   it's off the end, with extension lines out to it and arrows at its
///   ends; with a line, the line extended to the foot if it's off its
///   ends;
/// - an angle, an arc around where the lines meet, as far out as the
///   label, over the angle measured, or across the corner if the label is
///   there, with the lines extended to it;
/// - a radius, from the centre towards the label to the curve, and a
///   diameter across the curve, each reaching out to the label;
/// - an offset, between lines as a distance from the copy's middle; of a
///   circle or an arc, from the one to the other towards the label, or a
///   round join's as a radius.
pub(crate) fn lines(sketch: &Sketch, measure: &Measure, side: Side, label: DVec2) -> Option<Lines> {
    let at = |id| sketch.point(id).map(|point| point.at);
    let mut lines = match *measure {
        Measure::Length(line) => {
            let (start, end) = sketch.line(line)?;
            aligned(start, end, label)?
        }
        Measure::Distance(a, b) => match (at(a), at(b)) {
            (Some(p), Some(q)) => aligned(p, q, label)?,
            (Some(point), None) => to_line(point, sketch.line(b)?, label)?,
            (None, Some(point)) => to_line(point, sketch.line(a)?, label)?,
            (None, None) => {
                let (start, end) = sketch.line(a)?;
                to_line(start.midpoint(end), sketch.line(b)?, label)?
            }
        },
        Measure::HorizontalDistance(a, b) => {
            let (p, q) = (at(a)?, at(b)?);
            between(
                p,
                q,
                DVec2::new(p.x, label.y),
                DVec2::new(q.x, label.y),
                label,
            )
        }
        Measure::VerticalDistance(a, b) => {
            let (p, q) = (at(a)?, at(b)?);
            between(
                p,
                q,
                DVec2::new(label.x, p.y),
                DVec2::new(label.x, q.y),
                label,
            )
        }
        Measure::Angle(a, b) => angle(sketch, a, b, side, label)?,
        Measure::Radius(round) => {
            let (center, radius) = sketch.round(round)?;
            let toward = outward(center, label);
            let tip = center + toward * radius;
            let mut lines = vec![vec![center, tip]];
            lines.extend(reach(tip, center, radius, label));
            Lines {
                lines,
                arrows: vec![[tip, center]],
            }
        }
        Measure::Diameter(round) => {
            let (center, radius) = sketch.round(round)?;
            let toward = outward(center, label);
            let (near, far) = (center + toward * radius, center - toward * radius);
            let mut lines = vec![vec![far, near]];
            lines.extend(reach(near, center, radius, label));
            Lines {
                lines,
                arrows: vec![[near, far], [far, near]],
            }
        }
        Measure::Offset(a, b) => match sketch.offset_pair([a, b])? {
            OffsetPair::Lines => {
                let (start, end) = sketch.line(b)?;
                to_line(start.midpoint(end), sketch.line(a)?, label)?
            }
            OffsetPair::Rounds => {
                let ((center, from), (_, to)) = (sketch.round(a)?, sketch.round(b)?);
                let toward = outward(center, label);
                let (inner, outer) = (center + toward * from, center + toward * to);
                let mut lines = vec![vec![inner, outer]];
                lines.extend(reach(outer, center, from.max(to), label));
                Lines {
                    lines,
                    arrows: vec![[outer, inner], [inner, outer]],
                }
            }
            OffsetPair::Join => {
                let (center, radius) = sketch.round(b)?;
                let tip = center + outward(center, label) * radius;
                let mut lines = vec![vec![center, tip]];
                lines.extend(reach(tip, center, radius, label));
                Lines {
                    lines,
                    arrows: vec![[tip, center]],
                }
            }
            // From the place on the spline nearest the point.
            OffsetPair::Spline => {
                let point = sketch.point(b)?.at;
                aligned(sketch.nearest_on(a, point)?, point, label)?
            }
        },
    };
    lines
        .lines
        .retain(|line| line.iter().all(|p| p.is_finite()));
    lines
        .arrows
        .retain(|arrow| arrow.iter().all(|p| p.is_finite()));
    Some(lines)
}

/// The distance from `from` to the line from `start` to `end`, measured
/// to its foot on it, the line extended to the foot if that's off its
/// ends. `None` for a line of no length, or `from` on it.
fn to_line(from: DVec2, (start, end): (DVec2, DVec2), label: DVec2) -> Option<Lines> {
    let t = param(from, start, end);
    if !t.is_finite() {
        return None;
    }
    let foot = start + (end - start) * t;
    let mut lines = aligned(from, foot, label)?;
    lines.lines.extend(extended(foot, t, start, end));
    Some(lines)
}

/// How far along the line from `start` to `end` the place nearest `point`
/// is, as a share of the line: 0 at `start`, 1 at `end`. Not a number for
/// a line of no length.
fn param(point: DVec2, start: DVec2, end: DVec2) -> f64 {
    let along = end - start;
    along.dot(point - start) / along.length_squared()
}

/// The unit direction from `center` to `label`, or right if they're at
/// one place.
fn outward(center: DVec2, label: DVec2) -> DVec2 {
    (label - center).try_normalize().unwrap_or(DVec2::X)
}

/// From `tip`, on the circle around `center` of `radius`, out to `label`
/// if that's outside it.
fn reach(tip: DVec2, center: DVec2, radius: f64, label: DVec2) -> Option<Vec<DVec2>> {
    (label.distance(center) > radius).then(|| vec![tip, label])
}

/// The line through `start` and `end` extended from its nearer end to
/// the point `t` along it, `foot`, if that's off its ends.
fn extended(foot: DVec2, t: f64, start: DVec2, end: DVec2) -> Option<Vec<DVec2>> {
    if t < 0.0 {
        Some(vec![start, foot])
    } else if t > 1.0 {
        Some(vec![end, foot])
    } else {
        None
    }
}

/// A distance between `p` and `q` measured straight: the dimension line
/// parallel to the line between them, through `label`. `None` if they're
/// at one place.
fn aligned(p: DVec2, q: DVec2, label: DVec2) -> Option<Lines> {
    let normal = (q - p).try_normalize()?.perp();
    let off = normal * normal.dot(label - p);
    Some(between(p, q, p + off, q + off, label))
}

/// A distance between `p` and `q` shown by the dimension line from `p_on`
/// to `q_on`, extension lines from each to its place on it, arrows at its
/// ends, and the line going on to where `label` is along it, if that's
/// past its ends.
fn between(p: DVec2, q: DVec2, p_on: DVec2, q_on: DVec2, label: DVec2) -> Lines {
    let t = param(label, p_on, q_on);
    let dimension = if !t.is_finite() || (0.0..=1.0).contains(&t) {
        vec![p_on, q_on]
    } else {
        let past = p_on + (q_on - p_on) * t;
        if t < 0.0 {
            vec![past, p_on, q_on]
        } else {
            vec![p_on, q_on, past]
        }
    };
    Lines {
        lines: vec![vec![p, p_on], vec![q, q_on], dimension],
        arrows: vec![[p_on, q_on], [q_on, p_on]],
    }
}

/// The angle from line `a` to line `b` (or a handle) on `side` (see [`Measure::Angle`])
/// as an arc around where they meet through `label`, over that angle or
/// the one across the corner from it, whichever `label` is in, the lines
/// extended to its ends where it's past them.
fn angle(sketch: &Sketch, a: Id, b: Id, side: Side, label: DVec2) -> Option<Lines> {
    let measure = Measure::Angle(a, b);
    let meet = sketch.anchor(&measure)?;
    let sweep = sketch.measure(&measure, side)?;
    // Either may be a spline's handle, from its fit point to its tip.
    let ((a_start, a_end), (b_start, b_end)) = (sketch.direction(a)?, sketch.direction(b)?);
    let radius = label.distance(meet);
    if !(radius > 0.0 && sweep > 0.0) {
        return None;
    }
    let from = ((a_end - a_start) * side.sign()).try_normalize()?;
    // The arc turns from `from`, or across the corner from `-from`.
    let from = if sector_holds(from, sweep, label - meet) {
        from
    } else {
        -from
    };
    let start = meet + from * radius;
    let end = meet + DVec2::from_angle(sweep).rotate(from) * radius;
    let arc = flatten_arc(meet, start, end);
    let mut lines = Vec::new();
    for (tip, line_start, line_end) in [(start, a_start, a_end), (end, b_start, b_end)] {
        let t = param(tip, line_start, line_end);
        lines.extend(extended(tip, t, line_start, line_end));
    }
    let arrows = match arc.as_slice() {
        [first, second, .., before_last, last] => vec![[*first, *second], [*last, *before_last]],
        _ => Vec::new(),
    };
    lines.push(arc);
    Some(Lines { lines, arrows })
}

#[cfg(test)]
mod tests;
