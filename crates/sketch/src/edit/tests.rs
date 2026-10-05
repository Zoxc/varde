use glam::DVec2;
use varde_expr::LengthUnit;

use super::*;
use crate::origin::LAST_ID;
use crate::testing::{DESIGN, point, value};
use crate::{Kind, List, Measure};

const MAX: f64 = DESIGN.max;

/// A line from the origin to (10, 0) and a lone point above it.
fn drawn() -> (Sketch, [Id; 3], Id) {
    let mut sketch = Sketch::default();
    let (start, end) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 10.0, 0.0));
    let line = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    let lone = point(&mut sketch, 5.0, 5.0);
    (sketch, [start, end, lone], line)
}

/// A triangle on the line's end and a new point, horizontal and with its
/// new corner on the line: new items naming new and old ones.
fn triangle(sketch: &Sketch, [start, end, _]: [Id; 3], line: Id) -> (Add, [Id; 3]) {
    let mut add = Add::new(sketch);
    let apex = add.point(DVec2::new(5.0, 8.0)).unwrap();
    let corner = add.point(DVec2::new(3.0, 0.0)).unwrap();
    let base = add
        .curve(Curve::Line { start: corner, end }, false)
        .unwrap();
    for (a, b) in [(end, apex), (apex, corner)] {
        add.curve(Curve::Line { start: a, end: b }, true).unwrap();
    }
    add.constraints.push(Constraint::Horizontal(base));
    add.constraints.push(Constraint::PointOnCurve {
        point: corner,
        curve: line,
    });
    add.auto.push(Constraint::Coincident(start, corner));
    (add, [apex, corner, base])
}

#[test]
fn adding_gives_placeholders_ids_after_the_sketchs() {
    let (sketch, points, line) = drawn();
    let (add, [apex, corner, base]) = triangle(&sketch, points, line);
    assert_eq!(apex.0, sketch.next_id);
    let edit = SketchEdit::Add(add.clone());
    let (added, auto) = edit.apply_marked(&sketch, &DESIGN).unwrap();
    assert_eq!(added.points.len(), 5);
    assert_eq!(added.curves.len(), 4);
    assert_eq!(added.constraints.len(), 3);
    // Made on this sketch, the placeholders are the ids.
    assert_eq!(added.kind(apex), Some(Kind::Point));
    assert_eq!(added.kind(base), Some(Kind::Line));
    assert_eq!(
        added.curve(base).unwrap().curve,
        Curve::Line {
            start: corner,
            end: points[1]
        }
    );
    assert_eq!(auto, [added.constraints[2].id]);
    // Numbered after what's there, construction as asked.
    assert_eq!(added.point(apex).unwrap().name(), "Point 4");
    let names: Vec<_> = added.curves.iter().map(|entry| entry.name()).collect();
    assert_eq!(names, ["Line 1", "Line 2", "Line 3", "Line 4"]);
    assert!(added.curves[2].construction && !added.curves[1].construction);

    // Applied to the sketch after another edit added to it, everything
    // new moves along by as many ids, and old ids stay.
    let mut later = sketch.clone();
    let other = point(&mut later, -5.0, 0.0);
    let moved = edit.apply(&later, &DESIGN).unwrap();
    let resolve = |id| add.resolve(later.next_id, id).unwrap();
    assert_eq!(resolve(line), line);
    assert_eq!(resolve(apex).0, apex.0 + 1);
    assert!(moved.point(other).is_some());
    assert_eq!(
        moved.curve(resolve(base)).unwrap().curve,
        Curve::Line {
            start: resolve(corner),
            end: points[1]
        }
    );
    assert_eq!(
        moved.constraints[0].constraint,
        Constraint::Horizontal(resolve(base))
    );
    assert_eq!(moved.check(&DESIGN), Ok(()));
}

