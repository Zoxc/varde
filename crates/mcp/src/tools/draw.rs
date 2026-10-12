//! `edit_sketch`'s arguments: a list of operations on a sketch, in
//! order, read into checked [`SketchOp`]s, and its schema.
//!
//! Everything is bounded as it's read: at most [`MAX_OPERATIONS`]
//! operations, each list at most [`MAX_ITEMS`] (a spline's points
//! [`SPLINE_POINTS`]), labels at most [`LABEL_BYTES`], unique, without a
//! `.` (which names a part: `"l.start"`), a polygon's sides in
//! [`MIN_SIDES`]`..=`[`MAX_SIDES`], ids whole numbers that fit a `u32`.
//! Coordinates and values stay typed text, which the app reads in the
//! design's units and bounds as the sketch's fields do.

use serde_json::{Map, Value, json};

use super::{
    Arguments, CallError, MAX_ITEMS, TEXT_BYTES, coordinate, edit_once, id, sketch, words,
};

/// The most operations one `edit_sketch` takes: a sketch of a few
/// shapes, their constraints and dimensions in one call, each
/// dimension solved on its own (as the user's are placed one by one),
/// so the call stays well inside the solver's 30 s.
pub const MAX_OPERATIONS: usize = 64;

/// The longest label, in bytes.
pub const LABEL_BYTES: usize = 64;

/// The most points a spline operation takes: the sketch's own bound
/// (`varde_sketch::MAX_SPLINE_POINTS`).
pub const SPLINE_POINTS: usize = 100;

/// The fewest and the most sides a polygon takes, as the Polygon tool
/// (`varde_view::typed::MIN_SIDES`, `MAX_SIDES`).
pub const MIN_SIDES: u32 = 3;
pub const MAX_SIDES: u32 = 64;

/// The names of the sketch's origin and axes, which items may name and
/// labels can't be.
pub const RESERVED: [&str; 3] = ["origin", "x_axis", "y_axis"];

/// `edit_sketch`'s arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct EditSketch {
    /// The sketch, by its Timeline name.
    pub sketch: String,
    /// At least one, at most [`MAX_OPERATIONS`].
    pub operations: Vec<SketchOp>,
}

/// One operation of `edit_sketch`.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchOp {
    /// What later operations name what it makes by, for a point, a
    /// curve or a shape: at most [`LABEL_BYTES`], no `.`, unique in
    /// the call.
    pub label: Option<String>,
    /// Whether the curves it makes are construction geometry.
    pub construction: bool,
    pub op: Op,
}

/// A point or curve of the sketch: by its id, as `sketch` lists it, or
/// by a label of the call's, `"l"` or a part of it, `"l.start"`, at
/// most twice [`LABEL_BYTES`] and a part's name.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemRef {
    Id(u32),
    Label(String),
}

/// Where a shape's point goes: a new point at `[x, y]` (typed text,
/// lengths in the sketch's own coordinates), or a point of the
/// sketch's, which the shape takes as its own (the origin: a new point
/// on it).
#[derive(Clone, Debug, PartialEq)]
pub enum At {
    Point([String; 2]),
    Item(ItemRef),
}

/// What a rectangle is drawn from.
#[derive(Clone, Debug, PartialEq)]
pub enum RectangleFrom {
    /// A corner, the size running from it (either way, by sign).
    Corner(At),
    /// Its centre.
    Center(At),
}

/// Where a polygon's first corner is.
#[derive(Clone, Debug, PartialEq)]
pub enum PolygonSize {
    /// At a corner.
    Corner(At),
    /// Its corners on a circle of this diameter, the first along x from
    /// the centre.
    Diameter(String),
}

/// A constraint's kind, as the sketch's Constrain list names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintWord {
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    Smooth,
    Equal,
    Concentric,
    Midpoint,
    Symmetric,
    Fix,
}

/// The words a constraint's `kind` takes.
const CONSTRAINT_WORDS: &[(&str, ConstraintWord)] = &[
    ("coincident", ConstraintWord::Coincident),
    ("horizontal", ConstraintWord::Horizontal),
    ("vertical", ConstraintWord::Vertical),
    ("parallel", ConstraintWord::Parallel),
    ("perpendicular", ConstraintWord::Perpendicular),
    ("tangent", ConstraintWord::Tangent),
    ("smooth", ConstraintWord::Smooth),
    ("equal", ConstraintWord::Equal),
    ("concentric", ConstraintWord::Concentric),
    ("midpoint", ConstraintWord::Midpoint),
    ("symmetric", ConstraintWord::Symmetric),
    ("fix", ConstraintWord::Fix),
];

