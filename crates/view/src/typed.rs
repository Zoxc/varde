//! Typed values while drawing, and the shapes the drawing tools draw with
//! them: the fields a tool offers next to the cursor once its shape has a
//! point (a line's length and angle, a circle's diameter, a rectangle's
//! width and height, ...), what each asks for, and the shape drawn with the
//! cursor where it is and the values typed holding what they fix. Pure,
//! shared by the viewport's preview and the app's tools, so what's shown is
//! what's placed.

use std::f64::consts::TAU;

use glam::DVec2;
use varde_expr::{Ask, Error, ErrorKind, Quantity, Span, Value};
use varde_sketch::{
    ArcPoints, Design, Id, Measure, Setback, Sketch, SplineKind, arc_sweep, arc_through,
    flatten_spline,
};

use crate::{ActiveTool, Snap, Tool, ToolClick};

/// The fewest and the most sides a polygon may have, and how many it has
/// until others are typed.
pub const MIN_SIDES: u32 = 3;
pub const MAX_SIDES: u32 = 64;
pub const DEFAULT_SIDES: u32 = 6;

/// A value typed while drawing, in a field next to the cursor. All but
/// [`Field::Sides`] fix a size of the shape and are added with it as its
/// driving dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// A line's length.
    Length,
    /// A line's direction, counter-clockwise from the X axis.
    Angle,
    /// A circle's diameter, or the circle a polygon's corners are on.
    Diameter,
    /// An arc's radius.
    Radius,
    /// A rectangle's width and height.
    Width,
    Height,
    /// How many sides a polygon has.
    Sides,
    /// How far the Offset tool offsets, or how far back along the first of
    /// its lines the Chamfer tool cuts a corner.
    Distance,
    /// How far back along the second of its lines the Chamfer tool cuts a
    /// corner, if not as far as along the first.
    SecondDistance,
}

impl Field {
    /// The field as the user sees it.
    pub fn label(self) -> &'static str {
        match self {
            Field::Length => "Length",
            Field::Angle => "Angle",
            Field::Diameter => "Diameter",
            Field::Radius => "Radius",
            Field::Width => "Width",
            Field::Height => "Height",
            Field::Sides => "Sides",
            Field::Distance => "Distance",
            Field::SecondDistance => "Distance 2",
        }
    }

    /// The kind of value it is.
    pub fn quantity(self) -> Quantity {
        match self {
            Field::Angle => Quantity::Angle,
            Field::Sides => Quantity::Number,
            _ => Quantity::Length,
        }
    }

    /// What it asks for in `design`: what its dimension's measure asks
    /// (see [`Measure::ask`]), or for the sides a whole number from
    /// [`MIN_SIDES`] to [`MAX_SIDES`].
    pub fn ask(self, design: &Design) -> Ask {
        // Only the kind of measure matters to what it asks.
        let any = Id::ORIGIN;
        match self {
            Field::Sides => Ask::number(design.units, f64::from(MAX_SIDES))
                .at_least(f64::from(MIN_SIDES))
                .whole(),
            Field::Angle => Measure::Angle(any, any).ask(design),
            Field::Diameter => Measure::Diameter(any).ask(design),
            Field::Radius => Measure::Radius(any).ask(design),
            Field::Length | Field::Width | Field::Height => Measure::Length(any).ask(design),
            Field::Distance | Field::SecondDistance => Measure::Offset(any, any).ask(design),
        }
    }
}

/// The fields `tool` offers with `placed` points of its shape placed, in
/// the order `Tab` goes through them: none before its first point, nor
/// for an arc until its ends are placed.
pub fn fields(tool: Tool, placed: usize) -> &'static [Field] {
    match (tool, placed) {
        (Tool::Line, 1) => &[Field::Length, Field::Angle],
        (Tool::Circle, 1) => &[Field::Diameter],
        (Tool::Arc, 2) => &[Field::Radius],
        (Tool::Rectangle, 1) => &[Field::Width, Field::Height],
        (Tool::Polygon, 1) => &[Field::Sides, Field::Diameter],
        _ => &[],
    }
}

impl<'a> ActiveTool<'a> {
    /// The fields its shape offers now, see [`fields`], the Offset tool's
    /// distance once it has its chain, the Fillet tool's radius once it has
    /// its corner, and the Chamfer tool's distance, and the second distance
    /// or the angle to the first line that aren't as far along both.
    pub fn fields(&self) -> &'static [Field] {
        match self.tool {
            _ if self.picked.is_empty() => fields(self.tool, self.placed.len()),
            Tool::Offset => &[Field::Distance],
            Tool::Fillet => &[Field::Radius],
            Tool::Chamfer => &[Field::Distance, Field::SecondDistance, Field::Angle],
            _ => fields(self.tool, self.placed.len()),
        }
    }

    /// The corner the Fillet or Chamfer tool has picked: its point and its
    /// lines, the first the one a chamfer's first distance runs along.
    pub fn corner(&self) -> Option<(Id, [Id; 2])> {
        match *self.picked {
            [at, a, b] if self.tool.corners() => Some((at, [a, b])),
            _ => None,
        }
    }

    /// The value typed in `field` for its shape, if one is.
    pub fn value_in(&self, field: Field) -> Option<&'a Value> {
        let typed = self.typed.iter().find(|(typed, _)| *typed == field);
        typed.map(|(_, value)| value)
    }
}

