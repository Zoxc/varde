//! What the lane does with each request: the open files, their auto-saves,
//! the store of new designs and the recent files list. Synchronous, so the
//! lane runs it wherever it lives, and tests call it directly.

use std::path::{Path, PathBuf};

use varde_document::{Document, Snapshot};

use super::sidecar::{self, LockFile};
use super::{recent, store};
use crate::autosave::{Ending, Origin, to_open};
use crate::open::OpenFiles;
use crate::store::{NO_RECOVERED, NO_STORE, entry_in};
use crate::{
    Access, Chosen, Closing, FileId, Offer, OpenId, Opened, ReadOnly, RecentFile, Request,
    Response, SaveError, SaveTo, SavedAs, Stores,
};

mod document_file;

use document_file::DocumentFile;
pub(crate) use document_file::sync_parent;

/// The lane's state.
#[derive(Debug)]
pub(crate) struct Files {
    open: OpenFiles<Kind>,
    /// Where the recent files list and new designs are kept, if anywhere.
    stores: Stores,
}

/// What an open document is.
#[derive(Debug)]
enum Kind {
    /// A new design never saved, which lives in its store entry: the file
    /// its auto-saves go to.
    New(LockFile),
    /// A design with a file of its own.
    Design(Design),
}

/// A design's own file.
#[derive(Debug)]
struct Design {
    file: DocumentFile,
    /// The path the lock is next to, see [`resolved`].
    real: PathBuf,
    lock: Lock,
}

/// A design's lock, which auto-saves go to if it's editable.
#[derive(Debug)]
enum Lock {
    /// The design's sidecar.
    Sidecar {
        sidecar: LockFile,
        /// Whether it holds what a crashed session left, offered to the
        /// user as [`Opened::recovered`] and not answered yet: until it's
        /// discarded or auto-saved over, saves leave it be.
        offered: bool,
    },
    /// Not held: the document is read-only.
    ReadOnly(ReadOnly),
}

/// Why a document can't be saved or auto-saved to.
const READ_ONLY: &str = "the design is read-only";

impl Kind {
    /// A design with a file of its own whose lock is held: saved to
    /// directly.
    fn editable_design(&mut self) -> Option<&mut Design> {
        match self {
            Kind::Design(
                design @ Design {
                    lock: Lock::Sidecar { .. },
                    ..
                },
            ) => Some(design),
            _ => None,
        }
    }

    /// The file auto-saves go to, if the document is editable.
    fn held(&mut self) -> Option<&mut LockFile> {
        match self {
            Kind::New(entry) => Some(entry),
            Kind::Design(design) => design.lock.held(),
        }
    }

    /// The user answered the offer of what a crashed session left.
    fn answered(&mut self) {
        if let Kind::Design(design) = self {
            design.lock.answered();
        }
    }

    /// How the clean close lets go of its lock: only a new design's store
    /// entry may hold the design as downloaded; a design's sidecar goes,
    /// whatever is in it.
    fn clean(&self) -> Ending {
        match self {
            Kind::New(_) => Ending::CloseButDownloaded,
            Kind::Design(_) => Ending::Close,
        }
    }

    /// Lets go of its lock as `ending` says, as the document closes.
    fn end(self, ending: Ending) -> std::io::Result<()> {
        match self {
            Kind::New(entry) => entry.end(ending),
            Kind::Design(design) => design.lock.end(ending),
        }
    }

    /// Lets go of its lock once the document is saved as another file.
    /// Failing to delete it only leaves it behind, unlocked and empty. What
    /// a crashed session left and the user hasn't answered stays with the
    /// design it's of, to be offered again.
    fn saved_elsewhere(self) {
        let offered = matches!(&self, Kind::Design(design) if design.lock.offered());
        let _ = self.end(if offered {
            Ending::Release
        } else {
            Ending::Close
        });
    }
}

