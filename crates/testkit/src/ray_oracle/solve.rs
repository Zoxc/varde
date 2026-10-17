//! Signed root counting of a polynomial map `F: [0, 1]^k → R^k` (`k` 2
//! or 3) in Bernstein form, by subdivision.
//!
//! Each box is first preconditioned: `A` is an integer matrix near the
//! inverse Jacobian at its centre (from f64 approximations; any
//! invertible `A` is sound, a good one makes the tests sharp), and
//! `A·F` is formed exactly. A box is dropped when a component of `F` or
//! of `A·F` has coefficients of one strict sign (no root there), when a
//! side condition (a polynomial that must be positive at a counted root)
//! has all its coefficients negative, or when its weight is 0. It is
//! counted when every side condition has all its coefficients positive,
//! its weight is settled, and it passes the Poincaré–Miranda test: each
//! component `r` of `A·F` has coefficients of one strict sign on the
//! face `x_r = 0` and of the other on `x_r = 1`. Then the degree of `F`
//! on the box, the sum of `sign det F′` over its roots, is `sign det A`
//! times `−1` per component negative on `x_r = 1`, however many roots it
//! holds.
//!
//! Otherwise the box is split, where a Newton estimate of a root says:
//! with the root certified but a side condition or the weight unsettled,
//! closely around it (see `zoom`); else along the axes `F` varies most
//! along, away from the estimate, and right next to a face the estimate
//! is close to (see `split_at`), so roots never sit on the faces of the
//! boxes holding them and roots near a domain end are closed in on fast.
//! The guidance is f64 and only affects the work done, never a count.
//!
//! A root on the boundary of the whole box (a domain end), on the zero
//! set of a side condition, a non-simple root, or a curve of roots never
//! passes: the work caps then report the configuration degenerate.

use super::bern::{Bern, Cut, derivative, eval, slice};
use super::exact::{Dy, ord_sign};
use super::trim::{TrimPiece, Trims, refine};
use malachite::Integer;
use malachite::base::num::arithmetic::traits::Sign;

/// Work limits of one count.
#[derive(Clone, Copy, Debug)]
pub(super) struct Caps {
    /// Subdivision levels.
    pub(super) depth: u32,
    /// The narrowest box side, `2^-width` of its domain.
    pub(super) width: i64,
    /// Boxes looked at.
    pub(super) boxes: u32,
    /// Subdivision levels of a p-curve in a trim test.
    pub(super) trim_depth: u32,
}

/// Why a count gave up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Degenerate {
    Depth,
    Boxes,
    Winding,
    Zero,
}

/// A count's weight: the winding number of the face's loops around the
/// UV point of the box's axes `axes`, the tile at `off`.
pub(super) struct Weight<'a> {
    pub(super) trims: &'a Trims,
    pub(super) axes: [usize; 2],
    pub(super) off: [i64; 2],
}

struct Node {
    f: Vec<Bern>,
    preds: Vec<Bern>,
    lo: [Dy; 3],
    hi: [Dy; 3],
    depth: u32,
    w: Option<i64>,
    /// While `w` is unknown: the trim parts that may meet the box.
    trims: Vec<TrimPiece>,
}

/// Boxes this shallow look up their weight as soon as no trim meets
/// them, to drop regions outside the face early; deeper ones only once
/// they hold a certified root.
const PRUNE_DEPTH: u32 = 12;

