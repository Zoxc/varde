//! Splines in the sketch being edited: the Spline tool, which places the
//! points clicked until a double-click, `Enter` or a click on its first
//! point ends the spline, switching between through fit points and by
//! control points as it goes; and what's done to splines selected:
//! switching their kind, handles, a point added on one, and the
//! curvature comb. Each change is a [`SketchEdit`], proposed like any
//! other (see [`propose`](super::propose)), one undo step.

use std::collections::BTreeSet;

use varde_sketch::{
    Add, Constraint, Curve, Handle, Id, MAX_SPLINE_POINTS, OutOfIds, Selectable, Sketch,
    SketchEdit, Spline, SplineKind, control_knots, handle_tips,
};
use varde_view::{Own, Target, Tool, ToolClick};

use super::Drawing;
use super::edit::place;
use crate::doc::Doc;

impl Doc {
    /// Takes `click` with the Spline tool, `drawing`: on its first point,
    /// once it has enough to close, ends the spline closed; a
    /// double-click, once it has enough, ends it open where it is; else
    /// the click places its next point. A point within a pixel of the
    /// last, on its first before it can close, on a point of the sketch's
    /// it has already, or past [`MAX_SPLINE_POINTS`], is refused.
    pub(super) fn spline_click(&mut self, mut drawing: Drawing, click: ToolClick) {
        let active = drawing.active();
        if active.closes(click.at, click.pixel) {
            self.end_spline(drawing, true);
            return;
        }
        if click.double {
            if active.spline_ends() {
                self.end_spline(drawing, false);
            }
            return;
        }
        let near_last = drawing
            .placed
            .last()
            .is_some_and(|last| last.distance(click.at) < click.pixel);
        let again = matches!(click.target, Some(Target::Point(id))
            if !id.is_builtin() && drawing.targets.contains(&Some(Target::Point(id))));
        // On the first point with too few to close.
        let first = active.on_first(click.at, click.pixel);
        if near_last || again || first || drawing.placed.len() >= MAX_SPLINE_POINTS {
            return;
        }
        drawing.placed.push(click.at);
        drawing.targets.push(click.target);
        self.set_drawing(drawing);
    }

    /// Ends the spline the Spline tool is drawing where it is, open:
    /// `Enter`. Whether the Spline tool is in use, whatever it made of
    /// it.
    pub(super) fn end_spline_here(&mut self) -> bool {
        let drawing = self
            .sketch
            .as_ref()
            .and_then(|session| session.tool.clone());
        match drawing {
            Some(drawing) if drawing.tool == Tool::Spline => {
                if drawing.active().spline_ends() {
                    self.end_spline(drawing, false);
                }
                true
            }
            _ => false,
        }
    }

