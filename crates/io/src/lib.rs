//! The IO lane: all file system access, away from the UI thread.
//!
//! The UI sends a [`Request`] and never waits; the [`Response`] comes back
//! later as a message. Unlike the regeneration lane (`varde-regen`), requests
//! are handled in the order they were sent, since writes to one file must
//! land in order. The one exception is coalescing per target: natively, a
//! [`Request::WriteRecent`] or [`Request::WriteSettings`] still waiting
//! replaces an older one (the web has no recent files store to write),
//! and a [`Request::Save`] replaces
//! one of the same file still waiting behind it, unless something else is
//! to happen to that file in between, and so does a [`Request::AutoSave`].
//! None of them crosses a [`Request::Flush`].
//!
//! The lane owns the open files. A `DocumentFile` (natively, see
//! `src/native/files/document_file.rs`) keeps the tail it last read or
//! wrote as its conflict check, so two operations must never use it at
//! once; the UI holds a [`FileId`] instead. The lane also holds each
//! editable document's lock file (the sidecar, see
//! `src/native/sidecar.rs`) for as long as it is open, which makes sure
//! only one editor edits a document.
//!
//! The sidecar also holds the document's auto-saves (`src/autosave.rs`),
//! and a new design, never saved, lives in an entry of the app's store of
//! new designs (`src/store.rs`) instead, locked the same way. A clean close
//! empties and deletes them; one found with something in it and not locked
//! is from a session that crashed, and is offered back to the user.
//!
//! Natively the lane is a thread for the whole app
//! (`src/native/thread.rs`). The web has no path based file system: there
//! the lane is a Web Worker (`src/web.rs`, handling requests in
//! `src/web/worker/files.rs`) keeping
//! auto-saves in the Origin Private File System (`src/opfs.rs`), and
//! requests and responses cross to it as bytes (see `src/wire.rs`). Both
//! are [`lane`]. Files of the user's are picked on the web by the page,
//! see [`pick`], and handed to the worker as a [`Picked`].
//!
//! [`three_mf`] writes bodies' meshes as a 3MF package, for printing.

// Modules at the root are compiled for both targets. What only native
// builds have, with a path based file system, is under `native`; what only
// the web has is under `web`, split into the page's side and the IO
// worker's. The plain Rust halves of what only the web uses (`js`, `opfs`,
// `wire`) are at the root, tested natively too.
mod autosave;
#[cfg(any(target_arch = "wasm32", test))]
mod js;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod open;
#[cfg(any(target_arch = "wasm32", test))]
mod opfs;
pub mod pick;
mod queue;
pub mod recent;
pub mod settings;
mod store;
pub mod three_mf;
pub mod vrdp;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(any(target_arch = "wasm32", test))]
mod wire;

/// Handles the app's file system requests in order: natively a thread
/// (`src/native/thread.rs`), on the web a Web Worker (`src/web.rs`). Both
/// have the same API: [`spawn`](lane::spawn) starts one and returns the
/// [`Lane`](lane::Lane) to send requests through and the
/// [`Responses`](lane::Responses) stream to read. Natively, `spawn_at`
/// starts one with the app's own files elsewhere, see [`Stores`], which
/// keeps tests off the user's; the web lane always keeps them in
/// [`Stores::user`].
pub mod lane {
    use crate::{Request, Response};

    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::native::thread::{spawn, spawn_at};
    #[cfg(target_arch = "wasm32")]
    pub use crate::web::spawn;

    /// Sends requests to the lane. Cheap to clone; all clones feed the same
    /// thread or worker.
    pub type Lane = varde_lane::Lane<Request>;

    /// The responses of the lane, as a stream. Dropping it ends the lane:
    /// natively once the queue is done, on the web at once, terminating the
    /// worker and letting go of the files it holds, as closing the tab
    /// would.
    pub type Responses = varde_lane::Responses<Request, Response>;
}

/// The IO Web Worker's side, run by its `main` (see
/// `src/bin/varde-io-worker.rs`), never by the page.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use crate::web::serve as serve_worker;

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use varde_document::{Document, Revision, Snapshot};

use crate::vrdp::Error as FileError;

pub use crate::recent::RecentFile;
pub use crate::settings::Settings;
pub use crate::store::Recovered;
/// Carries [`Request`]s to the lane without waiting for them to be handled.
pub use varde_lane::Transport;

/// Where the lane keeps the app's own files. [`Stores::user`] is the
/// user's; tests use a temporary directory, or [`Stores::default`], which
/// is nowhere, to keep off them. On the web the paths are directories in
/// the Origin Private File System.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stores {
    /// The recent files list, `recent.toml`.
    pub recent: Option<PathBuf>,
    /// The settings, `settings.toml`.
    pub settings: Option<PathBuf>,
    /// The directory holding new designs until they're first saved.
    pub designs: Option<PathBuf>,
}

