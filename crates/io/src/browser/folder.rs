//! Browser storage's designs over a [`Dir`], see the parent module:
//! opening, saving, saving as, renaming, deleting, listing and copying
//! designs in.
//!
//! Whoever writes a design's file whole, under a temporary name put in
//! place after (see [`write()`]), holds the design's sidecar meanwhile:
//! saving as it, saving over it, copying a file in as it. So listing,
//! which puts in place what a tab closed in between left under a
//! temporary name, takes the sidecar before touching that, and leaves it
//! be while someone holds it.

use std::io;

use varde_document::Document;

use super::{BrowserDesign, DownloadStatus, is_design_name, within};
use crate::autosave::{AutoSaved, Ending, Held, Origin};
use crate::dir::{Dir, Make, remove_if_free, take_waiting};
use crate::downloads::Downloads;
use crate::lock::{Lock, READ_ONLY};
use crate::vrdp::{
    self, Error as FileError, FileEnd, Known, Preview, Storage, Tail, WithFound,
    from_bytes_with_report, write_new,
};
use crate::wire::MAX_MESSAGE_BYTES;
use crate::{Access, ListedDamage, Offer, ReadOnly, RecoveryError, SaveError, thumbnail};

/// The sidecar of the design `name`: `.{name}.autosave`, as natively.
pub(crate) fn sidecar_name(name: &str) -> String {
    format!(".{name}.autosave")
}

/// The design whose sidecar `name` is, if it's one.
fn sidecar_of(name: &str) -> Option<&str> {
    let design = name.strip_prefix('.')?.strip_suffix(".autosave")?;
    is_design_name(design).then_some(design)
}

/// A temporary file the design `name` is written to before it's put in
/// place: `.{name}.{id}.tmp`, `id` in hex, which [`MAX_NAME`] leaves room
/// for.
///
/// [`MAX_NAME`]: super::MAX_NAME
fn temp_name(name: &str, id: u64) -> String {
    format!(".{name}.{id:x}.tmp")
}

/// The design the temporary file `name` was to be, if it's one.
fn temp_of(name: &str) -> Option<&str> {
    let (design, id) = name
        .strip_prefix('.')?
        .strip_suffix(".tmp")?
        .rsplit_once('.')?;
    let hex = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit());
    (hex && is_design_name(design)).then_some(design)
}

/// How many names [`copy_name`] tries: `name`, then `name (2)` up to this.
const COPIES: u32 = 999;

/// The file name for a copy of the file `file` in browser storage: its
/// name as a design's (see [`file_name`](super::file_name)), made unique
/// with " (2)", " (3)" and so on while `taken` says it's taken, the stem
/// cut to make room for them. `None` if they all are.
pub(crate) fn copy_name(file: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    (1..=COPIES)
        .map(|n| match n {
            1 => super::file_name(file),
            n => within(file, &format!(" ({n})")),
        })
        .find(|name| !taken(name))
}

/// A design in browser storage, open: its file name, what's known of its
/// file to save to it, and its lock.
#[derive(Debug)]
pub(crate) struct Design<F> {
    name: String,
    known: Known,
    lock: Lock<Held<F>>,
}

impl<F: Storage> Design<F> {
    /// Its file name in browser storage.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Its file as last read or written, which auto-saves are based on.
    pub(crate) fn tail(&self) -> Tail {
        self.known.tail()
    }

    pub(crate) fn lock(&mut self) -> &mut Lock<Held<F>> {
        &mut self.lock
    }

    pub(crate) fn access(&self) -> Access {
        self.lock.access()
    }

    /// Goes over to `found`, the save a search found past damage in the
    /// file as it was opened, returning what it holds: auto-saves are
    /// based on it from now on, and saves still refused.
    pub(crate) fn open_found(&mut self, found: vrdp::Opened<Document>) -> Document {
        self.known = found.known();
        found.payload
    }
}

/// A design opened from browser storage, see [`open`].
#[derive(Debug)]
pub(crate) struct Opened<F> {
    pub(crate) design: Design<F>,
    /// The design file as read, with the save a search found past damage,
    /// if any.
    pub(crate) read: WithFound,
    /// What a tab closed with changes left in its sidecar, to offer.
    pub(crate) recovered: Result<Option<Offer>, RecoveryError>,
}

