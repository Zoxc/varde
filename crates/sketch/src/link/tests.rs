use glam::{DAffine2, DMat2, DVec2};

use super::*;
use crate::testing::{DESIGN, at, line, point, propose_it};
use crate::{Constraint, Rejected, SketchEdit, SketchError};

/// A line from (0, 0) to (10, 0) as a shape.
fn segment(from: DVec2, to: DVec2) -> LinkShape {
    let mut shape = LinkShape::default();
    let start = shape.point(from);
    let end = shape.point(to);
    shape.curve(Curve::Line { start, end });
    shape
}

/// A sketch with a link of `kind` holding `shape`, and the link's id.
fn linked(kind: LinkKind, shape: &LinkShape) -> (Sketch, Id) {
    let sketch = Sketch::default();
    let added = SketchEdit::AddLink { kind }
        .apply(&sketch, &DESIGN)
        .unwrap();
    let link = added.links[0].id;
    let relinked = SketchEdit::Relink(vec![(link, shape.clone())])
        .apply(&added, &DESIGN)
        .unwrap();
    (relinked, link)
}

#[test]
fn a_link_starts_empty_and_takes_its_shape() {
    let shape = segment(DVec2::ZERO, DVec2::new(10.0, 0.0));
    let (sketch, id) = linked(LinkKind::Project, &shape);
    let link = sketch.link(id).unwrap();
    assert_eq!((link.points.len(), link.curves.len()), (2, 1));
    // Added links count for profiles, so their curves aren't construction.
    assert!(link.profiles);
    assert!(!sketch.curve(link.curves[0]).unwrap().construction);
    assert_eq!(sketch.link_shape(link), Some(shape));
    assert!(sketch.is_linked(link.points[0]));
    assert_eq!(sketch.link_of(link.curves[0]).map(|l| l.id), Some(id));
    assert_eq!(sketch.check(&DESIGN), Ok(()));
    // The link's own id names no item.
    assert_eq!(sketch.kind(id), None);
}

#[test]
fn relinking_the_same_form_keeps_ids_and_what_is_on_them() {
    let (mut sketch, id) = linked(
        LinkKind::Project,
        &segment(DVec2::ZERO, DVec2::new(10.0, 0.0)),
    );
    let link = sketch.link(id).unwrap().clone();
    // The user's line held parallel to the link's.
    let a = point(&mut sketch, 0.0, 5.0);
    let b = point(&mut sketch, 8.0, 5.5);
    let own = line(&mut sketch, a, b);
    let parallel = sketch
        .add_constraint(Constraint::Parallel(own, link.curves[0]))
        .unwrap();
    assert_eq!(sketch.check(&DESIGN), Ok(()));

    let moved = segment(DVec2::new(0.0, 1.0), DVec2::new(10.0, 3.0));
    let accepted = propose_it(&sketch, &SketchEdit::Relink(vec![(id, moved.clone())])).unwrap();
    let relinked = &accepted.sketch;
    assert_eq!(relinked.link(id), Some(&link));
    assert_eq!(relinked.link_shape(&link), Some(moved));
    assert!(relinked.constraint(parallel).is_some());
    // The user's line followed: parallel to the moved link line.
    let along = at(relinked, b) - at(relinked, a);
    assert!(along.perp_dot(DVec2::new(10.0, 2.0)).abs() < 1e-6);
}

#[test]
fn relinking_another_form_keeps_what_it_can() {
    let (mut sketch, id) = linked(
        LinkKind::Intersect,
        &segment(DVec2::ZERO, DVec2::new(10.0, 0.0)),
    );
    let old = sketch.link(id).unwrap().clone();
    let free = point(&mut sketch, 20.0, 20.0);
    let on_end = sketch
        .add_constraint(Constraint::Coincident(free, old.points[1]))
        .unwrap();
    let on_line = sketch
        .add_constraint(Constraint::PointOnCurve {
            point: free,
            curve: old.curves[0],
        })
        .unwrap();
    // A circle: no line in it, so the line goes with what's on it; its
    // center keeps the id of the point nearest it, and with it what's on
    // that.
    let mut circle = LinkShape::default();
    let center = circle.point(DVec2::new(9.0, 2.0));
    circle.curve(Curve::Circle {
        center,
        radius: 3.0,
    });
    let relinked = SketchEdit::Relink(vec![(id, circle.clone())])
        .apply(&sketch, &DESIGN)
        .unwrap();
    let link = relinked.link(id).unwrap();
    assert_eq!(relinked.link_shape(link), Some(circle.clone()));
    assert_eq!(link.points, vec![old.points[1]]);
    assert!(relinked.point(old.points[0]).is_none());
    assert!(relinked.curve(old.curves[0]).is_none());
    assert!(relinked.constraint(on_end).is_some());
    assert!(relinked.constraint(on_line).is_none());
    assert!(relinked.link_follows(id, &circle, 1e-9));
    assert_eq!(relinked.check(&DESIGN), Ok(()));
}

