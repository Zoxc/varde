//! The move, mirror or pattern being set up: what the app hands the view
//! of it, the messages changing it, and its floating panel over the right
//! of the viewport. Its bodies are picked as a combine's (in the viewport,
//! where a click picks the body of what it's on, or in Objects); a move's
//! or pattern's axis and a mirror's plane are picked in the viewport too
//! (a model edge or face) or, while they're the ones picking, from the
//! toolbar's origin axes or planes. The viewport's side, drawing the axis or plane and a
//! move's handles, which set its offsets and turn, is in
//! `viewport/motion.rs`.

use glam::DVec3;
use iced::Element;
use iced::widget::text::Wrapping;
use iced::widget::{column, text};
use varde_document::{
    Axis3, AxisRef, BodyId, Document, EdgeRef, FaceRef, FeatureId, FeatureKind, Keep, OriginPlane,
    Pattern, PatternKind, PlaneRef, Side,
};
use varde_expr::{AngleUnit, LengthUnit, Unit};
use varde_sketch::Id;

/// Angles are shown in degrees.
const DEGREES: Unit = Unit::Angle(AngleUnit::Deg);

use crate::chrome::sentence;
use crate::icons::Icon;
use crate::operation_panel::{
    Footer, Framing, PanelHover, Parts, TypedField, field, footer_message, label, message_text,
    operation_panel, pick_field, picked_row, tile, tiles, toggle, value_field,
};
use crate::plane_pick::face_name;
use crate::revolve::edge_axis_name;
use crate::theme;
use crate::{CombineBody, Edit, Look, Message, VALUE_FIELD};

/// Which is set up: a move, a mirror, a linear or circular pattern, an
/// align, a scale, a split, a chamfer, a shell or a fillet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionKind {
    Move,
    Mirror,
    LinearPattern,
    CircularPattern,
    Align,
    Scale,
    Split,
    Chamfer,
    Shell,
    Fillet,
}

impl MotionKind {
    /// Its name as a feature's noun: "Move", "Pattern".
    pub fn noun(self) -> &'static str {
        match self {
            MotionKind::Move => "Move",
            MotionKind::Mirror => "Mirror",
            MotionKind::LinearPattern | MotionKind::CircularPattern => "Pattern",
            MotionKind::Align => "Align",
            MotionKind::Scale => "Scale",
            MotionKind::Split => "Split",
            MotionKind::Chamfer => "Chamfer",
            MotionKind::Shell => "Shell",
            MotionKind::Fillet => "Fillet",
        }
    }

    /// Its icon, the UI mock's `move`, `bmirror`, `lpattern`, `cpattern`,
    /// `align`, `scale`, `chamfer`, `shell` and `fillet`, and the icon mock's split
    /// body.
    pub fn icon(self) -> Icon {
        match self {
            MotionKind::Move => Icon::Move,
            MotionKind::Mirror => Icon::BMirror,
            MotionKind::LinearPattern => Icon::LPattern,
            MotionKind::CircularPattern => Icon::CPattern,
            MotionKind::Align => Icon::Align,
            MotionKind::Scale => Icon::Scale,
            MotionKind::Split => Icon::Split,
            MotionKind::Chamfer => Icon::BChamfer,
            MotionKind::Shell => Icon::Shell,
            MotionKind::Fillet => Icon::BFillet,
        }
    }

    /// The panel's title for a new one, the mock's: "New move", "New
    /// linear pattern".
    pub fn new_title(self) -> &'static str {
        match self {
            MotionKind::Move => "New move",
            MotionKind::Mirror => "New mirror",
            MotionKind::LinearPattern => "New linear pattern",
            MotionKind::CircularPattern => "New circular pattern",
            MotionKind::Align => "New align",
            MotionKind::Scale => "New scale",
            MotionKind::Split => "New split",
            MotionKind::Chamfer => "New chamfer",
            MotionKind::Shell => "New shell",
            MotionKind::Fillet => "New fillet",
        }
    }

    /// Whether it's a pattern.
    pub fn pattern(self) -> bool {
        matches!(
            self,
            MotionKind::LinearPattern | MotionKind::CircularPattern
        )
    }

    /// Whether its reference is an axis (a move's, a pattern's), not a
    /// plane (a mirror's), an align's points and directions, a scale's
    /// point and edge, a split's tool, a chamfer's edges or a shell's
    /// faces.
    pub fn takes_axis(self) -> bool {
        !matches!(
            self,
            MotionKind::Mirror
                | MotionKind::Align
                | MotionKind::Scale
                | MotionKind::Split
                | MotionKind::Chamfer
                | MotionKind::Shell
                | MotionKind::Fillet
        )
    }

    /// Whether it picks edges to blend, a chamfer's or a fillet's: its
    /// body is its edges', never picked itself.
    pub fn blends(self) -> bool {
        matches!(self, MotionKind::Chamfer | MotionKind::Fillet)
    }

    /// Whether it picks faces of one body, a shell's (an offset face's and
    /// a draft's, once there are those): its body is its faces', or picked
    /// itself while it has none.
    pub fn picks_faces(self) -> bool {
        self == MotionKind::Shell
    }

    /// The session that edits a feature of `kind`, if one does: a move, a
    /// mirror, a linear or circular pattern, an align, a scale, a split, a
    /// chamfer or a shell.
    pub fn of(kind: &FeatureKind) -> Option<MotionKind> {
        Some(match kind {
            FeatureKind::Move(_) => MotionKind::Move,
            FeatureKind::Mirror(_) => MotionKind::Mirror,
            FeatureKind::Pattern(pattern) => match pattern.kind {
                PatternKind::Linear { .. } => MotionKind::LinearPattern,
                PatternKind::Circular { .. } => MotionKind::CircularPattern,
            },
            FeatureKind::Align(_) => MotionKind::Align,
            FeatureKind::Scale(_) => MotionKind::Scale,
            FeatureKind::Split(_) => MotionKind::Split,
            FeatureKind::Chamfer(_) => MotionKind::Chamfer,
            FeatureKind::Shell(_) => MotionKind::Shell,
            FeatureKind::Fillet(_) => MotionKind::Fillet,
            _ => return None,
        })
    }
}

