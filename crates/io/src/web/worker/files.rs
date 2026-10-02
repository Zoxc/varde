//! The lane on the web: what its IO worker (`src/web.rs`) does with each
//! request, the counterpart of the native lane's `src/native/files.rs`.
//! New designs and auto-saves go to entries in the Origin Private File
//! System, see `src/opfs.rs`, and saves to files the user picked, see
//! `src/pick.rs`. Getting at files there is asynchronous, so unlike the
//! native lane's this is too; the worker still handles one request at a
//! time.

use std::io;
use std::path::{Path, PathBuf};

use varde_document::{Document, Snapshot};
use web_sys::{FileSystemDirectoryHandle, FileSystemFileHandle};

use super::disk::{self, Handed};
use super::opfs::{self, Handle, file, modified, names};
use super::settings;
use crate::autosave::{AutoSaved, Ending, Held, Origin, to_open};
use crate::open::{NOT_FOUND, OpenFiles};
use crate::opfs::{lost_to_another_tab, new_name};
use crate::store::{
    ATTEMPTS, Listing, NO_RECOVERED, NO_STORE, is_entry_name, listed_entry, listing, newest_first,
};
use crate::three_mf;
use crate::thumbnail::previews;
use crate::vrdp::{
    self, Error as FileError, HeldFile, Known, Preview, ReadAt, Tail, check_unchanged,
    from_bytes_with_found, whole_file,
};
use crate::web::js::call;
use crate::{
    Access, Chosen, Closing, Damage, FileId, OpenId, Opened, Picked, PickedFrom, ReadOnly,
    Recovered, Request, Response, SaveError, SaveTo, SavedAs, Stores,
};

/// What the web build answers requests for files at a path with: it only
/// has the files the user picks, see `src/pick.rs`.
const NO_FILES: &str = "the web build has no paths to open or save files at";

/// The worker's state: the designs open in this tab, each with an entry.
#[derive(Debug)]
pub(crate) struct Files {
    open: OpenFiles<Open>,
    /// Counts the names made for new entries.
    named: u64,
    /// Where new designs are kept, if anywhere, see [`Stores::designs`](crate::Stores::designs).
    designs: Option<PathBuf>,
    /// Where the settings are kept, if anywhere, see [`Stores::settings`](crate::Stores::settings).
    settings: Option<PathBuf>,
    /// The directory `designs` names, once found.
    dir: Option<FileSystemDirectoryHandle>,
}

/// An open design: its entry, held for as long as it's open, which its
/// auto-saves go to, and the file of the user's it came from, if any.
#[derive(Debug)]
struct Open {
    /// Its entry, or why it has none: a design opened from or saved as a
    /// file of the user's still is when the store can't be used, say with
    /// the site's data blocked, only without auto-saves.
    entry: Result<Entry, String>,
    /// The file of the user's the design was opened from or saved as, if
    /// it can be written to: saves go there.
    disk: Option<Disk>,
    /// The name of the file of the user's the design was opened from or
    /// saved as, kept with its auto-saves; `None` for a new design.
    title: Option<String>,
    /// The newest save a search found past damage in the file of the
    /// user's the design was opened from, offered as
    /// [`FoundSave`](crate::FoundSave) for [`Request::OpenFound`], till
    /// it's opened.
    found: Option<vrdp::Opened<Document>>,
}

/// A design's entry in the store, held.
#[derive(Debug)]
struct Entry {
    /// Its file name in the store.
    name: String,
    held: Held<Handle>,
}

/// A file of the user's a design is saved to, through the File System
/// Access API.
#[derive(Debug)]
struct Disk {
    handle: FileSystemFileHandle,
    /// What the file held when last read or written, see
    /// [`check_unchanged`]: one read as damaged is never saved to.
    known: Known,
}

impl Open {
    /// The design was just saved to its file, so what was auto-saved is
    /// older. Failing to empty it only leaves an older state to be offered
    /// should the tab close; the next save or a clean close tries again.
    fn saved(&mut self) {
        if let Ok(entry) = &mut self.entry {
            let _ = entry.held.clear();
        }
    }
}

impl Files {
    /// No designs open, and new ones and the settings kept where `stores`
    /// says, if anywhere. The web has no recent files list.
    pub(crate) fn new(stores: Stores) -> Self {
        Self {
            open: OpenFiles::new(),
            named: 0,
            designs: stores.designs,
            settings: stores.settings,
            dir: None,
        }
    }