/// What a dimension measures, as `sketch` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureWord {
    Distance,
    HorizontalDistance,
    VerticalDistance,
    Length,
    Angle,
    Radius,
    Diameter,
    EdgeDistance,
}

/// The words a dimension's `kind` takes.
const MEASURE_WORDS: &[(&str, MeasureWord)] = &[
    ("distance", MeasureWord::Distance),
    ("horizontal_distance", MeasureWord::HorizontalDistance),
    ("vertical_distance", MeasureWord::VerticalDistance),
    ("length", MeasureWord::Length),
    ("angle", MeasureWord::Angle),
    ("radius", MeasureWord::Radius),
    ("diameter", MeasureWord::Diameter),
    ("edge_distance", MeasureWord::EdgeDistance),
];

/// The words a spline's `kind` takes: through fit points (the default)
/// or by control points.
const SPLINE_KINDS: &[(&str, bool)] = &[("through", false), ("control", true)];

/// An operation, as the sketch's tools make it.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// A lone point.
    Point {
        at: [String; 2],
    },
    Line {
        start: At,
        end: At,
    },
    /// From `start` to `end` through `through`, as the Arc tool's three
    /// clicks.
    Arc {
        start: At,
        end: At,
        through: [String; 2],
    },
    Circle {
        center: At,
        diameter: String,
    },
    /// Through fit points, or by control points (`control`), open or
    /// closed: 2 to [`SPLINE_POINTS`] points.
    Spline {
        points: Vec<At>,
        closed: bool,
        control: bool,
    },
    /// Four lines held horizontal and vertical; from the centre, a
    /// construction diagonal with the centre its midpoint. `size` is
    /// the width and the height.
    Rectangle {
        from: RectangleFrom,
        size: [String; 2],
    },
    /// Equal lines with their corners on a construction circle about
    /// `center`.
    Polygon {
        center: At,
        sides: u32,
        size: PolygonSize,
    },
    /// Constraints of `kind` on `items`, as the Constrain list applies
    /// it to a selection: 1 to [`MAX_ITEMS`] items.
    Constraint {
        kind: ConstraintWord,
        items: Vec<ItemRef>,
    },
    /// A dimension of `kind` on its one or two `items`: driving at
    /// `value` (typed text, as the value field takes it), or at what it
    /// measures without one; a reference with `driving` false.
    Dimension {
        kind: MeasureWord,
        items: Vec<ItemRef>,
        value: Option<String>,
        driving: bool,
    },
    /// Points, curves, constraints and dimensions deleted with what
    /// depends on them: 1 to [`MAX_ITEMS`].
    Delete {
        items: Vec<ItemRef>,
    },
    /// A point moved to `to`, the rest following as a drag dropped there.
    Move {
        point: ItemRef,
        to: [String; 2],
    },
}

/// An `edit_sketch` operation as its `op` names it: what it takes
/// besides `op`, those of them required, and whether it takes a `label`
/// and `construction`. The parser and the schema both read it.
struct OpSpec {
    kind: OpKind,
    keys: &'static [&'static str],
    required: &'static [&'static str],
    labelled: bool,
    construction: bool,
}

impl OpSpec {
    const fn new(
        kind: OpKind,
        keys: &'static [&'static str],
        required: &'static [&'static str],
        labelled: bool,
        construction: bool,
    ) -> OpSpec {
        OpSpec {
            kind,
            keys,
            required,
            labelled,
            construction,
        }
    }
}

