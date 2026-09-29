use varde_document::Command;
use varde_sketch::{Constraint, Curve, Side, Spline};
use varde_view::{Edit, Inference, Level, Look, Target, Tool, ToolClick};

use super::*;
use crate::doc::sketch::tests::{
    Answered, at, click, click_at, click_on, lines, position, sketch, sketching, undo_to,
};

/// Clicks the tool in use at `x`, `y`, snapped to `target` with
/// `inference`, as the viewport sends it.
fn snapped(
    doc: &mut Answered,
    x: f64,
    y: f64,
    target: Option<Target>,
    inference: Option<Inference>,
) {
    doc.update(Edit::ToolClick(ToolClick {
        target,
        inference,
        ..click_at(x, y)
    }));
}

/// The constraints of the sketch being edited, in order.
fn constraints(doc: &Answered) -> Vec<Constraint> {
    sketch(doc)
        .constraints
        .iter()
        .map(|entry| entry.constraint.clone())
        .collect()
}

#[test]
fn a_line_snapped_to_the_origin_and_along_the_x_axis_keeps_what_holds() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    snapped(&mut doc, 0.0, 0.0, Some(Target::Point(Id::ORIGIN)), None);
    // On the x axis and horizontal from the origin restate each other: one
    // of the two is dropped, and the line is drawn all the same.
    snapped(
        &mut doc,
        10.0,
        0.0,
        Some(Target::On(Id::X_AXIS)),
        Some(Inference::Horizontal),
    );
    let drawn = sketch(&doc).clone();
    let [(start, end)] = lines(&drawn)[..] else {
        panic!("{drawn:?}");
    };
    let made = constraints(&doc);
    assert_eq!(made.len(), 2, "{made:?}");
    assert_eq!(made[0], Constraint::Coincident(start, Id::ORIGIN));
    assert_eq!(
        made[1],
        Constraint::PointOnCurve {
            point: end,
            curve: Id::X_AXIS
        }
    );
    // Fixed but for its length.
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert_eq!(analysis.freedom, 1);
    assert!(analysis.redundant.is_empty());
    assert_eq!(position(&drawn, start), at(0.0, 0.0));
    // One undo step.
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 1);
}

#[test]
fn a_line_joins_points_it_snaps_to_and_infers_from_the_last() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 5.0);
    snapped(&mut doc, 10.0, 5.0, None, Some(Inference::Horizontal));
    let first = sketch(&doc).clone();
    let [(a, b)] = lines(&first)[..] else {
        panic!("{first:?}");
    };
    let [first_line] = [first.curves[0].id];
    assert_eq!(constraints(&doc), [Constraint::Horizontal(first_line)]);
    doc.look(Look::Escape);

    // A new chain from `b`, a point of the sketch's, takes it as its own.
    click_on(&mut doc, 10.0, 5.0, Some(b));
    snapped(
        &mut doc,
        10.0,
        12.0,
        None,
        Some(Inference::Perpendicular(first_line)),
    );
    let second = sketch(&doc).clone();
    assert_eq!(second.points.len(), 3);
    let (start, c) = lines(&second)[1];
    assert_eq!(start, b);
    let second_line = second.curves[1].id;
    assert_eq!(
        constraints(&doc)[1],
        Constraint::Perpendicular(second_line, first_line)
    );
    // Ending on `a` joins it too, and goes on from it.
    click_on(&mut doc, 0.0, 5.0, Some(a));
    assert_eq!(lines(sketch(&doc))[2], (c, a));
    assert_eq!(sketch(&doc).points.len(), 3);
    let tool = doc.sketch.as_ref().unwrap().tool.clone().unwrap();
    assert_eq!(tool.chain.map(|chain| chain.last), Some(a));
    assert_eq!(tool.targets, [Some(Target::Point(a))]);
}

#[test]
fn points_snapped_to_curves_are_tied_to_them() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 3.0);
    click(&mut doc, 8.0, 7.0);
    let line = sketch(&doc).curves[0].id;
    doc.look(Look::Escape);
    doc.look(Look::SelectTool(Tool::Circle));
    click(&mut doc, 20.0, 0.0);
    click(&mut doc, 22.0, 0.0);
    let circle = sketch(&doc).curves[1].id;
    let Curve::Circle { center, .. } = sketch(&doc).curves[1].curve else {
        unreachable!()
    };
    doc.look(Look::SelectTool(Tool::Point));
    snapped(&mut doc, 4.0, 5.0, Some(Target::Midpoint(line)), None);
    snapped(
        &mut doc,
        20.0,
        2.0,
        Some(Target::Quadrant {
            round: circle,
            level: Level::Vertical,
        }),
        None,
    );
    snapped(&mut doc, 2.0, 3.5, Some(Target::On(line)), None);
    let points: Vec<_> = sketch(&doc).points.iter().map(|point| point.id).collect();
    let [.., middle, top, on] = points[..] else {
        panic!("{points:?}");
    };
    assert_eq!(
        constraints(&doc),
        [
            Constraint::Midpoint {
                point: middle,
                line
            },
            Constraint::PointOnCurve {
                point: top,
                curve: circle
            },
            Constraint::VerticalPoints(center, top),
            Constraint::PointOnCurve {
                point: on,
                curve: line
            },
        ]
    );
    // A point where the sketch has one is refused, but at the origin it's
    // one of its own, coincident.
    let before = sketch(&doc).clone();
    click_on(&mut doc, 0.0, 3.0, Some(before.points[0].id));
    assert_eq!(*sketch(&doc), before);
    snapped(&mut doc, 0.0, 0.0, Some(Target::Point(Id::ORIGIN)), None);
    let last = sketch(&doc).points.last().unwrap().id;
    assert_eq!(
        constraints(&doc).last(),
        Some(&Constraint::Coincident(last, Id::ORIGIN))
    );
}

