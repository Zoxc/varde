//! A draft's notes in the Timeline and the status bar. Its panel comes
//! with its session.

use varde_document::{Document, FaceDraft};

use crate::document::counted;
use crate::panels::angle_note;

/// A draft's Timeline note: its angle and the faces it turns, "3° · 4
/// faces".
pub(crate) fn draft_note(draft: &FaceDraft) -> String {
    format!(
        "{} · {}",
        angle_note(draft.angle.value),
        counted(draft.faces.len(), "face", "faces")
    )
}

/// What the status bar says of a selected draft: "4 faces · 3° from XY",
/// "1 face · 2° from a face, flipped".
pub fn draft_info(document: &Document, draft: &FaceDraft) -> String {
    let neutral = crate::motion::plane_short(document, &draft.neutral);
    let flipped = if draft.flip { ", flipped" } else { "" };
    format!(
        "{} · {} from {neutral}{flipped}",
        counted(draft.faces.len(), "face", "faces"),
        angle_note(draft.angle.value)
    )
}

#[cfg(test)]
mod tests;