#[test]
fn placeholders_must_name_the_edits_own_items() {
    let (sketch, points, line) = drawn();
    let (add, _) = triangle(&sketch, points, line);
    let fails = |add: Add| SketchEdit::Add(add).apply(&sketch, &DESIGN);

    // Naming a new item the edit doesn't have.
    let mut missing = add.clone();
    missing
        .constraints
        .push(Constraint::Horizontal(Id(add.first + 40)));
    assert_eq!(
        fails(missing),
        Err(EditError::Placeholder(Id(add.first + 40)))
    );
    // Two items with one placeholder, out of order, or below the first.
    let mut repeated = add.clone();
    repeated.curves[1].id = repeated.points[0].0;
    assert!(matches!(fails(repeated), Err(EditError::Placeholder(_))));
    let mut unordered = add.clone();
    unordered.points.swap(0, 1);
    assert!(matches!(fails(unordered), Err(EditError::Placeholder(_))));
    let mut below = add.clone();
    below.points[0].0 = line;
    assert_eq!(fails(below), Err(EditError::Placeholder(line)));
    // An old id naming nothing, or the wrong kind, is the check's.
    let mut wrong = add;
    wrong.constraints.push(Constraint::Midpoint {
        point: points[2],
        line: points[2],
    });
    assert!(matches!(
        fails(wrong),
        Err(EditError::Sketch(SketchError::Reference { .. }))
    ));
}

#[test]
fn an_add_made_after_one_not_applied_names_nothing_of_it() {
    // The first draws a line; the second, made while the first was on its
    // way, draws on from the line's end.
    let (sketch, _, _) = drawn();
    let mut first = Add::new(&sketch);
    let a = first.point(DVec2::new(0.0, 20.0)).unwrap();
    let b = first.point(DVec2::new(10.0, 20.0)).unwrap();
    first
        .curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let made = SketchEdit::Add(first).apply(&sketch, &DESIGN).unwrap();
    let mut second = Add::new(&made);
    let c = second.point(DVec2::new(10.0, 30.0)).unwrap();
    second
        .curve(Curve::Line { start: b, end: c }, false)
        .unwrap();
    let second = SketchEdit::Add(second);
    assert!(second.apply(&made, &DESIGN).is_ok());
    // Without the first, its end would be the second's new point's id.
    assert_eq!(second.apply(&sketch, &DESIGN), Err(EditError::Target(b)));
}

#[test]
fn adding_past_the_last_id_fails() {
    let (mut sketch, _, _) = drawn();
    sketch.next_id = LAST_ID - 2;
    let mut add = Add::new(&sketch);
    let a = add.point(DVec2::ZERO).unwrap();
    add.point(DVec2::X).unwrap();
    // The builder runs out of placeholders before the origin's and axes'
    // ids.
    assert_eq!(add.point(DVec2::Y), Err(OutOfIds));
    // Applied to a sketch one id on, it runs out too.
    let mut on = sketch.clone();
    on.next_id += 1;
    let edit = SketchEdit::Add(add.clone());
    assert_eq!(edit.apply(&on, &DESIGN), Err(EditError::OutOfIds));
    // One point fits there, taking the last id.
    let mut one = add;
    one.points.truncate(1);
    let added = SketchEdit::Add(one).apply(&on, &DESIGN).unwrap();
    assert_eq!(added.next_id, LAST_ID);
    assert!(added.point(Id(a.0 + 1)).is_some());
}

#[test]
fn ids_are_never_reused() {
    let (sketch, points, line) = drawn();
    let deleted = SketchEdit::Delete(vec![points[2]])
        .apply(&sketch, &DESIGN)
        .unwrap();
    assert_eq!(deleted.next_id, sketch.next_id);
    let mut add = Add::new(&deleted);
    let new = add.point(DVec2::ONE).unwrap();
    add.constraints.push(Constraint::Horizontal(line));
    let added = SketchEdit::Add(add).apply(&deleted, &DESIGN).unwrap();
    // The deleted point's id stays unused.
    assert!(new > points[2]);
    assert!(added.constraints[0].id > new);
    assert_eq!(added.kind(points[2]), None);
}

#[test]
fn deleting_takes_what_depends_along() {
    let (mut sketch, [start, end, lone], line) = drawn();
    let horizontal = sketch.add_constraint(Constraint::Horizontal(line)).unwrap();
    let on = sketch
        .add_constraint(Constraint::PointOnCurve {
            point: lone,
            curve: line,
        })
        .unwrap();
    let deleted = SketchEdit::Delete(vec![start])
        .apply(&sketch, &DESIGN)
        .unwrap();
    // The line made from it, its other end only it used, and the
    // constraints on the line.
    assert_eq!(deleted.kind(line), None);
    assert_eq!(deleted.kind(end), None);
    assert_eq!(deleted.kind(horizontal), None);
    assert_eq!(deleted.kind(on), None);
    assert!(deleted.point(lone).is_some());
    // What names nothing is ignored.
    assert_eq!(
        SketchEdit::Delete(vec![start]).apply(&deleted, &DESIGN),
        Ok(deleted)
    );
}

