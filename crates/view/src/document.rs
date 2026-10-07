//! The document screen: its layout, the banners over it, the prompt about
//! unsaved changes, what the status bar says and the card telling of a
//! slow regeneration.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, space, stack, text, text_input};
use iced::{Alignment, Element, Length};
use varde_document::EXTENSION;
use varde_document::{
    APP_NAME, AxisLine, Body, BodyId, Document, EditError, Editor, Extent, Feature, FeatureId,
    FeatureKind, Opacity, Placement, Plane, Tint,
};
use varde_expr::LengthUnit;
use varde_kernel::{RenderLines, RenderMesh};
use varde_render::{BodyTint, Camera};
use varde_sketch::{
    Analysis, Failure, Id, Kind, LinkKind, Measure, Profiles, Rejected, Selectable, Side, Sketch,
    TooComplex,
};

use crate::chrome::{
    self, Hint, chord_hint, dialog, dialog_button, key_hint, mouse_hint, small_button, step_hint,
};
use crate::icons::{self, Icon, MouseButton};
use crate::shortcut::{DocumentKeys, Held, Shortcut};
use crate::status::{self, Status};
use crate::theme::Emphasis;
use crate::typed::Field;
use crate::{
    ConstraintKind, Downloads, Edit, ExtrudeState, File, Location, Look, Message, OperationKind,
    Panel, PlanePick, RevolvePick, RevolveState, RowMenu, SavePlace, Snap, Target, Tool, Unsaved,
    panels, theme, toolbar, viewport,
};

/// Borrowed state needed to build the document screen.
pub struct DocumentState<'a> {
    pub editor: &'a Editor,
    pub camera: &'a Camera,
    /// The point the camera orbits, marked as it's picked and while the
    /// cursor is over the view cube, if one was picked and it shows.
    pub pivot: Option<varde_render::Pivot>,
    /// The document's mesh, which the app gets from the regeneration side,
    /// so it may lag behind the document.
    pub mesh: &'a Arc<RenderMesh>,
    /// The body each of `mesh`'s parts is of, in order, whose opacity it's
    /// drawn with ([`DocumentState::part_opacity`]).
    pub parts: &'a [BodyId],
    /// A body's opacity shown in place of the document's while its context
    /// menu's slider is dragged, if one is.
    pub opacity_preview: Option<(BodyId, Opacity)>,
    /// A body's colour shown in place of the document's while its context
    /// menu's Hue or Saturation slider is dragged, if one is.
    pub color_preview: Option<(BodyId, Tint)>,
    /// Whether the cursor is over the Opacity and Colour part of a
    /// body's context menu: washed as hovered.
    pub body_look_hovered: bool,
    /// The feature, sketch or body being renamed and the name as typed,
    /// if one is: its row in the side panel holds the rename field.
    pub renaming: Option<(varde_document::Named, &'a str)>,
    /// The finished sketches' curves, which come with `mesh`.
    pub sketches: &'a Arc<RenderLines>,
    /// How `mesh` and `sketches` stand against the document.
    pub mesh_status: MeshStatus<'a>,
    /// A regeneration that's slow, shown over the top of the viewport,
    /// with how far it has got, if the lane has said: `None` while
    /// there's none, or it's quick enough not to show.
    pub regenerating: Option<Option<&'a varde_regen::Progress>>,
    /// Picking `mesh` with the cursor, if the cursor does: outside
    /// sketches and sessions.
    pub picking: Option<crate::ModelPicking<'a>>,
    /// What's hovered and selected in `mesh`, drawn over it, if anything.
    pub highlight: Option<&'a Arc<crate::ModelHighlight>>,
    /// Whether what's hovered is drawn over what hides it too: hovered in
    /// the list of what overlaps.
    pub hover_through: bool,
    /// The failures' geometry drawn over the model, in red.
    pub errors: &'a Arc<crate::ShownErrors>,
    /// What's selected in the model: Objects marks the bodies selected,
    /// and the status bar tells of it.
    pub model_selection: &'a crate::Selection,
    /// What the newest answer measures of one or two items of
    /// `model_selection`, which the status bar shows with them: `None`
    /// while it's on its way, or where they aren't measured.
    pub selection_measured: Option<&'a varde_regen::Inspected>,
    /// The document name, without extension.
    pub name: &'a str,
    /// Whether the design has no name, never saved: [`NOT_SAVED`] shows
    /// in its name's place in the file cell.
    ///
    /// [`NOT_SAVED`]: crate::NOT_SAVED
    pub unnamed: bool,
    /// Natively, the path of the design's own file, which pointing at the
    /// file cell shows.
    pub path: Option<&'a str>,
    /// Where the design is kept, on the web: a bar under the file cell
    /// says, and pointing at the cell says more.
    pub location: Option<Location>,
    /// Where a design in browser storage stands against its downloads, as
    /// the file menu says.
    pub downloads: Option<Downloads>,
    /// Whether the file menu offers Download: on the web.
    pub downloadable: bool,
    /// For a design in browser storage, the file menu offers Rename…:
    /// whether it may be renamed now, not while it's read-only, or saving,
    /// or asked about.
    pub rename: Option<bool>,
    /// The app's own Save As dialog, on the web, while it shows.
    pub naming: Option<NamePrompt<'a>>,
    /// Whether there are unsaved changes.
    pub edited: bool,
    /// Why the document can't be edited, if it can't. Edit commands are
    /// disabled then; the camera still works.
    pub read_only: Option<&'a str>,
    /// Why the last edit was refused, if it was.
    pub edit_error: Option<&'a EditError>,
    /// Why the last thing asked couldn't be done, if the app refused it
    /// itself (a sketch on a curved face, a sketch that isn't placed
    /// entered), until the next thing asked.
    pub notice: Option<&'a str>,
    /// A sketch edit the solver refused after its sketch was left, if
    /// there's one not dismissed: a banner over the viewport says so.
    pub refused_edit: Option<RefusedEdit<'a>>,
    /// Whether a save is in flight.
    pub saving: bool,
    /// Why the last save or auto-save failed, if it did. Shown with Save
    /// As, which keeps what the user has whatever went wrong with the file.
    pub save_error: Option<Cow<'a, str>>,
    /// Unsaved changes a session that crashed left of the document, if
    /// there are any to offer to restore.
    pub recovered: Option<RecoveredChanges>,
    /// How the file the document was opened from was found damaged, if
    /// it was, until dismissed.
    pub damage: Option<DamagedFile>,
    /// Whether the visible bodies can be exported now: there are some,
    /// regenerating hasn't failed, and no export is on its way.
    pub exportable: bool,
    /// Whether an export is on its way to its file, after the user chose
    /// where.
    pub exporting: bool,
    /// Why the last export failed, if it did, until dismissed.
    pub export_error: Option<&'a str>,
    /// What's shown over the screen, if anything.
    pub overlay: Option<Overlay>,
    /// The selected side panel tab.
    pub panel: Panel,
    /// Whether the peek key is held, showing the other tab.
    pub peek: bool,
    pub mode: theme::Mode,
    /// What the view options menu turns on and off.
    pub options: crate::ViewOptions,
    /// What a plane is being picked for, if one is: a new sketch, or the
    /// sketch whose plane is changed.
    pub picking_plane: Option<&'a PlanePick>,
    /// A thumbnail for the viewport to render on its next frame, if one
    /// is asked for (see [`ThumbnailRequest`](crate::ThumbnailRequest)).
    pub thumbnail: Option<&'a Arc<crate::ThumbnailRequest>>,
    /// The viewport's width over its height, as it last told the app, if
    /// it has ([`Look::ViewAspect`]).
    pub aspect: Option<f32>,
    /// The feature selected in the Timeline, if any.
    pub selected_feature: Option<FeatureId>,
    /// The feature the model is rolled back to before, where the
    /// Timeline's marker shows, and whether only while a feature is
    /// edited, when the marker can't be dragged.
    pub rollback: (Option<FeatureId>, bool),
    /// Whether the Timeline's marker is being dragged.
    pub rolling: bool,
    /// The row of the side panel whose context menu is open, if one is.
    pub row_menu: Option<RowMenu>,
    /// The world's origin, axes and planes Objects has shown.
    pub origin: varde_render::OriginShown,
    /// The origin objects and sketches selected in Objects: the origin
    /// objects drawn emphasised.
    pub objects_selected: &'a [crate::ObjectRow],
    /// The origin object hovered, if one is: drawn emphasised.
    pub origin_hover: Option<crate::OriginObject>,
    /// The Objects tab's groups folded.
    pub objects_folded: &'a std::collections::BTreeSet<crate::ObjectGroup>,
    /// What overlaps where the left button was held still in the
    /// viewport, listed to choose from, if it's open.
    pub overlaps: Option<&'a crate::Overlaps>,
    /// How each of the model's overlaps listed shows where a session
    /// picks the model for itself: ticked as the session has it (what a
    /// click on it would leave or take out), and what it is to the
    /// session; `None`: ticked as `model_selection` has it.
    pub overlap_ticks: Option<Vec<crate::OverlapTick>>,
    /// The sketch being edited, if one is.
    pub sketch: Option<SketchState<'a>>,
    /// The extrude being set up, if one is: never with a sketch.
    pub extrude: Option<ExtrudeState<'a>>,
    /// The revolve being set up, if one is: never with a sketch or an
    /// extrude.
    pub revolve: Option<RevolveState<'a>>,
    /// The combine being set up, if one is: never with a sketch or another
    /// operation.
    pub combine: Option<crate::CombineState<'a>>,
    /// Whether the document has two bodies or more: the Combine tool works
    /// outside sketches then.
    pub combinable: bool,
    /// The move or mirror being set up, if one is: never with a sketch or
    /// another operation.
    pub motion: Option<crate::MotionState<'a>>,
    /// The measure tool, while it's in use: never with a sketch or an
    /// operation being set up.
    pub measure: Option<crate::MeasureState<'a>>,
    /// The sketches that don't solve, as regenerating found.
    pub unsolved: &'a [FeatureId],
    /// The features that failed and why, as regenerating found, in the
    /// document's order.
    pub failed: &'a [varde_regen::FeatureFailure],
    /// Each body a join merged into another (*consumed*), and the body
    /// holding it now, as regenerating found, in the document's order:
    /// Objects shows a consumed body in its holder.
    pub merged: &'a [(BodyId, BodyId)],
    /// What deleting a feature or body would take with it, asked about
    /// before it's deleted, if it's being asked.
    pub deleting: Option<DeletePrompt<'a>>,
    /// Whether edits are waiting on the solver: undo drops the newest
    /// of them, and redo is off.
    pub proposing: bool,
    /// The rail's tool set whose list is open, if one is, and the row of
    /// it the keys are on.
    pub rail: Option<crate::RailOpen>,
}

impl DocumentState<'_> {
    /// Whether a face and nothing else is selected in the model while the
    /// cursor picks it, outside picking a plane: what a new sketch goes
    /// on.
    pub(crate) fn face_selected(&self) -> bool {
        self.picking.is_some()
            && self.picking_plane.is_none()
            && self.model_selection.single_face().is_some()
    }

    /// Whether the document can be changed, including by undo, and saved.
    pub(crate) fn editable(&self) -> bool {
        self.read_only.is_none()
    }

    /// What the viewport draws of the world's origin, axes and planes:
    /// what Objects has shown, and the planes while a tool offers them
    /// ([`toolbar::picks_origin_planes`]), drawn over the model then.
    pub fn origin_drawn(&self) -> varde_render::OriginShown {
        let mut origin = self.origin;
        if toolbar::picks_origin_planes(self) {
            origin.planes = [true; 3];
            origin.planes_on_top = true;
        }
        origin.hovered = self.origin_hover.map(crate::OriginObject::part);
        for row in self.objects_selected {
            if let crate::ObjectRow::Origin(object) = row
                && let Some(selected) = origin.selected.get_mut(object.part().index())
            {
                *selected = true;
            }
        }
        origin
    }

    /// How opaque the viewport draws each part of the mesh: as its body
    /// in `editor`'s document, or `opacity_preview`, has it.
    pub fn part_opacity(&self) -> Arc<[f32]> {
        part_opacity(self.editor.document(), self.parts, self.opacity_preview)
    }

    /// The colour the viewport draws each part of the mesh in: as its
    /// body in `editor`'s document, or `color_preview`, has it.
    pub fn part_tints(&self) -> Arc<[Option<BodyTint>]> {
        part_tints(self.editor.document(), self.parts, self.color_preview)
    }

    /// What the screen's shortcuts depend on.
    pub(crate) fn keys(&self) -> DocumentKeys {
        DocumentKeys::new(self.editable(), self.selected_feature, self.sketch)
            .with_face_selected(self.face_selected())
            .with_objects_deletable(
                (self.objects_selected.iter())
                    .any(|row| matches!(row, crate::ObjectRow::Sketch(_))),
            )
            .with_extrude(self.extrude.as_ref())
            .with_revolve(self.revolve.as_ref())
            .with_combine(self.combinable, self.combine.as_ref())
            .with_motion(
                !self.editor.document().bodies().is_empty(),
                self.motion.as_ref(),
            )
            .with_measure(self.measure.is_some())
            .with_picking_plane(self.picking_plane.is_some())
            .with_rail(self.rail)
            .with_edited(self.edited)
            .with_history(
                self.editor.can_undo() || self.proposing,
                // Not while edits wait on the solver, which come after.
                self.editor.can_redo() && !self.proposing,
            )
    }
}

