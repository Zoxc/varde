//! What the renderer draws around the camera, and the depth range that
//! covers it.

use std::ops::Range;

use glam::Mat4;

use varde_kernel::Aabb;

use crate::Camera;

/// How many view heights from the target the grid fades out within. The
/// renderer passes it to `fs_grid` as a pipeline constant.
pub(crate) const GRID_FADE_HEIGHTS: f32 = 6.0;

/// The view projection for `camera` in a viewport of `aspect`, with a depth
/// range fitted to the scene around it and the model's `bounds`.
pub(crate) fn view_projection(camera: &Camera, aspect: f32, bounds: Option<Aabb>) -> Mat4 {
    camera.projection_matrix(aspect, depth_range(camera, bounds)) * camera.view()
}

/// The nearest and farthest distances in front of the eye to draw: the
/// visible region at the target, the grid, which fades out within
/// [`GRID_FADE_HEIGHTS`] of the target, and `bounds`.
///
/// Sized to the scene rather than to the camera's distance, which in
/// orthographic mode says nothing about depth.
fn depth_range(camera: &Camera, bounds: Option<Aabb>) -> Range<f32> {
    let height = camera.view_height();
    let target = camera.target();
    let spheres = [
        Some((target, height)),
        Some((target.with_z(0.0), height * GRID_FADE_HEIGHTS)),
        bounds.map(|b| (b.center(), b.radius())),
    ];
    let (eye, backward) = (camera.eye(), camera.backward());
    let (near, far) = spheres.into_iter().flatten().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(near, far), (center, radius)| {
            let depth = (eye - center).dot(backward);
            (near.min(depth - radius), far.max(depth + radius))
        },
    );
    // Mesh positions (within `RenderMesh::MAX_POSITION`) and the camera are
    // bounded, so this stays finite. The margin keeps faces on the
    // bounds, and edges pulled towards the camera, inside.
    let margin = (far - near) * 0.01;
    near - margin..far + margin
}

#[cfg(test)]
mod tests;
