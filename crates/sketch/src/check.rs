//! [`Sketch::check`]: what a file could get wrong about a sketch.

use std::collections::HashSet;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use varde_expr::{Ask, LengthUnit, Params};

use crate::origin::LAST_ID;
use crate::{Annotation, Constraint, Curve, Id, Role, Sketch};

/// What a sketch is checked against, from the design it's part of: the
/// coordinate limit, the design's units, in which bare numbers in
/// dimensions' expressions are read, and its parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Design<'a> {
    /// How far from zero a coordinate may be, and the largest radius.
    pub max: f64,
    pub units: LengthUnit,
    /// The design's parameters, resolved: what names in dimensions'
    /// expressions (and features' values) are read with
    /// ([`Measure::ask`](crate::Measure::ask)).
    pub params: &'a Params,
}

impl<'a> Design<'a> {
    /// A length within `max` millimetres of zero ([`Ask::length`]), in
    /// the design's units and with its parameters.
    pub fn length(&self, max: f64) -> Ask<'a> {
        Ask::length(self.units, max).with_params(self.params)
    }

    /// An angle within `max` radians of zero ([`Ask::angle`]), with the
    /// design's parameters.
    pub fn angle(&self, max: f64) -> Ask<'a> {
        Ask::angle(self.units, max).with_params(self.params)
    }

    /// A plain number within `max` of zero ([`Ask::number`]), with the
    /// design's parameters.
    pub fn number(&self, max: f64) -> Ask<'a> {
        Ask::number(self.units, max).with_params(self.params)
    }

    /// A factor from `1 / max` to `max` ([`Ask::factor`]), with the
    /// design's parameters.
    pub fn factor(&self, max: f64) -> Ask<'a> {
        Ask::factor(self.units, max).with_params(self.params)
    }
}

/// One of a sketch's lists of items, as [`SketchError::TooMany`] names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum List {
    Points,
    Curves,
    Constraints,
    Dimensions,
    Links,
}

impl List {
    pub fn name(self) -> &'static str {
        match self {
            List::Points => "points",
            List::Curves => "curves",
            List::Constraints => "constraints",
            List::Dimensions => "dimensions",
            List::Links => "links",
        }
    }
}

/// The most curves a sketch may hold. A file could ask for millions, and
/// the view lays out a row for each and the solver will be handed them
/// all; a sketch people draw holds hundreds.
pub const MAX_CURVES: usize = 10_000;

/// The most points a sketch may hold: enough for [`MAX_CURVES`] arcs, each
/// with points of its own, or 150 splines of
/// [`MAX_SPLINE_POINTS`](crate::MAX_SPLINE_POINTS) fit points with a
/// handle at each, 300 without. Not raised for splines: a file's points
/// are what the solver's analysis is handed, whose time grows faster than
/// a component's points (a chain of splines of 4000 points takes over a
/// second).
pub const MAX_POINTS: usize = 3 * MAX_CURVES;

/// The most constraints a sketch may hold, a few per curve.
pub const MAX_CONSTRAINTS: usize = 4 * MAX_CURVES;

/// The most dimensions a sketch may hold, as many as constraints. Each is
/// re-evaluated on every check, and holds at most
/// [`MAX_LEN`](varde_expr::MAX_LEN) bytes of text.
pub const MAX_DIMENSIONS: usize = MAX_CONSTRAINTS;

impl Sketch {
    /// Checks what a file or an edit could get wrong, list by list: the
    /// counts and ids first, as each list is sorted before an id is looked
    /// up in it, then points, curves, links, constraints and dimensions.
    /// Coordinates and radii are bounded by `design.max`; a dimension's
    /// expression is read in `design.units`.
    pub fn check(&self, design: &Design) -> Result<(), SketchError> {
        self.check_counts()?;
        self.check_order()?;
        self.check_points(design.max)?;
        self.check_curves(design.max)?;
        self.check_links()?;
        let tips = self.tips();
        self.check_constraints(&tips)?;
        self.check_dimensions(design, &tips)
    }

