//! Weight conditioning: the Möbius reparametrisations that keep the
//! weights of tiles and edges close together, applied at construction
//! only (never during an operation), and the weight ratio `κ_w` that
//! measures what is left.
//!
//! **The map.** A rational quadratic (or Möbius) span with weights `wᵢ`
//! and the span with weights `wᵢ·ρⁱ` (any `ρ > 0`, times any common
//! factor) are the same curve: the second at `σ` is the first at `s =
//! ρσ/(1 − σ + ρσ)`, so `σ = s/(s + ρ(1 − s))`, a Möbius map of `[0, 1]`
//! onto itself fixing both ends. Control points don't move, only the
//! weights; every point at a span's ends, so every knot and every tile
//! corner, stays where it was to the bit. G, linear and iso p-curve spans
//! keep their kind (their Möbius coordinates stay Möbius).
//!
//! **`κ_w`** ([`kappa`]) is the ratio of the largest weight to the
//! least: the factor by which the weights' rounding grows in an
//! evaluation. It is cached per tile ([`Tile::kappa_w`]); `check` warns
//! above [`KAPPA_WARN`].
//!
//! **Per column or row.** A Möbius map in `u` must act on a whole tile
//! column (and one in `v` on a whole row) to keep the rows neighbours
//! share and the global UV of every tile in it. The lattices' columns
//! and rows are angle cells (quarter circles, weights `1, √½, 1`
//! already), lines (weights 1), a spindle torus's tube arcs (`1, w, 1`)
//! and profile spans: a conic-chain cylinder's columns and a revolved
//! conic's rows. Each profile span is put in **standard form**
//! ([`standard`]): end weights 1 and the middle weight `ω = w₁/√(w₀w₂)`,
//! the conic's shape invariant, rounded once to the nearest `f64` (exact
//! where `w₀ = w₂`; a stored `1, w, 1` span keeps its bits), `ρ =
//! √(w₀/w₂)`. That is the least `κ_w` of any map (`max(ω, 1/ω)`), and
//! tensor products of standard rows and columns have the least of
//! theirs. It is a function of the form's key and the cell alone (the
//! span's stored values), so `extend` and every constructor of the form
//! reproduce it; and every joint between spans has the weight 1 on both
//! sides, so neighbouring tiles share their row bit for bit whatever
//! weights the profile was given with. A span whose `ω` lies outside the
//! tiles' weight range (`κ_w > 64` at best) has no tile. The analytic
//! inverse reads the standard spans ([`super::inverse`]).
//!
//! **Per edge.** An edge's space curve `C` and its coedges' p-curves
//! share the parameter `t` (the B-rep's invariant 2(e)), so one map acts
//! on all of them at once ([`condition_edge`]): on each interval between
//! consecutive knots of their common refinement (each chain split at the
//! others' knots, [`Curve3D::split`], [`PCurve::split`]), the map with
//! `ρ = 2ᵏ`, `k` in `−12..=12`, whose largest `κ_w` over the interval's
//! spans (`C`'s and each p-curve coordinate's weights) is least, the
//! least `|k|` on a tie (then the negative). Each span's weights `wᵢ·2^{ik}`
//! are then scaled by `2^{−⌊k·d/2⌋}` (`d` its degree) to stay near the
//! given ones; a `k` that puts a weight outside the curves' range is not
//! taken. A power of two keeps it exact: no value is rounded, so the
//! curve, the p-curves and everything certified between them (the seam
//! bound, the knot points) are the same, and only `t` along each
//! interval moves; it comes within a factor 2 of the least `κ_w` a
//! common map reaches. The splits are the only rounding (each cut's
//! point the chain's canonical point there, its sub-spans rounded once),
//! and where the chains already share their knots there is none.
//! Construction-time geometry, as the chain constructors: not charged; a
//! caller charges the spans gained.

use malachite::Rational;

use super::curve::{Curve3D, CurveError, MAX_WEIGHT, MIN_WEIGHT};
use super::form::Span2;
use super::pcurve::{PCurve, PSpan};
use super::round::{nearest_sqrt, q};

#[cfg(doc)]
use super::tile::Tile;

/// The `κ_w` above which `check` warns.
pub const KAPPA_WARN: f64 = 16.0;

