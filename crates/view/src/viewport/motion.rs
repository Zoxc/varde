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
//! else their box's centre), a fixed size on the screen: an arrow along
//! each world axis, a shaft in the axis's colour with a knob at its end
//! as the extrude's handle has, and a ring about each. They take the mouse ahead of picking the model. Dragging an
//! arrow sets that axis's offset, where the cursor's ray passes nearest
//! the arrow's line, snapped as the extrude's handle ([`snap_step`] of a
//! pixel's size at the camera's target);
//! dragging a ring turns the bodies about that world axis by the angle
//! the cursor sweeps about the centre on the ring's plane, snapped to
//! round degrees ([`angle_step`]), shifting them so they turn about the
//! centre ([`MotionLook::Turn`]). While the move turns about another
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
//! ([`MotionLook::OffsetBy`]), so the preview and OK follow as for a
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

use std::f64::consts::{PI, TAU};
use std::sync::{Arc, LazyLock};

use glam::{DVec2, DVec3};
use iced::widget::shader::Action;
use iced::widget::{container, text};
use iced::{Element, Point, Rectangle, mouse};
use varde_document::{Axis3, FeatureId, MAX_COORD, OriginPlane, Placement};
use varde_expr::Unit;
use varde_kernel::Motion;
use varde_render::{Camera, Colors, GridPlane, PointStyle, SketchLayer, Space as LayerSpace, Srgb};
use varde_sketch::{Id, angle};

use super::regions::{self, Regions, grid_plane};
use super::sketch::{line, srgba};
use crate::anchors::Anchors;
use crate::extrude::snap_step;
use crate::hit::{self, segment_distance};
use crate::motion::{
    AlignView, FaceHandle, MotionField, MotionKind, MotionLook, MotionPick, MotionState, ScaleView,
    SplitMode, SplitView,
};
use crate::operation_panel::PanelHover;
use crate::projection::Projector;
use crate::theme::{self, SketchColors};
use crate::{Look, Message};

/// How wide the axis and the plane's outline are drawn, in pixels.
const LINE_WIDTH: f32 = 2.5;
/// The arrow at the axis's end: how long, and how wide either side, in
/// pixels.
const ARROW_LENGTH: f64 = 11.0;
const ARROW_HALF_WIDTH: f64 = 4.5;
/// How far the axis and the plane reach at least either side of the
/// bodies' centre, in millimetres, and how much past the bodies' box
/// they reach.
const MIN_REACH: f64 = 10.0;
const REACH_PAST: f64 = 1.25;
/// How opaque the plane's fill is.
const PLANE_FILL: f32 = 0.12;
/// How wide an align's points are drawn, in pixels: the measure tool's.
const ALIGN_POINT_RADIUS: f32 = 5.0;
/// How wide a split's sketches' curves are drawn, the one hovered, and
/// those picked for its line, in pixels: a revolve's lines'.
const CURVE_WIDTH: f32 = 1.5;
const HOVERED_CURVE_WIDTH: f32 = 3.0;
const PICKED_CURVE_WIDTH: f32 = 2.5;

/// How long the handles' arrows are, and the rings' radius, in pixels.
const ARROW_PIXELS: f64 = 100.0;
const RING_PIXELS: f64 = 70.0;
/// How wide the arrows' shafts and the rings are drawn, in pixels: the
/// extrude handle's shaft.
const SHAFT_WIDTH: f32 = 2.0;
/// The knob at an arrow's end, the extrude handle's: its radius and its
/// rim's width, in pixels.
const KNOB_RADIUS: f32 = 7.0;
const KNOB_RIM: f32 = 2.0;
/// How many segments a ring is drawn and hit tested with.
const RING_SEGMENTS: usize = 64;
/// How near the cursor a shaft or ring is grabbed, in pixels: a sketch's
/// hit tolerance; a knob, anywhere on it too.
const HIT_PIXELS: f64 = 6.0;
/// How long an arrow has to show, in pixels, to be dragged: shorter, it
/// runs nearly along the view, where the cursor says next to nothing of
/// how far along it.
const MIN_ARROW_PIXELS: f64 = 12.0;
/// How near to edge on a ring's plane the cursor's ray may run and still
/// turn it, as the cosine between the ray and the ring's axis.
const EDGE_ON: f64 = 1e-3;
/// How far apart a ring's snapping steps are at least, in pixels along
/// it: the extrude handle's steps' least.
const SNAP_PIXELS: f64 = 6.0;
/// The steps, in degrees, a ring's angle snaps to: the first at least
/// [`SNAP_PIXELS`] along the ring.
const ANGLE_STEPS: [f64; 8] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 45.0, 90.0];

/// A handle of a move: an arrow or a ring, of a world axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grip {
    Arrow(Axis3),
    Ring(Axis3),
}

/// A handle being dragged, as it was grabbed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    grip: Grip,
    /// The handles' centre, which the drag keeps where it was.
    centre: DVec3,
    /// A pixel's size at the camera's target, in millimetres, which an
    /// arrow's offset snaps by, as the extrude's handle's length does:
    /// the steps are the same wherever the bodies are (the handles' size
    /// is a pixel's at their centre).
    pixel: f64,
    /// The offsets, in millimetres, and the angle, in degrees, as the
    /// fields had them (none read as zero).
    offset: DVec3,
    angle: f64,
    /// An arrow's: how far along its line the cursor was. A ring's: the
    /// angle of the cursor about the centre on its plane, as last seen.
    at: f64,
    /// A ring's: how far the cursor has turned about the centre since
    /// it was grabbed, in radians, all the way round as often as it went.
    turned: f64,
    /// The value last sent: the offset, or the angle in degrees.
    sent: f64,
}

