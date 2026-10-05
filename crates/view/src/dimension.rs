//! Dimensions as the user places and reads them: what the Dimension tool
//! measures of the items picked and where the cursor is, and what a
//! dimension is called and shows. Pure functions of the sketch, tested
//! headless.

use std::f64::consts::{PI, TAU};

use glam::DVec2;
use varde_expr::{LengthUnit, format};
use varde_sketch::{Curve, Dimension, Id, Kind, Measure, Role, Side, Sketch, arc_sweep};

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
/// tip), two points, a point and a line that doesn't end at it, two
/// lines, or a circle or an arc and a point, a line or another circle or
/// arc, by its edge (see [`edge_distance`]); not an axis alone, nor the
/// origin and axes among themselves.
fn measurable(sketch: &Sketch, picked: &[Id]) -> bool {
    let plays = |id, role: Role| sketch.kind(id).is_some_and(|kind| role.admits(kind));
    match *picked {
        [one] if sketch.handle(one).is_some() => true,
        [one] => plays(one, Role::Curve) && !one.is_builtin(),
        [a, b] if a != b && !(a.is_builtin() && b.is_builtin()) => {
            if let Some(edge) = edge_distance(sketch, a, b) {
                let tips = std::collections::HashSet::new();
                return edge.items().all(|(id, role)| plays(id, role)) && edge.fits(sketch, &tips);
            }
            let distance = Measure::Distance(a, b);
            distance.items().all(|(id, role)| plays(id, role))
                && distance.own_point(sketch).is_none()
        }
        _ => false,
    }
}

