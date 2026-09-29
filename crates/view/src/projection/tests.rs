use glam::Vec3;
use varde_document::OriginPlane;
use varde_render::View;

use super::*;

const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 600.0;

/// A camera in `projection` looking at `target`, from `view`.
fn camera(projection: Projection, view: View, target: Vec3) -> Camera {
    let mut camera = Camera::default();
    camera.set_projection(projection);
    camera.look_from(view);
    camera.set_target(target);
    camera
}

fn projector(camera: &Camera, plane: OriginPlane) -> Projector {
    Projector::new(camera, plane.placement(), WIDTH, HEIGHT).unwrap()
}

/// Within rounding: the camera's axes are `f32`.
fn close(a: DVec2, b: DVec2) -> bool {
    a.abs_diff_eq(b, 1e-3)
}

const PROJECTIONS: [Projection; 2] = [Projection::Orthographic, Projection::Perspective];

#[test]
fn the_target_shows_in_the_middle_and_right_is_right() {
    for projection in PROJECTIONS {
        let top = camera(projection, View::Top, Vec3::new(2.0, 3.0, 0.0));
        let xy = projector(&top, OriginPlane::XY);
        let middle = DVec2::new(400.0, 300.0);
        assert!(close(xy.project(DVec2::new(2.0, 3.0)).unwrap(), middle));
        // Sketch x is right on screen and y up, which is less y.
        let right = xy.project(DVec2::new(3.0, 3.0)).unwrap();
        let up = xy.project(DVec2::new(2.0, 4.0)).unwrap();
        assert!(right.x > middle.x && close(DVec2::new(middle.x, right.y), middle));
        assert!(up.y < middle.y && close(DVec2::new(up.x, middle.y), middle));
        // At the target's depth, the view is `view_height` tall.
        let pixel = f64::from(HEIGHT) / f64::from(top.view_height());
        assert!((right.x - middle.x - pixel).abs() < 1e-3);
    }
}

#[test]
fn the_cursor_finds_the_point_it_shows_under_any_camera() {
    let mut oblique = Camera::default();
    oblique.set_target(Vec3::new(1.0, -2.0, 0.5));
    oblique.orbit(0.4, 0.2);
    for projection in PROJECTIONS {
        oblique.set_projection(projection);
        for plane in OriginPlane::ALL {
            let projector = projector(&oblique, plane);
            for at in [DVec2::ZERO, DVec2::new(1.5, -0.5), DVec2::new(-2.0, 3.0)] {
                let shown = projector.project(at).unwrap();
                let cursor = projector.cursor(shown).unwrap();
                assert!(close(cursor.at, at), "{plane:?} {projection:?} {at}");
                assert!(cursor.pixel > 0.0 && cursor.pixel.is_finite());
            }
        }
    }
}

#[test]
fn a_pixel_is_the_view_height_over_the_viewport_s_at_the_target() {
    for projection in PROJECTIONS {
        let front = camera(projection, View::Front, Vec3::ZERO);
        let xz = projector(&front, OriginPlane::XZ);
        let cursor = xz.cursor(DVec2::new(400.0, 300.0)).unwrap();
        assert!(close(cursor.at, DVec2::ZERO));
        let expected = f64::from(front.view_height()) / f64::from(HEIGHT);
        assert!((cursor.pixel - expected).abs() < 1e-6, "{projection:?}");
    }
    // Farther away in perspective, a pixel covers more.
    let mut tilted = camera(Projection::Perspective, View::Front, Vec3::ZERO);
    tilted.orbit(0.0, 0.6);
    let xy = projector(&tilted, OriginPlane::XY);
    let near = xy.cursor(DVec2::new(400.0, 500.0)).unwrap();
    let far = xy.cursor(DVec2::new(400.0, 100.0)).unwrap();
    assert!(far.pixel > near.pixel, "{far:?} {near:?}");
}

#[test]
fn a_ray_along_the_plane_meets_nothing() {
    // Looking down on XZ, which stands straight up: every ray runs along
    // it.
    for projection in PROJECTIONS {
        let top = camera(projection, View::Top, Vec3::ZERO);
        let xz = projector(&top, OriginPlane::XZ);
        assert_eq!(xz.cursor(DVec2::new(400.0, 300.0)), None);
        assert_eq!(xz.cursor(DVec2::new(10.0, 20.0)), None);
    }
}

#[test]
fn a_plane_behind_the_eye_is_under_no_cursor_in_perspective() {
    // Looking down from below the XY plane, with the plane behind.
    let mut below = camera(
        Projection::Perspective,
        View::Top,
        Vec3::new(0.0, 0.0, -20.0),
    );
    let xy = projector(&below, OriginPlane::XY);
    assert_eq!(xy.cursor(DVec2::new(400.0, 300.0)), None);
    assert_eq!(xy.project(DVec2::ZERO), None);
    // Orthographic views see behind the eye.
    below.set_projection(Projection::Orthographic);
    let xy = projector(&below, OriginPlane::XY);
    assert!(close(
        xy.cursor(DVec2::new(400.0, 300.0)).unwrap().at,
        DVec2::ZERO
    ));
}

#[test]
fn a_segment_through_the_eye_s_plane_is_cut_at_the_near_plane() {
    // Looking down at the XY plane from the front, above it.
    let mut front = camera(Projection::Perspective, View::Front, Vec3::ZERO);
    front.orbit(0.0, 0.3);
    let xy = projector(&front, OriginPlane::XY);
    // From the target to past the foot of the eye, towards the viewer.
    let behind = DVec2::new(0.0, -2.0 * f64::from(front.distance()));
    let (a, b) = xy.segment(DVec2::ZERO, behind).unwrap();
    assert!(close(a, DVec2::new(400.0, 300.0)), "{a}");
    assert!(b.is_finite() && b.y > a.y, "{b}");
    // Wholly behind, it isn't shown.
    assert_eq!(xy.segment(behind, behind + DVec2::X), None);
    assert_eq!(xy.project(behind), None);
}

#[test]
fn cursors_past_the_bounds_or_nonsense_find_nothing() {
    let top = camera(Projection::Orthographic, View::Top, Vec3::ZERO);
    let xy = projector(&top, OriginPlane::XY);
    assert_eq!(xy.cursor(DVec2::new(f64::NAN, 0.0)), None);
    assert_eq!(xy.cursor(DVec2::new(1e12, 0.0)), None);
    assert!(Projector::new(&top, OriginPlane::XY.placement(), 0.0, 10.0).is_none());
    assert!(Projector::new(&top, OriginPlane::XY.placement(), f32::NAN, 10.0).is_none());
    // Above the horizon, looking over the plane, the ray misses it.
    let mut over = camera(Projection::Perspective, View::Front, Vec3::ZERO);
    over.orbit(0.0, 0.01);
    let xy = projector(&over, OriginPlane::XY);
    assert_eq!(xy.cursor(DVec2::new(400.0, 0.0)), None);
    assert!(xy.cursor(DVec2::new(400.0, 600.0)).is_some());
}
