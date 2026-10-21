//! Certified bounds: the **seam bound** of a coedge, an upper bound of
//! `|S(p(t)) − C(t)|` over the coedge, with the **same `t`** on the face's
//! surface `S` composed with its p-curve `p` and on the edge's curve `C`.
//! It is a parametric deviation, so it also bounds the distance between
//! the two images; it is the width of the coedge's ribbon, the surface
//! between them.
//!
//! **Pieces.** The coedge is cut at every knot of `p` and of `C` inside
//! it, so each piece lies in one span of each. A p-curve span lies in
//! one tile, `(i, j)`, decided exactly: each stored control point's `u`
//! within `[i, i + 1]` and `v` within `[j, j + 1]`, compared as floats
//! (the span lies in its control points' hull, as its weights are
//! positive); the lower candidate where a span runs on a tile line at
//! the domain's far side.
//!
//! **The bound on a cell.** A piece is bounded cell by cell, a cell its
//! `k`-th of `2^d` equal parts in `t`, at a precision level of the
//! escalation schedule (`f64`, double-double, big-float at 212 to 848
//! bits). At the cell's level the spans' homogeneous coefficients are
//! formed from their affine control points ([`Curve3D::span_at`]: each
//! product `w·x` rounded once with its rounding carried at the first
//! level, exact above), and so are the tile's; the spans are restricted
//! to the piece and halved down to the cell. With the span's local
//! homogeneous coordinates `U' = U − i·W`, `V' = V − j·W`, the tile's
//! homogeneous net `Pₖₗ` composed with it is
//! `H = Σₖₗ Pₖₗ·Bₖ(U', W)·Bₗ(V', W)`, `Bₖ` the homogeneous quadratic
//! Bernstein basis (`(W − U')²`, `2U'(W − U')`, `U'²`): degree 8, the
//! tile's point `(H₀, H₁, H₂) / H₃`. The difference with `C = (X, W_C)`
//! is `N / D`, `Nₘ = Hₘ·W_C − Xₘ·H₃` of degree 10 and `D = H₃·W_C`. Its
//! length is at most `√(Σₘ max|Nₘ|²) / min D`, the maxima and the
//! minimum certified from the Bernstein coefficients (the forms lie in
//! their coefficients' hull), every error carried, rounded upwards. The
//! first and last coefficients are the values at the cell's ends, so they
//! also give a certified **lower** bound of the deviation.
//!
//! **Escalation: space, then precision.** A cell whose bound is at most
//! the target is done. Otherwise its excess is split by a fixed
//! comparison of computed values: the part due to rounding (the bound
//! less the same bound from the coefficients' values alone) against the
//! part due to the Bernstein hull (that value bound less the deviation at
//! the cell's ends). Where rounding dominates, or the denominator is
//! positive in value but not certified so, the cell is computed again one
//! level up; else it is halved in `t`, its halves at its level. A cell is
//! also done once its bound is within [`TIGHT`] of the greatest certified
//! lower bound found on the coedge, if that is above the target: the
//! target is out of reach then (the true deviation exceeds it), and the
//! bound is within that factor of the greatest deviation ("Above the
//! target", below). The cells run depth first in `t` order, so the
//! result depends on the input alone.
//!
//! **Charges and limits.** Each cell's computation is charged before it
//! runs, the unit table's units for a seam cell at its level
//! (`UNITS.seam_cell`, dearer at each level up): ordinary work at the
//! first level, a precision step above it; running out of budget stops
//! the coedge. A cell still open at the top level, or at
//! [`MAX_SUBDIV_DEPTH`] halvings, is done with its bound, above the
//! target; without a certified bound it stops the coedge at
//! [`Cap::PrecisionBits`] or [`Cap::SubdivisionDepth`].
//!
//! **Above the target.** A seam bound is an accuracy item: the result is
//! always a certified bound, at most the target where it can be met.
//! Above it (the target out of reach, or a cell open at a limit) the
//! caller stores it all the same, as the coedge's seam bound and so its
//! edge's tolerance, and lists the edge with this achieved bound among
//! the operation's warnings; only running out of budget, or a cell with
//! no certified bound at a limit, fails.
//!
//! Biquadratic tiles and Q spans only. On a plane, whose one tile spans
//! the coordinate box (4·10⁶ mm), the first level's bound on a whole 90°
//! arc of radius 50 is about `5·10⁻⁹` mm for weights 1, though the arc
//! lies exactly on the plane; it grows with the spread of a span's
//! weights, as the bound divides the largest coefficients' rounding by
//! the least denominator: with end weights 3 and ⅓ about `10⁻⁴` mm, with
//! end weights 64 and 1/64 about `2·10⁹` mm. Such cells escalate.

