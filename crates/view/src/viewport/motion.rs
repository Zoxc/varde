//! The move or mirror being set up, in the viewport. The cursor picks the
//! model as outside the sessions (`ModelPicking`, the app routing the
//! clicks); drawn here, on top of the model so it shows through the
//! bodies it runs through: a move's axis, a line across the bodies' box
//! with an arrowhead (on the screen) at the end positive angles turn
//! right-handed about, or a mirror's plane, a square across their box,
//! outlined dashed and filled faintly, with its normal's short line; a
//! draft's neutral plane as a mirror's, with the pull drawn through it
//! as an axis. All in the selected colour, or the hovered one while the
//! panel's row of it is hovered.
//!
//! A move has handles at its bodies' pivot ([`MotionState::centre`],
//! else their box's centre), a fixed size on the screen and laid out
//! there to keep apart ([`Handles::new`](handles::Handles::new)): a short arrow along each world
//! axis, a shaft in the axis's colour to a puck as the extrude's handle
//! has, flipped to spread them and left out pointing at the eye; and on a
//! faint orb round them, in the gaps between the arrows, a knob for each
//! ring not seen edge on, on a stretch of its ring. They take the mouse
//! ahead of picking the model. Dragging an
//! arrow sets that axis's offset, where the cursor's ray passes nearest
//! the arrow's line, snapped as the extrude's handle ([`snap_step`](crate::extrude::snap_step) of a
//! pixel's size at the camera's target);
//! dragging a ring turns the bodies about that world axis by the angle
//! the cursor sweeps about the centre on the ring's plane, snapped to
//! round degrees ([`angle_step`](super::handle::angle_step)), shifting them so they turn about the
//! centre ([`MotionLook::Turn`](crate::motion::MotionLook::Turn)). While the move turns about another
//! axis, only that axis's ring is shown, if it's a world axis. The texts
//! go to the panel's fields, so the preview and OK follow as for typed
//! values. No handles while the axis is being picked, for a mirror, or
//! in a document that can't be changed.
//!
//! An offset face has the extrude's handle: an arrow from its first
//! face's point along the face's outward normal, as the face is before
//! the offset, its knob at the distance (behind the face inward). Dragged
//! (knob or shaft), the knob follows the cursor's ray where it passes
//! nearest the arrow's line, snapped as the extrude's handle, through
//! zero to the other side: the distance and Inward go to the panel
//! ([`MotionLook::OffsetBy`](crate::motion::MotionLook::OffsetBy)), so the preview and OK follow as for a
//! typed distance, and a typed one moves the knob. It takes the mouse
//! ahead of picking the model's faces.
//!
//! A split's origin plane is drawn as a mirror's. While its tool is a
//! sketch's regions, they're shaded as an extrude's (`regions.rs`), those
//! picked filled; while it's a line, the sketches' curves are drawn (the
//! line's curves picked in the selected colour). While its tool is
//! picked, those take the left button: the region or curve under the
//! cursor (hit tested on each sketch's plane, the nearest by depth) is
//! hovered and a click picks or un-picks it, the model not picked
//! meanwhile. The pieces the preview shows are labelled with their
//! bodies' names ([`Moving::labels`]).

use std::sync::Arc;

use glam::DVec2;
use iced::widget::shader::Action;
use iced::widget::{container, text};
use iced::{Element, Rectangle, mouse};
use varde_document::{OriginPlane, Placement};
use varde_render::{Camera, Colors, GridPlane, SketchLayer};

mod handles;
mod loft;
mod marks;
mod split;
mod sweep;

use super::knobs;
use crate::Message;
use crate::anchors::Anchors;
use crate::motion::{MotionField, MotionKind, MotionState, SplitMode};
use crate::operation_panel::PanelHover;
use crate::theme::{self, SketchColors};
use crate::viewport::local_cursor;
use handles::{Drag, Grip, draw_handles};
use loft::{LoftInput, SeamInput, loft_labels};
use marks::scale_marks;
use split::{SplitInput, split_regions};
use sweep::{sweep_lines, sweep_regions};

/// How wide a split's sketches' curves are drawn, in pixels: a
/// revolve's lines'. The one hovered and those picked are drawn as the
/// sketches' items picked with the model ([`HOVERED_WIDTH`](super::sketch_pick::HOVERED_WIDTH),
/// [`SELECTED_WIDTH`](super::sketch_pick::SELECTED_WIDTH)).
const CURVE_WIDTH: f32 = 1.5;
/// How near the cursor a move's shaft or ring, or a loft's corner, is
/// grabbed, in pixels: a sketch's hit tolerance; a knob, anywhere on it
/// too.
const HIT_PIXELS: f64 = 6.0;

