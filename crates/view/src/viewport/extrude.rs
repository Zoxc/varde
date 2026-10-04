//! The extrude being set up, in the viewport: its sketches' regions
//! shaded, hovering and picking them with the left button (`regions.rs`),
//! and its handle: a shaft along the axis and at each knob a puck, all
//! drawn by the renderer and hit tested here, which the viewport follows
//! while one is dragged. The model hides what's behind it of the regions,
//! which the renderer depth tests; the handle is drawn on the screen over
//! it, so it always shows (`handle.rs`).

use std::sync::Arc;

use glam::DVec2;
use iced::widget::shader::Action;
use iced::{Point, Rectangle, mouse};
use varde_document::MAX_COORD;
use varde_render::{Camera, GridPlane, SketchLayer};

use super::handle::{self, Puck};
use super::regions::{self, Regions};
use crate::extrude::{Distance, ExtrudeLook, ExtrudeState, Handle, snap_step};
use crate::motion::KnobTone;
use crate::operation_panel::PanelHover;
use crate::projection::Projector;
use crate::theme::SketchColors;
use crate::{Look, Message};

/// The extrude being set up, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Extruding<'a> {
    state: ExtrudeState<'a>,
    handle: Option<Handle>,
}

/// What the viewport keeps of the extrude between events and frames.
#[derive(Default)]
pub(crate) struct Input {
    regions: regions::Input,
    /// The knob under the cursor as it last moved, if one is: then no
    /// region is.
    knob: Option<Distance>,
}

impl<'a> Extruding<'a> {
    pub(crate) fn new(state: ExtrudeState<'a>) -> Self {
        let handle = state.handle();
        Self { state, handle }
    }

    /// Its sketches and the regions picked.
    fn regions(&self) -> Regions<'_, 'a> {
        Regions {
            candidates: &self.state.candidates,
            source: self.state.source,
            picked: self.state.picked,
            panel: self.state.hover.and_then(PanelHover::region),
        }
    }

    /// Where its layers are drawn: see [`Regions::plane`].
    pub(crate) fn plane(&self) -> GridPlane {
        self.regions().plane()
    }

    /// The pucks of the handle's knobs, seen by `camera` over `bounds`,
    /// those behind the eye left out.
    fn pucks(&self, camera: &Camera, bounds: Rectangle) -> Vec<Puck<Distance>> {
        let Some(handle) = &self.handle else {
            return Vec::new();
        };
        let Some(projector) =
            Projector::new(camera, handle.placement(), bounds.width, bounds.height)
        else {
            return Vec::new();
        };
        let puck = |(distance, t): (Distance, f64)| {
            let at = handle.origin + handle.normal * t;
            Puck::new(
                distance,
                at,
                handle.normal * outward(distance, t),
                &projector,
            )
        };
        handle.knobs.iter().copied().filter_map(puck).collect()
    }

    /// The knob under the screen position `at`, if the extrude can be
    /// changed ([`handle::knob_at`]).
    fn knob_at(&self, at: DVec2, camera: &Camera, bounds: Rectangle) -> Option<Distance> {
        if !self.state.editable {
            return None;
        }
        handle::knob_at(&self.pucks(camera, bounds), at)
    }

