use glam::DVec2;
use iced::keyboard::{self, key};
use varde_document::EditError as DocumentError;
use varde_sketch::Selectable;
use varde_sketch::{Constraint, Curve, EditError, Measure};
use varde_view::{Edit, Look, Tool, ToolClick};

use super::*;
use crate::doc::sketch::tests::{
    Answered, at, click, click_at, drawing, letter, lines, position, sketch, sketching, undo_to,
};

/// Draws a line from (`x0`, `y0`) to (`x1`, `y1`) with the Line tool,
/// putting it down after, and gives its id.
fn draw(doc: &mut Answered, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> Id {
    doc.look(Look::SelectTool(Tool::Line));
    click(doc, x0, y0);
    click(doc, x1, y1);
    doc.look(Look::Escape);
    doc.look(Look::Escape);
    sketch(doc).curves.last().unwrap().id
}

/// Clicks the tool in use at `x`, `y` with `hit` under the cursor.
fn click_hit(doc: &mut Answered, x: f64, y: f64, hit: Id) {
    doc.update(Edit::ToolClick(ToolClick {
        hit: Some(Selectable::Item(hit)),
        ..click_at(x, y)
    }));
}

#[test]
fn trim_takes_away_the_piece_clicked() {
    let (mut doc, _, _) = sketching();
    let across = draw(&mut doc, (0.0, 0.0), (30.0, 0.0));
    draw(&mut doc, (10.0, -5.0), (10.0, 5.0));
    draw(&mut doc, (20.0, -5.0), (20.0, 5.0));
    let before = sketch(&doc).clone();
    doc.key(letter("t"));
    assert_eq!(drawing(&doc).map(|drawing| drawing.tool), Some(Tool::Trim));
    // On nothing, nothing.
    click(&mut doc, 15.0, 3.0);
    assert_eq!(*sketch(&doc), before);

    click_hit(&mut doc, 15.0, 0.0, across);
    let trimmed = sketch(&doc).clone();
    assert_eq!(lines(&trimmed).len(), 4);
    let (start, end) = lines(&trimmed)[0];
    assert_eq!(position(&trimmed, start), at(0.0, 0.0));
    assert_eq!(position(&trimmed, end), at(10.0, 0.0));
    // The tool stays for the next.
    assert_eq!(drawing(&doc).map(|drawing| drawing.tool), Some(Tool::Trim));
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert!(analysis.redundant.is_empty());
    assert_eq!(undo_to(&mut doc, &before), 1);
}

#[test]
fn extend_reaches_the_next_curve_or_says_why_not() {
    let (mut doc, _, _) = sketching();
    let short = draw(&mut doc, (0.0, 0.0), (10.0, 0.0));
    let wall = draw(&mut doc, (20.0, -5.0), (20.0, 5.0));
    let before = sketch(&doc).clone();
    doc.key(letter("j"));
    assert_eq!(
        drawing(&doc).map(|drawing| drawing.tool),
        Some(Tool::Extend)
    );

    // Nearer its end, the end goes.
    click_hit(&mut doc, 8.0, 0.0, short);
    let extended = sketch(&doc).clone();
    let (_, end) = lines(&extended)[0];
    assert_eq!(position(&extended, end), at(20.0, 0.0));
    assert!(extended.constraints.iter().any(|entry| entry.constraint
        == Constraint::PointOnCurve {
            point: end,
            curve: wall
        }));
    assert_eq!(undo_to(&mut doc, &before), 1);

    // The wall's lower end, nearer its start, has nothing ahead.
    click_hit(&mut doc, 20.0, -4.0, wall);
    assert_eq!(*sketch(&doc), before);
    let feature = doc.sketch.as_ref().unwrap().feature;
    assert_eq!(
        doc.edit_error,
        Some(DocumentError::Sketch(feature, EditError::NothingAhead))
    );
}

#[test]
fn mirror_takes_the_selection_then_the_line() {
    let (mut doc, _, _) = sketching();
    let slanted = draw(&mut doc, (1.0, 0.0), (5.0, 3.0));
    let before = sketch(&doc).clone();
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(slanted)),
        add: false,
    });
    doc.shift_key("m");
    let tool = drawing(&doc).unwrap();
    assert_eq!((tool.tool, tool.about), (Tool::Mirror, true));
    assert_eq!(tool.picked, [slanted]);

    click_hit(&mut doc, 0.0, 1.0, Id::Y_AXIS);
    let mirrored = sketch(&doc).clone();
    let [_, (start, end)] = lines(&mirrored)[..] else {
        panic!("{mirrored:?}");
    };
    assert!(position(&mirrored, start).distance(at(-1.0, 0.0)) < 1e-9);
    assert!(position(&mirrored, end).distance(at(-5.0, 3.0)) < 1e-9);
    let symmetric = mirrored.constraints.iter().filter(|entry| {
        matches!(
            entry.constraint,
            Constraint::Symmetric {
                about: Id::Y_AXIS,
                ..
            }
        )
    });
    assert_eq!(symmetric.count(), 2);
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert_eq!(analysis.freedom, 4);
    // Afresh, picking.
    let tool = drawing(&doc).unwrap();
    assert!(tool.picked.is_empty() && !tool.about);
    assert_eq!(undo_to(&mut doc, &before), 1);
}

