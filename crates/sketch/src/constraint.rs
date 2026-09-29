//! Constraints: the geometric relations between a sketch's items that the
//! solver holds.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::{Curve, Id, Joint, Kind, Sketch};

/// Which of two mirror-image solutions a constraint was made on, stored so
/// the solver can't hop from one to the other: each constraint's equation
/// is signed by it. What each side means is up to the constraint, see
/// [`Constraint::Tangent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Positive,
    Negative,
}

impl Side {
    /// `Positive` for zero and above, `Negative` below.
    pub fn of(value: f64) -> Side {
        if value < 0.0 {
            Side::Negative
        } else {
            Side::Positive
        }
    }

    /// 1 or -1.
    pub fn sign(self) -> f64 {
        match self {
            Side::Positive => 1.0,
            Side::Negative => -1.0,
        }
    }
}

/// A geometric relation between items of a sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    /// Two points at the same place.
    Coincident(Id, Id),
    /// A point on a curve: anywhere on the endless line through a line,
    /// on a circle, or on the circle an arc is part of.
    PointOnCurve { point: Id, curve: Id },
    /// A line parallel to the x axis.
    Horizontal(Id),
    /// A line parallel to the y axis.
    Vertical(Id),
    /// Two points at the same y.
    HorizontalPoints(Id, Id),
    /// Two points at the same x.
    VerticalPoints(Id, Id),
    /// Two lines parallel, running the same way or opposite ways.
    Parallel(Id, Id),
    /// Two lines at right angles.
    Perpendicular(Id, Id),
    /// Two curves touching without crossing, not both lines. A line and a
    /// circle or an arc (either way round): the circle's centre is on the
    /// `side` of the line, [`Side::Positive`] being left looking from its
    /// start to its end. Two circles or arcs: [`Side::Positive`] touching
    /// from outside each other, [`Side::Negative`] with `b` inside `a`.
    /// `at` is `None` for those.
    ///
    /// With a spline, either or both: they touch at `at`, an end of the
    /// spline (of both, where both are splines, which share it), and run
    /// along each other there (see [`Joint`]). Each has its way along
    /// there: a line's from its start to its end, a circle's or an arc's
    /// counter-clockwise, a spline's as its parameter runs; with
    /// [`Side::Positive`] they run the same way, with [`Side::Negative`]
    /// opposite ways.
    ///
    /// [`Sketch::tangent`] makes one with the side the geometry is on.
    Tangent {
        a: Id,
        b: Id,
        side: Side,
        at: Option<Id>,
    },
    /// A spline's end joined smoothly to a line, a circle, an arc or
    /// another spline: tangent at `at` as [`Constraint::Tangent`] with a
    /// spline, `side` saying the same, and curving as much there, the
    /// same way round, so the join shows no kink in reflections. A spline
    /// through fit points needs a handle at `at`, else its end is
    /// straight there whatever moves. [`Sketch::smooth`] makes one.
    Smooth { a: Id, b: Id, at: Id, side: Side },
    /// Two lines of the same length, or two circles or arcs (either) of
    /// the same radius.
    Equal(Id, Id),
    /// Two circles or arcs about the same centre, or a point at the centre
    /// of one.
    Concentric(Id, Id),
    /// A point halfway along a line.
    Midpoint { point: Id, line: Id },
    /// Two points mirror images of each other in the line `about`.
    Symmetric { a: Id, b: Id, about: Id },
    /// Two offset pairs as far apart as each other: the second of each
    /// pair lies as far from the first as the second of the other does
    /// from its first, either side (see [`Measure::Offset`](crate::Measure::Offset)
    /// for what the pairs are and how far each is). What
    /// [`SketchEdit::Offset`](crate::SketchEdit::Offset) ties every
    /// piece of a copy to its original by, so one dimension drives the
    /// whole offset.
    EqualOffset { a: [Id; 2], b: [Id; 2] },
    /// A point or a curve pinned where it is: the solver never moves it.
    Fix(Id),
}