/// The largest `|k|` of an edge's map `ρ = 2ᵏ` ([`condition_edge`]):
/// weights in `[1/64, 64]` are within `2¹²` of each other, so the best
/// map of a span of degree 1 or 2 has `|k| ≤ 12`.
pub const MAX_EDGE_SHIFT: i32 = 12;

/// `κ_w` of `weights`: the largest over the least, rounded to the
/// nearest `f64`; 1 for no weights, `+∞` where the least isn't positive.
pub fn kappa(weights: impl IntoIterator<Item = f64>) -> f64 {
    let (mut lo, mut hi) = (f64::INFINITY, 0.0f64);
    let mut any = false;
    for w in weights {
        any = true;
        if w < lo {
            lo = w;
        }
        if w > hi {
            hi = w;
        }
    }
    if !any {
        1.0
    } else if lo > 0.0 {
        hi / lo
    } else {
        f64::INFINITY
    }
}

/// The square of a profile span's standard middle weight, exactly:
/// `ω² = w₁²/(w₀w₂)` (module docs). The weights must be positive.
pub(in crate::geom) fn middle_square(span: &Span2) -> Rational {
    let [w0, w1, w2] = span.map(|p| q(p[2]));
    &w1 * &w1 / (w0 * w2)
}

/// The profile span `span` (`(x, y, w)` per control point) in standard
/// form (module docs): the same control points, end weights 1 and the
/// middle weight `w₁/√(w₀w₂)` rounded once. The weights must be
/// positive (a key's are).
pub fn standard(span: &Span2) -> Span2 {
    let [p0, p1, p2] = *span;
    let w = nearest_sqrt(&middle_square(span));
    [[p0[0], p0[1], 1.0], [p1[0], p1[1], w], [p2[0], p2[1], 1.0]]
}

/// An edge's curve and its coedges' p-curves, conditioned together
/// ([`condition_edge`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionedEdge {
    /// The space curve, over the common knots.
    pub curve: Curve3D,
    /// The p-curves, in the order given, over the common knots.
    pub pcurves: Vec<PCurve>,
    /// The map of each interval between the common knots: `k` of `ρ =
    /// 2ᵏ`, 0 where the spans kept their weights.
    pub shifts: Vec<i32>,
}

/// Why an edge can't be conditioned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionError {
    /// A p-curve doesn't run over the curve's range of `t` (first and
    /// last knots, bit for bit).
    Range { pcurve: usize },
    /// A chain refused its split at the common knots.
    Curve(CurveError),
}

impl From<CurveError> for ConditionError {
    fn from(e: CurveError) -> Self {
        ConditionError::Curve(e)
    }
}

