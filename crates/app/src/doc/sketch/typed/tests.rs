use std::f64::consts::FRAC_PI_2;

use iced::keyboard::{self, key};
use iced::time::Instant;
use varde_expr::ErrorKind;
use varde_sketch::{Constraint, Curve, Id, Measure, Sketch};
use varde_view::{Edit, Look, Message as Ui, Target, Tool, ToolClick, ValueTarget};

use super::*;
use crate::doc::CAMERA_ANIMATION;
use crate::doc::sketch::Focus;
use crate::doc::sketch::tests::{
    Answered, at, click, click_at, click_on, drawing, position, pressed, sketch, sketching, typing,
    undo_to,
};

/// Moves the cursor to `x`, `y` with the tool in use, snapping to
/// nothing, as the viewport tells the app while its shape has fields.
fn aim(doc: &mut Answered, x: f64, y: f64) {
    doc.look(Look::Aim(click_at(x, y)));
}

/// The field the value field is open on, if it's a drawing tool's.
fn open(doc: &Doc) -> Option<Field> {
    doc.drawing_field()
}

/// Types `text` in the value field over what's there, then presses
/// `Tab`.
fn tab_in(doc: &mut Answered, text: &str) {
    doc.look(Look::ValueInput(text.to_owned()));
    doc.look(Look::NextField);
}

/// Types `text` in the value field over what's there, then presses
/// `Enter`.
fn enter_in(doc: &mut Answered, text: &str) {
    doc.look(Look::ValueInput(text.to_owned()));
    doc.update(Edit::SubmitValue);
}

/// The degrees of freedom the solver found the sketch committed has.
fn freedom(doc: &Doc) -> usize {
    let session = doc.sketch.as_ref().unwrap();
    session.analysis(doc.editor.revision()).unwrap().freedom
}

/// The constraints of `sketch`, but those the snapping made.
fn constraints(sketch: &Sketch) -> Vec<Constraint> {
    let constraints = sketch.constraints.iter();
    constraints.map(|entry| entry.constraint.clone()).collect()
}

/// Each driving dimension's measure and value as typed.
fn dimensions(sketch: &Sketch) -> Vec<(Measure, String)> {
    let dimensions = sketch.dimensions.iter().map(|entry| &entry.dimension);
    dimensions
        .inspect(|dimension| assert!(dimension.driving))
        .map(|dimension| (dimension.measure.clone(), dimension.value.text.clone()))
        .collect()
}

/// The lines of `sketch`, each with its ends.
fn lines(sketch: &Sketch) -> Vec<(Id, Id, Id)> {
    let lines = sketch.curves.iter().filter_map(|entry| match entry.curve {
        Curve::Line { start, end } => Some((entry.id, start, end)),
        _ => None,
    });
    lines.collect()
}

#[test]
fn a_line_is_placed_at_the_length_and_angle_typed_with_their_dimensions() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    // No fields before the first point.
    doc.look(Look::NextField);
    assert_eq!(open(&doc), None);
    click(&mut doc, 1.0, 1.0);
    aim(&mut doc, 4.0, 5.0);

    // Tab opens the first field, empty to type in, with the focus.
    doc.look(Look::NextField);
    assert_eq!(open(&doc), Some(Field::Length));
    assert_eq!(
        doc.sketch.as_ref().unwrap().value.as_ref().unwrap().text,
        ""
    );
    assert_eq!(doc.take_focus(), Some(Focus::All));
    // Typed and tabbed out of, the value holds.
    tab_in(&mut doc, "40 / 2");
    assert_eq!(open(&doc), Some(Field::Angle));
    let typed = &drawing(&doc).unwrap().typed;
    assert_eq!(typed.len(), 1);
    assert_eq!(typed[0].0, Field::Length);
    assert_eq!(typed[0].1.value, 20.0);
    // The cursor moves only what isn't typed: the length stays.
    aim(&mut doc, 1.0, 9.0);
    let state = doc.sketch_state().unwrap();
    let tool = state.tool.unwrap();
    let preview = varde_view::typed::outline(&tool, at(1.0, 9.0)).unwrap();
    assert_eq!(preview.value(Field::Length), Some(20.0));
    assert!(sketch(&doc).points.is_empty(), "nothing until it's placed");

    // Enter takes the angle and places the line, as one step.
    enter_in(&mut doc, "90 deg");
    assert_eq!(open(&doc), None);
    let drawn = sketch(&doc).clone();
    let [(line, start, end)] = lines(&drawn)[..] else {
        panic!("{drawn:?}")
    };
    assert_eq!(position(&drawn, start), at(1.0, 1.0));
    assert!(position(&drawn, end).abs_diff_eq(at(1.0, 21.0), 1e-9));
    assert_eq!(
        dimensions(&drawn),
        [
            (Measure::Length(line), "40 / 2".to_owned()),
            (Measure::Angle(Id::X_AXIS, line), "90 deg".to_owned()),
        ]
    );
    let angle = &drawn.dimensions[1].dimension;
    assert!((angle.value.value - FRAC_PI_2).abs() < 1e-12);
    // Only its start is free.
    assert_eq!(freedom(&doc), 2);
    // The chain goes on from its end, nothing typed.
    let tool = drawing(&doc).unwrap();
    assert_eq!(tool.placed.len(), 1);
    assert!(tool.typed.is_empty());
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 1);
}

