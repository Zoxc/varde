//! A design's preview: its model and finished sketches alone, drawn offscreen on nothing
//! (transparent), framed to fit, in one or more sets of colours (a theme's
//! each), and read back as straight alpha sRGB RGBA, for a thumbnail such
//! as the welcome screen's.

use std::sync::Arc;

use glam::Vec3;
use varde_kernel_compat::{RenderLines, RenderMesh};

use crate::renderer::{linear_to_srgb, srgb_to_linear};
use crate::{
    BodyTint, Camera, ClipRect, Colors, Frame, GridPlane, Highlights, PrepareError, Projection,
    Renderer, Shading, Viewport,
};

/// Where a preview looks from and how large it is: see [`frame`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewShot {
    /// Framing the model, orthographic or in perspective as [`frame`] was
    /// asked.
    pub camera: Camera,
    /// The image's width and height, in pixels.
    pub size: [u32; 2],
}

impl PreviewShot {
    /// Closes in on the framing's middle by `factor`, the image's size and
    /// the camera's place kept: the view is narrowed ([`Camera::narrow`]),
    /// never moved, so in perspective the eye stays outside what was
    /// framed. Below 1 it shows more around it.
    pub fn zoom(&mut self, factor: f32) {
        self.camera.narrow(factor);
    }
}

/// A preview as read back: `rgba` holds `width` by `height` pixels, rows
/// top down, each red, green, blue and alpha, the colour sRGB encoded and
/// not premultiplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Why [`render_preview`] couldn't draw.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreviewError {
    /// The renderer draws to a format that isn't 8 bit RGBA or BGRA, which
    /// isn't read back.
    #[error("a target of {0:?} isn't read back")]
    Format(wgpu::TextureFormat),
    /// The image is larger than the device's textures may be.
    #[error("a {}×{} image is larger than the device's {limit} pixels", size[0], size[1])]
    TooLarge { size: [u32; 2], limit: u32 },
    /// The mesh didn't fit the device's buffers.
    #[error("{0}")]
    Prepare(#[source] PrepareError),
}

/// The shot of `mesh` and `sketches` from `from`'s direction, framing
/// them as large as fits within `max` pixels less `margin` on each side,
/// and cropped to it: the image is only as wide or tall as the model is,
/// plus the margins, so it has no empty space but them. With `parts`,
/// only those of `mesh` (by their place in [`RenderMesh::parts`]; any it
/// hasn't are left out) count, by the positions their faces' triangles
/// use, so the other parts may be left out of the picture (drawn at
/// opacity 0, see [`Frame::opacity`]). Orthographic, or perspective if
/// `from` is: then the sphere about the bounds is fitted to the field of
/// view and the image cropped to what it shows of them. `None` with
/// nothing to frame (no vertices and no sketch lines), or `max` with no
/// room inside the margins.
pub fn frame(
    mesh: &RenderMesh,
    sketches: &RenderLines,
    parts: Option<&[usize]>,
    from: &Camera,
    max: [u32; 2],
    margin: u32,
) -> Option<PreviewShot> {
    let positions = mesh.positions();
    let used: Box<dyn Iterator<Item = &[f32; 3]>> = match parts {
        None => Box::new(positions.iter()),
        Some(parts) => {
            let all: Vec<_> = mesh.parts().collect();
            let indices = (parts.iter())
                .filter_map(move |&part| all.get(part).map(|part| part.indices.clone()))
                .flat_map(|range| mesh.indices().get(range).unwrap_or_default());
            Box::new(indices.filter_map(|&index| positions.get(index as usize)))
        }
    };
    let points = used.chain(sketches.points()).map(|&p| Vec3::from(p));
    match from.projection() {
        Projection::Orthographic => frame_orthographic(points, from, max, margin),
        Projection::Perspective => frame_perspective(points, from, max, margin),
    }
}

/// The room inside `margin` on each side of `max`, if there's some.
fn room(max: [u32; 2], margin: u32) -> Option<[u32; 2]> {
    let room = max.map(|side| side.checked_sub(margin.checked_mul(2)?).filter(|&r| r > 0));
    match room {
        [Some(x), Some(y)] => Some([x, y]),
        _ => None,
    }
}

