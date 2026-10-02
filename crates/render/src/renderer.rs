use std::ops::Range;
use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use varde_kernel::{Aabb, RenderLines, RenderMesh};
use wgpu::util::DeviceExt;

use crate::Camera;
use crate::highlight::{Highlight, HighlightVertex};
use crate::scene::{self, GRID_FADE_HEIGHTS, GridPlane};
use crate::sketch::{FillVertex, LineInstance, PointInstance, SketchLayer, SketchScene, Srgba};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A quad per marker, an instance each: the origin's and the pivot's. See
/// `vs_origin`.
const ORIGIN_VERTICES: u32 = 6;
const MARKERS: u32 = 2;

/// A quad per segment of a line, two triangles. See `line_vertex`.
const LINE_VERTICES: u32 = 6;

/// A quad per point of the sketch being edited. See `vs_point`.
const POINT_VERTICES: u32 = 6;

/// How wide [`Frame::sketches`] are drawn, in logical pixels.
pub const LINE_WIDTH: f32 = 1.5;

/// How wide the model's feature edges are drawn, in logical pixels.
pub const EDGE_WIDTH: f32 = 1.5;

/// How wide the edges the model hides are drawn, in logical pixels: see
/// [`Frame::hidden_edges`].
pub const HIDDEN_EDGE_WIDTH: f32 = 1.0;

/// The dashes of the edges the model hides: how long a dash and a gap are
/// along the edge, in logical pixels at the target.
pub const HIDDEN_DASH: [f32; 2] = [4.0, 3.0];

/// A rectangle on the render target, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    /// Width over height, at least a pixel each way, so it's positive and
    /// finite for the camera's projection however degenerate the viewport.
    pub fn aspect(&self) -> f32 {
        self.width.max(1.0) / self.height.max(1.0)
    }
}

/// A scissor rectangle on the render target, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClipRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Everything needed to draw one frame.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub camera: &'a Camera,
    /// Only re-uploaded when it's another `Arc` than the last one prepared.
    pub mesh: &'a Arc<RenderMesh>,
    /// How opaque each of the mesh's parts is drawn, from 0 to 1, in the
    /// order of [`RenderMesh::parts`]: its faces and its edges, hidden
    /// ones too. A part with no entry, or one out of range or NaN, is
    /// opaque; one within half a step of 8 bits of 1 too. Parts less than
    /// opaque are drawn after the opaque ones, far to near by the centres
    /// of their bounds, see [`Renderer::render`]. Ignored while
    /// [`Self::faded`]: every part is faded alike then. Changing it
    /// re-uploads nothing.
    pub opacity: &'a [f32],
    /// Finished sketches' curves, drawn with the model as lines
    /// [`LINE_WIDTH`] wide, hidden by what's in front of them. Only
    /// re-uploaded when it's another `Arc` than the last one prepared.
    pub sketches: &'a Arc<RenderLines>,
    /// The plane the grid is drawn on.
    pub grid: GridPlane,
    /// Whether the model is drawn faded, as it is behind a sketch being
    /// edited: see [`Colors::faded_alpha`].
    pub faded: bool,
    /// Whether the feature edges the model hides are drawn too, dashed
    /// ([`HIDDEN_DASH`]), [`HIDDEN_EDGE_WIDTH`] wide, at
    /// [`Colors::hidden_edge_alpha`]. Never while [`Self::faded`].
    pub hidden_edges: bool,
    /// The sketch being edited, or the extrude being set up, if one is:
    /// drawn over everything else, or hidden by the model in front of it
    /// ([`SketchScene::depth_tested`]).
    pub sketch: Option<SketchScene<'a>>,
    /// The point the camera orbits, if one was picked: marked over
    /// everything but the sketch being edited, unless it shows where the
    /// origin does.
    pub pivot: Option<Pivot>,
    /// The faces and edges of the model hovered and selected, drawn over
    /// it, hidden by what's in front of them. Only re-uploaded when it's
    /// another `Arc` than the last one prepared, or the colours change.
    pub highlight: Option<&'a Arc<Highlight>>,
    /// Where to draw on the target.
    pub viewport: Viewport,
    /// Size of the whole render target in physical pixels.
    pub target_size: [u32; 2],
    /// Physical pixels per logical pixel, so on-screen sizes such as the
    /// origin marker and grid spacing follow the display scale.
    pub scale_factor: f32,
    /// Colours from the UI theme.
    pub colors: Colors,
}

/// The marker of the point the camera orbits: a ring facing the screen
/// with a dot at the point, like the origin's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pivot {
    pub at: Vec3,
    /// From 0, not drawn, to 1, as it fades out. Out of range or not
    /// finite, it isn't drawn.
    pub opacity: f32,
}

/// An opaque sRGB-encoded colour, like CSS and UI colours. The renderer
/// converts it for its target.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Srgb(pub [f32; 3]);

/// Scene colours, from the UI theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    /// Background gradient, top to bottom.
    pub background_top: Srgb,
    pub background_bottom: Srgb,
    /// Base colour of model faces, before lighting.
    pub model: Srgb,
    /// Feature edges, drawn over the faces [`EDGE_WIDTH`] wide.
    pub edge: Srgb,
    /// Grid lines.
    pub grid: Srgb,
    /// The X, Y and Z axes, for the grid's axis lines.
    pub axes: [Srgb; 3],
    /// The rims of the origin marker's ring and dot, and the pivot's.
    pub origin_outline: Srgb,
    /// The core of the pivot's ring and dot.
    pub pivot: Srgb,
    /// Finished sketches' curves.
    pub sketch: Srgb,
    /// The base colours of a hovered and a selected face of
    /// [`Frame::highlight`], lit as [`Colors::model`] is: the model's
    /// lightness in another hue, so they still read as 3D.
    pub hovered_face: Srgb,
    pub selected_face: Srgb,
    /// A hovered and a selected edge of [`Frame::highlight`].
    pub hovered_edge: Srgba,
    pub selected_edge: Srgba,
    /// How opaque the model's faces and edges are when [`Frame::faded`],
    /// from 0 to 1. Only its nearest faces are drawn, and they still hide
    /// what's behind them from the grid and the sketches.
    pub faded_alpha: f32,
    /// How opaque the edges the model hides are drawn, from 0 to 1, of
    /// [`Self::edge`]: see [`Frame::hidden_edges`].
    pub hidden_edge_alpha: f32,
}

/// The scene shader's uniforms. `Uniforms` in `scene.wgsl` mirrors this
/// field for field, and the bind group layout's `min_binding_size` makes wgpu
/// check that it fits when each pipeline is built.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    /// The eye with w = 1, or in orthographic mode the direction towards the
    /// camera with w = 0, an eye at infinity.
    eye: [f32; 4],
    /// xyz: unit vector right on screen, w: half the visible width at the
    /// target in world units.
    right: [f32; 4],
    /// xyz: unit vector up on screen, w: half the visible height at the
    /// target in world units.
    up: [f32; 4],
    /// xyz: orbit target, w: how far in front of the eye a perspective
    /// view starts, [`Camera::near`]. Not `target`, which WGSL reserves.
    focus: [f32; 4],
    /// Unit vector towards the camera.
    backward: [f32; 4],
    /// xy: viewport size in physical pixels, z: physical pixels per logical
    /// pixel.
    viewport: [f32; 4],
    /// xy: the viewport's top left corner on the target, in physical
    /// pixels, where fragment positions count from; zw unused.
    viewport_origin: [f32; 4],
    /// [`Colors`], converted to linear with w = 1, except `model` and
    /// `edge`, whose w is how opaque the model is.
    background_top: [f32; 4],
    background_bottom: [f32; 4],
    model: [f32; 4],
    edge: [f32; 4],
    grid: [f32; 4],
    axes: [[f32; 4]; 3],
    origin_outline: [f32; 4],
    sketch: [f32; 4],
    /// [`Frame::pivot`]: xyz where it is, w its opacity, 0 if it isn't
    /// drawn; and its core's colour.
    pivot: [f32; 4],
    pivot_color: [f32; 4],
    /// [`Frame::grid`]: xyz its origin, and unit x and y axes; w unused.
    grid_origin: [f32; 4],
    grid_x: [f32; 4],
    grid_y: [f32; 4],
    /// The colours of the grid's x and y axis lines: of the world axis
    /// each lies along, or of the grid if it lies along none.
    grid_axes: [[f32; 4]; 2],
    /// [`SketchScene::plane`]: xyz its origin, and unit x and y axes; w
    /// unused.
    sketch_origin: [f32; 4],
    sketch_x: [f32; 4],
    sketch_y: [f32; 4],
    /// [`Colors::edge`] with w [`Colors::hidden_edge_alpha`], for the
    /// edges the model hides.
    hidden_edge: [f32; 4],
}

