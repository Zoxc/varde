use iced::widget::shader::Program as _;
use iced::{Event, Size};
use varde_expr::LengthUnit;
use varde_render::{Projection, View};

use super::*;
use crate::motion::PatternMode;
use crate::operation_panel::TypedField;
use crate::pick::tests::{SIZE, camera, plate, shown};
use crate::pick::{Picked, Picks};
use crate::theme::Mode;
use crate::viewport::{Interaction, ModelPicking, Operating, Program, program};

fn field(value: f64) -> TypedField<'static> {
    TypedField {
        params: crate::ParamsIn::NONE,
        text: "",
        error: None,
        value: Some(value),
    }
}

/// A move of `kind` picking `picking`, about the Z axis drawn along
/// `line`, its bodies the example's plate (60 × 40 × 10 mm from z 0 up,
/// its box's centre (0, 0, 5)), by nothing.
fn state(kind: MotionKind, picking: MotionPick, line: Option<[DVec3; 2]>) -> MotionState<'static> {
    MotionState {
        kind,
        editing: None,
        bodies: Vec::new(),
        picking,
        fields: [field(0.0); 20],
        reference: Some("Z axis".to_owned()),
        line,
        bounds: Some([DVec3::new(-30.0, -20.0, 0.0), DVec3::new(30.0, 20.0, 10.0)]),
        centre: None,
        origin_axis: Some(Axis3::Z),
        units: LengthUnit::Mm,
        keep_original: true,
        flip: false,
        join: true,
        warning: None,
        mode: PatternMode::Spacing,
        spread_error: None,
        copies: None,
        need: None,
        refused: None,
        error: None,
        show_error: None,
        checking: false,
        ready: false,
        accept: false,
        editable: true,
        hover: None,
        align: None,
        scale: None,
        split: None,
        chamfer: None,
        shell: None,
        fillet: None,
        offset_face: None,
        draft: None,
        sweep: None,
        loft: None,
        knobs: Vec::new(),
    }
}

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(SIZE[0], SIZE[1]))
}

/// Seen from the front, orthographic: 5 pixels a millimetre, so the
/// arrows are 20 mm long, the rings 14 mm across, and the offsets snap
/// to 2 mm.
fn front() -> Camera {
    camera(View::Front, Projection::Orthographic)
}

/// Where the front view shows the world point `at`.
fn at(at: DVec3) -> Point {
    let p = shown(&front(), at);
    Point::new(p.x as f32, p.y as f32)
}

/// The viewport's program setting up `state`, picking `index`'s model if
/// given.
fn viewport<'a>(
    state: MotionState<'a>,
    camera: &'a Camera,
    index: Option<&'a crate::pick::PickIndex>,
) -> Program<'a> {
    let mut program = program(
        &Arc::default(),
        &Arc::default(),
        camera,
        None,
        Mode::Light.palette(),
        None,
        Some(Operating::Motion(Moving::new(state))),
    );
    program.picking = index.map(|index| ModelPicking {
        index,
        hovered: None,
        hovered_snap: None,
        picks: Picks::All,
        snaps: false,
        planes: None,
        hovered_origin: None,
        origin_planes: false,
        sketches: Vec::new(),
        hovered_sketch: None,
        marked: Vec::new(),
        whole: Vec::new(),
    });
    program
}

/// The messages `viewport` sends for each of `events`, the cursor at
/// its point, and whether it captured the last.
fn feed(
    viewport: &Program<'_>,
    input: &mut Interaction,
    events: &[(Event, Point)],
) -> (Vec<Message>, bool) {
    let mut messages = Vec::new();
    let mut captured = false;
    for (event, at) in events {
        let cursor = mouse::Cursor::Available(*at);
        let action = viewport.update(input, event, bounds(), cursor);
        captured = false;
        if let Some(action) = action {
            let (message, _, status) = action.into_inner();
            messages.extend(message);
            captured = status == iced::event::Status::Captured;
        }
    }
    (messages, captured)
}

fn moved(to: Point) -> (Event, Point) {
    (Event::Mouse(mouse::Event::CursorMoved { position: to }), to)
}

fn press(at: Point) -> (Event, Point) {
    let event = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    (event, at)
}

fn release(at: Point) -> (Event, Point) {
    let event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    (event, at)
}

/// The changes to the move `messages` are, each `None` if it's another
/// message.
fn looks(messages: &[Message]) -> Vec<Option<&MotionLook>> {
    (messages.iter())
        .map(|message| match message {
            Message::Look(Look::Motion(look)) => Some(look),
            _ => None,
        })
        .collect()
}

const CENTRE: DVec3 = DVec3::new(0.0, 0.0, 5.0);

#[test]
fn the_axis_and_the_plane_are_drawn_where_they_re_found() {
    let colors = Mode::Light.palette().sketching;
    let scene = Mode::Light.palette().scene;
    let camera = Camera::default();
    let bounds = Rectangle::new(iced::Point::ORIGIN, iced::Size::new(800.0, 600.0));
    let line = Some([DVec3::ZERO, DVec3::Z]);
    let input = Input::default();
    // Picking the axis or plane, so a move has no handles.
    let picking = MotionPick::Reference;
    for kind in [MotionKind::Move, MotionKind::Mirror] {
        let layers = |line| {
            let moving = Moving::new(state(kind, picking, line));
            moving.layers(&input, &scene, colors, &camera, bounds).1
        };
        assert!(!layers(line).is_empty(), "{kind:?}");
        // Nothing found, nothing drawn; nor a direction of nothing.
        assert!(layers(None).is_empty(), "{kind:?}");
        assert!(
            layers(Some([DVec3::ZERO, DVec3::ZERO])).is_empty(),
            "{kind:?}"
        );
    }
}

/// An align draws each side's point and directions where they're
/// known, and nothing else: no handles, no axis.
#[test]
fn an_align_draws_its_points_and_directions() {
    let colors = Mode::Light.palette().sketching;
    let scene = Mode::Light.palette().scene;
    let camera = front();
    let input = Input::default();
    let layers = |marks: [crate::AlignMark; 2]| {
        let mut state = state(MotionKind::Align, MotionPick::Nothing, None);
        state.reference = None;
        state.align = Some(Box::new(crate::AlignView {
            names: Default::default(),
            info: None,
            marks,
            snaps: None,
        }));
        let moving = Moving::new(state);
        moving.layers(&input, &scene, colors, &camera, bounds()).1
    };
    assert!(layers(Default::default()).is_empty());
    let mark = crate::AlignMark {
        point: Some(DVec3::new(30.0, 20.0, 10.0)),
        directions: [Some(DVec3::Z), None],
    };
    assert!(!layers([Default::default(), mark]).is_empty());
    // A point that isn't finite isn't drawn.
    let far = crate::AlignMark {
        point: Some(DVec3::NAN),
        directions: [Some(DVec3::Z), None],
    };
    assert!(layers([far, Default::default()]).is_empty());
}

/// A scale's point is drawn where it's known, and nothing else (no
/// handles, no axis); a point that isn't finite isn't.
#[test]
fn a_scale_draws_its_point() {
    let colors = Mode::Light.palette().sketching;
    let scene = Mode::Light.palette().scene;
    let camera = front();
    let input = Input::default();
    let layers = |at: Option<DVec3>| {
        let mut state = state(MotionKind::Scale, MotionPick::Bodies, None);
        state.reference = None;
        state.scale = Some(Box::new(crate::ScaleView {
            mode: crate::ScaleMode::Uniform,
            point: "Origin".to_owned(),
            at,
            edge: None,
            length: None,
            offered: false,
            axis_only: false,
            info: None,
            snaps: None,
        }));
        let moving = Moving::new(state);
        moving.layers(&input, &scene, colors, &camera, bounds()).1
    };
    assert!(layers(None).is_empty());
    assert!(!layers(Some(DVec3::new(-30.0, -20.0, 0.0))).is_empty());
    assert!(layers(Some(DVec3::NAN)).is_empty());
}

