use iced::keyboard::{self, key};
use iced::time::{Duration, Instant};
use iced::{Event, Size, mouse};
use iced_runtime::user_interface::{Cache, UserInterface};
use varde_expr::LengthUnit;
use varde_sketch::{Curve, Measure, Rejected, Side};
use varde_view::{ConstraintKind, Edit, Look, Message as Ui, Mode, Tool};

use super::*;
use crate::Message;
use crate::doc::CAMERA_ANIMATION;
use crate::doc::sketch::tests::{
    Answered, at, click, click_at, drawing, position, selection, sketch, sketching, undo_to,
};
use crate::doc::sketch::{Refusal, Sketch};
use crate::tests::{pressed, typing};

/// Clicks the tool in use at `x`, `y` on `hit`, with the reference
/// modifier held if `reference`.
fn tool_click(doc: &mut Answered, x: f64, y: f64, hit: Option<Id>, reference: bool) {
    doc.update(Edit::ToolClick(ToolClick {
        hit,
        reference,
        ..click_at(x, y)
    }));
}

/// Picks `id` with the Dimension tool, clicking at `x`, `y`.
fn pick(doc: &mut Answered, x: f64, y: f64, id: Id) {
    tool_click(doc, x, y, Some(id), false);
}

/// Places the dimension of what's picked with its label at `x`, `y`.
fn place(doc: &mut Answered, x: f64, y: f64) {
    tool_click(doc, x, y, None, false);
}

