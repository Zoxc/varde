//! The document screen: its state and layout. The banners and prompts
//! over it are in `prompts.rs`, what the status bar says in
//! `status_text.rs`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::widget::{column, container, row, stack, text};
use iced::{Element, Length};
use varde_document::{
    Body, BodyId, Document, EditError, Editor, Feature, FeatureId, Opacity, Placement, Plane, Tint,
};
use varde_expr::LengthUnit;
use varde_kernel_compat::{RenderLines, RenderMesh};
use varde_render::{BodyTint, Camera};
use varde_sketch::{
    Analysis, Id, LinkKind, Measure, Profiles, Rejected, Selectable, Side, Sketch, SplineKind,
    TooComplex,
};

use crate::chrome::small_button;
use crate::file_menu;
use crate::shortcut::{DocumentKeys, KeysInput, OperationKeys};
use crate::status::{self};
use crate::theme::Emphasis;
use crate::typed::Field;
use crate::{
    Downloads, Edit, ExtrudeState, File, Location, Message, Panel, PlanePick, RevolveState,
    RowMenu, SNAP_TOLERANCE, SavePlace, Snap, Target, Tool, panels, theme, toolbar, viewport,
};

mod prompts;
mod status_text;

use prompts::{
    banner, damage_detail, delete_prompt, name_prompt, recovered_detail, refused_banner,
    unsaved_prompt,
};
pub use status_text::CURVED_FACE;
pub(crate) use status_text::sketch_summary;
use status_text::status;

