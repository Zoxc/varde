//! Exact signs of polynomials in the input coordinates, with ties broken
//! by symbolic perturbation.
//!
//! A predicate is a polynomial in the coordinates of a few points, some
//! of which are perturbed: moved by `ε·n + ε²·T2 + ε³·T3` for an
//! infinitely small `ε`, their own direction `n` and two fixed generic
//! translations. Its value is then a polynomial in `ε`, and its sign is
//! the sign of the first coefficient that isn't zero. The constant
//! coefficient is first worked out in floating point with a running
//! bound on its rounding error ([`Approx`]); only when that can't tell
//! the sign (the value is zero or close to it: a tie, which flush CAD
//! geometry makes on purpose) is every coefficient worked out exactly,
//! with floating-point expansions ([`Exp`]). Every operation is a
//! correctly rounded `+ − ×`, so the signs are the same on every platform.

use glam::DVec3;

/// The second perturbation: a fixed translation of every perturbed point,
/// generic so that it breaks the ties the first one leaves.
const T2: DVec3 = DVec3::new(
    0.271_828_182_845_904_5,
    -0.593_762_184_935_115_2,
    0.689_413_223_871_442_7,
);
/// The third perturbation, independent of the first two.
const T3: DVec3 = DVec3::new(
    -0.318_274_654_401_827_1,
    0.161_803_398_874_989_5,
    0.428_571_428_571_428_6,
);

/// Half an ulp, relative.
const HALF_ULP: f64 = f64::EPSILON / 2.0;
/// Grows each error bound by a little more than its own rounding.
const INFLATE: f64 = 1.0 + 8.0 * f64::EPSILON;

/// Numbers predicates are evaluated in: [`Approx`] for the constant term
/// alone, fast, and [`Poly`]s of [`Exp`] (or of `Approx`) for every power
/// of `ε`.
pub(super) trait Num: Clone {
    fn lit(x: f64) -> Self;
    /// A perturbed coordinate: its value, then its coefficients of `ε`,
    /// `ε²` and `ε³`. Numbers without powers of `ε` keep the value.
    fn perturbed(c: [f64; 4]) -> Self;
    fn add(&self, other: &Self) -> Self;
    fn sub(&self, other: &Self) -> Self;
    fn mul(&self, other: &Self) -> Self;
}

/// A floating-point value with a bound on how far rounding may have taken
/// it from the exact value of the same expression.
#[derive(Debug, Clone, Copy)]
pub(super) struct Approx {
    v: f64,
    err: f64,
}

impl Approx {
    /// The sign, if the error bound decides it: `Some(0)` only when the
    /// value is exactly zero with no error.
    fn sign(self) -> Option<i8> {
        if self.v == 0.0 && self.err == 0.0 {
            Some(0)
        } else if self.v - self.err > 0.0 {
            Some(1)
        } else if self.v + self.err < 0.0 {
            Some(-1)
        } else {
            None
        }
    }

    /// The value as computed.
    pub(super) fn value(self) -> f64 {
        self.v
    }

    fn rounded(v: f64, err: f64) -> Approx {
        let err = (err + v.abs() * HALF_ULP + f64::MIN_POSITIVE) * INFLATE;
        Approx {
            v,
            err: if v.is_finite() { err } else { f64::INFINITY },
        }
    }
}

impl Num for Approx {
    fn lit(x: f64) -> Self {
        Approx { v: x, err: 0.0 }
    }

    fn perturbed(c: [f64; 4]) -> Self {
        Self::lit(c[0])
    }

    fn add(&self, o: &Self) -> Self {
        let (v, low) = two_sum(self.v, o.v);
        if self.err == 0.0 && o.err == 0.0 && low == 0.0 && v.is_finite() {
            // Exact: a difference of equal coordinates is a true zero.
            return Approx { v, err: 0.0 };
        }
        Approx::rounded(v, self.err + o.err)
    }

    fn sub(&self, o: &Self) -> Self {
        self.add(&Approx {
            v: -o.v,
            err: o.err,
        })
    }

