//! A witness that two patches' surfaces come within a distance: a point of
//! each, evaluated, closer than it.
//!
//! Repair uses it on a failing pair of non-neighbours: if their surfaces
//! come within the margin, no split mends the pair (see
//! [`failures`](super::failures)). Finding the points is a search, which
//! may miss; what it finds is always a pair of evaluated points of the two
//! patches, so a claim never rests on the search converging. It is plain
//! `f64` arithmetic (no transcendental functions), the same bits natively
//! and on wasm.

use glam::DVec3;

use super::super::check::samples;
use crate::patch::Patch;

/// The most Gauss–Newton steps one search takes after its seed.
const MAX_STEPS: usize = 30;

/// The most a step is stretched, doubling it while that comes closer.
const MAX_STRETCH: f64 = 64.0;

/// The smallest damping the steps use. The normal equations of four
/// parameters against a three-dimensional distance are singular, so they
/// are never solved undamped.
const MIN_DAMPING: f64 = 1e-6;

/// Rounding the witness allows for, relative to the largest coordinate of
/// the two patches (repair takes the pieces of their leaves): in evaluating the points, in the pieces refinement
/// makes of the patches (each split rounds its new points by a few ulps,
/// over up to [`MAX_REFINE_DEPTH`](crate::MAX_REFINE_DEPTH) splits), and
/// in the GJK test those pieces later face. Too much only sends a pair
/// back to splitting; at the finest resolution it leaves room for a
/// witness out to about `1.5e5` from the origin.
pub(super) const ROUNDING: f64 = 128.0 * f64::EPSILON;

/// Whether a point of `a` and a point of `b` were found less than `limit`
/// apart, both evaluated with [`Patch::eval`]. The search: the closest
/// pair among the 15 [`samples`] of each, then damped Gauss–Newton
/// (Levenberg–Marquardt) on the four barycentric coordinates, kept in the
/// triangles, each step stretched by doubling while that comes closer, for
/// at most [`MAX_STEPS`] steps, or until it stops making progress. A
/// `limit` that isn't positive finds nothing, and NaN never passes.
pub(super) fn surfaces_within(a: &Patch, b: &Patch, limit: f64) -> bool {
    if !(limit > 0.0 && limit.is_finite()) {
        return false;
    }
    let limit2 = limit * limit;
    let mut grid = [DVec3::ZERO; 15];
    for (g, u) in grid.iter_mut().zip(samples()) {
        *g = u;
    }
    let pa = grid.map(|u| a.eval(u));
    let pb = grid.map(|u| b.eval(u));
    let mut best = (f64::INFINITY, 0, 0);
    for (i, p) in pa.iter().enumerate() {
        for (j, q) in pb.iter().enumerate() {
            let d = p.distance_squared(*q);
            if d < best.0 {
                best = (d, i, j);
            }
        }
    }
    if best.0 < limit2 {
        return true;
    }
    if !best.0.is_finite() {
        return false;
    }
    let (mut x, mut y) = (grid[best.1], grid[best.2]);
    let mut current = best.0;
    let mut damping = 1e-3;
    for _ in 0..MAX_STEPS {
        let [p, pu, pv] = a.eval_derivs(x);
        let [q, qu, qv] = b.eval_derivs(y);
        let r = p - q;
        let cols = [pu, pv, -qu, -qv];
        let mut m = [[0.0; 4]; 4];
        let mut rhs = [0.0; 4];
        for i in 0..4 {
            for k in 0..4 {
                m[i][k] = cols[i].dot(cols[k]);
            }
            rhs[i] = -cols[i].dot(r);
        }
        // Damped in proportion to each parameter's own scale, and a
        // little in proportion to the largest, so that a parameter that
        // moves nothing (a degenerate patch) stays put.
        let size = (0..4).map(|i| m[i][i]).fold(0.0, f64::max);
        for (i, row) in m.iter_mut().enumerate() {
            row[i] += damping * (row[i] + 1e-6 * size);
        }
        let Some(d) = solve4(m, rhs) else {
            break;
        };
        // The step, then twice, four times ... as long as that comes
        // closer: at a tangential touch the steps fall short by a steady
        // factor, and convergence would otherwise be slow.
        let at = |k: f64| {
            let nx = inside(x.x + k * d[0], x.y + k * d[1]);
            let ny = inside(y.x + k * d[2], y.y + k * d[3]);
            (a.eval(nx).distance_squared(b.eval(ny)), nx, ny)
        };
        let (mut dist, mut nx, mut ny) = at(1.0);
        let mut k = 1.0;
        while dist < current && k < MAX_STRETCH {
            k *= 2.0;
            let further = at(k);
            // Written so that NaN stops.
            if further.0 < dist {
                (dist, nx, ny) = further;
            } else {
                break;
            }
        }
        if dist < limit2 {
            return true;
        }
        if dist < current {
            let gain = current - dist;
            (x, y, current) = (nx, ny, dist);
            damping = (damping * 0.3).max(MIN_DAMPING);
            // A slow approach (surfaces about parallel and apart, which
            // the closest points slide along) won't get there.
            if gain <= 1e-3 * current {
                break;
            }
        } else {
            damping *= 10.0;
            if damping > 1e8 {
                break;
            }
        }
    }
    false
}

/// The barycentric point `(u0, u1, 1 − u0 − u1)` moved into the closed
/// triangle: negative coordinates to zero, then scaled down if the two
/// sum past 1. NaN goes to zero.
fn inside(u0: f64, u1: f64) -> DVec3 {
    let (mut u0, mut u1) = (u0.max(0.0), u1.max(0.0));
    let sum = u0 + u1;
    if sum > 1.0 {
        (u0, u1) = (u0 / sum, u1 / sum);
    }
    DVec3::new(u0, u1, (1.0 - u0 - u1).max(0.0))
}

/// `m·x = rhs` by Gaussian elimination with partial pivoting, or `None`
/// for a pivot that is zero, tiny next to the largest diagonal entry, or
/// not finite.
fn solve4(mut m: [[f64; 4]; 4], mut rhs: [f64; 4]) -> Option<[f64; 4]> {
    let size = (0..4).map(|i| m[i][i].abs()).fold(0.0, f64::max);
    if !(size > 0.0 && size.is_finite()) {
        return None;
    }
    for c in 0..4 {
        let mut pivot = c;
        for i in c + 1..4 {
            if m[i][c].abs() > m[pivot][c].abs() {
                pivot = i;
            }
        }
        m.swap(c, pivot);
        rhs.swap(c, pivot);
        // Written so that NaN fails.
        let usable = m[c][c].abs() > 1e-14 * size;
        if !usable {
            return None;
        }
        for i in c + 1..4 {
            let f = m[i][c] / m[c][c];
            for k in c..4 {
                m[i][k] -= f * m[c][k];
            }
            rhs[i] -= f * rhs[c];
        }
    }
    let mut x = [0.0; 4];
    for i in (0..4).rev() {
        let mut v = rhs[i];
        for k in i + 1..4 {
            v -= m[i][k] * x[k];
        }
        x[i] = v / m[i][i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

#[cfg(test)]
mod tests;
