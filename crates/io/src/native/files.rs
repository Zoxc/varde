//! What the lane does with each request: the open files, their auto-saves,
//! the store of new designs and the recent files list. Synchronous, so the
//! lane runs it wherever it lives, and tests call it directly.

use std::path::{Path, PathBuf};

use varde_document::{Document, Snapshot};

use super::sidecar::{self, LockFile};
use super::{recent, settings, store};
use crate::autosave::{Ending, Origin, to_open};
use crate::open::{KEPT, NOT_FOUND, OpenFiles};
use crate::store::{NO_RECOVERED, NO_STORE, entry_in};
use crate::thumbnail::{self, Image, previews};
use crate::vrdp::{self, Error as FileError, Preview};
use crate::{
    Access, Chosen, Closing, Damage, FileId, Offer, OpenId, Opened, ReadOnly, RecentFile,
    RecoveryError, Request, Response, SaveError, SaveTo, SavedAs, Settings, Stores,
};

mod document_file;

use document_file::DocumentFile;
pub(crate) use document_file::{sync_parent, temp_path};

/// The lane's state.
#[derive(Debug)]
pub(crate) struct Files {
    open: OpenFiles<Kind>,
    /// Where the recent files list, the settings and new designs are kept,
    /// if anywhere.
    stores: Stores,
}

/// What an open document is.
#[derive(Debug)]
enum Kind {
    /// A new design never saved, which lives in its store entry: the file
    /// its auto-saves go to.
    New(LockFile),
    /// A design with a file of its own.
    Design(Box<Design>),
}

/// A design's own file.
#[derive(Debug)]
struct Design {
    file: DocumentFile,
    /// The path the lock is next to, see [`resolved`].
    real: PathBuf,
    lock: Lock,
    /// The newest save a search found past damage in the file, offered as
    /// [`FoundSave`](crate::FoundSave) for [`Request::OpenFound`], till it's opened.
    found: Option<vrdp::Opened<Document>>,
}