/// Draws a line from (`x0`, `y0`) to (`x1`, `y1`) with the Line tool,
/// then puts the tool down: its ends and its id.
fn draw_line(doc: &mut Answered, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> [Id; 3] {
    doc.look(Look::SelectTool(Tool::Line));
    click(doc, x0, y0);
    click(doc, x1, y1);
    doc.look(Look::SelectTool(Tool::Line));
    let sketch = sketch(doc);
    let entry = sketch.curves.last().unwrap();
    let Curve::Line { start, end } = entry.curve else {
        unreachable!()
    };
    [start, end, entry.id]
}

/// A sketch with a line from (0, 0) to (10, 0) and the Dimension tool in
/// use: the line's ends and its id.
fn line_to_dimension() -> (Answered, [Id; 3]) {
    let (mut doc, _, _) = sketching();
    let line = draw_line(&mut doc, (0.0, 0.0), (10.0, 0.0));
    doc.look(Look::SelectTool(Tool::Dimension));
    (doc, line)
}

/// The value field, if it's open.
fn field(doc: &Doc) -> Option<&ValueEdit> {
    doc.sketch.as_ref()?.value.as_ref()
}

/// Types `text` in the value field over what's there, and takes it.
fn enter(doc: &mut Answered, text: &str) {
    doc.look(Look::ValueInput(text.to_owned()));
    doc.update(Edit::SubmitValue);
}

fn length(sketch: &Sketch, [start, end, _]: [Id; 3]) -> f64 {
    position(sketch, start).distance(position(sketch, end))
}

#[test]
fn a_line_is_dimensioned_with_the_value_typed_as_one_step() {
    let (mut doc, line) = line_to_dimension();
    let before = sketch(&doc).clone();
    pick(&mut doc, 5.0, 0.0, line[2]);
    assert_eq!(drawing(&doc).unwrap().picked, [line[2]]);
    place(&mut doc, 5.0, 3.0);
    // The field shows what it measures now, ready to overtype.
    let placing = field(&doc).unwrap();
    assert_eq!(
        placing.target,
        ValueTarget::New {
            measure: Measure::Length(line[2]),
            side: Side::Positive,
            label: at(0.0, 3.0),
        }
    );
    assert_eq!(placing.text, "10 mm");
    assert!(!placing.in_list);
    assert_eq!(doc.take_focus(), Some(Focus::All));
    assert!(drawing(&doc).unwrap().picked.is_empty());
    assert!(sketch(&doc).dimensions.is_empty(), "nothing until Enter");

    enter(&mut doc, "25");
    assert!(field(&doc).is_none());
    let dimension = &sketch(&doc).dimensions[0].dimension;
    assert_eq!(dimension.value.text, "25");
    assert_eq!(dimension.value.value, 25.0);
    assert!(dimension.driving);
    assert!((length(sketch(&doc), line) - 25.0).abs() < 1e-9);
    assert_eq!(
        drawing(&doc).unwrap().tool,
        Tool::Dimension,
        "the tool stays"
    );
    assert_eq!(undo_to(&mut doc, &before), 1);
}

#[test]
fn picks_join_what_they_measure_with_and_start_afresh_otherwise() {
    let (mut doc, [start, end, line]) = line_to_dimension();
    doc.look(Look::SelectTool(Tool::Circle));
    click(&mut doc, 20.0, 0.0);
    click(&mut doc, 23.0, 0.0);
    let circle = sketch(&doc).curves.last().unwrap().id;
    doc.look(Look::SelectTool(Tool::Dimension));

    // Two points: the distance between them.
    pick(&mut doc, 0.0, 0.0, start);
    // A line's own end is on it, so its end starts afresh.
    pick(&mut doc, 10.0, 0.0, line);
    pick(&mut doc, 0.0, 0.0, start);
    assert_eq!(drawing(&doc).unwrap().picked, [start]);
    // Nothing placed with a point alone.
    place(&mut doc, 5.0, 3.0);
    assert!(field(&doc).is_none());
    pick(&mut doc, 10.0, 0.0, end);
    place(&mut doc, 5.0, 3.0);
    let target = |doc: &Doc| match &field(doc).unwrap().target {
        ValueTarget::New { measure, .. } => measure.clone(),
        ValueTarget::Dimension(_) | ValueTarget::Field(_) => unreachable!(),
    };
    assert_eq!(target(&doc), Measure::Distance(start, end));

    // A circle doesn't join a point: it starts afresh, and is measured by
    // its diameter, or switched, its radius.
    pick(&mut doc, 0.0, 0.0, start);
    pick(&mut doc, 23.0, 0.0, circle);
    assert_eq!(drawing(&doc).unwrap().picked, [circle]);
    doc.look(Look::SwitchRound);
    place(&mut doc, 26.0, 3.0);
    assert_eq!(target(&doc), Measure::Radius(circle));
    assert_eq!(field(&doc).unwrap().text, "3 mm");
    pick(&mut doc, 23.0, 0.0, circle);
    place(&mut doc, 26.0, 3.0);
    assert_eq!(target(&doc), Measure::Diameter(circle), "switched back");
}

#[test]
fn values_are_expressions_kept_as_typed_and_refused_with_where() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    let _ = doc.take_focus();

    enter(&mut doc, "1 mm + 2 deg");
    let refused = field(&doc).expect("still open");
    let error = refused.error.as_ref().unwrap();
    assert_eq!(error.to_string(), "can't add a length and an angle");
    assert_eq!(doc.take_focus(), Some(Focus::Range(0, 12)));
    assert!(sketch(&doc).dimensions.is_empty());
    enter(&mut doc, "10 * (2");
    assert_eq!(
        field(&doc).unwrap().error.as_ref().unwrap().to_string(),
        "missing ')'"
    );
    // Typing again takes the error away.
    doc.look(Look::ValueInput("1 in + 3".to_owned()));
    assert!(field(&doc).unwrap().error.is_none());

    doc.update(Edit::SubmitValue);
    let dimension = &sketch(&doc).dimensions[0];
    assert_eq!(dimension.dimension.value.text, "1 in + 3");
    assert!((dimension.dimension.value.value - 28.4).abs() < 1e-9);
    assert!((length(sketch(&doc), line) - 28.4).abs() < 1e-9);

    // Double-clicked, it shows the expression again; set to something
    // else, the geometry follows.
    let id = dimension.id;
    doc.look(Look::EditDimension { id, in_list: false });
    let editing = field(&doc).unwrap();
    assert_eq!(editing.target, ValueTarget::Dimension(id));
    assert_eq!(editing.text, "1 in + 3");
    assert_eq!(doc.take_focus(), Some(Focus::All));
    enter(&mut doc, "40 / 2");
    assert!(field(&doc).is_none());
    assert!((length(sketch(&doc), line) - 20.0).abs() < 1e-9);
    assert_eq!(sketch(&doc).dimensions[0].dimension.value.text, "40 / 2");

    // From the Constraints list, in the list; the same value changes
    // nothing.
    let revision = doc.editor.revision();
    doc.look(Look::EditDimension { id, in_list: true });
    assert!(field(&doc).unwrap().in_list);
    doc.update(Edit::SubmitValue);
    assert!(field(&doc).is_none());
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn escape_closes_the_field_then_drops_the_picks_then_the_tool() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    doc.look(Look::CancelValue);
    assert!(field(&doc).is_none());
    assert!(sketch(&doc).dimensions.is_empty());

    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    doc.look(Look::Escape);
    assert!(field(&doc).is_none());
    pick(&mut doc, 5.0, 0.0, line[2]);
    doc.look(Look::Escape);
    assert!(drawing(&doc).unwrap().picked.is_empty());
    doc.look(Look::Escape);
    assert!(drawing(&doc).is_none());
    assert!(doc.sketch.is_some());

    // Acting elsewhere closes it too.
    doc.look(Look::EditDimension {
        id: line[2],
        in_list: false,
    });
    assert!(field(&doc).is_none(), "a line has no value to edit");
    doc.look(Look::SelectTool(Tool::Dimension));
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    doc.look(Look::ClickGeometry {
        hit: None,
        add: false,
    });
    assert!(field(&doc).is_none());
}

