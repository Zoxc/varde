//! Edits: what the user does to a sketch, as an intent ([`SketchEdit`]),
//! and applying one ([`SketchEdit::apply`]) to make the sketch it asks
//! for, before solving it ([`propose`](crate::propose())).

use std::collections::BTreeMap;
use std::fmt;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use varde_expr::{AngleUnit, LengthUnit, Unit, Value, format};

use crate::origin::LAST_ID;
use crate::{
    Constraint, Curve, CurveEntry, Design, Dimension, Id, Kind, LinkKind, LinkShape, OutOfIds,
    Point, Setback, Side, Sketch, SketchError, SplineKind,
};

/// A change to a sketch as the user asks for it, not its result: applied
/// to whichever sketch it's proposed on, which may have moved on since it
/// was made (an earlier edit accepted meanwhile).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SketchEdit {
    /// New points, curves, constraints and dimensions.
    Add(Add),
    /// Deletes the items and what depends on them, see [`Sketch::delete`].
    /// Ids naming nothing are ignored, as an item an earlier edit deleted
    /// is gone already.
    Delete(Vec<Id>),
    /// Points put at new places and circles given new radii: the end of a
    /// drag. Proposed, what's fixed stays where it is and the rest follows
    /// the points moved, see [`propose`](crate::propose()).
    Move {
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
    },
    /// Curves made construction geometry, or normal.
    SetConstruction { ids: Vec<Id>, construction: bool },
    /// A dimension given a new value. Proposed, a driving one's value
    /// changes in steps, each solved from the last, so the geometry stays
    /// on the branch it's on, see [`propose`](crate::propose()).
    SetDimension { id: Id, value: Value },
    /// A dimension made driving, holding the value it measures now,
    /// shown in the design's units ([`format()`]), or made a reference,
    /// keeping the value it had.
    SetDriving { id: Id, driving: bool },
    /// A dimension's label moved, see [`Dimension::label`].
    MoveLabel { id: Id, label: DVec2 },
    /// The piece of the line, circle, arc or spline `curve` between the
    /// curves cutting it either side of the place on it nearest `near`
    /// taken away, or all of it if none do there (a circle or a closed
    /// spline needs two). What's left keeps the curve's id (a circle's
    /// becomes an arc, a closed spline an open one), and a line, an arc or
    /// a spline cut in two gets a new curve for the rest, held on one line
    /// (its points on the first) or one circle (equal radii); a spline's
    /// parts keep its shape, by control points exactly, through fit points
    /// with a handle at each new end. New ends are the end of a curve
    /// cutting there if one is (a new point coincident with it if it's a
    /// link's), or else new points on the curves cutting there. Those ties are `auto`, kept only where they hold with the
    /// rest (see [`Add::auto`]). Constraints and dimensions stay where
    /// they still mean something: those on an end taken away go with it,
    /// and a line's length, equal length, midpoint and distance from its
    /// midpoint go, what's on the endless line through it stays; a point
    /// on a spline stays on the part it's on. See `agents/sketch.md`.
    Trim { curve: Id, near: DVec2 },
    /// The line, arc or open spline `curve` lengthened past its point
    /// `end` to the first curve it meets, which its new end is on
    /// (`auto`), or at the end of if that's where: refused if it meets
    /// none ([`EditError::NothingAhead`]). A spline looks along its
    /// tangent at the end and runs on through `end` to a fit or control
    /// point more there, what held it tangent or smooth at `end` going.
    /// Constraints on the end, and on the line's length, go as for
    /// [`SketchEdit::Trim`].
    Extend { curve: Id, end: Id },
    /// Mirror images of the points and curves among `ids` in the line
    /// `about`, each new point [`Constraint::Symmetric`] with its
    /// original and each circle's copy [`Constraint::Equal`] to it, arcs
    /// running the other way so they stay counter-clockwise. Points on
    /// the line are shared with the copies (but an arc's or a link's) and
    /// held on it (`auto`); curves there are their own images. Constraints among
    /// what's mirrored aren't copied: the symmetry holds the copies
    /// already, so they'd be redundant. Constraints and dimensions among
    /// `ids`, and `about`, are left out; nothing else to mirror is
    /// [`EditError::NothingToMirror`].
    Mirror { ids: Vec<Id>, about: Id },
    /// A copy of the chain of lines and arcs `chain` (joined end to end,
    /// in any order), or of a circle, `distance` to its `side`: the left
    /// of the way the chain runs for [`Side::Positive`], run so that its
    /// first curve goes from its start to its end (an arc, or a circle,
    /// counter-clockwise). Each piece is copied exactly, the copies joined
    /// where they meet (lines run on to meet where the corner opens, arcs
    /// round it), and what falls within `distance` of the chain goes.
    /// Held by one driving [`Measure::Offset`](crate::Measure::Offset) of
    /// `distance` and each copy's
    /// [`Constraint::EqualOffset`] to it, a line's copy parallel to it, an
    /// arc's about its centre, copies meeting tangent tangent. A spline
    /// alone is copied as a spline through fit points along its exact
    /// offset, each held as far from it by an equal offset. Refused if
    /// the curves aren't a chain ([`EditError::NotAChain`]), nothing's
    /// left ([`EditError::NothingLeft`]), or a spline's copy would fold
    /// ([`EditError::TooTight`]). See `agents/sketch.md`.
    Offset {
        chain: Vec<Id>,
        distance: Value,
        side: Side,
    },
    /// The corner at the point `at` where the lines `lines` end rounded by
    /// a fillet of `radius`: an arc tangent to both, as a
    /// [`Corner`](crate::Corner) of theirs, held by a driving radius
    /// dimension. The lines stay whole, cut back to it
    /// ([`Sketch::cut_back`]). One already on that corner is replaced.
    /// Refused if the lines make no corner there
    /// ([`EditError::NoCorner`]) or it doesn't fit along them
    /// ([`EditError::NoRoom`]).
    Fillet {
        at: Id,
        lines: [Id; 2],
        radius: Value,
    },
    /// The corner at `at` of `lines` cut by a chamfer, a line across it as
    /// far back as `setback` says, from the first line to the second, held
    /// by driving dimensions of those distances from the corner (the
    /// first, equal ones held equal by the corner), or the angle to the
    /// first line. Otherwise as [`SketchEdit::Fillet`].
    Chamfer {
        at: Id,
        lines: [Id; 2],
        setback: Setback,
    },
    /// The spline `spline` switched to `to`, keeping its shape as closely
    /// as it can, see [`Sketch::convert_spline`]. Nothing changes if it's
    /// that kind already.
    Convert { spline: Id, to: SplineKind },
    /// Handles given to fit points of splines through fit points, each on
    /// every such spline it's a fit point of without one, its tip where
    /// the spline keeps the tangent it has there
    /// ([`Sketch::handle_tip`]). A handle goes by deleting its tip.
    /// [`EditError::Target`] for a point that's no fit point without a
    /// handle.
    AddHandles(Vec<Id>),
    /// A point more on the spline `spline`, where it passes nearest
    /// `near`, the curve staying smooth: through fit points a fit point
    /// there, the spline then passing through it and the rest as before
    /// (straying a little between); by control points a knot there
    /// ([`BSpline::with_knot`](crate::BSpline::with_knot)), a control
    /// point more and the shape just as it was, the control points either
    /// side moving to make room.
    /// [`EditError::Target`] where it has a point there already.
    InsertPoint { spline: Id, near: DVec2 },
    /// A new link of `kind`, making nothing yet: its geometry comes once
    /// what it comes from is found ([`SketchEdit::Relink`]). Its id is
    /// the sketch's `next_id`.
    AddLink { kind: LinkKind },
    /// Links given the geometry found for them, each as
    /// `Sketch::relink` does: moved in place, keeping the ids of what
    /// goes on (every id with the same form), deleting what doesn't and
    /// adding what's new. Proposed, the rest of the
    /// sketch follows, the links' geometry fixed.
    Relink(Vec<(Id, LinkShape)>),
    /// A link's curves made to count for profiles, or not (construction
    /// geometry).
    SetLinkProfiles { link: Id, profiles: bool },
    /// The point curves share made one of each, see [`Sketch::detach`].
    Detach(Id),
    /// The arc's end made its start, so it runs all the way round, see
    /// [`Sketch::closable`].
    CloseArc(Id),
    /// The spline made to run on from its last point round to its first,
    /// see [`Sketch::spline_closable`].
    CloseSpline(Id),
    /// The closed spline made to start at its point `at` and end at a new
    /// point there, see [`Sketch::spline_openable`].
    OpenSpline { spline: Id, at: Id },
}

