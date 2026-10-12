use glam::DVec3;

use crate::mesh::Mesh;
use crate::patch::Bounds3;
use crate::{
    Aabb, Display, KernelError, ManifoldError, ManifoldMesh, MeshError, RenderMesh, Tolerance,
    Topology,
};

/// A solid. In the facade every solid is empty: the old kernel's meshes
/// are gone, and the Varde kernel's B-reps don't reach the facade yet.
/// Everything that would make a non-empty one fails with
/// [`KernelError::NotImplemented`], and the queries answer for the empty
/// solid: no bounds, no volume, nothing to draw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Solid {
    /// Always empty; kept so [`Solid::mesh`] has a mesh to lend.
    mesh: Mesh,
}

impl Solid {
    /// The solid of `mesh`: the empty solid for a mesh with no triangles,
    /// else [`KernelError::NotImplemented`] (the old kernel's check of
    /// such meshes is gone).
    pub fn new(mesh: Mesh, _tol: &Tolerance) -> Result<Solid, KernelError> {
        if mesh.is_empty() {
            Ok(Solid::empty())
        } else {
            Err(KernelError::NotImplemented("solids of patch meshes"))
        }
    }

    /// The empty solid: no triangles, no volume.
    pub fn empty() -> Solid {
        Solid::default()
    }

    /// The box from `min` of `size`, its faces named for `feature`. Not
    /// built yet: always [`KernelError::NotImplemented`].
    pub fn cuboid(
        _min: DVec3,
        _size: DVec3,
        _feature: u64,
        _tol: &Tolerance,
    ) -> Result<Solid, KernelError> {
        Err(KernelError::NotImplemented("boxes"))
    }

    /// The upright cylinder on the circle about `base` of `radius`, its
    /// faces named for `feature`. Not built yet: always
    /// [`KernelError::NotImplemented`].
    pub fn cylinder(
        _base: DVec3,
        _radius: f64,
        _height: f64,
        _feature: u64,
        _tol: &Tolerance,
    ) -> Result<Solid, KernelError> {
        Err(KernelError::NotImplemented("cylinders"))
    }

    /// Its mesh: always empty.
    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// Its mesh, taken: always empty.
    pub fn into_mesh(self) -> Mesh {
        self.mesh
    }

    /// Whether it is the empty solid: always.
    pub fn is_empty(&self) -> bool {
        self.mesh.is_empty()
    }

    /// The box around it in `f64`: `None` for the empty solid.
    pub fn bounds3(&self) -> Option<Bounds3> {
        Bounds3::around(self.mesh.verts())
    }

    /// The box around it, for the view: `None` for the empty solid.
    pub fn bounds(&self) -> Option<Aabb> {
        let b = self.bounds3()?;
        Some(Aabb {
            min: b.min.as_vec3(),
            max: b.max.as_vec3(),
        })
    }

    /// The volume it encloses: 0 for the empty solid.
    pub fn volume(&self) -> f64 {
        0.0
    }

    /// Its surface area: 0 for the empty solid.
    pub fn area(&self) -> f64 {
        0.0
    }

    /// It tessellated for drawing: the empty mesh.
    pub fn tessellate(&self, display: &Display) -> Result<RenderMesh, MeshError> {
        self.tessellate_with(display, &Topology::default())
    }

    /// It tessellated for drawing, face by face of `topology`: the empty
    /// mesh.
    pub fn tessellate_with(
        &self,
        _display: &Display,
        _topology: &Topology,
    ) -> Result<RenderMesh, MeshError> {
        Ok(RenderMesh::default())
    }

    /// It welded into a closed manifold of triangles for export: the
    /// empty solid has none, [`ManifoldError::Empty`].
    pub fn manifold_mesh(&self, _display: &Display) -> Result<ManifoldMesh, ManifoldError> {
        Err(ManifoldError::Empty)
    }
}
