//! Distances from a point to patches and curves, for choosing the nearest
//! of several regions or chains ([`super::Topology::face`],
//! [`super::Topology::edge`]).
//!
//! Each is a best-first search over pieces of the patch (or curve) by
//! blossoming: a piece lies in the box of its control points (weights
//! are positive), so the box's distance is a lower bound, and the points
//! looked at (corners, middles, and on patches the foot of the
//! perpendicular by Newton's method) are upper bounds, each a distance to a
//! point of it. The search stops once no piece can come nearer than the
//! best found by more than a billionth of the patch's size, or after a
//! fixed number of pieces: so it is bounded and deterministic, and where
//! it stops before the cap, within that much of the true distance.
//!
//! One resolve's searches share a fixed [`Allowance`] of patch and curve
//! evaluations, so its cost is bounded whatever the mesh: past it, the
//! patches and curves still to look at count by their corners and ends
//! alone (an upper bound), which is a pass over their boxes, like
//! tessellating's. That only arises where many patches are about as near
//! as each other (a point at the middle of a sphere cut in pieces), where
//! any of the candidates is as good an answer. It only ever chooses
//! among regions or chains: nothing is decided by how far apart two
//! things are.

use glam::DVec3;

use crate::patch::{Bounds3, Conic3, Patch};

/// The most pieces one patch's or curve's search looks at.
const MAX_PIECES: usize = 256;

/// How close, relative to the patch's or curve's size, the bounds must
/// come for the search to stop.
const CLOSE: f64 = 1e-9;

/// Newton steps towards the foot of the perpendicular.
const NEWTON_STEPS: usize = 16;

/// The evaluations one resolve's searches may spend, all of them
/// together: see [`Allowance`].
const ALLOWANCE: usize = 1 << 20;

/// What a patch's foot of the perpendicular costs: five evaluations a
/// Newton step.
const FOOT_COST: usize = 5 * NEWTON_STEPS;

/// What a patch's piece costs: its middle and its four quarters' six
/// control points.
const PIECE_COST: usize = 1 + 4 * 6;

/// What a curve's piece costs: its middle and its halves' three control
/// points.
const CURVE_PIECE_COST: usize = 1 + 2 * 3;

/// The evaluations left to one resolve's distance searches (at most
/// [`ALLOWANCE`]), so resolving costs a bounded search plus a pass over
/// the candidates' boxes and corners, whatever the mesh.
#[derive(Debug)]
pub(crate) struct Allowance(usize);

impl Allowance {
    pub(crate) fn new() -> Allowance {
        Allowance(ALLOWANCE)
    }

    /// Takes `cost` if that much is left: whether it was.
    fn take(&mut self, cost: usize) -> bool {
        let left = self.0.checked_sub(cost);
        self.0 = left.unwrap_or(0);
        left.is_some()
    }
}

/// How far `x` is from the box `b`: 0 inside it.
fn gap(b: &Bounds3, x: DVec3) -> f64 {
    (b.min - x).max(x - b.max).max(DVec3::ZERO).length()
}

/// The size of the box `b`: its longest side.
fn size(b: &Bounds3) -> f64 {
    (b.max - b.min).max_element()
}

/// The least distance from `x` to `patches`, or a number at least
/// `below` where none comes nearer than that. Patches are looked at
/// nearest box first, and only while their box comes nearer than the
/// best found.
pub(crate) fn to_patches(
    x: DVec3,
    patches: impl Iterator<Item = Patch>,
    below: f64,
    left: &mut Allowance,
) -> f64 {
    let mut patches: Vec<(f64, Patch)> = patches.map(|p| (gap(&p.bounds(), x), p)).collect();
    patches.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best = below;
    for (g, patch) in &patches {
        if *g >= best {
            break;
        }
        best = best.min(to_patch(x, patch, best, left));
    }
    best
}

/// The least distance from `x` to `curves`, or a number at least `below`
/// where none comes nearer than that.
pub(super) fn to_curves(
    x: DVec3,
    curves: impl Iterator<Item = Conic3>,
    below: f64,
    left: &mut Allowance,
) -> f64 {
    let mut curves: Vec<(f64, Conic3)> = curves.map(|c| (gap(&c.bounds(), x), c)).collect();
    curves.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best = below;
    for (g, curve) in &curves {
        if *g >= best {
            break;
        }
        best = best.min(to_curve(x, curve, best, left));
    }
    best
}

/// The point of `patch` over the barycentric pair `(a, b)`'s blossom.
fn blossom_point(patch: &Patch, a: DVec3, b: DVec3) -> DVec3 {
    let h = patch.blossom(a, b);
    h.truncate() / h.w
}

