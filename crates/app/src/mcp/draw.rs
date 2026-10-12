//! `edit_sketch {sketch, operations}`: the MCP client's drawing in a
//! sketch, a list of operations made into the sketch edits the user's
//! tools make, proposed to the solve lane as the user's are, and
//! committed together as one undo step.
//!
//! Each operation is made on the sketch as the ones before it left it
//! (applied, not solved), so its ids and labels are checked as it
//! comes: shapes through the drawing tools' own code
//! ([`crate::doc::clicked`], the clicks a tool would take, a point given
//! by id or label the tool's snap to it), constraints as the Constrain
//! list applies them to a selection ([`ConstraintKind::make`]),
//! dimensions with their values read by the value field's bounds
//! ([`Measure::ask`]), deletions as `SketchEdit::Delete`, moves as a
//! drag dropped. They're then grouped into steps, each a `SketchEdit`:
//! the shapes and constraints in a row one `Add`, the moves in a row
//! one `Move`, each deletion one, and each dimension one of its own, so
//! one that would over-constrain the sketch is placed as a reference,
//! as the user's is (`Doc::as_reference`), which the result says. The
//! steps are proposed one after the other, each on the solution of the
//! one before (`Proposals::propose_steps`), and committed once the last
//! is accepted: one `Command::SetSketch`.
//!
//! The ids a label's items get are the ids they have in the sketch the
//! operations were made on: with that sketch changed between the steps
//! (the user's Undo of it meanwhile), it's not applied (`propose.rs`,
//! `SKETCH_CHANGED`); the rest of the design may change.

use std::collections::BTreeSet;

use glam::DVec2;
use serde_json::{Map, Value, json};
use varde_document::FeatureId;
use varde_expr::Value as ExprValue;
use varde_mcp::tools::{
    At, ConstraintWord, EditSketch, ItemRef, MeasureWord, Op, PolygonSize, RectangleFrom, SketchOp,
};
use varde_sketch::{
    Add, Constraint, Curve, Design, Dimension, Id, Kind, MAX_SPLINE_POINTS, MIN_LENGTH, Measure,
    NewCurve, Selectable, Side, Sketch, SketchEdit,
};
use varde_view::typed::{MAX_SIDES, MIN_SIDES};
use varde_view::{ConstraintKind, Target, Tool};

use super::args::sketch_named;
use super::edit::{Made, Refused};
use super::sketches::{Proposed, Proposing, sketch_made};
use crate::doc::{Clicks, Doc, Step, clicked, sketch_of};

/// The smallest a shape the client draws may be, as a tool refuses one
/// under a pixel: the shortest a dimension can hold.
const SMALLEST: f64 = MIN_LENGTH;

/// What an operation labelled made: what it is, the item the label
/// alone names (none for a rectangle or a polygon), and its parts by
/// name (`"start"`, `"side1"`, ...).
#[derive(Debug)]
struct Labelled {
    label: String,
    what: &'static str,
    main: Option<Id>,
    parts: Vec<(String, Id)>,
}

/// Points moved, each to its place.
type Moves = Vec<(Id, DVec2)>;

/// The operations made so far, on the sketch they leave.
struct Planner<'a> {
    design: Design<'a>,
    /// The sketch as the operations so far leave it, applied, not solved.
    sketch: Sketch,
    labels: Vec<Labelled>,
    steps: Vec<Step>,
    /// The shapes and constraints in a row not yet a step: their `Add`,
    /// and the first and the last of their operations.
    geometry: Option<(Add, usize, usize)>,
    /// The moves in a row not yet a step.
    moves: Option<(Moves, usize, usize)>,
}

/// An operation's argument error, failing the call.
fn failed(index: usize, why: impl std::fmt::Display) -> Refused {
    Refused::failed(format!("edit_sketch: operations[{index}]: {why}"))
}

/// An operation the sketch refuses, the design unchanged.
fn refused(index: usize, why: impl std::fmt::Display) -> Refused {
    Refused::not_applied(format!("operations[{index}]: {why}"))
}

