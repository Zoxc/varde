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
//! On the web, designs are saved in browser storage, by name, laid out
//! like a folder of designs with their sidecars (`src/browser.rs`, over the
//! directories of `src/dir.rs`), and the downloads made of them are
//! recorded (`src/downloads.rs`).
//!
//! Natively the lane is a thread for the whole app
//! (`src/native/thread.rs`). The web has no path based file system: there
//! the lane is a Web Worker (`src/web.rs`, handling requests in
//! `src/web/worker/files.rs`) keeping
//! designs and auto-saves in the Origin Private File System (`src/opfs.rs`), and
//! requests and responses cross to it as bytes (see `src/wire.rs`). Both
//! are [`lane`]. Files of the user's are picked on the web by the page,
//! see [`pick`], and handed to the worker as a [`Picked`].
//!
//! [`three_mf`] writes bodies' meshes as a 3MF package, for printing, and
//! [`thumbnail`] encodes and decodes the PNG thumbnails saves write.
//! [`panicked`] records the first panic of a session, from the panic hook
//! rather than the lane, for the welcome screen to show next time.

// Modules at the root are compiled for both targets. What only native
// builds have, with a path based file system, is under `native`; what only
// the web has is under `web`, split into the page's side and the IO
// worker's. The plain Rust halves of what only the web uses (`js`, `opfs`,
// `wire`) are at the root, tested natively too.
mod autosave;
pub mod browser;
#[cfg(any(target_arch = "wasm32", test))]
mod dir;
#[cfg(any(target_arch = "wasm32", test))]
mod downloads;
#[cfg(any(target_arch = "wasm32", test))]
mod js;
mod lock;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod open;
#[cfg(any(target_arch = "wasm32", test))]
mod opfs;
pub mod panicked;
pub mod pick;
mod queue;
pub mod recent;
pub mod settings;
pub mod storage;
mod store;
pub mod three_mf;
pub mod thumbnail;
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

pub use crate::browser::{BrowserDesign, DownloadStatus, LastDownload};
pub use crate::panicked::Panic;
pub use crate::recent::RecentFile;
pub use crate::settings::Settings;
pub use crate::store::{ListedDamage, Recovered};
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
    /// The panic recorded, `panic.toml` (see [`panicked`]). Unused on the
    /// web, which keeps it on the page.
    pub panic: Option<PathBuf>,
}

