use glam::DVec2;
use iced::keyboard::{self, key};
use varde_sketch::Selectable;
use varde_sketch::{Constraint, Curve, Id, Kind, SplineKind};
use varde_view::{Edit, Look, Message as Ui, Own, Target, Tool, ToolClick};

use crate::Message;
use crate::doc::sketch::tests::{
    Answered, at, click, click_at, click_on, drawing, letter, sketch, sketching, undo_to,
};
use crate::doc::sketch::{Doc, Sketch};

fn enter() -> keyboard::Key {
    keyboard::Key::Named(key::Named::Enter)
}

/// Double-clicks the tool in use at `x`, `y`: a click, then its second.
fn double_click(doc: &mut Answered, x: f64, y: f64) {
    click(doc, x, y);
    doc.update(Edit::ToolClick(ToolClick {
        double: true,
        ..click_at(x, y)
    }));
}

/// The splines of `sketch`, by id.
fn splines(sketch: &Sketch) -> Vec<(Id, varde_sketch::Spline)> {
    sketch
        .curves
        .iter()
        .filter_map(|entry| match &entry.curve {
            Curve::Spline(spline) => Some((entry.id, spline.clone())),
            _ => None,
        })
        .collect()
}

/// The places of `ids` in `sketch`.
fn places(sketch: &Sketch, ids: &[Id]) -> Vec<DVec2> {
    ids.iter().map(|&id| sketch.point(id).unwrap().at).collect()
}

/// Draws a spline through a wave with the Spline tool, ending it with a
/// double-click, putting the tool down after, and gives its id.
fn draw_wave(doc: &mut Answered) -> Id {
    doc.key(letter("n"));
    for (x, y) in [(0.0, 0.0), (5.0, 4.0), (10.0, 1.0), (15.0, -3.0)] {
        click(doc, x, y);
    }
    double_click(doc, 20.0, 0.0);
    doc.look(Look::Escape);
    splines(sketch(doc)).last().unwrap().0
}

/// [`draw_wave`], its handles, which the Spline tool gives every fit
/// point, deleted.
fn draw_bare_wave(doc: &mut Answered) -> Id {
    let id = draw_wave(doc);
    let tips: Vec<Id> = sketch(doc)
        .spline(id)
        .unwrap()
        .handles
        .iter()
        .map(|handle| handle.tip)
        .collect();
    for (i, &tip) in tips.iter().enumerate() {
        doc.look(Look::ClickGeometry {
            hit: Some(Selectable::Item(tip)),
            add: i > 0,
        });
    }
    doc.update(Edit::DeleteSelection);
    assert!(sketch(doc).spline(id).unwrap().handles.is_empty());
    id
}

/// Presses `key` with Shift in `doc`, sending what it sends.
fn shift_key(doc: &mut Answered, key: &str) {
    let press = crate::tests::press(letter(key), keyboard::Modifiers::SHIFT);
    match crate::keys::document_key((doc.keys(), press)) {
        Some(Message::Ui(Ui::Edit(edit))) => doc.update(edit),
        Some(Message::Ui(Ui::Look(look))) => doc.look(look),
        other => panic!("{key}: {other:?}"),
    }
}

fn select(doc: &mut Answered, id: impl Into<Selectable>) {
    doc.look(Look::ClickGeometry {
        hit: Some(id.into()),
        add: false,
    });
}

#[test]
fn the_spline_tool_draws_through_the_points_clicked_until_a_double_click() {
    let (mut doc, _, _) = sketching();
    let before = sketch(&doc).clone();
    doc.key(letter("n"));
    assert_eq!(drawing(&doc).map(|d| d.tool), Some(Tool::Spline));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 5.0, 4.0);
    // Nothing is made until it ends; a click on the last point again, or
    // within a pixel of it, places nothing.
    click(&mut doc, 5.0, 4.05);
    assert_eq!(*sketch(&doc), before);
    assert_eq!(drawing(&doc).unwrap().placed.len(), 2);
    double_click(&mut doc, 10.0, 1.0);
    let drawn = sketch(&doc).clone();
    let [(spline_id, spline)] = &splines(&drawn)[..] else {
        panic!("{drawn:?}");
    };
    assert_eq!((spline.kind, spline.closed), (SplineKind::Through, false));
    assert_eq!(
        places(&drawn, &spline.points),
        [at(0.0, 0.0), at(5.0, 4.0), at(10.0, 1.0)]
    );
    // A handle at each fit point, keeping the shape it'd have without.
    assert_eq!(spline.handles.len(), 3);
    for (handle, &point) in spline.handles.iter().zip(&spline.points) {
        assert_eq!(handle.at, point);
        let tip = drawn.point(handle.tip).unwrap().at;
        let kept = drawn.handle_tip(*spline_id, point).unwrap();
        assert!(tip.distance(kept) < 1e-9, "{tip} {kept}");
    }
    // The tool stays, afresh, for the next.
    let tool = drawing(&doc).unwrap();
    assert_eq!(tool.tool, Tool::Spline);
    assert!(tool.placed.is_empty());
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    // Two for each fit point and each tip.
    assert_eq!(analysis.freedom, 12);
    assert_eq!(undo_to(&mut doc, &before), 1);

    // One point is no spline: a double-click there does nothing.
    double_click(&mut doc, 3.0, 3.0);
    assert_eq!(*sketch(&doc), before);
    assert_eq!(drawing(&doc).unwrap().placed.len(), 1);
}

