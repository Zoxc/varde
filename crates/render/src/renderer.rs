use std::any::Any;
use std::ops::Range;
use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use varde_kernel::{Aabb, RenderLines, RenderMesh};
use wgpu::util::DeviceExt;

use crate::Camera;
use crate::highlight::{Highlights, VertexInstance};
use crate::scene::{self, GRID_FADE_HEIGHTS, GridPlane};
use crate::sketch::{FillVertex, LineInstance, PointInstance, SketchLayer, SketchScene, Srgba};

/// The depth buffer's format on `device`, with a stencil: 32 bit float
/// depth if it has that, else 24 bits. The stencil marks which part less
/// than opaque is nearest at a pixel.
fn depth_format(device: &wgpu::Device) -> wgpu::TextureFormat {
    if device
        .features()
        .contains(wgpu::Features::DEPTH32FLOAT_STENCIL8)
    {
        wgpu::TextureFormat::Depth32FloatStencil8
    } else {
        wgpu::TextureFormat::Depth24PlusStencil8
    }
}

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

/// How wide creases are drawn, in logical pixels: feature edges inside one
/// face (see [`RenderMesh::edge_faces`]), where its patches' normals part.
pub const CREASE_WIDTH: f32 = 1.0;

/// How opaque creases are drawn, from 0 to 1, of what the feature edges
/// are, seen and hidden alike: they're how a face was cut into patches,
/// not where it ends.
pub const CREASE_ALPHA: f32 = 0.35;

/// How wide the edges the model hides are drawn, in logical pixels: see
/// [`Frame::hidden_edges`].
pub const HIDDEN_EDGE_WIDTH: f32 = 1.0;

/// The dashes of the edges the model hides: how long a dash and a gap are
/// along the edge, in logical pixels at the target.
pub const HIDDEN_DASH: [f32; 2] = [4.0, 3.0];

/// How wide the hovered edges are drawn, in logical pixels, within their
/// rim: see [`Highlights::outlined`].
pub const HOVERED_EDGE_WIDTH: f32 = 2.5;

/// How wide the rim of [`Colors::hover_outline`] is around the hovered
/// edges and a hovered vertex, in logical pixels: see
/// [`Highlights::outlined`].
pub const HOVER_RIM: f32 = 1.5;

/// How wide the faint white rim around the selected edges and vertices
/// is, in logical pixels, for contrast with what's behind them.
pub const SELECTED_RIM: f32 = 1.0;

/// How wide the selected edges are drawn, in logical pixels, over their
/// [`EDGE_WIDTH`]: see [`Highlights::selected_edges`].
pub const SELECTED_EDGE_WIDTH: f32 = 2.5;

/// The radius of a hovered or selected vertex's disc, in logical pixels,
/// within its rim: see [`Highlights::vertices`].
pub const VERTEX_RADIUS: f32 = 3.5;

/// How wide error geometry's curves are drawn, in logical pixels: see
/// [`Frame::errors`].
pub const ERROR_EDGE_WIDTH: f32 = 3.0;

/// How far the halo around error geometry reaches beyond it on every side,
/// in logical pixels: see [`Frame::errors`].
pub const ERROR_HALO: f32 = 12.0;

/// The radius of an error point's disc, in logical pixels: see
/// [`Frame::errors`].
pub const ERROR_POINT_RADIUS: f32 = 4.0;

/// How strongly error geometry is drawn where the model hides it, of its
/// full strength, core and halo alike.
const ERROR_HIDDEN: f32 = 0.4;

/// The format of the target the errors' halo is drawn into as coverage.
const HALO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

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
    /// order of [`RenderMesh::parts`]: faces and edges, hidden ones too. A
    /// part with no entry, out of range or NaN is opaque, and so is one
    /// within half an 8 bit step of 1. Ignored while [`Self::faded`].
    /// Changing it re-uploads nothing.
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
    /// Whether the mesh's wires ([`RenderMesh::wires`]) are drawn with its
    /// edges, as creases are: [`CREASE_WIDTH`] wide at [`CREASE_ALPHA`].
    /// Changing it re-uploads nothing.
    pub wireframe: bool,
    /// Whether the feature edges the model hides are drawn too, dashed
    /// ([`HIDDEN_DASH`]), [`HIDDEN_EDGE_WIDTH`] wide, at
    /// [`Colors::hidden_edge_alpha`]. Never while [`Self::faded`].
    pub hidden_edges: bool,
    /// The faces hovered, by their ids in the mesh (their runs of
    /// [`RenderMesh::face_ends`]): the one the cursor is over, or all of
    /// what it would select (a body's). Each is drawn again over itself
    /// towards [`Colors::hover_face`], as opaque as its part. Not drawn
    /// while [`Self::faded`], nor a face the mesh hasn't.
    pub hovered_faces: &'a [u32],
    /// The faces selected, by their ids likewise: drawn again over
    /// themselves, tinted with [`Colors::selected`], over the hover.
    pub selected_faces: &'a [u32],
    /// The faces in the second colour (the measure tool's B), by their
    /// ids likewise: tinted with [`Colors::second`] as the selected are
    /// with theirs, over them.
    pub second_faces: &'a [u32],
    /// The edges and vertices hovered and selected, drawn over the model.
    /// Only re-uploaded when it's another `Arc` than the last one
    /// prepared, or the mesh is. Not drawn while [`Self::faded`].
    pub highlights: &'a Arc<Highlights>,
    /// The sketch being edited, or the extrude being set up, if one is:
    /// drawn over everything else, or hidden by the model in front of it
    /// ([`SketchScene::depth_tested`]).
    pub sketch: Option<SketchScene<'a>>,
    /// The geometry of the failures shown, drawn after everything else,
    /// faded or not: solid red ([`Colors::error`]) within a halo of
    /// [`Colors::error_halo`] reaching [`ERROR_HALO`] beyond it, depth
    /// tested, and at about 40 % where the model hides it. Uploaded again
    /// only when its sources ([`ErrorParts::source`]) differ from the last
    /// frame prepared's.
    pub errors: &'a [ErrorParts<'a>],
    /// The point the camera orbits, if one was picked: marked over
    /// everything but the sketch being edited, unless it shows where the
    /// origin does.
    pub pivot: Option<Pivot>,
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

