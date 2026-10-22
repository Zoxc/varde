//! Compositions: a rational span or patch put into a line or plane
//! equation, the linear image of a span (such as its shadow on a plane),
//! a patch along a curve in its parameter domain (a p-curve span: the
//! surface's point along it, and the seam numerator against a space
//! curve), and a patch or a span put into an implicit equation of any
//! degree. Each result is a polynomial (a tensor for a patch), its
//! degrees computed from the operands', exact on rationals and certified
//! at every level.
//!
//! **Linear forms.** In homogeneous coordinates the equation
//! `a·x + c = 0` at the point `x = P / W` is `a·P + c·W = 0`: a
//! combination of the coordinate polynomials, of the span's own degree,
//! coefficient by coefficient. Each coefficient is the sum
//! `a₁·P₁ + a₂·P₂ + … + c·W` in that order, a factor exactly 0 left out,
//! exactly 1 or −1 a copy or negation (no rounding); so the composition
//! of the reversed span is the reversed composition, to the bit, and an
//! image keeps the weights' bits.
//!
//! **A patch along a curve.** With the curve's parameters rational,
//! `u = U/D_u` of degree `k_u` and `v = V/D_v` of degree `k_v`, the
//! Bernstein basis of degree `m` at `u` times `D_u^m` is the polynomial
//! `C(m, a)·(D_u − U)^(m−a)·U^a`, of degree `m·k_u`; so the patch of
//! degrees `(m, n)` along the curve, its homogeneous coordinates times
//! `D_u^m·D_v^n`, is `Σₐᵦ Hₐᵦ·Bₐ(U, D_u)·Bᵦ(V, D_v)`, of degree
//! `m·k_u + n·k_v`; the point `(H₀, H₁, H₂) / H₃` is the surface's
//! along the curve, the common factor cancelling. A p-curve span shares
//! its weight, `D_u = D_v = W`: a quadratic span on a biquadratic tile
//! gives degree 8, a straight UV segment 4, an iso line 2
//! ([`HomPatch::compose_span`]). With a space curve `C = (X, W_C)` of
//! degree `c` the **seam numerator** `N = H·W_C − X·H₃`, of degree
//! `m·k_u + n·k_v + c`, gives the deviation `S(p(t)) − C(t) =
//! N / (H₃·W_C)` ([`HomCurve::seam_numerator`]). Powers are formed by
//! repeated products, a constant factor (degree 0) as a scaling; each
//! term is weighted by its patch coefficient under the rule of the linear
//! forms (0 left out, ±1 no rounding), and the grid of terms is summed
//! from its four corners at once as a tensor product's coefficients are
//! (`sym_fold2`). So the composition along the reversed curve is the
//! reversed composition, and the transposed patch along the curve with
//! its parameters swapped is the same composition, to the bit.
//!
//! **Implicits** ([`Implicit`]): a polynomial `F` of degree `d` in the
//! point relative to the implicit's own origin `o` (its exact frame), in
//! homogeneous form `F̂(X − o·W, W) = W^d·F(X/W)`: each term
//! `c·Π (Xₖ − oₖ·W)^eₖ·W^e₃` with `Σ eₖ = d`, the shifted coordinates
//! by the rule of the linear forms (an origin coordinate 0 a copy), the
//! powers by repeated products and the terms summed in their stored
//! order. A patch of degrees `(m, n)` gives a tensor of degrees
//! `(d·m, d·n)`, (8, 8) for a torus's quartic on a biquadratic tile; a
//! span of degree `c` a polynomial of degree `d·c`. Every term is formed
//! of the same products in either direction, so the composition commutes
//! with reversal and transposition to the bit. The sign of the result at
//! a parameter is `F`'s at the point where `W > 0` (times `sign(W)^d`).

use smallvec::SmallVec;

use super::curve::HomCurve;
use super::poly::{Coefs, MAX_DEGREE, Poly, binom};
use super::product::sym_fold2;
use super::scalar::Scalar;
use super::tensor::{HomPatch, Tensor};