// WGSL lays out uniform structs in 16-byte steps.
const _: () = assert!(size_of::<Uniforms>().is_multiple_of(16));

/// The largest buffer the renderer relies on, in bytes: WebGPU's default
/// `maxBufferSize`, which the devices iced asks for have natively and on
/// the web. A larger buffer is a validation error, and wgpu panics on those.
const MAX_BUFFER_BYTES: usize = 256 << 20;

// Every mesh the kernel allows fits, one buffer per part: positions and
// normals are uploaded as they are, and indices too. Edges are uploaded an
// `EdgePoint` per edge vertex at most, two more for each closed polyline of
// four points or more, so half as many again at most, and one more at each
// end.
const _: () = assert!(size_of::<[f32; 3]>() * RenderMesh::MAX_VERTICES <= MAX_BUFFER_BYTES);
const _: () = assert!(size_of::<u32>() * RenderMesh::MAX_INDICES <= MAX_BUFFER_BYTES);
const _: () =
    assert!(size_of::<EdgePoint>() * (RenderMesh::MAX_EDGE_POINTS / 2 * 3 + 2) <= MAX_BUFFER_BYTES);
// Lines are uploaded a segment of two points each, fewer than points.
const _: () = assert!(size_of::<Segment>() * RenderLines::MAX_POINTS <= MAX_BUFFER_BYTES);

/// A segment of a line as the GPU takes it: its two ends.
type Segment = [[f32; 3]; 2];

/// A point of the stream the feature edges are drawn from: polyline after
/// polyline, a point again in a row left out (a polyline left with one
/// point has it twice, a dot), with a point of no edge ([`NO_EDGE`]) at
/// each end of the stream. A polyline ending where it starts, of four
/// points or more, has its last but one point before it and its second
/// after it, marked [`NEIGHBOUR_ONLY`], so its first and last segments
/// join. The stream is bound to [`EDGE_SLOTS`] vertex buffer slots a point
/// apart, so the instance drawing the segment from point `i + 1` to
/// `i + 2` sees the points before and after it too: the segment is drawn
/// if its ends are of the same edge, and joined to the points either side
/// that are of it too, marked or not. See `edge_segment` in the shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct EdgePoint {
    position: [f32; 3],
    /// How far along its polyline it is, in world units.
    along: f32,
    /// Which polyline it's on.
    edge: u32,
}

/// The edge of the points at the ends of the stream, which no polyline is,
/// marked or not.
const NO_EDGE: u32 = u32::MAX;

/// Set in [`EdgePoint::edge`] for a point that is only a neighbour of its
/// edge's segments, where a closed polyline joins itself, not an end of
/// one. As `NEIGHBOUR_ONLY` in the shader.
const NEIGHBOUR_ONLY: u32 = 1 << 31;
const _: () = assert!(RenderMesh::MAX_EDGE_POLYLINES <= NEIGHBOUR_ONLY as usize);

/// How many slots the edge stream is bound to: previous, start, end and
/// next point.
const EDGE_SLOTS: u32 = 4;

// Vertex buffer offsets are multiples of 4 bytes, and wgpu's GL backend
// (the browser's WebGL2) allows strides up to 255.
const _: () = assert!(size_of::<EdgePoint>() == 20);

/// The most alphas a part can be drawn at, from 0 to 1, the last opaque:
/// 8 bits' worth, as fine as the target shows. See [`Alphas`].
const ALPHA_STEPS: u32 = 256;

/// What a draw of a part of the mesh takes besides [`Uniforms`]: how
/// opaque the part is. `Part` in `scene.wgsl` mirrors it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PartUniforms {
    /// x: the alpha the part's faces and edges are drawn at, of the
    /// model's and the edges' own; yzw unused.
    alpha: [f32; 4],
}

/// The alphas the mesh's parts are drawn at: a uniform buffer of
/// [`PartUniforms`], one for each of [`ALPHA_STEPS`] steps from 0 to 1,
/// each at the device's uniform offset alignment, written once, from which
/// a part's draws pick theirs with the bind group's dynamic offset. WebGL2
/// has no push constants, and this needs no buffer written between draws
/// nor anything per vertex. A device whose buffers can't hold that many
/// gets as many as they hold, at least one, opaque.
struct Alphas {
    /// Group 1 of every pipeline.
    group: wgpu::BindGroup,
    /// How far apart the steps are in the buffer, in bytes.
    stride: u32,
    /// The last step, opaque.
    opaque: u32,
}

impl Alphas {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Alphas {
        let size = size_of::<PartUniforms>() as u32;
        let limits = device.limits();
        let stride = limits.min_uniform_buffer_offset_alignment.max(size);
        let fit = limits.max_buffer_size / u64::from(stride);
        let steps = u32::try_from(fit).unwrap_or(u32::MAX).clamp(1, ALPHA_STEPS);
        let opaque = steps - 1;
        let mut table = vec![0u8; (steps * stride) as usize];
        for (step, entry) in (0..steps).zip(table.chunks_exact_mut(stride as usize)) {
            let alpha = if step == opaque {
                1.0
            } else {
                step as f32 / opaque as f32
            };
            let part = PartUniforms {
                alpha: [alpha, 0.0, 0.0, 0.0],
            };
            entry[..size as usize].copy_from_slice(bytemuck::bytes_of(&part));
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde part alphas"),
            contents: &table,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde part alphas"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(u64::from(size)),
                }),
            }],
        });
        Alphas {
            group,
            stride,
            opaque,
        }
    }

    /// The step a part of opacity `alpha` is drawn at, see [`alpha_step`].
    fn step(&self, alpha: Option<f32>) -> u32 {
        alpha_step(alpha, self.opaque)
    }

    /// Sets the alpha the parts drawn next are drawn at to `step`'s.
    fn set(&self, pass: &mut wgpu::RenderPass<'_>, step: u32) {
        pass.set_bind_group(1, &self.group, &[step * self.stride]);
    }
}

/// The step of a table of alphas from 0 to 1 whose last step, `opaque`, is
/// opaque, that a part of opacity `alpha` is drawn at, the nearest: opaque
/// if it's out of range or NaN. Only 0 is drawn at step 0: on a short
/// table, which a device with tiny buffers gets, a faint part is drawn at
/// the first step past it, opaque if that's the last, rather than not at
/// all.
fn alpha_step(alpha: Option<f32>, opaque: u32) -> u32 {
    match alpha {
        Some(0.0) => 0,
        // In range, so the cast is exact.
        Some(alpha) if (0.0..=1.0).contains(&alpha) => {
            ((alpha * opaque as f32).round() as u32).max(1).min(opaque)
        }
        _ => opaque,
    }
}

