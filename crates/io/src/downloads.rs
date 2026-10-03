//! The downloads made of designs in browser storage, on the web: what the
//! welcome screen and the file menu tell where each design stands against
//! by, see [`DownloadStatus`]. A download changes nothing in storage, and
//! the page isn't told whether the user kept the file, so it's only
//! recorded as made.
//!
//! Kept in `downloads.toml` at the root of the Origin Private File System,
//! the newest download of each design:
//!
//! ```toml
//! [[download]]
//! name = "bracket.vrdp"   # its file name in browser storage
//! time = 1727000000       # when, in seconds since the Unix epoch
//! record = "5f0e…"        # the sum of the save downloaded, 32 hex digits
//! ```
//!
//! `record` names the save downloaded by its `sum` (see `src/vrdp.rs`),
//! which any later save of the design, or a whole file written in its
//! place, tells apart; it's left out for a download of changes not saved.
//! The file is user data: each entry, from its `[[download]]` line to the
//! next, is read on its own, so one gone bad costs only itself, and there
//! are at most [`MAX`] of them. Of two of one design, the newer is kept.
//!
//! It's changed through its handle, read and written while held, so two
//! tabs recording at once don't lose each other's: one waits a moment for
//! the other. It's written in place, the new entries over the old, then
//! cut to length: a tab closed as it's written leaves the new entries up
//! to where it stopped, the old ones after, and at most one torn entry
//! between, which reading steps over, or reads as the start of a new
//! entry and the end of an old one: a wrong time at worst, as a `record`
//! is only ever the sum of a save of the design it was recorded for. A
//! download recorded twice so, the newer of the two counts.

use std::io;

use serde::{Deserialize, Serialize};

use crate::UnixSeconds;
use crate::browser::{DownloadStatus, LastDownload, is_design_name};
use crate::dir::{Dir, Make, take_waiting};
use crate::vrdp::{ReadAt, Storage};

/// The file's name at the root of the Origin Private File System.
pub(crate) const FILE: &str = "downloads.toml";

/// The most bytes of the file read: a larger one is taken as gone bad.
pub(crate) const MAX_BYTES: usize = 1 << 20;

/// The most designs whose downloads are kept: the newest.
pub(crate) const MAX: usize = 1024;

/// The downloads recorded, one per design.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Downloads {
    entries: Vec<Download>,
}

/// The newest download of a design.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Download {
    name: String,
    time: UnixSeconds,
    /// The `sum` of the save downloaded, if it was the design as saved.
    record: Option<u128>,
}

/// A [`Download`] as the file has it.
#[derive(Serialize, Deserialize)]
struct Entry {
    name: String,
    time: UnixSeconds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record: Option<String>,
}

/// The file as written.
#[derive(Serialize)]
struct File {
    download: Vec<Entry>,
}

impl Downloads {
    /// The downloads `toml` holds, each entry read on its own, from its
    /// `[[download]]` line to the next: one that doesn't parse, or names
    /// no design, is left out. Of two of one design, the newer.
    pub(crate) fn parse(toml: &str) -> Self {
        let mut downloads = Self::default();
        for section in sections(toml) {
            let Ok(table) = section.parse::<toml::Table>() else {
                continue;
            };
            let Some(toml::Value::Array(entries)) = table.get("download") else {
                continue;
            };
            let [entry] = entries.as_slice() else {
                continue;
            };
            let Ok(entry) = Entry::deserialize(entry.clone()) else {
                continue;
            };
            let record = match entry.record {
                Some(hex) => match parse_sum(&hex) {
                    Some(sum) => Some(sum),
                    None => continue,
                },
                None => None,
            };
            let newer = (downloads.entries.iter())
                .find(|kept| kept.name == entry.name)
                .is_none_or(|kept| kept.time <= entry.time);
            if is_design_name(&entry.name) && newer {
                downloads.put(Download {
                    name: entry.name,
                    time: entry.time,
                    record,
                });
            }
        }
        downloads
    }

    pub(crate) fn serialize(&self) -> String {
        let file = File {
            download: (self.entries.iter())
                .map(|download| Entry {
                    name: download.name.clone(),
                    time: download.time,
                    record: download.record.map(|sum| format!("{sum:032x}")),
                })
                .collect(),
        };
        toml::to_string(&file).expect("plain tables serialize")
    }

