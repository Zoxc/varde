//! The tools: the static `tools/list` of what's built, a `tools/call`'s
//! params read into a checked [`Call`], and the [`ToolReply`] a call is
//! answered with.
//!
//! A call naming no tool of ours is a JSON-RPC error (-32602); arguments
//! that don't fit its tool are a tool result with `isError`, as MCP has
//! input validation errors, so the model reads why and tries again.
//! Everything in the arguments comes off the network: names bounded,
//! directions finite, not zero and normalized, offsets and limits
//! converted with `usize::try_from` and limits capped.

use crate::shown;
use serde_json::{Map, Value, json};

mod draw;
pub use draw::{
    At, ConstraintWord, EditSketch, ItemRef, LABEL_BYTES, MAX_OPERATIONS, MAX_SIDES, MIN_SIDES,
    MeasureWord, Op, PolygonSize, RESERVED, RectangleFrom, SPLINE_POINTS, SketchOp,
};

/// The longest name taken (a feature's, a body's), in bytes: as long as
/// the longest a design may hold (`varde_document::MAX_NAME_LEN`), so
/// every feature and body can be named.
pub const NAME_BYTES: usize = 1024;

/// `design_summary`'s default `limit`, per listing: a page of each of
/// its three listings fits under [`ToolReply::CAP`] even with parameters
/// at their longest (about 400 bytes each), names of usual length.
pub const SUMMARY_LIMIT: usize = 30;

/// `body_topology`'s default `limit`, per listing: a page of each of its
/// three listings, at about 850 bytes for a face, an edge and a corner
/// together, fits well under [`ToolReply::CAP`].
pub const LIMIT: usize = 16;

/// `feature`'s default `limit`, per list in a feature's settings (a
/// chamfer's edges and faces, a loft's sections and rails, ...): at
/// about 120 bytes for an edge's key, a page of one fits well under
/// [`ToolReply::CAP`]; the app shortens the pages past that (several
/// long lists, names at their longest) rather than cut the reply.
pub const FEATURE_LIMIT: usize = 50;

/// `sketch`'s default `limit`, per listing: a page of each of its five
/// listings (points, curves, constraints, dimensions, regions), at about
/// 500 bytes for one of each, fits under [`ToolReply::CAP`].
pub const SKETCH_LIMIT: usize = 20;

/// The largest `limit` taken; more is cut down to it.
pub const MAX_LIMIT: usize = 500;

/// A `tools/call`, checked. The largest arguments, an align's, a
/// scale's and a sweep's, are boxed to keep every call small.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// The design's units, bodies, Timeline and parameters, each
    /// listing paged alone.
    DesignSummary(Page),
    /// One feature's settings, by its Timeline name, each list in them
    /// paged alone.
    Feature { name: String, page: Page },
    /// A body's faces, edges and corners.
    BodyTopology(Topology),
    /// A sketch's geometry, constraints, dimensions, regions and
    /// degrees of freedom, each listing paged alone.
    Sketch(SketchQuery),
    /// What the measure tool measures of one item, or between two.
    Measure(Measure),
    /// A picture of the model, PNG.
    Render(Render),
    /// Renames a feature or a body: an edit, as are those below.
    Rename(Rename),
    /// Shows or hides a body.
    SetVisible { body: String, visible: bool },
    /// Gives a body a colour of its own, or the theme's back with `None`.
    SetColor { body: String, color: Option<Color> },
    /// Sets how opaque a body is drawn, in [`MIN_OPACITY`]`..=1`.
    SetOpacity { body: String, opacity: f64 },
    /// Removes a feature and the bodies it makes, if nothing else uses it.
    RemoveFeature { feature: String },
    /// Removes a body with the feature making it, if nothing else uses it.
    RemoveBody { body: String },
    /// Adds a parameter, `name = value`.
    AddParameter { name: String, value: String },
    /// Sets a parameter's expression.
    SetParameter { name: String, value: String },
    /// Renames a parameter, rewriting its uses.
    RenameParameter { name: String, to: String },
    /// Removes a parameter nothing uses.
    RemoveParameter { name: String },
    /// Changes the design's units.
    SetUnits(Units),
    /// Changes the design's tolerance, as typed text (`"1 µm"`).
    SetTolerance { tolerance: String },
    /// Moves the Timeline's rollback marker to before the feature named,
    /// or to the end with `None`.
    SetRollback { before: Option<String> },
    /// Adds an empty sketch on an origin plane or a flat face, at the
    /// rollback marker.
    AddSketch {
        plane: Reference<WorldPlane>,
        /// Its name, at most [`NAME_BYTES`]; the default ("Sketch 2")
        /// if not given.
        name: Option<String>,
    },
    /// Puts a sketch on another origin plane or flat face, its drawing
    /// kept as it is in its own coordinates.
    SetSketchPlane {
        sketch: String,
        plane: Reference<WorldPlane>,
    },
    /// Adds a link to a sketch: model geometry or another sketch's
    /// projected into it, or the model cut with its plane.
    AddLink(AddLink),
    /// Edits a sketch by a list of operations: drawing, constraints,
    /// dimensions, deleting and moving.
    EditSketch(EditSketch),
    /// Changes a feature's settings by a patch in its tool's shape.
    SetFeature(SetFeature),
    /// Adds an extrude, at the rollback marker.
    Extrude(Extrude),
    /// Adds a revolve, at the rollback marker.
    Revolve(Revolve),
    /// Adds a combine, at the rollback marker.
    Combine(Combine),
    /// Adds a move of bodies, at the rollback marker.
    MoveBody(MoveBody),
    /// Adds a mirror of bodies, at the rollback marker.
    Mirror(Mirror),
    /// Adds a linear or circular pattern of bodies, at the rollback
    /// marker.
    Pattern(Pattern),
    /// Adds an align of a body, at the rollback marker.
    Align(Box<Align>),
    /// Adds a scale of bodies, at the rollback marker.
    Scale(Box<Scale>),
    /// Adds a split of a body, at the rollback marker.
    Split(Split),
    /// Adds a shell of a body, at the rollback marker.
    Shell(Shell),
    /// Adds an offset of faces, at the rollback marker.
    OffsetFace(OffsetFace),
    /// Adds a draft of faces, at the rollback marker.
    Draft(Draft),
    /// Adds a chamfer of edges, at the rollback marker.
    Chamfer(Chamfer),
    /// Adds a fillet of edges, at the rollback marker.
    Fillet(Fillet),
    /// Adds a sweep of regions of a sketch, at the rollback marker.
    Sweep(Box<Sweep>),
    /// Adds a loft through sections, at the rollback marker.
    Loft(Loft),
    /// Takes back the session's own newest edits, `steps` steps of the
    /// design's history (1 by default, at most [`MAX_UNDO_STEPS`]), as the
    /// user's Undo would: refused if any of them isn't the session's.
    Undo { steps: usize },
}

impl Call {
    /// The tool's name, as listed.
    pub fn tool(&self) -> &'static str {
        match self {
            Call::DesignSummary(_) => "design_summary",
            Call::Feature { .. } => "feature",
            Call::BodyTopology(_) => "body_topology",
            Call::Sketch(_) => "sketch",
            Call::Measure(_) => "measure",
            Call::Render(_) => "render",
            Call::Rename(_) => "rename",
            Call::SetVisible { .. } => "set_visible",
            Call::SetColor { .. } => "set_color",
            Call::SetOpacity { .. } => "set_opacity",
            Call::RemoveFeature { .. } => "remove_feature",
            Call::RemoveBody { .. } => "remove_body",
            Call::AddParameter { .. } => "add_parameter",
            Call::SetParameter { .. } => "set_parameter",
            Call::RenameParameter { .. } => "rename_parameter",
            Call::RemoveParameter { .. } => "remove_parameter",
            Call::SetUnits(_) => "set_units",
            Call::SetTolerance { .. } => "set_tolerance",
            Call::SetRollback { .. } => "set_rollback",
            Call::AddSketch { .. } => "add_sketch",
            Call::SetSketchPlane { .. } => "set_sketch_plane",
            Call::AddLink(_) => "add_link",
            Call::EditSketch(_) => "edit_sketch",
            Call::SetFeature(_) => "set_feature",
            Call::Extrude(_) => "extrude",
            Call::Revolve(_) => "revolve",
            Call::Combine(_) => "combine",
            Call::MoveBody(_) => "move_body",
            Call::Mirror(_) => "mirror",
            Call::Pattern(_) => "pattern",
            Call::Align(_) => "align",
            Call::Scale(_) => "scale",
            Call::Split(_) => "split",
            Call::Shell(_) => "shell",
            Call::OffsetFace(_) => "offset_face",
            Call::Draft(_) => "draft",
            Call::Chamfer(_) => "chamfer",
            Call::Fillet(_) => "fillet",
            Call::Sweep(_) => "sweep",
            Call::Loft(_) => "loft",
            Call::Undo { .. } => "undo",
        }
    }

    /// The names a feature tool's call asks for what it makes.
    pub fn naming(&self) -> Option<&Naming> {
        Some(match self {
            Call::Extrude(args) => &*args.naming,
            Call::Revolve(args) => &*args.naming,
            Call::Combine(args) => &*args.naming,
            Call::MoveBody(args) => &*args.naming,
            Call::Mirror(args) => &*args.naming,
            Call::Pattern(args) => &*args.naming,
            Call::Align(args) => &*args.naming,
            Call::Scale(args) => &*args.naming,
            Call::Split(args) => &*args.naming,
            Call::Shell(args) => &*args.naming,
            Call::OffsetFace(args) => &*args.naming,
            Call::Draft(args) => &*args.naming,
            Call::Chamfer(args) => &*args.naming,
            Call::Fillet(args) => &*args.naming,
            Call::Sweep(args) => &*args.naming,
            Call::Loft(args) => &*args.naming,
            _ => return None,
        })
    }

    /// The name a creating tool's call asks for what it adds: a
    /// sketch's or a feature's.
    pub fn new_name(&self) -> Option<&str> {
        match self {
            Call::AddSketch { name, .. } => name.as_deref(),
            _ => self.naming()?.name.as_deref(),
        }
    }

    /// Whether the call changes the design.
    pub fn edits(&self) -> bool {
        !matches!(
            self,
            Call::DesignSummary(_)
                | Call::Feature { .. }
                | Call::BodyTopology(_)
                | Call::Sketch(_)
                | Call::Measure(_)
                | Call::Render(_)
        )
    }
}

/// The most steps `undo` takes back at once: the user's Undo's own reach,
/// the editor's `MAX_UNDO` (`varde_document`), 200.
pub const MAX_UNDO_STEPS: usize = 200;

/// The longest expression taken (a parameter's value, a tolerance), in
/// bytes: `varde_expr::MAX_LEN`, the longest a field evaluates.
pub const TEXT_BYTES: usize = 256;

/// The faintest opacity taken: `varde_document::Opacity::MIN`, 10 %.
pub const MIN_OPACITY: f64 = 0.1;

/// The most saturated colour taken: `varde_document::Tint::MAX_SATURATION`,
/// 30 %, so a body's colour stays duller than the highlights.
pub const MAX_SATURATION: f64 = 0.3;

/// A body's colour: a hue and a saturation, the lightness the theme's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// Degrees round the colour wheel, in `0..360`.
    pub hue: f64,
    /// In `0..=`[`MAX_SATURATION`].
    pub saturation: f64,
}

/// The design units `set_units` offers: the file menu's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Mm,
    In,
}

/// The words `units` takes, as typed and shown.
const UNITS: &[(&str, Units)] = &[("mm", Units::Mm), ("in", Units::In)];

/// The most profiles, excluded bodies, tools, faces or curves a feature
/// tool takes: `varde_document`'s `MAX_EXTRUDE_REGIONS`,
/// `MAX_REVOLVE_REGIONS`, `MAX_FEATURE_BODIES`, `MAX_SHELL_FACES`,
/// `MAX_OFFSET_FACES`, `MAX_DRAFT_FACES` and `MAX_SPLIT_CURVES`, all
/// 256.
pub const MAX_ITEMS: usize = 256;

/// The words `extent` takes for an extrude, the default first.
const EXTRUDE_EXTENTS: &[(&str, ExtrudeExtent)] = &[
    ("one_side", ExtrudeExtent::OneSide),
    ("symmetric", ExtrudeExtent::Symmetric),
    ("two_sides", ExtrudeExtent::TwoSides),
    ("through_all", ExtrudeExtent::ThroughAll),
];

/// The words `extent` takes for a revolve, the default first.
const REVOLVE_EXTENTS: &[(&str, RevolveExtent)] = &[
    ("full", RevolveExtent::Full),
    ("one_side", RevolveExtent::OneSide),
    ("symmetric", RevolveExtent::Symmetric),
    ("two_sides", RevolveExtent::TwoSides),
];

/// The words an extrude's or revolve's `operation` takes, the default
/// first.
const OPERATIONS: &[(&str, Operation)] = &[
    ("new_body", Operation::NewBody),
    ("join", Operation::Join),
    ("cut", Operation::Cut),
    ("intersect", Operation::Intersect),
];

/// The words a combine's `operation` takes, the default first.
const BODY_OPS: &[(&str, BodyOp)] = &[
    ("union", BodyOp::Union),
    ("subtract", BodyOp::Subtract),
    ("intersect", BodyOp::Intersect),
];

/// The words of `choices`, for a schema's `enum`.
fn words<T>(choices: &[(&str, T)]) -> Value {
    choices.iter().map(|(word, _)| json!(word)).collect()
}

/// The one with `word` among `choices`, by one rule for every argument:
/// trimmed and ignoring ASCII case.
fn lookup<T: Copy>(choices: &[(&str, T)], word: &str) -> Option<T> {
    let word = word.trim();
    (choices.iter())
        .find(|(said, _)| word.eq_ignore_ascii_case(said))
        .map(|&(_, choice)| choice)
}

/// A choice the tools name by a word from a table, which the tool
/// parsers take and the app answers in.
pub trait Worded: Copy + PartialEq + 'static {
    /// The words it's named by, the default first where there is one.
    const WORDS: &'static [(&'static str, Self)];

    /// The word it's named by, as the client gives it.
    fn word(self) -> &'static str {
        (Self::WORDS.iter())
            .find(|(_, choice)| *choice == self)
            .map_or("", |(word, _)| word)
    }
}

/// Implements [`Worded`] for each type by its table.
macro_rules! worded {
    ($($ty:ty => $table:ident),* $(,)?) => {
        $(impl Worded for $ty {
            const WORDS: &'static [(&'static str, Self)] = $table;
        })*
    };
}

worded! {
    Units => UNITS,
    ExtrudeExtent => EXTRUDE_EXTENTS,
    RevolveExtent => REVOLVE_EXTENTS,
    Operation => OPERATIONS,
    BodyOp => BODY_OPS,
    WorldAxis => WORLD_AXES,
    WorldPlane => WORLD_PLANES,
    () => ORIGIN,
    PatternKind => PATTERN_KINDS,
    PatternMode => PATTERN_MODES,
    ScaleMode => SCALE_MODES,
    SplitSide => SPLIT_SIDES,
    SplitKeep => SPLIT_KEEPS,
    ShellDirection => SHELL_DIRECTIONS,
    ChamferType => CHAMFER_TYPES,
    LoftMode => LOFT_MODES,
    LinkKind => LINK_KINDS,
    NamedView => NAMED_VIEWS,
}

/// The regions of a sketch a feature takes.
#[derive(Clone, Debug, PartialEq)]
pub enum Profiles {
    /// Every region of the sketch.
    All,
    /// These, at least one and at most [`MAX_ITEMS`].
    Picked(Vec<Profile>),
}

/// One region of a sketch.
#[derive(Clone, Debug, PartialEq)]
pub enum Profile {
    /// By its index in the sketch's regions, as `sketch` lists them.
    Index(usize),
    /// The region a point is in, in the sketch's own coordinates: two
    /// lengths as typed text (a bare number in the design's units), at
    /// most [`TEXT_BYTES`] each.
    Point([String; 2]),
}

/// What an extrude or a revolve does with its solid: the panel's
/// Operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Operation {
    #[default]
    NewBody,
    Join,
    Cut,
    Intersect,
}

/// How far an extrude goes: the panel's Extent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExtrudeExtent {
    #[default]
    OneSide,
    Symmetric,
    TwoSides,
    /// Only with [`Operation::Cut`].
    ThroughAll,
}

/// `extrude`'s arguments: what the Extrude panel shows, values as
/// typed text of at most [`TEXT_BYTES`], `None` for the panel's default.
#[derive(Clone, Debug, PartialEq)]
pub struct Extrude {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The sketch, by its Timeline name.
    pub sketch: String,
    pub profiles: Profiles,
    pub extent: ExtrudeExtent,
    /// One side's or symmetric's distance, two sides' first.
    pub distance: Option<String>,
    /// Two sides' second distance; the first's if not given.
    pub second_distance: Option<String>,
    pub flip: bool,
    /// The taper angle; none if not given.
    pub taper: Option<String>,
    pub operation: Operation,
    /// The bodies a join, cut or intersect leaves alone, by name.
    pub exclude: Vec<String>,
}

/// How far a revolve turns: the panel's Extent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RevolveExtent {
    #[default]
    Full,
    OneSide,
    Symmetric,
    TwoSides,
}

/// What a revolve turns about.
#[derive(Clone, Debug, PartialEq)]
pub enum Axis {
    /// The sketch's own x axis.
    SketchX,
    /// The sketch's own y axis.
    SketchY,
    /// A line of the sketch, by its id there.
    Line(u32),
    /// A straight model edge in the sketch's plane, by its key from
    /// `body_topology`, at most [`NAME_BYTES`].
    Edge(String),
}