impl Lock {
    /// The sidecar of the design at `real`, locked if it can be.
    fn of_design(real: &Path) -> Self {
        match sidecar::lock(real) {
            Ok(sidecar) => Lock::Sidecar {
                sidecar,
                offered: false,
            },
            Err(read_only) => Lock::ReadOnly(read_only),
        }
    }

    /// What a crashed session left in the sidecar of the design `file`,
    /// just opened holding `document`, to offer the user, or why it
    /// couldn't be read. Marks the lock `offered` if there's an offer.
    fn offer(&mut self, file: &DocumentFile, document: &Document) -> Result<Option<Offer>, String> {
        // Only the editor holding the lock may look: otherwise the sidecar
        // is another editor's, auto-saving as it goes.
        let Lock::Sidecar { sidecar, offered } = self else {
            return Ok(None);
        };
        match sidecar.read() {
            Ok(Some(recovered)) if *recovered.document != *document => {
                *offered = true;
                Ok(Some(Offer {
                    design_changed: !recovered.based_on(file.tail()),
                    document: Snapshot::unwrap_or_clone(recovered.document),
                }))
            }
            Ok(_) => Ok(None),
            Err(error) => Err(format!("couldn't read what was auto-saved: {error}")),
        }
    }

    fn access(&self) -> Access {
        match self {
            Lock::Sidecar { .. } => Access::Edit,
            Lock::ReadOnly(read_only) => Access::ReadOnly(read_only.clone()),
        }
    }

    fn held(&mut self) -> Option<&mut LockFile> {
        match self {
            Lock::Sidecar { sidecar, .. } => Some(sidecar),
            Lock::ReadOnly(_) => None,
        }
    }

    fn offered(&self) -> bool {
        matches!(self, Lock::Sidecar { offered: true, .. })
    }

    fn answered(&mut self) {
        if let Lock::Sidecar { offered, .. } = self {
            *offered = false;
        }
    }

    /// The design was just saved, so what was auto-saved is older. Failing
    /// to empty it only leaves an older state to be offered should this
    /// session crash; the next save or clean close tries again. What a
    /// crashed session left is kept until the user answers the offer.
    fn saved(&mut self) {
        if let Lock::Sidecar {
            sidecar,
            offered: false,
        } = self
        {
            let _ = sidecar.clear();
        }
    }

    /// Lets go of it as `ending` says. A read-only design holds none.
    fn end(self, ending: Ending) -> std::io::Result<()> {
        match self {
            Lock::Sidecar { sidecar, .. } => sidecar.end(ending),
            Lock::ReadOnly(_) => Ok(()),
        }
    }
}

impl Files {
    /// No files open, and the app's own files kept in `stores`.
    pub(crate) fn new(stores: Stores) -> Self {
        Self {
            open: OpenFiles::new(),
            stores,
        }
    }

    /// Handles `request`: its answer, followed by the recovered designs as
    /// they are now if handling it may have changed which there are, see
    /// [`Response::RecoveredListed`].
    pub(crate) fn answer(&mut self, request: Request) -> Vec<Response> {
        // Only a new design has a store entry: a design's sidecar is never
        // listed.
        let relist = self
            .open
            .relists(&request, |kind| matches!(kind, Kind::New(_)));
        let mut responses = vec![self.handle(request)];
        if relist {
            responses.push(self.handle(Request::ListRecovered));
        }
        responses
    }

