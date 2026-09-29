//! The sidecar: the lock file that makes sure one editor edits a document.
//!
//! `dir/design.vrdp` has the hidden sidecar `dir/.design.vrdp.autosave`
//! (the dot hides it on Unix; on Windows it gets the hidden attribute).
//! Opening a document for editing creates it if needed and takes an
//! exclusive, non-blocking OS lock on it (`flock` / `LockFileEx`), held for
//! as long as the document is open. If another editor holds the lock, or the
//! sidecar can't be created or locked, the document opens read-only. OS
//! locks are released when a process dies, so a crash never leaves a stale
//! lock; a sidecar left behind unlocked only means nobody has the document
//! open.
//!
//! The sidecar is also where auto-saves go, as a `.vrdp` written through
//! the lock's own handle (see [`HeldFile`](crate::vrdp::HeldFile)), holding
//! [`AutoSaved`] records, see `src/autosave.rs` for what's kept of them. On
//! Unix it's created with the document's permission bits (as far as the
//! umask allows), plus the owner's read and write: it holds the same
//! content.
//!
//! Store entries of new designs (`src/store.rs`) are locked and written the
//! same way; there the entry is the lock file itself. Both are a
//! [`LockFile`].
//!
//! Deleting on close races with an editor opening the document just then:
//! it can open the sidecar before the delete and lock it after the unlock,
//! ending up with a lock on a file no longer at the path. So after locking,
//! [`lock`] checks that the path still refers to the locked file and starts
//! over if not. On Windows the sidecar is opened without share-delete
//! instead, so nobody can delete it while it's open, and close unlocks
//! before deleting; a delete that loses to a new opener fails harmlessly.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

use varde_document::Snapshot;

use crate::ReadOnly;
use crate::autosave::{AutoSaved, Ending, Held, Origin};
use crate::vrdp::{Error as FileError, Tail};

/// How often [`lock`] starts over when the sidecar it locked was deleted.
/// Each retry means another editor closed the document meanwhile, so
/// running out takes one opening and closing it in a loop.
const ATTEMPTS: u32 = 8;

/// The sidecar of the document at `path`, next to it: `.{name}.autosave`.
/// `None` if `path` has no file name.
pub(crate) fn sidecar_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?;
    let mut sidecar = std::ffi::OsString::from(".");
    sidecar.push(name);
    sidecar.push(".autosave");
    Some(path.with_file_name(sidecar))
}

/// Whether `path` is named as some document's sidecar, `.{name}.autosave`,
/// the suffix in any case, as a case-insensitive file system sees it.
pub(crate) fn is_sidecar_name(path: &Path) -> bool {
    const SUFFIX: &[u8] = b".autosave";
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.as_encoded_bytes();
    name.len() > 1 + SUFFIX.len()
        && name.starts_with(b".")
        && name[name.len() - SUFFIX.len()..].eq_ignore_ascii_case(SUFFIX)
}

impl AutoSaved {
    /// Whether this was based on the design whose file ends at `tail`.
    /// Anything else means the design changed since: saved by someone
    /// else, rewritten, or saved by the session that auto-saved this but
    /// kept it, e.g. crashing before emptying the sidecar.
    pub(crate) fn based_on(&self, tail: Tail) -> bool {
        self.base == Some(tail)
    }
}

/// A locked auto-save file at a path, natively: a design's sidecar or a
/// store entry, which differ in how they're let go of (see [`Ending`]).
/// Deleted only while still at its path. Dropping it unlocks it without
/// deleting it, as a crash would; [`LockFile::end`] lets go of it as an
/// [`Ending`] says, e.g. the clean close.
#[derive(Debug)]
pub(crate) struct LockFile {
    path: PathBuf,
    held: Held<File>,
}

/// Creates the sidecar of the document at `document` if needed and locks
/// it, or says why the document is read-only. The document needn't exist
/// yet, as when saving as a new file.
pub(crate) fn lock(document: &Path) -> Result<LockFile, ReadOnly> {
    let path = sidecar_path(document)
        .ok_or_else(|| ReadOnly::NoLock("the design's path has no file name".to_owned()))?;
    let options = options(document);
    lock_at(&path, &options, |_| {})
}

