//! Writing an exported file natively, see [`Request::Export`]: a new file
//! at a path, or one written next to the file it replaces and renamed over
//! it, so a crash or a failure leaves the old file whole.
//!
//! [`Request::Export`]: crate::Request::Export

use std::io;
use std::path::Path;

use super::atomic::{self, Mode};
use super::path::{display_name, resolved};
use crate::three_mf::{self, Body};

/// Writes `bodies` as a 3MF package titled `title` to `path`, replacing a
/// file there only if `overwrite`, which the user agreed to. Written to
/// where a symbolic link points, so the link stays.
pub(crate) fn export(
    path: &Path,
    overwrite: bool,
    title: &str,
    bodies: &[Body],
) -> Result<(), String> {
    let bytes = three_mf::package(title, bodies).map_err(|e| e.to_string())?;
    let real = resolved(path);
    let written = if overwrite {
        atomic::replace(&real, Mode::Keep, &bytes)
    } else {
        atomic::create(&real, &bytes)
    };
    written.map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => format!("{} already exists", display_name(path)),
        _ => format!("couldn't write {}: {e}", display_name(path)),
    })
}

#[cfg(test)]
mod tests;
