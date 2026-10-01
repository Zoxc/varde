//! An open document: its editor and view, and saving and leaving it, see
//! [`save`].

mod camera;
mod delete;
mod extrude;
mod feed;
mod rail;
mod save;
mod sketch;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::Vec3;
use iced::Element;
use iced::time::Instant;
use varde_document::name::UNTITLED;
use varde_document::{
    BodyId, Command, Document, EditError, Editor, FeatureId, FeatureKind, LengthUnit, OriginPlane,
    Removable, Removal, Revision, Tolerance,
};
use varde_io::{Access, Offer, OpenId};
use varde_render::{Camera, Projection};
use varde_solve::{Request as SolveRequest, Transport};
use varde_view::{DocumentKeys, Edit, Look, Message as Ui, Mode, Overlay, Panel, RowMenu, Snap};

#[cfg(test)]
pub(crate) use camera::CAMERA_ANIMATION;
pub(crate) use camera::CameraAnimation;
use camera::Pivot;
#[cfg(test)]
pub(crate) use camera::{PIVOT_FADE, PIVOT_SHOWN};
use delete::Deleting;
pub(crate) use extrude::ExtrudeSession;
use feed::MeshFeed;
use rail::Rail;
use save::Persist;
#[cfg(test)]
pub(crate) use save::{AutoSave, Picking};
pub(crate) use save::{Downloader, Downloads, Leave, Target};
use sketch::GEOMETRY_SHARE;
#[cfg(test)]
pub(crate) use sketch::Refusal;
pub(crate) use sketch::{Focus, Proposals, SketchSession};

use crate::{Files, ForDoc, Next};

/// An open document and the state of its view.
pub(crate) struct Doc {
    /// Tags the regeneration lane and its responses.
    pub(crate) id: DocId,
    pub(crate) editor: Editor,
    pub(crate) camera: Camera,
    /// The mesh the viewport shows, as it comes from the document's
    /// regeneration lane.
    pub(crate) feed: MeshFeed,
    /// The document's solver lane, a thread natively and a Web Worker on
    /// the web, once it has started, see [`Varde::solve_lane`]: sketch
    /// edits are proposed through it, drags stepped and sketches analysed.
    /// Until then proposals wait in [`Doc::proposals`].
    ///
    /// [`Varde::solve_lane`]: crate::Varde::solve_lane
    pub(crate) solver: Option<Box<dyn Transport<SolveRequest>>>,
    /// What's asked of the solver lane and not answered yet.
    pub(crate) proposals: Proposals,
    /// Why the document can't be edited, if it can't.
    pub(crate) read_only: Option<String>,
    /// Why the last edit was refused, if it was.
    pub(crate) edit_error: Option<EditError>,
    /// A sketch edit the solver refused after its sketch was left, which
    /// sketch it was of and why: shown in a banner over the viewport
    /// until dismissed, see [`Doc::refused_edit`].
    pub(crate) refused_edit: Option<(FeatureId, sketch::Refusal)>,
    /// Shown to the user in place of a path.
    pub(crate) name: String,
    pub(crate) panel: Panel,
    pub(crate) file_menu: bool,
    /// Whether the view options menu, from the status bar, is open.
    pub(crate) view_menu: bool,
    /// The removal the user is asked about, if one is: see [`Doc::remove`].
    pub(crate) deleting: Option<Deleting>,
    /// Whether the plane for a new sketch is being picked.
    pub(crate) picking_plane: bool,
    /// The editor's [`lineage`](varde_document::Editor::lineage) as of
    /// the last [`Doc::sync`]: the ids held across edits (the feature
    /// selected, the sessions') name things in it, see [`Doc::prune`] and
    /// [`Doc::prune_extrude`].
    lineage: Revision,
    /// The feature selected in the Timeline, if any.
    pub(crate) selected_feature: Option<FeatureId>,
    /// The row of the side panel whose context menu is open, if one is:
    /// a feature of the Timeline's only while it's selected.
    pub(crate) row_menu: Option<RowMenu>,
    /// The sketch being edited, if one is.
    pub(crate) sketch: Option<SketchSession>,
    /// The extrude being set up, if one is: never with a sketch.
    pub(crate) extrude: Option<ExtrudeSession>,
    /// The share of the Sketch tab's height the Geometry list takes, kept
    /// from one sketch to the next.
    pub(crate) sketch_split: f32,
    /// What the value field is to do once it shows, which the app asks of
    /// it: see [`Doc::take_focus`].
    focus: Option<Focus>,
    animation: Option<CameraAnimation>,
    /// The view the camera had before turning to the sketch being edited,
    /// which it turns back to on leaving it.
    before_sketch: Option<Camera>,
    /// The point picked for the camera to orbit, if one was.
    pivot: Option<Pivot>,
    /// Whether the cursor is over the view cube, where the pivot is
    /// marked.
    cube_hovered: bool,
    /// The tool rail's open set.
    pub(crate) rail: Rail,
    /// Saving and leaving it, see [`Persist`].
    persist: Persist,
}