/// Reads `text`, typed in `field` for the shape `tool` draws, as the
/// value it asks for in `design`. Beyond what the field asks, an arc's
/// radius has to reach across the chord between its ends.
pub fn read(tool: &ActiveTool, field: Field, text: &str, design: &Design) -> Result<Value, Error> {
    let value = Value::new(text, &field.ask(design))?;
    if let (Field::Radius, &[a, b]) = (field, tool.placed) {
        let least = a.distance(b) / 2.0;
        if value.value < least {
            return Err(Error {
                kind: ErrorKind::TooSmall {
                    min: least,
                    unit: Some(design.units.into()),
                },
                span: Span::new(0, text.len()),
            });
        }
    }
    Ok(value)
}

/// The Chamfer tool's cut, the first distance `first`, as the values typed
/// in `tool`'s other fields say (`value` of each): as far along the
/// second line as typed, or at the angle typed to the first, or else as
/// far along both.
pub fn setback<V>(tool: &ActiveTool, first: V, value: impl Fn(&Value) -> V) -> Setback<V> {
    match (
        tool.value_in(Field::SecondDistance),
        tool.value_in(Field::Angle),
    ) {
        (Some(second), _) => Setback::Two(first, value(second)),
        (None, Some(angle)) => Setback::Angle(first, value(angle)),
        (None, None) => Setback::Equal(first),
    }
}

/// What the Fillet or Chamfer tool makes on the corner it's picked in
/// `sketch` with the cursor at `at`, as the values typed fix it: a fillet
/// of the radius typed, or whose middle is as far into the corner as the
/// cursor; a chamfer as far back as typed, or crossing the way into the
/// corner as far in as the cursor. `None` for another tool, with no corner
/// picked, or where it doesn't fit.
pub fn corner_outline(tool: &ActiveTool, sketch: &Sketch, at: DVec2) -> Option<Outline> {
    let (corner, lines) = tool.corner()?;
    let place = sketch.point(corner)?.at;
    let typed = |field| tool.value_in(field).map(|value| value.value);
    match tool.tool {
        Tool::Fillet => {
            let radius =
                typed(Field::Radius).or_else(|| sketch.fillet_through(corner, lines, at))?;
            let arc = sketch.fillet_preview(corner, lines, radius)?;
            Some(Outline::Fillet { corner: place, arc })
        }
        Tool::Chamfer => {
            let first =
                typed(Field::Distance).or_else(|| sketch.chamfer_through(corner, lines, at))?;
            let setback = setback(tool, first, |value| value.value);
            let ends = sketch.chamfer_preview(corner, lines, &setback)?;
            Some(Outline::Chamfer {
                corner: place,
                ends,
            })
        }
        _ => None,
    }
}

/// A shape a drawing tool is drawing, or a fillet or chamfer the Fillet or
/// Chamfer tool is making (see [`corner_outline`]), in sketch coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Outline {
    /// A line, or the chord of an arc whose end isn't placed yet.
    Line {
        start: DVec2,
        end: DVec2,
    },
    Circle {
        center: DVec2,
        radius: f64,
    },
    Arc(ArcPoints),
    /// Its corners counter-clockwise or clockwise from the first placed,
    /// or with its `center` placed, from the one across from the corner
    /// the cursor is at: the second and fourth level with the first, then
    /// above or below it.
    Rectangle {
        corners: [DVec2; 4],
        center: Option<DVec2>,
    },
    /// Its corners on the circle about `center`, the first where the
    /// cursor is.
    Polygon {
        center: DVec2,
        radius: f64,
        corners: Vec<DVec2>,
    },
    /// A fillet on the corner at `corner`.
    Fillet {
        corner: DVec2,
        arc: ArcPoints,
    },
    /// A chamfer on the corner at `corner`, from its end on the first line
    /// to that on the second.
    Chamfer {
        corner: DVec2,
        ends: [DVec2; 2],
    },
    /// An open spline of `kind` through or by `points`, the last where
    /// the cursor is.
    Spline {
        points: Vec<DVec2>,
        kind: SplineKind,
    },
}

