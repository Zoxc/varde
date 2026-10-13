//! Path helpers shared by the lane's requests (`files`) and exports
//! (`export`).

use std::path::{Path, PathBuf};

/// The file name of `path`, for messages.
pub(super) fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// `path` made absolute, as the lane keeps and reports every path the user
/// gave: left as it is if the current directory can't be found.
pub(super) fn absolute(path: PathBuf) -> PathBuf {
    std::path::absolute(&path).unwrap_or(path)
}

/// Where the design at `path` really is: through symbolic links, including
/// ones to its directory, should it not exist yet. Its lock sits next to
/// it, so every path to a design shares one lock.
pub(super) fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .ok()
        .or_else(|| {
            let parent = std::fs::canonicalize(path.parent()?).ok()?;
            Some(parent.join(path.file_name()?))
        })
        .unwrap_or_else(|| path.to_owned())
}
