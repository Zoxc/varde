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
//! after enough splits. One that does fails: with
//! [`KernelError::Invalid`] at once where no split can mend it (a
//! degenerate corner, flat pieces breaking a hull rule, or points of two
//! pieces that share no vertex found within the resolution: see
//! [`witness_limit`]), with `Invalid` too once a piece to split is too
//! small to ([`MIN_SPLIT`] resolutions across if flat, else
//! [`MIN_CURVED_SPLIT`]), and otherwise with [`KernelError::TooComplex`]
//! at [`MAX_REFINE_DEPTH`](crate::MAX_REFINE_DEPTH), past
//! [`MAX_PATCHES`] or out of budget. Repair never gives
//! a mesh that fails the embedding part of [`Mesh::check`]. It doesn't
//! check face tags, with one exception: an input patch it splits on a
//! [`Surface::Plane`] face, whose pieces get
//! straight inner edges, must be on that plane, or repair fails with
//! [`CheckError::Face`] naming it rather than reshape it. Other tags pass
//! through unchecked, and splitting keeps patches on their surfaces up to
//! rounding.

use super::check::check_pair;
use super::hull::flat;
use super::refine::{Piece, Refiner};
use super::{Bvh, CheckError, Face, LookupMap, Mesh, Surface};
use crate::budget::{Budget, Work};
use crate::par::par_map;
use crate::patch::Patch;
use crate::{KernelError, MAX_PATCHES, Tolerance};
use witness::{ROUNDING, surfaces_within};

/// The smallest flat piece repair splits, in resolutions across (along
/// the longest axis of its leaf's control points' box; flat as [`flat`]
/// within the resolution). Pieces a few resolutions across can't keep the
/// hull rules' margin from their own neighbours, so splitting them
/// further only makes more that fail; a flat piece's failure, which
/// splitting hasn't mended by this size, doesn't shrink with it. Pieces
/// of a surface that keeps clear of itself pass long before this: a patch
/// of size `s` on a curve of radius `R` sags by about `s²/8R`. A piece
/// that must be split and can't fails the repair with
/// [`KernelError::Invalid`] of the failure that asked for the split:
/// splitting can't mend it at this tolerance, though a finer one, with
/// pieces larger in resolutions, may.
/// An extrude's profile segments are halved no smaller either, for its
/// own reasons.
pub(crate) const MIN_SPLIT: f64 = 64.0;

/// The smallest piece that isn't flat repair splits, in resolutions
/// across, as for [`MIN_SPLIT`]. Only such pieces are split below
/// `MIN_SPLIT`, since only their sag shrinks when split: a piece of size
/// `s` on radius `R` is flat once `s` is under about `√(8R)` resolutions,
/// so on surfaces of radius under about 500 resolutions curved pieces
/// reach `MIN_SPLIT` still curved, and small round surfaces near each
/// other (within a few resolutions) need pieces that small to pass.
pub(crate) const MIN_CURVED_SPLIT: f64 = 8.0;

/// How flat, in margins, a failing pair of non-neighbours must both be for
/// repair to stop splitting them (each control point of an edge within
/// this of its chord). A piece flat within the margin of a curved surface
/// still has a hull up to about half a margin off the surface, which
/// splitting shrinks by a quarter a time, so stopping there refused
/// surfaces a little more than a margin apart. At a sixteenth it refuses
/// only those within about 1.03 margins. That splits touching curved
/// surfaces deeper before they are flat enough, which the witness
/// ([`witness_limit`]) makes up for, failing them as soon as it finds
/// their surfaces within the margin. The neighbour rules keep the margin
/// itself: splitting halves their clearances while it quarters the
/// overshoot, so they gain nothing from going deeper.
pub(crate) const FLAT_STOP: f64 = 1.0 / 16.0;

/// The work a witness search is charged, about what it costs next to a
/// pair test.
const WITNESS_WORK: usize = 8;

