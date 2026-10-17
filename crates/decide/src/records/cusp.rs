//! Sharp-corner records: at a corner of a face where two trimming curves
//! leave their vertex in nearly the same direction (an acute corner, or a
//! curve tangent to another: a hole tangent to an edge, a slot tangent to
//! a wall), the counting reads one certificate of how the two curves come
//! apart ([`CuspRec`]), so the thin **spike** between them is one corner
//! piece of the face, however many bands long it is.
//!
//! **Sharp.** The two curves' tangents at the vertex, carried into space by
//! the surface's partial derivatives there (certified enclosures), are
//! *apart* when their angle is certified above `θ_SHARP = 2⁻⁴` rad: their
//! dot product certified negative, or the square of their cross product
//! certified above `(1/16)²` times the product of their squared lengths
//! (`sin θ_SHARP < 1/16`). Anything else is sharp, which is always sound:
//! a sharp corner of a wide angle only gets a short spike.
//!
//! **The frame.** The record is a certificate on the face's own trims, in
//! its UV plane, exact: each curve is the first rational quadratic span of
//! its p-curve oriented away from the vertex `w`, its control data the
//! stored `f64`s read as rationals. The **axis** `k` is the UV coordinate
//! along which the second curve's tangent is the larger (`u` on a tie), the
//! spike runs towards `σ = ±1` along it, and both tangents must point that
//! way. A curve's **offset** at `s ≥ 0` is its other coordinate minus
//! `w`'s at the point where `σ·(k − w_k) = s`, as long as it is a graph
//! over `k` (its `k` moves monotonically); the **gap** is the difference
//! of the two offsets, signed so that it is positive near `w`.
//!
//! **Contact order and bounds.** Each offset's power series in `s` is
//! formed exactly (the span's coordinates as series in its parameter `t`,
//! `t` as a series in `s` by reversion, composed). The **contact order**
//! `m` is the least order at which the two series differ, `g_m` the
//! difference there (`m = 1` at an acute corner, 2 at a cusp of different
//! curvatures, higher where the curvatures agree); two different conics
//! meet with multiplicity at most 4, so series agreeing up to
//! [`MAX_ORDER`] are one curve ([`CuspError::Contact`]). With `δ = |g_m|/4`
//! each offset stays within `δ·s^m` of its series' terms up to order `m`
//! over a parameter stretch `[0, t_c]`: two polynomial inequalities in `t`
//! (the offsets times `W^m`, divided by `t^m`), certified by the signs of
//! their Bernstein coefficients over `[0, t_c]`, together with the
//! monotonicity of `k` there; `t_c` is the first of `1, ½, ¼, …` (at most
//! [`MAX_HALVINGS`] halvings) that certifies. So over `s ≤ s_c` (the lesser
//! of the two curves' `s` at their `t_c`) the gap lies between `A_lo·s^m`
//! and `A_hi·s^m`, `A_lo = |g_m|/2`, `A_hi = 3|g_m|/2`.
//!
//! **The cut.** The two bands `β_c + β_c′` (in space) are carried into the
//! UV plane by the surface's least stretch at `w` (from the partial
//! derivatives' midpoints, a fixed function of the stored patch): `β_uv`.
//! The cut is an iso line `k = κ`, `κ` a dyadic of the coarsest depth (at
//! most `2⁻⁵²`) whose `s_β = σ·(κ − w_k)` has `A_lo·s_β^m > β_uv`,
//! `s_β ≤ s_c` and `s_β` at most twice the least such value on the finest
//! grid (so the spike is no longer than about twice its threshold): beyond it the curves are band-apart and ordinary strip
//! boxes take over ([`CuspError::Reach`] where `s_c` comes first). Each
//! curve's **reach** is the greatest dyadic parameter (at most `2⁻⁵²` fine)
//! of its first span whose `s` is at most `s_β`: the stretch of the curve
//! that bounds the spike.
//!
//! The record is a function of the stored trims, the patch at `w` and the
//! bands: built once per operation, never perturbed, the same bits on
//! every run.

use std::cmp::Ordering;

use malachite::base::num::arithmetic::traits::{Abs, Floor, Pow, PowerOf2, Sign};
use malachite::base::num::basic::traits::{One, Zero};
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode;
use malachite::{Integer, Rational};

