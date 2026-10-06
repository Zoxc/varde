use std::collections::BTreeSet;

use iced::widget::shader::Program as _;
use iced::{Event, Size};
use varde_document::{OriginPlane, Plane};
use varde_sketch::{Id, Profiles};

use super::*;
use crate::operation_panel::{OperationKind, TypedField};
use crate::pick::PickIndex;
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
        edge_ends: None,
        edge_body: None,
        index: crate::pick::empty_index(),
        resolution: varde_kernel::Tolerance::DEFAULT.resolution(),
        picking,
        extent: TurnKind::Full,
        fields: [field; 2],
        flip: false,
        operation: OperationKind::NewBody,
        targets: Vec::new(),
        error: None,
        show_error: None,
        refused: None,
        held: None,
        uncut: None,
        checking: false,
        ready: axis.is_some() && !picked.is_empty(),
        accept: false,
        editable: true,
        hover: None,
        grabbed: None,
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
/// the source's lines and axes, a knob only of the extent's angles, and
/// drawing never panics.
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
                    // A knob, of an angle the extent has.
                    Message::Look(Look::Revolve(RevolveLook::GrabHandle(angle))) => {
                        assert!(revolve.extent.angles().contains(&angle), "round {round}");
                    }
                    other => panic!("round {round}: {other:?}"),
                }
            }
        }
    }
    eprintln!("{axes} axes, {regions} regions picked");
    assert!(axes > 20 && regions > 20, "{axes} {regions}");
}

