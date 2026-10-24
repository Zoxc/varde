//! The **distance to an implicit surface**: an upper bound of the
//! distance from every point of a curve to the zero set of a polynomial
//! `F` (a per-tile implicit, such as a quadric's or a torus's quartic),
//! certified by the **ball condition**.
//!
//! **The condition.** If `|∇F| ≥ m > 0` on the ball of radius `|F(x)|/m`
//! about `x`, then `F` has a zero within that radius, so
//! `dist(x, F = 0) ≤ |F(x)|/m` (the gradient's flow from `x` reaches the
//! zero set before leaving the ball). Without the gradient's bound on the
//! whole ball the quotient bounds nothing: near a point where `∇F`
//! vanishes, such as a sphere's centre, `|F|/|∇F|` may be small far from
//! the surface.
//!
//! **The bound on a cell.** A curve is bounded cell by cell, a cell the
//! `k`-th of `2^d` equal parts of one of its spans, at a precision level
//! of the escalation schedule, as the seam bound's cells
//! ([`super`]). At the cell's level the span's homogeneous coefficients
//! `(X, W)` are formed from its affine control points
//! ([`Curve3D::span_at`]) and halved down to the cell; the implicit,
//! `F̂(X − o·W, W) = W^d·F(X/W)` in its own frame at `o`, is formed at
//! the level ([`ImplicitAt`]) and the span put into it
//! ([`HomCurve::compose_implicit`]): `G = W^d·F(x)`, of degree `d·c`, so
//! `|F(x)| ≤ f = max|G| / min W^d` over the cell, the maximum and the
//! minimum certified from the Bernstein coefficients of `G` and of `W^d`
//! (a product of the weights). The span lies in its projected control
//! points' box `B` ([`HomCurve::control_box`]). The squared gradient
//! `|∇F|²`, a polynomial of degree `2(d − 1)` in the point, is formed in
//! the implicit's frame (its terms by the derivatives of `F`'s, their
//! products summed by exponent in order) and its trivariate Bernstein
//! net over a box computed: each coordinate `y = a + (b − a)·s` a linear
//! form in its own `s ∈ [0, 1]`, its powers by repeated products,
//! elevated to the net's degree in that direction, every term the
//! product of its three powers' coefficients times its own; the net's
//! least coefficient is a certified lower bound of `|∇F|²` on the box,
//! its greatest an upper one, the net computed one direction at a time.
//! First over `B`, giving `m₀`; then over `B` grown by `ρ = 9/8·f/m₀` on
//! every side (rounded outwards), giving `m`: where `f/m ≤ ρ`, every
//! point of the cell has its ball of radius `|F(x)|/m` inside that box,
//! and the cell's bound is `f/m`, rounded upwards; else once more with
//! `ρ` grown to `9/8·f/m`, then halving. Where `F` is exactly zero on the
//! cell (`G` all zero coefficients) the bound is 0. The values at the
//! cell's ends (`G`'s and `W^d`'s end coefficients) give a certified
//! **lower** bound of the distance there: `|F(x)| ≤ M·dist(x, F = 0)`
//! with `M` the upper bound of `|∇F|` on the grown box, which holds the
//! segment to the nearest zero.
//!
//! **Escalation and limits** are the seam bound's: a cell at most the
//! target, or within [`TIGHT`](super::TIGHT) of the greatest lower bound
//! found when that is above the target, is done; else rounding (the
//! bound less the one from the values alone) against the hull (that
//! value less `|F|` over `|∇F|` at the cell's ends, from the values
//! alone: what halving may come down to) decides between the next level
//! and halving. A cell with no bound goes to the next level where its
//! failure is rounding's: `|∇F|²`'s net or `W^d` positive in value but
//! not certified so, or the net not positive in value either while no
//! coefficient is certified at most 0 and its rounding is more than a
//! small part of its size (far from the implicit's origin the first
//! level's terms cancel to noise); else (the ball condition unmet: a
//! coefficient certified at most 0, the box reaching where `∇F` is
//! small, or the ball leaving the box) it is halved. Each cell is
//! charged before it runs (`UNITS.implicit_cell` at its level, by the
//! implicit's degree), the stop at [`MAX_SUBDIV_DEPTH`] or the top level
//! with the bound reached, or [`Cap::SubdivisionDepth`] /
//! [`Cap::PrecisionBits`] without one: a curve through a point where
//! `∇F` vanishes, or far enough inside a closed surface that no ball
//! about it avoids such a point, has no bound. The boxes lie along the
//! world's axes, so a long cell along an oblique axis holds the axis
//! (where a cylinder's `∇F` vanishes) until it is about the radius long:
//! a ruling a lattice cell long halves until the budget runs out.
//!
//! **The certificate's own limit.** Unlike a seam's, this bound doesn't
//! come down to the distance as cells shrink: at a point `x` it stays at
//! least `|F(x)|` over the least `|∇F|` on a ball about `x` as wide as
//! the bound, more than the distance by a part growing with the distance
//! over the surface's curvature radius (a few tenths more for a point a
//! tenth of the radius inside a sphere). So a cell is also done once
//! halving gains little and so does the next level: its box's side at
//! most `ρ/16`, `|F|`'s value bound within `1/16` of its values at the
//! ends, and the rounding at most `1/16` of the bound; it then counts as
//! reached, as a lower bound would within `TIGHT` (where only the
//! rounding is above `1/16`, the next level). Without it a curve far from
//! a curved surface would halve to the deepest level everywhere.
//!
//! The result is an upper bound of the distance, so of the deviation's
//! distance part only; a caller with a per-tile implicit gives each
//! tile's part of the curve ([`Curve3D::slice`]).
//!
//! [`MAX_SUBDIV_DEPTH`]: crate::infra::budget::MAX_SUBDIV_DEPTH
//! [`Cap::SubdivisionDepth`]: crate::infra::budget::Cap::SubdivisionDepth
//! [`Cap::PrecisionBits`]: crate::infra::budget::Cap::PrecisionBits
//! [`HomCurve::compose_implicit`]: varde_decide::bezier::HomCurve::compose_implicit
//! [`HomCurve::control_box`]: varde_decide::bezier::HomCurve::control_box