/// `Σ sign det F′ · weight` over the roots of `f` in the open unit box
/// where every `pred` is positive.
pub(super) fn count(
    k: usize,
    f: Vec<Bern>,
    preds: Vec<Bern>,
    weight: Option<&Weight>,
    caps: &Caps,
) -> Result<i64, Degenerate> {
    debug_assert_eq!(f.len(), k);
    if f.iter().any(|p| p.is_zero()) {
        return Err(Degenerate::Zero);
    }
    let one = Dy::from_int(1);
    let mut stack = vec![Node {
        f,
        preds,
        lo: [Dy::ZERO, Dy::ZERO, Dy::ZERO],
        hi: [one.clone(), one.clone(), one],
        depth: 0,
        w: if weight.is_some() { None } else { Some(1) },
        trims: weight.map(|w| w.trims.pieces()).unwrap_or_default(),
    }];
    let mut total = 0;
    let mut boxes = 0u32;
    while let Some(mut nd) = stack.pop() {
        boxes += 1;
        if boxes > caps.boxes {
            return Err(Degenerate::Boxes);
        }
        if nd.f.iter().any(|p| p.signs().strict()) {
            continue;
        }
        let mut dead = false;
        nd.preds.retain(|p| {
            let s = p.signs();
            dead |= s.negative();
            !s.positive()
        });
        if dead {
            continue;
        }
        let ap: Vec<(Vec<f64>, i64)> = nd.f.iter().map(Bern::approx).collect();
        let n = nd.f[0].n;
        let jac = |x: [f64; 3]| -> [[f64; 3]; 3] {
            let mut j = [[0.0; 3]; 3];
            for (r, row) in j.iter_mut().enumerate().take(k) {
                for (c, v) in row.iter_mut().enumerate().take(k) {
                    let (d, dn) = derivative(&ap[r].0, n, c);
                    *v = eval(&d, dn, x);
                }
            }
            j
        };
        let certified = match precondition(k, &nd.f, &ap, &jac([0.5; 3])) {
            Pre::Excluded => continue,
            Pre::Degree(d) => Some(d),
            Pre::Unknown => None,
        };
        let deg = certified.filter(|_| nd.preds.is_empty());
        if let (Some(wt), None) = (weight, nd.w) {
            let at = |b: &[Dy; 3], a: usize| b[wt.axes[a]].add(&Dy::from_int(wt.off[a]));
            let lo = [at(&nd.lo, 0), at(&nd.lo, 1)];
            let hi = [at(&nd.hi, 0), at(&nd.hi, 1)];
            let (live, free) = refine(std::mem::take(&mut nd.trims), &lo, &hi, caps.trim_depth);
            nd.trims = live;
            if free && (deg.is_some() || nd.depth < PRUNE_DEPTH) {
                let mid = [lo[0].add(&hi[0]).shl(-1), lo[1].add(&hi[1]).shl(-1)];
                nd.w = Some(
                    wt.trims
                        .winding(&mid, caps.trim_depth)
                        .ok_or(Degenerate::Winding)?,
                );
            }
        }
        if nd.w.is_some() {
            nd.trims.clear();
        }
        if nd.w == Some(0) {
            continue;
        }
        if let (Some(w), Some(deg)) = (nd.w, deg) {
            total += w * deg;
            continue;
        }
        if nd.depth >= caps.depth || (0..k).any(|a| nd.hi[a].sub(&nd.lo[a]).log2() < -caps.width) {
            return Err(Degenerate::Depth);
        }
        let est = newton(k, n, &ap, &jac);
        // A certified root whose side conditions or weight aren't settled
        // yet: close in on it; else split along the axes that matter.
        let plan: Vec<(usize, Cut, Option<Cut>)> =
            match certified.and_then(|_| zoom(k, n, &ap, &jac, est?)) {
                Some(z) => z,
                None => split_axes(k, n, &ap)
                    .into_iter()
                    .map(|a| (a, split_at(est.map(|e| e[a])), None))
                    .collect(),
            };
        let mut kids = vec![nd];
        for (axis, hi, lo) in plan {
            let mut next = Vec::with_capacity(kids.len() * 3);
            for nd in kids {
                let (left, right) = split_node(nd, axis, hi);
                match lo {
                    Some(lo) => {
                        let (a, b) = split_node(left, axis, lo);
                        next.extend([a, b]);
                    }
                    None => next.push(left),
                }
                next.push(right);
            }
            kids = next;
        }
        for mut kid in kids {
            kid.depth += 1;
            stack.push(kid);
        }
    }
    Ok(total)
}