impl Stores {
    /// The user's: the recent files list and the settings in the platform
    /// config directory and new designs in the data directory, e.g.
    /// `~/.config/varde-cad` and `~/.local/share/varde-cad/designs`. On the
    /// web there's no recent files list, and the settings and new designs
    /// are kept in `settings.toml` and `designs` in the Origin Private File
    /// System.
    pub fn user() -> Self {
        Self {
            recent: recent::store(),
            settings: settings::store(),
            designs: store::designs(),
        }
    }
}

/// An open file, owned by the lane. Unique within the lane, which hands
/// them out; public so tests can stand in for the lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u64);

/// Tags a [`Request::Open`] and its answer, so the UI can tell an answer
/// it's still waiting for from one it gave up on. Chosen by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpenId(pub u64);

/// Work for the IO lane.
///
/// Serializable to cross to the web's IO worker, see `src/wire.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Reads the document the user chose, answered with
    /// [`Response::Opened`]. Natively, at a path: takes its lock, see
    /// [`Access`], and finds what a crashed session auto-saved, see
    /// [`Opened::recovered`]. On the web, the file the user picked, which
    /// has no path to answer with: auto-saves go to a store entry made for
    /// it, as for a new design. If it's from a [`PickedFrom::Handle`], its
    /// file refers to the picked file from then on, which [`Request::Save`]
    /// writes; otherwise the design is a copy, never saved.
    Open { id: OpenId, from: Chosen },
    /// Creates and locks an entry in the store of new designs for a new
    /// design, which its auto-saves go to until it's first saved as a file
    /// of the user's. Answered with [`Response::Created`].
    New { id: OpenId },
    /// Appends `document`, the editor's state at `revision`, to `file`.
    /// Encoding, locking and writing all happen in the lane. Replaces a
    /// `Save` of the same file still waiting, see the crate docs.
    Save {
        file: FileId,
        revision: Revision,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
    },
    /// Writes `document` as a new file where `to` says, and makes `file`
    /// refer to it from then on, letting go of the old file. Without a
    /// `file`, e.g. for a design never saved, the new file is opened as a
    /// new [`FileId`]. Natively the new file's lock is taken and the old
    /// one's let go of; refused if another editor has the design at the
    /// path open: `file` stays as it was. On the web, `file` keeps its
    /// store entry for auto-saves.
    SaveAs {
        file: Option<FileId>,
        to: SaveTo,
        revision: Revision,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
    },
    /// Appends `document`, the editor's state at `revision`, to the
    /// sidecar of `file`, or its store entry if it's a new design. Never to
    /// the design's own file. Replaces an `AutoSave` of the same file still
    /// waiting, like `Save` does: only the newest state matters.
    AutoSave {
        file: FileId,
        revision: Revision,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
    },
    /// Empties the sidecar of `file`, or its store entry, which stays
    /// held for later auto-saves: the user doesn't want what a crashed
    /// session left in it, see [`Opened::recovered`], or the editor is
    /// back at the state last saved, so what was auto-saved of edits
    /// since is undone.
    DiscardRecovery { file: FileId },
    /// Forgets `file`, releasing its lock, and lets go of its sidecar (or
    /// store entry) as `closing` says.
    Close { file: FileId, closing: Closing },
    /// The UI gave up on the open tagged `id`: closes the file it opened,
    /// if it did. Sent before the UI asks for anything else, so the lock
    /// is let go of before a later open of the same design needs it.
    Abandon { id: OpenId },
    /// Reads the recent files list.
    LoadRecent,
    /// Lists the new designs left behind by sessions that crashed: store
    /// entries nobody holds with something in them, and on the web the
    /// designs downloaded and closed since. Deletes empty ones.
    ListRecovered,
    /// Opens a store entry listed by [`Request::ListRecovered`] as a new
    /// design, answered with [`Response::Opened`]. Its file refers to the
    /// entry, which auto-saves go to, like one made by [`Request::New`].
    OpenRecovered { id: OpenId, path: PathBuf },
    /// On the web without the File System Access API: appends `document`,
    /// the design as the page just downloaded it at `revision`, to `file`'s
    /// store entry, marked so (see [`Recovered::downloaded`]). The page is
    /// never told whether the user kept the download, so the entry holds
    /// on to it, rather than being emptied as by a save, and a clean close
    /// goes back to it, see [`Request::Close`]. So it's never replaced
    /// while it waits. Answered with [`Response::AutoSaved`].
    KeepDownload {
        file: FileId,
        revision: Revision,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
    },
    /// Deletes a store entry listed by [`Request::ListRecovered`]. Unless,
    /// on the web, it holds changes never saved on top of the design as
    /// downloaded (see [`Request::KeepDownload`]): then it goes back to that, as
    /// for a clean close, and is kept, to be listed as downloaded.
    DiscardRecovered { path: PathBuf },
    /// Replaces the stored recent files list. Replaces a `WriteRecent`
    /// still waiting in the queue, unless a `LoadRecent` is queued after it.
    WriteRecent { entries: Vec<RecentFile> },
    /// Writes `bodies` as a 3MF package titled `title`, the design's
    /// name, where `to` says (see [`three_mf`]), answered with
    /// [`Response::Exported`]: natively a new file at a path, replacing
    /// one there only if `overwrite`, written next to it and renamed over
    /// it; on the web the file the user picked in the save picker. Never
    /// replaced, and nothing to do with the open designs' files.
    Export {
        to: SaveTo,
        title: String,
        bodies: Vec<three_mf::Body>,
    },
    /// Does nothing, answered once everything sent before it is done, e.g.
    /// before quitting.
    Flush,
    /// Reads the settings.
    LoadSettings,
    /// Replaces the stored settings. Replaces a `WriteSettings` still
    /// waiting in the queue, unless a `LoadSettings` is queued after it.
    WriteSettings { settings: Settings },
}

