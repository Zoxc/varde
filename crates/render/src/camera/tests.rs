use super::*;

fn close(a: Vec3, b: Vec3) -> bool {
    a.abs_diff_eq(b, 1e-5)
}

#[test]
fn views_match_the_camera_looking_from_them() {
    for view in View::ALL {
        let mut camera = Camera::default();
        camera.look_from(view);
        assert!(close(camera.backward(), view.normal()), "{view:?}");
        assert!(close(camera.right(), view.right()), "{view:?}");
        assert!(close(camera.up(), view.up()), "{view:?}");
    }
}

#[test]
fn view_normals_are_distinct_axes() {
    let normals = View::ALL.map(View::normal);
    for (i, n) in normals.iter().enumerate() {
        assert_eq!(n.abs().element_sum(), 1.0, "{:?}", View::ALL[i]);
        assert!(!normals[..i].contains(n), "{:?}", View::ALL[i]);
    }
    assert_eq!(View::Front.normal(), Vec3::NEG_Y);
    assert_eq!(View::Right.normal(), Vec3::X);
}

#[test]
fn basis_matches_view_matrix() {
    for view in View::ALL {
        let mut camera = Camera::default();
        camera.look_from(view);
        let m = camera.view();
        assert!(m.is_finite(), "{view:?}");
        assert!(
            close(m.transform_vector3(camera.right()), Vec3::X),
            "{view:?}"
        );
        assert!(close(m.transform_vector3(camera.up()), Vec3::Y), "{view:?}");
        assert!(
            close(m.transform_vector3(camera.backward()), Vec3::Z),
            "{view:?}"
        );
    }
}

#[test]
fn lerp_hits_both_ends() {
    let from = Camera::default();
    let mut to = Camera {
        target: Vec3::new(4.0, -2.0, 0.0),
        distance: 30.0,
        ..from
    };
    to.look_from(View::Back);
    assert_eq!(from.lerp(&to, 0.0).target, from.target);
    let end = from.lerp(&to, 1.0);
    assert!(close(end.backward(), to.backward()));
    assert!(close(end.target, to.target));
    assert!((end.distance - to.distance).abs() < 1e-4);
}

#[test]
fn lerp_turns_the_short_way() {
    let from = Camera {
        yaw: 170f32.to_radians(),
        ..Camera::default()
    };
    let to = Camera {
        yaw: -170f32.to_radians(),
        ..Camera::default()
    };
    let mid = from.lerp(&to, 0.5).yaw.rem_euclid(std::f32::consts::TAU);
    assert!(
        (mid - 180f32.to_radians()).abs() < 1e-4,
        "{}",
        mid.to_degrees()
    );
}

#[test]
fn top_view_has_front_at_the_bottom() {
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    assert!(close(camera.up(), Vec3::Y));
    assert!(close(camera.right(), Vec3::X));
}

#[test]
fn non_finite_input_leaves_the_camera_unchanged() {
    let mut camera = Camera::default();
    camera.orbit(f32::NAN, 0.0);
    camera.orbit(0.0, f32::INFINITY);
    camera.pan(f32::INFINITY, 0.0);
    camera.pan(0.0, f32::NAN);
    camera.zoom(f32::NAN);
    camera.set_target(Vec3::new(0.0, f32::NAN, 0.0));
    assert_eq!(camera, Camera::default());
}

#[test]
fn pan_and_zoom_stay_bounded() {
    let mut camera = Camera::default();
    camera.zoom(0.0);
    assert_eq!(camera.distance, 1e-3);
    camera.zoom(f32::INFINITY);
    assert_eq!(camera.distance, Camera::EXTENT);

    for _ in 0..100 {
        camera.pan(1e3, -1e3);
    }
    assert_eq!(camera.target.abs().max_element(), Camera::EXTENT);
    camera.set_target(Vec3::new(0.0, 0.0, -1e30));
    assert_eq!(camera.target, Vec3::new(0.0, 0.0, -Camera::EXTENT));
    let bounds = varde_kernel::Aabb {
        min: Vec3::ZERO,
        max: Vec3::ONE,
    };
    assert!(
        crate::scene::view_projection(&camera, 1.0, &crate::GridPlane::XY, Some(bounds))
            .is_finite()
    );
}

#[test]
fn facing_a_view_matches_looking_from_it() {
    for view in View::ALL {
        let (mut from, mut faced) = (Camera::default(), Camera::default());
        from.look_from(view);
        faced.face(view.normal(), view.up());
        assert!(close(faced.backward(), from.backward()), "{view:?}");
        assert!(close(faced.right(), from.right()), "{view:?}");
        assert!(close(faced.up(), from.up()), "{view:?}");
        assert_eq!(faced.target, from.target);
        assert_eq!(faced.distance, from.distance);
    }
}

#[test]
fn facing_straight_down_turns_to_the_up_given() {
    for up in [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::new(1.0, 1.0, 0.0).normalize(),
    ] {
        for normal in [Vec3::Z, Vec3::NEG_Z] {
            let mut camera = Camera::default();
            camera.face(normal, up);
            assert!(close(camera.backward(), normal), "{normal} {up}");
            assert!(close(camera.up(), up), "{normal} {up}");
            assert!(close(camera.right(), up.cross(normal)), "{normal} {up}");
        }
    }
}

