//! Tensor-product Bernstein forms on `[0, 1]²` ([`Tensor`]) and rational
//! patches in homogeneous coordinates ([`HomPatch`]): evaluation, partial
//! derivatives, subdivision in either direction and the hull.
//!
//! Every operation in one direction acts on the lines of the net in that
//! direction, each a [`Poly`], by the 1D operation; so the 1D guarantees
//! carry over line by line. **Same bits under reversal and swap.** The
//! split at ½ in `u` of the net reversed in `u` gives this net's halves
//! swapped and reversed in `u`, to the bit, and likewise in `v`; the
//! split in `v` of the transposed net is the split in `u` transposed, to
//! the bit. Two patches sharing a boundary or meeting with their
//! directions swapped split into the same pieces from either side, and
//! evaluate on the shared line to the same bits: evaluation there uses
//! the boundary line alone.
//!
//! **Rational patches.** A [`HomPatch`] holds the net `(w·x, w·y, w·z, w)`
//! as four tensors of the same degrees, given homogeneous or formed at
//! the level from affine control points and their weights
//! ([`HomPatch::from_affine`]). Its point is `(X, Y, Z) / W`; its
//! homogeneous partials are tensors one degree lower in their direction,
//! and the partials of the point follow by the quotient rule,
//! `x_u = (X_u − x·W_u) / W`. With every weight certified positive the
//! patch lies in the convex hull of its projected control points, which
//! [`HomPatch::hull_box`] bounds.

use smallvec::SmallVec;

use super::poly::{Coefs, Hull, MAX_DEGREE, Poly, derive, hull_of};
use super::scalar::Scalar;
use crate::num::bounded::{Bounded, down};

/// A parameter direction of a tensor-product form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    /// The first parameter, `u` (the row index `i` of the net).
    U,
    /// The second parameter, `v` (the column index `j`).
    V,
}

/// Coefficients stored inline up to this many (a biquadratic net).
const INLINE: usize = 9;

/// The net's storage, row-major in `u`: `bᵢⱼ` at `i·(n + 1) + j`.
type Net<S> = SmallVec<[S; INLINE]>;

/// `f(u, v) = Σᵢⱼ bᵢⱼ·Bᵢᵐ(u)·Bⱼⁿ(v)` on `[0, 1]²`, of degree `m` in `u`
/// and `n` in `v` (each at most [`MAX_DEGREE`]), the `Bᵏ` the Bernstein
/// basis of [`Poly`].
///
/// At a [`Bounded`] level every result carries a certified bound on its
/// distance from the same operation on the true coefficients; with
/// [`malachite::Rational`] coefficients every operation is exact.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor<S> {
    m: usize,
    n: usize,
    c: Net<S>,
}

impl<S: Scalar> Tensor<S> {
    /// The form of degrees `(m, n)` with the coefficients `b₀₀, b₀₁, …,
    /// b₀ₙ, b₁₀, …` (row-major in `u`), `None` unless both degrees are at
    /// most [`MAX_DEGREE`] and there are `(m + 1)·(n + 1)` of them.
    pub fn new(m: usize, n: usize, c: impl IntoIterator<Item = S>) -> Option<Self> {
        if m > MAX_DEGREE || n > MAX_DEGREE {
            return None;
        }
        let len = (m + 1) * (n + 1);
        let c: Net<S> = c.into_iter().take(len + 1).collect();
        (c.len() == len).then_some(Tensor { m, n, c })
    }

    /// The degrees `(m, n)`.
    #[inline]
    pub fn degrees(&self) -> (usize, usize) {
        (self.m, self.n)
    }

    /// The degree in `dir`.
    #[inline]
    pub fn degree(&self, dir: Dir) -> usize {
        match dir {
            Dir::U => self.m,
            Dir::V => self.n,
        }
    }

    /// The coefficients, row-major in `u` ([`new`](Self::new)'s order).
    #[inline]
    pub fn coefs(&self) -> &[S] {
        &self.c
    }

