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
//! right-handed about (against the line or edge when flipped). No
//! handle. The renderer depth tests all of it, so the model hides what's
//! behind it; the arrow, in screen space, is on top.

use std::sync::Arc;

use glam::{DVec2, DVec3};
use iced::widget::shader::Action;
use iced::{Point, Rectangle, mouse};
use varde_document::{AxisLine, FeatureId};
use varde_render::{Camera, GridPlane, SketchLayer, Space as LayerSpace};
use varde_sketch::{Curve, Sketch};

use super::regions::{self, Regions, grid_plane};
use super::sketch::{line, srgba};
use crate::hit;
use crate::operation_panel::{Candidate, PanelHover};
use crate::pick::{Picked, Picks};
use crate::projection::Projector;
use crate::revolve::{
    RevolveLook, RevolvePick, RevolveState, axis_edge, axis_line, axis_of, axis_reach, on_sketch,
};
use crate::theme::SketchColors;
use crate::{Look, Message};

/// How near the cursor a line or an axis is picked, in pixels: a
/// sketch's own hit tolerance.
const HIT_PIXELS: f64 = 6.0;
/// How wide the sketches' lines and axes are drawn while the axis is
/// picked, the one hovered, and the axis picked, in pixels.
const LINE_WIDTH: f32 = 1.5;
const HOVERED_WIDTH: f32 = 3.0;
const AXIS_WIDTH: f32 = 2.5;
/// The arrow at the axis's end: how long, and how wide either side, in
/// pixels.
const ARROW_LENGTH: f64 = 11.0;
const ARROW_HALF_WIDTH: f64 = 4.5;

/// The revolve being set up, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Revolving<'a> {
    state: RevolveState<'a>,
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
}

impl<'a> Revolving<'a> {
    pub(crate) fn new(state: RevolveState<'a>) -> Self {
        Self { state }
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
    /// `camera`: hovering and picking regions, and while the axis is
    /// picked lines and axes, which go first. `None` for what's left to
    /// the camera: the left button pressed off them orbits.
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
            mouse::Event::CursorMoved { .. } => {
                let over = cursor.position_over(bounds).map(local);
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
                changed.then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => {
                let had = input.axis.take().is_some()
                    | input.edge.take().is_some()
                    | input.regions.hover.take().is_some();
                had.then(Action::request_redraw)
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
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

    /// The cursor over a region, a line or an axis it would pick.
    pub(crate) fn mouse_interaction(
        &self,
        input: &Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<mouse::Interaction> {
        cursor.position_over(bounds)?;
        let over = input.axis.is_some() || input.edge.is_some() || input.regions.hover.is_some();
        (over && self.state.editable).then_some(mouse::Interaction::Pointer)
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
        axis_edge(self.state.index, edge, &source.placement).ok()
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
            .filter_map(|edge| Some((edge, axis_edge(index, edge, &source.placement).ok()?)))
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
                ends.map(|at| on_sketch(&source.placement, at))
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
        (base, live)
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
