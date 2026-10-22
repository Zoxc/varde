//! Exact divisions of Bernstein forms: by the linear factors of the ends
//! of `[0, 1]` (`t`, `1 − t`) or the sides of `[0, 1]²` (`u`, `1 − u`,
//! `v`, `1 − v`), where the form is zero there, and degree reduction, the
//! inverse of elevation, of a form that is an elevation.
//!
//! **Linear factors.** `tᵃ·(1 − t)ᵇ·Bᵢᵏ(t) = C(k, i) / C(n, i + a) ·
//! Bⁿᵢ₊ₐ(t)` for `k = n − a − b`, so the polynomial `p = tᵃ·(1 − t)ᵇ·q`
//! has the coefficients `bᵢ₊ₐ = qᵢ·C(k, i) / C(n, i + a)` and `b₀ = 0`
//! (`a = 1`), `bₙ = 0` (`b = 1`): the quotient's coefficients are
//! `qᵢ = bᵢ₊ₐ·C(n, i + a) / C(k, i)`, each one of `p`'s times an exact
//! rational (in lowest terms; `n/(i + 1)` for `t`, `n/(n − i)` for
//! `1 − t`), with no sum and so no cancellation. The dropped coefficients
//! are not read: in floats a form that is zero at an end in truth has
//! there only coefficients within `±err` of zero, never a certified zero,
//! so the caller proves them zero on its exact data (a patch's singular
//! side, [`normal`](super::normal)), and the quotient's enclosures then
//! hold the true quotient. A weight of 1 is no operation (the kept end of
//! a one-sided division, to the bit); else one product and one quotient,
//! each rounded once. The quotient of the reversed form by the mirrored
//! factors is the reversed quotient, to the bit; on `[0, 1]²` the factors
//! of both directions make one weight per coefficient, so the quotient
//! commutes with transposition too, and a kept boundary line is the 1D
//! quotient of the form's.
//!
//! **Reduction.** A form of degree `d` that is the elevation of one of
//! degree `d − 1` has `bᵢ = (i·cᵢ₋₁ + (d − i)·cᵢ) / d`, `d + 1` equations
//! for `d` unknowns. Each half of `c` is solved from its own end,
//! `cᵢ = (d·bᵢ − i·cᵢ₋₁) / (d − i)` from the start and
//! `cᵢ₋₁ = (d·bᵢ − (d − i)·cᵢ) / i` from the end, so the error grows along
//! half the coefficients only, and the reversed form gives the reversed
//! result to the bit; for odd `d` the middle coefficient is the midpoint
//! of its two solutions, for even `d` one equation is left over. The
//! formulas hold for the true coefficients, so each result's bound, the
//! level's rounding carried through them, holds the true reduced form:
//! the residual of the redundant equation is never needed for the bound,
//! only checked, and a form certainly not an elevation (its residual
//! certainly non-zero; at the rational level any non-zero one) gives
//! `None`. On `[0, 1]²` every line in the direction is reduced.

use smallvec::SmallVec;

use super::poly::{Coefs, Poly, binom, divide, weigh};
use super::scalar::Scalar;
use super::tensor::{Dir, Tensor};

/// A side of `[0, 1]²`: the line where `u` or `v` is 0 or 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// `u = 0`: the net's first row, `b₀ⱼ`.
    U0,
    /// `u = 1`: the last row, `bₘⱼ`.
    U1,
    /// `v = 0`: the first column, `bᵢ₀`.
    V0,
    /// `v = 1`: the last column, `bᵢₙ`.
    V1,
}

impl Side {
    /// All four, in order.
    pub const ALL: [Side; 4] = [Side::U0, Side::U1, Side::V0, Side::V1];

    /// The direction whose coordinate is fixed on the side.
    pub fn dir(self) -> Dir {
        match self {
            Side::U0 | Side::U1 => Dir::U,
            Side::V0 | Side::V1 => Dir::V,
        }
    }

    /// Whether the fixed coordinate is 1 (else 0).
    pub fn at_one(self) -> bool {
        matches!(self, Side::U1 | Side::V1)
    }

    /// The same side of the form reversed in `dir`.
    pub fn reversed(self, dir: Dir) -> Side {
        match (self, dir) {
            (Side::U0, Dir::U) => Side::U1,
            (Side::U1, Dir::U) => Side::U0,
            (Side::V0, Dir::V) => Side::V1,
            (Side::V1, Dir::V) => Side::V0,
            (s, _) => s,
        }
    }

