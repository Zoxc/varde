//! What the lane does with each request: the open files, their auto-saves,
//! the store of new designs and the recent files list. Synchronous, so the
//! lane runs it wherever it lives, and tests call it directly.

use std::path::{Path, PathBuf};

use varde_document::{Document, Snapshot};

use super::path::{absolute, display_name, resolved};
use super::sidecar::{self, LockFile};
use super::{mcp_token, panicked, recent, settings, store};
use crate::autosave::{Ending, to_open};
use crate::lock::{self, READ_ONLY};
use crate::open::{NOT_FOUND, OpenFiles};
use crate::store::{NO_RECOVERED, NO_STORE, entry_in};
use crate::thumbnail::{self, Thumbnail, previews};
use crate::vrdp::{self, Preview};
use crate::{
    Chosen, Closing, Damage, FileId, OpenId, Opened, ReadOnly, Request, Response, SaveError,
    SaveTo, SavedAs, Stores,
};

mod document_file;

use document_file::DocumentFile;

/// What requests about browser storage are answered with natively.
const NO_BROWSER_STORAGE: &str = "only the web build keeps designs in browser storage";

/// The lane's state.
#[derive(Debug)]
pub(crate) struct Files {
    open: OpenFiles<Kind>,
    /// Where the recent files list, the settings and new designs are kept,
    /// if anywhere.
    stores: Stores,
    /// The MCP token, in its store or memory.
    mcp_token: mcp_token::Tokens,
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

/// A design's lock, which auto-saves go to if it's editable: its sidecar,
/// see `src/lock.rs`.
type Lock = lock::Lock<LockFile>;

impl Kind {
    /// A design with a file of its own whose lock is held: saved to
    /// directly.
    fn editable_design(&mut self) -> Option<&mut Design> {
        match self {
            Kind::Design(design) if matches!(design.lock, Lock::Sidecar { .. }) => Some(design),
            _ => None,
        }
    }

    /// Lets go of its lock as `ending` says, as the document closes.
    fn end(self, ending: Ending) -> std::io::Result<()> {
        match self {
            Kind::New(entry) => entry.end(ending),
            Kind::Design(design) => end_lock(design.lock, ending),
        }
    }