    /// `bᵢⱼ`; panics past the degrees.
    #[inline]
    pub fn get(&self, i: usize, j: usize) -> &S {
        assert!(i <= self.m && j <= self.n, "index past the degrees");
        &self.c[i * (self.n + 1) + j]
    }

    /// The number of lines in `dir` (each a polynomial in `dir`).
    fn lines(&self, dir: Dir) -> usize {
        match dir {
            Dir::U => self.n + 1,
            Dir::V => self.m + 1,
        }
    }

    /// Line `k` in `dir`: column `j = k` for `u`, row `i = k` for `v`.
    fn line(&self, dir: Dir, k: usize) -> Coefs<S> {
        match dir {
            Dir::U => (0..=self.m)
                .map(|i| self.c[i * (self.n + 1) + k].clone())
                .collect(),
            Dir::V => self.c[k * (self.n + 1)..(k + 1) * (self.n + 1)]
                .iter()
                .cloned()
                .collect(),
        }
    }

    /// The form whose lines in `dir` are `lines` (all of one length, at
    /// most [`MAX_DEGREE`] + 1), in order.
    fn from_lines(dir: Dir, lines: &[Coefs<S>]) -> Self {
        let deg = lines[0].len() - 1;
        let other = lines.len() - 1;
        debug_assert!(lines.iter().all(|l| l.len() == deg + 1) && deg <= MAX_DEGREE);
        match dir {
            Dir::V => Tensor {
                m: other,
                n: deg,
                c: lines.iter().flatten().cloned().collect(),
            },
            Dir::U => {
                let c = (0..=deg)
                    .flat_map(|i| lines.iter().map(move |l| l[i].clone()))
                    .collect();
                Tensor {
                    m: deg,
                    n: other,
                    c,
                }
            }
        }
    }

    /// Every line in `dir` through `f`, which keeps one length.
    pub(super) fn map_lines(&self, dir: Dir, f: impl Fn(Poly<S>) -> Coefs<S>) -> Self {
        let lines: SmallVec<[Coefs<S>; 3]> = (0..self.lines(dir))
            .map(|k| f(Poly::from_coefs(self.line(dir, k))))
            .collect();
        Self::from_lines(dir, &lines)
    }

    /// Every line in `dir` split in two by `f`: the two forms of the
    /// pieces, in the order `f` gives them.
    fn split_lines(&self, dir: Dir, f: impl Fn(&Poly<S>) -> [Poly<S>; 2]) -> [Self; 2] {
        let mut a: SmallVec<[Coefs<S>; 3]> = SmallVec::new();
        let mut b: SmallVec<[Coefs<S>; 3]> = SmallVec::new();
        for k in 0..self.lines(dir) {
            let [l, r] = f(&Poly::from_coefs(self.line(dir, k)));
            a.push(l.coefs().iter().cloned().collect());
            b.push(r.coefs().iter().cloned().collect());
        }
        [Self::from_lines(dir, &a), Self::from_lines(dir, &b)]
    }

    /// `f(u, v)`. On a boundary line (`u` or `v` exactly 0 or 1) the
    /// boundary line alone at the other parameter ([`Poly::eval`]; a
    /// corner is its coefficient): the same bits, error parts included,
    /// as the neighbour sharing that line gets, and under
    /// [`transposed`](Self::transposed). Inside, each column evaluated at
    /// `u` ([`Poly::eval`], so the first level's fast path for an exact
    /// `u` in `[0, 1]`), then the resulting row at `v`; not the same bits
    /// as the transposed form's, which goes rows first.
    pub fn eval(&self, u: &S, v: &S) -> S {
        if let Some(i) = end_index(u, self.m) {
            return Poly::from_coefs(self.line(Dir::V, i)).eval(v);
        }
        if let Some(j) = end_index(v, self.n) {
            return Poly::from_coefs(self.line(Dir::U, j)).eval(u);
        }
        let row: Coefs<S> = (0..=self.n)
            .map(|j| Poly::from_coefs(self.line(Dir::U, j)).eval(u))
            .collect();
        Poly::from_coefs(row).eval(v)
    }