#[test]
fn enter_ends_the_spline_and_its_first_point_closes_it() {
    let (mut doc, _, _) = sketching();
    let before = sketch(&doc).clone();
    doc.key(letter("n"));
    // Enter with one point placed ends nothing.
    click(&mut doc, 0.0, 0.0);
    assert!(crate::tests::press_in(&doc, enter()).is_none());
    click(&mut doc, 8.0, 2.0);
    doc.key(enter());
    let [(_, spline)] = &splines(sketch(&doc))[..] else {
        panic!("one spline");
    };
    assert_eq!(spline.points.len(), 2);
    assert_eq!(undo_to(&mut doc, &before), 1);

    // Round, and back to the first point, within the snap's reach of
    // it: closed through the three.
    for (x, y) in [(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)] {
        click(&mut doc, x, y);
    }
    click(&mut doc, 0.3, -0.2);
    let [(_, spline)] = &splines(sketch(&doc))[..] else {
        panic!("one spline");
    };
    assert!(spline.closed);
    assert_eq!(spline.points.len(), 3);
    // A closed spline is a profile on its own.
    let profiles = doc.sketch_state().unwrap().profiles.unwrap();
    assert_eq!(profiles.as_ref().unwrap().regions.len(), 1);
    assert_eq!(undo_to(&mut doc, &before), 1);

    // Back to the first point with too few to close: nothing is placed
    // on top of it, to close on it after (the tool is afresh).
    assert!(drawing(&doc).unwrap().placed.is_empty());
    for (x, y) in [(0.0, 0.0), (10.0, 0.0)] {
        click(&mut doc, x, y);
    }
    click(&mut doc, 0.3, -0.2);
    assert_eq!(drawing(&doc).unwrap().placed, [at(0.0, 0.0), at(10.0, 0.0)]);
}

#[test]
fn z_switches_the_spline_tool_to_control_points() {
    let (mut doc, _, _) = sketching();
    let before = sketch(&doc).clone();
    doc.key(letter("n"));
    click(&mut doc, 0.0, 0.0);
    doc.key(letter("z"));
    assert!(drawing(&doc).unwrap().control);
    assert_eq!(drawing(&doc).unwrap().placed.len(), 1);
    for (x, y) in [(4.0, 6.0), (10.0, 6.0)] {
        click(&mut doc, x, y);
    }
    // By control points it takes four.
    assert!(crate::tests::press_in(&doc, enter()).is_none());
    double_click(&mut doc, 14.0, 0.0);
    let drawn = sketch(&doc).clone();
    let [(id, spline)] = &splines(&drawn)[..] else {
        panic!("{drawn:?}");
    };
    assert_eq!(spline.kind, SplineKind::Control);
    assert_eq!(spline.points.len(), 4);
    // Starting and ending at its first and last control points.
    let geom = drawn.flatten(&drawn.curve(*id).unwrap().curve).unwrap();
    assert!(geom[0].distance(at(0.0, 0.0)) < 1e-9);
    assert!(geom.last().unwrap().distance(at(14.0, 0.0)) < 1e-9);
    // Kept for the next.
    assert!(drawing(&doc).unwrap().control);
    assert_eq!(undo_to(&mut doc, &before), 1);
    doc.key(letter("z"));
    assert!(!drawing(&doc).unwrap().control);
}