impl SketchEdit {
    /// Adds `constraints` alone.
    pub fn constrain(sketch: &Sketch, constraints: Vec<Constraint>) -> SketchEdit {
        SketchEdit::Add(Add {
            constraints,
            ..Add::new(sketch)
        })
    }

    /// The sketch the edit makes of `sketch`, which is to have passed
    /// [`Sketch::check`], before solving: new items get new ids, deleted
    /// ones take what depends on them along. The result passes
    /// [`Sketch::check`] against `design`, or the edit fails, leaving
    /// `sketch` as it was.
    pub fn apply(&self, sketch: &Sketch, design: &Design) -> Result<Sketch, EditError> {
        self.apply_marked(sketch, design).map(|(sketch, _)| sketch)
    }

    /// As [`apply`](SketchEdit::apply), with the ids the `auto`
    /// constraints of an [`Add`], or of a shape tool's, got, in
    /// increasing order.
    pub(crate) fn apply_marked(
        &self,
        sketch: &Sketch,
        design: &Design,
    ) -> Result<(Sketch, Vec<Id>), EditError> {
        let mut next = sketch.clone();
        let mut auto = Vec::new();
        match self {
            SketchEdit::Add(add) => {
                auto = add.apply(&mut next)?;
                let new = next
                    .dimensions
                    .iter()
                    .filter(|entry| entry.id.0 >= sketch.next_id);
                if new
                    .filter(|entry| entry.dimension.driving)
                    .any(|entry| next.same_place(&entry.dimension.measure))
                {
                    return Err(EditError::SamePlace);
                }
            }
            SketchEdit::Delete(ids) => next.delete(ids),
            SketchEdit::Move { points, radii } => {
                for &(id, at) in points {
                    next.point_mut(id).ok_or(EditError::Target(id))?.at = at;
                }
                for &(id, new) in radii {
                    match next.curve_mut(id).map(|entry| &mut entry.curve) {
                        Some(Curve::Circle { radius, .. }) => *radius = new,
                        _ => return Err(EditError::Target(id)),
                    }
                }
            }
            SketchEdit::SetConstruction { ids, construction } => {
                for &id in ids {
                    next.curve_mut(id)
                        .ok_or(EditError::Target(id))?
                        .construction = *construction;
                }
            }
            SketchEdit::SetDimension { id, value } => {
                dimension(&mut next, *id)?.value = value.clone();
            }
            SketchEdit::SetDriving { id, driving } => {
                let entry = next.dimension(*id).ok_or(EditError::Target(*id))?;
                let old = &entry.dimension;
                let held = (*driving && !old.driving)
                    .then(|| next.held(&old.measure, old.side, design))
                    .transpose()?;
                let dimension = dimension(&mut next, *id)?;
                if let Some((value, side)) = held {
                    (dimension.value, dimension.side) = (value, side);
                }
                dimension.driving = *driving;
            }
            SketchEdit::MoveLabel { id, label } => dimension(&mut next, *id)?.label = *label,
            SketchEdit::Trim { curve, near } => auto = next.trim(*curve, *near)?,
            // Far enough to cross the whole of the sketch's room.
            SketchEdit::Extend { curve, end } => {
                auto = next.extend(*curve, *end, 4.0 * design.max)?
            }
            SketchEdit::Mirror { ids, about } => auto = next.mirror(ids, *about)?,
            SketchEdit::Offset {
                chain,
                distance,
                side,
            } => next.offset(chain, distance, *side, design)?,
            SketchEdit::Fillet { at, lines, radius } => next.fillet(*at, *lines, radius)?,
            SketchEdit::Chamfer { at, lines, setback } => next.chamfer(*at, *lines, setback)?,
            SketchEdit::Convert { spline, to } => next.convert_spline(*spline, *to)?,
            SketchEdit::AddHandles(points) => next.add_handles(points)?,
            SketchEdit::InsertPoint { spline, near } => next.insert_spline_point(*spline, *near)?,
            SketchEdit::AddLink { kind } => {
                next.add_link(*kind)?;
            }
            SketchEdit::Relink(found) => {
                for (link, shape) in found {
                    next.relink(*link, shape)?;
                }
            }
            SketchEdit::SetLinkProfiles { link, profiles } => {
                next.set_link_profiles(*link, *profiles)?;
            }
            SketchEdit::Detach(point) => next.detach(*point)?,
            SketchEdit::CloseArc(arc) => next.close_arc(*arc)?,
            SketchEdit::CloseSpline(spline) => next.close_spline(*spline)?,
            SketchEdit::OpenSpline { spline, at } => next.open_spline(*spline, *at)?,
        }
        // Only a link's own edits change what it made; deleting a link
        // takes it whole.
        match self {
            SketchEdit::AddLink { .. }
            | SketchEdit::Relink(_)
            | SketchEdit::SetLinkProfiles { .. } => {}
            SketchEdit::Delete(ids) => next.links_kept(sketch, ids)?,
            _ => next.links_kept(sketch, &[])?,
        }
        next.check(design)?;
        Ok((next, auto))
    }
}

