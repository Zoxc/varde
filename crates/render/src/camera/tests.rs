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
    assert!(crate::scene::view_projection(&camera, 1.0, Some(bounds)).is_finite());
}
