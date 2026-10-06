//! UI code: iced widgets and layout.
//!
//! The view is a pure function of application state. It produces
//! [`Message`]s but never mutates anything itself; that is the job of the
//! `app` crate.

// Proving wgpu types `Send + Sync` for the shader pipeline exceeds the default
// limit on recent nightlies.
#![recursion_limit = "256"]

mod anchors;
mod chamfer;
mod chrome;
mod combine;
mod constrain;
mod context_menu;
mod controls;
pub mod dimension;
mod document;
mod errors;
mod escape;
mod extrude;
mod face_draft;
mod fillet;
mod hit;
mod icons;
mod loft;
pub use loft::loft_info;
mod measure;
mod motion;
mod mouse_only;
mod offset_face;
mod operation_panel;
mod overlaps;
mod panels;
mod pick;
mod plane_pick;
#[cfg(any(test, feature = "probe"))]
pub mod probe;
mod projection;
mod rail;
mod regenerating;
mod revolve;
mod select;
mod shell;
mod shortcut;
mod snap;
pub mod spline;
mod split;
mod status;
mod sweep;
mod toast;
pub use sweep::sweep_info;
#[cfg(test)]
mod testing;
mod theme;
mod thumbnail;
mod toolbar;
pub use toolbar::motion_picks_origin_planes;
pub mod typed;
mod view_cube;
mod viewport;
mod welcome;

use std::path::PathBuf;

use glam::DVec2;
use varde_document::{BodyId, FaceRef, FeatureId, Opacity, OriginPlane, Tint, Tolerance};
use varde_expr::LengthUnit;
use varde_render::{Projection, Shading, View};
use varde_sketch::{Id, Sketch};

pub use chamfer::chamfer_info;
pub use combine::{CombineBody, CombineLook, CombinePick, CombineState};
pub use constrain::{ConstraintKind, ConstraintSet};
pub use document::{
    ActiveTool, CURVED_FACE, Damage, DamagedFile, DeletePrompt, DocumentState, LinkRow, MeshStatus,
    NamePrompt, Overlay, RecoveredChanges, RefusedEdit, SketchState, ValueField, ValueTarget,
    body_tint, document,
};
pub use errors::{ShownError, ShownErrors};
pub use extrude::{Distance, ExtentKind, ExtrudeLook, ExtrudeState, Handle, snap_step};
pub use face_draft::draft_info;
pub use fillet::fillet_info;
pub use icons::LOGO_SVG;
pub use measure::{
    MeasureLook, MeasureSlot, MeasureState, Outcome, Picked as MeasuredPick, Value as MeasureValue,
    between_values, face_kind, values as measure_values,
};
pub use motion::{
    AlignMark, AlignRole, AlignSide, AlignSlot, AlignView, BlendEdge, BlendEdges, ChamferType,
    ChamferView, DraftView, FaceHandle, FilletView, KnobPath, KnobRadius, KnobScale, KnobSnap,
    KnobTone, LoftSection, LoftShape, LoftView, MotionField, MotionKind, MotionLook, MotionPick,
    MotionState, OffsetFaceView, OpKnob, PatternMode, PickedFace, PickedFaces, ScaleMode,
    ScaleView, ShellDirection, ShellView, SketchLines, SplitMode, SplitPiece, SplitView, SweepPart,
    SweepPath, SweepView, align_info, axis_name, direction_name, pattern_copies, plane_name,
    point_name, scale_info, split_info,
};
pub use offset_face::offset_info;
pub use operation_panel::{
    BodyTarget, Candidate, Framing, OperationKind, PANEL_BODY, PanelHover, TypedField,
};
pub use overlaps::{OverlapItem, OverlapItems, OverlapNote, OverlapTick, Overlaps};
pub use pick::{
    EDGE_REACH, ModelHighlight, Pick, PickIndex, Picked, Picks, SNAP_REACH, Snapped, VERTEX_REACH,
};
pub use plane_pick::{Naming, PlanePick, Shown, Unnamed, face_name, plane_note};
pub use rail::{RAIL_LIST, RailLook, RailOpen, RailSpot, rail_rows, rail_sets};
pub use revolve::{
    Angle, EDGE_NOT_STRAIGHT, EDGE_OFF_PLANE, RevolveLook, RevolvePick, RevolveState, TurnKind,
    axis_edge,
};
pub use select::{Selected, Selection, SelectionMode, SketchItem};
pub use shell::shell_info;
pub use shortcut::{
    Binding, DocumentKeys, Held, claimed, document_bindings, escapes, pressed, welcome_bindings,
};
pub use snap::{Inference, Level, SNAP_TOLERANCE, Snap, Target};
pub use status::{STATUS_BAR_HEIGHT, STATUS_BAR_ROOM};
pub use theme::{Mode, SIDE_PANEL_WIDTH, ThemeChoice, theme as iced_theme};
pub use thumbnail::{
    THUMBNAIL_ROOM, THUMBNAIL_SCALE, ThumbnailImages, ThumbnailRequest, thumbnail_shot,
};
pub use toast::toast;
pub use viewport::ModelPicking;
pub use viewport::warm_up;
pub use welcome::{
    CardKey, DamagedPrompt, DeleteFromBrowserPrompt, DesignCard, Downloads, PanicNote, RecentCard,
    SampleCard, StorageNote, WelcomeState, welcome,
};

/// The text field a dimension's value is typed in, placing it or editing
/// it in place: there's one at a time, focused as it opens.
pub const VALUE_FIELD: iced::widget::Id = iced::widget::Id::new("dimension-value");

/// The text field of the Save As dialog's name, on the web, focused as it
/// opens.
pub const NAME_FIELD: iced::widget::Id = iced::widget::Id::new("design-name");

/// The text field a feature, sketch or body is renamed in, in its row of
/// the side panel: there's one at a time, focused as it opens.
pub const RENAME_FIELD: iced::widget::Id = iced::widget::Id::new("rename");

