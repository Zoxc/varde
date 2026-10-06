use std::sync::Arc;

use iced::Size;
use iced::widget::shader::Program as _;
use varde_document::OriginPlane;
use varde_sketch::{Curve, Measure, Side};

use super::line as line_style;
use super::*;
use crate::Target;
use crate::projection::top_camera;
use crate::testing;
use crate::theme::Mode;
use crate::viewport::{Interaction, Program, program};

/// The viewport's width and height: through [`top_camera`], 20 sketch
/// units across, the origin in its middle, a unit 10 pixels.
const SIZE: f32 = 200.0;

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(SIZE, SIZE))
}

/// Where the sketch point `x`, `y` shows.
fn screen_at(x: f64, y: f64) -> Point {
    let projector = Projector::new(&top_camera(), OriginPlane::XY.placement(), SIZE, SIZE).unwrap();
    let shown = projector.project(DVec2::new(x, y)).unwrap();
    Point::new(shown.x as f32, shown.y as f32)
}

/// A line from (-5, 0) to (5, 0), its points and its id.
fn drawn() -> (Sketch, [Id; 3]) {
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::new(-5.0, 0.0)).unwrap();
    let b = sketch.add_point(DVec2::new(5.0, 0.0)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    (sketch, [a, b, line])
}

/// `sketch` as the viewport shows it, which can be changed if `editable`,
/// with `selection` and `tool` in use if there is one.
fn sketching<'a>(
    sketch: &'a Sketch,
    selection: &'a BTreeSet<Id>,
    tool: Option<ActiveTool<'a>>,
    editable: bool,
) -> Sketching<'a> {
    let state = SketchState::plain(sketch, selection, tool);
    Sketching::new(state, editable)
}

/// The viewport's program showing [`sketching`] through [`top_camera`].
fn viewport<'a>(
    sketch: &'a Sketch,
    selection: &'a BTreeSet<Id>,
    tool: Option<ActiveTool<'a>>,
    editable: bool,
) -> Program<'a> {
    shown(sketching(sketch, selection, tool, editable))
}

/// The viewport's program showing `sketching` through [`top_camera`].
fn shown(sketching: Sketching<'_>) -> Program<'_> {
    program(
        &Arc::default(),
        &Arc::default(),
        &top_camera(),
        None,
        Mode::Light.palette(),
        Some(sketching),
        None,
    )
}

/// Feeds `events` to `viewport` with the cursor at `at`, and returns the
/// messages it sends and whether it captured the last event.
fn feed(
    viewport: &Program<'_>,
    state: &mut Interaction,
    at: Point,
    events: &[Event],
) -> (Vec<Message>, bool) {
    let mut messages = Vec::new();
    let mut captured = false;
    for event in events {
        let action = viewport.update(state, event, bounds(), mouse::Cursor::Available(at));
        captured = false;
        if let Some(action) = action {
            let (message, _, status) = action.into_inner();
            messages.extend(message);
            captured = status == iced::event::Status::Captured;
        }
    }
    (messages, captured)
}

fn moved(at: Point) -> Event {
    Event::Mouse(mouse::Event::CursorMoved { position: at })
}

fn press() -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
}

fn release() -> Event {
    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
}

/// Clicks at `at`: moves there, presses and releases.
/// The one tool click `messages` are.
fn tool_click(messages: Vec<Message>) -> ToolClick {
    match messages.as_slice() {
        [Message::Edit(Edit::ToolClick(click))] => *click,
        other => panic!("{other:?}"),
    }
}

/// `Esc` pressed.
fn escape() -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(Named::Escape),
        modified_key: keyboard::Key::Named(Named::Escape),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

fn click(viewport: &Program<'_>, state: &mut Interaction, at: Point) -> Vec<Message> {
    feed(viewport, state, at, &[moved(at), press(), release()]).0
}

/// Drags from `from` to `to` and lets go.
fn drag(viewport: &Program<'_>, state: &mut Interaction, from: Point, to: Point) -> Vec<Message> {
    let mut messages = feed(viewport, state, from, &[moved(from), press()]).0;
    let middle = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
    messages.extend(feed(viewport, state, middle, &[moved(middle)]).0);
    messages.extend(feed(viewport, state, to, &[moved(to), release()]).0);
    messages
}

#[test]
fn a_click_selects_what_it_hits_and_ctrl_adds() {
    let (sketch, [a, _, line]) = drawn();
    let selection = BTreeSet::new();
    let viewport = viewport(&sketch, &selection, None, true);
    let mut state = Interaction::default();

    let hits = |messages: Vec<Message>| match messages.as_slice() {
        [Message::Look(Look::ClickGeometry { hit, add })] => (*hit, *add),
        other => panic!("{other:?}"),
    };
    // A few pixels off counts.
    let near_a = screen_at(-5.0, 0.3);
    assert_eq!(hits(click(&viewport, &mut state, near_a)), (Some(a), false));
    assert_eq!(state.sketch.hover, Some(a));
    assert_eq!(
        hits(click(&viewport, &mut state, screen_at(1.0, 0.0))),
        (Some(line), false)
    );
    assert_eq!(
        hits(click(&viewport, &mut state, screen_at(1.0, 5.0))),
        (None, false)
    );

    let ctrl = Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::COMMAND));
    feed(&viewport, &mut state, near_a, &[ctrl]);
    assert_eq!(hits(click(&viewport, &mut state, near_a)), (Some(a), true));
}

/// A frame drawn well after the button went down.
fn later() -> Event {
    Event::Window(iced::window::Event::RedrawRequested(
        Instant::now() + overlaps::HOLD_DELAY * 2,
    ))
}

#[test]
fn a_press_held_still_over_overlapping_items_lists_them() {
    let (sketch, [a, _, line]) = drawn();
    let selection = BTreeSet::new();
    let viewport = viewport(&sketch, &selection, None, true);
    let mut state = Interaction::default();

    // On `a`, where the line ends, on the X axis: the point, then the
    // line, then the axis.
    let on_a = screen_at(-5.0, 0.0);
    let (messages, captured) = feed(&viewport, &mut state, on_a, &[moved(on_a), press()]);
    assert!(messages.is_empty() && captured);
    let messages = feed(&viewport, &mut state, on_a, &[later()]).0;
    let [Message::Look(Look::OpenOverlaps(list))] = &messages[..] else {
        panic!("{messages:?}");
    };
    assert_eq!(list.items, OverlapItems::Sketch(vec![a, line, Id::X_AXIS]));
    // Let go of: nothing more, no drag.
    assert!(feed(&viewport, &mut state, on_a, &[release()]).0.is_empty());

    // Over nothing, held still it's still a click.
    let off = screen_at(1.0, 5.0);
    let events = [moved(off), press(), later(), release()];
    let messages = feed(&viewport, &mut state, off, &events).0;
    assert!(
        matches!(
            messages.as_slice(),
            [Message::Look(Look::ClickGeometry {
                hit: None,
                add: false
            })]
        ),
        "{messages:?}"
    );
}

#[test]
fn a_box_selects_inside_to_the_right_and_touching_to_the_left() {
    let (sketch, [a, b, line]) = drawn();
    let selection = BTreeSet::new();
    let viewport = viewport(&sketch, &selection, None, true);
    let mut state = Interaction::default();

    let boxed = |messages: Vec<Message>| match messages.as_slice() {
        [Message::Look(Look::SelectBox { ids, add: false })] => {
            let mut ids = ids.clone();
            ids.sort();
            ids
        }
        other => panic!("{other:?}"),
    };
    let around = boxed(drag(
        &viewport,
        &mut state,
        screen_at(-6.0, 2.0),
        screen_at(6.0, -2.0),
    ));
    assert_eq!(around, [a, b, line]);
    // Over the right half: to the right it holds only the point.
    let right = boxed(drag(
        &viewport,
        &mut state,
        screen_at(1.0, 2.0),
        screen_at(6.0, -2.0),
    ));
    assert_eq!(right, [b]);
    // To the left, the line it touches too.
    let left = boxed(drag(
        &viewport,
        &mut state,
        screen_at(6.0, 2.0),
        screen_at(1.0, -2.0),
    ));
    assert_eq!(left, [b, line]);
}

#[test]
fn geometry_is_dragged_and_dropped() {
    let (sketch, [_, b, _]) = drawn();
    let selection = BTreeSet::new();
    let viewport = viewport(&sketch, &selection, None, true);
    let mut state = Interaction::default();

    let messages = drag(
        &viewport,
        &mut state,
        screen_at(5.0, 0.0),
        screen_at(5.0, 3.0),
    );
    let [
        ..,
        Message::Look(Look::DragGeometry { id, from, to }),
        Message::Edit(Edit::DropGeometry),
    ] = messages.as_slice()
    else {
        panic!("{messages:?}");
    };
    assert_eq!(*id, b);
    assert!(from.abs_diff_eq(DVec2::new(5.0, 0.0), 1e-3), "{from}");
    assert!(to.abs_diff_eq(DVec2::new(5.0, 3.0), 1e-3), "{to}");

    // Short of the drag distance, it's a click.
    let from = screen_at(5.0, 0.0);
    let nudged = Point::new(from.x + 1.0, from.y);
    feed(&viewport, &mut state, from, &[moved(from), press()]);
    let (messages, _) = feed(&viewport, &mut state, nudged, &[moved(nudged), release()]);
    assert!(matches!(
        messages.as_slice(),
        [Message::Look(Look::ClickGeometry { hit: Some(hit), .. })] if *hit == b
    ));
}