#[test]
fn a_value_refused_keeps_its_field_saying_why() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Circle));
    click(&mut doc, 0.0, 0.0);
    aim(&mut doc, 3.0, 0.0);
    doc.look(Look::NextField);
    doc.take_focus();
    tab_in(&mut doc, "2 + 3 deg");
    // Still there, the part it's about selected.
    assert_eq!(open(&doc), Some(Field::Diameter));
    let field = doc.sketch.as_ref().unwrap().value.clone().unwrap();
    assert!(matches!(field.error.unwrap().kind, ErrorKind::Wrong { .. }));
    assert_eq!(doc.take_focus(), Some(Focus::Range(0, 9)));
    assert!(drawing(&doc).unwrap().typed.is_empty());
    // Enter and a click place nothing either.
    enter_in(&mut doc, "abc");
    assert_eq!(open(&doc), Some(Field::Diameter));
    click(&mut doc, 3.0, 0.0);
    assert!(sketch(&doc).curves.is_empty());
    assert_eq!(open(&doc), Some(Field::Diameter));

    // A click takes a value typed, not tabbed out of: the diameter holds
    // wherever it lands.
    doc.look(Look::ValueInput("8".to_owned()));
    click(&mut doc, 30.0, 0.0);
    assert_eq!(open(&doc), None);
    let drawn = sketch(&doc);
    let circle = drawn.curves[0].id;
    assert!((drawn.round(circle).unwrap().1 - 4.0).abs() < 1e-9);
    assert_eq!(
        dimensions(drawn),
        [(Measure::Diameter(circle), "8".to_owned())]
    );
    assert_eq!(freedom(&doc), 2);
}

#[test]
fn a_value_typed_takes_the_point_off_what_it_snapped_to() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Point));
    click(&mut doc, 10.0, 0.0);
    let lone = sketch(&doc).points[0].id;
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    aim(&mut doc, 10.0, 0.0);
    doc.look(Look::NextField);
    doc.look(Look::ValueInput("4".to_owned()));
    // Clicked on the lone point, the line ends 4 along towards it, not
    // on it.
    click_on(&mut doc, 10.0, 0.0, Some(lone));
    let drawn = sketch(&doc).clone();
    let [(_, _, end)] = lines(&drawn)[..] else {
        panic!("{drawn:?}")
    };
    assert_ne!(end, lone);
    assert_eq!(position(&drawn, end), at(4.0, 0.0));
    assert_eq!(drawn.points.len(), 3);
}