/// What a tab closed with changes left in the sidecar of `design`, offered
/// again against `document`, the save a search found past damage, just
/// gone over to (see [`Design::open_found`]).
pub(crate) async fn offer_again<D: Dir>(
    dir: &D,
    design: &mut Design<D::File>,
    document: &Document,
) -> Result<Option<Offer>, RecoveryError> {
    let bytes = dir
        .read(&design.name, MAX_MESSAGE_BYTES)
        .await
        .unwrap_or_default();
    let tail = design.tail();
    design
        .lock
        .offer(tail, |base| based_past(&bytes, tail, base), document)
}

/// Whether an auto-save based on `base` was based on a newer save than the
/// one opened, at `opened`, of the file read as `bytes`.
fn based_past(bytes: &[u8], opened: Tail, base: Tail) -> bool {
    vrdp::based_past(bytes, opened, base).unwrap_or(false)
}

/// Opens the design `name` in `dir` for editing: read first, so a name
/// that isn't a design never gets a sidecar, then its sidecar taken, or
/// read-only if another tab holds it. Another tab saving in between is
/// caught by the next save's check, as natively.
pub(crate) async fn open<D: Dir>(dir: &D, name: &str) -> Result<Opened<D::File>, String> {
    if !is_design_name(name) {
        return Err(format!("{name} isn't a design in browser storage"));
    }
    let bytes = dir
        .read(name, MAX_MESSAGE_BYTES)
        .await
        .map_err(|e| read_error(name, &e))?;
    let read = vrdp::from_bytes_with_found(&bytes).map_err(|e| e.to_string())?;
    let sidecar = take_sidecar(dir, name).await;
    Ok(opened(name, &bytes, read, sidecar))
}

/// The design `name`, its file `bytes` as `read`, opened with its sidecar
/// `taken`, or read-only: what a closed tab left in it offered.
fn opened<F: Storage>(
    name: &str,
    bytes: &[u8],
    read: WithFound,
    taken: Result<Held<F>, ReadOnly>,
) -> Opened<F> {
    let mut lock = Lock::new(taken);
    let tail = read.opened.tail;
    let recovered = lock.offer(
        tail,
        |base| based_past(bytes, tail, base),
        &read.opened.payload,
    );
    let design = Design {
        name: name.to_owned(),
        known: read.opened.known(),
        lock,
    };
    Opened {
        design,
        read,
        recovered,
    }
}

/// What `error`, reading or taking the design `name`, means to the user.
fn read_error(name: &str, error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => format!("{name} isn't in browser storage"),
        io::ErrorKind::ResourceBusy => format!("{name} is being saved elsewhere just now"),
        _ => error.to_string(),
    }
}

/// Takes the sidecar of the design `name`, made if needed, or says why the
/// design is read-only: another tab holding it a moment, as it lists the
/// designs, is waited for, see [`take_waiting`].
async fn take_sidecar<D: Dir>(dir: &D, name: &str) -> Result<Held<D::File>, ReadOnly> {
    match take_waiting(dir, &sidecar_name(name), Make::IfMissing).await {
        Ok(file) => Ok(Held::new(file)),
        Err(e) if e.kind() == io::ErrorKind::ResourceBusy => Err(ReadOnly::InUse),
        Err(e) => Err(ReadOnly::NoLock(format!(
            "couldn't make the design's lock in browser storage: {e}"
        ))),
    }
}

/// What a save failing with `error` says: browser storage running out of
/// room is told plainly, as it's the user's to make room.
pub(crate) fn save_error(error: FileError) -> SaveError {
    match error {
        FileError::Io(e) if e.kind() == io::ErrorKind::StorageFull => {
            SaveError::Failed(FULL.to_owned())
        }
        error => error.into(),
    }
}

/// What browser storage running out of room says.
pub(crate) const FULL: &str =
    "browser storage is full: download designs and delete them, or free space";

