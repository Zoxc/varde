//! Snapping: where a drawing tool's click goes near the cursor, and what
//! it's tied to there, which the tool adds as `auto` constraints. Pure
//! functions of the sketch, the tool and the cursor, tested headless.
//!
//! Candidates are gathered in sketch coordinates within
//! [`SNAP_TOLERANCE`] pixels of the cursor, and the first kind found
//! wins, the nearest of it:
//!
//! 1. a point of the sketch (an end, a centre, a lone point) or the
//!    origin;
//! 2. a line's midpoint, or a circle's or an arc's quadrant;
//! 3. with the Line tool, a tangent point on a circle or an arc, from the
//!    line's start; with the Arc tool's last click, the arc tangent at an
//!    end to the line or arc that end is on;
//! 4. where a curve (or an axis) meets a direction inferred from the
//!    line's start (see 6);
//! 5. the nearest place on a curve, the sketch's before the axes;
//! 6. a direction inferred from the line's start: horizontal, vertical,
//!    tangent to an arc ending there or a circle it starts on, and
//!    perpendicular or parallel to a line ending there or it starts on,
//!    in that order where two are as near.
//!
//! A click that isn't a point of the shape (a circle's rim, an arc's last
//! point) snaps only to points, which it then passes through, and, for an
//! arc, to the tangent arc.
//!
//! A spline being drawn also snaps to itself ([`Own`]): to the points
//! it has placed before the sketch's points (the first only once a
//! click there closes it, never the last), and to its curve through
//! them where it's nearer than the sketch's curves.

use glam::DVec2;
use varde_sketch::{Curve, Id, Sketch, angle, arc_sweep, crossing, flatten_spline};

use crate::dimension::sector_holds;
use crate::hit;
use crate::{ActiveTool, ConstraintKind, Tool};

/// How near the cursor a snap is taken, in pixels: also how near its
/// first point the Spline tool's click closes the spline.
pub const SNAP_TOLERANCE: f64 = 8.0;

/// Directions from a start nearer it than this many pixels are too short
/// to tell apart.
const MIN_RUN: f64 = 2.0;

/// How far a guide runs on past where the cursor snapped, in pixels.
const GUIDE_PAST: f64 = 40.0;

/// Where a drawing tool's click goes, and what snapped it there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// Where, in sketch coordinates.
    pub at: DVec2,
    /// What the point is on, if anything.
    pub target: Option<Target>,
    /// How the shape runs, from where it starts, if inferred.
    pub inference: Option<Inference>,
}

/// What a snapped point is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A point of the sketch's, which the shape takes as its own, or the
    /// origin, which it's made coincident with.
    Point(Id),
    /// The middle of a line.
    Midpoint(Id),
    /// Where a circle or an arc is level with its centre, or right above
    /// or below it.
    Quadrant { round: Id, level: Level },
    /// Somewhere on a curve or an axis.
    On(Id),
    /// The shape being drawn itself, which isn't in the sketch yet.
    Own(Own),
}

/// What of a spline being drawn a point snapped to it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Own {
    /// The point it placed with this index.
    Point(usize),
    /// Somewhere on its curve through the points placed: only where, as
    /// a point on its own curve is so whatever it does.
    Curve,
}

/// Which of a circle's quadrants: level with its centre (left or right of
/// it), or above or below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Horizontal,
    Vertical,
}

/// How the shape being drawn runs, inferred from where it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inference {
    Horizontal,
    Vertical,
    Parallel(Id),
    Perpendicular(Id),
    /// Tangent to the curve: a line to a circle or an arc, an arc to the
    /// line or arc at one of its ends.
    Tangent(Id),
}

impl Target {
    /// The item of the sketch's it's on, which has to be there for it to
    /// mean anything: none for the shape itself.
    pub fn item(self) -> Option<Id> {
        match self {
            Target::Point(id) | Target::Midpoint(id) | Target::On(id) => Some(id),
            Target::Quadrant { round, .. } => Some(round),
            Target::Own(_) => None,
        }
    }
}

