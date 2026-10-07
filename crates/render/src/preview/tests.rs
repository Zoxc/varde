use glam::DVec3;
use varde_kernel::{Display, Solid, Tolerance};

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
    let both = frame(&mesh, &sketches, &Camera::default(), [400, 300], 4).unwrap();
    let alone = frame(
        &mesh,
        &RenderLines::default(),
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
    let half = (encode(0.5) * 255.0).round() as u8;
    let white = srgb.straight([half, half, half, 128]);
    assert!(white[..3].iter().all(|&c| c >= 254), "{white:?}");
    assert_eq!(
        Layout::of(wgpu::TextureFormat::Rgba16Float).map(|_| ()),
        None
    );
}