#[test]
fn mirror_picks_what_is_clicked_until_enter() {
    let (mut doc, _, _) = sketching();
    let first = draw(&mut doc, (1.0, 0.0), (5.0, 3.0));
    let second = draw(&mut doc, (2.0, 5.0), (6.0, 5.0));
    let before = sketch(&doc).clone();
    doc.look(Look::SelectTool(Tool::Mirror));
    let enter = || keyboard::Key::Named(key::Named::Enter);
    // Nothing picked yet: `Enter` does nothing.
    doc.key(enter());
    assert!(!drawing(&doc).unwrap().about);
    click_hit(&mut doc, 3.0, 1.5, first);
    click_hit(&mut doc, 4.0, 5.0, second);
    // A second click takes one out, and the origin isn't picked.
    click_hit(&mut doc, 4.0, 5.0, second);
    click_hit(&mut doc, 0.0, 0.0, Id::ORIGIN);
    assert_eq!(drawing(&doc).unwrap().picked, [first]);
    click_hit(&mut doc, 4.0, 5.0, second);
    doc.key(enter());
    assert!(drawing(&doc).unwrap().about);
    // Both, about the x axis.
    click_hit(&mut doc, 0.0, 0.0, Id::X_AXIS);
    let mirrored = sketch(&doc).clone();
    let curves: Vec<_> = mirrored.curves.iter().map(|entry| &entry.curve).collect();
    assert_eq!(curves.len(), 4);
    let Curve::Line { start, end } = *curves[3] else {
        panic!("{mirrored:?}");
    };
    assert!(position(&mirrored, start).distance(at(2.0, -5.0)) < 1e-9);
    assert!(position(&mirrored, end).distance(at(6.0, -5.0)) < 1e-9);
    assert_eq!(undo_to(&mut doc, &before), 1);

    // `Esc` lets go of what's picked, then puts the tool down.
    click_hit(&mut doc, 3.0, 1.5, first);
    doc.look(Look::Escape);
    assert!(drawing(&doc).unwrap().picked.is_empty());
    doc.look(Look::Escape);
    assert!(drawing(&doc).is_none());
}

/// Draws a rectangle from (0, 0) to (10, 6) with the Rectangle tool,
/// putting it down after, and gives its lines, the bottom first.
fn rectangle(doc: &mut Answered) -> Vec<Id> {
    doc.look(Look::SelectTool(Tool::Rectangle));
    click(doc, 0.0, 0.0);
    click(doc, 10.0, 6.0);
    doc.look(Look::Escape);
    let drawn = sketch(doc);
    let bottom = drawn
        .curves
        .iter()
        .find(|entry| {
            drawn
                .line(entry.id)
                .is_some_and(|(a, b)| a.y == 0.0 && b.y == 0.0)
        })
        .unwrap()
        .id;
    let mut chain = drawn.chain_of(bottom);
    chain.sort_unstable();
    chain.retain(|&id| id != bottom);
    chain.insert(0, bottom);
    chain
}