    /// The same form with `u` and `v` swapped, exactly.
    pub fn transposed(&self) -> Self {
        let c = (0..=self.n)
            .flat_map(|j| (0..=self.m).map(move |i| (i, j)))
            .map(|(i, j)| self.c[i * (self.n + 1) + j].clone())
            .collect();
        Tensor {
            m: self.n,
            n: self.m,
            c,
        }
    }

    /// The form reversed in `dir` (`f(1 − u, v)` or `f(u, 1 − v)`),
    /// exactly.
    pub fn reversed(&self, dir: Dir) -> Self {
        self.map_lines(dir, |p| p.coefs().iter().rev().cloned().collect())
    }

    /// The partial derivative in `dir`, one degree lower in it
    /// ([`Poly::derivative`] on each line); a form of degree 0 in `dir`
    /// has the zero form.
    pub fn derivative(&self, dir: Dir) -> Self {
        self.map_lines(dir, |p| derive(p.coefs()))
    }

    /// The halves on `[0, ½]` and `[½, 1]` in `dir`, each as a form on
    /// `[0, 1]²`, by [`Poly::split_half`] on every line: symmetric to the
    /// bit under reversal and transposition (module docs).
    pub fn split_half(&self, dir: Dir) -> [Self; 2] {
        self.split_lines(dir, Poly::split_half)
    }

    /// The pieces on `[0, t]` and `[t, 1]` in `dir`, by
    /// [`Poly::split_at`] on every line.
    pub fn split_at(&self, dir: Dir, t: &S) -> [Self; 2] {
        self.split_lines(dir, |p| p.split_at(t))
    }

    /// The coefficient hull: a certified enclosure of `f` on `[0, 1]²`.
    pub fn hull(&self) -> Hull {
        hull_of(&self.c)
    }
}

impl Tensor<Bounded<f64>> {
    /// From exact `f64` coefficients ([`Tensor::new`]'s order and limits).
    pub fn from_f64(m: usize, n: usize, c: &[f64]) -> Option<Self> {
        Tensor::new(m, n, c.iter().map(|&x| Bounded::exact(x)))
    }
}

/// A rational patch on `[0, 1]²` in homogeneous coordinates: the net
/// `Pᵢⱼ = (wᵢⱼ·xᵢⱼ, wᵢⱼ·yᵢⱼ, wᵢⱼ·zᵢⱼ, wᵢⱼ)` as the tensors `X, Y, Z, W`
/// of one pair of degrees. The point at `(u, v)` is
/// `(X, Y, Z)(u, v) / W(u, v)`.
///
/// Evaluating the point divides by `W`: certified at a [`Bounded`] level
/// only where `W` is certainly non-zero, and with [`malachite::Rational`]
/// coefficients a panic where it is zero (as `Rational`'s division).
/// Positive weights keep `W` positive on `[0, 1]²`.
#[derive(Clone, Debug, PartialEq)]
pub struct HomPatch<S> {
    h: [Tensor<S>; 4],
}

/// A point of a rational patch with its partial derivatives.
#[derive(Clone, Debug, PartialEq)]
pub struct Partials<S> {
    /// The point `(x, y, z)`.
    pub p: [S; 3],
    /// `∂p/∂u`.
    pub du: [S; 3],
    /// `∂p/∂v`.
    pub dv: [S; 3],
}

/// A certified axis-aligned box: `lo[k] ≤ p[k] ≤ hi[k]` for every point
/// `p` it bounds, a side `±∞` where nothing finite is certified.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HullBox {
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

impl HullBox {
    /// Whether `p` lies in the box (sides included).
    pub fn contains(&self, p: [f64; 3]) -> bool {
        (0..3).all(|k| self.lo[k] <= p[k] && p[k] <= self.hi[k])
    }
}

