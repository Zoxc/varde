//! Writing an exported file natively, see [`Request::Export`]: a new file
//! at a path, or one written next to the file it replaces and renamed over
//! it, so a crash or a failure leaves the old file whole.
//!
//! [`Request::Export`]: crate::Request::Export

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use super::files::{display_name, resolved, sync_parent, temp_path};
use super::unique;
use crate::three_mf::{self, Body};

/// Writes `bodies` as a 3MF package titled `title` to `path`, replacing a
/// file there only if `overwrite`, which the user agreed to. Written to
/// where a symbolic link points, so the link stays.
pub(crate) fn export(
    path: &Path,
    overwrite: bool,
    title: &str,
    bodies: &[Body],
) -> Result<(), String> {
    let bytes = three_mf::package(title, bodies).map_err(|e| e.to_string())?;
    let real = resolved(path);
    let written = if overwrite {
        replace(&real, &bytes)
    } else {
        create(&real, &bytes)
    };
    written.map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => format!("{} already exists", display_name(path)),
        _ => format!("couldn't write {}: {e}", display_name(path)),
    })
}

/// Creates the file at `path` holding `bytes`. Fails if there's one there.
/// A file it fails to finish is removed, since it created it.
fn create(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = finish(file, bytes).and_then(|()| sync_parent(path));
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Writes `bytes` over the file at `path`, or where there's none: next to
/// it, then renamed over it, with its permissions.
fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let permissions = std::fs::metadata(path).ok().map(|m| m.permissions());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    // A name that's taken is someone else's, left alone.
    let taken = |e: &io::Error| e.kind() == io::ErrorKind::AlreadyExists;
    let (temp, file) = unique::retry(unique::ATTEMPTS, taken, |name| {
        let temp = temp_path(path, name)?;
        options.open(&temp).map(|file| (temp, file))
    })?;
    let written = permissions
        .map_or(Ok(()), |permissions| file.set_permissions(permissions))
        .and_then(|()| finish(file, bytes))
        .and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return written;
    }
    // Best effort, as for a design's file: the file is written.
    let _ = sync_parent(path);
    Ok(())
}

/// Writes `bytes` to `file` and makes them durable. Closed as it returns,
/// before any rename, which Windows needs.
fn finish(mut file: File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests;