/// What the viewport keeps of a move between events and frames.
#[derive(Debug, Default)]
pub(crate) struct Input {
    /// The handle under the cursor, if any.
    hover: Option<Grip>,
    drag: Option<Drag>,
    /// A split's sketches' regions or curves.
    split: SplitInput,
    /// The knobs of an operation's handle ([`MotionState::knobs`]).
    knobs: knobs::Input,
    /// A sweep's profile's regions and its path sketches' curves.
    sweep: SplitInput,
    /// A loft's sections and rails picked in their sketches.
    loft: LoftInput,
    /// A loft's seam knobs, its sections' starts.
    seams: SeamInput,
    /// Whether the cursor just left a sweep's path curve: the move is
    /// the model's picking's too (an edge there hovered at once), and the
    /// curve's lit chain is drawn away after it ([`Input::take_redraw`]).
    redraw: bool,
}

impl Input {
    /// Whether the handles have the mouse: one is under the cursor or
    /// dragged, so the model isn't picked under them.
    pub(crate) fn holds(&self) -> bool {
        self.hover.is_some()
            || self.drag.is_some()
            || self.knobs.holds()
            || self.sweep.curve.is_some()
            || self.seams != SeamInput::default()
    }

    /// Lets go of a sweep's, a split's or a loft's region, curve, point
    /// or corner under the cursor: whether there was one, to be drawn
    /// away.
    pub(crate) fn leave_sketches(&mut self) -> bool {
        self.leave_sweep() | self.leave_split() | std::mem::take(&mut self.loft).over()
    }

    /// Lets go of a sweep's region or path curve under the cursor:
    /// whether there was one.
    fn leave_sweep(&mut self) -> bool {
        self.sweep.leave()
    }

    /// Lets go of a split's region or curve under the cursor: whether
    /// there was one.
    fn leave_split(&mut self) -> bool {
        self.split.leave()
    }

    /// Lets go of what another kind of session held, for a session of
    /// `kind`, with knobs (`knobs`) or without: a move's handles, an
    /// operation's knobs, a split's region or curve. What a session
    /// left held (its handle under the cursor or dragged as it ended)
    /// would otherwise keep the next session's hover off the model, as
    /// [`Input::holds`] says: even one returning before its own handles
    /// are worked out (a split, sweep or loft picking in its sketches).
    fn settle(&mut self, kind: MotionKind, knobs: bool) {
        if knobs || kind != MotionKind::Move {
            self.hover = None;
            self.drag = None;
        }
        if !knobs {
            self.knobs.clear();
        }
        if kind != MotionKind::Split {
            self.split = SplitInput::default();
        }
        if kind != MotionKind::Sweep {
            self.sweep = SplitInput::default();
        }
        if kind != MotionKind::Loft {
            self.loft = LoftInput::default();
            self.seams = SeamInput::default();
        }
    }

    /// Whether a frame is wanted for what the last event changed without
    /// taking it (see [`Input::redraw`]): asked once.
    pub(crate) fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }
}

/// The move or mirror being set up, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Moving<'a> {
    /// Boxed: it's the largest of what the viewport operates.
    state: Box<MotionState<'a>>,
}

impl<'a> Moving<'a> {
    pub(crate) fn new(state: MotionState<'a>) -> Self {
        Self {
            state: Box::new(state),
        }
    }