/// What the viewport keeps of a move between events and frames.
#[derive(Debug, Default)]
pub(crate) struct Input {
    /// The handle under the cursor, if any.
    hover: Option<Grip>,
    drag: Option<Drag>,
    /// A split's sketches' regions or curves.
    split: SplitInput,
    /// An offset face's handle.
    face: FaceInput,
}

/// What the viewport keeps of an offset face's handle: whether it's
/// under the cursor, and the drag.
#[derive(Debug, Default)]
struct FaceInput {
    hover: bool,
    drag: Option<FaceDrag>,
}

/// An offset face's handle being dragged, as it was grabbed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FaceDrag {
    /// The handle's origin and normal, kept while it's dragged.
    origin: DVec3,
    normal: DVec3,
    /// A pixel's size at the origin, in millimetres, which the distance
    /// snaps by.
    pixel: f64,
    /// Where the knob was, and how far along the line the cursor was.
    from: f64,
    grabbed: f64,
    /// The distance last sent, negative inward.
    sent: f64,
}

/// What the viewport keeps of a split's tool picked in its sketches: the
/// region under the cursor and the base layer of the source's regions,
/// and the curve under the cursor, with its sketch.
#[derive(Default)]
struct SplitInput {
    regions: regions::Input,
    curve: Option<(FeatureId, Id)>,
}

impl std::fmt::Debug for SplitInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitInput")
            .field("region", &self.regions.hover)
            .field("curve", &self.curve)
            .finish()
    }
}

impl Input {
    /// Whether the handles have the mouse: one is under the cursor or
    /// dragged, so the model isn't picked under them.
    pub(crate) fn holds(&self) -> bool {
        self.hover.is_some() || self.drag.is_some() || self.face.hover || self.face.drag.is_some()
    }

    /// Lets go of what the other kind of handle held, for a session of
    /// `kind`: an offset face's handle, or a move's. What a session left
    /// held (its handle under the cursor or dragged as it ended) would
    /// otherwise keep the next session's clicks off the model.
    fn settle(&mut self, kind: MotionKind) {
        if kind == MotionKind::OffsetFace {
            self.hover = None;
            self.drag = None;
        } else {
            self.face = FaceInput::default();
        }
    }
}

/// The handles as they're shown: where, a pixel's size there, and which
/// rings.
#[derive(Debug, Clone, Copy)]
struct Handles {
    centre: DVec3,
    pixel: f64,
    rings: [bool; 3],
    projector: Projector,
}

impl Handles {
    /// The end of the arrow of `axis`.
    fn arrow(&self, axis: Axis3) -> DVec3 {
        self.centre + axis.direction() * (ARROW_PIXELS * self.pixel)
    }

    /// The points around the ring of `axis`, the first again at the end.
    fn ring(&self, axis: Axis3) -> Vec<DVec3> {
        let (u, v) = plane_axes(axis);
        let radius = RING_PIXELS * self.pixel;
        (0..=RING_SEGMENTS)
            .map(|k| {
                let turn = TAU * k as f64 / RING_SEGMENTS as f64;
                self.centre + (u * angle::cos(turn) + v * angle::sin(turn)) * radius
            })
            .collect()
    }

    /// Where the world segment from `a` to `b` shows, unless it's behind
    /// the eye.
    fn shown(&self, a: DVec3, b: DVec3) -> Option<(DVec2, DVec2)> {
        let (a, b) = self.projector.in_front(a, b)?;
        Some((self.projector.show(a), self.projector.show(b)))
    }

    /// The handle at the screen position `at`, if any: the nearest
    /// arrow's knob or shaft first, then the nearest ring.
    fn grip_at(&self, at: DVec2) -> Option<Grip> {
        let arrows = Axis3::ALL.into_iter().filter_map(|axis| {
            let (a, b) = self.shown(self.centre, self.arrow(axis))?;
            if a.distance(b) < MIN_ARROW_PIXELS {
                return None;
            }
            let knob = (at.distance(b) - f64::from(KNOB_RADIUS)).max(0.0);
            Some((Grip::Arrow(axis), segment_distance(at, a, b).min(knob)))
        });
        let rings = Axis3::ALL.into_iter().filter_map(|axis| {
            if !self.rings[axis_index(axis)] {
                return None;
            }
            let ring = self.ring(axis);
            let distance = (ring.windows(2))
                .filter_map(|pair| self.shown(pair[0], pair[1]))
                .map(|(a, b)| segment_distance(at, a, b))
                .fold(f64::INFINITY, f64::min);
            Some((Grip::Ring(axis), distance))
        });
        nearest(arrows).or_else(|| nearest(rings))
    }

    /// How far along the line through `centre` along `axis` the cursor's
    /// ray at the screen position `at` passes nearest it, in
    /// millimetres from `centre`, as the extrude's handle is dragged
    /// ([`Projector::along_line`]). `None` looking along the line.
    fn along(&self, centre: DVec3, axis: Axis3, at: DVec2) -> Option<f64> {
        self.projector.along_line(centre, axis.direction(), at)
    }

