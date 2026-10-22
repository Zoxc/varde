//! Products, sums and degree elevation of Bernstein forms: [`Poly`] by
//! [`Poly`], [`Tensor`] by [`Tensor`], the cross and dot products of
//! 3-vectors of forms ([`Form`]), and the numerator of a rational
//! patch's normal (the tangent and partial numerators are in
//! [`deriv`](super::deriv)).
//!
//! **Weights.** The product of degrees `m` and `n` has the coefficients
//! `cₖ = Σᵢ C(m, i)·C(n, k − i)·aᵢ·bₖ₋ᵢ / C(m + n, k)`. The binomials are
//! exact integers (`u64`: every one up to [`MAX_DEGREE`] is below `2⁶¹`),
//! taken into the scalar by [`Scalar::int_u64`], so exactly where the
//! level holds them and certified where it doesn't (the first level
//! above `2⁵³`, from degree 57 on). A weight of 1 is no operation: the
//! end coefficients of a product are the products of the ends, one
//! rounding. A tensor product's term has the product of its two
//! directions' weights, one `u64` where it fits (a weight of 1 in one
//! direction leaves the other's), else formed at the level.
//!
//! **Same bits under reversal and swap.** Each coefficient's terms are
//! summed pairwise from both ends of their list (first with last, second
//! with second-last, the middle carried, repeated), so the reversed list
//! gives the same bits. The product of the reversed operands lists the
//! terms of `cₙ₊ₘ₋ₖ` in reverse, as `b × a` does those of `cₖ`: so
//! `a.reversed() × b.reversed()` is `(a × b).reversed()` and `b × a` is
//! `a × b`, to the bit (the levels' `+` and `×` commute to the bit). A
//! tensor product's coefficient sums the grid of its terms, `(i₁, j₁)`
//! over the rows and columns that meet, by the same rule in both
//! directions at once: each term with its three mirror images, the pairs
//! of opposite corners first, then the quarter grid again
//! (`sym_sum2`). Reversal in either direction, transposition and the
//! swap of the operands only mirror the grid, so they give the same
//! bits, and a boundary line, a grid of one row or column, is the 1D
//! product of the operands' boundary lines.
//!
//! **Elevation** of a tensor in both directions raises both degrees at
//! once, one step at a time, an inner coefficient from its four
//! neighbours paired the same way, and the boundary lines by the 1D
//! step: the same bits under reversal and transposition, the boundary
//! lines those of the 1D elevation.

use smallvec::SmallVec;

use super::poly::{Coefs, MAX_DEGREE, Poly, binom, divide, elevate_once, weigh};
use super::scalar::Scalar;
use super::tensor::{Dir, HomPatch, Tensor};

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

/// `wu·wv·x`, the two weights of a tensor product's term (each below
/// `2⁶¹`): one weight where their product is a `u64` (so a weight of 1
/// in either direction is the 1D weight), else their product at the
/// level, which commutes, then times `x`.
fn weigh2<S: Scalar>(wu: u64, wv: u64, x: S) -> S {
    match u64::try_from(u128::from(wu) * u128::from(wv)) {
        Ok(w) => weigh(w, x),
        Err(_) => S::int_u64(wu, &x).mul(&S::int_u64(wv, &x)).mul(&x),
    }
}

/// `x / (du·dv)`, as [`weigh2`] forms the divisor.
fn divide2<S: Scalar>(x: S, du: u64, dv: u64) -> S {
    match u64::try_from(u128::from(du) * u128::from(dv)) {
        Ok(d) => divide(x, d),
        Err(_) => {
            let d = S::int_u64(du, &x).mul(&S::int_u64(dv, &x));
            x.div(&d)
        }
    }
}