#[test]
fn a_click_with_the_reference_modifier_places_a_reference_at_once() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    tool_click(&mut doc, 5.0, 3.0, None, true);
    assert!(field(&doc).is_none());
    let dimension = &sketch(&doc).dimensions[0].dimension;
    assert!(!dimension.driving);
    assert_eq!(dimension.value.value, 10.0);
    // It adds no equation: the line is as free as it was.
    let freedom = |doc: &Doc| {
        let session = doc.sketch.as_ref().unwrap();
        session.analysis(doc.editor.revision()).unwrap().freedom
    };
    assert_eq!(freedom(&doc), 4);
}

#[test]
fn selected_dimensions_turn_between_driving_and_reference() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "10");
    let id = sketch(&doc).dimensions[0].id;
    doc.look(Look::ClickGeometry {
        hit: Some(id),
        add: false,
    });
    assert_eq!(selection(&doc), [id]);
    let driving = |doc: &Doc| sketch(doc).dimensions[0].dimension.driving;

    // Its key, Shift D.
    let shift_d = crate::tests::press(
        keyboard::Key::Character("D".into()),
        keyboard::Modifiers::SHIFT,
    );
    let pressed = crate::keys::document_key((doc.keys(), shift_d));
    assert!(matches!(
        pressed,
        Some(Message::Ui(Ui::Edit(Edit::ToggleReference)))
    ));
    doc.update(Edit::ToggleReference);
    assert!(!driving(&doc));
    doc.update(Edit::ToggleReference);
    assert!(driving(&doc));
}

