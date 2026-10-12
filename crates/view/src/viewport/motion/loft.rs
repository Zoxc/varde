//! A loft's sections and rails picked in their sketches, and its seam
//! knobs.

use glam::DVec2;
use iced::widget::shader::Action;
use iced::widget::{container, text};
use iced::{Element, Rectangle, mouse};
use varde_document::{FeatureId, OriginPlane, Placement};
use varde_render::{Camera, Colors, PointStyle, SketchLayer, Space as LayerSpace};
use varde_sketch::Id;

use super::{HIT_PIXELS, Input, Moving};
use crate::anchors::Anchors;
use crate::motion::{KnobTone, LoftShape, LoftView, MotionLook, MotionPick, SketchLines};
use crate::operation_panel::PanelHover;
use crate::projection::Projector;
use crate::theme::{self, SketchColors};
use crate::viewport::handle::{self, Puck};
use crate::viewport::regions::{Regions, grid_plane};
use crate::viewport::sketch::{fill_region_in, line, mark_dot, opaque, srgba};
use crate::viewport::sketch_pick::{
    Points, SELECTED_WIDTH, SketchHit, curve_under, loose, point_under,
};
use crate::viewport::{drag_step, hover_action, local, local_cursor};
use crate::{Look, Message};

use super::sweep::sweep_lines;

/// How opaque a loft's sections are filled.
const PICKED_FILL: f32 = 0.35;
/// How wide a loft's sections' corners and its points on their own are
/// drawn while its sections are picked, and its start dots (and the
/// corner or point under the cursor), in pixels.
const CORNER_RADIUS: f32 = 3.5;
const START_RADIUS: f32 = 5.5;
/// What the viewport keeps of a loft's sections and rails picked in
/// their sketches: what's under the cursor, the corner of a section (by
/// its place, and the sketch point there) taking the click ahead of a
/// sketch point on its own, and that ahead of a region; or, while its
/// rails are picked, the curve.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct LoftInput {
    pub(super) corner: Option<(usize, Id)>,
    point: Option<(FeatureId, Id)>,
    pub(super) region: Option<(FeatureId, usize)>,
    pub(super) curve: Option<(FeatureId, Id)>,
}

/// What the viewport keeps of a loft's seam knobs: the section whose
/// start's knob is under the cursor, and the one dragged, by their place,
/// with its start as grabbed or last sent.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct SeamInput {
    pub(super) hover: Option<usize>,
    pub(super) drag: Option<(usize, Option<Id>)>,
}

impl LoftInput {
    /// Whether anything is under the cursor.
    pub(super) fn over(&self) -> bool {
        *self != Self::default()
    }
}

/// A loft's sketches whose regions show, each on its own plane (no
/// source: its sections are drawn apart), and the one hovered in the
/// panel (`hover`).
fn loft_regions<'s, 'a>(loft: &'s LoftView<'a>, hover: Option<PanelHover>) -> Regions<'s, 'a> {
    Regions {
        candidates: &loft.candidates,
        source: None,
        picked: &crate::NO_REGIONS,
        panel: hover.and_then(PanelHover::region),
    }
}

/// The corner of a loft's section under the screen position `at`, by the
/// section's place and the sketch point there: of those within
/// [`HIT_PIXELS`] on the screen, the nearest.
fn corner_under(
    loft: &LoftView<'_>,
    at: DVec2,
    camera: &Camera,
    bounds: Rectangle,
) -> Option<(usize, Id)> {
    let mut nearest: Option<(f64, usize, Id)> = None;
    for (index, section) in loft.sections.iter().enumerate() {
        let (Some(placement), LoftShape::Region { corners, .. }) =
            (section.placement, &section.shape)
        else {
            continue;
        };
        let Some(projector) = Projector::new(camera, placement, bounds.width, bounds.height) else {
            continue;
        };
        for &(id, corner) in corners {
            let Some(shown) = projector.project(corner) else {
                continue;
            };
            let distance = shown.distance(at);
            if distance <= HIT_PIXELS && nearest.is_none_or(|(nearest, ..)| distance < nearest) {
                nearest = Some((distance, index, id));
            }
        }
    }
    nearest.map(|(_, index, id)| (index, id))
}

