//! A rational curve span in homogeneous coordinates ([`HomCurve`]):
//! evaluation, reversal, subdivision, elevation and the homogeneous
//! derivative, each coordinate by the 1D operation of [`Poly`], so the 1D
//! guarantees (the same bits under reversal for the split at ½ and
//! elevation, the end rule of evaluation) carry over.

use super::poly::{MAX_DEGREE, Poly};
use super::scalar::Scalar;
use super::tensor::quotient;

/// A rational span in `N` dimensions: the polynomials `P = (w·x₁, …,
/// w·x_N)` and `W`, of one degree. The point at `t` is `P(t) / W(t)`;
/// `N = 3` for a space curve, `N = 2` for a curve in a plane (a shadow).
/// Built from homogeneous control points ([`new`](Self::new)) or from
/// affine ones with their weights, the products formed at the level
/// ([`from_affine`](Self::from_affine)).
///
/// Evaluating the point divides by `W`: certified at a
/// [`Bounded`](crate::num::bounded::Bounded) level only where `W` is
/// certainly non-zero, and with [`malachite::Rational`] coefficients a
/// panic where it is zero. Positive weights keep `W` positive on
/// `[0, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct HomCurve<S, const N: usize> {
    p: [Poly<S>; N],
    w: Poly<S>,
}

impl<S: Scalar, const N: usize> HomCurve<S, N> {
    /// The span with the homogeneous control points `(w·x, w)`, `b₀`
    /// first; `None` unless there are `1..=MAX_DEGREE + 1` of them.
    pub fn new(pts: impl IntoIterator<Item = ([S; N], S)>) -> Option<Self> {
        let mut p: [Vec<S>; N] = std::array::from_fn(|_| Vec::new());
        let mut w = Vec::new();
        for (x, wt) in pts.into_iter().take(MAX_DEGREE + 2) {
            for (k, c) in x.into_iter().enumerate() {
                p[k].push(c);
            }
            w.push(wt);
        }
        let w = Poly::new(w)?;
        let p = p.map(Poly::new);
        if p.iter().any(Option::is_none) {
            return None;
        }
        Self::from_polys(p.map(|c| c.expect("checked")), w)
    }

    /// The span with the affine control points `(x, w)` (a point and its
    /// weight, as stored), `b₀` first: the homogeneous coefficients
    /// `(w·x, w)` formed at `like`'s level by [`Scalar::product`], so
    /// exact from double-double up, and at the first level rounded once
    /// per coefficient with the rounding carried (none where the product
    /// is a float: a weight of 1, a zero coordinate). `None` as for
    /// [`new`](Self::new).
    pub fn from_affine(pts: impl IntoIterator<Item = ([f64; N], f64)>, like: &S) -> Option<Self> {
        Self::new(
            pts.into_iter()
                .map(|(x, w)| (x.map(|c| S::product(c, w, like)), S::from_f64(w, like))),
        )
    }

    /// The span with the coordinate polynomials `P` and weights `W`,
    /// `None` unless they have one degree.
    pub fn from_polys(p: [Poly<S>; N], w: Poly<S>) -> Option<Self> {
        let d = w.degree();
        p.iter()
            .all(|c| c.degree() == d)
            .then_some(HomCurve { p, w })
    }

    /// The coordinate polynomials `P = (w·x₁, …)`.
    #[inline]
    pub fn coords(&self) -> &[Poly<S>; N] {
        &self.p
    }

    /// The weights `W`.
    #[inline]
    pub fn weights(&self) -> &Poly<S> {
        &self.w
    }

    /// The degree.
    #[inline]
    pub fn degree(&self) -> usize {
        self.w.degree()
    }

    fn map(&self, f: impl Fn(&Poly<S>) -> Poly<S>) -> Self {
        HomCurve {
            p: std::array::from_fn(|k| f(&self.p[k])),
            w: f(&self.w),
        }
    }

    fn split(&self, f: impl Fn(&Poly<S>) -> [Poly<S>; 2]) -> [Self; 2] {
        let parts: [[Poly<S>; 2]; N] = std::array::from_fn(|k| f(&self.p[k]));
        let [w0, w1] = f(&self.w);
        let mut left: [Option<Poly<S>>; N] = std::array::from_fn(|_| None);
        let mut right: [Option<Poly<S>>; N] = std::array::from_fn(|_| None);
        for (k, [l, r]) in parts.into_iter().enumerate() {
            left[k] = Some(l);
            right[k] = Some(r);
        }
        [
            HomCurve {
                p: left.map(|c| c.expect("filled")),
                w: w0,
            },
            HomCurve {
                p: right.map(|c| c.expect("filled")),
                w: w1,
            },
        ]
    }

    /// The homogeneous point `(P(t), W(t))`, each by [`Poly::eval`].
    pub fn eval_hom(&self, t: &S) -> ([S; N], S) {
        (std::array::from_fn(|k| self.p[k].eval(t)), self.w.eval(t))
    }

    /// The point `P(t) / W(t)`.
    pub fn eval(&self, t: &S) -> [S; N] {
        let (p, w) = self.eval_hom(t);
        p.map(|x| x.div(&w))
    }

    /// The span run backwards, `t ↦ 1 − t`, exactly.
    pub fn reversed(&self) -> Self {
        self.map(Poly::reversed)
    }

    /// The same span at degree `n + by`, `None` past [`MAX_DEGREE`];
    /// commutes with reversal to the bit.
    pub fn elevate(&self, by: usize) -> Option<Self> {
        let w = self.w.elevate(by)?;
        let p = std::array::from_fn(|k| self.p[k].elevate(by).expect("as the weights"));
        Some(HomCurve { p, w })
    }

    /// The homogeneous derivative `(P', W')`, one degree lower.
    pub fn derivative(&self) -> Self {
        self.map(Poly::derivative)
    }

    /// The halves at ½ ([`Poly::split_half`]): symmetric to the bit under
    /// reversal.
    pub fn split_half(&self) -> [Self; 2] {
        self.split(Poly::split_half)
    }

    /// The pieces on `[0, t]` and `[t, 1]` ([`Poly::split_at`]).
    pub fn split_at(&self, t: &S) -> [Self; 2] {
        self.split(|p| p.split_at(t))
    }

    /// Whether every weight is certified positive: then `W > 0` on
    /// `[0, 1]` and the span lies in the convex hull of its projected
    /// control points.
    pub fn weights_positive(&self) -> bool {
        self.w.coefs().iter().all(|w| w.enclose()[0] > 0.0)
    }

    /// A certified box `[lo, hi]` per coordinate holding every point of
    /// the span on `[0, 1]`: the projected control points `(w·x) / w`
    /// bounded outwards from the enclosures of `w·x` and `w` (as
    /// [`HomPatch::hull_box`](super::HomPatch::hull_box)). `None` unless
    /// every weight is certified positive.
    pub fn control_box(&self) -> Option<[[f64; 2]; N]> {
        let mut b = [[f64::INFINITY, f64::NEG_INFINITY]; N];
        for (idx, w) in self.w.coefs().iter().enumerate() {
            let [wl, wh] = w.enclose();
            if wl <= 0.0 || wl.is_nan() {
                return None;
            }
            for (k, side) in b.iter_mut().enumerate() {
                let [lo, hi] = quotient(self.p[k].coefs()[idx].enclose(), [wl, wh]);
                if lo < side[0] {
                    side[0] = lo;
                }
                if hi > side[1] {
                    side[1] = hi;
                }
            }
        }
        Some(b)
    }
}