/// What a constraint needs an item it names to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Point,
    Line,
    /// A circle or an arc.
    Round,
    /// A line, a circle or an arc.
    Curve,
    /// A line, a circle, an arc or a spline.
    AnyCurve,
    /// A point, a circle or an arc.
    PointOrRound,
    /// A point or a line.
    PointOrLine,
    /// A line, or a handle by its tip, a point (that it's a handle's tip
    /// is [`Measure::fits`](crate::Measure::fits)' to say).
    LineOrHandle,
    /// A point or a curve, a spline included.
    Geometry,
}

impl Role {
    /// Whether an item of `kind` can play the role.
    pub fn admits(self, kind: Kind) -> bool {
        let round = matches!(kind, Kind::Circle | Kind::Arc);
        let curve = round || kind == Kind::Line;
        let any_curve = curve || kind == Kind::Spline;
        match self {
            Role::Point => kind == Kind::Point,
            Role::Line => kind == Kind::Line,
            Role::Round => round,
            Role::Curve => curve,
            Role::AnyCurve => any_curve,
            Role::PointOrRound => round || kind == Kind::Point,
            Role::PointOrLine | Role::LineOrHandle => kind == Kind::Point || kind == Kind::Line,
            Role::Geometry => any_curve || kind == Kind::Point,
        }
    }

    /// The role as an error message names it: "which is no {name}".
    pub fn name(self) -> &'static str {
        match self {
            Role::Point => "point",
            Role::Line => "line",
            Role::Round => "circle or arc",
            Role::Curve => "line, circle or arc",
            Role::AnyCurve => "curve",
            Role::PointOrRound => "point, circle or arc",
            Role::PointOrLine => "point or line",
            Role::LineOrHandle => "line or handle",
            Role::Geometry => "point or curve",
        }
    }
}

impl Constraint {
    /// The items the constraint ties together, each with the role it
    /// needs it to play.
    pub fn items(&self) -> impl Iterator<Item = (Id, Role)> + Clone + use<> {
        use Role::*;
        match *self {
            Constraint::Coincident(a, b)
            | Constraint::HorizontalPoints(a, b)
            | Constraint::VerticalPoints(a, b) => [Some((a, Point)), Some((b, Point)), None, None],
            Constraint::PointOnCurve { point, curve } => {
                [Some((point, Point)), Some((curve, AnyCurve)), None, None]
            }
            Constraint::Horizontal(line) | Constraint::Vertical(line) => {
                [Some((line, Line)), None, None, None]
            }
            Constraint::Parallel(a, b) | Constraint::Perpendicular(a, b) => {
                [Some((a, Line)), Some((b, Line)), None, None]
            }
            Constraint::Tangent { a, b, at: None, .. } | Constraint::Equal(a, b) => {
                [Some((a, Curve)), Some((b, Curve)), None, None]
            }
            Constraint::Tangent {
                a, b, at: Some(at), ..
            }
            | Constraint::Smooth { a, b, at, .. } => [
                Some((a, AnyCurve)),
                Some((b, AnyCurve)),
                Some((at, Point)),
                None,
            ],
            Constraint::Concentric(a, b) => {
                [Some((a, PointOrRound)), Some((b, PointOrRound)), None, None]
            }
            Constraint::Midpoint { point, line } => {
                [Some((point, Point)), Some((line, Line)), None, None]
            }
            Constraint::Symmetric { a, b, about } => [
                Some((a, Point)),
                Some((b, Point)),
                Some((about, Line)),
                None,
            ],
            Constraint::EqualOffset {
                a: [a, a_copy],
                b: [b, b_copy],
            } => [
                Some((a, Geometry)),
                Some((a_copy, Geometry)),
                Some((b, Geometry)),
                Some((b_copy, Geometry)),
            ],
            Constraint::Fix(item) => [Some((item, Geometry)), None, None, None],
        }
        .into_iter()
        .flatten()
    }

