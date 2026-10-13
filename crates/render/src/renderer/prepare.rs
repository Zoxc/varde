//! A frame's per-slot state and [`Renderer::prepare`], which uploads it.

use super::*;

/// The scene shader's uniforms. `Uniforms` in `scene.wgsl` mirrors this
/// field for field, and the bind group layout's `min_binding_size` makes wgpu
/// check that it fits when each pipeline is built.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct Uniforms {
    pub(super) view_proj: [[f32; 4]; 4],
    /// The eye with w = 1, or in orthographic mode the direction towards the
    /// camera with w = 0, an eye at infinity.
    pub(super) eye: [f32; 4],
    /// xyz: unit vector right on screen, w: half the visible width at the
    /// target in world units.
    pub(super) right: [f32; 4],
    /// xyz: unit vector up on screen, w: half the visible height at the
    /// target in world units.
    pub(super) up: [f32; 4],
    /// xyz: orbit target, w: how far in front of the eye a perspective
    /// view starts, [`Camera::near`]. Not `target`, which WGSL reserves.
    pub(super) focus: [f32; 4],
    /// Unit vector towards the camera; w: [`PatternStyle::wash`].
    pub(super) backward: [f32; 4],
    /// xy: viewport size in physical pixels, z: physical pixels per logical
    /// pixel, w: [`PatternStyle::stripes`].
    pub(super) viewport: [f32; 4],
    /// xy: the viewport's top left corner on the target, in physical
    /// pixels, where fragment positions count from; z: [`Frame::shading`]
    /// ([`Shading::code`]); w: [`Colors::contrast`].
    pub(super) viewport_origin: [f32; 4],
    /// [`Colors`], converted to linear with w = 1, except `model`, whose
    /// w is how opaque the model is, and `edge`, whose w is
    /// [`Colors::hidden_edge_alpha`]. The errors' colours are in
    /// [`ErrorUniforms`].
    pub(super) background_top: [f32; 4],
    pub(super) background_bottom: [f32; 4],
    pub(super) model: [f32; 4],
    pub(super) edge: [f32; 4],
    pub(super) grid: [f32; 4],
    pub(super) axes: [[f32; 4]; 3],
    pub(super) origin_outline: [f32; 4],
    pub(super) sketch: [f32; 4],
    /// [`Frame::pivot`]: xyz where it is, w its opacity, 0 if it isn't
    /// drawn; and its core's colour.
    pub(super) pivot: [f32; 4],
    pub(super) pivot_color: [f32; 4],
    /// [`Frame::grid`]: xyz its origin, and unit x and y axes. The axes'
    /// w is [`axis_index`], for the colour of its axis line; the origin's
    /// is [`OriginShown::mask`], what's drawn of the axes and the marker.
    pub(super) grid_origin: [f32; 4],
    pub(super) grid_x: [f32; 4],
    pub(super) grid_y: [f32; 4],
    /// [`SketchScene::plane`]: xyz its origin, and unit x and y axes; the
    /// w's [`Colors::second`]'s red, green and blue.
    pub(super) sketch_origin: [f32; 4],
    pub(super) sketch_x: [f32; 4],
    pub(super) sketch_y: [f32; 4],
    /// [`Colors::hover_face`], with w [`Colors::selected_tint`],
    /// [`Colors::hover_outline`], with w [`PatternStyle::edge_width`], and [`Colors::selected`],
    /// with w [`Colors::selected_edge_shade`].
    pub(super) hover_face: [f32; 4],
    pub(super) hover_outline: [f32; 4],
    pub(super) selected: [f32; 4],
}

// WGSL lays out uniform structs in 16-byte steps.
const _: () = assert!(size_of::<Uniforms>().is_multiple_of(16));
// The limits tests' device takes buffers of 512 bytes at most, and the
// uniforms are at that: a new value has to go in an unused `w`, or be
// worked out from another (as the axis lines' colours are from `grid_x.w`).
const _: () = assert!(size_of::<Uniforms>() <= 512);

/// The errors' colours, for the pipelines drawing their halo's composite
/// and their core: `ErrorColors` in `scene.wgsl` mirrors it. Their own,
/// since [`Uniforms`] are full.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct ErrorUniforms {
    /// [`Colors::error`], linear, w = 1.
    pub(super) core: [f32; 4],
    /// [`Colors::error_halo`], linear, its alpha as it is.
    pub(super) halo: [f32; 4],
}

/// The size of an entry of a slot's tints: a linear colour, w unused. See
/// [`Slot::tints`].
pub(super) const TINT_SIZE: u64 = 16;

