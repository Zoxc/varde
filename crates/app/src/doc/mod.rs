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
pub(crate) mod params;
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
    Plane, Removable, Removal, Revision, Tint, Tolerance,
};
use varde_io::{Access, Damage, DamageKind, LastDownload, Offer, OpenId, UnixSeconds};
use varde_render::{Camera, Projection};
use varde_solve::{Request as SolveRequest, Transport};
use varde_view::{
    DocumentKeys, Edit, Look, Message as Ui, Mode, OperationKeys, OperationState, Overlay, Panel,
    PanelHover, RowMenu, Snap, ViewOptions,
};

#[cfg(test)]
pub(crate) use camera::CAMERA_ANIMATION;
pub(crate) use camera::CameraAnimation;
use camera::Fade;
use camera::Pivot;
#[cfg(test)]
pub(crate) use camera::{PIVOT_FADE, PIVOT_SHOWN};
pub(crate) use combine::CombineSession;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use combine::pickable;
use delete::Deleting;
use export::Export;
#[cfg(test)]
pub(crate) use export::Exporting;
pub(crate) use extrude::ExtrudeSession;
use feed::MeshFeed;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use feed::{Marks, Merges, Shown, merged_in};
pub(crate) use measure::MeasureSession;
pub(crate) use motion::MotionSession;
use pick::ModelPick;
use rail::Rail;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use regions::{BodyTargets, RegionPick};
pub(crate) use revolve::RevolveSession;
use save::Persist;
pub(crate) use save::{AUTO_SAVE_WAIT, Downloader, Leave, Target};
#[cfg(test)]
pub(crate) use save::{AutoSave, Picking};
use sketch::GEOMETRY_SHARE;
#[cfg(test)]
pub(crate) use sketch::Refusal;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use sketch::{
    Clicks, Settled, Step, clicked, face_placement_in, outside_model_in, outside_sketch_item_at,
    outside_target_in,
};
pub(crate) use sketch::{Focus, Proposals, SketchSession};

use crate::files::{Files, Offers};
use crate::{ForDoc, Next, when};

/// An open document and the state of its view.
pub(crate) struct Doc {
    /// Tags the regeneration lane and its responses.
    pub(crate) id: DocId,
    pub(crate) editor: Editor,
    pub(crate) camera: Camera,
    /// The viewport's width over its height, once it has told
    /// ([`Look::ViewAspect`]): which bodies are in view, for their detail.
    pub(crate) aspect: Option<f32>,
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
    pub(crate) toast: Option<String>,
    /// The MCP calls waiting on this design (`mcp/waiting.rs`). Natively
    /// only.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) mcp: crate::mcp::waiting::Waiting,
    /// The rename field, if it's open on a feature, sketch or body.
    pub(crate) renaming: Option<rename::Renaming>,
    /// Whether the rename field is to take the focus, as it just opened.
    rename_focus: bool,
    /// The parameters' popup.
    pub(crate) params: params::Params,
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
    /// What owns the document's view: a plane being picked, a sketch
    /// edited, an operation set up, the measure tool, or none of them.
    /// One at a time, so entering one leaves another.
    pub(crate) mode: DocMode,
    /// The editor's [`lineage`](varde_document::Editor::lineage) as of
    /// the last [`Doc::sync`]: the ids held across edits (the feature
    /// selected, the sessions') name things in it, see [`Doc::prune`] and
    /// [`Doc::prune_extrude`].
    lineage: Revision,
    /// The feature selected in the Timeline, if any.
    pub(crate) selected_feature: Option<FeatureId>,
    /// The feature whose row in the Timeline the cursor is over, if any.
    pub(crate) hovered_feature: Option<FeatureId>,
    /// Where the Timeline's rollback marker is being dragged to, the
    /// feature it's before or `None` at the end, shown until it's dropped
    /// and the document rolled there ([`Look::DragRollback`]).
    pub(crate) rolling: Option<Option<FeatureId>>,
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
    /// A body's look previewed from its context menu's sliders.
    pub(crate) body_look: BodyLook,
    /// The feature the extrude, revolve, combine or motion session open
    /// built as it opened ([`Doc::operation_built`]), none if it wasn't
    /// whole: it has nothing unfinished while it builds the same. Kept,
    /// not rebuilt, as a new extrude's distance depends on the camera.
    opened_with: Option<FeatureKind>,
    /// Whether the command modifier (`Ctrl`, `Cmd` on macOS) is held:
    /// a fillet or chamfer picking its edges then shows the model as of
    /// the feature rather than its preview ([`Doc::blend_before`]), another
    /// operation the model before it ([`Doc::operation_before`]).
    pub(crate) command_held: bool,
    /// How each pattern committed in this run was set up (its mode, Flip
    /// and spread as typed), which its stored values can't always tell:
    /// taken again on editing it while it still gives them, see
    /// [`motion::PatternShape`].
    pub(crate) pattern_shapes: HashMap<FeatureId, motion::PatternShape>,
    /// The share of the Sketch tab's height the Geometry list takes, kept
    /// from one sketch to the next.
    pub(crate) sketch_split: f32,
    /// What the value field is to do once it shows, which the app asks of
    /// it: see [`Doc::take_focus`].
    focus: Option<Focus>,
    pub(crate) animation: Option<CameraAnimation>,
    /// How faded the model is, turning with the camera into a sketch
    /// and out of it.
    fade: Fade,
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
    /// How dragging turns the camera, as the settings say.
    rotation: varde_render::Rotation,
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

/// A body's look previewed from its context menu's Opacity and Colour
/// sliders, only while that menu is open.
#[derive(Debug, Default)]
pub(crate) struct BodyLook {
    /// A body's opacity previewed while the slider is dragged
    /// ([`Doc::preview_opacity`]).
    pub(crate) opacity: Option<(BodyId, Opacity)>,
    /// A body's colour previewed while the Hue or Saturation slider is
    /// dragged ([`Doc::preview_color`]).
    pub(crate) color: Option<(BodyId, Tint)>,
    /// Whether the cursor is over the Opacity and Colour part of the
    /// menu: the viewport leaves the selection out meanwhile.
    pub(crate) hovered: bool,
}

impl BodyLook {
    /// Whether a slider is being dragged: an opacity or colour is
    /// previewed.
    pub(crate) fn sliding(&self) -> bool {
        self.opacity.is_some() || self.color.is_some()
    }

    /// Drops what's previewed.
    fn clear(&mut self) {
        self.opacity = None;
        self.color = None;
    }

