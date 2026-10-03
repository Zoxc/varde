//! Browser storage: on the web, designs saved by name in the Origin
//! Private File System, laid out like a folder of designs on disk.
//!
//! ```text
//! designs/<id>.vrdp            new designs never saved (`src/store.rs`)
//! saved/<name>.vrdp            designs saved to browser storage
//! saved/.<name>.vrdp.autosave  their auto-saves and lock
//! downloads.toml               the downloads made of them (`src/downloads.rs`)
//! settings.toml                the settings (`src/settings.rs`)
//! ```
//!
//! A design in browser storage behaves like a native design saved at a
//! path (see `src/native/files.rs`): its `.vrdp` keeps one record per save,
//! appended through a sync access handle taken for the save, so its history
//! is kept, and its auto-saves go to its sidecar, whose handle, held for as
//! long as the design is open, is the lock, see `src/lock.rs`:
//! another tab can't take it, and opens the design read-only, as natively
//! another process can't take the sidecar's OS lock. A tab closed with
//! changes leaves the sidecar behind, offered back as natively after a
//! crash. Neither metadata nor an index: the file name is the design's
//! name, as [`file_name`] makes it, so a design downloads under the name it
//! has in storage, and two can't share one.
//!
//! Saving as another name writes a new file of one record (the sidecar of
//! the new name taken first, so a design open elsewhere is never written
//! over); any file written whole, saved as a name, over a design or copied
//! in, is written under a temporary name, `.<name>.<id>.tmp`, then put in
//! place, and listing puts one left by a tab closed in between in place.
//! Renaming moves the file, keeping its saves. The code over the directory
//! is `folder`, generic over `Dir` (`src/dir.rs`), so it's tested natively;
//! here are the names and what the lane answers with.

use serde::{Deserialize, Serialize};

use crate::{ListedDamage, UnixSeconds, thumbnail};

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod folder;

/// A design saved in browser storage, as listed for the welcome screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDesign {
    /// Its file name there, see [`Chosen::Browser`](crate::Chosen::Browser).
    pub name: String,
    /// When it was last saved, as the browser says the file was last
    /// written, if it says.
    pub saved: Option<UnixSeconds>,
    /// The `sum` of its newest save (see `src/vrdp.rs`), if the end of its
    /// file could be read: it tells any later save apart, so what's shown
    /// of the design, its thumbnail say, is kept till it changes.
    pub sum: Option<u128>,
    /// The thumbnail its last save wrote, if it has one.
    pub thumbnail: Option<thumbnail::Image>,
    /// Where it stands against its downloads.
    pub download: DownloadStatus,
    /// Whether its sidecar holds changes not saved, left by a tab closed
    /// with them, which opening it offers back.
    pub unsaved: bool,
    /// Whether another tab has it open: it opens read-only, and can't be
    /// deleted.
    pub in_use: bool,
    /// Whether it's no design file at all, so it doesn't open. Listing
    /// reads only the end of each file, so damage before that shows as
    /// it's opened.
    pub damage: Option<ListedDamage>,
}

/// Where a design in browser storage stands against its downloads, see
/// `src/downloads.rs`, each with when it was last downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    /// Nothing is recorded for it: what's in the browser is all there is.
    Never,
    /// Saved since it was downloaded, or with changes not saved, or
    /// downloaded with changes not saved then.
    Changed(UnixSeconds),
    /// The download is the design as saved, with no changes since.
    Latest(UnixSeconds),
}

/// A download just recorded of a design in browser storage, or the last
/// one recorded as it's opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastDownload {
    pub time: UnixSeconds,
    /// Whether it's the design as saved then, with no changes since.
    pub latest: bool,
}

/// The directory of browser storage, from the root of the Origin Private
/// File System.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) const DIR: &str = "saved";

/// The most bytes in a design's file name in browser storage: room left
/// within the 255 bytes of a file name for the longest name made from it,
/// its temporary file's (see `folder`), `.{name}.{id}.tmp` with a 64-bit
/// `id` in hex. Its sidecar's, `.{name}.autosave`, is shorter.
pub const MAX_NAME: usize = 255 - ".".len() - ".".len() - 16 - ".tmp".len();

/// Whether `name` is a design's file name in browser storage: a plain
/// name ending in `.vrdp`, in any case, after a stem that doesn't start
/// with a dot, which [`file_name`] never makes, so it's never a sidecar's
/// or a temporary file's, and at most [`MAX_NAME`] bytes.
pub fn is_design_name(name: &str) -> bool {
    use varde_document::EXTENSION;
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !stem.starts_with('.')
        && extension.eq_ignore_ascii_case(EXTENSION)
        && name.len() <= MAX_NAME
        && !name.contains(['/', '\\', '\0'])
}

/// The file name in browser storage of a design called `name`, as typed
/// or shown, with or without the extension: as it downloads (see
/// [`varde_document::name::download_name`]), its stem cut to fit
/// [`MAX_NAME`].
pub fn file_name(name: &str) -> String {
    within(name, "")
}

/// [`file_name`] of `name` with `suffix` after its stem, such as " (2)",
/// the stem cut, rather than the suffix, to fit [`MAX_NAME`].
pub(crate) fn within(name: &str, suffix: &str) -> String {
    use varde_document::name::download_name;
    let whole = download_name(name);
    let stem = whole
        .rsplit_once('.')
        .map_or(whole.as_str(), |(stem, _)| stem);
    let extension = varde_document::EXTENSION;
    // What's left for the stem, past the suffix and the extension: never
    // less than none, as suffixes are short.
    let room = MAX_NAME.saturating_sub(suffix.len() + 1 + extension.len());
    let mut cut = room.min(stem.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    // Made again, as cutting may leave what a file name can't end in.
    download_name(&format!("{}{suffix}.{extension}", &stem[..cut]))
}

#[cfg(test)]
mod tests;