impl Stores {
    /// The user's: the recent files list and the settings in the platform
    /// config directory and new designs and the panic recorded in the data
    /// directory, e.g. `~/.config/varde-cad` and
    /// `~/.local/share/varde-cad/designs`. On the
    /// web there's no recent files list, and the settings and new designs
    /// are kept in `settings.toml` and `designs` in the Origin Private File
    /// System.
    pub fn user() -> Self {
        Self {
            recent: recent::store(),
            settings: settings::store(),
            designs: store::designs(),
            panic: panicked::store(),
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
    /// [`Opened::recovered`]. On the web, a design in browser storage
    /// ([`Chosen::Browser`]) opens the same way, its sidecar next to it
    /// there. The file the user picked has no path to answer with: from a
    /// [`PickedFrom::Handle`], its file refers to the picked file from then
    /// on, which [`Request::Save`] writes, and auto-saves go to a store
    /// entry made for it, as for a new design; from a file input
    /// ([`PickedFrom::Input`]), it's copied into browser storage under its
    /// name, made unique, and opened from there ([`Opened::browser`]), or,
    /// should copying it fail, opened as a copy, as a new design
    /// ([`Opened::not_copied`]).
    Open { id: OpenId, from: Chosen },
    /// Opens the save a search found past damage in `file`'s design
    /// instead of the one it opened, answered with [`Response::Opened`]
    /// for the same `file`: the user chose the [`FoundSave`] its
    /// [`Opened::damage`] offered, named by its `found` tail. Its answer's
    /// damage is [`DamageKind::Damaged`] with nothing found, its time the
    /// found save's, and the recovery offer is looked for again, against
    /// that save. `id` tags the answer; the open's own keeps
    /// [`Request::Abandon`] of it closing `file`. Refused for a save that
    /// wasn't offered, or once it's opened.
    OpenFound {
        id: OpenId,
        file: FileId,
        found: vrdp::Tail,
    },
    /// Creates and locks an entry in the store of new designs for a new
    /// design, which its auto-saves go to until it's first saved as a file
    /// of the user's. Answered with [`Response::Created`].
    New { id: OpenId },
    /// Appends `document`, the editor's state at `revision`, to `file`,
    /// with its `thumbnail`, if it has one, as the save's previews (see
    /// [`thumbnail`]). Encoding, locking and writing all happen in the
    /// lane. Replaces a `Save` of the same file still waiting, see the
    /// crate docs.
    Save {
        file: FileId,
        revision: Revision,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
        thumbnail: Option<thumbnail::Thumbnail>,
    },
    /// Writes `document` as a new file where `to` says, and makes `file`
    /// refer to it from then on, letting go of the old file. Without a
    /// `file`, e.g. for a design never saved, the new file is opened as a
    /// new [`FileId`]. The new file's lock is taken and the old one's let
    /// go of; refused if another editor has the design there open: `file`
    /// stays as it was. On the web, a design going to a file of the user's
    /// keeps a store entry for auto-saves, and one going to browser storage
    /// has its sidecar there. The `thumbnail` is written as for a
    /// [`Request::Save`].
    SaveAs {
        file: Option<FileId>,
        to: SaveTo,
        revision: Revision,
        #[serde(with = "varde_document::codec::snapshot")]
        document: Snapshot,
        thumbnail: Option<thumbnail::Thumbnail>,
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
    /// Reads the thumbnails of the designs at `paths`, the recent files
    /// (see [`thumbnail`]), answered with [`Response::ThumbnailsLoaded`].
    /// Natively a design another program is writing just then has none;
    /// the web has no paths to read.
    LoadThumbnails { paths: Vec<PathBuf> },
    /// Lists the new designs left behind by sessions that crashed: store
    /// entries nobody holds with something in them. Deletes empty ones.
    ListRecovered,
    /// Opens a store entry listed by [`Request::ListRecovered`] as a new
    /// design, answered with [`Response::Opened`]. Its file refers to the
    /// entry, which auto-saves go to, like one made by [`Request::New`].
    OpenRecovered { id: OpenId, path: PathBuf },
    /// Deletes a store entry listed by [`Request::ListRecovered`].
    DiscardRecovered { path: PathBuf },
    /// On the web: lists the designs saved in browser storage, answered
    /// with [`Response::BrowserListed`]. Natively there are none.
    ListBrowser,
    /// On the web: gives `file`, a design in browser storage, the file name
    /// `name` there (see [`Chosen::Browser`]), keeping its saves and what
    /// was auto-saved of it, and its downloads recorded. Refused if a
    /// design is called that, or someone has one of that name open.
    Rename { file: FileId, name: String },
    /// On the web: deletes the design `name` in browser storage, with what
    /// was auto-saved of it and the downloads recorded. Refused while it's
    /// open, in this tab or another.
    DeleteFromBrowser { name: String },
    /// On the web: records that `file`, a design in browser storage, was
    /// just downloaded, see [`DownloadStatus`]: as it was last saved, or
    /// with changes not saved yet if `edited`. Answered with
    /// [`Response::DownloadRecorded`]. Nothing to record for another file.
    RecordDownload { file: FileId, edited: bool },
    /// On the web: reads the design `name` in browser storage whole, as
    /// it's saved, to download it, and records the download. Answered with
    /// [`Response::DownloadedFromBrowser`].
    DownloadFromBrowser { name: String },
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
    /// Reads the panic a session recorded (see [`panicked`]), answered
    /// with [`Response::PanicLoaded`]. On the web the page answers it.
    LoadPanic,
    /// Deletes the panic recorded, if it's still `panic` rather than one
    /// recorded since. On the web the page answers it.
    DiscardPanic { panic: Panic },
}

/// The answer to a [`Request`]. Errors are the messages to show.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "a response is made once per request and handled at once"
)]
pub enum Response {
    /// Answers [`Request::Open`], [`Request::OpenRecovered`] and
    /// [`Request::OpenFound`] with the path opened, made absolute, or the
    /// store entry's. A file picked on the web has none, nor has a failed
    /// [`Request::OpenFound`].
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
    /// Answers [`Request::AutoSave`]. One replaced by a newer one isn't
    /// answered.
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
    /// Answers [`Request::LoadThumbnails`] with the thumbnails found, in
    /// the order asked, each with its path as asked. A design without one,
    /// or that couldn't be read, isn't listed.
    ThumbnailsLoaded {
        thumbnails: Vec<(PathBuf, thumbnail::Thumbnail)>,
    },
    /// Answers [`Request::ListRecovered`], newest first. Also follows the
    /// answer to a request that may have changed which there are: an
    /// [`Request::OpenRecovered`], a [`Request::DiscardRecovered`], or a
    /// [`Request::Close`] or [`Request::Abandon`] of a new design's store
    /// entry (on the web, of any design's that has one), which may be left
    /// holding it.
    RecoveredListed {
        designs: Vec<Recovered>,
    },
    RecoveredDiscarded {
        path: PathBuf,
        result: Result<(), String>,
    },
    /// Answers [`Request::ListBrowser`], newest first.
    BrowserListed {
        designs: Vec<BrowserDesign>,
    },
    Renamed {
        file: FileId,
        name: String,
        result: Result<(), String>,
    },
    DeletedFromBrowser {
        name: String,
        result: Result<(), String>,
    },
    /// Answers [`Request::RecordDownload`] with the download recorded, if
    /// one was: `None` for a design not in browser storage.
    DownloadRecorded {
        file: FileId,
        result: Result<Option<LastDownload>, String>,
    },
    /// Answers [`Request::DownloadFromBrowser`] with the design's file,
    /// whole, to download whether or not the download could be recorded,
    /// and why it couldn't, if it couldn't.
    DownloadedFromBrowser {
        name: String,
        result: Result<Vec<u8>, String>,
        not_recorded: Option<String>,
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
    /// The panic recorded, if there's one that can be read.
    PanicLoaded {
        panic: Option<Panic>,
    },
    PanicDiscarded {
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
            Request::OpenFound { id, .. } => Response::Opened {
                id,
                path: None,
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
            Request::AutoSave { file, revision, .. } => Response::AutoSaved {
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
            Request::LoadThumbnails { .. } => Response::ThumbnailsLoaded {
                thumbnails: Vec::new(),
            },
            Request::WriteRecent { .. } => Response::RecentWritten { result: Err(error) },
            Request::ListRecovered => Response::RecoveredListed {
                designs: Vec::new(),
            },
            Request::DiscardRecovered { path } => Response::RecoveredDiscarded {
                path,
                result: Err(error),
            },
            Request::ListBrowser => Response::BrowserListed {
                designs: Vec::new(),
            },
            Request::Rename { file, name } => Response::Renamed {
                file,
                name,
                result: Err(error),
            },
            Request::DeleteFromBrowser { name } => Response::DeletedFromBrowser {
                name,
                result: Err(error),
            },
            Request::RecordDownload { file, .. } => Response::DownloadRecorded {
                file,
                result: Err(error),
            },
            Request::DownloadFromBrowser { name } => Response::DownloadedFromBrowser {
                name,
                result: Err(error),
                not_recorded: None,
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
            Request::LoadPanic => Response::PanicLoaded { panic: None },
            Request::DiscardPanic { .. } => Response::PanicDiscarded { result: Err(error) },
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
            // Nor of the thumbnail's pixels, nor of the paths.
            Request::Save {
                file,
                revision,
                document,
                ..
            } => Request::Save {
                file: *file,
                revision: *revision,
                document: document.clone(),
                thumbnail: None,
            },
            Request::SaveAs {
                file,
                to,
                revision,
                document,
                ..
            } => Request::SaveAs {
                file: *file,
                to: to.clone(),
                revision: *revision,
                document: document.clone(),
                thumbnail: None,
            },
            Request::LoadThumbnails { .. } => Request::LoadThumbnails { paths: Vec::new() },
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

    /// Now, by the system clock natively and `Date.now()` on the web,
    /// rounded down to the second. A clock can be wrong, so it's only for
    /// showing.
    pub(crate) fn now() -> UnixSeconds {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            UnixSeconds(match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
                // Before the epoch: at most `i64::MAX` seconds back, so
                // negating it can't overflow.
                Err(before) => {
                    i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |secs| -secs)
                }
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Milliseconds. `as` saturates, and makes NaN 0.
            UnixSeconds((js_sys::Date::now() / 1000.0).floor() as i64)
        }
    }
}

/// Why a save failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveError {
    /// The file was changed by someone else since it was opened or last
    /// saved, so saving would lose their changes. Nothing was written.
    Conflict,
    /// The file was found damaged when it was opened, past a save that
    /// couldn't be read (see [`DamageKind::Damaged`]), so it's never saved
    /// to: saving would cut off what can still be got out of it. Saved
    /// as another file instead, Save acts as Save As. Nothing was written.
    OpenedDamaged,
    /// The file was damaged since it was opened or last saved: saving
    /// could cut off what's still readable. Nothing was written; Save As
    /// keeps the design.
    Damaged,
    /// On the web, saving as a design in browser storage without replacing
    /// one ([`SaveTo::Browser`]): a design there has the name, as another
    /// tab may have saved one since the app listed them. Nothing was
    /// written; the app asks whether to replace it.
    Taken,
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
            SaveError::OpenedDamaged => f.write_str(
                "the file is damaged, so it can only be saved as another file. \
                 Save As keeps the design.",
            ),
            SaveError::Damaged => f.write_str(
                "the file was damaged since it was opened or saved. Save As keeps \
                 the design.",
            ),
            SaveError::Taken => f.write_str("a design of that name is in browser storage already"),
            SaveError::Failed(error) => f.write_str(error),
        }
    }
}

impl From<FileError> for SaveError {
    fn from(error: FileError) -> Self {
        match error {
            FileError::Conflict => SaveError::Conflict,
            FileError::OpenedDamaged => SaveError::OpenedDamaged,
            FileError::Damaged => SaveError::Damaged,
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
    /// couldn't be read, e.g. it was damaged; the design opens anyway. If
    /// its records are all damaged, or reading it failed, it's kept until
    /// [`Request::DiscardRecovery`] or the clean close
    /// ([`RecoveryError::kept`]): auto-saves are refused while it's kept,
    /// never starting it over, and saves leave it be.
    pub recovered: Result<Option<Offer>, RecoveryError>,
    /// On the web, the file name in browser storage of the design opened
    /// from there, or copied there from a file input to be opened.
    pub browser: Option<String>,
    /// On the web, why a file from a file input wasn't copied into browser
    /// storage, if it wasn't, say with the site's data blocked: it opens
    /// as a copy, as a new design never saved, known by the file's name.
    pub not_copied: Option<String>,
    /// For a design in browser storage, its last download recorded, if
    /// there's one, see [`Request::RecordDownload`].
    pub download: Option<LastDownload>,
    /// How reading the design's file, or the store entry, found it
    /// damaged, if it did, and so which save `document` is: `None` for an
    /// intact file, or one whose torn tail (an interrupted save, say) the
    /// next save cuts off.
    pub damage: Option<Damage>,
}

impl Opened {
    /// `document`, opened as `file` for editing with nothing to offer, as
    /// a store entry and on the web a design the user picked are: `damage`
    /// says how reading found the file.
    pub(crate) fn editable(file: FileId, document: Document, damage: Option<Damage>) -> Self {
        Self {
            file,
            document,
            access: Access::Edit,
            recovered: Ok(None),
            browser: None,
            not_copied: None,
            download: None,
            damage,
        }
    }
}

/// How reading a file found it damaged: a design's file, a sidecar or a
/// store entry, see [`Opened::damage`] and [`Offer::damage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Damage {
    pub kind: DamageKind,
    /// When the save opened was written, by the writer's clock: only for
    /// showing, as a clock can be wrong.
    pub time: UnixSeconds,
    /// How many bytes of the file can't be read.
    pub unreadable: u64,
}

/// What [`Damage`] there is, and so what's opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageKind {
    /// Some earlier saves are damaged, and were stepped over: the newest
    /// save opened. Saves go on as usual.
    Bridged,
    /// The newest save is damaged, its header intact: the one before it
    /// opened. A save goes after the damaged one, cutting nothing off.
    NewestDamaged,
    /// The file is damaged past the save opened, the newest the file
    /// proves, in a way only a search could get past, if anything could.
    /// Shown only once the user agrees, and a design's file is never saved
    /// to, only saved as another file ([`SaveError::OpenedDamaged`]); a
    /// store entry's damage is cut off by its next auto-save.
    Damaged {
        /// The newest save the search found after the damage, if it's
        /// another than the one opened and can be read, which
        /// [`Request::OpenFound`] opens instead. Never for a store entry
        /// or a sidecar.
        found: Option<FoundSave>,
    },
}

/// A save found by searching a damaged design file, see
/// [`DamageKind::Damaged`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundSave {
    /// Names it to [`Request::OpenFound`].
    pub tail: vrdp::Tail,
    /// When it was written, by the writer's clock.
    pub time: UnixSeconds,
}

impl Damage {
    /// The damage `report` says reading found, if any, with `found`, the
    /// save a search found that [`Request::OpenFound`] can open.
    pub(crate) fn of(report: &vrdp::Report, found: Option<FoundSave>) -> Option<Damage> {
        let kind = match report.outcome {
            vrdp::Outcome::Intact | vrdp::Outcome::TornTail => return None,
            vrdp::Outcome::Bridged => DamageKind::Bridged,
            vrdp::Outcome::NewestDamaged(_) => DamageKind::NewestDamaged,
            vrdp::Outcome::Damaged { .. } => DamageKind::Damaged { found },
        };
        Some(Damage {
            kind,
            time: report.time,
            unreadable: report.unreadable,
        })
    }