#[test]
fn a_spline_s_points_snap_and_are_tied_as_any_shape_s() {
    let (mut doc, _, _) = sketching();
    // A line to start on, and to end on.
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, -5.0);
    click(&mut doc, 20.0, -5.0);
    doc.look(Look::Escape);
    doc.look(Look::Escape);
    let lined = sketch(&doc).clone();
    let entry = &lined.curves[0];
    let (line, [start, _]) = (entry.id, entry.curve.ends().unwrap());
    doc.look(Look::SelectTool(Tool::Spline));
    click(&mut doc, -6.0, 3.0);
    click_on(&mut doc, 0.0, -5.0, Some(start));
    doc.update(Edit::ToolClick(ToolClick {
        target: Some(Target::On(line)),
        ..click_at(12.0, -5.0)
    }));
    // A point it has already is none to go through again (but its first,
    // which closes it).
    click_on(&mut doc, 0.0, -5.0, Some(start));
    assert_eq!(drawing(&doc).unwrap().placed.len(), 3);
    doc.key(enter());
    let drawn = sketch(&doc).clone();
    let (_, spline) = splines(&drawn).pop().unwrap();
    assert!(!spline.closed);
    assert_eq!(spline.points[1], start);
    let last = spline.points[2];
    assert!(drawn.constraints.iter().any(|entry| entry.constraint
        == Constraint::PointOnCurve {
            point: last,
            curve: line
        }));
}

#[test]
fn handles_come_and_go_by_their_key() {
    let (mut doc, _, _) = sketching();
    let id = draw_bare_wave(&mut doc);
    let before = sketch(&doc).clone();
    // With the spline selected, at all its fit points.
    select(&mut doc, id);
    shift_key(&mut doc, "H");
    let handled = sketch(&doc).clone();
    let spline = handled.spline(id).unwrap();
    let [first, last] = spline.ends().unwrap();
    assert!(spline.has_handle(first) && spline.has_handle(last));
    assert_eq!(spline.handles.len(), 5);
    assert_eq!(undo_to(&mut doc, &before), 1);
    shift_key(&mut doc, "H");
    // And away again, in one step.
    let with = sketch(&doc).clone();
    shift_key(&mut doc, "H");
    assert!(sketch(&doc).spline(id).unwrap().handles.is_empty());
    assert_eq!(undo_to(&mut doc, &with), 1);

    // At a fit point selected, there.
    undo_to(&mut doc, &before);
    let middle = sketch(&doc).spline(id).unwrap().points[2];
    select(&mut doc, middle);
    shift_key(&mut doc, "H");
    assert!(sketch(&doc).spline(id).unwrap().has_handle(middle));
    // A tip dragged is any point's drag; deleted, its handle goes.
    let tip = sketch(&doc).spline(id).unwrap().handles[0].tip;
    select(&mut doc, tip);
    doc.update(Edit::DeleteSelection);
    assert!(!sketch(&doc).spline(id).unwrap().has_handle(middle));
}

#[test]
fn a_double_click_on_a_spline_adds_a_point_and_delete_takes_one() {
    let (mut doc, _, _) = sketching();
    let id = draw_wave(&mut doc);
    let before = sketch(&doc).clone();
    let near = sketch(&doc).nearest_on(id, at(7.5, 3.0)).unwrap();
    doc.update(Edit::InsertSplinePoint {
        spline: id,
        at: near,
    });
    let inserted = sketch(&doc).clone();
    let spline = inserted.spline(id).unwrap();
    assert_eq!(spline.points.len(), 6);
    assert!(inserted.point(spline.points[2]).unwrap().at.distance(near) < 1e-6);
    assert_eq!(undo_to(&mut doc, &before), 1);

    // Deleting a fit point keeps a smooth spline through the rest.
    let second = before.spline(id).unwrap().points[1];
    select(&mut doc, second);
    doc.update(Edit::DeleteSelection);
    let spline = sketch(&doc).spline(id).unwrap();
    assert_eq!(spline.points.len(), 4);
    assert_eq!(undo_to(&mut doc, &before), 1);
}

#[test]
fn z_converts_the_splines_selected_and_back() {
    let (mut doc, _, _) = sketching();
    let id = draw_bare_wave(&mut doc);
    let before = sketch(&doc).clone();
    // Nothing selected, Z does nothing.
    assert!(crate::tests::press_in(&doc, letter("z")).is_none());
    select(&mut doc, id);
    doc.key(letter("z"));
    assert_eq!(sketch(&doc).spline(id).unwrap().kind, SplineKind::Control);
    let converted = sketch(&doc).clone();
    select(&mut doc, id);
    doc.key(letter("z"));
    let back = sketch(&doc).clone();
    assert_eq!(back.spline(id).unwrap().kind, SplineKind::Through);
    // Through its places at its knots, the fit points it had.
    let (was, is) = (
        before.flatten(&before.curve(id).unwrap().curve).unwrap(),
        back.flatten(&back.curve(id).unwrap().curve).unwrap(),
    );
    for place in was {
        let nearest = is
            .iter()
            .map(|p| p.distance(place))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 0.05, "{place}: {nearest}");
    }
    assert_eq!(undo_to(&mut doc, &converted), 1);
    assert_eq!(undo_to(&mut doc, &before), 1);
}

