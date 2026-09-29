use std::f64::consts::{FRAC_PI_4, PI};

use glam::DVec2;

use super::*;
use crate::testing::{
    DESIGN, at, circle, constrain, dimension, line, near, point, propose_it, value,
};
use crate::{Budget, Constraint, Kind, SketchEdit, SketchError, analyse};

/// An L: the corner at the origin, a line along x to (20, 0) from it and
/// one down y from (0, 10) to it: the corner, and the lines.
fn l_shape() -> (Sketch, Id, [Id; 2]) {
    let mut sketch = Sketch::default();
    let corner = point(&mut sketch, 0.0, 0.0);
    let (along, up) = (point(&mut sketch, 20.0, 0.0), point(&mut sketch, 0.0, 10.0));
    let a = line(&mut sketch, corner, along);
    let b = line(&mut sketch, up, corner);
    (sketch, corner, [a, b])
}

fn fillet(at: Id, lines: [Id; 2], radius: &str) -> SketchEdit {
    SketchEdit::Fillet {
        at,
        lines,
        radius: value(radius, &Measure::Radius(Id::ORIGIN)),
    }
}

fn length(text: &str) -> Value {
    value(text, &Measure::Length(Id::ORIGIN))
}

fn chamfer(at: Id, lines: [Id; 2], setback: Setback) -> SketchEdit {
    SketchEdit::Chamfer { at, lines, setback }
}

/// The last curve, as a fillet's or chamfer's points: its start, its end
/// and a fillet's centre.
fn made(sketch: &Sketch) -> (&CurveEntry, [DVec2; 2], Option<DVec2>) {
    let entry = sketch.curves.last().unwrap();
    let place = |id| at(sketch, id);
    match entry.curve {
        Curve::Arc { center, start, end } => {
            (entry, [place(start), place(end)], Some(place(center)))
        }
        Curve::Line { start, end } => (entry, [place(start), place(end)], None),
        Curve::Circle { .. } | Curve::Spline(_) => panic!("no corner is a circle or a spline"),
    }
}

#[test]
fn a_fillet_rounds_a_corner_tangent_to_both_lines() {
    let (sketch, corner, [a, b]) = l_shape();
    assert_eq!(analyse(&sketch).freedom, 6);
    let accepted = propose_it(&sketch, &fillet(corner, [a, b], "3")).unwrap();
    let filleted = &accepted.sketch;
    let (entry, [start, end], center) = made(filleted);
    assert_eq!(entry.name(), "Fillet 1");
    assert_eq!(entry.curve.kind(), Kind::Arc);
    // Counter-clockwise the short way: from the end on `b` to that on `a`.
    assert_eq!(
        entry.corner,
        Some(Corner {
            a: b,
            b: a,
            at: corner,
            equal: false
        })
    );
    assert!(near(center.unwrap(), DVec2::new(3.0, 3.0)));
    assert!(near(start, DVec2::new(0.0, 3.0)) && near(end, DVec2::new(3.0, 0.0)));
    // Held by its radius: no more freedom than the lines had.
    let radius = filleted.dimensions.last().unwrap();
    assert_eq!(radius.dimension.measure, Measure::Radius(entry.id));
    assert!(radius.dimension.driving);
    assert_eq!(accepted.analysis.freedom, 6);
    assert!(accepted.analysis.redundant.is_empty() && accepted.analysis.solved);
    // The lines stay whole.
    assert_eq!(filleted.line(a), sketch.line(a));
    assert_eq!(filleted.line(b), sketch.line(b));

    // A new radius moves its ends along the lines, still tangent.
    let mut fixed = filleted.clone();
    for point in &sketch.points {
        constrain(&mut fixed, Constraint::Fix(point.id));
    }
    let filleted = &fixed;
    let id = radius.id;
    let set = SketchEdit::SetDimension {
        id,
        value: value("5", &Measure::Radius(entry.id)),
    };
    let resized = propose_it(filleted, &set).unwrap().sketch;
    let (_, [start, end], center) = made(&resized);
    assert!(center.unwrap().abs_diff_eq(DVec2::new(5.0, 5.0), 1e-7));
    assert!(start.abs_diff_eq(DVec2::new(0.0, 5.0), 1e-7));
    assert!(end.abs_diff_eq(DVec2::new(5.0, 0.0), 1e-7));
}