/// The box around the control points of the piece of `patch` over the
/// barycentric triangle `d`, which holds the piece.
fn piece_box(patch: &Patch, d: [DVec3; 3]) -> Bounds3 {
    let points = [
        blossom_point(patch, d[0], d[0]),
        blossom_point(patch, d[1], d[1]),
        blossom_point(patch, d[2], d[2]),
        blossom_point(patch, d[0], d[1]),
        blossom_point(patch, d[1], d[2]),
        blossom_point(patch, d[2], d[0]),
    ];
    Bounds3::around(&points).expect("six points")
}

/// The four pieces of the barycentric triangle `d`, split at its sides'
/// midpoints.
fn quarters(d: [DVec3; 3]) -> [[DVec3; 3]; 4] {
    let m01 = (d[0] + d[1]) * 0.5;
    let m12 = (d[1] + d[2]) * 0.5;
    let m20 = (d[2] + d[0]) * 0.5;
    [
        [d[0], m01, m20],
        [m01, d[1], m12],
        [m20, m12, d[2]],
        [m01, m12, m20],
    ]
}

/// The foot of the perpendicular from `x` on `patch` by Newton's method
/// on the squared distance from the middle (its second derivatives by
/// central differences, and Gauss–Newton's step where that isn't a
/// minimum's), moved into the triangle: a point of the patch, or `None`
/// where the steps don't stay finite.
fn foot(patch: &Patch, x: DVec3) -> Option<DVec3> {
    /// The step of the central differences, in the domain.
    const H: f64 = 1e-5;
    let derivs = |u: DVec3| {
        let [_, pu, pv] = patch.eval_derivs(u);
        (pu, pv)
    };
    let mut u = DVec3::splat(1.0 / 3.0);
    for _ in 0..NEWTON_STEPS {
        let [p, pu, pv] = patch.eval_derivs(u);
        let r = x - p;
        let (e, f) = (pu.dot(r), pv.dot(r));
        let (along_u, along_v) = (DVec3::new(H, 0.0, -H), DVec3::new(0.0, H, -H));
        let (uu1, uv1) = derivs(u + along_u);
        let (uu0, uv0) = derivs(u - along_u);
        let (_, vv1) = derivs(u + along_v);
        let (_, vv0) = derivs(u - along_v);
        let (puu, puv, pvv) = (
            (uu1 - uu0) / (2.0 * H),
            (uv1 - uv0) / (2.0 * H),
            (vv1 - vv0) / (2.0 * H),
        );
        let gauss = (pu.dot(pu), pu.dot(pv), pv.dot(pv));
        let newton = (
            gauss.0 - r.dot(puu),
            gauss.1 - r.dot(puv),
            gauss.2 - r.dot(pvv),
        );
        let det = |(a, b, c): (f64, f64, f64)| a * c - b * b;
        let (a, b, c) = if newton.0 > 0.0 && det(newton) > 0.0 {
            newton
        } else {
            gauss
        };
        let d = det((a, b, c));
        if !(d > 0.0 && d.is_finite()) {
            break;
        }
        let (s, t) = ((c * e - b * f) / d, (a * f - b * e) / d);
        let next = DVec3::new(u.x + s, u.y + t, u.z - s - t);
        if !next.is_finite() || next.abs().max_element() > 4.0 {
            break;
        }
        u = next;
        if s.abs().max(t.abs()) <= 1e-15 {
            break;
        }
    }
    let u = u.max(DVec3::ZERO);
    let sum = u.element_sum();
    (sum > 0.0 && sum.is_finite()).then(|| patch.eval(u / sum))
}

/// The distance from `x` to `patch` (see the [module](self) docs), or a
/// number at least `below` where it is no nearer: by its corners alone
/// once `left` runs out.
fn to_patch(x: DVec3, patch: &Patch, below: f64, left: &mut Allowance) -> f64 {
    let whole = patch.bounds();
    let close = CLOSE * size(&whole);
    let mut best = patch
        .p
        .iter()
        .map(|p| p.distance(x))
        .fold(f64::INFINITY, f64::min);
    if !left.take(FOOT_COST) {
        return best;
    }
    if let Some(p) = foot(patch, x) {
        best = best.min(p.distance(x));
    }
    let mut open = vec![(gap(&whole, x), DVec3::AXES)];
    for _ in 0..MAX_PIECES {
        let Some(i) = (0..open.len()).min_by(|&i, &j| open[i].0.total_cmp(&open[j].0)) else {
            break;
        };
        let bound = best.min(below);
        let (g, d) = open.swap_remove(i);
        if g >= bound - close || !left.take(PIECE_COST) {
            break;
        }
        best = best.min(patch.eval((d[0] + d[1] + d[2]) / 3.0).distance(x));
        for q in quarters(d) {
            let g = gap(&piece_box(patch, q), x);
            if g < best.min(below) - close {
                open.push((g, q));
            }
        }
    }
    best
}

