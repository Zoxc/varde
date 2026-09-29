//! Renders into an offscreen texture and checks the scene stays inside its viewport.

use std::sync::Arc;

use glam::Vec3;
use varde_kernel::{RenderMesh, Shape};
use varde_render::{Camera, ClipRect, Colors, Frame, Renderer, Srgb, View, Viewport, wgpu};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// A black background and a grey model, with the app's scene colours
/// otherwise.
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
};
const SIZE: [u32; 2] = [512, 256];
const SENTINEL: [u8; 4] = [255, 0, 255, 255];

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// Renders `mesh` into `viewport` and returns RGBA8 pixels, row-major.
fn render(
    camera: &Camera,
    mesh: &RenderMesh,
    viewport: Viewport,
    clip: ClipRect,
    scale_factor: f32,
) -> Option<Vec<[u8; 4]>> {
    render_to(FORMAT, camera, mesh, viewport, clip, scale_factor)
}

/// Like [`render`], into a target of the given RGBA8 format, returning the
/// stored bytes.
fn render_to(
    format: wgpu::TextureFormat,
    camera: &Camera,
    mesh: &RenderMesh,
    viewport: Viewport,
    clip: ClipRect,
    scale_factor: f32,
) -> Option<Vec<[u8; 4]>> {
    let (device, queue) = device()?;
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
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

    let renderer = Renderer::new(&device, format);
    let mut slot = renderer.slot(&device);
    renderer
        .prepare(
            &mut slot,
            &device,
            &queue,
            &Frame {
                camera,
                mesh: &Arc::new(mesh.clone()),
                viewport,
                target_size: SIZE,
                scale_factor,
                colors: COLORS,
            },
        )
        .unwrap();

    let mut encoder = device.create_command_encoder(&Default::default());
    let [r, g, b, a] = SENTINEL.map(|c| c as f64 / 255.0);
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    renderer.render(&slot, &mut encoder, &view, clip);

    let bytes_per_row = width * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (bytes_per_row * height) as u64,
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
    Some(bytemuck_pixels(&data))
}

fn bytemuck_pixels(data: &[u8]) -> Vec<[u8; 4]> {
    data.as_chunks::<4>().0.to_vec()
}

fn pixel(pixels: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    pixels[(y * SIZE[0] + x) as usize]
}

/// A viewport in the right half, offset from the top.
const VIEWPORT: Viewport = Viewport {
    x: 128.0,
    y: 32.0,
    width: 96.0,
    height: 64.0,
};
const CLIP: ClipRect = ClipRect {
    x: 128,
    y: 32,
    width: 96,
    height: 64,
};

