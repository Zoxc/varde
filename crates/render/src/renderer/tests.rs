use super::*;

#[test]
fn alpha_steps_are_the_nearest_of_the_table() {
    assert_eq!(alpha_step(Some(0.0), 255), 0);
    assert_eq!(alpha_step(Some(0.3), 255), 77);
    assert_eq!(alpha_step(Some(0.999), 255), 255);
    assert_eq!(alpha_step(Some(1.0), 255), 255);
    for alpha in [None, Some(f32::NAN), Some(-0.5), Some(1.5)] {
        assert_eq!(alpha_step(alpha, 255), 255, "{alpha:?}");
    }
}

#[test]
fn alpha_steps_multiply_to_the_nearest_step() {
    assert_eq!(product_step(255, 77, 255), 77);
    assert_eq!(product_step(128, 128, 255), 64);
    assert_eq!(product_step(0, 255, 255), 0);
    assert_eq!(product_step(3, 3, 3), 3);
    assert_eq!(product_step(1, 2, 3), 1);
    // A table of a single step has none to multiply.
    assert_eq!(product_step(0, 0, 0), 0);
}

#[test]
fn a_part_less_than_opaque_is_never_drawn_invisible_on_a_short_table() {
    // Two steps, 0 and 1: a faint body is drawn opaque, not at all only
    // at 0.
    for alpha in [0.1, 0.3, 0.49] {
        assert_eq!(alpha_step(Some(alpha), 1), 1, "{alpha}");
    }
    assert_eq!(alpha_step(Some(0.0), 1), 0);
    // A single step is opaque.
    assert_eq!(alpha_step(Some(0.1), 0), 0);
    // A finer table rounds the faintest up to its first step.
    assert_eq!(alpha_step(Some(0.01), 3), 1);
}
