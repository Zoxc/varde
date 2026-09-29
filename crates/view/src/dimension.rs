//! Dimensions as the user places and reads them: what the Dimension tool
//! measures of the items picked and where the cursor is, and what a
//! dimension is called and shows. Pure functions of the sketch, tested
//! headless.

use std::f64::consts::{PI, TAU};

use glam::DVec2;
use varde_expr::{LengthUnit, format};
use varde_sketch::{Curve, Dimension, Id, Kind, Measure, Role, Side, Sketch};

/// Under this sine of the angle between them, two lines picked are
/// parallel, and dimensioned by the distance between them rather than
/// their angle.
const PARALLEL: f64 = 1e-3;

/// What a line, or two points, is measured along, by where the label is
/// placed: see [`extent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Extent {
    Aligned,
    Horizontal,
    Vertical,
}

/// Whether the items `picked` of `sketch` measure something together: a
/// line alone, a circle or an arc alone, a spline's handle alone (by its
/// tip), two points, a point and a line that doesn't end at it, or two
/// lines; not an axis alone, nor the origin and axes among themselves.
fn measurable(sketch: &Sketch, picked: &[Id]) -> bool {
    let plays = |id, role: Role| sketch.kind(id).is_some_and(|kind| role.admits(kind));
    match *picked {
        [one] if sketch.handle(one).is_some() => true,
        [one] => plays(one, Role::Curve) && !one.is_builtin(),
        [a, b] if a != b && !(a.is_builtin() && b.is_builtin()) => {
            let distance = Measure::Distance(a, b);
            distance.items().all(|(id, role)| plays(id, role))
                && distance.own_point(sketch).is_none()
        }
        _ => false,
    }
}

/// Whether picking `id` after `picked` makes something to measure
/// together, rather than starting afresh from `id`.
pub fn joins(sketch: &Sketch, picked: &[Id], id: Id) -> bool {
    match *picked {
        [first] => measurable(sketch, &[first, id]),
        _ => false,
    }
}

/// Whether `id` can start a pick: a point, a line, a circle or an arc.
pub fn pickable(sketch: &Sketch, id: Id) -> bool {
    sketch
        .kind(id)
        .is_some_and(|kind| kind != Kind::Spline && Role::Geometry.admits(kind))
}

/// Whether `picked` is a circle or an arc alone, whose dimension can be
/// its radius or its diameter.
pub fn round(sketch: &Sketch, picked: &[Id]) -> bool {
    matches!(*picked, [one] if sketch.kind(one).is_some_and(|kind| Role::Round.admits(kind)))
}

/// What the Dimension tool measures of the items `picked` of `sketch`
/// with its label placed at `at`, and on which side, if they measure
/// anything:
///
/// - a line: its length, or placed off its ends (past the lines across
///   them), its horizontal extent while `at` is above or below both, its
///   vertical while it's left or right of both;
/// - two points: the distance between them, or the same way their
///   horizontal or vertical distance;
/// - a point and a line: the point's distance from the line;
/// - two lines: the distance between them if they're parallel, else the
///   angle between them that `at` is in, or the one across the corner
///   from it, which is the same;
/// - a circle its diameter, an arc its radius, or with `switched` the
///   other;
/// - a spline's handle, by its tip, its angle from the X axis
///   (counter-clockwise; its length is its fit point's and its tip's
///   distance).
pub fn measure(
    sketch: &Sketch,
    picked: &[Id],
    at: DVec2,
    switched: bool,
) -> Option<(Measure, Side)> {
    if !measurable(sketch, picked) {
        return None;
    }
    let point = |id| sketch.point(id).map(|point| point.at);
    let measure = match *picked {
        [one] if sketch.handle(one).is_some() => Measure::Angle(Id::X_AXIS, one),
        [one] => match sketch.curve(one)?.curve {
            Curve::Line { start, end } => along(
                extent(point(start)?, point(end)?, at),
                start,
                end,
                Measure::Length(one),
            ),
            Curve::Circle { .. } if switched => Measure::Radius(one),
            Curve::Circle { .. } => Measure::Diameter(one),
            Curve::Arc { .. } if switched => Measure::Diameter(one),
            Curve::Arc { .. } => Measure::Radius(one),
            Curve::Spline(_) => return None,
        },
        [a, b] => match (point(a), point(b)) {
            (Some(p), Some(q)) => along(extent(p, q, at), a, b, Measure::Distance(a, b)),
            (None, None) => {
                let ((a_start, a_end), (b_start, b_end)) = (sketch.line(a)?, sketch.line(b)?);
                let (u, w) = ((a_end - a_start).normalize(), (b_end - b_start).normalize());
                if u.perp_dot(w).abs() < PARALLEL {
                    // From the middle of the first, which an axis hasn't.
                    if a.is_builtin() {
                        Measure::Distance(b, a)
                    } else {
                        Measure::Distance(a, b)
                    }
                } else {
                    return angle(sketch, a, b, at);
                }
            }
            _ => Measure::Distance(a, b),
        },
        _ => return None,
    };
    let side = sketch.side(&measure);
    Some((measure, side))
}