impl<S: Scalar> HomPatch<S> {
    /// The patch of degrees `(m, n)` with the homogeneous control points
    /// `(w·x, w·y, w·z, w)` row-major in `u` ([`Tensor::new`]'s order and
    /// limits).
    pub fn new(m: usize, n: usize, net: impl IntoIterator<Item = [S; 4]>) -> Option<Self> {
        if m > MAX_DEGREE || n > MAX_DEGREE {
            return None;
        }
        let mut h: [Net<S>; 4] = Default::default();
        for p in net.into_iter().take((m + 1) * (n + 1) + 1) {
            for (k, x) in p.into_iter().enumerate() {
                h[k].push(x);
            }
        }
        let [x, y, z, w] = h;
        Some(HomPatch {
            h: [
                Tensor::new(m, n, x)?,
                Tensor::new(m, n, y)?,
                Tensor::new(m, n, z)?,
                Tensor::new(m, n, w)?,
            ],
        })
    }

    /// The patch of degrees `(m, n)` with the affine control points
    /// `(x, y, z, w)` (a point and its weight, as stored) row-major in
    /// `u`: the homogeneous coefficients `w·x` formed at `like`'s level by
    /// [`Scalar::product`], so exact from double-double up, and at the
    /// first level rounded once per coefficient with the rounding carried
    /// (none where the product is a float: a weight of 1, a zero
    /// coordinate). Limits as for [`new`](Self::new).
    pub fn from_affine(
        m: usize,
        n: usize,
        net: impl IntoIterator<Item = [f64; 4]>,
        like: &S,
    ) -> Option<Self> {
        Self::new(
            m,
            n,
            net.into_iter().map(|[x, y, z, w]| {
                [
                    S::product(x, w, like),
                    S::product(y, w, like),
                    S::product(z, w, like),
                    S::from_f64(w, like),
                ]
            }),
        )
    }

    /// The patch with the coordinate tensors `[X, Y, Z, W]`, `None` unless
    /// they have one pair of degrees.
    pub fn from_tensors(h: [Tensor<S>; 4]) -> Option<Self> {
        let d = h[0].degrees();
        h.iter().all(|t| t.degrees() == d).then_some(HomPatch { h })
    }

    /// The coordinate tensors `[X, Y, Z, W]`.
    #[inline]
    pub fn tensors(&self) -> &[Tensor<S>; 4] {
        &self.h
    }

    /// The weights `W`.
    #[inline]
    pub fn weights(&self) -> &Tensor<S> {
        &self.h[3]
    }

    /// The degrees `(m, n)`.
    #[inline]
    pub fn degrees(&self) -> (usize, usize) {
        self.h[0].degrees()
    }

    fn map(&self, f: impl Fn(&Tensor<S>) -> Tensor<S>) -> Self {
        HomPatch {
            h: [f(&self.h[0]), f(&self.h[1]), f(&self.h[2]), f(&self.h[3])],
        }
    }

    fn split(&self, f: impl Fn(&Tensor<S>) -> [Tensor<S>; 2]) -> [Self; 2] {
        let [[x0, x1], [y0, y1], [z0, z1], [w0, w1]] =
            [f(&self.h[0]), f(&self.h[1]), f(&self.h[2]), f(&self.h[3])];
        [
            HomPatch {
                h: [x0, y0, z0, w0],
            },
            HomPatch {
                h: [x1, y1, z1, w1],
            },
        ]
    }

    /// The homogeneous point `(X, Y, Z, W)(u, v)`.
    pub fn eval_hom(&self, u: &S, v: &S) -> [S; 4] {
        [0, 1, 2, 3].map(|k| self.h[k].eval(u, v))
    }

    /// The point `(X, Y, Z)(u, v) / W(u, v)`.
    pub fn eval(&self, u: &S, v: &S) -> [S; 3] {
        let [x, y, z, w] = self.eval_hom(u, v);
        [x.div(&w), y.div(&w), z.div(&w)]
    }

    /// The homogeneous partial derivative in `dir`, `(X, Y, Z, W)_u` or
    /// `_v`, one degree lower in it ([`Tensor::derivative`]).
    pub fn derivative(&self, dir: Dir) -> Self {
        self.map(|t| t.derivative(dir))
    }