/// The terms of a `p × q` grid (row-major, non-empty) summed in an order
/// the grid reversed in either direction, or transposed, gives too: each
/// cell of the upper-left quarter (the middle row and column included)
/// takes the cells it mirrors to, `(t(i, j) + t(ī, j̄)) + (t(ī, j) +
/// t(i, j̄))` with `ī = p − 1 − i`, `j̄ = q − 1 − j` (on the middle row or
/// column the two distinct cells, at the centre the cell), and the
/// quarter is folded again until one cell is left. On a single row or
/// column this is [`sym_sum`]: a line of the grid sums as the 1D product
/// does.
fn sym_sum2<S: Scalar>(mut t: SmallVec<[S; 9]>, mut p: usize, mut q: usize) -> S {
    debug_assert!(p * q == t.len() && !t.is_empty());
    while p * q > 1 {
        let (p2, q2) = (p.div_ceil(2), q.div_ceil(2));
        let at = |i: usize, j: usize| &t[i * q + j];
        let mut next: SmallVec<[S; 9]> = SmallVec::with_capacity(p2 * q2);
        for i in 0..p2 {
            let ib = p - 1 - i;
            for j in 0..q2 {
                let jb = q - 1 - j;
                next.push(match (i < ib, j < jb) {
                    (true, true) => at(i, j).add(at(ib, jb)).add(&at(ib, j).add(at(i, jb))),
                    (false, true) => at(i, j).add(at(i, jb)),
                    (true, false) => at(i, j).add(at(ib, j)),
                    (false, false) => at(i, j).clone(),
                });
            }
        }
        (t, p, q) = (next, p2, q2);
    }
    t.swap_remove(0)
}

/// One degree up in both directions at once (degrees `(m, n)` below
/// [`MAX_DEGREE`]). The boundary lines are the 1D elevations of the
/// form's (`elevate_once`), so a line shared with a neighbour stays the
/// same bits; an inner coefficient is
/// `((i·j)·b₍ᵢ₋₁,ⱼ₋₁₎ + (m + 1 − i)(n + 1 − j)·bᵢⱼ) + (i(n + 1 − j)·b₍ᵢ₋₁,ⱼ₎
/// + (m + 1 − i)·j·b₍ᵢ,ⱼ₋₁₎)` over `(m + 1)(n + 1)`, its terms paired
/// with their opposites as in [`sym_sum2`], so reversal in either
/// direction and transposition give the same bits. Where that numerator
/// isn't bounded the four coefficients are scaled by `2⁻¹³` (below
/// `1/(m + 1)(n + 1)`) and the quotient back, as in 1D.
fn elevate_both<S: Scalar>(f: &Tensor<S>) -> Tensor<S> {
    let (m, n) = f.degrees();
    let line = |dir: Dir, k: usize| -> Coefs<S> {
        let c: Coefs<S> = match dir {
            Dir::V => (0..=n).map(|j| f.get(k, j).clone()).collect(),
            Dir::U => (0..=m).map(|i| f.get(i, k).clone()).collect(),
        };
        elevate_once(&c)
    };
    let (first_row, last_row) = (line(Dir::V, 0), line(Dir::V, m));
    let (first_col, last_col) = (line(Dir::U, 0), line(Dir::U, n));
    let (m1, n1) = (m as u32 + 1, n as u32 + 1);
    let unbounded = |x: &S| x.enclose().iter().any(|e| !e.is_finite());
    let mut c: SmallVec<[S; 9]> = SmallVec::with_capacity((m + 2) * (n + 2));
    for i in 0..=m + 1 {
        for j in 0..=n + 1 {
            c.push(if i == 0 {
                first_row[j].clone()
            } else if i == m + 1 {
                last_row[j].clone()
            } else if j == 0 {
                first_col[i].clone()
            } else if j == n + 1 {
                last_col[i].clone()
            } else {
                let (iu, ju) = (i as u32, j as u32);
                let terms = [
                    (iu * ju, f.get(i - 1, j - 1)),
                    ((m1 - iu) * (n1 - ju), f.get(i, j)),
                    (iu * (n1 - ju), f.get(i - 1, j)),
                    ((m1 - iu) * ju, f.get(i, j - 1)),
                ];
                let num = |scaled: bool| {
                    let t = terms.map(|(w, x)| {
                        let x = if scaled {
                            x.mul(&S::from_f64(ELEVATE_DOWN2, x))
                        } else {
                            x.clone()
                        };
                        S::int(w, &x).mul(&x)
                    });
                    t[0].add(&t[1]).add(&t[2].add(&t[3]))
                };
                let q = num(false);
                if unbounded(&q) {
                    let q = num(true);
                    let q = q.div(&S::int(m1 * n1, &q));
                    q.mul(&S::from_f64(ELEVATE_UP2, &q))
                } else {
                    q.div(&S::int(m1 * n1, &q))
                }
            });
        }
    }
    Tensor::new(m + 1, n + 1, c).expect("one degree up, checked by the caller")
}

