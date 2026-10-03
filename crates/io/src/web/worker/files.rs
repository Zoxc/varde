//! The lane on the web: what its IO worker (`src/web.rs`) does with each
//! request, the counterpart of the native lane's `src/native/files.rs`.
//! Designs are saved in browser storage, by name, see `src/browser.rs`; new
//! designs never saved, and designs opened from files of the user's, are
//! auto-saved to entries in the store of new designs, see `src/opfs.rs`,
//! and the latter saved to the files the user picked, see `src/pick.rs`.
//! Getting at files there is asynchronous, so unlike the native lane's
//! this is too; the worker still handles one request at a time.

use std::io;
use std::mem;
use std::path::{Path, PathBuf};

use varde_document::{Document, Snapshot};
use web_sys::FileSystemFileHandle;

use super::disk::{self, Handed};
use super::opfs::{self, Handle, OpfsDir};
use super::settings;
use crate::autosave::{Ending, Held, to_open};
use crate::browser::folder::{self, Design};
use crate::browser::{self, BrowserDesign, LastDownload};
use crate::dir::{Dir, Make, remove_if_free};
use crate::downloads;
use crate::lock::READ_ONLY;
use crate::open::{KEPT, NOT_FOUND, OpenFiles};
use crate::opfs::{lost_to_another_tab, new_name};
use crate::store::{
    ATTEMPTS, Listing, NO_RECOVERED, NO_STORE, is_entry_name, listed_entry, listing, newest_first,
};
use crate::three_mf;
use crate::thumbnail::previews;
use crate::vrdp::{
    self, Error as FileError, Known, Preview, ReadAt, Tail, check_unchanged, from_bytes_with_found,
    whole_file,
};
use crate::{
    Access, Chosen, Closing, Damage, FileId, OpenId, Opened, Picked, PickedFrom, ReadOnly,
    Recovered, Request, Response, SaveError, SaveTo, SavedAs, Stores, UnixSeconds,
};

/// What the web build answers requests for files at a path with: it only
/// has the files the user picks, see `src/pick.rs`, and browser storage.
const NO_FILES: &str = "the web build has no paths to open or save files at";

/// The worker's state: the designs open in this tab.
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
    dir: Option<OpfsDir>,
    /// Browser storage's directory, `saved`, once found.
    browser: Option<OpfsDir>,
    /// The root of the Origin Private File System, where `downloads.toml`
    /// is, once found.
    root: Option<OpfsDir>,
    /// Whether listing browser storage made an entry in the store, of
    /// changes a tab closed as it renamed a design left, since this was
    /// last asked: see [`Files::take_rescued`].
    rescued: bool,
}

/// An open design: where it's kept, and the newest save a search found
/// past damage in the file it was opened from, offered as
/// [`FoundSave`](crate::FoundSave) for [`Request::OpenFound`], till it's
/// opened.
#[derive(Debug)]
struct Open {
    place: Place,
    found: Option<vrdp::Opened<Document>>,
}

/// Where an open design is kept.
#[derive(Debug)]
enum Place {
    /// Auto-saved to an entry in the store of new designs: a design never
    /// saved, or one opened from or saved as a file of the user's.
    Entry(Unsaved),
    /// In browser storage, by name: its saves there, its auto-saves in its
    /// sidecar.
    Browser(Design<Handle>),
}

/// A design auto-saved to an entry in the store of new designs.
#[derive(Debug)]
struct Unsaved {
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

impl Unsaved {
    /// The design was just saved to its file, so what was auto-saved is
    /// older. Failing to empty it only leaves an older state to be offered
    /// should the tab close; the next save or a clean close tries again.
    fn saved(&mut self) {
        if let Ok(entry) = &mut self.entry {
            let _ = entry.held.clear();
        }
    }
}

impl Open {
    /// Whether it has a store entry, which may be left holding it.
    fn has_entry(&self) -> bool {
        matches!(&self.place, Place::Entry(unsaved) if unsaved.entry.is_ok())
    }