/// [`lock`] at `path`, creating the sidecar with `options`, calling
/// `locked` with the attempt number right after each lock is taken, which
/// tests use to race a closing editor.
pub(crate) fn lock_at(
    path: &Path,
    options: &OpenOptions,
    mut locked: impl FnMut(u32),
) -> Result<LockFile, ReadOnly> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    for attempt in 0..ATTEMPTS {
        let file = options
            .open(path)
            .map_err(|e| ReadOnly::NoLock(format!("couldn't create the lock file {name}: {e}")))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(ReadOnly::InUse),
            Err(TryLockError::Error(e)) => {
                return Err(ReadOnly::NoLock(format!(
                    "couldn't lock the lock file {name}: {e}"
                )));
            }
        }
        locked(attempt);
        match still_at(&file, path) {
            Ok(true) => return Ok(LockFile::new(path.to_owned(), file)),
            // Deleted by an editor closing the document: start over. The
            // failed attempt's lock goes with `file`.
            Ok(false) => {}
            Err(e) => {
                return Err(ReadOnly::NoLock(format!(
                    "couldn't check the lock file {name}: {e}"
                )));
            }
        }
    }
    Err(ReadOnly::NoLock(format!(
        "the lock file {name} kept being replaced"
    )))
}

/// How to open the sidecar of the document at `document`: for reading and
/// writing, creating it if needed.
fn options(document: &Path) -> OpenOptions {
    let mut options = lockable();
    options.create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        // Best effort: without the document, the default is what a new
        // document gets too. The owner can always read and write it, even
        // of a read-only document: otherwise the next editor couldn't lock
        // it, nor recover what a crash left in it.
        if let Ok(metadata) = std::fs::metadata(document) {
            options.mode(metadata.permissions().mode() & 0o777 | 0o600);
        }
    }
    #[cfg(not(unix))]
    let _ = document;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        // Only applies when the file is created, which is when it matters.
        options.attributes(FILE_ATTRIBUTE_HIDDEN);
    }
    options
}

/// How to open a lock file, a sidecar or store entry: for reading and
/// writing, and on Windows without share-delete, see the module docs.
pub(crate) fn lockable() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    options
}

/// Whether `path` still refers to `file`, rather than to nothing or to a
/// new file created after `file` was deleted.
#[cfg(unix)]
pub(crate) fn still_at(file: &File, path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let locked = file.metadata()?;
    match std::fs::metadata(path) {
        Ok(current) => Ok(current.dev() == locked.dev() && current.ino() == locked.ino()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Nobody can delete the file while it's open without share-delete, see
/// [`options`], so it's always still there.
#[cfg(not(unix))]
pub(crate) fn still_at(_file: &File, _path: &Path) -> io::Result<bool> {
    Ok(true)
}

impl LockFile {
    /// `file`, open at `path` and locked.
    pub(crate) fn new(path: PathBuf, file: File) -> Self {
        Self {
            path,
            held: Held::new(file),
        }
    }

    /// A store entry left behind, opened again, see [`Held::left_behind`].
    pub(crate) fn left_behind(self) -> Self {
        Self {
            held: self.held.left_behind(),
            ..self
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn as_file(&self) -> &File {
        self.held.storage()
    }

    /// The newest auto-save, if there is one.
    pub(crate) fn read(&mut self) -> Result<Option<AutoSaved>, FileError> {
        self.held.read()
    }

    /// Auto-saves `document`, based on the design's file at `base`, if
    /// it has one, holding it as `origin` says. The sidecar sits next to
    /// the design, so it's not named.
    pub(crate) fn append(
        &mut self,
        base: Option<Tail>,
        document: &Snapshot,
        origin: Origin,
    ) -> Result<(), FileError> {
        self.held.append(base, None, document, origin)
    }

    /// Empties it, once what's in it is saved or not wanted.
    pub(crate) fn clear(&mut self) -> io::Result<()> {
        self.held.clear()
    }

    /// Lets go of it as `ending` says: deletes it unless that keeps what's
    /// in it, then unlocks it.
    pub(crate) fn end(mut self, ending: Ending) -> io::Result<()> {
        let delete = self.held.end(ending)?;
        self.finish(delete)
    }

    /// Deletes it if `delete`, then unlocks it. Only the locked file is
    /// deleted: another file put at its path since, as by renaming a
    /// design over it, is left alone.
    fn finish(self, delete: bool) -> io::Result<()> {
        // `held` holds the lock until it goes out of scope.
        let LockFile { path, held } = self;
        let delete = delete && still_at(held.storage(), &path)?;
        // Deleted while still locked, so an editor that locks it after the
        // unlock sees it's gone and starts over, see [`lock`]. Windows
        // can't delete it while it's open without share-delete, so there
        // it unlocks first.
        #[cfg(windows)]
        drop(held);
        let result = if delete {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                // Another editor opened it in between and has it now
                // (`ERROR_SHARING_VIOLATION`, or access denied while it's
                // being deleted).
                #[cfg(windows)]
                Err(e)
                    if e.raw_os_error() == Some(32)
                        || e.kind() == io::ErrorKind::PermissionDenied =>
                {
                    Ok(())
                }
                result => result,
            }
        } else {
            Ok(())
        };
        // Elsewhere `held` unlocks here, as it goes out of scope.
        result
    }
}

#[cfg(test)]
mod tests;
