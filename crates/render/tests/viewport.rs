//! Renders into an offscreen texture and checks the scene stays inside its viewport.
// Holding the shared wgpu device in a `static` asks whether it's `Sync`
// deeper than the default limit.
#![recursion_limit = "256"]

use std::sync::Arc;

use glam::{DVec3, Vec3};
use varde_kernel::{MeshParts, RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::{
    CREASE_ALPHA, CREASE_WIDTH, Camera, ClipRect, Colors, EDGE_WIDTH, Frame, GridPlane,
    HIDDEN_DASH, HOVER_RIM, HOVERED_EDGE_WIDTH, Highlights, LINE_WIDTH, LineStyle, Pivot,
    PointStyle, Projection, Renderer, Shading, SketchLayer, SketchScene, Space, Srgb, Srgba,
    VERTEX_RADIUS, Vertex, View, Viewport, wgpu,
};
use varde_render::{OriginPart, OriginShown};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
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
    // Pure yellow, found by its lack of blue.
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
const SIZE: [u32; 2] = [512, 256];
const SENTINEL: [u8; 4] = [255, 0, 255, 255];

// The binary's first GPU test pays for the device and the shared renderer's
// pipelines (about 0.5-2s in a debug build, more for each further texture
// format): a floor shared by every test here, so over the 0.5s aim.
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
            opacity: &[],
            tints: &[],
            sketches: &Arc::default(),
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
            highlights: &Arc::default(),
            errors: &[],
            sketch: None,
            pivot: None,
            // The grid's axis lines, as before the Z axis was drawn.
            origin: OriginShown {
                axes: [true, true, false],
                ..OriginShown::DEFAULT
            },
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
    /// Whether the mesh's wires are drawn.
    wireframe: bool,
    /// Whether the edges of the mesh's triangles are drawn.
    tessellation: bool,
    shading: Shading,
    /// Whether the edges the model hides are drawn.
    hidden_edges: bool,
    /// The sketch being edited: its plane and a layer of it, drawn as its
    /// live layer if `live`, else as its base layer.
    sketch: Option<(GridPlane, SketchLayer)>,
    live: bool,
    /// Whether the sketch is hidden by the model in front of it.
    depth_tested: bool,
    pivot: Option<Pivot>,
    /// Colours other than [`COLORS`].
    colors: Option<Colors>,
    /// How opaque each part of the mesh is, opaque past its end.
    opacity: Vec<f32>,
    /// The colour of each part of the mesh, the model's past its end.
    tints: Vec<Option<varde_render::BodyTint>>,
    /// The faces hovered, the faces selected, those in the second colour,
    /// and the edges and vertices hovered and selected.
    hovered_faces: Vec<u32>,
    selected_faces: Vec<u32>,
    second_faces: Vec<u32>,
    /// Whether the hover is drawn over what hides it too.
    hover_through: bool,
    highlights: Highlights,
    /// What's drawn of the world's origin objects, if not the grid's axis
    /// lines and the marker.
    origin: Option<OriginShown>,
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
            opacity: &extras.opacity,
            tints: &extras.tints,
            sketches: &Arc::new(extras.sketches),
            grid: extras.grid,
            fade: if extras.faded { 1.0 } else { 0.0 },
            fading_grid: None,
            wireframe: extras.wireframe,
            tessellation: extras.tessellation,
            shading: extras.shading,
            hidden_edges: extras.hidden_edges,
            hovered_faces: &extras.hovered_faces,
            selected_faces: &extras.selected_faces,
            second_faces: &extras.second_faces,
            hover_through: extras.hover_through,
            highlights: &Arc::new(extras.highlights),
            errors: &[],
            sketch,
            pivot: extras.pivot,
            // The grid's axis lines, as before the Z axis was drawn.
            origin: extras.origin.unwrap_or(OriginShown {
                axes: [true, true, false],
                ..OriginShown::DEFAULT
            }),
            viewport,
            target_size: SIZE,
            scale_factor,
            colors: extras.colors.unwrap_or(COLORS),
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

/// Seen from the front, each origin object shows only while it's shown:
/// the Z axis's line, blue, up the middle; the X axis's, red, across it;
/// the marker, white, on the origin; and the XZ plane, faintly green,
/// around it.
#[test]
fn origin_objects_are_drawn_as_shown() {
    let mut camera = Camera::default();
    camera.look_from(View::Front);
    camera.orbit(0.0, 0.3);
    let count = |origin: OriginShown, matches: &dyn Fn([u8; 4]) -> bool| {
        let extras = Extras {
            origin: Some(origin),
            ..Extras::default()
        };
        let pixels = render_with(&camera, &RenderMesh::default(), extras)?;
        let mut count = 0;
        for y in CLIP.y..CLIP.y + CLIP.height {
            for x in CLIP.x..CLIP.x + CLIP.width {
                count += u32::from(matches(pixel(&pixels, x, y)));
            }
        }
        Some(count)
    };
    let blue = |[r, g, b, _]: [u8; 4]| b > 128 && r < 128 && g < 160;
    let red = |[r, g, _, _]: [u8; 4]| r > 128 && g < 128;
    let white = |[r, g, b, _]: [u8; 4]| r > 220 && g > 220 && b > 220;
    // Faint green on black: more green than either of the others.
    let green =
        |[r, g, b, _]: [u8; 4]| g > 10 && g > r.saturating_add(5) && g > b.saturating_add(5);
    let all = OriginShown {
        planes: [false, true, false],
        ..OriginShown::DEFAULT
    };
    let z = OriginShown {
        axes: [true; 3],
        ..OriginShown::DEFAULT
    };
    assert_eq!(OriginShown::DEFAULT.axes, [true, true, false]);
    let Some(shown) = count(z, &blue) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert!(shown > 50, "{shown} blue");
    let hidden = OriginShown {
        axes: [true, true, false],
        ..all
    };
    assert_eq!(count(hidden, &blue), Some(0));

    assert!(count(OriginShown::DEFAULT, &red).unwrap() > 50);
    let no_x = OriginShown {
        axes: [false, true, true],
        ..OriginShown::DEFAULT
    };
    assert_eq!(count(no_x, &red), Some(0));

    assert!(count(OriginShown::DEFAULT, &white).unwrap() > 5);
    let no_marker = OriginShown {
        marker: false,
        ..OriginShown::DEFAULT
    };
    assert_eq!(count(no_marker, &white), Some(0));

    let plane = count(all, &green).unwrap();
    let none = count(OriginShown::DEFAULT, &green).unwrap();
    assert!(
        plane > none + 60,
        "{plane} green with the plane, {none} without"
    );

    // Hovered, the plane is drawn though hidden, and stronger; the Z axis
    // too, and wider.
    let hovered = |part| OriginShown {
        hovered: Some(part),
        ..OriginShown::DEFAULT
    };
    let strong =
        |[r, g, b, _]: [u8; 4]| g > 40 && g > r.saturating_add(20) && g > b.saturating_add(20);
    let lit = count(hovered(OriginPart::Plane(1)), &strong).unwrap();
    let plain = count(all, &strong).unwrap();
    assert!(lit > plain + 30, "{lit} strong green hovered, {plain} not");
    let z = count(hovered(OriginPart::Axis(2)), &blue).unwrap();
    assert!(z > shown, "{z} blue hovered, {shown} shown");
}

#[test]
fn an_axis_pointing_at_the_camera_fades_out_alone() {
    // From the front the Y axis points at the camera: its line is gone,
    // nearly so too, while the X axis's stays. Tilted further, it's back.
    let red = |[r, g, _, _]: [u8; 4]| r > 128 && g < 128;
    let green = |[r, g, _, _]: [u8; 4]| g > 128 && r < 128;
    let counts = |pitch: f32, projection| {
        let mut camera = Camera::default();
        camera.set_projection(projection);
        camera.look_from(View::Front);
        camera.orbit(0.0, pitch);
        let pixels = render(&camera, &RenderMesh::default(), VIEWPORT, CLIP, 1.0)?;
        let (mut reds, mut greens) = (0, 0);
        for y in CLIP.y..CLIP.y + CLIP.height {
            for x in CLIP.x..CLIP.x + CLIP.width {
                let p = pixel(&pixels, x, y);
                reds += u32::from(red(p));
                greens += u32::from(green(p));
            }
        }
        Some((reds, greens))
    };
    for projection in [Projection::Orthographic, Projection::Perspective] {
        for pitch in [0.0, 0.02] {
            let Some((reds, greens)) = counts(pitch, projection) else {
                eprintln!("no GPU adapter, skipping");
                return;
            };
            assert!(reds > 100, "{projection:?} at {pitch}: {reds} red");
            assert_eq!(greens, 0, "{projection:?} at {pitch}");
        }
        let (reds, greens) = counts(0.3, projection).unwrap();
        assert!(
            reds > 100 && greens > 30,
            "{projection:?}: {reds} red, {greens} green"
        );
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
fn origin_marker_is_on_the_sketch_origin() {
    // Editing a sketch on a plane away from the world origin, the marker
    // is on the sketch's origin, where its axis lines cross, in either
    // projection.
    let plane = GridPlane::new(Vec3::new(2.0, 1.0, 1.5), Vec3::X, Vec3::Y).unwrap();
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = Camera::default();
        camera.set_projection(projection);
        let extras = Extras {
            grid: plane,
            sketch: Some((plane, SketchLayer::default())),
            ..Extras::default()
        };
        let Some(pixels) = render_with(&camera, &RenderMesh::default(), extras) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        // The middle of the marker's white pixels.
        let (mut sum, mut white) = (glam::Vec2::ZERO, 0);
        for y in CLIP.y..CLIP.y + CLIP.height {
            for x in CLIP.x..CLIP.x + CLIP.width {
                if pixel(&pixels, x, y)[..3].iter().all(|&c| c > 200) {
                    sum += glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    white += 1;
                }
            }
        }
        assert!(white > 0, "{projection:?}: no marker");
        let (ox, oy) = on_screen(&camera, plane.origin());
        let off = (sum / white as f32).distance(glam::Vec2::new(ox as f32, oy as f32));
        assert!(off < 2.0, "{projection:?}: marker {off} pixels off");
    }
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

/// Where `world` is drawn in [`VIEWPORT`] by `camera`.
fn on_screen(camera: &Camera, world: Vec3) -> (u32, u32) {
    let offset = world - camera.eye();
    let pixels = VIEWPORT.height / camera.view_height()
        * match camera.projection() {
            Projection::Perspective => camera.distance() / -offset.dot(camera.backward()),
            Projection::Orthographic => 1.0,
        };
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

/// A cube `size` on a side from `at`, tessellated, see [`block`].
fn cube(size: f32, at: Vec3) -> RenderMesh {
    block(at, Vec3::splat(size))
}

/// A box from `at`, `size` on each side, tessellated. Built at the origin
/// and moved, so it can reach past [`varde_kernel::MAX_COORD`], which a
/// solid can't.
fn block(at: Vec3, size: Vec3) -> RenderMesh {
    let solid = Solid::cuboid(DVec3::ZERO, size.as_dvec3(), 0, &Tolerance::DEFAULT);
    let mesh = solid.unwrap().tessellate(&varde_kernel::Display::default());
    let mut moved = RenderMesh::default();
    moved.append_at(&mesh.unwrap(), at).unwrap();
    moved
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
fn sketches_in_front_of_glass_are_drawn_over_it() {
    // A line above a cube nearly opaque shows as it does over an opaque
    // one, and one beneath it is hidden as by an opaque one.
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    let mesh = cube(2.0, Vec3::ZERO);
    for (z, shown) in [(-1.0, false), (3.0, true)] {
        let (from, to) = (Vec3::new(-3.0, 1.0, z), Vec3::new(5.0, 1.0, z));
        let render = |opacity: f32| {
            let extras = Extras {
                sketches: lines(&[(from, to)]),
                opacity: vec![opacity],
                ..Extras::default()
            };
            render_with(&camera, &mesh, extras)
        };
        let (Some(glass), Some(opaque)) = (render(0.95), render(1.0)) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let (column, _) = on_screen(&camera, Vec3::new(1.0, 1.0, z));
        assert_eq!(yellow_in_column(&opaque, column), shown, "z {z}");
        assert_eq!(yellow_in_column(&glass, column), shown, "z {z}");
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

/// A mesh of only the edges along `polylines`, each between two faces (so
/// none is a crease) of one triangle of no area each, which draw nothing.
/// A polyline ending where it starts closes on one corner.
fn edges(polylines: &[&[Vec3]]) -> RenderMesh {
    let mut parts = MeshParts {
        indices: vec![0; 6],
        face_ends: vec![3, 6],
        ..MeshParts::default()
    };
    for polyline in polylines {
        let first = parts.positions.len() as u32;
        parts
            .positions
            .extend(polyline.iter().map(|point| point.to_array()));
        parts
            .edge_vertices
            .extend(first..parts.positions.len() as u32);
        parts.edge_ends.push(parts.edge_vertices.len() as u32);
        parts.edge_faces.push([0, 1]);
        let (start, end) = (polyline[0], polyline[polyline.len() - 1]);
        let corner = parts.corners.len() as u32;
        parts.corners.push(start.to_array());
        if end == start {
            parts.edge_corners.push([corner, corner]);
        } else {
            parts.corners.push(end.to_array());
            parts.edge_corners.push([corner, corner + 1]);
        }
    }
    parts.normals = vec![[0.0, 0.0, 1.0]; parts.positions.len()];
    parts.part_ends = vec![[
        2,
        parts.edge_ends.len() as u32,
        parts.corners.len() as u32,
        0,
    ]];
    RenderMesh::from_parts(parts).unwrap()
}

/// A grid that doesn't show: its plane is seen edge on from the top, and
/// its axes are off the screen or seen end on.
fn hidden_grid() -> GridPlane {
    GridPlane::new(Vec3::new(0.0, 1000.0, 0.0), Vec3::X, Vec3::Z).unwrap()
}

/// What draws `mesh`'s edges yellow, optionally faded, over a grid that
/// doesn't show ([`hidden_grid`]).
fn yellow_edges(faded: bool) -> Extras {
    Extras {
        grid: hidden_grid(),
        faded,
        colors: Some(Colors {
            edge: Srgb([1.0, 1.0, 0.0]),
            ..COLORS
        }),
        ..Extras::default()
    }
}

#[test]
fn edges_are_anti_aliased_at_their_sides() {
    // Along the middle of a row: 1.5 pixels wide, the row covered and
    // a quarter of each row either side.
    let y = -3.35;
    let mesh = edges(&[&[Vec3::new(-100.0, y, 0.0), Vec3::new(100.0, y, 0.0)]]);
    let Some(pixels) = render_sketch(&top_camera(), &mesh, yellow_edges(false), 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (_, row) = sketch_pixel(0.0, y, 1.0);
    let row = row as u32;
    for x in [40, 75, 200] {
        let covered = |y| yellowness(pixel(&pixels, x, y));
        assert!(covered(row) > 0.95, "row {row} is {} covered", covered(row));
        for y in [row - 1, row + 1] {
            assert!(
                (0.15..0.35).contains(&covered(y)),
                "row {y} is {} covered",
                covered(y)
            );
        }
        for y in [row - 2, row + 2] {
            assert_eq!(covered(y), 0.0, "row {y}");
        }
    }
}

/// A mesh of an edge along `edge`, a crease along `crease` and a wire
/// along `wire`, each a segment.
fn edge_crease_and_wire(edge: [Vec3; 2], crease: [Vec3; 2], wire: [Vec3; 2]) -> RenderMesh {
    let mut parts = edges(&[&edge, &crease]).into_parts();
    parts.edge_faces[1] = [0, 0];
    let first = parts.positions.len() as u32;
    parts.positions.extend(wire.map(|p| p.to_array()));
    parts.normals.extend([[0.0, 0.0, 1.0]; 2]);
    parts.wire_vertices = vec![first, first + 1];
    parts.wire_ends = vec![2];
    parts.part_ends[0][3] = 1;
    RenderMesh::from_parts(parts).unwrap()
}

#[test]
fn creases_are_thinner_and_fainter_and_wires_only_in_a_wireframe() {
    // Ten rows apart, each along the middle of a row.
    let ys = [-1.35, -2.35, -3.35];
    let [edge, crease, wire] = ys.map(|y| [Vec3::new(-100.0, y, 0.0), Vec3::new(100.0, y, 0.0)]);
    let mesh = edge_crease_and_wire(edge, crease, wire);
    let render = |wireframe| {
        let extras = Extras {
            wireframe,
            ..yellow_edges(false)
        };
        let pixels = render_sketch(&top_camera(), &mesh, extras, 1.0)?;
        // How much yellow covers down a column within 4 rows of each line.
        Some(ys.map(|y| {
            let (_, row) = sketch_pixel(0.0, y, 1.0);
            let row = row as u32;
            yellow_down(&pixels, 75, row - 4..row + 5)
        }))
    };
    let (Some(plain), Some(wired)) = (render(false), render(true)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let faint = CREASE_WIDTH * CREASE_ALPHA;
    assert!((plain[0] - EDGE_WIDTH).abs() < 0.1, "{plain:?}");
    assert!((plain[1] - faint).abs() < 0.1, "{plain:?}");
    assert_eq!(plain[2], 0.0, "{plain:?}");
    assert_eq!(wired[..2], plain[..2]);
    assert!((wired[2] - faint).abs() < 0.1, "{wired:?}");
}

/// One face of two triangles, seen from the top, sharing an edge along
/// `y` and reaching far either side of it, with no feature edges; its
/// vertices' normals as `normals` gives them.
fn two_triangles(y: f32, normals: [[f32; 3]; 4]) -> RenderMesh {
    RenderMesh::from_parts(MeshParts {
        positions: vec![
            [-100.0, y, 0.0],
            [100.0, y, 0.0],
            [0.0, y + 4.35, 0.0],
            [0.0, y - 4.65, 0.0],
        ],
        normals: normals.to_vec(),
        indices: vec![0, 1, 2, 1, 0, 3],
        face_ends: vec![6],
        part_ends: vec![[1, 0, 0, 0]],
        ..MeshParts::default()
    })
    .unwrap()
}

#[test]
fn triangle_edges_show_only_in_a_tessellation_wireframe() {
    // Along the middle of a row; the triangles' other edges are far from
    // it down the column measured.
    let y = -1.35;
    let mesh = two_triangles(y, [[0.0, 0.0, 1.0]; 4]);
    let render = |tessellation| {
        let extras = Extras {
            tessellation,
            // Black faces, lit grey, under which yellow still shows as
            // much as it covers.
            colors: Some(Colors {
                model: Srgb([0.0; 3]),
                contrast: 1.0,
                edge: Srgb([1.0, 1.0, 0.0]),
                ..COLORS
            }),
            ..yellow_edges(false)
        };
        let pixels = render_sketch(&top_camera(), &mesh, extras, 1.0)?;
        let (_, row) = sketch_pixel(0.0, y, 1.0);
        let row = row as u32;
        Some(yellow_down(&pixels, 75, row - 4..row + 5))
    };
    let (Some(plain), Some(tessellated)) = (render(false), render(true)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert_eq!(plain, 0.0);
    let faint = CREASE_WIDTH * CREASE_ALPHA;
    assert!((tessellated - faint).abs() < 0.1, "{tessellated}");
}

#[test]
fn flat_shading_lights_a_face_by_its_plane_and_metal_differs_flat_or_not() {
    // Normals bent apart across a flat face: lit smoothly they shade it
    // unevenly, flat it's one colour.
    let y = -1.35;
    let bent = [
        [-0.6, 0.0, 0.8],
        [0.6, 0.0, 0.8],
        [0.0, 0.6, 0.8],
        [0.0, -0.6, 0.8],
    ];
    let mesh = two_triangles(y, bent);
    let render = |shading| {
        let extras = Extras {
            shading,
            ..yellow_edges(false)
        };
        let pixels = render_sketch(&top_camera(), &mesh, extras, 1.0)?;
        // Two points of the upper triangle, apart across it.
        let at = |x: f32, y: f32| {
            let (x, y) = sketch_pixel(x, y, 1.0);
            pixel(&pixels, x as u32, y as u32)
        };
        Some([at(-6.0, y + 1.0), at(6.0, y + 1.0)])
    };
    let (Some(regular), Some(flat), Some(metal), Some(flat_metal)) = (
        render(Shading::Regular),
        render(Shading::Flat),
        render(Shading::Metal),
        render(Shading::FlatMetal),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert_ne!(regular[0], regular[1]);
    assert_eq!(flat[0], flat[1]);
    assert_ne!(metal[0], regular[0]);
    assert_eq!(flat_metal[0], flat_metal[1]);
    assert_ne!(flat_metal[0], flat[0]);
}

#[test]
fn edges_keep_their_width_at_any_scale_and_zoom() {
    let measure = |scale: f32, zoom: f32, projection| {
        let mut camera = top_camera();
        camera.set_projection(projection);
        camera.zoom(zoom);
        // A quarter of the view below the middle, measured left of it,
        // clear of the origin marker.
        let height = camera.view_height();
        let (y, x) = (-0.25 * height, 100.0 * height);
        let mesh = edges(&[&[Vec3::new(-x, y, 0.0), Vec3::new(x, y, 0.0)]]);
        let pixels = render_sketch(&camera, &mesh, yellow_edges(false), scale)?;
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
    assert!(
        (EDGE_WIDTH - 0.1..EDGE_WIDTH + 0.1).contains(&one),
        "{one} pixels wide"
    );
    assert!((1.9..2.1).contains(&(two / one)), "grew {}x", two / one);
    assert!((zoomed - one).abs() < 0.1, "{zoomed} zoomed in, {one} not");
    assert!(
        (perspective - one).abs() < 0.1,
        "{perspective} in perspective"
    );
}

#[test]
fn edge_joins_are_no_more_opaque_than_their_middles() {
    // Faded, so a pixel drawn twice would be more opaque: a polyline with
    // sharp turns, the same point twice, and three quarters of a circle in
    // short segments over the top from the left, then straight down; clear
    // of the origin marker.
    let mut curve = vec![
        Vec3::new(-12.0, -5.0, 0.0),
        Vec3::new(-10.0, 1.0, 0.0),
        Vec3::new(-10.0, 1.0, 0.0),
    ];
    curve.extend((1..=48).map(|i| {
        let angle = std::f32::consts::PI * (1.0 - 1.5 * i as f32 / 48.0);
        Vec3::new(-6.0 + 4.0 * angle.cos(), 1.0 + 4.0 * angle.sin(), 0.0)
    }));
    curve.push(Vec3::new(-6.0, -6.0, 0.0));
    let mesh = edges(&[&curve]);
    let (Some(faded), Some(opaque)) = (
        render_sketch(&top_camera(), &mesh, yellow_edges(true), 1.0),
        render_sketch(&top_camera(), &mesh, yellow_edges(false), 1.0),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let alpha = COLORS.faded_alpha;
    let most = faded.iter().map(|&p| yellowness(p)).fold(0.0, f32::max);
    assert!(
        (alpha - 0.02..alpha + 0.02).contains(&most),
        "{most} at most"
    );
    // Faded where it's drawn opaque.
    for (faded, opaque) in faded.iter().zip(&opaque) {
        let (faded, opaque) = (yellowness(*faded), yellowness(*opaque));
        assert!(
            (faded - opaque * alpha).abs() < 0.02,
            "{faded} for {opaque}"
        );
    }
}

#[test]
fn edges_crossing_the_near_plane_are_cut_there() {
    // As `sketch_lines_crossing_the_near_plane_are_cut_there`, an edge
    // of several segments, some cut, some wholly behind the eye.
    let mut camera = Camera::default();
    camera.set_projection(Projection::Perspective);
    camera.look_from(View::Front);
    let points: Vec<_> = (0..=7)
        .map(|i| Vec3::new(1.0, 5.0 - 5.0 * i as f32, 1.0))
        .collect();
    let mesh = edges(&[&points]);
    let Some(pixels) = render_with(&camera, &mesh, yellow_edges(false)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let mut drawn = 0;
    for y in CLIP.y..CLIP.y + CLIP.height {
        for x in CLIP.x..CLIP.x + CLIP.width {
            if yellow(pixel(&pixels, x, y)) {
                assert!(x >= cx && y <= cy, "edge at ({x}, {y})");
                drawn += 1;
            }
        }
    }
    assert!(drawn > 10, "only {drawn} pixels of edge");
}

#[test]
fn a_closed_edge_is_joined_where_it_closes() {
    // Faded, so a pixel drawn twice would be more opaque: a square ending
    // where it starts, along the middles of pixels so they're covered,
    // clear of the origin marker.
    let corners = [
        Vec3::new(-12.05, -4.95, 0.0),
        Vec3::new(-4.05, -4.95, 0.0),
        Vec3::new(-4.05, 3.05, 0.0),
        Vec3::new(-12.05, 3.05, 0.0),
        Vec3::new(-12.05, -4.95, 0.0),
    ];
    let mesh = edges(&[&corners]);
    let Some(faded) = render_sketch(&top_camera(), &mesh, yellow_edges(true), 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let alpha = COLORS.faded_alpha;
    let most = faded.iter().map(|&p| yellowness(p)).fold(0.0, f32::max);
    assert!(
        (alpha - 0.02..alpha + 0.02).contains(&most),
        "{most} at most"
    );
}

#[test]
fn edges_near_the_eye_show_where_they_are_in_perspective() {
    // Looking down in perspective, an edge across the view a twentieth of
    // the way from the eye to the target, 40.5 pixels below the middle:
    // along the middle of a row, which it covers evenly either side.
    // Pulled towards the camera for depth, it isn't moved on the screen.
    let mut camera = top_camera();
    camera.set_projection(Projection::Perspective);
    let (distance, height) = (camera.distance(), camera.view_height());
    let depth = 0.05 * distance;
    let rows = SKETCH_VIEW[1];
    // A pixel is this many world units at that depth.
    let unit = depth * height / (distance * rows);
    let (y, z) = (-40.5 * unit, distance - depth);
    let mesh = edges(&[&[Vec3::new(-height, y, z), Vec3::new(height, y, z)]]);
    let Some(pixels) = render_sketch(&camera, &mesh, yellow_edges(false), 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let x = SKETCH_VIEW[0] as u32 / 4;
    let (mut sum, mut weight) = (0.0, 0.0);
    for row in rows as u32 / 2 + 4..rows as u32 {
        let covered = yellowness(pixel(&pixels, x, row));
        sum += covered * (row as f32 + 0.5);
        weight += covered;
    }
    let centre = sum / weight;
    let expected = rows / 2.0 + 40.5;
    assert!(
        (centre - expected).abs() < 0.2,
        "at {centre}, not {expected}"
    );
}

/// Whether `along` is dashed by [`HIDDEN_DASH`] at `alpha`: as opaque
/// as that at most and reaching it, clear between, repeating every dash
/// and gap.
fn dashed(along: &[f32], alpha: f32) {
    let period = (HIDDEN_DASH[0] + HIDDEN_DASH[1]) as usize;
    let most = along.iter().copied().fold(0.0, f32::max);
    assert!((alpha - 0.05..alpha + 0.05).contains(&most), "{along:?}");
    assert!(along.iter().any(|&c| c < 0.02), "{along:?}");
    for (x, pair) in along.iter().zip(&along[period..]).enumerate() {
        assert!((pair.0 - pair.1).abs() < 0.08, "{x}: {along:?}");
    }
}

#[test]
fn an_edge_of_a_cube_behind_another_body_is_dashed_where_its_hidden() {
    // From the top in perspective, a cube whose top face is level with
    // the target, its near edge along a row, and a thin plate over its
    // middle, nearer the eye: the edge is solid either side of the plate
    // and dashed under it. Not with the option off, nor faded.
    let (camera, y, mesh) = cube_under_plate();
    let render = |hidden_edges, faded| {
        let extras = Extras {
            hidden_edges,
            ..yellow_edges(faded)
        };
        render_scaled(&camera, &mesh, extras, FULL, FULL_CLIP, 1.0)
    };
    let (Some(on), Some(off), Some(faded)) = (
        render(true, false),
        render(false, false),
        render(true, true),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let edge = in_perspective(&camera, Vec3::new(0.0, y, 0.0));
    let row = edge.y as u32;
    let plate = [-1.0, 1.0].map(|x| in_perspective(&camera, Vec3::new(x, y, 1.05)).x as u32);
    let ends = [-3.0, 3.0].map(|x| in_perspective(&camera, Vec3::new(x, y, 0.0)).x as u32);
    // Clear of the corners and the plate's own edges.
    let hidden = plate[0] + 4..plate[1] - 4;
    let shown = [ends[0] + 4..plate[0] - 4, plate[1] + 4..ends[1] - 4];
    assert!(hidden.len() > 25, "{hidden:?}");
    dashed(
        &yellow_along(&on, row, hidden.clone()),
        COLORS.hidden_edge_alpha,
    );
    for pixels in [&on, &off] {
        for span in shown.clone() {
            let along = yellow_along(pixels, row, span);
            assert!(along.iter().all(|&c| c > 0.95), "{along:?}");
        }
    }
    for pixels in [&off, &faded] {
        let along = yellow_along(pixels, row, hidden.clone());
        assert!(along.iter().all(|&c| c < 0.02), "{along:?}");
    }
}

#[test]
fn an_edge_is_dashed_from_where_it_goes_behind_a_face_without_a_gap() {
    // From the top, an edge rising through a cube's top face: under it
    // to the left, dashed, and solid from where it comes out, which is
    // between two pixels' middles (taking the edges' pull towards the
    // eye into account). Its dashes are laid so the pixel left of there
    // is in the middle of one: the two stretches meet with neither a gap
    // nor a pixel of both.
    let pull = 0.002 * top_camera().view_height();
    let (rise, through) = (0.5, 2.0 * pull);
    let z = |x: f32| 5.0 + rise * (x - through);
    // How far along the edge a unit across is, in pixels.
    let per_unit = 10.0 * (1.0 + rise * rise).sqrt();
    // The pixel left of where it comes out, 0.05 left of it, in the
    // middle of a dash.
    let at = -0.05;
    let period = HIDDEN_DASH[0] + HIDDEN_DASH[1];
    let start = at - (19.0 * period + HIDDEN_DASH[0] / 2.0) / per_unit;
    let y = -3.35;
    let mut mesh = edges(&[&[Vec3::new(start, y, z(start)), Vec3::new(12.0, y, z(12.0))]]);
    mesh.append(&cube(5.0, Vec3::new(-2.5, -6.0, 0.0))).unwrap();
    let extras = Extras {
        hidden_edges: true,
        ..yellow_edges(false)
    };
    let Some(pixels) = render_sketch(&top_camera(), &mesh, extras, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (_, row) = sketch_pixel(0.0, y, 1.0);
    let (left, _) = sketch_pixel(at, y, 1.0);
    let (row, left) = (row as u32, left as u32);
    // That pixel and the one before it are in the dash.
    let along = yellow_along(&pixels, row, left - 1..left + 3);
    let alpha = COLORS.hidden_edge_alpha;
    for &hidden in &along[..2] {
        assert!((alpha - 0.05..alpha + 0.05).contains(&hidden), "{along:?}");
    }
    for &shown in &along[2..] {
        assert!(shown > 0.95, "{along:?}");
    }
    // Dashed under the face, clear of the cube's own edges: its dashes
    // are shorter across the screen than along it, which rises.
    let (face, _) = sketch_pixel(-2.5, y, 1.0);
    let under = yellow_along(&pixels, row, face as u32 + 3..left - 2);
    assert!(under.iter().all(|&c| c < alpha + 0.05), "{under:?}");
    assert!(
        under.iter().filter(|&&c| c > alpha - 0.05).count() > 4,
        "{under:?}"
    );
    assert!(under.iter().filter(|&&c| c < 0.02).count() > 4, "{under:?}");
}

#[test]
fn hidden_dashes_keep_their_length_zoomed_far_into_a_long_edge() {
    // A 2000 long edge of two segments, under a plate, looked at from
    // the top 1500 along it, 0.005 across the view's height: 25600 pixels
    // a unit, so how far along the edge a pixel is, in pixels, is past
    // what `f32` holds to a pixel.
    let mut camera = top_camera();
    camera.set_target(Vec3::new(500.0, 0.0, 0.0));
    camera.zoom(0.005 / camera.view_height());
    let height = camera.view_height();
    // On the middle of the row 32 below the view's middle.
    let y = -32.5 * height / SKETCH_VIEW[1];
    let mut mesh = edges(&[&[
        Vec3::new(-1000.0, y, 0.0),
        Vec3::new(0.0, y, 0.0),
        Vec3::new(1000.0, y, 0.0),
    ]]);
    mesh.append(&block(
        Vec3::new(400.0, -100.0, 1.0),
        Vec3::new(200.0, 200.0, 1.0),
    ))
    .unwrap();
    let extras = Extras {
        hidden_edges: true,
        ..yellow_edges(false)
    };
    let Some(pixels) = render_sketch(&camera, &mesh, extras, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let row = SKETCH_VIEW[1] as u32 / 2 + 32;
    dashed(
        &yellow_along(&pixels, row, 10..SKETCH_VIEW[0] as u32 - 10),
        COLORS.hidden_edge_alpha,
    );
}

#[test]
fn hidden_dashes_shorter_than_a_pixel_on_the_screen_blur_to_their_average() {
    // From the top, an edge plunging through a block, 7.5 times as long
    // as it shows: a dash and a gap, 7 pixels along it, take less than a
    // pixel on the screen, so every pixel is as opaque as the dashes are
    // on average.
    let ratio = 7.5f32;
    let steep = (ratio * ratio - 1.0).sqrt();
    let y = -3.35;
    let z = |x: f32| -70.0 + steep * x;
    let mut mesh = edges(&[&[Vec3::new(-8.0, y, z(-8.0)), Vec3::new(8.0, y, z(8.0))]]);
    mesh.append(&block(
        Vec3::new(-10.0, -5.0, -200.0),
        Vec3::new(20.0, 10.0, 200.0),
    ))
    .unwrap();
    let extras = Extras {
        hidden_edges: true,
        ..yellow_edges(false)
    };
    let Some(pixels) = render_sketch(&top_camera(), &mesh, extras, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (_, row) = sketch_pixel(0.0, y, 1.0);
    let ends = [-7.0, 7.0].map(|x| sketch_pixel(x, y, 1.0).0 as u32);
    let along = yellow_along(&pixels, row as u32, ends[0]..ends[1]);
    let average = COLORS.hidden_edge_alpha * HIDDEN_DASH[0] / (HIDDEN_DASH[0] + HIDDEN_DASH[1]);
    for &c in &along {
        assert!(
            (c - average).abs() < 0.03,
            "{average} on average: {along:?}"
        );
    }
}

/// `alpha` as a part is drawn at: in steps of 8 bits.
fn drawn_alpha(alpha: f32) -> f32 {
    (alpha * 255.0).round() / 255.0
}

/// From the front, the middle of the front face of a cube from 0 to 2,
/// and a cube behind it: the far one the mesh's first part, the near one
/// its second.
fn cube_behind_cube() -> (Camera, RenderMesh, RenderMesh) {
    let mut camera = Camera::default();
    camera.set_target(Vec3::ONE);
    camera.look_from(View::Front);
    let near = cube(2.0, Vec3::ZERO);
    let mut both = cube(2.0, Vec3::Y * 5.0);
    both.append(&near).unwrap();
    (camera, near, both)
}

#[test]
fn a_body_behind_a_transparent_one_shows_through_it() {
    // A cube behind a 30 % one shows through both its back and front
    // faces: two layers of the glass.
    let (camera, near, both) = cube_behind_cube();
    let far = cube(2.0, Vec3::Y * 5.0);
    let with = |opacity: Vec<f32>| Extras {
        opacity,
        ..Extras::default()
    };
    let (Some(glass), Some(through), Some(behind), Some(opaque)) = (
        render_with(&camera, &near, with(vec![0.3])),
        render_with(&camera, &both, with(vec![1.0, 0.3])),
        render_with(&camera, &far, Extras::default()),
        render_with(&camera, &both, Extras::default()),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let [glass, through, behind, opaque] =
        [&glass, &through, &behind, &opaque].map(|pixels| pixel(pixels, cx, cy));
    let alpha = drawn_alpha(0.3);
    for c in 0..3 {
        let shown = f32::from(through[c]) - f32::from(glass[c]);
        let expected = f32::from(behind[c]) * (1.0 - alpha) * (1.0 - alpha);
        assert!(
            (shown - expected).abs() <= 2.0,
            "{through:?} over {glass:?} for {behind:?}"
        );
    }
    // The glass alone tints the black background, and is fainter than
    // the cube opaque.
    for c in 0..3 {
        assert!(
            glass[c] > 20 && glass[c] < opaque[c],
            "{glass:?}, {opaque:?}"
        );
    }
}

#[test]
fn a_transparent_body_behind_an_opaque_one_is_hidden() {
    // The same cubes, the far one 30 %: the near one hides it, as if it
    // were opaque.
    let (camera, near, both) = cube_behind_cube();
    let (Some(alone), Some(hiding)) = (
        render_with(&camera, &near, Extras::default()),
        render_with(
            &camera,
            &both,
            Extras {
                opacity: vec![0.3, 1.0],
                ..Extras::default()
            },
        ),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    for (dx, dy) in [(0, 0), (-4, -4), (4, 4), (4, -4)] {
        let (x, y) = (cx.wrapping_add_signed(dx), cy.wrapping_add_signed(dy));
        assert_eq!(pixel(&hiding, x, y), pixel(&alone, x, y), "at {x}, {y}");
    }
}

/// From the top in perspective, a cube 6 on a side whose top face is level
/// with the target, its near edge along the middle of a row, at `y`.
fn cube_under_top_camera() -> (Camera, f32, RenderMesh) {
    let mut camera = top_camera();
    camera.set_projection(Projection::Perspective);
    let y = -2.025;
    (camera, y, cube(6.0, Vec3::new(-3.0, y, -6.0)))
}

/// [`cube_under_top_camera`], and a thin plate over the middle of its
/// near edge, nearer the eye: the plate the mesh's second part.
fn cube_under_plate() -> (Camera, f32, RenderMesh) {
    let (camera, y, mut mesh) = cube_under_top_camera();
    let plate = block(Vec3::new(-1.0, -3.0, 1.0), Vec3::new(2.0, 2.0, 0.05));
    mesh.append(&plate).unwrap();
    (camera, y, mesh)
}

/// How much yellow covers a column `x` near `row`, in pixels.
fn yellow_near(pixels: &[[u8; 4]], x: u32, row: u32) -> f32 {
    (row - 3..=row + 3)
        .map(|y| yellowness(pixel(pixels, x, y)))
        .sum()
}

#[test]
fn a_transparent_bodys_back_edges_are_dimmed_and_its_front_edges_crisp() {
    // The cube at 50 %, its edges yellow: the near edge of its top face
    // is drawn again over the glass, at its alpha, while the bottom's is
    // dimmed by the faces in front of it, and dashed over that.
    let (camera, y, mesh) = cube_under_top_camera();
    let alpha = drawn_alpha(0.5);
    let render = |opacity| {
        let extras = Extras {
            hidden_edges: true,
            opacity: vec![opacity],
            ..yellow_edges(false)
        };
        render_scaled(&camera, &mesh, extras, FULL, FULL_CLIP, 1.0)
    };
    let (Some(glass), Some(opaque)) = (render(0.5), render(1.0)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let top = in_perspective(&camera, Vec3::new(0.0, y, 0.0)).y as u32;
    let bottom = in_perspective(&camera, Vec3::new(0.0, y, -6.0)).y as u32;
    let ends = [-2.0, 2.0].map(|x| in_perspective(&camera, Vec3::new(x, y, -6.0)).x as u32);
    let middle = (ends[0] + ends[1]) / 2;
    let front = yellow_near(&glass, middle, top);
    let solid = yellow_near(&opaque, middle, top);
    // As opaque as the part, over its width.
    assert!(
        front >= solid * alpha - 0.05,
        "{front} of the front edge, {solid} opaque"
    );
    // Seen through the bottom's back face and the top face.
    let back: Vec<f32> = (ends[0]..ends[1])
        .map(|x| yellow_near(&glass, x, bottom))
        .collect();
    let dimmed = solid * alpha * (1.0 - alpha) * (1.0 - alpha);
    // Dimmed between the dashes, and dashed over that by the glass
    // hiding them, at its alpha.
    let least = back.iter().copied().fold(f32::MAX, f32::min);
    let most = back.iter().copied().fold(0.0, f32::max);
    assert!((least - dimmed).abs() < 0.05, "{dimmed} expected: {back:?}");
    assert!(most > least + 0.05, "not dashed: {back:?}");
    assert!(front > 2.0 * most, "{front} not crisper than {back:?}");
}

#[test]
fn edges_behind_glass_are_seen_and_dashed_by_its_alpha() {
    // The cube's near edge under a thin plate over its middle: behind a
    // plate at 50 % it's solid, dimmed by the plate, and dashed over that;
    // behind one at 95 % it's much as behind an opaque one; a 50 % cube's
    // behind an opaque plate is dashed at half the alpha.
    let (camera, y, mesh) = cube_under_plate();
    let render = |opacity| {
        let extras = Extras {
            hidden_edges: true,
            opacity,
            ..yellow_edges(false)
        };
        render_scaled(&camera, &mesh, extras, FULL, FULL_CLIP, 1.0)
    };
    let (Some(half), Some(nearly), Some(opaque), Some(dashes)) = (
        render(vec![1.0, 0.5]),
        render(vec![1.0, 0.95]),
        render(vec![1.0, 1.0]),
        render(vec![0.5, 1.0]),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let alpha = drawn_alpha(0.5);
    let row = in_perspective(&camera, Vec3::new(0.0, y, 0.0)).y as u32;
    let plate = [-1.0, 1.0].map(|x| in_perspective(&camera, Vec3::new(x, y, 1.05)).x as u32);
    let under = plate[0] + 4..plate[1] - 4;
    let seen = yellow_along(&half, row, under.clone());
    let least = seen.iter().copied().fold(1.0, f32::min);
    let most = seen.iter().copied().fold(0.0, f32::max);
    assert!(least > 0.15 && most > least + 0.1, "{seen:?}");
    let period = (HIDDEN_DASH[0] + HIDDEN_DASH[1]) as usize;
    for (x, pair) in seen.iter().zip(&seen[period..]).enumerate() {
        assert!((pair.0 - pair.1).abs() < 0.08, "{x}: {seen:?}");
    }
    let nearly = yellow_along(&nearly, row, under.clone());
    let opaque = yellow_along(&opaque, row, under.clone());
    for (n, o) in nearly.iter().zip(&opaque) {
        assert!((n - o).abs() < 0.08, "{nearly:?} against {opaque:?}");
    }
    dashed(
        &yellow_along(&dashes, row, under),
        COLORS.hidden_edge_alpha * alpha,
    );
}

#[test]
fn the_order_of_two_transparent_bodies_barely_changes_the_pixels() {
    // A cube and a bar through it, their bounds' centres the same, so
    // they're drawn in the mesh's order, at 50 % each: either way round,
    // each pixel is about the same.
    let mut camera = Camera::default();
    camera.set_target(Vec3::ONE);
    let a = cube(2.0, Vec3::ZERO);
    let b = block(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(4.0, 1.0, 1.0));
    let mut ab = a.clone();
    ab.append(&b).unwrap();
    let mut ba = b;
    ba.append(&a).unwrap();
    let extras = || Extras {
        opacity: vec![0.5, 0.5],
        ..Extras::default()
    };
    let (Some(ab), Some(ba)) = (
        render_with(&camera, &ab, extras()),
        render_with(&camera, &ba, extras()),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let most = ab
        .iter()
        .zip(&ba)
        .flat_map(|(p, q)| (0..3).map(move |c| p[c].abs_diff(q[c])))
        .max()
        .unwrap_or(0);
    assert!(most <= 12, "{most} apart");
}

#[test]
fn opacity_out_of_range_is_opaque() {
    // NaN, past 1, below 0 or missing: each drawn as an opaque part.
    let (camera, _, both) = cube_behind_cube();
    let Some(opaque) = render_with(&camera, &both, Extras::default()) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    for opacity in [vec![f32::NAN, 1.5], vec![-0.5], vec![1.0, 1.0, 0.3]] {
        let extras = Extras {
            opacity: opacity.clone(),
            ..Extras::default()
        };
        let drawn = render_with(&camera, &both, extras).unwrap();
        assert!(drawn == opaque, "{opacity:?}");
    }
}

/// The id of `mesh`'s first face whose normals point along `normal`.
fn face_facing(mesh: &RenderMesh, normal: Vec3) -> u32 {
    let faces = mesh.faces().position(|indices| {
        let at = Vec3::from(mesh.normals()[indices[0] as usize]);
        at.dot(normal) > 0.99
    });
    faces.unwrap() as u32
}

/// How much brighter `a` is than `b`, summed over its channels.
fn brighter(a: [u8; 4], b: [u8; 4]) -> i32 {
    (0..3).map(|c| i32::from(a[c]) - i32::from(b[c])).sum()
}

/// How much bluer than red a pixel is: the selection's teal over the
/// grey model.
fn tint([r, _, b, _]: [u8; 4]) -> i32 {
    i32::from(b) - i32::from(r)
}

#[test]
fn a_hovered_face_is_brighter_and_only_where_it_shows() {
    // From the front, the cube's front face under the middle: hovered, it's
    // brighter; its back face hovered, hidden behind it, is washed and
    // striped over it (see the test of that), outside it nothing changes.
    let (camera, near, _) = cube_behind_cube();
    let front = face_facing(&near, -Vec3::Y);
    let back = face_facing(&near, Vec3::Y);
    let hovering = |hover: Option<u32>| Extras {
        hovered_faces: hover.into_iter().collect(),
        ..Extras::default()
    };
    let (Some(plain), Some(hovered), Some(behind)) = (
        render_with(&camera, &near, Extras::default()),
        render_with(&camera, &near, hovering(Some(front))),
        render_with(&camera, &near, hovering(Some(back))),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let (plain_at, hovered_at) = (pixel(&plain, cx, cy), pixel(&hovered, cx, cy));
    assert!(
        brighter(hovered_at, plain_at) > 60,
        "{hovered_at:?} hovered, {plain_at:?} not"
    );
    assert!(behind != plain);
    // The background around the cube is as it was.
    assert_eq!(
        pixel(&behind, CLIP.x + 2, CLIP.y + 2),
        pixel(&plain, CLIP.x + 2, CLIP.y + 2)
    );
    assert_eq!(
        pixel(&hovered, CLIP.x + 2, CLIP.y + 2),
        pixel(&plain, CLIP.x + 2, CLIP.y + 2)
    );
}

#[test]
fn a_hovered_face_drawn_through_shows_behind_what_hides_it() {
    // The near cube's back face, hidden behind its front: drawn through,
    // it lightens the middle, though less than the front hovered does.
    let (camera, near, _) = cube_behind_cube();
    let front = face_facing(&near, -Vec3::Y);
    let back = face_facing(&near, Vec3::Y);
    let hovering = |face: u32, through: bool| Extras {
        hovered_faces: vec![face],
        hover_through: through,
        ..Extras::default()
    };
    let (Some(plain), Some(through), Some(hovered)) = (
        render_with(&camera, &near, Extras::default()),
        render_with(&camera, &near, hovering(back, true)),
        render_with(&camera, &near, hovering(front, false)),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let [plain_at, through_at, hovered_at] = [&plain, &through, &hovered].map(|p| pixel(p, cx, cy));
    assert!(
        brighter(through_at, plain_at) > 30,
        "{through_at:?} through, {plain_at:?} not"
    );
    assert!(
        brighter(hovered_at, through_at) > 0,
        "{hovered_at:?} hovered, {through_at:?} through"
    );
    // The background around the cube is as it was.
    assert_eq!(
        pixel(&through, CLIP.x + 2, CLIP.y + 2),
        pixel(&plain, CLIP.x + 2, CLIP.y + 2)
    );
}

#[test]
fn a_selected_face_is_tinted_with_the_selection_colour() {
    // Hovered too, it's tinted over the hover, brighter still.
    let (camera, near, _) = cube_behind_cube();
    let front = face_facing(&near, -Vec3::Y);
    let (Some(plain), Some(selected), Some(both)) = (
        render_with(&camera, &near, Extras::default()),
        render_with(
            &camera,
            &near,
            Extras {
                selected_faces: vec![front],
                ..Extras::default()
            },
        ),
        render_with(
            &camera,
            &near,
            Extras {
                hovered_faces: vec![front],
                selected_faces: vec![front],
                ..Extras::default()
            },
        ),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let [plain, selected, both] = [&plain, &selected, &both].map(|p| pixel(p, cx, cy));
    assert!(tint(plain).abs() < 4, "{plain:?}");
    assert!(tint(selected) > 40, "{selected:?}");
    assert!(tint(both) > 40 && brighter(both, selected) > 30, "{both:?}");
}

#[test]
fn a_selected_face_hidden_by_the_model_is_washed_and_striped() {
    // The far cube's front face, selected, behind the opaque near one:
    // tinted all along a row across it, more in the stripes, most at the
    // edge of what's hidden.
    let (camera, _, both) = cube_behind_cube();
    let far_front = face_facing(&both, -Vec3::Y);
    let render = |selected_faces| {
        let extras = Extras {
            selected_faces,
            ..Extras::default()
        };
        render_with(&camera, &both, extras)
    };
    let (Some(plain), Some(selected)) = (render(vec![]), render(vec![far_front])) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let row = |pixels: &[[u8; 4]]| {
        (cx - 6..cx + 6)
            .map(|x| tint(pixel(pixels, x, cy)))
            .collect::<Vec<_>>()
    };
    // Edged where the pattern ends, either side, in the stripes' colour.
    let whole: Vec<i32> = (0..SIZE[0])
        .map(|x| tint(pixel(&selected, x, cy)))
        .collect();
    let (plain, selected) = (row(&plain), row(&selected));
    assert!(plain.iter().all(|t| t.abs() < 4), "{plain:?}");
    let least = selected.iter().copied().min().unwrap();
    let most = selected.iter().copied().max().unwrap();
    assert!(least > 4 && most > least + 5, "{selected:?}");
    let first = whole.iter().position(|&t| t > 4).unwrap();
    let last = whole.iter().rposition(|&t| t > 4).unwrap();
    let max = |at: std::ops::Range<usize>| whole[at].iter().copied().max().unwrap();
    let inside = max(first + 6..last - 5);
    assert!(max(first..first + 3) > inside + 5, "{whole:?}");
    assert!(max(last - 2..last + 1) > inside + 5, "{whole:?}");
}

#[test]
fn a_hovered_face_hidden_by_the_model_is_washed_and_striped() {
    // The far cube's front face, hovered, behind the opaque near one:
    // brighter all along a row across it, more in the stripes; not while
    // the hover is drawn through, whole.
    let (camera, _, both) = cube_behind_cube();
    let far_front = face_facing(&both, -Vec3::Y);
    let render = |hovered_faces, hover_through| {
        let extras = Extras {
            hovered_faces,
            hover_through,
            ..Extras::default()
        };
        render_with(&camera, &both, extras)
    };
    let (Some(plain), Some(hovered), Some(through)) = (
        render(vec![], false),
        render(vec![far_front], false),
        render(vec![far_front], true),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let lift = |pixels: &[[u8; 4]]| {
        (cx - 6..cx + 6)
            .map(|x| i32::from(pixel(pixels, x, cy)[1]) - i32::from(pixel(&plain, x, cy)[1]))
            .collect::<Vec<_>>()
    };
    let (hovered, through) = (lift(&hovered), lift(&through));
    let least = hovered.iter().copied().min().unwrap();
    let most = hovered.iter().copied().max().unwrap();
    assert!(least > 2 && most > least + 5, "{hovered:?}");
    // Drawn through, it's even: no stripes over it.
    let spread = through.iter().max().unwrap() - through.iter().min().unwrap();
    assert!(spread < 4, "{through:?}");
}

#[test]
fn a_selected_face_behind_a_transparent_body_is_still_tinted() {
    // The far cube's front face, selected, seen through the near one at
    // 30 %; and the near one's own front face, selected.
    let (camera, _, both) = cube_behind_cube();
    let far_front = face_facing(&both, -Vec3::Y);
    let near_front = far_front + 6;
    let render = |selected_faces| {
        let extras = Extras {
            opacity: vec![1.0, 0.3],
            selected_faces,
            ..Extras::default()
        };
        render_with(&camera, &both, extras)
    };
    let (Some(plain), Some(far), Some(near)) = (
        render(vec![]),
        render(vec![far_front]),
        render(vec![near_front]),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let [plain, far, near] = [&plain, &far, &near].map(|p| pixel(p, cx, cy));
    assert!(tint(plain).abs() < 4, "{plain:?}");
    assert!(tint(far) > 20, "{far:?} behind the glass");
    assert!(tint(near) > 10, "{near:?} on the glass");
}

#[test]
fn a_selected_edge_and_vertex_behind_a_transparent_body_are_still_drawn() {
    // From the front, a cube 2 on a side behind one 4 on a side at 30 %,
    // both above the grid's axis: the far one's front face's bottom edge
    // and a corner, selected, show through the glass in the selection's
    // colour.
    let mut camera = Camera::default();
    camera.set_target(Vec3::new(1.0, 1.0, 2.0));
    camera.look_from(View::Front);
    let mut mesh = cube(2.0, Vec3::new(0.0, 5.0, 1.0));
    mesh.append(&cube(4.0, Vec3::new(-1.0, 0.0, 0.0))).unwrap();
    let edge = mesh
        .polylines()
        .position(|polyline| {
            polyline.iter().all(|&v| {
                let [_, y, z] = mesh.positions()[v as usize];
                y == 5.0 && z == 1.0
            })
        })
        .unwrap() as u32;
    let corner = mesh
        .corners()
        .iter()
        .position(|&p| p == [2.0, 5.0, 3.0])
        .unwrap() as u32;
    let render = |highlights| {
        let extras = Extras {
            opacity: vec![1.0, 0.3],
            highlights,
            ..Extras::default()
        };
        render_with(&camera, &mesh, extras)
    };
    let (Some(plain), Some(selected)) = (
        render(Highlights::default()),
        render(Highlights {
            selected_edges: vec![edge],
            vertices: vec![Vertex {
                corner,
                hovered: false,
                selected: true,
            }],
            ..Highlights::default()
        }),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    for at in [Vec3::new(1.0, 5.0, 1.0), Vec3::new(2.0, 5.0, 3.0)] {
        let (x, y) = on_screen(&camera, at);
        let (was, now) = (pixel(&plain, x, y), pixel(&selected, x, y));
        assert!(tint(now) - tint(was) > 20, "{now:?} for {was:?} at {at}");
    }
}

/// From the top, through [`top_camera`], a box whose top face is level
/// with the grid's plane, its edge along y = -2.05 drawn along the middle
/// of row 84 in [`SKETCH_VIEW`], from x = -6 at column 68 to x = 6; and
/// that edge's id.
fn box_under_top_camera() -> (RenderMesh, u32) {
    let mesh = block(Vec3::new(-6.0, -2.05, -4.0), Vec3::new(12.0, 4.1, 4.0));
    let edge = mesh.polylines().position(|polyline| {
        polyline.iter().all(|&v| {
            let [_, y, z] = mesh.positions()[v as usize];
            (y + 2.05).abs() < 1e-4 && z.abs() < 1e-4
        })
    });
    (mesh, edge.unwrap() as u32)
}

/// What draws `highlights` over the box from the top, its grid hidden.
fn highlighted(highlights: Highlights) -> Extras {
    Extras {
        highlights,
        grid: hidden_grid(),
        ..Extras::default()
    }
}

#[test]
fn an_outline_is_as_wide_beside_a_face_rising_towards_the_eye() {
    // A box standing on the grid's plane, seen 25 degrees from the top:
    // its front face rises from its bottom edge towards the eye, seen that
    // steeply. The edge's rim is as wide on that face as on the plane
    // below, rather than hidden by the face beyond its middle.
    let mesh = block(Vec3::new(-6.0, -2.05, 0.0), Vec3::new(12.0, 4.1, 4.0));
    let edge = mesh.polylines().position(|polyline| {
        polyline.iter().all(|&v| {
            let [_, y, z] = mesh.positions()[v as usize];
            (y + 2.05).abs() < 1e-4 && z.abs() < 1e-4
        })
    });
    let edge = edge.unwrap() as u32;
    let mut camera = top_camera();
    camera.orbit(0.0, -25f32.to_radians());
    let render = |outlined| {
        let extras = highlighted(Highlights {
            outlined,
            ..Highlights::default()
        });
        render_sketch(&camera, &mesh, extras, 1.0)
    };
    let (Some(plain), Some(hovered)) = (render(vec![]), render(vec![edge])) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let up = (Vec3::new(0.0, -2.05, 0.0) - camera.target()).dot(camera.up());
    let row = (SKETCH_VIEW[1] * (0.5 - up / camera.view_height())) as u32;
    // The rim's colour, full, the same either side.
    let green = |y| pixel(&hovered, 128, y) == [191, 255, 153, 255];
    let rims = [
        (row - 6..row).filter(|&y| green(y)).count(),
        (row + 1..row + 7).filter(|&y| green(y)).count(),
    ];
    assert!(rims[0] > 0 && rims[0] == rims[1], "{rims:?} at {row}");
    assert_ne!(pixel(&plain, 128, row - 3), pixel(&plain, 128, row + 3));
}

#[test]
fn outlined_edges_are_drawn_wider_within_a_bright_rim() {
    // From the top, the box's top face's corner at x = -6.05, y = -2.05
    // in the middle of pixel (67, 84), its edges along row 84 and column
    // 67, both outlined with the face's other two: their middles are in
    // the edges' colour about as an opaque body's are, near the corner too,
    // where the other's rim would reach them, and on a body at 30 % as
    // well; the face's pixel beside it is darker than unhovered, as the
    // edge is wider. The rim shows outside them, and nothing past it.
    let mesh = block(Vec3::new(-6.05, -2.05, -4.0), Vec3::new(12.1, 4.1, 4.0));
    let top = face_facing(&mesh, Vec3::Z);
    let camera = top_camera();
    let bordering: Vec<u32> = (0..mesh.edge_count() as u32)
        .filter(|&edge| {
            let [a, b] = mesh.edge_faces()[edge as usize];
            a != b && (a == top || b == top)
        })
        .collect();
    assert_eq!(bordering.len(), 4);
    for opacity in [1.0, 0.3] {
        let render = |outlined: Vec<u32>| {
            let extras = Extras {
                opacity: vec![opacity],
                ..highlighted(Highlights {
                    outlined,
                    ..Highlights::default()
                })
            };
            render_sketch(&camera, &mesh, extras, 1.0)
        };
        let (Some(plain), Some(hovered)) = (render(vec![]), render(bordering.clone())) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let Some(edge) = render_sketch(&camera, &mesh, Extras::default(), 1.0) else {
            return;
        };
        for k in 0..12 {
            for (x, y) in [(67 + k, 84), (67, 84 - k)] {
                let (drawn, opaque) = (pixel(&hovered, x, y), pixel(&edge, x, y));
                assert!(
                    brighter(drawn, opaque).abs() < 30,
                    "({x}, {y}) at {opacity}: {drawn:?} for {opaque:?}"
                );
            }
        }
        // On the face, not the black background below.
        let (wider, was) = (pixel(&hovered, 120, 83), pixel(&plain, 120, 83));
        assert!(
            brighter(was, wider) > 50,
            "{wider:?} for {was:?} at {opacity}"
        );
        // Away from the corner, the rim either side of the edge along row
        // 84 is brighter than what's there unhovered.
        for y in [82, 86] {
            let (rim, was) = (pixel(&hovered, 120, y), pixel(&plain, 120, y));
            assert!(brighter(rim, was) > 60, "{rim:?} for {was:?} at {opacity}");
        }
        let reach = (HOVERED_EDGE_WIDTH / 2.0 + HOVER_RIM + 1.0).ceil() as u32;
        for y in [84 - reach, 84 + reach] {
            assert_eq!(pixel(&hovered, 120, y), pixel(&plain, 120, y), "row {y}");
        }
    }
}

#[test]
fn a_selected_edge_is_drawn_in_the_selection_colour_shaded() {
    // In the selection's colour, and darker or lighter by the shade,
    // within a faint white rim either side.
    let (mesh, edge) = box_under_top_camera();
    let unselected = render_sketch(
        &top_camera(),
        &mesh,
        highlighted(Highlights::default()),
        1.0,
    );
    let render = |selected_edge_shade| {
        let extras = Extras {
            colors: Some(Colors {
                selected_edge_shade,
                ..COLORS
            }),
            ..highlighted(Highlights {
                selected_edges: vec![edge],
                ..Highlights::default()
            })
        };
        render_sketch(&top_camera(), &mesh, extras, 1.0)
    };
    let (Some(unselected), Some(plain), Some(darker), Some(lighter)) =
        (unselected, render(0.0), render(-0.6), render(0.6))
    else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    for x in [90, 150, 180] {
        let core = pixel(&plain, x, 84);
        assert!(tint(core) > 60 && core[1] > 100, "{core:?} at {x}");
        let (dark, light) = (pixel(&darker, x, 84), pixel(&lighter, x, 84));
        assert!(brighter(core, dark) > 100, "{dark:?} for {core:?}");
        assert!(brighter(light, core) > 100, "{light:?} for {core:?}");
        for y in [82, 86] {
            let (rim, was) = (pixel(&plain, x, y), pixel(&unselected, x, y));
            assert!(brighter(rim, was) > 60, "{rim:?} for {was:?} at ({x}, {y})");
        }
    }
}

#[test]
fn a_hovered_or_selected_vertex_is_round() {
    // The box's corner at (-6, -2.05, 0), at (68, 84.5) on the screen.
    let (mesh, _) = box_under_top_camera();
    let corner = mesh
        .corners()
        .iter()
        .position(|&[x, y, z]| x == -6.0 && (y + 2.05).abs() < 1e-4 && z == 0.0)
        .unwrap() as u32;
    let camera = top_camera();
    let render = |vertices| {
        let extras = highlighted(Highlights {
            vertices,
            ..Highlights::default()
        });
        render_sketch(&camera, &mesh, extras, 1.0)
    };
    let vertex = |hovered, selected| Vertex {
        corner,
        hovered,
        selected,
    };
    let (Some(plain), Some(hovered), Some(selected)) = (
        render(vec![]),
        render(vec![vertex(true, false)]),
        render(vec![vertex(false, true)]),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let center = glam::Vec2::new(68.0, 84.5);
    // Hovered, its middle is the edges' colour, as the corner was, so
    // only its rim must show; selected, all of it is the selection's.
    for (drawn, inner, rim) in [(&hovered, VERTEX_RADIUS, HOVER_RIM), (&selected, 0.0, 1.0)] {
        let outer = VERTEX_RADIUS + rim;
        for y in 74..96 {
            for x in 58..80 {
                let distance = (glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - center).length();
                let changed = pixel(drawn, x, y) != pixel(&plain, x, y);
                // Within the disc it's drawn, and past it, even along the
                // diagonal, nothing is: it's round.
                if (inner + 0.75..outer - 0.75).contains(&distance) {
                    assert!(changed, "({x}, {y}) at {distance} not drawn");
                } else if distance > outer + 0.75 {
                    assert!(!changed, "({x}, {y}) at {distance} drawn");
                }
            }
        }
    }
    let middle = |pixels: &[[u8; 4]]| pixel(pixels, 68, 84);
    assert!(
        brighter(middle(&hovered), [31, 33, 38, 255]).abs() < 30,
        "{:?}",
        middle(&hovered)
    );
    assert!(
        pixel(&hovered, 68, 80)[1] > 200,
        "{:?}",
        pixel(&hovered, 68, 80)
    );
    assert!(tint(middle(&selected)) > 60, "{:?}", middle(&selected));
}

/// What's hovered and selected of the faded model behind a sketch is
/// drawn over it: what a sketch's Project or Intersect picks of it.
#[test]
fn hover_and_selection_are_drawn_over_the_faded_model() {
    let (mesh, edge) = box_under_top_camera();
    let camera = top_camera();
    let face = face_facing(&mesh, Vec3::Z);
    let render = |picked: bool| {
        let extras = Extras {
            faded: true,
            hovered_faces: picked.then_some(face).into_iter().collect(),
            selected_faces: if picked { vec![face] } else { vec![] },
            ..highlighted(if picked {
                Highlights {
                    outlined: vec![edge],
                    selected_edges: vec![edge],
                    second_edges: vec![],
                    vertices: vec![Vertex {
                        corner: 0,
                        hovered: true,
                        selected: true,
                    }],
                }
            } else {
                Highlights::default()
            })
        };
        render_sketch(&camera, &mesh, extras, 1.0)
    };
    let (Some(plain), Some(picked)) = (render(false), render(true)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert!(picked != plain);
}

#[test]
fn faces_and_edges_in_the_second_colour_are_drawn_in_it() {
    // The cube's front face in the second colour (orange in the tests'
    // colours) is tinted towards it, over a selection of it too; the
    // box's edge in it is drawn in it, apart from a selected one.
    let (camera, near, _) = cube_behind_cube();
    let front = face_facing(&near, -Vec3::Y);
    let faces = |selected: Vec<u32>, second: Vec<u32>| Extras {
        selected_faces: selected,
        second_faces: second,
        hover_through: false,
        ..Extras::default()
    };
    let (Some(plain), Some(second), Some(over)) = (
        render_with(&camera, &near, faces(vec![], vec![])),
        render_with(&camera, &near, faces(vec![], vec![front])),
        render_with(&camera, &near, faces(vec![front], vec![front])),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let [plain, second, over] = [&plain, &second, &over].map(|p| pixel(p, cx, cy));
    assert!(tint(plain).abs() < 4, "{plain:?}");
    assert!(tint(second) < -40, "{second:?}");
    assert!(tint(over) < -20, "{over:?} over the selection");

    // The selection's edges shaded far towards white, as the dark theme
    // has them: the second colour's aren't, so they keep their hue.
    let (mesh, edge) = box_under_top_camera();
    let edges = |highlights| {
        let extras = Extras {
            colors: Some(Colors {
                selected_edge_shade: 0.85,
                ..COLORS
            }),
            ..highlighted(highlights)
        };
        render_sketch(&top_camera(), &mesh, extras, 1.0)
    };
    let (Some(second), Some(selected)) = (
        edges(Highlights {
            second_edges: vec![edge],
            ..Highlights::default()
        }),
        edges(Highlights {
            selected_edges: vec![edge],
            ..Highlights::default()
        }),
    ) else {
        return;
    };
    for x in [90, 150, 180] {
        let (b, a) = (pixel(&second, x, 84), pixel(&selected, x, 84));
        assert!(tint(b) < -150, "{b:?} at {x}");
        assert!(brighter(a, b) > 100, "{a:?} for {b:?} at {x}");
    }
}

#[test]
fn a_preview_is_the_model_alone_on_nothing() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mesh = Arc::new(block(Vec3::ZERO, Vec3::new(4.0, 1.0, 1.0)));
    let margin = 4;
    let shot = varde_render::frame(
        &mesh,
        &RenderLines::default(),
        &Camera::default(),
        [300, 200],
        margin,
    )
    .unwrap();
    // Drawn once in each set of colours: the grey model, and a red one.
    let red = Colors {
        model: Srgb([0.8, 0.2, 0.2]),
        ..COLORS
    };
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let (send, read) = std::sync::mpsc::channel();
        varde_render::render_preview(
            &renderer(&device, format),
            &device,
            &queue,
            &mesh,
            &Arc::new(RenderLines::default()),
            &[],
            &[],
            &shot,
            &[COLORS, red],
            2.0,
            // The tests share the device: another's poll may call it.
            move |images| {
                let _ = send.send(images);
            },
        )
        .unwrap();
        let images = read
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
            .unwrap();
        assert_eq!(images.len(), 2, "{format:?}");
        for (i, image) in images.iter().enumerate() {
            assert_eq!([image.width, image.height], shot.size);
            let pixels = bytemuck_pixels(&image.rgba);
            let (width, height) = (image.width, image.height);
            let at = |x: u32, y: u32| pixels[(y * width + x) as usize];
            // Nothing drawn in the corners, nor anywhere in the margins
            // but where the outline's edges reach into them (half their
            // width and their smoothing, under 2.5 pixels at this scale):
            // no background, grid or markers.
            for (x, y) in [
                (0, 0),
                (width - 1, 0),
                (0, height - 1),
                (width - 1, height - 1),
            ] {
                assert_eq!(at(x, y), [0; 4], "{format:?} {i} at {x}, {y}");
            }
            let clear = margin - 2;
            let outside = |x: u32, y: u32| {
                x < clear || y < clear || x >= width - clear || y >= height - clear
            };
            for y in 0..height {
                for x in 0..width {
                    if outside(x, y) {
                        assert_eq!(at(x, y)[3], 0, "{format:?} {i} at {x}, {y}");
                    }
                }
            }
            // The model opaque in the middle, lit, in its colour: grey,
            // then red.
            let middle = at(width / 2, height / 2);
            assert_eq!(middle[3], 255, "{format:?} {i}: {middle:?}");
            assert!(middle[0] > 20, "{format:?} {i}: {middle:?}");
            let grey = middle[0].abs_diff(middle[2]) < 8;
            assert_eq!(grey, i == 0, "{format:?} {i}: {middle:?}");
            // It reaches out to the margins either way across.
            let row = height / 2;
            assert!((0..margin + 2).any(|x| at(x, row)[3] > 0), "{format:?} {i}");
            assert!(
                (width - margin - 2..width).any(|x| at(x, row)[3] > 0),
                "{format:?} {i}"
            );
        }
    }
}

#[test]
fn a_sketch_axis_through_the_origin_lies_on_the_grid_axis() {
    // The sketch's X axis drawn as the view crate draws it, as far as a
    // sketch reaches either way in two segments out from its origin, lies
    // on the grid's axis line zoomed in, either side of the origin, in
    // either projection. A segment starting that far away was pixels off.
    let plane = GridPlane::new(Vec3::new(40.0, 25.0, 30.0), Vec3::X, Vec3::Y).unwrap();
    let reach = f64::from(varde_kernel::MAX_COORD);
    let halves = [-reach, reach].map(|end| [glam::DVec2::ZERO, glam::DVec2::new(end, 0.0)]);
    // The whole target, large enough for the error to show.
    let [width, height] = SIZE;
    let viewport = Viewport {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let clip = ClipRect {
        x: 0,
        y: 0,
        width,
        height,
    };
    // The mean row of the pixels `matches` picks in each column.
    let rows = |pixels: &[[u8; 4]], matches: fn([u8; 4]) -> bool| -> Vec<Option<f32>> {
        (0..width)
            .map(|x| {
                let ys: Vec<u32> = (0..height)
                    .filter(|&y| matches(pixel(pixels, x, y)))
                    .collect();
                (!ys.is_empty()).then(|| ys.iter().sum::<u32>() as f32 / ys.len() as f32)
            })
            .collect()
    };
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = Camera::default();
        camera.set_projection(projection);
        camera.set_target(plane.origin());
        camera.zoom(1.0 / camera.view_height());
        let mut layer = SketchLayer::default();
        let style = LineStyle {
            color: YELLOW,
            width: 1.0,
            dash: None,
        };
        for half in &halves {
            layer.polyline(Space::Sketch, half, style);
        }
        let extras = |layer| Extras {
            grid: plane,
            sketch: Some((plane, layer)),
            ..Extras::default()
        };
        let mesh = RenderMesh::default();
        let render = |layer| render_scaled(&camera, &mesh, extras(layer), viewport, clip, 1.0);
        let (Some(grid), Some(sketch)) = (render(SketchLayer::default()), render(layer)) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let red = rows(&grid, |[r, g, _, _]| r > 128 && g < 100);
        let drawn = rows(&sketch, yellow);
        let offs: Vec<f32> = red
            .iter()
            .zip(&drawn)
            .filter_map(|(r, d)| Some((r.as_ref()? - d.as_ref()?).abs()))
            .collect();
        assert!(offs.len() > 200, "{projection:?}: {} columns", offs.len());
        let worst = offs.iter().copied().fold(0.0, f32::max);
        assert!(worst <= 1.0, "{projection:?}: {worst} pixels off");
    }
}

#[test]
fn a_sketch_axis_fades_with_the_grid_axis_it_lies_on() {
    // A sketch's axes drawn as axis polylines, from the front with the
    // sketch on XY: its Y axis points at the camera and is gone, nearly
    // so too, its X axis stays. Tilted further, the Y axis is back.
    let reach = f64::from(varde_kernel::MAX_COORD);
    let mut layer = SketchLayer::default();
    for (along, color) in [(glam::DVec2::X, YELLOW), (glam::DVec2::Y, BLUE)] {
        let style = LineStyle {
            color,
            width: 2.0,
            dash: None,
        };
        for end in [-reach, reach] {
            layer.axis_polyline(Space::Sketch, &[glam::DVec2::ZERO, along * end], style);
        }
    }
    let blue = |[r, g, b, _]: [u8; 4]| b > 150 && r < 100 && g < 100;
    let counts = |pitch: f32, projection| {
        let mut camera = Camera::default();
        camera.set_projection(projection);
        camera.look_from(View::Front);
        camera.orbit(0.0, pitch);
        let extras = Extras {
            sketch: Some((GridPlane::XY, layer.clone())),
            ..Extras::default()
        };
        let pixels = render_with(&camera, &RenderMesh::default(), extras)?;
        let (mut yellows, mut blues) = (0, 0);
        for y in CLIP.y..CLIP.y + CLIP.height {
            for x in CLIP.x..CLIP.x + CLIP.width {
                let p = pixel(&pixels, x, y);
                yellows += u32::from(yellow(p));
                blues += u32::from(blue(p));
            }
        }
        Some((yellows, blues))
    };
    for projection in [Projection::Orthographic, Projection::Perspective] {
        for pitch in [0.0, 0.02] {
            let Some((yellows, blues)) = counts(pitch, projection) else {
                eprintln!("no GPU adapter, skipping");
                return;
            };
            assert!(yellows > 100, "{projection:?} at {pitch}: {yellows} yellow");
            assert_eq!(blues, 0, "{projection:?} at {pitch}");
        }
        let (yellows, blues) = counts(0.3, projection).unwrap();
        assert!(
            yellows > 100 && blues > 30,
            "{projection:?}: {yellows} yellow, {blues} blue"
        );
    }
}

/// A part with a tint is drawn in the model's colour with its hue and
/// saturation, opaque or not, and only that part: the others keep the
/// model's.
#[test]
fn a_tinted_part_is_drawn_in_its_colour() {
    use varde_render::BodyTint;
    let (camera, _, both) = cube_behind_cube();
    let red = Some(BodyTint {
        hue: 0.0,
        saturation: 0.45,
    });
    // The near cube is the second part.
    let with = |tints: Vec<Option<BodyTint>>, opacity: Vec<f32>| Extras {
        tints,
        opacity,
        ..Extras::default()
    };
    let (Some(plain), Some(near), Some(far), Some(glass)) = (
        render_with(&camera, &both, Extras::default()),
        render_with(&camera, &both, with(vec![None, red], vec![])),
        render_with(&camera, &both, with(vec![red], vec![])),
        render_with(&camera, &both, with(vec![None, red], vec![1.0, 0.6])),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let (cx, cy) = CENTER;
    let [plain, near, far, glass] = [plain, near, far, glass].map(|p| pixel(&p, cx, cy));
    let reddish = |[r, _, b, _]: [u8; 4]| i32::from(r) - i32::from(b);
    assert!(reddish(plain).abs() < 8, "{plain:?}");
    assert!(reddish(near) > 40, "{near:?}");
    // The far cube is hidden behind the near one, drawn as before.
    assert_eq!(far, plain);
    assert!(reddish(glass) > 20, "{glass:?}");
}

/// Tinting keeps the colour's lightness: no saturation is grey of it, and
/// the hue sets which channel leads.
#[test]
fn tinting_keeps_the_lightness() {
    use varde_render::BodyTint;
    let model = Srgb([0.7, 0.72, 0.75]);
    let lightness = |Srgb([r, g, b]): Srgb| (r.max(g).max(b) + r.min(g).min(b)) / 2.0;
    let grey = model.tinted(BodyTint {
        hue: 120.0,
        saturation: 0.0,
    });
    let l = lightness(model);
    for c in grey.0 {
        assert!((c - l).abs() < 1e-5, "{grey:?}");
    }
    for (hue, lead) in [(0.0, 0), (120.0, 1), (240.0, 2), (360.0, 0)] {
        let tinted = model.tinted(BodyTint {
            hue,
            saturation: 0.45,
        });
        assert!((lightness(tinted) - l).abs() < 1e-5, "{tinted:?}");
        let most = (0..3)
            .max_by(|&a, &b| tinted.0[a].total_cmp(&tinted.0[b]))
            .unwrap();
        assert_eq!(most, lead, "{hue}: {tinted:?}");
    }
    // As vivid on a pale model as on a dark one: the same chroma.
    let chroma = |Srgb([r, g, b]): Srgb| r.max(g).max(b) - r.min(g).min(b);
    let tint = BodyTint {
        hue: 200.0,
        saturation: 0.45,
    };
    let pale = Srgb([0.83, 0.82, 0.87]).tinted(tint);
    let dark = Srgb([0.42, 0.39, 0.49]).tinted(tint);
    assert!(
        (chroma(pale) - chroma(dark)).abs() < 1e-5,
        "{pale:?} {dark:?}"
    );
    // Out of range is clamped, NaN grey.
    let nan = model.tinted(BodyTint {
        hue: f32::NAN,
        saturation: f32::NAN,
    });
    assert_eq!(nan, grey);
}

#[test]
fn a_preview_of_sketch_lines_alone_draws_them() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let mesh = Arc::new(RenderMesh::default());
    let sketches = Arc::new(lines(&[
        (Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)),
        (Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 3.0, 0.0)),
    ]));
    let shot = varde_render::frame(&mesh, &sketches, &Camera::default(), [300, 200], 6).unwrap();
    let (send, read) = std::sync::mpsc::channel();
    varde_render::render_preview(
        &renderer(&device, wgpu::TextureFormat::Rgba8Unorm),
        &device,
        &queue,
        &mesh,
        &sketches,
        &[],
        &[],
        &shot,
        &[COLORS],
        2.0,
        move |images| {
            let _ = send.send(images);
        },
    )
    .unwrap();
    let images = read
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .unwrap();
    let image = &images[0];
    assert_eq!([image.width, image.height], shot.size);
    let drawn = image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > 128)
        .count();
    assert!(drawn > 100, "{drawn} pixels drawn");
    // The corners stay clear: no background or grid.
    assert_eq!(image.rgba[3], 0);
}