/// What shows in the name's place of a design with no name, never saved.
pub const NOT_SAVED: &str = "Not saved";

/// Where a design is kept, on the web, as the bar under the file cell says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// In browser storage, by name.
    Browser,
    /// In a file on the user's computer, through the File System Access
    /// API.
    Computer,
}

/// Where the Save As dialog saves a design, on the web.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePlace {
    /// In browser storage, under the name typed.
    Browser,
    /// In a file on the computer, which the system's save picker asks for:
    /// only where the File System Access API is.
    Computer,
}

/// What the user asks for through the view, grouped by what acts on it.
/// The app has messages of its own on top, from its subscriptions, lanes
/// and dialogs.
#[derive(Debug, Clone)]
pub enum Message {
    Welcome(Welcome),
    File(File),
    Edit(Edit),
    Look(Look),
    /// Goes on to the next [`ThemeChoice`].
    CycleTheme,
    /// Shows the status bar's hints for the mouse, or hides them: the
    /// view options menu's Mouse hints.
    ToggleMouseHints,
    /// Shows the edges the model hides, dashed, or hides them: the view
    /// options menu's Hidden edges.
    ToggleHiddenEdges,
    /// Draws the model's edges as `Edges` says, from the view options
    /// menu's Edges submenu.
    SetEdges(Edges),
    /// Lights the model's faces as `Shading` says, from the view options
    /// menu's Shading submenu.
    SetShading(Shading),
    /// Puts the text on the clipboard: a measured value with its unit,
    /// from its copy button.
    Copy(String),
}

/// What the view options menu turns on and off, and the theme button
/// picks, kept by the app for every document. Both hints and hidden edges
/// on to start with, the edges and shading the defaults and the theme the
/// system's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Whether the status bar shows the hints of the mouse.
    pub mouse_hints: bool,
    /// Whether the viewport shows the edges the model hides, dashed.
    pub hidden_edges: bool,
    /// Which of the model's edges the viewport draws.
    pub edges: Edges,
    /// How the viewport lights the model's faces.
    pub shading: Shading,
    pub theme: ThemeChoice,
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            mouse_hints: true,
            hidden_edges: true,
            edges: Edges::Default,
            shading: Shading::Regular,
            theme: ThemeChoice::default(),
        }
    }
}

/// Which of the model's edges the viewport draws, besides the feature
/// edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edges {
    /// Only the feature edges.
    #[default]
    Default,
    /// Every patch's edges too, faint: [`varde_render::Frame::wireframe`].
    Wireframe,
    /// Every triangle's edges too, faint:
    /// [`varde_render::Frame::tessellation`].
    Tessellation,
}

/// A submenu of the view options menu, open to its left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewSubmenu {
    Shading,
    Edges,
}

/// What the user asks for on the welcome screen.
#[derive(Debug, Clone)]
pub enum Welcome {
    NewDesign,
    Open,
    OpenPath(PathBuf),
    /// Opens a design kept in the store: left behind by a crash.
    OpenStored(PathBuf),
    /// Deletes a design kept in the store.
    DiscardStored(PathBuf),
    /// Opens a design saved in browser storage, by its file name there.
    OpenFromBrowser(String),
    /// Deletes a design saved in browser storage: at once if its latest
    /// is downloaded, otherwise once the user agrees.
    DeleteFromBrowser(String),
    /// Downloads a design saved in browser storage, as it's saved.
    DownloadFromBrowser(String),
    /// Opens the sample design of this index, built into the web build,
    /// as a new design.
    OpenSample(usize),
    /// Agrees to delete the design asked about.
    ConfirmDelete,
    /// Keeps the design asked about.
    CancelDelete,
    /// Opens the newest save that can be read of the damaged file the
    /// prompt asks about.
    OpenDamaged,
    /// Opens the save found after the damage instead, see
    /// [`DamagedPrompt::found`].
    OpenFound,
    /// Leaves the damaged file the prompt asks about as it is, unopened.
    CancelDamaged,
    /// Shows the whole of the panic recorded, see [`PanicNote`].
    ShowPanic,
    /// Closes what [`Welcome::ShowPanic`] showed.
    ClosePanic,
    /// Deletes the panic recorded.
    DiscardPanic,
}

/// What the user asks of the document's file: saving it, leaving it, and
/// the unsaved changes a crashed session left.
#[derive(Debug, Clone)]
pub enum File {
    CloseDocument,
    /// Saves the document to its file, or asks where if it has none.
    Save,
    /// Asks where to save the document, then saves it there.
    SaveAs,
    /// Asks where to export the visible bodies as a 3MF file, then writes
    /// them there; on the web without the File System Access API,
    /// downloads it.
    Export,
    /// On the web: downloads the design as it is, which changes nothing
    /// in storage.
    Download,
    /// On the web: asks for a new name for the design in browser storage.
    Rename,
    /// The Save As dialog's name, as typed.
    Name(String),
    /// Where the Save As dialog saves the design, as chosen.
    Place(SavePlace),
    /// Saves, or renames, as the Save As dialog says.
    ConfirmName,
    /// Closes the Save As dialog without saving.
    CancelName,
    /// The answer to the prompt about unsaved changes.
    Unsaved(Unsaved),
    /// Applies the unsaved changes a crashed session left of the document.
    RestoreChanges,
    /// Throws those changes away.
    DiscardChanges,
}