    /// Drops what's previewed unless `menu`, the row menu open, is its
    /// body's, and the hover unless it's a body's.
    fn prune(&mut self, menu: Option<RowMenu>) {
        if !matches!(menu, Some(RowMenu::Body(_))) {
            self.hovered = false;
        }
        (self.opacity).take_if(|(id, _)| menu != Some(RowMenu::Body(*id)));
        (self.color).take_if(|(id, _)| menu != Some(RowMenu::Body(*id)));
    }
}

/// The operation being set up: one at a time, so opening one replaces
/// another.
#[expect(
    clippy::large_enum_variant,
    reason = "one per open document, where all four sessions were held before"
)]
pub(crate) enum OperationSession {
    Extrude(ExtrudeSession),
    Revolve(RevolveSession),
    Combine(CombineSession),
    /// A move, mirror or pattern, or another motion kind.
    Motion(MotionSession),
}

impl OperationSession {
    /// The feature it edits, none for a new one.
    fn feature(&self) -> Option<FeatureId> {
        match self {
            Self::Extrude(session) => session.feature,
            Self::Revolve(session) => session.feature,
            Self::Combine(session) => session.feature,
            Self::Motion(session) => session.feature,
        }
    }

    /// The row of its panel the cursor is over, see [`Doc::panel_hover`].
    fn hover(&self) -> Option<PanelHover> {
        match self {
            Self::Extrude(session) => session.hover,
            Self::Revolve(session) => session.hover,
            Self::Combine(session) => session.hover,
            Self::Motion(session) => session.hover,
        }
    }

    fn hover_mut(&mut self) -> &mut Option<PanelHover> {
        match self {
            Self::Extrude(session) => &mut session.hover,
            Self::Revolve(session) => &mut session.hover,
            Self::Combine(session) => &mut session.hover,
            Self::Motion(session) => &mut session.hover,
        }
    }
}

/// What owns a document's view, see [`Doc::mode`].
#[derive(Default)]
#[expect(
    clippy::large_enum_variant,
    reason = "one per open document, where each mode's state was held before"
)]
pub(crate) enum DocMode {
    /// None of the others: the model, its selection.
    #[default]
    Idle,
    /// The plane being picked: for a new sketch, or for the sketch whose
    /// plane is changed.
    PickingPlane(sketch::PickingPlane),
    /// The sketch being edited.
    Sketch(SketchSession),
    /// The extrude, revolve, combine or motion kind being set up.
    Operation(OperationSession),
    /// The measure tool.
    Measure(MeasureSession),
}

/// `$get` and `$get_mut` read the mode's `$variant` if it's the one in,
/// `$take` leaves it, handing it back, and leaves any other as it is.
macro_rules! mode_accessors {
    ($($variant:ident: $state:ty => $get:ident, $get_mut:ident, $take:ident;)*) => {
        $(
            pub(crate) fn $get(&self) -> Option<&$state> {
                match self {
                    Self::$variant(state) => Some(state),
                    _ => None,
                }
            }

            pub(crate) fn $get_mut(&mut self) -> Option<&mut $state> {
                match self {
                    Self::$variant(state) => Some(state),
                    _ => None,
                }
            }

            pub(crate) fn $take(&mut self) -> Option<$state> {
                match std::mem::take(self) {
                    Self::$variant(state) => Some(state),
                    other => {
                        *self = other;
                        None
                    }
                }
            }
        )*
    };
}

impl DocMode {
    mode_accessors! {
        PickingPlane: sketch::PickingPlane => picking_plane, picking_plane_mut, take_picking_plane;
        Sketch: SketchSession => sketch, sketch_mut, take_sketch;
        Measure: MeasureSession => measure, measure_mut, take_measure;
    }

    /// The operation being set up, if it's the mode.
    pub(crate) fn operation(&self) -> Option<&OperationSession> {
        match self {
            Self::Operation(session) => Some(session),
            _ => None,
        }
    }

    pub(crate) fn operation_mut(&mut self) -> Option<&mut OperationSession> {
        match self {
            Self::Operation(session) => Some(session),
            _ => None,
        }
    }

    /// Closes the operation being set up, if it's the mode, handing it
    /// back.
    pub(crate) fn take_operation(&mut self) -> Option<OperationSession> {
        match std::mem::take(self) {
            Self::Operation(session) => Some(session),
            other => {
                *self = other;
                None
            }
        }
    }
}

/// Each kind's session in the operation slot, if it's the one being set
/// up: `$kind` and `$kind_mut` read it, `take_$kind` closes it, handing
/// it back. On the field rather than [`Doc`], so it borrows the slot
/// alone.
pub(crate) trait OperationSlot {
    fn extrude(&self) -> Option<&ExtrudeSession>;
    fn extrude_mut(&mut self) -> Option<&mut ExtrudeSession>;
    fn take_extrude(&mut self) -> Option<ExtrudeSession>;
    fn revolve(&self) -> Option<&RevolveSession>;
    fn revolve_mut(&mut self) -> Option<&mut RevolveSession>;
    fn take_revolve(&mut self) -> Option<RevolveSession>;
    fn combine(&self) -> Option<&CombineSession>;
    fn combine_mut(&mut self) -> Option<&mut CombineSession>;
    fn take_combine(&mut self) -> Option<CombineSession>;
    fn motion(&self) -> Option<&MotionSession>;
    fn motion_mut(&mut self) -> Option<&mut MotionSession>;
    fn take_motion(&mut self) -> Option<MotionSession>;
}

macro_rules! slot_accessors {
    ($($variant:ident: $session:ty => $get:ident, $get_mut:ident, $take:ident;)*) => {
        $(
            fn $get(&self) -> Option<&$session> {
                match self {
                    DocMode::Operation(OperationSession::$variant(session)) => Some(session),
                    _ => None,
                }
            }

            fn $get_mut(&mut self) -> Option<&mut $session> {
                match self {
                    DocMode::Operation(OperationSession::$variant(session)) => Some(session),
                    _ => None,
                }
            }

            fn $take(&mut self) -> Option<$session> {
                match std::mem::take(self) {
                    DocMode::Operation(OperationSession::$variant(session)) => Some(session),
                    other => {
                        *self = other;
                        None
                    }
                }
            }
        )*
    };
}

impl OperationSlot for DocMode {
    slot_accessors! {
        Extrude: ExtrudeSession => extrude, extrude_mut, take_extrude;
        Revolve: RevolveSession => revolve, revolve_mut, take_revolve;
        Combine: CombineSession => combine, combine_mut, take_combine;
        Motion: MotionSession => motion, motion_mut, take_motion;
    }
}