/// `revolve`'s arguments: what the Revolve panel shows, as
/// [`Extrude`]'s.
#[derive(Clone, Debug, PartialEq)]
pub struct Revolve {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    pub sketch: String,
    pub profiles: Profiles,
    pub axis: Axis,
    pub extent: RevolveExtent,
    /// One side's or symmetric's angle, two sides' first.
    pub angle: Option<String>,
    /// Two sides' second angle.
    pub second_angle: Option<String>,
    pub flip: bool,
    pub operation: Operation,
    pub exclude: Vec<String>,
}

/// What a combine does: the panel's Operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BodyOp {
    #[default]
    Union,
    Subtract,
    Intersect,
}

/// `combine`'s arguments: what the Combine panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Combine {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The body kept, by name.
    pub target: String,
    /// The bodies combined into it, by name, at least one and at most
    /// [`MAX_ITEMS`].
    pub tools: Vec<String>,
    pub operation: BodyOp,
    pub keep_tools: bool,
}

/// A world axis through the origin, along its positive side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldAxis {
    X,
    Y,
    Z,
}

/// The words a world axis is named by.
const WORLD_AXES: &[(&str, WorldAxis)] = &[
    ("x", WorldAxis::X),
    ("y", WorldAxis::Y),
    ("z", WorldAxis::Z),
];

/// An origin plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldPlane {
    Xy,
    Xz,
    Yz,
}

/// The words an origin plane is named by.
const WORLD_PLANES: &[(&str, WorldPlane)] = &[
    ("xy", WorldPlane::Xy),
    ("xz", WorldPlane::Xz),
    ("yz", WorldPlane::Yz),
];

/// The word the origin is named by, as a point.
const ORIGIN: &[(&str, ())] = &[("origin", ())];

/// One of the world's a motion refers to: a world axis, an origin
/// plane, or the origin (`()`), with the word it's named by.
pub trait World: Worded {}

impl World for WorldAxis {}

impl World for WorldPlane {}

impl World for () {}

/// What a key from `body_topology` names, as the argument wraps it:
/// `{"face": key}`, `{"edge": key}` or `{"corner": key}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Face,
    Edge,
    Corner,
}

impl ItemKind {
    /// The word it's wrapped in.
    pub fn word(self) -> &'static str {
        match self {
            ItemKind::Face => "face",
            ItemKind::Edge => "edge",
            ItemKind::Corner => "corner",
        }
    }
}

/// A face, edge or corner of the model, by its key from
/// `body_topology`, at most [`NAME_BYTES`]; which the app checks the key
/// names.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub key: String,
}

/// What a motion refers to: one of the world's (an axis, a plane, the
/// origin), or an item of the model by key.
#[derive(Clone, Debug, PartialEq)]
pub enum Reference<W> {
    World(W),
    Key(Item),
}

/// `move_body`'s arguments: what the Move panel shows, values as typed
/// text of at most [`TEXT_BYTES`], `None` for the panel's default.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveBody {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The bodies moved, by name, at least one and at most
    /// [`MAX_ITEMS`].
    pub bodies: Vec<String>,
    /// The offsets along world X, Y and Z: none, "0 mm".
    pub offset: [Option<String>; 3],
    /// The angle turned about `axis`, before the offsets: none, 0°.
    pub angle: Option<String>,
    /// A world axis, or a straight or round edge or a round face: none,
    /// the Z axis.
    pub axis: Option<Reference<WorldAxis>>,
}

/// `mirror`'s arguments: what the Mirror panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Mirror {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    pub bodies: Vec<String>,
    /// An origin plane or a flat face.
    pub plane: Reference<WorldPlane>,
    /// The panel's Create copy: on unless given.
    pub keep_original: bool,
}

/// A pattern's kind: the panel's Linear or Circular pattern.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PatternKind {
    #[default]
    Linear,
    Circular,
}

/// The words `kind` takes, the default first.
const PATTERN_KINDS: &[(&str, PatternKind)] = &[
    ("linear", PatternKind::Linear),
    ("circular", PatternKind::Circular),
];

/// How a pattern's copies are spread: the panel's mode tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternMode {
    /// `spread` is one step.
    Spacing,
    /// `spread` is from the first copy to the last.
    Total,
    /// A circular one's copies evenly round a full turn.
    Full,
}

/// The words `mode` takes.
const PATTERN_MODES: &[(&str, PatternMode)] = &[
    ("spacing", PatternMode::Spacing),
    ("total", PatternMode::Total),
    ("full", PatternMode::Full),
];

/// `pattern`'s arguments: what the Linear or Circular pattern panel
/// shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    pub kind: PatternKind,
    pub bodies: Vec<String>,
    /// The direction or axis: none, the X axis (linear) or the Z axis.
    pub axis: Option<Reference<WorldAxis>>,
    /// How many, the original among them, as typed (a number sent is
    /// its text): none, 3 (linear) or 4.
    pub count: Option<String>,
    /// None, spacing (linear) or full.
    pub mode: Option<PatternMode>,
    /// The Spacing or Total field: a length, or a circular one's angle.
    pub spread: Option<String>,
    /// A linear one's Flip direction.
    pub flip: bool,
    /// Join to original: on unless given.
    pub join: bool,
}

/// One side of an align: its point and directions.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignSide {
    /// The origin, a corner, or an edge (a straight one's middle, a
    /// round one's centre).
    pub point: Reference<()>,
    /// A world axis, a face (a flat one's normal, a round one's axis) or
    /// an edge (a straight one's direction, a round one's axis).
    pub direction: Option<Reference<WorldAxis>>,
    pub second_direction: Option<Reference<WorldAxis>>,
}

/// `align`'s arguments: what the Align panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Align {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The body moved, by name: none, the one `from`'s point is on.
    pub body: Option<String>,
    /// On the body moved.
    pub from: AlignSide,
    /// What it's aligned to.
    pub to: AlignSide,
    pub flip: bool,
    /// The offset along `to`'s direction: none, 0 mm.
    pub distance: Option<String>,
    /// The turn about it: none, 0°.
    pub angle: Option<String>,
}

/// How a scale scales: the panel's mode tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScaleMode {
    #[default]
    Uniform,
    PerAxis,
    EdgeLength,
}

/// The words a scale's `mode` takes, the default first.
const SCALE_MODES: &[(&str, ScaleMode)] = &[
    ("uniform", ScaleMode::Uniform),
    ("per_axis", ScaleMode::PerAxis),
    ("edge_length", ScaleMode::EdgeLength),
];

/// `scale`'s arguments: what the Scale panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    pub bodies: Vec<String>,
    /// The origin, a corner or an edge (as an align's point): none, the
    /// origin.
    pub point: Option<Reference<()>>,
    pub mode: ScaleMode,
    /// Uniform's factor: none, 1.
    pub factor: Option<String>,
    /// Per axis's X, Y and Z factors: none, 1.
    pub factors: [Option<String>; 3],
    /// Edge length's edge, by key.
    pub edge: Option<String>,
    /// Edge length's length.
    pub length: Option<String>,
    /// Edge length's Along its axis only.
    pub axis_only: bool,
}

/// What a split cuts its body with: the panel's Split with tiles.
#[derive(Clone, Debug, PartialEq)]
pub enum SplitWith {
    /// Face: an origin plane, or a face by key, flat as its plane,
    /// curved as its surface continued past the body.
    Surface(Reference<WorldPlane>),
    /// Body: another body, by name, kept.
    Body(String),
    /// Region: regions of a sketch, through the body both ways.
    Regions { sketch: String, profiles: Profiles },
    /// Line: the curves of one open line of a sketch, by their ids
    /// there, 1 to [`MAX_ITEMS`] (the document's `MAX_SPLIT_CURVES`).
    Line { sketch: String, curves: Vec<u32> },
}

/// A split's side: the part inside the tool (front) or the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitSide {
    #[default]
    Front,
    Back,
}

/// The words a split's `original` takes, the default first.
const SPLIT_SIDES: &[(&str, SplitSide)] = &[("front", SplitSide::Front), ("back", SplitSide::Back)];

/// Which of a split's pieces stay: the panel's Keep tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitKeep {
    #[default]
    Both,
    Front,
    Back,
}

/// The words a split's `keep` takes, the default first.
const SPLIT_KEEPS: &[(&str, SplitKeep)] = &[
    ("both", SplitKeep::Both),
    ("front", SplitKeep::Front),
    ("back", SplitKeep::Back),
];

/// `split`'s arguments: what the Split panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The body split, by name.
    pub body: String,
    pub tool: SplitWith,
    /// The panel's Keeps: which piece keeps the body's id while both are
    /// kept; none, the front.
    pub original: Option<SplitSide>,
    pub keep: SplitKeep,
}

/// Which way a shell's walls grow: the panel's Direction tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellDirection {
    #[default]
    Inward,
    Outward,
}

/// The words a shell's `direction` takes, the default first.
const SHELL_DIRECTIONS: &[(&str, ShellDirection)] = &[
    ("inward", ShellDirection::Inward),
    ("outward", ShellDirection::Outward),
];

/// `shell`'s arguments: what the Shell panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Shell {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The body hollowed, by name, given with no faces only (a shell's
    /// body is its faces'): none, the faces' body, or the model's only
    /// body.
    pub body: Option<String>,
    /// The faces removed, by key, at most [`MAX_ITEMS`]: none, a closed
    /// hollow body.
    pub faces: Vec<String>,
    /// None, 2 mm.
    pub thickness: Option<String>,
    pub direction: ShellDirection,
}

/// `offset_face`'s arguments: what the Offset face panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetFace {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The faces moved, by key, 1 to [`MAX_ITEMS`].
    pub faces: Vec<String>,
    /// None, 1 mm.
    pub distance: Option<String>,
    /// Into the body rather than out.
    pub inward: bool,
    /// Tangent faces: on unless given.
    pub tangent: bool,
}

/// `draft`'s arguments: what the Draft panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The faces drafted, by key, 1 to [`MAX_ITEMS`].
    pub faces: Vec<String>,
    /// An origin plane or a flat face: none, XY.
    pub neutral: Option<Reference<WorldPlane>>,
    /// None, 3°.
    pub angle: Option<String>,
    /// The pull against the neutral plane's normal.
    pub flip: bool,
    /// Tangent faces: on unless given.
    pub tangent: bool,
}

/// How a chamfer is sized: the panel's Type tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChamferType {
    /// One distance along both faces.
    #[default]
    Equal,
    /// A distance along each face.
    TwoDistances,
    /// A distance along the first face and the cut's angle to it.
    DistanceAngle,
}

/// The words a chamfer's `type` takes, the default first.
const CHAMFER_TYPES: &[(&str, ChamferType)] = &[
    ("equal", ChamferType::Equal),
    ("two_distances", ChamferType::TwoDistances),
    ("distance_angle", ChamferType::DistanceAngle),
];

/// `chamfer`'s arguments: what the Chamfer panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Chamfer {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The edges, by key, at most [`MAX_ITEMS`]; with `faces`, at least
    /// one between them.
    pub edges: Vec<String>,
    /// Faces standing for the edges round them, by key, at most
    /// [`MAX_ITEMS`].
    pub faces: Vec<String>,
    pub kind: ChamferType,
    /// None, 1 mm.
    pub distance: Option<String>,
    /// Two distances' second: none, 2 mm.
    pub second_distance: Option<String>,
    /// Distance and angle's angle: none, 45°.
    pub angle: Option<String>,
    /// Flip sides: the first face is the other one.
    pub flip: bool,
    /// Tangent chain: each edge with the edges running on smoothly from
    /// it; on unless given.
    pub tangent: bool,
}

/// `fillet`'s arguments: what the Fillet panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Fillet {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The edges, by key, as a chamfer's.
    pub edges: Vec<String>,
    /// Faces standing for the edges round them, as a chamfer's.
    pub faces: Vec<String>,
    /// None, 2 mm.
    pub radius: Option<String>,
    /// Tangent chain, as a chamfer's: on unless given.
    pub tangent: bool,
}

/// A chain of a sketch's curves, named by one of them: a sweep's path's
/// part or a loft's rail, as a click on the curve picks it.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveIn {
    /// The sketch, by its Timeline name.
    pub sketch: String,
    /// One of the chain's curves, by its id in the sketch.
    pub curve: u32,
}

/// What a sweep runs along: the panel's Along tiles.
#[derive(Clone, Debug, PartialEq)]
pub enum SweepAlong {
    /// Path: chains of sketch curves and model edges, at least one of
    /// them, each list at most [`MAX_ITEMS`].
    Path {
        curves: Vec<CurveIn>,
        /// Model edges, by key, all on one body.
        edges: Vec<String>,
        /// Tangent chain: each edge with the edges running on smoothly
        /// from it; on unless given.
        tangent: bool,
    },
    /// Helix: about an axis, with its pitch and turns.
    Helix {
        axis: Reference<WorldAxis>,
        /// None, 10 mm.
        pitch: Option<String>,
        /// None, 5.
        turns: Option<String>,
        left_handed: bool,
        flip: bool,
    },
}

/// `sweep`'s arguments: what the Sweep panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// The profile's sketch, by its Timeline name.
    pub sketch: String,
    pub profiles: Profiles,
    pub along: SweepAlong,
    /// A path's Keep orientation.
    pub keep_orientation: bool,
    /// A path's twist: none, 0°.
    pub twist: Option<String>,
    pub operation: Operation,
    pub exclude: Vec<String>,
}

/// What a loft's section is.
#[derive(Clone, Debug, PartialEq)]
pub enum SectionShape {
    /// A region of the sketch.
    Region(Profile),
    /// A point of the sketch on its own, by its id: the first or last
    /// section only.
    Point(u32),
}

/// One of a loft's sections.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// Its sketch, by its Timeline name.
    pub sketch: String,
    pub shape: SectionShape,
    /// A region's start, a point of the sketch at one of its corners, by
    /// its id: none, the corner nearest the previous section's start.
    pub start: Option<u32>,
}

/// How a loft runs between its sections: the panel's tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoftMode {
    #[default]
    Smooth,
    Ruled,
}

/// The words a loft's `mode` takes, the default first.
const LOFT_MODES: &[(&str, LoftMode)] = &[("smooth", LoftMode::Smooth), ("ruled", LoftMode::Ruled)];

/// `loft`'s arguments: what the Loft panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Loft {
    /// The name and new bodies' names asked for.
    pub naming: Box<Naming>,
    /// Its sections in order, 1 to [`MAX_ITEMS`].
    pub sections: Vec<Section>,
    pub mode: LoftMode,
    /// The last section lofted back to the first.
    pub closed: bool,
    /// Its rails, at most [`MAX_ITEMS`].
    pub rails: Vec<CurveIn>,
    pub operation: Operation,
    pub exclude: Vec<String>,
}

/// The kind of link `add_link` adds: the sketch tools' Project and
/// Intersect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkKind {
    /// Model edges, faces' outlines, corners, or another sketch's curves
    /// and points, projected onto the sketch's plane.
    #[default]
    Project,
    /// Model faces and edges cut with the sketch's plane.
    Intersect,
}

/// The words `kind` takes, the default first.
const LINK_KINDS: &[(&str, LinkKind)] = &[
    ("project", LinkKind::Project),
    ("intersect", LinkKind::Intersect),
];

/// What a link comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkFrom {
    /// A face, edge or corner of the model, by its key.
    Item(Item),
    /// Another sketch's curve, by its id there.
    Curve(CurveIn),
    /// Another sketch's point, by its id there.
    Point { sketch: String, point: u32 },
}

/// `add_link`'s arguments: a click of the sketch's Project or Intersect
/// tool.
#[derive(Clone, Debug, PartialEq)]
pub struct AddLink {
    /// The sketch linked into, by its Timeline name.
    pub sketch: String,
    pub kind: LinkKind,
    pub from: LinkFrom,
}

/// `set_feature`'s arguments: the feature, by its Timeline name, and
/// the patch, the rest of the arguments as sent (within the JSON-RPC
/// limits), which the app reads as the feature's tool's arguments
/// ([`fields`], [`parse_arguments`]).
#[derive(Clone, Debug, PartialEq)]
pub struct SetFeature {
    pub name: String,
    pub patch: Map<String, Value>,
}

/// The names a creating tool asks for what it makes: the feature's,
/// at most [`NAME_BYTES`], the default ("Extrude 2") if `None`; and its
/// new bodies', in the order its result lists them, those not named
/// keeping their defaults ("Body 3").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Naming {
    pub name: Option<String>,
    pub bodies: Vec<String>,
}

/// Which names of new bodies a creating tool takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NewBodies {
    /// None: it makes no body.
    None,
    /// One, `body_name`.
    One,
    /// A list, `body_names`.
    Many,
}

impl NewBodies {
    /// The argument naming them, if any.
    fn key(self) -> Option<&'static str> {
        match self {
            NewBodies::None => None,
            NewBodies::One => Some("body_name"),
            NewBodies::Many => Some("body_names"),
        }
    }
}

/// `rename`'s arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Rename {
    /// What's renamed, by its name now.
    pub target: Target,
    /// The name it's to have, at most [`NAME_BYTES`]: one another has
    /// gets a number after it.
    pub to: String,
}

/// A feature or a body, by name.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A feature, by its Timeline name.
    Feature(String),
    /// A body, by its Objects name.
    Body(String),
}

