use glam::DVec2;
use varde_expr::LengthUnit;

use super::*;
use crate::testing::{DESIGN, at, circle, dimension, line, point, value};
use crate::{Curve, Kind, List, MAX_DIMENSIONS, SketchError};

/// A line from (0, 0) to (10, 0), a point at (3, 4) above it, a line from
/// (0, 5) to (10, 5) and a circle of radius 2 around (20, 0): the two
/// ends, the point, the lines and the circle.
fn drawn() -> (Sketch, [Id; 2], Id, [Id; 2], Id) {
    let mut sketch = Sketch::default();
    let a = point(&mut sketch, 0.0, 0.0);
    let b = point(&mut sketch, 10.0, 0.0);
    let p = point(&mut sketch, 3.0, 4.0);
    let bottom = line(&mut sketch, a, b);
    let c = point(&mut sketch, 0.0, 5.0);
    let d = point(&mut sketch, 10.0, 5.0);
    let top = line(&mut sketch, c, d);
    let center = point(&mut sketch, 20.0, 0.0);
    let round = circle(&mut sketch, center, 2.0);
    (sketch, [a, b], p, [bottom, top], round)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn each_measure_measures_its_geometry_on_its_side() {
    let (sketch, [a, _], p, [bottom, top], round) = drawn();
    use Side::{Negative, Positive};
    let cases = [
        (Measure::Distance(a, p), Positive, 5.0),
        (Measure::Distance(p, bottom), Positive, 4.0),
        (Measure::Distance(bottom, p), Negative, -4.0),
        (Measure::Distance(top, bottom), Positive, 5.0),
        (Measure::HorizontalDistance(a, p), Positive, 3.0),
        (Measure::HorizontalDistance(p, a), Negative, 3.0),
        (Measure::VerticalDistance(p, a), Positive, -4.0),
        (Measure::Length(bottom), Negative, 10.0),
        (Measure::Radius(round), Positive, 2.0),
        (Measure::Diameter(round), Positive, 4.0),
    ];
    for (measure, side, expected) in cases {
        let measured = sketch.measure(&measure, side).unwrap();
        assert!(close(measured, expected), "{measure:?}: {measured}");
    }
    assert_eq!(sketch.measure(&Measure::Length(a), Positive), None);
    assert_eq!(sketch.measure(&Measure::Radius(bottom), Positive), None);
    assert_eq!(sketch.measure(&Measure::Distance(round, p), Positive), None);

    // The side the geometry is on.
    assert_eq!(sketch.side(&Measure::Distance(p, bottom)), Positive);
    assert_eq!(sketch.side(&Measure::Distance(bottom, top)), Negative);
    assert_eq!(sketch.side(&Measure::HorizontalDistance(p, a)), Negative);
    assert_eq!(sketch.side(&Measure::VerticalDistance(a, p)), Positive);
    for measure in [
        Measure::Distance(p, bottom),
        Measure::Distance(bottom, top),
        Measure::HorizontalDistance(p, a),
        Measure::VerticalDistance(p, a),
    ] {
        let side = sketch.side(&measure);
        assert!(sketch.measure(&measure, side).unwrap() > 0.0, "{measure:?}");
    }

    // Anchors.
    let anchor = |measure| sketch.anchor(&measure).unwrap();
    assert_eq!(anchor(Measure::Distance(a, p)), DVec2::new(1.5, 2.0));
    assert_eq!(anchor(Measure::Distance(p, bottom)), DVec2::new(3.0, 2.0));
    assert_eq!(anchor(Measure::Distance(top, bottom)), DVec2::new(5.0, 2.5));
    assert_eq!(anchor(Measure::Length(bottom)), DVec2::new(5.0, 0.0));
    assert_eq!(anchor(Measure::Radius(round)), DVec2::new(20.0, 0.0));
    assert_eq!(sketch.anchor(&Measure::Length(p)), None);
}

#[test]
fn an_angle_is_counter_clockwise_from_the_first_line_reversed_on_the_negative_side() {
    let mut sketch = Sketch::default();
    let o = point(&mut sketch, 0.0, 0.0);
    let x = point(&mut sketch, 10.0, 0.0);
    let e = point(&mut sketch, 0.0, 10.0);
    let f = point(&mut sketch, 10.0, 10.0);
    let along = line(&mut sketch, o, x);
    let up = line(&mut sketch, o, e);
    let diagonal = line(&mut sketch, o, f);
    let angle = |a, b, side| sketch.measure(&Measure::Angle(a, b), side).unwrap();
    let degrees = |degrees: f64| degrees.to_radians();
    assert!(close(angle(along, up, Side::Positive), degrees(90.0)));
    assert!(close(angle(up, along, Side::Positive), degrees(270.0)));
    assert!(close(angle(up, along, Side::Negative), degrees(90.0)));
    assert!(close(angle(along, diagonal, Side::Positive), degrees(45.0)));
    assert!(close(
        angle(along, diagonal, Side::Negative),
        degrees(225.0)
    ));
    assert!(close(
        angle(diagonal, along, Side::Negative),
        degrees(135.0)
    ));
    // The side under half a turn.
    assert_eq!(sketch.side(&Measure::Angle(up, along)), Side::Negative);
    assert_eq!(sketch.side(&Measure::Angle(along, up)), Side::Positive);
    // Where they meet.
    assert_eq!(
        sketch.anchor(&Measure::Angle(along, diagonal)),
        Some(DVec2::ZERO)
    );
}

#[test]
fn check_takes_dimensions_of_what_they_measure() {
    let (mut sketch, [a, b], p, [bottom, top], round) = drawn();
    for measure in [
        Measure::Distance(a, p),
        Measure::Distance(p, bottom),
        Measure::Distance(top, bottom),
        Measure::HorizontalDistance(a, b),
        Measure::VerticalDistance(a, p),
        Measure::Length(bottom),
        Measure::Angle(bottom, top),
        Measure::Radius(round),
        Measure::Diameter(round),
    ] {
        let text = match measure {
            Measure::Angle(..) => "180",
            _ => "1",
        };
        dimension(&mut sketch, measure, text);
    }
    assert_eq!(sketch.kind(sketch.dimensions[0].id), Some(Kind::Dimension));
}

#[test]
fn check_refuses_dimensions_of_the_wrong_kinds() {
    let (sketch, [a, b], p, [bottom, top], round) = drawn();
    let with = |measure: Measure| {
        let mut sketch = sketch.clone();
        let value = Value {
            text: "1".to_owned(),
            value: 1.0,
        };
        let id = sketch
            .add_dimension(Dimension {
                measure,
                value,
                driving: true,
                label: DVec2::ZERO,
                side: Side::Positive,
            })
            .unwrap();
        (sketch.check(&DESIGN), id)
    };
    let reference = |measure, to, expected| {
        let (result, from) = with(measure);
        assert_eq!(result, Err(SketchError::Reference { from, to, expected }));
    };
    reference(Measure::Distance(a, round), round, Role::PointOrLine);
    reference(Measure::HorizontalDistance(a, bottom), bottom, Role::Point);
    reference(Measure::VerticalDistance(round, p), round, Role::Point);
    reference(Measure::Length(a), a, Role::Line);
    reference(Measure::Angle(bottom, round), round, Role::LineOrHandle);
    // A point, but no handle's tip.
    let (result, from) = with(Measure::Angle(bottom, p));
    assert_eq!(result, Err(SketchError::Unfit(from)));
    reference(Measure::Radius(bottom), bottom, Role::Round);
    reference(Measure::Diameter(Id(99)), Id(99), Role::Round);

    let (result, from) = with(Measure::Distance(top, top));
    assert_eq!(result, Err(SketchError::Repeated { from, to: top }));
    // A line's own end is on it whatever moves.
    let (result, from) = with(Measure::Distance(b, bottom));
    assert_eq!(
        result,
        Err(SketchError::OwnPoint {
            from,
            curve: bottom,
            point: b
        })
    );
}

#[test]
fn check_refuses_values_their_expressions_do_not_give() {
    let (mut sketch, [a, _], p, [bottom, _], round) = drawn();
    let id = dimension(&mut sketch, Measure::Distance(a, p), "2 + 3");
    let length = dimension(&mut sketch, Measure::Length(bottom), "10");
    let set = |sketch: &Sketch, id, text: &str, value| {
        let mut sketch = sketch.clone();
        sketch.dimension_mut(id).unwrap().dimension.value = Value {
            text: text.to_owned(),
            value,
        };
        sketch.check(&DESIGN)
    };
    assert_eq!(set(&sketch, id, "2 + 3", 5.0), Ok(()));
    for (text, value) in [
        ("2 + 3", 5.000001),
        ("2 + 3", f64::NAN),
        ("2 +", 5.0),
        // Not a length, not above zero, past the limit.
        ("5 deg", 5.0_f64.to_radians()),
        ("5 mm * 1 mm", 5.0),
        ("0", 0.0),
        ("-5", -5.0),
        ("2e6", 2e6),
        (&"1".repeat(300), 1.0),
    ] {
        assert_eq!(
            set(&sketch, id, text, value),
            Err(SketchError::Value(id)),
            "{text}"
        );
    }

    // Bare numbers are in the design's units.
    let inches = Design {
        units: LengthUnit::In,
        ..DESIGN
    };
    assert_eq!(sketch.check(&inches), Err(SketchError::Value(id)));
    assert_eq!(set(&sketch, length, "10 mm", 10.0).map(|_| ()), Ok(()));
    let mut pinned = sketch.clone();
    for (id, text) in [(id, "(2 + 3) mm"), (length, "10 mm")] {
        pinned.dimension_mut(id).unwrap().dimension.value.text = text.to_owned();
    }
    assert_eq!(pinned.check(&inches), Ok(()));

    // A diameter within twice the limit; an angle within a turn.
    let mut diameters = sketch.clone();
    let diameter = diameters
        .add_dimension(Dimension {
            measure: Measure::Diameter(round),
            value: value("1500000", &Measure::Diameter(round)),
            driving: false,
            label: DVec2::ZERO,
            side: Side::Positive,
        })
        .unwrap();
    assert_eq!(diameters.check(&DESIGN), Ok(()));
    assert_eq!(
        set(&diameters, diameter, "2500000", 2_500_000.0),
        Err(SketchError::Value(diameter))
    );
    let angle = Measure::Angle(bottom, bottom).ask(&DESIGN);
    assert!(Value::new("361", &angle).is_err());
    assert!(Value::new("359.9", &angle).is_ok());
}

#[test]
fn check_refuses_labels_out_of_bounds() {
    let (mut sketch, [a, _], p, _, _) = drawn();
    let id = dimension(&mut sketch, Measure::Distance(a, p), "5");
    for (label, fits) in [
        (DVec2::new(DESIGN.max, -DESIGN.max), true),
        (DVec2::new(DESIGN.max * 1.5, 0.0), false),
        (DVec2::new(0.0, f64::NAN), false),
        (DVec2::new(f64::NEG_INFINITY, 0.0), false),
    ] {
        sketch.dimension_mut(id).unwrap().dimension.label = label;
        let expected = if fits {
            Ok(())
        } else {
            Err(SketchError::Label(id))
        };
        assert_eq!(sketch.check(&DESIGN), expected, "{label}");
    }
}

#[test]
fn check_bounds_dimensions_and_their_ids() {
    let (mut sketch, [a, _], p, _, _) = drawn();
    let id = dimension(&mut sketch, Measure::Distance(a, p), "5");
    let entry = sketch.dimension(id).unwrap().clone();

    let mut shared = sketch.clone();
    shared.dimensions[0].id = a;
    assert_eq!(shared.check(&DESIGN), Err(SketchError::Shared(a)));

    let mut past = sketch.clone();
    past.next_id = id.0;
    assert_eq!(past.check(&DESIGN), Err(SketchError::NextId(id)));

    let mut many = sketch.clone();
    many.next_id = u32::MAX;
    many.dimensions = (0..=MAX_DIMENSIONS)
        .map(|n| DimensionEntry {
            id: Id(id.0 + n as u32),
            ..entry.clone()
        })
        .collect();
    assert_eq!(
        many.check(&DESIGN),
        Err(SketchError::TooMany {
            list: List::Dimensions,
            count: MAX_DIMENSIONS + 1,
            limit: MAX_DIMENSIONS
        })
    );
}

#[test]
fn deleting_geometry_deletes_its_dimensions() {
    let (mut sketch, [a, b], p, [bottom, top], round) = drawn();
    let to_p = dimension(&mut sketch, Measure::Distance(a, p), "5");
    let length = dimension(&mut sketch, Measure::Length(bottom), "10");
    let between = dimension(&mut sketch, Measure::Distance(top, bottom), "5");
    let radius = dimension(&mut sketch, Measure::Radius(round), "2");
    let kept: Vec<_> = sketch.dimensions.iter().map(|entry| entry.id).collect();
    assert_eq!(kept, [to_p, length, between, radius]);

    // The line's end takes the line, and with it its other end: each
    // takes its dimensions.
    let mut deleted = sketch.clone();
    deleted.delete(&[b]);
    let kept: Vec<_> = deleted.dimensions.iter().map(|entry| entry.id).collect();
    assert_eq!(kept, [radius]);
    assert_eq!(deleted.check(&DESIGN), Ok(()));

    // A dimension deleted alone.
    let mut deleted = sketch.clone();
    deleted.delete(&[length, p]);
    let kept: Vec<_> = deleted.dimensions.iter().map(|entry| entry.id).collect();
    assert_eq!(kept, [between, radius]);
    assert!(deleted.line(bottom).is_some());
}

#[test]
fn pinning_units_keeps_every_value_and_its_meaning() {
    let (mut sketch, [a, _], p, [bottom, top], round) = drawn();
    let sum = dimension(&mut sketch, Measure::Distance(a, p), "1 + 4");
    let mixed = dimension(&mut sketch, Measure::Length(bottom), "0.1 in + 7.46");
    let angle = dimension(&mut sketch, Measure::Angle(bottom, top), "90 * 2");
    let radius = dimension(&mut sketch, Measure::Radius(round), "2 mm");
    // Pinned, this one is past the longest an expression may be.
    let long = format!("{}1", "0 + ".repeat(63));
    assert!(long.len() <= varde_expr::MAX_LEN);
    let diameter = dimension(&mut sketch, Measure::Diameter(round), &long);
    let before = sketch.clone();

    sketch.pin_units(&DESIGN);
    let text = |id| sketch.dimension(id).unwrap().dimension.value.text.clone();
    assert_eq!(text(sum), "(1 + 4) mm");
    assert_eq!(text(mixed), "0.1 in + 7.46 mm");
    assert_eq!(text(angle), "90 * 2");
    assert_eq!(text(radius), "2 mm");
    assert_eq!(text(diameter), "1e0 mm");
    for (entry, was) in sketch.dimensions.iter().zip(&before.dimensions) {
        assert_eq!(entry.dimension.value.value, was.dimension.value.value);
    }
    // The same in any units.
    for units in LengthUnit::ALL {
        assert_eq!(
            sketch.check(&Design { units, ..DESIGN }),
            Ok(()),
            "{units:?}"
        );
    }
}

#[test]
fn exact_values_evaluate_exactly() {
    for value in [
        1.0,
        0.1,
        1.0 / 3.0,
        12.345_678_901_234_567,
        1e-300,
        999_999.999_999_999,
    ] {
        let length = varde_expr::Ask::length(LengthUnit::In, 1e6);
        assert_eq!(
            varde_expr::evaluate(&exact(value, Quantity::Length), &length),
            Ok(value)
        );
        let angle = varde_expr::Ask::angle(LengthUnit::Ft, 1e6);
        assert_eq!(
            varde_expr::evaluate(&exact(value, Quantity::Angle), &angle),
            Ok(value)
        );
    }
}

#[test]
fn an_arc_s_radius_is_its_start_s_distance() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 0.0, 0.0);
    let start = point(&mut sketch, 3.0, 4.0);
    let end = point(&mut sketch, -5.0, 0.0);
    let arc = sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    let measure = Measure::Diameter(arc);
    assert_eq!(sketch.measure(&measure, Side::Positive), Some(10.0));
    let id = dimension(&mut sketch, measure.clone(), "10");
    assert_eq!(
        sketch.dimension(id).unwrap().dimension.value,
        value("10", &measure)
    );
    assert_eq!(at(&sketch, start), DVec2::new(3.0, 4.0));
}