    /// Proposes the spline `drawing` has placed, open or `closed`, of the
    /// kind it draws, from its points placed where they snapped (see
    /// [`place`]), and starts it afresh. Nothing if it has too few for
    /// its kind.
    fn end_spline(&mut self, mut drawing: Drawing, closed: bool) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let kind = drawing.active().spline_kind();
        if drawing.placed.len() < kind.least(closed) {
            return;
        }
        let add = match spline_add(sketch, &drawing, kind, closed) {
            Ok(add) => add,
            Err(out) => {
                self.refuse(out.into());
                return;
            }
        };
        if self.propose(SketchEdit::Add(add)) {
            drawing.restart();
            self.set_drawing(drawing);
        }
    }

    /// Switches the Spline tool between drawing through fit points and by
    /// control points, keeping the points placed.
    pub(crate) fn toggle_spline_kind(&mut self) {
        let drawing = self.sketch.as_mut().and_then(|s| s.tool.as_mut());
        if let Some(drawing) = drawing.filter(|drawing| drawing.tool == Tool::Spline) {
            drawing.control = !drawing.control;
        }
    }

    /// Switches the splines selected between through fit points and by
    /// control points, each keeping its shape as closely as it can: a
    /// proposal each.
    pub(crate) fn convert_splines(&mut self) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(session) = &self.sketch else {
            return;
        };
        for edit in varde_view::spline::conversions(sketch, &session.selection) {
            self.propose(edit);
        }
    }

    /// Gives the fit points selected handles, or the ends of the splines
    /// selected, or takes them away where they all have them (see
    /// [`varde_view::spline::handles`]).
    pub(crate) fn toggle_handles(&mut self) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(session) = &self.sketch else {
            return;
        };
        if let Some(edit) = varde_view::spline::handles(sketch, &session.selection) {
            self.propose(edit);
        }
    }

    /// Closes the shape the drawing tool is drawing where it can (the
    /// Spline tool's spline, the Line tool's chain back to its first
    /// point), or with no drawing tool closes or opens the curves
    /// selected (see [`varde_view::spline::closings`]), a proposal each.
    pub(crate) fn toggle_closed(&mut self) {
        let drawing = self
            .sketch
            .as_ref()
            .and_then(|session| session.tool.clone());
        if let Some(drawing) = drawing.filter(|drawing| drawing.tool.draws()) {
            if drawing.active().can_close() {
                match drawing.tool {
                    Tool::Spline => self.end_spline(drawing, true),
                    Tool::Line => self.close_chain(drawing),
                    _ => {}
                }
            }
            return;
        }
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(session) = &self.sketch else {
            return;
        };
        if let Some((_, edits)) = varde_view::spline::closings(sketch, &session.selection) {
            for edit in edits {
                self.propose(edit);
            }
        }
    }

    /// Closes or opens the curve of a Geometry row, `item`, from its
    /// context menu, or the selection if it's among it, as the key does.
    pub(crate) fn toggle_closed_item(&mut self, item: Selectable) {
        let Some(sketch) = self.editable_sketch() else {
            return;
        };
        let Some(session) = &self.sketch else {
            return;
        };
        let items = if session.selection.contains(&item) {
            session.selection.clone()
        } else {
            BTreeSet::from([item])
        };
        if let Some((_, edits)) = varde_view::spline::closings(sketch, &items) {
            for edit in edits {
                self.propose(edit);
            }
        }
    }

    /// Adds a point to the spline `spline` where it passes nearest `at`
    /// (see [`SketchEdit::InsertPoint`]).
    pub(crate) fn insert_spline_point(&mut self, spline: Id, at: glam::DVec2) {
        if self.editable_sketch().is_some() && at.is_finite() {
            self.propose(SketchEdit::InsertPoint { spline, near: at });
        }
    }

    /// Takes the spline points selected out of their splines, if they
    /// all can be (see [`varde_view::spline::removals`]).
    pub(crate) fn remove_spline_points(&mut self) {
        let Some((sketch, session)) = self.editable_sketch().zip(self.sketch.as_ref()) else {
            return;
        };
        if let Some(Ok(edit)) = varde_view::spline::removals(sketch, &session.selection) {
            self.propose(edit);
        }
    }

    /// Starts adding points to splines by clicking them, putting down the
    /// tool in use, or stops, in a sketch that can be changed.
    pub(crate) fn toggle_insert_point(&mut self) {
        let editable = self.editable_sketch().is_some();
        if let Some(session) = &mut self.sketch {
            let inserting = !session.inserting && editable;
            session.drag = None;
            session.tool = None;
            session.constraining = false;
            session.inserting = inserting;
        }
    }

    /// Shows the curvature comb of the splines selected, or hides it.
    pub(crate) fn toggle_comb(&mut self) {
        if let Some(session) = &mut self.sketch {
            session.comb = !session.comb;
        }
    }
}

/// The spline of `kind` that `drawing` has placed, open or `closed`, as
/// an addition to `sketch`: its points where they snapped (see
/// [`place`]), one snapped to the spline itself a new point, coincident
/// (`auto`) with the point placed there if it snapped to one, through fit points with a handle at each, by control
/// points with knots from where they are.
fn spline_add(
    sketch: &Sketch,
    drawing: &Drawing,
    kind: SplineKind,
    closed: bool,
) -> Result<Add, OutOfIds> {
    let mut add = Add::new(sketch);
    let mut points: Vec<Id> = Vec::with_capacity(drawing.placed.len());
    for (&at, &target) in drawing.placed.iter().zip(&drawing.targets) {
        let point = match target {
            Some(Target::Own(own)) => {
                let point = add.point(at)?;
                if let Own::Point(index) = own
                    && let Some(&other) = points.get(index)
                {
                    add.auto.push(Constraint::Coincident(point, other));
                }
                point
            }
            _ => place(sketch, &mut add, at, target)?,
        };
        points.push(point);
    }
    let knots = match kind {
        SplineKind::Through => Vec::new(),
        SplineKind::Control => control_knots(&drawing.placed, closed),
    };
    // Through fit points, a handle at each, keeping the shape drawn.
    let mut handles = Vec::new();
    if kind == SplineKind::Through
        && let Some(tips) = handle_tips(&drawing.placed, closed)
    {
        for ((&at, &from), tip) in points.iter().zip(&drawing.placed).zip(tips) {
            handles.push(Handle {
                at,
                tip: add.point(tip)?,
                end: add.point(2.0 * from - tip)?,
            });
        }
    }
    let spline = Spline {
        kind,
        points,
        closed,
        handles,
        knots,
    };
    add.curve(Curve::Spline(spline), drawing.construction)?;
    Ok(add)
}

#[cfg(test)]
mod tests;
