use glam::DVec2;

use super::*;
use crate::testing::{DESIGN, at, constrain, line, point};
use crate::{
    Add, Budget, Curve, Dimension, Goal, Kind, Side, SketchEdit, SketchError, analyse, propose,
    solve,
};

#[test]
fn every_sketch_has_the_origin_and_its_axes() {
    let sketch = Sketch::default();
    assert_eq!(sketch.kind(Id::ORIGIN), Some(Kind::Point));
    assert_eq!(sketch.kind(Id::X_AXIS), Some(Kind::Line));
    assert_eq!(sketch.kind(Id::Y_AXIS), Some(Kind::Line));
    assert_eq!(sketch.point(Id::ORIGIN).unwrap().at, DVec2::ZERO);
    assert_eq!(sketch.line(Id::X_AXIS), Some((DVec2::ZERO, DVec2::X)));
    assert_eq!(sketch.line(Id::Y_AXIS), Some((DVec2::ZERO, DVec2::Y)));
    // Made from no points of the sketch's, the axes are no curves of it.
    assert!(sketch.curve(Id::X_AXIS).is_none());
    assert_eq!(sketch.name(Id::ORIGIN).as_deref(), Some("Origin"));
    assert_eq!(sketch.name(Id::Y_AXIS).as_deref(), Some("Y axis"));
    assert!(
        [Id::ORIGIN, Id::X_AXIS, Id::Y_AXIS]
            .iter()
            .all(|id| id.is_builtin())
    );
    assert!(Id::X_AXIS.is_axis() && Id::Y_AXIS.is_axis() && !Id::ORIGIN.is_axis());
    // Nor are they in its lists.
    assert!(sketch.points.is_empty() && sketch.curves.is_empty());
}

#[test]
fn the_origin_and_axes_are_never_deleted() {
    let mut sketch = Sketch::default();
    let p = point(&mut sketch, 1.0, 0.0);
    let on = constrain(
        &mut sketch,
        Constraint::PointOnCurve {
            point: p,
            curve: Id::X_AXIS,
        },
    );
    let before = sketch.clone();
    sketch.delete(&[Id::ORIGIN, Id::X_AXIS, Id::Y_AXIS]);
    assert_eq!(sketch, before);
    assert!(sketch.constraint(on).is_some());
}

#[test]
fn check_refuses_the_origin_and_axes_used_as_they_can_t_be() {
    let mut sketch = Sketch::default();
    let p = point(&mut sketch, 1.0, 2.0);
    let q = point(&mut sketch, 3.0, 4.0);
    let pq = line(&mut sketch, p, q);
    assert_eq!(sketch.check(&DESIGN), Ok(()));

    let refused = |constraint: Constraint| {
        let mut with = sketch.clone();
        let id = with.add_constraint(constraint).unwrap();
        assert_eq!(with.check(&DESIGN), Err(SketchError::Builtin(id)));
    };
    refused(Constraint::PointOnCurve {
        point: Id::ORIGIN,
        curve: Id::X_AXIS,
    });
    refused(Constraint::Perpendicular(Id::X_AXIS, Id::Y_AXIS));
    refused(Constraint::Fix(Id::ORIGIN));
    refused(Constraint::Midpoint {
        point: p,
        line: Id::X_AXIS,
    });
    refused(Constraint::Equal(pq, Id::Y_AXIS));

    let fits = |constraint: Constraint| {
        let mut with = sketch.clone();
        with.add_constraint(constraint).unwrap();
        assert_eq!(with.check(&DESIGN), Ok(()));
    };
    fits(Constraint::Coincident(p, Id::ORIGIN));
    fits(Constraint::Parallel(pq, Id::X_AXIS));
    fits(Constraint::Symmetric {
        a: p,
        b: q,
        about: Id::Y_AXIS,
    });

    let measured = |measure: Measure| {
        let mut with = sketch.clone();
        let side = with.side(&measure);
        let value = with.measure(&measure, side).unwrap();
        let dimension = Dimension {
            value: varde_expr::Value::new(&value.to_string(), &measure.ask(&DESIGN)).unwrap(),
            measure,
            driving: false,
            label: DVec2::ZERO,
            side,
        };
        let id = with.add_dimension(dimension).unwrap();
        with.check(&DESIGN).map_err(|why| (why, id))
    };
    assert!(measured(Measure::Distance(p, Id::ORIGIN)).is_ok());
    assert!(measured(Measure::Distance(Id::X_AXIS, p)).is_ok());
    assert!(measured(Measure::Distance(pq, Id::X_AXIS)).is_ok());
    let (why, id) = measured(Measure::Distance(Id::X_AXIS, pq)).unwrap_err();
    assert_eq!(why, SketchError::Builtin(id));

    // No curve is made from the origin.
    let mut from_origin = sketch.clone();
    let id = from_origin
        .add_curve(
            Curve::Line {
                start: Id::ORIGIN,
                end: p,
            },
            false,
        )
        .unwrap();
    assert_eq!(from_origin.check(&DESIGN), Err(SketchError::Builtin(id)));

    // Nor does a sketch give out their ids.
    let mut past = sketch.clone();
    past.next_id = FIRST_BUILTIN + 1;
    assert_eq!(
        past.check(&DESIGN),
        Err(SketchError::NextIdReserved(FIRST_BUILTIN + 1))
    );
}