#[test]
fn a_move_has_handles_while_it_picks_bodies_and_a_mirror_none() {
    let colors = Mode::Light.palette().sketching;
    let scene = Mode::Light.palette().scene;
    let camera = front();
    let input = Input::default();
    let layers = |kind, picking, editable| {
        let mut state = state(kind, picking, None);
        state.editable = editable;
        let moving = Moving::new(state);
        moving.layers(&input, &scene, colors, &camera, bounds()).1
    };
    assert!(!layers(MotionKind::Move, MotionPick::Bodies, true).is_empty());
    assert!(layers(MotionKind::Move, MotionPick::Reference, true).is_empty());
    assert!(layers(MotionKind::Move, MotionPick::Bodies, false).is_empty());
    assert!(layers(MotionKind::Mirror, MotionPick::Bodies, true).is_empty());
}

#[test]
fn dragging_the_z_arrow_sets_the_z_offset_snapped() {
    let camera = front();
    let viewport = viewport(
        state(MotionKind::Move, MotionPick::Bodies, None),
        &camera,
        None,
    );
    let mut input = Interaction::default();
    // Over the shaft, halfway up: the arrow is under the cursor.
    let grab = at(CENTRE + DVec3::Z * 10.0);
    let (messages, captured) = feed(&viewport, &mut input, &[moved(grab)]);
    assert!(captured && messages.is_empty(), "{messages:?}");
    assert_eq!(input.motion.hover, Some(Grip::Arrow(Axis3::Z)));
    let (messages, captured) = feed(&viewport, &mut input, &[press(grab)]);
    assert!(captured && messages.is_empty(), "{messages:?}");
    assert_eq!(
        viewport.mouse_interaction(&input, bounds(), mouse::Cursor::Available(grab)),
        mouse::Interaction::Grabbing
    );
    // 6.6 mm up snaps to 6 mm, the steps 2 mm apart at 5 pixels a
    // millimetre.
    let to = at(CENTRE + DVec3::Z * 16.6);
    let (messages, captured) = feed(&viewport, &mut input, &[moved(to)]);
    assert!(captured);
    let six = MotionLook::Input {
        field: MotionField::Offset(Axis3::Z),
        text: "6 mm".to_owned(),
    };
    assert_eq!(looks(&messages), [Some(&six)]);
    // Still 6 mm: nothing more is sent. Down past the centre, below zero.
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(at(CENTRE + DVec3::Z * 16.2))],
    );
    assert!(messages.is_empty(), "{messages:?}");
    let (messages, _) = feed(&viewport, &mut input, &[moved(at(CENTRE - DVec3::Z * 3.0))]);
    let below = MotionLook::Input {
        field: MotionField::Offset(Axis3::Z),
        text: "-14 mm".to_owned(),
    };
    assert_eq!(looks(&messages), [Some(&below)]);
    let (messages, captured) = feed(&viewport, &mut input, &[release(to)]);
    assert!(captured && messages.is_empty(), "{messages:?}");
    assert!(input.motion.drag.is_none());
}

#[test]
fn dragging_a_ring_turns_about_the_centre() {
    let camera = front();
    let viewport = viewport(
        state(MotionKind::Move, MotionPick::Bodies, None),
        &camera,
        None,
    );
    let mut input = Interaction::default();
    // The Y ring's knob, square to the view, on the orb where the layout
    // put it, and dragged on round its ring 89° back: -90° about Y.
    let state = state(MotionKind::Move, MotionPick::Bodies, None);
    let handles =
        (Moving::new(state).handles(&Input::default(), &camera, bounds())).expect("handles");
    let knob = (handles.knobs.iter())
        .find(|knob| knob.axis == Axis3::Y)
        .expect("the Y ring's knob");
    let (from, radius) = (knob.at, knob.radius);
    let on_ring = |degrees: f64| {
        let turn = from + degrees.to_radians();
        at(CENTRE + (DVec3::Z * angle::cos(turn) + DVec3::X * angle::sin(turn)) * radius)
    };
    let grab = on_ring(0.0);
    let (_, captured) = feed(&viewport, &mut input, &[moved(grab), press(grab)]);
    assert!(captured);
    assert_eq!(input.motion.hover, Some(Grip::Ring(Axis3::Y)));
    let (messages, _) = feed(&viewport, &mut input, &[moved(on_ring(-89.0))]);
    // Turning about the Y axis through the origin, then shifted so the
    // centre stays where it is.
    let turn = MotionLook::Turn {
        axis: Axis3::Y,
        angle: "-90°".to_owned(),
        offset: ["5 mm", "0 mm", "5 mm"].map(str::to_owned),
    };
    assert_eq!(looks(&messages), [Some(&turn)]);
    let (_, captured) = feed(&viewport, &mut input, &[release(grab)]);
    assert!(captured);
}

#[test]
fn while_turning_about_another_axis_only_its_ring_shows() {
    let camera = front();
    let mut turned = state(MotionKind::Move, MotionPick::Bodies, None);
    turned.fields[MotionField::Angle.index()] = field(0.5);
    let viewport = viewport(turned.clone(), &camera, None);
    let mut input = Interaction::default();
    let grab = at(CENTRE + DVec3::new(14.0, 0.0, 14.0) / 2f64.sqrt());
    // The Y ring's place: the press goes to the camera.
    feed(&viewport, &mut input, &[moved(grab), press(grab)]);
    assert!(input.motion.hover.is_none() && input.motion.drag.is_none());
    assert!(input.drag.is_some());
    // About an edge, none.
    turned.origin_axis = None;
    let handles = Moving::new(turned).handles(&Input::default(), &camera, bounds());
    assert_eq!(handles.map(|handles| handles.rings), Some([false; 3]));
}

#[test]
fn clicks_off_the_handles_pick_the_model() {
    let index = plate();
    let camera = front();
    let viewport = viewport(
        state(MotionKind::Move, MotionPick::Bodies, None),
        &camera,
        Some(&index),
    );
    let mut input = Interaction::default();
    // The plate's front, low by its right end, clear of the handles.
    let body = at(DVec3::new(25.0, -20.0, 2.0));
    let (messages, _) = feed(&viewport, &mut input, &[moved(body)]);
    assert!(
        matches!(messages[..], [Message::Look(Look::Hover(Some(_)))]),
        "{messages:?}"
    );
    let (messages, _) = feed(&viewport, &mut input, &[press(body), release(body)]);
    let [
        Message::Look(Look::ClickModel {
            pick: Some(pick), ..
        }),
    ] = &messages[..]
    else {
        panic!("{messages:?}");
    };
    assert!(matches!(pick.target, Picked::Face(_)));
    // On the Z arrow's knob, over the plate, the handle takes the click.
    let knob = at(CENTRE + DVec3::Z * 20.0);
    let (messages, captured) = feed(&viewport, &mut input, &[moved(knob), press(knob)]);
    assert!(captured);
    assert!(
        (messages.iter()).all(|message| !matches!(message, Message::Look(Look::ClickModel { .. }))),
        "{messages:?}"
    );
    feed(&viewport, &mut input, &[release(knob)]);
}

