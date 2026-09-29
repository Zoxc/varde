use std::sync::Arc;

use iced::widget::shader::Pipeline as _;
use varde_document::Document;
use varde_render::Camera;

use super::*;

/// The mesh of `document`.
fn shown(document: &Document) -> Arc<RenderMesh> {
    Arc::new(varde_regen::tessellate(document).unwrap())
}

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
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
    let primitive = primitive(state, mesh);
    prepare(device, queue, pipeline, &primitive);
    draw(device, queue, pipeline, &primitive)
}

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, iced::Size::new(SIZE as f32, SIZE as f32))
}

/// What the widget with `state` draws for `mesh`.
fn primitive(state: &Interaction, mesh: &Arc<RenderMesh>) -> Primitive {
    use iced::widget::shader::Program as _;

    program(
        mesh,
        &Camera::default(),
        crate::theme::Mode::Light.palette(),
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
    let cube = shown(&Document::example());
    let mut shared = Pipeline::new(&device, &queue, FORMAT);
    let widget = Interaction::default();
    let first = render(&device, &queue, &mut shared, &widget, &cube);

    // An empty document's mesh, and no mesh yet.
    for next in [shown(&Document::default()), Arc::default()] {
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
    let cube = shown(&Document::example());
    let empty = shown(&Document::default());
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
    let mesh = shown(&Document::example());
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
