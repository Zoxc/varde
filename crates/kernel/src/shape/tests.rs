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