    fn mul(&self, o: &Self) -> Self {
        let v = self.v * o.v;
        if self.err == 0.0 && o.err == 0.0 && (self.v == 0.0 || o.v == 0.0) {
            // A product with an exact zero is an exact zero.
            return Approx { v: 0.0, err: 0.0 };
        }
        let err = self.v.abs() * o.err + o.v.abs() * self.err + self.err * o.err;
        Approx::rounded(v, err)
    }
}

/// An exact value as a floating-point expansion: a sum of non-overlapping
/// components in increasing magnitude, none zero (so zero is empty).
#[derive(Debug, Clone, Default)]
pub(super) struct Exp(Vec<f64>);

impl Exp {
    /// The sign of the exact value: that of its largest component.
    fn sign(&self) -> i8 {
        match self.0.last() {
            Some(&x) if x > 0.0 => 1,
            Some(&x) if x < 0.0 => -1,
            _ => 0,
        }
    }

    /// `e + b`, exactly.
    fn grow(e: &[f64], b: f64) -> Vec<f64> {
        let mut out = Vec::with_capacity(e.len() + 1);
        let mut q = b;
        for &x in e {
            let (s, err) = two_sum(q, x);
            q = s;
            if err != 0.0 {
                out.push(err);
            }
        }
        if q != 0.0 {
            out.push(q);
        }
        out
    }

    /// `e · b`, exactly.
    fn scale(e: &[f64], b: f64) -> Vec<f64> {
        let mut out = Vec::with_capacity(2 * e.len());
        let Some((&first, rest)) = e.split_first() else {
            return out;
        };
        if b == 0.0 {
            return out;
        }
        let (mut q, low) = two_product(first, b);
        if low != 0.0 {
            out.push(low);
        }
        for &x in rest {
            let (hi, lo) = two_product(x, b);
            let (sum, err) = two_sum(q, lo);
            if err != 0.0 {
                out.push(err);
            }
            let (s, err) = fast_two_sum(hi, sum);
            q = s;
            if err != 0.0 {
                out.push(err);
            }
        }
        if q != 0.0 {
            out.push(q);
        }
        out
    }

    fn sum(a: &[f64], b: &[f64]) -> Vec<f64> {
        let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
        let mut out = long.to_vec();
        for &x in short {
            out = Self::grow(&out, x);
        }
        out
    }
}

impl Num for Exp {
    fn lit(x: f64) -> Self {
        Exp(if x == 0.0 { Vec::new() } else { vec![x] })
    }

    fn perturbed(c: [f64; 4]) -> Self {
        Self::lit(c[0])
    }

    fn add(&self, o: &Self) -> Self {
        Exp(Exp::sum(&self.0, &o.0))
    }

    fn sub(&self, o: &Self) -> Self {
        let neg: Vec<f64> = o.0.iter().map(|x| -x).collect();
        Exp(Exp::sum(&self.0, &neg))
    }

    fn mul(&self, o: &Self) -> Self {
        let (long, short) = if self.0.len() >= o.0.len() {
            (&self.0, &o.0)
        } else {
            (&o.0, &self.0)
        };
        let mut out = Vec::new();
        for &x in short {
            out = Exp::sum(&out, &Exp::scale(long, x));
        }
        Exp(out)
    }
}

fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    let bv = x - a;
    let av = x - bv;
    (x, (a - av) + (b - bv))
}

/// [`two_sum`] for `|a| >= |b|`.
fn fast_two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    (x, b - (x - a))
}

/// Splits `a` into two halves of 26 bits each (Dekker), so their products
/// are exact.
fn split(a: f64) -> (f64, f64) {
    const SPLITTER: f64 = 134_217_729.0; // 2^27 + 1
    let c = SPLITTER * a;
    let big = c - a;
    let hi = c - big;
    (hi, a - hi)
}

fn two_product(a: f64, b: f64) -> (f64, f64) {
    let x = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    let err = ((x - ah * bh) - al * bh) - ah * bl;
    (x, al * bl - err)
}

/// A polynomial in `ε`, lowest power first.
#[derive(Debug, Clone)]
pub(super) struct Poly<N>(pub(super) Vec<N>);