/// [`frame`] of `points`, orthographic.
fn frame_orthographic(
    points: impl Iterator<Item = Vec3>,
    from: &Camera,
    max: [u32; 2],
    margin: u32,
) -> Option<PreviewShot> {
    let [room_x, room_y] = room(max, margin)?;
    let (right, up, backward) = (from.right(), from.up(), from.backward());
    let mut low = Vec3::INFINITY;
    let mut high = Vec3::NEG_INFINITY;
    for p in points {
        let seen = Vec3::new(p.dot(right), p.dot(up), p.dot(backward));
        low = low.min(seen);
        high = high.max(seen);
    }
    if !(low.is_finite() && high.is_finite()) {
        return None;
    }
    // A flat model seen edge on has no width or height: it's drawn as its
    // edges, a line across the middle.
    let extent = (high - low).max(Vec3::splat(1e-6));
    // Pixels per world unit, as large as both fit.
    let scale = (room_x as f32 / extent.x).min(room_y as f32 / extent.y);
    let fit = |extent: f32, room: u32| ((extent * scale).ceil() as u32).clamp(1, room);
    let size = [
        fit(extent.x, room_x) + 2 * margin,
        fit(extent.y, room_y) + 2 * margin,
    ];
    let middle = (low + high) * 0.5;
    let mut camera = *from;
    camera.set_projection(Projection::Orthographic);
    camera.set_target(right * middle.x + up * middle.y + backward * middle.z);
    camera.set_view_height(size[1] as f32 / scale);
    Some(PreviewShot { camera, size })
}

/// [`frame`] of `points` in perspective: the sphere about their
/// bounds fitted to the field of view inside the margins, the image as
/// tall as `max` lets and cropped across to what the points take of it.
fn frame_perspective(
    points: impl Iterator<Item = Vec3>,
    from: &Camera,
    max: [u32; 2],
    margin: u32,
) -> Option<PreviewShot> {
    let [room_x, room_y] = room(max, margin)?;
    let points: Vec<Vec3> = points.collect();
    let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for &p in &points {
        low = low.min(p);
        high = high.max(p);
    }
    if !(low.is_finite() && high.is_finite()) {
        return None;
    }
    let center = (low + high) * 0.5;
    let radius = (points.iter())
        .map(|p| p.distance(center))
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let mut camera = *from;
    camera.set_projection(Projection::Perspective);
    camera.set_target(center);
    // The field of view's half tangent, up and down, at any distance.
    let half = camera.view_height() / (2.0 * camera.distance());
    // The tangents across and up of where the points show, from `camera`.
    let (right, up, backward) = (camera.right(), camera.up(), camera.backward());
    let spread = |camera: &Camera| {
        let mut most = [0.0f32; 2];
        for &p in &points {
            let offset = p - camera.target();
            let depth = camera.distance() - offset.dot(backward);
            if depth > 0.0 {
                most[0] = most[0].max((offset.dot(right) / depth).abs());
                most[1] = most[1].max((offset.dot(up) / depth).abs());
            }
        }
        most
    };
    // The sphere within the field of view inside the margins: the
    // image's full height is the field of view, so the sphere takes all
    // of it but them, and the image is as wide as the points then show.
    let fit = |camera: &mut Camera, height: u32| {
        let inside = half * (height - 2 * margin) as f32 / height as f32;
        camera.set_view_height(2.0 * (radius / inside.atan().sin()) * half);
        let [across, _] = spread(camera);
        let per_tangent = height as f32 / (2.0 * half);
        let width = (2.0 * across * per_tangent).ceil().max(1.0);
        // Within `u32`, as `room_x` is: past it, too wide anyway.
        (width.min(room_x as f32 + 1.0) as u32).saturating_add(2 * margin)
    };
    // The tallest image whose width fits: the width grows with the
    // height, as the margins take less of it, so it's found by halving.
    let (mut low, mut high) = (2 * margin + 1, room_y + 2 * margin);
    while low < high {
        let height = low + (high - low).div_ceil(2);
        if fit(&mut camera, height) <= max[0] {
            low = height;
        } else {
            high = height - 1;
        }
    }
    let height = low;
    let width = fit(&mut camera, height).min(max[0]);
    Some(PreviewShot {
        camera,
        size: [width, height],
    })
}

/// What a preview draws as selected, as the viewport draws a selection:
/// faces by their ids in the mesh (see [`Frame::selected_faces`]) and
/// the edges and vertices in `highlights`.
#[derive(Debug, Clone, Copy)]
pub struct PreviewSelection<'a> {
    pub faces: &'a [u32],
    pub highlights: &'a Arc<Highlights>,
}