#[test]
fn escape_lets_go_of_a_drag_and_a_box() {
    let (sketch, _) = drawn();
    let selection = BTreeSet::new();
    let viewport = viewport(&sketch, &selection, None, true);
    let escape = escape();
    // Both keep the key, so it does nothing else: a drag of geometry asks
    // the app to put it back, even if it has nothing to put back (a drag
    // that never met the plane), rather than leave the key to back out of
    // the sketch.
    for (from, cancel) in [(screen_at(5.0, 0.0), true), (screen_at(0.0, 5.0), false)] {
        let mut state = Interaction::default();
        let to = Point::new(from.x + 30.0, from.y + 30.0);
        feed(
            &viewport,
            &mut state,
            from,
            &[moved(from), press(), moved(to)],
        );
        let (messages, escaped) = feed(&viewport, &mut state, to, std::slice::from_ref(&escape));
        assert_eq!(
            matches!(messages.as_slice(), [Message::Look(Look::CancelDrag)]),
            cancel,
            "{messages:?}"
        );
        assert!(cancel || messages.is_empty());
        assert!(escaped);
        let (messages, _) = feed(&viewport, &mut state, to, &[release()]);
        assert!(messages.is_empty(), "{messages:?}");
    }
    // Nothing held, it's the app's.
    let mut state = Interaction::default();
    let at = screen_at(0.0, 5.0);
    let (messages, captured) = feed(&viewport, &mut state, at, &[escape]);
    assert!(messages.is_empty() && !captured);
}

#[test]
fn read_only_geometry_is_boxed_rather_than_dragged() {
    let (sketch, [_, b, _]) = drawn();
    let selection = BTreeSet::new();
    let viewport = viewport(&sketch, &selection, None, false);
    let mut state = Interaction::default();
    let messages = drag(
        &viewport,
        &mut state,
        screen_at(4.8, 0.3),
        screen_at(7.0, -2.0),
    );
    assert!(matches!(
        messages.as_slice(),
        [Message::Look(Look::SelectBox { ids, .. })] if ids == &[b]
    ));
}

#[test]
fn a_tool_clicks_where_it_is_pressed_as_it_snaps() {
    let (sketch, [a, _, line]) = drawn();
    let selection = BTreeSet::new();
    let tool = testing::tool(Tool::Line, &[], &[]);
    let viewport = viewport(&sketch, &selection, Some(tool), true);
    let mut state = Interaction::default();

    // Moving there first tells the app where it snaps, if anywhere.
    let clicks = |messages: Vec<Message>| match messages.as_slice() {
        [Message::Edit(Edit::ToolClick(click))]
        | [
            Message::Look(Look::Snap(_)),
            Message::Edit(Edit::ToolClick(click)),
        ] => *click,
        other => panic!("{other:?}"),
    };
    let at = screen_at(2.0, 3.0);
    let first = clicks(click(&viewport, &mut state, at));
    assert!(first.at.abs_diff_eq(DVec2::new(2.0, 3.0), 1e-3));
    assert!((first.pixel - 0.1).abs() < 1e-6, "{}", first.pixel);
    assert_eq!(first.point(), None);
    assert!(!first.double);
    // Again at once is a double-click, and a third a single one again.
    assert!(clicks(click(&viewport, &mut state, at)).double);
    assert!(!clicks(click(&viewport, &mut state, at)).double);
    // On a point, it's named, and snaps there.
    let on = clicks(click(&viewport, &mut state, screen_at(-5.0, 0.2)));
    assert_eq!(on.point(), Some(a));
    assert_eq!(on.at, DVec2::new(-5.0, 0.0));
    // Not a line, though, which it's on, nor the origin, which is a point.
    let on_line = clicks(click(&viewport, &mut state, screen_at(2.0, 0.3)));
    assert_eq!(on_line.point(), None);
    assert_eq!(on_line.target, Some(Target::On(line)));
    assert!(on_line.at.abs_diff_eq(DVec2::new(2.0, 0.0), 1e-3));
    assert_eq!(on_line.at.y, 0.0);
    let origin = clicks(click(&viewport, &mut state, screen_at(0.3, 0.2)));
    assert_eq!(origin.point(), Some(Id::ORIGIN));
    // With `Shift` held, where it's pressed, snapping to nothing.
    let free = feed(
        &viewport,
        &mut state,
        screen_at(-5.0, 0.2),
        &[
            Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::SHIFT)),
            moved(screen_at(-5.0, 0.2)),
            press(),
            release(),
        ],
    );
    let free = clicks(free.0);
    assert_eq!(free.target, None);
    assert!(free.at.abs_diff_eq(DVec2::new(-5.0, 0.2), 1e-3));
}

#[test]
fn the_other_buttons_and_the_wheel_move_the_camera() {
    use crate::viewport::DragKind;

    let (sketch, _) = drawn();
    let selection = BTreeSet::new();
    let viewport = viewport(&sketch, &selection, None, true);
    let mut state = Interaction::default();
    let (from, to) = (screen_at(1.5, 5.0), screen_at(5.0, 0.0));
    for (button, kind) in [
        (mouse::Button::Right, DragKind::Pan),
        (mouse::Button::Middle, DragKind::Orbit),
    ] {
        feed(&viewport, &mut state, from, &[moved(from)]);
        let (messages, captured) = feed(
            &viewport,
            &mut state,
            from,
            &[Event::Mouse(mouse::Event::ButtonPressed(button))],
        );
        assert!(messages.is_empty() && captured);
        assert!(matches!(state.drag, Some((drag, _)) if drag == kind));
        assert!(state.sketch.press.is_none());
        // Past geometry, which isn't hovered meanwhile.
        let (messages, _) = feed(&viewport, &mut state, to, &[moved(to)]);
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Look(Look::Pan { .. } | Look::Orbit { .. })]
            ),
            "{messages:?}"
        );
        assert_eq!(state.sketch.hover, None);
        let release = Event::Mouse(mouse::Event::ButtonReleased(button));
        let (messages, captured) = feed(&viewport, &mut state, to, &[release]);
        assert!(messages.is_empty() && captured && state.drag.is_none());
    }
    let wheel = Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
    });
    let (messages, _) = feed(&viewport, &mut state, from, &[wheel]);
    assert!(matches!(
        messages.as_slice(),
        [Message::Look(Look::Zoom { .. })]
    ));
    // The left button is the sketch's.
    let (_, captured) = feed(&viewport, &mut state, from, &[press()]);
    assert!(captured && state.drag.is_none() && state.sketch.press.is_some());
}

#[test]
fn the_cursor_shows_what_the_left_button_does() {
    use iced::widget::shader::Program as _;

    let (sketch, [a, ..]) = drawn();
    let selection = BTreeSet::new();
    let interaction = |viewport: &Program<'_>, state: &Interaction, at: Point| {
        viewport.mouse_interaction(state, bounds(), mouse::Cursor::Available(at))
    };
    let (empty, on_a) = (screen_at(1.5, 5.0), screen_at(-5.0, 0.0));
    let outside = Point::new(-10.0, -10.0);

    let selecting = viewport(&sketch, &selection, None, true);
    let mut state = Interaction::default();
    feed(&selecting, &mut state, empty, &[moved(empty)]);
    assert_eq!(
        interaction(&selecting, &state, empty),
        mouse::Interaction::None
    );
    feed(&selecting, &mut state, on_a, &[moved(on_a)]);
    assert_eq!(state.sketch.hover, Some(a));
    assert_eq!(
        interaction(&selecting, &state, on_a),
        mouse::Interaction::Pointer
    );
    // Grabbing while it's dragged, wherever the cursor is.
    feed(&selecting, &mut state, on_a, &[press(), moved(outside)]);
    assert_eq!(
        interaction(&selecting, &state, outside),
        mouse::Interaction::Grabbing
    );

    let tool = testing::tool(Tool::Point, &[], &[]);
    let drawing = viewport(&sketch, &selection, Some(tool), true);
    let state = Interaction::default();
    assert_eq!(
        interaction(&drawing, &state, empty),
        mouse::Interaction::Crosshair
    );
    // Not off the viewport, and not without a tool in a read-only sketch.
    assert_eq!(
        interaction(&drawing, &state, outside),
        mouse::Interaction::None
    );
    let read_only = viewport(&sketch, &selection, Some(tool), false);
    assert_eq!(
        interaction(&read_only, &state, empty),
        mouse::Interaction::None
    );
}

/// The base layer of an empty sketch in `colors`: the origin's axes and
/// the origin.
fn builtins(colors: SketchColors) -> SketchLayer {
    let mut layer = SketchLayer::default();
    for id in [Id::X_AXIS, Id::Y_AXIS] {
        for half in axis(id).unwrap() {
            layer.axis_polyline(
                Space::Sketch,
                &half,
                line_style(colors.axis, AXIS_WIDTH, false),
            );
        }
    }
    let mut origin = dot(POINT_RADIUS, colors.point_fill, colors.axis);
    origin.fixed = true;
    layer.point(DVec2::ZERO, origin);
    layer
}

/// The layers `sketching` draws with `state`, through [`top_camera`] and in
/// the light theme.
fn layers(sketching: &Sketching<'_>, state: &Interaction) -> (Arc<SketchLayer>, SketchLayer) {
    let colors = Mode::Light.palette().sketching;
    sketching.layers(
        &state.sketch,
        &top_camera(),
        bounds(),
        colors,
        Modifiers::default(),
    )
}

#[test]
fn the_sketch_is_uploaded_again_only_when_it_changes() {
    let (sketch, [a, ..]) = drawn();
    let none = BTreeSet::new();
    let state = Interaction::default();
    let (base, live) = layers(&sketching(&sketch, &none, None, true), &state);
    assert!(!base.is_empty() && live.is_empty());
    // Not for the camera, which only changes what the renderer projects.
    let mut camera = top_camera();
    camera.orbit(0.3, -0.2);
    let colors = Mode::Light.palette().sketching;
    let sketching = sketching(&sketch, &none, None, true);
    let (orbited, _) = sketching.layers(
        &state.sketch,
        &camera,
        bounds(),
        colors,
        Modifiers::default(),
    );
    assert!(Arc::ptr_eq(&base, &orbited));
    // For the selection, the theme and the sketch.
    let selected = BTreeSet::from([a]);
    let (reselected, _) = layers(&self::sketching(&sketch, &selected, None, true), &state);
    assert!(*reselected != *base);
    let dark = Mode::Dark.palette().sketching;
    let (themed, _) =
        sketching.layers(&state.sketch, &camera, bounds(), dark, Modifiers::default());
    assert!(*themed != *base);
    let mut moved = sketch.clone();
    moved.points[0].at.x -= 1.0;
    let (changed, _) = layers(&self::sketching(&moved, &none, None, true), &state);
    assert!(*changed != *base);
}

