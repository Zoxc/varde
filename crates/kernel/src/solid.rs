use glam::DVec3;

use crate::budget::Work;
use crate::mesh::{Hint, Mesh, Refusal, tested};
use crate::par::par_map;
use crate::patch::{Bounds3, Patch};
use crate::quadrature::triangle_rule;
use crate::tessellate::{Display, Limits, tessellate, tessellate_with, weld};
use crate::{
    Aabb, Failure, KernelError, ManifoldError, ManifoldMesh, MeshError, RenderMesh, Tolerance,
    Topology,
};

/// Units of work per patch whose volume [`Mesh::check`] integrates to
/// tell which way a shell faces (about 17 µs a patch on one thread, the
/// units about half a microsecond): see [`Solid::new_within`].
pub(crate) const INTEGRATE_WORK: usize = 32;

/// Units of work per patch that checking a result takes (about 2.7 µs a
/// patch on one thread), not counting the patches it integrates to tell
/// which way the shells face ([`Solid::new_within`] charges those).
pub(crate) const CHECK_WORK: usize = 5;

/// How many patches a unit of work stands for in a pass over a whole
/// mesh that does little for each: telling which triangles a boolean
/// kept as they were ([`Kept`]).
pub(crate) const SCAN: usize = 16;

/// Units of work per patch that the parts of the check that run over
/// the whole mesh take when it tests the fold and hull rules only near
/// a boolean's changes ([`Solid::finished_near`]): topology, bounds,
/// the boxes' tree, which way the shells face and the face tags (about
/// 1.2 µs a patch on one thread under load, a fifth of the whole check).
pub(crate) const CHECK_WHOLE_WORK: usize = 1;

/// A mesh that failed on its way to a [`Solid`] as
/// [`KernelError::Invalid`] ([`Solid::finished_or_unfinished`]), with
/// what the error names.
pub(crate) enum Unfinished {
    /// Repair refused the mesh as given; `pieces` are those it names, if
    /// any ([`Refusal`]), else the error names triangles of `given`.
    Repair {
        given: Box<Mesh>,
        pieces: Vec<Patch>,
    },
    /// The check refused the repaired and merged mesh `checked`, whose
    /// triangles the error names; `given` is the mesh repair was given,
    /// where repair changed it (else `checked` has its positions and
    /// triangles: merging changes only names).
    Check {
        given: Option<Box<Mesh>>,
        checked: Box<Mesh>,
    },
}

impl Unfinished {
    /// The mesh as given to repair, up to face names.
    pub(crate) fn given(&self) -> &Mesh {
        match self {
            Unfinished::Repair { given, .. }
            | Unfinished::Check {
                given: Some(given), ..
            } => given,
            Unfinished::Check { checked, .. } => checked,
        }
    }

    /// `error`, the one this mesh failed with, with what it names as
    /// [`Failure::evidence`]: repair's pieces where it names some, else
    /// the triangles of the mesh that failed ([`Failure::of_mesh`]).
    pub(crate) fn failure(&self, error: KernelError) -> Failure {
        match self {
            Unfinished::Repair { given, pieces } if pieces.is_empty() => {
                Failure::of_mesh(error, given)
            }
            Unfinished::Repair { pieces, .. } => Failure::of_patches(error, pieces),
            Unfinished::Check { checked, .. } => Failure::of_mesh(error, checked),
        }
    }
}

/// An error of [`Solid::finished_or_unfinished`] as a failure, with the
/// evidence the mesh left holds ([`Unfinished::failure`]).
fn failure((error, unfinished): (KernelError, Option<Unfinished>)) -> Failure {
    match unfinished {
        Some(unfinished) => unfinished.failure(error),
        None => error.into(),
    }
}

/// A closed solid: a [`Mesh`] of rational quadratic patches that passes
/// [`Mesh::check`], always. It is never stored; documents keep what builds
/// it.
///
/// The only ways to get one are [`Solid::new`], which checks, and the
/// constructors here, which build meshes that are checked as they're
/// made.
///
/// Two solids are equal when their meshes are.
#[derive(Debug, Clone)]
pub struct Solid {
    mesh: Mesh,
    /// The resolution its mesh passed the check with: a boolean at the
    /// same resolution needn't test again the pairs of its triangles it
    /// keeps as they were (see [`Kept`]).
    resolution: f64,
}

impl PartialEq for Solid {
    fn eq(&self, other: &Self) -> bool {
        self.mesh == other.mesh
    }
}

