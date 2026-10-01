//! What the renderer draws around the camera, and the depth range that
//! covers it.

use std::ops::Range;

use glam::{Mat4, Vec3};

use varde_kernel::Aabb;

use crate::Camera;

/// How many view heights from the target the grid fades out within. The
/// renderer passes it to `fs_grid` as a pipeline constant.
pub const GRID_FADE_HEIGHTS: f32 = 6.0;

/// A plane in the world with axes in it, which the grid is drawn on: the
/// XY plane, or the plane of the sketch being edited. The grid's lines
/// follow the axes, and its axis lines are where they cross the origin.
///
/// The fields are private so the plane stays usable: the origin within
/// [`Camera::EXTENT`], and the axes unit length and at right angles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridPlane {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
}

impl GridPlane {
    /// The world's XY plane, seen from above.
    pub const XY: GridPlane = GridPlane {
        origin: Vec3::ZERO,
        x: Vec3::X,
        y: Vec3::Y,
    };

    /// The plane through `origin` spanned by `x` and `y`, which is `x`
    /// normalized and `y` turned in the plane to be at right angles to it.
    /// `None` if `origin` is past [`Camera::EXTENT`] or not finite, or the
    /// axes don't span a plane.
    pub fn new(origin: Vec3, x: Vec3, y: Vec3) -> Option<GridPlane> {
        let extent = Vec3::splat(Camera::EXTENT);
        if !origin.abs().cmple(extent).all() {
            return None;
        }
        let x = x.try_normalize()?;
        let y = y.reject_from_normalized(x).try_normalize()?;
        Some(GridPlane { origin, x, y })
    }

    pub fn origin(&self) -> Vec3 {
        self.origin
    }

    pub fn x(&self) -> Vec3 {
        self.x
    }

    pub fn y(&self) -> Vec3 {
        self.y
    }

    /// `x × y`, the side the plane is seen from.
    pub fn normal(&self) -> Vec3 {
        self.x.cross(self.y)
    }

    /// The point of the plane nearest `point`.
    pub(crate) fn project(&self, point: Vec3) -> Vec3 {
        let normal = self.normal();
        point - normal * (point - self.origin).dot(normal)
    }
}

impl Default for GridPlane {
    fn default() -> Self {
        GridPlane::XY
    }
}

/// The view projection for `camera` in a viewport of `aspect`, with a depth
/// range fitted to the scene around it, the grid on `grid`, and `bounds`,
/// which hold the model and the lines drawn with it.
pub(crate) fn view_projection(
    camera: &Camera,
    aspect: f32,
    grid: &GridPlane,
    bounds: impl IntoIterator<Item = Aabb>,
) -> Mat4 {
    camera.projection_matrix(aspect, depth_range(camera, grid, bounds)) * camera.view()
}

/// The nearest and farthest distances in front of the eye to draw: the
/// visible region at the target, the grid, which fades out within
/// [`GRID_FADE_HEIGHTS`] of the target seen on its plane, and `bounds`.
///
/// Sized to the scene rather than to the camera's distance, which in
/// orthographic mode says nothing about depth.
fn depth_range(
    camera: &Camera,
    grid: &GridPlane,
    bounds: impl IntoIterator<Item = Aabb>,
) -> Range<f32> {
    let height = camera.view_height();
    let target = camera.target();
    let spheres = [
        (target, height),
        (grid.project(target), height * GRID_FADE_HEIGHTS),
    ]
    .into_iter()
    .chain(bounds.into_iter().map(|b| (b.center(), b.radius())));
    let (eye, backward) = (camera.eye(), camera.backward());
    let (near, far) = spheres.fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(near, far), (center, radius)| {
            let depth = (eye - center).dot(backward);
            (near.min(depth - radius), far.max(depth + radius))
        },
    );
    // Mesh positions and line points (within `RenderMesh::MAX_POSITION`
    // and `RenderLines::MAX_POSITION`), the grid's origin and the camera
    // are bounded, so this stays finite. The margin keeps faces on the
    // bounds, and edges pulled towards the camera, inside.
    let margin = (far - near) * 0.01;
    near - margin..far + margin
}

#[cfg(test)]
mod tests;