impl Mesh {
    /// The mesh with the fold and control-hull invariants restored by
    /// splitting what breaks them, within `budget`. Returns the mesh as it
    /// is if nothing does.
    ///
    /// The mesh must pass the topology and shared-edge parts of
    /// [`Mesh::check`], and its patches must be within the patch bounds;
    /// otherwise it fails with [`KernelError::Invalid`]. So does a failure
    /// no split can mend, naming the input triangles it came from, and a
    /// patch to split whose face's `Plane` tag is wrong
    /// ([`CheckError::Face`]). The result passes `check` with `tol`, face
    /// tags aside: other tags aren't checked, and splitting keeps patches
    /// on their surfaces up to rounding.
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
        let patches = self.bounded_patches().map_err(KernelError::Invalid)?;
        work.spend(patches.len())?;
        let pieces: Vec<Piece> = (0..self.tris.len() as u32)
            .zip(patches)
            .map(|(t, patch)| Piece {
                corners: self.corners(t),
                patch,
                face: self.tris[t as usize].face,
                leaf: t,
                origin: t,
                changed: true,
            })
            .collect();
        // The input's leaves are its patches, the pieces.
        let mut failing = failures(
            &pieces,
            |t| &pieces[t as usize].patch,
            &self.faces,
            tol,
            work,
        )?;
        if failing.is_empty() {
            return Ok(self);
        }
        // Leaves too small to split are caught by `failures` first: the
        // refiner's own floor stops only those its rules take with them.
        let res = tol.resolution();
        let mut refiner = Refiner::new(&self, res, MIN_CURVED_SPLIT * res);
        let pieces = loop {
            refiner.split(&failing, work)?;
            let pieces = refiner.pieces()?;
            if pieces.len() > MAX_PATCHES {
                return Err(KernelError::TooComplex);
            }
            work.spend(pieces.len())?;
            refiner.settle();
            failing = failures(&pieces, |t| refiner.leaf_patch(t), &self.faces, tol, work)?;
            if failing.is_empty() {
                break pieces;
            }
        };
        let mesh = refiner.mesh(&pieces);
        // Face tags aside: unsplit patches, and any not on a plane, keep
        // the input's claims, which repair doesn't check.
        debug_assert_eq!(mesh.check_embedding(tol).err(), None);
        Ok(mesh)
    }
}

/// The leaves to split: those of pieces that changed and fail the fold
/// check, and of pairs with a changed piece that fail the hull rules,
/// sorted. A failure no split can mend fails the repair, naming the input
/// triangles the pieces came from: a piece with a degenerate corner
/// ([`Patch::degenerate_corner`](crate::patch::Patch::degenerate_corner)),
/// a failing pair of flat pieces ([`flat`]: within the margin for the
/// neighbour rules, within [`FLAT_STOP`] of it for non-neighbours), and a
/// failing pair of non-neighbours whose surfaces are found within the
/// margin ([`witness_limit`]). Of the pairs, the first such in pair order
/// names the error.
///
/// After those, a leaf to split that is too small to ([`splittable`]: the
/// patch of leaf `t` is `leaf(t)`) fails the repair with the failure that
/// asked for its split, the fold check's or the pair's, of the first such
/// leaf by id (and the first failure asking for it, folds then pairs in
/// order).
fn failures<'p>(
    pieces: &[Piece],
    leaf: impl Fn(u32) -> &'p Patch,
    faces: &[Face],
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Vec<u32>, KernelError> {
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
    // Each with what asked for it.
    let mut leaves: Vec<(u32, Asked)> = Vec::new();
    for (&p, fold) in changed.iter().zip(folds) {
        let piece = &pieces[p as usize];
        match fold {
            Fold::Passes => {}
            Fold::Split => leaves.push((piece.leaf, Asked::Fold(piece.origin))),
            Fold::Never => return Err(KernelError::Invalid(CheckError::Fold(piece.origin))),
        }
    }

    let bvh = Bvh::new(pieces.iter().map(|p| p.patch.bounds()).collect());
    // Each pair once: with an unchanged piece, or the later one.
    let pairs = bvh.pairs_within(
        &changed,
        margin,
        |p, q| q != p && (!pieces[q as usize].changed || q > p),
        work,
    )?;
    // Kept as small as a pass: most pairs pass, and there may be millions.
    let tested = par_map(&pairs, |&[p, q]| {
        let (a, b) = (&pieces[p as usize], &pieces[q as usize]);
        let ids = [a.origin, b.origin];
        let Err(e) = check_pair(ids, [&a.patch, &b.patch], [a.corners, b.corners], margin) else {
            return Ok(Split::NONE);
        };
        if !matches!(e, CheckError::Hull(..)) {
            return match [flat(&a.patch, margin), flat(&b.patch, margin)] {
                // The rules on flat triangles are exact, and their pieces
                // keep the same angles at shared corners and edges:
                // splitting can't mend them.
                [true, true] => Err(e),
                _ => Ok(Split {
                    pieces: [true; 2],
                    witness: false,
                }),
            };
        }
        let stop = FLAT_STOP * margin;
        match [flat(&a.patch, stop), flat(&b.patch, stop)] {
            // Pieces this flat are their own hulls up to a sixteenth of
            // the margin, and their pieces keep the gaps: splitting can't
            // mend them. Curved surfaces that touch, the witness has
            // caught first, if it was asked (see `witness_limit`).
            [true, true] => Err(e),
            // A flat piece is its own hull: splitting brings it no
            // further from a non-neighbour.
            [fa, fb] => Ok(Split {
                pieces: [!fa, !fb],
                witness: true,
            }),
        }
    });

    let witnessed = witnessed(pieces, faces, &pairs, &tested, margin, work)?;

    for (i, (&[p, q], tested)) in pairs.iter().zip(tested).enumerate() {
        if Some(i) == witnessed {
            let [a, b] = [p, q].map(|x| pieces[x as usize].origin);
            return Err(KernelError::Invalid(CheckError::Hull(a, b)));
        }
        let split = tested.map_err(KernelError::Invalid)?;
        for (piece, split) in [p, q].into_iter().zip(split.pieces) {
            if split {
                leaves.push((pieces[piece as usize].leaf, Asked::Pair(i)));
            }
        }
    }
    // Stable, so each leaf keeps the first failure that asked for it.
    leaves.sort_by_key(|&(t, _)| t);
    leaves.dedup_by_key(|&mut (t, _)| t);
    if let Some(&(_, asked)) = leaves.iter().find(|&&(t, _)| !splittable(leaf(t), margin)) {
        let error = match asked {
            Asked::Fold(origin) => CheckError::Fold(origin),
            Asked::Pair(i) => {
                let [a, b] = pairs[i].map(|x| &pieces[x as usize]);
                let ids = [a.origin, b.origin];
                check_pair(ids, [&a.patch, &b.patch], [a.corners, b.corners], margin)
                    .expect_err("the pair failed")
            }
        };
        return Err(KernelError::Invalid(error));
    }
    Ok(leaves.into_iter().map(|(t, _)| t).collect())
}