/// The most alphas a part can be drawn at, from 0 to 1: 8 bits' worth, as
/// fine as the target shows. See [`Alphas`].
pub(super) const ALPHA_STEPS: u32 = 256;

/// What a draw of a part of the mesh takes besides [`Uniforms`]. `Part`
/// in `scene.wgsl` mirrors it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct PartUniforms {
    /// x: the part's alpha, multiplying the faces' and edges' own; yzw
    /// unused.
    pub(super) alpha: [f32; 4],
}

/// The alphas the mesh's parts are drawn at: a uniform buffer, written
/// once, of a [`PartUniforms`] for each of [`ALPHA_STEPS`] steps from 0 to
/// 1 at the device's uniform offset alignment, picked per draw by dynamic
/// offset. WebGL2 has no push constants. A device whose buffers hold fewer
/// gets fewer, at least one, opaque.
pub(super) struct Alphas {
    /// Group 1 of every pipeline.
    pub(super) group: wgpu::BindGroup,
    /// How far apart the steps are in the buffer, in bytes.
    pub(super) stride: u32,
    /// The last step, opaque.
    pub(super) opaque: u32,
}

impl Alphas {
    pub(super) fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Alphas {
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
    pub(super) fn step(&self, alpha: Option<f32>) -> u32 {
        alpha_step(alpha, self.opaque)
    }

    /// Sets the alpha the parts drawn next are drawn at to `step`'s.
    pub(super) fn set(&self, pass: &mut wgpu::RenderPass<'_>, step: u32) {
        pass.set_bind_group(1, &self.group, &[step * self.stride]);
    }
}

/// The nearest step to `alpha` of a table of alphas from 0 to 1 whose last
/// step, `opaque`, is opaque; opaque if `alpha` is out of range or NaN.
/// Only 0 gets step 0, so on a short table (tiny buffers) a faint part is
/// drawn at the next step up rather than not at all.
pub(super) fn alpha_step(alpha: Option<f32>, opaque: u32) -> u32 {
    match alpha {
        Some(0.0) => 0,
        // In range, so the cast is exact.
        Some(alpha) if (0.0..=1.0).contains(&alpha) => {
            ((alpha * opaque as f32).round() as u32).max(1).min(opaque)
        }
        _ => opaque,
    }
}

/// A face of the mesh drawn again over itself, hovered or selected: see
/// [`Frame::hovered_faces`].
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FaceDraw {
    pub(super) indices: Range<u32>,
    /// Its part's alpha's step, see [`Alphas`].
    pub(super) step: u32,
    /// Brightened as hovered, or tinted as selected or in the second
    /// colour.
    pub(super) tint: Tint,
    /// Its part, for its colour ([`Slot::part_tints`]).
    pub(super) part: usize,
    /// Whether it's both hovered and selected: its hidden stripes are
    /// then the selection's alone, from the hover's colour, as it's shown.
    pub(super) hovered: bool,
}

/// How a [`FaceDraw`] is drawn again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tint {
    Hovered,
    Selected,
    Second,
}

/// Which parts of a frame's mesh are drawn how, worked out in
/// [`Renderer::prepare`] from [`Frame::opacity`].
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct PartDraws {
    /// The runs of opaque parts, one after another, each drawn at once.
    pub(super) opaque: Vec<Range<usize>>,
    /// The parts less than opaque, far to near, and their alphas' steps.
    pub(super) transparent: Vec<(usize, u32)>,
}

impl PartDraws {
    /// How `parts` are drawn at `opacity`, at `alphas`' steps, in the
    /// colours of `tints`' entries ([`Slot::part_tints`]), seen from
    /// `camera`. A run of opaque parts is of one colour.
    pub(super) fn new(
        parts: &[GpuPart],
        opacity: &[f32],
        tints: &[u32],
        alphas: &Alphas,
        camera: &Camera,
    ) -> PartDraws {
        let mut draws = PartDraws::default();
        let tint = |i: usize| tints.get(i).copied().unwrap_or(0);
        for i in 0..parts.len() {
            // Not drawn at all: left out of a preview.
            if opacity.get(i) == Some(&0.0) {
                continue;
            }
            let step = alphas.step(opacity.get(i).copied());
            if step < alphas.opaque {
                draws.transparent.push((i, step));
            } else if let Some(run) =
                (draws.opaque.last_mut()).filter(|run| run.end == i && tint(run.start) == tint(i))
            {
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

/// Why [`Renderer::prepare`] couldn't upload some of a frame: a buffer of
/// `upload` would take `bytes`, more than the device's `max_buffer_size`
/// of `limit`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{} {} a {bytes}-byte buffer, but the device allows at most {limit} bytes",
    upload.subject().0,
    upload.subject().1
)]
pub struct PrepareError {
    pub upload: Upload,
    pub bytes: u64,
    pub limit: u64,
}

/// What of a frame [`PrepareError`] couldn't upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upload {
    /// The mesh, or its triangles' edges.
    Mesh,
    /// The sketches' lines.
    Lines,
    /// A layer of the sketch being edited.
    Sketch,
    /// The hovered and selected edges and vertices.
    Highlights,
    /// The errors' geometry ([`Frame::errors`]).
    Errors,
}

impl Upload {
    /// What the error's sentence is about, and its verb.
    fn subject(self) -> (&'static str, &'static str) {
        match self {
            Upload::Mesh => ("the mesh", "needs"),
            Upload::Lines => ("the sketches", "need"),
            Upload::Sketch => ("the sketch being edited", "needs"),
            Upload::Highlights => ("what's hovered and selected", "needs"),
            Upload::Errors => ("the errors' geometry", "needs"),
        }
    }

