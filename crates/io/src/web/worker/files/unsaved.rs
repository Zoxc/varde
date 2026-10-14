//! A design auto-saved to an entry in the store of new designs, and the
//! file of the user's it's saved to, if any.

use varde_document::Document;
use web_sys::FileSystemFileHandle;

use super::super::disk;
use super::super::opfs::Handle;
use crate::SaveError;
use crate::autosave::Held;
use crate::vrdp::{Error as FileError, Known, Preview, check_unchanged, whole_file};

/// A design auto-saved to an entry in the store of new designs.
#[derive(Debug)]
pub(super) struct Unsaved {
    /// Its entry, or why it has none: a design opened from or saved as a
    /// file of the user's still is when the store can't be used, say with
    /// the site's data blocked, only without auto-saves.
    pub(super) entry: Result<Entry, String>,
    /// The file of the user's the design was opened from or saved as, if
    /// it can be written to: saves go there.
    pub(super) disk: Option<Disk>,
    /// The name of the file of the user's the design was opened from or
    /// saved as, kept with its auto-saves; `None` for a new design.
    pub(super) title: Option<String>,
}

/// A design's entry in the store, held.
#[derive(Debug)]
pub(super) struct Entry {
    /// Its file name in the store.
    pub(super) name: String,
    pub(super) held: Held<Handle>,
}

/// A file of the user's a design is saved to, through the File System
/// Access API.
#[derive(Debug)]
pub(super) struct Disk {
    pub(super) handle: FileSystemFileHandle,
    /// What the file held when last read or written, see
    /// [`check_unchanged`]: one read as damaged is never saved to.
    pub(super) known: Known,
}

impl Unsaved {
    /// The design was just saved to its file, so what was auto-saved is
    /// older. Failing to empty it only leaves an older state to be offered
    /// should the tab close; the next save or a clean close tries again.
    pub(super) fn saved(&mut self) {
        if let Ok(entry) = &mut self.entry {
            let _ = entry.held.clear();
        }
    }

    /// A design just saved as `disk`, the file of the user's named
    /// `title`, auto-saved to `entry`.
    pub(super) fn saved_as(entry: Result<Entry, String>, disk: Disk, title: String) -> Self {
        Self {
            entry,
            disk: Some(disk),
            title: Some(title),
        }
    }

    /// The design was just saved as `disk`, the file of the user's named
    /// `title`: saves go there from now on, see [`Unsaved::saved`].
    pub(super) fn saved_to(&mut self, disk: Disk, title: String) {
        self.disk = Some(disk);
        self.title = Some(title);
        self.saved();
    }
}

impl Disk {
    /// Replaces the file with `document` and `previews`, unless someone
    /// else changed it since it was last read or written. Saving through
    /// [`Unsaved`] also empties its auto-saves, see [`Unsaved::saved`].
    pub(super) async fn save(
        &mut self,
        document: &Document,
        previews: &[Preview],
    ) -> Result<(), SaveError> {
        // A file that's no longer a `.vrdp` was changed by someone else:
        // a native save, reading no header, would find other bytes where
        // the last record was. Other errors are `check_unchanged`'s to say,
        // as a native save's are.
        let current = disk::read_handle(&self.handle).await.map_err(|e| match e {
            FileError::Io(e) => SaveError::Failed(e.to_string()),
            _ => SaveError::Conflict,
        })?;
        // No failed save to allow for: the browser replaces the file as a
        // write is closed, a failure leaving it as it was.
        check_unchanged(current.as_slice(), &self.known)?;
        let (bytes, known) = whole_file(document, previews)?;
        disk::write(&self.handle, &bytes)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))?;
        self.known = known;
        Ok(())
    }
}