#[test]
fn construction_is_dashed_and_the_selection_drawn_over_the_rest() {
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::new(-5.0, 0.0)).unwrap();
    let b = sketch.add_point(DVec2::new(5.0, 0.0)).unwrap();
    let c = sketch.add_point(DVec2::new(0.0, 5.0)).unwrap();
    let line = |start, end| Curve::Line { start, end };
    let normal = sketch.add_curve(line(a, b), false).unwrap();
    sketch.add_curve(line(b, c), true).unwrap();
    let selection = BTreeSet::from([normal, a]);
    let (base, _) = layers(
        &sketching(&sketch, &selection, None, true),
        &Interaction::default(),
    );

    let colors = Mode::Light.palette().sketching;
    let at = |id| sketch.point(id).unwrap().at;
    let mut expected = builtins(colors);
    let construction = line_style(colors.construction, CURVE_WIDTH, true);
    expected.polyline(Space::Sketch, &[at(b), at(c)], construction);
    let selected = line_style(colors.selected, SELECTED_WIDTH, false);
    expected.polyline(Space::Sketch, &[at(a), at(b)], selected);
    for (id, fill) in [
        (b, colors.point_fill),
        (c, colors.point_fill),
        (a, colors.selected),
    ] {
        expected.point(at(id), dot(POINT_RADIUS, fill, colors.point));
    }
    assert_eq!(*base, expected);
}

#[test]
fn the_live_layer_has_the_box_the_hover_and_the_tool() {
    let (sketch, [a, ..]) = drawn();
    let selection = BTreeSet::new();
    let colors = Mode::Light.palette().sketching;

    // A box dragged right to left, dashed: it selects what it touches.
    let selecting = viewport(&sketch, &selection, None, true);
    let mut state = Interaction::default();
    let (from, to) = (Point::new(150.0, 20.0), Point::new(120.0, 40.0));
    feed(
        &selecting,
        &mut state,
        from,
        &[moved(from), press(), moved(to)],
    );
    let (_, live) = layers(&sketching(&sketch, &selection, None, true), &state);
    let mut expected = SketchLayer::default();
    let corners = [(120.0, 20.0), (150.0, 20.0), (150.0, 40.0), (120.0, 40.0)].map(DVec2::from);
    expected.fill(Space::Screen, [&corners[..]], srgba(colors.box_fill));
    let outline = [corners.as_slice(), &corners[..1]].concat();
    let style = line_style(colors.box_line, BOX_LINE_WIDTH, true);
    expected.polyline(Space::Screen, &outline, style);
    assert_eq!(live, expected);

    // The point hovered, and the line from the point placed to the cursor.
    let placed = [DVec2::new(0.0, 5.0)];
    let tool = testing::tool(Tool::Line, &placed, &[None; 1]);
    let drawing = viewport(&sketch, &selection, Some(tool), true);
    let mut state = Interaction::default();
    let on_a = screen_at(-5.0, 0.1);
    feed(&drawing, &mut state, on_a, &[moved(on_a)]);
    let (_, live) = layers(&sketching(&sketch, &selection, Some(tool), true), &state);
    // Snapped to the point.
    let cursor = sketch.point(a).unwrap().at;
    let mut expected = SketchLayer::default();
    let hovered = dot(HOVERED_POINT_RADIUS, colors.hovered, colors.hovered);
    expected.point(sketch.point(a).unwrap().at, hovered);
    // Ringed, so it shows round the cursor.
    expected.point(sketch.point(a).unwrap().at, snap_disc(colors.point));
    let preview = line_style(colors.preview, CURVE_WIDTH, false);
    expected.polyline(Space::Sketch, &[placed[0], cursor], preview);
    expected.point(cursor, dot(POINT_RADIUS, colors.preview, colors.preview));
    expected.point(
        placed[0],
        dot(POINT_RADIUS, colors.point_fill, colors.preview),
    );
    assert_eq!(live, expected);
}

#[test]
fn geometry_is_coloured_by_its_state() {
    let (mut sketch, [a, b, line]) = drawn();
    let level = sketch
        .add_constraint(varde_sketch::Constraint::Horizontal(line))
        .unwrap();
    let lone = sketch.add_point(DVec2::new(0.0, 5.0)).unwrap();
    // `a` fixed; the constraint on the line in a conflict; the lone point
    // waiting on the solver.
    let analysis = varde_sketch::Analysis {
        freedom: 3,
        fixed: BTreeSet::from([a]),
        redundant: BTreeSet::from([level]),
        solved: false,
    };
    let pending = BTreeSet::from([lone]);
    let none = BTreeSet::new();
    let state = SketchState {
        analysis: Some(&analysis),
        pending: &pending,
        ..SketchState::plain(&sketch, &none, None)
    };
    let (base, _) = layers(&Sketching::new(state, true), &Interaction::default());

    let colors = Mode::Light.palette().sketching;
    let at = |id| sketch.point(id).unwrap().at;
    let mut expected = builtins(colors);
    let conflict = line_style(colors.conflict, CURVE_WIDTH, false);
    expected.polyline(Space::Sketch, &[at(a), at(b)], conflict);
    let mut fixed = dot(POINT_RADIUS, colors.point_fill, colors.fixed);
    fixed.fixed = true;
    expected.point(at(a), fixed);
    expected.point(at(b), dot(POINT_RADIUS, colors.point_fill, colors.point));
    let faded = |color: Color| Color {
        a: color.a * PENDING_ALPHA,
        ..color
    };
    let waiting = dot(POINT_RADIUS, faded(colors.point_fill), faded(colors.point));
    expected.point(at(lone), waiting);
    assert_eq!(*base, expected);
}

#[test]
fn a_failing_curve_is_red_within_the_errors_halo() {
    let (mut sketch, [a, b, line]) = drawn();
    let c = sketch.add_point(DVec2::new(0.0, 5.0)).unwrap();
    sketch
        .add_curve(Curve::Line { start: b, end: c }, false)
        .unwrap();
    let gone = sketch
        .add_curve(Curve::Line { start: c, end: a }, false)
        .unwrap();
    sketch.delete(&[gone]);
    let none = BTreeSet::new();
    // The point `a` named too, and a curve the sketch no longer holds:
    // only its curves are marked.
    let failing = BTreeSet::from([line, a, gone]);
    // On XZ, so the halo's lines are placed in the world.
    let placement = OriginPlane::XZ.placement();
    let state = SketchState {
        placement,
        failing: &failing,
        ..SketchState::plain(&sketch, &none, None)
    };
    let sketching = Sketching::new(state, true);
    let interaction = Interaction::default();
    let (base, _) = layers(&sketching, &interaction);

    let colors = Mode::Light.palette().sketching;
    let at = |id| sketch.point(id).unwrap().at;
    let mut expected = builtins(colors);
    let red = line_style(colors.conflict, CURVE_WIDTH, false);
    expected.polyline(Space::Sketch, &[at(a), at(b)], red);
    let free = line_style(colors.curve, CURVE_WIDTH, false);
    expected.polyline(Space::Sketch, &[at(b), at(c)], free);
    for id in [a, b, c] {
        expected.point(at(id), dot(POINT_RADIUS, colors.point_fill, colors.point));
    }
    assert_eq!(*base, expected);
    // The red is the errors' own.
    let error = Mode::Light.palette().scene.error;
    assert_eq!(
        error.0,
        [colors.conflict.r, colors.conflict.g, colors.conflict.b]
    );

    // The failing line placed in the world, for the halo: kept with the
    // base layer, so it isn't uploaded again while that isn't.
    let halo = sketching
        .failing(&interaction.sketch)
        .expect("the halo's lines");
    let world = |id| placement.to_world(at(id)).as_vec3().to_array();
    assert_eq!(halo.points(), [world(a), world(b)]);
    let _ = layers(&sketching, &interaction);
    let again = sketching.failing(&interaction.sketch).unwrap();
    assert!(Arc::ptr_eq(&halo, &again));

    // Nothing failing, no halo.
    let plain = Sketching::new(SketchState::plain(&sketch, &none, None), true);
    let _ = layers(&plain, &interaction);
    assert!(plain.failing(&interaction.sketch).is_none());
}

#[test]
fn each_constraint_has_its_glyph_unless_they_are_hidden() {
    let (mut sketch, [a, b, line]) = drawn();
    sketch
        .add_constraint(varde_sketch::Constraint::Horizontal(line))
        .unwrap();
    sketch
        .add_constraint(varde_sketch::Constraint::HorizontalPoints(a, b))
        .unwrap();
    let none = BTreeSet::new();
    let shown = SketchState {
        glyphs: true,
        ..SketchState::plain(&sketch, &none, None)
    };
    let glyphs = Sketching::new(shown, true).glyphs();
    let anchors: Vec<_> = glyphs.iter().map(|(at, _)| *at).collect();
    assert_eq!(anchors, [DVec2::ZERO, DVec2::ZERO]);
    let hidden = SketchState::plain(&sketch, &none, None);
    assert!(Sketching::new(hidden, true).glyphs().is_empty());
}