/// The ends of the lines `sketch` has that `before` hadn't, each from the
/// lower left.
fn new_lines(sketch: &Sketch, before: &Sketch) -> Vec<(DVec2, DVec2)> {
    let mut found: Vec<_> = sketch
        .curves
        .iter()
        .filter(|entry| before.curve(entry.id).is_none())
        .filter_map(|entry| sketch.line(entry.id))
        .map(|(a, b)| {
            let rounded = |p: DVec2| (p * 1e6).round() / 1e6;
            let (a, b) = (rounded(a), rounded(b));
            if (a.x, a.y) <= (b.x, b.y) {
                (a, b)
            } else {
                (b, a)
            }
        })
        .collect();
    found.sort_by(|a, b| {
        (a.0.x, a.0.y, a.1.x, a.1.y)
            .partial_cmp(&(b.0.x, b.0.y, b.1.x, b.1.y))
            .unwrap()
    });
    found
}

/// The rectangle from (`x0`, `y0`) to (`x1`, `y1`)'s sides as
/// [`new_lines`] gives them.
fn sides(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(DVec2, DVec2)> {
    vec![
        (at(x0, y0), at(x0, y1)),
        (at(x0, y0), at(x1, y0)),
        (at(x0, y1), at(x1, y1)),
        (at(x1, y0), at(x1, y1)),
    ]
}

#[test]
fn offset_copies_the_chain_clicked_to_the_click() {
    let (mut doc, _, _) = sketching();
    let chain = rectangle(&mut doc);
    let before = sketch(&doc).clone();
    let freedom = doc.sketch_state().unwrap().analysis.unwrap().freedom;
    doc.key(letter("o"));
    assert_eq!(
        drawing(&doc).map(|drawing| drawing.tool),
        Some(Tool::Offset)
    );
    // On nothing, nothing; on a line, its loop.
    click(&mut doc, 5.0, 3.0);
    assert!(drawing(&doc).unwrap().picked.is_empty());
    click_hit(&mut doc, 5.0, 0.0, chain[0]);
    let mut picked = drawing(&doc).unwrap().picked.clone();
    picked.sort_unstable();
    let mut all = chain.clone();
    all.sort_unstable();
    assert_eq!(picked, all);
    assert_eq!(*sketch(&doc), before);

    // Two below the bottom: outside, by 2.
    click(&mut doc, 5.0, -2.0);
    let grown = sketch(&doc).clone();
    assert_eq!(new_lines(&grown, &before), sides(-2.0, -2.0, 12.0, 8.0));
    let offset = grown
        .dimensions
        .iter()
        .find(|entry| matches!(entry.dimension.measure, Measure::Offset(..)))
        .unwrap();
    assert_eq!(offset.dimension.value.value, 2.0);
    // The copy is held by the rectangle and the one dimension.
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert_eq!(analysis.freedom, freedom);
    assert!(analysis.redundant.is_empty());
    // The tool stays, afresh.
    let tool = drawing(&doc).unwrap();
    assert_eq!(tool.tool, Tool::Offset);
    assert!(tool.picked.is_empty());
    assert_eq!(undo_to(&mut doc, &before), 1);
}

#[test]
fn offset_takes_a_distance_typed() {
    let (mut doc, _, _) = sketching();
    let chain = rectangle(&mut doc);
    let before = sketch(&doc).clone();
    doc.key(letter("o"));
    // No distance to type until it has its chain.
    doc.look(Look::NextField);
    assert_eq!(doc.drawing_field(), None);
    click_hit(&mut doc, 5.0, 0.0, chain[0]);
    // Inside, where the cursor is, by the distance typed.
    doc.look(Look::Aim(click_at(5.0, 2.5)));
    doc.look(Look::NextField);
    assert_eq!(doc.drawing_field(), Some(Field::Distance));
    doc.look(Look::ValueInput("1".to_owned()));
    doc.update(Edit::SubmitValue);
    assert_eq!(new_lines(sketch(&doc), &before), sides(1.0, 1.0, 9.0, 5.0));
    assert_eq!(undo_to(&mut doc, &before), 1);

    // A distance that isn't one keeps the field open, saying why.
    click_hit(&mut doc, 5.0, 0.0, chain[0]);
    doc.look(Look::Aim(click_at(5.0, 2.5)));
    doc.look(Look::NextField);
    doc.look(Look::ValueInput("0".to_owned()));
    doc.update(Edit::SubmitValue);
    assert_eq!(doc.drawing_field(), Some(Field::Distance));
    assert_eq!(*sketch(&doc), before);
}

#[test]
fn offset_starts_from_the_selection_and_says_why_it_can_t() {
    let (mut doc, _, _) = sketching();
    let chain = rectangle(&mut doc);
    let before = sketch(&doc).clone();
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(chain[1])),
        add: false,
    });
    doc.look(Look::SelectTool(Tool::Offset));
    assert_eq!(drawing(&doc).unwrap().picked.len(), 4);
    // Inside by half its height, nothing's left.
    click(&mut doc, 5.0, 3.0);
    assert_eq!(*sketch(&doc), before);
    let feature = doc.sketch.as_ref().unwrap().feature;
    assert_eq!(
        doc.edit_error,
        Some(DocumentError::Sketch(feature, EditError::NothingLeft))
    );
    // Less than a pixel off it, no distance was meant.
    click(&mut doc, 5.0, 1e-9);
    assert_eq!(*sketch(&doc), before);
    // `Esc` lets go of the chain, then puts the tool down.
    doc.look(Look::Escape);
    let tool = drawing(&doc).unwrap();
    assert!(tool.picked.is_empty() && tool.tool == Tool::Offset);
    doc.look(Look::Escape);
    assert!(drawing(&doc).is_none());
}

