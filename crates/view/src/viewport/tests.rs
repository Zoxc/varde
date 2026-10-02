use std::sync::Arc;

use iced::widget::shader::Pipeline as _;
use varde_render::Camera;

use super::*;

/// A box's mesh, as a document with one would show.
fn cube() -> Arc<RenderMesh> {
    let tol = varde_kernel::Tolerance::DEFAULT;
    let solid = varde_kernel::Solid::cuboid(glam::DVec3::ZERO, glam::DVec3::splat(2.0), 0, &tol);
    let display = varde_kernel::Display::new(&tol);
    Arc::new(solid.unwrap().tessellate(&display).unwrap())
}

/// The one device the tests share, if there's an adapter. The Vulkan
/// loader isn't thread safe across instances: a test creating its own
/// while another names an object on its device crashed it
/// (`loader_get_icd_and_device`, SIGSEGV about one run in three), so the
/// instance and device are made once for the whole binary.
fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    static DEVICE: std::sync::OnceLock<Option<(wgpu::Device, wgpu::Queue)>> =
        std::sync::OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let instance = wgpu::Instance::default();
            let options = wgpu::RequestAdapterOptions::default();
            let adapter = pollster::block_on(instance.request_adapter(&options)).ok()?;
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
        })
        .clone()
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: u32 = 64;

/// Draws `mesh` through `pipeline` in the widget with `state` the way iced
/// does, and returns the RGBA8 pixels.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut Pipeline,
    state: &Interaction,
    mesh: &Arc<RenderMesh>,
) -> Vec<[u8; 4]> {
    render_with(device, queue, pipeline, state, mesh, &Arc::default())
}

/// Like [`render`], with `sketches`.
fn render_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut Pipeline,
    state: &Interaction,
    mesh: &Arc<RenderMesh>,
    sketches: &Arc<RenderLines>,
) -> Vec<[u8; 4]> {
    let primitive = primitive_with(state, mesh, sketches);
    prepare(device, queue, pipeline, &primitive);
    draw(device, queue, pipeline, &primitive)
}

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, iced::Size::new(SIZE as f32, SIZE as f32))
}

/// What the widget with `state` draws for `mesh`.
fn primitive(state: &Interaction, mesh: &Arc<RenderMesh>) -> Primitive {
    primitive_with(state, mesh, &Arc::default())
}

/// What the widget with `state` draws for `mesh` and `sketches`.
fn primitive_with(
    state: &Interaction,
    mesh: &Arc<RenderMesh>,
    sketches: &Arc<RenderLines>,
) -> Primitive {
    use iced::widget::shader::Program as _;

    program(
        mesh,
        sketches,
        &Camera::default(),
        None,
        crate::theme::Mode::Light.palette(),
        None,
        None,
    )
    .draw(state, mouse::Cursor::Unavailable, bounds())
}

fn prepare(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut Pipeline,
    primitive: &Primitive,
) {
    use iced::widget::shader::Primitive as _;

    primitive.prepare(
        pipeline,
        device,
        queue,
        &bounds(),
        &shader::Viewport::with_physical_size(iced::Size::new(SIZE, SIZE), 1.0),
    );
}

/// Renders a prepared `primitive` and returns the RGBA8 pixels.
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &Pipeline,
    primitive: &Primitive,
) -> Vec<[u8; 4]> {
    use iced::widget::shader::Primitive as _;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

    let mut encoder = device.create_command_encoder(&Default::default());
    primitive.render(
        pipeline,
        &mut encoder,
        &view,
        &Rectangle {
            x: 0,
            y: 0,
            width: SIZE,
            height: SIZE,
        },
    );

    let bytes_per_row = SIZE * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(bytes_per_row * SIZE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer
        .slice(..)
        .get_mapped_range()
        .as_chunks::<4>()
        .0
        .to_vec()
}

/// iced keeps one `Pipeline` for every viewport and document, so closing a
/// document and opening another mustn't show the mesh left over from the
/// first.
#[test]
fn next_document_does_not_show_the_previous_mesh() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let cube = cube();
    let mut shared = Pipeline::new(&device, &queue, FORMAT);
    let widget = Interaction::default();
    let first = render(&device, &queue, &mut shared, &widget, &cube);

    // An empty document's mesh, and no mesh yet.
    for next in [Arc::new(RenderMesh::default()), Arc::default()] {
        let expected = render(
            &device,
            &queue,
            &mut Pipeline::new(&device, &queue, FORMAT),
            &Interaction::default(),
            &next,
        );
        assert!(first != expected, "the cube should show");
        render(&device, &queue, &mut shared, &widget, &cube);
        let shown = render(&device, &queue, &mut shared, &widget, &next);
        assert!(shown == expected, "the previous document's mesh shows");
    }
}

