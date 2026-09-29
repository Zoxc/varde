use super::*;

#[test]
fn check_bounds_values_and_resolves_ids() {
    let (a, b) = (PointId(0), PointId(1));
    let mut sketch = Sketch {
        points: vec![DVec2::ZERO, DVec2::splat(10.0)],
        entities: vec![Entity::Line { start: a, end: b }],
        constraints: vec![Constraint::Distance(a, b, 10.0)],
    };
    assert_eq!(sketch.check(10.0), Ok(()));
    assert!(matches!(
        sketch.check(9.0),
        Err(SketchError::Coordinate { value: 10.0, .. })
    ));

    for bad in [f64::NAN, 0.0, -1.0, 11.0] {
        sketch.constraints.push(Constraint::Distance(a, b, bad));
        assert!(matches!(
            sketch.check(10.0),
            Err(SketchError::Distance { .. })
        ));
        sketch.constraints.pop();
    }

    sketch
        .constraints
        .push(Constraint::Coincident(a, PointId(2)));
    assert_eq!(
        sketch.check(10.0),
        Err(SketchError::UnknownPoint {
            id: PointId(2),
            points: 2
        })
    );
}