struct GpuMesh {
    positions: wgpu::Buffer,
    normals: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// The [`EdgePoint`] stream, if there are any edges.
    edges: Option<wgpu::Buffer>,
    /// The mesh's parts, as [`RenderMesh::parts`] gives them; one for the
    /// whole mesh if it has none.
    parts: Vec<GpuPart>,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
}

/// What the renderer keeps of a part of the mesh to draw it on its own.
#[derive(Debug, Clone)]
struct GpuPart {
    /// Its triangles' indices.
    indices: Range<u32>,
    /// Its edges' points in the [`EdgePoint`] stream, the neighbour only
    /// points of closed ones included, empty if it has none. The parts'
    /// follow one another, from after the stream's first point.
    points: Range<u32>,
    /// Its triangles' bounds, to sort parts less than opaque by.
    bounds: Option<Aabb>,
}

/// Which parts of a frame's mesh are drawn how, worked out in
/// [`Renderer::prepare`] from [`Frame::opacity`].
#[derive(Debug, Default)]
struct PartDraws {
    /// The runs of opaque parts, one after another, each drawn at once.
    opaque: Vec<Range<usize>>,
    /// The parts less than opaque, far to near, and their alphas' steps.
    transparent: Vec<(usize, u32)>,
}

impl PartDraws {
    /// How `parts` are drawn at `opacity`, at `alphas`' steps, seen from
    /// `camera`.
    fn new(parts: &[GpuPart], opacity: &[f32], alphas: &Alphas, camera: &Camera) -> PartDraws {
        let mut draws = PartDraws::default();
        for i in 0..parts.len() {
            let step = alphas.step(opacity.get(i).copied());
            if step < alphas.opaque {
                draws.transparent.push((i, step));
            } else if let Some(run) = draws.opaque.last_mut().filter(|run| run.end == i) {
                run.end = i + 1;
            } else {
                draws.opaque.push(i..i + 1);
            }
        }
        // Far to near by their bounds' centres along the view, the same in
        // either projection.
        let backward = camera.backward();
        let depth = |&(i, _): &(usize, u32)| {
            parts[i]
                .bounds
                .map_or(0.0, |bounds| bounds.center().dot(backward))
        };
        draws
            .transparent
            .sort_by(|a, b| depth(a).total_cmp(&depth(b)));
        draws
    }
}

struct GpuLines {
    segments: wgpu::Buffer,
    segment_count: u32,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
}

struct DepthTarget {
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// What differs between the renderer's pipelines. They all share the scene
/// shader, the uniform layout and the depth buffer.
#[derive(Clone)]
struct Pass<'a> {
    label: &'a str,
    /// Vertex and fragment entry points in the scene shader.
    vs: &'a str,
    fs: &'a str,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
    depth_write: bool,
    depth_compare: wgpu::CompareFunction,
    blend: wgpu::BlendState,
    write_mask: wgpu::ColorWrites,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    /// Sets the shader's `SKETCH_DEPTH`: the sketch's layers are given
    /// their depth, pulled towards the camera, rather than drawn on top.
    sketch_depth: bool,
}

impl<'a> Pass<'a> {
    /// Triangles drawn over the scene without writing depth, blended and
    /// not culled.
    const fn overlay(label: &'a str, vs: &'a str, fs: &'a str) -> Self {
        Pass {
            label,
            vs,
            fs,
            buffers: &[],
            depth_write: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            blend: wgpu::BlendState::ALPHA_BLENDING,
            write_mask: wgpu::ColorWrites::ALL,
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            sketch_depth: false,
        }
    }

    /// Like [`Self::on_top`], hidden by what's nearer, its depth from
    /// the shader with `SKETCH_DEPTH` set.
    const fn depth_tested(
        label: &'a str,
        vs: &'a str,
        fs: &'a str,
        buffers: &'a [wgpu::VertexBufferLayout<'a>],
    ) -> Self {
        Pass {
            buffers,
            sketch_depth: true,
            ..Pass::overlay(label, vs, fs)
        }
    }

    /// Like [`Self::overlay`], drawn over everything whatever its depth,
    /// taking a buffer of `buffers`.
    const fn on_top(
        label: &'a str,
        vs: &'a str,
        fs: &'a str,
        buffers: &'a [wgpu::VertexBufferLayout<'a>],
    ) -> Self {
        Pass {
            buffers,
            depth_compare: wgpu::CompareFunction::Always,
            ..Pass::overlay(label, vs, fs)
        }
    }
}

/// Why [`Renderer::prepare`] couldn't upload a frame's mesh or lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// A buffer of the mesh would take `bytes`, more than the device's
    /// `max_buffer_size` of `limit`.
    MeshTooLarge { bytes: u64, limit: u64 },
    /// The same for the sketches' lines.
    LinesTooLarge { bytes: u64, limit: u64 },
    /// The same for a layer of the sketch being edited.
    SketchTooLarge { bytes: u64, limit: u64 },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrepareError::MeshTooLarge { bytes, limit } => write!(
                f,
                "the mesh needs a {bytes}-byte buffer, but the device allows at most {limit} bytes"
            ),
            PrepareError::LinesTooLarge { bytes, limit } => write!(
                f,
                "the sketches need a {bytes}-byte buffer, but the device allows at most {limit} \
                 bytes"
            ),
            PrepareError::SketchTooLarge { bytes, limit } => write!(
                f,
                "the sketch being edited needs a {bytes}-byte buffer, but the device allows at \
                 most {limit} bytes"
            ),
        }
    }
}

impl std::error::Error for PrepareError {}

/// Renders a scene into a caller-provided color target.
///
/// Holds only what every scene shares; what one scene draws is in a
/// [`Slot`]. Call [`Renderer::prepare`] to upload a frame into a slot, then
/// [`Renderer::render`] to record its draw commands.
pub struct Renderer {
    background: wgpu::RenderPipeline,
    grid: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    /// The faded model: its depth first, then its nearest faces blended.
    mesh_depth: wgpu::RenderPipeline,
    mesh_faded: wgpu::RenderPipeline,
    /// Parts less than opaque: their back faces, then their front faces,
    /// blended, not writing depth.
    glass_back: wgpu::RenderPipeline,
    glass_front: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    /// The faces of [`Frame::highlight`]; its edges are drawn as the
    /// sketch's depth tested lines.
    highlight: wgpu::RenderPipeline,
    /// The edges again where the model hides them, dashed.
    hidden_edges: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    origin: wgpu::RenderPipeline,
    /// The sketch being edited: its fills, lines and points, drawn over
    /// everything, and the same hidden by the model in front of them
    /// ([`SketchScene::depth_tested`]).
    sketch_on_top: SketchPipelines,
    sketch_depth_tested: SketchPipelines,
    bind_group_layout: wgpu::BindGroupLayout,
    alphas: Alphas,
}