/// Lines through `places` in turn, closed: a section's outline.
fn outline(places: &[(f64, f64)]) -> LinkShape {
    let mut shape = LinkShape::default();
    let points: Vec<Id> = (places.iter())
        .map(|&(x, y)| shape.point(DVec2::new(x, y)))
        .collect();
    for (i, &start) in points.iter().enumerate() {
        let end = points[(i + 1) % points.len()];
        shape.curve(Curve::Line { start, end });
    }
    shape
}

#[test]
fn a_section_gaining_a_side_keeps_its_lines_and_corners() {
    let square = outline(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    let (mut sketch, id) = linked(LinkKind::Intersect, &square);
    let old = sketch.link(id).unwrap().clone();
    let free = point(&mut sketch, 3.0, 4.0);
    let on_side = sketch
        .add_constraint(Constraint::PointOnCurve {
            point: free,
            curve: old.curves[1],
        })
        .unwrap();
    // A corner cut off: five sides.
    let cut = outline(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 8.0),
        (8.0, 10.0),
        (0.0, 10.0),
    ]);
    let relinked = SketchEdit::Relink(vec![(id, cut.clone())])
        .apply(&sketch, &DESIGN)
        .unwrap();
    let link = relinked.link(id).unwrap();
    assert_eq!(&link.points[..4], &old.points[..]);
    assert_eq!(&link.curves[..4], &old.curves[..]);
    assert_eq!((link.points.len(), link.curves.len()), (5, 5));
    assert!(relinked.constraint(on_side).is_some());
    assert_eq!(relinked.check(&DESIGN), Ok(()));
    // It holds what was found, so finding it again changes nothing.
    assert!(relinked.link_follows(id, &cut, 1e-9));
    let again = SketchEdit::Relink(vec![(id, cut.clone())])
        .apply(&relinked, &DESIGN)
        .unwrap();
    assert_eq!(again, relinked);
    // And back to the square: the side added goes, the rest stay.
    let back = SketchEdit::Relink(vec![(id, square.clone())])
        .apply(&relinked, &DESIGN)
        .unwrap();
    let link = back.link(id).unwrap();
    assert_eq!(link.points, old.points);
    assert_eq!(link.curves, old.curves);
    assert!(back.link_follows(id, &square, 1e-9));
}

#[test]
fn a_new_curve_found_first_keeps_the_ids_after_it() {
    // Two lines, then an arc found before them: the lines keep their
    // ids, the arc's are new, and the link lists them in id order.
    let mut two = LinkShape::default();
    let [a, b, c] =
        [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)].map(|(x, y)| two.point(DVec2::new(x, y)));
    two.curve(Curve::Line { start: a, end: b });
    two.curve(Curve::Line { start: b, end: c });
    let (sketch, id) = linked(LinkKind::Project, &two);
    let old = sketch.link(id).unwrap().clone();

    let mut three = LinkShape::default();
    let center = three.point(DVec2::new(0.0, 5.0));
    let start = three.point(DVec2::new(0.0, 10.0));
    let a = three.point(DVec2::new(0.0, 0.0));
    let b = three.point(DVec2::new(10.0, 0.0));
    let c = three.point(DVec2::new(10.0, 10.0));
    three.curve(Curve::Arc {
        center,
        start,
        end: a,
    });
    three.curve(Curve::Line { start: a, end: b });
    three.curve(Curve::Line { start: b, end: c });
    let relinked = SketchEdit::Relink(vec![(id, three.clone())])
        .apply(&sketch, &DESIGN)
        .unwrap();
    let link = relinked.link(id).unwrap();
    assert_eq!(&link.points[..3], &old.points[..]);
    assert_eq!(&link.curves[..2], &old.curves[..]);
    assert_eq!(at(&relinked, old.points[0]), DVec2::ZERO);
    assert_eq!(relinked.check(&DESIGN), Ok(()));
    assert!(relinked.link_follows(id, &three, 1e-9));
    // Not what it holds: another shape, or this one moved.
    assert!(!relinked.link_follows(id, &two, 1e-9));
    let mut moved = three.clone();
    moved.points[0].x += 1.0;
    assert!(!relinked.link_follows(id, &moved, 1e-9));
}