use std::collections::BTreeMap;

use varde_decide::bezier::{Implicit, Poly, Scalar};
use varde_decide::leaves::near_quadric::Quadric;
use varde_decide::num::bounded::{Bounded, div_up, down, mul_up, up};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::Level;

use super::{Cell, Eval, SeamBound, down0, refine};
use crate::geom::curve::Curve3D;
use crate::infra::budget::{LEVELS, Ledger, Stop, implicit_cell_units};

/// The highest degree of an implicit the distance takes: a torus's
/// quartic (`UNITS.implicit_cell` prices up to it).
pub const MAX_IMPLICIT_DEGREE: usize = 4;

/// An implicit surface `F = 0` at every precision level: the same
/// polynomial, its coefficients formed at the level of `like` (exactly
/// for stored values), each carrying its rounding.
pub trait ImplicitAt {
    /// The implicit at `like`'s level.
    fn implicit_at<S: Scalar>(&self, like: &S) -> Implicit<S>;
}

impl ImplicitAt for Quadric {
    fn implicit_at<S: Scalar>(&self, like: &S) -> Implicit<S> {
        self.implicit_form(like)
    }
}

/// A curve's distance bound to an implicit surface and the highest
/// precision level it took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceBound {
    /// The bound, certified and finite: at most the target unless the
    /// target is out of reach (the bound then within
    /// [`TIGHT`](super::TIGHT) of the greatest distance found) or a cell
    /// was open at the top level or the deepest halving; above it an
    /// achieved bound the caller warns of, as a seam bound's.
    pub bound: f64,
    /// The highest level a cell was done at: 0 for `f64`, 1 for
    /// double-double, 2 to 4 for the big-float widths.
    pub level: u8,
}

/// Why a distance to an implicit can't be given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceError {
    /// The implicit's degree, 0 or above [`MAX_IMPLICIT_DEGREE`].
    Degree(usize),
}