use varde_decide::bezier::{HomCurve, Poly, Scalar};
use varde_decide::num::bigf::Width;
use varde_decide::num::bounded::{add_up, div_up, down, mul_up, up};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::Level;

use super::curve::{Curve3D, PCurve, Span2, local};
use super::surface::Surface;
use super::tile::Tile;
use crate::infra::budget::{
    Cap, LEVELS, Ledger, MAX_SUBDIV_DEPTH, Stop, level_step, seam_cell_units,
};

/// How far above the greatest lower bound found a cell's bound may be to
/// be done when the target is out of reach (module docs).
pub const TIGHT: f64 = 1.125;

const _: () = assert!(LEVELS as usize == 2 + Width::ALL.len());

/// A coedge's seam bound and the highest precision level it took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamBound {
    /// The bound, certified and finite: at most the target unless the
    /// target is out of reach (the bound then within [`TIGHT`] of the
    /// greatest deviation) or a cell was open at the top level or at
    /// [`MAX_SUBDIV_DEPTH`]. Above the target it is an achieved bound the
    /// caller warns of (module docs, "Above the target").
    pub bound: f64,
    /// The highest level a cell was done at: 0 for `f64`, 1 for
    /// double-double, 2 to 4 for the big-float widths.
    pub level: u8,
}

/// Why a seam bound can't be given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeamError {
    /// The p-curve's knots run outside the curve's.
    Range,
    /// Span `span` of the p-curve doesn't lie in one tile of the domain.
    Tile { span: usize },
}

/// A piece of the coedge: `[a, b]` of `t`, in span `sp` of the p-curve
/// and span `sc` of the curve, on tile `(i, j)`.
struct Piece<'a> {
    tile: &'a Tile,
    i: i64,
    j: i64,
    sp: usize,
    sc: usize,
    a: f64,
    b: f64,
}

/// Part `k` of `2^depth` of a piece, at a precision level.
#[derive(Clone, Copy)]
struct Cell {
    piece: usize,
    k: u64,
    depth: u32,
    level: u8,
}

