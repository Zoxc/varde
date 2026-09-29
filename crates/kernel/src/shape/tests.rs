use super::*;

#[test]
fn check_wants_a_positive_bounded_size() {
    assert_eq!(Shape::cuboid(Vec3::splat(MAX_COORD)).check(), Ok(()));
    for bad in [0.0, -1.0, 2.0 * MAX_COORD, f32::NAN, f32::INFINITY] {
        let shape = Shape::cuboid(Vec3::new(1.0, bad, 1.0));
        assert!(matches!(shape.check(), Err(ShapeError::Size(_))));
        assert!(matches!(shape.build(), Err(ShapeError::Size(_))));
    }
}

#[test]
fn positions_are_in_range_within_max_coord() {
    assert!(crate::position_in_range(Vec3::splat(-MAX_COORD)));
    for bad in [1.01 * MAX_COORD, f32::NAN, f32::INFINITY] {
        assert!(!crate::position_in_range(Vec3::new(0.0, bad, 0.0)));
    }
}

#[test]
fn builds_a_checked_box_with_its_bounds() {
    for size in [Vec3::ONE, Vec3::new(0.5, 2.0, 3.0), Vec3::splat(MAX_COORD)] {
        let shape = Shape::cuboid(size);
        let solid = shape.build().unwrap();
        assert_eq!(solid.bounds(), Some(shape.bounds()));
        assert_eq!(solid.mesh().tris().len(), 12);
    }
    // Checked, but thinner than the default resolution.
    let thin = Shape::cuboid(Vec3::new(1.0, 1e-9, 1.0));
    assert_eq!(thin.check(), Ok(()));
    assert!(matches!(
        thin.build(),
        Err(ShapeError::Kernel(KernelError::Invalid(_)))
    ));
}
