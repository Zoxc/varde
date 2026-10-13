//! A sketch's summary in the status bar.

use varde_sketch::{Kind, Sketch};

use crate::words::counts;

/// What a sketch holds, for the status bar: "Empty", or how many of each
/// kind of curve, fillets and chamfers apart, and the points, "2 lines ·
/// 1 circle · 5 points".
pub(crate) fn sketch_summary(sketch: &Sketch) -> String {
    // Fillets are arcs and chamfers lines, with corners.
    let count = |kind: Kind, corner: bool| {
        sketch
            .curves
            .iter()
            .filter(|entry| entry.curve.kind() == kind && entry.corner.is_some() == corner)
            .count()
    };
    let parts = counts([
        (count(Kind::Line, false), "line", "lines"),
        (count(Kind::Circle, false), "circle", "circles"),
        (count(Kind::Arc, false), "arc", "arcs"),
        (count(Kind::Arc, true), "fillet", "fillets"),
        (count(Kind::Line, true), "chamfer", "chamfers"),
        (count(Kind::Spline, false), "spline", "splines"),
        (sketch.points.len(), "point", "points"),
    ]);
    if parts.is_empty() {
        "Empty".to_owned()
    } else {
        parts.join(" · ")
    }
}