/// Whether repair may split a leaf with `patch`: whether it is at least
/// [`MIN_SPLIT`] resolutions (`margin`) across along some axis of its
/// control points' box, or [`MIN_CURVED_SPLIT`] if it isn't [`flat`]
/// within the resolution. The box is the one the refiner measures.
fn splittable(patch: &Patch, margin: f64) -> bool {
    let bounds = patch.bounds();
    let size = (bounds.max - bounds.min).max_element();
    size >= MIN_SPLIT * margin || (size >= MIN_CURVED_SPLIT * margin && !flat(patch, margin))
}

/// The failure that asked for a leaf's split.
#[derive(Debug, Clone, Copy)]
enum Asked {
    /// The fold check, on a piece from this input triangle.
    Fold(u32),
    /// The hull rules, on the pair of this index.
    Pair(usize),
}

/// The first of `pairs` (by index), with what [`failures`] made of them,
/// found to be a failing pair of non-neighbours whose surfaces come
/// within the margin, if any.
///
/// A witness is looked for on one failing pair of each pair of input
/// triangles a round, the one whose corners' centroids are nearest (the
/// first of equals), and only before the first failure that can't be
/// mended, which names the error anyway. Chosen and charged
/// ([`WITNESS_WORK`] a search) sequentially, so the work doesn't depend on
/// the thread count.
fn witnessed(
    pieces: &[Piece],
    faces: &[Face],
    pairs: &[[u32; 2]],
    tested: &[Result<Split, CheckError>],
    margin: f64,
    work: &mut Work,
) -> Result<Option<usize>, KernelError> {
    let mut nearest: LookupMap<(u32, u32), (f64, usize)> = LookupMap::default();
    for (i, tested) in tested.iter().enumerate() {
        match tested {
            Err(_) => break,
            Ok(split) if split.witness => {}
            Ok(_) => continue,
        }
        let [a, b] = pairs[i].map(|x| &pieces[x as usize]);
        let centroid = |x: &Piece| (x.patch.p[0] + x.patch.p[1] + x.patch.p[2]) / 3.0;
        let near = centroid(a).distance_squared(centroid(b));
        let key = (a.origin.min(b.origin), a.origin.max(b.origin));
        let best = nearest.entry(key).or_insert((near, i));
        if near < best.0 {
            *best = (near, i);
        }
    }
    if nearest.is_empty() {
        return Ok(None);
    }
    // Sorted, so the hashed map's order doesn't matter.
    let mut chosen: Vec<usize> = nearest.into_values().map(|(_, i)| i).collect();
    chosen.sort_unstable();
    work.spend(chosen.len().saturating_mul(WITNESS_WORK))?;
    let found = par_map(&chosen, |&i| {
        let [p, q] = pairs[i];
        witness_limit(pieces, faces, [p, q], margin).is_some_and(|limit| {
            surfaces_within(&pieces[p as usize].patch, &pieces[q as usize].patch, limit)
        })
    });
    Ok(chosen.into_iter().zip(found).find(|f| f.1).map(|f| f.0))
}