/// The operation being set up, or the measure tool in use: one at a
/// time.
#[expect(
    clippy::large_enum_variant,
    reason = "built once per view, where all five states were held before"
)]
pub enum OperationState<'a> {
    Extrude(ExtrudeState<'a>),
    Revolve(RevolveState<'a>),
    Combine(crate::CombineState<'a>),
    /// A move, mirror or pattern, or another motion kind.
    Motion(crate::MotionState<'a>),
    Measure(crate::MeasureState<'a>),
}

/// The design's file as the document screen shows it: its name and
/// where it's kept, its saving, recovery and damage, and exporting.
pub struct FileShown<'a> {
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
}

/// Borrowed state needed to build the document screen.
pub struct DocumentState<'a> {
    pub editor: &'a Editor,
    pub camera: &'a Camera,
    /// The point the camera orbits, marked as it's picked and while the
    /// cursor is over the view cube, if one was picked and it shows.
    pub pivot: Option<varde_render::Pivot>,
    /// How faded the model is, from 0 to 1: fully in a sketch, and
    /// between as it fades going into one or out of it.
    pub fade: f32,
    /// The plane of the sketch the grid fades onto or off with it.
    pub fading_grid: Option<varde_render::GridPlane>,
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
    /// The design's file and its saving and exporting, as the banners,
    /// the file cell and the file menu show it.
    pub file: FileShown<'a>,
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
    /// is asked for (see [`PreviewRequest`](crate::PreviewRequest)).
    pub thumbnail: Option<&'a Arc<crate::PreviewRequest>>,
    /// The MCP server's renders for the viewport to draw on its next
    /// frame, beside the thumbnail.
    pub renders: &'a [Arc<crate::PreviewRequest>],
    /// The viewport's width over its height, as it last told the app, if
    /// it has ([`Look::ViewAspect`](crate::Look::ViewAspect)).
    pub aspect: Option<f32>,
    /// The feature selected in the Timeline, if any.
    pub selected_feature: Option<FeatureId>,
    /// Where the Timeline's marker shows.
    pub rollback: Rollback,
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
    /// The operation being set up or the measure tool in use, if one
    /// is: never with a sketch.
    pub operation: Option<OperationState<'a>>,
    /// Whether the document has two bodies or more: the Combine tool works
    /// outside sketches then.
    pub combinable: bool,
    /// The parameters' popup, while it's open.
    pub params: Option<crate::ParamsState<'a>>,
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
    /// The settings page, while it shows in the viewport's place: the
    /// status bar, the rail and anything else floating over the viewport
    /// go with it.
    pub settings: Option<crate::SettingsState<'a>>,
    /// The MCP server, for the toolbar's toggle, the status bar's chip
    /// and the panel on the right: natively only, `None` on the web.
    pub mcp: Option<crate::McpShown<'a>>,
}

impl<'a> DocumentState<'a> {
    /// The extrude being set up, if it's the operation.
    pub(crate) fn extrude(&self) -> Option<&ExtrudeState<'a>> {
        match &self.operation {
            Some(OperationState::Extrude(extrude)) => Some(extrude),
            _ => None,
        }
    }

    /// The revolve being set up, if it's the operation.
    pub(crate) fn revolve(&self) -> Option<&RevolveState<'a>> {
        match &self.operation {
            Some(OperationState::Revolve(revolve)) => Some(revolve),
            _ => None,
        }
    }

    /// The combine being set up, if it's the operation.
    pub(crate) fn combine(&self) -> Option<&crate::CombineState<'a>> {
        match &self.operation {
            Some(OperationState::Combine(combine)) => Some(combine),
            _ => None,
        }
    }

    /// The move, mirror or pattern being set up, if it's the operation.
    pub(crate) fn motion(&self) -> Option<&crate::MotionState<'a>> {
        match &self.operation {
            Some(OperationState::Motion(motion)) => Some(motion),
            _ => None,
        }
    }

    /// The measure tool, if it's in use.
    pub(crate) fn measure(&self) -> Option<&crate::MeasureState<'a>> {
        match &self.operation {
            Some(OperationState::Measure(measure)) => Some(measure),
            _ => None,
        }
    }

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

    /// Whether the toolbar offers the origin planes now: picking a sketch's
    /// plane, a split's tool by face, or a reference that may be a plane. The
    /// viewport shows them then, whether Objects has them shown or not.
    pub(crate) fn picks_origin_planes(&self) -> bool {
        let Some(motion) = self.motion() else {
            return self.picking_plane.is_some();
        };
        let split_face = motion
            .split()
            .is_some_and(|split| split.mode == crate::SplitMode::Face);
        (motion.kind).picks_origin_planes(motion.picking, split_face)
    }

    /// What the viewport draws of the world's origin, axes and planes:
    /// what Objects has shown, and the planes while a tool offers them
    /// (`DocumentState::picks_origin_planes`), drawn over the model then.
    pub fn origin_drawn(&self) -> varde_render::OriginShown {
        let mut origin = self.origin;
        if self.picks_origin_planes() {
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
        DocumentKeys::new(KeysInput {
            editable: self.editable(),
            selected: self.selected_feature,
            sketch: self.sketch,
            face_selected: self.face_selected(),
            objects_deletable: (self.objects_selected.iter())
                .any(|row| matches!(row, crate::ObjectRow::Sketch(_))),
            picking_plane: self.picking_plane.is_some(),
            operation: OperationKeys::of(
                self.extrude(),
                self.revolve(),
                self.combine(),
                self.motion(),
            ),
            bodies: !self.editor.document().bodies().is_empty(),
            combinable: self.combinable,
            measuring: self.measure().is_some(),
            rail: self.rail,
            // The screen shows no chip for `F2` or the context menu key.
            rename: None,
            menu: None,
            edited: self.file.edited,
            settings: self.settings.is_some(),
            undo: self.editor.can_undo() || self.proposing,
            // Not while edits wait on the solver, which come after.
            redo: self.editor.can_redo() && !self.proposing,
        })
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
    /// Why the solver refused the last edit, shown until the next action,
    /// with the constraints involved in red.
    pub refusal: Option<Refusal<'a>>,
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
    /// Whether clicks on splines add points to them (the toolbar's Add
    /// point), with no tool in use.
    pub inserting: bool,
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
    /// The design's parameters, whose names are offered as they're typed
    /// (see the `suggest` module).
    pub params: crate::ParamsIn<'a>,
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
            inserting: false,
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
        let refused = match self.refusal {
            Some(Refusal::Rejected(why)) => Some(why.involved()),
            _ => None,
        };
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
    /// How many lines the Line tool's chain has drawn, 0 without one: a
    /// chain of two or more can close.
    pub chained: usize,
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

    /// The kind of spline the Spline tool draws.
    pub fn spline_kind(&self) -> SplineKind {
        if self.control {
            SplineKind::Control
        } else {
            SplineKind::Through
        }
    }

    /// Whether the Spline tool has placed enough points to end its
    /// spline open: two through fit points, four control points.
    pub fn spline_ends(&self) -> bool {
        self.tool == Tool::Spline && self.placed.len() >= self.spline_kind().least(false)
    }

    /// Whether the tool can close what it's drawing without a click on
    /// its first point (the Close key or button): the Spline tool once it
    /// has enough points to close, the Line tool once its chain has two
    /// lines.
    pub fn can_close(&self) -> bool {
        match self.tool {
            Tool::Spline => self.placed.len() >= self.spline_kind().least(true),
            Tool::Line => self.chained >= 2,
            _ => false,
        }
    }

    /// Whether the Spline tool's click at `at`, a pixel being `pixel`
    /// sketch units there, closes its spline: on its first point (within
    /// [`SNAP_TOLERANCE`] pixels of it), once there are enough to close
    /// it, three.
    pub fn closes(&self, at: DVec2, pixel: f64) -> bool {
        self.placed.len() >= self.spline_kind().least(true) && self.on_first(at, pixel)
    }

    /// Whether the Spline tool's click at `at`, a pixel being `pixel`
    /// sketch units there, is on its first point (within
    /// [`SNAP_TOLERANCE`] pixels of it): closing its spline if it has
    /// enough points, else placing nothing, as a point there would close
    /// it on itself.
    pub fn on_first(&self, at: DVec2, pixel: f64) -> bool {
        self.tool == Tool::Spline
            && self
                .placed
                .first()
                .is_some_and(|&first| first.distance(at) <= SNAP_TOLERANCE * pixel)
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
    /// Whether the features depending on it can stay instead, naming what
    /// goes, which then fails them until they're given another: Delete
    /// only, beside Delete all.
    pub keeping: bool,
}

/// A sketch edit the solver refused after its sketch was left, which
/// nothing else would show: the edit is gone.
#[derive(Debug, Clone, Copy)]
pub struct RefusedEdit<'a> {
    /// The sketch feature's name.
    pub name: &'a str,
    /// The sketch as committed, without the edit.
    pub sketch: &'a Sketch,
    /// Why the solver refused it.
    pub refusal: Refusal<'a>,
}

/// Why the solver refused a sketch edit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refusal<'a> {
    /// The edit doesn't hold together, see [`Rejected`].
    Rejected(&'a Rejected),
    /// The solver couldn't answer: it panicked, or its worker stopped.
    Failed(&'a str),
}

/// Where the model is rolled back to, as the Timeline's marker shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rollback {
    /// The feature the model is rolled back to before, if it is.
    pub before: Option<FeatureId>,
    /// Whether it's only there while a feature is edited, when the
    /// marker can't be dragged.
    pub editing: bool,
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

impl NamePrompt<'_> {
    /// Whether the design is kept in browser storage: saved there, or
    /// renamed, which is only done there.
    fn in_browser(&self) -> bool {
        self.place == SavePlace::Browser || self.rename
    }
}

/// The document screen: toolbar on top, side panel on the left and the 3D
/// viewport filling the rest.
pub fn document<'a>(state: DocumentState<'a>) -> Element<'a, Message> {
    let editable = state.editable();

    let read_only = state
        .read_only
        .map(|reason| banner(text("Read-only").font(theme::SEMIBOLD), reason, None));
    let save_error = state.file.save_error.as_deref().map(|error| {
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

    let export_error = state.file.export_error.map(|error| {
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

    let recovered = state.file.recovered.as_ref().map(|recovered| {
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

    let damage = state.file.damage.as_ref().map(|damaged| {
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

    let panel = state.operation.as_ref().map(|operation| match operation {
        OperationState::Extrude(extrude) => crate::extrude::panel(extrude),
        OperationState::Revolve(revolve) => crate::revolve::panel(revolve),
        OperationState::Combine(combine) => crate::combine::panel(combine),
        OperationState::Motion(motion) => crate::motion::panel(motion),
        OperationState::Measure(measure) => crate::measure::panel(measure),
    });
    // The parameters' popup, left of the operation's panel if one shows.
    let params = (state.params).map(|params| crate::params::popup(params, panel.is_some()));
    let ask = state.mcp.as_ref().and_then(crate::mcp::ask_popup);
    let content = column![
        toolbar::toolbar(&state),
        banners,
        row![
            panels::side_panel(&state),
            column![
                refused,
                match state.settings {
                    // In the viewport's place, which keeps its camera
                    // and whatever is open, shown again as it closes;
                    // the pictures it would draw offscreen drawn beside.
                    Some(settings) => stack![
                        crate::settings::page(settings),
                        crate::viewport::offscreen(
                            state.thumbnail,
                            state.renders,
                            state.mode.palette()
                        ),
                    ]
                    .into(),
                    None => viewport_stack(&state, editable, panel, params),
                },
            ],
            // The MCP panel, on the right: the viewport shrinks.
            state.mcp.as_ref().and_then(crate::mcp::panel),
        ]
        .height(Length::Fill),
    ];
    // The prompt about unsaved changes shows over the delete prompt,
    // which shows over the menus.
    let screen = match (state.overlay, &state.deleting) {
        (Some(Overlay::UnsavedPrompt), _) => {
            Element::from(stack![content, unsaved_prompt(state.file.name)])
        }
        (Some(Overlay::NamePrompt), _) => match &state.file.naming {
            Some(prompt) => Element::from(stack![content, name_prompt(prompt.clone())]),
            None => content.into(),
        },
        (_, Some(deleting)) => Element::from(stack![content, delete_prompt(deleting)]),
        (Some(Overlay::FileMenu), None) => {
            let document = state.editor.document();
            let menu = file_menu::file_menu(file_menu::FileMenu {
                editable,
                edited: state.file.edited,
                exportable: state.file.exportable,
                units: document.units(),
                tolerance: document.tolerance(),
                downloads: state.file.downloads.clone(),
                downloadable: state.file.downloadable,
                rename: state.file.rename,
            });
            Element::from(stack![content, menu])
        }
        (Some(Overlay::ViewMenu(submenu)), None) => {
            let menu = status::view_menu(state.camera.projection(), state.options, submenu);
            Element::from(stack![content, menu])
        }
        (None, None) => content.into(),
    };
    // An MCP client's edit asking the user to finish theirs first shows
    // over all of it.
    match ask {
        Some(popup) => Element::from(stack![screen, popup]),
        None => screen,
    }
}

/// The viewport, with what floats over it: the names, the status bar
/// and the regenerating card.
fn viewport_stack<'a>(
    state: &DocumentState<'a>,
    editable: bool,
    panel: Option<Element<'a, Message>>,
    params: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    // The status bar floats over the viewport's bottom right.
    Element::from(stack![
        viewport::viewport(
            viewport::SceneInput {
                mesh: state.mesh,
                opacity: state.part_opacity(),
                tints: state.part_tints(),
                sketches: state.sketches,
                camera: state.camera,
                pivot: state.pivot,
                fade: (state.fade, state.fading_grid),
                highlight: state.highlight,
                hover_through: state.hover_through,
                errors: state.errors,
                origin: state.origin_drawn(),
                options: state.options,
                previews: state
                    .thumbnail
                    .into_iter()
                    .chain(state.renders)
                    .cloned()
                    .collect(),
            },
            state.picking.clone(),
            state.mode.palette(),
            state.sketch.map(|sketch| viewport::Sketching::new(
                sketch,
                editable,
                state.options.mouse.sketch_snap
            )),
            operating(state),
            viewport::Overlays {
                panel,
                params,
                rail: crate::rail::rail(state),
                overlaps: state.overlaps.map(|overlaps| crate::overlaps::view(
                    overlaps,
                    (state.sketch.as_ref()).map(|sketch| (sketch.sketch, sketch.selection)),
                    state.model_selection,
                    state.overlap_ticks.as_deref(),
                    state.editor.document(),
                )),
            },
            Some(state.aspect.unwrap_or(0.0)),
        ),
        (state.options.names)
            .then_some(state.picking.as_ref())
            .flatten()
            .map(|picking| crate::names::overlay(
                picking,
                state.model_selection,
                *state.camera,
                state.editor.document(),
                state.editor.generation(),
            )),
        status::status_bar(status(state)),
        state.regenerating.map(crate::regenerating::regenerating),
    ])
}

/// The operation being set up, as the viewport shows it, if one is.
fn operating<'a>(state: &DocumentState<'a>) -> Option<viewport::Operating<'a>> {
    Some(match state.operation.as_ref()? {
        OperationState::Extrude(extrude) => {
            viewport::Operating::Extrude(Box::new(viewport::Extruding::new(extrude.clone())))
        }
        OperationState::Revolve(revolve) => {
            viewport::Operating::Revolve(viewport::Revolving::new(revolve.clone()))
        }
        OperationState::Motion(motion) => {
            viewport::Operating::Motion(viewport::Moving::new(motion.clone()))
        }
        OperationState::Measure(measure) => {
            viewport::Operating::Measure(viewport::Measuring::new(measure.clone()))
        }
        OperationState::Combine(_) => return None,
    })
}

/// How opaque each part of the mesh is drawn, the parts being of `parts`'
/// bodies in order: as its body in `document` is shown, with `preview`'s
/// opacity if it's of that body, or opaque if it's of none there, as a
/// draft's new body is. The viewport and the saved thumbnail draw them so.
pub fn part_opacity(
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
/// `parts`' bodies in order: as its body in `document` is shown, with
/// `preview`'s colour if it's of that body, or the theme's if it's of
/// none there. The viewport and the saved thumbnail draw them so.
pub fn part_tints(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome;

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
