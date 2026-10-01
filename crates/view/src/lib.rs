//! UI code: iced widgets and layout.
//!
//! The view is a pure function of application state. It produces
//! [`Message`]s but never mutates anything itself; that is the job of the
//! `app` crate.

// Proving wgpu types `Send + Sync` for the shader pipeline exceeds the default
// limit on recent nightlies.
#![recursion_limit = "256"]

mod anchors;
mod chrome;
mod constrain;
mod context_menu;
mod controls;
pub mod dimension;
mod document;
mod escape;
mod extrude;
mod hit;
mod icons;
mod operation_panel;
mod panels;
mod pick;
#[cfg(any(test, feature = "probe"))]
pub mod probe;
mod projection;
mod rail;
mod shortcut;
mod snap;
pub mod spline;
mod split;
mod status;
#[cfg(test)]
mod testing;
mod theme;
mod toolbar;
pub mod typed;
mod view_cube;
mod viewport;
mod welcome;

use std::path::PathBuf;

use glam::DVec2;
use varde_document::{BodyId, FeatureId, OriginPlane, Tolerance};
use varde_expr::LengthUnit;
use varde_render::{Projection, View};
use varde_sketch::{Id, Sketch};

pub use constrain::{ConstraintKind, ConstraintSet};
pub use document::{
    ActiveTool, DeletePrompt, DocumentState, MeshStatus, Overlay, RecoveredChanges, RefusedEdit,
    SketchState, ValueField, ValueTarget, document,
};
pub use extrude::{
    Candidate, Distance, DistanceField, ExtentKind, ExtrudeLook, ExtrudeState, ExtrudeTarget,
    Handle, OperationKind, snap_step,
};
pub use icons::LOGO_SVG;
pub use operation_panel::PANEL_BODY;
pub use pick::{EDGE_REACH, Pick, PickIndex, Picked};
pub use rail::{RAIL_LIST, RailLook, RailOpen, RailSpot, rail_rows, rail_sets};
pub use shortcut::{Binding, DocumentKeys, Held, document_bindings, pressed, welcome_bindings};
pub use snap::{Inference, Level, SNAP_TOLERANCE, Snap, Target};
pub use status::{STATUS_BAR_HEIGHT, STATUS_BAR_ROOM};
pub use theme::{Mode, SIDE_PANEL_WIDTH, theme as iced_theme};
pub use viewport::ModelPicking;
pub use welcome::{RecentCard, StoredDesign, WelcomeState, welcome};

/// The text field a dimension's value is typed in, placing it or editing
/// it in place: there's one at a time, focused as it opens.
pub const VALUE_FIELD: iced::widget::Id = iced::widget::Id::new("dimension-value");

/// What the user asks for through the view, grouped by what acts on it.
/// The app has messages of its own on top, from its subscriptions, lanes
/// and dialogs.
#[derive(Debug, Clone)]
pub enum Message {
    Welcome(Welcome),
    File(File),
    Edit(Edit),
    Look(Look),
    ToggleTheme,
    /// Shows the status bar's hints for the mouse, or hides them: the
    /// view options menu's Mouse hints.
    ToggleMouseHints,
}

/// What the user asks for on the welcome screen.
#[derive(Debug, Clone)]
pub enum Welcome {
    NewDesign,
    Open,
    OpenPath(PathBuf),
    /// Opens a design kept in the store: left behind by a crash, or
    /// downloaded on the web.
    OpenStored(PathBuf),
    /// Deletes a design kept in the store.
    DiscardStored(PathBuf),
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
    /// Hides the sketch edit the solver refused after its sketch was
    /// left.
    DismissRefusedEdit,
    ToggleFileMenu,
    /// Removes the body and the feature making it, as one undo step, at
    /// once if nothing else goes with them, or else asking first (see
    /// [`DeletePrompt`]).
    RemoveBody(BodyId),
    ToggleVisible(BodyId),
    /// Adds a sketch on `plane` and edits it.
    NewSketch(OriginPlane),
    /// Removes the feature and the bodies it makes, as one undo step, at
    /// once if no other feature depends on it, or else asking first (see
    /// [`DeletePrompt`]).
    RemoveFeature(FeatureId),
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
    /// Turns the curves selected in the sketch being edited between normal
    /// and construction geometry, or while a tool is in use, the shapes it
    /// draws next.
    ToggleConstruction,
    /// Constrains the geometry selected in the sketch being edited so, if
    /// it fits, see [`ConstraintKind::make`].
    Constrain(ConstraintKind),
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
    /// Changes the design's units.
    SetUnits(LengthUnit),
    /// Changes the design's tolerance, which regenerates everything.
    SetTolerance(Tolerance),
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
    /// Closes the delete prompt, deleting nothing: its Cancel button, or
    /// `Esc`.
    CancelDelete,
    /// Backs out of whatever is open, the innermost first: the delete
    /// prompt, the rail's list, the feature's context menu, the file menu,
    /// the view options menu, picking a plane, the extrude being set up,
    /// dragging geometry, the shape the sketch's tool is drawing, the tool
    /// (or the Constrain tool), the sketch, the selection.
    Escape,
    SelectPanel(Panel),
    /// Starts picking the plane for a new sketch, or backs out of it.
    PickPlane,
    /// Edits the feature: a sketch is entered, an extrude opens its
    /// session (see [`Look::StartExtrude`]) with its values.
    EditFeature(FeatureId),
    /// Starts setting up a new extrude, from the sketch selected in the
    /// Timeline if one is, or backs out of the extrude being set up.
    StartExtrude,
    /// Changes the extrude being set up, see [`ExtrudeLook`]: it isn't in
    /// the document until [`Edit::CommitExtrude`].
    Extrude(ExtrudeLook),
    /// Leaves the sketch being edited.
    FinishSketch,
    /// Selects a feature in the Timeline.
    SelectFeature(FeatureId),
    /// Opens the context menu of a row of the side panel: right-clicking
    /// it. A feature in the Timeline is selected too.
    OpenMenu(RowMenu),
    /// Closes the row's context menu: a press off it.
    CloseMenu,
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
    /// What the cursor is over in the model shown, outside sketches and
    /// sessions, or nothing: sent as it changes, see [`PickIndex::pick`].
    /// The viewport highlights it.
    Hover(Option<Pick>),
    /// Clears the selection: the sketch's in a sketch, else the Timeline's.
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
}

impl Tool {
    /// In the order the toolbar shows them.
    pub const ALL: [Tool; 14] = [
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
        )
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowMenu {
    /// A feature in the Timeline, which is selected with it.
    Feature(FeatureId),
    /// A body in Objects.
    Body(BodyId),
    /// A sketch in Objects.
    Sketch(FeatureId),
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
