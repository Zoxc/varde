//! The renderer on wgpu's GL backend, which the browser build runs on
//! (WebGL2): its own binary, so its instance never meets the Vulkan one
//! of the other tests. Skipped where there's no GL adapter.

use std::sync::Arc;

use glam::{DVec2, DVec3, Vec3};
use varde_kernel::{RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::{
    Camera, ClipRect, Colors, Frame, GridPlane, Projection, Renderer, SketchLayer, SketchScene,
    Space, Srgb, Srgba, View, Viewport, wgpu,
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
    pivot: Srgb([0.04, 0.58, 0.68]),
    sketch: Srgb([1.0, 1.0, 0.0]),
    faded_alpha: 0.3,
    // Pure green and pure blue faces, pure red and pure cyan edges.
    hovered_face: Srgb([0.0, 1.0, 0.0]),
    selected_face: Srgb([0.0, 0.0, 1.0]),
    hovered_edge: Srgba([1.0, 0.0, 0.0, 1.0]),
    selected_edge: Srgba([0.0, 1.0, 1.0, 1.0]),
    hidden_edge_alpha: 0.45,
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

/// From the top, a cube from (-2, -2, 0) to (2, 2, 4) over the whole
/// middle of a view 12 high.
fn cube_from_top() -> (Camera, Arc<RenderMesh>) {
    let mesh = Solid::cuboid(
        DVec3::new(-2.0, -2.0, 0.0),
        DVec3::splat(4.0),
        0,
        &Tolerance::DEFAULT,
    )
    .unwrap()
    .tessellate(&varde_kernel::Display::default())
    .unwrap();
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    camera.zoom(12.0 / camera.view_height());
    (camera, Arc::new(mesh))
}

/// A frame of `mesh` and `sketches` filling a target of [`SIZE`].
fn frame<'a>(
    camera: &'a Camera,
    mesh: &'a Arc<RenderMesh>,
    sketches: &'a Arc<RenderLines>,
) -> Frame<'a> {
    let [width, height] = SIZE.map(|s| s as f32);
    Frame {
        camera,
        mesh,
        sketches,
        grid: GridPlane::XY,
        faded: false,
        hidden_edges: true,
        sketch: None,
        pivot: None,
        highlight: None,
        viewport: Viewport {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        target_size: SIZE,
        scale_factor: 1.0,
        colors: COLORS,
    }
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
    let (camera, mesh) = cube_from_top();
    let plane = GridPlane::new(Vec3::new(0.0, 0.0, 2.0), Vec3::X, Vec3::Y).unwrap();
    let mut live = SketchLayer::default();
    let square =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| DVec2::new(x, y));
    live.fill(Space::On(plane), [&square[..]], Srgba([0.0, 1.0, 0.0, 1.0]));
    let renderer = Renderer::new(&device, FORMAT);
    let (sketches, base) = (Arc::default(), Arc::new(SketchLayer::default()));
    for depth_tested in [false, true] {
        let frame = Frame {
            sketch: Some(SketchScene {
                plane: GridPlane::XY,
                depth_tested,
                base: &base,
                live: &live,
            }),
            ..frame(&camera, &mesh, &sketches)
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

#[test]
fn edges_are_drawn_on_gl() {
    // From the top, a cube from (-2, -2, 0) to (2, 2, 4), its edges
    // yellow: the edge stream bound at offsets of a point and more, which
    // GL takes as attribute offsets.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let (camera, mesh) = cube_from_top();
    let renderer = Renderer::new(&device, FORMAT);
    let sketches = Arc::default();
    let frame = Frame {
        colors: Colors {
            edge: Srgb([1.0, 1.0, 0.0]),
            ..COLORS
        },
        ..frame(&camera, &mesh, &sketches)
    };
    let pixels = draw(&device, &queue, &renderer, &frame);
    // The edge at x = 2, 2 / 12 of the view's height right of its middle,
    // looked for in a row above the origin marker.
    let column = SIZE[0] / 2 + 2 * SIZE[1] / 12;
    let yellow = (column - 2..=column + 2).any(|x| {
        let [r, g, b, _] = pixels[(50 * SIZE[0] + x) as usize];
        r > 150 && g > 150 && b < 100
    });
    assert!(yellow, "no edge near column {column}");
}

#[test]
fn a_closed_edge_is_joined_where_it_closes_on_gl() {
    // From the top in perspective, the circle round a cylinder's top, one
    // edge closing on itself, the one round its bottom smaller within it
    // and hidden. Faded, a pixel drawn twice would be more opaque than the
    // faded colour over what's drawn opaque. The points either side of
    // where it closes are marked in their edge's top bit, which GL takes
    // as an integer attribute.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let mesh = Solid::cylinder(DVec3::ZERO, 3.0, 2.0, 0, &Tolerance::DEFAULT)
        .unwrap()
        .tessellate(&varde_kernel::Display::default())
        .unwrap();
    let mesh = Arc::new(mesh);
    let mut camera = Camera::default();
    camera.set_projection(Projection::Perspective);
    camera.look_from(View::Top);
    camera.zoom(12.0 / camera.view_height());
    let renderer = Renderer::new(&device, FORMAT);
    let sketches = Arc::default();
    let render = |faded| {
        let frame = Frame {
            faded,
            // The bottom's circle, hidden, isn't drawn faded.
            hidden_edges: false,
            // Its plane seen edge on, so the grid doesn't show.
            grid: GridPlane::new(Vec3::new(0.0, 1000.0, 0.0), Vec3::X, Vec3::Z).unwrap(),
            colors: Colors {
                edge: Srgb([1.0, 1.0, 0.0]),
                ..COLORS
            },
            ..frame(&camera, &mesh, &sketches)
        };
        draw(&device, &queue, &renderer, &frame)
    };
    let (faded, opaque) = (render(true), render(false));
    // How much yellow covers a pixel: over the grey faces, red and green
    // gain on blue as much as it does.
    let yellowness = |[r, g, b, _]: [u8; 4]| f32::from(r.min(g).saturating_sub(b)) / 255.0;
    let alpha = COLORS.faded_alpha;
    let mut drawn = 0;
    for (i, (faded, opaque)) in faded.iter().zip(&opaque).enumerate() {
        let (faded, opaque) = (yellowness(*faded), yellowness(*opaque));
        let [x, y] = [i as u32 % SIZE[0], i as u32 / SIZE[0]];
        assert!(
            (faded - opaque * alpha).abs() < 0.03,
            "{faded} for {opaque} at ({x}, {y})"
        );
        drawn += u32::from(opaque > 0.5);
    }
    assert!(drawn > 100, "{drawn} pixels of edge");
}

#[test]
fn hidden_edges_are_drawn_on_gl() {
    // From the top in perspective, the cube's bottom square shows within
    // its top face, hidden by it: drawn dashed with the option on, by a
    // pipeline of its own entry point, which GL keys programs by; nothing
    // with it off.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let (mut camera, mesh) = cube_from_top();
    camera.set_projection(Projection::Perspective);
    let renderer = Renderer::new(&device, FORMAT);
    let sketches = Arc::default();
    let yellow = |hidden_edges| {
        let frame = Frame {
            hidden_edges,
            grid: GridPlane::new(Vec3::new(0.0, 1000.0, 0.0), Vec3::X, Vec3::Z).unwrap(),
            colors: Colors {
                edge: Srgb([1.0, 1.0, 0.0]),
                ..COLORS
            },
            ..frame(&camera, &mesh, &sketches)
        };
        let pixels = draw(&device, &queue, &renderer, &frame);
        // Within the top face, clear of its outline.
        let middle = SIZE.map(|s| s / 2);
        let mut yellow = 0;
        for y in middle[1] - 25..middle[1] + 25 {
            for x in middle[0] - 25..middle[0] + 25 {
                let [r, g, b, _] = pixels[(y * SIZE[0] + x) as usize];
                yellow += u32::from(r.min(g).saturating_sub(b) > 40);
            }
        }
        yellow
    };
    let (on, off) = (yellow(true), yellow(false));
    assert!(on > 40, "{on} pixels of hidden edges");
    assert_eq!(off, 0);
}