/// The corner at (10, 6) of [`rectangle`]'s, and its lines along the top
/// and down the right.
fn top_right(doc: &Answered) -> (Id, Id, Id) {
    let drawn = sketch(doc);
    let corner = drawn
        .points
        .iter()
        .find(|point| point.at == at(10.0, 6.0))
        .unwrap()
        .id;
    let line = |a: DVec2, b: DVec2| {
        drawn
            .curves
            .iter()
            .find(|entry| {
                drawn
                    .line(entry.id)
                    .is_some_and(|(s, e)| (s == a && e == b) || (s == b && e == a))
            })
            .unwrap()
            .id
    };
    let top = line(at(0.0, 6.0), at(10.0, 6.0));
    let right = line(at(10.0, 0.0), at(10.0, 6.0));
    (corner, top, right)
}

/// The fillets or chamfers of `sketch`.
fn corners(sketch: &Sketch) -> Vec<&varde_sketch::CurveEntry> {
    let curves = sketch.curves.iter();
    curves.filter(|entry| entry.corner.is_some()).collect()
}

#[test]
fn fillet_rounds_the_corner_clicked_through_the_next_click() {
    let (mut doc, _, _) = sketching();
    rectangle(&mut doc);
    let (corner, top, right) = top_right(&doc);
    let before = sketch(&doc).clone();
    let freedom = doc.sketch_state().unwrap().analysis.unwrap().freedom;
    doc.key(letter("f"));
    assert_eq!(
        drawing(&doc).map(|drawing| drawing.tool),
        Some(Tool::Fillet)
    );
    // On nothing, nothing; by the corner, it and the line nearer first.
    click(&mut doc, 5.0, 3.0);
    assert!(drawing(&doc).unwrap().picked.is_empty());
    click_hit(&mut doc, 9.8, 5.9, corner);
    assert_eq!(drawing(&doc).unwrap().picked, [corner, top, right]);
    assert_eq!(*sketch(&doc), before);

    // Its middle through (9, 5): a radius of 1 / (1 - sin 45°).
    click(&mut doc, 9.0, 5.0);
    let filleted = sketch(&doc).clone();
    let made = corners(&filleted);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].name(), "Fillet 1");
    let radius = filleted.dimensions.last().unwrap();
    assert_eq!(radius.dimension.measure, Measure::Radius(made[0].id));
    let expected = 1.0 / (1.0 - std::f64::consts::FRAC_1_SQRT_2);
    assert!((radius.dimension.value.value - expected).abs() < 1e-3);
    // The lines stay whole, the fillet held by its radius.
    assert_eq!(filleted.line(top), before.line(top));
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert_eq!(analysis.freedom, freedom);
    assert!(analysis.redundant.is_empty());
    // The tool stays, afresh.
    let tool = drawing(&doc).unwrap();
    assert_eq!(tool.tool, Tool::Fillet);
    assert!(tool.picked.is_empty());
    assert_eq!(undo_to(&mut doc, &before), 1);
}

