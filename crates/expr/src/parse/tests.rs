use super::*;

fn num(value: f64) -> Node {
    Node::Number(value)
}

/// The tree without spans, as nested tuples of what matters.
fn shape(expr: &Expr) -> String {
    match &expr.node {
        Node::Number(value) => format!("{value}"),
        Node::Group(inner) => format!("({})", shape(inner)),
        Node::Unit(inner, unit) => format!("[{} {}]", shape(inner), unit.symbol()),
        Node::Neg(inner) => format!("-{}", shape(inner)),
        Node::Binary(op, left, right) => {
            let op = match op {
                Op::Add => "+",
                Op::Sub => "-",
                Op::Mul => "*",
                Op::Div => "/",
            };
            format!("{{{} {op} {}}}", shape(left), shape(right))
        }
    }
}

fn parsed(text: &str) -> String {
    shape(&parse(text).unwrap_or_else(|e| panic!("{text:?}: {e}")))
}

fn error(text: &str) -> Error {
    parse(text).expect_err(text)
}

#[test]
fn numbers() {
    assert_eq!(parse("12").unwrap().node, num(12.0));
    assert_eq!(parse("12.5").unwrap().node, num(12.5));
    assert_eq!(parse(".5").unwrap().node, num(0.5));
    assert_eq!(parse("5.").unwrap().node, num(5.0));
    assert_eq!(parse("1e3").unwrap().node, num(1000.0));
    assert_eq!(parse("1.5E-3").unwrap().node, num(0.0015));
    assert_eq!(parse("2e+2").unwrap().node, num(200.0));
    assert_eq!(parse("007").unwrap().node, num(7.0));
}

#[test]
fn precedence_and_associativity() {
    assert_eq!(parsed("1 + 2 * 3"), "{1 + {2 * 3}}");
    assert_eq!(parsed("1 * 2 + 3"), "{{1 * 2} + 3}");
    assert_eq!(parsed("1 - 2 - 3"), "{{1 - 2} - 3}");
    assert_eq!(parsed("8 / 4 / 2"), "{{8 / 4} / 2}");
    assert_eq!(parsed("(1 + 2) * 3"), "{({1 + 2}) * 3}");
    assert_eq!(parsed("-2 * 3"), "{-2 * 3}");
    assert_eq!(parsed("2 * -3"), "{2 * -3}");
    assert_eq!(parsed("--2"), "--2");
    assert_eq!(parsed("+2"), "2");
    assert_eq!(parsed("1 - -2"), "{1 - -2}");
}

#[test]
fn units_bind_tightest() {
    assert_eq!(parsed("-5 mm"), "-[5 mm]");
    assert_eq!(parsed("2 * 3 in"), "{2 * [3 in]}");
    assert_eq!(parsed("(10 + 2.5) mm"), "[({10 + 2.5}) mm]");
    assert_eq!(parsed("10mm"), "[10 mm]");
    assert_eq!(parsed("1e3mm"), "[1000 mm]");
    assert_eq!(parsed("1 in + 3 mm"), "{[1 in] + [3 mm]}");
}

#[test]
fn every_unit_in_any_case() {
    for (text, unit) in [
        ("1 mm", "mm"),
        ("1 CM", "cm"),
        ("1 m", "m"),
        ("1 In", "in"),
        ("1 ft", "ft"),
        ("1 deg", "deg"),
        ("1 RAD", "rad"),
        ("1\"", "in"),
        ("1°", "deg"),
        ("1 °", "deg"),
    ] {
        assert_eq!(parsed(text), format!("[1 {unit}]"), "{text}");
    }
}

#[test]
fn whitespace_and_unicode_operators() {
    assert_eq!(parsed("  1+2  "), "{1 + 2}");
    assert_eq!(parsed("\t1\n*\u{a0}2\u{2003}"), "{1 * 2}");
    assert_eq!(parsed("3 − 1"), "{3 - 1}");
    assert_eq!(parsed("3×2÷4"), "{{3 * 2} / 4}");
}

