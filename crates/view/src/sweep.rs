//! The sweep feature's words in the Timeline and the status bar. Its
//! panel is the move's session's (`motion/sweep.rs`).

use varde_document::{Document, PathPart, PathRef, Sweep};

use crate::operation_panel::OperationKind;
use crate::panels::angle_note;

/// What the Timeline notes of a sweep, as the plan's rows: "along Sketch
/// 3", "along Body 1", "along 3 parts", "helix · 10 turns".
pub(crate) fn sweep_note(document: &Document, sweep: &Sweep) -> String {
    match &sweep.path {
        PathRef::Chain(parts) => match parts.as_slice() {
            [PathPart::Curves(chain)] => {
                let name = document
                    .feature(chain.sketch)
                    .map_or("a sketch", |feature| feature.name.as_str());
                format!("along {name}")
            }
            [PathPart::Edges { edges, .. }] => {
                let name = (edges.first())
                    .and_then(|edge| document.body(edge.body))
                    .map_or("a body", |body| body.name.as_str());
                format!("along {name}")
            }
            parts => format!("along {} parts", parts.len()),
        },
        PathRef::Helix(helix) => {
            let turns = helix.turns.value;
            let count = varde_expr::format(turns, None);
            if turns == 1.0 {
                "helix · 1 turn".to_owned()
            } else {
                format!("helix · {count} turns")
            }
        }
    }
}

/// What the status bar says of a selected sweep: "Along Sketch 3 ·
/// Follow path · New body", "Along 2 parts · Keep orientation · Twist
/// 90° · Join", "Helix · 10 turns · Cut".
pub fn sweep_info(document: &Document, sweep: &Sweep) -> String {
    let note = sweep_note(document, sweep);
    let mut note_chars = note.chars();
    let path = match note_chars.next() {
        Some(first) => first.to_uppercase().chain(note_chars).collect(),
        None => note.clone(),
    };
    let mut parts = vec![path];
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
