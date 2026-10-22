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
//! its UV plane, exact: each curve is its p-curve's chain of rational
//! quadratic spans read from the vertex `w` (each span oriented away from
//! it, at most [`MAX_SPANS`]; a curve broken at knots, a tile line among
//! them, reads on across them), its control data the stored `f64`s read
//! as rationals. The **axis** `k` is the UV coordinate along which the
//! second curve's tangent is the larger (`u` on a tie), the spike runs
//! towards `σ = ±1` along it, and both tangents must point that way. A
//! curve's **offset** at `s ≥ 0` is its other coordinate minus `w`'s at
//! the point where `σ·(k − w_k) = s`, as long as it is a graph over `k`
//! (its `k` moves monotonically); the **gap** is the difference of the
//! two offsets.
//!
//! **Orders and bounds.** Each offset's power series in `s` is formed
//! exactly from the first span (its coordinates as series in its
//! parameter `t`, `t` as a series in `s` by reversion, composed); `g_j`
//! is the two series' difference at order `j`. The **contact order** is
//! the least `j` with `g_j ≠ 0` (1 at an acute corner, 2 at a cusp of
//! different curvatures, higher where the curvatures agree); two different
//! conics meet with multiplicity at most 4, so series agreeing up to
//! [`MAX_ORDER`] are one curve ([`CuspError::Contact`]). The record takes
//! the least order `m` with `g_m ≠ 0` whose bounds reach a cut: the
//! contact order, or a higher one where the contact order's term is so
//! small that its bounds hold only next to the vertex (a tangency the
//! stored data's rounding has turned into a tiny angle). With
//! `δ = |g_m|/4` each offset stays within `δ·s^m` of its series' terms up
//! to order `m` over a **stretch** of its chain: two polynomial
//! inequalities in each span's parameter (the offsets times `W^m`, on the
//! first span divided by `t^m`), certified by the signs of their
//! Bernstein coefficients, together with the monotonicity of `k`; the
//! stretch holds each whole span while it certifies, then the first of
//! `1, ½, ¼, …` (at most [`MAX_HALVINGS`] halvings) of the next span that
//! does. So over `s ≤ s_c` (the lesser of the two curves' `s` at their
//! stretch's end), with the gap signed as its order-`m` term and `l_j` the
//! lower orders' terms signed so, the gap lies between
//! `Σ_{j<m} l_j·s^j + A_lo·s^m` and `Σ_{j<m} l_j·s^j + A_hi·s^m`,
//! `A_lo = |g_m|/2`, `A_hi = 3|g_m|/2` (at the contact order every `l_j`
//! is zero).
//!
//! **The cut.** The two bands `β_c + β_c′` (in space) are carried into the
//! UV plane by the surface's least stretch at `w` (from the partial
//! derivatives' midpoints, a fixed function of the stored patch): `β_uv`.
//! The cut is an iso line `k = κ`, `κ` a dyadic of the coarsest depth (at
//! most `2⁻⁵²`) whose `s_β = σ·(κ − w_k)` has
//! `A_lo·s_β^m + Σ_{l_j < 0} l_j·s_β^j > β_uv` (the lower bound with the
//! helping terms dropped, which grows with `s` once positive, so the
//! curves stay band-apart from the cut to `s_c`), `s_β ≤ s_c` and `s_β`
//! at most twice the least such value on the finest grid (so the spike is
//! no longer than about twice its threshold): beyond it the curves are
//! band-apart and ordinary strip boxes take over ([`CuspError::Reach`]
//! where no order's `s_c` reaches a cut). Each curve's **reach** is the
//! greatest dyadic point of its stretch (a span and a parameter at most
//! `2⁻⁵²` fine) whose `s` is at most `s_β`: the stretch of the curve that
//! bounds the spike.
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
use crate::num::exact::rat;
use crate::num::poly;

/// The highest contact order the record looks for: two different conics
/// meet with multiplicity at most 4.
pub const MAX_ORDER: usize = 5;

/// The most halvings of a span's parameter stretch certifying the bounds.
pub const MAX_HALVINGS: u32 = 40;

/// The finest cut and reach: `2⁻⁵²`.
pub const CUT_DEPTH: u32 = 52;

/// The most spans of each curve from the vertex the record reads.
pub const MAX_SPANS: usize = 64;

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
    /// The order `m ≥ 1` whose term bounds the gap.
    pub m: u32,
    /// The gap's terms of the orders below `m`, `l_j` for `j < m`, signed
    /// as its order-`m` term (all zero at the contact order).
    pub lower: Vec<Rational>,
    /// `A_lo`: the gap is at least `Σ l_j·s^j + A_lo·s^m` for `s ≤ s_c`.
    pub a_lo: Rational,
    /// `A_hi`: the gap is at most `Σ l_j·s^j + A_hi·s^m` for `s ≤ s_c`.
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
    /// point of its chain whose `s` is at most `s_β`: the span (from the
    /// vertex) and the parameter in it, both counted from the vertex.
    pub reach: [(u32, Dyadic); 2],
}