    /// The point and its partials at `(u, v)`, by the quotient rule from
    /// the homogeneous point and partials: `x_u = (X_u − x·W_u) / W`.
    pub fn eval_partials(&self, u: &S, v: &S) -> Partials<S> {
        let [x, y, z, w] = self.eval_hom(u, v);
        let p = [x.div(&w), y.div(&w), z.div(&w)];
        let partial = |dir| {
            let d = self.derivative(dir).eval_hom(u, v);
            [0, 1, 2].map(|k| d[k].sub(&p[k].mul(&d[3])).div(&w))
        };
        let du = partial(Dir::U);
        let dv = partial(Dir::V);
        Partials { p, du, dv }
    }

    /// The same patch with `u` and `v` swapped, exactly.
    pub fn transposed(&self) -> Self {
        self.map(Tensor::transposed)
    }

    /// The patch reversed in `dir`, exactly.
    pub fn reversed(&self, dir: Dir) -> Self {
        self.map(|t| t.reversed(dir))
    }

    /// The halves at ½ in `dir` ([`Tensor::split_half`]), symmetric to
    /// the bit under reversal and transposition.
    pub fn split_half(&self, dir: Dir) -> [Self; 2] {
        self.split(|t| t.split_half(dir))
    }

    /// The pieces on `[0, t]` and `[t, 1]` in `dir`
    /// ([`Tensor::split_at`]).
    pub fn split_at(&self, dir: Dir, t: &S) -> [Self; 2] {
        self.split(|h| h.split_at(dir, t))
    }

    /// Whether every weight is certified positive (its enclosure's lower
    /// end above zero): then `W > 0` on `[0, 1]²` and the patch lies in
    /// the convex hull of its projected control points.
    pub fn weights_positive(&self) -> bool {
        self.h[3].coefs().iter().all(|w| w.enclose()[0] > 0.0)
    }

    /// A certified box holding every point of the patch on `[0, 1]²`: the
    /// projected control points `(w·x) / w` bounded outwards from the
    /// enclosures of `w·x` and `w`. `None` unless every weight is
    /// certified positive ([`weights_positive`](Self::weights_positive)).
    pub fn hull_box(&self) -> Option<HullBox> {
        let mut b = HullBox {
            lo: [f64::INFINITY; 3],
            hi: [f64::NEG_INFINITY; 3],
        };
        let ws = self.h[3].coefs();
        for (idx, w) in ws.iter().enumerate() {
            let [wl, wh] = w.enclose();
            if wl <= 0.0 || wl.is_nan() {
                return None;
            }
            for k in 0..3 {
                let [lo, hi] = quotient(self.h[k].coefs()[idx].enclose(), [wl, wh]);
                if lo < b.lo[k] {
                    b.lo[k] = lo;
                }
                if hi > b.hi[k] {
                    b.hi[k] = hi;
                }
            }
        }
        Some(b)
    }
}

impl HomPatch<Bounded<f64>> {
    /// From exact `f64` homogeneous control points `(w·x, w·y, w·z, w)`
    /// ([`HomPatch::new`]'s order and limits).
    pub fn from_f64(m: usize, n: usize, net: &[[f64; 4]]) -> Option<Self> {
        HomPatch::new(m, n, net.iter().map(|p| p.map(Bounded::exact)))
    }
}

/// An enclosure of `x / w` for `x` in `[xl, xh]` and `w` in `[wl, wh]`,
/// `wl > 0`: the least quotient is `xl/wh` for `xl ≥ 0` and `xl/wl`
/// otherwise, the greatest `xh/wl` for `xh ≥ 0` and `xh/wh` otherwise,
/// each rounded outwards ([`down`]; a NaN, as `∞/∞`, gives `∓∞`).
pub(super) fn quotient([xl, xh]: [f64; 2], [wl, wh]: [f64; 2]) -> [f64; 2] {
    let lo = down(xl / if xl >= 0.0 { wh } else { wl });
    let hi = -down(-(xh / if xh >= 0.0 { wl } else { wh }));
    [lo, hi]
}

/// The index of the end line at a parameter exactly 0 (`0`) or 1
/// (`deg`).
fn end_index<S: Scalar>(t: &S, deg: usize) -> Option<usize> {
    if t.is_int(0) {
        Some(0)
    } else if t.is_int(1) {
        Some(deg)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