/// How a pattern's copies are spread, the UI mock's choices: by the
/// spacing between neighbours, by the total from the first to the last,
/// or (a circular one's) evenly round a full turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PatternMode {
    #[default]
    Spacing,
    Total,
    Full,
}

impl PatternMode {
    /// The choices a pattern of `kind` offers, in the mock's order.
    pub fn of(kind: MotionKind) -> &'static [PatternMode] {
        match kind {
            MotionKind::CircularPattern => {
                &[PatternMode::Full, PatternMode::Spacing, PatternMode::Total]
            }
            _ => &[PatternMode::Spacing, PatternMode::Total],
        }
    }

    /// Its label, the mock's: "Spacing", "Total", "Full 360°".
    pub fn label(self) -> &'static str {
        match self {
            PatternMode::Spacing => "Spacing",
            PatternMode::Total => "Total",
            PatternMode::Full => "Full 360°",
        }
    }

    /// Its tile's icon for a pattern of `kind`, the mock's `lp-spacing`,
    /// `lp-total`, `cp-full`, `cp-spacing` and `cp-total`.
    fn icon(self, kind: MotionKind) -> Icon {
        match (kind, self) {
            (MotionKind::CircularPattern, PatternMode::Spacing) => Icon::CpSpacing,
            (MotionKind::CircularPattern, PatternMode::Total) => Icon::CpTotal,
            (_, PatternMode::Full) => Icon::CpFull,
            (_, PatternMode::Spacing) => Icon::LpSpacing,
            (_, PatternMode::Total) => Icon::LpTotal,
        }
    }
}

/// What a click in the viewport picks: bodies, the reference (a move's
/// axis, a mirror's plane), one of an align's points or directions, a
/// scale's point or its edge, a split's tool, a chamfer's edges, a
/// shell's faces, or nothing (an align with all it asks for picked, a split with its
/// tool). A click on one of the panel's fields makes it the one picking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionPick {
    #[default]
    Bodies,
    Reference,
    Align(AlignSlot),
    /// A scale's point, picked as an align's.
    Point,
    /// A scale's edge, to scale to a length.
    Edge,
    /// A split's tool, of the kind its "Split with" tiles choose: a plane
    /// or face, a body, a sketch's regions or the curves of a line.
    Tool,
    /// A chamfer's edges, each click picking an edge or taking it out.
    Edges,
    /// A shell's faces, each click picking a face or taking it out.
    Faces,
    Nothing,
}

/// Which side of an align a point or direction is on: the body moved, or
/// what it's aligned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlignSide {
    Moved,
    Target,
}

impl AlignSide {
    /// Where it's kept in a pair: the moved side first.
    pub fn index(self) -> usize {
        match self {
            AlignSide::Moved => 0,
            AlignSide::Target => 1,
        }
    }
}

/// Which of a side's references: its point, its direction or its second
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlignRole {
    Point,
    Primary,
    Secondary,
}

impl AlignRole {
    /// The three in the panel's order.
    pub const ALL: [AlignRole; 3] = [AlignRole::Point, AlignRole::Primary, AlignRole::Secondary];

    /// Where it's kept in a side's three.
    pub fn index(self) -> usize {
        match self {
            AlignRole::Point => 0,
            AlignRole::Primary => 1,
            AlignRole::Secondary => 2,
        }
    }

    /// Its field's label in the panel: "Point", "Direction", "Second
    /// direction".
    pub fn label(self) -> &'static str {
        match self {
            AlignRole::Point => "Point",
            AlignRole::Primary => "Direction",
            AlignRole::Secondary => "Second direction",
        }
    }
}

/// One of an align's six references: a side's point, direction or
/// second direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AlignSlot {
    pub side: AlignSide,
    pub role: AlignRole,
}

impl AlignSlot {
    pub const fn new(side: AlignSide, role: AlignRole) -> Self {
        Self { side, role }
    }
}

/// A typed field: a move's offset along a world axis or its angle, a
/// pattern's count or its spacing or total (a length, or a circular
/// one's angle), an align's distance along the target's direction (its
/// turn about it is the angle's field), a scale's factor, its factor
/// along a world axis, or the length its edge is to have, or a
/// chamfer's distance (the first of two), its second distance, or the
/// angle of its cut, or a shell's thickness, or a fillet's radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionField {
    Offset(Axis3),
    Angle,
    Count,
    Spread,
    Distance,
    Factor,
    AxisFactor(Axis3),
    Length,
    ChamferDistance,
    ChamferSecond,
    ChamferAngle,
    Thickness,
    Radius,
}