/// Where a listing starts and how much of it to give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    /// How many to skip.
    pub offset: usize,
    /// How many to list at most, at least 1 and at most [`MAX_LIMIT`].
    pub limit: usize,
}

impl Page {
    /// `design_summary`'s page when none is asked for.
    pub const SUMMARY: Page = Page {
        offset: 0,
        limit: SUMMARY_LIMIT,
    };

    /// `feature`'s page when none is asked for.
    pub const FEATURE: Page = Page {
        offset: 0,
        limit: FEATURE_LIMIT,
    };
}

/// `sketch`'s arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SketchQuery {
    /// The sketch, by its Timeline name.
    pub name: String,
    /// Each listing's page.
    pub page: Page,
}

/// `measure`'s arguments: one item, or two to measure between.
#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    pub a: Measured,
    pub b: Option<Measured>,
}

/// `render`'s arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Render {
    /// The bodies drawn, by their Objects names, at most [`MAX_ITEMS`];
    /// every visible one if empty.
    pub bodies: Vec<String>,
    /// Where it looks from.
    pub from: RenderFrom,
    pub perspective: bool,
    /// A factor on the framing, from [`MIN_ZOOM`] to [`MAX_ZOOM`]: above
    /// 1 closes in on the bodies' centre, below 1 shows more around them;
    /// the camera doesn't move, so it never goes inside them.
    pub zoom: f64,
    /// Faces and edges drawn selected, by key, at most [`MAX_ITEMS`].
    pub highlight: Vec<Item>,
    /// The picture's long side at most, in pixels, from
    /// [`MIN_RENDER_SIZE`] to [`MAX_RENDER_SIZE`].
    pub size: u32,
}

/// Where `render` looks from: a named view, or yaw and pitch as typed
/// angles (at most [`TEXT_BYTES`] each).
#[derive(Clone, Debug, PartialEq)]
pub enum RenderFrom {
    View(NamedView),
    Angles { yaw: String, pitch: String },
}

/// The views `render` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedView {
    /// From above the front right, as Home looks.
    Iso,
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

const NAMED_VIEWS: &[(&str, NamedView)] = &[
    ("iso", NamedView::Iso),
    ("front", NamedView::Front),
    ("back", NamedView::Back),
    ("left", NamedView::Left),
    ("right", NamedView::Right),
    ("top", NamedView::Top),
    ("bottom", NamedView::Bottom),
];

/// `render`'s smallest and largest `zoom`.
pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 10.0;

/// `render`'s default `size`, and the smallest and largest taken.
pub const RENDER_SIZE: u32 = 768;
pub const MIN_RENDER_SIZE: u32 = 64;
pub const MAX_RENDER_SIZE: u32 = 1024;

/// What `measure` measures: a face, edge or corner by its key, or a
/// body by its Objects name, at most [`NAME_BYTES`] either.
#[derive(Clone, Debug, PartialEq)]
pub enum Measured {
    Item(Item),
    Body(String),
}

/// `body_topology`'s arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Topology {
    /// The body, by its Objects name.
    pub body: String,
    /// Only the faces passing this, if sent.
    pub faces: Option<FaceFilter>,
    /// Only the edges passing this, if sent.
    pub edges: Option<EdgeFilter>,
    /// How many to skip in each listing.
    pub offset: usize,
    /// How many to list at most in each, at most [`MAX_LIMIT`].
    pub limit: usize,
}

/// Which faces to list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceFilter {
    /// Flat faces facing this way.
    pub normal: Option<Direction>,
}

/// Which edges to list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeFilter {
    /// Straight edges along this, either way whatever its sign: an
    /// edge's sense is arbitrary.
    pub direction: Option<Direction>,
    /// Only convex edges (`true`) or only concave ones (`false`).
    pub convex: Option<bool>,
}

/// A direction from the client: `"+z"`, `"-x"`, `"z"` or three numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Direction {
    /// Unit length.
    pub vector: [f64; 3],
    /// Whether the sense counts: `"z"` alone is either way along it,
    /// `"+z"`, `"-z"` and numbers are one way.
    pub signed: bool,
}

/// Why `tools/call` params weren't a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// No such tool, or params not shaped as `tools/call`'s: -32602.
    Params(String),
    /// The tool's arguments don't fit it: a tool result with `isError`.
    Arguments(String),
}

/// Reads `tools/call`'s params: `{name, arguments?}`.
pub fn parse_call(params: Option<&Value>) -> Result<Call, CallError> {
    let bad = |m: &str| CallError::Params(m.to_owned());
    let Some(Value::Object(params)) = params else {
        return Err(bad("tools/call needs params with the tool's name"));
    };
    let Some(Value::String(name)) = params.get("name") else {
        return Err(bad("tools/call needs the tool's name"));
    };
    let empty = Map::new();
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(arguments)) => arguments,
        Some(_) => return Err(bad("A tool's arguments are an object")),
    };
    parse_arguments(name, arguments)
}

/// Reads `arguments` as the tool `tool`'s, as `tools/call` does (a
/// `set_feature` patch: [`parse_patch`]).
pub fn parse_arguments(tool: &str, arguments: &Map<String, Value>) -> Result<Call, CallError> {
    let name = tool;
    let mut args = Arguments {
        tool: name,
        map: arguments,
    };
    // The names of new bodies a feature tool takes, and its arguments
    // checked here once for every feature tool.
    let feature = feature_tool(name);
    let new_bodies = feature.map_or(NewBodies::None, |tool| tool.new_bodies);
    if let Some(tool) = feature {
        args.only_making(tool.fields, new_bodies)?;
    }
    let call = match name {
        "design_summary" => {
            args.only(&["offset", "limit"])?;
            Call::DesignSummary(page(&args, SUMMARY_LIMIT)?)
        }
        "feature" => {
            args.only(&["name", "offset", "limit"])?;
            Call::Feature {
                name: args.name("name")?,
                page: page(&args, FEATURE_LIMIT)?,
            }
        }
        "body_topology" => {
            args.only(&["body", "faces", "edges", "offset", "limit"])?;
            Call::BodyTopology(topology(&mut args)?)
        }
        "measure" => {
            args.only(&["a", "b"])?;
            let a = args.measured("a")?;
            let a = a.ok_or_else(|| args.error(format_args!("a is required: {MEASURED}")))?;
            Call::Measure(Measure {
                a,
                b: args.measured("b")?,
            })
        }
        "render" => {
            args.only(&["bodies", "from", "projection", "zoom", "highlight", "size"])?;
            Call::Render(render(&args)?)
        }
        "sketch" => {
            args.only(&["name", "offset", "limit"])?;
            Call::Sketch(SketchQuery {
                name: args.name("name")?,
                page: page(&args, SKETCH_LIMIT)?,
            })
        }
        "rename" => {
            args.only(&["feature", "body", "to"])?;
            let target = match (
                args.map.contains_key("feature"),
                args.map.contains_key("body"),
            ) {
                (true, false) => Target::Feature(args.name("feature")?),
                (false, true) => Target::Body(args.name("body")?),
                _ => return Err(args.error("give either feature or body, not both")),
            };
            Call::Rename(Rename {
                target,
                to: args.name("to")?,
            })
        }
        "set_visible" => {
            args.only(&["body", "visible"])?;
            Call::SetVisible {
                body: args.name("body")?,
                visible: args.required_boolean("visible")?,
            }
        }
        "set_color" => {
            args.only(&["body", "color"])?;
            let body = args.name("body")?;
            let color = match args.nullable("color", "the theme's")? {
                None => None,
                Some(Value::Object(map)) => {
                    let color = args.nested(map);
                    color.only(&["hue", "saturation"])?;
                    Some(Color {
                        hue: color.number_in(
                            "hue",
                            0.0..360.0,
                            "in degrees, from 0 to under 360",
                        )?,
                        saturation: color.number_in(
                            "saturation",
                            0.0..=MAX_SATURATION,
                            format_args!("from 0 to {MAX_SATURATION}"),
                        )?,
                    })
                }
                Some(_) => return Err(args.error("color is {hue, saturation} or null")),
            };
            Call::SetColor { body, color }
        }
        "set_opacity" => {
            args.only(&["body", "opacity"])?;
            Call::SetOpacity {
                body: args.name("body")?,
                opacity: args.number_in(
                    "opacity",
                    MIN_OPACITY..=1.0,
                    format_args!("from {MIN_OPACITY} to 1"),
                )?,
            }
        }
        "remove_feature" => Call::RemoveFeature {
            feature: args.sole_name("feature")?,
        },
        "remove_body" => Call::RemoveBody {
            body: args.sole_name("body")?,
        },
        tool @ ("add_parameter" | "set_parameter") => {
            args.only(&["name", "value"])?;
            let name = args.name("name")?;
            let value = args.text("value")?;
            match tool {
                "add_parameter" => Call::AddParameter { name, value },
                _ => Call::SetParameter { name, value },
            }
        }
        "rename_parameter" => {
            args.only(&["name", "to"])?;
            Call::RenameParameter {
                name: args.name("name")?,
                to: args.name("to")?,
            }
        }
        "remove_parameter" => Call::RemoveParameter {
            name: args.sole_name("name")?,
        },
        "set_units" => {
            args.only(&["units"])?;
            Call::SetUnits(args.required_choice("units", UNITS)?)
        }
        "set_tolerance" => {
            args.only(&["tolerance"])?;
            Call::SetTolerance {
                tolerance: args.text("tolerance")?,
            }
        }
        "undo" => {
            args.only(&["steps"])?;
            let steps = args.count("steps")?.unwrap_or(1);
            if !(1..=MAX_UNDO_STEPS).contains(&steps) {
                return Err(args.error(format_args!(
                    "steps is a whole number from 1 to {MAX_UNDO_STEPS}"
                )));
            }
            Call::Undo { steps }
        }
        "set_rollback" => {
            args.only(&["before"])?;
            let before = match args.nullable("before", "the end")? {
                None => None,
                Some(_) => Some(args.name("before")?),
            };
            Call::SetRollback { before }
        }
        "add_sketch" => {
            args.only(&["plane", "name"])?;
            Call::AddSketch {
                plane: args.sketch_plane()?,
                name: args.optional_name("name")?,
            }
        }
        "set_sketch_plane" => {
            args.only(&["sketch", "plane"])?;
            Call::SetSketchPlane {
                sketch: args.name("sketch")?,
                plane: args.sketch_plane()?,
            }
        }
        "add_link" => {
            args.only(&["sketch", "kind", "from"])?;
            Call::AddLink(AddLink {
                sketch: args.name("sketch")?,
                kind: args.choice("kind", LINK_KINDS)?,
                from: args.link_from()?,
            })
        }
        "edit_sketch" => {
            args.only(&["sketch", "operations"])?;
            Call::EditSketch(EditSketch {
                sketch: args.name("sketch")?,
                operations: args.operations()?,
            })
        }
        "set_feature" => {
            let name = args.name("name")?;
            let patch: Map<String, Value> = (args.map.iter())
                .filter(|(key, _)| *key != "name")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            if let Some(why) = renaming(&patch) {
                return Err(args.error(why));
            }
            if patch.is_empty() {
                return Err(args.error(
                    "give the fields to change beside name, as feature shows them (to \
                     rename the feature, use rename)",
                ));
            }
            Call::SetFeature(SetFeature { name, patch })
        }
        "extrude" => Call::Extrude(Extrude {
            naming: Box::new(args.naming(new_bodies)?),
            sketch: args.name("sketch")?,
            profiles: args.profiles()?,
            extent: args.choice("extent", EXTRUDE_EXTENTS)?,
            distance: args.optional_text("distance")?,
            second_distance: args.optional_text("second_distance")?,
            flip: args.flag("flip", false)?,
            taper: args.optional_text("taper")?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        }),
        "revolve" => Call::Revolve(Revolve {
            naming: Box::new(args.naming(new_bodies)?),
            sketch: args.name("sketch")?,
            profiles: args.profiles()?,
            axis: args.axis()?,
            extent: args.choice("extent", REVOLVE_EXTENTS)?,
            angle: args.optional_text("angle")?,
            second_angle: args.optional_text("second_angle")?,
            flip: args.flag("flip", false)?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        }),
        "combine" => {
            let operation = args.choice("operation", BODY_OPS)?;
            let tools = args.names("tools")?;
            if tools.is_empty() {
                return Err(args.error("tools is required: at least one body"));
            }
            Call::Combine(Combine {
                naming: Box::new(args.naming(new_bodies)?),
                target: args.name("target")?,
                tools,
                operation,
                keep_tools: args.flag("keep_tools", false)?,
            })
        }
        "move_body" => Call::MoveBody(MoveBody {
            naming: Box::new(args.naming(new_bodies)?),
            bodies: args.bodies()?,
            offset: [
                args.optional_text("x")?,
                args.optional_text("y")?,
                args.optional_text("z")?,
            ],
            angle: args.optional_text("angle")?,
            axis: args.reference("axis", WORLD_AXES, AXIS_ITEMS)?,
        }),
        "mirror" => {
            let bodies = args.bodies()?;
            let plane = args.reference("plane", WORLD_PLANES, &[ItemKind::Face])?;
            Call::Mirror(Mirror {
                naming: Box::new(args.naming(new_bodies)?),
                bodies,
                plane: plane.ok_or_else(|| args.error("plane is required"))?,
                keep_original: args.flag("keep_original", true)?,
            })
        }
        "pattern" => Call::Pattern(Pattern {
            naming: Box::new(args.naming(new_bodies)?),
            kind: args.choice("kind", PATTERN_KINDS)?,
            bodies: args.bodies()?,
            axis: args.reference("axis", WORLD_AXES, AXIS_ITEMS)?,
            count: args.count_text("count")?,
            mode: args.optional_choice("mode", PATTERN_MODES)?,
            spread: args.optional_text("spread")?,
            flip: args.flag("flip", false)?,
            join: args.flag("join", true)?,
        }),
        "align" => Call::Align(Box::new(Align {
            naming: Box::new(args.naming(new_bodies)?),
            body: args.optional_name("body")?,
            from: args.align_side("from")?,
            to: args.align_side("to")?,
            flip: args.flag("flip", false)?,
            distance: args.optional_text("distance")?,
            angle: args.optional_text("angle")?,
        })),
        "scale" => Call::Scale(Box::new(Scale {
            naming: Box::new(args.naming(new_bodies)?),
            bodies: args.bodies()?,
            point: args.reference("point", ORIGIN, POINT_ITEMS)?,
            mode: args.choice("mode", SCALE_MODES)?,
            factor: args.optional_text("factor")?,
            factors: [
                args.optional_text("x")?,
                args.optional_text("y")?,
                args.optional_text("z")?,
            ],
            edge: args.optional_name("edge")?,
            length: args.optional_text("length")?,
            axis_only: args.flag("axis_only", false)?,
        })),
        "split" => Call::Split(Split {
            naming: Box::new(args.naming(new_bodies)?),
            body: args.name("body")?,
            tool: args.split_tool()?,
            original: args.optional_choice("original", SPLIT_SIDES)?,
            keep: args.choice("keep", SPLIT_KEEPS)?,
        }),
        "shell" => Call::Shell(Shell {
            naming: Box::new(args.naming(new_bodies)?),
            body: args.optional_name("body")?,
            faces: args.names("faces")?,
            thickness: args.optional_text("thickness")?,
            direction: args.choice("direction", SHELL_DIRECTIONS)?,
        }),
        "offset_face" => Call::OffsetFace(OffsetFace {
            naming: Box::new(args.naming(new_bodies)?),
            faces: args.faces()?,
            distance: args.optional_text("distance")?,
            inward: args.flag("inward", false)?,
            tangent: args.flag("tangent", true)?,
        }),
        "draft" => Call::Draft(Draft {
            naming: Box::new(args.naming(new_bodies)?),
            faces: args.faces()?,
            neutral: args.reference("neutral", WORLD_PLANES, &[ItemKind::Face])?,
            angle: args.optional_text("angle")?,
            flip: args.flag("flip", false)?,
            tangent: args.flag("tangent", true)?,
        }),
        "chamfer" => {
            let (edges, faces) = args.edges_and_faces()?;
            Call::Chamfer(Chamfer {
                naming: Box::new(args.naming(new_bodies)?),
                edges,
                faces,
                kind: args.choice("type", CHAMFER_TYPES)?,
                distance: args.optional_text("distance")?,
                second_distance: args.optional_text("second_distance")?,
                angle: args.optional_text("angle")?,
                flip: args.flag("flip", false)?,
                tangent: args.flag("tangent", true)?,
            })
        }
        "fillet" => {
            let (edges, faces) = args.edges_and_faces()?;
            Call::Fillet(Fillet {
                naming: Box::new(args.naming(new_bodies)?),
                edges,
                faces,
                radius: args.optional_text("radius")?,
                tangent: args.flag("tangent", true)?,
            })
        }
        "sweep" => Call::Sweep(Box::new(Sweep {
            naming: Box::new(args.naming(new_bodies)?),
            sketch: args.name("sketch")?,
            profiles: args.profiles()?,
            along: args.sweep_along()?,
            keep_orientation: args.flag("keep_orientation", false)?,
            twist: args.optional_text("twist")?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        })),
        "loft" => Call::Loft(Loft {
            naming: Box::new(args.naming(new_bodies)?),
            sections: args.sections()?,
            mode: args.choice("mode", LOFT_MODES)?,
            closed: args.flag("closed", false)?,
            rails: args.curves_in("rails")?,
            operation: args.choice("operation", OPERATIONS)?,
            exclude: args.names("exclude")?,
        }),
        _ => {
            return Err(CallError::Params(format!(
                "No tool named {:?}",
                shown(name)
            )));
        }
    };
    Ok(call)
}

