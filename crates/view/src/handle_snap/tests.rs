use varde_expr::LengthUnit;

use super::*;

#[test]
fn drags_snap_to_round_steps_of_the_units() {
    // 6 pixels of 0.1 mm: 0.6 mm, up to 1 mm.
    assert_eq!(snap_step(0.1, LengthUnit::Mm), Some(1.0));
    assert_eq!(snap_step(0.03, LengthUnit::Mm), Some(0.2));
    assert_eq!(snap_step(0.5, LengthUnit::Mm), Some(5.0));
    // In inches: 0.6 mm is 0.0236 in, up to 0.05 in.
    let step = snap_step(0.1, LengthUnit::In).unwrap();
    assert!((step - 0.05 * 25.4).abs() < 1e-12, "{step}");
    assert_eq!(snap_step(0.0, LengthUnit::Mm), None);
    assert_eq!(snap_step(f64::NAN, LengthUnit::Mm), None);
    assert_eq!(snap_step(f64::INFINITY, LengthUnit::Mm), None);
}

#[test]
fn a_snap_step_is_the_decimal_it_names() {
    // 6 pixels of 0.01 mm in metres, 6e-5 m: up to 1e-4 m, a tenth of a
    // millimetre, not the ulp under it that libm's `pow(10, -5)` gives.
    assert_eq!(snap_step(0.01, LengthUnit::M), Some(1e-4 * 1000.0));
    // Over 60 decades either way, the step is 1, 2 or 5 times a power of
    // ten, the nearest double to it, and the least such at or above what
    // 6 pixels come to; exactly at a step, that step.
    for k in -300..=300 {
        for (m, want) in [
            (1.0, "1"),
            (1.5, "2"),
            (2.0, "2"),
            (3.0, "5"),
            (5.0, "5"),
            (7.0, "10"),
        ] {
            let decade: f64 = format!("1e{k}").parse().unwrap();
            let least = m * decade;
            let step = snap_step(least / 6.0, LengthUnit::Mm).unwrap();
            let want: f64 = format!("{want}e{k}").parse().unwrap();
            // Divided by 6 and back, `least` can come out an ulp over.
            let least_back = least / 6.0 * 6.0;
            if least_back <= want {
                assert_eq!(step, want, "{m}e{k}");
            }
        }
    }
    // Where the step is past the largest double, none.
    assert_eq!(snap_step(f64::MAX / 6.0, LengthUnit::Mm), None);
    // 6 pixels come to 5.6e305 ft, up to 1e306 ft: 3e308 mm.
    assert_eq!(snap_step(2.8e307, LengthUnit::Ft), None);
}