    /// How reading a design's file found it, as [`Damage::of`], `found`
    /// being the search's newest save, opened in case it's chosen.
    pub(crate) fn of_design(read: &vrdp::WithFound) -> Option<Damage> {
        let found = read.found.as_ref().map(|found| FoundSave {
            tail: found.tail,
            time: found.report.time,
        });
        Damage::of(&read.opened.report, found)
    }
}

/// Why what a crashed session auto-saved of a design couldn't be offered,
/// see [`Opened::recovered`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryError {
    /// What's wrong, as the message to show.
    pub message: String,
    /// Whether it's kept for whatever can still be got out of it: its
    /// records are framed but none intact, or reading it failed. Then the
    /// lane refuses auto-saves of the design, keeping it as it is, until
    /// the user answers with [`Request::DiscardRecovery`], as for an
    /// [`Offer`]; the app holds them till then. Otherwise it's of no use
    /// to anyone, and auto-saving starts it over.
    pub kept: bool,
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
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
    /// How reading the sidecar found it damaged, if it did: `document` is
    /// its newest intact auto-save, which may not be the newest.
    pub damage: Option<Damage>,
    /// Whether `document` was auto-saved from a newer save of the design
    /// than the one opened, which couldn't be read: its newest save
    /// damaged, say. The design changed since, then, but `document` may
    /// be the best copy there is.
    pub newer_base: bool,
}