/// An upper bound of the distance from every point of `curve` to the
/// zero set of `f`, refined towards `target` (module docs), each cell's
/// computation charged to `ledger`. A target below 0, or a NaN, counts as
/// 0. Stops when the ledger runs out, or at
/// [`Cap::PrecisionBits`](crate::infra::budget::Cap::PrecisionBits) or
/// [`Cap::SubdivisionDepth`](crate::infra::budget::Cap::SubdivisionDepth)
/// on a cell with no certified bound there.
pub(crate) fn implicit_distance<F: ImplicitAt>(
    f: &F,
    curve: &Curve3D,
    target: f64,
    ledger: &mut Ledger<'_>,
) -> Result<DistanceBound, Stop<DistanceError>> {
    let d = f.implicit_at(&Bounded::exact(0.0)).degree();
    if d == 0 || d > MAX_IMPLICIT_DEGREE {
        return Err(Stop::Failed(DistanceError::Degree(d)));
    }
    let SeamBound { bound, level } = refine(
        curve.spans().len(),
        target,
        ledger,
        |level| implicit_cell_units(d, level),
        |cell| eval(f, curve, cell),
    )?;
    Ok(DistanceBound { bound, level })
}

/// The cell's bounds at its level.
fn eval<F: ImplicitAt>(f: &F, curve: &Curve3D, cell: Cell) -> Eval {
    debug_assert!(
        cell.level < LEVELS,
        "a cell's level is one of the schedule's"
    );
    let level = Level::from_index(usize::from(cell.level)).unwrap_or(Level::TOP);
    at_level(&mut EvalAt { f, curve, cell }, level, &mut ())
}

/// A cell's bounds at one precision level ([`eval_at`]).
struct EvalAt<'a, F> {
    f: &'a F,
    curve: &'a Curve3D,
    cell: Cell,
}

impl<F: ImplicitAt> AtLevel<()> for EvalAt<'_, F> {
    type Out = Eval;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Eval {
        eval_at(self.f, self.curve, self.cell, like)
    }

    /// The distance escalates cell by cell, not here.
    fn escalates(_: &Eval) -> bool {
        false
    }
}

/// The factor to spare in the ball's radius.
const SPARE: f64 = 1.125;

/// The radii a cell tries before halving.
const RADII: u32 = 2;

/// How small a part of the ball's radius the box's side, and of the
/// bound the rounding, must be for the bound to count as reached.
const SMALL: f64 = 16.0;

/// No bound: the next level where `value` is finite, else halving.
fn open(value: f64) -> Eval {
    Eval {
        bound: f64::INFINITY,
        value,
        ends: 0.0,
        lower: 0.0,
    }
}

/// `x` rounded upwards, any sign.
fn up_any(x: f64) -> f64 {
    -down(-x)
}

/// The least of the finite values of `x`, `+∞` for none.
fn least_of(x: impl IntoIterator<Item = f64>) -> f64 {
    x.into_iter()
        .fold(f64::INFINITY, |a, x| if x < a { x } else { a })
}

