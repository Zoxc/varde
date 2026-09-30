//! Dimensions: sizes and angles the user gives a sketch, as typed values
//! ([`Value`]), which either hold the geometry to them (driving) or only
//! measure it (reference).

use std::collections::HashSet;
use std::f64::consts::{PI, TAU};

use glam::DVec2;

use crate::angle;
use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Quantity, Value};

use crate::constraint::own_point;
use crate::{Design, EditError, Id, Role, Side, Sketch, crossing, foot};

/// The least length a dimension may be, in millimetres: a micrometre. The
/// solver holds equations to a ten-billionth of the sketch's size, which
/// at the coordinate limit (the kernel's `MAX_COORD`, a kilometre) is a
/// tenth of a micrometre, so a length ten times that is
/// still told from none, while being finer than anything made is.
pub const MIN_LENGTH: f64 = 1e-3;

/// What a dimension measures, by the items it names. Each is signed by
/// the dimension's [`Side`] where the geometry has two mirror images, so
/// its equation can't reach the other one: a value that would flip the
/// shape isn't one the solver can get to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Measure {
    /// Between two points; a point and the endless line through a line,
    /// either way round; or two lines, as the distance of the first's
    /// midpoint from the line through the second, which is the distance
    /// between them when they're parallel. With a line, the point (or the
    /// first line's midpoint) is on the `side` of it: [`Side::Positive`]
    /// left, looking from its start to its end. Between two points the
    /// side means nothing.
    Distance(Id, Id),
    /// How far the second point is to the right of the first
    /// ([`Side::Positive`]) or to the left ([`Side::Negative`]).
    HorizontalDistance(Id, Id),
    /// How far the second point is above the first ([`Side::Positive`])
    /// or below ([`Side::Negative`]).
    VerticalDistance(Id, Id),
    /// A line's length.
    Length(Id),
    /// Counter-clockwise from the first line's direction (start to end),
    /// reversed where the side is [`Side::Negative`], to the second
    /// line's: which of the angles between two lines is the order of the
    /// lines and the side. [`Sketch::side`] picks the side under half a
    /// turn. Either may be a spline's handle instead, named by its tip,
    /// running from its fit point to its tip ([`Sketch::direction`]).
    Angle(Id, Id),
    /// A circle's or an arc's radius.
    Radius(Id),
    /// A circle's or an arc's diameter.
    Diameter(Id),
    /// How far the second of an offset pair lies from the first (see
    /// [`Sketch::offset_pair`]): a line's midpoint from the line through
    /// another, [`Side::Positive`] left of it; a circle's or an arc's
    /// radius less another's, [`Side::Positive`] larger; a round join's
    /// radius about its corner. What drives a copy
    /// [`SketchEdit::Offset`](crate::SketchEdit::Offset) makes.
    Offset(Id, Id),
}

impl Measure {
    /// The items it measures, each with the role it needs it to play.
    pub fn items(&self) -> impl Iterator<Item = (Id, Role)> + Clone + use<> {
        use Role::*;
        match *self {
            Measure::Distance(a, b) => [Some((a, PointOrLine)), Some((b, PointOrLine))],
            Measure::HorizontalDistance(a, b) | Measure::VerticalDistance(a, b) => {
                [Some((a, Point)), Some((b, Point))]
            }
            Measure::Length(line) => [Some((line, Line)), None],
            Measure::Angle(a, b) => [Some((a, LineOrHandle)), Some((b, LineOrHandle))],
            Measure::Radius(round) | Measure::Diameter(round) => [Some((round, Round)), None],
            // Which go together is `fits`'s to say: a point may be a
            // spline's copy's.
            Measure::Offset(a, b) => [Some((a, Geometry)), Some((b, Geometry))],
        }
        .into_iter()
        .flatten()
    }

    /// A line it names and one of that line's own ends it names too, if
    /// it does: no distance to measure, so [`Sketch::check`] refuses it,
    /// as [`Constraint::own_point`](crate::Constraint::own_point). Not an
    /// offset of a round join from its corner, which is meant.
    pub fn own_point(&self, sketch: &Sketch) -> Option<(Id, Id)> {
        if let Measure::Offset(..) = self {
            return None;
        }
        own_point(self.items(), sketch)
    }

