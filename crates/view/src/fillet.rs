//! A fillet's notes in the Timeline and the status bar. Its panel is in
//! `motion/fillet.rs`, its edges' field in `motion/blend.rs`.

use varde_document::{Fillet, LengthUnit};

use crate::panels::length_note;

/// A fillet's Timeline note, as the UI mock's row: its radius, "R2" (a
/// number of the design's units).
pub(crate) fn fillet_note(fillet: &Fillet, units: LengthUnit) -> String {
    format!(
        "R{}",
        varde_expr::format_number(fillet.radius.value, Some(units.into()))
    )
}

/// What the status bar says of a selected fillet, as the UI mock's row:
/// "2 edges · R2 mm · Tangent chain".
pub fn fillet_info(fillet: &Fillet, units: LengthUnit) -> String {
    let count = fillet.edges.len();
    let edges = if count == 1 {
        "1 edge".to_owned()
    } else {
        format!("{count} edges")
    };
    let chain = if fillet.chains {
        " · Tangent chain"
    } else {
        ""
    };
    format!("{edges} · R{}{chain}", length_note(&fillet.radius, units))
}

#[cfg(test)]
mod tests;
