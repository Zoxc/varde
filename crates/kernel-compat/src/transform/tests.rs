use glam::DVec3;

use super::*;

/// Quarter turns about a coordinate axis are exact whatever the axis's
/// length, and so are steps along one: the axis is made unit without
/// rounding, as a direction along an axis must be.
#[test]
fn quarter_turns_and_steps_along_an_axis_of_any_length_are_exact() {
    for length in [1.0, 3.0, 49.0, 0.1, 1e-200, 1e200, f64::MAX] {
        let axis = DVec3::new(0.0, 0.0, length);
        let turn = Motion::turn(DVec3::ZERO, axis, 90.0).expect("an axis");
        assert_eq!(turn.vector(DVec3::X), DVec3::Y, "{length}");
        assert_eq!(turn.vector(DVec3::Z), DVec3::Z, "{length}");
        let step = Motion::pattern_step(axis, 2.0, 3).expect("a direction");
        assert_eq!(
            step.point(DVec3::ZERO),
            DVec3::new(0.0, 0.0, 6.0),
            "{length}"
        );
    }
    for axis in [DVec3::ZERO, DVec3::NAN, DVec3::new(0.0, f64::INFINITY, 1.0)] {
        assert_eq!(Motion::turn(DVec3::ZERO, axis, 90.0), None);
        assert_eq!(Motion::pattern_step(axis, 1.0, 1), None);
    }
}
