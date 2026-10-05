use glam::DVec2;

use super::*;
use crate::{Constraint, ConstraintEntry, CurveEntry, Point, Side};

const MAX: f64 = 100.0;
const DESIGN: Design = Design {
    max: MAX,
    units: LengthUnit::Mm,
};

/// Points 0 and 1, line 2 between them, circle 3 around point 0 and a
/// constraint 4 on the line.
fn sketch() -> Sketch {
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::ZERO).unwrap();
    let b = sketch.add_point(DVec2::splat(MAX)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    sketch
        .add_curve(
            Curve::Circle {
                center: a,
                radius: MAX,
            },
            false,
        )
        .unwrap();
    sketch.add_constraint(Constraint::Horizontal(line)).unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    sketch
}

#[test]
fn check_bounds_coordinates_and_radii() {
    let sketch = sketch();
    assert_eq!(
        sketch.check(&Design {
            max: MAX / 2.0,
            ..DESIGN
        }),
        Err(SketchError::Coordinate {
            id: Id(1),
            value: MAX,
            max: MAX / 2.0
        })
    );
    for bad in [f64::NAN, f64::INFINITY, -MAX - 1.0] {
        let mut sketch = sketch.clone();
        sketch.points[0].at.y = bad;
        assert!(matches!(
            sketch.check(&DESIGN),
            Err(SketchError::Coordinate { id: Id(0), .. })
        ));
    }
    for bad in [f64::NAN, 0.0, -1.0, MAX + 1.0, f64::INFINITY] {
        let mut sketch = sketch.clone();
        sketch.curves[1].curve = Curve::Circle {
            center: Id(0),
            radius: bad,
        };
        assert!(matches!(
            sketch.check(&DESIGN),
            Err(SketchError::Radius { id: Id(3), .. })
        ));
    }
}

#[test]
fn check_refuses_ids_out_of_order_or_reused() {
    let mut swapped = sketch();
    swapped.points.swap(0, 1);
    assert_eq!(
        swapped.check(&DESIGN),
        Err(SketchError::Order {
            id: Id(0),
            before: Id(1)
        })
    );
    let mut twins = sketch();
    twins.points[1].id = Id(0);
    assert_eq!(
        twins.check(&DESIGN),
        Err(SketchError::Order {
            id: Id(0),
            before: Id(0)
        })
    );
    let mut curves = sketch();
    curves.curves.swap(0, 1);
    assert!(matches!(
        curves.check(&DESIGN),
        Err(SketchError::Order { .. })
    ));
    let mut constraints = sketch();
    constraints.constraints.insert(
        0,
        ConstraintEntry {
            id: Id(4),
            constraint: Constraint::Vertical(Id(2)),
        },
    );
    assert!(matches!(
        constraints.check(&DESIGN),
        Err(SketchError::Order { .. })
    ));

    // Ids at or past the next one would be handed out again.
    for next_id in [0, 4] {
        let mut sketch = sketch();
        sketch.next_id = next_id;
        assert!(matches!(sketch.check(&DESIGN), Err(SketchError::NextId(_))));
    }
    let mut sketch = sketch();
    sketch.next_id = 1;
    assert_eq!(sketch.check(&DESIGN), Err(SketchError::NextId(Id(1))));
    sketch.next_id = 3;
    assert_eq!(sketch.check(&DESIGN), Err(SketchError::NextId(Id(3))));

    // An id names one thing, in whichever list.
    let mut shared = self::sketch();
    shared.curves[0].id = Id(1);
    assert_eq!(shared.check(&DESIGN), Err(SketchError::Shared(Id(1))));
    let mut shared = self::sketch();
    shared.constraints[0].id = Id(3);
    assert_eq!(shared.check(&DESIGN), Err(SketchError::Shared(Id(3))));
    let mut shared = self::sketch();
    shared.constraints[0].id = Id(0);
    assert_eq!(shared.check(&DESIGN), Err(SketchError::Shared(Id(0))));
}

