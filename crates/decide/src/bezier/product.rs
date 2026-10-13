//! Products, sums and degree elevation of Bernstein forms: [`Poly`] by
//! [`Poly`], [`Tensor`] by [`Tensor`], the cross and dot products of
//! 3-vectors of forms ([`Form`]), and the numerators of a rational
//! curve's tangent and a rational patch's normal.
//!
//! **Weights.** The product of degrees `m` and `n` has the coefficients
//! `cₖ = Σᵢ C(m, i)·C(n, k − i)·aᵢ·bₖ₋ᵢ / C(m + n, k)`. The binomials are
//! exact integers (`u64`: every one up to [`MAX_DEGREE`] is below `2⁶¹`),
//! taken into the scalar by [`Scalar::int_u64`], so exactly where the
//! level holds them and certified where it doesn't (the first level
//! above `2⁵³`, from degree 57 on). A weight of 1 is no operation: the
//! end coefficients of a product are the products of the ends, one
//! rounding.
//!
//! **Same bits under reversal and swap.** Each coefficient's terms are
//! summed pairwise from both ends of their list (first with last, second
//! with second-last, the middle carried, repeated), so the reversed list
//! gives the same bits. The product of the reversed operands lists the
//! terms of `cₙ₊ₘ₋ₖ` in reverse, as `b × a` does those of `cₖ`: so
//! `a.reversed() × b.reversed()` is `(a × b).reversed()` and `b × a` is
//! `a × b`, to the bit (the levels' `+` and `×` commute to the bit). A
//! tensor product multiplies the lines in `v` (1D products) and combines
//! them in `u` by the same rule: it commutes with reversal in either
//! direction and with the swap of its operands, and its boundary lines
//! are the 1D products of the operands' boundary lines, to the bit. With
//! `u` and `v` transposed the order of the two stages changes, so the
//! inner coefficients may differ in their last bits.

use smallvec::SmallVec;

use super::curve::HomCurve;
use super::poly::{Coefs, MAX_DEGREE, Poly};
use super::scalar::Scalar;
use super::tensor::{Dir, HomPatch, Tensor};

/// `C(n, k)` for `k ≤ n ≤ MAX_DEGREE`, exactly: `C(64, 32) < 2⁶¹`.
fn binom(n: usize, k: usize) -> u64 {
    debug_assert!(k <= n && n <= MAX_DEGREE);
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k {
        // Exact at every step: `r·(n − i)` is `(i + 1)·C(n, i + 1)`, below
        // `2⁶⁷`.
        r = r * (n - i) as u128 / (i + 1) as u128;
    }
    u64::try_from(r).expect("C(n, k) < 2⁶¹ for n ≤ 64")
}

/// `w·x`, with no operation for `w = 1`.
fn weigh<S: Scalar>(w: u64, x: S) -> S {
    if w == 1 { x } else { S::int_u64(w, &x).mul(&x) }
}

/// `x / d`, `d` at `x`'s level (and width), with no operation for
/// `d = 1`.
fn divide<S: Scalar>(x: S, d: u64) -> S {
    if d == 1 { x } else { x.div(&S::int_u64(d, &x)) }
}

/// The terms summed in an order the reversed list gives too: the first
/// with the last, the second with the second-last, the middle one
/// carried, repeated until one is left (non-empty).
fn sym_sum<S: Scalar>(mut t: SmallVec<[S; 8]>) -> S {
    while t.len() > 1 {
        let n = t.len();
        let mut next: SmallVec<[S; 8]> = (0..n / 2).map(|i| t[i].add(&t[n - 1 - i])).collect();
        if n % 2 == 1 {
            next.push(t[n / 2].clone());
        }
        t = next;
    }
    t.swap_remove(0)
}

