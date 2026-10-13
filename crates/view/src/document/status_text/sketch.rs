//! The status bar's hints in a sketch: for the value field, each kind of
//! tool, the Constrain tool, adding points to splines and selecting.

use glam::DVec2;
use varde_sketch::{Kind, Measure, Sketch};

use crate::chrome::{Hint, chord_hint, double_hint, key_hint, mouse_hint, step_hint};
use crate::document::{ActiveTool, SketchState, ValueField, ValueTarget};
use crate::icons::MouseButton;
use crate::shortcut::{Held, Shortcut};
use crate::{ConstraintKind, Tool};

use super::hint_list;

/// The status bar's hints for the left button and the keys in `sketch`,
/// which can be changed if `editable`: those of the value field, else of
/// the tool, else of the Constrain tool, else of adding points to
/// splines, else of selecting; checked in that order.
pub(super) fn sketch_hints<'a>(
    sketch: &SketchState<'a>,
    editable: bool,
    snap: bool,
) -> Vec<Hint<'a>> {
    if let Some(field) = sketch.value {
        return value_hints(field);
    }
    if let Some(tool) = sketch.tool {
        return if tool.tool == Tool::Dimension {
            dimension_hints(sketch.sketch, &tool)
        } else if tool.tool.draws() {
            drawing_tool_hints(sketch.sketch, &tool, snap)
        } else {
            picking_tool_hints(sketch.sketch, &tool)
        };
    }
    if sketch.constraining {
        return constrain_hints(sketch);
    }
    if sketch.inserting && editable {
        return vec![
            step_hint(MouseButton::Left, "Click a spline to add a point"),
            key_hint(Shortcut::ESCAPE, "Stop adding points"),
        ];
    }
    selection_hints(sketch, editable)
}

/// The value field's hints: placing or setting what it's typed for,
/// moving to the next value where there's one, cancelling.
fn value_hints<'a>(field: ValueField<'a>) -> Vec<Hint<'a>> {
    let enter = match field.target {
        ValueTarget::New { .. } | ValueTarget::Field(_) => "Place",
        ValueTarget::Dimension(_) => "Set",
    };
    let next = matches!(field.target, ValueTarget::Field(_))
        .then(|| key_hint(Shortcut::NEXT_FIELD, "Next value"));
    hint_list([
        Some(key_hint(Shortcut::ENTER, enter)),
        next,
        Some(key_hint(Shortcut::ESCAPE, "Cancel")),
    ])
}

/// The hints of a tool that picks rather than draws (Trim, Mirror,
/// Offset, Fillet, ...): what it asks for, finishing or typing its value
/// once something's picked, and cancelling or stopping it.
fn picking_tool_hints<'a>(sketch: &Sketch, tool: &ActiveTool<'a>) -> Vec<Hint<'a>> {
    let picked = !tool.picked.is_empty();
    let done = match tool.tool {
        Tool::Mirror => (picked && !tool.about).then_some((Shortcut::ENTER, "Pick the line")),
        Tool::Offset | Tool::Fillet | Tool::Chamfer if !tool.typed.is_empty() => {
            Some((Shortcut::ENTER, "Place"))
        }
        Tool::Offset => picked.then_some((Shortcut::NEXT_FIELD, "Type distance")),
        Tool::Fillet => picked.then_some((Shortcut::NEXT_FIELD, "Type radius")),
        Tool::Chamfer => picked.then_some((Shortcut::NEXT_FIELD, "Type distances")),
        _ => None,
    };
    let escape = if picked { "Cancel" } else { "Stop tool" };
    hint_list([
        Some(step_hint(MouseButton::Left, tool.step(sketch))),
        done.map(|(key, label)| key_hint(key, label)),
        Some(key_hint(Shortcut::ESCAPE, escape)),
    ])
}

/// The hints of a drawing tool: what it asks for, placing without
/// snapping (or with it, if tools don't otherwise `snap`), typing values,
/// its own keys, construction, and how to stop it.
fn drawing_tool_hints<'a>(sketch: &Sketch, tool: &ActiveTool<'a>, snap: bool) -> Vec<Hint<'a>> {
    let escape = match (tool.tool, tool.placed.is_empty()) {
        (_, true) => "Stop tool",
        (Tool::Line, false) => "End line",
        (_, false) => "Cancel",
    };
    let end = tool
        .spline_ends()
        .then(|| key_hint(Shortcut::ENTER, "End spline"));
    let close = tool.can_close().then(|| key_hint(Shortcut::CLOSE, "Close"));
    let kind = (tool.tool == Tool::Spline).then(|| {
        let label = if tool.control {
            "Through points"
        } else {
            "Control points"
        };
        key_hint(Shortcut::SPLINE_KIND, label)
    });
    let fields = !tool.fields().is_empty();
    let values = fields.then(|| key_hint(Shortcut::NEXT_FIELD, "Type values"));
    let place = (fields && !tool.typed.is_empty()).then(|| key_hint(Shortcut::ENTER, "Place"));
    let centered = (tool.tool == Tool::Rectangle).then(|| {
        let label = if tool.centered {
            "From corner"
        } else {
            "From center"
        };
        key_hint(Shortcut::CENTERED, label)
    });
    hint_list([
        Some(step_hint(MouseButton::Left, tool.step(sketch))),
        Some(chord_hint(
            Held::FREE,
            MouseButton::Left,
            if snap { "Don't snap" } else { "Snap" },
        )),
        values,
        place,
        centered,
        end,
        close,
        kind,
        Some(key_hint(Shortcut::CONSTRUCTION, "Construction")),
        Some(key_hint(Shortcut::ESCAPE, escape)),
    ])
}