#[test]
fn moving_needs_points_and_circles_within_the_limit() {
    let (mut sketch, [start, _, lone], line) = drawn();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center: lone,
                radius: 2.0,
            },
            false,
        )
        .unwrap();
    let edit = SketchEdit::Move {
        points: vec![(start, DVec2::new(-1.0, 2.0))],
        radii: vec![(circle, 3.0)],
    };
    let moved = edit.apply(&sketch, &DESIGN).unwrap();
    assert_eq!(moved.point(start).unwrap().at, DVec2::new(-1.0, 2.0));
    assert_eq!(moved.round(circle).unwrap().1, 3.0);

    let fails = |points: Vec<(Id, DVec2)>, radii: Vec<(Id, f64)>| {
        SketchEdit::Move { points, radii }.apply(&sketch, &DESIGN)
    };
    assert_eq!(
        fails(vec![(line, DVec2::ZERO)], vec![]),
        Err(EditError::Target(line))
    );
    assert_eq!(
        fails(vec![], vec![(line, 1.0)]),
        Err(EditError::Target(line))
    );
    for bad in [f64::NAN, 2.0 * MAX] {
        assert!(matches!(
            fails(vec![(start, DVec2::new(bad, 0.0))], vec![]),
            Err(EditError::Sketch(SketchError::Coordinate { .. }))
        ));
        assert!(matches!(
            fails(vec![], vec![(circle, bad)]),
            Err(EditError::Sketch(SketchError::Radius { .. }))
        ));
    }
    assert!(matches!(
        fails(vec![], vec![(circle, 0.0)]),
        Err(EditError::Sketch(SketchError::Radius { .. }))
    ));
}

#[test]
fn construction_is_set_on_curves_only() {
    let (sketch, [start, ..], line) = drawn();
    let set = |ids| SketchEdit::SetConstruction {
        ids,
        construction: true,
    };
    let made = set(vec![line]).apply(&sketch, &DESIGN).unwrap();
    assert!(made.curve(line).unwrap().construction);
    assert_eq!(
        set(vec![line, start]).apply(&sketch, &DESIGN),
        Err(EditError::Target(start))
    );
}

#[test]
fn the_result_is_checked() {
    let (sketch, [start, end, _], line) = drawn();
    // A constraint whose items don't go together, or too many.
    let tangent = SketchEdit::constrain(
        &sketch,
        vec![Constraint::Tangent {
            a: line,
            b: line,
            side: crate::Side::Positive,
            at: None,
        }],
    );
    assert!(matches!(
        tangent.apply(&sketch, &DESIGN),
        Err(EditError::Sketch(SketchError::Repeated { .. }))
    ));
    let many = SketchEdit::constrain(
        &sketch,
        vec![Constraint::Coincident(start, end); crate::MAX_CONSTRAINTS + 1],
    );
    assert!(matches!(
        many.apply(&sketch, &DESIGN),
        Err(EditError::Sketch(SketchError::TooMany {
            list: List::Constraints,
            ..
        }))
    ));
}

/// A dimension of `measure` that's `text` millimetres, on the positive
/// side.
fn dimension_of(measure: Measure, text: &str, driving: bool) -> Dimension {
    let value = value(text, &measure);
    Dimension {
        measure,
        value,
        driving,
        label: DVec2::new(1.0, 2.0),
        side: crate::Side::Positive,
    }
}

