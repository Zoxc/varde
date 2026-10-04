//! A chamfer's notes in the Timeline and the status bar. Its panel and
//! edge picking come with its session.

use varde_document::{Chamfer, ChamferSize, LengthUnit};

use crate::panels::{angle_note, length_note};

/// A chamfer's Timeline note, as the UI mock's row: its size, "1 mm",
/// "1 mm × 2 mm" (the first face's first), "3 mm 30°".
pub(crate) fn chamfer_note(chamfer: &Chamfer, units: LengthUnit) -> String {
    match &chamfer.distances {
        ChamferSize::Equal(d) => length_note(d, units),
        ChamferSize::Two(a, b) => {
            let (a, b) = if chamfer.flip { (b, a) } else { (a, b) };
            format!("{} × {}", length_note(a, units), length_note(b, units))
        }
        ChamferSize::Angle(d, a) => format!("{} {}", length_note(d, units), angle_note(a.value)),
    }
}

/// What the status bar says of a selected chamfer: "2 edges · Equal · 1
/// mm", "1 edge · Distance and angle · 3 mm at 30° · Tangent chain".
pub(crate) fn chamfer_info(chamfer: &Chamfer, units: LengthUnit) -> String {
    let count = chamfer.edges.len();
    let edges = if count == 1 {
        "1 edge".to_owned()
    } else {
        format!("{count} edges")
    };
    let size = match &chamfer.distances {
        ChamferSize::Angle(d, a) => {
            format!("{} at {}", length_note(d, units), angle_note(a.value))
        }
        _ => chamfer_note(chamfer, units),
    };
    let chain = if chamfer.chains {
        " · Tangent chain"
    } else {
        ""
    };
    format!("{edges} · {} · {size}{chain}", chamfer.distances.name())
}

#[cfg(test)]
mod tests;
