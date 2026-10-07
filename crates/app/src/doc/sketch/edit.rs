//! Changing the sketch being edited: drawing with the tools, dragging,
//! deleting, construction and constraints. Each change is a
//! [`SketchEdit`], proposed to the solver (see [`propose`](super::propose))
//! and committed as the whole sketch it makes, solved, one
//! [`Command::SetSketch`], one undo step.
//!
//! [`Command::SetSketch`]: varde_document::Command::SetSketch

use std::sync::Arc;

use glam::DVec2;
use varde_document::{EditError, Sketch};
use varde_sketch::{
    Add, Constraint, Curve, Design, Dimension, Id, Kind, Measure, OutOfIds, Selectable, Shape,
    Side, SketchEdit, arc_through, tangent_between,
};
use varde_solve::Request;
use varde_view::typed::{self, Field, Outline};
use varde_view::{ConstraintKind, Inference, Level, Target, Tool, ToolClick};

use super::{Chain, Drag, Drawing};
use crate::doc::Doc;

impl Doc {
    /// The sketch being edited as it's worked on, with the edits waiting
    /// on the solver applied, if one is being edited and it can be
    /// changed.
    pub(super) fn editable_sketch(&self) -> Option<&Sketch> {
        self.working_sketch().filter(|_| self.editable())
    }

    /// Shows why an edit of the sketch being edited can't be made.
    pub(super) fn refuse(&mut self, why: varde_sketch::EditError) {
        if let Some(session) = &self.sketch {
            self.edit_error = Some(EditError::Sketch(session.feature, why));
        }
    }

    /// Takes `click` with the tool in use: places the shape's next point,
    /// and once the shape has all it needs, proposes it, with what its
    /// points snapped to as `auto` constraints (see [`place`]) and the
    /// values typed in its fields as its driving dimensions, which hold
    /// the click where they fix it (see [`typed::aim`]). A shape smaller
    /// than a pixel, or an arc through points on one line, is refused. A
    /// double-click ends a chain of lines. The value typed in a field open
    /// is taken first, and one refused takes no click. The Dimension tool
    /// and the shape tools pick what's clicked instead, see
    /// [`Doc::dimension_click`] and [`Doc::shape_click`].
    pub(crate) fn tool_click(&mut self, click: ToolClick) {
        // The viewport says where the next click snaps once the cursor
        // moves.
        if let Some(session) = &mut self.sketch {
            session.snap = None;
        }
        if self.drawing_field().is_some() {
            if self.take_field().is_err() {
                return;
            }
            self.close_value();
        }
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(mut drawing) = self
            .sketch
            .as_ref()
            .and_then(|session| session.tool.clone())
        else {
            return;
        };
        if !(click.at.is_finite() && click.pixel.is_finite() && click.pixel > 0.0) {
            return;
        }
        if drawing.tool == Tool::Dimension {
            self.dimension_click(click);
            return;
        }
        if !drawing.tool.draws() {
            self.shape_click(click);
            return;
        }
        if drawing.tool == Tool::Spline {
            self.spline_click(drawing, click);
            return;
        }
        if drawing.tool == Tool::Line && click.double {
            drawing.restart();
            self.set_drawing(drawing);
            return;
        }
        let Some(click) = typed::aim(&drawing.active(), click) else {
            return;
        };
        let next_id = sketch.next_id;
        let design = self.editor.document().design();
        let step = step(sketch, &drawing, click, &design);
        // The fields of the shape the click leaves show by it.
        if let Some(session) = &mut self.sketch {
            session.aim = Some(click);
        }
        match step {
            Err(OutOfIds) => self.refuse(OutOfIds.into()),
            Ok(Step::Refused) => {}
            Ok(Step::Placed) => {
                drawing.placed.push(click.at);
                drawing.targets.push(click.target);
                self.set_drawing(drawing);
            }
            Ok(Step::Added(add, chain)) => {
                // The chain's new points by the ids they got.
                let chain = chain.and_then(|chain| {
                    Some(Chain {
                        first: add.resolve(next_id, chain.first)?,
                        last: add.resolve(next_id, chain.last)?,
                        ..chain
                    })
                });
                if self.propose(SketchEdit::Add(add)) {
                    drawing.restart();
                    if let Some(chain) = chain {
                        drawing.placed = vec![click.at];
                        drawing.targets = vec![Some(Target::Point(chain.last))];
                        drawing.chain = Some(chain);
                    }
                    self.set_drawing(drawing);
                }
            }
        }
    }