use crate::bezier::{Dir, Dyadic, Scalar};
use crate::num::bounded::Bounded;

/// The highest contact order the record looks for: two different conics
/// meet with multiplicity at most 4.
pub const MAX_ORDER: usize = 5;

/// The most halvings of a span's parameter stretch certifying the bounds.
pub const MAX_HALVINGS: u32 = 40;

/// The finest cut and reach: `2⁻⁵²`.
pub const CUT_DEPTH: u32 = 52;

/// A trimming curve's span: three affine control points with their
/// weights, `(u, v, w)`.
pub type CuspSpan = [[f64; 3]; 3];

/// Why a corner has no record: it isn't one the record covers, and the
/// counting treats it as any corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CuspError {
    /// A span's values aren't finite, a weight isn't positive, or the two
    /// spans don't start at one UV point.
    Span,
    /// A tangent at the vertex is zero, or the surface's stretch there
    /// isn't positive.
    Degenerate,
    /// The tangents don't both point along one axis direction: not graphs
    /// over it.
    Graph,
    /// The curves agree to [`MAX_ORDER`]: one curve (a slit).
    Contact,
    /// The bounds don't certify within [`MAX_HALVINGS`], or no cut lies
    /// within the stretch they hold over.
    Reach,
}

/// A sharp corner's certificate (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CuspRec {
    /// The contact order `m ≥ 1`.
    pub m: u32,
    /// `A_lo`: the gap is at least `A_lo·s^m` for `s ≤ s_c`.
    pub a_lo: Rational,
    /// `A_hi`: the gap is at most `A_hi·s^m` for `s ≤ s_c`.
    pub a_hi: Rational,
    /// The axis the curves are graphs over.
    pub axis: Dir,
    /// The direction along it the spike runs, `±1`.
    pub toward: i8,
    /// How far the bounds hold.
    pub s_c: Rational,
    /// The cut's distance from the vertex along the axis.
    pub s_beta: Rational,
    /// The cut's global coordinate `κ` on the axis: a tile index and the
    /// local dyadic in `[0, 1)`.
    pub cut: (i64, Dyadic),
    /// Per curve (the first given, then the second), the greatest dyadic
    /// parameter of its first span from the vertex whose `s` is at most
    /// `s_β`.
    pub reach: [Dyadic; 2],
}

/// Whether a corner is sharp, and its record where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sharp {
    /// The tangents are certified more than `θ_SHARP` apart.
    Apart,
    /// Sharp, with its record.
    Cusp(Box<CuspRec>),
}