/// The seam bound of `pcurve` on `surface` against `curve` over the
/// p-curve's knots' range, refined towards `target` (module docs), each
/// cell's computation charged to `ledger`. A target below 0, or a NaN,
/// counts as 0. Stops when the ledger runs out, or at
/// [`Cap::PrecisionBits`] or [`Cap::SubdivisionDepth`] on a cell with no
/// certified bound there.
pub(crate) fn seam_bound(
    surface: &Surface,
    curve: &Curve3D,
    pcurve: &PCurve,
    target: f64,
    ledger: &mut Ledger<'_>,
) -> Result<SeamBound, Stop<SeamError>> {
    // A NaN compares false with every bound: never met, and never out of
    // reach.
    let target = if target > 0.0 { target } else { 0.0 };
    let pieces = pieces(surface, curve, pcurve).map_err(Stop::Failed)?;
    // A stack: the next cell last.
    let mut cells: Vec<Cell> = (0..pieces.len())
        .rev()
        .map(|piece| Cell {
            piece,
            k: 0,
            depth: 0,
            level: 0,
        })
        .collect();
    let mut out = SeamBound {
        bound: 0.0,
        level: 0,
    };
    // The greatest certified lower bound of the deviation found.
    let mut lower = 0.0;
    while let Some(cell) = cells.pop() {
        ledger.charge(level_step(cell.level), seam_cell_units(cell.level))?;
        let e = eval(curve, pcurve, &pieces[cell.piece], cell);
        if e.lower > lower {
            lower = e.lower;
        }
        let mut done = || {
            if e.bound > out.bound {
                out.bound = e.bound;
            }
            if cell.level > out.level {
                out.level = cell.level;
            }
        };
        match judge(&e, target, lower) {
            Verdict::Done => done(),
            Verdict::Precision if cell.level + 1 < LEVELS => cells.push(Cell {
                level: cell.level + 1,
                ..cell
            }),
            Verdict::Precision if e.bound < f64::INFINITY => done(),
            Verdict::Precision => return Err(Stop::Cap(Cap::PrecisionBits)),
            Verdict::Space if cell.depth < MAX_SUBDIV_DEPTH => {
                // The left half on top.
                for k in [2 * cell.k + 1, 2 * cell.k] {
                    cells.push(Cell {
                        k,
                        depth: cell.depth + 1,
                        ..cell
                    });
                }
            }
            Verdict::Space if e.bound < f64::INFINITY => done(),
            Verdict::Space => return Err(Stop::Cap(Cap::SubdivisionDepth)),
        }
    }
    Ok(out)
}

/// The pieces of the coedge (module docs), in `t` order.
fn pieces<'a>(
    surface: &'a Surface,
    curve: &Curve3D,
    pcurve: &PCurve,
) -> Result<Vec<Piece<'a>>, SeamError> {
    let (pk, ck) = (pcurve.knots(), curve.knots());
    let (t0, t1) = (pk[0], pk[pk.len() - 1]);
    if t0 < ck[0] || t1 > ck[ck.len() - 1] {
        return Err(SeamError::Range);
    }
    let tiles = pcurve
        .spans()
        .iter()
        .enumerate()
        .map(|(span, s)| tile_of(surface, s).ok_or(SeamError::Tile { span }))
        .collect::<Result<Vec<_>, _>>()?;
    // The cuts: the p-curve's knots and the curve's inside them, merged.
    let mut cuts: Vec<f64> = pk
        .iter()
        .chain(ck.iter().filter(|&&t| t0 < t && t < t1))
        .copied()
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    Ok(cuts
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            // The spans holding the piece: the last starting at or below
            // `a`.
            let sp = pk.partition_point(|&k| k <= a) - 1;
            let sc = ck.partition_point(|&k| k <= a) - 1;
            let (tile, i, j) = tiles[sp];
            Piece {
                tile,
                i,
                j,
                sp,
                sc,
                a,
                b,
            }
        })
        .collect())
}

/// What to do with a cell.
enum Verdict {
    /// Its bound counts.
    Done,
    /// Compute it again one level up.
    Precision,
    /// Halve it.
    Space,
}

/// The cell's next step (module docs), `lower` the greatest certified
/// lower bound of the deviation found so far.
fn judge(e: &Eval, target: f64, lower: f64) -> Verdict {
    if e.bound == f64::INFINITY {
        // Positive in value but not certified so: the rounding.
        return if e.value < f64::INFINITY {
            Verdict::Precision
        } else {
            Verdict::Space
        };
    }
    if e.bound <= target || (lower > target && e.bound <= TIGHT * lower) {
        return Verdict::Done;
    }
    let rounding = e.bound - e.value;
    let hull = e.value - e.ends;
    if rounding > hull {
        Verdict::Precision
    } else {
        Verdict::Space
    }
}

/// One cell's bounds (module docs).
struct Eval {
    /// The certified upper bound, `+∞` unless certified.
    bound: f64,
    /// The bound from the coefficients' values alone, `+∞` unless the
    /// denominator's are positive.
    value: f64,
    /// The greater deviation at the cell's ends, from the values alone.
    ends: f64,
    /// A certified lower bound of the deviation at the cell's ends.
    lower: f64,
}