/// The sketch being edited, and how it's shown.
#[derive(Debug, Clone, Copy)]
pub struct SketchState<'a> {
    /// The sketch feature's name.
    pub name: &'a str,
    /// Whether the sketch differs from what it was when entered.
    pub modified: bool,
    pub plane: Plane,
    /// Where its plane is.
    pub placement: Placement,
    /// The sketch as it's shown: as committed, with the edits waiting on
    /// the solver applied, or as it's dragged.
    pub sketch: &'a Sketch,
    /// The items edits waiting on the solver add, not committed yet:
    /// they're drawn faded.
    pub pending: &'a BTreeSet<Id>,
    /// The items selected.
    pub selection: &'a BTreeSet<Selectable>,
    /// The points and curves the Constraints list lists the constraints
    /// and dimensions on, all of them when none: those selected, kept
    /// while only constraints and dimensions are, so the list stays as it
    /// is while its rows are clicked.
    pub listed_on: &'a BTreeSet<Id>,
    /// The tool in use, if one is.
    pub tool: Option<ActiveTool<'a>>,
    /// Whether the Constrain tool is in use.
    pub constraining: bool,
    /// The share of the Sketch tab's height the Geometry list takes.
    pub split: f32,
    /// How far the Geometry list is scrolled, in pixels.
    pub scroll: f32,
    /// How far the Constraints list is scrolled, in pixels.
    pub constraint_scroll: f32,
    /// What the constraints do to the sketch as committed, once the solver
    /// has said.
    pub analysis: Option<&'a Analysis>,
    /// Why the last edit was refused by the solver, shown until the next
    /// action, with the constraints involved in red.
    pub refusal: Option<&'a Rejected>,
    /// Why the solver couldn't answer about the last edit, if it couldn't:
    /// it panicked, or its worker stopped.
    pub solver_error: Option<&'a str>,
    /// The item hovered in a list or by its glyph, if any.
    pub hovered: Option<Selectable>,
    /// Whether the constraints' glyphs are shown.
    pub glyphs: bool,
    /// Whether the sketch doesn't solve, as regenerating found: a file's
    /// that a different build solved, say.
    pub unsolved: bool,
    /// Whether an edit has waited on the solver long enough to say so.
    pub checking: bool,
    /// The design's units, which dimensions show in.
    pub units: LengthUnit,
    /// The value field, if it's open.
    pub value: Option<ValueField<'a>>,
    /// The dimension whose label is grabbed, if one is, and how far it's
    /// been dragged, in sketch units: shown there until it's dropped.
    pub label_drag: Option<(Id, DVec2)>,
    /// Where the drawing tool's next click would snap to with the cursor
    /// where it is, if anywhere: its glyph shows by the cursor.
    pub snap: Option<Snap>,
    /// Where the drawing tool's next click would go with the cursor where
    /// it last was, while its shape has fields: they show by it.
    pub aim: Option<DVec2>,
    /// The regions the sketch as shown encloses, shaded, or that it's too
    /// complex to find them: `None` until the app has looked.
    pub profiles: Option<&'a Result<Arc<Profiles>, TooComplex>>,
    /// Whether the curvature comb of the splines selected shows.
    pub comb: bool,
    /// The curves a failing feature using the sketch names, as the model
    /// shown found it: drawn red within the halo the failures' geometry
    /// has in the model. Ids the sketch doesn't hold as curves are
    /// ignored.
    pub failing: &'a BTreeSet<Id>,
    /// The sketch's links as its Sketch tab lists them, in the links'
    /// order.
    pub links: &'a [LinkRow],
    /// The link whose row's context menu is open, if one's is.
    pub link_menu: Option<Id>,
    /// The point or curve whose Geometry row's context menu is open, if
    /// one's is.
    pub item_menu: Option<Selectable>,
    /// The Geometry list's groups folded.
    pub folded: &'a BTreeSet<crate::GeometryGroup>,
    /// The curves whose Geometry rows are unfolded, listing their points.
    pub expanded: &'a BTreeSet<Id>,
    /// Whether the document can be changed: a link's menu acts only then.
    pub editable: bool,
}

/// A link of the sketch being edited, as its Sketch tab lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkRow {
    pub link: Id,
    pub kind: LinkKind,
    /// What it comes from, by name: "Edge of Body 1", "Line 3 of Sketch
    /// 2".
    pub source: String,
    /// Why it found nothing, as the model shown found, if it didn't: its
    /// row in the danger colour with why.
    pub broken: Option<String>,
    /// Whether its curves count for profiles.
    pub profiles: bool,
    /// Whether it's the sketch face, the outline of the face the sketch
    /// is on, which can't be removed: its menu has no Remove.
    pub sketch_face: bool,
}

/// The value field of a dimension: placing one with the Dimension tool,
/// or changing one's value in place.
#[derive(Debug, Clone, Copy)]
pub struct ValueField<'a> {
    pub target: &'a ValueTarget,
    /// The text as typed so far.
    pub text: &'a str,
    /// Why the text last submitted was refused, if it was, with the part
    /// of it that's about.
    pub error: Option<&'a varde_expr::Error>,
    /// Whether it's in the Constraints list, rather than over the
    /// viewport at the label.
    pub in_list: bool,
}

/// What the value field sets the value of.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueTarget {
    /// A dimension being placed, not in the sketch until the value is
    /// taken: what it measures, on which side, and its label, from the
    /// measure's anchor (see [`varde_sketch::Dimension`]).
    New {
        measure: Measure,
        side: Side,
        label: DVec2,
    },
    /// The dimension of the sketch with this id.
    Dimension(Id),
    /// A field of the drawing tool's, next to the cursor: see
    /// [`crate::typed`].
    Field(Field),
}

#[cfg(test)]
impl<'a> SketchState<'a> {
    /// `sketch` on XY with `selection` selected and `tool` in use, as
    /// committed, analysed or not, with no glyphs.
    pub(crate) fn plain(
        sketch: &'a Sketch,
        selection: &'a BTreeSet<Selectable>,
        tool: Option<ActiveTool<'a>>,
    ) -> Self {
        static NONE: BTreeSet<Id> = BTreeSet::new();
        static NO_GROUPS: BTreeSet<crate::GeometryGroup> = BTreeSet::new();
        SketchState {
            name: "Sketch",
            modified: false,
            plane: Plane::Origin(varde_document::OriginPlane::XY),
            placement: varde_document::OriginPlane::XY.placement(),
            sketch,
            pending: &NONE,
            selection,
            // Leaked: a test's state lives as long as its test.
            listed_on: Box::leak(Box::new(
                selection.iter().map(|target| target.id()).collect(),
            )),
            tool,
            constraining: false,
            split: 0.5,
            scroll: 0.0,
            constraint_scroll: 0.0,
            analysis: None,
            refusal: None,
            solver_error: None,
            hovered: None,
            glyphs: false,
            unsolved: false,
            checking: false,
            units: LengthUnit::Mm,
            value: None,
            label_drag: None,
            snap: None,
            aim: None,
            profiles: None,
            comb: false,
            failing: &NONE,
            links: &[],
            link_menu: None,
            item_menu: None,
            folded: &NO_GROUPS,
            expanded: &NONE,
            editable: true,
        }
    }
}

impl SketchState<'_> {
    /// The constraints in conflict, and arcs for the equation each
    /// implies, to show in red: those the analysis finds in a dependency,
    /// which a sketch as committed only has if it came so from a file,
    /// and those a refused edit ran into, which the sketch holds.
    pub(crate) fn conflicts(&self) -> BTreeSet<Id> {
        let analysed = self.analysis.map(|analysis| &analysis.redundant);
        let refused = self.refusal.map(Rejected::involved);
        analysed
            .into_iter()
            .chain(refused)
            .flatten()
            .copied()
            .filter(|&id| self.sketch.kind(id).is_some())
            .collect()
    }

    /// The items to show in red: the [`conflicts`](Self::conflicts), and
    /// the points and curves the constraints and dimensions among them
    /// tie together.
    pub(crate) fn conflicting_items(&self) -> BTreeSet<Id> {
        let conflicts = self.conflicts();
        let tied = conflicts.iter().flat_map(|&id| tied_items(self.sketch, id));
        tied.chain(conflicts.iter().copied()).collect()
    }
}

/// The points and curves the constraint or dimension `id` of `sketch`
/// ties together, or `id` itself if it's neither.
pub(crate) fn tied_items(sketch: &Sketch, id: Id) -> Vec<Id> {
    if let Some(entry) = sketch.constraint(id) {
        entry.constraint.items().map(|(item, _)| item).collect()
    } else if let Some(entry) = sketch.dimension(id) {
        entry
            .dimension
            .measure
            .items()
            .map(|(item, _)| item)
            .collect()
    } else {
        vec![id]
    }
}

/// The tool in use in a sketch, and the shape it's drawing.
#[derive(Debug, Clone, Copy)]
pub struct ActiveTool<'a> {
    pub tool: Tool,
    /// The points of the shape being drawn placed so far, in sketch
    /// coordinates: the start of the next line, a circle's centre, an
    /// arc's ends.
    pub placed: &'a [DVec2],
    /// What each point placed snapped to, if anything: a point of the
    /// sketch's is the shape's own.
    pub targets: &'a [Option<Target>],
    /// Whether it draws construction geometry.
    pub construction: bool,
    /// The Dimension tool's items picked so far, to measure, the Mirror
    /// tool's to mirror, the chain the Offset tool offsets, or the corner
    /// Fillet or Chamfer is on, as its point and its lines (see
    /// [`Tool::pick`](crate::Tool::pick)).
    pub picked: &'a [Selectable],
    /// Whether the Mirror tool has what it mirrors and waits for the line
    /// to mirror about.
    pub about: bool,
    /// Whether the Dimension tool measures a circle's radius, or an
    /// arc's diameter, rather than the other.
    pub switched: bool,
    /// The values typed in the tool's fields for the shape being drawn,
    /// each fixing what its field measures: see [`crate::typed`].
    pub typed: &'a [(Field, varde_expr::Value)],
    /// How many sides the Polygon tool draws.
    pub sides: u32,
    /// Whether the Rectangle tool draws from the centre, rather than from
    /// a corner.
    pub centered: bool,
    /// Whether the Spline tool draws by control points, rather than
    /// through fit points.
    pub control: bool,
}

impl ActiveTool<'_> {
    /// What the tool asks of the user next in `sketch`, for the status
    /// bar.
    pub(crate) fn step(&self, sketch: &Sketch) -> &'static str {
        match (self.tool, self.placed.len()) {
            (Tool::Line, 0) | (Tool::Arc, 0) => "Click start point",
            (Tool::Line, _) => "Click next point",
            (Tool::Circle | Tool::Polygon, 0) => "Click center point",
            (Tool::Rectangle, 0) if self.centered => "Click center point",
            (Tool::Rectangle, 0) => "Click first corner",
            (Tool::Rectangle, _) if self.centered => "Click a corner",
            (Tool::Rectangle, _) => "Click opposite corner",
            (Tool::Polygon, _) => "Click a corner",
            (Tool::Circle, _) => "Click a point on the circle",
            (Tool::Arc, 1) => "Click end point",
            (Tool::Arc, _) => "Click a point on the arc",
            (Tool::Point, _) => "Click to place a point",
            (Tool::Spline, 0) if self.control => "Click first control point",
            (Tool::Spline, 0) => "Click first fit point",
            (Tool::Spline, _) if self.control => "Click next control point",
            (Tool::Spline, _) => "Click next fit point",
            (Tool::Dimension, _) => match *self.picked {
                [] => "Click what to measure",
                // A handle's angle, or with its fit point its length.
                [one] if sketch.handle(one.id()).is_some() => "Click to place, or pick another",
                [Selectable::Item(one)] if sketch.point(one).is_some() => "Click a point or a line",
                [Selectable::Item(one)] if sketch.line(one).is_some() => {
                    "Click to place, or pick another"
                }
                _ => "Click to place",
            },
            (Tool::Trim, _) => "Click the piece to trim away",
            (Tool::Extend, _) => "Click a curve near the end to extend",
            (Tool::Offset, _) if self.picked.is_empty() => "Click the chain or loop to offset",
            (Tool::Offset, _) => "Click the side and distance to offset by",
            (Tool::Mirror, _) if self.about => "Click the line to mirror about",
            (Tool::Mirror, _) => "Click geometry to mirror",
            (Tool::Fillet | Tool::Chamfer, _) if self.picked.is_empty() => {
                "Click a corner where two lines meet"
            }
            (Tool::Fillet, _) => "Click to round it this far",
            (Tool::Chamfer, _) => "Click to cut it this far",
            (Tool::Project, _) => "Click edges, corners or other sketches' geometry to project",
            (Tool::Intersect, _) => "Click faces or edges to cut with the sketch's plane",
        }
    }
}

