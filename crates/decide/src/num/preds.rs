//! The flat predicate library: every exact sign the kernel reads on lines
//! and planes, as polynomials in stored coordinates.
//!
//! **Forms.** Each predicate is written once, as a function over
//! [`Num`] ([`along`], [`orient2`], [`dot2`], [`orient3`],
//! [`orient3_dir`], [`plane_side`], [`shadow_side`], [`plane_height`],
//! [`crossing_height`], [`hom_sub`], [`near_gap`]): unnormalised and
//! sign-equivalent, of fixed degree, with no division and no root, so
//! [`exact::sign`](super::exact::sign) works it out exactly through its
//! stages (the `f64` filter, expansions, rationals where an expansion
//! would leave its range), each stage reported for its kind's price
//! ([`Work`](super::exact::Work)). A direction a form takes (the frame's
//! `UP`, `r̂`, `ν`, a stored normal) is exact and unnormalised, so its
//! value is a positive multiple of the spatial one: a squared length, a
//! facing of the unmoved data.
//!
//! **Predicates.** The types here ([`Orient2d`], [`Dot2d`], [`Orient3d`],
//! [`Orient3Dir`], [`PlaneSide`], [`Along`], [`NearGap`]) are the forms
//! on stored points that don't move: the certificates and checks on one
//! operand's geometry, never perturbed. The perturbed rows of the
//! records (the vertex–vertex and vertex–line shadows, the heights, the
//! edge–edge crossing; [`crate::perturb`]) are the same forms on moving
//! points, their orders 2 to 4 the same rows with the fixed translations
//! for the rates: `perturb` derives the rows, these forms are their
//! values.
//!
//! **Kinds.** A form's kind ([`PredKind`]) prices its stages, one price
//! per kind and stage: [`along`] and [`plane_side`] are
//! [`PredKind::Side`] (degree 2, three products of an input and a
//! difference or an input); [`orient2`], [`dot2`] and [`near_gap`]
//! [`PredKind::Orient2`]; [`orient3`], [`orient3_dir`], [`shadow_side`]
//! and [`plane_height`] [`PredKind::Orient3`]; [`crossing_height`]
//! [`PredKind::Crossing`] (degree 4); the shadow rows of homogeneous
//! points ([`hom_sub`]) [`PredKind::Hom`].

use glam::DVec3;
use malachite::Rational;

use super::exact::{Num, Pred, V3, cross, det3, dot, point, sub};
use super::schedule::PredKind;

/// `dir·(w − q)`: how far `w` lies past `q` along `dir`, times `|dir|`.
/// The side of a shadow line, the place along it, a height above a
/// point.
pub fn along<N: Num>(dir: &V3<N>, q: &V3<N>, w: &V3<N>) -> N {
    dot(dir, &sub(w, q))
}

/// `orient(a, b, c) = (b − a) × (c − a)` in the plane: positive when `c`
/// lies left of `a → b`, twice the triangle's signed area.
pub fn orient2<N: Num>(a: &[N; 2], b: &[N; 2], c: &[N; 2]) -> N {
    let (bx, by) = (b[0].sub(&a[0]), b[1].sub(&a[1]));
    let (cx, cy) = (c[0].sub(&a[0]), c[1].sub(&a[1]));
    bx.mul(&cy).sub(&by.mul(&cx))
}

/// `(b − a)·(d − c)` in the plane: positive when the segments run the
/// same way.
pub fn dot2<N: Num>(a: &[N; 2], b: &[N; 2], c: &[N; 2], d: &[N; 2]) -> N {
    let (x, y) = (b[0].sub(&a[0]), b[1].sub(&a[1]));
    let (z, w) = (d[0].sub(&c[0]), d[1].sub(&c[1]));
    x.mul(&z).add(&y.mul(&w))
}

/// `det(b − a, c − a, d − a)`: positive when `d` lies on the side of the
/// plane of `a, b, c` its normal `(b − a) × (c − a)` points to, six
/// times the tetrahedron's signed volume.
pub fn orient3<N: Num>(a: &V3<N>, b: &V3<N>, c: &V3<N>, d: &V3<N>) -> N {
    det3(&sub(b, a), &sub(c, a), &sub(d, a))
}

/// `det(a − p, b − p, d)`: the side of the line through `p` along `d`
/// that the segment `a → b` passes, or, with `p, a, b` a triangle, the
/// facing of the direction `d` against its normal.
pub fn orient3_dir<N: Num>(p: &V3<N>, a: &V3<N>, b: &V3<N>, d: &V3<N>) -> N {
    det3(&sub(a, p), &sub(b, p), d)
}

/// `n·x − c`: the side of the plane `n·x = c` the point `x` lies on,
/// positive on the side `n` points to, its distance times `|n|`.
pub fn plane_side<N: Num>(n: &V3<N>, c: &N, x: &V3<N>) -> N {
    dot(n, x).sub(c)
}

/// `(up × d)·off`: the side of the shadow along `up` of a line along `d`
/// that a point `off` from the line's point lies on, positive to its
/// left seen from above, times `|up × d|·|up|`. Zero for a line along
/// `up`.
pub fn shadow_side<N: Num>(up: &V3<N>, d: &V3<N>, off: &V3<N>) -> N {
    dot(&cross(up, d), off)
}