impl MotionField {
    /// The seventeen, a move's in the panel's order, then a pattern's, an
    /// align's distance, a scale's, a chamfer's, a shell's, then a
    /// fillet's.
    pub const ALL: [MotionField; 17] = [
        MotionField::Offset(Axis3::X),
        MotionField::Offset(Axis3::Y),
        MotionField::Offset(Axis3::Z),
        MotionField::Angle,
        MotionField::Count,
        MotionField::Spread,
        MotionField::Distance,
        MotionField::Factor,
        MotionField::AxisFactor(Axis3::X),
        MotionField::AxisFactor(Axis3::Y),
        MotionField::AxisFactor(Axis3::Z),
        MotionField::Length,
        MotionField::ChamferDistance,
        MotionField::ChamferSecond,
        MotionField::ChamferAngle,
        MotionField::Thickness,
        MotionField::Radius,
    ];

    /// Where it's kept in an array of them all.
    pub fn index(self) -> usize {
        match self {
            MotionField::Offset(Axis3::X) => 0,
            MotionField::Offset(Axis3::Y) => 1,
            MotionField::Offset(Axis3::Z) => 2,
            MotionField::Angle => 3,
            MotionField::Count => 4,
            MotionField::Spread => 5,
            MotionField::Distance => 6,
            MotionField::Factor => 7,
            MotionField::AxisFactor(Axis3::X) => 8,
            MotionField::AxisFactor(Axis3::Y) => 9,
            MotionField::AxisFactor(Axis3::Z) => 10,
            MotionField::Length => 11,
            MotionField::ChamferDistance => 12,
            MotionField::ChamferSecond => 13,
            MotionField::ChamferAngle => 14,
            MotionField::Thickness => 15,
            MotionField::Radius => 16,
        }
    }

    /// Its text field's id: the first one of a panel's, a move's first
    /// offset, a pattern's count or a scale's factor, is
    /// [`VALUE_FIELD`], which takes the focus as the session opens.
    fn id(self) -> iced::widget::Id {
        match self {
            MotionField::Offset(Axis3::X) | MotionField::Count | MotionField::Factor => VALUE_FIELD,
            MotionField::Offset(Axis3::Y) => iced::widget::Id::new("move-y"),
            MotionField::Offset(Axis3::Z) => iced::widget::Id::new("move-z"),
            MotionField::Angle => iced::widget::Id::new("move-angle"),
            MotionField::Spread => iced::widget::Id::new("pattern-spread"),
            MotionField::Distance => iced::widget::Id::new("align-distance"),
            MotionField::AxisFactor(Axis3::X) => iced::widget::Id::new("scale-x"),
            MotionField::AxisFactor(Axis3::Y) => iced::widget::Id::new("scale-y"),
            MotionField::AxisFactor(Axis3::Z) => iced::widget::Id::new("scale-z"),
            MotionField::Length => iced::widget::Id::new("scale-length"),
            MotionField::ChamferDistance => iced::widget::Id::new("chamfer-distance"),
            MotionField::ChamferSecond => iced::widget::Id::new("chamfer-second"),
            MotionField::ChamferAngle => iced::widget::Id::new("chamfer-angle"),
            MotionField::Thickness => iced::widget::Id::new("shell-thickness"),
            MotionField::Radius => iced::widget::Id::new("fillet-radius"),
        }
    }

    /// Its label, the mock's: "X", "Y", "Z", "Angle", "Count"; a
    /// pattern's spread is labelled by its mode.
    fn label(self) -> &'static str {
        match self {
            MotionField::Offset(axis) | MotionField::AxisFactor(axis) => axis.name(),
            MotionField::Angle => "Angle",
            MotionField::Count => "Count",
            MotionField::Spread => "Spacing",
            MotionField::Distance | MotionField::ChamferDistance => "Distance",
            MotionField::Factor => "Factor",
            MotionField::Length => "Length",
            MotionField::ChamferSecond => "Distance 2",
            MotionField::ChamferAngle => "Angle",
            MotionField::Thickness => "Thickness",
            MotionField::Radius => "Radius",
        }
    }
}