    /// The angle about `centre`, in radians, of where the cursor's ray at
    /// the screen position `at` meets the plane through it square to
    /// `axis`, from the plane's first axis towards its second
    /// ([`plane_axes`]): right-handed about `axis`. `None` with the plane
    /// edge on, or the ray meeting it behind the eye or at the centre.
    fn angle_at(&self, centre: DVec3, axis: Axis3, at: DVec2) -> Option<f64> {
        let (origin, ray) = self.projector.ray(at)?;
        let normal = axis.direction();
        let facing = ray.dot(normal);
        if facing.is_nan() || facing.abs() <= EDGE_ON * ray.length() {
            return None;
        }
        let t = (centre - origin).dot(normal) / facing;
        if self.projector.perspective() && t <= 0.0 {
            return None;
        }
        let p = origin + ray * t - centre;
        let (u, v) = plane_axes(axis);
        let (x, y) = (p.dot(u), p.dot(v));
        let turn = angle::atan2(y, x);
        (turn.is_finite() && (x != 0.0 || y != 0.0)).then_some(turn)
    }
}

/// The nearest of `grips`, each with how far from the cursor it is in
/// pixels, within [`HIT_PIXELS`].
fn nearest(grips: impl Iterator<Item = (Grip, f64)>) -> Option<Grip> {
    grips
        .filter(|&(_, distance)| distance <= HIT_PIXELS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(grip, _)| grip)
}

/// Where `axis` is kept in an array of the three.
fn axis_index(axis: Axis3) -> usize {
    match axis {
        Axis3::X => 0,
        Axis3::Y => 1,
        Axis3::Z => 2,
    }
}

/// Two world axes spanning the plane square to `axis`, the first crossed
/// with the second giving it.
fn plane_axes(axis: Axis3) -> (DVec3, DVec3) {
    match axis {
        Axis3::X => (DVec3::Y, DVec3::Z),
        Axis3::Y => (DVec3::Z, DVec3::X),
        Axis3::Z => (DVec3::X, DVec3::Y),
    }
}

/// The step, in degrees, a ring `radius` pixels across snaps its angle
/// to: the first of [`ANGLE_STEPS`] at least [`SNAP_PIXELS`] along it,
/// or the last.
fn angle_step(radius: f64) -> f64 {
    let least = (SNAP_PIXELS / radius).to_degrees();
    (ANGLE_STEPS.into_iter())
        .find(|&step| step >= least)
        .unwrap_or(ANGLE_STEPS[ANGLE_STEPS.len() - 1])
}

/// `turn` brought within half a turn either way.
fn wrapped(turn: f64) -> f64 {
    let turn = turn % TAU;
    if turn > PI {
        turn - TAU
    } else if turn <= -PI {
        turn + TAU
    } else {
        turn
    }
}

/// `offset` turned by `degrees` about the line through `centre` along
/// the world axis `axis`, as a point: where a move's shift goes when its
/// turn about the parallel axis through the origin grows by `degrees`, so
/// the bodies turn about `centre`. A move maps `p` to `R·p + offset`
/// (`R` its turn about the origin's axis); turning that about `centre`
/// by `T` gives `T·R·p + T·(offset − centre) + centre`, and `T·R` is
/// the turn about the same axis by the angles' sum. The kernel's turn,
/// so quarter turns are exact.
fn turned_about(offset: DVec3, centre: DVec3, axis: Axis3, degrees: f64) -> Option<DVec3> {
    Some(Motion::turn(centre, axis.direction(), degrees)?.point(offset))
}

