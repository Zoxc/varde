//! A chamfer's notes in the Timeline and the status bar. Its panel is
//! in `motion/chamfer.rs`, its edges' field in `motion/blend.rs`.

use varde_document::{Chamfer, ChamferSize, LengthUnit};
use varde_expr::Value;

use crate::feature_text::{angle_note, blend_count, chain_note, length_note};

/// A chamfer's Timeline note, as the UI mock's row: its size, "1 mm",
/// "1 × 2" (along the face of the edges' first key first, so Flip sides
/// swaps them), "3 mm 30°".
pub(crate) fn chamfer_note(chamfer: &Chamfer, units: LengthUnit) -> String {
    match &chamfer.distances {
        ChamferSize::Equal(d) => length_note(d, units),
        ChamferSize::Two(a, b) => {
            let [a, b] = two(a, b, chamfer.flip, units);
            format!("{a} × {b}")
        }
        ChamferSize::Angle(d, a) => format!("{} {}", length_note(d, units), angle_note(a.value)),
    }
}

/// A two-distance chamfer's distances `a` and `b` as numbers of
/// `units`, in the order its note shows them: swapped by `flip`.
fn two(a: &Value, b: &Value, flip: bool, units: LengthUnit) -> [String; 2] {
    let (a, b) = if flip { (b, a) } else { (a, b) };
    [a, b].map(|value| varde_expr::Shown::current().format_number(value.value, Some(units.into())))
}

/// What the status bar says of a selected chamfer, as the UI mock's: "2
/// edges · Equal · 1 mm", "2 edges · Two distances · 1 × 2 mm", "1 edge ·
/// Distance and angle · 3 mm at 30° · Tangent chain".
pub fn chamfer_info(chamfer: &Chamfer, units: LengthUnit) -> String {
    let edges = blend_count(&chamfer.edges, &chamfer.faces);
    let size = match &chamfer.distances {
        ChamferSize::Angle(d, a) => {
            format!("{} at {}", length_note(d, units), angle_note(a.value))
        }
        ChamferSize::Two(a, b) => {
            let [a, b] = two(a, b, chamfer.flip, units);
            format!("{a} × {b} {}", units.symbol())
        }
        ChamferSize::Equal(_) => chamfer_note(chamfer, units),
    };
    let chain = chain_note(chamfer.chains);
    format!("{edges} · {} · {size}{chain}", chamfer.distances.name())
}

#[cfg(test)]
mod tests;
