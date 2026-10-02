//! The files a lane has open, by the [`FileId`] the UI knows each by:
//! natively `src/native/files.rs`'s, on the web `src/web/worker/files.rs`'s, each keeping
//! its own of what an open file is.

use std::collections::HashMap;
use std::fmt;

use crate::{FileId, OpenId, Request, SaveError};

/// Why a document isn't auto-saved while what a crashed session left
/// can't be read, see [`RecoveryError::kept`](crate::RecoveryError::kept).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) const KEPT: &str =
    "what was auto-saved before is damaged, and kept until it's discarded";

/// Why a [`Request::OpenFound`] is refused.
pub(crate) const NOT_FOUND: &str = "no such save was found in the file";

/// The files a lane has open, each with the open or new design that
/// opened it, for [`Request::Abandon`]: `None` if a Save As did.
///
/// [`Request::Abandon`]: crate::Request::Abandon
#[derive(Debug)]
pub(crate) struct OpenFiles<T> {
    open: HashMap<FileId, (Option<OpenId>, T)>,
    /// The id of the next file opened.
    next: u64,
}

/// A request for a file that isn't open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NotOpen;

impl<T> OpenFiles<T> {
    /// No files open.
    pub(crate) fn new() -> Self {
        Self {
            open: HashMap::new(),
            next: 0,
        }
    }

    /// Keeps `file` as a new open file, opened by the open tagged `by`
    /// (`None` for a Save As), returning its id.
    pub(crate) fn add(&mut self, by: Option<OpenId>, file: T) -> FileId {
        let id = FileId(self.next);
        // A u64 counted up once per open or save never overflows.
        self.next += 1;
        self.open.insert(id, (by, file));
        id
    }

    pub(crate) fn get_mut(&mut self, file: FileId) -> Result<&mut T, NotOpen> {
        self.open
            .get_mut(&file)
            .map(|(_, file)| file)
            .ok_or(NotOpen)
    }

    /// The open file `request` closes, if it's a close or abandon of one.
    fn closed_by(&self, request: &Request) -> Option<&T> {
        let file = match *request {
            Request::Close { file, .. } => file,
            Request::Abandon { id } => self.opened_by(id)?,
            _ => return None,
        };
        self.open.get(&file).map(|(_, file)| file)
    }

    /// Whether handling `request` may change the recovered designs, which
    /// both lanes then follow its answer with, see
    /// [`Response::RecoveredListed`]: it opens or discards one, or closes
    /// a file that `holds_entry` says has a store entry, which may be left
    /// holding it.
    ///
    /// [`Response::RecoveredListed`]: crate::Response::RecoveredListed
    pub(crate) fn relists(&self, request: &Request, holds_entry: impl FnOnce(&T) -> bool) -> bool {
        matches!(
            request,
            Request::OpenRecovered { .. } | Request::DiscardRecovered { .. }
        ) || self.closed_by(request).is_some_and(holds_entry)
    }

    pub(crate) fn remove(&mut self, file: FileId) -> Result<T, NotOpen> {
        self.open.remove(&file).map(|(_, file)| file).ok_or(NotOpen)
    }

    /// The file the open tagged `id` opened, if it's still open.
    pub(crate) fn opened_by(&self, id: OpenId) -> Option<FileId> {
        self.open
            .iter()
            .find_map(|(file, (by, _))| (*by == Some(id)).then_some(*file))
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.open.values().map(|(_, file)| file)
    }

    /// Takes out every file open, as the lane stops. The web's lane
    /// stops with its worker, which lets go of everything.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = T> {
        self.open.drain().map(|(_, (_, file))| file)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn is_empty(&self) -> bool {
        self.open.is_empty()
    }
}

impl fmt::Display for NotOpen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the file isn't open")
    }
}

impl From<NotOpen> for String {
    fn from(not_open: NotOpen) -> Self {
        not_open.to_string()
    }
}

impl From<NotOpen> for SaveError {
    fn from(not_open: NotOpen) -> Self {
        SaveError::Failed(not_open.to_string())
    }
}

#[cfg(test)]
mod tests;