    /// No more items than [`MAX_POINTS`], [`MAX_CURVES`],
    /// [`MAX_CONSTRAINTS`] and [`MAX_DIMENSIONS`], and no id given out
    /// reaching the origin's and axes'.
    fn check_counts(&self) -> Result<(), SketchError> {
        for (list, count, limit) in [
            (List::Points, self.points.len(), MAX_POINTS),
            (List::Curves, self.curves.len(), MAX_CURVES),
            (List::Constraints, self.constraints.len(), MAX_CONSTRAINTS),
            (List::Dimensions, self.dimensions.len(), MAX_DIMENSIONS),
        ] {
            if count > limit {
                return Err(SketchError::TooMany { list, count, limit });
            }
        }
        if self.next_id > LAST_ID {
            return Err(SketchError::NextIdReserved(self.next_id));
        }
        Ok(())
    }

    /// Each list in increasing id order with every id below `next_id`,
    /// and each id naming one item only.
    fn check_order(&self) -> Result<(), SketchError> {
        check_ids(self.points.iter().map(|point| point.id), self.next_id)?;
        check_ids(self.curves.iter().map(|curve| curve.id), self.next_id)?;
        check_ids(
            self.constraints.iter().map(|constraint| constraint.id),
            self.next_id,
        )?;
        check_ids(
            self.dimensions.iter().map(|dimension| dimension.id),
            self.next_id,
        )?;
        let curve_ids = self.curves.iter().map(|curve| curve.id);
        let constraint_ids = self.constraints.iter().map(|constraint| constraint.id);
        let dimension_ids = self.dimensions.iter().map(|dimension| dimension.id);
        let geometry = |id| self.point(id).is_some() || self.curve(id).is_some();
        let shared = curve_ids
            .filter(|&id| self.point(id).is_some())
            .chain(constraint_ids.filter(|&id| geometry(id)))
            .chain(dimension_ids.filter(|&id| geometry(id) || self.constraint(id).is_some()))
            .next();
        match shared {
            Some(id) => Err(SketchError::Shared(id)),
            None => Ok(()),
        }
    }

    /// Every point's coordinates within `max` of zero.
    fn check_points(&self, max: f64) -> Result<(), SketchError> {
        for point in &self.points {
            if let Some(value) = outside(point.at, max) {
                return Err(SketchError::Coordinate {
                    id: point.id,
                    value,
                    max,
                });
            }
        }
        Ok(())
    }

    /// Every radius above zero and at most `max`, every spline's counts,
    /// handles and knots as they can be, every curve made of points
    /// (never the origin), none twice, and every fillet and chamfer on a
    /// corner of two lines, a corner at most once.
    fn check_curves(&self, max: f64) -> Result<(), SketchError> {
        for entry in &self.curves {
            match &entry.curve {
                &Curve::Circle { radius, .. } if !(radius > 0.0 && radius <= max) => {
                    return Err(SketchError::Radius {
                        id: entry.id,
                        radius,
                        max,
                    });
                }
                // Its counts bounded before its points are looked at.
                Curve::Spline(spline) if !spline.fits() => {
                    return Err(SketchError::Spline(entry.id));
                }
                _ => {}
            }
            let points = entry.curve.points().map(|id| (id, Role::Point));
            self.check_references(entry.id, points)?;
            if entry.curve.points().any(Id::is_builtin) {
                return Err(SketchError::Builtin(entry.id));
            }
            if let Some(corner) = entry.corner {
                let lines = [(corner.a, Role::Line), (corner.b, Role::Line)];
                let at = (corner.at, Role::Point);
                self.check_references(entry.id, lines.into_iter().chain([at]))?;
                if !self.fits_corner(entry) {
                    return Err(SketchError::Corner(entry.id));
                }
            }
        }
        match self.repeated_corner() {
            Some(id) => Err(SketchError::Corner(id)),
            None => Ok(()),
        }
    }

