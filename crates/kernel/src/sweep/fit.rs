//! Fitted strips, and how far a patch is from a form.

use glam::{DMat4, DVec3, DVec4};

use super::strip;
use crate::mesh::Form;
use crate::patch::{Conic3, Patch, PatchError, W_MAX, W_MIN};

/// A strip fitted to a form, and how far its patches are from it (the
/// larger of their [`deviation`]s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fitted {
    pub patches: [Patch; 2],
    pub error: f64,
}

/// The strip between the four given edges (as for
/// [`revolution_strip`](super::revolution_strip): `bottom` from `a0` to
/// `a1`, `top` from `b0` to `b1`, `left` from `a0` to `b0`, `right` from
/// `a1` to `b1`), its diagonal from `a0` to `b1` fitted to `form`, and how
/// far it is from it.
///
/// The diagonal's control point and weight (four numbers: the control
/// point times the weight, and the weight, the conic's homogeneous middle
/// point) are fitted by damped Gauss–Newton (Levenberg–Marquardt) on the
/// form's signed distance at a grid of points of both patches, starting
/// from the parabola through `a0`, `b1` and the form's point nearest the
/// strip's middle; a fixed number of steps at most, so the same input
/// gives the same bits. Then each patch is measured by [`deviation`].
///
/// The edges must meet at the corners to the bit
/// ([`PatchError::Mismatch`]); a form with no signed distance where the
/// strip is ([`Form::Unknown`], or the strip on an axis), or no valid
/// diagonal, is [`PatchError::Degenerate`]. Whether the patches pass the
/// fold check is the caller's to ask: a strip whose patches fold is
/// halved, as one too far off is.
pub fn fitted_strip(
    bottom: &Conic3,
    top: &Conic3,
    left: &Conic3,
    right: &Conic3,
    form: &Form,
) -> Result<Fitted, PatchError> {
    fitted_strip_touching(bottom, top, left, right, form, None)
}

/// [`fitted_strip`] with the diagonal, if `touch` is given, leaving its
/// end `a0` (`true`) or `b1` (`false`) square to the unit `n`: in the
/// plane through that end square to `n` (its control point is fitted in
/// that plane). A ring at a turn of a surface of revolution's meridian
/// is where the surface touches the ring's plane, and an exact diagonal
/// leaves it in that plane; so must a fitted one, or its control point
/// dips under the plane, which the hull rule between the strip and a
/// face on the plane's other side refuses.
pub(crate) fn fitted_strip_touching(
    bottom: &Conic3,
    top: &Conic3,
    left: &Conic3,
    right: &Conic3,
    form: &Form,
    touch: Option<(bool, DVec3)>,
) -> Result<Fitted, PatchError> {
    for edge in [bottom, top, left, right] {
        edge.check()?;
    }
    let (a0, a1, b0, b1) = (bottom.p0, bottom.p1, top.p0, top.p1);
    if left.p0 != a0 || left.p1 != b0 || right.p0 != a1 || right.p1 != b1 {
        return Err(PatchError::Mismatch);
    }
    let touch = touch.map(|(start, n)| (if start { a0 } else { b1 }, n));
    let diagonal = fit_diagonal(bottom, top, left, right, form, touch)?;
    let patches = strip(bottom, top, left, right, &diagonal)?;
    let error = deviation(&patches[0], form).max(deviation(&patches[1], form));
    Ok(Fitted { patches, error })
}

/// Grid steps per side of the points each patch is fitted at.
const FIT_GRID: usize = 8;
/// Most Gauss–Newton steps.
const FIT_STEPS: usize = 40;
/// Most tries of one step with growing damping.
const FIT_TRIES: usize = 12;

