//! Building the render pipelines: the pass shapes, blends and stencils.

use super::*;

/// A pipeline's stencil state when it neither tests nor writes it.
pub(super) const NO_STENCIL: wgpu::StencilState = wgpu::StencilState {
    front: wgpu::StencilFaceState::IGNORE,
    back: wgpu::StencilFaceState::IGNORE,
    read_mask: 0,
    write_mask: 0,
};

/// The stencil bits [`Renderer::draw_hidden_picks`] marks where the
/// hovered and the selected faces are the nearest of the model with, so
/// what they hide of themselves (a selected body's far side) isn't drawn
/// as hidden. Above the glass's references.
pub(super) const PICK_MARKS: [u32; 2] = [0x40, 0x80];

/// Each pixel keeps the most drawn there: the errors' halo's coverage, so
/// overlapping halos don't add up, and where a face shows in the pattern's
/// mask, so it never covers the pattern.
pub(super) const MAX_BLENDING: wgpu::BlendState = wgpu::BlendState {
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
pub(super) struct Pass<'a> {
    pub(super) label: &'a str,
    /// Vertex and fragment entry points in the scene shader.
    pub(super) vs: &'a str,
    pub(super) fs: &'a str,
    pub(super) buffers: &'a [wgpu::VertexBufferLayout<'a>],
    pub(super) depth_write: bool,
    pub(super) depth_compare: wgpu::CompareFunction,
    pub(super) blend: wgpu::BlendState,
    pub(super) write_mask: wgpu::ColorWrites,
    pub(super) topology: wgpu::PrimitiveTopology,
    pub(super) cull_mode: Option<wgpu::Face>,
    pub(super) stencil: wgpu::StencilState,
    /// Sets the shader's `SKETCH_DEPTH`: the sketch's layers are given
    /// their depth, pulled towards the camera, rather than drawn on top.
    pub(super) sketch_depth: bool,
    /// The target's format if not the frame's: the errors' halo's.
    pub(super) format: Option<wgpu::TextureFormat>,
    /// The layout if not the scene's: the errors' composite's and core's,
    /// which read their colours and the halo's coverage (group 0).
    pub(super) layout: Option<&'a wgpu::PipelineLayout>,
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

/// The pipelines drawing the errors' geometry ([`Frame::errors`]), each
/// kind where it shows (`[0]`, depth `LessEqual`) and where the model hides
/// it (`[1]`, `Greater`, drawn at [`ERROR_HIDDEN`]).
pub(super) struct ErrorPipelines {
    /// The halo, as coverage into a [`HALO_FORMAT`] target of its own,
    /// blended by [`MAX_BLENDING`].
    pub(super) halo: [ErrorLayer; 2],
    /// The halo's coverage, once over the frame in
    /// [`Colors::error_halo`].
    pub(super) composite: wgpu::RenderPipeline,
    /// The geometry itself, over that.
    pub(super) core: [ErrorLayer; 2],
    /// Group 0 of the composite and the core: [`CoverageTarget::group`].
    pub(super) layout: wgpu::BindGroupLayout,
}

/// The pipelines drawing one way the errors' patches, curves and points.
pub(super) struct ErrorLayer {
    pub(super) faces: wgpu::RenderPipeline,
    pub(super) lines: wgpu::RenderPipeline,
    pub(super) points: wgpu::RenderPipeline,
}

/// The pipelines drawing the sketch's layers, one way or the other: see
/// [`SketchScene::depth_tested`].
pub(super) struct SketchPipelines {
    pub(super) fills: wgpu::RenderPipeline,
    pub(super) lines: wgpu::RenderPipeline,
    pub(super) points: wgpu::RenderPipeline,
}