/// Coefficient `k` of a product of degrees `m` and `n` (`m + n ≤
/// MAX_DEGREE`) whose term `(i, k − i)` is `x(i, k − i)`, the product of
/// the operands' coefficients `i` and `k − i`: `Σᵢ C(m, i)·C(n, k − i)·
/// x(i, k − i) / C(m + n, k)`, `i` over `max(0, k − n) ..= min(k, m)`,
/// summed by [`sym_sum`].
fn product_coef<S: Scalar>(m: usize, n: usize, k: usize, x: impl Fn(usize, usize) -> S) -> S {
    let t = (k.saturating_sub(n)..=k.min(m))
        .map(|i| weigh(binom(m, i) * binom(n, k - i), x(i, k - i)))
        .collect();
    divide(sym_sum(t), binom(m + n, k))
}

/// The product's coefficients from those of `a` (degree `m`) and `b`
/// (degree `n`), `m + n ≤ MAX_DEGREE`.
fn mul_coefs<S: Scalar>(a: &[S], b: &[S]) -> Coefs<S> {
    let (m, n) = (a.len() - 1, b.len() - 1);
    (0..=m + n)
        .map(|k| product_coef(m, n, k, |i, j| a[i].mul(&b[j])))
        .collect()
}

/// The coefficients of `a + b` or `a − b` (`sub`), both raised to the
/// higher degree first.
fn add_coefs<S: Scalar>(a: &Poly<S>, b: &Poly<S>, sub: bool) -> Poly<S> {
    let d = a.degree().max(b.degree());
    let a = a.elevate(d - a.degree()).expect("at most MAX_DEGREE");
    let b = b.elevate(d - b.degree()).expect("at most MAX_DEGREE");
    let c = a
        .coefs()
        .iter()
        .zip(b.coefs())
        .map(|(x, y)| if sub { x.sub(y) } else { x.add(y) })
        .collect();
    Poly::from_coefs(c)
}

impl<S: Scalar> Poly<S> {
    /// The product `p·q`, of degree `m + n`, `None` past [`MAX_DEGREE`].
    /// Commutes with reversal and with the swap of the operands, to the
    /// bit (module docs).
    pub fn mul(&self, o: &Poly<S>) -> Option<Poly<S>> {
        if self.degree().checked_add(o.degree())? > MAX_DEGREE {
            return None;
        }
        Some(Poly::from_coefs(mul_coefs(self.coefs(), o.coefs())))
    }

    /// `p + q`, at the higher degree (the other elevated to it).
    pub fn add(&self, o: &Poly<S>) -> Poly<S> {
        add_coefs(self, o, false)
    }

    /// `p − q`, at the higher degree (the other elevated to it).
    pub fn sub(&self, o: &Poly<S>) -> Poly<S> {
        add_coefs(self, o, true)
    }

    /// `k·p`, coefficient by coefficient.
    pub fn scale(&self, k: &S) -> Poly<S> {
        Poly::from_coefs(self.coefs().iter().map(|b| k.mul(b)).collect())
    }

    /// `−p`, exactly.
    pub fn neg(&self) -> Poly<S> {
        Poly::from_coefs(self.coefs().iter().map(Scalar::neg).collect())
    }
}

impl<S: Scalar> Tensor<S> {
    /// The product `f·g`, of degrees `(m₁ + m₂, n₁ + n₂)`, `None` past
    /// [`MAX_DEGREE`] in either. The rows (lines in `v`) multiplied by
    /// [`Poly::mul`], then combined in `u` with the binomial weights of
    /// `u`: commutes with reversal in either direction and with the swap
    /// of the operands, its boundary lines the 1D products of the
    /// operands', to the bit (module docs).
    pub fn mul(&self, o: &Tensor<S>) -> Option<Tensor<S>> {
        let ((m1, n1), (m2, n2)) = (self.degrees(), o.degrees());
        let (m, n) = (m1.checked_add(m2)?, n1.checked_add(n2)?);
        if m > MAX_DEGREE || n > MAX_DEGREE {
            return None;
        }
        fn row<S: Scalar>(f: &Tensor<S>, i: usize) -> &[S] {
            let w = f.degree(Dir::V) + 1;
            &f.coefs()[i * w..(i + 1) * w]
        }
        // `rows[i₁·(m₂ + 1) + i₂]`: row `i₁` of `f` times row `i₂` of `g`.
        let rows: SmallVec<[Coefs<S>; 9]> = (0..=m1)
            .flat_map(|i1| (0..=m2).map(move |i2| (i1, i2)))
            .map(|(i1, i2)| mul_coefs(row(self, i1), row(o, i2)))
            .collect();
        let mut c: SmallVec<[S; 9]> = SmallVec::with_capacity((m + 1) * (n + 1));
        for i in 0..=m {
            for j in 0..=n {
                c.push(product_coef(m1, m2, i, |i1, i2| {
                    rows[i1 * (m2 + 1) + i2][j].clone()
                }));
            }
        }
        Tensor::new(m, n, c)
    }