/// The distance from the edge of a circle or an arc to the other of `a`
/// and `b` of `sketch`, if either is one: the circle first, and of two,
/// the one holding the other if one does, else `a`. A circle's centre is
/// a point, measured from as any other.
fn edge_distance(sketch: &Sketch, a: Id, b: Id) -> Option<Measure> {
    let round = |id| sketch.kind(id).is_some_and(|kind| Role::Round.admits(kind));
    match (round(a), round(b)) {
        (true, true) => {
            let ((ca, ra), (cb, rb)) = (sketch.round(a)?, sketch.round(b)?);
            // `b` holds `a`: from `b`'s edge in to `a`'s.
            if cb.distance(ca) + ra < rb {
                Some(Measure::EdgeDistance(b, a))
            } else {
                Some(Measure::EdgeDistance(a, b))
            }
        }
        (true, false) => Some(Measure::EdgeDistance(a, b)),
        (false, true) => Some(Measure::EdgeDistance(b, a)),
        (false, false) => None,
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
/// - a circle or an arc and a point, a line or another circle or arc:
///   the gap from its edge, see [`Measure::EdgeDistance`];
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
        [a, b] if edge_distance(sketch, a, b).is_some() => edge_distance(sketch, a, b)?,
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
/// counter-clockwise from the direction `from`; never for a direction of
/// no length.
///
/// The turn is [`arc_sweep`]'s, by libm's `atan2` (the side picked and the
/// snaps it lets through are saved), so an arc's own ends are on its run:
/// the turn to `toward` the very vector the sweep was made to is that
/// sweep to the bit, and `toward` along `from` is no turn, not a whole one.
/// (`acos` of the cosine, as glam's `angle_to` has it, is up to 10⁻⁸ off
/// near no turn and a half turn.)
pub(crate) fn sector_holds(from: DVec2, sweep: f64, toward: DVec2) -> bool {
    if from == DVec2::ZERO || toward == DVec2::ZERO {
        return false;
    }
    let turn = arc_sweep(from, toward);
    turn <= sweep || turn == TAU
}

/// A measure as the user sees it: "Length", "Horizontal distance".
pub fn name(measure: &Measure) -> &'static str {
    match measure {
        Measure::Distance(..) | Measure::EdgeDistance(..) => "Distance",
        Measure::HorizontalDistance(..) => "Horizontal distance",
        Measure::VerticalDistance(..) => "Vertical distance",
        Measure::Length(_) => "Length",
        Measure::Angle(..) => "Angle",
        Measure::Radius(_) => "Radius",
        Measure::Diameter(_) => "Diameter",
        Measure::Offset(..) => "Offset",
    }
}

/// What the items `selected` of `sketch` measure together, as the
/// Dimension tool would with its label nowhere in particular (see
/// [`measure`]): a line's length rather than an extent, two points'
/// distance straight, the first angle under a half turn between two
/// lines; and the value measured, as a size. `None` for what doesn't
/// measure anything.
pub fn selected(sketch: &Sketch, selected: &[Id]) -> Option<(Measure, f64)> {
    // A place nowhere: no line runs to it (straight, `extent`), nor is
    // it in any angle (the first, `angle`).
    let (measure, side) = measure(sketch, selected, DVec2::NAN, false)?;
    let value = sketch.measure(&measure, side)?.abs();
    Some((measure, value))
}

/// Under this cosine of the angle between them, two sides of a
/// rectangle selected are square to each other.
const SQUARE: f64 = 1e-6;

/// The width and height of the rectangle the items `selected` of
/// `sketch` make, if they do: four lines closing a loop, each square to
/// the next, and perhaps the points at its corners. The width is the
/// side nearer the horizontal.
pub fn rectangle(sketch: &Sketch, selected: &[Id]) -> Option<(f64, f64)> {
    let mut lines = Vec::new();
    let mut points = Vec::new();
    for &id in selected {
        match sketch.curve(id).map(|entry| &entry.curve) {
            Some(&Curve::Line { start, end }) if lines.len() < 4 => lines.push([start, end]),
            None if sketch.point(id).is_some() => points.push(id),
            _ => return None,
        }
    }
    let [first, ..] = lines[..] else {
        return None;
    };
    if lines.len() != 4 {
        return None;
    }
    // The corners in order round the loop, from the first line's start.
    let mut corners = vec![first[0]];
    let mut used = [true, false, false, false];
    let mut at = first[1];
    while at != first[0] {
        corners.push(at);
        let (next, line) =
            (lines.iter().enumerate()).find(|&(i, line)| !used[i] && line.contains(&at))?;
        used[next] = true;
        at = if line[0] == at { line[1] } else { line[0] };
    }
    if corners.len() != 4 || points.iter().any(|point| !corners.contains(point)) {
        return None;
    }
    let place = |id| sketch.point(id).map(|point| point.at);
    let places = [0, 1, 2, 3].map(|i| place(corners[i]));
    let places: Vec<DVec2> = places.into_iter().collect::<Option<_>>()?;
    let sides: Vec<DVec2> = (0..4).map(|i| places[(i + 1) % 4] - places[i]).collect();
    let square = (0..4).all(|i| {
        let (u, w) = (sides[i].try_normalize(), sides[(i + 1) % 4].try_normalize());
        u.zip(w).is_some_and(|(u, w)| u.dot(w).abs() < SQUARE)
    });
    if !square {
        return None;
    }
    let (a, b) = (sides[0], sides[1]);
    Some(if a.x.abs() >= a.y.abs() {
        (a.length(), b.length())
    } else {
        (b.length(), a.length())
    })
}

/// `measure` and its `value`, in model units, as the status bar shows it
/// in a design in `units`: "Length 40 mm", "Diameter 10 mm".
pub fn shown_measure(measure: &Measure, value: f64, units: LengthUnit) -> String {
    let value = format(value, measure.quantity().unit(units));
    format!("{} {value}", name(measure))
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

/// The size of the curve `entry` of `sketch` as its row in the Geometry
/// list notes it, in `units`: a line's length, a circle's diameter, an
/// arc's radius (or the other of the two where a driving dimension
/// measures it), and whether a driving dimension sets it. `None` for a
/// spline, or what doesn't measure.
pub fn size_note(
    sketch: &Sketch,
    entry: &varde_sketch::CurveEntry,
    units: LengthUnit,
) -> Option<(String, bool)> {
    let id = entry.id;
    let driving = |measure: &Measure| {
        (sketch.dimensions.iter())
            .any(|dimension| dimension.dimension.driving && dimension.dimension.measure == *measure)
    };
    let measure = match entry.curve {
        Curve::Line { .. } => Measure::Length(id),
        Curve::Circle { .. } | Curve::Arc { .. } => {
            let [usual, other] = match entry.curve {
                Curve::Circle { .. } => [Measure::Diameter(id), Measure::Radius(id)],
                _ => [Measure::Radius(id), Measure::Diameter(id)],
            };
            if !driving(&usual) && driving(&other) {
                other
            } else {
                usual
            }
        }
        Curve::Spline(_) => return None,
    };
    let value = sketch.measure(&measure, Side::Positive)?.abs();
    let driven = driving(&measure);
    Some((shown_value(&measure, value, units), driven))
}

/// Where the point `at` is, in `units`, as its row in the Geometry list
/// notes it: "10 mm, -5 mm".
pub fn point_note(at: DVec2, units: LengthUnit) -> String {
    let unit = Some(units.into());
    format!("{}, {}", format(at.x, unit), format(at.y, unit))
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
