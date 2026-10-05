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
    let (sketch, [slope, lone, parallel, steep, _, _], [a, b]) = shapes();
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
    assert_eq!(measured(&sketch, &[a, b, lone], 5.0, 8.0), None);
    assert!(joins(&sketch, &[a], lone));
    assert!(joins(&sketch, &[slope], steep));
    assert!(!joins(&sketch, &[slope], b));
    // A circle with the rest measures from its edge: see
    // `a_circle_picked_with_other_geometry_measures_from_its_edge`.
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

#[test]
fn a_sector_holds_its_own_ends_and_not_a_direction_of_no_length() {
    let dirs = [
        at(1.0, 0.0),
        at(0.0, 1.0),
        at(0.3, 2.0),
        at(-1.0, 0.5),
        at(-2.0, -0.1),
    ];
    for from in dirs {
        // Its start, at any sweep.
        assert!(sector_holds(from, 1e-6, from), "{from}");
        assert!(sector_holds(from, 1e-6, from * 3.0), "{from}");
        // Straight across, a half turn to within the rounding of the
        // two directions' angles, and not under it.
        for across in [-from, at(-from.x, -from.y + 0.0), -2.0 * from] {
            assert!(sector_holds(from, PI + 1e-12, across), "{from} {across}");
            assert!(!sector_holds(from, PI - 1e-9, across), "{from} {across}");
        }
        // A quarter turn back is three quarters on.
        assert!(!sector_holds(from, PI, -from.perp()), "{from}");
        assert!(sector_holds(from, 1.5 * PI + 1e-9, -from.perp()), "{from}");
        // No direction at all is in no sector.
        assert!(!sector_holds(from, TAU, DVec2::ZERO), "{from}");
        assert!(!sector_holds(DVec2::ZERO, TAU, from), "{from}");
    }
    // Axis-aligned opposites, with a negative zero too, are a half turn
    // to the bit.
    assert!(sector_holds(at(0.0, 1.0), PI, at(0.0, -1.0)));
    assert!(sector_holds(at(1.0, 0.0), PI, at(-1.0, -0.0)));
    assert!(sector_holds(at(-1.0, 0.0), PI, at(1.0, -0.0)));
}

#[test]
fn an_arc_s_sector_holds_both_its_ends() {
    // Whether a place on an arc's circle is on its run is `sector_holds`
    // of the arc's sweep: its own ends are on it, however it turns. By
    // `acos` of the cosine, the turn to its end can come out up to 10⁻⁸
    // over the sweep made by `atan2`.
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..2000 {
        let (a, b, radius) = (next() * TAU, next() * TAU, 0.1 + next() * 100.0);
        let center = at(next() * 100.0 - 50.0, next() * 100.0 - 50.0);
        let start = center + varde_sketch::angle::from_angle(a) * radius;
        let end = center + varde_sketch::angle::from_angle(b) * radius;
        let (from, to) = (start - center, end - center);
        let sweep = varde_sketch::arc_sweep(from, to);
        assert!(sector_holds(from, sweep, to), "{from} {to} {sweep}");
        assert!(sector_holds(from, sweep, from), "{from} {to} {sweep}");
    }
}

/// What a selection measures, for the status bar: straight lengths and
/// distances wherever the label would go, the first angle under a half
/// turn, a circle's diameter and an arc's radius; nothing for three
/// items or a point alone.
#[test]
fn a_selection_measures_as_its_dimension_would() {
    let (sketch, [slope, lone, parallel, steep, circle, arc], [a, _]) = shapes();
    let shown = |ids: &[Id]| {
        selected(&sketch, ids)
            .map(|(measure, value)| shown_measure(&measure, value, LengthUnit::Mm))
    };
    assert_eq!(shown(&[slope]).as_deref(), Some("Length 11.18 mm"));
    assert_eq!(shown(&[a, lone]).as_deref(), Some("Distance 28.284 mm"));
    assert_eq!(shown(&[circle]).as_deref(), Some("Diameter 6 mm"));
    assert_eq!(shown(&[arc]).as_deref(), Some("Radius 4 mm"));
    let apart = shown(&[slope, parallel]).unwrap();
    assert!(apart.starts_with("Distance "), "{apart}");
    let angle = shown(&[slope, steep]).unwrap();
    assert!(angle.starts_with("Angle "), "{angle}");
    assert_eq!(shown(&[lone]), None);
    assert_eq!(shown(&[slope, parallel, steep]), None);
}

