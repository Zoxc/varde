//! Error geometry ([`Frame::errors`]) drawn offscreen: its red core, the
//! halo around it, and how the model hiding it dims it.
// Holding the shared wgpu device in a `static` asks whether it's `Sync`
// deeper than the default limit.
#![recursion_limit = "256"]

use std::any::Any;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use glam::{DVec2, DVec3, Vec3};
use varde_kernel::{MeshParts, RenderLines, RenderMesh, Solid, Tolerance};
use varde_render::{
    Camera, ClipRect, Colors, ERROR_EDGE_WIDTH, ERROR_HALO, ERROR_POINT_RADIUS, ErrorParts, Frame,
    GridPlane, Highlights, LineStyle, Renderer, SketchLayer, SketchScene, Slot, Space, Srgb, Srgba,
    View, Viewport, wgpu,
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [256, 256];

/// A black background, a grey model, and errors in pure red within a halo
/// of it at 0.3: over black, a pixel the halo covers whole is 0.3 red.
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

/// The one device the tests share, if there's an adapter: see
/// `viewport.rs`.
fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    static DEVICE: OnceLock<Option<(wgpu::Device, wgpu::Queue)>> = OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let instance = wgpu::Instance::default();
            let options = wgpu::RequestAdapterOptions::default();
            let adapter = pollster::block_on(instance.request_adapter(&options)).ok()?;
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
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
        sketches: &NO_LINES,
        grid: hidden_grid(),
        faded,
        wireframe: false,
        hidden_edges: false,
        hovered_faces: &[],
        selected_faces: &[],
        second_faces: &[],
        highlights: &NO_HIGHLIGHTS,
        errors,
        sketch: None,
        pivot: None,
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

    /// Prepares and draws `frame` into a target of [`SIZE`], and returns
    /// its pixels, row-major.
    fn draw(&mut self, frame: &Frame<'_>) -> Pixels {
        let (device, queue) = (&self.device, &self.queue);
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
        Pixels(data.as_chunks::<4>().0.to_vec())
    }
}

#[derive(Debug, PartialEq)]
struct Pixels(Vec<[u8; 4]>);

impl Pixels {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        self.0[(y * SIZE[0] + x) as usize]
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
