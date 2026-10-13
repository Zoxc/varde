//! The app's own small files in the config directory, the recent files
//! list and the settings: read whole, and replaced whole.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::atomic::{self, Mode};

/// The text of `store`, if it can be read and holds no more than `max`
/// bytes. Decoded lossily, as the web reads these files: a stray invalid
/// byte in a value costs only that value.
pub(crate) fn read(store: &Path, max: u64) -> Option<String> {
    read_from(File::open(store).ok()?, max)
}

/// The text of `file`, as [`read`] reads it, for a file already open.
pub(crate) fn read_from(file: File, max: u64) -> Option<String> {
    let mut bytes = Vec::new();
    // One byte more, to tell a file of `max` from a larger one.
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (u64::try_from(bytes.len()).ok()? <= max).then(|| String::from_utf8_lossy(&bytes).into_owned())
}

/// Replaces `store` with `text`, see [`atomic::replace`]: a failed write
/// never leaves it truncated. Creates its directory if need be.
pub(crate) fn replace(store: &Path, text: &str) -> io::Result<()> {
    replace_with(store, text, Mode::Keep)
}

/// [`replace`], the file readable by the user only, see [`Mode::Private`].
pub(crate) fn replace_private(store: &Path, text: &str) -> io::Result<()> {
    replace_with(store, text, Mode::Private)
}

fn replace_with(store: &Path, text: &str, mode: Mode) -> io::Result<()> {
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)?;
    }
    atomic::replace(store, mode, text.as_bytes())
}