/// Tells open documents apart, so work done for a closed document is never
/// applied to the one opened after it. Unique within the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DocId(u64);

impl DocId {
    pub(crate) fn unique() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// What a document is made from, besides its design: where it came from.
pub(crate) struct Origin {
    pub(crate) target: Target,
    /// Whether it may be edited.
    pub(crate) access: Access,
    /// Shown to the user in place of a path.
    pub(crate) name: String,
    /// Whether what it was loaded as is saved: not for a new design
    /// recovered after a crash. It's in its store entry either way.
    pub(crate) saved: bool,
    /// The store entry asked for for a new design, see [`Persist::creating`].
    pub(crate) creating: Option<OpenId>,
    /// Unsaved changes a crashed session left, see [`Persist::recovered`].
    pub(crate) recovered: Option<Offer>,
}

impl Origin {
    /// A design written to `target`, loaded as saved, with nothing on its
    /// way and nothing recovered.
    pub(crate) fn new(target: Target, access: Access, name: String) -> Self {
        Self {
            target,
            access,
            name,
            saved: true,
            creating: None,
            recovered: None,
        }
    }
}

/// Where Home and the view cube turn the camera to look at: the origin.
const HOME_TARGET: Vec3 = Vec3::ZERO;

/// The camera Home turns to, framed on the origin.
fn home_camera(projection: Projection) -> Camera {
    let mut camera = Camera::default();
    camera.set_projection(projection);
    camera.set_target(HOME_TARGET);
    camera
}

impl Doc {
    pub(crate) fn new(document: Document, origin: Origin) -> Self {
        let Origin {
            target,
            access,
            name,
            saved,
            creating,
            recovered,
        } = origin;
        let editor = Editor::new(document);
        let revision = editor.revision();
        let lineage = editor.lineage();
        let mut doc = Self {
            id: DocId::unique(),
            persist: Persist::new(
                target,
                creating,
                recovered,
                saved.then_some(revision),
                revision,
            ),
            editor,
            camera: home_camera(Projection::default()),
            feed: MeshFeed::new(),
            solver: None,
            proposals: Proposals::default(),
            read_only: read_only(access),
            edit_error: None,
            refused_edit: None,
            name,
            panel: Panel::default(),
            file_menu: false,
            view_menu: false,
            deleting: None,
            picking_plane: false,
            lineage,
            selected_feature: None,
            row_menu: None,
            sketch: None,
            extrude: None,
            sketch_split: GEOMETRY_SHARE,
            focus: None,
            animation: None,
            before_sketch: None,
            pivot: None,
            cube_hovered: false,
            rail: Rail::default(),
        };
        doc.sync();
        doc
    }

    /// Lets go of what the document no longer holds, and of ids that may
    /// name other things since the document was replaced whole, see
    /// [`Doc::prune`], [`Doc::prune_extrude`] and [`MeshFeed::replaced`],
    /// asks for the model if the document
    /// changed, or the sketch being edited did, which is left out of it,
    /// or the extrude being set up, and finds the profiles of the sketch
    /// shown if it changed.
    pub(crate) fn sync(&mut self) {
        self.send_proposal();
        self.refresh_waiting();
        let lineage = self.editor.lineage();
        let replaced = std::mem::replace(&mut self.lineage, lineage) != lineage;
        if replaced {
            self.feed.replaced(self.editor.generation());
        }
        self.prune_deleting();
        self.prune(replaced);
        self.prune_extrude(replaced);
        self.request_analysis();
        self.request_model();
        self.refresh_profiles();
    }

