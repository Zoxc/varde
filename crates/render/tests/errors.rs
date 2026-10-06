//! Error geometry ([`Frame::errors`]) drawn offscreen: its red core, the
//! halo around it, and how the model hiding it dims it.
// Holding the shared wgpu device in a `static` asks whether it's `Sync`
// deeper than the default limit.
#![recursion_limit = "256"]

use std::any::Any;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use glam::{DVec2, DVec3, Vec3};
use varde_kernel::{MeshParts, RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::OriginShown;
use varde_render::{
    Camera, ClipRect, Colors, ERROR_EDGE_WIDTH, ERROR_HALO, ERROR_POINT_RADIUS, ErrorParts, Frame,
    GridPlane, Highlights, LineStyle, Projection, Renderer, Shading, SketchLayer, SketchScene,
    Slot, Space, Srgb, Srgba, View, Viewport, wgpu,
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [256, 256];

/// A black background, a grey model, and errors in pure red within a halo
/// of it at 0.3: over black, a pixel the halo covers whole is 0.3 red.
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
    second: Srgb([1.0, 0.5, 0.0]),
    error: Srgb([1.0, 0.0, 0.0]),
    error_halo: Srgba([1.0, 0.0, 0.0, 0.3]),
};

/// The red of a pixel the halo alone covers whole, over black.
const HALO_RED: u8 = 77;

static NO_HIGHLIGHTS: LazyLock<Arc<Highlights>> = LazyLock::new(Arc::default);
static NO_LINES: LazyLock<Arc<RenderLines>> = LazyLock::new(Arc::default);
static NO_MESH: LazyLock<Arc<RenderMesh>> = LazyLock::new(Arc::default);

// The binary's first GPU test pays for the device and the shared renderer's
// pipelines (about 0.5-2s in a debug build, more for each further texture
// format): a floor shared by every test here, so over the 0.5s aim.
/// The one device the tests share, if there's an adapter: see
/// `viewport.rs`.
fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    static DEVICE: OnceLock<Option<(wgpu::Device, wgpu::Queue)>> = OnceLock::new();
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

/// The renderer the tests share, made once.
fn renderer(device: &wgpu::Device) -> Arc<Renderer> {
    static RENDERER: Mutex<Option<Arc<Renderer>>> = Mutex::new(None);
    let mut renderer = RENDERER.lock().unwrap();
    renderer
        .get_or_insert_with(|| Arc::new(Renderer::new(device, FORMAT)))
        .clone()
}

/// A failure's parts, as the app holds them: behind an `Arc` that stays
/// the same while they do.
struct Failure {
    mesh: RenderMesh,
    lines: RenderLines,
    points: Vec<[f32; 3]>,
    source: Arc<dyn Any + Send + Sync>,
}

impl Failure {
    fn new(mesh: RenderMesh, lines: RenderLines, points: &[Vec3]) -> Failure {
        Failure {
            mesh,
            lines,
            points: points.iter().map(|p| p.to_array()).collect(),
            source: Arc::new(0u8),
        }
    }

    fn lines(segments: &[[Vec3; 2]]) -> Failure {
        let mut lines = RenderLines::default();
        for &segment in segments {
            lines.push(segment).unwrap();
        }
        Failure::new(RenderMesh::default(), lines, &[])
    }

    fn parts(&self) -> ErrorParts<'_> {
        ErrorParts {
            mesh: &self.mesh,
            lines: &self.lines,
            points: &self.points,
            source: Arc::downgrade(&self.source),
            halo_only: false,
        }
    }
}