/// `(c − n·q)·(n·up)`: the height of the plane `n·x = c` above `q` along
/// `up`, times `(n·up)²/|up|`, so positive with the plane above `q`
/// whichever way `n` faces. Zero on a plane along `up`.
pub fn plane_height<N: Num>(n: &V3<N>, c: &N, q: &V3<N>, up: &V3<N>) -> N {
    c.sub(&dot(n, q)).mul(&dot(n, up))
}

/// `(up·m)·(m·(g₀ − e₀))`, `m = (e₁ − e₀) × (g₁ − g₀)`: the height of the
/// line `g` above the line `e` along `up` where their shadows cross,
/// times `(m·up)²/|up|`. Zero where `m·up = 0`: parallel shadows, or a
/// line along `up`.
pub fn crossing_height<N: Num>(up: &V3<N>, e: [&V3<N>; 2], g: [&V3<N>; 2]) -> N {
    let m = cross(&sub(e[1], e[0]), &sub(g[1], g[0]));
    dot(up, &m).mul(&dot(&m, &sub(g[0], e[0])))
}

/// `W_q·X_p − W_p·X_q` for homogeneous points `p = X_p/W_p`, `q =
/// X_q/W_q`: `p − q` times `W_p·W_q`, positive with positive weights. A
/// weighted control point enters as its stored `(x, w)`, its products
/// formed in the stages, never rounded.
pub fn hom_sub<N: Num>(p: (&V3<N>, &N), q: (&V3<N>, &N)) -> V3<N> {
    let ((xp, wp), (xq, wq)) = (p, q);
    [0, 1, 2].map(|k| wq.mul(&xp[k]).sub(&wp.mul(&xq[k])))
}

/// `r² − ‖w − q‖²`: at least zero when the points are within `r`.
pub fn near_gap<N: Num>(r: &N, q: &V3<N>, w: &V3<N>) -> N {
    let d = sub(w, q);
    r.mul(r).sub(&dot(&d, &d))
}

/// A stored 2D point as inputs.
fn point2<N: Num>(p: [f64; 2]) -> [N; 2] {
    p.map(N::lit)
}

/// [`orient2`] of stored points `[a, b, c]`: positive when `c` lies left
/// of `a → b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orient2d(pub [[f64; 2]; 3]);

impl Pred for Orient2d {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c] = self.0.map(point2::<N>);
        orient2(&a, &b, &c)
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient2
    }
}

/// [`dot2`] of stored points `[a, b, c, d]`: `(b − a)·(d − c)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot2d(pub [[f64; 2]; 4]);

impl Pred for Dot2d {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c, d] = self.0.map(point2::<N>);
        dot2(&a, &b, &c, &d)
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient2
    }
}

/// [`orient3`] of stored points `[a, b, c, d]`: `det(b − a, c − a, d −
/// a)`, positive with `d` on the side the normal of `a, b, c` points to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orient3d(pub [DVec3; 4]);

impl Pred for Orient3d {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c, d] = self.0.map(point::<N>);
        orient3(&a, &b, &c, &d)
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient3
    }
}

/// [`orient3_dir`] of stored points `p`, `a`, `b` and a stored direction
/// `d`: `det(a − p, b − p, d)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orient3Dir {
    /// The point the differences are taken from.
    pub p: DVec3,
    /// The first other point.
    pub a: DVec3,
    /// The second.
    pub b: DVec3,
    /// The direction.
    pub d: DVec3,
}

impl Pred for Orient3Dir {
    fn eval<N: Num>(&self) -> N {
        orient3_dir(
            &point(self.p),
            &point(self.a),
            &point(self.b),
            &point(self.d),
        )
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient3
    }
}

/// [`plane_side`] of a stored point against a stored plane `n·x = c`:
/// `n·p − c`, positive on the side `n` points to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneSide {
    /// The plane's normal `n` (any scale: a stored normal of largest
    /// component 1).
    pub normal: DVec3,
    /// The offset `c`.
    pub offset: f64,
    /// The point.
    pub p: DVec3,
}

impl Pred for PlaneSide {
    fn eval<N: Num>(&self) -> N {
        plane_side(&point(self.normal), &N::lit(self.offset), &point(self.p))
    }

    fn kind(&self) -> PredKind {
        PredKind::Side
    }
}

/// [`along`] of stored points along a stored direction: `dir·(w − q)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Along {
    /// The direction (any scale).
    pub dir: DVec3,
    /// The point measured from.
    pub q: DVec3,
    /// The point measured.
    pub w: DVec3,
}

impl Pred for Along {
    fn eval<N: Num>(&self) -> N {
        along(&point(self.dir), &point(self.q), &point(self.w))
    }

    fn kind(&self) -> PredKind {
        PredKind::Side
    }
}

/// [`near_gap`] of stored points within a radius: `r² − ‖w − q‖²`, at
/// least zero when they are within `r`. The radius is any finite `r ≥
/// 0` (a sum of bands), so it enters as an exact rational, not bound by
/// the coordinates' input range.
#[derive(Clone, Debug, PartialEq)]
pub struct NearGap {
    /// One point.
    pub q: DVec3,
    /// The other.
    pub w: DVec3,
    /// The radius.
    pub r: Rational,
}

impl Pred for NearGap {
    fn eval<N: Num>(&self) -> N {
        near_gap(&N::rational(&self.r), &point(self.q), &point(self.w))
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient2
    }
}

#[cfg(test)]
mod tests;
