use super::*;
use crate::origin::LAST_ID;
use crate::testing::DESIGN;

/// A line from the origin to (10, 0), a circle and an arc, as the tools
/// would draw them.
fn drawn() -> (Sketch, [Id; 3]) {
    let mut sketch = Sketch::default();
    let start = sketch.add_point(DVec2::ZERO).unwrap();
    let end = sketch.add_point(DVec2::new(10.0, 0.0)).unwrap();
    let line = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    let center = sketch.add_point(DVec2::new(0.0, 20.0)).unwrap();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 5.0,
            },
            true,
        )
        .unwrap();
    let arc_start = sketch.add_point(DVec2::new(5.0, 20.0)).unwrap();
    let arc = sketch
        .add_curve(
            Curve::Arc {
                center,
                start: arc_start,
                end: start,
            },
            false,
        )
        .unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    (sketch, [line, circle, arc])
}

#[test]
fn items_get_increasing_ids_and_are_found_by_them() {
    let (sketch, [line, circle, arc]) = drawn();
    assert_eq!(sketch.next_id, 7);
    let ids: Vec<_> = sketch.points.iter().map(|point| point.id).collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(line < circle && circle < arc);

    assert_eq!(sketch.kind(line), Some(Kind::Line));
    assert_eq!(sketch.kind(circle), Some(Kind::Circle));
    assert_eq!(sketch.kind(arc), Some(Kind::Arc));
    assert_eq!(sketch.kind(ids[0]), Some(Kind::Point));
    assert_eq!(sketch.kind(Id(7)), None);
    assert_eq!(
        sketch.point(ids[1]).map(|point| point.at),
        Some(DVec2::new(10.0, 0.0))
    );
    assert!(sketch.point(line).is_none());
    assert!(sketch.curve(ids[0]).is_none());
    assert!(sketch.curve(circle).is_some_and(|entry| entry.construction));

    let mut sketch = sketch;
    sketch.point_mut(ids[0]).unwrap().at = DVec2::ONE;
    assert_eq!(sketch.points[0].at, DVec2::ONE);
    sketch.curve_mut(line).unwrap().construction = true;
    assert!(sketch.curves[0].construction);
    let horizontal = sketch.add_constraint(Constraint::Horizontal(line)).unwrap();
    assert_eq!(sketch.kind(horizontal), Some(Kind::Constraint));
    assert_eq!(
        sketch.constraint(horizontal).map(|entry| &entry.constraint),
        Some(&Constraint::Horizontal(line))
    );
    assert_eq!(sketch.check(&DESIGN), Ok(()));
}

#[test]
fn items_are_numbered_by_kind_for_their_names() {
    let (mut sketch, [line, circle, arc]) = drawn();
    let names: Vec<_> = sketch.points.iter().map(Point::name).collect();
    assert_eq!(names, ["Point 1", "Point 2", "Point 3", "Point 4"]);
    let names: Vec<_> = sketch.curves.iter().map(CurveEntry::name).collect();
    assert_eq!(names, ["Line 1", "Circle 1", "Arc 1"]);

    // One past the highest of its kind, so deleting one never renames or
    // reuses a later one's name.
    let (start, end) = (sketch.points[0].id, sketch.points[1].id);
    let second = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    assert_eq!(sketch.curve(second).unwrap().name(), "Line 2");
    sketch.delete(&[line]);
    let third = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    assert_eq!(sketch.curve(third).unwrap().name(), "Line 3");
    sketch.delete(&[circle, arc]);
    let (center, radius) = (sketch.points[0].id, 1.0);
    let again = sketch
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap();
    assert_eq!(sketch.curve(again).unwrap().name(), "Circle 1");

    // A file could hold the highest number; it's reused rather than
    // overflowing.
    sketch.curve_mut(again).unwrap().number = u32::MAX;
    let last = sketch
        .add_curve(Curve::Circle { center, radius }, false)
        .unwrap();
    assert_eq!(sketch.curve(last).unwrap().number, u32::MAX);
}

#[test]
fn adding_past_the_last_id_fails() {
    // `next_id` comes from the file, so it can be anything.
    let mut sketch = Sketch {
        next_id: LAST_ID - 1,
        ..Sketch::default()
    };
    let point = sketch.add_point(DVec2::ZERO).unwrap();
    assert_eq!(point, Id(LAST_ID - 1));
    let before = sketch.clone();
    assert_eq!(sketch.add_point(DVec2::ONE), Err(OutOfIds));
    let circle = Curve::Circle {
        center: point,
        radius: 1.0,
    };
    assert_eq!(sketch.add_curve(circle, false), Err(OutOfIds));
    assert_eq!(
        sketch.add_constraint(Constraint::Coincident(point, point)),
        Err(OutOfIds)
    );
    assert_eq!(sketch, before);
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    assert_eq!(OutOfIds.to_string(), "the sketch has no ids left");
}

