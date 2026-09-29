//! Reading and writing the recent files list, `recent.toml`, see
//! `src/recent.rs`.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{files, unique};
use crate::recent::{Listed, MAX, RecentFile};

/// The on-disk layout, read one entry at a time, so a bad one loses only
/// itself.
#[derive(Default, Deserialize)]
struct Stored {
    #[serde(default, rename = "file")]
    files: Vec<toml::Value>,
}

/// The on-disk layout, as written.
#[derive(Serialize)]
struct Store<'a> {
    #[serde(rename = "file")]
    files: Vec<&'a RecentFile>,
}

/// The user's home directory, abbreviated to `~` when showing paths.
pub(crate) fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_owned())
}

/// Reads the list from `store`. A missing or unreadable store gives an
/// empty list. Files that can't be found are kept: they may be on a drive
/// that isn't mounted just now, see [`listed`].
pub(crate) fn load(store: &Path) -> Vec<RecentFile> {
    std::fs::read_to_string(store)
        .ok()
        .and_then(|toml| parse(&toml).ok())
        .unwrap_or_default()
}

/// `entries`, each with whether it's there. One whose existence can't be
/// told, e.g. for lack of permission, is taken to be there: opening it
/// says what's wrong.
pub(crate) fn listed(entries: Vec<RecentFile>) -> Vec<Listed> {
    entries
        .into_iter()
        .map(|entry| Listed {
            available: !matches!(entry.path.try_exists(), Ok(false)),
            entry,
        })
        .collect()
}

/// Writes `entries` to `store`. Writes a temporary file first and renames
/// it over the store, so a failed write never leaves a truncated list, and
/// syncs the directory so the rename lasts. The temporary file's name is
/// this write's own, so another instance writing at the same time can't
/// write into it.
pub(crate) fn write(store: &Path, entries: &[RecentFile]) -> io::Result<()> {
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (temporary, file) = temporary(store)?;
    // Closed before it's renamed, which Windows needs.
    let written = {
        let mut file = file;
        file.write_all(serialize(entries).as_bytes())
            .and_then(|()| file.sync_all())
    };
    let result = written.and_then(|()| std::fs::rename(&temporary, store));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return result;
    }
    // Best effort, like a design file's replace: the list is written, and
    // failing here only risks a crash bringing the old one back.
    let _ = files::sync_parent(store);
    Ok(())
}

/// Creates a new temporary file next to `store`:
/// `recent.toml.{name}.tmp`, with a [`unique::name`].
fn temporary(store: &Path) -> io::Result<(PathBuf, File)> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    unique::create(&options, unique::ATTEMPTS, |name| {
        store.with_extension(format!("toml.{name}.tmp"))
    })
}

/// The entries of `toml`, leaving out the ones that don't parse.
fn parse(toml: &str) -> Result<Vec<RecentFile>, toml::de::Error> {
    let files = toml::from_str::<Stored>(toml)?.files;
    Ok(files
        .into_iter()
        .filter_map(|file| file.try_into().ok())
        .take(MAX)
        .collect())
}

fn serialize(entries: &[RecentFile]) -> String {
    let store = Store {
        // TOML strings are UTF-8, so other paths can't be stored.
        files: entries
            .iter()
            .filter(|entry| entry.path.to_str().is_some())
            .take(MAX)
            .collect(),
    };
    toml::to_string(&store).expect("a list of UTF-8 paths and integers serializes")
}

#[cfg(test)]
mod tests;
