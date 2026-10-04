//! A loft's notes in the Timeline and the status bar. Its panel comes
//! with its session.

use varde_document::{Loft, LoftMode};

use crate::operation_panel::OperationKind;

/// A loft's Timeline note, as the plan's rows: its section count, "3
/// sections".
pub(crate) fn loft_note(loft: &Loft) -> String {
    format!("{} sections", loft.sections.len())
}

/// What the status bar says of a selected loft: "3 sections · Smooth ·
/// Closed · 2 rails · New body", "2 sections · Ruled · Join" (two
/// sections are ruled whatever the mode says).
pub(crate) fn loft_info(loft: &Loft) -> String {
    let mut parts = vec![loft_note(loft)];
    let ruled = loft.mode == LoftMode::Ruled || loft.sections.len() <= 2;
    parts.push(if ruled { "Ruled" } else { "Smooth" }.to_owned());
    if loft.closed {
        parts.push("Closed".to_owned());
    }
    match loft.rails.len() {
        0 => {}
        1 => parts.push("1 rail".to_owned()),
        rails => parts.push(format!("{rails} rails")),
    }
    parts.push(OperationKind::of(&loft.operation).label().to_owned());
    parts.join(" · ")
}

#[cfg(test)]
mod tests;
