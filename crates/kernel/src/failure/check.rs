//! The evidence of a mesh failing [`Mesh::check`] or repair: the
//! triangles its [`CheckError`] names, as patches of that mesh.

use std::collections::HashSet;

use super::{Evidence, Failure, MAX_EVIDENCE, evidence_work};
use crate::KernelError;
use crate::budget::Work;
use crate::mesh::{CheckError, Mesh};
use crate::patch::Patch;

impl Failure {
    /// `error`, with the triangles of `mesh` it names as its evidence
    /// where it is [`KernelError::Invalid`] of `mesh` (the check's, or
    /// repair's naming the triangles of the mesh it was given); bare
    /// otherwise. See [`named`].
    pub(crate) fn of_mesh(error: KernelError, mesh: &Mesh) -> Failure {
        let evidence = match error {
            KernelError::Invalid(e) => named(mesh, e),
            _ => Evidence::default(),
        };
        Failure {
            error,
            evidence: Box::new(evidence),
        }
    }
}

/// The triangles of `mesh` that `error` names, as patches, gathered
/// from a fresh [`evidence_work`] allowance (a unit a triangle looked
/// at):
/// - one for [`CheckError::Fold`], [`CheckError::Face`] and
///   [`CheckError::FacesAgainst`];
/// - two for [`CheckError::Hull`], [`CheckError::EdgeNeighbours`],
///   [`CheckError::VertexNeighbours`] and [`CheckError::SameCorners`]
///   (one if both name the same triangle, as repair may when two pieces
///   of one triangle fail);
/// - for [`CheckError::InsideOut`], the shell (connected part, through
///   halfedge pairs) whose lowest triangle it names, outward from that
///   triangle breadth first, up to [`MAX_EVIDENCE`] and then truncated;
/// - for the structural errors naming a triangle (`Patch`) or a halfedge
///   (`Index`, `Pair`, `Loop`, `DirectedEdge`, `SharedEdge`), that
///   triangle if its indices are in range; none for those naming a
///   vertex, an edge, an alias or a count.
///
/// Patches are as the mesh holds them, unchecked; the receiver draws
/// those it can. Deterministic: the order depends on the mesh alone.
fn named(mesh: &Mesh, error: CheckError) -> Evidence {
    let mut evidence = Evidence::default();
    let mut work = evidence_work();
    let tris = match error {
        CheckError::Fold(t)
        | CheckError::Face(t)
        | CheckError::FacesAgainst(t)
        | CheckError::Patch(t, _) => vec![t],
        CheckError::Hull(t, u)
        | CheckError::EdgeNeighbours(t, u)
        | CheckError::VertexNeighbours(t, u)
        | CheckError::SameCorners(t, u) => {
            if t == u {
                vec![t]
            } else {
                vec![t, u]
            }
        }
        CheckError::InsideOut(t) => shell(mesh, t, &mut evidence, &mut work),
        CheckError::Index(h)
        | CheckError::Pair(h)
        | CheckError::Loop(h)
        | CheckError::DirectedEdge(h)
        | CheckError::SharedEdge(h) => vec![h / 3],
        CheckError::TooManyPatches(_)
        | CheckError::Counts
        | CheckError::Alias(_)
        | CheckError::Fan(_)
        | CheckError::EdgeUse(_) => Vec::new(),
    };
    for t in tris {
        if !evidence.afford(&mut work, 1) {
            break;
        }
        if let Some(patch) = patch_of(mesh, t) {
            evidence.add_patches([patch]);
        }
    }
    evidence
}

/// The triangles of the shell of `mesh` holding triangle `t`, `t` first
/// and then breadth first through each triangle's halfedges' pairs in
/// order, at most [`MAX_EVIDENCE`] patches of them: past that, or past
/// `work`, `evidence` is marked truncated. Indices out of range are
/// passed over (a mesh failing as inside out passed the topology checks,
/// but nothing here relies on it).
fn shell(mesh: &Mesh, t: u32, evidence: &mut Evidence, work: &mut Work) -> Vec<u32> {
    let tris = mesh.tris();
    if tris.get(t as usize).is_none() {
        return Vec::new();
    }
    let cap = MAX_EVIDENCE.patches;
    let mut order = vec![t];
    let mut seen = HashSet::from([t]);
    let mut next = 0;
    while let Some(&s) = order.get(next) {
        next += 1;
        if !evidence.afford(work, 1) {
            break;
        }
        for h in tris[s as usize].halfedges {
            let u = h.pair / 3;
            if tris.get(u as usize).is_none() || !seen.insert(u) {
                continue;
            }
            if order.len() >= cap {
                evidence.truncated = true;
                return order;
            }
            order.push(u);
        }
    }
    order
}

/// Triangle `t` of `mesh` as a patch ([`Mesh::patch`]), if it and the
/// vertices and edges its halfedges name are in range.
fn patch_of(mesh: &Mesh, t: u32) -> Option<Patch> {
    let tri = mesh.tris().get(t as usize)?;
    let mut patch = Patch {
        p: [glam::DVec3::ZERO; 3],
        c: [glam::DVec3::ZERO; 3],
        w: [0.0; 3],
    };
    for (i, h) in tri.halfedges.iter().enumerate() {
        patch.p[i] = *mesh.verts().get(h.start as usize)?;
        let edge = mesh.edges().get(h.edge as usize)?;
        patch.c[i] = edge.ctrl;
        patch.w[i] = edge.weight;
    }
    Some(patch)
}

#[cfg(test)]
mod tests;
