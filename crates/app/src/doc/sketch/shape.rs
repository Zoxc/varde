//! The shape tools in the sketch being edited: Trim takes away the piece
//! of a curve clicked, Extend lengthens a line or an arc clicked from the
//! end nearer the click, Offset copies the chain clicked to where the next
//! click is, or as far as typed, Mirror mirrors what it's picked about
//! the line clicked, and Fillet and Chamfer round or cut the corner
//! clicked as far as the next click, or as typed. Each change is a
//! [`SketchEdit`], proposed like any other (see
//! [`propose`](super::propose)), one undo step.

use varde_document::Sketch;
use varde_expr::Value;
use varde_sketch::{Id, Kind, SketchEdit};
use varde_view::typed::{self, Field};
use varde_view::{Tool, ToolClick};

use super::Drawing;
use crate::doc::Doc;

/// The chain the Offset tool offsets, from `ids` selected in `sketch`: the
/// curves among them if they're a chain, or the chain the one curve among
/// them is in ([`Sketch::chain_of`]). Empty for anything else.
///
/// [`Sketch::chain_of`]: varde_sketch::Sketch::chain_of
pub(super) fn offsettable(sketch: &Sketch, ids: &[Id]) -> Vec<Id> {
    let curves: Vec<Id> = ids
        .iter()
        .copied()
        .filter(|&id| sketch.curve(id).is_some())
        .collect();
    match curves[..] {
        [one] => sketch.chain_of(one),
        _ if sketch.is_chain(&curves) => curves,
        _ => Vec::new(),
    }
}

/// Whether what `drawing` has picked in `sketch` is still what its tool
/// picks: Offset's curves a chain, Fillet's and Chamfer's lines a corner
/// at its point. An undo can leave the items there but not so.
pub(super) fn still_picked(sketch: &Sketch, drawing: &Drawing) -> bool {
    if drawing.picked.is_empty() {
        return true;
    }
    match drawing.tool {
        Tool::Offset => sketch.is_chain(&drawing.picked),
        _ => match drawing.active().corner() {
            Some((at, [a, b])) => sketch.corner_of(a, b) == Some(at),
            None => !drawing.tool.corners(),
        },
    }
}

/// The corner Fillet and Chamfer start from, from `ids` selected in
/// `sketch`: two lines making one, as its point and the lines. Empty for
/// anything else.
pub(super) fn cornered(sketch: &Sketch, ids: &[Id]) -> Vec<Id> {
    match *ids {
        [a, b] => sketch
            .corner_of(a, b)
            .map(|at| vec![at, a, b])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The points and curves of `ids` Mirror can mirror, in `sketch`: not the
/// origin or axes, and neither constraints nor dimensions.
pub(super) fn mirrorable(sketch: &Sketch, ids: &[Id]) -> Vec<Id> {
    ids.iter()
        .copied()
        .filter(|&id| !id.is_builtin())
        .filter(|&id| {
            sketch
                .kind(id)
                .is_some_and(|kind| !matches!(kind, Kind::Constraint | Kind::Dimension))
        })
        .collect()
}

impl Doc {
    /// Takes `click` with a shape tool: on the curve under it for Trim
    /// and Extend, which change it at once; for Mirror picking what to
    /// mirror on what's under it (each click adding it or taking it out),
    /// or once it has, on the line to mirror about; for Offset, Fillet and
    /// Chamfer picking what's under it ([`Tool::pick`]), or once they have,
    /// placing what they make of it (see [`Doc::placed`]). A change made
    /// starts the tool afresh. A click on nothing does nothing.
    pub(super) fn shape_click(&mut self, click: ToolClick) {
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
        let placing = drawing.tool.places() && !drawing.picked.is_empty();
        let hit = click.hit.filter(|&id| sketch.kind(id).is_some());
        let edit = match (drawing.tool, hit) {
            _ if placing => self.placed(sketch, &drawing, click),
            (_, None) => None,
            (Tool::Trim, Some(hit)) => Some(SketchEdit::Trim {
                curve: hit,
                near: click.at,
            }),
            (Tool::Extend, Some(hit)) => sketch
                .nearer_end(hit, click.at)
                .map(|end| SketchEdit::Extend { curve: hit, end }),
            (Tool::Mirror, Some(hit)) if drawing.about => Some(SketchEdit::Mirror {
                ids: drawing.picked.clone(),
                about: hit,
            }),
            (Tool::Mirror, Some(hit)) => {
                if !mirrorable(sketch, &[hit]).is_empty() {
                    match drawing.picked.iter().position(|&id| id == hit) {
                        Some(index) => {
                            drawing.picked.remove(index);
                        }
                        None => drawing.picked.push(hit),
                    }
                    self.set_drawing(drawing);
                }
                return;
            }
            (tool, Some(hit)) => {
                let picked = tool.pick(sketch, hit, click.at);
                if !picked.is_empty() {
                    drawing.picked = picked;
                    self.set_drawing(drawing);
                }
                return;
            }
        };
        let Some(edit) = edit else {
            return;
        };
        if self.propose(edit) {
            drawing.restart();
            self.set_drawing(drawing);
        }
    }

    /// What Offset, Fillet or Chamfer, `drawing`, makes of what it's
    /// picked in `sketch` with `click`: the chain's copy on the side of it
    /// the click is, as far off as the click or the distance typed; the
    /// fillet or chamfer on the corner through the click (see
    /// [`typed::corner_outline`]), or as the values typed make it, a
    /// chamfer's first distance along the line picked first. `None` where
    /// the click is less than a pixel off, no size meant.
    fn placed(&self, sketch: &Sketch, drawing: &Drawing, click: ToolClick) -> Option<SketchEdit> {
        // The value typed in `field`, or else `reach`, the click's, as the
        // design's units show it: none where that's less than a pixel, or
        // no value the field can take.
        let typed_or = |field: Field, reach: f64| {
            if let Some(value) = drawing.active().value_in(field) {
                return Some(value.clone());
            }
            if reach < click.pixel {
                return None;
            }
            let ask = field.ask(&self.editor.document().design());
            Value::new(&varde_expr::format(reach, ask.unit()), &ask).ok()
        };
        if drawing.tool == Tool::Offset {
            let (reach, side) = sketch.offset_side(&drawing.picked, click.at)?;
            return Some(SketchEdit::Offset {
                chain: drawing.picked.clone(),
                distance: typed_or(Field::Distance, reach)?,
                side,
            });
        }
        let (at, lines) = drawing.active().corner()?;
        Some(if drawing.tool == Tool::Fillet {
            let reach = sketch.fillet_through(at, lines, click.at).unwrap_or(0.0);
            let radius = typed_or(Field::Radius, reach)?;
            SketchEdit::Fillet { at, lines, radius }
        } else {
            let reach = sketch.chamfer_through(at, lines, click.at).unwrap_or(0.0);
            let first = typed_or(Field::Distance, reach)?;
            let setback = typed::setback(&drawing.active(), first, Value::clone);
            SketchEdit::Chamfer { at, lines, setback }
        })
    }

    /// Ends the Mirror tool's picking, once it has picked something: its
    /// next click is on the line to mirror about.
    pub(crate) fn mirror_about(&mut self) {
        let drawing = self.sketch.as_mut().and_then(|s| s.tool.as_mut());
        if let Some(drawing) = drawing.filter(|drawing| drawing.tool == Tool::Mirror)
            && !drawing.picked.is_empty()
        {
            drawing.about = true;
        }
    }
}

#[cfg(test)]
mod tests;
