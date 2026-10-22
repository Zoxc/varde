//! Tensor-product Bernstein forms in any number of variables on
//! `[0, 1]ᵈ` ([`NdTensor`]): the forms of a polynomial system in `d`
//! unknowns (a patch against a patch is four), built from forms in
//! fewer variables ([`NdTensor::embed`]) and brought to common degrees
//! by elevation.
//!
//! Every operation in one variable acts on the lines of the net along
//! it, each a [`Poly`], by the 1D operation, so the 1D guarantees carry
//! over line by line: evaluation with the end rule, reversal, the
//! derivative, splits (at ½ the same bits under reversal) and elevation
//! (commuting with reversal in every variable to the bit). Elevation in
//! several variables raises them one after another in increasing order,
//! and evaluation reduces the first variable first; so a permutation of
//! the variables may change the inner coefficients' last bits (the two
//! variables of a [`Tensor`] elevate together, symmetrically). With
//! [`malachite::Rational`] coefficients every operation is exact.

use smallvec::SmallVec;

use super::poly::{Coefs, Hull, MAX_DEGREE, Poly, derive, hull_of};
use super::scalar::Scalar;
use super::tensor::Tensor;

/// `f(x₀, …, x_(d−1)) = Σ b_I·Π_k B^(n_k)_(i_k)(x_k)` on `[0, 1]ᵈ`, of
/// degree `n_k` (at most [`MAX_DEGREE`]) in variable `k`; the
/// coefficients row-major, the last variable's index running fastest
/// (for `d = 2` the order of a [`Tensor`]'s net).
#[derive(Clone, Debug, PartialEq)]
pub struct NdTensor<S> {
    deg: SmallVec<[usize; 4]>,
    c: Vec<S>,
}

/// The number of coefficients of the degrees, `None` past `usize`.
fn count(deg: &[usize]) -> Option<usize> {
    deg.iter().try_fold(1usize, |n, &d| n.checked_mul(d + 1))
}

impl<S: Scalar> NdTensor<S> {
    /// The form of these degrees (at least one variable, each at most
    /// [`MAX_DEGREE`]) with the coefficients in row-major order, `None`
    /// for another count.
    pub fn new(degrees: &[usize], c: impl IntoIterator<Item = S>) -> Option<Self> {
        if degrees.is_empty() || degrees.iter().any(|&d| d > MAX_DEGREE) {
            return None;
        }
        let len = count(degrees)?;
        let c: Vec<S> = c.into_iter().take(len + 1).collect();
        (c.len() == len).then(|| NdTensor {
            deg: degrees.into(),
            c,
        })
    }

    /// The polynomial as a form in one variable.
    pub fn from_poly(p: &Poly<S>) -> Self {
        NdTensor {
            deg: [p.degree()].into_iter().collect(),
            c: p.coefs().to_vec(),
        }
    }

    /// The tensor as a form in two variables, `u` first.
    pub fn from_tensor(f: &Tensor<S>) -> Self {
        let (m, n) = f.degrees();
        NdTensor {
            deg: [m, n].into_iter().collect(),
            c: f.coefs().to_vec(),
        }
    }

    /// This form as one in `dims` variables, its variable `k` the
    /// result's `axes[k]` (distinct, below `dims`), of degree 0 in the
    /// others: the same function of its own variables. `None` for axes
    /// that don't fit.
    pub fn embed(&self, axes: &[usize], dims: usize) -> Option<Self> {
        if axes.len() != self.dims() || axes.iter().any(|&a| a >= dims) {
            return None;
        }
        let mut deg: SmallVec<[usize; 4]> = smallvec::smallvec![0; dims];
        for (k, &a) in axes.iter().enumerate() {
            if deg[a] != 0 || axes[..k].contains(&a) {
                return None;
            }
            deg[a] = self.deg[k];
        }
        // The result's index along `axes[k]` is this form's along `k`;
        // the others are 0.
        let strides = strides_of(&deg);
        let own = strides_of(&self.deg);
        let mut c = vec![self.c[0].clone(); self.c.len()];
        for (flat, x) in self.c.iter().enumerate() {
            let at: usize = (0..self.dims())
                .map(|k| (flat / own[k]) % (self.deg[k] + 1) * strides[axes[k]])
                .sum();
            c[at] = x.clone();
        }
        Some(NdTensor { deg, c })
    }

