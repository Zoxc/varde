use std::f64::consts::{PI, TAU};

use super::*;
use crate::LengthUnit;

const MAX: f64 = 1e6;

fn mm() -> Ask {
    Ask::length(LengthUnit::Mm, MAX)
}

fn inches() -> Ask {
    Ask::length(LengthUnit::In, MAX)
}

fn angle() -> Ask {
    Ask::angle(LengthUnit::Mm, TAU)
}

fn number() -> Ask {
    Ask::number(LengthUnit::Mm, MAX)
}

fn eval(text: &str, ask: &Ask) -> f64 {
    evaluate(text, ask).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
}

#[track_caller]
fn assert_close(text: &str, ask: &Ask, expected: f64) {
    let value = eval(text, ask);
    assert!(close(value, expected), "{text:?}: {value} != {expected}");
}

fn kind(text: &str, ask: &Ask) -> ErrorKind {
    evaluate(text, ask).expect_err(text).kind
}

#[test]
fn arithmetic() {
    assert_close("40 / 2", &mm(), 20.0);
    assert_close("(10 + 2.5) * 2", &mm(), 25.0);
    assert_close("1 + 2 * 3", &mm(), 7.0);
    assert_close("(1 + 2) * 3", &mm(), 9.0);
    assert_close("10 - 4 - 3", &mm(), 3.0);
    assert_close("24 / 4 / 2", &mm(), 3.0);
    assert_close("-3 + 5", &mm(), 2.0);
    assert_close("--3", &mm(), 3.0);
    assert_close("2 * -3", &mm(), -6.0);
    assert_close("+4", &mm(), 4.0);
}

#[test]
fn length_units_convert_to_mm() {
    assert_close("1 mm", &mm(), 1.0);
    assert_close("1 cm", &mm(), 10.0);
    assert_close("1 m", &mm(), 1000.0);
    assert_close("1 in", &mm(), 25.4);
    assert_close("1\"", &mm(), 25.4);
    assert_close("1 ft", &mm(), 304.8);
    assert_close("1 in + 3 mm", &mm(), 28.4);
    assert_close("(10 + 2.5) mm", &mm(), 12.5);
    assert_close("2 * 3 in", &mm(), 152.4);
    assert_close("1 ft - 12 in", &mm(), 0.0);
    // The design's units only change bare numbers.
    assert_close("1 in + 3 mm", &inches(), 28.4);
}

#[test]
fn bare_numbers_take_the_design_unit() {
    assert_close("20", &mm(), 20.0);
    assert_close("20", &inches(), 508.0);
    assert_close("40 / 2", &inches(), 508.0);
    assert_close("1 in + 3", &mm(), 28.4);
    assert_close("3 + 1 in", &inches(), 101.6);
    assert_close("2 * 3 + 1 in", &mm(), 31.4);
    assert_close("1 in * 2 + 3", &mm(), 53.8);
    assert_close("-(2) + 1 cm", &mm(), 8.0);
}

#[test]
fn angles_convert_to_radians() {
    assert_close("90 deg", &angle(), PI / 2.0);
    assert_close("90°", &angle(), PI / 2.0);
    assert_close("0.5 rad", &angle(), 0.5);
    assert_close("90 deg - 15 deg", &angle(), 75f64.to_radians());
    assert_close("90 deg - 15", &angle(), 75f64.to_radians());
    assert_close("45", &angle(), PI / 4.0);
    assert_close("180 / 4", &angle(), PI / 4.0);
    assert_close("1 rad + 0", &angle(), 1.0);
    // A full turn is within a turn, either way.
    assert_close("360", &angle(), TAU);
    assert_close("-360 deg", &angle(), -TAU);
    assert_close("2 * 1.5 rad", &angle(), 3.0);
}

#[test]
fn quotients_and_products_of_units() {
    // A ratio of lengths is a number.
    assert_close("1 in / 1 mm", &number(), 25.4);
    assert_close("10 mm * 10 mm / 5 mm", &mm(), 20.0);
    assert_close("1 m / 2", &mm(), 500.0);
    assert_close("100 mm * (1 in / 1 mm)", &mm(), 2540.0);
    assert_close("90 deg / 1 deg", &number(), 90.0);
    assert_close("7", &number(), 7.0);
}

