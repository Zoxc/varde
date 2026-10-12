//! The depth, multisample and halo coverage targets a frame draws into.

use super::*;

/// The depth buffer's format on `device`, with a stencil: 32 bit float
/// depth if it has that, else 24 bits. The stencil marks which part less
/// than opaque is nearest at a pixel, and where a hidden error patch is
/// drawn already.
pub(super) fn depth_format(device: &wgpu::Device) -> wgpu::TextureFormat {
    if device
        .features()
        .contains(wgpu::Features::DEPTH32FLOAT_STENCIL8)
    {
        wgpu::TextureFormat::Depth32FloatStencil8
    } else {
        wgpu::TextureFormat::Depth24PlusStencil8
    }
}

/// How many samples a pixel of the scene takes: 4 where every format it
/// draws to is sure to take them (WebGL2 included), else 1. Faces have no
/// anti-aliasing of their own, and where no edge line is drawn over their
/// silhouette it would show stairs without.
pub(super) fn sample_count(device: &wgpu::Device, formats: &[wgpu::TextureFormat]) -> u32 {
    let four = formats.iter().all(|format| {
        let features = format.guaranteed_format_features(device.features());
        features.flags.sample_count_supported(4)
            && (format.is_depth_stencil_format()
                || features
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
    });
    if four { 4 } else { 1 }
}

/// The format of the target the errors' halo is drawn into as coverage.
pub(super) const HALO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// A slot's targets, the size of the frame: the depth, the colour the
/// scene is drawn into, multisampled when [`Renderer::samples`] is above
/// 1, and what that resolves to, single sampled, which is composited over
/// the frame's target last.
pub(super) struct DepthTarget {
    pub(super) view: wgpu::TextureView,
    /// The multisampled colour, resolving to `resolved`; `None` drawing
    /// into `resolved` itself.
    pub(super) color: Option<wgpu::TextureView>,
    pub(super) resolved: wgpu::TextureView,
    /// Group 0 of [`Renderer::composite`]: `resolved`.
    pub(super) composite: wgpu::BindGroup,
    pub(super) size: [u32; 2],
}

impl DepthTarget {
    /// The colour target drawn into and what it resolves to, if anything.
    pub(super) fn color(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        match &self.color {
            Some(color) => (color, Some(&self.resolved)),
            None => (&self.resolved, None),
        }
    }
}

/// A target of the frame's size drawn into beside the scene and read
/// back over it: the errors' halo's (see [`HALO_FORMAT`]), with their
/// colours, or the selected faces' pattern's mask (the colours unused).
pub(super) struct CoverageTarget {
    pub(super) view: wgpu::TextureView,
    /// The multisampled target, resolving to `view`, as the scene's colour.
    pub(super) samples: Option<wgpu::TextureView>,
    /// [`ErrorUniforms`], written each frame there are errors.
    pub(super) uniforms: wgpu::Buffer,
    /// Group 0 of the composite and the core: the scene's uniforms, the
    /// colours and the halo.
    pub(super) group: wgpu::BindGroup,
    pub(super) size: [u32; 2],
}

impl CoverageTarget {
    pub(super) fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        scene: &wgpu::Buffer,
        (label, format): (&str, wgpu::TextureFormat),
        size: [u32; 2],
        samples: u32,
    ) -> CoverageTarget {
        let [width, height] = size;
        let texture = |label, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let view = texture(label, 1, attachment | wgpu::TextureUsages::TEXTURE_BINDING);
        let samples = (samples > 1).then(|| texture(label, samples, attachment));
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde error colours"),
            size: size_of::<ErrorUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde errors"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        CoverageTarget {
            view,
            samples,
            uniforms,
            group,
            size,
        }
    }
}

impl CoverageTarget {
    /// The halo's target drawn into and what it resolves to, if anything.
    pub(super) fn color(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        match &self.samples {
            Some(samples) => (samples, Some(&self.view)),
            None => (&self.view, None),
        }
    }
}

impl Renderer {
    /// A slot's targets of `size`: see [`DepthTarget`].
    pub(super) fn create_targets(&self, device: &wgpu::Device, size: [u32; 2]) -> DepthTarget {
        let [width, height] = size;
        let texture = |label, samples, format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let view = texture("varde depth", self.samples, self.depth_format, attachment);
        let color = (self.samples > 1)
            .then(|| texture("varde scene samples", self.samples, self.format, attachment));
        let resolved = texture(
            "varde scene",
            1,
            self.format,
            attachment | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde composite"),
            layout: &self.composite_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&resolved),
            }],
        });
        DepthTarget {
            view,
            color,
            resolved,
            composite,
            size,
        }
    }
}
