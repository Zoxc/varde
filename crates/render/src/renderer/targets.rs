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

/// A 2D texture's view on `device` of `size`, `samples` a pixel.
fn texture_view(
    device: &wgpu::Device,
    label: &str,
    [width, height]: [u32; 2],
    samples: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureView {
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
}

/// A colour target drawn into, multisampled when there's more than one
/// sample, and what that resolves to, single sampled and bindable to be
/// read back.
pub(super) struct ColorTarget {
    /// The multisampled target, resolving to `resolved`; `None` drawing
    /// into `resolved` itself.
    samples: Option<wgpu::TextureView>,
    resolved: wgpu::TextureView,
}

impl ColorTarget {
    fn new(
        device: &wgpu::Device,
        [label, samples_label]: [&str; 2],
        format: wgpu::TextureFormat,
        size: [u32; 2],
        samples: u32,
    ) -> ColorTarget {
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let resolved = texture_view(
            device,
            label,
            size,
            1,
            format,
            attachment | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let samples = (samples > 1)
            .then(|| texture_view(device, samples_label, size, samples, format, attachment));
        ColorTarget { samples, resolved }
    }

    /// The target drawn into and what it resolves to, if anything.
    pub(super) fn color(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        match &self.samples {
            Some(samples) => (samples, Some(&self.resolved)),
            None => (&self.resolved, None),
        }
    }
}

/// A slot's targets, the size of the frame: the depth, the colour the
/// scene is drawn into, multisampled when [`Renderer::samples`] is above
/// 1, and what that resolves to, single sampled, which is composited over
/// the frame's target last.
pub(super) struct SlotTargets {
    pub(super) depth: wgpu::TextureView,
    color: ColorTarget,
    /// Group 0 of [`Renderer::composite`]: the colour, resolved.
    pub(super) composite: wgpu::BindGroup,
    pub(super) size: [u32; 2],
}

impl SlotTargets {
    /// The colour target drawn into and what it resolves to, if anything.
    pub(super) fn color(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        self.color.color()
    }
}

/// A target of the frame's size drawn into beside the scene and read
/// back over it: the errors' halo's (see [`HALO_FORMAT`]), with their
/// colours, or the selected faces' pattern's mask (the colours unused).
/// Multisampled as the scene's colour.
pub(super) struct CoverageTarget {
    target: ColorTarget,
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
        let samples_label = format!("{label} samples");
        let target = ColorTarget::new(device, [label, &samples_label], format, size, samples);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("varde error colours"),
            size: size_of::<ErrorUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
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
                    resource: wgpu::BindingResource::TextureView(&target.resolved),
                },
            ],
        });
        CoverageTarget {
            target,
            uniforms,
            group,
            size,
        }
    }

    /// The target drawn into and what it resolves to, if anything.
    pub(super) fn color(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        self.target.color()
    }
}

impl Renderer {
    /// A slot's targets of `size`: see [`SlotTargets`].
    pub(super) fn create_targets(&self, device: &wgpu::Device, size: [u32; 2]) -> SlotTargets {
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let depth = texture_view(
            device,
            "varde depth",
            size,
            self.samples,
            self.depth_format,
            attachment,
        );
        let labels = ["varde scene", "varde scene samples"];
        let color = ColorTarget::new(device, labels, self.format, size, self.samples);
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("varde composite"),
            layout: &self.composite_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&color.resolved),
            }],
        });
        SlotTargets {
            depth,
            color,
            composite,
            size,
        }
    }
}