/// A square patch from `min` to `max` at z = `z`, facing up, and its
/// boundary as a closed curve.
fn patch(min: [f32; 2], max: [f32; 2], z: f32) -> Failure {
    let corners = [
        Vec3::new(min[0], min[1], z),
        Vec3::new(max[0], min[1], z),
        Vec3::new(max[0], max[1], z),
        Vec3::new(min[0], max[1], z),
    ];
    let mesh = RenderMesh::from_parts(MeshParts {
        positions: corners.map(|c| c.to_array()).to_vec(),
        normals: vec![[0.0, 0.0, 1.0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
        face_ends: vec![6],
        part_ends: vec![[1, 0, 0, 0]],
        ..MeshParts::default()
    })
    .unwrap();
    let mut lines = RenderLines::default();
    lines
        .push([corners[0], corners[1], corners[2], corners[3], corners[0]])
        .unwrap();
    Failure::new(mesh, lines, &[])
}

/// Looking straight down with the origin in the middle, 25.6 units across
/// the view's height: a unit is 10 pixels, and the world point (x, y) is
/// at pixel (128 + 10 x, 128 - 10 y).
fn top_camera() -> Camera {
    let mut camera = Camera::default();
    camera.look_from(View::Top);
    camera.zoom(25.6 / camera.view_height());
    camera
}

/// A grid that doesn't show: its plane is seen edge on from the top.
fn hidden_grid() -> GridPlane {
    GridPlane::new(Vec3::new(0.0, 1000.0, 0.0), Vec3::X, Vec3::Z).unwrap()
}

/// A frame of `mesh` and `errors` from [`top_camera`] filling the target.
fn frame<'a>(
    camera: &'a Camera,
    mesh: &'a Arc<RenderMesh>,
    errors: &'a [ErrorParts<'a>],
    faded: bool,
) -> Frame<'a> {
    let [width, height] = SIZE.map(|s| s as f32);
    Frame {
        camera,
        mesh,
        opacity: &[],
        tints: &[],
        sketches: &NO_LINES,
        grid: hidden_grid(),
        faded,
        wireframe: false,
        tessellation: false,
        shading: Shading::Regular,
        hidden_edges: false,
        hovered_faces: &[],
        selected_faces: &[],
        second_faces: &[],
        hover_through: false,
        highlights: &NO_HIGHLIGHTS,
        errors,
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
            width,
            height,
        },
        target_size: SIZE,
        scale_factor: 1.0,
        colors: COLORS,
    }
}

/// A slot of the shared renderer, drawn into again and again.
struct Scene {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<Renderer>,
    slot: Slot,
}

impl Scene {
    /// `None` if there's no adapter.
    fn new() -> Option<Scene> {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter, skipping");
            return None;
        };
        let renderer = renderer(&device);
        let slot = renderer.slot(&device);
        Some(Scene {
            device,
            queue,
            renderer,
            slot,
        })
    }

    /// Prepares and draws `frame` into a target of its size, and returns
    /// its pixels, row-major.
    fn draw(&mut self, frame: &Frame<'_>) -> Pixels {
        let (device, queue) = (&self.device, &self.queue);
        let [width, height] = frame.target_size;
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
        (self.renderer)
            .prepare(&mut self.slot, device, queue, frame)
            .unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        let clip = ClipRect {
            x: 0,
            y: 0,
            width,
            height,
        };
        self.renderer.render(&self.slot, &mut encoder, &view, clip);
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
        Pixels(data.as_chunks::<4>().0.to_vec(), width)
    }
}

/// A target's pixels, row-major, and its width.
#[derive(Debug, PartialEq)]
struct Pixels(Vec<[u8; 4]>, u32);

impl Pixels {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        self.0[(y * self.1 + x) as usize]
    }
}

/// How much redder than green a pixel is.
fn redness([r, g, _, _]: [u8; 4]) -> i32 {
    i32::from(r) - i32::from(g)
}

/// Whether a pixel is `red` red, give or take a few, and nothing else.
fn red_of(pixel: [u8; 4], red: u8) -> bool {
    let [r, g, b, _] = pixel;
    r.abs_diff(red) <= 4 && g <= 2 && b <= 2
}

/// A horizontal line at y = -6 (on the boundary between rows 187 and 188)
/// from x = 2 to 10 (columns 148 to 228).
fn line_across() -> Failure {
    Failure::lines(&[[Vec3::new(2.0, -6.0, 0.0), Vec3::new(10.0, -6.0, 0.0)]])
}