    /// The same form at degree `deg + by` in `dir` ([`Poly::elevate`] on
    /// every line in `dir`), `None` past [`MAX_DEGREE`]. Commutes with
    /// reversal in either direction to the bit.
    pub fn elevate(&self, dir: Dir, by: usize) -> Option<Tensor<S>> {
        if self.degree(dir).checked_add(by)? > MAX_DEGREE {
            return None;
        }
        if by == 0 {
            return Some(self.clone());
        }
        Some(self.map_lines(dir, |p| {
            p.elevate(by)
                .expect("checked above")
                .coefs()
                .iter()
                .cloned()
                .collect()
        }))
    }

    /// The same form at degrees `(m, n)`, elevated in `u` then `v`;
    /// `None` below its own degrees or past [`MAX_DEGREE`].
    pub fn elevate_to(&self, m: usize, n: usize) -> Option<Tensor<S>> {
        let (m0, n0) = self.degrees();
        self.elevate(Dir::U, m.checked_sub(m0)?)?
            .elevate(Dir::V, n.checked_sub(n0)?)
    }

    /// `f + g`, at the higher degree in each direction (each elevated to
    /// it).
    pub fn add(&self, o: &Tensor<S>) -> Tensor<S> {
        self.combine(o, S::add)
    }

    /// `f − g`, at the higher degree in each direction (each elevated to
    /// it).
    pub fn sub(&self, o: &Tensor<S>) -> Tensor<S> {
        self.combine(o, S::sub)
    }

    /// Both at the higher degrees, then `op` coefficient by coefficient.
    fn combine(&self, o: &Tensor<S>, op: fn(&S, &S) -> S) -> Tensor<S> {
        let ((m1, n1), (m2, n2)) = (self.degrees(), o.degrees());
        let (m, n) = (m1.max(m2), n1.max(n2));
        let a = self.elevate_to(m, n).expect("at most MAX_DEGREE");
        let b = o.elevate_to(m, n).expect("at most MAX_DEGREE");
        let c = a.coefs().iter().zip(b.coefs()).map(|(x, y)| op(x, y));
        Tensor::new(m, n, c).expect("same degrees")
    }

    /// `k·f`, coefficient by coefficient.
    pub fn scale(&self, k: &S) -> Tensor<S> {
        let (m, n) = self.degrees();
        Tensor::new(m, n, self.coefs().iter().map(|b| k.mul(b))).expect("same degrees")
    }

    /// `−f`, exactly.
    pub fn neg(&self) -> Tensor<S> {
        let (m, n) = self.degrees();
        Tensor::new(m, n, self.coefs().iter().map(Scalar::neg)).expect("same degrees")
    }
}

/// A Bernstein form that multiplies and adds: [`Poly`] and [`Tensor`],
/// for the vector products [`cross`] and [`dot`].
pub trait Form: Sized {
    /// The product, `None` past [`MAX_DEGREE`].
    fn mul(&self, o: &Self) -> Option<Self>;

    /// The sum, at the higher degrees.
    fn add(&self, o: &Self) -> Self;

    /// The difference, at the higher degrees.
    fn sub(&self, o: &Self) -> Self;
}

impl<S: Scalar> Form for Poly<S> {
    fn mul(&self, o: &Self) -> Option<Self> {
        Poly::mul(self, o)
    }

    fn add(&self, o: &Self) -> Self {
        Poly::add(self, o)
    }

    fn sub(&self, o: &Self) -> Self {
        Poly::sub(self, o)
    }
}

impl<S: Scalar> Form for Tensor<S> {
    fn mul(&self, o: &Self) -> Option<Self> {
        Tensor::mul(self, o)
    }

