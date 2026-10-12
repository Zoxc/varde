//! The app's own small files in the config directory, the recent files
//! list and the settings: read whole, and replaced whole.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::atomic::{self, Mode};

/// The text of `store`, if it can be read, is UTF-8 and holds no more than
/// `max` bytes.
pub(crate) fn read(store: &Path, max: u64) -> Option<String> {
    read_from(File::open(store).ok()?, max)
}

/// The text of `file`, as [`read`] reads it, for a file already open.
pub(crate) fn read_from(file: File, max: u64) -> Option<String> {
    let mut text = String::new();
    // One byte more, to tell a file of `max` from a larger one.
    file.take(max.saturating_add(1))
        .read_to_string(&mut text)
        .ok()?;
    (u64::try_from(text.len()).ok()? <= max).then_some(text)
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
