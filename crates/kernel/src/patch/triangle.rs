use glam::{DVec3, DVec4};

use super::conic::{standard_corner, standard_edge};
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
    /// Where the children of [`Patch::split4`] lie in the parent's
    /// barycentric domain, corner by corner: the three corner triangles,
    /// then the middle one.
    pub const SPLIT4_DOMAINS: [[DVec3; 3]; 4] = {
        let e0 = DVec3::new(1.0, 0.0, 0.0);
        let e1 = DVec3::new(0.0, 1.0, 0.0);
        let e2 = DVec3::new(0.0, 0.0, 1.0);
        let m01 = DVec3::new(0.5, 0.5, 0.0);
        let m12 = DVec3::new(0.0, 0.5, 0.5);
        let m20 = DVec3::new(0.5, 0.0, 0.5);
        [
            [e0, m01, m20],
            [m01, e1, m12],
            [m20, m12, e2],
            [m01, m12, m20],
        ]
    };

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

    /// The patch with homogeneous `corners` and edge control points
    /// `edges` (edge `i` from corner `i` to `i + 1`), in the standard
    /// form: each corner weight is scaled to 1, which turns an edge weight
    /// `w` into `w / √(wa·wb)`. Every homogeneous weight must be positive,
    /// and the result must pass [`Patch::check`].
    pub fn from_hom(corners: [DVec4; 3], edges: [DVec4; 3]) -> Result<Self, PatchError> {
        let mut p = [DVec3::ZERO; 3];
        let mut cw = [0.0; 3];
        for i in 0..3 {
            (p[i], cw[i]) = standard_corner::<DVec3>(corners[i])?;
        }
        let mut c = [DVec3::ZERO; 3];
        let mut w = [0.0; 3];
        for i in 0..3 {
            (c[i], w[i]) = standard_edge::<DVec3>(edges[i], cw[i], cw[(i + 1) % 3])?;
        }
        Self::new(p, c, w)
    }

    /// The homogeneous blossom `B(a, b)` of barycentric points `a` and
    /// `b`: `B(a, a)` is the homogeneous point at `a`, and `B(a, b)` the
    /// homogeneous control point of the straight domain edge from `a` to
    /// `b`.
    pub fn blossom(&self, a: DVec3, b: DVec3) -> DVec4 {
        blossom(&self.net(), a, b)
    }

    /// The denominator at `u`: the weight of the homogeneous point there.
    /// It is at least [`W_MIN`](super::W_MIN) in the triangle.
    pub fn weight_at(&self, u: DVec3) -> f64 {
        blossom(&self.net(), u, u).w
    }

    /// The point at barycentric `u`.
    pub fn eval(&self, u: DVec3) -> DVec3 {
        let x = blossom(&self.net(), u, u);
        x.truncate() / x.w
    }

    /// The point at barycentric `u`, and its derivatives along `u0` and
    /// `u1` with `u2 = 1 - u0 - u1`. Their cross product is the normal,
    /// pointing to the side the corners run counter-clockwise from.
    pub fn eval_derivs(&self, u: DVec3) -> [DVec3; 3] {
        let net = self.net();
        let g = rows(&net, u);
        let x = g[0] * u.x + g[1] * u.y + g[2] * u.z;
        let xu = (g[0] - g[2]) * 2.0;
        let xv = (g[1] - g[2]) * 2.0;
        let p = x.truncate() / x.w;
        let d = |dx: DVec4| (dx.truncate() - p * dx.w) / x.w;
        [p, d(xu), d(xv)]
    }

    /// The normal's numerator at barycentric `u` (summing to 1): the
    /// derivatives' cross product (see [`Self::eval_derivs`]) times the
    /// cube of [`Self::weight_at`], which is positive. It is the cubic the
    /// [`normal_coeffs`](Self::normal_coeffs) are the Bernstein
    /// coefficients of.
    pub fn normal(&self, u: DVec3) -> DVec3 {
        let net = self.net_from(self.p[0]);
        let g = rows(&net, u);
        cross4(g[0], g[1], g[2]) * 4.0
    }

    /// The sub-patch over the barycentric triangle `domain`, exact by
    /// blossoming. A domain running clockwise gives the reversed patch.
    pub fn sub(&self, domain: [DVec3; 3]) -> Result<Self, PatchError> {
        let net = self.net();
        let corners = domain.map(|d| blossom(&net, d, d));
        let edges = [0, 1, 2].map(|i| blossom(&net, domain[i], domain[(i + 1) % 3]));
        Self::from_hom(corners, edges)
    }

    /// The four children of splitting at the edge midpoints, in the
    /// layout of [`Self::SPLIT4_DOMAINS`]. Each edge is split by
    /// [`Conic3::split_half`], which is symmetric to the bit, so a
    /// neighbour splitting its side of the edge at `½` (by `split4` or
    /// `bisect`) gets the same halves and midpoint.
    pub fn split4(&self) -> Result<[Self; 4], PatchError> {
        let halves = [
            self.edge(0).split_half()?,
            self.edge(1).split_half()?,
            self.edge(2).split_half()?,
        ];
        let net = self.net();
        let [[_, m01, _], [_, _, m12], [m20, _, _], _] = Self::SPLIT4_DOMAINS;
        let inner = |a: DVec3, b: DVec3| {
            let h = blossom(&net, a, b);
            standard_edge::<DVec3>(h, blossom(&net, a, a).w, blossom(&net, b, b).w)
        };
        // Edges between the midpoints: 01–12, 12–20 and 20–01.
        let ea = inner(m01, m12)?;
        let eb = inner(m12, m20)?;
        let ec = inner(m20, m01)?;
        let m = [halves[0][0].p1, halves[1][0].p1, halves[2][0].p1];
        let side = |c: &Conic3| (c.c, c.w);
        let [p0, p1, p2] = self.p;
        Ok([
            assemble(
                [p0, m[0], m[2]],
                [side(&halves[0][0]), ec, side(&halves[2][1])],
            )?,
            assemble(
                [m[0], p1, m[1]],
                [side(&halves[0][1]), side(&halves[1][0]), ea],
            )?,
            assemble(
                [m[2], m[1], p2],
                [eb, side(&halves[1][1]), side(&halves[2][0])],
            )?,
            assemble([m[0], m[1], m[2]], [ea, eb, ec])?,
        ])
    }

    /// The two children of splitting edge `edge` at `t`, in `(0, 1)`, and
    /// joining the split point to the opposite corner. With `a`, `b` and
    /// `o` corners `edge`, `edge + 1` and `edge + 2`, and `m` the split
    /// point, the children are `(a, m, o)` and `(m, b, o)`.
    ///
    /// The edge is split by [`Conic3::split`]; at `½` a neighbour doing the
    /// same from its side gets the same bits. Elsewhere, split the shared
    /// edge once and use [`Self::bisect_with`] on both sides.
    pub fn bisect(&self, edge: usize, t: f64) -> Result<[Self; 2], PatchError> {
        if edge > 2 {
            return Err(PatchError::Parameter(edge as f64));
        }
        let halves = self.edge(edge).split(t)?;
        self.bisect_with(edge, t, halves)
    }

    /// [`Self::bisect`] with the edge's halves given: from corner `edge`
    /// to the split point at `t`, and on to corner `edge + 1`. The halves
    /// must come from splitting this edge at `t` (the neighbour's side at
    /// `1 - t`, reversed): only their ends are checked, and the new inner
    /// edge is built for `t`.
    pub fn bisect_with(
        &self,
        edge: usize,
        t: f64,
        halves: [Conic3; 2],
    ) -> Result<[Self; 2], PatchError> {
        if edge > 2 {
            return Err(PatchError::Parameter(edge as f64));
        }
        if !(t > 0.0 && t < 1.0) {
            return Err(PatchError::Parameter(t));
        }
        self.check_halves(edge, &halves)?;
        let (ia, ib, io) = (edge, (edge + 1) % 3, (edge + 2) % 3);
        let net = self.net();
        let [a, b, o] = [ia, ib, io].map(|i| DVec3::AXES[i]);
        let md = a * (1.0 - t) + b * t;
        let inner = standard_edge::<DVec3>(blossom(&net, md, o), blossom(&net, md, md).w, 1.0)?;
        let m = halves[0].p1;
        let side = |c: &Conic3| (c.c, c.w);
        Ok([
            assemble(
                [self.p[ia], m, self.p[io]],
                [side(&halves[0]), inner, (self.c[io], self.w[io])],
            )?,
            assemble(
                [m, self.p[ib], self.p[io]],
                [side(&halves[1]), (self.c[ib], self.w[ib]), inner],
            )?,
        ])
    }

    /// Refuses halves that don't run from corner `i` through one shared
    /// point to corner `i + 1`.
    fn check_halves(&self, i: usize, halves: &[Conic3; 2]) -> Result<(), PatchError> {
        if halves[0].p0 == self.p[i]
            && halves[1].p1 == self.p[(i + 1) % 3]
            && halves[0].p1 == halves[1].p0
        {
            Ok(())
        } else {
            Err(PatchError::Mismatch)
        }
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
pub(super) fn blossom(net: &[[DVec4; 3]; 3], a: DVec3, b: DVec3) -> DVec4 {
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
    [0, 1, 2].map(|i| net[i][0] * u.x + net[i][1] * u.y + net[i][2] * u.z)
}

/// The spatial part of the 4D cross product of homogeneous points: the
/// normal of the plane through the three points they stand for.
/// Alternating and linear in each argument.
pub(super) fn cross4(a: DVec4, b: DVec4, c: DVec4) -> DVec3 {
    let (a3, b3, c3) = (a.truncate(), b.truncate(), c.truncate());
    b3.cross(c3) * a.w - a3.cross(c3) * b.w + a3.cross(b3) * c.w
}

/// The patch with corners `p` and edges `(control point, weight)`.
fn assemble(p: [DVec3; 3], edges: [(DVec3, f64); 3]) -> Result<Patch, PatchError> {
    Patch::new(p, edges.map(|e| e.0), edges.map(|e| e.1))
}