    fn add(&self, o: &Self) -> Self {
        Tensor::add(self, o)
    }

    fn sub(&self, o: &Self) -> Self {
        Tensor::sub(self, o)
    }
}

/// `a × b` of 3-vectors of forms, `(a₁b₂ − a₂b₁, a₂b₀ − a₀b₂,
/// a₀b₁ − a₁b₀)`; `None` past [`MAX_DEGREE`].
pub fn cross<F: Form>(a: &[F; 3], b: &[F; 3]) -> Option<[F; 3]> {
    let c = |i: usize, j: usize| Some(a[i].mul(&b[j])?.sub(&a[j].mul(&b[i])?));
    Some([c(1, 2)?, c(2, 0)?, c(0, 1)?])
}

/// `a · b` of 3-vectors of forms, `(a₀b₀ + a₁b₁) + a₂b₂`; `None` past
/// [`MAX_DEGREE`].
pub fn dot<F: Form>(a: &[F; 3], b: &[F; 3]) -> Option<F> {
    Some(
        a[0].mul(&b[0])?
            .add(&a[1].mul(&b[1])?)
            .add(&a[2].mul(&b[2])?),
    )
}

impl<S: Scalar> HomPatch<S> {
    /// The normal numerator `N = W·(P_u × P_v) + W_u·(P_v × P) +
    /// W_v·(P × P_u)` from the net `(P, W) = (X, Y, Z, W)`: the spatial
    /// part of `H ∧ H_u ∧ H_v`, equal to `W³·(x_u × x_v)` for the point
    /// `x = P / W`, so with `W > 0` a normal of the patch's orientation
    /// wherever it isn't zero. Of degrees `(3m − 1, 3n − 1)` for degrees
    /// `(m, n) ≥ (1, 1)`, `(5, 5)` for a biquadratic patch (whose exact
    /// numerator is of degree `(4, 4)`, not reduced here); `None` past
    /// [`MAX_DEGREE`]. The patch reversed in either direction gives the
    /// reversed numerator negated, to the bit but the signs of zeros.
    pub fn normal_numerator(&self) -> Option<[Tensor<S>; 3]> {
        let [x, y, z, w] = self.tensors().clone();
        let [xu, yu, zu, wu] = self.derivative(Dir::U).tensors().clone();
        let [xv, yv, zv, wv] = self.derivative(Dir::V).tensors().clone();
        let p = [x, y, z];
        let pu = [xu, yu, zu];
        let pv = [xv, yv, zv];
        let term = |k: &Tensor<S>, a: &[Tensor<S>; 3], b: &[Tensor<S>; 3]| {
            let c = cross(a, b)?;
            Some([k.mul(&c[0])?, k.mul(&c[1])?, k.mul(&c[2])?])
        };
        let t0 = term(&w, &pu, &pv)?;
        let t1 = term(&wu, &pv, &p)?;
        let t2 = term(&wv, &p, &pu)?;
        Some([0, 1, 2].map(|k| t0[k].add(&t1[k]).add(&t2[k])))
    }
}

impl<S: Scalar, const N: usize> HomCurve<S, N> {
    /// The tangent numerator `W·P' − W'·P`, equal to `W²·x'` for the
    /// point `x = P / W`: with `W > 0` the tangent's direction wherever it
    /// isn't zero. Of degree `2n − 1` for degree `n ≥ 1` (whose exact
    /// numerator is of degree `2n − 2`, not reduced here); `None` past
    /// [`MAX_DEGREE`]. The reversed span gives the reversed numerator
    /// negated, to the bit but the signs of zeros.
    pub fn tangent_numerator(&self) -> Option<[Poly<S>; N]> {
        let d = self.derivative();
        let (w, dw) = (self.weights(), d.weights());
        let mut out: [Option<Poly<S>>; N] = std::array::from_fn(|_| None);
        for (k, o) in out.iter_mut().enumerate() {
            *o = Some(w.mul(&d.coords()[k])?.sub(&dw.mul(&self.coords()[k])?));
        }
        Some(out.map(|p| p.expect("filled")))
    }
}

#[cfg(test)]
mod tests;
