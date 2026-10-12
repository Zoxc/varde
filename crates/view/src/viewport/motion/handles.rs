//! A move's handles: arrows along the world axes and knobs on rings about
//! them, laid out on the screen, grabbed and dragged.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use glam::{DVec2, DVec3};
use iced::widget::shader::Action;
use iced::{Rectangle, mouse};
use varde_document::{Axis3, MAX_COORD};
use varde_expr::Unit;
use varde_kernel_compat::Motion;
use varde_render::{Camera, Colors, SketchLayer, Space as LayerSpace};
use varde_sketch::angle;

use super::{HIT_PIXELS, Input, Moving};
use crate::extrude::snap_step;
use crate::hit::segment_distance;
use crate::motion::{MotionField, MotionKind, MotionLook, MotionPick};
use crate::projection::Projector;
use crate::theme::SketchColors;
use crate::viewport::handle::{self, Puck};
use crate::viewport::knobs;
use crate::viewport::sketch::line;
use crate::viewport::{drag_step, hover_action, local, local_cursor};
use crate::{Look, Message};

use crate::viewport::handle::{angle_step, wrapped};

/// How long the handles' arrows are on the screen, in pixels, and how
/// far out of their centre the rings' knobs are, on an orb drawn faintly
/// round them (the variants mock's "Big orb, short arrows").
const ARROW_PIXELS: f64 = 50.0;
pub(super) const ORB_PIXELS: f64 = 104.0;
/// How opaque the orb's outline is.
const ORB_ALPHA: f32 = 0.25;
/// An arrow shorter on the screen than this share of its length seen
/// square on (pointing within about 17° of the eye) is left out.
const MIN_FORESHORTENED: f64 = 0.3;
/// How far a ring's knob keeps clear of the arrows on the orb, and how
/// far apart knobs in one gap between arrows are at least, in pixels.
const KNOB_CLEAR: f64 = 31.0;
const KNOB_SEPARATION: f64 = 58.0;
/// A ring seen nearer edge on than this (the cosine between its axis and
/// the way to the eye) has no knob.
const RING_EDGE_ON: f64 = 0.09;
/// How far a ring's arc runs either side of its knob at most, in degrees,
/// and how far it keeps clear of its gap's edges, in pixels.
const ARC_DEGREES: f64 = 40.0;
const ARC_CLEAR: f64 = 12.0;
/// How many segments the orb is drawn with.
const RING_SEGMENTS: usize = 64;

/// A handle of a move: an arrow or a ring, of a world axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grip {
    Arrow(Axis3),
    Ring(Axis3),
}

/// A handle being dragged, as it was grabbed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Drag {
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

/// An arrow of the handles as it's laid out: along its axis's direction
/// or against it (`sign`), to `tip`, `ARROW_PIXELS` long on the screen.
#[derive(Debug, Clone)]
pub(super) struct ArrowShown {
    pub(super) axis: Axis3,
    pub(super) tip: DVec3,
    pub(super) puck: Puck<Grip>,
}

/// A ring's knob as it's laid out: on the orb, in a gap between the
/// arrows, with the stretch of its ring there (`arc`).
#[derive(Debug, Clone)]
pub(super) struct RingShown {
    pub(super) axis: Axis3,
    pub(super) arc: Vec<DVec3>,
    /// The ring's radius in millimetres, and where on it the knob is.
    pub(super) radius: f64,
    pub(super) at: f64,
    pub(super) puck: Puck<Grip>,
}

/// The handles as they're shown: where, a pixel's size there, which
/// rings, and the arrows and rings' knobs as they're laid out on the
/// screen.
#[derive(Debug, Clone)]
pub(super) struct Handles {
    pub(super) centre: DVec3,
    pixel: f64,
    pub(super) rings: [bool; 3],
    projector: Projector,
    pub(super) arrows: Vec<ArrowShown>,
    pub(super) knobs: Vec<RingShown>,
}

/// How a placing of rings' knobs in slots scores: more placed is better.
type Score<'s> = dyn Fn(&[(Axis3, usize)]) -> f64 + 's;

