//! The revolve being set up, in the viewport: its sketches' regions
//! shaded, hovering and picking them with the left button (`regions.rs`),
//! and while the axis is picked, the sketches' lines and axes drawn and
//! hit tested as in a sketch (`hit::hit_axis`), a click on one picking
//! it; and once there's a source, the model's straight edges in its
//! plane ([`axis_edge`]) drawn and picked as the model picks edges
//! ([`PickIndex::pick`](crate::pick::PickIndex::pick)), after the lines
//! and before the regions. A click on another edge, where there's no
//! region, sends it to be refused, saying why. The axis picked is drawn
//! on the source with an arrow at the end positive angles turn
//! right-handed about (against the line or edge when flipped). Its
//! handle (`handle.rs`, as the extrude's): a puck at the picked regions'
//! centre turned to each angle, its arrow along the way the centre turns,
//! and a shaft round the axis from the sketch plane to it; the knob
//! hovered or dragged lighter, with its rail round the axis. Dragging a
//! knob turns it to where the cursor's ray meets the plane square to the
//! axis through the regions' centre, snapped as a move's ring
//! ([`angle_step`]). The renderer depth tests the regions and lines, so
//! the model hides what's behind them; the axis's arrow and the handle,
//! on the screen, are on top, so the handle always shows.

use std::sync::Arc;

use glam::{DVec2, DVec3};
use iced::widget::shader::Action;
use iced::{Point, Rectangle, mouse};
use varde_document::{AxisLine, FeatureId};
use varde_render::{Camera, GridPlane, SketchLayer, Space as LayerSpace};
use varde_sketch::{Curve, Sketch, angle};

use super::handle::{self, Puck};
use super::motion::{angle_step, wrapped};
use super::regions::{self, Regions, grid_plane};
use super::sketch::{line, srgba};
use super::sketch_pick::HIT_PIXELS;
use crate::hit;
use crate::motion::KnobTone;
use crate::operation_panel::{Candidate, PanelHover};
use crate::pick::{Picked, Picks};
use crate::projection::Projector;
use crate::revolve::{
    Angle, RevolveHandle, RevolveLook, RevolvePick, RevolveState, axis_edge, axis_line, axis_of,
    axis_reach,
};
use crate::theme::SketchColors;
use crate::{Look, Message};

/// How wide the sketches' lines and axes are drawn while the axis is
/// picked, the one hovered, and the axis picked, in pixels.
const LINE_WIDTH: f32 = 1.5;
const HOVERED_WIDTH: f32 = 3.0;
const AXIS_WIDTH: f32 = 2.5;
/// The arrow at the axis's end: how long, and how wide either side, in
/// pixels.
const ARROW_LENGTH: f64 = 11.0;
const ARROW_HALF_WIDTH: f64 = 4.5;
/// How many degrees each step of the shaft's arc turns at most.
const SHAFT_STEP: f64 = 3.0;
/// How near to edge on the plane a knob turns in the cursor's ray may run
/// and still turn it, as the cosine between the ray and the axis: a
/// move's ring's.
const EDGE_ON: f64 = 1e-3;

/// The revolve being set up, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Revolving<'a> {
    /// Boxed, as a move's is: it's of the largest the viewport operates.
    state: Box<RevolveState<'a>>,
    handle: Option<RevolveHandle>,
}

/// What the viewport keeps of the revolve between events and frames.
#[derive(Default)]
pub(crate) struct Input {
    regions: regions::Input,
    /// The line or axis under the cursor as it last moved while the axis
    /// is picked, and its sketch.
    axis: Option<(FeatureId, AxisLine)>,
    /// The model edge that could be the axis under the cursor as it last
    /// moved while the axis is picked, and its ends.
    edge: Option<(u32, [DVec3; 2])>,
    /// The knob under the cursor as it last moved, if one is: then nothing
    /// else is.
    knob: Option<Angle>,
}