/// iced prepares every viewport of a frame before rendering any, through
/// the one `Pipeline`, so each must keep what it prepared.
#[test]
fn viewports_in_one_frame_show_their_own_mesh() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let cube = cube();
    let empty = Arc::new(RenderMesh::default());
    let alone = |mesh: &Arc<RenderMesh>| {
        let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
        render(
            &device,
            &queue,
            &mut pipeline,
            &Interaction::default(),
            mesh,
        )
    };

    let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
    let (first, second) = (Interaction::default(), Interaction::default());
    let (a, b) = (primitive(&first, &cube), primitive(&second, &empty));
    prepare(&device, &queue, &mut pipeline, &a);
    prepare(&device, &queue, &mut pipeline, &b);
    assert!(draw(&device, &queue, &pipeline, &a) == alone(&cube));
    assert!(draw(&device, &queue, &pipeline, &b) == alone(&empty));
}

/// The cube's hidden edges show dashed, by default, until turned off.
#[test]
fn hidden_edges_show_unless_turned_off() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let cube = cube();
    let state = Interaction::default();
    assert!(primitive(&state, &cube).scene.hidden_edges);
    let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
    let mut drawn = |hidden_edges| {
        let mut primitive = primitive(&state, &cube);
        primitive.scene.hidden_edges = hidden_edges;
        prepare(&device, &queue, &mut pipeline, &primitive);
        draw(&device, &queue, &pipeline, &primitive)
    };
    let (on, off) = (drawn(true), drawn(false));
    assert!(on != off, "the hidden edges don't show");
}

/// The cube's wires, its sides' diagonals, show only in a wireframe.
#[test]
fn wires_show_only_in_a_wireframe() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let cube = cube();
    let state = Interaction::default();
    assert!(!primitive(&state, &cube).scene.wireframe);
    let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
    let mut drawn = |wireframe| {
        let mut primitive = primitive(&state, &cube);
        primitive.scene.wireframe = wireframe;
        prepare(&device, &queue, &mut pipeline, &primitive);
        draw(&device, &queue, &pipeline, &primitive)
    };
    let (on, off) = (drawn(true), drawn(false));
    assert!(on != off, "the wires don't show");
}

/// A viewport's slot goes once its widget and what it drew are gone.
#[test]
fn trim_drops_the_slots_of_gone_viewports() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mesh = cube();
    let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
    let kept = Interaction::default();
    render(&device, &queue, &mut pipeline, &kept, &mesh);
    render(
        &device,
        &queue,
        &mut pipeline,
        &Interaction::default(),
        &mesh,
    );
    assert_eq!(pipeline.slots.len(), 2);

    pipeline.trim();
    assert_eq!(pipeline.slots.len(), 1);
    render(&device, &queue, &mut pipeline, &kept, &mesh);
    assert_eq!(pipeline.slots.len(), 1);
}

/// Lines across the middle of the viewport, seen from the default camera.
fn sketches() -> Arc<RenderLines> {
    let mut lines = RenderLines::default();
    lines
        .push([
            glam::Vec3::new(-2.0, -2.0, 0.0),
            glam::Vec3::new(2.0, 2.0, 0.0),
        ])
        .unwrap();
    lines
        .push([
            glam::Vec3::new(-2.0, 2.0, 0.0),
            glam::Vec3::new(2.0, -2.0, 0.0),
        ])
        .unwrap();
    Arc::new(lines)
}