/// Unsaved changes a session that crashed left of a document.
pub struct RecoveredChanges {
    /// Whether the design changed since the changes were made to it, so
    /// restoring them may undo changes saved since.
    pub design_changed: bool,
    /// Whether they were made to a newer save of the design than the one
    /// that could be read: then they may be the best copy there is, and
    /// aren't warned against.
    pub newer_base: bool,
    /// How the file they were auto-saved to was found damaged, if it was.
    pub damage: Option<Damage>,
    /// Whether they can't be read at all: the file holding them is
    /// damaged, and kept until discarded, so there's nothing to restore.
    pub unreadable: bool,
}

/// How reading a file found it damaged, and so which of its saves is
/// open, with when that one was saved, like "2 h ago" (lower case, to go
/// in a sentence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    /// Some earlier saves are damaged; the newest is open.
    Bridged,
    /// The newest save is damaged; the one before it, from `from`, is open.
    NewestDamaged { from: String },
    /// Damaged past the save from `from`, which the user chose to open.
    Damaged { from: String },
}

/// The file a document was opened from, found damaged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamagedFile {
    pub damage: Damage,
    /// Whether the file is the store entry of a new design, holding
    /// auto-saves, rather than a design's own file.
    pub auto_saves: bool,
}

/// How the mesh shown stands against the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshStatus<'a> {
    /// The mesh is of the document.
    Current,
    /// The document is newer, and its mesh is still being built.
    Regenerating,
    /// The document's mesh couldn't be built, and why. The mesh shown is an
    /// older one.
    Failed(&'a str),
}

/// Asks whether to delete a feature or a body, and every feature and
/// body that goes with it, before it's deleted.
#[derive(Debug, Clone)]
pub struct DeletePrompt<'a> {
    /// The name of the feature or body asked to be deleted.
    pub name: &'a str,
    /// Whether it's a body, which goes with the feature making it, rather
    /// than a feature.
    pub body: bool,
    /// The features that go, in the Timeline's order: those depending on
    /// it, and it or the feature making it.
    pub features: Vec<&'a Feature>,
    /// The bodies those make, which go with them.
    pub bodies: Vec<&'a Body>,
    /// The joins, cuts and intersects that stay but worked only on bodies
    /// that go, in the Timeline's order: they may fail without them.
    pub worked: Vec<&'a Feature>,
    /// The bodies that go that those worked on, in the bodies' order.
    pub worked_on: Vec<&'a Body>,
}

/// A sketch edit the solver refused after its sketch was left, which
/// nothing else would show: the edit is gone.
#[derive(Debug, Clone, Copy)]
pub struct RefusedEdit<'a> {
    /// The sketch feature's name.
    pub name: &'a str,
    /// The sketch as committed, without the edit.
    pub sketch: &'a Sketch,
    /// Why the solver refused it, or else
    pub why: Option<&'a Rejected>,
    /// how it failed to answer.
    pub error: Option<&'a str>,
}

/// A layer over the whole document screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    /// Asks what to do about unsaved changes.
    UnsavedPrompt,
    /// The app's own Save As dialog, see [`NamePrompt`].
    NamePrompt,
    FileMenu,
    /// The view options menu, from the status bar, and its submenu open
    /// if one is.
    ViewMenu(Option<crate::ViewSubmenu>),
}

/// The app's own Save As dialog, on the web, where browser storage has no
/// picker of the system's: the design's name, and where the File System
/// Access API is, whether to save it in browser storage or as a file on
/// the computer. It renames the design instead, in browser storage, if
/// `rename`.
#[derive(Debug, Clone)]
pub struct NamePrompt<'a> {
    /// As typed.
    pub name: &'a str,
    pub place: SavePlace,
    /// Whether to offer saving as a file on the computer: where the File
    /// System Access API is.
    pub places: bool,
    pub rename: bool,
    /// The file name in browser storage the name typed is taken by, if
    /// it's asked about: confirming again replaces it, unless renaming,
    /// which never does.
    pub taken: Option<&'a str>,
}

/// The document screen: toolbar on top, side panel on the left and the 3D
/// viewport filling the rest.
pub fn document<'a>(state: DocumentState<'a>) -> Element<'a, Message> {
    let editable = state.editable();

    let read_only = state
        .read_only
        .map(|reason| banner(text("Read-only").font(theme::SEMIBOLD), reason, None));
    let save_error = state.save_error.as_deref().map(|error| {
        let save_as =
            small_button("Save As…", Emphasis::Primary).on_press(Message::File(File::SaveAs));
        let dismiss = small_button("Dismiss", Emphasis::Secondary)
            .on_press(Message::Edit(Edit::DismissSaveError));
        banner(
            text("Couldn't save")
                .font(theme::SEMIBOLD)
                .style(theme::danger_text),
            error,
            Some(row![save_as, dismiss].spacing(6).into()),
        )
    });

    let export_error = state.export_error.map(|error| {
        let dismiss = small_button("Dismiss", Emphasis::Secondary)
            .on_press(Message::Edit(Edit::DismissExportError));
        banner(
            text("Couldn't export")
                .font(theme::SEMIBOLD)
                .style(theme::danger_text),
            error,
            Some(dismiss.into()),
        )
    });

    let recovered = state.recovered.as_ref().map(|recovered| {
        // Nothing to restore of what can't be read.
        let restore = (!recovered.unreadable).then(|| {
            small_button("Restore", Emphasis::Primary).on_press(Message::File(File::RestoreChanges))
        });
        let discard = small_button("Discard", Emphasis::Secondary)
            .on_press(Message::File(File::DiscardChanges));
        banner(
            text("Unsaved changes found").font(theme::SEMIBOLD),
            &recovered_detail(recovered),
            Some(row![restore, discard].spacing(6).into()),
        )
    });

    let damage = state.damage.as_ref().map(|damaged| {
        let dismiss = small_button("Dismiss", Emphasis::Secondary)
            .on_press(Message::Edit(Edit::DismissDamage));
        banner(
            text("Damaged file")
                .font(theme::SEMIBOLD)
                .style(theme::warning_text),
            &damage_detail(damaged),
            Some(dismiss.into()),
        )
    });

    // On the panel's colour, the window's, rather than over whatever's
    // behind: a banner's own is translucent.
    let banners = container(column![
        read_only,
        damage,
        recovered,
        save_error,
        export_error
    ])
    .width(Length::Fill)
    .style(theme::toolbar);
    let refused = (state.refused_edit)
        .map(|refused| container(refused_banner(refused)).style(theme::toolbar));

    let content = column![
        toolbar::toolbar(&state),
        banners,
        row![
            panels::side_panel(&state),
            column![
                refused,
                // The status bar floats over the viewport's bottom right.
                stack![
                    viewport::viewport(
                        state.mesh,
                        state.part_opacity(),
                        state.part_tints(),
                        state.sketches,
                        state.camera,
                        state.pivot,
                        state.picking.clone(),
                        state.highlight,
                        state.hover_through,
                        state.errors,
                        state.origin_drawn(),
                        state.options,
                        state.mode.palette(),
                        state
                            .sketch
                            .map(|sketch| viewport::Sketching::new(sketch, editable)),
                        operating(&state),
                        (state.extrude.as_ref().map(crate::extrude::panel))
                            .or_else(|| state.revolve.as_ref().map(crate::revolve::panel))
                            .or_else(|| state.combine.as_ref().map(crate::combine::panel))
                            .or_else(|| state.motion.as_ref().map(crate::motion::panel))
                            .or_else(|| state.measure.as_ref().map(crate::measure::panel)),
                        crate::rail::rail(&state),
                        state.overlaps.map(|overlaps| crate::overlaps::view(
                            overlaps,
                            (state.sketch.as_ref()).map(|sketch| (sketch.sketch, sketch.selection)),
                            state.model_selection,
                            state.overlap_ticks.as_deref(),
                            state.editor.document(),
                        )),
                        state.thumbnail,
                        Some(state.aspect.unwrap_or(0.0)),
                    ),
                    status::status_bar(status(&state)),
                    state.regenerating.map(crate::regenerating::regenerating),
                ],
            ],
        ]
        .height(Length::Fill),
    ];
    // The prompt about unsaved changes shows over the delete prompt,
    // which shows over the menus.
    match (state.overlay, &state.deleting) {
        (Some(Overlay::UnsavedPrompt), _) => {
            Element::from(stack![content, unsaved_prompt(state.name)])
        }
        (Some(Overlay::NamePrompt), _) => match &state.naming {
            Some(prompt) => Element::from(stack![content, name_prompt(prompt.clone())]),
            None => content.into(),
        },
        (_, Some(deleting)) => Element::from(stack![content, delete_prompt(deleting)]),
        (Some(Overlay::FileMenu), None) => {
            let document = state.editor.document();
            let menu = toolbar::file_menu(toolbar::FileMenu {
                editable,
                edited: state.edited,
                exportable: state.exportable,
                units: document.units(),
                tolerance: document.tolerance(),
                downloads: state.downloads.clone(),
                downloadable: state.downloadable,
                rename: state.rename,
            });
            Element::from(stack![content, menu])
        }
        (Some(Overlay::ViewMenu(submenu)), None) => {
            let menu = status::view_menu(state.camera.projection(), state.options, submenu);
            Element::from(stack![content, menu])
        }
        (None, None) => content.into(),
    }
}

/// The operation being set up, as the viewport shows it, if one is.
fn operating<'a>(state: &DocumentState<'a>) -> Option<viewport::Operating<'a>> {
    let extruding = state.extrude.clone().map(viewport::Extruding::new);
    let revolving = state.revolve.clone().map(viewport::Revolving::new);
    let measuring = state.measure.clone().map(viewport::Measuring::new);
    let moving = state.motion.clone().map(viewport::Moving::new);
    (extruding.map(viewport::Operating::Extrude))
        .or_else(|| revolving.map(viewport::Operating::Revolve))
        .or_else(|| moving.map(viewport::Operating::Motion))
        .or_else(|| measuring.map(viewport::Operating::Measure))
}

/// How opaque each part of the mesh is drawn, the parts being of `parts`'
/// bodies in order: as its body in `document` is shown ([`shown_opacity`]),
/// or opaque if it's of none there, as a draft's new body is.
fn part_opacity(
    document: &Document,
    parts: &[BodyId],
    preview: Option<(BodyId, Opacity)>,
) -> Arc<[f32]> {
    parts
        .iter()
        .map(|&id| (document.body(id)).map_or(1.0, |body| shown_opacity(body, preview).alpha()))
        .collect()
}

/// How opaque `body` is shown: as `preview` has it if it's of `body`, else
/// its own.
pub(crate) fn shown_opacity(body: &Body, preview: Option<(BodyId, Opacity)>) -> Opacity {
    match preview {
        Some((id, opacity)) if id == body.id => opacity,
        _ => body.opacity,
    }
}

/// The colour each part of the mesh is drawn in, the parts being of
/// `parts`' bodies in order: as its body in `document` is shown
/// ([`shown_color`]), or the theme's if it's of none there.
fn part_tints(
    document: &Document,
    parts: &[BodyId],
    preview: Option<(BodyId, Tint)>,
) -> Arc<[Option<BodyTint>]> {
    parts
        .iter()
        .map(|&id| {
            let body = document.body(id)?;
            shown_color(body, preview).map(body_tint)
        })
        .collect()
}

/// The colour `body` is shown in, none for the theme's: as `preview` has
/// it if it's of `body`, else its own.
pub(crate) fn shown_color(body: &Body, preview: Option<(BodyId, Tint)>) -> Option<Tint> {
    match preview {
        Some((id, tint)) if id == body.id => Some(tint),
        _ => body.color,
    }
}

/// `tint` as the renderer takes it.
pub fn body_tint(tint: Tint) -> BodyTint {
    BodyTint {
        hue: f32::from(tint.hue()),
        saturation: f32::from(tint.saturation()) / 100.0,
    }
}

/// What the status bar shows: the feature selected, what's going on, the
/// hints, and the view options menu's button.
fn status<'a>(state: &DocumentState<'a>) -> Status<'a> {
    Status {
        selection: selection(state),
        info: info(state),
        hints: hints(state),
        mouse_hints: state.options.mouse_hints,
        view_menu: Some(matches!(state.overlay, Some(Overlay::ViewMenu(_)))),
    }
}