impl DocId {
    pub(crate) fn unique() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// Its number, for the MCP keys, which carry it to tell a key from
    /// another design.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn number(self) -> u64 {
        self.0
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
    /// Unsaved changes, offered to be restored: boxed, as they hold a
    /// whole document.
    Offered(Box<Offer>),
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

/// The sketch of the sketch feature `id` of `document`, if it's one.
pub(crate) fn sketch_of(document: &Document, id: FeatureId) -> Option<&varde_document::Sketch> {
    match &document.feature(id)?.kind {
        FeatureKind::Sketch { sketch, .. } => Some(sketch),
        _ => None,
    }
}

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
            aspect: None,
            feed: MeshFeed::new(),
            solver: None,
            proposals: Proposals::default(),
            read_only: read_only(access),
            edit_error: None,
            notice: None,
            toast: None,
            #[cfg(not(target_arch = "wasm32"))]
            mcp: Default::default(),
            renaming: None,
            rename_focus: false,
            params: params::Params::default(),
            placed: None,
            refused_edit: None,
            name,
            path: None,
            panel: Panel::default(),
            file_menu: false,
            view_menu: false,
            view_submenu: None,
            deleting: None,
            mode: DocMode::Idle,
            lineage,
            selected_feature: None,
            hovered_feature: None,
            rolling: None,
            errors: Arc::default(),
            row_menu: None,
            origin: varde_render::OriginShown::DEFAULT,
            origin_hover: None,
            plane_hover: None,
            objects_selected: Vec::new(),
            objects_folded: [varde_view::ObjectGroup::Origin].into(),
            overlaps: None,
            body_look: BodyLook::default(),
            opened_with: None,
            command_held: false,
            pattern_shapes: HashMap::new(),
            sketch_split: GEOMETRY_SHARE,
            focus: None,
            animation: None,
            fade: Fade::default(),
            fit_on_model,
            before_sketch: None,
            panel_before_sketch: None,
            before_show: None,
            pivot: None,
            rotation: varde_render::Rotation::default(),
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
        self.refresh_param_uses();
    }

    /// Asks for the model if the document changed, the sketch left out of
    /// it (the one being edited) or the extrude, revolve or combine being
    /// set up did, which is previewed as a draft, or the measure tool's
    /// picks, or else what's selected (see [`Doc::selection_inspect`]),
    /// measured on it. Neither is measured with a draft, so no request
    /// carries both: a draft dragged never measures again at each step.
    fn request_model(&mut self) {
        let exclude = self.mode.sketch().map(|session| session.feature);
        // Held, the command modifier shows the model before the
        // operation, what its preview covers there to see.
        let before = self.operation_before();
        let draft = self.operation_draft();
        // The draft's still known, its error and OK waiting on it kept,
        // but not applied.
        self.feed.hide_draft = before;
        let inspect = (self.mode.measure())
            .and_then(MeasureSession::inspect)
            .or_else(|| self.scale_inspect())
            .or_else(|| self.selection_inspect());
        // Drawn as finely as the camera shows it, where an animation
        // ends rather than at each of its steps.
        let camera = self.animation.as_ref().map_or(&self.camera, |a| &a.to);
        self.feed.view(camera, self.aspect);
        let until = (before.then(|| self.opened_feature()).flatten()).or_else(|| self.rollback().0);
        self.feed
            .request_with(&self.editor, exclude, until, draft, inspect);
    }

    /// The feature being edited, if one is: the sketch entered, or the
    /// operation set up from a feature rather than a new one.
    pub(crate) fn opened_feature(&self) -> Option<FeatureId> {
        (self.mode.sketch().map(|session| session.feature))
            .or_else(|| self.mode.operation()?.feature())
    }

    /// The feature the model is rolled back to before, if it is, and
    /// whether that's only while a feature is edited: one opened shows
    /// the history up to it, those after it left out; a new operation
    /// being set up, which goes in where the document's marker is, the
    /// history up to there; otherwise the marker where it's dragged or
    /// where the document has it.
    pub(crate) fn rollback(&self) -> (Option<FeatureId>, bool) {
        let document = self.editor.document();
        if let Some(opened) = self.opened_feature() {
            let after = (document.feature_index(opened))
                .and_then(|at| document.features().get(at + 1))
                .map(|feature| feature.id);
            return (after, true);
        }
        if self.operating() {
            return (document.rollback(), true);
        }
        let until = self.rolling.unwrap_or_else(|| document.rollback());
        (until, false)
    }

    /// Whether the model shown is the one before the operation being set
    /// up rather than its preview: while the command modifier is held, so
    /// what it covers shows (an extrude's profile, a move's bodies where
    /// they were). A new one shows the document as it is, an edited one
    /// the history up to it. A blend picking its edges has its own
    /// ([`Doc::blend_before`]).
    pub(crate) fn operation_before(&self) -> bool {
        self.command_held && self.operating() && !self.blend_before()
    }

    /// Whether an operation is being set up: an extrude, a revolve, a
    /// combine, a move or a mirror.
    pub(crate) fn operating(&self) -> bool {
        self.mode.operation().is_some()
    }

    /// Closes the operation being set up, if one is, leaving the
    /// document as it was: its Cancel, or `Esc`.
    pub(crate) fn close_operation(&mut self) {
        self.mode.take_operation();
    }

    /// Commits the operation being set up, if one is, as its OK would;
    /// its preview failed only if `accept` (see `commit_extrude`).
    /// Refused, it stays open, and why shows.
    pub(crate) fn commit_operation(&mut self, accept: bool) {
        match self.mode.operation() {
            Some(OperationSession::Extrude(_)) => self.commit_extrude(accept),
            Some(OperationSession::Revolve(_)) => self.commit_revolve(accept),
            Some(OperationSession::Combine(_)) => self.commit_combine(accept),
            Some(OperationSession::Motion(_)) => self.commit_motion(accept),
            None => {}
        }
    }

    /// Keeps what the operation just opened builds, to tell later whether
    /// it has anything unfinished: called by each opener once its session
    /// is set up, and only then, so that an opener refused leaves the
    /// session open as it was, with its baseline.
    pub(crate) fn opened_operation(&mut self) {
        self.opened_with = self.operation_built();
    }

    /// The feature the operation being set up would commit, if one is
    /// and it's whole.
    pub(crate) fn operation_built(&self) -> Option<FeatureKind> {
        if self.mode.motion().is_some() {
            return self.motion_built();
        }
        self.operation_draft().map(|(_, kind)| kind)
    }

    /// The operation being set up as the regeneration lane previews it,
    /// with the feature it edits: none if there's none, or it isn't whole.
    fn operation_draft(&self) -> Option<(Option<FeatureId>, FeatureKind)> {
        match self.mode.operation()? {
            OperationSession::Extrude(_) => self.extrude_draft(),
            OperationSession::Revolve(_) => self.revolve_draft(),
            OperationSession::Combine(_) => self.combine_draft(),
            OperationSession::Motion(_) => self.motion_draft(),
        }
    }

    /// The operation being set up, or else the measure tool, for the view.
    fn operation_state(&self) -> Option<OperationState<'_>> {
        match self.mode.operation() {
            Some(OperationSession::Extrude(_)) => self.extrude_state().map(OperationState::Extrude),
            Some(OperationSession::Revolve(_)) => self.revolve_state().map(OperationState::Revolve),
            Some(OperationSession::Combine(_)) => self.combine_state().map(OperationState::Combine),
            Some(OperationSession::Motion(_)) => self.motion_state().map(OperationState::Motion),
            None => self.measure_state().map(OperationState::Measure),
        }
    }

    /// Whether the operation being set up builds another feature than it
    /// did as it opened: it has something unfinished.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "on the web, MCP edits are none")
    )]
    pub(crate) fn operation_changed(&self) -> bool {
        self.operating() && self.operation_built() != self.opened_with
    }

    /// Whether the camera is turning to a new view or the pivot's marker
    /// fading.
    pub(crate) fn animating(&self) -> bool {
        self.animation.is_some() || self.fade.moving() || self.pivot_fading()
    }

    /// Whether the document may be edited, and so saved.
    pub(crate) fn editable(&self) -> bool {
        self.read_only.is_none()
    }

    /// What the document has to say as a toast, if anything: asked once.
    pub(crate) fn take_toast(&mut self) -> Option<String> {
        self.toast.take()
    }

    /// Changes the document through `edit`, unless it's read-only, keeping
    /// why the edit failed, if it did. Every change the user makes goes
    /// through here or [`Doc::edit_surely`]; an MCP client's document
    /// replacing it and a relink's amend check editability themselves.
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

    /// Applies `command`, keeping why it failed, if it did. Sketches it
    /// solves again (a parameter's change) get the time a proposal gets
    /// on the solver lane, [`varde_solve::PROPOSAL_TIME`], as this runs on
    /// the UI thread: one that runs out refuses the command.
    pub(crate) fn apply(&mut self, command: Command) {
        self.edit(|editor| apply_as_one(editor, [command]));
    }

    /// Applies an MCP client's `commands` as [`Doc::apply`] does (the
    /// same solve budget, shared by them all, and the same checks), as
    /// one undo step, or none of them: why they were refused comes back
    /// to the call, never to [`Doc::edit_error`], which is the user's own.
    /// The caller checks the document can be changed ([`Doc::editable`]).
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "on the web, MCP edits are none")
    )]
    pub(crate) fn apply_client(&mut self, commands: Vec<Command>) -> Result<(), EditError> {
        // Made on a copy (the history is `Arc`s, so it's cheap), so a
        // refused command after others leaves the design as it was.
        let mut editor = self.editor.clone();
        apply_as_one(&mut editor, commands)?;
        self.editor = editor;
        Ok(())
    }

    /// Takes `message`, asking something of the document itself. What
    /// the solver last refused shows until then.
    pub(crate) fn update(&mut self, message: Edit) {
        // Anything else asked of the document renames first.
        if !matches!(message, Edit::CommitRename) {
            self.commit_rename();
        }
        // And what's typed in the parameters' popup.
        if !matches!(message, Edit::Param(_)) {
            self.commit_param_typing();
        }
        self.end_refusal();
        self.notice = None;
        // Letting go of the Opacity slider leaves its menu open, to go on.
        if !matches!(
            message,
            Edit::CommitOpacity | Edit::ResetOpacity(_) | Edit::CommitColor | Edit::ResetColor(_)
        ) {
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
            Edit::Param(message) => self.params_edit(message),
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
            Edit::ConfirmDelete => self.confirm_delete(false),
            Edit::ConfirmDeleteOnly => self.confirm_delete(true),
            Edit::ToggleFeatureVisible(id) => self.change(Change::ToggleFeatureVisible(id)),
            Edit::SetRollback(until) => {
                self.rolling = None;
                self.change(Change::SetRollback(until));
            }
            Edit::DropRollback => {
                if let Some(until) = self.rolling.take() {
                    self.change(Change::SetRollback(until));
                }
            }
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
            Edit::ToggleClosed => self.toggle_closed(),
            Edit::ToggleClosedItem(item) => self.toggle_closed_item(item),
            Edit::InsertSplinePoint { spline, at } => self.insert_spline_point(spline, at),
            Edit::RemoveSplinePoints => self.remove_spline_points(),
            Edit::CommitExtrude => self.commit_extrude(false),
            Edit::CommitRevolve => self.commit_revolve(false),
            Edit::CommitCombine => self.commit_combine(false),
            Edit::CommitMotion => self.commit_motion(false),
            Edit::AcceptError => self.commit_operation(true),
            Edit::CommitOpacity => {
                if let Some((id, opacity)) = self.body_look.opacity.take() {
                    self.change(Change::SetOpacity(id, opacity));
                }
            }
            Edit::ResetOpacity(id) => {
                self.body_look.opacity = None;
                let opaque =
                    (self.editor.document().body(id)).is_none_or(|body| body.opacity.is_opaque());
                if !opaque {
                    self.change(Change::SetOpacity(id, Opacity::MAX));
                }
            }
            Edit::CommitColor => {
                if let Some((id, tint)) = self.body_look.color.take() {
                    self.change(Change::SetColor(id, Some(tint)));
                }
            }
            Edit::ResetColor(id) => {
                self.body_look.color = None;
                let colored = (self.editor.document().body(id)).is_some_and(|b| b.color.is_some());
                if colored {
                    self.change(Change::SetColor(id, None));
                }
            }
            Edit::CommitRename => self.commit_rename(),
            Edit::SetUnits(units) => self.change(Change::SetUnits(units)),
            Edit::SetTolerance(tolerance) => self.change(Change::SetTolerance(tolerance)),
            Edit::RemoveLink(link) => self.remove_link(link),
            Edit::SetLinkProfiles(link, profiles) => self.set_link_profiles(link, profiles),
            // What waits on the solver, and what waits behind it, is newer
            // than anything committed: undo takes back the newest of it.
            // An MCP client's edit waiting isn't shown, so isn't the
            // user's to drop: with none of theirs waiting, Undo undoes the
            // history, and the client's edit applies in its turn.
            Edit::Undo if self.proposals.users_waiting() => self.drop_newest(),
            Edit::Undo => self.edit_surely(Editor::undo),
            // What waits comes after what's undone, as a new edit does,
            // and edits waiting name items by the ids the sketch has
            // without it.
            Edit::Redo if self.proposals.users_waiting() => {}
            Edit::Redo => self.edit_surely(Editor::redo),
        }

        self.sync();
    }

    /// Makes `change`, or, while the user's edits wait on the solver, has
    /// it wait behind them, see [`Proposals`]: so what's committed, and so
    /// undo, keeps the order the user made them in. An MCP client's edit
    /// waiting alone doesn't hold it ([`Doc::users_proposing`]).
    fn change(&mut self, change: Change) {
        if self.users_proposing() {
            self.proposals.wait(change);
        } else {
            self.make(change);
        }
    }

    /// Makes `change` now, on the document as it is.
    pub(crate) fn make(&mut self, change: Change) {
        match change {
            Change::Remove {
                targets,
                confirmed,
                only,
            } => self.remove_now(targets, confirmed, only),
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
            Change::SetRollback(until) => self.apply(Command::SetRollback(until)),
            Change::NewSketch(plane) => self.new_sketch(plane),
            Change::SetPlane {
                feature,
                plane,
                placed,
                enter,
            } => self.set_sketch_plane(feature, plane, placed, enter),
            // Nothing if it's as it was: the editor adds no undo step.
            Change::SetOpacity(id, opacity) => self.apply(Command::SetOpacity(id, opacity)),
            Change::SetColor(id, color) => self.apply(Command::SetColor(id, color)),
            Change::SetUnits(units) => self.apply(Command::SetUnits(units)),
            Change::SetTolerance(tolerance) => self.apply(Command::SetTolerance(tolerance)),
            Change::Rename(command) => self.apply(command),
            Change::Param(command) => self.make_param_now(command),
        }
    }

    /// Takes `message`, which only changes how the document is looked at.
    /// An action in the sketch ends what the solver last refused showing.
    pub(crate) fn look(&mut self, message: Look) {
        let asked = self.delete_asked();
        if !self.rename_look(&message) && !self.params_message(&message) {
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
            ) && !hovers(&message)
            {
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
        if !(hovers(&message) || moves_camera(&message) || matches!(message, Look::Rail(_))) {
            self.notice = None;
        }
        // A row's context menu does its job or is left by anything else
        // done: `Esc` closes it alone.
        if !matches!(
            message,
            Look::OpenMenu(_)
                | Look::KeyMenu(_)
                | Look::PreviewOpacity(..)
                | Look::PreviewColor(..)
                | Look::Escape
        ) && !hovers(&message)
        {
            self.row_menu = None;
        }
        // Picking a tool, from the rail's list or not, closes the list.
        if matches!(
            message,
            Look::SelectTool(_)
                | Look::ToggleConstrain
                | Look::PickPlane
                | Look::ChangePlane(_)
                | Look::StartMeasure
        ) || opens_operation(&message)
        {
            self.rail.close();
        }
        // Another tool leaves the measure tool.
        if matches!(message, Look::PickPlane | Look::ChangePlane(_)) || opens_operation(&message) {
            self.mode.take_measure();
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
                self.panel = panel.for_sketching(self.mode.sketch().is_some());
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
            Look::StartMotion(kind) => self.start_motion(kind),
            Look::Motion(message) => self.motion_look(message),
            Look::StartMeasure => self.start_measure(),
            Look::Measure(message) => self.measure_look(message),
            Look::FinishSketch => self.finish_sketch(),
            // Only outside sketches and operations, as the marker shows
            // where they put it then.
            Look::DragRollback(until) => {
                if self.editable() && !self.operating() && self.mode.sketch().is_none() {
                    self.rolling = Some(until);
                }
            }
            Look::ClickFeature { feature, roll } => {
                if roll && self.editable() && !self.rollback().1 && self.mode.sketch().is_none() {
                    let document = self.editor.document();
                    if let Some(at) = document.feature_index(feature) {
                        let after = document.features().get(at + 1).map(|f| f.id);
                        if after != document.rollback() {
                            self.update(Edit::SetRollback(after));
                        }
                    }
                } else {
                    self.look(Look::SelectFeature(feature));
                }
            }
            Look::SelectFeature(id) => {
                if self.editor.document().feature(id).is_some() {
                    self.selected_feature = Some(id);
                    self.clear_model_selection();
                }
            }
            Look::OpenMenu(menu) => self.open_menu(menu),
            Look::KeyMenu(menu) => {
                self.panel = match menu {
                    RowMenu::Feature(_) => Panel::Timeline,
                    RowMenu::Body(_) | RowMenu::Sketch(_) => Panel::Objects,
                    RowMenu::Link(_) | RowMenu::Item(_) => Panel::Sketch,
                }
                .for_sketching(self.mode.sketch().is_some());
                self.open_menu(menu);
            }
            Look::CloseMenu => {}
            Look::PreviewOpacity(id, opacity) => self.preview_opacity(id, opacity),
            Look::PreviewColor(id, tint) => self.preview_color(id, tint),
            Look::ClickGeometry { hit, add } => self.click_geometry(hit, add),
            // The app turns this into a `ClickGeometry`, knowing the keys
            // held; alone, it selects.
            Look::ClickRow(id) => self.click_geometry(Some(id), false),
            Look::HoverItem(id) => self.hover_item(id),
            Look::ClickLink(link) => self.click_link(link),
            Look::HoverLink(link) => self.hover_link(link),
            Look::LeaveItem(id) => self.leave_item(id),
            Look::LeaveLink(link) => self.leave_link(link),
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
            Look::HoverBodyLook(hovered) => self.body_look.hovered = hovered,
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
            Look::ClickModel { pick, .. } if self.mode.combine().is_some() => {
                self.combine_click(pick)
            }
            Look::ClickModel { pick, .. } if self.mode.motion().is_some() => {
                self.motion_click(pick)
            }
            Look::ClickModel { pick, add, double } if self.mode.measure().is_some() => {
                self.measure_click(pick, add, double);
            }
            Look::ClickModel { pick, add, double } => self.click_model(pick, add, double),
            Look::ClickBody { body, .. } if self.mode.combine().is_some() => {
                self.combine_body(body)
            }
            Look::ClickBody { body, .. } if self.mode.motion().is_some() => self.motion_body(body),
            Look::ClickBody { body, add } if self.mode.measure().is_some() => {
                self.measure_body(body, add)
            }
            Look::ClickBody { body, add } => self.click_body(body, add),
            Look::ClickObject { row, add } => self.click_object(row, add),
            Look::Snap(snap) => {
                if let Some(session) = self.mode.sketch_mut() {
                    session.snap = snap;
                }
            }
            Look::Aim(click) => {
                if let Some(session) = self.mode.sketch_mut() {
                    session.snap = Some(click.snap()).filter(Snap::snapped);
                    session.aim = Some(click);
                }
            }
            Look::NextField => self.next_field(),
            Look::ToggleCentered => self.toggle_centered(),
            Look::ToggleSplineKind => self.toggle_spline_kind(),
            Look::ToggleComb => self.toggle_comb(),
            Look::ToggleInsertPoint => self.toggle_insert_point(),
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
                // The row's hovered as its toggle's clicked, which would
                // draw the object hidden anyway, as its selection would:
                // hiding it drops the hover, to show the change, until the
                // row's entered again, and deselects it.
                if !*shown {
                    self.origin_hover.take_if(|&mut hovered| hovered == object);
                    let row = varde_view::ObjectRow::Origin(object);
                    self.objects_selected.retain(|&selected| selected != row);
                }
            }
            Look::ToggleExpanded(id) => self.toggle_expanded(id),
            Look::SelectBox { ids, add } => self.select_box(ids, add),
            // While measuring, the selection is hidden, and kept for
            // after: Space doesn't clear what can't be seen.
            Look::ClearSelection => {
                if self.mode.measure().is_none() {
                    self.clear_selection();
                }
            }
            Look::SelectTool(tool) => self.select_tool(tool),
            Look::DragGeometry {
                id,
                from,
                to,
                target,
            } => self.drag_geometry(id, from, to, target),
            Look::CancelDrag => {
                if let Some(session) = self.mode.sketch_mut() {
                    session.drag = None;
                }
            }
            Look::PressLabel { id, add } => self.press_label(id, add),
            Look::DragLabel { id, from, to } => self.drag_label(id, from, to),
            Look::EditDimension { id, in_list } => self.edit_dimension(id, in_list),
            // Taken by `Doc::rename_look`.
            Look::StartRename(_) | Look::RenameInput(_) | Look::CancelRename => {}
            // Taken by `Doc::params_message`.
            Look::ToggleParams | Look::Params(_) => {}
            Look::ValueInput(text) => self.value_input(text),
            Look::CancelValue => self.close_value(),
            Look::SwitchRound => self.switch_round(),
            Look::SplitSketchTab(share) => {
                if share.is_finite() {
                    self.sketch_split = share.clamp(0.0, 1.0);
                }
            }
            Look::ScrollGeometry(offset) => {
                if let Some(session) = self.mode.sketch_mut() {
                    session.scroll = offset;
                }
            }
            Look::ScrollConstraints(offset) => {
                if let Some(session) = self.mode.sketch_mut() {
                    session.constraint_scroll = offset;
                }
            }
            Look::Orbit { yaw, pitch, about } => self.orbit(yaw, pitch, about),
            Look::Pan { dx, dy } => {
                self.animation = None;
                self.camera.pan(dx, dy);
            }
            Look::Zoom { factor, x, y } => {
                self.animation = None;
                self.camera.zoom_at(factor, x, y);
            }
            // Bounded, so no view is taken for wider or narrower than
            // any window gets.
            Look::ViewAspect(aspect) if aspect.is_finite() && aspect > 0.0 => {
                self.aspect = Some(aspect.clamp(0.01, 100.0));
            }
            Look::ViewAspect(_) => {}
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
            Look::Rail(message) => self.rail_look(message),
        }
    }

    /// Opens the context menu of the side panel's row `menu` is on,
    /// selecting a feature of the Timeline's, or an Objects row not
    /// already selected, unless in a sketch, where
    /// neither list's rows show.
    fn open_menu(&mut self, menu: RowMenu) {
        let document = self.editor.document();
        let exists = match menu {
            RowMenu::Feature(id) | RowMenu::Sketch(id) => document.feature(id).is_some(),
            RowMenu::Body(id) => document.body(id).is_some(),
            // A link's row shows in a sketch alone.
            RowMenu::Link(id) => {
                let links = self
                    .mode
                    .sketch()
                    .map_or(&[][..], |session| &session.links[..]);
                links.iter().any(|row| row.link == id)
            }
            // So does a point's or curve's.
            RowMenu::Item(id) => self.holds(id),
        };
        let in_sketch = matches!(menu, RowMenu::Link(_) | RowMenu::Item(_));
        if self.mode.sketch().is_some() != in_sketch || !exists {
            return;
        }
        match menu {
            RowMenu::Feature(id) => {
                self.selected_feature = Some(id);
                self.clear_model_selection();
            }
            // An Objects row is selected alone as a click would, unless
            // already part of the selection, which is kept.
            RowMenu::Body(id) => {
                let shown = self.feed.shown_body(id);
                let picking = self.mode.combine().is_some() || self.mode.motion().is_some();
                if !picking
                    && self.mode.measure().is_none()
                    && self.pick.selection.mode().takes_bodies()
                    && !self.pick.selection.bodies().any(|body| body == shown)
                {
                    self.click_body(id, false);
                }
            }
            RowMenu::Sketch(id) => {
                let row = varde_view::ObjectRow::Sketch(id);
                if !self.objects_selected.contains(&row) {
                    self.click_object(row, false);
                }
            }
            RowMenu::Link(_) | RowMenu::Item(_) => {}
        }
        self.row_menu = Some(menu);
    }

    /// The row whose context menu the context menu key opens: the
    /// feature selected in the Timeline, else the last sketch selected in
    /// Objects, else the first body selected or holding what's selected
    /// in the model; in a sketch, the first point or curve selected.
    pub(crate) fn menu_target(&self) -> Option<RowMenu> {
        if let Some(session) = self.mode.sketch() {
            return (session.selection.iter())
                .copied()
                .find(|&id| self.holds(id))
                .map(RowMenu::Item);
        }
        if let Some(id) = self.selected_feature {
            return Some(RowMenu::Feature(id));
        }
        if let Some(id) = self.selected_sketches().last() {
            return Some(RowMenu::Sketch(id));
        }
        self.selected_bodies().first().copied().map(RowMenu::Body)
    }

    /// Previews `opacity` for the body `id` while its context menu's
    /// slider is dragged, if that menu is open and the document editable;
    /// [`Edit::CommitOpacity`] commits it on letting go, and
    /// [`Doc::prune_preview`] drops it if the menu closes first.
    fn preview_opacity(&mut self, id: BodyId, opacity: Opacity) {
        if self.editable() && self.row_menu == Some(RowMenu::Body(id)) {
            self.body_look.opacity = Some((id, opacity));
        }
    }

    /// Previews `tint` for the body `id` as [`Doc::preview_opacity`] does
    /// its opacity: [`Edit::CommitColor`] commits it on letting go.
    fn preview_color(&mut self, id: BodyId, tint: Tint) {
        if self.editable() && self.row_menu == Some(RowMenu::Body(id)) {
            self.body_look.color = Some((id, tint));
        }
    }

    /// Drops the opacity and colour previewed unless their body's context
    /// menu, and so the sliders, is still open.
    fn prune_preview(&mut self) {
        self.body_look.prune(self.row_menu);
    }

    /// Starts sending requests to `lane`, the document's regeneration
    /// lane.
    pub(crate) fn lane_ready(&mut self, lane: varde_regen::lane::Lane) {
        self.feed.connect(lane);
        self.regen_replaced();
        self.sync();
    }

    /// Whether a picture an MCP call asked for waits for the viewport's
    /// next frame (natively: the web has no MCP server).
    pub(crate) fn drawing_for_mcp(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.mcp_drawing();
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// Whether the one-second timer is to tick for this design, besides
    /// auto-save: while a thumbnail renders, to give up on it, and while
    /// an MCP call waits, to bound its wait.
    pub(crate) fn ticking(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if self.mcp_waiting() {
            return true;
        }
        self.rendering_thumbnail()
    }

    /// Shows `response`, computed for the document. An export's answer
    /// goes to [`Doc::export_welded`] instead, and an MCP call's edge
    /// facts or measure to the call waiting on them.
    pub(crate) fn computed(&mut self, response: varde_regen::Response) {
        // An MCP call's edge facts or measure: nothing shown changes.
        if let varde_regen::Response::Answered { .. } = response {
            #[cfg(not(target_arch = "wasm32"))]
            self.query_answered(response);
            return;
        }
        self.feed.apply(response);
        #[cfg(not(target_arch = "wasm32"))]
        let before = self.editor.revision();
        self.relink();
        // An edit waiting on regeneration made the state the links' amend
        // replaced: the amended one is its after now. Then those whose
        // regeneration came are answered.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.mcp.amended(before, self.editor.revision());
            self.edits_regenerated();
        }
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
        if let Some(session) = self.mode.sketch_mut() {
            session.refusal = None;
        }
    }

    /// What the document screen's shortcuts depend on, or `None` while
    /// the user is asked about unsaved changes or deleting, or drags the
    /// Opacity or a Colour slider: no key changes the document behind the prompt, or
    /// before the opacity or colour is committed.
    pub(crate) fn keys(&self) -> Option<DocumentKeys> {
        let dragging = self.body_look.sliding();
        (self.dialog().is_none() && !dragging).then(|| {
            DocumentKeys::new(self.editable(), self.selected_feature, self.sketch_state())
                .with_face_selected(self.selected_face().is_some())
                .with_operation(OperationKeys::of(
                    self.extrude_state().as_ref(),
                    self.revolve_state().as_ref(),
                    self.combine_state().as_ref(),
                    self.motion_state().as_ref(),
                ))
                .with_bodies(
                    !self.editor.document().bodies().is_empty(),
                    self.combinable(),
                )
                .with_measure(self.mode.measure().is_some())
                .with_picking_plane(self.mode.picking_plane().is_some())
                .with_rail(self.rail.state())
                .with_rename(self.rename_target())
                .with_menu(self.menu_target())
                .with_edited(self.edited())
                .with_history(
                    self.editor.can_undo() || self.proposing(),
                    self.editor.can_redo() && !self.proposing(),
                )
        })
    }

    /// The prompt the user is being asked, if any, which `Esc` cancels:
    /// an MCP client's edit asking first (natively), over the one about
    /// unsaved changes, over the one about deleting.
    pub(crate) fn dialog(&self) -> Option<Dialog> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.mcp.ask.is_some() {
            return Some(Dialog::McpAsk);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.mcp.revert_ask.is_some() {
            return Some(Dialog::McpRevert);
        }
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
    /// a body's Opacity or Colour slider is dragged, which would go with its tab and so
    /// never see its release.
    pub(crate) fn peeks(&self, held: bool) -> bool {
        held && !self.dimensioning() && !self.body_look.sliding()
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
            #[cfg(not(target_arch = "wasm32"))]
            ForDoc::McpRendered(tag, image) => {
                self.mcp_rendered(&mut cx.io, tag, image);
                Next::Stay
            }
        }
    }

    /// Takes the settings in `options` the camera follows: the projection,
    /// how dragging turns it (a turntable rights the view, should it be
    /// upside down or rolled, and over the top unrolls it, should a
    /// trackball have rolled it) and the point it orbits (forgotten unless
    /// picked by a click).
    pub(crate) fn take_options(&mut self, options: &ViewOptions) {
        use varde_render::Rotation;
        self.set_projection(options.projection);
        self.rotation = options.mouse.rotation;
        let right: Option<fn(&mut Camera)> = match self.rotation {
            Rotation::Turntable => Some(Camera::upright),
            Rotation::OverTheTop => Some(Camera::unroll),
            Rotation::Trackball => None,
        };
        if let Some(right) = right {
            right(&mut self.camera);
            if let Some(animation) = &mut self.animation {
                right(&mut animation.from);
                right(&mut animation.to);
            }
            // As are the views gone back to.
            let kept = (self.before_sketch.iter_mut())
                .chain(self.before_show.iter_mut().map(|(camera, _)| camera));
            kept.for_each(right);
        }
        if options.mouse.rotation_point != varde_view::RotationPoint::Clicked {
            self.pivot = None;
            if let Some((_, pivot)) = &mut self.before_show {
                *pivot = None;
            }
        }
    }

    /// Views the model in `projection`, the camera turning to come in
    /// it too if it is: the app's setting, which every document takes.
    pub(crate) fn set_projection(&mut self, projection: Projection) {
        self.camera.set_projection(projection);
        if let Some(animation) = &mut self.animation {
            animation.to.set_projection(projection);
        }
    }

    /// The document screen, showing the other panel tab if `peek`, unless
    /// the Dimension tool is in use, where the peek key places references,
    /// with the view `options`, and what the platform `offers`.
    #[cfg(test)]
    pub(crate) fn view(
        &self,
        peek: bool,
        mode: Mode,
        options: ViewOptions,
        offers: Offers,
    ) -> Element<'_, Ui> {
        self.view_with(peek, mode, options, offers, None, None)
    }

    /// The document screen (see [`Doc::state`]), with the `settings`
    /// page in the viewport's place while it shows.
    pub(crate) fn view_with<'a>(
        &'a self,
        peek: bool,
        mode: Mode,
        options: ViewOptions,
        offers: Offers,
        settings: Option<varde_view::SettingsState<'a>>,
        mcp: Option<varde_view::McpShown<'a>>,
    ) -> Element<'a, Ui> {
        let state = varde_view::DocumentState {
            settings,
            mcp,
            ..self.state(peek, mode, options, offers)
        };
        varde_view::document(state)
    }

    /// What the document screen is built from, see [`Doc::view_with`].
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
            fade: self.fade.value,
            fading_grid: self.fade.grid.filter(|_| self.fade.moving()),
            mesh: self.feed.mesh(),
            parts: self.feed.parts(),
            opacity_preview: self.body_look.opacity,
            body_look_hovered: self.body_look.hovered,
            color_preview: self.body_look.color,
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
            file: varde_view::FileShown {
                name: &self.name,
                unnamed: self.unnamed(),
                path: self.path.as_deref(),
                location: self.target().location(),
                downloads: self.download_status(),
                downloadable: offers.download,
                rename: matches!(self.target(), Target::Browser { .. }).then(|| self.renamable()),
                naming: self.naming().map(|naming| varde_view::NamePrompt {
                    name: &naming.name,
                    place: naming.place,
                    places: offers.file_system_access,
                    rename: naming.rename,
                    taken: naming.replacing.as_deref(),
                }),
                edited: self.edited(),
                saving: self.saving(),
                save_error: self.banner_error(),
                recovered: self.recovered_changes(now),
                damage: self.damage().map(|damage| damage.shown(now)),
                exportable: self.exportable(),
                exporting: self.exporting(),
                export_error: self.export_error(),
            },
            read_only: self.read_only.as_deref(),
            edit_error: self.edit_error.as_ref(),
            notice: self.notice.as_deref(),
            refused_edit: self.refused_edit(),
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
            picking_plane: self.mode.picking_plane().map(|picking| &picking.pick),
            thumbnail: self.thumbnail_request(),
            #[cfg(not(target_arch = "wasm32"))]
            renders: self.mcp.renders(),
            #[cfg(target_arch = "wasm32")]
            renders: &[],
            aspect: self.aspect,
            selected_feature: self.selected_feature,
            rollback: self.rollback(),
            rolling: self.rolling.is_some(),
            row_menu: self.row_menu,
            origin: self.origin,
            objects_selected: &self.objects_selected,
            origin_hover: (self.hovered_plane().map(varde_view::OriginObject::Plane))
                .or(self.origin_hover),
            objects_folded: &self.objects_folded,
            overlaps: self.overlaps.as_ref().map(|listed| &listed.list),
            overlap_ticks: self.overlap_ticks(),
            sketch: self.sketch_state(),
            operation: self.operation_state(),
            combinable: self.combinable(),
            params: self.params_state(),
            unsolved: self.feed.unsolved(),
            failed: self.feed.failed_features(),
            merged: self.feed.merged_bodies(),
            deleting: self.delete_prompt(),
            proposing: self.proposing(),
            rail: self.rail.state(),
            settings: None,
            mcp: None,
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
    /// [`Doc::remove`]; with `only`, just `targets`, if that's what
    /// `confirmed` is, see [`Doc::confirm_delete`].
    Remove {
        targets: Vec<Removable>,
        confirmed: Option<Removal>,
        only: bool,
    },
    ToggleVisible(BodyId),
    ToggleFeatureVisible(FeatureId),
    SetRollback(Option<FeatureId>),
    SetOpacity(BodyId, Opacity),
    SetColor(BodyId, Option<Tint>),
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
    /// Adds, changes, renames or removes a parameter.
    Param(Command),
}