impl Inference {
    /// The item it's against, if it's against one.
    pub fn item(self) -> Option<Id> {
        match self {
            Inference::Horizontal | Inference::Vertical => None,
            Inference::Parallel(id) | Inference::Perpendicular(id) | Inference::Tangent(id) => {
                Some(id)
            }
        }
    }
}

impl Snap {
    /// Where the cursor is, snapped to nothing.
    pub fn free(at: DVec2) -> Snap {
        Snap {
            at,
            target: None,
            inference: None,
        }
    }

    /// At `at`, on `target`.
    fn on(at: DVec2, target: Target) -> Snap {
        Snap {
            target: Some(target),
            ..Snap::free(at)
        }
    }

    /// The same place, snapped only to what's among the items `has`
    /// says are there: an undo may have taken the rest.
    pub fn within(self, has: impl Fn(Id) -> bool) -> Snap {
        Snap {
            at: self.at,
            target: (self.target).filter(|target| target.item().is_none_or(&has)),
            inference: self
                .inference
                .filter(|inference| inference.item().is_none_or(&has)),
        }
    }

    /// Whether it snapped to anything.
    pub fn snapped(&self) -> bool {
        self.target.is_some() || self.inference.is_some()
    }

    /// The glyphs shown by the cursor: what the point is on, then how the
    /// shape runs.
    pub(crate) fn kinds(&self) -> Vec<ConstraintKind> {
        // A point on its own spline isn't tied there: true whatever.
        let target = self.target.and_then(|target| match target {
            Target::Midpoint(_) => Some(ConstraintKind::Midpoint),
            Target::Own(Own::Curve) => None,
            Target::Point(_) | Target::Quadrant { .. } | Target::On(_) | Target::Own(_) => {
                Some(ConstraintKind::Coincident)
            }
        });
        let inference = self.inference.map(|inference| match inference {
            Inference::Horizontal => ConstraintKind::Horizontal,
            Inference::Vertical => ConstraintKind::Vertical,
            Inference::Parallel(_) => ConstraintKind::Parallel,
            Inference::Perpendicular(_) => ConstraintKind::Perpendicular,
            Inference::Tangent(_) => ConstraintKind::Tangent,
        });
        target.into_iter().chain(inference).collect()
    }

    /// The item to highlight: what the point is on.
    pub(crate) fn highlighted(&self) -> Option<Id> {
        self.target.and_then(Target::item)
    }

    /// The dashed guide to draw for it, in sketch coordinates, with
    /// `tool` drawing in `sketch` and a pixel `pixel` sketch units: along a
    /// direction inferred, from the start on past where it snapped, or
    /// from a circle's centre out to its quadrant.
    pub(crate) fn guide(
        &self,
        sketch: &Sketch,
        tool: &ActiveTool,
        pixel: f64,
    ) -> Option<[DVec2; 2]> {
        if let Some(Target::Quadrant { round, .. }) = self.target {
            let (center, _) = sketch.round(round)?;
            return Some([center, self.at]);
        }
        match self.inference? {
            Inference::Tangent(_) => None,
            _ => {
                let start = *tool.placed.first()?;
                let along = (self.at - start).try_normalize()?;
                Some([start, self.at + along * GUIDE_PAST * pixel])
            }
        }
    }
}

/// What the click the tool takes next makes of the place it snaps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placing {
    /// One of the shape's points.
    Point,
    /// A place the shape passes through: a circle's rim, an arc's last
    /// point.
    Through,
}

/// What the tool's next click places, if it snaps at all: the Dimension
/// tool and the shape tools pick instead.
fn placing(tool: &ActiveTool) -> Option<Placing> {
    Some(match (tool.tool, tool.placed.len()) {
        (kind, _) if !kind.draws() => return None,
        (Tool::Circle, 1) | (Tool::Arc, 2) => Placing::Through,
        _ => Placing::Point,
    })
}

