//! The app's own small files in the config directory, the recent files
//! list and the settings: read whole, and replaced whole.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::{files, unique};

/// The text of `store`, if it can be read, is UTF-8 and holds no more than
/// `max` bytes.
pub(crate) fn read(store: &Path, max: u64) -> Option<String> {
    let mut text = String::new();
    File::open(store)
        .ok()?
        // One byte more, to tell a file of `max` from a larger one.
        .take(max.saturating_add(1))
        .read_to_string(&mut text)
        .ok()?;
    (u64::try_from(text.len()).ok()? <= max).then_some(text)
}

/// Replaces `store` with `text`. Writes a temporary file first and renames
/// it over the store, so a failed write never leaves it truncated, and
/// syncs the directory so the rename lasts. The temporary file's name is
/// this write's own, so another instance writing at the same time can't
/// write into it.
pub(crate) fn replace(store: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (temporary, file) = temporary(store)?;
    // Closed before it's renamed, which Windows needs.
    let written = {
        let mut file = file;
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
    };
    let result = written.and_then(|()| std::fs::rename(&temporary, store));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return result;
    }
    // Best effort, like a design file's replace: the file is written, and
    // failing here only risks a crash bringing the old one back.
    let _ = files::sync_parent(store);
    Ok(())
}

/// Creates a new temporary file next to `store`, e.g.
/// `recent.toml.{name}.tmp`, with a [`unique::name`].
fn temporary(store: &Path) -> io::Result<(PathBuf, File)> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let extension = store
        .extension()
        .map(|extension| format!("{}.", extension.to_string_lossy()))
        .unwrap_or_default();
    unique::create(&options, unique::ATTEMPTS, |name| {
        store.with_extension(format!("{extension}{name}.tmp"))
    })
}