    /// What it draws, seen by `camera` over `bounds` with `input` as it
    /// is: no base layer, as it all changes with the answers, and a live
    /// one with the axis and its arrow or the plane, and a move's handles
    /// in the `scene`'s axis colours, the one under the cursor or dragged
    /// in the hovered colour.
    pub(crate) fn layers(
        &self,
        input: &Input,
        scene: &Colors,
        colors: SketchColors,
        camera: &Camera,
        bounds: Rectangle,
    ) -> (Arc<SketchLayer>, SketchLayer) {
        let mut live = SketchLayer::default();
        if let Some([point, along]) = self.state.line
            && point.is_finite()
            && let Some(along) = along.try_normalize()
        {
            let color = if self.state.hover == Some(PanelHover::Axis) {
                colors.hovered
            } else {
                colors.selected
            };
            match self.state.kind {
                MotionKind::Mirror | MotionKind::Split => {
                    self.plane(&mut live, point, along, color);
                }
                // A draft's neutral plane, and the pull's arrow through it.
                MotionKind::Draft => {
                    self.plane(&mut live, point, along, color);
                    self.axis(&mut live, point, along, color, camera, bounds);
                }
                _ => self.axis(&mut live, point, along, color, camera, bounds),
            }
        }
        if let Some(handles) = self.handles(input, camera, bounds) {
            draw_handles(&mut live, &handles, input, scene, colors);
        }
        knobs::draw(
            &mut live,
            &self.state.knobs,
            &input.knobs,
            colors,
            camera,
            bounds,
        );
        if let Some(align) = &self.state.align {
            self.align(&mut live, align, scene, colors, camera, bounds);
        }
        if let Some(scale) = &self.state.scale {
            scale_marks(&mut live, scale, scene, colors);
        }
        if let Some(sweep) = &self.state.sweep {
            return self.sweep_layers(live, sweep, input, colors, (camera, bounds));
        }
        if let Some(loft) = &self.state.loft {
            self.loft_layers(&mut live, loft, input, scene, colors, (camera, bounds));
            return (super::NO_LAYER.clone(), live);
        }
        if let Some(split) = &self.state.split {
            let picking = self.split_picking();
            if split.mode == SplitMode::Regions {
                let regions = split_regions(split);
                if picking == Some(SplitMode::Regions) {
                    regions.live(input.split.regions.hover, colors, &mut live);
                }
                if regions.source().is_some() {
                    let base = regions.base_layer(&input.split.regions, colors);
                    return (base, live);
                }
            }
            if split.mode == SplitMode::Line {
                // The curve hovered alone is lit, as a part of a sweep is.
                let hovered = input.split.curve.filter(|_| picking.is_some());
                let lit = hovered
                    .as_ref()
                    .map(|(sketch, curve)| (*sketch, std::slice::from_ref(curve)));
                let chains = split.chain.as_slice();
                sweep_lines(
                    &mut live,
                    &split.lines,
                    chains,
                    None,
                    lit,
                    picking.is_some(),
                    colors,
                );
            }
        }
        (super::NO_LAYER.clone(), live)
    }

    /// Lets go of what's under the cursor of the kinds of picking in
    /// sketches this session doesn't do now: a sweep's, a split's or a
    /// loft's (another session, or picking something else).
    fn leave_unpicked(&self, input: &mut Input) {
        if self.sweep_picking().is_none() {
            input.leave_sweep();
        }
        if self.split_picking().is_none() {
            input.leave_split();
        }
        if self.loft_picking().is_none() {
            input.loft = LoftInput::default();
        }
    }