    /// The same measure of the items `map` gives for its own.
    pub(crate) fn map_items<E>(
        &self,
        mut map: impl FnMut(Id) -> Result<Id, E>,
    ) -> Result<Measure, E> {
        use Measure::*;
        Ok(match *self {
            Distance(a, b) => Distance(map(a)?, map(b)?),
            HorizontalDistance(a, b) => HorizontalDistance(map(a)?, map(b)?),
            VerticalDistance(a, b) => VerticalDistance(map(a)?, map(b)?),
            Length(line) => Length(map(line)?),
            Angle(a, b) => Angle(map(a)?, map(b)?),
            Radius(round) => Radius(map(round)?),
            Diameter(round) => Diameter(map(round)?),
            Offset(a, b) => Offset(map(a)?, map(b)?),
        })
    }

    /// Whether the items it names in `sketch`, each already playing its
    /// role (see [`items`](Measure::items)), are what it measures: an
    /// offset's an offset pair ([`Sketch::offset_pair`]), and a point an
    /// angle names a handle's tip, one of `tips` ([`Sketch::tips`]).
    pub fn fits(&self, sketch: &Sketch, tips: &HashSet<Id>) -> bool {
        match *self {
            Measure::Offset(a, b) => sketch.offset_pair([a, b]).is_some(),
            Measure::Angle(a, b) => [a, b]
                .into_iter()
                .all(|id| sketch.line(id).is_some() || tips.contains(&id)),
            _ => true,
        }
    }

    /// Whether it's an angle or a length.
    pub fn quantity(&self) -> Quantity {
        match self {
            Measure::Angle(..) => Quantity::Angle,
            _ => Quantity::Length,
        }
    }

    /// What its value must be, read in `design`'s units: an angle above
    /// zero and under a turn, or a length of at least [`MIN_LENGTH`] and
    /// at most the design's `max` (twice that for a diameter, as a radius
    /// may be `max`).
    pub fn ask(&self, design: &Design) -> Ask {
        match self {
            Measure::Angle(..) => Ask::angle(design.units, TAU).positive().under_max(),
            Measure::Diameter(_) => Ask::length(design.units, 2.0 * design.max)
                .positive()
                .at_least(MIN_LENGTH),
            _ => Ask::length(design.units, design.max)
                .positive()
                .at_least(MIN_LENGTH),
        }
    }
}

/// A dimension of a sketch: what it measures, its value as typed, and
/// where its label is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub measure: Measure,
    /// The expression as typed and its value in model units, which
    /// [`Sketch::check`] re-evaluates in the design's units. A reference
    /// dimension keeps the value it had when it was made or stopped
    /// driving; what it shows is [`Sketch::measure`].
    pub value: Value,
    /// Whether it holds the geometry to its value (an equation of the
    /// solver's), or is a reference, only measuring it.
    pub driving: bool,
    /// Where its label is, from the measure's [`Sketch::anchor`], so it
    /// follows the geometry.
    pub label: DVec2,
    /// Which of two mirror images it holds, see [`Measure`].
    pub side: Side,
}

/// A dimension of a sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionEntry {
    pub id: Id,
    pub dimension: Dimension,
}

/// A point or a line, as a [`Measure::Distance`] names it.
#[derive(Debug, Clone, Copy)]
enum End {
    Point(DVec2),
    Line(DVec2, DVec2),
}

/// The signed distance of `point` from the line through `start` and
/// `end`, positive on its left. Not a number for a line of no length.
fn from_line(point: DVec2, start: DVec2, end: DVec2) -> f64 {
    let along = end - start;
    along.perp_dot(point - start) / along.length()
}

impl Sketch {
    fn end(&self, id: Id) -> Option<End> {
        match self.point(id) {
            Some(point) => Some(End::Point(point.at)),
            None => self.line(id).map(|(start, end)| End::Line(start, end)),
        }
    }