impl<N: Num> Num for Poly<N> {
    fn lit(x: f64) -> Self {
        Poly(vec![N::lit(x)])
    }

    fn perturbed(c: [f64; 4]) -> Self {
        Poly(c.map(N::lit).to_vec())
    }

    fn add(&self, o: &Self) -> Self {
        let n = self.0.len().max(o.0.len());
        Poly(
            (0..n)
                .map(|i| match (self.0.get(i), o.0.get(i)) {
                    (Some(a), Some(b)) => a.add(b),
                    (Some(a), None) => a.clone(),
                    (None, Some(b)) => b.clone(),
                    (None, None) => unreachable!(),
                })
                .collect(),
        )
    }

    fn sub(&self, o: &Self) -> Self {
        let n = self.0.len().max(o.0.len());
        Poly(
            (0..n)
                .map(|i| match (self.0.get(i), o.0.get(i)) {
                    (Some(a), Some(b)) => a.sub(b),
                    (Some(a), None) => a.clone(),
                    (None, Some(b)) => N::lit(0.0).sub(b),
                    (None, None) => unreachable!(),
                })
                .collect(),
        )
    }

    fn mul(&self, o: &Self) -> Self {
        let n = self.0.len() + o.0.len() - 1;
        let mut out: Vec<Option<N>> = vec![None; n];
        for (i, a) in self.0.iter().enumerate() {
            for (j, b) in o.0.iter().enumerate() {
                let p = a.mul(b);
                out[i + j] = Some(match out[i + j].take() {
                    Some(s) => s.add(&p),
                    None => p,
                });
            }
        }
        Poly(out.into_iter().map(|c| c.expect("every power")).collect())
    }
}

/// A point of a predicate: a vertex's position and, for a perturbed one,
/// its first perturbation direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Pt {
    pub(super) p: DVec3,
    /// `None` for a point that isn't perturbed.
    pub(super) n: Option<DVec3>,
}

/// A point or direction as a vector of numbers.
pub(super) type V3<N> = [N; 3];

impl Pt {
    /// The point, perturbed if it is.
    pub(super) fn v3<N: Num>(&self) -> V3<N> {
        let p = self.p;
        match self.n {
            Some(n) => [0, 1, 2].map(|i| N::perturbed([p[i], n[i], T2[i], T3[i]])),
            None => [0, 1, 2].map(|i| N::lit(p[i])),
        }
    }
}

/// A fixed direction.
pub(super) fn dir<N: Num>(d: DVec3) -> V3<N> {
    [0, 1, 2].map(|i| N::lit(d[i]))
}

pub(super) fn sub<N: Num>(a: &V3<N>, b: &V3<N>) -> V3<N> {
    [0, 1, 2].map(|i| a[i].sub(&b[i]))
}

