use std::f64::consts::PI;

use super::*;
use crate::{Ask, Kind, evaluate, pin_units};

fn params(list: &[(&str, &str)]) -> Params {
    Params::evaluate(list.iter().copied(), LengthUnit::Mm)
}

fn value(params: &Params, name: &str) -> Resolved {
    params
        .get(name)
        .unwrap_or_else(|| panic!("{name}?"))
        .clone()
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn error(params: &Params, name: &str) -> Error {
    params.get(name).unwrap().clone().expect_err(name)
}

#[test]
fn parameters_resolve_in_any_order() {
    let params = params(&[
        ("half", "width / 2"),
        ("width", "40 mm"),
        ("turn", "90 deg"),
        ("count", "3"),
        ("ratio", "width / half"),
    ]);
    assert_eq!(
        value(&params, "half"),
        Resolved {
            value: 20.0,
            quantity: Quantity::Length
        }
    );
    assert_eq!(value(&params, "turn").quantity, Quantity::Angle);
    assert!((value(&params, "turn").value - PI / 2.0).abs() < 1e-15);
    assert_eq!(
        value(&params, "count"),
        Resolved {
            value: 3.0,
            quantity: Quantity::Number
        }
    );
    assert_eq!(value(&params, "ratio").value, 2.0);
    assert_eq!(value(&params, "ratio").quantity, Quantity::Number);
    let names: Vec<&str> = params.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["half", "width", "turn", "count", "ratio"]);
}

#[test]
fn bare_numbers_in_a_parameter_take_the_design_unit_beside_a_length() {
    let inches = Params::evaluate([("a", "1 mm + 1"), ("b", "2")], LengthUnit::In);
    assert_eq!(value(&inches, "a").value, 1.0 + 25.4);
    // Pinned, it means the same in any units.
    let pinned = inches.pin_units("1 mm + 1", LengthUnit::In).unwrap();
    assert_eq!(pinned, "1 mm + 1 in");
    let mm = Params::evaluate([("a", pinned.as_str())], LengthUnit::Mm);
    assert_eq!(value(&mm, "a").value, 1.0 + 25.4);
    assert_eq!(inches.pin_units("2", LengthUnit::In).unwrap(), "2");
}

#[test]
fn values_use_parameters_by_name() {
    let params = params(&[("width", "40 mm"), ("count", "4"), ("tilt", "15 deg")]);
    let mm = Ask::length(LengthUnit::Mm, 1e6).with_params(&params);
    assert_eq!(evaluate("width / 2", &mm), Ok(20.0));
    assert_eq!(evaluate("width + 5", &mm), Ok(45.0));
    assert_eq!(evaluate("width / count", &mm), Ok(10.0));
    let angle = Ask::angle(LengthUnit::Mm, 7.0).with_params(&params);
    assert!((evaluate("tilt * 2", &angle).unwrap() - PI / 6.0).abs() < 1e-15);
    // A number isn't a length, nor does it take the design's units.
    assert_eq!(
        evaluate("count", &mm).unwrap_err().kind,
        ErrorKind::Wrong {
            want: Quantity::Length,
            got: Kind::Number
        }
    );
    assert_eq!(
        evaluate("width mm", &mm).unwrap_err().kind,
        ErrorKind::UnitOnParam("width".into())
    );
    // A number parameter too: its units are its own, none.
    let error = evaluate("count mm", &mm).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnitOnParam("count".into()));
    assert_eq!(
        error.to_string(),
        "a parameter takes no unit after it: multiply by one, as 'count * 1 mm'"
    );
    // Names take no pins: they mean the same in any units.
    assert_eq!(pin_units("width + 5", &mm).unwrap(), "width + 5 mm");
    let inches = Ask::length(LengthUnit::In, 1e6).with_params(&params);
    assert_eq!(evaluate("width", &inches), Ok(40.0));
    // Without the parameters, the name is unknown.
    let none = Ask::length(LengthUnit::Mm, 1e6);
    assert_eq!(
        evaluate("width", &none).unwrap_err().kind,
        ErrorKind::UnknownName {
            name: "width".into(),
            suggestion: None
        }
    );
}