/// The answer to a [`Request`]. Errors are the messages to show.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Answers [`Request::Open`] and [`Request::OpenRecovered`] with the
    /// path opened, made absolute, or the store entry's. A file picked on
    /// the web has none.
    Opened {
        id: OpenId,
        path: Option<PathBuf>,
        result: Result<Opened, String>,
    },
    /// Answers [`Request::New`] with the store entry made.
    Created {
        id: OpenId,
        result: Result<FileId, String>,
    },
    /// Answers [`Request::Save`] with its `revision`, which is saved if
    /// the result is `Ok`. A `Save` replaced by a newer one isn't answered.
    Saved {
        file: FileId,
        revision: Revision,
        result: Result<(), SaveError>,
    },
    /// Answers [`Request::SaveAs`] with its `file` and `revision`, and
    /// where it wrote: a path is made absolute.
    SavedAs {
        file: Option<FileId>,
        to: Chosen,
        revision: Revision,
        result: Result<SavedAs, SaveError>,
    },
    /// Answers [`Request::AutoSave`] and [`Request::KeepDownload`]. One
    /// replaced by a newer one isn't answered.
    AutoSaved {
        file: FileId,
        revision: Revision,
        result: Result<(), String>,
    },
    RecoveryDiscarded {
        file: FileId,
        result: Result<(), String>,
    },
    Closed {
        file: FileId,
        result: Result<(), String>,
    },
    /// Answers [`Request::Abandon`]; `Ok` too if the open failed.
    Abandoned {
        id: OpenId,
        result: Result<(), String>,
    },
    /// The recent files, newest first, and the user's home directory for
    /// showing their paths. Empty if the list couldn't be read. Files known
    /// not to be there are kept, since they may be on a drive that isn't
    /// mounted just now, and marked unavailable.
    RecentLoaded {
        entries: Vec<recent::Listed>,
        home: Option<PathBuf>,
    },
    RecentWritten {
        result: Result<(), String>,
    },
    /// Answers [`Request::ListRecovered`], newest first. Also follows the
    /// answer to a request that may have changed which there are: an
    /// [`Request::OpenRecovered`], a [`Request::DiscardRecovered`], or a
    /// [`Request::Close`] or [`Request::Abandon`] of a new design's store
    /// entry (on the web, of any design's), which may be left holding it.
    RecoveredListed {
        designs: Vec<Recovered>,
    },
    RecoveredDiscarded {
        path: PathBuf,
        result: Result<(), String>,
    },
    /// Answers [`Request::Export`] with where it wrote: a path is made
    /// absolute.
    Exported {
        to: Chosen,
        result: Result<(), String>,
    },
    Flushed,
    /// The settings, defaulted where they couldn't be read.
    SettingsLoaded {
        settings: Settings,
    },
    SettingsWritten {
        result: Result<(), String>,
    },
}

