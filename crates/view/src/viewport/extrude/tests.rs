use iced::widget::shader::Program as _;
use iced::{Event, Point, Size};
use varde_document::{OriginPlane, Plane};
use varde_expr::LengthUnit;

use super::*;
use crate::extrude::{DistanceField, ExtentKind, OperationKind};
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
    let field = DistanceField {
        text: "10",
        error: None,
        value: Some(10.0),
    };
    ExtrudeState {
        editing: None,
        candidates: vec![crate::Candidate {
            feature,
            plane: Plane::Origin(plane),
            profiles,
        }],
        source: source.then_some(feature),
        picked,
        missing: 0,
        extent: ExtentKind::OneSide,
        fields: [field; 2],
        flip: false,
        operation: OperationKind::NewBody,
        targets: Vec::new(),
        grabbed,
        error: None,
        refused: None,
        checking: false,
        ready: !picked.is_empty(),
        editable: true,
        units: LengthUnit::Mm,
    }
}

/// The viewport's program setting up `state`, through [`top_camera`].
fn shown(state: ExtrudeState<'_>) -> Program<'_> {
    program(
        &Arc::default(),
        &Arc::default(),
        &top_camera(),
        Mode::Light.palette(),
        None,
        Some(Extruding::new(state)),
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
    let frame = draw(&viewport).sketch.expect("drawn");
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
    let frame = draw(&viewport).sketch.expect("drawn");
    assert!(!frame.base.is_empty());
    assert!(!frame.live.is_empty());
    // The base layer is kept while nothing it shows changes.
    let again = draw(&viewport).sketch.unwrap();
    assert!(Arc::ptr_eq(&frame.base, &again.base));
}

#[test]
fn an_extrude_its_own_check_refuses_draws_no_shaft() {
    // Two sides together over the limit: no preview for the shaft to
    // stand on, which would be a line on its own.
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    let input = Interaction::default();
    let mut state = state(&profiles, feature, OriginPlane::XY, true, &picked, None);
    let shaft = |state: ExtrudeState<'_>| {
        let viewport = shown(state);
        let frame = viewport.draw(&input, mouse::Cursor::Unavailable, bounds());
        !frame.sketch.expect("drawn").live.is_empty()
    };
    assert!(shaft(state.clone()));
    state.refused = Some(ExtrudeError::Length);
    assert!(!shaft(state));
}

/// A box from the origin to (2, 2, 2), tessellated.
fn cube() -> RenderMesh {
    let tol = varde_kernel::Tolerance::DEFAULT;
    let solid = varde_kernel::Solid::cuboid(DVec3::ZERO, DVec3::splat(2.0), 0, &tol);
    let display = varde_kernel::Display::new(&tol);
    solid.unwrap().tessellate(&display).unwrap()
}

#[test]
fn a_knob_the_model_is_in_front_of_is_hidden() {
    // From the top, over a box from the origin to (2, 2, 2): beneath it,
    // inside it, on its top face, above it and beside it.
    let mesh = cube();
    let mut perspective = top_camera();
    perspective.set_projection(Projection::Perspective);
    for camera in [top_camera(), perspective] {
        for (z, behind) in [(-1.0, true), (1.0, true), (2.0, false), (3.0, false)] {
            let at = DVec3::new(1.0, 1.5, z);
            assert_eq!(hidden(&mesh, &camera, at), behind, "{camera:?} at {at}");
        }
        assert!(!hidden(&mesh, &camera, DVec3::new(5.0, 1.0, -1.0)));
        assert!(!hidden(
            &RenderMesh::default(),
            &camera,
            DVec3::new(1.0, 1.0, -1.0)
        ));
    }
    // From below, the other way round.
    let mut camera = top_camera();
    camera.look_from(varde_render::View::Bottom);
    assert!(hidden(&mesh, &camera, DVec3::new(1.0, 1.0, 3.0)));
    assert!(!hidden(&mesh, &camera, DVec3::new(1.0, 1.0, 0.0)));
}

#[test]
fn knobs_the_model_hides_are_left_out() {
    // One side, 10 mm up from the plate's ring on XY: a box around the
    // plate up to 20 mm hides the knob from the top.
    let (profiles, feature) = plate();
    let picked = BTreeSet::from([ring(&profiles)]);
    let extruding = Extruding::new(state(
        &profiles,
        feature,
        OriginPlane::XY,
        true,
        &picked,
        None,
    ));
    let tol = varde_kernel::Tolerance::DEFAULT;
    let solid = varde_kernel::Solid::cuboid(DVec3::splat(-20.0), DVec3::splat(40.0), 0, &tol);
    let display = varde_kernel::Display::new(&tol);
    let mesh = solid.unwrap().tessellate(&display).unwrap();
    let knob = extruding.handle.as_ref().unwrap();
    let at = knob.origin + knob.normal * knob.knobs[0].1;
    assert!(hidden(&mesh, &top_camera(), at));
    assert!(!hidden(&RenderMesh::default(), &top_camera(), at));
    let shown = |mesh: &RenderMesh| extruding.shown_knobs(&top_camera(), mesh).count();
    assert_eq!(shown(&RenderMesh::default()), 1);
    assert_eq!(shown(&mesh), 0);
}