#[test]
fn check_refuses_references_to_the_wrong_kind() {
    let refused = |curve: Option<Curve>, constraint: Option<Constraint>, error: SketchError| {
        let mut sketch = sketch();
        if let Some(curve) = curve {
            sketch.add_curve(curve, true).unwrap();
        }
        if let Some(constraint) = constraint {
            sketch.add_constraint(constraint).unwrap();
        }
        assert_eq!(sketch.check(&DESIGN), Err(error));
    };
    let (a, b, line, circle, horizontal) = (Id(0), Id(1), Id(2), Id(3), Id(4));
    let new = Id(5);
    let missing = Id(1000);

    refused(
        Some(Curve::Line {
            start: a,
            end: missing,
        }),
        None,
        SketchError::Reference {
            from: new,
            to: missing,
            expected: Role::Point,
        },
    );
    refused(
        Some(Curve::Circle {
            center: line,
            radius: 1.0,
        }),
        None,
        SketchError::Reference {
            from: new,
            to: line,
            expected: Role::Point,
        },
    );
    refused(
        Some(Curve::Arc {
            center: a,
            start: b,
            end: horizontal,
        }),
        None,
        SketchError::Reference {
            from: new,
            to: horizontal,
            expected: Role::Point,
        },
    );
    for constraint in [Constraint::Horizontal(circle), Constraint::Vertical(circle)] {
        refused(
            None,
            Some(constraint),
            SketchError::Reference {
                from: new,
                to: circle,
                expected: Role::LineOrHandle,
            },
        );
    }
    // A point that's no handle's tip names no handle.
    refused(None, Some(Constraint::Vertical(a)), SketchError::Unfit(new));
    refused(
        None,
        Some(Constraint::Parallel(line, b)),
        SketchError::Unfit(new),
    );
    for constraint in [
        Constraint::Coincident(a, line),
        Constraint::HorizontalPoints(a, line),
        Constraint::VerticalPoints(a, line),
    ] {
        refused(
            None,
            Some(constraint),
            SketchError::Reference {
                from: new,
                to: line,
                expected: Role::Point,
            },
        );
    }
    // A constraint can't name a constraint either, nor itself.
    refused(
        None,
        Some(Constraint::Horizontal(new)),
        SketchError::Reference {
            from: new,
            to: new,
            expected: Role::LineOrHandle,
        },
    );
}

#[test]
fn check_refuses_an_item_naming_one_twice() {
    let (a, b) = (Id(0), Id(1));
    let from = Id(5);
    for curve in [
        Curve::Line { start: a, end: a },
        Curve::Arc {
            center: a,
            start: b,
            end: a,
        },
        Curve::Arc {
            center: b,
            start: b,
            end: a,
        },
    ] {
        let mut sketch = sketch();
        sketch.add_curve(curve, false).unwrap();
        assert!(matches!(
            sketch.check(&DESIGN),
            Err(SketchError::Repeated { from: f, .. }) if f == from
        ));
    }
    let mut sketch = sketch();
    sketch.add_constraint(Constraint::Coincident(b, b)).unwrap();
    assert_eq!(
        sketch.check(&DESIGN),
        Err(SketchError::Repeated { from, to: b })
    );
}

#[test]
fn check_bounds_counts() {
    let points = |count: u32| Sketch {
        points: (0..count)
            .map(|i| Point {
                id: Id(i),
                number: i + 1,
                at: DVec2::ZERO,
            })
            .collect(),
        next_id: count,
        ..Sketch::default()
    };
    let limit = u32::try_from(MAX_POINTS).unwrap();
    assert_eq!(points(limit).check(&DESIGN), Ok(()));
    assert_eq!(
        points(limit + 1).check(&DESIGN),
        Err(SketchError::TooMany {
            list: List::Points,
            count: MAX_POINTS + 1,
            limit: MAX_POINTS
        })
    );

    // Counted before anything else is looked at.
    let entry = CurveEntry {
        id: Id(0),
        number: 1,
        construction: false,
        curve: Curve::Line {
            start: Id(0),
            end: Id(0),
        },
        corner: None,
    };
    let curves = Sketch {
        curves: vec![entry; MAX_CURVES + 1],
        ..Sketch::default()
    };
    assert!(matches!(
        curves.check(&DESIGN),
        Err(SketchError::TooMany {
            list: List::Curves,
            ..
        })
    ));
    let entry = ConstraintEntry {
        id: Id(0),
        constraint: Constraint::Horizontal(Id(0)),
    };
    let constraints = Sketch {
        constraints: vec![entry; MAX_CONSTRAINTS + 1],
        ..Sketch::default()
    };
    assert!(matches!(
        constraints.check(&DESIGN),
        Err(SketchError::TooMany {
            list: List::Constraints,
            ..
        })
    ));
}

#[test]
fn errors_say_what_is_wrong() {
    let error = SketchError::Reference {
        from: Id(5),
        to: Id(2),
        expected: Role::Point,
    };
    assert_eq!(
        error.to_string(),
        "sketch item 5 refers to 2, which is no point"
    );
    let error = SketchError::TooMany {
        list: List::Curves,
        count: 10_001,
        limit: MAX_CURVES,
    };
    assert_eq!(
        error.to_string(),
        "the sketch has 10001 curves, over the limit of 10000"
    );
}

