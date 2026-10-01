//! The document screen: its layout, the banners over it, the prompt about
//! unsaved changes and the status bar's info.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, opaque, row, space, stack, text};
use iced::{Alignment, Element, Length};
use varde_document::EXTENSION;
use varde_document::{
    APP_NAME, Body, BodyId, Document, EditError, Editor, Extent, Feature, FeatureId, FeatureKind,
    Plane,
};
use varde_expr::LengthUnit;
use varde_kernel::{RenderLines, RenderMesh};
use varde_render::Camera;
use varde_sketch::{
    Analysis, Failure, Id, Kind, Measure, Profiles, Rejected, Side, Sketch, TooComplex,
};

use crate::chrome::{self, chord_hint, key_hint, mouse_hint, small_button};
use crate::icons::{self, Icon, MouseButton};
use crate::shortcut::{DocumentKeys, Held, Shortcut};
use crate::theme::Emphasis;
use crate::typed::Field;
use crate::{
    ConstraintKind, Edit, ExtrudeState, File, Look, Message, OperationKind, Panel, Snap, Target,
    Tool, Unsaved, panels, theme, toolbar, viewport,
};

/// Borrowed state needed to build the document screen.
pub struct DocumentState<'a> {
    pub editor: &'a Editor,
    pub camera: &'a Camera,
    /// The document's mesh, which the app gets from the regeneration side,
    /// so it may lag behind the document.
    pub mesh: &'a Arc<RenderMesh>,
    /// The finished sketches' curves, which come with `mesh`.
    pub sketches: &'a Arc<RenderLines>,
    /// How `mesh` and `sketches` stand against the document.
    pub mesh_status: MeshStatus<'a>,
    /// The document name, without extension.
    pub name: &'a str,
    /// Whether there are unsaved changes.
    pub edited: bool,
    /// Why the document can't be edited, if it can't. Edit commands are
    /// disabled then; the camera still works.
    pub read_only: Option<&'a str>,
    /// Why the last edit was refused, if it was.
    pub edit_error: Option<&'a EditError>,
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
    /// What's shown over the screen, if anything.
    pub overlay: Option<Overlay>,
    /// The selected side panel tab.
    pub panel: Panel,
    /// Whether the peek key is held, showing the other tab.
    pub peek: bool,
    pub mode: theme::Mode,
    /// Whether the plane for a new sketch is being picked.
    pub picking_plane: bool,
    /// The feature selected in the Timeline, if any.
    pub selected_feature: Option<FeatureId>,
    /// The sketch being edited, if one is.
    pub sketch: Option<SketchState<'a>>,
    /// The extrude being set up, if one is: never with a sketch.
    pub extrude: Option<ExtrudeState<'a>>,
    /// Whether there's a sketch to extrude regions of: the Extrude tool
    /// works outside sketches then.
    pub extrudable: bool,
    /// The sketches that don't solve, as regenerating found.
    pub unsolved: &'a [FeatureId],
    /// The features that failed and why, as regenerating found, in the
    /// document's order.
    pub failed: &'a [(FeatureId, String)],
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
}

impl DocumentState<'_> {
    /// Whether the document can be changed, including by undo, and saved.
    pub(crate) fn editable(&self) -> bool {
        self.read_only.is_none()
    }

    /// What the screen's shortcuts depend on.
    pub(crate) fn keys(&self) -> DocumentKeys {
        DocumentKeys::new(self.editable(), self.selected_feature, self.sketch)
            .with_extrude(self.extrudable, self.extrude.as_ref())
    }
}

