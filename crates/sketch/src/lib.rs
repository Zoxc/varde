//! 2D sketches: points, lines, circles, arcs and splines on a plane, plus the
//! geometric constraints between them (coincident, horizontal, ...), the
//! dimensions giving their sizes and angles ([`Dimension`]), the solver
//! that holds them ([`solve()`], [`analyse`]), and the edits
//! made to them ([`SketchEdit`]), solved into proposals ([`propose()`]).
//!
//! Every sketch has an origin and two axes, fixed, built in rather than
//! stored (see [`Id::ORIGIN`]).
//!
//! Splines ([`Spline`]) are cubic B-splines through fit points or by
//! control points, their math in [`BSpline`] and [`Interpolation`].
//!
//! Also curves flattened for drawing ([`Sketch::flatten`]), the geometry
//! the drawing tools need ([`arc_through`]), what the shape tools would
//! trim, extend, offset, fillet and chamfer ([`Sketch::trim_piece`],
//! [`Sketch::extension`], [`Sketch::offset_preview`],
//! [`Sketch::fillet_preview`], [`Sketch::chamfer_preview`]), and
//! the regions the curves enclose, ready to extrude
//! ([`Sketch::profiles`]), and links, geometry taken from outside the
//! sketch that follows it ([`Link`]). Everything is in
//! model units, millimetres. A [`Sketch`] read from a file is trusted only
//! after [`Sketch::check`].

pub mod angle;
mod check;
mod constraint;
mod corner;
mod detach;
mod dimension;
mod edit;
mod flatten;
mod geometry;
mod intersect;
mod joint;
mod link;
mod offset;
mod origin;
mod profile;
mod propose;
mod sets;
mod shape;
mod solve;
mod spline;

pub use check::{
    Design, List, MAX_CONSTRAINTS, MAX_CURVES, MAX_DIMENSIONS, MAX_POINTS, SketchError,
};
pub use constraint::{Constraint, ConstraintEntry, Role, Shape, Side, tangent_between};
pub use corner::{Corner, Setback};
pub use dimension::{Dimension, DimensionEntry, MIN_LENGTH, Measure};
pub use edit::{Add, EditError, NewCurve, SketchEdit};
pub use flatten::{CIRCLE_SEGMENTS, cut_line, flatten_arc, flatten_circle};
pub use geometry::{ArcPoints, arc_sweep, arc_through, crossing, foot};
pub use joint::Joint;
pub use link::{
    FitError, Link, LinkKind, LinkShape, MAX_LINK_CURVES, MAX_LINK_POINTS, MAX_LINKS, ProjectError,
    SampledChain,
};
pub use offset::{MAX_OFFSET_WORK, MITER_TURN, OffsetPair};
pub use profile::{
    MAX_NEAR_MISSES, MAX_NEAR_PAIRS, MAX_REGION_CURVES, MAX_SPLITS, MAX_WORK, MergeError, NearMiss,
    OpenEnd, Piece, Profiles, Region, RegionRef, RegionRefError, TooComplex,
};
pub use propose::{Accepted, DragSession, Rejected, propose};
pub use solve::{Analysis, Budget, DEFAULT_ITERATIONS, Failure, Goal, Solution, analyse, solve};
pub use spline::{
    BSpline, Handle, Interpolation, MAX_COMB_TEETH, MAX_SPLINE_POINTS, MIN_KNOT_GAP, MIN_SPAN,
    Spline, SplineKind, chord_params, control_knots, flatten_spline, handle_scale, handle_tips,
};

use std::collections::HashSet;
use std::fmt;

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// An item's handle in one sketch: a point, a curve, a constraint or a
/// dimension. They share one counter, [`Sketch::next_id`], so an id names
/// one thing, and never changes. It's opaque: ids come from a sketch's
/// items, not from literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Id(u32);

impl Id {
    /// Its number, unique in its sketch: for naming what's made from the
    /// item outside the sketch, such as the wall an extruded curve sweeps.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What an [`Id`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Point,
    Line,
    Circle,
    Arc,
    Spline,
    Constraint,
    Dimension,
}