/// The fitted diagonal (see [`fitted_strip`]), its control point in the
/// plane through `touch`'s point square to its unit normal if given.
fn fit_diagonal(
    bottom: &Conic3,
    top: &Conic3,
    left: &Conic3,
    right: &Conic3,
    form: &Form,
    touch: Option<(DVec3, DVec3)>,
) -> Result<Conic3, PatchError> {
    let (a0, a1, b0, b1) = (bottom.p0, bottom.p1, top.p0, top.p1);
    // Everything relative to the strip's middle, so rounding is relative
    // to its size.
    let origin = (a0 + a1 + b0 + b1) * 0.25;
    let middle = nearest(form, origin).ok_or(PatchError::Degenerate)?;
    // The parabola (weight 1) through `a0`, `b1` and `middle` at its
    // half: `(a0 + 2c + b1)/4 = middle`.
    // In the plane `(c − p)·n = 0`: homogeneously `g·h = 0` with `g =
    // (n, −(p − origin)·n)`, which the start and every step keep.
    let onto = |c: DVec3| match touch {
        Some((p, n)) => c - n * (c - p).dot(n),
        None => c,
    };
    let g = touch.map(|(p, n)| n.extend(-(p - origin).dot(n)));
    let start = Conic3::new(a0, onto(middle * 2.0 - (a0 + b1) * 0.5), 1.0, b1)?;
    let patches = strip(bottom, top, left, right, &start)?;
    // Each patch's point at `u` is `(A + k·h)` homogeneously, `h` the
    // diagonal's middle homogeneous point and `k = 2·ui·uj` of the
    // diagonal's corners `i`, `j`: corners 2 and 0 of the first patch, 0
    // and 1 of the second.
    let mut samples: Vec<(DVec4, f64)> = Vec::new();
    for (patch, (i, j)) in patches.iter().zip([(2, 0), (0, 1)]) {
        let mut net = patch.net();
        for row in &mut net {
            for h in row.iter_mut() {
                *h = (h.truncate() - origin * h.w).extend(h.w);
            }
        }
        net[i][j] = DVec4::ZERO;
        net[j][i] = DVec4::ZERO;
        for u in grid(FIT_GRID) {
            let k = 2.0 * u[i] * u[j];
            if k > 0.0 {
                let mut x = DVec4::ZERO;
                for (r, row) in net.iter().enumerate() {
                    for (c, h) in row.iter().enumerate() {
                        x += *h * (u[r] * u[c]);
                    }
                }
                samples.push((x, k));
            }
        }
    }
    // The residuals at `h` (signed distances) and their gradients.
    let residuals = |h: DVec4| -> Option<(Vec<f64>, Vec<DVec4>)> {
        if !(W_MIN..=W_MAX).contains(&h.w) {
            return None;
        }
        let mut r = Vec::with_capacity(samples.len());
        let mut jacobian = Vec::with_capacity(samples.len());
        for &(a, k) in &samples {
            let x = a + h * k;
            if x.w.is_nan() || x.w <= 0.0 {
                return None;
            }
            let p = x.truncate() / x.w;
            let (d, n) = form.signed(origin + p)?;
            r.push(d);
            jacobian.push(n.extend(-n.dot(p)) * (k / x.w));
        }
        Some((r, jacobian))
    };
    let cost = |r: &[f64]| r.iter().map(|d| d * d).sum::<f64>();
    let mut h = ((start.c - origin) * start.w).extend(start.w);
    let (mut r, mut jacobian) = residuals(h).ok_or(PatchError::Degenerate)?;
    let mut now = cost(&r);
    let mut damping = 1e-3;
    for _ in 0..FIT_STEPS {
        if now == 0.0 {
            break;
        }
        let mut normal = DMat4::ZERO;
        let mut rhs = DVec4::ZERO;
        for (d, g) in r.iter().zip(&jacobian) {
            normal += outer4(*g, *g);
            rhs -= *g * *d;
        }
        let diagonal = DVec4::new(
            normal.x_axis.x,
            normal.y_axis.y,
            normal.z_axis.z,
            normal.w_axis.w,
        );
        let mut moved = false;
        for _ in 0..FIT_TRIES {
            let damped = normal + DMat4::from_diagonal(diagonal * damping);
            let inverse = damped.inverse();
            let mut step = inverse * rhs;
            // The least step in the constraint's plane (a Lagrange
            // multiplier on `g·step = 0`).
            if let Some(g) = g {
                let across = inverse * g;
                step -= across * (g.dot(step) / g.dot(across));
            }
            if !step.is_finite() {
                damping *= 8.0;
                continue;
            }
            let next = h + step;
            if let Some((r2, j2)) = residuals(next) {
                let after = cost(&r2);
                if after < now {
                    let gain = now - after;
                    (h, r, jacobian) = (next, r2, j2);
                    now = after;
                    damping = (damping / 4.0).max(1e-12);
                    // Settled once a step gains under a millionth.
                    moved = gain > 1e-6 * now;
                    break;
                }
            }
            damping *= 8.0;
        }
        if !moved {
            break;
        }
    }
    Conic3::new(a0, onto(origin + h.truncate() / h.w), h.w, b1)
}