    /// Its name in browser storage, if it's kept there.
    fn browser_name(&self) -> Option<&str> {
        match &self.place {
            Place::Browser(design) => Some(design.name()),
            Place::Entry(_) => None,
        }
    }
}

impl Files {
    /// No designs open, and new ones and the settings kept where `stores`
    /// says, if anywhere; browser storage in `saved` and the downloads
    /// made in `downloads.toml`, see `src/browser.rs`. The web has no recent
    /// files list.
    pub(crate) fn new(stores: Stores) -> Self {
        Self {
            open: OpenFiles::new(),
            named: 0,
            designs: stores.designs,
            settings: stores.settings,
            dir: None,
            browser: None,
            root: None,
            rescued: false,
        }
    }

    /// Whether listing browser storage made an entry in the store since
    /// this was last asked: then the store is listed again, as it is
    /// after a request that may change it, see [`Files::relists`].
    pub(crate) fn take_rescued(&mut self) -> bool {
        mem::take(&mut self.rescued)
    }

    /// Whether handling `request` may change the recovered designs, see
    /// `OpenFiles::relists`: any design with an entry may be left holding
    /// it.
    pub(crate) fn relists(&self, request: &Request) -> bool {
        self.open.relists(request, Open::has_entry)
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
                to: SaveTo::Path { .. } | SaveTo::Browser { .. },
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
            Request::Open {
                id,
                from: Chosen::Browser(name),
            } => Response::Opened {
                id,
                path: None,
                result: self.open_in_browser(Some(id), &name).await,
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
                to,
                revision,
                document,
                thumbnail,
            } => {
                let previews = previews(thumbnail.as_ref());
                let result = match &to {
                    SaveTo::Picked(picked) => {
                        self.save_as(file, picked, object, &document, &previews)
                            .await
                    }
                    SaveTo::Browser { name, overwrite } => {
                        self.save_as_in_browser(file, name, *overwrite, &document, &previews)
                            .await
                    }
                    SaveTo::Path { .. } => Err(SaveError::Failed(NO_FILES.to_owned())),
                };
                Response::SavedAs {
                    file,
                    revision,
                    result,
                    to: to.into(),
                }
            }
            Request::OpenFound { id, file, found } => Response::Opened {
                id,
                path: None,
                result: self.open_found(file, found).await,
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
                result: self.auto_save(file, &document),
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
            Request::ListBrowser => Response::BrowserListed {
                designs: self.list_browser().await,
            },
            Request::Rename { file, name } => Response::Renamed {
                result: self.rename(file, &name).await,
                file,
                name,
            },
            Request::DeleteFromBrowser { name } => Response::DeletedFromBrowser {
                result: self.delete_from_browser(&name).await,
                name,
            },
            Request::RecordDownload { file, edited } => Response::DownloadRecorded {
                file,
                result: self.record_download(file, edited).await,
            },
            Request::DownloadFromBrowser { name } => {
                let (result, not_recorded) = match self.download_from_browser(&name).await {
                    Ok((bytes, recorded)) => (Ok(bytes), recorded.err()),
                    Err(error) => (Err(error), None),
                };
                Response::DownloadedFromBrowser {
                    name,
                    result,
                    not_recorded,
                }
            }
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
            // Answered on the page, which keeps it, see `src/panicked.rs`.
            Request::LoadPanic | Request::DiscardPanic { .. } => {
                request.failed("the page keeps the panic recorded".to_owned())
            }
            Request::Flush => Response::Flushed,
        }
    }

    /// The store's directory, made if needed.
    async fn dir(&mut self) -> io::Result<OpfsDir> {
        if let Some(dir) = &self.dir {
            return Ok(dir.clone());
        }
        let designs = self
            .designs
            .as_deref()
            .ok_or_else(|| io::Error::other(NO_STORE))?;
        let dir = OpfsDir(opfs::dir(designs).await?);
        self.dir = Some(dir.clone());
        Ok(dir)
    }

    /// Browser storage's directory, made if needed.
    async fn browser_dir(&mut self) -> io::Result<OpfsDir> {
        if let Some(dir) = &self.browser {
            return Ok(dir.clone());
        }
        let dir = OpfsDir(opfs::dir(Path::new(browser::DIR)).await?);
        self.browser = Some(dir.clone());
        Ok(dir)
    }