    /// Takes the mouse `event` with the `cursor` over `bounds` seen by
    /// `camera`: hovering and grabbing the knobs, ahead of hovering
    /// and picking regions, and dragging the knob grabbed. `None` for
    /// what's left to the camera: the left button pressed off the knobs
    /// and regions orbits, as outside a session.
    pub(crate) fn mouse(
        &self,
        input: &mut Input,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let local = |p: Point| DVec2::new((p.x - bounds.x).into(), (p.y - bounds.y).into());
        match event {
            mouse::Event::CursorMoved { position } => {
                if let Some(distance) = self.state.grabbed {
                    // The raw position, so a drag goes on over the rest of
                    // the window.
                    let to = self.drag_to(local(position), camera, bounds)?;
                    let look = ExtrudeLook::DragHandle { distance, to };
                    return Some(Action::publish(Message::Look(Look::Extrude(look))).and_capture());
                }
                let over = cursor.position_over(bounds).map(local);
                let knob = over.and_then(|at| self.knob_at(at, camera, bounds));
                let hover = over
                    .filter(|_| knob.is_none())
                    .and_then(|at| self.regions().region_under(at, camera, bounds));
                let knob_changed = std::mem::replace(&mut input.knob, knob) != knob;
                let region_changed = std::mem::replace(&mut input.regions.hover, hover) != hover;
                let changed = knob_changed || region_changed;
                match (changed, knob) {
                    (true, Some(_)) => Some(Action::request_redraw().and_capture()),
                    (false, Some(_)) => Some(Action::capture()),
                    (true, None) => Some(Action::request_redraw()),
                    (false, None) => None,
                }
            }
            mouse::Event::CursorLeft => {
                let had = input.knob.take().is_some() | input.regions.hover.take().is_some();
                had.then(Action::request_redraw)
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
                if let Some(distance) = self.knob_at(at, camera, bounds) {
                    let look = ExtrudeLook::GrabHandle(distance);
                    return Some(Action::publish(Message::Look(Look::Extrude(look))).and_capture());
                }
                let (sketch, region) = self.regions().region_under(at, camera, bounds)?;
                if !self.state.editable {
                    return None;
                }
                let look = ExtrudeLook::PickRegion { sketch, region };
                Some(Action::publish(Message::Look(Look::Extrude(look))).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                self.state.grabbed?;
                let look = Look::Extrude(ExtrudeLook::DropHandle);
                Some(Action::publish(Message::Look(look)).and_capture())
            }
            _ => None,
        }
    }

    /// The cursor dragging a knob, over one, or over a region.
    pub(crate) fn mouse_interaction(
        &self,
        input: &Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<mouse::Interaction> {
        if self.state.grabbed.is_some() {
            return Some(mouse::Interaction::Grabbing);
        }
        cursor.position_over(bounds)?;
        if input.knob.is_some() {
            return Some(mouse::Interaction::Grab);
        }
        (input.regions.hover.is_some() && self.state.editable)
            .then_some(mouse::Interaction::Pointer)
    }

    /// Where on the handle's axis the screen position `at` drags a knob:
    /// the point of the axis nearest the cursor's ray, in millimetres from
    /// the handle's origin, snapped to the design's units' round steps
    /// ([`snap_step`]). `None` without a handle, looking along its axis, or
    /// past [`MAX_COORD`].
    fn drag_to(&self, at: DVec2, camera: &Camera, bounds: Rectangle) -> Option<f64> {
        let handle = self.handle.as_ref()?;
        let projector = Projector::new(camera, handle.placement(), bounds.width, bounds.height)?;
        let t = projector.along_line(handle.origin, handle.normal, at)?;
        let step = snap_step(projector.pixel(), self.state.units)?;
        let t = (t / step).round() * step;
        (t.is_finite() && t.abs() <= f64::from(MAX_COORD)).then_some(t)
    }

    /// What the renderer draws of the extrude with `input` as it is, all
    /// of it hidden by the model in front of it: the base layer, the
    /// source sketch's regions shaded and those picked marked, built again
    /// only when they change; and the live layer, the region hovered,
    /// before there's a source every candidate's regions, each on its own
    /// plane, and the handle: its shaft, on the screen over the model,
    /// unless the extrude's own check refuses it, when there's no preview
    /// for it to stand on, and the
    /// pucks of the knobs `model` doesn't hide, the one hovered or dragged
    /// lighter, with its rail, which is on the screen, over the model.
    pub(crate) fn layers(
        &self,
        input: &Input,
        colors: SketchColors,
        camera: &Camera,
        bounds: Rectangle,
    ) -> (Arc<SketchLayer>, SketchLayer) {
        let regions = self.regions();
        let base = regions.base_layer(&input.regions, colors);
        let mut live = SketchLayer::default();
        regions.live(input.regions.hover, colors, &mut live);
        regions.panel_region(camera, bounds, colors, &mut live);
        let projector = (self.handle.as_ref()).and_then(|handle| {
            Projector::new(camera, handle.placement(), bounds.width, bounds.height)
        });
        if let Some(handle) = &self.handle
            && self.state.refused.is_none()
            && let Some(projector) = &projector
        {
            for &(_, at) in &handle.knobs {
                let end = handle.origin + handle.normal * at;
                handle::draw_shaft(&mut live, projector, &[handle.origin, end], colors.handle);
            }
        }
        let active = self.state.grabbed.or(input.knob);
        for puck in self.pucks(camera, bounds) {
            let hot = active == Some(puck.knob) && self.state.editable;
            if hot && let Some(projector) = &projector {
                handle::draw_rail(&mut live, projector, colors.rail, |px| puck.along(px));
            }
            if let Some(projector) = &projector {
                let tone = handle::tone(colors, KnobTone::Create, hot);
                handle::draw_puck(&mut live, projector, &puck, tone);
            }
        }
        (base, live)
    }
}

/// Which way out of the cap the knob of `distance` at `t` along the
/// normal points: along the normal (1) or against it (-1), as its sign
/// says, or at the sketch plane, as its side does.
fn outward(distance: Distance, t: f64) -> f64 {
    if t < 0.0 || (t == 0.0 && distance == Distance::Second) {
        -1.0
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests;