/// The cell's bounds at `like`'s level (module docs).
fn eval_at<S: LevelScalar, F: ImplicitAt>(f: &F, curve: &Curve3D, cell: Cell, like: &S) -> Eval {
    let mut c = curve.span_at(cell.piece, like).expect("a span");
    for bit in (0..cell.depth).rev() {
        let [l, r] = c.split_half();
        c = if (cell.k >> bit) & 1 == 1 { r } else { l };
    }
    let imp = f.implicit_at(like);
    let d = imp.degree();
    let g = c.compose_implicit(&imp).expect("degree at most 8");
    let mut wd = c.weights().clone();
    for _ in 1..d {
        wd = wd.mul(c.weights()).expect("degree at most 8");
    }
    // `|F|` on the cell, certified and from the values alone.
    let gh = g.hull();
    if gh.lo == 0.0 && gh.hi == 0.0 {
        // On the surface exactly.
        return Eval {
            bound: 0.0,
            value: 0.0,
            ends: 0.0,
            lower: 0.0,
        };
    }
    let f_up = div_up(gh.abs_max(), wd.hull().lo);
    let w_val = least_of(wd.coefs().iter().map(LevelScalar::approx));
    let g_val = g
        .coefs()
        .iter()
        .map(|x| x.approx().abs())
        .fold(0.0, |a, x| if x > a { x } else { a });
    let f_val = if w_val > 0.0 {
        g_val / w_val
    } else {
        f64::INFINITY
    };
    if f_up.partial_cmp(&f64::INFINITY) != Some(std::cmp::Ordering::Less) {
        return open(if f_val < f64::INFINITY {
            0.0
        } else {
            f64::INFINITY
        });
    }
    let Some(hull) = c.control_box() else {
        return open(0.0);
    };
    let grad = SquaredGradient::of(&imp);
    // `m₀` on the hull box, then `m` on the box grown by `ρ`.
    let first = grad.net(&hull, 0.0, like);
    if first.lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return first.open();
    }
    let m0 = down0(first.lo.sqrt());
    // The radius: `f/m₀` with an eighth to spare, then once more from the
    // bound it gives where that ball leaves the box.
    let mut rho = mul_up(SPARE, div_up(f_up, m0));
    let mut tries = 0;
    let (grown, bound) = loop {
        if rho.partial_cmp(&f64::INFINITY) != Some(std::cmp::Ordering::Less) {
            return open(f64::INFINITY);
        }
        let grown = grad.net(&hull, rho, like);
        if grown.lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return grown.open();
        }
        let bound = div_up(f_up, down0(grown.lo.sqrt()));
        if bound <= rho {
            break (grown, bound);
        }
        tries += 1;
        if tries == RADII {
            // The ball leaves the box: a smaller cell, a smaller box.
            return open(f64::INFINITY);
        }
        rho = mul_up(SPARE, bound);
    };
    let m_val = grown.least.sqrt();
    let value = f_val / m_val;
    // At the ends: the first and last coefficients, `|F|` over the
    // gradient there (what halving may come down to), and the
    // certified lower bound.
    let m_hi = up(grown.hi.sqrt());
    let (mut ends, mut f_ends, mut lower) = (0.0, 0.0, 0.0);
    for at in [0, 1] {
        let pick = |p: &Poly<S>| -> S {
            let k = if at == 0 { 0 } else { p.degree() };
            p.coefs()[k].clone()
        };
        let (gi, wi) = (&pick(&g), &pick(&wd));
        let w = pick(c.weights()).approx();
        let x: [f64; 3] = std::array::from_fn(|k| pick(&c.coords()[k]).approx() / w);
        let f_end = (gi.approx() / wi.approx()).abs();
        if f_end > f_ends {
            f_ends = f_end;
        }
        let e = f_end / grad.value_at(x).sqrt();
        if e > ends {
            ends = e;
        }
        let at_least = match gi.enclose() {
            [lo, _] if lo > 0.0 => lo,
            [_, hi] if hi < 0.0 => -hi,
            _ => 0.0,
        };
        let w_hi = wi.enclose()[1];
        if w_hi > 0.0 && w_hi < f64::INFINITY && m_hi < f64::INFINITY {
            let l = down0(down0(at_least / w_hi) / m_hi);
            if l > lower {
                lower = l;
            }
        }
    }
    // Halving gains little once `|F|`'s hull is close to its values at
    // the ends and the box is small beside the ball: then the next level
    // where rounding is more than a small part, else the bound is the
    // certificate's own, and counts as reached (module docs).
    let side = hull
        .iter()
        .map(|[lo, hi]| hi - lo)
        .fold(0.0, |a, x| if x > a { x } else { a });
    if side * SMALL <= rho && f_val * SMALL <= f_ends * (SMALL + 1.0) {
        if (bound - value) * SMALL <= bound {
            if bound > lower {
                lower = bound;
            }
        } else {
            ends = value;
        }
    }
    Eval {
        bound,
        value: if value < f64::INFINITY {
            value
        } else {
            f64::INFINITY
        },
        ends,
        lower,
    }
}

/// `at + t`, or `t` where `at` is empty.
fn acc<S: Scalar>(at: &mut Option<S>, t: S) {
    *at = Some(match at.take() {
        Some(s) => s.add(&t),
        None => t,
    });
}

/// `|∇F|²` of an implicit, in its frame: `Σ c·yᵉ` with `y = x − o`, the
/// terms in increasing order of their exponents.
struct SquaredGradient<S> {
    origin: [S; 3],
    terms: Vec<([usize; 3], S)>,
    /// The greatest exponent of each coordinate.
    top: [usize; 3],
}

/// The Bernstein net of `|∇F|²` over a box: its least and greatest
/// coefficients certified, its least value alone, whether a coefficient
/// is certified at most 0, and the widest coefficient's rounding and the
/// greatest value's magnitude.
struct NetHull {
    lo: f64,
    hi: f64,
    least: f64,
    nonpositive: bool,
    rounding: f64,
    size: f64,
}