#[test]
fn a_driving_dimension_over_constraining_is_refused_and_can_be_a_reference() {
    let (mut doc, line) = line_to_dimension();
    doc.look(Look::ClickGeometry {
        hit: Some(line[2]),
        add: false,
    });
    doc.update(Edit::Constrain(ConstraintKind::Fix));
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "10");
    assert!(sketch(&doc).dimensions.is_empty());
    let refusal = doc.sketch.as_ref().unwrap().refusal.as_ref();
    assert!(matches!(
        refusal,
        Some(Refusal::Rejected(Rejected::Driving { .. }))
    ));
    // As the hint says, placed as a reference it's accepted.
    pick(&mut doc, 5.0, 0.0, line[2]);
    tool_click(&mut doc, 5.0, 3.0, None, true);
    assert!(!sketch(&doc).dimensions[0].dimension.driving);

    // Made driving, it's refused, and stays a reference.
    let id = sketch(&doc).dimensions[0].id;
    doc.look(Look::ClickGeometry {
        hit: Some(id),
        add: false,
    });
    doc.update(Edit::ToggleReference);
    assert!(!sketch(&doc).dimensions[0].dimension.driving);
    let refusal = doc.sketch.as_ref().unwrap().refusal.as_ref();
    assert!(matches!(
        refusal,
        Some(Refusal::Rejected(Rejected::Driving { dimensions, .. })) if dimensions.contains(&id)
    ));
}

#[test]
fn a_label_is_dragged_and_dropped_as_one_step() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "10");
    let before = sketch(&doc).clone();
    let id = before.dimensions[0].id;
    doc.look(Look::PressLabel { id, add: false });
    assert_eq!(selection(&doc), [id], "pressing selects it");
    doc.look(Look::DragLabel {
        id,
        from: at(5.0, 3.0),
        to: at(7.0, 6.0),
    });
    let state = doc.sketch_state().unwrap();
    assert_eq!(state.label_drag, Some((id, at(2.0, 3.0))));
    doc.update(Edit::DropLabel);
    assert_eq!(sketch(&doc).dimensions[0].dimension.label, at(2.0, 6.0));
    assert_eq!(doc.sketch_state().unwrap().label_drag, None);
    assert_eq!(undo_to(&mut doc, &before), 1);

    // Esc puts it back; a press without a drag moves nothing.
    doc.look(Look::PressLabel { id, add: false });
    doc.look(Look::DragLabel {
        id,
        from: at(5.0, 3.0),
        to: at(7.0, 6.0),
    });
    doc.look(Look::Escape);
    doc.update(Edit::DropLabel);
    doc.look(Look::PressLabel { id, add: false });
    doc.update(Edit::DropLabel);
    assert_eq!(sketch(&doc), &before);
}

#[test]
fn units_change_how_dimensions_show_not_their_values() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "25.4");
    let before = sketch(&doc).clone();
    let label = |doc: &Doc| {
        let entry = &sketch(doc).dimensions[0];
        let units = doc.editor.document().units();
        varde_view::dimension::label(sketch(doc), &entry.dimension, units)
    };
    assert_eq!(label(&doc), "25.4 mm");

    doc.update(Edit::SetUnits(LengthUnit::In));
    assert_eq!(doc.editor.document().units(), LengthUnit::In);
    assert_eq!(label(&doc), "1 in");
    // The bare number keeps meaning millimetres.
    let value = &sketch(&doc).dimensions[0].dimension.value;
    assert_eq!((value.text.as_str(), value.value), ("25.4 mm", 25.4));
    assert!((length(sketch(&doc), line) - 25.4).abs() < 1e-9);
    // New values are read in inches, and shown so.
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, -3.0);
    assert_eq!(field(&doc).unwrap().text, "1 in");
    doc.look(Look::CancelValue);
    let id = sketch(&doc).dimensions[0].id;
    doc.look(Look::EditDimension { id, in_list: false });
    enter(&mut doc, "2");
    assert!((length(sketch(&doc), line) - 50.8).abs() < 1e-9);
    assert_eq!(label(&doc), "2 in");

    doc.update(Edit::Undo);
    doc.update(Edit::Undo);
    assert_eq!(sketch(&doc), &before);
    assert_eq!(doc.editor.document().units(), LengthUnit::Mm);
}