/// Where the click of `tool` in `sketch` with the cursor at `cursor` goes,
/// a pixel being `pixel` sketch units there, and what it snaps to: see
/// the module's documentation for the order. Where the cursor is if it
/// snaps to nothing.
pub(crate) fn snap(sketch: &Sketch, tool: &ActiveTool, cursor: DVec2, pixel: f64) -> Snap {
    let free = Snap::free(cursor);
    let Some(placing) = placing(tool) else {
        return free;
    };
    let tolerance = SNAP_TOLERANCE * pixel;
    let snapped = snap_within(sketch, tool, placing, cursor, tolerance, pixel);
    snapped.filter(|snap| snap.at.is_finite()).unwrap_or(free)
}

/// Where the point `dragged` of `sketch` snaps with the cursor at
/// `cursor`, a pixel being `pixel`: as a drawing tool's click would,
/// without directions, to another point or the origin, then a midpoint or
/// a quadrant, then the nearest place on a curve, then on an axis. What's on a
/// curve the point is on is left out, so it isn't snapped to its own
/// curves nor to any of their points (a line's other end, an arc's
/// centre), but those it can be tied to ([`own_snaps`]): the other end
/// of an arc it's an end of, which closes it, and every other fit or
/// control point of a spline of three points or more it's one of. Free
/// where nothing's near.
pub(crate) fn snap_drag(sketch: &Sketch, dragged: Id, cursor: DVec2, pixel: f64) -> Snap {
    let tolerance = SNAP_TOLERANCE * pixel;
    let own: Vec<Id> = (sketch.curves.iter())
        .filter(|entry| entry.curve.points().any(|point| point == dragged))
        .map(|entry| entry.id)
        .collect();
    let closes: Vec<Id> = (own.iter())
        .flat_map(|&curve| own_snaps(sketch, curve, dragged))
        .collect();
    let neighbours: Vec<Id> = (own.iter())
        .filter_map(|&curve| sketch.curve(curve))
        .flat_map(|entry| entry.curve.points())
        .filter(|point| !closes.contains(point))
        .collect();
    let near = |candidates: Vec<Snap>| {
        candidates
            .into_iter()
            .filter(|snap| snap.at.is_finite())
            .map(|snap| (snap.at.distance(cursor), snap))
            .filter(|&(distance, _)| distance <= tolerance)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, snap)| snap)
    };
    let points = (sketch.points.iter())
        .filter(|point| point.id != dragged && !neighbours.contains(&point.id))
        .map(|point| (point.at, point.id))
        .chain([(DVec2::ZERO, Id::ORIGIN)])
        .filter(|&(_, id)| id != dragged)
        .map(|(at, id)| Snap::on(at, Target::Point(id)))
        .collect();
    let special = || {
        (special_points(sketch).into_iter())
            .filter(|snap| {
                snap.target
                    .and_then(Target::item)
                    .is_none_or(|item| !own.contains(&item))
            })
            .collect()
    };
    let on = |ids: &mut dyn Iterator<Item = Id>| {
        near(
            ids.filter_map(|id| Some(Snap::on(foot(sketch, id, cursor)?, Target::On(id))))
                .collect(),
        )
    };
    near(points)
        .or_else(|| near(special()))
        .or_else(|| {
            on(&mut sketch
                .curves
                .iter()
                .map(|entry| entry.id)
                .filter(|id| !own.contains(id)))
        })
        .or_else(|| on(&mut [Id::X_AXIS, Id::Y_AXIS].into_iter()))
        .unwrap_or(Snap::free(cursor))
}

/// Where the rim of the circle `circle` of `sketch`, dragged, snaps with
/// the cursor at `cursor`, a pixel being `pixel`: as a circle's rim drawn
/// does, only to points, another point of the sketch or the origin, not
/// its own centre. Free where nothing's near, or `circle` is no circle.
pub(crate) fn snap_rim(sketch: &Sketch, circle: Id, cursor: DVec2, pixel: f64) -> Snap {
    let Some(&Curve::Circle { center, .. }) = sketch.curve(circle).map(|entry| &entry.curve) else {
        return Snap::free(cursor);
    };
    let tolerance = SNAP_TOLERANCE * pixel;
    (sketch.points.iter())
        .filter(|point| point.id != center)
        .map(|point| (point.at, point.id))
        .chain([(DVec2::ZERO, Id::ORIGIN)])
        .filter(|&(at, id)| id != center && at.is_finite())
        .map(|(at, id)| (at.distance(cursor), Snap::on(at, Target::Point(id))))
        .filter(|&(distance, _)| distance <= tolerance)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(Snap::free(cursor), |(_, snap)| snap)
}