/// What the user asks of the open document itself: its edits, and the
/// menu and banner that lead to more.
#[derive(Debug, Clone)]
pub enum Edit {
    /// Hides why the last save failed.
    DismissSaveError,
    /// Hides why the last export failed.
    DismissExportError,
    /// Hides the banner saying the file was found damaged.
    DismissDamage,
    /// Hides the sketch edit the solver refused after its sketch was
    /// left.
    DismissRefusedEdit,
    ToggleFileMenu,
    /// Removes the body and the feature making it, as one undo step, at
    /// once if nothing else goes with them, or else asking first (see
    /// [`DeletePrompt`]).
    RemoveBody(BodyId),
    ToggleVisible(BodyId),
    /// The origin plane picked while picking a plane: a new sketch on it,
    /// edited, or the sketch whose plane is changed put on it (see
    /// [`PlanePick`]).
    PlanePicked(OriginPlane),
    /// The flat face picked in the viewport while picking a plane: a new
    /// sketch on it, edited, or the sketch whose plane is changed put on
    /// it.
    FacePicked(FaceRef),
    /// Adds a sketch on the face selected in the model, if it's the only
    /// thing selected and it's flat, and edits it: `S`, or the rail's
    /// Sketch on face.
    SketchOnSelection,
    /// Removes the feature and the bodies it makes, as one undo step, at
    /// once if no other feature depends on it, or else asking first (see
    /// [`DeletePrompt`]).
    RemoveFeature(FeatureId),
    /// Removes the sketches selected in Objects together, as one undo
    /// step, as [`Edit::RemoveFeature`] does one.
    RemoveObjects,
    /// Removes what the delete prompt lists, as one undo step: its
    /// Delete button.
    ConfirmDelete,
    ToggleFeatureVisible(FeatureId),
    /// A click in the sketch being edited with its tool.
    ToolClick(ToolClick),
    /// Ends dragging geometry in the sketch being edited, where it was
    /// dragged to.
    DropGeometry,
    /// Deletes what's selected in the sketch being edited, and what
    /// depends on it.
    DeleteSelection,
    /// Deletes a point or curve of the sketch being edited from its row's
    /// context menu: the selection if it's among it, else it alone.
    DeleteItem(Id),
    /// Gives each curve made from a point the curves share a point of its
    /// own, coincident with it, from the point's row's context menu.
    DetachPoint(Id),
    /// Turns the curves selected in the sketch being edited between normal
    /// and construction geometry, or while a tool is in use, the shapes it
    /// draws next.
    ToggleConstruction,
    /// Constrains the geometry selected in the sketch being edited so, if
    /// it fits, see [`ConstraintKind::make`].
    Constrain(ConstraintKind),
    /// Constrains the geometry selected so, or takes the constraint off
    /// if all of it has it already: the keys and the toolbar's and rail's
    /// buttons.
    ToggleConstraint(ConstraintKind),
    /// Renames what the rename field is open on to what's typed in it,
    /// closing it.
    CommitRename,
    /// Takes the value typed in the value field: places the dimension
    /// with it, or sets the one edited to it, if it reads as a value of
    /// the kind asked for; else says why.
    SubmitValue,
    /// Ends dragging a dimension's label in the sketch being edited,
    /// where it was dragged to.
    DropLabel,
    /// Turns the dimensions selected in the sketch being edited between
    /// driving and reference: all references unless they all are, then
    /// all driving.
    ToggleReference,
    /// Places the shape the tool is drawing where the cursor last was, as
    /// the values typed in its fields fix it, or ends the spline the
    /// Spline tool is drawing: `Enter` while drawing.
    PlaceShape,
    /// Switches the splines selected in the sketch being edited between
    /// through fit points and by control points, each keeping its shape
    /// as closely as it can.
    ConvertSplines,
    /// Gives the fit points selected in the sketch being edited handles,
    /// or the ends of the splines selected, or takes them away where they
    /// all have them.
    ToggleHandles,
    /// Adds a point to the spline `spline` where it passes nearest `at`,
    /// in sketch coordinates: a double-click on it.
    InsertSplinePoint {
        spline: Id,
        at: DVec2,
    },
    /// Adds the extrude being set up, or changes the one being edited, as
    /// one undo step, and ends its session: OK, or `Enter`.
    CommitExtrude,
    /// Adds the revolve being set up, or changes the one being edited, as
    /// one undo step, and ends its session: OK, or `Enter`.
    CommitRevolve,
    /// Adds the combine being set up, or changes the one being edited, as
    /// one undo step, and ends its session: OK, or `Enter`.
    CommitCombine,
    /// Adds the move or mirror being set up, or changes the one being
    /// edited, as one undo step, and ends its session: OK, or `Enter`.
    CommitMotion,
    /// Commits the extrude, revolve, combine, move or mirror being set up as
    /// [`Edit::CommitExtrude`] and the others do, though its preview
    /// failed: the feature is kept with its error, marked failed in the
    /// Timeline, to fix later. The Accept error button only, never
    /// `Enter`.
    AcceptError,
    /// Sets the opacity previewed ([`Look::PreviewOpacity`]) as one undo
    /// step, keeping the context menu open: letting go of the slider.
    CommitOpacity,
    /// Makes the body fully opaque as one undo step, keeping the context
    /// menu open: the Opaque item under its Opacity slider.
    ResetOpacity(BodyId),
    /// Sets the colour previewed ([`Look::PreviewColor`]) as one undo
    /// step, keeping the context menu open: letting go of the Hue or
    /// Saturation slider.
    CommitColor,
    /// Gives the body the theme's colour back as one undo step, keeping
    /// the context menu open: the item under its Colour sliders.
    ResetColor(BodyId),
    /// Changes the design's units.
    SetUnits(LengthUnit),
    /// Changes the design's tolerance, which regenerates everything.
    SetTolerance(Tolerance),
    /// Deletes the link of the sketch being edited with what it made.
    RemoveLink(Id),
    /// Makes the curves of the link of the sketch being edited count for
    /// profiles, or not.
    SetLinkProfiles(Id, bool),
    Undo,
    Redo,
}