/// Appends `document` and `previews` to `design`'s file as a new save,
/// unless someone else changed it since, see [`vrdp::save`]: its history
/// is kept. Its sidecar is emptied after, as what it held is older.
pub(crate) async fn save<D: Dir>(
    dir: &D,
    design: &mut Design<D::File>,
    document: &Document,
    previews: &[Preview],
) -> Result<(), SaveError> {
    if !design.lock.is_held() {
        return Err(SaveError::Failed(READ_ONLY.to_owned()));
    }
    let mut file = dir
        .take(&design.name, Make::No)
        .await
        .map_err(|e| SaveError::Failed(read_error(&design.name, &e)))?;
    vrdp::save(&mut file, &mut design.known, document, previews).map_err(save_error)?;
    drop(file);
    design.lock.saved();
    Ok(())
}

/// Writes `document` and `previews` as the design `name`, a new file of
/// one save, see [`write()`]. Its sidecar is taken first and held while
/// it's written, refused if another tab holds it, as it has the design
/// open. A design there already is replaced only if `overwrite`, else it's
/// [`SaveError::Taken`]; so is a sidecar of no design with changes in it,
/// left by a tab closed as it renamed a design, which listing offers back
/// (see [`list`]). Replaced, whatever a closed tab left in its sidecar is
/// of the design replaced, and older than what's written, so it's emptied.
pub(crate) async fn save_as<D: Dir>(
    dir: &D,
    name: &str,
    overwrite: bool,
    document: &Document,
    previews: &[Preview],
) -> Result<Design<D::File>, SaveError> {
    if !is_design_name(name) {
        return Err(SaveError::Failed(format!(
            "{name} isn't a design's name in browser storage"
        )));
    }
    let sidecar = match take_sidecar(dir, name).await {
        Ok(sidecar) => sidecar,
        Err(ReadOnly::InUse) => {
            return Err(SaveError::Failed(format!("{name} is open elsewhere")));
        }
        Err(ReadOnly::NoLock(why)) => return Err(SaveError::Failed(why)),
    };
    let written = if !overwrite && !sidecar.is_empty().unwrap_or(true) {
        Err(SaveError::Taken)
    } else {
        write(dir, name, overwrite, |file| {
            write_new(file, document, previews)
        })
        .await
    };
    match written {
        Ok(known) => {
            let mut design = Design {
                name: name.to_owned(),
                known,
                lock: Lock::new(Ok(sidecar)),
            };
            design.lock.saved();
            Ok(design)
        }
        Err(error) => {
            // Anything in it is a closed tab's of the design that wasn't
            // replaced after all, to be offered still.
            let _ = end(dir, name, sidecar, Ending::Release).await;
            Err(error)
        }
    }
}

/// Writes `document` and `previews` over `design`'s own file, as a new
/// file of one save, as a Save As over the design itself does once the
/// user agrees to replace it, keeping the lock it holds, and its history
/// gone.
pub(crate) async fn save_over<D: Dir>(
    dir: &D,
    design: &mut Design<D::File>,
    document: &Document,
    previews: &[Preview],
) -> Result<(), SaveError> {
    if !design.lock.is_held() {
        return Err(SaveError::Failed(READ_ONLY.to_owned()));
    }
    design.known = write(dir, &design.name, true, |file| {
        write_new(file, document, previews)
    })
    .await?;
    design.lock.saved();
    Ok(())
}

