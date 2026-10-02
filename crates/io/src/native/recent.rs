//! Reading and writing the recent files list, `recent.toml`, see
//! `src/recent.rs`.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::config;
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

/// Writes `entries` to `store`, see [`config::replace`].
pub(crate) fn write(store: &Path, entries: &[RecentFile]) -> io::Result<()> {
    config::replace(store, &serialize(entries))
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
