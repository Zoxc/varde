//! UI code: iced widgets and layout.
//!
//! The view is a pure function of application state. It produces
//! [`Message`]s but never mutates anything itself; that is the job of the
//! `app` crate.

// Proving wgpu types `Send + Sync` for the shader pipeline exceeds the default
// limit on recent nightlies.
#![recursion_limit = "256"]

mod chrome;
mod controls;
mod document;
mod icons;
mod panels;
mod shortcut;
mod theme;
mod toolbar;
mod view_cube;
mod viewport;
mod welcome;

use std::path::PathBuf;

use varde_document::BodyId;
use varde_render::{Projection, View};

pub use document::{DocumentState, MeshStatus, Overlay, RecoveredChanges, document};
pub use icons::LOGO_SVG;
pub use shortcut::{Binding, Held, document_bindings, pressed, welcome_bindings};
pub use theme::{Mode, theme as iced_theme};
pub use welcome::{RecentCard, StoredDesign, WelcomeState, welcome};

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
    ToggleFileMenu,
    AddCube,
    RemoveBody(BodyId),
    ToggleVisible(BodyId),
    Undo,
    Redo,
}

/// What only changes how the open document is looked at: the camera, the
/// side panel tab, closing the file menu.
#[derive(Debug, Clone)]
pub enum Look {
    CloseFileMenu,
    SelectPanel(Panel),
    /// Turns the camera around its target by these angles in radians.
    Orbit {
        yaw: f32,
        pitch: f32,
    },
    Pan {
        dx: f32,
        dy: f32,
    },
    Zoom(f32),
    ResetCamera,
    LookFrom(View),
    SetProjection(Projection),
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

/// A tab of the side panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    Timeline,
    #[default]
    Objects,
}

impl Panel {
    pub fn other(self) -> Self {
        match self {
            Panel::Timeline => Panel::Objects,
            Panel::Objects => Panel::Timeline,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Panel::Timeline => "Timeline",
            Panel::Objects => "Objects",
        }
    }
}
