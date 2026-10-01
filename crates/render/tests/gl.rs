//! The renderer on wgpu's GL backend, which the browser build runs on
//! (WebGL2): its own binary, so its instance never meets the Vulkan one
//! of the other tests. Skipped where there's no GL adapter.

use std::sync::Arc;

use glam::{DVec2, DVec3, Vec3};
use varde_kernel::{RenderLines, Solid, Tolerance};
use varde_render::{
    Camera, ClipRect, Colors, Frame, GridPlane, Renderer, SketchLayer, SketchScene, Space, Srgb,
    Srgba, View, Viewport, wgpu,
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [128, 128];

/// A black background and a grey model.
const COLORS: Colors = Colors {
    background_top: Srgb([0.0; 3]),
    background_bottom: Srgb([0.0; 3]),
    model: Srgb([0.5; 3]),
    edge: Srgb([0.12, 0.13, 0.15]),
    grid: Srgb([0.45, 0.49, 0.54]),
    axes: [
        Srgb([0.85, 0.25, 0.22]),
        Srgb([0.30, 0.65, 0.25]),
        Srgb([0.20, 0.45, 0.85]),
    ],
    origin_outline: Srgb([0.2, 0.22, 0.25]),
    sketch: Srgb([1.0, 1.0, 0.0]),
    faded_alpha: 0.3,
};

/// A device on the GL backend, if there's an adapter for it.
fn gl_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::GL,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// Draws `frame` on `renderer` into a target of [`SIZE`] and returns its
/// pixels, row-major.
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &Renderer,
    frame: &Frame<'_>,
) -> Vec<[u8; 4]> {
    let [width, height] = SIZE;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width,
            height,
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
    let mut slot = renderer.slot(device);
    renderer.prepare(&mut slot, device, queue, frame).unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    let clip = ClipRect {
        x: 0,
        y: 0,
        width,
        height,
    };
    renderer.render(&slot, &mut encoder, &view, clip);
    let bytes_per_row = width * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(bytes_per_row * height),
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
    let data = buffer.slice(..).get_mapped_range();
    data.as_chunks::<4>().0.to_vec()
}

#[test]
fn a_depth_tested_sketch_is_hidden_by_the_model_on_gl() {
    // From the top, a cube from (-2, -2, 0) to (2, 2, 4) over the whole
    // middle of the view, and a green square on a plane through its
    // middle: hidden depth tested, shown on top. GL caches programs by
    // shader module and entry point, not by pipeline constants, so the
    // depth tested pipelines need a module of their own.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let solid = Solid::cuboid(
        DVec3::new(-2.0, -2.0, 0.0),
        DVec3::splat(4.0),
        0,
        &Tolerance::DEFAULT,
    );
    let mesh = solid
        .unwrap()
        .tessellate(&varde_kernel::Display::default())
        .unwrap();
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    camera.zoom(12.0 / camera.view_height());
    let plane = GridPlane::new(Vec3::new(0.0, 0.0, 2.0), Vec3::X, Vec3::Y).unwrap();
    let mut live = SketchLayer::default();
    let square =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| DVec2::new(x, y));
    live.fill(Space::On(plane), [&square[..]], Srgba([0.0, 1.0, 0.0, 1.0]));
    let renderer = Renderer::new(&device, FORMAT);
    let (mesh, sketches, base) = (
        Arc::new(mesh),
        Arc::new(RenderLines::default()),
        Arc::new(SketchLayer::default()),
    );
    let [width, height] = SIZE.map(|s| s as f32);
    for depth_tested in [false, true] {
        let frame = Frame {
            camera: &camera,
            mesh: &mesh,
            sketches: &sketches,
            grid: GridPlane::XY,
            faded: false,
            sketch: Some(SketchScene {
                plane: GridPlane::XY,
                depth_tested,
                base: &base,
                live: &live,
            }),
            viewport: Viewport {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            target_size: SIZE,
            scale_factor: 1.0,
            colors: COLORS,
        };
        let pixels = draw(&device, &queue, &renderer, &frame);
        let [r, g, b, _] = pixels[(SIZE[1] / 2 * SIZE[0] + SIZE[0] / 2) as usize];
        let green = g > 150 && r < 100 && b < 100;
        assert_eq!(
            green, !depth_tested,
            "depth tested {depth_tested}: {r} {g} {b}"
        );
    }
}