/// The sketch being edited, and how it's shown.
#[derive(Debug, Clone, Copy)]
pub struct SketchState<'a> {
    /// The sketch feature's name.
    pub name: &'a str,
    pub plane: Plane,
    /// The sketch as it's shown: as committed, with the edits waiting on
    /// the solver applied, or as it's dragged.
    pub sketch: &'a Sketch,
    /// The items edits waiting on the solver add, not committed yet:
    /// they're drawn faded.
    pub pending: &'a BTreeSet<Id>,
    /// The items selected.
    pub selection: &'a BTreeSet<Id>,
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
    pub hovered: Option<Id>,
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
        selection: &'a BTreeSet<Id>,
        tool: Option<ActiveTool<'a>>,
    ) -> Self {
        static NONE: BTreeSet<Id> = BTreeSet::new();
        SketchState {
            name: "Sketch",
            plane: Plane::Origin(varde_document::OriginPlane::XY),
            sketch,
            pending: &NONE,
            selection,
            listed_on: selection,
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

    /// Whether the Dimension tool is in use.
    pub(crate) fn dimensioning(&self) -> bool {
        self.tool.is_some_and(|tool| tool.tool == Tool::Dimension)
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
    pub picked: &'a [Id],
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
            (Tool::Dimension, _) => match self.picked {
                [] => "Click what to measure",
                // A handle's angle, or with its fit point its length.
                [one] if sketch.handle(*one).is_some() => "Click to place, or pick another",
                [one] if sketch.point(*one).is_some() => "Click a point or a line",
                [one] if sketch.line(*one).is_some() => "Click to place, or pick another",
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
        }
    }
}

/// Unsaved changes a session that crashed left of a document.
pub struct RecoveredChanges {
    /// Whether the design changed since the changes were made to it, so
    /// restoring them may undo changes saved since.
    pub design_changed: bool,
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
    FileMenu,
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

    let recovered = state.recovered.as_ref().map(|recovered| {
        let restore = small_button("Restore", Emphasis::Primary)
            .on_press(Message::File(File::RestoreChanges));
        let discard = small_button("Discard", Emphasis::Secondary)
            .on_press(Message::File(File::DiscardChanges));
        banner(
            text("Unsaved changes found").font(theme::SEMIBOLD),
            &format!(
                "{APP_NAME} closed unexpectedly while this design had changes that weren't \
                 saved{}",
                if recovered.design_changed {
                    ", but the design has changed since: restoring them may undo newer changes"
                } else {
                    ""
                }
            ),
            Some(row![restore, discard].spacing(6).into()),
        )
    });

    // On the panel's colour, as the banners under the toolbar are on the
    // window's: the banner's own is translucent.
    let refused = (state.refused_edit)
        .map(|refused| container(refused_banner(refused)).style(theme::toolbar));

    let content = column![
        toolbar::toolbar(&state),
        read_only,
        recovered,
        save_error,
        row![
            panels::side_panel(&state),
            column![
                refused,
                viewport::viewport(
                    state.mesh,
                    state.sketches,
                    state.camera,
                    state.mode.palette(),
                    state
                        .sketch
                        .map(|sketch| viewport::Sketching::new(sketch, editable)),
                    state.extrude.clone().map(viewport::Extruding::new),
                    state.extrude.as_ref().map(crate::extrude::panel),
                ),
            ],
        ]
        .height(Length::Fill),
    ];
    // The prompt about unsaved changes shows over the delete prompt,
    // which shows over the file menu.
    let content = match (state.overlay, &state.deleting) {
        (Some(Overlay::UnsavedPrompt), _) => {
            Element::from(stack![content, unsaved_prompt(state.name)])
        }
        (_, Some(deleting)) => Element::from(stack![content, delete_prompt(deleting)]),
        (Some(Overlay::FileMenu), None) => {
            let document = state.editor.document();
            let menu = toolbar::file_menu(editable, document.units(), document.tolerance());
            Element::from(stack![content, menu])
        }
        (None, None) => content.into(),
    };

    chrome::window(content, status(&state), hints(&state))
}

/// The status bar's hints: what the keys do for what's going on, the
/// viewport's mouse bindings, and peeking; under the delete prompt only
/// that `Esc` cancels it.
fn hints<'a>(state: &DocumentState<'a>) -> Vec<Element<'a, Message>> {
    // The delete prompt takes every key but `Esc`, and the viewport
    // behind it nothing.
    if state.deleting.is_some() {
        return vec![key_hint(Shortcut::ESCAPE, "Cancel")];
    }
    let sketching = state.sketch.is_some();
    let keys: Vec<_> = if state.picking_plane {
        vec![key_hint(Shortcut::ESCAPE, "Cancel")]
    } else if let Some(extrude) = &state.extrude {
        let pick = extrude
            .editable
            .then(|| mouse_hint(MouseButton::Left, "Pick regions"));
        let ok = extrude.ready.then(|| key_hint(Shortcut::ENTER, "OK"));
        [pick, ok, Some(key_hint(Shortcut::ESCAPE, "Cancel"))]
            .into_iter()
            .flatten()
            .collect()
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
    } else {
        Vec::new()
    };
    // In the Dimension tool the peek key places references instead.
    let peek = !state.sketch.is_some_and(|sketch| sketch.dimensioning());
    let peek = peek.then(|| key_hint(Held::PEEK, state.panel.other(sketching).label()));
    keys.into_iter()
        .chain(viewport::hints(sketching))
        .chain(peek)
        .collect()
}

