//! The store of new designs: where a design never saved lives until it's
//! first saved as a file of the user's.
//!
//! New designs have no directory of their own for a sidecar, so each gets
//! an entry `<data dir>/designs/<id>.vrdp` instead, e.g. in
//! `~/.local/share/varde-cad/designs`. An entry is a `.vrdp` like any other,
//! auto-saved to through the lock its editor holds on it for as long as the
//! design is open, like a sidecar (see `src/native/sidecar.rs`). The first Save As
//! writes the user's file and deletes the entry; from then on the file's
//! sidecar takes over. A clean close deletes it too, when the user chose
//! not to save the design, or it was never edited.
//!
//! An entry is made as soon as the design is (eagerly, rather than on its
//! first auto-save): auto-saves then always have a file to go to, so they
//! are one request that coalesces per file like any other, and the first
//! Save As moves the design from its entry to the user's file the way it
//! moves any design from one file to another. The cost is an empty file
//! per new design, deleted on close, or when listed after a crash.
//!
//! An entry left behind with something in it, and not locked, is from a
//! session that crashed: each lane lists them (natively
//! `native::store::list`, on the web the worker's `Files::list`), and the
//! welcome screen offers them as recovered designs. On the web, one is also left behind on
//! purpose by a design downloaded and then closed, see
//! [`Recovered::downloaded`]. Entries are private to the user, so on Unix
//! they're made readable by the owner only.
//!
//! On the web the store is the directory `designs` in the Origin Private
//! File System, laid out and used the same way, see `src/opfs.rs`. What's
//! here is what both lanes use: [`Recovered`], the checks on entry names
//! and the constants; making, opening, listing and discarding entries at a
//! path is `src/native/store.rs`'s.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use varde_document::EXTENSION;

use crate::UnixSeconds;
use crate::autosave::AutoSaved;

/// How many lost names `create` skips before giving up, and how many
/// taken ones for each.
pub(crate) const ATTEMPTS: u32 = 16;

/// Whether `name` is an entry's file name: a plain name ending in `.vrdp`.
pub(crate) fn is_entry_name(name: &str) -> bool {
    let stem = name
        .strip_suffix(EXTENSION)
        .and_then(|name| name.strip_suffix('.'));
    stem.is_some_and(|stem| !stem.is_empty()) && !name.contains(['/', '\\', '\0'])
}

/// Why a request for a recovered design fails without a store.
pub(crate) const NO_RECOVERED: &str = "there are no recovered designs";

/// Why making a new design fails without a store.
pub(crate) const NO_STORE: &str = "there's no place to keep new designs";

/// The file name of the entry at `path` in the store `dir`, if it is one.
pub(crate) fn entry_in<'a>(dir: &Path, path: &'a Path) -> Option<&'a str> {
    if path.parent() != Some(dir) {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    is_entry_name(name).then_some(name)
}

/// Where new designs are kept, if there is a data directory to put them in.
#[cfg(not(target_arch = "wasm32"))]
pub fn designs() -> Option<PathBuf> {
    crate::native::project_dirs().map(|dirs| dirs.data_dir().join("designs"))
}

/// On the web, the directory `designs` in the Origin Private File System.
#[cfg(target_arch = "wasm32")]
pub fn designs() -> Option<PathBuf> {
    Some(PathBuf::from("designs"))
}

/// A new design left behind by a session that crashed, or on the web by a
/// tab closed with it open, or a design downloaded and closed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recovered {
    /// Its store entry, to open or discard it by.
    pub path: PathBuf,
    /// When it was last auto-saved, if that's known.
    pub modified: Option<UnixSeconds>,
    /// The file name of the design it was opened from, if any: on the web
    /// designs opened from the user's files are auto-saved to entries too.
    pub name: Option<String>,
    /// Whether what's in it is the design as it was last downloaded, on the
    /// web, rather than changes never saved: kept in case the download
    /// didn't finish, see [`Request::KeepDownload`](crate::Request::KeepDownload).
    pub downloaded: bool,
}

/// The file name of the entry at `path` in the store `dir`, to open or
/// discard as a recovered design: refused for anything but an entry there.
pub(crate) fn listed_entry<'a>(dir: &Path, path: &'a Path) -> Result<&'a str, String> {
    entry_in(dir, path).ok_or_else(|| format!("{} isn't a recovered design", path.display()))
}

impl Recovered {
    /// The entry at `path`, last written at `modified`, whose newest record
    /// is `saved`.
    pub(crate) fn new(path: PathBuf, modified: Option<UnixSeconds>, saved: AutoSaved) -> Self {
        Self {
            path,
            modified,
            name: saved.name,
            downloaded: saved.origin.is_download(),
        }
    }
}

/// Sorts `found` newest first, as they're listed; unknown times last.
pub(crate) fn newest_first(found: &mut [Recovered]) {
    found.sort_by_key(|design| Reverse(design.modified));
}

#[cfg(test)]
mod tests;