#[test]
fn deleting_a_point_deletes_the_curves_and_constraints_on_it() {
    let (mut sketch, [line, circle, arc]) = drawn();
    let [start, end, center, arc_start] = [0, 1, 2, 3].map(|i| sketch.points[i].id);
    let coincident = sketch
        .add_constraint(Constraint::Coincident(end, arc_start))
        .unwrap();
    let horizontal = sketch.add_constraint(Constraint::Horizontal(line)).unwrap();

    sketch.delete(&[center]);
    // The circle and the arc went with their centre, and the arc's start
    // with the arc, and the constraint on it; the line's start stays, the
    // line is made from it.
    assert_eq!(sketch.curves.len(), 1);
    assert!(sketch.curve(line).is_some());
    assert!(sketch.curve(circle).is_none() && sketch.curve(arc).is_none());
    let points: Vec<_> = sketch.points.iter().map(|point| point.id).collect();
    assert_eq!(points, [start, end]);
    assert!(sketch.constraint(coincident).is_none());
    assert!(sketch.constraint(horizontal).is_some());
    assert_eq!(sketch.check(&DESIGN), Ok(()));
}

#[test]
fn deleting_a_curve_deletes_the_points_only_it_used() {
    let (mut sketch, [line, _, arc]) = drawn();
    let [start, end, center, arc_start] = [0, 1, 2, 3].map(|i| sketch.points[i].id);
    let lone = sketch.add_point(DVec2::splat(-3.0)).unwrap();
    let horizontal = sketch.add_constraint(Constraint::Horizontal(line)).unwrap();
    let vertical = sketch
        .add_constraint(Constraint::VerticalPoints(end, lone))
        .unwrap();

    sketch.delete(&[line]);
    // The arc still ends at `start`; `end` was the line's alone.
    let points: Vec<_> = sketch.points.iter().map(|point| point.id).collect();
    assert_eq!(points, [start, center, arc_start, lone]);
    assert!(sketch.constraint(horizontal).is_none());
    assert!(sketch.constraint(vertical).is_none());
    assert_eq!(sketch.check(&DESIGN), Ok(()));

    // A constraint alone goes alone; ids naming nothing are ignored.
    let coincident = sketch
        .add_constraint(Constraint::Coincident(center, lone))
        .unwrap();
    let before = sketch.points.clone();
    sketch.delete(&[coincident, Id(1000), line]);
    assert!(sketch.constraints.is_empty());
    assert_eq!(sketch.points, before);
    assert!(sketch.curve(arc).is_some());

    // A lone point goes when asked.
    sketch.delete(&[lone]);
    assert!(sketch.point(lone).is_none());
    assert_eq!(sketch.check(&DESIGN), Ok(()));
}

#[test]
fn a_sketch_round_trips_through_postcard() {
    let (mut sketch, [line, ..]) = drawn();
    sketch.add_constraint(Constraint::Vertical(line)).unwrap();
    let bytes = postcard::to_stdvec(&sketch).unwrap();
    assert_eq!(postcard::from_bytes::<Sketch>(&bytes).unwrap(), sketch);
}

#[test]
fn deleting_a_curve_deletes_every_kind_of_constraint_on_it() {
    let (mut sketch, [line, circle, arc]) = drawn();
    let lone = sketch.add_point(DVec2::new(3.0, 3.0)).unwrap();
    let side = Side::Positive;
    let on_line = [
        Constraint::Tangent {
            a: line,
            b: arc,
            side,
            at: None,
        },
        Constraint::Midpoint { point: lone, line },
        Constraint::Fix(line),
    ]
    .map(|constraint| sketch.add_constraint(constraint).unwrap());
    let on_circle = [
        Constraint::Equal(circle, arc),
        Constraint::Concentric(lone, circle),
        Constraint::PointOnCurve {
            point: lone,
            curve: circle,
        },
    ]
    .map(|constraint| sketch.add_constraint(constraint).unwrap());
    assert_eq!(sketch.check(&DESIGN), Ok(()));

    sketch.delete(&[line]);
    assert!(on_line.iter().all(|&id| sketch.constraint(id).is_none()));
    assert!(on_circle.iter().all(|&id| sketch.constraint(id).is_some()));
    sketch.delete(&[circle]);
    assert!(sketch.constraints.is_empty());
    assert!(sketch.point(lone).is_some());
    assert_eq!(sketch.check(&DESIGN), Ok(()));
}

#[test]
fn points_are_named_by_their_role_in_a_curve() {
    let (mut sketch, _) = drawn();
    let names: Vec<_> = (sketch.points.iter())
        .map(|point| sketch.point_name(point))
        .collect();
    // The line's start is the arc's end; the line's own end has no role.
    assert_eq!(
        names,
        [
            "End of Arc 1",
            "Point 2",
            "Centre of Circle 1",
            "Start of Arc 1"
        ]
    );

    let a = sketch.add_point(DVec2::ZERO).unwrap();
    let b = sketch.add_point(DVec2::X).unwrap();
    let mut spline = Spline::through(vec![a, b], false);
    sketch
        .add_curve(Curve::Spline(spline.clone()), false)
        .unwrap();
    assert_eq!(sketch.name(b).unwrap(), "Point 6");
    spline.kind = SplineKind::Control;
    sketch.curves.last_mut().unwrap().curve = Curve::Spline(spline);
    assert_eq!(sketch.name(b).unwrap(), "Control point 2 of Spline 1");
}