impl Kind {
    /// The kind as the user sees it, e.g. in "Line 3".
    pub fn name(self) -> &'static str {
        match self {
            Kind::Point => "Point",
            Kind::Line => "Line",
            Kind::Circle => "Circle",
            Kind::Arc => "Arc",
            Kind::Spline => "Spline",
            Kind::Constraint => "Constraint",
            Kind::Dimension => "Dimension",
        }
    }
}

/// A point of a sketch: a curve's end or centre, or a lone one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub id: Id,
    /// Numbers the points for their names, see [`Point::name`].
    pub number: u32,
    pub at: DVec2,
}

impl Point {
    /// "Point 3", or "Origin".
    pub fn name(&self) -> String {
        if self.id == Id::ORIGIN {
            return "Origin".into();
        }
        format!("{} {}", Kind::Point.name(), self.number)
    }
}

/// A curve's shape, by the points it's made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Curve {
    Line {
        start: Id,
        end: Id,
    },
    Circle {
        center: Id,
        radius: f64,
    },
    /// Counter-clockwise from `start` to `end`. Its radius is
    /// `|start - center|`, which `|end - center|` is to equal: the solver
    /// holds that as an equation the arc implies.
    Arc {
        center: Id,
        start: Id,
        end: Id,
    },
    /// A smooth free-form curve through or by its points, see [`Spline`].
    Spline(Spline),
}

impl Curve {
    pub fn kind(&self) -> Kind {
        match self {
            Curve::Line { .. } => Kind::Line,
            Curve::Circle { .. } => Kind::Circle,
            Curve::Arc { .. } => Kind::Arc,
            Curve::Spline(_) => Kind::Spline,
        }
    }

    /// The points the curve is made from: a spline's fit or control
    /// points, then its handles' tips.
    pub fn points(&self) -> impl Iterator<Item = Id> + Clone + '_ {
        let (own, spline) = match self {
            &Curve::Line { start, end } => ([Some(start), Some(end), None], None),
            &Curve::Circle { center, .. } => ([Some(center), None, None], None),
            &Curve::Arc { center, start, end } => ([Some(center), Some(start), Some(end)], None),
            Curve::Spline(spline) => ([None; 3], Some(spline)),
        };
        let spline = spline.into_iter().flat_map(Spline::all_points);
        own.into_iter().flatten().chain(spline)
    }

    /// A line's, an arc's or an open spline's start and end. `None` for
    /// a circle or a closed spline.
    pub fn ends(&self) -> Option<[Id; 2]> {
        match self {
            &Curve::Line { start, end } | &Curve::Arc { start, end, .. } => Some([start, end]),
            Curve::Spline(spline) => spline.ends(),
            Curve::Circle { .. } => None,
        }
    }

    /// A circle's or an arc's centre. `None` for a line or a spline.
    pub fn center(&self) -> Option<Id> {
        match *self {
            Curve::Circle { center, .. } | Curve::Arc { center, .. } => Some(center),
            Curve::Line { .. } | Curve::Spline(_) => None,
        }
    }

    /// The same curve made from the points `map` gives for its own.
    pub(crate) fn map_points<E>(
        &self,
        mut map: impl FnMut(Id) -> Result<Id, E>,
    ) -> Result<Curve, E> {
        Ok(match self {
            &Curve::Line { start, end } => Curve::Line {
                start: map(start)?,
                end: map(end)?,
            },
            &Curve::Circle { center, radius } => Curve::Circle {
                center: map(center)?,
                radius,
            },
            &Curve::Arc { center, start, end } => Curve::Arc {
                center: map(center)?,
                start: map(start)?,
                end: map(end)?,
            },
            Curve::Spline(spline) => {
                let points = spline.points.iter().map(|&id| map(id));
                let points = points.collect::<Result<_, E>>()?;
                let mut handles = Vec::with_capacity(spline.handles.len());
                for handle in &spline.handles {
                    handles.push(Handle {
                        at: map(handle.at)?,
                        tip: map(handle.tip)?,
                    });
                }
                Curve::Spline(Spline {
                    points,
                    handles,
                    ..spline.clone()
                })
            }
        })
    }
}

