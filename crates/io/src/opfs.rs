//! The store of new designs on the web, in the Origin Private File System.
//!
//! Laid out like the native store (`src/native/store.rs`): each new design is an
//! entry `designs/<id>.vrdp`, a `.vrdp` like any other. OPFS is only
//! reachable through handles, and its synchronous access handles, which
//! read and write at an offset, exist only in workers, so the web's IO
//! worker handles the entries, in `src/web/worker/files.rs`, with the OPFS primitives of
//! `src/web/worker/opfs.rs`. A sync access handle is
//! exclusive: holding one open on an entry for as long as its design is
//! open is the entry's lock, like the OS lock natively, and another tab
//! trying to take it is refused (`NoModificationAllowedError`). The browser
//! lets go of it when the tab or its worker goes, so a closed tab never
//! leaves an entry locked. Auto-saves are appended through the held
//! handle by [`autosave::Held`](crate::autosave::Held), which works on any
//! [`Storage`](crate::vrdp::Storage).
//!
//! A clean close empties and deletes the entry. One found with something
//! in it and not held by anyone is from a tab that was closed or reloaded
//! with its design open, and is offered back on the welcome screen, as
//! natively after a crash. Where saving downloads the design, the entry
//! gets the design as downloaded, marked so, and closing keeps it, since
//! the page isn't told whether the download was kept, going back to it
//! should later changes not be saved: the welcome screen lists those
//! apart, see [`Request::Close`](crate::Request::Close).
//!
//! Designs opened from files of the user's get an entry too, which their
//! auto-saves go to along with the file's name, see `src/pick.rs`. The
//! settings are kept beside the store, in `settings.toml` at the root (see
//! `src/settings.rs`). The recent files list has no place on the web, and
//! files at a path neither.
//!
//! What's here is plain Rust: the checks on paths and names,
//! and on the numbers reads and writes hand JS and get back, which are
//! `f64`s there (see `src/js.rs`). It is tested natively.

use std::io;
use std::path::{Component, Path};

use varde_document::EXTENSION;

use crate::js;

/// The names of the directories, from the root of the Origin Private File
/// System, that `dir` is made of. `None` unless it's all plain names.
pub(crate) fn dir_names(dir: &Path) -> Option<Vec<&str>> {
    let names: Option<Vec<&str>> = dir
        .components()
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    names.filter(|names| !names.is_empty())
}

/// The name of a new entry, from the time in milliseconds since the Unix
/// epoch, a random number in `[0, 1)` and a count: unlikely to be taken,
/// which the lane still checks. Any `f64`s make a valid name.
pub(crate) fn new_name(millis: f64, random: f64, n: u64) -> String {
    // `as` saturates, and turns NaN into 0.
    let millis = millis as u64;
    let random = (random * f64::from(u32::MAX)) as u32;
    format!("{millis:x}-{random:08x}-{n:x}.{EXTENSION}")
}

/// Whether taking the handle of an entry just made failed with `kind`
/// because another tab got to it first: it holds it, or it listed it as
/// empty and deleted it in between, which a listing does to an entry
/// nobody holds (the handle of a deleted entry can't be taken). Either way
/// it's another name's turn.
pub(crate) fn lost_to_another_tab(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::ResourceBusy | io::ErrorKind::NotFound)
}

/// How many bytes a read or write that asked for `wanted` did, as JS gave
/// it: never more than asked for.
pub(crate) fn done(value: f64, wanted: usize) -> io::Result<usize> {
    js::len(value, wanted, |_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the browser did {value} bytes of {wanted}"),
        )
    })
}

#[cfg(test)]
mod tests;