    /// Asks for the model if the document changed, the sketch left out of
    /// it (the one being edited) or the extrude being set up did, which is
    /// previewed as a draft.
    fn request_model(&mut self) {
        let exclude = self.sketch.as_ref().map(|session| session.feature);
        let draft = self.draft();
        self.feed.request_with(&self.editor, exclude, draft);
    }

    /// Whether the camera is turning to a new view, the pivot's marker
    /// fading, or the rail's list waiting to close.
    pub(crate) fn animating(&self) -> bool {
        self.animation.is_some() || self.pivot_fading() || self.rail.closing()
    }

    /// Whether the document may be edited, and so saved.
    pub(crate) fn editable(&self) -> bool {
        self.read_only.is_none()
    }

    /// Changes the document through `edit`, unless it's read-only, keeping
    /// why the edit failed, if it did. Every change to the document goes
    /// through here or [`Doc::edit_surely`].
    fn edit(&mut self, edit: impl FnOnce(&mut Editor) -> Result<(), EditError>) {
        if !self.editable() {
            return;
        }
        self.edit_error = edit(&mut self.editor).err();
    }

    /// Changes the document through `edit`, which can't fail, unless it's
    /// read-only: why the last edit failed no longer shows.
    fn edit_surely(&mut self, edit: impl FnOnce(&mut Editor)) {
        if self.editable() {
            edit(&mut self.editor);
            self.edit_error = None;
        }
    }

    /// Applies `command`, keeping why it failed, if it did.
    pub(crate) fn apply(&mut self, command: Command) {
        self.edit(|editor| editor.apply(command));
    }

    /// Takes `message`, asking something of the document itself. What
    /// the solver last refused shows until then.
    pub(crate) fn update(&mut self, message: Edit) {
        self.end_refusal();
        self.row_menu = None;
        // Any other edit, a click of the Dimension tool included, leaves
        // the value field; placing one opens another. Not a label let go
        // of: the release of a double-click opening the field on it. Nor
        // placing a shape while a drawing tool's field is open, which
        // takes the value typed there first.
        let placing = matches!(message, Edit::ToolClick(_) | Edit::PlaceShape)
            && self.drawing_field().is_some();
        if !(placing || matches!(message, Edit::SubmitValue | Edit::DropLabel)) {
            self.close_value();
        }
        // Picked from the rail's list, or not: the list has done its job.
        if matches!(message, Edit::Constrain(_)) {
            self.rail.close();
        }
        match message {
            Edit::ToggleFileMenu => self.file_menu = !self.file_menu,
            Edit::DismissSaveError => self.dismiss_save_error(),
            Edit::DismissRefusedEdit => self.refused_edit = None,
            Edit::RemoveBody(id) => self.remove(Removable::Body(id)),
            Edit::ToggleVisible(id) => self.change(Change::ToggleVisible(id)),
            // Not another plane picked while the new sketch waits.
            Edit::NewSketch(plane) => {
                self.picking_plane = false;
                self.change(Change::NewSketch(plane));
            }
            Edit::RemoveFeature(id) => self.remove(Removable::Feature(id)),
            Edit::ConfirmDelete => self.confirm_delete(),
            Edit::ToggleFeatureVisible(id) => self.change(Change::ToggleFeatureVisible(id)),
            Edit::ToolClick(click) => self.tool_click(click),
            Edit::DropGeometry => self.drop_geometry(),
            Edit::DeleteSelection => self.delete_selection(),
            Edit::ToggleConstruction => self.toggle_construction(),
            Edit::Constrain(kind) => self.constrain(kind),
            Edit::SubmitValue => self.submit_value(),
            Edit::DropLabel => self.drop_label(),
            Edit::ToggleReference => self.toggle_reference(),
            Edit::PlaceShape => self.place_shape(),
            Edit::ConvertSplines => self.convert_splines(),
            Edit::ToggleHandles => self.toggle_handles(),
            Edit::InsertSplinePoint { spline, at } => self.insert_spline_point(spline, at),
            Edit::CommitExtrude => self.commit_extrude(),
            Edit::SetUnits(units) => self.change(Change::SetUnits(units)),
            Edit::SetTolerance(tolerance) => self.change(Change::SetTolerance(tolerance)),
            // What waits on the solver, and what waits behind it, is newer
            // than anything committed: undo takes back the newest of it.
            Edit::Undo if self.proposing() => self.drop_newest(),
            Edit::Undo => self.edit_surely(Editor::undo),
            // What waits comes after what's undone, as a new edit does,
            // and edits waiting name items by the ids the sketch has
            // without it.
            Edit::Redo if self.proposing() => {}
            Edit::Redo => self.edit_surely(Editor::redo),
        }

        self.sync();
    }

