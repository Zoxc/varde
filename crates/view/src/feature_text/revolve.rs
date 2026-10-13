//! A revolve's notes in the Timeline and the status bar. Its panel is
//! in `revolve.rs`.

use varde_document::{AxisLine, Document, FeatureKind};

use crate::feature_text::angle_note;
use crate::operation_panel::OperationKind;
use crate::revolve::{axis_id, edge_axis_name};

/// How far a revolve turns in all, for its Timeline row: "360°", "90°",
/// "135°" for two sides of 90° and 45°.
pub(crate) fn turn_note(turn: &varde_document::Turn) -> String {
    use varde_document::Turn;
    let total = match turn {
        Turn::Full => std::f64::consts::TAU,
        Turn::OneSide(a) | Turn::Symmetric(a) => a.value,
        // Each side is checked to be at most a turn, so the sum is
        // finite.
        Turn::TwoSides(a, b) => a.value + b.value,
    };
    angle_note(total)
}

/// How far a revolve turns, for the status bar: "Full 360°", "One side
/// 90°", "Symmetric 90°", "Two sides 90° + 45°".
pub(crate) fn turn_info(turn: &varde_document::Turn) -> String {
    use varde_document::Turn;
    match turn {
        Turn::Full => format!("Full {}", angle_note(std::f64::consts::TAU)),
        Turn::OneSide(a) => format!("One side {}", angle_note(a.value)),
        Turn::Symmetric(a) => format!("Symmetric {}", angle_note(a.value)),
        Turn::TwoSides(a, b) => {
            format!(
                "Two sides {} + {}",
                angle_note(a.value),
                angle_note(b.value)
            )
        }
    }
}

/// A revolve's status bar info: its turn, operation and axis, "One side
/// 90° · Join · about Line 3" (the axis left out while its sketch doesn't
/// have it).
pub(crate) fn revolve_info(document: &Document, revolve: &varde_document::Revolve) -> String {
    let operation = OperationKind::of(&revolve.operation).label();
    let axis = match (
        revolve.axis,
        document.feature(revolve.sketch).map(|f| &f.kind),
    ) {
        (AxisLine::Edge(edge), _) => Some(edge_axis_name(document, &edge)),
        (axis, Some(FeatureKind::Sketch { sketch, .. })) => {
            axis_id(axis).and_then(|id| sketch.name(id))
        }
        _ => None,
    };
    let turn = turn_info(&revolve.extent);
    match axis {
        Some(axis) => format!("{turn} · {operation} · about {axis}"),
        None => format!("{turn} · {operation}"),
    }
}
