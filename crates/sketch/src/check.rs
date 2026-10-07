//! [`Sketch::check`]: what a file could get wrong about a sketch.

use std::fmt;

use serde::{Deserialize, Serialize};

use varde_expr::LengthUnit;

use crate::origin::LAST_ID;
use crate::{Constraint, Curve, Id, Role, Sketch};

/// What a sketch is checked against, from the design it's part of: the
/// coordinate limit, and the design's units, in which bare numbers in
/// dimensions' expressions are read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Design {
    /// How far from zero a coordinate may be, and the largest radius.
    pub max: f64,
    pub units: LengthUnit,
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
    /// Checks what a file or an edit could get wrong: no more items than
    /// [`MAX_POINTS`], [`MAX_CURVES`], [`MAX_CONSTRAINTS`] and
    /// [`MAX_DIMENSIONS`], each list in increasing id order with every id
    /// below `next_id` and naming one item only, every coordinate within
    /// `design.max` of zero, every radius above zero and at most that, and
    /// every reference naming an item of the kind it has to be, no item
    /// naming the same one twice, every spline's counts, handles and knots
    /// as they can be, and every fillet and chamfer on a corner
    /// of two lines, a corner at most once. A constraint's items also have
    /// to go together ([`Constraint::fits`](crate::Constraint::fits)), an
    /// offset's being offset pairs ([`Sketch::offset_pair`]), and a
    /// constraint or dimension naming a curve can't name one of that
    /// curve's own points too, but for a round join's corner. A dimension's expression, read in
    /// `design.units`, must evaluate to its stored value exactly and be
    /// what its measure asks for ([`Measure::ask`](crate::Measure::ask)),
    /// and its label must be within `design.max` of its anchor. The
    /// origin and axes are named only as they can be
    /// ([`Constraint::fits_builtins`](crate::Constraint::fits_builtins)),
    /// never as a curve's point, and no id given out reaches theirs.
    /// Links ([`Link`](crate::Link)) are in increasing id order, at most
    /// [`MAX_LINKS`](crate::MAX_LINKS), each id below `next_id` and
    /// naming no item, each listing points and curves of its own, in
    /// increasing order, its curves made of its points alone, no fillet,
    /// chamfer or spline with handles among them, construction unless
    /// the link counts for profiles; no other curve is made of a link's
    /// points.
    pub fn check(&self, design: &Design) -> Result<(), SketchError> {
        let max = design.max;
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
        // Each list is sorted before an id is looked up in it.
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
        if let Some(id) = shared {
            return Err(SketchError::Shared(id));
        }

        for point in &self.points {
            if let Some(value) = point
                .at
                .to_array()
                .into_iter()
                .find(|v| !(-max..=max).contains(v))
            {
                return Err(SketchError::Coordinate {
                    id: point.id,
                    value,
                    max,
                });
            }
        }
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
        if let Some(id) = self.repeated_corner() {
            return Err(SketchError::Corner(id));
        }
        self.check_links()?;
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
            if !entry.constraint.fits(self) {
                return Err(SketchError::Unfit(from));
            }
            if !entry.constraint.fits_builtins() {
                return Err(SketchError::Builtin(from));
            }
            if let Some((curve, point)) = entry.constraint.own_point(self) {
                return Err(SketchError::OwnPoint { from, curve, point });
            }
        }
        let tips = self.tips();
        for entry in &self.dimensions {
            let (from, dimension) = (entry.id, &entry.dimension);
            self.check_references(from, dimension.measure.items())?;
            if let Some((curve, point)) = dimension.measure.own_point(self) {
                return Err(SketchError::OwnPoint { from, curve, point });
            }
            if !dimension.measure.fits_builtins(self) {
                return Err(SketchError::Builtin(from));
            }
            if !dimension.measure.fits(self, &tips) {
                return Err(SketchError::Unfit(from));
            }
            let within = |v: &f64| (-max..=max).contains(v);
            if !dimension.label.to_array().iter().all(within) {
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SketchError {
    /// More items in the list than `limit`.
    TooMany {
        list: List,
        count: usize,
        limit: usize,
    },
    /// The first id doesn't come after the second, the one before it in
    /// the same list.
    Order { id: Id, before: Id },
    /// The highest id of a list isn't below the sketch's next id.
    NextId(Id),
    /// The id names items in two lists.
    Shared(Id),
    /// The sketch's next id is past the ids it can give out, which end
    /// well short of the origin's and axes'.
    NextIdReserved(u32),
    /// The item names the origin or an axis as it can't be: a curve made
    /// from the origin, a constraint or dimension on them alone, or one
    /// taking an axis as a segment, see
    /// [`Constraint::fits_builtins`](crate::Constraint::fits_builtins).
    Builtin(Id),
    /// A point coordinate farther than `max` from zero, or not a number.
    Coordinate { id: Id, value: f64, max: f64 },
    /// A circle's radius not above zero, or past `max`.
    Radius { id: Id, radius: f64, max: f64 },
    /// `from` names `to`, which can't play the role `expected`.
    Reference { from: Id, to: Id, expected: Role },
    /// `from` names `to` more than once.
    Repeated { from: Id, to: Id },
    /// The constraint names items whose kinds don't go together, such as
    /// a tangent between two lines, or the constraint or dimension names
    /// what's no offset pair.
    Unfit(Id),
    /// The constraint or dimension `from` names `curve` and `point`, one
    /// of the points `curve` is made from.
    OwnPoint { from: Id, curve: Id, point: Id },
    /// The spline has too few or too many points
    /// ([`SplineKind::least`](crate::SplineKind::least),
    /// [`MAX_SPLINE_POINTS`](crate::MAX_SPLINE_POINTS)), a handle on none
    /// of its fit points, two on one, or by control points, or knots not
    /// as its kind and count take them.
    Spline(Id),
    /// The fillet or chamfer isn't on a corner (see
    /// [`Corner`](crate::Corner)): a circle, or an arc that's `equal`, or
    /// its lines chamfers, not ending at its point, or its own points
    /// theirs; or another is on the same corner of the same lines.
    Corner(Id),
    /// The dimension's label is farther than the limit from its anchor, or
    /// not a number.
    Label(Id),
    /// The link names what isn't a point or curve of its own: one that
    /// isn't there or is another link's, a curve made of points not its
    /// own, a fillet or chamfer, a spline with handles, a curve counting
    /// for profiles as the link doesn't say (or the other way), its ids
    /// out of order or too many; or a curve not the link's is made of its
    /// points.
    Link(Id),
    /// The dimension's expression doesn't evaluate to its stored value in
    /// the design's units, or not to what its measure asks for: not a
    /// length or an angle, not above zero, past the limit.
    Value(Id),
}

impl fmt::Display for SketchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            SketchError::TooMany { list, count, limit } => write!(
                f,
                "the sketch has {count} {}, over the limit of {limit}",
                list.name()
            ),
            SketchError::Order { id, before } => {
                write!(f, "sketch item {id} doesn't come after {before}")
            }
            SketchError::NextId(id) => {
                write!(f, "sketch item {id} is not below the sketch's next id")
            }
            SketchError::Shared(id) => write!(f, "sketch id {id} names two items"),
            SketchError::NextIdReserved(next_id) => write!(
                f,
                "the sketch's next id {next_id} runs into its origin's and axes'"
            ),
            SketchError::Builtin(id) => write!(
                f,
                "sketch item {id} uses the origin or an axis in a way it can't"
            ),
            SketchError::Coordinate { id, value, max } => write!(
                f,
                "sketch point {id} has a coordinate of {value}, outside the limit of {max}"
            ),
            SketchError::Radius { id, radius, max } => write!(
                f,
                "sketch circle {id} has a radius of {radius}, not above zero and within {max}"
            ),
            SketchError::Reference { from, to, expected } => write!(
                f,
                "sketch item {from} refers to {to}, which is no {}",
                expected.name()
            ),
            SketchError::Repeated { from, to } => {
                write!(f, "sketch item {from} refers to {to} more than once")
            }
            SketchError::Unfit(id) => {
                write!(
                    f,
                    "sketch item {id} ties together items that don't go together"
                )
            }
            SketchError::OwnPoint { from, curve, point } => write!(
                f,
                "sketch item {from} refers to {curve} and to its own point {point}"
            ),
            SketchError::Spline(id) => write!(
                f,
                "sketch spline {id} has points, handles or knots it can't have"
            ),
            SketchError::Corner(id) => write!(
                f,
                "sketch curve {id} is a fillet or chamfer on no corner of its own"
            ),
            SketchError::Label(id) => {
                write!(f, "sketch dimension {id} has its label out of bounds")
            }
            SketchError::Link(id) => write!(
                f,
                "sketch link {id} names geometry that isn't its own, or shares it"
            ),
            SketchError::Value(id) => write!(
                f,
                "sketch dimension {id} has a value its expression doesn't give, or out of bounds"
            ),
        }
    }
}

impl std::error::Error for SketchError {}

#[cfg(test)]
mod tests;