/// Writes the new file `name` as `fill` fills it, or over the one there if
/// `overwrite`: written whole under a temporary name, made durable, then
/// put in place, so that the design there is never half written, nor
/// replaced by one that is. A file `name` there already, unless it's
/// replaced, is [`SaveError::Taken`]. The caller holds the design's
/// sidecar throughout, see the module docs. Returns what `fill` does.
async fn write<D: Dir, T>(
    dir: &D,
    name: &str,
    overwrite: bool,
    fill: impl FnOnce(&mut D::File) -> Result<T, FileError>,
) -> Result<T, SaveError> {
    let temp = temp_name(name, random()?);
    let mut file = dir
        .take(&temp, Make::New)
        .await
        .map_err(|e| save_error(e.into()))?;
    let written = fill(&mut file).and_then(|filled| {
        file.sync()?;
        Ok(filled)
    });
    drop(file);
    let filled = match written {
        Ok(filled) => filled,
        Err(error) => {
            let _ = dir.remove(&temp).await;
            return Err(save_error(error));
        }
    };
    if overwrite {
        match dir.remove(name).await {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                let _ = dir.remove(&temp).await;
                return Err(SaveError::Failed(read_error(name, &e)));
            }
        }
    }
    match dir.rename(&temp, name).await {
        Ok(()) => Ok(filled),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let _ = dir.remove(&temp).await;
            Err(SaveError::Taken)
        }
        // The design replaced is gone: listing puts this in its place.
        Err(e) if overwrite => Err(save_error(e.into())),
        Err(e) => {
            let _ = dir.remove(&temp).await;
            Err(save_error(e.into()))
        }
    }
}

/// A random number, for a temporary file's name.
fn random() -> Result<u64, SaveError> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes)
        .map_err(|e| SaveError::Failed(format!("no random number for a file name: {e}")))?;
    Ok(u64::from_le_bytes(bytes))
}

/// Gives `design` the file name `to`, keeping its saves: its sidecar for
/// the new name taken first, refused if another tab holds it, the file
/// moved, refused if a design is called `to` already, and what was
/// auto-saved of it moved over to the new sidecar, its newest auto-save,
/// still offered if it was.
pub(crate) async fn rename<D: Dir>(
    dir: &D,
    design: &mut Design<D::File>,
    to: &str,
) -> Result<(), String> {
    if !is_design_name(to) {
        return Err(format!("{to} isn't a design's name in browser storage"));
    }
    if to == design.name {
        return Ok(());
    }
    if !design.lock.is_held() {
        return Err(READ_ONLY.to_owned());
    }
    if design.lock.kept() {
        return Err(crate::open::KEPT.to_owned());
    }
    let mut sidecar = match take_sidecar(dir, to).await {
        Ok(sidecar) => sidecar,
        Err(ReadOnly::InUse) => return Err(format!("{to} is open elsewhere")),
        Err(ReadOnly::NoLock(why)) => return Err(why),
    };
    let taken = || format!("{to} is in browser storage already");
    // Changes a closed tab left of no design, offered back by listing.
    if !sidecar.is_empty().unwrap_or(true) {
        let _ = end(dir, to, sidecar, Ending::Release).await;
        return Err(taken());
    }
    if let Err(e) = dir.rename(&design.name, to).await {
        let _ = end(dir, to, sidecar, Ending::Release).await;
        return Err(match e.kind() {
            io::ErrorKind::AlreadyExists => taken(),
            _ => format!("couldn't rename {}: {e}", design.name),
        });
    }
    // Should anything be in it, it's of no design.
    let _ = sidecar.clear();
    let mut lock = Lock::new(Ok(sidecar));
    let old = std::mem::replace(&mut design.lock, Lock::ReadOnly(ReadOnly::InUse));
    let offered = old.offered();
    if let Some(mut held) = old.into_held() {
        let newest: Option<AutoSaved> = held.read().ok().flatten();
        if let (Some(newest), Some(new)) = (newest, lock.held()) {
            // Best effort: should it fail, the next auto-save writes it.
            let _ = new.append_saved(&newest);
        }
        let _ = end(dir, &design.name, held, Ending::Close).await;
    }
    if offered && let Lock::Sidecar { offered, .. } = &mut lock {
        *offered = true;
    }
    design.lock = lock;
    design.name = to.to_owned();
    Ok(())
}

/// Lets go of the sidecar `held` of the design `name` as `ending` says,
/// deleting it unless that keeps what's in it.
pub(crate) async fn end<D: Dir>(
    dir: &D,
    name: &str,
    mut held: Held<D::File>,
    ending: Ending,
) -> io::Result<()> {
    let delete = held.end(ending)?;
    // Let go of first: a file that's held can't be deleted.
    drop(held);
    if delete {
        remove_if_free(dir, &sidecar_name(name)).await?;
    }
    Ok(())
}

