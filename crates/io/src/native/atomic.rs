//! Writing a whole file safely: a new one without replacing anything, or
//! one written next to the file it replaces and renamed over it, so a
//! crash or a failure leaves the old file whole. For the app's own small
//! files (`src/native/config.rs`) and exports (`src/native/export.rs`);
//! a design's file does the same with its own locking and test hooks, see
//! `DocumentFile::replace`.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use super::files::{sync_parent, temp_path};
use super::unique;

/// The permissions a replacing file gets.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Mode {
    /// The replaced file's, if there is one.
    Keep,
    /// Readable by the user only: on Unix the temporary file is created at
    /// mode 0600, so it never is anything else.
    Private,
}

/// Creates the file at `path` holding `bytes`. Fails if there's one there.
/// A file it fails to finish is removed, since it created it.
pub(crate) fn create(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = finish(file, bytes).and_then(|()| sync_parent(path));
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Writes `bytes` over the file at `path`, or where there's none: to a new
/// file next to it, named by [`temp_path`] with this write's own
/// [`unique::name`] so another instance writing at the same time can't
/// write into it, then renamed over it.
pub(crate) fn replace(path: &Path, mode: Mode, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let permissions = match mode {
        Mode::Keep => std::fs::metadata(path).ok().map(|m| m.permissions()),
        Mode::Private => private(&mut options),
    };
    // A name that's taken is someone else's, left alone.
    let taken = |e: &io::Error| e.kind() == io::ErrorKind::AlreadyExists;
    let (temp, file) = unique::retry(unique::ATTEMPTS, taken, |name| {
        let temp = temp_path(path, name)?;
        options.open(&temp).map(|file| (temp, file))
    })?;
    // The umask can narrow the mode it was created at; set it outright.
    let written = permissions
        .map_or(Ok(()), |permissions| file.set_permissions(permissions))
        .and_then(|()| finish(file, bytes))
        .and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return written;
    }
    // Best effort, as for a design's file: the file is written, and
    // failing here only risks a crash bringing the old one back.
    let _ = sync_parent(path);
    Ok(())
}

/// Has `options` create a file at mode 0600, returning the permissions to
/// set on it after. Nothing elsewhere than Unix.
fn private(options: &mut OpenOptions) -> Option<std::fs::Permissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        Some(std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = options;
        None
    }
}

/// Writes `bytes` to `file` and makes them durable. Closed as it returns,
/// before any rename, which Windows needs.
fn finish(mut file: File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}