#[test]
fn mismatches_are_refused() {
    let mismatch = |add, left, right| ErrorKind::Mismatch { add, left, right };
    assert_eq!(
        kind("1 mm + 1 deg", &mm()),
        mismatch(true, Kind::Length, Kind::Angle)
    );
    assert_eq!(
        kind("1 rad - 2 in", &angle()),
        mismatch(false, Kind::Angle, Kind::Length)
    );
    assert_eq!(
        kind("1 mm * 1 mm + 1 mm", &mm()),
        mismatch(true, Kind::Area, Kind::Length)
    );
    // A ratio isn't bare: it doesn't take the design's unit.
    assert_eq!(
        kind("1 in / 1 mm + 1 mm", &mm()),
        mismatch(true, Kind::Number, Kind::Length)
    );
    // Nor does an area take one.
    assert_eq!(
        kind("1 mm * 1 mm + 2", &mm()),
        mismatch(true, Kind::Area, Kind::Number)
    );
    let wrong = |want, got| ErrorKind::Wrong { want, got };
    assert_eq!(
        kind("2 mm * 3 mm", &mm()),
        wrong(Quantity::Length, Kind::Area)
    );
    assert_eq!(kind("90 deg", &mm()), wrong(Quantity::Length, Kind::Angle));
    assert_eq!(kind("1 in", &angle()), wrong(Quantity::Angle, Kind::Length));
    assert_eq!(
        kind("1 in / 1 mm", &mm()),
        wrong(Quantity::Length, Kind::Number)
    );
    assert_eq!(
        kind("1 mm * 1 deg", &angle()),
        wrong(Quantity::Angle, Kind::Mixed)
    );
    assert_eq!(
        kind("3 mm", &number()),
        wrong(Quantity::Number, Kind::Length)
    );
    assert_eq!(kind("(1 in) mm", &mm()), ErrorKind::UnitOnUnit);
    assert_eq!(kind("(2 * 1 in) mm", &mm()), ErrorKind::UnitOnUnit);
}

#[test]
fn mismatch_spans_cover_both_sides() {
    let text = "2 * (1 mm + 1 deg) ";
    let error = evaluate(text, &mm()).unwrap_err();
    assert_eq!(&text[error.span.range()], "1 mm + 1 deg");
    let error = evaluate("4 / (2 - 2)", &mm()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::DivideByZero);
    assert_eq!(error.span, Span::new(4, 11));
    let error = evaluate(" 2 mm * 3 mm", &mm()).unwrap_err();
    assert_eq!(error.span, Span::new(1, 12));
}

#[test]
fn division_by_zero() {
    assert_eq!(kind("1 / 0", &mm()), ErrorKind::DivideByZero);
    assert_eq!(kind("1 / -0", &mm()), ErrorKind::DivideByZero);
    assert_eq!(kind("1 mm / 0 mm", &number()), ErrorKind::DivideByZero);
    assert_eq!(kind("0 / 0", &mm()), ErrorKind::DivideByZero);
}

#[test]
fn overflow_is_refused_at_every_step() {
    let huge = Ask::length(LengthUnit::Mm, f64::MAX);
    assert_eq!(kind("1e308 * 10", &huge), ErrorKind::Overflow);
    assert_eq!(kind("1e308 + 1e308", &huge), ErrorKind::Overflow);
    assert_eq!(kind("-1e308 - 1e308", &huge), ErrorKind::Overflow);
    assert_eq!(kind("1 / 1e-320", &huge), ErrorKind::Overflow);
    // Converting to millimetres overflows, explicitly or by default.
    assert_eq!(kind("1e308 ft", &huge), ErrorKind::Overflow);
    let feet = Ask::length(LengthUnit::Ft, f64::MAX);
    assert_eq!(kind("1e308", &feet), ErrorKind::Overflow);
    assert_eq!(kind("1e308 + 1 mm", &feet), ErrorKind::Overflow);
    // An intermediate overflow is refused even if a later step would
    // bring it back.
    assert_eq!(kind("1e200 * 1e200 / 1e200", &huge), ErrorKind::Overflow);
    // Underflow to zero is fine: it's a number.
    assert_close("1e-200 * 1e-200", &huge, 0.0);
}

#[test]
fn bounds_from_the_ask() {
    let small = Ask::length(LengthUnit::Mm, 100.0);
    assert_close("100", &small, 100.0);
    assert_close("-100", &small, -100.0);
    assert_eq!(
        kind("100.001", &small),
        ErrorKind::TooLarge {
            max: 100.0,
            unit: Some(LengthUnit::Mm.into()),
            under: false,
        }
    );
    assert_eq!(
        kind("-4 in", &small),
        ErrorKind::TooLarge {
            max: 100.0,
            unit: Some(LengthUnit::Mm.into()),
            under: false,
        }
    );
    assert_eq!(
        kind("361", &angle()),
        ErrorKind::TooLarge {
            max: TAU,
            unit: Some(AngleUnit::Deg.into()),
            under: false,
        }
    );
    let positive = small.positive();
    assert_close("0.001", &positive, 0.001);
    assert_eq!(kind("0", &positive), ErrorKind::NotPositive);
    assert_eq!(kind("-1", &positive), ErrorKind::NotPositive);
    assert_eq!(kind("1 - 1", &positive), ErrorKind::NotPositive);
    // A NaN bound refuses everything.
    let nan = Ask::length(LengthUnit::Mm, f64::NAN);
    assert!(matches!(kind("1", &nan), ErrorKind::TooLarge { .. }));
}