    /// Two points, or a point and a line (the point first, and whether it
    /// was so), or two lines: what a [`Measure::Distance`] of `a` and `b`
    /// is between, as the sided distance and its anchor. `None` if either
    /// is missing or of another kind.
    fn distance(&self, a: Id, b: Id) -> Option<(f64, DVec2, Side)> {
        let (a, b) = (self.end(a)?, self.end(b)?);
        let sided = |point: DVec2, start, end| {
            let distance = from_line(point, start, end);
            let anchor = point.midpoint(foot(point, start, end));
            (distance, anchor, Side::of(distance))
        };
        Some(match (a, b) {
            (End::Point(a), End::Point(b)) => (a.distance(b), a.midpoint(b), Side::Positive),
            (End::Point(point), End::Line(start, end))
            | (End::Line(start, end), End::Point(point)) => sided(point, start, end),
            (End::Line(a_start, a_end), End::Line(start, end)) => {
                sided(a_start.midpoint(a_end), start, end)
            }
        })
    }

    /// The tips of the handles of the sketch's splines, which an angle
    /// may name ([`Measure::fits`]).
    pub fn tips(&self) -> HashSet<Id> {
        self.splines()
            .flat_map(|(_, spline)| spline.handles.iter().map(|handle| handle.tip))
            .collect()
    }

    /// The counter-clockwise angle from `a`'s direction, reversed if
    /// `side` is negative, to `b`'s, in `[0, 2π)`, if both are lines or
    /// handles ([`Sketch::direction`]).
    fn angle(&self, a: Id, b: Id, side: Side) -> Option<f64> {
        let (a_start, a_end) = self.direction(a)?;
        let (b_start, b_end) = self.direction(b)?;
        let (u, w) = ((a_end - a_start) * side.sign(), b_end - b_start);
        let angle = angle::atan2(u.perp_dot(w), u.dot(w));
        Some(if angle < 0.0 { angle + TAU } else { angle })
    }

    /// What `measure` comes to now, sided as a dimension on `side` holds
    /// it: the value the dimension would have to be to hold here, which
    /// is what a reference dimension shows, and what a driving one's value
    /// is once solved. Negative where the geometry is on the other side
    /// (show its size); `None` if an item is missing or of another kind,
    /// or it isn't a number (a line of no length).
    pub fn measure(&self, measure: &Measure, side: Side) -> Option<f64> {
        let at = |id| self.point(id).map(|point| point.at);
        let value = match *measure {
            Measure::Distance(a, b) => {
                let (distance, _, _) = self.distance(a, b)?;
                let line = self.line(a).is_some() || self.line(b).is_some();
                if line {
                    distance * side.sign()
                } else {
                    distance
                }
            }
            Measure::HorizontalDistance(a, b) => (at(b)?.x - at(a)?.x) * side.sign(),
            Measure::VerticalDistance(a, b) => (at(b)?.y - at(a)?.y) * side.sign(),
            Measure::Length(line) => {
                let (start, end) = self.line(line)?;
                start.distance(end)
            }
            Measure::Angle(a, b) => self.angle(a, b, side)?,
            Measure::Radius(round) => self.round(round)?.1,
            Measure::Diameter(round) => 2.0 * self.round(round)?.1,
            Measure::Offset(a, b) => self.offset_of([a, b])? * side.sign(),
        };
        value.is_finite().then_some(value)
    }

    /// The side the geometry `measure` names is on now, for a dimension
    /// made of it to hold: the side a point is of a line, whether the
    /// second point is right of (or above) the first, and for an angle
    /// the side under half a turn. [`Side::Positive`] where it doesn't
    /// matter or can't be told.
    pub fn side(&self, measure: &Measure) -> Side {
        let side = match *measure {
            Measure::Distance(a, b) => self.distance(a, b).map(|(_, _, side)| side),
            Measure::HorizontalDistance(..) | Measure::VerticalDistance(..) => {
                self.measure(measure, Side::Positive).map(Side::of)
            }
            Measure::Angle(a, b) => self.angle(a, b, Side::Positive).map(|angle| {
                if angle < PI {
                    Side::Positive
                } else {
                    Side::Negative
                }
            }),
            Measure::Offset(a, b) => self.offset_of([a, b]).map(Side::of),
            Measure::Length(_) | Measure::Radius(_) | Measure::Diameter(_) => None,
        };
        side.unwrap_or(Side::Positive)
    }

