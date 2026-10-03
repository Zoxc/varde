//! The panic recorded natively, `panic.toml` in the data directory, see
//! `src/panicked.rs`.

use std::io;
use std::path::Path;

use super::config;
use crate::panicked::{MAX_BYTES, Panic, store};

/// Records `panic` where [`store`] says, from the panic hook: failing is
/// only logged, to stderr, as the logger may be what panicked.
pub(crate) fn record(panic: &Panic) {
    let Some(store) = store() else {
        return;
    };
    if let Err(error) = config::replace(&store, &panic.serialize()) {
        eprintln!("Couldn't record the panic in {}: {error}", store.display());
    }
}

/// The panic recorded in `store`, if there's one that can be read.
pub(crate) fn load(store: &Path) -> Option<Panic> {
    config::read(store, MAX_BYTES).and_then(|toml| Panic::parse(&toml))
}

/// Deletes the panic recorded in `store` if it's `panic`, rather than one
/// recorded since, by another session.
pub(crate) fn discard(store: &Path, panic: &Panic) -> io::Result<()> {
    if load(store).as_ref() != Some(panic) {
        return Ok(());
    }
    match std::fs::remove_file(store) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