/// `Σ fₖ·xₖ` in order, a factor exactly 0 left out, ±1 no rounding;
/// zero (at the level of the last `x`) when every factor is 0.
fn combine<'a, S: Scalar + 'a>(terms: impl IntoIterator<Item = (&'a S, &'a S)>) -> S {
    let mut acc: Option<S> = None;
    let mut last = None;
    for (f, x) in terms {
        last = Some(x);
        let t = if f.is_int(0) {
            continue;
        } else if f.is_int(1) {
            x.clone()
        } else if f.neg().is_int(1) {
            x.neg()
        } else {
            f.mul(x)
        };
        acc = Some(match acc {
            None => t,
            Some(a) => a.add(&t),
        });
    }
    acc.unwrap_or_else(|| S::int(0, last.expect("at least one term")))
}

impl<S: Scalar, const N: usize> HomCurve<S, N> {
    /// The span in the equation `a·x + c`: the polynomial
    /// `a·P + c·W`, of the span's degree, whose sign at `t` is the side
    /// of `x(t)` where `W(t) > 0` (a plane `n·x = d` for `N = 3` with
    /// `a = n`, `c = −d`; a line in the plane for `N = 2`). Commutes with
    /// reversal to the bit; exact on rationals.
    pub fn compose_linear(&self, a: &[S; N], c: &S) -> Poly<S> {
        let n = self.degree();
        let coefs: Coefs<S> = (0..=n)
            .map(|i| {
                let xs = self.coords().iter().map(|p| &p.coefs()[i]);
                combine(
                    a.iter()
                        .zip(xs)
                        .chain(std::iter::once((c, &self.weights().coefs()[i]))),
                )
            })
            .collect();
        Poly::from_coefs(coefs)
    }

    /// The image under the linear map `x ↦ A·x` (`rows` the rows of `A`):
    /// the span `(A·P, W)`, each row by [`compose_linear`] with `c = 0`,
    /// the weights the same bits.
    ///
    /// [`compose_linear`]: Self::compose_linear
    pub fn map_linear<const M: usize>(&self, rows: &[[S; N]; M]) -> HomCurve<S, M> {
        let zero = S::int(0, &self.weights().coefs()[0]);
        let p = std::array::from_fn(|r| self.compose_linear(&rows[r], &zero));
        HomCurve::from_polys(p, self.weights().clone()).expect("one degree")
    }
}

impl<S: Scalar> HomCurve<S, 3> {
    /// The shadow on a plane with the in-plane axes `ex` and `ey` (any
    /// vectors; perpendicular to the projection direction for a shadow
    /// along it): the plane span `(ex·P, ey·P, W)`, its point
    /// `(ex·x, ey·x)`. Exact on rationals; the weights the same bits.
    pub fn shadow(&self, ex: &[S; 3], ey: &[S; 3]) -> HomCurve<S, 2> {
        self.map_linear(&[ex.clone(), ey.clone()])
    }
}

impl<S: Scalar> HomPatch<S> {
    /// The patch in the plane equation `a·x + c`: the tensor
    /// `a·(X, Y, Z) + c·W`, of the patch's degrees, the side of the plane
    /// where `W > 0`. Commutes with reversal and transposition to the
    /// bit; exact on rationals.
    pub fn compose_linear(&self, a: &[S; 3], c: &S) -> Tensor<S> {
        let [x, y, z, w] = self.tensors();
        let (m, n) = self.degrees();
        let coefs = (0..x.coefs().len()).map(|i| {
            combine([
                (&a[0], &x.coefs()[i]),
                (&a[1], &y.coefs()[i]),
                (&a[2], &z.coefs()[i]),
                (c, &w.coefs()[i]),
            ])
        });
        Tensor::new(m, n, coefs).expect("the patch's degrees")
    }
}

