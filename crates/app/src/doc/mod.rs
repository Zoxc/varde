//! An open document: its editor and view, and saving and leaving it, see
//! [`save`].

mod camera;
mod combine;
mod delete;
mod errors;
mod export;
mod extrude;
mod feed;
mod measure;
mod motion;
mod overlaps;
mod pick;
mod rail;
mod regions;
mod relink;
pub(crate) mod rename;
mod revolve;
mod save;
mod sketch;
mod thumbnail;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::Vec3;
use iced::Element;
use iced::time::Instant;
use varde_document::name::UNTITLED;
use varde_document::{
    BodyId, Command, Document, EditError, Editor, FeatureId, FeatureKind, LengthUnit, Opacity,
    Plane, Removable, Removal, Revision, Tolerance,
};
use varde_io::{Access, Damage, DamageKind, LastDownload, Offer, OpenId, UnixSeconds};
use varde_render::{Camera, Projection};
use varde_solve::{Request as SolveRequest, Transport};
use varde_view::{
    DocumentKeys, Edit, Look, Message as Ui, Mode, Overlay, Panel, RowMenu, Snap, ViewOptions,
};

#[cfg(test)]
pub(crate) use camera::CAMERA_ANIMATION;
pub(crate) use camera::CameraAnimation;
use camera::Pivot;
#[cfg(test)]
pub(crate) use camera::{PIVOT_FADE, PIVOT_SHOWN};
pub(crate) use combine::CombineSession;
use delete::Deleting;
use export::Export;
#[cfg(test)]
pub(crate) use export::Exporting;
pub(crate) use extrude::ExtrudeSession;
use feed::MeshFeed;
pub(crate) use measure::MeasureSession;
pub(crate) use motion::MotionSession;
use pick::ModelPick;
use rail::Rail;
pub(crate) use revolve::RevolveSession;
use save::Persist;
#[cfg(test)]
pub(crate) use save::{AutoSave, Picking};
pub(crate) use save::{Downloader, Leave, Target};
use sketch::GEOMETRY_SHARE;
#[cfg(test)]
pub(crate) use sketch::Refusal;
pub(crate) use sketch::{Focus, Proposals, SketchSession};

