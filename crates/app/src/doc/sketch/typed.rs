//! Typed values while drawing: `Tab` moving the value field through the
//! drawing tool's fields by the cursor, each value typed fixing what its
//! field measures of the shape (see [`varde_view::typed`]), and `Enter`
//! placing the shape. The values go into the shape's edit as its driving
//! dimensions, see [`edit`](super::edit).

use varde_view::typed::{self, Field, MAX_SIDES, MIN_SIDES};
use varde_view::{ToolClick, ValueTarget};

use super::ValueEdit;
use crate::doc::Doc;

/// The value field's text refused, and why: it stays open, saying so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Refused;

impl Doc {
    /// The drawing tool's field the value field is open on, if it is.
    pub(crate) fn drawing_field(&self) -> Option<Field> {
        match self.sketch.as_ref()?.value.as_ref()?.target {
            ValueTarget::Field(field) => Some(field),
            _ => None,
        }
    }

    /// Moves the value field to the drawing tool's next field, or its
    /// first, taking the value typed in the one it leaves (see
    /// [`Doc::take_field`]): one refused keeps it there. It opens with the
    /// value typed before in the field, as typed, else empty, showing what
    /// the field measures of the shape until something's typed.
    pub(crate) fn next_field(&mut self) {
        if self.editable_sketch().is_none() || self.take_field().is_err() {
            return;
        }
        let Some(drawing) = self.sketch.as_ref().and_then(|s| s.tool.as_ref()) else {
            return;
        };
        let fields = drawing.fields();
        let at = self
            .drawing_field()
            .and_then(|open| fields.iter().position(|&field| field == open));
        let next = match at {
            // Checked: the position is of one of them.
            Some(at) => fields.get((at + 1) % fields.len()),
            None => fields.first(),
        };
        let Some(&next) = next else {
            return;
        };
        let typed = drawing.active().value_in(next);
        let text = typed.map_or_else(String::new, |value| value.text.clone());
        self.open_value(ValueEdit {
            target: ValueTarget::Field(next),
            text,
            error: None,
            in_list: false,
        });
    }

    /// Takes the text of the value field, if it's open on one of the
    /// drawing tool's fields, as that field's value: nothing typed lets go
    /// of the value typed there before (the Polygon tool's sides stay as
    /// they are), anything else is read as what the field asks for in the
    /// design's units. Text that isn't such a value keeps the field open,
    /// saying why, the part it's about selected: [`Refused`].
    pub(super) fn take_field(&mut self) -> Result<(), Refused> {
        let Some(field) = self.drawing_field() else {
            return Ok(());
        };
        let design = self.editor.document().design();
        let Some(session) = &self.sketch else {
            return Ok(());
        };
        let (Some(drawing), Some(open)) = (&session.tool, &session.value) else {
            return Ok(());
        };
        let read = if open.text.trim().is_empty() {
            Ok(None)
        } else {
            typed::read(&drawing.active(), field, &open.text, &design).map(Some)
        };
        let value = match read {
            Ok(value) => value,
            Err(error) => {
                self.refuse_value(error);
                return Err(Refused);
            }
        };
        let Some(drawing) = self.sketch.as_mut().and_then(|s| s.tool.as_mut()) else {
            return Ok(());
        };
        match (field, value) {
            // Read as a whole number within these.
            (Field::Sides, Some(value)) => {
                drawing.sides = (value.value as u32).clamp(MIN_SIDES, MAX_SIDES);
            }
            (Field::Sides, None) => {}
            (_, value) => {
                // A chamfer's second distance and its angle each say how
                // it's cut: one typed lets go of the other.
                let other = match field {
                    Field::SecondDistance if value.is_some() => Some(Field::Angle),
                    Field::Angle if value.is_some() => Some(Field::SecondDistance),
                    _ => None,
                };
                drawing
                    .typed
                    .retain(|(typed, _)| *typed != field && Some(*typed) != other);
                drawing.typed.extend(value.map(|value| (field, value)));
            }
        }
        Ok(())
    }

    /// Places the shape the drawing tool is drawing where the cursor last
    /// was, as the values typed hold it: `Enter`, taking the value typed
    /// in the field open first. Only while the shape has fields, or with
    /// the Spline tool, ending its spline where it is.
    pub(crate) fn place_shape(&mut self) {
        if self.end_spline_here() {
            return;
        }
        let aim = self.sketch.as_ref().and_then(|session| {
            let drawing = session.tool.as_ref()?;
            (!drawing.fields().is_empty()).then_some(session.aim?)
        });
        if let Some(aim) = aim {
            self.tool_click(ToolClick {
                double: false,
                ..aim
            });
        }
    }

    /// Switches the Rectangle tool between drawing from a corner and from
    /// the centre, the point placed, if any, becoming the other.
    pub(crate) fn toggle_centered(&mut self) {
        let drawing = self.sketch.as_mut().and_then(|s| s.tool.as_mut());
        if let Some(drawing) = drawing.filter(|drawing| drawing.tool == varde_view::Tool::Rectangle)
        {
            drawing.centered = !drawing.centered;
        }
    }
}

#[cfg(test)]
mod tests;
