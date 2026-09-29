//! Checks that a mesh past the device's buffer limit is skipped and
//! reported rather than a panic in wgpu.
// Holding the shared wgpu device in a `static` asks whether it's `Sync`
// deeper than the default limit.
#![recursion_limit = "256"]

use std::sync::Arc;

use glam::{DVec3, Vec3};
use varde_kernel::{RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::{
    Camera, Colors, Frame, GridPlane, LineStyle, PrepareError, Renderer, SketchLayer, SketchScene,
    Space, Srgb, Srgba, Viewport, wgpu,
};

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
    sketch: Srgb([0.04, 0.58, 0.68]),
    faded_alpha: 0.3,
};

/// The device the tests share, whose buffers hold at most 512 bytes, if
/// there's an adapter. Made once for the binary: the Vulkan loader isn't
/// thread safe across instances made and dropped while another test uses
/// its device (a SIGSEGV in `loader_get_icd_and_device`).
fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    static DEVICE: std::sync::OnceLock<Option<(wgpu::Device, wgpu::Queue)>> =
        std::sync::OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let instance = wgpu::Instance::default();
            let options = wgpu::RequestAdapterOptions::default();
            let adapter = pollster::block_on(instance.request_adapter(&options)).ok()?;
            let descriptor = wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_buffer_size: 512,
                    ..wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
                },
                ..Default::default()
            };
            pollster::block_on(adapter.request_device(&descriptor)).ok()
        })
        .clone()
}

/// Two cubes' positions on a device taking 512-byte buffers.
const TOO_LARGE: PrepareError = PrepareError::MeshTooLarge {
    bytes: 576,
    limit: 512,
};

#[test]
fn mesh_past_the_buffer_limit_is_skipped() {
    // Two cubes' 48 positions take 576 bytes.
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert_eq!(device.limits().max_buffer_size, 512);
    let cube = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 0, &Tolerance::DEFAULT)
        .unwrap()
        .tessellate(&varde_kernel::Display::default())
        .unwrap();
    let mut mesh = cube.clone();
    mesh.append_at(&cube, Vec3::X * 2.0).unwrap();
    let mesh = Arc::new(mesh);

    let renderer = Renderer::new(&device, FORMAT);
    let mut slot = renderer.slot(&device);
    let frame = Frame {
        camera: &Camera::default(),
        mesh: &mesh,
        sketches: &Arc::default(),
        grid: GridPlane::XY,
        faded: false,
        sketch: None,
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

#[test]
fn lines_past_the_buffer_limit_are_skipped() {
    // 23 segments of 24 bytes take 552 bytes.
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mut lines = RenderLines::default();
    lines.push((0..24).map(|i| Vec3::X * i as f32)).unwrap();
    let lines = Arc::new(lines);
    let too_large = PrepareError::LinesTooLarge {
        bytes: 552,
        limit: 512,
    };

    let renderer = Renderer::new(&device, FORMAT);
    let mut slot = renderer.slot(&device);
    let frame = Frame {
        camera: &Camera::default(),
        mesh: &Arc::default(),
        sketches: &lines,
        grid: GridPlane::XY,
        faded: false,
        sketch: None,
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
        Err(too_large.clone())
    );
    // Reported once, not on every frame.
    assert_eq!(renderer.prepare(&mut slot, &device, &queue, &frame), Ok(()));
    // Other lines are tried, even with the same contents.
    let copy = Arc::new(RenderLines::clone(&lines));
    assert_eq!(
        renderer.prepare(
            &mut slot,
            &device,
            &queue,
            &Frame {
                sketches: &copy,
                ..frame
            }
        ),
        Err(too_large)
    );
}

#[test]
fn sketch_layers_past_the_buffer_limit_are_skipped() {
    // Segments take 80 bytes each: 6 fit in 512 bytes, 7 take 560.
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let layer = |segments: u32| {
        let mut layer = SketchLayer::default();
        let points: Vec<_> = (0..=segments)
            .map(|i| glam::DVec2::new(f64::from(i), f64::from(i % 2)))
            .collect();
        let style = LineStyle {
            color: Srgba([1.0; 4]),
            width: 1.0,
            dash: None,
        };
        layer.polyline(Space::Sketch, &points, style);
        layer
    };
    let (fits, too_large) = (Arc::new(layer(6)), Arc::new(layer(7)));
    let error = PrepareError::SketchTooLarge {
        bytes: 560,
        limit: 512,
    };

    let renderer = Renderer::new(&device, FORMAT);
    let mut slot = renderer.slot(&device);
    let empty = SketchLayer::default();
    let (camera, mesh, sketches) = (Camera::default(), Arc::default(), Arc::default());
    let frame = |base, live| Frame {
        camera: &camera,
        mesh: &mesh,
        sketches: &sketches,
        grid: GridPlane::XY,
        faded: false,
        sketch: Some(SketchScene {
            plane: GridPlane::XY,
            base,
            live,
        }),
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
    let mut prepare = |frame| renderer.prepare(&mut slot, &device, &queue, &frame);
    // As large as fits, in a buffer no larger than the device allows.
    assert_eq!(prepare(frame(&fits, &fits)), Ok(()));
    // The base layer is reported once, the live one on every frame.
    assert_eq!(prepare(frame(&too_large, &empty)), Err(error.clone()));
    assert_eq!(prepare(frame(&too_large, &empty)), Ok(()));
    for _ in 0..2 {
        assert_eq!(prepare(frame(&fits, &too_large)), Err(error.clone()));
    }
    assert_eq!(prepare(frame(&fits, &fits)), Ok(()));
}
