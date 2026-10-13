//! An extrude's notes in the Timeline and the status bar. Its panel is
//! in `extrude.rs`.

use varde_document::Extent;
use varde_expr::LengthUnit;

use crate::feature_text::{angle_note, length_note};
use crate::operation_panel::OperationKind;

/// How far an extrude goes, for its Timeline row, in `units`: "10 mm",
/// "10 mm symmetric", "10 mm + 5 mm", "Through all".
pub(crate) fn extent_note(extent: &Extent, units: LengthUnit) -> String {
    let length = |value| length_note(value, units);
    match extent {
        Extent::OneSide(d) => length(d),
        Extent::Symmetric(d) => format!("{} symmetric", length(d)),
        Extent::TwoSides(a, b) => format!("{} + {}", length(a), length(b)),
        Extent::ThroughAll => "Through all".to_owned(),
    }
}

/// An extrude's Timeline note: its extent, and its taper if it has
/// one, "10 mm · 2°".
pub(crate) fn extrude_note(extrude: &varde_document::Extrude, units: LengthUnit) -> String {
    let extent = extent_note(&extrude.extent, units);
    match extrude.tapered() {
        Some(taper) => format!("{extent} · {}", angle_note(taper.value)),
        None => extent,
    }
}

/// An extrude's status bar info, in `units`: its extent and operation,
/// "Distance 10 mm · New body", with its taper if it has one.
pub(crate) fn extrude_info(extrude: &varde_document::Extrude, units: LengthUnit) -> String {
    let note = extent_note(&extrude.extent, units);
    let extent = match &extrude.extent {
        Extent::OneSide(d) => format!("Distance {}", length_note(d, units)),
        Extent::Symmetric(d) => format!("Symmetric {}", length_note(d, units)),
        Extent::TwoSides(..) => format!("Two sides {note}"),
        Extent::ThroughAll => note,
    };
    let operation = OperationKind::of(&extrude.operation).label();
    match extrude.tapered() {
        Some(taper) => {
            let taper = angle_note(taper.value);
            format!("{extent} · Taper {taper} · {operation}")
        }
        None => format!("{extent} · {operation}"),
    }
}

#[cfg(test)]
mod tests;