#[test]
fn the_field_goes_with_the_dimension_it_edits() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "10");
    let id = sketch(&doc).dimensions[0].id;
    doc.look(Look::EditDimension { id, in_list: false });
    // Undone from elsewhere (the toolbar's button), it's gone.
    doc.update(Edit::Undo);
    assert!(sketch(&doc).dimensions.is_empty());
    assert!(field(&doc).is_none());
    // A drawing tool's click leaves it.
    doc.update(Edit::Redo);
    doc.look(Look::EditDimension { id, in_list: false });
    doc.look(Look::SelectTool(Tool::Line));
    assert!(field(&doc).is_none());
}

#[test]
fn the_peek_key_places_references_in_the_dimension_tool_rather_than_peeking() {
    let (mut doc, _) = line_to_dimension();
    assert!(!doc.peeks(true));
    // The tool put down, it peeks while held.
    doc.look(Look::SelectTool(Tool::Dimension));
    assert!(doc.peeks(true));
    assert!(!doc.peeks(false));
}

#[test]
fn typing_in_the_value_field_fires_no_shortcuts() {
    let (mut doc, line) = line_to_dimension();
    doc.look(Look::ClickGeometry {
        hit: Some(line[2]),
        add: false,
    });
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    // The field shows where the camera, settled, sees the label.
    doc.animation_frame(Instant::now() + 2 * CAMERA_ANIMATION);
    let character = |c: &str| typing(keyboard::Key::Character(c.into()), Some(c));
    let named = |named, text| typing(keyboard::Key::Named(named), text);
    let keys = [
        character("d"),
        character("l"),
        character("x"),
        named(key::Named::Space, Some(" ")),
        named(key::Named::Delete, None),
        named(key::Named::Backspace, None),
    ];

    // Unfocused, the keys are the shortcuts'.
    let (_, shortcuts) = pressed(&doc, &keys, false);
    assert!(
        shortcuts.len() >= 4,
        "{shortcuts:?}: D, L, Space and Delete at least"
    );

    // Focused, they're the field's.
    let (sent, shortcuts) = pressed(&doc, &keys, true);
    assert!(shortcuts.is_empty(), "{shortcuts:?}");
    let typed: Vec<_> = sent
        .iter()
        .filter_map(|message| match message {
            Ui::Look(Look::ValueInput(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    // The value it opened with overtyped.
    assert_eq!(typed, ["d", "dl", "dlx", "dlx ", "dlx ", "dlx"], "{sent:?}");

    // Enter takes the value, Esc closes the field: neither reaches the
    // app's `Esc` or Enter.
    let (sent, shortcuts) = pressed(&doc, &[named(key::Named::Enter, None)], true);
    assert!(shortcuts.is_empty());
    assert!(matches!(sent.as_slice(), [Ui::Edit(Edit::SubmitValue)]));
    let (sent, shortcuts) = pressed(&doc, &[named(key::Named::Escape, None)], true);
    assert!(shortcuts.is_empty());
    assert!(matches!(sent.as_slice(), [Ui::Look(Look::CancelValue)]));
}

#[test]
fn a_dimension_placed_asks_for_its_measure_s_kind_of_value() {
    // An angle between two lines, typed in degrees by default.
    let (mut doc, _, _) = sketching();
    let [_, _, a] = draw_line(&mut doc, (0.0, 0.0), (10.0, 0.0));
    let [_, _, b] = draw_line(&mut doc, (0.0, 1.0), (0.0, 10.0));
    doc.look(Look::SelectTool(Tool::Dimension));
    pick(&mut doc, 5.0, 0.0, a);
    pick(&mut doc, 0.0, 5.0, b);
    place(&mut doc, 2.0, 2.0);
    let Some(ValueTarget::New { measure, .. }) = field(&doc).map(|field| &field.target) else {
        panic!("no field");
    };
    assert!(matches!(measure, Measure::Angle(..)));
    assert_eq!(field(&doc).unwrap().text, "90°");
    enter(&mut doc, "60");
    let dimension = &sketch(&doc).dimensions[0].dimension;
    assert!((dimension.value.value - 60f64.to_radians()).abs() < 1e-12);
}

#[test]
fn the_dimension_tool_starts_from_what_s_selected() {
    let (mut doc, [start, end, line]) = line_to_dimension();
    doc.look(Look::SelectTool(Tool::Dimension));
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    doc.look(Look::SelectTool(Tool::Dimension));
    assert_eq!(drawing(&doc).unwrap().picked, [line]);
    place(&mut doc, 5.0, 3.0);
    assert!(field(&doc).is_some());
    // Two points, likewise; what measures nothing together, nothing.
    for (selected, picked) in [
        (vec![start, end], vec![start, end]),
        (vec![start, line], vec![]),
    ] {
        doc.look(Look::SelectTool(Tool::Dimension));
        doc.look(Look::ClearSelection);
        for id in selected {
            doc.look(Look::ClickGeometry {
                hit: Some(id),
                add: true,
            });
        }
        doc.look(Look::SelectTool(Tool::Dimension));
        assert_eq!(drawing(&doc).unwrap().picked, picked);
    }
}

#[test]
fn a_double_click_on_a_label_opens_the_field_on_it_and_its_release_keeps_it() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "10");
    let id = sketch(&doc).dimensions[0].id;
    // What the label sends for a double-click: a press and a release,
    // then the second press, its double-click, and its release.
    doc.look(Look::PressLabel { id, add: false });
    doc.update(Edit::DropLabel);
    doc.look(Look::PressLabel { id, add: false });
    doc.look(Look::EditDimension { id, in_list: false });
    doc.update(Edit::DropLabel);
    assert_eq!(field(&doc).unwrap().target, ValueTarget::Dimension(id));
    // A reference's has nothing to type.
    doc.look(Look::CancelValue);
    doc.update(Edit::ToggleReference);
    assert!(!sketch(&doc).dimensions[0].dimension.driving);
    doc.look(Look::EditDimension { id, in_list: false });
    assert!(field(&doc).is_none());
}

/// What the document screen of `doc`, shown headless from `cache`, sends
/// for a click at `at`, its cache after, for the next click to be a
/// double-click, and the times just before and after the click went in.
fn click_screen(doc: &Doc, at: iced::Point, cache: Cache) -> (Vec<Ui>, Cache, [Instant; 2]) {
    crate::tests::with_renderer(|renderer| {
        let view = doc.view_in(Mode::Light);
        let mut ui = UserInterface::build(view, Size::new(1280.0, 800.0), cache, renderer);
        let mut sent = Vec::new();
        let events = [
            mouse::Event::CursorMoved { position: at },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ]
        .map(Event::Mouse);
        let before = Instant::now();
        ui.update(
            &events,
            mouse::Cursor::Available(at),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut sent,
        );
        (sent, ui.into_cache(), [before, Instant::now()])
    })
}

/// How far apart iced's two clicks may be to make a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(300);

/// Whether `sent` has a click on the row of `id`.
fn clicked_row(sent: &[Ui], id: Id) -> bool {
    sent.iter()
        .any(|message| matches!(message, Ui::Look(Look::ClickRow(row)) if *row == id))
}

#[test]
fn a_dimension_s_row_double_clicked_opens_the_field_in_it() {
    let (mut doc, line) = line_to_dimension();
    let other = draw_line(&mut doc, (0.0, 5.0), (10.0, 6.0));
    // Constraints before the dimension in the list.
    for id in [other[2], line[2]] {
        doc.look(Look::ClickGeometry {
            hit: Some(id),
            add: false,
        });
        doc.update(Edit::Constrain(ConstraintKind::Horizontal));
    }
    doc.look(Look::ClearSelection);
    doc.look(Look::SelectTool(Tool::Dimension));
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "10");
    doc.look(Look::SelectTool(Tool::Dimension));
    doc.look(Look::SelectPanel(varde_view::Panel::Sketch));
    let id = sketch(&doc).dimensions[0].id;

    // With nothing selected, and with its line selected: the row stays
    // where it is as the first click selects it, so the second is on it.
    for selected in [None, Some(line[2])] {
        doc.look(Look::ClearSelection);
        if let Some(selected) = selected {
            doc.look(Look::ClickGeometry {
                hit: Some(selected),
                add: false,
            });
        }
        // The row, in the Sketch tab on the left, rows 28 pixels tall.
        let row = (0..800).step_by(14).find_map(|y| {
            let at = iced::Point::new(60.0, y as f32);
            let (sent, ..) = click_screen(&doc, at, Cache::default());
            clicked_row(&sent, id).then_some(at)
        });
        let row = row.expect("the dimension's row");
        // Two clicks further apart than a double-click's time (building
        // the screen again in between, in a debug build under load, can
        // take that long) tell nothing: they are clicked again.
        let mut tries = 0;
        let sent = loop {
            doc.look(Look::ClearSelection);
            if let Some(selected) = selected {
                doc.look(Look::ClickGeometry {
                    hit: Some(selected),
                    add: false,
                });
            }
            let (sent, cache, [first, _]) = click_screen(&doc, row, Cache::default());
            assert!(clicked_row(&sent, id), "{sent:?}");
            doc.look(Look::ClickGeometry {
                hit: Some(id),
                add: false,
            });
            let (sent, _, [_, second]) = click_screen(&doc, row, cache);
            tries += 1;
            if second - first <= DOUBLE_CLICK || tries == 20 {
                break sent;
            }
        };
        let edits = sent.iter().any(|message| {
            matches!(message, Ui::Look(Look::EditDimension { id: edited, in_list: true }) if *edited == id)
        });
        assert!(edits, "{selected:?}, {tries} tries: {sent:?}");
        doc.look(Look::EditDimension { id, in_list: true });
        assert!(field(&doc).is_some_and(|field| field.in_list));
        doc.look(Look::CancelValue);
    }
}

#[test]
fn construction_in_the_dimension_tool_is_the_selection_s() {
    let (mut doc, [.., line]) = line_to_dimension();
    doc.look(Look::SelectTool(Tool::Dimension));
    doc.look(Look::ClickGeometry {
        hit: Some(line),
        add: false,
    });
    doc.look(Look::SelectTool(Tool::Dimension));
    // The key is bound, the line being selected, and it's the line's: the
    // Dimension tool draws nothing to make construction.
    let x = keyboard::Key::Character("x".into());
    doc.key(x);
    assert!(sketch(&doc).curve(line).unwrap().construction);
}

/// Places lone points with the Point tool at `places`, then takes up the
/// Dimension tool: their ids.
fn lone_points<const N: usize>(doc: &mut Answered, places: [(f64, f64); N]) -> [Id; N] {
    doc.look(Look::SelectTool(Tool::Point));
    let ids = places.map(|(x, y)| {
        click(doc, x, y);
        sketch(doc).points.last().unwrap().id
    });
    doc.look(Look::SelectTool(Tool::Dimension));
    ids
}

/// Why the last edit was refused, as the status bar says it.
fn refused(doc: &Doc) -> String {
    doc.edit_error
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default()
}

#[test]
fn points_at_one_place_are_refused_a_dimension_saying_why() {
    let (mut doc, [start, ..]) = line_to_dimension();
    let [twin] = lone_points(&mut doc, [(0.0, 0.0)]);
    assert_eq!(position(sketch(&doc), twin), position(sketch(&doc), start));
    for reference in [false, true] {
        pick(&mut doc, 0.0, 0.0, start);
        pick(&mut doc, 0.0, 0.0, twin);
        tool_click(&mut doc, 3.0, 3.0, None, reference);
        assert!(field(&doc).is_none());
        assert!(sketch(&doc).dimensions.is_empty());
        assert!(refused(&doc).contains("same place"), "{}", refused(&doc));
        // Picked afresh.
        assert!(drawing(&doc).unwrap().picked.is_empty());
    }
}

#[test]
fn a_reference_past_what_a_dimension_can_be_is_refused_saying_why() {
    let (mut doc, _) = line_to_dimension();
    let far = lone_points(&mut doc, [(-600_000.0, 0.0), (600_000.0, 0.0)]);
    pick(&mut doc, -600_000.0, 0.0, far[0]);
    pick(&mut doc, 600_000.0, 0.0, far[1]);
    // Above both: their horizontal distance, past the limit.
    tool_click(&mut doc, 0.0, 10.0, None, true);
    assert!(sketch(&doc).dimensions.is_empty());
    assert!(
        refused(&doc).contains("more than a dimension can be"),
        "{}",
        refused(&doc)
    );
}

#[test]
fn units_chosen_while_edits_wait_are_set_once_they_are_answered() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    // Taken, but not answered yet.
    doc.doc.look(Look::ValueInput("25.4".into()));
    doc.doc.update(Edit::SubmitValue);
    assert!(doc.proposing());
    doc.doc.update(Edit::SetUnits(LengthUnit::In));
    assert_eq!(doc.editor.document().units(), LengthUnit::Mm);
    doc.lane.answer(&mut doc.doc);
    assert_eq!(doc.editor.document().units(), LengthUnit::In);
    let value = &sketch(&doc).dimensions[0].dimension.value;
    assert_eq!((value.text.as_str(), value.value), ("25.4 mm", 25.4));
    // Undoing the edits waiting drops the units chosen after them too.
    let id = sketch(&doc).dimensions[0].id;
    doc.look(Look::EditDimension { id, in_list: false });
    doc.doc.look(Look::ValueInput("2".into()));
    doc.doc.update(Edit::SubmitValue);
    doc.doc.update(Edit::SetUnits(LengthUnit::Mm));
    doc.doc.update(Edit::Undo);
    doc.lane.answer(&mut doc.doc);
    assert!(!doc.proposing());
    assert_eq!(doc.editor.document().units(), LengthUnit::In);
}