/// Draws `mesh` and the finished `sketches` as `shot` frames them, its parts as opaque as `opacity` says
/// (see [`Frame::opacity`]) and in the colours `tints` gives them (see
/// [`Frame::tints`]), once in each of `colors`, with nothing behind
/// it: no background, grid, sketch being edited or markers, nothing hovered or
/// selected but `selection`, if given. Lines are as wide as `scale_factor` physical pixels to a
/// logical one make them.
///
/// The images are read back once the GPU is done: `done` is called with
/// them, one for each of `colors` in their order, or with `None` should
/// mapping them fail, on the thread that polls the device then. Natively
/// this waits for the GPU before it returns, so unless another thread
/// polls the device too, `done` has been called by then; on the web the
/// browser maps them later, on a later submit of the device's queue. On
/// an error returned, nothing is drawn and `done` is dropped uncalled.
#[expect(clippy::too_many_arguments, reason = "one frame's worth, as `Frame`")]
pub fn render_preview(
    renderer: &Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mesh: &Arc<RenderMesh>,
    sketches: &Arc<RenderLines>,
    opacity: &[f32],
    tints: &[Option<BodyTint>],
    shot: &PreviewShot,
    selection: Option<PreviewSelection<'_>>,
    colors: &[Colors],
    scale_factor: f32,
    done: impl FnOnce(Option<Vec<PreviewImage>>) + Send + 'static,
) -> Result<(), PreviewError> {
    let format = renderer.format();
    let Some(layout) = Layout::of(format) else {
        return Err(PreviewError::Format(format));
    };
    let [width, height] = shot.size;
    let limit = device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > limit || height > limit {
        return Err(PreviewError::TooLarge {
            size: shot.size,
            limit,
        });
    }
    // At most 4 × 2^16 bytes a row by the texture limit, well within u32;
    // the images follow one another in the buffer, each a multiple of the
    // row alignment, as the copies' offsets must be.
    let row = width * 4;
    let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let image_bytes = u64::from(padded) * u64::from(height);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("varde preview, read back"),
        size: image_bytes * colors.len().max(1) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("varde preview"),
    });
    let none = Arc::new(Highlights::default());
    for (i, &colors) in colors.iter().enumerate() {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("varde preview"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());

        // A slot each, as the uniforms are written before the one submit.
        let mut slot = renderer.slot(device);
        let frame = Frame {
            camera: &shot.camera,
            mesh,
            opacity,
            tints,
            sketches,
            grid: GridPlane::XY,
            fade: 0.0,
            fading_grid: None,
            wireframe: false,
            tessellation: false,
            shading: Shading::Regular,
            hidden_edges: false,
            hovered_faces: &[],
            selected_faces: selection.map_or(&[], |selection| selection.faces),
            second_faces: &[],
            hover_through: false,
            highlights: selection.map_or(&none, |selection| selection.highlights),
            errors: &[],
            sketch: None,
            pivot: None,
            origin: crate::OriginShown::NONE,
            viewport: Viewport {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
            target_size: shot.size,
            scale_factor,
            colors,
        };
        renderer
            .prepare(&mut slot, device, queue, &frame)
            .map_err(PreviewError::Prepare)?;

        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("varde preview, cleared"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        let clip = ClipRect {
            x: 0,
            y: 0,
            width,
            height,
        };
        renderer.record(&slot, &mut encoder, &view, clip, false);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: image_bytes * i as u64,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
    }
    queue.submit([encoder.finish()]);

    let count = colors.len();
    let mapped = buffer.clone();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let images = result.ok().map(|()| {
                let data = mapped.slice(..).get_mapped_range();
                (data.chunks_exact(image_bytes as usize).take(count))
                    .map(|image| {
                        let mut rgba = Vec::with_capacity(row as usize * height as usize);
                        for line in image.chunks_exact(padded as usize) {
                            for pixel in line[..row as usize].as_chunks::<4>().0 {
                                rgba.extend(layout.straight(*pixel));
                            }
                        }
                        PreviewImage {
                            width,
                            height,
                            rgba,
                        }
                    })
                    .collect()
            });
            done(images);
        });
    // Natively the GPU is waited for here, a small image's worth; the
    // browser can't wait, and maps the buffer on a later submit.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    Ok(())
}

/// How a target format's pixels are laid out, of those read back.
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// Blue first, as `Bgra8`.
    bgra: bool,
    /// Stored linear and blended so, encoded by the format: else the shader
    /// encodes and blending is in sRGB.
    srgb: bool,
}

impl Layout {
    fn of(format: wgpu::TextureFormat) -> Option<Layout> {
        use wgpu::TextureFormat as F;
        let (bgra, srgb) = match format {
            F::Rgba8Unorm => (false, false),
            F::Rgba8UnormSrgb => (false, true),
            F::Bgra8Unorm => (true, false),
            F::Bgra8UnormSrgb => (true, true),
            _ => return None,
        };
        Some(Layout { bgra, srgb })
    }

    /// `pixel` as stored, drawn over transparent black so premultiplied
    /// by its alpha, as straight alpha sRGB RGBA. Blending was in the
    /// space the target stores, so the alpha is divided out there.
    fn straight(self, pixel: [u8; 4]) -> [u8; 4] {
        let [r, g, b, a] = pixel;
        let [r, g, b] = if self.bgra { [b, g, r] } else { [r, g, b] };
        match a {
            0 => return [0; 4],
            255 => return [r, g, b, a],
            _ => {}
        }
        let alpha = f32::from(a) / 255.0;
        let unblend = |c: u8| {
            let c = f32::from(c) / 255.0;
            let c = if self.srgb {
                linear_to_srgb(srgb_to_linear(c) / alpha)
            } else {
                c / alpha
            };
            (c.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        [unblend(r), unblend(g), unblend(b), a]
    }
}

#[cfg(test)]
mod tests;