/// `aligned`, the measure between the points `a` and `b` straight, or
/// their horizontal or vertical distance for `extent`.
fn along(extent: Extent, a: Id, b: Id, aligned: Measure) -> Measure {
    match extent {
        Extent::Aligned => aligned,
        Extent::Horizontal => Measure::HorizontalDistance(a, b),
        Extent::Vertical => Measure::VerticalDistance(a, b),
    }
}

/// What the distance from `p` to `q` is measured along with its label at
/// `at`: straight while `at` is beside the segment between them; off its
/// ends, the vertical distance while `at` is left or right of both, the
/// horizontal while it's above or below both; and straight again past
/// their corners, as for a line along an axis.
fn extent(p: DVec2, q: DVec2, at: DVec2) -> Extent {
    let along = q - p;
    let t = along.dot(at - p) / along.length_squared();
    let (low, high) = (p.min(q), p.max(q));
    if (0.0..=1.0).contains(&t) || !t.is_finite() {
        Extent::Aligned
    } else if (low.y..=high.y).contains(&at.y) {
        Extent::Vertical
    } else if (low.x..=high.x).contains(&at.x) {
        Extent::Horizontal
    } else {
        Extent::Aligned
    }
}

/// The angle between the lines `a` and `b` of `sketch` on the side `at`
/// is: of the four angles between them, the one `at` is in, or the one
/// across the corner from it, which is the same. As a measure under half
/// a turn, the order of the lines and the side giving which.
fn angle(sketch: &Sketch, a: Id, b: Id, at: DVec2) -> Option<(Measure, Side)> {
    let meet = sketch.anchor(&Measure::Angle(a, b))?;
    let toward = at - meet;
    let candidates = [(a, b), (b, a)]
        .into_iter()
        .flat_map(|(a, b)| [(a, b, Side::Positive), (a, b, Side::Negative)]);
    let mut under_half = candidates.filter_map(|(a, b, side)| {
        let value = sketch.measure(&Measure::Angle(a, b), side)?;
        (value < PI).then_some((a, b, side, value))
    });
    let first = under_half.next()?;
    let holds = |&(a, _, side, value): &(Id, Id, Side, f64)| {
        let (start, end) = sketch.line(a)?;
        let from = (end - start) * side.sign();
        Some(sector_holds(from, value, toward) || sector_holds(from, value, -toward))
    };
    let (a, b, side, _) = std::iter::once(first)
        .chain(under_half)
        .find(|candidate| holds(candidate) == Some(true))
        .unwrap_or(first);
    Some((Measure::Angle(a, b), side))
}

/// Whether the direction `toward` is within the angle `sweep` turning
/// counter-clockwise from the direction `from`.
pub(crate) fn sector_holds(from: DVec2, sweep: f64, toward: DVec2) -> bool {
    from.angle_to(toward).rem_euclid(TAU) <= sweep
}

/// A measure as the user sees it: "Length", "Horizontal distance".
pub fn name(measure: &Measure) -> &'static str {
    match measure {
        Measure::Distance(..) => "Distance",
        Measure::HorizontalDistance(..) => "Horizontal distance",
        Measure::VerticalDistance(..) => "Vertical distance",
        Measure::Length(_) => "Length",
        Measure::Angle(..) => "Angle",
        Measure::Radius(_) => "Radius",
        Measure::Diameter(_) => "Diameter",
        Measure::Offset(..) => "Offset",
    }
}

/// `value`, in model units, as a dimension of `measure` shows it in a
/// design in `units`: "40 mm", "90°", "R 5 mm", "Ø 10 mm".
fn shown_value(measure: &Measure, value: f64, units: LengthUnit) -> String {
    let value = format(value, measure.quantity().unit(units));
    match measure {
        Measure::Radius(_) => format!("R {value}"),
        Measure::Diameter(_) => format!("Ø {value}"),
        _ => value,
    }
}

/// What `dimension` shows on its label in `sketch` in a design in
/// `units`: its value, or a reference's measure of the sketch as it is,
/// in brackets.
pub fn label(sketch: &Sketch, dimension: &Dimension, units: LengthUnit) -> String {
    let measure = &dimension.measure;
    if dimension.driving {
        return shown_value(measure, dimension.value.value, units);
    }
    // Negative where the geometry has crossed to the other side: its size
    // all the same.
    let measured = sketch
        .measure(measure, dimension.side)
        .map_or(dimension.value.value, f64::abs);
    format!("({})", shown_value(measure, measured, units))
}

/// What the value field for a new dimension of `measure` on `side` starts
/// with: what it measures now, in `units`, without the "R" or "Ø" the
/// label shows. Empty if it doesn't measure anything.
pub fn measured_text(sketch: &Sketch, measure: &Measure, side: Side, units: LengthUnit) -> String {
    sketch
        .measure(measure, side)
        .map(|value| format(value, measure.quantity().unit(units)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