/// Finished sketches are drawn with the model, and like its mesh, a
/// viewport shown the next document's shows none of the previous one's.
#[test]
fn sketches_show_and_go_with_their_document() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let empty = Arc::new(RenderMesh::default());
    let alone = |sketches: &Arc<RenderLines>| {
        let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
        let widget = Interaction::default();
        render_with(&device, &queue, &mut pipeline, &widget, &empty, sketches)
    };
    let without = alone(&Arc::default());
    let with = alone(&sketches());
    // Drawn in the light theme's sketch colour, far bluer than red.
    let sketched = with
        .iter()
        .zip(&without)
        .filter(|&(&[r, _, b, _], other)| {
            i32::from(b) - i32::from(r) > 80 && [r, b] != [other[0], other[2]]
        })
        .count();
    assert!(
        sketched > SIZE as usize,
        "the sketches should show: {sketched}"
    );

    let mut shared = Pipeline::new(&device, &queue, FORMAT);
    let widget = Interaction::default();
    render_with(&device, &queue, &mut shared, &widget, &empty, &sketches());
    let next = render_with(
        &device,
        &queue,
        &mut shared,
        &widget,
        &empty,
        &Arc::default(),
    );
    assert!(next == without, "the previous document's sketches show");
}

#[test]
fn drags_orbit_and_pan_by_button_and_mode() {
    use keyboard::Modifiers;
    use mouse::Button;

    let none = Modifiers::empty();
    let shift = Modifiers::SHIFT;
    for sketching in [false, true] {
        let drag = |button, modifiers| DragKind::for_button(button, modifiers, sketching);
        assert_eq!(drag(Button::Middle, none), Some(DragKind::Orbit));
        assert_eq!(drag(Button::Middle, shift), Some(DragKind::Orbit));
        assert_eq!(drag(Button::Right, shift), Some(DragKind::Orbit));
        assert_eq!(drag(Button::Right, none), Some(DragKind::Pan));
        assert_eq!(drag(Button::Right, Modifiers::CTRL), Some(DragKind::Pan));
        assert_eq!(drag(Button::Back, none), None);
    }
    // The left button orbits outside a sketch, and is for geometry inside.
    assert_eq!(
        DragKind::for_button(Button::Left, none, false),
        Some(DragKind::Orbit)
    );
    assert_eq!(DragKind::for_button(Button::Left, none, true), None);
    assert_eq!(DragKind::for_button(Button::Left, shift, true), None);
}

/// What the widget with `state` does with `event` over it, in a sketch if
/// `sketching`.
fn handle(state: &mut Interaction, event: Event, sketching: bool) -> Option<Action<Message>> {
    use iced::widget::shader::Program as _;

    let (sketch, selection) = (varde_sketch::Sketch::default(), Default::default());
    let state_of_sketch = crate::SketchState::plain(&sketch, &selection, None);
    let program = program(
        &Arc::default(),
        &Arc::default(),
        &Camera::default(),
        None,
        crate::theme::Mode::Light.palette(),
        sketching.then(|| Sketching::new(state_of_sketch, true)),
        None,
    );
    let cursor = mouse::Cursor::Available(Point::new(10.0, 10.0));
    program.update(state, &event, bounds(), cursor)
}

#[test]
fn shift_held_turns_a_right_drag_into_an_orbit() {
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
    let shift = Event::Keyboard(keyboard::Event::ModifiersChanged(
        keyboard::Modifiers::SHIFT,
    ));
    let mut state = Interaction::default();
    assert!(handle(&mut state, press.clone(), true).is_some());
    assert!(matches!(state.drag, Some((DragKind::Pan, _))));
    let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right));
    handle(&mut state, release, true);

    assert!(handle(&mut state, shift, true).is_none());
    handle(&mut state, press, true);
    assert!(matches!(state.drag, Some((DragKind::Orbit, _))));
}

#[test]
fn a_left_drag_in_a_sketch_moves_no_camera() {
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let mut state = Interaction::default();
    // The sketch's instead.
    handle(&mut state, press.clone(), true);
    assert!(state.drag.is_none());
    handle(&mut state, press, false);
    assert!(matches!(state.drag, Some((DragKind::Orbit, _))));
}