#[test]
fn the_core_is_red() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let point = Failure::new(
        RenderMesh::default(),
        RenderLines::default(),
        &[Vec3::new(-6.05, 6.05, 0.0)],
    );
    let failures = [
        line_across(),
        point,
        patch([-10.0, -10.0], [-4.0, -4.0], 0.0),
    ];
    let errors: Vec<_> = failures.iter().map(Failure::parts).collect();
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    // The line's middle, both rows either side of it.
    for row in [187, 188] {
        assert_eq!(pixels.at(188, row), [255, 0, 0, 255], "row {row}");
    }
    // `ERROR_EDGE_WIDTH` wide: a row a pixel and a half out is half
    // covered, two out not.
    assert_eq!(ERROR_EDGE_WIDTH, 3.0);
    let half = pixels.at(188, 189);
    assert!(half[0] > 150 && half[0] < 255 && half[1] < 20, "{half:?}");
    // The point's disc, red across its radius.
    assert_eq!(ERROR_POINT_RADIUS, 4.0);
    for dx in [-2, 0, 2] {
        let x = (67 + dx) as u32;
        assert_eq!(pixels.at(x, 67), [255, 0, 0, 255], "x {x}");
    }
    // The patch filled red, lit: a little light in green and blue.
    let [r, g, b, _] = pixels.at(58, 198);
    assert!(r > 200 && g < 90 && b < 90, "{:?}", [r, g, b]);
}

#[test]
fn the_halo_reaches_error_halo_beyond_the_core_and_no_farther() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let line = line_across();
    let errors = [line.parts()];
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    assert_eq!(ERROR_HALO, 12.0);
    // Row `188 + k` and row `187 - k` are `k + 0.5` from the line: its
    // core reaches 1.5, the halo 12 more, the edges anti-aliased over a
    // pixel.
    for k in 0..30 {
        let (below, above) = (pixels.at(188, 188 + k), pixels.at(188, 187 - k));
        assert!(below[0].abs_diff(above[0]) <= 1, "{k}: {below:?} {above:?}");
        match k {
            0 => assert_eq!(below, [255, 0, 0, 255], "{k}"),
            // Half the core over the halo: (255 + 77) / 2.
            1 => assert!(red_of(below, 166), "{k}: {below:?}"),
            2..=12 => assert!(red_of(below, HALO_RED), "{k}: {below:?}"),
            13 => assert!(red_of(below, HALO_RED / 2), "{k}: {below:?}"),
            _ => assert_eq!(below, [0, 0, 0, 255], "{k}"),
        }
    }
    // Round ends: as far past the line's end, and no farther.
    assert!(red_of(pixels.at(228 + 10, 188), HALO_RED));
    assert_eq!(pixels.at(228 + 16, 188), [0, 0, 0, 255]);
}

#[test]
fn parts_drawn_as_their_halo_alone_have_no_core() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    // The line across as halo only, and a point on it drawn whole: the
    // line's own pixels take the halo, the point's disc its core.
    let line = line_across();
    let point = Failure::new(
        RenderMesh::default(),
        RenderLines::default(),
        &[Vec3::new(9.0, -6.0, 0.0)],
    );
    let errors = [
        ErrorParts {
            halo_only: true,
            ..line.parts()
        },
        point.parts(),
    ];
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    for row in [187, 188] {
        assert!(red_of(pixels.at(168, row), HALO_RED), "row {row}");
    }
    assert!(red_of(pixels.at(168, 196), HALO_RED));
    assert_eq!(pixels.at(168, 204), [0, 0, 0, 255]);
    assert_eq!(pixels.at(218, 188), [255, 0, 0, 255]);
}

#[test]
fn a_points_halo_reaches_as_far_beyond_its_disc() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    // At pixel (188, 188), between four pixels.
    let point = Failure::new(
        RenderMesh::default(),
        RenderLines::default(),
        &[Vec3::new(6.0, -6.0, 0.0)],
    );
    let errors = [point.parts()];
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    // A pixel's centre `k + 0.5` from the point, along its row.
    let at = |k: u32| pixels.at(188 + k, 188);
    assert_eq!(at(2), [255, 0, 0, 255]);
    for k in 5..=14 {
        assert!(red_of(at(k), HALO_RED), "{k}: {:?}", at(k));
    }
    assert_eq!(at(17), [0, 0, 0, 255]);
}

