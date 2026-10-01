use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use varde_kernel::{Aabb, RenderLines, RenderMesh};
use wgpu::util::DeviceExt;

use crate::Camera;
use crate::scene::{self, GRID_FADE_HEIGHTS, GridPlane};
use crate::sketch::{FillVertex, LineInstance, PointInstance, SketchLayer, SketchScene};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// One quad for the origin marker. See `vs_origin`.
const ORIGIN_VERTICES: u32 = 6;

/// A quad per segment of a line, two triangles. See `line_vertex`.
const LINE_VERTICES: u32 = 6;

/// A quad per point of the sketch being edited. See `vs_point`.
const POINT_VERTICES: u32 = 6;

/// How wide [`Frame::sketches`] are drawn, in logical pixels.
pub const LINE_WIDTH: f32 = 1.5;

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
    /// Finished sketches' curves, drawn with the model as lines
    /// [`LINE_WIDTH`] wide, hidden by what's in front of them. Only
    /// re-uploaded when it's another `Arc` than the last one prepared.
    pub sketches: &'a Arc<RenderLines>,
    /// The plane the grid is drawn on.
    pub grid: GridPlane,
    /// Whether the model is drawn faded, as it is behind a sketch being
    /// edited: see [`Colors::faded_alpha`].
    pub faded: bool,
    /// The sketch being edited, or the extrude being set up, if one is:
    /// drawn over everything else, or hidden by the model in front of it
    /// ([`SketchScene::depth_tested`]).
    pub sketch: Option<SketchScene<'a>>,
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
    /// Feature edges, drawn over the faces.
    pub edge: Srgb,
    /// Grid lines.
    pub grid: Srgb,
    /// The X, Y and Z axes, for the grid's axis lines.
    pub axes: [Srgb; 3],
    /// The rims of the origin marker's ring and dot.
    pub origin_outline: Srgb,
    /// Finished sketches' curves.
    pub sketch: Srgb,
    /// How opaque the model's faces and edges are when [`Frame::faded`],
    /// from 0 to 1. Only its nearest faces are drawn, and they still hide
    /// what's behind them from the grid and the sketches.
    pub faded_alpha: f32,
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
}

// WGSL lays out uniform structs in 16-byte steps.
const _: () = assert!(size_of::<Uniforms>().is_multiple_of(16));

/// The largest buffer the renderer relies on, in bytes: WebGPU's default
/// `maxBufferSize`, which the devices iced asks for have natively and on
/// the web. A larger buffer is a validation error, and wgpu panics on those.
const MAX_BUFFER_BYTES: usize = 256 << 20;

// Every mesh the kernel allows fits, one buffer per part: positions and
// normals are uploaded as they are, and indices and edges too.
const _: () = assert!(size_of::<[f32; 3]>() * RenderMesh::MAX_VERTICES <= MAX_BUFFER_BYTES);
const _: () = assert!(size_of::<u32>() * RenderMesh::MAX_INDICES <= MAX_BUFFER_BYTES);
const _: () = assert!(size_of::<[u32; 2]>() * RenderMesh::MAX_EDGES <= MAX_BUFFER_BYTES);
// Lines are uploaded a segment of two points each, fewer than points.
const _: () = assert!(size_of::<Segment>() * RenderLines::MAX_POINTS <= MAX_BUFFER_BYTES);

/// A segment of a line as the GPU takes it: its two ends.
type Segment = [[f32; 3]; 2];

