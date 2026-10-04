//! The knobs of the handle of an operation set up in the move's session
//! ([`OpKnob`], the app's), in the viewport: each drawn as the extrude's
//! knob is (`handle.rs`), a puck with its shaft from where its value is
//! zero and, hovered or dragged, its rail along its path, all on the
//! screen over the model, in its tool's colours. They take the mouse
//! ahead of picking the model, as a move's handles do: over one, the
//! cursor is a grab hand and the model's hover is let go of; a press
//! grabs it. Dragged along a line, its value is where the cursor's ray
//! passes nearest the line, on from where it was grabbed; round an arc,
//! the angle the cursor turns about the axis on the arc's plane, all the
//! way round as often as it goes. Snapped as the knob says
//! ([`KnobSnap`]), each new value is sent ([`MotionLook::DragKnob`]) for
//! the app to type into the knob's field. While one is dragged, its path
//! is kept as it was grabbed, so the knob doesn't run off as the preview
//! moves what it stands on.

use glam::{DVec2, DVec3};
use iced::widget::shader::Action;
use iced::{Point, Rectangle, mouse};
use varde_document::OriginPlane;
use varde_expr::LengthUnit;
use varde_render::{Camera, SketchLayer};
use varde_sketch::angle;

use super::handle::{self, Puck};
use super::motion::{angle_step, wrapped};
use crate::extrude::snap_step;
use crate::motion::{KnobPath, KnobRadius, KnobSnap, MotionLook, OpKnob};
use crate::projection::Projector;
use crate::theme::SketchColors;
use crate::{Look, Message};

/// How near to edge on an arc's plane the cursor's ray may run and still
/// turn its knob, as the cosine between the ray and the axis: a move's
/// ring's.
const EDGE_ON: f64 = 1e-3;
/// How many segments a whole turn of an arc's shaft and rail is drawn
/// with.
const ARC_SEGMENTS: f64 = 96.0;

/// What the viewport keeps of the knobs between events and frames.
#[derive(Debug, Default)]
pub(crate) struct Input {
    /// The knob under the cursor, by its place, if any.
    hover: Option<usize>,
    drag: Option<Drag>,
}

impl Input {
    /// Whether a knob has the mouse: one is under the cursor or dragged.
    pub(crate) fn holds(&self) -> bool {
        self.hover.is_some() || self.drag.is_some()
    }

    /// Whether one is dragged.
    pub(crate) fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Lets go of what it holds.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// A knob being dragged, as it was grabbed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    /// Which, by its place.
    index: usize,
    /// The knob as it was grabbed: its path is kept while it's dragged.
    knob: OpKnob,
    /// A pixel's size at the path's origin or centre, in millimetres.
    pixel: f64,
    /// An arc's radius, in millimetres, as grabbed.
    radius: f64,
    /// How far along its path it was, and where the cursor was on it.
    from: f64,
    grabbed: f64,
    /// An arc's: how far the cursor has turned since it was grabbed.
    turned: f64,
    /// The value last sent.
    sent: f64,
}

/// A knob as it's seen: its puck, a pixel's size at its path's origin
/// or centre, and an arc's radius in millimetres.
struct Shown {
    puck: Puck<usize>,
    pixel: f64,
    radius: f64,
}

/// The projector for what's in the world, seen by `camera` over
/// `bounds`.
fn projector(camera: &Camera, bounds: Rectangle) -> Option<Projector> {
    Projector::new(
        camera,
        OriginPlane::XY.placement(),
        bounds.width,
        bounds.height,
    )
}

/// A pixel's size at `at`, in millimetres, if there's one: not behind the
/// eye of a perspective view.
fn pixel_at(projector: &Projector, at: DVec3) -> Option<f64> {
    let depth = projector.world_depth(at);
    if projector.perspective() && (depth.is_nan() || depth <= projector.near()) {
        return None;
    }
    let pixel = projector.pixel_at(depth);
    (pixel > 0.0 && pixel.is_finite()).then_some(pixel)
}