    /// The error for a buffer of this taking `bytes`, past `limit`.
    pub(super) fn too_large(self, bytes: u64, limit: u64) -> PrepareError {
        PrepareError {
            upload: self,
            bytes,
            limit,
        }
    }
}

/// One scene's state on the GPU, from [`Renderer::prepare`] to
/// [`Renderer::render`]: its uniforms, mesh, lines and depth buffer.
///
/// Each scene drawn in the same frame needs its own slot, since every
/// `prepare` may run before any `render`. Keeping a slot across frames
/// keeps its mesh uploaded.
pub struct Slot {
    pub(super) uniforms: wgpu::Buffer,
    /// Group 0: the uniforms, and the tints at a dynamic offset.
    pub(super) bind_group: wgpu::BindGroup,
    /// The uniforms again with the grid on the other plane of
    /// [`Frame::fading_grid`]'s crossfade, and their group 0.
    pub(super) other_grid: wgpu::Buffer,
    pub(super) other_grid_group: wgpu::BindGroup,
    /// The colours the mesh's parts are drawn in, linear, an entry each
    /// [`Renderer::tint_stride`] apart: the first [`Colors::model`], then
    /// one for each other colour of [`Frame::tints`]. Written each
    /// prepare; made again, with the bind group, when it needs more room.
    pub(super) tints: wgpu::Buffer,
    /// How many entries `tints` holds.
    pub(super) tint_room: u32,
    /// The entry of `tints` each part of the mesh is drawn in.
    pub(super) part_tints: Vec<u32>,
    pub(super) mesh: Option<GpuMesh>,
    /// The [`Frame::mesh`] `mesh` was uploaded from, or skipped for. Holding
    /// it keeps the allocation from being reused for another mesh, and
    /// `Arc::get_mut` from changing that mesh in place.
    pub(super) source: Weak<RenderMesh>,
    pub(super) lines: Option<GpuLines>,
    /// The [`Frame::sketches`] `lines` were uploaded from, like `source`.
    pub(super) lines_source: Weak<RenderLines>,
    /// The layers of [`Frame::sketch`], and the base layer's `Arc` like
    /// `source`. The live layer's buffers are kept to be written again.
    pub(super) sketch_base: SketchBuffers,
    pub(super) sketch_source: Weak<SketchLayer>,
    pub(super) sketch_live: SketchBuffers,
    pub(super) depth: Option<DepthTarget>,
    /// What the draws do, set afresh each prepare.
    pub(super) state: SlotState,
    /// [`Frame::highlights`] as uploaded, and its `Arc` like `source`.
    pub(super) highlights: HighlightBuffers,
    pub(super) highlights_source: Weak<Highlights>,
    /// [`Frame::errors`] as uploaded, and their sources like `source`.
    pub(super) errors: ErrorBuffers,
    pub(super) error_sources: Vec<Weak<dyn Any + Send + Sync>>,
    /// The halo's target and the errors' colours, made the first time
    /// there are errors and again when the target is resized while there
    /// are; kept while there are none, so showing and hiding them (a
    /// hover) makes no texture.
    pub(super) error_target: Option<CoverageTarget>,
    /// The selected faces' pattern's mask, made the first time some are
    /// selected and kept like `error_target`.
    pub(super) pattern_target: Option<CoverageTarget>,
    /// The bytes the last prepare wrote to the uniforms, tints and live
    /// sketch layer, to tell whether the next one changed them.
    pub(super) written: Vec<u8>,
    /// The clip the scene's resolved target holds the frame last prepared
    /// for, drawn: a frame prepared alike (a redraw of the window for
    /// something else, a hover that changes nothing here) is only
    /// composited again. `None` until it's drawn.
    pub(super) drawn: Mutex<Option<ClipRect>>,
}

impl Slot {
    /// Whether the model has glass to draw: [`Renderer::draw_scene`] draws
    /// a depth-tested sketch under it, and [`Renderer::draw_sketch`] only
    /// one without.
    pub(super) fn has_glass(&self) -> bool {
        self.mesh.is_some() && !self.state.draws.transparent.is_empty()
    }
}

/// What [`Renderer::prepare`] leaves in a [`Slot`] for the draws, beside
/// its buffers, all of it set afresh each prepare: two alike draw the same.
#[derive(PartialEq)]
pub(super) struct SlotState {
    /// The alpha steps the grid is drawn at, on [`Frame::grid`] and on
    /// the other plane if it's crossfading: see [`Frame::fading_grid`].
    pub(super) grid_steps: (u32, Option<u32>),
    /// Whether the frame has a sketch to draw, and whether it's depth
    /// tested.
    pub(super) sketching: bool,
    pub(super) sketch_depth: bool,
    pub(super) viewport: Viewport,
    pub(super) faded: bool,
    /// [`Frame::hover_through`], never while faded.
    pub(super) hover_through: bool,
    /// How the mesh's parts are drawn, by [`Frame::opacity`].
    pub(super) draws: PartDraws,
    /// Whether the edges the model hides are drawn: [`Frame::hidden_edges`]
    /// and not [`Self::faded`].
    pub(super) hidden_edges: bool,
    /// What's drawn of [`Frame::origin`]: while [`Self::faded`], in a
    /// sketch, only the grid's axis lines and the marker, the sketch's.
    pub(super) origin: OriginShown,
    /// The faces hovered and selected, the hovered first: none while
    /// [`Self::faded`].
    pub(super) faces: Vec<FaceDraw>,
}

impl Default for SlotState {
    fn default() -> SlotState {
        SlotState {
            grid_steps: (0, None),
            sketching: false,
            sketch_depth: false,
            viewport: Viewport::default(),
            faded: false,
            hover_through: false,
            draws: PartDraws::default(),
            hidden_edges: false,
            origin: OriginShown::NONE,
            faces: Vec::new(),
        }
    }
}

impl Renderer {
    /// A slot's tints' buffer with room for `room` entries, and its
    /// groups 0 binding it with `uniforms` and with `other_grid`.
    pub(super) fn tints(
        &self,
        device: &wgpu::Device,
        uniforms: &wgpu::Buffer,
        other_grid: &wgpu::Buffer,
        room: u32,
    ) -> (wgpu::Buffer, wgpu::BindGroup, wgpu::BindGroup) {
        let tints = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde tints"),
            size: u64::from(room) * u64::from(self.tint_stride),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = |uniforms: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("varde uniforms"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &tints,
                            offset: 0,
                            size: wgpu::BufferSize::new(TINT_SIZE),
                        }),
                    },
                ],
            })
        };
        let (bind_group, other_group) = (group(uniforms), group(other_grid));
        (tints, bind_group, other_group)
    }

    /// Writes the colours `frame`'s parts are drawn in to `slot`'s tints,
    /// making room for them if they need it and the device has it; a
    /// part whose colour finds none is drawn in the model's.
    fn write_tints(&self, slot: &mut Slot, p: &mut Prepare<'_, '_>) {
        let (device, frame) = (p.device, p.frame);
        let parts = slot.mesh.as_ref().map_or(0, |mesh| mesh.parts.len());
        let model = frame.colors.model;
        let mut colors = vec![model.linear()];
        slot.part_tints.clear();
        let most = (device.limits().max_buffer_size / u64::from(self.tint_stride)).max(1);
        for i in 0..parts {
            let Some(tint) = frame.tints.get(i).copied().flatten() else {
                slot.part_tints.push(0);
                continue;
            };
            let color = model.tinted(tint).linear();
            let entry = match colors.iter().position(|&c| c == color) {
                Some(at) => at,
                None if (colors.len() as u64) < most => {
                    colors.push(color);
                    colors.len() - 1
                }
                None => 0,
            };
            // At most the parts' count plus one, well within u32.
            slot.part_tints.push(entry as u32);
        }
        // Within `most`, which fits a buffer.
        let needed = colors.len() as u32;
        if needed > slot.tint_room {
            let room = needed.next_power_of_two().min(most as u32).max(needed);
            (slot.tints, slot.bind_group, slot.other_grid_group) =
                self.tints(device, &slot.uniforms, &slot.other_grid, room);
            slot.tint_room = room;
        }
        let stride = self.tint_stride as usize;
        let mut bytes = vec![0u8; colors.len() * stride];
        for (color, entry) in colors.iter().zip(bytes.chunks_exact_mut(stride)) {
            entry[..TINT_SIZE as usize].copy_from_slice(bytemuck::bytes_of(color));
        }
        p.write(&slot.tints, &bytes);
    }

    /// Creates a slot to draw a scene with, empty until it's prepared.
    pub fn slot(&self, device: &wgpu::Device) -> Slot {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let other_grid = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde uniforms, other grid"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (tints, bind_group, other_grid_group) = self.tints(device, &uniforms, &other_grid, 1);
        Slot {
            uniforms,
            bind_group,
            other_grid,
            other_grid_group,
            tints,
            tint_room: 1,
            part_tints: Vec::new(),
            mesh: None,
            source: Weak::new(),
            lines: None,
            lines_source: Weak::new(),
            sketch_base: SketchBuffers::default(),
            sketch_source: Weak::new(),
            sketch_live: SketchBuffers::default(),
            depth: None,
            state: SlotState {
                grid_steps: (self.alphas.opaque, None),
                ..SlotState::default()
            },
            highlights: HighlightBuffers::default(),
            highlights_source: Weak::new(),
            errors: ErrorBuffers::default(),
            error_sources: Vec::new(),
            error_target: None,
            pattern_target: None,
            written: Vec::new(),
            drawn: Mutex::new(None),
        }
    }

    /// Uploads what `frame` needs to draw into `slot`.
    ///
    /// Fails with [`Upload::Mesh`] when the frame's mesh
    /// doesn't fit in the device's buffers, which can be smaller than the
    /// 256 MiB every mesh the kernel allows fits in. The frame is then
    /// drawn without it, and it isn't tried again until the slot's frame
    /// has another mesh, so this is only reported once per mesh. The same
    /// for the frame's sketches with [`Upload::Lines`], and
    /// the layers of the sketch being edited with
    /// [`Upload::Sketch`], its live layer reported each frame
    /// it's too large, and the errors' geometry with
    /// [`Upload::Errors`], tried again only once their
    /// sources change; should several fail at once, the mesh's error is
    /// the one returned, then the sketches'.
    pub fn prepare(
        &self,
        slot: &mut Slot,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<(), PrepareError> {
        let before = std::mem::take(&mut slot.state);
        let mut p = Prepare {
            device,
            queue,
            frame,
            colors: frame.colors.sanitized(),
            written: Vec::with_capacity(slot.written.len()),
            changed: false,
        };
        slot.state.viewport = frame.viewport;
        let fade = if frame.fade.is_nan() {
            0.0
        } else {
            frame.fade.clamp(0.0, 1.0)
        };
        let faded = fade > 0.0;
        // Wholly faded, the model's drawn as one faded surface; on the way,
        // its parts as glass growing fainter, and its hidden edges with it.
        slot.state.faded = fade == 1.0;
        slot.state.hover_through = frame.hover_through && !faded;
        slot.state.hidden_edges = frame.hidden_edges && !slot.state.faded;
        // In a sketch, the grid's axis lines and the marker are the
        // sketch's.
        slot.state.origin = if faded {
            OriginShown::SKETCH
        } else {
            frame.origin.with_hovered()
        };

        let result = self.upload_sources(slot, &mut p);
        let opacity = opacity(slot, frame, &p.colors, fade);
        self.write_tints(slot, &mut p);
        let parts = slot.mesh.as_ref().map_or(&[][..], |mesh| &mesh.parts);
        slot.state.draws = PartDraws::new(
            parts,
            &opacity,
            &slot.part_tints,
            &self.alphas,
            frame.camera,
        );
        self.face_draws(slot, frame, &opacity);
        slot.state.sketching = frame.sketch.is_some();
        slot.state.sketch_depth = frame.sketch.is_some_and(|sketch| sketch.depth_tested);

        let uniforms = uniforms(slot, frame, &p.colors, fade);
        self.write_grids(slot, &mut p, &uniforms, fade);
        self.ensure_targets(slot, &mut p);
        if p.changed || p.written != slot.written || before != slot.state {
            *slot.drawn.get_mut().unwrap_or_else(|e| e.into_inner()) = None;
        }
        slot.written = p.written;
        result
    }

    /// Uploads what of `frame`'s geometry `slot` doesn't hold yet: its
    /// sketches, mesh and the mesh's triangle edges, highlights, errors and
    /// the sketch being edited, noting in `changed` that it did, and the
    /// live sketch layer's bytes in `written`. Returns the first failure,
    /// as [`Renderer::prepare`] describes.
    fn upload_sources(&self, slot: &mut Slot, p: &mut Prepare<'_, '_>) -> Result<(), PrepareError> {
        let (device, queue, frame) = (p.device, p.queue, p.frame);
        let mut result = Ok(());
        if !std::ptr::eq(slot.lines_source.as_ptr(), Arc::as_ptr(frame.sketches)) {
            p.changed = true;
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
            p.changed = true;
            slot.source = Arc::downgrade(frame.mesh);
            slot.mesh = upload_mesh(device, frame.mesh).unwrap_or_else(|error| {
                result = Err(error);
                None
            });
        }
        if let Some(mesh) = &mut slot.mesh {
            // Drawn otherwise, though nothing's uploaded.
            p.changed |=
                mesh.wireframe != frame.wireframe || mesh.tessellation != frame.tessellation;
            mesh.wireframe = frame.wireframe;
            mesh.tessellation = frame.tessellation;
            if frame.tessellation && matches!(mesh.triangles, Triangles::Unbuilt) {
                // Like the mesh's, a failure isn't tried again.
                p.changed = true;
                mesh.triangles = match triangle_edges(device, frame.mesh, &mesh.parts) {
                    Ok(triangles) => triangles,
                    Err(error) => {
                        result = result.and(Err(error));
                        Triangles::TooLarge
                    }
                };
            }
        }

        // What's hovered and selected names the mesh's faces, edges and
        // vertices, so it's written again with the mesh too.
        if new_mesh
            || !std::ptr::eq(
                slot.highlights_source.as_ptr(),
                Arc::as_ptr(frame.highlights),
            )
        {
            p.changed = true;
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
            p.changed = true;
            slot.error_sources = frame.errors.iter().map(|e| e.source.clone()).collect();
            let written = slot.errors.write(device, queue, frame.errors);
            result = result.and(written);
        }

        if let Some(sketch) = &frame.sketch {
            let mut sketch_result = Ok(());
            if !std::ptr::eq(slot.sketch_source.as_ptr(), Arc::as_ptr(sketch.base)) {
                p.changed = true;
                slot.sketch_source = Arc::downgrade(sketch.base);
                if let Err(error) = slot.sketch_base.write(device, queue, sketch.base) {
                    sketch_result = Err(error);
                }
            }
            let live = sketch.live;
            p.written
                .extend_from_slice(bytemuck::cast_slice(&live.lines));
            p.written
                .extend_from_slice(bytemuck::cast_slice(&live.points));
            p.written
                .extend_from_slice(bytemuck::cast_slice(&live.fills));
            if let Err(error) = slot.sketch_live.write(device, queue, sketch.live) {
                sketch_result = sketch_result.and(Err(error));
            }
            result = result.and(sketch_result);
        }
        result
    }

    /// Lists the draws of `frame`'s hovered, selected and second faces in
    /// `slot`, each at its part's `opacity`.
    fn face_draws(&self, slot: &mut Slot, frame: &Frame<'_>, opacity: &[f32]) {
        slot.state.faces.clear();
        let Some(gpu) = &slot.mesh else {
            return;
        };
        let tinted = [
            (frame.hovered_faces, Tint::Hovered),
            (frame.selected_faces, Tint::Selected),
            (frame.second_faces, Tint::Second),
        ];
        let faces = (tinted.into_iter())
            .flat_map(|(faces, tint)| faces.iter().map(move |&face| (face, tint)));
        for (face, tint) in faces {
            if let Some((indices, part)) = gpu.face(frame.mesh, face) {
                slot.state.faces.push(FaceDraw {
                    indices,
                    step: self.alphas.step(opacity.get(part).copied()),
                    tint,
                    part,
                    hovered: frame.hovered_faces.contains(&face)
                        && frame.selected_faces.contains(&face),
                });
            }
        }
    }

    /// Writes `uniforms` to `slot`'s, and when `frame` crossfades between
    /// two grids, each grid's share of them to its own, setting the steps
    /// they're drawn at.
    fn write_grids(
        &self,
        slot: &mut Slot,
        p: &mut Prepare<'_, '_>,
        uniforms: &Uniforms,
        fade: f32,
    ) {
        let frame = p.frame;
        let grid = &frame.grid;
        // Crossfading between the world's XY plane and a sketch's, each
        // drawn at its share, the one that isn't `grid` from the other
        // uniforms.
        let (steps, main, other) = match frame.fading_grid {
            // On the XY plane the grid stays; the axis lines it has outside
            // the sketch, not in it, the Z axis's, fade over it.
            Some(plane) if plane == GridPlane::XY => {
                let z_only = WORLD_Z | WORLD_Z << HOVERED_SHIFT;
                let outside = frame.origin.with_hovered().mask(&plane) & z_only | AXES_ONLY;
                let with_mask = |mask: u32| Uniforms {
                    grid_origin: plane.origin().extend(mask as f32).to_array(),
                    ..*uniforms
                };
                (
                    (self.alphas.opaque, Some(self.alphas.step(Some(1.0 - fade)))),
                    with_mask(OriginShown::SKETCH.mask(&plane)),
                    Some(with_mask(outside)),
                )
            }
            Some(plane) => {
                let share = |on: &GridPlane| if *on == plane { fade } else { 1.0 - fade };
                let other = if *grid == plane { GridPlane::XY } else { plane };
                // Each grid with its own axis lines, the XY plane's with
                // the Z axis's, fading with it either way.
                let on = |at: &GridPlane| {
                    let shown = if *at == plane {
                        OriginShown::SKETCH
                    } else {
                        frame.origin.with_hovered()
                    };
                    Uniforms {
                        grid_origin: at.origin().extend(shown.mask(at) as f32).to_array(),
                        grid_x: at.x().extend(axis_index(at.x())).to_array(),
                        grid_y: at.y().extend(axis_index(at.y())).to_array(),
                        ..*uniforms
                    }
                };
                (
                    (
                        self.alphas.step(Some(share(grid))),
                        Some(self.alphas.step(Some(share(&other)))),
                    ),
                    on(grid),
                    Some(on(&other)),
                )
            }
            None => ((self.alphas.opaque, None), *uniforms, None),
        };
        slot.state.grid_steps = steps;
        if let Some(other) = other {
            p.write(&slot.other_grid, bytemuck::bytes_of(&other));
        }
        p.write(&slot.uniforms, bytemuck::bytes_of(&main));
    }

    /// Makes `slot`'s depth target, and the selected faces' pattern's and
    /// the errors' when they're drawn, at `frame`'s target size if they
    /// aren't yet, noting in `changed` that it did; and writes the errors'
    /// colours.
    fn ensure_targets(&self, slot: &mut Slot, p: &mut Prepare<'_, '_>) {
        let device = p.device;
        let size = p.frame.target_size.map(|s| s.max(1));
        if slot.depth.as_ref().map(|d| d.size) != Some(size) {
            p.changed = true;
            slot.depth = Some(self.create_targets(device, size));
        }
        if patterned(slot) && slot.pattern_target.as_ref().map(|t| t.size) != Some(size) {
            p.changed = true;
            slot.pattern_target = Some(CoverageTarget::new(
                device,
                &self.errors.layout,
                &slot.uniforms,
                ("varde selected pattern", self.format),
                size,
                self.samples,
            ));
        }
        if !slot.errors.any() {
            return;
        }
        if slot.error_target.as_ref().map(|t| t.size) != Some(size) {
            p.changed = true;
            slot.error_target = Some(CoverageTarget::new(
                device,
                &self.errors.layout,
                &slot.uniforms,
                ("varde error halo", HALO_FORMAT),
                size,
                self.samples,
            ));
        }
        if let Some(target) = &slot.error_target {
            let uniforms = ErrorUniforms {
                core: p.colors.error.linear(),
                halo: p.colors.error_halo.linear(),
            };
            p.write(&target.uniforms, bytemuck::bytes_of(&uniforms));
        }
    }
}