/// An angle as a move stores it, in radians, in degrees: divided by the
/// factor a degree is typed with, as regenerating turns by it, so a
/// typed whole number of degrees comes back as typed.
fn degrees(radians: f64) -> f64 {
    radians / (PI / 180.0)
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

    /// The bodies' centre and how far the axis or plane reaches either
    /// side of it: a quarter past half the box's diagonal, at least
    /// [`MIN_REACH`]; the reference's point and that least reach without
    /// a box.
    fn extent(&self, at: DVec3) -> (DVec3, f64) {
        match self.state.bounds {
            Some([low, high]) if low.is_finite() && high.is_finite() => {
                let reach = (high - low).length() / 2.0 * REACH_PAST;
                ((low + high) / 2.0, reach.max(MIN_REACH))
            }
            _ => (at, MIN_REACH),
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
        static EMPTY: LazyLock<Arc<SketchLayer>> = LazyLock::new(Arc::default);
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
        if let Some(handle) = self.face_handle(input.face.drag) {
            let active = input.face.hover || input.face.drag.is_some();
            draw_face_handle(&mut live, &handle, active, scene, colors);
        }
        if let Some(align) = &self.state.align {
            self.align(&mut live, align, scene, colors, camera, bounds);
        }
        if let Some(scale) = &self.state.scale {
            scale_marks(&mut live, scale, scene, colors);
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
                let hovered = input.split.curve.filter(|_| picking.is_some());
                split_lines(&mut live, split, hovered, picking.is_some(), colors);
            }
        }
        (EMPTY.clone(), live)
    }

    /// The kind of a split's tool picked in its sketches, while one is:
    /// regions or a line, which then take the left button.
    fn split_picking(&self) -> Option<SplitMode> {
        let split = self.state.split.as_ref()?;
        let picking = self.state.picking == MotionPick::Tool && self.state.editable;
        (picking && matches!(split.mode, SplitMode::Regions | SplitMode::Line))
            .then_some(split.mode)
    }

    /// The curve of a split's sketches under the screen position `at`, and
    /// its sketch: of those within [`HIT_PIXELS`] on each sketch's plane,
    /// the nearest by depth.
    fn curve_under(
        &self,
        split: &SplitView<'_>,
        at: DVec2,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<(FeatureId, Id)> {
        let mut nearest: Option<(f64, FeatureId, Id)> = None;
        for candidate in &split.lines {
            let Some(projector) =
                Projector::new(camera, candidate.placement, bounds.width, bounds.height)
            else {
                continue;
            };
            let Some(cursor) = projector.cursor(at) else {
                continue;
            };
            let tolerance = HIT_PIXELS * cursor.pixel;
            let Some(curve) = hit::hit_curve(candidate.sketch, cursor.at, tolerance) else {
                continue;
            };
            let depth = projector.depth(cursor.at);
            if nearest.is_none_or(|(nearest, ..)| depth < nearest) {
                nearest = Some((depth, candidate.feature, curve));
            }
        }
        nearest.map(|(_, feature, curve)| (feature, curve))
    }

    /// Takes the mouse `event` while a split's tool is picked in its
    /// sketches ([`Moving::split_picking`]): hovering and clicking the
    /// region or curve under the cursor. `None` for what's left to the
    /// camera: the left button pressed off them orbits.
    fn split_mouse(
        &self,
        mode: SplitMode,
        input: &mut SplitInput,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
    ) -> Option<Action<Message>> {
        let split = self.state.split.as_ref()?;
        let local = |p: Point| DVec2::new((p.x - bounds.x).into(), (p.y - bounds.y).into());
        let regions = split_regions(split);
        match event {
            mouse::Event::CursorMoved { .. } => {
                let over = cursor.position_over(bounds).map(local);
                let (region, curve) = match mode {
                    SplitMode::Regions => (
                        over.and_then(|at| regions.region_under(at, camera, bounds)),
                        None,
                    ),
                    _ => (
                        None,
                        over.and_then(|at| self.curve_under(split, at, camera, bounds)),
                    ),
                };
                let changed = std::mem::replace(&mut input.regions.hover, region) != region
                    || std::mem::replace(&mut input.curve, curve) != curve;
                changed.then(Action::request_redraw)
            }
            mouse::Event::CursorLeft => {
                let had = input.regions.hover.take().is_some() | input.curve.take().is_some();
                had.then(Action::request_redraw)
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
                let look = match mode {
                    SplitMode::Regions => {
                        let (sketch, region) = regions.region_under(at, camera, bounds)?;
                        MotionLook::SplitRegion { sketch, region }
                    }
                    _ => {
                        let (sketch, curve) = self.curve_under(split, at, camera, bounds)?;
                        MotionLook::SplitCurve { sketch, curve }
                    }
                };
                Some(Action::publish(Message::Look(Look::Motion(look))).and_capture())
            }
            _ => None,
        }
    }

    /// The labels of the pieces the preview of a split shows, each its
    /// body's name in a chip at the middle of its box, seen by `camera`,
    /// the one keeping the body's id in the accent: none while there are
    /// none.
    pub(crate) fn labels(&self, camera: &Camera) -> Option<Element<'a, Message>> {
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

    /// An align's points and directions, each side's: its point as a dot,
    /// its direction as an arrow from it across the bodies, its second
    /// direction as a shorter dashed one; the moved side's in the accent,
    /// the target's in the second colour. While a point is picked, the
    /// snap points of what the cursor is over, as the measure tool's.
    fn align(
        &self,
        live: &mut SketchLayer,
        align: &AlignView<'_>,
        scene: &Colors,
        colors: SketchColors,
        camera: &Camera,
        bounds: Rectangle,
    ) {
        let opaque = |Srgb([r, g, b])| iced::Color::from_rgb(r, g, b);
        let dot = |radius: f32, rim: iced::Color, fixed: bool| PointStyle {
            radius,
            rim_width: 1.5,
            rim: srgba(rim),
            fill: srgba(colors.point_fill),
            fixed,
        };
        for (mark, color) in align.marks.iter().zip([scene.selected, scene.second]) {
            let color = opaque(color);
            let Some(point) = mark.point.filter(|point| point.is_finite()) else {
                continue;
            };
            let (_, reach) = self.extent(point);
            for (k, direction) in mark.directions.iter().enumerate() {
                let Some(along) = direction.and_then(DVec3::try_normalize) else {
                    continue;
                };
                // The second direction shorter and dashed.
                let (length, dashed) = if k == 0 {
                    (reach, false)
                } else {
                    (reach / 2.0, true)
                };
                let tip = point + along * length;
                live.world_polyline(
                    &[point.as_vec3(), tip.as_vec3()],
                    line(color, LINE_WIDTH, dashed),
                );
                self.arrowhead(live, point, tip, color, camera, bounds);
            }
            live.world_point(point.as_vec3(), dot(ALIGN_POINT_RADIUS, color, true));
        }
        if let Some((index, hover)) = align.snaps {
            super::measure::snap_dots(live, index, hover, colors);
        }
    }

    /// An arrowhead on the screen at `to`, the end of the line from
    /// `from`.
    fn arrowhead(
        &self,
        live: &mut SketchLayer,
        from: DVec3,
        to: DVec3,
        color: iced::Color,
        camera: &Camera,
        bounds: Rectangle,
    ) {
        let placement = OriginPlane::XY.placement();
        let Some(projector) = Projector::new(camera, placement, bounds.width, bounds.height) else {
            return;
        };
        let (a, b) = (projector.show(from), projector.show(to));
        let towards = (b - a).normalize_or_zero();
        if towards == DVec2::ZERO || !b.is_finite() {
            return;
        }
        let base = b - towards * ARROW_LENGTH;
        let across = towards.perp() * ARROW_HALF_WIDTH;
        live.triangle(
            LayerSpace::Screen,
            [b, base + across, base - across],
            srgba(color),
        );
    }

    /// The handles seen by `camera` over `bounds`, if there are any: a
    /// move's whose bodies the model shows, picking bodies, in a document
    /// that can be changed, their centre in front of the eye. At the
    /// bodies' pivot as the app has it, else their box's centre; kept
    /// while a handle is dragged (moved along with the offsets while an
    /// arrow is).
    fn handles(&self, input: &Input, camera: &Camera, bounds: Rectangle) -> Option<Handles> {
        let state = &self.state;
        if state.kind != MotionKind::Move || state.picking != MotionPick::Bodies || !state.editable
        {
            return None;
        }
        let [low, high] = state.bounds?;
        let centre = match input.drag {
            Some(
                drag @ Drag {
                    grip: Grip::Arrow(_),
                    ..
                },
            ) => drag.centre + (self.offset() - drag.offset),
            Some(drag) => drag.centre,
            None => state.centre.unwrap_or((low + high) / 2.0),
        };
        if !centre.is_finite() {
            return None;
        }
        let placement = OriginPlane::XY.placement();
        let projector = Projector::new(camera, placement, bounds.width, bounds.height)?;
        let depth = projector.world_depth(centre);
        if projector.perspective() && (depth.is_nan() || depth <= projector.near()) {
            return None;
        }
        let pixel = projector.pixel_at(depth);
        if !(pixel > 0.0 && pixel.is_finite()) {
            return None;
        }
        let angle = self.field(MotionField::Angle).unwrap_or(0.0);
        let rings = Axis3::ALL.map(|axis| angle == 0.0 || state.origin_axis == Some(axis));
        Some(Handles {
            centre,
            pixel,
            rings,
            projector,
        })
    }

    /// The value of `field` as it last read, if it reads.
    fn field(&self, field: MotionField) -> Option<f64> {
        self.state.fields[field.index()].value
    }

    /// The offsets as they last read, none read as zero.
    fn offset(&self) -> DVec3 {
        let [x, y, z] = Axis3::ALL.map(|axis| self.field(MotionField::Offset(axis)).unwrap_or(0.0));
        DVec3::new(x, y, z)
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
        input.settle(self.state.kind);
        if let Some(mode) = self.split_picking() {
            return self.split_mouse(mode, &mut input.split, event, bounds, cursor, camera);
        }
        if self.state.kind == MotionKind::OffsetFace {
            return self.face_mouse(&mut input.face, event, bounds, cursor, camera, hovered);
        }
        let Some(handles) = self.handles(input, camera, bounds) else {
            input.hover = None;
            input.drag = None;
            return None;
        };
        let local = |p: Point| DVec2::new((p.x - bounds.x).into(), (p.y - bounds.y).into());
        match event {
            mouse::Event::CursorMoved { position } => {
                if let Some(drag) = &mut input.drag {
                    // The raw position, so a drag goes on over the rest
                    // of the window.
                    let message = self.drag(drag, &handles, local(position));
                    return Some(
                        match message {
                            Some(look) => Action::publish(Message::Look(Look::Motion(look))),
                            None => Action::capture(),
                        }
                        .and_capture(),
                    );
                }
                let over = cursor.position_over(bounds).map(local);
                let grip = over.and_then(|at| handles.grip_at(at));
                let changed = std::mem::replace(&mut input.hover, grip) != grip;
                match grip {
                    Some(_) if hovered => {
                        Some(Action::publish(Message::Look(Look::Hover(None))).and_capture())
                    }
                    Some(_) if changed => Some(Action::request_redraw().and_capture()),
                    Some(_) => Some(Action::capture()),
                    // Just off the handles: redrawn, and what's under the
                    // cursor worked out again as the frame is drawn.
                    None if changed => Some(Action::request_redraw()),
                    None => None,
                }
            }
            mouse::Event::CursorLeft => input.hover.take().map(|_| Action::request_redraw()),
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
                let grip = handles.grip_at(at)?;
                input.drag = Some(self.grab(grip, &handles, at)?);
                input.hover = Some(grip);
                Some(Action::request_redraw().and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                input.drag.take()?;
                Some(Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }

    /// Works out again which handle is under the `cursor` as a frame is
    /// drawn, as picking the model does: the camera, or the handles with
    /// the bodies, may have moved under a cursor that didn't, which would
    /// leave the model unpicked under a handle no longer there. While
    /// one's under it, what the model held hovered, if `hovered`, is let
    /// go of. Not while one is dragged.
    pub(crate) fn redraw(
        &self,
        input: &mut Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        hovered: bool,
    ) -> Option<Action<Message>> {
        input.settle(self.state.kind);
        if input.drag.is_some() || input.face.drag.is_some() {
            return None;
        }
        if self.state.kind == MotionKind::OffsetFace {
            let over = cursor.position_over(bounds).is_some_and(|at| {
                let at = DVec2::new((at.x - bounds.x).into(), (at.y - bounds.y).into());
                self.face_handle(input.face.drag)
                    .is_some_and(|handle| face_handle_at(&handle, camera, bounds, at))
            });
            input.face.hover = over;
            return (over && hovered).then(|| Action::publish(Message::Look(Look::Hover(None))));
        }
        let at = cursor.position_over(bounds);
        let grip = (self.handles(input, camera, bounds)).and_then(|handles| {
            let at = at?;
            handles.grip_at(DVec2::new(
                (at.x - bounds.x).into(),
                (at.y - bounds.y).into(),
            ))
        });
        input.hover = grip;
        (grip.is_some() && hovered).then(|| Action::publish(Message::Look(Look::Hover(None))))
    }

    /// `grip` grabbed with the cursor at `at`, of `handles` as they are:
    /// `None` where the cursor says nothing of where it is along an
    /// arrow, or on a ring's plane.
    fn grab(&self, grip: Grip, handles: &Handles, at: DVec2) -> Option<Drag> {
        let centre = handles.centre;
        let offset = self.offset();
        let angle = degrees(self.field(MotionField::Angle).unwrap_or(0.0));
        let (from, sent) = match grip {
            Grip::Arrow(axis) => (handles.along(centre, axis, at)?, offset[axis_index(axis)]),
            Grip::Ring(axis) => (handles.angle_at(centre, axis, at)?, angle),
        };
        Some(Drag {
            grip,
            centre,
            pixel: handles.projector.pixel(),
            offset,
            angle,
            at: from,
            turned: 0.0,
            sent,
        })
    }

    /// The handle `drag` dragged to the screen position `at`: the
    /// message setting what it sets there, if that's changed since it was
    /// last sent, and within what a move takes.
    fn drag(&self, drag: &mut Drag, handles: &Handles, at: DVec2) -> Option<MotionLook> {
        match drag.grip {
            Grip::Arrow(axis) => {
                let along = handles.along(drag.centre, axis, at)?;
                let step = snap_step(drag.pixel, self.state.units)?;
                let to = drag.offset[axis_index(axis)] + (along - drag.at);
                let to = (to / step).round() * step + 0.0;
                if !(to.is_finite() && to.abs() <= f64::from(MAX_COORD)) || to == drag.sent {
                    return None;
                }
                drag.sent = to;
                let units = Unit::Length(self.state.units);
                Some(MotionLook::Input {
                    field: MotionField::Offset(axis),
                    text: varde_expr::format(to, Some(units)),
                })
            }
            Grip::Ring(axis) => {
                let now = handles.angle_at(drag.centre, axis, at)?;
                drag.turned += wrapped(now - drag.at);
                drag.at = now;
                let step = angle_step(RING_PIXELS);
                let to = drag.angle + degrees(drag.turned);
                // Within a turn either way, as a move takes.
                let to = ((to / step).round() * step % 360.0) + 0.0;
                if !to.is_finite() || to == drag.sent {
                    return None;
                }
                let shift = turned_about(drag.offset, drag.centre, axis, to - drag.angle)?;
                let limit = f64::from(MAX_COORD);
                if !(shift.is_finite() && shift.abs().max_element() <= limit) {
                    return None;
                }
                drag.sent = to;
                let units = Some(Unit::Length(self.state.units));
                Some(MotionLook::Turn {
                    axis,
                    angle: format!("{to}°"),
                    offset: [shift.x, shift.y, shift.z].map(|v| varde_expr::format(v, units)),
                })
            }
        }
    }

    /// The cursor over a handle, or dragging one; over a region or curve
    /// a split's tool picks.
    pub(crate) fn mouse_interaction(&self, input: &Input) -> Option<mouse::Interaction> {
        if self.split_picking().is_some() {
            let over = input.split.regions.hover.is_some() || input.split.curve.is_some();
            return over.then_some(mouse::Interaction::Pointer);
        }
        if input.drag.is_some() || input.face.drag.is_some() {
            Some(mouse::Interaction::Grabbing)
        } else if input.face.hover {
            Some(mouse::Interaction::Grab)
        } else {
            input.hover.map(|_| mouse::Interaction::Grab)
        }
    }

    /// An offset face's handle, if it has one and the document can be
    /// changed: kept where it was grabbed while it's dragged, the knob at
    /// the distance as it last read.
    fn face_handle(&self, drag: Option<FaceDrag>) -> Option<FaceHandle> {
        let state = &self.state;
        if state.kind != MotionKind::OffsetFace || !state.editable {
            return None;
        }
        let mut handle = state.offset_face.as_ref()?.handle?;
        if let Some(drag) = drag {
            handle.origin = drag.origin;
            handle.normal = drag.normal;
        }
        (handle.origin.is_finite() && handle.normal.is_finite() && handle.at.is_finite())
            .then_some(handle)
    }

    /// Takes the mouse `event` for an offset face's handle: hovering,
    /// grabbing and dragging it. `None` for what's left to picking the
    /// model's faces and the camera: anything off the handle. While the
    /// cursor is over it, what the model held hovered, if `hovered`, is
    /// let go of.
    fn face_mouse(
        &self,
        input: &mut FaceInput,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        hovered: bool,
    ) -> Option<Action<Message>> {
        let Some(handle) = self.face_handle(input.drag) else {
            input.hover = false;
            input.drag = None;
            return None;
        };
        let local = |p: Point| DVec2::new((p.x - bounds.x).into(), (p.y - bounds.y).into());
        let placement = OriginPlane::XY.placement();
        let projector = Projector::new(camera, placement, bounds.width, bounds.height)?;
        match event {
            mouse::Event::CursorMoved { position } => {
                if let Some(drag) = &mut input.drag {
                    let look = self.face_drag(drag, &projector, local(position));
                    return Some(
                        match look {
                            Some(look) => Action::publish(Message::Look(Look::Motion(look))),
                            None => Action::capture(),
                        }
                        .and_capture(),
                    );
                }
                let over = cursor
                    .position_over(bounds)
                    .is_some_and(|at| face_handle_at(&handle, camera, bounds, local(at)));
                let changed = std::mem::replace(&mut input.hover, over) != over;
                match (over, changed) {
                    (true, _) if hovered => {
                        Some(Action::publish(Message::Look(Look::Hover(None))).and_capture())
                    }
                    (true, true) => Some(Action::request_redraw().and_capture()),
                    (true, false) => Some(Action::capture()),
                    (false, true) => Some(Action::request_redraw()),
                    (false, false) => None,
                }
            }
            mouse::Event::CursorLeft => {
                std::mem::take(&mut input.hover).then(Action::request_redraw)
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local(cursor.position_over(bounds)?);
                if !face_handle_at(&handle, camera, bounds, at) {
                    return None;
                }
                let grabbed = projector.along_line(handle.origin, handle.normal, at)?;
                let depth = projector.world_depth(handle.origin);
                let pixel = projector.pixel_at(depth);
                if !(pixel > 0.0 && pixel.is_finite()) {
                    return None;
                }
                input.drag = Some(FaceDrag {
                    origin: handle.origin,
                    normal: handle.normal,
                    pixel,
                    from: handle.at,
                    grabbed,
                    sent: handle.at,
                });
                input.hover = true;
                Some(Action::request_redraw().and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                input.drag.take()?;
                Some(Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }

    /// An offset face's handle `drag` dragged to the screen position
    /// `at`: the message setting the distance and side there, if that's
    /// changed since it was last sent, within the coordinate limit and
    /// not zero (which no distance is: the knob passes it), nor shown as
    /// zero in the design's units (zoomed in far, the snap is finer than
    /// they show).
    fn face_drag(
        &self,
        drag: &mut FaceDrag,
        projector: &Projector,
        at: DVec2,
    ) -> Option<MotionLook> {
        let along = projector.along_line(drag.origin, drag.normal, at)?;
        let step = snap_step(drag.pixel, self.state.units)?;
        let to = drag.from + (along - drag.grabbed);
        let to = (to / step).round() * step + 0.0;
        if !(to.is_finite() && to.abs() <= f64::from(MAX_COORD)) || to == 0.0 || to == drag.sent {
            return None;
        }
        let units = Some(Unit::Length(self.state.units));
        let distance = varde_expr::format(to.abs(), units);
        if distance == varde_expr::format(0.0, units) {
            return None;
        }
        drag.sent = to;
        Some(MotionLook::OffsetBy {
            distance,
            inward: to < 0.0,
        })
    }

    /// The axis through `point` along the unit `along`, across the bodies,
    /// with its arrowhead.
    fn axis(
        &self,
        live: &mut SketchLayer,
        point: DVec3,
        along: DVec3,
        color: iced::Color,
        camera: &Camera,
        bounds: Rectangle,
    ) {
        let (centre, reach) = self.extent(point);
        let middle = point + along * (centre - point).dot(along);
        let [from, to] = [middle - along * reach, middle + along * reach];
        live.world_polyline(
            &[from.as_vec3(), to.as_vec3()],
            line(color, LINE_WIDTH, false),
        );
        self.arrowhead(live, from, to, color, camera, bounds);
    }

    /// The plane through `point` square to the unit `normal`, as a square
    /// across the bodies, and a short line along its normal from its
    /// middle.
    fn plane(&self, live: &mut SketchLayer, point: DVec3, normal: DVec3, color: iced::Color) {
        let (centre, reach) = self.extent(point);
        let middle = centre - normal * (centre - point).dot(normal);
        let x = normal.any_orthonormal_vector();
        let y = normal.cross(x);
        let Some(plane) = GridPlane::new(middle.as_vec3(), x.as_vec3(), y.as_vec3()) else {
            return;
        };
        let space = LayerSpace::On(plane);
        let corners = [
            DVec2::new(-reach, -reach),
            DVec2::new(reach, -reach),
            DVec2::new(reach, reach),
            DVec2::new(-reach, reach),
            DVec2::new(-reach, -reach),
        ];
        let mut fill = srgba(color);
        fill.0[3] = PLANE_FILL;
        live.fill(space, [&corners[..4]], fill);
        live.polyline(space, &corners, line(color, LINE_WIDTH, true));
        let tip = middle + normal * (reach / 4.0);
        live.world_polyline(
            &[middle.as_vec3(), tip.as_vec3()],
            line(color, LINE_WIDTH, false),
        );
    }

    /// Where its layers are drawn: the world's XY plane, as the measure
    /// tool's, since it draws in the world; a split's regions' source's
    /// plane, whose regions its base layer draws.
    pub(crate) fn plane_of_layers(&self) -> GridPlane {
        (self.state.split.as_ref())
            .filter(|split| split.mode == SplitMode::Regions)
            .map_or(GridPlane::XY, |split| split_regions(split).plane())
    }
}

/// A split's sketches whose regions show, and those picked.
fn split_regions<'s, 'a>(split: &'s SplitView<'a>) -> Regions<'s, 'a> {
    Regions {
        candidates: &split.candidates,
        source: split.source,
        picked: split.picked,
        panel: None,
    }
}

/// Draws a split's line on `live`: while it's picked (`picking`), the
/// curves of the sketches it may be of (construction ones dashed, the
/// one `hovered` stronger), and the curves picked in the selected colour.
fn split_lines(
    live: &mut SketchLayer,
    split: &SplitView<'_>,
    hovered: Option<(FeatureId, Id)>,
    picking: bool,
    colors: SketchColors,
) {
    let picked = |feature: FeatureId, id: Id| {
        split
            .chain
            .is_some_and(|(sketch, curves)| sketch == feature && curves.binary_search(&id).is_ok())
    };
    for candidate in &split.lines {
        let Some(plane) = grid_plane(candidate.placement) else {
            continue;
        };
        let space = LayerSpace::On(plane);
        let sketch = candidate.sketch;
        for entry in &sketch.curves {
            let is_picked = picked(candidate.feature, entry.id);
            let is_hovered = hovered == Some((candidate.feature, entry.id));
            if !(picking || is_picked) {
                continue;
            }
            let Some(points) = sketch.flatten(&entry.curve) else {
                continue;
            };
            let (color, width) = if is_hovered {
                (colors.hovered, HOVERED_CURVE_WIDTH)
            } else if is_picked {
                (colors.selected, PICKED_CURVE_WIDTH)
            } else if entry.construction {
                (colors.construction, CURVE_WIDTH)
            } else {
                (colors.curve, CURVE_WIDTH)
            };
            let dashed = entry.construction && !is_picked && !is_hovered;
            live.polyline(space, &points, line(color, width, dashed));
        }
    }
}

/// Draws a scale's point on `live`, a dot in the accent as an align's
/// moved point is (its edge is lit in the model's highlight), and while
/// the point is picked the snap points of what the cursor is over, as the
/// measure tool's.
fn scale_marks(
    live: &mut SketchLayer,
    scale: &ScaleView<'_>,
    scene: &Colors,
    colors: SketchColors,
) {
    if let Some(point) = scale.at.filter(|point| point.is_finite()) {
        let Srgb([r, g, b]) = scene.selected;
        let style = PointStyle {
            radius: ALIGN_POINT_RADIUS,
            rim_width: 1.5,
            rim: srgba(iced::Color::from_rgb(r, g, b)),
            fill: srgba(colors.point_fill),
            fixed: true,
        };
        live.world_point(point.as_vec3(), style);
    }
    if let Some((index, hover)) = scale.snaps {
        super::measure::snap_dots(live, index, hover, colors);
    }
}

/// Draws `handles` on `live`: the rings, then the arrows' shafts and
/// knobs over them; each in its axis's colour from `scene`, the one
/// under the cursor or dragged in `colors`' hovered colour, the knobs the
/// accent's (`scene`'s selected colour) in a rim of the points' fill.
fn draw_handles(
    live: &mut SketchLayer,
    handles: &Handles,
    input: &Input,
    scene: &Colors,
    colors: SketchColors,
) {
    let active = input.drag.map(|drag| drag.grip).or(input.hover);
    let color = |grip: Grip, axis: Axis3| {
        if active == Some(grip) {
            colors.hovered
        } else {
            let [r, g, b] = scene.axes[axis_index(axis)].0;
            iced::Color::from_rgb(r, g, b)
        }
    };
    for axis in Axis3::ALL {
        if handles.rings[axis_index(axis)] {
            let ring: Vec<_> = handles.ring(axis).iter().map(|p| p.as_vec3()).collect();
            let grip = Grip::Ring(axis);
            live.world_polyline(&ring, line(color(grip, axis), SHAFT_WIDTH, false));
        }
    }
    let [r, g, b] = scene.selected.0;
    let accent = iced::Color::from_rgb(r, g, b);
    for axis in Axis3::ALL {
        let grip = Grip::Arrow(axis);
        let tip = handles.arrow(axis);
        let shaft = [handles.centre.as_vec3(), tip.as_vec3()];
        live.world_polyline(&shaft, line(color(grip, axis), SHAFT_WIDTH, false));
        let fill = if active == Some(grip) {
            colors.hovered
        } else {
            accent
        };
        let knob = PointStyle {
            radius: KNOB_RADIUS,
            rim_width: KNOB_RIM,
            rim: srgba(colors.point_fill),
            fill: srgba(fill),
            fixed: false,
        };
        live.world_point(tip.as_vec3(), knob);
    }
}

/// Whether the screen position `at` is on an offset face's `handle`
/// seen by `camera` over `bounds`: within [`HIT_PIXELS`] of its shaft,
/// or on its knob.
fn face_handle_at(handle: &FaceHandle, camera: &Camera, bounds: Rectangle, at: DVec2) -> bool {
    let placement = OriginPlane::XY.placement();
    let Some(projector) = Projector::new(camera, placement, bounds.width, bounds.height) else {
        return false;
    };
    let tip = handle.origin + handle.normal * handle.at;
    let Some((a, b)) = projector.in_front(handle.origin, tip) else {
        return false;
    };
    let (a, b) = (projector.show(a), projector.show(b));
    let knob = (at.distance(b) - f64::from(KNOB_RADIUS)).max(0.0);
    segment_distance(at, a, b).min(knob) <= HIT_PIXELS
}

/// Draws an offset face's `handle` on `live`: its shaft and its knob, as
/// a move's arrows are, in the selected colour, or the hovered one while
/// it's under the cursor or dragged (`active`).
fn draw_face_handle(
    live: &mut SketchLayer,
    handle: &FaceHandle,
    active: bool,
    scene: &Colors,
    colors: SketchColors,
) {
    let [r, g, b] = scene.selected.0;
    let accent = iced::Color::from_rgb(r, g, b);
    let color = if active { colors.hovered } else { accent };
    let tip = handle.origin + handle.normal * handle.at;
    live.world_polyline(
        &[handle.origin.as_vec3(), tip.as_vec3()],
        line(color, SHAFT_WIDTH, false),
    );
    let knob = PointStyle {
        radius: KNOB_RADIUS,
        rim_width: KNOB_RIM,
        rim: srgba(colors.point_fill),
        fill: srgba(color),
        fixed: false,
    };
    live.world_point(tip.as_vec3(), knob);
}

#[cfg(test)]
mod tests;
