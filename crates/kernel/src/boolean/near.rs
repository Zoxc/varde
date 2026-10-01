//! Whether two operands' surfaces come within a distance of each other:
//! their patches split until each pair's control hulls are apart, or both
//! pieces are flat. What [`touches`](super::touches) asks of curved
//! operands whose counting shows no crossing and no vertex inside.
//!
//! The search ([`search`]) runs depth first over pairs of pieces, from
//! the broad phase's pairs of patches in order, and a visit decides each
//! pair: drop it, stop, or split one piece or both with
//! [`Patch::split4`] and visit the pieces' pairs. [`near`]'s visit drops
//! a pair whose hulls are more than the distance apart (GJK,
//! [`apart`]) and stops on a pair of [`settled`] pieces. A minimum
//! distance is the same search with a bound in place of the yes or no:
//! drop the pairs whose hulls are further apart than the closest points
//! found so far, and settle the rest.
//!
//! **Why the answer holds.** A patch lies in the convex hull of its six
//! control points (its weights are positive, which `check` sees to), and
//! so does each of its pieces. So surfaces within the distance `d` are
//! never dropped, and a `false` means every point of one is more than `d`
//! from every point of the other. A `true` means two pieces' hulls come
//! within `d`, and each piece is flat within `d/4`: its control points
//! within `d/4` of its corners' plane and its edges' control points
//! within `d/4` of their chords, so every point of its hull is within
//! about `d/2` of the piece (the piece covers its corners' triangle, give
//! or take `d/4`, and the hull holds little more where the edges' control
//! points lie between their ends, as on pieces of smooth patches). So the
//! surfaces come within about `2d`. Pieces stop at a size floor too
//! ([`MIN_SPLIT`] times `d` across), where the bound is the floor piece's
//! sag instead: that takes surfaces curving tighter than some hundreds of
//! `d`. A `true` further than `2d` is only ever a pair kept that needn't
//! have been, never a contact missed.
//!
//! Every visit is charged ([`NEAR_WORK`]), sequentially, so the answer
//! and the work spent are the same at any thread count. No trig.

use super::input::{Input, planar};
use crate::KernelError;
use crate::budget::Work;
use crate::mesh::{MIN_SPLIT, apart, straight};
use crate::patch::Patch;

/// The work of one visit, in units of about half a microsecond: a GJK
/// test of two six-point hulls, two flatness tests and a share of the
/// splits. Measured at 0.24 to 0.44 µs a visit (release, one thread), on
/// cylinders side by side (2 and 100 long), a pin against a hole's wall
/// and their near misses, at both ends of the fit tolerance.
pub(crate) const NEAR_WORK: usize = 1;

/// How many visits run between spending their work.
const NEAR_CHUNK: usize = 256;

/// Which pieces of a pair to split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
    First,
    Second,
    Both,
}

/// What a visit makes of a pair of pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Nothing more to look for in this pair.
    Drop,
    /// The search is over.
    Stop,
    /// Split these pieces into four each and visit the pairs they make.
    Split(Which),
}

/// Visits pairs of pieces depth first, from each pair of `roots` in
/// order, as `visit` says (see the [module](self) docs), and says
/// whether a visit stopped it. A split pair's pieces are visited in a
/// fixed order (the first's four pieces in [`Patch::split4`]'s order,
/// each against the second's in theirs) before the next pair, so the
/// visits and the work charged are the same every time.
///
/// Every visit costs [`NEAR_WORK`], spent each [`NEAR_CHUNK`] visits and
/// at the end; a piece `split4` can't split is
/// [`KernelError::TooComplex`].
pub(crate) fn search(
    roots: impl IntoIterator<Item = [Patch; 2]>,
    mut visit: impl FnMut(&[Patch; 2]) -> Step,
    work: &mut Work,
) -> Result<bool, KernelError> {
    let mut stack: Vec<[Patch; 2]> = Vec::new();
    let mut visits = 0usize;
    let mut stopped = false;
    'roots: for root in roots {
        stack.push(root);
        while let Some(pair) = stack.pop() {
            visits += 1;
            if visits == NEAR_CHUNK {
                work.spend(NEAR_CHUNK * NEAR_WORK)?;
                visits = 0;
            }
            let which = match visit(&pair) {
                Step::Drop => continue,
                Step::Stop => {
                    stopped = true;
                    break 'roots;
                }
                Step::Split(which) => which,
            };
            let pieces = |patch: &Patch, split: bool| -> Result<([Patch; 4], usize), KernelError> {
                if split {
                    let pieces = patch.split4().map_err(|_| KernelError::TooComplex)?;
                    Ok((pieces, 4))
                } else {
                    Ok(([*patch; 4], 1))
                }
            };
            let (xs, nx) = pieces(&pair[0], which != Which::Second)?;
            let (ys, ny) = pieces(&pair[1], which != Which::First)?;
            // Pushed last to first, so popped first to last.
            for x in xs[..nx].iter().rev() {
                for y in ys[..ny].iter().rev() {
                    stack.push([*x, *y]);
                }
            }
        }
    }
    work.spend(visits * NEAR_WORK)?;
    Ok(stopped)
}

/// Whether a piece needs no more splitting in a search within `within`:
/// it is flat within a quarter of it (its control points within `within
/// / 4` of its corners' plane, and each edge's control point within
/// `within / 4` of the edge's chord), or no larger than `floor` across.
pub(crate) fn settled(patch: &Patch, within: f64, floor: f64) -> bool {
    let b = patch.bounds();
    if (b.max - b.min).max_element() <= floor {
        return true;
    }
    let margin = within / 4.0;
    planar(patch, margin)
        && (0..3).all(|i| straight(patch.p[i], patch.c[i], patch.p[(i + 1) % 3], margin))
}

/// Whether the surfaces of `a` and `b` come within `within` of each other
/// anywhere in the pairs of triangles `pairs` (the broad phase's, whose
/// boxes come within `within`): `false` only if they don't, `true` only
/// if they come within about twice it (see the [module](self) docs).
/// Pieces stop at [`MIN_SPLIT`] times `within` across.
pub(super) fn near(
    a: &Input,
    b: &Input,
    pairs: &[[u32; 2]],
    within: f64,
    work: &mut Work,
) -> Result<bool, KernelError> {
    let floor = MIN_SPLIT * within;
    let roots = pairs
        .iter()
        .map(|&[p, q]| [a.patches[p as usize], b.patches[q as usize]]);
    search(
        roots,
        |[x, y]| {
            if apart(&x.hull(), &y.hull(), within) {
                return Step::Drop;
            }
            match (settled(x, within, floor), settled(y, within, floor)) {
                (true, true) => Step::Stop,
                (true, false) => Step::Split(Which::Second),
                (false, true) => Step::Split(Which::First),
                (false, false) => Step::Split(Which::Both),
            }
        },
        work,
    )
}

#[cfg(test)]
mod tests;
