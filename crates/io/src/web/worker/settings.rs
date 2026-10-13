//! Reading and writing the settings, `settings.toml` in the Origin Private
//! File System, see `src/settings.rs`.

use std::io;
use std::path::Path;

use super::opfs::{self, OpfsDir, not_a_place};
use crate::settings::{MAX_BYTES, Settings};
use crate::web_support::dir::{Dir, Make, WholeFile};

/// Reads the settings from `store`. A missing or unreadable store gives
/// the defaults.
pub(crate) async fn load(store: &Path) -> Settings {
    match read(store).await {
        Ok(bytes) => Settings::parse(&String::from_utf8_lossy(&bytes)),
        Err(_) => Settings::default(),
    }
}

/// The bytes of `store`, if it's there and no larger than settings are.
/// Read without taking its handle, which a write in another tab may hold.
async fn read(store: &Path) -> io::Result<Vec<u8>> {
    let (dir, name) = place(store).await?;
    let max = usize::try_from(MAX_BYTES).unwrap_or(usize::MAX);
    dir.read(name, max).await
}

/// Writes `settings` to `store`. Through its sync access handle, which
/// every browser with the Origin Private File System has in workers: a
/// failure part of the way leaves a file that may not parse, which only
/// costs the settings.
pub(crate) async fn write(store: &Path, settings: &Settings) -> io::Result<()> {
    let bytes = settings.serialize().into_bytes();
    let (dir, name) = place(store).await?;
    dir.take(name, Make::IfMissing).await?.replace_whole(&bytes)
}

/// The directory `store` is in, made if needed, and its name there.
async fn place(store: &Path) -> io::Result<(OpfsDir, &str)> {
    let name = store
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| not_a_place(store))?;
    let dir = match store.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => opfs::dir(parent).await?,
        _ => opfs::root().await?,
    };
    Ok((OpfsDir(dir), name))
}