/// The dimension `id` names, to change.
fn dimension(sketch: &mut Sketch, id: Id) -> Result<&mut Dimension, EditError> {
    sketch
        .dimension_mut(id)
        .map(|entry| &mut entry.dimension)
        .ok_or(EditError::Target(id))
}

impl SketchEdit {
    /// The driving dimensions the edit adds, or makes driving, in the
    /// sketch `applied` it made of `sketch`: those a refusal of it names
    /// as [`Rejected::Driving`](crate::Rejected::Driving).
    pub(crate) fn driving(&self, sketch: &Sketch, applied: &Sketch) -> Vec<Id> {
        match self {
            SketchEdit::Add(_) => applied
                .dimensions
                .iter()
                .filter(|entry| entry.id.0 >= sketch.next_id && entry.dimension.driving)
                .map(|entry| entry.id)
                .collect(),
            SketchEdit::SetDriving { id, driving: true } => sketch
                .dimension(*id)
                .filter(|entry| !entry.dimension.driving)
                .map(|entry| entry.id)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// What [`SketchEdit::Add`] adds. New items name each other by
/// placeholder ids before they have ids of their own: the ids from
/// `first` up, which [`Add::point`] and [`Add::curve`] hand out, while ids
/// below `first` name the sketch's items. Applied to a sketch, the
/// placeholder `first + n` becomes the id `next_id + n`, see
/// [`Add::resolve`], so an edit made on one sketch applies to one an
/// earlier edit has added to since.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Add {
    /// The first placeholder: the `next_id` of the sketch the edit was
    /// made on.
    pub first: u32,
    /// New points by placeholder, in increasing order.
    pub points: Vec<(Id, DVec2)>,
    /// New curves, in increasing order of placeholder, none shared with a
    /// point.
    pub curves: Vec<NewCurve>,
    pub constraints: Vec<Constraint>,
    /// New dimensions, naming items as the constraints do.
    pub dimensions: Vec<Dimension>,
    /// Constraints that snapping inferred: kept if they're independent of
    /// the rest, dropped silently if not, since a snap that restates what's
    /// already true shouldn't fail the edit. Applying keeps them all; it's
    /// [`propose`](crate::propose()) that tells them apart.
    pub auto: Vec<Constraint>,
}

/// A curve an [`Add`] adds, known by a placeholder `id` and made from
/// points that are the sketch's or placeholders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewCurve {
    pub id: Id,
    pub curve: Curve,
    pub construction: bool,
}

impl Add {
    /// Adding nothing yet to `sketch`.
    pub fn new(sketch: &Sketch) -> Add {
        Add {
            first: sketch.next_id,
            ..Add::default()
        }
    }

