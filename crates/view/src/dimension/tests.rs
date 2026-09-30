use super::*;
use crate::testing::{at, dimension, handled_spline, line, point};

/// A line from (0, 0) to (10, 5), a point at (20, 20), a line from
/// (0, 10) to (10, 15) parallel to the first, a line from (0, 0) up at
/// 60°, a circle and an arc.
fn shapes() -> (Sketch, [Id; 6], [Id; 2]) {
    let mut sketch = Sketch::default();
    let [a, b] = [(0.0, 0.0), (10.0, 5.0)].map(|(x, y)| point(&mut sketch, x, y));
    let slope = line(&mut sketch, a, b);
    let lone = point(&mut sketch, 20.0, 20.0);
    let [c, d] = [(0.0, 10.0), (10.0, 15.0)].map(|(x, y)| point(&mut sketch, x, y));
    let parallel = line(&mut sketch, c, d);
    let up = varde_sketch::angle::from_angle(60f64.to_radians()) * 10.0;
    let e = point(&mut sketch, up.x, up.y);
    let steep = line(&mut sketch, a, e);
    let center = point(&mut sketch, 40.0, 0.0);
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 3.0,
            },
            false,
        )
        .unwrap();
    let [s, t] = [(44.0, 0.0), (40.0, 4.0)].map(|(x, y)| point(&mut sketch, x, y));
    let arc = sketch
        .add_curve(
            Curve::Arc {
                center,
                start: s,
                end: t,
            },
            false,
        )
        .unwrap();
    (sketch, [slope, lone, parallel, steep, circle, arc], [a, b])
}

fn measured(sketch: &Sketch, picked: &[Id], x: f64, y: f64) -> Option<Measure> {
    measure(sketch, picked, at(x, y), false).map(|(measure, _)| measure)
}

#[test]
fn a_line_measures_its_length_or_beside_its_ends_its_extent() {
    let (sketch, [slope, ..], [a, b]) = shapes();
    // Beside it, straight, below as above.
    assert_eq!(
        measured(&sketch, &[slope], 3.0, 6.0),
        Some(Measure::Length(slope))
    );
    assert_eq!(
        measured(&sketch, &[slope], 8.0, -6.0),
        Some(Measure::Length(slope))
    );
    // Right of both ends, level with them: how far up it goes.
    assert_eq!(
        measured(&sketch, &[slope], 15.0, 2.0),
        Some(Measure::VerticalDistance(a, b))
    );
    // Off its start, below both ends: how far along.
    assert_eq!(
        measured(&sketch, &[slope], 1.0, -8.0),
        Some(Measure::HorizontalDistance(a, b))
    );
    // Past a corner, straight again.
    assert_eq!(
        measured(&sketch, &[slope], 20.0, 30.0),
        Some(Measure::Length(slope))
    );
}

#[test]
fn two_items_measure_what_lies_between_them() {
    let (sketch, [slope, lone, parallel, steep, circle, _], [a, b]) = shapes();
    assert_eq!(
        measured(&sketch, &[a, lone], 5.0, 15.0),
        Some(Measure::Distance(a, lone))
    );
    // Their extent the same way as a line's.
    assert_eq!(
        measured(&sketch, &[a, b], 15.0, 2.0),
        Some(Measure::VerticalDistance(a, b))
    );
    let (point_line, side) = measure(&sketch, &[lone, slope], at(0.0, 0.0), false).unwrap();
    assert_eq!(point_line, Measure::Distance(lone, slope));
    assert_eq!(side, sketch.side(&point_line));
    assert_eq!(
        measured(&sketch, &[slope, lone], 0.0, 0.0),
        Some(Measure::Distance(slope, lone))
    );
    assert_eq!(
        measured(&sketch, &[slope, parallel], 5.0, 8.0),
        Some(Measure::Distance(slope, parallel))
    );
    assert!(matches!(
        measured(&sketch, &[slope, steep], 5.0, 8.0),
        Some(Measure::Angle(..))
    ));
    // What doesn't measure anything together.
    assert_eq!(measured(&sketch, &[a], 5.0, 8.0), None);
    assert_eq!(measured(&sketch, &[a, a], 5.0, 8.0), None);
    assert_eq!(
        measured(&sketch, &[a, slope], 5.0, 8.0),
        None,
        "its own end"
    );
    assert_eq!(measured(&sketch, &[slope, circle], 5.0, 8.0), None);
    assert_eq!(measured(&sketch, &[a, b, lone], 5.0, 8.0), None);
    assert!(joins(&sketch, &[a], lone));
    assert!(joins(&sketch, &[slope], steep));
    assert!(!joins(&sketch, &[slope], b));
    assert!(!joins(&sketch, &[circle], a));
    assert!(!joins(&sketch, &[a, lone], b));
}

