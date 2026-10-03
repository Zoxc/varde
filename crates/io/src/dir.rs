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
use crate::vrdp::{ReadAt, Storage};

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

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod fs;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