/// What to do with a pair of pieces that passes, or fails in a way a
/// split may mend.
#[derive(Debug, Clone, Copy)]
struct Split {
    /// Which of the two to split.
    pieces: [bool; 2],
    /// Whether it is a failing pair of non-neighbours, not both flat, for
    /// which a witness may show that no split mends it.
    witness: bool,
}

impl Split {
    /// A pair that passes.
    const NONE: Split = Split {
        pieces: [false; 2],
        witness: false,
    };
}

/// The distance within which points of the surfaces of the non-neighbour
/// pieces `p` and `q` show that no split can mend them, if some is: the
/// margin less [`ROUNDING`] and how far planar splits may move a surface.
/// `None` where the pieces' leaves touch, or nothing is left.
///
/// The argument: say `x` on `p` and `y` on `q` are less than the margin
/// apart. The pieces refinement makes of `p`'s leaf cover it, with exact
/// splits, so some piece of every later mesh holds `x`, lies in that leaf,
/// and has `x` in its hull; the same for `y`. If the two leaves share no
/// vertex (the pieces form a conforming mesh, so leaves that touch share a
/// vertex of their pieces), those later pieces share none either: they
/// are non-neighbours whose hulls come within the margin, at every depth.
/// So repair could never pass.
///
/// A leaf on a [`Surface::Plane`] face is split with straight inner edges
/// instead, whose pieces cover what it covers seen along the plane's
/// normal (if it doesn't fold: so its pieces must pass the fold check)
/// and keep their control points in the hull of the leaf's pieces now. A
/// point of the piece and the point of a later piece over it then differ
/// by no more than that hull's thickness along the normal, which comes off
/// the limit.
fn witness_limit(pieces: &[Piece], faces: &[Face], [p, q]: [u32; 2], margin: f64) -> Option<f64> {
    let [p, q] = [p as usize, q as usize];
    let leaf = |x: usize| {
        let piece = &pieces[x];
        // The other half of a green piece: pieces are leaf by leaf.
        let sibling = [x.wrapping_sub(1), x + 1]
            .into_iter()
            .find(|&s| s < pieces.len() && pieces[s].leaf == piece.leaf);
        (piece, sibling.map(|s| &pieces[s]))
    };
    let (a, a2) = leaf(p);
    let (b, b2) = leaf(q);
    let corners = |x: &Piece, x2: Option<&Piece>| {
        let [c0, c1, c2] = x.corners;
        let [d0, d1, d2] = x2.map_or(x.corners, |x2| x2.corners);
        [c0, c1, c2, d0, d1, d2]
    };
    let (ca, cb) = (corners(a, a2), corners(b, b2));
    let shared = ca.iter().any(|v| cb.contains(v));
    if shared {
        return None;
    }
    let scale = [a, b]
        .into_iter()
        .flat_map(|x| x.patch.hull())
        .fold(0.0, |m: f64, x| m.max(x.abs().max_element()));
    let limit = margin - ROUNDING * scale - thickness(faces, a, a2) - thickness(faces, b, b2);
    (limit > 0.0).then_some(limit)
}

/// How far apart along the normal of the plane its face is tagged with
/// the control points of `piece` and its other half `sibling` lie; 0 off
/// a plane, and infinite for a plane that isn't well defined or for
/// pieces that fail the fold check.
fn thickness(faces: &[Face], piece: &Piece, sibling: Option<&Piece>) -> f64 {
    let Surface::Plane { n, .. } = faces[piece.face as usize].surface else {
        return 0.0;
    };
    let folds = |x: &Piece| x.patch.fold_direction().is_none();
    if folds(piece) || sibling.is_some_and(folds) {
        return f64::INFINITY;
    }
    // Scaled as `Surface::distance` scales it, so any finite size works.
    let scale = n.abs().max_element();
    if !(scale > 0.0 && scale.is_finite()) {
        return f64::INFINITY;
    }
    let n = n / scale;
    let n = n / n.length();
    let origin = piece.patch.p[0];
    let (lo, hi) = piece
        .patch
        .hull()
        .into_iter()
        .chain(sibling.into_iter().flat_map(|s| s.patch.hull()))
        .map(|x| n.dot(x - origin))
        .fold((0.0, 0.0), |(lo, hi): (f64, f64), h| (lo.min(h), hi.max(h)));
    hi - lo
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

#[cfg(test)]
mod tests;
mod witness;