impl<'a> Revolving<'a> {
    pub(crate) fn new(state: RevolveState<'a>) -> Self {
        let handle = state.handle();
        Self {
            state: Box::new(state),
            handle,
        }
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

    /// Takes the mouse `event` with the `cursor` over `bounds` seen by
    /// `camera`: hovering and grabbing the knobs,
    /// which go first, and dragging the knob grabbed; hovering and picking
    /// regions, and while the axis is picked lines and axes, which go
    /// ahead of them. `None` for what's left to the camera: the left
    /// button pressed off them orbits.
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
                if let Some(angle) = self.state.grabbed {
                    // The raw position, so a drag goes on over the rest of
                    // the window.
                    let to = self.drag_to(angle, local(position), camera, bounds)?;
                    let look = RevolveLook::DragHandle { angle, to };
                    return Some(Action::publish(Message::Look(Look::Revolve(look))).and_capture());
                }
                let over = cursor.position_over(bounds).map(local);
                let knob = over.and_then(|at| self.knob_at(at, camera, bounds));
                if knob.is_some() {
                    let moved = std::mem::replace(&mut input.knob, knob) != knob;
                    let changed = moved
                        | input.axis.take().is_some()
                        | input.edge.take().is_some()
                        | input.regions.hover.take().is_some();
                    return Some(if changed {
                        Action::request_redraw().and_capture()
                    } else {
                        Action::capture()
                    });
                }
                let knob_left = input.knob.take().is_some();
                let axis = over.and_then(|at| self.axis_under(at, camera, bounds));
                let edge = over
                    .filter(|_| axis.is_none())
                    .and_then(|at| self.edge_under(at, camera, bounds))
                    .and_then(|(edge, _)| Some((edge, self.axis_edge(edge)?)));
                let region = over
                    .filter(|_| axis.is_none() && edge.is_none())
                    .and_then(|at| self.regions().region_under(at, camera, bounds));
                let changed = std::mem::replace(&mut input.axis, axis) != axis
                    || std::mem::replace(&mut input.edge, edge).map(|(e, _)| e)
                        != edge.map(|(e, _)| e)
                    || std::mem::replace(&mut input.regions.hover, region) != region;
                (changed || knob_left).then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => {
                let had = input.axis.take().is_some()
                    | input.edge.take().is_some()
                    | input.regions.hover.take().is_some()
                    | input.knob.take().is_some();
                had.then(Action::request_redraw)
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                self.state.grabbed?;
                let look = Look::Revolve(RevolveLook::DropHandle);
                Some(Action::publish(Message::Look(look)).and_capture())
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
                if let Some(angle) = self.knob_at(at, camera, bounds) {
                    let look = RevolveLook::GrabHandle(angle);
                    return Some(Action::publish(Message::Look(Look::Revolve(look))).and_capture());
                }
                let model = self.state.index.model();
                let edge = self.edge_under(at, camera, bounds);
                let look = match self.axis_under(at, camera, bounds) {
                    Some((sketch, axis)) => RevolveLook::PickAxis { sketch, axis },
                    None => match edge {
                        Some((edge, at)) if self.axis_edge(edge).is_some() => {
                            RevolveLook::PickEdge { model, edge, at }
                        }
                        _ => match self.regions().region_under(at, camera, bounds) {
                            Some((sketch, region)) => RevolveLook::PickRegion { sketch, region },
                            // Another edge, to be refused, saying why.
                            None => {
                                let (edge, at) = edge?;
                                RevolveLook::PickEdge { model, edge, at }
                            }
                        },
                    },
                };
                if !self.state.editable {
                    return None;
                }
                Some(Action::publish(Message::Look(Look::Revolve(look))).and_capture())
            }
            _ => None,
        }
    }

    /// The cursor dragging a knob, over one, or over a region, a line or
    /// an axis it would pick.
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
        let over = input.axis.is_some() || input.edge.is_some() || input.regions.hover.is_some();
        (over && self.state.editable).then_some(mouse::Interaction::Pointer)
    }

    /// A projector for what's in the world, seen by `camera` over
    /// `bounds`.
    fn projector(camera: &Camera, bounds: Rectangle) -> Option<Projector> {
        Projector::world(camera, bounds.width, bounds.height)
    }

    /// The pucks of the handle's knobs, seen by `camera` over `bounds`,
    /// those behind the eye left out: each at the regions' centre turned
    /// to its angle, its arrow the way it turns, on round past the end
    /// it's on.
    fn pucks(&self, camera: &Camera, bounds: Rectangle) -> Vec<Puck<Angle>> {
        let (Some(handle), Some(projector)) = (&self.handle, Self::projector(camera, bounds))
        else {
            return Vec::new();
        };
        (handle.knobs.iter())
            .filter_map(|&(angle, turn)| {
                let at = handle.at(turn);
                let back = turn < 0.0 || (turn == 0.0 && angle == Angle::Second);
                let out = handle.tangent(turn) * if back { -1.0 } else { 1.0 };
                Puck::new(angle, at, out, &projector)
            })
            .collect()
    }

    /// The knob under the screen position `at`, if the revolve can be
    /// changed ([`handle::knob_at`]).
    fn knob_at(&self, at: DVec2, camera: &Camera, bounds: Rectangle) -> Option<Angle> {
        if !self.state.editable {
            return None;
        }
        handle::knob_at(&self.pucks(camera, bounds), at)
    }

    /// Where the screen position `at` turns the knob of `angle`: the
    /// angle about the axis, in radians from the sketch plane, of where
    /// the cursor's ray meets the plane through the regions' centre square
    /// to the axis, the nearer way round from where the knob is, snapped
    /// to round degrees ([`angle_step`] of the arc's radius on the
    /// screen). `None` without a handle or the knob, with the plane edge
    /// on, or the ray meeting it behind the eye or on the axis.
    fn drag_to(&self, angle: Angle, at: DVec2, camera: &Camera, bounds: Rectangle) -> Option<f64> {
        let handle = self.handle.as_ref()?;
        let &(_, from) = handle.knobs.iter().find(|(knob, _)| *knob == angle)?;
        let projector = Self::projector(camera, bounds)?;
        let (origin, ray) = projector.ray(at)?;
        let facing = ray.dot(handle.axis);
        if facing.is_nan() || facing.abs() <= EDGE_ON * ray.length() {
            return None;
        }
        let t = (handle.origin - origin).dot(handle.axis) / facing;
        if projector.perspective() && t <= 0.0 {
            return None;
        }
        let p = origin + ray * t - handle.origin;
        let x = handle.radial.normalize_or_zero();
        let y = handle.axis.cross(x);
        let (u, v) = (p.dot(x), p.dot(y));
        if u == 0.0 && v == 0.0 {
            return None;
        }
        let to = from + wrapped(angle::atan2(v, u) - from);
        let pixel = projector.pixel_at(projector.world_depth(handle.at(from)));
        let step = angle_step(handle.radius() / pixel).to_radians();
        let to = (to / step).round() * step;
        to.is_finite().then_some(to)
    }

    /// The sketches whose lines and axes a click picks: none unless the
    /// axis is picked, then the source, or before there's one every
    /// candidate.
    fn axis_candidates(&self) -> impl Iterator<Item = &Candidate<'a>> {
        let picking = self.state.picking == RevolvePick::Axis;
        let source = self.state.source;
        (self.state.candidates.iter())
            .filter(move |candidate| picking && source.is_none_or(|s| s == candidate.feature))
    }

    /// The line or axis under the screen position `at` while the axis is
    /// picked, and its sketch: of those within [`HIT_PIXELS`] on each
    /// candidate's plane, the nearest by depth.
    fn axis_under(
        &self,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<(FeatureId, AxisLine)> {
        let mut nearest: Option<(f64, FeatureId, AxisLine)> = None;
        for candidate in self.axis_candidates() {
            let placement = candidate.placement;
            let Some(projector) = Projector::new(camera, placement, bounds.width, bounds.height)
            else {
                continue;
            };
            let Some(cursor) = projector.cursor(at) else {
                continue;
            };
            let sketch = candidate.sketch;
            let reach = axis_reach(sketch);
            let hit = hit::hit_axis(sketch, cursor.at, HIT_PIXELS * cursor.pixel, reach);
            let Some(axis) = hit.and_then(|id| axis_of(sketch, id)) else {
                continue;
            };
            let depth = projector.depth(cursor.at);
            if nearest.is_none_or(|(nearest, ..)| depth < nearest) {
                nearest = Some((depth, candidate.feature, axis));
            }
        }
        nearest.map(|(_, feature, axis)| (feature, axis))
    }

    /// The model edge under the screen position `at` while the axis is
    /// picked, and the point on it the cursor's at, if there's a source
    /// whose plane it could be in: the edge the model picks there.
    fn edge_under(&self, at: DVec2, camera: &Camera, bounds: Rectangle) -> Option<(u32, DVec3)> {
        if self.state.picking != RevolvePick::Axis {
            return None;
        }
        self.regions().source()?;
        let size = [bounds.width, bounds.height];
        let pick = (self.state.index).pick(camera, size, at, Picks::Edges)?;
        match pick.target {
            Picked::Edge(edge) => Some((edge, pick.at)),
            _ => None,
        }
    }

    /// The ends of edge `edge` of the model shown, if it can be the axis:
    /// straight and in the source's plane ([`axis_edge`]).
    fn axis_edge(&self, edge: u32) -> Option<[DVec3; 2]> {
        let source = self.regions().source()?;
        axis_edge(
            self.state.index,
            edge,
            &source.placement,
            self.state.resolution,
        )
        .ok()
    }

    /// The model's edges that can be the axis while it's picked, and
    /// their ends: none before there's a source.
    fn axis_edges(&self) -> Vec<(u32, [DVec3; 2])> {
        if self.state.picking != RevolvePick::Axis {
            return Vec::new();
        }
        let Some(source) = self.regions().source() else {
            return Vec::new();
        };
        let index = self.state.index;
        (0..index.mesh().edge_count())
            .filter_map(|edge| u32::try_from(edge).ok())
            .filter_map(|edge| {
                Some((
                    edge,
                    axis_edge(index, edge, &source.placement, self.state.resolution).ok()?,
                ))
            })
            .collect()
    }

    /// The axis picked as it's drawn on the source, from the end its arrow
    /// points away from to the end it's at: along the axis's direction,
    /// or against it flipped ([`RevolveState::reversed`]). Positive
    /// angles turn right-handed about the arrow. A model edge is drawn
    /// where the model shown has it, mapped onto the source's plane.
    fn pointed(&self) -> Option<(&Candidate<'a>, [DVec2; 2])> {
        let source = self.regions().source()?;
        let [start, end] = match self.state.axis? {
            AxisLine::Edge(_) => {
                let ends = self.state.edge_ends?;
                ends.map(|at| source.placement.to_sketch(at))
            }
            axis => drawn(source.sketch, axis)?,
        };
        let ends = if self.state.reversed() {
            [end, start]
        } else {
            [start, end]
        };
        Some((source, ends))
    }

    /// What the renderer draws of the revolve with `input` as it is, seen
    /// by `camera` over `bounds`: the base layer, the source's regions
    /// shaded and those picked marked; the live layer, the region
    /// hovered, before there's a source every candidate's regions, while
    /// the axis is picked the lines, axes and model edges a click picks
    /// (the one hovered stronger), and the axis picked with its arrow.
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
        for candidate in self.axis_candidates() {
            let Some(plane) = grid_plane(candidate.placement) else {
                continue;
            };
            let space = LayerSpace::On(plane);
            draw_targets(&mut live, space, candidate.sketch, colors);
            if let Some((_, axis)) = input.axis.filter(|(f, _)| *f == candidate.feature)
                && let Some(ends) = drawn(candidate.sketch, axis)
            {
                live.polyline(space, &ends, line(colors.hovered, HOVERED_WIDTH, false));
            }
        }
        let hovered_edge = input.edge.map(|(edge, _)| edge);
        for (edge, ends) in self.axis_edges() {
            let (color, width) = if hovered_edge == Some(edge) {
                (colors.hovered, HOVERED_WIDTH)
            } else {
                (colors.curve, LINE_WIDTH)
            };
            live.world_polyline(&ends.map(|at| at.as_vec3()), line(color, width, false));
        }
        if let Some((source, [from, to])) = self.pointed()
            && let Some(plane) = grid_plane(source.placement)
        {
            let space = LayerSpace::On(plane);
            // Lit as hovered while its row in the panel is.
            let color = if self.state.hover == Some(PanelHover::Axis) {
                colors.hovered
            } else {
                colors.selected
            };
            live.polyline(space, &[from, to], line(color, AXIS_WIDTH, false));
            let projector = Projector::new(camera, source.placement, bounds.width, bounds.height);
            if let Some((a, b)) = projector.and_then(|projector| projector.segment(from, to)) {
                arrow(&mut live, a, b, colors);
            }
        }
        self.draw_handle(&mut live, input, colors, camera, bounds);
        (base, live)
    }

    /// The handle into `live`: each knob's shaft round the axis from the
    /// sketch plane, unless the revolve's own check refuses it, and the
    /// pucks, the one hovered or dragged lighter with its rail round the
    /// axis, half a turn each way at most.
    fn draw_handle(
        &self,
        live: &mut SketchLayer,
        input: &Input,
        colors: SketchColors,
        camera: &Camera,
        bounds: Rectangle,
    ) {
        let (Some(handle), Some(projector)) = (&self.handle, Self::projector(camera, bounds))
        else {
            return;
        };
        if self.state.refused.is_none() {
            for &(_, turn) in &handle.knobs {
                let steps = (turn.abs().to_degrees() / SHAFT_STEP).ceil().max(1.0) as usize;
                let arc: Vec<DVec3> = (0..=steps)
                    .map(|k| handle.at(turn * k as f64 / steps as f64))
                    .collect();
                handle::draw_shaft(live, &projector, &arc, colors.handle);
            }
        }
        let active = self.state.grabbed.or(input.knob);
        let radius = handle.radius();
        for puck in self.pucks(camera, bounds) {
            let hot = active == Some(puck.knob) && self.state.editable;
            if hot && let Some(&(_, turn)) = handle.knobs.iter().find(|(k, _)| *k == puck.knob) {
                // An angle a pixel along the arc turns, at most half a turn
                // over the rail's reach.
                let per_pixel =
                    (puck.pixel / radius).min(std::f64::consts::PI / handle::RAIL_REACH);
                let path = |px: f64| handle.at(turn + px * per_pixel);
                handle::draw_rail(live, &projector, colors.rail, path);
            }
            let tone = handle::tone(colors, KnobTone::Create, hot);
            handle::draw_puck(live, &projector, &puck, tone);
        }
    }
}

