//! Dimensions in the sketch being edited: the Dimension tool picking what
//! to measure and placing the dimension, the value field placing it with
//! a value or changing one's value in place, dragging labels, and
//! turning dimensions between driving and reference. Each change is a
//! [`SketchEdit`], proposed like any other (see [`propose`](super::propose)).

use glam::DVec2;
use varde_expr::Value;
use varde_sketch::{Add, Dimension, EditError, Id, SketchEdit};
use varde_view::{ToolClick, ValueTarget, dimension};

use super::{LabelDrag, ValueEdit};
use crate::doc::Doc;

/// What the value field is to do as it shows, once the view has it: take
/// the focus, with all its text selected to overtype, or the part of it a
/// refusal is about, in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    All,
    Range(usize, usize),
}

impl Doc {
    /// Takes `click` with the Dimension tool: picks what it's on, to
    /// measure, or places the dimension of what's picked with its label
    /// there. Two items picked, or a click on one picked or on nothing,
    /// places it; an item that measures something with the one picked
    /// joins it, and any other starts afresh from it. Placing opens the
    /// value field with what it measures now, once the solver takes it as
    /// driving (it's placed as a reference if not), or with the reference
    /// modifier held adds it as a reference at once.
    pub(super) fn dimension_click(&mut self, click: ToolClick) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(mut drawing) = self
            .sketch
            .as_ref()
            .and_then(|session| session.tool.clone())
        else {
            return;
        };
        let hit = click.hit.filter(|&id| dimension::pickable(sketch, id));
        let picked = &drawing.picked;
        if let Some(id) = hit
            && picked.len() < 2
            && !picked.contains(&id)
        {
            if picked.is_empty() || dimension::joins(sketch, picked, id) {
                drawing.picked.push(id);
            } else {
                drawing.picked = vec![id];
            }
            drawing.switched = false;
            self.set_drawing(drawing);
            return;
        }
        let Some((measure, side)) = dimension::measure(sketch, picked, click.at, drawing.switched)
        else {
            return;
        };
        let Some(anchor) = sketch.anchor(&measure) else {
            return;
        };
        let label = click.at - anchor;
        let design = self.editor.document().design();
        let text = dimension::measured_text(sketch, &measure, side, design.units);
        // Points at one place, or a reference of what no dimension can
        // be, are refused saying why, and picked afresh.
        let held = if click.reference {
            sketch.held(&measure, side, &design).map(Some)
        } else if sketch.same_place(&measure) {
            Err(EditError::SamePlace)
        } else {
            Ok(None)
        };
        let held = match held {
            Ok(held) => held,
            Err(why) => {
                self.refuse(why);
                drawing.restart();
                self.set_drawing(drawing);
                return;
            }
        };
        if let Some((value, side)) = held {
            let mut add = Add::new(sketch);
            add.dimensions.push(Dimension {
                measure,
                value,
                driving: false,
                label,
                side,
            });
            if self.propose(SketchEdit::Add(add)) {
                drawing.restart();
                self.set_drawing(drawing);
            }
            return;
        }
        // It's proposed driving, holding what it measures, before the
        // field opens, so one that would over-constrain the sketch is
        // placed as a reference at once rather than after its value is
        // typed.
        let probe = sketch
            .held(&measure, side, &design)
            .ok()
            .map(|(value, side)| {
                let mut add = Add::new(sketch);
                add.dimensions.push(Dimension {
                    measure: measure.clone(),
                    value,
                    driving: true,
                    label,
                    side,
                });
                add
            });
        drawing.restart();
        self.set_drawing(drawing);
        let field = ValueEdit {
            target: ValueTarget::New {
                measure,
                side,
                label,
            },
            text,
            error: None,
            in_list: false,
        };
        match probe {
            Some(add) => {
                self.probe(SketchEdit::Add(add), field);
            }
            None => self.open_value(field),
        }
    }

    /// Opens the value field on the dimension `id`, if it's driving and
    /// the sketch can be changed, with its expression as typed: in the
    /// Constraints list if `in_list`, else at its label.
    pub(crate) fn edit_dimension(&mut self, id: Id, in_list: bool) {
        let Some(entry) = self
            .editable_sketch()
            .and_then(|sketch| sketch.dimension(id))
        else {
            return;
        };
        if !entry.dimension.driving {
            return;
        }
        let text = entry.dimension.value.text.clone();
        self.open_value(ValueEdit {
            target: ValueTarget::Dimension(id),
            text,
            error: None,
            in_list,
        });
    }

    /// Opens `field`, in place of any open, to take the focus with its
    /// text selected.
    pub(super) fn open_value(&mut self, field: ValueEdit) {
        if let Some(session) = &mut self.sketch {
            session.label = None;
            session.value = Some(field);
            self.focus = Some(Focus::All);
        }
    }

    /// Keeps `text` as typed in the value field, if it's open, and stops
    /// showing why the last was refused.
    pub(crate) fn value_input(&mut self, text: String) {
        if let Some(field) = self.sketch.as_mut().and_then(|s| s.value.as_mut()) {
            field.text = text;
            field.error = None;
        }
    }

    /// Closes the value field, if it's open, changing nothing.
    pub(crate) fn close_value(&mut self) {
        if let Some(session) = &mut self.sketch {
            session.value = None;
        }
    }

    /// Takes the text of the value field, read as the value its dimension
    /// asks for in the design's units: places the dimension being placed
    /// with it, driving, or sets the one edited to it, and closes the
    /// field. Text that isn't such a value keeps the field open, saying
    /// why, the part it's about selected. Open on a drawing tool's field,
    /// takes its value and places the shape, see [`Doc::place_shape`].
    pub(crate) fn submit_value(&mut self) {
        if self.drawing_field().is_some() {
            if self.editable_sketch().is_some() && self.take_field().is_ok() {
                self.close_value();
                self.place_shape();
            }
            return;
        }
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(field) = self.sketch.as_ref().and_then(|s| s.value.as_ref()) else {
            return;
        };
        let design = self.editor.document().design();
        let edit = match &field.target {
            ValueTarget::New {
                measure,
                side,
                label,
            } => Value::new(&field.text, &measure.ask(&design)).map(|value| {
                let mut add = Add::new(sketch);
                add.dimensions.push(Dimension {
                    measure: measure.clone(),
                    value,
                    driving: true,
                    label: *label,
                    side: *side,
                });
                Some(SketchEdit::Add(add))
            }),
            ValueTarget::Dimension(id) => {
                let Some(entry) = sketch.dimension(*id) else {
                    self.close_value();
                    return;
                };
                let ask = entry.dimension.measure.ask(&design);
                Value::new(&field.text, &ask).map(|value| {
                    // Unchanged, there's nothing to do.
                    (value != entry.dimension.value)
                        .then_some(SketchEdit::SetDimension { id: *id, value })
                })
            }
            // Taken above.
            ValueTarget::Field(_) => return,
        };
        match edit {
            Ok(edit) => {
                self.close_value();
                if let Some(edit) = edit {
                    self.propose(edit);
                }
            }
            Err(error) => self.refuse_value(error),
        }
    }

    /// Keeps the value field open saying why its text was refused,
    /// `error`, with the part it's about selected.
    pub(super) fn refuse_value(&mut self, error: varde_expr::Error) {
        let Some(field) = self.sketch.as_mut().and_then(|s| s.value.as_mut()) else {
            return;
        };
        let text = &field.text;
        let chars = |at: usize| text.get(..at).map_or(0, |part| part.chars().count());
        self.focus = Some(Focus::Range(chars(error.span.start), chars(error.span.end)));
        field.error = Some(error);
    }

    /// Takes a press on the label of the dimension `id`: selects it alone,
    /// or with `add` adds it to the selection or takes it out, and grabs
    /// it to drag if the sketch can be changed.
    pub(crate) fn press_label(&mut self, id: Id, add: bool) {
        self.click_geometry(Some(id.into()), add);
        let editable = self
            .editable_sketch()
            .is_some_and(|s| s.dimension(id).is_some());
        if let Some(session) = &mut self.sketch {
            session.label = editable.then_some(LabelDrag {
                id,
                by: DVec2::ZERO,
            });
        }
    }

    /// Drags the label grabbed, of the dimension `id`, grabbed at `from`,
    /// to `to`: shown there until it's dropped.
    pub(crate) fn drag_label(&mut self, id: Id, from: DVec2, to: DVec2) {
        let by = to - from;
        let label = self.sketch.as_mut().and_then(|s| s.label.as_mut());
        if let Some(label) = label.filter(|label| label.id == id)
            && by.is_finite()
        {
            label.by = by;
        }
    }

    /// Lets go of the label grabbed, proposing it where it was dragged to,
    /// if it was.
    pub(crate) fn drop_label(&mut self) {
        let Some(LabelDrag { id, by }) = self.sketch.as_mut().and_then(|s| s.label.take()) else {
            return;
        };
        let Some(entry) = self
            .editable_sketch()
            .and_then(|sketch| sketch.dimension(id))
        else {
            return;
        };
        if by != DVec2::ZERO {
            let label = entry.dimension.label + by;
            self.propose(SketchEdit::MoveLabel { id, label });
        }
    }

    /// Turns the dimensions selected between driving and reference: all
    /// references unless they all are, then all driving. Each is an edit
    /// of its own, made driving holding what it measures now.
    pub(crate) fn toggle_reference(&mut self) {
        let (Some(sketch), Some(session)) = (self.editable_sketch(), &self.sketch) else {
            return;
        };
        let selected: Vec<_> = session
            .selection
            .iter()
            .filter_map(|&target| sketch.dimension(target.item()?))
            .collect();
        let driving = !selected.iter().any(|entry| entry.dimension.driving);
        let edits: Vec<_> = selected
            .iter()
            .filter(|entry| entry.dimension.driving != driving)
            .map(|entry| SketchEdit::SetDriving {
                id: entry.id,
                driving,
            })
            .collect();
        for edit in edits {
            self.propose(edit);
        }
    }

    /// Switches the Dimension tool between the radius and the diameter of
    /// the circle or arc picked.
    pub(crate) fn switch_round(&mut self) {
        let drawing = self.sketch.as_mut().and_then(|s| s.tool.as_mut());
        if let Some(drawing) = drawing.filter(|drawing| !drawing.picked.is_empty()) {
            drawing.switched = !drawing.switched;
        }
    }

    /// What the value field is to do as it shows, once: see [`Focus`].
    pub(crate) fn take_focus(&mut self) -> Option<Focus> {
        self.focus.take()
    }
}

#[cfg(test)]
mod tests;
