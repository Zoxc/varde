//! Files of the user's in the IO worker, see `src/pick.rs`: reading
//! and writing what the page handed over for them. Never on the page.

use std::io;

use js_sys::{ArrayBuffer, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FileSystemFileHandle, FileSystemWritableFileStream};

use crate::js::{self, file_len};
use crate::vrdp::{self, WholeRead};
use crate::web::js::{call, js_error};

/// What the page posted along with a request using a file the user
/// picked, see [`object`](crate::web::page::pick::object).
#[derive(Debug)]
pub(crate) enum Handed {
    /// From the File System Access API's pickers: read, and written back.
    Handle(FileSystemFileHandle),
    /// From a file input: read once.
    File(File),
}

impl Handed {
    /// What `value` is, `None` if it's neither.
    pub(crate) fn from_js(value: JsValue) -> Option<Self> {
        match value.dyn_into::<FileSystemFileHandle>() {
            Ok(handle) => Some(Self::Handle(handle)),
            Err(value) => value.dyn_into::<File>().ok().map(Self::File),
        }
    }
}

/// Reads the whole file `handed` stands for, see
/// [`read_file`].
pub(crate) async fn read(handed: &Handed) -> vrdp::Result<Vec<u8>> {
    match handed {
        Handed::Handle(handle) => read_handle(handle).await,
        Handed::File(file) => read_file(file).await,
    }
}

/// Reads the whole file `handle` refers to, see
/// [`read_file`].
pub(crate) async fn read_handle(handle: &FileSystemFileHandle) -> vrdp::Result<Vec<u8>> {
    let file: File = call(handle.get_file()).await?.unchecked_into();
    read_file(&file).await
}

/// Reads the whole of `file` once its start shows it's a `.vrdp` file, as
/// natively: anything else is refused having read no more than the file
/// header, and one too large to hold in memory is an error rather than an
/// abort.
async fn read_file(file: &File) -> vrdp::Result<Vec<u8>> {
    let read = WholeRead::new(file_len(file.size())?);
    let mut head = vec![0; read.head_len()];
    read_at(file, &mut head, 0).await?;
    let mut bytes = read.buffer(head)?;
    let start = read.head_len();
    // A `usize` fits a `u64` on every target.
    read_at(file, &mut bytes[start..], start as u64).await?;
    Ok(bytes)
}

/// Reads the bytes of `file` from `at` into `buf`, whose end is within
/// the file's length as read.
async fn read_at(file: &File, buf: &mut [u8], at: u64) -> io::Result<()> {
    let part = file
        .slice_with_f64_and_f64(js::offset(at, 0)?, js::offset(at, buf.len())?)
        .map_err(js_error)?;
    let buffer: ArrayBuffer = call(part.array_buffer()).await?.unchecked_into();
    let bytes = Uint8Array::new(&buffer);
    // The length is the browser's to say, twice: they must agree.
    if usize::try_from(bytes.length()).ok() != Some(buf.len()) {
        return Err(io::Error::other("the file changed while it was read"));
    }
    bytes.copy_to(buf);
    Ok(())
}

/// Replaces the whole file `handle` refers to with `bytes`.
/// The browser writes a copy and moves it over the file as it's closed, so
/// a failure leaves the file as it was.
pub(crate) async fn write(handle: &FileSystemFileHandle, bytes: &[u8]) -> io::Result<()> {
    let stream: FileSystemWritableFileStream =
        call(handle.create_writable()).await?.unchecked_into();
    let written = async {
        call(stream.write_with_u8_array(bytes).map_err(js_error)?).await?;
        call(stream.close()).await
    };
    if let Err(error) = written.await {
        let _ = JsFuture::from(stream.abort()).await;
        return Err(error);
    }
    Ok(())
}