/// The two parts of a box at `cut` along `axis`.
fn split_node(nd: Node, axis: usize, cut: Cut) -> (Node, Node) {
    let (mut a, mut b) = (
        Vec::with_capacity(nd.f.len()),
        Vec::with_capacity(nd.f.len()),
    );
    for p in &nd.f {
        let (x, y) = p.split(axis, cut);
        a.push(x);
        b.push(y);
    }
    let (mut pa, mut pb) = (Vec::new(), Vec::new());
    for p in &nd.preds {
        let (x, y) = p.split(axis, cut);
        pa.push(x);
        pb.push(y);
    }
    let width = nd.hi[axis].sub(&nd.lo[axis]);
    let at = nd.lo[axis].add(&width.mul(&Dy::from_int(cut.m as i64)).shl(-(cut.p as i64)));
    let mut ahi = nd.hi.clone();
    ahi[axis] = at.clone();
    let mut blo = nd.lo.clone();
    blo[axis] = at;
    let left = Node {
        f: a,
        preds: pa,
        lo: nd.lo,
        hi: ahi,
        depth: nd.depth,
        w: nd.w,
        trims: nd.trims.clone(),
    };
    let right = Node {
        f: b,
        preds: pb,
        lo: blo,
        hi: nd.hi,
        depth: nd.depth,
        w: nd.w,
        trims: nd.trims,
    };
    (left, right)
}

/// Cuts closing in on the root estimated at `e`, along every axis: at
/// `e ± δ`, `δ` sixteen times the largest departure of the
/// Newton-preconditioned `F` from linear at the box's corners (so the
/// parts beside the root drop out by their sign) and at least `2^-40`
/// (the estimate's precision). Per axis: the cut of the box, and the
/// cut of its lower part. `None` when that doesn't shrink the box.
fn zoom(
    k: usize,
    n: [usize; 3],
    ap: &[(Vec<f64>, i64)],
    jac: &dyn Fn([f64; 3]) -> [[f64; 3]; 3],
    e: [f64; 3],
) -> Option<Vec<(usize, Cut, Option<Cut>)>> {
    if (0..k).any(|a| !(0.0 < e[a] && e[a] < 1.0)) {
        return None;
    }
    let inv = inverse(k, &jac(e))?;
    let mut nu = 0.0f64;
    for corner in 0..1usize << k {
        let x: [f64; 3] = std::array::from_fn(|a| {
            if a < k && corner >> a & 1 == 1 {
                1.0
            } else {
                0.0
            }
        });
        let fx: Vec<f64> = (0..k).map(|r| eval(&ap[r].0, n, x)).collect();
        for r in 0..k {
            let g: f64 = (0..k).map(|c| inv[r][c] * fx[c]).sum::<f64>() - (x[r] - e[r]);
            if !g.is_finite() {
                return None;
            }
            if g.abs() > nu {
                nu = g.abs();
            }
        }
    }
    let floor = libm::scalbn(1.0, -40);
    let delta = if 16.0 * nu > floor { 16.0 * nu } else { floor };
    if delta >= 0.0625 {
        return None;
    }
    // Cuts on a grid of δ/16.
    let p = (-libm::ilogb(delta) + 4) as u64;
    let unit = libm::scalbn(1.0, p as i32);
    let mut plan = Vec::new();
    for (a, &ea) in e.iter().enumerate().take(k) {
        let top = ((ea + delta) * unit).ceil() as u64;
        let bottom = ((ea - delta) * unit).floor();
        let hi_cut = (top < (1u64 << p)).then_some(Cut { m: top, p });
        let lo_cut = (bottom >= 1.0).then(|| {
            // `bottom/2^p` within `[0, top/2^p]` (or `[0, 1]`), on a grid
            // two bits finer.
            let span = hi_cut.map_or(unit, |c| c.m as f64);
            let q = p + 2;
            let m = ((bottom / span) * libm::scalbn(1.0, q as i32)).floor() as u64;
            Cut {
                m: if m == 0 { 1 } else { m },
                p: q,
            }
        });
        match (hi_cut, lo_cut) {
            (Some(h), l) => plan.push((a, h, l)),
            (None, Some(l)) => plan.push((a, l, None)),
            (None, None) => {}
        }
    }
    (!plan.is_empty()).then_some(plan)
}