/// A change to the move, mirror or pattern being set up, see
/// [`Look::Motion`].
#[derive(Debug, Clone, PartialEq)]
pub enum MotionLook {
    /// Whether clicks pick bodies or the reference: the panel's fields
    /// clicked.
    Picking(MotionPick),
    /// Takes `body` out: its row's cross.
    Drop(BodyId),
    /// The text in a move's field, as typed.
    Input { field: MotionField, text: String },
    /// An origin axis as a move's axis, or an align's direction on the
    /// target: the toolbar's, while the axis or direction is picked.
    OriginAxis(Axis3),
    /// The origin as an align's point on the target, or a scale's point:
    /// the toolbar's, while that point is picked.
    OriginPoint,
    /// Takes an align's reference out: its row's cross.
    Clear(AlignSlot),
    /// An origin plane as a mirror's plane: the toolbar's, while the
    /// plane is picked.
    OriginPlane(OriginPlane),
    /// A ring of a move's handles dragged: turns the bodies about the
    /// world axis `axis` by `angle` and shifts them by `offset`, so they
    /// turn about the handles' centre (the move turns about an axis
    /// through the origin, then shifts), the texts as typed in the
    /// angle's and the offsets' fields.
    Turn {
        axis: Axis3,
        angle: String,
        offset: [String; 3],
    },
    /// A mirror's Create copy: keeps the original, or not.
    Copy,
    /// A linear pattern's Flip direction: runs the other way, or not; an
    /// align's Flip: its directions meet the other way round; a chamfer's
    /// Flip sides: its first distance (and angle) on the other face.
    Flip,
    /// A pattern's Join to original: its copies in their bodies, or each
    /// a body of its own.
    Join,
    /// How a pattern's copies are spread.
    Mode(PatternMode),
    /// How a scale scales: by one factor, one per axis, or to an edge's
    /// length.
    ScaleMode(ScaleMode),
    /// A scale to an edge's length: along the world axis the edge runs
    /// along only, or not.
    AxisOnly,
    /// What a split splits with: a plane or face, a body, a sketch's
    /// regions or a line of its curves (its "Split with" tiles).
    SplitWith(SplitMode),
    /// Picks the region `region` of `sketch` for a split's tool, or takes
    /// it out: clicked in the viewport.
    SplitRegion { sketch: FeatureId, region: usize },
    /// Picks the curve `curve` of `sketch` for a split's line, or takes
    /// it out: clicked in the viewport.
    SplitCurve { sketch: FeatureId, curve: Id },
    /// Which piece of a split keeps the body's id.
    Original(Side),
    /// Which pieces of a split stay.
    Keep(Keep),
    /// How a chamfer is sized: its Type tiles.
    ChamferType(ChamferType),
    /// A chamfer's Tangent chain: each edge takes in the edges running
    /// on smoothly from it, or not.
    Chain,
    /// Takes a chamfer's edge out: its row's cross.
    DropEdge(EdgeRef),
    /// Takes a shell's face out: its row's cross.
    DropFace(FaceRef),
    /// Which way a shell's walls grow from the body's faces: its
    /// Direction tiles.
    ShellDirection(ShellDirection),
    /// Drops the move or mirror being set up, changing nothing: Cancel,
    /// or `Esc`.
    Cancel,
}

/// The move, mirror or pattern being set up, and how it's shown.
#[derive(Debug, Clone)]
pub struct MotionState<'a> {
    pub kind: MotionKind,
    /// The name of the feature edited, or none for a new one.
    pub editing: Option<&'a str>,
    /// The bodies picked, sorted by id as the document keeps them.
    pub bodies: Vec<CombineBody<'a>>,
    /// What a click picks.
    pub picking: MotionPick,
    /// Its fields ([`MotionField::index`]): a move's offsets along X, Y
    /// and Z and its angle, a pattern's count and spread, an align's
    /// distance, a scale's factors and length, a chamfer's distances
    /// and angle, a shell's thickness, a fillet's radius.
    pub fields: [TypedField<'a>; 17],
    /// The reference's name, "Z axis", "Edge of Body 1", "XY plane",
    /// "Extrude 1's end", if there's one.
    pub reference: Option<String>,
    /// Where the reference is, as a point on it and its direction (a
    /// plane's normal), not unit, if that's known: an origin axis or
    /// plane, or where regenerating the preview found an edge or face.
    pub line: Option<[DVec3; 2]>,
    /// Where the bodies are, the corners of their box in the model shown,
    /// if it shows them: the axis and plane are drawn across it, and a
    /// move's handles at its centre.
    pub bounds: Option<[DVec3; 2]>,
    /// Where a move's handles stand, if the app knows: a point of the
    /// bodies found once, taken where the move as set up takes it, so
    /// they stay put as a ring turns the bodies about them and move with
    /// the offsets at once. Without it, the centre of
    /// [`MotionState::bounds`].
    pub centre: Option<DVec3>,
    /// A move's axis, if it's a world axis: while the move turns, only
    /// that axis's ring of the handles turns it further.
    pub origin_axis: Option<Axis3>,
    /// The design's units, which the handles' snapped offsets are typed
    /// in.
    pub units: LengthUnit,
    /// A mirror's Create copy.
    pub keep_original: bool,
    /// A linear pattern's Flip direction.
    pub flip: bool,
    /// A pattern's Join to original.
    pub join: bool,
    /// What the panel warns of, if anything, the mock's words: a linear
    /// pattern's copies, each a body of its own, overlapping ("The copies
    /// overlap (12 mm long this way): tick Join to original to merge
    /// them"). Shown where nothing else is.
    pub warning: Option<String>,
    /// How a pattern's copies are spread.
    pub mode: PatternMode,
    /// Why a pattern's spread is refused where its own text isn't, the
    /// mock's words under its field: "The pattern runs past 1000000 mm",
    /// "4 copies 120° apart go past a full turn".
    pub spread_error: Option<String>,
    /// A whole pattern's copies as the status bar says them, "4 × 25 mm
    /// along X axis", "6 × 60° about Z axis" ([`pattern_copies`]).
    pub copies: Option<String>,
    /// What's still to be done before it can be committed, if anything:
    /// "pick the bodies to move", for the status bar.
    pub need: Option<&'static str>,
    /// Why it can't be committed as set up, if its own check refuses it:
    /// shown in place of [`MotionState::error`].
    pub refused: Option<String>,
    /// Why the preview failed, if it did.
    pub error: Option<&'a str>,
    /// The button framing the camera on the geometry of why the preview
    /// failed, or going back from it, if that geometry has a box.
    pub show_error: Option<Framing>,
    /// Whether sketch edits have waited on the solver long enough to say
    /// so: OK waits for them, and the panel says why.
    pub checking: bool,
    /// Whether OK (and `Enter`) can be pressed: not while the preview
    /// failed.
    pub ready: bool,
    /// Whether Add anyway can be pressed: the preview failed, and it
    /// could be committed otherwise.
    pub accept: bool,
    /// Whether the document can be changed.
    pub editable: bool,
    /// The row of the panel the cursor is over, if any: the viewport
    /// lights it up too.
    pub hover: Option<PanelHover>,
    /// An align's own parts, for an align.
    pub align: Option<Box<AlignView<'a>>>,
    /// A scale's own parts, for a scale.
    pub scale: Option<Box<ScaleView<'a>>>,
    /// A split's own parts, for a split.
    pub split: Option<Box<SplitView<'a>>>,
    /// A chamfer's own parts, for a chamfer.
    pub chamfer: Option<Box<ChamferView>>,
    /// A shell's own parts, for a shell.
    pub shell: Option<Box<ShellView>>,
    /// A fillet's own parts, for a fillet.
    pub fillet: Option<Box<FilletView>>,
}