/// The edge's curve `curve` and its coedges' p-curves `pcurves`, each
/// split at the others' knots and reparametrised together by one Möbius
/// map per interval between them (module docs). Every point at a knot,
/// and the image of every interval, is kept; the knots too.
pub fn condition_edge(
    curve: &Curve3D,
    pcurves: &[&PCurve],
) -> Result<ConditionedEdge, ConditionError> {
    let range = |k: &[f64]| [k[0], k[k.len() - 1]].map(f64::to_bits);
    let want = range(curve.knots());
    if let Some(pcurve) = pcurves.iter().position(|p| range(p.knots()) != want) {
        return Err(ConditionError::Range { pcurve });
    }
    let mut knots: Vec<f64> = curve
        .knots()
        .iter()
        .chain(pcurves.iter().flat_map(|p| p.knots()))
        .copied()
        .collect();
    knots.sort_by(f64::total_cmp);
    knots.dedup_by(|a, b| a.to_bits() == b.to_bits());
    let c = curve.split(&knots)?;
    let ps = pcurves
        .iter()
        .map(|p| p.split(&knots))
        .collect::<Result<Vec<_>, _>>()?;
    debug_assert!(ps.iter().all(|p| p.knots() == c.knots()));
    let shifts: Vec<i32> = (0..c.spans().len())
        .map(|j| {
            let mut ws: Vec<Vec<f64>> = vec![c.spans()[j].iter().map(|p| p[3]).collect()];
            for p in &ps {
                ws.extend(weight_rows(&p.spans()[j]));
            }
            best_shift(&ws)
        })
        .collect();
    let spans = c
        .spans()
        .iter()
        .zip(&shifts)
        .map(|(s, &k)| shifted(s, k))
        .collect();
    let curve = Curve3D::new(spans, c.knots().to_vec())?;
    let pcurves = ps
        .iter()
        .map(|p| {
            let spans = p
                .spans()
                .iter()
                .zip(&shifts)
                .map(|(s, &k)| shifted_pspan(s, k))
                .collect();
            PCurve::from_spans(spans, p.knots().to_vec())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ConditionedEdge {
        curve,
        pcurves,
        shifts,
    })
}

/// The weights of a p-curve span's coordinates of degree 1 or 2, one row
/// per set of weights (a Q or linear span's shared ones once).
fn weight_rows(s: &PSpan) -> Vec<Vec<f64>> {
    let row = |c: &[[f64; 2]]| c.iter().map(|p| p[1]).collect::<Vec<f64>>();
    match s {
        PSpan::Q(p) => vec![p.iter().map(|x| x[2]).collect()],
        PSpan::Linear(p) => vec![p.iter().map(|x| x[2]).collect()],
        PSpan::G { m, q, .. } => vec![row(m), row(q)],
        PSpan::Iso { m, .. } => vec![row(m)],
    }
}

/// The `k` of the edge's map on one interval (module docs): of `0, −1,
/// 1, −2, 2, …` up to [`MAX_EDGE_SHIFT`], the first with the least
/// largest `κ_w` over `rows` whose weights all stay in range.
fn best_shift(rows: &[Vec<f64>]) -> i32 {
    let mut best = (0, f64::INFINITY);
    for n in 0..=2 * MAX_EDGE_SHIFT {
        // 0, −1, 1, −2, 2, …
        let k = if n % 2 == 1 { -(n + 1) / 2 } else { n / 2 };
        let mut worst = 0.0f64;
        let mut ok = true;
        for r in rows {
            let w = scale_row(r, k);
            if !w.iter().all(|w| (MIN_WEIGHT..=MAX_WEIGHT).contains(w)) {
                ok = false;
                break;
            }
            let kw = kappa(w.iter().copied());
            if kw > worst {
                worst = kw;
            }
        }
        if ok && worst < best.1 {
            best = (k, worst);
        }
    }
    best.0
}

/// The weights `wᵢ·2^{ik − ⌊k·d/2⌋}` of a span of degree `d = len − 1`:
/// exact (powers of two), or out of range and refused by the caller.
fn scale_row(w: &[f64], k: i32) -> Vec<f64> {
    let d = w.len() as i32 - 1;
    let m = (k * d).div_euclid(2);
    w.iter()
        .enumerate()
        .map(|(i, &w)| times_pow2(w, k * i as i32 - m))
        .collect()
}

/// `x·2^e`, exact for the weights and shifts of [`scale_row`] (no
/// overflow or underflow at `|e| ≤ 36` on values within `[2⁻⁶, 2⁶]`).
fn times_pow2(x: f64, e: i32) -> f64 {
    x * f64::from_bits(((1023 + e) as u64) << 52)
}

/// The control values `pts`, weights last, with the weights scaled as
/// [`scale_row`] does.
fn shifted<const N: usize, const D: usize>(pts: &[[f64; D]; N], k: i32) -> [[f64; D]; N] {
    let w = scale_row(&pts.map(|p| p[D - 1]), k);
    std::array::from_fn(|i| {
        let mut p = pts[i];
        p[D - 1] = w[i];
        p
    })
}

/// The p-curve span with each coordinate's weights scaled as
/// [`scale_row`] does: its kind kept.
fn shifted_pspan(s: &PSpan, k: i32) -> PSpan {
    match s {
        PSpan::Q(p) => PSpan::Q(shifted(p, k)),
        PSpan::Linear(p) => PSpan::Linear(shifted(p, k)),
        PSpan::G { mobius, m, q } => PSpan::G {
            mobius: *mobius,
            m: shifted(m, k),
            q: shifted(q, k),
        },
        PSpan::Iso { fixed, at, m } => PSpan::Iso {
            fixed: *fixed,
            at: *at,
            m: shifted(m, k),
        },
    }
}

#[cfg(test)]
mod tests;