/// Lets go of `design` as `ending` says, as it's closed.
pub(crate) async fn close<D: Dir>(
    dir: &D,
    design: Design<D::File>,
    ending: Ending,
) -> io::Result<()> {
    match design.lock.into_held() {
        Some(held) => end(dir, &design.name, held, ending).await,
        None => Ok(()),
    }
}

/// How `design` lets go of its sidecar as it's saved as another design:
/// what a closed tab left that the user hasn't answered stays with it, to
/// be offered again.
pub(crate) fn ending_for_saved_as<F: Storage>(design: &Design<F>) -> Ending {
    if design.lock.offered() {
        Ending::Release
    } else {
        Ending::Close
    }
}

/// Deletes the design `name` and its sidecar, refused while it's open,
/// in this tab (as `held` says) or another.
pub(crate) async fn delete<D: Dir>(dir: &D, name: &str, held: bool) -> Result<(), String> {
    if !is_design_name(name) {
        return Err(format!("{name} isn't a design in browser storage"));
    }
    let open_elsewhere = || format!("{name} is open, so it can't be deleted");
    if held {
        return Err(open_elsewhere());
    }
    let sidecar = match take_sidecar(dir, name).await {
        Ok(sidecar) => sidecar,
        Err(ReadOnly::InUse) => return Err(open_elsewhere()),
        Err(ReadOnly::NoLock(why)) => return Err(why),
    };
    let removed = match dir.remove(name).await {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(read_error(name, &e)),
        _ => Ok(()),
    };
    let ending = if removed.is_ok() {
        Ending::Close
    } else {
        Ending::Release
    };
    end(dir, name, sidecar, ending)
        .await
        .map_err(|e| e.to_string())?;
    removed
}

/// Copies `bytes`, the whole of a design file called `file`, read as
/// `read`, into browser storage under its name, made unique, see
/// [`copy_name`], and opens it from there. Written as it is, with its
/// history, as [`save_as`] writes: its sidecar taken first, made new, so
/// that one left by a tab closed as it renamed a design isn't taken over,
/// and held while it's written; a name another tab takes meanwhile is
/// passed over for the next. Should it fail, why, with `read` back.
pub(crate) async fn copy_in<D: Dir>(
    dir: &D,
    file: &str,
    bytes: &[u8],
    read: WithFound,
) -> Result<Opened<D::File>, NotCopied> {
    let names = match dir.names().await {
        Ok(names) => names,
        Err(e) => {
            return Err(Box::new((
                copy_error(&SaveError::from(FileError::Io(e))),
                read,
            )));
        }
    };
    let mut tried: Vec<String> = Vec::new();
    for _ in 0..crate::store::ATTEMPTS {
        let taken = |name: &str| {
            let sidecar = sidecar_name(name);
            (names.iter().chain(&tried)).any(|taken| *taken == name || *taken == sidecar)
        };
        let Some(name) = copy_name(file, taken) else {
            let error = format!("there are too many designs called {file} already");
            return Err(Box::new((error, read)));
        };
        let sidecar = match dir.take(&sidecar_name(&name), Make::New).await {
            Ok(sidecar) => Held::new(sidecar),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::AlreadyExists
                        | io::ErrorKind::ResourceBusy
                        | io::ErrorKind::NotFound
                ) =>
            {
                tried.push(name);
                continue;
            }
            Err(e) => {
                return Err(Box::new((
                    copy_error(&SaveError::from(FileError::Io(e))),
                    read,
                )));
            }
        };
        let written = write(dir, &name, false, |copy| {
            copy.write_at(bytes, 0).map_err(FileError::Io)
        })
        .await;
        match written {
            Ok(()) => return Ok(opened(&name, bytes, read, Ok(sidecar))),
            Err(SaveError::Taken) => {
                let _ = end(dir, &name, sidecar, Ending::Release).await;
                tried.push(name);
            }
            Err(error) => {
                let _ = end(dir, &name, sidecar, Ending::Release).await;
                return Err(Box::new((copy_error(&error), read)));
            }
        }
    }
    Err(Box::new((
        format!("couldn't find a free name for {file}"),
        read,
    )))
}

