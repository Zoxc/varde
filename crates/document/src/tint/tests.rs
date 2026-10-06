use super::*;

#[test]
fn new_takes_only_the_range() {
    assert_eq!(Tint::new(360, 0), None);
    assert_eq!(Tint::new(0, Tint::MAX_SATURATION + 1), None);
    let tint = Tint::new(359, Tint::MAX_SATURATION).unwrap();
    assert_eq!((tint.hue(), tint.saturation()), (359, Tint::MAX_SATURATION));
}

#[test]
fn clamped_wraps_the_hue_and_clamps_the_saturation() {
    let parts = |tint: Tint| (tint.hue(), tint.saturation());
    assert_eq!(parts(Tint::clamped(f32::NAN, f32::NAN)), (0, 0));
    assert_eq!(
        parts(Tint::clamped(f32::INFINITY, f32::INFINITY)),
        (0, Tint::MAX_SATURATION)
    );
    assert_eq!(parts(Tint::clamped(-30.0, -5.0)), (330, 0));
    assert_eq!(
        parts(Tint::clamped(360.0, 100.0)),
        (0, Tint::MAX_SATURATION)
    );
    assert_eq!(parts(Tint::clamped(-0.2, 20.4)), (0, 20));
    assert_eq!(parts(Tint::clamped(719.6, 20.6)), (0, 21));
    assert_eq!(parts(Tint::clamped(1e30, 1e30)).1, Tint::MAX_SATURATION);
    assert!(Tint::clamped(1e30, 0.0).in_range());
}