#[test]
fn what_a_hovered_constraint_ties_together_is_highlighted() {
    let (mut sketch, [a, b, line]) = drawn();
    let level = sketch
        .add_constraint(varde_sketch::Constraint::HorizontalPoints(a, b))
        .unwrap();
    let none = BTreeSet::new();
    let state = SketchState {
        hovered: Some(level),
        ..SketchState::plain(&sketch, &none, None)
    };
    let (_, live) = layers(&Sketching::new(state, true), &Interaction::default());
    let colors = Mode::Light.palette().sketching;
    let hovered = dot(HOVERED_POINT_RADIUS, colors.hovered, colors.hovered);
    let mut expected = SketchLayer::default();
    expected.point(DVec2::new(-5.0, 0.0), hovered);
    expected.point(DVec2::new(5.0, 0.0), hovered);
    assert_eq!(live, expected);
    // A row of geometry hovered highlights it.
    let state = SketchState {
        hovered: Some(line),
        ..SketchState::plain(&sketch, &none, None)
    };
    let (_, live) = layers(&Sketching::new(state, true), &Interaction::default());
    assert!(!live.is_empty());
}

/// [`drawn`] with its line's length dimensioned, driving, its label 3
/// above the line's middle: its points, the line and the dimension.
fn dimensioned() -> (Sketch, [Id; 4]) {
    let (mut sketch, [a, b, line]) = drawn();
    let measure = Measure::Length(line);
    let id = testing::dimension(&mut sketch, measure, "10", true, DVec2::new(0.0, 3.0));
    (sketch, [a, b, line, id])
}

#[test]
fn the_dimension_tool_clicks_what_it_is_on_and_alt_makes_a_reference() {
    let (sketch, [_, _, line]) = drawn();
    let selection = BTreeSet::new();
    let tool = testing::tool(Tool::Dimension, &[], &[]);
    let viewport = viewport(&sketch, &selection, Some(tool), true);
    let mut state = Interaction::default();
    let on = tool_click(click(&viewport, &mut state, screen_at(1.0, 0.0)));
    assert_eq!(
        (on.hit, on.point(), on.reference),
        (Some(line), None, false)
    );
    // What's under the cursor shows, a line too.
    let (_, live) = layers(&viewport.sketching.clone().unwrap(), &state);
    assert!(!live.is_empty());
    let alt = Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::ALT));
    feed(&viewport, &mut state, screen_at(1.0, 3.0), &[alt]);
    let placed = tool_click(click(&viewport, &mut state, screen_at(1.0, 3.0)));
    assert_eq!((placed.hit, placed.reference), (None, true));
}

#[test]
fn a_grabbed_label_follows_the_cursor_until_let_go() {
    let (sketch, [.., id]) = dimensioned();
    let none = BTreeSet::new();
    let grabbed = SketchState {
        label_drag: Some((id, DVec2::ZERO)),
        ..SketchState::plain(&sketch, &none, None)
    };
    let viewport = program(
        &Arc::default(),
        &Arc::default(),
        &top_camera(),
        None,
        Mode::Light.palette(),
        Some(Sketching::new(grabbed, true)),
        None,
    );
    let mut state = Interaction::default();
    // Where it was grabbed, then a little way off, which isn't a drag.
    let (from, near, far) = (
        screen_at(0.0, 3.0),
        screen_at(0.2, 3.0),
        screen_at(2.0, 5.0),
    );
    for at in [from, near] {
        let (messages, captured) = feed(&viewport, &mut state, at, &[moved(at)]);
        assert!(messages.is_empty() && captured);
    }
    let (messages, _) = feed(&viewport, &mut state, far, &[moved(far)]);
    let [
        Message::Look(Look::DragLabel {
            id: dragged,
            from,
            to,
        }),
    ] = messages.as_slice()
    else {
        panic!("{messages:?}");
    };
    assert_eq!(*dragged, id);
    assert!(from.abs_diff_eq(DVec2::new(0.0, 3.0), 1e-3), "{from}");
    assert!(to.abs_diff_eq(DVec2::new(2.0, 5.0), 1e-3), "{to}");
    let cursor = mouse::Cursor::Available(far);
    let interaction = viewport.mouse_interaction(&state, bounds(), cursor);
    assert_eq!(interaction, mouse::Interaction::Grabbing);
    let (messages, captured) = feed(&viewport, &mut state, far, &[release()]);
    assert!(matches!(
        messages.as_slice(),
        [Message::Edit(Edit::DropLabel)]
    ));
    assert!(captured);
}

#[test]
fn dimensions_have_lines_arrows_labels_and_the_value_field_in_place() {
    let (sketch, [.., line, id]) = dimensioned();
    let none = BTreeSet::new();
    let plain = || SketchState::plain(&sketch, &none, None);
    let sketching = Sketching::new(plain(), true);
    let labels = sketching.labels();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].0, DVec2::new(0.0, 3.0));
    assert!(sketching.field().is_none());
    // The lines with the sketch, the arrows, a size on the screen, live.
    let (base, live) = layers(&sketching, &Interaction::default());
    let (undimensioned, _) = drawn();
    let (bare, bare_live) = layers(
        &self::sketching(&undimensioned, &none, None, true),
        &Interaction::default(),
    );
    assert!(*base != *bare && !live.is_empty() && bare_live.is_empty());

    // A label dragged is drawn where it's dragged to.
    let dragged = SketchState {
        label_drag: Some((id, DVec2::new(1.0, 1.0))),
        ..plain()
    };
    let dragged = Sketching::new(dragged, true);
    assert_eq!(dragged.labels()[0].0, DVec2::new(1.0, 4.0));
    let (moved, _) = layers(&dragged, &Interaction::default());
    assert!(*moved != *base);

    // The field takes the place of the label it edits.
    let editing = ValueTarget::Dimension(id);
    let field = |target, in_list| SketchState {
        value: Some(ValueField {
            target,
            text: "10",
            error: None,
            in_list,
        }),
        ..plain()
    };
    let edited = Sketching::new(field(&editing, false), true);
    assert!(edited.labels().is_empty());
    assert_eq!(edited.field().unwrap().0, DVec2::new(0.0, 3.0));
    // In the list, the list has it.
    let listed = Sketching::new(field(&editing, true), true);
    assert_eq!(listed.labels().len(), 1);
    assert!(listed.field().is_none());
    // One being placed is drawn from what it measures.
    let placing = ValueTarget::New {
        measure: Measure::Length(line),
        side: Side::Positive,
        label: DVec2::new(0.0, -4.0),
    };
    let placed = Sketching::new(field(&placing, false), true);
    assert_eq!(placed.field().unwrap().0, DVec2::new(0.0, -4.0));
    let (_, preview) = layers(&placed, &Interaction::default());
    assert!(preview != live);
}

#[test]
fn a_hovered_dimension_s_arrows_are_in_the_hover_colour_too() {
    let (sketch, [.., id]) = dimensioned();
    let none = BTreeSet::new();
    let state = SketchState {
        hovered: Some(id),
        ..SketchState::plain(&sketch, &none, None)
    };
    let sketching = Sketching::new(state, true);
    let (_, live) = layers(&sketching, &Interaction::default());
    let colors = Mode::Light.palette().sketching;
    let projector = Projector::new(&top_camera(), OriginPlane::XY.placement(), SIZE, SIZE).unwrap();
    let mut expected = SketchLayer::default();
    for item in tied_items(&sketch, id) {
        sketching.highlight(&mut expected, item, colors.hovered);
    }
    let lines = sketching.dimension_lines(&sketch.dimensions[0]).unwrap();
    draw_lines(&mut expected, &lines, colors.hovered);
    arrows(&mut expected, &projector, &lines.arrows, colors.hovered);
    assert_eq!(live, expected);
    // Kept with the base layer, rather than worked out every frame.
    let state = Interaction::default();
    layers(&sketching, &state);
    let base = state.sketch.base.borrow();
    let kept = &base.as_ref().unwrap().arrows;
    assert_eq!(kept.len(), 1);
    assert_eq!((kept[0].id, &kept[0].arrows), (id, &lines.arrows));
}

#[test]
fn a_tool_whose_shape_has_fields_tells_the_app_where_it_aims() {
    let (sketch, [a, ..]) = drawn();
    let selection = BTreeSet::new();
    // Nothing placed: no fields, only where it snaps, when that changes.
    let tool = testing::tool(Tool::Line, &[], &[]);
    let viewport = viewport(&sketch, &selection, Some(tool), true);
    let mut state = Interaction::default();
    let (messages, _) = feed(
        &viewport,
        &mut state,
        screen_at(1.0, 3.0),
        &[moved(screen_at(1.0, 3.0))],
    );
    assert!(messages.is_empty(), "{messages:?}");

    // A point placed: every move, where the click would go.
    let placed = [DVec2::new(0.0, 5.0)];
    let tool = testing::tool(Tool::Line, &placed, &[None]);
    let viewport = self::viewport(&sketch, &selection, Some(tool), true);
    let aims = |messages: Vec<Message>| match messages.as_slice() {
        [Message::Look(Look::Aim(click))] => *click,
        other => panic!("{other:?}"),
    };
    let at = screen_at(3.0, 7.0);
    let aim = aims(feed(&viewport, &mut state, at, &[moved(at)]).0);
    assert!(aim.at.abs_diff_eq(DVec2::new(3.0, 7.0), 1e-3));
    assert_eq!(aim.target, None);
    let aim = aims(feed(&viewport, &mut state, at, &[moved(at)]).0);
    assert!(aim.at.abs_diff_eq(DVec2::new(3.0, 7.0), 1e-3));
    // Snapped, with where it snaps.
    let near_a = screen_at(-5.0, 0.2);
    let aim = aims(feed(&viewport, &mut state, near_a, &[moved(near_a)]).0);
    assert_eq!(aim.point(), Some(a));
    // Shift lets go of the snap, and says so as an aim.
    let shift = Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::SHIFT));
    let aim = aims(feed(&viewport, &mut state, near_a, &[shift]).0);
    assert_eq!(aim.target, None);
}

