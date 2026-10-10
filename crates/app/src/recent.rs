//! The recently opened files listed on the welcome screen.
//!
//! The IO lane stores the list (see [`varde_io::recent`]) and hands it over
//! once read at startup; until then the list is empty. Its times are shown
//! with [`crate::when`].

use std::path::{Path, PathBuf};

use varde_io::recent::{Listed, MAX, RecentFile};
use varde_io::{Request as IoRequest, UnixSeconds};

/// The recent files, newest first.
#[derive(Debug)]
pub(crate) struct Recent {
    entries: Vec<Listed>,
    /// The most entries kept, as the settings say: at most [`MAX`].
    keep: usize,
    /// Whether the stored list has arrived.
    loaded: bool,
    /// The user's home directory, abbreviated to `~` when showing paths.
    home: Option<PathBuf>,
}

impl Default for Recent {
    fn default() -> Self {
        Recent {
            entries: Vec::new(),
            keep: MAX,
            loaded: false,
            home: None,
        }
    }
}

impl Recent {
    /// Keeps at most `keep` entries, up to [`MAX`], from now on, the
    /// oldest beyond them dropped now: returning the write storing the
    /// list if that changed it, once it's complete.
    #[must_use = "the write stores the list"]
    pub(crate) fn keep(&mut self, keep: usize) -> Option<IoRequest> {
        self.keep = keep.min(MAX);
        self.trim().then(|| self.write()).flatten()
    }

    /// Drops the entries past [`Recent::keep`]'s count, saying whether
    /// there were any.
    fn trim(&mut self) -> bool {
        let trimmed = self.entries.len() > self.keep;
        self.entries.truncate(self.keep);
        trimmed
    }

    /// Newest first.
    pub(crate) fn entries(&self) -> &[Listed] {
        &self.entries
    }

    /// The directory holding `path`, with the user's home directory shown
    /// as `~`.
    pub(crate) fn display_dir(&self, path: &Path) -> String {
        display_dir(path, self.home.as_deref())
    }

    /// Takes the stored list, each file with whether it's there.
    /// Files opened before it arrived stay in front of it; if there were
    /// any, or it holds more than are kept, the list changed, and this
    /// returns the write storing it.
    #[must_use = "the write stores the list"]
    pub(crate) fn loaded(
        &mut self,
        entries: Vec<Listed>,
        home: Option<PathBuf>,
    ) -> Option<IoRequest> {
        let opened = std::mem::replace(&mut self.entries, entries);
        self.loaded = true;
        self.home = home;
        let changed = !opened.is_empty();
        for listed in opened.into_iter().rev() {
            self.remember(listed.entry.path, listed.entry.opened);
        }
        let changed = self.trim() || changed;
        if changed { self.write() } else { None }
    }

    /// Records that `path` was opened at `now`, returning the write
    /// storing the list, once it's complete.
    #[must_use = "the write stores the list"]
    pub(crate) fn opened(&mut self, path: PathBuf, now: UnixSeconds) -> Option<IoRequest> {
        self.remember(path, now);
        self.write()
    }

    /// The write storing the list, once it's complete: before the stored
    /// list has arrived, writing would drop it.
    fn write(&self) -> Option<IoRequest> {
        self.loaded.then(|| IoRequest::WriteRecent {
            entries: self
                .entries
                .iter()
                .map(|listed| listed.entry.clone())
                .collect(),
        })
    }

    /// Moves `path`, opened at `now`, to the front and drops the oldest
    /// entries beyond those kept.
    fn remember(&mut self, path: PathBuf, now: UnixSeconds) {
        self.entries.retain(|listed| listed.entry.path != path);
        let entry = RecentFile { path, opened: now };
        self.entries.insert(
            0,
            Listed {
                entry,
                available: true,
            },
        );
        self.trim();
    }
}

/// The directory holding `path`, with the `home` prefix shown as `~`.
fn display_dir(path: &Path, home: Option<&Path>) -> String {
    let Some(dir) = path.parent() else {
        return String::new();
    };
    match home.and_then(|home| dir.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => dir.display().to_string(),
    }
}

#[cfg(test)]
mod tests;