/// A curve of a sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveEntry {
    pub id: Id,
    /// Numbers the curves of its kind for their names, see
    /// [`CurveEntry::name`].
    pub number: u32,
    /// Construction geometry helps place other geometry but is never part
    /// of a profile.
    pub construction: bool,
    pub curve: Curve,
    /// The corner it rounds or cuts, if it's a fillet (an arc) or a
    /// chamfer (a line), see [`Corner`].
    pub corner: Option<Corner>,
}

impl CurveEntry {
    /// What it's called: its kind's name, or a fillet's or chamfer's.
    pub fn noun(&self) -> &'static str {
        match self.corner {
            Some(_) => Corner::noun(&self.curve),
            None => self.curve.kind().name(),
        }
    }

    /// "Line 3", "Arc 1", "Fillet 2".
    pub fn name(&self) -> String {
        format!("{} {}", self.noun(), self.number)
    }
}

/// A sketch's items, each list sorted by id, which is what finding one
/// by binary search relies on, and every id below `next_id`. Those, and
/// the rest of what a file could get wrong, are held by [`Sketch::check`],
/// the one place a sketch from outside is trusted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub points: Vec<Point>,
    pub curves: Vec<CurveEntry>,
    pub constraints: Vec<ConstraintEntry>,
    pub dimensions: Vec<DimensionEntry>,
    /// The id the next item gets.
    pub next_id: u32,
    /// Geometry it takes from outside it, by id, see [`Link`]: their
    /// points and curves are among the lists above. Defaulted, so a
    /// sketch from before links reads with none.
    #[serde(default)]
    pub links: Vec<Link>,
}

/// Every id of a sketch has been used, so nothing more can be added to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfIds;

impl fmt::Display for OutOfIds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the sketch has no ids left")
    }
}

impl std::error::Error for OutOfIds {}

impl Sketch {
    /// The point `id`, the origin included.
    pub fn point(&self, id: Id) -> Option<&Point> {
        if id == Id::ORIGIN {
            return Some(&origin::ORIGIN);
        }
        self.point_index(id).map(|index| &self.points[index])
    }

    /// Where the points `ids` are, in order. `None` if one is missing.
    pub(crate) fn places(&self, ids: &[Id]) -> Option<Vec<DVec2>> {
        ids.iter()
            .map(|&id| self.point(id).map(|point| point.at))
            .collect()
    }

    pub fn point_mut(&mut self, id: Id) -> Option<&mut Point> {
        self.point_index(id).map(|index| &mut self.points[index])
    }

    pub fn curve(&self, id: Id) -> Option<&CurveEntry> {
        self.curve_index(id).map(|index| &self.curves[index])
    }

    pub fn curve_mut(&mut self, id: Id) -> Option<&mut CurveEntry> {
        self.curve_index(id).map(|index| &mut self.curves[index])
    }

    pub fn constraint(&self, id: Id) -> Option<&ConstraintEntry> {
        self.constraints
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()
            .map(|index| &self.constraints[index])
    }

    pub fn dimension(&self, id: Id) -> Option<&DimensionEntry> {
        self.dimension_index(id)
            .map(|index| &self.dimensions[index])
    }

    pub fn dimension_mut(&mut self, id: Id) -> Option<&mut DimensionEntry> {
        self.dimension_index(id)
            .map(|index| &mut self.dimensions[index])
    }

    fn dimension_index(&self, id: Id) -> Option<usize> {
        self.dimensions
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()
    }

    pub(crate) fn point_index(&self, id: Id) -> Option<usize> {
        self.points.binary_search_by_key(&id, |point| point.id).ok()
    }