#[test]
fn no_handles_while_the_axis_is_picked() {
    let index = plate();
    let camera = front();
    let viewport = viewport(
        state(MotionKind::Move, MotionPick::Reference, None),
        &camera,
        Some(&index),
    );
    let mut input = Interaction::default();
    // Where the Z arrow would be: a click on the model, on the plate's
    // front, as without handles.
    let shaft = at(CENTRE + DVec3::Z * 2.0);
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(shaft), press(shaft), release(shaft)],
    );
    assert!(
        (messages.iter()).any(|message| matches!(
            message,
            Message::Look(Look::ClickModel { pick: Some(_), .. })
        )),
        "{messages:?}"
    );
    assert!(input.motion.hover.is_none() && input.motion.drag.is_none());
}

#[test]
fn angle_steps_are_round_degrees_a_few_pixels_apart() {
    assert_eq!(angle_step(ORB_PIXELS), 5.0);
    assert_eq!(angle_step(1000.0), 1.0);
    assert_eq!(angle_step(1.0), 90.0);
    let turned = turned_about(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0), Axis3::Z, 90.0);
    assert_eq!(turned, Some(DVec3::new(10.0, -10.0, 0.0)));
}

/// A ring's drag turns the moved bodies on about the handles' centre:
/// the move turned by the angles' sum about the world axis through the
/// origin, then shifted by the turned offset, takes every point where the
/// move as it was, then the turn about the centre, takes it; about each
/// world axis, either way.
#[test]
fn a_ring_turns_on_about_the_centre() {
    use varde_kernel::Motion;
    let (offset, centre) = (DVec3::new(1.0, 2.0, 3.0), DVec3::new(5.0, -4.0, 7.0));
    let p = DVec3::new(-2.0, 6.0, 1.5);
    for axis in Axis3::ALL {
        for (was, by) in [(20.0, 35.0), (-50.0, -95.0), (0.0, 90.0)] {
            let before = Motion::turn(DVec3::ZERO, axis.direction(), was).unwrap();
            let before = before.then(&Motion::translation(offset).unwrap());
            let about = Motion::turn(centre, axis.direction(), by).unwrap();
            let expected = about.point(before.point(p));
            let shift = turned_about(offset, centre, axis, by).unwrap();
            let after = Motion::turn(DVec3::ZERO, axis.direction(), was + by).unwrap();
            let after = after.then(&Motion::translation(shift).unwrap());
            let got = after.point(p);
            assert!(
                got.distance(expected) < 1e-12,
                "{axis:?} {was} {by}: {got} {expected}"
            );
        }
    }
}

/// A handle hovered that moves away from a cursor that stays put (the
/// bodies moved, or the camera) is let go of as the next frame is drawn,
/// and the model under the cursor picked again.
#[test]
fn a_handle_that_moves_away_is_let_go_of_as_a_frame_is_drawn() {
    let index = plate();
    let camera = front();
    let here = viewport(
        state(MotionKind::Move, MotionPick::Bodies, None),
        &camera,
        Some(&index),
    );
    let mut input = Interaction::default();
    // The X arrow's shaft, over the plate's front.
    let grab = at(CENTRE + DVec3::X * 10.0);
    feed(&here, &mut input, &[moved(grab)]);
    assert_eq!(input.motion.hover, Some(Grip::Arrow(Axis3::X)));
    let redraw = Event::Window(iced::window::Event::RedrawRequested(
        iced::time::Instant::now(),
    ));
    // Unmoved: still hovered.
    feed(&here, &mut input, &[(redraw.clone(), grab)]);
    assert_eq!(input.motion.hover, Some(Grip::Arrow(Axis3::X)));
    // The bodies' box 30 mm up: the handles went with it.
    let mut away = state(MotionKind::Move, MotionPick::Bodies, None);
    away.bounds = Some([DVec3::new(-30.0, -20.0, 30.0), DVec3::new(30.0, 20.0, 40.0)]);
    let there = viewport(away, &camera, Some(&index));
    let (messages, _) = feed(&there, &mut input, &[(redraw, grab)]);
    assert!(input.motion.hover.is_none());
    assert!(
        matches!(messages[..], [Message::Look(Look::Hover(Some(_)))]),
        "{messages:?}"
    );
}

/// A split picking its tool in a sketch on XY holding a line from
/// (-20, 0) to (20, 0) and a disc of radius 5 about (0, 10): seen from
/// the top, a click on the line picks it while a line is picked, a click
/// in the disc its region while regions are, the model isn't picked
/// under them, and a click off them is left to the camera.
#[test]
fn a_split_s_line_and_regions_are_picked_in_their_sketch() {
    use varde_sketch::{Curve, Sketch};
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::new(-20.0, 0.0)).unwrap();
    let b = sketch.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let center = sketch.add_point(DVec2::new(0.0, 10.0)).unwrap();
    (sketch.add_curve(
        Curve::Circle {
            center,
            radius: 5.0,
        },
        false,
    ))
    .unwrap();
    let profiles = Arc::new(sketch.profiles().unwrap());
    let feature = varde_document::Document::example().features()[0].id;
    let placement = OriginPlane::XY.placement();
    let camera = camera(View::Top, Projection::Orthographic);
    let top = |at: DVec3| {
        let p = shown(&camera, at);
        Point::new(p.x as f32, p.y as f32)
    };
    let index = plate();
    for mode in [SplitMode::Line, SplitMode::Regions] {
        let mut state: MotionState<'_> = state(MotionKind::Split, MotionPick::Tool, None);
        state.split = Some(Box::new(SplitView {
            mode,
            tool: None,
            body: Some("Body 1"),
            original: varde_document::Side::Front,
            keep: varde_document::Keep::Both,
            later: None,
            info: None,
            candidates: vec![crate::Candidate {
                feature,
                placement,
                sketch: &sketch,
                profiles: &profiles,
            }],
            source: None,
            picked: SplitView::none_picked(),
            missing: 0,
            lines: vec![crate::SketchLines {
                feature,
                placement,
                sketch: &sketch,
            }],
            chain: None,
            pieces: Vec::new(),
        }));
        let program = viewport(state, &camera, Some(&index));
        let mut input = Interaction::default();
        let (on, wanted) = match mode {
            SplitMode::Line => (
                DVec3::new(10.0, 0.0, 0.0),
                MotionLook::SplitCurve {
                    sketch: feature,
                    curve: line,
                },
            ),
            _ => (
                DVec3::new(0.0, 10.0, 0.0),
                MotionLook::SplitRegion {
                    sketch: feature,
                    region: 0,
                },
            ),
        };
        let (messages, _) = feed(&program, &mut input, &[moved(top(on))]);
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, Message::Look(Look::Hover(_)))),
            "{mode:?}: the model isn't picked: {messages:?}"
        );
        let (messages, captured) = feed(&program, &mut input, &[press(top(on))]);
        assert_eq!(looks(&messages), [Some(&wanted)], "{mode:?}");
        assert!(captured);
        // Off the sketch's curves and regions: the camera's.
        let off = DVec3::new(25.0, -15.0, 0.0);
        let (messages, _) = feed(&program, &mut input, &[moved(top(off)), press(top(off))]);
        assert!(
            looks(&messages).iter().all(Option::is_none),
            "{mode:?}: {messages:?}"
        );
    }
}