/// `edit_sketch`'s operations made into the steps to propose on the
/// sketch named, or why not, naming the operation. `queued`: checked as
/// it comes, behind the sketch edits waiting on the solver, on the
/// sketch with theirs applied; else at its turn, on the committed
/// sketch, which it's proposed on.
pub(super) fn edit_sketch(doc: &Doc, args: &EditSketch, queued: bool) -> Result<Proposed, Refused> {
    let document = doc.editor.document();
    let feature = sketch_named(document, &args.sketch)?;
    let base = match queued {
        true => doc.sketch_as_queued(feature),
        false => sketch_of(document, feature).cloned(),
    };
    let base = base.ok_or_else(|| Refused::failed("internal error: the sketch is gone"))?;
    let next_id = base.next_id;
    let mut planner = Planner {
        design: document.design(),
        sketch: base,
        labels: Vec::new(),
        steps: Vec::new(),
        geometry: None,
        moves: None,
    };
    for (index, operation) in args.operations.iter().enumerate() {
        planner.operation(index, operation)?;
    }
    planner.flush();
    let labels = planner.labels_made();
    Ok(Proposed {
        feature,
        what: Proposing::Steps {
            steps: planner.steps,
            next_id,
            made: Made::Drawn {
                feature,
                labels,
                freedom: None,
                referenced: Vec::new(),
            },
        },
    })
}

/// An `edit_sketch`'s result: the sketch's name, id and plane, each
/// label's ids, the degrees of freedom left, and the operations whose
/// dimensions were placed as references, saying so.
pub(super) fn drawn_made(
    doc: &Doc,
    feature: FeatureId,
    labels: &Value,
    freedom: Option<usize>,
    referenced: &[usize],
) -> Value {
    let mut made = sketch_made(doc, feature);
    made["labels"] = labels.clone();
    if let Some(freedom) = freedom {
        made["degrees_of_freedom"] = json!(freedom);
    }
    if !referenced.is_empty() {
        made["references"] = json!(referenced);
        let which: Vec<String> = (referenced.iter())
            .map(|index| format!("operations[{index}]"))
            .collect();
        made["note"] = json!(format!(
            "The dimension of {} would over-constrain the sketch: placed as a reference",
            which.join(", ")
        ));
    }
    made
}

/// The item of `sketch` numbered `number`: a point, curve, constraint,
/// dimension or link.
fn id_in(sketch: &Sketch, number: u32) -> Option<Id> {
    let points = sketch.points.iter().map(|point| point.id);
    let curves = sketch.curves.iter().map(|entry| entry.id);
    let constraints = sketch.constraints.iter().map(|entry| entry.id);
    let dimensions = sketch.dimensions.iter().map(|entry| entry.id);
    let links = sketch.links.iter().map(|link| link.id);
    (points
        .chain(curves)
        .chain(constraints)
        .chain(dimensions)
        .chain(links))
    .find(|id| id.get() == number)
}

/// What `kind` is, in a word for a sentence.
fn noun(kind: Option<Kind>) -> &'static str {
    match kind {
        Some(Kind::Point) => "a point",
        Some(Kind::Line) => "a line",
        Some(Kind::Circle) => "a circle",
        Some(Kind::Arc) => "an arc",
        Some(Kind::Spline) => "a spline",
        Some(Kind::Constraint) => "a constraint",
        Some(Kind::Dimension) => "a dimension",
        None => "a link",
    }
}