#[test]
fn the_preview_holds_what_the_values_typed_fix() {
    let (sketch, _) = drawn();
    let selection = BTreeSet::new();
    let colors = Mode::Light.palette().sketching;
    let placed = [DVec2::new(-2.0, -2.0)];
    let length = [testing::typed(Field::Length, "5")];
    let tool = ActiveTool {
        typed: &length,
        ..testing::tool(Tool::Line, &placed, &[None])
    };
    let viewport = viewport(&sketch, &selection, Some(tool), true);
    let mut state = Interaction::default();
    let cursor = screen_at(1.6, 2.8);
    feed(&viewport, &mut state, cursor, &[moved(cursor)]);
    let (_, live) = layers(&sketching(&sketch, &selection, Some(tool), true), &state);
    // As long as typed, towards the cursor, the point placed at its end.
    let projector = Projector::new(&top_camera(), OriginPlane::XY.placement(), SIZE, SIZE).unwrap();
    let under = projector
        .cursor(DVec2::new(cursor.x.into(), cursor.y.into()))
        .unwrap();
    let end = placed[0] + (under.at - placed[0]).normalize() * 5.0;
    assert!(end.abs_diff_eq(DVec2::new(1.0, 2.0), 1e-3));
    let mut expected = SketchLayer::default();
    let preview = line_style(colors.preview, CURVE_WIDTH, false);
    expected.polyline(Space::Sketch, &[placed[0], end], preview);
    expected.point(end, dot(POINT_RADIUS, colors.preview, colors.preview));
    expected.point(
        placed[0],
        dot(POINT_RADIUS, colors.point_fill, colors.preview),
    );
    assert_eq!(live, expected);

    // A rectangle from its centre, with its construction diagonal.
    let center = [DVec2::new(1.0, 1.0)];
    let tool = ActiveTool {
        centered: true,
        ..testing::tool(Tool::Rectangle, &center, &[None])
    };
    let viewport = self::viewport(&sketch, &selection, Some(tool), true);
    let mut state = Interaction::default();
    let cursor = screen_at(3.0, 4.0);
    feed(&viewport, &mut state, cursor, &[moved(cursor)]);
    let (_, live) = layers(&sketching(&sketch, &selection, Some(tool), true), &state);
    // The cursor is at a corner, the one across from it as far the other
    // way.
    let at = projector
        .cursor(DVec2::new(cursor.x.into(), cursor.y.into()))
        .unwrap()
        .at;
    assert!(at.abs_diff_eq(DVec2::new(3.0, 4.0), 1e-3));
    let across = 2.0 * center[0] - at;
    let corners = [
        across,
        DVec2::new(at.x, across.y),
        at,
        DVec2::new(across.x, at.y),
        across,
    ];
    let mut expected = SketchLayer::default();
    let diagonal = line_style(colors.construction, CURVE_WIDTH, true);
    expected.polyline(Space::Sketch, &[corners[0], corners[2]], diagonal);
    expected.polyline(Space::Sketch, &corners, preview);
    expected.point(at, dot(POINT_RADIUS, colors.preview, colors.preview));
    expected.point(
        center[0],
        dot(POINT_RADIUS, colors.point_fill, colors.preview),
    );
    assert_eq!(live, expected);
}

#[test]
fn a_drawing_tool_s_fields_show_by_where_it_aims() {
    let (sketch, _) = drawn();
    let none = BTreeSet::new();
    let placed = [DVec2::new(0.0, 5.0)];
    let aim = DVec2::new(3.0, 7.0);
    let with = |tool, aim| SketchState {
        aim,
        ..SketchState::plain(&sketch, &none, tool)
    };
    let line = testing::tool(Tool::Line, &placed, &[None]);
    let shown = Sketching::new(with(Some(line), Some(aim)), true);
    assert_eq!(shown.fields().map(|(at, _)| at), Some(aim));
    // Not before the cursor's been anywhere, nor for a shape with none.
    assert!(
        Sketching::new(with(Some(line), None), true)
            .fields()
            .is_none()
    );
    let point = testing::tool(Tool::Point, &[], &[]);
    assert!(
        Sketching::new(with(Some(point), Some(aim)), true)
            .fields()
            .is_none()
    );
    // Nor in a sketch that can't be changed, which has no tool.
    assert!(
        Sketching::new(with(Some(line), Some(aim)), false)
            .fields()
            .is_none()
    );
    // The value field open on one is among them, not by itself.
    let target = ValueTarget::Field(Field::Angle);
    let open = SketchState {
        value: Some(ValueField {
            target: &target,
            text: "",
            error: None,
            in_list: false,
        }),
        ..with(Some(line), Some(aim))
    };
    let open = Sketching::new(open, true);
    assert!(open.field().is_none());
    assert!(open.fields().is_some());

    // A value typed takes the point off what it snapped to: no glyph.
    let snap = Some(Snap {
        at: DVec2::new(-5.0, 0.0),
        target: Some(Target::Point(sketch.points[0].id)),
        inference: None,
    });
    let snapped = |tool| SketchState {
        snap,
        ..with(Some(tool), Some(aim))
    };
    assert!(Sketching::new(snapped(line), true).snap_glyph().is_some());
    let length = [testing::typed(Field::Length, "5")];
    let typed = ActiveTool {
        typed: &length,
        ..line
    };
    assert!(Sketching::new(snapped(typed), true).snap_glyph().is_none());
}

#[test]
fn a_shape_with_fields_stays_aimed_where_the_cursor_left_it() {
    let (sketch, [a, ..]) = drawn();
    let selection = BTreeSet::new();
    let placed = [DVec2::new(0.0, 5.0)];
    let tool = testing::tool(Tool::Line, &placed, &[None]);
    let viewport = viewport(&sketch, &selection, Some(tool), true);
    let mut state = Interaction::default();
    let near_a = screen_at(-5.0, 0.2);
    let (messages, _) = feed(&viewport, &mut state, near_a, &[moved(near_a)]);
    let [Message::Look(Look::Aim(aim))] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    assert_eq!(aim.point(), Some(a));
    let there = |state: &Interaction| {
        let shown = SketchState {
            aim: Some(aim.at),
            snap: Some(aim.snap()),
            ..SketchState::plain(&sketch, &selection, Some(tool))
        };
        layers(&Sketching::new(shown, true), state).1
    };
    let before = there(&state);

    // `Enter` places at the aim, snapped as it was: the snap and its
    // glyph stay, and the preview, as the cursor leaves the viewport...
    let left = Event::Mouse(mouse::Event::CursorLeft);
    let (messages, _) = feed(&viewport, &mut state, near_a, &[left]);
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(there(&state), before);

    // ... or goes over a widget above it.
    let mut state = Interaction::default();
    feed(&viewport, &mut state, near_a, &[moved(near_a)]);
    let over = mouse::Cursor::Levitating(near_a);
    let action = viewport.update(&mut state, &moved(near_a), bounds(), over);
    let messages: Vec<_> = action.and_then(|a| a.into_inner().0).into_iter().collect();
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(there(&state), before);
}

/// A plate from (-8, -6) to (8, 6) with a hole of radius 3 at the origin.
fn plate() -> Sketch {
    testing::plate(8.0, 6.0, 3.0)
}

/// The profiles of `sketch`, as the app finds them.
fn found(sketch: &Sketch) -> Result<Arc<varde_sketch::Profiles>, varde_sketch::TooComplex> {
    sketch.profiles().map(Arc::new)
}

/// `sketch` as the viewport shows it with `profiles`, and `tool` in use.
fn profiled<'a>(
    sketch: &'a Sketch,
    selection: &'a BTreeSet<Id>,
    tool: Option<ActiveTool<'a>>,
    profiles: Option<&'a Result<Arc<varde_sketch::Profiles>, varde_sketch::TooComplex>>,
) -> Sketching<'a> {
    let state = SketchState {
        profiles,
        ..SketchState::plain(sketch, selection, tool)
    };
    Sketching::new(state, true)
}

#[test]
fn each_region_is_shaded_on_its_own_under_the_rest() {
    let sketch = plate();
    let none = BTreeSet::new();
    let profiles = found(&sketch);
    let regions = &profiles.as_ref().unwrap().regions;
    // The plate with its hole, and the hole's inside.
    assert_eq!(regions.len(), 2);
    let state = Interaction::default();
    let (unshaded, _) = layers(&profiled(&sketch, &none, None, None), &state);
    let (base, live) = layers(&profiled(&sketch, &none, None, Some(&profiles)), &state);
    assert!(live.is_empty());

    let colors = Mode::Light.palette().sketching;
    let mut expected = builtins(colors);
    for region in regions {
        fill_region(&mut expected, region, colors.region);
    }
    for entry in &sketch.curves {
        let polyline = sketch.flatten(&entry.curve).unwrap();
        let style = line_style(colors.curve, CURVE_WIDTH, false);
        expected.polyline(Space::Sketch, &polyline, style);
    }
    for point in &sketch.points {
        expected.point(point.at, dot(POINT_RADIUS, colors.point_fill, colors.point));
    }
    assert_eq!(*base, expected);
    assert!(*unshaded != *base);

    // Found again, they're drawn again; too complex, nothing is shaded.
    let again = found(&sketch);
    let (redrawn, _) = layers(&profiled(&sketch, &none, None, Some(&again)), &state);
    assert!(!Arc::ptr_eq(&base, &redrawn));
    let complex = Err(varde_sketch::TooComplex);
    let (plain, _) = layers(&profiled(&sketch, &none, None, Some(&complex)), &state);
    assert_eq!(*plain, *unshaded);
}