    pub(crate) fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Open {
                id,
                from: Chosen::Path(path),
            } => self.open(id, absolute(path)),
            Request::New { id } => Response::Created {
                id,
                result: self.create(id),
            },
            Request::Save {
                file,
                revision,
                document,
            } => Response::Saved {
                file,
                revision,
                result: self.save(file, &document),
            },
            Request::SaveAs {
                file,
                to: SaveTo::Path { path, overwrite },
                revision,
                document,
            } => {
                let path = absolute(path);
                Response::SavedAs {
                    file,
                    revision,
                    result: self.save_as(file, &path, overwrite, &document),
                    to: Chosen::Path(path),
                }
            }
            Request::AutoSave {
                file,
                revision,
                document,
            } => Response::AutoSaved {
                file,
                revision,
                result: self.auto_save(file, &document, Origin::Edited),
            },
            // What the web does after a download, handled the same way here
            // so the native tests cover it.
            Request::KeepDownload {
                file,
                revision,
                document,
            } => Response::AutoSaved {
                file,
                revision,
                result: self.auto_save(file, &document, Origin::Downloaded),
            },
            Request::DiscardRecovery { file } => Response::RecoveryDiscarded {
                file,
                result: self.discard_recovery(file),
            },
            Request::Close { file, closing } => Response::Closed {
                file,
                result: self.close(file, closing),
            },
            Request::Abandon { id } => Response::Abandoned {
                id,
                result: self.abandon(id),
            },
            Request::LoadRecent => Response::RecentLoaded {
                entries: recent::listed(
                    self.stores
                        .recent
                        .as_deref()
                        .map(recent::load)
                        .unwrap_or_default(),
                ),
                home: recent::home(),
            },
            Request::WriteRecent { entries } => Response::RecentWritten {
                result: self.write_recent(&entries),
            },
            Request::ListRecovered => Response::RecoveredListed {
                designs: self
                    .stores
                    .designs
                    .as_deref()
                    .map(store::list)
                    .unwrap_or_default(),
            },
            Request::OpenRecovered { id, path } => self.open_recovered(id, path),
            Request::DiscardRecovered { path } => Response::RecoveredDiscarded {
                result: self
                    .designs(NO_RECOVERED)
                    .and_then(|dir| store::discard(dir, &path)),
                path,
            },
            // Natively the user's files are picked by path.
            Request::Open {
                from: Chosen::File(_),
                ..
            }
            | Request::SaveAs {
                to: SaveTo::Picked(_),
                ..
            }
            | Request::Export {
                to: SaveTo::Picked(_),
                ..
            } => request.failed("only the web build picks files this way".to_owned()),
            Request::Export {
                to: SaveTo::Path { path, overwrite },
                title,
                bodies,
            } => {
                let path = absolute(path);
                Response::Exported {
                    result: super::export::export(&path, overwrite, &title, &bodies),
                    to: Chosen::Path(path),
                }
            }
            Request::Flush => Response::Flushed,
        }
    }

    /// Opens the design at `path`, an absolute path.
    fn open(&mut self, open_id: OpenId, path: PathBuf) -> Response {
        // Read first, so a path that isn't a design never gets a sidecar.
        // Should another editor save in between, the conflict check on
        // saving catches it.
        let result = match DocumentFile::open(&path) {
            Ok((file, document)) => {
                let real = resolved(&path);
                let mut lock = Lock::of_design(&real);
                let recovered = lock.offer(&file, &document);
                let access = lock.access();
                let id = self
                    .open
                    .add(Some(open_id), Kind::Design(Design { file, real, lock }));
                Ok(Opened {
                    file: id,
                    document,
                    access,
                    recovered,
                    downloaded: false,
                })
            }
            Err(error) => Err(error.to_string()),
        };
        Response::Opened {
            id: open_id,
            path: Some(path),
            result,
        }
    }

    /// Makes a store entry for a new design.
    fn create(&mut self, open_id: OpenId) -> Result<FileId, String> {
        let dir = self.designs(NO_STORE)?;
        let entry = store::create(dir)
            .map_err(|e| format!("couldn't make a place for the new design: {e}"))?;
        Ok(self.open.add(Some(open_id), Kind::New(entry)))
    }

    /// Opens the store entry at `path`, left behind by a session that
    /// crashed, as a new design.
    fn open_recovered(&mut self, open_id: OpenId, path: PathBuf) -> Response {
        let entry = self
            .designs(NO_RECOVERED)
            .and_then(|dir| store::open(dir, &path));
        let result = entry.and_then(|mut entry| {
            let saved = to_open(entry.read())?;
            let id = self.open.add(Some(open_id), Kind::New(entry));
            Ok(Opened::editable(
                id,
                Snapshot::unwrap_or_clone(saved.document),
                saved.origin.is_download(),
            ))
        });
        Response::Opened {
            id: open_id,
            path: Some(path),
            result,
        }
    }

    fn save(&mut self, file: FileId, document: &Document) -> Result<(), SaveError> {
        // The UI doesn't offer it otherwise: someone else may be editing
        // it, or it's a new design, which is saved as.
        let design = self
            .open
            .get_mut(file)?
            .editable_design()
            .ok_or_else(|| SaveError::Failed(READ_ONLY.to_owned()))?;
        design.file.save(document)?;
        design.lock.saved();
        Ok(())
    }

    /// Auto-saves `document`, holding it as `origin` says: the design as
    /// downloaded only for [`Request::KeepDownload`], which the native app
    /// never sends.
    fn auto_save(
        &mut self,
        file: FileId,
        document: &Snapshot,
        origin: Origin,
    ) -> Result<(), String> {
        let open = self.open.get_mut(file)?;
        // Based on the design as this lane last read or wrote it: whatever
        // changes it after, a save of ours included, makes it another.
        let base = match open {
            Kind::New(_) => None,
            Kind::Design(design) => Some(design.file.tail()),
        };
        let sidecar = open.held().ok_or_else(|| READ_ONLY.to_owned())?;
        sidecar
            .append(base, document, origin)
            .map_err(|e| e.to_string())?;
        // The UI only auto-saves once the offer is answered.
        open.answered();
        Ok(())
    }

    fn discard_recovery(&mut self, file: FileId) -> Result<(), String> {
        let open = self.open.get_mut(file)?;
        if let Some(sidecar) = open.held() {
            sidecar.clear().map_err(|e| e.to_string())?;
        }
        open.answered();
        Ok(())
    }

    fn save_as(
        &mut self,
        file: Option<FileId>,
        path: &Path,
        overwrite: bool,
        document: &Document,
    ) -> Result<SavedAs, SaveError> {
        let real = resolved(path);
        self.refuse_app_file(&real, path)?;
        let mut current = match file {
            Some(file) => Some((file, self.open.get_mut(file)?)),
            None => None,
        };
        // Saving over the design itself keeps the lock it already holds,
        // which locking it again would find taken.
        if let Some((file, open)) = &mut current
            && let Some(design) = open.editable_design()
            && design.real == real
        {
            design.file = write(&real, path, overwrite, document)?;
            // What it held is older than what was just saved.
            design.lock.saved();
            return Ok(SavedAs {
                file: *file,
                access: design.lock.access(),
                offered: design.lock.offered(),
            });
        }
        let lock = target_lock(&real, path)?;
        let written = match write(&real, path, overwrite, document) {
            Ok(written) => written,
            Err(error) => {
                // Best effort: the error to show is the one above. Anything
                // in it is a crashed editor's of the file that wasn't
                // replaced after all, to be offered still.
                let _ = lock.end(Ending::Release);
                return Err(error);
            }
        };
        let mut design = Design {
            file: written,
            real,
            lock,
        };
        // Whatever it held was left by a crashed editor of the file just
        // replaced, and is older than what was just saved.
        design.lock.saved();
        let access = design.lock.access();
        let kind = Kind::Design(design);
        let id = match current {
            Some((file, open)) => {
                std::mem::replace(open, kind).saved_elsewhere();
                file
            }
            None => self.open.add(None, kind),
        };
        Ok(SavedAs {
            file: id,
            access,
            // A lock just taken has offered nothing.
            offered: false,
        })
    }

    /// The store of new designs, or `missing` as the error if there's none.
    fn designs(&self, missing: &str) -> Result<&Path, String> {
        self.stores
            .designs
            .as_deref()
            .ok_or_else(|| missing.to_owned())
    }

    /// Refuses to save as one of varde's own files: `real`, shown to the
    /// user as `shown`.
    fn refuse_app_file(&self, real: &Path, shown: &Path) -> Result<(), SaveError> {
        // A store entry is locked on itself, not by a sidecar, so the lock
        // taken to save wouldn't see it held, and its editor's close would
        // delete what was saved over it.
        if let Some(designs) = self.stores.designs.as_deref().map(resolved)
            && entry_in(&designs, real).is_some()
        {
            return Err(SaveError::Failed(format!(
                "{} is in varde's store of new designs",
                display_name(shown)
            )));
        }
        // Nor is a sidecar locked by a sidecar of its own: saved over, its
        // design's editor would hold a lock on a file no longer there, and
        // the next editor would take the design saved for its lock file.
        if sidecar::is_sidecar_name(real) {
            return Err(SaveError::Failed(format!(
                "{} is the name of a lock file of varde's",
                display_name(shown)
            )));
        }
        Ok(())
    }

    /// Lets go of `file`'s lock as `closing` says, see [`Request::Close`].
    fn close(&mut self, file: FileId, closing: Closing) -> Result<(), String> {
        let open = self.open.remove(file)?;
        let ending = closing.ending(open.clean());
        open.end(ending)
            .map_err(|e| format!("couldn't remove the lock file: {e}"))
    }

    /// Closes the file the open tagged `id` opened, if it's still open,
    /// keeping anything a crashed session auto-saved: the user never saw
    /// it.
    fn abandon(&mut self, id: OpenId) -> Result<(), String> {
        match self.open.opened_by(id) {
            Some(file) => self.close(file, Closing::Keep),
            None => Ok(()),
        }
    }

    fn write_recent(&self, entries: &[RecentFile]) -> Result<(), String> {
        match &self.stores.recent {
            Some(store) => recent::write(store, entries).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Lets go of every file still open, as the lane stops. The UI closes
    /// the files it's done with before, so any still open are left as a
    /// crash would leave them: auto-saves are kept, to be recovered.
    fn close_all(&mut self) {
        for open in self.open.drain() {
            let _ = open.end(Ending::Release);
        }
    }
}

/// The lock of `real`, shown to the user as `shown`, for saving a design
/// as it: another file than the design's own.
fn target_lock(real: &Path, shown: &Path) -> Result<Lock, SaveError> {
    // Locked before writing, so a design another editor has open is never
    // written over.
    match Lock::of_design(real) {
        Lock::ReadOnly(ReadOnly::InUse) => Err(SaveError::Failed(format!(
            "{} is open elsewhere",
            display_name(shown)
        ))),
        // Otherwise written even if it can't be locked, and open read-only
        // after, like a design opened there.
        lock => Ok(lock),
    }
}

/// Writes `document` to `real`, shown to the user as `shown`, replacing
/// what's there if `overwrite`. Written to where a symbolic link points, so
/// the link stays.
fn write(
    real: &Path,
    shown: &Path,
    overwrite: bool,
    document: &Document,
) -> Result<DocumentFile, SaveError> {
    let written = if overwrite {
        DocumentFile::replace(real, document)
    } else {
        DocumentFile::create(real, document)
    };
    written.map_err(|error| match error {
        crate::vrdp::Error::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            SaveError::Failed(format!("{} already exists", display_name(shown)))
        }
        error => error.into(),
    })
}

/// The file name of `path`, for messages.
pub(super) fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// `path` made absolute, as the lane keeps and reports every path the user
/// gave: left as it is if the current directory can't be found.
fn absolute(path: PathBuf) -> PathBuf {
    std::path::absolute(&path).unwrap_or(path)
}

/// Where the design at `path` really is: through symbolic links, including
/// ones to its directory, should it not exist yet. Its lock sits next to
/// it, so every path to a design shares one lock.
pub(super) fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .ok()
        .or_else(|| {
            let parent = std::fs::canonicalize(path.parent()?).ok()?;
            Some(parent.join(path.file_name()?))
        })
        .unwrap_or_else(|| path.to_owned())
}

/// The lane stopping lets go of every file still open.
impl Drop for Files {
    fn drop(&mut self) {
        self.close_all();
    }
}

#[cfg(test)]
mod tests;
