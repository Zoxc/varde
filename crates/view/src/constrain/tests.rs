use glam::DVec2;
use varde_sketch::{Curve, Handle, Side, Spline};

use super::*;
use ConstraintKind::*;

/// A sketch with a nearly horizontal line, a steep one, a lone point, a
/// circle and an arc about another centre, and a spline on from the
/// flat line's end with a handle there, and their ids.
struct Drawn {
    sketch: Sketch,
    flat: Id,
    steep: Id,
    lone: Id,
    circle: Id,
    arc: Id,
    spline: Id,
    flat_start: Id,
}

fn drawn() -> Drawn {
    let mut sketch = Sketch::default();
    let mut point = |x, y| sketch.add_point(DVec2::new(x, y)).unwrap();
    let (a, b) = (point(0.0, 0.0), point(10.0, 1.0));
    let (c, d) = (point(20.0, 0.0), point(21.0, 10.0));
    let lone = point(5.0, 5.0);
    let center = point(0.0, 20.0);
    let (arc_center, arc_start, arc_end) =
        (point(30.0, 20.0), point(34.0, 20.0), point(30.0, 24.0));
    let flat = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let steep = sketch
        .add_curve(Curve::Line { start: c, end: d }, false)
        .unwrap();
    let circle = sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 3.0,
            },
            false,
        )
        .unwrap();
    let arc = sketch
        .add_curve(
            Curve::Arc {
                center: arc_center,
                start: arc_start,
                end: arc_end,
            },
            false,
        )
        .unwrap();
    let mut point = |x, y| sketch.add_point(DVec2::new(x, y)).unwrap();
    let (on, last, tip) = (point(15.0, 5.0), point(18.0, 3.0), point(13.0, 1.3));
    let mut through = Spline::through(vec![b, on, last], false);
    through.handles.push(Handle { at: b, tip });
    let spline = sketch.add_curve(Curve::Spline(through), false).unwrap();
    Drawn {
        sketch,
        flat,
        steep,
        lone,
        circle,
        arc,
        spline,
        flat_start: a,
    }
}

fn set(ids: &[Id]) -> BTreeSet<Id> {
    ids.iter().copied().collect()
}

#[test]
fn only_what_fits_is_offered_most_likely_first() {
    let d = drawn();
    let fitting = |ids: &[Id]| ConstraintKind::fitting(&d.sketch, &set(ids));

    assert!(fitting(&[]).is_empty());
    // A nearly flat line is more likely horizontal, a steep one vertical.
    assert_eq!(fitting(&[d.flat]), [Horizontal, Vertical, Fix]);
    assert_eq!(fitting(&[d.steep]), [Vertical, Horizontal, Fix]);
    // Two lines at about right angles: perpendicular before parallel.
    assert_eq!(
        fitting(&[d.flat, d.steep]),
        [Perpendicular, Parallel, Horizontal, Vertical, Equal, Fix]
    );
    assert_eq!(
        fitting(&[d.lone, d.flat]),
        [Coincident, Midpoint, Fix],
        "a point on the line, or at its middle"
    );
    assert_eq!(fitting(&[d.lone, d.circle]), [Coincident, Concentric, Fix]);
    assert_eq!(fitting(&[d.flat, d.circle]), [Tangent, Fix]);
    assert_eq!(
        fitting(&[d.circle, d.arc]),
        [Tangent, Equal, Concentric, Fix]
    );
    let other = d.sketch.points[1].id;
    assert_eq!(
        fitting(&[d.lone, other]),
        [Coincident, Horizontal, Vertical, Fix]
    );
    // A point of the line's own, symmetric about it, fits nothing but a
    // fix: the check refuses a curve with its own point.
    assert_eq!(fitting(&[d.flat_start, d.lone, d.steep]), [Symmetric, Fix]);
    assert_eq!(fitting(&[d.flat_start, d.flat]), [Fix]);
}

#[test]
fn circles_about_one_centre_are_offered_concentric_first() {
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::ZERO).unwrap();
    let b = sketch.add_point(DVec2::new(0.1, 0.0)).unwrap();
    let big = sketch
        .add_curve(
            Curve::Circle {
                center: a,
                radius: 5.0,
            },
            false,
        )
        .unwrap();
    let small = sketch
        .add_curve(
            Curve::Circle {
                center: b,
                radius: 2.0,
            },
            false,
        )
        .unwrap();
    assert_eq!(
        ConstraintKind::fitting(&sketch, &set(&[big, small])),
        [Concentric, Tangent, Equal, Fix]
    );
}

#[test]
fn several_items_are_tied_to_the_first() {
    let d = drawn();
    let make = |kind: ConstraintKind, ids: &[Id]| kind.make(&d.sketch, &set(ids));
    let [p, q, r] = [0, 1, 2].map(|i| d.sketch.points[i].id);
    assert_eq!(
        make(Coincident, &[p, q, r]),
        Some(vec![
            Constraint::Coincident(p, q),
            Constraint::Coincident(p, r)
        ])
    );
    assert_eq!(
        make(Horizontal, &[d.flat, d.steep]),
        Some(vec![
            Constraint::Horizontal(d.flat),
            Constraint::Horizontal(d.steep)
        ])
    );
    assert_eq!(
        make(Vertical, &[p, r]),
        Some(vec![Constraint::VerticalPoints(p, r)])
    );
    assert_eq!(make(Horizontal, &[p, q, r]), None);
    assert_eq!(
        make(Equal, &[d.circle, d.arc]),
        Some(vec![Constraint::Equal(d.circle, d.arc)])
    );
    assert_eq!(make(Equal, &[d.flat, d.circle]), None);
    assert_eq!(
        make(Concentric, &[d.lone, d.circle, d.arc]),
        Some(vec![
            Constraint::Concentric(d.circle, d.lone),
            Constraint::Concentric(d.circle, d.arc)
        ])
    );
    assert_eq!(
        make(Fix, &[d.lone, d.flat]),
        Some(vec![Constraint::Fix(d.lone), Constraint::Fix(d.flat)])
    );
    // Constraints selected are left out.
    let mut sketch = d.sketch.clone();
    let fixed = sketch.add_constraint(Constraint::Fix(d.lone)).unwrap();
    assert_eq!(Fix.make(&sketch, &set(&[fixed])), None);
    assert_eq!(
        Fix.make(&sketch, &set(&[fixed, d.flat])),
        Some(vec![Constraint::Fix(d.flat)])
    );
}