#[test]
fn a_spline_found_with_more_points_keeps_its_id() {
    let fit = |count: usize| {
        let mut shape = LinkShape::default();
        let points = (0..count)
            .map(|i| shape.point(DVec2::new(i as f64, (i % 2) as f64)))
            .collect();
        shape.curve(Curve::Spline(Spline::through(points, false)));
        shape
    };
    let (sketch, id) = linked(LinkKind::Project, &fit(4));
    let old = sketch.link(id).unwrap().clone();
    let relinked = SketchEdit::Relink(vec![(id, fit(6))])
        .apply(&sketch, &DESIGN)
        .unwrap();
    let link = relinked.link(id).unwrap();
    assert_eq!(link.curves, old.curves);
    assert_eq!(&link.points[..4], &old.points[..]);
    assert_eq!(relinked.check(&DESIGN), Ok(()));
    assert!(relinked.link_follows(id, &fit(6), 1e-9));
}

#[test]
fn link_geometry_is_fixed_in_the_solver() {
    let (mut sketch, id) = linked(
        LinkKind::Project,
        &segment(DVec2::ZERO, DVec2::new(10.0, 0.0)),
    );
    let link = sketch.link(id).unwrap().clone();
    let free = point(&mut sketch, 3.0, 4.0);
    // A point made coincident with the link's end moves to it, never the
    // other way.
    let edit = SketchEdit::constrain(&sketch, vec![Constraint::Coincident(free, link.points[1])]);
    let accepted = propose_it(&sketch, &edit).unwrap();
    assert!(at(&accepted.sketch, free).distance(DVec2::new(10.0, 0.0)) < 1e-9);
    assert_eq!(at(&accepted.sketch, link.points[1]), DVec2::new(10.0, 0.0));
    assert!(accepted.analysis.fixed.contains(&link.curves[0]));
    // Moving it is refused, and a drag of the user's point can't take it.
    let moved = SketchEdit::Move {
        points: vec![(link.points[0], DVec2::new(5.0, 5.0))],
        radii: Vec::new(),
    };
    assert_eq!(
        propose_it(&sketch, &moved),
        Err(Rejected::Edit(EditError::Linked(link.points[0])))
    );
    let mut session = crate::DragSession::new(accepted.sketch.clone(), &DESIGN);
    let dragged = session
        .step(
            vec![(free, DVec2::new(4.0, 4.0))],
            Vec::new(),
            &crate::Budget::default(),
        )
        .unwrap();
    assert_eq!(at(dragged, link.points[1]), DVec2::new(10.0, 0.0));
}

#[test]
fn edits_to_link_geometry_are_refused() {
    let (sketch, id) = linked(
        LinkKind::Project,
        &segment(DVec2::ZERO, DVec2::new(10.0, 0.0)),
    );
    let link = sketch.link(id).unwrap().clone();
    let refused = |edit: SketchEdit| edit.apply(&sketch, &DESIGN).unwrap_err();
    assert_eq!(
        refused(SketchEdit::Delete(vec![link.points[0]])),
        EditError::Linked(link.points[0])
    );
    assert!(matches!(
        refused(SketchEdit::SetConstruction {
            ids: vec![link.curves[0]],
            construction: true,
        }),
        EditError::Linked(_)
    ));
    assert!(matches!(
        refused(SketchEdit::Trim {
            curve: link.curves[0],
            near: DVec2::new(5.0, 0.0),
        }),
        EditError::Linked(_)
    ));
    assert!(
        EditError::Linked(id)
            .to_string()
            .contains("remove its link")
    );
    // Deleting the link takes what it made.
    let deleted = SketchEdit::Delete(vec![id])
        .apply(&sketch, &DESIGN)
        .unwrap();
    assert!(deleted.links.is_empty());
    assert!(deleted.points.is_empty() && deleted.curves.is_empty());
}

#[test]
fn a_links_curves_count_for_profiles_only_when_it_says() {
    let (sketch, id) = linked(
        LinkKind::Project,
        &segment(DVec2::ZERO, DVec2::new(10.0, 0.0)),
    );
    let counted = SketchEdit::SetLinkProfiles {
        link: id,
        profiles: false,
    }
    .apply(&sketch, &DESIGN)
    .unwrap();
    let link = counted.link(id).unwrap();
    assert!(!link.profiles);
    // Construction unless it counts for profiles.
    assert!(counted.curve(link.curves[0]).unwrap().construction);
    assert_eq!(counted.check(&DESIGN), Ok(()));
}

