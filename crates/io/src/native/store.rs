//! The store's entries at a path, see `src/store.rs`.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use varde_document::EXTENSION;

use super::sidecar::{self, LockFile};
use super::unique;
use crate::UnixSeconds;
use crate::autosave::Ending;
use crate::store::{ATTEMPTS, Recovered, listed_entry, newest_first};

/// Creates and locks a new, empty entry in `dir`, making `dir` if needed.
pub(crate) fn create(dir: &Path) -> io::Result<LockFile> {
    create_with(dir, File::try_lock)
}

/// [`create`], locking each new entry with `lock`, which tests use to make
/// locking fail.
pub(crate) fn create_with(
    dir: &Path,
    mut lock: impl FnMut(&File) -> Result<(), TryLockError>,
) -> io::Result<LockFile> {
    std::fs::create_dir_all(dir)?;
    for _ in 0..ATTEMPTS {
        let (path, file) = unique::create(&options(true), ATTEMPTS, |name| {
            dir.join(format!("{name}.{EXTENSION}"))
        })?;
        match lock(&file) {
            Ok(()) => {}
            // Someone listing entries found it empty and locked it right
            // now: it's in use, and gets a new name.
            Err(TryLockError::WouldBlock) => continue,
            // The file system can't lock it, and won't the next one either:
            // take back the entry nobody can use.
            Err(TryLockError::Error(e)) => {
                // Windows can't delete it while it's open.
                #[cfg(windows)]
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(e);
            }
        }
        // Someone listing entries may have found it empty and unlocked in
        // between, and deleted it: then it's a new name.
        if sidecar::still_at(&file, &path)? {
            return Ok(LockFile::new(path, file));
        }
    }
    Err(io::Error::other(
        "couldn't find a free name for the new design",
    ))
}

/// Opens and locks the entry at `path` in `dir`, left behind by a session
/// that crashed, to go on editing it. Refused for anything but an entry in
/// `dir`: it's taken for a new design's from then on, and deleted with it.
pub(crate) fn open(dir: &Path, path: &Path) -> Result<LockFile, String> {
    listed_entry(dir, path)?;
    sidecar::lock_at(path, &options(false), |_| {})
        .map(LockFile::left_behind)
        .map_err(|error| error.to_string())
}

/// The new designs in `dir` left behind by sessions that crashed, newest
/// first: entries nobody holds with a design in them. Empty ones, of
/// sessions that crashed before their design was first auto-saved, are
/// deleted. Damaged ones are left alone, and not listed: they can't be
/// opened.
pub(crate) fn list(dir: &Path) -> Vec<Recovered> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Recovered> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| recovered(dir, &entry.path()))
        .collect();
    newest_first(&mut found);
    found
}

/// The entry at `path` in `dir`, if it's one left behind with a design in
/// it.
fn recovered(dir: &Path, path: &Path) -> Option<Recovered> {
    // Not an entry, or held by an editor, in this process or another: not
    // left behind.
    let mut entry = open(dir, path).ok()?;
    let modified = modified(&entry);
    match entry.read() {
        // Unlocked again as `entry` goes, and kept.
        Ok(Some(auto_saved)) => Some(Recovered::new(path.to_owned(), modified, auto_saved)),
        Ok(None) => {
            let _ = entry.end(Ending::Close);
            None
        }
        Err(_) => None,
    }
}

/// When `entry` was last written.
fn modified(entry: &LockFile) -> Option<UnixSeconds> {
    let modified = entry.as_file().metadata().ok()?.modified().ok()?;
    let seconds = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(seconds).ok().map(UnixSeconds)
}

/// Deletes the entry at `path` in `dir`, left behind by a session that
/// crashed: the user doesn't want it. Refused for anything but an entry in
/// `dir`, or one that's open. Changes on top of a design downloaded on the
/// web go back to it instead, see [`Ending::Discard`].
pub(crate) fn discard(dir: &Path, path: &Path) -> Result<(), String> {
    open(dir, path)?
        .end(Ending::Discard)
        .map_err(|e| format!("couldn't delete the recovered design: {e}"))
}

/// How to open an entry: for reading and writing, creating it if `new`,
/// in which case it mustn't exist yet.
fn options(new: bool) -> OpenOptions {
    let mut options = sidecar::lockable();
    options.create_new(new);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

#[cfg(test)]
mod tests;