/// The drawable parts of a failure's geometry, for [`Frame::errors`].
#[derive(Debug, Clone)]
pub struct ErrorParts<'a> {
    /// Patches, filled red and lit as faces are: only its triangles are
    /// drawn.
    pub mesh: &'a RenderMesh,
    /// Curves, [`ERROR_EDGE_WIDTH`] wide.
    pub lines: &'a RenderLines,
    /// Points, discs of radius [`ERROR_POINT_RADIUS`]. One not finite or
    /// past [`RenderLines::MAX_POSITION`] isn't drawn.
    pub points: &'a [[f32; 3]],
    /// What these are the parts of, kept unchanged while they are (the
    /// failure's `Arc`): the renderer uploads the errors again only when
    /// one of them is another allocation than the last frame's.
    pub source: Weak<dyn Any + Send + Sync>,
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
    /// How opaque the model's faces and edges are when [`Frame::faded`],
    /// from 0 to 1. Only its nearest faces are drawn, and they still hide
    /// what's behind them from the grid and the sketches.
    pub faded_alpha: f32,
    /// How opaque the edges the model hides are drawn, from 0 to 1, of
    /// [`Self::edge`]: see [`Frame::hidden_edges`].
    pub hidden_edge_alpha: f32,
    /// The hovered faces' colour, before lighting: brighter than
    /// [`Self::model`]. See [`Frame::hovered_faces`].
    pub hover_face: Srgb,
    /// The rim around the hovered edges and vertex, bright for contrast:
    /// see [`HOVER_RIM`].
    pub hover_outline: Srgb,
    /// The accent: selected faces are tinted with it, by
    /// [`Self::selected_tint`], selected edges and vertices drawn in it,
    /// shaded by [`Self::selected_edge_shade`].
    pub selected: Srgb,
    /// How far a selected face is tinted towards [`Self::selected`], from
    /// 0 to 1. Out of range or NaN is 0.6.
    pub selected_tint: f32,
    /// How far the selected edges' and vertices' colour is from
    /// [`Self::selected`], so they stand out on a selected face: from -1,
    /// black, through 0, the accent itself, to 1, white, of the way in
    /// linear light. Out of range or NaN is 0.
    pub selected_edge_shade: f32,
    /// The second colour, for the second of two picks (the measure
    /// tool's B), where the first is in [`Self::selected`]: faces tinted
    /// with it as the selected are with theirs, and edges drawn in it,
    /// unshaded. See
    /// [`Frame::second_faces`] and [`Highlights::second_edges`].
    pub second: Srgb,
    /// Error geometry ([`Frame::errors`]): its patches (lit), curves and
    /// points.
    pub error: Srgb,
    /// The halo around error geometry, translucent: drawn once over the
    /// frame wherever any of it reaches, so overlaps don't darken. Alpha
    /// out of range or NaN is 0.3.
    pub error_halo: Srgba,
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
    /// [`Colors`], converted to linear with w = 1, except `model`, whose
    /// w is how opaque the model is, and `edge`, whose w is
    /// [`Colors::hidden_edge_alpha`]. The axes' w's are
    /// [`Colors::error`]'s red, green and blue; `origin_outline`'s,
    /// `sketch`'s and `pivot_color`'s [`Colors::error_halo`]'s, and
    /// `hover_outline`'s its alpha.
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
    /// [`Frame::grid`]: xyz its origin, and unit x and y axes. The axes'
    /// w is [`axis_index`], for the colour of its axis line; the origin's
    /// is unused.
    grid_origin: [f32; 4],
    grid_x: [f32; 4],
    grid_y: [f32; 4],
    /// [`SketchScene::plane`]: xyz its origin, and unit x and y axes; the
    /// w's [`Colors::second`]'s red, green and blue.
    sketch_origin: [f32; 4],
    sketch_x: [f32; 4],
    sketch_y: [f32; 4],
    /// [`Colors::hover_face`], with w [`Colors::selected_tint`],
    /// [`Colors::hover_outline`], with w [`Colors::error_halo`]'s alpha,
    /// and [`Colors::selected`], with w [`Colors::selected_edge_shade`].
    hover_face: [f32; 4],
    hover_outline: [f32; 4],
    selected: [f32; 4],
}

// WGSL lays out uniform structs in 16-byte steps.
const _: () = assert!(size_of::<Uniforms>().is_multiple_of(16));
// The limits tests' device takes buffers of 512 bytes at most, and the
// uniforms are at that: a new value has to go in an unused `w`, or be
// worked out from another (as the axis lines' colours are from `grid_x.w`).
const _: () = assert!(size_of::<Uniforms>() <= 512);

/// The largest buffer the renderer relies on, in bytes: WebGPU's default
/// `maxBufferSize`, which the devices iced asks for have natively and on
/// the web. A larger buffer is a validation error, and wgpu panics on those.
const MAX_BUFFER_BYTES: usize = 256 << 20;

// Every mesh the kernel allows fits, one buffer per part: positions,
// normals and indices as they are; edges at most one and a half
// `EdgePoint`s per edge vertex (closed polylines get two more), plus one at
// each end.
const _: () = assert!(size_of::<[f32; 3]>() * RenderMesh::MAX_VERTICES <= MAX_BUFFER_BYTES);
const _: () = assert!(size_of::<u32>() * RenderMesh::MAX_INDICES <= MAX_BUFFER_BYTES);
const _: () =
    assert!(size_of::<EdgePoint>() * (RenderMesh::MAX_EDGE_POINTS / 2 * 3 + 2) <= MAX_BUFFER_BYTES);
// Lines are uploaded a segment of two points each, fewer than points.
const _: () = assert!(size_of::<Segment>() * RenderLines::MAX_POINTS <= MAX_BUFFER_BYTES);

/// A segment of a line as the GPU takes it: its two ends.
type Segment = [[f32; 3]; 2];

/// A point of the stream the feature edges are drawn from (see
/// [`EdgeStream`] and `agents/viewport.md`). The stream is bound to
/// [`EDGE_SLOTS`] vertex buffer slots a point apart, so the instance
/// drawing the segment from point `i + 1` to `i + 2` sees the points either
/// side too. See `edge_segment` in the shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct EdgePoint {
    pub(crate) position: [f32; 3],
    /// How far along its polyline it is, in world units.
    along: f32,
    /// Which polyline it's on.
    pub(crate) edge: u32,
}

/// The edge of the points at the ends of the stream: no polyline's.
const NO_EDGE: u32 = u32::MAX;

/// Set in [`EdgePoint::edge`] for a point that is only a neighbour of its
/// edge's segments, where a closed polyline joins itself. As
/// `NEIGHBOUR_ONLY` in the shader.
const NEIGHBOUR_ONLY: u32 = 1 << 31;

/// Set in [`EdgePoint::edge`] for a crease's points: an edge with one face
/// on both sides. As `CREASE` in the shader.
const CREASE: u32 = 1 << 30;
const _: () = assert!(RenderMesh::MAX_EDGE_POLYLINES <= CREASE as usize);

/// How many slots the edge stream is bound to: previous, start, end and
/// next point.
const EDGE_SLOTS: u32 = 4;

// Vertex buffer offsets are multiples of 4 bytes, and wgpu's GL backend
// (the browser's WebGL2) allows strides up to 255.
const _: () = assert!(size_of::<EdgePoint>() == 20);

/// The most alphas a part can be drawn at, from 0 to 1: 8 bits' worth, as
/// fine as the target shows. See [`Alphas`].
const ALPHA_STEPS: u32 = 256;

/// What a draw of a part of the mesh takes besides [`Uniforms`]. `Part`
/// in `scene.wgsl` mirrors it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PartUniforms {
    /// x: the part's alpha, multiplying the faces' and edges' own; yzw
    /// unused.
    alpha: [f32; 4],
}

/// The alphas the mesh's parts are drawn at: a uniform buffer, written
/// once, of a [`PartUniforms`] for each of [`ALPHA_STEPS`] steps from 0 to
/// 1 at the device's uniform offset alignment, picked per draw by dynamic
/// offset. WebGL2 has no push constants. A device whose buffers hold fewer
/// gets fewer, at least one, opaque.
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

/// The nearest step to `alpha` of a table of alphas from 0 to 1 whose last
/// step, `opaque`, is opaque; opaque if `alpha` is out of range or NaN.
/// Only 0 gets step 0, so on a short table (tiny buffers) a faint part is
/// drawn at the next step up rather than not at all.
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
    /// The [`EdgePoint`] stream, if there are any edges or wires.
    edges: Option<wgpu::Buffer>,
    /// Whether the wires are drawn with the edges: [`Frame::wireframe`].
    wireframe: bool,
    /// The mesh's parts, as [`RenderMesh::parts`] gives them; one for the
    /// whole mesh if it has none.
    parts: Vec<GpuPart>,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
}

impl GpuMesh {
    /// The indices of face `face` of `mesh`, the mesh uploaded, and the
    /// part it's of, if it has one.
    fn face(&self, mesh: &RenderMesh, face: u32) -> Option<(Range<u32>, usize)> {
        let indices = mesh.face_indices(usize::try_from(face).ok()?)?;
        let part = self.parts.partition_point(|part| part.faces.end <= face);
        // The kernel bounds indices well within `u32`.
        let to_u32 = |n| u32::try_from(n).ok();
        let indices = to_u32(indices.start)?..to_u32(indices.end)?;
        (part < self.parts.len()).then_some((indices, part))
    }
}