/// Why a file wasn't copied into browser storage, see [`copy_in`], and
/// the file as read, handed back.
pub(crate) type NotCopied = Box<(String, WithFound)>;

/// What copying a file into browser storage failing with `error` says.
fn copy_error(error: &SaveError) -> String {
    match error {
        SaveError::Failed(full) if full == FULL => FULL.to_owned(),
        SaveError::Failed(error) => format!("couldn't copy it into browser storage: {error}"),
        error => format!("couldn't copy it into browser storage: {error}"),
    }
}

/// The designs in browser storage, newest first, and whether listing them
/// gave the store of new designs another, see [`tidy`].
#[derive(Debug, Default)]
pub(crate) struct Listing {
    pub(crate) designs: Vec<BrowserDesign>,
    pub(crate) rescued: bool,
}

/// Lists the designs in browser storage, newest first, each with where it
/// stands against `downloads`, reading only the end of each file (see
/// [`vrdp::end`]): not every save, so damage before the newest shows only
/// as it's opened. One this tab holds (as `held` says) isn't looked at past
/// its file; others' sidecars are taken a moment to tell whether they're
/// open elsewhere or hold changes, and deleted if they're empty. What tabs
/// closed in between left is tidied first, see [`tidy`]: changes left of
/// no design go to `store`, the store of new designs, as an entry named by
/// `entry_name`.
pub(crate) async fn list<D: Dir, S: Dir>(
    dir: &D,
    store: Option<&S>,
    entry_name: impl FnMut() -> String,
    downloads: &Downloads,
    held: impl Fn(&str) -> bool,
) -> Listing {
    let Ok(mut names) = dir.names().await else {
        return Listing::default();
    };
    let rescued = tidy(dir, &mut names, store, entry_name).await;
    let mut designs = Vec::new();
    for name in names.iter().filter(|name| is_design_name(name)) {
        designs.push(listed(dir, name, downloads, held(name)).await);
    }
    designs.sort_by_key(|design| std::cmp::Reverse(design.saved));
    Listing { designs, rescued }
}

/// Tidies what tabs closed in between left, of designs nobody holds the
/// sidecar of, returning whether it gave `store` an entry. `names`, the
/// names in `dir`, follows.
///
/// - A design left under a temporary name, by a tab closed as it saved
///   over one (see [`write()`]), is put in place if there's none there,
///   otherwise deleted. Its sidecar is taken first, so one a tab is
///   writing just now, holding it, is left be.
/// - The sidecar of no design, empty, is deleted. One with changes in it,
///   left by a tab closed as it renamed a design, has its newest auto-save
///   made an entry in `store`, named by `entry_name`, as a new design
///   known by the design's name, and is deleted: it's offered back with
///   the designs crashed sessions left. One that can't be read is left as
///   it is, as are all of them without a store.
async fn tidy<D: Dir, S: Dir>(
    dir: &D,
    names: &mut Vec<String>,
    store: Option<&S>,
    mut entry_name: impl FnMut() -> String,
) -> bool {
    let temps: Vec<String> = (names.iter())
        .filter(|name| temp_of(name).is_some())
        .cloned()
        .collect();
    for temp in temps {
        let Some(design) = temp_of(&temp) else {
            continue;
        };
        let Ok(sidecar) = dir.take(&sidecar_name(design), Make::IfMissing).await else {
            continue;
        };
        let gone = match dir.rename(&temp, design).await {
            Ok(()) => {
                if !names.iter().any(|name| name == design) {
                    names.push(design.to_owned());
                }
                true
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                remove_if_free(dir, &temp).await.is_ok()
            }
            Err(_) => false,
        };
        // Deleted unless it holds changes, of the design there.
        let _ = end(dir, design, Held::new(sidecar), Ending::Release).await;
        if gone {
            names.retain(|name| *name != temp);
        }
    }
    let mut rescued = false;
    let orphans: Vec<String> = (names.iter())
        .filter(|name| sidecar_of(name).is_some_and(|design| !names.iter().any(|n| n == design)))
        .cloned()
        .collect();
    for orphan in orphans {
        let Some(design) = sidecar_of(&orphan) else {
            continue;
        };
        let Ok(file) = dir.take(&orphan, Make::No).await else {
            continue;
        };
        let mut sidecar = Held::new(file);
        let gone = match sidecar.is_empty() {
            Ok(true) => true,
            Ok(false) => {
                let made = match store {
                    Some(store) => rescue(&mut sidecar, design, store, &mut entry_name).await,
                    None => false,
                };
                rescued |= made;
                made
            }
            Err(_) => false,
        };
        drop(sidecar);
        if gone && remove_if_free(dir, &orphan).await.is_ok() {
            names.retain(|name| *name != orphan);
        }
    }
    rescued
}