impl NetHull {
    /// What a net not certified positive asks for: the next level where
    /// it is positive in value, or where no coefficient is certified at
    /// most 0 and the rounding is more than a small part of the net's
    /// size (its values are rounding's, not the geometry's); else
    /// halving (the box reaches where `∇F` is small).
    fn open(&self) -> Eval {
        let rounding = !self.nonpositive && self.rounding * SMALL > self.size;
        open(if self.least > 0.0 || rounding {
            0.0
        } else {
            f64::INFINITY
        })
    }
}

impl<S: Scalar> SquaredGradient<S> {
    /// `Σₖ (∂ₖF)²`: each partial's terms `c·eₖ·y^(e − 1ₖ)` (the
    /// homogeneous form's last exponent, the weight's, dropped), the
    /// products of every pair summed by exponent in order (a pair of two
    /// different terms once, doubled).
    fn of(imp: &Implicit<S>) -> Self {
        let like = &imp.origin()[0];
        let mut sum: BTreeMap<[usize; 3], S> = BTreeMap::new();
        let two = S::int(2, like);
        for k in 0..3 {
            let partial: Vec<([usize; 3], S)> = imp
                .terms()
                .iter()
                .filter(|(e, _)| e[k] > 0)
                .map(|(e, c)| {
                    let mut x = [e[0], e[1], e[2]];
                    x[k] -= 1;
                    let n = u32::try_from(e[k]).expect("a degree at most 4");
                    let c = if n == 1 {
                        c.clone()
                    } else {
                        S::int(n, like).mul(c)
                    };
                    (x, c)
                })
                .collect();
            for (a, (ea, ca)) in partial.iter().enumerate() {
                for (eb, cb) in &partial[a..] {
                    let e = [ea[0] + eb[0], ea[1] + eb[1], ea[2] + eb[2]];
                    let p = ca.mul(cb);
                    let p = if ea == eb { p } else { two.mul(&p) };
                    match sum.get_mut(&e) {
                        Some(s) => *s = s.add(&p),
                        None => {
                            sum.insert(e, p);
                        }
                    }
                }
            }
        }
        let terms: Vec<([usize; 3], S)> = sum.into_iter().collect();
        let mut top = [0; 3];
        for (e, _) in &terms {
            for k in 0..3 {
                top[k] = top[k].max(e[k]);
            }
        }
        SquaredGradient {
            origin: imp.origin().clone(),
            terms,
            top,
        }
    }

    /// `|∇F|²` at the point `x`, from the coefficients' values alone.
    fn value_at(&self, x: [f64; 3]) -> f64
    where
        S: LevelScalar,
    {
        let y: [f64; 3] = std::array::from_fn(|k| x[k] - self.origin[k].approx());
        self.terms
            .iter()
            .map(|(e, c)| {
                // Repeated products, the same bits on every platform.
                (0..3).fold(c.approx(), |p, k| (0..e[k]).fold(p, |p, _| p * y[k]))
            })
            .fold(0.0, |s, t| s + t)
    }