/// What only changes how the open document is looked at: the camera, the
/// side panel tab, closing the file menu, what's selected, and editing a
/// sketch, which is looking at it closely.
#[derive(Debug, Clone)]
pub enum Look {
    CloseFileMenu,
    /// Opens the view options menu, from the status bar's button, or
    /// closes it.
    ToggleViewMenu,
    CloseViewMenu,
    /// Opens a submenu of the view options menu, its item hovered or
    /// clicked, or closes the one open, another item hovered.
    ViewSubmenu(Option<ViewSubmenu>),
    /// Closes the delete prompt, deleting nothing: its Cancel button, or
    /// `Esc`.
    CancelDelete,
    /// Backs out of whatever is open, the innermost first: the delete
    /// prompt, a drag of a body's Opacity slider with its menu, the rail's
    /// list, a row's context menu, the file menu, the view options menu,
    /// picking a plane, the extrude, revolve or combine being set up, the
    /// measure tool, dragging geometry, the shape the sketch's tool is drawing, the
    /// tool (or the Constrain tool), the sketch, the selection.
    Escape,
    SelectPanel(Panel),
    /// Starts picking the plane for a new sketch, or backs out of it.
    PickPlane,
    /// Starts picking another plane for the sketch feature, an origin
    /// plane or a flat face of a body made before it: from its Timeline
    /// row's context menu, or the Sketch tab while it's edited, which is
    /// left for it and entered again after.
    ChangePlane(FeatureId),
    /// Edits the feature: a sketch is entered, an extrude, a revolve, a
    /// combine, a move, a mirror, a pattern or an align opens its session (see
    /// [`Look::StartExtrude`], [`Look::StartRevolve`],
    /// [`Look::StartCombine`], [`Look::StartMove`]) with its values.
    EditFeature(FeatureId),
    /// Starts setting up a new extrude, from the sketch selected in the
    /// Timeline if one is, or backs out of the extrude being set up.
    StartExtrude,
    /// Changes the extrude being set up, see [`ExtrudeLook`]: it isn't in
    /// the document until [`Edit::CommitExtrude`].
    Extrude(ExtrudeLook),
    /// Starts setting up a new revolve, from the sketch selected in the
    /// Timeline if one is, or backs out of the revolve being set up.
    StartRevolve,
    /// Changes the revolve being set up, see [`RevolveLook`]: it isn't in
    /// the document until [`Edit::CommitRevolve`].
    Revolve(RevolveLook),
    /// Starts setting up a new combine, its target and tools from what's
    /// selected in the model if anything is, or backs out of the combine
    /// being set up.
    StartCombine,
    /// Changes the combine being set up, see [`CombineLook`]: it isn't in
    /// the document until [`Edit::CommitCombine`].
    Combine(CombineLook),
    /// Starts setting up a new move, its bodies from what's selected in
    /// the model if anything is, or backs out of the move being set up.
    StartMove,
    /// Starts setting up a new mirror, as [`Look::StartMove`] a move.
    StartMirror,
    /// Starts setting up a new linear pattern, as [`Look::StartMove`] a
    /// move.
    StartPattern,
    /// Starts setting up a new circular pattern, as [`Look::StartMove`]
    /// a move.
    StartCircularPattern,
    /// Starts setting up a new align, its body from what's selected in
    /// the model if anything is, as [`Look::StartMove`] a move.
    StartAlign,
    /// Starts setting up a new scale, its bodies from what's selected in
    /// the model if anything is, as [`Look::StartMove`] a move.
    StartScale,
    /// Starts setting up a new split, its body from what's selected in
    /// the model if anything is, as [`Look::StartMove`] a move.
    StartSplit,
    /// Starts setting up a new chamfer, its edges those selected in the
    /// model if any are, or backs out of the one being set up.
    StartChamfer,
    /// Starts setting up a new fillet, as [`Look::StartChamfer`] a
    /// chamfer.
    StartFillet,
    /// Starts setting up a new shell, its faces those selected in the
    /// model if any are (else its body the one selected, or the model's
    /// only one), or backs out of the one being set up.
    StartShell,
    /// Starts setting up a new offset face, its faces those selected in
    /// the model if any are, or backs out of the one being set up.
    StartOffsetFace,
    /// Starts setting up a new draft, its faces those selected in the
    /// model if any are, or backs out of the one being set up.
    StartDraft,
    /// Starts setting up a new sweep, its profile's regions those of the
    /// sketch selected in the Timeline if one is, or backs out of the one
    /// being set up.
    StartSweep,
    /// Starts setting up a new loft, or backs out of the one being set
    /// up.
    StartLoft,
    /// Changes the move, mirror, pattern, align or scale being set up, see [`MotionLook`]: it
    /// isn't in the document until [`Edit::CommitMotion`].
    Motion(MotionLook),
    /// Starts the measure tool, outside sketches and operations being
    /// set up, or leaves it.
    StartMeasure,
    /// Changes the measure tool, see [`MeasureLook`].
    Measure(MeasureLook),
    /// Leaves the sketch being edited.
    FinishSketch,
    /// Selects a feature in the Timeline.
    SelectFeature(FeatureId),
    /// Opens the context menu of a row of the side panel: right-clicking
    /// it. A feature in the Timeline is selected too.
    OpenMenu(RowMenu),
    /// Opens the context menu of a row of the side panel from the
    /// keyboard, showing its tab: as [`Look::OpenMenu`], but the menu
    /// shows at the row rather than where it was last right-clicked.
    KeyMenu(RowMenu),
    /// Closes the row's context menu: a press off it.
    CloseMenu,
    /// Shows the body as `Opacity` says while its context menu's slider is
    /// dragged, without changing the document.
    PreviewOpacity(BodyId, Opacity),
    /// Shows the body in the colour `Tint` says while its context menu's
    /// Hue or Saturation slider is dragged, without changing the document.
    PreviewColor(BodyId, Tint),
    /// A click in the sketch being edited without a tool, or on a row of
    /// its Geometry list, on `hit` if anything: selects it alone, or
    /// nothing, or with `add` (`Ctrl`, or `Cmd` on macOS) adds it to the
    /// selection or takes it out.
    ClickGeometry {
        hit: Option<Id>,
        add: bool,
    },
    /// Selects the items of the sketch being edited that a box dragged
    /// over them selects: those alone, or with `add` as well.
    SelectBox {
        ids: Vec<Id>,
        add: bool,
    },
    /// A row of the sketch's Geometry or Constraints list, or a constraint's
    /// glyph, clicked: selects its item alone, or with `Ctrl` (`Cmd` on
    /// macOS) held, which the app knows, adds it to the selection or takes
    /// it out.
    ClickRow(Id),
    /// An item of the sketch being edited hovered in a list or by its
    /// glyph, or none: the viewport highlights it, or what a constraint
    /// ties together.
    HoverItem(Option<Id>),
    /// A link's row of the Sketch tab clicked: selects what it made, as
    /// [`Look::ClickRow`] selects an item.
    ClickLink(Id),
    /// A link's row of the Sketch tab hovered, or none: what it comes
    /// from is lit in the model.
    HoverLink(Option<Id>),
    /// The cursor left the row or glyph of the sketch item `id`: it's no
    /// longer hovered, unless another was since, as with
    /// [`Look::LeaveFeature`].
    LeaveItem(Id),
    /// The cursor left a link's row: it's no longer hovered, unless
    /// another was since, as with [`Look::LeaveFeature`].
    LeaveLink(Id),
    /// A feature's row in the Timeline hovered, or none: a failed
    /// feature's error geometry shows in the viewport while it is.
    HoverFeature(Option<FeatureId>),
    /// The cursor left a feature's row in the Timeline: it's no longer
    /// hovered, unless another row was since. The row a move enters may
    /// tell before the one it leaves does (moving up, the row above
    /// comes first), so leaving one doesn't let go of the other.
    LeaveFeature(FeatureId),
    /// Frames the camera on the box of the geometry of why the draft of
    /// the operation being set up fails, from Show beside its Add anyway.
    ShowFailure,
    /// Turns the camera back to where it was before [`Look::ShowFailure`],
    /// from Go back, which Show turned into.
    BackFromFailure,
    /// A row of an operation's panel hovered (a region or a body picked,
    /// a body a join, cut or intersect touches), or none: the viewport
    /// lights it up too.
    HoverPanel(Option<PanelHover>),
    /// The cursor left that row: nothing's hovered, unless another row
    /// already is. Moving from a row to the one above, the one entered
    /// tells it first.
    LeavePanel(PanelHover),
    /// What the cursor is over in the model shown, outside sketches and
    /// sessions, or nothing: sent as it changes, and as the camera or the
    /// model does under a cursor that stays, see [`PickIndex::pick`]. The
    /// viewport highlights it.
    Hover(Option<Pick>),
    /// A curve or point of a finished sketch the cursor is over in the
    /// model, outside sketches and sessions, nearer than the model under
    /// it, or nothing: sent in place of [`Look::Hover`] while it is, so
    /// either says what alone is hovered. The viewport highlights it.
    HoverSketch(Option<SketchItem>),
    /// A click on the model, outside sketches and the extrude being set
    /// up, on `pick` or on nothing: selects it, or with `add` (`Shift`
    /// or `Ctrl` held, see [`Held::TOGGLE`]) adds it or takes it out;
    /// `double` is the second click of a double-click, which selects the
    /// body. See [`Selection::click`]. While a combine is set up it picks
    /// the body of what it's on; while measuring, A or B.
    ClickModel {
        pick: Option<Pick>,
        add: bool,
        double: bool,
    },
    /// A click on a finished sketch's curve or point in the model: selects
    /// it, or with `add` adds it or takes it out, as [`Look::ClickModel`]
    /// does; see [`Selection::click_sketch`].
    ClickSketch {
        item: SketchItem,
        add: bool,
    },
    /// A body's row in Objects clicked: selects the body alone, or with
    /// `Ctrl` (`Cmd` on macOS) held, which the app knows, adds it or takes
    /// it out. See [`Selection::click_body`]. While a combine is set up
    /// it picks the body.
    ClickBody {
        body: BodyId,
        add: bool,
    },
    /// An origin object's or a sketch's row in Objects clicked: selects it
    /// alone, or with `Ctrl` (`Cmd` on macOS) held, which the app knows,
    /// adds it or takes it out.
    ClickObject {
        row: ObjectRow,
        add: bool,
    },
    /// The left button held still in a sketch or on the model, over more
    /// than one item: lists them there to choose from.
    OpenOverlaps(Overlaps),
    /// A row of that list hovered: its item is highlighted.
    HoverOverlap(Option<usize>),
    /// The cursor left that row: nothing's hovered, unless another row
    /// already is.
    LeaveOverlap(usize),
    /// A row of that list clicked: selects its item as a click on it
    /// would, alone, closing the list, or with `add` (`Ctrl`, `Cmd` on
    /// macOS, held, which the app knows) added or taken out, the list kept
    /// open to pick more.
    ChooseOverlap {
        index: usize,
        add: bool,
    },
    /// A row's tick of that list clicked: adds its item to the selection
    /// or takes it out, as a click on it with `Ctrl` would, the list kept
    /// open.
    ToggleOverlap(usize),
    /// Closes that list, the selection as it was: a press anywhere else.
    CloseOverlaps,
    /// Clears the selection: the sketch's in a sketch, else the
    /// Timeline's and the model's.
    ClearSelection,
    /// Where the drawing tool's next click would snap to, and what to,
    /// with the cursor where it is, or none: the glyph shown by the
    /// cursor.
    Snap(Option<Snap>),
    /// The click the drawing tool would take with the cursor where it is,
    /// sent as the cursor moves while its shape has fields (see
    /// [`typed::fields`]): where they show, what they measure until values
    /// are typed, and where `Enter` places the shape. Says where it snaps
    /// too, as [`Look::Snap`] would.
    Aim(ToolClick),
    /// Moves the focus to the drawing tool's next field, taking the value
    /// typed in the one it leaves: `Tab` while drawing.
    NextField,
    /// Switches the Rectangle tool between drawing from a corner and from
    /// the centre.
    ToggleCentered,
    /// Switches the Spline tool between drawing through fit points and by
    /// control points.
    ToggleSplineKind,
    /// Shows the curvature comb of the splines selected, or hides it.
    ToggleComb,
    /// Ends picking what the Mirror tool mirrors: its next click picks the
    /// line to mirror about. `Enter`, once it has picked something.
    MirrorAbout,
    /// Takes up `tool` in the sketch being edited, or puts it down if it's
    /// the one in use.
    SelectTool(Tool),
    /// Takes up the Constrain tool, which offers the constraints that fit
    /// the selection, or puts it down.
    ToggleConstrain,
    /// Shows the constraints' glyphs in the viewport, or hides them.
    ToggleGlyphs,
    /// Puts down the tool in use in the sketch being edited, the
    /// Constrain tool too: `Space` while one is.
    PutDownTool,
    /// Folds a group of the Sketch tab's Geometry list, or unfolds it.
    ToggleGroup(GeometryGroup),
    /// Unfolds a curve's row of the Geometry list to list its points under
    /// it, or folds it.
    ToggleExpanded(Id),
    /// Folds a group of the Objects tab, or unfolds it.
    ToggleObjectGroup(ObjectGroup),
    /// Shows one of the world's origin objects in the viewport, or hides
    /// it: not an edit of the document.
    ToggleOrigin(OriginObject),
    /// An origin object hovered by its row in Objects, or none: the
    /// viewport draws it emphasised, shown or not.
    HoverOrigin(Option<OriginObject>),
    /// Picking a plane, the origin plane the cursor is over in the
    /// viewport, nearer than the model, in place of the model's hover.
    HoverPlane(OriginPlane),
    /// The cursor left an origin object's row in Objects: it's no longer
    /// hovered, unless another row was since (see [`Look::LeaveFeature`]).
    LeaveOrigin(OriginObject),
    /// The cursor entered or left the Opacity and Colour part of a body's
    /// context menu: the viewport leaves the selection out while it's
    /// there, so the body shows as it's drawn.
    HoverBodyLook(bool),
    /// A body's row in Objects hovered, or none: its faces are lit in the
    /// viewport while it is.
    HoverBodyRow(Option<BodyId>),
    /// The cursor left a body's row in Objects, as [`Look::LeaveOrigin`].
    LeaveBodyRow(BodyId),
    /// Drags the item `id` of the sketch being edited, grabbed at `from`,
    /// to `to`, in sketch coordinates. Shown until it's dropped
    /// ([`Edit::DropGeometry`]) or `Esc` puts it back.
    DragGeometry {
        id: Id,
        from: DVec2,
        to: DVec2,
    },
    /// Puts back the geometry being dragged in the sketch being edited, if
    /// any is: `Esc` during a drag, which does nothing else then, even if
    /// the drag hasn't moved anything yet.
    CancelDrag,
    /// A dimension's label pressed: selects it alone, or with `add`
    /// (`Ctrl`, or `Cmd` on macOS, which the app knows) adds it to the
    /// selection or takes it out, and grabs the label to drag.
    PressLabel {
        id: Id,
        add: bool,
    },
    /// Drags the label of the dimension `id`, grabbed at `from`, to `to`,
    /// in sketch coordinates. Shown until it's dropped
    /// ([`Edit::DropLabel`]).
    DragLabel {
        id: Id,
        from: DVec2,
        to: DVec2,
    },
    /// Opens the value field on the dimension `id` to change its value,
    /// showing its expression as typed: in the Constraints list if
    /// `in_list`, else at its label.
    EditDimension {
        id: Id,
        in_list: bool,
    },
    /// Opens the rename field on a feature, sketch or body, in its row
    /// of the side panel, holding its name: `F2` or its context menu.
    StartRename(varde_document::Named),
    /// The text in the rename field, as typed.
    RenameInput(String),
    /// Closes the rename field, changing nothing.
    CancelRename,
    /// The text in the value field, as typed.
    ValueInput(String),
    /// Closes the value field, changing nothing.
    CancelValue,
    /// Switches the Dimension tool between the radius and the diameter of
    /// the circle or arc it's placing a dimension of.
    SwitchRound,
    /// Gives the Geometry list this share of the Sketch tab's height.
    SplitSketchTab(f32),
    /// The Geometry list scrolled to this offset, in pixels.
    ScrollGeometry(f32),
    /// The Constraints list scrolled to this offset, in pixels.
    ScrollConstraints(f32),
    /// Turns the camera around the point it orbits by these angles in
    /// radians: the pivot if one was picked, else its target.
    Orbit {
        yaw: f32,
        pitch: f32,
    },
    Pan {
        dx: f32,
        dy: f32,
    },
    /// Zooms by `factor` towards the point that shows `x` right and `y`
    /// down of the viewport's middle, in fractions of its height: the
    /// cursor's, which stays under it.
    Zoom {
        factor: f32,
        x: f32,
        y: f32,
    },
    /// Home: turns the camera to the home view and forgets the pivot.
    ResetCamera,
    LookFrom(View),
    /// A middle click in the viewport: the point picked for the camera to
    /// orbit, which is marked for a moment, or none, back to its target.
    SetPivot(Option<glam::Vec3>),
    /// The cursor over the view cube or off it: the pivot is marked
    /// while it's over.
    HoverCube(bool),
    /// Views in `Projection`, from the view options menu, which closes.
    SetProjection(Projection),
    /// Opens or closes a tool set's list on the rail, see [`RailLook`].
    Rail(RailLook),
}