#[test]
fn a_fillet_runs_counter_clockwise_whichever_way_the_corner_turns() {
    for lines in [false, true] {
        for flip in [1.0, -1.0] {
            let mut sketch = Sketch::default();
            let corner = point(&mut sketch, 1.0, 2.0);
            let along = point(&mut sketch, 15.0, 2.0 + 3.0 * flip);
            let across = point(&mut sketch, -2.0, 2.0 + 12.0 * flip);
            let a = line(&mut sketch, along, corner);
            let b = line(&mut sketch, corner, across);
            let pair = if lines { [a, b] } else { [b, a] };
            let filleted = propose_it(&sketch, &fillet(corner, pair, "2"))
                .unwrap()
                .sketch;
            let (entry, [start, end], center) = made(&filleted);
            let center = center.unwrap();
            // The short way round, inside the corner.
            assert!(crate::arc_sweep(start - center, end - center) < PI);
            let corner_at = at(&filleted, corner);
            assert!((center - corner_at).length() > (start - corner_at).length());
            let corner = entry.corner.unwrap();
            let on = |line: Id, place: DVec2| {
                let (s, e) = filleted.line(line).unwrap();
                (e - s).perp_dot(place - s).abs() < 1e-9 * (e - s).length()
            };
            assert!(on(corner.a, start) && on(corner.b, end));
        }
    }
}

#[test]
fn a_fillet_on_a_fixed_rectangle_is_fully_constrained() {
    let (mut sketch, corners, [bottom, right, top, left]) = crate::testing::quadrilateral();
    for constraint in [
        Constraint::Horizontal(bottom),
        Constraint::Horizontal(top),
        Constraint::Vertical(left),
        Constraint::Vertical(right),
        Constraint::Fix(corners[0]),
    ] {
        constrain(&mut sketch, constraint);
    }
    dimension(&mut sketch, Measure::Length(bottom), "10");
    dimension(&mut sketch, Measure::Length(left), "6");
    let sketch = crate::solve(&sketch, &crate::Goal::Settle, &Budget::default())
        .unwrap()
        .sketch;
    assert_eq!(analyse(&sketch).freedom, 0);
    let accepted = propose_it(&sketch, &fillet(corners[2], [right, top], "2")).unwrap();
    assert_eq!(accepted.analysis.freedom, 0);
    assert!(accepted.analysis.redundant.is_empty());
    let (_, _, center) = made(&accepted.sketch);
    assert!(center.unwrap().abs_diff_eq(DVec2::new(8.0, 4.0), 1e-7));
}

#[test]
fn chamfers_cut_by_equal_distances_two_or_a_distance_and_an_angle() {
    let (sketch, corner, [a, b]) = l_shape();

    let equal = propose_it(
        &sketch,
        &chamfer(corner, [a, b], Setback::Equal(length("2"))),
    )
    .unwrap();
    let (entry, [start, end], center) = made(&equal.sketch);
    assert_eq!(entry.name(), "Chamfer 1");
    assert_eq!(entry.curve.kind(), Kind::Line);
    assert!(center.is_none() && entry.corner.unwrap().equal);
    assert!(near(start, DVec2::new(2.0, 0.0)) && near(end, DVec2::new(0.0, 2.0)));
    let dimensions: Vec<_> = equal
        .sketch
        .dimensions
        .iter()
        .map(|e| &e.dimension.measure)
        .collect();
    let (from, to) = match entry.curve {
        Curve::Line { start, end } => (start, end),
        _ => unreachable!(),
    };
    assert_eq!(dimensions, [&Measure::Distance(corner, from)]);
    assert_eq!(equal.analysis.freedom, 6);
    assert!(equal.analysis.redundant.is_empty());
    // Held equal: a new distance moves both ends.
    let mut fixed = equal.sketch.clone();
    for point in &sketch.points {
        constrain(&mut fixed, Constraint::Fix(point.id));
    }
    let id = equal.sketch.dimensions[0].id;
    let set = SketchEdit::SetDimension {
        id,
        value: length("4"),
    };
    let moved = propose_it(&fixed, &set).unwrap().sketch;
    assert!(at(&moved, to).abs_diff_eq(DVec2::new(0.0, 4.0), 1e-7));

    let two = SketchEdit::Chamfer {
        at: corner,
        lines: [b, a],
        setback: Setback::Two(length("3"), length("1")),
    };
    let two = propose_it(&sketch, &two).unwrap();
    let (entry, [start, end], _) = made(&two.sketch);
    // From the first line to the second.
    assert!(near(start, DVec2::new(0.0, 3.0)) && near(end, DVec2::new(1.0, 0.0)));
    assert!(!entry.corner.unwrap().equal);
    assert_eq!(two.sketch.dimensions.len(), 2);
    assert_eq!(two.analysis.freedom, 6);
    assert!(two.analysis.redundant.is_empty());

    // At 30° to the line along x, from 2 along it: the end on y is
    // 2 tan 30° up.
    let angle = value("30", &Measure::Angle(Id::ORIGIN, Id::ORIGIN));
    let angled = chamfer(corner, [a, b], Setback::Angle(length("2"), angle));
    let angled = propose_it(&sketch, &angled).unwrap();
    let (entry, [start, end], _) = made(&angled.sketch);
    assert!(near(start, DVec2::new(2.0, 0.0)));
    assert!(end.abs_diff_eq(DVec2::new(0.0, 2.0 * (PI / 6.0).tan()), 1e-9));
    let dimension = &angled.sketch.dimensions.last().unwrap().dimension;
    assert!(matches!(dimension.measure, Measure::Angle(..)));
    let measured = angled.sketch.measure(&dimension.measure, dimension.side);
    assert!((measured.unwrap() - PI / 6.0).abs() < 1e-9);
    assert_eq!(angled.analysis.freedom, 6);
    assert!(angled.analysis.redundant.is_empty());
    assert!(!entry.corner.unwrap().equal);
}

