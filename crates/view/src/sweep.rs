//! The sweep feature's words in the Timeline and the status bar. Its
//! panel is the move's session's (`motion/sweep.rs`).

use varde_document::{Document, PathPart, PathRef, Sweep};

use crate::feature_text::angle_note;
use crate::operation_panel::OperationKind;

/// What the Timeline notes of a sweep, as the plan's rows: "along Sketch
/// 3", "along Body 1", "along 3 parts", "helix · 10 turns".
pub(crate) fn sweep_note(document: &Document, sweep: &Sweep) -> String {
    path_text(document, sweep, false)
}

/// The words for a sweep's path, "along Sketch 3" or "helix · 10
/// turns", their first capital if `capital`.
fn path_text(document: &Document, sweep: &Sweep, capital: bool) -> String {
    let (along, coil) = if capital {
        ("Along", "Helix")
    } else {
        ("along", "helix")
    };
    match &sweep.path {
        PathRef::Chain(parts) => match parts.as_slice() {
            [PathPart::Curves(chain)] => {
                let name = document
                    .feature(chain.sketch)
                    .map_or("a sketch", |feature| feature.name.as_str());
                format!("{along} {name}")
            }
            [PathPart::Edges { edges, .. }] => {
                let name = (edges.first())
                    .and_then(|edge| document.body(edge.body))
                    .map_or("a body", |body| body.name.as_str());
                format!("{along} {name}")
            }
            parts => format!("{along} {} parts", parts.len()),
        },
        PathRef::Helix(helix) => {
            let turns = helix.turns.value;
            let count = varde_expr::format(turns, None);
            if turns == 1.0 {
                format!("{coil} · 1 turn")
            } else {
                format!("{coil} · {count} turns")
            }
        }
    }
}

/// What the status bar says of a selected sweep: "Along Sketch 3 ·
/// Follow path · New body", "Along 2 parts · Keep orientation · Twist
/// 90° · Join", "Helix · 10 turns · Cut".
pub fn sweep_info(document: &Document, sweep: &Sweep) -> String {
    let mut parts = vec![path_text(document, sweep, true)];
    if !matches!(sweep.path, PathRef::Helix(_)) {
        parts.push(
            match sweep.orientation {
                varde_document::Orientation::FollowPath => "Follow path",
                varde_document::Orientation::Keep => "Keep orientation",
            }
            .to_owned(),
        );
    }
    if let Some(twist) = &sweep.twist {
        parts.push(format!("Twist {}", angle_note(twist.value)));
    }
    parts.push(OperationKind::of(&sweep.operation).label().to_owned());
    parts.join(" · ")
}

#[cfg(test)]
mod tests;
