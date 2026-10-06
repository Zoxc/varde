//! Checks that a mesh past the device's buffer limit is skipped and
//! reported rather than a panic in wgpu.
// Holding the shared wgpu device in a `static` asks whether it's `Sync`
// deeper than the default limit.
#![recursion_limit = "256"]

use std::sync::Arc;

use glam::{DVec3, Vec3};
use varde_kernel::{MeshParts, RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::OriginShown;
use varde_render::{
    Camera, ClipRect, Colors, ErrorParts, Frame, GridPlane, Highlights, LineStyle, PrepareError,
    Renderer, Shading, SketchLayer, SketchScene, Space, Srgb, Srgba, Viewport, wgpu,
};

/// Nothing hovered or selected.
static NO_HIGHLIGHTS: std::sync::LazyLock<Arc<Highlights>> = std::sync::LazyLock::new(Arc::default);

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The renderer the tests share, on the shared device, made once: it
/// holds only pipelines, and building them is most of a test's time
/// (about 0.4 s in a debug build).
fn renderer(device: &wgpu::Device) -> Arc<Renderer> {
    static RENDERER: std::sync::OnceLock<Arc<Renderer>> = std::sync::OnceLock::new();
    RENDERER
        .get_or_init(|| Arc::new(Renderer::new(device, FORMAT)))
        .clone()
}
/// A black background and a grey model, with the app's scene colours
/// otherwise.
const COLORS: Colors = Colors {
    background_top: Srgb([0.0; 3]),
    background_bottom: Srgb([0.0; 3]),
    model: Srgb([0.5; 3]),
    contrast: 1.0,
    edge: Srgb([0.12, 0.13, 0.15]),
    grid: Srgb([0.45, 0.49, 0.54]),
    axes: [
        Srgb([0.85, 0.25, 0.22]),
        Srgb([0.30, 0.65, 0.25]),
        Srgb([0.20, 0.45, 0.85]),
    ],
    origin_outline: Srgb([0.2, 0.22, 0.25]),
    pivot: Srgb([0.04, 0.58, 0.68]),
    sketch: Srgb([0.04, 0.58, 0.68]),
    faded_alpha: 0.3,
    hidden_edge_alpha: 0.45,
    hover_face: Srgb([0.8; 3]),
    hover_outline: Srgb([0.75, 1.0, 0.6]),
    selected: Srgb([0.04, 0.58, 0.68]),
    selected_tint: 0.6,
    selected_edge_shade: 0.0,
    second: Srgb([1.0, 0.5, 0.0]),
    error: Srgb([0.9, 0.1, 0.1]),
    error_halo: Srgba([0.9, 0.1, 0.1, 0.3]),
};

// The binary's first GPU test pays for the device and the shared renderer's
// pipelines (about 0.5-2s in a debug build, more for each further texture
// format): a floor shared by every test here, so over the 0.5s aim.
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
                    max_bind_groups: 2,
                    ..wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
                },
                ..Default::default()
            };
            pollster::block_on(adapter.request_device(&descriptor)).ok()
        })
        .clone()
}

/// A frame of `mesh` and `sketches` on a 64 by 64 target.
fn frame<'a>(
    camera: &'a Camera,
    mesh: &'a Arc<RenderMesh>,
    sketches: &'a Arc<RenderLines>,
) -> Frame<'a> {
    Frame {
        camera,
        mesh,
        opacity: &[],
        tints: &[],
        sketches,
        grid: GridPlane::XY,
        faded: false,
        wireframe: false,
        tessellation: false,
        shading: Shading::Regular,
        hidden_edges: true,
        hovered_faces: &[],
        selected_faces: &[],
        second_faces: &[],
        hover_through: false,
        highlights: &NO_HIGHLIGHTS,
        errors: &[],
        sketch: None,
        pivot: None,
        // The grid's axis lines, as before the Z axis was drawn.
        origin: OriginShown {
            axes: [true, true, false],
            ..OriginShown::DEFAULT
        },
        viewport: Viewport {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        },
        target_size: [64, 64],
        scale_factor: 1.0,
        colors: COLORS,
    }
}

/// A cube's edges on a device taking 512-byte buffers: its 12 edges' 24
/// points, its 6 wires' 12 (the sides' diagonals) and the stream's two
/// ends, 20 bytes each.
const TOO_LARGE: PrepareError = PrepareError::MeshTooLarge {
    bytes: 760,
    limit: 512,
};

#[test]
fn mesh_past_the_buffer_limit_is_skipped() {
    // The cube's 24 positions take 288 bytes, which fit; its edges don't.
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert_eq!(device.limits().max_buffer_size, 512);
    let mesh = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 0, &Tolerance::DEFAULT)
        .unwrap()
        .tessellate(&varde_kernel::Display::default())
        .unwrap();
    assert_eq!(mesh.positions().len(), 24);
    let mesh = Arc::new(mesh);

    let renderer = renderer(&device);
    let mut slot = renderer.slot(&device);
    let (camera, sketches) = (Camera::default(), Arc::default());
    let frame = frame(&camera, &mesh, &sketches);
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

    // Without edges, 48 positions take 576 bytes.
    let positions = Arc::new(
        RenderMesh::from_parts(MeshParts {
            positions: vec![[0.0; 3]; 48],
            normals: vec![[0.0, 0.0, 1.0]; 48],
            indices: vec![0, 1, 2],
            face_ends: vec![3],
            part_ends: vec![[1, 0, 0, 0]],
            ..MeshParts::default()
        })
        .unwrap(),
    );
    assert_eq!(
        renderer.prepare(
            &mut slot,
            &device,
            &queue,
            &Frame {
                mesh: &positions,
                ..frame
            }
        ),
        Err(PrepareError::MeshTooLarge {
            bytes: 576,
            limit: 512
        })
    );
}

