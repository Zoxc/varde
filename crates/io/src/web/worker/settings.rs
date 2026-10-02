//! Reading and writing the settings, `settings.toml` in the Origin Private
//! File System, see `src/settings.rs`.

use std::io;
use std::path::Path;

use wasm_bindgen::JsCast;
use web_sys::File;

use super::opfs::{Handle, file_at};
use crate::js::size;
use crate::settings::{MAX_BYTES, Settings};
use crate::vrdp::Storage;
use crate::web::js::call;

/// Reads the settings from `store`. A missing or unreadable store gives
/// the defaults.
pub(crate) async fn load(store: &Path) -> Settings {
    match read(store).await {
        Ok(Some(toml)) => Settings::parse(&toml),
        _ => Settings::default(),
    }
}

/// The text of `store`, if it's there and no larger than settings are.
/// Read without taking its handle, which a write in another tab may hold.
async fn read(store: &Path) -> io::Result<Option<String>> {
    let handle = match file_at(store, false).await {
        Ok(handle) => handle,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let file: File = call(handle.get_file()).await?.unchecked_into();
    if size(file.size())? > MAX_BYTES {
        return Ok(None);
    }
    Ok(call(file.text()).await?.as_string())
}

/// Writes `settings` to `store`. Through its sync access handle, which
/// every browser with the Origin Private File System has in workers: a
/// failure part of the way leaves a file that may not parse, which only
/// costs the settings.
pub(crate) async fn write(store: &Path, settings: &Settings) -> io::Result<()> {
    let bytes = settings.serialize().into_bytes();
    let mut handle = Handle::take(&file_at(store, true).await?).await?;
    handle.write_at(&bytes, 0)?;
    // A `usize` fits a `u64` on every target.
    handle.truncate(bytes.len() as u64)?;
    handle.sync()
}