    /// The number of variables.
    #[inline]
    pub fn dims(&self) -> usize {
        self.deg.len()
    }

    /// The degrees, one per variable.
    #[inline]
    pub fn degrees(&self) -> &[usize] {
        &self.deg
    }

    /// The coefficients, row-major.
    #[inline]
    pub fn coefs(&self) -> &[S] {
        &self.c
    }

    /// The coefficient at the multi-index `idx`; `None` past the degrees.
    pub fn get(&self, idx: &[usize]) -> Option<&S> {
        if idx.len() != self.dims() || idx.iter().zip(&self.deg).any(|(i, d)| i > d) {
            return None;
        }
        let st = strides_of(&self.deg);
        Some(&self.c[idx.iter().zip(&st).map(|(i, s)| i * s).sum::<usize>()])
    }

    /// The lines along variable `axis`, in the order of the other
    /// variables' indices, with the number of index blocks before the axis
    /// and of coefficients after it.
    fn lines(&self, axis: usize) -> (Vec<Poly<S>>, usize, usize) {
        let len = self.deg[axis] + 1;
        let inner: usize = self.deg[axis + 1..].iter().map(|d| d + 1).product();
        let outer = self.c.len() / (len * inner);
        let mut lines = Vec::with_capacity(outer * inner);
        for o in 0..outer {
            for r in 0..inner {
                let line: Coefs<S> = (0..len)
                    .map(|i| self.c[(o * len + i) * inner + r].clone())
                    .collect();
                lines.push(Poly::from_coefs(line));
            }
        }
        (lines, outer, inner)
    }

    /// The form whose lines along `axis` are `lines` ([`lines`]'s order,
    /// one length, at most [`MAX_DEGREE`] + 1).
    ///
    /// [`lines`]: Self::lines
    fn with_lines(&self, axis: usize, lines: &[Coefs<S>], outer: usize, inner: usize) -> Self {
        let len = lines[0].len();
        debug_assert!(lines.iter().all(|l| l.len() == len) && len <= MAX_DEGREE + 1);
        let mut c = Vec::with_capacity(outer * len * inner);
        for o in 0..outer {
            for i in 0..len {
                for r in 0..inner {
                    c.push(lines[o * inner + r][i].clone());
                }
            }
        }
        let mut deg = self.deg.clone();
        deg[axis] = len - 1;
        NdTensor { deg, c }
    }

    /// Every line along `axis` through `f`, which keeps one length.
    fn map_lines(&self, axis: usize, f: impl Fn(&Poly<S>) -> Coefs<S>) -> Self {
        let (lines, outer, inner) = self.lines(axis);
        let lines: Vec<Coefs<S>> = lines.iter().map(f).collect();
        self.with_lines(axis, &lines, outer, inner)
    }

    /// Every line along `axis` split in two by `f`.
    fn split_lines(&self, axis: usize, f: impl Fn(&Poly<S>) -> [Poly<S>; 2]) -> [Self; 2] {
        let (lines, outer, inner) = self.lines(axis);
        let mut parts: [Vec<Coefs<S>>; 2] = Default::default();
        for l in &lines {
            for (k, p) in f(l).into_iter().enumerate() {
                parts[k].push(p.coefs().iter().cloned().collect());
            }
        }
        parts.map(|ls| self.with_lines(axis, &ls, outer, inner))
    }

    /// `f(x)`, `x` one parameter per variable: the lines along the first
    /// variable evaluated ([`Poly::eval`], so a parameter exactly 0 or 1
    /// takes the end line), then along the next, and so on. In two
    /// variables the same bits as [`Tensor::eval`].
    pub fn eval(&self, x: &[S]) -> Option<S> {
        if x.len() != self.dims() {
            return None;
        }
        let mut f = self.clone();
        for (axis, t) in x.iter().enumerate() {
            f = f.map_lines(axis, |p| smallvec::smallvec![p.eval(t)]);
        }
        Some(f.c.swap_remove(0))
    }