#[test]
fn a_new_fillet_or_chamfer_replaces_the_one_on_its_corner() {
    let (sketch, corner, [a, b]) = l_shape();
    let first = propose_it(&sketch, &fillet(corner, [a, b], "3"))
        .unwrap()
        .sketch;
    let second = propose_it(&first, &fillet(corner, [b, a], "4"))
        .unwrap()
        .sketch;
    let corners: Vec<_> = second
        .curves
        .iter()
        .filter(|e| e.corner.is_some())
        .collect();
    assert_eq!(corners.len(), 1);
    // The first's points and radius went with it.
    assert_eq!(second.points.len(), 6);
    assert_eq!(second.dimensions.len(), 1);
    let (_, _, center) = made(&second);
    assert!(near(center.unwrap(), DVec2::new(4.0, 4.0)));

    let cut = chamfer(corner, [a, b], Setback::Equal(length("1")));
    let third = propose_it(&second, &cut).unwrap().sketch;
    let corners: Vec<_> = third.curves.iter().filter(|e| e.corner.is_some()).collect();
    assert_eq!(corners.len(), 1);
    assert_eq!(corners[0].noun(), "Chamfer");
    assert_eq!(third.points.len(), 5);
}

#[test]
fn a_fillet_needs_a_corner_with_room() {
    let (mut sketch, corner, [a, b]) = l_shape();
    let apply = |sketch: &Sketch, edit: &SketchEdit| edit.apply(sketch, &DESIGN).err();
    // The line up is 10 long: a right angle's fillet reaches along it as
    // far as its radius.
    assert_eq!(
        apply(&sketch, &fillet(corner, [a, b], "11")),
        Some(EditError::NoRoom)
    );
    assert_eq!(
        apply(&sketch, &fillet(corner, [a, a], "1")),
        Some(EditError::NoCorner)
    );
    let far = sketch.points[1].id;
    assert_eq!(
        apply(&sketch, &fillet(far, [a, b], "1")),
        Some(EditError::NoCorner)
    );
    let cut = chamfer(corner, [a, b], Setback::Two(length("1"), length("11")));
    assert_eq!(apply(&sketch, &cut), Some(EditError::NoRoom));
    // Past half a turn with the corner's right angle, the angle reaches
    // nowhere.
    let angle = value("100", &Measure::Angle(Id::ORIGIN, Id::ORIGIN));
    let cut = chamfer(corner, [a, b], Setback::Angle(length("1"), angle));
    assert_eq!(apply(&sketch, &cut), Some(EditError::NoRoom));
    // Lines running on from each other make no corner.
    let on = point(&mut sketch, -5.0, 0.0);
    let straight = line(&mut sketch, on, corner);
    assert_eq!(
        apply(&sketch, &fillet(corner, [a, straight], "1")),
        Some(EditError::NoCorner)
    );
    // Nor does a chamfer.
    let chamfered = propose_it(
        &sketch,
        &chamfer(corner, [a, b], Setback::Equal(length("1"))),
    )
    .unwrap()
    .sketch;
    let cut = chamfered
        .curves
        .iter()
        .find(|e| e.corner.is_some())
        .unwrap()
        .id;
    let (start, _) = match chamfered.curve(cut).unwrap().curve {
        Curve::Line { start, end } => (start, end),
        _ => unreachable!(),
    };
    assert_eq!(
        apply(&chamfered, &fillet(start, [cut, a], "0.1")),
        Some(EditError::NoCorner)
    );
}