/// `2⁻¹³`, below `1/(MAX_DEGREE + 1)²`: the numerator of an inner
/// coefficient of [`elevate_both`] of coefficients scaled by it stays
/// below the top of the range.
const ELEVATE_DOWN2: f64 = 1.0 / 8192.0;

/// `2¹³`, scaling such a coefficient back.
const ELEVATE_UP2: f64 = 8192.0;

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
    /// [`MAX_DEGREE`] in either. Each coefficient is the sum over the
    /// grid of its terms, weighted by the binomials of both directions and
    /// folded from the four corners at once (`sym_sum2`): commutes with
    /// reversal in either direction, with the swap of the operands and
    /// with transposition, its boundary lines the 1D products of the
    /// operands', to the bit (module docs).
    pub fn mul(&self, o: &Tensor<S>) -> Option<Tensor<S>> {
        let ((m1, n1), (m2, n2)) = (self.degrees(), o.degrees());
        let (m, n) = (m1.checked_add(m2)?, n1.checked_add(n2)?);
        if m > MAX_DEGREE || n > MAX_DEGREE {
            return None;
        }
        let mut c: SmallVec<[S; 9]> = SmallVec::with_capacity((m + 1) * (n + 1));
        for i in 0..=m {
            let rows = i.saturating_sub(m2)..=i.min(m1);
            for j in 0..=n {
                let cols = j.saturating_sub(n2)..=j.min(n1);
                let q = cols.clone().count();
                let mut t: SmallVec<[S; 9]> = SmallVec::new();
                for i1 in rows.clone() {
                    let wu = binom(m1, i1) * binom(m2, i - i1);
                    for j1 in cols.clone() {
                        let wv = binom(n1, j1) * binom(n2, j - j1);
                        let x = self.get(i1, j1).mul(o.get(i - i1, j - j1));
                        t.push(weigh2(wu, wv, x));
                    }
                }
                let p = t.len() / q;
                c.push(divide2(sym_sum2(t, p, q), binom(m, i), binom(n, j)));
            }
        }
        Tensor::new(m, n, c)
    }

    /// The same form at degree `deg + by` in `dir` ([`Poly::elevate`] on
    /// every line in `dir`), `None` past [`MAX_DEGREE`]. Commutes with
    /// reversal in either direction to the bit, and with transposition
    /// (the other direction's elevation of the transposed form).
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

    /// The same form at degrees `(m, n)`, `None` below its own degrees or
    /// past [`MAX_DEGREE`]: one degree in both directions at once while
    /// both rise (`elevate_both`), then the rest in the one direction.
    /// Commutes with reversal in either direction and with transposition,
    /// its boundary lines the 1D elevations of the form's, to the bit.
    pub fn elevate_to(&self, m: usize, n: usize) -> Option<Tensor<S>> {
        let (m0, n0) = self.degrees();
        let (a, b) = (m.checked_sub(m0)?, n.checked_sub(n0)?);
        if m > MAX_DEGREE || n > MAX_DEGREE {
            return None;
        }
        let mut f = self.clone();
        for _ in 0..a.min(b) {
            f = elevate_both(&f);
        }
        f.elevate(Dir::U, a - a.min(b))?
            .elevate(Dir::V, b - a.min(b))
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

#[cfg(test)]
mod tests;