#[test]
fn overlapping_halos_dont_darken() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    // Two lines 8 pixels apart, a point on the first, and a patch with
    // its boundary, whose halos overlap.
    let lines = Failure::lines(&[
        [Vec3::new(2.0, -6.0, 0.0), Vec3::new(10.0, -6.0, 0.0)],
        [Vec3::new(2.0, -6.8, 0.0), Vec3::new(10.0, -6.8, 0.0)],
    ]);
    let point = Failure::new(
        RenderMesh::default(),
        RenderLines::default(),
        &[Vec3::new(6.0, -6.0, 0.0)],
    );
    let failures = [lines, point, patch([-10.0, -10.0], [-4.0, -4.0], 0.0)];
    let errors: Vec<_> = failures.iter().map(Failure::parts).collect();
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    // Between the lines, 4 pixels from each, and off to the side of the
    // point where its halo and both lines' meet.
    for (x, y) in [(160, 191), (200, 191), (196, 182), (196, 200)] {
        assert!(
            red_of(pixels.at(x, y), HALO_RED),
            "({x}, {y}): {:?}",
            pixels.at(x, y)
        );
    }
    // Outside the patch (pixels 28 to 88 each way), beside its sides and
    // off its corners, where its boundary's segments' halos meet.
    for (x, y) in [(22, 198), (58, 162), (22, 162), (94, 234)] {
        assert!(
            red_of(pixels.at(x, y), HALO_RED),
            "({x}, {y}): {:?}",
            pixels.at(x, y)
        );
    }
}

#[test]
fn overlapping_hidden_halos_dont_add_up() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let block = Arc::new(block());
    // Under the block: two lines 8 pixels apart, and one alone. Hidden,
    // each halo covers at about 40 %, which adding would double.
    let pair = Failure::lines(&[
        [Vec3::new(2.0, -3.0, -2.0), Vec3::new(10.0, -3.0, -2.0)],
        [Vec3::new(2.0, -3.8, -2.0), Vec3::new(10.0, -3.8, -2.0)],
    ]);
    let alone = Failure::lines(&[[Vec3::new(2.0, -9.0, -2.0), Vec3::new(10.0, -9.0, -2.0)]]);
    let failures = [pair, alone];
    let errors: Vec<_> = failures.iter().map(Failure::parts).collect();
    let pixels = scene.draw(&frame(&camera, &block, &errors, false));
    // Row 161 is 3.5 and 4.5 pixels from the pair, row 221 3.5 from the
    // one alone.
    let (between, beside) = (pixels.at(188, 161), pixels.at(188, 221));
    assert!(redness(beside) > 10, "{beside:?}");
    assert!(
        redness(between).abs_diff(redness(beside)) <= 2,
        "{between:?} {beside:?}"
    );
}

/// A block under the right half of the view, from (0, -12, 0) to
/// (12, 0, 4): pixels 128 to 248 each way.
fn block() -> RenderMesh {
    Solid::cuboid(
        DVec3::new(0.0, -12.0, 0.0),
        DVec3::new(12.0, 12.0, 4.0),
        0,
        &Tolerance::DEFAULT,
    )
    .unwrap()
    .tessellate(&varde_kernel::Display::default())
    .unwrap()
}

#[test]
fn a_hidden_part_is_dimmer() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    // A line over the block and one under it.
    let block = Arc::new(block());
    let seen = Failure::lines(&[[Vec3::new(2.0, -3.0, 6.0), Vec3::new(10.0, -3.0, 6.0)]]);
    let hidden = Failure::lines(&[[Vec3::new(2.0, -9.0, -2.0), Vec3::new(10.0, -9.0, -2.0)]]);
    let failures = [seen, hidden];
    let errors: Vec<_> = failures.iter().map(Failure::parts).collect();
    let pixels = scene.draw(&frame(&camera, &block, &errors, false));
    let none = scene.draw(&frame(&camera, &block, &[], false));
    // Rows 157 and 217 are half a pixel from the lines, 165 and 225 eight
    // and a half: the core and the halo.
    let face = none.at(188, 157);
    assert_eq!(face, none.at(188, 217), "the top is lit evenly");
    let (core, dim_core) = (pixels.at(188, 157), pixels.at(188, 217));
    assert_eq!(core, [255, 0, 0, 255]);
    assert!(
        redness(core) > redness(dim_core) + 60,
        "{core:?} {dim_core:?}"
    );
    assert!(
        redness(dim_core) > redness(face) + 40,
        "{dim_core:?} {face:?}"
    );
    let (halo, dim_halo) = (pixels.at(188, 165), pixels.at(188, 225));
    assert!(
        redness(halo) > redness(dim_halo) + 20,
        "{halo:?} {dim_halo:?}"
    );
    assert!(
        redness(dim_halo) > redness(face) + 10,
        "{dim_halo:?} {face:?}"
    );
}