impl Request {
    /// The answer to this request should handling it fail with `error`:
    /// every request has one.
    pub(crate) fn failed(self, error: String) -> Response {
        match self {
            Request::Open { id, from } => Response::Opened {
                id,
                path: from.into_path(),
                result: Err(error),
            },
            Request::OpenRecovered { id, path } => Response::Opened {
                id,
                path: Some(path),
                result: Err(error),
            },
            Request::New { id } => Response::Created {
                id,
                result: Err(error),
            },
            Request::Save { file, revision, .. } => Response::Saved {
                file,
                revision,
                result: Err(SaveError::Failed(error)),
            },
            Request::SaveAs {
                file, to, revision, ..
            } => Response::SavedAs {
                file,
                to: to.into(),
                revision,
                result: Err(SaveError::Failed(error)),
            },
            Request::AutoSave { file, revision, .. }
            | Request::KeepDownload { file, revision, .. } => Response::AutoSaved {
                file,
                revision,
                result: Err(error),
            },
            Request::DiscardRecovery { file } => Response::RecoveryDiscarded {
                file,
                result: Err(error),
            },
            Request::Close { file, .. } => Response::Closed {
                file,
                result: Err(error),
            },
            Request::Abandon { id } => Response::Abandoned {
                id,
                result: Err(error),
            },
            Request::LoadRecent => Response::RecentLoaded {
                entries: Vec::new(),
                home: None,
            },
            Request::WriteRecent { .. } => Response::RecentWritten { result: Err(error) },
            Request::ListRecovered => Response::RecoveredListed {
                designs: Vec::new(),
            },
            Request::DiscardRecovered { path } => Response::RecoveredDiscarded {
                path,
                result: Err(error),
            },
            Request::Export { to, .. } => Response::Exported {
                to: to.into(),
                result: Err(error),
            },
            Request::Flush => Response::Flushed,
            Request::LoadSettings => Response::SettingsLoaded {
                settings: Settings::default(),
            },
            Request::WriteSettings { .. } => Response::SettingsWritten { result: Err(error) },
        }
    }

    /// [`Request::failed`], for a request about to be moved into its
    /// handler: keeps only what the answer needs.
    pub(crate) fn failure(&self) -> impl FnOnce(String) -> Response + use<> {
        let request = match self {
            // The answer has none of the list.
            Request::WriteRecent { .. } => Request::WriteRecent {
                entries: Vec::new(),
            },
            // Nor of the bodies, which may be large.
            Request::Export { to, title, .. } => Request::Export {
                to: to.clone(),
                title: title.clone(),
                bodies: Vec::new(),
            },
            // Snapshots are shared, so this clone is cheap.
            request => request.clone(),
        };
        move |error| request.failed(error)
    }
}

/// What [`Request::Close`] keeps of what was auto-saved of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Closing {
    /// The clean close: the sidecar (or store entry) is emptied and
    /// deleted, as the design is saved, or the user chose not to save it.
    /// Unless, on the web, it holds the design as downloaded (see
    /// [`Request::KeepDownload`]), which may be the only copy if the
    /// download wasn't kept: then it's rolled back to the newest such
    /// record, dropping the auto-saves of later edits, and kept, to be
    /// listed as downloaded.
    Clean,
    /// Deleted only if there's nothing in it, keeping what it holds to be
    /// recovered later, e.g. changes offered back and not answered yet.
    Keep,
}

/// A time in seconds since the Unix epoch, as the recent files list and
/// the store keep them. User data: it may be anything, so arithmetic on it
/// is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixSeconds(pub i64);

impl UnixSeconds {
    /// The seconds from `earlier` to `self`, unless that overflows.
    pub fn checked_since(self, earlier: UnixSeconds) -> Option<i64> {
        self.0.checked_sub(earlier.0)
    }
}

/// Why a save failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveError {
    /// The file was changed by someone else since it was opened or last
    /// saved, so saving would lose their changes. Nothing was written.
    Conflict,
    /// Anything else, as the message to show.
    Failed(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Conflict => f.write_str(
                "the file was changed elsewhere since it was opened or saved. \
                 Save As keeps both versions.",
            ),
            SaveError::Failed(error) => f.write_str(error),
        }
    }
}

impl From<FileError> for SaveError {
    fn from(error: FileError) -> Self {
        match error {
            FileError::Conflict => SaveError::Conflict,
            error => SaveError::Failed(error.to_string()),
        }
    }
}

/// A design written by [`Request::SaveAs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedAs {
    /// Refers to the new file from now on: the request's `file` if it had
    /// one, a new one otherwise.
    pub file: FileId,
    /// [`Access::ReadOnly`] if the new file's lock couldn't be taken, e.g.
    /// in a directory the lock file can't be created in.
    pub access: Access,
    /// Whether what a crashed session left, offered as [`Opened::recovered`]
    /// and not answered yet, is still offered: saved over the design itself,
    /// it is; saved as another file, it stays with the design it's of, to
    /// be offered when that's next opened.
    pub offered: bool,
}

