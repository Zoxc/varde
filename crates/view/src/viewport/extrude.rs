//! The extrude being set up, in the viewport: its sketches' regions
//! shaded, hovering and picking them with the left button (`regions.rs`),
//! and its handle: a shaft along the axis and at each knob a puck, all
//! drawn by the renderer and hit tested here, which the viewport follows
//! while one is dragged. The model hides what's behind it: the renderer
//! depth tests the regions and the pucks (but not the shaft and the rail,
//! drawn on the screen over it), and a knob the model's mesh is in front
//! of isn't drawn or grabbed ([`hidden_by`]), its parts less than opaque
//! hiding nothing, as the renderer draws what's behind them through them.

use std::sync::Arc;

use glam::{DVec2, DVec3};
use iced::widget::shader::Action;
use iced::{Color, Point, Rectangle, mouse};
use varde_document::{MAX_COORD, Placement};
use varde_kernel::RenderMesh;
use varde_render::{Camera, GridPlane, PointStyle, Projection, SketchLayer, Space as LayerSpace};
use varde_sketch::angle;

use super::regions::{self, Regions, grid_plane};
use super::sketch::{line, srgba};
use crate::extrude::{Distance, ExtrudeLook, ExtrudeState, Handle, snap_step};
use crate::hit::segment_distance;
use crate::operation_panel::PanelHover;
use crate::pick::{aabb, ray_hits, through_box};
use crate::projection::Projector;
use crate::theme::SketchColors;
use crate::{Look, Message};

/// How wide the handle's shaft is, in pixels.
const SHAFT_WIDTH: f32 = 2.0;
/// A knob's puck, in pixels: a ring square to the axis, filled faintly,
/// with a dot at its middle, and an arrow out of the cap with an open
/// head, the same size on the screen wherever it is.
const RING_RADIUS: f64 = 11.0;
const RING_WIDTH: f32 = 2.0;
const RING_SEGMENTS: usize = 48;
/// How opaque the ring's fill is.
const RING_FILL: f32 = 0.2;
const DOT_RADIUS: f32 = 2.6;
const ARROW_LENGTH: f64 = 20.0;
const ARROW_WIDTH: f32 = 2.0;
/// The arrow's head: how far back from its tip, and how wide either side.
const HEAD_LENGTH: f64 = 6.0;
const HEAD_HALF_WIDTH: f64 = 4.5;
/// The arrow's head is left out shorter than this on the screen, in
/// pixels, looking along the axis.
const MIN_HEAD: f64 = 1.5;
/// The rail along the axis while a knob is hovered or dragged: how far it
/// reaches either way, in pixels, how wide it is, and in how many steps
/// either way it fades out, from opaque at the knob to clear.
const RAIL_REACH: f64 = 170.0;
const RAIL_WIDTH: f32 = 1.5;
const RAIL_STEPS: usize = 16;
/// How near the cursor a knob is grabbed, in pixels: within its ring or
/// a little past it, or near its arrow (a sketch's hit tolerance).
const RING_HIT: f64 = RING_RADIUS + 2.0;
const ARROW_HIT: f64 = 6.0;
/// How near in front of a knob, in view heights, the model may be and
/// not hide it: as far as the renderer pulls the regions and the handle
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
#[derive(Default)]
pub(crate) struct Input {
    regions: regions::Input,
    /// The knob under the cursor as it last moved, if one is: then no
    /// region is.
    knob: Option<Distance>,
}

/// The model shown, which hides the knobs behind it ([`hidden_by`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Model<'m> {
    pub(crate) mesh: &'m RenderMesh,
    /// How opaque each of its parts is.
    pub(crate) opacity: &'m [f32],
}

/// A knob's puck as it's seen: where, which way out of the cap, how big
/// a pixel is there, and where it and its arrow's tip show.
#[derive(Debug, Clone, Copy)]
struct Puck {
    distance: Distance,
    at: DVec3,
    /// The axis out of the cap: the normal, or against it for a knob on
    /// the other side.
    out: DVec3,
    /// A pixel's size at `at`, in millimetres.
    pixel: f64,
    /// Towards the eye from `at`.
    to_eye: DVec3,
    screen: DVec2,
    tip: DVec2,
}

impl Puck {
    /// The world point `pixels` along its axis, out of the cap.
    fn along(&self, pixels: f64) -> DVec3 {
        self.at + self.out * (pixels * self.pixel)
    }

    /// The screen position `at`'s distance from it, if it's within reach
    /// of the cursor: its ring's, or its arrow's.
    fn reach(&self, at: DVec2) -> Option<f64> {
        let arrow = segment_distance(at, self.screen, self.tip);
        (at.distance(self.screen) <= RING_HIT || arrow <= ARROW_HIT).then_some(arrow)
    }
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