/// The creating tools' arguments naming new bodies, which a
/// `set_feature` patch doesn't take: renaming is `rename`'s.
const RENAMES: &[&str] = &["body_name", "body_names"];

/// The arguments of each feature tool, which `set_feature` patches a
/// feature of its kind with.
const EXTRUDE_FIELDS: &[&str] = &[
    "sketch",
    "profiles",
    "extent",
    "distance",
    "second_distance",
    "flip",
    "taper",
    "operation",
    "exclude",
];
const REVOLVE_FIELDS: &[&str] = &[
    "sketch",
    "profiles",
    "axis",
    "extent",
    "angle",
    "second_angle",
    "flip",
    "operation",
    "exclude",
];
const COMBINE_FIELDS: &[&str] = &["target", "tools", "operation", "keep_tools"];
const MOVE_BODY_FIELDS: &[&str] = &["bodies", "x", "y", "z", "angle", "axis"];
const MIRROR_FIELDS: &[&str] = &["bodies", "plane", "keep_original"];
const PATTERN_FIELDS: &[&str] = &[
    "kind", "bodies", "axis", "count", "mode", "spread", "flip", "join",
];
const ALIGN_FIELDS: &[&str] = &["body", "from", "to", "flip", "distance", "angle"];
const SCALE_FIELDS: &[&str] = &[
    "bodies",
    "point",
    "mode",
    "factor",
    "x",
    "y",
    "z",
    "edge",
    "length",
    "axis_only",
];
const SPLIT_FIELDS: &[&str] = &["body", "tool", "original", "keep"];
const SHELL_FIELDS: &[&str] = &["body", "faces", "thickness", "direction"];
const OFFSET_FACE_FIELDS: &[&str] = &["faces", "distance", "inward", "tangent"];
const DRAFT_FIELDS: &[&str] = &["faces", "neutral", "angle", "flip", "tangent"];
const CHAMFER_FIELDS: &[&str] = &[
    "edges",
    "faces",
    "type",
    "distance",
    "second_distance",
    "angle",
    "flip",
    "tangent",
];
const FILLET_FIELDS: &[&str] = &["edges", "faces", "radius", "tangent"];
const SWEEP_FIELDS: &[&str] = &[
    "sketch",
    "profiles",
    "path",
    "helix",
    "keep_orientation",
    "twist",
    "operation",
    "exclude",
];
const LOFT_FIELDS: &[&str] = &[
    "sections",
    "mode",
    "closed",
    "rails",
    "operation",
    "exclude",
];

/// A feature tool: its name, its arguments (the fields `feature` shows
/// a feature of its kind by and `set_feature` patches) and the names of
/// new bodies it takes.
#[derive(Clone, Copy)]
struct FeatureTool {
    name: &'static str,
    fields: &'static [&'static str],
    new_bodies: NewBodies,
}

/// The feature tools, the one place a tool is marked as one.
const FEATURE_TOOLS: &[FeatureTool] = &[
    FeatureTool {
        name: "extrude",
        fields: EXTRUDE_FIELDS,
        new_bodies: NewBodies::One,
    },
    FeatureTool {
        name: "revolve",
        fields: REVOLVE_FIELDS,
        new_bodies: NewBodies::One,
    },
    FeatureTool {
        name: "combine",
        fields: COMBINE_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "move_body",
        fields: MOVE_BODY_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "mirror",
        fields: MIRROR_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "pattern",
        fields: PATTERN_FIELDS,
        new_bodies: NewBodies::Many,
    },
    FeatureTool {
        name: "align",
        fields: ALIGN_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "scale",
        fields: SCALE_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "split",
        fields: SPLIT_FIELDS,
        new_bodies: NewBodies::One,
    },
    FeatureTool {
        name: "shell",
        fields: SHELL_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "offset_face",
        fields: OFFSET_FACE_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "draft",
        fields: DRAFT_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "chamfer",
        fields: CHAMFER_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "fillet",
        fields: FILLET_FIELDS,
        new_bodies: NewBodies::None,
    },
    FeatureTool {
        name: "sweep",
        fields: SWEEP_FIELDS,
        new_bodies: NewBodies::One,
    },
    FeatureTool {
        name: "loft",
        fields: LOFT_FIELDS,
        new_bodies: NewBodies::One,
    },
];

/// The feature tool named `tool`, if it is one.
fn feature_tool(tool: &str) -> Option<&'static FeatureTool> {
    FEATURE_TOOLS.iter().find(|feature| feature.name == tool)
}

/// The arguments the feature tool `tool` takes, the fields `feature`
/// shows a feature of its kind by and `set_feature` patches: `None` for
/// any other tool.
pub fn fields(tool: &str) -> Option<&'static [&'static str]> {
    Some(feature_tool(tool)?.fields)
}

/// Why `patch`, a `set_feature` patch, is refused if it names what a
/// feature makes (`name`, `body_name`, `body_names`): renaming is
/// `rename`'s.
pub fn renaming(patch: &Map<String, Value>) -> Option<String> {
    let key = (RENAMES.iter().chain(&["name"])).find(|key| patch.contains_key(**key))?;
    Some(format!(
        "{key} isn't a feature's setting: rename renames features and bodies"
    ))
}

/// Reads `patch`, a `set_feature` patch of a feature made by `tool`, as
/// that tool's arguments all optional: those the tool requires and the
/// patch doesn't give are filled in with placeholders for its parser,
/// which the app never takes (it takes only the fields the patch gives).
pub fn parse_patch(tool: &str, patch: &Map<String, Value>) -> Result<Call, CallError> {
    if let Some(why) = renaming(patch) {
        return Err(CallError::Arguments(format!("set_feature: {why}")));
    }
    let fields = fields(tool).unwrap_or_default();
    let needs = |key: &str| fields.contains(&key) && !patch.contains_key(key);
    let mut filled = patch.clone();
    let required = [
        ("sketch", json!("-")),
        ("profiles", json!("all")),
        ("axis", json!("x")),
        ("target", json!("-")),
        ("tools", json!(["-"])),
        ("bodies", json!(["-"])),
        ("plane", json!("xy")),
        ("from", json!({ "point": "origin" })),
        ("to", json!({ "point": "origin" })),
        ("body", json!("-")),
        ("tool", json!("xy")),
        ("sections", json!([{ "sketch": "-", "point": 1 }])),
    ];
    for (key, value) in required {
        if needs(key) {
            filled.insert(key.to_owned(), value);
        }
    }
    // One of two: a face feature's faces, a blend's edges or faces, a
    // sweep's path or helix.
    match tool {
        "offset_face" | "draft" if needs("faces") => {
            filled.insert("faces".to_owned(), json!(["-"]));
        }
        "chamfer" | "fillet" if needs("edges") && needs("faces") => {
            filled.insert("edges".to_owned(), json!(["-"]));
        }
        "sweep" if needs("path") && needs("helix") => {
            filled.insert("path".to_owned(), json!({ "edges": ["-"] }));
        }
        _ => {}
    }
    parse_arguments(tool, &filled)
}

/// What a move's or pattern's axis, or an align's direction, may be by
/// key.
const AXIS_ITEMS: &[ItemKind] = &[ItemKind::Edge, ItemKind::Face];

/// What an align's or scale's point may be by key.
const POINT_ITEMS: &[ItemKind] = &[ItemKind::Corner, ItemKind::Edge];

fn topology(args: &mut Arguments) -> Result<Topology, CallError> {
    let body = args.name("body")?;
    let faces = match args.object("faces")? {
        None => None,
        Some(faces) => {
            faces.only(&["normal"])?;
            Some(FaceFilter {
                normal: faces.direction("normal")?,
            })
        }
    };
    let edges = match args.object("edges")? {
        None => None,
        Some(edges) => {
            edges.only(&["direction", "convex"])?;
            Some(EdgeFilter {
                direction: edges.direction("direction")?,
                convex: edges.boolean("convex")?,
            })
        }
    };
    let Page { offset, limit } = page(args, LIMIT)?;
    Ok(Topology {
        body,
        faces,
        edges,
        offset,
        limit,
    })
}

/// `render`'s arguments, all optional.
fn render(args: &Arguments) -> Result<Render, CallError> {
    let from = match args.map.get("from") {
        None | Some(Value::Null) => RenderFrom::View(NamedView::Iso),
        Some(Value::String(word)) => lookup(NAMED_VIEWS, word)
            .map(RenderFrom::View)
            .ok_or_else(|| args.error(RENDER_FROM))?,
        Some(Value::Object(map)) => {
            let from = args.nested(map);
            from.only(&["yaw", "pitch"])?;
            let angle = |key: &str| {
                (from.map.get(key))
                    .and_then(coordinate)
                    .ok_or_else(|| args.error(RENDER_FROM))
            };
            RenderFrom::Angles {
                yaw: angle("yaw")?,
                pitch: angle("pitch")?,
            }
        }
        Some(_) => return Err(args.error(RENDER_FROM)),
    };
    let perspective = args.choice("projection", PROJECTIONS)?;
    let zoom = match args.map.get("zoom") {
        None | Some(Value::Null) => 1.0,
        Some(_) => args.number_in(
            "zoom",
            MIN_ZOOM..=MAX_ZOOM,
            format_args!("from {MIN_ZOOM} to {MAX_ZOOM}"),
        )?,
    };
    let size = match args.map.get("size") {
        None | Some(Value::Null) => RENDER_SIZE,
        Some(value) => (value.as_u64())
            .and_then(|size| u32::try_from(size).ok())
            .filter(|size| (MIN_RENDER_SIZE..=MAX_RENDER_SIZE).contains(size))
            .ok_or_else(|| {
                args.error(format_args!(
                    "size is a whole number of pixels from {MIN_RENDER_SIZE} to {MAX_RENDER_SIZE}"
                ))
            })?,
    };
    const HIGHLIGHT: &str = "highlight is a list of {\"face\": key} and {\"edge\": key} (keys \
                             from body_topology)";
    let highlight = (args.list("highlight", 0, MAX_ITEMS, "faces and edges")?)
        .unwrap_or_default()
        .iter()
        .map(|item| match item {
            Value::Object(map) if map.len() == 1 => args
                .keyed(map, &[ItemKind::Face, ItemKind::Edge])?
                .ok_or_else(|| args.error(HIGHLIGHT)),
            _ => Err(args.error(HIGHLIGHT)),
        })
        .collect::<Result<_, _>>()?;
    Ok(Render {
        bodies: args.names("bodies")?,
        from,
        perspective,
        zoom,
        highlight,
        size,
    })
}

/// The words `render`'s `projection` takes, the default first: whether
/// it's perspective.
const PROJECTIONS: &[(&str, bool)] = &[("orthographic", false), ("perspective", true)];

/// What `render`'s `from` takes, in words.
const RENDER_FROM: &str = "from is \"iso\", \"front\", \"back\", \"left\", \"right\", \"top\" \
                           or \"bottom\", or {\"yaw\": angle, \"pitch\": angle}, each typed text \
                           (\"30 deg\") or a number of degrees";

/// `offset` and `limit`, `limit` defaulting to `default` and cut down to
/// [`MAX_LIMIT`].
fn page(args: &Arguments, default: usize) -> Result<Page, CallError> {
    let offset = args.count("offset")?.unwrap_or(0);
    let limit = args.count("limit")?.unwrap_or(default).min(MAX_LIMIT);
    if limit == 0 {
        return Err(args.error("limit must be at least 1"));
    }
    Ok(Page { offset, limit })
}

/// One region as `profiles` lists it: an index, or a point `[x, y]`,
/// each coordinate typed text of at most [`TEXT_BYTES`] or a finite
/// number.
fn profile(item: &Value) -> Option<Profile> {
    match item {
        Value::Number(_) => item
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Profile::Index),
        Value::Array(point) if point.len() == 2 => Some(Profile::Point([
            coordinate(&point[0])?,
            coordinate(&point[1])?,
        ])),
        _ => None,
    }
}

/// An id: a whole number that fits a `u32`.
fn id(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// A coordinate or a length: typed text of at most [`TEXT_BYTES`], not
/// empty, or a finite number, taken as its text.
fn coordinate(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() && text.len() <= TEXT_BYTES => {
            Some(text.clone())
        }
        Value::Number(number) => number
            .as_f64()
            .filter(|n| n.is_finite())
            .map(|_| number.to_string()),
        _ => None,
    }
}

/// A tool's arguments (or an object within them), read field by field.
struct Arguments<'a> {
    tool: &'a str,
    map: &'a Map<String, Value>,
}

impl<'a> Arguments<'a> {
    fn error(&self, message: impl std::fmt::Display) -> CallError {
        CallError::Arguments(format!("{}: {message}", self.tool))
    }

    /// Refuses keys not in `known`, so a misspelt filter isn't ignored.
    fn only(&self, known: &[&str]) -> Result<(), CallError> {
        match self.map.keys().find(|k| !known.contains(&k.as_str())) {
            None => Ok(()),
            Some(key) => Err(self.error(format_args!("no argument named {:?}", shown(key)))),
        }
    }

    /// Refuses keys not in `fields`, a creating tool's, nor its `name`
    /// and the names of its new `bodies`.
    fn only_making(&self, fields: &[&str], bodies: NewBodies) -> Result<(), CallError> {
        let known = |key: &str| fields.contains(&key) || key == "name" || bodies.key() == Some(key);
        match self.map.keys().find(|k| !known(k)) {
            None => Ok(()),
            Some(key) => Err(self.error(format_args!("no argument named {:?}", shown(key)))),
        }
    }

    /// A creating tool's `name` and the names of its new `bodies`.
    fn naming(&self, bodies: NewBodies) -> Result<Naming, CallError> {
        let names = match bodies {
            NewBodies::None => Vec::new(),
            NewBodies::One => self.optional_name("body_name")?.into_iter().collect(),
            NewBodies::Many => self.names("body_names")?,
        };
        Ok(Naming {
            name: self.optional_name("name")?,
            bodies: names,
        })
    }

    /// The one argument `key`, a name, refusing any other.
    fn sole_name(&self, key: &str) -> Result<String, CallError> {
        self.only(&[key])?;
        self.name(key)
    }

    /// A name: a string, not empty, at most [`NAME_BYTES`].
    fn name(&self, key: &str) -> Result<String, CallError> {
        match self.map.get(key) {
            None => Err(self.error(format_args!("{key} is required"))),
            Some(value) => self.check_name(key, value),
        }
    }

    /// [`Arguments::name`] of `value`, sent as `key`.
    fn check_name(&self, key: &str, value: &Value) -> Result<String, CallError> {
        match value {
            Value::String(s) if s.trim().is_empty() => {
                Err(self.error(format_args!("{key} is empty")))
            }
            Value::String(s) if s.len() > NAME_BYTES => {
                Err(self.error(format_args!("{key} is longer than {NAME_BYTES} bytes")))
            }
            Value::String(s) => Ok(s.clone()),
            _ => Err(self.error(format_args!("{key} is a string"))),
        }
    }

    /// An expression: a string, not empty, at most [`TEXT_BYTES`].
    fn text(&self, key: &str) -> Result<String, CallError> {
        match self.map.get(key) {
            Some(Value::String(s)) if s.len() > TEXT_BYTES => {
                Err(self.error(format_args!("{key} is longer than {TEXT_BYTES} bytes")))
            }
            _ => self.name(key),
        }
    }

    /// A number, required: finite (JSON has no others, but checked all
    /// the same).
    fn number(&self, key: &str) -> Result<f64, CallError> {
        match self.map.get(key) {
            None => Err(self.error(format_args!("{key} is required"))),
            Some(value) => (value.as_f64())
                .filter(|v| v.is_finite())
                .ok_or_else(|| self.error(format_args!("{key} is a number"))),
        }
    }

    /// A number, required, within `range`, which `said` says.
    fn number_in(
        &self,
        key: &str,
        range: impl std::ops::RangeBounds<f64>,
        said: impl std::fmt::Display,
    ) -> Result<f64, CallError> {
        let number = self.number(key)?;
        match range.contains(&number) {
            true => Ok(number),
            false => Err(self.error(format_args!("{key} is {said}"))),
        }
    }