/// The distance from `x` to `curve`, or a number at least `below` where
/// it is no nearer, by the same search over its pieces (its ends alone
/// once `left` runs out).
fn to_curve(x: DVec3, curve: &Conic3, below: f64, left: &mut Allowance) -> f64 {
    let point = |s: f64, t: f64| {
        let h = curve.blossom(s, t);
        h.truncate() / h.w
    };
    let piece_box = |s: f64, t: f64| {
        Bounds3::around(&[point(s, s), point(s, t), point(t, t)]).expect("three points")
    };
    let whole = curve.bounds();
    let close = CLOSE * size(&whole);
    let mut best = curve.p0.distance(x).min(curve.p1.distance(x));
    let mut open = vec![(gap(&whole, x), 0.0, 1.0)];
    for _ in 0..MAX_PIECES {
        let Some(i) = (0..open.len()).min_by(|&i, &j| open[i].0.total_cmp(&open[j].0)) else {
            break;
        };
        let bound = best.min(below);
        let (g, s, t) = open.swap_remove(i);
        if g >= bound - close || !left.take(CURVE_PIECE_COST) {
            break;
        }
        let m = 0.5 * (s + t);
        best = best.min(point(m, m).distance(x));
        for (a, b) in [(s, m), (m, t)] {
            let g = gap(&piece_box(a, b), x);
            if g < best.min(below) - close {
                open.push((g, a, b));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;

    #[test]
    fn distances_to_a_quarter_cylinder_patch_are_its_radius_off() {
        let base = crate::patch::Conic3::new(
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            std::f64::consts::FRAC_1_SQRT_2,
            DVec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        let [first, second] = crate::patch::cylinder_strip(&base, DVec3::Z).unwrap();
        // The axis through the middle of the strip: 1 from every point.
        for z in [0.25, 0.5, 0.75] {
            let x = DVec3::new(0.0, 0.0, z);
            let d = to_patches(
                x,
                [first, second].into_iter(),
                f64::INFINITY,
                &mut Allowance::new(),
            );
            assert!((d - 1.0).abs() < 1e-9, "{d}");
        }
        // Outside, across from the middle of the arc.
        let x = DVec3::new(2.0, 2.0, 0.5) * std::f64::consts::FRAC_1_SQRT_2;
        let d = to_patches(
            x,
            [first, second].into_iter(),
            f64::INFINITY,
            &mut Allowance::new(),
        );
        assert!((d - 1.0).abs() < 1e-9, "{d}");
        // Inside, on the concave side.
        let x = DVec3::new(0.3, 0.3, 0.5) * std::f64::consts::FRAC_1_SQRT_2;
        let d = to_patches(
            x,
            [first, second].into_iter(),
            f64::INFINITY,
            &mut Allowance::new(),
        );
        assert!((d - 0.7).abs() < 1e-9, "{d}");
        // Past a corner: the corner.
        let x = DVec3::new(2.0, -1.0, 1.5);
        let d = to_patches(
            x,
            [first, second].into_iter(),
            f64::INFINITY,
            &mut Allowance::new(),
        );
        assert!(
            (d - x.distance(DVec3::new(1.0, 0.0, 1.0))).abs() < 1e-9,
            "{d}"
        );
        // And on its curves: the bottom arc from a point out from its
        // middle.
        let y = DVec3::new(1.5, 1.5, 0.0) * std::f64::consts::FRAC_1_SQRT_2;
        let d = to_curves(y, [base].into_iter(), f64::INFINITY, &mut Allowance::new());
        assert!((d - 0.5).abs() < 1e-9, "{d}");
        // Nothing nearer than a bound below the distance.
        assert!(to_patches(x, [first, second].into_iter(), 0.5, &mut Allowance::new()) >= 0.5);
        // With nothing left to search, the corners: an upper bound.
        let y = DVec3::new(0.0, 0.0, 0.5);
        let d = to_patches(
            y,
            [first, second].into_iter(),
            f64::INFINITY,
            &mut Allowance(0),
        );
        assert_eq!(d, DVec3::new(1.0, 0.0, 0.0).distance(y));
    }

    #[test]
    fn one_allowance_bounds_many_patches_as_near_as_each_other() {
        // Every patch 1 from the point, all through: the searches stop
        // when the allowance runs out, the answer still the distance.
        let base = crate::patch::Conic3::new(
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            std::f64::consts::FRAC_1_SQRT_2,
            DVec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        let [first, _] = crate::patch::cylinder_strip(&base, DVec3::Z).unwrap();
        let x = DVec3::new(0.0, 0.0, 0.5);
        let mut left = Allowance::new();
        let d = to_patches(
            x,
            std::iter::repeat_n(first, 20_000),
            f64::INFINITY,
            &mut left,
        );
        assert!((d - 1.0).abs() < 1e-9, "{d}");
        assert_eq!(left.0, 0);
    }
}