/// The record of the corner at `w` between the curves whose first spans
/// from `w` are `a` (the curve into `w`, run backwards) and `b` (the curve
/// out of `w`), on a surface whose partial derivatives at `w` are `du` and
/// `dv` (certified enclosures), with the two curves' bands summed in
/// `bands` (module docs).
pub fn cusp_rec(
    a: &CuspSpan,
    b: &CuspSpan,
    du: &[Bounded<f64>; 3],
    dv: &[Bounded<f64>; 3],
    bands: f64,
) -> Result<Sharp, CuspError> {
    let ok = |s: &CuspSpan| {
        s.iter()
            .all(|p| p.iter().all(|x| x.is_finite()) && p[2] > 0.0)
    };
    if !ok(a) || !ok(b) || !(bands.is_finite() && bands >= 0.0) {
        return Err(CuspError::Span);
    }
    if a[0][0] != b[0][0] || a[0][1] != b[0][1] {
        return Err(CuspError::Span);
    }
    let q = |x: f64| Rational::try_from(x).expect("finite");
    let tangent = |s: &CuspSpan| [q(s[1][0]) - q(s[0][0]), q(s[1][1]) - q(s[0][1])];
    let (ta, tb) = (tangent(a), tangent(b));
    let zero = |t: &[Rational; 2]| t[0] == 0 && t[1] == 0;
    if zero(&ta) || zero(&tb) {
        return Err(CuspError::Degenerate);
    }
    if apart(a, b, du, dv) {
        return Ok(Sharp::Apart);
    }
    // The axis: where the second tangent is the larger, `u` on a tie.
    let k = usize::from((&tb[1]).abs() > (&tb[0]).abs());
    let toward = Sign::sign(&tb[k]);
    if Sign::sign(&ta[k]) != toward || toward == Ordering::Equal {
        return Err(CuspError::Graph);
    }
    let sigma: i8 = if toward == Ordering::Greater { 1 } else { -1 };
    let curves = [Curve::new(a, k, sigma), Curve::new(b, k, sigma)];
    let series = [curves[0].offsets()?, curves[1].offsets()?];
    let (m, gm) = (1..=MAX_ORDER)
        .find_map(|j| {
            let g = &series[1][j] - &series[0][j];
            (g != 0).then_some((j, g))
        })
        .ok_or(CuspError::Contact)?;
    let g_abs = (&gm).abs();
    let delta = &g_abs / Rational::from(4);
    let mut ends = Vec::with_capacity(2);
    for (c, y) in curves.iter().zip(&series) {
        ends.push(c.stretch(m, &y[..m], &y[m], &delta)?);
    }
    let s_c = if ends[0].1 < ends[1].1 {
        ends[0].1.clone()
    } else {
        ends[1].1.clone()
    };
    let a_lo = &g_abs / Rational::from(2);
    let a_hi = &g_abs * Rational::from(3) / Rational::from(2);
    let beta = band_uv(du, dv, bands)?;
    let (x, s_beta) = cut(&q(a[0][k]), sigma, m, &a_lo, &beta, &s_c)?;
    let reach = [0, 1].map(|i| curves[i].reach(&s_beta, ends[i].0));
    // Fits: `|κ| < 2⁵³`, a tile index.
    let kappa = if sigma > 0 { x } else { -x };
    let tile = (&kappa).floor();
    let local = &kappa - Rational::from(tile.clone());
    let tile = i64::try_from(&tile).map_err(|_| CuspError::Reach)?;
    let cut = (tile, dyadic_of(&local).ok_or(CuspError::Reach)?);
    Ok(Sharp::Cusp(Box::new(CuspRec {
        // Fits: at most `MAX_ORDER`.
        m: m as u32,
        a_lo,
        a_hi,
        axis: if k == 0 { Dir::U } else { Dir::V },
        toward: sigma,
        s_c,
        s_beta,
        cut,
        reach,
    })))
}

/// Whether the tangents of `a` and `b` at their start, carried by the
/// partial derivatives, are certified more than `θ_SHARP` apart (module
/// docs).
fn apart(a: &CuspSpan, b: &CuspSpan, du: &[Bounded<f64>; 3], dv: &[Bounded<f64>; 3]) -> bool {
    let e = Bounded::exact;
    let space = |s: &CuspSpan| {
        let (x, y) = (e(s[1][0]) - e(s[0][0]), e(s[1][1]) - e(s[0][1]));
        [0, 1, 2].map(|i| du[i] * x + dv[i] * y)
    };
    let (ta, tb) = (space(a), space(b));
    let dot =
        |p: &[Bounded<f64>; 3], r: &[Bounded<f64>; 3]| p[0] * r[0] + p[1] * r[1] + p[2] * r[2];
    let d = dot(&ta, &tb);
    if d.enclose()[1] < 0.0 {
        return true;
    }
    let cross = [
        ta[1] * tb[2] - ta[2] * tb[1],
        ta[2] * tb[0] - ta[0] * tb[2],
        ta[0] * tb[1] - ta[1] * tb[0],
    ];
    let c2 = dot(&cross, &cross);
    let lens = dot(&ta, &ta) * dot(&tb, &tb) * e(1.0 / 256.0);
    (c2 - lens).enclose()[0] > 0.0
}

/// The two bands carried into the UV plane by the surface's least stretch
/// at the vertex (module docs).
fn band_uv(
    du: &[Bounded<f64>; 3],
    dv: &[Bounded<f64>; 3],
    bands: f64,
) -> Result<Rational, CuspError> {
    let mid = |x: &[Bounded<f64>; 3]| {
        x.map(|c| {
            let [lo, hi] = c.enclose();
            lo * 0.5 + hi * 0.5
        })
    };
    let (p, r) = (mid(du), mid(dv));
    let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let (e, f, g) = (dot(p, p), dot(p, r), dot(r, r));
    let root = ((e - g) * (e - g) + 4.0 * f * f).sqrt();
    let least = ((e + g) - root) * 0.5;
    if !(least.is_finite() && least > 0.0) {
        return Err(CuspError::Degenerate);
    }
    let beta = bands / least.sqrt();
    if !beta.is_finite() {
        return Err(CuspError::Degenerate);
    }
    Ok(Rational::try_from(beta).expect("finite"))
}