    /// A value that must be sent but may be null, `None` for null:
    /// `null_is` says what null asks for.
    fn nullable(&self, key: &str, null_is: &str) -> Result<Option<&'a Value>, CallError> {
        match self.map.get(key) {
            None => Err(self.error(format_args!("{key} is required (null for {null_is})"))),
            Some(Value::Null) => Ok(None),
            Some(value) => Ok(Some(value)),
        }
    }

    /// The arguments of the object `map` sent among these, of the same
    /// tool.
    fn nested(&self, map: &'a Map<String, Value>) -> Arguments<'a> {
        Arguments {
            tool: self.tool,
            map,
        }
    }

    /// A boolean, `default` if not sent.
    fn flag(&self, key: &str, default: bool) -> Result<bool, CallError> {
        Ok(self.boolean(key)?.unwrap_or(default))
    }

    /// A name, if sent: as [`Arguments::name`].
    fn optional_name(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.name(key).map(Some),
        }
    }

    /// One of `choices` by its word, if sent: as [`Arguments::choice`].
    fn optional_choice<T: Copy>(
        &self,
        key: &str,
        choices: &[(&str, T)],
    ) -> Result<Option<T>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.choice(key, choices).map(Some),
        }
    }

    fn required_boolean(&self, key: &str) -> Result<bool, CallError> {
        self.boolean(key)?
            .ok_or_else(|| self.error(format_args!("{key} is required")))
    }

    fn object(&self, key: &'a str) -> Result<Option<Arguments<'a>>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(map)) => Ok(Some(self.nested(map))),
            Some(_) => Err(self.error(format_args!("{key} is an object"))),
        }
    }

    fn boolean(&self, key: &str) -> Result<Option<bool>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(self.error(format_args!("{key} is true or false"))),
        }
    }

    /// A whole number, 0 or more, that fits a `usize`.
    fn count(&self, key: &str) -> Result<Option<usize>, CallError> {
        let value = match self.map.get(key) {
            None | Some(Value::Null) => return Ok(None),
            Some(value) => value,
        };
        value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| self.error(format_args!("{key} is a whole number, 0 or more")))
    }

    /// One of `choices` by its word, required.
    fn required_choice<T: Copy>(&self, key: &str, choices: &[(&str, T)]) -> Result<T, CallError> {
        if matches!(self.map.get(key), None | Some(Value::Null)) {
            return Err(self.error(format_args!("{key} is required")));
        }
        self.choice(key, choices)
    }

    /// One of `choices` by its word, the first if not sent.
    fn choice<T: Copy>(&self, key: &str, choices: &[(&str, T)]) -> Result<T, CallError> {
        let word = match self.map.get(key) {
            None | Some(Value::Null) => return Ok(choices[0].1),
            Some(Value::String(word)) => Some(word.as_str()),
            Some(_) => None,
        };
        word.and_then(|word| lookup(choices, word)).ok_or_else(|| {
            let words: Vec<String> = choices
                .iter()
                .map(|(said, _)| format!("{said:?}"))
                .collect();
            let (last, rest) = words.split_last().expect("choices aren't empty");
            self.error(format_args!("{key} is {} or {last}", rest.join(", ")))
        })
    }

    /// An expression, if sent: as [`Arguments::text`].
    fn optional_text(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.text(key).map(Some),
        }
    }

    /// A list of names, at most [`MAX_ITEMS`]; none if not sent.
    fn names(&self, key: &str) -> Result<Vec<String>, CallError> {
        (self.list(key, 0, MAX_ITEMS, "names")?)
            .unwrap_or_default()
            .iter()
            .map(|item| self.check_name(key, item))
            .collect()
    }

    /// `key`, a list of `least` to `most` `what`; none if not sent.
    fn list(
        &self,
        key: &str,
        least: usize,
        most: usize,
        what: &str,
    ) -> Result<Option<&'a [Value]>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Array(items)) if (least..=most).contains(&items.len()) => Ok(Some(items)),
            Some(_) if least == 0 => {
                Err(self.error(format_args!("{key} is a list of at most {most} {what}")))
            }
            Some(_) => Err(self.error(format_args!("{key} is a list of {least} to {most} {what}"))),
        }
    }

    /// [`Arguments::list`], required.
    fn required_list(
        &self,
        key: &str,
        least: usize,
        most: usize,
        what: &str,
    ) -> Result<&'a [Value], CallError> {
        (self.list(key, least, most, what)?)
            .ok_or_else(|| self.error(format_args!("{key} is required")))
    }

    /// `profiles`, required: `"all"`, or a list of region indices and
    /// points `[x, y]` (each a length as typed text, or a number), at
    /// least one and at most [`MAX_ITEMS`].
    fn profiles(&self) -> Result<Profiles, CallError> {
        const SAID: &str = "profiles is \"all\" or a list of region indices (as sketch lists \
                            them) and points [x, y] inside regions";
        let items = match self.map.get("profiles") {
            Some(Value::String(all)) if all == "all" => return Ok(Profiles::All),
            Some(Value::String(_)) => return Err(self.error(SAID)),
            _ => self.required_list("profiles", 1, MAX_ITEMS, "regions, or \"all\"")?,
        };
        items
            .iter()
            .map(|item| profile(item).ok_or_else(|| self.error(SAID)))
            .collect::<Result<_, _>>()
            .map(Profiles::Picked)
    }

    /// A revolve's `axis`, required.
    fn axis(&self) -> Result<Axis, CallError> {
        const SAID: &str = "axis is \"x\" or \"y\" (the sketch's axes), {\"line\": id} (a line \
                            of the sketch) or {\"edge\": key} (a straight edge from body_topology)";
        match self.map.get("axis") {
            None => Err(self.error("axis is required")),
            Some(Value::String(axis)) if axis.eq_ignore_ascii_case("x") => Ok(Axis::SketchX),
            Some(Value::String(axis)) if axis.eq_ignore_ascii_case("y") => Ok(Axis::SketchY),
            Some(Value::Object(map)) => {
                let axis = self.nested(map);
                axis.only(&["line", "edge"])?;
                match (map.get("line"), map.contains_key("edge")) {
                    (Some(line), false) => id(line)
                        .map(Axis::Line)
                        .ok_or_else(|| self.error("axis.line is a line's id, a whole number")),
                    (None, true) => axis.name("edge").map(Axis::Edge),
                    _ => Err(self.error(SAID)),
                }
            }
            Some(_) => Err(self.error(SAID)),
        }
    }

    /// A blend's `edges` and `faces`, keys, each at most [`MAX_ITEMS`]:
    /// at least one between them.
    fn edges_and_faces(&self) -> Result<(Vec<String>, Vec<String>), CallError> {
        let (edges, faces) = (self.names("edges")?, self.names("faces")?);
        if edges.is_empty() && faces.is_empty() {
            return Err(self.error("edges or faces is required: at least one edge's or face's key"));
        }
        Ok((edges, faces))
    }

    /// A list of chains of sketch curves `key`, each `{"sketch": name,
    /// "curve": id}`, at most [`MAX_ITEMS`]; none if not sent.
    fn curves_in(&self, key: &str) -> Result<Vec<CurveIn>, CallError> {
        const WHAT: &str = "{\"sketch\": name, \"curve\": id}";
        let said = || self.error(format_args!("{key} is a list of {WHAT}"));
        (self.list(key, 0, MAX_ITEMS, WHAT)?)
            .unwrap_or_default()
            .iter()
            .map(|item| {
                let Value::Object(map) = item else {
                    return Err(said());
                };
                let one = self.nested(map);
                one.only(&["sketch", "curve"])?;
                let curve = map.get("curve").and_then(id).ok_or_else(said)?;
                Ok(CurveIn {
                    sketch: one.name("sketch")?,
                    curve,
                })
            })
            .collect()
    }

    /// A sweep's `path` or `helix`, one of them.
    fn sweep_along(&self) -> Result<SweepAlong, CallError> {
        match (self.object("path")?, self.object("helix")?) {
            (Some(path), None) => {
                path.only(&["curves", "edges", "tangent"])?;
                let curves = path.curves_in("curves")?;
                let edges = path.names("edges")?;
                if curves.is_empty() && edges.is_empty() {
                    return Err(self.error(
                        "path takes at least one of curves (chains of sketch curves) and \
                         edges (model edges' keys)",
                    ));
                }
                Ok(SweepAlong::Path {
                    curves,
                    edges,
                    tangent: path.flag("tangent", true)?,
                })
            }
            (None, Some(helix)) => {
                helix.only(&["axis", "pitch", "turns", "left_handed", "flip"])?;
                let axis = helix.reference("axis", WORLD_AXES, AXIS_ITEMS)?;
                Ok(SweepAlong::Helix {
                    axis: axis.ok_or_else(|| self.error("helix.axis is required"))?,
                    pitch: helix.optional_text("pitch")?,
                    turns: helix.number_text("turns")?,
                    left_handed: helix.flag("left_handed", false)?,
                    flip: helix.flag("flip", false)?,
                })
            }
            _ => {
                Err(self.error("path or helix is required, one of them: what the sweep runs along"))
            }
        }
    }

    /// A loft's `sections`, required: 1 to [`MAX_ITEMS`], each
    /// `{"sketch": name, "region": index or [x, y], "start"?: id}` or
    /// `{"sketch": name, "point": id}`.
    fn sections(&self) -> Result<Vec<Section>, CallError> {
        const WHAT: &str = "sections, each {\"sketch\": name, \"region\": index or [x, y], \
                            \"start\"?: point id} or {\"sketch\": name, \"point\": id}";
        let said = || self.error(format_args!("sections is a list of {WHAT}"));
        (self.required_list("sections", 1, MAX_ITEMS, WHAT)?)
            .iter()
            .map(|item| {
                let Value::Object(map) = item else {
                    return Err(said());
                };
                let one = self.nested(map);
                one.only(&["sketch", "region", "point", "start"])?;
                let shape = match (map.get("region"), map.get("point")) {
                    (Some(region), None) => SectionShape::Region(profile(region).ok_or_else(said)?),
                    (None, Some(point)) if !map.contains_key("start") => {
                        SectionShape::Point(id(point).ok_or_else(said)?)
                    }
                    (None, Some(_)) => {
                        return Err(self.error("a section's start goes with a region"));
                    }
                    _ => return Err(said()),
                };
                let start = match map.get("start") {
                    None | Some(Value::Null) => None,
                    Some(start) => Some(id(start).ok_or_else(said)?),
                };
                Ok(Section {
                    sketch: one.name("sketch")?,
                    shape,
                    start,
                })
            })
            .collect()
    }

    /// A face feature's faces, required: at least one key, at most
    /// [`MAX_ITEMS`].
    fn faces(&self) -> Result<Vec<String>, CallError> {
        let faces = self.names("faces")?;
        if faces.is_empty() {
            return Err(self.error("faces is required: at least one face's key"));
        }
        Ok(faces)
    }

    /// A split's `tool`, required: an origin plane's word, or one of
    /// `{"face": key}`, `{"body": name}`, `{"sketch": name, "profiles":
    /// ...}` and `{"sketch": name, "curves": [id, ...]}`.
    fn split_tool(&self) -> Result<SplitWith, CallError> {
        const SAID: &str = "tool is \"xy\", \"xz\" or \"yz\" (an origin plane), {\"face\": key} \
                            (a face from body_topology), {\"body\": name}, {\"sketch\": name, \
                            \"profiles\": ...} or {\"sketch\": name, \"curves\": [id, ...]}";
        let map = match self.map.get("tool") {
            None | Some(Value::Null) => return Err(self.error("tool is required")),
            Some(Value::String(word)) => {
                return lookup(WORLD_PLANES, word)
                    .map(|plane| SplitWith::Surface(Reference::World(plane)))
                    .ok_or_else(|| self.error(SAID));
            }
            Some(Value::Object(map)) => map,
            Some(_) => return Err(self.error(SAID)),
        };
        let tool = self.nested(map);
        let has = |key: &str| map.contains_key(key);
        if has("face") || has("body") {
            if map.len() != 1 {
                return Err(self.error(SAID));
            }
            if has("body") {
                return tool.name("body").map(SplitWith::Body);
            }
            let face = self.reference("tool", WORLD_PLANES, &[ItemKind::Face])?;
            return Ok(SplitWith::Surface(face.expect("tool was sent")));
        }
        match (has("profiles"), has("curves")) {
            (true, false) => {
                tool.only(&["sketch", "profiles"])?;
                Ok(SplitWith::Regions {
                    sketch: tool.name("sketch")?,
                    profiles: tool.profiles()?,
                })
            }
            (false, true) => {
                tool.only(&["sketch", "curves"])?;
                const WHAT: &str = "curve ids, whole numbers";
                let curves = (tool.required_list("curves", 1, MAX_ITEMS, WHAT)?)
                    .iter()
                    .map(id)
                    .collect::<Option<Vec<u32>>>()
                    .ok_or_else(|| tool.error(format_args!("curves is a list of {WHAT}")))?;
                Ok(SplitWith::Line {
                    sketch: tool.name("sketch")?,
                    curves,
                })
            }
            _ => Err(self.error(SAID)),
        }
    }

    /// A motion's bodies, required: at least one name, at most
    /// [`MAX_ITEMS`].
    fn bodies(&self) -> Result<Vec<String>, CallError> {
        let bodies = self.names("bodies")?;
        if bodies.is_empty() {
            return Err(self.error("bodies is required: at least one body"));
        }
        Ok(bodies)
    }

    /// A count, if sent: typed text as [`Arguments::text`], or a whole
    /// number, taken as its text for the panel's field to read.
    fn count_text(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            Some(Value::Number(_)) => self
                .count(key)
                .map(|count| count.map(|count| count.to_string())),
            _ => self.optional_text(key),
        }
    }

    /// A value, if sent: typed text as [`Arguments::text`], or a number
    /// (JSON's are finite), taken as its text for the panel's field to
    /// read and bound.
    fn number_text(&self, key: &str) -> Result<Option<String>, CallError> {
        match self.map.get(key) {
            Some(Value::Number(number)) => Ok(Some(number.to_string())),
            _ => self.optional_text(key),
        }
    }

    /// One of the world's, by a word of `worlds` (in any case), or a
    /// model item by key, `{"edge": key}` and the like, of `kinds`; none
    /// if not sent.
    fn reference<W: Copy>(
        &self,
        key: &str,
        worlds: &[(&str, W)],
        kinds: &[ItemKind],
    ) -> Result<Option<Reference<W>>, CallError> {
        let said = || {
            let mut choices: Vec<String> = worlds.iter().map(|(w, _)| format!("{w:?}")).collect();
            choices.extend(
                kinds
                    .iter()
                    .map(|kind| format!("{{\"{}\": key}}", kind.word())),
            );
            let (last, rest) = choices.split_last().expect("choices aren't empty");
            self.error(format_args!(
                "{key} is {} or {last} (keys from body_topology)",
                rest.join(", ")
            ))
        };
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(word)) => lookup(worlds, word)
                .map(|world| Some(Reference::World(world)))
                .ok_or_else(said),
            Some(Value::Object(map)) if map.len() == 1 => {
                let item = self.keyed(map, kinds)?.ok_or_else(said)?;
                Ok(Some(Reference::Key(item)))
            }
            Some(_) => Err(said()),
        }
    }

    /// The model item `map` names by key, of one of `kinds`: `None` if
    /// it names none of them.
    fn keyed(
        &self,
        map: &'a Map<String, Value>,
        kinds: &[ItemKind],
    ) -> Result<Option<Item>, CallError> {
        let Some(kind) = kinds.iter().find(|kind| map.contains_key(kind.word())) else {
            return Ok(None);
        };
        let key = self.nested(map).name(kind.word())?;
        Ok(Some(Item { kind: *kind, key }))
    }

    /// What `measure` takes as `key`, if sent: `{"body": name}`, or a
    /// face, edge or corner by key.
    fn measured(&self, key: &'a str) -> Result<Option<Measured>, CallError> {
        let said = || self.error(format_args!("{key} is {MEASURED}"));
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(map)) if map.len() == 1 && map.contains_key("body") => {
                Ok(Some(Measured::Body(self.nested(map).name("body")?)))
            }
            Some(Value::Object(map)) if map.len() == 1 => {
                let kinds = [ItemKind::Face, ItemKind::Edge, ItemKind::Corner];
                let item = self.keyed(map, &kinds)?.ok_or_else(said)?;
                Ok(Some(Measured::Item(item)))
            }
            Some(_) => Err(said()),
        }
    }

    /// A sketch's `plane`, required: an origin plane or a face.
    fn sketch_plane(&self) -> Result<Reference<WorldPlane>, CallError> {
        self.reference("plane", WORLD_PLANES, &[ItemKind::Face])?
            .ok_or_else(|| {
                self.error("plane is required: \"xy\", \"xz\", \"yz\" or {\"face\": key}")
            })
    }

    /// What a link comes from, `from`, required: a model item by key, or
    /// another sketch's curve or point by id.
    fn link_from(&self) -> Result<LinkFrom, CallError> {
        const SAID: &str = "from is required: {\"edge\": key}, {\"face\": key}, \
                            {\"corner\": key} (keys from body_topology), or {\"sketch\": name, \
                            \"curve\": id} or {\"sketch\": name, \"point\": id}";
        let Some(from) = self.object("from")? else {
            return Err(self.error(SAID));
        };
        let id_of = |key: &str| {
            (from.map.get(key))
                .and_then(id)
                .ok_or_else(|| self.error(SAID))
        };
        if from.map.contains_key("sketch") {
            from.only(&["sketch", "curve", "point"])?;
            let sketch = from.name("sketch")?;
            return match (
                from.map.contains_key("curve"),
                from.map.contains_key("point"),
            ) {
                (true, false) => Ok(LinkFrom::Curve(CurveIn {
                    sketch,
                    curve: id_of("curve")?,
                })),
                (false, true) => Ok(LinkFrom::Point {
                    sketch,
                    point: id_of("point")?,
                }),
                _ => Err(self.error(SAID)),
            };
        }
        let kinds = [ItemKind::Face, ItemKind::Edge, ItemKind::Corner];
        match self.reference("from", &[] as &[(&str, ())], &kinds) {
            Ok(Some(Reference::Key(item))) => Ok(LinkFrom::Item(item)),
            _ => Err(self.error(SAID)),
        }
    }

    /// An align's side `key`, required: `{point, direction?,
    /// second_direction?}`.
    fn align_side(&self, key: &'a str) -> Result<AlignSide, CallError> {
        let side = self
            .object(key)?
            .ok_or_else(|| self.error(format_args!("{key} is required: {{point, direction?}}")))?;
        side.only(&["point", "direction", "second_direction"])?;
        let point = side
            .reference("point", ORIGIN, POINT_ITEMS)?
            .ok_or_else(|| self.error(format_args!("{key}.point is required")))?;
        Ok(AlignSide {
            point,
            direction: side.reference("direction", WORLD_AXES, AXIS_ITEMS)?,
            second_direction: side.reference("second_direction", WORLD_AXES, AXIS_ITEMS)?,
        })
    }

    fn direction(&self, key: &str) -> Result<Option<Direction>, CallError> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => direction(value).map(Some).ok_or_else(|| {
                self.error(format_args!(
                    "{key} is an axis such as \"+z\", \"-x\" or \"y\", or three numbers \
                     not all zero"
                ))
            }),
        }
    }
}