#[test]
fn a_point_snapped_to_a_spline_is_held_on_it() {
    let (mut doc, feature, _) = sketching();
    let mut drawn = Sketch::default();
    let fit =
        [(0.0, 0.0), (10.0, 6.0), (20.0, 4.0)].map(|(x, y)| drawn.add_point(at(x, y)).unwrap());
    let spline = Curve::Spline(Spline::through(fit.to_vec(), false));
    let spline = drawn.add_curve(spline, false).unwrap();
    doc.editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(drawn),
        })
        .unwrap();
    doc.sync();
    doc.lane.answer(&mut doc.doc);
    doc.look(Look::SelectTool(Tool::Point));
    let place = sketch(&doc).nearest_on(spline, at(9.0, 7.0)).unwrap();
    snapped(&mut doc, place.x, place.y, Some(Target::On(spline)), None);
    let point = sketch(&doc).points.last().unwrap().id;
    assert_eq!(
        constraints(&doc),
        [Constraint::PointOnCurve {
            point,
            curve: spline
        }]
    );
    // The spline's three fit points, and the point sliding along it.
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert_eq!(analysis.freedom, 7);
    assert!(analysis.redundant.is_empty());
}

#[test]
fn circles_and_arcs_pass_through_points_and_arcs_run_on_tangent() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 1.0);
    click(&mut doc, 8.0, 1.0);
    let drawn = sketch(&doc).clone();
    let [(a, b)] = lines(&drawn)[..] else {
        panic!("{drawn:?}");
    };
    let line = drawn.curves[0].id;
    doc.look(Look::Escape);

    // A circle through the line's start.
    doc.look(Look::SelectTool(Tool::Circle));
    click(&mut doc, -3.0, 1.0);
    click_on(&mut doc, 0.0, 1.0, Some(a));
    let circle = sketch(&doc).curves[1].id;
    assert_eq!(
        constraints(&doc),
        [Constraint::PointOnCurve {
            point: a,
            curve: circle
        }]
    );

    // An arc from the line's end, tangent to it: its centre above.
    doc.look(Look::SelectTool(Tool::Arc));
    click_on(&mut doc, 8.0, 1.0, Some(b));
    click(&mut doc, 8.0, 9.0);
    snapped(&mut doc, 12.0, 5.0, None, Some(Inference::Tangent(line)));
    let made = sketch(&doc).clone();
    let arc = made.curves[2].id;
    let Curve::Arc { start, end, .. } = made.curves[2].curve else {
        panic!("{made:?}");
    };
    // Counter-clockwise from the line's end, round to the right.
    assert_eq!(start, b);
    assert_eq!(position(&made, end), at(8.0, 9.0));
    assert_eq!(
        constraints(&doc).last(),
        Some(&Constraint::Tangent {
            a: line,
            b: arc,
            side: Side::Positive,
            at: None,
        })
    );
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert!(analysis.redundant.is_empty());
}

#[test]
fn the_snap_shows_until_the_click_takes_it() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Point));
    let snap = varde_view::Snap {
        at: at(0.0, 0.0),
        target: Some(Target::Point(Id::ORIGIN)),
        inference: None,
    };
    doc.look(Look::Snap(Some(snap)));
    assert_eq!(doc.sketch_state().unwrap().snap, Some(snap));
    snapped(&mut doc, 0.0, 0.0, snap.target, None);
    assert_eq!(doc.sketch_state().unwrap().snap, None);
    doc.look(Look::Snap(Some(snap)));
    doc.look(Look::SelectTool(Tool::Line));
    assert_eq!(doc.sketch_state().unwrap().snap, None);
}

#[test]
fn the_origin_and_axes_are_selected_but_never_deleted() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::ClickGeometry {
        hit: Some(Id::ORIGIN),
        add: false,
    });
    doc.look(Look::ClickGeometry {
        hit: Some(Id::X_AXIS),
        add: true,
    });
    let selection = &doc.sketch.as_ref().unwrap().selection;
    assert_eq!(selection.len(), 2);
    let revision = doc.editor.revision();
    doc.update(Edit::DeleteSelection);
    assert_eq!(doc.editor.revision(), revision);
    assert!(!doc.proposing());
}

#[test]
fn a_snap_to_what_an_undo_takes_is_let_go_of() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Point));
    click(&mut doc, 5.0, 0.0);
    let point = sketch(&doc).points[0].id;
    doc.look(Look::SelectTool(Tool::Circle));
    click(&mut doc, 0.0, 0.0);
    // Aimed through the point, which the undo takes.
    doc.look(Look::Aim(ToolClick {
        target: Some(Target::Point(point)),
        hit: Some(point),
        ..click_at(5.0, 0.0)
    }));
    doc.update(Edit::Undo);
    assert_eq!(sketch(&doc), &Sketch::default());
    let state = doc.sketch_state().unwrap();
    assert_eq!(state.snap, None);
    assert_eq!(state.aim, Some(at(5.0, 0.0)));
    // Enter places the circle where it was aimed, through nothing.
    doc.update(Edit::PlaceShape);
    assert_eq!(doc.edit_error, None);
    let drawn = sketch(&doc);
    assert_eq!(drawn.curves.len(), 1, "{drawn:?}");
    assert!(drawn.constraints.is_empty(), "{drawn:?}");
}