#[test]
fn u_shows_the_curvature_comb() {
    let (mut doc, _, _) = sketching();
    let state = |doc: &Doc| doc.sketch_state().unwrap().comb;
    assert!(!state(&doc));
    doc.key(letter("u"));
    assert!(state(&doc));
    doc.key(letter("u"));
    assert!(!state(&doc));
}

#[test]
fn trim_extend_and_offset_take_splines() {
    let (mut doc, _, _) = sketching();
    let id = draw_wave(&mut doc);
    // A line across it at x = 7.
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 7.0, -8.0);
    click(&mut doc, 7.0, 8.0);
    doc.look(Look::Escape);
    doc.look(Look::Escape);
    let before = sketch(&doc).clone();

    doc.key(letter("t"));
    let near = before.nearest_on(id, at(2.0, 2.0)).unwrap();
    doc.update(Edit::ToolClick(ToolClick {
        hit: Some(Selectable::Item(id)),
        ..click_at(near.x, near.y)
    }));
    let trimmed = sketch(&doc).clone();
    let [start, _] = trimmed.spline(id).unwrap().ends().unwrap();
    assert!((trimmed.point(start).unwrap().at.x - 7.0).abs() < 1e-3);
    assert_eq!(undo_to(&mut doc, &before), 1);

    // Its end on to a wall.
    doc.look(Look::Escape);
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 26.0, -8.0);
    click(&mut doc, 26.0, 8.0);
    doc.look(Look::Escape);
    doc.look(Look::Escape);
    let walled = sketch(&doc).clone();
    doc.key(letter("j"));
    doc.update(Edit::ToolClick(ToolClick {
        hit: Some(Selectable::Item(id)),
        ..click_at(19.5, 0.0)
    }));
    let extended = sketch(&doc).clone();
    let [_, end] = extended.spline(id).unwrap().ends().unwrap();
    assert!((extended.point(end).unwrap().at.x - 26.0).abs() < 1e-3);
    assert_eq!(undo_to(&mut doc, &walled), 1);

    // Offset alone, by a click to its side: a spline, tied by one
    // dimension.
    doc.look(Look::Escape);
    doc.key(letter("o"));
    doc.update(Edit::ToolClick(ToolClick {
        hit: Some(Selectable::Item(id)),
        ..click_at(10.0, 1.0)
    }));
    // In a chain with nothing: picked alone.
    assert_eq!(drawing(&doc).unwrap().picked, [id]);
    let side = walled.nearest_on(id, at(10.0, 6.0)).unwrap() + DVec2::new(0.0, 2.0);
    click(&mut doc, side.x, side.y);
    let offset = sketch(&doc).clone();
    let copy = offset.curves.last().unwrap();
    assert_eq!(copy.curve.kind(), Kind::Spline);
    assert_eq!(offset.dimensions.len(), 1);
    assert_eq!(undo_to(&mut doc, &walled), 1);
}

#[test]
fn a_handle_is_selected_dragged_constrained_and_deleted_as_a_line() {
    let (mut doc, _, _) = sketching();
    let id = draw_wave(&mut doc);
    let handle = sketch(&doc).spline(id).unwrap().handles[1];
    let line = Selectable::HandleLine(handle.tip);
    let (fit, tip) = (
        sketch(&doc).point(handle.at).unwrap().at,
        sketch(&doc).point(handle.tip).unwrap().at,
    );
    // Picked as a line, apart from its tip.
    select(&mut doc, line);
    assert_eq!(
        doc.sketch.as_ref().unwrap().selection,
        [line].into_iter().collect()
    );
    // Dragged, it turns about its fit point, its length kept.
    let before = sketch(&doc).clone();
    let up = fit + DVec2::new(0.0, 5.0);
    doc.look(Look::DragGeometry {
        id: line,
        from: (fit + tip) / 2.0,
        to: up,
        target: None,
    });
    doc.update(Edit::DropGeometry);
    let turned = sketch(&doc).point(handle.tip).unwrap().at - fit;
    assert!(turned.x.abs() < 1e-6, "{turned}");
    assert!((turned.length() - (tip - fit).length()).abs() < 1e-6);
    assert_eq!(undo_to(&mut doc, &before), 1);
    // Constrained horizontal, named by its tip.
    select(&mut doc, line);
    doc.update(Edit::Constrain(varde_view::ConstraintKind::Horizontal));
    let held = sketch(&doc).clone();
    assert!(
        held.constraints
            .iter()
            .any(|entry| entry.constraint == Constraint::Horizontal(handle.tip))
    );
    let along = held.point(handle.tip).unwrap().at - held.point(handle.at).unwrap().at;
    assert!(along.y.abs() < 1e-9, "{along}");
    // Deleted, the handle goes, its fit point stays.
    select(&mut doc, line);
    doc.update(Edit::DeleteSelection);
    let spline = sketch(&doc).spline(id).unwrap();
    assert!(!spline.has_handle(handle.at) && spline.points.contains(&handle.at));
}