/// A split's pieces, once the preview shows them, are labelled; none
/// without.
#[test]
fn a_split_s_pieces_are_labelled() {
    let camera = front();
    let view = |pieces: Vec<crate::SplitPiece>| {
        let mut state = state(MotionKind::Split, MotionPick::Nothing, None);
        state.split = Some(Box::new(SplitView {
            mode: SplitMode::Body,
            tool: Some(("Body 2".to_owned(), None)),
            body: Some("Body 1"),
            original: varde_document::Side::Front,
            keep: varde_document::Keep::Both,
            later: None,
            info: None,
            candidates: Vec::new(),
            source: None,
            picked: SplitView::none_picked(),
            missing: 0,
            lines: Vec::new(),
            chain: None,
            pieces,
        }));
        Moving::new(state)
    };
    assert!(view(Vec::new()).labels(&camera).is_none());
    let pieces = vec![
        crate::SplitPiece {
            at: DVec3::new(20.0, 0.0, 5.0),
            name: "Body 1".to_owned(),
            keeps: true,
        },
        crate::SplitPiece {
            at: DVec3::new(0.0, 0.0, 5.0),
            name: "New body".to_owned(),
            keeps: false,
        },
    ];
    let moving = view(pieces);
    let labels = moving.labels(&camera).expect("labels");
    let texts = crate::testing::Laid::new(labels, iced::Size::new(SIZE[0], SIZE[1])).texts();
    let names: Vec<&str> = texts.iter().map(|shown| shown.text.as_str()).collect();
    assert_eq!(names, ["Body 1", "New body"]);
}

/// An offset face of the plate's top, its knob on the line from
/// (0, 0, 10) up, at `at` (negative inward).
fn offset_face(at: f64) -> MotionState<'static> {
    let mut state = state(MotionKind::OffsetFace, MotionPick::Faces, None);
    state.reference = None;
    state.knobs = vec![crate::OpKnob {
        field: MotionField::Distance,
        path: crate::KnobPath::Line {
            origin: DVec3::new(0.0, 0.0, 10.0),
            along: DVec3::Z,
        },
        value: at,
        scale: crate::KnobScale::Times(1.0),
        snap: crate::KnobSnap::Length,
        out: DVec3::Z,
        shaft: Some(0.0),
        tone: crate::KnobTone::Modify,
    }];
    state
}

/// A session's knobs are drawn while it has some; a move's handles
/// aren't.
#[test]
fn a_session_draws_its_knobs() {
    let colors = Mode::Light.palette().sketching;
    let scene = Mode::Light.palette().scene;
    let camera = front();
    let input = Input::default();
    let layers = |state: MotionState<'static>| {
        let moving = Moving::new(state);
        moving.layers(&input, &scene, colors, &camera, bounds()).1
    };
    assert!(!layers(offset_face(4.0)).is_empty());
    let mut none = offset_face(4.0);
    none.knobs.clear();
    assert!(layers(none).is_empty());
}

/// A knob dragged along its line: its value snapped as the extrude's
/// handle's, on from where it was grabbed, up out of the body, then down
/// through zero, inward; the knob holds the cursor ahead of the model.
/// Not in a document that can't be changed.
#[test]
fn a_line_knob_drags_through_zero() {
    let camera = front();
    let shown = viewport(offset_face(4.0), &camera, None);
    let viewport = &shown;
    let mut input = Interaction::default();
    let knob = at(DVec3::new(0.0, 0.0, 14.0));
    let (messages, captured) = feed(viewport, &mut input, &[moved(knob)]);
    assert!(captured && messages.is_empty(), "{messages:?}");
    assert!(input.motion.holds());
    let (_, captured) = feed(viewport, &mut input, &[press(knob)]);
    assert!(captured);
    assert_eq!(
        viewport.mouse_interaction(&input, bounds(), mouse::Cursor::Available(knob)),
        mouse::Interaction::Grabbing
    );
    let drag = |input: &mut Interaction, z: f64| {
        let (messages, _) = feed(viewport, input, &[moved(at(DVec3::new(0.0, 0.0, z)))]);
        messages
    };
    let sent = |value: f64| MotionLook::DragKnob {
        knob: 0,
        value,
        step: 2.0,
    };
    // 2.3 mm above the face snaps to 2 mm out.
    assert_eq!(looks(&drag(&mut input, 12.3)), [Some(&sent(2.0))]);
    // The same again sends nothing.
    assert!(drag(&mut input, 12.2).is_empty());
    // 3.7 mm into the body: 4 mm inward.
    assert_eq!(looks(&drag(&mut input, 6.3)), [Some(&sent(-4.0))]);
    let (messages, captured) = feed(viewport, &mut input, &[release(knob)]);
    assert!(captured && messages.is_empty());
    // Off the knob, nothing's held.
    let (_, captured) = feed(
        viewport,
        &mut input,
        &[moved(at(DVec3::new(25.0, 0.0, 2.0)))],
    );
    assert!(!captured && !input.motion.holds());

    let mut locked = offset_face(4.0);
    locked.editable = false;
    let locked = self::viewport(locked, &camera, None);
    let mut input = Interaction::default();
    let (_, captured) = feed(&locked, &mut input, &[moved(knob)]);
    assert!(!captured && !input.motion.holds());
}

/// A knob held as its session ends (under the cursor as OK was hit)
/// doesn't keep the next session's clicks off the model: a session with
/// no knobs lets go of it at its first mouse event.
#[test]
fn a_knob_left_held_lets_go_in_the_next_session() {
    let camera = front();
    let mut input = Interaction::default();
    let knob = at(DVec3::new(0.0, 0.0, 14.0));
    let offset = viewport(offset_face(4.0), &camera, None);
    feed(&offset, &mut input, &[moved(knob)]);
    assert!(input.motion.holds());
    let mut shell = state(MotionKind::Shell, MotionPick::Faces, None);
    shell.reference = None;
    let shell = viewport(shell, &camera, None);
    let (_, captured) = feed(&shell, &mut input, &[moved(knob)]);
    assert!(!captured && !input.motion.holds());
}

/// A move's arrow left under the cursor as its session ends doesn't keep
/// the model unhovered in a sweep picking its path next, which takes the
/// mouse before a move's handles are worked out: the model's edge off
/// the path's curves is hovered at once.
#[test]
fn a_move_s_handle_left_hovered_lets_go_in_a_session_picking_sketches() {
    use crate::motion::{BlendEdges, SweepPath, SweepView};
    let camera = front();
    let mut input = Interaction::default();
    let arrow = at(CENTRE + DVec3::Z * 10.0);
    let moving = viewport(
        state(MotionKind::Move, MotionPick::Bodies, None),
        &camera,
        None,
    );
    feed(&moving, &mut input, &[moved(arrow)]);
    assert_eq!(input.motion.hover, Some(Grip::Arrow(Axis3::Z)));
    let index = plate();
    let mut sweep = state(MotionKind::Sweep, MotionPick::Path, None);
    sweep.sweep = Some(Box::new(SweepView {
        path: SweepPath::Path,
        candidates: Vec::new(),
        source: None,
        picked: SweepView::none_picked(),
        missing: 0,
        parts: Vec::new(),
        edges: BlendEdges::default(),
        lines: Vec::new(),
        chains: Vec::new(),
        keep_orientation: false,
        left_handed: false,
        operation: crate::OperationKind::NewBody,
        targets: Vec::new(),
        info: None,
    }));
    let sweep = viewport(sweep, &camera, Some(&index));
    // The plate's front, low by its right end.
    let body = at(DVec3::new(25.0, -20.0, 2.0));
    let (messages, _) = feed(&sweep, &mut input, &[moved(body)]);
    assert!(!input.motion.holds());
    assert!(
        matches!(messages[..], [Message::Look(Look::Hover(Some(_)))]),
        "{messages:?}"
    );
}