/// A block from z = −5 up to 0, x from −6 to −3 and y from −6 to 6: its
/// top edges lie in XY, the right one at x = −3, 30 pixels left of the
/// middle through [`top_camera`] (where y = 3 is 30 pixels above it). Made ready for picking as model 5.
fn block() -> PickIndex {
    use varde_document::{BodyId, Command, Document, Editor, Extent, Extrude, Operation};
    let mut editor = Editor::new(Document::default());
    let plane = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let feature = editor.document().features()[0].id;
    let mut sketch = Sketch::default();
    let corners = [(-6.0, -6.0), (-3.0, -6.0), (-3.0, 6.0), (-6.0, 6.0)]
        .map(|(x, y)| testing::point(&mut sketch, x, y));
    for k in 0..4 {
        testing::line(&mut sketch, corners[k], corners[(k + 1) % 4]);
    }
    let region = sketch.profiles().unwrap().reference(0).unwrap();
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let design = editor.document().design();
    let depth = varde_expr::Value::new("5", &Extent::ask(&design)).unwrap();
    let extrude = Extrude {
        taper: None,
        sketch: feature,
        regions: vec![region],
        extent: Extent::OneSide(depth),
        flip: true,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let document = editor.document().clone();
    let mut cache = varde_regen::Cache::default();
    let evaluation = varde_regen::evaluate(&document, &mut cache);
    let (mesh, picking) =
        varde_regen::tessellate_picking(&document, &evaluation, &mut cache).unwrap();
    PickIndex::new(mesh, picking, 5)
}

/// While the axis is picked, a model edge in the source's plane is
/// picked after the sketch's lines and before its regions, drawn and
/// hovered; while regions are, edges aren't. The arrow on a model edge
/// runs between its ends, mapped onto the source.
#[test]
fn model_edges_in_the_plane_can_be_the_axis() {
    let (sketch, left, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let index = block();
    let revolve = RevolveState {
        index: &index,
        ..state(&sketch, &profiles, &picked, RevolvePick::Axis, None)
    };
    let revolving = Revolving::new(revolve.clone());
    let edges = revolving.axis_edges();
    // The block's four top edges, nothing of its sides or bottom.
    assert_eq!(edges.len(), 4, "{edges:?}");
    assert!(
        edges
            .iter()
            .all(|(_, ends)| ends.iter().all(|p| p.z == 0.0))
    );
    let right = edges
        .iter()
        .find(|(_, [a, b])| a.x == -3.0 && b.x == -3.0)
        .unwrap();

    let viewport = shown(revolve.clone());
    let picked_edge = |at: Point| match click(&viewport, at)[..] {
        [Message::Look(Look::Revolve(RevolveLook::PickEdge { model, edge, at }))] => {
            assert_eq!(model, 5);
            Some((edge, at))
        }
        _ => None,
    };
    let (edge, at) = picked_edge(Point::new(70.0, 70.0))
        .unwrap_or_else(|| panic!("{:?}", click(&viewport, Point::new(70.0, 70.0))));
    assert_eq!(edge, right.0);
    assert!((at - DVec3::new(-3.0, 3.0, 0.0)).length() < 0.2, "{at}");
    // The sketch's left line, 10 pixels off, still goes first when it's
    // the nearer.
    assert_eq!(
        axis_picked(&viewport, Point::new(118.0, 70.0)),
        Some(AxisLine::Curve(left))
    );
    // Hovered, it's drawn stronger.
    let mut input = Input::default();
    let at = Point::new(70.0, 70.0);
    let moved = mouse::Event::CursorMoved { position: at };
    let cursor = mouse::Cursor::Available(at);
    let _ = revolving.mouse(&mut input, moved, bounds(), cursor, &top_camera());
    assert_eq!(input.edge.map(|(edge, _)| edge), Some(right.0));
    assert_eq!(
        revolving.mouse_interaction(&input, bounds(), cursor),
        Some(mouse::Interaction::Pointer)
    );

    // Picking regions, a click on the edge picks nothing.
    let regions = shown(RevolveState {
        picking: RevolvePick::Regions,
        ..revolve.clone()
    });
    assert!(click(&regions, Point::new(70.0, 70.0)).is_empty());

    // Picked, the arrow runs between its ends on the source.
    let edge_ref = varde_document::EdgeRef {
        body: index.face_body(0).unwrap(),
        faces: index.chain_keys(right.0).unwrap(),
        near: DVec3::new(-3.0, 0.0, 0.0),
    };
    let about = RevolveState {
        axis: Some(AxisLine::Edge(edge_ref)),
        edge_ends: Some(right.1),
        picking: RevolvePick::Regions,
        ..revolve
    };
    let pointed = Revolving::new(about.clone())
        .pointed()
        .map(|(_, ends)| ends);
    assert_eq!(pointed, Some(right.1.map(|p| p.truncate())));
    let flipped = RevolveState {
        extent: TurnKind::OneSide,
        flip: true,
        ..about
    };
    let pointed = Revolving::new(flipped).pointed().map(|(_, ends)| ends);
    assert_eq!(
        pointed,
        Some([right.1[1], right.1[0]].map(|p| p.truncate()))
    );
}

/// The model shown directs a straight edge as regenerating does, with
/// the face of its reference's first key on its left seen from outside:
/// on every straight edge of the example plate, that face's outward
/// normal (its triangles run round anticlockwise from outside) crossed
/// with the way the ends run points into it.
#[test]
fn the_model_shown_directs_edges_as_regenerating_does() {
    let document = varde_document::Document::example();
    let mut cache = varde_regen::Cache::default();
    let evaluation = varde_regen::evaluate(&document, &mut cache);
    let (mesh, picking) =
        varde_regen::tessellate_picking(&document, &evaluation, &mut cache).unwrap();
    let index = PickIndex::new(mesh, picking, 1);
    let mesh = index.mesh();
    let at = |i: u32| DVec3::from(mesh.positions()[i as usize].map(f64::from));
    let faces: Vec<&[u32]> = mesh.faces().collect();
    let mut checked = 0;
    for edge in 0..mesh.edge_count() as u32 {
        let Some(keys) = index.chain_keys(edge) else {
            continue;
        };
        let Some([from, to]) = index.edge_ends(edge, &keys) else {
            continue;
        };
        let [a, b] = index.edge_faces(edge).unwrap();
        let left = if index.face_ref(a, DVec3::ZERO).unwrap().key == keys[0] {
            a
        } else {
            b
        };
        let tris = faces[left as usize];
        let [p, q, r] = [0, 1, 2].map(|k| at(tris[k]));
        let outward = (q - p).cross(r - p);
        let inside = tris.iter().map(|&i| at(i)).sum::<DVec3>() / tris.len() as f64;
        let into = outward.cross(to - from);
        assert!(into.dot(inside - (from + to) / 2.0) > 0.0, "edge {edge}");
        checked += 1;
    }
    // The plate's twelve box edges.
    assert_eq!(checked, 12);
}

/// A model edge is in the source's plane as regenerating tells it:
/// within the resolution of the document's tolerance, not the rounding
/// of the mesh's `f32` points. The block's top edges are in XY, and in
/// planes parallel to it nearer than the resolution, not in ones
/// further off; a coarser tolerance takes planes further off.
#[test]
fn a_model_edge_is_in_the_plane_within_the_resolution() {
    use crate::revolve::{EDGE_OFF_PLANE, axis_edge};
    let index = block();
    let fine = varde_kernel::Tolerance::DEFAULT.resolution();
    let coarse = varde_kernel::Tolerance::new(0.1).unwrap().resolution();
    let top: Vec<u32> = (0..index.mesh().edge_count() as u32)
        .filter(|&edge| axis_edge(&index, edge, &OriginPlane::XY.placement(), fine).is_ok())
        .collect();
    assert_eq!(top.len(), 4);
    let at = |height: f64| varde_document::Placement {
        origin: DVec3::new(0.0, 0.0, height),
        ..OriginPlane::XY.placement()
    };
    for &edge in &top {
        for (height, resolution, taken) in [
            (0.5 * fine, fine, true),
            (-0.5 * fine, fine, true),
            (3.0 * fine, fine, false),
            (-3.0 * fine, fine, false),
            (0.5 * coarse, coarse, true),
            (3.0 * coarse, coarse, false),
        ] {
            let found = axis_edge(&index, edge, &at(height), resolution);
            match taken {
                true => assert!(found.is_ok(), "{height}: {found:?}"),
                false => assert_eq!(found, Err(EDGE_OFF_PLANE), "{height}"),
            }
        }
    }
}

/// One side of 90° about the lathe's left side, from (2, 4) to (2, -4):
/// its knob at the rectangle's centre, (4, 0), turned a quarter about -y,
/// up to (2, 0, 2).
fn turned<'a>(
    sketch: &'a Sketch,
    profiles: &'a Arc<Profiles>,
    picked: &'a BTreeSet<usize>,
    axis: Id,
) -> RevolveState<'a> {
    let mut state = state(
        sketch,
        profiles,
        picked,
        RevolvePick::Regions,
        Some(AxisLine::Curve(axis)),
    );
    state.extent = TurnKind::OneSide;
    state
}