#[test]
fn the_region_under_the_cursor_is_highlighted() {
    let sketch = plate();
    let none = BTreeSet::new();
    let profiles = found(&sketch);
    let regions = &profiles.as_ref().unwrap().regions;
    let colors = Mode::Light.palette().sketching;
    let highlighted = |index: usize| {
        let mut layer = SketchLayer::default();
        fill_region(&mut layer, &regions[index], colors.region_hovered);
        layer
    };
    let plate_region = regions.iter().position(|r| !r.holes.is_empty()).unwrap();
    let hole_region = 1 - plate_region;
    let sketching = profiled(&sketch, &none, None, Some(&profiles));
    let viewport = shown(sketching.clone());
    let mut state = Interaction::default();
    let hover = |state: &mut Interaction, x, y| {
        let at = screen_at(x, y);
        let action = viewport.update(state, &moved(at), bounds(), mouse::Cursor::Available(at));
        let live = layers(&sketching, state).1;
        (action.is_some(), live)
    };
    // On the plate, then in its hole, which is a region of its own.
    assert_eq!(
        hover(&mut state, 5.0, 4.0),
        (true, highlighted(plate_region))
    );
    assert_eq!(state.sketch.region, Some(plate_region));
    assert_eq!(
        hover(&mut state, 5.5, 4.0),
        (false, highlighted(plate_region))
    );
    assert_eq!(
        hover(&mut state, 1.0, 1.0),
        (true, highlighted(hole_region))
    );
    // Outside, nothing.
    assert_eq!(hover(&mut state, 10.0, 0.0), (true, SketchLayer::default()));
    assert_eq!(state.sketch.region, None);
    // An item under the cursor is highlighted instead.
    let (_, live) = hover(&mut state, 7.8, 0.0);
    let mut expected = SketchLayer::default();
    let side = [DVec2::new(8.0, -6.0), DVec2::new(8.0, 6.0)];
    expected.polyline(
        Space::Sketch,
        &side,
        line_style(colors.hovered, HOVERED_WIDTH, false),
    );
    assert_eq!(live, expected);

    // Not with a tool, which shows what it would draw instead.
    let tool = testing::tool(Tool::Line, &[], &[]);
    let viewport = shown(profiled(&sketch, &none, Some(tool), Some(&profiles)));
    let mut state = Interaction::default();
    let at = screen_at(5.0, 4.0);
    viewport.update(
        &mut state,
        &moved(at),
        bounds(),
        mouse::Cursor::Available(at),
    );
    assert_eq!(state.sketch.region, None);
}

#[test]
fn open_ends_a_few_pixels_apart_are_marked_at_any_zoom() {
    // Two lines along x, 0.3 apart: 3 pixels through the top camera.
    let mut sketch = Sketch::default();
    for (from, to) in [(-5.0, 0.0), (0.3, 5.0)] {
        let start = sketch.add_point(DVec2::new(from, 0.0)).unwrap();
        let end = sketch.add_point(DVec2::new(to, 0.0)).unwrap();
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    let none = BTreeSet::new();
    let profiles = found(&sketch);
    let sketching = profiled(&sketch, &none, None, Some(&profiles));
    let colors = Mode::Light.palette().sketching;
    let state = Interaction::default();
    let live = |camera: &Camera| {
        sketching
            .layers(
                &state.sketch,
                camera,
                bounds(),
                colors,
                Modifiers::default(),
            )
            .1
    };
    let mut marked = SketchLayer::default();
    marked.point(DVec2::new(0.15, 0.0), ring(colors.near_miss));
    let camera = top_camera();
    assert_eq!(live(&camera), marked);
    // The camera is in `f32`.
    let gap = |state: &Interaction, pixels: f64| {
        let near = state.sketch.near.borrow();
        let gap = near.as_ref().unwrap().gap;
        assert!((gap - NEAR_MISS_GAP / pixels).abs() < 1e-6, "{gap}");
    };
    gap(&state, 10.0);
    // Zoomed in four times, the gap is 12 pixels: no longer a near miss,
    // paired again for the zoom from the same profiles.
    let mut closer = camera;
    closer.zoom(0.25);
    assert_eq!(live(&closer), SketchLayer::default());
    gap(&state, 40.0);
    // Orbiting keeps the zoom, and the pairs.
    let mut orbited = camera;
    orbited.orbit(0.2, 0.1);
    assert_eq!(live(&orbited), marked);
    let near = state.sketch.near.borrow();
    assert!(Arc::ptr_eq(
        &near.as_ref().unwrap().profiles,
        profiles.as_ref().unwrap()
    ));
}

#[test]
fn a_region_stays_highlighted_while_pressed_as_an_item_does() {
    let sketch = plate();
    let none = BTreeSet::new();
    let profiles = found(&sketch);
    let regions = &profiles.as_ref().unwrap().regions;
    let plate_region = regions.iter().position(|r| !r.holes.is_empty()).unwrap();
    let colors = Mode::Light.palette().sketching;
    let mut highlighted = SketchLayer::default();
    fill_region(
        &mut highlighted,
        &regions[plate_region],
        colors.region_hovered,
    );
    let sketching = profiled(&sketch, &none, None, Some(&profiles));
    let viewport = shown(sketching.clone());
    let mut state = Interaction::default();
    let at = screen_at(5.0, 4.0);
    feed(&viewport, &mut state, at, &[moved(at), press()]);
    assert_eq!(layers(&sketching, &state).1, highlighted);
    // Dragging a box, it isn't.
    let to = screen_at(6.0, 5.0);
    feed(&viewport, &mut state, to, &[moved(to)]);
    let unshaded = profiled(&sketch, &none, None, None);
    let boxed = layers(&unshaded, &state).1;
    assert!(!boxed.is_empty(), "the box");
    assert_eq!(layers(&sketching, &state).1, boxed);
}

/// Where the viewport's cursor is in the sketch, with it at `at`.
fn under(at: Point) -> DVec2 {
    let projector = Projector::new(&top_camera(), OriginPlane::XY.placement(), SIZE, SIZE).unwrap();
    let cursor = DVec2::new(at.x.into(), at.y.into());
    projector.cursor(cursor).unwrap().at
}

#[test]
fn the_shape_tools_click_curves_and_show_what_they_would_do() {
    // The line from (-5, 0) to (5, 0), crossed at x = 0, and a wall at
    // x = 8 ahead of it.
    let (mut sketch, [a, _, across]) = drawn();
    let (low, high) = (
        testing::point(&mut sketch, 0.0, -3.0),
        testing::point(&mut sketch, 0.0, 3.0),
    );
    let cutter = testing::line(&mut sketch, low, high);
    let (low, high) = (
        testing::point(&mut sketch, 8.0, -3.0),
        testing::point(&mut sketch, 8.0, 3.0),
    );
    testing::line(&mut sketch, low, high);
    let selection = BTreeSet::new();
    let colors = Mode::Light.palette().sketching;
    let hovered = line_style(colors.hovered, HOVERED_WIDTH, false);

    // By the point at its start, Trim takes the line, and shows the piece
    // up to the crossing going, over the line hovered.
    let trim = testing::tool(Tool::Trim, &[], &[]);
    let viewport = viewport(&sketch, &selection, Some(trim), true);
    let mut state = Interaction::default();
    let by_a = screen_at(-4.6, 0.1);
    assert_eq!(
        tool_click(click(&viewport, &mut state, by_a)).hit,
        Some(across)
    );
    let (_, live) = layers(&sketching(&sketch, &selection, Some(trim), true), &state);
    let mut expected = SketchLayer::default();
    expected.polyline(
        Space::Sketch,
        &sketch
            .flatten(&sketch.curve(across).unwrap().curve)
            .unwrap(),
        hovered,
    );
    let piece = sketch.trim_piece(across, under(by_a)).unwrap();
    assert!(piece[0].abs_diff_eq(sketch.point(a).unwrap().at, 1e-9));
    assert!(piece[1].abs_diff_eq(DVec2::ZERO, 1e-9));
    let gone = line_style(colors.conflict, HOVERED_WIDTH, false);
    expected.polyline(Space::Sketch, &piece, gone);
    assert_eq!(live, expected);

    // Extend shows the line reaching the wall from its nearer end.
    let extend = testing::tool(Tool::Extend, &[], &[]);
    let viewport = self::viewport(&sketch, &selection, Some(extend), true);
    let mut state = Interaction::default();
    let by_end = screen_at(4.0, 0.1);
    assert_eq!(
        tool_click(click(&viewport, &mut state, by_end)).hit,
        Some(across)
    );
    let (_, live) = layers(&sketching(&sketch, &selection, Some(extend), true), &state);
    let mut expected = SketchLayer::default();
    expected.polyline(
        Space::Sketch,
        &sketch
            .flatten(&sketch.curve(across).unwrap().curve)
            .unwrap(),
        hovered,
    );
    let preview = line_style(colors.preview, CURVE_WIDTH, false);
    expected.polyline(
        Space::Sketch,
        &[DVec2::new(5.0, 0.0), DVec2::new(8.0, 0.0)],
        preview,
    );
    assert_eq!(live, expected);

    // Mirror, choosing its line, takes the line under the cursor over the
    // point there, and shows the picks' images in it.
    let picked = [across];
    let mirror = ActiveTool {
        picked: &picked,
        about: true,
        ..testing::tool(Tool::Mirror, &[], &[])
    };
    let viewport = self::viewport(&sketch, &selection, Some(mirror), true);
    let mut state = Interaction::default();
    let on_cutter = screen_at(0.1, 2.8);
    assert_eq!(
        tool_click(click(&viewport, &mut state, on_cutter)).hit,
        Some(cutter)
    );
    let (_, live) = layers(&sketching(&sketch, &selection, Some(mirror), true), &state);
    let mut expected = SketchLayer::default();
    expected.polyline(
        Space::Sketch,
        &[DVec2::new(0.0, -3.0), DVec2::new(0.0, 3.0)],
        hovered,
    );
    let selected = line_style(colors.selected, HOVERED_WIDTH, false);
    expected.polyline(
        Space::Sketch,
        &[DVec2::new(-5.0, 0.0), DVec2::new(5.0, 0.0)],
        selected,
    );
    expected.polyline(
        Space::Sketch,
        &[DVec2::new(5.0, 0.0), DVec2::new(-5.0, 0.0)],
        preview,
    );
    assert_eq!(live, expected);
}

#[test]
fn offset_picks_the_chain_clicked_and_shows_its_copy_through_the_cursor() {
    // A square from (-4, -4) to (4, 4), counter-clockwise.
    let mut sketch = Sketch::default();
    let corners = [(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)]
        .map(|(x, y)| testing::point(&mut sketch, x, y));
    let sides: Vec<Id> = (0..4)
        .map(|i| testing::line(&mut sketch, corners[i], corners[(i + 1) % 4]))
        .collect();
    let selection = BTreeSet::new();
    let colors = Mode::Light.palette().sketching;
    let polyline = |id: Id| sketch.flatten(&sketch.curve(id).unwrap().curve).unwrap();

    // Picking: by a corner, the side is clicked, and its whole loop shows.
    let picking = testing::tool(Tool::Offset, &[], &[]);
    let viewport = viewport(&sketch, &selection, Some(picking), true);
    let mut state = Interaction::default();
    let by_corner = screen_at(-3.7, -4.1);
    let clicked = tool_click(click(&viewport, &mut state, by_corner));
    assert_eq!(clicked.hit, Some(sides[0]));
    let (_, live) = layers(&sketching(&sketch, &selection, Some(picking), true), &state);
    let mut expected = SketchLayer::default();
    let hovered = line_style(colors.hovered, HOVERED_WIDTH, false);
    let chain = sketch.chain_of(sides[0]);
    assert_eq!(chain[0], sides[0]);
    for &side in &chain {
        expected.polyline(Space::Sketch, &polyline(side), hovered);
    }
    assert_eq!(live, expected);

    // Picked: the cursor 2 outside, the loop's copy 2 out, and where it
    // aims said on every move, for its distance's field.
    let offsetting = ActiveTool {
        picked: &sides,
        ..picking
    };
    let viewport = self::viewport(&sketch, &selection, Some(offsetting), true);
    let mut state = Interaction::default();
    let outside = screen_at(0.0, 6.0);
    match feed(&viewport, &mut state, outside, &[moved(outside)])
        .0
        .as_slice()
    {
        [Message::Look(Look::Aim(aim))] => assert!(aim.at.abs_diff_eq(DVec2::new(0.0, 6.0), 1e-3)),
        other => panic!("{other:?}"),
    }
    let shown = sketching(&sketch, &selection, Some(offsetting), true);
    let (_, live) = layers(&shown, &state);
    let mut expected = SketchLayer::default();
    let selected = line_style(colors.selected, HOVERED_WIDTH, false);
    for &side in &sides {
        expected.polyline(Space::Sketch, &polyline(side), selected);
    }
    let (reach, side) = sketch.offset_side(&sides, under(outside)).unwrap();
    assert!((reach - 2.0).abs() < 1e-3);
    let preview = line_style(colors.preview, CURVE_WIDTH, false);
    for copy in sketch.offset_preview(&sides, reach, side).unwrap() {
        assert!(copy.iter().all(|p| p.x.abs().max(p.y.abs()) > 5.99));
        expected.polyline(Space::Sketch, &copy, preview);
    }
    assert_eq!(live, expected);
    // The distance typed holds the copy, on the cursor's side.
    let distance = [testing::typed(Field::Distance, "1")];
    let typed = ActiveTool {
        typed: &distance,
        ..offsetting
    };
    let (_, live) = layers(&sketching(&sketch, &selection, Some(typed), true), &state);
    let mut expected = SketchLayer::default();
    for &side in &sides {
        expected.polyline(Space::Sketch, &polyline(side), selected);
    }
    for copy in sketch.offset_preview(&sides, 1.0, side).unwrap() {
        expected.polyline(Space::Sketch, &copy, preview);
    }
    assert_eq!(live, expected);
    let aimed = SketchState {
        aim: Some(DVec2::new(0.0, 6.0)),
        ..SketchState::plain(&sketch, &selection, Some(offsetting))
    };
    assert!(Sketching::new(aimed, true).fields().is_some());

    // Placed where the button's let go, dragged from the chain or not.
    let on_side = screen_at(0.0, 4.0);
    let messages = drag(&viewport, &mut state, on_side, outside);
    let placed: Vec<_> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Edit(Edit::ToolClick(click)) => Some(click.at),
            _ => None,
        })
        .collect();
    assert_eq!(placed.len(), 1, "{messages:?}");
    assert!(placed[0].abs_diff_eq(DVec2::new(0.0, 6.0), 1e-3));
    let placed = click(&viewport, &mut state, outside);
    assert!(
        placed
            .iter()
            .any(|message| matches!(message, Message::Edit(Edit::ToolClick(_))))
    );
    // `Esc` while it's held lets go of it, placing nothing.
    let escape = escape();
    let (mut messages, captured) = feed(&viewport, &mut state, outside, &[press(), escape]);
    assert!(captured);
    messages.extend(feed(&viewport, &mut state, outside, &[release()]).0);
    assert!(messages.is_empty(), "{messages:?}");
}