#[test]
fn errors_are_drawn_over_the_faded_model() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let block = Arc::new(block());
    let seen = Failure::lines(&[[Vec3::new(2.0, -3.0, 6.0), Vec3::new(10.0, -3.0, 6.0)]]);
    let errors = [seen.parts()];
    let pixels = scene.draw(&frame(&camera, &block, &errors, true));
    assert_eq!(pixels.at(188, 157), [255, 0, 0, 255]);
}

#[test]
fn nothing_is_drawn_without_errors() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let block = Arc::new(
        Solid::cuboid(DVec3::ZERO, DVec3::splat(6.0), 0, &Tolerance::DEFAULT)
            .unwrap()
            .tessellate(&varde_kernel::Display::default())
            .unwrap(),
    );
    let none = scene.draw(&frame(&camera, &block, &[], false));
    // Errors with nothing to draw draw nothing.
    let empty = Failure::new(RenderMesh::default(), RenderLines::default(), &[]);
    let errors = [empty.parts()];
    assert_eq!(scene.draw(&frame(&camera, &block, &errors, false)), none);
    // And once some were drawn, none again leaves nothing of them.
    let failure = line_across();
    let errors = [failure.parts()];
    assert_ne!(scene.draw(&frame(&camera, &block, &errors, false)), none);
    assert_eq!(scene.draw(&frame(&camera, &block, &[], false)), none);
}

#[test]
fn errors_are_uploaded_again_only_when_their_sources_change() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let first = line_across();
    let errors = [first.parts()];
    let drawn = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    // Other parts from the same source are taken to be the same.
    let other = Failure::lines(&[[Vec3::new(-10.0, 6.0, 0.0), Vec3::new(-2.0, 6.0, 0.0)]]);
    let mut moved = other.parts();
    moved.source = Arc::downgrade(&first.source);
    let errors = [moved];
    assert_eq!(scene.draw(&frame(&camera, &NO_MESH, &errors, false)), drawn);
    // Another source is uploaded.
    let errors = [other.parts()];
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    assert_eq!(pixels.at(188, 188), [0, 0, 0, 255]);
    assert_eq!(pixels.at(68, 68), [255, 0, 0, 255]);
}

#[test]
fn overlapping_hidden_patches_dont_darken() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let block = Arc::new(block());
    // Under the block, two patches overlapping from (4, -8) to (8, -4):
    // hidden, each is drawn at about 40 %, which drawing both would add up.
    let failures = [
        patch([2.0, -10.0], [8.0, -4.0], -2.0),
        patch([4.0, -8.0], [10.0, -2.0], -1.0),
    ];
    let errors: Vec<_> = failures.iter().map(Failure::parts).collect();
    let pixels = scene.draw(&frame(&camera, &block, &errors, false));
    // Where they overlap, and in the first alone, far from their
    // boundaries.
    let (both, one) = (pixels.at(188, 188), pixels.at(155, 225));
    let none = scene.draw(&frame(&camera, &block, &[], false));
    assert!(redness(one) > redness(none.at(155, 225)) + 10, "{one:?}");
    assert!(
        both.iter().zip(one).all(|(a, b)| a.abs_diff(b) <= 1),
        "{both:?} {one:?}"
    );
}

#[test]
fn the_sketch_being_edited_is_drawn_over_the_errors() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    // A green line across the error's, both at z = 0.
    let mut layer = SketchLayer::default();
    let style = LineStyle {
        color: Srgba([0.0, 1.0, 0.0, 1.0]),
        width: 2.0,
        dash: None,
    };
    let ends = [DVec2::new(6.0, -2.0), DVec2::new(6.0, -10.0)];
    layer.polyline(Space::Sketch, &ends, style);
    let (base, live) = (Arc::new(layer), SketchLayer::default());
    let line = line_across();
    let errors = [line.parts()];
    let mut sketched = frame(&camera, &NO_MESH, &errors, true);
    sketched.sketch = Some(SketchScene {
        plane: GridPlane::XY,
        depth_tested: false,
        base: &base,
        live: &live,
    });
    let pixels = scene.draw(&sketched);
    // Where they cross, and in the error's halo.
    for y in [188, 195] {
        let [r, g, _, _] = pixels.at(188, y);
        assert!(g > 200 && r < 40, "row {y}: {:?}", pixels.at(188, y));
    }
}