/// Whether a corner is sharp, and its record where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sharp {
    /// The tangents are certified more than `θ_SHARP` apart.
    Apart,
    /// Sharp, with its record.
    Cusp(Box<CuspRec>),
}

/// The record of the corner at `w` between the curves whose spans from
/// `w` are `a` (the curve into `w`, run backwards) and `b` (the curve out
/// of `w`), each a chain oriented away from `w`, on a surface whose
/// partial derivatives at `w` are `du` and `dv` (certified enclosures),
/// with the two curves' bands summed in `bands` (module docs).
pub fn cusp_rec(
    a: &[CuspSpan],
    b: &[CuspSpan],
    du: &[Bounded<f64>; 3],
    dv: &[Bounded<f64>; 3],
    bands: f64,
) -> Result<Sharp, CuspError> {
    cusp_rec_counted(a, b, du, dv, bands).0
}

/// [`cusp_rec`], with the spans whose bounds it tested (each a few
/// exact polynomial tests), for the caller to charge.
pub fn cusp_rec_counted(
    a: &[CuspSpan],
    b: &[CuspSpan],
    du: &[Bounded<f64>; 3],
    dv: &[Bounded<f64>; 3],
    bands: f64,
) -> (Result<Sharp, CuspError>, u32) {
    let mut tested = 0u32;
    let r = record(a, b, du, dv, bands, &mut tested);
    (r, tested)
}

