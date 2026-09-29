use std::f64::consts::{PI, TAU};

use super::*;

#[test]
fn format_in_units() {
    let mm = Some(Unit::from(LengthUnit::Mm));
    assert_eq!(format(12.5, mm), "12.5 mm");
    assert_eq!(format(12.0, mm), "12 mm");
    assert_eq!(format(1.0 / 3.0, mm), "0.333 mm");
    assert_eq!(format(-0.0001, mm), "0 mm");
    assert_eq!(format(-2.5, mm), "-2.5 mm");
    assert_eq!(format(1e6, mm), "1000000 mm");
    assert_eq!(format(12.7, Some(LengthUnit::In.into())), "0.5 in");
    assert_eq!(format(1.0, Some(LengthUnit::In.into())), "0.0394 in");
    assert_eq!(format(1500.0, Some(LengthUnit::M.into())), "1.5 m");
    assert_eq!(format(304.8, Some(LengthUnit::Ft.into())), "1 ft");
    assert_eq!(format(25.0, Some(LengthUnit::Cm.into())), "2.5 cm");
    assert_eq!(format(PI / 2.0, Some(AngleUnit::Deg.into())), "90°");
    assert_eq!(format(1.0, Some(AngleUnit::Deg.into())), "57.296°");
    assert_eq!(format(PI, Some(AngleUnit::Rad.into())), "3.14159 rad");
    assert_eq!(format(0.25, None), "0.25");
    assert_eq!(format(2.0 / 3.0, None), "0.666667");
}

#[test]
fn formatted_values_read_back() {
    for value in [0.0, 1.0, 12.5, -3.25, 1e-3, 999_999.0, 25.4, 1.0 / 3.0] {
        for unit in LengthUnit::ALL {
            let text = format(value, Some(unit.into()));
            let back = evaluate(&text, &Ask::length(LengthUnit::Mm, 1e6)).unwrap();
            // To the shown precision: a few micrometres at most.
            assert!((back - value).abs() <= 2e-3, "{text}: {back} vs {value}");
        }
    }
    for degrees in [0.0, 15.0, 90.0, -45.5, 360.0] {
        let text = format(f64::to_radians(degrees), Some(AngleUnit::Deg.into()));
        let back = evaluate(&text, &Ask::angle(LengthUnit::Mm, TAU)).unwrap();
        assert!((back.to_degrees() - degrees).abs() <= 1e-3, "{text}");
    }
}

#[test]
fn values_keep_the_text_and_check_it() {
    let ask = Ask::length(LengthUnit::Mm, 1e6).positive();
    let value = Value::new("  40 / 2 ", &ask).unwrap();
    assert_eq!(value.text, "40 / 2");
    assert_eq!(value.value, 20.0);
    assert_eq!(value.check(&ask), Ok(()));

    let changed = Value {
        value: 20.5,
        ..value.clone()
    };
    let error = changed.check(&ask).unwrap_err();
    assert_eq!(
        error.kind,
        ErrorKind::Disagrees {
            stored: 20.5,
            evaluated: 20.0
        }
    );
    assert_eq!(error.span, Span::new(0, 6));

    let nan = Value {
        value: f64::NAN,
        ..value.clone()
    };
    assert!(nan.check(&ask).is_err());
    // Read in other units, bare numbers mean something else.
    assert!(value.check(&Ask::length(LengthUnit::In, 1e6)).is_err());
    // Text from a file is checked like typed text.
    let bad = Value {
        text: "1 +".into(),
        value: 1.0,
    };
    assert_eq!(bad.check(&ask).unwrap_err().kind, ErrorKind::ExpectedNumber);
    let long = Value {
        text: "1".repeat(10_000),
        value: 1.0,
    };
    assert_eq!(long.check(&ask).unwrap_err().kind, ErrorKind::TooLong);
    assert_eq!(
        Value::new("0", &ask).unwrap_err().kind,
        ErrorKind::NotPositive
    );
}

#[test]
fn messages() {
    let message = |text: &str, ask: &Ask| evaluate(text, ask).unwrap_err().to_string();
    let mm = Ask::length(LengthUnit::Mm, 1000.0);
    let angle = Ask::angle(LengthUnit::Mm, TAU);
    assert_eq!(
        message("1 mm + 2 deg", &mm),
        "can't add a length and an angle"
    );
    assert_eq!(
        message("1 mm - 2 mm * 2 mm", &mm),
        "can't subtract a length and an area"
    );
    assert_eq!(message("2 mm * 3 mm", &mm), "that's an area, not a length");
    assert_eq!(message("2 in", &angle), "that's a length, not an angle");
    assert_eq!(message("2 m", &mm), "must be at most 1000 mm");
    assert_eq!(message("400", &angle), "must be at most 360°");
    assert_eq!(message("0", &mm.positive()), "must be above zero");
    assert_eq!(message("1 / 0", &mm), "division by zero");
    assert_eq!(message("1,5", &mm), "use '.' for decimals");
    assert_eq!(message("3 yd", &mm), "unknown unit 'yd'");
    assert_eq!(message("(1", &mm), "missing ')'");
    assert_eq!(message("", &mm), "enter a value");
    assert_eq!(message("1 $", &mm), "unexpected '$'");
    assert_eq!(message("(1 in) mm", &mm), "that already has a unit");
}

/// xorshift64*, so the test needs no crate and repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Every ask a caller might make, bounds included.
fn asks() -> Vec<Ask> {
    let mut asks = Vec::new();
    for units in LengthUnit::ALL {
        asks.push(Ask::length(units, 1e6));
        asks.push(Ask::length(units, f64::MAX).positive());
        asks.push(Ask::angle(units, TAU));
        asks.push(Ask::number(units, f64::INFINITY));
    }
    asks
}

/// Whatever the input, evaluating never panics, and what it accepts is
/// finite, within bounds and pins to the same value.
fn exercise(text: &str, asks: &[Ask]) {
    for ask in asks {
        match evaluate(text, ask) {
            Ok(value) => {
                assert!(value.is_finite() && value.abs() <= ask.max, "{text:?}");
                assert!(!ask.positive || value > 0.0, "{text:?}");
                if let Ok(pinned) = pin_units(text, ask) {
                    assert_eq!(evaluate(&pinned, ask), Ok(value), "{text:?} {pinned:?}");
                }
            }
            Err(error) => {
                assert!(error.span.start <= error.span.end, "{text:?}");
                // On boundaries, or slicing would panic.
                if text.len() <= MAX_LEN {
                    let _ = &text[error.span.range()];
                }
                assert!(!error.to_string().is_empty());
            }
        }
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    let asks = asks();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3000 {
        let len = rng.below(MAX_LEN + 20);
        let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        exercise(&String::from_utf8_lossy(&bytes), &asks);
    }
}

#[test]
fn arbitrary_tokens_never_panic() {
    const PIECES: &[&str] = &[
        "0", "1", "7", "25.4", ".5", "5.", "1e3", "1e308", "1e-320", "9e", "e", "+", "-", "*", "/",
        "(", ")", " ", "mm", "cm", "m", "in", "ft", "deg", "rad", "\"", "°", "−", "×", "÷", ",",
        "µ", "x", "\u{a0}",
    ];
    let asks = asks();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for _ in 0..20_000 {
        let count = rng.below(40);
        let text: String = (0..count)
            .map(|_| PIECES[rng.below(PIECES.len())])
            .collect();
        exercise(&text, &asks);
    }
}
