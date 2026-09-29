use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use varde_kernel::{Aabb, RenderMesh};
use wgpu::util::DeviceExt;

use crate::Camera;
use crate::scene::{self, GRID_FADE_HEIGHTS};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Three axis quads and the origin dot, six vertices each. See `vs_origin`.
const ORIGIN_VERTICES: u32 = 4 * 6;

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
    /// The X, Y and Z axes, on the grid and the origin marker.
    pub axes: [Srgb; 3],
    /// Outline of the origin marker's dot.
    pub origin_outline: Srgb,
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
    /// xyz: orbit target, w: unused. Not `target`, which WGSL reserves.
    focus: [f32; 4],
    /// Unit vector towards the camera.
    backward: [f32; 4],
    /// xy: viewport size in physical pixels, z: physical pixels per logical
    /// pixel.
    viewport: [f32; 4],
    /// [`Colors`], converted to linear with w = 1.
    background_top: [f32; 4],
    background_bottom: [f32; 4],
    model: [f32; 4],
    edge: [f32; 4],
    grid: [f32; 4],
    axes: [[f32; 4]; 3],
    origin_outline: [f32; 4],
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

struct DepthTarget {
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// What differs between the renderer's pipelines. They all share the scene
/// shader, the uniform layout and a depth test.
struct Pass<'a> {
    label: &'a str,
    /// Vertex and fragment entry points in the scene shader.
    vs: &'a str,
    fs: &'a str,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
    depth_write: bool,
    blend: wgpu::BlendState,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
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
            blend: wgpu::BlendState::ALPHA_BLENDING,
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
        }
    }
}

/// Why [`Renderer::prepare`] couldn't upload a frame's mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// A buffer of the mesh would take `bytes`, more than the device's
    /// `max_buffer_size` of `limit`.
    MeshTooLarge { bytes: u64, limit: u64 },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrepareError::MeshTooLarge { bytes, limit } => write!(
                f,
                "the mesh needs a {bytes}-byte buffer, but the device allows at most {limit} bytes"
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
    edges: wgpu::RenderPipeline,
    origin: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

/// One scene's state on the GPU, from [`Renderer::prepare`] to
/// [`Renderer::render`]: its uniforms, mesh and depth buffer.
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
    depth: Option<DepthTarget>,
    viewport: Viewport,
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
        let constants = [
            ("GRID_FADE_HEIGHTS", f64::from(GRID_FADE_HEIGHTS)),
            ("ENCODE_SRGB", if format.is_srgb() { 0.0 } else { 1.0 }),
        ];
        let compilation_options = wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        };

        let pipeline = |pass: Pass<'_>| {
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
                        write_mask: wgpu::ColorWrites::ALL,
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
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };

        // Positions in slot 0 and normals in slot 1, each a buffer of its
        // own. Edges only need positions.
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

        Self {
            background: pipeline(Pass {
                blend: wgpu::BlendState::REPLACE,
                ..Pass::overlay("varde background", "vs_fullscreen", "fs_background")
            }),
            grid: pipeline(Pass::overlay("varde grid", "vs_fullscreen", "fs_grid")),
            mesh: pipeline(Pass {
                label: "varde mesh",
                vs: "vs_mesh",
                fs: "fs_mesh",
                buffers: &[positions.clone(), normals],
                depth_write: true,
                blend: wgpu::BlendState::REPLACE,
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
            }),
            edges: pipeline(Pass {
                buffers: &[positions],
                topology: wgpu::PrimitiveTopology::LineList,
                ..Pass::overlay("varde edges", "vs_edge", "fs_edge")
            }),
            origin: pipeline(Pass::overlay("varde origin", "vs_origin", "fs_origin")),
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
            depth: None,
            viewport: Viewport::default(),
        }
    }

    /// Uploads what `frame` needs to draw into `slot`.
    ///
    /// Fails with [`PrepareError::MeshTooLarge`] when the frame's mesh
    /// doesn't fit in the device's buffers, which can be smaller than the
    /// 256 MiB every mesh the kernel allows fits in. The frame is then drawn without it, and it
    /// isn't tried again until the slot's frame has another mesh, so this
    /// is only reported once per mesh.
    pub fn prepare(
        &self,
        slot: &mut Slot,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<(), PrepareError> {
        slot.viewport = frame.viewport;

        let mut result = Ok(());
        if !std::ptr::eq(slot.source.as_ptr(), Arc::as_ptr(frame.mesh)) {
            slot.source = Arc::downgrade(frame.mesh);
            slot.mesh = upload_mesh(device, frame.mesh).unwrap_or_else(|error| {
                result = Err(error);
                None
            });
        }

        let bounds = slot.mesh.as_ref().and_then(|m| m.bounds);
        let (camera, colors) = (frame.camera, frame.colors);
        let aspect = frame.viewport.aspect();
        let half = camera.half_extents(aspect);
        let uniforms = Uniforms {
            view_proj: scene::view_projection(camera, aspect, bounds).to_cols_array_2d(),
            eye: camera.eye_homogeneous().to_array(),
            right: camera.right().extend(half.x).to_array(),
            up: camera.up().extend(half.y).to_array(),
            focus: camera.target().extend(0.0).to_array(),
            backward: camera.backward().extend(0.0).to_array(),
            viewport: [
                frame.viewport.width,
                frame.viewport.height,
                frame.scale_factor,
                0.0,
            ],
            background_top: linear(colors.background_top),
            background_bottom: linear(colors.background_bottom),
            model: linear(colors.model),
            edge: linear(colors.edge),
            grid: linear(colors.grid),
            axes: colors.axes.map(linear),
            origin_outline: linear(colors.origin_outline),
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
            pass.set_pipeline(&self.mesh);
            pass.set_vertex_buffer(0, mesh.positions.slice(..));
            pass.set_vertex_buffer(1, mesh.normals.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);

            if mesh.edge_count > 0 {
                pass.set_pipeline(&self.edges);
                pass.set_index_buffer(mesh.edges.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.edge_count, 0, 0..1);
            }
        }

        // Drawn after the model so they blend over the background and are
        // occluded by geometry.
        pass.set_pipeline(&self.grid);
        pass.draw(0..3, 0..1);

        pass.set_pipeline(&self.origin);
        pass.draw(0..ORIGIN_VERTICES, 0..1);
    }
}

/// Converts a colour to linear, which the shader works in.
fn linear(Srgb(rgb): Srgb) -> [f32; 4] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    [r, g, b, 1.0]
}

fn srgb_to_linear(c: f32) -> f32 {
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