    pub(crate) fn curve_index(&self, id: Id) -> Option<usize> {
        self.curves.binary_search_by_key(&id, |curve| curve.id).ok()
    }

    /// What `id` names, if anything: the origin is a point and the axes
    /// lines.
    pub fn kind(&self, id: Id) -> Option<Kind> {
        if id == Id::ORIGIN || self.point_index(id).is_some() {
            Some(Kind::Point)
        } else if id.is_builtin() {
            Some(Kind::Line)
        } else if let Some(curve) = self.curve(id) {
            Some(curve.curve.kind())
        } else if self.constraint(id).is_some() {
            Some(Kind::Constraint)
        } else {
            self.dimension(id).map(|_| Kind::Dimension)
        }
    }

    /// A new id. Fails, leaving the sketch as it was, once the ids have
    /// run out, into those reserved for handles and the origin and axes
    /// ([`Id::handle`]): `next_id` may come from
    /// a file, so it can be anything.
    pub(crate) fn new_id(&mut self) -> Result<Id, OutOfIds> {
        if self.next_id >= origin::LAST_ID {
            return Err(OutOfIds);
        }
        let id = Id(self.next_id);
        self.next_id += 1;
        Ok(id)
    }

    /// One past the highest number of the items called `noun` (a
    /// [`Kind::name`], or a [`CurveEntry::noun`]), so names stay unique
    /// across undo and sessions. A file could hold the highest number
    /// there is, which is then reused: a repeated name is harmless.
    fn next_number(&self, noun: &str) -> u32 {
        // The noun compared once rather than for every point: a test
        // building a sketch of thousands of points calls this for each.
        let points: &[Point] = match noun == Kind::Point.name() {
            true => &self.points,
            false => &[],
        };
        let points = points.iter().map(|point| point.number);
        let curves = self
            .curves
            .iter()
            .filter(|curve| curve.noun() == noun)
            .map(|curve| curve.number);
        points
            .chain(curves)
            .max()
            .map_or(1, |highest| highest.saturating_add(1))
    }

    /// Adds a point at `at`. New ids are the highest, so it goes last and
    /// the points stay sorted. The rest of [`Sketch::check`], such as `at`
    /// being within bounds, is up to the caller, as the document's editor
    /// does.
    pub fn add_point(&mut self, at: DVec2) -> Result<Id, OutOfIds> {
        let id = self.new_id()?;
        let number = self.next_number(Kind::Point.name());
        self.points.push(Point { id, number, at });
        Ok(id)
    }

    /// Adds `curve`, numbered after the others of its kind, like
    /// [`add_point`](Sketch::add_point). Whether its points exist is up
    /// to [`Sketch::check`].
    pub fn add_curve(&mut self, curve: Curve, construction: bool) -> Result<Id, OutOfIds> {
        let id = self.new_id()?;
        let number = self.next_number(curve.kind().name());
        self.curves.push(CurveEntry {
            id,
            number,
            construction,
            curve,
            corner: None,
        });
        Ok(id)
    }

    /// Adds `constraint`, like [`add_curve`](Sketch::add_curve).
    pub fn add_constraint(&mut self, constraint: Constraint) -> Result<Id, OutOfIds> {
        let id = self.new_id()?;
        self.constraints.push(ConstraintEntry { id, constraint });
        Ok(id)
    }

    /// Adds `dimension`, like [`add_curve`](Sketch::add_curve).
    pub fn add_dimension(&mut self, dimension: Dimension) -> Result<Id, OutOfIds> {
        let id = self.new_id()?;
        self.dimensions.push(DimensionEntry { id, dimension });
        Ok(id)
    }