impl Outline {
    /// The polylines it's drawn as.
    pub fn polylines(&self) -> Vec<Vec<DVec2>> {
        match self {
            Outline::Line { start, end } => vec![vec![*start, *end]],
            Outline::Circle { center, radius } => {
                vec![varde_sketch::flatten_circle(*center, *radius)]
            }
            Outline::Arc(arc) | Outline::Fillet { arc, .. } => {
                vec![varde_sketch::flatten_arc(arc.center, arc.start, arc.end)]
            }
            Outline::Chamfer { ends, .. } => vec![ends.to_vec()],
            Outline::Rectangle { corners, .. } => vec![closed(corners)],
            Outline::Polygon { corners, .. } => vec![closed(corners)],
            // Short of the points it takes, straight between them.
            Outline::Spline { points, kind } => {
                vec![flatten_spline(points, *kind, false).unwrap_or_else(|| points.clone())]
            }
        }
    }

    /// The construction geometry that comes with it, drawn dashed: a
    /// polygon's circle, a rectangle's diagonal from its centre, a
    /// spline's control polygon.
    pub fn construction(&self) -> Option<Vec<DVec2>> {
        match self {
            Outline::Spline {
                points,
                kind: SplineKind::Control,
            } => Some(points.clone()),
            Outline::Polygon { center, radius, .. } => {
                Some(varde_sketch::flatten_circle(*center, *radius))
            }
            Outline::Rectangle {
                corners,
                center: Some(_),
            } => Some(vec![corners[0], corners[2]]),
            _ => None,
        }
    }

    /// What `field` measures of it, in model units, if it's a field of its
    /// shape: the value the field shows until one is typed. An angle is in
    /// `[0, 2π)`.
    pub fn value(&self, field: Field) -> Option<f64> {
        let value = match (self, field) {
            (Outline::Line { start, end }, Field::Length) => start.distance(*end),
            (Outline::Line { start, end }, Field::Angle) => {
                let angle = (*end - *start).to_angle();
                if angle < 0.0 { angle + TAU } else { angle }
            }
            (Outline::Circle { radius, .. } | Outline::Polygon { radius, .. }, Field::Diameter) => {
                2.0 * radius
            }
            (Outline::Arc(arc), Field::Radius) => arc.center.distance(arc.start),
            (Outline::Rectangle { corners, .. }, Field::Width) => corners[0].distance(corners[1]),
            (Outline::Rectangle { corners, .. }, Field::Height) => corners[0].distance(corners[3]),
            (Outline::Polygon { corners, .. }, Field::Sides) => corners.len() as f64,
            (Outline::Fillet { arc, .. }, Field::Radius) => arc.center.distance(arc.start),
            (Outline::Chamfer { corner, ends }, Field::Distance) => corner.distance(ends[0]),
            (Outline::Chamfer { corner, ends }, Field::SecondDistance) => corner.distance(ends[1]),
            // At its first end, inside the corner.
            (Outline::Chamfer { corner, ends }, Field::Angle) => {
                let (back, across) = (*corner - ends[0], ends[1] - ends[0]);
                back.perp_dot(across).abs().atan2(back.dot(across))
            }
            _ => return None,
        };
        value.is_finite().then_some(value)
    }

    /// The point a click at `at` places of it: a line's end, a point on a
    /// circle (towards `at`) or an arc (its middle), a rectangle's corner
    /// across from the first, a polygon's first corner.
    pub(crate) fn aimed(&self, at: DVec2) -> DVec2 {
        match self {
            Outline::Line { end, .. } => *end,
            Outline::Circle { center, radius } => {
                *center + (at - *center).try_normalize().unwrap_or(DVec2::X) * *radius
            }
            Outline::Arc(arc) => {
                let (from, to) = (arc.start - arc.center, arc.end - arc.center);
                arc.center + DVec2::from_angle(arc_sweep(from, to) / 2.0).rotate(from)
            }
            Outline::Rectangle { corners, .. } => corners[2],
            Outline::Polygon { corners, .. } => corners[0],
            // The corner tools click where the cursor is, and so does the
            // Spline tool.
            Outline::Fillet { .. } | Outline::Chamfer { .. } | Outline::Spline { .. } => at,
        }
    }
}

/// `points` and the first again.
fn closed(points: &[DVec2]) -> Vec<DVec2> {
    points.iter().chain(points.first()).copied().collect()
}