#[test]
fn added_dimensions_name_new_items_and_get_ids_after_the_constraints() {
    let (sketch, points, line) = drawn();
    let (mut add, [apex, corner, base]) = triangle(&sketch, points, line);
    add.dimensions
        .push(dimension_of(Measure::Length(base), "7", true));
    add.dimensions
        .push(dimension_of(Measure::Distance(apex, line), "8", false));
    // Applied to a sketch added to since, the new items move along.
    let mut later = sketch.clone();
    point(&mut later, 1.0, 1.0);
    let edit = SketchEdit::Add(add);
    let (added, auto) = edit.apply_marked(&later, &DESIGN).unwrap();
    let moved = |id: Id| Id(id.0 + 1);
    let [length, distance] = [0, 1].map(|i| &added.dimensions[i]);
    assert_eq!(length.dimension.measure, Measure::Length(moved(base)));
    assert_eq!(
        distance.dimension.measure,
        Measure::Distance(moved(apex), line)
    );
    assert!(length.dimension.driving && !distance.dimension.driving);
    assert_eq!(length.dimension.label, DVec2::new(1.0, 2.0));
    // After the constraints, before the auto one.
    let constraint = added.constraints[1].id;
    assert_eq!(length.id, Id(constraint.0 + 1));
    assert_eq!(distance.id, Id(length.id.0 + 1));
    assert_eq!(auto, [Id(distance.id.0 + 1)]);
    assert_eq!(edit.driving(&later, &added), [length.id]);
    assert_eq!(added.kind(moved(corner)), Some(Kind::Point));

    // A new dimension naming a placeholder of no new item.
    let mut add = Add::new(&sketch);
    add.dimensions.push(dimension_of(
        Measure::Length(Id(sketch.next_id + 3)),
        "7",
        true,
    ));
    assert!(SketchEdit::Add(add).apply(&sketch, &DESIGN).is_err());
}

#[test]
fn dimensions_are_set_made_driving_or_reference_and_their_labels_moved() {
    let (mut sketch, _, line) = drawn();
    let id = sketch
        .add_dimension(dimension_of(Measure::Length(line), "4", false))
        .unwrap();
    let dimension = |sketch: &Sketch| sketch.dimension(id).unwrap().dimension.clone();

    let set = SketchEdit::SetDimension {
        id,
        value: value("2 * 3", &Measure::Length(line)),
    };
    let changed = set.apply(&sketch, &DESIGN).unwrap();
    assert_eq!(dimension(&changed).value.text, "2 * 3");
    assert_eq!(dimension(&changed).value.value, 6.0);

    // Made driving, it holds what it measures, in the design's units.
    let driving = SketchEdit::SetDriving { id, driving: true };
    let held = driving.apply(&sketch, &DESIGN).unwrap();
    assert!(dimension(&held).driving);
    assert_eq!(dimension(&held).value.value, 10.0);
    assert_eq!(dimension(&held).value.text, "10 mm");
    assert_eq!(driving.driving(&sketch, &held), [id]);
    let inches = Design {
        units: LengthUnit::In,
        ..DESIGN
    };
    let mut inch = sketch.clone();
    inch.dimension_mut(id).unwrap().dimension.value =
        Value::new("4", &Measure::Length(line).ask(&inches)).unwrap();
    let held = driving.apply(&inch, &inches).unwrap();
    assert_eq!(dimension(&held).value.text, "0.3937 in");
    // Driving already, it keeps its value; made a reference, it keeps it
    // too.
    let again = driving
        .apply(&changed_driving(&changed, id), &DESIGN)
        .unwrap();
    assert_eq!(dimension(&again).value.value, 6.0);
    assert!(
        driving
            .driving(&changed_driving(&changed, id), &again)
            .is_empty()
    );
    let reference = SketchEdit::SetDriving { id, driving: false }
        .apply(&again, &DESIGN)
        .unwrap();
    assert!(!dimension(&reference).driving);
    assert_eq!(dimension(&reference).value.value, 6.0);

    let label = DVec2::new(-3.0, 4.0);
    let moved = SketchEdit::MoveLabel { id, label }
        .apply(&sketch, &DESIGN)
        .unwrap();
    assert_eq!(dimension(&moved).label, label);
    assert!(matches!(
        SketchEdit::MoveLabel {
            id,
            label: DVec2::splat(2.0 * MAX)
        }
        .apply(&sketch, &DESIGN),
        Err(EditError::Sketch(SketchError::Label(_)))
    ));

    // Naming no dimension, or a value its text doesn't give.
    for edit in [
        SketchEdit::SetDimension {
            id: line,
            value: value("1", &Measure::Length(line)),
        },
        SketchEdit::SetDriving {
            id: line,
            driving: true,
        },
        SketchEdit::MoveLabel {
            id: line,
            label: DVec2::ZERO,
        },
    ] {
        assert_eq!(edit.apply(&sketch, &DESIGN), Err(EditError::Target(line)));
    }
    let wrong = SketchEdit::SetDimension {
        id,
        value: Value {
            text: "2".to_owned(),
            value: 3.0,
        },
    };
    assert_eq!(
        wrong.apply(&sketch, &DESIGN),
        Err(EditError::Sketch(SketchError::Value(id)))
    );
}