struct GpuMesh {
    positions: wgpu::Buffer,
    normals: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    edges: wgpu::Buffer,
    edge_count: u32,
    /// To fit the depth range to.
    bounds: Option<Aabb>,
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
    edges: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    origin: wgpu::RenderPipeline,
    /// The sketch being edited: its fills, lines and points, drawn over
    /// everything, and the same hidden by the model in front of them
    /// ([`SketchScene::depth_tested`]).
    sketch_on_top: SketchPipelines,
    sketch_depth_tested: SketchPipelines,
    bind_group_layout: wgpu::BindGroupLayout,
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
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("varde scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/scene.wgsl").into()),
        });

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

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde scene"),
            bind_group_layouts: &[&bind_group_layout],
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
                ("ENCODE_SRGB", if format.is_srgb() { 0.0 } else { 1.0 }),
                ("SKETCH_DEPTH", if sketch_depth { 1.0 } else { 0.0 }),
            ]
        };

        let pipeline = |pass: Pass<'_>| {
            let constants = constants(pass.sketch_depth);
            let compilation_options = wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(pass.label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(pass.vs),
                    buffers: pass.buffers,
                    compilation_options: compilation_options.clone(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
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
        // own. Edges only need positions. Lines take a segment per
        // instance, its ends in slots 0 and 1.
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
        let mesh = Pass {
            label: "varde mesh",
            vs: "vs_mesh",
            fs: "fs_mesh",
            buffers: &[positions.clone(), normals],
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
            mesh: pipeline(mesh),
            edges: pipeline(Pass {
                buffers: &[positions],
                topology: wgpu::PrimitiveTopology::LineList,
                ..Pass::overlay("varde edges", "vs_edge", "fs_edge")
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
            sketching: false,
            sketch_depth: false,
            depth: None,
            viewport: Viewport::default(),
            faded: false,
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
        let faded = |color| {
            let [r, g, b, _] = linear(color);
            [r, g, b, alpha]
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
            grid_origin: grid.origin().extend(0.0).to_array(),
            grid_x: grid.x().extend(0.0).to_array(),
            grid_y: grid.y().extend(0.0).to_array(),
            grid_axes: [grid.x(), grid.y()].map(|axis| linear(axis_color(axis, &colors))),
            sketch_origin: sketch_plane.origin().extend(0.0).to_array(),
            sketch_x: sketch_plane.x().extend(0.0).to_array(),
            sketch_y: sketch_plane.y().extend(0.0).to_array(),
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

        pass.set_pipeline(&self.background);
        pass.draw(0..3, 0..1);

        if let Some(mesh) = &slot.mesh {
            pass.set_vertex_buffer(0, mesh.positions.slice(..));
            pass.set_vertex_buffer(1, mesh.normals.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            if slot.faded {
                pass.set_pipeline(&self.mesh_depth);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                pass.set_pipeline(&self.mesh_faded);
            } else {
                pass.set_pipeline(&self.mesh);
            }
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);

            if mesh.edge_count > 0 {
                pass.set_pipeline(&self.edges);
                pass.set_index_buffer(mesh.edges.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.edge_count, 0, 0..1);
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

        pass.set_pipeline(&self.origin);
        pass.draw(0..ORIGIN_VERTICES, 0..1);

        // The sketch being edited, over everything, the origin marker
        // included, since its points often lie on it; or depth tested.
        if slot.sketching {
            let pipelines = if slot.sketch_depth {
                &self.sketch_depth_tested
            } else {
                &self.sketch_on_top
            };
            for layer in [&slot.sketch_base, &slot.sketch_live] {
                pipelines.draw(&mut pass, layer);
            }
        }
    }
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
    let largest = [
        bytes(mesh.positions().len(), size_of::<[f32; 3]>()),
        bytes(mesh.indices().len(), size_of::<u32>()),
        bytes(mesh.edges().len(), size_of::<[u32; 2]>()),
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
    // The kernel's limits keep these well within `u32`.
    let index_count = u32::try_from(mesh.indices().len()).expect("kernel bounds indices");
    let edge_count = mesh
        .edges()
        .len()
        .checked_mul(2)
        .and_then(|n| u32::try_from(n).ok())
        .expect("kernel bounds edges");

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
        index_count,
        edges: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("varde mesh edges"),
            contents: bytemuck::cast_slice(mesh.edges()),
            usage: wgpu::BufferUsages::INDEX,
        }),
        edge_count,
        bounds: mesh.bounds(),
    }))
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
