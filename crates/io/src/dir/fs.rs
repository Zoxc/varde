//! [`Dir`] over `std::fs`, for the tests: a directory on disk, a file
//! taken held through an exclusive OS lock, which two handles in one
//! process conflict over as two tabs' sync access handles do. Reading,
//! deleting and renaming check the lock first, as the browser refuses
//! them on a file someone holds.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::{Dir, Make};
use crate::UnixSeconds;

/// A directory at a path, see the module docs.
#[derive(Debug, Clone)]
pub(crate) struct FsDir(pub(crate) PathBuf);

impl FsDir {
    fn path(&self, name: &str) -> io::Result<PathBuf> {
        if !super::is_plain_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name:?} isn't a file name"),
            ));
        }
        Ok(self.0.join(name))
    }
}

/// Fails with [`io::ErrorKind::ResourceBusy`] if someone holds the file
/// at `path`, as the browser does.
fn free(path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    match file.try_lock_shared() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(busy()),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

fn busy() -> io::Error {
    io::Error::new(io::ErrorKind::ResourceBusy, "the file is held")
}

impl Dir for FsDir {
    type File = File;
    type Reader = File;

    async fn names(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.0)? {
            if let Ok(name) = entry?.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }

    async fn take(&self, name: &str, make: Make) -> io::Result<File> {
        let path = self.path(name)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        match make {
            Make::No => {}
            Make::IfMissing => {
                options.create(true);
            }
            Make::New => {
                options.create_new(true);
            }
        }
        let file = options.open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(TryLockError::WouldBlock) => Err(busy()),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    async fn read(&self, name: &str, max: usize) -> io::Result<Vec<u8>> {
        let path = self.path(name)?;
        free(&path)?;
        let bytes = std::fs::read(path)?;
        if bytes.len() > max {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }
        Ok(bytes)
    }

    async fn reader(&self, name: &str) -> io::Result<File> {
        let path = self.path(name)?;
        free(&path)?;
        File::open(path)
    }

    /// Doesn't wait: the tests make whatever holds a file let go of it
    /// as they need, see `Hooked` in `src/browser/folder/tests.rs`.
    async fn pause(&self, _millis: u32) {}

    async fn modified(&self, name: &str) -> Option<UnixSeconds> {
        let modified = std::fs::metadata(self.path(name).ok()?)
            .ok()?
            .modified()
            .ok()?;
        let seconds = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
        i64::try_from(seconds).ok().map(UnixSeconds)
    }

    async fn remove(&self, name: &str) -> io::Result<()> {
        let path = self.path(name)?;
        free(&path)?;
        std::fs::remove_file(path)
    }

    async fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let (from, to) = (self.path(from)?, self.path(to)?);
        free(&from)?;
        if to.exists() {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        std::fs::rename(from, to)
    }
}