    /// The next placeholder: one past the points and curves so far.
    fn placeholder(&self) -> Result<Id, OutOfIds> {
        let made = self.points.len() + self.curves.len();
        u32::try_from(made)
            .ok()
            .and_then(|made| self.first.checked_add(made))
            .filter(|&id| id < LAST_ID)
            .map(Id)
            .ok_or(OutOfIds)
    }

    /// Adds a point at `at`, giving its placeholder.
    pub fn point(&mut self, at: DVec2) -> Result<Id, OutOfIds> {
        let id = self.placeholder()?;
        self.points.push((id, at));
        Ok(id)
    }

    /// Adds `curve`, giving its placeholder.
    pub fn curve(&mut self, curve: Curve, construction: bool) -> Result<Id, OutOfIds> {
        let id = self.placeholder()?;
        self.curves.push(NewCurve {
            id,
            curve,
            construction,
        });
        Ok(id)
    }

    /// The id that `id`, an id of the edit's, names once the edit is
    /// applied to a sketch whose `next_id` is `next_id`: a placeholder's
    /// new id, or else the sketch's own, the origin and axes included.
    /// `None` past the last id.
    pub fn resolve(&self, next_id: u32, id: Id) -> Option<Id> {
        if id.is_builtin() {
            return Some(id);
        }
        match id.0.checked_sub(self.first) {
            None => Some(id),
            Some(offset) => next_id.checked_add(offset).map(Id),
        }
    }