    /// Deletes the items `ids` names and what depends on them: a link's
    /// points and curves with the link (and a point or curve of a link's
    /// leaves it, as relinking does), curves made
    /// from a deleted point, the fillets and chamfers on a deleted line,
    /// points no curve is made from any more that a deleted curve was (a
    /// lone point stays), and constraints and dimensions on anything
    /// deleted. A spline losing points keeps the rest where it still has
    /// enough ([`SplineKind::least`]), and loses its handles on them (or
    /// their tips, the handle going with the tip), else it goes too.
    /// Ids naming nothing, and the origin and axes, which are
    /// always there, are ignored. Keeps the lists sorted, and a sketch that
    /// passed [`Sketch::check`] passing it.
    pub fn delete(&mut self, ids: &[Id]) {
        let own = ids.iter().copied().filter(|id| !id.is_builtin());
        let mut deleted: HashSet<Id> = own.collect();
        // A link goes with what it made.
        self.links.retain(|link| {
            let gone = deleted.contains(&link.id);
            if gone {
                deleted.extend(link.items());
            }
            !gone
        });
        let at = |id| self.point(id).map(|point| point.at);
        let mut kept_splines = Vec::new();
        for (index, entry) in self.curves.iter().enumerate() {
            if let Curve::Spline(spline) = &entry.curve
                && !deleted.contains(&entry.id)
                && entry.curve.points().any(|id| deleted.contains(&id))
                && let Some(kept) = spline.without(&deleted, at)
            {
                kept_splines.push((index, kept));
            }
        }
        // The tips of handles gone, unless something else is made from
        // them.
        let mut dropped = Vec::new();
        for (index, (spline, tips)) in kept_splines {
            self.curves[index].curve = Curve::Spline(spline);
            dropped.extend(tips);
        }
        let curves = std::mem::take(&mut self.curves);
        let (mut gone, kept): (Vec<_>, Vec<_>) = curves.into_iter().partition(|entry| {
            deleted.contains(&entry.id) || entry.curve.points().any(|id| deleted.contains(&id))
        });
        // A corner's lines are never corners themselves, so one more round
        // takes all those on the lines gone.
        let lines: HashSet<Id> = gone.iter().map(|entry| entry.id).collect();
        let (corners, kept): (Vec<_>, Vec<_>) = kept.into_iter().partition(|entry| {
            entry
                .corner
                .is_some_and(|corner| lines.contains(&corner.a) || lines.contains(&corner.b))
        });
        gone.extend(corners);
        self.curves = kept;
        let used: HashSet<Id> = self
            .curves
            .iter()
            .flat_map(|entry| entry.curve.points())
            .collect();
        deleted.extend(gone.iter().map(|entry| entry.id));
        deleted.extend(
            gone.iter()
                .flat_map(|entry| entry.curve.points())
                .chain(dropped)
                .filter(|id| !used.contains(id)),
        );
        self.points.retain(|point| !deleted.contains(&point.id));
        for link in &mut self.links {
            link.points.retain(|id| !deleted.contains(id));
            link.curves.retain(|id| !deleted.contains(id));
        }
        /// Whether `id`, on `items`, is neither deleted nor on anything
        /// deleted.
        fn stays(
            deleted: &HashSet<Id>,
            id: Id,
            mut items: impl Iterator<Item = (Id, Role)>,
        ) -> bool {
            !deleted.contains(&id) && !items.any(|(item, _)| deleted.contains(&item))
        }
        self.constraints
            .retain(|entry| stays(&deleted, entry.id, entry.constraint.items()));
        self.dimensions
            .retain(|entry| stays(&deleted, entry.id, entry.dimension.measure.items()));
        // A spline that kept its points may have lost a handle, which a
        // smooth join or an angle needed.
        let tips = self.tips();
        let unfit: HashSet<Id> = self
            .constraints
            .iter()
            .filter(|entry| !entry.constraint.fits(self))
            .map(|entry| entry.id)
            .chain(
                self.dimensions
                    .iter()
                    .filter(|entry| !entry.dimension.measure.fits(self, &tips))
                    .map(|entry| entry.id),
            )
            .collect();
        if !unfit.is_empty() {
            self.constraints.retain(|entry| !unfit.contains(&entry.id));
            self.dimensions.retain(|entry| !unfit.contains(&entry.id));
        }
    }
}

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
