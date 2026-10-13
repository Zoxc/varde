//! The checked calls: [`Call`] and the arguments of each tool, with the
//! word tables ([`Worded`]) they're read through.

use super::*;

/// Declares a choice the tools name by a word: the enum, its table
/// `$table` of words in the variants' order (the default first, where
/// there is one), and [`Worded`] by both, so every variant has its word.
macro_rules! worded {
    (
        $(#[$table_meta:meta])*
        $table:ident;
        $(#[$meta:meta])*
        $vis:vis enum $ty:ident {
            $($(#[$variant_meta:meta])* $variant:ident = $word:literal,)*
        }
    ) => {
        $(#[$meta])*
        $vis enum $ty {
            $($(#[$variant_meta])* $variant,)*
        }

        $(#[$table_meta])*
        pub(super) const $table: &[(&str, $ty)] = &[$(($word, $ty::$variant)),*];

        impl Worded for $ty {
            const WORDS: &'static [(&'static str, Self)] = $table;

            fn word(self) -> &'static str {
                match self {
                    $($ty::$variant => $word,)*
                }
            }
        }
    };
}

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

/// The most a pattern makes, the original among them:
/// `varde_document::MAX_PATTERN_COUNT`.
pub const MAX_PATTERN_COUNT: u32 = 1024;

/// A body's colour: a hue and a saturation, the lightness the theme's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// Degrees round the colour wheel, in `0..360`.
    pub hue: f64,
    /// In `0..=`[`MAX_SATURATION`].
    pub saturation: f64,
}

worded! {
    /// The words `units` takes, as typed and shown.
    UNITS;
    /// The design units `set_units` offers: the file menu's.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Units {
        Mm = "mm",
        In = "in",
    }
}

/// The most profiles, excluded bodies, tools, faces or curves a feature
/// tool takes: `varde_document`'s `MAX_EXTRUDE_REGIONS`,
/// `MAX_REVOLVE_REGIONS`, `MAX_FEATURE_BODIES`, `MAX_SHELL_FACES`,
/// `MAX_OFFSET_FACES`, `MAX_DRAFT_FACES` and `MAX_SPLIT_CURVES`, all
/// 256.
pub const MAX_ITEMS: usize = 256;

/// The words of `choices`, for a schema's `enum`.
pub(super) fn words<T>(choices: &[(&str, T)]) -> Value {
    choices.iter().map(|(word, _)| json!(word)).collect()
}

/// The one with `word` among `choices`, by one rule for every argument:
/// trimmed and ignoring ASCII case.
pub(super) fn lookup<T: Copy>(choices: &[(&str, T)], word: &str) -> Option<T> {
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
    fn word(self) -> &'static str;
}

impl Worded for () {
    const WORDS: &'static [(&'static str, Self)] = ORIGIN;

    fn word(self) -> &'static str {
        ORIGIN[0].0
    }
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

worded! {
    /// The words an extrude's or revolve's `operation` takes, the default
    /// first.
    OPERATIONS;
    /// What an extrude or a revolve does with its solid: the panel's
    /// Operation.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Operation {
        #[default]
        NewBody = "new_body",
        Join = "join",
        Cut = "cut",
        Intersect = "intersect",
    }
}

worded! {
    /// The words `extent` takes for an extrude, the default first.
    EXTRUDE_EXTENTS;
    /// How far an extrude goes: the panel's Extent.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum ExtrudeExtent {
        #[default]
        OneSide = "one_side",
        Symmetric = "symmetric",
        TwoSides = "two_sides",
        /// Only with [`Operation::Cut`].
        ThroughAll = "through_all",
    }
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

worded! {
    /// The words `extent` takes for a revolve, the default first.
    REVOLVE_EXTENTS;
    /// How far a revolve turns: the panel's Extent.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum RevolveExtent {
        #[default]
        Full = "full",
        OneSide = "one_side",
        Symmetric = "symmetric",
        TwoSides = "two_sides",
    }
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

worded! {
    /// The words a combine's `operation` takes, the default first.
    BODY_OPS;
    /// What a combine does: the panel's Operation.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum BodyOp {
        #[default]
        Union = "union",
        Subtract = "subtract",
        Intersect = "intersect",
    }
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

worded! {
    /// The words a world axis is named by.
    WORLD_AXES;
    /// A world axis through the origin, along its positive side.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WorldAxis {
        X = "x",
        Y = "y",
        Z = "z",
    }
}

worded! {
    /// The words an origin plane is named by.
    WORLD_PLANES;
    /// An origin plane.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WorldPlane {
        Xy = "xy",
        Xz = "xz",
        Yz = "yz",
    }
}