/// Whether `message` opens an operation's panel, new or on a feature
/// (a sketch's [`Look::EditFeature`] enters the sketch instead).
fn opens_operation(message: &Look) -> bool {
    matches!(
        message,
        Look::StartExtrude
            | Look::StartRevolve
            | Look::StartCombine
            | Look::StartMotion(_)
            | Look::EditFeature(_)
    )
}

/// Whether `message` only hovers, leaves, scrolls, snaps or aims, which
/// leaves menus, lists, notices and fields open.
pub(super) fn hovers(message: &Look) -> bool {
    matches!(
        message,
        Look::Hover(_)
            | Look::HoverSketch(_)
            | Look::HoverItem(_)
            | Look::HoverLink(_)
            | Look::LeaveItem(_)
            | Look::LeaveLink(_)
            | Look::HoverFeature(_)
            | Look::LeaveFeature(_)
            | Look::HoverOrigin(_)
            | Look::HoverPlane(_)
            | Look::LeaveOrigin(_)
            | Look::HoverBodyRow(_)
            | Look::LeaveBodyRow(_)
            | Look::HoverBodyLook(_)
            | Look::HoverPanel(_)
            | Look::LeavePanel(_)
            | Look::HoverCube(_)
            | Look::Snap(_)
            | Look::Aim(_)
            | Look::ScrollGeometry(_)
            | Look::ScrollConstraints(_)
    )
}