    /// Puts `drawing` in place of the tool in use.
    pub(super) fn set_drawing(&mut self, drawing: Drawing) {
        if let Some(session) = &mut self.sketch {
            session.tool = Some(drawing);
        }
    }

    /// Drags the item `id`, grabbed at `from`, to `to`: a point, or a line
    /// with its ends, moves as the cursor does, a handle's mirrored end
    /// too (its tip moving the other way), and a circle or an arc takes
    /// the cursor's distance from its centre as its radius. Each
    /// step goes to the solver, and the sketch is shown as it last
    /// converged. A step that would put anything past the coordinate limit
    /// is left out. Not while edits wait on the solver: the drag starts
    /// once they're answered.
    pub(crate) fn drag_geometry(
        &mut self,
        id: Selectable,
        from: DVec2,
        to: DVec2,
        target: Option<Target>,
    ) {
        if !self.editable() || self.proposing() {
            return;
        }
        let Some((_, sketch)) = self.edited_sketch() else {
            return;
        };
        if self
            .sketch
            .as_ref()
            .is_some_and(|session| session.tool.is_some())
        {
            return;
        }
        let Some(edit) = dragged(sketch, id, from, to) else {
            return;
        };
        if edit
            .apply(sketch, &self.editor.document().design())
            .is_err()
        {
            return;
        }
        let (generation, revision) = (self.editor.generation(), self.editor.revision());
        let going = self
            .sketch
            .as_ref()
            .and_then(|session| session.drag.as_ref())
            .filter(|drag| drag.generation == generation)
            .map(|drag| (drag.session, drag.start.clone(), drag.solution.clone()));
        let (session, start, solution) = match going {
            Some(going) => going,
            None => {
                let start = Arc::new(sketch.clone());
                (self.next_drag(), start, None)
            }
        };
        if let (Some(solver), SketchEdit::Move { points, radii }) = (&mut self.solver, &edit) {
            solver.send(Request::Drag {
                session,
                sketch: start.clone(),
                points: points.clone(),
                radii: radii.clone(),
                units: self.editor.document().units(),
            });
        }
        if let Some(sketch) = &mut self.sketch {
            sketch.refusal = None;
            sketch.drag = Some(Drag {
                generation,
                revision,
                session,
                start,
                edit,
                target,
                solution,
            });
        }
    }

    /// Proposes the geometry dragged where it was dropped: the move to
    /// where the cursor was, from where the solver last converged, so the
    /// drag commits as one undo step what it showed, solved. A point
    /// snapped is tied to what it snapped to ([`snapped`]), a circle's rim
    /// put through the point it snapped to ([`rim_snapped`]): on that
    /// solution, still one step, or after the move where there's none.
    pub(crate) fn drop_geometry(&mut self) {
        let Some(drag) = self.sketch.as_mut().and_then(|session| session.drag.take()) else {
            return;
        };
        let tie = match drag.edit {
            SketchEdit::Move {
                ref points,
                ref radii,
            } => match (drag.target, points.first(), radii.first()) {
                (Some(target), Some(&(point, _)), _) => snapped(&drag.start, point, target),
                (Some(target), None, Some(&(circle, _))) => {
                    rim_snapped(&drag.start, circle, target)
                }
                _ => None,
            },
            _ => None,
        };
        match (tie, drag.solution) {
            (Some(tie), Some(solution)) => {
                self.propose_from(tie, Some((drag.revision, solution)));
            }
            (tie, solution) => {
                let from = solution.map(|solution| (drag.revision, solution));
                self.propose_from(drag.edit, from);
                if let Some(tie) = tie {
                    self.propose(tie);
                }
            }
        }
    }

    /// Deletes what's selected in the sketch and what depends on it, see
    /// [`Sketch::delete`], with every link any of whose geometry is
    /// selected, whole.
    pub(crate) fn delete_selection(&mut self) {
        let Some(session) = &self.sketch else {
            return;
        };
        let selected: Vec<_> = session.selection.iter().copied().collect();
        self.delete_items(selected);
    }

    /// Deletes the point or curve `id` (or a handle by its mirrored end)
    /// from its row's context menu: the whole selection if it's among it,
    /// as `Delete` would, else it alone.
    pub(crate) fn delete_item(&mut self, id: Selectable) {
        let Some(session) = &self.sketch else {
            return;
        };
        if session.selection.contains(&id) {
            self.delete_selection();
        } else {
            self.delete_items(vec![id]);
        }
    }