    /// Keeps `download`, in place of the one before of its design, and
    /// within [`MAX`], dropping the oldest.
    fn put(&mut self, download: Download) {
        self.entries.retain(|kept| kept.name != download.name);
        self.entries.push(download);
        if self.entries.len() > MAX {
            let oldest = (self.entries.iter().enumerate())
                .min_by_key(|(_, kept)| kept.time)
                .map(|(at, _)| at);
            if let Some(at) = oldest {
                self.entries.remove(at);
            }
        }
    }

    /// Records a download of the design `name` at `time`: of its save
    /// whose sum is `record`, or of changes not saved if that's `None`.
    pub(crate) fn record(&mut self, name: &str, time: UnixSeconds, record: Option<u128>) {
        self.put(Download {
            name: name.to_owned(),
            time,
            record,
        });
    }

    /// The design `from` is called `to` now: its downloads follow it.
    pub(crate) fn renamed(&mut self, from: &str, to: &str) {
        self.entries.retain(|kept| kept.name != to);
        if let Some(download) = self.entries.iter_mut().find(|kept| kept.name == from) {
            download.name = to.to_owned();
        }
    }

    /// The design `name` is gone, and its downloads with it.
    pub(crate) fn removed(&mut self, name: &str) {
        self.entries.retain(|kept| kept.name != name);
    }

    /// Where the design `name` stands against its downloads, its newest
    /// save's sum being `newest`, if it can be read, and `unsaved` if
    /// there are changes not saved: the latest downloaded only if the
    /// download was of that save, with nothing changed since.
    pub(crate) fn status(&self, name: &str, newest: Option<u128>, unsaved: bool) -> DownloadStatus {
        match self.entries.iter().find(|kept| kept.name == name) {
            None => DownloadStatus::Never,
            Some(download)
                if download.record.is_some() && download.record == newest && !unsaved =>
            {
                DownloadStatus::Latest(download.time)
            }
            Some(download) => DownloadStatus::Changed(download.time),
        }
    }

    /// The last download of the design `name`, if there's one, as it
    /// stands against its newest save, whose sum is `newest`, with no
    /// changes since.
    pub(crate) fn last(&self, name: &str, newest: u128) -> Option<LastDownload> {
        let download = self.entries.iter().find(|kept| kept.name == name)?;
        Some(LastDownload {
            time: download.time,
            latest: download.record == Some(newest),
        })
    }
}

/// The entries of `toml`, each a `[[download]]` line and the lines after
/// it up to the next: what's before the first is no entry.
fn sections(toml: &str) -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    for line in toml.lines() {
        if line.trim() == "[[download]]" {
            sections.push(String::new());
        }
        if let Some(section) = sections.last_mut() {
            section.push_str(line);
            section.push('\n');
        }
    }
    sections
}

/// A sum as the file writes it: 32 hex digits.
fn parse_sum(hex: &str) -> Option<u128> {
    (hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| u128::from_str_radix(hex, 16).ok())
        .flatten()
}

/// The downloads recorded in `dir`, the root, read without taking the
/// file: none if it can't be read, which only costs what's shown.
pub(crate) async fn load(dir: &impl Dir) -> Downloads {
    match dir.read(FILE, MAX_BYTES).await {
        Ok(bytes) => Downloads::parse(&String::from_utf8_lossy(&bytes)),
        Err(_) => Downloads::default(),
    }
}

/// Changes the downloads recorded in `dir`, the root, as `change` does,
/// returning what it returns: read and written while the file is held, so
/// another tab changing them meanwhile waits its turn, a moment (see
/// [`take_waiting`]), refused if it's held longer. One gone bad, or too
/// large, starts over.
pub(crate) async fn update<T>(
    dir: &impl Dir,
    change: impl FnOnce(&mut Downloads) -> T,
) -> io::Result<T> {
    let mut file = take_waiting(dir, FILE, Make::IfMissing).await?;
    let len = file.len()?;
    let mut downloads = match usize::try_from(len) {
        Ok(len) if len <= MAX_BYTES => {
            let mut bytes = vec![0; len];
            file.read_at(&mut bytes, 0)?;
            Downloads::parse(&String::from_utf8_lossy(&bytes))
        }
        _ => Downloads::default(),
    };
    let answer = change(&mut downloads);
    let bytes = downloads.serialize().into_bytes();
    file.write_at(&bytes, 0)?;
    // A `usize` fits a `u64` on every target.
    file.truncate(bytes.len() as u64)?;
    file.sync()?;
    Ok(answer)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