/// Whether `message` moves the camera.
pub(super) fn moves_camera(message: &Look) -> bool {
    matches!(
        message,
        Look::Orbit { .. }
            | Look::Pan { .. }
            | Look::Zoom { .. }
            | Look::ViewAspect(_)
            | Look::SetPivot(_)
    )
}

/// A prompt over a screen, see [`Doc::dialog`] and
/// [`Welcome::dialog`](crate::welcome::Welcome::dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Dialog {
    /// About unsaved changes, before the document is closed.
    Unsaved,
    /// About deleting more than was asked for.
    Delete,
    /// An MCP client's edit asking the user to finish theirs first
    /// (`mcp/ask.rs`), natively.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "no MCP on the web")
    )]
    McpAsk,
    /// Asking before a revert from the MCP panel that undoes later
    /// changes (`mcp/revert.rs`), natively.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "no MCP on the web")
    )]
    McpRevert,
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

/// Applies `commands` to `editor` as one undo step: the first that
/// changes something is the step, the rest amended into it. Sketches
/// they solve again share the time a proposal gets
/// ([`within_proposal_time`]). Stops at the first refused, those before
/// it applied.
fn apply_as_one(
    editor: &mut Editor,
    commands: impl IntoIterator<Item = Command>,
) -> Result<(), EditError> {
    let start = editor.generation();
    within_proposal_time(|budget| {
        for command in commands {
            match editor.generation() == start {
                true => editor.apply_within(command, budget)?,
                false => editor.amend_within(command, budget)?,
            }
        }
        Ok(())
    })
}

/// Runs `act` with the time a proposal gets on the solver lane,
/// [`varde_solve::PROPOSAL_TIME`], for the sketches an edit solves again
/// on the UI thread.
fn within_proposal_time<R>(act: impl FnOnce(&varde_sketch::Budget) -> R) -> R {
    // iced's clock, which the web has too.
    let end = iced::time::Instant::now().checked_add(varde_solve::PROPOSAL_TIME);
    let expired = move || end.is_some_and(|end| iced::time::Instant::now() >= end);
    let budget = varde_sketch::Budget {
        expired: &expired,
        ..varde_sketch::Budget::default()
    };
    act(&budget)
}

/// Why a document with `access` can't be edited, if it can't.
fn read_only(access: Access) -> Option<String> {
    match access {
        Access::Edit => None,
        Access::ReadOnly(reason) => Some(reason.to_string()),
    }
}