#[test]
fn unknown_names_suggest_a_near_one() {
    let params = params(&[("width", "40 mm"), ("Height", "10 mm")]);
    let mm = Ask::length(LengthUnit::Mm, 1e6).with_params(&params);
    let error = evaluate("2 * widht", &mm).unwrap_err();
    assert_eq!(
        error.kind,
        ErrorKind::UnknownName {
            name: "widht".into(),
            suggestion: Some("width".into())
        }
    );
    assert_eq!(error.span, Span::new(4, 9));
    assert_eq!(
        error.to_string(),
        "unknown parameter 'widht', did you mean 'width'?"
    );
    let suggested = |text: &str| match evaluate(text, &mm).unwrap_err().kind {
        ErrorKind::UnknownName { suggestion, .. } => suggestion,
        kind => panic!("{kind:?}"),
    };
    assert_eq!(suggested("height"), Some("Height".into()));
    assert_eq!(suggested("depth"), None);
    assert_eq!(suggested("w"), None);
}

#[test]
fn cycles_and_their_users_are_errors() {
    let params = params(&[
        ("a", "b + 1 mm"),
        ("b", "c * 2"),
        ("c", "a"),
        ("d", "a / 2"),
        ("e", "e"),
        ("f", "3 mm"),
    ]);
    for name in ["a", "b", "c", "e"] {
        assert_eq!(error(&params, name).kind, ErrorKind::Cycle, "{name}");
    }
    // Each points at the use that leads round.
    assert_eq!(error(&params, "a").span, Span::new(0, 1));
    assert_eq!(error(&params, "c").span, Span::new(0, 1));
    assert_eq!(error(&params, "d").kind, ErrorKind::BrokenParam("a".into()));
    assert_eq!(value(&params, "f").value, 3.0);
    let mm = Ask::length(LengthUnit::Mm, 1e6).with_params(&params);
    assert_eq!(
        evaluate("f + a", &mm).unwrap_err().kind,
        ErrorKind::BrokenParam("a".into())
    );
    assert_eq!(evaluate("f", &mm), Ok(3.0));
}

#[test]
fn names_and_kinds_are_checked() {
    let params = params(&[
        ("area", "2 mm * 3 mm"),
        ("mm", "3"),
        ("2x", "3"),
        ("x", "1"),
        ("x", "2"),
        ("", "1"),
    ]);
    assert_eq!(
        error(&params, "area").kind,
        ErrorKind::ParamKind(Kind::Area)
    );
    assert_eq!(
        error(&params, "mm").kind,
        ErrorKind::BadName(NameError::Unit)
    );
    assert_eq!(
        error(&params, "2x").kind,
        ErrorKind::BadName(NameError::NotAWord)
    );
    // The first of a name is the one used.
    assert_eq!(value(&params, "x").value, 1.0);
    assert_eq!(
        params.result(4).unwrap().clone().unwrap_err().kind,
        ErrorKind::Duplicate("x".into())
    );
    assert_eq!(
        params.result(5).unwrap().clone().unwrap_err().kind,
        ErrorKind::BadName(NameError::Empty)
    );

    assert_eq!(check_name("width_2"), Ok(()));
    assert_eq!(check_name("_x"), Ok(()));
    assert_eq!(check_name("DEG"), Err(NameError::Unit));
    assert_eq!(check_name("a b"), Err(NameError::NotAWord));
    assert_eq!(check_name("é"), Err(NameError::NotAWord));
    assert_eq!(check_name(&"a".repeat(MAX_NAME_LEN)), Ok(()));
    assert_eq!(
        check_name(&"a".repeat(MAX_NAME_LEN + 1)),
        Err(NameError::TooLong)
    );
}

#[test]
fn too_many_parameters_are_errors() {
    let names: Vec<String> = (0..MAX_PARAMS + 2).map(|i| format!("p{i}")).collect();
    let params = Params::evaluate(
        names.iter().map(|name| (name.as_str(), "1")),
        LengthUnit::Mm,
    );
    assert_eq!(value(&params, "p0").value, 1.0);
    assert_eq!(
        error(&params, &format!("p{MAX_PARAMS}")).kind,
        ErrorKind::TooManyParams
    );
}

