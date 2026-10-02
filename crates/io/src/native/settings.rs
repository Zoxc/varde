//! Reading and writing the settings, `settings.toml`, see
//! `src/settings.rs`.

use std::io;
use std::path::Path;

use super::config;
use crate::settings::{MAX_BYTES, Settings};

/// Reads the settings from `store`. A missing or unreadable store gives
/// the defaults.
pub(crate) fn load(store: &Path) -> Settings {
    config::read(store, MAX_BYTES)
        .map(|toml| Settings::parse(&toml))
        .unwrap_or_default()
}

/// Writes `settings` to `store`, see [`config::replace`].
pub(crate) fn write(store: &Path, settings: &Settings) -> io::Result<()> {
    config::replace(store, &settings.serialize())
}

#[cfg(test)]
mod tests;
