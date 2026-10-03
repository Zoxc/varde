//! Whether two operands' surfaces come within a distance of each other:
//! their patches split until each pair's control hulls are apart, or both
//! pieces are flat. What [`touches`](super::touches) asks of curved
//! operands whose counting shows no crossing and no vertex inside
//! ([`touching`]).
//!
//! The search ([`search`]) runs depth first over pairs of pieces, from
//! the broad phase's pairs of patches in order, and a visit decides each
//! pair: drop it, stop, or split one piece or both with
//! [`Patch::split4`] and visit the pieces' pairs. [`near`]'s visit drops
//! a pair whose hulls are more than the distance apart (GJK,
//! [`apart`], or for settled pieces [`apart_across`]) and stops on a
//! pair of [`settled`] pieces. A minimum distance
//! ([`measure::distance`](crate::measure::distance)) is the same search
//! with a bound in place of the yes or no: it drops the pairs whose hulls
//! are further apart than the closest points found so far, less the
//! resolution, and settles the rest.
//!
//! **Why the answer holds.** A patch lies in the convex hull of its six
//! control points (its weights are positive, which `check` sees to), and
//! so does each of its pieces. So surfaces within the distance `d` are
//! never dropped, and a `false` means every point of one is more than `d`
//! from every point of the other, up to the splits' rounding (a few ulps
//! of the coordinates a split, which nears `d` only at the finest
//! tolerance some `1e5` from the origin). A `true` means two pieces'
//! hulls come within `d`, and each piece is flat within `d/4`: its
//! control points within `d/4` of its corners' plane and its edges'
//! control points within `d/4` of their chords. Then every point of its
//! hull is within `d/√2` of the piece: within `d/2` along the plane's
//! normal (both lie within `d/4` of the plane), and within `d/2` across
//! it, as the piece covers its corners' triangle but for a band `d/4`
//! wide along its sides, whose curves lie within `d/4` of them (where
//! the edges' control points lie between their ends, as on pieces of
//! smooth patches). So the surfaces come within `(1 + √2)·d`, about
//! `2.4d`, up to GJK's rounding where neither piece's plane separates
//! the hulls ([`apart_across`]). Pieces stop at a size floor too
//! ([`MIN_SPLIT`] times `d` across), where the bound is the floor
//! piece's sag instead: that takes surfaces curving tighter than some
//! hundreds of `d`. A `true` further
//! than `d` is only ever a pair kept that needn't have been, never a
//! contact missed.
//!
//! Every visit is charged ([`NEAR_WORK`]), sequentially, so the answer
//! and the work spent are the same at any thread count. No trig.

use glam::DVec3;

use super::input::{Input, planar};
use super::pairs;
use crate::budget::Work;
use crate::mesh::{MIN_SPLIT, apart, flat};
use crate::patch::Patch;
use crate::{Failure, KernelError, Tolerance};

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

/// Visits pairs of pieces depth first, from each of `roots` in order, as
/// `visit` says (see the [module](self) docs), and says whether a visit
/// stopped it. A pair `visit` splits is handed to `split`, which pushes
/// its children onto the vector it is given in the order they are to be
/// visited, and they are all visited before the next pair, so the visits
/// and the work charged are the same every time. `visit` may change the
/// pair before it is split (what its children inherit) and spend work of
/// its own.
///
/// Every visit costs [`NEAR_WORK`], spent each [`NEAR_CHUNK`] visits and
/// at the end.
pub(crate) fn search<P>(
    roots: impl IntoIterator<Item = P>,
    mut visit: impl FnMut(&mut P, &mut Work) -> Result<Step, KernelError>,
    mut split: impl FnMut(&P, Which, &mut Vec<P>) -> Result<(), KernelError>,
    work: &mut Work,
) -> Result<bool, KernelError> {
    let mut stack: Vec<P> = Vec::new();
    let mut children: Vec<P> = Vec::new();
    let mut visits = 0usize;
    let mut stopped = false;
    'roots: for root in roots {
        stack.push(root);
        while let Some(mut pair) = stack.pop() {
            visits += 1;
            if visits == NEAR_CHUNK {
                work.spend(NEAR_CHUNK * NEAR_WORK)?;
                visits = 0;
            }
            let which = match visit(&mut pair, work)? {
                Step::Drop => continue,
                Step::Stop => {
                    stopped = true;
                    break 'roots;
                }
                Step::Split(which) => which,
            };
            children.clear();
            split(&pair, which, &mut children)?;
            // Pushed last to first, so popped first to last.
            stack.extend(children.drain(..).rev());
        }
    }
    work.spend(visits * NEAR_WORK)?;
    Ok(stopped)
}