/// One scene's state on the GPU, from [`Renderer::prepare`] to
/// [`Renderer::render`]: its uniforms, mesh, lines and depth buffer.
///
/// Each scene drawn in the same frame needs its own slot, since every
/// `prepare` may run before any `render`. Keeping a slot across frames
/// keeps its mesh uploaded.
pub struct Slot {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    mesh: Option<GpuMesh>,
    /// The [`Frame::mesh`] `mesh` was uploaded from, or skipped for. Holding
    /// it keeps the allocation from being reused for another mesh, and
    /// `Arc::get_mut` from changing that mesh in place.
    source: Weak<RenderMesh>,
    lines: Option<GpuLines>,
    /// The [`Frame::sketches`] `lines` were uploaded from, like `source`.
    lines_source: Weak<RenderLines>,
    /// The layers of [`Frame::sketch`], and the base layer's `Arc` like
    /// `source`. The live layer's buffers are kept to be written again.
    sketch_base: SketchBuffers,
    sketch_source: Weak<SketchLayer>,
    sketch_live: SketchBuffers,
    /// [`Frame::highlight`]'s faces and edges, the `Arc` they were
    /// uploaded from like `source`, and the colours they were uploaded in.
    highlight_faces: Instances,
    highlight_lines: SketchBuffers,
    highlight_source: Weak<Highlight>,
    highlight_colors: Option<Colors>,
    /// Whether the frame has a sketch to draw, and whether it's depth
    /// tested.
    sketching: bool,
    sketch_depth: bool,
    depth: Option<DepthTarget>,
    viewport: Viewport,
    faded: bool,
    /// How the mesh's parts are drawn, by [`Frame::opacity`].
    draws: PartDraws,
    /// Whether the edges the model hides are drawn: [`Frame::hidden_edges`]
    /// and not [`Frame::faded`].
    hidden_edges: bool,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = |label| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/scene.wgsl").into()),
            })
        };
        let shader = module("varde scene");
        // The same for the sketch's depth tested pipelines, which differ
        // from the others only by `SKETCH_DEPTH`: wgpu's GL backend (the
        // browser's WebGL2) caches a program by its module and entry
        // points, not its pipeline constants, so on one module they'd get
        // the on-top program.
        let depth_shader = module("varde scene, sketch depth tested");

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });

        // How opaque the part drawn is, at a dynamic offset into the
        // alphas' buffer: see `Alphas`. Every pipeline has it, so
        // it's bound once for all of them.
        let part_size = size_of::<PartUniforms>() as u64;
        let part_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde part"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(part_size),
                },
                count: None,
            }],
        });
        let alphas = Alphas::new(device, &part_layout);

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde scene"),
            bind_group_layouts: &[&bind_group_layout, &part_layout],
            push_constant_ranges: &[],
        });

        // Overrides in the scene shader. `ENCODE_SRGB` is set if the target
        // stores what the shader writes as is, so the shader must sRGB-encode
        // its linear output.
        // `SKETCH_DEPTH` is set for the sketch's depth tested layers.
        let constants = |sketch_depth: bool| {
            [
                ("GRID_FADE_HEIGHTS", f64::from(GRID_FADE_HEIGHTS)),
                ("LINE_WIDTH", f64::from(LINE_WIDTH)),
                ("EDGE_WIDTH", f64::from(EDGE_WIDTH)),
                ("HIDDEN_EDGE_WIDTH", f64::from(HIDDEN_EDGE_WIDTH)),
                ("HIDDEN_DASH", f64::from(HIDDEN_DASH[0])),
                ("HIDDEN_GAP", f64::from(HIDDEN_DASH[1])),
                ("ENCODE_SRGB", if format.is_srgb() { 0.0 } else { 1.0 }),
                ("SKETCH_DEPTH", if sketch_depth { 1.0 } else { 0.0 }),
            ]
        };

        let pipeline = |pass: Pass<'_>| {
            let constants = constants(pass.sketch_depth);
            let shader = if pass.sketch_depth {
                &depth_shader
            } else {
                &shader
            };
            let compilation_options = wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(pass.label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(pass.vs),
                    buffers: pass.buffers,
                    compilation_options: compilation_options.clone(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(pass.fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(pass.blend),
                        write_mask: pass.write_mask,
                    })],
                    compilation_options: compilation_options.clone(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: pass.topology,
                    cull_mode: pass.cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: pass.depth_write,
                    depth_compare: pass.depth_compare,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };

        // Positions in slot 0 and normals in slot 1, each a buffer of its
        // own. Lines take a segment per instance, its ends in slots 0 and 1.
        let positions = wgpu::VertexBufferLayout {
            array_stride: size_of::<[f32; 3]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        };
        let normals = wgpu::VertexBufferLayout {
            array_stride: size_of::<[f32; 3]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![1 => Float32x3],
        };
        let segments = wgpu::VertexBufferLayout {
            array_stride: size_of::<Segment>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        // The sketch being edited: a segment or a point per instance, and
        // fills as triangles.
        let sketch_lines = wgpu::VertexBufferLayout {
            array_stride: size_of::<LineInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
                4 => Float32x4, 5 => Float32x2, 6 => Uint32,
            ],
        };
        let sketch_points = wgpu::VertexBufferLayout {
            array_stride: size_of::<PointInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4,
            ],
        };
        let sketch_fills = wgpu::VertexBufferLayout {
            array_stride: size_of::<FillVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Uint32, 2 => Float32x4, 3 => Float32,
            ],
        };
        let highlight_vertices = wgpu::VertexBufferLayout {
            array_stride: size_of::<HighlightVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
        };
        // The feature edges: the `EdgePoint` stream in slots 0 to 3, a
        // point apart, an instance per segment: the points before, at its
        // start, at its end and after it. Its neighbours' positions and
        // edges are all it needs of them, and the start's distance along
        // its edge, for the hidden edges' dashes. 9 attributes and 4 slots,
        // within WebGL2's 16 and 8.
        let attribute = |format, offset: usize, shader_location| wgpu::VertexAttribute {
            format,
            offset: offset as u64,
            shader_location,
        };
        let position = |location| {
            attribute(
                wgpu::VertexFormat::Float32x3,
                std::mem::offset_of!(EdgePoint, position),
                location,
            )
        };
        let along = |location| {
            attribute(
                wgpu::VertexFormat::Float32,
                std::mem::offset_of!(EdgePoint, along),
                location,
            )
        };
        let edge = |location| {
            attribute(
                wgpu::VertexFormat::Uint32,
                std::mem::offset_of!(EdgePoint, edge),
                location,
            )
        };
        let edge_attributes: [&[_]; EDGE_SLOTS as usize] = [
            &[position(0), edge(1)],
            &[position(2), along(3), edge(4)],
            &[position(5), edge(6)],
            &[position(7), edge(8)],
        ];
        let edge_points = edge_attributes.map(|attributes| wgpu::VertexBufferLayout {
            array_stride: size_of::<EdgePoint>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        });
        let mesh = Pass {
            label: "varde mesh",
            vs: "vs_mesh",
            fs: "fs_mesh",
            buffers: &[positions, normals],
            depth_write: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            blend: wgpu::BlendState::REPLACE,
            write_mask: wgpu::ColorWrites::ALL,
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: Some(wgpu::Face::Back),
            sketch_depth: false,
        };

        Self {
            background: pipeline(Pass {
                blend: wgpu::BlendState::REPLACE,
                ..Pass::overlay("varde background", "vs_fullscreen", "fs_background")
            }),
            grid: pipeline(Pass::overlay("varde grid", "vs_fullscreen", "fs_grid")),
            mesh_depth: pipeline(Pass {
                label: "varde faded mesh depth",
                write_mask: wgpu::ColorWrites::empty(),
                ..mesh.clone()
            }),
            // Drawn over its own depth, so only the nearest faces pass.
            mesh_faded: pipeline(Pass {
                label: "varde faded mesh",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                ..mesh.clone()
            }),
            // Hidden by the opaque parts, and leaving the depth to them.
            glass_back: pipeline(Pass {
                label: "varde glass, back faces",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                cull_mode: Some(wgpu::Face::Front),
                ..mesh.clone()
            }),
            glass_front: pipeline(Pass {
                label: "varde glass, front faces",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                ..mesh.clone()
            }),
            mesh: pipeline(mesh),
            edges: pipeline(Pass {
                buffers: &edge_points,
                ..Pass::overlay("varde edges", "vs_edge", "fs_line")
            }),
            // Over the faces they lie on, pulled towards the camera like
            // the edges, culled like the model.
            highlight: pipeline(Pass {
                buffers: std::slice::from_ref(&highlight_vertices),
                cull_mode: Some(wgpu::Face::Back),
                ..Pass::overlay("varde highlight faces", "vs_highlight", "fs_highlight")
            }),
            // Exactly the pixels the visible edges didn't draw: the same
            // quads at the same depth, tested the other way round. Its own
            // entry point, which wgpu's GL backend keys programs by.
            hidden_edges: pipeline(Pass {
                buffers: &edge_points,
                depth_compare: wgpu::CompareFunction::Greater,
                ..Pass::overlay("varde hidden edges", "vs_hidden_edge", "fs_line")
            }),
            lines: pipeline(Pass {
                buffers: &[segments],
                ..Pass::overlay("varde sketch lines", "vs_line", "fs_line")
            }),
            origin: pipeline(Pass::overlay("varde origin", "vs_origin", "fs_origin")),
            sketch_on_top: SketchPipelines {
                fills: pipeline(Pass::on_top(
                    "varde sketch fills",
                    "vs_fill",
                    "fs_fill",
                    std::slice::from_ref(&sketch_fills),
                )),
                lines: pipeline(Pass::on_top(
                    "varde sketch being edited",
                    "vs_sketch_line",
                    "fs_line",
                    std::slice::from_ref(&sketch_lines),
                )),
                points: pipeline(Pass::on_top(
                    "varde sketch points",
                    "vs_point",
                    "fs_point",
                    std::slice::from_ref(&sketch_points),
                )),
            },
            sketch_depth_tested: SketchPipelines {
                fills: pipeline(Pass::depth_tested(
                    "varde sketch fills, depth tested",
                    "vs_fill",
                    "fs_fill",
                    &[sketch_fills],
                )),
                lines: pipeline(Pass::depth_tested(
                    "varde sketch lines, depth tested",
                    "vs_sketch_line",
                    "fs_line",
                    &[sketch_lines],
                )),
                points: pipeline(Pass::depth_tested(
                    "varde sketch points, depth tested",
                    "vs_point",
                    "fs_point",
                    &[sketch_points],
                )),
            },
            bind_group_layout,
            alphas,
        }
    }

    /// Creates a slot to draw a scene with, empty until it's prepared.
    pub fn slot(&self, device: &wgpu::Device) -> Slot {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde uniforms"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        Slot {
            uniforms,
            bind_group,
            mesh: None,
            source: Weak::new(),
            lines: None,
            lines_source: Weak::new(),
            sketch_base: SketchBuffers::default(),
            sketch_source: Weak::new(),
            sketch_live: SketchBuffers::default(),
            highlight_faces: Instances::default(),
            highlight_lines: SketchBuffers::default(),
            highlight_source: Weak::new(),
            highlight_colors: None,
            sketching: false,
            sketch_depth: false,
            depth: None,
            viewport: Viewport::default(),
            faded: false,
            draws: PartDraws::default(),
            hidden_edges: false,
        }
    }

    /// Uploads what `frame` needs to draw into `slot`.
    ///
    /// Fails with [`PrepareError::MeshTooLarge`] when the frame's mesh
    /// doesn't fit in the device's buffers, which can be smaller than the
    /// 256 MiB every mesh the kernel allows fits in. The frame is then
    /// drawn without it, and it isn't tried again until the slot's frame
    /// has another mesh, so this is only reported once per mesh. The same
    /// for the frame's sketches with [`PrepareError::LinesTooLarge`], and
    /// the layers of the sketch being edited with
    /// [`PrepareError::SketchTooLarge`], its live layer reported each frame
    /// it's too large; should several fail at once, the mesh's error is
    /// the one returned, then the sketches'.
    pub fn prepare(
        &self,
        slot: &mut Slot,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<(), PrepareError> {
        slot.viewport = frame.viewport;
        slot.faded = frame.faded;
        slot.hidden_edges = frame.hidden_edges && !frame.faded;

        let mut result = Ok(());
        if !std::ptr::eq(slot.lines_source.as_ptr(), Arc::as_ptr(frame.sketches)) {
            slot.lines_source = Arc::downgrade(frame.sketches);
            slot.lines = upload_lines(device, frame.sketches).unwrap_or_else(|error| {
                result = Err(error);
                None
            });
        }
        // The regen lane hands an unchanged model back as the same `Arc`,
        // so this skips its upload. A failed upload isn't tried again for
        // the same `Arc`: it can only fail on the device's buffer limit,
        // which the same mesh would hit again.
        if !std::ptr::eq(slot.source.as_ptr(), Arc::as_ptr(frame.mesh)) {
            slot.source = Arc::downgrade(frame.mesh);
            slot.mesh = upload_mesh(device, frame.mesh).unwrap_or_else(|error| {
                result = Err(error);
                None
            });
        }
        // Faded, every part is drawn alike.
        let opacity = if frame.faded { &[] } else { frame.opacity };
        let parts = slot.mesh.as_ref().map_or(&[][..], |mesh| &mesh.parts);
        slot.draws = PartDraws::new(parts, opacity, &self.alphas, frame.camera);

        slot.sketching = frame.sketch.is_some();
        slot.sketch_depth = frame.sketch.is_some_and(|sketch| sketch.depth_tested);
        if let Some(sketch) = &frame.sketch {
            let mut sketch_result = Ok(());
            if !std::ptr::eq(slot.sketch_source.as_ptr(), Arc::as_ptr(sketch.base)) {
                slot.sketch_source = Arc::downgrade(sketch.base);
                if let Err(error) = slot.sketch_base.write(device, queue, sketch.base) {
                    sketch_result = Err(error);
                }
            }
            if let Err(error) = slot.sketch_live.write(device, queue, sketch.live) {
                sketch_result = sketch_result.and(Err(error));
            }
            result = result.and(sketch_result);
        }

        result = result.and(slot.prepare_highlight(device, queue, frame));

        // A depth tested sketch is hidden by what's in front of it, and
        // so needs the depth range to cover it.
        let sketch_bounds = frame
            .sketch
            .filter(|sketch| sketch.depth_tested)
            .map(|sketch| {
                [
                    sketch.base.bounds(&sketch.plane),
                    sketch.live.bounds(&sketch.plane),
                ]
            });
        let bounds = [
            slot.mesh.as_ref().and_then(|m| m.bounds),
            slot.lines.as_ref().and_then(|l| l.bounds),
        ]
        .into_iter()
        .chain(sketch_bounds.into_iter().flatten());
        let (camera, colors, grid) = (frame.camera, frame.colors, &frame.grid);
        let sketch_plane = frame.sketch.map_or(*grid, |sketch| sketch.plane);
        let aspect = frame.viewport.aspect();
        let half = camera.half_extents(aspect);
        // Out of range, the model is drawn opaque rather than not at all.
        let alpha = if frame.faded && (0.0..=1.0).contains(&colors.faded_alpha) {
            colors.faded_alpha
        } else {
            1.0
        };
        let with_alpha = |color, alpha| {
            let [r, g, b, _] = linear(color);
            [r, g, b, alpha]
        };
        let faded = |color| with_alpha(color, alpha);
        // Out of range, as faint as can be rather than solid.
        let hidden_alpha = if (0.0..=1.0).contains(&colors.hidden_edge_alpha) {
            colors.hidden_edge_alpha
        } else {
            0.0
        };
        let uniforms = Uniforms {
            view_proj: scene::view_projection(camera, aspect, grid, bounds.flatten())
                .to_cols_array_2d(),
            eye: camera.eye_homogeneous().to_array(),
            right: camera.right().extend(half.x).to_array(),
            up: camera.up().extend(half.y).to_array(),
            focus: camera.target().extend(camera.near()).to_array(),
            backward: camera.backward().extend(0.0).to_array(),
            viewport: [
                frame.viewport.width,
                frame.viewport.height,
                frame.scale_factor,
                0.0,
            ],
            viewport_origin: [frame.viewport.x, frame.viewport.y, 0.0, 0.0],
            background_top: linear(colors.background_top),
            background_bottom: linear(colors.background_bottom),
            model: faded(colors.model),
            edge: faded(colors.edge),
            grid: linear(colors.grid),
            axes: colors.axes.map(linear),
            origin_outline: linear(colors.origin_outline),
            sketch: linear(colors.sketch),
            pivot: frame
                .pivot
                .filter(|pivot| pivot.at.is_finite() && (0.0..=1.0).contains(&pivot.opacity))
                .map_or([0.0; 4], |pivot| pivot.at.extend(pivot.opacity).to_array()),
            pivot_color: linear(colors.pivot),
            grid_origin: grid.origin().extend(0.0).to_array(),
            grid_x: grid.x().extend(0.0).to_array(),
            grid_y: grid.y().extend(0.0).to_array(),
            grid_axes: [grid.x(), grid.y()].map(|axis| linear(axis_color(axis, &colors))),
            sketch_origin: sketch_plane.origin().extend(0.0).to_array(),
            sketch_x: sketch_plane.x().extend(0.0).to_array(),
            sketch_y: sketch_plane.y().extend(0.0).to_array(),
            hidden_edge: with_alpha(colors.edge, hidden_alpha),
        };
        queue.write_buffer(&slot.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let size = frame.target_size.map(|s| s.max(1));
        if slot.depth.as_ref().map(|d| d.size) != Some(size) {
            slot.depth = Some(create_depth(device, size));
        }
        result
    }

    /// Records draw commands for the frame last prepared into `slot` into
    /// `encoder`, compositing over `target`, within `clip`.
    ///
    /// Outside a sketch, in order: the background; the opaque parts'
    /// faces, writing depth, and their edges; the grid and the finished
    /// sketches; the edges the opaque parts hide, of every part, dashed;
    /// the edges of the parts less than opaque, against the opaque parts'
    /// depth; the extrude's depth tested layers, if there are parts less
    /// than opaque; those parts far to near, each its back faces then its
    /// front faces, blended at its alpha without writing depth; their
    /// front faces' depth, then their edges again, over the glass they lie
    /// on; the origin marker and the pivot's; and the sketch being edited,
    /// or the extrude's layers if they weren't drawn yet. In a sketch
    /// ([`Frame::faded`]), the model's depth, its nearest faces blended
    /// and its edges, every part alike, and no hidden edges.
    pub fn render(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
    ) {
        let Some(depth) = &slot.depth else { return };
        if clip.width == 0 || clip.height == 0 {
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("varde scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let vp = slot.viewport;
        pass.set_viewport(vp.x, vp.y, vp.width, vp.height, 0.0, 1.0);
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_bind_group(0, &slot.bind_group, &[]);
        self.alphas.set(&mut pass, self.alphas.opaque);

        pass.set_pipeline(&self.background);
        pass.draw(0..3, 0..1);

        let mesh = slot.mesh.as_ref();
        let draws = &slot.draws;
        if let Some(mesh) = mesh {
            bind_faces(&mut pass, mesh);
            if slot.faded {
                let all = 0..mesh.parts.len();
                pass.set_pipeline(&self.mesh_depth);
                draw_faces(&mut pass, mesh, all.clone());
                pass.set_pipeline(&self.mesh_faded);
                draw_faces(&mut pass, mesh, all.clone());
                draw_edges(&mut pass, &self.edges, mesh, all);
            } else {
                pass.set_pipeline(&self.mesh);
                for run in &draws.opaque {
                    draw_faces(&mut pass, mesh, run.clone());
                }

                // The highlighted faces over the model's, under its edges,
                // which are pulled as far towards the camera.
                if let Some(buffer) = slot.highlight_faces.drawn() {
                    pass.set_pipeline(&self.highlight);
                    pass.set_vertex_buffer(0, buffer);
                    pass.draw(0..slot.highlight_faces.count, 0..1);
                }

                for run in &draws.opaque {
                    draw_edges(&mut pass, &self.edges, mesh, run.clone());
                }
            }

            // The highlighted edges over the model's.
            if let Some(buffer) = slot.highlight_lines.lines.drawn() {
                pass.set_pipeline(&self.sketch_depth_tested.lines);
                pass.set_vertex_buffer(0, buffer);
                pass.draw(0..LINE_VERTICES, 0..slot.highlight_lines.lines.count);
            }
        }

        // Drawn after the model so they blend over the background and are
        // occluded by geometry. The sketches go over the grid, since they
        // often lie on it.
        pass.set_pipeline(&self.grid);
        pass.draw(0..3, 0..1);

        if let Some(lines) = &slot.lines {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, lines.segments.slice(..));
            pass.draw(0..LINE_VERTICES, 0..lines.segment_count);
        }

        // Only the opaque parts have written depth so far, and nothing
        // above writes it after them: the edges they hide, of every part,
        // are against them alone, so an edge behind a part less than
        // opaque is seen through it rather than hidden. Then the visible
        // edges of the parts less than opaque, under the faces in front of
        // them, which are drawn next and dim them.
        if !slot.faded
            && let Some(mesh) = mesh
        {
            if slot.hidden_edges {
                for run in &draws.opaque {
                    draw_edges(&mut pass, &self.hidden_edges, mesh, run.clone());
                }
                self.draw_glass_edges(&mut pass, &self.hidden_edges, mesh, draws);
            }
            self.draw_glass_edges(&mut pass, &self.edges, mesh, draws);
        }

        // The extrude's layers, depth tested: under the faces less than
        // opaque in front of them, if there are any, and so under the
        // origin marker too; else over it, as the sketch being edited is.
        let glass = mesh.is_some() && !draws.transparent.is_empty();
        let layers = [&slot.sketch_base, &slot.sketch_live];
        if slot.sketching && slot.sketch_depth && glass {
            for layer in layers {
                self.sketch_depth_tested.draw(&mut pass, layer);
            }
        }

        // The parts less than opaque, far to near: each one's back faces,
        // then its front faces, over what's behind them. Then their front
        // faces' depth, and their edges again, so the edges on the nearest
        // faces show on them undimmed.
        if let Some(mesh) = mesh.filter(|_| glass) {
            bind_faces(&mut pass, mesh);
            for &(part, step) in &draws.transparent {
                self.alphas.set(&mut pass, step);
                for pipeline in [&self.glass_back, &self.glass_front] {
                    pass.set_pipeline(pipeline);
                    draw_faces(&mut pass, mesh, part..part + 1);
                }
            }
            pass.set_pipeline(&self.mesh_depth);
            for &(part, _) in &draws.transparent {
                draw_faces(&mut pass, mesh, part..part + 1);
            }
            self.draw_glass_edges(&mut pass, &self.edges, mesh, draws);
        }

        pass.set_pipeline(&self.origin);
        pass.draw(0..ORIGIN_VERTICES, 0..MARKERS);

        // The sketch being edited, over everything, the origin marker
        // included, since its points often lie on it; or depth tested.
        if slot.sketching && !(slot.sketch_depth && glass) {
            let pipelines = if slot.sketch_depth {
                &self.sketch_depth_tested
            } else {
                &self.sketch_on_top
            };
            for layer in layers {
                pipelines.draw(&mut pass, layer);
            }
        }
    }

    /// Records drawing the edges of `mesh`'s parts less than opaque, as
    /// `draws` has them, each at its alpha, with `pipeline`.
    fn draw_glass_edges(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        mesh: &GpuMesh,
        draws: &PartDraws,
    ) {
        for &(part, step) in &draws.transparent {
            self.alphas.set(pass, step);
            draw_edges(pass, pipeline, mesh, part..part + 1);
        }
    }
}

impl Slot {
    /// Uploads `frame`'s highlight if it's another than the one uploaded,
    /// or its colours changed. On failing, holds none.
    fn prepare_highlight(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<(), PrepareError> {
        let Some(highlight) = frame.highlight.filter(|highlight| !highlight.is_empty()) else {
            self.highlight_source = Weak::new();
            self.highlight_colors = None;
            self.highlight_faces.count = 0;
            self.highlight_lines.lines.count = 0;
            return Ok(());
        };
        let current = std::ptr::eq(self.highlight_source.as_ptr(), Arc::as_ptr(highlight))
            && self.highlight_colors == Some(frame.colors);
        if current {
            return Ok(());
        }
        self.highlight_source = Arc::downgrade(highlight);
        self.highlight_colors = Some(frame.colors);
        let colors = &frame.colors;
        let written = (self.highlight_faces)
            .write(
                device,
                queue,
                "varde highlight faces",
                &highlight.vertices(colors),
            )
            .and_then(|()| (self.highlight_lines).write(device, queue, &highlight.lines(colors)));
        if written.is_err() {
            self.highlight_faces.count = 0;
            self.highlight_lines = SketchBuffers::default();
        }
        written
    }
}

/// Binds `mesh`'s buffers for drawing its faces.
fn bind_faces(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh) {
    pass.set_vertex_buffer(0, mesh.positions.slice(..));
    pass.set_vertex_buffer(1, mesh.normals.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
}

/// What `parts` of `mesh`, which follow one another, take of what `of`
/// gives each of them, from the first's start to the last's end: empty if
/// there are no parts.
fn span(mesh: &GpuMesh, parts: Range<usize>, of: fn(&GpuPart) -> &Range<u32>) -> Range<u32> {
    let parts = &mesh.parts[parts];
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) => of(first).start..of(last).end,
        _ => 0..0,
    }
}

/// Records drawing the faces of `mesh`'s `parts`, which follow one another,
/// with the pipeline set and its buffers bound ([`bind_faces`]).
fn draw_faces(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh, parts: Range<usize>) {
    let indices = span(mesh, parts, |part| &part.indices);
    if !indices.is_empty() {
        pass.draw_indexed(indices, 0, 0..1);
    }
}

/// Records drawing the feature edges of `mesh`'s `parts`, which follow one
/// another, with `pipeline`. Their points follow one another in the
/// stream, so they're drawn at once by the instances whose segments start
/// at their first point up to their last but one, and each slot is bound
/// from the first of those instances' points: the instance before the
/// first point's sees the point before it, of another part's edge or of
/// none, as its previous point, and the last one sees the one after the
/// last point, neither of which a segment of these parts joins.
fn draw_edges(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    mesh: &GpuMesh,
    parts: Range<usize>,
) {
    let Some(edges) = &mesh.edges else { return };
    let points = span(mesh, parts, |part| &part.points);
    // A segment needs two points; and a part's points come after the
    // stream's first, so there's a point before them.
    if points.end.saturating_sub(points.start) < 2 {
        return;
    }
    pass.set_pipeline(pipeline);
    let stride = size_of::<EdgePoint>() as u64;
    let from = u64::from(points.start - 1);
    for slot in 0..EDGE_SLOTS {
        pass.set_vertex_buffer(slot, edges.slice((from + u64::from(slot)) * stride..));
    }
    pass.draw(0..LINE_VERTICES, 0..points.end - points.start - 1);
}

/// The pipelines drawing the sketch's layers, one way or the other: see
/// [`SketchScene::depth_tested`].
struct SketchPipelines {
    fills: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    points: wgpu::RenderPipeline,
}

impl SketchPipelines {
    /// Records drawing a layer of the sketch: its fills, then its lines,
    /// then its points.
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, layer: &SketchBuffers) {
        if let Some(buffer) = layer.fills.drawn() {
            pass.set_pipeline(&self.fills);
            pass.set_vertex_buffer(0, buffer);
            pass.draw(0..layer.fills.count, 0..1);
        }
        if let Some(buffer) = layer.lines.drawn() {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, buffer);
            pass.draw(0..LINE_VERTICES, 0..layer.lines.count);
        }
        if let Some(buffer) = layer.points.drawn() {
            pass.set_pipeline(&self.points);
            pass.set_vertex_buffer(0, buffer);
            pass.draw(0..POINT_VERTICES, 0..layer.points.count);
        }
    }
}