#[test]
fn deleting_a_fillet_gives_the_corner_back_and_a_line_takes_its_corners() {
    let (sketch, corner, [a, b]) = l_shape();
    let filleted = propose_it(&sketch, &fillet(corner, [a, b], "3"))
        .unwrap()
        .sketch;
    let id = filleted.curves.last().unwrap().id;
    assert!(!filleted.cut_back().is_empty());

    let mut deleted = filleted.clone();
    deleted.delete(&[id]);
    // Its points and radius go, and nothing else changes.
    assert_eq!(deleted.points, sketch.points);
    assert_eq!(deleted.curves, sketch.curves);
    assert!(deleted.dimensions.is_empty());
    assert!(deleted.cut_back().is_empty());
    assert_eq!(deleted.check(&DESIGN), Ok(()));

    let mut deleted = filleted.clone();
    deleted.delete(&[b]);
    assert_eq!(deleted.curves.len(), 1);
    assert_eq!(deleted.curves[0].id, a);
    assert!(deleted.dimensions.is_empty());
    assert_eq!(deleted.check(&DESIGN), Ok(()));

    // So does its corner's point, with both lines.
    let mut deleted = filleted;
    deleted.delete(&[corner]);
    assert!(deleted.curves.is_empty());
    assert_eq!(deleted.check(&DESIGN), Ok(()));
}

#[test]
fn cut_back_is_what_each_line_keeps() {
    let (sketch, corner, [a, b]) = l_shape();
    let filleted = propose_it(&sketch, &fillet(corner, [a, b], "4"))
        .unwrap()
        .sketch;
    let cut = filleted.cut_back();
    // `a` starts at the corner, `b` ends there.
    assert!((cut[&a][0] - 0.2).abs() < 1e-9 && cut[&a][1] == 1.0);
    assert!(cut[&b][0] == 0.0 && (cut[&b][1] - 0.6).abs() < 1e-9);

    let (kept, off) = crate::cut_line(DVec2::ZERO, DVec2::new(20.0, 0.0), cut[&a]);
    assert!(near(kept.unwrap()[0], DVec2::new(4.0, 0.0)));
    assert_eq!(off.len(), 1);
    assert!(near(off[0][0], DVec2::new(4.0, 0.0)) && off[0][1] == DVec2::ZERO);
    let (kept, off) = crate::cut_line(DVec2::ZERO, DVec2::X, [0.7, 0.3]);
    assert!(kept.is_none());
    assert_eq!(off, [[DVec2::ZERO, DVec2::X]]);
    let (kept, off) = crate::cut_line(DVec2::ZERO, DVec2::X, [0.0, 1.0]);
    assert_eq!((kept, off), (Some([DVec2::ZERO, DVec2::X]), Vec::new()));
}

#[test]
fn profiles_take_the_fillet_for_the_corner() {
    let (sketch, corners, [bottom, right, top, left]) = {
        let mut sketch = Sketch::default();
        let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)]
            .map(|(x, y)| point(&mut sketch, x, y));
        let lines = [0, 1, 2, 3].map(|i| line(&mut sketch, corners[i], corners[(i + 1) % 4]));
        (sketch, corners, lines)
    };
    let filleted = propose_it(&sketch, &fillet(corners[2], [right, top], "2"))
        .unwrap()
        .sketch;
    let chamfered = propose_it(
        &filleted,
        &chamfer(corners[0], [left, bottom], Setback::Equal(length("1"))),
    )
    .unwrap()
    .sketch;
    let profiles = chamfered.profiles().unwrap();
    assert_eq!(profiles.regions.len(), 1);
    let region = &profiles.regions[0];
    let expected = 60.0 - (4.0 - PI) - 0.5;
    assert!((region.area - expected).abs() < 1e-9, "{}", region.area);
    let fillet = chamfered
        .curves
        .iter()
        .find(|e| e.noun() == "Fillet")
        .unwrap()
        .id;
    let chamfer = chamfered
        .curves
        .iter()
        .find(|e| e.noun() == "Chamfer")
        .unwrap()
        .id;
    let used: Vec<Id> = region.outer.iter().map(|piece| piece.curve).collect();
    assert_eq!(used.len(), 6);
    assert!(used.contains(&fillet) && used.contains(&chamfer));
    // The pieces cut off are no part of it: each line's once.
    for line in [bottom, right, top, left] {
        assert_eq!(used.iter().filter(|&&id| id == line).count(), 1);
    }
    assert!(profiles.open_ends.is_empty());
}