/// The lines of `sketch` (construction ones dashed) and its two axes
/// (dashed, as far as [`axis_reach`]) into `live` on `space`: what a
/// click picks the axis from.
fn draw_targets(live: &mut SketchLayer, space: LayerSpace, sketch: &Sketch, colors: SketchColors) {
    for entry in &sketch.curves {
        let Curve::Line { start, end } = entry.curve else {
            continue;
        };
        let (Some(start), Some(end)) = (sketch.point(start), sketch.point(end)) else {
            continue;
        };
        let (color, dashed) = if entry.construction {
            (colors.construction, true)
        } else {
            (colors.curve, false)
        };
        live.polyline(space, &[start.at, end.at], line(color, LINE_WIDTH, dashed));
    }
    for axis in [AxisLine::SketchX, AxisLine::SketchY] {
        if let Some(ends) = drawn(sketch, axis) {
            live.polyline(space, &ends, line(colors.axis, LINE_WIDTH, true));
        }
    }
}

/// Where `axis` of `sketch` is drawn, in the sketch's coordinates, from
/// its start to its end along its direction: a line as it is, an axis
/// across [`axis_reach`] either side of the origin.
fn drawn(sketch: &Sketch, axis: AxisLine) -> Option<[DVec2; 2]> {
    let (at, along) = axis_line(sketch, axis)?;
    Some(match axis {
        AxisLine::Curve(_) | AxisLine::Edge(_) => [at, at + along],
        AxisLine::SketchX | AxisLine::SketchY => {
            let reach = axis_reach(sketch);
            [-along * reach, along * reach]
        }
    })
}

/// An arrowhead into `live` at `to`, on the screen, pointing from `from`.
fn arrow(live: &mut SketchLayer, from: DVec2, to: DVec2, colors: SketchColors) {
    let along = (to - from).normalize_or_zero();
    if along == DVec2::ZERO {
        return;
    }
    let base = to - along * ARROW_LENGTH;
    let across = along.perp() * ARROW_HALF_WIDTH;
    live.triangle(
        LayerSpace::Screen,
        [to, base + across, base - across],
        srgba(colors.selected),
    );
}

#[cfg(test)]
mod tests;