/// The start of `loft`'s section `section`, if it's a region's with one.
fn loft_start(loft: &LoftView<'_>, section: usize) -> Option<Id> {
    match &loft.sections.get(section)?.shape {
        LoftShape::Region { start, .. } => *start,
        LoftShape::Point(_) => None,
    }
}

/// The seam knobs of `loft` as `projector` shows them: each region
/// section's start, its arrow along the loop towards its next corner (its
/// sketch's x for a loop of one corner), by the section's place.
fn seam_pucks(loft: &LoftView<'_>, projector: &Projector) -> Vec<Puck<usize>> {
    let mut pucks = Vec::new();
    for (index, section) in loft.sections.iter().enumerate() {
        let (Some(placement), LoftShape::Region { corners, start, .. }) =
            (section.placement, &section.shape)
        else {
            continue;
        };
        let Some(k) = (*start).and_then(|start| corners.iter().position(|&(id, _)| id == start))
        else {
            continue;
        };
        let at = placement.to_world(corners[k].1);
        let next = placement.to_world(corners[(k + 1) % corners.len()].1);
        let out = (next - at).try_normalize().unwrap_or(placement.x);
        pucks.extend(Puck::new(index, at, out, projector));
    }
    pucks
}

/// The corner of `loft`'s section `section` nearest the screen position
/// `at`, its sketch point.
fn nearest_corner(
    loft: &LoftView<'_>,
    section: usize,
    at: DVec2,
    camera: &Camera,
    bounds: Rectangle,
) -> Option<Id> {
    let section = loft.sections.get(section)?;
    let (Some(placement), LoftShape::Region { corners, .. }) = (section.placement, &section.shape)
    else {
        return None;
    };
    let projector = Projector::new(camera, placement, bounds.width, bounds.height)?;
    (corners.iter())
        .filter_map(|&(id, corner)| Some((id, projector.project(corner)?.distance(at))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Draws the points on their own of `lines`' sketches on `live`, as a
/// sketch's points, the one `hovered` in the hovered colour.
fn loose_points(
    live: &mut SketchLayer,
    lines: &[SketchLines<'_>],
    hovered: Option<(FeatureId, Id)>,
    colors: SketchColors,
) {
    for candidate in lines {
        for (id, point) in loose(candidate.sketch) {
            let lit = hovered == Some((candidate.feature, id));
            let radius = if lit { START_RADIUS } else { CORNER_RADIUS };
            let rim = if lit { colors.hovered } else { colors.curve };
            let style = mark_dot(radius, rim, colors, false);
            let at = candidate.placement.to_world(point);
            live.world_point(at.as_vec3(), style);
        }
    }
}

/// The numbers of a loft's sections, each in a chip by its start dot
/// (or its region's middle) seen by `camera`: none while there are none.
pub(super) fn loft_labels<'a>(
    loft: &LoftView<'_>,
    camera: &Camera,
) -> Option<Element<'a, Message>> {
    let chips: Vec<Element<'a, Message>> = (loft.sections.iter().enumerate())
        .filter_map(|(at, section)| {
            let placement = section.placement?;
            let middle = match &section.shape {
                LoftShape::Region {
                    region: Some(region),
                    ..
                } => (region.bounds.0 + region.bounds.1) / 2.0,
                shape => shape.dot()?,
            };
            let world = placement.to_world(middle);
            if !world.is_finite() {
                return None;
            }
            let chip = container(text((at + 1).to_string()).size(12))
                .padding([1, 4])
                .style(|theme| theme::glyph(theme, false));
            let placement = Placement {
                origin: world,
                ..OriginPlane::XY.placement()
            };
            Some(Element::from(Anchors::new(
                *camera,
                placement,
                [(DVec2::ZERO, chip.into())],
            )))
        })
        .collect();
    (!chips.is_empty()).then(|| iced::widget::stack(chips).into())
}

impl<'a> Moving<'a> {
    /// The section whose seam knob is under the screen position `at`,
    /// by its place, if the loft can be changed.
    pub(super) fn seam_under(
        &self,
        loft: &LoftView<'_>,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<usize> {
        if !self.state.editable {
            return None;
        }
        let projector = Projector::world_in(camera, bounds)?;
        handle::knob_at(&seam_pucks(loft, &projector), at)
    }

    /// Takes the mouse `event` for a loft's seam knobs, with the `cursor`
    /// over `bounds` seen by `camera`, ahead of what the loft picks: over
    /// one, the cursor is a grab hand; a press grabs it, and dragged, its
    /// section's start moves to the corner nearest the cursor
    /// ([`MotionLook::LoftStart`]), each time it's another. `None` for
    /// what's left to the rest: anything off them.
    pub(super) fn seams_mouse(
        &self,
        input: &mut SeamInput,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let Some(loft) = &self.state.loft else {
            *input = SeamInput::default();
            return None;
        };
        match event {
            mouse::Event::CursorMoved { position } => {
                if let Some((section, sent)) = &mut input.drag {
                    let section = *section;
                    let look =
                        nearest_corner(loft, section, local(bounds, position), camera, bounds)
                            .filter(|&point| *sent != Some(point))
                            .map(|point| {
                                *sent = Some(point);
                                MotionLook::LoftStart { section, point }
                            });
                    return Some(drag_step(look));
                }
                let over = (cursor.position_over(bounds))
                    .and_then(|at| self.seam_under(loft, local(bounds, at), camera, bounds));
                let was = std::mem::replace(&mut input.hover, over);
                hover_action(over.is_some(), was != over, false)
            }
            mouse::Event::CursorLeft if input.drag.is_none() => {
                input.hover.take().map(|_| Action::request_redraw())
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local_cursor(bounds, cursor)?;
                let section = self.seam_under(loft, at, camera, bounds)?;
                *input = SeamInput {
                    hover: Some(section),
                    drag: Some((section, loft_start(loft, section))),
                };
                Some(Action::request_redraw().and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                input.drag.take()?;
                Some(Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }

    /// What a loft picks in its sketches, while it does: its sections
    /// (regions, sketch points and its sections' corners) or its rails'
    /// curves. The model isn't picked meanwhile.
    pub(super) fn loft_picking(&self) -> Option<MotionPick> {
        self.state.loft.as_ref()?;
        let picking = self.state.picking;
        (self.state.editable && matches!(picking, MotionPick::Regions | MotionPick::Path))
            .then_some(picking)
    }

    /// What's under the screen position `at` for a loft picking
    /// `picking`: see [`LoftInput`].
    pub(super) fn loft_under(
        &self,
        loft: &LoftView<'_>,
        picking: MotionPick,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> LoftInput {
        if picking == MotionPick::Path {
            return LoftInput {
                curve: curve_under(&loft.lines, at, camera, bounds, None).map(SketchHit::of),
                ..LoftInput::default()
            };
        }
        if let Some(corner) = corner_under(loft, at, camera, bounds) {
            return LoftInput {
                corner: Some(corner),
                ..LoftInput::default()
            };
        }
        if let Some(point) =
            point_under(&loft.lines, at, camera, bounds, Points::Loose, None).map(SketchHit::of)
        {
            return LoftInput {
                point: Some(point),
                ..LoftInput::default()
            };
        }
        LoftInput {
            region: loft_regions(loft, None).region_under(at, camera, bounds),
            ..LoftInput::default()
        }
    }

    /// Takes the mouse `event` while a loft picks in its sketches
    /// ([`Moving::loft_picking`]): hovering and clicking what's under the
    /// cursor ([`LoftInput`]). `None` for what's left to the camera: the
    /// left button pressed off them orbits.
    pub(super) fn loft_mouse(
        &self,
        picking: MotionPick,
        input: &mut LoftInput,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let loft = self.state.loft.as_ref()?;
        match event {
            mouse::Event::CursorMoved { .. } => {
                let over = local_cursor(bounds, cursor);
                let under = over.map_or_else(LoftInput::default, |at| {
                    self.loft_under(loft, picking, at, camera, bounds)
                });
                (std::mem::replace(input, under) != under).then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => {
                let had = std::mem::take(input).over();
                had.then(Action::request_redraw)
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local_cursor(bounds, cursor)?;
                let under = self.loft_under(loft, picking, at, camera, bounds);
                let look = if let Some((section, point)) = under.corner {
                    MotionLook::LoftStart { section, point }
                } else if let Some((sketch, point)) = under.point {
                    MotionLook::LoftPoint { sketch, point }
                } else if let Some((sketch, region)) = under.region {
                    MotionLook::LoftRegion { sketch, region }
                } else {
                    let (sketch, curve) = under.curve?;
                    MotionLook::LoftRail { sketch, curve }
                };
                Some(Action::publish(Message::Look(Look::Motion(look))).and_capture())
            }
            _ => None,
        }
    }

    /// Draws a loft on `live`: while its sections are picked, every
    /// candidate sketch's regions (the one under the cursor filled
    /// stronger) and its points on their own; its sections' regions
    /// filled and outlined in the selected colour (the one whose row is
    /// hovered in the hovered colour), their corners while picking, and
    /// a point section's point in the accent, each region section's start
    /// as its seam knob (a teal puck, its arrow along the loop; the one
    /// under the cursor or dragged lighter, with its section's corners),
    /// the corner under the cursor in the hovered colour; its rails as a
    /// sweep's path.
    pub(super) fn loft_layers(
        &self,
        live: &mut SketchLayer,
        loft: &LoftView<'_>,
        input: &Input,
        scene: &Colors,
        colors: SketchColors,
        (camera, bounds): (&Camera, Rectangle),
    ) {
        let (input, seams) = (&input.loft, input.seams);
        let held = seams.drag.map(|(section, _)| section).or(seams.hover);
        let picking = self.loft_picking();
        let sections = picking == Some(MotionPick::Regions);
        if sections {
            loft_regions(loft, None).live(input.region, colors, live);
            loose_points(live, &loft.lines, input.point, colors);
        }
        let panel = match self.state.hover {
            Some(PanelHover::Section(at)) => Some(at),
            _ => None,
        };
        let accent = opaque(scene.selected);
        let dot = |radius: f32, rim: iced::Color| mark_dot(radius, rim, colors, true);
        for (at, section) in loft.sections.iter().enumerate() {
            let Some(placement) = section.placement else {
                continue;
            };
            let lit = panel == Some(at);
            let color = if lit { colors.hovered } else { colors.profile };
            if let LoftShape::Region {
                region: Some(region),
                corners,
                ..
            } = &section.shape
                && let Some(plane) = grid_plane(placement)
            {
                let space = LayerSpace::On(plane);
                fill_region_in(live, space, region, color.scale_alpha(PICKED_FILL));
                for polyline in &region.outline {
                    let mut closed = polyline.clone();
                    closed.extend(polyline.first().copied());
                    live.polyline(space, &closed, line(color, SELECTED_WIDTH, false));
                }
                if sections || held == Some(at) {
                    for &(id, corner) in corners {
                        let hovered = input.corner == Some((at, id));
                        let rim = if hovered {
                            colors.hovered
                        } else {
                            colors.curve
                        };
                        let radius = if hovered { START_RADIUS } else { CORNER_RADIUS };
                        live.world_point(placement.to_world(corner).as_vec3(), dot(radius, rim));
                    }
                }
            }
            if matches!(section.shape, LoftShape::Region { .. }) {
                continue;
            }
            if let Some(start) = section.shape.dot() {
                let color = if lit { colors.hovered } else { accent };
                let style = PointStyle {
                    fill: srgba(color),
                    fixed: false,
                    ..dot(START_RADIUS, colors.point_fill)
                };
                live.world_point(placement.to_world(start).as_vec3(), style);
            }
        }
        if let Some(projector) = Projector::world_in(camera, bounds) {
            for puck in seam_pucks(loft, &projector) {
                let hot = held == Some(puck.knob);
                let tone = handle::tone(colors, KnobTone::Count, hot);
                handle::draw_puck(live, &projector, &puck, tone);
            }
        }
        let hovered = input.curve.filter(|_| picking == Some(MotionPick::Path));
        let part = match self.state.hover {
            Some(PanelHover::Part(at)) => loft.chains.get(at).copied(),
            _ => None,
        };
        let rails = picking == Some(MotionPick::Path);
        if rails || !loft.chains.is_empty() {
            sweep_lines(
                live,
                &loft.lines,
                &loft.chains,
                hovered,
                part,
                rails,
                colors,
            );
        }
    }
}