#[test]
fn a_long_chain_resolves_without_recursion() {
    let names: Vec<String> = (0..MAX_PARAMS).map(|i| format!("p{i}")).collect();
    let texts: Vec<String> = (0..MAX_PARAMS)
        .map(|i| match i + 1 {
            next if next < MAX_PARAMS => format!("p{next} + 1 mm"),
            _ => "1 mm".into(),
        })
        .collect();
    let params = Params::evaluate(
        names
            .iter()
            .zip(&texts)
            .map(|(n, t)| (n.as_str(), t.as_str())),
        LengthUnit::Mm,
    );
    assert_eq!(value(&params, "p0").value, MAX_PARAMS as f64);
    // And round: the last uses the first.
    let mut texts = texts;
    texts[MAX_PARAMS - 1] = "p0".into();
    let params = Params::evaluate(
        names
            .iter()
            .zip(&texts)
            .map(|(n, t)| (n.as_str(), t.as_str())),
        LengthUnit::Mm,
    );
    assert!(params.iter().all(|(_, result)| {
        result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Cycle)
    }));
}

#[test]
fn names_found_and_renamed() {
    let text = "width + 2 * (width_2 - w) / width";
    let found: Vec<&str> = names(text).iter().map(|span| &text[span.range()]).collect();
    assert_eq!(found, ["width", "width_2", "w", "width"]);
    assert!(uses(text, "w"));
    assert!(!uses(text, "wid"));
    assert_eq!(
        rename(text, "width", "w2").unwrap(),
        "w2 + 2 * (width_2 - w) / w2"
    );
    assert_eq!(rename(text, "nothing", "x").unwrap(), text);
    // Units aren't names, and a word after a number is a unit.
    assert!(names("2 mm + 3 in").is_empty());
    assert!(names("2 width").is_empty());
    // Names past a lex error are found too, for renaming and uses.
    let erring = "a + 1,5 + b $ c + 2 yd * d";
    let found: Vec<&str> = names(erring)
        .iter()
        .map(|span| &erring[span.range()])
        .collect();
    assert_eq!(found, ["a", "b", "c", "d"]);
    assert!(uses(erring, "d"));
    assert_eq!(
        rename(erring, "d", "e").unwrap(),
        "a + 1,5 + b $ c + 2 yd * e"
    );
    let long = format!("x{}", " + x".repeat(60));
    assert!(long.len() <= MAX_LEN);
    assert_eq!(
        rename(&long, "x", "longer").unwrap_err().kind,
        ErrorKind::TooLong
    );
}

#[test]
fn banded_edits_agree_with_the_whole_table() {
    fn whole(a: &str, b: &str) -> usize {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, &x) in a.iter().enumerate() {
            let mut diagonal = row[0];
            row[0] = i + 1;
            for (j, &y) in b.iter().enumerate() {
                let above = row[j + 1];
                row[j + 1] = (above + 1)
                    .min(row[j] + 1)
                    .min(diagonal + usize::from(x != y));
                diagonal = above;
            }
        }
        row[b.len()]
    }
    let words = [
        "", "a", "ab", "ba", "abc", "acb", "width", "widht", "wdth", "with", "widths", "xwidthx",
        "height", "eight", "heigth", "abcdef", "fedcba", "aabbcc",
    ];
    for a in words {
        for b in words {
            for most in 0..=3 {
                let full = whole(a, b);
                let expected = (full <= most).then_some(full);
                assert_eq!(edits_within(a, b, most), expected, "{a:?} {b:?} {most}");
            }
        }
    }
}

#[test]
fn many_unknown_names_evaluate_quickly() {
    // The worst case for suggestions: every parameter the longest name,
    // all alike, each using an unknown name near all of them.
    let stem = "a".repeat(MAX_NAME_LEN - 4);
    let names: Vec<String> = (0..MAX_PARAMS).map(|i| format!("{stem}{i:04}")).collect();
    let texts: Vec<String> = (0..MAX_PARAMS).map(|i| format!("{stem}x{i:03}")).collect();
    let start = std::time::Instant::now();
    let params = Params::evaluate(
        names
            .iter()
            .zip(&texts)
            .map(|(n, t)| (n.as_str(), t.as_str())),
        LengthUnit::Mm,
    );
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "{elapsed:?}");
    // The first ones still get their suggestion.
    match error(&params, &names[0]).kind {
        ErrorKind::UnknownName { suggestion, .. } => assert!(suggestion.is_some()),
        kind => panic!("{kind:?}"),
    }
    // And a value asked for after suggests as before.
    let mm = Ask::length(LengthUnit::Mm, 1e6).with_params(&params);
    match evaluate(&format!("{stem}000"), &mm).unwrap_err().kind {
        ErrorKind::UnknownName { suggestion, .. } => assert!(suggestion.is_some()),
        kind => panic!("{kind:?}"),
    }
}
