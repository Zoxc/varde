//! Fresh file names, for files only one write or editor may have: store
//! entries (`src/native/store.rs`), and the temporary files that design files
//! (`src/native/files/document_file.rs`) and the recent files list
//! (`src/native/recent.rs`) are written to before being renamed over the old one.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A name no other file is likely to have: `{time}-{pid}-{n}` in hex. The
/// time in nanoseconds tells apart instances on machines sharing a
/// directory, the process id instances on one machine, and `n` names made
/// by one instance. [`create`] makes sure.
pub(crate) fn name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{:x}-{n:x}", std::process::id())
}

/// How many taken names [`retry`] skips for a temporary file.
pub(crate) const ATTEMPTS: u32 = 16;

/// Creates the file at `path(name())` with `options`, which must make it
/// new, skipping taken names up to `attempts` times.
pub(crate) fn create(
    options: &OpenOptions,
    attempts: u32,
    path: impl Fn(&str) -> PathBuf,
) -> io::Result<(PathBuf, File)> {
    let taken = |e: &io::Error| e.kind() == io::ErrorKind::AlreadyExists;
    retry(attempts, taken, |name| {
        let path = path(name);
        options.open(&path).map(|file| (path, file))
    })
}

/// Has `make` create something new under a fresh [`name`], trying again
/// with another name while the error it fails with is one `taken` says
/// means the name was taken, up to `attempts` times.
pub(crate) fn retry<T, E>(
    attempts: u32,
    taken: impl Fn(&E) -> bool,
    mut make: impl FnMut(&str) -> Result<T, E>,
) -> Result<T, E> {
    let mut attempt = 0;
    loop {
        match make(&name()) {
            Err(e) if taken(&e) && attempt < attempts => attempt += 1,
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests;