/// `sketch` with its dimension `id` driving as it is.
fn changed_driving(sketch: &Sketch, id: Id) -> Sketch {
    let mut sketch = sketch.clone();
    sketch.dimension_mut(id).unwrap().dimension.driving = true;
    sketch
}

#[test]
fn a_reference_crossed_to_the_other_side_is_made_driving_where_it_is() {
    let (mut sketch, [_, _, lone], line) = drawn();
    let measure = Measure::Distance(lone, line);
    let id = sketch
        .add_dimension(Dimension {
            side: crate::Side::Negative,
            ..dimension_of(measure, "5", false)
        })
        .unwrap();
    let held = SketchEdit::SetDriving { id, driving: true }
        .apply(&sketch, &DESIGN)
        .unwrap();
    let dimension = &held.dimension(id).unwrap().dimension;
    assert_eq!(dimension.side, crate::Side::Positive);
    assert_eq!(dimension.value.value, 5.0);
}

#[test]
fn a_distance_between_points_at_one_place_is_refused_driving() {
    let (mut sketch, [start, ..], _) = drawn();
    let twin = point(&mut sketch, 0.0, 0.0);
    let measure = Measure::Distance(start, twin);
    // Driving, there's nothing to tell which way to move them apart.
    let mut add = Add::new(&sketch);
    add.dimensions
        .push(dimension_of(measure.clone(), "5", true));
    let driving = SketchEdit::Add(add);
    assert_eq!(driving.apply(&sketch, &DESIGN), Err(EditError::SamePlace));
    assert_eq!(
        EditError::SamePlace.to_string(),
        "the points are at the same place; move them apart first"
    );
    // A reference measures nothing to hold, and isn't made driving.
    let id = sketch
        .add_dimension(dimension_of(measure.clone(), "5", false))
        .unwrap();
    let made_driving = SketchEdit::SetDriving { id, driving: true };
    assert_eq!(
        made_driving.apply(&sketch, &DESIGN),
        Err(EditError::SamePlace)
    );
    assert_eq!(
        sketch.held(&measure, crate::Side::Positive, &DESIGN),
        Err(EditError::SamePlace)
    );
}

#[test]
fn what_a_dimension_can_t_be_is_refused_saying_what_it_measures() {
    let mut sketch = Sketch::default();
    let far = [(-MAX, 0.0), (MAX, 0.0)].map(|(x, y)| point(&mut sketch, x, y));
    let near = [(0.0, 5.0), (0.0004, 5.0)].map(|(x, y)| point(&mut sketch, x, y));
    let measure = Measure::HorizontalDistance(far[0], far[1]);
    let side = sketch.side(&measure);
    let too_far = sketch.held(&measure, side, &DESIGN).unwrap_err();
    assert!(matches!(too_far, EditError::OutOfRange { measured, .. } if measured == 2.0 * MAX));
    assert_eq!(
        too_far.to_string(),
        "it measures 2000000 mm, more than a dimension can be, 1000000 mm"
    );
    let measure = Measure::HorizontalDistance(near[0], near[1]);
    let too_near = sketch.held(&measure, side, &DESIGN).unwrap_err();
    assert_eq!(
        too_near.to_string(),
        "it measures less than a dimension can be, 0.001 mm"
    );
    // Made driving, a reference past the limit says so too.
    let reference = Dimension {
        side,
        ..dimension_of(Measure::HorizontalDistance(far[0], far[1]), "5", false)
    };
    let id = sketch.add_dimension(reference).unwrap();
    let made_driving = SketchEdit::SetDriving { id, driving: true };
    assert!(matches!(
        made_driving.apply(&sketch, &DESIGN),
        Err(EditError::OutOfRange { .. })
    ));
}