    /// Makes `change`, or, while edits wait on the solver, has it wait
    /// behind them, see [`Proposals`]: so what's committed, and so undo,
    /// keeps the order the user made them in.
    fn change(&mut self, change: Change) {
        if self.proposing() {
            self.proposals.wait(change);
        } else {
            self.make(change);
        }
    }

    /// Makes `change` now, on the document as it is.
    pub(crate) fn make(&mut self, change: Change) {
        match change {
            Change::Remove { target, confirmed } => self.remove_now(target, confirmed),
            Change::ToggleVisible(id) => {
                if let Some(body) = self.editor.document().body(id) {
                    let visible = !body.visible;
                    self.apply(Command::SetVisible(id, visible));
                }
            }
            Change::ToggleFeatureVisible(id) => {
                if let Some(feature) = self.editor.document().feature(id) {
                    let visible = !feature.visible;
                    self.apply(Command::SetFeatureVisible(id, visible));
                }
            }
            Change::NewSketch(plane) => self.new_sketch(plane),
            Change::SetUnits(units) => self.apply(Command::SetUnits(units)),
            Change::SetTolerance(tolerance) => self.apply(Command::SetTolerance(tolerance)),
        }
    }

    /// Takes `message`, which only changes how the document is looked at.
    /// An action in the sketch ends what the solver last refused showing.
    pub(crate) fn look(&mut self, message: Look) {
        let asked = self.delete_asked();
        self.look_at(message);
        // The delete prompt cancelled: what waits on the solver, held while
        // it was up, goes on.
        if asked && !self.delete_asked() && self.proposing() {
            self.sync();
        }
        self.list_selection();
        // A drag's step shows another sketch, and letting go of it the
        // sketch before.
        self.refresh_profiles();
        // The extrude being set up is previewed as it changes.
        self.request_model();
    }