#[test]
fn the_wheel_zooms_towards_the_cursor() {
    let wheel = Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
    });
    let mut state = Interaction::default();
    let message = handle(&mut state, wheel, false).unwrap().into_inner().0;
    // The cursor is at (10, 10), up and left of the middle.
    let off = (10.0 - SIZE as f32 / 2.0) / SIZE as f32;
    assert!(
        matches!(
            message,
            Some(Message::Look(Look::Zoom { factor, x, y }))
                if factor == ZOOM_PER_LINE && x == off && y == off
        ),
        "{message:?}"
    );
}

/// The sketch being edited is drawn by the scene, through the widget's
/// primitive, and its base layer is uploaded again only when it changes.
#[test]
fn the_sketch_being_edited_is_drawn_with_the_scene() {
    use iced::widget::shader::Program as _;
    use varde_sketch::{Curve, Sketch};

    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mut sketch = Sketch::default();
    let a = sketch.add_point(glam::DVec2::new(-3.0, -2.0)).unwrap();
    let b = sketch.add_point(glam::DVec2::new(3.0, 2.0)).unwrap();
    sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let empty = Sketch::default();
    let selection = Default::default();
    let state = Interaction::default();
    let primitive = |sketch| {
        let sketching = Sketching::new(crate::SketchState::plain(sketch, &selection, None), true);
        program(
            &Arc::default(),
            &Arc::default(),
            &Camera::default(),
            None,
            crate::theme::Mode::Light.palette(),
            Some(sketching),
            None,
        )
        .draw(&state, mouse::Cursor::Unavailable, bounds())
    };
    let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
    let mut shown = |primitive: &Primitive| {
        prepare(&device, &queue, &mut pipeline, primitive);
        draw(&device, &queue, &pipeline, primitive)
    };
    let base = |primitive: &Primitive| primitive.sketch.as_ref().unwrap().base.clone();
    let (with, again) = (primitive(&sketch), primitive(&sketch));
    assert!(Arc::ptr_eq(&base(&with), &base(&again)));
    let without = primitive(&empty);
    assert!(shown(&with) != shown(&without), "the sketch should show");
}

/// A plate's regions are shaded, each once, and the plate's hole is left
/// out of it: hovering the plate highlights it around its hole, and
/// hovering the hole highlights only that.
#[test]
fn a_region_with_a_hole_is_shaded_around_it() {
    use iced::widget::shader::Program as _;

    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let sketch = crate::testing::plate(5.2, 5.2, 3.4);
    let profiles = sketch.profiles().map(Arc::new);
    assert_eq!(profiles.as_ref().unwrap().regions.len(), 2);

    // 12 units across 64 pixels, so the spots below are farther than
    // the hit tolerance from any point, curve or axis.
    let mut camera = crate::projection::top_camera();
    camera.zoom(0.6);
    let placement = varde_document::OriginPlane::XY.placement();
    let projector =
        crate::projection::Projector::new(&camera, placement, SIZE as f32, SIZE as f32).unwrap();
    let selection = Default::default();
    let mut pipeline = Pipeline::new(&device, &queue, FORMAT);
    // Renders the sketch, shaded or not, with the cursor at the sketch
    // point `hover` if there is one, and returns the pixels.
    let mut shown = |shaded: bool, hover: Option<(f64, f64)>| {
        let state = crate::SketchState {
            profiles: shaded.then_some(&profiles),
            ..crate::SketchState::plain(&sketch, &selection, None)
        };
        let program = program(
            &Arc::default(),
            &Arc::default(),
            &camera,
            None,
            crate::theme::Mode::Light.palette(),
            Some(Sketching::new(state, true)),
            None,
        );
        let mut interaction = Interaction::default();
        if let Some((x, y)) = hover {
            let at = projector.project(glam::DVec2::new(x, y)).unwrap();
            let at = Point::new(at.x as f32, at.y as f32);
            let moved = Event::Mouse(mouse::Event::CursorMoved { position: at });
            program.update(
                &mut interaction,
                &moved,
                bounds(),
                mouse::Cursor::Available(at),
            );
        }
        let primitive = program.draw(&interaction, mouse::Cursor::Unavailable, bounds());
        prepare(&device, &queue, &mut pipeline, &primitive);
        draw(&device, &queue, &pipeline, &primitive)
    };
    // In the plate, in the hole, and outside: a pixel each, away from the
    // lines.
    let pixel = |pixels: &[[u8; 4]], x: f64, y: f64| {
        let at = projector.project(glam::DVec2::new(x, y)).unwrap();
        pixels[at.y as usize * SIZE as usize + at.x as usize]
    };
    let spots = [(3.9, 3.9), (1.4, 1.4), (5.75, 3.0)];
    let at = |pixels: &[[u8; 4]]| spots.map(|(x, y)| pixel(pixels, x, y));
    let [plain_plate, plain_hole, plain_outside] = at(&shown(false, None));
    let [plate, hole, outside] = at(&shown(true, None));
    assert_ne!(plate, plain_plate, "the plate is shaded");
    assert_ne!(hole, plain_hole, "the hole's inside is a region too");
    assert_eq!(outside, plain_outside, "outside isn't");
    let [hovered_plate, around, _] = at(&shown(true, Some(spots[0])));
    assert_ne!(hovered_plate, plate, "the plate is highlighted");
    assert_eq!(around, hole, "its hole isn't");
    let [beside, hovered_hole, _] = at(&shown(true, Some(spots[1])));
    assert_eq!(beside, plate, "the plate isn't");
    assert_ne!(hovered_hole, hole, "the hole is highlighted");
}