/// A tool drawing in a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// Connected lines, click after click.
    Line,
    /// Centre, then a point on the circle.
    Circle,
    /// Start, end, then a point on the arc.
    Arc,
    /// A lone point.
    Point,
    /// Two opposite corners, or the centre and a corner: four lines, held
    /// horizontal and vertical.
    Rectangle,
    /// The centre, then a corner: equal lines with their corners on a
    /// construction circle.
    Polygon,
    /// A smooth curve through the points clicked, or by them as control
    /// points, until a double-click, `Enter` or its first point again.
    Spline,
    /// Measures what's clicked: see `dimension::measure`.
    Dimension,
    /// Takes away the piece of a curve clicked, between the curves
    /// cutting it.
    Trim,
    /// Lengthens a line, an arc or a spline clicked, at the end nearer the
    /// click, to the next curve it meets.
    Extend,
    /// Copies the chain of curves clicked (or selected) a distance to one
    /// side, the cursor's or typed, tied to it by that distance.
    Offset,
    /// Mirrors geometry about a line: the selection, or what's clicked
    /// until `Enter`, then the line clicked.
    Mirror,
    /// Rounds the corner clicked, where two lines end, with an arc tangent
    /// to both, of the radius the cursor's or typed.
    Fillet,
    /// Cuts the corner clicked with a line across it, as far back as the
    /// cursor's, or the distances or the distance and the angle typed.
    Chamfer,
    /// Picks what to project onto the sketch's plane, outside it: the
    /// model's edges and corners, and other sketches' curves and points,
    /// made before the sketch.
    Project,
    /// Picks what to cut with the sketch's plane, outside it: the model's
    /// faces and edges, made before the sketch.
    Intersect,
}