#[test]
fn fillet_takes_a_radius_typed_and_says_when_it_does_not_fit() {
    let (mut doc, _, _) = sketching();
    rectangle(&mut doc);
    let (corner, ..) = top_right(&doc);
    let before = sketch(&doc).clone();
    doc.key(letter("f"));
    // No radius to type until it has its corner.
    doc.look(Look::NextField);
    assert_eq!(doc.drawing_field(), None);
    click_hit(&mut doc, 9.8, 5.9, corner);
    doc.look(Look::Aim(click_at(9.0, 5.0)));
    doc.look(Look::NextField);
    assert_eq!(doc.drawing_field(), Some(Field::Radius));
    doc.look(Look::ValueInput("2".to_owned()));
    doc.update(Edit::SubmitValue);
    let filleted = sketch(&doc).clone();
    let Curve::Arc { center, .. } = corners(&filleted)[0].curve else {
        panic!("a fillet is an arc");
    };
    assert!(position(&filleted, center).abs_diff_eq(at(8.0, 4.0), 1e-9));
    assert_eq!(undo_to(&mut doc, &before), 1);

    // Past the right side's height, it's too large.
    click_hit(&mut doc, 9.8, 5.9, corner);
    doc.look(Look::Aim(click_at(9.0, 5.0)));
    doc.look(Look::NextField);
    doc.look(Look::ValueInput("7".to_owned()));
    doc.update(Edit::SubmitValue);
    assert_eq!(*sketch(&doc), before);
    let feature = doc.sketch.as_ref().unwrap().feature;
    assert_eq!(
        doc.edit_error,
        Some(DocumentError::Sketch(feature, EditError::NoRoom))
    );
}

#[test]
fn chamfer_cuts_as_far_as_typed_along_each_or_at_an_angle() {
    let (mut doc, _, _) = sketching();
    rectangle(&mut doc);
    let (corner, top, right) = top_right(&doc);
    let before = sketch(&doc).clone();
    // Taken up with the corner's lines selected, it has the corner.
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(right)),
        add: false,
    });
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(top)),
        add: true,
    });
    doc.shift_key("b");
    assert_eq!(
        drawing(&doc).map(|drawing| drawing.tool),
        Some(Tool::Chamfer)
    );
    assert_eq!(drawing(&doc).unwrap().picked, [corner, right, top]);
    // 1 down the right side, 2 along the top.
    doc.look(Look::Aim(click_at(9.0, 5.0)));
    doc.look(Look::NextField);
    assert_eq!(doc.drawing_field(), Some(Field::Distance));
    doc.look(Look::ValueInput("1".to_owned()));
    doc.look(Look::NextField);
    assert_eq!(doc.drawing_field(), Some(Field::SecondDistance));
    doc.look(Look::ValueInput("2".to_owned()));
    doc.update(Edit::SubmitValue);
    let chamfered = sketch(&doc).clone();
    let made = corners(&chamfered);
    assert_eq!(made[0].name(), "Chamfer 1");
    let (start, end) = chamfered.line(made[0].id).unwrap();
    assert!(start.abs_diff_eq(at(10.0, 5.0), 1e-9) && end.abs_diff_eq(at(8.0, 6.0), 1e-9));
    assert_eq!(chamfered.dimensions.len(), 2);
    let analysis = doc.sketch_state().unwrap().analysis.unwrap();
    assert!(analysis.redundant.is_empty());
    assert_eq!(undo_to(&mut doc, &before), 1);

    // An angle typed after the second distance lets go of it: 1 along
    // the top, at 45° to it.
    click_hit(&mut doc, 9.8, 5.9, corner);
    doc.look(Look::Aim(click_at(9.0, 5.0)));
    for (field, text) in [
        (Field::Distance, "1"),
        (Field::SecondDistance, "3"),
        (Field::Angle, "45"),
    ] {
        doc.look(Look::NextField);
        assert_eq!(doc.drawing_field(), Some(field));
        doc.look(Look::ValueInput(text.to_owned()));
    }
    doc.update(Edit::SubmitValue);
    let chamfered = sketch(&doc).clone();
    let (start, end) = chamfered.line(corners(&chamfered)[0].id).unwrap();
    assert!(start.abs_diff_eq(at(9.0, 6.0), 1e-9) && end.abs_diff_eq(at(10.0, 5.0), 1e-9));
    assert!(!corners(&chamfered)[0].corner.unwrap().equal);

    // Cut again, the corner's chamfer is replaced, by one as far along
    // both as the click, placed with `Enter` where it aims.
    click_hit(&mut doc, 9.8, 5.9, corner);
    doc.look(Look::Aim(click_at(9.5, 5.5)));
    doc.update(Edit::PlaceShape);
    let replaced = sketch(&doc).clone();
    let made = corners(&replaced);
    assert_eq!(made.len(), 1);
    assert!(made[0].corner.unwrap().equal);
    let (start, end) = replaced.line(made[0].id).unwrap();
    assert!(start.abs_diff_eq(at(9.0, 6.0), 1e-3) && end.abs_diff_eq(at(10.0, 5.0), 1e-3));

    // Deleting it gives the corner back.
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(made[0].id)),
        add: false,
    });
    doc.update(Edit::DeleteSelection);
    assert!(corners(sketch(&doc)).is_empty());
    assert_eq!(sketch(&doc).curves.len(), before.curves.len());
}