    /// The same side of the transposed form.
    pub fn transposed(self) -> Side {
        match self {
            Side::U0 => Side::V0,
            Side::U1 => Side::V1,
            Side::V0 => Side::U0,
            Side::V1 => Side::U1,
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of [`Side`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sides(u8);

impl Sides {
    /// No side.
    pub const NONE: Sides = Sides(0);

    /// The set with `side` added.
    pub fn with(self, side: Side) -> Sides {
        Sides(self.0 | side.bit())
    }

    /// Whether `side` is in the set.
    pub fn contains(self, side: Side) -> bool {
        self.0 & side.bit() != 0
    }

    /// Whether the set is empty.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The sides in [`Side::ALL`]'s order.
    pub fn iter(self) -> impl Iterator<Item = Side> {
        Side::ALL.into_iter().filter(move |&s| self.contains(s))
    }

    /// The sides of the form reversed in `dir`.
    pub fn reversed(self, dir: Dir) -> Sides {
        self.iter().map(|s| s.reversed(dir)).collect()
    }

    /// The sides of the transposed form.
    pub fn transposed(self) -> Sides {
        self.iter().map(Side::transposed).collect()
    }

    /// The factors `(a, b)` of `dir`: `a` for the side at 0, `b` at 1.
    fn factors(self, dir: Dir) -> (usize, usize) {
        let (s0, s1) = match dir {
            Dir::U => (Side::U0, Side::U1),
            Dir::V => (Side::V0, Side::V1),
        };
        (
            usize::from(self.contains(s0)),
            usize::from(self.contains(s1)),
        )
    }
}

impl FromIterator<Side> for Sides {
    fn from_iter<I: IntoIterator<Item = Side>>(it: I) -> Sides {
        it.into_iter().fold(Sides::NONE, Sides::with)
    }
}

impl From<Side> for Sides {
    fn from(side: Side) -> Sides {
        Sides::NONE.with(side)
    }
}

/// `p / q` in lowest terms (`q > 0`).
fn lowest(p: u128, q: u128) -> (u128, u128) {
    let (mut a, mut b) = (p, q);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    (p / a, q / a)
}

/// The weight of the quotient's coefficient `i` (degree `n`, factors
/// `tᵃ·(1 − t)ᵇ`): `C(n, i + a) / C(n − a − b, i)` in lowest terms.
fn ratio(n: usize, (a, b): (usize, usize), i: usize) -> (u64, u64) {
    let (p, q) = lowest(binom(n, i + a).into(), binom(n - a - b, i).into());
    let narrow = |x: u128| u64::try_from(x).expect("at most a binomial");
    (narrow(p), narrow(q))
}

/// `x·p / q`, no operation for a weight of 1 (`p`, `q` in lowest terms).
fn scaled<S: Scalar>(x: &S, (p, q): (u64, u64)) -> S {
    divide(weigh(p, x.clone()), q)
}

impl<S: Scalar> Poly<S> {
    /// The quotient by `t` (`start`) and `1 − t` (`end`), of degree
    /// `n − a − b`: the coefficients `bᵢ₊ₐ·C(n, i + a) / C(n − a − b, i)`,
    /// the dropped end coefficients not read (module docs); `None` below
    /// degree `a + b`. Exact where the polynomial is zero at the divided
    /// ends in truth, which the caller proves.
    pub fn divide_ends(&self, start: bool, end: bool) -> Option<Poly<S>> {
        let f = (usize::from(start), usize::from(end));
        let n = self.degree();
        let k = n.checked_sub(f.0 + f.1)?;
        let c = self.coefs();
        Some(Poly::from_coefs(
            (0..=k)
                .map(|i| scaled(&c[i + f.0], ratio(n, f, i)))
                .collect(),
        ))
    }

    /// The polynomial of degree `n − 1` whose elevation this is (module
    /// docs); `None` at degree 0 and where this is certainly not an
    /// elevation.
    pub fn reduced(&self) -> Option<Poly<S>> {
        reduce(self.coefs()).map(Poly::from_coefs)
    }
}

/// The reduction of the coefficients `b` (module docs).
fn reduce<S: Scalar>(b: &[S]) -> Option<Coefs<S>> {
    let d = b.len() - 1;
    if d == 0 {
        return None;
    }
    let du = |k: usize| u64::try_from(k).expect("degree at most MAX_DEGREE");
    let half = d / 2;
    let mut c: SmallVec<[Option<S>; 11]> = (0..d).map(|_| None).collect();
    // From the start: `c₀ = b₀`, `cᵢ = (d·bᵢ − i·cᵢ₋₁) / (d − i)`.
    let from_start = |i: usize, prev: Option<&S>| match prev {
        None => b[0].clone(),
        Some(p) => divide(
            weigh(du(d), b[i].clone()).sub(&weigh(du(i), p.clone())),
            du(d - i),
        ),
    };
    // From the end, its mirror: `c_{d−1} = b_d`, `cₖ = (d·bₖ₊₁ − (d − k −
    // 1)·cₖ₊₁) / (k + 1)`.
    let from_end = |k: usize, next: Option<&S>| match next {
        None => b[d].clone(),
        Some(x) => divide(
            weigh(du(d), b[k + 1].clone()).sub(&weigh(du(d - k - 1), x.clone())),
            du(k + 1),
        ),
    };
    for i in 0..half {
        let v = from_start(i, i.checked_sub(1).and_then(|p| c[p].as_ref()));
        c[i] = Some(v);
        let k = d - 1 - i;
        let v = from_end(k, c.get(k + 1).and_then(Option::as_ref));
        c[k] = Some(v);
    }
    if d % 2 == 1 {
        let left = from_start(half, half.checked_sub(1).and_then(|p| c[p].as_ref()));
        let right = from_end(half, c.get(half + 1).and_then(Option::as_ref));
        if left.sub(&right).sign().is_some() {
            return None;
        }
        c[half] = Some(left.mid(&right));
    } else {
        // The equation left over, `d·b_h = h·(c_{h−1} + c_h)`.
        let (l, r) = (c[half - 1].as_ref()?, c[half].as_ref()?);
        let residual = weigh(du(d), b[half].clone()).sub(&weigh(du(half), l.add(r)));
        if residual.sign().is_some() {
            return None;
        }
    }
    c.into_iter().collect()
}

impl<S: Scalar> Tensor<S> {
    /// The boundary line on `side`: the first or last row (a polynomial
    /// in `v`) for `u = 0` or `1`, the first or last column (in `u`) for
    /// `v = 0` or `1`.
    pub fn side(&self, side: Side) -> Poly<S> {
        let (m, n) = self.degrees();
        let c: Coefs<S> = match side {
            Side::U0 | Side::U1 => {
                let i = if side.at_one() { m } else { 0 };
                (0..=n).map(|j| self.get(i, j).clone()).collect()
            }
            Side::V0 | Side::V1 => {
                let j = if side.at_one() { n } else { 0 };
                (0..=m).map(|i| self.get(i, j).clone()).collect()
            }
        };
        Poly::from_coefs(c)
    }

    /// The quotient by the linear factors of `sides` (`u` for
    /// [`Side::U0`], `1 − u` for [`Side::U1`], …), of degrees one lower
    /// per side in its direction: `qᵢⱼ = bᵢ₊ₐ,ⱼ₊ₐ'` times the product of
    /// both directions' weights, one exact rational in lowest terms, the
    /// dropped rows and columns not read (module docs); `None` below
    /// degree 2 in a direction with both its sides. Exact where the form
    /// is zero on those sides in truth, which the caller proves.
    pub fn divide_sides(&self, sides: Sides) -> Option<Tensor<S>> {
        let (m, n) = self.degrees();
        let (fu, fv) = (sides.factors(Dir::U), sides.factors(Dir::V));
        let mk = m.checked_sub(fu.0 + fu.1)?;
        let nk = n.checked_sub(fv.0 + fv.1)?;
        let rv: SmallVec<[(u64, u64); 11]> = (0..=nk).map(|j| ratio(n, fv, j)).collect();
        let mut c: SmallVec<[S; 9]> = SmallVec::with_capacity((mk + 1) * (nk + 1));
        for i in 0..=mk {
            let (pu, qu) = ratio(m, fu, i);
            for (j, &(pv, qv)) in rv.iter().enumerate() {
                // Each weight is at most `n·(n − 1)` in lowest terms, so
                // their product fits; in lowest terms again, a weight of 1
                // in one direction leaves the other's.
                let (p, q) = lowest(
                    u128::from(pu) * u128::from(pv),
                    u128::from(qu) * u128::from(qv),
                );
                let x = self.get(i + fu.0, j + fv.0);
                c.push(match (u64::try_from(p), u64::try_from(q)) {
                    (Ok(p), Ok(q)) => scaled(x, (p, q)),
                    _ => scaled(&scaled(x, (pu, qu)), (pv, qv)),
                });
            }
        }
        Tensor::new(mk, nk, c)
    }

    /// The form one degree lower in `dir` whose elevation in `dir` this
    /// is, every line in `dir` reduced ([`Poly::reduced`]); `None` at
    /// degree 0 in `dir` and where a line certainly isn't an elevation.
    pub fn reduced(&self, dir: Dir) -> Option<Tensor<S>> {
        let (m, n) = self.degrees();
        match dir {
            Dir::U => {
                let cols = (0..=n)
                    .map(|j| reduce(self.column(j).as_slice()))
                    .collect::<Option<SmallVec<[Coefs<S>; 11]>>>()?;
                let c = (0..m).flat_map(|i| cols.iter().map(move |col| col[i].clone()));
                Tensor::new(m - 1, n, c)
            }
            Dir::V => {
                let rows = (0..=m)
                    .map(|i| {
                        let row: Coefs<S> = (0..=n).map(|j| self.get(i, j).clone()).collect();
                        reduce(&row)
                    })
                    .collect::<Option<SmallVec<[Coefs<S>; 11]>>>()?;
                Tensor::new(m, n - 1, rows.into_iter().flatten())
            }
        }
    }

    /// Column `j`, a polynomial in `u`.
    fn column(&self, j: usize) -> Coefs<S> {
        (0..=self.degree(Dir::U))
            .map(|i| self.get(i, j).clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
