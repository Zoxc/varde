use glam::{DVec3, Vec2};
use varde_kernel_compat::{Display, Solid, Tolerance};

use super::*;

/// A box `size` on its sides from the origin, tessellated.
fn block(size: DVec3) -> RenderMesh {
    let solid = Solid::cuboid(DVec3::ZERO, size, 0, &Tolerance::DEFAULT).unwrap();
    solid.tessellate(&Display::default()).unwrap()
}

/// Where `point` shows in `shot`'s image, in pixels from its top left.
fn shown(shot: &PreviewShot, point: Vec3) -> [f32; 2] {
    let camera = &shot.camera;
    let [width, height] = shot.size.map(|side| side as f32);
    let per_pixel = camera.view_height() / height;
    let offset = point - camera.target();
    [
        width / 2.0 + offset.dot(camera.right()) / per_pixel,
        height / 2.0 - offset.dot(camera.up()) / per_pixel,
    ]
}

#[test]
fn the_model_fills_the_image_within_its_margins() {
    let mesh = block(DVec3::new(4.0, 1.0, 1.0));
    let shot = frame(
        &mesh,
        &RenderLines::default(),
        None,
        &Camera::default(),
        [400, 300],
        5,
    )
    .unwrap();
    assert_eq!(shot.camera.projection(), Projection::Orthographic);
    assert!(
        shot.size[0] <= 400 && shot.size[1] <= 300,
        "{:?}",
        shot.size
    );
    // The long box is wider than tall from the home view: the width is
    // used up, the height cropped to it.
    assert_eq!(shot.size[0], 400);
    assert!(shot.size[1] < 300, "{:?}", shot.size);
    let (mut low, mut high) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for &p in mesh.positions() {
        let at = shown(&shot, Vec3::from(p));
        for axis in 0..2 {
            low[axis] = low[axis].min(at[axis]);
            high[axis] = high[axis].max(at[axis]);
        }
    }
    // Touching the margins across the width, and within a pixel of them
    // down the height, which was rounded up.
    assert!((low[0] - 5.0).abs() < 0.01 && (high[0] - 395.0).abs() < 0.01);
    let bottom = shot.size[1] as f32 - 5.0;
    assert!(low[1] >= 4.99 && low[1] < 6.0, "{low:?}");
    assert!(
        high[1] <= bottom + 0.01 && high[1] > bottom - 1.0,
        "{high:?}"
    );
    // Looking as the home camera does.
    let home = Camera::default();
    assert!(shot.camera.backward().distance(home.backward()) < 1e-6);
}

#[test]
fn a_tall_model_crops_the_width() {
    let mesh = block(DVec3::new(1.0, 1.0, 6.0));
    let shot = frame(
        &mesh,
        &RenderLines::default(),
        None,
        &Camera::default(),
        [400, 300],
        4,
    )
    .unwrap();
    assert_eq!(shot.size[1], 300);
    assert!(shot.size[0] < 200, "{:?}", shot.size);
}

#[test]
fn sketch_lines_alone_are_framed() {
    let mut sketches = RenderLines::default();
    sketches
        .push([
            Vec3::ZERO,
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(3.0, 2.0, 0.0),
        ])
        .unwrap();
    let shot = frame(
        &RenderMesh::default(),
        &sketches,
        None,
        &Camera::default(),
        [400, 300],
        4,
    )
    .unwrap();
    for &p in sketches.points() {
        let [x, y] = shown(&shot, Vec3::from(p));
        assert!(x > 3.0 && x < shot.size[0] as f32 - 3.0, "{x} {shot:?}");
        assert!(y > 3.0 && y < shot.size[1] as f32 - 3.0, "{y} {shot:?}");
    }
    // A mesh's bounds and the lines' are framed together.
    let mesh = block(DVec3::ONE);
    let both = frame(&mesh, &sketches, None, &Camera::default(), [400, 300], 4).unwrap();
    let alone = frame(
        &mesh,
        &RenderLines::default(),
        None,
        &Camera::default(),
        [400, 300],
        4,
    )
    .unwrap();
    assert!(
        both.camera.view_height() / both.size[1] as f32
            > alone.camera.view_height() / alone.size[1] as f32
    );
}

#[test]
fn nothing_to_frame_has_no_shot() {
    let mesh = block(DVec3::ONE);
    assert_eq!(
        frame(
            &RenderMesh::default(),
            &RenderLines::default(),
            None,
            &Camera::default(),
            [400, 300],
            4
        ),
        None
    );
    // No room inside the margins.
    assert_eq!(
        frame(
            &mesh,
            &RenderLines::default(),
            None,
            &Camera::default(),
            [8, 300],
            4
        ),
        None
    );
    assert_eq!(
        frame(
            &mesh,
            &RenderLines::default(),
            None,
            &Camera::default(),
            [400, 300],
            u32::MAX
        ),
        None
    );
}