/// A form the compositions build of products, scalings and sums:
/// [`Poly`] and [`Tensor`].
trait Built<S: Scalar>: Clone + Sized {
    /// The only coefficient, if the form is a constant (degree 0).
    fn constant(&self) -> Option<&S>;
    fn mul(&self, o: &Self) -> Option<Self>;
    fn add(&self, o: &Self) -> Self;
    fn scale(&self, k: &S) -> Self;
    fn neg(&self) -> Self;
    /// The zero form at `d` times this one's degrees, at `z`'s level.
    fn zeros(&self, d: usize, z: &S) -> Option<Self>;
    fn first(&self) -> &S;
    /// The form of these degrees with the coefficients `f(aᵢ, bᵢ)`.
    fn zip_map(&self, o: &Self, f: impl Fn(&S, &S) -> S) -> Self;
    /// The constant `k` (degree 0).
    fn constant_of(k: &S) -> Self;
}

impl<S: Scalar> Built<S> for Poly<S> {
    fn constant(&self) -> Option<&S> {
        (self.degree() == 0).then(|| &self.coefs()[0])
    }

    fn mul(&self, o: &Self) -> Option<Self> {
        Poly::mul(self, o)
    }

    fn add(&self, o: &Self) -> Self {
        Poly::add(self, o)
    }

    fn scale(&self, k: &S) -> Self {
        Poly::scale(self, k)
    }

    fn neg(&self) -> Self {
        Poly::neg(self)
    }

    fn zeros(&self, d: usize, z: &S) -> Option<Self> {
        let n = self.degree().checked_mul(d)?;
        (n <= MAX_DEGREE).then(|| Poly::from_coefs((0..=n).map(|_| S::int(0, z)).collect()))
    }

    fn first(&self) -> &S {
        &self.coefs()[0]
    }

    fn zip_map(&self, o: &Self, f: impl Fn(&S, &S) -> S) -> Self {
        debug_assert_eq!(self.degree(), o.degree());
        Poly::from_coefs(
            self.coefs()
                .iter()
                .zip(o.coefs())
                .map(|(a, b)| f(a, b))
                .collect(),
        )
    }

    fn constant_of(k: &S) -> Self {
        Poly::from_coefs(smallvec::smallvec![k.clone()])
    }
}

impl<S: Scalar> Built<S> for Tensor<S> {
    fn constant(&self) -> Option<&S> {
        (self.degrees() == (0, 0)).then(|| &self.coefs()[0])
    }

    fn mul(&self, o: &Self) -> Option<Self> {
        Tensor::mul(self, o)
    }

    fn add(&self, o: &Self) -> Self {
        Tensor::add(self, o)
    }

    fn scale(&self, k: &S) -> Self {
        Tensor::scale(self, k)
    }

    fn neg(&self) -> Self {
        Tensor::neg(self)
    }

    fn zeros(&self, d: usize, z: &S) -> Option<Self> {
        let (m, n) = self.degrees();
        let (m, n) = (m.checked_mul(d)?, n.checked_mul(d)?);
        if m > MAX_DEGREE || n > MAX_DEGREE {
            return None;
        }
        Tensor::new(m, n, (0..(m + 1) * (n + 1)).map(|_| S::int(0, z)))
    }

    fn first(&self) -> &S {
        &self.coefs()[0]
    }

    fn zip_map(&self, o: &Self, f: impl Fn(&S, &S) -> S) -> Self {
        debug_assert_eq!(self.degrees(), o.degrees());
        let (m, n) = self.degrees();
        Tensor::new(
            m,
            n,
            self.coefs().iter().zip(o.coefs()).map(|(a, b)| f(a, b)),
        )
        .expect("the same degrees")
    }

    fn constant_of(k: &S) -> Self {
        Tensor::new(0, 0, [k.clone()]).expect("a constant")
    }
}

/// `a·b`, a constant factor (degree 0) a scaling: no binomial weights,
/// one rounding per coefficient. `None` past [`MAX_DEGREE`].
fn times<S: Scalar, F: Built<S>>(a: &F, b: &F) -> Option<F> {
    if let Some(k) = a.constant() {
        Some(b.scale(k))
    } else if let Some(k) = b.constant() {
        Some(a.scale(k))
    } else {
        a.mul(b)
    }
}