    /// Whether handling `request` may change the recovered designs, see
    /// `OpenFiles::relists`: any design with an entry may be left holding
    /// it.
    pub(crate) fn relists(&self, request: &Request) -> bool {
        self.open.relists(request, |open| open.entry.is_ok())
    }

    /// Handles `request`, with `object`, what the browser handed over for
    /// the file it picked, if it uses one.
    pub(crate) async fn handle(&mut self, request: Request, object: Option<Handed>) -> Response {
        match request {
            // Refused, as by a failure.
            Request::Open {
                from: Chosen::Path(_),
                ..
            }
            | Request::SaveAs {
                to: SaveTo::Path { .. },
                ..
            }
            | Request::Export {
                to: SaveTo::Path { .. },
                ..
            } => request.failed(NO_FILES.to_owned()),
            Request::Export {
                to: SaveTo::Picked(picked),
                title,
                bodies,
            } => Response::Exported {
                result: export(&picked, object, &title, &bodies).await,
                to: Chosen::File(picked),
            },
            Request::Open {
                id,
                from: Chosen::File(picked),
            } => Response::Opened {
                id,
                path: None,
                result: self.open_picked(id, &picked, object).await,
            },
            Request::Save {
                file,
                revision,
                document,
                thumbnail,
            } => Response::Saved {
                file,
                revision,
                result: self
                    .save(file, &document, &previews(thumbnail.as_ref()))
                    .await,
            },
            Request::SaveAs {
                file,
                to: SaveTo::Picked(picked),
                revision,
                document,
                thumbnail,
            } => {
                let previews = previews(thumbnail.as_ref());
                Response::SavedAs {
                    file,
                    revision,
                    result: self
                        .save_as(file, &picked, object, &document, &previews)
                        .await,
                    to: Chosen::File(picked),
                }
            }
            Request::OpenFound { id, file, found } => Response::Opened {
                id,
                path: None,
                result: self.open_found(file, found),
            },
            Request::New { id } => Response::Created {
                id,
                result: self.create(id).await,
            },
            Request::AutoSave {
                file,
                revision,
                document,
            } => Response::AutoSaved {
                file,
                revision,
                result: self.auto_save(file, &document, Origin::Edited),
            },
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
                result: self.close(file, closing).await,
            },
            Request::Abandon { id } => Response::Abandoned {
                id,
                result: self.abandon(id).await,
            },
            Request::LoadRecent => Response::RecentLoaded {
                entries: Vec::new(),
                home: None,
            },
            // Nor are there paths to read thumbnails from.
            Request::LoadThumbnails { .. } => Response::ThumbnailsLoaded {
                thumbnails: Vec::new(),
            },
            // There's no list to write to; the app never has entries for it.
            Request::WriteRecent { .. } => Response::RecentWritten { result: Ok(()) },
            Request::ListRecovered => Response::RecoveredListed {
                designs: self.list().await,
            },
            Request::OpenRecovered { id, path } => self.open_recovered(id, path).await,
            Request::DiscardRecovered { path } => Response::RecoveredDiscarded {
                result: self.discard(&path).await,
                path,
            },
            Request::LoadSettings => Response::SettingsLoaded {
                settings: match &self.settings {
                    Some(store) => settings::load(store).await,
                    None => Default::default(),
                },
            },
            Request::WriteSettings { settings } => Response::SettingsWritten {
                result: match &self.settings {
                    Some(store) => settings::write(store, &settings)
                        .await
                        .map_err(|e| e.to_string()),
                    None => Ok(()),
                },
            },
            Request::Flush => Response::Flushed,
        }
    }

    /// The store's directory, made if needed.
    async fn dir(&mut self) -> io::Result<FileSystemDirectoryHandle> {
        if let Some(dir) = &self.dir {
            return Ok(dir.clone());
        }
        let designs = self
            .designs
            .as_deref()
            .ok_or_else(|| io::Error::other(NO_STORE))?;
        let dir = opfs::dir(designs).await?;
        self.dir = Some(dir.clone());
        Ok(dir)
    }

    /// Makes and holds an entry for a new design.
    async fn create(&mut self, open_id: OpenId) -> Result<FileId, String> {
        let entry = self.entry().await?;
        Ok(self.open.add(
            Some(open_id),
            Open {
                entry: Ok(entry),
                disk: None,
                title: None,
                found: None,
            },
        ))
    }

    /// Makes and holds a new entry.
    async fn entry(&mut self) -> Result<Entry, String> {
        let error = |e: io::Error| format!("couldn't make a place to auto-save the design: {e}");
        let dir = self.dir().await.map_err(error)?;
        for _ in 0..ATTEMPTS {
            let name = new_name(js_sys::Date::now(), js_sys::Math::random(), self.named);
            // Counted up once per name tried: never overflows.
            self.named += 1;
            let file = file(&dir, &name, true).await.map_err(error)?;
            let handle = match Handle::take(&file).await {
                Ok(handle) => handle,
                // Someone else's, which a name like it is unlikely to be,
                // or deleted by another tab listing entries just now.
                Err(e) if lost_to_another_tab(e.kind()) => continue,
                Err(e) => return Err(error(e)),
            };
            // Someone else's too, left behind: let go of it.
            if !handle.is_empty().map_err(error)? {
                continue;
            }
            return Ok(Entry {
                name,
                held: Held::new(handle),
            });
        }
        Err("couldn't find a free name to auto-save the design under".to_owned())
    }

    /// Appends `document` to `file`'s entry, holding it as `origin` says:
    /// the design as downloaded for [`Request::KeepDownload`].
    fn auto_save(
        &mut self,
        file: FileId,
        document: &Snapshot,
        origin: Origin,
    ) -> Result<(), String> {
        let open = self.open.get_mut(file)?;
        // Based on the file as last read or written, like natively. New
        // designs have none.
        let base = open.disk.as_ref().map(|disk| disk.known.tail());
        let entry = open.entry.as_mut().map_err(|e| e.clone())?;
        entry
            .held
            .append(base, open.title.clone(), document, origin)
            .map_err(|e| e.to_string())
    }

    fn discard_recovery(&mut self, file: FileId) -> Result<(), String> {
        let open = self.open.get_mut(file)?;
        match &mut open.entry {
            Ok(entry) => entry.held.clear().map_err(|e| e.to_string()),
            // Nothing was auto-saved.
            Err(_) => Ok(()),
        }
    }

    /// Lets go of `file`'s entry as `closing` says. The clean close keeps
    /// the design as downloaded, see [`Ending::CloseButDownloaded`].
    async fn close(&mut self, file: FileId, closing: Closing) -> Result<(), String> {
        let open = self.open.remove(file)?;
        let Ok(entry) = open.entry else {
            return Ok(());
        };
        self.end(entry, closing.ending(Ending::CloseButDownloaded))
            .await
            .map_err(|e| format!("couldn't remove the design's entry: {e}"))
    }

    /// Lets go of `entry` as `ending` says, deleting it unless that keeps
    /// what's in it.
    async fn end(&mut self, entry: Entry, ending: Ending) -> io::Result<()> {
        let Entry { name, mut held } = entry;
        let delete = held.end(ending)?;
        // Let go of first: an entry that's held can't be deleted.
        drop(held);
        if delete {
            self.remove(&name).await?;
        }
        Ok(())
    }

    /// Closes the file the open tagged `id` opened, if it's still open.
    async fn abandon(&mut self, id: OpenId) -> Result<(), String> {
        match self.open.opened_by(id) {
            Some(file) => self.close(file, Closing::Keep).await,
            None => Ok(()),
        }
    }

    /// Deletes the entry `name`, which nobody may hold. One that's gone
    /// already, or that another tab took just now, which then finds it
    /// empty and deletes it itself, is fine.
    async fn remove(&mut self, name: &str) -> io::Result<()> {
        let dir = self.dir().await?;
        match call(dir.remove_entry(name)).await {
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ResourceBusy
                ) =>
            {
                Ok(())
            }
            result => result.map(|_| ()),
        }
    }

    /// The new designs left behind by tabs closed or reloaded with them
    /// open, or downloaded and closed since, newest first: entries nobody
    /// holds with a design in them.
    /// Empty ones are deleted. Damaged ones are listed, marked so, see
    /// [`listing`].
    async fn list(&mut self) -> Vec<Recovered> {
        let (Some(designs), Ok(dir)) = (self.designs.clone(), self.dir().await) else {
            return Vec::new();
        };
        let Ok(names) = names(&dir).await else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for name in names {
            // Held by this tab: not left behind.
            let held = |open: &Open| open.entry.as_ref().is_ok_and(|entry| entry.name == name);
            if !is_entry_name(&name) || self.open.values().any(held) {
                continue;
            }
            // Held by another tab if it can't be taken: not left behind
            // either.
            let Ok(file) = file(&dir, &name, false).await else {
                continue;
            };
            let Ok(handle) = Handle::take(&file).await else {
                continue;
            };
            let mut entry = HeldFile::<_, AutoSaved>::new(handle);
            let read = entry.read_with_report();
            // Let go of before anything else: it can't be deleted, nor its
            // time read, while it's held.
            drop(entry);
            match listing(designs.join(&name), modified(&file).await, read) {
                Listing::Listed(recovered) => found.push(recovered),
                Listing::Empty => {
                    let _ = self.remove(&name).await;
                }
                Listing::Skipped => {}
            }
        }
        newest_first(&mut found);
        found
    }

    /// Takes the entry at `path`, listed by [`Files::list`]: one left
    /// behind.
    async fn take(&mut self, path: &Path) -> Result<Entry, String> {
        let designs = self.designs.clone().ok_or(NO_RECOVERED)?;
        let name = listed_entry(&designs, path)?.to_owned();
        let dir = self.dir().await.map_err(|e| e.to_string())?;
        let file = file(&dir, &name, false).await.map_err(|e| e.to_string())?;
        match Handle::take(&file).await {
            Ok(handle) => Ok(Entry {
                name,
                held: Held::new(handle).left_behind(),
            }),
            Err(e) if e.kind() == io::ErrorKind::ResourceBusy => Err(ReadOnly::InUse.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Opens the entry at `path` as a new design, to go on editing it.
    async fn open_recovered(&mut self, open_id: OpenId, path: PathBuf) -> Response {
        let result = match self.take(&path).await {
            Ok(mut entry) => to_open(entry.held.read_with_report()).map(|(saved, damage)| {
                let id = self.open.add(
                    Some(open_id),
                    Open {
                        entry: Ok(entry),
                        // Its handle went with the tab that had it.
                        disk: None,
                        title: saved.name,
                        found: None,
                    },
                );
                Opened::editable(
                    id,
                    Snapshot::unwrap_or_clone(saved.document),
                    saved.origin.is_download(),
                    damage,
                )
            }),
            Err(error) => Err(error),
        };
        Response::Opened {
            id: open_id,
            path: Some(path),
            result,
        }
    }

    /// Opens the design the user picked, see [`Request::Open`].
    async fn open_picked(
        &mut self,
        open_id: OpenId,
        picked: &Picked,
        object: Option<Handed>,
    ) -> Result<Opened, String> {
        let object = object.ok_or_else(|| format!("{} isn't there anymore", picked.name))?;
        let bytes = disk::read(&object).await.map_err(|e| e.to_string())?;
        // One found damaged past what was opened refuses saves.
        let read = from_bytes_with_found(&bytes).map_err(|e| e.to_string())?;
        let damage = Damage::of_design(&read);
        let (known, document) = (read.opened.known(), read.opened.payload);
        let disk = match (picked.from, object) {
            (PickedFrom::Handle, Handed::Handle(handle)) => Some(Disk { handle, known }),
            (PickedFrom::Input, Handed::File(_)) => None,
            _ => {
                return Err(format!(
                    "the browser handed over something else for {}",
                    picked.name
                ));
            }
        };
        // Without an entry, it's only not auto-saved, which auto-saving
        // says.
        let entry = self.entry().await;
        let file = self.open.add(
            Some(open_id),
            Open {
                entry,
                disk,
                title: Some(picked.name.clone()),
                found: read.found,
            },
        );
        // Editable: nothing can lock a file of the user's; the conflict
        // check on saving keeps two tabs from saving over each other.
        Ok(Opened::editable(file, document, false, damage))
    }

    /// Opens the save a search found past damage in the file `file` was
    /// opened from, named by its tail `found`, instead of the one opened,
    /// see [`Request::OpenFound`]. Nothing is offered on opening here, so
    /// there's no offer to look for again.
    fn open_found(&mut self, file: FileId, found: Tail) -> Result<Opened, String> {
        let open = self.open.get_mut(file)?;
        let chosen = open
            .found
            .take_if(|chosen| chosen.tail == found)
            .ok_or_else(|| NOT_FOUND.to_owned())?;
        let damage = Damage::of(&chosen.report, None);
        // Auto-saves are based on it from now on, and saves still refused.
        if let Some(disk) = &mut open.disk {
            disk.known = chosen.known();
        }
        Ok(Opened::editable(file, chosen.payload, false, damage))
    }

    /// Replaces the file `file` was opened from or saved as with
    /// `document`, unless someone else changed it since.
    async fn save(
        &mut self,
        file: FileId,
        document: &Document,
        previews: &[Preview],
    ) -> Result<(), SaveError> {
        let open = self.open.get_mut(file)?;
        let disk = open
            .disk
            .as_mut()
            .ok_or_else(|| SaveError::Failed("the design has no file to save to".to_owned()))?;
        // A file that's no longer a `.vrdp` was changed by someone else:
        // a native save, reading no header, would find other bytes where
        // the last record was. Other errors are `check_unchanged`'s to say,
        // as a native save's are.
        let current = disk::read_handle(&disk.handle).await.map_err(|e| match e {
            FileError::Io(e) => SaveError::Failed(e.to_string()),
            _ => SaveError::Conflict,
        })?;
        // No failed save to allow for: the browser replaces the file as a
        // write is closed, a failure leaving it as it was.
        check_unchanged(current.as_slice(), &disk.known)?;
        let (bytes, known) = whole_file(document, previews)?;
        disk::write(&disk.handle, &bytes)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))?;
        disk.known = known;
        open.saved();
        Ok(())
    }

    /// Writes `document` to the file the user `picked` to save to, and
    /// makes `file` refer to it from then on, see [`Request::SaveAs`].
    async fn save_as(
        &mut self,
        file: Option<FileId>,
        picked: &Picked,
        object: Option<Handed>,
        document: &Document,
        previews: &[Preview],
    ) -> Result<SavedAs, SaveError> {
        let Some(Handed::Handle(handle)) = object else {
            return Err(SaveError::Failed(format!(
                "{} can't be written to",
                picked.name
            )));
        };
        // Refused before writing anything.
        if let Some(file) = file {
            self.open.get_mut(file)?;
        }
        let (bytes, known) = whole_file(document, previews)?;
        disk::write(&handle, &bytes)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))?;
        let disk = Some(Disk { handle, known });
        let title = Some(picked.name.clone());
        let file = match file {
            Some(file) => {
                let open = self.open.get_mut(file)?;
                open.disk = disk;
                open.title = title;
                open.found = None;
                open.saved();
                file
            }
            // A design without an entry gets one, for its auto-saves:
            // without it, it's only not auto-saved, as for
            // `Files::open_picked`. Made after the write, so a failed one
            // leaves nothing behind.
            None => {
                let entry = self.entry().await;
                self.open.add(
                    None,
                    Open {
                        entry,
                        disk,
                        title,
                        found: None,
                    },
                )
            }
        };
        // Designs are only recovered from the welcome screen here, never
        // offered when opened.
        Ok(SavedAs {
            file,
            access: Access::Edit,
            offered: false,
        })
    }

    /// Deletes the entry at `path`, listed by [`Files::list`]: the user
    /// doesn't want it. Refused for one that's open. Changes on top of a
    /// design downloaded go back to it instead, see [`Ending::Discard`].
    async fn discard(&mut self, path: &Path) -> Result<(), String> {
        let entry = self.take(path).await?;
        self.end(entry, Ending::Discard)
            .await
            .map_err(|e| format!("couldn't delete the recovered design: {e}"))
    }
}

/// Writes `bodies` as a 3MF package titled `title` to the file the user
/// `picked` in the save picker, see [`Request::Export`]: replaced whole,
/// like a design saved as it.
async fn export(
    picked: &Picked,
    object: Option<Handed>,
    title: &str,
    bodies: &[three_mf::Body],
) -> Result<(), String> {
    let Some(Handed::Handle(handle)) = object else {
        return Err(format!("{} can't be written to", picked.name));
    };
    let bytes = three_mf::package(title, bodies).map_err(|e| e.to_string())?;
    disk::write(&handle, &bytes)
        .await
        .map_err(|e| format!("couldn't write {}: {e}", picked.name))
}