/// The colour of the grid's axis line along `axis`, a unit vector: the
/// world axis it lies along, or the grid's if none.
fn axis_color(axis: Vec3, colors: &Colors) -> Srgb {
    let along = axis.abs();
    (0..3)
        .find(|&i| along[i] > 1.0 - 1e-6)
        .map_or(colors.grid, |i| colors.axes[i])
}

/// Converts a colour to linear, which the shader works in.
fn linear(Srgb(rgb): Srgb) -> [f32; 4] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    [r, g, b, 1.0]
}

pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Uploads `mesh`, or nothing if it is empty.
fn upload_mesh(device: &wgpu::Device, mesh: &RenderMesh) -> Result<Option<GpuMesh>, PrepareError> {
    if mesh.indices().is_empty() {
        return Ok(None);
    }
    // Meshes are bounded where they're built, but the device's limit can be
    // lower, and a buffer past it is a validation error, which wgpu panics
    // on.
    let limit = device.limits().max_buffer_size;
    // The kernel's limits keep every part within `MAX_BUFFER_BYTES`, so
    // this doesn't saturate.
    let bytes = |len: usize, size: usize| len.saturating_mul(size) as u64;
    let (edge_points, polyline_ends) = if mesh.edge_vertices().is_empty() {
        (None, Vec::new())
    } else {
        let (points, ends) = edge_stream(mesh);
        (Some(points), ends)
    };
    let largest = [
        bytes(mesh.positions().len(), size_of::<[f32; 3]>()),
        bytes(mesh.indices().len(), size_of::<u32>()),
        bytes(
            edge_points.as_ref().map_or(0, Vec::len),
            size_of::<EdgePoint>(),
        ),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if largest > limit {
        return Err(PrepareError::MeshTooLarge {
            bytes: largest,
            limit,
        });
    }
    let edges = edge_points.map(|points| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh edges"),
            contents: bytemuck::cast_slice(&points),
            usage: wgpu::BufferUsages::VERTEX,
        })
    });
    // Where a part's edges' points start in the stream: after the
    // stream's first point, and after the edges before it.
    let point = |edge: usize| {
        edge.checked_sub(1)
            .map_or(1, |before| polyline_ends[before])
    };
    // The kernel's limits keep these well within `u32`.
    let to_u32 = |range: Range<usize>| {
        let at = |n| u32::try_from(n).expect("kernel bounds indices");
        at(range.start)..at(range.end)
    };
    let part = |indices: Range<usize>, edges: Range<usize>| GpuPart {
        bounds: bounds_of(mesh.positions(), &mesh.indices()[indices.clone()]),
        indices: to_u32(indices),
        points: point(edges.start)..point(edges.end),
    };
    let mut parts: Vec<GpuPart> = mesh
        .parts()
        .map(|part_of| part(part_of.indices, part_of.edges))
        .collect();
    if parts.is_empty() {
        parts.push(part(0..mesh.indices().len(), 0..mesh.edge_count()));
    }

    Ok(Some(GpuMesh {
        positions: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh positions"),
            contents: bytemuck::cast_slice(mesh.positions()),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        // As many as positions, so the same size.
        normals: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh normals"),
            contents: bytemuck::cast_slice(mesh.normals()),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh indices"),
            contents: bytemuck::cast_slice(mesh.indices()),
            usage: wgpu::BufferUsages::INDEX,
        }),
        edges,
        parts,
        bounds: mesh.bounds(),
    }))
}