/// The points of the curve `curve` of `sketch` its point `point`,
/// dragged, snaps to: an arc's other end from it ([`closing`]), or, of
/// a spline of three points or more, open or closed, every other fit or
/// control point if `point` is one (not a handle's tip): fewer would
/// fold back on itself. None for a line or a circle.
fn own_snaps(sketch: &Sketch, curve: Id, point: Id) -> Vec<Id> {
    match sketch.curve(curve).map(|entry| &entry.curve) {
        Some(Curve::Spline(spline))
            if spline.points.len() >= 3 && spline.points.contains(&point) =>
        {
            (spline.points.iter())
                .copied()
                .filter(|&other| other != point)
                .collect()
        }
        Some(Curve::Arc { .. }) => closing(sketch, curve, point).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The other end of the curve `curve` of `sketch` from its end `end`,
/// which `end` dragged there closes it: an arc's, or an open spline's of
/// three points or more (fewer would fold back on itself). `None` for
/// any other curve, or a point that's no end of it.
pub fn closing(sketch: &Sketch, curve: Id, end: Id) -> Option<Id> {
    let entry = sketch.curve(curve)?;
    let [start, last] = entry.curve.ends()?;
    let closes = match &entry.curve {
        Curve::Arc { .. } => true,
        Curve::Spline(spline) => spline.points.len() >= 3,
        Curve::Line { .. } | Curve::Circle { .. } => false,
    };
    match (start == end, last == end) {
        _ if !closes => None,
        (true, false) => Some(last),
        (false, true) => Some(start),
        _ => None,
    }
}

/// Where the click of `tool`, placing `placing`, snaps with the cursor at
/// `cursor`, within `tolerance` sketch units, a pixel being `pixel`, if
/// anywhere.
fn snap_within(
    sketch: &Sketch,
    tool: &ActiveTool,
    placing: Placing,
    cursor: DVec2,
    tolerance: f64,
    pixel: f64,
) -> Option<Snap> {
    // The nearest within reach, the first of those as near.
    let near = |candidates: Vec<Snap>| {
        candidates
            .into_iter()
            .map(|snap| (snap.at.distance(cursor), snap))
            .filter(|&(distance, _)| distance <= tolerance)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, snap)| snap)
    };
    let start = match (tool.tool, tool.placed) {
        (Tool::Line, &[start]) => Some(start),
        _ => None,
    };
    // A line doesn't snap to the point it starts from, which would make
    // it no line.
    let points = sketch
        .points
        .iter()
        .map(|point| (point.at, point.id))
        .chain([(DVec2::ZERO, Id::ORIGIN)])
        .filter(|&(at, _)| Some(at) != start)
        .map(|(at, id)| Snap::on(at, Target::Point(id)))
        .collect();
    if let Some(snap) = near(own_points(tool, pixel)).or_else(|| near(points)) {
        return Some(snap);
    }
    if placing == Placing::Through {
        return match tool.tool {
            Tool::Arc => near(tangent_arcs(sketch, tool, cursor)),
            _ => None,
        };
    }
    if let Some(snap) = near(special_points(sketch)) {
        return Some(snap);
    }
    if let Some(start) = start
        && let Some(snap) = near(tangent_points(sketch, start))
    {
        return Some(snap);
    }
    let directions = start
        .map(|start| directions(sketch, tool, start))
        .unwrap_or_default();
    // Too near the start, a direction can't be told.
    let runs = |start: DVec2, direction: DVec2, at: DVec2| {
        (at - start).dot(direction).abs() >= MIN_RUN * pixel
    };
    let own = own_foot(tool, cursor).filter(|at| at.distance(cursor) <= tolerance);
    let curve = on_curve(sketch, cursor, tolerance);
    let foot_of = |curve| foot(sketch, curve, cursor);
    if let Some(at) = own
        && curve
            .and_then(foot_of)
            .is_none_or(|foot| at.distance(cursor) < foot.distance(cursor))
    {
        return Some(Snap::on(at, Target::Own(Own::Curve)));
    }
    if let Some(curve) = curve {
        let crossings = start.map_or_else(Vec::new, |start| {
            directions
                .iter()
                .flat_map(|&(inference, direction)| {
                    crossings(sketch, curve, start, direction)
                        .into_iter()
                        .filter(move |&at| runs(start, direction, at))
                        .map(move |at| Snap {
                            inference: Some(inference),
                            ..Snap::on(at, Target::On(curve))
                        })
                })
                .collect()
        });
        return near(crossings).or(Some(Snap::on(
            foot(sketch, curve, cursor)?,
            Target::On(curve),
        )));
    }
    let start = start?;
    let along = directions
        .iter()
        .filter_map(|&(inference, direction)| {
            let at = start + direction * (cursor - start).dot(direction);
            runs(start, direction, at).then_some(Snap {
                inference: Some(inference),
                ..Snap::free(at)
            })
        })
        .collect();
    near(along)
}