#[test]
fn check_refuses_links_naming_what_isnt_theirs() {
    let (sketch, id) = linked(
        LinkKind::Project,
        &segment(DVec2::ZERO, DVec2::new(10.0, 0.0)),
    );
    let link = sketch.link(id).unwrap().clone();
    // A user's curve made of a link's point.
    let mut shared = sketch.clone();
    let lone = point(&mut shared, 3.0, 3.0);
    line(&mut shared, link.points[0], lone);
    assert_eq!(shared.check(&DESIGN), Err(SketchError::Link(id)));
    // A link curve left out of profiles the link counts it for.
    let mut counted = sketch.clone();
    counted.curve_mut(link.curves[0]).unwrap().construction = true;
    assert_eq!(counted.check(&DESIGN), Err(SketchError::Link(id)));
    // A link naming a point that isn't there.
    let mut missing = sketch.clone();
    missing.points.clear();
    missing.curves.clear();
    assert!(missing.check(&DESIGN).is_err());
}

/// Mirroring a link's line that ends on the mirror line copies it with a
/// new point there, held symmetric, rather than sharing the link's point
/// (no curve of the user's may be made of one).
#[test]
fn mirroring_link_geometry_touching_the_axis_makes_its_own_point() {
    let (mut sketch, id) = linked(
        LinkKind::Project,
        &segment(DVec2::ZERO, DVec2::new(5.0, 0.0)),
    );
    let link = sketch.link(id).unwrap().clone();
    let a = point(&mut sketch, 0.0, -10.0);
    let b = point(&mut sketch, 0.0, 10.0);
    let axis = line(&mut sketch, a, b);
    let edit = SketchEdit::Mirror {
        ids: vec![link.curves[0]],
        about: axis,
    };
    let mirrored = edit.apply(&sketch, &DESIGN).unwrap();
    assert_eq!(mirrored.check(&DESIGN), Ok(()));
    let copy = mirrored.curves.last().unwrap();
    assert_ne!(copy.id, link.curves[0]);
    assert!(copy.curve.points().all(|p| !mirrored.is_linked(p)));
    let accepted = propose_it(&sketch, &edit).unwrap();
    let Curve::Line { start, end } = accepted.sketch.curves.last().unwrap().curve else {
        panic!("not a line");
    };
    let ends = [at(&accepted.sketch, start), at(&accepted.sketch, end)];
    assert!(ends.iter().any(|p| p.distance(DVec2::ZERO) < 1e-9));
    assert!(
        ends.iter()
            .any(|p| p.distance(DVec2::new(-5.0, 0.0)) < 1e-9)
    );
}

/// Trimming the user's line back to where a link's line ends on it ends
/// it at a new point coincident with the link's, not at the link's point
/// itself; extending one to there likewise.
#[test]
fn trimming_or_extending_to_a_links_end_makes_a_coincident_point() {
    let (mut sketch, id) = linked(
        LinkKind::Project,
        &segment(DVec2::new(5.0, 0.0), DVec2::new(5.0, 10.0)),
    );
    let link = sketch.link(id).unwrap().clone();
    let a = point(&mut sketch, 0.0, 0.0);
    let b = point(&mut sketch, 10.0, 0.0);
    let own = line(&mut sketch, a, b);
    let trim = SketchEdit::Trim {
        curve: own,
        near: DVec2::new(8.0, 0.0),
    };
    let trimmed = trim.apply(&sketch, &DESIGN).unwrap();
    assert_eq!(trimmed.check(&DESIGN), Ok(()));
    let Curve::Line { start, end } = trimmed.curve(own).unwrap().curve else {
        panic!("not a line");
    };
    assert_eq!(start, a);
    assert!(!trimmed.is_linked(end));
    assert!(
        trimmed
            .constraints
            .iter()
            .any(|entry| { entry.constraint == Constraint::Coincident(end, link.points[0]) })
    );
    let accepted = propose_it(&sketch, &trim).unwrap();
    assert!(at(&accepted.sketch, end).distance(DVec2::new(5.0, 0.0)) < 1e-9);

    // A line short of the link's end, extended to it.
    let mut short = Sketch::clone(&sketch);
    short.delete(&[own]);
    let c = point(&mut short, 0.0, 0.0);
    let d = point(&mut short, 2.0, 0.0);
    let short_line = line(&mut short, c, d);
    let extend = SketchEdit::Extend {
        curve: short_line,
        end: d,
    };
    let extended = extend.apply(&short, &DESIGN).unwrap();
    assert_eq!(extended.check(&DESIGN), Ok(()));
    let Curve::Line { end, .. } = extended.curve(short_line).unwrap().curve else {
        panic!("not a line");
    };
    assert!(!extended.is_linked(end));
    assert!(at(&extended, end).distance(DVec2::new(5.0, 0.0)) < 1e-9);
}

