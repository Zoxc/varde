//! How design files are named: the name to save or download a design as,
//! and the `.vrdp` extension a chosen path gets.

use std::path::PathBuf;

use crate::EXTENSION;

/// The name of a design with no file.
pub const UNTITLED: &str = "Untitled";

/// The name to download the design `name` as, or suggest saving it as:
/// `name.vrdp`, with what a file name can't hold replaced. An empty name
/// is `Untitled`.
pub fn download_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Leading dots would hide it, trailing ones Windows drops.
    let name = name.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let name = if name.is_empty() { UNTITLED } else { name };
    if has_extension(name) {
        name.to_owned()
    } else {
        format!("{name}.{EXTENSION}")
    }
}

/// `path` with `.vrdp` added unless it ends with it already, and whether
/// it did.
pub fn with_extension(path: PathBuf) -> (PathBuf, bool) {
    let has = path
        .file_name()
        .is_some_and(|name| has_extension(&name.to_string_lossy()));
    if has {
        (path, true)
    } else {
        let mut path = path.into_os_string();
        path.push(format!(".{EXTENSION}"));
        (path.into(), false)
    }
}

/// Whether the file name `name` ends in `.vrdp`, in any case, after a
/// non-empty stem.
fn has_extension(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty() && extension.eq_ignore_ascii_case(EXTENSION)
    })
}

#[cfg(test)]
mod tests;