/// The status bar's hints for the left button and the keys in `sketch`,
/// which can be changed if `editable`: what the tool asks for, placing
/// without snapping, typing values and how to stop it, or selecting, what
/// can be done with the selection and how to leave.
fn sketch_hints<'a>(sketch: &SketchState<'a>, editable: bool) -> Vec<Element<'a, Message>> {
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
            Some(mouse_hint(MouseButton::Left, tool.step(sketch.sketch))),
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
            Some(mouse_hint(MouseButton::Left, tool.step(sketch.sketch))),
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
            Some(mouse_hint(MouseButton::Left, select)),
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
            .chain([key_hint(Shortcut::SPACE, "Clear")])
    });
    std::iter::once(mouse_hint(MouseButton::Left, "Select"))
        .chain(selection.into_iter().flatten())
        .chain([key_hint(Shortcut::ESCAPE, "Finish")])
        .collect()
}

/// The status bar's hints in the Dimension tool: what it asks for, the
/// radius or diameter to switch to, placing a reference and stopping.
fn dimension_hints<'a>(sketch: &Sketch, tool: &ActiveTool<'a>) -> Vec<Element<'a, Message>> {
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
        Some(mouse_hint(MouseButton::Left, tool.step(sketch))),
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
        .filter_map(|&id| sketch.sketch.dimension(id))
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

