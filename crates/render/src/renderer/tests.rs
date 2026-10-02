use super::*;

#[test]
fn alpha_steps_are_the_nearest_of_the_table() {
    // 256 steps: 8 bits' worth.
    assert_eq!(alpha_step(Some(0.0), 255), 0);
    assert_eq!(alpha_step(Some(0.3), 255), 77);
    assert_eq!(alpha_step(Some(0.999), 255), 255);
    assert_eq!(alpha_step(Some(1.0), 255), 255);
    for alpha in [None, Some(f32::NAN), Some(-0.5), Some(1.5)] {
        assert_eq!(alpha_step(alpha, 255), 255, "{alpha:?}");
    }
}

#[test]
fn a_part_less_than_opaque_is_never_drawn_invisible_on_a_short_table() {
    // A device whose buffers hold two steps, 0 and 1: a body at 10 or
    // 30 % is drawn opaque rather than not at all.
    for alpha in [0.1, 0.3, 0.49] {
        assert_eq!(alpha_step(Some(alpha), 1), 1, "{alpha}");
    }
    // Nor on one of a single step, which is opaque.
    assert_eq!(alpha_step(Some(0.1), 0), 0);
    // A finer table still rounds the faintest a body is up to its first
    // step that shows.
    assert_eq!(alpha_step(Some(0.1), 3), 1);
    assert_eq!(alpha_step(Some(0.01), 3), 1);
    // Only 0 itself is drawn at 0.
    assert_eq!(alpha_step(Some(0.0), 1), 0);
}