#[test]
fn pinning_writes_the_design_unit_in() {
    let pin = |text: &str, ask: &Ask| pin_units(text, ask).unwrap();
    assert_eq!(pin("20", &mm()), "20 mm");
    assert_eq!(pin("20", &inches()), "20 in");
    assert_eq!(pin("40 / 2", &mm()), "(40 / 2) mm");
    assert_eq!(pin("(40 / 2)", &mm()), "(40 / 2) mm");
    assert_eq!(pin("-3", &mm()), "-3 mm");
    assert_eq!(pin("-(1 + 2)", &mm()), "-(1 + 2) mm");
    assert_eq!(pin("1 in + 3", &mm()), "1 in + 3 mm");
    assert_eq!(pin("2 * 3 + 1 in", &mm()), "(2 * 3) mm + 1 in");
    assert_eq!(pin("1 in + 2 * 3 - 4", &mm()), "1 in + (2 * 3) mm - 4 mm");
    assert_eq!(pin("  7  ", &mm()), "  7 mm  ");
    assert_eq!(pin("1 in", &mm()), "1 in");
    // Angles: bare numbers are degrees in any design.
    assert_eq!(pin("90 - 15", &angle()), "90 - 15");
    assert_eq!(pin("1 in / 1 mm * 3", &number()), "1 in / 1 mm * 3");
    // Pinning is refused where it takes the text over the limit.
    let long = vec!["1 in + 1"; 20].join(" + ");
    assert!(long.len() <= MAX_LEN);
    assert_eq!(
        pin_units(&long, &mm()).unwrap_err().kind,
        ErrorKind::TooLong
    );
    // An expression that doesn't evaluate isn't pinned.
    assert_eq!(
        pin_units("1 mm + 1 deg", &mm()).unwrap_err().kind,
        kind("1 mm + 1 deg", &mm())
    );
}

#[test]
fn pinned_text_means_the_same_in_any_units() {
    for text in [
        "20",
        "40 / 2",
        "-(1 + 2)",
        "1 in + 3",
        "2 * 3 + 1 in",
        "1 in + 2 * 3 - 4",
        "---5 + 1 cm",
        "+(2) - 1 ft",
    ] {
        for from in LengthUnit::ALL {
            let ask = Ask::length(from, MAX);
            let value = eval(text, &ask);
            let pinned = pin_units(text, &ask).unwrap();
            for to in LengthUnit::ALL {
                let other = Ask::length(to, MAX);
                assert_eq!(eval(&pinned, &other), value, "{text:?} as {pinned:?}");
            }
        }
    }
}

#[test]
fn a_least_value_and_a_bound_to_stay_under() {
    let least = mm().positive().at_least(1e-3);
    assert_close("0.001", &least, 1e-3);
    assert_close("1 in", &least, 25.4);
    // Zero and below are still not above zero; above it, too small.
    assert_eq!(kind("0", &least), ErrorKind::NotPositive);
    let too_small = ErrorKind::TooSmall {
        min: 1e-3,
        unit: Some(LengthUnit::Mm.into()),
    };
    assert_eq!(kind("0.0009", &least), too_small);
    assert_eq!(kind("1e-300", &least), too_small);
    // In inches too, the least is said in millimetres, which show it.
    assert_eq!(
        kind("0.00001", &inches().positive().at_least(1e-3)),
        too_small
    );

    let turn = angle().positive().under_max();
    assert_close("359.999", &turn, 359.999f64.to_radians());
    assert_eq!(
        kind("360", &turn),
        ErrorKind::TooLarge {
            max: TAU,
            unit: Some(AngleUnit::Deg.into()),
            under: true,
        }
    );
    assert_eq!(kind("0", &turn), ErrorKind::NotPositive);
    assert_eq!(
        evaluate("360", &turn).unwrap_err().to_string(),
        "must be under 360°"
    );
    assert_eq!(
        evaluate("0.0001", &least).unwrap_err().to_string(),
        "must be at least 0.001 mm"
    );
}

#[test]
fn a_count_is_a_whole_number_within_its_bounds() {
    let count = Ask::number(LengthUnit::Mm, 64.0).at_least(3.0).whole();
    assert_close("6", &count, 6.0);
    assert_close("2 * 4", &count, 8.0);
    assert_close("64", &count, 64.0);
    assert_eq!(kind("6.5", &count), ErrorKind::NotWhole);
    assert_eq!(kind("10 / 3", &count), ErrorKind::NotWhole);
    assert_eq!(
        kind("2", &count),
        ErrorKind::TooSmall {
            min: 3.0,
            unit: None
        }
    );
    assert!(matches!(kind("65", &count), ErrorKind::TooLarge { .. }));
    assert!(matches!(kind("6 mm", &count), ErrorKind::Wrong { .. }));
    assert_eq!(
        evaluate("6.5", &count).unwrap_err().to_string(),
        "must be a whole number"
    );
}
