//! The extrude being set up, in the viewport: its sketches' regions
//! shaded, hovering and picking them with the left button, and its
//! handle, the shaft drawn by the renderer and the knobs widgets over it
//! (`crate::extrude`) that the viewport follows while one is dragged.
//! Picking casts the cursor's ray onto each sketch's plane and asks
//! [`Profiles::region_at`] there, the nearest hit winning; no GPU picking.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DVec2;
use iced::widget::shader::Action;
use iced::widget::{Space, container, mouse_area};
use iced::{Element, Point, Rectangle, mouse};
use varde_document::{FeatureId, MAX_COORD};
use varde_render::{Camera, GridPlane, SketchLayer, Space as LayerSpace};
use varde_sketch::{Profiles, Region};

use super::sketch::{fill_region, line, srgba};
use crate::anchors::Anchors;
use crate::extrude::{Distance, ExtrudeLook, ExtrudeState, Handle, snap_step};
use crate::projection::Projector;
use crate::theme::{self, SketchColors};
use crate::{Look, Message};

/// How wide the handle's shaft and the outline of picked regions are, in
/// pixels.
const SHAFT_WIDTH: f32 = 2.0;
const OUTLINE_WIDTH: f32 = 1.8;
/// How opaque the fill of a picked region is.
const PICKED_ALPHA: f32 = 0.35;
/// The side of a knob of the handle, in pixels.
const KNOB: f32 = 14.0;
/// How near to along the handle's axis the cursor's ray may run and still
/// drag it, as a share of the ray's length squared: nearer, where the
/// cursor is along it says next to nothing.
const ALONG_AXIS: f64 = 1e-6;

/// The extrude being set up, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Extruding<'a> {
    state: ExtrudeState<'a>,
    handle: Option<Handle>,
}

/// What the viewport keeps of the extrude between events and frames.
#[derive(Default)]
pub(crate) struct Input {
    /// The region under the cursor as it last moved, and its sketch.
    hover: Option<(FeatureId, usize)>,
    /// The base layer last built, and what from.
    base: RefCell<Option<Base>>,
}

/// The base layer as built, and what it was built from.
struct Base {
    /// Compared by pointer: the app finds them again only when the sketch
    /// changes.
    profiles: Arc<Profiles>,
    picked: BTreeSet<usize>,
    colors: SketchColors,
    layer: Arc<SketchLayer>,
}

impl<'a> Extruding<'a> {
    pub(crate) fn new(state: ExtrudeState<'a>) -> Self {
        let handle = state.handle();
        Self { state, handle }
    }

    /// Where its layers are drawn: on the source sketch's plane, or XY
    /// before there is one, when all is drawn on the screen.
    pub(crate) fn plane(&self) -> GridPlane {
        self.state
            .source()
            .and_then(|source| {
                let placement = source.plane.placement();
                GridPlane::new(
                    placement.origin.as_vec3(),
                    placement.x.as_vec3(),
                    placement.y.as_vec3(),
                )
            })
            .unwrap_or(GridPlane::XY)
    }

