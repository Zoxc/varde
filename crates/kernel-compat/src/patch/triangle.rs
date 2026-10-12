use glam::{DVec3, DVec4};

use super::{Bounds, Bounds3, Conic3, PatchError, Point, check_point, check_weight};

/// A rational quadratic triangle in the standard form (see the
/// [module](super) docs): corners `p`, and edge `i` from corner `i` to
/// corner `i + 1` with control point `c[i]` and weight `w[i]` (so edges
/// 01, 12 and 20). Corners run counter-clockwise seen from the side the
/// normal points to.
///
/// Positions in the patch are barycentric `(u0, u1, u2)`, with `u0 = 1`
/// at corner 0; functions taking one expect it in the closed triangle.
///
/// The fields are open, so [`Patch::check`] is what makes one trusted;
/// [`Patch::new`] and every construction here check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    pub p: [DVec3; 3],
    pub c: [DVec3; 3],
    pub w: [f64; 3],
}

impl Patch {
    /// The patch, if it passes [`Patch::check`].
    pub fn new(p: [DVec3; 3], c: [DVec3; 3], w: [f64; 3]) -> Result<Self, PatchError> {
        let patch = Patch { p, c, w };
        patch.check()?;
        Ok(patch)
    }

    /// The flat triangle on `p`: straight edges.
    pub fn flat(p: [DVec3; 3]) -> Result<Self, PatchError> {
        let mid = |i: usize| (p[i] + p[(i + 1) % 3]) * 0.5;
        Self::new(p, [mid(0), mid(1), mid(2)], [1.0; 3])
    }

    /// Every coordinate finite and within
    /// [`MAX_CONTROL`](super::MAX_CONTROL), and every weight within
    /// [`W_MIN`](super::W_MIN)`..=`[`W_MAX`](super::W_MAX).
    pub fn check(&self) -> Result<(), PatchError> {
        for i in 0..3 {
            check_point(self.p[i])?;
            check_point(self.c[i])?;
            check_weight(self.w[i])?;
        }
        Ok(())
    }

    /// Edge `i`, from corner `i` to corner `i + 1`.
    pub fn edge(&self, i: usize) -> Conic3 {
        Conic3 {
            p0: self.p[i],
            c: self.c[i],
            w: self.w[i],
            p1: self.p[(i + 1) % 3],
        }
    }

    /// The homogeneous control net: `net[i][i]` is corner `i` as
    /// `(p, 1)`, and `net[i][j] = net[j][i]` the control point of the
    /// edge between corners `i` and `j` as `(w·c, w)`.
    pub fn net(&self) -> [[DVec4; 3]; 3] {
        self.net_from(DVec3::ZERO)
    }

    /// [`Self::net`] with `origin` moved to zero, which keeps rounding
    /// relative to the patch rather than to the model's origin.
    pub(super) fn net_from(&self, origin: DVec3) -> [[DVec4; 3]; 3] {
        let corner = |i: usize| (self.p[i] - origin).hom(1.0);
        let edge = |i: usize| (self.c[i] - origin).hom(self.w[i]);
        let (e01, e12, e20) = (edge(0), edge(1), edge(2));
        [
            [corner(0), e01, e20],
            [e01, corner(1), e12],
            [e20, e12, corner(2)],
        ]
    }

    /// The point at barycentric `u`.
    pub fn eval(&self, u: DVec3) -> DVec3 {
        let x = blossom(&self.net(), u, u);
        (x.truncate() / x.w).shared(&self.hull())
    }

    /// The normal's numerator at barycentric `u` (summing to 1): the
    /// cross product of the derivatives along `u0` and `u1` (with `u2 =
    /// 1 - u0 - u1`) times the cube of the point's homogeneous weight,
    /// which is positive. It points to the side the corners run
    /// counter-clockwise from.
    pub fn normal(&self, u: DVec3) -> DVec3 {
        let net = self.net_from(self.p[0]);
        let g = rows(&net, u);
        cross4(g[0], g[1], g[2]) * 4.0
    }

    /// The six control points, whose convex hull holds the patch.
    pub fn hull(&self) -> [DVec3; 6] {
        [
            self.p[0], self.c[0], self.p[1], self.c[1], self.p[2], self.c[2],
        ]
    }

    /// The box around the control points, and so around the patch.
    pub fn bounds(&self) -> Bounds3 {
        let [first, rest @ ..] = self.hull();
        rest.into_iter().fold(Bounds::point(first), Bounds::include)
    }
}

/// The homogeneous blossom `B(a, b) = Σ ai·bj·net[i][j]`.
fn blossom(net: &[[DVec4; 3]; 3], a: DVec3, b: DVec3) -> DVec4 {
    let mut sum = DVec4::ZERO;
    for i in 0..3 {
        for j in 0..3 {
            sum += net[i][j] * (a[i] * b[j]);
        }
    }
    sum
}

/// `G_i(u) = Σ uj·net[i][j]`: half the partial derivative of the
/// homogeneous point along `ui`.
fn rows(net: &[[DVec4; 3]; 3], u: DVec3) -> [DVec4; 3] {
    // Written out rather than by `[0, 1, 2].map`, which the dev profile
    // leaves as calls: this is the inner loop of every patch evaluation.
    [
        net[0][0] * u.x + net[0][1] * u.y + net[0][2] * u.z,
        net[1][0] * u.x + net[1][1] * u.y + net[1][2] * u.z,
        net[2][0] * u.x + net[2][1] * u.y + net[2][2] * u.z,
    ]
}

/// The spatial part of the 4D cross product of homogeneous points: the
/// normal of the plane through the three points they stand for.
/// Alternating and linear in each argument.
fn cross4(a: DVec4, b: DVec4, c: DVec4) -> DVec3 {
    let (a3, b3, c3) = (a.truncate(), b.truncate(), c.truncate());
    b3.cross(c3) * a.w - a3.cross(c3) * b.w + a3.cross(b3) * c.w
}
