use super::*;
use crate::testing::{DESIGN, circle, line, point};
use crate::{Curve, SketchEdit};

/// Two lines sharing a corner and a third ending there: detached, each
/// line has a corner of its own, coincident with the first's, where it
/// was.
#[test]
fn detaching_gives_each_curve_but_the_first_its_own_point() {
    let mut sketch = Sketch::default();
    let corner = point(&mut sketch, 5.0, 5.0);
    let [a, b, c] = [(0.0, 5.0), (5.0, 0.0), (9.0, 9.0)].map(|(x, y)| point(&mut sketch, x, y));
    let first = line(&mut sketch, a, corner);
    let second = line(&mut sketch, corner, b);
    let third = line(&mut sketch, c, corner);
    assert!(sketch.detachable(corner));
    let detached = SketchEdit::Detach(corner).apply(&sketch, &DESIGN).unwrap();
    let ends = |curve| match detached.curve(curve).unwrap().curve {
        Curve::Line { start, end } => (start, end),
        _ => unreachable!(),
    };
    assert_eq!(ends(first), (a, corner));
    let (own, _) = ends(second);
    let (_, other) = ends(third);
    assert!(own != corner && other != corner && own != other);
    for id in [own, other] {
        assert_eq!(
            detached.point(id).unwrap().at,
            detached.point(corner).unwrap().at
        );
        assert!(
            detached
                .constraints
                .iter()
                .any(|entry| { entry.constraint == Constraint::Coincident(corner, id) })
        );
    }
    assert!(!detached.detachable(corner));
}

/// A point one curve is made from, a lone one, or the origin, can't be
/// detached.
#[test]
fn only_a_point_curves_share_is_detachable() {
    let mut sketch = Sketch::default();
    let hub = point(&mut sketch, 0.0, 0.0);
    circle(&mut sketch, hub, 2.0);
    let lone = point(&mut sketch, 5.0, 5.0);
    for id in [hub, lone, Id::ORIGIN] {
        assert!(!sketch.detachable(id), "{id:?}");
        assert_eq!(
            SketchEdit::Detach(id).apply(&sketch, &DESIGN),
            Err(EditError::Target(id))
        );
    }
}