/// A knob dragged round an arc: about -y through the origin from +x, 10
/// mm out (50 pixels, so it snaps to 10°), the angle the cursor turns
/// about the axis; a slider in pixels (a scale's), 100 pixels a factor
/// of 1, snapped to tenths.
#[test]
fn an_arc_knob_turns_and_a_slider_scales() {
    let camera = front();
    let mut arc = offset_face(0.0);
    arc.kind = MotionKind::Draft;
    arc.knobs = vec![crate::OpKnob {
        field: MotionField::Angle,
        path: crate::KnobPath::Arc {
            centre: DVec3::ZERO,
            axis: -DVec3::Y,
            radial: DVec3::X,
            radius: crate::KnobRadius::World(10.0),
        },
        value: 0.0,
        scale: crate::KnobScale::Times(1.0),
        snap: crate::KnobSnap::Angle,
        out: DVec3::Z,
        shaft: Some(0.0),
        tone: crate::KnobTone::Modify,
    }];
    let viewport_arc = viewport(arc, &camera, None);
    let mut input = Interaction::default();
    let knob = at(DVec3::new(10.0, 0.0, 0.0));
    feed(&viewport_arc, &mut input, &[moved(knob), press(knob)]);
    let turned = |degrees: f64| {
        let a = degrees.to_radians();
        at(DVec3::new(
            10.0 * varde_sketch::angle::cos(a),
            0.0,
            10.0 * varde_sketch::angle::sin(a),
        ))
    };
    let (messages, _) = feed(&viewport_arc, &mut input, &[moved(turned(37.0))]);
    let [Some(MotionLook::DragKnob { knob: 0, value, .. })] = looks(&messages)[..] else {
        panic!("{messages:?}");
    };
    assert!(
        (value.to_degrees() - 40.0).abs() < 1e-9,
        "{}",
        value.to_degrees()
    );
    feed(&viewport_arc, &mut input, &[release(knob)]);

    let mut slider = offset_face(1.0);
    slider.kind = MotionKind::Scale;
    slider.knobs[0].path = crate::KnobPath::Line {
        origin: DVec3::ZERO,
        along: DVec3::X,
    };
    slider.knobs[0].scale = crate::KnobScale::Pixels(100.0);
    slider.knobs[0].snap = crate::KnobSnap::Factor;
    let viewport_slider = viewport(slider, &camera, None);
    let mut input = Interaction::default();
    // 100 pixels is 20 mm here.
    let knob = at(DVec3::new(20.0, 0.0, 0.0));
    feed(&viewport_slider, &mut input, &[moved(knob), press(knob)]);
    let (messages, _) = feed(
        &viewport_slider,
        &mut input,
        &[moved(at(DVec3::new(30.4, 0.0, 0.0)))],
    );
    let [Some(MotionLook::DragKnob { knob: 0, value, .. })] = looks(&messages)[..] else {
        panic!("{messages:?}");
    };
    assert!((value - 1.5).abs() < 1e-12, "{value}");
}

/// A sweep in a sketch on XY holding a disc of radius 5 about (0, 10),
/// its profile's candidate, and a path sketch holding a line from
/// (-20, 0) to (20, 0): seen from the top, a click in the disc picks its
/// region while the profile is picked, a click on the line its chain
/// while the path is, the model not hovered over it; off the line, the
/// model's edges are left to the model's picking.
#[test]
fn a_sweep_s_regions_and_path_curves_are_picked_in_their_sketches() {
    use crate::motion::{BlendEdges, SweepPath, SweepView};
    use varde_sketch::{Curve, Sketch};
    let mut disc = Sketch::default();
    let center = disc.add_point(DVec2::new(0.0, 10.0)).unwrap();
    (disc.add_curve(
        Curve::Circle {
            center,
            radius: 5.0,
        },
        false,
    ))
    .unwrap();
    let mut path = Sketch::default();
    let a = path.add_point(DVec2::new(-20.0, 0.0)).unwrap();
    let b = path.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let line = path
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let profiles = Arc::new(disc.profiles().unwrap());
    let features = varde_document::Document::example();
    let [profile, path_feature] = [features.features()[0].id, features.features()[1].id];
    let placement = OriginPlane::XY.placement();
    let camera = camera(View::Top, Projection::Orthographic);
    let top = |at: DVec3| {
        let p = shown(&camera, at);
        Point::new(p.x as f32, p.y as f32)
    };
    let index = plate();
    for picking in [MotionPick::Regions, MotionPick::Path] {
        let mut state: MotionState<'_> = state(MotionKind::Sweep, picking, None);
        state.sweep = Some(Box::new(SweepView {
            path: SweepPath::Path,
            candidates: vec![crate::Candidate {
                feature: profile,
                placement,
                sketch: &disc,
                profiles: &profiles,
            }],
            source: None,
            picked: SweepView::none_picked(),
            missing: 0,
            parts: Vec::new(),
            edges: BlendEdges::default(),
            lines: vec![crate::SketchLines {
                feature: path_feature,
                placement,
                sketch: &path,
            }],
            chains: Vec::new(),
            keep_orientation: false,
            left_handed: false,
            operation: crate::OperationKind::NewBody,
            targets: Vec::new(),
            info: None,
        }));
        let program = viewport(state, &camera, Some(&index));
        let mut input = Interaction::default();
        let (on, wanted) = match picking {
            MotionPick::Path => (
                DVec3::new(10.0, 0.0, 0.0),
                MotionLook::SweepCurve {
                    sketch: path_feature,
                    curve: line,
                },
            ),
            _ => (
                DVec3::new(0.0, 10.0, 0.0),
                MotionLook::SweepRegion {
                    sketch: profile,
                    region: 0,
                },
            ),
        };
        let (messages, _) = feed(&program, &mut input, &[moved(top(on))]);
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, Message::Look(Look::Hover(Some(_))))),
            "{picking:?}: the model isn't hovered: {messages:?}"
        );
        let (messages, captured) = feed(&program, &mut input, &[press(top(on))]);
        assert_eq!(looks(&messages), [Some(&wanted)], "{picking:?}");
        assert!(captured);
        if picking == MotionPick::Path {
            // Straight off the line onto the plate's edge: hovered by
            // the same move, the line's lit chain drawn away after it.
            let edge = DVec3::new(30.0, 5.0, 10.0);
            let (messages, _) = feed(&program, &mut input, &[moved(top(edge))]);
            assert!(
                (messages.iter()).any(|m| matches!(m, Message::Look(Look::Hover(Some(_))))),
                "the model's edge hovered at once: {messages:?}"
            );
            let (messages, _) = feed(&program, &mut input, &[moved(top(on))]);
            assert!(
                (messages.iter()).any(|m| matches!(m, Message::Look(Look::Hover(None)))),
                "back on the line, the model's let go of: {messages:?}"
            );
        }
        let off = DVec3::new(25.0, -15.0, 0.0);
        let (messages, _) = feed(&program, &mut input, &[moved(top(off)), press(top(off))]);
        assert!(
            looks(&messages).iter().all(Option::is_none),
            "{picking:?}: {messages:?}"
        );
    }
}

