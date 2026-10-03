//! What's done with the Origin Private File System's handles, see
//! `src/opfs.rs`: its directories and entries, the sync access handle
//! that holds an entry, and a directory as a [`Dir`], see `src/dir.rs`.

use std::io;
use std::path::Path;

use js_sys::{ArrayBuffer, AsyncIterator, Function, IteratorNext, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    File, FileReaderSync, FileSystemDirectoryHandle, FileSystemFileHandle,
    FileSystemGetDirectoryOptions, FileSystemGetFileOptions, FileSystemReadWriteOptions,
    FileSystemSyncAccessHandle, WorkerGlobalScope,
};

use crate::UnixSeconds;
use crate::dir::{Dir, Make};
use crate::js::{self, number, offset, seconds, size};
use crate::opfs::{dir_names, done};
use crate::vrdp::{ReadAt, Storage};
use crate::web::js::{call, js_error};
use crate::wire::MAX_MESSAGE_BYTES;

/// The root of the Origin Private File System. A browser without it, or
/// with it turned off, has no `getDirectory`: an error then, never an
/// exception, which would stop the worker.
pub(crate) async fn root() -> io::Result<FileSystemDirectoryHandle> {
    let scope: WorkerGlobalScope = js_sys::global().unchecked_into();
    let storage = scope.navigator().storage();
    let get_directory = Reflect::get(&storage, &JsValue::from_str("getDirectory"))
        .ok()
        .and_then(|method| method.dyn_into::<Function>().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "this browser keeps no files for the site",
            )
        })?;
    let promise: Promise = get_directory
        .call0(&storage)
        .map_err(js_error)?
        .unchecked_into();
    Ok(call(promise).await?.unchecked_into())
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

/// A file of the Origin Private File System as it was when found, read
/// in parts without taking it: synchronously, as the worker may, through
/// `FileReaderSync`. A read fails once the file has changed since.
#[derive(Debug)]
pub(crate) struct Snapshot(File);

impl ReadAt for Snapshot {
    fn len(&self) -> io::Result<u64> {
        size(self.0.size())
    }

    fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<()> {
        let end = offset(at, buf.len())?;
        if end > self.0.size() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let part = self
            .0
            .slice_with_f64_and_f64(number(at)?, end)
            .map_err(js_error)?;
        let buffer = FileReaderSync::new()
            .and_then(|reader| reader.read_as_array_buffer(&part))
            .map_err(js_error)?;
        let bytes = Uint8Array::new(&buffer);
        if usize::try_from(bytes.length()).ok() != Some(buf.len()) {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.copy_to(buf);
        Ok(())
    }
}

/// A directory of the Origin Private File System as a [`Dir`]: a file
/// taken is held through its sync access handle.
#[derive(Debug, Clone)]
pub(crate) struct OpfsDir(pub(crate) FileSystemDirectoryHandle);

impl OpfsDir {
    /// The handle of the file `name`, if it's there.
    async fn get(&self, name: &str) -> io::Result<FileSystemFileHandle> {
        file(&self.0, plain(name)?, false).await
    }

    /// Whether there's a file `name`.
    async fn has(&self, name: &str) -> io::Result<bool> {
        match self.get(name).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

impl Dir for OpfsDir {
    type File = Handle;
    type Reader = Snapshot;

    async fn names(&self) -> io::Result<Vec<String>> {
        names(&self.0).await
    }

    async fn take(&self, name: &str, make: Make) -> io::Result<Handle> {
        let handle = match make {
            Make::No => self.get(name).await?,
            Make::IfMissing => file(&self.0, plain(name)?, true).await?,
            Make::New => {
                if self.has(name).await? {
                    return Err(io::ErrorKind::AlreadyExists.into());
                }
                file(&self.0, plain(name)?, true).await?
            }
        };
        let taken = Handle::take(&handle).await?;
        // Made by another tab between looking and making, and written to.
        if make == Make::New && !taken.is_empty()? {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        Ok(taken)
    }

    async fn read(&self, name: &str, max: usize) -> io::Result<Vec<u8>> {
        let file: File = call(self.get(name).await?.get_file())
            .await?
            .unchecked_into();
        let len = js::len(file.size(), max, |size| {
            io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!("{name} is too large to read: {size} bytes"),
            )
        })?;
        let buffer: ArrayBuffer = call(file.array_buffer()).await?.unchecked_into();
        let bytes = Uint8Array::new(&buffer);
        // The length is the browser's to say, twice: they must agree.
        if usize::try_from(bytes.length()).ok() != Some(len) {
            return Err(io::Error::other(format!(
                "{name} changed while it was read"
            )));
        }
        Ok(bytes.to_vec())
    }

    async fn reader(&self, name: &str) -> io::Result<Snapshot> {
        let file: File = call(self.get(name).await?.get_file())
            .await?
            .unchecked_into();
        Ok(Snapshot(file))
    }

    async fn pause(&self, millis: u32) {
        let scope: WorkerGlobalScope = js_sys::global().unchecked_into();
        let mut wait = |resolve: Function, _reject: Function| {
            // `millis` is small, so it fits an `i32`; should it not, no
            // pause is only trying again sooner.
            let millis = i32::try_from(millis).unwrap_or(0);
            let _ = scope.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, millis);
        };
        let _ = call(Promise::new(&mut wait)).await;
    }

    async fn modified(&self, name: &str) -> Option<UnixSeconds> {
        modified(&self.get(name).await.ok()?).await
    }

    async fn remove(&self, name: &str) -> io::Result<()> {
        call(self.0.remove_entry(plain(name)?)).await.map(|_| ())
    }

    async fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        if self.has(plain(to)?).await? {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let handle = self.get(from).await?;
        // `move()`, where the browser has it; otherwise a copy.
        let moving = Reflect::get(&handle, &JsValue::from_str("move"))
            .ok()
            .and_then(|method| method.dyn_into::<Function>().ok());
        if let Some(moving) = moving {
            let promise: Promise = moving
                .call1(&handle, &JsValue::from_str(to))
                .map_err(js_error)?
                .unchecked_into();
            return call(promise).await.map(|_| ());
        }
        // Taken while it's copied, so nobody writes it meanwhile, and
        // read whole, as a design is, within what a design may be.
        let source = Handle::take(&handle).await?;
        let len = usize::try_from(source.len()?)
            .ok()
            .filter(|&len| len <= MAX_MESSAGE_BYTES)
            .ok_or_else(|| io::Error::from(io::ErrorKind::FileTooLarge))?;
        let mut bytes = vec![0; len];
        source.read_at(&mut bytes, 0)?;
        let mut copy = self.take(to, Make::New).await?;
        let copied = copy.write_at(&bytes, 0).and_then(|()| copy.sync());
        drop((copy, source));
        // Never two of it: the copy goes if it isn't whole, or if the
        // file can't be moved from after all.
        let moved = match copied {
            Ok(()) => match self.remove(from).await {
                // Deleted by someone else just now: the copy is all there is.
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                removed => removed,
            },
            Err(e) => Err(e),
        };
        if moved.is_err() {
            let _ = self.remove(to).await;
        }
        moved
    }
}

/// `name`, if it's a plain file name, as a [`Dir`]'s names are.
fn plain(name: &str) -> io::Result<&str> {
    if crate::dir::is_plain_name(name) {
        Ok(name)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} isn't a file name"),
        ))
    }
}
