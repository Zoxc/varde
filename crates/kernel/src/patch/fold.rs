//! The normal's Bernstein coefficients, the fold check and the normal
//! cone.
//!
//! With the homogeneous point `X(u)` a quadratic form in the barycentric
//! `u`, and `G_i = ½·∂X/∂ui`, the derivatives' cross product is
//! `4·cross4(G0, G1, G2) / D³` on the triangle (`D` the denominator), by
//! Euler's identity `X = Σ ui·G_i`. So the normal's numerator is a *cubic*
//! with ten Bernstein coefficients, each `G_i` being linear in `u`. Every
//! normal is a positive combination of them.

use std::f64::consts::PI;

use glam::DVec3;

use super::triangle::cross4;
use super::{FOLD_FLOOR, FOLD_MARGIN, Patch};

/// The multi-indices `(i, j, k)` of the normal's cubic Bernstein
/// coefficients, in the order [`Patch::normal_coeffs`] gives them: `i`
/// counts corner 0, `j` corner 1 and `k` corner 2.
pub(super) const NORMAL_INDEX: [[u8; 3]; 10] = [
    [3, 0, 0],
    [2, 1, 0],
    [2, 0, 1],
    [1, 2, 0],
    [1, 1, 1],
    [1, 0, 2],
    [0, 3, 0],
    [0, 2, 1],
    [0, 1, 2],
    [0, 0, 3],
];

/// Where the multi-index with `i` and `j` sits in [`NORMAL_INDEX`].
fn slot(i: usize, j: usize) -> usize {
    [6, 3, 1, 0][i] + (3 - i - j)
}

/// Slack added to a normal cone's angle, radians, for the rounding in its
/// coefficients.
const CONE_SLACK: f64 = 1e-9;

/// A cone of directions: every direction within `angle` (radians, at most
/// π) of the unit `axis`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalCone {
    pub axis: DVec3,
    pub angle: f64,
}

impl NormalCone {
    /// Whether no direction in this cone is parallel to one in `other`,
    /// either way round: two patches with cones apart can't meet in a
    /// closed loop.
    pub fn apart(&self, other: &NormalCone) -> bool {
        let between = self.axis.dot(other.axis).clamp(-1.0, 1.0).acos();
        let reach = self.angle + other.angle;
        between > reach && PI - between > reach
    }
}

impl Patch {
    /// The normal numerator's ten cubic Bernstein coefficients, in the
    /// order of the multi-indices `(3,0,0), (2,1,0), (2,0,1), (1,2,0),
    /// (1,1,1), (1,0,2), (0,3,0), (0,2,1), (0,1,2), (0,0,3)`: the normal
    /// at barycentric `u` is `Σ 3!/(i!·j!·k!)·u0^i·u1^j·u2^k·c`, which is
    /// [`Self::normal`]. `(3,0,0)` is at corner 0, where it is the cross
    /// product of the two edge tangents leaving the corner.
    pub fn normal_coeffs(&self) -> [DVec3; 10] {
        self.normal_coeffs_scaled().0
    }

    /// [`Self::normal_coeffs`], with the size of the terms each was summed
    /// from, which bounds its rounding error.
    fn normal_coeffs_scaled(&self) -> ([DVec3; 10], [f64; 10]) {
        let net = self.net_from(self.p[0]);
        let mut sum = [DVec3::ZERO; 10];
        let mut size = [0.0; 10];
        for a in 0..3 {
            for b in 0..3 {
                for c in 0..3 {
                    let mut count = [0; 3];
                    count[a] += 1;
                    count[b] += 1;
                    count[c] += 1;
                    let k = slot(count[0], count[1]);
                    let (x, y, z) = (net[0][a], net[1][b], net[2][c]);
                    sum[k] += cross4(x, y, z);
                    let (x3, y3, z3) = (x.truncate(), y.truncate(), z.truncate());
                    size[k] += x.w.abs() * y3.length() * z3.length()
                        + y.w.abs() * x3.length() * z3.length()
                        + z.w.abs() * x3.length() * y3.length();
                }
            }
        }
        // u^α's coefficient becomes the Bernstein coefficient through
        // α!/3!, times the 4 of the numerator.
        for (k, index) in NORMAL_INDEX.iter().enumerate() {
            let factorial = index.iter().map(|&n| [1.0, 1.0, 2.0, 6.0][n as usize]);
            let scale = 4.0 * factorial.product::<f64>() / 6.0;
            sum[k] *= scale;
            size[k] *= scale;
        }
        (sum, size)
    }