#[test]
fn an_angle_is_the_one_the_label_is_in_or_across_the_corner_from() {
    let mut sketch = Sketch::default();
    let o = point(&mut sketch, 0.0, 0.0);
    let x = point(&mut sketch, 10.0, 0.0);
    let up = varde_sketch::angle::from_angle(60f64.to_radians()) * 10.0;
    let e = point(&mut sketch, up.x, up.y);
    let (a, b) = (line(&mut sketch, o, x), line(&mut sketch, o, e));
    let degrees = |toward: f64| {
        let at = varde_sketch::angle::from_angle(toward.to_radians()) * 3.0;
        let (measure, side) = measure(&sketch, &[a, b], at, false).unwrap();
        sketch.measure(&measure, side).unwrap().to_degrees()
    };
    // Between the lines, and across the corner from there.
    assert!((degrees(30.0) - 60.0).abs() < 1e-9);
    assert!((degrees(210.0) - 60.0).abs() < 1e-9);
    // Beside them, and across.
    assert!((degrees(120.0) - 120.0).abs() < 1e-9);
    assert!((degrees(300.0) - 120.0).abs() < 1e-9);
}

#[test]
fn a_circle_is_measured_by_its_diameter_an_arc_by_its_radius_or_switched() {
    let (sketch, [.., circle, arc], _) = shapes();
    let round = |id, switched| measure(&sketch, &[id], at(0.0, 0.0), switched).unwrap().0;
    assert_eq!(round(circle, false), Measure::Diameter(circle));
    assert_eq!(round(circle, true), Measure::Radius(circle));
    assert_eq!(round(arc, false), Measure::Radius(arc));
    assert_eq!(round(arc, true), Measure::Diameter(arc));
    assert!(super::round(&sketch, &[circle]) && super::round(&sketch, &[arc]));
    let (_, [slope, ..], [a, _]) = shapes();
    assert!(!super::round(&sketch, &[slope]) && !super::round(&sketch, &[a]));
    assert!(pickable(&sketch, circle) && pickable(&sketch, a));
}

#[test]
fn labels_show_values_in_the_design_s_units_references_in_brackets() {
    let (mut sketch, [slope, _, _, steep, circle, _], _) = shapes();
    let mut dimension = |measure: Measure, text: &str, driving| {
        let id = dimension(&mut sketch, measure, text, driving, DVec2::ZERO);
        sketch.dimension(id).unwrap().dimension.clone()
    };
    let length = dimension(Measure::Length(slope), "1 in + 3", true);
    let radius = dimension(Measure::Radius(circle), "3", true);
    let diameter = dimension(Measure::Diameter(circle), "2", false);
    let angle = dimension(Measure::Angle(slope, steep), "30", true);
    assert_eq!(label(&sketch, &length, LengthUnit::Mm), "28.4 mm");
    assert_eq!(label(&sketch, &length, LengthUnit::In), "1.1181 in");
    assert_eq!(label(&sketch, &radius, LengthUnit::Mm), "R 3 mm");
    // A reference shows what it measures, not the value it had.
    assert_eq!(label(&sketch, &diameter, LengthUnit::Mm), "(Ø 6 mm)");
    assert_eq!(label(&sketch, &angle, LengthUnit::In), "30°");
    assert_eq!(name(&length.measure), "Length");
    assert_eq!(name(&Measure::Offset(slope, steep)), "Offset");
    assert_eq!(
        measured_text(
            &sketch,
            &Measure::Diameter(circle),
            Side::Positive,
            LengthUnit::Mm
        ),
        "6 mm"
    );
}

#[test]
fn a_spline_s_handle_measures_its_angle_alone_and_its_length_with_its_point() {
    let mut sketch = Sketch::default();
    let (spline, fit, tip) = handled_spline(&mut sketch);
    assert_eq!(
        measured(&sketch, &[tip], 16.0, 9.0),
        Some(Measure::Angle(Id::X_AXIS, tip))
    );
    let angle = sketch.measure(&Measure::Angle(Id::X_AXIS, tip), Side::Positive);
    assert!((angle.unwrap() - 45f64.to_radians()).abs() < 1e-9);
    assert_eq!(
        measured(&sketch, &[fit[1], tip], 11.0, 6.0),
        Some(Measure::Distance(fit[1], tip))
    );
    // Its other points measure nothing alone, nor the spline.
    assert_eq!(measured(&sketch, &[fit[0]], 1.0, 1.0), None);
    assert_eq!(measured(&sketch, &[spline], 1.0, 1.0), None);
}