#[test]
fn check_refuses_what_is_no_corner() {
    let (sketch, corner, [a, b]) = l_shape();
    let filleted = propose_it(&sketch, &fillet(corner, [a, b], "3"))
        .unwrap()
        .sketch;
    let fillet = filleted.curves.last().unwrap().id;
    // The fillet's corner changed so.
    let broken = |change: &dyn Fn(&mut Corner)| {
        let mut sketch = filleted.clone();
        change(sketch.curve_mut(fillet).unwrap().corner.as_mut().unwrap());
        sketch.check(&DESIGN)
    };
    assert_eq!(broken(&|_| {}), Ok(()));
    assert_eq!(
        broken(&|c| c.equal = true),
        Err(SketchError::Corner(fillet))
    );
    let far = sketch.points[1].id;
    assert_eq!(broken(&|c| c.at = far), Err(SketchError::Corner(fillet)));
    assert_eq!(
        broken(&|c| c.a = c.b),
        Err(SketchError::Repeated {
            from: fillet,
            to: a
        })
    );
    assert_eq!(
        broken(&|c| c.a = Id::X_AXIS),
        Err(SketchError::Corner(fillet))
    );
    assert!(matches!(
        broken(&|c| c.a = corner),
        Err(SketchError::Reference { .. })
    ));
    // A circle is no fillet.
    let mut round = sketch.clone();
    let center = point(&mut round, 5.0, 5.0);
    let id = circle(&mut round, center, 1.0);
    round.curve_mut(id).unwrap().corner = Some(Corner {
        a,
        b,
        at: corner,
        equal: false,
    });
    assert_eq!(round.check(&DESIGN), Err(SketchError::Corner(id)));
    // Two on one corner.
    let mut twice = filleted.clone();
    let copy = twice.curves.last().unwrap().clone();
    let id = twice.add_curve(copy.curve.clone(), false).unwrap();
    twice.curve_mut(id).unwrap().corner = copy.corner;
    assert_eq!(twice.check(&DESIGN), Err(SketchError::Corner(id)));
    // Nor a corner of its own point.
    let mut own = filleted.clone();
    if let Curve::Arc { start, .. } = &mut own.curve_mut(fillet).unwrap().curve {
        *start = corner;
    }
    assert_eq!(own.check(&DESIGN), Err(SketchError::Corner(fillet)));
}

#[test]
fn mirroring_a_fillet_with_its_lines_copies_the_corner() {
    let mut sketch = Sketch::default();
    let corner = point(&mut sketch, 5.0, 0.0);
    let (along, up) = (point(&mut sketch, 15.0, 0.0), point(&mut sketch, 5.0, 10.0));
    let a = line(&mut sketch, corner, along);
    let b = line(&mut sketch, up, corner);
    let filleted = propose_it(&sketch, &fillet(corner, [a, b], "2")).unwrap();
    let fillet = filleted.sketch.curves.last().unwrap().id;
    let before = filleted.analysis.freedom;
    let mirror = SketchEdit::Mirror {
        ids: vec![a, b, fillet],
        about: Id::Y_AXIS,
    };
    let accepted = propose_it(&filleted.sketch, &mirror).unwrap();
    let mirrored = &accepted.sketch;
    let (entry, [start, end], center) = made(mirrored);
    assert_eq!(entry.name(), "Fillet 2");
    let copy = entry.corner.unwrap();
    assert!(near(center.unwrap(), DVec2::new(-7.0, 2.0)));
    assert!(crate::arc_sweep(start - center.unwrap(), end - center.unwrap()) < PI);
    assert_eq!(mirrored.check(&DESIGN), Ok(()));
    let on = |line: Id, place: DVec2| {
        let (s, e) = mirrored.line(line).unwrap();
        (e - s).perp_dot(place - s).abs() < 1e-9
    };
    assert!(on(copy.a, start) && on(copy.b, end));
    // Nothing more to move, nothing said twice.
    assert_eq!(accepted.analysis.freedom, before);
    assert!(accepted.analysis.redundant.is_empty());
    assert_eq!(mirrored.cut_back().len(), 4);
}