/// The word the origin is named by, as a point.
pub(super) const ORIGIN: &[(&str, ())] = &[("origin", ())];

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

worded! {
    /// The words `kind` takes, the default first.
    PATTERN_KINDS;
    /// A pattern's kind: the panel's Linear or Circular pattern.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum PatternKind {
        #[default]
        Linear = "linear",
        Circular = "circular",
    }
}

worded! {
    /// The words `mode` takes.
    PATTERN_MODES;
    /// How a pattern's copies are spread: the panel's mode tiles.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PatternMode {
        /// `spread` is one step.
        Spacing = "spacing",
        /// `spread` is from the first copy to the last.
        Total = "total",
        /// A circular one's copies evenly round a full turn.
        Full = "full",
    }
}

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

worded! {
    /// The words a scale's `mode` takes, the default first.
    SCALE_MODES;
    /// How a scale scales: the panel's mode tiles.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum ScaleMode {
        #[default]
        Uniform = "uniform",
        PerAxis = "per_axis",
        EdgeLength = "edge_length",
    }
}

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

worded! {
    /// The words a split's `original` takes, the default first.
    SPLIT_SIDES;
    /// A split's side: the part inside the tool (front) or the rest.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum SplitSide {
        #[default]
        Front = "front",
        Back = "back",
    }
}

worded! {
    /// The words a split's `keep` takes, the default first.
    SPLIT_KEEPS;
    /// Which of a split's pieces stay: the panel's Keep tiles.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum SplitKeep {
        #[default]
        Both = "both",
        Front = "front",
        Back = "back",
    }
}

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

worded! {
    /// The words a shell's `direction` takes, the default first.
    SHELL_DIRECTIONS;
    /// Which way a shell's walls grow: the panel's Direction tiles.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum ShellDirection {
        #[default]
        Inward = "inward",
        Outward = "outward",
    }
}

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

worded! {
    /// The words a chamfer's `type` takes, the default first.
    CHAMFER_TYPES;
    /// How a chamfer is sized: the panel's Type tiles.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum ChamferType {
        /// One distance along both faces.
        #[default]
        Equal = "equal",
        /// A distance along each face.
        TwoDistances = "two_distances",
        /// A distance along the first face and the cut's angle to it.
        DistanceAngle = "distance_angle",
    }
}

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

worded! {
    /// The words a loft's `mode` takes, the default first.
    LOFT_MODES;
    /// How a loft runs between its sections: the panel's tiles.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum LoftMode {
        #[default]
        Smooth = "smooth",
        Ruled = "ruled",
    }
}

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

worded! {
    /// The words `kind` takes, the default first.
    LINK_KINDS;
    /// The kind of link `add_link` adds: the sketch tools' Project and
    /// Intersect.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum LinkKind {
        /// Model edges, faces' outlines, corners, or another sketch's curves
        /// and points, projected onto the sketch's plane.
        #[default]
        Project = "project",
        /// Model faces and edges cut with the sketch's plane.
        Intersect = "intersect",
    }
}

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
/// ([`FeatureTool::fields`], [`parse_arguments`]).
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
pub(super) enum NewBodies {
    /// None: it makes no body.
    None,
    /// One, `body_name`.
    One,
    /// A list, `body_names`.
    Many,
}

impl NewBodies {
    /// The argument naming them, if any.
    pub(super) fn key(self) -> Option<&'static str> {
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

    /// `body_topology`'s page when none is asked for.
    pub const TOPOLOGY: Page = Page {
        offset: 0,
        limit: TOPOLOGY_LIMIT,
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

worded! {
    /// The words `render`'s `projection` takes, the default first.
    PROJECTIONS;
    /// How `render` projects the view.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Projection {
        Orthographic = "orthographic",
        Perspective = "perspective",
    }
}

/// `render`'s arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Render {
    /// The bodies drawn, by their Objects names, at most [`MAX_ITEMS`];
    /// every visible one if empty.
    pub bodies: Vec<String>,
    /// Where it looks from.
    pub from: RenderFrom,
    pub projection: Projection,
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

worded! {
    NAMED_VIEWS;
    /// The views `render` names.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum NamedView {
        /// From above the front right, as Home looks.
        Iso = "iso",
        Front = "front",
        Back = "back",
        Left = "left",
        Right = "right",
        Top = "top",
        Bottom = "bottom",
    }
}

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
    /// Where each listing starts and how much of it to give.
    pub page: Page,
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