#[test]
fn pixels_read_back_lose_their_premultiplied_alpha() {
    let rgba = Layout::of(wgpu::TextureFormat::Rgba8Unorm).unwrap();
    let bgra = Layout::of(wgpu::TextureFormat::Bgra8Unorm).unwrap();
    let srgb = Layout::of(wgpu::TextureFormat::Rgba8UnormSrgb).unwrap();
    assert_eq!(rgba.straight([10, 20, 30, 255]), [10, 20, 30, 255]);
    assert_eq!(bgra.straight([10, 20, 30, 255]), [30, 20, 10, 255]);
    assert_eq!(rgba.straight([10, 20, 30, 0]), [0; 4]);
    // Half covered, blended in the space stored.
    assert_eq!(rgba.straight([50, 64, 128, 128]), [100, 128, 255, 128]);
    // White half covered, blended in linear light.
    let half = (linear_to_srgb(0.5) * 255.0).round() as u8;
    let white = srgb.straight([half, half, half, 128]);
    assert!(white[..3].iter().all(|&c| c >= 254), "{white:?}");
    assert_eq!(
        Layout::of(wgpu::TextureFormat::Rgba16Float).map(|_| ()),
        None
    );
}

/// Two blocks apart in one mesh: a unit cube at the origin, and a long
/// one well off along X.
fn two_blocks() -> RenderMesh {
    let mut mesh = block(DVec3::ONE);
    let far = block(DVec3::new(4.0, 1.0, 1.0));
    mesh.append_at(&far, Vec3::new(10.0, 0.0, 0.0)).unwrap();
    mesh
}

#[test]
fn parts_frame_only_their_own_positions() {
    let mesh = two_blocks();
    let lines = RenderLines::default();
    let first = frame(&mesh, &lines, Some(&[0]), &Camera::default(), [400, 300], 5).unwrap();
    let alone = frame(
        &block(DVec3::ONE),
        &lines,
        None,
        &Camera::default(),
        [400, 300],
        5,
    )
    .unwrap();
    assert_eq!(first.size, alone.size);
    assert!(first.camera.target().distance(alone.camera.target()) < 1e-4);
    assert!((first.camera.view_height() - alone.camera.view_height()).abs() < 1e-4);
    // Both as the whole mesh frames it.
    let both = frame(
        &mesh,
        &lines,
        Some(&[0, 1]),
        &Camera::default(),
        [400, 300],
        5,
    )
    .unwrap();
    let whole = frame(&mesh, &lines, None, &Camera::default(), [400, 300], 5).unwrap();
    assert_eq!(both, whole);
    // Parts the mesh hasn't are left out; none at all, nothing to frame.
    assert_eq!(
        frame(
            &mesh,
            &lines,
            Some(&[0, 7]),
            &Camera::default(),
            [400, 300],
            5
        ),
        Some(first)
    );
    assert_eq!(
        frame(&mesh, &lines, Some(&[7]), &Camera::default(), [400, 300], 5),
        None
    );
    assert_eq!(
        frame(&mesh, &lines, Some(&[]), &Camera::default(), [400, 300], 5),
        None
    );
}

/// Where `point` shows in perspective `shot`'s image, in pixels from its
/// top left.
fn shown_perspective(shot: &PreviewShot, point: Vec3) -> [f32; 2] {
    let camera = &shot.camera;
    let [width, height] = shot.size.map(|side| side as f32);
    let offset = point - camera.target();
    let depth = camera.distance() - offset.dot(camera.backward());
    let half = camera.view_height() / (2.0 * camera.distance());
    let per_tangent = height / (2.0 * half);
    [
        width / 2.0 + offset.dot(camera.right()) / depth * per_tangent,
        height / 2.0 - offset.dot(camera.up()) / depth * per_tangent,
    ]
}

#[test]
fn perspective_fits_the_bounds_sphere_within_the_margins() {
    let lines = RenderLines::default();
    let mesh = two_blocks();
    let mut from = Camera::default();
    from.set_projection(Projection::Perspective);
    let margin = 6;
    for parts in [&[0usize, 1][..], &[1]] {
        let shot = frame(&mesh, &lines, Some(parts), &from, [400, 300], margin).unwrap();
        assert_eq!(shot.camera.projection(), Projection::Perspective);
        assert!(shot.size[0] <= 400 && shot.size[1] <= 300, "{shot:?}");
        assert!(shot.camera.backward().distance(from.backward()) < 1e-6);
        let all: Vec<_> = mesh.parts().collect();
        let mut used = Vec::new();
        for &part in parts {
            for &i in &mesh.indices()[all[part].indices.clone()] {
                used.push(Vec3::from(mesh.positions()[i as usize]));
            }
        }
        let m = margin as f32 - 0.01;
        for &p in &used {
            let [x, y] = shown_perspective(&shot, p);
            assert!(x >= m && x <= shot.size[0] as f32 - m, "{x} {shot:?}");
            assert!(y >= m && y <= shot.size[1] as f32 - m, "{y} {shot:?}");
            // In front of the eye, past the near plane.
            let depth =
                shot.camera.distance() - (p - shot.camera.target()).dot(shot.camera.backward());
            assert!(depth > shot.camera.near());
        }
        // Cropped across: the points reach within a few pixels of the
        // margin either side.
        let xs: Vec<f32> = used
            .iter()
            .map(|&p| shown_perspective(&shot, p)[0])
            .collect();
        let reach = xs.iter().fold(0.0f32, |most, &x| {
            most.max((x - shot.size[0] as f32 / 2.0).abs())
        });
        assert!(
            reach > shot.size[0] as f32 / 2.0 - margin as f32 - 2.0,
            "{reach} {shot:?}"
        );
    }
    // The second block alone is framed apart from the first.
    let far = frame(&mesh, &lines, Some(&[1]), &from, [400, 300], margin).unwrap();
    assert!(far.camera.target().x > 9.0, "{far:?}");
}