/// The points the Spline tool `tool` has placed that its next click
/// snaps to, a pixel being `pixel`: all but the last, and the first only
/// where a click there closes the spline.
fn own_points(tool: &ActiveTool, pixel: f64) -> Vec<Snap> {
    if tool.tool != Tool::Spline {
        return Vec::new();
    }
    let last = tool.placed.len().saturating_sub(1);
    (tool.placed.iter().enumerate())
        .filter(|&(index, &at)| index < last && (index > 0 || tool.closes(at, pixel)))
        .map(|(index, &at)| Snap::on(at, Target::Own(Own::Point(index))))
        .collect()
}

/// The place nearest `cursor` on the curve the Spline tool `tool` has
/// placed so far, as its preview draws it short of the cursor: straight
/// between its points while too few. None with fewer than two.
fn own_foot(tool: &ActiveTool, cursor: DVec2) -> Option<DVec2> {
    if tool.tool != Tool::Spline || tool.placed.len() < 2 {
        return None;
    }
    let line = flatten_spline(tool.placed, tool.spline_kind(), false)
        .unwrap_or_else(|| tool.placed.to_vec());
    (line.windows(2))
        .filter_map(|pair| {
            let along = pair[1] - pair[0];
            let t = ((cursor - pair[0]).dot(along) / along.length_squared()).clamp(0.0, 1.0);
            let at = pair[0] + along * t;
            at.is_finite().then_some(at)
        })
        .min_by(|a, b| a.distance(cursor).total_cmp(&b.distance(cursor)))
}

/// Lines' midpoints and circles' and arcs' quadrants.
fn special_points(sketch: &Sketch) -> Vec<Snap> {
    let mut found = Vec::new();
    for entry in &sketch.curves {
        let id = entry.id;
        match entry.curve {
            Curve::Line { .. } => {
                if let Some((start, end)) = sketch.line(id) {
                    found.push(Snap::on(start.midpoint(end), Target::Midpoint(id)));
                }
            }
            Curve::Circle { .. } | Curve::Arc { .. } => {
                let Some((center, radius)) = sketch.round(id) else {
                    continue;
                };
                for (direction, level) in [
                    (DVec2::X, Level::Horizontal),
                    (DVec2::Y, Level::Vertical),
                    (-DVec2::X, Level::Horizontal),
                    (-DVec2::Y, Level::Vertical),
                ] {
                    let at = center + direction * radius;
                    if on_round(sketch, id, at) {
                        found.push(Snap::on(at, Target::Quadrant { round: id, level }));
                    }
                }
            }
            // Its ends are points already.
            Curve::Spline(_) => {}
        }
    }
    found
}