impl Tool {
    /// Every tool, drawing ones first.
    pub const ALL: [Tool; 16] = [
        Tool::Line,
        Tool::Rectangle,
        Tool::Circle,
        Tool::Arc,
        Tool::Polygon,
        Tool::Spline,
        Tool::Point,
        Tool::Dimension,
        Tool::Trim,
        Tool::Extend,
        Tool::Offset,
        Tool::Mirror,
        Tool::Fillet,
        Tool::Chamfer,
        Tool::Project,
        Tool::Intersect,
    ];

    /// Those the sketch toolbar shows, in its order, as the UI mock's
    /// sketch bar: the rest are on the rail, as these are too.
    pub const BAR: [Tool; 7] = [
        Tool::Line,
        Tool::Rectangle,
        Tool::Circle,
        Tool::Arc,
        Tool::Trim,
        Tool::Offset,
        Tool::Dimension,
    ];

    /// The tool as the user sees it.
    pub fn label(self) -> &'static str {
        match self {
            Tool::Line => "Line",
            Tool::Circle => "Circle",
            Tool::Arc => "Arc",
            Tool::Point => "Point",
            Tool::Rectangle => "Rectangle",
            Tool::Polygon => "Polygon",
            Tool::Spline => "Spline",
            Tool::Dimension => "Dimension",
            Tool::Trim => "Trim",
            Tool::Extend => "Extend",
            Tool::Offset => "Offset",
            Tool::Mirror => "Mirror",
            Tool::Fillet => "Fillet",
            Tool::Chamfer => "Chamfer",
            Tool::Project => "Project",
            Tool::Intersect => "Intersect",
        }
    }

    /// Whether it draws shapes, which `X` can make construction geometry
    /// and which snap, rather than measuring or changing them.
    pub fn draws(self) -> bool {
        !matches!(
            self,
            Tool::Dimension
                | Tool::Trim
                | Tool::Extend
                | Tool::Offset
                | Tool::Mirror
                | Tool::Fillet
                | Tool::Chamfer
                | Tool::Project
                | Tool::Intersect
        )
    }

    /// Whether it picks outside the sketch, in the model and other
    /// sketches, rather than in it: Project and Intersect. While it's in
    /// use the sketch's own geometry isn't hit.
    pub fn picks_outside(self) -> bool {
        matches!(self, Tool::Project | Tool::Intersect)
    }

    /// Whether it works on a corner where two lines end: Fillet and
    /// Chamfer.
    pub fn corners(self) -> bool {
        matches!(self, Tool::Fillet | Tool::Chamfer)
    }

    /// Whether it picks something with a click, then places what it makes
    /// of it through the cursor, where the button's let go: Offset its
    /// copy of a chain, Fillet and Chamfer theirs on a corner.
    pub fn places(self) -> bool {
        matches!(self, Tool::Offset | Tool::Fillet | Tool::Chamfer)
    }

    /// What a tool that [`places`](Tool::places) picks with a click on
    /// `hit` at `at` in `sketch`: Offset the chain the curve is in
    /// ([`Sketch::chain_of`]), Fillet and Chamfer the corner at the point,
    /// as it and its lines ([`Sketch::corner_lines`]). Empty for nothing
    /// to pick, or another tool.
    pub fn pick(self, sketch: &Sketch, hit: Id, at: DVec2) -> Vec<Id> {
        match self {
            Tool::Offset => sketch.chain_of(hit),
            _ if self.corners() => sketch
                .corner_lines(hit, at)
                .map(|[a, b]| vec![hit, a, b])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

/// A click in the sketch being edited with its tool, from the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolClick {
    /// Where, in sketch coordinates, snapped (see [`Snap`]), within
    /// `MAX_COORD` of zero.
    pub at: DVec2,
    /// What it snapped to, if anything: a drawing tool ties its point
    /// there to it.
    pub target: Option<Target>,
    /// How the shape runs from its start, if that snapped: a drawing tool
    /// holds it so.
    pub inference: Option<Inference>,
    /// The point or curve under the cursor, if there is one: what the
    /// Dimension tool picks. For Trim and Extend, and Offset picking its
    /// chain, the curve under it, points aside, for Mirror's line to
    /// mirror about the line (see [`ActiveTool::about`]), and for Fillet
    /// and Chamfer picking their corner the point where lines make one.
    pub hit: Option<Id>,
    /// A pixel's size at `at`, in sketch units, above zero: a shape
    /// smaller than that can't have been meant, and is refused.
    pub pixel: f64,
    /// Whether it's the second click of a double-click.
    pub double: bool,
    /// Whether the reference modifier (`Alt`) is held, which places the
    /// Dimension tool's dimension as a reference.
    pub reference: bool,
}

impl ToolClick {
    /// Where it goes, and what snapped it there.
    pub fn snap(&self) -> Snap {
        Snap {
            at: self.at,
            target: self.target,
            inference: self.inference,
        }
    }

    /// The same click, going where `snap` says, snapped as it says.
    pub fn snapped(self, snap: Snap) -> ToolClick {
        ToolClick {
            at: snap.at,
            target: snap.target,
            inference: snap.inference,
            ..self
        }
    }

    /// The point of the sketch's it snapped to, if any: the origin, or one
    /// a drawing tool takes as its own.
    pub fn point(&self) -> Option<Id> {
        match self.target {
            Some(Target::Point(id)) => Some(id),
            _ => None,
        }
    }
}

/// What to do about unsaved changes before closing the document or
/// quitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsaved {
    /// Save, then go on if that worked.
    Save,
    /// Go on without saving.
    Discard,
    /// Stay.
    Cancel,
}