impl SketchPipelines {
    /// Records drawing a layer of the sketch: its fills, then its lines,
    /// then its points.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, layer: &SketchBuffers) {
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

/// The composite of a slot's resolved scene over the frame's target of
/// `format`, premultiplied, and its group 0's layout: the scene.
pub(super) fn composite_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("varde composite"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/composite.wgsl").into()),
    });
    let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("varde composite"),
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
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("varde composite"),
        bind_group_layouts: &[&group],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("varde composite"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });
    (pipeline, group)
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = |label| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/scene.wgsl").into()),
            })
        };
        let shader = module("varde scene");
        // The same for the sketch's depth tested pipelines, which differ
        // from the others only by `SKETCH_DEPTH`: wgpu's GL backend (the
        // browser's WebGL2) caches a program by its module and entry
        // points, not its pipeline constants, so on one module they'd get
        // the on-top program.
        let depth_shader = module("varde scene, sketch depth tested");
        let depth_format = depth_format(device);
        let samples = sample_count(device, &[format, depth_format, HALO_FORMAT]);
        let (composite, composite_layout) = composite_pipeline(device, format);

        // The uniforms, and at binding 3 (the errors' group 0 has 1 and 2)
        // the colour of the part drawn, at a dynamic offset into the
        // slot's tints: see `Slot::tints`. Group 0 rather than a group of
        // its own: iced asks the device for two bind groups only.
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde uniforms"),
            entries: &[
                uniform_entry(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    false,
                    size_of::<Uniforms>() as u64,
                ),
                uniform_entry(3, wgpu::ShaderStages::FRAGMENT, true, TINT_SIZE),
            ],
        });
        let tint_stride =
            (device.limits().min_uniform_buffer_offset_alignment).max(TINT_SIZE as u32);
        // How opaque the part drawn is, at a dynamic offset into the
        // alphas' buffer: see `Alphas`. Every pipeline has it, so
        // it's bound once for all of them.
        let part_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde part"),
            entries: &[uniform_entry(
                0,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                true,
                size_of::<PartUniforms>() as u64,
            )],
        });
        let alphas = Alphas::new(device, &part_layout);

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde scene"),
            bind_group_layouts: &[&bind_group_layout, &part_layout],
            push_constant_ranges: &[],
        });
        // Group 0 of the halo's composite and the errors' core: the scene's
        // uniforms as everywhere, their colours (`ErrorUniforms`), and the
        // halo's coverage, read texel by texel (`textureLoad`, no sampler)
        // by the composite. In group 0 rather than a group of their own:
        // iced asks the device for two bind groups only.
        let errors_group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("varde errors"),
            entries: &[
                uniform_entry(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    false,
                    size_of::<Uniforms>() as u64,
                ),
                uniform_entry(
                    1,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    false,
                    size_of::<ErrorUniforms>() as u64,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let errors_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("varde errors"),
            bind_group_layouts: &[&errors_group, &part_layout],
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
                ("HOVER_THROUGH_ALPHA", f64::from(HOVER_THROUGH_ALPHA)),
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
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
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
        // of both tests, `pass` done to it, in the stencil bits `bits`
        // only.
        use wgpu::StencilOperation::{Keep, Replace};
        let marked = |bits: u32, compare, pass| {
            let face = wgpu::StencilFaceState {
                compare,
                fail_op: Keep,
                depth_fail_op: Keep,
                pass_op: pass,
            };
            wgpu::StencilState {
                front: face,
                back: face,
                read_mask: bits,
                write_mask: bits,
            }
        };
        // Where glass is the nearest of the model: its parts' references
        // are never 0, which is drawn with as the reference.
        let over_glass = marked(0xff, wgpu::CompareFunction::NotEqual, Keep);
        // Lines drawn from the `EdgePoint` stream, over the scene.
        // Pulled in by their distance from the edge, as the highlights'
        // lines below are.
        let edge_pass = |label, vs| Pass {
            buffers: &edge_points,
            ..Pass::overlay(label, vs, "fs_highlight_line")
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

        // Where something hides the faces, either side, but not where the
        // faces so tinted are the nearest, as marked in `mark`'s bit.
        let face_pattern = |label, fs, mark| Pass {
            label,
            fs,
            depth_write: false,
            depth_compare: wgpu::CompareFunction::Greater,
            cull_mode: None,
            stencil: marked(mark, wgpu::CompareFunction::NotEqual, Keep),
            ..mesh.clone()
        };
        let face_hidden = |label, fs, mark| Pass {
            blend: wgpu::BlendState::ALPHA_BLENDING,
            ..face_pattern(label, fs, mark)
        };
        // Where the faces are the nearest, marking `mark`'s bit there.
        let face_mark = |label, mark| Pass {
            label,
            write_mask: wgpu::ColorWrites::empty(),
            depth_write: false,
            depth_compare: wgpu::CompareFunction::Equal,
            cull_mode: None,
            stencil: marked(mark, wgpu::CompareFunction::Always, Replace),
            ..mesh.clone()
        };
        // The same, and into the pattern's mask.
        let face_shown = |label, mark| Pass {
            fs: "fs_face_shown",
            blend: MAX_BLENDING,
            write_mask: wgpu::ColorWrites::ALL,
            ..face_mark(label, mark)
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
                stencil: marked(0xff, wgpu::CompareFunction::Always, Replace),
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
            // Over everything, either side: entry points of their own.
            hover_face_through: pipeline(Pass {
                label: "varde hovered face through",
                fs: "fs_hover_face_through",
                blend: wgpu::BlendState::ALPHA_BLENDING,
                depth_write: false,
                depth_compare: wgpu::CompareFunction::Always,
                cull_mode: None,
                ..mesh.clone()
            }),
            // Where something hides it, either side, but not where the
            // faces so tinted are the nearest, as marked.
            selected_face_hidden: pipeline(face_hidden(
                "varde selected face hidden",
                "fs_selected_face_hidden",
                PICK_MARKS[1],
            )),
            hovered_selected_face_hidden: pipeline(face_hidden(
                "varde hovered selected face hidden",
                "fs_hovered_selected_face_hidden",
                PICK_MARKS[1],
            )),
            // The pattern's mask: where it's drawn, opaque.
            selected_face_pattern: pipeline(face_pattern(
                "varde selected face pattern",
                "fs_selected_face_pattern",
                PICK_MARKS[1],
            )),
            hovered_selected_face_pattern: pipeline(face_pattern(
                "varde hovered selected face pattern",
                "fs_hovered_selected_face_pattern",
                PICK_MARKS[1],
            )),
            hovered_face_pattern: pipeline(face_pattern(
                "varde hovered face pattern",
                "fs_hovered_face_pattern",
                PICK_MARKS[0],
            )),
            pattern_edge: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Always,
                layout: Some(&errors_layout),
                ..Pass::overlay(
                    "varde selected pattern edge",
                    "vs_fullscreen",
                    "fs_pattern_edge",
                )
            }),
            hovered_face_hidden: pipeline(face_hidden(
                "varde hovered face hidden",
                "fs_hovered_face_hidden",
                PICK_MARKS[0],
            )),
            hovered_face_mark: pipeline(face_mark("varde hovered face mark", PICK_MARKS[0])),
            selected_face_mark: pipeline(face_mark("varde selected face mark", PICK_MARKS[1])),
            hovered_face_shown: pipeline(face_shown("varde hovered face shown", PICK_MARKS[0])),
            selected_face_shown: pipeline(face_shown("varde selected face shown", PICK_MARKS[1])),
            mesh: pipeline(mesh),
            // Entry points of their own, which wgpu's GL backend keys
            // programs by.
            outline: pipeline(edge_pass("varde hover outline", "vs_outline")),
            hovered_edges: pipeline(edge_pass("varde hovered edges", "vs_hovered_edge")),
            selected_outline: pipeline(edge_pass("varde selected outline", "vs_selected_outline")),
            selected_edges: pipeline(edge_pass("varde selected edges", "vs_selected_edge")),
            // Exactly the pixels the selected edges didn't draw, as the
            // hidden edges are the visible ones'.
            selected_edges_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..edge_pass("varde selected edges hidden", "vs_selected_edge_hidden")
            }),
            outline_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..edge_pass("varde hover outline hidden", "vs_outline_hidden")
            }),
            selected_outline_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..edge_pass(
                    "varde selected outline hidden",
                    "vs_selected_outline_hidden",
                )
            }),
            hovered_edges_hidden: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Greater,
                ..edge_pass("varde hovered edges hidden", "vs_hovered_edge_hidden")
            }),
            second_outline: pipeline(edge_pass("varde second outline", "vs_second_outline")),
            second_edges: pipeline(edge_pass("varde second edges", "vs_second_edge")),
            vertices: pipeline(Pass {
                buffers: std::slice::from_ref(&vertices),
                ..Pass::overlay("varde vertices", "vs_vertex", "fs_highlight_point")
            }),
            outline_through: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Always,
                ..edge_pass("varde hover outline through", "vs_outline_through")
            }),
            hovered_edges_through: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Always,
                ..edge_pass("varde hovered edges through", "vs_hovered_edge_through")
            }),
            vertices_through: pipeline(Pass {
                buffers: std::slice::from_ref(&vertices),
                depth_compare: wgpu::CompareFunction::Always,
                ..Pass::overlay(
                    "varde hovered vertices through",
                    "vs_vertex_through",
                    "fs_highlight_point",
                )
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
                stencil: marked(0xff, wgpu::CompareFunction::Equal, Keep),
                ..edge_pass("varde edges hidden by glass", "vs_hidden_edge")
            }),
            lines: pipeline(Pass {
                buffers: std::slice::from_ref(&segments),
                ..Pass::overlay("varde sketch lines", "vs_line", "fs_line")
            }),
            lines_over_glass: pipeline(Pass {
                buffers: std::slice::from_ref(&segments),
                stencil: over_glass.clone(),
                ..Pass::overlay("varde sketch lines over glass", "vs_line", "fs_line")
            }),
            triangle_edges: pipeline(Pass {
                buffers: &[segments],
                ..Pass::overlay("varde triangle edges", "vs_triangle_edge", "fs_line")
            }),
            origin: pipeline(Pass::overlay("varde origin", "vs_origin", "fs_origin")),
            origin_planes: pipeline(Pass::overlay(
                "varde origin planes",
                "vs_origin_plane",
                "fs_origin_plane",
            )),
            origin_planes_on_top: pipeline(Pass {
                depth_compare: wgpu::CompareFunction::Always,
                ..Pass::overlay(
                    "varde origin planes on top",
                    "vs_origin_plane",
                    "fs_origin_plane",
                )
            }),
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
                    std::slice::from_ref(&sketch_fills),
                )),
                lines: pipeline(Pass::depth_tested(
                    "varde sketch lines, depth tested",
                    "vs_sketch_line",
                    "fs_line",
                    std::slice::from_ref(&sketch_lines),
                )),
                points: pipeline(Pass::depth_tested(
                    "varde sketch points, depth tested",
                    "vs_point",
                    "fs_point",
                    std::slice::from_ref(&sketch_points),
                )),
            },
            sketch_over_glass: SketchPipelines {
                fills: pipeline(Pass {
                    stencil: over_glass.clone(),
                    ..Pass::depth_tested(
                        "varde sketch fills, over glass",
                        "vs_fill",
                        "fs_fill",
                        std::slice::from_ref(&sketch_fills),
                    )
                }),
                lines: pipeline(Pass {
                    stencil: over_glass.clone(),
                    ..Pass::depth_tested(
                        "varde sketch lines, over glass",
                        "vs_sketch_line",
                        "fs_line",
                        std::slice::from_ref(&sketch_lines),
                    )
                }),
                points: pipeline(Pass {
                    stencil: over_glass.clone(),
                    ..Pass::depth_tested(
                        "varde sketch points, over glass",
                        "vs_point",
                        "fs_point",
                        std::slice::from_ref(&sketch_points),
                    )
                }),
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
                    layout: Some(&errors_layout),
                    ..Pass::overlay(
                        "varde error halo composite",
                        "vs_fullscreen",
                        "fs_error_halo",
                    )
                }),
                core: [seen, hidden].map(|depth_compare| {
                    let core_pass = |label, vs, fs, buffers| Pass {
                        label,
                        buffers,
                        depth_compare,
                        layout: Some(&errors_layout),
                        ..Pass::overlay(label, vs, fs)
                    };
                    let core = |label, vs, fs, buffers| pipeline(core_pass(label, vs, fs, buffers));
                    // Hidden, patches overlapping would blend twice and
                    // darken: only the first drawn at a pixel is, marked
                    // in the stencil, cleared to 0, the reference.
                    let stencil = if depth_compare == hidden {
                        marked(
                            0xff,
                            wgpu::CompareFunction::Equal,
                            wgpu::StencilOperation::IncrementClamp,
                        )
                    } else {
                        NO_STENCIL
                    };
                    ErrorLayer {
                        faces: pipeline(Pass {
                            stencil,
                            ..core_pass(
                                "varde error faces",
                                "vs_error_face",
                                "fs_error_face",
                                &faces,
                            )
                        }),
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
                layout: errors_group,
            },
            bind_group_layout,
            samples,
            composite,
            composite_layout,
            alphas,
            tint_stride,
            depth_format,
            format,
        }
    }
}

/// A uniform buffer at `binding` of at least `size` bytes, at a dynamic
/// offset if `dynamic`, seen by `visibility`.
fn uniform_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    dynamic: bool,
    size: u64,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    }
}