/// A file the user chose in the Open or Save As dialog: natively its path,
/// on the web the file the browser's picker or file input handed over, or
/// a design in browser storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Chosen {
    Path(PathBuf),
    File(Picked),
    /// A design in browser storage, by its file name there, like
    /// `bracket.vrdp`: what the design is called, as
    /// [`varde_document::name::download_name`] names it, so it downloads
    /// by the same name.
    Browser(String),
}

impl Chosen {
    /// Its path, if it's one, as [`Response::Opened`] has it.
    pub(crate) fn into_path(self) -> Option<PathBuf> {
        match self {
            Chosen::Path(path) => Some(path),
            Chosen::File(_) | Chosen::Browser(_) => None,
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
    /// On the web: the design `name` in browser storage, see
    /// [`Chosen::Browser`], replacing one of that name if `overwrite`,
    /// which the user agreed to; otherwise it's [`SaveError::Taken`]. To the
    /// design's own name, it's refused unless `overwrite`, as the app
    /// saves to it instead, and replaces it only once agreed to.
    Browser { name: String, overwrite: bool },
}

impl From<SaveTo> for Chosen {
    fn from(to: SaveTo) -> Self {
        match to {
            SaveTo::Path { path, .. } => Chosen::Path(path),
            SaveTo::Picked(picked) => Chosen::File(picked),
            SaveTo::Browser { name, .. } => Chosen::Browser(name),
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
