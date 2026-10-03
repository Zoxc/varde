//! The move or mirror being set up, in the viewport. The cursor picks the
//! model as outside the sessions (`ModelPicking`, the app routing the
//! clicks); drawn here, on top of the model so it shows through the
//! bodies it runs through: a move's axis, a line across the bodies' box
//! with an arrowhead (on the screen) at the end positive angles turn
//! right-handed about, or a mirror's plane, a square across their box,
//! outlined dashed and filled faintly, with its normal's short line.
//! Both in the selected colour, or the hovered one while the panel's
//! row of it is hovered.

use std::sync::{Arc, LazyLock};

use glam::{DVec2, DVec3};
use iced::Rectangle;
use varde_document::OriginPlane;
use varde_render::{Camera, GridPlane, SketchLayer, Space as LayerSpace};

use super::sketch::{line, srgba};
use crate::motion::{MotionKind, MotionState};
use crate::operation_panel::PanelHover;
use crate::projection::Projector;
use crate::theme::SketchColors;

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

/// The move or mirror being set up, as the viewport shows it.
#[derive(Debug, Clone)]
pub(crate) struct Moving<'a> {
    state: MotionState<'a>,
}

impl<'a> Moving<'a> {
    pub(crate) fn new(state: MotionState<'a>) -> Self {
        Self { state }
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

    /// What it draws, seen by `camera` over `bounds`: no base layer, as
    /// it all changes with the answers, and a live one with the axis and
    /// its arrow or the plane.
    pub(crate) fn layers(
        &self,
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
                MotionKind::Move => {
                    self.axis(&mut live, point, along, color, camera, bounds);
                }
                MotionKind::Mirror => self.plane(&mut live, point, along, color),
            }
        }
        (EMPTY.clone(), live)
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
    /// tool's, since it draws in the world.
    pub(crate) fn plane_of_layers(&self) -> GridPlane {
        GridPlane::XY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation_panel::TypedField;

    fn state(kind: MotionKind, line: Option<[DVec3; 2]>) -> MotionState<'static> {
        let field = TypedField {
            text: "",
            error: None,
            value: None,
        };
        MotionState {
            kind,
            editing: None,
            bodies: Vec::new(),
            picking: crate::MotionPick::Bodies,
            fields: [field; 4],
            reference: Some("Z axis".to_owned()),
            line,
            bounds: Some([DVec3::new(-10.0, -10.0, 0.0), DVec3::new(10.0, 10.0, 5.0)]),
            keep_original: true,
            need: None,
            refused: None,
            error: None,
            show_error: None,
            checking: false,
            ready: false,
            accept: false,
            editable: true,
            hover: None,
        }
    }

    #[test]
    fn the_axis_and_the_plane_are_drawn_where_they_re_found() {
        let colors = crate::Mode::Light.palette().sketching;
        let camera = Camera::default();
        let bounds = Rectangle::new(iced::Point::ORIGIN, iced::Size::new(800.0, 600.0));
        let line = Some([DVec3::ZERO, DVec3::Z]);
        for kind in [MotionKind::Move, MotionKind::Mirror] {
            let (_, live) = Moving::new(state(kind, line)).layers(colors, &camera, bounds);
            assert!(!live.is_empty(), "{kind:?}");
            // Nothing found, nothing drawn; nor a direction of nothing.
            let (_, live) = Moving::new(state(kind, None)).layers(colors, &camera, bounds);
            assert!(live.is_empty(), "{kind:?}");
            let zero = Some([DVec3::ZERO, DVec3::ZERO]);
            let (_, live) = Moving::new(state(kind, zero)).layers(colors, &camera, bounds);
            assert!(live.is_empty(), "{kind:?}");
        }
    }
}