    /// Detaches the point `id` the curves share, if it can be, see
    /// [`Sketch::detach`](varde_sketch::Sketch::detachable).
    pub(crate) fn detach_point(&mut self, id: Id) {
        if self
            .editable_sketch()
            .is_some_and(|sketch| sketch.detachable(id))
        {
            self.propose(SketchEdit::Detach(id));
        }
    }

    /// Deletes `items` and what depends on them.
    fn delete_items(&mut self, items: Vec<Selectable>) {
        // The origin and axes are always there; a handle goes by its tip,
        // picked as a line or by its mirrored end.
        let mut ids: Vec<Id> = Vec::new();
        for id in items
            .into_iter()
            .map(Selectable::id)
            .filter(|id| !id.is_builtin())
        {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            return;
        }
        // A link any of whose geometry is selected goes whole, as only the
        // link can go: its row picked, or one curve of it. The sketch
        // face stays, its geometry left out.
        let face = self.sketch_face();
        if let Some(sketch) = self.editable_sketch() {
            for link in &sketch.links {
                if link.items().any(|item| ids.contains(&item)) {
                    ids.retain(|&id| link.items().all(|item| item != id));
                    if face != Some(link.id) {
                        ids.push(link.id);
                    }
                }
            }
        }
        ids.retain(|&id| face != Some(id));
        if ids.is_empty() {
            if face.is_some() {
                self.notice = Some(super::links::SKETCH_FACE_STAYS.to_owned());
            }
            return;
        }
        self.propose(SketchEdit::Delete(ids));
    }

    /// Turns the tool's next shapes between normal and construction
    /// geometry, or without a tool drawing shapes the curves selected: all
    /// construction unless they all are, then all normal.
    pub(crate) fn toggle_construction(&mut self) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(session) = &self.sketch else {
            return;
        };
        if let Some(drawing) = session.tool.as_ref().filter(|drawing| drawing.tool.draws()) {
            let drawing = Drawing {
                construction: !drawing.construction,
                ..drawing.clone()
            };
            self.set_drawing(drawing);
            return;
        }
        // The sketch face, any of it selected, turns whole, by whether it
        // counts for profiles; the rest selected as ever.
        let face = (self.sketch_face())
            .and_then(|id| sketch.link(id))
            .filter(|link| {
                link.items()
                    .any(|item| session.selection.contains(&item.into()))
            });
        let face_items: Vec<Id> = face.map_or_else(Vec::new, |link| link.items().collect());
        let selected: Vec<_> = sketch
            .curves
            .iter()
            .filter(|entry| {
                session.selection.contains(&entry.id.into()) && !face_items.contains(&entry.id)
            })
            .collect();
        let construction = selected.iter().any(|entry| !entry.construction);
        let ids: Vec<Id> = selected.iter().map(|entry| entry.id).collect();
        let face = face.map(|link| (link.id, link.profiles));
        if !ids.is_empty() || face.is_none() {
            self.propose(SketchEdit::SetConstruction { ids, construction });
        }
        if let Some((link, profiles)) = face {
            self.propose(SketchEdit::SetLinkProfiles {
                link,
                profiles: !profiles,
            });
        }
    }

    /// Constrains the geometry selected so, as [`Doc::constrain`] does,
    /// unless all of it has the constraint already (any way round, see
    /// [`same_constraint`]): then takes it off, rather than restate it,
    /// which the solver would refuse. The keys and the toolbar's and
    /// rail's buttons.
    pub(crate) fn toggle_constraint(&mut self, kind: ConstraintKind) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(session) = &self.sketch else {
            return;
        };
        let Some(constraints) = kind.make(sketch, &session.selection) else {
            return;
        };
        let held: Option<Vec<Id>> = (constraints.iter())
            .map(|made| {
                let mut entries = sketch.constraints.iter();
                let entry = entries.find(|entry| same_constraint(&entry.constraint, made))?;
                Some(entry.id)
            })
            .collect();
        match held {
            Some(ids) if !ids.is_empty() => {
                self.propose(SketchEdit::Delete(ids));
            }
            _ => self.constrain(kind),
        }
    }

    /// Constrains the geometry selected so, if it fits it, see
    /// [`ConstraintKind::make`]. With the Constrain tool the selection is
    /// cleared for the next.
    pub(crate) fn constrain(&mut self, kind: ConstraintKind) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(session) = &self.sketch else {
            return;
        };
        let Some(constraints) = kind.make(sketch, &session.selection) else {
            return;
        };
        let edit = SketchEdit::constrain(sketch, constraints);
        if self.propose(edit)
            && let Some(session) = &mut self.sketch
            && session.constraining
        {
            session.selection.clear();
        }
    }
}