/// Points that `shot` shows outside its margins, less half a pixel.
fn outside(shot: &PreviewShot, points: &[[f32; 3]], margin: u32) -> Vec<[f32; 2]> {
    let m = margin as f32 - 0.5;
    let [width, height] = shot.size.map(|side| side as f32);
    (points.iter())
        .map(|&p| match shot.camera.projection() {
            Projection::Perspective => shown_perspective(shot, Vec3::from(p)),
            Projection::Orthographic => shown(shot, Vec3::from(p)),
        })
        .filter(|&[x, y]| !(x >= m && x <= width - m && y >= m && y <= height - m))
        .collect()
}

#[test]
fn framing_keeps_the_model_inside_the_margins() {
    let lines = RenderLines::default();
    let sizes = [
        DVec3::new(1e-3, 2e-3, 1e-3),
        DVec3::new(20.0, 1.0, 1.0),
        DVec3::new(1.0, 1.0, 20.0),
        DVec3::new(1.0, 20.0, 0.01),
        DVec3::ONE,
    ];
    let views = [
        (0.0f32, 0.0f32),
        (-90.0, 0.0),
        (-60.0, 30.0),
        (10.0, 89.0),
        (45.0, -45.0),
    ];
    for size in sizes {
        let mesh = block(size);
        for (yaw, pitch) in views {
            let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
            let mut from = Camera::default();
            let normal = Vec3::new(
                pitch.cos() * yaw.cos(),
                pitch.cos() * yaw.sin(),
                pitch.sin(),
            );
            let up = Vec3::new(
                -pitch.sin() * yaw.cos(),
                -pitch.sin() * yaw.sin(),
                pitch.cos(),
            );
            from.face(normal, up);
            // Square as `render` asks, at its bounds, and narrow rooms
            // where the width binds: in perspective, a long model seen
            // across was cropped by a pixel or more there.
            for max in [[64u32, 64], [1024, 1024], [400, 300], [65, 1023]] {
                for projection in [Projection::Perspective, Projection::Orthographic] {
                    from.set_projection(projection);
                    let shot = frame(&mesh, &lines, None, &from, max, 8).unwrap();
                    assert!(shot.size[0] <= max[0] && shot.size[1] <= max[1], "{shot:?}");
                    let out = outside(&shot, mesh.positions(), 8);
                    assert!(out.is_empty(), "{size} {max:?} {out:?} {shot:?}");
                }
            }
        }
    }
}

#[test]
fn zoom_closes_in_on_the_framing_without_moving_the_camera() {
    let lines = RenderLines::default();
    let mesh = block(DVec3::new(4.0, 1.0, 1.0));
    let points: Vec<Vec3> = mesh.positions().iter().map(|&p| Vec3::from(p)).collect();
    let center = Vec3::new(2.0, 0.5, 0.5);
    let radius = points
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0, f32::max);
    let corner = Vec3::new(4.0, 1.0, 1.0);
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut from = Camera::default();
        from.set_projection(projection);
        let framed = frame(&mesh, &lines, None, &from, [300, 300], 8).unwrap();
        let at = |shot: &PreviewShot, p| match projection {
            Projection::Perspective => shown_perspective(shot, p),
            Projection::Orthographic => shown(shot, p),
        };
        let from_middle = |shot: &PreviewShot| {
            let [x, y] = at(shot, corner);
            Vec2::new(x - shot.size[0] as f32 / 2.0, y - shot.size[1] as f32 / 2.0).length()
        };
        for zoom in [10.0f32, 2.0, 0.1] {
            let mut shot = framed;
            shot.zoom(zoom);
            assert_eq!(shot.size, framed.size);
            // The camera stays where the framing put it, outside the
            // bodies' bounding sphere.
            assert!(shot.camera.eye().distance(framed.camera.eye()) < 1e-4);
            assert!(shot.camera.eye().distance(center) > radius, "{shot:?}");
            // The subject shows `zoom` times as large about the middle.
            let ratio = from_middle(&shot) / from_middle(&framed);
            assert!(
                (ratio - zoom).abs() < zoom * 1e-3,
                "{projection:?} {zoom}: {ratio}"
            );
            let view = framed.camera.view_height() / zoom;
            assert!((shot.camera.view_height() - view).abs() < view * 1e-4);
        }
    }
}