    /// The same constraint on the items `map` gives for its own.
    pub(crate) fn map_items<E>(
        &self,
        mut map: impl FnMut(Id) -> Result<Id, E>,
    ) -> Result<Constraint, E> {
        use Constraint::*;
        Ok(match *self {
            Coincident(a, b) => Coincident(map(a)?, map(b)?),
            PointOnCurve { point, curve } => PointOnCurve {
                point: map(point)?,
                curve: map(curve)?,
            },
            Horizontal(line) => Horizontal(map(line)?),
            Vertical(line) => Vertical(map(line)?),
            HorizontalPoints(a, b) => HorizontalPoints(map(a)?, map(b)?),
            VerticalPoints(a, b) => VerticalPoints(map(a)?, map(b)?),
            Parallel(a, b) => Parallel(map(a)?, map(b)?),
            Perpendicular(a, b) => Perpendicular(map(a)?, map(b)?),
            Tangent { a, b, side, at } => Tangent {
                a: map(a)?,
                b: map(b)?,
                side,
                at: at.map(&mut map).transpose()?,
            },
            Smooth { a, b, at, side } => Smooth {
                a: map(a)?,
                b: map(b)?,
                at: map(at)?,
                side,
            },
            Equal(a, b) => Equal(map(a)?, map(b)?),
            Concentric(a, b) => Concentric(map(a)?, map(b)?),
            Midpoint { point, line } => Midpoint {
                point: map(point)?,
                line: map(line)?,
            },
            Symmetric { a, b, about } => Symmetric {
                a: map(a)?,
                b: map(b)?,
                about: map(about)?,
            },
            EqualOffset {
                a: [a, a_copy],
                b: [b, b_copy],
            } => EqualOffset {
                a: [map(a)?, map(a_copy)?],
                b: [map(b)?, map(b_copy)?],
            },
            Fix(item) => Fix(map(item)?),
        })
    }

    /// Whether the kinds of the items it names in `sketch`, each already
    /// playing its role (see [`items`](Constraint::items)), go together: a
    /// tangent isn't between two lines, and is at a spline's end
    /// ([`Joint`]) where there's a spline, as a smooth join is, equal is
    /// two lines or two round curves, concentric isn't two points, and an
    /// equal offset's pairs are offset pairs ([`Sketch::offset_pair`]).
    pub fn fits(&self, sketch: &Sketch) -> bool {
        let round = |id| matches!(sketch.kind(id), Some(Kind::Circle | Kind::Arc));
        match *self {
            Constraint::Tangent { a, b, at: None, .. } => round(a) || round(b),
            Constraint::Tangent {
                a, b, at: Some(at), ..
            } => Joint::of(sketch, a, b, at).is_some(),
            Constraint::Smooth { a, b, at, .. } => {
                Joint::of(sketch, a, b, at).is_some_and(|joint| joint.curves(sketch, at))
            }
            Constraint::Equal(a, b) => round(a) == round(b),
            Constraint::Concentric(a, b) => round(a) || round(b),
            Constraint::EqualOffset { a, b } => {
                sketch.offset_pair(a).is_some() && sketch.offset_pair(b).is_some()
            }
            _ => true,
        }
    }

    /// A curve the constraint names and one of that curve's own points it
    /// names too, if it does: a point on its own line, or at its own
    /// circle's centre, which is true or false whatever the solver does,
    /// so [`Sketch::check`] refuses it. Not an equal offset's round join
    /// about its corner ([`Sketch::offset_pair`]), which is meant.
    pub fn own_point(&self, sketch: &Sketch) -> Option<(Id, Id)> {
        match *self {
            Constraint::EqualOffset { .. } => None,
            // Where a spline touches is its own end, as is meant.
            Constraint::Tangent { at: Some(_), .. } | Constraint::Smooth { .. } => None,
            _ => own_point(self.items(), sketch),
        }
    }
}

/// A curve among `items` and one of that curve's own points among them
/// too, if there is one, see [`Constraint::own_point`].
pub(crate) fn own_point(
    items: impl Iterator<Item = (Id, Role)> + Clone,
    sketch: &Sketch,
) -> Option<(Id, Id)> {
    items.clone().find_map(|(curve, _)| {
        let point = sketch
            .curve(curve)?
            .curve
            .points()
            .find(|&point| items.clone().any(|(id, _)| id == point))?;
        Some((curve, point))
    })
}

