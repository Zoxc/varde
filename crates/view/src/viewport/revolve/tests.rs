use std::collections::BTreeSet;

use iced::widget::shader::Program as _;
use iced::{Event, Size};
use varde_document::{OriginPlane, Plane};
use varde_sketch::{Id, Profiles};

use super::*;
use crate::operation_panel::{OperationKind, TypedField};
use crate::projection::top_camera;
use crate::revolve::TurnKind;
use crate::testing;
use crate::theme::Mode;
use crate::viewport::{Interaction, Operating, Program, program};

/// The viewport's side, in pixels: through [`top_camera`], a unit is 10
/// pixels, the origin in the middle, y up.
const SIZE: f32 = 200.0;

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(SIZE, SIZE))
}

/// A sketch on XY of a rectangle from (2, -4) to (6, 4), its left side
/// from (2, 4) to (2, -4), and its profiles.
fn lathe() -> (Sketch, Id, Arc<Profiles>) {
    let mut sketch = Sketch::default();
    let corners = [(2.0, -4.0), (6.0, -4.0), (6.0, 4.0), (2.0, 4.0)]
        .map(|(x, y)| testing::point(&mut sketch, x, y));
    let sides: Vec<Id> = (0..4)
        .map(|k| testing::line(&mut sketch, corners[k], corners[(k + 1) % 4]))
        .collect();
    let profiles = Arc::new(sketch.profiles().unwrap());
    (sketch, sides[3], profiles)
}

fn feature() -> FeatureId {
    let mut editor = varde_document::Editor::new(Default::default());
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    editor.document().features()[0].id
}

/// A revolve of `sketch`, its source, picking `picking` first, about
/// `axis` if given.
fn state<'a>(
    sketch: &'a Sketch,
    profiles: &'a Arc<Profiles>,
    picked: &'a BTreeSet<usize>,
    picking: RevolvePick,
    axis: Option<AxisLine>,
) -> RevolveState<'a> {
    let feature = feature();
    let field = TypedField {
        text: "90",
        error: None,
        value: Some(std::f64::consts::FRAC_PI_2),
    };
    RevolveState {
        editing: None,
        candidates: vec![Candidate {
            feature,
            placement: OriginPlane::XY.placement(),
            sketch,
            profiles,
        }],
        source: Some(feature),
        picked,
        missing: 0,
        axis,
        axis_missing: false,
        picking,
        extent: TurnKind::Full,
        fields: [field; 2],
        flip: false,
        operation: OperationKind::NewBody,
        targets: Vec::new(),
        error: None,
        refused: None,
        held: None,
        checking: false,
        ready: axis.is_some() && !picked.is_empty(),
        editable: true,
    }
}