/// A row of the side panel whose context menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowMenu {
    /// A feature in the Timeline, which is selected with it.
    Feature(FeatureId),
    /// A body in Objects.
    Body(BodyId),
    /// A sketch in Objects.
    Sketch(FeatureId),
    /// A link in the Sketch tab of the sketch being edited.
    Link(Id),
    /// A point or curve in the Sketch tab's Geometry list.
    Item(Id),
}

/// A group of the Sketch tab's Geometry list, which can be folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GeometryGroup {
    /// The sketch's own points and curves.
    Own,
    /// Its sketch face: the outline of the face it's on, projected.
    SketchFace,
    /// Its links projecting outside geometry.
    Projected,
    /// Its links intersecting outside geometry with its plane.
    Intersected,
}

/// A group of the Objects tab, which can be folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectGroup {
    Origin,
    Bodies,
    Sketches,
}

/// A row of Objects that's selected in the list itself, not in the
/// model: an origin object or a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRow {
    Origin(OriginObject),
    Sketch(FeatureId),
}

/// One of the world's origin objects, listed in Objects' Origin group,
/// which can be shown and hidden but not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginObject {
    /// The origin's marker.
    Point,
    Axis(varde_document::Axis3),
    Plane(OriginPlane),
}

impl OriginObject {
    /// Each, in the order Objects lists them.
    pub const ALL: [OriginObject; 7] = [
        OriginObject::Point,
        OriginObject::Axis(varde_document::Axis3::X),
        OriginObject::Axis(varde_document::Axis3::Y),
        OriginObject::Axis(varde_document::Axis3::Z),
        OriginObject::Plane(OriginPlane::XY),
        OriginObject::Plane(OriginPlane::XZ),
        OriginObject::Plane(OriginPlane::YZ),
    ];