/// A loft over a sketch on XY holding an 8 × 8 square at (20, 12) as
/// its section starting at its corner (20, 12), a disc of radius 5 about
/// (0, 10) as a candidate, and a sketch holding a point on its own at
/// (-30, 20) and a line from (-20, -20) to (20, -20): seen from the top,
/// while its sections are picked a click on the square's corner
/// (28, 20) moves its start there, one on the point adds it, one in the
/// disc adds its region; while its rails are, one on the line adds its
/// chain; off them, nothing.
#[test]
fn a_loft_s_sections_starts_and_rails_are_picked_in_their_sketches() {
    use crate::motion::{LoftSection, LoftShape, LoftView};
    use varde_sketch::{Curve, Sketch};
    let mut drawn = Sketch::default();
    let center = drawn.add_point(DVec2::new(0.0, 10.0)).unwrap();
    (drawn.add_curve(
        Curve::Circle {
            center,
            radius: 5.0,
        },
        false,
    ))
    .unwrap();
    let corners = [(20.0, 12.0), (28.0, 12.0), (28.0, 20.0), (20.0, 20.0)]
        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end = corners[(k + 1) % 4];
        drawn.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    let mut other = Sketch::default();
    let apex = other.add_point(DVec2::new(-30.0, 20.0)).unwrap();
    let a = other.add_point(DVec2::new(-20.0, -20.0)).unwrap();
    let b = other.add_point(DVec2::new(20.0, -20.0)).unwrap();
    let line = other
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let profiles = Arc::new(drawn.profiles().unwrap());
    let square = (profiles.regions.iter())
        .position(|region| region.bounds.0.x > 15.0)
        .unwrap();
    let disc = 1 - square;
    let features = varde_document::Document::example();
    let [section_sketch, other_sketch] = [features.features()[0].id, features.features()[1].id];
    let placement = OriginPlane::XY.placement();
    let camera = camera(View::Top, Projection::Orthographic);
    let top = |at: DVec3| {
        let p = shown(&camera, at);
        Point::new(p.x as f32, p.y as f32)
    };
    let index = plate();
    let corner_list: Vec<(varde_sketch::Id, DVec2)> = corners
        .iter()
        .map(|&id| (id, drawn.point(id).unwrap().at))
        .collect();
    for picking in [MotionPick::Regions, MotionPick::Path] {
        let mut state: MotionState<'_> = state(MotionKind::Loft, picking, None);
        state.loft = Some(Box::new(LoftView {
            candidates: vec![crate::Candidate {
                feature: section_sketch,
                placement,
                sketch: &drawn,
                profiles: &profiles,
            }],
            lines: vec![crate::SketchLines {
                feature: other_sketch,
                placement,
                sketch: &other,
            }],
            sections: vec![LoftSection {
                name: "Sketch 1".to_owned(),
                gone: false,
                sketch: section_sketch,
                placement: Some(placement),
                shape: LoftShape::Region {
                    region: profiles.regions.get(square),
                    corners: corner_list.clone(),
                    start: Some(corners[0]),
                },
            }],
            rails: Vec::new(),
            chains: Vec::new(),
            mode: varde_document::LoftMode::Smooth,
            closed: false,
            operation: crate::OperationKind::NewBody,
            targets: Vec::new(),
            info: None,
        }));
        let program = viewport(state, &camera, Some(&index));
        let mut input = Interaction::default();
        let clicks: Vec<(DVec3, MotionLook)> = match picking {
            MotionPick::Path => vec![(
                DVec3::new(10.0, -20.0, 0.0),
                MotionLook::LoftRail {
                    sketch: other_sketch,
                    curve: line,
                },
            )],
            _ => vec![
                (
                    DVec3::new(28.0, 20.0, 0.0),
                    MotionLook::LoftStart {
                        section: 0,
                        point: corners[2],
                    },
                ),
                (
                    DVec3::new(-30.0, 20.0, 0.0),
                    MotionLook::LoftPoint {
                        sketch: other_sketch,
                        point: apex,
                    },
                ),
                (
                    DVec3::new(0.0, 10.0, 0.0),
                    MotionLook::LoftRegion {
                        sketch: section_sketch,
                        region: disc,
                    },
                ),
            ],
        };
        for (on, wanted) in clicks {
            let (messages, captured) =
                feed(&program, &mut input, &[moved(top(on)), press(top(on))]);
            let sent: Vec<&MotionLook> = looks(&messages).into_iter().flatten().collect();
            assert_eq!(sent, [&wanted], "{picking:?}");
            assert!(captured);
        }
        let off = DVec3::new(25.0, -15.0, 0.0);
        let (messages, _) = feed(&program, &mut input, &[moved(top(off)), press(top(off))]);
        assert!(
            looks(&messages).iter().all(Option::is_none),
            "{picking:?}: {messages:?}"
        );
    }
}