#[test]
fn the_dimension_tool_takes_a_handle_as_a_line() {
    let (mut doc, _, _) = sketching();
    let id = draw_wave(&mut doc);
    let tip = sketch(&doc).spline(id).unwrap().handles[1].tip;
    let line = Selectable::HandleLine(tip);
    doc.look(Look::SelectTool(Tool::Dimension));
    doc.update(Edit::ToolClick(ToolClick {
        hit: Some(line),
        ..click_at(0.0, 0.0)
    }));
    assert_eq!(drawing(&doc).unwrap().picked, [line]);
    // Selected first, it's picked as the tool's taken.
    doc.look(Look::PutDownTool);
    select(&mut doc, line);
    doc.look(Look::SelectTool(Tool::Dimension));
    assert_eq!(drawing(&doc).unwrap().picked, [line]);
}

/// A handle's end is selected alone, dragged with the tip mirroring it,
/// and deleted as its tip is.
#[test]
fn a_handle_s_end_is_selected_dragged_and_deleted() {
    let (mut doc, _, _) = sketching();
    let id = draw_wave(&mut doc);
    let handle = sketch(&doc).spline(id).unwrap().handles[1];
    let end = Selectable::Item(handle.end);
    let fit = sketch(&doc).point(handle.at).unwrap().at;
    let mirrored = sketch(&doc).point(handle.end).unwrap().at;
    select(&mut doc, end);
    assert_eq!(
        doc.sketch.as_ref().unwrap().selection,
        [end].into_iter().collect()
    );
    // Dragged, the end follows the cursor and the tip mirrors it.
    let before = sketch(&doc).clone();
    let to = mirrored + DVec2::new(1.0, -2.0);
    doc.look(Look::DragGeometry {
        id: end,
        from: mirrored,
        to,
        target: None,
    });
    doc.update(Edit::DropGeometry);
    let dragged = sketch(&doc);
    assert!(dragged.point(handle.end).unwrap().at.distance(to) < 1e-6);
    let tip = dragged.point(handle.tip).unwrap().at;
    assert!(tip.distance(2.0 * fit - to) < 1e-6, "{tip}");
    assert_eq!(undo_to(&mut doc, &before), 1);
    // Deleted, the handle goes, its fit point stays.
    select(&mut doc, end);
    doc.update(Edit::DeleteSelection);
    let spline = sketch(&doc).spline(id).unwrap();
    assert!(!spline.has_handle(handle.at) && spline.points.contains(&handle.at));
}

/// A spline snapped to itself while drawn gets a point of its own there,
/// coincident with the point it placed there, or where it was on its
/// curve, untied (a point on its own curve would be so whatever).
#[test]
fn a_spline_snapped_to_itself_is_tied_to_itself() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Spline));
    click(&mut doc, 0.0, 0.0);
    click(&mut doc, 10.0, 5.0);
    click(&mut doc, 20.0, 0.0);
    let own = |target, x, y| {
        Edit::ToolClick(ToolClick {
            target: Some(Target::Own(target)),
            ..click_at(x, y)
        })
    };
    doc.update(own(Own::Point(1), 10.0, 5.0));
    doc.update(own(Own::Curve, 5.0, 3.0));
    doc.key(enter());
    let drawn = sketch(&doc).clone();
    let (_, spline) = splines(&drawn).pop().unwrap();
    assert_eq!(spline.points.len(), 5);
    let [_, second, _, again, on] = spline.points[..] else {
        unreachable!()
    };
    assert_ne!(again, second);
    let has = |constraint| drawn.constraints.iter().any(|e| e.constraint == constraint);
    assert!(
        has(Constraint::Coincident(again, second)) || has(Constraint::Coincident(second, again)),
        "{:?}",
        drawn.constraints
    );
    assert_eq!(drawn.point(on).unwrap().at, DVec2::new(5.0, 3.0));
}