    /// The net over `hull` grown by `grow ≥ 0` on every side (module
    /// docs), the box's corners in the frame rounded outwards.
    fn net(&self, hull: &[[f64; 2]; 3], grow: f64, like: &S) -> NetHull
    where
        S: LevelScalar,
    {
        // Each coordinate's powers `y⁰ … y^top`, each elevated to `top`.
        let powers: [Vec<Vec<S>>; 3] = std::array::from_fn(|k| {
            let [ol, oh] = self.origin[k].enclose();
            let a = down(down(hull[k][0] - grow) - oh);
            let b = up_any(up_any(hull[k][1] + grow) - ol);
            let base = Poly::new([S::from_f64(a, like), S::from_f64(b, like)]).expect("linear");
            let mut p = Poly::new([S::int(1, like)]).expect("a constant");
            (0..=self.top[k])
                .map(|e| {
                    if e > 0 {
                        p = p.mul(&base).expect("degree at most 6");
                    }
                    p.elevate(self.top[k] - e)
                        .expect("degree at most 6")
                        .coefs()
                        .to_vec()
                })
                .collect()
        });
        let [n0, n1, n2] = self.top.map(|t| t + 1);
        // Contracted one direction at a time, last first: per `(e₀, e₁)`
        // the sum over `e₂` along `s₂`, per `e₀` the sum over `e₁` along
        // `s₁`, then over `e₀` along `s₀`; each sum in the terms' order.
        let mut by_01: Vec<([usize; 2], Vec<Option<S>>)> = Vec::new();
        for (e, c) in &self.terms {
            if by_01.last().is_none_or(|(k, _)| *k != [e[0], e[1]]) {
                by_01.push(([e[0], e[1]], vec![None; n2]));
            }
            let row = &mut by_01.last_mut().expect("a row").1;
            for (l, at) in row.iter_mut().enumerate() {
                acc(at, c.mul(&powers[2][e[2]][l]));
            }
        }
        let mut by_0: Vec<(usize, Vec<Option<S>>)> = Vec::new();
        for ([e0, e1], row) in &by_01 {
            if by_0.last().is_none_or(|(k, _)| k != e0) {
                by_0.push((*e0, vec![None; n1 * n2]));
            }
            let plane = &mut by_0.last_mut().expect("a plane").1;
            for j in 0..n1 {
                let p = &powers[1][*e1][j];
                for (l, x) in row.iter().enumerate() {
                    if let Some(x) = x {
                        acc(&mut plane[j * n2 + l], p.mul(x));
                    }
                }
            }
        }
        let mut net: Vec<Option<S>> = vec![None; n0 * n1 * n2];
        for (e0, plane) in &by_0 {
            for i in 0..n0 {
                let p = &powers[0][*e0][i];
                for (jl, x) in plane.iter().enumerate() {
                    if let Some(x) = x {
                        acc(&mut net[i * n1 * n2 + jl], p.mul(x));
                    }
                }
            }
        }
        let mut out = NetHull {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
            least: f64::INFINITY,
            nonpositive: false,
            rounding: 0.0,
            size: 0.0,
        };
        for x in &net {
            // A plane's constant gradient, or a zero polynomial: no term
            // there is 0.
            let (lo, hi, v) = match x {
                Some(x) => {
                    let [lo, hi] = x.enclose();
                    (lo, hi, x.approx())
                }
                None => (0.0, 0.0, 0.0),
            };
            // An uncertified coefficient, a NaN, bounds nothing.
            let lo = if lo.is_nan() { f64::NEG_INFINITY } else { lo };
            let hi = if hi.is_nan() { f64::INFINITY } else { hi };
            if lo < out.lo {
                out.lo = lo;
            }
            if hi > out.hi {
                out.hi = hi;
            }
            if hi <= 0.0 {
                out.nonpositive = true;
            }
            let width = hi - lo;
            if width > out.rounding {
                out.rounding = width;
            }
            if v.abs() > out.size {
                out.size = v.abs();
            }
            if v.is_nan() || v < out.least {
                out.least = if v.is_nan() { f64::NEG_INFINITY } else { v };
            }
        }
        out
    }
}

