//! A directory of files the lane keeps, each taken whole for as long as
//! it's held: on the web the Origin Private File System's directories,
//! such as browser storage's `saved` (see `src/browser.rs`), through
//! `src/web/worker/opfs.rs`'s `OpfsDir`.
//!
//! The [`Dir`] trait says what the code over a directory needs, and what
//! it may rely on: a file taken ([`Dir::take`]) is the taker's alone until
//! it's dropped, so nobody else, in this tab or another, can take it,
//! remove it or rename it meanwhile ([`io::ErrorKind::ResourceBusy`]).
//! That's an Origin Private File System sync access handle's exclusive
//! lock, which the browser lets go of as the tab goes. A file is read
//! whole without taking it ([`Dir::read`]), as the welcome screen lists
//! designs another tab may have open, or in parts ([`Dir::reader`]), as
//! listing them reads only their ends. Getting at files is asynchronous
//! there, so the trait is too. Another tab may hold a file a moment, say
//! as it lists the designs: [`take_waiting`] tries again a few times
//! before taking that to mean it holds it for good.
//!
//! The code over it is plain Rust, tested natively over `std::fs`
//! (`FsDir`, in the tests), whose OS locks do what the sync access
//! handles do.

use std::io;

use crate::UnixSeconds;
use crate::autosave::{Ending, Held};
use crate::vrdp::{ReadAt, Storage};

/// A small file read or written whole, such as the downloads and
/// settings TOML files, on top of what the `.vrdp` code needs of a file.
pub(crate) trait WholeFile: Storage {
    /// The whole file, if it's at most `max` bytes long: `None` if longer.
    fn read_whole(&self, max: usize) -> io::Result<Option<Vec<u8>>> {
        let Some(len) = usize::try_from(self.len()?).ok().filter(|&len| len <= max) else {
            return Ok(None);
        };
        let mut bytes = vec![0; len];
        self.read_at(&mut bytes, 0)?;
        Ok(Some(bytes))
    }

    /// Replaces the whole file with `bytes`, made durable.
    fn replace_whole(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write_at(bytes, 0)?;
        // `usize` is at most 64 bits wide, so exact.
        self.truncate(bytes.len() as u64)?;
        self.sync()
    }
}

impl<S: Storage + ?Sized> WholeFile for S {}

/// How [`Dir::take`] finds the file it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Make {
    /// It must be there: [`io::ErrorKind::NotFound`] otherwise.
    No,
    /// Made, empty, if it isn't there.
    IfMissing,
    /// Made, empty: [`io::ErrorKind::AlreadyExists`] if it's there.
    New,
}

/// A directory of files, see the module docs. Names are plain file
/// names, never paths.
pub(crate) trait Dir {
    /// A file taken, held until it's dropped.
    type File: Storage;

    /// A file read in parts without taking it, as it was when found.
    type Reader: ReadAt;

    /// The names of the files in it, in no order.
    async fn names(&self) -> io::Result<Vec<String>>;

    /// Takes the file `name`, found or made as `make` says, to read and
    /// write: [`io::ErrorKind::ResourceBusy`] if someone else holds it.
    async fn take(&self, name: &str, make: Make) -> io::Result<Self::File>;

    /// All of the file `name`, read without taking it:
    /// [`io::ErrorKind::FileTooLarge`] past `max` bytes, and
    /// [`io::ErrorKind::ResourceBusy`] if someone holds it and the
    /// directory can't read it meanwhile, as the browser may not.
    async fn read(&self, name: &str, max: usize) -> io::Result<Vec<u8>>;

    /// The file `name` to read parts of without taking it, as it is now:
    /// [`io::ErrorKind::ResourceBusy`] as for [`Dir::read`], and a read
    /// fails should the file change meanwhile.
    async fn reader(&self, name: &str) -> io::Result<Self::Reader>;