    /// Every constraint naming items of the kinds it has to, none twice,
    /// and passing [`check_annotation`](Sketch::check_annotation): its
    /// items go together ([`Constraint::fits`](crate::Constraint::fits)),
    /// an offset's being offset pairs ([`Sketch::offset_pair`]), the
    /// origin and axes named only as they can be
    /// ([`Constraint::fits_builtins`](crate::Constraint::fits_builtins)),
    /// and no curve named with one of its own points, but for a round
    /// join's corner. `tips` are the sketch's handles' tips.
    fn check_constraints(&self, tips: &HashSet<Id>) -> Result<(), SketchError> {
        for entry in &self.constraints {
            let from = entry.id;
            match entry.constraint {
                // A line cut in two by offsetting it has two copies, each
                // tied by its pair, so the pairs may share their first.
                Constraint::EqualOffset { a, b } => {
                    let items = entry.constraint.items();
                    self.check_references(from, items.clone().take(2))?;
                    self.check_references(from, items.skip(2))?;
                    if a == b {
                        return Err(SketchError::Repeated { from, to: a[1] });
                    }
                }
                _ => self.check_references(from, entry.constraint.items())?,
            }
            self.check_annotation(from, Annotation::Constraint(&entry.constraint), tips)?;
        }
        Ok(())
    }

    /// Every dimension's measure checked as a constraint's items are
    /// ([`check_constraints`](Sketch::check_constraints)), its label within
    /// `design.max` of zero, and its expression, read in `design.units`,
    /// evaluating to its stored value exactly and being what its measure
    /// asks for ([`Measure::ask`](crate::Measure::ask)).
    fn check_dimensions(&self, design: &Design, tips: &HashSet<Id>) -> Result<(), SketchError> {
        for entry in &self.dimensions {
            let (from, dimension) = (entry.id, &entry.dimension);
            self.check_references(from, dimension.measure.items())?;
            self.check_annotation(from, Annotation::Dimension(&dimension.measure), tips)?;
            if outside(dimension.label, design.max).is_some() {
                return Err(SketchError::Label(from));
            }
            if dimension
                .value
                .check(&dimension.measure.ask(design))
                .is_err()
            {
                return Err(SketchError::Value(from));
            }
        }
        Ok(())
    }

    /// The rules constraints and dimensions share, once their references
    /// are checked, in one order: items of kinds that go together,
    /// built-ins used as they may be, and no curve tied to its own point.
    /// `tips` are the sketch's handles' tips.
    fn check_annotation(
        &self,
        from: Id,
        annotation: Annotation<'_>,
        tips: &HashSet<Id>,
    ) -> Result<(), SketchError> {
        if !annotation.fits(self, tips) {
            return Err(SketchError::Unfit(from));
        }
        if !annotation.fits_builtins(self) {
            return Err(SketchError::Builtin(from));
        }
        if let Some((curve, point)) = annotation.own_point(self) {
            return Err(SketchError::OwnPoint { from, curve, point });
        }
        Ok(())
    }

    /// That the items `from` names can play the roles they have to, and
    /// that none is named twice: a line from a point to itself, or a
    /// point coincident with itself, means nothing.
    fn check_references(
        &self,
        from: Id,
        references: impl Iterator<Item = (Id, Role)> + Clone,
    ) -> Result<(), SketchError> {
        for (index, (to, expected)) in references.clone().enumerate() {
            if !self.kind(to).is_some_and(|kind| expected.admits(kind)) {
                return Err(SketchError::Reference { from, to, expected });
            }
            if references.clone().take(index).any(|(other, _)| other == to) {
                return Err(SketchError::Repeated { from, to });
            }
        }
        Ok(())
    }
}

/// The first of `at`'s coordinates past `max` from zero, if any.
fn outside(at: DVec2, max: f64) -> Option<f64> {
    at.to_array()
        .into_iter()
        .find(|v| !(-max..=max).contains(v))
}

/// That `ids` increase and are below `next_id`, so new items get new
/// ids, which come last, where adding them puts them.
fn check_ids(mut ids: impl Iterator<Item = Id>, next_id: u32) -> Result<(), SketchError> {
    let Some(mut before) = ids.next() else {
        return Ok(());
    };
    for id in ids {
        if id <= before {
            return Err(SketchError::Order { id, before });
        }
        before = id;
    }
    // `before` is the highest.
    if before.0 >= next_id {
        return Err(SketchError::NextId(before));
    }
    Ok(())
}

