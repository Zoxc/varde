//! Restoring the fold and control-hull invariants by refinement.
//!
//! [`Mesh::repair`] tests every patch and every pair of patches whose
//! boxes come within the resolution, splits what fails (red–green, see
//! [`refine`](super::refine)) and tests again, until nothing fails. Only
//! the pieces a round made or changed are tested again, against every
//! patch near them: a pair of pieces neither of which changed passed
//! before. Splitting shrinks hulls towards the surface (a child's hull
//! lies in its parent's) and turns normal coefficients towards the true
//! normals, so a mesh whose surface doesn't fold or touch itself passes
//! after enough splits. One that does runs into
//! [`MAX_REFINE_DEPTH`](crate::MAX_REFINE_DEPTH), pieces too small to
//! split ([`MIN_SPLIT`] resolutions) or the budget, and fails with
//! [`KernelError::TooComplex`]; repair never gives a mesh that fails
//! [`Mesh::check`].

use super::check::check_pair;
use super::refine::{Piece, Refiner};
use super::{Bvh, CheckError, Mesh};
use crate::budget::{Budget, Work};
use crate::par::par_map;
use crate::patch::Patch;
use crate::{KernelError, MAX_PATCHES, Tolerance};

/// The smallest piece repair splits, in resolutions across (along the
/// longest axis of its control points' box). Pieces a few resolutions
/// across can't keep the hull rules' margin from their own neighbours, so
/// splitting them further only makes more that fail. Pieces of a surface
/// that keeps clear of itself pass long before this: a patch of size `s`
/// on a curve of radius `R` sags by about `s²/8R`.
const MIN_SPLIT: f64 = 64.0;

impl Mesh {
    /// The mesh with the fold and control-hull invariants restored by
    /// splitting what breaks them, within `budget`. Returns the mesh as it
    /// is if nothing does.
    ///
    /// The mesh must pass the topology and shared-edge parts of
    /// [`Mesh::check`], and its patches must be within the patch bounds;
    /// otherwise it fails with [`KernelError::Invalid`]. The result passes
    /// `check` with `tol`, face tags aside: splitting keeps patches on
    /// their surfaces, up to rounding.
    pub fn repair(self, tol: &Tolerance, budget: &Budget) -> Result<Mesh, KernelError> {
        self.repair_within(tol, &mut Work::new(budget))
    }

    /// [`Self::repair`], taking its work from an operation's `work`.
    pub(crate) fn repair_within(
        self,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<Mesh, KernelError> {
        self.check_topology().map_err(KernelError::Invalid)?;
        let tris: Vec<u32> = (0..self.tris.len() as u32).collect();
        work.spend(tris.len())?;
        let pieces: Vec<Piece> = par_map(&tris, |&t| Piece {
            corners: self.corners(t),
            patch: self.patch(t as usize),
            face: self.tris[t as usize].face,
            leaf: t,
            origin: t,
            changed: true,
        });
        for (t, piece) in pieces.iter().enumerate() {
            piece
                .patch
                .check()
                .map_err(|e| KernelError::Invalid(CheckError::Patch(t as u32, e)))?;
        }
        let mut failing = failures(&pieces, tol, work)?;
        if failing.is_empty() {
            return Ok(self);
        }
        let mut refiner = Refiner::new(&self, MIN_SPLIT * tol.resolution());
        let pieces = loop {
            refiner.split(&failing, work)?;
            let pieces = refiner.pieces()?;
            if pieces.len() > MAX_PATCHES {
                return Err(KernelError::TooComplex);
            }
            work.spend(pieces.len())?;
            refiner.settle();
            failing = failures(&pieces, tol, work)?;
            if failing.is_empty() {
                break pieces;
            }
        };
        let mesh = refiner.mesh(&pieces);
        debug_assert_eq!(mesh.check(tol), Ok(()));
        Ok(mesh)
    }
}

/// The leaves to split: those of pieces that changed and fail the fold
/// check, and of pairs with a changed piece that fail the hull rules,
/// sorted. A piece with a degenerate corner
/// ([`Patch::degenerate_corner`]) fails the repair, naming the input
/// triangle it came from.
fn failures(pieces: &[Piece], tol: &Tolerance, work: &mut Work) -> Result<Vec<u32>, KernelError> {
    let margin = tol.resolution();
    let changed: Vec<u32> = (0..pieces.len() as u32)
        .filter(|&p| pieces[p as usize].changed)
        .collect();
    work.spend(changed.len())?;
    let folds = par_map(&changed, |&p| {
        let patch = &pieces[p as usize].patch;
        match patch.fold_direction() {
            Some(_) => Fold::Passes,
            None if patch.degenerate_corner().is_some() => Fold::Never,
            None => Fold::Split,
        }
    });
    let mut leaves = Vec::new();
    for (&p, fold) in changed.iter().zip(folds) {
        let piece = &pieces[p as usize];
        match fold {
            Fold::Passes => {}
            Fold::Split => leaves.push(piece.leaf),
            Fold::Never => return Err(KernelError::Invalid(CheckError::Fold(piece.origin))),
        }
    }

    let bvh = Bvh::new(pieces.iter().map(|p| p.patch.bounds()).collect());
    let near = par_map(&changed, |&p| {
        let mut near = Vec::new();
        bvh.query(&bvh.bounds(p), margin, &mut near);
        // Each pair once: with an unchanged piece, or the later one.
        near.retain(|&q| q != p && (!pieces[q as usize].changed || q > p));
        near
    });
    let mut pairs = Vec::new();
    for (&p, near) in changed.iter().zip(near) {
        pairs.extend(near.into_iter().map(|q| [p, q]));
    }
    work.spend(pairs.len())?;
    let split = par_map(&pairs, |&[p, q]| {
        let (a, b) = (&pieces[p as usize], &pieces[q as usize]);
        match check_pair([p, q], [&a.patch, &b.patch], [a.corners, b.corners], margin) {
            Ok(()) => [false; 2],
            // Splitting a patch that is its own hull (flat, with straight
            // edges) brings it no further from a non-neighbour, unless
            // both are.
            Err(CheckError::Hull(..)) => match [flat(&a.patch, margin), flat(&b.patch, margin)] {
                [true, true] => [true; 2],
                [fa, fb] => [!fa, !fb],
            },
            Err(_) => [true; 2],
        }
    });
    for (&[p, q], split) in pairs.iter().zip(split) {
        for (piece, split) in [p, q].into_iter().zip(split) {
            if split {
                leaves.push(pieces[piece as usize].leaf);
            }
        }
    }
    leaves.sort_unstable();
    leaves.dedup();
    Ok(leaves)
}

/// What the fold check makes of a piece.
enum Fold {
    Passes,
    /// Fails, and splitting may mend it.
    Split,
    /// Fails at a corner whose edges leave it at 0° or 180°, which every
    /// piece keeping the corner will.
    Never,
}

/// Whether every edge's control point is within `margin` of the line
/// through its ends, so the patch is within `margin` of its flat triangle.
fn flat(patch: &Patch, margin: f64) -> bool {
    (0..3).all(|i| {
        let (p, q) = (patch.p[i], patch.p[(i + 1) % 3]);
        let Some(u) = (q - p).try_normalize() else {
            return false;
        };
        let d = patch.c[i] - p;
        (d - u * d.dot(u)).length() <= margin
    })
}

#[cfg(test)]
mod tests;