/// The messages the widget over [`cube`] seen from the top sends for
/// `events`, each with the cursor where it's sent.
fn middle_button(events: &[(Event, Point)]) -> Vec<Message> {
    use iced::widget::shader::Program as _;

    let mesh = cube();
    let program = program(
        &mesh,
        &Arc::default(),
        &crate::projection::top_camera(),
        None,
        crate::theme::Mode::Light.palette(),
        None,
        None,
    );
    let mut state = Interaction::default();
    let mut messages = Vec::new();
    for (event, at) in events {
        let action = program.update(&mut state, event, bounds(), mouse::Cursor::Available(*at));
        if let Some(action) = action {
            messages.extend(action.into_inner().0);
        }
    }
    messages
}

#[test]
fn a_middle_click_picks_the_point_to_orbit_and_a_drag_orbits() {
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle));
    let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Middle));
    let moved = |at| (Event::Mouse(mouse::Event::CursorMoved { position: at }), at);
    // A unit is 3.2 pixels, the origin in the middle: over the box's top.
    let on = Point::new(35.0, 29.0);
    let nudged = Point::new(36.0, 30.0);
    let messages = middle_button(&[
        (press.clone(), on),
        moved(nudged),
        (release.clone(), nudged),
    ]);
    let [Message::Look(Look::SetPivot(Some(at)))] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    assert!(
        at.abs_diff_eq(glam::Vec3::new(0.9375, 0.9375, 2.0), 1e-3),
        "{at}"
    );
    // Off the box, the grid.
    let off = Point::new(10.0, 10.0);
    let messages = middle_button(&[(press.clone(), off), (release.clone(), off)]);
    let [Message::Look(Look::SetPivot(Some(at)))] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    assert!(
        at.abs_diff_eq(glam::Vec3::new(-6.875, 6.875, 0.0), 1e-3),
        "{at}"
    );
    // A drag orbits, from where it was pressed, and picks nothing.
    let far = Point::new(45.0, 29.0);
    let messages = middle_button(&[(press, on), moved(nudged), moved(far), (release, far)]);
    let [Message::Look(Look::Orbit { yaw, pitch })] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    assert_eq!((*yaw, *pitch), (-10.0 * ORBIT_SPEED, 0.0));
}

/// The example's plate seen from the top in a viewport of the pick
/// tests' size, `hovered` held hovered: picking its model.
struct Plate {
    index: crate::pick::PickIndex,
    camera: Camera,
}