#[test]
fn draws_only_inside_viewport() {
    let Some(pixels) = render(
        &Camera::default(),
        &RenderMesh::default(),
        VIEWPORT,
        CLIP,
        1.0,
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    for y in 0..SIZE[1] {
        for x in 0..SIZE[0] {
            let inside = (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y);
            let touched = pixel(&pixels, x, y) != SENTINEL;
            assert_eq!(inside, touched, "pixel ({x}, {y})");
        }
    }
}

#[test]
fn origin_is_centered_in_viewport() {
    // The default camera orbits the origin.
    let Some(pixels) = render(
        &Camera::default(),
        &RenderMesh::default(),
        VIEWPORT,
        CLIP,
        1.0,
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    // The origin dot is white; find the centroid of white pixels.
    let (mut sx, mut sy, mut n) = (0u64, 0u64, 0u64);
    for y in 0..SIZE[1] {
        for x in 0..SIZE[0] {
            if pixel(&pixels, x, y)[..3].iter().all(|&c| c > 240) {
                sx += x as u64;
                sy += y as u64;
                n += 1;
            }
        }
    }
    assert!(n > 0, "origin dot not drawn");
    let (cx, cy) = (sx as f32 / n as f32, sy as f32 / n as f32);
    let expected = (
        VIEWPORT.x + VIEWPORT.width / 2.0,
        VIEWPORT.y + VIEWPORT.height / 2.0,
    );
    assert!(
        (cx - expected.0).abs() < 1.5 && (cy - expected.1).abs() < 1.5,
        "origin at ({cx}, {cy}), expected {expected:?}"
    );
}

#[test]
fn draws_bodies_far_along_the_view_axis() {
    // Far behind the target, but on screen in an orthographic front view.
    let mut camera = Camera::default();
    camera.look_from(View::Front);
    let mut mesh = RenderMesh::default();
    mesh.append_at(
        &Shape::cuboid(Vec3::splat(2.0))
            .build()
            .unwrap()
            .tessellate(),
        Vec3::new(0.0, 1000.0, 0.0),
    )
    .unwrap();
    let Some(pixels) = render(&camera, &mesh, VIEWPORT, CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    // Shading lights the model; the background is black.
    let lit = pixel(&pixels, x0 + w / 2, y0 + h / 2);
    assert!(lit[..3].iter().all(|&c| c > 64), "centre is {lit:?}");
}

#[test]
fn edges_stay_in_front_of_faces_zoomed_into_a_large_scene() {
    // Zoomed far into the top front edge of a unit cube, with a second cube
    // far behind it stretching the depth range, so pulling edges towards the
    // camera by a fraction of the view height falls below depth precision.
    let mut camera = Camera::default();
    camera.set_target(Vec3::new(0.5, 0.0, 1.0));
    camera.zoom(0.01 / camera.view_height());
    let cube = Shape::cuboid(Vec3::ONE).build().unwrap().tessellate();
    let mut mesh = RenderMesh::default();
    mesh.append_at(&cube, Vec3::ZERO).unwrap();
    mesh.append_at(&cube, Vec3::Y * 1e5).unwrap();
    let Some(pixels) = render(&camera, &mesh, VIEWPORT, CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    // The edge crosses every column. Faces are lit and edges are dark.
    for x in x0..x0 + w {
        let dark = (y0..y0 + h).any(|y| pixel(&pixels, x, y)[..3].iter().all(|&c| c < 64));
        assert!(dark, "no edge in column {x}");
    }
}

#[test]
fn axes_stay_put_zoomed_into_a_large_scene() {
    // Zoomed far into the origin, with a cube at the edge of the document
    // limit stretching the depth range, so unprojecting the near and far planes
    // to find the ground loses pixels to rounding.
    let mut camera = Camera::default();
    camera.zoom(0.01 / camera.view_height());
    let mut mesh = RenderMesh::default();
    mesh.append_at(
        &Shape::cuboid(Vec3::ONE).build().unwrap().tessellate(),
        Vec3::Y * varde_kernel::MAX_COORD,
    )
    .unwrap();
    let Some(pixels) = render(&camera, &mesh, VIEWPORT, CLIP, 1.0) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let center = glam::Vec2::new(
        VIEWPORT.x + VIEWPORT.width / 2.0,
        VIEWPORT.y + VIEWPORT.height / 2.0,
    );
    // The X axis is red and the Y axis green, each a line through the origin
    // dot at the centre, along the axis as seen on screen.
    let red: fn([u8; 4]) -> bool = |[r, g, _, _]| r > 128 && g < 128;
    let green: fn([u8; 4]) -> bool = |[r, g, _, _]| g > 128 && r < 128;
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    for (axis, colored) in [(Vec3::X, red), (Vec3::Y, green)] {
        let along = glam::Vec2::new(camera.right().dot(axis), -camera.up().dot(axis)).normalize();
        let mut drawn = 0;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if !colored(pixel(&pixels, x, y)) {
                    continue;
                }
                let offset = glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - center;
                let off = offset.perp_dot(along).abs();
                assert!(off < 2.0, "{axis} axis at ({x}, {y}) is {off} pixels off");
                drawn += 1;
            }
        }
        assert!(drawn > 100, "{axis} axis has only {drawn} pixels");
    }
}

#[test]
fn origin_marker_follows_scale_factor() {
    // The same logical viewport at 1x and 2x: the marker should cover twice
    // the physical pixels in each direction at 2x.
    let [width, height] = SIZE;
    let measure = |scale: f32| {
        let size = [width as f32 / 2.0 * scale, height as f32 / 2.0 * scale];
        let viewport = Viewport {
            x: 0.0,
            y: 0.0,
            width: size[0],
            height: size[1],
        };
        let clip = ClipRect {
            x: 0,
            y: 0,
            width: size[0] as u32,
            height: size[1] as u32,
        };
        let pixels = render(
            &Camera::default(),
            &RenderMesh::default(),
            viewport,
            clip,
            scale,
        )?;
        let center_y = size[1] / 2.0;
        // The dot is white and the Z axis, pointing up on screen, is blue.
        let (mut dot, mut axis) = (0, 0.0f32);
        for y in 0..clip.height {
            for x in 0..clip.width {
                let [r, g, b, _] = pixel(&pixels, x, y).map(i32::from);
                if [r, g, b].iter().all(|&c| c > 240) {
                    dot += 1;
                } else if b > 150 && b > r + 80 && b > g + 60 {
                    axis = axis.max(center_y - y as f32);
                }
            }
        }
        Some((dot as f32, axis))
    };
    let (Some((dot1, axis1)), Some((dot2, axis2))) = (measure(1.0), measure(2.0)) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    assert!(dot1 > 0.0 && axis1 > 20.0, "dot {dot1}, axis {axis1}");
    let (dot, axis) = (dot2 / dot1, axis2 / axis1);
    assert!((3.0..5.0).contains(&dot), "dot area grew {dot}x");
    assert!((1.8..2.2).contains(&axis), "axis length grew {axis}x");
}

#[test]
fn looks_the_same_on_srgb_and_linear_targets() {
    // A face of a cube filling the centre of the viewport, which the grid
    // is hidden behind. Partly covered grid lines and edges may differ, since the
    // linear target blends encoded values.
    let mut mesh = RenderMesh::default();
    mesh.append_at(
        &Shape::cuboid(Vec3::splat(2.0))
            .build()
            .unwrap()
            .tessellate(),
        Vec3::ZERO,
    )
    .unwrap();
    let mut camera = Camera::default();
    camera.set_target(Vec3::ONE);
    camera.look_from(View::Front);
    let render = |format| render_to(format, &camera, &mesh, VIEWPORT, CLIP, 1.0);
    let (Some(unorm), Some(srgb)) = (
        render(wgpu::TextureFormat::Rgba8Unorm),
        render(wgpu::TextureFormat::Rgba8UnormSrgb),
    ) else {
        eprintln!("no GPU adapter, skipping");
        return;
    };
    let ClipRect {
        x: x0,
        y: y0,
        width: w,
        height: h,
    } = CLIP;
    let (cx, cy) = (x0 + w / 2, y0 + h / 2);
    for (x, y) in (cx - 4..=cx + 4).flat_map(|x| (cy - 4..=cy + 4).map(move |y| (x, y))) {
        let (a, b) = (pixel(&unorm, x, y), pixel(&srgb, x, y));
        let diff = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max();
        assert!(diff <= Some(1), "pixel ({x}, {y}) is {a:?} and {b:?}");
    }
    let lit = pixel(&srgb, cx, cy);
    assert!(lit[..3].iter().all(|&c| c > 64), "centre is {lit:?}");
}
