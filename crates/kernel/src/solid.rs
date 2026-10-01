use glam::DVec3;

use crate::budget::Work;
use crate::mesh::Mesh;
use crate::par::par_map;
use crate::patch::{Bounds3, Patch};
use crate::quadrature::triangle_rule;
use crate::tessellate::{Display, Limits, Picking, tessellate, tessellate_picking, weld};
use crate::{
    Aabb, KernelError, ManifoldError, ManifoldMesh, MeshError, RenderMesh, Tolerance, Topology,
};

/// Units of work per patch whose volume [`Mesh::check`] integrates to
/// tell which way a shell faces (about 17 µs a patch on one thread, the
/// units about half a microsecond): see [`Solid::new_within`].
pub(crate) const INTEGRATE_WORK: usize = 32;

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

    /// [`Solid::new`] for an operation with `work` left: it charges
    /// [`INTEGRATE_WORK`] for each patch whose volume the check integrated
    /// ([`Mesh::check_counted`]), after the check, since only the check
    /// tells how many. So a mesh can pass and still be
    /// [`KernelError::TooComplex`].
    pub(crate) fn new_within(
        mesh: Mesh,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, KernelError> {
        let integrated = mesh.check_counted(tol).map_err(KernelError::Invalid)?;
        work.spend(integrated.saturating_mul(INTEGRATE_WORK))?;
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

    /// The volume it encloses: by the divergence theorem, a third of the
    /// integral of `(P − o)·n` over its patches, `o` the middle of its
    /// box, each patch integrated in its parameters by Gauss–Legendre
    /// ([`Self::area`] has the rule). The patches are rational, so this is
    /// not exact, but for patches as round as a 90° arc it is accurate to
    /// rounding. 0 for the empty solid.
    pub fn volume(&self) -> f64 {
        let Some(bounds) = self.bounds3() else {
            return 0.0;
        };
        let o = (bounds.min + bounds.max) * 0.5;
        self.sum(|patch| flux(patch, o).x)
    }

    /// Its surface area: the integral of `|P_u × P_v|` over each patch's
    /// parameter triangle, cut into its four half-edge pieces, each by the
    /// 8 × 8 Gauss–Legendre rule through the collapsed square. The
    /// patches' sums are added in patch order, so the result doesn't
    /// depend on the thread count.
    pub fn area(&self) -> f64 {
        self.sum(|patch| {
            triangle_rule()
                .map(|(u, w)| {
                    let [_, pu, pv] = patch.eval_derivs(u);
                    w * pu.cross(pv).length()
                })
                .sum::<f64>()
        })
    }

    /// `f` over every patch, summed in patch order (see [`pieces`]).
    fn sum(&self, f: impl Fn(&Patch) -> f64 + Sync + Send) -> f64 {
        let tris: Vec<usize> = (0..self.mesh.tris().len()).collect();
        par_map(&tris, |&t| pieces(&self.mesh.patch(t), 0, &f))
            .into_iter()
            .sum()
    }

    /// The solid as triangles for drawing, within `display`'s targets:
    /// see [`Display`]. Fails with [`MeshError::TooLarge`] if that would
    /// take more vertices, indices or edges than a [`RenderMesh`] may
    /// hold, and with [`MeshError::Values`] if a position is past
    /// [`RenderMesh::MAX_POSITION`].
    pub fn tessellate(&self, display: &Display) -> Result<RenderMesh, MeshError> {
        tessellate(&self.mesh, display)
    }

    /// [`Solid::tessellate`], with which region and chain of `topology`
    /// each triangle and edge of the mesh draws, for picking. `topology`
    /// must be this solid's ([`Solid::topology`]).
    pub fn tessellate_picking(
        &self,
        display: &Display,
        topology: &Topology,
    ) -> Result<(RenderMesh, Picking), MeshError> {
        tessellate_picking(&self.mesh, display, topology)
    }

    /// The solid as a closed, oriented manifold of triangles for export,
    /// within `display`'s targets: the samples and triangles of
    /// [`Solid::tessellate`] in `f64`, each point once, welded by the
    /// patches' shared vertices and edges, never by distance, then
    /// checked ([`ManifoldMesh::new`]). Fails with
    /// [`ManifoldError::Empty`] for the empty solid,
    /// [`ManifoldError::TooLarge`] past the mesh's limits, and with the
    /// check's failure should the triangles not make a manifold (two
    /// samples rounding to one point on a tiny curved edge, say).
    pub fn manifold_mesh(&self, display: &Display) -> Result<ManifoldMesh, ManifoldError> {
        self.manifold_mesh_within(display, &Limits::EXPORT)
    }

    /// [`Solid::manifold_mesh`] within `limits`, which tests make small.
    pub(crate) fn manifold_mesh_within(
        &self,
        display: &Display,
        limits: &Limits,
    ) -> Result<ManifoldMesh, ManifoldError> {
        let (positions, triangles) = weld(&self.mesh, display, limits)?;
        ManifoldMesh::new(positions, triangles)
    }
}

/// `f` over `patch`. A patch with a weight far from 1, whose points
/// crowd towards some corner or edge where the rule can't follow them, is
/// split first ([`Patch::split4`], up to five times) until its pieces'
/// weights are near 1, and `f` summed over them.
fn pieces<T: std::iter::Sum>(patch: &Patch, depth: u32, f: &impl Fn(&Patch) -> T) -> T {
    /// The weights within which a patch is integrated as it is: a
    /// quarter circle's `√½` is, to rounding.
    const WELL_SHAPED: std::ops::RangeInclusive<f64> = 0.7..=1.4;
    let shaped = patch.w.iter().all(|w| WELL_SHAPED.contains(w));
    if depth < 5
        && !shaped
        && let Ok(children) = patch.split4()
    {
        return children.iter().map(|c| pieces(c, depth + 1, f)).sum();
    }
    f(patch)
}

/// A third of the integral of `(P − o)·n` over `patch`: its share of the
/// volume a closed surface of patches encloses (see [`Solid::volume`]);
/// and the same of its absolute value, the scale of the quadrature's
/// error however the integrand cancels.
fn flux(patch: &Patch, o: glam::DVec3) -> glam::DVec2 {
    triangle_rule()
        .map(|(u, w)| {
            let [p, pu, pv] = patch.eval_derivs(u);
            let term = w * (p - o).dot(pu.cross(pv));
            glam::DVec2::new(term, term.abs())
        })
        .sum::<glam::DVec2>()
        / 3.0
}

/// [`flux`] for a patch, split where its weights ask as
/// [`Solid::volume`] does: its share of the volume, and the scale of the
/// error.
pub(crate) fn patch_volume(patch: &Patch, o: glam::DVec3) -> (f64, f64) {
    let sized = pieces(patch, 0, &|piece| flux(piece, o));
    (sized.x, sized.y)
}

#[cfg(test)]
mod tests;
