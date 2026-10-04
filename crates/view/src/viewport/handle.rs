//! The pucks the extrude's and the revolve's handles have at their knobs,
//! their shafts and the rail a knob hovered or dragged shows: drawn by
//! the renderer on the screen over the model, so the handle always shows,
//! from any side and through the model.

use glam::{DVec2, DVec3};
use iced::Color;
use varde_render::{SketchLayer, Space as LayerSpace};
use varde_sketch::angle;

use super::sketch::{line, srgba};
use crate::hit::segment_distance;
use crate::projection::Projector;
use crate::theme::SketchColors;

/// How wide a handle's shaft is, in pixels.
pub(crate) const SHAFT_WIDTH: f32 = 2.0;
/// A knob's puck, in pixels: a ring square to the way the knob drags,
/// filled faintly, with a dot at its middle, and an arrow out of the end
/// it's on with an open head, the same size on the screen wherever it is.
const RING_RADIUS: f64 = 11.0;
const RING_WIDTH: f32 = 2.0;
const RING_SEGMENTS: usize = 48;
/// How opaque the ring's fill is.
const RING_FILL: f32 = 0.2;
const DOT_RADIUS: f64 = 2.6;
/// How many sides the dot is drawn with.
const DOT_SEGMENTS: usize = 16;
const ARROW_LENGTH: f64 = 20.0;
const ARROW_WIDTH: f32 = 2.0;
/// The arrow's head: how far back from its tip, and how wide either side.
const HEAD_LENGTH: f64 = 6.0;
const HEAD_HALF_WIDTH: f64 = 4.5;
/// The arrow's head is left out shorter than this on the screen, in
/// pixels, looking along the arrow.
const MIN_HEAD: f64 = 1.5;
/// The rail along the way a knob drags while it's hovered or dragged: how
/// far it reaches either way, in pixels, how wide it is, and in how many
/// steps either way it fades out, from opaque at the knob to clear.
pub(crate) const RAIL_REACH: f64 = 170.0;
const RAIL_WIDTH: f32 = 1.5;
const RAIL_STEPS: usize = 16;
/// How near the cursor a knob is grabbed, in pixels: within its ring or
/// a little past it, or near its arrow (a sketch's hit tolerance).
const RING_HIT: f64 = RING_RADIUS + 2.0;
const ARROW_HIT: f64 = 6.0;

/// A knob's puck as it's seen: which knob, where, which way out of the
/// end it's on, how big a pixel is there, and where it and its arrow's
/// tip show.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Puck<K> {
    pub(crate) knob: K,
    pub(crate) at: DVec3,
    /// The way out of the end it's on, of unit length.
    out: DVec3,
    /// A pixel's size at `at`, in millimetres.
    pub(crate) pixel: f64,
    /// Towards the eye from `at`.
    to_eye: DVec3,
    screen: DVec2,
    tip: DVec2,
}

impl<K> Puck<K> {
    /// The world point `pixels` along its arrow's line, out of the end.
    pub(crate) fn along(&self, pixels: f64) -> DVec3 {
        self.at + self.out * (pixels * self.pixel)
    }
}

impl<K: Copy> Puck<K> {
    /// The puck of `knob` at `at`, its arrow along the unit `out`, seen
    /// through `projector`: `None` behind the eye of a perspective view.
    pub(crate) fn new(knob: K, at: DVec3, out: DVec3, projector: &Projector) -> Option<Self> {
        let depth = projector.world_depth(at);
        if projector.perspective() && (depth.is_nan() || depth <= projector.near()) {
            return None;
        }
        let pixel = projector.pixel_at(depth);
        if !(pixel > 0.0 && pixel.is_finite() && at.is_finite() && out.is_finite()) {
            return None;
        }
        let (eye, backward) = projector.eye();
        let to_eye = if projector.perspective() {
            (eye - at).normalize_or_zero()
        } else {
            backward
        };
        let tip = at + out * (ARROW_LENGTH * pixel);
        Some(Self {
            knob,
            at,
            out,
            pixel,
            to_eye,
            screen: projector.show(at),
            tip: projector.show(tip),
        })
    }

    /// The screen position `at`'s distance from it, if it's within reach
    /// of the cursor: its ring's, or its arrow's.
    fn reach(&self, at: DVec2) -> Option<f64> {
        let arrow = segment_distance(at, self.screen, self.tip);
        (at.distance(self.screen) <= RING_HIT || arrow <= ARROW_HIT).then_some(arrow)
    }
}