/// An L: a line from the corner at the origin to (8, 0) and one from
/// (0, 6) to it, rounded by a fillet of radius 2 made by hand: the
/// sketch, the corner, the lines and the fillet.
fn filleted() -> (Sketch, Id, [Id; 2], Id) {
    let mut sketch = Sketch::default();
    let corner = testing::point(&mut sketch, 0.0, 0.0);
    let along = testing::point(&mut sketch, 8.0, 0.0);
    let up = testing::point(&mut sketch, 0.0, 6.0);
    let a = testing::line(&mut sketch, corner, along);
    let b = testing::line(&mut sketch, up, corner);
    let center = testing::point(&mut sketch, 2.0, 2.0);
    let start = testing::point(&mut sketch, 0.0, 2.0);
    let end = testing::point(&mut sketch, 2.0, 0.0);
    let fillet = sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    sketch.curve_mut(fillet).unwrap().corner = Some(varde_sketch::Corner {
        a: b,
        b: a,
        at: corner,
        equal: false,
    });
    assert_eq!(sketch.check(&testing::DESIGN), Ok(()));
    (sketch, corner, [a, b], fillet)
}

#[test]
fn the_ends_of_lines_a_fillet_cuts_off_are_dashed() {
    let (sketch, _, _, fillet) = filleted();
    let none = BTreeSet::new();
    let (base, _) = layers(
        &sketching(&sketch, &none, None, true),
        &Interaction::default(),
    );
    let colors = Mode::Light.palette().sketching;
    let mut expected = builtins(colors);
    let solid = line_style(colors.curve, CURVE_WIDTH, false);
    let dashed = line_style(colors.curve, CURVE_WIDTH, true);
    let at = DVec2::new;
    expected.polyline(Space::Sketch, &[at(2.0, 0.0), at(8.0, 0.0)], solid);
    expected.polyline(Space::Sketch, &[at(2.0, 0.0), at(0.0, 0.0)], dashed);
    expected.polyline(Space::Sketch, &[at(0.0, 6.0), at(0.0, 2.0)], solid);
    expected.polyline(Space::Sketch, &[at(0.0, 2.0), at(0.0, 0.0)], dashed);
    let arc = sketch
        .flatten(&sketch.curve(fillet).unwrap().curve)
        .unwrap();
    expected.polyline(Space::Sketch, &arc, solid);
    for point in &sketch.points {
        expected.point(point.at, dot(POINT_RADIUS, colors.point_fill, colors.point));
    }
    assert_eq!(*base, expected);
}