/// The children of a pair of patches' pieces for [`search`]: the first's
/// four [`Patch::split4`] pieces (or itself, if it isn't split), each
/// against the second's in theirs. A piece `split4` can't split is
/// [`KernelError::TooComplex`].
fn split_patches(
    pair: &[Patch; 2],
    which: Which,
    out: &mut Vec<[Patch; 2]>,
) -> Result<(), KernelError> {
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
    for x in &xs[..nx] {
        for y in &ys[..ny] {
            out.push([*x, *y]);
        }
    }
    Ok(())
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
    planar(patch, within / 4.0) && flat(patch, within / 4.0)
}

/// Whether the control hulls of `x` and `y` are more than `within`
/// apart along the normal of either's corners' plane: for a pair of
/// settled pieces [`apart`] calls near. GJK's direction to the closest
/// points rounds relative to the hulls' size over their distance, so on
/// a small piece by a large flat face (a ball by a slab, at the finest
/// tolerance) it stopped short at some 3 resolutions; near a tangency
/// the pieces' own planes are the direction it was after.
fn apart_across(x: &Patch, y: &Patch, within: f64) -> bool {
    apart_along(
        &x.hull(),
        &y.hull(),
        [corner_normal(x), corner_normal(y)].into_iter().flatten(),
        within,
    )
}

/// The unit normal of `patch`'s corners' plane, if they span one.
pub(crate) fn corner_normal(patch: &Patch) -> Option<DVec3> {
    let [p0, p1, p2] = patch.p;
    (p1 - p0).cross(p2 - p0).try_normalize()
}

/// Whether the convex hulls of the points `x` and `y` (neither empty) are
/// more than `within` apart along one of the unit `normals`: a lower
/// bound on their distance where [`apart`]'s rounds. Measured from a
/// point of `x`, so the rounding is relative to the hulls' size, not to
/// their distance from the origin.
pub(crate) fn apart_along(
    x: &[DVec3],
    y: &[DVec3],
    normals: impl IntoIterator<Item = DVec3>,
    within: f64,
) -> bool {
    let origin = x[0];
    let span = |points: &[DVec3], n: DVec3| {
        points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                let along = (*p - origin).dot(n);
                (lo.min(along), hi.max(along))
            })
    };
    normals.into_iter().any(|n| {
        let ((xlo, xhi), (ylo, yhi)) = (span(x, n), span(y, n));
        ylo - xhi > within || xlo - yhi > within
    })
}

/// Whether `a` and `b`, one of them with curved patches, touch or
/// overlap, for [`touches`](super::touches): one counting
/// ([`pairs::counted`], what [`pairs::refined_with`] counts first), `true`
/// where it shows an edge through a face or a vertex inside, else
/// whether their surfaces come within the resolution ([`near`]) in the
/// broad phase's pairs, whose margin is the resolution too. A counting
/// whose decisions don't fit together is counted again with no near
/// ties, as the operation's is ([`super::tied_or_exact`]), and fails with
/// what it is about if that doesn't fit either.
pub(super) fn touching(
    a: &Input,
    b: &Input,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<bool, Failure> {
    let counts = super::tied_or_exact(super::tie(tol), work, |tie, work| {
        pairs::counted(a, b, true, tie, tol, work)
    })?;
    if counts.meet() {
        return Ok(true);
    }
    Ok(near(a, b, &counts.pairs, tol.resolution(), work)?)
}

/// Whether the surfaces of `a` and `b` come within `within` of each other
/// anywhere in the pairs of triangles `pairs` (the broad phase's, whose
/// boxes come within `within`): `false` only if they don't, `true` only
/// if they come within about 2.4 times it (see the [module](self) docs).
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
        |[x, y], _| {
            if apart(&x.hull(), &y.hull(), within) {
                return Ok(Step::Drop);
            }
            Ok(
                match (settled(x, within, floor), settled(y, within, floor)) {
                    (true, true) if apart_across(x, y, within) => Step::Drop,
                    (true, true) => Step::Stop,
                    (true, false) => Step::Split(Which::Second),
                    (false, true) => Step::Split(Which::First),
                    (false, false) => Step::Split(Which::Both),
                },
            )
        },
        split_patches,
        work,
    )
}

#[cfg(test)]
mod tests;