/// A stretch of the orb between two arrows that a ring's knob can take:
/// its middle and its ends, as angles on the screen.
#[derive(Debug, Clone, Copy)]
struct Slot {
    middle: f64,
    low: f64,
    high: f64,
}

impl Handles {
    /// The handles about `centre`, a pixel `pixel` there, seen through
    /// `projector`, with the rings `rings` says: the arrows laid out
    /// first, each flipped to whichever end spreads the three the most
    /// on the screen and left out pointing at the eye; then a knob for
    /// each ring not seen edge on, in the gaps between them on the orb,
    /// as many as fit.
    fn new(centre: DVec3, pixel: f64, rings: [bool; 3], projector: Projector) -> Self {
        let mut handles = Self {
            centre,
            pixel,
            rings,
            projector,
            arrows: Vec::new(),
            knobs: Vec::new(),
        };
        let origin = projector.show(centre);
        // Each axis's way on the screen, and how foreshortened it is.
        let seen: Vec<(Axis3, f64, f64)> = Axis3::ALL
            .into_iter()
            .filter_map(|axis| {
                let way = projector.show(centre + axis.direction()) - origin;
                let shown = way.length() * pixel;
                (shown >= MIN_FORESHORTENED && shown.is_finite())
                    .then(|| (axis, shown, angle::atan2(way.y, way.x)))
            })
            .collect();
        // The flips spreading them most: the least gap between them the
        // widest, flipping as few as that allows.
        let mut best: Option<(f64, usize)> = None;
        for mask in 0..1usize << seen.len() {
            let mut angles: Vec<f64> = (seen.iter().enumerate())
                .map(|(k, &(_, _, at))| turned(at + if mask >> k & 1 == 1 { PI } else { 0.0 }))
                .collect();
            angles.sort_by(f64::total_cmp);
            let gap = least_gap(&angles);
            let score = gap - 0.05 * f64::from(mask.count_ones());
            if best.is_none_or(|(best, _)| score > best) {
                best = Some((score, mask));
            }
        }
        let mask = best.map_or(0, |(_, mask)| mask);
        let mut rays = Vec::new();
        for (k, &(axis, shown, at)) in seen.iter().enumerate() {
            let sign = if mask >> k & 1 == 1 { -1.0 } else { 1.0 };
            let way = axis.direction() * sign;
            let tip = centre + way * (ARROW_PIXELS * pixel / shown);
            rays.push(turned(at + if sign < 0.0 { PI } else { 0.0 }));
            if let Some(puck) = Puck::new(Grip::Arrow(axis), tip, way, &projector) {
                handles.arrows.push(ArrowShown { axis, tip, puck });
            }
        }
        rays.sort_by(f64::total_cmp);
        handles.knobs = handles.lay_rings(&rays, origin);
        handles
    }

    /// The rings' knobs in the gaps between the arrows at `rays` (their
    /// angles on the screen, sorted), the centre showing at `origin`: of
    /// the ways to put the rings not seen edge on in the gaps' slots, the
    /// one placing the most, then with the largest least scale.
    fn lay_rings(&self, rays: &[f64], origin: DVec2) -> Vec<RingShown> {
        let projector = &self.projector;
        let (eye, backward) = projector.eye();
        let to_eye = if projector.perspective() {
            (eye - self.centre).normalize_or_zero()
        } else {
            backward
        };
        let axes: Vec<Axis3> = Axis3::ALL
            .into_iter()
            .filter(|&axis| {
                self.rings[axis_index(axis)] && axis.direction().dot(to_eye).abs() >= RING_EDGE_ON
            })
            .collect();
        let slots = slots(rays, ORB_PIXELS, KNOB_CLEAR, KNOB_SEPARATION);
        let toward =
            |axis: Axis3, middle: f64| ring_toward(projector, self.centre, origin, axis, middle);
        let mut best: Option<(f64, Vec<(Axis3, usize)>)> = None;
        let mut picked = Vec::new();
        fn search(
            axes: &[Axis3],
            slots: &[Slot],
            picked: &mut Vec<(Axis3, usize)>,
            score: &Score<'_>,
            best: &mut Option<(f64, Vec<(Axis3, usize)>)>,
        ) {
            let Some((&axis, rest)) = axes.split_first() else {
                let got = score(picked);
                if best.as_ref().is_none_or(|(best, _)| got > *best) {
                    *best = Some((got, picked.clone()));
                }
                return;
            };
            search(rest, slots, picked, score, best);
            for slot in 0..slots.len() {
                if picked.iter().any(|&(_, taken)| taken == slot) {
                    continue;
                }
                picked.push((axis, slot));
                search(rest, slots, picked, score, best);
                picked.pop();
            }
        }
        let score = |picked: &[(Axis3, usize)]| {
            let least = (picked.iter())
                .map(|&(axis, slot)| toward(axis, slots[slot].middle).1 * self.pixel)
                .fold(1.0, f64::min);
            picked.len() as f64 * 10.0 + least
        };
        search(&axes, &slots, &mut picked, &score, &mut best);
        let Some((_, picked)) = best else {
            return Vec::new();
        };
        (picked.into_iter())
            .filter_map(|(axis, slot)| self.ring_knob(axis, slots[slot], origin))
            .collect()
    }