#[test]
fn escape_closes_the_field_then_drops_the_values_with_the_shape() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    aim(&mut doc, 1.0, 0.0);
    doc.look(Look::NextField);
    tab_in(&mut doc, "5");
    assert_eq!(open(&doc), Some(Field::Angle));
    // Round again, the field shows what was typed in it.
    tab_in(&mut doc, "");
    assert_eq!(open(&doc), Some(Field::Length));
    assert_eq!(
        doc.sketch.as_ref().unwrap().value.as_ref().unwrap().text,
        "5"
    );
    doc.look(Look::CancelValue);
    assert_eq!(open(&doc), None);
    assert_eq!(drawing(&doc).unwrap().typed.len(), 1);
    // Enter places it with the value typed, at the cursor.
    doc.update(Edit::PlaceShape);
    assert!(position(sketch(&doc), lines(sketch(&doc))[0].2).abs_diff_eq(at(5.0, 0.0), 1e-12));

    // Emptied, a field lets go of its value.
    aim(&mut doc, 5.0, 3.0);
    doc.look(Look::NextField);
    tab_in(&mut doc, "7");
    doc.look(Look::NextField);
    tab_in(&mut doc, "");
    assert!(drawing(&doc).unwrap().typed.is_empty());
    doc.look(Look::NextField);
    tab_in(&mut doc, "7");
    doc.look(Look::Escape);
    assert_eq!(open(&doc), None);
    assert_eq!(drawing(&doc).unwrap().typed.len(), 1);
    doc.look(Look::Escape);
    let tool = drawing(&doc).unwrap();
    assert!(tool.placed.is_empty() && tool.typed.is_empty());
}

#[test]
fn a_rectangle_comes_with_shared_corners_held_level_and_upright() {
    let (mut doc, _, _) = sketching();
    doc.key(keyboard::Key::Character("r".into()));
    assert_eq!(drawing(&doc).unwrap().tool, Tool::Rectangle);
    click(&mut doc, 1.0, 1.0);
    // Too flat a rectangle is refused.
    click(&mut doc, 5.0, 1.05);
    assert!(sketch(&doc).curves.is_empty());
    click(&mut doc, 5.0, -2.0);
    let drawn = sketch(&doc).clone();
    let lines = lines(&drawn);
    assert_eq!(lines.len(), 4);
    assert_eq!(drawn.points.len(), 4, "corners shared");
    for (k, &(_, _, end)) in lines.iter().enumerate() {
        assert_eq!(end, lines[(k + 1) % 4].1);
    }
    let at_of = |id| position(&drawn, id);
    assert_eq!(
        lines
            .iter()
            .map(|&(_, start, _)| at_of(start))
            .collect::<Vec<_>>(),
        [at(1.0, 1.0), at(5.0, 1.0), at(5.0, -2.0), at(1.0, -2.0)]
    );
    let [a, b, c, d] = [0, 1, 2, 3].map(|k| lines[k].0);
    assert_eq!(
        constraints(&drawn),
        [
            Constraint::Horizontal(a),
            Constraint::Vertical(b),
            Constraint::Horizontal(c),
            Constraint::Vertical(d),
        ]
    );
    // Where it is and its size are free.
    assert_eq!(freedom(&doc), 4);
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 1);
    assert!(
        drawing(&doc).unwrap().placed.is_empty(),
        "ready for the next"
    );
}

#[test]
fn a_rectangle_s_width_and_height_typed_are_its_dimensions() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Rectangle));
    click(&mut doc, 0.0, 0.0);
    aim(&mut doc, -1.0, 1.0);
    doc.look(Look::NextField);
    assert_eq!(open(&doc), Some(Field::Width));
    tab_in(&mut doc, "30");
    assert_eq!(open(&doc), Some(Field::Height));
    enter_in(&mut doc, "1 cm");
    let drawn = sketch(&doc).clone();
    let lines = lines(&drawn);
    // Up and left, where the cursor was.
    let corners: Vec<_> = lines
        .iter()
        .map(|&(_, start, _)| position(&drawn, start))
        .collect();
    assert_eq!(
        corners,
        [at(0.0, 0.0), at(-30.0, 0.0), at(-30.0, 10.0), at(0.0, 10.0)]
    );
    assert_eq!(
        dimensions(&drawn),
        [
            (Measure::Length(lines[0].0), "30".to_owned()),
            (Measure::Length(lines[3].0), "1 cm".to_owned()),
        ]
    );
    // Only where it is is free.
    assert_eq!(freedom(&doc), 2);
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 1);
}