    /// Adds the items to `sketch`, giving the ids the `auto` constraints
    /// got. `sketch` may be left part way on failure.
    fn apply(&self, sketch: &mut Sketch) -> Result<Vec<Id>, EditError> {
        let placeholders = self
            .points
            .iter()
            .map(|&(id, _)| id)
            .chain(self.curves.iter().map(|curve| curve.id));
        let mut placed = BTreeMap::new();
        for id in placeholders {
            if id.0 < self.first || id.0 >= LAST_ID {
                return Err(EditError::Placeholder(id));
            }
            let new = self.resolve(sketch.next_id, id);
            let new = new.filter(|new| new.0 < LAST_ID).ok_or(OutOfIds)?;
            if placed.insert(id, new).is_some() {
                return Err(EditError::Placeholder(id));
            }
        }
        // In increasing order, the new ids go last, keeping the lists
        // sorted.
        let increasing = |ids: Vec<Id>| match ids.windows(2).find(|pair| pair[0] >= pair[1]) {
            Some(pair) => Err(EditError::Placeholder(pair[1])),
            None => Ok(()),
        };
        increasing(self.points.iter().map(|&(id, _)| id).collect())?;
        increasing(self.curves.iter().map(|curve| curve.id).collect())?;
        let first = self.first;
        // The sketch's own ids are below its `next_id`: one at or past it
        // (and below `first`) named an item of an edit before this one
        // that wasn't applied, and mustn't come to name a new item here.
        let own = sketch.next_id;
        let resolve = |id: Id| {
            if id.is_builtin() {
                Ok(id)
            } else if id.0 < first {
                if id.0 >= own {
                    return Err(EditError::Target(id));
                }
                Ok(id)
            } else {
                placed.get(&id).copied().ok_or(EditError::Placeholder(id))
            }
        };

        for &(id, at) in &self.points {
            let number = sketch.next_number(Kind::Point.name());
            sketch.points.push(Point {
                id: resolve(id)?,
                number,
                at,
            });
        }
        for new in &self.curves {
            let curve = new.curve.map_points(resolve)?;
            let number = sketch.next_number(curve.kind().name());
            sketch.curves.push(CurveEntry {
                id: resolve(new.id)?,
                number,
                construction: new.construction,
                curve,
                corner: None,
            });
        }
        if let Some(&last) = placed.values().max() {
            sketch.next_id = last.0.checked_add(1).ok_or(OutOfIds)?;
        }
        for constraint in &self.constraints {
            let constraint = constraint.map_items(resolve)?;
            sketch.add_constraint(constraint)?;
        }
        for dimension in &self.dimensions {
            let dimension = Dimension {
                measure: dimension.measure.map_items(resolve)?,
                ..dimension.clone()
            };
            sketch.add_dimension(dimension)?;
        }
        let mut auto = Vec::with_capacity(self.auto.len());
        for constraint in &self.auto {
            let constraint = constraint.map_items(resolve)?;
            auto.push(sketch.add_constraint(constraint)?);
        }
        Ok(auto)
    }
}

/// Why [`SketchEdit::apply`] can't make the sketch an edit asks for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EditError {
    /// Every id of the sketch has been used.
    OutOfIds,
    /// The edit changes the item `id` names, which is missing or can't be
    /// changed so: a point moved, a circle's radius, a curve made
    /// construction, a dimension changed, or made driving where it
    /// measures nothing. Or an [`Add`] names as the sketch's an id the sketch
    /// hasn't given out, one an edit before it made that wasn't applied.
    Target(Id),
    /// An [`Add`]'s placeholder is below its first, repeated, out of
    /// order, or names none of its new items.
    Placeholder(Id),
    /// The sketch made fails [`Sketch::check`], such as a point past the
    /// coordinate limit.
    Sketch(SketchError),
    /// A driving dimension, added or made so, of the distance between
    /// points at one place ([`Sketch::same_place`]).
    SamePlace,
    /// A dimension would hold what it measures now, `measured`
    /// (millimetres, or radians if an `angle`), which is past what one can
    /// be: more than `max`, less than `min`, or no angle.
    OutOfRange {
        measured: f64,
        min: f64,
        max: f64,
        angle: bool,
    },
    /// An end extended ([`SketchEdit::Extend`]) meets nothing ahead of
    /// it.
    NothingAhead,
    /// A [`SketchEdit::Mirror`] names no point or curve to mirror, off the
    /// line or of its own.
    NothingToMirror,
    /// A [`SketchEdit::Offset`]'s curves aren't a chain: lines and arcs
    /// joined end to end, no more than two at a point, or a circle or a
    /// spline alone.
    NotAChain,
    /// A [`SketchEdit::Offset`] of a spline would fold over itself: the
    /// spline turns towards the copy tighter than the distance.
    TooTight,
    /// A [`SketchEdit::Offset`] leaves nothing: every piece of the copy is
    /// within the distance of the chain, such as a loop offset inwards
    /// past its middle.
    NothingLeft,
    /// A [`SketchEdit::Offset`] would take more steps than
    /// [`MAX_OFFSET_WORK`](crate::MAX_OFFSET_WORK) working out what's
    /// left, or it or a [`SketchEdit::Extend`] meets a spline lying along
    /// another so closely that where they cross can't all be found.
    TooComplex,
    /// A [`SketchEdit::Fillet`]'s or [`SketchEdit::Chamfer`]'s lines make
    /// no corner at its point: two lines ending there, not chamfers, not
    /// parallel.
    NoCorner,
    /// A fillet or chamfer doesn't fit on its corner: it would reach past
    /// the end of a line, or a chamfer's angle wouldn't reach across.
    NoRoom,
    /// The edit would change a link's point or curve (`id`), or the link
    /// itself, which only follows what it comes from: it's removed whole.
    Linked(Id),
}