/// The axes to split: those along which some component's coefficients
/// vary at least a quarter as much (relative to its size) as along the
/// axis they vary most along. A thin ribbon is split across only once
/// its boxes are as short as it is wide.
fn split_axes(k: usize, n: [usize; 3], ap: &[(Vec<f64>, i64)]) -> Vec<usize> {
    let mut var = [0.0f64; 3];
    for (c, _) in ap {
        let size = c
            .iter()
            .fold(0.0f64, |m, x| if x.abs() > m { x.abs() } else { m });
        if size == 0.0 {
            continue;
        }
        for (axis, v) in var.iter_mut().enumerate().take(k) {
            let (d, _) = derivative(c, n, axis);
            let most = d
                .iter()
                .fold(0.0f64, |m, x| if x.abs() > m { x.abs() } else { m })
                / size;
            if most > *v {
                *v = most;
            }
        }
    }
    let top = var.iter().fold(0.0f64, |m, &x| if x > m { x } else { m });
    if top == 0.0 {
        return (0..k).collect();
    }
    (0..k).filter(|&a| var[a] >= 0.25 * top).collect()
}

/// Where to split, given where a root is estimated: outside the box, a
/// quarter off that side (the boxes beside a root just outside need to
/// shrink to its distance to drop out); near a face (within an eighth)
/// at a power of two putting the root between a quarter and a half of
/// the way into the part beside that face (at most `2^-40` deep, about
/// the estimate's precision), so roots close to a domain end are closed
/// in on fast; else at the quarter point at least an eighth away from
/// the estimate making the root's part smallest. The middle without an
/// estimate.
fn split_at(est: Option<f64>) -> Cut {
    let Some(e) = est else { return Cut::HALF };
    if e < 0.0 {
        return Cut { m: 1, p: 2 };
    }
    if e > 1.0 {
        return Cut { m: 3, p: 2 };
    }
    let near = |d: f64| -> Option<u64> {
        // 2^-j in (2d, 4d] (2^L ≤ d < 2^(L+1), j = −L − 2), j at most 40.
        let d = if d > 1e-12 { d } else { 1e-12 };
        (d < 0.125).then(|| (-libm::ilogb(d) - 2).clamp(2, 40) as u64)
    };
    if let Some(j) = near(e) {
        return Cut { m: 1, p: j };
    }
    if let Some(j) = near(1.0 - e) {
        return Cut {
            m: (1u64 << j) - 1,
            p: j,
        };
    }
    let mut best = (2.0, 2);
    for m in [2u64, 1, 3] {
        let cut = m as f64 / 4.0;
        if (e - cut).abs() < 0.125 {
            continue;
        }
        let part = if e < cut { cut } else { 1.0 - cut };
        if part < best.0 {
            best = (part, m);
        }
    }
    Cut { m: best.1, p: 2 }
}

fn solve(k: usize, j: &[[f64; 3]; 3], b: &[f64; 3]) -> Option<[f64; 3]> {
    let inv = inverse(k, j)?;
    let mut x = [0.0; 3];
    for r in 0..k {
        for c in 0..k {
            x[r] += inv[r][c] * b[c];
        }
    }
    Some(x)
}

fn inverse(k: usize, j: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let mut inv = [[0.0; 3]; 3];
    if k == 2 {
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        inv[0][0] = j[1][1] / det;
        inv[0][1] = -j[0][1] / det;
        inv[1][0] = -j[1][0] / det;
        inv[1][1] = j[0][0] / det;
    } else {
        let cof = |r: usize, c: usize| {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            j[r1][c1] * j[r2][c2] - j[r1][c2] * j[r2][c1]
        };
        let det = j[0][0] * cof(0, 0) + j[0][1] * cof(0, 1) + j[0][2] * cof(0, 2);
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        for (r, row) in inv.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = cof(c, r) / det;
            }
        }
    }
    inv.iter().flatten().all(|x| x.is_finite()).then_some(inv)
}

