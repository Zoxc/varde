//! Renders into an offscreen texture and checks the scene stays inside its viewport.
// Holding the shared wgpu device in a `static` asks whether it's `Sync`
// deeper than the default limit.
#![recursion_limit = "256"]

use std::sync::Arc;

use glam::{DVec3, Vec3};
use varde_kernel::{RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::{
    Camera, ClipRect, Colors, Emphasis, Frame, GridPlane, Highlight, LINE_WIDTH, LineStyle, Pivot,
    PointStyle, Projection, Renderer, SketchLayer, SketchScene, Space, Srgb, Srgba, View, Viewport,
    wgpu,
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
    pivot: Srgb([0.04, 0.58, 0.68]),
    // Pure yellow, found by its lack of blue.
    sketch: Srgb([1.0, 1.0, 0.0]),
    faded_alpha: 0.3,
    // Pure green and pure blue faces, pure red and pure cyan edges.
    hovered_face: Srgb([0.0, 1.0, 0.0]),
    selected_face: Srgb([0.0, 0.0, 1.0]),
    hovered_edge: Srgba([1.0, 0.0, 0.0, 1.0]),
    selected_edge: Srgba([0.0, 1.0, 1.0, 1.0]),
};
const SIZE: [u32; 2] = [512, 256];
const SENTINEL: [u8; 4] = [255, 0, 255, 255];

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

/// Renders `mesh` into `viewport` and returns RGBA8 pixels, row-major.
fn render(
    camera: &Camera,
    mesh: &RenderMesh,
    viewport: Viewport,
    clip: ClipRect,
    scale_factor: f32,
) -> Option<Vec<[u8; 4]>> {
    render_to(FORMAT, camera, mesh, viewport, clip, scale_factor)
}

/// Like [`render`], into a target of the given RGBA8 format, returning the
/// stored bytes.
fn render_to(
    format: wgpu::TextureFormat,
    camera: &Camera,
    mesh: &RenderMesh,
    viewport: Viewport,
    clip: ClipRect,
    scale_factor: f32,
) -> Option<Vec<[u8; 4]>> {
    draw(
        format,
        &Frame {
            camera,
            mesh: &Arc::new(mesh.clone()),
            sketches: &Arc::default(),
            grid: GridPlane::XY,
            faded: false,
            sketch: None,
            pivot: None,
            highlight: None,
            viewport,
            target_size: SIZE,
            scale_factor,
            colors: COLORS,
        },
        clip,
    )
}

/// What a test draws besides the model, and how.
#[derive(Default)]
struct Extras {
    sketches: RenderLines,
    grid: GridPlane,
    faded: bool,
    /// The sketch being edited: its plane and a layer of it, drawn as its
    /// live layer if `live`, else as its base layer.
    sketch: Option<(GridPlane, SketchLayer)>,
    live: bool,
    /// Whether the sketch is hidden by the model in front of it.
    depth_tested: bool,
    pivot: Option<Pivot>,
    /// The faces and edges hovered and selected.
    highlight: Option<Highlight>,
}

/// Renders `mesh` and `extras` into [`VIEWPORT`] at a scale factor of 1.
fn render_with(camera: &Camera, mesh: &RenderMesh, extras: Extras) -> Option<Vec<[u8; 4]>> {
    render_scaled(camera, mesh, extras, VIEWPORT, CLIP, 1.0)
}

/// Like [`render_with`], into `viewport` at `scale_factor`.
fn render_scaled(
    camera: &Camera,
    mesh: &RenderMesh,
    extras: Extras,
    viewport: Viewport,
    clip: ClipRect,
    scale_factor: f32,
) -> Option<Vec<[u8; 4]>> {
    let layers = extras.sketch.map(|(plane, layer)| {
        let (base, live) = if extras.live {
            (SketchLayer::default(), layer)
        } else {
            (layer, SketchLayer::default())
        };
        (plane, Arc::new(base), live)
    });
    let highlight = extras.highlight.map(Arc::new);
    let sketch = layers.as_ref().map(|(plane, base, live)| SketchScene {
        plane: *plane,
        depth_tested: extras.depth_tested,
        base,
        live,
    });
    draw(
        FORMAT,
        &Frame {
            camera,
            mesh: &Arc::new(mesh.clone()),
            sketches: &Arc::new(extras.sketches),
            grid: extras.grid,
            faded: extras.faded,
            sketch,
            pivot: extras.pivot,
            highlight: highlight.as_ref(),
            viewport,
            target_size: SIZE,
            scale_factor,
            colors: COLORS,
        },
        clip,
    )
}

/// Draws `frame` within `clip` into a target of `format`, and returns the
/// stored bytes.
fn draw(format: wgpu::TextureFormat, frame: &Frame<'_>, clip: ClipRect) -> Option<Vec<[u8; 4]>> {
    let (device, queue) = device()?;
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
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

    let renderer = renderer(&device, format);
    let mut slot = renderer.slot(&device);
    renderer.prepare(&mut slot, &device, &queue, frame).unwrap();

    let mut encoder = device.create_command_encoder(&Default::default());
    let [r, g, b, a] = SENTINEL.map(|c| c as f64 / 255.0);
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    renderer.render(&slot, &mut encoder, &view, clip);

    let bytes_per_row = width * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (bytes_per_row * height) as u64,
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
    Some(bytemuck_pixels(&data))
}

/// The renderer for `format` on the shared `device`, made once: building
/// its pipelines is most of what drawing a test's frame takes.
fn renderer(device: &wgpu::Device, format: wgpu::TextureFormat) -> Arc<Renderer> {
    static RENDERERS: std::sync::Mutex<Vec<(wgpu::TextureFormat, Arc<Renderer>)>> =
        std::sync::Mutex::new(Vec::new());
    let mut renderers = RENDERERS.lock().unwrap();
    if let Some((_, renderer)) = renderers.iter().find(|(f, _)| *f == format) {
        return renderer.clone();
    }
    let renderer = Arc::new(Renderer::new(device, format));
    renderers.push((format, renderer.clone()));
    renderer
}

fn bytemuck_pixels(data: &[u8]) -> Vec<[u8; 4]> {
    data.as_chunks::<4>().0.to_vec()
}

fn pixel(pixels: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    pixels[(y * SIZE[0] + x) as usize]
}

/// A viewport in the right half, offset from the top.
const VIEWPORT: Viewport = Viewport {
    x: 128.0,
    y: 32.0,
    width: 96.0,
    height: 64.0,
};
const CLIP: ClipRect = ClipRect {
    x: 128,
    y: 32,
    width: 96,
    height: 64,
};

#[test]
fn draws_only_inside_viewport() {
    let Some(pixels) = render(
        &Camera::default(),
        &RenderMesh::default(),
        VIEWPORT,
        CLIP,
        1.0,
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    for y in 0..SIZE[1] {
        for x in 0..SIZE[0] {
            let inside = (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y);
            let touched = pixel(&pixels, x, y) != SENTINEL;
            assert_eq!(inside, touched, "pixel ({x}, {y})");
        }
    }
}

#[test]
fn origin_is_centered_in_viewport() {
    // The default camera orbits the origin.
    let Some(pixels) = render(
        &Camera::default(),
        &RenderMesh::default(),
        VIEWPORT,
        CLIP,
        1.0,
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    // The origin dot is white; find the centroid of white pixels.
    let (mut sx, mut sy, mut n) = (0u64, 0u64, 0u64);
    for y in 0..SIZE[1] {
        for x in 0..SIZE[0] {
            if pixel(&pixels, x, y)[..3].iter().all(|&c| c > 240) {
                sx += x as u64;
                sy += y as u64;
                n += 1;
            }
        }
    }
    assert!(n > 0, "origin dot not drawn");
    let (cx, cy) = (sx as f32 / n as f32, sy as f32 / n as f32);
    let expected = (
        VIEWPORT.x + VIEWPORT.width / 2.0,
        VIEWPORT.y + VIEWPORT.height / 2.0,
    );
    assert!(
        (cx - expected.0).abs() < 1.5 && (cy - expected.1).abs() < 1.5,
        "origin at ({cx}, {cy}), expected {expected:?}"
    );
}

#[test]
fn draws_bodies_far_along_the_view_axis() {
    // Far behind the target, but on screen in an orthographic front view.
    let mut camera = Camera::default();
    camera.look_from(View::Front);
    let mesh = cube(2.0, Vec3::new(0.0, 1000.0, 0.0));
    let Some(pixels) = render(&camera, &mesh, VIEWPORT, CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    // Shading lights the model; the background is black.
    let lit = pixel(&pixels, x0 + w / 2, y0 + h / 2);
    assert!(lit[..3].iter().all(|&c| c > 64), "centre is {lit:?}");
}

#[test]
fn edges_stay_in_front_of_faces_zoomed_into_a_large_scene() {
    // Zoomed far into the top front edge of a unit cube, with a second cube
    // far behind it stretching the depth range, so pulling edges towards the
    // camera by a fraction of the view height falls below depth precision.
    let mut camera = Camera::default();
    camera.set_target(Vec3::new(0.5, 0.0, 1.0));
    camera.zoom(0.01 / camera.view_height());
    let mut mesh = cube(1.0, Vec3::ZERO);
    mesh.append(&cube(1.0, Vec3::Y * 1e5)).unwrap();
    let Some(pixels) = render(&camera, &mesh, VIEWPORT, CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    // The edge crosses every column. Faces are lit and edges are dark.
    for x in x0..x0 + w {
        let dark = (y0..y0 + h).any(|y| pixel(&pixels, x, y)[..3].iter().all(|&c| c < 64));
        assert!(dark, "no edge in column {x}");
    }
}

#[test]
fn axes_stay_put_zoomed_into_a_large_scene() {
    // Zoomed far into the origin, with a cube at the edge of the document
    // limit stretching the depth range, so unprojecting the near and far planes
    // to find the ground loses pixels to rounding.
    let mut camera = Camera::default();
    camera.zoom(0.01 / camera.view_height());
    let mesh = cube(1.0, Vec3::Y * varde_kernel::MAX_COORD);
    let Some(pixels) = render(&camera, &mesh, VIEWPORT, CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let center = glam::Vec2::new(
        VIEWPORT.x + VIEWPORT.width / 2.0,
        VIEWPORT.y + VIEWPORT.height / 2.0,
    );
    // The X axis is red and the Y axis green, each a line through the origin
    // dot at the centre, along the axis as seen on screen.
    let red: fn([u8; 4]) -> bool = |[r, g, _, _]| r > 128 && g < 128;
    let green: fn([u8; 4]) -> bool = |[r, g, _, _]| g > 128 && r < 128;
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    for (axis, colored) in [(Vec3::X, red), (Vec3::Y, green)] {
        let along = glam::Vec2::new(camera.right().dot(axis), -camera.up().dot(axis)).normalize();
        let mut drawn = 0;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if !colored(pixel(&pixels, x, y)) {
                    continue;
                }
                let offset = glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - center;
                let off = offset.perp_dot(along).abs();
                assert!(off < 2.0, "{axis} axis at ({x}, {y}) is {off} pixels off");
                drawn += 1;
            }
        }
        assert!(drawn > 100, "{axis} axis has only {drawn} pixels");
    }
}

#[test]
fn origin_marker_follows_scale_factor() {
    // The same logical viewport at 1x and 2x: the marker should cover twice
    // the physical pixels in each direction at 2x.
    let [width, height] = SIZE;
    let measure = |scale: f32| {
        let size = [width as f32 / 2.0 * scale, height as f32 / 2.0 * scale];
        let viewport = Viewport {
            x: 0.0,
            y: 0.0,
            width: size[0],
            height: size[1],
        };
        let clip = ClipRect {
            x: 0,
            y: 0,
            width: size[0] as u32,
            height: size[1] as u32,
        };
        let pixels = render(
            &Camera::default(),
            &RenderMesh::default(),
            viewport,
            clip,
            scale,
        )?;
        let center_x = size[0] / 2.0;
        // The ring's core and the dot are white, and its edges nearly so:
        // how many pixels, and how far the ring reaches sideways.
        let (mut white, mut reach) = (0, 0.0f32);
        for y in 0..clip.height {
            for x in 0..clip.width {
                if pixel(&pixels, x, y)[..3].iter().all(|&c| c > 200) {
                    white += 1;
                    reach = reach.max((x as f32 + 0.5 - center_x).abs());
                }
            }
        }
        Some((white as f32, reach))
    };
    let (Some((white1, reach1)), Some((white2, reach2))) = (measure(1.0), measure(2.0)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert!(
        white1 > 0.0 && reach1 > 5.0,
        "white {white1}, reach {reach1}"
    );
    let (white, reach) = (white2 / white1, reach2 / reach1);
    assert!((3.0..5.0).contains(&white), "white area grew {white}x");
    assert!((1.8..2.2).contains(&reach), "ring grew {reach}x");
}

#[test]
fn origin_marker_is_a_ring_flat_in_the_grid_plane() {
    // From the top the ring is round; from the front, with the XY plane
    // edge on, it's flattened to a line along the X axis. The dot in the
    // middle stays round.
    let white = |p: [u8; 4]| p[..3].iter().all(|&c| c > 200);
    let extent = |camera: &Camera| {
        let pixels = render(camera, &RenderMesh::default(), VIEWPORT, CLIP, 1.0)?;
        let (cx, cy) = CENTER;
        let (mut wide, mut tall) = (0u32, 0u32);
        for y in CLIP.y..CLIP.y + CLIP.height {
            for x in CLIP.x..CLIP.x + CLIP.width {
                if white(pixel(&pixels, x, y)) {
                    wide = wide.max(x.abs_diff(cx));
                    tall = tall.max(y.abs_diff(cy));
                }
            }
        }
        Some((wide, tall))
    };
    let mut top = Camera::default();
    top.look_from(View::Top);
    let mut front = Camera::default();
    front.look_from(View::Front);
    let (Some(top), Some(front)) = (extent(&top), extent(&front)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert!(
        (9..=12).contains(&top.0) && (9..=12).contains(&top.1),
        "{top:?}"
    );
    assert!((9..=12).contains(&front.0) && front.1 <= 4, "{front:?}");
}

#[test]
fn pivot_marker_is_a_ring_facing_the_screen() {
    // From the front, where the origin's ring is a line, the pivot's is
    // round, in its colour; it isn't drawn at no opacity, nor on the
    // origin, where the origin's marker is.
    let teal = |p: [u8; 4]| p[0] < 60 && p[1] > 90 && p[2] > 110 && p[1] < p[2];
    let mut camera = Camera::default();
    camera.look_from(View::Front);
    let at = Vec3::new(2.5, 0.0, 2.0);
    let (cx, cy) = on_screen(&camera, at);
    let extent = |pivot| {
        let extras = Extras {
            pivot,
            ..Extras::default()
        };
        let pixels = render_with(&camera, &RenderMesh::default(), extras)?;
        let (mut wide, mut tall, mut any) = (0u32, 0u32, false);
        for y in CLIP.y..CLIP.y + CLIP.height {
            for x in CLIP.x..CLIP.x + CLIP.width {
                if teal(pixel(&pixels, x, y)) {
                    wide = wide.max(x.abs_diff(cx));
                    tall = tall.max(y.abs_diff(cy));
                    any = true;
                }
            }
        }
        Some(any.then_some((wide, tall)))
    };
    let pivot = |at, opacity| Some(Pivot { at, opacity });
    let Some(shown) = extent(pivot(at, 1.0)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let shown = shown.expect("the pivot's marker isn't drawn");
    assert!(
        (9..=12).contains(&shown.0) && (9..=12).contains(&shown.1),
        "{shown:?}"
    );
    assert_eq!(extent(pivot(at, 0.0)), Some(None));
    assert_eq!(extent(pivot(Vec3::ZERO, 1.0)), Some(None));
    assert_eq!(extent(None), Some(None));
}

#[test]
fn looks_the_same_on_srgb_and_linear_targets() {
    // A face of a cube filling the centre of the viewport, which the grid
    // is hidden behind. Partly covered grid lines and edges may differ, since the
    // linear target blends encoded values.
    let mesh = cube(2.0, Vec3::ZERO);
    let mut camera = Camera::default();
    camera.set_target(Vec3::ONE);
    camera.look_from(View::Front);
    let render = |format| render_to(format, &camera, &mesh, VIEWPORT, CLIP, 1.0);
    let (Some(unorm), Some(srgb)) = (
        render(wgpu::TextureFormat::Rgba8Unorm),
        render(wgpu::TextureFormat::Rgba8UnormSrgb),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    let (cx, cy) = (x0 + w / 2, y0 + h / 2);
    for (x, y) in (cx - 4..=cx + 4).flat_map(|x| (cy - 4..=cy + 4).map(move |y| (x, y))) {
        let (a, b) = (pixel(&unorm, x, y), pixel(&srgb, x, y));
        let diff = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max();
        assert!(diff <= Some(1), "pixel ({x}, {y}) is {a:?} and {b:?}");
    }
    let lit = pixel(&srgb, cx, cy);
    assert!(lit[..3].iter().all(|&c| c > 64), "centre is {lit:?}");
}

/// Whether a pixel is mostly [`COLORS`]' sketch yellow, which nothing else
/// in the scene is.
fn yellow([r, g, b, _]: [u8; 4]) -> bool {
    r > 150 && g > 150 && b < 100
}

/// The centre of [`VIEWPORT`], in whole pixels.
const CENTER: (u32, u32) = (CLIP.x + CLIP.width / 2, CLIP.y + CLIP.height / 2);

/// Where `world` is drawn in [`VIEWPORT`] by `camera`, orthographic.
fn on_screen(camera: &Camera, world: Vec3) -> (u32, u32) {
    let pixels = VIEWPORT.height / camera.view_height();
    let offset = world - camera.target();
    let x = VIEWPORT.x + VIEWPORT.width / 2.0 + offset.dot(camera.right()) * pixels;
    let y = VIEWPORT.y + VIEWPORT.height / 2.0 - offset.dot(camera.up()) * pixels;
    (x as u32, y as u32)
}

/// Whether any pixel of column `x` within [`CLIP`] is yellow.
fn yellow_in_column(pixels: &[[u8; 4]], x: u32) -> bool {
    (CLIP.y..CLIP.y + CLIP.height).any(|y| yellow(pixel(pixels, x, y)))
}

/// Lines from `a` to `b`, one segment each.
fn lines(segments: &[(Vec3, Vec3)]) -> RenderLines {
    let mut lines = RenderLines::default();
    for &(a, b) in segments {
        lines.push([a, b]).unwrap();
    }
    lines
}

/// A box `size` on a side from `at`, tessellated. Built at the origin and
/// moved, so it can reach past [`varde_kernel::MAX_COORD`], which a
/// solid can't.
fn cube(size: f32, at: Vec3) -> RenderMesh {
    let solid = Solid::cuboid(
        DVec3::ZERO,
        DVec3::splat(f64::from(size)),
        0,
        &Tolerance::DEFAULT,
    );
    let mesh = solid.unwrap().tessellate(&varde_kernel::Display::default());
    let mut moved = RenderMesh::default();
    moved.append_at(&mesh.unwrap(), at).unwrap();
    moved
}

/// The triangles of `mesh` facing along `normal`, as a highlight's.
fn face(mesh: &RenderMesh, normal: Vec3, emphasis: Emphasis) -> Highlight {
    let mut highlight = Highlight::default();
    for triangle in mesh.indices().as_chunks::<3>().0 {
        let corner = |i: usize| Vec3::from(mesh.positions()[triangle[i] as usize]);
        let normals = [0, 1, 2].map(|i| Vec3::from(mesh.normals()[triangle[i] as usize]));
        if normals.iter().all(|n| n.dot(normal) > 0.9) {
            highlight.triangle(emphasis, [0, 1, 2].map(corner), normals);
        }
    }
    highlight
}

#[test]
fn highlight_shows_faces_and_edges_in_front_only() {
    // From the top, a cube 2 on a side from (2, -1, 0), clear of the
    // origin's marker: its top shows, its bottom doesn't, and a line
    // across either face likewise.
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    let mesh = cube(2.0, Vec3::new(2.0, -1.0, 0.0));
    let render = |highlight| {
        let extras = Extras {
            highlight,
            ..Extras::default()
        };
        render_with(&camera, &mesh, extras)
    };
    let Some(plain) = render(None) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (x, y) = on_screen(&camera, Vec3::new(3.0, -0.5, 2.0));
    let shade = pixel(&plain, x, y);
    assert!(shade[0].abs_diff(shade[1]) < 8, "{shade:?}");

    let top = render(Some(face(&mesh, Vec3::Z, Emphasis::Hovered))).unwrap();
    let [r, g, b, _] = pixel(&top, x, y);
    assert!(
        g > r.saturating_add(60) && g > b.saturating_add(60),
        "{r} {g} {b}"
    );
    let selected = render(Some(face(&mesh, Vec3::Z, Emphasis::Selected))).unwrap();
    let [r, g, b, _] = pixel(&selected, x, y);
    assert!(
        b > r.saturating_add(60) && b > g.saturating_add(60),
        "{r} {g} {b}"
    );
    // The model's edges still draw over it, where they did: dark pixels
    // around the cube, the background black.
    let (left, top_row) = on_screen(&camera, Vec3::new(1.5, 1.5, 2.0));
    let (right, bottom_row) = on_screen(&camera, Vec3::new(4.5, -1.5, 2.0));
    let edges = |pixels: &[[u8; 4]]| {
        let dark = |p: [u8; 4]| (15..80).contains(&p[0]) && p[1] < 80 && p[2] < 80;
        (left..right)
            .flat_map(|x| (top_row..bottom_row).map(move |y| (x, y)))
            .filter(|&(x, y)| dark(pixel(pixels, x, y)))
            .collect::<Vec<_>>()
    };
    assert!(!edges(&plain).is_empty());
    assert_eq!(edges(&top), edges(&plain));
    let bottom = render(Some(face(&mesh, -Vec3::Z, Emphasis::Hovered))).unwrap();
    assert_eq!(pixel(&bottom, x, y), shade);

    for (z, shows) in [(2.0, true), (0.0, false)] {
        let mut highlight = Highlight::default();
        let line = vec![Vec3::new(2.0, 0.0, z), Vec3::new(4.0, 0.0, z)];
        highlight.edge(Emphasis::Hovered, line);
        let pixels = render(Some(highlight)).unwrap();
        let (x, y) = on_screen(&camera, Vec3::new(3.0, 0.0, z));
        let [r, g, b, _] = pixel(&pixels, x, y);
        let red = r > g.saturating_add(100) && r > b.saturating_add(100);
        assert_eq!(red, shows, "z {z}: {r} {g} {b}");
        if !shows {
            assert_eq!(pixel(&pixels, x, y), pixel(&plain, x, y));
        }
    }
}

#[test]
fn sketches_are_hidden_by_bodies_in_front_of_them() {
    // From the top, across a cube from 0 to 2: beneath it, on its top face
    // and above it. Looked at away from the origin marker's ring, which is
    // drawn over them.
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    let mesh = cube(2.0, Vec3::ZERO);
    for (z, hidden) in [(-1.0, true), (2.0, false), (3.0, false)] {
        let (from, to) = (Vec3::new(-3.0, 1.0, z), Vec3::new(5.0, 1.0, z));
        let extras = Extras {
            sketches: lines(&[(from, to)]),
            ..Extras::default()
        };
        let Some(pixels) = render_with(&camera, &mesh, extras) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        for x in [-2.0, -1.5, 1.0, 2.5, 4.0] {
            let over_cube = (0.0..=2.0).contains(&x);
            let (column, _) = on_screen(&camera, Vec3::new(x, 1.0, z));
            assert_eq!(
                yellow_in_column(&pixels, column),
                !(hidden && over_cube),
                "z {z}, x {x}"
            );
        }
    }
}

#[test]
fn sketch_lines_keep_their_width_on_screen() {
    // Across the bottom half from the top, measured in a column left of
    // the origin, clear of the grid's axes and the origin marker.
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    let [width, height] = SIZE;
    let measure = |scale: f32, zoom: f32| {
        let mut camera = camera;
        camera.zoom(zoom);
        let y = -0.2 * camera.view_height();
        let extras = Extras {
            sketches: lines(&[(Vec3::new(-100.0, y, 0.0), Vec3::new(100.0, y, 0.0))]),
            ..Extras::default()
        };
        let size = [width as f32 / 2.0 * scale, height as f32 / 2.0 * scale];
        let viewport = Viewport {
            x: 0.0,
            y: 0.0,
            width: size[0],
            height: size[1],
        };
        let clip = ClipRect {
            x: 0,
            y: 0,
            width: size[0] as u32,
            height: size[1] as u32,
        };
        let pixels = render_scaled(
            &camera,
            &RenderMesh::default(),
            extras,
            viewport,
            clip,
            scale,
        )?;
        // How much of the column the line covers, from how much redder
        // than blue it is, which grid lines and the background aren't.
        let column = clip.width / 4;
        let covered: f32 = (clip.height / 2 + 4..clip.height)
            .map(|y| {
                let [r, _, b, _] = pixel(&pixels, column, y);
                f32::from(r.saturating_sub(b)) / 255.0
            })
            .sum();
        Some(covered)
    };
    let (Some(one), Some(two), Some(zoomed)) =
        (measure(1.0, 1.0), measure(2.0, 1.0), measure(1.0, 0.01))
    else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert!(
        (LINE_WIDTH * 0.8..LINE_WIDTH * 1.3).contains(&one),
        "{one} pixels wide"
    );
    assert!((1.8..2.2).contains(&(two / one)), "grew {}x", two / one);
    // The same however far away.
    assert!((zoomed - one).abs() < 0.2, "{zoomed} zoomed in, {one} not");
}

#[test]
fn sketch_lines_crossing_the_near_plane_are_cut_there() {
    // In perspective from the front, a line running from in front of the
    // target to behind the eye, up and right of it: it heads for the top
    // right corner, and nothing of it may come round behind the eye to
    // the bottom left.
    let mut camera = Camera::default();
    camera.set_projection(Projection::Perspective);
    camera.look_from(View::Front);
    let extras = Extras {
        sketches: lines(&[(Vec3::new(1.0, 5.0, 1.0), Vec3::new(1.0, -30.0, 1.0))]),
        ..Extras::default()
    };
    let Some(pixels) = render_with(&camera, &RenderMesh::default(), extras) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let mut drawn = 0;
    for y in CLIP.y..CLIP.y + CLIP.height {
        for x in CLIP.x..CLIP.x + CLIP.width {
            if yellow(pixel(&pixels, x, y)) {
                assert!(x >= cx && y <= cy, "line at ({x}, {y})");
                drawn += 1;
            }
        }
    }
    assert!(drawn > 10, "only {drawn} pixels of line");
}

#[test]
fn faded_model_shows_only_its_nearest_faces_faintly() {
    // From the front, the centre of a cube's front face, with another cube
    // behind it drawn first.
    let mut camera = Camera::default();
    camera.set_target(Vec3::ONE);
    camera.look_from(View::Front);
    let near = cube(2.0, Vec3::ZERO);
    let mut both = cube(2.0, Vec3::Y * 5.0);
    both.append_at(&near, Vec3::ZERO).unwrap();
    let faded = || Extras {
        faded: true,
        ..Extras::default()
    };
    let (Some(opaque), Some(faint), Some(faint_both)) = (
        render_with(&camera, &near, Extras::default()),
        render_with(&camera, &near, faded()),
        render_with(&camera, &both, faded()),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let lit = pixel(&opaque, cx, cy);
    let dim = pixel(&faint, cx, cy);
    // Over the black background.
    for (lit, dim) in lit[..3].iter().zip(&dim[..3]) {
        let expected = f32::from(*lit) * COLORS.faded_alpha;
        assert!((f32::from(*dim) - expected).abs() <= 1.5, "{dim} for {lit}");
    }
    assert_eq!(pixel(&faint_both, cx, cy), dim);
}

#[test]
fn faded_model_still_hides_sketches_behind_it() {
    let mut camera = Camera::default();
    camera.set_target(Vec3::ONE);
    camera.look_from(View::Front);
    let behind = || Extras {
        sketches: lines(&[(Vec3::new(-1.0, 3.0, 1.0), Vec3::new(3.0, 3.0, 1.0))]),
        faded: true,
        ..Extras::default()
    };
    let (Some(alone), Some(hidden)) = (
        render_with(&camera, &RenderMesh::default(), behind()),
        render_with(&camera, &cube(2.0, Vec3::ZERO), behind()),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, _) = CENTER;
    assert!(yellow_in_column(&alone, cx));
    assert!(!yellow_in_column(&hidden, cx));
}

#[test]
fn grid_is_drawn_on_its_plane() {
    // From the front, the XZ plane shows its X axis left of the origin and
    // its Z axis below it, clear of the origin marker. The XY plane is seen
    // edge on: its grid doesn't show, but its X axis does, on and on as
    // an axis line, and there's no Z axis.
    let mut camera = Camera::default();
    camera.look_from(View::Front);
    let xz = GridPlane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
    let with_grid = |grid| Extras {
        grid,
        ..Extras::default()
    };
    let (Some(on_xz), Some(on_xy)) = (
        render_with(&camera, &RenderMesh::default(), with_grid(xz)),
        render_with(&camera, &RenderMesh::default(), with_grid(GridPlane::XY)),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let red: fn([u8; 4]) -> bool = |[r, g, _, _]| r > 150 && g < 100;
    let blue: fn([u8; 4]) -> bool = |[r, _, b, _]| b > 150 && b > r + 80;
    let (cx, cy) = CENTER;
    let x_axis = (cy - 1..=cy + 1).map(|y| pixel(&on_xz, cx - 30, y));
    let z_axis = (cx - 1..=cx + 1).map(|x| pixel(&on_xz, x, cy + 20));
    assert!(x_axis.clone().any(red), "{:?}", x_axis.collect::<Vec<_>>());
    assert!(z_axis.clone().any(blue), "{:?}", z_axis.collect::<Vec<_>>());
    assert!((cy - 1..=cy + 1).any(|y| red(pixel(&on_xy, cx - 30, y))));
    assert!(!(cx - 1..=cx + 1).any(|x| blue(pixel(&on_xy, x, cy + 20))));
}

/// Looking straight down at the XY plane with the origin in the middle,
/// 12.8 units across the view's height: a unit is 10 logical pixels in
/// [`SKETCH_VIEW`].
fn top_camera() -> Camera {
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    camera.zoom(12.8 / camera.view_height());
    camera
}

/// The logical size of the viewport the sketch pass is tested in, at the
/// target's top left: the whole target at a scale factor of 2.
const SKETCH_VIEW: [f32; 2] = [256.0, 128.0];

/// Draws `extras` from `camera` over `mesh` into [`SKETCH_VIEW`] at
/// `scale`, and returns the pixels.
fn render_sketch(
    camera: &Camera,
    mesh: &RenderMesh,
    extras: Extras,
    scale: f32,
) -> Option<Vec<[u8; 4]>> {
    let [width, height] = SKETCH_VIEW.map(|s| s * scale);
    let viewport = Viewport {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let clip = ClipRect {
        x: 0,
        y: 0,
        width: width as u32,
        height: height as u32,
    };
    render_scaled(camera, mesh, extras, viewport, clip, scale)
}

/// `layer` of a sketch on the XY plane, drawn as its base layer.
fn sketched(layer: SketchLayer) -> Extras {
    Extras {
        sketch: Some((GridPlane::XY, layer)),
        ..Extras::default()
    }
}

/// Where the sketch point `x`, `y` shows through [`top_camera`], in
/// physical pixels at `scale`.
fn sketch_pixel(x: f32, y: f32, scale: f32) -> (f32, f32) {
    let [width, height] = SKETCH_VIEW;
    (
        (width / 2.0 + x * 10.0) * scale,
        (height / 2.0 - y * 10.0) * scale,
    )
}

const YELLOW: Srgba = Srgba([1.0, 1.0, 0.0, 1.0]);
const RED: Srgba = Srgba([1.0, 0.0, 0.0, 1.0]);
const BLUE: Srgba = Srgba([0.0, 0.0, 1.0, 1.0]);
const GREEN: Srgba = Srgba([0.0, 1.0, 0.0, 1.0]);

/// How much of a pixel yellow covers, over the black background: as much
/// as its red and green are above its blue, which the grid's grey isn't.
fn yellowness([r, g, b, _]: [u8; 4]) -> f32 {
    f32::from(r.min(g).saturating_sub(b)) / 255.0
}

/// A layer with a line in `style` along the sketch's `y`, from `x` to `-x`.
fn line_layer(y: f32, x: f32, style: LineStyle) -> SketchLayer {
    let mut layer = SketchLayer::default();
    let (x, y) = (f64::from(x), f64::from(y));
    let points = [glam::DVec2::new(-x, y), glam::DVec2::new(x, y)];
    layer.polyline(Space::Sketch, &points, style);
    layer
}

/// How much of column `x` yellow covers, in pixels.
fn yellow_down(pixels: &[[u8; 4]], x: u32, rows: std::ops::Range<u32>) -> f32 {
    rows.map(|y| yellowness(pixel(pixels, x, y))).sum()
}

#[test]
fn sketch_lines_keep_their_width_at_any_zoom_and_scale() {
    let style = LineStyle {
        color: YELLOW,
        width: 3.0,
        dash: None,
    };
    let measure = |scale: f32, zoom: f32, projection| {
        let mut camera = top_camera();
        camera.set_projection(projection);
        camera.zoom(zoom);
        // A quarter of the view below the middle, measured left of it,
        // clear of the grid's axes and the origin marker.
        let height = camera.view_height();
        let layer = line_layer(-0.25 * height, 100.0 * height, style);
        let pixels = render_sketch(&camera, &RenderMesh::default(), sketched(layer), scale)?;
        let [width, rows] = SKETCH_VIEW.map(|s| (s * scale) as u32);
        Some(yellow_down(&pixels, width / 4, rows / 2 + 4..rows))
    };
    use Projection::{Orthographic, Perspective};
    let (Some(one), Some(two), Some(zoomed), Some(perspective)) = (
        measure(1.0, 1.0, Orthographic),
        measure(2.0, 1.0, Orthographic),
        measure(1.0, 1e-3, Orthographic),
        measure(1.0, 50.0, Perspective),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert!((2.8..3.2).contains(&one), "{one} pixels wide");
    assert!((1.9..2.1).contains(&(two / one)), "grew {}x", two / one);
    assert!((zoomed - one).abs() < 0.1, "{zoomed} zoomed in, {one} not");
    assert!(
        (perspective - one).abs() < 0.1,
        "{perspective} in perspective"
    );
}

#[test]
fn sketch_lines_are_anti_aliased_at_their_edges() {
    // A pixel wide, on the boundary between two rows: half of each.
    let style = LineStyle {
        color: YELLOW,
        width: 1.0,
        dash: None,
    };
    let (_, row) = sketch_pixel(0.0, -3.3, 1.0);
    let Some(pixels) = render_sketch(
        &top_camera(),
        &RenderMesh::default(),
        sketched(line_layer(-3.3, 100.0, style)),
        1.0,
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let row = row.round() as u32;
    for y in [row - 1, row] {
        let covered = yellowness(pixel(&pixels, 40, y));
        assert!(
            (0.4..0.6).contains(&covered),
            "row {y} is {covered} covered"
        );
    }
    for y in [row - 2, row + 1] {
        assert_eq!(yellowness(pixel(&pixels, 40, y)), 0.0, "row {y}");
    }
}

/// How much of each pixel of row `y` from `x` yellow covers.
fn yellow_along(pixels: &[[u8; 4]], y: u32, x: std::ops::Range<u32>) -> Vec<f32> {
    x.map(|x| yellowness(pixel(pixels, x, y))).collect()
}

#[test]
fn dashes_run_on_along_a_polyline() {
    // Six pixels on and four off, two wide on the middle of a row.
    let style = LineStyle {
        color: YELLOW,
        width: 2.0,
        dash: Some([6.0, 4.0]),
    };
    let y = -3.35;
    let (_, row) = sketch_pixel(0.0, y, 1.0);
    let row = row as u32;
    // In one segment, and in many of a third of a dash.
    let mut chained = SketchLayer::default();
    let points: Vec<_> = (0..=120)
        .map(|i| glam::DVec2::new(-12.0 + 0.2 * f64::from(i), y.into()))
        .collect();
    chained.polyline(Space::Sketch, &points, style);
    let straight = line_layer(y, 12.0, style);
    let camera = top_camera();
    let mesh = RenderMesh::default();
    let (Some(straight), Some(chained), Some(live)) = (
        render_sketch(&camera, &mesh, sketched(straight.clone()), 1.0),
        render_sketch(&camera, &mesh, sketched(chained), 1.0),
        render_sketch(
            &camera,
            &mesh,
            Extras {
                live: true,
                ..sketched(straight)
            },
            1.0,
        ),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let span = 20..230;
    let along = yellow_along(&straight, row, span.clone());
    // Six in ten covered, repeating every ten pixels.
    let lit = along.iter().sum::<f32>() / along.len() as f32;
    assert!((0.55..0.65).contains(&lit), "{lit} covered");
    for (x, pair) in along.iter().zip(&along[10..]).enumerate() {
        assert!((pair.0 - pair.1).abs() < 0.1, "{x}: {along:?}");
    }
    assert!(along.iter().any(|&c| c > 0.95) && along.iter().any(|&c| c < 0.05));
    // The same through many segments, and drawn as the live layer.
    let chained = yellow_along(&chained, row, span.clone());
    for (x, (a, b)) in along.iter().zip(&chained).enumerate() {
        assert!((a - b).abs() < 0.1, "{x}: {a} and {b}");
    }
    assert!(live == straight, "the live layer draws differently");
}

#[test]
fn sketch_lines_crossing_the_near_plane_are_cut_there_too() {
    // As `sketch_lines_crossing_the_near_plane_are_cut_there`, in the
    // sketch being edited, on a plane a unit above XY.
    let mut camera = Camera::default();
    camera.set_projection(Projection::Perspective);
    camera.look_from(View::Front);
    let plane = GridPlane::new(Vec3::Z, Vec3::X, Vec3::Y).unwrap();
    let mut layer = SketchLayer::default();
    let points = [glam::DVec2::new(1.0, 5.0), glam::DVec2::new(1.0, -30.0)];
    let style = LineStyle {
        color: YELLOW,
        width: 2.0,
        dash: None,
    };
    layer.polyline(Space::Sketch, &points, style);
    let extras = Extras {
        sketch: Some((plane, layer)),
        ..Extras::default()
    };
    let Some(pixels) = render_with(&camera, &RenderMesh::default(), extras) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let mut drawn = 0;
    for y in CLIP.y..CLIP.y + CLIP.height {
        for x in CLIP.x..CLIP.x + CLIP.width {
            if yellow(pixel(&pixels, x, y)) {
                assert!(x >= cx && y <= cy, "line at ({x}, {y})");
                drawn += 1;
            }
        }
    }
    assert!(drawn > 10, "only {drawn} pixels of line");
}

#[test]
fn a_point_is_a_smooth_disc_with_a_rim() {
    let style = PointStyle {
        radius: 6.0,
        rim_width: 2.0,
        rim: RED,
        fill: BLUE,
        fixed: false,
    };
    // Rim and fill, and a fixed point: a red disc.
    let mut layer = SketchLayer::default();
    layer.point(glam::DVec2::new(5.0, 3.0), style);
    layer.point(
        glam::DVec2::new(-5.0, 3.0),
        PointStyle {
            fixed: true,
            ..style
        },
    );
    for scale in [1.0, 2.0] {
        let Some(pixels) = render_sketch(
            &top_camera(),
            &RenderMesh::default(),
            sketched(layer.clone()),
            scale,
        ) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let pixels = &pixels;
        let near = |x: f32, y: f32| {
            let (cx, cy) = sketch_pixel(x, y, scale);
            move |dx: f32, dy: f32| pixel(pixels, (cx + dx) as u32, (cy + dy) as u32)
        };
        let at = near(5.0, 3.0);
        // Blue inside, red on the rim.
        let [r, _, b, _] = at(0.0, 0.0);
        assert!(b > 240 && r < 10, "centre {:?}", at(0.0, 0.0));
        let [r, _, b, _] = at(5.0 * scale, 0.0);
        assert!(r > 200 && b < 50, "rim {:?}", at(5.0 * scale, 0.0));
        // Red, and not the grid's grey.
        let red = |[r, g, _, _]: [u8; 4]| f32::from(r.saturating_sub(g)) / 255.0;
        let fixed = near(-5.0, 3.0);
        assert!(red(fixed(0.0, 0.0)) > 0.95, "{:?}", fixed(0.0, 0.0));
        assert!(red(fixed(0.0, 7.0 * scale)) < 0.05);
        // Covering a disc, partly at its edge.
        let (mut covered, mut partly) = (0.0, 0);
        let reach = (8.0 * scale) as i32;
        for dy in -reach..reach {
            for dx in -reach..reach {
                let coverage = red(fixed(dx as f32, dy as f32));
                covered += coverage;
                partly += usize::from((0.1..0.9).contains(&coverage));
            }
        }
        let area = std::f32::consts::PI * (6.0 * scale).powi(2);
        assert!((covered - area).abs() < area * 0.03, "{covered} of {area}");
        assert!(partly > 8, "only {partly} pixels partly covered");
    }
}

#[test]
fn a_fill_leaves_its_holes_empty() {
    let square = |min: f64, max: f64| {
        [(min, min), (max, min), (max, max), (min, max)].map(|(x, y)| glam::DVec2::new(x, y))
    };
    let (outer, hole) = (square(-5.0, 3.0), square(-3.0, 1.0));
    let mut layer = SketchLayer::default();
    layer.fill(Space::Sketch, [&outer[..], &hole[..]], GREEN);
    // The same in screen space, 20 logical pixels square at the top left.
    let corners = [(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)];
    let screen = corners.map(|(x, y)| glam::DVec2::new(x, y));
    layer.fill(Space::Screen, [&screen[..]], GREEN);
    for scale in [1.0, 2.0] {
        let Some(pixels) = render_sketch(
            &top_camera(),
            &RenderMesh::default(),
            sketched(layer.clone()),
            scale,
        ) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let green = |(x, y): (f32, f32)| {
            let [r, g, b, _] = pixel(&pixels, x as u32, y as u32);
            g > 200 && r < 50 && b < 50
        };
        assert!(green(sketch_pixel(-4.5, -1.5, scale)), "in the ring");
        assert!(green(sketch_pixel(2.5, 2.5, scale)), "in the ring");
        assert!(!green(sketch_pixel(-1.5, -1.5, scale)), "in the hole");
        assert!(!green(sketch_pixel(-5.5, -1.5, scale)), "outside");
        assert!(green((20.0 * scale, 20.0 * scale)), "in the screen's");
        assert!(!green((35.0 * scale, 20.0 * scale)), "right of it");
    }
}

#[test]
fn a_triangle_is_filled_like_an_arrowhead() {
    // An arrowhead in screen space pointing left, its tip at (10, 20), and
    // one in sketch space.
    let corners = [(10.0, 20.0), (40.0, 10.0), (40.0, 30.0)];
    let mut layer = SketchLayer::default();
    layer.triangle(
        Space::Screen,
        corners.map(|(x, y)| glam::DVec2::new(x, y)),
        GREEN,
    );
    let corners = [(-2.0, -2.0), (2.0, -2.0), (0.0, 2.0)];
    layer.triangle(
        Space::Sketch,
        corners.map(|(x, y)| glam::DVec2::new(x, y)),
        GREEN,
    );
    for scale in [1.0, 2.0] {
        let Some(pixels) = render_sketch(
            &top_camera(),
            &RenderMesh::default(),
            sketched(layer.clone()),
            scale,
        ) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let green = |(x, y): (f32, f32)| {
            let [r, g, b, _] = pixel(&pixels, x as u32, y as u32);
            g > 200 && r < 50 && b < 50
        };
        assert!(green((35.0 * scale, 20.0 * scale)), "inside");
        assert!(green((15.0 * scale, 20.0 * scale)), "near the tip");
        assert!(!green((15.0 * scale, 12.0 * scale)), "beside the tip");
        assert!(!green((45.0 * scale, 20.0 * scale)), "behind it");
        assert!(green(sketch_pixel(0.0, 0.0, scale)), "in the sketch's");
        assert!(!green(sketch_pixel(1.8, 1.8, scale)), "beside the sketch's");
    }
}

#[test]
fn the_sketch_is_drawn_over_the_faded_model() {
    // From the top, across a cube standing on the sketch's plane: its top
    // is in front of the sketch, which shows over it anyway.
    let camera = top_camera();
    let style = LineStyle {
        color: YELLOW,
        width: 2.0,
        dash: None,
    };
    let extras = Extras {
        faded: true,
        ..sketched(line_layer(1.0, 5.0, style))
    };
    let Some(pixels) = render_sketch(&camera, &cube(2.0, Vec3::ZERO), extras, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let rows = 0..SKETCH_VIEW[1] as u32;
    for x in [-3.0, 0.5, 1.5, 3.0] {
        let (column, _) = sketch_pixel(x, 0.0, 1.0);
        let covered = yellow_down(&pixels, column as u32, rows.clone());
        assert!(covered > 1.5, "at {x}: {covered}");
    }
}

#[test]
fn screen_space_lines_show_in_perspective_from_afar() {
    // Far enough that the near plane is more than a unit in front of the
    // eye, which the screen's coordinates mustn't be cut by.
    let mut camera = top_camera();
    camera.set_projection(Projection::Perspective);
    camera.zoom(1000.0);
    assert!(camera.near() > 1.0);
    let mut layer = SketchLayer::default();
    let points = [(10.0, 60.0), (240.0, 60.0)].map(glam::DVec2::from);
    let style = LineStyle {
        color: YELLOW,
        width: 2.0,
        dash: None,
    };
    layer.polyline(Space::Screen, &points, style);
    let Some(pixels) = render_sketch(&camera, &RenderMesh::default(), sketched(layer), 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let covered = yellow_down(&pixels, 40, 50..70);
    assert!((1.8..2.2).contains(&covered), "{covered} pixels wide");
}

/// The whole target as a viewport, at a scale factor of 1.
const FULL: Viewport = Viewport {
    x: 0.0,
    y: 0.0,
    width: SIZE[0] as f32,
    height: SIZE[1] as f32,
};
const FULL_CLIP: ClipRect = ClipRect {
    x: 0,
    y: 0,
    width: SIZE[0],
    height: SIZE[1],
};

/// Where `world` shows in [`FULL`] from `camera`, in perspective, in
/// pixels.
fn in_perspective(camera: &Camera, world: Vec3) -> glam::Vec2 {
    let offset = world - camera.eye();
    let depth = -offset.dot(camera.backward());
    // The view height at the target, over the distance there.
    let slope = camera.view_height() / camera.distance();
    let pixels = FULL.height / slope / depth;
    glam::Vec2::new(
        FULL.width / 2.0 + offset.dot(camera.right()) * pixels,
        FULL.height / 2.0 - offset.dot(camera.up()) * pixels,
    )
}

/// How red a pixel is over the black background: 1 where the X axis
/// covers it, 0 where nothing or only the grey grid does.
fn redness([r, g, _, _]: [u8; 4]) -> f32 {
    (f32::from(r) - f32::from(g)).max(0.0) / (217.0 - 64.0)
}

#[test]
fn grid_axes_run_on_unfaded_far_past_the_grid() {
    // The -X half of the axis runs away from the camera, turned to look
    // nearly along it, towards the horizon, far past where the grid has
    // faded out.
    let mut camera = Camera::default();
    camera.set_projection(Projection::Perspective);
    camera.orbit(0.7, -0.45);
    let Some(pixels) = render(&camera, &RenderMesh::default(), FULL, FULL_CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    for heights in [2.0, 8.0, 20.0, 60.0] {
        let at = in_perspective(&camera, Vec3::X * -heights * camera.view_height());
        let (x, y) = (at.x as u32, at.y as u32);
        assert!(x > 2 && x < SIZE[0] - 2 && y > 2 && y < SIZE[1] - 2, "{at}");
        let most = (y - 2..=y + 2)
            .flat_map(|y| (x - 2..=x + 2).map(move |x| (x, y)))
            .map(|(x, y)| redness(pixel(&pixels, x, y)))
            .fold(0.0, f32::max);
        assert!(
            most > 0.9,
            "{heights} view heights out the axis is {most} red"
        );
    }
}

#[test]
fn grid_axes_are_anti_aliased() {
    // From the top, turned so the X axis crosses the view at a slant: each
    // column has a pixel of it at full strength, and its edges are partly
    // covered, a line about AXIS_WIDTH wide across.
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    camera.orbit(0.3, 0.0);
    let Some(pixels) = render(&camera, &RenderMesh::default(), FULL, FULL_CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mut partial = 0;
    // Clear of the origin marker in the middle.
    for x in (8..SIZE[0] / 2 - 24).chain(SIZE[0] / 2 + 24..SIZE[0] - 8) {
        let column: Vec<f32> = (0..SIZE[1])
            .map(|y| redness(pixel(&pixels, x, y)))
            .collect();
        let most = column.iter().copied().fold(0.0, f32::max);
        let sum: f32 = column.iter().sum();
        assert!(most > 0.8, "column {x}: {most}");
        // 1.75 pixels across the line, a little more down a column.
        assert!((1.4..2.6).contains(&sum), "column {x}: {sum}");
        partial += column.iter().filter(|&&r| (0.1..0.9).contains(&r)).count();
    }
    assert!(
        partial > SIZE[0] as usize / 2,
        "only {partial} partly covered pixels"
    );
}

/// Whether a pixel is mostly `color`'s pure red, green or blue: index 0,
/// 1 or 2.
fn mostly(pixel: [u8; 4], color: usize) -> bool {
    let others = (0..3).filter(|&i| i != color).map(|i| pixel[i]);
    pixel[color] > 150 && others.max().unwrap_or(0) < 100
}

/// A square of `size` around `center` on its plane.
fn square(center: (f64, f64), size: f64) -> [glam::DVec2; 4] {
    let (x, y, h) = (center.0, center.1, size / 2.0);
    [(-h, -h), (h, -h), (h, h), (-h, h)].map(|(dx, dy)| glam::DVec2::new(x + dx, y + dy))
}

/// Where `world` shows through `camera` in [`SKETCH_VIEW`] at a scale
/// factor of 1, in whole pixels.
fn sketch_view_pixel(camera: &Camera, world: Vec3) -> (u32, u32) {
    let [width, height] = SKETCH_VIEW;
    let from = world - camera.eye();
    let depth = -from.dot(camera.backward());
    let scale = height / camera.view_height()
        * match camera.projection() {
            Projection::Perspective => camera.distance() / depth,
            Projection::Orthographic => 1.0,
        };
    let x = width / 2.0 + from.dot(camera.right()) * scale;
    let y = height / 2.0 - from.dot(camera.up()) * scale;
    (x as u32, y as u32)
}

#[test]
fn a_depth_tested_sketch_is_hidden_by_the_model_in_front_of_it() {
    // From the top, a cube from (0, 0, 0) to (2, 2, 2), with the black
    // background left of x = 0. A square, a line and a point across its
    // edge, on the sketch's plane or another, one at a time.
    let cube = cube(2.0, Vec3::ZERO);
    let plane = |z: f32| GridPlane::new(Vec3::new(0.0, 0.0, z), Vec3::X, Vec3::Y).unwrap();
    let style = LineStyle {
        color: YELLOW,
        width: 3.0,
        dash: None,
    };
    let point = PointStyle {
        radius: 4.0,
        rim_width: 1.0,
        rim: RED,
        fill: RED,
        fixed: false,
    };
    let at = glam::DVec2::new;
    type Item = (&'static str, fn([u8; 4]) -> bool);
    let items: [Item; 3] = [
        ("square", |p| mostly(p, 1)),
        ("line", yellow),
        ("point", |p| mostly(p, 0)),
    ];
    let mut perspective = top_camera();
    perspective.set_projection(Projection::Perspective);
    for camera in [top_camera(), perspective] {
        // On the plane of the cube's bottom, its middle, its top, above
        // it and beneath it.
        for (z, hidden) in [
            (0.0, true),
            (1.0, true),
            (2.0, false),
            (3.0, false),
            (-1.0, true),
        ] {
            for (other, depth_tested, live) in [
                (false, true, false),
                (false, true, true),
                (true, true, false),
                (false, false, false),
            ] {
                for (name, shows_in) in items {
                    // On the sketch's plane, or on another with the
                    // sketch's on XY.
                    let (space, sketch_plane) = if other {
                        (Space::On(plane(z)), GridPlane::XY)
                    } else {
                        (Space::Sketch, plane(z))
                    };
                    let mut layer = SketchLayer::default();
                    match name {
                        "square" => layer.fill(space, [&square((0.0, 1.0), 1.6)[..]], GREEN),
                        "line" => layer.polyline(space, &[at(-1.0, 1.0), at(1.0, 1.0)], style),
                        _ if other => continue,
                        _ => {
                            layer.point(at(-0.7, 1.0), point);
                            layer.point(at(0.7, 1.0), point);
                        }
                    }
                    let extras = Extras {
                        sketch: Some((sketch_plane, layer)),
                        depth_tested,
                        live,
                        ..Extras::default()
                    };
                    let Some(pixels) = render_sketch(&camera, &cube, extras, 1.0) else {
                        eprintln!("no GPU adapter, skipping");
                        return;
                    };
                    let on = |x: f32| {
                        let (x, y) = sketch_view_pixel(&camera, Vec3::new(x, 1.0, z));
                        shows_in(pixel(&pixels, x, y))
                    };
                    let case = format!(
                        "{:?} {name} at z {z} on another plane {other} tested {depth_tested} \
                         live {live}",
                        camera.projection()
                    );
                    // Off the cube it shows; over it, if it's behind it,
                    // only if it isn't depth tested.
                    assert!(on(-0.7), "{case}: off the cube");
                    assert_eq!(on(0.7), !(hidden && depth_tested), "{case}: over the cube");
                }
            }
        }
    }
}

#[test]
fn a_depth_tested_sketch_far_from_the_model_still_shows() {
    // Far below the grid and the cube, past the depth range the scene
    // would have without it.
    let mut camera = top_camera();
    let far = GridPlane::new(Vec3::new(0.0, 0.0, -500.0), Vec3::X, Vec3::Y).unwrap();
    // Not depth tested, it shows over the cube too.
    for (projection, depth_tested) in [
        (Projection::Orthographic, true),
        (Projection::Perspective, true),
        (Projection::Orthographic, false),
        (Projection::Perspective, false),
    ] {
        camera.set_projection(projection);
        let mut layer = SketchLayer::default();
        layer.fill(Space::On(far), [&square((0.0, 0.0), 4000.0)[..]], GREEN);
        let extras = Extras {
            sketch: Some((GridPlane::XY, layer)),
            depth_tested,
            ..Extras::default()
        };
        let Some(pixels) = render_sketch(&camera, &cube(2.0, Vec3::ZERO), extras, 1.0) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let case = format!("{projection:?} tested {depth_tested}");
        let (x, y) = sketch_pixel(-3.0, -3.0, 1.0);
        assert!(mostly(pixel(&pixels, x as u32, y as u32), 1), "{case}");
        let (x, y) = sketch_pixel(1.0, 1.0, 1.0);
        let over = mostly(pixel(&pixels, x as u32, y as u32), 1);
        assert_eq!(over, !depth_tested, "{case}");
    }
}

#[test]
fn a_depth_tested_line_inside_the_model_is_hidden_from_any_side() {
    // A line up the middle of a cube from (0, 0, 0) to (2, 2, 2), as an
    // extrude's shaft is, seen from above at an angle.
    let cube = cube(2.0, Vec3::ZERO);
    let axis = GridPlane::new(Vec3::new(1.0, 1.0, 0.0), Vec3::Z, Vec3::X).unwrap();
    let style = LineStyle {
        color: YELLOW,
        width: 3.0,
        dash: None,
    };
    let mut camera = top_camera();
    camera.set_target(Vec3::ONE);
    camera.orbit(0.6, -0.9);
    for projection in [Projection::Orthographic, Projection::Perspective] {
        camera.set_projection(projection);
        for depth_tested in [true, false] {
            let mut layer = SketchLayer::default();
            let ends = [glam::DVec2::new(0.2, 0.0), glam::DVec2::new(1.8, 0.0)];
            layer.polyline(Space::On(axis), &ends, style);
            let extras = Extras {
                sketch: Some((GridPlane::XY, layer)),
                depth_tested,
                live: true,
                ..Extras::default()
            };
            let Some(pixels) = render_sketch(&camera, &cube, extras, 1.0) else {
                eprintln!("no GPU adapter, skipping");
                return;
            };
            let shown = pixels.iter().filter(|&&p| yellow(p)).count();
            assert_eq!(shown > 0, !depth_tested, "{projection:?}: {shown} pixels");
        }
    }
}

#[test]
fn a_depth_tested_sketch_on_a_face_shows_at_any_scale_and_angle() {
    // A cube `size` on a side, framed, looked at from barely above its
    // top up to nearly straight down. A square on its top face, or on a
    // side face, shows where the face is seen, however steeply; one on a
    // plane through its middle never does.
    let mut failures = Vec::new();
    for size in [1e-2f32, 1.0, 1e3, 1e5] {
        let cube = cube(size, Vec3::ZERO);
        let s = f64::from(size);
        for elevation in [89.0f32, 80.0, 20.0, 3.0, 1.0] {
            for projection in [Projection::Orthographic, Projection::Perspective] {
                let mut camera = top_camera();
                camera.set_projection(projection);
                camera.set_target(Vec3::splat(size / 2.0));
                camera.zoom(size * 3.0 / camera.view_height());
                camera.orbit(0.4, elevation.to_radians() - Camera::PITCH_LIMIT);
                let plane = |origin: Vec3, x: Vec3, y: Vec3| GridPlane::new(origin, x, y).unwrap();
                // The faces facing +Z, +X and -X, and the middle.
                let planes = [
                    ("top", plane(Vec3::Z * size, Vec3::X, Vec3::Y), true),
                    ("+x side", plane(Vec3::X * size, Vec3::Y, Vec3::Z), true),
                    ("-x side", plane(Vec3::ZERO, Vec3::Z, Vec3::Y), true),
                    (
                        "middle",
                        plane(Vec3::Z * size / 2.0, Vec3::X, Vec3::Y),
                        false,
                    ),
                ];
                for (name, plane, face) in planes {
                    let center = plane.origin() + (plane.x() + plane.y()) * (size / 2.0);
                    // How steeply the face is seen at its centre: the sine
                    // of the angle between the face and the eye's ray.
                    let towards_eye = match projection {
                        Projection::Orthographic => camera.backward(),
                        Projection::Perspective => (camera.eye() - center).normalize(),
                    };
                    let steepness = towards_eye.dot(plane.normal());
                    if face && steepness.abs() < 0.01 {
                        continue;
                    }
                    let shows = face && steepness > 0.0;
                    let mut layer = SketchLayer::default();
                    let square = square((s / 2.0, s / 2.0), s * 0.8);
                    layer.fill(Space::On(plane), [&square[..]], GREEN);
                    let extras = Extras {
                        sketch: Some((GridPlane::XY, layer)),
                        depth_tested: true,
                        live: true,
                        ..Extras::default()
                    };
                    let Some(pixels) = render_sketch(&camera, &cube, extras, 1.0) else {
                        eprintln!("no GPU adapter, skipping");
                        return;
                    };
                    let (x, y) = sketch_view_pixel(&camera, center);
                    let p = pixel(&pixels, x, y);
                    if mostly(p, 1) != shows {
                        failures.push(format!(
                            "{name} of {size} at {elevation}° {projection:?}, seen at \
                             {steepness}: {p:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