    /// Waits `millis` milliseconds, before trying again to take a file
    /// someone holds a moment, see [`take_waiting`].
    async fn pause(&self, millis: u32);

    /// When the file `name` was last written, if that can be told: not
    /// always while someone holds it.
    async fn modified(&self, name: &str) -> Option<UnixSeconds>;

    /// Deletes the file `name`: [`io::ErrorKind::ResourceBusy`] if someone
    /// holds it, [`io::ErrorKind::NotFound`] if it isn't there.
    async fn remove(&self, name: &str) -> io::Result<()>;

    /// Gives the file `from` the name `to`, keeping what's in it:
    /// [`io::ErrorKind::AlreadyExists`] if there's a file `to`, and
    /// [`io::ErrorKind::ResourceBusy`] if someone holds `from`.
    async fn rename(&self, from: &str, to: &str) -> io::Result<()>;
}

/// Whether `name` is a plain file name, as a directory's names are: no
/// separators, nothing a file system takes for a path.
pub(crate) fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// How many times [`take_waiting`] tries to take a file.
const TRIES: u32 = 5;

/// The pause before [`take_waiting`] tries again the first time, in
/// milliseconds, doubled each time after: 300 ms in all.
const FIRST_PAUSE: u32 = 20;

/// [`Dir::take`], tried again after a short pause while someone holds the
/// file, a few times: another tab listing the designs holds each one's
/// sidecar a moment. [`io::ErrorKind::ResourceBusy`] once they're all
/// refused: someone holds it for longer, say a tab with the design open.
pub(crate) async fn take_waiting<D: Dir>(dir: &D, name: &str, make: Make) -> io::Result<D::File> {
    let mut pause = FIRST_PAUSE;
    for _ in 1..TRIES {
        match dir.take(name, make).await {
            Err(e) if e.kind() == io::ErrorKind::ResourceBusy => {
                dir.pause(pause).await;
                // At most `FIRST_PAUSE << TRIES`: no overflow.
                pause *= 2;
            }
            result => return result,
        }
    }
    dir.take(name, make).await
}

/// Deletes the file `name` in `dir` if it's there and nobody holds it:
/// one gone already, or taken just now by someone else, is fine.
pub(crate) async fn remove_if_free(dir: &impl Dir, name: &str) -> io::Result<()> {
    match dir.remove(name).await {
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ResourceBusy
            ) =>
        {
            Ok(())
        }
        result => result,
    }
}

/// Whether taking a file just named, with [`Make::New`], failed with
/// `kind` because someone else got to the name first: it's there already,
/// another tab holds it, or another tab listed it as empty and deleted it
/// between its making and its taking (the handle of a deleted file can't
/// be taken). Either way it's another name's turn.
pub(crate) fn lost_race(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::AlreadyExists | io::ErrorKind::ResourceBusy | io::ErrorKind::NotFound
    )
}

/// Takes a new, empty file in `dir` under the first of `name`'s names
/// nobody got to first (see [`lost_race`]), trying
/// [`crate::store::ATTEMPTS`] of them: the name and the file, or `None`
/// if all were taken.
pub(crate) async fn take_new<D: Dir>(
    dir: &D,
    mut name: impl FnMut() -> String,
) -> io::Result<Option<(String, D::File)>> {
    for _ in 0..crate::store::ATTEMPTS {
        let name = name();
        match dir.take(&name, Make::New).await {
            Ok(file) => return Ok(Some((name, file))),
            Err(e) if lost_race(e.kind()) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// Lets go of `held`, the file `name` in `dir`, as `ending` says,
/// deleting it unless that keeps what's in it.
pub(crate) async fn end_held<D: Dir>(
    dir: &D,
    name: &str,
    mut held: Held<D::File>,
    ending: Ending,
) -> io::Result<()> {
    let delete = held.end(ending)?;
    // Let go of first: a file that's held can't be deleted.
    drop(held);
    if delete {
        remove_if_free(dir, name).await?;
    }
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod fs;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