/// A constraint of a sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintEntry {
    pub id: Id,
    pub constraint: Constraint,
}

impl Sketch {
    /// A line's start and end, an axis's the origin and a unit along it.
    /// `None` for anything else, or a point missing.
    pub fn line(&self, id: Id) -> Option<(DVec2, DVec2)> {
        if let Some(axis) = crate::origin::axis(id) {
            return Some(axis);
        }
        match self.curve(id)?.curve {
            Curve::Line { start, end } => Some((self.point(start)?.at, self.point(end)?.at)),
            Curve::Circle { .. } | Curve::Arc { .. } | Curve::Spline(_) => None,
        }
    }

    /// A circle's or an arc's centre and radius: an arc's is its start's
    /// distance from its centre. `None` for anything else, or a point
    /// missing.
    pub fn round(&self, id: Id) -> Option<(DVec2, f64)> {
        let at = |id| self.point(id).map(|point| point.at);
        match self.curve(id)?.curve {
            Curve::Circle { center, radius } => Some((at(center)?, radius)),
            Curve::Arc { center, start, .. } => {
                let center = at(center)?;
                Some((center, at(start)?.distance(center)))
            }
            Curve::Line { .. } | Curve::Spline(_) => None,
        }
    }

    /// A [`Constraint::Tangent`] between `a` and `b` on the side their
    /// geometry is nearer to, see [`tangent_between`], or with a spline at
    /// the end [`Sketch::joint`] picks. `None` unless one is a circle or an
    /// arc and the other a line, a circle or an arc, or one is a spline
    /// with an end to touch the other at.
    pub fn tangent(&self, a: Id, b: Id) -> Option<Constraint> {
        if let Some((at, side)) = self.joint(a, b) {
            return Some(Constraint::Tangent {
                a,
                b,
                side,
                at: Some(at),
            });
        }
        tangent_between((a, self.shape(a)?), (b, self.shape(b)?))
    }

    /// The shape of the curve `id`, or an axis, for [`tangent_between`].
    pub fn shape(&self, id: Id) -> Option<Shape> {
        match self.round(id) {
            Some((center, radius)) => Some(Shape::Round(center, radius)),
            None => self.line(id).map(|(start, end)| Shape::Line(start, end)),
        }
    }
}

/// A curve's shape, whether in a sketch or not yet: a line's ends, or a
/// circle's or an arc's centre and radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Line(DVec2, DVec2),
    Round(DVec2, f64),
}

/// A [`Constraint::Tangent`] between the curves `a` and `b`, each with its
/// shape, on the side their geometry is nearer to: a circle or an arc on
/// the side of a line its centre is on, two of them outside each other or
/// one inside the other, whichever they're nearer to, the larger first
/// when inside. `None` for two lines.
pub fn tangent_between((a, a_shape): (Id, Shape), (b, b_shape): (Id, Shape)) -> Option<Constraint> {
    let side_of =
        |start: DVec2, end: DVec2, center: DVec2| Side::of((end - start).perp_dot(center - start));
    Some(match (a_shape, b_shape) {
        (Shape::Round(ca, ra), Shape::Round(cb, rb)) => {
            let distance = ca.distance(cb);
            let inside = (distance - (ra - rb).abs()).abs() < (distance - (ra + rb)).abs();
            match (inside, ra >= rb) {
                (false, _) => Constraint::Tangent {
                    a,
                    b,
                    side: Side::Positive,
                    at: None,
                },
                (true, larger) => Constraint::Tangent {
                    a: if larger { a } else { b },
                    b: if larger { b } else { a },
                    side: Side::Negative,
                    at: None,
                },
            }
        }
        (Shape::Round(center, _), Shape::Line(start, end)) => Constraint::Tangent {
            a,
            b,
            side: side_of(start, end, center),
            at: None,
        },
        (Shape::Line(start, end), Shape::Round(center, _)) => Constraint::Tangent {
            a,
            b,
            side: side_of(start, end, center),
            at: None,
        },
        (Shape::Line(..), Shape::Line(..)) => return None,
    })
}