/// The point `num/2^exp` of `[0, 1)` that `x` is, if `exp ≤` [`CUT_DEPTH`].
fn dyadic_of(x: &Rational) -> Option<Dyadic> {
    let scaled = x * Rational::power_of_2(i64::from(CUT_DEPTH));
    let n = (&scaled).floor();
    if scaled != n {
        return None;
    }
    Dyadic::new(u64::try_from(&n).ok()?, CUT_DEPTH)
}

/// The cut (module docs) in the mirrored coordinate `x = σ·κ`: the
/// coarsest dyadic `x` past `σ·w_k` whose `s = x − σ·w_k` has
/// `A_lo·s^m > β`, `s ≤ s_c` and `s` at most twice the least such `s` on
/// the finest grid, with that `s`.
fn cut(
    wk: &Rational,
    sigma: i8,
    m: usize,
    a_lo: &Rational,
    beta: &Rational,
    s_c: &Rational,
) -> Result<(Rational, Rational), CuspError> {
    let w = if sigma > 0 { wk.clone() } else { -wk };
    let fine = Rational::power_of_2(-i64::from(CUT_DEPTH));
    let s_of = |n: &Integer, h: &Rational| Rational::from(n.clone()) * h - &w;
    let ok = |s: &Rational| *s > 0 && a_lo * s.pow(m as u64) > *beta;
    // The finest grid: the least `n` whose point is past the threshold,
    // by bisection between the point at `w` and the one at `s_c`.
    let mut lo = (&w / &fine).floor();
    let mut hi = ((&w + s_c) / &fine).floor();
    if !ok(&s_of(&hi, &fine)) {
        return Err(CuspError::Reach);
    }
    // `lo` fails (`s ≤ 0`), `hi` holds.
    while &hi - &lo > 1u32 {
        let mid: Integer = (&lo + &hi) >> 1u32;
        if ok(&s_of(&mid, &fine)) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let least = Rational::from(hi) * &fine;
    let twice = (&least - &w) * Rational::from(2);
    // The coarsest grid holding a point past the threshold within `s_c`
    // and within twice the threshold.
    for d in 0..=CUT_DEPTH {
        let h = Rational::power_of_2(-i64::from(d));
        let n = (&least / &h).ceiling_int();
        let x = Rational::from(n) * &h;
        let s = &x - &w;
        if s <= *s_c && s <= twice {
            return Ok((x, s));
        }
    }
    Err(CuspError::Reach)
}

/// The least integer at or above.
trait CeilingInt {
    fn ceiling_int(&self) -> Integer;
}

impl CeilingInt for Rational {
    fn ceiling_int(&self) -> Integer {
        -(-self).floor()
    }
}

/// A polynomial in power form, exact.
pub(crate) type Poly = Vec<Rational>;

fn padd(a: &[Rational], b: &[Rational]) -> Poly {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| {
            let x = a.get(i).cloned().unwrap_or(Rational::ZERO);
            x + b.get(i).cloned().unwrap_or(Rational::ZERO)
        })
        .collect()
}

fn pscale(a: &[Rational], k: &Rational) -> Poly {
    a.iter().map(|x| x * k).collect()
}