impl<'a> MotionState<'a> {
    /// The panel's title: the feature edited's name, or "New move".
    pub fn title(&self) -> &'a str {
        self.editing.unwrap_or(self.kind.new_title())
    }
}

/// A move's axis as the panel and notes name it: "Z axis", "Edge of
/// Body 1", "Extrude 1's side".
pub fn axis_name(document: &Document, axis: &AxisRef) -> String {
    match axis {
        AxisRef::Origin(axis) => format!("{} axis", axis.name()),
        AxisRef::Edge(edge) => edge_axis_name(document, edge),
        AxisRef::Face(face) => face_name(document, face),
    }
}

/// A mirror's plane as the panel and notes name it: "XY plane",
/// "Extrude 1's end".
pub fn plane_name(document: &Document, plane: &PlaneRef) -> String {
    match plane {
        PlaneRef::Origin(plane) => format!("{} plane", plane.name()),
        PlaneRef::Face(face) => face_name(document, face),
    }
}

/// A mirror's plane as its Timeline row notes it, the mock's short form:
/// "XY", "Extrude 1's end".
pub(crate) fn plane_short(document: &Document, plane: &PlaneRef) -> String {
    match plane {
        PlaneRef::Origin(plane) => plane.name().to_owned(),
        PlaneRef::Face(face) => face_name(document, face),
    }
}

/// A move's Timeline note, the mock's: how far it shifts in all and its
/// turn, "82.5 mm 30°", either left out when it's none; "0 mm" for a
/// move that does nothing.
pub(crate) fn move_note(moved: &varde_document::Move, units: LengthUnit) -> String {
    let shift = moved.offset_vector().length();
    let angle = moved.turn.as_ref().map(|(_, angle)| angle.value);
    let mut parts = Vec::new();
    if shift != 0.0 || angle.is_none_or(|angle| angle == 0.0) {
        parts.push(varde_expr::format(shift, Some(units.into())));
    }
    if let Some(angle) = angle.filter(|&angle| angle != 0.0) {
        parts.push(varde_expr::format(angle, Some(DEGREES)));
    }
    parts.join(" ")
}

/// What the status bar says of a selected move, the mock's: "Body 1 by
/// 10, 0, -5 mm, 30° about Z axis".
pub(crate) fn move_info(document: &Document, moved: &varde_document::Move) -> String {
    let units = document.units();
    let bodies = body_names(document, &moved.bodies);
    let mut parts = Vec::new();
    if moved.offset.iter().any(|value| value.value != 0.0) {
        let symbol = format!(" {}", Unit::from(units).symbol());
        let [x, y, z] = moved.offset.each_ref().map(|value| {
            let shown = varde_expr::format(value.value, Some(units.into()));
            shown
                .strip_suffix(&symbol)
                .map(str::to_owned)
                .unwrap_or(shown)
        });
        parts.push(format!("by {x}, {y}, {z}{symbol}"));
    }
    if let Some((axis, angle)) = &moved.turn
        && angle.value != 0.0
    {
        let angle = varde_expr::format(angle.value, Some(DEGREES));
        parts.push(format!("{angle} about {}", axis_name(document, axis)));
    }
    if parts.is_empty() {
        format!("{bodies} not moved")
    } else {
        format!("{bodies} {}", parts.join(", "))
    }
}

/// What the status bar says of a selected mirror, the mock's: "Body 1
/// across XY plane · copy".
pub(crate) fn mirror_info(document: &Document, mirror: &varde_document::Mirror) -> String {
    let bodies = body_names(document, &mirror.bodies);
    let copy = if mirror.keep_original { " · copy" } else { "" };
    format!(
        "{bodies} across {}{copy}",
        plane_name(document, &mirror.plane)
    )
}