    /// The handle's knobs `mesh`'s opaque parts don't hide from `camera`.
    fn shown_knobs<'s>(
        &'s self,
        camera: &'s Camera,
        mesh: &'s RenderMesh,
        opacity: &'s [f32],
    ) -> impl Iterator<Item = (Distance, f64)> + 's {
        let handle = self.handle.as_ref();
        let knobs = handle.map_or(&[][..], |handle| &handle.knobs);
        knobs.iter().copied().filter(move |&(_, at)| {
            handle.is_some_and(|h| !hidden_by(mesh, opacity, camera, h.origin + h.normal * at))
        })
    }

    /// The pucks of the knobs `model` doesn't hide ([`Self::shown_knobs`]),
    /// seen by `camera` over `bounds`, those behind the eye left out.
    fn pucks(&self, camera: &Camera, bounds: Rectangle, model: Model<'_>) -> Vec<Puck> {
        let Some(handle) = &self.handle else {
            return Vec::new();
        };
        let Some(projector) =
            Projector::new(camera, handle.placement(), bounds.width, bounds.height)
        else {
            return Vec::new();
        };
        let (eye, backward) = projector.eye();
        let puck = |(distance, t): (Distance, f64)| {
            let at = handle.origin + handle.normal * t;
            let depth = projector.world_depth(at);
            if projector.perspective() && (depth.is_nan() || depth <= projector.near()) {
                return None;
            }
            let pixel = projector.pixel_at(depth);
            if !(pixel > 0.0 && pixel.is_finite() && at.is_finite()) {
                return None;
            }
            let out = handle.normal * outward(distance, t);
            let to_eye = if projector.perspective() {
                (eye - at).normalize_or_zero()
            } else {
                backward
            };
            let tip = at + out * (ARROW_LENGTH * pixel);
            let (screen, tip) = (projector.show(at), projector.show(tip));
            Some(Puck {
                distance,
                at,
                out,
                pixel,
                to_eye,
                screen,
                tip,
            })
        };
        self.shown_knobs(camera, model.mesh, model.opacity)
            .filter_map(puck)
            .collect()
    }

    /// The knob under the screen position `at`, if the extrude can be
    /// changed: of those in reach ([`Puck::reach`]), the one whose arrow
    /// is nearest.
    fn knob_at(
        &self,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
        model: Model<'_>,
    ) -> Option<Distance> {
        if !self.state.editable {
            return None;
        }
        (self.pucks(camera, bounds, model).into_iter())
            .filter_map(|puck| Some((puck.distance, puck.reach(at)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(distance, _)| distance)
    }

    /// Takes the mouse `event` with the `cursor` over `bounds` seen by
    /// `camera`, `model` the model shown: hovering and grabbing the knobs, ahead of hovering
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
        model: Model<'_>,
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
                let knob = over.and_then(|at| self.knob_at(at, camera, bounds, model));
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
                if let Some(distance) = self.knob_at(at, camera, bounds, model) {
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
        model: Model<'_>,
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
            // On the screen, so it shows through the model.
            for &(_, at) in &handle.knobs {
                let end = handle.origin + handle.normal * at;
                if let Some((a, b)) = projector.in_front(handle.origin, end) {
                    live.polyline(
                        LayerSpace::Screen,
                        &[projector.show(a), projector.show(b)],
                        line(colors.handle, SHAFT_WIDTH, false),
                    );
                }
            }
        }
        let active = self.state.grabbed.or(input.knob);
        for puck in self.pucks(camera, bounds, model) {
            let hot = active == Some(puck.distance) && self.state.editable;
            if hot && let Some(projector) = &projector {
                draw_rail(&mut live, &puck, projector, colors.rail);
            }
            draw_puck(&mut live, &puck, colors, hot);
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

/// The ring of `puck`, square to its axis, filled faintly, with a dot at
/// its middle, and its arrow out of the cap, in the handle's colours, the
/// hovered ones if `hot`. The arrow's head is open, across the axis in
/// the plane through it facing the eye, left out looking along the axis.
fn draw_puck(live: &mut SketchLayer, puck: &Puck, colors: SketchColors, hot: bool) {
    let (color, accent) = if hot {
        (colors.handle_hovered, colors.handle_accent_hovered)
    } else {
        (colors.handle, colors.handle_accent)
    };
    let (x, y) = puck.out.any_orthonormal_pair();
    let placement = Placement {
        origin: puck.at,
        x,
        y,
        normal: puck.out,
    };
    if let Some(plane) = grid_plane(placement) {
        let radius = RING_RADIUS * puck.pixel;
        let ring: Vec<DVec2> = (0..=RING_SEGMENTS)
            .map(|k| {
                let turn = std::f64::consts::TAU * k as f64 / RING_SEGMENTS as f64;
                DVec2::new(angle::cos(turn), angle::sin(turn)) * radius
            })
            .collect();
        let fill = Color {
            a: color.a * RING_FILL,
            ..color
        };
        live.fill(LayerSpace::On(plane), [&ring[..]], srgba(fill));
        live.polyline(LayerSpace::On(plane), &ring, line(color, RING_WIDTH, false));
    }
    live.world_point(
        puck.at.as_vec3(),
        PointStyle {
            radius: DOT_RADIUS,
            rim_width: DOT_RADIUS,
            rim: srgba(color),
            fill: srgba(color),
            fixed: true,
        },
    );
    let tip = puck.along(ARROW_LENGTH);
    let style = line(accent, ARROW_WIDTH, false);
    live.world_polyline(&[puck.at.as_vec3(), tip.as_vec3()], style);
    let across = puck.out.cross(puck.to_eye).normalize_or_zero() * (HEAD_HALF_WIDTH * puck.pixel);
    let back = puck.along(ARROW_LENGTH - HEAD_LENGTH);
    let shown = puck.tip.distance(puck.screen) * HEAD_LENGTH / ARROW_LENGTH;
    if across != DVec3::ZERO && shown >= MIN_HEAD {
        let head = [back + across, tip, back - across].map(|p| p.as_vec3());
        live.world_polyline(&head, style);
    }
}

/// The rail along `puck`'s axis, in `color`, on the screen as
/// `projector` shows it, so it's drawn over the model: [`RAIL_REACH`]
/// either way of it, fading out in [`RAIL_STEPS`] from opaque at the knob
/// to clear, cut where it passes behind the eye of a perspective view.
fn draw_rail(live: &mut SketchLayer, puck: &Puck, projector: &Projector, color: Color) {
    let alpha: Vec<f32> = (0..RAIL_STEPS)
        .map(|k| 1.0 - (k as f32 + 0.5) / RAIL_STEPS as f32)
        .collect();
    for way in [1.0, -1.0] {
        let Some((from, to)) = projector.in_front(puck.at, puck.along(RAIL_REACH * way)) else {
            continue;
        };
        let points: Vec<_> = (0..=RAIL_STEPS)
            .map(|k| projector.show(from.lerp(to, k as f64 / RAIL_STEPS as f64)))
            .collect();
        let style = line(color, RAIL_WIDTH, false);
        live.polyline_fading(LayerSpace::Screen, &points, &alpha, style);
    }
}

/// [`hidden_by`] with every part of `mesh` opaque.
#[cfg(test)]
pub(crate) fn hidden(mesh: &RenderMesh, camera: &Camera, at: DVec3) -> bool {
    hidden_by(mesh, &[], camera, at)
}

/// Whether `mesh`, its parts as opaque as `opacity` has them (see
/// [`varde_render::Frame::opacity`]), hides the world point `at` from
/// `camera`: a triangle of an opaque part is in front of `at`, more than
/// [`KNOB_PULL`] view heights nearer the eye, so one `at` lies on
/// doesn't. Nor does one whose plane passes within the mesh's `f32`
/// rounding of `at` (a few units in the last place of the largest
/// coordinate), which seen at a grazing angle can be far along the ray: a
/// knob on the cap it ends on, far from the origin, would be hidden by
/// the cap's rounded corners. A part less than opaque hides nothing; one
/// past `opacity`'s end, or out of its range, is opaque. Never with more
/// than [`MAX_HIDING_TRIANGLES`].
pub(crate) fn hidden_by(mesh: &RenderMesh, opacity: &[f32], camera: &Camera, at: DVec3) -> bool {
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
    // The parts' runs of indices that hide nothing, in order.
    let see_through: Vec<_> = (mesh.parts().zip(opacity))
        .filter(|&(_, &alpha)| (0.0..1.0).contains(&alpha))
        .map(|(part, _)| part.indices)
        .collect();
    let hides = |triangle: usize| {
        let index = triangle.saturating_mul(3);
        let at = see_through.partition_point(|run| run.end <= index);
        see_through.get(at).is_none_or(|run| !run.contains(&index))
    };
    let triangles = mesh.indices().as_chunks::<3>().0.iter().enumerate();
    triangles.filter(|&(i, _)| hides(i)).any(|(_, triangle)| {
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

#[cfg(test)]
mod tests;