/// The bounds of the positions `indices` refer to, or `None` if there are
/// none.
fn bounds_of(positions: &[[f32; 3]], indices: &[u32]) -> Option<Aabb> {
    let mut points = indices.iter().map(|&i| Vec3::from(positions[i as usize]));
    let first = points.next()?;
    let (min, max) = points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
    Some(Aabb { min, max })
}

/// The [`EdgePoint`] stream of `mesh`'s feature edges, and where each
/// polyline's points end in it, one past its last.
fn edge_stream(mesh: &RenderMesh) -> (Vec<EdgePoint>, Vec<u32>) {
    let none = EdgePoint {
        position: [0.0; 3],
        along: 0.0,
        edge: NO_EDGE,
    };
    let mut points = Vec::with_capacity(mesh.edge_vertices().len().saturating_add(2));
    let mut ends = Vec::with_capacity(mesh.edge_count());
    points.push(none);
    let positions = mesh.positions();
    // A polyline's points, a point again in a row left out: a segment of
    // no length between two others would keep them from seeing each
    // other, and both would draw where they overlap at the join.
    let mut kept: Vec<[f32; 3]> = Vec::new();
    // The kernel bounds the polylines well within `u32`, below
    // `NEIGHBOUR_ONLY`.
    for (edge, polyline) in (0..).zip(mesh.polylines()) {
        kept.clear();
        for &vertex in polyline {
            let position = positions[vertex as usize];
            if kept.last() != Some(&position) {
                kept.push(position);
            }
        }
        let neighbour = |position| EdgePoint {
            position,
            along: 0.0,
            edge: edge | NEIGHBOUR_ONLY,
        };
        // Closed, round three segments or more; two would be one there and
        // back, joined at its turns already.
        let closed = kept.len() >= 4 && kept.first() == kept.last();
        if closed {
            points.push(neighbour(kept[kept.len() - 2]));
        }
        // Summed in `f64`, so a long polyline of short segments doesn't
        // drift. Positions are bounded, so it stays finite.
        let mut along = 0.0f64;
        for (i, &position) in kept.iter().enumerate() {
            if let Some(&last) = i.checked_sub(1).and_then(|i| kept.get(i)) {
                along += f64::from(Vec3::from(position).distance(Vec3::from(last)));
            }
            points.push(EdgePoint {
                position,
                along: along as f32,
                edge,
            });
        }
        if closed {
            points.push(neighbour(kept[1]));
        }
        // One left of the polyline, all its points the same, is a dot.
        if let [point] = kept[..] {
            points.push(EdgePoint {
                position: point,
                along: 0.0,
                edge,
            });
        }
        // The kernel's limits keep the stream well within `u32`.
        ends.push(u32::try_from(points.len()).expect("kernel bounds edges"));
    }
    points.push(none);
    (points, ends)
}

