use glam::DVec3;

use crate::mesh::Mesh;
use crate::patch::Bounds3;
use crate::tessellate::{Display, tessellate};
use crate::{Aabb, KernelError, MeshError, RenderMesh, Tolerance};

/// A closed solid: a [`Mesh`] of rational quadratic patches that passes
/// [`Mesh::check`], always. It is never stored; documents keep what builds
/// it.
///
/// The only ways to get one are [`Solid::new`], which checks, and the
/// constructors here, which build meshes that are checked as they're
/// made.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    mesh: Mesh,
}

impl Solid {
    /// The solid bounded by `mesh`, if it passes [`Mesh::check`] with
    /// `tol`; [`KernelError::Invalid`] with the first failure if not.
    pub fn new(mesh: Mesh, tol: &Tolerance) -> Result<Solid, KernelError> {
        mesh.check(tol).map_err(KernelError::Invalid)?;
        Ok(Solid { mesh })
    }

    /// The empty solid.
    pub fn empty() -> Solid {
        Solid {
            mesh: Mesh::default(),
        }
    }

    /// The axis-aligned box from `min` to `min + size`: see
    /// [`Mesh::cuboid`], which also names its faces after `feature`.
    pub fn cuboid(
        min: DVec3,
        size: DVec3,
        feature: u64,
        tol: &Tolerance,
    ) -> Result<Solid, KernelError> {
        // `Mesh::cuboid` checks what it builds.
        Mesh::cuboid(min, size, feature, tol).map(|mesh| Solid { mesh })
    }

    /// The circular cylinder standing on `base` along `+z`: see
    /// [`Mesh::cylinder`].
    pub fn cylinder(
        base: DVec3,
        radius: f64,
        height: f64,
        feature: u64,
        tol: &Tolerance,
    ) -> Result<Solid, KernelError> {
        // `Mesh::cylinder` checks what it builds.
        Mesh::cylinder(base, radius, height, feature, tol).map(|mesh| Solid { mesh })
    }

    /// The patches bounding it.
    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    pub fn into_mesh(self) -> Mesh {
        self.mesh
    }

    pub fn is_empty(&self) -> bool {
        self.mesh.is_empty()
    }

    /// The box around its control points, which holds every point of it,
    /// or `None` for the empty solid.
    pub fn bounds3(&self) -> Option<Bounds3> {
        let corners = Bounds3::around(self.mesh.verts())?;
        Some(
            self.mesh
                .edges()
                .iter()
                .fold(corners, |b, edge| b.include(edge.ctrl)),
        )
    }

    /// [`Solid::bounds3`] in `f32`, as drawing takes it. Rounding to
    /// nearest is monotonic, so the box still holds every position of
    /// [`Solid::tessellate`]'s mesh.
    pub fn bounds(&self) -> Option<Aabb> {
        self.bounds3().map(|b| Aabb {
            min: b.min.as_vec3(),
            max: b.max.as_vec3(),
        })
    }

    /// The solid as triangles for drawing, within `display`'s targets:
    /// see [`Display`]. Fails with [`MeshError::TooLarge`] if that would
    /// take more vertices, indices or edges than a [`RenderMesh`] may
    /// hold, and with [`MeshError::Values`] if a position is past
    /// [`RenderMesh::MAX_POSITION`].
    pub fn tessellate(&self, display: &Display) -> Result<RenderMesh, MeshError> {
        tessellate(&self.mesh, display)
    }
}

#[cfg(test)]
mod tests;
