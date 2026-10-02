//! The user's settings.
//!
//! Kept in `settings.toml`: natively in the platform config directory next
//! to the recent files list, e.g. `~/.config/varde-cad/settings.toml`; on
//! the web at the root of the Origin Private File System. A table of
//! plain keys:
//!
//! ```toml
//! theme = "auto" # or "light", "dark"
//! ```
//!
//! The file is user data, edited by hand perhaps: each key is read on its
//! own, so one missing or gone bad gets its default and costs no other.
//! Keys this build doesn't know are left out, and dropped by the next
//! write. Reading and writing it is `src/native/settings.rs`'s natively,
//! `src/web/worker/files.rs`'s on the web.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The most bytes of the file read: a larger one is taken as gone bad.
pub(crate) const MAX_BYTES: u64 = 64 * 1024;

/// The settings, each defaulted when the file doesn't say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Settings {
    pub theme: Theme,
}

/// Whether the UI is light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// As the system prefers, light if it doesn't say.
    #[default]
    Auto,
    Light,
    Dark,
}

impl Settings {
    /// The settings `toml` holds, any of them missing or gone bad
    /// defaulted. Nothing in it is usable if it isn't a TOML table.
    pub(crate) fn parse(toml: &str) -> Self {
        let Ok(table) = toml.parse::<toml::Table>() else {
            return Self::default();
        };
        let key = |name: &str| table.get(name).cloned();
        Self {
            theme: key("theme")
                .and_then(|value| value.try_into().ok())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn serialize(&self) -> String {
        toml::to_string(self).expect("plain keys serialize")
    }
}

/// Where the settings are stored, if there is a config directory to put
/// them in.
#[cfg(not(target_arch = "wasm32"))]
pub fn store() -> Option<PathBuf> {
    crate::native::project_dirs().map(|dirs| dirs.config_dir().join("settings.toml"))
}

/// On the web, `settings.toml` at the root of the Origin Private File
/// System.
#[cfg(target_arch = "wasm32")]
pub fn store() -> Option<PathBuf> {
    Some(PathBuf::from("settings.toml"))
}

#[cfg(test)]
mod tests;