#[test]
fn facing_a_tilted_plane_keeps_z_up() {
    let normal = Vec3::new(1.0, 2.0, 0.5).normalize();
    let mut camera = Camera::default();
    // Up is asked for upside down, which the camera can't roll to.
    camera.face(normal * 3.0, Vec3::NEG_Z);
    assert!(close(camera.backward(), normal));
    assert!(camera.up().z > 0.0);
    assert!(camera.right().z.abs() < 1e-6);
    assert!(camera.view().is_finite());
}

#[test]
fn facing_nothing_in_particular_changes_nothing() {
    let mut camera = Camera::default();
    for normal in [Vec3::ZERO, Vec3::NAN, Vec3::new(f32::INFINITY, 0.0, 0.0)] {
        camera.face(normal, Vec3::Y);
        assert_eq!(camera, Camera::default(), "{normal}");
    }
    // Straight down with no horizontal up keeps the way it's turned.
    let mut camera = Camera::default();
    camera.face(Vec3::Z, Vec3::Z);
    assert!(close(camera.backward(), Vec3::Z));
    assert_eq!(camera.yaw, Camera::default().yaw);
}

#[test]
fn orbiting_about_a_pivot_keeps_it_where_it_shows() {
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = Camera::default();
        camera.set_projection(projection);
        camera.set_target(Vec3::new(1.0, 2.0, 0.5));
        let pivot = Vec3::new(3.0, -1.0, 2.0);
        let shows = |camera: &Camera| {
            let clip =
                camera.projection_matrix(1.5, 0.1..100.0) * camera.view() * pivot.extend(1.0);
            clip.truncate().truncate() / clip.w
        };
        let before = shows(&camera);
        let distance = camera.distance();
        // Past the pole too, where the pitch is clamped.
        camera.orbit_about(pivot, 0.7, 3.0);
        assert!(shows(&camera).abs_diff_eq(before, 1e-4), "{projection:?}");
        assert_eq!(camera.distance(), distance);
        assert_eq!(camera.pitch, Camera::PITCH_LIMIT);
    }
}

#[test]
fn orbiting_about_the_target_is_orbiting() {
    let mut about = Camera::default();
    let mut plain = about;
    about.orbit_about(about.target(), 0.3, -0.2);
    plain.orbit(0.3, -0.2);
    assert!(close(about.target(), plain.target()));
    assert_eq!((about.yaw, about.pitch), (plain.yaw, plain.pitch));
}

#[test]
fn orbiting_about_a_pivot_ignores_what_is_not_finite() {
    let mut camera = Camera::default();
    let before = camera;
    camera.orbit_about(Vec3::new(f32::NAN, 0.0, 0.0), 0.3, 0.2);
    camera.orbit_about(Vec3::ZERO, f32::INFINITY, 0.2);
    assert_eq!(camera, before);
    // A pivot far out swings the target past the extent: it's clamped.
    camera.orbit_about(Vec3::splat(Camera::EXTENT), std::f32::consts::PI, 0.0);
    assert!(camera.target().abs().max_element() <= Camera::EXTENT);
}

#[test]
fn centering_on_a_point_pans_across_the_view() {
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = Camera::default();
        camera.set_projection(projection);
        let point = Vec3::new(3.0, -1.0, 2.0);
        let depth = (point - camera.eye()).dot(camera.backward());
        camera.center_on(point);
        let clip = camera.projection_matrix(1.5, 0.1..100.0) * camera.view() * point.extend(1.0);
        let shows = clip.truncate().truncate() / clip.w;
        assert!(
            shows.abs_diff_eq(glam::Vec2::ZERO, 1e-5),
            "{projection:?}: {shows}"
        );
        // As far in front of the eye as before.
        let after = (point - camera.eye()).dot(camera.backward());
        assert!((after - depth).abs() < 1e-4, "{projection:?}");
    }
    let mut camera = Camera::default();
    let before = camera;
    camera.center_on(Vec3::new(f32::NAN, 0.0, 0.0));
    assert_eq!(camera, before);
}

#[test]
fn zooming_at_a_point_keeps_it_where_it_shows() {
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = Camera::default();
        camera.set_projection(projection);
        camera.set_target(Vec3::new(1.0, 2.0, 0.5));
        // A quarter of the height right and a tenth up of the middle, on
        // a viewport 1.5 times as wide as it is high.
        let (x, y) = (0.25, -0.1);
        let at = camera.target() + (camera.right() * x - camera.up() * y) * camera.view_height();
        let shows = |camera: &Camera| {
            let clip = camera.projection_matrix(1.5, 0.1..100.0) * camera.view() * at.extend(1.0);
            clip.truncate().truncate() / clip.w
        };
        let before = shows(&camera);
        assert!(before.abs_diff_eq(glam::Vec2::new(x * 2.0 / 1.5, -y * 2.0), 1e-5));
        for factor in [0.5, 3.0] {
            camera.zoom_at(factor, x, y);
            assert!(shows(&camera).abs_diff_eq(before, 1e-4), "{projection:?}");
        }
        assert!((camera.distance() - Camera::default().distance() * 1.5).abs() < 1e-4);
    }
    let mut camera = Camera::default();
    let before = camera;
    camera.zoom_at(0.5, f32::NAN, 0.0);
    camera.zoom_at(f32::INFINITY, 0.0, 0.0);
    assert_eq!(camera, before);
    // At the middle, only the distance changes.
    camera.zoom_at(0.5, 0.0, 0.0);
    assert!(close(camera.target(), before.target()));
    // Far off to the side, the target is clamped.
    camera.zoom_at(1e-30, 1e30, 0.0);
    assert!(camera.target().abs().max_element() <= Camera::EXTENT);
}