/// Where a line from `start` touches a circle or an arc of `sketch`, each
/// on it as a tangent.
fn tangent_points(sketch: &Sketch, start: DVec2) -> Vec<Snap> {
    let mut found = Vec::new();
    for entry in &sketch.curves {
        let id = entry.id;
        let Some((center, radius)) = sketch.round(id) else {
            continue;
        };
        let away = start - center;
        let distance = away.length();
        // Only from outside it.
        if distance <= radius || distance.is_nan() {
            continue;
        }
        let spread = angle::acos(radius / distance);
        let toward = angle::to_angle(away);
        for turn in [toward + spread, toward - spread] {
            let at = center + radius * angle::from_angle(turn);
            if on_round(sketch, id, at) {
                found.push(Snap {
                    inference: Some(Inference::Tangent(id)),
                    ..Snap::on(at, Target::On(id))
                });
            }
        }
    }
    found
}

/// For the Arc tool's last click, the places on the arc through its ends
/// tangent at one of them to the line or arc ending there: each the
/// nearest place to `cursor` on such an arc.
fn tangent_arcs(sketch: &Sketch, tool: &ActiveTool, cursor: DVec2) -> Vec<Snap> {
    let (&[a, b], [ta, tb]) = (tool.placed, tool.targets) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (end, other, target) in [(a, b, *ta), (b, a, *tb)] {
        let Some(Target::Point(point)) = target else {
            continue;
        };
        for (curve, tangent) in tangents_at(sketch, point) {
            // The centre is on the normal at `end`, as far from `other`.
            let normal = tangent.perp();
            let chord = other - end;
            let across = normal.dot(chord);
            if across.abs() <= f64::EPSILON * chord.length() {
                continue;
            }
            let center = end + normal * (chord.length_squared() / (2.0 * across));
            let radius = center.distance(end);
            let Some(out) = (cursor - center).try_normalize() else {
                continue;
            };
            found.push(Snap {
                inference: Some(Inference::Tangent(curve)),
                ..Snap::free(center + out * radius)
            });
        }
    }
    found
}

/// The lines and arcs of `sketch` ending at the point `point`, each with
/// its direction there (either way along it).
fn tangents_at(sketch: &Sketch, point: Id) -> Vec<(Id, DVec2)> {
    let at = |id| sketch.point(id).map(|point| point.at);
    let mut found = Vec::new();
    for entry in &sketch.curves {
        let direction = match entry.curve {
            Curve::Line { start, end } if start == point || end == point => {
                at(end).zip(at(start)).map(|(end, start)| end - start)
            }
            Curve::Arc { center, start, end } if start == point || end == point => {
                at(point).zip(at(center)).map(|(p, c)| (p - c).perp())
            }
            _ => None,
        };
        if let Some(direction) = direction.and_then(DVec2::try_normalize) {
            found.push((entry.id, direction));
        }
    }
    found
}

/// The directions a line from `start`, drawn by `tool` in `sketch`, is
/// inferred along, each a unit vector with what it infers, in the order
/// they're preferred: horizontal, vertical, tangent to an arc ending at
/// the start or a circle or an arc it's on, perpendicular then parallel
/// to a line ending there or it's on.
fn directions(sketch: &Sketch, tool: &ActiveTool, start: DVec2) -> Vec<(Inference, DVec2)> {
    let mut found = vec![
        (Inference::Horizontal, DVec2::X),
        (Inference::Vertical, DVec2::Y),
    ];
    let mut at_start = match tool.targets.first().copied().flatten() {
        Some(Target::Point(point)) => tangents_at(sketch, point),
        Some(
            Target::On(curve) | Target::Midpoint(curve) | Target::Quadrant { round: curve, .. },
        ) => match (sketch.line(curve), sketch.round(curve)) {
            (Some((a, b)), _) => (b - a)
                .try_normalize()
                .map(|d| (curve, d))
                .into_iter()
                .collect(),
            (None, Some((center, _))) => (start - center)
                .perp()
                .try_normalize()
                .map(|d| (curve, d))
                .into_iter()
                .collect(),
            (None, None) => Vec::new(),
        },
        Some(Target::Own(_)) | None => Vec::new(),
    };
    at_start.retain(|&(curve, _)| !curve.is_builtin());
    let is_line = |curve| sketch.line(curve).is_some();
    for &(curve, direction) in &at_start {
        if !is_line(curve) {
            found.push((Inference::Tangent(curve), direction));
        }
    }
    for &(curve, direction) in &at_start {
        if is_line(curve) {
            found.push((Inference::Perpendicular(curve), direction.perp()));
        }
    }
    for &(curve, direction) in &at_start {
        if is_line(curve) {
            found.push((Inference::Parallel(curve), direction));
        }
    }
    found
}