    /// Takes `message`, see [`Doc::look`].
    fn look_at(&mut self, message: Look) {
        if matches!(
            message,
            Look::Escape
                | Look::ClickGeometry { .. }
                | Look::ClickRow(_)
                | Look::SelectBox { .. }
                | Look::ClearSelection
                | Look::SelectTool(_)
                | Look::ToggleConstrain
                | Look::PressLabel { .. }
                | Look::EditDimension { .. }
        ) {
            self.end_refusal();
        }
        // Acting on the sketch elsewhere leaves the value field.
        if matches!(
            message,
            Look::ClickGeometry { .. }
                | Look::ClickRow(_)
                | Look::SelectBox { .. }
                | Look::SelectTool(_)
                | Look::ToggleConstrain
                | Look::PressLabel { .. }
                | Look::DragGeometry { .. }
                | Look::EditFeature(_)
                | Look::FinishSketch
                | Look::PickPlane
        ) {
            self.close_value();
        }
        // A row's context menu does its job or is left by anything else
        // done: `Esc` closes it alone.
        if !matches!(
            message,
            Look::OpenMenu(_)
                | Look::Escape
                | Look::HoverItem(_)
                | Look::HoverCube(_)
                | Look::Snap(_)
                | Look::Aim(_)
                | Look::ScrollGeometry(_)
                | Look::ScrollConstraints(_)
        ) {
            self.row_menu = None;
        }
        // Picking a tool, from the rail's list or not, closes the list.
        if matches!(
            message,
            Look::SelectTool(_)
                | Look::ToggleConstrain
                | Look::PickPlane
                | Look::StartExtrude
                | Look::EditFeature(_)
        ) {
            self.rail.close();
        }
        match message {
            Look::CloseFileMenu => self.file_menu = false,
            Look::ToggleViewMenu => self.view_menu = !self.view_menu,
            Look::CloseViewMenu => self.view_menu = false,
            Look::CancelDelete => self.deleting = None,
            Look::Escape => self.escape(),
            // Only the tabs showing can be picked, but a message sent before
            // entering or leaving a sketch may come after.
            Look::SelectPanel(panel) => self.panel = panel.for_sketching(self.sketch.is_some()),
            Look::PickPlane => self.pick_plane(),
            Look::EditFeature(id) => match self.editor.document().feature(id).map(|f| &f.kind) {
                Some(FeatureKind::Extrude(_)) => self.edit_extrude(id),
                _ => self.enter_sketch(id),
            },
            Look::StartExtrude => self.start_extrude(),
            Look::Extrude(message) => self.extrude_look(message),
            Look::FinishSketch => self.finish_sketch(),
            Look::SelectFeature(id) => {
                if self.editor.document().feature(id).is_some() {
                    self.selected_feature = Some(id);
                }
            }
            Look::OpenMenu(menu) => self.open_menu(menu),
            Look::CloseMenu => {}
            Look::ClickGeometry { hit, add } => self.click_geometry(hit, add),
            // The app turns this into a `ClickGeometry`, knowing the keys
            // held; alone, it selects.
            Look::ClickRow(id) => self.click_geometry(Some(id), false),
            Look::HoverItem(id) => self.hover_item(id),
            Look::Snap(snap) => {
                if let Some(session) = &mut self.sketch {
                    session.snap = snap;
                }
            }
            Look::Aim(click) => {
                if let Some(session) = &mut self.sketch {
                    session.snap = Some(click.snap()).filter(Snap::snapped);
                    session.aim = Some(click);
                }
            }
            Look::NextField => self.next_field(),
            Look::ToggleCentered => self.toggle_centered(),
            Look::ToggleSplineKind => self.toggle_spline_kind(),
            Look::ToggleComb => self.toggle_comb(),
            Look::MirrorAbout => self.mirror_about(),
            Look::ToggleConstrain => self.toggle_constrain(),
            Look::ToggleGlyphs => self.toggle_glyphs(),
            Look::SelectBox { ids, add } => self.select_box(ids, add),
            Look::ClearSelection => self.clear_selection(),
            Look::SelectTool(tool) => self.select_tool(tool),
            Look::DragGeometry { id, from, to } => self.drag_geometry(id, from, to),
            Look::CancelDrag => {
                if let Some(session) = &mut self.sketch {
                    session.drag = None;
                }
            }
            Look::PressLabel { id, add } => self.press_label(id, add),
            Look::DragLabel { id, from, to } => self.drag_label(id, from, to),
            Look::EditDimension { id, in_list } => self.edit_dimension(id, in_list),
            Look::ValueInput(text) => self.value_input(text),
            Look::CancelValue => self.close_value(),
            Look::SwitchRound => self.switch_round(),
            Look::SplitSketchTab(share) => {
                if share.is_finite() {
                    self.sketch_split = share.clamp(0.0, 1.0);
                }
            }
            Look::ScrollGeometry(offset) => {
                if let Some(session) = &mut self.sketch {
                    session.scroll = offset;
                }
            }
            Look::ScrollConstraints(offset) => {
                if let Some(session) = &mut self.sketch {
                    session.constraint_scroll = offset;
                }
            }
            Look::Orbit { yaw, pitch } => self.orbit(yaw, pitch),
            Look::Pan { dx, dy } => {
                self.animation = None;
                self.camera.pan(dx, dy);
            }
            Look::Zoom { factor, x, y } => {
                self.animation = None;
                self.camera.zoom_at(factor, x, y);
            }
            // In a sketch, Home faces it. Either way it orbits its target
            // again.
            Look::ResetCamera => {
                self.pivot = None;
                let home = self
                    .sketch_camera()
                    .unwrap_or_else(|| home_camera(self.camera.projection()));
                self.animate_camera(home);
            }
            Look::LookFrom(view) => {
                let mut to = self.camera;
                to.look_from(view);
                to.set_target(
                    self.sketch_camera()
                        .map_or(HOME_TARGET, |home| home.target()),
                );
                self.animate_camera(to);
            }
            Look::SetPivot(at) => self.set_pivot(at, Instant::now()),
            Look::HoverCube(over) => self.hover_cube(over, Instant::now()),
            Look::Rail(message) => self.rail_look(message, Instant::now()),
            Look::SetProjection(projection) => {
                self.view_menu = false;
                self.camera.set_projection(projection);
                if let Some(animation) = &mut self.animation {
                    animation.to.set_projection(projection);
                }
            }
        }
    }