pub(super) fn dot<N: Num>(a: &V3<N>, b: &V3<N>) -> N {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

pub(super) fn cross<N: Num>(a: &V3<N>, b: &V3<N>) -> V3<N> {
    [
        a[1].mul(&b[2]).sub(&a[2].mul(&b[1])),
        a[2].mul(&b[0]).sub(&a[0].mul(&b[2])),
        a[0].mul(&b[1]).sub(&a[1].mul(&b[0])),
    ]
}

/// `det[a, b, c] = (a × b)·c`.
pub(super) fn det<N: Num>(a: &V3<N>, b: &V3<N>, c: &V3<N>) -> N {
    dot(&cross(a, b), c)
}

/// A predicate: a number built from its points.
pub(super) trait Pred {
    fn eval<N: Num>(&self) -> N;
}

/// The predicate's sign as `ε → 0⁺`: that of its first non-zero
/// coefficient, or 0 if every one is zero.
pub(super) fn sign(pred: &impl Pred) -> i8 {
    if let Some(s) = pred.eval::<Approx>().sign()
        && s != 0
    {
        return s;
    }
    pred.eval::<Poly<Exp>>()
        .0
        .iter()
        .map(Exp::sign)
        .find(|&s| s != 0)
        .unwrap_or(0)
}

/// The predicate's coefficients in floating point, perturbations
/// included: for positions, not decisions.
pub(super) fn approx(pred: &impl Pred) -> Vec<f64> {
    pred.eval::<Poly<Approx>>()
        .0
        .into_iter()
        .map(Approx::value)
        .collect()
}

/// The sign of `(b − a) × (c − a)` in the plane, exactly.
pub(super) fn orient2d(a: glam::DVec2, b: glam::DVec2, c: glam::DVec2) -> i8 {
    fn eval<N: Num>(a: glam::DVec2, b: glam::DVec2, c: glam::DVec2) -> N {
        let (ax, ay) = (N::lit(a.x), N::lit(a.y));
        let (bx, by) = (N::lit(b.x).sub(&ax), N::lit(b.y).sub(&ay));
        let (cx, cy) = (N::lit(c.x).sub(&ax), N::lit(c.y).sub(&ay));
        bx.mul(&cy).sub(&by.mul(&cx))
    }
    if let Some(s) = eval::<Approx>(a, b, c).sign() {
        return s;
    }
    eval::<Exp>(a, b, c).sign()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rng::Rng;

    fn exact_value(e: &Exp) -> f64 {
        e.0.iter().rev().sum()
    }

    #[test]
    fn expansions_are_exact() {
        // (1 + 2^-60) - 1 = 2^-60, lost in floating point.
        let tiny = 2f64.powi(-60);
        let x = Exp::lit(1.0).add(&Exp::lit(tiny)).sub(&Exp::lit(1.0));
        assert_eq!(exact_value(&x), tiny);
        // (1e8 + 1)² − 1e16 − 2e8 = 1.
        let a = Exp::lit(1e8).add(&Exp::lit(1.0));
        let y = a.mul(&a).sub(&Exp::lit(1e16)).sub(&Exp::lit(2e8));
        assert_eq!(y.sign(), 1);
        assert_eq!(exact_value(&y), 1.0);
        assert_eq!(Exp::lit(3.0).sub(&Exp::lit(3.0)).sign(), 0);
    }

    #[test]
    fn approximate_signs_are_right_when_given() {
        let mut rng = Rng::new(7);
        for _ in 0..2000 {
            let v: Vec<f64> = (0..6).map(|_| rng.range(-1e3, 1e3)).collect();
            let expr = |n: &dyn Fn(f64) -> Approx| {
                n(v[0])
                    .mul(&n(v[1]))
                    .sub(&n(v[2]).mul(&n(v[3])))
                    .add(&n(v[4]).mul(&n(v[5])))
            };
            let a = expr(&Approx::lit);
            let e = Exp::lit(v[0])
                .mul(&Exp::lit(v[1]))
                .sub(&Exp::lit(v[2]).mul(&Exp::lit(v[3])))
                .add(&Exp::lit(v[4]).mul(&Exp::lit(v[5])));
            if let Some(s) = a.sign() {
                assert_eq!(s, e.sign());
            }
        }
    }

    #[test]
    fn orientation_is_exact_near_a_line() {
        let (o, a) = (glam::DVec2::ZERO, glam::DVec2::new(0.5, 0.5));
        assert_eq!(orient2d(o, a, glam::DVec2::new(1.0, 1.0)), 0);
        assert_eq!(orient2d(o, a, glam::DVec2::new(1.0, 1.0 + 1e-15)), 1);
        assert_eq!(orient2d(o, a, glam::DVec2::new(1.0, 1.0 - 1e-15)), -1);
        // Far from the origin, where the float filter can't decide.
        let (p, q) = (
            glam::DVec2::new(1e6, 1e6),
            glam::DVec2::new(1e6 + 0.1, 1e6 + 0.1),
        );
        let r = glam::DVec2::new(1e6 + 0.2, 1e6 + 0.2);
        let exact = Exp::lit(q.x)
            .sub(&Exp::lit(p.x))
            .mul(&Exp::lit(r.y).sub(&Exp::lit(p.y)))
            .sub(
                &Exp::lit(q.y)
                    .sub(&Exp::lit(p.y))
                    .mul(&Exp::lit(r.x).sub(&Exp::lit(p.x))),
            );
        assert_eq!(orient2d(p, q, r), exact.sign());
    }
}