/// The point of `knob`'s path `along` it (millimetres along a line,
/// radians round an arc of `radius` millimetres).
fn on_path(knob: &OpKnob, along: f64, radius: f64) -> DVec3 {
    match knob.path {
        KnobPath::Line { origin, along: way } => origin + way * along,
        KnobPath::Arc {
            centre,
            axis,
            radial,
            ..
        } => {
            let (sin, cos) = (angle::sin(along), angle::cos(along));
            centre + (radial * cos + axis.cross(radial) * sin) * radius
        }
    }
}

/// `knob` as it's seen through `projector`: `None` behind the eye.
fn shown(index: usize, knob: &OpKnob, projector: &Projector) -> Option<Shown> {
    let (pixel, radius) = match knob.path {
        KnobPath::Line { origin, .. } => (pixel_at(projector, origin)?, 0.0),
        KnobPath::Arc { centre, radius, .. } => {
            let pixel = pixel_at(projector, centre)?;
            let radius = match radius {
                KnobRadius::World(radius) => radius,
                KnobRadius::Pixels(pixels) => pixels * pixel,
            };
            (pixel, radius)
        }
    };
    let at = on_path(knob, knob.along(knob.value, pixel), radius);
    let puck = Puck::new(index, at, knob.out, projector)?;
    Some(Shown {
        puck,
        pixel,
        radius,
    })
}

/// The knob of `knobs` under the screen position `at`, by its place.
fn knob_under(knobs: &[OpKnob], projector: &Projector, at: DVec2) -> Option<usize> {
    let pucks: Vec<_> = (knobs.iter().enumerate())
        .filter_map(|(index, knob)| shown(index, knob, projector))
        .map(|shown| shown.puck)
        .collect();
    handle::knob_at(&pucks, at)
}

/// The knobs, drawn into `live` seen by `camera` over `bounds`: each
/// one's shaft, then the one hovered or dragged with its rail, then the
/// pucks, the one hovered or dragged lighter. A knob dragged is drawn on
/// its path as it was grabbed.
pub(crate) fn draw(
    live: &mut SketchLayer,
    knobs: &[OpKnob],
    input: &Input,
    colors: SketchColors,
    camera: &Camera,
    bounds: Rectangle,
) {
    let Some(projector) = projector(camera, bounds) else {
        return;
    };
    let active = input.drag.map(|drag| drag.index).or(input.hover);
    for (index, knob) in knobs.iter().enumerate() {
        let knob = match input.drag {
            Some(drag) if drag.index == index => OpKnob {
                path: drag.knob.path,
                ..*knob
            },
            _ => *knob,
        };
        let Some(seen) = shown(index, &knob, &projector) else {
            continue;
        };
        let hot = active == Some(index);
        let (color, _) = handle::tone(colors, knob.tone, hot);
        let along = knob.along(knob.value, seen.pixel);
        if knob.shaft {
            let shaft = path_points(&knob, 0.0, along, seen.radius);
            handle::draw_shaft(live, &projector, &shaft, color);
        }
        if hot {
            let per_pixel = match knob.path {
                KnobPath::Line { .. } => seen.puck.pixel,
                // At most half a turn either way.
                KnobPath::Arc { .. } => {
                    (seen.puck.pixel / seen.radius).min(std::f64::consts::PI / handle::RAIL_REACH)
                }
            };
            let path = |px: f64| on_path(&knob, along + px * per_pixel, seen.radius);
            handle::draw_rail(live, &projector, colors.rail, path);
        }
        handle::draw_puck(
            live,
            &projector,
            &seen.puck,
            handle::tone(colors, knob.tone, hot),
        );
    }
}

