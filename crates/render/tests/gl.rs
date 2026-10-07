//! The renderer on wgpu's GL backend, which the browser build runs on
//! (WebGL2): its own binary, so its instance never meets the Vulkan one
//! of the other tests. Skipped where there's no GL adapter.
// Holding the shared wgpu device in a `static` asks whether it's `Sync`
// deeper than the default limit.
#![recursion_limit = "256"]

use std::sync::Arc;

use glam::{DVec2, DVec3, Vec3};
use varde_kernel::{RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::OriginShown;
use varde_render::{
    Camera, ClipRect, Colors, ErrorParts, Frame, GridPlane, Highlights, Projection, Renderer,
    Shading, SketchLayer, SketchScene, Space, Srgb, Srgba, Vertex, View, Viewport, wgpu,
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Nothing hovered or selected.
static NO_HIGHLIGHTS: std::sync::LazyLock<Arc<Highlights>> = std::sync::LazyLock::new(Arc::default);
const SIZE: [u32; 2] = [128, 128];

/// A black background and a grey model.
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
    sketch: Srgb([1.0, 1.0, 0.0]),
    faded_alpha: 0.3,
    hidden_edge_alpha: 0.45,
    hover_face: Srgb([0.8; 3]),
    hover_outline: Srgb([0.75, 1.0, 0.6]),
    selected: Srgb([0.04, 0.58, 0.68]),
    selected_tint: 0.6,
    selected_edge_shade: 0.0,
    pattern: varde_render::PatternStyle::DEFAULT,
    second: Srgb([1.0, 0.5, 0.0]),
    error: Srgb([0.9, 0.1, 0.1]),
    error_halo: Srgba([0.9, 0.1, 0.1, 0.3]),
};

/// [`COLORS`] with yellow edges.
const YELLOW_EDGES: Colors = Colors {
    edge: Srgb([1.0, 1.0, 0.0]),
    ..COLORS
};

/// A grid plane seen edge on from the top, so the grid doesn't show.
fn hidden_grid() -> GridPlane {
    GridPlane::new(Vec3::new(0.0, 1000.0, 0.0), Vec3::X, Vec3::Z).unwrap()
}

// The binary's first GPU test pays for the device and the shared renderer's
// pipelines (about 0.5-2s in a debug build, more for each further texture
// format): a floor shared by every test here, so over the 0.5s aim.
/// The one GL device the tests share, if there's an adapter for it.
fn gl_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    static DEVICE: std::sync::OnceLock<Option<(wgpu::Device, wgpu::Queue)>> =
        std::sync::OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..Default::default()
            });
            let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                // As iced asks for, which allows two bind groups only.
                required_limits: wgpu::Limits {
                    max_bind_groups: 2,
                    ..wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
                },
                ..Default::default()
            }))
            .ok()
        })
        .clone()
}