/// Whether `a` and `b` are one constraint: of one kind on the same items,
/// in any order, whatever side a tangent is on.
fn same_constraint(a: &Constraint, b: &Constraint) -> bool {
    let items = |c: &Constraint| {
        let mut ids: Vec<Id> = c.items().map(|(id, _)| id).collect();
        ids.sort();
        ids
    };
    a == b || (ConstraintKind::of(a) == ConstraintKind::of(b) && items(a) == items(b))
}

/// What a tool's click does to the shape being drawn.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    /// Nothing: the shape would be too small, or no shape at all.
    Refused,
    /// The click is the shape's next point, and it needs more.
    Placed,
    /// The shape is complete, added by the edit, and the Line tool goes
    /// on from this chain, if it's given: its ids are the edit's.
    Added(Add, Option<Chain>),
}

/// How far a dimension added with a shape has its label from what it
/// measures, in pixels.
const LABEL_GAP: f64 = 24.0;

/// What `click` with `drawing` does to `sketch` in `design`, adding the
/// shape once it has all it needs, with the values typed as its
/// dimensions.
fn step(
    sketch: &Sketch,
    drawing: &Drawing,
    click: ToolClick,
    design: &Design,
) -> Result<Step, OutOfIds> {
    let construction = drawing.construction;
    let mut add = Add::new(sketch);
    let target = |index: usize| drawing.targets.get(index).copied().flatten();
    let gap = LABEL_GAP * click.pixel;
    Ok(match (drawing.tool, drawing.placed.as_slice()) {
        // A point where the sketch has one is that point again.
        (Tool::Point, _)
            if click
                .point()
                .is_some_and(|id| !id.is_builtin() && !sketch.is_linked(id)) =>
        {
            Step::Refused
        }
        (Tool::Point, _) => {
            place(sketch, &mut add, click.at, click.target)?;
            Step::Added(add, None)
        }
        (Tool::Line, &[start]) => line(sketch, add, drawing, start, click, design)?,
        (Tool::Circle, &[center_at]) => {
            let radius = center_at.distance(click.at);
            if radius < click.pixel {
                return Ok(Step::Refused);
            }
            let center = place(sketch, &mut add, center_at, target(0))?;
            let circle = add.curve(Curve::Circle { center, radius }, construction)?;
            add.auto.extend(through(circle, click.target));
            let label = center_at + away(center_at, click.at) * (radius + gap);
            let diameter = (Field::Diameter, Measure::Diameter(circle), label);
            typed_dimensions(sketch, &mut add, drawing, design, &[diameter]);
            Step::Added(add, None)
        }
        (Tool::Rectangle, &[_]) => rectangle(sketch, add, drawing, click, design)?,
        (Tool::Polygon, &[_]) => polygon(sketch, add, drawing, click, design)?,
        (Tool::Arc, &[start]) if start.distance(click.at) < click.pixel => Step::Refused,
        (Tool::Arc, &[start, end]) => {
            let through_at = click.at;
            // How far `through_at` is off the line through the ends, which
            // are a pixel apart or more.
            let chord = end - start;
            let off = chord.perp_dot(through_at - start).abs() / chord.length();
            let near_end = through_at.distance(start).min(through_at.distance(end));
            let arc = arc_through(start, end, through_at)
                .filter(|_| off >= click.pixel && near_end >= click.pixel);
            let Some(arc) = arc else {
                return Ok(Step::Refused);
            };
            let first = place(sketch, &mut add, start, target(0))?;
            let second = place(sketch, &mut add, end, target(1))?;
            // It runs counter-clockwise from whichever end that takes.
            let (arc_start, arc_end) = if arc.start == start {
                (first, second)
            } else {
                (second, first)
            };
            let center = add.point(arc.center)?;
            let curve = Curve::Arc {
                center,
                start: arc_start,
                end: arc_end,
            };
            let new = add.curve(curve, construction)?;
            add.auto.extend(through(new, click.target));
            let radius = arc.center.distance(arc.start);
            if let Some(Inference::Tangent(curve)) = click.inference {
                let shape = Shape::Round(arc.center, radius);
                add.auto.extend(tangent(sketch, curve, new, shape));
            }
            let label = arc.center + away(arc.center, through_at) * (radius + gap);
            let radius = (Field::Radius, Measure::Radius(new), label);
            typed_dimensions(sketch, &mut add, drawing, design, &[radius]);
            Step::Added(add, None)
        }
        // The Dimension tool and the shape tools pick rather than place,
        // see `Doc::dimension_click` and `Doc::shape_click`.
        (kind, _) if !kind.draws() => Step::Refused,
        _ => Step::Placed,
    })
}