#[test]
fn edges_fit_up_to_the_buffer_limit() {
    // 20 bytes a point. Open, an edge of 23 points and the stream's two
    // ends take 500 bytes, which fit; one of 24 takes 520. Closed, an edge
    // of 20 points and its first again also takes a point either side
    // where it closes: 500 bytes; one of 21 takes 520.
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let edge = |points: u32, closed: bool| {
        let positions: Vec<[f32; 3]> = (0..points)
            .map(|i| [i as f32, (i % 2) as f32, 0.0])
            .collect();
        let (first, last) = (positions[0], positions[positions.len() - 1]);
        let (vertices, corners, edge_corners, corner_count): (Vec<u32>, _, _, _) = if closed {
            ((0..points).chain([0]).collect(), vec![first], [0, 0], 1)
        } else {
            ((0..points).collect(), vec![first, last], [0, 1], 2)
        };
        Arc::new(
            RenderMesh::from_parts(MeshParts {
                normals: vec![[0.0, 0.0, 1.0]; positions.len()],
                positions,
                indices: vec![0, 1, 2],
                face_ends: vec![3],
                edge_ends: vec![vertices.len() as u32],
                edge_vertices: vertices,
                edge_faces: vec![[0, 0]],
                corners,
                edge_corners: vec![edge_corners],
                part_ends: vec![[1, 1, corner_count, 0]],
                ..MeshParts::default()
            })
            .unwrap(),
        )
    };
    let renderer = renderer(&device);
    let mut slot = renderer.slot(&device);
    let (camera, sketches) = (Camera::default(), Arc::default());
    let mut prepare = |mesh: &Arc<RenderMesh>| {
        renderer.prepare(&mut slot, &device, &queue, &frame(&camera, mesh, &sketches))
    };
    for (fits, too_large, closed) in [(23, 24, false), (20, 21, true)] {
        let (fits, too_large) = (edge(fits, closed), edge(too_large, closed));
        assert_eq!(prepare(&fits), Ok(()), "closed: {closed}");
        assert_eq!(
            prepare(&too_large),
            Err(PrepareError::MeshTooLarge {
                bytes: 520,
                limit: 512
            }),
            "closed: {closed}"
        );
    }
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

    let renderer = renderer(&device);
    let mut slot = renderer.slot(&device);
    let (camera, mesh) = (Camera::default(), Arc::default());
    let frame = frame(&camera, &mesh, &lines);
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
    // Segments take 96 bytes each: 5 fit in 512 bytes, 6 take 576.
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
    let (fits, too_large) = (Arc::new(layer(5)), Arc::new(layer(6)));
    let error = PrepareError::SketchTooLarge {
        bytes: 576,
        limit: 512,
    };

    let renderer = renderer(&device);
    let mut slot = renderer.slot(&device);
    let empty = SketchLayer::default();
    let (camera, mesh, sketches) = (Camera::default(), Arc::default(), Arc::default());
    let frame = |base, live| Frame {
        sketch: Some(SketchScene {
            plane: GridPlane::XY,
            depth_tested: false,
            base,
            live,
        }),
        ..frame(&camera, &mesh, &sketches)
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

#[test]
fn errors_past_the_buffer_limit_are_skipped() {
    // A polyline of 30 points is a stream of 32 points of 20 bytes, 640
    // bytes.
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mut lines = RenderLines::default();
    lines.push((0..30).map(|i| Vec3::X * i as f32)).unwrap();
    let mesh = RenderMesh::default();
    let source: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let parts = |source: &Arc<dyn std::any::Any + Send + Sync>| ErrorParts {
        mesh: &mesh,
        lines: &lines,
        points: &[],
        source: Arc::downgrade(source),
        halo_only: false,
    };
    let errors = [parts(&source)];
    let too_large = PrepareError::ErrorsTooLarge {
        bytes: 640,
        limit: 512,
    };

    let renderer = renderer(&device);
    let mut slot = renderer.slot(&device);
    let (camera, model, sketches) = (Camera::default(), Arc::default(), Arc::default());
    let frame = Frame {
        errors: &errors,
        ..frame(&camera, &model, &sketches)
    };
    assert_eq!(
        renderer.prepare(&mut slot, &device, &queue, &frame),
        Err(too_large.clone())
    );
    // Reported once, not on every frame, and the frame is drawn without.
    assert_eq!(renderer.prepare(&mut slot, &device, &queue, &frame), Ok(()));
    let mut encoder = device.create_command_encoder(&Default::default());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let clip = ClipRect {
        x: 0,
        y: 0,
        width: 64,
        height: 64,
    };
    renderer.render(&slot, &mut encoder, &view, clip);
    queue.submit([encoder.finish()]);
    // Another source is tried, even with the same parts.
    let other: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let errors = [parts(&other)];
    assert_eq!(
        renderer.prepare(
            &mut slot,
            &device,
            &queue,
            &Frame {
                errors: &errors,
                ..frame
            }
        ),
        Err(too_large)
    );
    // Smaller ones after, from another source, are uploaded and drawn.
    let mut small = RenderLines::default();
    small.push([Vec3::ZERO, Vec3::X]).unwrap();
    let smaller: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let errors = [ErrorParts {
        lines: &small,
        ..parts(&smaller)
    }];
    let frame = Frame {
        errors: &errors,
        ..frame
    };
    assert_eq!(renderer.prepare(&mut slot, &device, &queue, &frame), Ok(()));
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.render(&slot, &mut encoder, &view, clip);
    queue.submit([encoder.finish()]);
}