/// The shape `tool` draws with its next click at `at`, as the values typed
/// fix it, if its next click completes one or draws a line to it: `None`
/// with nothing placed, or values typed it can't hold (an arc's radius
/// short of its chord).
pub fn outline(tool: &ActiveTool, at: DVec2) -> Option<Outline> {
    let typed = |field| tool.value_in(field).map(|value| value.value);
    // The way from `from` to the cursor, or along X where it's there.
    let toward = |from: DVec2| (at - from).try_normalize().unwrap_or(DVec2::X);
    Some(match (tool.tool, tool.placed) {
        (Tool::Line, &[start]) => {
            let end = match (typed(Field::Length), typed(Field::Angle)) {
                (None, None) => at,
                (Some(length), None) => start + toward(start) * length,
                (length, Some(angle)) => {
                    let direction = DVec2::from_angle(angle);
                    // Along the angle as far as the cursor goes, unless
                    // the length is typed too.
                    let along = (at - start).dot(direction).max(0.0);
                    start + direction * length.unwrap_or(along)
                }
            };
            Outline::Line { start, end }
        }
        (Tool::Arc, &[start]) => Outline::Line { start, end: at },
        (Tool::Circle, &[center]) => Outline::Circle {
            center,
            radius: typed(Field::Diameter).map_or(center.distance(at), |d| d / 2.0),
        },
        (Tool::Arc, &[a, b]) => Outline::Arc(match typed(Field::Radius) {
            Some(radius) => arc_with_radius(a, b, radius, at)?,
            None => arc_through(a, b, at)?,
        }),
        (Tool::Rectangle, &[first]) => {
            let away = at - first;
            let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
            // From the centre, the cursor's at half the size.
            let whole = if tool.centered { 2.0 } else { 1.0 };
            let width = typed(Field::Width).unwrap_or(away.x.abs() * whole);
            let height = typed(Field::Height).unwrap_or(away.y.abs() * whole);
            let across = DVec2::new(sign(away.x) * width, sign(away.y) * height);
            let (start, center) = if tool.centered {
                (first - across / 2.0, Some(first))
            } else {
                (first, None)
            };
            let end = start + across;
            Outline::Rectangle {
                corners: [
                    start,
                    DVec2::new(end.x, start.y),
                    end,
                    DVec2::new(start.x, end.y),
                ],
                center,
            }
        }
        (Tool::Spline, placed) if !placed.is_empty() => Outline::Spline {
            points: placed.iter().copied().chain([at]).collect(),
            kind: tool.spline_kind(),
        },
        (Tool::Polygon, &[center]) => {
            let radius = typed(Field::Diameter).map_or(center.distance(at), |d| d / 2.0);
            let first = toward(center) * radius;
            let sides = tool.sides.clamp(MIN_SIDES, MAX_SIDES);
            let corners = (0..sides)
                .map(|k| {
                    let turn = TAU * f64::from(k) / f64::from(sides);
                    center + DVec2::from_angle(turn).rotate(first)
                })
                .collect();
            Outline::Polygon {
                center,
                radius,
                corners,
            }
        }
        _ => return None,
    })
}

/// The arc from `a` to `b` of `radius`, bulging towards `toward`: of the
/// two such arcs on that side of the chord (the short way round or the long
/// one), the one whose middle is nearer `toward`'s distance from the
/// chord. `None` if the radius doesn't reach across the chord.
fn arc_with_radius(a: DVec2, b: DVec2, radius: f64, toward: DVec2) -> Option<ArcPoints> {
    let chord = b - a;
    let half = chord.length() / 2.0;
    let normal = chord.perp().try_normalize()?;
    // Not `radius < half`, so a NaN radius makes none.
    if radius.partial_cmp(&half).is_none_or(|order| order.is_lt()) {
        return None;
    }
    let middle = a.midpoint(b);
    let off = (toward - middle).dot(normal);
    let side = if off < 0.0 { -1.0 } else { 1.0 };
    // How far the centre is from the chord's middle, and so how far from
    // the chord each arc's middle is.
    let depth = (radius * radius - half * half).max(0.0).sqrt();
    let (short, long) = (radius - depth, radius + depth);
    let bulge = if (off.abs() - short).abs() <= (off.abs() - long).abs() {
        short
    } else {
        long
    };
    arc_through(a, b, middle + normal * side * bulge)
}

/// `snap`, where the click of `tool` goes, as the values typed hold it,
/// see [`aim`].
pub(crate) fn hold(tool: &ActiveTool, snap: Snap) -> Option<Snap> {
    if tool.typed.is_empty() {
        return Some(snap);
    }
    let outline = outline(tool, snap.at)?;
    let fixed = |field| tool.value_in(field).is_some();
    Some(Snap {
        at: outline.aimed(snap.at),
        target: None,
        inference: snap
            .inference
            .filter(|_| !fixed(Field::Angle) && !fixed(Field::Radius)),
    })
}

/// `click` of `tool` as the values typed hold it: at the point the shape
/// they fix places there (see [`outline`]), and no longer on what it
/// snapped to where a value typed takes it off it, nor running as it was
/// inferred to where a direction is typed (an angle, an arc's radius).
/// `None` if the values typed can't make a shape there.
pub fn aim(tool: &ActiveTool, click: ToolClick) -> Option<ToolClick> {
    hold(tool, click.snap()).map(|snap| click.snapped(snap))
}

#[cfg(test)]
mod tests;