/// `a·b` of factors `None` for the constant 1.
fn times_opt<S: Scalar, F: Built<S>>(a: Option<&F>, b: Option<&F>) -> Option<Option<F>> {
    Some(match (a, b) {
        (Some(a), Some(b)) => Some(times(a, b)?),
        (Some(x), None) | (None, Some(x)) => Some(x.clone()),
        (None, None) => None,
    })
}

/// `f·x` under the rule of the linear forms: `None` for `f` exactly 0, a
/// copy or negation for ±1; `x` `None` for the constant 1.
fn weighted<S: Scalar, F: Built<S>>(f: &S, x: Option<&F>, one: impl Fn(&S) -> F) -> Option<F> {
    if f.is_int(0) {
        return None;
    }
    let Some(x) = x else {
        return Some(one(f));
    };
    Some(if f.is_int(1) {
        x.clone()
    } else if f.neg().is_int(1) {
        x.neg()
    } else {
        x.scale(f)
    })
}

/// The sum of two optional terms, `None` the zero form.
fn add_opt<S: Scalar, F: Built<S>>(a: &Option<F>, b: &Option<F>) -> Option<F> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.add(b)),
        (Some(x), None) | (None, Some(x)) => Some(x.clone()),
        (None, None) => None,
    }
}

/// `x^e` for `e` in `1..=n`, by repeated products (`x^e = x^(e−1)·x`);
/// `None` past [`MAX_DEGREE`].
fn powers<S: Scalar, F: Built<S>>(x: &F, n: usize) -> Option<Vec<F>> {
    let mut out: Vec<F> = Vec::with_capacity(n);
    for _ in 0..n {
        let next = match out.last() {
            None => x.clone(),
            Some(p) => times(p, x)?,
        };
        out.push(next);
    }
    Some(out)
}

/// `x^e` from [`powers`]'s list, `None` for `e = 0`.
fn power<F>(p: &[F], e: usize) -> Option<&F> {
    e.checked_sub(1).map(|i| &p[i])
}

/// The homogeneous Bernstein basis of degree `m` at the rational
/// parameter `num / den`: `C(m, a)·(den − num)^(m−a)·num^a` for
/// `a = 0..=m`, of degree `m·k` (`k` the higher of the two degrees, the
/// other raised to it first); `None` in place of the constant 1
/// (`m = 0`). `None` past [`MAX_DEGREE`].
fn basis<S: Scalar>(m: usize, num: &Poly<S>, den: &Poly<S>) -> Option<Vec<Option<Poly<S>>>> {
    let k = num.degree().max(den.degree());
    let (num, den) = (
        &num.elevate(k - num.degree())?,
        &den.elevate(k - den.degree())?,
    );
    let s = den.sub(num);
    let (ps, pn) = (powers(&s, m)?, powers(num, m)?);
    (0..=m)
        .map(|a| {
            let b = times_opt(power(&ps, m - a), power(&pn, a))?;
            let c = binom(m, a);
            Some(match b {
                Some(b) if c != 1 => {
                    let k = S::int_u64(c, &b.coefs()[0]);
                    Some(b.scale(&k))
                }
                b => b,
            })
        })
        .collect()
}

/// The products `Bₐ(u)·Bᵦ(v)` of the two directions' bases, row-major in
/// `u` as a patch's net (`None` for the constant 1).
fn grid<S: Scalar>(
    (m, n): (usize, usize),
    u: [&Poly<S>; 2],
    v: [&Poly<S>; 2],
) -> Option<Vec<Option<Poly<S>>>> {
    let (bu, bv) = (basis(m, u[0], u[1])?, basis(n, v[0], v[1])?);
    let mut out = Vec::with_capacity(bu.len() * bv.len());
    for a in &bu {
        for b in &bv {
            out.push(times_opt(a.as_ref(), b.as_ref())?);
        }
    }
    Some(out)
}