/// What a boolean's result may keep of its operands as they were: the
/// two operands, and for each triangle of the result, the triangle of an
/// operand (`0` or `1`, and its index) it may be. Only a hint, which
/// [`tested`] verifies bit for bit: where it is right, repair and the
/// check test only the triangles the operation changed or that come near
/// the other operand's kept ones, and every pair with one of them, as
/// the rest passed the check in their operand.
pub(crate) struct Kept<'a> {
    pub(crate) operands: [&'a Solid; 2],
    pub(crate) source: Vec<Hint>,
}

impl Kept<'_> {
    /// Which triangles of `mesh`, whose triangles' operand triangles
    /// `source` names, repair and the check must test with `tol` (see
    /// [`tested`]): an operand counts only if it passed the check with
    /// `tol`'s resolution.
    fn tested(&self, mesh: &Mesh, source: &[Hint], tol: &Tolerance) -> Vec<bool> {
        let resolution = tol.resolution();
        let operands = self
            .operands
            .map(|s| (s.resolution == resolution).then_some(&s.mesh));
        tested(mesh, operands, source, resolution)
    }
}

impl Solid {
    /// The solid bounded by `mesh`, if it passes [`Mesh::check`] with
    /// `tol`; [`KernelError::Invalid`] with the first failure if not.
    pub fn new(mesh: Mesh, tol: &Tolerance) -> Result<Solid, KernelError> {
        mesh.check(tol).map_err(KernelError::Invalid)?;
        Ok(Solid::checked(mesh, tol))
    }

    /// The solid of `mesh`, which passed the check with `tol`.
    fn checked(mesh: Mesh, tol: &Tolerance) -> Solid {
        Solid {
            mesh,
            resolution: tol.resolution(),
        }
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
        Solid::new_or_checked(mesh, tol, work).map_err(|(e, _)| e)
    }