/// The renderer the tests share, on the shared GL device, made once: it
/// holds only pipelines, and building them is most of a test's time
/// (about 0.45 s in a debug build).
fn renderer(device: &wgpu::Device) -> Arc<Renderer> {
    static RENDERER: std::sync::OnceLock<Arc<Renderer>> = std::sync::OnceLock::new();
    RENDERER
        .get_or_init(|| Arc::new(Renderer::new(device, FORMAT)))
        .clone()
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
        opacity: &[],
        tints: &[],
        sketches,
        grid: GridPlane::XY,
        fade: 0.0,
        fading_grid: None,
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
        // The world's axes would cross what the tests read.
        origin: OriginShown::NONE,
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
    let renderer = renderer(&device);
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
fn a_closed_edge_is_joined_where_it_closes_on_gl() {
    // From the top in perspective, the circle round a cylinder's top, one
    // edge closing on itself. Faded, a pixel drawn twice would be more
    // opaque than the faded colour over what's drawn opaque. This also
    // covers the edge stream bound at offsets of a point (GL attribute
    // offsets) and the neighbour mark in the edge's top bit (an integer
    // attribute).
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
    let renderer = renderer(&device);
    let sketches = Arc::default();
    let render = |faded| {
        let frame = Frame {
            fade: if faded { 1.0 } else { 0.0 },
            fading_grid: None,
            // The bottom's circle, hidden, isn't drawn faded.
            hidden_edges: false,
            grid: hidden_grid(),
            colors: YELLOW_EDGES,
            ..frame(&camera, &mesh, &sketches)
        };
        draw(&device, &queue, &renderer, &frame)
    };
    let (faded, opaque) = (render(true), render(false));
    // How much yellow covers a pixel, over the grey faces.
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
    // its top face, hidden by it: drawn with the option on, by an entry
    // point of its own, which GL keys programs by; nothing with it off.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let (mut camera, mesh) = cube_from_top();
    camera.set_projection(Projection::Perspective);
    let renderer = renderer(&device);
    let sketches = Arc::default();
    let yellow = |hidden_edges| {
        let frame = Frame {
            hidden_edges,
            grid: hidden_grid(),
            colors: YELLOW_EDGES,
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

#[test]
fn transparent_parts_are_drawn_on_gl() {
    // From the top, the cube at 30 % and at 60 %: each part's alpha is
    // picked with a dynamic offset into a uniform buffer, which GL binds
    // as a range of it. Its middle is its bottom's back face, lit as its
    // top is, and its top over it, each at that alpha, over black.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let (camera, mesh) = cube_from_top();
    let renderer = renderer(&device);
    let sketches = Arc::default();
    let middle = |opacity: &[f32]| {
        let frame = Frame {
            opacity,
            grid: hidden_grid(),
            ..frame(&camera, &mesh, &sketches)
        };
        let pixels = draw(&device, &queue, &renderer, &frame);
        // Up and left of the origin marker.
        pixels[((SIZE[1] / 2 - 15) * SIZE[0] + SIZE[0] / 2 - 15) as usize]
    };
    let opaque = middle(&[]);
    for opacity in [0.3, 0.6] {
        let drawn = middle(&[opacity]);
        let alpha = (opacity * 255.0).round() / 255.0;
        for c in 0..3 {
            let expected = f32::from(opaque[c]) * alpha * (2.0 - alpha);
            assert!(
                (f32::from(drawn[c]) - expected).abs() <= 2.0,
                "{drawn:?} at {opacity}, {opaque:?} opaque"
            );
        }
    }
}

#[test]
fn hover_and_selection_are_drawn_on_gl() {
    // From the top, the cube's top face hovered, then selected, drawn
    // again over itself by programs of their own at exactly its depth
    // (`Equal`, the position invariant); its edges outlined and a corner
    // hovered, by entry points of their own.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let (camera, mesh) = cube_from_top();
    let top = mesh
        .faces()
        .position(|indices| mesh.normals()[indices[0] as usize][2] > 0.99)
        .unwrap() as u32;
    let renderer = renderer(&device);
    let sketches = Arc::default();
    let draw_with = |hovered_faces: &[u32], selected_faces: &[u32], highlights: Highlights| {
        let highlights = Arc::new(highlights);
        let frame = Frame {
            hovered_faces,
            selected_faces,
            highlights: &highlights,
            errors: &[],
            grid: hidden_grid(),
            ..frame(&camera, &mesh, &sketches)
        };
        draw(&device, &queue, &renderer, &frame)
    };
    let plain = draw_with(&[], &[], Highlights::default());
    let hovered = draw_with(&[top], &[], Highlights::default());
    let selected = draw_with(&[], &[top], Highlights::default());
    // Up and left of the origin marker.
    let at =
        |pixels: &[[u8; 4]]| pixels[((SIZE[1] / 2 - 15) * SIZE[0] + SIZE[0] / 2 - 15) as usize];
    let [plain_at, hovered_at, selected_at] = [&plain, &hovered, &selected].map(|p| at(p));
    let sum = |p: [u8; 4]| (0..3).map(|c| i32::from(p[c])).sum::<i32>();
    assert!(
        sum(hovered_at) > sum(plain_at) + 60,
        "{hovered_at:?} hovered, {plain_at:?} not"
    );
    assert!(
        i32::from(selected_at[2]) - i32::from(selected_at[0]) > 40,
        "{selected_at:?} selected"
    );

    let edges = (0..mesh.edge_count() as u32).collect();
    let outlined = draw_with(
        &[],
        &[],
        Highlights {
            outlined: edges,
            vertices: vec![Vertex {
                corner: 0,
                hovered: true,
                selected: false,
            }],
            ..Highlights::default()
        },
    );
    // The outline's rim, bright green, around the top's square.
    let green = outlined
        .iter()
        .filter(|[r, g, b, _]| *g > 200 && *g > r.saturating_add(20) && *g > b.saturating_add(40))
        .count();
    assert!(green > 200, "{green} pixels of outline");
}

#[test]
fn errors_and_their_halo_are_drawn_on_gl() {
    // The halo's coverage is drawn into an R8Unorm target with Max
    // blending and read back texel by texel (`textureLoad`) over the
    // frame, all of which WebGL2 has.
    let Some((device, queue)) = gl_device() else {
        eprintln!("no GL adapter, skipping");
        return;
    };
    let (camera, mesh) = cube_from_top();
    // 128 / 12 pixels a unit: a line at y = 4.5 is on the boundary of rows
    // 15 and 16, off the cube; one at y = 0 is under the cube.
    let mut lines = RenderLines::default();
    lines
        .push([Vec3::new(-4.0, 4.5, 0.0), Vec3::new(4.0, 4.5, 0.0)])
        .unwrap();
    let mut under = RenderLines::default();
    under
        .push([Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.0, -1.0)])
        .unwrap();
    let source: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let empty = RenderMesh::default();
    let parts = |lines| ErrorParts {
        mesh: &empty,
        lines,
        points: &[],
        source: Arc::downgrade(&source),
        halo_only: false,
    };
    let errors = [parts(&lines), parts(&under)];
    let renderer = renderer(&device);
    let sketches = Arc::default();
    let pixels = draw(
        &device,
        &queue,
        &renderer,
        &Frame {
            grid: hidden_grid(),
            errors: &errors,
            ..frame(&camera, &mesh, &sketches)
        },
    );
    let at = |x: u32, y: u32| pixels[(y * SIZE[0] + x) as usize];
    // The core, then the halo alone both sides at the error's colour (0.9
    // red) at 0.3, then nothing; and nothing where a halo read upside
    // down would be.
    for row in [15, 16] {
        let [r, g, _, _] = at(64, row);
        assert!(r > 200 && g < 40, "row {row}: {:?}", at(64, row));
    }
    for row in [7, 24] {
        let [r, g, _, _] = at(64, row);
        assert!(
            (60..=80).contains(&r) && g < 15,
            "row {row}: {:?}",
            at(64, row)
        );
    }
    for row in [36, 112] {
        assert_eq!(at(64, row), [0, 0, 0, 255], "row {row}");
    }
    // Under the cube, dimmed: red, with the cube showing through.
    let [r, g, _, _] = at(64, 64);
    assert!(r > g + 30 && g > 60, "{:?}", at(64, 64));
}