/// Why a [`Sketch`] fails [`Sketch::check`], against the limit `max` it
/// was checked with where it applies.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, thiserror::Error)]
pub enum SketchError {
    /// More items in the list than `limit`.
    #[error("the sketch has {count} {}, over the limit of {limit}", list.name())]
    TooMany {
        list: List,
        count: usize,
        limit: usize,
    },
    /// The first id doesn't come after the second, the one before it in
    /// the same list.
    #[error("sketch item {id} doesn't come after {before}")]
    Order { id: Id, before: Id },
    /// The highest id of a list isn't below the sketch's next id.
    #[error("sketch item {0} is not below the sketch's next id")]
    NextId(Id),
    /// The id names items in two lists.
    #[error("sketch id {0} names two items")]
    Shared(Id),
    /// The sketch's next id is past the ids it can give out, which end
    /// well short of the origin's and axes'.
    #[error("the sketch's next id {0} runs into its origin's and axes'")]
    NextIdReserved(u32),
    /// The item names the origin or an axis as it can't be: a curve made
    /// from the origin, a constraint or dimension on them alone, or one
    /// taking an axis as a segment, see
    /// [`Constraint::fits_builtins`](crate::Constraint::fits_builtins).
    #[error("sketch item {0} uses the origin or an axis in a way it can't")]
    Builtin(Id),
    /// A point coordinate farther than `max` from zero, or not a number.
    #[error("sketch point {id} has a coordinate of {value}, outside the limit of {max}")]
    Coordinate { id: Id, value: f64, max: f64 },
    /// A circle's radius not above zero, or past `max`.
    #[error("sketch circle {id} has a radius of {radius}, not above zero and within {max}")]
    Radius { id: Id, radius: f64, max: f64 },
    /// `from` names `to`, which can't play the role `expected`.
    #[error("sketch item {from} refers to {to}, which is no {}", expected.name())]
    Reference { from: Id, to: Id, expected: Role },
    /// `from` names `to` more than once.
    #[error("sketch item {from} refers to {to} more than once")]
    Repeated { from: Id, to: Id },
    /// The constraint names items whose kinds don't go together, such as
    /// a tangent between two lines, or the constraint or dimension names
    /// what's no offset pair.
    #[error("sketch item {0} ties together items that don't go together")]
    Unfit(Id),
    /// The constraint or dimension `from` names `curve` and `point`, one
    /// of the points `curve` is made from.
    #[error("sketch item {from} refers to {curve} and to its own point {point}")]
    OwnPoint { from: Id, curve: Id, point: Id },
    /// The spline has too few or too many points
    /// ([`SplineKind::least`](crate::SplineKind::least),
    /// [`MAX_SPLINE_POINTS`](crate::MAX_SPLINE_POINTS)), a handle on none
    /// of its fit points, two on one, or by control points, or knots not
    /// as its kind and count take them.
    #[error("sketch spline {0} has points, handles or knots it can't have")]
    Spline(Id),
    /// The fillet or chamfer isn't on a corner (see
    /// [`Corner`](crate::Corner)): a circle, or an arc that's `equal`, or
    /// its lines chamfers, not ending at its point, or its own points
    /// theirs; or another is on the same corner of the same lines.
    #[error("sketch curve {0} is a fillet or chamfer on no corner of its own")]
    Corner(Id),
    /// The dimension's label is farther than the limit from its anchor, or
    /// not a number.
    #[error("sketch dimension {0} has its label out of bounds")]
    Label(Id),
    /// The link names what isn't a point or curve of its own: one that
    /// isn't there or is another link's, a curve made of points not its
    /// own, a fillet or chamfer, a spline with handles, a curve counting
    /// for profiles as the link doesn't say (or the other way), its ids
    /// out of order or too many; or a curve not the link's is made of its
    /// points.
    #[error("sketch link {0} names geometry that isn't its own, or shares it")]
    Link(Id),
    /// The dimension's expression doesn't evaluate to its stored value in
    /// the design's units, or not to what its measure asks for: not a
    /// length or an angle, not above zero, past the limit.
    #[error("sketch dimension {0} has a value its expression doesn't give, or out of bounds")]
    Value(Id),
}

#[cfg(test)]
mod tests;