#[test]
fn the_solver_holds_the_origin_and_axes_where_they_are() {
    let mut sketch = Sketch::default();
    let p = point(&mut sketch, 0.5, -0.3);
    let q = point(&mut sketch, 7.0, 2.0);
    let pq = line(&mut sketch, p, q);
    constrain(&mut sketch, Constraint::Coincident(p, Id::ORIGIN));
    constrain(&mut sketch, Constraint::Parallel(pq, Id::X_AXIS));
    let solved = solve(&sketch, &Goal::Settle, &Budget::default())
        .unwrap()
        .sketch;
    assert!(at(&solved, p).length() < 1e-9);
    assert!(at(&solved, q).y.abs() < 1e-9);
    let analysis = analyse(&solved);
    // Only the line's length is left.
    assert_eq!(analysis.freedom, 1);
    assert!(analysis.fixed.contains(&p));

    // Dragged, the line stays on the axis.
    let goal = Goal::Drag {
        points: vec![(q, DVec2::new(9.0, 3.0))],
        radii: Vec::new(),
    };
    let dragged = solve(&solved, &goal, &Budget::default()).unwrap().sketch;
    assert!(at(&dragged, q).y.abs() < 1e-6);
    assert!(at(&dragged, p).length() < 1e-9);
}

#[test]
fn an_edit_names_the_origin_and_axes_as_they_are() {
    let mut sketch = Sketch::default();
    point(&mut sketch, 3.0, 3.0);
    let mut add = Add::new(&sketch);
    let start = add.point(DVec2::new(0.1, 0.0)).unwrap();
    let end = add.point(DVec2::new(5.0, 0.2)).unwrap();
    let new = add.curve(Curve::Line { start, end }, false).unwrap();
    add.constraints
        .push(Constraint::Coincident(start, Id::ORIGIN));
    // Snaps: the end on the x axis and the line horizontal restate each
    // other with the start at the origin; one of them goes.
    add.auto.push(Constraint::PointOnCurve {
        point: end,
        curve: Id::X_AXIS,
    });
    add.auto.push(Constraint::Horizontal(new));
    assert_eq!(
        add.resolve(sketch.next_id + 4, Id::ORIGIN),
        Some(Id::ORIGIN)
    );
    let edit = SketchEdit::Add(add);
    let accepted = propose(&sketch, &edit, &DESIGN, &Budget::default()).unwrap();
    let solved = &accepted.sketch;
    assert_eq!(solved.constraints.len(), 2);
    assert!(accepted.analysis.redundant.is_empty());
    let (a, b) = solved.line(solved.curves[0].id).unwrap();
    assert!(a.length() < 1e-9 && b.y.abs() < 1e-9);
}

#[test]
fn a_tangent_to_an_axis_keeps_its_side() {
    let mut sketch = Sketch::default();
    let center = point(&mut sketch, 2.0, 3.0);
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 2.5,
            },
            false,
        )
        .unwrap();
    let tangent = sketch.tangent(Id::X_AXIS, circle).unwrap();
    assert_eq!(
        tangent,
        Constraint::Tangent {
            a: Id::X_AXIS,
            b: circle,
            side: Side::Positive,
            at: None,
        }
    );
    constrain(&mut sketch, tangent);
    let solved = solve(&sketch, &Goal::Settle, &Budget::default())
        .unwrap()
        .sketch;
    let (center_at, radius) = solved.round(circle).unwrap();
    assert!((center_at.y - radius).abs() < 1e-9);
}