#[test]
fn check_refuses_constraints_on_items_that_dont_fit() {
    let mut sketch = sketch();
    let (a, b, line, circle) = (Id(0), Id(1), Id(2), Id(3));
    let c = sketch.add_point(DVec2::new(3.0, 1.0)).unwrap();
    let d = sketch.add_point(DVec2::new(5.0, 2.0)).unwrap();
    let other = sketch
        .add_curve(Curve::Line { start: c, end: d }, false)
        .unwrap();
    let arc = sketch
        .add_curve(
            Curve::Arc {
                center: c,
                start: d,
                end: b,
            },
            false,
        )
        .unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    let side = Side::Positive;
    let with = |constraint: Constraint| {
        let mut sketch = sketch.clone();
        let id = sketch.add_constraint(constraint).unwrap();
        (sketch.check(&DESIGN), id)
    };

    // Each fits.
    for constraint in [
        Constraint::PointOnCurve {
            point: c,
            curve: line,
        },
        Constraint::PointOnCurve {
            point: a,
            curve: arc,
        },
        Constraint::Parallel(line, other),
        Constraint::Perpendicular(line, other),
        Constraint::Tangent {
            a: line,
            b: arc,
            side,
            at: None,
        },
        Constraint::Tangent {
            a: circle,
            b: arc,
            side,
            at: None,
        },
        Constraint::Equal(line, other),
        Constraint::Equal(arc, circle),
        Constraint::Concentric(circle, arc),
        Constraint::Concentric(b, circle),
        Constraint::Midpoint { point: c, line },
        Constraint::Symmetric {
            a: c,
            b: d,
            about: line,
        },
        Constraint::Fix(a),
        Constraint::Fix(arc),
    ] {
        assert_eq!(with(constraint.clone()).0, Ok(()), "{constraint:?}");
    }

    // An item of the wrong kind.
    for (constraint, to, expected) in [
        (
            Constraint::PointOnCurve { point: c, curve: d },
            d,
            Role::AnyCurve,
        ),
        (
            Constraint::Parallel(line, circle),
            circle,
            Role::LineOrHandle,
        ),
        (
            Constraint::Tangent {
                a: c,
                b: arc,
                side,
                at: None,
            },
            c,
            Role::Curve,
        ),
        (
            Constraint::Concentric(line, circle),
            line,
            Role::PointOrRound,
        ),
        (
            Constraint::Midpoint {
                point: c,
                line: arc,
            },
            arc,
            Role::Line,
        ),
        (
            Constraint::Symmetric {
                a: c,
                b: line,
                about: other,
            },
            line,
            Role::Point,
        ),
        (Constraint::Fix(Id(4)), Id(4), Role::Geometry),
    ] {
        let (result, from) = with(constraint.clone());
        assert_eq!(
            result,
            Err(SketchError::Reference { from, to, expected }),
            "{constraint:?}"
        );
    }

    // Items each fine alone, but not together.
    for constraint in [
        Constraint::Tangent {
            a: line,
            b: other,
            side,
            at: None,
        },
        Constraint::Equal(line, circle),
        Constraint::Concentric(a, c),
    ] {
        let (result, from) = with(constraint.clone());
        assert_eq!(result, Err(SketchError::Unfit(from)), "{constraint:?}");
    }

    // A curve with one of its own points: true or false whatever moves.
    for (constraint, curve, point) in [
        (
            Constraint::PointOnCurve {
                point: a,
                curve: line,
            },
            line,
            a,
        ),
        (Constraint::Midpoint { point: b, line }, line, b),
        (Constraint::Concentric(c, arc), arc, c),
        (
            Constraint::Symmetric {
                a: c,
                b: d,
                about: other,
            },
            other,
            c,
        ),
    ] {
        let (result, from) = with(constraint.clone());
        assert_eq!(
            result,
            Err(SketchError::OwnPoint { from, curve, point }),
            "{constraint:?}"
        );
    }
    assert_eq!(
        SketchError::Unfit(Id(9)).to_string(),
        "sketch item 9 ties together items that don't go together"
    );
    let error = SketchError::Reference {
        from: Id(9),
        to: Id(2),
        expected: Role::Round,
    };
    assert_eq!(
        error.to_string(),
        "sketch item 9 refers to 2, which is no circle or arc"
    );
}