#[test]
fn a_shape_fits_lines_circles_arcs_and_splines() {
    let line = SampledChain {
        places: (0..=10)
            .map(|i| DVec2::new(i as f64, 2.0 * i as f64))
            .collect(),
        closed: false,
    };
    let shape = LinkShape::fit(&[line], 1e-9, 1e-3).unwrap();
    assert_eq!(shape.points, vec![DVec2::ZERO, DVec2::new(10.0, 20.0)]);
    assert!(matches!(shape.curves[..], [Curve::Line { .. }]));

    let circle = |from: f64, to: f64, count: usize| -> Vec<DVec2> {
        (0..count)
            .map(|i| {
                let t = from + (to - from) * i as f64 / (count - 1) as f64;
                DVec2::new(3.0, 4.0) + angle::from_angle(t) * 5.0
            })
            .collect()
    };
    let whole = SampledChain {
        places: circle(0.0, std::f64::consts::TAU, 65)[..64].to_vec(),
        closed: true,
    };
    let shape = LinkShape::fit(&[whole], 1e-9, 1e-3).unwrap();
    let [Curve::Circle { radius, .. }] = shape.curves[..] else {
        panic!("{shape:?}");
    };
    assert!((radius - 5.0).abs() < 1e-9);
    assert!(shape.points[0].distance(DVec2::new(3.0, 4.0)) < 1e-9);

    // Clockwise samples make a counter-clockwise arc from the last.
    let backwards = SampledChain {
        places: circle(1.0, 0.0, 20),
        closed: false,
    };
    let shape = LinkShape::fit(&[backwards], 1e-9, 1e-3).unwrap();
    let [Curve::Arc { start, end, .. }] = shape.curves[..] else {
        panic!("{shape:?}");
    };
    let start = shape.points[start.0 as usize];
    let end = shape.points[end.0 as usize];
    assert!(start.distance(DVec2::new(3.0, 4.0) + DVec2::X * 5.0) < 1e-9);
    assert!(end.distance(DVec2::new(3.0, 4.0) + angle::from_angle(1.0) * 5.0) < 1e-9);

    // An ellipse: a spline within the tolerance.
    let ellipse = SampledChain {
        places: (0..200)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 200.0;
                DVec2::new(6.0 * angle::cos(t), 2.0 * angle::sin(t))
            })
            .collect(),
        closed: true,
    };
    let shape = LinkShape::fit(&[ellipse], 1e-9, 1e-3).unwrap();
    let [Curve::Spline(spline)] = &shape.curves[..] else {
        panic!("{shape:?}");
    };
    assert!(spline.closed && spline.points.len() <= MAX_SPLINE_POINTS);
    assert!(shape.fits(DESIGN.max));

    // Seen edge on, a circle is the line it covers.
    let flat = SampledChain {
        places: circle(0.0, std::f64::consts::TAU, 41)[..40]
            .iter()
            .map(|p| DVec2::new(p.x, 0.0))
            .collect(),
        closed: true,
    };
    let shape = LinkShape::fit(&[flat], 1e-9, 1e-3).unwrap();
    assert!(matches!(shape.curves[..], [Curve::Line { .. }]));
    let span = shape.points[0].distance(shape.points[1]);
    assert!((span - 10.0).abs() < 1e-6);
}