/// Asks whether to save the changes to the document `name` before it's
/// closed, as a dialog over the whole screen, which dims the rest and
/// keeps it from being clicked.
fn unsaved_prompt(name: &str) -> Element<'_, Message> {
    let choice = |label, choice, emphasis: Emphasis| {
        dialog_button(
            label,
            emphasis.button_style(),
            Message::File(File::Unsaved(choice)),
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

/// `content` as a dialog over the whole screen, which dims the rest and
/// keeps it from being clicked.
fn dialog<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    let dialog = container(content.width(380)).padding(18).style(theme::menu);
    opaque(
        container(opaque(dialog))
            .center(Length::Fill)
            .style(theme::scrim),
    )
}

/// A dialog's button, labelled `label`, in `style`, sending `message`.
fn dialog_button<'a>(
    label: &'a str,
    style: fn(&iced::Theme, button::Status) -> button::Style,
    message: Message,
) -> iced::widget::Button<'a, Message> {
    button(text(label).font(theme::SEMIBOLD))
        .padding([6, 14])
        .style(style)
        .on_press(message)
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
        Message::Look(Look::CancelDelete),
    );
    let delete = dialog_button(
        "Delete",
        theme::danger_button,
        Message::Edit(Edit::ConfirmDelete),
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

/// The status bar's info on the document: what's asked of the user while
/// picking a plane, what the sketch being edited holds, the extrude being
/// set up, the feature selected ([`feature_info`]) or the model
/// ([`model_info`]). Whether its mesh is still being regenerated, or why it
/// couldn't be built, if it couldn't. Then why the last edit was refused,
/// if it was, and whether a save is in flight. On one line, cut where
/// the hints start if it's longer (`chrome::window`).
fn status<'a>(state: &DocumentState<'a>) -> Element<'a, Message> {
    if state.picking_plane {
        return text("Pick a plane for the new sketch")
            .size(12)
            .wrapping(Wrapping::None)
            .font(theme::SEMIBOLD)
            .style(theme::accent_text)
            .into();
    }
    if let Some(sketch) = &state.sketch {
        let standing = standing(sketch).map(|(standing, trouble)| {
            text(format!("{standing} ·"))
                .size(12)
                .wrapping(Wrapping::None)
                .style(if trouble {
                    theme::danger_text
                } else {
                    theme::muted_text
                })
        });
        let refusal = edit_refused(sketch.refusal, sketch.solver_error, sketch.sketch).map(|why| {
            text(format!("· {why}"))
                .size(12)
                .wrapping(Wrapping::None)
                .style(theme::danger_text)
        });
        let checking = sketch.checking.then(|| {
            text("· Checking…")
                .size(12)
                .wrapping(Wrapping::None)
                .style(theme::muted_text)
        });
        return row![
            text(sketch.name)
                .size(12)
                .wrapping(Wrapping::None)
                .font(theme::SEMIBOLD),
            standing,
            text(format!(
                "{}{} · on {}{}",
                sketch_summary(sketch.sketch),
                profile_count(sketch).map_or_else(String::new, |count| format!(" · {count}")),
                sketch.plane.name(),
                status_suffix(state)
            ))
            .size(12)
            .wrapping(Wrapping::None)
            .style(theme::muted_text),
            refusal,
            checking,
        ]
        .spacing(4)
        .into();
    }
    if let Some(extrude) = &state.extrude {
        let regions = match extrude.picked.len() {
            0 => "pick the regions to extrude".to_owned(),
            n => format!("{} picked", counted(n, "region", "regions")),
        };
        return row![
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
        .into();
    }
    let document = state.editor.document();
    if let Some(feature) = state.selected_feature.and_then(|id| document.feature(id)) {
        return row![
            icons::icon(panels::feature_icon(feature), icons::INLINE),
            text(feature.name.as_str())
                .size(12)
                .wrapping(Wrapping::None)
                .font(theme::SEMIBOLD),
            text(format!(
                "{}{}",
                feature_info(feature, document.units()),
                status_suffix(state)
            ))
            .size(12)
            .wrapping(Wrapping::None)
            .style(theme::muted_text),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into();
    }
    let info = model_info(document, state.merged);
    text(format!("{info}{}", status_suffix(state)))
        .size(12)
        .wrapping(Wrapping::None)
        .style(theme::muted_text)
        .into()
}

/// The status bar's info on `document` with nothing selected: "No
/// selection · 2 bodies · 3 features · mm", or "Empty design · mm", in
/// its units. The bodies are counted as the joins leave them, `merged`
/// (see [`DocumentState::merged`]) each one with its holder.
fn model_info(document: &Document, merged: &[(BodyId, BodyId)]) -> String {
    let units = document.units().symbol();
    let features = document.features().len();
    if features == 0 {
        return format!("Empty design · {units}");
    }
    format!(
        "No selection · {} · {} · {units}",
        counted(
            panels::bodies_after_joins(document, merged),
            "body",
            "bodies"
        ),
        counted(features, "feature", "features"),
    )
}

/// The status bar's info on the selected `feature`, after its name: a
/// sketch's curves and plane, "4 lines · 1 circle · 5 points · on XY", or
/// an extrude's extent in `units` and operation, "Distance 10 mm · New
/// body".
fn feature_info(feature: &Feature, units: LengthUnit) -> String {
    match &feature.kind {
        FeatureKind::Sketch { plane, sketch } => {
            format!("{} · on {}", sketch_summary(sketch), plane.name())
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
            format!("{extent} · {operation}")
        }
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

/// The end of the status bar's info, after what it says of the model or
/// the sketch: regenerating, why the last edit failed, saving.
fn status_suffix(state: &DocumentState<'_>) -> String {
    let regenerating = match state.mesh_status {
        MeshStatus::Current => String::new(),
        MeshStatus::Regenerating => " · Regenerating…".to_owned(),
        MeshStatus::Failed(error) => format!(" · Couldn't regenerate: {error}"),
    };
    let edit_error = state
        .edit_error
        .map_or_else(String::new, |error| format!(" · Couldn't edit: {error}"));
    let saving = if state.saving { " · Saving…" } else { "" };
    format!("{regenerating}{edit_error}{saving}")
}

#[cfg(test)]
mod tests {
    use glam::DVec2;
    use varde_sketch::Curve;

    use super::*;

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
    fn the_status_bar_sums_up_the_model_and_the_feature_selected() {
        use varde_document::{Document, Extent, FeatureKind, Operation};

        assert_eq!(model_info(&Document::default(), &[]), "Empty design · mm");
        let document = Document::example();
        assert_eq!(
            model_info(&document, &[]),
            "No selection · 1 body · 2 features · mm"
        );
        let units = document.units();
        let [sketch, extrude] = [0, 1].map(|k| &document.features()[k]);
        assert_eq!(
            feature_info(sketch, units),
            "4 lines · 1 circle · 5 points · on XY"
        );
        assert_eq!(feature_info(extrude, units), "Distance 10 mm · New body");

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
            feature_info(&feature, units)
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
        assert_eq!(info(Extent::ThroughAll, cut), "Through all · Cut");
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
