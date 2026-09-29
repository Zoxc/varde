//! Checks that a mesh past the device's buffer limit is skipped and
//! reported rather than a panic in wgpu.

use std::sync::Arc;

use glam::Vec3;
use varde_kernel::{RenderMesh, Shape};
use varde_render::{Camera, Colors, Frame, PrepareError, Renderer, Srgb, Viewport, wgpu};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// A black background and a grey model, with the app's scene colours
/// otherwise.
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
};

/// A device whose buffers hold at most `max_buffer_size` bytes.
fn device(max_buffer_size: u64) -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let descriptor = wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_buffer_size,
            ..wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
        },
        ..Default::default()
    };
    pollster::block_on(adapter.request_device(&descriptor)).ok()
}

/// Two cubes' positions on a device taking 512-byte buffers.
const TOO_LARGE: PrepareError = PrepareError::MeshTooLarge {
    bytes: 576,
    limit: 512,
};

#[test]
fn mesh_past_the_buffer_limit_is_skipped() {
    // Two cubes' 48 positions take 576 bytes.
    let Some((device, queue)) = device(512) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert_eq!(device.limits().max_buffer_size, 512);
    let cube = Shape::cuboid(Vec3::ONE).build().unwrap().tessellate();
    let mut mesh = cube.clone();
    mesh.append_at(&cube, Vec3::X * 2.0).unwrap();
    let mesh = Arc::new(mesh);

    let renderer = Renderer::new(&device, FORMAT);
    let mut slot = renderer.slot(&device);
    let frame = Frame {
        camera: &Camera::default(),
        mesh: &mesh,
        viewport: Viewport {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        },
        target_size: [64, 64],
        scale_factor: 1.0,
        colors: COLORS,
    };
    assert_eq!(
        renderer.prepare(&mut slot, &device, &queue, &frame),
        Err(TOO_LARGE)
    );
    // Reported once, not on every frame.
    assert_eq!(renderer.prepare(&mut slot, &device, &queue, &frame), Ok(()));
    // Another mesh is tried, even with the same contents.
    let copy = Arc::new(RenderMesh::clone(&mesh));
    assert_eq!(
        renderer.prepare(
            &mut slot,
            &device,
            &queue,
            &Frame {
                mesh: &copy,
                ..frame
            }
        ),
        Err(TOO_LARGE)
    );
}