    /// A unit direction `d` every normal coefficient `c` leans towards,
    /// `c·d > FOLD_MARGIN·|c|`, or `None` if there is none. With one, the
    /// normal never vanishes or flips anywhere on the patch, so the patch
    /// doesn't fold, and the same `d` serves every piece split from it.
    ///
    /// Tries the flat triangle's normal, then the mean of the normalized
    /// coefficients, then the exact answer: the axis of the smallest cone
    /// around the coefficients. A coefficient too small to tell from
    /// rounding ([`FOLD_FLOOR`]), such as at a corner whose edges leave
    /// at 0° or 180°, fails the check.
    pub fn fold_direction(&self) -> Option<DVec3> {
        let (coeffs, size) = self.normal_coeffs_scaled();
        let mut dirs = [DVec3::ZERO; 10];
        for k in 0..10 {
            let length = coeffs[k].length();
            // Written so that NaN fails too.
            if length.is_nan() || length <= FOLD_FLOOR * size[k] {
                return None;
            }
            dirs[k] = coeffs[k] / length;
        }
        let flat = (self.p[1] - self.p[0]).cross(self.p[2] - self.p[0]);
        fold_direction(&dirs, flat)
    }

    /// The cone holding every normal direction of the patch: the smallest
    /// cone around its normal coefficients, widened a little for rounding.
    /// If they don't fit in an open half-space the angle is π.
    pub fn normal_cone(&self) -> NormalCone {
        let dirs: Vec<DVec3> = self
            .normal_coeffs()
            .iter()
            .map(|c| c.normalize_or_zero())
            .filter(|&d| d != DVec3::ZERO)
            .collect();
        if dirs.is_empty() {
            return NormalCone {
                axis: DVec3::Z,
                angle: PI,
            };
        }
        let (axis, least) = smallest_cone(&dirs);
        let angle = if least > 0.0 {
            (least.min(1.0).acos() + CONE_SLACK).min(PI)
        } else {
            PI
        };
        NormalCone { axis, angle }
    }
}

/// A unit direction every one of the unit `dirs` leans towards by more
/// than [`FOLD_MARGIN`]: `hint`, their mean, or the axis of the smallest
/// cone around them, whichever passes first.
pub(super) fn fold_direction(dirs: &[DVec3], hint: DVec3) -> Option<DVec3> {
    let passes = |d: DVec3| {
        let d = d.normalize_or_zero();
        (d != DVec3::ZERO && dirs.iter().all(|c| c.dot(d) > FOLD_MARGIN)).then_some(d)
    };
    passes(hint)
        .or_else(|| passes(dirs.iter().copied().sum()))
        .or_else(|| passes(smallest_cone(dirs).0))
}

/// The unit axis whose smallest dot product with the unit `dirs` is
/// largest, and that dot product: the smallest cone around them, when it
/// is narrower than a half-space. `dirs` is not empty.
///
/// That cone is fixed by at most three of the directions on its rim, so
/// every candidate is tried: each direction, the bisector of each pair,
/// and the point on the sphere equally far from each triple. Ties keep
/// the first found, so the answer depends only on the input order.
pub(super) fn smallest_cone(dirs: &[DVec3]) -> (DVec3, f64) {
    let least = |d: DVec3| dirs.iter().map(|c| c.dot(d)).fold(f64::INFINITY, f64::min);
    let mut best = (dirs[0], least(dirs[0]));
    let mut try_axis = |d: DVec3| {
        if d != DVec3::ZERO {
            let score = least(d);
            if score > best.1 {
                best = (d, score);
            }
        }
    };
    for (i, &a) in dirs.iter().enumerate() {
        try_axis(a);
        for (j, &b) in dirs.iter().enumerate().skip(i + 1) {
            try_axis((a + b).normalize_or_zero());
            for &c in &dirs[j + 1..] {
                let n = (b - a).cross(c - a).normalize_or_zero();
                try_axis(if n.dot(a) < 0.0 { -n } else { n });
            }
        }
    }
    best
}
