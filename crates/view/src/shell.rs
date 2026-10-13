//! A shell's notes in the Timeline and the status bar. Its panel is in
//! `motion/shell.rs`, its faces' field in `motion/faces.rs`.

use varde_document::{LengthUnit, Shell};

use crate::feature_text::length_note;

/// A shell's Timeline note, as the UI mock's row: its thickness, "2 mm".
pub(crate) fn shell_note(shell: &Shell, units: LengthUnit) -> String {
    length_note(&shell.thickness, units)
}

/// What the status bar says of a selected shell, as the UI mock's row:
/// "2 faces removed · 2 mm inward", "Closed · 1 mm outward".
pub fn shell_info(shell: &Shell, units: LengthUnit) -> String {
    let faces = match shell.open.len() {
        0 => "Closed".to_owned(),
        count => format!("{} removed", crate::words::counted(count, "face", "faces")),
    };
    let side = if shell.outward { "outward" } else { "inward" };
    format!("{faces} · {} {side}", shell_note(shell, units))
}

#[cfg(test)]
mod tests;