/// What the status bar says of a selected pattern, the mock's row info:
/// "Body 1 · 4 × 25 mm along X axis · joined", "Body 1 · 3 × 25 mm along
/// X axis, flipped", "Body 1 · 6 × 60° about Z axis".
pub(crate) fn pattern_info(document: &Document, pattern: &Pattern) -> String {
    let bodies = body_names(document, &pattern.bodies);
    format!("{bodies} · {}", pattern_copies(document, pattern))
}

/// A pattern's copies as the mock's row info says them, after its
/// bodies: the count and the step between neighbours, and the axis,
/// "4 × 25 mm along X axis" (the spacing's size, ", flipped" for a
/// negative one), "6 × 60° about Z axis" (as the copies turn,
/// [`Pattern::step_degrees`]), and " · joined" for copies joined to
/// their originals.
pub fn pattern_copies(document: &Document, pattern: &Pattern) -> String {
    let count = pattern.kind.count_value().value;
    let axis = axis_name(document, pattern.kind.axis());
    let joined = if pattern.joins() { " · joined" } else { "" };
    match &pattern.kind {
        PatternKind::Linear { spacing, .. } => {
            let step = varde_expr::format(spacing.value.abs(), Some(document.units().into()));
            let flipped = if spacing.value < 0.0 { ", flipped" } else { "" };
            format!("{count} × {step} along {axis}{flipped}{joined}")
        }
        PatternKind::Circular { .. } => {
            let step = pattern.step_degrees().unwrap_or(0.0).to_radians();
            let step = varde_expr::format(step, Some(DEGREES));
            format!("{count} × {step} about {axis}{joined}")
        }
    }
}

/// A pattern's Timeline note, the mock's: its count, "×4".
pub(crate) fn pattern_note(pattern: &Pattern) -> String {
    format!("×{}", pattern.kind.count_value().value)
}

/// What an align's target is on: the body its point or, failing that,
/// a direction is on, or "the origin". The Timeline's note, "to Body 2".
pub(crate) fn align_note(document: &Document, align: &varde_document::Align) -> String {
    let to = &align.to;
    let body = (to.point.body()).or_else(|| to.directions().find_map(|direction| direction.body()));
    match body {
        Some(body) => format!("to {}", body_names(document, &[body])),
        None => "to the origin".to_owned(),
    }
}

/// What the status bar says of a selected align: "Body 1 to Body 2".
pub fn align_info(document: &Document, align: &varde_document::Align) -> String {
    let moved = body_names(document, &[align.body]);
    format!("{moved} {}", align_note(document, align))
}

/// A scale's Timeline note: "×2", "×1 · 1 · 2", "edge → 50 mm"
/// (lengths in `units`), the factor written as a pattern's count is.
pub(crate) fn scale_note(scale: &varde_document::Scale, units: LengthUnit) -> String {
    use varde_document::ScaleFactor;
    let factor = |value: &varde_expr::Value| varde_expr::format(value.value, None);
    match &scale.factor {
        ScaleFactor::Uniform(value) => format!("×{}", factor(value)),
        ScaleFactor::PerAxis(values) => format!("×{}", values.each_ref().map(factor).join(" · ")),
        ScaleFactor::EdgeLength { length, .. } => format!(
            "edge → {}",
            varde_expr::format(length.value, Some(units.into()))
        ),
    }
}

/// What the status bar says of a selected scale: "Body 1 ×2".
pub fn scale_info(document: &Document, scale: &varde_document::Scale, units: LengthUnit) -> String {
    let bodies = body_names(document, &scale.bodies);
    format!("{bodies} {}", scale_note(scale, units))
}

/// A split's Timeline note: what it splits with, "by XY", "by Body 3",
/// "by a face", "by Sketch 2", and the side kept where it keeps one
/// ("by XY · front only").
pub(crate) fn split_note(document: &Document, split: &varde_document::Split) -> String {
    use varde_document::{Keep, SplitTool};
    let tool = match &split.tool {
        SplitTool::Plane(plane) => plane_short(document, plane),
        SplitTool::Face(face) => face_name(document, face),
        SplitTool::Body(body) => body_names(document, std::slice::from_ref(body)),
        SplitTool::Regions { sketch, .. } | SplitTool::Chain { sketch, .. } => (document
            .feature(*sketch))
        .map_or_else(|| "a sketch".to_owned(), |feature| feature.name.clone()),
    };
    match split.keep {
        Keep::Both => format!("by {tool}"),
        Keep::Front => format!("by {tool} · front only"),
        Keep::Back => format!("by {tool} · back only"),
    }
}

/// What the status bar says of a selected split: "Body 1 by XY".
pub fn split_info(document: &Document, split: &varde_document::Split) -> String {
    let body = body_names(document, std::slice::from_ref(&split.body));
    format!("{body} {}", split_note(document, split))
}

/// The names of `bodies` of `document`, joined: "Body 1, Body 2".
pub(crate) fn body_names(document: &Document, bodies: &[BodyId]) -> String {
    let names: Vec<&str> = (bodies.iter())
        .map(|&body| {
            document
                .body(body)
                .map_or("a body", |body| body.name.as_str())
        })
        .collect();
    names.join(", ")
}