/// Adds to `add`, made on `sketch`, the line `drawing` draws from `start`
/// to `click`: from the chain's last point if there is one, to the chain's
/// first point if it's clicked, closing the loop, or else to where it
/// snapped, with what it inferred of the line, and its length and angle
/// if they're typed. Refused if it's shorter than a pixel, or would lie
/// over a line joining the same points (doubling back over the one just
/// drawn, or closing a chain of one line).
fn line(
    sketch: &Sketch,
    mut add: Add,
    drawing: &Drawing,
    start: DVec2,
    click: ToolClick,
    design: &Design,
) -> Result<Step, OutOfIds> {
    let chain = drawing.chain;
    let closing = chain.filter(|chain| click.point() == Some(chain.first));
    let end_at = match closing {
        Some(chain) if chain.lines < 2 => return Ok(Step::Refused),
        Some(chain) => match sketch.point(chain.first) {
            Some(first) => first.at,
            None => return Ok(Step::Refused),
        },
        None => click.at,
    };
    if start.distance(end_at) < click.pixel {
        return Ok(Step::Refused);
    }
    let start_at = start;
    let start = match chain {
        Some(chain) => chain.last,
        None => {
            let target = drawing.targets.first().copied().flatten();
            place(sketch, &mut add, start, target)?
        }
    };
    let end = match closing {
        Some(chain) => chain.first,
        None => place(sketch, &mut add, end_at, click.target)?,
    };
    // Nor over a line joining the same points, such as the one just drawn.
    let joined = |entry: &varde_sketch::CurveEntry| match entry.curve {
        Curve::Line { start: a, end: b } => [a, b] == [start, end] || [b, a] == [start, end],
        _ => false,
    };
    if start == end || sketch.curves.iter().any(joined) {
        return Ok(Step::Refused);
    }
    let new = add.curve(Curve::Line { start, end }, drawing.construction)?;
    if let Some(inference) = click.inference {
        let shape = Shape::Line(start_at, end_at);
        add.auto.extend(inferred(sketch, new, shape, inference));
    }
    // The length's label beside the line, the angle's across from it
    // towards its end.
    let along = end_at - start_at;
    let beside = along.perp().normalize_or_zero() * LABEL_GAP * click.pixel;
    let measures = [
        (
            Field::Length,
            Measure::Length(new),
            start_at.midpoint(end_at) + beside,
        ),
        (
            Field::Angle,
            Measure::Angle(Id::X_AXIS, new),
            start_at + along * 0.75 - beside,
        ),
    ];
    typed_dimensions(sketch, &mut add, drawing, design, &measures);
    let chain = match (closing, chain) {
        (Some(_), _) => None,
        (None, Some(chain)) => Some(Chain {
            last: end,
            lines: chain.lines.saturating_add(1),
            ..chain
        }),
        (None, None) => Some(Chain {
            first: start,
            last: end,
            lines: 1,
        }),
    };
    Ok(Step::Added(add, chain))
}

