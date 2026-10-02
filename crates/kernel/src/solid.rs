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

/// Units of work per patch that checking a result takes (about 2.7 µs a
/// patch on one thread), not counting the patches it integrates to tell
/// which way the shells face ([`Solid::new_within`] charges those).
pub(crate) const CHECK_WORK: usize = 5;

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

    /// The solid bounded by `mesh` with its faces merged
    /// ([`Mesh::merge_faces`]), repaired first only if it fails
    /// [`Mesh::check`] with `tol`: for a mesh built to pass, such as an
    /// extrude's, which then takes one pass over its pairs of patches
    /// rather than two (repair's, finding nothing, and the check's).
    /// Repair returns a mesh that passes the check as it is, and merging
    /// changes only names, so the solid is the one [`Mesh::repair_within`],
    /// [`Mesh::merge_faces`] and then [`Solid::new_within`] give, for the
    /// same work; a mesh that fails is repaired, merged and checked just
    /// so. The check is charged as repair's first pass would be
    /// ([`Mesh::check_counted_within`]); a merged mesh that passed it is
    /// checked again only for what names touch
    /// ([`Mesh::check_topology`]).
    pub(crate) fn new_repaired_within(
        mesh: Mesh,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, KernelError> {
        match mesh.check_counted_within(tol, work) {
            Ok(integrated) => {
                let mesh = mesh.merge_faces(tol.resolution(), work)?;
                mesh.check_topology().map_err(KernelError::Invalid)?;
                work.spend(integrated.saturating_mul(INTEGRATE_WORK))?;
                Ok(Solid { mesh })
            }
            Err(KernelError::Invalid(_)) => {
                let mesh = mesh
                    .repair_within(tol, work)?
                    .merge_faces(tol.resolution(), work)?;
                Solid::new_within(mesh, tol, work)
            }
            Err(e) => Err(e),
        }
    }

    /// The solid of `mesh`, closed as an operation built it but perhaps
    /// breaking the fold and hull rules: repaired, faces of one surface
    /// that meet merged ([`Mesh::merge_faces`]), and checked, the check
    /// charged [`CHECK_WORK`] a patch and the patches it integrated as
    /// [`Self::new_within`] charges them: for booleans' and revolves'
    /// meshes, which often need repair; an extrude's, built to pass,
    /// takes [`Self::new_repaired_within`].
    pub(crate) fn finished(
        mesh: Mesh,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, KernelError> {
        let mesh = mesh
            .repair_within(tol, work)?
            .merge_faces(tol.resolution(), work)?;
        work.spend(mesh.tris().len().saturating_mul(CHECK_WORK))?;
        Solid::new_within(mesh, tol, work)
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
    /// [`Solid::tessellate`], each point once, welded by the patches'
    /// shared vertices and edges, never by distance, about the middle of
    /// the solid's box rounded to whole numbers and rounded to `f32` there,
    /// then checked ([`ManifoldMesh::new`]). Fails with
    /// [`ManifoldError::Empty`] for the empty solid,
    /// [`ManifoldError::TooLarge`] past the mesh's limits,
    /// [`ManifoldError::Origin`] too far out, and with the check's failure
    /// should the triangles not make a manifold (two samples rounding to
    /// one `f32` point on a tiny curved edge, say).
    pub fn manifold_mesh(&self, display: &Display) -> Result<ManifoldMesh, ManifoldError> {
        self.manifold_mesh_within(display, &Limits::EXPORT)
    }

    /// [`Solid::manifold_mesh`] within `limits`, which tests make small.
    pub(crate) fn manifold_mesh_within(
        &self,
        display: &Display,
        limits: &Limits,
    ) -> Result<ManifoldMesh, ManifoldError> {
        let (origin, positions, triangles) = weld(&self.mesh, display, limits)?;
        ManifoldMesh::new(origin, positions, triangles)
    }
}

/// `f` over `patch`. A patch with a weight far from 1, whose points
/// crowd towards some corner or edge where the rule can't follow them, is
/// split first ([`Patch::split4`], up to five times) until its pieces'
/// weights are near 1, and `f` summed over them.
pub(crate) fn pieces<T: std::iter::Sum>(patch: &Patch, depth: u32, f: &impl Fn(&Patch) -> T) -> T {
    match quartered(patch, depth) {
        Some(children) => children.iter().map(|c| pieces(c, depth + 1, f)).sum(),
        None => f(patch),
    }
}

/// How many pieces [`pieces`] integrates `patch` in: what measuring
/// charges before integrating.
pub(crate) fn piece_count(patch: &Patch, depth: u32) -> usize {
    match quartered(patch, depth) {
        Some(children) => children.iter().map(|c| piece_count(c, depth + 1)).sum(),
        None => 1,
    }
}

/// The four pieces [`pieces`] splits `patch` into at `depth`, or `None`
/// where it integrates the patch as it is.
fn quartered(patch: &Patch, depth: u32) -> Option<[Patch; 4]> {
    /// The weights within which a patch is integrated as it is: a
    /// quarter circle's `√½` is, to rounding.
    const WELL_SHAPED: std::ops::RangeInclusive<f64> = 0.7..=1.4;
    let shaped = patch.w.iter().all(|w| WELL_SHAPED.contains(w));
    if depth < 5 && !shaped {
        patch.split4().ok()
    } else {
        None
    }
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