#[test]
fn a_rectangle_from_its_centre_has_the_centre_on_a_construction_diagonal() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Rectangle));
    doc.key(keyboard::Key::Character("q".into()));
    assert!(drawing(&doc).unwrap().centered);
    // Snapped to the origin, the centre is held there.
    click_on(&mut doc, 0.0, 0.0, Some(Id::ORIGIN));
    click(&mut doc, 3.0, 2.0);
    let drawn = sketch(&doc).clone();
    assert_eq!(drawn.points.len(), 5);
    let all = lines(&drawn);
    let [.., (diagonal, start, end)] = all[..] else {
        panic!("{drawn:?}")
    };
    assert!(drawn.curve(diagonal).unwrap().construction);
    assert_eq!(position(&drawn, start), at(-3.0, -2.0));
    assert_eq!(position(&drawn, end), at(3.0, 2.0));
    let center = drawn.points[0].id;
    assert_eq!(position(&drawn, center), at(0.0, 0.0));
    let made = constraints(&drawn);
    assert!(made.contains(&Constraint::Midpoint {
        point: center,
        line: diagonal
    }));
    assert!(made.contains(&Constraint::Coincident(center, Id::ORIGIN)));
    // Its size is free, and nothing is redundant.
    assert_eq!(freedom(&doc), 2);
    let session = doc.sketch.as_ref().unwrap();
    assert!(
        session
            .analysis(doc.editor.revision())
            .unwrap()
            .redundant
            .is_empty()
    );
    // Still from the centre, until Q again.
    assert!(drawing(&doc).unwrap().centered);
    doc.look(Look::ToggleCentered);
    assert!(!drawing(&doc).unwrap().centered);
}

#[test]
fn a_rectangle_s_corners_snap_as_a_shape_s_points_do() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Point));
    click(&mut doc, 4.0, 3.0);
    let lone = sketch(&doc).points[0].id;
    doc.look(Look::SelectTool(Tool::Rectangle));
    // On the X axis, then on the lone point, which is its corner.
    doc.update(Edit::ToolClick(ToolClick {
        target: Some(Target::On(Id::X_AXIS)),
        ..click_at(1.0, 0.0)
    }));
    click_on(&mut doc, 4.0, 3.0, Some(lone));
    let drawn = sketch(&doc).clone();
    assert_eq!(drawn.points.len(), 4);
    let lines = lines(&drawn);
    assert_eq!(lines[1].2, lone, "the corner clicked is the point");
    let first = lines[0].1;
    assert!(constraints(&drawn).contains(&Constraint::PointOnCurve {
        point: first,
        curve: Id::X_AXIS
    }));
    // The lone point had two, the axis takes one.
    assert_eq!(freedom(&doc), 3);
}

#[test]
fn a_polygon_has_equal_sides_on_its_construction_circle() {
    let (mut doc, _, _) = sketching();
    doc.key(keyboard::Key::Character("g".into()));
    assert_eq!(drawing(&doc).unwrap().tool, Tool::Polygon);
    click(&mut doc, 1.0, 1.0);
    click(&mut doc, 1.0, 5.0);
    let drawn = sketch(&doc).clone();
    let circle = drawn
        .curves
        .iter()
        .find(|entry| entry.curve.kind() == varde_sketch::Kind::Circle)
        .unwrap();
    assert!(circle.construction);
    let Curve::Circle { center, radius } = circle.curve else {
        unreachable!()
    };
    assert_eq!(position(&drawn, center), at(1.0, 1.0));
    assert_eq!(radius, 4.0);
    let sides = lines(&drawn);
    assert_eq!(sides.len(), 6);
    assert_eq!(drawn.points.len(), 7);
    let made = constraints(&drawn);
    for &(_, start, _) in &sides {
        assert!(made.contains(&Constraint::PointOnCurve {
            point: start,
            curve: circle.id
        }));
        assert!((position(&drawn, start).distance(at(1.0, 1.0)) - 4.0).abs() < 1e-9);
    }
    for &(line, ..) in &sides[1..] {
        assert!(made.contains(&Constraint::Equal(sides[0].0, line)));
    }
    assert_eq!(made.len(), 11);
    // Where it is, its size and how it's turned.
    assert_eq!(freedom(&doc), 4);
    assert_eq!(undo_to(&mut doc, &Sketch::default()), 1);
}