/// `a·bᵀ`.
fn outer4(a: DVec4, b: DVec4) -> DMat4 {
    DMat4::from_cols(b * a.x, b * a.y, b * a.z, b * a.w).transpose()
}

/// The form's point nearest `x`, by a few steps along its normal.
fn nearest(form: &Form, mut x: DVec3) -> Option<DVec3> {
    for _ in 0..4 {
        let (d, n) = form.signed(x)?;
        x -= n * d;
    }
    Some(x)
}

/// The barycentric grid with `n` steps a side, in a fixed order: each
/// coordinate a whole number of steps, so the ones on an edge are zero
/// exactly (`1 − a − b` can round below it).
fn grid(n: usize) -> impl Iterator<Item = [f64; 3]> {
    (0..=n)
        .flat_map(move |i| (0..=n - i).map(move |j| [i, j, n - i - j].map(|k| k as f64 / n as f64)))
}

/// Grid steps per side of the points [`deviation`] starts from.
const MEASURE_GRID: usize = 12;
/// How many of the grid's peaks it climbs from.
const MEASURE_STARTS: usize = 4;
/// How many times it halves its step.
const MEASURE_HALVINGS: usize = 20;
/// Most steps of one climb, moves and halvings together.
const MEASURE_MOVES: usize = 400;

/// How far `patch` is from `form`: the largest [`Form::distance`] found
/// over the patch, NaN counted as infinite. The distance is taken on a
/// grid of 12 steps a side (91 points, edges included); then from each of
/// the grid's four farthest peaks (points no grid neighbour beats, so
/// separate maxima each get a climb rather than four starts round one;
/// the farthest points fill in where there are fewer peaks) a compass
/// search climbs to the local maximum over the triangle (moving along the
/// six directions of the grid by a step that starts at one grid step and
/// is halved, 20 times, whenever no move gains), staying on the triangle,
/// its edges included. The same patch and form give the same bits.
///
/// It is the maximum found, not a certified bound: a maximum no grid
/// point leads to would be missed. A fitted patch's error is smooth over
/// it, vanishing on its exact edges, with a few extrema the size of the
/// patch, so the climbs land on its maxima to about `2⁻²⁰` of a grid
/// step (on tori, elliptic tori and caps it was never under grids of 200
/// steps a side with random points besides); the bands and caps of a
/// [`Lathe`](super::Lathe) keep a margin under their limit for what is
/// left.
pub fn deviation(patch: &Patch, form: &Form) -> f64 {
    let net = patch.net();
    let at = |u: [f64; 3]| {
        // Summed row by row, written out rather than looped: the dev
        // profile leaves the iterators as calls, and this is the inner
        // loop of every fit.
        let mut x = DVec4::ZERO;
        x += net[0][0] * (u[0] * u[0]);
        x += net[0][1] * (u[0] * u[1]);
        x += net[0][2] * (u[0] * u[2]);
        x += net[1][0] * (u[1] * u[0]);
        x += net[1][1] * (u[1] * u[1]);
        x += net[1][2] * (u[1] * u[2]);
        x += net[2][0] * (u[2] * u[0]);
        x += net[2][1] * (u[2] * u[1]);
        x += net[2][2] * (u[2] * u[2]);
        let d = form.distance(x.truncate() / x.w);
        if d.is_nan() { f64::INFINITY } else { d }
    };
    let points: Vec<([f64; 3], f64)> = grid(MEASURE_GRID).map(|u| (u, at(u))).collect();
    // Peaks first, then the rest; each farthest first, ties in grid
    // order.
    let peak = peaks(&points, MEASURE_GRID);
    let ahead = |a: usize, b: usize| {
        peak[b]
            .cmp(&peak[a])
            .then(points[b].1.total_cmp(&points[a].1))
    };
    // The first `MEASURE_STARTS` in that order, as a stable sort by
    // `ahead` would put them, picked in one pass: the points come in grid
    // order, so a tie stays behind those before it.
    let mut order: Vec<usize> = Vec::with_capacity(MEASURE_STARTS + 1);
    for i in 0..points.len() {
        let mut place = order.len();
        while place > 0 && ahead(order[place - 1], i) == std::cmp::Ordering::Greater {
            place -= 1;
        }
        if place < MEASURE_STARTS {
            order.insert(place, i);
            order.truncate(MEASURE_STARTS);
        }
    }
    let mut worst = points[order[0]].1;
    if worst == f64::INFINITY {
        return worst;
    }
    const DIRECTIONS: [[f64; 3]; 6] = [
        [1.0, -1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, 0.0, -1.0],
        [-1.0, 0.0, 1.0],
        [0.0, 1.0, -1.0],
        [0.0, -1.0, 1.0],
    ];
    for &(start, value) in order.iter().map(|&i| &points[i]) {
        let (mut u, mut best) = (start, value);
        let mut step = 1.0 / MEASURE_GRID as f64;
        let (mut halvings, mut moves) = (0, 0);
        while halvings < MEASURE_HALVINGS && moves < MEASURE_MOVES {
            let mut moved = false;
            for d in &DIRECTIONS {
                let v = [u[0] + d[0] * step, u[1] + d[1] * step, u[2] + d[2] * step];
                if v[0] < 0.0 || v[1] < 0.0 || v[2] < 0.0 {
                    continue;
                }
                let value = at(v);
                if value > best {
                    (u, best, moved) = (v, value, true);
                }
            }
            moves += 1;
            if !moved {
                step *= 0.5;
                halvings += 1;
            }
        }
        worst = worst.max(best);
    }
    worst
}