/// The status bar's hints: what the keys do for what's going on and the
/// viewport's mouse bindings; under the unsaved changes or delete prompt
/// only that `Esc` cancels it, under the name prompt what `Enter` does, if
/// anything, and that `Esc` cancels it.
fn hints<'a>(state: &DocumentState<'a>) -> Vec<Hint<'a>> {
    // The unsaved changes and delete prompts take every key but `Esc`,
    // and the viewport behind them nothing.
    if state.overlay == Some(Overlay::UnsavedPrompt) || state.deleting.is_some() {
        return vec![key_hint(Shortcut::ESCAPE, "Cancel")];
    }
    // The name prompt's field takes the keys: `Enter` confirms it, as
    // its button says, unless that's blocked, and `Esc` cancels it.
    if state.overlay == Some(Overlay::NamePrompt)
        && let Some(prompt) = &state.naming
    {
        let (action, blocked) = name_action(prompt);
        let enter = (!blocked).then(|| key_hint(Shortcut::ENTER, action));
        return (enter.into_iter())
            .chain([key_hint(Shortcut::ESCAPE, "Cancel")])
            .collect();
    }
    let sketching = !left_orbits(state.sketch.as_ref());
    let keys: Vec<_> = if state.picking_plane.is_some() {
        vec![key_hint(Shortcut::ESCAPE, "Cancel")]
    } else if let Some(extrude) = &state.extrude {
        let pick = extrude
            .editable
            .then(|| step_hint(MouseButton::Left, "Pick regions"));
        let ok = extrude.ready.then(|| key_hint(Shortcut::ENTER, "OK"));
        [pick, ok, Some(key_hint(Shortcut::ESCAPE, "Cancel"))]
            .into_iter()
            .flatten()
            .collect()
    } else if let Some(revolve) = &state.revolve {
        let pick = revolve.editable.then(|| {
            let what = match revolve.picking {
                RevolvePick::Regions => "Pick regions",
                RevolvePick::Axis => "Pick the axis",
            };
            step_hint(MouseButton::Left, what)
        });
        let ok = revolve.ready.then(|| key_hint(Shortcut::ENTER, "OK"));
        [pick, ok, Some(key_hint(Shortcut::ESCAPE, "Cancel"))]
            .into_iter()
            .flatten()
            .collect()
    } else if let Some(combine) = &state.combine {
        let pick = combine.editable.then(|| {
            let what = match combine.picking {
                crate::CombinePick::Target => "Pick the target",
                crate::CombinePick::Tools => "Pick tools",
            };
            step_hint(MouseButton::Left, what)
        });
        let ok = combine.ready.then(|| key_hint(Shortcut::ENTER, "OK"));
        [pick, ok, Some(key_hint(Shortcut::ESCAPE, "Cancel"))]
            .into_iter()
            .flatten()
            .collect()
    } else if let Some(motion) = &state.motion {
        let pick = motion.editable.then(|| {
            let what = match (motion.picking, motion.kind) {
                (crate::MotionPick::Nothing, _) => return None,
                (crate::MotionPick::Bodies, crate::MotionKind::Align) => "Pick the body",
                (crate::MotionPick::Align(slot), _) => match slot.role {
                    crate::AlignRole::Point => "Pick the point",
                    crate::AlignRole::Primary => "Pick the direction",
                    crate::AlignRole::Secondary => "Pick the second direction",
                },
                (crate::MotionPick::Point, _) => "Pick the point",
                (crate::MotionPick::Edge, _) => "Pick the edge",
                (crate::MotionPick::Edges, _) => "Pick edges",
                (crate::MotionPick::Faces, crate::MotionKind::Shell) => "Pick faces to remove",
                (crate::MotionPick::Faces, crate::MotionKind::OffsetFace) => "Pick faces to move",
                (crate::MotionPick::Faces, crate::MotionKind::Draft) => "Pick faces to draft",
                (crate::MotionPick::Faces, _) => "Pick faces",
                (crate::MotionPick::Tool, _) => {
                    (motion.split.as_ref()).map_or("Pick the tool", |split| split.mode.hint())
                }
                (crate::MotionPick::Regions, crate::MotionKind::Loft) => {
                    "Pick sections: regions or points"
                }
                (crate::MotionPick::Path, crate::MotionKind::Loft) => "Pick the rails' curves",
                (crate::MotionPick::Regions, _) => "Pick regions",
                (crate::MotionPick::Path, _) => "Pick the path's curves or edges",
                (crate::MotionPick::Bodies, crate::MotionKind::Split) => "Pick the body",
                (crate::MotionPick::Bodies, _) => "Pick bodies",
                (crate::MotionPick::Reference, crate::MotionKind::LinearPattern) => {
                    "Pick the direction"
                }
                (crate::MotionPick::Reference, crate::MotionKind::Mirror) => "Pick the plane",
                (crate::MotionPick::Reference, crate::MotionKind::Draft) => {
                    "Pick the neutral plane"
                }
                (crate::MotionPick::Reference, _) => "Pick the axis",
            };
            Some(step_hint(MouseButton::Left, what))
        });
        let pick = pick.flatten();
        let ok = motion.ready.then(|| key_hint(Shortcut::ENTER, "OK"));
        [pick, ok, Some(key_hint(Shortcut::ESCAPE, "Cancel"))]
            .into_iter()
            .flatten()
            .collect()
    } else if let Some(measure) = &state.measure {
        measure_hints(measure)
    } else if let Some(sketch) = state.sketch {
        sketch_hints(&sketch, state.editable())
    } else if state.selected_feature.is_some() {
        let delete = state
            .editable()
            .then(|| key_hint(Shortcut::DELETE, "Delete"));
        [Some(key_hint(Shortcut::ENTER, "Edit")), delete]
            .into_iter()
            .flatten()
            .collect()
    } else if state.picking.is_some() {
        selecting_hints(state.model_selection)
    } else {
        Vec::new()
    };
    keys.into_iter().chain(viewport::hints(sketching)).collect()
}

/// Whether the left button orbits the camera, as outside a sketch: not
/// in `sketch`, where it's the sketch's, but while its tool picks outside
/// it ([`Tool::picks_outside`]).
fn left_orbits(sketch: Option<&SketchState<'_>>) -> bool {
    sketch.is_none_or(|sketch| (sketch.tool).is_some_and(|tool| tool.tool.picks_outside()))
}

/// The status bar's hints for the measure tool: a click picks A, then B,
/// then A again; `Shift` with it replaces B; a double-click picks the
/// body; `Esc` leaves.
fn measure_hints<'a>(measure: &crate::MeasureState<'a>) -> Vec<Hint<'a>> {
    let click = match measure.picks {
        [Some(_), None] => "Pick B",
        _ => "Pick A",
    };
    vec![
        step_hint(MouseButton::Left, click),
        chord_hint(Held::TOGGLE, MouseButton::Left, "Replace B"),
        chrome::double_hint(MouseButton::Left, "Body"),
        key_hint(Shortcut::ESCAPE, "Done"),
    ]
}

/// The status bar's hints for selecting in the model with `selection`:
/// clicking to select and, once something is, holding [`Held::TOGGLE`] to
/// add or take out, and double-clicking for the body where that selects
/// it.
fn selecting_hints<'a>(selection: &crate::Selection) -> Vec<Hint<'a>> {
    use crate::SelectionMode;

    let click = if selection.is_empty() {
        mouse_hint(MouseButton::Left, "Select")
    } else {
        chord_hint(Held::TOGGLE, MouseButton::Left, "Add or remove")
    };
    let body = (selection.mode() == SelectionMode::Any)
        .then(|| chrome::double_hint(MouseButton::Left, "Body"));
    [Some(click), body].into_iter().flatten().collect()
}

/// The status bar's hints for the left button and the keys in `sketch`,
/// which can be changed if `editable`: what the tool asks for, placing
/// without snapping, typing values and how to stop it, or selecting, what
/// can be done with the selection and how to leave.
fn sketch_hints<'a>(sketch: &SketchState<'a>, editable: bool) -> Vec<Hint<'a>> {
    if let Some(field) = sketch.value {
        let enter = match field.target {
            ValueTarget::New { .. } | ValueTarget::Field(_) => "Place",
            ValueTarget::Dimension(_) => "Set",
        };
        let next = matches!(field.target, ValueTarget::Field(_))
            .then(|| key_hint(Shortcut::NEXT_FIELD, "Next value"));
        return [Some(key_hint(Shortcut::ENTER, enter)), next]
            .into_iter()
            .flatten()
            .chain([key_hint(Shortcut::ESCAPE, "Cancel")])
            .collect();
    }
    if let Some(tool) = sketch.tool
        && tool.tool == Tool::Dimension
    {
        return dimension_hints(sketch.sketch, &tool);
    }
    if let Some(tool) = sketch.tool
        && !tool.tool.draws()
    {
        let picked = !tool.picked.is_empty();
        let done = match tool.tool {
            Tool::Mirror => (picked && !tool.about).then_some((Shortcut::ENTER, "Pick the line")),
            Tool::Offset | Tool::Fillet | Tool::Chamfer if !tool.typed.is_empty() => {
                Some((Shortcut::ENTER, "Place"))
            }
            Tool::Offset => picked.then_some((Shortcut::NEXT_FIELD, "Type distance")),
            Tool::Fillet => picked.then_some((Shortcut::NEXT_FIELD, "Type radius")),
            Tool::Chamfer => picked.then_some((Shortcut::NEXT_FIELD, "Type distances")),
            _ => None,
        };
        let escape = if picked { "Cancel" } else { "Stop tool" };
        return [
            Some(step_hint(MouseButton::Left, tool.step(sketch.sketch))),
            done.map(|(key, label)| key_hint(key, label)),
            Some(key_hint(Shortcut::ESCAPE, escape)),
        ]
        .into_iter()
        .flatten()
        .collect();
    }
    if let Some(tool) = sketch.tool {
        let escape = match (tool.tool, tool.placed.is_empty()) {
            (_, true) => "Stop tool",
            (Tool::Line, false) => "End line",
            (_, false) => "Cancel",
        };
        let end = tool
            .spline_ends()
            .then(|| key_hint(Shortcut::ENTER, "End spline"));
        let kind = (tool.tool == Tool::Spline).then(|| {
            let label = if tool.control {
                "Through points"
            } else {
                "Control points"
            };
            key_hint(Shortcut::SPLINE_KIND, label)
        });
        let fields = !tool.fields().is_empty();
        let values = fields.then(|| key_hint(Shortcut::NEXT_FIELD, "Type values"));
        let place = (fields && !tool.typed.is_empty()).then(|| key_hint(Shortcut::ENTER, "Place"));
        let centered = (tool.tool == Tool::Rectangle).then(|| {
            let label = if tool.centered {
                "From corner"
            } else {
                "From center"
            };
            key_hint(Shortcut::CENTERED, label)
        });
        return [
            Some(step_hint(MouseButton::Left, tool.step(sketch.sketch))),
            Some(chord_hint(Held::FREE, MouseButton::Left, "Don't snap")),
            values,
            place,
            centered,
            end,
            kind,
            Some(key_hint(Shortcut::CONSTRUCTION, "Construction")),
            Some(key_hint(Shortcut::ESCAPE, escape)),
        ]
        .into_iter()
        .flatten()
        .collect();
    }
    if sketch.constraining {
        let most_likely = ConstraintKind::fitting(sketch.sketch, sketch.selection)
            .first()
            .and_then(|kind| Some(key_hint(kind.shortcut()?, kind.label())));
        let select = if sketch.selection.is_empty() {
            "Select geometry to constrain"
        } else {
            "Select"
        };
        return [
            Some(step_hint(MouseButton::Left, select)),
            most_likely,
            Some(key_hint(Shortcut::ESCAPE, "Stop tool")),
        ]
        .into_iter()
        .flatten()
        .collect();
    }
    let selection = (!sketch.selection.is_empty()).then(|| {
        let edits = editable.then(|| {
            [
                key_hint(Shortcut::DELETE, "Delete"),
                key_hint(Shortcut::CONSTRUCTION, "Construction"),
            ]
        });
        let reference = editable
            .then(|| reference_hint(sketch))
            .flatten()
            .map(|label| key_hint(Shortcut::REFERENCE, label));
        // Splines switched, given handles, and their curvature shown.
        let splines = crate::spline::any_selected(sketch.sketch, sketch.selection);
        let convert = (editable && splines).then(|| key_hint(Shortcut::SPLINE_KIND, "Convert"));
        let handles = crate::spline::handles(sketch.sketch, sketch.selection)
            .filter(|_| editable)
            .map(|edit| {
                let label = match edit {
                    varde_sketch::SketchEdit::Delete(_) => "No handles",
                    _ => "Handles",
                };
                key_hint(Shortcut::HANDLES, label)
            });
        let comb = splines.then(|| {
            let label = if sketch.comb { "Hide comb" } else { "Comb" };
            key_hint(Shortcut::COMB, label)
        });
        edits
            .into_iter()
            .flatten()
            .chain(reference)
            .chain(convert)
            .chain(handles)
            .chain(comb)
    });
    std::iter::once(mouse_hint(MouseButton::Left, "Select"))
        .chain(selection.into_iter().flatten())
        .chain([key_hint(Shortcut::ESCAPE, "Finish")])
        .collect()
}