    /// Opens the context menu of the side panel's row `menu` is on,
    /// selecting a feature of the Timeline's, unless in a sketch, where
    /// neither list's rows show.
    fn open_menu(&mut self, menu: RowMenu) {
        let document = self.editor.document();
        let exists = match menu {
            RowMenu::Feature(id) | RowMenu::Sketch(id) => document.feature(id).is_some(),
            RowMenu::Body(id) => document.body(id).is_some(),
        };
        if self.sketch.is_some() || !exists {
            return;
        }
        if let RowMenu::Feature(id) = menu {
            self.selected_feature = Some(id);
        }
        self.row_menu = Some(menu);
    }

    /// Starts sending requests to `lane`, the document's regeneration
    /// lane.
    pub(crate) fn lane_ready(&mut self, lane: varde_regen::lane::Lane) {
        self.feed.connect(lane);
        self.sync();
    }

    /// Shows `response`, computed for the document.
    pub(crate) fn computed(&mut self, response: varde_regen::Response) {
        self.feed.apply(response);
    }

    /// Starts sending requests to `lane`, the document's solver lane:
    /// the proposals waiting, and the analysis of the sketch being edited.
    pub(crate) fn solver_ready(&mut self, lane: impl Transport<SolveRequest> + 'static) {
        if self.solver.is_some() {
            self.lane_replaced();
        }
        self.solver = Some(Box::new(lane));
        self.sync();
    }

    /// Stops showing why the solver refused the last edit.
    fn end_refusal(&mut self) {
        if let Some(session) = &mut self.sketch {
            session.refusal = None;
        }
    }

    /// What the document screen's shortcuts depend on, or `None` while
    /// the user is asked about unsaved changes or deleting: only the
    /// prompt's buttons and `Esc` act then, not keys changing the document
    /// behind it.
    pub(crate) fn keys(&self) -> Option<DocumentKeys> {
        self.dialog().is_none().then(|| {
            DocumentKeys::new(self.editable(), self.selected_feature, self.sketch_state())
                .with_extrude(self.extrudable(), self.extrude_state().as_ref())
                .with_rail(self.rail.state())
                .with_edited(self.edited())
                .with_history(
                    self.editor.can_undo() || self.proposing(),
                    self.editor.can_redo() && !self.proposing(),
                )
        })
    }

    /// The prompt the user is being asked, if any, which `Esc` cancels:
    /// about unsaved changes, over the one about deleting.
    pub(crate) fn dialog(&self) -> Option<Dialog> {
        if self.prompt().is_some() {
            Some(Dialog::Unsaved)
        } else if self.delete_prompt().is_some() {
            Some(Dialog::Delete)
        } else {
            None
        }
    }

    /// Whether the other panel tab shows with the peek key `held`: not in
    /// the Dimension tool, where it's held to place references.
    pub(crate) fn peeks(&self, held: bool) -> bool {
        held && !self.dimensioning()
    }