impl Planner<'_> {
    /// Makes the operation `operation`, at `index`, on the sketch.
    fn operation(&mut self, index: usize, operation: &SketchOp) -> Result<(), Refused> {
        let clicks = |tool: Tool| Clicks {
            tool,
            placed: Vec::new(),
            at: DVec2::ZERO,
            target: None,
            pixel: SMALLEST,
            construction: operation.construction,
            sides: MIN_SIDES,
            centered: false,
            control: false,
            closed: false,
        };
        let drawn = match &operation.op {
            Op::Point { at } => Clicks {
                at: self.coordinates(index, at)?,
                ..clicks(Tool::Point)
            },
            Op::Line { start, end } => {
                let (end, target) = self.at(index, end)?;
                Clicks {
                    placed: vec![self.at(index, start)?],
                    at: end,
                    target,
                    ..clicks(Tool::Line)
                }
            }
            Op::Arc {
                start,
                end,
                through,
            } => Clicks {
                placed: vec![self.at(index, start)?, self.at(index, end)?],
                at: self.coordinates(index, through)?,
                ..clicks(Tool::Arc)
            },
            Op::Circle { center, diameter } => {
                let (at, target) = self.at(index, center)?;
                let diameter = self.length(index, diameter, "diameter", true)?;
                Clicks {
                    placed: vec![(at, target)],
                    at: at + DVec2::new(diameter / 2.0, 0.0),
                    ..clicks(Tool::Circle)
                }
            }
            Op::Spline {
                points,
                closed,
                control,
            } => {
                if points.len() > MAX_SPLINE_POINTS {
                    return Err(failed(
                        index,
                        format_args!("a spline has at most {MAX_SPLINE_POINTS} points"),
                    ));
                }
                let placed = (points.iter())
                    .map(|point| self.at(index, point))
                    .collect::<Result<_, _>>()?;
                Clicks {
                    placed,
                    control: *control,
                    closed: *closed,
                    ..clicks(Tool::Spline)
                }
            }
            Op::Rectangle { from, size } => {
                let width = self.length(index, &size[0], "the width", false)?;
                let height = self.length(index, &size[1], "the height", false)?;
                let size = DVec2::new(width, height);
                let (first, centered) = match from {
                    RectangleFrom::Corner(corner) => (self.at(index, corner)?, false),
                    RectangleFrom::Center(center) => (self.at(index, center)?, true),
                };
                // The tool's second click: the opposite corner, or from
                // the centre one corner.
                let reach = if centered { size / 2.0 } else { size };
                Clicks {
                    placed: vec![first],
                    at: first.0 + reach,
                    centered,
                    ..clicks(Tool::Rectangle)
                }
            }
            Op::Polygon {
                center,
                sides,
                size,
            } => {
                if !(MIN_SIDES..=MAX_SIDES).contains(sides) {
                    return Err(failed(
                        index,
                        format_args!("a polygon has {MIN_SIDES} to {MAX_SIDES} sides"),
                    ));
                }
                let (center, target) = self.at(index, center)?;
                let (at, corner) = match size {
                    PolygonSize::Corner(corner) => self.at(index, corner)?,
                    PolygonSize::Diameter(diameter) => {
                        let diameter = self.length(index, diameter, "diameter", true)?;
                        (center + DVec2::new(diameter / 2.0, 0.0), None)
                    }
                };
                Clicks {
                    placed: vec![(center, target)],
                    at,
                    target: corner,
                    sides: *sides,
                    ..clicks(Tool::Polygon)
                }
            }
            Op::Constraint { kind, items } => return self.constrain(index, *kind, items),
            Op::Dimension {
                kind,
                items,
                value,
                driving,
            } => return self.dimension(index, *kind, items, value.as_deref(), *driving),
            Op::Delete { items } => return self.delete(index, items),
            Op::Move { point, to } => return self.move_point(index, point, to),
        };
        self.draw(index, operation, &drawn)
    }

    /// Draws the shape `clicks` place, as their tool would, labelling it
    /// as `operation` asks.
    fn draw(&mut self, index: usize, operation: &SketchOp, clicks: &Clicks) -> Result<(), Refused> {
        let add = clicked(&self.sketch, &self.design, clicks)
            .map_err(|out| refused(index, out))?
            .ok_or_else(|| refused(index, too_small(&operation.op)))?;
        self.add(index, add.clone())?;
        if let Some(label) = &operation.label {
            let first = clicks.placed.first().map(|&(at, _)| at);
            let (what, main, parts) = labelled(&operation.op, &add, &self.sketch, first);
            self.labels.push(Labelled {
                label: label.clone(),
                what,
                main,
                parts,
            });
        }
        Ok(())
    }

    /// Adds `add`, made on the sketch, to it and to the shapes and
    /// constraints in a row.
    fn add(&mut self, index: usize, add: Add) -> Result<(), Refused> {
        self.flush_moves();
        self.sketch = SketchEdit::Add(add.clone())
            .apply(&self.sketch, &self.design)
            .map_err(|why| refused(index, why))?;
        match &mut self.geometry {
            // Made on the sketch the group's left, its new items' ids go
            // on from the group's, so it's one `Add` with them.
            Some((group, _, last)) => {
                group.points.extend(add.points);
                group.curves.extend(add.curves);
                group.constraints.extend(add.constraints);
                group.dimensions.extend(add.dimensions);
                group.auto.extend(add.auto);
                *last = index;
            }
            None => self.geometry = Some((add, index, index)),
        }
        Ok(())
    }

    /// Ends the shapes and constraints, and the moves, in a row, each a
    /// step.
    fn flush(&mut self) {
        self.flush_geometry();
        self.flush_moves();
    }

    /// Ends the shapes and constraints in a row, a step.
    fn flush_geometry(&mut self) {
        if let Some((add, first, last)) = self.geometry.take() {
            self.steps.push(Step {
                edit: SketchEdit::Add(add),
                ops: (first, last),
                measured: None,
            });
        }
    }

    /// Ends the moves in a row, a step.
    fn flush_moves(&mut self) {
        if let Some((points, first, last)) = self.moves.take() {
            self.steps.push(Step {
                edit: SketchEdit::Move {
                    points,
                    radii: Vec::new(),
                },
                ops: (first, last),
                measured: None,
            });
        }
    }

    /// Applies `edit`, a step of its own made of the operation at
    /// `index`, after what's before it: a dimension's if `measured` is
    /// given ([`Step::measured`]).
    fn step(
        &mut self,
        index: usize,
        edit: SketchEdit,
        measured: Option<bool>,
    ) -> Result<(), Refused> {
        self.flush();
        self.sketch =
            (edit.apply(&self.sketch, &self.design)).map_err(|why| refused(index, why))?;
        self.steps.push(Step {
            edit,
            ops: (index, index),
            measured,
        });
        Ok(())
    }

    /// A constraint of `kind` on `items`, as the Constrain list applies
    /// it to them selected.
    fn constrain(
        &mut self,
        index: usize,
        kind: ConstraintWord,
        items: &[ItemRef],
    ) -> Result<(), Refused> {
        let kind = match kind {
            ConstraintWord::Coincident => ConstraintKind::Coincident,
            ConstraintWord::Horizontal => ConstraintKind::Horizontal,
            ConstraintWord::Vertical => ConstraintKind::Vertical,
            ConstraintWord::Parallel => ConstraintKind::Parallel,
            ConstraintWord::Perpendicular => ConstraintKind::Perpendicular,
            ConstraintWord::Tangent => ConstraintKind::Tangent,
            ConstraintWord::Smooth => ConstraintKind::Smooth,
            ConstraintWord::Equal => ConstraintKind::Equal,
            ConstraintWord::Concentric => ConstraintKind::Concentric,
            ConstraintWord::Midpoint => ConstraintKind::Midpoint,
            ConstraintWord::Symmetric => ConstraintKind::Symmetric,
            ConstraintWord::Fix => ConstraintKind::Fix,
        };
        let mut selected = BTreeSet::new();
        for item in items {
            selected.insert(Selectable::Item(self.item(index, item)?));
        }
        let constraints = kind.make(&self.sketch, &selected).ok_or_else(|| {
            failed(
                index,
                format_args!(
                    "a {} constraint doesn't fit those items, as the Constrain list wouldn't offer it",
                    kind.label().to_lowercase()
                ),
            )
        })?;
        let add = Add {
            constraints,
            ..Add::new(&self.sketch)
        };
        self.add(index, add)
    }

    /// A dimension of `kind` on `items`: driving at `value` as the value
    /// field reads it, or at what it measures without one, or a
    /// reference, a step of its own.
    fn dimension(
        &mut self,
        index: usize,
        kind: MeasureWord,
        items: &[ItemRef],
        value: Option<&str>,
        driving: bool,
    ) -> Result<(), Refused> {
        let ids = (items.iter())
            .map(|item| self.item(index, item))
            .collect::<Result<Vec<_>, _>>()?;
        let measure = match (kind, &ids[..]) {
            (MeasureWord::Distance, &[a, b]) => Measure::Distance(a, b),
            (MeasureWord::HorizontalDistance, &[a, b]) => Measure::HorizontalDistance(a, b),
            (MeasureWord::VerticalDistance, &[a, b]) => Measure::VerticalDistance(a, b),
            (MeasureWord::Angle, &[a, b]) => Measure::Angle(a, b),
            (MeasureWord::EdgeDistance, &[a, b]) => Measure::EdgeDistance(a, b),
            (MeasureWord::Length, &[line]) => Measure::Length(line),
            (MeasureWord::Radius, &[round]) => Measure::Radius(round),
            (MeasureWord::Diameter, &[round]) => Measure::Diameter(round),
            (
                MeasureWord::Distance
                | MeasureWord::HorizontalDistance
                | MeasureWord::VerticalDistance
                | MeasureWord::Angle
                | MeasureWord::EdgeDistance,
                _,
            ) => return Err(failed(index, "that dimension takes two items")),
            _ => return Err(failed(index, "that dimension takes one item")),
        };
        // What it measures now, and the side the geometry is on, as the
        // Dimension tool places one.
        let (held, side) = (self.sketch)
            .held(&measure, Side::Positive, &self.design)
            .map_err(|why| refused(index, why))?;
        let measured = value.is_none();
        let value = match value {
            Some(text) => ExprValue::new(text.trim(), &measure.ask(&self.design))
                .map_err(|why| failed(index, format_args!("value: {why}")))?,
            None => held,
        };
        let mut add = Add::new(&self.sketch);
        add.dimensions.push(Dimension {
            measure,
            value,
            driving,
            label: DVec2::ZERO,
            side,
        });
        self.step(index, SketchEdit::Add(add), Some(measured))
    }

    /// Deletes `items` with what depends on them, a step of its own.
    fn delete(&mut self, index: usize, items: &[ItemRef]) -> Result<(), Refused> {
        let mut ids = Vec::with_capacity(items.len());
        for item in items {
            let id = self.item(index, item)?;
            if id.is_builtin() {
                return Err(failed(index, "the origin and the axes can't be deleted"));
            }
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        self.step(index, SketchEdit::Delete(ids), None)
    }

    /// Moves `point` to `to`, the rest following as a drag dropped there;
    /// with the moves in a row before it, one step.
    fn move_point(
        &mut self,
        index: usize,
        point: &ItemRef,
        to: &[String; 2],
    ) -> Result<(), Refused> {
        let id = self.item(index, point)?;
        let kind = self.sketch.kind(id);
        if kind != Some(Kind::Point) || id.is_builtin() {
            return Err(failed(
                index,
                format_args!("only a point of the sketch's moves, not {}", noun(kind)),
            ));
        }
        let to = self.coordinates(index, to)?;
        self.flush_geometry();
        let edit = SketchEdit::Move {
            points: vec![(id, to)],
            radii: Vec::new(),
        };
        self.sketch =
            (edit.apply(&self.sketch, &self.design)).map_err(|why| refused(index, why))?;
        match &mut self.moves {
            Some((points, _, last)) => {
                match points.iter_mut().find(|(moved, _)| *moved == id) {
                    Some((_, at)) => *at = to,
                    None => points.push((id, to)),
                }
                *last = index;
            }
            None => self.moves = Some((vec![(id, to)], index, index)),
        }
        Ok(())
    }

    /// The item `item` names: by id, as `sketch` lists it; the origin or
    /// an axis by name; or what an earlier operation labelled, or a part
    /// of it (`"l.start"`). It must still be there.
    fn item(&self, index: usize, item: &ItemRef) -> Result<Id, Refused> {
        let text = match item {
            ItemRef::Id(number) => {
                return id_in(&self.sketch, *number)
                    .ok_or_else(|| failed(index, format_args!("the sketch has no item {number}")));
            }
            ItemRef::Label(text) => text.as_str(),
        };
        match text {
            "origin" => return Ok(Id::ORIGIN),
            "x_axis" => return Ok(Id::X_AXIS),
            "y_axis" => return Ok(Id::Y_AXIS),
            _ => {}
        }
        let shown = varde_mcp::shown(text);
        let (label, part) = match text.split_once('.') {
            Some((label, part)) => (label, Some(part)),
            None => (text, None),
        };
        let Some(labelled) = self.labels.iter().find(|made| made.label == label) else {
            return Err(failed(
                index,
                format_args!(
                    "no earlier operation is labelled {:?}, and it's no id",
                    varde_mcp::shown(label)
                ),
            ));
        };
        let parts = || {
            let names: Vec<&str> = labelled
                .parts
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();
            names.join(", ")
        };
        let id = match part {
            None => labelled.main.ok_or_else(|| {
                failed(
                    index,
                    format_args!(
                        "{shown:?} is {}: name one of its parts ({})",
                        labelled.what,
                        parts()
                    ),
                )
            })?,
            Some(part) => (labelled.parts.iter())
                .find(|(name, _)| name == part)
                .map(|&(_, id)| id)
                .ok_or_else(|| {
                    failed(
                        index,
                        format_args!(
                            "{shown:?}: {} has no such part; its parts are {}",
                            labelled.what,
                            parts()
                        ),
                    )
                })?,
        };
        if self.sketch.kind(id).is_none() {
            return Err(failed(
                index,
                format_args!("{shown:?} was deleted by an earlier operation"),
            ));
        }
        Ok(id)
    }

    /// Where a shape's point goes: a new point at coordinates, or a point
    /// of the sketch's, which the tool takes as snapped to.
    fn at(&self, index: usize, at: &At) -> Result<(DVec2, Option<Target>), Refused> {
        let item = match at {
            At::Point(coordinates) => return Ok((self.coordinates(index, coordinates)?, None)),
            At::Item(item) => self.item(index, item)?,
        };
        let place = match self.sketch.point(item) {
            Some(point) => point.at,
            None if item == Id::ORIGIN => DVec2::ZERO,
            None => {
                return Err(failed(
                    index,
                    format_args!("that's {}, not a point", noun(self.sketch.kind(item))),
                ));
            }
        };
        Ok((place, Some(Target::Point(item))))
    }

    /// The point `[x, y]` typed, lengths in the design's units, within
    /// the coordinate limit.
    fn coordinates(&self, index: usize, point: &[String; 2]) -> Result<DVec2, Refused> {
        super::args::typed_point(&self.design, point).map_err(|why| failed(index, why))
    }

    /// A length typed, `said` in an error: above zero if `positive`,
    /// within the coordinate limit either side.
    fn length(&self, index: usize, text: &str, said: &str, positive: bool) -> Result<f64, Refused> {
        let mut ask = super::args::coordinate_ask(&self.design);
        if positive {
            ask = ask.positive();
        }
        ExprValue::new(text.trim(), &ask)
            .map(|value| value.value)
            .map_err(|why| failed(index, format_args!("{said}: {why}")))
    }

    /// Each label and the ids its items got, for the result: the item
    /// it names alone as `id`, and its parts.
    fn labels_made(&self) -> Value {
        let mut labels = Map::new();
        for made in &self.labels {
            let mut ids = Map::new();
            if let Some(main) = made.main {
                ids.insert("id".into(), json!(main.get()));
            }
            for (part, id) in &made.parts {
                ids.insert(part.clone(), json!(id.get()));
            }
            labels.insert(made.label.clone(), Value::Object(ids));
        }
        Value::Object(labels)
    }
}