/// The points of `knob`'s path from `from` to `to` along it: a line's
/// ends, or enough points round an arc of `radius` to draw it smooth.
fn path_points(knob: &OpKnob, from: f64, to: f64, radius: f64) -> Vec<DVec3> {
    match knob.path {
        KnobPath::Line { .. } => vec![on_path(knob, from, radius), on_path(knob, to, radius)],
        KnobPath::Arc { .. } => {
            let turn = std::f64::consts::TAU;
            let steps = ((to - from).abs() / turn * ARC_SEGMENTS).ceil().max(1.0) as usize;
            (0..=steps)
                .map(|k| on_path(knob, from + (to - from) * k as f64 / steps as f64, radius))
                .collect()
        }
    }
}

/// Takes the mouse `event` for `knobs`, with the `cursor` over `bounds`
/// seen by `camera`: hovering, grabbing and dragging them, if `editable`.
/// `None` for what's left to picking the model and the camera: anything
/// off them. While the cursor is over one, what the model held hovered,
/// if `hovered`, is let go of. Lengths snap in `units`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mouse(
    knobs: &[OpKnob],
    editable: bool,
    units: LengthUnit,
    input: &mut Input,
    event: mouse::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    camera: &Camera,
    hovered: bool,
) -> Option<Action<Message>> {
    if knobs.is_empty() || !editable {
        input.clear();
        return None;
    }
    let local = |p: Point| DVec2::new((p.x - bounds.x).into(), (p.y - bounds.y).into());
    let projector = projector(camera, bounds)?;
    match event {
        mouse::Event::CursorMoved { position } => {
            if let Some(drag) = &mut input.drag {
                // The raw position, so a drag goes on over the rest of
                // the window.
                let look = dragged(drag, &projector, local(position), units);
                return Some(
                    match look {
                        Some(look) => Action::publish(Message::Look(Look::Motion(look))),
                        None => Action::capture(),
                    }
                    .and_capture(),
                );
            }
            let over = (cursor.position_over(bounds))
                .and_then(|at| knob_under(knobs, &projector, local(at)));
            let changed = std::mem::replace(&mut input.hover, over) != over;
            match (over, changed) {
                (Some(_), _) if hovered => {
                    Some(Action::publish(Message::Look(Look::Hover(None))).and_capture())
                }
                (Some(_), true) => Some(Action::request_redraw().and_capture()),
                (Some(_), false) => Some(Action::capture()),
                (None, true) => Some(Action::request_redraw()),
                (None, false) => None,
            }
        }
        mouse::Event::CursorLeft => input.hover.take().map(|_| Action::request_redraw()),
        mouse::Event::ButtonPressed(mouse::Button::Left) => {
            let at = local(cursor.position_over(bounds)?);
            let index = knob_under(knobs, &projector, at)?;
            let knob = *knobs.get(index)?;
            let seen = shown(index, &knob, &projector)?;
            let grabbed = cursor_on(&knob, &projector, at)?;
            input.drag = Some(Drag {
                index,
                knob,
                pixel: seen.pixel,
                radius: seen.radius,
                from: knob.along(knob.value, seen.pixel),
                grabbed,
                turned: 0.0,
                sent: knob.value,
            });
            input.hover = Some(index);
            Some(Action::request_redraw().and_capture())
        }
        mouse::Event::ButtonReleased(mouse::Button::Left) => {
            input.drag.take()?;
            Some(Action::request_redraw().and_capture())
        }
        _ => None,
    }
}

/// Works out again which knob is under the `cursor` as a frame is drawn:
/// the camera, or the knobs, may have moved under a cursor that didn't.
/// While one's under it, what the model held hovered, if `hovered`, is
/// let go of. Not while one is dragged.
pub(crate) fn redraw(
    knobs: &[OpKnob],
    input: &mut Input,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    camera: &Camera,
    hovered: bool,
) -> Option<Action<Message>> {
    if input.drag.is_some() {
        return None;
    }
    let projector = projector(camera, bounds)?;
    let over = cursor.position_over(bounds).and_then(|at| {
        let at = DVec2::new((at.x - bounds.x).into(), (at.y - bounds.y).into());
        knob_under(knobs, &projector, at)
    });
    input.hover = over;
    (over.is_some() && hovered).then(|| Action::publish(Message::Look(Look::Hover(None))))
}