/// A T trimmed to an L: a bar along the x axis from 0 to 10, a stem from
/// (5, 5) down to it, the bar's right part trimmed so it ends at the
/// stem's foot, as the edit before. The bar, the stem and the foot.
fn trimmed_t(doc: &mut Answered) -> (Id, Id, Id) {
    let bar = draw(doc, (0.0, 0.0), (10.0, 0.0));
    doc.look(Look::SelectTool(Tool::Line));
    click(doc, 5.0, 5.0);
    click(doc, 5.0, 0.0);
    doc.look(Look::Escape);
    doc.look(Look::Escape);
    let stem = sketch(doc).curves.last().unwrap().id;
    let foot = sketch(doc).curve(stem).unwrap().curve.ends().unwrap()[1];
    doc.key(letter("t"));
    click_hit(doc, 8.0, 0.0, bar);
    let ends = |doc: &Answered| sketch(doc).curve(bar).unwrap().curve.ends().unwrap();
    assert!(ends(doc).contains(&foot));
    doc.look(Look::Escape);
    (bar, stem, foot)
}

#[test]
fn what_fillet_or_offset_picked_is_let_go_of_once_undo_unmakes_it() {
    let (mut doc, _, _) = sketching();
    let (bar, stem, foot) = trimmed_t(&mut doc);
    let ends = |doc: &Answered| sketch(doc).curve(bar).unwrap().curve.ends().unwrap();
    doc.key(letter("f"));
    click_hit(&mut doc, 4.0, 1.0, foot);
    assert_eq!(drawing(&doc).unwrap().picked.len(), 3);
    // Undone, every item picked is there, but the bar no longer ends at
    // the corner.
    doc.update(Edit::Undo);
    assert!(!ends(&doc).contains(&foot));
    assert!(drawing(&doc).unwrap().picked.is_empty());

    // Nor are the bar and the stem a chain any more.
    doc.update(Edit::Redo);
    assert!(ends(&doc).contains(&foot));
    doc.key(letter("o"));
    click_hit(&mut doc, 2.0, 0.0, bar);
    let mut picked = drawing(&doc).unwrap().picked.clone();
    picked.sort_unstable();
    assert_eq!(picked, [bar, stem]);
    doc.update(Edit::Undo);
    assert!(drawing(&doc).unwrap().picked.is_empty());
}