/// Every operation, in [`Op`]'s order. A point is labelled but isn't a
/// curve, so it takes no `construction`.
#[rustfmt::skip]
const OPS: &[OpSpec] = &[
    OpSpec::new(OpKind::Point, &["at"], &["at"], true, false),
    OpSpec::new(OpKind::Line, &["start", "end"], &["start", "end"], true, true),
    OpSpec::new(OpKind::Arc, &["start", "end", "through"], &["start", "end", "through"], true, true),
    OpSpec::new(OpKind::Circle, &["center", "diameter"], &["center", "diameter"], true, true),
    OpSpec::new(OpKind::Spline, &["points", "closed", "kind"], &["points"], true, true),
    OpSpec::new(OpKind::Rectangle, &["corner", "center", "size"], &["size"], true, true),
    OpSpec::new(OpKind::Polygon, &["center", "sides", "corner", "diameter"], &["center", "sides"], true, true),
    OpSpec::new(OpKind::Constraint, &["kind", "items"], &["kind", "items"], false, false),
    OpSpec::new(OpKind::Dimension, &["kind", "items", "value", "driving"], &["kind", "items"], false, false),
    OpSpec::new(OpKind::Delete, &["items"], &["items"], false, false),
    OpSpec::new(OpKind::Move, &["point", "to"], &["point", "to"], false, false),
];

/// The operations' words, for errors: "point, line, ... or move".
fn op_words() -> String {
    let (last, rest) = OPS.split_last().expect("there are operations");
    let rest: Vec<&str> = rest.iter().map(|spec| spec.kind.word()).collect();
    format!("{} or {}", rest.join(", "), last.kind.word())
}

/// An [`Op`]'s kind, which [`OPS`] lists with what it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpKind {
    Point,
    Line,
    Arc,
    Circle,
    Spline,
    Rectangle,
    Polygon,
    Constraint,
    Dimension,
    Delete,
    Move,
}

impl OpKind {
    /// Its name, as `op` gives it.
    fn word(self) -> &'static str {
        match self {
            OpKind::Point => "point",
            OpKind::Line => "line",
            OpKind::Arc => "arc",
            OpKind::Circle => "circle",
            OpKind::Spline => "spline",
            OpKind::Rectangle => "rectangle",
            OpKind::Polygon => "polygon",
            OpKind::Constraint => "constraint",
            OpKind::Dimension => "dimension",
            OpKind::Delete => "delete",
            OpKind::Move => "move",
        }
    }
}

impl Op {
    /// The operation's kind.
    fn kind(&self) -> OpKind {
        match self {
            Op::Point { .. } => OpKind::Point,
            Op::Line { .. } => OpKind::Line,
            Op::Arc { .. } => OpKind::Arc,
            Op::Circle { .. } => OpKind::Circle,
            Op::Spline { .. } => OpKind::Spline,
            Op::Rectangle { .. } => OpKind::Rectangle,
            Op::Polygon { .. } => OpKind::Polygon,
            Op::Constraint { .. } => OpKind::Constraint,
            Op::Dimension { .. } => OpKind::Dimension,
            Op::Delete { .. } => OpKind::Delete,
            Op::Move { .. } => OpKind::Move,
        }
    }

    /// The operation's name, as `op` gives it.
    pub fn word(&self) -> &'static str {
        self.kind().word()
    }
}