/// Uploads `lines` a segment each, or nothing if there are none. See
/// [`upload_mesh`] for the limit.
fn upload_lines(
    device: &wgpu::Device,
    lines: &RenderLines,
) -> Result<Option<GpuLines>, PrepareError> {
    if lines.segment_count() == 0 {
        return Ok(None);
    }
    let limit = device.limits().max_buffer_size;
    // The kernel's limits keep this within `MAX_BUFFER_BYTES`.
    let bytes = lines.segment_count().saturating_mul(size_of::<Segment>()) as u64;
    if bytes > limit {
        return Err(PrepareError::LinesTooLarge { bytes, limit });
    }
    let segments: Vec<Segment> = lines
        .polylines()
        .flat_map(|polyline| polyline.windows(2).map(|pair| [pair[0], pair[1]]))
        .collect();
    let segment_count = u32::try_from(segments.len()).expect("kernel bounds points");

    Ok(Some(GpuLines {
        segments: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde sketch lines"),
            contents: bytemuck::cast_slice(&segments),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        segment_count,
        bounds: lines.bounds(),
    }))
}

/// A layer of the sketch being edited on the GPU: a buffer for each kind
/// of thing it draws.
#[derive(Default)]
struct SketchBuffers {
    lines: Instances,
    points: Instances,
    fills: Instances,
}

