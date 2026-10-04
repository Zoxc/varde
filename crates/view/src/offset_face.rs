//! An offset face's notes in the Timeline and the status bar. Its panel
//! and handle come with its session.

use varde_document::{LengthUnit, OffsetFace};

use crate::panels::length_note;

/// "1 face", "3 faces".
fn faces(count: usize) -> String {
    if count == 1 {
        "1 face".to_owned()
    } else {
        format!("{count} faces")
    }
}

/// An offset face's Timeline note: its distance and the faces it moves,
/// "2 mm · 3 faces", the distance "in" when it moves them inward.
pub(crate) fn offset_note(offset: &OffsetFace, units: LengthUnit) -> String {
    let distance = length_note(&offset.distance, units);
    let side = if offset.inward { " in" } else { "" };
    format!("{distance}{side} · {}", faces(offset.faces.len()))
}

/// What the status bar says of a selected offset face, in the style of
/// the UI mock's shell rows: "2 faces · 2 mm outward", "1 face · 0.5 mm
/// inward".
pub(crate) fn offset_info(offset: &OffsetFace, units: LengthUnit) -> String {
    let side = if offset.inward { "inward" } else { "outward" };
    format!(
        "{} · {} {side}",
        faces(offset.faces.len()),
        length_note(&offset.distance, units)
    )
}

#[cfg(test)]
mod tests;