    /// The root, where `downloads.toml` is.
    async fn root(&mut self) -> io::Result<OpfsDir> {
        if let Some(root) = &self.root {
            return Ok(root.clone());
        }
        let root = OpfsDir(opfs::root().await?);
        self.root = Some(root.clone());
        Ok(root)
    }

    /// Makes and holds an entry for a new design.
    async fn create(&mut self, open_id: OpenId) -> Result<FileId, String> {
        let entry = self.entry().await?;
        Ok(self.open.add(
            Some(open_id),
            Open {
                place: Place::Entry(Unsaved {
                    entry: Ok(entry),
                    disk: None,
                    title: None,
                }),
                found: None,
            },
        ))
    }

    /// Makes and holds a new entry.
    async fn entry(&mut self) -> Result<Entry, String> {
        let error = |e: io::Error| format!("couldn't make a place to auto-save the design: {e}");
        let dir = self.dir().await.map_err(error)?;
        for _ in 0..ATTEMPTS {
            let name = entry_name(&mut self.named);
            let handle = match dir.take(&name, Make::IfMissing).await {
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

    /// Appends `document` to `file`'s sidecar, or its entry.
    fn auto_save(&mut self, file: FileId, document: &Snapshot) -> Result<(), String> {
        match &mut self.open.get_mut(file)?.place {
            Place::Browser(design) => {
                // What a closed tab left that can't be read is kept as it
                // is till the user discards it.
                if design.lock().kept() {
                    return Err(KEPT.to_owned());
                }
                let base = Some(design.tail());
                let lock = design.lock();
                let sidecar = lock.held().ok_or_else(|| READ_ONLY.to_owned())?;
                sidecar
                    .append(base, None, document)
                    .map_err(|e| e.to_string())?;
                // The UI only auto-saves once the offer is answered.
                lock.answered();
                Ok(())
            }
            Place::Entry(unsaved) => {
                // Based on the file as last read or written, like natively.
                // New designs have none.
                let base = unsaved.disk.as_ref().map(|disk| disk.known.tail());
                let entry = unsaved.entry.as_mut().map_err(|e| e.clone())?;
                entry
                    .held
                    .append(base, unsaved.title.clone(), document)
                    .map_err(|e| e.to_string())
            }
        }
    }

    fn discard_recovery(&mut self, file: FileId) -> Result<(), String> {
        match &mut self.open.get_mut(file)?.place {
            Place::Browser(design) => {
                let lock = design.lock();
                if let Some(sidecar) = lock.held() {
                    sidecar.clear().map_err(|e| e.to_string())?;
                }
                lock.answered();
                Ok(())
            }
            Place::Entry(Unsaved {
                entry: Ok(entry), ..
            }) => entry.held.clear().map_err(|e| e.to_string()),
            // Nothing was auto-saved.
            Place::Entry(_) => Ok(()),
        }
    }

    /// Lets go of `file`'s entry or sidecar as `closing` says.
    async fn close(&mut self, file: FileId, closing: Closing) -> Result<(), String> {
        let open = self.open.remove(file)?;
        self.let_go(open.place, closing.ending())
            .await
            .map_err(|e| format!("couldn't remove the design's auto-saves: {e}"))
    }

    /// Lets go of what `place` holds as `ending` says, deleting it unless
    /// that keeps what's in it.
    async fn let_go(&mut self, place: Place, ending: Ending) -> io::Result<()> {
        match place {
            Place::Entry(Unsaved {
                entry: Ok(entry), ..
            }) => self.end(entry, ending).await,
            Place::Entry(_) => Ok(()),
            Place::Browser(design) => {
                let dir = self.browser_dir().await?;
                folder::close(&dir, design, ending).await
            }
        }
    }

    /// Lets go of what `place` holds once its design is saved elsewhere:
    /// what a closed tab left of a design in browser storage that the user
    /// hasn't answered stays with it, to be offered again. Failing only
    /// leaves it behind, unlocked.
    async fn saved_elsewhere(&mut self, place: Place) {
        let ending = match &place {
            Place::Browser(design) => folder::ending_for_saved_as(design),
            Place::Entry(_) => Ending::Close,
        };
        let _ = self.let_go(place, ending).await;
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
        remove_if_free(&dir, name).await
    }

    /// The new designs left behind by tabs closed or reloaded with them
    /// open, newest first: entries nobody holds with a design in them.
    /// Empty ones are deleted. Damaged ones are listed, marked so, see
    /// [`listing`].
    async fn list(&mut self) -> Vec<Recovered> {
        let (Some(designs), Ok(dir)) = (self.designs.clone(), self.dir().await) else {
            return Vec::new();
        };
        let Ok(names) = dir.names().await else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for name in names {
            // Held by this tab: not left behind.
            let held = |open: &Open| match &open.place {
                Place::Entry(Unsaved {
                    entry: Ok(entry), ..
                }) => entry.name == name,
                _ => false,
            };
            if !is_entry_name(&name) || self.open.values().any(held) {
                continue;
            }
            // Held by another tab if it can't be taken: not left behind
            // either.
            let Ok(handle) = dir.take(&name, Make::No).await else {
                continue;
            };
            let mut entry = Held::new(handle);
            let read = entry.read_with_report();
            // Let go of before anything else: it can't be deleted, nor its
            // time read, while it's held.
            drop(entry);
            match listing(designs.join(&name), dir.modified(&name).await, read) {
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
        match dir.take(&name, Make::No).await {
            Ok(handle) => Ok(Entry {
                name,
                held: Held::new(handle),
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
                        place: Place::Entry(Unsaved {
                            entry: Ok(entry),
                            // Its handle went with the tab that had it.
                            disk: None,
                            title: saved.name,
                        }),
                        found: None,
                    },
                );
                Opened::editable(id, Snapshot::unwrap_or_clone(saved.document), damage)
            }),
            Err(error) => Err(error),
        };
        Response::Opened {
            id: open_id,
            path: Some(path),
            result,
        }
    }

    /// Opens the design the user picked, see [`Request::Open`]: one from a
    /// file input is copied into browser storage and opened from there,
    /// or, should that fail, say with the site's data blocked, opened as a
    /// copy, a new design known by the file's name, as before there was
    /// browser storage ([`Opened::not_copied`] says why).
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
        let handle = match (picked.from, object) {
            (PickedFrom::Handle, Handed::Handle(handle)) => Some(handle),
            (PickedFrom::Input, Handed::File(_)) => None,
            _ => {
                return Err(format!(
                    "the browser handed over something else for {}",
                    picked.name
                ));
            }
        };
        let (read, not_copied) = match handle {
            Some(_) => (read, None),
            // Copied as it is, with its history, and opened from there.
            None => match self.copy_in(&picked.name, &bytes, read).await {
                Ok(opened) => return Ok(self.opened_in_browser(Some(open_id), opened).await),
                Err(not_copied) => {
                    let (error, read) = *not_copied;
                    (read, Some(error))
                }
            },
        };
        let damage = Damage::of_design(&read);
        let (known, document) = (read.opened.known(), read.opened.payload);
        // Without an entry, it's only not auto-saved, which auto-saving
        // says.
        let entry = self.entry().await;
        let file = self.open.add(
            Some(open_id),
            Open {
                place: Place::Entry(Unsaved {
                    entry,
                    disk: handle.map(|handle| Disk { handle, known }),
                    title: Some(picked.name.clone()),
                }),
                found: read.found,
            },
        );
        // Editable: nothing can lock a file of the user's; the conflict
        // check on saving keeps two tabs from saving over each other.
        Ok(Opened {
            not_copied,
            ..Opened::editable(file, document, damage)
        })
    }

    /// Copies `bytes`, the whole of the file `file`, read as `read`, into
    /// browser storage and opens it there, see [`folder::copy_in`], or says
    /// why it couldn't, handing `read` back. The downloads recorded of a
    /// design of the name before are of another.
    async fn copy_in(
        &mut self,
        file: &str,
        bytes: &[u8],
        read: vrdp::WithFound,
    ) -> Result<folder::Opened<Handle>, folder::NotCopied> {
        let dir = match self.browser_dir().await {
            Ok(dir) => dir,
            Err(e) => {
                let error = format!("couldn't copy it into browser storage: {e}");
                return Err(Box::new((error, read)));
            }
        };
        let opened = folder::copy_in(&dir, file, bytes, read).await?;
        self.forget_downloads(opened.design.name()).await;
        Ok(opened)
    }

    /// Forgets the downloads recorded of the design `name`, written anew:
    /// they're of another design of the name. Best effort: what's lost is
    /// only what the downloads say.
    async fn forget_downloads(&mut self, name: &str) {
        if let Ok(root) = self.root().await {
            let _ = downloads::update(&root, |downloads| downloads.removed(name)).await;
        }
    }

    /// Opens the design `name` in browser storage, see `src/browser.rs`:
    /// read-only if another tab has it open, as natively.
    async fn open_in_browser(
        &mut self,
        open_id: Option<OpenId>,
        name: &str,
    ) -> Result<Opened, String> {
        let dir = self.browser_dir().await.map_err(|e| e.to_string())?;
        let opened = folder::open(&dir, name).await?;
        Ok(self.opened_in_browser(open_id, opened).await)
    }

    /// `opened`, a design in browser storage, as a file of the lane's,
    /// opened by the open tagged `open_id`, if any, with its last download.
    async fn opened_in_browser(
        &mut self,
        open_id: Option<OpenId>,
        opened: folder::Opened<Handle>,
    ) -> Opened {
        let name = opened.design.name().to_owned();
        let damage = Damage::of_design(&opened.read);
        let access = opened.design.access();
        let download = self.last_download(&name, opened.read.opened.tail).await;
        let document = opened.read.opened.payload;
        let file = self.open.add(
            open_id,
            Open {
                place: Place::Browser(opened.design),
                found: opened.read.found,
            },
        );
        Opened {
            file,
            document,
            access,
            recovered: opened.recovered,
            browser: Some(name),
            not_copied: None,
            download,
            damage,
        }
    }

    /// The last download recorded of the design `name`, whose file ends at
    /// `tail`.
    async fn last_download(&mut self, name: &str, tail: Tail) -> Option<LastDownload> {
        let root = self.root().await.ok()?;
        downloads::load(&root).await.last(name, tail.sum())
    }

    /// Opens the save a search found past damage in the file `file` was
    /// opened from, named by its tail `found`, instead of the one opened,
    /// see [`Request::OpenFound`]. What a closed tab left of a design in
    /// browser storage is offered again, against it.
    async fn open_found(&mut self, file: FileId, found: Tail) -> Result<Opened, String> {
        let open = self.open.get_mut(file)?;
        let chosen = open
            .found
            .take_if(|chosen| chosen.tail == found)
            .ok_or_else(|| NOT_FOUND.to_owned())?;
        let damage = Damage::of(&chosen.report, None);
        match &mut open.place {
            Place::Browser(design) => {
                let document = design.open_found(chosen);
                let (name, tail) = (design.name().to_owned(), design.tail());
                let dir = self.browser_dir().await.map_err(|e| e.to_string())?;
                let download = self.last_download(&name, tail).await;
                let Place::Browser(design) = &mut self.open.get_mut(file)?.place else {
                    return Err(NOT_FOUND.to_owned());
                };
                let recovered = folder::offer_again(&dir, design, &document).await;
                Ok(Opened {
                    file,
                    document,
                    access: design.access(),
                    recovered,
                    browser: Some(name),
                    not_copied: None,
                    download,
                    damage,
                })
            }
            Place::Entry(unsaved) => {
                // Auto-saves are based on it from now on, and saves still
                // refused.
                if let Some(disk) = &mut unsaved.disk {
                    disk.known = chosen.known();
                }
                Ok(Opened::editable(file, chosen.payload, damage))
            }
        }
    }

    /// Saves `document` to `file`'s design: appended in browser storage,
    /// or replacing the file of the user's it was opened from or saved as,
    /// unless someone else changed it since.
    async fn save(
        &mut self,
        file: FileId,
        document: &Document,
        previews: &[Preview],
    ) -> Result<(), SaveError> {
        if let Place::Browser(_) = self.open.get_mut(file)?.place {
            let dir = (self.browser_dir().await).map_err(|e| SaveError::Failed(e.to_string()))?;
            let Place::Browser(design) = &mut self.open.get_mut(file)?.place else {
                return Err(SaveError::Failed(NOT_FOUND.to_owned()));
            };
            return folder::save(&dir, design, document, previews).await;
        }
        let Place::Entry(unsaved) = &mut self.open.get_mut(file)?.place else {
            return Err(SaveError::Failed(NOT_FOUND.to_owned()));
        };
        let disk = unsaved
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
        unsaved.saved();
        Ok(())
    }

    /// Writes `document` to the file the user `picked` to save to, and
    /// makes `file` refer to it from then on, see [`Request::SaveAs`]. A
    /// design in browser storage lets go of it, and gets an entry for its
    /// auto-saves.
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
                let has_entry = matches!(self.open.get_mut(file)?.place, Place::Entry(_));
                // A design leaving browser storage gets an entry for its
                // auto-saves, as one opened from a file of the user's.
                let entry = if has_entry {
                    None
                } else {
                    Some(self.entry().await)
                };
                let open = self.open.get_mut(file)?;
                open.found = None;
                match (&mut open.place, entry) {
                    (Place::Entry(unsaved), None) => {
                        unsaved.disk = disk;
                        unsaved.title = title;
                        unsaved.saved();
                    }
                    (_, entry) => {
                        let unsaved = Unsaved {
                            entry: entry.unwrap_or_else(|| Err(NOT_FOUND.to_owned())),
                            disk,
                            title,
                        };
                        let old = mem::replace(&mut open.place, Place::Entry(unsaved));
                        self.saved_elsewhere(old).await;
                    }
                }
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
                        place: Place::Entry(Unsaved { entry, disk, title }),
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

    /// Writes `document` as the design `name` in browser storage, replacing
    /// one there only if `overwrite`, else refused as
    /// [`SaveError::Taken`], and makes `file` refer to it from then on, see
    /// [`Request::SaveAs`]: over the design itself, which the app does only
    /// for one opened past damage, as Save appends otherwise, it keeps its
    /// lock; otherwise it lets go of what `file` held. A design written
    /// anew under a name has none of the downloads recorded of the name.
    async fn save_as_in_browser(
        &mut self,
        file: Option<FileId>,
        name: &str,
        overwrite: bool,
        document: &Document,
        previews: &[Preview],
    ) -> Result<SavedAs, SaveError> {
        let dir = (self.browser_dir().await).map_err(|e| SaveError::Failed(e.to_string()))?;
        if let Some(file) = file {
            let open = self.open.get_mut(file)?;
            if let Place::Browser(design) = &mut open.place
                && design.name() == name
            {
                if !overwrite {
                    return Err(SaveError::Taken);
                }
                folder::save_over(&dir, design, document, previews).await?;
                open.found = None;
                return Ok(SavedAs {
                    file,
                    access: design.access(),
                    offered: design.lock().offered(),
                });
            }
        }
        let design = folder::save_as(&dir, name, overwrite, document, previews).await?;
        self.forget_downloads(name).await;
        let access = design.access();
        let place = Place::Browser(design);
        let file = match file {
            Some(file) => {
                let open = self.open.get_mut(file)?;
                open.found = None;
                let old = mem::replace(&mut open.place, place);
                self.saved_elsewhere(old).await;
                file
            }
            None => self.open.add(None, Open { place, found: None }),
        };
        Ok(SavedAs {
            file,
            access,
            // A lock just taken has offered nothing.
            offered: false,
        })
    }

    /// Gives `file`, a design in browser storage, the name `name`, see
    /// [`Request::Rename`].
    async fn rename(&mut self, file: FileId, name: &str) -> Result<(), String> {
        let dir = self.browser_dir().await.map_err(|e| e.to_string())?;
        let Place::Browser(design) = &mut self.open.get_mut(file)?.place else {
            return Err("only a design in browser storage is renamed".to_owned());
        };
        let from = design.name().to_owned();
        folder::rename(&dir, design, name).await?;
        // Best effort: what's lost is only what the downloads say.
        if let Ok(root) = self.root().await {
            let _ = downloads::update(&root, |downloads| downloads.renamed(&from, name)).await;
        }
        Ok(())
    }

    /// The designs in browser storage, see [`Request::ListBrowser`].
    /// Changes a tab closed as it renamed a design left go to the store,
    /// as a new design, see [`folder::list`].
    async fn list_browser(&mut self) -> Vec<BrowserDesign> {
        let Ok(dir) = self.browser_dir().await else {
            return Vec::new();
        };
        let downloads = match self.root().await {
            Ok(root) => downloads::load(&root).await,
            Err(_) => Default::default(),
        };
        let store = self.dir().await.ok();
        let (open, named) = (&self.open, &mut self.named);
        let held = |name: &str| open.values().any(|open| open.browser_name() == Some(name));
        let listing =
            folder::list(&dir, store.as_ref(), || entry_name(named), &downloads, held).await;
        self.rescued |= listing.rescued;
        listing.designs
    }

    /// Deletes the design `name` in browser storage, see
    /// [`Request::DeleteFromBrowser`].
    async fn delete_from_browser(&mut self, name: &str) -> Result<(), String> {
        let dir = self.browser_dir().await.map_err(|e| e.to_string())?;
        let held = (self.open.values()).any(|open| open.browser_name() == Some(name));
        folder::delete(&dir, name, held).await?;
        self.forget_downloads(name).await;
        Ok(())
    }

    /// Records that `file` was just downloaded, see
    /// [`Request::RecordDownload`].
    async fn record_download(
        &mut self,
        file: FileId,
        edited: bool,
    ) -> Result<Option<LastDownload>, String> {
        let Place::Browser(design) = &mut self.open.get_mut(file)?.place else {
            return Ok(None);
        };
        let (name, sum) = (design.name().to_owned(), design.tail().sum());
        let time = UnixSeconds::now();
        let root = self.root().await.map_err(|e| e.to_string())?;
        downloads::update(&root, |downloads| {
            downloads.record(&name, time, (!edited).then_some(sum));
        })
        .await
        .map_err(|e| format!("couldn't record the download: {e}"))?;
        Ok(Some(LastDownload {
            time,
            latest: !edited,
        }))
    }

    /// The design `name` in browser storage, whole, to download, see
    /// [`Request::DownloadFromBrowser`], and whether the download could be
    /// recorded: it's downloaded either way.
    async fn download_from_browser(
        &mut self,
        name: &str,
    ) -> Result<(Vec<u8>, Result<(), String>), String> {
        let dir = self.browser_dir().await.map_err(|e| e.to_string())?;
        let (bytes, sum) = folder::read_whole(&dir, name).await?;
        let time = UnixSeconds::now();
        let recorded = match self.root().await {
            Ok(root) => {
                downloads::update(&root, |downloads| downloads.record(name, time, Some(sum))).await
            }
            Err(e) => Err(e),
        };
        Ok((bytes, recorded.map_err(|e| e.to_string())))
    }

    /// Deletes the entry at `path`, listed by [`Files::list`]: the user
    /// doesn't want it. Refused for one that's open.
    async fn discard(&mut self, path: &Path) -> Result<(), String> {
        let entry = self.take(path).await?;
        self.end(entry, Ending::Close)
            .await
            .map_err(|e| format!("couldn't delete the recovered design: {e}"))
    }
}

/// A name for a new entry in the store, `named` counting the names made:
/// never one made before in this tab, and unlikely to be one another tab
/// made.
fn entry_name(named: &mut u64) -> String {
    let name = new_name(js_sys::Date::now(), js_sys::Math::random(), *named);
    // Counted up once per name made: never overflows.
    *named += 1;
    name
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