/// What the status bar says of the move, mirror or pattern being set
/// up: "Body 1 · 10 mm, 0 mm, 0 mm", "Body 1 · 4 × 25 mm along X axis",
/// or what's still to do.
pub(crate) fn status_info(state: &MotionState<'_>) -> String {
    if let Some(need) = state.need {
        return need.to_owned();
    }
    let names: Vec<&str> = state.bodies.iter().map(|body| body.name).collect();
    let bodies = names.join(", ");
    match (state.kind, &state.reference) {
        (MotionKind::Mirror, Some(plane)) => format!("{bodies} across {plane}"),
        (MotionKind::Move, Some(axis)) => {
            let angle = state.fields[MotionField::Angle.index()]
                .value
                .filter(|&angle| angle != 0.0);
            match angle {
                Some(angle) => format!(
                    "{bodies} · {} about {axis}",
                    varde_expr::format(angle, Some(DEGREES))
                ),
                None => bodies,
            }
        }
        (MotionKind::LinearPattern | MotionKind::CircularPattern, Some(_)) => match &state.copies {
            Some(copies) => format!("{bodies} · {copies}"),
            None => bodies,
        },
        (MotionKind::Align, _) => (state.align.as_ref())
            .and_then(|align| align.info.clone())
            .unwrap_or(bodies),
        (MotionKind::Scale, _) => (state.scale.as_ref())
            .and_then(|scale| scale.info.clone())
            .unwrap_or(bodies),
        (MotionKind::Split, _) => (state.split.as_ref())
            .and_then(|split| split.info.clone())
            .unwrap_or(bodies),
        (MotionKind::Chamfer, _) => (state.chamfer.as_ref())
            .and_then(|chamfer| chamfer.info.clone())
            .unwrap_or(bodies),
        (MotionKind::Shell, _) => (state.shell.as_ref())
            .and_then(|shell| shell.info.clone())
            .unwrap_or(bodies),
        (MotionKind::Fillet, _) => (state.fillet.as_ref())
            .and_then(|fillet| fillet.info.clone())
            .unwrap_or(bodies),
        _ => bodies,
    }
}