#[test]
fn a_tangent_takes_the_side_the_geometry_is_on() {
    let d = drawn();
    // The circle's centre is left of the flat line, looking along it.
    assert_eq!(
        Tangent.make(&d.sketch, &set(&[d.flat, d.circle])),
        Some(vec![Constraint::Tangent {
            a: d.flat,
            b: d.circle,
            side: Side::Positive,
            at: None,
        }])
    );
}

#[test]
fn every_kind_is_the_kind_of_what_it_makes() {
    let d = drawn();
    let selections: [&[Id]; 7] = [
        &[d.flat],
        &[d.flat, d.spline],
        &[d.flat, d.steep],
        &[d.lone, d.flat],
        &[d.circle, d.arc],
        &[d.lone, d.circle],
        &[d.flat_start, d.lone, d.steep],
    ];
    let mut made_kinds = BTreeSet::new();
    for kind in ConstraintKind::ALL {
        for ids in selections {
            for constraint in kind.make(&d.sketch, &set(ids)).into_iter().flatten() {
                assert_eq!(ConstraintKind::of(&constraint), kind);
                made_kinds.insert(kind.label());
            }
        }
    }
    assert_eq!(made_kinds.len(), ConstraintKind::ALL.len());
    // Equal offsets are the Offset tool's alone: none are made of a
    // selection.
    let equal = Constraint::EqualOffset {
        a: [d.flat, d.steep],
        b: [d.steep, d.flat],
    };
    assert_eq!(ConstraintKind::of(&equal), ConstraintKind::Offset);
    assert!(!ConstraintKind::ALL.contains(&ConstraintKind::Offset));
    for ids in selections {
        assert!(ConstraintKind::Offset.make(&d.sketch, &set(ids)).is_none());
    }
}

#[test]
fn sets_hold_what_they_are_made_of() {
    let set: ConstraintSet = [Tangent, Fix].into_iter().collect();
    assert!(set.contains(Tangent) && set.contains(Fix));
    assert!(!set.contains(Coincident));
    assert!(!ConstraintSet::default().contains(Fix));
}

#[test]
fn the_origin_and_axes_are_constrained_to_as_they_can_be() {
    let d = drawn();
    let fitting = |ids: &[Id]| ConstraintKind::fitting(&d.sketch, &ids.iter().copied().collect());
    // A point to the origin, and a line to an axis.
    assert_eq!(
        Coincident.make(&d.sketch, &BTreeSet::from([d.lone, Id::ORIGIN])),
        Some(vec![Constraint::Coincident(d.lone, Id::ORIGIN)])
    );
    assert!(fitting(&[d.flat, Id::X_AXIS]).starts_with(&[Parallel, Perpendicular]));
    // Nothing between them alone, no midpoint of an axis, no length equal
    // to one's, and no fixing what's fixed already.
    assert!(fitting(&[Id::ORIGIN, Id::X_AXIS]).is_empty());
    assert!(fitting(&[Id::X_AXIS, Id::Y_AXIS]).is_empty());
    assert!(!fitting(&[d.lone, Id::Y_AXIS]).contains(&Midpoint));
    assert!(!fitting(&[d.flat, Id::Y_AXIS]).contains(&Equal));
    assert!(fitting(&[Id::ORIGIN]).is_empty());
}

#[test]
fn splines_take_a_point_on_them_a_tangent_a_smooth_join_and_a_fix() {
    let d = drawn();
    let fitting = |ids: &[Id]| ConstraintKind::fitting(&d.sketch, &set(ids));
    assert_eq!(fitting(&[d.lone, d.spline]), [Coincident, Fix]);
    assert_eq!(fitting(&[d.flat, d.spline]), [Tangent, Smooth, Fix]);
    assert_eq!(fitting(&[d.spline]), [Fix]);
    // Nothing else, with a spline among what's picked.
    assert_eq!(fitting(&[d.flat, d.steep, d.spline]), [Fix]);
    let b = d.sketch.points[1].id;
    assert_eq!(
        Smooth.make(&d.sketch, &set(&[d.flat, d.spline])),
        Some(vec![Constraint::Smooth {
            a: d.flat,
            b: d.spline,
            at: b,
            side: Side::Positive,
        }])
    );
    // At the spline's end nearer the other: the circle's is the one with
    // the handle, the arc's the other, straight, which can't be smooth.
    assert_eq!(fitting(&[d.circle, d.spline]), [Tangent, Smooth, Fix]);
    assert_eq!(fitting(&[d.arc, d.spline]), [Tangent, Fix]);
}