impl Arguments<'_> {
    /// `edit_sketch`'s `operations`, required: 1 to [`MAX_OPERATIONS`],
    /// labels unique.
    pub(super) fn operations(&self) -> Result<Vec<SketchOp>, CallError> {
        let items = match self.map.get("operations") {
            None | Some(Value::Null) => return Err(self.error("operations is required")),
            Some(Value::Array(items)) if items.is_empty() => {
                return Err(self.error("operations is empty: give at least one"));
            }
            Some(Value::Array(items)) if items.len() > MAX_OPERATIONS => {
                return Err(self.error(format_args!(
                    "operations has more than {MAX_OPERATIONS}: split the edit into several calls"
                )));
            }
            Some(Value::Array(items)) => items,
            Some(_) => return Err(self.error("operations is a list of operations")),
        };
        let mut operations: Vec<SketchOp> = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let tool = format!("{}: operations[{index}]", self.tool);
            let Value::Object(map) = item else {
                return Err(CallError::Arguments {
                    at: tool,
                    message: "an operation is an object with its op".to_owned(),
                });
            };
            let op = Arguments { tool: &tool, map };
            let read = op.operation()?;
            if let Some(label) = &read.label
                && operations
                    .iter()
                    .any(|earlier| earlier.label.as_ref() == Some(label))
            {
                return Err(op.error(format_args!(
                    "the label {:?} is given already: labels are unique in the call",
                    crate::shown(label)
                )));
            }
            operations.push(read);
        }
        Ok(operations)
    }

    /// One operation: `{"op": word, ...}`.
    fn operation(&self) -> Result<SketchOp, CallError> {
        let word = match self.map.get("op") {
            Some(Value::String(word)) => word.as_str(),
            _ => return Err(self.error(format_args!("op is required: {}", op_words()))),
        };
        let Some(spec) = OPS.iter().find(|spec| spec.kind.word() == word) else {
            return Err(self.error(format_args!(
                "no op named {:?}: {}",
                crate::shown(word),
                op_words()
            )));
        };
        self.only_where(|key| {
            key == "op"
                || spec.keys.contains(&key)
                || (spec.labelled && key == "label")
                || (spec.construction && key == "construction")
        })?;
        let label = match self.map.get("label") {
            None | Some(Value::Null) => None,
            Some(Value::String(label)) => Some(self.label(label)?),
            Some(_) => return Err(self.error("label is a string")),
        };
        let construction = self.flag("construction", false)?;
        let op = match spec.kind {
            OpKind::Point => Op::Point {
                at: self.coordinates("at")?,
            },
            OpKind::Line => Op::Line {
                start: self.at("start")?,
                end: self.at("end")?,
            },
            OpKind::Arc => Op::Arc {
                start: self.at("start")?,
                end: self.at("end")?,
                through: self.coordinates("through")?,
            },
            OpKind::Circle => Op::Circle {
                center: self.at("center")?,
                diameter: self.sized("diameter")?,
            },
            OpKind::Spline => {
                const WHAT: &str = "points: [x, y], an id or a label";
                let said = || self.error(format_args!("points is a list of {WHAT}"));
                let points = (self.required_list("points", 2, SPLINE_POINTS, WHAT)?)
                    .iter()
                    .map(|point| at_of(point).ok_or_else(said))
                    .collect::<Result<_, _>>()?;
                Op::Spline {
                    points,
                    closed: self.flag("closed", false)?,
                    control: self.choice("kind", SPLINE_KINDS)?,
                }
            }
            OpKind::Rectangle => {
                let from = match (
                    self.map.contains_key("corner"),
                    self.map.contains_key("center"),
                ) {
                    (true, false) => RectangleFrom::Corner(self.at("corner")?),
                    (false, true) => RectangleFrom::Center(self.at("center")?),
                    _ => return Err(self.error("give corner or center, one of them")),
                };
                Op::Rectangle {
                    from,
                    size: self.coordinates("size")?,
                }
            }
            OpKind::Polygon => {
                let size = match (
                    self.map.contains_key("corner"),
                    self.map.contains_key("diameter"),
                ) {
                    (true, false) => PolygonSize::Corner(self.at("corner")?),
                    (false, true) => PolygonSize::Diameter(self.sized("diameter")?),
                    _ => return Err(self.error("give corner or diameter, one of them")),
                };
                let sides = (self.map.get("sides"))
                    .and_then(id)
                    .filter(|sides| (MIN_SIDES..=MAX_SIDES).contains(sides))
                    .ok_or_else(|| {
                        self.error(format_args!(
                            "sides is required: a whole number from {MIN_SIDES} to {MAX_SIDES}"
                        ))
                    })?;
                Op::Polygon {
                    center: self.at("center")?,
                    sides,
                    size,
                }
            }
            OpKind::Constraint => Op::Constraint {
                kind: self.required_choice("kind", CONSTRAINT_WORDS)?,
                items: self.items("items", 1, MAX_ITEMS)?,
            },
            OpKind::Dimension => {
                let driving = self.flag("driving", true)?;
                let value = self.optional_text("value")?;
                if !driving && value.is_some() {
                    return Err(self
                        .error("a reference dimension takes no value: it measures what's there"));
                }
                Op::Dimension {
                    kind: self.required_choice("kind", MEASURE_WORDS)?,
                    items: self.items("items", 1, 2)?,
                    value,
                    driving,
                }
            }
            OpKind::Delete => Op::Delete {
                items: self.items("items", 1, MAX_ITEMS)?,
            },
            OpKind::Move => {
                let point = (self.map.get("point"))
                    .and_then(item_ref)
                    .ok_or_else(|| self.error("point is required: an id or a label"))?;
                Op::Move {
                    point,
                    to: self.coordinates("to")?,
                }
            }
        };
        debug_assert_eq!(op.kind(), spec.kind);
        Ok(SketchOp {
            label,
            construction,
            op,
        })
    }

    /// A label: not empty, at most [`LABEL_BYTES`], no `.` (which names
    /// a part).
    fn label(&self, label: &str) -> Result<String, CallError> {
        if label.trim().is_empty() {
            return Err(self.error("label is empty"));
        }
        if label.len() > LABEL_BYTES {
            return Err(self.error(format_args!("label is longer than {LABEL_BYTES} bytes")));
        }
        if RESERVED.contains(&label) {
            return Err(self.error(format_args!(
                "{label:?} names the sketch's own: give another label"
            )));
        }
        if label.contains('.') {
            return Err(self
                .error("a label has no \".\": it names a part of what's labelled (\"l.start\")"));
        }
        Ok(label.to_owned())
    }

    /// A point `[x, y]`, required: typed text or numbers.
    fn coordinates(&self, key: &str) -> Result<[String; 2], CallError> {
        (self.map.get(key)).and_then(pair).ok_or_else(|| {
            self.error(format_args!(
                "{key} is required: [x, y], lengths (\"5 mm\", or numbers in the design's units)"
            ))
        })
    }

    /// Where a shape's point goes, required: `[x, y]`, an id or a label.
    fn at(&self, key: &str) -> Result<At, CallError> {
        (self.map.get(key)).and_then(at_of).ok_or_else(|| {
            self.error(format_args!(
                "{key} is required: [x, y], or a point of the sketch's by id or label"
            ))
        })
    }

    /// A size, required: typed text, or a number taken as its text.
    fn sized(&self, key: &str) -> Result<String, CallError> {
        match self.map.get(key) {
            Some(value @ Value::Number(_)) => {
                coordinate(value).ok_or_else(|| self.error(format_args!("{key} is a length")))
            }
            _ => self.text(key),
        }
    }

    /// A list of items `key`, ids or labels, `least` to `most`.
    fn items(&self, key: &str, least: usize, most: usize) -> Result<Vec<ItemRef>, CallError> {
        const WHAT: &str = "items, each an id or a label";
        let said = || self.error(format_args!("{key} is a list of {WHAT}"));
        (self.required_list(key, least, most, WHAT)?)
            .iter()
            .map(|item| item_ref(item).ok_or_else(said))
            .collect()
    }
}