/// A design's lock, which auto-saves go to if it's editable.
#[derive(Debug)]
enum Lock {
    /// The design's sidecar.
    Sidecar {
        sidecar: LockFile,
        /// Whether it holds what a crashed session left, offered to the
        /// user as [`Opened::recovered`] and not answered yet: until it's
        /// discarded or auto-saved over, saves leave it be. So is one
        /// whose records are all damaged, or that couldn't be read, which
        /// [`Opened::recovered`] says why.
        offered: bool,
        /// Whether it's offered so, as one that can't be read
        /// ([`RecoveryError::kept`]): auto-saves are refused till it's
        /// discarded.
        kept: bool,
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
            Kind::Design(design) if matches!(design.lock, Lock::Sidecar { .. }) => Some(design),
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
                kept: false,
            },
            Err(read_only) => Lock::ReadOnly(read_only),
        }
    }

    /// What a crashed session left in the sidecar of the design `file`,
    /// just opened holding `document`, to offer the user, or why it
    /// couldn't be read. Marks the lock `offered` if there's an offer, and
    /// `kept` too if it's kept as it can't be read.
    fn offer(
        &mut self,
        file: &DocumentFile,
        document: &Document,
    ) -> Result<Option<Offer>, RecoveryError> {
        // Only the editor holding the lock may look: otherwise the sidecar
        // is another editor's, auto-saving as it goes.
        let Lock::Sidecar {
            sidecar,
            offered,
            kept,
        } = self
        else {
            return Ok(None);
        };
        (*offered, *kept) = (false, false);
        match sidecar.read_with_report() {
            Ok(Some(read)) if *read.payload.document != *document => {
                *offered = true;
                let recovered = read.payload;
                let newer_base = recovered
                    .base
                    .is_some_and(|base| base != file.tail() && file.based_past(base));
                Ok(Some(Offer {
                    design_changed: !recovered.based_on(file.tail()),
                    document: Snapshot::unwrap_or_clone(recovered.document),
                    damage: Damage::of(&read.report, None),
                    newer_base,
                }))
            }
            Ok(_) => Ok(None),
            Err(error) => {
                // What may yet be got out of it, damaged records or one that
                // couldn't be read, stays until the user answers, as an
                // offer does: auto-saves are refused rather than start it
                // over, nor do saves empty it. What can't ever be read,
                // auto-saving starts over.
                let unreadable = matches!(error, FileError::Corrupt { .. } | FileError::Io(_));
                (*offered, *kept) = (unreadable, unreadable);
                let message = match error {
                    FileError::Corrupt { .. } => {
                        "what was auto-saved is damaged and can't be read".to_owned()
                    }
                    error => format!("couldn't read what was auto-saved: {error}"),
                };
                Err(RecoveryError {
                    message,
                    kept: unreadable,
                })
            }
        }
    }

    /// Whether it holds what a crashed session left that can't be read,
    /// kept till the user discards it, see [`RecoveryError::kept`].
    fn kept(&self) -> bool {
        matches!(self, Lock::Sidecar { kept: true, .. })
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
        if let Lock::Sidecar { offered, kept, .. } = self {
            (*offered, *kept) = (false, false);
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
            ..
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
            Request::OpenFound { id, file, found } => self.open_found(id, file, found),
            Request::New { id } => Response::Created {
                id,
                result: self.create(id),
            },
            Request::Save {
                file,
                revision,
                document,
                thumbnail,
            } => Response::Saved {
                file,
                revision,
                result: self.save(file, &document, &previews(thumbnail.as_ref())),
            },
            Request::SaveAs {
                file,
                to: SaveTo::Path { path, overwrite },
                revision,
                document,
                thumbnail,
            } => {
                let path = absolute(path);
                let previews = previews(thumbnail.as_ref());
                Response::SavedAs {
                    file,
                    revision,
                    result: self.save_as(file, &path, overwrite, &document, &previews),
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
            Request::LoadThumbnails { paths } => Response::ThumbnailsLoaded {
                thumbnails: paths
                    .into_iter()
                    .filter_map(|path| thumbnail(&path).map(|image| (path, image)))
                    .collect(),
            },
            Request::WriteRecent { entries } => Response::RecentWritten {
                result: self.write_recent(&entries),
            },
            Request::LoadSettings => Response::SettingsLoaded {
                settings: self
                    .stores
                    .settings
                    .as_deref()
                    .map(settings::load)
                    .unwrap_or_default(),
            },
            Request::WriteSettings { settings } => Response::SettingsWritten {
                result: self.write_settings(&settings),
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
        let result = match DocumentFile::open_with_found(&path) {
            // One found damaged past what was opened refuses saves.
            Ok((file, read)) => {
                let damage = Damage::of_design(&read);
                let document = read.opened.payload;
                let real = resolved(&path);
                let mut lock = Lock::of_design(&real);
                let recovered = lock.offer(&file, &document);
                let access = lock.access();
                let design = Design {
                    file,
                    real,
                    lock,
                    found: read.found,
                };
                let id = self.open.add(Some(open_id), Kind::Design(Box::new(design)));
                Ok(Opened {
                    file: id,
                    document,
                    access,
                    recovered,
                    downloaded: false,
                    damage,
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

    /// Opens the save a search found past damage in `file`'s design,
    /// named by its tail `found`, instead of the one opened, see
    /// [`Request::OpenFound`].
    fn open_found(&mut self, open_id: OpenId, file: FileId, found: vrdp::Tail) -> Response {
        let mut path = None;
        let result = self
            .open
            .get_mut(file)
            .map_err(String::from)
            .and_then(|open| match open {
                Kind::Design(design) => Ok(design),
                Kind::New(_) => Err(NOT_FOUND.to_owned()),
            })
            .and_then(|design| {
                path = Some(design.file.path().to_owned());
                let chosen = design
                    .found
                    .take_if(|chosen| chosen.tail == found)
                    .ok_or_else(|| NOT_FOUND.to_owned())?;
                let damage = Damage::of(&chosen.report, None);
                let document = design.file.open_found(chosen);
                let recovered = design.lock.offer(&design.file, &document);
                Ok(Opened {
                    file,
                    document,
                    access: design.lock.access(),
                    recovered,
                    downloaded: false,
                    damage,
                })
            });
        Response::Opened {
            id: open_id,
            path,
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
            let (saved, damage) = to_open(entry.read_with_report())?;
            let id = self.open.add(Some(open_id), Kind::New(entry));
            Ok(Opened::editable(
                id,
                Snapshot::unwrap_or_clone(saved.document),
                saved.origin.is_download(),
                damage,
            ))
        });
        Response::Opened {
            id: open_id,
            path: Some(path),
            result,
        }
    }

    fn save(
        &mut self,
        file: FileId,
        document: &Document,
        previews: &[Preview],
    ) -> Result<(), SaveError> {
        // The UI doesn't offer it otherwise: someone else may be editing
        // it, or it's a new design, which is saved as.
        let design = self
            .open
            .get_mut(file)?
            .editable_design()
            .ok_or_else(|| SaveError::Failed(READ_ONLY.to_owned()))?;
        design.file.save(document, previews)?;
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
        // What a crashed session left that can't be read is kept as it
        // is till the user discards it.
        if matches!(open, Kind::Design(design) if design.lock.kept()) {
            return Err(KEPT.to_owned());
        }
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
        previews: &[Preview],
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
            design.file = write(&real, path, overwrite, document, previews)?;
            design.found = None;
            // What it held is older than what was just saved.
            design.lock.saved();
            return Ok(SavedAs {
                file: *file,
                access: design.lock.access(),
                offered: design.lock.offered(),
            });
        }
        let lock = target_lock(&real, path)?;
        let written = match write(&real, path, overwrite, document, previews) {
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
            found: None,
        };
        // Whatever it held was left by a crashed editor of the file just
        // replaced, and is older than what was just saved.
        design.lock.saved();
        let access = design.lock.access();
        let kind = Kind::Design(Box::new(design));
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

    fn write_settings(&self, written: &Settings) -> Result<(), String> {
        match &self.stores.settings {
            Some(store) => settings::write(store, written).map_err(|e| e.to_string()),
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

/// Writes `document` and `previews` to `real`, shown to the user as
/// `shown`, replacing what's there if `overwrite`. Written to where a
/// symbolic link points, so the link stays.
fn write(
    real: &Path,
    shown: &Path,
    overwrite: bool,
    document: &Document,
    previews: &[Preview],
) -> Result<DocumentFile, SaveError> {
    let written = if overwrite {
        DocumentFile::replace(real, document, previews)
    } else {
        DocumentFile::create(real, document, previews)
    };
    written.map_err(|error| match error {
        crate::vrdp::Error::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            SaveError::Failed(format!("{} already exists", display_name(shown)))
        }
        error => error.into(),
    })
}

/// The thumbnail of the design at `path`, if it has one that decodes.
fn thumbnail(path: &Path) -> Option<Image> {
    let preview = DocumentFile::read_preview(path, |preview| preview.is(thumbnail::MEDIA_TYPE))?;
    thumbnail::decode(&preview)
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