fn shown(state: RevolveState<'_>) -> Program<'_> {
    program(
        &Arc::default(),
        &Arc::default(),
        &top_camera(),
        None,
        Mode::Light.palette(),
        None,
        Some(Operating::Revolve(Revolving::new(state))),
    )
}

/// What a left press at `at` sends, after moving there.
fn click(viewport: &Program<'_>, at: Point) -> Vec<Message> {
    let mut input = Interaction::default();
    let mut messages = Vec::new();
    let events = [
        Event::Mouse(mouse::Event::CursorMoved { position: at }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ];
    for event in &events {
        let action = viewport.update(&mut input, event, bounds(), mouse::Cursor::Available(at));
        if let Some(action) = action {
            messages.extend(action.into_inner().0);
        }
    }
    messages
}

/// The axis a click at `at` picks, if it picks one.
fn axis_picked(viewport: &Program<'_>, at: Point) -> Option<AxisLine> {
    match click(viewport, at)[..] {
        [Message::Look(Look::Revolve(RevolveLook::PickAxis { axis, .. }))] => Some(axis),
        _ => None,
    }
}

/// Whether a click at `at` picks a region.
fn region_picked(viewport: &Program<'_>, at: Point) -> bool {
    matches!(
        click(viewport, at)[..],
        [Message::Look(Look::Revolve(RevolveLook::PickRegion {
            region: 0,
            ..
        }))]
    )
}

#[test]
fn while_the_axis_is_picked_lines_and_axes_go_before_regions() {
    let (sketch, left, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let viewport = shown(state(&sketch, &profiles, &picked, RevolvePick::Axis, None));
    // (0, 5) on the y axis, (0.5, 0) near the x axis, within the 10 its
    // drawn: past the sketch's points by a quarter, at least 10.
    assert_eq!(
        axis_picked(&viewport, Point::new(100.0, 50.0)),
        Some(AxisLine::SketchY)
    );
    assert_eq!(
        axis_picked(&viewport, Point::new(195.0, 100.0)),
        Some(AxisLine::SketchX)
    );
    // Within a few pixels of the left side, inside the rectangle: a line
    // goes before an axis (here the x axis) as near.
    assert_eq!(
        axis_picked(&viewport, Point::new(122.0, 100.0)),
        Some(AxisLine::Curve(left))
    );
    // Well inside, (4, 2), the region.
    assert!(region_picked(&viewport, Point::new(140.0, 80.0)));
    // Off them all, nothing: the left button orbits.
    assert!(click(&viewport, Point::new(160.0, 10.0)).is_empty());
}

#[test]
fn while_regions_are_picked_lines_are_not() {
    let (sketch, _, profiles) = lathe();
    let picked = BTreeSet::new();
    let viewport = shown(state(
        &sketch,
        &profiles,
        &picked,
        RevolvePick::Regions,
        None,
    ));
    assert!(region_picked(&viewport, Point::new(122.0, 100.0)));
    assert!(click(&viewport, Point::new(100.0, 50.0)).is_empty());
}

#[test]
fn the_arrow_follows_the_line_and_flip() {
    let (sketch, left, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let mut revolve = state(
        &sketch,
        &profiles,
        &picked,
        RevolvePick::Regions,
        Some(AxisLine::Curve(left)),
    );
    let ends = |state: &RevolveState<'_>| {
        let revolving = Revolving::new(state.clone());
        revolving.pointed().map(|(_, ends)| ends)
    };
    // From the line's start to its end.
    let down = [DVec2::new(2.0, 4.0), DVec2::new(2.0, -4.0)];
    assert_eq!(ends(&revolve), Some(down));
    // Flip turns one side the other way: the arrow turns round.
    revolve.extent = TurnKind::OneSide;
    revolve.flip = true;
    assert_eq!(ends(&revolve), Some([down[1], down[0]]));
    // Symmetric ignores it.
    revolve.extent = TurnKind::Symmetric;
    assert_eq!(ends(&revolve), Some(down));
    // A built-in axis is drawn across its reach, along +x.
    revolve.axis = Some(AxisLine::SketchX);
    let reach = axis_reach(&sketch);
    assert_eq!(reach, 10.0);
    assert_eq!(
        ends(&revolve),
        Some([DVec2::new(-reach, 0.0), DVec2::new(reach, 0.0)])
    );
    // The layers draw it, and the lines to pick while picking.
    let input = Input::default();
    let colors = Mode::Light.palette().sketching;
    let (_, live) = Revolving::new(revolve.clone()).layers(&input, colors, &top_camera(), bounds());
    assert!(!live.is_empty());
    revolve.axis = None;
    let (_, live) = Revolving::new(revolve.clone()).layers(&input, colors, &top_camera(), bounds());
    assert!(live.is_empty());
    revolve.picking = RevolvePick::Axis;
    let (_, live) = Revolving::new(revolve).layers(&input, colors, &top_camera(), bounds());
    assert!(!live.is_empty());
}

/// Random sketches (degenerate and far lines among them), cameras and
/// cursors: clicks pick only what the state lets them, an axis only of
/// the source's lines and axes, and drawing never panics.
#[test]
fn random_clicks_pick_only_what_they_may() {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        seed.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let mut unit = move || (next() >> 11) as f64 / (1u64 << 53) as f64;
    let (mut axes, mut regions) = (0, 0);
    for round in 0..200 {
        let (mut sketch, left, _) = lathe();
        let scale = [1.0, 1e3, 1e6, 1e-3][round % 4];
        // A line of no length, one far off, one through the origin.
        let a = testing::point(&mut sketch, 3.0 * scale, 3.0 * scale);
        let b = testing::point(&mut sketch, 3.0 * scale, 3.0 * scale);
        let degenerate = testing::line(&mut sketch, a, b);
        let c = testing::point(&mut sketch, -1e9, 5.0);
        let d = testing::point(&mut sketch, 1e9, 5.0 + scale);
        testing::line(&mut sketch, c, d);
        let profiles = Arc::new(sketch.profiles().unwrap_or_default());
        let picked = BTreeSet::from([0]);
        let picking = [RevolvePick::Axis, RevolvePick::Regions][round % 2];
        let axis = [
            None,
            Some(AxisLine::SketchX),
            Some(AxisLine::SketchY),
            Some(AxisLine::Curve(left)),
            Some(AxisLine::Curve(degenerate)),
        ][(round / 2) % 5];
        let mut revolve = state(&sketch, &profiles, &picked, picking, axis);
        revolve.extent = TurnKind::ALL[round % 4];
        revolve.flip = round % 3 == 0;
        let mut camera = top_camera();
        camera.orbit((unit() * 6.0 - 3.0) as f32, (unit() * 3.0 - 1.5) as f32);
        camera.zoom((unit() * 4.0 + 0.1) as f32);
        let revolving = Revolving::new(revolve.clone());
        let colors = Mode::Light.palette().sketching;
        for _ in 0..20 {
            let at = Point::new(
                (unit() * 240.0 - 20.0) as f32,
                (unit() * 240.0 - 20.0) as f32,
            );
            let mut input = Input::default();
            let cursor = mouse::Cursor::Available(at);
            let moved = mouse::Event::CursorMoved { position: at };
            let _ = revolving.mouse(&mut input, moved, bounds(), cursor, &camera);
            let _ = revolving.layers(&input, colors, &camera, bounds());
            let pressed = mouse::Event::ButtonPressed(mouse::Button::Left);
            let action = revolving.mouse(&mut input, pressed, bounds(), cursor, &camera);
            if let Some(message) = action.and_then(|action| action.into_inner().0) {
                match message {
                    Message::Look(Look::Revolve(RevolveLook::PickAxis { axis, .. })) => {
                        assert_eq!(picking, RevolvePick::Axis, "round {round}");
                        axes += 1;
                        assert!(
                            axis_line(&sketch, axis).is_some(),
                            "round {round}: {axis:?}"
                        );
                    }
                    Message::Look(Look::Revolve(RevolveLook::PickRegion { region, .. })) => {
                        assert!(region < profiles.regions.len(), "round {round}");
                        regions += 1;
                    }
                    other => panic!("round {round}: {other:?}"),
                }
            }
        }
    }
    eprintln!("{axes} axes, {regions} regions picked");
    assert!(axes > 20 && regions > 20, "{axes} {regions}");
}