/// The status bar's hints in the Dimension tool: what it asks for, the
/// radius or diameter to switch to, placing a reference and stopping.
fn dimension_hints<'a>(sketch: &Sketch, tool: &ActiveTool<'a>) -> Vec<Hint<'a>> {
    let placing = crate::dimension::measure(sketch, tool.picked, DVec2::ZERO, tool.switched);
    let switch = match placing {
        Some((Measure::Radius(_), _)) => Some("Diameter"),
        Some((Measure::Diameter(_), _)) => Some("Radius"),
        _ => None,
    };
    let escape = if tool.picked.is_empty() {
        "Stop tool"
    } else {
        "Cancel"
    };
    [
        Some(step_hint(MouseButton::Left, tool.step(sketch))),
        switch.map(|label| key_hint(Shortcut::SWITCH_ROUND, label)),
        placing
            .is_some()
            .then(|| chord_hint(Held::REFERENCE, MouseButton::Left, "Reference")),
        Some(key_hint(Shortcut::ESCAPE, escape)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// What the reference key makes of the dimensions selected, if any are:
/// references, unless they all are, then driving.
fn reference_hint(sketch: &SketchState<'_>) -> Option<&'static str> {
    let mut selected = sketch
        .selection
        .iter()
        .filter_map(|&target| sketch.sketch.dimension(target.item()?))
        .peekable();
    selected.peek()?;
    Some(if selected.any(|entry| entry.dimension.driving) {
        "Reference"
    } else {
        "Driving"
    })
}

/// The banner over the viewport saying a sketch edit was refused after
/// its sketch was left, and why, with Dismiss: "An edit of Sketch 1
/// wasn't kept — Would over-constrain the sketch".
fn refused_banner(refused: RefusedEdit<'_>) -> Element<'_, Message> {
    let dismiss = small_button("Dismiss", Emphasis::Secondary)
        .on_press(Message::Edit(Edit::DismissRefusedEdit));
    banner(
        text(format!("An edit of {} wasn't kept", refused.name))
            .font(theme::SEMIBOLD)
            .wrapping(Wrapping::None)
            .style(theme::warning_text),
        &refused_detail(&refused),
        Some(dismiss.into()),
    )
}

/// Why the solver refused the edit `refused` is of, as its banner says,
/// as the status bar would have in the sketch.
fn refused_detail(refused: &RefusedEdit<'_>) -> String {
    edit_refused(refused.why, refused.error, refused.sketch)
        .map_or_else(|| "The solver refused it".to_owned(), Cow::into_owned)
}

/// Why the solver refused an edit of `sketch`, `why` or else how it
/// failed to answer (`error`), as the status bar and the banner over the
/// viewport say it.
fn edit_refused(
    why: Option<&Rejected>,
    error: Option<&str>,
    sketch: &Sketch,
) -> Option<Cow<'static, str>> {
    why.map(|why| refusal_text(why, sketch))
        .or_else(|| error.map(|error| format!("Couldn't check the edit: {error}").into()))
}

/// A strip under the toolbar, or over the viewport, telling something
/// about the whole document: `title`, then `detail`, then any `actions`
/// at the right.
fn banner<'a>(
    title: impl Into<Element<'a, Message>>,
    detail: &str,
    actions: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        container(
            // The detail takes what's left and wraps, so a long one
            // doesn't push the actions off a small window.
            row![
                title.into(),
                text(format!("— {detail}"))
                    .style(theme::muted_text)
                    .width(Length::Fill),
                actions,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([6, 12])
        .style(theme::banner),
        chrome::hrule(),
    ]
    .into()
}

/// What the banner offering the changes a crashed session left says of
/// them: that there are some, and whether the design changed since, or
/// they were made to a newer save of it than could be read, how damaged
/// the file holding them is, or that they can't be read.
fn recovered_detail(recovered: &RecoveredChanges) -> String {
    let found =
        format!("{APP_NAME} closed unexpectedly while this design had changes that weren't saved");
    if recovered.unreadable {
        return format!("{found}, but the auto-save is damaged and can't be read");
    }
    let base = if recovered.newer_base {
        ", auto-saved from a newer save than could be read"
    } else if recovered.design_changed {
        ", but the design has changed since: restoring them may undo newer changes"
    } else {
        ""
    };
    let damage = match &recovered.damage {
        None => String::new(),
        Some(Damage::Bridged) => "; some earlier auto-saves of them are damaged".to_owned(),
        Some(Damage::NewestDamaged { from }) => {
            format!("; the newest auto-save of them is damaged, so these are from {from}")
        }
        Some(Damage::Damaged { from }) => format!(
            "; the auto-save of them is damaged, so these are the newest that can be read, \
             from {from}"
        ),
    };
    format!("{found}{base}{damage}")
}

/// What the banner about a damaged file says: which save is open.
fn damage_detail(damaged: &DamagedFile) -> String {
    let save = if damaged.auto_saves {
        "auto-save"
    } else {
        "save"
    };
    match &damaged.damage {
        Damage::Bridged => format!("Some earlier {save}s in this file are damaged"),
        Damage::NewestDamaged { from } => {
            format!("The newest {save} in this file is damaged; opened the one from {from}")
        }
        Damage::Damaged { from } => format!("This file is damaged; opened the {save} from {from}"),
    }
}

/// Asks whether to save the changes to the document `name` before it's
/// closed, as a dialog over the whole screen, which dims the rest and
/// keeps it from being clicked.
fn unsaved_prompt(name: &str) -> Element<'_, Message> {
    let choice = |label, choice, emphasis: Emphasis| {
        dialog_button(
            label,
            emphasis.button_style(),
            Some(Message::File(File::Unsaved(choice))),
        )
    };
    dialog(
        column![
            text(format!("Save the changes to {name}.{EXTENSION}?"))
                .size(14)
                .font(theme::SEMIBOLD),
            text("Your changes are lost if you don't save them.").style(theme::muted_text),
            Space::new().height(4),
            row![
                choice("Don't save", Unsaved::Discard, Emphasis::Secondary),
                space::horizontal(),
                choice("Cancel", Unsaved::Cancel, Emphasis::Secondary),
                choice("Save", Unsaved::Save, Emphasis::Primary),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
}

/// The app's own Save As dialog, see [`NamePrompt`], as a dialog over the
/// whole screen like [`unsaved_prompt`]: the name, with `.vrdp` after it,
/// where to save it if there's a choice, and what's in the way if the
/// name's taken. `Enter` in the field saves, as the primary button does.
/// What confirming the name prompt does, its button's label and the
/// `Enter` key's, and whether it's blocked (it then does nothing):
/// renaming to a name taken, or keeping in browser storage, which keeps a
/// design by its name, with none typed.
fn name_action(prompt: &NamePrompt<'_>) -> (&'static str, bool) {
    let browser = prompt.place == SavePlace::Browser || prompt.rename;
    let action = match (prompt.rename, browser, prompt.taken.is_some()) {
        (true, _, _) => "Rename",
        (false, false, _) => "Choose file…",
        (false, true, true) => "Replace",
        (false, true, false) => "Save",
    };
    let blocked =
        (prompt.rename && prompt.taken.is_some()) || (browser && prompt.name.trim().is_empty());
    (action, blocked)
}

fn name_prompt(prompt: NamePrompt<'_>) -> Element<'_, Message> {
    let browser = prompt.place == SavePlace::Browser || prompt.rename;
    let confirm = Message::File(File::ConfirmName);
    let field = text_input("Name", prompt.name)
        .id(crate::NAME_FIELD)
        .on_input(|name| Message::File(File::Name(name)))
        .on_submit(confirm.clone())
        .padding([6, 8])
        .style(theme::field_input(prompt.taken.is_some() && browser));
    let name = row![
        container(field).width(Length::Fill),
        text(format!(".{EXTENSION}")).style(theme::faint_text),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let places = (prompt.places && !prompt.rename).then(|| {
        let place = |label, place| {
            let chosen = prompt.place == place;
            toolbar::choice_item(
                match place {
                    SavePlace::Browser => Icon::Browser,
                    SavePlace::Computer => Icon::Computer,
                },
                label,
                chosen,
                Message::File(File::Place(place)),
            )
        };
        column![
            place("Browser storage", SavePlace::Browser),
            place("A file on your computer…", SavePlace::Computer),
        ]
    });
    // Where it's going, unless it stays where it is.
    let place = (!prompt.rename).then(|| {
        text(if browser {
            "Saved in this browser's storage."
        } else {
            "Saved to a file on your computer: you choose where next."
        })
        .size(12)
        .style(theme::muted_text)
    });
    let note = match (prompt.taken, prompt.rename) {
        (Some(taken), true) if browser => Some(
            text(format!(
                "A design in browser storage is called {taken} already."
            ))
            .style(theme::danger_text),
        ),
        (Some(taken), false) if browser => Some(
            text(format!(
                "A design in browser storage is called {taken} already. Save again to replace it."
            ))
            .style(theme::warning_text),
        ),
        _ => None,
    };
    let title = if prompt.rename {
        "Rename design"
    } else {
        "Save design as"
    };
    let (action, blocked) = name_action(&prompt);
    dialog(
        column![
            text(title).size(14).font(theme::SEMIBOLD),
            name,
            places,
            place,
            note,
            Space::new().height(4),
            row![
                space::horizontal(),
                dialog_button(
                    "Cancel",
                    Emphasis::Secondary.button_style(),
                    Some(Message::File(File::CancelName)),
                ),
                dialog_button(
                    action,
                    Emphasis::Primary.button_style(),
                    (!blocked).then_some(confirm),
                ),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
}

/// Asks whether to delete what `prompt` lists, as a dialog over the
/// whole screen like [`unsaved_prompt`]: [`delete_question`], then the
/// features, in the Timeline's order, and the bodies, scrolling past
/// about ten rows, [`delete_warning`] if there's one, and Cancel and
/// Delete.
fn delete_prompt<'a>(prompt: &DeletePrompt<'a>) -> Element<'a, Message> {
    /// The rows shown before the list scrolls.
    const ROWS: f32 = 10.5;
    let question = delete_question(prompt);
    let warning = delete_warning(prompt).map(|warning| text(warning).style(theme::warning_text));
    let item = |icon, name: &'a str| {
        row![icons::icon(icon, icons::INLINE), text(name)]
            .spacing(8)
            .height(panels::ROW_HEIGHT)
            .align_y(Alignment::Center)
            .into()
    };
    let features = prompt
        .features
        .iter()
        .map(|feature| item(panels::feature_icon(feature), feature.name.as_str()));
    let bodies = prompt
        .bodies
        .iter()
        .map(|body| item(Icon::Body, body.name.as_str()));
    let rows = prompt.features.len().saturating_add(prompt.bodies.len());
    // Counts are bounded by the document's, far below f32's exact range.
    let shown = (rows as f32).min(ROWS);
    let list = chrome::scrolled(column(features.chain(bodies)), 0.0)
        .height(shown * panels::ROW_HEIGHT)
        .width(Length::Fill);
    let cancel = dialog_button(
        "Cancel",
        theme::secondary_button,
        Some(Message::Look(Look::CancelDelete)),
    );
    let delete = dialog_button(
        "Delete",
        theme::danger_button,
        Some(Message::Edit(Edit::ConfirmDelete)),
    );
    dialog(
        column![
            text(question).size(14).font(theme::SEMIBOLD),
            list,
            warning,
            Space::new().height(4),
            row![space::horizontal(), cancel, delete].spacing(8),
        ]
        .spacing(8),
    )
}

/// What the delete prompt asks, counting what goes besides what was
/// asked to be deleted: "Delete Sketch 1 with the 1 feature and 1 body
/// that depend on it?", or for a body, "Delete Body 1 with the 2
/// features that go with it?", counting the one making it; a body going
/// with only that one names it: "Delete Body 1 and Extrude 1, which
/// makes it?".
fn delete_question(prompt: &DeletePrompt<'_>) -> String {
    if prompt.body
        && let ([maker], [_]) = (&prompt.features[..], &prompt.bodies[..])
    {
        return format!("Delete {} and {}, which makes it?", prompt.name, maker.name);
    }
    // A body asked for is one of the bodies listed, a feature one of the
    // features.
    let (features, bodies) = if prompt.body {
        (prompt.features.len(), prompt.bodies.len().saturating_sub(1))
    } else {
        (prompt.features.len().saturating_sub(1), prompt.bodies.len())
    };
    let parts: Vec<_> = [
        (features, "feature", "features"),
        (bodies, "body", "bodies"),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, one, many)| counted(n, one, many))
    .collect();
    if parts.is_empty() {
        return format!("Delete {}?", prompt.name);
    }
    let one = features.saturating_add(bodies) == 1;
    let tie = match (prompt.body, one) {
        (true, true) => "goes with",
        (true, false) => "go with",
        (false, true) => "depends on",
        (false, false) => "depend on",
    };
    format!(
        "Delete {} with the {} that {tie} it?",
        prompt.name,
        parts.join(" and ")
    )
}

/// What the delete prompt warns of, if a join, cut or intersect that
/// stays worked only on bodies that go: "Extrude 2 works on Body 1 and
/// stays, so it may fail with nothing to work on."
fn delete_warning(prompt: &DeletePrompt<'_>) -> Option<String> {
    if prompt.worked.is_empty() {
        return None;
    }
    let features = listed(prompt.worked.iter().map(|feature| feature.name.as_str()));
    let bodies = listed(prompt.worked_on.iter().map(|body| body.name.as_str()));
    let (works, stays, it) = if prompt.worked.len() == 1 {
        ("works", "stays", "it")
    } else {
        ("work", "stay", "they")
    };
    Some(format!(
        "{features} {works} on {bodies} and {stays}, so {it} may fail with nothing to work on."
    ))
}

/// `names` as a list in a sentence: "A", "A and B", "A, B and C".
fn listed<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> String {
    let count = names.len();
    let mut list = String::new();
    for (at, name) in names.enumerate() {
        if at > 0 {
            list.push_str(if at + 1 == count { " and " } else { ", " });
        }
        list.push_str(name);
    }
    list
}

/// What the status bar says while picking the plane for a new sketch
/// with a curved face under the cursor, and the app when a sketch is
/// asked for on one.
pub const CURVED_FACE: &str = "Only flat faces can be sketched on";

/// What the status bar says is going on: what's asked of the user while
/// picking a plane, what the sketch being edited holds or the extrude being
/// set up. Whether the mesh is still being regenerated, or why it couldn't
/// be built, if it couldn't; why the last edit was refused, if it was, and
/// whether a save is in flight. Nothing, with nothing of that, so with
/// nothing selected the bar has only its hints. On one line, cut short
/// where the bar doesn't fit (`status::status_bar`).
fn info<'a>(state: &DocumentState<'a>) -> Option<Element<'a, Message>> {
    if let Some(pick) = state.picking_plane {
        // A face hovered that can't take the sketch isn't highlighted:
        // this says why.
        let refused = (state.picking.as_ref()).and_then(|picking| match picking.hovered {
            Some(crate::Picked::Face(face)) => pick.refusal(picking.index, face),
            _ => None,
        });
        let (line, style): (String, fn(&iced::Theme) -> text::Style) = match refused {
            Some(why) => (why.into_owned(), theme::danger_text),
            None => (pick.asking(), theme::accent_text),
        };
        return Some(
            text(line)
                .size(12)
                .wrapping(Wrapping::None)
                .font(theme::SEMIBOLD)
                .style(style)
                .into(),
        );
    }
    if let Some(sketch) = &state.sketch {
        // A selection has the bar's box of its own: the sketch's standing
        // and profiles make way for it.
        let selected = !sketch.selection.is_empty();
        let piece = |line: String, style: fn(&iced::Theme) -> text::Style| {
            text(line).size(12).wrapping(Wrapping::None).style(style)
        };
        let standing = (!selected).then(|| standing(sketch)).flatten();
        let mut pieces: Vec<_> = (standing.into_iter())
            .map(|(standing, trouble)| {
                let style = if trouble {
                    theme::danger_text
                } else {
                    theme::muted_text
                };
                piece(standing, style)
            })
            .chain(
                (!selected)
                    .then(|| profile_count(sketch))
                    .flatten()
                    .map(|count| piece(count, theme::muted_text)),
            )
            .chain(
                status_notes(state)
                    .into_iter()
                    .map(|note| piece(note, theme::muted_text)),
            )
            .chain(
                edit_refused(sketch.refusal, sketch.solver_error, sketch.sketch)
                    .map(|why| piece(why.into_owned(), theme::danger_text)),
            )
            .chain(
                sketch
                    .checking
                    .then(|| piece("Checking…".to_owned(), theme::muted_text)),
            )
            .map(Element::from)
            .collect();
        if pieces.is_empty() {
            return None;
        }
        let mut line = Vec::with_capacity(pieces.len() * 2);
        for (i, element) in pieces.drain(..).enumerate() {
            if i > 0 {
                line.push(piece("·".to_owned(), theme::muted_text).into());
            }
            line.push(element);
        }
        return Some(row(line).spacing(4).into());
    }
    if let Some(extrude) = &state.extrude {
        let regions = match extrude.picked.len() {
            0 => "pick the regions to extrude".to_owned(),
            n => format!("{} picked", counted(n, "region", "regions")),
        };
        return Some(
            row![
                text(extrude.editing.unwrap_or("New extrude"))
                    .size(12)
                    .wrapping(Wrapping::None)
                    .font(theme::SEMIBOLD),
                text(format!("· {regions}{}", status_suffix(state)))
                    .size(12)
                    .wrapping(Wrapping::None)
                    .style(theme::muted_text),
            ]
            .spacing(4)
            .into(),
        );
    }
    if let Some(revolve) = &state.revolve {
        let regions = match revolve.picked.len() {
            0 => "pick the regions to revolve".to_owned(),
            n => format!("{} picked", counted(n, "region", "regions")),
        };
        let axis = match revolve.axis_name() {
            Some(name) => format!("about {name}"),
            None => "pick the axis".to_owned(),
        };
        return Some(
            row![
                text(revolve.editing.unwrap_or("New revolve"))
                    .size(12)
                    .wrapping(Wrapping::None)
                    .font(theme::SEMIBOLD),
                text(format!("· {regions} · {axis}{}", status_suffix(state)))
                    .size(12)
                    .wrapping(Wrapping::None)
                    .style(theme::muted_text),
            ]
            .spacing(4)
            .into(),
        );
    }
    if let Some(combine) = &state.combine {
        return Some(
            row![
                text(combine.editing.unwrap_or("New combine"))
                    .size(12)
                    .wrapping(Wrapping::None)
                    .font(theme::SEMIBOLD),
                text(format!(
                    "· {}{}",
                    combine_info(combine),
                    status_suffix(state)
                ))
                .size(12)
                .wrapping(Wrapping::None)
                .style(theme::muted_text),
            ]
            .spacing(4)
            .into(),
        );
    }
    if let Some(motion) = &state.motion {
        return Some(
            row![
                text(motion.title())
                    .size(12)
                    .wrapping(Wrapping::None)
                    .font(theme::SEMIBOLD),
                text(format!(
                    "· {}{}",
                    crate::motion::status_info(motion),
                    status_suffix(state)
                ))
                .size(12)
                .wrapping(Wrapping::None)
                .style(theme::muted_text),
            ]
            .spacing(4)
            .into(),
        );
    }
    let notes = status_notes(state);
    (!notes.is_empty()).then(|| {
        text(notes.join(" · "))
            .size(12)
            .wrapping(Wrapping::None)
            .style(theme::muted_text)
            .into()
    })
}

/// What the status bar says of the combine being set up: "Body 1 with 2
/// tools · Union", or what's still to pick.
fn combine_info(combine: &crate::CombineState<'_>) -> String {
    let Some(target) = combine.target else {
        return "pick the target body".to_owned();
    };
    match combine.tools.len() {
        0 => format!("{} · pick the tool bodies", target.name),
        n => format!(
            "{} with {} · {}",
            target.name,
            counted(n, "tool", "tools"),
            combine.op.label()
        ),
    }
}

/// The feature selected in the Timeline, for the status bar's box of the
/// selection: its icon, its name and [`feature_info`], or for one that
/// failed an alert, its name and why. Nothing in a sketch,
/// setting up an extrude or a revolve or picking a plane, which the bar
/// tells of instead.
fn selection<'a>(state: &DocumentState<'a>) -> Option<Element<'a, Message>> {
    if let Some(sketch) = &state.sketch {
        return sketch_selection(sketch);
    }
    if state.picking_plane.is_some()
        || state.extrude.is_some()
        || state.revolve.is_some()
        || state.combine.is_some()
        || state.motion.is_some()
        || state.measure.is_some()
    {
        return None;
    }
    let document = state.editor.document();
    let Some(feature) = state.selected_feature.and_then(|id| document.feature(id)) else {
        return model_selection(state);
    };
    // A feature that failed says why, after an alert in place of its icon.
    let failed = (state.failed.iter()).find(|failed| failed.feature == feature.id);
    if let Some(failed) = failed {
        return Some(
            row![
                icons::tinted(Icon::Alert, 14.0, |p| p.danger),
                text(feature.name.as_str())
                    .size(12)
                    .wrapping(Wrapping::None)
                    .font(theme::SEMIBOLD),
                text(crate::chrome::sentence(&failed.message))
                    .size(12)
                    .wrapping(Wrapping::None)
                    .style(theme::muted_text),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    Some(
        row![
            icons::icon(panels::feature_icon(feature), icons::INLINE),
            text(feature.name.as_str())
                .size(12)
                .wrapping(Wrapping::None)
                .font(theme::SEMIBOLD),
            text(feature_info(feature, document))
                .size(12)
                .wrapping(Wrapping::None)
                .style(theme::muted_text),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

/// What's selected in the sketch being edited, for the status bar's box
/// of the selection, while no tool is in use nor the value field open:
/// one item by name ("Line 3"), or how many, and what they measure
/// together as a dimension of them would ("Length 40 mm", see
/// [`dimension::selected`](crate::dimension::selected)); or a rectangle,
/// by its width and height (see
/// [`dimension::rectangle`](crate::dimension::rectangle)).
fn sketch_selection<'a>(sketch: &SketchState<'a>) -> Option<Element<'a, Message>> {
    if sketch.tool.is_some() || sketch.constraining || sketch.value.is_some() {
        return None;
    }
    let ids: Vec<Selectable> = sketch.selection.iter().copied().collect();
    let items: Option<Vec<Id>> = ids.iter().map(|target| target.item()).collect();
    let rectangle = items.and_then(|items| crate::dimension::rectangle(sketch.sketch, &items));
    let title = match ids[..] {
        [] => return None,
        _ if rectangle.is_some() => "Rectangle".to_owned(),
        [one] => (sketch.sketch.selectable_name(one)).unwrap_or_else(|| "1 selected".to_owned()),
        _ => format!("{} selected", ids.len()),
    };
    let measured = match rectangle {
        Some((width, height)) => {
            let unit = Some(varde_expr::Unit::Length(sketch.units));
            let [width, height] = [width, height].map(|size| varde_expr::format(size, unit));
            Some(format!("Width {width} · Height {height}"))
        }
        None => crate::dimension::selected(sketch.sketch, &ids)
            .map(|(measure, value)| crate::dimension::shown_measure(&measure, value, sketch.units)),
    };
    let measured = measured.map(|measured| text(measured).size(12).wrapping(Wrapping::None));
    Some(
        row![
            icons::icon(Icon::Sketch, icons::INLINE),
            text(title)
                .size(12)
                .wrapping(Wrapping::None)
                .font(theme::SEMIBOLD),
            measured,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

/// What's selected in the model, for the status bar's box of the
/// selection: one face, as "Face", what surface it's on (or "Rectangle",
/// for a flat face of four straight edges square at its corners) and its
/// body's name; one edge or vertex, as "Edge" or "Vertex" and its body's; one
/// body, by name; or how many, of each kind. Before the body's name, what
/// one or two items measure once the answer has it (see
/// [`measure::brief`](crate::measure::brief)). Nothing if nothing is
/// selected, or the cursor doesn't pick the model.
fn model_selection<'a>(state: &DocumentState<'a>) -> Option<Element<'a, Message>> {
    use crate::{Picked, Selected};

    let selection = state.model_selection;
    let picking = state.picking.as_ref()?;
    let document = state.editor.document();
    let body_name = |body: BodyId| document.body(body).map_or("", |body| body.name.as_str());
    let items: Vec<&Selected> = selection.items().collect();
    let fresh = selection.model() == Some(picking.index.model());
    let target = selection.targets().next().filter(|_| fresh);
    // The body a face or edge is of in the model: another than the one
    // it was picked in where a join merged that one into it.
    let drawn_in = |body: BodyId| {
        let drawn = target.and_then(|target| picking.index.body(target));
        body_name(drawn.unwrap_or(body))
    };
    let (title, info, note): (String, String, &str) = match items[..] {
        [] => return None,
        [Selected::Face { body, .. }] => {
            let summary = target.and_then(|target| {
                let Picked::Face(face) = target else {
                    return None;
                };
                picking.index.picking().faces().get(face as usize)
            });
            let surface = summary.map_or("", |face| surface_name(&face.summary));
            // A rectangle is named so once measured, its width and
            // height then shown.
            let rectangle = (state.selection_measured)
                .and_then(|inspected| inspected.first.as_ref().ok())
                .is_some_and(|probed| {
                    matches!(
                        probed.measure,
                        Ok(varde_regen::Measure::Face {
                            rectangle: Some(_),
                            ..
                        })
                    )
                });
            let surface = if rectangle { "Rectangle" } else { surface };
            ("Face".into(), surface.into(), drawn_in(*body))
        }
        [Selected::Edge { body, .. }] => ("Edge".into(), String::new(), drawn_in(*body)),
        [Selected::Vertex { body, .. }] => ("Vertex".into(), String::new(), drawn_in(*body)),
        [Selected::Body(body)] => (body_name(*body).into(), String::new(), "Body"),
        // "Line 3" of "Sketch 2".
        [Selected::SketchItem { sketch, item }] => {
            let feature = document.feature(*sketch);
            let name = feature.and_then(|feature| match &feature.kind {
                FeatureKind::Sketch { sketch, .. } => sketch.name(*item),
                _ => None,
            });
            let of = feature.map_or("", |feature| feature.name.as_str());
            (name.unwrap_or_default(), String::new(), of)
        }
        _ => {
            let count =
                |kind: fn(&Selected) -> bool| items.iter().filter(|item| kind(item)).count();
            let faces = count(|item| matches!(item, Selected::Face { .. }));
            let edges = count(|item| matches!(item, Selected::Edge { .. }));
            let vertices = count(|item| matches!(item, Selected::Vertex { .. }));
            let bodies = count(|item| matches!(item, Selected::Body(_)));
            let sketch_items = count(|item| matches!(item, Selected::SketchItem { .. }));
            let parts: Vec<String> = [
                (faces, "face", "faces"),
                (edges, "edge", "edges"),
                (vertices, "vertex", "vertices"),
                (bodies, "body", "bodies"),
                (sketch_items, "sketch item", "sketch items"),
            ]
            .into_iter()
            .filter(|&(n, _, _)| n > 0)
            .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
            .collect();
            (format!("{} selected", items.len()), parts.join(" · "), "")
        }
    };
    let info = (!info.is_empty()).then(|| text(info).size(12).wrapping(Wrapping::None));
    let measured = (state.selection_measured)
        .map(|inspected| crate::measure::brief(inspected, document.units()))
        .filter(|values| !values.is_empty())
        .map(|values| {
            let values: Vec<String> = (values.into_iter())
                .map(|value| format!("{} {}", value.label, value.shown))
                .collect();
            text(values.join(" · ")).size(12).wrapping(Wrapping::None)
        });
    let note = (!note.is_empty()).then(|| {
        text(note)
            .size(12)
            .wrapping(Wrapping::None)
            .style(theme::muted_text)
    });
    Some(
        row![
            icons::icon(Icon::Body, icons::INLINE),
            text(title)
                .size(12)
                .wrapping(Wrapping::None)
                .font(theme::SEMIBOLD),
            info,
            measured,
            note,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

/// What a face of `summary` is on, in a word or two.
fn surface_name(summary: &varde_regen::Summary) -> &'static str {
    use varde_regen::Summary;
    match summary {
        Summary::Plane { .. } => "Plane",
        Summary::Cylinder { .. } => "Cylinder",
        Summary::Cone { .. } => "Cone",
        Summary::Sphere { .. } => "Sphere",
        Summary::Torus { .. } => "Torus",
        Summary::ConicCylinder { .. } | Summary::Revolved { .. } | Summary::Other => "Curved",
    }
}

/// The status bar's info on the selected `feature` of `document`, after
/// its name: a sketch's curves and plane, "4 lines · 1 circle · 5 points
/// · on XY", an extrude's extent in the document's units and operation,
/// "Distance 10 mm · New body", or a revolve's turn, operation and axis,
/// "One side 90° · Join · about Line 3" (the axis left out while its
/// sketch doesn't have it; last, as the box clips what doesn't fit and
/// a line's name says least).
fn feature_info(feature: &Feature, document: &Document) -> String {
    let units = document.units();
    match &feature.kind {
        FeatureKind::Sketch { plane, sketch, .. } => {
            let on = crate::plane_pick::on_plane(document, plane);
            format!("{} · {on}", sketch_summary(sketch))
        }
        FeatureKind::Extrude(extrude) => {
            let note = panels::extent_note(&extrude.extent, units);
            let extent = match &extrude.extent {
                Extent::OneSide(d) => format!("Distance {}", panels::length_note(d, units)),
                Extent::Symmetric(d) => format!("Symmetric {}", panels::length_note(d, units)),
                Extent::TwoSides(..) => format!("Two sides {note}"),
                Extent::ThroughAll => note,
            };
            let operation = OperationKind::of(&extrude.operation).label();
            match extrude.tapered() {
                Some(taper) => {
                    let taper = panels::angle_note(taper.value);
                    format!("{extent} · Taper {taper} · {operation}")
                }
                None => format!("{extent} · {operation}"),
            }
        }
        FeatureKind::Revolve(revolve) => {
            let operation = OperationKind::of(&revolve.operation).label();
            let axis = match (
                revolve.axis,
                document.feature(revolve.sketch).map(|f| &f.kind),
            ) {
                (AxisLine::Edge(edge), _) => Some(crate::revolve::edge_axis_name(document, &edge)),
                (axis, Some(FeatureKind::Sketch { sketch, .. })) => {
                    crate::revolve::axis_id(axis).and_then(|id| sketch.name(id))
                }
                _ => None,
            };
            let turn = panels::turn_info(&revolve.extent);
            match axis {
                Some(axis) => format!("{turn} · {operation} · about {axis}"),
                None => format!("{turn} · {operation}"),
            }
        }
        FeatureKind::Combine(combine) => {
            let name = |body| {
                document
                    .body(body)
                    .map_or("a body", |body| body.name.as_str())
            };
            let tools: Vec<&str> = combine.tools.iter().map(|&tool| name(tool)).collect();
            let kept = if combine.keep_tools {
                " · tools kept"
            } else {
                ""
            };
            format!(
                "{} with {} · {}{kept}",
                name(combine.target),
                tools.join(", "),
                combine.op.label()
            )
        }
        FeatureKind::Move(moved) => crate::motion::move_info(document, moved),
        FeatureKind::Mirror(mirror) => crate::motion::mirror_info(document, mirror),
        FeatureKind::Pattern(pattern) => crate::motion::pattern_info(document, pattern),
        FeatureKind::Align(align) => crate::motion::align_info(document, align),
        FeatureKind::Scale(scale) => crate::motion::scale_info(document, scale, units),
        FeatureKind::Split(split) => crate::motion::split_info(document, split),
        FeatureKind::Chamfer(chamfer) => crate::chamfer::chamfer_info(chamfer, units),
        FeatureKind::Shell(shell) => crate::shell::shell_info(shell, units),
        FeatureKind::Fillet(fillet) => crate::fillet::fillet_info(fillet, units),
        FeatureKind::OffsetFace(offset) => crate::offset_face::offset_info(offset, units),
        FeatureKind::FaceDraft(draft) => crate::face_draft::draft_info(document, draft),
        FeatureKind::Sweep(sweep) => crate::sweep::sweep_info(document, sweep),
        FeatureKind::Loft(loft) => crate::loft::loft_info(loft),
    }
}

/// Where `sketch` stands, for the status bar, once that's known, and
/// whether that's trouble: "Fully constrained", "4 degrees of freedom
/// left", or that it doesn't solve or holds constraints in conflict.
fn standing(sketch: &SketchState<'_>) -> Option<(String, bool)> {
    // Nothing drawn is nothing to constrain.
    if sketch.sketch.points.is_empty() {
        return None;
    }
    let Some(analysis) = sketch.analysis else {
        return sketch.unsolved.then(|| ("Doesn't solve".to_owned(), true));
    };
    Some(if !analysis.solved {
        ("Doesn't solve".to_owned(), true)
    } else if !analysis.redundant.is_empty() {
        ("Over-constrained".to_owned(), true)
    } else if analysis.freedom == 0 {
        ("Fully constrained".to_owned(), false)
    } else {
        let freedom = counted(analysis.freedom, "degree", "degrees");
        (format!("{freedom} of freedom left"), false)
    })
}

/// Why an edit of `sketch` was refused, as the status bar says it. A
/// driving dimension refused is told how to have it as a reference: a
/// new one placed with the reference modifier held, one made driving
/// stays one.
fn refusal_text(why: &Rejected, sketch: &Sketch) -> Cow<'static, str> {
    match why {
        Rejected::Edit(why) => format!("Couldn't edit: {why}").into(),
        Rejected::Redundant { .. } => "Would over-constrain the sketch".into(),
        Rejected::Driving { dimensions, .. }
            if dimensions.iter().all(|&id| sketch.dimension(id).is_some()) =>
        {
            "Would over-constrain the sketch, so it stays a reference".into()
        }
        Rejected::Driving { .. } => format!(
            "Would over-constrain the sketch: place it with {} held to add it as a reference",
            Held::REFERENCE.label()
        )
        .into(),
        Rejected::Unsolved(Failure::NotConverged { .. }) => "Can't be solved with the rest".into(),
        Rejected::Unsolved(Failure::Degenerate { .. }) => "Would collapse the sketch".into(),
        Rejected::Unsolved(Failure::OutOfTime) => "Took too long to solve".into(),
    }
}

/// What a sketch holds, for the status bar: "Empty", or how many of each
/// kind of curve, fillets and chamfers apart, and the points, "2 lines ·
/// 1 circle · 5 points".
pub(crate) fn sketch_summary(sketch: &Sketch) -> String {
    // Fillets are arcs and chamfers lines, with corners.
    let count = |kind: Kind, corner: bool| {
        sketch
            .curves
            .iter()
            .filter(|entry| entry.curve.kind() == kind && entry.corner.is_some() == corner)
            .count()
    };
    let counts = [
        (count(Kind::Line, false), "line", "lines"),
        (count(Kind::Circle, false), "circle", "circles"),
        (count(Kind::Arc, false), "arc", "arcs"),
        (count(Kind::Arc, true), "fillet", "fillets"),
        (count(Kind::Line, true), "chamfer", "chamfers"),
        (count(Kind::Spline, false), "spline", "splines"),
        (sketch.points.len(), "point", "points"),
    ];
    let parts: Vec<_> = counts
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, one, many)| counted(n, one, many))
        .collect();
    if parts.is_empty() {
        "Empty".to_owned()
    } else {
        parts.join(" · ")
    }
}

/// How many profiles `sketch` has, for the status bar, once the app has
/// looked: "2 profiles", or that it's too complex to say. Nothing for a
/// sketch without curves.
fn profile_count(sketch: &SketchState<'_>) -> Option<String> {
    if sketch.sketch.curves.is_empty() {
        return None;
    }
    Some(match sketch.profiles? {
        Ok(profiles) => counted(profiles.regions.len(), "profile", "profiles"),
        Err(TooComplex) => "Too complex for profiles".to_owned(),
    })
}

/// `n` and what it counts, `one` or `many` of them: "1 line", "2 lines".
pub(crate) fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What the status bar tells of the model and the file, whatever else
/// it says: regenerating, why the last edit failed, saving, exporting.
fn status_notes(state: &DocumentState<'_>) -> Vec<String> {
    let regenerating = match state.mesh_status {
        // Shown over the viewport once it's slow, see `regenerating`.
        MeshStatus::Current | MeshStatus::Regenerating => None,
        MeshStatus::Failed(error) => Some(format!("Couldn't regenerate: {error}")),
    };
    let edit_error = state
        .edit_error
        .map(|error| format!("Couldn't edit: {error}"));
    let notice = state.notice.map(str::to_owned);
    let saving = state.saving.then(|| "Saving…".to_owned());
    let exporting = state.exporting.then(|| "Exporting…".to_owned());
    [regenerating, edit_error, notice, saving, exporting]
        .into_iter()
        .flatten()
        .collect()
}

/// [`status_notes`] after what the status bar says of the sketch or the
/// extrude.
fn status_suffix(state: &DocumentState<'_>) -> String {
    status_notes(state)
        .iter()
        .map(|note| format!(" · {note}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use glam::DVec2;
    use varde_sketch::Curve;

    use super::*;

    #[test]
    fn parts_are_as_opaque_as_their_bodies() {
        let mut editor = Editor::new(Document::example());
        let body = editor.document().bodies()[0].id;
        let opacity = Opacity::new(30).unwrap();
        editor
            .apply(varde_document::Command::SetOpacity(body, opacity))
            .unwrap();
        // A draft's new body isn't in the document: opaque.
        let parts = [body, BodyId::NEW, body];
        let document = editor.document();
        assert_eq!(*part_opacity(document, &parts, None), [0.3, 1.0, 0.3]);
        // The slider's preview stands in for its body's, and only its.
        let preview = Some((body, Opacity::new(55).unwrap()));
        assert_eq!(*part_opacity(document, &parts, preview), [0.55, 1.0, 0.55]);
    }

    /// Parts are drawn in their bodies' colours, the slider's preview
    /// standing in for its body's, and a part of no body in the theme's.
    #[test]
    fn parts_are_in_their_bodies_colours() {
        let mut editor = Editor::new(Document::example());
        let body = editor.document().bodies()[0].id;
        let teal = Tint::new(180, 30).unwrap();
        editor
            .apply(varde_document::Command::SetColor(body, Some(teal)))
            .unwrap();
        let parts = [body, BodyId::NEW];
        let document = editor.document();
        let tinted = Some(body_tint(teal));
        assert_eq!(*part_tints(document, &parts, None), [tinted, None]);
        let red = Tint::new(0, 30).unwrap();
        let preview = Some((body, red));
        let shown = Some(body_tint(red));
        assert_eq!(*part_tints(document, &parts, preview), [shown, None]);
        assert_eq!(body_tint(red).saturation, 0.3);
    }

    /// The status bar's mouse hints say the left button orbits outside a
    /// sketch and in one while Project or Intersect picks outside it.
    #[test]
    fn the_left_button_orbits_outside_a_sketch_and_picking_outside_it() {
        let sketch = Sketch::default();
        let none = BTreeSet::new();
        let with = |tool: Option<Tool>| {
            let tool = tool.map(|tool| crate::testing::tool(tool, &[], &[]));
            left_orbits(Some(&SketchState::plain(&sketch, &none, tool)))
        };
        assert!(left_orbits(None));
        assert!(!with(None));
        assert!(!with(Some(Tool::Line)));
        assert!(!with(Some(Tool::Trim)));
        assert!(with(Some(Tool::Project)));
        assert!(with(Some(Tool::Intersect)));
    }

    #[test]
    fn a_sketch_stands_as_its_analysis_says() {
        let mut sketch = Sketch::default();
        let none = BTreeSet::new();
        let mut analysis = Analysis {
            freedom: 0,
            ..Analysis::default()
        };
        let standing_of = |sketch: &Sketch, analysis: Option<&Analysis>, unsolved| {
            standing(&SketchState {
                analysis,
                unsolved,
                ..SketchState::plain(sketch, &none, None)
            })
        };
        assert_eq!(standing_of(&sketch, Some(&analysis), false), None);
        let point = sketch.add_point(DVec2::ZERO).unwrap();
        assert_eq!(standing_of(&sketch, None, false), None);
        assert_eq!(
            standing_of(&sketch, None, true),
            Some(("Doesn't solve".to_owned(), true))
        );
        analysis.solved = true;
        assert_eq!(
            standing_of(&sketch, Some(&analysis), false),
            Some(("Fully constrained".to_owned(), false))
        );
        analysis.freedom = 1;
        assert_eq!(
            standing_of(&sketch, Some(&analysis), false),
            Some(("1 degree of freedom left".to_owned(), false))
        );
        analysis.redundant.insert(point);
        assert_eq!(
            standing_of(&sketch, Some(&analysis), false),
            Some(("Over-constrained".to_owned(), true))
        );
        analysis.solved = false;
        assert_eq!(
            standing_of(&sketch, Some(&analysis), true),
            Some(("Doesn't solve".to_owned(), true))
        );
        let why = Rejected::Redundant {
            involved: BTreeSet::new(),
        };
        assert_eq!(
            refusal_text(&why, &sketch),
            "Would over-constrain the sketch"
        );
        // A driving dimension refused says how to have it as a reference:
        // a new one placed so, one made driving staying one.
        let refused = |dimension| Rejected::Driving {
            dimensions: BTreeSet::from([dimension]),
            involved: BTreeSet::from([dimension]),
        };
        let new = refusal_text(&refused(point), &sketch);
        let held = Held::REFERENCE.label();
        assert!(new.contains(&format!("place it with {held} held")), "{new}");
        let b = sketch.add_point(DVec2::X).unwrap();
        let line = sketch
            .add_curve(
                Curve::Line {
                    start: point,
                    end: b,
                },
                false,
            )
            .unwrap();
        let measure = Measure::Length(line);
        let placed = crate::testing::dimension(&mut sketch, measure, "1", false, DVec2::ZERO);
        let made_driving = refusal_text(&refused(placed), &sketch);
        assert_eq!(
            made_driving,
            "Would over-constrain the sketch, so it stays a reference"
        );
    }

    #[test]
    fn a_sketch_is_summed_up_by_kind() {
        let mut sketch = Sketch::default();
        assert_eq!(sketch_summary(&sketch), "Empty");
        let a = sketch.add_point(DVec2::ZERO).unwrap();
        assert_eq!(sketch_summary(&sketch), "1 point");
        let b = sketch.add_point(DVec2::X).unwrap();
        let c = sketch.add_point(DVec2::Y).unwrap();
        sketch
            .add_curve(Curve::Line { start: a, end: b }, false)
            .unwrap();
        sketch
            .add_curve(Curve::Line { start: b, end: c }, true)
            .unwrap();
        sketch
            .add_curve(
                Curve::Circle {
                    center: a,
                    radius: 1.0,
                },
                false,
            )
            .unwrap();
        assert_eq!(sketch_summary(&sketch), "2 lines · 1 circle · 3 points");
        // A chamfer is counted apart from the lines.
        let (start, end) = (
            sketch.add_point(DVec2::new(0.5, 0.0)).unwrap(),
            sketch.add_point(DVec2::new(0.5, 0.5)).unwrap(),
        );
        let chamfer = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
        let lines = [sketch.curves[0].id, sketch.curves[1].id];
        sketch.curve_mut(chamfer).unwrap().corner = Some(varde_sketch::Corner {
            a: lines[0],
            b: lines[1],
            at: b,
            equal: false,
        });
        assert_eq!(
            sketch_summary(&sketch),
            "2 lines · 1 circle · 1 chamfer · 5 points"
        );
    }

    #[test]
    fn the_profiles_are_counted_once_found() {
        let mut sketch = Sketch::default();
        let none = BTreeSet::new();
        let count = |sketch: &Sketch, profiles| {
            profile_count(&SketchState {
                profiles,
                ..SketchState::plain(sketch, &none, None)
            })
        };
        let found = Ok(Arc::new(sketch.profiles().unwrap()));
        // Nothing drawn, nothing to count.
        assert_eq!(count(&sketch, Some(&found)), None);
        let center = sketch.add_point(DVec2::ZERO).unwrap();
        for radius in [1.0, 2.0] {
            let circle = Curve::Circle { center, radius };
            sketch.add_curve(circle, false).unwrap();
        }
        // Not until the app has looked.
        assert_eq!(count(&sketch, None), None);
        // The disc and the ring round it.
        let found = Ok(Arc::new(sketch.profiles().unwrap()));
        assert_eq!(count(&sketch, Some(&found)), Some("2 profiles".to_owned()));
        let one = Ok(Arc::new(Profiles {
            regions: found.as_ref().unwrap().regions[..1].to_vec(),
            ..Profiles::default()
        }));
        assert_eq!(count(&sketch, Some(&one)), Some("1 profile".to_owned()));
        assert_eq!(
            count(&sketch, Some(&Err(TooComplex))),
            Some("Too complex for profiles".to_owned())
        );
    }

    #[test]
    fn the_status_bar_sums_up_the_feature_selected() {
        use varde_document::{Document, Extent, FeatureKind, Operation};

        let document = Document::example();
        let [sketch, extrude] = [0, 1].map(|k| &document.features()[k]);
        assert_eq!(
            feature_info(sketch, &document),
            "4 lines · 1 circle · 5 points · on XY"
        );
        assert_eq!(
            feature_info(extrude, &document),
            "Distance 10 mm · New body"
        );

        let FeatureKind::Extrude(mut changed) = extrude.kind.clone() else {
            panic!("the example's second feature is its extrude");
        };
        let Extent::OneSide(distance) = changed.extent.clone() else {
            panic!("the example's extrude goes one side");
        };
        let mut info = |extent: Extent, operation: Operation| {
            changed.extent = extent;
            changed.operation = operation;
            let feature = Feature {
                kind: FeatureKind::Extrude(changed.clone()),
                ..extrude.clone()
            };
            feature_info(&feature, &document)
        };
        let cut = Operation::Cut(Default::default());
        assert_eq!(
            info(Extent::Symmetric(distance.clone()), cut.clone()),
            "Symmetric 10 mm · Cut"
        );
        assert_eq!(
            info(Extent::TwoSides(distance.clone(), distance), cut.clone()),
            "Two sides 10 mm + 10 mm · Cut"
        );
        assert_eq!(info(Extent::ThroughAll, cut.clone()), "Through all · Cut");
        changed.extent = Extent::ThroughAll;
        changed.operation = cut;
        changed.taper = Some(varde_expr::Value {
            text: "1.5".to_owned(),
            value: 1.5f64.to_radians(),
        });
        let tapered = Feature {
            kind: FeatureKind::Extrude(changed),
            ..extrude.clone()
        };
        assert_eq!(
            feature_info(&tapered, &document),
            "Through all · Taper 1.5° · Cut"
        );
    }

    #[test]
    fn a_revolve_s_row_says_its_total_turn_and_the_status_bar_its_axis() {
        use varde_document::{AxisLine, FeatureKind, Operation, Revolve, Turn};

        let document = Document::example();
        let [sketch, extrude] = [0, 1].map(|k| &document.features()[k]);
        let FeatureKind::Extrude(extrude_kind) = &extrude.kind else {
            panic!("the example's second feature is its extrude");
        };
        let ask = Turn::ask(&document.design());
        let angle = |text| varde_expr::Value::new(text, &ask).unwrap();
        let revolve = |extent: Turn, axis: AxisLine| Feature {
            kind: FeatureKind::Revolve(Revolve {
                sketch: sketch.id,
                regions: extrude_kind.regions.clone(),
                axis,
                extent,
                flip: false,
                operation: Operation::Cut(Default::default()),
            }),
            ..extrude.clone()
        };
        let x = AxisLine::SketchX;
        let cases = [
            (Turn::Full, "360°", "Full 360° · Cut · about X axis"),
            (
                Turn::OneSide(angle("90")),
                "90°",
                "One side 90° · Cut · about X axis",
            ),
            (
                Turn::Symmetric(angle("90")),
                "90°",
                "Symmetric 90° · Cut · about X axis",
            ),
            (
                Turn::TwoSides(angle("90"), angle("45")),
                "135°",
                "Two sides 90° + 45° · Cut · about X axis",
            ),
        ];
        for (extent, note, info) in cases {
            assert_eq!(panels::turn_note(&extent), note);
            assert_eq!(feature_info(&revolve(extent, x), &document), info);
        }
        // The Y axis, and a line of the sketch, by their names.
        let y = revolve(Turn::Full, AxisLine::SketchY);
        assert_eq!(
            feature_info(&y, &document),
            "Full 360° · Cut · about Y axis"
        );
        let FeatureKind::Sketch { sketch: drawn, .. } = &sketch.kind else {
            panic!("the example's first feature is its sketch");
        };
        let line = drawn.curves.first().unwrap();
        let on_line = revolve(Turn::Full, AxisLine::Curve(line.id));
        assert_eq!(
            feature_info(&on_line, &document),
            format!("Full 360° · Cut · about {}", line.name())
        );
        assert_eq!(panels::feature_icon(&y), Icon::Revolve);
    }

    #[test]
    fn the_delete_prompt_counts_the_features_and_the_bodies() {
        fn prompt<'a>(
            name: &'a str,
            body: bool,
            features: Vec<&'a Feature>,
            bodies: Vec<&'a Body>,
        ) -> DeletePrompt<'a> {
            DeletePrompt {
                name,
                body,
                features,
                bodies,
                worked: Vec::new(),
                worked_on: Vec::new(),
            }
        }
        let document = varde_document::Document::example();
        let [sketch, extrude] = [0, 1].map(|k| &document.features()[k]);
        let body = &document.bodies()[0];
        assert_eq!(
            delete_question(&prompt(
                "Sketch 1",
                false,
                vec![sketch, extrude],
                vec![body]
            )),
            "Delete Sketch 1 with the 1 feature and 1 body that depend on it?"
        );
        assert_eq!(
            delete_question(&prompt("Extrude 1", false, vec![extrude], vec![body])),
            "Delete Extrude 1 with the 1 body that depends on it?"
        );
        assert_eq!(
            delete_question(&prompt(
                "Sketch 1",
                false,
                vec![sketch, extrude, extrude],
                vec![]
            )),
            "Delete Sketch 1 with the 2 features that depend on it?"
        );
        assert_eq!(
            delete_question(&prompt("Sketch 1", false, vec![sketch], vec![])),
            "Delete Sketch 1?"
        );
        // A body goes with the feature making it, named if it's the only
        // one, else counted, and the other bodies that one makes.
        assert_eq!(
            delete_question(&prompt("Body 1", true, vec![extrude], vec![body])),
            "Delete Body 1 and Extrude 1, which makes it?"
        );
        assert_eq!(
            delete_question(&prompt("Body 1", true, vec![extrude, extrude], vec![body])),
            "Delete Body 1 with the 2 features that go with it?"
        );
        assert_eq!(
            delete_question(&prompt(
                "Body 1",
                true,
                vec![extrude, extrude],
                vec![body, body]
            )),
            "Delete Body 1 with the 2 features and 1 body that go with it?"
        );
    }

    #[test]
    fn the_delete_prompt_warns_of_the_joins_and_cuts_that_stay() {
        let document = varde_document::Document::example();
        let extrude = document.features()[1].clone();
        let body = document.bodies()[0].clone();
        let named = |name: &str| varde_document::Feature {
            name: name.to_owned(),
            ..extrude.clone()
        };
        let [cut, join, other] = ["Extrude 2", "Extrude 3", "Extrude 4"].map(named);
        let second = varde_document::Body {
            name: "Body 2".to_owned(),
            ..body.clone()
        };
        let prompt = |worked, worked_on| DeletePrompt {
            name: "Body 1",
            body: true,
            features: vec![&extrude],
            bodies: vec![&body],
            worked,
            worked_on,
        };
        assert_eq!(delete_warning(&prompt(vec![], vec![])), None);
        assert_eq!(
            delete_warning(&prompt(vec![&cut], vec![&body])).as_deref(),
            Some("Extrude 2 works on Body 1 and stays, so it may fail with nothing to work on.")
        );
        assert_eq!(
            delete_warning(&prompt(vec![&cut, &join], vec![&body, &second])).as_deref(),
            Some(
                "Extrude 2 and Extrude 3 work on Body 1 and Body 2 and stay, so they may fail \
                 with nothing to work on."
            )
        );
        assert_eq!(
            delete_warning(&prompt(vec![&cut, &join, &other], vec![&body])).as_deref(),
            Some(
                "Extrude 2, Extrude 3 and Extrude 4 work on Body 1 and stay, so they may fail \
                 with nothing to work on."
            )
        );
    }

    #[test]
    fn an_error_reads_as_a_sentence() {
        assert_eq!(chrome::sentence("unknown unit 'yd'"), "Unknown unit 'yd'");
        assert_eq!(chrome::sentence("Already"), "Already");
        assert_eq!(chrome::sentence(""), "");
        assert_eq!(chrome::sentence("über"), "Über");
        // Title case, not upper case, where they differ.
        assert_eq!(chrome::sentence("ßtraße"), "Sstraße");
        assert_eq!(chrome::sentence("ﬁnd"), "Find");
        assert_eq!(chrome::sentence(" leading"), " leading");
        assert_eq!(chrome::sentence("1 mm"), "1 mm");
    }
}