#[test]
fn trimming_and_extending_lines_drop_corners_they_break() {
    let (mut sketch, corner, [a, b]) = l_shape();
    // A line across `a` at x = 10, and one past its end at x = 30.
    let (low, high) = (
        point(&mut sketch, 10.0, -5.0),
        point(&mut sketch, 10.0, 5.0),
    );
    line(&mut sketch, low, high);
    let (low, high) = (
        point(&mut sketch, 30.0, -5.0),
        point(&mut sketch, 30.0, 5.0),
    );
    line(&mut sketch, low, high);
    let filleted = propose_it(&sketch, &fillet(corner, [a, b], "3"))
        .unwrap()
        .sketch;
    let fillet = filleted.curves.last().unwrap().id;
    let corners = |sketch: &Sketch| sketch.curves.iter().filter(|e| e.corner.is_some()).count();

    // Trimmed off beyond the crossing, its corner stays.
    let trim = SketchEdit::Trim {
        curve: a,
        near: DVec2::new(15.0, 0.0),
    };
    let trimmed = trim.apply(&filleted, &DESIGN).unwrap();
    assert_eq!(corners(&trimmed), 1);
    // Trimmed through the middle, the corner goes with the part keeping
    // its end.
    let across = {
        let mut sketch = filleted.clone();
        let (low, high) = (point(&mut sketch, 6.0, -5.0), point(&mut sketch, 6.0, 5.0));
        line(&mut sketch, low, high);
        sketch
    };
    let trim = SketchEdit::Trim {
        curve: a,
        near: DVec2::new(8.0, 0.0),
    };
    let trimmed = trim.apply(&across, &DESIGN).unwrap();
    assert_eq!(corners(&trimmed), 1);
    let kept = trimmed.curve(fillet).unwrap().corner.unwrap();
    assert!(kept.a == a || kept.b == a);
    // Trimmed at the corner, it goes, and the fillet with it.
    let trim = SketchEdit::Trim {
        curve: b,
        near: DVec2::new(0.0, 1.0),
    };
    let trimmed = trim.apply(&filleted, &DESIGN).unwrap();
    assert_eq!(corners(&trimmed), 0);
    assert_eq!(trimmed.check(&DESIGN), Ok(()));
    // A fillet trimmed goes whole.
    let trim = SketchEdit::Trim {
        curve: fillet,
        near: DVec2::new(1.0, 1.0),
    };
    let trimmed = trim.apply(&filleted, &DESIGN).unwrap();
    assert_eq!(trimmed.curves, sketch.curves);

    // Extended from the corner, the line no longer ends there.
    let extend = SketchEdit::Extend {
        curve: a,
        end: corner,
    };
    let extended = SketchEdit::apply(
        &extend,
        &{
            let mut sketch = filleted.clone();
            let (low, high) = (
                point(&mut sketch, -4.0, -5.0),
                point(&mut sketch, -4.0, 5.0),
            );
            line(&mut sketch, low, high);
            sketch
        },
        &DESIGN,
    )
    .unwrap();
    assert_eq!(corners(&extended), 0);
    // A fillet isn't extended.
    let (start, _) = match filleted.curve(fillet).unwrap().curve {
        Curve::Arc { start, end, .. } => (start, end),
        _ => unreachable!(),
    };
    let extend = SketchEdit::Extend {
        curve: fillet,
        end: start,
    };
    assert_eq!(
        extend.apply(&filleted, &DESIGN).err(),
        Some(EditError::Target(fillet))
    );
}

