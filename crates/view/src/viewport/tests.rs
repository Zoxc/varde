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
