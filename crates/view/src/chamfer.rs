//! A chamfer's notes in the Timeline and the status bar. Its panel is
//! in `motion/chamfer.rs`, its edges' field in `motion/blend.rs`.

use varde_document::{Chamfer, ChamferSize, LengthUnit};

use crate::panels::{angle_note, length_note};

/// A chamfer's Timeline note, as the UI mock's row: its size, "1 mm",
/// "1 × 2" (along the face of the edges' first key first, so Flip sides
/// swaps them), "3 mm 30°".
pub(crate) fn chamfer_note(chamfer: &Chamfer, units: LengthUnit) -> String {
    match &chamfer.distances {
        ChamferSize::Equal(d) => length_note(d, units),
        ChamferSize::Two(..) => {
            let [a, b] = two(chamfer, units);
            format!("{a} × {b}")
        }
        ChamferSize::Angle(d, a) => format!("{} {}", length_note(d, units), angle_note(a.value)),
    }
}

/// A two-distance chamfer's distances as numbers of `units`, in the
/// order its note shows them.
fn two(chamfer: &Chamfer, units: LengthUnit) -> [String; 2] {
    let ChamferSize::Two(a, b) = &chamfer.distances else {
        return [String::new(), String::new()];
    };
    let (a, b) = if chamfer.flip { (b, a) } else { (a, b) };
    [a, b].map(|value| varde_expr::format_number(value.value, Some(units.into())))
}

/// What the status bar says of a selected chamfer, as the UI mock's: "2
/// edges · Equal · 1 mm", "2 edges · Two distances · 1 × 2 mm", "1 edge ·
/// Distance and angle · 3 mm at 30° · Tangent chain".
pub fn chamfer_info(chamfer: &Chamfer, units: LengthUnit) -> String {
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
        ChamferSize::Two(..) => {
            let [a, b] = two(chamfer, units);
            format!("{a} × {b} {}", units.symbol())
        }
        ChamferSize::Equal(_) => chamfer_note(chamfer, units),
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