#[test]
fn corner_lines_pick_the_nearest_two_and_previews_follow_the_cursor() {
    let (mut sketch, corner, [a, b]) = l_shape();
    assert_eq!(
        sketch.corner_lines(corner, DVec2::new(5.0, 1.0)),
        Some([a, b])
    );
    assert_eq!(
        sketch.corner_lines(corner, DVec2::new(1.0, 5.0)),
        Some([b, a])
    );
    // Not where lines don't end, nor at a lone end.
    assert_eq!(sketch.corner_lines(sketch.points[1].id, DVec2::ZERO), None);
    // A third line, diagonal: the two nearest the cursor.
    let diagonal = point(&mut sketch, 5.0, 5.0);
    let c = line(&mut sketch, corner, diagonal);
    assert_eq!(
        sketch.corner_lines(corner, DVec2::new(5.0, 1.0)),
        Some([a, c])
    );

    let (sketch, corner, lines) = l_shape();
    // The middle of a right angle's fillet of radius r is r (√2 - 1)
    // in, along the bisector.
    let r = 3.0;
    let middle = DVec2::splat(r * (1.0 - FRAC_PI_4.sin()));
    let radius = sketch.fillet_through(corner, lines, middle).unwrap();
    assert!((radius - r).abs() < 1e-9);
    assert_eq!(
        sketch.fillet_through(corner, lines, DVec2::new(-1.0, -1.0)),
        None
    );
    let arc = sketch.fillet_preview(corner, lines, r).unwrap();
    assert!(near(arc.center, DVec2::splat(r)));
    // A chamfer through (1, 1) cuts 2 back along each.
    let d = sketch.chamfer_through(corner, lines, DVec2::ONE).unwrap();
    assert!((d - 2.0).abs() < 1e-9);
    let ends = sketch
        .chamfer_preview(corner, lines, &Setback::Equal(d))
        .unwrap();
    assert!(near(ends[0], DVec2::new(2.0, 0.0)) && near(ends[1], DVec2::new(0.0, 2.0)));
}

#[test]
fn trimming_a_line_between_a_crossing_and_its_corner_keeps_the_corner() {
    for chamfered in [false, true] {
        let (mut sketch, corner, [a, b]) = l_shape();
        let (low, high) = (
            point(&mut sketch, 10.0, -5.0),
            point(&mut sketch, 10.0, 5.0),
        );
        line(&mut sketch, low, high);
        let edit = if chamfered {
            chamfer(corner, [a, b], Setback::Equal(length("3")))
        } else {
            fillet(corner, [a, b], "3")
        };
        let cornered = propose_it(&sketch, &edit).unwrap().sketch;
        let id = cornered.curves.last().unwrap().id;
        // From where the fillet or chamfer meets `a`, at x = 3, to the
        // crossing, and from the crossing to there, `a` running from the
        // corner.
        for near in [DVec2::new(6.0, 0.0), DVec2::new(1.0, 0.0)] {
            let trim = SketchEdit::Trim { curve: a, near };
            let accepted = propose_it(&cornered, &trim).unwrap();
            let trimmed = &accepted.sketch;
            assert_eq!(trimmed.check(&DESIGN), Ok(()));
            let kept = trimmed.curve(id).is_some();
            // Only where the corner's point is still `a`'s end does the
            // fillet or chamfer stay.
            assert_eq!(kept, near.x > 3.0, "{near}");
            if kept {
                // `a` runs from the corner to x = 3, cut back all of it.
                let (start, end) = trimmed.line(a).unwrap();
                assert!(near_enough(start, DVec2::ZERO) && near_enough(end, DVec2::X * 3.0));
                let [from, to] = trimmed.cut_back()[&a];
                assert!(from >= to);
            }
        }
    }
}

fn near_enough(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-6
}

#[test]
fn mirroring_a_fillet_about_one_of_its_lines_copies_the_corner() {
    let (sketch, corner, [a, b]) = l_shape();
    let filleted = propose_it(&sketch, &fillet(corner, [a, b], "3")).unwrap();
    let fillet = filleted.sketch.curves.last().unwrap().id;
    let mirror = SketchEdit::Mirror {
        ids: vec![b, fillet],
        about: a,
    };
    let accepted = propose_it(&filleted.sketch, &mirror).unwrap();
    let mirrored = &accepted.sketch;
    let (entry, _, center) = made(mirrored);
    assert!(near(center.unwrap(), DVec2::new(3.0, -3.0)));
    let copy = entry.corner.expect("the copy is a fillet");
    assert_eq!(copy.at, corner);
    assert!(copy.a == a || copy.b == a);
    assert_eq!(mirrored.check(&DESIGN), Ok(()));
    // Both copies' lines cut back to the fillets.
    assert_eq!(mirrored.cut_back().len(), 3);
    assert_eq!(accepted.analysis.freedom, filleted.analysis.freedom);
    assert!(accepted.analysis.redundant.is_empty());
}