/// Which points of [`grid`]`(n)` (with their values, in its order) are
/// peaks: no point a grid step away along one of its six directions is
/// farther.
fn peaks(points: &[([f64; 3], f64)], n: usize) -> Vec<bool> {
    // Row `i` holds `n + 1 − i` points, from `j = 0`.
    let index = |i: usize, j: usize| i * (n + 1) - i * i.saturating_sub(1) / 2 + j;
    let mut peak = Vec::with_capacity(points.len());
    for i in 0..=n {
        for j in 0..=n - i {
            let (u, value) = points[index(i, j)];
            debug_assert_eq!(u[..2], [i as f64 / n as f64, j as f64 / n as f64]);
            // (i, j) moved by whole steps, staying on the triangle.
            const STEPS: [(isize, isize); 6] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, -1), (-1, 1)];
            let mut higher = false;
            for &(di, dj) in &STEPS {
                let (a, b) = (i as isize + di, j as isize + dj);
                let on = a >= 0 && b >= 0 && a + b <= n as isize;
                if on && points[index(a as usize, b as usize)].1 > value {
                    higher = true;
                    break;
                }
            }
            peak.push(!higher);
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_maxima_are_both_peaks() {
        // Two bumps: a broad one whose four farthest grid points all lie
        // round it, and a lower, narrower one across the triangle. Both
        // tops are peaks, so each gets a climb.
        let n = MEASURE_GRID;
        let broad = [9.0, 2.0, 1.0].map(|k| k / n as f64);
        let bump = |u: [f64; 3], at: [f64; 3], height: f64, width: f64| {
            let d: f64 = (0..3).map(|i| (u[i] - at[i]) * (u[i] - at[i])).sum();
            height / (1.0 + d / (width * width))
        };
        let points: Vec<([f64; 3], f64)> = grid(n)
            .map(|u| {
                let v = bump(u, broad, 1.0, 0.5).max(bump(u, [0.0, 0.5, 0.5], 0.9, 0.05));
                (u, v)
            })
            .collect();
        let peak = peaks(&points, n);
        let tops: Vec<[f64; 3]> = points
            .iter()
            .zip(&peak)
            .filter(|(_, p)| **p)
            .map(|(p, _)| p.0)
            .collect();
        assert_eq!(tops, [[0.0, 0.5, 0.5], broad]);
        // The four farthest points all lie round the broad one.
        let mut farthest = points.clone();
        farthest.sort_by(|a, b| b.1.total_cmp(&a.1));
        assert!(farthest[..4].iter().all(|p| p.0[0] >= 0.5));
    }
}