    /// [`Self::new_within`], giving the mesh back with the error where
    /// the check found it invalid.
    fn new_or_checked(
        mesh: Mesh,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, (KernelError, Option<Box<Mesh>>)> {
        Solid::new_or_checked_near(mesh, None, tol, work)
    }

    /// [`Self::new_or_checked`], testing only the triangles `tested`
    /// marks and the pairs with one of them for the fold and hull rules
    /// where given ([`Mesh::check_counted_near`]), which must leave out
    /// only triangles and pairs that pass them.
    fn new_or_checked_near(
        mesh: Mesh,
        tested: Option<&[bool]>,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, (KernelError, Option<Box<Mesh>>)> {
        let checked = match tested {
            Some(tested) => {
                let near = mesh.check_counted_near(tol, tested);
                // The pairs and triangles left out pass, so the whole
                // check comes to the same, error and all.
                #[cfg(debug_assertions)]
                assert_eq!(near, mesh.check_counted(tol), "the check near the changes");
                near
            }
            None => mesh.check_counted(tol),
        };
        let integrated = match checked {
            Ok(integrated) => integrated,
            Err(e) => return Err((KernelError::Invalid(e), Some(Box::new(mesh)))),
        };
        work.spend(integrated.saturating_mul(INTEGRATE_WORK))
            .map_err(|e| (e, None))?;
        Ok(Solid::checked(mesh, tol))
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
    ///
    /// A [`KernelError::Invalid`] comes with what it names, of the mesh
    /// that failed ([`Unfinished::failure`]).
    pub(crate) fn new_repaired_within(
        mesh: Mesh,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, Failure> {
        match mesh.check_counted_within(tol, work) {
            Ok(integrated) => {
                let mesh = mesh.merge_faces(tol.resolution(), work)?;
                if let Err(e) = mesh.check_topology() {
                    return Err(Failure::of_mesh(KernelError::Invalid(e), &mesh));
                }
                work.spend(integrated.saturating_mul(INTEGRATE_WORK))?;
                Ok(Solid::checked(mesh, tol))
            }
            // The check already charged (`check_counted_within`), none
            // before checking the repaired mesh.
            Err(KernelError::Invalid(_)) => {
                Solid::finished_or_unfinished(mesh, 0, tol, work).map_err(failure)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The solid of `mesh`, closed as an operation built it but perhaps
    /// breaking the fold and hull rules: repaired, faces of one surface
    /// that meet merged ([`Mesh::merge_faces`]), and checked, the check
    /// charged [`CHECK_WORK`] a patch and the patches it integrated as
    /// [`Self::new_within`] charges them: for booleans' and revolves'
    /// meshes, which often need repair; an extrude's, built to pass,
    /// takes [`Self::new_repaired_within`]. A [`KernelError::Invalid`]
    /// comes with what it names, of the mesh that failed
    /// ([`Unfinished::failure`]).
    pub(crate) fn finished(mesh: Mesh, tol: &Tolerance, work: &mut Work) -> Result<Solid, Failure> {
        Solid::finished_or_unfinished(mesh, CHECK_WORK, tol, work).map_err(failure)
    }

    /// `mesh` repaired, merged and checked, the check charged
    /// `check_work` a patch before it and the patches it integrated
    /// after ([`Self::finished`] with [`CHECK_WORK`]); where that fails as
    /// [`KernelError::Invalid`], the mesh that failed comes back with the
    /// error, for the caller to tell why and where ([`Unfinished`]).
    pub(crate) fn finished_or_unfinished(
        mesh: Mesh,
        check_work: usize,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, (KernelError, Option<Unfinished>)> {
        Solid::finished_near(mesh, check_work, None, tol, work)
    }

    /// [`Self::finished_or_unfinished`] for a boolean's result, which
    /// `kept` tells what it may keep of the operands: repair starts from
    /// the triangles [`Kept::tested`] marks and the check tests only
    /// those it marks on the repaired mesh (and the pairs with one of
    /// them), charged `check_work` a patch it tests, [`CHECK_WHOLE_WORK`]
    /// a patch for what it runs over the whole mesh, and a unit for every
    /// [`SCAN`] patches for each time the kept triangles are told. Without
    /// `kept`, every triangle is tested, the check charged `check_work` a
    /// patch. The same result and error either way, for less work (debug
    /// builds check the whole mesh too, and assert that).
    pub(crate) fn finished_near(
        mesh: Mesh,
        check_work: usize,
        kept: Option<Kept>,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Solid, (KernelError, Option<Unfinished>)> {
        let tested = kept.as_ref().map(|k| {
            work.spend(mesh.tris().len() / SCAN)
                .map(|()| k.tested(&mesh, &k.source, tol))
        });
        let tested = tested.transpose().map_err(|e| (e, None))?;
        let (given, mesh, origins) = match mesh.repaired_from(tested.as_deref(), tol, work) {
            Ok(Some((repaired, origins))) => (Some(Box::new(mesh)), repaired, Some(origins)),
            Ok(None) => (None, mesh, None),
            Err(Refusal {
                error: error @ KernelError::Invalid(_),
                pieces,
            }) => {
                let given = Box::new(mesh);
                return Err((error, Some(Unfinished::Repair { given, pieces })));
            }
            Err(refusal) => return Err((refusal.error, None)),
        };
        let mesh = mesh
            .merge_faces(tol.resolution(), work)
            .map_err(|e| (e, None))?;
        // Each triangle of the repaired mesh may be what the triangle it
        // came from may be: a piece of a split one isn't, which
        // `tested` tells.
        let tested = match kept {
            Some(kept) => {
                let source: Vec<Hint> = match origins {
                    Some(origins) => origins
                        .iter()
                        .map(|&t| kept.source.get(t as usize).copied().flatten())
                        .collect(),
                    None => kept.source.clone(),
                };
                let n = mesh.tris().len();
                work.spend(n / SCAN + n.saturating_mul(CHECK_WHOLE_WORK))
                    .map_err(|e| (e, None))?;
                let tested = kept.tested(&mesh, &source, tol);
                let count = tested.iter().filter(|&&t| t).count();
                work.spend(count.saturating_mul(check_work))
                    .map_err(|e| (e, None))?;
                Some(tested)
            }
            None => {
                work.spend(mesh.tris().len().saturating_mul(check_work))
                    .map_err(|e| (e, None))?;
                None
            }
        };
        Solid::new_or_checked_near(mesh, tested.as_deref(), tol, work).map_err(
            |(error, checked)| {
                let unfinished = checked.map(|checked| Unfinished::Check { given, checked });
                (error, unfinished)
            },
        )
    }

    /// The empty solid.
    pub fn empty() -> Solid {
        Solid {
            mesh: Mesh::default(),
            resolution: 0.0,
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
        Mesh::cuboid(min, size, feature, tol).map(|mesh| Solid::checked(mesh, tol))
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
        Mesh::cylinder(base, radius, height, feature, tol).map(|mesh| Solid::checked(mesh, tol))
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

    /// [`Solid::tessellate`] with the solid's topology already worked out:
    /// the mesh's faces are `topology`'s regions and its first edges its
    /// chains, in order. `topology` must be this solid's
    /// ([`Solid::topology`]).
    pub fn tessellate_with(
        &self,
        display: &Display,
        topology: &Topology,
    ) -> Result<RenderMesh, MeshError> {
        tessellate_with(&self.mesh, display, topology)
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
