//! A design's own `.vrdp` file, natively: the format
//! ([`vrdp`]) on a [`File`] at a path, opened and locked for
//! each operation. Readers take a shared lock and writers an exclusive one,
//! each waiting at most a moment for it, except that reading a file's
//! preview never waits. A new file is created without
//! replacing anything, or written next to the one it replaces and renamed
//! over it, and made durable along with its directory entry.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use varde_document::Document;

use crate::native::unique;
#[cfg(test)]
use crate::vrdp::Report;
use crate::vrdp::{self, Error, Known, Opened, Preview, Result, Tail, WithFound};

/// An open `.vrdp` file that [`Document`]s can be saved to.
///
/// The file is only opened and locked for the duration of each operation.
#[derive(Debug)]
pub(crate) struct DocumentFile {
    path: PathBuf,
    /// The file as last read or written, see [`vrdp::save`]: its tail, how
    /// it was found after it, and a failed save.
    known: Known,
}

/// A step on a path that tests step in at, see [`Hooks`].
type PathHook<'a> = &'a mut dyn FnMut(&Path) -> io::Result<()>;

/// Where tests step into creating and replacing files: [`Hooks::default`]
/// otherwise.
#[derive(Default)]
struct Hooks<'a> {
    /// Called right after a file is created, to look at it or to fail
    /// there.
    created: Option<PathHook<'a>>,
    /// The temporary paths to write a replacing file to, instead of
    /// [`temp_path`]'s.
    temps: Option<&'a mut dyn FnMut() -> io::Result<PathBuf>>,
    /// Makes the rename that replaces a file durable, instead of
    /// [`sync_parent`], to fail there.
    sync_dir: Option<PathHook<'a>>,
}

impl DocumentFile {
    /// Creates a new file containing `document`, followed by `previews`,
    /// see [`vrdp::to_bytes`]. Fails if `path` exists.
    pub(crate) fn create(
        path: impl Into<PathBuf>,
        document: &Document,
        previews: &[Preview],
    ) -> Result<Self> {
        Self::create_with(path.into(), document, previews, None, &mut Hooks::default())
    }