/// `Σₐᵦ fₐᵦ·gridₐᵦ`, the terms weighted by [`weighted`] and summed by
/// `sym_fold2` over the `(m + 1) × (n + 1)` grid; the zero polynomial at
/// the composition's degree, at `like`'s level, if every term is left
/// out.
fn along<S: Scalar>(f: &Tensor<S>, grid: &[Option<Poly<S>>], like: &S) -> Poly<S> {
    let (m, n) = f.degrees();
    let terms: SmallVec<[Option<Poly<S>>; 9]> = f
        .coefs()
        .iter()
        .zip(grid)
        .map(|(c, g)| weighted(c, g.as_ref(), Poly::constant_of))
        .collect();
    let deg = grid.iter().flatten().map(Poly::degree).next().unwrap_or(0);
    let sum = sym_fold2(terms, m + 1, n + 1, add_opt);
    sum.unwrap_or_else(|| Poly::from_coefs((0..=deg).map(|_| S::int(0, like)).collect()))
}

impl<S: Scalar> Tensor<S> {
    /// The form along the curve `(u, v) = (U/D_u, V/D_v)` in its domain,
    /// `u = [U, D_u]` and `v = [V, D_v]` (each pair at its higher degree,
    /// `k_u` and `k_v`), times `D_u^m·D_v^n`: the polynomial
    /// `Σₐᵦ fₐᵦ·Bₐ(U, D_u)·Bᵦ(V, D_v)` of degree `m·k_u + n·k_v` (module
    /// docs). Its sign is the form's along the curve where `D_u^m·D_v^n`
    /// is positive. `None` past [`MAX_DEGREE`]. Commutes with reversal of
    /// the curve and with transposition (the parameters swapped) to the
    /// bit; exact on rationals.
    pub fn compose_params(&self, u: [&Poly<S>; 2], v: [&Poly<S>; 2]) -> Option<Poly<S>> {
        let g = grid(self.degrees(), u, v)?;
        Some(along(self, &g, &self.coefs()[0]))
    }
}

impl<S: Scalar> HomPatch<S> {
    /// The patch along the curve `(u, v) = (U/D_u, V/D_v)` in its domain
    /// ([`Tensor::compose_params`] of each coordinate tensor, the basis
    /// products shared): the space curve `(X, Y, Z, W)` of degree
    /// `m·k_u + n·k_v` whose point is the patch's at the curve's. `None`
    /// past [`MAX_DEGREE`].
    pub fn compose_params(&self, u: [&Poly<S>; 2], v: [&Poly<S>; 2]) -> Option<HomCurve<S, 3>> {
        let g = grid(self.degrees(), u, v)?;
        let t = self.tensors();
        let like = &t[3].coefs()[0];
        let [x, y, z, w] = [0, 1, 2, 3].map(|k| along(&t[k], &g, like));
        HomCurve::from_polys([x, y, z], w)
    }

    /// The patch along a p-curve span `(U, V, W)` in its local
    /// coordinates ([`HomCurve::translated`] to the tile's corner), its
    /// two parameters over the one weight: [`compose_params`] with
    /// `u = [U, W]`, `v = [V, W]`, of degree `(m + n)·k` for a span of
    /// degree `k` (8 for a quadratic span on a biquadratic tile). `None`
    /// past [`MAX_DEGREE`].
    ///
    /// [`compose_params`]: Self::compose_params
    pub fn compose_span(&self, p: &HomCurve<S, 2>) -> Option<HomCurve<S, 3>> {
        let [pu, pv] = p.coords();
        let w = p.weights();
        self.compose_params([pu, w], [pv, w])
    }
}