/// The curve of `sketch` nearest `cursor` within `tolerance`, or failing
/// one the axis.
fn on_curve(sketch: &Sketch, cursor: DVec2, tolerance: f64) -> Option<Id> {
    let distances = |ids: &mut dyn Iterator<Item = Id>| {
        let distances = ids.filter_map(|id| Some((id, foot(sketch, id, cursor)?.distance(cursor))));
        hit::nearest(distances, tolerance)
    };
    distances(&mut sketch.curves.iter().map(|entry| entry.id))
        .or_else(|| distances(&mut [Id::X_AXIS, Id::Y_AXIS].into_iter()))
}

/// The place on the curve (or axis) `id` of `sketch` nearest `cursor`: on
/// a line between its ends, on an axis anywhere, on an arc within it, on
/// a spline anywhere along it.
fn foot(sketch: &Sketch, id: Id, cursor: DVec2) -> Option<DVec2> {
    if let Some((center, radius)) = sketch.round(id) {
        let at = center + (cursor - center).try_normalize()? * radius;
        return on_round(sketch, id, at).then_some(at);
    }
    let Some((start, end)) = sketch.line(id) else {
        return sketch.nearest_on(id, cursor);
    };
    let along = end - start;
    let t = (cursor - start).dot(along) / along.length_squared();
    let t = if id.is_axis() { t } else { t.clamp(0.0, 1.0) };
    let at = start + along * t;
    at.is_finite().then_some(at)
}

/// Where the line through `start` along `direction` crosses the curve
/// (or axis) `id` of `sketch`: within a line's ends, anywhere on an axis,
/// within an arc.
fn crossings(sketch: &Sketch, id: Id, start: DVec2, direction: DVec2) -> Vec<DVec2> {
    if let Some((center, radius)) = sketch.round(id) {
        // |start + direction t - center| = radius, `direction` a unit.
        let to = start - center;
        let half_b = direction.dot(to);
        let c = to.length_squared() - radius * radius;
        let discriminant = half_b * half_b - c;
        if discriminant < 0.0 {
            return Vec::new();
        }
        let root = discriminant.sqrt();
        return [-half_b - root, -half_b + root]
            .into_iter()
            .map(|t| start + direction * t)
            .filter(|&at| on_round(sketch, id, at))
            .collect();
    }
    let Some((a, b)) = sketch.line(id) else {
        return Vec::new();
    };
    let along = b - a;
    let Some((_, u)) = crossing(start, direction, a, along) else {
        return Vec::new();
    };
    let at = a + along * u;
    let within = id.is_axis() || (0.0..=1.0).contains(&u);
    if within && at.is_finite() {
        vec![at]
    } else {
        Vec::new()
    }
}

/// Whether `at`, on the circle of the circle or arc `id` of `sketch`, is
/// on it: anywhere on a circle, within the sweep of an arc.
fn on_round(sketch: &Sketch, id: Id, at: DVec2) -> bool {
    let Some(entry) = sketch.curve(id) else {
        return false;
    };
    match entry.curve {
        Curve::Circle { .. } => true,
        Curve::Arc { center, start, end } => {
            let at_of = |id| sketch.point(id).map(|point| point.at);
            let (Some(center), Some(start), Some(end)) = (at_of(center), at_of(start), at_of(end))
            else {
                return false;
            };
            let from = start - center;
            sector_holds(from, arc_sweep(from, end - center), at - center)
        }
        Curve::Line { .. } | Curve::Spline(_) => false,
    }
}

#[cfg(test)]
mod tests;