/// The cell's bounds at its level.
fn eval(curve: &Curve3D, pcurve: &PCurve, piece: &Piece<'_>, cell: Cell) -> Eval {
    debug_assert!(
        cell.level < LEVELS,
        "a cell's level is one of the schedule's"
    );
    let level = Level::from_index(usize::from(cell.level)).unwrap_or(Level::TOP);
    let mut at = EvalAt {
        curve,
        pcurve,
        piece,
        cell,
    };
    at_level(&mut at, level, &mut ())
}

/// A cell's bounds at one precision level ([`eval_at`]).
struct EvalAt<'c, 'p> {
    curve: &'c Curve3D,
    pcurve: &'c PCurve,
    piece: &'c Piece<'p>,
    cell: Cell,
}

impl AtLevel<()> for EvalAt<'_, '_> {
    type Out = Eval;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Eval {
        eval_at(self.curve, self.pcurve, self.piece, self.cell, like)
    }

    /// The seam bound escalates cell by cell ([`judge`]), not here.
    fn escalates(_: &Eval) -> bool {
        false
    }
}

/// The cell's bounds at `like`'s level: the spans restricted to the
/// piece, halved down to the cell.
fn eval_at<S: LevelScalar>(
    curve: &Curve3D,
    pcurve: &PCurve,
    piece: &Piece<'_>,
    cell: Cell,
    like: &S,
) -> Eval {
    let (pk, ck) = (pcurve.knots(), curve.knots());
    let (sp, sc) = (piece.sp, piece.sc);
    let mut p = restrict(
        &pcurve.span_at(sp, like).expect("a span"),
        local(piece.a, pk[sp], pk[sp + 1], like),
        local(piece.b, pk[sp], pk[sp + 1], like),
    );
    let mut c = restrict(
        &curve.span_at(sc, like).expect("a span"),
        local(piece.a, ck[sc], ck[sc + 1], like),
        local(piece.b, ck[sc], ck[sc + 1], like),
    );
    for bit in (0..cell.depth).rev() {
        let right = (cell.k >> bit) & 1 == 1;
        let ([p0, p1], [c0, c1]) = (p.split_half(), c.split_half());
        (p, c) = if right { (p1, c1) } else { (p0, c0) };
    }
    direct(piece.tile, piece.i, piece.j, &p, &c, like)
}

/// The tile holding span `s` and its indices: `(⌊u⌋, ⌊v⌋)` of the least
/// control point, or the one below in either direction, whichever holds
/// every control point and lies in the domain.
fn tile_of<'a>(surface: &'a Surface, s: &Span2) -> Option<(&'a Tile, i64, i64)> {
    let pick = |k: usize, range: &std::ops::Range<i64>| -> Option<i64> {
        let least = s
            .iter()
            .map(|p| p[k])
            .fold(f64::INFINITY, |a, x| if x < a { x } else { a });
        // Below 2⁵² in magnitude, so the candidates and the tile lines
        // about them are exact as `i64`s and as `f64`s.
        if least.is_nan() || least.abs() >= 4_503_599_627_370_496.0 {
            return None;
        }
        let c = least.floor() as i64;
        [c, c - 1].into_iter().find(|&i| {
            if !range.contains(&i) {
                return false;
            }
            let (lo, hi) = (i as f64, (i + 1) as f64);
            s.iter().all(|p| lo <= p[k] && p[k] <= hi)
        })
    };
    let rect = surface.rect();
    let (i, j) = (pick(0, &rect.u)?, pick(1, &rect.v)?);
    let tile = surface.tile(i, j)?;
    Some((tile, i, j))
}

/// The span `c` restricted to `[sa, sb]`, as a span on `[0, 1]`.
fn restrict<S: Scalar, const N: usize>(c: &HomCurve<S, N>, sa: S, sb: S) -> HomCurve<S, N> {
    let left = if sb.is_int(1) {
        c.clone()
    } else {
        let [l, _] = c.split_at(&sb);
        l
    };
    if sa.is_int(0) {
        left
    } else {
        let [_, r] = left.split_at(&sa.div(&sb));
        r
    }
}