/// What the steps of [`Renderer::prepare`] share: the device, the
/// frame and its colours sanitized, and what they did that the slot's
/// state doesn't show, to tell whether its last drawing still holds.
struct Prepare<'a, 'f> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    frame: &'a Frame<'f>,
    colors: Colors,
    /// The bytes written to the slot's small buffers, and the live
    /// sketch layer's, to compare with [`Slot::written`].
    written: Vec<u8>,
    /// Whether something was uploaded or made.
    changed: bool,
}

impl Prepare<'_, '_> {
    /// Writes `bytes` to `buffer` and appends them to `written`.
    fn write(&mut self, buffer: &wgpu::Buffer, bytes: &[u8]) {
        self.queue.write_buffer(buffer, 0, bytes);
        self.written.extend_from_slice(bytes);
    }
}

/// How opaque each of `slot`'s parts is drawn in `frame`, `fade` of the
/// way to the faded model: none when wholly faded, every part drawn alike.
fn opacity<'a>(
    slot: &Slot,
    frame: &Frame<'a>,
    colors: &Colors,
    fade: f32,
) -> std::borrow::Cow<'a, [f32]> {
    if slot.state.faded {
        return Default::default();
    }
    if fade == 0.0 {
        return frame.opacity.into();
    }
    // Towards the alpha whose two layers, glass's back and front faces,
    // cover as much as the faded model's one: the nearest only.
    let to = 1.0 - (1.0 - colors.faded_alpha).sqrt();
    let parts = slot.mesh.as_ref().map_or(0, |mesh| mesh.parts.len());
    (0..parts)
        .map(|i| {
            let own = frame
                .opacity
                .get(i)
                .copied()
                .filter(|a| (0.0..=1.0).contains(a));
            let own = own.unwrap_or(1.0);
            own + (to - own) * fade
        })
        .collect()
}