/// What `measure`'s `a` and `b` take, in words.
const MEASURED: &str = "{\"face\": key}, {\"edge\": key}, {\"corner\": key} (keys from \
                        body_topology) or {\"body\": name}";

/// `"+z"`, `"-x"`, `"z"` (in any case), or `[x, y, z]`: finite, not all
/// zero, then scaled to unit length.
fn direction(value: &Value) -> Option<Direction> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            let (sign, axis, signed) = match text.as_bytes() {
                [b'+', axis] => (1.0, *axis, true),
                [b'-', axis] => (-1.0, *axis, true),
                [axis] => (1.0, *axis, false),
                _ => return None,
            };
            let mut vector = [0.0; 3];
            let index = match axis.to_ascii_lowercase() {
                b'x' => 0,
                b'y' => 1,
                b'z' => 2,
                _ => return None,
            };
            vector[index] = sign;
            Some(Direction { vector, signed })
        }
        Value::Array(items) if items.len() == 3 => {
            let mut vector = [0.0; 3];
            for (v, item) in vector.iter_mut().zip(items) {
                *v = item.as_f64().filter(|v| v.is_finite())?;
            }
            normalize(vector).map(|vector| Direction {
                vector,
                signed: true,
            })
        }
        _ => None,
    }
}

/// `v` at unit length, `None` if it's zero. Divided by its largest
/// component first, so squaring can neither overflow nor underflow.
fn normalize(v: [f64; 3]) -> Option<[f64; 3]> {
    let largest = v.iter().fold(0.0f64, |m, c| m.max(c.abs()));
    if largest == 0.0 || !largest.is_finite() {
        return None;
    }
    let scaled = v.map(|c| c / largest);
    let length = scaled.iter().map(|c| c * c).sum::<f64>().sqrt();
    Some(scaled.map(|c| c / length))
}

/// The `tools/list` result: the tools built so far.
pub fn list() -> Value {
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": false });
    let edit = json!({
        "readOnlyHint": false,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    });
    let destructive = json!({
        "readOnlyHint": false,
        "destructiveHint": true,
        "idempotentHint": false,
        "openWorldHint": false
    });
    let parameter = json!({ "type": "string", "description": "The parameter's name" });
    let expression = json!({
        "type": "string",
        "maxLength": TEXT_BYTES,
        "description": "An expression as typed in the Parameters popup: \"20 mm\", \"width / 2\", \
                        \"90 deg\"; bare numbers in the design's units"
    });
    let direction = json!({
        "description": "An axis such as \"+z\", \"-x\" or \"y\" (either way along it), \
                        or three numbers [x, y, z]",
        "oneOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 }
        ]
    });
    // The queries and the design's edits.
    let design = json!([
        {
            "name": "design_summary",
            "title": "Design summary",
            "description": "The open design at a glance: its units and tolerance, its \
                bodies (name, shown or hidden), the Timeline's features in order (kind, \
                name, id, and why one fails), the rollback marker, the parameters and \
                the revision. Call it first, and again when a result says the user \
                changed the design. Each listing is paged alone by offset and limit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "offset": { "type": "integer", "minimum": 0, "description": "How many to skip in each listing" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "How many to list at most in each" }
                },
                "additionalProperties": false
            },
            "outputSchema": design_summary_output(),
            "annotations": read_only,
        },
        {
            "name": "feature",
            "title": "Feature settings",
            "description": "One feature's settings, by its name in the Timeline: values \
                as typed text in the design's units, and what it refers to (sketches, \
                bodies, faces, edges) by name or key. Each list in them (edges, faces, \
                bodies, profiles, sections, rails, ...) is paged alone by offset and \
                limit: a list given whole is a plain array, one that isn't is a page \
                {items, total, left_out} with a note. A page is part of the list: \
                never send it back to set_feature as the list (it's refused); leave \
                the field out to keep the list, or send the whole list you want.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The feature's name, as the Timeline shows it" },
                    "offset": { "type": "integer", "minimum": 0, "description": "How many to skip in each list" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "How many to give at most of each" }
                },
                "required": ["name"],
                "additionalProperties": false
            },
            "annotations": read_only,
        },
        {
            "name": "body_topology",
            "title": "Body faces, edges and corners",
            "description": "A body's faces, edges and corners, each with a key to refer \
                to it by in other tools (to chamfer an edge, sketch on a face). Faces \
                carry their form, normal if flat, area and bounds; edges their form, \
                ends or axis, length, two faces and whether convex; corners their \
                point. Measures are approximate, from the display mesh. Filter to keep \
                it short, page with offset and limit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": { "type": "string", "description": "The body's name, as Objects shows it" },
                    "faces": {
                        "type": "object",
                        "description": "List only the flat faces facing this way",
                        "properties": { "normal": direction },
                        "additionalProperties": false
                    },
                    "edges": {
                        "type": "object",
                        "description": "List only these edges",
                        "properties": {
                            "direction": {
                                "description": "List only the straight edges along this axis, \
                                                either way: an edge's sense means nothing, so \
                                                \"+z\", \"-z\" and \"z\" are the same here",
                                "oneOf": direction["oneOf"].clone()
                            },
                            "convex": { "type": "boolean", "description": "Only convex (outside) edges, or only concave (inside) ones" }
                        },
                        "additionalProperties": false
                    },
                    "offset": { "type": "integer", "minimum": 0, "description": "How many to skip in each listing" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "How many to list at most in each" }
                },
                "required": ["body"],
                "additionalProperties": false
            },
            "outputSchema": body_topology_output(),
            "annotations": read_only,
        },
        {
            "name": "sketch",
            "title": "Sketch geometry and regions",
            "description": "A sketch's points and curves with their ids, its constraints \
                and dimensions (values as typed, in the design's units), its plane, its \
                regions and its degrees of freedom (0: fully constrained). Each region \
                has the index extrude, revolve, split and sweep take in profiles, its \
                area, a point inside it and its outer curves and holes by id. Any sketch, \
                open or not. Points are bare numbers in the design's units. Each listing \
                is paged alone by offset and limit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The sketch's name, as the Timeline shows it" },
                    "offset": { "type": "integer", "minimum": 0, "description": "How many to skip in each listing" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "How many to list at most in each" }
                },
                "required": ["name"],
                "additionalProperties": false
            },
            "outputSchema": sketch_output(),
            "annotations": read_only,
        },
        {
            "name": "measure",
            "title": "Measure",
            "description": "What the measure tool shows, on the model as it is: of one \
                item, a face's area (with its radius if round, width and height if a \
                rectangle, normal if flat), an edge's length (radius and centre if \
                round), a corner's point, a body's volume, area, centre and box; of \
                two, the minimum distance between them with its X, Y and Z parts, and \
                the angle between their directions where both have one. Exact, from \
                the solid; values as typed text in the design's units. Says which \
                generation of the model it measured.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "a": measured_schema("What to measure"),
                    "b": measured_schema("A second item, to measure the distance and angle between the two")
                },
                "required": ["a"],
                "additionalProperties": false
            },
            "annotations": read_only,
        },
        {
            "name": "render",
            "title": "Render",
            "description": format!("A picture of the model as a PNG image, to check by \
                eye: the bodies asked for (every visible one by default) on nothing, \
                framed and cropped to them, the others and the sketches not drawn, in \
                the app's theme. from is a named view (\"iso\", the default, as Home \
                looks; \"front\", \"back\", \"left\", \"right\", \"top\", \
                \"bottom\") or yaw and pitch: the direction looked from, yaw about Z \
                from +X (front is -90 deg, right 0), pitch up from the XY plane \
                (iso is yaw -60 deg, pitch 30 deg). highlight draws faces and edges \
                selected, to check keys before an edit. size is the long side in \
                pixels, {MIN_RENDER_SIZE} to {MAX_RENDER_SIZE}, default {RENDER_SIZE}; \
                the PNG is at most {} KiB. Waits for the app's next frame: it fails while \
                the window is minimized or there's no GPU.", ToolReply::IMAGE_BYTES / 1024),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "bodies": {
                        "type": "array",
                        "items": body(),
                        "maxItems": MAX_ITEMS,
                        "description": "The bodies to draw: every visible one if left out"
                    },
                    "from": {
                        "description": "A named view, or {yaw, pitch} as typed angles (\"30 deg\") or numbers of degrees",
                        "oneOf": [
                            { "type": "string", "enum": words(NAMED_VIEWS) },
                            {
                                "type": "object",
                                "properties": {
                                    "yaw": { "type": ["string", "number"] },
                                    "pitch": { "type": ["string", "number"] }
                                },
                                "required": ["yaw", "pitch"],
                                "additionalProperties": false
                            }
                        ]
                    },
                    "projection": { "type": "string", "enum": words(PROJECTIONS), "description": "Default orthographic" },
                    "zoom": { "type": "number", "minimum": MIN_ZOOM, "maximum": MAX_ZOOM, "description": "A factor on the framing, default 1; above 1 closes in on the bodies' centre, below 1 shows more around them, the camera staying outside them in either projection" },
                    "highlight": {
                        "type": "array",
                        "items": item(&[ItemKind::Face, ItemKind::Edge]),
                        "maxItems": MAX_ITEMS,
                        "description": "Faces and edges to draw selected, by their keys from body_topology"
                    },
                    "size": { "type": "integer", "minimum": MIN_RENDER_SIZE, "maximum": MAX_RENDER_SIZE }
                },
                "additionalProperties": false
            },
            "annotations": read_only,
        },
        {
            "name": "rename",
            "title": "Rename a feature or body",
            "description": "Renames a feature (by its Timeline name) or a body (by its \
                Objects name): give feature or body, and to. A name another feature or \
                body has gets a number after it, and the result says the name it got. \
                Applied to the design as the user's edit, one undo step; the result \
                waits for regeneration and says what failed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "feature": { "type": "string", "description": "The feature's name, as the Timeline shows it" },
                    "body": { "type": "string", "description": "The body's name, as Objects shows it" },
                    "to": { "type": "string", "description": "The new name" }
                },
                "required": ["to"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
        {
            "name": "set_visible",
            "title": "Show or hide a body",
            "description": "Shows or hides a body, as its eye in Objects does. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "body": body(), "visible": { "type": "boolean" } },
                "required": ["body", "visible"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
        {
            "name": "set_color",
            "title": "Colour a body",
            "description": "Gives a body a colour of its own, a hue and a saturation (the \
                lightness stays the theme's shading), or with color null the theme's back. \
                One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": body(),
                    "color": {
                        "description": "The colour, or null for the theme's",
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "hue": { "type": "number", "minimum": 0, "exclusiveMaximum": 360, "description": "Degrees round the colour wheel: 0 red, 120 green, 240 blue" },
                                    "saturation": { "type": "number", "minimum": 0, "maximum": MAX_SATURATION, "description": "From grey (0) to the most a body's colour may be" }
                                },
                                "required": ["hue", "saturation"],
                                "additionalProperties": false
                            },
                            { "type": "null" }
                        ]
                    }
                },
                "required": ["body", "color"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
        {
            "name": "set_opacity",
            "title": "Set a body's opacity",
            "description": "Sets how opaque a body is drawn, from 0.1 to 1 (opaque), in whole \
                percent. To hide a body use set_visible. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body": body(),
                    "opacity": { "type": "number", "minimum": MIN_OPACITY, "maximum": 1 }
                },
                "required": ["body", "opacity"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
        {
            "name": "remove_feature",
            "title": "Remove a feature",
            "description": "Removes a feature by its Timeline name, with the bodies it makes. \
                One that later features use is refused, naming them: remove those first \
                (last first), or change them. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "feature": { "type": "string", "description": "The feature's name, as the Timeline shows it" }
                },
                "required": ["feature"],
                "additionalProperties": false
            },
            "annotations": destructive,
        },
        {
            "name": "remove_body",
            "title": "Remove a body",
            "description": "Removes a body with the feature that makes it (and so that \
                feature's other bodies, as a pattern's copies: the result lists them). One \
                that later features use is refused, naming them. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "body": body() },
                "required": ["body"],
                "additionalProperties": false
            },
            "annotations": destructive,
        },
        {
            "name": "add_parameter",
            "title": "Add a parameter",
            "description": "Adds a parameter after the others, for feature values and \
                sketch dimensions to use by name. Its name is a word (letters, digits, _; \
                not starting with a digit, not a unit) no other parameter has. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": parameter, "value": expression },
                "required": ["name", "value"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        },
        {
            "name": "set_parameter",
            "title": "Set a parameter",
            "description": "Sets a parameter's expression; everything using it follows. \
                Refused if a value using it would be in error or a sketch wouldn't solve. \
                One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": parameter, "value": expression },
                "required": ["name", "value"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
        {
            "name": "rename_parameter",
            "title": "Rename a parameter",
            "description": "Renames a parameter, rewriting every use of it. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": parameter,
                    "to": { "type": "string", "description": "The new name" }
                },
                "required": ["name", "to"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        },
        {
            "name": "remove_parameter",
            "title": "Remove a parameter",
            "description": "Removes a parameter. One in use is refused, naming what uses \
                it. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": parameter },
                "required": ["name"],
                "additionalProperties": false
            },
            "annotations": destructive,
        },
        {
            "name": "set_units",
            "title": "Set the design's units",
            "description": "Changes the design's units, which bare numbers are in. Nothing \
                changes size: expressions get the old units written in. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "units": { "type": "string", "enum": words(UNITS) } },
                "required": ["units"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
        {
            "name": "set_tolerance",
            "title": "Set the design's tolerance",
            "description": "Changes how closely curved surfaces are fitted: a length from \
                0.01 µm to 0.1 mm, such as \"1 µm\" (the default) or \"0.01 mm\"; finer is \
                slower. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": { "tolerance": { "type": "string", "maxLength": TEXT_BYTES } },
                "required": ["tolerance"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
        {
            "name": "set_rollback",
            "title": "Move the rollback marker",
            "description": "Rolls the Timeline back to before a feature, so it and those \
                after aren't built (and new features go there), or forward to the end with \
                before null. One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "before": {
                        "description": "The feature to roll back to before, by its Timeline name, or null for the end",
                        "type": ["string", "null"]
                    }
                },
                "required": ["before"],
                "additionalProperties": false
            },
            "annotations": edit,
        },
    ]);
    let features = json!([
        {
            "name": "extrude",
            "title": "Extrude",
            "description": "Adds an extrude of regions of a sketch, as the Extrude panel's OK \
                does, at the rollback marker (the end unless rolled back). Values are typed \
                text in the design's units. Defaults are the panel's: one side, 10 mm (the \
                panel's own default follows the view, so give distance), no taper, a new \
                body. through_all goes with operation cut only. One undo step; the result \
                names the feature and its new bodies, and waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sketch": sketch(),
                    "profiles": profiles(),
                    "extent": { "enum": words(EXTRUDE_EXTENTS), "description": "How far it goes: default one_side; symmetric's distance is the whole, split either side" },
                    "distance": typed("A length, \"20 mm\" or a parameter expression; default 10 mm. Two sides' first"),
                    "second_distance": typed("Two sides' second length; default the first's"),
                    "flip": { "type": "boolean", "description": "One side the other way along the sketch's normal, or two sides swapped; default false" },
                    "taper": typed("A taper angle, \"5 deg\"; default none"),
                    "operation": operation(),
                    "exclude": exclude(),
                },
                "required": ["sketch", "profiles"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        },
        {
            "name": "revolve",
            "title": "Revolve",
            "description": "Adds a revolve of regions of a sketch about an axis, as the \
                Revolve panel's OK does, at the rollback marker. Defaults are the panel's: a \
                full turn, 180 deg for one side or symmetric, 90 deg for two sides' second, a \
                new body. One undo step; the result names the feature and its new bodies, \
                and waits for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sketch": sketch(),
                    "profiles": profiles(),
                    "axis": {
                        "description": "\"x\" or \"y\", the sketch's own axes; {\"line\": id}, a line of the sketch by id; or {\"edge\": key}, a straight edge in the sketch's plane by its body_topology key",
                        "oneOf": [
                            { "enum": ["x", "y"] },
                            { "type": "object", "properties": { "line": { "type": "integer", "minimum": 0 } }, "required": ["line"], "additionalProperties": false },
                            { "type": "object", "properties": { "edge": { "type": "string" } }, "required": ["edge"], "additionalProperties": false }
                        ]
                    },
                    "extent": { "enum": words(REVOLVE_EXTENTS), "description": "How far it turns: default full" },
                    "angle": typed("An angle, \"90 deg\": default 180 deg. Two sides' first"),
                    "second_angle": typed("Two sides' second angle: default 90 deg"),
                    "flip": { "type": "boolean", "description": "One side the other way round, or two sides swapped; default false" },
                    "operation": operation(),
                    "exclude": exclude(),
                },
                "required": ["sketch", "profiles", "axis"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        },
        {
            "name": "combine",
            "title": "Combine bodies",
            "description": "Adds a combine, as the Combine panel's OK does, at the rollback \
                marker: the target gets the union of it and the tools (default), the tools \
                cut from it (subtract), or only what it shares with every tool (intersect). \
                The tools are used up unless keep_tools. One undo step; the result waits \
                for regeneration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "The body kept, by its Objects name" },
                    "tools": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": MAX_ITEMS, "description": "The bodies combined into the target, by name" },
                    "operation": { "enum": words(BODY_OPS), "description": "Default union" },
                    "keep_tools": { "type": "boolean", "description": "Keep the tools as bodies of their own; default false" }
                },
                "required": ["target", "tools"],
                "additionalProperties": false
            },
            "annotations": edit_once(),
        },
    ]);
    let as_list = |tools: Value| match tools {
        Value::Array(tools) => tools,
        _ => unreachable!("a literal list"),
    };
    // In order: the queries and the design's edits, the sketch tools, the
    // patch the feature tools share, the feature tools, then the motion
    // panel's kinds, a schema each (as one literal they'd pass the
    // macro's recursion limit), and undo last.
    let mut tools = as_list(design);
    tools.extend([
        add_sketch_tool(),
        set_sketch_plane_tool(),
        add_link_tool(),
        draw::edit_sketch_tool(),
        set_feature_tool(),
    ]);
    tools.extend(as_list(features));
    tools.extend([
        move_body_tool(),
        mirror_tool(),
        pattern_tool(),
        align_tool(),
        scale_tool(),
        split_tool(),
        shell_tool(),
        offset_face_tool(),
        draft_tool(),
        chamfer_tool(),
        fillet_tool(),
        sweep_tool(),
        loft_tool(),
        undo_tool(),
    ]);
    for tool in tools.iter_mut() {
        let word = tool["name"].as_str().unwrap_or_default();
        let bodies = match word {
            "add_sketch" => NewBodies::None,
            _ => match making(word) {
                Some(bodies) => bodies,
                None => continue,
            },
        };
        let properties = &mut tool["inputSchema"]["properties"];
        properties["name"] = json!({
            "type": "string",
            "maxLength": NAME_BYTES,
            "description": "The feature's name in the Timeline; a default like 'Extrude 2' if left out — give a descriptive one"
        });
        match bodies {
            NewBodies::None => {}
            NewBodies::One => {
                properties["body_name"] = json!({
                    "type": "string",
                    "maxLength": NAME_BYTES,
                    "description": "The name of the body it makes, in Objects; a default like 'Body 3' if left out. Refused when it makes none"
                });
            }
            NewBodies::Many => {
                properties["body_names"] = json!({
                    "type": "array",
                    "items": { "type": "string", "maxLength": NAME_BYTES },
                    "maxItems": MAX_ITEMS,
                    "description": "Names for the bodies it makes (copies with join false), in the order the result lists them; those left out keep defaults like 'Body 3'. Refused when it makes fewer"
                });
            }
        }
    }
    json!({ "tools": tools })
}