#[test]
fn a_mirrored_chamfer_made_equal_keeps_its_equation() {
    let mut sketch = Sketch::default();
    let corner = point(&mut sketch, 5.0, 0.0);
    let (along, up) = (point(&mut sketch, 15.0, 0.0), point(&mut sketch, 5.0, 10.0));
    let a = line(&mut sketch, corner, along);
    let b = line(&mut sketch, up, corner);
    let cut = chamfer(corner, [a, b], Setback::Two(length("2"), length("3")));
    let chamfered = propose_it(&sketch, &cut).unwrap().sketch;
    let id = chamfered.curves.last().unwrap().id;
    let mirror = SketchEdit::Mirror {
        ids: vec![a, b, id],
        about: Id::Y_AXIS,
    };
    let mut mirrored = propose_it(&chamfered, &mirror).unwrap().sketch;
    assert!(analyse(&mirrored).solved);
    // The copy made equal, as a file could have it: its ends 2 and 3 from
    // the corner aren't, and nothing the other says holds them so.
    let copy = mirrored.curves.last_mut().unwrap();
    copy.corner.as_mut().unwrap().equal = true;
    assert_eq!(mirrored.check(&DESIGN), Ok(()));
    assert!(!analyse(&mirrored).solved);
}

/// A fillet on the corner at (10, 0) of a line along the x axis from
/// (0, 0) and one up from there to (12, 8), mirrored with both about the
/// x axis, which the first is its own image in: the sketch before and
/// the proposal accepted. With `held`, the first line is held on the axis
/// as drawn from the origin: its start on it, and horizontal.
fn mirrored_on_the_axis(held: bool) -> (Sketch, crate::Accepted) {
    let mut sketch = Sketch::default();
    let start = point(&mut sketch, 0.0, 0.0);
    let corner = point(&mut sketch, 10.0, 0.0);
    let up = point(&mut sketch, 12.0, 8.0);
    let base = line(&mut sketch, start, corner);
    let rising = line(&mut sketch, corner, up);
    if held {
        constrain(&mut sketch, Constraint::Coincident(start, Id::ORIGIN));
        constrain(&mut sketch, Constraint::Horizontal(base));
    }
    let filleted = propose_it(&sketch, &fillet(corner, [base, rising], "2"))
        .unwrap()
        .sketch;
    let id = filleted.curves.last().unwrap().id;
    let mirror = SketchEdit::Mirror {
        ids: vec![base, rising, id],
        about: Id::X_AXIS,
    };
    let accepted = propose_it(&filleted, &mirror).unwrap();
    (filleted, accepted)
}

/// How far each end of each fillet or chamfer of `sketch` is off the
/// endless line it's on, the most.
fn off_lines(sketch: &Sketch) -> f64 {
    let mut most: f64 = 0.0;
    for entry in sketch.curves.iter().filter(|entry| entry.corner.is_some()) {
        let corner = entry.corner.unwrap();
        let [start, end] = entry.curve.ends().unwrap();
        for (line, point) in [(corner.a, start), (corner.b, end)] {
            let (a, b) = sketch.line(line).unwrap();
            let off = (b - a).normalize().perp_dot(at(sketch, point) - a);
            most = most.max(off.abs());
        }
    }
    most
}

#[test]
fn a_fillet_mirrored_about_a_corner_on_the_line_stays_on_its_lines() {
    let (_, mirrored) = mirrored_on_the_axis(false);
    let freedom = mirrored.analysis.freedom;
    // What holds the points on the axis taken away, the mirror still
    // does: the copy's corner is the image of the other only so.
    let ties: Vec<Id> = mirrored
        .sketch
        .constraints
        .iter()
        .filter(|entry| matches!(entry.constraint, Constraint::PointOnCurve { .. }))
        .map(|entry| entry.id)
        .collect();
    assert!(!ties.is_empty());
    let untied = propose_it(&mirrored.sketch, &SketchEdit::Delete(ties)).unwrap();
    assert_eq!(untied.analysis.freedom, freedom);
    assert!(untied.analysis.redundant.is_empty());
    let corner = untied.sketch.points[1].id;
    let dragged = SketchEdit::Move {
        points: vec![(corner, DVec2::new(10.0, 1.0))],
        radii: Vec::new(),
    };
    let moved = propose_it(&untied.sketch, &dragged).unwrap().sketch;
    assert!(off_lines(&moved) < 1e-6, "{}", off_lines(&moved));
}

#[test]
fn a_fillet_mirrored_about_the_axis_its_line_is_held_on_adds_nothing() {
    let (filleted, mirrored) = mirrored_on_the_axis(true);
    assert!(mirrored.analysis.redundant.is_empty());
    assert_eq!(mirrored.analysis.freedom, analyse(&filleted).freedom);
    assert!(off_lines(&mirrored.sketch) < 1e-9);
}