    /// It as the renderer has it.
    pub fn part(self) -> varde_render::OriginPart {
        match self {
            OriginObject::Point => varde_render::OriginPart::Marker,
            OriginObject::Axis(axis) => varde_render::OriginPart::Axis(axis as usize),
            OriginObject::Plane(plane) => varde_render::OriginPart::Plane(plane as usize),
        }
    }

    /// Its entry in `origin`, whether it's shown.
    pub fn shown(self, origin: &mut varde_render::OriginShown) -> &mut bool {
        match self {
            OriginObject::Point => &mut origin.marker,
            OriginObject::Axis(axis) => &mut origin.axes[axis as usize],
            OriginObject::Plane(plane) => &mut origin.planes[plane as usize],
        }
    }
}

impl GeometryGroup {
    /// The group of the links of `kind`.
    pub fn of_links(kind: varde_sketch::LinkKind) -> Self {
        match kind {
            varde_sketch::LinkKind::Project => GeometryGroup::Projected,
            varde_sketch::LinkKind::Intersect => GeometryGroup::Intersected,
        }
    }
}

/// A tab of the side panel. Two show at a time: Timeline and Objects, or
/// in a sketch Sketch and Objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    Timeline,
    /// The sketch being edited: its geometry and constraints.
    Sketch,
    #[default]
    Objects,
}

impl Panel {
    /// The tab listing the features, or in a sketch what it holds, which
    /// shows with Objects.
    fn features(sketching: bool) -> Self {
        if sketching {
            Panel::Sketch
        } else {
            Panel::Timeline
        }
    }

    /// The other of the two tabs showing, given whether a sketch is being
    /// edited.
    pub fn other(self, sketching: bool) -> Self {
        match self {
            Panel::Timeline | Panel::Sketch => Panel::Objects,
            Panel::Objects => Panel::features(sketching),
        }
    }

    /// The tab to show on entering (`sketching`) or leaving a sketch:
    /// Sketch and Timeline trade places, and Objects stays.
    pub fn for_sketching(self, sketching: bool) -> Self {
        match self {
            Panel::Timeline | Panel::Sketch => Panel::features(sketching),
            Panel::Objects => Panel::Objects,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Panel::Timeline => "Timeline",
            Panel::Sketch => "Sketch",
            Panel::Objects => "Objects",
        }
    }
}