/// A document the lane opened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opened {
    /// Refers to the file in later requests, including [`Request::Close`],
    /// which every open needs, read-only or not.
    pub file: FileId,
    #[serde(with = "varde_document::codec::document")]
    pub document: Document,
    pub access: Access,
    /// What a session that crashed auto-saved of the design, if it differs
    /// from `document`: the user's unsaved changes, to offer back. Only
    /// looked for when the design is opened for editing. Saves leave it in
    /// the sidecar until the user answers: [`Request::DiscardRecovery`], or
    /// an [`Request::AutoSave`] once it's restored. An error says why it
    /// couldn't be read, e.g. it was damaged; the design opens anyway.
    pub recovered: Result<Option<Offer>, String>,
    /// For a store entry opened with [`Request::OpenRecovered`], whether
    /// `document` is the design as downloaded on the web, its newest
    /// record, rather than changes never saved: see
    /// [`Recovered::downloaded`], which may be out of date by now.
    pub downloaded: bool,
}

impl Opened {
    /// `document`, opened as `file` for editing with nothing to offer, as
    /// a store entry and on the web a design the user picked are:
    /// `downloaded` says whether it's the design as downloaded.
    pub(crate) fn editable(file: FileId, document: Document, downloaded: bool) -> Self {
        Self {
            file,
            document,
            access: Access::Edit,
            recovered: Ok(None),
            downloaded,
        }
    }
}

/// Unsaved changes a crashed session auto-saved of a design, offered to
/// the user as [`Opened::recovered`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    #[serde(with = "varde_document::codec::document")]
    pub document: Document,
    /// Whether the design changed since `document` was auto-saved from
    /// it: saved by another program or session, rewritten, or saved by the
    /// session that auto-saved it, which kept it, e.g. crashing before
    /// emptying the sidecar. Restoring `document` may undo that. Exact:
    /// each auto-save records the saved version of the design it was based
    /// on (see `src/autosave.rs`), compared with the design's as opened.
    pub design_changed: bool,
}

/// A file the user chose in the Open or Save As dialog: natively its path,
/// on the web the file the browser's picker or file input handed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Chosen {
    Path(PathBuf),
    File(Picked),
}

impl Chosen {
    /// Its path, if it isn't a picked file, as [`Response::Opened`] has
    /// it.
    pub(crate) fn into_path(self) -> Option<PathBuf> {
        match self {
            Chosen::Path(path) => Some(path),
            Chosen::File(_) => None,
        }
    }
}

/// Where a [`Request::SaveAs`] writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveTo {
    /// Natively: a path. A file there is replaced if `overwrite`, which the
    /// user agreed to; otherwise it's an error.
    Path { path: PathBuf, overwrite: bool },
    /// On the web: the file the user picked in the save picker, which
    /// asked about replacing it.
    Picked(Picked),
}

impl From<SaveTo> for Chosen {
    fn from(to: SaveTo) -> Self {
        match to {
            SaveTo::Path { path, .. } => Chosen::Path(path),
            SaveTo::Picked(picked) => Chosen::File(picked),
        }
    }
}

/// A file of the user's picked on the web, see [`pick`]. The page keeps
/// what the browser handed over for it, a File System Access handle or
/// the file a file input picked, and posts it to the IO worker along with
/// the request using it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picked {
    /// Tells the page's picked files apart.
    pub id: u64,
    /// The file's name, as the browser gave it, e.g. `design.vrdp`.
    pub name: String,
    /// What the browser handed over for it.
    pub from: PickedFrom,
}

/// What the browser handed over for a [`Picked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PickedFrom {
    /// A File System Access handle: read, and written back on saves.
    Handle,
    /// A file from a file input: read once, so the design opens as a copy.
    Input,
}

/// Whether an opened document may be edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Access {
    /// The lane holds the document's lock until it's closed.
    Edit,
    /// The document is shown but can't be edited, for the reason given.
    ReadOnly(ReadOnly),
}

/// Why a document opened read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadOnly {
    /// Another editor, in this process or another, has it open.
    InUse,
    /// The lock file couldn't be created or locked, e.g. in a read-only
    /// directory. The message says why.
    NoLock(String),
}

impl fmt::Display for ReadOnly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadOnly::InUse => f.write_str("the design is open elsewhere"),
            ReadOnly::NoLock(reason) => f.write_str(reason),
        }
    }
}

// Helpers for tests of files at a path, which only native builds have.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