/// A line a fillet cuts back projects as what's left of it, as it's
/// drawn and as profiles take it, not on to the corner the fillet
/// rounds.
#[test]
fn a_line_cut_back_by_a_fillet_projects_as_what_is_left() {
    let mut sketch = Sketch::default();
    let corner = point(&mut sketch, 0.0, 0.0);
    let (along, up) = (point(&mut sketch, 20.0, 0.0), point(&mut sketch, 0.0, 10.0));
    let a = line(&mut sketch, corner, along);
    let b = line(&mut sketch, up, corner);
    let filleted = SketchEdit::Fillet {
        at: corner,
        lines: [a, b],
        radius: crate::testing::value("2", &crate::Measure::Radius(Id::ORIGIN)),
    }
    .apply(&sketch, &DESIGN)
    .unwrap();
    let [kept_a, kept_b] = filleted.cut_back()[&a];
    assert!(kept_a > 0.0 && kept_b == 1.0);
    let projected = filleted
        .project_item(a, &DAffine2::IDENTITY, 1e-9, 1e-3)
        .unwrap();
    let [Curve::Line { start, end }] = projected.curves[..] else {
        panic!("{projected:?}");
    };
    let from = projected.points[start.0 as usize];
    let to = projected.points[end.0 as usize];
    assert!(from.distance(DVec2::new(2.0, 0.0)) < 1e-9, "{from}");
    assert!(to.distance(DVec2::new(20.0, 0.0)) < 1e-9, "{to}");
}

#[test]
fn projecting_a_sketchs_items() {
    let mut sketch = Sketch::default();
    let c = point(&mut sketch, 1.0, 1.0);
    let circle = crate::testing::circle(&mut sketch, c, 2.0);
    let (s, e) = (point(&mut sketch, 3.0, 1.0), point(&mut sketch, 1.0, 3.0));
    let arc = crate::testing::arc(&mut sketch, c, s, e);
    let (a, b) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 0.0, 5.0));
    let vertical = line(&mut sketch, a, b);
    let (spline, _) =
        crate::testing::spline(&mut sketch, &[(0.0, 0.0), (2.0, 3.0), (5.0, 1.0)], false);

    // A parallel plane, mirrored and shifted: circles stay circles, arcs
    // turn the other way round.
    let mirror = DAffine2::from_mat2_translation(
        DMat2::from_diagonal(DVec2::new(-1.0, 1.0)),
        DVec2::new(10.0, 0.0),
    );
    let projected = sketch.project_item(circle, &mirror, 1e-9, 1e-3).unwrap();
    assert_eq!(projected.points, vec![DVec2::new(9.0, 1.0)]);
    assert!(matches!(
        projected.curves[..],
        [Curve::Circle { radius: 2.0, .. }]
    ));
    let projected = sketch.project_item(arc, &mirror, 1e-9, 1e-3).unwrap();
    let [Curve::Arc { start, .. }] = projected.curves[..] else {
        panic!("{projected:?}");
    };
    assert_eq!(projected.points[start.0 as usize], DVec2::new(9.0, 3.0));

    // A plane square to this one along x: the circle is a line, the
    // vertical line a point.
    let edge_on =
        DAffine2::from_mat2_translation(DMat2::from_cols(DVec2::X, DVec2::ZERO), DVec2::ZERO);
    let projected = sketch.project_item(circle, &edge_on, 1e-9, 1e-3).unwrap();
    assert!(matches!(projected.curves[..], [Curve::Line { .. }]));
    let projected = sketch.project_item(vertical, &edge_on, 1e-9, 1e-3).unwrap();
    assert_eq!(projected.points, vec![DVec2::ZERO]);
    assert!(projected.curves.is_empty());

    // A tilted plane: the circle an ellipse, held by a spline; a spline
    // by its control points mapped, exactly.
    let tilted =
        DAffine2::from_mat2_translation(DMat2::from_cols(DVec2::X, DVec2::Y * 0.5), DVec2::ZERO);
    let projected = sketch.project_item(circle, &tilted, 1e-9, 1e-3).unwrap();
    assert!(matches!(projected.curves[..], [Curve::Spline(_)]));
    let projected = sketch.project_item(spline, &tilted, 1e-9, 1e-3).unwrap();
    let [Curve::Spline(mapped)] = &projected.curves[..] else {
        panic!("{projected:?}");
    };
    assert_eq!(mapped.kind, SplineKind::Control);
    let shape = sketch.spline_shape(sketch.spline(spline).unwrap()).unwrap();
    let image = crate::BSpline::new(&mapped.knots, projected.points.clone(), false).unwrap();
    for t in [0.0, 0.3, 0.7, 1.0] {
        let p = shape.point(t);
        assert!(image.point(t).distance(DVec2::new(p.x, p.y * 0.5)) < 1e-9);
    }
    assert_eq!(
        sketch.project_item(Id(9999), &tilted, 1e-9, 1e-3),
        Err(ProjectError::Missing)
    );
}
