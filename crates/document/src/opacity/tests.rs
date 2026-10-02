use super::*;

#[test]
fn new_takes_only_the_range() {
    assert_eq!(Opacity::new(9), None);
    assert_eq!(Opacity::new(0), None);
    assert_eq!(Opacity::new(101), None);
    assert_eq!(Opacity::new(u8::MAX), None);
    assert_eq!(Opacity::new(10), Some(Opacity::MIN));
    assert_eq!(Opacity::new(100), Some(Opacity::MAX));
    assert_eq!(Opacity::new(55).map(Opacity::percent), Some(55));
}

#[test]
fn clamped_rounds_into_the_range() {
    assert_eq!(Opacity::clamped(f32::NAN), Opacity::MAX);
    assert_eq!(Opacity::clamped(f32::INFINITY), Opacity::MAX);
    assert_eq!(Opacity::clamped(f32::NEG_INFINITY), Opacity::MIN);
    assert_eq!(Opacity::clamped(-1e30), Opacity::MIN);
    assert_eq!(Opacity::clamped(1e30), Opacity::MAX);
    assert_eq!(Opacity::clamped(0.0), Opacity::MIN);
    assert_eq!(Opacity::clamped(42.4).percent(), 42);
    assert_eq!(Opacity::clamped(42.6).percent(), 43);
    assert_eq!(Opacity::clamped(100.4), Opacity::MAX);
}

#[test]
fn opaque_by_default() {
    let opacity = Opacity::default();
    assert!(opacity.is_opaque());
    assert_eq!(opacity.alpha(), 1.0);
    assert!(!Opacity::MIN.is_opaque());
    assert!((Opacity::MIN.alpha() - 0.1).abs() < 1e-7);
    assert_eq!(Opacity::new(50).unwrap().to_string(), "50 %");
}