#[test]
fn fillet_and_chamfer_pick_a_corner_and_show_what_they_make() {
    // The L without its fillet.
    let (mut sketch, corner, [a, b], fillet) = filleted();
    sketch.delete(&[fillet]);
    let selection = BTreeSet::new();
    let colors = Mode::Light.palette().sketching;
    let polyline = |id: Id| sketch.flatten(&sketch.curve(id).unwrap().curve).unwrap();

    // Picking: a little off the corner, it's what's clicked, and its lines
    // show, the one the click is nearer the way along first.
    for tool in [Tool::Fillet, Tool::Chamfer] {
        let picking = testing::tool(tool, &[], &[]);
        let viewport = viewport(&sketch, &selection, Some(picking), true);
        let mut state = Interaction::default();
        let by_corner = screen_at(0.8, 0.3);
        let clicked = tool_click(click(&viewport, &mut state, by_corner));
        assert_eq!(clicked.hit, Some(corner));
        assert_eq!(sketch.corner_lines(corner, clicked.at), Some([a, b]));
        let (_, live) = layers(&sketching(&sketch, &selection, Some(picking), true), &state);
        let mut expected = SketchLayer::default();
        let hovered = dot(HOVERED_POINT_RADIUS, colors.hovered, colors.hovered);
        expected.point(DVec2::ZERO, hovered);
        for id in [a, b] {
            let style = line_style(colors.hovered, HOVERED_WIDTH, false);
            expected.polyline(Space::Sketch, &polyline(id), style);
        }
        assert_eq!(live, expected);
    }

    // Picked, the fillet whose middle is as far in as the cursor, placed
    // where the button's let go.
    let picked = [corner, a, b];
    let filleting = ActiveTool {
        picked: &picked,
        ..testing::tool(Tool::Fillet, &[], &[])
    };
    assert_eq!(filleting.fields(), [Field::Radius]);
    let viewport = viewport(&sketch, &selection, Some(filleting), true);
    let mut state = Interaction::default();
    let inside = screen_at(1.0, 1.0);
    match feed(&viewport, &mut state, inside, &[moved(inside)])
        .0
        .as_slice()
    {
        [Message::Look(Look::Aim(aim))] => assert!(aim.hit.is_none()),
        other => panic!("{other:?}"),
    }
    let (_, live) = layers(
        &sketching(&sketch, &selection, Some(filleting), true),
        &state,
    );
    // The corner picked, and its lines, as selected.
    let mut expected = SketchLayer::default();
    let dot_selected = dot(HOVERED_POINT_RADIUS, colors.selected, colors.selected);
    expected.point(DVec2::ZERO, dot_selected);
    let selected = line_style(colors.selected, HOVERED_WIDTH, false);
    for id in [a, b] {
        expected.polyline(Space::Sketch, &polyline(id), selected);
    }
    let made = typed::corner_outline(&filleting, &sketch, under(inside)).unwrap();
    let typed::Outline::Fillet { arc, .. } = made else {
        panic!("{made:?}");
    };
    // Its middle where the cursor is.
    let middle = (arc.start + arc.end) / 2.0 - arc.center;
    let middle = arc.center + middle.normalize() * arc.center.distance(arc.start);
    assert!(middle.abs_diff_eq(under(inside), 1e-3));
    let preview = line_style(colors.preview, CURVE_WIDTH, false);
    for polyline in made.polylines() {
        expected.polyline(Space::Sketch, &polyline, preview);
    }
    assert_eq!(live, expected);
    let messages = drag(&viewport, &mut state, screen_at(3.0, 3.0), inside);
    let placed = messages
        .iter()
        .filter(|message| matches!(message, Message::Edit(Edit::ToolClick(_))))
        .count();
    assert_eq!(placed, 1, "{messages:?}");

    // A chamfer as typed: 1 along the first, at 45° to it.
    let typed = [
        testing::typed(Field::Distance, "1"),
        testing::typed(Field::Angle, "45"),
    ];
    let chamfering = ActiveTool {
        picked: &picked,
        typed: &typed,
        ..testing::tool(Tool::Chamfer, &[], &[])
    };
    assert_eq!(
        chamfering.fields(),
        [Field::Distance, Field::SecondDistance, Field::Angle]
    );
    let made = typed::corner_outline(&chamfering, &sketch, under(inside)).unwrap();
    let typed::Outline::Chamfer { ends, .. } = made else {
        panic!("{made:?}");
    };
    assert!(ends[0].abs_diff_eq(DVec2::new(1.0, 0.0), 1e-9));
    assert!(ends[1].abs_diff_eq(DVec2::new(0.0, 1.0), 1e-9));
    assert!((made.value(Field::Angle).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-9);
    assert!((made.value(Field::SecondDistance).unwrap() - 1.0).abs() < 1e-9);
    let aimed = SketchState {
        aim: Some(DVec2::new(1.0, 1.0)),
        ..SketchState::plain(&sketch, &selection, Some(chamfering))
    };
    assert!(Sketching::new(aimed, true).fields().is_some());
}

/// A spline through (-6, 0), (0, 4) and (6, 0), of `kind`, with a handle
/// at its middle point through fit points: the sketch, the spline and
/// the handle's tip, if it has one.
fn arched(kind: SplineKind) -> (Sketch, Id, Option<Id>) {
    let mut sketch = Sketch::default();
    let points: Vec<Id> = [(-6.0, 0.0), (0.0, 4.0), (6.0, 0.0)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap())
        .to_vec();
    let spline = sketch
        .add_curve(
            Curve::Spline(varde_sketch::Spline::through(points.clone(), false)),
            false,
        )
        .unwrap();
    let mut tip = None;
    match kind {
        SplineKind::Through => {
            let at = sketch.add_point(DVec2::new(2.0, 4.5)).unwrap();
            if let Some(Curve::Spline(shape)) = sketch.curve_mut(spline).map(|e| &mut e.curve) {
                shape.handles.push(varde_sketch::Handle {
                    at: points[1],
                    tip: at,
                });
            }
            tip = Some(at);
        }
        SplineKind::Control => sketch.convert_spline(spline, kind).unwrap(),
    }
    (sketch, spline, tip)
}

#[test]
fn a_spline_s_handles_show_and_selected_its_control_polygon() {
    let colors = Mode::Light.palette().sketching;
    let (sketch, spline, tip) = arched(SplineKind::Through);
    let (tip, middle) = (tip.unwrap(), sketch.spline(spline).unwrap().points[1]);
    let at = |id| sketch.point(id).unwrap().at;
    let curve = sketch
        .flatten(&sketch.curve(spline).unwrap().curve)
        .unwrap();
    let base = |selection: &BTreeSet<Id>| {
        layers(
            &sketching(&sketch, selection, None, true),
            &Interaction::default(),
        )
        .0
    };
    // Its handle as a line from its tip through its fit point to as far
    // the other side, it and its tip in the handles' colour.
    let arms = [2.0 * at(middle) - at(tip), at(middle), at(tip)];
    let rim = |id| {
        if id == tip {
            colors.spline_handle
        } else {
            colors.point
        }
    };
    let mut expected = builtins(colors);
    expected.polyline(
        Space::Sketch,
        &curve,
        line_style(colors.curve, CURVE_WIDTH, false),
    );
    let handle = line_style(colors.spline_handle, HANDLE_WIDTH, false);
    expected.polyline(Space::Sketch, &arms, handle);
    let end = |color| dot(POINT_RADIUS, colors.point_fill, color);
    expected.point(arms[0], end(colors.spline_handle));
    for point in &sketch.points {
        expected.point(
            point.at,
            dot(POINT_RADIUS, colors.point_fill, rim(point.id)),
        );
    }
    assert_eq!(*base(&BTreeSet::new()), expected);

    // Selected, its handle in the selection's colour; its other fit
    // points, without handles, show none.
    let selection = BTreeSet::from([spline]);
    let mut expected = builtins(colors);
    let selected = line_style(colors.selected, SELECTED_WIDTH, false);
    expected.polyline(Space::Sketch, &curve, selected);
    let handle = line_style(colors.selected, HANDLE_WIDTH, false);
    expected.polyline(Space::Sketch, &arms, handle);
    expected.point(arms[0], end(colors.selected));
    for point in &sketch.points {
        expected.point(
            point.at,
            dot(POINT_RADIUS, colors.point_fill, rim(point.id)),
        );
    }
    assert_eq!(*base(&selection), expected);

    // By control points, selected: its control polygon, dashed.
    let (sketch, spline, _) = arched(SplineKind::Control);
    let curve = sketch
        .flatten(&sketch.curve(spline).unwrap().curve)
        .unwrap();
    let selection = BTreeSet::from([spline]);
    let (layer, _) = layers(
        &sketching(&sketch, &selection, None, true),
        &Interaction::default(),
    );
    let mut expected = builtins(colors);
    expected.polyline(Space::Sketch, &curve, selected);
    let control = &sketch.spline(spline).unwrap().points;
    let polygon: Vec<DVec2> = control
        .iter()
        .map(|&id| sketch.point(id).unwrap().at)
        .collect();
    let dashed = line_style(colors.spline_handle, HANDLE_WIDTH, true);
    expected.polyline(Space::Sketch, &polygon, dashed);
    for point in &sketch.points {
        let rim = if control.contains(&point.id) {
            colors.spline_handle
        } else {
            colors.point
        };
        expected.point(point.at, dot(POINT_RADIUS, colors.point_fill, rim));
    }
    assert_eq!(*layer, expected);
}

#[test]
fn the_curvature_comb_of_a_spline_selected_shows_scaled_to_the_view() {
    let colors = Mode::Light.palette().sketching;
    let (sketch, spline, _) = arched(SplineKind::Through);
    let selection = BTreeSet::from([spline]);
    let live = |comb: bool, camera: &Camera| {
        let mut state = SketchState::plain(&sketch, &selection, None);
        state.comb = comb;
        let sketching = Sketching::new(state, true);
        sketching
            .layers(
                &Interaction::default().sketch,
                camera,
                bounds(),
                colors,
                Modifiers::default(),
            )
            .1
    };
    let camera = top_camera();
    assert_eq!(live(false, &camera), SketchLayer::default());
    // Its teeth, the longest `COMB_LENGTH` pixels at 10 a unit.
    let pixel = |camera: &Camera| {
        let placement = OriginPlane::XY.placement();
        Projector::new(camera, placement, SIZE, SIZE)
            .unwrap()
            .pixel()
    };
    assert!((pixel(&camera) - 0.1).abs() < 1e-6);
    let teeth = sketch.curvature_comb(spline).unwrap();
    let mut expected = SketchLayer::default();
    combs(
        &mut expected,
        std::slice::from_ref(&teeth),
        pixel(&camera),
        colors.guide,
    );
    assert_eq!(live(true, &camera), expected);
    let most = teeth
        .iter()
        .map(|[_, curving]| curving.length())
        .fold(0.0, f64::max);
    assert!(most > 0.0);
    // Zoomed in, the same on the screen, so shorter on the sketch.
    let mut closer = camera;
    closer.zoom(0.5);
    assert!((pixel(&closer) - 0.05).abs() < 1e-6);
    let mut expected = SketchLayer::default();
    combs(&mut expected, &[teeth], pixel(&closer), colors.guide);
    assert_eq!(live(true, &closer), expected);
}

#[test]
fn a_double_click_on_a_spline_adds_a_point_there() {
    let (sketch, spline, _) = arched(SplineKind::Through);
    let none = BTreeSet::new();
    let viewport = viewport(&sketch, &none, None, true);
    let mut state = Interaction::default();
    let on = sketch.nearest_on(spline, DVec2::new(-3.0, 3.0)).unwrap();
    let at = screen_at(on.x, on.y);
    assert!(matches!(
        click(&viewport, &mut state, at)[..],
        [Message::Look(Look::ClickGeometry { hit: Some(hit), .. })] if hit == spline
    ));
    let second = click(&viewport, &mut state, at);
    let [
        Message::Edit(Edit::InsertSplinePoint {
            spline: on_it,
            at: place,
        }),
    ] = second[..]
    else {
        panic!("{second:?}");
    };
    assert_eq!(on_it, spline);
    assert!(place.distance(on) < 0.2, "{place}");
    // A third is a click again.
    assert!(matches!(
        click(&viewport, &mut state, at)[..],
        [Message::Look(Look::ClickGeometry { .. })]
    ));
    // Read-only, it's only clicks.
    let viewport = self::viewport(&sketch, &none, None, false);
    let mut state = Interaction::default();
    click(&viewport, &mut state, at);
    assert!(matches!(
        click(&viewport, &mut state, at)[..],
        [Message::Look(Look::ClickGeometry { .. })]
    ));
}
