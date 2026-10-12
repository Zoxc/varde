//! The stored list of recently opened files.
//!
//! The list is kept in `recent.toml` in the platform config directory, e.g.
//! `~/.config/varde-cad/recent.toml`, as up to [`MAX`] `[[file]]` tables
//! with a `path` and an `opened` time in seconds since the Unix epoch,
//! newest first. The file is user data: times may be anything, so all
//! arithmetic on them is bounded or checked. Reading and writing it is
//! `src/native/recent.rs`'s.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::UnixSeconds;

/// The most files kept.
pub const MAX: usize = 100;

/// The largest `recent.toml` read: [`MAX`] entries of 8 KiB each, room for
/// the longest paths.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_BYTES: u64 = MAX as u64 * 8 * 1024;

/// A recently opened file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: PathBuf,
    /// When the file was last opened.
    pub opened: UnixSeconds,
}

/// A file of the list as loaded, and whether it's there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub entry: RecentFile,
    /// False for a file known not to be there when the list was loaded,
    /// e.g. on a drive that wasn't mounted. Kept, and shown as unavailable.
    pub available: bool,
}