impl<S: Scalar, const N: usize> HomCurve<S, N> {
    /// The span moved by `−d`: `(P − d·W, W)`, its point `x − d` (local
    /// coordinates at the corner `d`, such as a tile's), each coefficient
    /// `Pᵢ + (−d)·Wᵢ` by the rule of the linear forms (a coordinate of
    /// `d` exactly 0 a copy). The weights keep their bits; commutes with
    /// reversal to the bit; exact on rationals.
    pub fn translated(&self, d: &[S; N]) -> Self {
        let one = S::int(1, &self.weights().coefs()[0]);
        let p = std::array::from_fn(|k| {
            let nd = d[k].neg();
            let c: Coefs<S> = self.coords()[k]
                .coefs()
                .iter()
                .zip(self.weights().coefs())
                .map(|(x, w)| combine([(&one, x), (&nd, w)]))
                .collect();
            Poly::from_coefs(c)
        });
        HomCurve::from_polys(p, self.weights().clone()).expect("one degree")
    }
}

impl<S: Scalar> HomCurve<S, 3> {
    /// The seam numerator of this curve `A = (H, H₃)` (a surface along its
    /// p-curve span, [`HomPatch::compose_span`]) against the space curve
    /// `c = (X, W_C)` with the same parameter: `N = H·W_C − X·H₃`, of the
    /// two degrees' sum, so that `A(t) − c(t) = N(t) / (H₃(t)·W_C(t))`
    /// (module docs). `None` past [`MAX_DEGREE`]. Commutes with
    /// reversal of both to the bit; exact on rationals.
    pub fn seam_numerator(&self, c: &HomCurve<S, 3>) -> Option<[Poly<S>; 3]> {
        let (h3, wc) = (self.weights(), c.weights());
        let n = |m: usize| -> Option<Poly<S>> {
            Some(self.coords()[m].mul(wc)?.sub(&c.coords()[m].mul(h3)?))
        };
        Some([n(0)?, n(1)?, n(2)?])
    }

    /// The span in the implicit equation `f` (its homogeneous form at
    /// `(X, Y, Z, W)`, module docs): a polynomial of degree `d·c`, `W^d·F(x)` along the
    /// span. `None` past [`MAX_DEGREE`].
    pub fn compose_implicit(&self, f: &Implicit<S>) -> Option<Poly<S>> {
        let [x, y, z] = self.coords();
        f.compose([x, y, z, self.weights()])
    }
}

impl<S: Scalar> HomPatch<S> {
    /// The patch in the implicit equation `f` (its homogeneous form at
    /// `(X, Y, Z, W)`, module docs): a tensor of degrees `(d·m, d·n)`, `W^d·F(x)` over
    /// the patch, (8, 8) for a quartic on a biquadratic tile. `None` past
    /// [`MAX_DEGREE`].
    pub fn compose_implicit(&self, f: &Implicit<S>) -> Option<Tensor<S>> {
        let [x, y, z, w] = self.tensors();
        f.compose([x, y, z, w])
    }
}

/// An implicit surface `F(x) = 0` of degree `d`: a polynomial in the
/// point relative to its own origin `o` (its exact frame),
/// `F(x) = Σ c·Π (xₖ − oₖ)^eₖ`, stored as the terms of its homogeneous
/// form `F̂(y, W) = Σ c·Π yₖ^eₖ·W^e₃`, `Σ eₖ = d`, `y = X − o·W`, so that
/// `F̂(X − o·W, W) = W^d·F(X / W)`. The terms are kept in the descending
/// lexicographic order of their exponents `(e₀, e₁, e₂, e₃)` (`x²`, `xy`,
/// …, `x`, `y²`, …, the constant last; so a plane's terms come as
/// [`HomPatch::compose_linear`] sums them, and its composition has the
/// same bits); a coefficient exactly zero is dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct Implicit<S> {
    degree: usize,
    origin: [S; 3],
    terms: Vec<([usize; 4], S)>,
}