fn pmul(a: &[Rational], b: &[Rational]) -> Poly {
    let mut out = vec![Rational::ZERO; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

fn ppow(a: &[Rational], n: usize) -> Poly {
    let mut out = vec![Rational::ONE];
    for _ in 0..n {
        out = pmul(&out, a);
    }
    out
}

pub(crate) fn peval(a: &[Rational], t: &Rational) -> Rational {
    a.iter().rev().fold(Rational::ZERO, |acc, c| acc * t + c)
}

pub(crate) fn pderiv(a: &[Rational]) -> Poly {
    if a.len() <= 1 {
        return vec![Rational::ZERO];
    }
    a.iter()
        .enumerate()
        .skip(1)
        .map(|(i, c)| c * Rational::from(i as u64))
        .collect()
}

/// Whether every Bernstein coefficient of `p` over `[0, t]` is positive:
/// then `p > 0` there.
fn positive_on(p: &[Rational], t: &Rational) -> bool {
    let n = p.len() - 1;
    // `p(t·x)` in power form, then its Bernstein coefficients on `[0, 1]`:
    // `b_i = Σ_{j ≤ i} C(i, j)/C(n, j)·a_j`.
    let mut tj = Rational::ONE;
    let mut a = Vec::with_capacity(n + 1);
    for c in p {
        a.push(c * &tj);
        tj *= t;
    }
    let binom = |n: usize, k: usize| -> Rational {
        let mut r = Rational::ONE;
        for i in 0..k {
            r = r * Rational::from((n - i) as u64) / Rational::from((i + 1) as u64);
        }
        r
    };
    (0..=n).all(|i| {
        let b = (0..=i).fold(Rational::ZERO, |acc, j| {
            acc + binom(i, j) / binom(n, j) * &a[j]
        });
        b > 0
    })
}

/// Truncated power series to order [`MAX_ORDER`].
type Series = Vec<Rational>;

fn strunc(mut a: Series) -> Series {
    a.resize(MAX_ORDER + 1, Rational::ZERO);
    a
}

fn smul(a: &[Rational], b: &[Rational]) -> Series {
    let mut out = vec![Rational::ZERO; MAX_ORDER + 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            if i + j <= MAX_ORDER {
                out[i + j] += x * y;
            }
        }
    }
    out
}

/// `a / b`, `b[0] ≠ 0`.
pub(crate) fn sdiv(a: &[Rational], b: &[Rational]) -> Series {
    let (a, b) = (strunc(a.to_vec()), strunc(b.to_vec()));
    let mut out = vec![Rational::ZERO; MAX_ORDER + 1];
    for i in 0..=MAX_ORDER {
        let mut c = a[i].clone();
        for j in 1..=i {
            c -= &b[j] * &out[i - j];
        }
        out[i] = c / &b[0];
    }
    out
}

/// `Σ a_j·x^j` for a series `x` without a constant term.
pub(crate) fn scompose(a: &[Rational], x: &[Rational]) -> Series {
    let mut out = vec![Rational::ZERO; MAX_ORDER + 1];
    for c in a.iter().rev() {
        out = smul(&out, x);
        out[0] += c;
    }
    out
}

/// The inverse of a series `s` with `s[0] = 0`, `s[1] ≠ 0`.
pub(crate) fn srevert(s: &[Rational]) -> Series {
    let s1 = s[1].clone();
    // `τ ← (x − (s(τ) − s₁·τ))/s₁`, one more order exact per round.
    let x: Series = strunc(vec![Rational::ZERO, Rational::ONE]);
    let mut rest = s.to_vec();
    rest[1] = Rational::ZERO;
    let mut tau: Series = strunc(vec![Rational::ZERO, Rational::ONE / &s1]);
    for _ in 0..MAX_ORDER {
        let r = scompose(&rest, &tau);
        tau = (0..=MAX_ORDER).map(|i| (&x[i] - &r[i]) / &s1).collect();
    }
    tau
}

/// One curve in the record's frame: from its span, the numerators of
/// `s` and of the offset over the weight, in power form in `t`.
struct Curve {
    s: Poly,
    y: Poly,
    w: Poly,
}

impl Curve {
    fn new(sp: &CuspSpan, k: usize, sigma: i8) -> Curve {
        let q = |x: f64| Rational::try_from(x).expect("finite");
        let o = 1 - k;
        let ws = [q(sp[0][2]), q(sp[1][2]), q(sp[2][2])];
        // `Σ wᵢ·xᵢ·Bᵢ(t)` in power form.
        let hom = |x: [Rational; 3]| -> Poly {
            let (a, b, c) = (&ws[0] * &x[0], &ws[1] * &x[1], &ws[2] * &x[2]);
            vec![
                a.clone(),
                Rational::from(2) * (&b - &a),
                &a - Rational::from(2) * &b + &c,
            ]
        };
        let w = hom([Rational::ONE, Rational::ONE, Rational::ONE]);
        let (wk, wo) = (q(sp[0][k]), q(sp[0][o]));
        let xk = hom([q(sp[0][k]), q(sp[1][k]), q(sp[2][k])]);
        let xo = hom([q(sp[0][o]), q(sp[1][o]), q(sp[2][o])]);
        let sig = Rational::from(i64::from(sigma));
        let s = pscale(&padd(&xk, &pscale(&w, &-&wk)), &sig);
        let y = padd(&xo, &pscale(&w, &-&wo));
        Curve { s, y, w }
    }

    /// The offset's series in `s` (module docs).
    fn offsets(&self) -> Result<Series, CuspError> {
        let s = sdiv(&self.s, &self.w);
        if s[1] <= 0 {
            return Err(CuspError::Graph);
        }
        let y = sdiv(&self.y, &self.w);
        Ok(scompose(&y, &srevert(&s)))
    }

    /// The stretch `[0, t_c]` over which the offset stays within `δ·s^m`
    /// of `Σ_{j < m} p_j s^j + c·s^m` with `k` monotone, and the `s` at
    /// `t_c` (module docs).
    fn stretch(
        &self,
        m: usize,
        p: &[Rational],
        c: &Rational,
        delta: &Rational,
    ) -> Result<(Dyadic, Rational), CuspError> {
        // `Y·W^(m−1) − Σ p_j S^j W^(m−j) − (c ∓ δ) S^m`, over `t^m`.
        let mut base = pmul(&self.y, &ppow(&self.w, m - 1));
        for (j, pj) in p.iter().enumerate() {
            let term = pmul(&ppow(&self.s, j), &ppow(&self.w, m - j));
            base = padd(&base, &pscale(&term, &-pj));
        }
        let sm = ppow(&self.s, m);
        let over_tm = |f: Poly| -> Result<Poly, CuspError> {
            if f[..m].iter().any(|x| *x != 0) {
                return Err(CuspError::Contact);
            }
            Ok(f[m..].to_vec())
        };
        let lower = over_tm(padd(&base, &pscale(&sm, &-(c - delta))))?;
        let upper = over_tm(padd(&base, &pscale(&sm, &-(c + delta))))?;
        let neg_upper: Poly = upper.iter().map(|x| -x).collect();
        let mono = padd(
            &pmul(&pderiv(&self.s), &self.w),
            &pscale(&pmul(&self.s, &pderiv(&self.w)), &Rational::from(-1)),
        );
        for j in 0..=MAX_HALVINGS {
            let t = Rational::power_of_2(-i64::from(j));
            if positive_on(&lower, &t) && positive_on(&neg_upper, &t) && positive_on(&mono, &t) {
                let s = peval(&self.s, &t) / peval(&self.w, &t);
                let d = Dyadic::new(1, j).ok_or(CuspError::Reach)?;
                return Ok((d, s));
            }
        }
        Err(CuspError::Reach)
    }

    /// The greatest dyadic parameter in `[0, t_c]`, at most `2⁻⁵²` fine,
    /// whose `s` is at most `s_β` (`s` increases over the stretch).
    fn reach(&self, s_beta: &Rational, t_c: Dyadic) -> Dyadic {
        let within = |n: u64, e: u32| {
            let t = Rational::from(n) / Rational::power_of_2(i64::from(e));
            peval(&self.s, &t) - s_beta * peval(&self.w, &t) <= 0
        };
        if within(t_c.num(), t_c.exp()) {
            return t_c;
        }
        // Bisect on the grid `2^-CUT_DEPTH`: `lo` holds, `hi` doesn't.
        let shift = CUT_DEPTH - t_c.exp();
        let (mut lo, mut hi) = (0u64, t_c.num() << shift);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if within(mid, CUT_DEPTH) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Dyadic::new(lo, CUT_DEPTH).expect("within [0, 1]")
    }
}

impl CuspRec {
    /// `s_β` rounded to the nearest `f64`, for reports.
    pub fn s_beta_f64(&self) -> f64 {
        f64::rounding_from(&self.s_beta, RoundingMode::Nearest).0
    }

    /// `s_c` rounded to the nearest `f64`, for reports.
    pub fn s_c_f64(&self) -> f64 {
        f64::rounding_from(&self.s_c, RoundingMode::Nearest).0
    }
}

#[cfg(test)]
mod tests;
