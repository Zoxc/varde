//! The marks drawn for a session: an axis with its arrowhead, a plane, an
//! align's points and directions, a scale's point.

use glam::{DVec2, DVec3};
use iced::Rectangle;
use varde_render::{Camera, Colors, GridPlane, SketchLayer, Space as LayerSpace};

use super::Moving;
use crate::motion::{AlignView, ScaleView};
use crate::projection::Projector;
use crate::theme::SketchColors;
use crate::viewport::handle;
use crate::viewport::sketch::{line, mark_dot, opaque, srgba};

/// How wide the axis and the plane's outline are drawn, in pixels.
const LINE_WIDTH: f32 = 2.5;
/// How far the axis and the plane reach at least either side of the
/// bodies' centre, in millimetres, and how much past the bodies' box
/// they reach.
const MIN_REACH: f64 = 10.0;
const REACH_PAST: f64 = 1.25;
/// How opaque the plane's fill is.
const PLANE_FILL: f32 = 0.12;
/// How wide an align's points are drawn, in pixels: the measure tool's.
const ALIGN_POINT_RADIUS: f32 = 5.0;
/// Draws a scale's point on `live`, a dot in the accent as an align's
/// moved point is (its edge is lit in the model's highlight), and while
/// the point is picked the snap points of what the cursor is over, as the
/// measure tool's.
pub(super) fn scale_marks(
    live: &mut SketchLayer,
    scale: &ScaleView<'_>,
    scene: &Colors,
    colors: SketchColors,
) {
    if let Some(point) = scale.at.filter(|point| point.is_finite()) {
        let style = mark_dot(ALIGN_POINT_RADIUS, opaque(scene.selected), colors, true);
        live.world_point(point.as_vec3(), style);
    }
    if let Some((index, hover)) = scale.snaps {
        crate::viewport::measure::snap_dots(live, index, hover, colors);
    }
}

impl<'a> Moving<'a> {
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

    /// An align's points and directions, each side's: its point as a dot,
    /// its direction as an arrow from it across the bodies, its second
    /// direction as a shorter dashed one; the moved side's in the accent,
    /// the target's in the second colour. While a point is picked, the
    /// snap points of what the cursor is over, as the measure tool's.
    pub(super) fn align(
        &self,
        live: &mut SketchLayer,
        align: &AlignView<'_>,
        scene: &Colors,
        colors: SketchColors,
        camera: &Camera,
        bounds: Rectangle,
    ) {
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
            live.world_point(
                point.as_vec3(),
                mark_dot(ALIGN_POINT_RADIUS, color, colors, true),
            );
        }
        if let Some((index, hover)) = align.snaps {
            crate::viewport::measure::snap_dots(live, index, hover, colors);
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
        let Some(projector) = Projector::world_in(camera, bounds) else {
            return;
        };
        handle::screen_arrowhead(live, projector.show(from), projector.show(to), color);
    }

    /// The axis through `point` along the unit `along`, across the bodies,
    /// with its arrowhead.
    pub(super) fn axis(
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
    pub(super) fn plane(
        &self,
        live: &mut SketchLayer,
        point: DVec3,
        normal: DVec3,
        color: iced::Color,
    ) {
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
}