    /// [`create`](DocumentFile::create), with `permissions` from the start
    /// if given. A file it fails to finish is removed, since it created it.
    fn create_with(
        path: PathBuf,
        document: &Document,
        previews: &[Preview],
        permissions: Option<&std::fs::Permissions>,
        hooks: &mut Hooks,
    ) -> Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        // Set as it's created, so it's never readable by more than it's
        // meant to be. The umask may narrow it; `set_permissions` after
        // doesn't, but nothing is in the file yet. Elsewhere permissions
        // are only a read-only flag, which would keep the file from
        // replacing another, or from being removed on failure.
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(permissions.mode() & 0o7777);
        }
        let file = options.open(&path)?;
        let result = Self::write_new(file, path.clone(), document, previews, permissions, hooks);
        if result.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        result
    }

    /// Writes `document` to `file`, just created at `path`, see
    /// [`create_with`](DocumentFile::create_with).
    fn write_new(
        mut file: File,
        path: PathBuf,
        document: &Document,
        previews: &[Preview],
        permissions: Option<&std::fs::Permissions>,
        hooks: &mut Hooks,
    ) -> Result<Self> {
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            file.set_permissions(permissions.clone())?;
        }
        #[cfg(not(unix))]
        let _ = permissions;
        if let Some(created) = &mut hooks.created {
            created(&path)?;
        }
        lock(&file, Lock::Exclusive)?;

        let known = vrdp::write_new(&mut file, document, previews)?;
        file.sync_all()?;
        sync_parent(&path)?;
        Ok(Self { path, known })
    }

    /// Opens an existing file and reads its newest document, with how
    /// reading found the file, see
    /// [`open_with_found`](DocumentFile::open_with_found).
    #[cfg(test)]
    pub(crate) fn open(path: impl Into<PathBuf>) -> Result<(Self, Document, Report)> {
        let (file, read) = Self::open_with_found(path)?;
        Ok((file, read.opened.payload, read.opened.report))
    }

    /// Opens an existing file and reads its newest document, with how
    /// reading found the file and the newest save a search found past
    /// damage, if it's another than the one opened, see
    /// [`vrdp::from_bytes_with_found`]: [`DocumentFile::open_found`] goes
    /// over to it. One read as [`vrdp::Outcome::Damaged`] is never saved
    /// to, see [`DocumentFile::save`]: that's how the caller knows to save
    /// it as another file instead.
    pub(crate) fn open_with_found(path: impl Into<PathBuf>) -> Result<(Self, WithFound)> {
        let path = path.into();
        let file = File::open(&path)?;
        lock(&file, Lock::Shared)?;
        let read = vrdp::read_with_found(&file)?;
        let file = Self {
            path,
            known: read.opened.known(),
        };
        Ok((file, read))
    }

    /// Goes over to `found`, the save a search found past damage that
    /// [`DocumentFile::open_with_found`] read, returning what it holds:
    /// auto-saves are based on it from now on, and saves still refused.
    pub(crate) fn open_found(&mut self, found: Opened<Document>) -> Document {
        self.known = found.known();
        found.payload
    }

    /// Whether an auto-save based on the save at `base` was based on a
    /// newer save than the one this last read or wrote, which couldn't be
    /// read, see [`vrdp::based_past`]. Not if the file can't be read.
    pub(crate) fn based_past(&self, base: Tail) -> bool {
        let read = || -> Result<bool> {
            let file = File::open(&self.path)?;
            lock(&file, Lock::Shared)?;
            Ok(vrdp::based_past(&file, self.tail(), base)?)
        };
        read().unwrap_or(false)
    }

    /// Writes `document` and `previews` as a new file at `path`, like
    /// [`create`], but
    /// replacing whatever file is there already. The new file is written
    /// next to it under a temporary name and renamed over it, so a crash
    /// leaves either the old file or the new one, never a mix. It takes
    /// the replaced file's permissions.
    ///
    /// [`create`]: DocumentFile::create
    pub(crate) fn replace(
        path: impl Into<PathBuf>,
        document: &Document,
        previews: &[Preview],
    ) -> Result<Self> {
        Self::replace_with(path.into(), document, previews, Hooks::default())
    }

    /// [`replace`](DocumentFile::replace), with `hooks` for tests.
    fn replace_with(
        path: PathBuf,
        document: &Document,
        previews: &[Preview],
        mut hooks: Hooks,
    ) -> Result<Self> {
        let permissions = std::fs::metadata(&path).ok().map(|m| m.permissions());
        let mut temps = hooks.temps.take();
        // A name that's taken is someone else's: another instance saving
        // next to it, or one that crashed. Left alone.
        let taken =
            |e: &Error| matches!(e, Error::Io(e) if e.kind() == io::ErrorKind::AlreadyExists);
        let (temp, mut file) = unique::retry(unique::ATTEMPTS, taken, |name| {
            let temp = match &mut temps {
                Some(temps) => temps()?,
                None => temp_path(&path, name)?,
            };
            // A temporary file it fails to finish is removed.
            Self::create_with(
                temp.clone(),
                document,
                previews,
                permissions.as_ref(),
                &mut hooks,
            )
            .map(|file| (temp, file))
        })?;
        let result = std::fs::rename(&temp, &path);
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        // Best effort: the file is replaced, its contents synced. Failing
        // here only risks a crash bringing the old one back, and an error
        // would have the caller go on as if the old one were still there,
        // to find its own new file a conflict.
        let _ = match &mut hooks.sync_dir {
            Some(sync_dir) => sync_dir(&path),
            None => sync_parent(&path),
        };
        file.path = path;
        Ok(file)
    }

    /// The path it was opened or written at.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The saved version the file held when `self` last read or wrote it.
    pub(crate) fn tail(&self) -> Tail {
        self.known.tail()
    }

    /// Appends `document` as a new version, followed by `previews`, which
    /// replace the save before's, see [`vrdp::save`].
    ///
    /// Returns [`Error::Conflict`] if another writer has changed the file since
    /// it was opened or last saved by `self`, and [`Error::Damaged`] if it
    /// was damaged since. A file opened as [`vrdp::Outcome::Damaged`] is
    /// refused, [`Error::OpenedDamaged`], with nothing written: it can only
    /// be saved as another file, which may replace it.
    pub(crate) fn save(&mut self, document: &Document, previews: &[Preview]) -> Result<()> {
        let mut file = OpenOptions::new().read(true).write(true).open(&self.path)?;
        lock(&file, Lock::Exclusive)?;
        vrdp::save(&mut file, &mut self.known, document, previews)?;
        Ok(())
    }

    /// The first preview `supported` accepts of the design file at `path`,
    /// see [`vrdp::read_preview`], which reads no record's payload. It
    /// takes the shared lock without waiting, as it's for listing files
    /// on the one IO lane: a lock it can't get, as while another program
    /// saves where locks are mandatory, is no preview this time, as is any
    /// other failure.
    pub(crate) fn read_preview(
        path: &Path,
        supported: impl Fn(&Preview) -> bool,
    ) -> Option<Preview> {
        let file = File::open(path).ok()?;
        file.try_lock_shared().ok()?;
        vrdp::read_preview(&file, supported)
    }
}

/// How long opening or saving waits for a lock on the file to be let go
/// of: long enough for another program's save to finish, short enough that
/// a file kept locked, like the lock file of a design open for editing or a
/// file another program holds on to, is refused rather than waited on for
/// good, which would stall whatever is opening or saving it.
const LOCK_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
enum Lock {
    /// For reading.
    Shared,
    /// For writing.
    Exclusive,
}

/// Locks `file`, waiting at most [`LOCK_WAIT`]. A lock still held then is
/// an [`io::ErrorKind::WouldBlock`] error.
fn lock(file: &File, lock: Lock) -> Result<()> {
    let start = Instant::now();
    loop {
        let result = match lock {
            Lock::Shared => file.try_lock_shared(),
            Lock::Exclusive => file.try_lock(),
        };
        match result {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => {
                if start.elapsed() >= LOCK_WAIT {
                    return Err(Error::Io(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "file is locked by another program",
                    )));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
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

#[cfg(test)]
mod tests;