    /// Lets go of its lock once the document is saved as another file.
    /// Failing to delete it only leaves it behind, unlocked and empty. What
    /// a crashed session left and the user hasn't answered stays with the
    /// design it's of, to be offered again.
    fn saved_elsewhere(self) {
        let ending = match &self {
            Kind::New(_) => Ending::Close,
            Kind::Design(design) => design.lock.ending_for_saved_as(),
        };
        let _ = self.end(ending);
    }
}

/// The sidecar of the design at `real`, locked if it can be.
fn lock_of_design(real: &Path) -> Lock {
    Lock::new(sidecar::lock(real))
}

/// What a crashed session left in the sidecar `lock` of the design `file`,
/// just opened holding `document`, to offer the user, see
/// [`lock::Lock::offer`].
fn offer(
    lock: &mut Lock,
    file: &DocumentFile,
    document: &Document,
) -> Result<Option<crate::Offer>, crate::RecoveryError> {
    lock.offer(file.tail(), |base| file.based_past(base), document)
}

/// Lets go of `lock` as `ending` says. A read-only design holds none.
fn end_lock(lock: Lock, ending: Ending) -> std::io::Result<()> {
    match lock.into_held() {
        Some(sidecar) => sidecar.end(ending),
        None => Ok(()),
    }
}

impl Files {
    /// No files open, and the app's own files kept in `stores`.
    pub(crate) fn new(stores: Stores) -> Self {
        Self {
            open: OpenFiles::new(),
            mcp_token: mcp_token::Tokens::new(stores.mcp_token.clone()),
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
                result: self.auto_save(file, &document),
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
                result: to_store(self.stores.recent.as_deref(), |s| {
                    recent::write(s, &entries)
                }),
            },
            Request::LoadSettings => Response::SettingsLoaded {
                settings: self.stores.read_settings_now(),
            },
            Request::WriteSettings { settings } => Response::SettingsWritten {
                result: to_store(self.stores.settings.as_deref(), |s| {
                    settings::write(s, &settings)
                }),
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
            // Nor is there browser storage.
            Request::Open {
                from: Chosen::Browser(_),
                ..
            }
            | Request::SaveAs {
                to: SaveTo::Browser { .. },
                ..
            }
            | Request::Export {
                to: SaveTo::Browser { .. },
                ..
            }
            | Request::Rename { .. }
            | Request::DeleteFromBrowser { .. }
            | Request::RecordDownload { .. }
            | Request::DownloadFromBrowser { .. } => request.failed(NO_BROWSER_STORAGE.to_owned()),
            Request::ListBrowser => Response::BrowserListed {
                designs: Vec::new(),
            },
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
            Request::LoadPanic => Response::PanicLoaded {
                panic: self.stores.panic.as_deref().and_then(panicked::load),
            },
            Request::DiscardPanic { panic } => Response::PanicDiscarded {
                result: to_store(self.stores.panic.as_deref(), |s| {
                    panicked::discard(s, &panic)
                }),
            },
            Request::LoadMcpToken => Response::McpToken {
                regenerated: false,
                result: self.mcp_token.load(),
            },
            Request::RegenerateMcpToken => Response::McpToken {
                regenerated: true,
                result: self.mcp_token.regenerate(),
            },
            Request::EncodePng { owner, tag, image } => Response::PngEncoded {
                owner,
                tag,
                result: thumbnail::encode_png(&image).map_err(|error| error.to_string()),
            },
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
                let mut lock = lock_of_design(&real);
                let recovered = offer(&mut lock, &file, &document);
                let access = lock.access();
                let design = Design {
                    file,
                    real,
                    lock,
                    found: read.found,
                };
                let id = self.open.add(Some(open_id), Kind::Design(Box::new(design)));
                Ok(Opened {
                    access,
                    recovered,
                    ..Opened::editable(id, document, damage)
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
        let design = match self.open.get_mut(file) {
            Ok(Kind::Design(design)) => design,
            not_design => {
                let error = not_design.map_or_else(String::from, |_| NOT_FOUND.to_owned());
                return Response::Opened {
                    id: open_id,
                    path: None,
                    result: Err(error),
                };
            }
        };
        let path = Some(design.file.path().to_owned());
        let result = design
            .found
            .take_if(|chosen| chosen.tail == found)
            .ok_or_else(|| NOT_FOUND.to_owned())
            .map(|chosen| {
                let damage = Damage::of(&chosen.report);
                let document = design.file.open_found(chosen);
                let recovered = offer(&mut design.lock, &design.file, &document);
                Opened {
                    access: design.lock.access(),
                    recovered,
                    ..Opened::editable(file, document, damage)
                }
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

    /// Auto-saves `document` to `file`'s sidecar or store entry.
    fn auto_save(&mut self, file: FileId, document: &Snapshot) -> Result<(), String> {
        match self.open.get_mut(file)? {
            // A new design is based on no save.
            Kind::New(entry) => entry
                .append(None, None, document)
                .map_err(|e| e.to_string()),
            Kind::Design(design) => {
                // Based on the design as this lane last read or wrote it:
                // whatever changes it after, a save of ours included, makes
                // it another.
                let base = Some(design.file.tail());
                design
                    .lock
                    .auto_save(|sidecar| sidecar.append(base, None, document))
            }
        }
    }

    fn discard_recovery(&mut self, file: FileId) -> Result<(), String> {
        match self.open.get_mut(file)? {
            Kind::New(entry) => entry.clear().map_err(|e| e.to_string()),
            Kind::Design(design) => design.lock.discard_recovery(),
        }
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
                let _ = end_lock(lock, Ending::Release);
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
        open.end(closing.ending())
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
    match lock_of_design(real) {
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
fn thumbnail(path: &Path) -> Option<Thumbnail> {
    let previews = DocumentFile::read_previews(path);
    thumbnail::decode(&previews)
}

/// Writes to `store` if there is one: without, there's nothing to keep.
fn to_store(
    store: Option<&Path>,
    write: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    store.map_or(Ok(()), |s| write(s).map_err(|e| e.to_string()))
}

/// The lane stopping lets go of every file still open.
impl Drop for Files {
    fn drop(&mut self) {
        self.close_all();
    }
}

#[cfg(test)]
mod tests;