/// Makes the newest auto-save in `sidecar`, of the design `design` gone,
/// an entry in `store` named by `entry_name`, known by the design's name:
/// whether it did. Nothing if it can't be read.
async fn rescue<F: Storage, S: Dir>(
    sidecar: &mut Held<F>,
    design: &str,
    store: &S,
    entry_name: &mut impl FnMut() -> String,
) -> bool {
    let Ok(Some(newest)) = sidecar.read() else {
        return false;
    };
    let saved = AutoSaved {
        // Of the design's file, not the entry's, which has none.
        base: None,
        name: Some(design.to_owned()),
        document: newest.document,
        origin: Origin::Edited,
    };
    for _ in 0..crate::store::ATTEMPTS {
        let name = entry_name();
        let file = match store.take(&name, Make::New).await {
            Ok(file) => file,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::AlreadyExists | io::ErrorKind::ResourceBusy
                ) =>
            {
                continue;
            }
            Err(_) => return false,
        };
        let mut entry = Held::new(file);
        let written = entry.append_saved(&saved).is_ok();
        drop(entry);
        if !written {
            let _ = store.remove(&name).await;
        }
        return written;
    }
    false
}

/// The design `name` as listed, see [`list`]; `held` if this tab holds it.
async fn listed<D: Dir>(dir: &D, name: &str, downloads: &Downloads, held: bool) -> BrowserDesign {
    let mut design = BrowserDesign {
        name: name.to_owned(),
        saved: dir.modified(name).await,
        sum: None,
        thumbnail: None,
        download: DownloadStatus::Never,
        unsaved: false,
        in_use: held,
        damage: None,
    };
    // Being saved just now, or elsewhere, or changed while read: what's
    // known is its name and time.
    if let Ok(reader) = dir.reader(name).await {
        match vrdp::end(&reader) {
            Ok(FileEnd::NotADesign) => design.damage = Some(ListedDamage::Unreadable),
            Ok(FileEnd::Design { newest, previews }) => {
                design.sum = newest;
                design.thumbnail = thumbnail::decode(&previews);
            }
            Err(_) => {}
        }
    }
    if !held {
        match dir.take(&sidecar_name(name), Make::No).await {
            Ok(file) => {
                let sidecar = Held::new(file);
                match sidecar.is_empty() {
                    Ok(true) => {
                        drop(sidecar);
                        let _ = remove_if_free(dir, &sidecar_name(name)).await;
                    }
                    // Changes, or what may yet be got out of them.
                    Ok(false) => design.unsaved = true,
                    Err(_) => {}
                }
            }
            Err(e) if e.kind() == io::ErrorKind::ResourceBusy => design.in_use = true,
            Err(_) => {}
        }
    }
    design.download = downloads.status(name, design.sum, design.unsaved);
    design
}

/// The whole file of the design `name`, as it's saved, to download, and
/// the sum of its newest save, which the download is recorded by.
pub(crate) async fn read_whole<D: Dir>(dir: &D, name: &str) -> Result<(Vec<u8>, u128), String> {
    if !is_design_name(name) {
        return Err(format!("{name} isn't a design in browser storage"));
    }
    let bytes = dir
        .read(name, MAX_MESSAGE_BYTES)
        .await
        .map_err(|e| read_error(name, &e))?;
    let opened = from_bytes_with_report(&bytes).map_err(|e| e.to_string())?;
    Ok((bytes, opened.tail.sum()))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