use crate::{Files, ForDoc, Next, Offers, when};

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
    /// Why the last thing asked couldn't be done, if the app refused it
    /// itself (a sketch on a curved face, a sketch that isn't placed
    /// entered): shown in the status bar until the next thing asked.
    pub(crate) notice: Option<String>,
    /// What the app is to show as a toast, for a few seconds, if
    /// anything: taken by the app after each message
    /// ([`Doc::take_toast`]).
    toast: Option<String>,
    /// The rename field, if it's open on a feature, sketch or body.
    pub(crate) renaming: Option<rename::Renaming>,
    /// Whether the rename field is to take the focus, as it just opened.
    rename_focus: bool,
    /// Where the newest sketch on a face was placed when its face was
    /// picked, until a model places it: see [`Doc::placement`].
    placed: Option<sketch::Placed>,
    /// A sketch edit the solver refused after its sketch was left, which
    /// sketch it was of and why: shown in a banner over the viewport
    /// until dismissed, see [`Doc::refused_edit`].
    pub(crate) refused_edit: Option<(FeatureId, sketch::Refusal)>,
    /// Shown to the user in place of a path.
    pub(crate) name: String,
    /// Natively, the path of the design's own file, the home directory
    /// as `~`, which pointing at the file cell shows.
    pub(crate) path: Option<String>,
    pub(crate) panel: Panel,
    pub(crate) file_menu: bool,
    /// Whether the view options menu, from the status bar, is open.
    pub(crate) view_menu: bool,
    /// Its submenu open, if one is: only shown while it is.
    pub(crate) view_submenu: Option<varde_view::ViewSubmenu>,
    /// The removal the user is asked about, if one is: see [`Doc::remove`].
    pub(crate) deleting: Option<Deleting>,
    /// The plane being picked, if one is: for a new sketch, or for the
    /// sketch whose plane is changed.
    pub(crate) picking_plane: Option<sketch::PickingPlane>,
    /// The editor's [`lineage`](varde_document::Editor::lineage) as of
    /// the last [`Doc::sync`]: the ids held across edits (the feature
    /// selected, the sessions') name things in it, see [`Doc::prune`] and
    /// [`Doc::prune_extrude`].
    lineage: Revision,
    /// The feature selected in the Timeline, if any.
    pub(crate) selected_feature: Option<FeatureId>,
    /// The feature whose row in the Timeline the cursor is over, if any.
    pub(crate) hovered_feature: Option<FeatureId>,
    /// The failures' geometry the viewport draws, see
    /// [`Doc::shown_errors`].
    errors: Arc<varde_view::ShownErrors>,
    /// The row of the side panel whose context menu is open, if one is:
    /// a feature of the Timeline's only while it's selected.
    pub(crate) row_menu: Option<RowMenu>,
    /// The world's origin objects shown, as Objects toggles them: the
    /// app's, not the document's, so they're neither saved nor undone.
    pub(crate) origin: varde_render::OriginShown,
    /// The origin objects and sketches selected in Objects, in the order
    /// they were.
    pub(crate) objects_selected: Vec<varde_view::ObjectRow>,
    /// The origin object whose row in Objects is hovered, if one is.
    pub(crate) origin_hover: Option<varde_view::OriginObject>,
    /// The origin plane hovered in the viewport picking a plane, if one
    /// is: only while a plane is picked.
    pub(crate) plane_hover: Option<varde_document::OriginPlane>,
    /// The Objects tab's groups folded.
    pub(crate) objects_folded: std::collections::BTreeSet<varde_view::ObjectGroup>,
    /// What overlaps where the left button was held still in the
    /// viewport, listed to choose from, and the row hovered.
    overlaps: Option<overlaps::Listed>,
    /// A body's opacity previewed while its context menu's slider is
    /// dragged: only while that menu is open ([`Doc::preview_opacity`]).
    pub(crate) opacity_preview: Option<(BodyId, Opacity)>,
    /// The sketch being edited, if one is.
    pub(crate) sketch: Option<SketchSession>,
    /// The extrude being set up, if one is: never with a sketch.
    pub(crate) extrude: Option<ExtrudeSession>,
    /// The revolve being set up, if one is: never with a sketch or an
    /// extrude.
    pub(crate) revolve: Option<RevolveSession>,
    /// The combine being set up, if one is: never with a sketch or another
    /// operation.
    pub(crate) combine: Option<CombineSession>,
    /// The move, mirror or pattern being set up, if one is: never with a
    /// sketch or another operation.
    pub(crate) motion: Option<MotionSession>,
    /// How each pattern committed in this run was set up (its mode, Flip
    /// and spread as typed), which its stored values can't always tell:
    /// taken again on editing it while it still gives them, see
    /// [`motion::PatternShape`].
    pub(crate) pattern_shapes: HashMap<FeatureId, motion::PatternShape>,
    /// The measure tool, while it's in use: never with a sketch or an
    /// operation being set up.
    pub(crate) measure: Option<MeasureSession>,
    /// The share of the Sketch tab's height the Geometry list takes, kept
    /// from one sketch to the next.
    pub(crate) sketch_split: f32,
    /// What the value field is to do once it shows, which the app asks of
    /// it: see [`Doc::take_focus`].
    focus: Option<Focus>,
    animation: Option<CameraAnimation>,
    /// Whether the camera is to frame the first model shown, see
    /// [`Doc::fit_first_model`]: a document opened with features (from a
    /// file or as a sample), until its first model shows.
    fit_on_model: bool,
    /// The view the camera had before turning to the sketch being edited,
    /// which it turns back to on leaving it.
    before_sketch: Option<Camera>,
    /// The side panel tab shown before the sketch was entered, shown
    /// again on leaving it: entering one shows the Sketch tab.
    panel_before_sketch: Option<Panel>,
    /// The view the camera had, and the point it orbited, before Show
    /// framed it on where the draft fails, which Go back turns it back
    /// to: kept while the draft fails with geometry to frame.
    before_show: Option<(Camera, Option<Pivot>)>,
    /// The point picked for the camera to orbit, if one was.
    pivot: Option<Pivot>,
    /// Whether the cursor is over the view cube, where the pivot is
    /// marked.
    cube_hovered: bool,
    /// What the cursor is over and what's selected in the model shown.
    pub(crate) pick: ModelPick,
    /// The tool rail's open set.
    pub(crate) rail: Rail,
    /// Saving and leaving it, see [`Persist`].
    persist: Persist,
    /// Exporting its bodies, see [`Doc::request_export`].
    export: Export,
    /// The thumbnails its saves write, see `thumbnail.rs`.
    thumbnails: thumbnail::Thumbnails,
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
    /// What a crashed session left, see [`Persist::recovered`].
    pub(crate) recovered: Option<Recovery>,
    /// How the file it was opened from was found damaged, if it was.
    pub(crate) damage: Option<FileDamage>,
    /// For a design in browser storage, its last download, if any.
    pub(crate) download: Option<LastDownload>,
}

/// What a crashed session left of a design, waiting for the user's
/// answer: auto-saves wait meanwhile, so as not to replace it, and
/// closing keeps it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Recovery {
    /// Unsaved changes, offered to be restored.
    Offered(Offer),
    /// What can't be read, kept for whatever can still be got out of it
    /// until the user discards it (see [`varde_io::RecoveryError::kept`]):
    /// the IO lane refuses auto-saves meanwhile.
    Kept,
}

/// How the file a document was opened from was found damaged, see
/// [`Origin::damage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileDamage {
    pub(crate) damage: Damage,
    /// Whether the file is the store entry of a new design, holding
    /// auto-saves, rather than a file of the user's.
    pub(crate) entry: bool,
}

impl FileDamage {
    /// Whether the file is a user's, opened past damage the user was
    /// asked about: never saved to, so as not to cut off what can still be
    /// got out of it.
    pub(crate) fn kept_as_it_is(&self) -> bool {
        !self.entry && matches!(self.damage.kind, DamageKind::Damaged { .. })
    }

    /// Whether a banner says so: not for damage past the save opened,
    /// which the user was asked about before it showed.
    pub(crate) fn has_banner(&self) -> bool {
        !matches!(self.damage.kind, DamageKind::Damaged { .. })
    }

    /// How the banner shows it, with times relative to `now`.
    fn shown(&self, now: UnixSeconds) -> varde_view::DamagedFile {
        varde_view::DamagedFile {
            damage: shown_damage(&self.damage, now),
            auto_saves: self.entry,
        }
    }
}

/// How the view shows `damage`, with its time relative to `now`.
pub(crate) fn shown_damage(damage: &Damage, now: UnixSeconds) -> varde_view::Damage {
    let from = || when::ago_in_sentence(damage.time, now);
    match damage.kind {
        DamageKind::Bridged => varde_view::Damage::Bridged,
        DamageKind::NewestDamaged => varde_view::Damage::NewestDamaged { from: from() },
        DamageKind::Damaged { .. } => varde_view::Damage::Damaged { from: from() },
    }
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
            damage: None,
            download: None,
        }
    }
}