#[test]
fn a_patch_before_the_near_plane_is_clipped() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let mut camera = top_camera();
    camera.set_projection(Projection::Perspective);
    // Between the eye and the near plane, across the line of sight: the
    // model's faces there are clipped, and so must the errors' be, rather
    // than cover the view.
    let z = camera.distance() - camera.near() * 0.5;
    let near = patch([-1.0, -1.0], [1.0, 1.0], z);
    let errors = [near.parts()];
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    let none = scene.draw(&frame(&camera, &NO_MESH, &[], false));
    assert_eq!(pixels.at(128, 128), none.at(128, 128));
    assert_eq!(pixels, none);
    // One crossing the near plane still shows beyond it: standing up
    // from z = 0 past the eye at y = -0.5, x = 0.5 to 1.5, below and right
    // of the middle, farther out the nearer the eye.
    let corners = [
        Vec3::new(0.5, -0.5, 0.0),
        Vec3::new(1.5, -0.5, 0.0),
        Vec3::new(1.5, -0.5, z + 1.0),
        Vec3::new(0.5, -0.5, z + 1.0),
    ];
    let mesh = RenderMesh::from_parts(MeshParts {
        positions: corners.map(|c| c.to_array()).to_vec(),
        normals: vec![[0.0, 1.0, 0.0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
        face_ends: vec![6],
        part_ends: vec![[1, 0, 0, 0]],
        ..MeshParts::default()
    })
    .unwrap();
    let crossing = Failure::new(mesh, RenderLines::default(), &[]);
    let errors = [crossing.parts()];
    let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
    // Half way to the eye, twice as far out: (138 to 158, 138).
    assert!(
        redness(pixels.at(148, 138)) > 100,
        "{:?}",
        pixels.at(148, 138)
    );
    assert_eq!(pixels.at(100, 138), none.at(100, 138));
}

/// The halo of [`line_across`] at `scale` physical pixels a logical
/// one: the core `ERROR_EDGE_WIDTH / 2` and the halo `ERROR_HALO` beyond
/// it, both scaled.
fn halo_at_scale(scale: f32) {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let line = line_across();
    let errors = [line.parts()];
    let pixels = scene.draw(&Frame {
        scale_factor: scale,
        ..frame(&camera, &NO_MESH, &errors, false)
    });
    let core = ERROR_EDGE_WIDTH * 0.5 * scale;
    let reach = core + ERROR_HALO * scale;
    // Row `188 + k` is `k + 0.5` from the line.
    for k in 0..40u32 {
        let d = k as f32 + 0.5;
        let below = pixels.at(188, 188 + k);
        if d + 0.5 <= core {
            assert_eq!(below, [255, 0, 0, 255], "scale {scale}, {k}");
        } else if d - 0.5 >= core && d + 0.5 <= reach {
            assert!(red_of(below, HALO_RED), "scale {scale}, {k}: {below:?}");
        } else if d - 0.5 >= reach {
            assert_eq!(below, [0, 0, 0, 255], "scale {scale}, {k}");
        }
    }
}

#[test]
fn the_halo_reaches_as_far_at_any_scale() {
    for scale in [1.0, 1.5, 2.0] {
        halo_at_scale(scale);
    }
}

#[test]
fn the_halo_is_read_where_the_viewport_is_as_the_target_is_resized() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let line = line_across();
    let errors = [line.parts()];
    // The viewport within a larger target, not at its origin: the line's
    // middle at (288, 238) on the boundary of rows 237 and 238.
    let at = |x, y, target_size| Frame {
        viewport: Viewport {
            x,
            y,
            width: 256.0,
            height: 256.0,
        },
        target_size,
        ..frame(&camera, &NO_MESH, &errors, false)
    };
    let pixels = scene.draw(&at(100.0, 50.0, [448, 350]));
    assert_eq!(pixels.at(288, 238), [255, 0, 0, 255]);
    for (x, y) in [(288, 246), (288, 229), (338, 238)] {
        assert!(red_of(pixels.at(x, y), HALO_RED), "({x}, {y})");
    }
    for (x, y) in [(288, 254), (288, 221), (188, 196), (188, 188)] {
        assert_eq!(pixels.at(x, y), [0, 0, 0, 255], "({x}, {y})");
    }
    // Resized while they show, then while none do, then shown again.
    let resized = scene.draw(&at(0.0, 0.0, SIZE));
    assert_eq!(resized.at(188, 188), [255, 0, 0, 255]);
    assert!(red_of(resized.at(188, 196), HALO_RED));
    let none = scene.draw(&Frame {
        errors: &[],
        ..at(20.0, 30.0, [320, 300])
    });
    assert_eq!(none.at(208, 218), [0, 0, 0, 255]);
    let again = scene.draw(&at(20.0, 30.0, [320, 300]));
    assert_eq!(again.at(208, 218), [255, 0, 0, 255]);
    assert!(red_of(again.at(208, 226), HALO_RED));
    assert_eq!(again.at(208, 234), [0, 0, 0, 255]);
}