/// Hooks for the unit table's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times distance cells
/// per precision level and degree, and the fixtures it shares with this
/// module's tests. Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;

    /// An implicit with `f64` coefficients, each exact at every level:
    /// `Σ c·(x − o)ᵉ`, `Σ e ≤ degree`.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Terms {
        /// The degree.
        pub degree: usize,
        /// The frame's origin.
        pub origin: [f64; 3],
        /// The terms `(e, c)`.
        pub terms: Vec<([usize; 3], f64)>,
    }

    impl ImplicitAt for Terms {
        fn implicit_at<S: Scalar>(&self, like: &S) -> Implicit<S> {
            Implicit::new(
                self.degree,
                self.origin.map(|x| S::from_f64(x, like)),
                self.terms.iter().map(|&(e, c)| (e, S::from_f64(c, like))),
            )
            .expect("an implicit's terms")
        }
    }

    impl Terms {
        /// The sphere `|x − c|² − r²` about `centre`.
        pub fn sphere(centre: [f64; 3], r: f64) -> Terms {
            Terms {
                degree: 2,
                origin: centre,
                terms: vec![
                    ([2, 0, 0], 1.0),
                    ([0, 2, 0], 1.0),
                    ([0, 0, 2], 1.0),
                    ([0, 0, 0], -r * r),
                ],
            }
        }

        /// The plane `n·(x − o)`.
        pub fn plane(origin: [f64; 3], n: [f64; 3]) -> Terms {
            Terms {
                degree: 1,
                origin,
                terms: vec![([1, 0, 0], n[0]), ([0, 1, 0], n[1]), ([0, 0, 1], n[2])],
            }
        }

        /// The torus of radii `big` and `small` about the axis through
        /// `centre` along coordinate `axis`:
        /// `(|y|² + R² − r²)² − 4R²·(|y|² − y_axis²)`, `y = x − centre`,
        /// expanded with its coefficients computed in `f64` (exact for
        /// small dyadic radii).
        pub fn torus(centre: [f64; 3], axis: usize, big: f64, small: f64) -> Terms {
            let k = big * big - small * small;
            let r4 = 4.0 * big * big;
            let mut terms: BTreeMap<[usize; 3], f64> = BTreeMap::new();
            let mut add = |e: [usize; 3], c: f64| *terms.entry(e).or_insert(0.0) += c;
            // `|y|⁴`.
            for a in 0..3 {
                for b in 0..3 {
                    let mut e = [0; 3];
                    e[a] += 2;
                    e[b] += 2;
                    add(e, 1.0);
                }
            }
            for a in 0..3 {
                let mut e = [0; 3];
                e[a] = 2;
                // `2k·|y|²` and `−4R²·y²` off the axis.
                add(e, 2.0 * k - if a == axis { 0.0 } else { r4 });
            }
            add([0; 3], k * k);
            Terms {
                degree: 4,
                origin: centre,
                terms: terms.into_iter().filter(|&(_, c)| c != 0.0).collect(),
            }
        }
    }

    /// Why [`cells`] can't run.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CellsError {
        /// No precision level of this index: at least `LEVELS`.
        Level(u8),
        /// Deeper than the distance halves a span.
        Depth(u32),
    }

    /// Evaluates every cell `depth` halvings deep of every span of
    /// `curve` (`2^depth` per span) against `f` at precision level
    /// `level`, as the distance does; the number of cells.
    pub fn cells<F: ImplicitAt>(
        f: &F,
        curve: &Curve3D,
        level: u8,
        depth: u32,
    ) -> Result<u64, CellsError> {
        if level >= LEVELS {
            return Err(CellsError::Level(level));
        }
        if depth > crate::infra::budget::MAX_SUBDIV_DEPTH {
            return Err(CellsError::Depth(depth));
        }
        let mut n = 0u64;
        let mut lower = 0.0;
        for piece in 0..curve.spans().len() {
            for k in 0..1u64 << depth {
                let e = eval(
                    f,
                    curve,
                    Cell {
                        piece,
                        k,
                        depth,
                        level,
                    },
                );
                // Kept, so the evaluation isn't optimised away.
                if e.lower > lower {
                    lower = e.lower;
                }
                n += 1;
            }
        }
        std::hint::black_box(lower);
        Ok(n)
    }

    /// An arc near the sphere `|x| = 50` and one near the torus of radii
    /// 64 and 16 about `z`: quarter circles of their surfaces, the middle
    /// control point moved by `10⁻³` mm, end weights 3 and ⅓.
    pub fn arcs() -> (Terms, Curve3D, Terms, Curve3D) {
        let arc = |r: f64, z: f64| {
            let (w, mid) = (3.0, std::f64::consts::FRAC_1_SQRT_2);
            Curve3D::new(
                vec![[[r, 0.0, z, w], [r + 1e-3, r, z, mid], [0.0, r, z, 1.0 / w]]],
                vec![0.0, 1.0],
            )
            .expect("a curve")
        };
        (
            Terms::sphere([0.0; 3], 50.0),
            arc(50.0, 0.0),
            Terms::torus([0.0; 3], 2, 64.0, 16.0),
            arc(80.0, 0.0),
        )
    }

    /// The distance of `curve` to `f` towards `target` in a pass of its
    /// own on a budget of `budget` units: the bound, `None` if the pass
    /// ran out or failed, and the units used.
    pub fn alone<F: ImplicitAt + Sync>(
        f: &F,
        curve: &Curve3D,
        target: f64,
        budget: u64,
    ) -> (Option<DistanceBound>, u64) {
        use crate::infra::budget::{Budget, Pass, Work};
        let mut work = Work::new(&Budget::new(budget));
        let pass = Pass {
            stage: "distance",
            rung: 0,
        };
        let out = work.pass(pass, &[()], |_, ledger| {
            implicit_distance(f, curve, target, ledger)
        });
        (out.ok().map(|b| b[0]), work.used())
    }
}

#[cfg(test)]
mod tests;
