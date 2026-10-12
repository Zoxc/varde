//! Writing a whole file safely: a new one without replacing anything, or
//! one written next to the file it replaces and renamed over it, so a
//! crash or a failure leaves the old file whole. For the app's own small
//! files (`src/native/config.rs`) and exports (`src/native/export.rs`);
//! a design's file replaces itself through [`replace_through`] too, with
//! its own locking and test hooks, see `DocumentFile::replace`.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
/// file next to it, see [`replace_by`], then renamed over it.
pub(crate) fn replace(path: &Path, mode: Mode, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let permissions = match mode {
        Mode::Keep => std::fs::metadata(path).ok().map(|m| m.permissions()),
        Mode::Private => private(&mut options),
    };
    // A name that's taken is someone else's, left alone.
    let taken = |e: &io::Error| e.kind() == io::ErrorKind::AlreadyExists;
    replace_by(path, taken, |temp| {
        let file = options.open(temp)?;
        // The umask can narrow the mode it was created at; set it outright.
        let written = permissions
            .clone()
            .map_or(Ok(()), |permissions| file.set_permissions(permissions))
            .and_then(|()| finish(file, bytes));
        if written.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        written
    })
}

/// Replaces the file at `path`, or makes one where there's none, with
/// what `make` writes to a new file next to it: named by [`temp_path`]
/// with this write's own [`unique::name`], so another instance writing
/// at the same time can't write into it, trying another name while
/// `make` fails as `taken` says a taken name does. `make` creates the
/// file new and removes it if it fails to finish it. The file is then
/// renamed over `path` (removed if that fails), and the rename made
/// durable as far as it can be.
pub(crate) fn replace_by<T, E: From<io::Error>>(
    path: &Path,
    taken: impl Fn(&E) -> bool,
    make: impl FnMut(&Path) -> Result<T, E>,
) -> Result<T, E> {
    let temp = |name: &str| temp_path(path, name);
    replace_through(path, taken, temp, make, sync_parent)
}

/// [`replace_by`], naming the temporary file `temp(name)` and making the
/// rename durable with `sync`, which tests replace.
pub(crate) fn replace_through<T, E: From<io::Error>>(
    path: &Path,
    taken: impl Fn(&E) -> bool,
    mut temp: impl FnMut(&str) -> io::Result<PathBuf>,
    mut make: impl FnMut(&Path) -> Result<T, E>,
    sync: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<T, E> {
    let (temp, made) = unique::retry(unique::ATTEMPTS, taken, |name| {
        let temp = temp(name)?;
        make(&temp).map(|made| (temp, made))
    })?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    // Best effort: the file is replaced, its contents synced. Failing
    // here only risks a crash bringing the old one back, and an error
    // would have the caller go on as if the old one were still there.
    let _ = sync(path);
    Ok(made)
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

/// A path next to `path` that nothing is likely to be at, for writing a
/// file that then replaces it: `.{file name}.{name}.tmp`, with a
/// [`unique::name`].
pub(crate) fn temp_path(path: &Path, name: &str) -> io::Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut temp = std::ffi::OsString::from(".");
    temp.push(file_name);
    temp.push(format!(".{name}.tmp"));
    Ok(path.with_file_name(temp))
}

/// Makes a newly created or renamed file's directory entry durable.
pub(crate) fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