impl<S: Scalar> Implicit<S> {
    /// The implicit of degree `d` with the origin `origin` and the terms
    /// `c·Π (xₖ − oₖ)^eₖ`, `(e₀, e₁, e₂)` with `e₀ + e₁ + e₂ ≤ d`: `None`
    /// for a term past the degree, a repeated exponent, a degree past
    /// [`MAX_DEGREE`] or no term (zero coefficients count as terms, then
    /// are dropped).
    pub fn new(
        degree: usize,
        origin: [S; 3],
        terms: impl IntoIterator<Item = ([usize; 3], S)>,
    ) -> Option<Self> {
        if degree > MAX_DEGREE {
            return None;
        }
        let mut t: Vec<([usize; 4], S)> = Vec::new();
        let mut any = false;
        for (e, c) in terms {
            any = true;
            let s = e[0].checked_add(e[1])?.checked_add(e[2])?;
            let h = degree.checked_sub(s)?;
            t.push(([e[0], e[1], e[2], h], c));
        }
        t.sort_by_key(|x| std::cmp::Reverse(x.0));
        if !any || t.windows(2).any(|w| w[0].0 == w[1].0) {
            return None;
        }
        t.retain(|(_, c)| !c.is_int(0));
        Some(Implicit {
            degree,
            origin,
            terms: t,
        })
    }

    /// The degree `d`.
    #[inline]
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// The origin `o` of the frame.
    #[inline]
    pub fn origin(&self) -> &[S; 3] {
        &self.origin
    }

    /// The terms of the homogeneous form, `([e₀, e₁, e₂, e₃], c)` with
    /// `Σ eₖ = d`, in order (none with a zero coefficient).
    #[inline]
    pub fn terms(&self) -> &[([usize; 4], S)] {
        &self.terms
    }

    /// `F̂(X − o·W, W)` at the homogeneous point `(X, W)`, each term's
    /// powers by repeated products and the terms summed in order:
    /// `W^d·F(X / W)`.
    pub fn eval_hom(&self, x: &[S; 3], w: &S) -> S {
        let y: [S; 3] = std::array::from_fn(|k| x[k].sub(&self.origin[k].mul(w)));
        let vars = [&y[0], &y[1], &y[2], w];
        let mut acc: Option<S> = None;
        for (e, c) in &self.terms {
            let mut t = c.clone();
            for (k, &ek) in e.iter().enumerate() {
                for _ in 0..ek {
                    t = t.mul(vars[k]);
                }
            }
            acc = Some(match acc {
                None => t,
                Some(a) => a.add(&t),
            });
        }
        acc.unwrap_or_else(|| S::int(0, w))
    }

    /// The homogeneous form at the forms `(X, Y, Z, W)` (each of the
    /// same degrees): `Σ c·Π yₖ^eₖ·W^e₃` with `yₖ = Xₖ − oₖ·W`
    /// coefficient by coefficient by the rule of the linear forms, each
    /// power by repeated products, each term the product of its powers in
    /// the order of the variables times its coefficient (±1 no rounding),
    /// the terms summed in their order. `d` times the forms' degrees;
    /// `None` past [`MAX_DEGREE`].
    fn compose<F: Built<S>>(&self, h: [&F; 4]) -> Option<F> {
        let w = h[3];
        let like = w.first();
        let one = S::int(1, like);
        let y: [F; 3] = std::array::from_fn(|k| {
            let no = self.origin[k].neg();
            h[k].zip_map(w, |x, wc| combine([(&one, x), (&no, wc)]))
        });
        let vars: [&F; 4] = [&y[0], &y[1], &y[2], w];
        let mut top = [0usize; 4];
        for (e, _) in &self.terms {
            for k in 0..4 {
                top[k] = top[k].max(e[k]);
            }
        }
        let pw: Vec<Vec<F>> = (0..4)
            .map(|k| powers(vars[k], top[k]))
            .collect::<Option<_>>()?;
        let mut acc: Option<F> = None;
        for (e, c) in &self.terms {
            let mut prod: Option<F> = None;
            for k in 0..4 {
                if e[k] > 0 {
                    prod = times_opt(prod.as_ref(), Some(&pw[k][e[k] - 1]))?;
                }
            }
            let t = weighted(c, prod.as_ref(), F::constant_of);
            acc = add_opt(&acc, &t);
        }
        match acc {
            Some(a) => Some(a),
            None => w.zeros(self.degree, like),
        }
    }
}

#[cfg(test)]
mod tests;