impl From<OutOfIds> for EditError {
    fn from(_: OutOfIds) -> Self {
        EditError::OutOfIds
    }
}

impl From<SketchError> for EditError {
    fn from(why: SketchError) -> Self {
        EditError::Sketch(why)
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OutOfIds => OutOfIds.fmt(f),
            EditError::Target(id) => {
                write!(f, "sketch item {id} can't be changed so, or is gone")
            }
            EditError::Placeholder(id) => {
                write!(f, "the sketch edit's new item {id} is misnumbered")
            }
            EditError::Sketch(why) => why.fmt(f),
            EditError::SamePlace => {
                f.write_str("the points are at the same place; move them apart first")
            }
            EditError::NothingAhead => f.write_str("there's nothing ahead of that end to reach"),
            EditError::NothingToMirror => f.write_str("there's nothing to mirror but the line"),
            EditError::NotAChain => f.write_str(
                "only a chain of lines and arcs joined end to end, or a circle or a spline alone, offsets",
            ),
            EditError::TooTight => {
                f.write_str("offset that far, the copy would fold where the spline curves tighter")
            }
            EditError::NothingLeft => f.write_str("offset that far, nothing is left of it"),
            EditError::TooComplex => f.write_str("that's too complex to work out"),
            EditError::NoCorner => f.write_str("only two lines ending at a point make a corner"),
            EditError::NoRoom => f.write_str("that's too large for the corner's lines"),
            EditError::Linked(_) => f.write_str(
                "projected and intersected geometry follows what it comes from: remove its link instead",
            ),
            &EditError::OutOfRange {
                measured,
                min,
                max,
                angle,
            } => {
                let unit = Some(if angle {
                    Unit::Angle(AngleUnit::Deg)
                } else {
                    Unit::Length(LengthUnit::Mm)
                });
                if measured > max {
                    let (measured, max) = (format(measured, unit), format(max, unit));
                    write!(
                        f,
                        "it measures {measured}, more than a dimension can be, {max}"
                    )
                } else if measured < min {
                    let min = format(min, unit);
                    write!(f, "it measures less than a dimension can be, {min}")
                } else {
                    let measured = format(measured, unit);
                    write!(f, "it measures {measured}, which a dimension can't be")
                }
            }
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EditError::Sketch(why) => Some(why),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
