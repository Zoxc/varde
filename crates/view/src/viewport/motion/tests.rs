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
        fields: [field(0.0); 17],
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
    // The Y ring, square to the view, at 45° from Z towards X, and dragged
    // a quarter turn back past Z: -90° about Y.
    let on_ring = |degrees: f64| {
        let turn = degrees.to_radians();
        at(CENTRE + (DVec3::Z * angle::cos(turn) + DVec3::X * angle::sin(turn)) * 14.0)
    };
    let grab = on_ring(45.0);
    let (_, captured) = feed(&viewport, &mut input, &[moved(grab), press(grab)]);
    assert!(captured);
    assert_eq!(input.motion.hover, Some(Grip::Ring(Axis3::Y)));
    let (messages, _) = feed(&viewport, &mut input, &[moved(on_ring(-44.0))]);
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
    assert_eq!(angle_step(RING_PIXELS), 5.0);
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

/// An offset face of the plate's top, its handle from (0, 0, 10) up,
/// the knob at `at` (negative inward).
fn offset_face(at: f64) -> MotionState<'static> {
    let mut state = state(MotionKind::OffsetFace, MotionPick::Faces, None);
    state.reference = None;
    state.offset_face = Some(Box::new(crate::OffsetFaceView {
        faces: crate::PickedFaces::default(),
        inward: at < 0.0,
        tangent: true,
        handle: Some(crate::FaceHandle {
            origin: DVec3::new(0.0, 0.0, 10.0),
            normal: DVec3::Z,
            at,
        }),
        info: None,
    }));
    state
}

/// An offset face's handle is drawn while there's one and the document
/// can be changed; a move's handles aren't.
#[test]
fn an_offset_face_draws_its_handle() {
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
    none.offset_face.as_mut().unwrap().handle = None;
    assert!(layers(none).is_empty());
    let mut locked = offset_face(4.0);
    locked.editable = false;
    assert!(layers(locked).is_empty());
}

/// The knob dragged: the distance snapped as the extrude's handle's, up
/// out of the body, then down through zero, inward; zero itself is
/// never sent; the handle holds the cursor ahead of the model.
#[test]
fn the_offset_face_handle_drags_through_zero_turning_inward() {
    let camera = front();
    let viewport = viewport(offset_face(4.0), &camera, None);
    let mut input = Interaction::default();
    let knob = at(DVec3::new(0.0, 0.0, 14.0));
    let (messages, captured) = feed(&viewport, &mut input, &[moved(knob)]);
    assert!(captured && messages.is_empty(), "{messages:?}");
    assert!(input.motion.holds());
    let (_, captured) = feed(&viewport, &mut input, &[press(knob)]);
    assert!(captured);
    assert_eq!(
        viewport.mouse_interaction(&input, bounds(), mouse::Cursor::Available(knob)),
        mouse::Interaction::Grabbing
    );
    // 2.3 mm above the face snaps to 2 mm out.
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(at(DVec3::new(0.0, 0.0, 12.3)))],
    );
    let out = MotionLook::OffsetBy {
        distance: "2 mm".to_owned(),
        inward: false,
    };
    assert_eq!(looks(&messages), [Some(&out)]);
    // At the face: zero, which isn't sent.
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(at(DVec3::new(0.0, 0.0, 10.2)))],
    );
    assert!(messages.is_empty(), "{messages:?}");
    // 3.7 mm into the body: 4 mm inward.
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(at(DVec3::new(0.0, 0.0, 6.3)))],
    );
    let inward = MotionLook::OffsetBy {
        distance: "4 mm".to_owned(),
        inward: true,
    };
    assert_eq!(looks(&messages), [Some(&inward)]);
    let (messages, captured) = feed(&viewport, &mut input, &[release(knob)]);
    assert!(captured && messages.is_empty());
    assert!(input.motion.face.drag.is_none());
    // Off the handle, nothing's held.
    let (_, captured) = feed(
        &viewport,
        &mut input,
        &[moved(at(DVec3::new(25.0, 0.0, 2.0)))],
    );
    assert!(!captured && !input.motion.holds());
}

/// An offset face's handle held as its session ends (under the cursor
/// as OK was hit) doesn't keep the next session's clicks off the model:
/// a shell's first mouse event lets go of it.
#[test]
fn an_offset_face_handle_left_held_lets_go_in_the_next_session() {
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

/// Zoomed in as far as the camera goes, the knob snaps finer than the
/// design's units show a distance: dragged to where its distance would
/// show as nothing ("0 mm", which no offset is), nothing's sent, as at
/// zero; a little further, the distance is sent as it shows.
#[test]
fn the_offset_face_handle_sends_no_distance_that_shows_as_nothing() {
    let mut camera = front();
    camera.set_target(glam::Vec3::new(0.0, 0.0, 10.0));
    camera.zoom(1e-9);
    let point = |p: DVec3| {
        let p = shown(&camera, p);
        Point::new(p.x as f32, p.y as f32)
    };
    // How far a pixel is here, and the knob's snap.
    let pixel = 1.0
        / (shown(&camera, DVec3::new(0.0, 0.0, 10.0)) - shown(&camera, DVec3::new(0.0, 0.0, 11.0)))
            .length();
    let step = crate::extrude::snap_step(pixel, LengthUnit::Mm).expect("a step");
    assert!(step < 1e-4, "{step}");
    let start = 10.0 * step;
    let viewport = viewport(offset_face(start), &camera, None);
    let mut input = Interaction::default();
    let knob = point(DVec3::new(0.0, 0.0, 10.0 + start));
    feed(&viewport, &mut input, &[moved(knob)]);
    let (_, captured) = feed(&viewport, &mut input, &[press(knob)]);
    assert!(captured);
    // Two steps up from the face: it would show as "0 mm".
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(point(DVec3::new(0.0, 0.0, 10.0 + 2.0 * step)))],
    );
    assert!(messages.is_empty(), "{messages:?}");
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(point(DVec3::new(0.0, 0.0, 10.0 - 2.0 * step)))],
    );
    assert!(messages.is_empty(), "{messages:?}");
    // A micrometre into the body shows (the snap a fraction of it).
    let (messages, _) = feed(
        &viewport,
        &mut input,
        &[moved(point(DVec3::new(0.0, 0.0, 10.0 - 1e-3)))],
    );
    let inward = MotionLook::OffsetBy {
        distance: "0.001 mm".to_owned(),
        inward: true,
    };
    assert_eq!(looks(&messages), [Some(&inward)]);
}
