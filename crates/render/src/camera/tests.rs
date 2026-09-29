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