impl Plate {
    fn new() -> Self {
        use crate::pick::tests::{camera, plate};
        Plate {
            index: plate(),
            camera: camera(
                varde_render::View::Top,
                varde_render::Projection::Orthographic,
            ),
        }
    }

    fn bounds() -> Rectangle {
        let [width, height] = crate::pick::tests::SIZE;
        Rectangle::new(Point::ORIGIN, iced::Size::new(width, height))
    }

    /// Where the world point `at` shows.
    fn at(&self, at: glam::DVec3) -> Point {
        let p = crate::pick::tests::shown(&self.camera, at);
        Point::new(p.x as f32, p.y as f32)
    }

    /// The widget's program, picking with `hovered` held hovered.
    fn program(&self, camera: &Camera, hovered: Option<Picked>) -> Program<'_> {
        let mesh = self.index.mesh().clone();
        let mut program = program(
            &mesh,
            &Arc::default(),
            camera,
            None,
            crate::theme::Mode::Light.palette(),
            None,
            None,
        );
        program.picking = Some(ModelPicking {
            index: &self.index,
            hovered,
            hovered_snap: None,
            picks: Picks::All,
            snaps: false,
            planes: None,
        });
        program
    }

    /// The messages the widget with `state` sends for `events` with the
    /// cursor at `cursor`.
    fn send(
        &self,
        state: &mut Interaction,
        camera: &Camera,
        hovered: Option<Picked>,
        events: &[Event],
        cursor: Point,
    ) -> Vec<Message> {
        use iced::widget::shader::Program as _;
        let program = self.program(camera, hovered);
        let cursor = mouse::Cursor::Available(cursor);
        events
            .iter()
            .filter_map(|event| program.update(state, event, Self::bounds(), cursor))
            .filter_map(|action| action.into_inner().0)
            .collect()
    }
}

fn left(pressed: bool) -> Event {
    Event::Mouse(if pressed {
        mouse::Event::ButtonPressed(mouse::Button::Left)
    } else {
        mouse::Event::ButtonReleased(mouse::Button::Left)
    })
}

fn redraw() -> Event {
    Event::Window(iced::window::Event::RedrawRequested(
        iced::time::Instant::now(),
    ))
}

#[test]
fn a_left_click_on_the_model_selects_and_a_drag_still_orbits() {
    let plate = Plate::new();
    let at = plate.at(glam::DVec3::new(20.0, 5.0, 10.0));
    let mut state = Interaction::default();
    let camera = plate.camera;
    let sent = plate.send(&mut state, &camera, None, &[left(true), left(false)], at);
    let [
        Message::Look(Look::ClickModel {
            pick: Some(pick),
            add: false,
            double: false,
        }),
    ] = sent[..]
    else {
        panic!("{sent:?}");
    };
    assert!(matches!(pick.target, Picked::Face(_)));
    // Soon after, there again: a double-click.
    let sent = plate.send(&mut state, &camera, None, &[left(true), left(false)], at);
    assert!(
        matches!(
            sent[..],
            [Message::Look(Look::ClickModel { double: true, .. })]
        ),
        "{sent:?}"
    );
    // With Shift or Ctrl held, it adds or takes out.
    for modifiers in [keyboard::Modifiers::SHIFT, keyboard::Modifiers::CTRL] {
        let held = Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers));
        let mut state = Interaction::default();
        let sent = plate.send(
            &mut state,
            &camera,
            None,
            &[held, left(true), left(false)],
            at,
        );
        assert!(
            matches!(
                sent[..],
                [Message::Look(Look::ClickModel { add: true, .. })]
            ),
            "{sent:?}"
        );
    }
    // Off the model: a click on nothing.
    let off = plate.at(glam::DVec3::new(0.0, 26.0, 10.0));
    let sent = plate.send(&mut state, &camera, None, &[left(true), left(false)], off);
    assert!(
        matches!(
            sent[..],
            [Message::Look(Look::ClickModel { pick: None, .. })]
        ),
        "{sent:?}"
    );
    // Dragged past the slop, it orbits and selects nothing.
    let mut state = Interaction::default();
    let moved = Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(at.x + 20.0, at.y),
    });
    let sent = plate.send(
        &mut state,
        &camera,
        None,
        &[left(true), moved, left(false)],
        at,
    );
    assert!(
        matches!(sent[..], [Message::Look(Look::Orbit { .. })]),
        "{sent:?}"
    );
}

