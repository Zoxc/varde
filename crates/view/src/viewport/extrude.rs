//! The extrude being set up, in the viewport: its sketches' regions
//! shaded, hovering and picking them with the left button (`regions.rs`),
//! and its handle, the shaft drawn by the renderer and the knobs widgets
//! over it (`crate::extrude`) that the viewport follows while one is
//! dragged. The model hides what's behind it: the renderer depth tests
//! the regions and the shaft, and a knob the model's mesh is in front of
//! isn't shown.

use std::sync::Arc;

use glam::{DVec2, DVec3};
use iced::widget::shader::Action;
use iced::widget::{Space, container, mouse_area};
use iced::{Element, Point, Rectangle, mouse};
use varde_document::MAX_COORD;
use varde_kernel::RenderMesh;
use varde_render::{Camera, GridPlane, Projection, SketchLayer, Space as LayerSpace};

use super::regions::{self, Regions, grid_plane};
use super::sketch::line;
use crate::anchors::Anchors;
use crate::extrude::{Distance, ExtrudeLook, ExtrudeState, Handle, snap_step};
use crate::pick::{aabb, ray_hits, through_box};
use crate::projection::Projector;
use crate::theme::{self, SketchColors};
use crate::{Look, Message};

/// How wide the handle's shaft is, in pixels.
const SHAFT_WIDTH: f32 = 2.0;
/// The side of a knob of the handle, in pixels.
const KNOB: f32 = 14.0;
/// How near to along the handle's axis the cursor's ray may run and still
/// drag it, as a share of the ray's length squared: nearer, where the
/// cursor is along it says next to nothing.
const ALONG_AXIS: f64 = 1e-6;
/// How near in front of a knob, in view heights, the model may be and
/// not hide it: as far as the renderer pulls the regions and the shaft
/// towards the camera, so a knob on a face shows like a region on it.
const KNOB_PULL: f64 = 0.002;
/// The most triangles the model is looked through for what hides a knob;
/// with more, the knobs show wherever they are, rather than slow every
/// frame down.
const MAX_HIDING_TRIANGLES: usize = 1 << 18;

/// The extrude being set up, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Extruding<'a> {
    state: ExtrudeState<'a>,
    handle: Option<Handle>,
}

/// What the viewport keeps of the extrude between events and frames.
pub(crate) type Input = regions::Input;

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
        }
    }

    /// Where its layers are drawn: see [`Regions::plane`].
    pub(crate) fn plane(&self) -> GridPlane {
        self.regions().plane()
    }

    /// The handle's knobs, anchored over the viewport seen by `camera`
    /// on its axis, if there's a handle: those `mesh`, the model shown,
    /// doesn't hide ([`hidden`]).
    pub(crate) fn knobs(&self, camera: &Camera, mesh: &RenderMesh) -> Option<Element<'a, Message>> {
        let handle = self.handle.as_ref()?;
        let editable = self.state.editable;
        let knobs = self
            .shown_knobs(camera, mesh)
            .map(|(distance, at)| (DVec2::new(at, 0.0), knob(distance, editable)));
        Some(Anchors::new(*camera, handle.placement(), knobs).into())
    }

    /// The handle's knobs `mesh` doesn't hide from `camera`.
    fn shown_knobs(
        &self,
        camera: &Camera,
        mesh: &RenderMesh,
    ) -> impl Iterator<Item = (Distance, f64)> {
        let handle = self.handle.as_ref();
        let knobs = handle.map_or(&[][..], |handle| &handle.knobs);
        knobs.iter().copied().filter(move |&(_, at)| {
            handle.is_some_and(|h| !hidden(mesh, camera, h.origin + h.normal * at))
        })
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
                let hover = over.and_then(|at| self.regions().region_under(at, camera, bounds));
                (std::mem::replace(&mut input.hover, hover) != hover).then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => input.hover.take().map(|_| Action::request_redraw()),
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
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

    /// What the renderer draws of the extrude with `input` as it is, all
    /// of it hidden by the model in front of it: the base layer, the
    /// source sketch's regions shaded and those picked marked, built again
    /// only when they change; and the live layer, the region hovered,
    /// before there's a source every candidate's regions, each on its own
    /// plane, and the handle's shaft, unless the extrude's own check
    /// refuses it, when there's no preview for it to stand on.
    pub(crate) fn layers(
        &self,
        input: &Input,
        colors: SketchColors,
    ) -> (Arc<SketchLayer>, SketchLayer) {
        let regions = self.regions();
        let base = regions.base_layer(input, colors);
        let mut live = SketchLayer::default();
        regions.live(input.hover, colors, &mut live);
        if let Some(handle) = &self.handle
            && self.state.refused.is_none()
            && let Some(plane) = grid_plane(handle.placement())
        {
            for &(_, at) in &handle.knobs {
                live.polyline(
                    LayerSpace::On(plane),
                    &[DVec2::ZERO, DVec2::new(at, 0.0)],
                    line(colors.selected, SHAFT_WIDTH, false),
                );
            }
        }
        (base, live)
    }
}

/// Whether `mesh` hides the world point `at` from `camera`: a triangle
/// of it is in front of `at`, more than [`KNOB_PULL`] view heights nearer
/// the eye, so one `at` lies on doesn't. Nor does one whose plane passes
/// within the mesh's `f32` rounding of `at` (a few units in the last
/// place of the largest coordinate), which seen at a grazing angle can
/// be far along the ray: a knob on the cap it ends on, far from the
/// origin, would be hidden by the cap's rounded corners. Never with more
/// than [`MAX_HIDING_TRIANGLES`].
pub(crate) fn hidden(mesh: &RenderMesh, camera: &Camera, at: DVec3) -> bool {
    if mesh.triangle_count() > MAX_HIDING_TRIANGLES || !at.is_finite() {
        return false;
    }
    let Some(bounds) = mesh.bounds() else {
        return false;
    };
    // Towards the eye, as far as it in perspective, without end in an
    // orthographic view, which sees what's behind its eye too.
    let pull = KNOB_PULL * f64::from(camera.view_height());
    let (direction, end) = match camera.projection() {
        Projection::Perspective => {
            let to_eye = camera.eye().as_dvec3() - at;
            let length = to_eye.length();
            if length.is_nan() || length <= pull {
                return false;
            }
            (to_eye / length, length)
        }
        Projection::Orthographic => (camera.backward().as_dvec3(), f64::INFINITY),
    };
    let Some((near, far)) = through_box(at, direction, aabb(bounds), 0.0, f64::INFINITY) else {
        return false;
    };
    if far <= pull || near >= end {
        return false;
    }
    // The rounding of the mesh's corners to `f32`, at the scale of `at`
    // and the mesh's.
    let scale = at.abs().max_element().max(f64::from(
        bounds.min.abs().max(bounds.max.abs()).max_element(),
    ));
    let slack = 4.0 * f64::from(f32::EPSILON) * scale;
    let corner = |index: &u32| {
        let p = mesh.positions().get(usize::try_from(*index).ok()?)?;
        Some(glam::Vec3::from(*p).as_dvec3())
    };
    mesh.indices().as_chunks::<3>().0.iter().any(|triangle| {
        let [Some(a), Some(b), Some(c)] = triangle.each_ref().map(corner) else {
            return false;
        };
        let off_its_plane = || {
            let normal = (b - a).cross(c - a).normalize_or_zero();
            (at - a).dot(normal).abs() > slack
        };
        ray_hits(at, direction, [a, b, c]).is_some_and(|t| t > pull && t < end) && off_its_plane()
    })
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
