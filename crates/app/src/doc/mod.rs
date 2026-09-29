//! An open document: its editor and view, and saving and leaving it, see
//! [`save`].

mod camera;
mod feed;
mod save;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::Vec3;
use iced::Element;
use varde_document::name::UNTITLED;
use varde_document::{CUBE_SIZE, Command, Document, EditError, Editor};
use varde_io::{Access, Offer, OpenId};
use varde_render::{Camera, Projection};
use varde_view::{Edit, Look, Message as Ui, Mode, Overlay, Panel};

#[cfg(test)]
pub(crate) use camera::CAMERA_ANIMATION;
pub(crate) use camera::CameraAnimation;
use feed::MeshFeed;
use save::Persist;
#[cfg(test)]
pub(crate) use save::{AutoSave, Picking};
pub(crate) use save::{Downloader, Downloads, Leave, Target};

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
    /// Why the document can't be edited, if it can't.
    pub(crate) read_only: Option<String>,
    /// Why the last edit was refused, if it was.
    pub(crate) edit_error: Option<EditError>,
    /// Shown to the user in place of a path.
    pub(crate) name: String,
    pub(crate) panel: Panel,
    pub(crate) file_menu: bool,
    animation: Option<CameraAnimation>,
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

/// Where Home and the view cube turn the camera to look at: the centre of
/// the first cube Add Cube makes.
const HOME_TARGET: Vec3 = Vec3::splat(CUBE_SIZE / 2.0);

/// The camera Home turns to, framed on the first cube.
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
            read_only: read_only(access),
            edit_error: None,
            name,
            panel: Panel::default(),
            file_menu: false,
            animation: None,
        };
        doc.sync();
        doc
    }

    /// Asks for the mesh of the document if it changed.
    pub(crate) fn sync(&mut self) {
        self.feed.request(&self.editor);
    }

    /// Whether the camera is turning to a new view.
    pub(crate) fn animating(&self) -> bool {
        self.animation.is_some()
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
    fn apply(&mut self, command: Command) {
        self.edit(|editor| editor.apply(command));
    }

    /// Takes `message`, asking something of the document itself.
    pub(crate) fn update(&mut self, message: Edit) {
        match message {
            Edit::ToggleFileMenu => self.file_menu = !self.file_menu,
            Edit::DismissSaveError => self.dismiss_save_error(),
            Edit::AddCube => self.apply(self.editor.document().add_cube()),
            Edit::RemoveBody(id) => self.apply(Command::RemoveBody(id)),
            Edit::ToggleVisible(id) => {
                if let Some(body) = self.editor.document().body(id) {
                    let visible = !body.visible;
                    self.apply(Command::SetVisible(id, visible));
                }
            }
            Edit::Undo => self.edit_surely(Editor::undo),
            Edit::Redo => self.edit_surely(Editor::redo),
        }

        self.sync();
    }

    /// Takes `message`, which only changes how the document is looked at.
    pub(crate) fn look(&mut self, message: Look) {
        match message {
            Look::CloseFileMenu => self.file_menu = false,
            Look::SelectPanel(panel) => self.panel = panel,
            Look::Orbit { yaw, pitch } => {
                self.animation = None;
                self.camera.orbit(yaw, pitch);
            }
            Look::Pan { dx, dy } => {
                self.animation = None;
                self.camera.pan(dx, dy);
            }
            Look::Zoom(factor) => {
                self.animation = None;
                self.camera.zoom(factor);
            }
            Look::ResetCamera => self.animate_camera(home_camera(self.camera.projection())),
            Look::LookFrom(view) => {
                let mut to = self.camera;
                to.look_from(view);
                to.set_target(HOME_TARGET);
                self.animate_camera(to);
            }
            Look::SetProjection(projection) => {
                self.camera.set_projection(projection);
                if let Some(animation) = &mut self.animation {
                    animation.to.set_projection(projection);
                }
            }
        }
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
        }
    }

    /// The document screen, showing the other panel tab if `peek`.
    pub(crate) fn view(&self, peek: bool, mode: Mode) -> Element<'_, Ui> {
        varde_view::document(varde_view::DocumentState {
            editor: &self.editor,
            camera: &self.camera,
            mesh: self.feed.mesh(),
            mesh_status: self.feed.status(&self.editor),
            name: &self.name,
            edited: self.edited(),
            read_only: self.read_only.as_deref(),
            edit_error: self.edit_error.as_ref(),
            saving: self.saving(),
            save_error: self.banner_error(),
            recovered: self.recovered().map(|offer| varde_view::RecoveredChanges {
                design_changed: offer.design_changed,
            }),
            // The prompt shows over the file menu.
            overlay: self
                .prompt()
                .map(|_| Overlay::UnsavedPrompt)
                .or(self.file_menu.then_some(Overlay::FileMenu)),
            panel: self.panel,
            peek,
            mode,
        })
    }
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
