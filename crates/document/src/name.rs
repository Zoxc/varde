//! How design files are named: the name to save or download a design as,
//! and the `.vrdp` extension a chosen path gets; and likewise for files
//! exported from a design, with their own extension.

use std::path::PathBuf;

use crate::EXTENSION;

/// The name of a design with no file.
pub const UNTITLED: &str = "Untitled";

/// The name to download the design `name` as, or suggest saving it as:
/// `name.vrdp`, with what a file name can't hold replaced, cut to 255
/// bytes, and an underscore after a name Windows keeps for a device
/// (`CON_.vrdp`). An empty name is `Untitled`.
pub fn download_name(name: &str) -> String {
    download_name_with(name, EXTENSION)
}

/// [`download_name`] with `extension` (without the dot) in place of
/// `.vrdp`: the name to export the design `name` as, e.g. `name.3mf`.
pub fn download_name_with(name: &str, extension: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Leading dots would hide it, trailing ones Windows drops.
    let trim = |name: &str| -> String {
        let name = name.trim_matches(|c: char| c == '.' || c.is_whitespace());
        if name.is_empty() { UNTITLED } else { name }.to_owned()
    };
    let name = trim(&name);
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, ends)) if has_extension(&name, extension) => (stem, ends),
        _ => (name.as_str(), extension),
    };
    // A file name is at most 255 bytes almost everywhere.
    let mut room = MAX_FILE_NAME - 1 - extension.len();
    while !stem.is_char_boundary(room.min(stem.len())) {
        room -= 1;
    }
    let mut stem = trim(&stem[..room.min(stem.len())]);
    if reserved_on_windows(&stem) {
        stem.push('_');
    }
    format!("{stem}.{extension}")
}

/// The most bytes in a file name [`download_name`] gives.
const MAX_FILE_NAME: usize = 255;

/// Whether Windows takes a file named `stem` and an extension for a
/// device, such as `CON` or `com1.tar`: by the part before its first dot,
/// trailing spaces dropped, in any case.
fn reserved_on_windows(stem: &str) -> bool {
    let base = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    let base = base.to_ascii_uppercase();
    let numbered = |prefix: &str| {
        base.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    };
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || numbered("COM")
        || numbered("LPT")
}

/// `path` with `.vrdp` added unless it ends with it already, and whether
/// it did.
pub fn with_extension(path: PathBuf) -> (PathBuf, bool) {
    with_extension_of(path, EXTENSION)
}

/// [`with_extension`] with `extension` (without the dot) in place of
/// `.vrdp`.
pub fn with_extension_of(path: PathBuf, extension: &str) -> (PathBuf, bool) {
    let has = path
        .file_name()
        .is_some_and(|name| has_extension(&name.to_string_lossy(), extension));
    if has {
        (path, true)
    } else {
        let mut path = path.into_os_string();
        path.push(format!(".{extension}"));
        (path.into(), false)
    }
}

/// Whether the file name `name` ends in `.{extension}`, in any case,
/// after a non-empty stem.
fn has_extension(name: &str, extension: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ends)| !stem.is_empty() && ends.eq_ignore_ascii_case(extension))
}

#[cfg(test)]
mod tests;