/// `x` rounded down, at least 0.
fn down0(x: f64) -> f64 {
    let d = down(x);
    if d > 0.0 { d } else { 0.0 }
}

/// The greatest of `a` (none a NaN), by compare and select.
fn greatest(a: [f64; 3]) -> f64 {
    a.into_iter().fold(0.0, |m, x| if x > m { x } else { m })
}

/// The length of `a`, scaled by its greatest part so that no square
/// underflows.
fn norm(a: [f64; 3]) -> f64 {
    let m = greatest(a.map(f64::abs));
    if m > 0.0 && m < f64::INFINITY {
        let s: f64 = a.iter().map(|x| (x / m) * (x / m)).sum();
        m * s.sqrt()
    } else {
        m
    }
}

/// An upper bound of the length of `a` (parts `≥ 0`), scaled as [`norm`].
fn norm_up(a: [f64; 3]) -> f64 {
    let m = greatest(a);
    if m > 0.0 && m < f64::INFINITY {
        let s = a
            .iter()
            .fold(0.0, |s, &x| add_up(s, mul_up(div_up(x, m), div_up(x, m))));
        mul_up(m, up(s.sqrt()))
    } else if a.iter().any(|x| x.is_nan()) {
        f64::INFINITY
    } else {
        m
    }
}

/// A lower bound of the length of `a` (parts `≥ 0`), scaled as [`norm`].
fn norm_down(a: [f64; 3]) -> f64 {
    let m = greatest(a);
    if m > 0.0 && m < f64::INFINITY {
        let s = a.iter().fold(0.0, |s, &x| {
            let r = down0(x / m);
            down0(s + down0(r * r))
        });
        // At least `m` itself: its own part's ratio is exactly 1.
        let n = down0(m * down0(s.sqrt()));
        if n > m { n } else { m }
    } else {
        0.0
    }
}