/// Where a few Newton steps from the centre land, if they stay near the
/// box.
fn newton(
    k: usize,
    n: [usize; 3],
    ap: &[(Vec<f64>, i64)],
    jac: &dyn Fn([f64; 3]) -> [[f64; 3]; 3],
) -> Option<[f64; 3]> {
    let mut x = [0.5, 0.5, 0.5];
    for _ in 0..8 {
        let mut fx = [0.0; 3];
        for (r, v) in fx.iter_mut().enumerate().take(k) {
            *v = eval(&ap[r].0, n, x);
        }
        let dx = solve(k, &jac(x), &fx)?;
        let mut small = true;
        for a in 0..k {
            x[a] -= dx[a];
            if !(-1.0..=2.0).contains(&x[a]) {
                return None;
            }
            small &= dx[a].abs() < 1e-9;
        }
        if small {
            break;
        }
    }
    (0..k).all(|a| (-0.25..=1.25).contains(&x[a])).then_some(x)
}

/// What the system preconditioned by an integer `A` near the inverse
/// Jacobian at the centre shows: no root (a row of `A·F` of one strict
/// sign; `A·F = 0` exactly where `F = 0`), or the degree of `F` on the
/// box by the Poincaré–Miranda test.
enum Pre {
    Excluded,
    Degree(i64),
    Unknown,
}

fn precondition(k: usize, f: &[Bern], ap: &[(Vec<f64>, i64)], j: &[[f64; 3]; 3]) -> Pre {
    let Some(inv) = inverse(k, j) else {
        return Pre::Unknown;
    };
    // Integer rows near `inv`'s, each scaled to about 2^52.
    let mut a = [[0i64; 3]; 3];
    for r in 0..k {
        let most = (0..k).fold(0.0f64, |m, c| {
            if inv[r][c].abs() > m {
                inv[r][c].abs()
            } else {
                m
            }
        });
        if most == 0.0 {
            return Pre::Unknown;
        }
        let scale = libm::scalbn(1.0, 52 - libm::ilogb(most));
        for c in 0..k {
            a[r][c] = (inv[r][c] * scale).round() as i64;
        }
    }
    // Approximation `c` is `F_c·2^-s_c`: combine `F_c·2^(top - s_c)`.
    let top = ap.iter().map(|x| x.1).max().expect("components");
    let n = f[0].n;
    let scaled: Vec<Vec<Integer>> = (0..k)
        .map(|c| f[c].c.iter().map(|x| x << (top - ap[c].1) as u64).collect())
        .collect();
    let rows: Vec<Bern> = (0..k)
        .map(|r| {
            let c = (0..f[0].c.len())
                .map(|i| {
                    let mut v = Integer::from(0);
                    for (c, col) in scaled.iter().enumerate() {
                        if a[r][c] != 0 {
                            v += Integer::from(a[r][c]) * &col[i];
                        }
                    }
                    v
                })
                .collect();
            Bern::new(n, c)
        })
        .collect();
    if rows.iter().any(|r| r.signs().strict()) {
        return Pre::Excluded;
    }
    let mut sigma = 1;
    for (r, row) in rows.iter().enumerate() {
        let face = |side: usize| -> Option<i64> {
            let mut sign = 0;
            for i in slice(n, r, side * n[r]) {
                let s = ord_sign(row.c[i].sign());
                if s == 0 || (sign != 0 && s != sign) {
                    return None;
                }
                sign = s;
            }
            Some(sign)
        };
        match (face(0), face(1)) {
            (Some(-1), Some(1)) => {}
            (Some(1), Some(-1)) => sigma = -sigma,
            _ => return Pre::Unknown,
        }
    }
    let ai = |r: usize, c: usize| Integer::from(a[r][c]);
    let det = if k == 2 {
        ai(0, 0) * ai(1, 1) - ai(0, 1) * ai(1, 0)
    } else {
        ai(0, 0) * (ai(1, 1) * ai(2, 2) - ai(1, 2) * ai(2, 1))
            - ai(0, 1) * (ai(1, 0) * ai(2, 2) - ai(1, 2) * ai(2, 0))
            + ai(0, 2) * (ai(1, 0) * ai(2, 1) - ai(1, 1) * ai(2, 0))
    };
    match ord_sign(det.sign()) {
        0 => Pre::Unknown,
        d => Pre::Degree(d * sigma),
    }
}