/// The viewport setting up `state`, seen by `camera`.
fn seen<'a>(state: RevolveState<'a>, camera: &Camera) -> Program<'a> {
    program(
        &Arc::default(),
        &Arc::default(),
        camera,
        None,
        Mode::Light.palette(),
        None,
        Some(Operating::Revolve(Revolving::new(state))),
    )
}

/// The messages `viewport` sends for `events` with the cursor at `at`,
/// and whether it captured the last.
fn feed(
    viewport: &Program<'_>,
    input: &mut Interaction,
    at: Point,
    events: &[mouse::Event],
) -> (Vec<Message>, bool) {
    let mut messages = Vec::new();
    let mut captured = false;
    for event in events {
        let cursor = mouse::Cursor::Available(at);
        let action = viewport.update(input, &Event::Mouse(*event), bounds(), cursor);
        captured = false;
        if let Some(action) = action {
            let (message, _, status) = action.into_inner();
            messages.extend(message);
            captured = status == iced::event::Status::Captured;
        }
    }
    (messages, captured)
}

#[test]
fn a_knob_is_hovered_and_grabbed_ahead_of_the_regions() {
    let (sketch, left, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let viewport = shown(turned(&sketch, &profiles, &picked, left));
    let mut input = Interaction::default();
    // Seen from the top, the knob at (2, 0, 2), its arrow on towards -x.
    for at in [Point::new(120.0, 100.0), Point::new(108.0, 100.0)] {
        let moved = mouse::Event::CursorMoved { position: at };
        let (messages, captured) = feed(&viewport, &mut input, at, &[moved]);
        assert!(captured && messages.is_empty(), "{at:?} {messages:?}");
        let cursor = mouse::Cursor::Available(at);
        assert_eq!(
            viewport.mouse_interaction(&input, bounds(), cursor),
            mouse::Interaction::Grab
        );
        let press = mouse::Event::ButtonPressed(mouse::Button::Left);
        let (messages, _) = feed(&viewport, &mut input, at, &[press]);
        assert!(
            matches!(
                messages[..],
                [Message::Look(Look::Revolve(RevolveLook::GrabHandle(
                    Angle::First
                )))]
            ),
            "{messages:?}"
        );
    }

    // A full turn has no knob.
    let mut state = turned(&sketch, &profiles, &picked, left);
    state.extent = TurnKind::Full;
    let viewport = shown(state);
    let at = Point::new(120.0, 100.0);
    let moved = mouse::Event::CursorMoved { position: at };
    let (_, captured) = feed(&viewport, &mut Interaction::default(), at, &[moved]);
    assert!(!captured);
}

#[test]
fn a_grabbed_knob_turns_where_the_cursor_is_round_the_axis_snapped() {
    let (sketch, left, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let mut camera = top_camera();
    camera.look_from(varde_render::View::Front);
    let mut state = turned(&sketch, &profiles, &picked, left);
    state.grabbed = Some(Angle::First);
    let handle = Revolving::new(state.clone()).handle.expect("a handle");
    let viewport = seen(state, &camera);
    let projector = Projector::new(&camera, OriginPlane::XY.placement(), SIZE, SIZE).unwrap();
    // 2 mm out, 20 pixels: snapped to 30°.
    for (towards, snapped) in [(55.0f64, 60.0f64), (-80.0, -90.0), (200.0, 210.0)] {
        let at = projector.show(handle.at(towards.to_radians()));
        let at = Point::new(at.x as f32, at.y as f32);
        let moved = mouse::Event::CursorMoved { position: at };
        let (messages, captured) = feed(&viewport, &mut Interaction::default(), at, &[moved]);
        assert!(captured);
        let [Message::Look(Look::Revolve(RevolveLook::DragHandle { angle, to }))] = messages[..]
        else {
            panic!("{messages:?}");
        };
        assert_eq!(angle, Angle::First);
        assert!(
            (to.to_degrees() - snapped).abs() < 1e-9,
            "{towards}: {}",
            to.to_degrees()
        );
    }
    let release = mouse::Event::ButtonReleased(mouse::Button::Left);
    let at = Point::new(100.0, 100.0);
    let (messages, _) = feed(&viewport, &mut Interaction::default(), at, &[release]);
    assert!(matches!(
        messages[..],
        [Message::Look(Look::Revolve(RevolveLook::DropHandle))]
    ));
}

#[test]
fn a_knob_is_drawn_lighter_with_its_rail_while_grabbed() {
    let (sketch, left, profiles) = lathe();
    let picked = BTreeSet::from([0]);
    let input = Interaction::default();
    let live = |state: RevolveState<'_>| {
        let viewport = shown(state);
        let frame = viewport.draw(&input, mouse::Cursor::Unavailable, bounds());
        frame.sketch.clone().expect("drawn").live
    };
    let idle = live(turned(&sketch, &profiles, &picked, left));
    let mut grabbed = turned(&sketch, &profiles, &picked, left);
    grabbed.grabbed = Some(Angle::First);
    assert_ne!(live(grabbed), idle);
}