/// `[x, y]`, each typed text or a finite number.
fn pair(value: &Value) -> Option<[String; 2]> {
    match value {
        Value::Array(pair) if pair.len() == 2 => {
            Some([coordinate(&pair[0])?, coordinate(&pair[1])?])
        }
        _ => None,
    }
}

/// An id, a whole number that fits a `u32`, or a label (with its part)
/// at most twice [`LABEL_BYTES`] and not empty.
fn item_ref(value: &Value) -> Option<ItemRef> {
    match value {
        Value::Number(_) => id(value).map(ItemRef::Id),
        Value::String(label) if !label.trim().is_empty() && label.len() <= 2 * LABEL_BYTES => {
            Some(ItemRef::Label(label.clone()))
        }
        _ => None,
    }
}

/// `[x, y]` for a new point, or a point by id or label.
fn at_of(value: &Value) -> Option<At> {
    match value {
        Value::Array(_) => pair(value).map(At::Point),
        _ => item_ref(value).map(At::Item),
    }
}

/// The `edit_sketch` tool's schema.
pub(super) fn edit_sketch_tool() -> Value {
    json!({
        "name": "edit_sketch",
        "title": "Draw in a sketch",
        "description": "Edits a sketch by a list of operations, in order, as its tools make \
            them: points, lines, arcs, circles, splines, rectangles and polygons, \
            constraints, dimensions, deleting, and moving points. A point or curve may carry \
            a label, local to the call, which later operations name it by, as they name the \
            sketch's own by id (as sketch lists them) and its origin and axes as \"origin\", \
            \"x_axis\" and \"y_axis\": a line's ends \"l.start\" and \"l.end\", an \
            arc's \"a.start\" and \"a.end\" (the points given so) and \"a.center\", a circle's centre \
            \"c.center\", a spline's points \"s.point1\"... (\"s.start\", \"s.end\"), a \
            rectangle's sides \"r.side1\" to \"r.side4\" (from its corner along x, then on \
            round) and corners \"r.corner1\" to \"r.corner4\" (and \"r.center\" drawn from \
            it), a polygon's \"p.side1\"..., \"p.corner1\"..., \"p.center\" and \
            \"p.circle\". A shape's point given as an id or label is that point, shared; as \
            [x, y] a new point. Coordinates and sizes are lengths in the sketch's own \
            coordinates (\"5 mm\", or numbers in the design's units); dimension values typed \
            text as the value field takes them (\"20 mm\", \"width / 2\", \"30 deg\"). It's \
            solved as the user's edits are; a dimension that would over-constrain the sketch \
            is placed as a reference instead, which the result says. One undo step; the \
            result maps each label to the ids it got and gives the degrees of freedom left. \
            Errors name the operation by its index, from 0.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sketch": sketch(),
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_OPERATIONS,
                    "items": { "oneOf": operation_schemas() }
                }
            },
            "required": ["sketch", "operations"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The schema of each operation.
fn operation_schemas() -> Vec<Value> {
    let label = json!({
        "type": "string",
        "maxLength": LABEL_BYTES,
        "pattern": "^[^.]+$",
        "description": "What later operations name it by"
    });
    let construction =
        json!({ "type": "boolean", "description": "Construction geometry; default false" });
    let coordinates = json!({
        "type": "array",
        "items": { "type": ["string", "number"] },
        "minItems": 2,
        "maxItems": 2,
        "description": "[x, y]"
    });
    let item = json!({
        "type": ["integer", "string"],
        "description": "An id, as sketch lists it, a label (\"l\", \"l.start\"), or \"origin\", \"x_axis\" or \"y_axis\""
    });
    let at = json!({
        "description": "[x, y] for a new point, or a point by id or label",
        "oneOf": [coordinates.clone(), item.clone()]
    });
    let size = json!({ "type": ["string", "number"], "description": "A length" });
    let items = |least: usize, most: usize| json!({ "type": "array", "items": item.clone(), "minItems": least, "maxItems": most });
    // The schema of `key` as `word` takes it.
    let property = |word: &str, key: &str| -> Value {
        match (word, key) {
            ("point" | "arc", "at" | "through") | (_, "to") => coordinates.clone(),
            (_, "start" | "end" | "center" | "corner") => at.clone(),
            (_, "diameter") => size.clone(),
            ("spline", "points") => {
                json!({ "type": "array", "items": at.clone(), "minItems": 2, "maxItems": SPLINE_POINTS })
            }
            ("spline", "closed") => json!({ "type": "boolean", "description": "default false" }),
            ("spline", "kind") => {
                json!({ "enum": words(SPLINE_KINDS), "description": "Through fit points (default) or by control points" })
            }
            ("rectangle", "size") => {
                json!({ "type": "array", "items": size.clone(), "minItems": 2, "maxItems": 2, "description": "[width, height], from a corner either way by sign" })
            }
            ("polygon", "sides") => {
                json!({ "type": "integer", "minimum": MIN_SIDES, "maximum": MAX_SIDES })
            }
            ("constraint", "kind") => json!({ "enum": words(CONSTRAINT_WORDS) }),
            ("dimension", "kind") => json!({ "enum": words(MEASURE_WORDS) }),
            ("dimension", "items") => items(1, 2),
            (_, "items") => items(1, MAX_ITEMS),
            ("dimension", "value") => {
                json!({ "type": "string", "maxLength": TEXT_BYTES, "description": "As typed in the value field; without it, what it measures now" })
            }
            ("dimension", "driving") => {
                json!({ "type": "boolean", "description": "default true; false for a reference, which takes no value" })
            }
            ("move", "point") => item.clone(),
            _ => unreachable!("{word} takes no {key}"),
        }
    };
    // An operation's schema: its `op`, its keys, those required, and a
    // label and construction where it takes them.
    let one = |spec: &OpSpec| {
        let mut properties: Map<String, Value> = Map::new();
        properties.insert("op".into(), json!({ "const": spec.kind.word() }));
        for key in spec.keys {
            properties.insert((*key).into(), property(spec.kind.word(), key));
        }
        if spec.labelled {
            properties.insert("label".into(), label.clone());
        }
        if spec.construction {
            properties.insert("construction".into(), construction.clone());
        }
        let mut required: Vec<&str> = spec.required.to_vec();
        required.insert(0, "op");
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    };
    OPS.iter().map(one).collect()
}
