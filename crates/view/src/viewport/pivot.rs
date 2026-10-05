//! Picking the point the camera orbits: a middle click in the viewport
//! casts the cursor's ray at the model's mesh, the nearest hit winning,
//! and off it on to the grid's plane, the sketch's in a sketch; no GPU
//! picking.

use glam::{DVec2, DVec3, Vec3};
use varde_document::{OriginPlane, Placement};
use varde_kernel::RenderMesh;
use varde_render::{Camera, GRID_FADE_HEIGHTS, Projection};

use crate::pick::{aabb, ray_hits, through_box};
use crate::projection::Projector;

/// What a middle click at `at`, in logical pixels from the top left of a
/// viewport `size` big, picks for the camera to orbit: where the cursor's
/// ray first meets `mesh`, or else the grid's plane, `sketch` in a sketch
/// or else XY, where the grid shows, within [`GRID_FADE_HEIGHTS`] view
/// heights of the target (seen on the plane). `None` if it meets neither,
/// within the camera's extent.
pub(super) fn pick(
    mesh: &RenderMesh,
    camera: &Camera,
    size: [f32; 2],
    at: DVec2,
    sketch: Option<Placement>,
) -> Option<Vec3> {
    let placement = sketch.unwrap_or(OriginPlane::XY.placement());
    let projector = Projector::new(camera, placement, size[0], size[1])?;
    let (origin, direction) = projector.ray(at)?;
    let hit = on_mesh(mesh, camera, origin, direction)
        .or_else(|| on_grid(camera, &projector, placement, at))?;
    let hit = hit.as_vec3();
    (hit.is_finite() && hit.abs().max_element() <= Camera::EXTENT).then_some(hit)
}

/// Where the cursor's ray at `at` meets the grid on `placement`, if it
/// does where the grid shows.
fn on_grid(
    camera: &Camera,
    projector: &Projector,
    placement: Placement,
    at: DVec2,
) -> Option<DVec3> {
    let hit = projector.cursor(at)?.at;
    let target = placement.to_sketch(camera.target().as_dvec3());
    let reach = f64::from(GRID_FADE_HEIGHTS) * f64::from(camera.view_height());
    (hit.distance(target) <= reach).then(|| placement.to_world(hit))
}

/// Where the ray from `origin` along `direction` first meets `mesh`, in
/// front of the eye; in an orthographic view, which sees what's behind its
/// eye too, anywhere along it.
fn on_mesh(mesh: &RenderMesh, camera: &Camera, origin: DVec3, direction: DVec3) -> Option<DVec3> {
    let bounds = mesh.bounds()?;
    let direction = direction.try_normalize()?;
    // An orthographic ray starts at the target's depth: from far enough
    // back that the whole mesh is ahead of it.
    let origin = match camera.projection() {
        Projection::Perspective => origin,
        Projection::Orthographic => {
            let (min, max) = (bounds.min.as_dvec3(), bounds.max.as_dvec3());
            let back = (max - min).length() + (origin - (min + max) / 2.0).length();
            origin - direction * back
        }
    };
    let (near, far) = through_box(origin, direction, aabb(bounds), 0.0, f64::INFINITY)?;
    let corner = |index: &u32| {
        let p = mesh.positions().get(usize::try_from(*index).ok()?)?;
        Some(Vec3::from(*p).as_dvec3())
    };
    let nearest = mesh
        .indices()
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|triangle| {
            let [Some(a), Some(b), Some(c)] = triangle.each_ref().map(corner) else {
                return None;
            };
            ray_hits(origin, direction, [a, b, c])
        })
        // A little slack either way, for the box's rounding.
        .filter(|&t| t > 0.0 && t >= near * (1.0 - 1e-9) && t <= far * (1.0 + 1e-9) + 1e-9)
        .min_by(f64::total_cmp)?;
    Some(origin + direction * nearest)
}

#[cfg(test)]
mod tests;
