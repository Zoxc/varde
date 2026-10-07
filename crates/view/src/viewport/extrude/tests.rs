use iced::widget::shader::Program as _;
use iced::{Event, Point, Size};
use std::collections::BTreeSet;

use varde_document::{FeatureId, OriginPlane, Plane};
use varde_expr::LengthUnit;
use varde_sketch::Profiles;

use super::*;
use crate::extrude::ExtentKind;
use crate::operation_panel::{OperationKind, TypedField};
use crate::projection::top_camera;
use crate::testing;
use crate::theme::Mode;
use crate::viewport::{Interaction, Program, program};
use varde_document::ExtrudeError;

/// The viewport's side, in pixels: through [`top_camera`], a unit is 10
/// pixels, the origin in the middle.
const SIZE: f32 = 200.0;

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(SIZE, SIZE))
}

/// A plate from (-8, -6) to (8, 6) with a hole of radius 3 at the origin,
/// its profiles, and a sketch feature's id.
fn plate() -> (Arc<Profiles>, FeatureId) {
    let profiles = Arc::new(testing::plate(8.0, 6.0, 3.0).profiles().unwrap());
    let mut editor = varde_document::Editor::new(Default::default());
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    (profiles, editor.document().features()[0].id)
}

/// The ring of `profiles`: the plate less its hole.
fn ring(profiles: &Profiles) -> usize {
    profiles
        .regions
        .iter()
        .position(|region| region.holes.len() == 1)
        .unwrap()
}

/// An extrude of the `picked` regions of `profiles` on `plane`, from the
/// sketch `feature`, its source if `source`, 10 mm one side, with the
/// knob `grabbed` grabbed.
fn state<'a>(
    profiles: &'a Arc<Profiles>,
    feature: FeatureId,
    plane: OriginPlane,
    source: bool,
    picked: &'a BTreeSet<usize>,
    grabbed: Option<Distance>,
) -> ExtrudeState<'a> {
    let field = TypedField {
        params: crate::ParamsIn::NONE,
        text: "10",
        error: None,
        value: Some(10.0),
    };
    ExtrudeState {
        editing: None,
        candidates: vec![crate::Candidate {
            feature,
            placement: plane.placement(),
            sketch: Box::leak(Box::default()),
            profiles,
        }],
        source: source.then_some(feature),
        picked,
        missing: 0,
        extent: ExtentKind::OneSide,
        fields: [field; 2],
        flip: false,
        taper: TypedField {
            params: crate::ParamsIn::NONE,
            text: "0°",
            error: None,
            value: Some(0.0),
        },
        operation: OperationKind::NewBody,
        targets: Vec::new(),
        grabbed,
        error: None,
        show_error: None,
        refused: None,
        held: None,
        uncut: None,
        checking: false,
        ready: !picked.is_empty(),
        accept: false,
        editable: true,
        units: LengthUnit::Mm,
        hover: None,
    }
}