/// The names of new bodies the feature tool `tool` takes, `None` for
/// any other tool.
fn making(tool: &str) -> Option<NewBodies> {
    Some(feature_tool(tool)?.new_bodies)
}

/// A sketch's plane's schema.
fn sketch_plane() -> Value {
    json!({
        "description": "\"xy\", \"xz\" or \"yz\", an origin plane; or {\"face\": key}, a flat face by its body_topology key",
        "oneOf": [
            { "enum": words(WORLD_PLANES) },
            item(&[ItemKind::Face])
        ]
    })
}

/// The `add_sketch` tool's schema.
fn add_sketch_tool() -> Value {
    json!({
        "name": "add_sketch",
        "title": "Add a sketch",
        "description": "Adds an empty sketch on an origin plane or a flat face, as New sketch \
            does, at the rollback marker (the end unless rolled back). It isn't opened for \
            the user. Draw in it with the sketch tools. One undo step; the result names the \
            sketch and its id, and waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": { "plane": sketch_plane() },
            "required": ["plane"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `set_sketch_plane` tool's schema.
fn set_sketch_plane_tool() -> Value {
    json!({
        "name": "set_sketch_plane",
        "title": "Change a sketch's plane",
        "description": "Puts a sketch on another origin plane or flat face, as Change plane \
            does, its drawing kept as it is in its own coordinates. A face must be made \
            before the sketch. One undo step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": { "sketch": sketch(), "plane": sketch_plane() },
            "required": ["sketch", "plane"],
            "additionalProperties": false
        },
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
        },
    })
}

/// The `add_link` tool's schema.
fn add_link_tool() -> Value {
    let id = |what: &str| json!({ "type": "integer", "minimum": 0, "description": what });
    json!({
        "name": "add_link",
        "title": "Link into a sketch",
        "description": "Adds a link to a sketch, as a click of its Project or Intersect tool \
            does: project takes a model edge, a face (its outline), a corner, or another \
            sketch's curve or point, onto the sketch's plane; intersect cuts a model face or \
            edge with the plane. Only what's made before the sketch. The linked geometry \
            follows its source as the design changes. One undo step, solved first; the \
            result names the link and the ids of the points and curves it made, as sketch \
            lists them.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sketch": sketch(),
                "kind": { "enum": words(LINK_KINDS), "description": "default project" },
                "from": {
                    "description": "What it links: a model face, edge or corner by its \
                        body_topology key, or another sketch's curve or point by its id there",
                    "oneOf": [
                        item(&[ItemKind::Face, ItemKind::Edge, ItemKind::Corner]),
                        {
                            "type": "object",
                            "properties": { "sketch": sketch(), "curve": id("The curve's id in that sketch") },
                            "required": ["sketch", "curve"],
                            "additionalProperties": false
                        },
                        {
                            "type": "object",
                            "properties": { "sketch": sketch(), "point": id("The point's id in that sketch") },
                            "required": ["sketch", "point"],
                            "additionalProperties": false
                        }
                    ]
                }
            },
            "required": ["sketch", "from"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `set_feature` tool's schema.
fn set_feature_tool() -> Value {
    json!({
        "name": "set_feature",
        "title": "Change a feature",
        "description": "Changes a feature's settings, as its panel's OK on the edited \
            feature does: name is the feature, and the other arguments a patch in the \
            shape feature shows it, its creating tool's arguments (extrude's for an \
            extrude, ...), each given replacing the feature's, the rest kept. Read it \
            with feature first, change fields and send them back; a field sent as \
            feature shows it changes nothing. Fields its kind or mode doesn't have \
            are refused. A list feature gave as a page (part of it) is refused: \
            leave it out to keep the list, or send it whole. One undo step; the \
            result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "The feature's name, as the Timeline shows it" }
            },
            "required": ["name"],
            "additionalProperties": true
        },
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": true,
            "idempotentHint": true,
            "openWorldHint": false
        },
    })
}

/// The `move_body` tool's schema.
fn move_body_tool() -> Value {
    json!({
        "name": "move_body",
        "title": "Move bodies",
        "description": "Adds a move, as the Move panel's OK does, at the rollback marker: \
            each body turned by angle about axis, then shifted by x, y and z along the \
            world axes. Values are typed text in the design's units. Defaults are the \
            panel's: 0 mm, 0 deg about the Z axis; one that moves nothing is refused. \
            One undo step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "bodies": bodies(),
                "x": typed("The shift along world X, a length: default 0 mm"),
                "y": typed("The shift along world Y: default 0 mm"),
                "z": typed("The shift along world Z: default 0 mm"),
                "angle": typed("The turn about axis, right-handed, \"90 deg\": default 0 deg"),
                "axis": motion_axis("What it turns about: default \"z\""),
            },
            "required": ["bodies"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `mirror` tool's schema.
fn mirror_tool() -> Value {
    json!({
        "name": "mirror",
        "title": "Mirror bodies",
        "description": "Adds a mirror, as the Mirror panel's OK does, at the rollback \
            marker: each body reflected in plane, keeping the original too (default) \
            or becoming its image. One undo step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "bodies": bodies(),
                "plane": {
                    "description": "\"xy\", \"xz\" or \"yz\", an origin plane; or {\"face\": key}, a flat face by its body_topology key",
                    "oneOf": [
                        { "enum": words(WORLD_PLANES) },
                        item(&[ItemKind::Face])
                    ]
                },
                "keep_original": { "type": "boolean", "description": "The panel's Create copy: keep each body and its image together; default true" }
            },
            "required": ["bodies", "plane"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `pattern` tool's schema.
fn pattern_tool() -> Value {
    json!({
        "name": "pattern",
        "title": "Pattern bodies",
        "description": "Adds a linear or circular pattern, as its panel's OK does, at the \
            rollback marker: count copies of each body, the original among them, along \
            or about axis. Defaults are the panel's: linear along X, 3, spaced 10 mm \
            (the panel's own spacing follows the view, so give spread); circular about Z, \
            4 evenly round a full turn. Copies join their body unless join is false, \
            when each is a body of its own. One undo step; the result waits for \
            regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "kind": { "enum": words(PATTERN_KINDS), "description": "Default linear" },
                "bodies": bodies(),
                "axis": motion_axis("The direction (linear: default \"x\") or the axis turned about (circular: default \"z\")"),
                "count": {
                    "type": ["string", "integer"],
                    "maxLength": TEXT_BYTES,
                    "description": "How many, the original among them, 2 to 1024: a whole number or an expression; default 3 (linear) or 4 (circular)"
                },
                "mode": { "enum": words(PATTERN_MODES), "description": "What spread is: spacing, one step (linear's default); total, first to last; full, a circular one's copies round a full turn with no spread (circular's default)" },
                "spread": typed("The spacing or total: a positive length for linear (default 10 mm), an angle for circular (default 90 deg)"),
                "flip": { "type": "boolean", "description": "A linear one's copies the other way along axis; default false" },
                "join": { "type": "boolean", "description": "Join to original: the copies in their body (default true), else each copy a new body" }
            },
            "required": ["bodies"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `align` tool's schema.
fn align_tool() -> Value {
    json!({
        "name": "align",
        "title": "Align a body",
        "description": "Adds an align, as the Align panel's OK does, at the rollback \
            marker: the body moved so from's point meets to's, and with directions on \
            both sides, from's direction turned onto to's (a second pair fixes the turn \
            about it). Points are corners or edges (a straight one's middle, a round \
            one's centre; a round edge's centre with no direction also gives its side \
            the rim's axis) by body_topology key, or to's \"origin\"; directions are \
            faces or edges by key, or to's \"x\", \"y\" or \"z\". One undo step; the \
            result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "body": { "type": "string", "description": "The body moved, by name: default the one from's point is on" },
                "from": align_side("On the body moved"),
                "to": align_side("What it's aligned to, on another body or the origin"),
                "flip": { "type": "boolean", "description": "The directions meet the other way round (with directions only); default false" },
                "distance": typed("An offset along to's direction (with directions only): default 0 mm"),
                "angle": typed("A turn about to's direction (with directions only): default 0 deg")
            },
            "required": ["from", "to"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `scale` tool's schema.
fn scale_tool() -> Value {
    json!({
        "name": "scale",
        "title": "Scale bodies",
        "description": "Adds a scale, as the Scale panel's OK does, at the rollback \
            marker, about point: uniform by factor, per_axis by x, y and z factors, or \
            edge_length, the factor that gives edge (an edge of a scaled body) length. \
            Factors are from 0.001 to 1000; one scaling nothing is refused. One undo \
            step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "bodies": bodies(),
                "point": {
                    "description": "What it scales about: \"origin\" (default), or {\"corner\": key} or {\"edge\": key} (a straight edge's middle, a round one's centre) by body_topology key",
                    "oneOf": [
                        { "const": "origin" },
                        item(&[ItemKind::Corner, ItemKind::Edge])
                    ]
                },
                "mode": { "enum": words(SCALE_MODES), "description": "Default uniform" },
                "factor": typed("Uniform's factor: default 1"),
                "x": typed("Per axis's factor along world X: default 1"),
                "y": typed("Per axis's factor along world Y: default 1"),
                "z": typed("Per axis's factor along world Z: default 1"),
                "edge": { "type": "string", "description": "Edge length's edge, by its body_topology key" },
                "length": typed("Edge length's length, \"50 mm\""),
                "axis_only": { "type": "boolean", "description": "Edge length's Along its axis only, for a straight edge along a world axis: scale along that axis alone; default false" }
            },
            "required": ["bodies"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `split` tool's schema.
fn split_tool() -> Value {
    json!({
        "name": "split",
        "title": "Split a body",
        "description": "Adds a split, as the Split panel's OK does, at the rollback \
            marker: body cut in two by tool. The front is the part inside the tool (on \
            the side a plane's normal points to, out of a face's body, inside a tool \
            body or the sketch's regions, left of a line); the back the rest. With keep \
            both, the piece original names (default front) keeps the body's name and id \
            and the other becomes a new body; keeping one side trims the other away. \
            One undo step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "body": body(),
                "tool": {
                    "description": "\"xy\", \"xz\" or \"yz\", an origin plane; {\"face\": key}, a face by its body_topology key (a flat one's plane, a curved one's surface continued); {\"body\": name}, another body, kept; {\"sketch\": name, \"profiles\": ...}, regions of a sketch made before, through the body both ways; or {\"sketch\": name, \"curves\": [id, ...]}, the curves of one open line of a sketch",
                    "oneOf": [
                        { "enum": words(WORLD_PLANES) },
                        item(&[ItemKind::Face]),
                        {
                            "type": "object",
                            "properties": { "body": { "type": "string" } },
                            "required": ["body"],
                            "additionalProperties": false
                        },
                        {
                            "type": "object",
                            "properties": { "sketch": sketch(), "profiles": profiles() },
                            "required": ["sketch", "profiles"],
                            "additionalProperties": false
                        },
                        {
                            "type": "object",
                            "properties": {
                                "sketch": sketch(),
                                "curves": {
                                    "type": "array",
                                    "items": { "type": "integer", "minimum": 0 },
                                    "minItems": 1,
                                    "maxItems": MAX_ITEMS
                                }
                            },
                            "required": ["sketch", "curves"],
                            "additionalProperties": false
                        }
                    ]
                },
                "original": { "enum": words(SPLIT_SIDES), "description": "With keep both: the piece that keeps the body's name and id; default front" },
                "keep": { "enum": words(SPLIT_KEEPS), "description": "The pieces kept: both (default), or front or back alone, trimming the other away" }
            },
            "required": ["body", "tool"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `shell` tool's schema.
fn shell_tool() -> Value {
    json!({
        "name": "shell",
        "title": "Shell a body",
        "description": "Adds a shell, as the Shell panel's OK does, at the rollback \
            marker: a body hollowed to walls thickness thick, faces opened (none, a \
            closed hollow body). Its body is its faces'; with no faces give body, or the \
            model's only body is taken. Defaults: 2 mm (the panel's own follows the \
            view), inward. One undo step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "body": { "type": "string", "description": "The body hollowed closed, by its Objects name: only with no faces" },
                "faces": faces("The faces removed, opening the shell, all on one body", 0),
                "thickness": typed("The walls' thickness, a positive length: default 2 mm"),
                "direction": { "enum": words(SHELL_DIRECTIONS), "description": "inward (default), the walls inside the body's faces; or outward, outside them" }
            },
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `offset_face` tool's schema.
fn offset_face_tool() -> Value {
    json!({
        "name": "offset_face",
        "title": "Offset faces",
        "description": "Adds an offset face, as the Offset face panel's OK does, at the \
            rollback marker: faces of one body moved along their normals by distance, \
            out of the body or inward into it. Defaults: 1 mm (the panel's own follows \
            the view), outward, tangent faces taken in. One undo step; the result waits \
            for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "faces": faces("The faces moved, all on one body", 1),
                "distance": typed("How far, a positive length: default 1 mm"),
                "inward": { "type": "boolean", "description": "Into the body, shrinking it; default false" },
                "tangent": { "type": "boolean", "description": "Take in faces that run on smoothly; default true" }
            },
            "required": ["faces"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `draft` tool's schema.
fn draft_tool() -> Value {
    json!({
        "name": "draft",
        "title": "Draft faces",
        "description": "Adds a draft, as the Draft panel's OK does, at the rollback \
            marker: faces of one body turned by angle about where they meet the neutral \
            plane, so the part narrows along the pull (the plane's normal, reversed by \
            flip). Defaults: XY, 3 deg, tangent faces taken in. One undo step; the result \
            waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "faces": faces("The faces drafted, all on one body", 1),
                "neutral": {
                    "description": "The neutral plane: \"xy\" (default), \"xz\" or \"yz\", an origin plane; or {\"face\": key}, a flat face not drafted, by its body_topology key",
                    "oneOf": [
                        { "enum": words(WORLD_PLANES) },
                        item(&[ItemKind::Face])
                    ]
                },
                "angle": typed("The draft angle, above 0 and under 90 deg: default 3 deg"),
                "flip": { "type": "boolean", "description": "Pull against the neutral plane's normal; default false" },
                "tangent": { "type": "boolean", "description": "Take in faces that run on smoothly; default true" }
            },
            "required": ["faces"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `chamfer` tool's schema.
fn chamfer_tool() -> Value {
    json!({
        "name": "chamfer",
        "title": "Chamfer edges",
        "description": "Adds a chamfer, as the Chamfer panel's OK does, at the rollback \
            marker: edges of one body cut off flat, a face standing for the edges round it \
            (an edge named round a face named is left out). type equal cuts distance along \
            both faces; two_distances distance along the first face and second_distance \
            along the other; distance_angle distance along the first face at angle to it. \
            An edge's first face is its key's first, the other with flip (not for equal). \
            Defaults: equal, 1 mm, second 2 mm, 45 deg (the panel's distances follow the \
            view), tangent chains taken in. The kernel doesn't chamfer yet: it applies and \
            fails in regeneration, as for the user. One undo step; the result waits for \
            regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "edges": faces("The edges cut off, all on one body", 0),
                "faces": faces("Faces whose edges are cut off, on the edges' body", 0),
                "type": { "enum": words(CHAMFER_TYPES), "description": "How it's sized: default equal" },
                "distance": typed("A positive length: default 1 mm"),
                "second_distance": typed("With two_distances, the length along the other face: default 2 mm"),
                "angle": typed("With distance_angle, the cut's angle to the first face, above 0 and under 90 deg: default 45 deg"),
                "flip": { "type": "boolean", "description": "With two_distances or distance_angle: the first face is the other; default false" },
                "tangent": tangent()
            },
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `fillet` tool's schema.
fn fillet_tool() -> Value {
    json!({
        "name": "fillet",
        "title": "Fillet edges",
        "description": "Adds a fillet, as the Fillet panel's OK does, at the rollback \
            marker: edges of one body rounded to radius, a face standing for the edges round \
            it (an edge named round a face named is left out). Defaults: 2 mm (the panel's \
            follows the view), tangent chains taken in. The kernel doesn't fillet yet: it \
            applies and fails in regeneration, as for the user. One undo step; the result \
            waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "edges": faces("The edges rounded, all on one body", 0),
                "faces": faces("Faces whose edges are rounded, on the edges' body", 0),
                "radius": typed("A positive length: default 2 mm"),
                "tangent": tangent()
            },
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `sweep` tool's schema.
fn sweep_tool() -> Value {
    json!({
        "name": "sweep",
        "title": "Sweep",
        "description": "Adds a sweep of regions of a sketch, as the Sweep panel's OK does, \
            at the rollback marker: along a path (chains of curves of sketches made before \
            but the profile's, each named by one of its curves, and model edges of one body) \
            or a helix about an axis. keep_orientation and twist go with a path only. \
            Defaults: follow the path, no twist, a helix's pitch 10 mm (the panel's follows \
            the view) and 5 turns, a new body. One undo step; the result waits for \
            regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sketch": sketch(),
                "profiles": profiles(),
                "path": {
                    "type": "object",
                    "description": "Along a path: give this or helix",
                    "properties": {
                        "curves": curves_in("Chains of sketch curves, each the chain the curve named is in"),
                        "edges": faces("Model edges, all on one body", 0),
                        "tangent": tangent()
                    },
                    "additionalProperties": false
                },
                "helix": {
                    "type": "object",
                    "description": "Along a helix: give this or path",
                    "properties": {
                        "axis": motion_axis("The helix's axis: \"x\", \"y\" or \"z\", or {\"edge\": key} (straight or round) or {\"face\": key} (round)"),
                        "pitch": typed("The rise per turn, a positive length: default 10 mm"),
                        "turns": { "type": ["string", "number"], "maxLength": TEXT_BYTES, "description": "How many turns, part of one too, typed text or a number: default 5" },
                        "left_handed": { "type": "boolean", "description": "Turning the other way; default false" },
                        "flip": { "type": "boolean", "description": "Rising the other way along the axis; default false" }
                    },
                    "required": ["axis"],
                    "additionalProperties": false
                },
                "keep_orientation": { "type": "boolean", "description": "A path's profile keeps its orientation rather than turning with the path; default false" },
                "twist": typed("A path's twist from end to end, an angle: default 0 deg"),
                "operation": operation(),
                "exclude": exclude()
            },
            "required": ["sketch", "profiles"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// The `loft` tool's schema.
fn loft_tool() -> Value {
    json!({
        "name": "loft",
        "title": "Loft",
        "description": "Adds a loft, as the Loft panel's OK does, at the rollback marker: a \
            solid through its sections in order, each a region without holes of a sketch \
            made before, or first or last a sketch point on its own. A region's start is a \
            sketch point at one of its corners (default the corner nearest the previous \
            section's start). Rails, open chains of sketch curves each named by one of its \
            curves, guide it, not with closed. Defaults: smooth, open, a new body. One undo \
            step; the result waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sections": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_ITEMS,
                    "description": "The sections in order, at least two for a loft",
                    "items": {
                        "type": "object",
                        "properties": {
                            "sketch": sketch(),
                            "region": {
                                "description": "A region of the sketch: its index (as sketch lists them) or a point [x, y] inside it",
                                "oneOf": [
                                    { "type": "integer", "minimum": 0 },
                                    { "type": "array", "items": { "type": ["string", "number"] }, "minItems": 2, "maxItems": 2 }
                                ]
                            },
                            "point": { "type": "integer", "minimum": 0, "description": "A point of the sketch on its own, by id: the first or last section only" },
                            "start": { "type": "integer", "minimum": 0, "description": "A region's start: a point of the sketch at one of its corners, by id" }
                        },
                        "required": ["sketch"],
                        "additionalProperties": false
                    }
                },
                "mode": { "enum": words(LOFT_MODES), "description": "Between sections: smooth (default) or ruled; two sections are ruled either way" },
                "closed": { "type": "boolean", "description": "The last section lofted back to the first; no rails then; default false" },
                "rails": curves_in("Rails guiding it from the first section to the last, each the open chain the curve named is in"),
                "operation": operation(),
                "exclude": exclude()
            },
            "required": ["sections"],
            "additionalProperties": false
        },
        "annotations": edit_once(),
    })
}

/// `undo`'s schema: the session's own newest edits taken back.
fn undo_tool() -> Value {
    json!({
        "name": "undo",
        "title": "Undo your own edits",
        "description": "Takes back your own newest edits, as the user's Undo would: steps \
            steps of the design's history (default 1), each one of your edits in this \
            session. Refused, nothing undone, if any of them isn't yours (the user edited, \
            undid or redid since, or reverted) or the history doesn't reach that far: \
            then edit again instead. There's no redo: edit again. The result lists what \
            was undone, newest first, and waits for regeneration.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "steps": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_UNDO_STEPS,
                    "description": "How many of your newest edits to take back; default 1"
                }
            },
            "additionalProperties": false
        },
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": true,
            "idempotentHint": false,
            "openWorldHint": false
        },
    })
}

/// An edit tool's annotations, asked twice the second failing or adding another.
fn edit_once() -> Value {
    json!({
        "readOnlyHint": false,
        "destructiveHint": false,
        "idempotentHint": false,
        "openWorldHint": false
    })
}

/// A body's schema, by name.
fn body() -> Value {
    json!({ "type": "string", "description": "The body's name, as Objects shows it" })
}

/// A sketch feature's schema, by name.
fn sketch() -> Value {
    json!({ "type": "string", "description": "The sketch, by its Timeline name" })
}

/// A feature's profiles' schema: a sketch's regions.
fn profiles() -> Value {
    json!({
        "description": "The sketch's regions: \"all\", or a list of region indices (in the \
                        order the sketch's regions are found, as sketch lists them) and points \
                        [x, y] inside a region, in the sketch's own coordinates as lengths \
                        (\"5 mm\", or bare numbers in the design's units)",
        "oneOf": [
            { "const": "all" },
            {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_ITEMS,
                "items": { "oneOf": [
                    { "type": "integer", "minimum": 0 },
                    {
                        "type": "array",
                        "items": { "type": ["string", "number"] },
                        "minItems": 2,
                        "maxItems": 2
                    }
                ]}
            }
        ]
    })
}

/// A feature's operation's schema.
fn operation() -> Value {
    json!({
        "enum": words(OPERATIONS),
        "description": "What it does with its solid: default new_body"
    })
}

/// The schema of the bodies a join, cut or intersect leaves out.
fn exclude() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "maxItems": MAX_ITEMS,
        "description": "Bodies a join, cut or intersect leaves alone, by name; it takes every \
                        other body it touches"
    })
}

/// A motion's bodies' schema.
fn bodies() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "minItems": 1,
        "maxItems": MAX_ITEMS,
        "description": "The bodies, by their Objects names"
    })
}

/// A blend's or face feature's Tangent chain schema.
fn tangent() -> Value {
    json!({ "type": "boolean", "description": "Tangent chain: each edge with the edges running on smoothly from it; default true" })
}

/// A feature's value's schema, as typed text.
fn typed(description: &str) -> Value {
    json!({ "type": "string", "maxLength": TEXT_BYTES, "description": description })
}

/// The schema of a list of chains of sketch curves.
fn curves_in(description: &str) -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_ITEMS,
        "items": {
            "type": "object",
            "properties": {
                "sketch": { "type": "string", "description": "The sketch, by its Timeline name" },
                "curve": { "type": "integer", "minimum": 0, "description": "A curve of the chain, by its id in the sketch" }
            },
            "required": ["sketch", "curve"],
            "additionalProperties": false
        },
        "description": description
    })
}

/// A face feature's faces' schema, at least `least` keys.
fn faces(description: &str, least: usize) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "minItems": least,
        "maxItems": MAX_ITEMS,
        "description": format!("{description}, by their body_topology keys")
    })
}

/// `measure`'s `a` or `b` schema: a body by name, or a face, edge or
/// corner by key.
fn measured_schema(description: &str) -> Value {
    let body = json!({
        "type": "object",
        "properties": { "body": { "type": "string", "description": "The body's name, as Objects shows it" } },
        "required": ["body"],
        "additionalProperties": false
    });
    let mut one_of = vec![body];
    if let Value::Object(items) = item(&[ItemKind::Face, ItemKind::Edge, ItemKind::Corner])
        && let Some(Value::Array(items)) = items.get("oneOf")
    {
        one_of.extend(items.iter().cloned());
    }
    json!({
        "description": format!("{description}: {{\"face\": key}}, {{\"edge\": key}} or \
                                {{\"corner\": key}} by body_topology key, or {{\"body\": name}}"),
        "oneOf": one_of
    })
}

/// A schema for a model item by key, of one of `kinds`.
fn item(kinds: &[ItemKind]) -> Value {
    let one = |kind: &ItemKind| {
        json!({
            "type": "object",
            "properties": { kind.word(): { "type": "string" } },
            "required": [kind.word()],
            "additionalProperties": false
        })
    };
    match kinds {
        [kind] => one(kind),
        _ => json!({ "oneOf": kinds.iter().map(one).collect::<Vec<_>>() }),
    }
}

/// A move's or pattern's axis's schema, `description` saying what it's
/// for.
fn motion_axis(description: &str) -> Value {
    json!({
        "description": format!("{description}. \"x\", \"y\" or \"z\", a world axis; or \
                                {{\"edge\": key}}, a straight or round edge, or {{\"face\": key}}, \
                                a round face, by body_topology key"),
        "oneOf": [{ "enum": words(WORLD_AXES) }, item(AXIS_ITEMS)]
    })
}

/// An align's side's schema.
fn align_side(description: &str) -> Value {
    let direction = |what: &str| {
        json!({
            "description": format!("{what}: a face (a flat one's normal, a round one's axis) or an \
                                    edge (a straight one's direction, a round one's axis) by \
                                    body_topology key; on to also \"x\", \"y\" or \"z\""),
            "oneOf": [{ "enum": words(WORLD_AXES) }, item(AXIS_ITEMS)]
        })
    };
    json!({
        "type": "object",
        "description": description,
        "properties": {
            "point": {
                "description": "A corner, or an edge (a straight one's middle, a round one's centre), by body_topology key; on to also \"origin\"",
                "oneOf": [{ "const": "origin" }, item(POINT_ITEMS)]
            },
            "direction": direction("The direction"),
            "second_direction": direction("A second direction, fixing the turn about the first")
        },
        "required": ["point"],
        "additionalProperties": false
    })
}

fn design_summary_output() -> Value {
    json!({
        "type": "object",
        "properties": {
            "units": { "type": "string" },
            "tolerance": { "type": "string" },
            "revision": { "type": "integer" },
            "pending_edit": { "type": "string" },
            "user_changes": { "type": "object" },
            "bodies": listing(json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "visible": { "type": "boolean" }
            }})),
            "timeline": listing(json!({ "type": "object", "properties": {
                "kind": { "type": "string" },
                "name": { "type": "string" },
                "id": { "type": "integer" },
                "failure": { "type": "string" }
            }})),
            "rollback": { "type": ["string", "null"] },
            "parameters": listing(json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "value": { "type": "string" }
            }}))
        }
    })
}

/// A paged listing's schema, its items `item`.
fn listing(item: Value) -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": { "type": "array", "items": item },
            "total": { "type": "integer" },
            "left_out": { "type": "integer" }
        }
    })
}

fn sketch_output() -> Value {
    let ids = json!({ "type": "array", "items": { "type": "integer" } });
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "id": { "type": "integer" },
            "units": { "type": "string" },
            "plane": {},
            "revision": { "type": "integer" },
            "degrees_of_freedom": { "type": "integer" },
            "pending_edit": { "type": "string" },
            "user_changes": { "type": "object" },
            "points": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "at": { "type": "array", "items": { "type": "number" } }
            }})),
            "curves": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "kind": { "type": "string" },
                "construction": { "type": "boolean" }
            }})),
            "constraints": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "kind": { "type": "string" }
            }})),
            "dimensions": listing(json!({ "type": "object", "properties": {
                "id": { "type": "integer" },
                "measure": { "type": "string" },
                "value": { "type": "string" },
                "driving": { "type": "boolean" }
            }})),
            "regions": listing(json!({ "type": "object", "properties": {
                "index": { "type": "integer" },
                "area": { "type": "string" },
                "inside": { "type": "array", "items": { "type": "number" } },
                "outer": ids,
                "outer_left_out": { "type": "integer" },
                "holes": { "type": "array", "items": ids },
                "holes_left_out": { "type": "integer" },
                "hole_curves_left_out": { "type": "integer" }
            }}))
        }
    })
}