    /// The knob of the ring of `axis` in `slot`, the centre showing at
    /// `origin`: where the ring runs through the slot's middle, with the
    /// ring's radius making that `ORB_PIXELS` out; and the stretch of the
    /// ring either side of it in the slot, `ARC_CLEAR` clear of its edges,
    /// at most `ARC_DEGREES` and the orb's radius long on the screen.
    fn ring_knob(&self, axis: Axis3, slot: Slot, origin: DVec2) -> Option<RingShown> {
        let projector = &self.projector;
        let (at, scale) = ring_toward(projector, self.centre, origin, axis, slot.middle);
        if !(scale > 0.0 && scale.is_finite()) {
            return None;
        }
        let radius = ORB_PIXELS / scale;
        let (u, v) = plane_axes(axis);
        let point =
            |turn: f64| self.centre + (u * angle::cos(turn) + v * angle::sin(turn)) * radius;
        let fits = |turn: f64| {
            let q = projector.show(point(turn)) - origin;
            let r = q.length();
            let along = slot.low + turned(angle::atan2(q.y, q.x) - slot.low);
            let clear = |by: f64| r * angle::sin(by.min(FRAC_PI_2));
            along <= slot.high
                && clear(along - slot.low) >= ARC_CLEAR
                && clear(slot.high - along) >= ARC_CLEAR
        };
        let side = |way: f64| {
            let mut points = Vec::new();
            let (mut run, mut last) = (0.0, projector.show(point(at)));
            let mut step = 2.0;
            while step <= ARC_DEGREES {
                let turn = at + way * step.to_radians();
                let shown = projector.show(point(turn));
                run += shown.distance(last);
                last = shown;
                if !fits(turn) || run > ORB_PIXELS {
                    break;
                }
                points.push(point(turn));
                step += 2.0;
            }
            points
        };
        let mut arc: Vec<DVec3> = side(-1.0).into_iter().rev().collect();
        arc.push(point(at));
        arc.extend(side(1.0));
        let tangent = v * angle::cos(at) - u * angle::sin(at);
        let puck = Puck::new(Grip::Ring(axis), point(at), tangent, projector)?;
        Some(RingShown {
            axis,
            arc,
            radius,
            at,
            puck,
        })
    }

    /// Where the world segment from `a` to `b` shows, unless it's behind
    /// the eye.
    fn shown(&self, a: DVec3, b: DVec3) -> Option<(DVec2, DVec2)> {
        let (a, b) = self.projector.in_front(a, b)?;
        Some((self.projector.show(a), self.projector.show(b)))
    }

    /// How far the screen position `at` is from the polyline through the
    /// world `points`, in pixels.
    pub(super) fn distance_to_polyline(&self, points: &[DVec3], at: DVec2) -> f64 {
        (points.windows(2))
            .filter_map(|pair| self.shown(pair[0], pair[1]))
            .map(|(a, b)| segment_distance(at, a, b))
            .fold(f64::INFINITY, f64::min)
    }