/// The bounds of `|S(p) − C|` on one cell (module docs).
fn direct<S: LevelScalar>(
    tile: &Tile,
    i: i64,
    j: i64,
    p: &HomCurve<S, 2>,
    c: &HomCurve<S, 3>,
    like: &S,
) -> Eval {
    let [pu, pv] = p.coords();
    let w = p.weights();
    // Tile indices below 2⁵² in magnitude (`tile_of`): exact in `f64`.
    let shift = |x: &Poly<S>, k: i64| x.sub(&w.scale(&S::from_f64(k as f64, like)));
    let (u, v) = (shift(pu, i), shift(pv, j));
    let basis = |x: &Poly<S>| -> [Poly<S>; 3] {
        let y = w.sub(x);
        let two = S::int(2, like);
        [
            y.mul(&y).expect("degree 4"),
            x.mul(&y).expect("degree 4").scale(&two),
            x.mul(x).expect("degree 4"),
        ]
    };
    let (bu, bv) = (basis(&u), basis(&v));
    let net = tile.net();
    // The tile's homogeneous coefficient `m` of an affine control point:
    // `w·x` at the level ([`Scalar::product`]), or the weight.
    let coef = |p: [f64; 4], m: usize| {
        if m == 3 {
            S::from_f64(p[3], like)
        } else {
            S::product(p[m], p[3], like)
        }
    };
    // The nine products `Bₖ(U')·Bₗ(V')`, shared by the four coordinates.
    let products: Vec<Poly<S>> = bu
        .iter()
        .flat_map(|a| bv.iter().map(move |b| a.mul(b).expect("degree 8")))
        .collect();
    let h: [Poly<S>; 4] = std::array::from_fn(|m| {
        let mut sum: Option<Poly<S>> = None;
        for (kl, ab) in products.iter().enumerate() {
            let term = ab.scale(&coef(net[kl], m));
            sum = Some(match sum {
                Some(s) => s.add(&term),
                None => term,
            });
        }
        sum.expect("nine terms")
    });
    let wc = c.weights();
    let den = h[3].mul(wc).expect("degree 10");
    let num: [Poly<S>; 3] = std::array::from_fn(|m| {
        h[m].mul(wc)
            .expect("degree 10")
            .sub(&c.coords()[m].mul(&h[3]).expect("degree 10"))
    });
    // The certified bound.
    let den_lo = den.hull().lo;
    let bound = div_up(norm_up(num.each_ref().map(|n| n.hull().abs_max())), den_lo);
    let bound = if bound < f64::INFINITY {
        bound
    } else {
        f64::INFINITY
    };
    // From the values alone.
    let least = den
        .coefs()
        .iter()
        .map(LevelScalar::approx)
        .fold(f64::INFINITY, |a, x| if x < a { x } else { a });
    let most = num.each_ref().map(|n| {
        n.coefs()
            .iter()
            .map(|x| x.approx().abs())
            .fold(0.0, |a, x| if x > a { x } else { a })
    });
    let value = if least > 0.0 {
        norm(most) / least
    } else {
        f64::INFINITY
    };
    let value = if value < f64::INFINITY {
        value
    } else {
        f64::INFINITY
    };
    // At the ends: the first and last coefficients.
    let last = den.coefs().len() - 1;
    let (mut ends, mut lower) = (0.0, 0.0);
    for at in [0, last] {
        let x = num.each_ref().map(|n| &n.coefs()[at]);
        let d = &den.coefs()[at];
        let e = norm(x.map(LevelScalar::approx)) / d.approx();
        if e > ends {
            ends = e;
        }
        let least = x.map(|x| match x.enclose() {
            [lo, _] if lo > 0.0 => lo,
            [_, hi] if hi < 0.0 => -hi,
            _ => 0.0,
        });
        let d_hi = d.enclose()[1];
        if d_hi > 0.0 && d_hi < f64::INFINITY {
            let l = down0(norm_down(least) / d_hi);
            if l > lower {
                lower = l;
            }
        }
    }
    Eval {
        bound,
        value,
        ends,
        lower,
    }
}