/// Four lines closing `corners` in order, and the corners' points.
fn quad(sketch: &mut Sketch, corners: [(f64, f64); 4]) -> ([Id; 4], [Id; 4]) {
    let points = corners.map(|(x, y)| point(sketch, x, y));
    let lines = [0, 1, 2, 3].map(|i| line(sketch, points[i], points[(i + 1) % 4]));
    (lines, points)
}

/// A rectangle selected measures its width, the side nearer the
/// horizontal, and its height, its corners selected with it or not; a
/// quadrilateral that isn't square, three of its lines, or a point off
/// it with them, nothing.
#[test]
fn a_rectangle_selected_measures_its_width_and_height() {
    let mut sketch = Sketch::default();
    let (lines, points) = quad(
        &mut sketch,
        [(0.0, 0.0), (0.0, 10.0), (20.0, 10.0), (20.0, 0.0)],
    );
    assert_eq!(rectangle(&sketch, &lines), Some((20.0, 10.0)));
    let mut all = lines.to_vec();
    all.extend(points);
    assert_eq!(rectangle(&sketch, &all), Some((20.0, 10.0)));
    assert_eq!(rectangle(&sketch, &lines[..3]), None);
    let lone = point(&mut sketch, 50.0, 50.0);
    all.push(lone);
    assert_eq!(rectangle(&sketch, &all), None);

    // Turned 30°, its lines in any order.
    let along = varde_sketch::angle::from_angle(30f64.to_radians());
    let up = along.perp();
    let corner = |p: glam::DVec2| (p.x, p.y);
    let turned = [
        corner(at(100.0, 0.0)),
        corner(at(100.0, 0.0) + along * 20.0),
        corner(at(100.0, 0.0) + along * 20.0 + up * 10.0),
        corner(at(100.0, 0.0) + up * 10.0),
    ];
    let (mut lines, _) = quad(&mut sketch, turned);
    lines.reverse();
    let (width, height) = rectangle(&sketch, &lines).unwrap();
    assert!((width - 20.0).abs() < 1e-9 && (height - 10.0).abs() < 1e-9);

    let (skewed, _) = quad(
        &mut sketch,
        [(0.0, 50.0), (2.0, 60.0), (22.0, 60.0), (20.0, 50.0)],
    );
    assert_eq!(rectangle(&sketch, &skewed), None);
}

/// A circle or an arc picked with a point, a line or another circle or
/// arc measures from its edge, the one holding the other first; with its
/// own centre, nothing.
#[test]
fn a_circle_picked_with_other_geometry_measures_from_its_edge() {
    let (sketch, [slope, lone, _, _, circle, arc], _) = shapes();
    assert_eq!(
        measured(&sketch, &[circle, lone], 30.0, 10.0),
        Some(Measure::EdgeDistance(circle, lone))
    );
    assert_eq!(
        measured(&sketch, &[lone, circle], 30.0, 10.0),
        Some(Measure::EdgeDistance(circle, lone))
    );
    assert_eq!(
        measured(&sketch, &[slope, circle], 30.0, 0.0),
        Some(Measure::EdgeDistance(circle, slope))
    );
    // The arc, of radius 4 about the same centre, holds the circle.
    assert_eq!(
        measured(&sketch, &[circle, arc], 47.0, 0.0),
        Some(Measure::EdgeDistance(arc, circle))
    );
    assert!(joins(&sketch, &[lone], circle));
    let center = match sketch.curve(circle).unwrap().curve {
        Curve::Circle { center, .. } => center,
        _ => unreachable!(),
    };
    assert_eq!(measured(&sketch, &[circle, center], 30.0, 10.0), None);
    assert!(!joins(&sketch, &[circle], center));
}