/// Adds to `add`, made on `sketch`, the rectangle `drawing` draws to
/// `click` (see [`typed::outline`]): four lines, held horizontal and
/// vertical, sharing their corners, the first placed (or its centre) and
/// the one clicked where they snapped, with its width and height if
/// they're typed. From the centre, a construction diagonal has the centre
/// as its midpoint. Refused if it's narrower or lower than a pixel.
fn rectangle(
    sketch: &Sketch,
    mut add: Add,
    drawing: &Drawing,
    click: ToolClick,
    design: &Design,
) -> Result<Step, OutOfIds> {
    let Some(Outline::Rectangle { corners, center }) = typed::outline(&drawing.active(), click.at)
    else {
        return Ok(Step::Refused);
    };
    let [a, b, c, d] = corners;
    if a.distance(b) < click.pixel || a.distance(d) < click.pixel {
        return Ok(Step::Refused);
    }
    let first = drawing.targets.first().copied().flatten();
    let (center, a_id) = match center {
        Some(center) => (Some(place(sketch, &mut add, center, first)?), add.point(a)?),
        None => (None, place(sketch, &mut add, a, first)?),
    };
    let b_id = add.point(b)?;
    let c_id = place(sketch, &mut add, c, click.target)?;
    let d_id = add.point(d)?;
    let lines = closed_loop(&mut add, &[a_id, b_id, c_id, d_id], drawing.construction)?;
    add.constraints.extend([
        Constraint::Horizontal(lines[0]),
        Constraint::Vertical(lines[1]),
        Constraint::Horizontal(lines[2]),
        Constraint::Vertical(lines[3]),
    ]);
    if let Some(point) = center {
        let diagonal = add.curve(
            Curve::Line {
                start: a_id,
                end: c_id,
            },
            true,
        )?;
        add.constraints.push(Constraint::Midpoint {
            point,
            line: diagonal,
        });
    }
    // Labels outside it, off the middles of the sides at the first corner.
    let middle = a.midpoint(c);
    let outside =
        |side: DVec2| side + (side - middle).normalize_or_zero() * LABEL_GAP * click.pixel;
    let measures = [
        (
            Field::Width,
            Measure::Length(lines[0]),
            outside(a.midpoint(b)),
        ),
        (
            Field::Height,
            Measure::Length(lines[3]),
            outside(a.midpoint(d)),
        ),
    ];
    typed_dimensions(sketch, &mut add, drawing, design, &measures);
    Ok(Step::Added(add, None))
}

/// Adds to `add`, made on `sketch`, the polygon `drawing` draws to `click`
/// (see [`typed::outline`]): a construction circle about the centre
/// placed, the corners on it, the first where the click snapped, and
/// lines between them held equal, with the circle's diameter if it's
/// typed. Refused if its sides are shorter than a pixel.
fn polygon(
    sketch: &Sketch,
    mut add: Add,
    drawing: &Drawing,
    click: ToolClick,
    design: &Design,
) -> Result<Step, OutOfIds> {
    let outline = typed::outline(&drawing.active(), click.at);
    let Some(Outline::Polygon {
        center,
        radius,
        corners,
    }) = outline
    else {
        return Ok(Step::Refused);
    };
    let (Some(&first), Some(&second)) = (corners.first(), corners.get(1)) else {
        return Ok(Step::Refused);
    };
    if first.distance(second) < click.pixel {
        return Ok(Step::Refused);
    }
    let target = drawing.targets.first().copied().flatten();
    let center_id = place(sketch, &mut add, center, target)?;
    let circle = add.curve(
        Curve::Circle {
            center: center_id,
            radius,
        },
        true,
    )?;
    let mut points = vec![place(sketch, &mut add, first, click.target)?];
    for &corner in &corners[1..] {
        points.push(add.point(corner)?);
    }
    let lines = closed_loop(&mut add, &points, drawing.construction)?;
    add.constraints
        .extend(points.iter().map(|&point| Constraint::PointOnCurve {
            point,
            curve: circle,
        }));
    add.constraints.extend(
        lines[1..]
            .iter()
            .map(|&line| Constraint::Equal(lines[0], line)),
    );
    let label = center + away(center, first) * (radius + LABEL_GAP * click.pixel);
    let diameter = (Field::Diameter, Measure::Diameter(circle), label);
    typed_dimensions(sketch, &mut add, drawing, design, &[diameter]);
    Ok(Step::Added(add, None))
}

/// Adds to `add` a line from each of `points` to the next, and from the
/// last back to the first, construction if `construction`.
fn closed_loop(add: &mut Add, points: &[Id], construction: bool) -> Result<Vec<Id>, OutOfIds> {
    let next = points.iter().cycle().skip(1);
    points
        .iter()
        .zip(next)
        .map(|(&start, &end)| add.curve(Curve::Line { start, end }, construction))
        .collect()
}

/// The way from `from` to `to`, or along X where they're one place.
fn away(from: DVec2, to: DVec2) -> DVec2 {
    (to - from).try_normalize().unwrap_or(DVec2::X)
}