/// Where Home and the view cube turn the camera to look at with nothing
/// shown: the origin.
const HOME_TARGET: Vec3 = Vec3::ZERO;

/// Why a click on the model can't be taken while a newer one is on its
/// way, in the words the status bar shows.
const OUT_OF_DATE: &str = "The model shown is out of date: try again once it's regenerated";

/// How far from the origin, in millimetres, the camera looks with nothing
/// shown: about as far as a screen from the eye.
const HOME_DISTANCE: f32 = 1000.0;

/// The camera Home turns to, framed on the origin.
fn home_camera(projection: Projection) -> Camera {
    let mut camera = Camera::default();
    camera.set_projection(projection);
    camera.set_target(HOME_TARGET);
    camera.zoom(HOME_DISTANCE / camera.distance());
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
            damage,
            download,
        } = origin;
        // Opened from a file, or as a sample: anything with a model.
        let fit_on_model = !document.features().is_empty();
        let editor = Editor::new(document);
        let revision = editor.revision();
        let lineage = editor.lineage();
        let mut doc = Self {
            id: DocId::unique(),
            persist: Persist::new(
                target,
                creating,
                recovered,
                damage,
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
            notice: None,
            toast: None,
            renaming: None,
            rename_focus: false,
            placed: None,
            refused_edit: None,
            name,
            path: None,
            panel: Panel::default(),
            file_menu: false,
            view_menu: false,
            view_submenu: None,
            deleting: None,
            picking_plane: None,
            lineage,
            selected_feature: None,
            hovered_feature: None,
            errors: Arc::default(),
            row_menu: None,
            origin: varde_render::OriginShown::DEFAULT,
            origin_hover: None,
            plane_hover: None,
            objects_selected: Vec::new(),
            objects_folded: [varde_view::ObjectGroup::Origin].into(),
            overlaps: None,
            opacity_preview: None,
            sketch: None,
            extrude: None,
            revolve: None,
            combine: None,
            motion: None,
            pattern_shapes: HashMap::new(),
            measure: None,
            sketch_split: GEOMETRY_SHARE,
            focus: None,
            animation: None,
            fit_on_model,
            before_sketch: None,
            panel_before_sketch: None,
            before_show: None,
            pivot: None,
            cube_hovered: false,
            pick: ModelPick::default(),
            rail: Rail::default(),
            export: Export::default(),
            thumbnails: thumbnail::Thumbnails::default(),
        };
        doc.opened_download(download);
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
            self.forget_picks();
            // Its feature id may name another sketch now.
            self.placed = None;
        }
        self.prune_deleting();
        self.prune_renaming(replaced);
        self.prune_objects(replaced);
        self.prune(replaced);
        self.prune_plane_pick(replaced);
        self.prune_extrude(replaced);
        self.prune_revolve(replaced);
        self.prune_combine(replaced);
        self.prune_motion(replaced);
        self.prune_measure(replaced);
        self.request_analysis();
        self.refresh_profiles();
        self.prune_picks();
        // After the selection lets go of what's gone, as it's measured.
        self.request_model();
        self.refresh_errors();
        self.prune_preview();
        self.follow_placement();
        self.refresh_links();
    }

    /// Asks for the model if the document changed, the sketch left out of
    /// it (the one being edited) or the extrude, revolve or combine being
    /// set up did, which is previewed as a draft, or the measure tool's
    /// picks, or else what's selected (see [`Doc::selection_inspect`]),
    /// measured on it. Neither is measured with a draft, so no request
    /// carries both: a draft dragged never measures again at each step.
    fn request_model(&mut self) {
        let exclude = self.sketch.as_ref().map(|session| session.feature);
        let draft = (self.extrude_draft())
            .or_else(|| self.revolve_draft())
            .or_else(|| self.combine_draft())
            .or_else(|| self.motion_draft());
        let inspect = (self.measure.as_ref())
            .and_then(MeasureSession::inspect)
            .or_else(|| self.scale_inspect())
            .or_else(|| self.selection_inspect());
        self.feed
            .request_with(&self.editor, exclude, draft, inspect);
    }

    /// Whether an operation is being set up: an extrude, a revolve, a
    /// combine, a move or a mirror.
    pub(crate) fn operating(&self) -> bool {
        self.extrude.is_some()
            || self.revolve.is_some()
            || self.combine.is_some()
            || self.motion.is_some()
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
        // Anything else asked of the document renames first.
        if !matches!(message, Edit::CommitRename) {
            self.commit_rename();
        }
        self.end_refusal();
        self.notice = None;
        // Letting go of the Opacity slider leaves its menu open, to go on.
        if !matches!(message, Edit::CommitOpacity) {
            self.row_menu = None;
        }
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
        if matches!(message, Edit::Constrain(_) | Edit::ToggleConstraint(_)) {
            self.rail.close();
        }
        match message {
            Edit::ToggleFileMenu => self.file_menu = !self.file_menu,
            Edit::DismissSaveError => self.dismiss_save_error(),
            Edit::DismissExportError => self.dismiss_export_error(),
            Edit::DismissDamage => self.dismiss_damage(),
            Edit::DismissRefusedEdit => self.refused_edit = None,
            Edit::RemoveBody(id) => self.remove(Removable::Body(id)),
            Edit::ToggleVisible(id) => self.change(Change::ToggleVisible(id)),
            Edit::PlanePicked(plane) => self.plane_picked(Plane::Origin(plane)),
            Edit::FacePicked(face) => self.plane_picked(Plane::Face(face)),
            Edit::SketchOnSelection => {
                if let Some(face) = self.selected_face() {
                    self.change(Change::NewSketch(Plane::Face(face)));
                }
            }
            Edit::RemoveFeature(id) => self.remove(Removable::Feature(id)),
            Edit::RemoveObjects => {
                let sketches = self.selected_sketches().map(Removable::Feature).collect();
                self.remove_all(sketches);
            }
            Edit::ConfirmDelete => self.confirm_delete(),
            Edit::ToggleFeatureVisible(id) => self.change(Change::ToggleFeatureVisible(id)),
            Edit::ToolClick(click) => self.tool_click(click),
            Edit::DropGeometry => self.drop_geometry(),
            Edit::DeleteSelection => self.delete_selection(),
            Edit::DeleteItem(id) => self.delete_item(id),
            Edit::DetachPoint(id) => self.detach_point(id),
            Edit::ToggleConstruction => self.toggle_construction(),
            Edit::Constrain(kind) => self.constrain(kind),
            Edit::ToggleConstraint(kind) => self.toggle_constraint(kind),
            Edit::SubmitValue => self.submit_value(),
            Edit::DropLabel => self.drop_label(),
            Edit::ToggleReference => self.toggle_reference(),
            Edit::PlaceShape => self.place_shape(),
            Edit::ConvertSplines => self.convert_splines(),
            Edit::ToggleHandles => self.toggle_handles(),
            Edit::InsertSplinePoint { spline, at } => self.insert_spline_point(spline, at),
            Edit::CommitExtrude => self.commit_extrude(false),
            Edit::CommitRevolve => self.commit_revolve(false),
            Edit::CommitCombine => self.commit_combine(false),
            Edit::CommitMotion => self.commit_motion(false),
            Edit::AcceptError => {
                self.commit_extrude(true);
                self.commit_revolve(true);
                self.commit_combine(true);
                self.commit_motion(true);
            }
            Edit::CommitOpacity => {
                if let Some((id, opacity)) = self.opacity_preview.take() {
                    self.change(Change::SetOpacity(id, opacity));
                }
            }
            Edit::CommitRename => self.commit_rename(),
            Edit::SetUnits(units) => self.change(Change::SetUnits(units)),
            Edit::SetTolerance(tolerance) => self.change(Change::SetTolerance(tolerance)),
            // What waits on the solver, and what waits behind it, is newer
            // than anything committed: undo takes back the newest of it.
            Edit::RemoveLink(link) => self.remove_link(link),
            Edit::SetLinkProfiles(link, profiles) => self.set_link_profiles(link, profiles),
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
            Change::Remove { targets, confirmed } => self.remove_now(targets, confirmed),
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
            Change::SetPlane {
                feature,
                plane,
                placed,
                enter,
            } => self.set_sketch_plane(feature, plane, placed, enter),
            // Nothing if it's as it was: the editor adds no undo step.
            Change::SetOpacity(id, opacity) => self.apply(Command::SetOpacity(id, opacity)),
            Change::SetUnits(units) => self.apply(Command::SetUnits(units)),
            Change::SetTolerance(tolerance) => self.apply(Command::SetTolerance(tolerance)),
            Change::Rename(command) => self.apply(command),
        }
    }

    /// Takes `message`, which only changes how the document is looked at.
    /// An action in the sketch ends what the solver last refused showing.
    pub(crate) fn look(&mut self, message: Look) {
        let asked = self.delete_asked();
        if !self.rename_look(&message) {
            self.look_at(message);
        }
        // The delete prompt cancelled: what waits on the solver, held while
        // it was up, goes on.
        if asked && !self.delete_asked() && self.proposing() {
            self.sync();
        }
        self.list_selection();
        // A drag's step shows another sketch, and letting go of it the
        // sketch before.
        self.refresh_profiles();
        self.prune_picks();
        // The extrude being set up is previewed as it changes, and what's
        // selected measured.
        self.request_model();
        self.follow_motion_pivot();
        self.refresh_errors();
        self.prune_preview();
    }

    /// Takes `message`, see [`Doc::look`].
    fn look_at(&mut self, message: Look) {
        // The list of what overlaps where the button was held takes `Esc`
        // alone, and anything else done but hovering closes it.
        if self.overlaps.is_some() {
            if matches!(message, Look::Escape) {
                self.close_overlaps();
                return;
            }
            if !matches!(
                message,
                Look::OpenOverlaps(_)
                    | Look::HoverOverlap(_)
                    | Look::LeaveOverlap(_)
                    | Look::ChooseOverlap { .. }
                    | Look::ToggleOverlap(_)
                    | Look::Hover(_)
                    | Look::HoverSketch(_)
                    | Look::HoverItem(_)
                    | Look::HoverLink(_)
                    | Look::HoverFeature(_)
                    | Look::LeaveFeature(_)
                    | Look::HoverOrigin(_)
                    | Look::HoverPlane(_)
                    | Look::LeaveOrigin(_)
                    | Look::HoverBodyRow(_)
                    | Look::LeaveBodyRow(_)
                    | Look::HoverPanel(_)
                    | Look::LeavePanel(_)
                    | Look::HoverCube(_)
                    | Look::Snap(_)
                    | Look::Aim(_)
                    | Look::ScrollGeometry(_)
                    | Look::ScrollConstraints(_)
            ) {
                self.close_overlaps();
            }
        }
        if matches!(
            message,
            Look::Escape
                | Look::ClickGeometry { .. }
                | Look::ClickRow(_)
                | Look::ClickLink(_)
                | Look::SelectBox { .. }
                | Look::ClearSelection
                | Look::SelectTool(_)
                | Look::ToggleConstrain
                | Look::PutDownTool
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
                | Look::ClickLink(_)
                | Look::SelectBox { .. }
                | Look::SelectTool(_)
                | Look::ToggleConstrain
                | Look::PutDownTool
                | Look::PressLabel { .. }
                | Look::DragGeometry { .. }
                | Look::EditFeature(_)
                | Look::FinishSketch
                | Look::PickPlane
                | Look::ChangePlane(_)
        ) {
            self.close_value();
        }
        // What the app refused shows until something else is asked.
        if !matches!(
            message,
            Look::Hover(_)
                | Look::HoverSketch(_)
                | Look::HoverItem(_)
                | Look::HoverLink(_)
                | Look::HoverFeature(_)
                | Look::LeaveFeature(_)
                | Look::HoverOrigin(_)
                | Look::HoverPlane(_)
                | Look::LeaveOrigin(_)
                | Look::HoverBodyRow(_)
                | Look::LeaveBodyRow(_)
                | Look::HoverPanel(_)
                | Look::LeavePanel(_)
                | Look::HoverCube(_)
                | Look::Snap(_)
                | Look::Aim(_)
                | Look::Rail(_)
                | Look::ScrollGeometry(_)
                | Look::ScrollConstraints(_)
                | Look::Orbit { .. }
                | Look::Pan { .. }
                | Look::Zoom { .. }
                | Look::SetPivot(_)
        ) {
            self.notice = None;
        }
        // A row's context menu does its job or is left by anything else
        // done: `Esc` closes it alone.
        if !matches!(
            message,
            Look::OpenMenu(_)
                | Look::PreviewOpacity(..)
                | Look::Escape
                | Look::HoverItem(_)
                | Look::HoverLink(_)
                | Look::HoverFeature(_)
                | Look::LeaveFeature(_)
                | Look::HoverOrigin(_)
                | Look::HoverPlane(_)
                | Look::LeaveOrigin(_)
                | Look::HoverBodyRow(_)
                | Look::LeaveBodyRow(_)
                | Look::HoverPanel(_)
                | Look::LeavePanel(_)
                | Look::Hover(_)
                | Look::HoverSketch(_)
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
                | Look::ChangePlane(_)
                | Look::StartExtrude
                | Look::StartRevolve
                | Look::StartCombine
                | Look::StartMove
                | Look::StartMirror
                | Look::StartPattern
                | Look::StartCircularPattern
                | Look::StartAlign
                | Look::StartScale
                | Look::StartSplit
                | Look::StartChamfer
                | Look::StartFillet
                | Look::StartShell
                | Look::StartOffsetFace
                | Look::StartDraft
                | Look::StartSweep
                | Look::StartLoft
                | Look::StartMeasure
                | Look::EditFeature(_)
        ) {
            self.rail.close();
        }
        // Another tool leaves the measure tool.
        if matches!(
            message,
            Look::PickPlane
                | Look::ChangePlane(_)
                | Look::StartExtrude
                | Look::StartRevolve
                | Look::StartCombine
                | Look::StartMove
                | Look::StartMirror
                | Look::StartPattern
                | Look::StartCircularPattern
                | Look::StartAlign
                | Look::StartScale
                | Look::StartSplit
                | Look::StartChamfer
                | Look::StartFillet
                | Look::StartShell
                | Look::StartOffsetFace
                | Look::StartDraft
                | Look::StartSweep
                | Look::StartLoft
                | Look::EditFeature(_)
        ) {
            self.measure = None;
        }
        match message {
            Look::CloseFileMenu => self.file_menu = false,
            Look::ToggleViewMenu => {
                self.view_menu = !self.view_menu;
                self.view_submenu = None;
            }
            Look::ViewSubmenu(submenu) => self.view_submenu = submenu,
            Look::CloseViewMenu => self.view_menu = false,
            Look::CancelDelete => self.deleting = None,
            Look::Escape => self.escape(),
            // Only the tabs showing can be picked, but a message sent before
            // entering or leaving a sketch may come after.
            // The Timeline's rows go with it, sending no exit.
            Look::SelectPanel(panel) => {
                self.panel = panel.for_sketching(self.sketch.is_some());
                self.hovered_feature = None;
            }
            Look::PickPlane => self.pick_plane(),
            Look::ChangePlane(id) => self.change_plane(id),
            Look::EditFeature(id) => match self.editor.document().feature(id).map(|f| &f.kind) {
                Some(FeatureKind::Extrude(_)) => self.edit_extrude(id),
                Some(FeatureKind::Revolve(_)) => self.edit_revolve(id),
                Some(FeatureKind::Combine(_)) => self.edit_combine(id),
                Some(
                    FeatureKind::Move(_)
                    | FeatureKind::Mirror(_)
                    | FeatureKind::Pattern(_)
                    | FeatureKind::Align(_)
                    | FeatureKind::Scale(_)
                    | FeatureKind::Split(_)
                    | FeatureKind::Chamfer(_)
                    | FeatureKind::Shell(_)
                    | FeatureKind::Fillet(_)
                    | FeatureKind::OffsetFace(_)
                    | FeatureKind::FaceDraft(_)
                    | FeatureKind::Sweep(_)
                    | FeatureKind::Loft(_),
                ) => self.edit_motion(id),
                _ => self.enter_sketch(id),
            },
            Look::StartExtrude => self.start_extrude(),
            Look::Extrude(message) => self.extrude_look(message),
            Look::StartRevolve => self.start_revolve(),
            Look::Revolve(message) => self.revolve_look(message),
            Look::StartCombine => self.start_combine(),
            Look::Combine(message) => self.combine_look(message),
            Look::StartMove => self.start_motion(varde_view::MotionKind::Move),
            Look::StartMirror => self.start_motion(varde_view::MotionKind::Mirror),
            Look::StartPattern => self.start_motion(varde_view::MotionKind::LinearPattern),
            Look::StartCircularPattern => {
                self.start_motion(varde_view::MotionKind::CircularPattern)
            }
            Look::StartAlign => self.start_motion(varde_view::MotionKind::Align),
            Look::StartScale => self.start_motion(varde_view::MotionKind::Scale),
            Look::StartSplit => self.start_motion(varde_view::MotionKind::Split),
            Look::StartChamfer => self.start_motion(varde_view::MotionKind::Chamfer),
            Look::StartFillet => self.start_motion(varde_view::MotionKind::Fillet),
            Look::StartShell => self.start_motion(varde_view::MotionKind::Shell),
            Look::StartOffsetFace => self.start_motion(varde_view::MotionKind::OffsetFace),
            Look::StartDraft => self.start_motion(varde_view::MotionKind::Draft),
            Look::StartSweep => self.start_motion(varde_view::MotionKind::Sweep),
            Look::StartLoft => self.start_motion(varde_view::MotionKind::Loft),
            Look::Motion(message) => self.motion_look(message),
            Look::StartMeasure => self.start_measure(),
            Look::Measure(message) => self.measure_look(message),
            Look::FinishSketch => self.finish_sketch(),
            Look::SelectFeature(id) => {
                if self.editor.document().feature(id).is_some() {
                    self.selected_feature = Some(id);
                    self.clear_model_selection();
                }
            }
            Look::OpenMenu(menu) => self.open_menu(menu),
            Look::CloseMenu => {}
            Look::PreviewOpacity(id, opacity) => self.preview_opacity(id, opacity),
            Look::ClickGeometry { hit, add } => self.click_geometry(hit, add),
            // The app turns this into a `ClickGeometry`, knowing the keys
            // held; alone, it selects.
            Look::ClickRow(id) => self.click_geometry(Some(id), false),
            Look::HoverItem(id) => self.hover_item(id),
            Look::ClickLink(link) => self.click_link(link),
            Look::HoverLink(link) => self.hover_link(link),
            Look::HoverFeature(feature) => self.hovered_feature = feature,
            Look::LeaveFeature(feature) => {
                (self.hovered_feature).take_if(|&mut hovered| hovered == feature);
            }
            Look::ShowFailure => self.show_failure(),
            Look::BackFromFailure => self.back_from_failure(),
            Look::HoverPanel(hover) => self.hover_panel(hover),
            Look::LeavePanel(left) => self.leave_panel(left),
            // While the list is open, its row hovered is.
            Look::Hover(_) if self.overlaps.is_some() => {}
            Look::Hover(pick) => {
                self.plane_hover = None;
                self.hover(pick);
            }
            Look::HoverOrigin(object) => self.origin_hover = object,
            Look::HoverPlane(plane) => {
                // In the model's place.
                self.plane_hover = Some(plane);
                self.hover(None);
            }
            Look::LeaveOrigin(object) => {
                self.origin_hover.take_if(|&mut hovered| hovered == object);
            }
            Look::HoverBodyRow(body) => {
                self.pick.row_hover = body;
                self.refresh_highlight();
            }
            Look::LeaveBodyRow(body) => {
                if self.pick.row_hover == Some(body) {
                    self.pick.row_hover = None;
                    self.refresh_highlight();
                }
            }
            Look::HoverSketch(_) if self.overlaps.is_some() => {}
            Look::HoverSketch(item) => self.hover_sketch(item),
            Look::ClickSketch { item, .. } if self.picks_outside() => {
                self.outside_click(sketch::OutsideClick::Sketch(item));
            }
            Look::ClickSketch { item, add } => self.click_sketch(item, add),
            Look::OpenOverlaps(list) => self.open_overlaps(list),
            Look::HoverOverlap(row) => self.hover_overlap(row),
            Look::LeaveOverlap(row) => self.leave_overlap(row),
            Look::ChooseOverlap { index, add } => self.choose_overlap(index, add),
            Look::ToggleOverlap(index) => self.choose_overlap(index, true),
            Look::CloseOverlaps => self.close_overlaps(),
            Look::ClickModel { pick, .. } if self.picks_outside() => {
                self.outside_click(sketch::OutsideClick::Model(pick));
            }
            Look::ClickModel { pick, .. } if self.combine.is_some() => self.combine_click(pick),
            Look::ClickModel { pick, .. } if self.motion.is_some() => self.motion_click(pick),
            Look::ClickModel { pick, add, double } if self.measure.is_some() => {
                self.measure_click(pick, add, double);
            }
            Look::ClickModel { pick, add, double } => self.click_model(pick, add, double),
            Look::ClickBody { body, .. } if self.combine.is_some() => self.combine_body(body),
            Look::ClickBody { body, .. } if self.motion.is_some() => self.motion_body(body),
            Look::ClickBody { body, add } if self.measure.is_some() => self.measure_body(body, add),
            Look::ClickBody { body, add } => self.click_body(body, add),
            Look::ClickObject { row, add } => self.click_object(row, add),
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
            Look::PutDownTool => self.put_down_tool(),
            Look::ToggleGroup(group) => self.toggle_group(group),
            Look::ToggleObjectGroup(group) => {
                if !self.objects_folded.remove(&group) {
                    self.objects_folded.insert(group);
                }
            }
            Look::ToggleOrigin(object) => {
                let shown = object.shown(&mut self.origin);
                *shown = !*shown;
            }
            Look::ToggleExpanded(id) => self.toggle_expanded(id),
            Look::SelectBox { ids, add } => self.select_box(ids, add),
            // While measuring, the selection is hidden, and kept for
            // after: Space doesn't clear what can't be seen.
            Look::ClearSelection => {
                if self.measure.is_none() {
                    self.clear_selection();
                }
            }
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
            // Taken by `Doc::rename_look`.
            Look::StartRename(_) | Look::RenameInput(_) | Look::CancelRename => {}
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
            // In a sketch, Home faces it; outside, it frames the model.
            // Either way it orbits its target again.
            Look::ResetCamera => {
                self.pivot = None;
                let home = self.sketch_camera().unwrap_or_else(|| self.home_view());
                self.animate_camera(home);
            }
            // A view cube face looks from its side, the zoom kept, at
            // what Home looks at: the sketch's middle in a sketch, the
            // model's outside one.
            Look::LookFrom(view) => {
                let mut to = self.camera;
                to.look_from(view);
                let home = self.sketch_camera().unwrap_or_else(|| self.home_view());
                to.set_target(home.target());
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
            // A link's row shows in a sketch alone.
            RowMenu::Link(id) => {
                let links = self
                    .sketch
                    .as_ref()
                    .map_or(&[][..], |session| &session.links[..]);
                links.iter().any(|row| row.link == id)
            }
            // So does a point's or curve's.
            RowMenu::Item(id) => self.holds(id),
        };
        let in_sketch = matches!(menu, RowMenu::Link(_) | RowMenu::Item(_));
        if self.sketch.is_some() != in_sketch || !exists {
            return;
        }
        if let RowMenu::Feature(id) = menu {
            self.selected_feature = Some(id);
            self.clear_model_selection();
        }
        self.row_menu = Some(menu);
    }

    /// Previews `opacity` for the body `id` while its context menu's
    /// slider is dragged, if that menu is open and the document editable;
    /// [`Edit::CommitOpacity`] commits it on letting go, and
    /// [`Doc::prune_preview`] drops it if the menu closes first.
    fn preview_opacity(&mut self, id: BodyId, opacity: Opacity) {
        if self.editable() && self.row_menu == Some(RowMenu::Body(id)) {
            self.opacity_preview = Some((id, opacity));
        }
    }

    /// Drops the opacity previewed unless its body's context menu, and so
    /// the slider, is still open.
    fn prune_preview(&mut self) {
        let menu = self.row_menu;
        (self.opacity_preview).take_if(|(id, _)| menu != Some(RowMenu::Body(*id)));
    }

    /// Starts sending requests to `lane`, the document's regeneration
    /// lane.
    pub(crate) fn lane_ready(&mut self, lane: varde_regen::lane::Lane) {
        self.feed.connect(lane);
        self.regen_replaced();
        self.sync();
    }

    /// Shows `response`, computed for the document. An export's answer
    /// goes to [`Doc::export_welded`] instead.
    pub(crate) fn computed(&mut self, response: varde_regen::Response) {
        self.feed.apply(response);
        self.relink();
        self.fit_first_model();
        // Which bodies are merged, which faces show as whose, may change.
        self.prune_plane_pick(false);
        self.prune_picks();
        // The combine's and the motion's bodies may have moved on, and
        // what's selected be found otherwise: asked again only if so.
        self.follow_merges();
        self.follow_motion_merges();
        self.request_model();
        self.follow_motion_pivot();
        self.follow_edge_axis();
        self.refresh_errors();
        self.follow_placement();
        self.refresh_links();
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
    /// the user is asked about unsaved changes or deleting, or drags the
    /// Opacity slider: no key changes the document behind the prompt, or
    /// before the opacity is committed.
    pub(crate) fn keys(&self) -> Option<DocumentKeys> {
        let dragging = self.opacity_preview.is_some();
        (self.dialog().is_none() && !dragging).then(|| {
            DocumentKeys::new(self.editable(), self.selected_feature, self.sketch_state())
                .with_face_selected(self.selected_face().is_some())
                .with_extrude(self.extrude_state().as_ref())
                .with_revolve(self.revolve_state().as_ref())
                .with_combine(self.combinable(), self.combine_state().as_ref())
                .with_motion(
                    !self.editor.document().bodies().is_empty(),
                    self.motion_state().as_ref(),
                )
                .with_measure(self.measure.is_some())
                .with_rail(self.rail.state())
                .with_rename(self.rename_target())
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
        } else if self.naming().is_some() {
            Some(Dialog::Naming)
        } else if self.delete_prompt().is_some() {
            Some(Dialog::Delete)
        } else {
            None
        }
    }

    /// Whether the other panel tab shows with the peek key `held`: not in
    /// the Dimension tool, where it's held to place references, nor while
    /// the Opacity slider is dragged, which would go with its tab and so
    /// never see its release.
    pub(crate) fn peeks(&self, held: bool) -> bool {
        held && !self.dimensioning() && self.opacity_preview.is_none()
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
            ForDoc::Computed(varde_regen::Response::Exported { export, result }) => {
                self.export_welded(cx, export, result);
                Next::Stay
            }
            // Only news: nothing shown changes but the progress.
            ForDoc::Computed(response @ varde_regen::Response::Progress(_)) => {
                self.feed.apply(response);
                Next::Stay
            }
            ForDoc::Computed(response) => {
                self.computed(response);
                Next::Stay
            }
            ForDoc::ExportPicked(chosen) => {
                self.export_picked(chosen);
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
            ForDoc::Thumbnail(tag, image) => self.thumbnail_rendered(cx, tag, image),
        }
    }

    /// The document screen, showing the other panel tab if `peek`, unless
    /// the Dimension tool is in use, where the peek key places references,
    /// with the view `options`, and what the platform `offers`.
    pub(crate) fn view(
        &self,
        peek: bool,
        mode: Mode,
        options: ViewOptions,
        offers: Offers,
    ) -> Element<'_, Ui> {
        varde_view::document(self.state(peek, mode, options, offers))
    }

    /// What the document screen is built from, see [`Doc::view`].
    pub(crate) fn state(
        &self,
        peek: bool,
        mode: Mode,
        options: ViewOptions,
        offers: Offers,
    ) -> varde_view::DocumentState<'_> {
        let peek = self.peeks(peek);
        let now = when::now();
        varde_view::DocumentState {
            editor: &self.editor,
            camera: &self.camera,
            pivot: self.pivot_marker(),
            mesh: self.feed.mesh(),
            parts: self.feed.parts(),
            opacity_preview: self.opacity_preview,
            renaming: (self.renaming.as_ref())
                .map(|renaming| (renaming.target, renaming.text.as_str())),
            sketches: self.feed.sketches(),
            mesh_status: self.feed.status(&self.editor),
            regenerating: self.feed.slow(&self.editor),
            picking: self.model_picking(),
            highlight: self.highlight(),
            hover_through: self.hovers_through(),
            errors: self.shown_errors(),
            model_selection: &self.pick.selection,
            selection_measured: self.selection_measured(),
            name: &self.name,
            unnamed: self.unnamed(),
            path: self.path.as_deref(),
            location: self.persist_target().location(),
            downloads: self.download_status(),
            downloadable: offers.download,
            rename: matches!(self.persist_target(), Target::Browser { .. })
                .then(|| self.renamable()),
            naming: self.naming().map(|naming| varde_view::NamePrompt {
                name: &naming.name,
                place: naming.place,
                places: offers.file_system_access,
                rename: naming.rename,
                taken: naming.replacing.as_deref(),
            }),
            edited: self.edited(),
            read_only: self.read_only.as_deref(),
            edit_error: self.edit_error.as_ref(),
            notice: self.notice.as_deref(),
            refused_edit: self.refused_edit(),
            saving: self.saving(),
            save_error: self.banner_error(),
            recovered: self.recovered_changes(now),
            damage: self.damage().map(|damage| damage.shown(now)),
            exportable: self.exportable(),
            exporting: self.exporting(),
            export_error: self.export_error(),
            // The prompt shows over the menus.
            overlay: self
                .prompt()
                .map(|_| Overlay::UnsavedPrompt)
                .or(self.naming().map(|_| Overlay::NamePrompt))
                .or(self.file_menu.then_some(Overlay::FileMenu))
                .or(self
                    .view_menu
                    .then_some(Overlay::ViewMenu(self.view_submenu))),
            panel: self.panel,
            peek,
            mode,
            options,
            picking_plane: self.picking_plane.as_ref().map(|picking| &picking.pick),
            thumbnail: self.thumbnail_request(),
            selected_feature: self.selected_feature,
            row_menu: self.row_menu,
            origin: self.origin,
            objects_selected: &self.objects_selected,
            origin_hover: (self.hovered_plane().map(varde_view::OriginObject::Plane))
                .or(self.origin_hover),
            objects_folded: &self.objects_folded,
            overlaps: self.overlaps.as_ref().map(|listed| &listed.list),
            overlap_ticks: self.overlap_ticks(),
            sketch: self.sketch_state(),
            extrude: self.extrude_state(),
            revolve: self.revolve_state(),
            combine: self.combine_state(),
            combinable: self.combinable(),
            motion: self.motion_state(),
            measure: self.measure_state(),
            unsolved: self.feed.unsolved(),
            failed: self.feed.failed_features(),
            merged: self.feed.merged_bodies(),
            deleting: self.delete_prompt(),
            proposing: self.proposing(),
            rail: self.rail.state(),
        }
    }

    /// The document screen in `mode` as the app first shows it: not
    /// peeking, the view options at their defaults.
    #[cfg(test)]
    pub(crate) fn view_in(&self, mode: Mode) -> Element<'_, Ui> {
        self.view(false, mode, ViewOptions::default(), Offers::default())
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
        targets: Vec<Removable>,
        confirmed: Option<Removal>,
    },
    ToggleVisible(BodyId),
    ToggleFeatureVisible(FeatureId),
    SetOpacity(BodyId, Opacity),
    NewSketch(Plane),
    /// Puts the sketch `feature` on `plane`, where it's `placed` if
    /// that's a face (as worked out where it was picked), and edits it if
    /// `enter`.
    SetPlane {
        feature: FeatureId,
        plane: Plane,
        placed: Option<varde_document::Placement>,
        enter: bool,
    },
    SetUnits(LengthUnit),
    SetTolerance(Tolerance),
    /// Renames a feature, sketch or body: [`Command::Rename`].
    Rename(Command),
}

/// A prompt over a screen, see [`Doc::dialog`] and
/// [`Welcome::dialog`](crate::welcome::Welcome::dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Dialog {
    /// About unsaved changes, before the document is closed.
    Unsaved,
    /// About deleting more than was asked for.
    Delete,
    /// About a damaged file, before its design is shown.
    Damaged,
    /// The whole of the panic recorded, on the welcome screen.
    Panic,
    /// The app's own Save As dialog, on the web.
    Naming,
    /// About deleting a design in browser storage not downloaded as it
    /// is, on the welcome screen.
    DeleteFromBrowser,
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