impl SketchBuffers {
    /// Replaces what the buffers hold with `layer`. On failing, holds
    /// nothing.
    fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SketchLayer,
    ) -> Result<(), PrepareError> {
        let written = self
            .lines
            .write(device, queue, "varde sketch lines", &layer.lines)
            .and_then(|()| {
                self.points
                    .write(device, queue, "varde sketch points", &layer.points)
            })
            .and_then(|()| {
                self.fills
                    .write(device, queue, "varde sketch fills", &layer.fills)
            });
        if written.is_err() {
            *self = SketchBuffers::default();
        }
        written
    }
}

/// A vertex buffer written again and again, kept while what's written fits
/// in it, and how many things it holds now.
#[derive(Default)]
struct Instances {
    buffer: Option<wgpu::Buffer>,
    count: u32,
}

impl Instances {
    /// The smallest buffer made, in bytes, so a layer growing a little at a
    /// time doesn't make a buffer each time.
    const MIN_BYTES: u64 = 4096;

    /// Replaces what it holds with `items`, in a larger buffer if they
    /// don't fit.
    fn write<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        items: &[T],
    ) -> Result<(), PrepareError> {
        let bytes: &[u8] = bytemuck::cast_slice(items);
        let size = bytes.len() as u64;
        let limit = device.limits().max_buffer_size;
        let count = u32::try_from(items.len()).ok().filter(|_| size <= limit);
        let Some(count) = count else {
            self.count = 0;
            return Err(PrepareError::SketchTooLarge { bytes: size, limit });
        };
        self.count = count;
        if items.is_empty() {
            return Ok(());
        }
        if self.buffer.as_ref().is_none_or(|b| b.size() < size) {
            // Within the limit, which `size` is.
            let capacity = size
                .checked_next_power_of_two()
                .unwrap_or(size)
                .max(Self::MIN_BYTES)
                .min(limit)
                .max(size);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.buffer {
            queue.write_buffer(buffer, 0, bytes);
        }
        Ok(())
    }

    /// The part of the buffer to draw from, if it holds anything.
    fn drawn(&self) -> Option<wgpu::BufferSlice<'_>> {
        let buffer = self.buffer.as_ref().filter(|_| self.count > 0)?;
        Some(buffer.slice(..))
    }
}

fn create_depth(device: &wgpu::Device, [width, height]: [u32; 2]) -> DepthTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("varde depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    DepthTarget {
        view: texture.create_view(&Default::default()),
        size: [width, height],
    }
}

#[cfg(test)]
mod tests;