    /// The form reversed in `axis` (`xₐ ↦ 1 − xₐ`), exactly.
    pub fn reversed(&self, axis: usize) -> Self {
        self.map_lines(axis, |p| p.coefs().iter().rev().cloned().collect())
    }

    /// The partial derivative in `axis`, one degree lower in it (degree 0
    /// gives the zero form).
    pub fn derivative(&self, axis: usize) -> Self {
        self.map_lines(axis, |p| derive(p.coefs()))
    }

    /// The halves at ½ in `axis` ([`Poly::split_half`] on every line),
    /// the same bits under reversal.
    pub fn split_half(&self, axis: usize) -> [Self; 2] {
        self.split_lines(axis, Poly::split_half)
    }

    /// The pieces on `[0, t]` and `[t, 1]` in `axis` ([`Poly::split_at`]).
    pub fn split_at(&self, axis: usize, t: &S) -> [Self; 2] {
        self.split_lines(axis, |p| p.split_at(t))
    }

    /// The same form at degree `deg + by` in `axis` ([`Poly::elevate`] on
    /// every line), `None` past [`MAX_DEGREE`] or for an axis it hasn't.
    /// Commutes with reversal in every variable to the bit.
    pub fn elevate(&self, axis: usize, by: usize) -> Option<Self> {
        if self.deg.get(axis)?.checked_add(by)? > MAX_DEGREE {
            return None;
        }
        if by == 0 {
            return Some(self.clone());
        }
        Some(self.map_lines(axis, |p| {
            p.elevate(by)
                .expect("checked above")
                .coefs()
                .iter()
                .cloned()
                .collect()
        }))
    }

    /// The same form at `degrees`, raised variable by variable in
    /// increasing order; `None` for another number of variables, below
    /// its own degrees or past [`MAX_DEGREE`].
    pub fn elevate_to(&self, degrees: &[usize]) -> Option<Self> {
        if degrees.len() != self.dims() {
            return None;
        }
        let mut f = self.clone();
        for (axis, &d) in degrees.iter().enumerate() {
            f = f.elevate(axis, d.checked_sub(self.deg[axis])?)?;
        }
        Some(f)
    }

    /// `f + g` at the higher degree in each variable (each elevated to
    /// it); `None` for different numbers of variables.
    pub fn add(&self, o: &Self) -> Option<Self> {
        self.combine(o, S::add)
    }

    /// `f − g` at the higher degree in each variable; `None` for
    /// different numbers of variables.
    pub fn sub(&self, o: &Self) -> Option<Self> {
        self.combine(o, S::sub)
    }

    fn combine(&self, o: &Self, op: fn(&S, &S) -> S) -> Option<Self> {
        if self.dims() != o.dims() {
            return None;
        }
        let deg: SmallVec<[usize; 4]> = self
            .deg
            .iter()
            .zip(&o.deg)
            .map(|(&a, &b)| a.max(b))
            .collect();
        let (a, b) = (self.elevate_to(&deg)?, o.elevate_to(&deg)?);
        let c = a.c.iter().zip(&b.c).map(|(x, y)| op(x, y)).collect();
        Some(NdTensor { deg, c })
    }

    /// `k·f`, coefficient by coefficient.
    pub fn scale(&self, k: &S) -> Self {
        NdTensor {
            deg: self.deg.clone(),
            c: self.c.iter().map(|b| k.mul(b)).collect(),
        }
    }

    /// `−f`, exactly.
    pub fn neg(&self) -> Self {
        NdTensor {
            deg: self.deg.clone(),
            c: self.c.iter().map(Scalar::neg).collect(),
        }
    }

    /// The coefficient hull: a certified enclosure of `f` on `[0, 1]ᵈ`.
    pub fn hull(&self) -> Hull {
        hull_of(&self.c)
    }
}

/// The strides of row-major coefficients of the degrees.
fn strides_of(deg: &[usize]) -> SmallVec<[usize; 4]> {
    let mut s: SmallVec<[usize; 4]> = smallvec::smallvec![1; deg.len()];
    for k in (0..deg.len().saturating_sub(1)).rev() {
        s[k] = s[k + 1] * (deg[k + 1] + 1);
    }
    s
}

#[cfg(test)]
mod tests;