#[test]
fn lengths_are_at_least_a_micrometre_and_angles_short_of_a_turn() {
    let (mut sketch, [a, b], _, [bottom, top], _) = drawn();
    let length = Measure::Length(bottom).ask(&DESIGN);
    assert_eq!(MIN_LENGTH, 1e-3);
    assert!(varde_expr::evaluate("0.001", &length).is_ok());
    assert!(varde_expr::evaluate("0.0009", &length).is_err());
    let angle = Measure::Angle(bottom, top).ask(&DESIGN);
    assert!(varde_expr::evaluate("359.99", &angle).is_ok());
    assert!(varde_expr::evaluate("360", &angle).is_err());
    assert!(varde_expr::evaluate("0", &angle).is_err());
    // A file's stored values are held to the same.
    let tiny = Value {
        text: "0.0005".into(),
        value: 0.0005,
    };
    let id = sketch
        .add_dimension(Dimension {
            measure: Measure::Distance(a, b),
            value: tiny,
            driving: false,
            label: DVec2::ZERO,
            side: Side::Positive,
        })
        .unwrap();
    assert_eq!(sketch.check(&DESIGN), Err(SketchError::Value(id)));
}

#[test]
fn a_handle_s_angle_is_measured_and_goes_with_the_handle() {
    use crate::{Handle, Spline};
    let mut sketch = Sketch::default();
    let fit = [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let tip = point(&mut sketch, 10.0 + 3.0, 5.0 + 3.0);
    let mut through = Spline::through(fit.to_vec(), false);
    through.handles.push(Handle { at: fit[1], tip });
    sketch.add_curve(Curve::Spline(through), false).unwrap();
    // The tip is a line's end too, which keeps it.
    let other = point(&mut sketch, 30.0, 8.0);
    line(&mut sketch, tip, other);
    let angle = Measure::Angle(Id::X_AXIS, tip);
    assert_eq!(
        sketch.direction(tip),
        Some((at(&sketch, fit[1]), at(&sketch, tip)))
    );
    let measured = sketch.measure(&angle, Side::Positive).unwrap();
    assert!((measured - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    assert_eq!(sketch.anchor(&angle), Some(DVec2::new(5.0, 0.0)));
    let id = dimension(&mut sketch, angle, "45 deg");
    sketch.delete(&[fit[1]]);
    assert!(sketch.point(tip).is_some());
    assert!(sketch.dimension(id).is_none());
    assert_eq!(sketch.check(&DESIGN), Ok(()));
}