#[test]
fn a_polygon_s_sides_are_typed_within_their_bounds_and_kept() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Polygon));
    click(&mut doc, 0.0, 0.0);
    aim(&mut doc, 5.0, 0.0);
    doc.look(Look::NextField);
    assert_eq!(open(&doc), Some(Field::Sides));
    for (text, refused) in [
        ("2", "must be at least 3"),
        ("65", "must be at most 64"),
        ("4.5", "must be a whole number"),
    ] {
        tab_in(&mut doc, text);
        let field = doc.sketch.as_ref().unwrap().value.clone().unwrap();
        assert_eq!(field.error.unwrap().to_string(), refused, "{text}");
        assert_eq!(drawing(&doc).unwrap().sides, 6);
    }
    tab_in(&mut doc, "5");
    assert_eq!(drawing(&doc).unwrap().sides, 5);
    assert_eq!(open(&doc), Some(Field::Diameter));
    enter_in(&mut doc, "20");
    let drawn = sketch(&doc).clone();
    assert_eq!(lines(&drawn).len(), 5);
    let circle = drawn
        .curves
        .iter()
        .find(|entry| entry.construction)
        .unwrap()
        .id;
    assert_eq!(
        dimensions(&drawn),
        [(Measure::Diameter(circle), "20".to_owned())]
    );
    assert_eq!(freedom(&doc), 3);
    // The next polygon has as many sides, as does one sides were left
    // empty for.
    click(&mut doc, 50.0, 0.0);
    aim(&mut doc, 55.0, 0.0);
    doc.look(Look::NextField);
    tab_in(&mut doc, "");
    assert_eq!(drawing(&doc).unwrap().sides, 5);
    click(&mut doc, 55.0, 0.0);
    assert_eq!(lines(sketch(&doc)).len(), 10);
}

#[test]
fn an_arc_s_radius_typed_holds_it_and_one_short_of_its_ends_is_refused() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Arc));
    click(&mut doc, -3.0, 0.0);
    click(&mut doc, 3.0, 0.0);
    aim(&mut doc, 0.0, 1.0);
    doc.look(Look::NextField);
    assert_eq!(open(&doc), Some(Field::Radius));
    tab_in(&mut doc, "2");
    let field = doc.sketch.as_ref().unwrap().value.clone().unwrap();
    assert!(matches!(
        field.error.unwrap().kind,
        ErrorKind::TooSmall { .. }
    ));
    enter_in(&mut doc, "5");
    let drawn = sketch(&doc).clone();
    let arc = drawn.curves[0].id;
    let (center, radius) = drawn.round(arc).unwrap();
    assert!((radius - 5.0).abs() < 1e-9);
    assert!(
        center.abs_diff_eq(at(0.0, -4.0), 1e-9),
        "the short way round"
    );
    assert_eq!(dimensions(&drawn), [(Measure::Radius(arc), "5".to_owned())]);
}

#[test]
fn typing_in_a_drawing_tool_s_field_fires_no_shortcuts_but_tab() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    aim(&mut doc, 1.0, 1.0);
    doc.look(Look::NextField);
    // The fields show where the camera, settled, sees the cursor.
    doc.animation_frame(Instant::now() + 2 * CAMERA_ANIMATION);
    let character = |c: &str| typing(keyboard::Key::Character(c.into()), Some(c));
    let keys = [
        character("r"),
        character("g"),
        character("q"),
        character("x"),
    ];
    let (sent, shortcuts) = pressed(&doc, &keys, true);
    assert!(shortcuts.is_empty(), "{shortcuts:?}");
    assert_eq!(sent.len(), 4, "{sent:?}");
    // Tab is the app's, moving to the next field; Enter the field's.
    let tab = typing(keyboard::Key::Named(key::Named::Tab), None);
    let (_, shortcuts) = pressed(&doc, &[tab], true);
    assert!(
        matches!(
            shortcuts.as_slice(),
            [crate::Message::Ui(Ui::Look(Look::NextField))]
        ),
        "{shortcuts:?}"
    );
    let enter = typing(keyboard::Key::Named(key::Named::Enter), None);
    let (sent, shortcuts) = pressed(&doc, &[enter], true);
    assert!(shortcuts.is_empty());
    assert!(matches!(sent.as_slice(), [Ui::Edit(Edit::SubmitValue)]));
}

#[test]
fn a_field_goes_with_the_shape_having_it() {
    let (mut doc, _, _) = sketching();
    doc.look(Look::SelectTool(Tool::Line));
    click(&mut doc, 0.0, 0.0);
    aim(&mut doc, 1.0, 1.0);
    doc.look(Look::NextField);
    assert!(matches!(
        doc.sketch.as_ref().unwrap().value.as_ref().unwrap().target,
        ValueTarget::Field(Field::Length)
    ));
    // Another tool closes it.
    doc.look(Look::SelectTool(Tool::Circle));
    assert_eq!(open(&doc), None);
}