/// The Constrain tool's hints: selecting, the constraint most likely
/// for what's selected, and stopping it.
fn constrain_hints<'a>(sketch: &SketchState<'a>) -> Vec<Hint<'a>> {
    let most_likely = ConstraintKind::fitting(sketch.sketch, sketch.selection)
        .first()
        .and_then(|kind| Some(key_hint(kind.shortcut()?, kind.label())));
    let select = if sketch.selection.is_empty() {
        "Select geometry to constrain"
    } else {
        "Select"
    };
    hint_list([
        Some(step_hint(MouseButton::Left, select)),
        most_likely,
        Some(key_hint(Shortcut::ESCAPE, "Stop tool")),
    ])
}

/// The hints of selecting in `sketch`, which can be changed if
/// `editable`: adding a point on a spline, what can be done with the
/// selection, and leaving.
fn selection_hints<'a>(sketch: &SketchState<'a>, editable: bool) -> Vec<Hint<'a>> {
    // Adding a point by a double-click, while there's a spline to add to.
    let insert = (editable
        && (sketch.sketch.curves.iter()).any(|entry| entry.curve.kind() == Kind::Spline))
    .then(|| double_hint(MouseButton::Left, "Add point on spline"));
    let selection = (!sketch.selection.is_empty()).then(|| {
        let edits = editable.then(|| {
            [
                key_hint(Shortcut::DELETE, "Delete"),
                key_hint(Shortcut::CONSTRUCTION, "Construction"),
            ]
        });
        let reference = editable
            .then(|| reference_hint(sketch))
            .flatten()
            .map(|label| key_hint(Shortcut::REFERENCE, label));
        // Splines switched, given handles, and their curvature shown.
        let splines = crate::spline::any_selected(sketch.sketch, sketch.selection);
        let convert = (editable && splines).then(|| key_hint(Shortcut::SPLINE_KIND, "Convert"));
        let handles = crate::spline::handles(sketch.sketch, sketch.selection)
            .filter(|_| editable)
            .map(|edit| {
                let label = match edit {
                    varde_sketch::SketchEdit::Delete(_) => "No handles",
                    _ => "Handles",
                };
                key_hint(Shortcut::HANDLES, label)
            });
        let comb = splines.then(|| {
            let label = if sketch.comb { "Hide comb" } else { "Comb" };
            key_hint(Shortcut::COMB, label)
        });
        edits
            .into_iter()
            .flatten()
            .chain(reference)
            .chain(convert)
            .chain(handles)
            .chain(comb)
    });
    std::iter::once(mouse_hint(MouseButton::Left, "Select"))
        .chain(insert)
        .chain(selection.into_iter().flatten())
        .chain([key_hint(Shortcut::ESCAPE, "Finish")])
        .collect()
}

/// The status bar's hints in the Dimension tool: what it asks for, the
/// radius or diameter to switch to, placing a reference and stopping.
fn dimension_hints<'a>(sketch: &Sketch, tool: &ActiveTool<'a>) -> Vec<Hint<'a>> {
    let placing = crate::dimension::measure(sketch, tool.picked, DVec2::ZERO, tool.switched);
    let switch = match placing {
        Some((Measure::Radius(_), _)) => Some("Diameter"),
        Some((Measure::Diameter(_), _)) => Some("Radius"),
        _ => None,
    };
    let escape = if tool.picked.is_empty() {
        "Stop tool"
    } else {
        "Cancel"
    };
    hint_list([
        Some(step_hint(MouseButton::Left, tool.step(sketch))),
        switch.map(|label| key_hint(Shortcut::SWITCH_ROUND, label)),
        placing
            .is_some()
            .then(|| chord_hint(Held::REFERENCE, MouseButton::Left, "Reference")),
        Some(key_hint(Shortcut::ESCAPE, escape)),
    ])
}

/// What the reference key makes of the dimensions selected, if any are:
/// references, unless they all are, then driving.
fn reference_hint(sketch: &SketchState<'_>) -> Option<&'static str> {
    let mut selected = sketch
        .selection
        .iter()
        .filter_map(|&target| sketch.sketch.dimension(target.item()?))
        .peekable();
    selected.peek()?;
    Some(if selected.any(|entry| entry.dimension.driving) {
        "Reference"
    } else {
        "Driving"
    })
}
