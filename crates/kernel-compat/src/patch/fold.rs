//! The normal's Bernstein coefficients, the fold check and the normal
//! cone.
//!
//! With the homogeneous point `X(u)` a quadratic form in the barycentric
//! `u`, and `G_i = ½·∂X/∂ui`, the derivatives' cross product is
//! `4·cross4(G0, G1, G2) / D³` on the triangle (`D` the denominator), by
//! Euler's identity `X = Σ ui·G_i`. So the normal's numerator is a *cubic*
//! with ten Bernstein coefficients, each `G_i` being linear in `u`. Every
//! normal is a positive combination of them.

use glam::DVec3;

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

/// A bound on a normal coefficient's rounding error, relative to the size
/// of the terms it was summed from. Each term, a weighted cross product of
/// two differences, is off by at most about `7·ε/2` of its size, and adding
/// up to six of them and scaling adds about `3·ε`: `16·ε` has room to
/// spare. Divided by the coefficient's length it bounds how far, in
/// radians, the coefficient's direction may be off.
const ROUNDING: f64 = 16.0 * f64::EPSILON;

/// Slack added to a normal cone's angle, radians, for the rounding in
/// measuring angles from its axis.
const CONE_SLACK: f64 = 1e-9;

/// Apart cones must clear each other by this much more than rounding in
/// the sine of the angle between their axes can hide.
const APART_ROUNDING: f64 = 8.0 * f64::EPSILON;

// A coefficient that passes the floor points within `ROUNDING /
// FOLD_FLOOR` radians of its true direction, less than the fold check's
// margin, so a patch that passes really doesn't fold. Its normal cone is
// widened by up to twice that, and so still fits in a half-space.
const _: () = assert!(2.0 * (ROUNDING / FOLD_FLOOR + CONE_SLACK) < FOLD_MARGIN);

/// A cone of directions: every direction within the half-angle `θ` of the
/// unit `axis`, `θ` in `[0, π]`.
///
/// `θ` is kept as its cosine and sine, so deciding with it takes only
/// correctly rounded arithmetic: `acos` loses half the digits of an angle
/// near 0, and its last bits differ between platforms' maths libraries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalCone {
    pub axis: DVec3,
    /// `cos θ`.
    pub cos: f64,
    /// `sin θ`, not negative.
    pub sin: f64,
}

impl NormalCone {
    /// Every direction: `θ = π`.
    pub const ALL: NormalCone = NormalCone {
        axis: DVec3::Z,
        cos: -1.0,
        sin: 0.0,
    };

    /// The half-angle `θ`, radians. For showing: decisions use
    /// [`Self::cos`] and [`Self::sin`].
    pub fn angle(&self) -> f64 {
        crate::trig::atan2(self.sin, self.cos)
    }

    /// Whether no direction in this cone is parallel to one in `other`,
    /// either way round: two patches with cones apart can't meet in a
    /// closed loop.
    pub fn apart(&self, other: &NormalCone) -> bool {
        // The angles add up to the reach R; the axes are φ apart. Apart
        // means R < φ < π - R, so R < 90°, and then sin(φ - R) > 0 and
        // sin(π - φ - R) > 0. Each angle below 90° first, so that R's sine
        // and cosine can't wrap past a full turn.
        if !(self.cos > 0.0 && other.cos > 0.0) {
            return false;
        }
        let cos_r = self.cos * other.cos - self.sin * other.sin;
        let sin_r = self.sin * other.cos + self.cos * other.sin;
        let cos_phi = self.axis.dot(other.axis);
        let sin_phi = self.axis.cross(other.axis).length();
        cos_r > 0.0 && sin_phi * cos_r - cos_phi.abs() * sin_r > APART_ROUNDING
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
        // The term of net entries x, y and z (homogeneous points, weights
        // wx, wy, wz) is `cross4(x, y, z) = wx·wy·wz·(y - x) × (z - x)`:
        // written with differences, its rounding stays relative to the
        // distances between the three points, not their distance from
        // anywhere else. Entry (i, j) as its point and weight: corner i
        // when i == j, else the control point of the edge between corners
        // i and j.
        let entry = |i: usize, j: usize| match (i + 3 - j) % 3 {
            0 => (self.p[i], 1.0),
            1 => (self.c[j], self.w[j]),
            _ => (self.c[i], self.w[i]),
        };
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
                    let ((x, wx), (y, wy), (z, wz)) = (entry(0, a), entry(1, b), entry(2, c));
                    let (dy, dz) = (y - x, z - x);
                    let weight = wx * wy * wz;
                    sum[k] += dy.cross(dz) * weight;
                    size[k] += weight * dy.length() * dz.length();
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

    /// A corner whose normal coefficient is too small to tell from zero
    /// ([`FOLD_FLOOR`]), the first if several: its two edges leave it at
    /// 0° or 180°. The patch fails the fold check, and so does every piece
    /// split from it that keeps the corner, whose edges leave it the same
    /// ways: splitting can't mend it.
    pub fn degenerate_corner(&self) -> Option<usize> {
        let (coeffs, size) = self.normal_coeffs_scaled();
        // (3,0,0), (0,3,0) and (0,0,3).
        [0, 6, 9].into_iter().position(|k| {
            let length = coeffs[k].length();
            length.is_nan() || length <= FOLD_FLOOR * size[k]
        })
    }

    /// The cone holding every normal direction of the patch: the smallest
    /// cone around its normal coefficients, each widened by how far
    /// rounding may have turned it. A coefficient rounding may have turned
    /// by a radian or more, such as one that should be zero but isn't,
    /// gives [`NormalCone::ALL`]; one that is exactly zero, with no terms
    /// to round, is left out.
    pub fn normal_cone(&self) -> NormalCone {
        let (coeffs, size) = self.normal_coeffs_scaled();
        let mut dirs = Vec::with_capacity(10);
        let mut errors = Vec::with_capacity(10);
        for k in 0..10 {
            if size[k] == 0.0 {
                continue;
            }
            let length = coeffs[k].length();
            // NaN from coordinates that aren't finite, infinity from a
            // length of zero.
            let error = ROUNDING * size[k] / length;
            if error.is_nan() || error >= 1.0 {
                return NormalCone::ALL;
            }
            dirs.push(coeffs[k] / length);
            errors.push(error);
        }
        if dirs.is_empty() {
            return NormalCone::ALL;
        }
        let (axis, _) = smallest_cone(&dirs);
        // The widest of the directions, each turned further from the axis
        // by its error and the slack: a turn by atan(t), with t twice the
        // angle, which is at least the angle for angles below 1.
        let (mut cos, mut sin) = (1.0, 0.0);
        for (d, error) in dirs.iter().zip(errors) {
            let (c, s) = (axis.dot(*d), axis.cross(*d).length());
            let t = 2.0 * (error + CONE_SLACK);
            let norm = (1.0 + t * t).sqrt();
            let (c, s) = ((c - s * t) / norm, (s + c * t) / norm);
            if s <= 0.0 {
                // Turned to π or past it.
                return NormalCone::ALL;
            }
            // Wider when sin(θ - widest) > 0.
            if s * cos - c * sin > 0.0 {
                (cos, sin) = (c, s);
            }
        }
        NormalCone { axis, cos, sin }
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
pub(crate) fn smallest_cone(dirs: &[DVec3]) -> (DVec3, f64) {
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