/// Hooks for the unit table's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times seam cells per
/// precision level and whole seam passes against the units charged for
/// them, and the fixtures it shares with this module's tests. Not part
/// of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;
    use crate::geom::form::PlaneKey;
    use crate::geom::lattice::{CELL, Lattice};
    use crate::infra::budget::{Budget, Pass, Work};

    /// Why [`cells`] can't run.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CellsError {
        /// The coedge has no seam bound.
        Seam(SeamError),
        /// No precision level of this index: at least `LEVELS`.
        Level(u8),
        /// Deeper than the seam bound halves a piece: above
        /// [`MAX_SUBDIV_DEPTH`].
        Depth(u32),
    }

    /// Evaluates every cell `depth` halvings deep of every piece of the
    /// coedge (`2^depth` per piece) at precision level `level`, as the
    /// seam bound does; the number of cells. A level the schedule doesn't
    /// have, or a depth past [`MAX_SUBDIV_DEPTH`], is an error.
    pub fn cells(
        surface: &Surface,
        curve: &Curve3D,
        pcurve: &PCurve,
        level: u8,
        depth: u32,
    ) -> Result<u64, CellsError> {
        if level >= LEVELS {
            return Err(CellsError::Level(level));
        }
        if depth > MAX_SUBDIV_DEPTH {
            return Err(CellsError::Depth(depth));
        }
        let pieces = pieces(surface, curve, pcurve).map_err(CellsError::Seam)?;
        let mut n = 0u64;
        let mut lower = 0.0;
        for (piece, p) in pieces.iter().enumerate() {
            for k in 0..1u64 << depth {
                let cell = Cell {
                    piece,
                    k,
                    depth,
                    level,
                };
                let e = eval(curve, pcurve, p, cell);
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

    /// The seam bounds of `coedges` on `surface` towards `target`, in one
    /// pass on a budget of `budget` units: the bounds, `None` if the pass
    /// ran out or failed, and the units used.
    pub fn pass(
        surface: &Surface,
        coedges: &[(Curve3D, PCurve)],
        target: f64,
        budget: u64,
    ) -> (Option<Vec<SeamBound>>, u64) {
        let mut work = Work::new(&Budget::new(budget));
        let pass = Pass {
            stage: "seam",
            rung: 0,
        };
        let out = work.pass(pass, coedges, |(c, p), ledger| {
            seam_bound(surface, c, p, target, ledger)
        });
        (out.ok(), work.used())
    }

    /// The plane `z = 0` through the origin on its one base tile, 4·10⁶
    /// mm across.
    pub fn base_plane() -> Surface {
        let key = PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).expect("a plane");
        let lat = Lattice::new(key.into()).expect("a lattice");
        let rect = lat.half(0).expect("the base tile");
        Surface::new(lat, rect).expect("a surface")
    }

    /// The p-curve of `c` on `s`, a plane on its base tile: each control
    /// point mapped by the plane's frame, `(x − o)·axis / CELL + ½`, its
    /// weight kept.
    pub fn on_plane(s: &Surface, c: &Curve3D) -> PCurve {
        let lat = s.lattice();
        let (o, [u, v, _]) = (lat.origin(), lat.axes());
        let spans = c
            .spans()
            .iter()
            .map(|sp| {
                sp.map(|h| {
                    let d = [h[0] - o[0], h[1] - o[1], h[2] - o[2]];
                    let along =
                        |a: [f64; 3]| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
                    [along(u), along(v), h[3]]
                })
            })
            .collect();
        PCurve::new(spans, c.knots().to_vec()).expect("a p-curve")
    }

    /// The quarter circle of radius 50 from `(50, 0, 0)` to `(0, 50, 0)`,
    /// reparametrised to end weights `w` and `1/w` (the middle's `√2/2`).
    pub fn quarter(w: f64) -> Curve3D {
        let (r, mid) = (50.0, std::f64::consts::FRAC_1_SQRT_2);
        Curve3D::new(
            vec![[[r, 0.0, 0.0, w], [r, r, 0.0, mid], [0.0, r, 0.0, 1.0 / w]]],
            vec![0.0, 1.0],
        )
        .expect("a curve")
    }

    /// The quarter circles with end weights 1, 3 and 64 on the base plane
    /// `s` ([`base_plane`]), lying exactly on it, and the one with
    /// weights 3 whose p-curve's middle control point is moved by
    /// `2·10⁻⁷` of the tile each way.
    pub fn arcs(s: &Surface) -> Vec<(Curve3D, PCurve)> {
        let mut out: Vec<_> = [1.0, 3.0, 64.0]
            .map(|w| {
                let c = quarter(w);
                let p = on_plane(s, &c);
                (c, p)
            })
            .into();
        let c = quarter(3.0);
        let p = on_plane(s, &c)
            .perturbed(&[[2e-7, -2e-7]])
            .expect("a moved p-curve");
        out.push((c, p));
        out
    }
}

/// [`seam_bound`] alone, in a pass of its own on a fresh budget of
/// `budget` units.
#[cfg(test)]
pub(crate) fn seam_bound_alone(
    surface: &Surface,
    curve: &Curve3D,
    pcurve: &PCurve,
    target: f64,
    budget: u64,
) -> Result<SeamBound, crate::infra::budget::Halt<SeamError>> {
    use crate::infra::budget::{Budget, Pass, Work};
    let pass = Pass {
        stage: "seam",
        rung: 0,
    };
    let mut work = Work::new(&Budget::new(budget));
    work.pass(pass, &[()], |_, ledger| {
        seam_bound(surface, curve, pcurve, target, ledger)
    })
    .map(|b| b[0])
}

#[cfg(test)]
mod tests;
