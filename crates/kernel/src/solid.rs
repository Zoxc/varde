use glam::DVec3;

use crate::mesh::Mesh;
use crate::par::par_map;
use crate::patch::{Bounds3, Patch};
use crate::quadrature::triangle_rule;
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
        self.sum(|patch| flux(patch, o))
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
}

/// `f` over `patch`. A patch with a weight far from 1, whose points
/// crowd towards some corner or edge where the rule can't follow them, is
/// split first ([`Patch::split4`], up to five times) until its pieces'
/// weights are near 1, and `f` summed over them.
fn pieces(patch: &Patch, depth: u32, f: &impl Fn(&Patch) -> f64) -> f64 {
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
/// volume a closed surface of patches encloses (see [`Solid::volume`]).
fn flux(patch: &Patch, o: glam::DVec3) -> f64 {
    triangle_rule()
        .map(|(u, w)| {
            let [p, pu, pv] = patch.eval_derivs(u);
            w * (p - o).dot(pu.cross(pv))
        })
        .sum::<f64>()
        / 3.0
}

/// [`flux`] for a patch, split where its weights ask as
/// [`Solid::volume`] does.
pub(crate) fn patch_volume(patch: &Patch, o: glam::DVec3) -> f64 {
    pieces(patch, 0, &|piece| flux(piece, o))
}

#[cfg(test)]
mod tests;