/// The box the origin planes [`OriginShown`] has drawn take, seen with
/// `camera`, if it has any: see [`PLANE_REACH`].
pub(super) fn planes_bounds(camera: &Camera, origin: OriginShown) -> Option<Aabb> {
    let reach = PLANE_REACH * camera.view_height();
    (origin.planes.contains(&true) && reach.is_finite()).then(|| Aabb {
        min: Vec3::splat(-reach),
        max: Vec3::splat(reach),
    })
}

/// Which world axis `axis`, a unit vector, lies along, 0 to 2, or 3 if
/// none, for its axis line's colour: of [`Colors::axes`], or the grid's.
pub(super) fn axis_index(axis: Vec3) -> f32 {
    let along = axis.abs();
    (0..3)
        .find(|&i| along[i] > 1.0 - 1e-6)
        .map_or(3.0, |i| i as f32)
}

/// The scene shader's uniforms for `frame` drawn into `slot`, in
/// `colors`, sanitized, `fade` of the way to the faded model.
fn uniforms(slot: &Slot, frame: &Frame<'_>, colors: &Colors, fade: f32) -> Uniforms {
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
        planes_bounds(frame.camera, slot.state.origin),
    ]
    .into_iter()
    .chain(sketch_bounds.into_iter().flatten());
    let (camera, grid) = (frame.camera, &frame.grid);
    let sketch_plane = frame.sketch.map_or(*grid, |sketch| sketch.plane);
    let aspect = frame.viewport.aspect();
    let half = camera.half_extents(aspect);
    let alpha = if slot.state.faded {
        colors.faded_alpha
    } else {
        1.0
    };
    let with_alpha = |color: Srgb, alpha| {
        let [r, g, b, _] = color.linear();
        [r, g, b, alpha]
    };
    let second = colors.second.linear();
    let pattern = colors.pattern;
    Uniforms {
        view_proj: scene::view_projection(camera, aspect, grid, bounds.flatten())
            .to_cols_array_2d(),
        eye: camera.eye_homogeneous().to_array(),
        right: camera.right().extend(half.x).to_array(),
        up: camera.up().extend(half.y).to_array(),
        focus: camera.target().extend(camera.near()).to_array(),
        backward: camera.backward().extend(pattern.wash).to_array(),
        viewport: [
            frame.viewport.width,
            frame.viewport.height,
            frame.scale_factor,
            pattern.stripes,
        ],
        viewport_origin: [
            frame.viewport.x,
            frame.viewport.y,
            frame.shading.code(),
            colors.contrast,
        ],
        background_top: colors.background_top.linear(),
        background_bottom: colors.background_bottom.linear(),
        model: with_alpha(colors.model, alpha),
        edge: with_alpha(colors.edge, colors.hidden_edge_alpha * (1.0 - fade)),
        grid: colors.grid.linear(),
        axes: colors.axes.map(Srgb::linear),
        origin_outline: colors.origin_outline.linear(),
        sketch: colors.sketch.linear(),
        pivot: frame
            .pivot
            .filter(|pivot| pivot.at.is_finite() && (0.0..=1.0).contains(&pivot.opacity))
            .map_or([0.0; 4], |pivot| pivot.at.extend(pivot.opacity).to_array()),
        pivot_color: colors.pivot.linear(),
        grid_origin: (grid.origin())
            .extend(slot.state.origin.mask(grid) as f32)
            .to_array(),
        grid_x: grid.x().extend(axis_index(grid.x())).to_array(),
        grid_y: grid.y().extend(axis_index(grid.y())).to_array(),
        sketch_origin: sketch_plane.origin().extend(second[0]).to_array(),
        sketch_x: sketch_plane.x().extend(second[1]).to_array(),
        sketch_y: sketch_plane.y().extend(second[2]).to_array(),
        hover_face: with_alpha(colors.hover_face, colors.selected_tint),
        hover_outline: with_alpha(colors.hover_outline, pattern.edge_width),
        selected: with_alpha(colors.selected, colors.selected_edge_shade),
    }
}