/// Why the tool refuses the shape `op` asks for.
fn too_small(op: &Op) -> String {
    match op {
        Op::Line { .. } => format!(
            "a line is at least {SMALLEST} mm long, between two points no line joins already"
        ),
        Op::Arc { .. } => format!(
            "an arc's three points can't be on one line, nor within {SMALLEST} mm of each other"
        ),
        Op::Rectangle { .. } => format!("a rectangle is at least {SMALLEST} mm wide and high"),
        Op::Polygon { .. } => format!("a polygon's sides are at least {SMALLEST} mm long"),
        Op::Circle { .. } => format!("a circle is at least {SMALLEST} mm across"),
        Op::Spline { .. } => {
            format!(
                "the Spline tool doesn't take those points: too few for its kind, open or closed, \
                 two in a row within {SMALLEST} mm, a point given twice, or the first again (give \
                 closed instead)"
            )
        }
        _ => "the tool refuses that".to_owned(),
    }
}

/// What a shape `op` made, `add`, is for a label: what it is, the item
/// the label names alone, and its parts by name. `sketch` is the sketch
/// with `add` applied, and `first` the operation's first click.
fn labelled(
    op: &Op,
    add: &Add,
    sketch: &Sketch,
    first: Option<DVec2>,
) -> (&'static str, Option<Id>, Vec<(String, Id)>) {
    let part = |name: &str, id: Id| (name.to_owned(), id);
    let curve = add.curves.last();
    match (op, curve.map(|new| (new.id, &new.curve))) {
        (Op::Point { .. }, _) => {
            let point = add.points.first().map(|&(id, _)| id);
            ("a point", point, Vec::new())
        }
        (Op::Line { .. }, Some((id, &Curve::Line { start, end }))) => (
            "a line",
            Some(id),
            vec![part("start", start), part("end", end)],
        ),
        (Op::Arc { .. }, Some((id, &Curve::Arc { center, start, end }))) => {
            // An arc runs counter-clockwise, from whichever end that
            // takes: its label's start is where the operation's is.
            let from_first = |point: Id| Some(sketch.point(point)?.at.distance(first?));
            let (start, end) = match (from_first(start), from_first(end)) {
                (Some(from_start), Some(from_end)) if from_end < from_start => (end, start),
                _ => (start, end),
            };
            (
                "an arc",
                Some(id),
                vec![
                    part("center", center),
                    part("start", start),
                    part("end", end),
                ],
            )
        }
        (Op::Circle { .. }, Some((id, &Curve::Circle { center, .. }))) => {
            ("a circle", Some(id), vec![part("center", center)])
        }
        (Op::Spline { .. }, Some((id, Curve::Spline(spline)))) => {
            let mut parts: Vec<(String, Id)> = (spline.points.iter().enumerate())
                .map(|(at, &point)| (format!("point{}", at.saturating_add(1)), point))
                .collect();
            if let (Some(&first), Some(&last)) = (spline.points.first(), spline.points.last()) {
                parts.push(part("start", first));
                parts.push(part("end", last));
            }
            ("a spline", Some(id), parts)
        }
        (Op::Rectangle { .. }, _) => {
            // Its four lines first, from its corner on round; from the
            // centre, the centre the diagonal's midpoint.
            let mut parts = sides_and_corners(add.curves.iter().take(4));
            let center = add
                .constraints
                .iter()
                .find_map(|constraint| match constraint {
                    &Constraint::Midpoint { point, .. } => Some(point),
                    _ => None,
                });
            if let Some(center) = center {
                parts.push(part("center", center));
            }
            ("a rectangle", None, parts)
        }
        (Op::Polygon { .. }, _) => {
            // Its construction circle first, then its sides.
            let mut parts = Vec::new();
            if let Some(first) = add.curves.first()
                && let Curve::Circle { center, .. } = first.curve
            {
                parts.push(part("center", center));
                parts.push(part("circle", first.id));
            }
            parts.extend(sides_and_corners(add.curves.iter().skip(1)));
            ("a polygon", None, parts)
        }
        _ => ("a shape", None, Vec::new()),
    }
}

/// The corners and sides of a closed loop of `lines`: each line's start
/// as `corner1`, `corner2`, ..., then the lines as `side1`, `side2`, ...
fn sides_and_corners<'a>(lines: impl Iterator<Item = &'a NewCurve> + Clone) -> Vec<(String, Id)> {
    let numbered = |name: &str, at: usize| format!("{name}{}", at.saturating_add(1));
    let corners = (lines.clone().enumerate()).filter_map(|(at, new)| match new.curve {
        Curve::Line { start, .. } => Some((numbered("corner", at), start)),
        _ => None,
    });
    let sides = (lines.enumerate()).map(|(at, new)| (numbered("side", at), new.id));
    corners.chain(sides).collect()
}