#[test]
fn spans_cover_the_text_of_each_node() {
    let expr = parse(" (1 + 2) mm * -3 ").unwrap();
    assert_eq!(expr.span, Span::new(1, 16));
    let Node::Binary(Op::Mul, left, right) = expr.node else {
        panic!("{expr:?}");
    };
    assert_eq!(left.span, Span::new(1, 11));
    assert_eq!(right.span, Span::new(14, 16));
    let Node::Unit(group, _) = left.node else {
        panic!();
    };
    assert_eq!(group.span, Span::new(1, 8));
}

#[test]
fn errors_with_spans() {
    let cases: &[(&str, ErrorKind, (usize, usize))] = &[
        ("", ErrorKind::Empty, (0, 0)),
        ("   ", ErrorKind::Empty, (0, 3)),
        ("1 +", ErrorKind::ExpectedNumber, (3, 3)),
        ("1 + * 2", ErrorKind::ExpectedNumber, (4, 5)),
        ("mm", ErrorKind::ExpectedNumber, (0, 2)),
        ("()", ErrorKind::ExpectedNumber, (1, 2)),
        ("(1 + 2", ErrorKind::Unclosed, (0, 1)),
        ("1 + 2)", ErrorKind::Unexpected(")".into()), (5, 6)),
        ("(1 2)", ErrorKind::Unexpected("2".into()), (3, 4)),
        ("2 (3)", ErrorKind::Unexpected("(".into()), (2, 3)),
        ("1.5.2", ErrorKind::Unexpected(".2".into()), (3, 5)),
        (".", ErrorKind::Unexpected(".".into()), (0, 1)),
        ("1,5", ErrorKind::Comma, (1, 2)),
        (
            "3 furlongs",
            ErrorKind::UnknownUnit("furlongs".into()),
            (2, 10),
        ),
        ("2em", ErrorKind::UnknownUnit("em".into()), (1, 3)),
        ("2 µm", ErrorKind::Unexpected("µ".into()), (2, 4)),
        ("1 ^ 2", ErrorKind::Unexpected("^".into()), (2, 3)),
        ("1 mm mm", ErrorKind::UnitOnUnit, (5, 7)),
        ("1e400", ErrorKind::BadNumber, (0, 5)),
    ];
    for (text, kind, (start, end)) in cases {
        let error = error(text);
        assert_eq!(error.kind, *kind, "{text:?}");
        assert_eq!(error.span, Span::new(*start, *end), "{text:?}");
        // Spans are on character boundaries.
        let _ = &text[error.span.range()];
    }
}

#[test]
fn length_limit() {
    let at_limit = format!("1{}", " ".repeat(MAX_LEN - 1));
    assert_eq!(parse(&at_limit).unwrap().node, num(1.0));
    let over = format!("1{}", " ".repeat(MAX_LEN));
    assert_eq!(error(&over).kind, ErrorKind::TooLong);
    // A long number is fine within the limit, even past f64's digits.
    let digits = "1".repeat(200);
    assert!(parse(&digits).is_ok());
}

#[test]
fn depth_limit() {
    let nested = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
    assert!(parse(&nested(MAX_DEPTH)).is_ok());
    let error = error(&nested(MAX_DEPTH + 1));
    assert_eq!(error.kind, ErrorKind::TooDeep);
    assert_eq!(error.span, Span::new(MAX_DEPTH, MAX_DEPTH + 1));

    assert!(parse(&format!("{}1", "-".repeat(MAX_DEPTH))).is_ok());
    let signs = format!("{}1", "-".repeat(MAX_DEPTH + 1));
    assert_eq!(parse(&signs).unwrap_err().kind, ErrorKind::TooDeep);
    // Signs and brackets count together.
    let mixed = "-(".repeat(MAX_DEPTH / 2 + 1) + "1" + &")".repeat(MAX_DEPTH / 2 + 1);
    assert_eq!(parse(&mixed).unwrap_err().kind, ErrorKind::TooDeep);
    // Long chains aren't nesting.
    let chain = vec!["1"; MAX_LEN / 2].join("+");
    assert!(parse(&chain).is_ok());
}