    /// The handle's knobs, anchored over the viewport seen by `camera`
    /// on its axis, if there's a handle.
    pub(crate) fn knobs(&self, camera: &Camera) -> Option<Element<'a, Message>> {
        let handle = self.handle.as_ref()?;
        let editable = self.state.editable;
        let knobs = handle
            .knobs
            .iter()
            .map(|&(distance, at)| (DVec2::new(at, 0.0), knob(distance, editable)));
        Some(Anchors::new(*camera, handle.placement(), knobs).into())
    }

    /// Takes the mouse `event` with the `cursor` over `bounds` seen by
    /// `camera`: hovering and picking regions, and dragging the knob
    /// grabbed. `None` for what's left to the camera: the left button
    /// pressed off the regions orbits, as outside a session.
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
                let hover = over.and_then(|at| self.region_under(at, camera, bounds));
                (std::mem::replace(&mut input.hover, hover) != hover).then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => input.hover.take().map(|_| Action::request_redraw()),
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
                let (sketch, region) = self.region_under(at, camera, bounds)?;
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

    /// The cursor over a region, or dragging a knob.
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
        (input.hover.is_some() && self.state.editable).then_some(mouse::Interaction::Pointer)
    }

    /// The region under the screen position `at`, and its sketch: of the
    /// candidates' regions the cursor's ray meets, the nearest.
    fn region_under(
        &self,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<(FeatureId, usize)> {
        let mut nearest: Option<(f64, FeatureId, usize)> = None;
        for candidate in &self.state.candidates {
            let placement = candidate.plane.placement();
            let Some(projector) = Projector::new(camera, placement, bounds.width, bounds.height)
            else {
                continue;
            };
            let Some(cursor) = projector.cursor(at) else {
                continue;
            };
            let Some(region) = candidate.profiles.region_at(cursor.at) else {
                continue;
            };
            let depth = projector.depth(cursor.at);
            if nearest.is_none_or(|(nearest, ..)| depth < nearest) {
                nearest = Some((depth, candidate.feature, region));
            }
        }
        nearest.map(|(_, feature, region)| (feature, region))
    }

    /// Where on the handle's axis the screen position `at` drags a knob:
    /// the point of the axis nearest the cursor's ray, in millimetres from
    /// the handle's origin, snapped to the design's units' round steps
    /// ([`snap_step`]). `None` without a handle, looking along its axis, or
    /// past [`MAX_COORD`].
    fn drag_to(&self, at: DVec2, camera: &Camera, bounds: Rectangle) -> Option<f64> {
        let handle = self.handle.as_ref()?;
        let projector = Projector::new(camera, handle.placement(), bounds.width, bounds.height)?;
        let (origin, ray) = projector.ray(at)?;
        let axis = handle.normal;
        let w = origin - handle.origin;
        let (a, b) = (ray.dot(ray), ray.dot(axis));
        let (d, e) = (ray.dot(w), axis.dot(w));
        let denominator = a - b * b;
        if denominator.is_nan() || denominator <= ALONG_AXIS * a {
            return None;
        }
        let t = (a * e - b * d) / denominator;
        let step = snap_step(projector.pixel(), self.state.units)?;
        let t = (t / step).round() * step;
        (t.is_finite() && t.abs() <= f64::from(MAX_COORD)).then_some(t)
    }

    /// What the renderer draws of the extrude with `input` as it is, in a
    /// viewport of `bounds` seen by `camera`: the base layer, the source
    /// sketch's regions shaded and those picked marked, built again only
    /// when they change; and the live layer, the region hovered, before
    /// there's a source every candidate's regions, and the handle's shaft.
    pub(crate) fn layers(
        &self,
        input: &Input,
        camera: &Camera,
        bounds: Rectangle,
        colors: SketchColors,
    ) -> (Arc<SketchLayer>, SketchLayer) {
        let base = self.base_layer(input, colors);
        let mut live = SketchLayer::default();
        let source = self.state.source();
        for candidate in &self.state.candidates {
            let hovered = input
                .hover
                .filter(|(feature, _)| *feature == candidate.feature)
                .and_then(|(_, region)| candidate.profiles.regions.get(region));
            if source.is_some() {
                if let Some(region) = hovered {
                    fill_region(&mut live, region, colors.region_hovered);
                }
                continue;
            }
            // Each candidate is on its own plane, so they're drawn where
            // they show.
            let placement = candidate.plane.placement();
            let Some(projector) = Projector::new(camera, placement, bounds.width, bounds.height)
            else {
                continue;
            };
            for region in &candidate.profiles.regions {
                fill_on_screen(&mut live, &projector, region, colors.region);
            }
            if let Some(region) = hovered {
                fill_on_screen(&mut live, &projector, region, colors.region_hovered);
            }
        }
        if let Some(handle) = &self.handle
            && let Some(projector) =
                Projector::new(camera, handle.placement(), bounds.width, bounds.height)
        {
            for &(_, at) in &handle.knobs {
                if let Some((a, b)) = projector.segment(DVec2::ZERO, DVec2::new(at, 0.0)) {
                    live.polyline(
                        LayerSpace::Screen,
                        &[a, b],
                        line(colors.selected, SHAFT_WIDTH, false),
                    );
                }
            }
        }
        (base, live)
    }

    /// The source sketch's regions shaded, and those picked filled and
    /// outlined, kept in `input` until they change.
    fn base_layer(&self, input: &Input, colors: SketchColors) -> Arc<SketchLayer> {
        let Some(source) = self.state.source() else {
            return Arc::default();
        };
        let mut base = input.base.borrow_mut();
        let current = base.as_ref().is_some_and(|base| {
            Arc::ptr_eq(&base.profiles, source.profiles)
                && base.picked == *self.state.picked
                && base.colors == colors
        });
        if let Some(base) = base.as_ref().filter(|_| current) {
            return base.layer.clone();
        }
        let mut layer = SketchLayer::default();
        let picked = colors.selected.scale_alpha(PICKED_ALPHA);
        for (index, region) in source.profiles.regions.iter().enumerate() {
            if self.state.picked.contains(&index) {
                fill_region(&mut layer, region, picked);
                for polyline in &region.outline {
                    let mut closed = polyline.clone();
                    closed.extend(polyline.first().copied());
                    layer.polyline(
                        LayerSpace::Sketch,
                        &closed,
                        line(colors.selected, OUTLINE_WIDTH, false),
                    );
                }
            } else {
                fill_region(&mut layer, region, colors.region);
            }
        }
        let layer = Arc::new(layer);
        *base = Some(Base {
            profiles: source.profiles.clone(),
            picked: self.state.picked.clone(),
            colors,
            layer: layer.clone(),
        });
        layer
    }
}

/// Fills `region`, on the plane `projector` is of, on `layer` where it
/// shows, in `color`: nothing if any of it is behind the eye.
fn fill_on_screen(
    layer: &mut SketchLayer,
    projector: &Projector,
    region: &Region,
    color: iced::Color,
) {
    let projected: Option<Vec<Vec<DVec2>>> = region
        .outline
        .iter()
        .map(|polyline| polyline.iter().map(|&at| projector.project(at)).collect())
        .collect();
    if let Some(outline) = projected {
        layer.fill(
            LayerSpace::Screen,
            outline.iter().map(Vec::as_slice),
            srgba(color),
        );
    }
}

/// The knob of `distance`, grabbed by pressing it if `editable`.
fn knob<'a>(distance: Distance, editable: bool) -> Element<'a, Message> {
    let dot = container(Space::new().width(KNOB).height(KNOB)).style(theme::knob);
    let area = mouse_area(dot).interaction(if editable {
        mouse::Interaction::Grab
    } else {
        mouse::Interaction::Idle
    });
    if editable {
        area.on_press(Message::Look(Look::Extrude(ExtrudeLook::GrabHandle(
            distance,
        ))))
        .into()
    } else {
        area.into()
    }
}

#[cfg(test)]
mod tests;