fn record(
    a: &[CuspSpan],
    b: &[CuspSpan],
    du: &[Bounded<f64>; 3],
    dv: &[Bounded<f64>; 3],
    bands: f64,
    tested: &mut u32,
) -> Result<Sharp, CuspError> {
    let ok = |s: &CuspSpan| {
        s.iter()
            .all(|p| p.iter().all(|x| x.is_finite()) && p[2] > 0.0)
    };
    // Chains: non-empty, finite, positive weights, each span starting
    // where the one before ends.
    let chain = |c: &[CuspSpan]| {
        !c.is_empty()
            && c.iter().all(ok)
            && c.windows(2)
                .all(|w| w[0][2][0] == w[1][0][0] && w[0][2][1] == w[1][0][1])
    };
    if !chain(a) || !chain(b) || !(bands.is_finite() && bands >= 0.0) {
        return Err(CuspError::Span);
    }
    let (a, b) = (&a[..a.len().min(MAX_SPANS)], &b[..b.len().min(MAX_SPANS)]);
    let (a0, b0) = (&a[0], &b[0]);
    if a0[0][0] != b0[0][0] || a0[0][1] != b0[0][1] {
        return Err(CuspError::Span);
    }
    let tangent = |s: &CuspSpan| [rat(s[1][0]) - rat(s[0][0]), rat(s[1][1]) - rat(s[0][1])];
    let (ta, tb) = (tangent(a0), tangent(b0));
    let zero = |t: &[Rational; 2]| t[0] == 0 && t[1] == 0;
    if zero(&ta) || zero(&tb) {
        return Err(CuspError::Degenerate);
    }
    if apart(a0, b0, du, dv) {
        return Ok(Sharp::Apart);
    }
    // The axis: where the second tangent is the larger, `u` on a tie.
    let k = usize::from((&tb[1]).abs() > (&tb[0]).abs());
    let toward = Sign::sign(&tb[k]);
    if Sign::sign(&ta[k]) != toward || toward == Ordering::Equal {
        return Err(CuspError::Graph);
    }
    let sigma: i8 = if toward == Ordering::Greater { 1 } else { -1 };
    let w = [rat(a0[0][0]), rat(a0[0][1])];
    let curves = [Chain::new(a, &w, k, sigma), Chain::new(b, &w, k, sigma)];
    let series = [curves[0].offsets()?, curves[1].offsets()?];
    let g: Vec<Rational> = (0..=MAX_ORDER)
        .map(|j| &series[1][j] - &series[0][j])
        .collect();
    if g.iter().all(|x| *x == 0) {
        return Err(CuspError::Contact);
    }
    let beta = band_uv(du, dv, bands)?;
    // The least order whose bounds hold far enough to place a cut.
    for m in 1..=MAX_ORDER {
        if g[m] == 0 {
            continue;
        }
        let g_abs = (&g[m]).abs();
        let delta = &g_abs / Rational::from(4);
        let mut ends = Vec::with_capacity(2);
        for (c, y) in curves.iter().zip(&series) {
            ends.push(c.stretch(m, &y[..m], &y[m], &delta, tested));
        }
        let [Some(ea), Some(eb)] = [ends[0].take(), ends[1].take()] else {
            continue;
        };
        let s_c = if ea.2 < eb.2 {
            ea.2.clone()
        } else {
            eb.2.clone()
        };
        // The lower orders' terms, signed by the order's own.
        let lower: Vec<Rational> = g[..m]
            .iter()
            .map(|x| if g[m] > 0 { x.clone() } else { -x })
            .collect();
        let a_lo = &g_abs / Rational::from(2);
        let a_hi = &g_abs * Rational::from(3) / Rational::from(2);
        let Ok((x, s_beta)) = cut(&w[k], sigma, m, &lower, &a_lo, &beta, &s_c) else {
            continue;
        };
        let reach = [(0, &ea), (1, &eb)].map(|(i, e)| curves[i].reach(&s_beta, (e.0, e.1)));
        // Fits: `|κ| < 2⁵³`, a tile index.
        let kappa = if sigma > 0 { x } else { -x };
        let tile = (&kappa).floor();
        let local = &kappa - Rational::from(tile.clone());
        let tile = i64::try_from(&tile).map_err(|_| CuspError::Reach)?;
        let cut = (tile, dyadic_of(&local).ok_or(CuspError::Reach)?);
        return Ok(Sharp::Cusp(Box::new(CuspRec {
            // Fits: at most `MAX_ORDER`.
            m: m as u32,
            lower,
            a_lo,
            a_hi,
            axis: if k == 0 { Dir::U } else { Dir::V },
            toward: sigma,
            s_c,
            s_beta,
            cut,
            reach,
        })));
    }
    Err(CuspError::Reach)
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
/// `A_lo·s^m + Σ_{j<m} min(l_j, 0)·s^j > β` (`l` the lower orders'
/// signed terms), `s ≤ s_c` and `s` at most twice the least such `s` on
/// the finest grid, with that `s`. Past a point where it holds it holds
/// at every greater `s`: `s^m·(A_lo − Σ |l_j|·s^(j−m))` grows with `s`
/// once positive.
fn cut(
    wk: &Rational,
    sigma: i8,
    m: usize,
    lower: &[Rational],
    a_lo: &Rational,
    beta: &Rational,
    s_c: &Rational,
) -> Result<(Rational, Rational), CuspError> {
    let w = if sigma > 0 { wk.clone() } else { -wk };
    let fine = Rational::power_of_2(-i64::from(CUT_DEPTH));
    let s_of = |n: &Integer, h: &Rational| Rational::from(n.clone()) * h - &w;
    let ok = |s: &Rational| {
        if *s <= 0 {
            return false;
        }
        let mut v = a_lo * s.pow(m as u64);
        for (j, l) in lower.iter().enumerate() {
            if *l < 0 {
                v += l * s.pow(j as u64);
            }
        }
        v > *beta
    };
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

fn pderiv(a: &[Rational]) -> Poly {
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
    // `p(t·x)` in power form, then its Bernstein coefficients on `[0, 1]`
    // of `p`'s length's degree.
    let mut tj = Rational::ONE;
    let mut a = Vec::with_capacity(n + 1);
    for c in p {
        a.push(c * &tj);
        tj *= t;
    }
    poly::to_bernstein(&a, n + 1).iter().all(|b| *b > 0)
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

/// One span in the record's frame: the numerators of `s` and of the
/// offset over the weight, in power form in its parameter `t`.
struct Curve {
    s: Poly,
    y: Poly,
    w: Poly,
}

impl Curve {
    fn new(sp: &CuspSpan, w0: &[Rational; 2], k: usize, sigma: i8) -> Curve {
        let o = 1 - k;
        let ws = [rat(sp[0][2]), rat(sp[1][2]), rat(sp[2][2])];
        // `Σ wᵢ·xᵢ·Bᵢ(t)` in power form, of degree 2.
        let hom = |x: [Rational; 3]| -> Poly {
            poly::from_bernstein_full(&[&ws[0] * &x[0], &ws[1] * &x[1], &ws[2] * &x[2]])
        };
        let w = hom([Rational::ONE, Rational::ONE, Rational::ONE]);
        let xk = hom([rat(sp[0][k]), rat(sp[1][k]), rat(sp[2][k])]);
        let xo = hom([rat(sp[0][o]), rat(sp[1][o]), rat(sp[2][o])]);
        let sig = Rational::from(i64::from(sigma));
        let s = pscale(&padd(&xk, &pscale(&w, &-&w0[k])), &sig);
        let y = padd(&xo, &pscale(&w, &-&w0[o]));
        Curve { s, y, w }
    }

    /// `s` at the parameter `t`.
    fn s_at(&self, t: &Rational) -> Rational {
        poly::eval(&self.s, t) / poly::eval(&self.w, t)
    }
}

/// A curve's chain of spans from the vertex in the record's frame.
struct Chain(Vec<Curve>);

impl Chain {
    fn new(spans: &[CuspSpan], w0: &[Rational; 2], k: usize, sigma: i8) -> Chain {
        Chain(
            spans
                .iter()
                .map(|sp| Curve::new(sp, w0, k, sigma))
                .collect(),
        )
    }

    /// The offset's series in `s` at the vertex, from the first span
    /// (module docs).
    fn offsets(&self) -> Result<Series, CuspError> {
        let c = &self.0[0];
        let s = sdiv(&c.s, &c.w);
        if s[1] <= 0 {
            return Err(CuspError::Graph);
        }
        let y = sdiv(&c.y, &c.w);
        Ok(scompose(&y, &srevert(&s)))
    }

    /// The stretch over which the offset stays within `δ·s^m` of
    /// `Σ_{j < m} p_j s^j + c·s^m` with `k` increasing (module docs): the
    /// last span it reaches into, the parameter `t_c` there, and the `s`
    /// at it; `None` where it doesn't hold on any stretch of the first
    /// span. Each span tested counts in `tested`.
    fn stretch(
        &self,
        m: usize,
        p: &[Rational],
        c: &Rational,
        delta: &Rational,
        tested: &mut u32,
    ) -> Option<(usize, Dyadic, Rational)> {
        let mut last = None;
        for (j, cv) in self.0.iter().enumerate() {
            *tested = tested.saturating_add(1);
            // `Y·W^(m−1) − Σ p_j S^j W^(m−j) − (c ∓ δ) S^m`; on the first
            // span over `t^m` (it vanishes to that order at the vertex).
            let mut base = pmul(&cv.y, &ppow(&cv.w, m - 1));
            for (i, pi) in p.iter().enumerate() {
                let term = pmul(&ppow(&cv.s, i), &ppow(&cv.w, m - i));
                base = padd(&base, &pscale(&term, &-pi));
            }
            let sm = ppow(&cv.s, m);
            let over_tm = |f: Poly| -> Option<Poly> {
                if j > 0 {
                    return Some(f);
                }
                if f[..m].iter().any(|x| *x != 0) {
                    return None;
                }
                Some(f[m..].to_vec())
            };
            let lower = over_tm(padd(&base, &pscale(&sm, &-(c - delta))))?;
            let upper = over_tm(padd(&base, &pscale(&sm, &-(c + delta))))?;
            let neg_upper: Poly = upper.iter().map(|x| -x).collect();
            let mono = padd(
                &pmul(&pderiv(&cv.s), &cv.w),
                &pscale(&pmul(&cv.s, &pderiv(&cv.w)), &Rational::from(-1)),
            );
            let held = (0..=MAX_HALVINGS).find_map(|h| {
                let t = Rational::power_of_2(-i64::from(h));
                (positive_on(&lower, &t) && positive_on(&neg_upper, &t) && positive_on(&mono, &t))
                    .then(|| (Dyadic::new(1, h), t))
            });
            match held {
                Some((Some(d), t)) => {
                    last = Some((j, d, cv.s_at(&t)));
                    if d != Dyadic::ONE {
                        break;
                    }
                }
                _ => break,
            }
        }
        last
    }

    /// The greatest dyadic point (a span and a parameter at most `2⁻⁵²`
    /// fine) within the stretch ending at `(j_c, t_c)` whose `s` is at
    /// most `s_β` (`s` increases over the stretch).
    fn reach(&self, s_beta: &Rational, (jc, tc): (usize, Dyadic)) -> (u32, Dyadic) {
        for (j, cv) in self.0.iter().enumerate().take(jc + 1) {
            let t_end = if j == jc { tc } else { Dyadic::ONE };
            let within = |n: u64, e: u32| {
                let t = Rational::from(n) / Rational::power_of_2(i64::from(e));
                poly::eval(&cv.s, &t) - s_beta * poly::eval(&cv.w, &t) <= 0
            };
            if within(t_end.num(), t_end.exp()) {
                if j == jc {
                    // Fits: at most `MAX_SPANS`.
                    return (j as u32, t_end);
                }
                continue;
            }
            // Bisect on the grid `2^-CUT_DEPTH`: `lo` holds (the span's
            // start: the vertex, or a knot within the cut), `hi` doesn't.
            let shift = CUT_DEPTH - t_end.exp();
            let (mut lo, mut hi) = (0u64, t_end.num() << shift);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if within(mid, CUT_DEPTH) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            return (j as u32, Dyadic::new(lo, CUT_DEPTH).expect("within [0, 1]"));
        }
        (jc as u32, tc)
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