/// A loft whose section, an 8 × 8 square at (20, 12) on XY, starts at
/// its corner (20, 12), seen from the top, picking nothing: its seam knob
/// there takes the mouse ahead of the model, a grab hand over it; pressed
/// and dragged, the start goes to the corner nearest the cursor, sent
/// once for each corner it reaches; let go of on release. Read-only, no
/// knob.
#[test]
fn a_loft_s_seam_knob_drags_its_start_round_its_corners() {
    use crate::motion::{LoftSection, LoftShape, LoftView};
    use varde_sketch::{Curve, Sketch};
    let mut drawn = Sketch::default();
    let corners = [(20.0, 12.0), (28.0, 12.0), (28.0, 20.0), (20.0, 20.0)]
        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    for (k, &start) in corners.iter().enumerate() {
        let end = corners[(k + 1) % 4];
        drawn.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    let profiles = Arc::new(drawn.profiles().unwrap());
    let features = varde_document::Document::example();
    let sketch = features.features()[0].id;
    let placement = OriginPlane::XY.placement();
    let camera = camera(View::Top, Projection::Orthographic);
    let top = |at: DVec3| {
        let p = shown(&camera, at);
        Point::new(p.x as f32, p.y as f32)
    };
    let index = plate();
    let corner_list: Vec<(varde_sketch::Id, DVec2)> = corners
        .iter()
        .map(|&id| (id, drawn.point(id).unwrap().at))
        .collect();
    let lofting = |editable: bool| {
        let mut state: MotionState<'_> = state(MotionKind::Loft, MotionPick::Nothing, None);
        state.editable = editable;
        state.loft = Some(Box::new(LoftView {
            candidates: Vec::new(),
            lines: Vec::new(),
            sections: vec![LoftSection {
                name: "Sketch 1".to_owned(),
                gone: false,
                sketch,
                placement: Some(placement),
                shape: LoftShape::Region {
                    region: profiles.regions.first(),
                    corners: corner_list.clone(),
                    start: Some(corners[0]),
                },
            }],
            rails: Vec::new(),
            chains: Vec::new(),
            mode: varde_document::LoftMode::Smooth,
            closed: false,
            operation: crate::OperationKind::NewBody,
            targets: Vec::new(),
            info: None,
        }));
        state
    };
    let program = viewport(lofting(true), &camera, Some(&index));
    let mut input = Interaction::default();
    let seam = DVec3::new(20.0, 12.0, 0.0);
    let (_, captured) = feed(&program, &mut input, &[moved(top(seam))]);
    assert!(captured, "the knob takes the mouse");
    assert_eq!(input.motion.seams.hover, Some(0));
    assert!(input.motion.holds());
    let (messages, _) = feed(
        &program,
        &mut input,
        &[
            press(top(seam)),
            moved(top(DVec3::new(21.0, 13.0, 0.0))),
            moved(top(DVec3::new(27.0, 19.0, 0.0))),
            moved(top(DVec3::new(27.5, 19.5, 0.0))),
            release(top(DVec3::new(27.5, 19.5, 0.0))),
        ],
    );
    let sent: Vec<&MotionLook> = looks(&messages).into_iter().flatten().collect();
    assert_eq!(
        sent,
        [&MotionLook::LoftStart {
            section: 0,
            point: corners[2],
        }]
    );
    assert_eq!(input.motion.seams.drag, None, "let go of");

    let program = viewport(lofting(false), &camera, Some(&index));
    let mut input = Interaction::default();
    feed(&program, &mut input, &[moved(top(seam))]);
    assert_eq!(input.motion.seams.hover, None, "read-only");
}

/// A sweep picking its path in a sketch on XY holding a line from
/// (-20, 0) to (20, 0), seen from the top over the plate, the cursor
/// still while the camera moves: a line brought under it is hovered as
/// the frame is drawn, the model let go of (never the model's edge or
/// face hovered under a path curve, where a click takes the curve); the
/// line moved away is let go of, the model under the cursor picked
/// again; and while the camera's dragged, the line hovered is let go of.
#[test]
fn a_sweep_s_path_curve_and_the_model_hand_the_hover_over_as_the_camera_moves() {
    use crate::motion::{BlendEdges, SweepPath, SweepView};
    use varde_sketch::{Curve, Sketch};
    let mut path = Sketch::default();
    let a = path.add_point(DVec2::new(-20.0, 0.0)).unwrap();
    let b = path.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let line = path
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let feature = varde_document::Document::example().features()[0].id;
    let placement = OriginPlane::XY.placement();
    let index = plate();
    let state = || {
        let mut state: MotionState<'_> = state(MotionKind::Sweep, MotionPick::Path, None);
        state.sweep = Some(Box::new(SweepView {
            path: SweepPath::Path,
            candidates: Vec::new(),
            source: None,
            picked: SweepView::none_picked(),
            missing: 0,
            parts: Vec::new(),
            edges: BlendEdges::default(),
            lines: vec![crate::SketchLines {
                feature,
                placement,
                sketch: &path,
            }],
            chains: Vec::new(),
            keep_orientation: false,
            left_handed: false,
            operation: crate::OperationKind::NewBody,
            targets: Vec::new(),
            info: None,
        }));
        state
    };
    let off = camera(View::Top, Projection::Orthographic);
    // Panned so (10, 0) shows where (10, 8) did.
    let mut on = off;
    on.set_target(off.target() - glam::Vec3::new(0.0, 8.0, 0.0));
    let cursor = {
        let p = shown(&off, DVec3::new(10.0, 8.0, 10.0));
        Point::new(p.x as f32, p.y as f32)
    };
    let redraw = || {
        Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))
    };
    let hovering = |viewport: &mut Program<'_>, hovered: bool| {
        if let Some(picking) = &mut viewport.picking {
            picking.hovered = hovered.then_some(Picked::Face(0));
        }
    };
    let mut input = Interaction::default();
    let mut here = viewport(state(), &off, Some(&index));
    let (messages, _) = feed(&here, &mut input, &[moved(cursor)]);
    assert_eq!(input.motion.sweep.curve, None);
    assert!(
        matches!(messages[..], [Message::Look(Look::Hover(Some(_)))]),
        "the plate: {messages:?}"
    );
    // The line brought under the cursor.
    let mut there = viewport(state(), &on, Some(&index));
    hovering(&mut there, true);
    let (messages, _) = feed(&there, &mut input, &[(redraw(), cursor)]);
    assert_eq!(input.motion.sweep.curve, Some((feature, line)));
    assert!(
        matches!(messages[..], [Message::Look(Look::Hover(None))]),
        "{messages:?}"
    );
    hovering(&mut there, false);
    let (messages, _) = feed(&there, &mut input, &[(redraw(), cursor)]);
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(input.motion.sweep.curve, Some((feature, line)));
    // Moved away again: let go of, the plate picked.
    hovering(&mut here, false);
    let (messages, _) = feed(&here, &mut input, &[(redraw(), cursor)]);
    assert_eq!(input.motion.sweep.curve, None);
    assert!(
        matches!(messages[..], [Message::Look(Look::Hover(Some(_)))]),
        "the plate again: {messages:?}"
    );
    // Hovered, then the camera dragged with the right button: let go of.
    feed(&there, &mut input, &[(redraw(), cursor)]);
    assert_eq!(input.motion.sweep.curve, Some((feature, line)));
    let right = |pressed: bool| {
        let button = mouse::Button::Right;
        Event::Mouse(if pressed {
            mouse::Event::ButtonPressed(button)
        } else {
            mouse::Event::ButtonReleased(button)
        })
    };
    let far = Point::new(cursor.x + 40.0, cursor.y + 40.0);
    feed(
        &there,
        &mut input,
        &[(right(true), cursor), moved(far), (redraw(), far)],
    );
    assert!(input.drag.is_some(), "the camera's dragged");
    assert_eq!(input.motion.sweep.curve, None);
    feed(&there, &mut input, &[(right(false), far)]);
}

/// A loft picking its rails and a split picking its line, each in a
/// sketch on XY holding a line from (-20, 0) to (20, 0), seen from the
/// top, the cursor still while the camera moves: the line brought under
/// it is hovered as the frame is drawn, moved away let go of, and let go
/// of while the camera's dragged.
#[test]
fn a_loft_s_and_a_split_s_curves_are_hovered_again_as_the_camera_moves() {
    use crate::motion::LoftView;
    use varde_sketch::{Curve, Sketch};
    let mut path = Sketch::default();
    let a = path.add_point(DVec2::new(-20.0, 0.0)).unwrap();
    let b = path.add_point(DVec2::new(20.0, 0.0)).unwrap();
    let line = path
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let profiles = Arc::new(path.profiles().unwrap());
    let feature = varde_document::Document::example().features()[0].id;
    let placement = OriginPlane::XY.placement();
    let lines = || {
        vec![crate::SketchLines {
            feature,
            placement,
            sketch: &path,
        }]
    };
    let loft = || {
        let mut state: MotionState<'_> = state(MotionKind::Loft, MotionPick::Path, None);
        state.loft = Some(Box::new(LoftView {
            candidates: Vec::new(),
            lines: lines(),
            sections: Vec::new(),
            rails: Vec::new(),
            chains: Vec::new(),
            mode: varde_document::LoftMode::Smooth,
            closed: false,
            operation: crate::OperationKind::NewBody,
            targets: Vec::new(),
            info: None,
        }));
        state
    };
    let split = || {
        let mut state: MotionState<'_> = state(MotionKind::Split, MotionPick::Tool, None);
        state.split = Some(Box::new(SplitView {
            mode: SplitMode::Line,
            tool: None,
            body: Some("Body 1"),
            original: varde_document::Side::Front,
            keep: varde_document::Keep::Both,
            later: None,
            info: None,
            candidates: vec![crate::Candidate {
                feature,
                placement,
                sketch: &path,
                profiles: &profiles,
            }],
            source: None,
            picked: SplitView::none_picked(),
            missing: 0,
            lines: lines(),
            chain: None,
            pieces: Vec::new(),
        }));
        state
    };
    let off = camera(View::Top, Projection::Orthographic);
    // Panned so (10, 0) shows where (10, 8) did.
    let mut on = off;
    on.set_target(off.target() - glam::Vec3::new(0.0, 8.0, 0.0));
    let cursor = {
        let p = shown(&off, DVec3::new(10.0, 8.0, 0.0));
        Point::new(p.x as f32, p.y as f32)
    };
    let redraw = || {
        Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))
    };
    let right = |pressed: bool| {
        let button = mouse::Button::Right;
        Event::Mouse(if pressed {
            mouse::Event::ButtonPressed(button)
        } else {
            mouse::Event::ButtonReleased(button)
        })
    };
    let hovered = |input: &Interaction, lofting: bool| {
        if lofting {
            input.motion.loft.curve
        } else {
            input.motion.split.curve
        }
    };
    for lofting in [true, false] {
        let state = || if lofting { loft() } else { split() };
        let here = viewport(state(), &off, None);
        let there = viewport(state(), &on, None);
        let mut input = Interaction::default();
        feed(&here, &mut input, &[moved(cursor)]);
        assert_eq!(hovered(&input, lofting), None, "{lofting}");
        // The line brought under the cursor.
        feed(&there, &mut input, &[(redraw(), cursor)]);
        assert_eq!(hovered(&input, lofting), Some((feature, line)), "{lofting}");
        // Moved away again: let go of.
        feed(&here, &mut input, &[(redraw(), cursor)]);
        assert_eq!(hovered(&input, lofting), None, "{lofting}");
        // Hovered, then the camera dragged: let go of.
        feed(&there, &mut input, &[(redraw(), cursor)]);
        assert_eq!(hovered(&input, lofting), Some((feature, line)), "{lofting}");
        let far = Point::new(cursor.x + 40.0, cursor.y + 40.0);
        feed(
            &there,
            &mut input,
            &[(right(true), cursor), moved(far), (redraw(), far)],
        );
        assert!(input.drag.is_some(), "the camera's dragged");
        assert_eq!(hovered(&input, lofting), None, "{lofting}");
        feed(&there, &mut input, &[(right(false), far)]);
    }
}