/// Where the cursor at the screen position `at` is on `knob`'s path:
/// how far along a line its ray passes nearest it, or the angle round
/// an arc's axis where its ray meets the arc's plane, from the radial.
/// `None` looking along a line, or with an arc's plane edge on, or its
/// ray meeting it behind the eye or at the centre.
fn cursor_on(knob: &OpKnob, projector: &Projector, at: DVec2) -> Option<f64> {
    match knob.path {
        KnobPath::Line { origin, along } => projector.along_line(origin, along, at),
        KnobPath::Arc {
            centre,
            axis,
            radial,
            ..
        } => {
            let (origin, ray) = projector.ray(at)?;
            let facing = ray.dot(axis);
            if facing.is_nan() || facing.abs() <= EDGE_ON * ray.length() {
                return None;
            }
            let t = (centre - origin).dot(axis) / facing;
            if projector.perspective() && t <= 0.0 {
                return None;
            }
            let p = origin + ray * t - centre;
            let (x, y) = (p.dot(radial), p.dot(axis.cross(radial)));
            let turn = angle::atan2(y, x);
            (turn.is_finite() && (x != 0.0 || y != 0.0)).then_some(turn)
        }
    }
}

/// `drag` dragged to the screen position `at`: the message with the
/// value there, snapped, if it's changed since it was last sent.
fn dragged(
    drag: &mut Drag,
    projector: &Projector,
    at: DVec2,
    units: LengthUnit,
) -> Option<MotionLook> {
    let knob = drag.knob;
    let now = cursor_on(&knob, projector, at)?;
    let along = match knob.path {
        KnobPath::Line { .. } => drag.from + (now - drag.grabbed),
        KnobPath::Arc { .. } => {
            drag.turned += wrapped(now - drag.grabbed);
            drag.grabbed = now;
            drag.from + drag.turned
        }
    };
    let value = snapped(&knob, knob.value_at(along, drag.pixel), drag, units)?;
    if !value.is_finite() || value == drag.sent {
        return None;
    }
    drag.sent = value;
    Some(MotionLook::DragKnob {
        knob: drag.index,
        value,
    })
}

/// `value` of `knob` snapped to its steps ([`KnobSnap`]) at least 6
/// pixels apart along its path as `drag` grabbed it.
fn snapped(knob: &OpKnob, value: f64, drag: &Drag, units: LengthUnit) -> Option<f64> {
    // How many pixels a unit of the value is along the path.
    let unit = knob.along(1.0, drag.pixel).abs()
        * match knob.path {
            KnobPath::Line { .. } => 1.0,
            KnobPath::Arc { .. } => drag.radius,
        }
        / drag.pixel;
    if !(unit > 0.0 && unit.is_finite()) {
        return None;
    }
    let step = match knob.snap {
        KnobSnap::Length => snap_step(1.0 / unit, units)?,
        KnobSnap::Factor => round_step(1.0 / unit),
        // A unit of an angle is a radian: `unit` is the arc's radius.
        KnobSnap::Angle => angle_step(unit).to_radians(),
    };
    Some((value / step).round() * step + 0.0)
}

/// The roundest step, 1, 2 or 5 × 10ⁿ, at least 6 pixels long, a pixel
/// being `pixel` of the value.
fn round_step(pixel: f64) -> f64 {
    let least = 6.0 * pixel;
    if !(least > 0.0 && least.is_finite()) {
        return 1.0;
    }
    let mut power = 1e-12;
    while power * 10.0 <= least && power < 1e12 {
        power *= 10.0;
    }
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * power)
        .find(|&step| step >= least)
        .unwrap_or(10.0 * power)
}