    /// The labels of the pieces the preview of a split shows, each its
    /// body's name in a chip at the middle of its box, seen by `camera`,
    /// the one keeping the body's id in the accent: none while there are
    /// none.
    pub(crate) fn labels(&self, camera: &Camera) -> Option<Element<'a, Message>> {
        if let Some(loft) = &self.state.loft {
            return loft_labels(loft, camera);
        }
        let split = self.state.split.as_ref()?;
        if split.pieces.is_empty() {
            return None;
        }
        let layers = (split.pieces.iter())
            .filter(|piece| piece.at.is_finite())
            .map(|piece| {
                let keeps = piece.keeps;
                let chip = container(text(piece.name.clone()).size(12).style(
                    move |theme: &iced::Theme| {
                        let palette = theme::palette(theme);
                        text::Style {
                            color: Some(if keeps { palette.accent } else { palette.text }),
                        }
                    },
                ))
                .padding([1, 4])
                .style(|theme| theme::glyph(theme, false));
                let placement = Placement {
                    origin: piece.at,
                    ..OriginPlane::XY.placement()
                };
                Element::from(Anchors::new(
                    *camera,
                    placement,
                    [(DVec2::ZERO, chip.into())],
                ))
            });
        Some(iced::widget::stack(layers).into())
    }

    /// The value of `field` as it last read, if it reads.
    fn field(&self, field: MotionField) -> Option<f64> {
        self.state.fields[field.index()].value
    }

    /// Takes the mouse `event` with the `cursor` over `bounds` seen by
    /// `camera`: hovering and dragging the handles. `None` for what's left
    /// to picking the model and the camera: anything off the handles.
    /// While the cursor is over a handle, what the model held hovered, if
    /// `hovered`, is let go of.
    pub(crate) fn mouse(
        &self,
        input: &mut Input,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        hovered: bool,
    ) -> Option<Action<Message>> {
        input.settle(self.state.kind, !self.state.knobs.is_empty());
        self.leave_unpicked(input);
        if let Some(mode) = self.split_picking() {
            return self.split_mouse(mode, &mut input.split, event, bounds, cursor, camera);
        }
        if !self.state.knobs.is_empty() {
            let state = &self.state;
            return knobs::mouse(
                &state.knobs,
                state.editable,
                state.units,
                &mut input.knobs,
                event,
                bounds,
                cursor,
                camera,
                hovered,
            );
        }
        if let Some(picking) = self.sweep_picking() {
            return self.sweep_mouse(picking, input, event, bounds, cursor, camera);
        }
        if let Some(action) = self.seams_mouse(&mut input.seams, event, bounds, cursor, camera) {
            return Some(action);
        }
        let Some(picking) = self.loft_picking() else {
            return self.handles_mouse(input, event, bounds, cursor, camera, hovered);
        };
        self.loft_mouse(picking, &mut input.loft, event, bounds, cursor, camera)
    }

    /// Works out again which handle is under the `cursor` as a frame is
    /// drawn, as picking the model does: the camera, or the handles with
    /// the bodies, may have moved under a cursor that didn't, which would
    /// leave the model unpicked under a handle no longer there. While
    /// one's under it, what the model held hovered, if `hovered`, is let
    /// go of. Not while one is dragged. A sweep picking in its sketches
    /// works out its region or path curve under the cursor likewise,
    /// the model let go of under a curve; a split or a loft picking in
    /// theirs, what's under it of them (the model isn't picked
    /// meanwhile).
    pub(crate) fn redraw(
        &self,
        input: &mut Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        hovered: bool,
    ) -> Option<Action<Message>> {
        input.settle(self.state.kind, !self.state.knobs.is_empty());
        self.leave_unpicked(input);
        let at = local_cursor(bounds, cursor);
        if let Some(mode) = self.split_picking() {
            return (self.split_hover(mode, &mut input.split, at, camera, bounds))
                .then(Action::request_redraw);
        }
        if let Some(loft) = &self.state.loft
            && input.seams.drag.is_none()
        {
            let over = at.and_then(|at| self.seam_under(loft, at, camera, bounds));
            if std::mem::replace(&mut input.seams.hover, over) != over {
                return Some(Action::request_redraw());
            }
        }
        if let Some(picking) = self.loft_picking()
            && let Some(loft) = &self.state.loft
        {
            let under = at.map_or_else(LoftInput::default, |at| {
                self.loft_under(loft, picking, at, camera, bounds)
            });
            return (std::mem::replace(&mut input.loft, under) != under)
                .then(Action::request_redraw);
        }
        let Some(picking) = self.sweep_picking() else {
            return self.handles_redraw(input, bounds, cursor, camera, hovered);
        };
        self.sweep_redraw(picking, input, at, camera, bounds, hovered)
    }

    /// The cursor over a handle, or dragging one; over a region or curve
    /// a split's tool picks.
    pub(crate) fn mouse_interaction(&self, input: &Input) -> Option<mouse::Interaction> {
        if self.split_picking().is_some() {
            return input.split.over().then_some(mouse::Interaction::Pointer);
        }
        if self.sweep_picking().is_some() {
            // Off the curves, the model's edges as the model picks them.
            return input.sweep.over().then_some(mouse::Interaction::Pointer);
        }
        if input.seams.drag.is_some() {
            return Some(mouse::Interaction::Grabbing);
        }
        if input.seams.hover.is_some() {
            return Some(mouse::Interaction::Grab);
        }
        if self.loft_picking().is_some() {
            return input.loft.over().then_some(mouse::Interaction::Pointer);
        }
        if input.drag.is_some() || input.knobs.dragging() {
            Some(mouse::Interaction::Grabbing)
        } else if input.knobs.holds() {
            Some(mouse::Interaction::Grab)
        } else {
            input.hover.map(|_| mouse::Interaction::Grab)
        }
    }

    /// Where its layers are drawn: the world's XY plane, as the measure
    /// tool's, since it draws in the world; a split's regions' source's
    /// plane, whose regions its base layer draws.
    pub(crate) fn plane_of_layers(&self) -> GridPlane {
        if let Some(sweep) = &self.state.sweep {
            return sweep_regions(sweep, None).plane();
        }
        (self.state.split.as_ref())
            .filter(|split| split.mode == SplitMode::Regions)
            .map_or(GridPlane::XY, |split| split_regions(split).plane())
    }
}

#[cfg(test)]
mod tests;