    /// Whether `measure` is the distance between two points at one place,
    /// or the length of a line whose ends are: nothing a driving dimension
    /// could hold, as there's no telling which way to move them apart.
    pub fn same_place(&self, measure: &Measure) -> bool {
        let at = |id| self.point(id).map(|point| point.at);
        match *measure {
            Measure::Distance(a, b) => at(a).is_some_and(|a| Some(a) == at(b)),
            Measure::Length(line) => self.line(line).is_some_and(|(start, end)| start == end),
            _ => false,
        }
    }

    /// The value and side a dimension of `measure`, on `side`, holds the
    /// geometry where it is at: what it measures, as shown in the
    /// design's units ([`format()`](varde_expr::format())), within a few
    /// micrometres of it; on the side the geometry is on, where it has
    /// crossed to the other since. For a reference placed, or one made
    /// driving. Refused, saying why, where the points are at one place
    /// ([`EditError::SamePlace`]), or what it measures is past what a
    /// dimension can be ([`EditError::OutOfRange`]).
    pub fn held(
        &self,
        measure: &Measure,
        side: Side,
        design: &Design,
    ) -> Result<(Value, Side), EditError> {
        let measured = match self.measure(measure, side) {
            Some(value) if value > 0.0 => Some((value, side)),
            _ => {
                let side = self.side(measure);
                self.measure(measure, side).map(|value| (value, side))
            }
        };
        // Not a number: a line whose ends are at one place.
        let Some((measured, side)) = measured.filter(|_| !self.same_place(measure)) else {
            return Err(EditError::SamePlace);
        };
        let ask = measure.ask(design);
        let value = Value::new(&varde_expr::format(measured, ask.unit()), &ask);
        let out = EditError::OutOfRange {
            measured,
            min: ask.min.unwrap_or(0.0),
            max: ask.max,
            angle: measure.quantity() == Quantity::Angle,
        };
        Ok((value.map_err(|_| out)?, side))
    }

    /// Where a dimension of `measure` is anchored, its label placed from:
    /// between the points, between a point and its nearest place on a
    /// line, a line's middle, where two lines meet (or between their
    /// middles, parallel), a circle's centre, halfway across an offset.
    /// `None` if an item is missing or of another kind.
    pub fn anchor(&self, measure: &Measure) -> Option<DVec2> {
        let at = |id| self.point(id).map(|point| point.at);
        let middle = |id| self.line(id).map(|(start, end)| start.midpoint(end));
        let anchor = match *measure {
            Measure::Distance(a, b) => self.distance(a, b)?.1,
            Measure::HorizontalDistance(a, b) | Measure::VerticalDistance(a, b) => {
                at(a)?.midpoint(at(b)?)
            }
            Measure::Length(line) => middle(line)?,
            Measure::Angle(a, b) => {
                let ((a_start, a_end), (b_start, b_end)) = (self.direction(a)?, self.direction(b)?);
                let u = a_end - a_start;
                match crossing(a_start, u, b_start, b_end - b_start) {
                    Some((t, _)) => a_start + u * t,
                    // Near parallel, where they meet is far off, or nowhere.
                    None => {
                        let middle =
                            |id| self.direction(id).map(|(start, end)| start.midpoint(end));
                        middle(a)?.midpoint(middle(b)?)
                    }
                }
            }
            Measure::Radius(round) | Measure::Diameter(round) => self.round(round)?.0,
            Measure::Offset(a, b) => self.offset_anchor([a, b])?,
        };
        anchor.is_finite().then_some(anchor)
    }
}

impl Sketch {
    /// Writes `design`'s length unit into every dimension's expression
    /// after each bare number that took it ([`varde_expr::pin_units`]), so
    /// each means the same in any units: for changing the design's units,
    /// `design` being the design before. Values stay exactly as they were.
    /// An expression the units would take past
    /// [`MAX_LEN`](varde_expr::MAX_LEN) is replaced by its value, exactly,
    /// in model units: its shortest exact decimal and `mm` or `rad`, such
    /// as `1.25e1 mm`.
    pub fn pin_units(&mut self, design: &Design) {
        for entry in &mut self.dimensions {
            let dimension = &mut entry.dimension;
            // Pinned, it has no bare lengths left, so it's the same value
            // in any units if it's the same in these.
            dimension.value.pin_units(&dimension.measure.ask(design));
        }
    }
}

#[cfg(test)]
mod tests;