/// A face of the mesh drawn again over itself, hovered or selected: see
/// [`Frame::hovered_faces`].
#[derive(Debug, Clone)]
struct FaceDraw {
    indices: Range<u32>,
    /// Its part's alpha's step, see [`Alphas`].
    step: u32,
    /// Brightened as hovered, or tinted as selected or in the second
    /// colour.
    tint: Tint,
}

/// How a [`FaceDraw`] is drawn again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tint {
    Hovered,
    Selected,
    Second,
}

/// What the renderer keeps of a part of the mesh to draw it on its own.
#[derive(Debug, Clone)]
struct GpuPart {
    /// Its faces' ids.
    faces: Range<u32>,
    /// Its triangles' indices.
    indices: Range<u32>,
    /// Its edges' points in the [`EdgePoint`] stream, neighbour only ones
    /// included. The parts' follow one another from the stream's second
    /// point, and all the parts' wires follow them.
    points: Range<u32>,
    /// Its wires' points in the stream, likewise.
    wires: Range<u32>,
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

/// A pipeline's stencil state when it neither tests nor writes it.
const NO_STENCIL: wgpu::StencilState = wgpu::StencilState {
    front: wgpu::StencilFaceState::IGNORE,
    back: wgpu::StencilFaceState::IGNORE,
    read_mask: 0,
    write_mask: 0,
};

/// The errors' halo's blending: each pixel keeps the most coverage drawn
/// there, so overlapping halos don't add up.
const MAX_BLENDING: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    },
};

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
    stencil: wgpu::StencilState,
    /// Sets the shader's `SKETCH_DEPTH`: the sketch's layers are given
    /// their depth, pulled towards the camera, rather than drawn on top.
    sketch_depth: bool,
    /// The target's format if not the frame's: the errors' halo's.
    format: Option<wgpu::TextureFormat>,
    /// The layout if not the scene's: the halo's composite's, which reads
    /// the halo's coverage.
    layout: Option<&'a wgpu::PipelineLayout>,
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
            stencil: NO_STENCIL,
            sketch_depth: false,
            format: None,
            layout: None,
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
    /// The same for the hovered and selected edges and vertices.
    HighlightsTooLarge { bytes: u64, limit: u64 },
    /// The same for the errors' geometry ([`Frame::errors`]).
    ErrorsTooLarge { bytes: u64, limit: u64 },
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
            PrepareError::HighlightsTooLarge { bytes, limit } => write!(
                f,
                "what's hovered and selected needs a {bytes}-byte buffer, but the device allows \
                 at most {limit} bytes"
            ),
            PrepareError::ErrorsTooLarge { bytes, limit } => write!(
                f,
                "the errors' geometry needs a {bytes}-byte buffer, but the device allows at most \
                 {limit} bytes"
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
    /// The edges again where the model hides them, dashed.
    hidden_edges: wgpu::RenderPipeline,
    /// A part less than opaque's front faces' depth, marking where it's
    /// the nearest in the stencil, and the edges it hides there, dashed.
    glass_depth: wgpu::RenderPipeline,
    hidden_by_glass: wgpu::RenderPipeline,
    /// Faces drawn again over themselves (`Equal`), hovered and selected.
    hover_face: wgpu::RenderPipeline,
    selected_face: wgpu::RenderPipeline,
    second_face: wgpu::RenderPipeline,
    /// The rim of the hovered edges' outline, the hovered edges within
    /// it, the selected edges, and the hovered and selected vertices.
    outline: wgpu::RenderPipeline,
    hovered_edges: wgpu::RenderPipeline,
    selected_outline: wgpu::RenderPipeline,
    selected_edges: wgpu::RenderPipeline,
    /// The edges in the second colour, with their rim.
    second_outline: wgpu::RenderPipeline,
    second_edges: wgpu::RenderPipeline,
    vertices: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    origin: wgpu::RenderPipeline,
    /// The sketch being edited: its fills, lines and points, drawn over
    /// everything, and the same hidden by the model in front of them
    /// ([`SketchScene::depth_tested`]).
    sketch_on_top: SketchPipelines,
    sketch_depth_tested: SketchPipelines,
    /// The errors' geometry: its halo, its composite and its core.
    errors: ErrorPipelines,
    bind_group_layout: wgpu::BindGroupLayout,
    alphas: Alphas,
    depth_format: wgpu::TextureFormat,
    /// The target's format, which the pipelines are built for.
    format: wgpu::TextureFormat,
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
    /// The faces hovered and selected, the hovered first: none while
    /// [`Frame::faded`].
    faces: Vec<FaceDraw>,
    /// [`Frame::highlights`] as uploaded, and its `Arc` like `source`.
    highlights: HighlightBuffers,
    highlights_source: Weak<Highlights>,
    /// [`Frame::errors`] as uploaded, and their sources like `source`.
    errors: ErrorBuffers,
    error_sources: Vec<Weak<dyn Any + Send + Sync>>,
    /// The target the errors' halo is drawn into, made the first time
    /// there are errors, at the target's size.
    halo: Option<HaloTarget>,
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

        // A uniform buffer of at least `size` bytes, at a dynamic offset
        // if `dynamic`, seen by both stages.
        let uniform_layout = |label, size: usize, dynamic| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: dynamic,
                        min_binding_size: wgpu::BufferSize::new(size as u64),
                    },
                    count: None,
                }],
            })
        };
        let bind_group_layout = uniform_layout("varde uniforms", size_of::<Uniforms>(), false);
        // How opaque the part drawn is, at a dynamic offset into the
        // alphas' buffer: see `Alphas`. Every pipeline has it, so
        // it's bound once for all of them.
        let part_layout = uniform_layout("varde part", size_of::<PartUniforms>(), true);
        let alphas = Alphas::new(device, &part_layout);
        let depth_format = depth_format(device);

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde scene"),
            bind_group_layouts: &[&bind_group_layout, &part_layout],
            push_constant_ranges: &[],
        });
        // The errors' halo's coverage, read texel by texel (`textureLoad`,
        // no sampler) by its composite, as group 2.
        let halo_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde error halo"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde error halo composite"),
            bind_group_layouts: &[&bind_group_layout, &part_layout, &halo_layout],
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
                ("CREASE_WIDTH", f64::from(CREASE_WIDTH)),
                ("CREASE_ALPHA", f64::from(CREASE_ALPHA)),
                ("HIDDEN_DASH", f64::from(HIDDEN_DASH[0])),
                ("HIDDEN_GAP", f64::from(HIDDEN_DASH[1])),
                ("HOVERED_EDGE_WIDTH", f64::from(HOVERED_EDGE_WIDTH)),
                ("HOVER_RIM", f64::from(HOVER_RIM)),
                ("SELECTED_RIM", f64::from(SELECTED_RIM)),
                ("SELECTED_EDGE_WIDTH", f64::from(SELECTED_EDGE_WIDTH)),
                ("VERTEX_RADIUS", f64::from(VERTEX_RADIUS)),
                ("ERROR_EDGE_WIDTH", f64::from(ERROR_EDGE_WIDTH)),
                ("ERROR_HALO", f64::from(ERROR_HALO)),
                ("ERROR_POINT_RADIUS", f64::from(ERROR_POINT_RADIUS)),
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
                layout: Some(pass.layout.unwrap_or(&layout)),
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
                        format: pass.format.unwrap_or(format),
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
                    format: depth_format,
                    depth_write_enabled: pass.depth_write,
                    depth_compare: pass.depth_compare,
                    stencil: pass.stencil,
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
                4 => Float32, 5 => Uint32,
            ],
        };
        let sketch_fills = wgpu::VertexBufferLayout {
            array_stride: size_of::<FillVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Uint32, 2 => Float32x4, 3 => Float32,
            ],
        };
        // The feature edges: the `EdgePoint` stream in slots 0 to 3, a
        // point apart, an instance per segment. Only the start needs
        // `along`, for the dashes. 9 attributes and 4 slots, within
        // WebGL2's 16 and 8.
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
        // The hovered and selected vertices, a disc per instance.
        let vertices = wgpu::VertexBufferLayout {
            array_stride: size_of::<VertexInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32],
        };
        let faces = [positions, normals];
        let mesh = Pass {
            label: "varde mesh",
            vs: "vs_mesh",
            fs: "fs_mesh",
            buffers: &faces,
            depth_write: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            blend: wgpu::BlendState::REPLACE,
            write_mask: wgpu::ColorWrites::ALL,
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: Some(wgpu::Face::Back),
            stencil: NO_STENCIL,
            sketch_depth: false,
            format: None,
            layout: None,
        };
        // Tested against the stencil reference by `compare`, and on a pass
        // of both tests, `pass` done to it.
        let tagged = |compare, pass| {
            let face = wgpu::StencilFaceState {
                compare,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: pass,
            };
            wgpu::StencilState {
                front: face,
                back: face,
                read_mask: 0xff,
                write_mask: 0xff,
            }
        };
        // Lines drawn from the `EdgePoint` stream, over the scene.
        let edge_pass = |label, vs| Pass {
            buffers: &edge_points,
            ..Pass::overlay(label, vs, "fs_line")
        };
        // The hover and the selection's lines, pulled in by their distance
        // from the edge (`highlight_slope` in the shader).
        let highlight_pass = |label, vs| Pass {
            fs: "fs_highlight_line",
            ..edge_pass(label, vs)
        };
        // Over the faces' own pixels and no others: the same vertex
        // shader, its position invariant, at exactly their depth.
        let redrawn = |label, fs| {
            pipeline(Pass {
                label,
                fs,
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Equal,
                ..mesh.clone()
            })
        };

        // The errors' geometry where it shows and where the model hides
        // it: the same programs, so the two split the pixels between them.
        let (seen, hidden) = (
            wgpu::CompareFunction::LessEqual,
            wgpu::CompareFunction::Greater,
        );

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
            // Marking the part's pixels where it's the nearest of the model
            // so far with the stencil reference.
            glass_depth: pipeline(Pass {
                label: "varde glass depth",
                write_mask: wgpu::ColorWrites::empty(),
                stencil: tagged(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Replace,
                ),
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
            hover_face: redrawn("varde hovered face", "fs_hover_face"),
            selected_face: redrawn("varde selected face", "fs_selected_face"),
            second_face: redrawn("varde second face", "fs_second_face"),
            mesh: pipeline(mesh),
            // Entry points of their own, which wgpu's GL backend keys
            // programs by.
            outline: pipeline(highlight_pass("varde hover outline", "vs_outline")),
            hovered_edges: pipeline(highlight_pass("varde hovered edges", "vs_hovered_edge")),
            selected_outline: pipeline(highlight_pass(
                "varde selected outline",
                "vs_selected_outline",
            )),
            selected_edges: pipeline(highlight_pass("varde selected edges", "vs_selected_edge")),
            second_outline: pipeline(highlight_pass("varde second outline", "vs_second_outline")),
            second_edges: pipeline(highlight_pass("varde second edges", "vs_second_edge")),
            vertices: pipeline(Pass {
                buffers: std::slice::from_ref(&vertices),
                ..Pass::overlay("varde vertices", "vs_vertex", "fs_highlight_point")
            }),
            edges: pipeline(edge_pass("varde edges", "vs_edge")),
            // Exactly the pixels the visible edges didn't draw: the same
            // quads at the same depth, tested the other way round. Its own
            // entry point, which wgpu's GL backend keys programs by.
            hidden_edges: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..edge_pass("varde hidden edges", "vs_hidden_edge")
            }),
            // The same where the stencil has the reference: behind the
            // part less than opaque nearest there.
            hidden_by_glass: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: tagged(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Keep),
                ..edge_pass("varde edges hidden by glass", "vs_hidden_edge")
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
            errors: ErrorPipelines {
                halo: [seen, hidden].map(|depth_compare| {
                    let halo = |label, vs, fs, buffers| {
                        pipeline(Pass {
                            label,
                            buffers,
                            depth_compare,
                            blend: MAX_BLENDING,
                            format: Some(HALO_FORMAT),
                            ..Pass::overlay(label, vs, fs)
                        })
                    };
                    ErrorLayer {
                        faces: halo(
                            "varde error halo, faces",
                            "vs_error_face",
                            "fs_halo_face",
                            &faces,
                        ),
                        lines: halo(
                            "varde error halo, lines",
                            "vs_error_halo_line",
                            "fs_halo_line",
                            &edge_points,
                        ),
                        points: halo(
                            "varde error halo, points",
                            "vs_error_halo_point",
                            "fs_halo_point",
                            std::slice::from_ref(&vertices),
                        ),
                    }
                }),
                composite: pipeline(Pass {
                    depth_compare: wgpu::CompareFunction::Always,
                    layout: Some(&composite_layout),
                    ..Pass::overlay(
                        "varde error halo composite",
                        "vs_fullscreen",
                        "fs_error_halo",
                    )
                }),
                core: [seen, hidden].map(|depth_compare| {
                    let core = |label, vs, fs, buffers| {
                        pipeline(Pass {
                            label,
                            buffers,
                            depth_compare,
                            ..Pass::overlay(label, vs, fs)
                        })
                    };
                    ErrorLayer {
                        faces: core(
                            "varde error faces",
                            "vs_error_face",
                            "fs_error_face",
                            &faces,
                        ),
                        lines: core(
                            "varde error lines",
                            "vs_error_line",
                            "fs_highlight_line",
                            &edge_points,
                        ),
                        points: core(
                            "varde error points",
                            "vs_error_point",
                            "fs_highlight_point",
                            std::slice::from_ref(&vertices),
                        ),
                    }
                }),
                halo_layout,
            },
            bind_group_layout,
            alphas,
            depth_format,
            format,
        }
    }

    /// The format of the targets it draws to, as it was made for.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
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
            sketching: false,
            sketch_depth: false,
            depth: None,
            viewport: Viewport::default(),
            faded: false,
            draws: PartDraws::default(),
            hidden_edges: false,
            faces: Vec::new(),
            highlights: HighlightBuffers::default(),
            highlights_source: Weak::new(),
            errors: ErrorBuffers::default(),
            error_sources: Vec::new(),
            halo: None,
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
    /// it's too large, and the errors' geometry with
    /// [`PrepareError::ErrorsTooLarge`], tried again only once their
    /// sources change; should several fail at once, the mesh's error is
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
        let new_mesh = !std::ptr::eq(slot.source.as_ptr(), Arc::as_ptr(frame.mesh));
        if new_mesh {
            slot.source = Arc::downgrade(frame.mesh);
            slot.mesh = upload_mesh(device, frame.mesh).unwrap_or_else(|error| {
                result = Err(error);
                None
            });
        }
        // Faded, every part is drawn alike.
        let opacity = if frame.faded { &[] } else { frame.opacity };
        if let Some(mesh) = &mut slot.mesh {
            mesh.wireframe = frame.wireframe;
        }
        let parts = slot.mesh.as_ref().map_or(&[][..], |mesh| &mesh.parts);
        slot.draws = PartDraws::new(parts, opacity, &self.alphas, frame.camera);

        // What's hovered and selected names the mesh's faces, edges and
        // vertices, so it's written again with the mesh too.
        if new_mesh
            || !std::ptr::eq(
                slot.highlights_source.as_ptr(),
                Arc::as_ptr(frame.highlights),
            )
        {
            slot.highlights_source = Arc::downgrade(frame.highlights);
            let written = slot
                .highlights
                .write(device, queue, frame.mesh, frame.highlights);
            result = result.and(written);
        }
        // The failures shown keep their `Arc`s while they're unchanged.
        let same_errors = slot.error_sources.len() == frame.errors.len()
            && (slot.error_sources.iter())
                .zip(frame.errors)
                .all(|(kept, error)| Weak::ptr_eq(kept, &error.source));
        if !same_errors {
            slot.error_sources = frame.errors.iter().map(|e| e.source.clone()).collect();
            let written = slot.errors.write(device, queue, frame.errors);
            result = result.and(written);
        }

        slot.faces.clear();
        if !frame.faded
            && let Some(gpu) = &slot.mesh
        {
            let tinted = [
                (frame.hovered_faces, Tint::Hovered),
                (frame.selected_faces, Tint::Selected),
                (frame.second_faces, Tint::Second),
            ];
            let faces = (tinted.into_iter())
                .flat_map(|(faces, tint)| faces.iter().map(move |&face| (face, tint)));
            for (face, tint) in faces {
                if let Some((indices, part)) = gpu.face(frame.mesh, face) {
                    slot.faces.push(FaceDraw {
                        indices,
                        step: self.alphas.step(opacity.get(part).copied()),
                        tint,
                    });
                }
            }
        }

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
            slot.errors.bounds,
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
        let selected_tint = if (0.0..=1.0).contains(&colors.selected_tint) {
            colors.selected_tint
        } else {
            0.6
        };
        let selected_edge_shade = if (-1.0..=1.0).contains(&colors.selected_edge_shade) {
            colors.selected_edge_shade
        } else {
            0.0
        };
        let hidden_alpha = if (0.0..=1.0).contains(&colors.hidden_edge_alpha) {
            colors.hidden_edge_alpha
        } else {
            0.0
        };
        let second = linear(colors.second);
        let error = linear(colors.error);
        let error_halo = colors.error_halo.linear();
        let halo_alpha = if (0.0..=1.0).contains(&error_halo[3]) {
            error_halo[3]
        } else {
            0.3
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
            edge: with_alpha(colors.edge, hidden_alpha),
            grid: linear(colors.grid),
            axes: [0, 1, 2].map(|i| with_alpha(colors.axes[i], error[i])),
            origin_outline: with_alpha(colors.origin_outline, error_halo[0]),
            sketch: with_alpha(colors.sketch, error_halo[1]),
            pivot: frame
                .pivot
                .filter(|pivot| pivot.at.is_finite() && (0.0..=1.0).contains(&pivot.opacity))
                .map_or([0.0; 4], |pivot| pivot.at.extend(pivot.opacity).to_array()),
            pivot_color: with_alpha(colors.pivot, error_halo[2]),
            grid_origin: grid.origin().extend(0.0).to_array(),
            grid_x: grid.x().extend(axis_index(grid.x())).to_array(),
            grid_y: grid.y().extend(axis_index(grid.y())).to_array(),
            sketch_origin: sketch_plane.origin().extend(second[0]).to_array(),
            sketch_x: sketch_plane.x().extend(second[1]).to_array(),
            sketch_y: sketch_plane.y().extend(second[2]).to_array(),
            hover_face: with_alpha(colors.hover_face, selected_tint),
            hover_outline: with_alpha(colors.hover_outline, halo_alpha),
            selected: with_alpha(colors.selected, selected_edge_shade),
        };
        queue.write_buffer(&slot.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let size = frame.target_size.map(|s| s.max(1));
        if slot.depth.as_ref().map(|d| d.size) != Some(size) {
            slot.depth = Some(create_depth(device, self.depth_format, size));
        }
        if slot.errors.any() && slot.halo.as_ref().map(|h| h.size) != Some(size) {
            slot.halo = Some(create_halo(device, &self.errors.halo_layout, size));
        }
        result
    }

    /// Records draw commands for the frame last prepared into `slot` into
    /// `encoder`, compositing over `target`, within `clip`. The order of
    /// the draws, and why, is in `agents/viewport.md`.
    pub fn render(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
    ) {
        self.record(slot, encoder, target, clip, true);
    }

    /// Records the frame last prepared into `slot` as [`Self::render`]
    /// does, with the background, the grid, the finished sketches and the
    /// origin and pivot markers only if `backdrop`: without, the model
    /// alone, over what `target` holds (a preview's, see `preview.rs`).
    pub(crate) fn record(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: ClipRect,
        backdrop: bool,
    ) {
        let Some(depth) = &slot.depth else { return };
        if clip.width == 0 || clip.height == 0 {
            return;
        }
        // The errors are drawn in passes of their own after the scene's,
        // depth tested against the model, so its depth is kept for them.
        let halo = slot.halo.as_ref().filter(|_| slot.errors.any());
        let depth_store = if halo.is_some() {
            wgpu::StoreOp::Store
        } else {
            wgpu::StoreOp::Discard
        };
        let mut pass = self.begin(
            slot,
            encoder,
            ("varde scene", target, wgpu::LoadOp::Load),
            depth,
            (wgpu::LoadOp::Clear(1.0), depth_store),
            clip,
        );
        self.draw_scene(&mut pass, slot, backdrop);
        drop(pass);
        if let Some(halo) = halo {
            self.draw_errors(slot, encoder, target, depth, halo, clip);
        }
    }

    /// Begins a pass of `slot`'s frame drawing into the colour target
    /// `target` (its label, view and load) and `depth`, its depth loaded
    /// and stored as `depth_ops` say (the stencil cleared or loaded with it,
    /// never kept), within `clip`, with the uniforms bound and parts drawn
    /// opaque.
    fn begin<'p>(
        &self,
        slot: &Slot,
        encoder: &'p mut wgpu::CommandEncoder,
        (label, target, load): (&str, &wgpu::TextureView, wgpu::LoadOp<wgpu::Color>),
        depth: &DepthTarget,
        (depth_load, depth_store): (wgpu::LoadOp<f32>, wgpu::StoreOp),
        clip: ClipRect,
    ) -> wgpu::RenderPass<'p> {
        let stencil_load = match depth_load {
            wgpu::LoadOp::Clear(_) => wgpu::LoadOp::Clear(0),
            _ => wgpu::LoadOp::Load,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: depth_store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: stencil_load,
                    store: wgpu::StoreOp::Discard,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let vp = slot.viewport;
        pass.set_viewport(vp.x, vp.y, vp.width, vp.height, 0.0, 1.0);
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_bind_group(0, &slot.bind_group, &[]);
        self.alphas.set(&mut pass, self.alphas.opaque);
        pass
    }

    /// Records drawing `slot`'s errors over what's in `target`, with
    /// `depth` holding the model's depth: their halo's coverage into
    /// `halo`, seen and hidden, then composited over the target once, then
    /// the errors themselves, a kind at a time, hidden then seen.
    fn draw_errors(
        &self,
        slot: &Slot,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &DepthTarget,
        halo: &HaloTarget,
        clip: ClipRect,
    ) {
        let steps = [self.alphas.opaque, self.alphas.step(Some(ERROR_HIDDEN))];
        let errors = &slot.errors;
        let load = (wgpu::LoadOp::Load, wgpu::StoreOp::Store);
        let mut pass = self.begin(
            slot,
            encoder,
            (
                "varde error halo",
                &halo.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ),
            depth,
            load,
            clip,
        );
        self.draw_error_layers(&mut pass, &self.errors.halo, steps, errors);
        drop(pass);

        let load = (wgpu::LoadOp::Load, wgpu::StoreOp::Discard);
        let target = ("varde errors", target, wgpu::LoadOp::Load);
        let mut pass = self.begin(slot, encoder, target, depth, load, clip);
        pass.set_pipeline(&self.errors.composite);
        pass.set_bind_group(2, &halo.group, &[]);
        pass.draw(0..3, 0..1);
        // Hidden first, so what shows of each kind goes over it.
        let [seen, hidden] = &self.errors.core;
        let [opaque, dimmed] = steps;
        let layers = [hidden, seen];
        self.draw_error_layers(&mut pass, layers, [dimmed, opaque], errors);
    }

    /// Records drawing `errors` with `layers` at the alphas' steps `steps`,
    /// their faces with each, then their lines, then their points.
    fn draw_error_layers<'l>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        layers: impl IntoIterator<Item = &'l ErrorLayer> + Clone,
        steps: [u32; 2],
        errors: &ErrorBuffers,
    ) {
        let with_steps = || layers.clone().into_iter().zip(steps);
        if let (Some(positions), Some(normals)) = (errors.positions.drawn(), errors.normals.drawn())
        {
            pass.set_vertex_buffer(0, positions);
            pass.set_vertex_buffer(1, normals);
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                pass.set_pipeline(&layer.faces);
                pass.draw(0..errors.positions.count, 0..1);
            }
        }
        if let Some(edges) = errors.edges.held() {
            // Between the stream's two ends.
            let points = 1..errors.edges.count.saturating_sub(1);
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                draw_stream(pass, &layer.lines, edges, points.clone());
            }
        }
        if let Some(points) = errors.points.drawn() {
            pass.set_vertex_buffer(0, points);
            for (layer, step) in with_steps() {
                self.alphas.set(pass, step);
                pass.set_pipeline(&layer.points);
                pass.draw(0..POINT_VERTICES, 0..errors.points.count);
            }
        }
    }

    /// Records drawing `slot`'s scene into `pass`: everything but the
    /// errors, the backdrop only if `backdrop` (see [`Self::record`]).
    fn draw_scene(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot, backdrop: bool) {
        if backdrop {
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
        }

        let mesh = slot.mesh.as_ref();
        let draws = &slot.draws;
        if let Some(mesh) = mesh {
            bind_faces(pass, mesh);
            if slot.faded {
                let all = 0..mesh.parts.len();
                pass.set_pipeline(&self.mesh_depth);
                draw_faces(pass, mesh, all.clone());
                pass.set_pipeline(&self.mesh_faded);
                draw_faces(pass, mesh, all.clone());
                draw_edges(pass, &self.edges, mesh, all);
            } else {
                pass.set_pipeline(&self.mesh);
                for run in &draws.opaque {
                    draw_faces(pass, mesh, run.clone());
                }
                // Before anything else writes depth where they are.
                self.draw_picked_faces(pass, &slot.faces, false);
                for run in &draws.opaque {
                    draw_edges(pass, &self.edges, mesh, run.clone());
                }
            }
        }

        // Drawn after the model so they blend over the background and are
        // occluded by geometry. The sketches go over the grid, since they
        // often lie on it.
        if backdrop {
            pass.set_pipeline(&self.grid);
            pass.draw(0..3, 0..1);
        }

        if let Some(lines) = slot.lines.as_ref().filter(|_| backdrop) {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, lines.segments.slice(..));
            pass.draw(0..LINE_VERTICES, 0..lines.segment_count);
        }

        // Only the opaque parts have written depth so far, so the hidden
        // edges here are those they hide; what glass hides is dashed below.
        // Then the glass's visible edges, under the faces drawn next.
        if !slot.faded
            && let Some(mesh) = mesh
        {
            if slot.hidden_edges {
                for run in &draws.opaque {
                    draw_edges(pass, &self.hidden_edges, mesh, run.clone());
                }
                self.draw_glass_edges(pass, &self.hidden_edges, mesh, draws);
            }
            self.draw_glass_edges(pass, &self.edges, mesh, draws);
        }

        // The extrude's layers under the glass in front of them, if there
        // is any, and so under the origin marker too.
        let glass = mesh.is_some() && !draws.transparent.is_empty();
        let layers = [&slot.sketch_base, &slot.sketch_live];
        if slot.sketching && slot.sketch_depth && glass {
            for layer in layers {
                self.sketch_depth_tested.draw(pass, layer);
            }
        }

        // The glass: the hover and selection under it, to show through
        // dimmed; its faces far to near; its depth a part at a time, each
        // followed by the edges it hides, dashed; then its picked faces and
        // its edges again, undimmed on the glass they lie on.
        if let Some(mesh) = mesh.filter(|_| glass) {
            if !slot.faded {
                self.draw_highlights(pass, slot);
            }
            bind_faces(pass, mesh);
            for &(part, step) in &draws.transparent {
                self.alphas.set(pass, step);
                for pipeline in [&self.glass_back, &self.glass_front] {
                    pass.set_pipeline(pipeline);
                    draw_faces(pass, mesh, part..part + 1);
                }
            }
            for (i, &(part, step)) in draws.transparent.iter().enumerate() {
                // Never 0, what the stencil is cleared to; repeating only
                // past 255 parts less than opaque.
                pass.set_stencil_reference(i as u32 % 255 + 1);
                bind_faces(pass, mesh);
                pass.set_pipeline(&self.glass_depth);
                draw_faces(pass, mesh, part..part + 1);
                if slot.hidden_edges {
                    self.draw_hidden_by_glass(pass, mesh, draws, step);
                }
            }
            bind_faces(pass, mesh);
            self.draw_picked_faces(pass, &slot.faces, true);
            self.draw_glass_edges(pass, &self.edges, mesh, draws);
        }

        // The hover and the selection over everything of the model.
        if !slot.faded && mesh.is_some() {
            self.draw_highlights(pass, slot);
        }

        if backdrop {
            pass.set_pipeline(&self.origin);
            pass.draw(0..ORIGIN_VERTICES, 0..MARKERS);
        }

        // The sketch being edited, over everything, the origin marker
        // included, since its points often lie on it; or depth tested.
        if slot.sketching && !(slot.sketch_depth && glass) {
            let pipelines = if slot.sketch_depth {
                &self.sketch_depth_tested
            } else {
                &self.sketch_on_top
            };
            for layer in layers {
                pipelines.draw(pass, layer);
            }
        }
    }

    /// Records drawing `faces` again over themselves, hovered or selected,
    /// those of parts less than opaque if `transparent`, else the opaque
    /// ones', with the mesh's buffers bound ([`bind_faces`]).
    fn draw_picked_faces(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        faces: &[FaceDraw],
        transparent: bool,
    ) {
        for face in faces {
            if (face.step < self.alphas.opaque) != transparent || face.indices.is_empty() {
                continue;
            }
            let pipeline = match face.tint {
                Tint::Hovered => &self.hover_face,
                Tint::Selected => &self.selected_face,
                Tint::Second => &self.second_face,
            };
            pass.set_pipeline(pipeline);
            self.alphas.set(pass, face.step);
            pass.draw_indexed(face.indices.clone(), 0, 0..1);
        }
    }

    /// Records drawing `slot`'s hovered edges within their outline,
    /// selected edges, and hovered and selected vertices.
    fn draw_highlights(&self, pass: &mut wgpu::RenderPass<'_>, slot: &Slot) {
        let highlights = &slot.highlights;
        self.alphas.set(pass, self.alphas.opaque);
        if let Some(edges) = highlights.edges.held() {
            let outlined = highlights.outlined.clone();
            draw_stream(pass, &self.outline, edges, outlined.clone());
            draw_stream(pass, &self.hovered_edges, edges, outlined);
            let selected = highlights.selected.clone();
            draw_stream(pass, &self.selected_outline, edges, selected.clone());
            draw_stream(pass, &self.selected_edges, edges, selected);
            let second = highlights.second.clone();
            draw_stream(pass, &self.second_outline, edges, second.clone());
            draw_stream(pass, &self.second_edges, edges, second);
        }
        if let Some(vertices) = highlights.vertices.drawn() {
            pass.set_pipeline(&self.vertices);
            pass.set_vertex_buffer(0, vertices);
            pass.draw(0..POINT_VERTICES, 0..highlights.vertices.count);
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

    /// Records drawing, dashed, the edges of every part of `mesh` hidden by
    /// the glass whose depth and stencil reference were just written, at
    /// its alpha's step `step` times their own part's.
    fn draw_hidden_by_glass(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        mesh: &GpuMesh,
        draws: &PartDraws,
        step: u32,
    ) {
        let opaque = self.alphas.opaque;
        let runs = draws.opaque.iter().map(|run| (run.clone(), step));
        let glass = draws
            .transparent
            .iter()
            .map(|&(part, own)| (part..part + 1, product_step(step, own, opaque)));
        for (parts, step) in runs.chain(glass) {
            if step > 0 {
                self.alphas.set(pass, step);
                draw_edges(pass, &self.hidden_by_glass, mesh, parts);
            }
        }
    }
}

/// The step of a table of alphas whose last step, `opaque`, is opaque,
/// nearest the product of steps `a`'s and `b`'s alphas.
fn product_step(a: u32, b: u32, opaque: u32) -> u32 {
    if opaque == 0 {
        return 0;
    }
    let product = u64::from(a) * u64::from(b) + u64::from(opaque / 2);
    // At most `opaque` for steps within the table.
    u32::try_from(product / u64::from(opaque)).map_or(opaque, |step| step.min(opaque))
}

/// Binds `mesh`'s buffers for drawing its faces.
fn bind_faces(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh) {
    pass.set_vertex_buffer(0, mesh.positions.slice(..));
    pass.set_vertex_buffer(1, mesh.normals.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
}

/// The range from `of` the first of `parts` to `of` the last, which
/// follow one another; empty if there are none.
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
/// another, with `pipeline`: their points follow one another in the
/// stream, so [`draw_stream`] draws them at once; and their wires likewise
/// in a wireframe.
fn draw_edges(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    mesh: &GpuMesh,
    parts: Range<usize>,
) {
    let Some(edges) = &mesh.edges else { return };
    let points = span(mesh, parts.clone(), |part| &part.points);
    draw_stream(pass, pipeline, edges, points);
    if mesh.wireframe {
        draw_stream(pass, pipeline, edges, span(mesh, parts, |part| &part.wires));
    }
}

/// Records drawing the polylines of the [`EdgePoint`] stream in `edges`
/// whose points are `points`, with `pipeline`, an instance per segment.
/// The points either side must be there, and of other polylines or of
/// none, so no segment joins them.
fn draw_stream(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    edges: &wgpu::Buffer,
    points: Range<u32>,
) {
    // A segment needs two points; and there's a point before them.
    if points.start == 0 || points.end.saturating_sub(points.start) < 2 {
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

/// The pipelines drawing the errors' geometry ([`Frame::errors`]), each
/// kind where it shows (`[0]`, depth `LessEqual`) and where the model hides
/// it (`[1]`, `Greater`, drawn at [`ERROR_HIDDEN`]).
struct ErrorPipelines {
    /// The halo, as coverage into a [`HALO_FORMAT`] target of its own,
    /// blended by [`MAX_BLENDING`].
    halo: [ErrorLayer; 2],
    /// The halo's coverage, once over the frame in
    /// [`Colors::error_halo`].
    composite: wgpu::RenderPipeline,
    /// The geometry itself, over that.
    core: [ErrorLayer; 2],
    /// Group 2 of the composite: the halo's coverage.
    halo_layout: wgpu::BindGroupLayout,
}

/// The pipelines drawing one way the errors' patches, curves and points.
struct ErrorLayer {
    faces: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    points: wgpu::RenderPipeline,
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

/// Which world axis `axis`, a unit vector, lies along, 0 to 2, or 3 if
/// none, for its axis line's colour: of [`Colors::axes`], or the grid's.
fn axis_index(axis: Vec3) -> f32 {
    let along = axis.abs();
    (0..3)
        .find(|&i| along[i] > 1.0 - 1e-6)
        .map_or(3.0, |i| i as f32)
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
    let (edge_points, part_points) =
        if mesh.edge_vertices().is_empty() && mesh.wire_vertices().is_empty() {
            (None, Vec::new())
        } else {
            let (points, parts) = edge_stream(mesh);
            (Some(points), parts)
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
    // The kernel's limits keep these well within `u32`.
    let to_u32 = |range: Range<usize>| {
        let at = |n| u32::try_from(n).expect("kernel bounds indices");
        at(range.start)..at(range.end)
    };
    let part =
        |faces: Range<usize>, indices: Range<usize>, [points, wires]: [Range<u32>; 2]| GpuPart {
            bounds: bounds_of(mesh.positions(), &mesh.indices()[indices.clone()]),
            faces: to_u32(faces),
            indices: to_u32(indices),
            points,
            wires,
        };
    // With no edges or wires, none of the parts has points.
    let points = |i: usize| part_points.get(i).cloned().unwrap_or([1..1, 1..1]);
    let mut parts: Vec<GpuPart> = (mesh.parts().enumerate())
        .map(|(i, part_of)| part(part_of.faces, part_of.indices, points(i)))
        .collect();
    if parts.is_empty() {
        let (faces, indices) = (0..mesh.face_count(), 0..mesh.indices().len());
        parts.push(part(faces, indices, [1..1, 1..1]));
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
        wireframe: false,
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

/// The [`EdgePoint`] stream of `mesh`'s feature edges, then its wires,
/// and where each part's edges' and wires' points are in it. Creases and
/// wires are marked [`CREASE`]; a wire is numbered after every edge.
fn edge_stream(mesh: &RenderMesh) -> (Vec<EdgePoint>, Vec<[Range<u32>; 2]>) {
    let points = (mesh.edge_vertices().len()).saturating_add(mesh.wire_vertices().len());
    let mut stream = EdgeStream::with_capacity(points);
    // The kernel bounds the edges and wires together well within `u32`,
    // below `CREASE`.
    let to_u32 = |n: usize| u32::try_from(n).expect("kernel bounds edges");
    let mut edge_points = Vec::with_capacity(mesh.part_ends().len());
    let mut polylines = mesh.polylines().zip(mesh.edge_faces());
    for part in mesh.parts() {
        let start = stream.len();
        for (edge, (polyline, [a, b])) in part.edges.zip(polylines.by_ref()) {
            let id = if a == b {
                to_u32(edge) | CREASE
            } else {
                to_u32(edge)
            };
            stream.push(id, mesh.positions(), polyline);
        }
        edge_points.push(start..stream.len());
    }
    let edges = to_u32(mesh.edge_count());
    let mut wires = mesh.wires();
    let parts = (mesh.parts().zip(edge_points))
        .map(|(part, edge_points)| {
            let start = stream.len();
            for (wire, polyline) in part.wires.zip(wires.by_ref()) {
                stream.push((edges + to_u32(wire)) | CREASE, mesh.positions(), polyline);
            }
            [edge_points, start..stream.len()]
        })
        .collect();
    (stream.finish(), parts)
}

/// An [`EdgePoint`] stream being built: polylines one after another, from
/// a point of no edge, which ends it too.
pub(crate) struct EdgeStream {
    points: Vec<EdgePoint>,
    /// A polyline's points, repeats in a row left out: a segment of no
    /// length between two others would keep them from joining.
    kept: Vec<[f32; 3]>,
}

impl EdgeStream {
    /// A stream with room for `points` points of polylines.
    pub(crate) fn with_capacity(points: usize) -> EdgeStream {
        let mut stream = EdgeStream {
            points: Vec::with_capacity(points.saturating_add(2)),
            kept: Vec::new(),
        };
        stream.separate();
        stream
    }

    /// Appends the polyline through `polyline`'s vertices of `positions`
    /// as edge `edge`, below [`NEIGHBOUR_ONLY`].
    pub(crate) fn push(&mut self, edge: u32, positions: &[[f32; 3]], polyline: &[u32]) {
        self.push_points(
            edge,
            polyline.iter().map(|&vertex| positions[vertex as usize]),
        );
    }

    /// Appends the polyline through `polyline` as edge `edge`, likewise.
    pub(crate) fn push_points(&mut self, edge: u32, polyline: impl IntoIterator<Item = [f32; 3]>) {
        let (points, kept) = (&mut self.points, &mut self.kept);
        kept.clear();
        for position in polyline {
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
    }

    /// Appends a point of no edge, which no segment joins.
    pub(crate) fn separate(&mut self) {
        self.points.push(EdgePoint {
            position: [0.0; 3],
            along: 0.0,
            edge: NO_EDGE,
        });
    }

    /// How many points it has.
    pub(crate) fn len(&self) -> u32 {
        // The kernel's limits keep the stream well within `u32`.
        u32::try_from(self.points.len()).expect("kernel bounds edges")
    }

    /// The stream, ended by a point of no edge.
    pub(crate) fn finish(mut self) -> Vec<EdgePoint> {
        self.separate();
        self.points
    }
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
        let too_large = |bytes, limit| PrepareError::SketchTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.lines)
            .write(d, q, "varde sketch lines", &layer.lines, too_large)
            .and_then(|()| {
                (self.points).write(d, q, "varde sketch points", &layer.points, too_large)
            })
            .and_then(|()| (self.fills).write(d, q, "varde sketch fills", &layer.fills, too_large));
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
    /// don't fit. Past the device's buffer size, holds nothing and fails
    /// with what `too_large` makes of the bytes needed and that size.
    fn write<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        items: &[T],
        too_large: fn(u64, u64) -> PrepareError,
    ) -> Result<(), PrepareError> {
        let bytes: &[u8] = bytemuck::cast_slice(items);
        let size = bytes.len() as u64;
        let limit = device.limits().max_buffer_size;
        let count = u32::try_from(items.len()).ok().filter(|_| size <= limit);
        let Some(count) = count else {
            self.count = 0;
            return Err(too_large(size, limit));
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
        Some(self.held()?.slice(..))
    }

    /// The buffer, if it holds anything.
    fn held(&self) -> Option<&wgpu::Buffer> {
        self.buffer.as_ref().filter(|_| self.count > 0)
    }
}

/// [`Frame::highlights`] on the GPU: the edges as an [`EdgePoint`] stream,
/// written again as they change, which of its points are of the outlined
/// edges and which of the selected ones, and the vertices.
#[derive(Default)]
struct HighlightBuffers {
    edges: Instances,
    outlined: Range<u32>,
    selected: Range<u32>,
    second: Range<u32>,
    vertices: Instances,
}

impl HighlightBuffers {
    /// Replaces what the buffers hold with `highlights` in `mesh`. On
    /// failing, holds nothing.
    fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &RenderMesh,
        highlights: &Highlights,
    ) -> Result<(), PrepareError> {
        let built = highlights.build(mesh);
        let too_large = |bytes, limit| PrepareError::HighlightsTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.edges)
            .write(d, q, "varde highlighted edges", &built.edges, too_large)
            .and_then(|()| {
                (self.vertices).write(d, q, "varde vertices", &built.vertices, too_large)
            });
        *self = match written {
            Ok(()) => HighlightBuffers {
                outlined: built.outlined,
                selected: built.selected,
                second: built.second,
                ..std::mem::take(self)
            },
            Err(_) => HighlightBuffers::default(),
        };
        written
    }
}

/// [`Frame::errors`] on the GPU, all of them together: the patches'
/// triangles a corner at a time (positions and normals), the curves as an
/// [`EdgePoint`] stream, a polyline each, and the points.
#[derive(Default)]
struct ErrorBuffers {
    positions: Instances,
    normals: Instances,
    edges: Instances,
    points: Instances,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
}

impl ErrorBuffers {
    /// Replaces what the buffers hold with `errors`. On failing, holds
    /// nothing.
    fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        errors: &[ErrorParts<'_>],
    ) -> Result<(), PrepareError> {
        let built = BuiltErrors::new(errors);
        let too_large = |bytes, limit| PrepareError::ErrorsTooLarge { bytes, limit };
        let (d, q) = (device, queue);
        let written = (self.positions)
            .write(d, q, "varde error faces", &built.positions, too_large)
            .and_then(|()| {
                (self.normals).write(d, q, "varde error normals", &built.normals, too_large)
            })
            .and_then(|()| (self.edges).write(d, q, "varde error lines", &built.edges, too_large))
            .and_then(|()| {
                (self.points).write(d, q, "varde error points", &built.points, too_large)
            });
        *self = match written {
            Ok(()) => ErrorBuffers {
                bounds: built.bounds,
                ..std::mem::take(self)
            },
            Err(_) => ErrorBuffers::default(),
        };
        written
    }

    /// Whether there's anything to draw.
    fn any(&self) -> bool {
        [&self.positions, &self.edges, &self.points]
            .iter()
            .any(|buffer| buffer.held().is_some())
    }
}

/// [`Frame::errors`] as the renderer draws them, see [`ErrorBuffers`].
struct BuiltErrors {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    edges: Vec<EdgePoint>,
    points: Vec<VertexInstance>,
    bounds: Option<Aabb>,
}

impl BuiltErrors {
    fn new(errors: &[ErrorParts<'_>]) -> BuiltErrors {
        let mut built = BuiltErrors {
            positions: Vec::new(),
            normals: Vec::new(),
            edges: Vec::new(),
            points: Vec::new(),
            bounds: None,
        };
        let mut stream = EdgeStream::with_capacity(0);
        // Each polyline a number of its own, so none joins the next; past
        // `CREASE` they start again, which only the next must differ from.
        let mut polyline = 0u32;
        for error in errors {
            let mesh = error.mesh;
            let (positions, normals) = (mesh.positions(), mesh.normals());
            // A corner at a time: the kernel's meshes index their own
            // positions and normals, in whole triangles.
            for &i in mesh.indices() {
                built.positions.push(positions[i as usize]);
                built.normals.push(normals[i as usize]);
            }
            for line in error.lines.polylines() {
                stream.push_points(polyline, line.iter().copied());
                polyline = (polyline + 1) % CREASE;
            }
            let mut boxes: Vec<Aabb> = [mesh.bounds(), error.lines.bounds()]
                .into_iter()
                .flatten()
                .collect();
            let shown =
                |point: &&[f32; 3]| (point.iter()).all(|x| x.abs() <= RenderLines::MAX_POSITION);
            for &position in error.points.iter().filter(shown) {
                built.points.push(VertexInstance { position, flags: 0 });
                let at = Vec3::from(position);
                boxes.push(Aabb { min: at, max: at });
            }
            built.bounds = (boxes.into_iter().chain(built.bounds)).reduce(|a, b| Aabb {
                min: a.min.min(b.min),
                max: a.max.max(b.max),
            });
        }
        built.edges = stream.finish();
        built
    }
}

/// The target the errors' halo is drawn into, see [`HALO_FORMAT`].
struct HaloTarget {
    view: wgpu::TextureView,
    /// Group 2 of the composite, reading it.
    group: wgpu::BindGroup,
    size: [u32; 2],
}

fn create_halo(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    size: [u32; 2],
) -> HaloTarget {
    let [width, height] = size;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("varde error halo"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HALO_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("varde error halo"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    HaloTarget { view, group, size }
}

fn create_depth(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    [width, height]: [u32; 2],
) -> DepthTarget {
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
        format,
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