/// A loft picking its sections and a split picking regions, each in a
/// sketch on XY holding a square from (0, 0) to (10, 10), seen from the
/// top, the cursor still while the camera moves: the square's region (and
/// for the loft, with the square a section, its corner) brought under it
/// is hovered as the frame is drawn, and let go of once moved away.
#[test]
fn a_loft_s_and_a_split_s_regions_and_corners_are_hovered_again_as_the_camera_moves() {
    use crate::motion::{LoftSection, LoftShape, LoftView};
    use varde_sketch::{Curve, Sketch};
    let mut square = Sketch::default();
    let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        .map(|(x, y)| square.add_point(DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let (start, end) = (corners[k], corners[(k + 1) % 4]);
        square.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    let profiles = Arc::new(square.profiles().unwrap());
    let feature = varde_document::Document::example().features()[0].id;
    let placement = OriginPlane::XY.placement();
    let candidates = || {
        vec![crate::Candidate {
            feature,
            placement,
            sketch: &square,
            profiles: &profiles,
        }]
    };
    let loft = |section: bool| {
        let mut state: MotionState<'_> = state(MotionKind::Loft, MotionPick::Regions, None);
        let sections = if section {
            vec![LoftSection {
                name: "Sketch 1".to_owned(),
                gone: false,
                sketch: feature,
                placement: Some(placement),
                shape: LoftShape::Region {
                    region: profiles.regions.first(),
                    corners: corners
                        .iter()
                        .map(|&id| (id, square.point(id).unwrap().at))
                        .collect(),
                    // Not the corner under the cursor: its seam knob
                    // would take it.
                    start: Some(corners[1]),
                },
            }]
        } else {
            Vec::new()
        };
        state.loft = Some(Box::new(LoftView {
            candidates: candidates(),
            lines: Vec::new(),
            sections,
            rails: Vec::new(),
            chains: Vec::new(),
            mode: varde_document::LoftMode::Smooth,
            closed: false,
            operation: crate::OperationKind::NewBody,
            targets: Vec::new(),
            info: None,
        }));
        state
    };
    let split = || {
        let mut state: MotionState<'_> = state(MotionKind::Split, MotionPick::Tool, None);
        state.split = Some(Box::new(SplitView {
            mode: SplitMode::Regions,
            tool: None,
            body: Some("Body 1"),
            original: varde_document::Side::Front,
            keep: varde_document::Keep::Both,
            later: None,
            info: None,
            candidates: candidates(),
            source: None,
            picked: SplitView::none_picked(),
            missing: 0,
            lines: Vec::new(),
            chain: None,
            pieces: Vec::new(),
        }));
        state
    };
    let redraw = || {
        Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))
    };
    let off = camera(View::Top, Projection::Orthographic);
    // Panned 30 along x, then 20 more: the cursor over (−25, 5) is over
    // (5, 5), the square's middle, then its corner (0, 0) one way and
    // nothing the other.
    let pan = |by: glam::Vec3| {
        let mut moved = off;
        moved.set_target(off.target() + by);
        moved
    };
    let cursor = |at: DVec3| {
        let p = shown(&off, at);
        Point::new(p.x as f32, p.y as f32)
    };
    // The square's middle and its first corner brought under the cursor.
    let middle = pan(glam::Vec3::new(30.0, 0.0, 0.0));
    let corner = pan(glam::Vec3::new(25.0, -5.0, 0.0));
    let at = cursor(DVec3::new(-25.0, 5.0, 0.0));
    for kind in 0..3 {
        let state = || match kind {
            0 => loft(false),
            1 => loft(true),
            _ => split(),
        };
        let here = viewport(state(), &off, None);
        let there = viewport(state(), &middle, None);
        let mut input = Interaction::default();
        feed(&here, &mut input, &[moved(at)]);
        let region = |input: &Interaction| match kind {
            2 => input.motion.split.regions.hover,
            _ => input.motion.loft.region,
        };
        assert_eq!(region(&input), None, "{kind}");
        feed(&there, &mut input, &[(redraw(), at)]);
        assert_eq!(region(&input), Some((feature, 0)), "{kind}");
        feed(&here, &mut input, &[(redraw(), at)]);
        assert_eq!(region(&input), None, "{kind}");
        if kind == 1 {
            // The section's corner, ahead of its region.
            let near = viewport(state(), &corner, None);
            feed(&near, &mut input, &[(redraw(), at)]);
            assert_eq!(input.motion.loft.corner, Some((0, corners[0])));
            assert_eq!(input.motion.loft.region, None);
            feed(&here, &mut input, &[(redraw(), at)]);
            assert_eq!(input.motion.loft.corner, None);
        }
    }
}

/// From all round, in both projections, the handles keep apart on the
/// screen: no two knobs' grab areas (13 pixels round each) meet, nor
/// does a ring's knob or arc come within reach of an arrow's shaft or
/// knob.
#[test]
fn the_move_s_arrows_and_rings_keep_apart_from_all_round() {
    let state = state(MotionKind::Move, MotionPick::Bodies, None);
    let moving = Moving::new(state);
    for projection in [Projection::Orthographic, Projection::Perspective] {
        for view in [View::Front, View::Top, View::Right, View::Bottom] {
            for (yaw, pitch) in [(0.0, 0.0), (0.4, 0.3), (-0.9, 0.6), (1.7, -0.5), (2.6, 1.2)] {
                let mut camera = camera(view, projection);
                camera.orbit(yaw, pitch);
                let case = format!("{view:?} {projection:?} {yaw} {pitch}");
                let handles = moving
                    .handles(&Input::default(), &camera, bounds())
                    .expect(&case);
                let knobs: Vec<DVec2> = (handles.arrows.iter().map(|arrow| arrow.puck.screen()))
                    .chain(handles.knobs.iter().map(|knob| knob.puck.screen()))
                    .collect();
                for (k, a) in knobs.iter().enumerate() {
                    for b in &knobs[k + 1..] {
                        assert!(a.distance(*b) > 26.0, "{case}: knobs {a} {b}");
                    }
                }
                for knob in &handles.knobs {
                    for arrow in &handles.arrows {
                        let shaft = [handles.centre, arrow.tip];
                        let near = handles.distance_to_polyline(&shaft, knob.puck.screen());
                        assert!(near > 13.0 + HIT_PIXELS, "{case}: knob by an arrow, {near}");
                        let tip = arrow.puck.screen();
                        let near = handles.distance_to_polyline(&knob.arc, tip);
                        assert!(near > 13.0 + HIT_PIXELS, "{case}: arc by a knob, {near}");
                    }
                }
                assert!(!handles.arrows.is_empty(), "{case}");
            }
        }
    }
}