#[test]
fn the_hover_is_worked_out_again_as_the_camera_moves() {
    let plate = Plate::new();
    let at = plate.at(glam::DVec3::new(20.0, 5.0, 10.0));
    let mut state = Interaction::default();
    let camera = plate.camera;
    let sent = plate.send(&mut state, &camera, None, &[redraw()], at);
    let [Message::Look(Look::Hover(Some(pick)))] = sent[..] else {
        panic!("{sent:?}");
    };
    // Held hovered, a frame with nothing changed says nothing, nor does
    // one with the cursor elsewhere on the same face.
    let hovered = Some(pick.target);
    let sent = plate.send(&mut state, &camera, hovered, &[redraw(), redraw()], at);
    assert!(sent.is_empty(), "{sent:?}");
    // The camera panned away under the cursor: nothing there now.
    let mut panned = camera;
    panned.pan(0.0, 2.0);
    let sent = plate.send(&mut state, &panned, hovered, &[redraw()], at);
    assert!(
        matches!(sent[..], [Message::Look(Look::Hover(None))]),
        "{sent:?}"
    );
    let sent = plate.send(&mut state, &panned, None, &[redraw()], at);
    assert!(sent.is_empty(), "{sent:?}");
    // While the camera's dragged, nothing.
    let mut state = Interaction::default();
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
    let sent = plate.send(&mut state, &camera, None, &[press, redraw()], at);
    assert!(sent.is_empty(), "{sent:?}");
}

#[test]
fn an_orbit_lets_go_of_the_hover_until_it_ends() {
    let plate = Plate::new();
    let at = plate.at(glam::DVec3::new(20.0, 5.0, 10.0));
    let camera = plate.camera;
    let mut state = Interaction::default();
    let sent = plate.send(&mut state, &camera, None, &[redraw()], at);
    let [Message::Look(Look::Hover(Some(pick)))] = sent[..] else {
        panic!("{sent:?}");
    };
    let hovered = Some(pick.target);
    // Pressed, not yet a drag: a click may follow, the hover stays.
    let sent = plate.send(&mut state, &camera, hovered, &[left(true), redraw()], at);
    assert!(sent.is_empty(), "{sent:?}");
    // Moved past the slop: it orbits, and the next frame lets go of it.
    let moved = Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(at.x + 20.0, at.y),
    });
    let sent = plate.send(&mut state, &camera, hovered, &[moved, redraw()], at);
    assert!(
        matches!(
            sent[..],
            [
                Message::Look(Look::Orbit { .. }),
                Message::Look(Look::Hover(None))
            ]
        ),
        "{sent:?}"
    );
    let sent = plate.send(&mut state, &camera, None, &[redraw()], at);
    assert!(sent.is_empty(), "{sent:?}");
    // Let go of, the next frame picks again, though nothing else changed.
    let sent = plate.send(&mut state, &camera, None, &[left(false), redraw()], at);
    assert!(
        matches!(sent[..], [Message::Look(Look::Hover(Some(_)))]),
        "{sent:?}"
    );
}

#[test]
fn the_cursor_points_over_what_a_click_selects() {
    use iced::widget::shader::Program as _;
    let plate = Plate::new();
    let at = plate.at(glam::DVec3::new(20.0, 5.0, 10.0));
    let state = Interaction::default();
    let cursor = mouse::Cursor::Available(at);
    let hovered = Some(Picked::Face(0));
    let interaction = |hovered| {
        let program = plate.program(&plate.camera, hovered);
        program.mouse_interaction(&state, Plate::bounds(), cursor)
    };
    assert_eq!(interaction(hovered), mouse::Interaction::Pointer);
    assert_eq!(interaction(None), mouse::Interaction::default());
    let program = plate.program(&plate.camera, hovered);
    let outside = mouse::Cursor::Available(Point::new(-5.0, -5.0));
    assert_eq!(
        program.mouse_interaction(&state, Plate::bounds(), outside),
        mouse::Interaction::default()
    );
}

