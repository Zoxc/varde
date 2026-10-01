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
//! with floating-point expansions ([`Exp`]). [`sign_tied`] takes near
//! ties as ties, at every order. Every operation is a
//! correctly rounded `+ − ×`, so the signs are the same on every platform.

use glam::DVec3;

/// The second perturbation: a fixed translation of every perturbed point,
/// generic so that it breaks the ties the first one leaves.
pub(crate) const T2: DVec3 = DVec3::new(
    0.271_828_182_845_904_5,
    -0.593_762_184_935_115_2,
    0.689_413_223_871_442_7,
);
/// The third perturbation, independent of the first two.
pub(crate) const T3: DVec3 = DVec3::new(
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
pub(crate) trait Num: Clone {
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

    /// The value, rounded.
    fn value(&self) -> f64 {
        self.0.iter().sum()
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
pub(crate) struct Pt {
    pub(crate) p: DVec3,
    /// `None` for a point that isn't perturbed.
    pub(crate) n: Option<DVec3>,
}

/// A point or direction as a vector of numbers.
pub(crate) type V3<N> = [N; 3];

/// Coordinates (and perturbation directions' components) smaller than
/// this count as zero. Products of expansions are exact only while they
/// stay clear of underflow: with every number zero or at least this, the
/// products of up to six of them the predicates take are. Far below any
/// geometry, and the same for every predicate, so they all still see one
/// configuration.
const FLUSH: f64 = 1.0 / (1u128 << 100) as f64;

fn flush(x: f64) -> f64 {
    if x.abs() < FLUSH { 0.0 } else { x }
}

impl Pt {
    /// The point, perturbed if it is, with coordinates below [`FLUSH`]
    /// taken as zero.
    pub(crate) fn v3<N: Num>(&self) -> V3<N> {
        let p = self.p;
        match self.n {
            Some(n) => [0, 1, 2].map(|i| N::perturbed([flush(p[i]), flush(n[i]), T2[i], T3[i]])),
            None => [0, 1, 2].map(|i| N::lit(flush(p[i]))),
        }
    }
}

/// A fixed direction.
pub(crate) fn dir<N: Num>(d: DVec3) -> V3<N> {
    [0, 1, 2].map(|i| N::lit(d[i]))
}

pub(crate) fn sub<N: Num>(a: &V3<N>, b: &V3<N>) -> V3<N> {
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
pub(crate) fn det<N: Num>(a: &V3<N>, b: &V3<N>, c: &V3<N>) -> N {
    dot(&cross(a, b), c)
}

/// A predicate: a number built from its points.
pub(crate) trait Pred {
    fn eval<N: Num>(&self) -> N;
    /// How much the value changes per unit of distance the configuration
    /// is from its tie (a point from a line or plane, say), roughly: its
    /// constant term within `tie` times this is a tie in [`sign_tied`].
    fn scale(&self) -> f64 {
        0.0
    }
}

/// The predicate's sign as `ε → 0⁺`: that of its first non-zero
/// coefficient, or 0 if every one is zero.
pub(crate) fn sign(pred: &impl Pred) -> i8 {
    if let Some(s) = pred.eval::<Approx>().sign()
        && s != 0
    {
        return s;
    }
    worked_out();
    pred.eval::<Poly<Exp>>()
        .0
        .iter()
        .map(Exp::sign)
        .find(|&s| s != 0)
        .unwrap_or(0)
}

/// The sum of `parts`' values (their constant terms), worked out exactly
/// and then rounded: within a few units in the last place of the exact
/// sum. The parts are worked out through `par_map` and added in order, so
/// the rounded sum doesn't depend on the thread count. Counts as one sign
/// worked out exactly ([`exact_count`]).
pub(crate) fn sum_value<P: Pred + Sync>(parts: &[P]) -> f64 {
    worked_out();
    crate::par::par_map(parts, |part| part.eval::<Exp>())
        .iter()
        .fold(Exp::default(), |sum, part| sum.add(part))
        .value()
}

/// [`sign`], taking a constant term within `tie` of the distance units
/// ([`Pred::scale`]) as zero: the configuration within `tie` of a tie is
/// decided as the tie, by the perturbation. The curved primitives decide
/// heights that close as ties too, so where a curved operand meets a flat
/// one, both see one configuration: a vertex `1e-9` off a face at the
/// coarsest tolerance is on it for all of them, not beside it for the
/// exact predicates and on it for the numerical ones. With `tie` zero,
/// exactly [`sign`].
///
/// Once the constant term is a tie, each later coefficient that is only
/// rounding (within [`RHO`] of its size, [`Abs`]) is taken as zero too: at
/// the exact tie the rounded configuration stands for, the first order is
/// often zero as well (two collinear edges' `Height`, a vertex whose
/// direction lies in a face's plane against it), and its rounding's sign
/// is noise. Each predicate is then decided as that exact tie is, so they
/// all describe one configuration. The rule is free of scale: a
/// coefficient is compared with its own terms. With every later order
/// only rounding, the sign is 0, as for a tie in every power. A scale of
/// 0 keeps the tie: only an exact zero is then one, and the later orders
/// that are only rounding are still skipped.
pub(super) fn sign_tied(pred: &impl Pred, tie: f64) -> i8 {
    let limit = tie * pred.scale();
    if limit.is_nan() || tie <= 0.0 {
        return sign(pred);
    }
    let approx = pred.eval::<Approx>();
    if approx.v.abs() - approx.err > limit {
        return if approx.v > 0.0 { 1 } else { -1 };
    }
    worked_out();
    let poly = pred.eval::<Poly<Exp>>().0;
    if !poly.first().is_some_and(|c| c.value().abs() <= limit) {
        return poly.first().map_or(0, Exp::sign);
    }
    // The constant term is a tie: so is every later order that is only
    // rounding, as it is zero at the exact tie this stands for.
    let rounding = |c: &Exp, size: &Abs| size.0.is_finite() && c.value().abs() <= RHO * size.0;
    let size = pred.eval::<Poly<Abs>>().0;
    poly.iter()
        .zip(&size)
        .skip(1)
        .find(|(c, size)| c.sign() != 0 && !rounding(c, size))
        .map_or(0, |(c, _)| c.sign())
}

/// A coefficient of a near tie's later order whose value is within this
/// share of its size ([`Abs`]) is only rounding, and taken as zero by
/// [`sign_tied`]. Anything from `1e-13` to `1e-7` decided the same on
/// turned flush boxes; rounding leaves some `1e-16`.
pub(super) const RHO: f64 = 1.0 / (1u64 << 32) as f64;

/// The size of a value: the same expression on its terms' absolute
/// values, `+` and `−` adding them and `×` multiplying them, in floating
/// point (the bound needn't be exact, only the same on every platform).
/// A coefficient much smaller than its size is all cancellation.
#[derive(Debug, Clone, Copy)]
struct Abs(f64);

impl Num for Abs {
    fn lit(x: f64) -> Self {
        Abs(x.abs())
    }

    fn perturbed(c: [f64; 4]) -> Self {
        Self::lit(c[0])
    }

    fn add(&self, o: &Self) -> Self {
        Abs(self.0 + o.0)
    }

    fn sub(&self, o: &Self) -> Self {
        Abs(self.0 + o.0)
    }

    fn mul(&self, o: &Self) -> Self {
        Abs(self.0 * o.0)
    }
}

/// `num / den` as `ε → 0⁺`, in floating point, for positions (not
/// decisions): the ratio of their constant terms, worked out exactly and
/// then rounded unless floating point already has both to [`CLOSE`], or
/// where `den`'s is zero, the ratio of the first coefficients of `den`
/// that isn't and of `num`'s beside it. Not a number when `den` is zero
/// in every power.
///
/// The constant terms of a near tie are both tiny and all rounding, so
/// their plain floating-point ratio can be anything: an edge nearly in
/// a face's plane would be cut far from where it crosses it.
pub(super) fn ratio(num: &impl Pred, den: &impl Pred) -> f64 {
    let (n, d) = (num.eval::<Approx>(), den.eval::<Approx>());
    let close = |x: Approx| x.err <= x.v.abs() * CLOSE;
    if d.v != 0.0 && close(d) && close(n) {
        return n.v / d.v;
    }
    worked_out();
    let (n, d) = (num.eval::<Poly<Exp>>(), den.eval::<Poly<Exp>>());
    match d.0.iter().position(|c| c.sign() != 0) {
        Some(k) => n.0.get(k).map_or(0.0, Exp::value) / d.0[k].value(),
        None => f64::NAN,
    }
}

/// The relative error below which [`ratio`] takes floating point's
/// values as they are.
const CLOSE: f64 = 1e-12;

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
    worked_out();
    eval::<Exp>(a, b, c).sign()
}

/// The sign of `(b − a) × (c − a)` in the plane, exactly, for points
/// of which those with `true` are moved towards `center` by an infinitely
/// small fraction `δ` of the way (`p + δ·(center − p)`): ties between
/// points that move and points that don't are broken as they are then,
/// while the points that move keep their shape among themselves.
pub(super) fn orient2d_towards(pts: [(glam::DVec2, bool); 3], center: glam::DVec2) -> i8 {
    let s = orient2d(pts[0].0, pts[1].0, pts[2].0);
    if s != 0 || pts.iter().all(|&(_, moves)| !moves) {
        return s;
    }
    worked_out();
    let coord = |p: f64, c: f64, moves: bool| {
        let p = Exp::lit(p);
        Poly(if moves {
            let d = Exp::lit(c).sub(&p);
            vec![p, d]
        } else {
            vec![p]
        })
    };
    let [a, b, c] =
        pts.map(|(p, moves)| [coord(p.x, center.x, moves), coord(p.y, center.y, moves)]);
    let (bx, by) = (b[0].sub(&a[0]), b[1].sub(&a[1]));
    let (cx, cy) = (c[0].sub(&a[0]), c[1].sub(&a[1]));
    bx.mul(&cy)
        .sub(&by.mul(&cx))
        .0
        .iter()
        .map(Exp::sign)
        .find(|&s| s != 0)
        .unwrap_or(0)
}

thread_local! {
    /// How many signs and ratios this thread has worked out exactly: see
    /// [`exact_count`].
    static EXACT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts one more sign or ratio worked out exactly.
fn worked_out() {
    EXACT.with(|n| n.set(n.get().wrapping_add(1)));
}

/// How many signs and ratios this thread has worked out exactly so far,
/// with expansions: some hundred times what floating point takes, where
/// operands tie (flush faces, a solid against itself, cuts along a line)
/// nearly every time. What it grows by over some work done on one thread
/// from start to end (sharing none out to others, as a primitive or a
/// face's triangulation) is that work's own, however the threads share
/// the work out, so the budget it is charged to stays the same.
pub(crate) fn exact_count() -> usize {
    EXACT.with(std::cell::Cell::get)
}

/// `f`'s value, and how many signs and ratios it worked out exactly (see
/// [`exact_count`]).
pub(super) fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = exact_count();
    let value = f();
    (value, exact_count().wrapping_sub(before))
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]
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

    /// `det[q − p, r − p, UP]`.
    struct Orient3([Pt; 3]);

    impl Pred for Orient3 {
        fn eval<N: Num>(&self) -> N {
            let [p, q, r] = self.0.map(|x| x.v3::<N>());
            det(&sub(&q, &p), &sub(&r, &p), &dir(DVec3::new(2.0, 3.0, 32.0)))
        }
    }

    #[test]
    fn signs_agree_however_small_the_coordinates() {
        // Points stacked along z within 1e-140..1e-170 of each other: the
        // products of their differences underflow, where expansions
        // aren't exact. The same orientation asked in any order must
        // agree.
        let mut rng = Rng::new(5);
        let n = Some(DVec3::new(0.3, 0.5, 0.8).normalize());
        let found = [
            DVec3::new(-6.5512545830042e-163, 2.737149257890908e-162, 1.0),
            DVec3::new(2.1726651218110564e-162, -2.4790985788555624e-163, 1.0),
            DVec3::new(4.2039487036732694e-163, 2.2229505953788046e-162, 1.0),
        ];
        for k in 0..500 {
            let scale = 10f64.powf(-rng.range(140.0, 170.0));
            let mut point = || {
                let x = rng.point(1.0);
                DVec3::new(x.x * scale, x.y * scale, (x.z * 4.0).round())
            };
            let pts = if k == 0 {
                found
            } else {
                [point(), point(), point()]
            };
            for mask in 0..8 {
                let [a, b, c] = [0, 1, 2].map(|i| Pt {
                    p: pts[i],
                    n: if mask >> i & 1 == 1 { n } else { None },
                });
                let s = sign(&Orient3([a, b, c]));
                assert_eq!(s, -sign(&Orient3([b, a, c])), "{pts:?} {mask}");
                assert_eq!(s, sign(&Orient3([b, c, a])), "{pts:?} {mask}");
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

    /// The side of the plane `z = 0` a point is on: its height.
    struct Height(Pt);

    impl Pred for Height {
        fn eval<N: Num>(&self) -> N {
            self.0.v3::<N>()[2].clone()
        }

        fn scale(&self) -> f64 {
            1.0
        }
    }

    #[test]
    fn ties_within_the_tie_go_by_the_perturbation() {
        let up = Some(glam::DVec3::Z);
        let down = Some(-glam::DVec3::Z);
        let at = |z: f64, n| {
            Height(Pt {
                p: DVec3::new(0.3, 0.2, z),
                n,
            })
        };
        // A hair below the plane, moved up: exactly below, a tie within
        // `1e-9`, the perturbation's side.
        assert_eq!(sign(&at(-1e-12, up)), -1);
        assert_eq!(sign_tied(&at(-1e-12, up), 1e-9), 1);
        assert_eq!(sign_tied(&at(1e-12, down), 1e-9), -1);
        // Past the tie, and with none, the value decides.
        assert_eq!(sign_tied(&at(-1e-6, up), 1e-9), -1);
        assert_eq!(sign_tied(&at(-1e-12, up), 0.0), -1);
        // An exact tie either way.
        assert_eq!(sign_tied(&at(0.0, up), 0.0), 1);
        assert_eq!(sign_tied(&at(0.0, down), 1e-9), -1);
        // A point that doesn't move: the tie stays a tie.
        assert_eq!(sign_tied(&at(1e-12, None), 1e-9), 0);
    }
}