/// Adds to `add`, made on `sketch` in `design`, the value typed in each
/// field of `drawing`'s among `measures`, if one is, as the driving
/// dimension of the measure given with it, its label at the place given
/// with it, in sketch coordinates.
fn typed_dimensions(
    sketch: &Sketch,
    add: &mut Add,
    drawing: &Drawing,
    design: &Design,
    measures: &[(Field, Measure, DVec2)],
) {
    if drawing.typed.is_empty() {
        return;
    }
    // Labels are kept from their measures' anchors in what the shape
    // makes of the sketch, whose new ids are the edit's placeholders.
    let made = SketchEdit::Add(add.clone()).apply(sketch, design).ok();
    for (field, measure, label) in measures {
        let Some(value) = drawing.active().value_in(*field) else {
            continue;
        };
        let anchor = made.as_ref().and_then(|made| made.anchor(measure));
        add.dimensions.push(Dimension {
            measure: measure.clone(),
            value: value.clone(),
            driving: true,
            label: anchor.map_or(DVec2::ZERO, |anchor| *label - anchor),
            side: Side::Positive,
        });
    }
}

/// The point of a shape placed at `at` by `add`, made on `sketch`, having
/// snapped to `target`: a point of the sketch's itself, taken as the
/// shape's own, or else a new point, tied to what it snapped to by `auto`
/// constraints, which the solver drops where they restate the rest. The
/// origin and a link's points are never a shape's own: a point snapped
/// there is a new one, coincident with it.
pub(super) fn place(
    sketch: &Sketch,
    add: &mut Add,
    at: DVec2,
    target: Option<Target>,
) -> Result<Id, OutOfIds> {
    if let Some(Target::Point(id)) = target
        && !id.is_builtin()
        && !sketch.is_linked(id)
    {
        return Ok(id);
    }
    let point = add.point(at)?;
    add.auto.extend(ties(sketch, point, target));
    Ok(point)
}

/// The constraints tying the point `point` of `sketch` to `target`, what
/// it snapped to: coincident with a point, a line's midpoint, on a
/// curve or an axis, on a circle level with its centre or above it.
fn ties(sketch: &Sketch, point: Id, target: Option<Target>) -> Vec<Constraint> {
    match target {
        // The shape's own ties are the shape's to make.
        None | Some(Target::Own(_)) => Vec::new(),
        Some(Target::Point(other)) => vec![Constraint::Coincident(point, other)],
        Some(Target::Midpoint(line)) => vec![Constraint::Midpoint { point, line }],
        Some(Target::On(curve)) => vec![Constraint::PointOnCurve { point, curve }],
        Some(Target::Quadrant { round, level }) => {
            let on = Constraint::PointOnCurve {
                point,
                curve: round,
            };
            let center = match sketch.curve(round).map(|entry| &entry.curve) {
                Some(&Curve::Circle { center, .. } | &Curve::Arc { center, .. }) => Some(center),
                _ => None,
            };
            let level = center.map(|center| match level {
                Level::Horizontal => Constraint::HorizontalPoints(center, point),
                Level::Vertical => Constraint::VerticalPoints(center, point),
            });
            [Some(on), level].into_iter().flatten().collect()
        }
    }
}

/// The `auto` constraint of a curve `new` passing through the point it
/// snapped to, if it snapped to one: a circle's rim, an arc's last point.
fn through(new: Id, target: Option<Target>) -> Option<Constraint> {
    match target? {
        Target::Point(point) => Some(Constraint::PointOnCurve { point, curve: new }),
        _ => None,
    }
}

/// The `auto` constraint of the line `new` of `shape`, as `inference`
/// says it runs in `sketch`.
fn inferred(sketch: &Sketch, new: Id, shape: Shape, inference: Inference) -> Option<Constraint> {
    Some(match inference {
        Inference::Horizontal => Constraint::Horizontal(new),
        Inference::Vertical => Constraint::Vertical(new),
        Inference::Parallel(other) => Constraint::Parallel(new, other),
        Inference::Perpendicular(other) => Constraint::Perpendicular(new, other),
        Inference::Tangent(curve) => return tangent(sketch, curve, new, shape),
    })
}

/// The tangent between the curve `curve` of `sketch` and the new curve
/// `new` of `shape`, on the side their geometry is on.
fn tangent(sketch: &Sketch, curve: Id, new: Id, shape: Shape) -> Option<Constraint> {
    tangent_between((curve, sketch.shape(curve)?), (new, shape))
}

/// The move of the handle whose tip is `tip`, grabbed as a line at
/// `from`, dragged to `to`: turned about its fit point to point at `to`
/// (away from it, grabbed on the arm mirroring its tip), its length kept.
fn handle_dragged(sketch: &Sketch, tip: Id, from: DVec2, to: DVec2) -> Option<SketchEdit> {
    sketch.handle(tip)?;
    let (at, tip_at) = sketch.direction(tip)?;
    let arm = tip_at - at;
    let sign = if (from - at).dot(arm) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let toward = ((to - at) * sign).try_normalize()?;
    Some(SketchEdit::Move {
        points: vec![(tip, at + toward * arm.length())],
        radii: Vec::new(),
    })
}