    /// Takes `message`, an answer for this document: the app has checked
    /// it's for this one and not one closed since.
    pub(crate) fn answer(&mut self, cx: &mut Files, message: ForDoc) -> Next {
        match message {
            ForDoc::SaveAsPicked(chosen) => self.save_as_picked(cx, chosen),
            ForDoc::Writable(result) => self.writable(cx, result),
            ForDoc::RegenReady(lane) => {
                self.lane_ready(lane);
                Next::Stay
            }
            ForDoc::Computed(response) => {
                self.computed(response);
                Next::Stay
            }
            // Proposals whose sketch went while the lane started are
            // dropped, which a save may have waited for.
            ForDoc::SolveReady(lane) => {
                self.solver_ready(lane);
                self.proposals_settled(cx)
            }
            ForDoc::Solved(response) => {
                self.solved(response);
                self.proposals_settled(cx)
            }
        }
    }

    /// The document screen, showing the other panel tab if `peek`, unless
    /// the Dimension tool is in use, where the peek key places references,
    /// and the status bar's hints of the mouse if `mouse_hints`.
    pub(crate) fn view(&self, peek: bool, mode: Mode, mouse_hints: bool) -> Element<'_, Ui> {
        let peek = self.peeks(peek);
        varde_view::document(varde_view::DocumentState {
            editor: &self.editor,
            camera: &self.camera,
            pivot: self.pivot_marker(),
            mesh: self.feed.mesh(),
            sketches: self.feed.sketches(),
            mesh_status: self.feed.status(&self.editor),
            name: &self.name,
            edited: self.edited(),
            read_only: self.read_only.as_deref(),
            edit_error: self.edit_error.as_ref(),
            refused_edit: self.refused_edit(),
            saving: self.saving(),
            save_error: self.banner_error(),
            recovered: self.recovered().map(|offer| varde_view::RecoveredChanges {
                design_changed: offer.design_changed,
            }),
            // The prompt shows over the menus.
            overlay: self
                .prompt()
                .map(|_| Overlay::UnsavedPrompt)
                .or(self.file_menu.then_some(Overlay::FileMenu))
                .or(self.view_menu.then_some(Overlay::ViewMenu)),
            panel: self.panel,
            peek,
            mode,
            mouse_hints,
            picking_plane: self.picking_plane,
            selected_feature: self.selected_feature,
            row_menu: self.row_menu,
            sketch: self.sketch_state(),
            extrude: self.extrude_state(),
            extrudable: self.extrudable(),
            unsolved: self.feed.unsolved(),
            failed: self.feed.failed_features(),
            merged: self.feed.merged_bodies(),
            deleting: self.delete_prompt(),
            proposing: self.proposing(),
            rail: self.rail.state(),
        })
    }
}

/// A change to the document other than a sketch edit, as the user asked
/// for it, which waits behind the edits waiting on the solver, see
/// [`Doc::change`]. It's made on the document as it is then, so toggling
/// twice toggles back.
#[derive(Debug, Clone)]
pub(crate) enum Change {
    /// Removes `target` and what goes with it, asking first if more goes
    /// than `confirmed`, what the user said yes to, if anything, see
    /// [`Doc::remove`].
    Remove {
        target: Removable,
        confirmed: Option<Removal>,
    },
    ToggleVisible(BodyId),
    ToggleFeatureVisible(FeatureId),
    NewSketch(OriginPlane),
    SetUnits(LengthUnit),
    SetTolerance(Tolerance),
}

/// A prompt over the document screen, see [`Doc::dialog`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Dialog {
    /// About unsaved changes, before the document is closed.
    Unsaved,
    /// About deleting more than was asked for.
    Delete,
}

/// The name shown for the design at `path`: its file name without the
/// extension.
pub(crate) fn design_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| UNTITLED.to_owned(), |s| s.to_string_lossy().into_owned())
}

/// Why a document with `access` can't be edited, if it can't.
fn read_only(access: Access) -> Option<String> {
    match access {
        Access::Edit => None,
        Access::ReadOnly(reason) => Some(reason.to_string()),
    }
}