#[test]
fn values_typed_while_new_units_wait_are_read_in_the_units_shown() {
    let (mut doc, line) = line_to_dimension();
    pick(&mut doc, 5.0, 0.0, line[2]);
    place(&mut doc, 5.0, 3.0);
    enter(&mut doc, "20");
    let id = sketch(&doc).dimensions[0].id;
    // 30 waits; inches wait behind it; then 50, typed while millimetres
    // still show, so millimetres.
    doc.doc.look(Look::EditDimension { id, in_list: false });
    doc.doc.look(Look::ValueInput("30".into()));
    doc.doc.update(Edit::SubmitValue);
    doc.doc.update(Edit::SetUnits(LengthUnit::In));
    assert_eq!(doc.editor.document().units(), LengthUnit::Mm);
    assert!(doc.proposing());
    doc.doc.look(Look::EditDimension { id, in_list: false });
    doc.doc.look(Look::ValueInput("50".into()));
    doc.doc.update(Edit::SubmitValue);
    doc.lane.answer(&mut doc.doc);
    assert!(!doc.proposing());
    assert_eq!(doc.editor.document().units(), LengthUnit::In);
    assert!((length(sketch(&doc), line) - 50.0).abs() < 1e-9);
    let value = &sketch(&doc).dimensions[0].dimension.value;
    assert_eq!((value.text.as_str(), value.value), ("50 mm", 50.0));
    // Undone in the order made.
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.document().units(), LengthUnit::In);
    assert!((length(sketch(&doc), line) - 30.0).abs() < 1e-9);
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.document().units(), LengthUnit::Mm);
    assert!((length(sketch(&doc), line) - 30.0).abs() < 1e-9);
    doc.update(Edit::Undo);
    assert!((length(sketch(&doc), line) - 20.0).abs() < 1e-9);
}