/// The viewport's program setting up `state`, through [`top_camera`].
fn shown(state: ExtrudeState<'_>) -> Program<'_> {
    program(
        &Arc::default(),
        &Arc::default(),
        &top_camera(),
        None,
        Mode::Light.palette(),
        None,
        Some(crate::viewport::Operating::Extrude(Box::new(
            Extruding::new(state),
        ))),
    )
}

/// The messages `viewport` sends for `events` with the cursor at `at`,
/// and whether it captured the last.
fn feed(
    viewport: &Program<'_>,
    input: &mut Interaction,
    at: Point,
    events: &[Event],
) -> (Vec<Message>, bool) {
    let mut messages = Vec::new();
    let mut captured = false;
    for event in events {
        let action = viewport.update(input, event, bounds(), mouse::Cursor::Available(at));
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

#[test]
fn a_click_on_a_region_picks_it_and_off_them_orbits() {
    let (profiles, feature) = plate();
    let picked = BTreeSet::new();
    let viewport = shown(state(
        &profiles,
        feature,
        OriginPlane::XY,
        false,
        &picked,
        None,
    ));
    let mut input = Interaction::default();

    // The ring, 5 right of the origin.
    let at = Point::new(150.0, 100.0);
    let (messages, captured) = feed(&viewport, &mut input, at, &[moved(at), press()]);
    let region = ring(&profiles);
    assert!(captured);
    assert!(
        matches!(
            messages[..],
            [Message::Look(Look::Extrude(ExtrudeLook::PickRegion { sketch, region: r }))]
                if sketch == feature && r == region
        ),
        "{messages:?}"
    );
    assert!(input.drag.is_none());
    feed(&viewport, &mut input, at, &[release()]);

    // The hole is a region of its own.
    let at = Point::new(100.0, 100.0);
    let (messages, _) = feed(&viewport, &mut input, at, &[moved(at), press()]);
    assert!(matches!(
        messages[..],
        [Message::Look(Look::Extrude(ExtrudeLook::PickRegion { region: r, .. }))] if r != region
    ));
    feed(&viewport, &mut input, at, &[release()]);

    // Off the plate, the left button orbits, as outside a session.
    let at = Point::new(190.0, 10.0);
    let (messages, _) = feed(&viewport, &mut input, at, &[moved(at), press()]);
    assert!(messages.is_empty(), "{messages:?}");
    assert!(input.drag.is_some());
}

#[test]
fn a_grabbed_knob_follows_the_cursor_along_the_axis_snapped() {
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    // On XZ, whose normal is -Y: seen from the top, down the screen.
    let grabbed = state(
        &profiles,
        feature,
        OriginPlane::XZ,
        true,
        &picked,
        Some(Distance::First),
    );
    let viewport = shown(grabbed);
    let mut input = Interaction::default();
    let at = Point::new(120.0, 131.4);
    let (messages, captured) = feed(&viewport, &mut input, at, &[moved(at)]);
    assert!(captured);
    let [Message::Look(Look::Extrude(ExtrudeLook::DragHandle { distance, to }))] = messages[..]
    else {
        panic!("{messages:?}");
    };
    assert_eq!(distance, Distance::First);
    // 3.14 along it, snapped to whole millimetres at 0.1 mm a pixel.
    assert_eq!(to, 3.0);
    let (messages, _) = feed(&viewport, &mut input, at, &[release()]);
    assert!(matches!(
        messages[..],
        [Message::Look(Look::Extrude(ExtrudeLook::DropHandle))]
    ));
}

#[test]
fn looking_along_the_axis_drags_nothing() {
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    let grabbed = state(
        &profiles,
        feature,
        OriginPlane::XY,
        true,
        &picked,
        Some(Distance::First),
    );
    let viewport = shown(grabbed);
    let mut input = Interaction::default();
    let at = Point::new(120.0, 130.0);
    let (messages, _) = feed(&viewport, &mut input, at, &[moved(at)]);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn the_regions_and_the_shaft_are_drawn() {
    let (profiles, feature) = plate();
    let none = BTreeSet::new();
    let input = Interaction::default();
    let draw = |viewport: &Program<'_>| viewport.draw(&input, mouse::Cursor::Unavailable, bounds());

    // Before a source, every candidate's regions, each on its plane.
    let viewport = shown(state(
        &profiles,
        feature,
        OriginPlane::XY,
        false,
        &none,
        None,
    ));
    let frame = draw(&viewport).sketch.clone().expect("drawn");
    assert!(frame.base.is_empty());
    assert!(!frame.live.is_empty());
    // Hidden by the model in front of it.
    assert!(frame.depth_tested);

    // Once picked, on the plane, and the handle's shaft along its axis.
    let picked = BTreeSet::from([ring(&profiles)]);
    let viewport = shown(state(
        &profiles,
        feature,
        OriginPlane::XZ,
        true,
        &picked,
        None,
    ));
    let frame = draw(&viewport).sketch.clone().expect("drawn");
    assert!(!frame.base.is_empty());
    assert!(!frame.live.is_empty());
    // The base layer is kept while nothing it shows changes.
    let again = draw(&viewport).sketch.clone().unwrap();
    assert!(Arc::ptr_eq(&frame.base, &again.base));
}

#[test]
fn an_extrude_its_own_check_refuses_draws_no_shaft() {
    // Two sides together over the limit: no preview for the shaft to
    // stand on, which would be a line on its own. The knob's puck stays.
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    let input = Interaction::default();
    let mut state = state(&profiles, feature, OriginPlane::XY, true, &picked, None);
    let live = |state: ExtrudeState<'_>| {
        let viewport = shown(state);
        let frame = viewport.draw(&input, mouse::Cursor::Unavailable, bounds());
        frame.sketch.clone().expect("drawn").live
    };
    let whole = live(state.clone());
    state.refused = Some(ExtrudeError::Length);
    let refused = live(state);
    assert!(!refused.is_empty());
    assert_ne!(whole, refused);
}

/// One side, 5 mm along -Y from the plate's ring on XZ: seen from the top,
/// the knob 50 pixels below the middle, its ring edge on across the
/// screen and its arrow on down it.
fn knob_state<'a>(
    profiles: &'a Arc<Profiles>,
    feature: FeatureId,
    picked: &'a BTreeSet<usize>,
) -> ExtrudeState<'a> {
    let mut state = state(profiles, feature, OriginPlane::XZ, true, picked, None);
    state.fields[0].value = Some(5.0);
    state
}

#[test]
fn a_knob_is_hovered_and_grabbed_ahead_of_the_regions() {
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    let viewport = shown(knob_state(&profiles, feature, &picked));
    let mut input = Interaction::default();
    // On its ring, its arrow, and the ring's edge: hovered and captured,
    // with the grab hand; a press grabs it.
    for at in [(100.0, 150.0), (100.0, 166.0), (112.0, 150.0)] {
        let at = Point::new(at.0, at.1);
        let (messages, captured) = feed(&viewport, &mut input, at, &[moved(at)]);
        assert!(captured, "{at:?}");
        assert!(messages.is_empty(), "{messages:?}");
        let cursor = mouse::Cursor::Available(at);
        assert_eq!(
            viewport.mouse_interaction(&input, bounds(), cursor),
            mouse::Interaction::Grab
        );
        let (messages, captured) = feed(&viewport, &mut input, at, &[press()]);
        assert!(captured);
        assert!(
            matches!(
                messages[..],
                [Message::Look(Look::Extrude(ExtrudeLook::GrabHandle(
                    Distance::First
                )))]
            ),
            "{messages:?}"
        );
    }
    // Off it, nothing's hovered.
    let at = Point::new(140.0, 150.0);
    let (_, captured) = feed(&viewport, &mut input, at, &[moved(at)]);
    assert!(!captured);
    assert_ne!(
        viewport.mouse_interaction(&input, bounds(), mouse::Cursor::Available(at)),
        mouse::Interaction::Grab
    );
}

#[test]
fn a_knob_is_drawn_lighter_with_its_rail_while_hovered() {
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    let viewport = shown(knob_state(&profiles, feature, &picked));
    let mut input = Interaction::default();
    let draw = |input: &Interaction| {
        let frame = viewport.draw(input, mouse::Cursor::Unavailable, bounds());
        frame.sketch.clone().expect("drawn").live
    };
    let idle = draw(&input);
    let at = Point::new(100.0, 150.0);
    feed(&viewport, &mut input, at, &[moved(at)]);
    assert_ne!(draw(&input), idle);
    let at = Point::new(160.0, 20.0);
    feed(&viewport, &mut input, at, &[moved(at)]);
    assert_eq!(draw(&input), idle);
}

#[test]
fn a_knob_is_grabbed_behind_the_model_but_not_read_only() {
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    let at = Point::new(100.0, 150.0);
    let grabs = |viewport: &Program<'_>| {
        let mut input = Interaction::default();
        let (messages, _) = feed(viewport, &mut input, at, &[moved(at), press()]);
        matches!(
            messages[..],
            [Message::Look(Look::Extrude(ExtrudeLook::GrabHandle(_)))]
        )
    };
    assert!(grabs(&shown(knob_state(&profiles, feature, &picked))));

    let mut state = knob_state(&profiles, feature, &picked);
    state.editable = false;
    assert!(!grabs(&shown(state)));

    // Inside a box around it, seen from the top: the handle is drawn
    // over the model, so it shows and grabs.
    let tol = varde_kernel::Tolerance::DEFAULT;
    let solid =
        varde_kernel::Solid::cuboid(glam::DVec3::splat(-20.0), glam::DVec3::splat(40.0), 0, &tol);
    let display = varde_kernel::Display::new(&tol);
    let mesh = Arc::new(solid.unwrap().tessellate(&display).unwrap());
    let state = knob_state(&profiles, feature, &picked);
    let viewport = program(
        &mesh,
        &Arc::default(),
        &top_camera(),
        None,
        Mode::Light.palette(),
        None,
        Some(crate::viewport::Operating::Extrude(Box::new(
            Extruding::new(state),
        ))),
    );
    assert!(grabs(&viewport));
}