fn body_topology_output() -> Value {
    json!({
        "type": "object",
        "properties": {
            "body": { "type": "string" },
            "revision": { "type": "integer" },
            "pending_edit": { "type": "string" },
            "user_changes": { "type": "object" },
            "faces": listing(json!({ "type": "object", "properties": {
                "key": { "type": "string" },
                "form": { "type": "string" }
            }})),
            "edges": listing(json!({ "type": "object", "properties": {
                "key": { "type": "string" },
                "form": { "type": "string" },
                "convex": { "type": "boolean" }
            }})),
            "corners": listing(json!({ "type": "object", "properties": {
                "key": { "type": "string" }
            }}))
        }
    })
}

/// A tool's answer: text, and for data the same JSON as structured
/// content, or an error the model reads and recovers from.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolReply {
    pub text: String,
    /// Sent as `structuredContent`, its JSON also the text.
    pub structured: Option<Value>,
    pub is_error: bool,
    /// A PNG, sent as image content after the text, base64: bounded by
    /// [`ToolReply::IMAGE_BYTES`], apart from the text's cap.
    pub png: Option<Vec<u8>>,
}

impl ToolReply {
    /// The default [`ToolReply::result`] cap, in bytes of text: well
    /// under the 10k tokens Claude Code warns at, counting the
    /// structured copy too.
    pub const CAP: usize = 16 * 1024;

    /// The largest PNG a reply carries, in bytes: 2 MiB, under 3 MB as
    /// base64, well within what clients take of an image (Claude's 5 MB).
    pub const IMAGE_BYTES: usize = 2 * 1024 * 1024;

    /// A plain text answer.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: false,
            png: None,
        }
    }

    /// A refusal or failure, text alone.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: true,
            png: None,
        }
    }

    /// Data: `value` as structured content and as its JSON text.
    pub fn data(value: Value) -> Self {
        Self {
            // A `Value`: serializing can't fail.
            text: serde_json::to_string(&value).expect("a value serializes"),
            structured: Some(value),
            is_error: false,
            png: None,
        }
    }

    /// The same, with the PNG `png` as image content after the text.
    pub fn with_png(self, png: Vec<u8>) -> Self {
        Self {
            png: Some(png),
            ..self
        }
    }

    /// The `tools/call` result, the text cut to `cap` bytes with a note
    /// saying so. Cut data can't be structured content any more, nor meet
    /// the tool's `outputSchema`, so it goes as text alone, with `isError`
    /// (clients check a result without it against the schema).
    pub fn result(&self, cap: usize) -> Value {
        let mut result = Map::new();
        let mut is_error = self.is_error;
        if self.text.len() <= cap {
            if let Some(structured) = &self.structured {
                result.insert("structuredContent".into(), structured.clone());
            }
            result.insert("content".into(), text_content(&self.text));
        } else {
            let kept = crate::cut(&self.text, cap);
            let note = format!(
                "\n[Cut: this reply was {} bytes, past the cap of {cap}. Ask for less, \
                 such as with filters or offset and limit.]",
                self.text.len()
            );
            let mut text = String::with_capacity(kept.len().saturating_add(note.len()));
            text.push_str(kept);
            text.push_str(&note);
            result.insert("content".into(), text_content(&text));
            is_error |= self.structured.is_some();
        }
        if let Some(png) = &self.png
            && let Some(Value::Array(content)) = result.get_mut("content")
        {
            if png.len() <= Self::IMAGE_BYTES {
                use base64::Engine;
                content.push(json!({
                    "type": "image",
                    "data": base64::engine::general_purpose::STANDARD.encode(png),
                    "mimeType": "image/png",
                }));
            } else {
                // The app keeps under the bound: a backstop.
                content.push(json!({
                    "type": "text",
                    "text": format!(
                        "[The image was {} bytes, past the bound of {}: ask for a smaller size.]",
                        png.len(),
                        Self::IMAGE_BYTES
                    ),
                }));
                is_error = true;
            }
        }
        result.insert("isError".into(), Value::Bool(is_error));
        Value::Object(result)
    }
}

fn text_content(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}

#[cfg(test)]
mod tests;