/// The move of `id` of `sketch`, grabbed at `from`, dragged to `to`, see
/// [`Doc::drag_geometry`]. `None` if `id` names nothing that can be
/// dragged; whether it stays within the coordinate limit is for applying
/// it to tell.
fn dragged(sketch: &Sketch, id: Selectable, from: DVec2, to: DVec2) -> Option<SketchEdit> {
    let delta = to - from;
    let id = match id {
        Selectable::Item(id) => id,
        Selectable::HandleLine(tip) => return handle_dragged(sketch, tip, from, to),
        // The end follows the cursor, mirroring the tip in its fit point.
        Selectable::HandleEnd(tip) => {
            sketch.handle(tip)?;
            return Some(SketchEdit::Move {
                points: vec![(tip, sketch.point(tip)?.at - delta)],
                radii: Vec::new(),
            });
        }
    };
    let mut radii = Vec::new();
    let mut points: Vec<(Id, DVec2)> = match sketch.kind(id)? {
        Kind::Point => vec![(id, sketch.point(id)?.at + delta)],
        Kind::Line | Kind::Spline => sketch
            .curve(id)?
            .curve
            .points()
            .map(|point| Some((point, sketch.point(point)?.at + delta)))
            .collect::<Option<_>>()?,
        Kind::Circle => {
            let Curve::Circle { center, .. } = sketch.curve(id)?.curve else {
                return None;
            };
            radii.push((id, sketch.point(center)?.at.distance(to)));
            Vec::new()
        }
        Kind::Arc => {
            let Curve::Arc { center, start, end } = sketch.curve(id)?.curve else {
                return None;
            };
            let center_at = sketch.point(center)?.at;
            let radius = center_at.distance(to);
            if radius == 0.0 {
                return None;
            }
            [start, end]
                .into_iter()
                .map(|point| {
                    let direction = (sketch.point(point)?.at - center_at).try_normalize()?;
                    Some((point, center_at + direction * radius))
                })
                .collect::<Option<_>>()?
        }
        Kind::Constraint | Kind::Dimension => return None,
    };
    // A fit point's handle goes with it, its tip moved as far.
    let tips: Vec<(Id, DVec2)> = (sketch.splines())
        .flat_map(|(_, spline)| &spline.handles)
        .filter(|handle| {
            points.iter().any(|&(point, _)| point == handle.at)
                && points.iter().all(|&(point, _)| point != handle.tip)
        })
        .filter_map(|handle| Some((handle.tip, sketch.point(handle.tip)?.at + delta)))
        .collect();
    points.extend(tips);
    Some(SketchEdit::Move { points, radii })
}

/// The edit tying the point `point` of `sketch`, dragged, to `target`,
/// what it snapped to: the `auto` constraints a drawing tool's point
/// gets ([`ties`]), so one that restates the rest is dropped; or, on the
/// other end of an arc it's an end of, the arc closed
/// ([`SketchEdit::CloseArc`]), as two coincident ends of one would be
/// redundant.
fn snapped(sketch: &Sketch, point: Id, target: Target) -> Option<SketchEdit> {
    if let Target::Point(other) = target
        && let Some(arc) = (sketch.curves.iter()).find(|entry| {
            entry.curve.kind() == Kind::Arc
                && varde_view::closing(sketch, entry.id, point) == Some(other)
        })
    {
        return Some(SketchEdit::CloseArc(arc.id));
    }
    let auto = ties(sketch, point, Some(target));
    (!auto.is_empty()).then(|| {
        SketchEdit::Add(Add {
            auto,
            ..Add::new(sketch)
        })
    })
}

/// The edit putting the circle `circle` of `sketch`, its rim dragged,
/// through `target`, the point it snapped to: an `auto` point on it, as
/// a circle's rim drawn there gets ([`through`]).
fn rim_snapped(sketch: &Sketch, circle: Id, target: Target) -> Option<SketchEdit> {
    let auto = through(circle, Some(target))?;
    Some(SketchEdit::Add(Add {
        auto: vec![auto],
        ..Add::new(sketch)
    }))
}

#[cfg(test)]
mod tests;