/// The knob of `pucks` under the screen position `at`: of those in reach
/// ([`Puck::reach`]), the one whose arrow is nearest.
pub(crate) fn knob_at<K: Copy>(pucks: &[Puck<K>], at: DVec2) -> Option<K> {
    (pucks.iter())
        .filter_map(|puck| Some((puck.knob, puck.reach(at)?)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(knob, _)| knob)
}

/// The ring of `puck`, square to its arrow, filled faintly, with a dot at
/// its middle, and its arrow out of the end, in the handle's colours, the
/// hovered ones if `hot`: on the screen as `projector` shows them, so it's
/// drawn over the model and always shows. The arrow's head is open, across
/// the arrow in the plane through it facing the eye, left out looking
/// along it; the ring is left out where it passes behind the eye of a
/// perspective view.
pub(crate) fn draw_puck<K>(
    live: &mut SketchLayer,
    projector: &Projector,
    puck: &Puck<K>,
    colors: SketchColors,
    hot: bool,
) {
    let (color, accent) = if hot {
        (colors.handle_hovered, colors.handle_accent_hovered)
    } else {
        (colors.handle, colors.handle_accent)
    };
    let (x, y) = puck.out.any_orthonormal_pair();
    let radius = RING_RADIUS * puck.pixel;
    let ring: Vec<DVec3> = (0..=RING_SEGMENTS)
        .map(|k| {
            let turn = std::f64::consts::TAU * k as f64 / RING_SEGMENTS as f64;
            puck.at + (x * angle::cos(turn) + y * angle::sin(turn)) * radius
        })
        .collect();
    let runs = shown_runs(projector, &ring);
    if let [whole] = &runs[..]
        && whole.len() == ring.len()
    {
        let fill = Color {
            a: color.a * RING_FILL,
            ..color
        };
        live.fill(LayerSpace::Screen, [&whole[..]], srgba(fill));
    }
    for run in &runs {
        live.polyline(LayerSpace::Screen, run, line(color, RING_WIDTH, false));
    }
    let dot: Vec<DVec2> = (0..DOT_SEGMENTS)
        .map(|k| {
            let turn = std::f64::consts::TAU * k as f64 / DOT_SEGMENTS as f64;
            puck.screen + DVec2::new(angle::cos(turn), angle::sin(turn)) * DOT_RADIUS
        })
        .collect();
    live.fill(LayerSpace::Screen, [&dot[..]], srgba(color));
    let tip = puck.along(ARROW_LENGTH);
    let style = line(accent, ARROW_WIDTH, false);
    for run in shown_runs(projector, &[puck.at, tip]) {
        live.polyline(LayerSpace::Screen, &run, style);
    }
    let across = puck.out.cross(puck.to_eye).normalize_or_zero() * (HEAD_HALF_WIDTH * puck.pixel);
    let back = puck.along(ARROW_LENGTH - HEAD_LENGTH);
    let shown = puck.tip.distance(puck.screen) * HEAD_LENGTH / ARROW_LENGTH;
    if across != DVec3::ZERO && shown >= MIN_HEAD {
        for run in shown_runs(projector, &[back + across, tip, back - across]) {
            live.polyline(LayerSpace::Screen, &run, style);
        }
    }
}

/// A rail in `color` on the screen as `projector` shows it, so it's drawn
/// over the model: from the knob along `path`, the world point `pixels`
/// along the way it drags (either way, as the pixels' sign says),
/// [`RAIL_REACH`] each way, fading out in [`RAIL_STEPS`] from opaque at
/// the knob to clear, cut where it passes behind the eye of a perspective
/// view.
pub(crate) fn draw_rail(
    live: &mut SketchLayer,
    projector: &Projector,
    color: Color,
    path: impl Fn(f64) -> DVec3,
) {
    let alpha: Vec<f32> = (0..RAIL_STEPS)
        .map(|k| 1.0 - (k as f32 + 0.5) / RAIL_STEPS as f32)
        .collect();
    let step = RAIL_REACH / RAIL_STEPS as f64;
    for way in [1.0, -1.0] {
        let world: Vec<DVec3> = (0..=RAIL_STEPS)
            .map(|k| path(k as f64 * step * way))
            .collect();
        let Some(run) = shown_runs(projector, &world).into_iter().next() else {
            continue;
        };
        // From the knob only.
        if run.first() != Some(&projector.show(world[0])) {
            continue;
        }
        let style = line(color, RAIL_WIDTH, false);
        live.polyline_fading(LayerSpace::Screen, &run, &alpha, style);
    }
}

/// The polyline through the world `points` in `color`, on the screen as
/// `projector` shows it, so it's drawn over the model: a shaft. What
/// passes behind the eye of a perspective view is left out.
pub(crate) fn draw_shaft(
    live: &mut SketchLayer,
    projector: &Projector,
    points: &[DVec3],
    color: Color,
) {
    for run in shown_runs(projector, points) {
        live.polyline(LayerSpace::Screen, &run, line(color, SHAFT_WIDTH, false));
    }
}

/// The runs of the polyline through the world `points` in front of the
/// eye, on the screen as `projector` shows them: one for an orthographic
/// view, and for a perspective view one for each stretch in front of it,
/// each segment cut where it passes behind.
fn shown_runs(projector: &Projector, points: &[DVec3]) -> Vec<Vec<DVec2>> {
    let mut runs: Vec<Vec<DVec2>> = Vec::new();
    let mut open = false;
    for pair in points.windows(2) {
        let Some((a, b)) = projector.in_front(pair[0], pair[1]) else {
            open = false;
            continue;
        };
        if !open || a != pair[0] {
            runs.push(vec![projector.show(a)]);
        }
        if let Some(run) = runs.last_mut() {
            run.push(projector.show(b));
        }
        open = b == pair[1];
    }
    runs
}
