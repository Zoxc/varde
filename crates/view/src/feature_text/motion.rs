//! The notes of a move, mirror, pattern, align, scale and split in the
//! Timeline and the status bar. Their panels are in `motion.rs`.

use varde_document::{Document, Pattern};
use varde_expr::{AngleUnit, LengthUnit, Unit};

use crate::motion::{axis_name, body_names, pattern_copies, plane_name, plane_short};
use crate::plane_pick::face_name;

const DEGREES: Unit = Unit::Angle(AngleUnit::Deg);

/// A move's Timeline note, the mock's: how far it shifts in all and its
/// turn, "82.5 mm 30°", either left out when it's none; "0 mm" for a
/// move that does nothing.
pub(crate) fn move_note(moved: &varde_document::Move, units: LengthUnit) -> String {
    let shift = moved.offset_vector().length();
    let angle = moved.turn.as_ref().map(|(_, angle)| angle.value);
    let mut parts = Vec::new();
    if shift != 0.0 || angle.is_none_or(|angle| angle == 0.0) {
        parts.push(varde_expr::Shown::current().format(shift, Some(units.into())));
    }
    if let Some(angle) = angle.filter(|&angle| angle != 0.0) {
        parts.push(varde_expr::Shown::current().format(angle, Some(DEGREES)));
    }
    parts.join(" ")
}

/// What the status bar says of a selected move, the mock's: "Body 1 by
/// 10, 0, -5 mm, 30° about Z axis".
pub(crate) fn move_info(document: &Document, moved: &varde_document::Move) -> String {
    let units = document.units();
    let bodies = body_names(document, &moved.bodies);
    let mut parts = Vec::new();
    if moved.offset.iter().any(|value| value.value != 0.0) {
        let symbol = format!(" {}", Unit::from(units).symbol());
        let [x, y, z] = moved.offset.each_ref().map(|value| {
            let shown = varde_expr::Shown::current().format(value.value, Some(units.into()));
            shown
                .strip_suffix(&symbol)
                .map(str::to_owned)
                .unwrap_or(shown)
        });
        parts.push(format!("by {x}, {y}, {z}{symbol}"));
    }
    if let Some((axis, angle)) = &moved.turn
        && angle.value != 0.0
    {
        let angle = varde_expr::Shown::current().format(angle.value, Some(DEGREES));
        parts.push(format!("{angle} about {}", axis_name(document, axis)));
    }
    if parts.is_empty() {
        format!("{bodies} not moved")
    } else {
        format!("{bodies} {}", parts.join(", "))
    }
}

/// What the status bar says of a selected mirror, the mock's: "Body 1
/// across XY plane · copy".
pub(crate) fn mirror_info(document: &Document, mirror: &varde_document::Mirror) -> String {
    let bodies = body_names(document, &mirror.bodies);
    let copy = if mirror.keep_original { " · copy" } else { "" };
    format!(
        "{bodies} across {}{copy}",
        plane_name(document, &mirror.plane)
    )
}

/// What the status bar says of a selected pattern, the mock's row info:
/// "Body 1 · 4 × 25 mm along X axis · joined", "Body 1 · 3 × 25 mm along
/// X axis, flipped", "Body 1 · 6 × 60° about Z axis".
pub(crate) fn pattern_info(document: &Document, pattern: &Pattern) -> String {
    let bodies = body_names(document, &pattern.bodies);
    format!("{bodies} · {}", pattern_copies(document, pattern))
}

/// A pattern's Timeline note, the mock's: its count, "×4".
pub(crate) fn pattern_note(pattern: &Pattern) -> String {
    format!("×{}", pattern.kind.count_value().value)
}

/// What an align's target is on: the body its point or, failing that,
/// a direction is on, or "the origin". The Timeline's note, "to Body 2".
pub(crate) fn align_note(document: &Document, align: &varde_document::Align) -> String {
    let to = &align.to;
    let body = (to.point.body()).or_else(|| to.directions().find_map(|direction| direction.body()));
    match body {
        Some(body) => format!("to {}", body_names(document, &[body])),
        None => "to the origin".to_owned(),
    }
}

/// What the status bar says of a selected align: "Body 1 to Body 2".
pub fn align_info(document: &Document, align: &varde_document::Align) -> String {
    let moved = body_names(document, &[align.body]);
    format!("{moved} {}", align_note(document, align))
}

/// A scale's Timeline note: "×2", "×1 · 1 · 2", "edge → 50 mm"
/// (lengths in `units`), the factor written as a pattern's count is.
pub(crate) fn scale_note(scale: &varde_document::Scale, units: LengthUnit) -> String {
    use varde_document::ScaleFactor;
    let factor = |value: &varde_expr::Value| varde_expr::format(value.value, None);
    match &scale.factor {
        ScaleFactor::Uniform(value) => format!("×{}", factor(value)),
        ScaleFactor::PerAxis(values) => format!("×{}", values.each_ref().map(factor).join(" · ")),
        ScaleFactor::EdgeLength { length, .. } => format!(
            "edge → {}",
            varde_expr::Shown::current().format(length.value, Some(units.into()))
        ),
    }
}

/// What the status bar says of a selected scale: "Body 1 ×2".
pub fn scale_info(document: &Document, scale: &varde_document::Scale, units: LengthUnit) -> String {
    let bodies = body_names(document, &scale.bodies);
    format!("{bodies} {}", scale_note(scale, units))
}

/// A split's Timeline note: what it splits with, "by XY", "by Body 3",
/// "by a face", "by Sketch 2", and the side kept where it keeps one
/// ("by XY · front only").
pub(crate) fn split_note(document: &Document, split: &varde_document::Split) -> String {
    use varde_document::{Keep, SplitTool};
    let tool = match &split.tool {
        SplitTool::Plane(plane) => plane_short(document, plane),
        SplitTool::Face(face) => face_name(document, face),
        SplitTool::Body(body) => body_names(document, std::slice::from_ref(body)),
        SplitTool::Regions { sketch, .. } | SplitTool::Chain { sketch, .. } => (document
            .feature(*sketch))
        .map_or_else(|| "a sketch".to_owned(), |feature| feature.name.clone()),
    };
    match split.keep {
        Keep::Both => format!("by {tool}"),
        Keep::Front => format!("by {tool} · front only"),
        Keep::Back => format!("by {tool} · back only"),
    }
}

/// What the status bar says of a selected split: "Body 1 by XY".
pub fn split_info(document: &Document, split: &varde_document::Split) -> String {
    let body = body_names(document, std::slice::from_ref(&split.body));
    format!("{body} {}", split_note(document, split))
}

#[cfg(test)]
mod tests;