/// The floating panel of the move, mirror or pattern being set up, the
/// mock's: Bodies; a move's Translate (X, Y, Z) and Rotate (Axis, Angle);
/// a mirror's Plane and Create copy; a linear pattern's Direction and
/// Flip direction, a circular one's Axis, then Copies: the Count, how
/// they're spread, the spacing, total or angle (none for Full 360°), and
/// Join to original.
pub(crate) fn panel<'a>(state: &MotionState<'a>) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));

    let picking_bodies = state.picking == MotionPick::Bodies;
    let pick_bodies = send(MotionLook::Picking(MotionPick::Bodies));
    let rows: Vec<_> = (state.bodies.iter())
        .map(|body| {
            picked_row(
                Icon::Body,
                body.name,
                None,
                send(MotionLook::Drop(body.body)),
                pick_bodies.clone(),
                PanelHover::Body(body.body),
                state.hover,
            )
        })
        .collect();
    // An align moves one body; a split splits one.
    let (bodies_label, bodies_place) = match state.kind {
        MotionKind::Align | MotionKind::Split => ("Body", "Click a body"),
        _ => ("Bodies", "Click bodies"),
    };
    let place = (rows.is_empty() || picking_bodies).then(|| bodies_place.to_owned());
    let bodies = field(
        bodies_label,
        pick_field(rows, place, picking_bodies, pick_bodies),
    );

    let picking_reference = state.picking == MotionPick::Reference;
    let pick_reference = send(MotionLook::Picking(MotionPick::Reference));
    let (reference_label, reference_icon, reference_place) = match state.kind {
        MotionKind::Move | MotionKind::CircularPattern => {
            ("Axis", Icon::SeAxis, "Click an axis or edge")
        }
        MotionKind::LinearPattern => ("Direction", Icon::SeAxis, "Click an axis or edge"),
        // An align's references are its own fields: this one isn't shown.
        MotionKind::Mirror
        | MotionKind::Align
        | MotionKind::Scale
        | MotionKind::Split
        | MotionKind::Chamfer
        | MotionKind::Shell
        | MotionKind::Fillet => ("Plane", Icon::SePlane, "Click a plane or face"),
    };
    let reference_row = state.reference.clone().map(|name| {
        picked_row(
            reference_icon,
            name,
            None,
            None,
            pick_reference.clone(),
            PanelHover::Axis,
            state.hover,
        )
    });
    let reference_place =
        (reference_row.is_none() || picking_reference).then(|| reference_place.to_owned());
    let reference = field(
        reference_label,
        pick_field(
            reference_row.into_iter().collect(),
            reference_place,
            picking_reference,
            pick_reference,
        ),
    );

    let field_named = |which: MotionField, label: &'a str| {
        let input = editable.then_some(move |text| {
            Message::Look(Look::Motion(MotionLook::Input { field: which, text }))
        });
        value_field(
            label,
            which.id(),
            state.fields[which.index()],
            input,
            Message::Edit(Edit::CommitMotion),
            Message::Look(Look::Motion(MotionLook::Cancel)),
        )
    };
    let field_of = |which: MotionField| field_named(which, which.label());
    let body: Element<'a, Message> = match state.kind {
        MotionKind::Align => align::body(state, bodies, field_named),
        MotionKind::Scale => scale::body(state, bodies, field_named),
        MotionKind::Split => split::body(state, bodies),
        MotionKind::Chamfer => chamfer::body(state, field_named),
        MotionKind::Shell => shell::body(state, field_named),
        MotionKind::Fillet => fillet::body(state, field_named),
        MotionKind::Move => {
            let translate = MotionField::ALL[..3].iter().map(|&which| field_of(which));
            column![
                bodies,
                section("Translate"),
                column(translate).spacing(8),
                section("Rotate"),
                reference,
                field_of(MotionField::Angle),
            ]
            .spacing(10)
            .into()
        }
        MotionKind::Mirror => {
            let copy = toggle(
                Icon::TkCopy,
                "Create copy",
                state.keep_original,
                send(MotionLook::Copy),
                Some("Keep the original too"),
            );
            column![bodies, reference, copy].spacing(10).into()
        }
        MotionKind::LinearPattern | MotionKind::CircularPattern => {
            let flip = (state.kind == MotionKind::LinearPattern).then(|| {
                toggle(
                    Icon::TkFlip,
                    "Flip direction",
                    state.flip,
                    send(MotionLook::Flip),
                    None,
                )
            });
            let modes = PatternMode::of(state.kind).iter().map(|&mode| {
                tile(
                    mode.icon(state.kind),
                    mode.label(),
                    state.mode == mode,
                    send(MotionLook::Mode(mode)),
                )
            });
            let spread = (state.mode != PatternMode::Full).then(|| {
                let label = match state.mode {
                    PatternMode::Total => "Total",
                    _ => "Spacing",
                };
                // The mock's error for the spread as a whole, under its
                // field, where its text has none of its own.
                let note = (state.spread_error.clone())
                    .filter(|_| state.fields[MotionField::Spread.index()].error.is_none())
                    .map(|note| {
                        text(sentence(&note).into_owned())
                            .size(11.5)
                            .wrapping(Wrapping::WordOrGlyph)
                            .style(theme::danger_text)
                    });
                column![field_named(MotionField::Spread, label), note].spacing(3)
            });
            let join = toggle(
                Icon::TkJoin,
                "Join to original",
                state.join,
                send(MotionLook::Join),
                Some("One body; otherwise each copy is its own"),
            );
            column![
                bodies,
                reference,
                flip,
                section("Copies"),
                field_of(MotionField::Count),
                tiles(modes),
                spread,
                join,
            ]
            .spacing(10)
            .into()
        }
    };
    let message = footer_message(
        state.kind.noun(),
        state.refused.clone(),
        state.error,
        state.show_error,
        state.accept.then_some(Message::Edit(Edit::AcceptError)),
        state.checking,
    )
    .or_else(|| {
        // The mock's warning, under the failure where there's one: here
        // only where there's no other message.
        (state.warning.clone())
            .map(|warning| Footer::Text(message_text(warning, theme::warning_text)))
    });
    operation_panel(Parts {
        icon: state.kind.icon(),
        title: state.title(),
        body,
        message,
        ok: state.ready.then_some(Message::Edit(Edit::CommitMotion)),
        cancel: Message::Look(Look::Motion(MotionLook::Cancel)),
        close: false,
    })
}

/// A section's heading in the panel, the mock's `h4`: as a field's
/// label.
fn section(name: &str) -> iced::widget::Text<'_> {
    label(name)
}

/// A row of a list of references picked ([`picks_field`]): its icon,
/// name and what's beside it, what its cross sends, and what hovering it
/// lights.
struct PickedRow {
    icon: Icon,
    name: String,
    meta: Option<String>,
    drop: MotionLook,
    hover: PanelHover,
}

/// The panel's field `label` of references picked by `pick` (a blend's
/// edges, a face session's faces): a row each, pressing one picking
/// them, and while picking, or with none, where to click (`place`).
fn picks_field<'a>(
    state: &MotionState<'a>,
    pick: MotionPick,
    label: &'a str,
    place: &str,
    rows: impl Iterator<Item = PickedRow>,
) -> Element<'a, Message> {
    let editable = state.editable;
    let send = |look: MotionLook| editable.then_some(Message::Look(Look::Motion(look)));
    let on = state.picking == pick;
    let press = send(MotionLook::Picking(pick));
    let rows: Vec<_> = rows
        .map(|row| {
            picked_row(
                row.icon,
                row.name,
                row.meta,
                send(row.drop),
                press.clone(),
                row.hover,
                state.hover,
            )
        })
        .collect();
    let place = (rows.is_empty() || on).then(|| place.to_owned());
    field(label, pick_field(rows, place, on, press))
}

mod align;
pub use align::{AlignMark, AlignView, direction_name, point_name};
mod scale;
pub use scale::{ScaleMode, ScaleView};
mod split;
pub use split::{SketchLines, SplitMode, SplitPiece, SplitView};
mod blend;
pub use blend::{BlendEdge, BlendEdges};
mod chamfer;
pub use chamfer::{ChamferType, ChamferView};
mod faces;
pub use faces::{PickedFace, PickedFaces};
mod fillet;
pub use fillet::FilletView;
mod shell;
pub use shell::{ShellDirection, ShellView};

#[cfg(test)]
mod tests;