#[test]
fn errors_far_from_the_model_are_in_the_depth_range() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let block = Arc::new(block());
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = top_camera();
        camera.set_projection(projection);
        // Far above the block (but below the eye) and far below it; in
        // perspective the line below shows nearer the middle.
        let z = camera.distance() * 0.5;
        let above = Failure::lines(&[[Vec3::new(2.0, -3.0, z), Vec3::new(10.0, -3.0, z)]]);
        let below = Failure::lines(&[[
            Vec3::new(-20.0, -9.0, -500.0),
            Vec3::new(500.0, -9.0, -500.0),
        ]]);
        let failures = [above, below];
        let errors: Vec<_> = failures.iter().map(Failure::parts).collect();
        let pixels = scene.draw(&frame(&camera, &block, &errors, false));
        let none = scene.draw(&frame(&camera, &block, &[], false));
        let changed = (0..SIZE[1])
            .filter(|&y| redness(pixels.at(200, y)) > redness(none.at(200, y)) + 10)
            .count();
        // Both lines and their halos, rows apart.
        let full = (0..SIZE[1])
            .filter(|&y| pixels.at(200, y) == [255, 0, 0, 255])
            .count();
        assert!(full >= 2, "{projection:?}: {full} rows of core");
        assert!(changed >= 40, "{projection:?}: {changed} rows changed");
        // Only errors, a point far off: drawn.
        let point = Failure::new(
            RenderMesh::default(),
            RenderLines::default(),
            &[Vec3::new(0.0, 0.0, -2000.0)],
        );
        let errors = [point.parts()];
        let pixels = scene.draw(&frame(&camera, &NO_MESH, &errors, false));
        assert_eq!(pixels.at(128, 128), [255, 0, 0, 255], "{projection:?}");
    }
}

#[test]
fn errors_are_drawn_with_glass_hidden_edges_and_wires() {
    let Some(mut scene) = Scene::new() else {
        return;
    };
    let camera = top_camera();
    let block = Arc::new(block());
    let seen = Failure::lines(&[[Vec3::new(2.0, -3.0, 6.0), Vec3::new(10.0, -3.0, 6.0)]]);
    let hidden = Failure::lines(&[[Vec3::new(2.0, -9.0, -2.0), Vec3::new(10.0, -9.0, -2.0)]]);
    let failures = [seen, hidden];
    let errors: Vec<_> = failures.iter().map(Failure::parts).collect();
    for (opacity, hidden_edges, wireframe) in [
        (&[0.5][..], false, false),
        (&[][..], true, false),
        (&[][..], false, true),
        (&[0.5][..], true, true),
    ] {
        let base = Frame {
            opacity,
            hidden_edges,
            wireframe,
            ..frame(&camera, &block, &errors, false)
        };
        let pixels = scene.draw(&base);
        let none = scene.draw(&Frame {
            errors: &[],
            ..base
        });
        let case = (opacity, hidden_edges, wireframe);
        assert_eq!(pixels.at(188, 157), [255, 0, 0, 255], "{case:?}");
        let (dim, face) = (pixels.at(188, 217), none.at(188, 217));
        assert!(
            redness(dim) > redness(face) + 40,
            "{case:?}: {dim:?} {face:?}"
        );
        assert!(redness(dim) < 200, "{case:?}: {dim:?}");
        // The halo over the block.
        let (halo, face) = (pixels.at(188, 165), none.at(188, 165));
        assert!(redness(halo) > redness(face) + 20, "{case:?}: {halo:?}");
    }
}