#[test]
fn measuring_the_cursor_takes_snap_points_and_says_when_they_change() {
    let plate = Plate::new();
    let camera = plate.camera;
    let corner = plate.at(glam::DVec3::new(30.0, 20.0, 10.0));
    let near = Point::new(corner.x - 6.0, corner.y + 6.0);
    let mut program = plate.program(&camera, None);
    let picking = program.picking.as_mut().unwrap();
    picking.snaps = true;
    let moved = Event::Mouse(mouse::Event::CursorMoved { position: near });
    let mut state = Interaction::default();
    let action = {
        use iced::widget::shader::Program as _;
        program.update(
            &mut state,
            &moved,
            Plate::bounds(),
            mouse::Cursor::Available(near),
        )
    };
    let sent = action.and_then(|action| action.into_inner().0);
    let Some(Message::Look(Look::Hover(Some(pick)))) = sent else {
        panic!("{sent:?}");
    };
    let snapped = pick.snap.expect("the corner is within reach");
    assert_eq!(
        plate.index.snap_point(snapped),
        Some(glam::DVec3::new(30.0, 20.0, 10.0))
    );
    // Held hovered with its snap, a move nearby says nothing; held
    // without it, it says.
    let mut program = plate.program(&camera, Some(pick.target));
    let picking = program.picking.as_mut().unwrap();
    picking.snaps = true;
    picking.hovered_snap = Some(snapped);
    let nudged = Point::new(near.x + 1.0, near.y);
    let moved = Event::Mouse(mouse::Event::CursorMoved { position: nudged });
    let action = {
        use iced::widget::shader::Program as _;
        program.update(
            &mut state,
            &moved,
            Plate::bounds(),
            mouse::Cursor::Available(nudged),
        )
    };
    assert!(action.is_none());
    program.picking.as_mut().unwrap().hovered_snap = None;
    let action = {
        use iced::widget::shader::Program as _;
        program.update(
            &mut state,
            &moved,
            Plate::bounds(),
            mouse::Cursor::Available(nudged),
        )
    };
    assert!(action.is_some());
}

#[test]
fn measuring_the_dots_of_what_s_hovered_stay_to_be_taken_off_it() {
    let plate = Plate::new();
    let camera = plate.camera;
    // The hole's rim on top, then its centre, over the hole: nothing
    // under the cursor but the rim's dot.
    let rim_at = plate.at(glam::DVec3::new(8.0, 0.0, 10.0));
    let rim = (plate.index)
        .pick(
            &camera,
            crate::pick::tests::SIZE,
            glam::DVec2::new(rim_at.x.into(), rim_at.y.into()),
            Picks::Edges,
        )
        .unwrap();
    let centre = plate.at(glam::DVec3::new(0.0, 0.0, 10.0));
    let near = Point::new(centre.x + 4.0, centre.y);
    let mut state = Interaction::default();
    let moved = Event::Mouse(mouse::Event::CursorMoved { position: near });
    for (held, snapped) in [(None, false), (Some(rim.target), true)] {
        let mut program = plate.program(&camera, held);
        program.picking.as_mut().unwrap().snaps = true;
        let action = {
            use iced::widget::shader::Program as _;
            program.update(
                &mut state,
                &moved,
                Plate::bounds(),
                mouse::Cursor::Available(near),
            )
        };
        let sent = action.and_then(|action| action.into_inner().0);
        match (sent, snapped) {
            (None, false) => {}
            (Some(Message::Look(Look::Hover(Some(pick)))), true) => {
                assert_eq!(pick.target, rim.target);
                assert!(matches!(
                    pick.snap,
                    Some(crate::pick::Snapped::EdgePoint(_))
                ));
            }
            (sent, _) => panic!("{sent:?}"),
        }
    }
}
