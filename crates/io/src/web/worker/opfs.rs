//! What's done with the Origin Private File System's handles, see
//! `src/opfs.rs`: its directories and entries, and the sync access handle
//! that holds an entry.

use std::io;
use std::path::Path;

use js_sys::{AsyncIterator, IteratorNext};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    File, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemReadWriteOptions, FileSystemSyncAccessHandle,
    WorkerGlobalScope,
};

use crate::UnixSeconds;
use crate::js::{number, offset, seconds, size};
use crate::opfs::{dir_names, done};
use crate::vrdp::{ReadAt, Storage};
use crate::web::js::{call, js_error};

/// The root of the Origin Private File System.
async fn root() -> io::Result<FileSystemDirectoryHandle> {
    let scope: WorkerGlobalScope = js_sys::global().unchecked_into();
    Ok(call(scope.navigator().storage().get_directory())
        .await?
        .unchecked_into())
}

/// The directory at `designs`, from the root of the Origin Private File
/// System, made if needed.
pub(crate) async fn dir(designs: &Path) -> io::Result<FileSystemDirectoryHandle> {
    let names = dir_names(designs).ok_or_else(|| not_a_place(designs))?;
    let mut dir = root().await?;
    let options = FileSystemGetDirectoryOptions::new();
    options.set_create(true);
    for name in names {
        dir = call(dir.get_directory_handle_with_options(name, &options))
            .await?
            .unchecked_into();
    }
    Ok(dir)
}

/// The file at `path`, from the root of the Origin Private File System,
/// made if `create`, along with the directories it's in.
pub(crate) async fn file_at(path: &Path, create: bool) -> io::Result<FileSystemFileHandle> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| not_a_place(path))?;
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => dir(parent).await?,
        _ => root().await?,
    };
    file(&dir, name, create).await
}

fn not_a_place(path: &Path) -> io::Error {
    io::Error::other(format!(
        "{} isn't a place in the Origin Private File System",
        path.display()
    ))
}

/// The names in `dir`.
pub(crate) async fn names(dir: &FileSystemDirectoryHandle) -> io::Result<Vec<String>> {
    let keys: AsyncIterator = dir.keys();
    let mut names = Vec::new();
    loop {
        let next: IteratorNext = call(keys.next().map_err(js_error)?).await?.unchecked_into();
        if next.done() {
            return Ok(names);
        }
        if let Some(name) = next.value().as_string() {
            names.push(name);
        }
    }
}

/// The file `name` in `dir`, made if `create`.
pub(crate) async fn file(
    dir: &FileSystemDirectoryHandle,
    name: &str,
    create: bool,
) -> io::Result<FileSystemFileHandle> {
    let options = FileSystemGetFileOptions::new();
    options.set_create(create);
    Ok(call(dir.get_file_handle_with_options(name, &options))
        .await?
        .unchecked_into())
}

/// When `file` was last written. Not while a sync access handle on it is
/// open.
pub(crate) async fn modified(file: &FileSystemFileHandle) -> Option<UnixSeconds> {
    let file: File = call(file.get_file()).await.ok()?.unchecked_into();
    seconds(file.last_modified()).map(UnixSeconds)
}

/// A sync access handle, held open: the entry's lock. Dropping it closes
/// it, letting go.
#[derive(Debug)]
pub(crate) struct Handle(FileSystemSyncAccessHandle);

impl Handle {
    /// Takes `file`'s handle, failing with [`io::ErrorKind::ResourceBusy`]
    /// if another holds it.
    pub(crate) async fn take(file: &FileSystemFileHandle) -> io::Result<Self> {
        Ok(Self(
            call(file.create_sync_access_handle())
                .await?
                .unchecked_into(),
        ))
    }
}

impl ReadAt for Handle {
    fn len(&self) -> io::Result<u64> {
        size(self.0.get_size().map_err(js_error)?)
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        all_at(
            buf.len(),
            at,
            io::ErrorKind::UnexpectedEof,
            |options, read| {
                self.0
                    .read_with_u8_array_and_options(&mut buf[read..], options)
            },
        )
    }
}

impl Storage for Handle {
    fn write_at(&mut self, buf: &[u8], at: u64) -> io::Result<()> {
        all_at(
            buf.len(),
            at,
            io::ErrorKind::WriteZero,
            |options, written| {
                self.0
                    .write_with_u8_array_and_options(&buf[written..], options)
            },
        )
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.0.truncate_with_f64(number(len)?).map_err(js_error)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.0.flush().map_err(js_error)
    }
}

/// Reads or writes all `len` bytes from `at`, however few each call does:
/// `part(options, done)` does what's left of them from `done` on, at the
/// offset `options` has, returning how many bytes it did as JS gave it. One
/// that does none fails with `stalled`.
fn all_at(
    len: usize,
    at: u64,
    stalled: io::ErrorKind,
    mut part: impl FnMut(&FileSystemReadWriteOptions, usize) -> Result<f64, JsValue>,
) -> io::Result<()> {
    let mut so_far = 0;
    while so_far < len {
        let options = FileSystemReadWriteOptions::new();
        options.set_at(offset(at, so_far)?);
        // `so_far < len`, so what's left doesn't wrap.
        match done(part(&options, so_far).map_err(js_error)?, len - so_far)? {
            0 => return Err(stalled.into()),
            // At most what's left, so within `len`.
            n => so_far += n,
        }
    }
    Ok(())
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.close();
    }
}