    /// The handle at the screen position `at`, if any: the nearest
    /// arrow (its puck, or its shaft within `HIT_PIXELS`) first, then the
    /// nearest ring's knob (its puck, or its arc).
    fn grip_at(&self, at: DVec2) -> Option<Grip> {
        let arrows = self.arrows.iter().map(|arrow| {
            let shaft = self.distance_to_polyline(&[self.centre, arrow.tip], at);
            let puck = arrow.puck.screen().distance(at);
            let reached = handle::knob_at(std::slice::from_ref(&arrow.puck), at).is_some();
            let distance = if reached { puck.min(shaft) } else { shaft };
            (Grip::Arrow(arrow.axis), distance)
        });
        let rings = self.knobs.iter().map(|knob| {
            let arc = self.distance_to_polyline(&knob.arc, at);
            let reached = handle::knob_at(std::slice::from_ref(&knob.puck), at).is_some();
            let distance = if reached { 0.0 } else { arc };
            (Grip::Ring(knob.axis), distance)
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
        let (u, _) = plane_axes(axis);
        (self.projector).angle_on_plane(centre, axis.direction(), u, at)
    }
}

/// `angle` brought into a turn from zero.
fn turned(angle: f64) -> f64 {
    angle.rem_euclid(TAU)
}

/// The least gap between the sorted `angles`, all the way round: a whole
/// turn for one or none.
fn least_gap(angles: &[f64]) -> f64 {
    if angles.len() < 2 {
        return TAU;
    }
    (0..angles.len())
        .map(|k| match angles.get(k + 1) {
            Some(next) => next - angles[k],
            None => angles[0] + TAU - angles[k],
        })
        .fold(TAU, f64::min)
}

/// The slots of the gaps between the arrows at `rays` (sorted angles on
/// the screen) on a circle `radius` pixels out: none nearer an arrow than
/// `clear` pixels, `separation` pixels apart at least, each gap's split
/// evenly, as many as fit (one in a gap that fits one). With no arrows, a
/// whole turn from the screen's right.
fn slots(rays: &[f64], radius: f64, clear: f64, separation: f64) -> Vec<Slot> {
    // The arcsine, by the crate's own arctangent.
    let asin = |x: f64| {
        let x = x.clamp(-1.0, 1.0);
        angle::atan2(x, (1.0 - x * x).sqrt())
    };
    let edge = asin(clear / radius);
    let apart = 2.0 * asin(separation / (2.0 * radius));
    let rays = if rays.is_empty() { &[0.0][..] } else { rays };
    let mut slots = Vec::new();
    for (k, &low) in rays.iter().enumerate() {
        let high = rays.get(k + 1).copied().unwrap_or(rays[0] + TAU);
        let room = high - low - 2.0 * edge;
        if room < 0.0 {
            continue;
        }
        let count = (room / apart).floor().max(1.0) as usize;
        for q in 0..count {
            let width = room / count as f64;
            let middle = low + edge + width * (q as f64 + 0.5);
            slots.push(Slot {
                middle,
                low: if q == 0 { low } else { middle - width / 2.0 },
                high: if q == count - 1 {
                    high
                } else {
                    middle + width / 2.0
                },
            });
        }
    }
    slots
}

/// Where the ring of `axis` about `centre` (showing at `origin`) runs the
/// way `angle` on the screen: the angle round it, from its plane's first
/// axis towards its second ([`plane_axes`]), and how many pixels a
/// millimetre of its radius makes there.
fn ring_toward(
    projector: &Projector,
    centre: DVec3,
    origin: DVec2,
    axis: Axis3,
    angle: f64,
) -> (f64, f64) {
    let (u, v) = plane_axes(axis);
    let p = projector.show(centre + u) - origin;
    let q = projector.show(centre + v) - origin;
    let way = DVec2::new(angle::cos(angle), angle::sin(angle));
    let mut turn = angle::atan2(-way.perp_dot(p), way.perp_dot(q));
    let mut reach = p * angle::cos(turn) + q * angle::sin(turn);
    if reach.dot(way) < 0.0 {
        turn += PI;
        reach = -reach;
    }
    (turn, reach.length())
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

/// `offset` turned by `degrees` about the line through `centre` along
/// the world axis `axis`, as a point: where a move's shift goes when its
/// turn about the parallel axis through the origin grows by `degrees`, so
/// the bodies turn about `centre`. A move maps `p` to `R·p + offset`
/// (`R` its turn about the origin's axis); turning that about `centre`
/// by `T` gives `T·R·p + T·(offset − centre) + centre`, and `T·R` is
/// the turn about the same axis by the angles' sum. The kernel's turn,
/// so quarter turns are exact.
pub(super) fn turned_about(
    offset: DVec3,
    centre: DVec3,
    axis: Axis3,
    degrees: f64,
) -> Option<DVec3> {
    Some(Motion::turn(centre, axis.direction(), degrees)?.point(offset))
}

/// An angle as a move stores it, in radians, in degrees: divided by the
/// factor a degree is typed with, as regenerating turns by it, so a
/// typed whole number of degrees comes back as typed.
fn degrees(radians: f64) -> f64 {
    radians / (PI / 180.0)
}

/// Draws `handles` on `live`: the rings, then the arrows' shafts and
/// knobs over them; each in its axis's colour from `scene`, the one
/// under the cursor or dragged in `colors`' hovered colour, the knobs the
/// accent's (`scene`'s selected colour) in a rim of the points' fill.
pub(super) fn draw_handles(
    live: &mut SketchLayer,
    handles: &Handles,
    input: &Input,
    scene: &Colors,
    colors: SketchColors,
) {
    let active = input.drag.map(|drag| drag.grip).or(input.hover);
    let projector = &handles.projector;
    // The orb, faintly, on the screen.
    let origin = projector.show(handles.centre);
    let orb: Vec<DVec2> = (0..=RING_SEGMENTS)
        .map(|k| {
            let turn = TAU * k as f64 / RING_SEGMENTS as f64;
            origin + DVec2::new(angle::cos(turn), angle::sin(turn)) * ORB_PIXELS
        })
        .collect();
    let faint = iced::Color {
        a: colors.rail.a * ORB_ALPHA,
        ..colors.rail
    };
    live.polyline(LayerSpace::Screen, &orb, line(faint, 1.0, false));
    // An axis's colour, lighter while its handle is hovered or dragged,
    // with the handles' accent.
    let tone = |grip: Grip, axis: Axis3| {
        let [r, g, b] = scene.axes[axis_index(axis)].0;
        let color = iced::Color::from_rgb(r, g, b);
        if active == Some(grip) {
            let lighter = |c: f32| c * 0.72 + 0.28;
            let lit = iced::Color::from_rgb(lighter(r), lighter(g), lighter(b));
            (lit, colors.handle_accent_hovered)
        } else {
            (color, colors.handle_accent)
        }
    };
    for knob in &handles.knobs {
        let grip = Grip::Ring(knob.axis);
        let (color, accent) = tone(grip, knob.axis);
        handle::draw_shaft(live, projector, &knob.arc, color);
        if active == Some(grip) {
            let (u, v) = plane_axes(knob.axis);
            let per_pixel = handle::arc_per_pixel(knob.puck.pixel, knob.radius);
            let path = |px: f64| {
                let turn = knob.at + px * per_pixel;
                handles.centre + (u * angle::cos(turn) + v * angle::sin(turn)) * knob.radius
            };
            handle::draw_rail(live, projector, colors.rail, path);
        }
        handle::draw_puck(live, projector, &knob.puck, (color, accent));
    }
    for arrow in &handles.arrows {
        let grip = Grip::Arrow(arrow.axis);
        let (color, accent) = tone(grip, arrow.axis);
        handle::draw_shaft(live, projector, &[handles.centre, arrow.tip], color);
        if active == Some(grip) {
            let way = arrow.axis.direction();
            let path = |px: f64| arrow.tip + way * (px * arrow.puck.pixel);
            handle::draw_rail(live, projector, colors.rail, path);
        }
        handle::draw_puck(live, projector, &arrow.puck, (color, accent));
    }
}

impl<'a> Moving<'a> {
    /// The offsets as they last read, none read as zero.
    fn offset(&self) -> DVec3 {
        let [x, y, z] = Axis3::ALL.map(|axis| self.field(MotionField::Offset(axis)).unwrap_or(0.0));
        DVec3::new(x, y, z)
    }

    /// The handles seen by `camera` over `bounds`, if there are any: a
    /// move's whose bodies the model shows, picking bodies, in a document
    /// that can be changed, their centre in front of the eye. At the
    /// bodies' pivot as the app has it, else their box's centre; kept
    /// while a handle is dragged (moved along with the offsets while an
    /// arrow is).
    pub(super) fn handles(
        &self,
        input: &Input,
        camera: &Camera,
        bounds: Rectangle,
    ) -> Option<Handles> {
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
        let projector = Projector::world_in(camera, bounds)?;
        let pixel = projector.pixel_at_point(centre)?;
        let angle = self.field(MotionField::Angle).unwrap_or(0.0);
        let rings = Axis3::ALL.map(|axis| angle == 0.0 || state.origin_axis == Some(axis));
        Some(Handles::new(centre, pixel, rings, projector))
    }

    /// [`Moving::mouse`] for a session picking none of its sketches: a
    /// move's handles.
    pub(super) fn handles_mouse(
        &self,
        input: &mut Input,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        hovered: bool,
    ) -> Option<Action<Message>> {
        let Some(handles) = self.handles(input, camera, bounds) else {
            input.hover = None;
            input.drag = None;
            return None;
        };
        match event {
            mouse::Event::CursorMoved { position } => {
                if let Some(drag) = &mut input.drag {
                    // The raw position, so a drag goes on over the rest
                    // of the window.
                    let message = self.drag(drag, &handles, local(bounds, position));
                    return Some(drag_step(message));
                }
                let over = local_cursor(bounds, cursor);
                let grip = over.and_then(|at| handles.grip_at(at));
                let changed = std::mem::replace(&mut input.hover, grip) != grip;
                // Just off the handles, a change redraws, and what's under
                // the cursor is worked out again as the frame is drawn.
                hover_action(grip.is_some(), changed, hovered)
            }
            mouse::Event::CursorLeft => input.hover.take().map(|_| Action::request_redraw()),
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let at = local_cursor(bounds, cursor)?;
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

    /// [`Moving::redraw`] for a session not picking a sweep's sketches:
    /// a move's handles or an operation's knobs.
    pub(super) fn handles_redraw(
        &self,
        input: &mut Input,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        camera: &Camera,
        hovered: bool,
    ) -> Option<Action<Message>> {
        if input.drag.is_some() || input.knobs.dragging() {
            return None;
        }
        if !self.state.knobs.is_empty() {
            let knobs = &self.state.knobs;
            return knobs::redraw(knobs, &mut input.knobs, bounds, cursor, camera, hovered);
        }
        let at = local_cursor(bounds, cursor);
        let grip = (self.handles(input, camera, bounds)).and_then(|handles| handles.grip_at(at?));
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
    /// last sent: an arrow's offset brought within what a move takes, a
    /// ring's turn only where it keeps the offset within it.
    fn drag(&self, drag: &mut Drag, handles: &Handles, at: DVec2) -> Option<MotionLook> {
        match drag.grip {
            Grip::Arrow(axis) => {
                let along = handles.along(drag.centre, axis, at)?;
                let step = snap_step(drag.pixel, self.state.units)?;
                let to = drag.offset[axis_index(axis)] + (along - drag.at);
                // Past what a move takes, the last step within it.
                let limit = (f64::from(MAX_COORD) / step).floor() * step;
                let to = ((to / step).round() * step).clamp(-limit, limit) + 0.0;
                if !to.is_finite() || to == drag.sent {
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
                let step = angle_step(ORB_PIXELS);
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
}
