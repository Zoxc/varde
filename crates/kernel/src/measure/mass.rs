//! Certified mass properties of a body: its volume, its area and its
//! centroid, each an enclosure ([`Bounded`]: the true value within the
//! error of the value), from the faces' surfaces over their trimmed
//! regions and the ribbons that close them.
//!
//! **Flux integrals.** By the divergence theorem the volume is
//! `⅓·∮ (x − o)·n dA` over the body's boundary, and the first moment
//! `∫ (x − o) dV` is `¼·∮ (x − o)·((x − o)·n) dA`, for any origin `o`
//! (here the centre of the box of the edges' control points, which keeps
//! the integrands small). On a face, a tile's rational patch `x = P/W`
//! gives `(x − o)·(x_u × x_v) = det(P', P'_u, P'_v)/W³` with `P' = P −
//! o·W` and `P'_u` its homogeneous partial (the terms in `P'` itself
//! cancel), and `|x_u × x_v| = |N|/W³` with `N` the normal numerator
//! ([`HomPatch::normal_numerator`]); so the volume's integrand is
//! rational of degrees `(5, 5)` over `W³`, a moment's `P'ₖ·det` over
//! `W⁴`, and the area's a square root over `W³`, its factors at a
//! singular side (a pole row) divided out exactly
//! ([`SingularSides::quotient`](varde_decide::bezier::normal::SingularSides::quotient)).
//!
//! **Green's form per tile.** A face's integral over its trimmed region
//! `R` becomes one along its trims, `∬_R f du dv = ∮_∂R G dv` with
//! `∂G/∂u = f`, the trims counted as their coedges run: they wind `+1`
//! round the inside of an unflipped face and `−1` round a flipped one, as
//! the outward normal turns with `flipped`, so a flux comes out with the
//! face's orientation (the area is turned back by `flipped`). The face's
//! UV box (its trims' control values) is cut by the tile lines into
//! **cells**, each a tile's share of the box, cut again into `4 × 4`
//! equal cells along each direction some tile's weights vary along
//! (unless a trim is a Q or G span, see below). The tile's patch is
//! restricted to each cell exactly (the stored net as rationals, split
//! there, moved by `−o·W`, then each value rounded once with its rounding
//! carried). On a cell, in its own coordinates `(s, r) ∈ [0, 1]²`, the
//! integrand is replaced by a polynomial with a **certified residual**:
//! a rational `f = num/Wᵏ` by `num'·A`, `num'` the numerator at its
//! least degrees within rounding (`|num − num'| ≤ δ`, certified) and `A`
//! a fit of `1/Wᵏ` (interpolation at Chebyshev–Lobatto points in `f64`:
//! any polynomial will do), so that `|f − num'·A| ≤ (δ + max|num'|·
//! max|1 − A·Wᵏ|) / (min W)ᵏ`; the area's `φ·√a/W³` by `φ·S·A`, `S` a fit
//! of `√a` with `|√a − S| = |a − S²|/(√a + S)`. Maxima and minima come
//! from Bernstein coefficients, a residual's halved (up to four times)
//! where its hull alone doesn't meet the target. Each fit takes the
//! least degrees found from the last cell's, raising one direction at a
//! time. The polynomial's antiderivative in `s` is exact (Bernstein
//! coefficients summed), and along a row of cells the antiderivatives
//! are joined end to end, each cell's starting from the integrals of the
//! cells left of it, so `G` is one continuous function on the row; a row
//! boundary is a line of constant `v`, where `dv = 0`, so rows need not
//! join. The residuals sum to the 2D error: the region covers each cell
//! at most once, of area 1 in the cell's coordinates.
//!
//! **Along the trims.** A straight or iso span is integrated along the
//! straight segment between its end control values, which is its image,
//! cut exactly (in rationals) where it crosses the cells' lines: on each
//! piece the line integral of `G` is a polynomial of the segment's
//! parameter, integrated exactly (an iso piece by the antiderivative of
//! `G` along it, another by the 32-point Gauss–Legendre rule on
//! certified nodes, [`gauss32_bounded`], exact up to degree 63). A Q or G
//! span lies in one cell (the tiles' shares are then left whole: a cut
//! of a curved span would fall between floats) and gives
//! `G(s(t), r(t))·r'(t) = N/(D_u^a·D_v^c)`: the 32-point sum encloses
//! the exact rule's, so its error is at most twice the distance, in the
//! maximum, from the integrand to **some** polynomial of degree 63. One
//! is `N·∏ qᵢ^pᵢ` with `qᵢ` the truncated series
//! `(1/cᵢ)·Σ_{l<L} (1 − Dᵢ/cᵢ)ˡ`, for which `1 − qᵢ·Dᵢ = (1 − Dᵢ/cᵢ)ᴸ`
//! exactly, so the distance is at most `max|N|·(∏(1 + βᵢᴸ)^pᵢ − 1) /
//! ∏ min Dᵢ^pᵢ` with `βᵢ = max|1 − Dᵢ/cᵢ|`; the span is halved until that
//! meets the target. A composition past the forms' degree limit is
//! bounded by `max|G|` times the variation of `r` along the span (value
//! 0): certified, but loose.
//!
//! **Ribbons.** A face's boundary as the operand sees it is its ribbons
//! too, one per coedge, at most the coedge's seam bound `w` wide, so a
//! ribbon's area is at most `w·(ℓ(S∘p) + ℓ(C))/2`; `ℓ(C)` is at most the edge
//! curve's control polygon (a conic arc lies in its triangle) and
//! `ℓ(S∘p)` at most the surface's speed bounds times the variation of
//! the p-curve's coordinates. The ribbons' flux is the error they add:
//! their area times `⅓·R` for the volume, `¼·R²` for each moment, `R`
//! the farthest the ribbon gets from `o`; their area itself for the area.
//!
//! **Rounding and charges.** Everything after the exact restriction runs
//! at the first precision level, in [`Bounded`] arithmetic, whose
//! rounding the results carry; the polynomial fits' residuals and the
//! Gauss bounds are truncation. Every fit, composition and Gauss piece
//! is charged to the face's ledger before it runs, scaled from the
//! extremum query's units by its size; the faces are the items of one
//! pass, summed in face order.

use malachite::Rational;
use varde_decide::bezier::product::{cross, dot};
use varde_decide::bezier::{Dir, HomPatch, Poly, Scalar, Side, Sides, Tensor};
use varde_decide::num::bounded::{Bounded, add_up, div_up, down, mul_up, up};
use varde_decide::num::exact::rat as q;
use varde_decide::num::storage::{nearest, store_exact};
use varde_decide::num::trig;

use super::quadrature::gauss32_bounded;
use crate::geom::pcurve::{Kind, PSpan};
use crate::geom::surface::Surface;
use crate::geom::tile::Tile;
use crate::infra::budget::{Budget, Ledger, Pass, Step, Stop, Work, extremum_cell_units};
use crate::infra::error::{Invalid, KernelError};
use crate::topo::brep::{BodyKind, Brep, CoedgeId, FaceId};

#[cfg(test)]
mod tests;

type B = Bounded<f64>;

/// A body's mass properties, each certified: the true value lies within
/// [`Bounded::err`] of the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProps {
    /// The volume (mm³).
    pub volume: Bounded<f64>,
    /// The area of the boundary (mm²).
    pub area: Bounded<f64>,
    /// The centroid of the volume, `None` where the volume isn't
    /// certainly positive (the empty solid).
    pub centroid: Option<[Bounded<f64>; 3]>,
}

/// A solid's volume, area and centroid (module docs), on a budget of
/// `budget`. A sheet has no volume: `Invalid(NotClosed)`. A store whose
/// loops don't close or whose p-curve spans don't each lie in one tile
/// of its face's surface is `Invalid(Check)` with the invariant it
/// breaks; running out of the budget is `OutOfBudget`.
pub fn mass_props(brep: &Brep, budget: &Budget) -> Result<MassProps, KernelError> {
    if brep.kind() != BodyKind::Solid {
        return Err(KernelError::Invalid(Invalid::NotClosed));
    }
    let set = [
        Integrand::Volume,
        Integrand::Moment(0),
        Integrand::Moment(1),
        Integrand::Moment(2),
        Integrand::Area,
    ];
    let o = centre(brep);
    let sums = faces_summed(brep, o, &set, budget)?;
    let volume = sums[0] / B::exact(3.0);
    let area = sums[4];
    let centroid = (volume.v > volume.err()).then(|| {
        [0, 1, 2].map(|k| {
            let m = sums[1 + k] / B::exact(4.0);
            B::exact(o[k]) + m / volume
        })
    });
    Ok(MassProps {
        volume,
        area,
        centroid,
    })
}

/// The area of a body's faces, a sheet's as a solid's (module docs).
pub fn area(brep: &Brep, budget: &Budget) -> Result<Bounded<f64>, KernelError> {
    let o = centre(brep);
    Ok(faces_summed(brep, o, &[Integrand::Area], budget)?[0])
}

/// What a face integral integrates (module docs), about the origin `o`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Integrand {
    /// The flux `∫ (x − o)·n dA`: three times the face's share of the
    /// volume.
    Volume,
    /// The flux `∫ (xₖ − oₖ)·((x − o)·n) dA`: four times the face's
    /// share of the first moment about `o` along axis `k`.
    Moment(usize),
    /// The area `∫ dA`.
    Area,
}

/// Why a face can't be measured: the store breaks an invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MassError {
    /// A loop of the face doesn't close within the store's coedges, or
    /// the face's loops don't end.
    Loop(FaceId),
    /// A span of the coedge's p-curve isn't in one tile of its face's
    /// surface.
    Span(CoedgeId),
}

impl From<MassError> for KernelError {
    fn from(e: MassError) -> Self {
        let inv = match e {
            MassError::Loop(_) => 1,
            MassError::Span(_) => 2,
        };
        KernelError::Invalid(Invalid::Check { inv })
    }
}

/// The integrals `set` summed over every face, in face order, the faces
/// the items of one pass on `budget`.
fn faces_summed(
    brep: &Brep,
    o: [f64; 3],
    set: &[Integrand],
    budget: &Budget,
) -> Result<Vec<B>, KernelError> {
    let faces: Vec<FaceId> = (0..brep.face_count())
        .map(|f| FaceId(u32::try_from(f).expect("ids below NONE")))
        .collect();
    let mut work = Work::new(budget);
    let pass = Pass {
        stage: "measure",
        rung: 0,
    };
    let per_face = work
        .pass(pass, &faces, |&f, ledger| {
            face_integrals(brep, f, o, set, ledger)
        })
        .map_err(KernelError::from)?;
    let mut sums = vec![B::exact(0.0); set.len()];
    for face in per_face {
        for (s, v) in sums.iter_mut().zip(face) {
            *s = *s + v;
        }
    }
    Ok(sums)
}

/// The centre of the box of every edge curve's control points, the
/// origin of the integrands; the world origin for a body without edges.
fn centre(brep: &Brep) -> [f64; 3] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for e in 0..brep.edge_count() {
        let e = crate::topo::brep::EdgeId(u32::try_from(e).expect("ids below NONE"));
        for span in brep.edge_spans(e) {
            for p in span {
                for k in 0..3 {
                    if p[k] < lo[k] {
                        lo[k] = p[k];
                    }
                    if p[k] > hi[k] {
                        hi[k] = p[k];
                    }
                }
            }
        }
    }
    std::array::from_fn(|k| {
        if lo[k] <= hi[k] {
            lo[k] * 0.5 + hi[k] * 0.5
        } else {
            0.0
        }
    })
}

/// A face's trim: a coedge, whether it runs against its p-curve, and the
/// p-curve's spans.
struct Trim<'a> {
    coedge: CoedgeId,
    reversed: bool,
    spans: &'a [PSpan],
}

/// The face's trims, loop by loop in order.
fn trims(brep: &Brep, face: FaceId) -> Result<Vec<Trim<'_>>, MassError> {
    let mut out = Vec::new();
    let mut l = brep.first_loop(face);
    let mut loops = 0usize;
    while !l.is_none() {
        loops += 1;
        if loops > brep.loop_count() || l.index() >= brep.loop_count() {
            return Err(MassError::Loop(face));
        }
        let first = brep.first_coedge(l);
        let mut c = first;
        let mut n = 0usize;
        loop {
            if c.is_none() || c.index() >= brep.coedge_count() || n >= brep.coedge_count() {
                return Err(MassError::Loop(face));
            }
            out.push(Trim {
                coedge: c,
                reversed: brep.reversed(c),
                spans: brep.coedge_pcurve(c).spans(),
            });
            n += 1;
            c = brep.next(c);
            if c == first {
                break;
            }
        }
        l = brep.next_loop(l);
    }
    Ok(out)
}

/// A cell: part of a tile's share of the face's UV box (the whole share,
/// or one of its `2^a × 2^b` equal parts), in global UV, with what the
/// face's integrals read there.
struct Cell {
    /// The cell's corners in global UV, exactly.
    lo: [Rational; 2],
    hi: [Rational; 2],
    /// Per integrand: `G` on the cell in its coordinates, the row's
    /// antiderivative of the fitted polynomial (module docs).
    g: Vec<Tensor<B>>,
    /// Per integrand: the fit's certified residual bound.
    resid: Vec<f64>,
    /// Bounds of the surface's speed along `s` and `r`, `|x_s|` and
    /// `|x_r|` over the cell.
    speed: [f64; 2],
}

impl Cell {
    /// Whether the closed cell holds the point.
    fn holds(&self, p: &[Rational; 2]) -> bool {
        (0..2).all(|k| self.lo[k] <= p[k] && p[k] <= self.hi[k])
    }

    /// Coordinate `k` of a global value in the cell's coordinates.
    fn local(&self, k: usize, x: &Rational) -> Rational {
        (x - &self.lo[k]) / (&self.hi[k] - &self.lo[k])
    }
}

/// The face's integrals `set` (module docs), each with its fits',
/// quadratures' and ribbons' errors, charged to `ledger`.
pub(crate) fn face_integrals(
    brep: &Brep,
    face: FaceId,
    o: [f64; 3],
    set: &[Integrand],
    ledger: &mut Ledger<'_>,
) -> Result<Vec<B>, Stop<MassError>> {
    let trims = trims(brep, face).map_err(Stop::Failed)?;
    let surface = brep.face_surface(face);
    let mut sums = vec![B::exact(0.0); set.len()];
    let first = trims.first().map_or(CoedgeId::NONE, |t| t.coedge);
    let Some((lo, hi)) = uv_box(&trims) else {
        return Err(Stop::Failed(MassError::Span(first)));
    };
    let curved = trims
        .iter()
        .flat_map(|t| t.spans)
        .any(|s| matches!(s.kind(), Kind::Q | Kind::G));
    let cells = if lo[0] < hi[0] && lo[1] < hi[1] {
        cells(surface, lo, hi, curved, o, set, ledger)?
            .ok_or(Stop::Failed(MassError::Span(first)))?
    } else {
        // No area: every integral is zero up to the ribbons.
        Vec::new()
    };
    let lines = grid_lines(&cells);
    let mut ribbon_area = Vec::with_capacity(trims.len());
    for t in &trims {
        // Without cells (a face of no area) nothing bounds the length of
        // the ribbon's side on the surface.
        let mut variation = if cells.is_empty() { f64::INFINITY } else { 0.0 };
        for span in t.spans {
            if cells.is_empty() {
                continue;
            }
            for (cell, sp) in
                pieces(span, &cells, &lines).ok_or(Stop::Failed(MassError::Span(t.coedge)))?
            {
                let cell = &cells[cell];
                variation = add_up(
                    variation,
                    add_up(
                        mul_up(cell.speed[0], sp.variation()[0]),
                        mul_up(cell.speed[1], sp.variation()[1]),
                    ),
                );
                for (k, g) in cell.g.iter().enumerate() {
                    let mut v = along(g, &sp, ledger)?;
                    if t.reversed {
                        v = -v;
                    }
                    sums[k] = sums[k] + v;
                }
            }
        }
        ribbon_area.push(ribbon(brep, t.coedge, variation, o));
    }
    // The fits' residuals, once per cell.
    for cell in &cells {
        for (s, &e) in sums.iter_mut().zip(&cell.resid) {
            *s = s.with_trunc(e);
        }
    }
    let flipped = brep.flipped(face);
    for (s, which) in sums.iter_mut().zip(set) {
        if *which == Integrand::Area && flipped {
            *s = -*s;
        }
        for &(area, reach) in &ribbon_area {
            let e = match which {
                Integrand::Volume => mul_up(area, reach),
                Integrand::Moment(_) => mul_up(area, mul_up(reach, reach)),
                Integrand::Area => area,
            };
            *s = s.with_trunc(e);
        }
    }
    Ok(sums)
}

/// The box of the trims' control values in global UV, `None` where one
/// isn't a number.
fn uv_box(trims: &[Trim<'_>]) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for t in trims {
        for s in t.spans {
            let [l, h] = s.hull();
            for k in 0..2 {
                if l[k].is_nan() || h[k].is_nan() {
                    return None;
                }
                if l[k] < lo[k] {
                    lo[k] = l[k];
                }
                if h[k] > hi[k] {
                    hi[k] = h[k];
                }
            }
        }
    }
    (lo[0] <= hi[0] && lo[1] <= hi[1]).then_some((lo, hi))
}

/// Below `2⁵²` in magnitude, UV values have exact integer neighbours.
const UV_LIMIT: f64 = 4_503_599_627_370_496.0;

/// The halvings of a tile's share of the box in a direction its weights
/// vary along (module docs): `2^SPLIT` cells across it.
const SPLIT: u32 = 2;

/// Whether a tile's weights vary along `dir`.
fn varies(tile: &Tile, dir: Dir) -> bool {
    let w = tile.patch().weights();
    let (m, n) = w.degrees();
    let at = |i: usize, j: usize| match dir {
        Dir::U => w.get(i, j).v.to_bits(),
        Dir::V => w.get(j, i).v.to_bits(),
    };
    let (a, b) = match dir {
        Dir::U => (m, n),
        Dir::V => (n, m),
    };
    (0..=b).any(|j| (1..=a).any(|i| at(i, j) != at(0, j)))
}

/// The cells of the box `[lo, hi]` (positive in both directions) on
/// `surface`, row by row, each with its fitted antiderivatives joined
/// along the row; `None` where the box leaves the surface's tiles. A
/// tile's share is cut into `2^SPLIT` parts along each direction some
/// tile's weights vary along, unless a trim is a Q or G span
/// (`curved`), which is never cut.
fn cells(
    surface: &Surface,
    lo: [f64; 2],
    hi: [f64; 2],
    curved: bool,
    o: [f64; 3],
    set: &[Integrand],
    ledger: &mut Ledger<'_>,
) -> Result<Option<Vec<Cell>>, Stop<MassError>> {
    if !(lo.iter().chain(&hi).all(|x| x.abs() < UV_LIMIT)) {
        return Ok(None);
    }
    // Tiles `⌊lo⌋ ..= ⌈hi⌉ − 1` in each direction: exact as `f64`s.
    let range = |k: usize| (lo[k].floor() as i64, hi[k].ceil() as i64 - 1);
    let ((i0, i1), (j0, j1)) = (range(0), range(1));
    let mut tiles = Vec::new();
    for j in j0..=j1 {
        for i in i0..=i1 {
            let Some(tile) = surface.tile(i, j) else {
                return Ok(None);
            };
            tiles.push(tile);
        }
    }
    let split = |dir: Dir| {
        if !curved && tiles.iter().any(|t| varies(t, dir)) {
            1u32 << SPLIT
        } else {
            1
        }
    };
    let (nu, nv) = (split(Dir::U), split(Dir::V));
    let mut out = Vec::new();
    let mut lus = Lus::default();
    let mut fits = Fits::default();
    // The tile's share `[max(lo, i), min(hi, i + 1)]` of the box.
    let lower = |x: f64, a: f64| q(pick(x > a, x, a));
    let upper = |x: f64, a: f64| q(pick(x < a, x, a));
    let cut = |a: &Rational, b: &Rational, k: u32, n: u32| {
        a + (b - a) * Rational::from(k) / Rational::from(n)
    };
    for j in j0..=j1 {
        let fj = j as f64;
        let (v_lo, v_hi) = (lower(lo[1], fj), upper(hi[1], fj + 1.0));
        for qv in 0..nv {
            let (r_lo, r_hi) = (cut(&v_lo, &v_hi, qv, nv), cut(&v_lo, &v_hi, qv + 1, nv));
            let mut carried: Vec<Option<Poly<B>>> = vec![None; set.len()];
            for i in i0..=i1 {
                let fi = i as f64;
                let tile = tiles[((j - j0) * (i1 - i0 + 1) + (i - i0)) as usize];
                let (u_lo, u_hi) = (lower(lo[0], fi), upper(hi[0], fi + 1.0));
                for pu in 0..nu {
                    let (s_lo, s_hi) = (cut(&u_lo, &u_hi, pu, nu), cut(&u_lo, &u_hi, pu + 1, nu));
                    let clo = [s_lo, r_lo.clone()];
                    let chi = [s_hi, r_hi.clone()];
                    let forms = Forms::new(tile, [i, j], &clo, &chi, o);
                    fits.next_cell();
                    let mut g = Vec::with_capacity(set.len());
                    let mut resid = Vec::with_capacity(set.len());
                    for (k, which) in set.iter().enumerate() {
                        let (p, e) = fits.integrand(&forms, *which, &mut lus, ledger)?;
                        let qu = antiderivative_u(&p);
                        let (m, n) = qu.degrees();
                        let last = Poly::new((0..=n).map(|c| *qu.get(m, c))).expect("a line");
                        let joined = match &carried[k] {
                            Some(c) => {
                                let row = Tensor::new(0, c.degree(), c.coefs().iter().copied())
                                    .expect("a line's degree");
                                qu.add(&row)
                            }
                            None => qu,
                        };
                        carried[k] = Some(match carried[k].take() {
                            Some(c) => c.add(&last),
                            None => last,
                        });
                        g.push(joined);
                        resid.push(e);
                    }
                    out.push(Cell {
                        lo: clo,
                        hi: chi,
                        g,
                        resid,
                        speed: forms.speed(),
                    });
                }
            }
        }
    }
    Ok(Some(out))
}

/// `a` if `c`, else `b`.
fn pick(c: bool, a: f64, b: f64) -> f64 {
    if c { a } else { b }
}

/// An exact rational rounded once to an `f64`, its rounding carried; not
/// certified past the `f64` range.
fn stored(x: &Rational) -> B {
    match store_exact(x) {
        Ok(s) => B::new(s.v, 0.0, s.err),
        Err(_) => B::new(nearest(x), f64::INFINITY, f64::INFINITY),
    }
}

/// The forms of a cell (module docs): its patch in cell coordinates,
/// moved by `−o·W`, and what the integrands are made of.
struct Forms {
    patch: HomPatch<B>,
    /// `det(P', P'_s, P'_r)`.
    det: Tensor<B>,
    /// `W`, `W²`, `W³` and `W⁴`.
    w: [Tensor<B>; 4],
    /// A lower bound of `W` on the cell, positive where certified.
    w_min: f64,
    /// `m·m`, `m` the normal numerator with the singular sides' factors
    /// divided out, and those sides; `None` where it can't be formed.
    area: Option<(Tensor<B>, Sides)>,
}

impl Forms {
    fn new(
        tile: &Tile,
        ij: [i64; 2],
        lo: &[Rational; 2],
        hi: &[Rational; 2],
        o: [f64; 3],
    ) -> Forms {
        let one = Rational::from(1u32);
        let mut exact = tile.exact();
        let mut sides = Sides::NONE;
        for (k, dir) in [Dir::U, Dir::V].into_iter().enumerate() {
            let base = Rational::from(ij[k]);
            let (a, b) = (&lo[k] - &base, &hi[k] - &base);
            let (s0, s1) = match dir {
                Dir::U => (Side::U0, Side::U1),
                Dir::V => (Side::V0, Side::V1),
            };
            if a == 0u32 {
                sides = sides.with(s0);
            }
            if b == one {
                sides = sides.with(s1);
            }
            if a > 0u32 {
                let [_, upper] = exact.split_at(dir, &a);
                exact = upper;
            }
            if b < one {
                let t = (&b - &a) / (&one - &a);
                let [lower, _] = exact.split_at(dir, &t);
                exact = lower;
            }
        }
        let sides = Sides::from_iter(sides.iter().filter(|s| tile.singular().contains(*s)));
        let t = exact.tensors();
        let w = &t[3];
        let moved: [Tensor<Rational>; 3] = std::array::from_fn(|k| t[k].sub(&w.scale(&q(o[k]))));
        let round = |t: &Tensor<Rational>| {
            let (m, n) = t.degrees();
            Tensor::new(m, n, t.coefs().iter().map(stored)).expect("the same degrees")
        };
        let [mx, my, mz] = &moved;
        let patch = HomPatch::from_tensors([round(mx), round(my), round(mz), round(w)])
            .expect("one pair of degrees");
        let [px, py, pz, pw] = patch.tensors();
        let p = [px.clone(), py.clone(), pz.clone()];
        let du = patch.derivative(Dir::U);
        let dv = patch.derivative(Dir::V);
        let [ux, uy, uz, _] = du.tensors();
        let [vx, vy, vz, _] = dv.tensors();
        let pu = [ux.clone(), uy.clone(), uz.clone()];
        let pv = [vx.clone(), vy.clone(), vz.clone()];
        let det = cross(&pu, &pv)
            .and_then(|c| dot(&p, &c))
            .expect("degrees (5, 5)");
        let w1 = pw.clone();
        let w2 = w1.mul(&w1).expect("degree 4");
        let w3 = w2.mul(&w1).expect("degree 6");
        let w4 = w2.mul(&w2).expect("degree 8");
        let w_min = w1.hull().lo;
        let area = patch.normal_numerator().and_then(|n| {
            let m = if sides.is_empty() {
                n
            } else {
                exact.singular_sides(sides)?.quotient(&n)?
            };
            Some((dot(&m, &m)?, sides))
        });
        Forms {
            patch,
            det,
            w: [w1, w2, w3, w4],
            w_min,
            area,
        }
    }

    /// Bounds of `|x_s|` and `|x_r|` over the cell: the partial
    /// numerators' coefficient bounds over `W²`.
    fn speed(&self) -> [f64; 2] {
        let w2 = pow_down(self.w_min, 2);
        [Dir::U, Dir::V].map(|d| match self.patch.partial_numerator(d) {
            Some(n) => {
                let mut s = 0.0;
                for t in &n {
                    let m = t.hull().abs_max();
                    s = add_up(s, mul_up(m, m));
                }
                div_up(up(s.sqrt()), w2)
            }
            None => f64::INFINITY,
        })
    }
}

/// `x` raised to `k`, rounded downwards: a lower bound of the power of
/// any value at least `x ≥ 0`; 0 for a negative `x`.
fn pow_down(x: f64, k: u32) -> f64 {
    if x.is_nan() || x <= 0.0 {
        return 0.0;
    }
    let mut p = 1.0;
    for _ in 0..k {
        p = down(p * x);
    }
    p
}

/// `x` raised to `k`, rounded upwards.
fn pow_up(x: f64, k: u32) -> f64 {
    let mut p = 1.0;
    for _ in 0..k {
        p = mul_up(p, x);
    }
    p
}

/// The highest fitted degree in each direction.
const MAX_FIT: usize = 24;

/// The target of a fit of `1/Wᵏ`: the relative residual `|1 − A·Wᵏ|`,
/// above the rounding the residual's own coefficients carry.
const DEN_TAU: f64 = 1.0 / 4_398_046_511_104.0; // 2⁻⁴²

/// The target of a fit of `√a`, relative to its largest node value.
const ROOT_TAU: f64 = 1.0 / 17_592_186_044_416.0; // 2⁻⁴⁴

/// The deepest halving of a residual for its bound (each halving along
/// the direction of the higher degree).
const RESID_DEPTH: u32 = 4;

/// The next degree tried after `k`.
fn next_degree(k: usize) -> usize {
    let n = if k < 4 { k + 1 } else { k + k.div_ceil(4) };
    if n > MAX_FIT { MAX_FIT } else { n }
}

/// A polynomial fit: its Bernstein form, degrees and certified residual
/// bound.
#[derive(Clone)]
struct Fit {
    p: Tensor<B>,
    k: [usize; 2],
    e: f64,
}

/// The fits of a face's cells: those of the cell at hand (`1/W³`,
/// `1/W⁴` and `√a`, each formed once), and the degrees each ended at on
/// the last cell, where the next cell's search starts.
#[derive(Default)]
struct Fits {
    start: [[usize; 2]; 3],
    cell: [Option<Fit>; 3],
}

impl Fits {
    /// Forgets the last cell's fits, keeping their degrees.
    fn next_cell(&mut self) {
        self.cell = [None, None, None];
    }

    /// The polynomial standing in for the integrand `which` on the cell
    /// and the certified bound of the difference (module docs).
    fn integrand(
        &mut self,
        forms: &Forms,
        which: Integrand,
        lus: &mut Lus,
        ledger: &mut Ledger<'_>,
    ) -> Result<(Tensor<B>, f64), Stop<MassError>> {
        let none = || {
            (
                Tensor::from_f64(0, 0, &[0.0]).expect("a constant"),
                f64::INFINITY,
            )
        };
        let (num, pow) = match which {
            Integrand::Volume => (forms.det.clone(), 3),
            Integrand::Moment(k) => match forms.patch.tensors()[k].mul(&forms.det) {
                Some(n) => (n, 4),
                None => return Ok(none()),
            },
            Integrand::Area => {
                let Some((a, sides)) = &forms.area else {
                    return Ok(none());
                };
                let root = self.root(forms, a, lus, ledger)?;
                let den = self.den(forms, 3, lus, ledger)?;
                let Some(p) = root.p.mul(&den.p).and_then(|p| with_sides(&p, *sides)) else {
                    return Ok(none());
                };
                let s_max = root.p.hull().abs_max();
                let e = add_up(root.e, mul_up(s_max, den.e));
                return Ok((p, div_up(e, pow_down(forms.w_min, 3))));
            }
        };
        let (red, delta) = reduced(&num);
        let den = self.den(forms, pow, lus, ledger)?;
        let Some(p) = red.mul(&den.p) else {
            return Ok(none());
        };
        let e = add_up(delta, mul_up(red.hull().abs_max(), den.e));
        Ok((p, div_up(e, pow_down(forms.w_min, pow))))
    }

    /// The fit `A` of `1/W^pow` with the bound of `|1 − A·W^pow|`.
    fn den(
        &mut self,
        forms: &Forms,
        pow: u32,
        lus: &mut Lus,
        ledger: &mut Ledger<'_>,
    ) -> Result<Fit, Stop<MassError>> {
        let den = &forms.w[pow as usize - 1];
        let slot = if pow == 3 { 0 } else { 1 };
        if let Some(f) = &self.cell[slot] {
            return Ok(f.clone());
        }
        let one = Tensor::from_f64(0, 0, &[1.0]).expect("a constant");
        let fit = search(self.start[slot], ledger, |k| {
            let (a, _) = interpolate(k, lus, |x, y| {
                let (bx, by) = (B::exact(x), B::exact(y));
                1.0 / den.eval(&bx, &by).v
            })?;
            let r = one.sub(&a.mul(den)?);
            Some((a, sup_abs(&r, DEN_TAU, RESID_DEPTH), DEN_TAU))
        })?;
        self.start[slot] = fit.k;
        self.cell[slot] = Some(fit.clone());
        Ok(fit)
    }

    /// The fit `S` of `√a` with the bound of `|√a − S|`.
    fn root(
        &mut self,
        forms: &Forms,
        a: &Tensor<B>,
        lus: &mut Lus,
        ledger: &mut Ledger<'_>,
    ) -> Result<Fit, Stop<MassError>> {
        let _ = forms;
        if let Some(f) = &self.cell[2] {
            return Ok(f.clone());
        }
        let fit = search(self.start[2], ledger, |k| {
            let (s, scale) = interpolate(k, lus, |x, y| {
                let v = a.eval(&B::exact(x), &B::exact(y)).v;
                pick(v > 0.0, v, 0.0).sqrt()
            })?;
            let r = a.sub(&s.mul(&s)?);
            let target = ROOT_TAU * scale;
            Some((s.clone(), sup_root(&r, a, &s, target, RESID_DEPTH), target))
        })?;
        self.start[2] = fit.k;
        self.cell[2] = Some(fit.clone());
        Ok(fit)
    }
}

/// The least-degree fit from `start` meeting its target (module docs):
/// each round raising the degree in the direction whose fit is better,
/// until the target is met, both directions are at [`MAX_FIT`], or,
/// within 1024 times the target, three rounds in a row fail to halve
/// the residual; the best fit found.
/// `fit` gives the polynomial, its residual bound and its target, `None`
/// where it can't be formed; each is charged before it is formed.
fn search(
    start: [usize; 2],
    ledger: &mut Ledger<'_>,
    mut fit: impl FnMut([usize; 2]) -> Option<(Tensor<B>, f64, f64)>,
) -> Result<Fit, Stop<MassError>> {
    let mut run = |k: [usize; 2], ledger: &mut Ledger<'_>| -> Result<(Fit, f64), Stop<MassError>> {
        ledger.charge(Step::Work, fit_units(k))?;
        Ok(match fit(k) {
            Some((p, e, target)) => (Fit { p, k, e }, target),
            None => (
                Fit {
                    p: Tensor::from_f64(0, 0, &[0.0]).expect("a constant"),
                    k,
                    e: f64::INFINITY,
                },
                0.0,
            ),
        })
    };
    let mut k = start;
    let (mut best, mut target) = run(k, ledger)?;
    let mut stall = 0;
    while best.e.is_nan() || best.e > target {
        let mut cand: Option<(Fit, f64)> = None;
        for d in 0..2 {
            if k[d] >= MAX_FIT {
                continue;
            }
            let mut k2 = k;
            k2[d] = next_degree(k[d]);
            let f = run(k2, ledger)?;
            if cand.as_ref().is_none_or(|c| f.0.e < c.0.e) {
                cand = Some(f);
            }
        }
        let Some((c, t)) = cand else { break };
        let halved = c.e < best.e * 0.5;
        k = c.k;
        if c.e < best.e {
            best = c;
            target = t;
        }
        // Near its target a fit may stall at the rounding of its
        // residual; far from it a few rounds may pass before it halves.
        stall = if halved { 0 } else { stall + 1 };
        if stall >= 3 && best.e <= 1024.0 * target {
            break;
        }
    }
    Ok(best)
}

/// `num` at its least degrees whose elevation is within rounding of it
/// ([`Tensor::reduced`], each kept while the difference stays below
/// `2⁻⁴⁴` of `num`), its coefficients as exact `f64`s, and the certified
/// bound of the difference.
fn reduced(num: &Tensor<B>) -> (Tensor<B>, f64) {
    let exact = |t: &Tensor<B>| {
        let (m, n) = t.degrees();
        let c: Vec<f64> = t.coefs().iter().map(|c| c.v).collect();
        Tensor::from_f64(m, n, &c).expect("the same degrees")
    };
    let limit = num.hull().abs_max() / 17_592_186_044_416.0;
    let mut cur = exact(num);
    let mut delta = num.sub(&cur).hull().abs_max();
    let mut form = num.clone();
    for dir in [Dir::U, Dir::V] {
        while let Some(r) = form.reduced(dir) {
            let e = exact(&r);
            let d = num.sub(&e).hull().abs_max();
            if d.is_nan() || d > limit {
                break;
            }
            form = r;
            cur = e;
            delta = d;
        }
    }
    (cur, delta)
}

/// A bound of `|t|` on `[0, 1]²` towards `target`: its coefficient hull,
/// or where that is above the target the greater of its halves' bounds,
/// down to `depth` halvings.
fn sup_abs(t: &Tensor<B>, target: f64, depth: u32) -> f64 {
    let h = t.hull().abs_max();
    if h <= target || depth == 0 {
        return h;
    }
    let [a, b] = t.split_half(wider(t));
    let (x, y) = (
        sup_abs(&a, target, depth - 1),
        sup_abs(&b, target, depth - 1),
    );
    pick(x > y, x, y)
}

/// A bound of `|√a − S| = |r|/(√a + S)` (`r = a − S²`) on `[0, 1]²`
/// towards `target`, halving as [`sup_abs`] does.
fn sup_root(r: &Tensor<B>, a: &Tensor<B>, s: &Tensor<B>, target: f64, depth: u32) -> f64 {
    let (a_lo, s_lo) = (a.hull().lo, s.hull().lo);
    let root = if a_lo > 0.0 { down(a_lo.sqrt()) } else { 0.0 };
    let den = down(root + pick(s_lo > 0.0, s_lo, 0.0));
    let h = div_up(r.hull().abs_max(), den);
    if h <= target || depth == 0 {
        return h;
    }
    let dir = wider(r);
    let ([r0, r1], [a0, a1], [s0, s1]) = (r.split_half(dir), a.split_half(dir), s.split_half(dir));
    let x = sup_root(&r0, &a0, &s0, target, depth - 1);
    let y = sup_root(&r1, &a1, &s1, target, depth - 1);
    pick(x > y, x, y)
}

/// The direction of a form's higher degree, `u` on a tie.
fn wider(t: &Tensor<B>) -> Dir {
    let (m, n) = t.degrees();
    if n > m { Dir::V } else { Dir::U }
}

/// The units of a fit of degrees `k`: the extremum query's tile cell per
/// coefficient (borrowed until the measures have their own row).
fn fit_units(k: [usize; 2]) -> u64 {
    let n = ((k[0] + 1) * (k[1] + 1)) as u64;
    extremum_cell_units(true, 0).saturating_mul(1 + n / 32)
}

/// The polynomial of degrees `k` interpolating `f` at the
/// Chebyshev–Lobatto points, and the largest `|f|` there; `None` where a
/// value isn't finite or a solve fails.
fn interpolate(
    k: [usize; 2],
    lus: &mut Lus,
    f: impl Fn(f64, f64) -> f64,
) -> Option<(Tensor<B>, f64)> {
    let xs = lobatto(k[0]);
    let ys = lobatto(k[1]);
    let mut vals = vec![0.0; xs.len() * ys.len()];
    let mut scale: f64 = 0.0;
    for (i, x) in xs.iter().enumerate() {
        for (j, y) in ys.iter().enumerate() {
            let v = f(*x, *y);
            if !v.is_finite() {
                return None;
            }
            if v.abs() > scale {
                scale = v.abs();
            }
            vals[i * ys.len() + j] = v;
        }
    }
    // Solve along `u` for every `v` node, then along `v` for every row.
    let lu_u = lus.get(k[0])?;
    for j in 0..ys.len() {
        let mut col: Vec<f64> = (0..xs.len()).map(|i| vals[i * ys.len() + j]).collect();
        lu_u.solve(&mut col);
        for (i, c) in col.into_iter().enumerate() {
            vals[i * ys.len() + j] = c;
        }
    }
    let lu_v = lus.get(k[1])?;
    for row in vals.chunks_mut(ys.len()) {
        lu_v.solve(row);
    }
    if !vals.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some((Tensor::from_f64(k[0], k[1], &vals)?, scale))
}

/// `f` times the linear factors of `sides` (`s`, `1 − s`, `r`, `1 − r`),
/// each at most 1 on the cell.
fn with_sides(f: &Tensor<B>, sides: Sides) -> Option<Tensor<B>> {
    let mut f = f.clone();
    for side in sides.iter() {
        let lin = if side.at_one() {
            [1.0, 0.0]
        } else {
            [0.0, 1.0]
        };
        let factor = match side.dir() {
            Dir::U => Tensor::from_f64(1, 0, &lin)?,
            Dir::V => Tensor::from_f64(0, 1, &lin)?,
        };
        f = f.mul(&factor)?;
    }
    Some(f)
}

/// The Chebyshev–Lobatto points of degree `k` on `[0, 1]`,
/// `(1 − cos(πi/k))/2`; the middle for `k = 0`.
fn lobatto(k: usize) -> Vec<f64> {
    if k == 0 {
        return vec![0.5];
    }
    (0..=k)
        .map(|i| {
            if i == 0 {
                0.0
            } else if i == k {
                1.0
            } else {
                (1.0 - trig::cos(std::f64::consts::PI * i as f64 / k as f64)) * 0.5
            }
        })
        .collect()
}

/// The Bernstein–Vandermonde factorisations by degree, formed once.
#[derive(Default)]
struct Lus(Vec<Option<Option<Lu>>>);

impl Lus {
    fn get(&mut self, k: usize) -> Option<&Lu> {
        if self.0.len() <= k {
            self.0.resize_with(k + 1, || None);
        }
        self.0[k].get_or_insert_with(|| Lu::bernstein(k)).as_ref()
    }
}

/// An LU factorisation with partial pivoting of the Bernstein basis of
/// degree `k` at its Chebyshev–Lobatto points.
struct Lu {
    n: usize,
    a: Vec<f64>,
    perm: Vec<usize>,
}

impl Lu {
    fn bernstein(k: usize) -> Option<Lu> {
        let xs = lobatto(k);
        let n = k + 1;
        let mut binom = vec![1.0; n];
        for j in 1..n {
            binom[j] = binom[j - 1] * (n - j) as f64 / j as f64;
        }
        let mut a = vec![0.0; n * n];
        for (i, &x) in xs.iter().enumerate() {
            for j in 0..n {
                let mut v = binom[j];
                for _ in 0..j {
                    v *= x;
                }
                for _ in j..k {
                    v *= 1.0 - x;
                }
                a[i * n + j] = v;
            }
        }
        let mut perm: Vec<usize> = (0..n).collect();
        for c in 0..n {
            let mut p = c;
            for r in c + 1..n {
                if a[r * n + c].abs() > a[p * n + c].abs() {
                    p = r;
                }
            }
            if !(a[p * n + c] != 0.0) {
                return None;
            }
            if p != c {
                for j in 0..n {
                    a.swap(p * n + j, c * n + j);
                }
                perm.swap(p, c);
            }
            for r in c + 1..n {
                let f = a[r * n + c] / a[c * n + c];
                a[r * n + c] = f;
                for j in c + 1..n {
                    a[r * n + j] -= f * a[c * n + j];
                }
            }
        }
        Some(Lu { n, a, perm })
    }

    /// Solves in place for the coefficients whose basis values at the
    /// points are `b`.
    fn solve(&self, b: &mut [f64]) {
        let n = self.n;
        let mut x: Vec<f64> = self.perm.iter().map(|&p| b[p]).collect();
        for r in 0..n {
            for c in 0..r {
                x[r] -= self.a[r * n + c] * x[c];
            }
        }
        for r in (0..n).rev() {
            for c in r + 1..n {
                x[r] -= self.a[r * n + c] * x[c];
            }
            x[r] /= self.a[r * n + r];
        }
        b.copy_from_slice(&x);
    }
}

/// The antiderivative in `u` from `u = 0`, of one degree more: Bernstein
/// coefficients `qₖ = (p₀ + … + pₖ₋₁)/(m + 1)` on every line.
fn antiderivative_u(p: &Tensor<B>) -> Tensor<B> {
    let (m, n) = p.degrees();
    let d = B::exact((m + 1) as f64);
    let mut c = vec![B::exact(0.0); (m + 2) * (n + 1)];
    for j in 0..=n {
        let mut acc = B::exact(0.0);
        for k in 1..=m + 1 {
            acc = acc + *p.get(k - 1, j);
            c[k * (n + 1) + j] = acc / d;
        }
    }
    Tensor::new(m + 1, n, c).expect("one degree more")
}

/// The sorted distinct lines of the cells' sides, in `u` and in `v`.
fn grid_lines(cells: &[Cell]) -> [Vec<Rational>; 2] {
    std::array::from_fn(|k| {
        let mut l: Vec<Rational> = cells
            .iter()
            .flat_map(|c| [c.lo[k].clone(), c.hi[k].clone()])
            .collect();
        l.sort();
        l.dedup();
        l
    })
}

/// A piece of a p-curve span in one cell, in its coordinates.
enum CellSpan {
    /// A straight piece from `ends[0]` to `ends[1]`, `[s, r]` each.
    /// `flat` where `r` is constant along it (no contribution), `iso`
    /// where `s` is.
    Line {
        ends: [[B; 2]; 2],
        flat: bool,
        iso: bool,
        variation: [f64; 2],
    },
    /// A rational span: each coordinate's numerator and weights.
    Curve {
        coords: Box<[(Poly<B>, Poly<B>); 2]>,
        variation: [f64; 2],
    },
}

impl CellSpan {
    fn variation(&self) -> [f64; 2] {
        match self {
            CellSpan::Line { variation, .. } | CellSpan::Curve { variation, .. } => *variation,
        }
    }
}

/// The span cut at the cells' lines into pieces, each with its cell's
/// index: a straight or iso span exactly (its image the segment between
/// its ends), a Q or G span whole in the cell holding its control values
/// (the cells are then the tiles' shares, uncut); `None` where no cell
/// holds a piece.
fn pieces(
    span: &PSpan,
    cells: &[Cell],
    lines: &[Vec<Rational>; 2],
) -> Option<Vec<(usize, CellSpan)>> {
    let find = |p: &[Rational; 2]| cells.iter().position(|c| c.holds(p));
    match span.kind() {
        Kind::Iso | Kind::Linear => {
            let (a, b) = (span.start().map(q), span.end().map(q));
            let mut ts = vec![Rational::from(0u32), Rational::from(1u32)];
            for k in 0..2 {
                let d = &b[k] - &a[k];
                if d == 0u32 {
                    continue;
                }
                for x in &lines[k] {
                    let t = (x - &a[k]) / &d;
                    if t > 0u32 && t < 1u32 {
                        ts.push(t);
                    }
                }
            }
            ts.sort();
            ts.dedup();
            let at = |t: &Rational| -> [Rational; 2] {
                std::array::from_fn(|k| &a[k] + (&b[k] - &a[k]) * t)
            };
            let mut out = Vec::with_capacity(ts.len() - 1);
            for w in ts.windows(2) {
                let mid = at(&((&w[0] + &w[1]) / Rational::from(2u32)));
                let ci = find(&mid)?;
                let cell = &cells[ci];
                let ends = [at(&w[0]), at(&w[1])].map(|p| [0, 1].map(|k| cell.local(k, &p[k])));
                let variation = [0, 1].map(|k| {
                    let d = &ends[1][k] - &ends[0][k];
                    stored(&(if d < 0u32 { -d } else { d })).enclose()[1]
                });
                let flat = ends[0][1] == ends[1][1];
                let iso = ends[0][0] == ends[1][0];
                out.push((
                    ci,
                    CellSpan::Line {
                        ends: ends.map(|e| e.map(|x| stored(&x))),
                        flat,
                        iso,
                        variation,
                    },
                ));
            }
            Some(out)
        }
        Kind::Q | Kind::G => {
            let [lo, hi] = span.hull().map(|p| p.map(q));
            let ci = cells.iter().position(|c| c.holds(&lo) && c.holds(&hi))?;
            let cell = &cells[ci];
            let all = span.coords();
            let coords = [0, 1].map(|k| {
                let values = all[k].values();
                let num = Poly::new(
                    values
                        .iter()
                        .map(|&[x, w]| stored(&(cell.local(k, &q(x)) * q(w)))),
                )
                .expect("one to three values");
                let den =
                    Poly::new(values.iter().map(|&[_, w]| B::exact(w))).expect("as many weights");
                (num, den)
            });
            let variation = [0, 1].map(|k| {
                let values = all[k].values();
                let mut total = Rational::from(0u32);
                for w in values.windows(2) {
                    let d = cell.local(k, &q(w[1][0])) - cell.local(k, &q(w[0][0]));
                    total += if d < 0u32 { -d } else { d };
                }
                stored(&total).enclose()[1]
            });
            Some(vec![(
                ci,
                CellSpan::Curve {
                    coords: Box::new(coords),
                    variation,
                },
            )])
        }
    }
}

/// `∫ G dr` along the piece (module docs).
fn along(g: &Tensor<B>, sp: &CellSpan, ledger: &mut Ledger<'_>) -> Result<B, Stop<MassError>> {
    let (a, b) = g.degrees();
    match sp {
        CellSpan::Line {
            ends, flat, iso, ..
        } => {
            if *flat {
                return Ok(B::exact(0.0));
            }
            let [[s0, r0], [s1, r1]] = *ends;
            ledger.charge(Step::Work, span_units(a + b))?;
            if *iso {
                // `G(s₀, r)` as a polynomial in `r`, integrated from `r₀`
                // to `r₁` by its antiderivative.
                let line: Vec<B> = (0..=b)
                    .map(|j| {
                        let col = Poly::new((0..=a).map(|i| *g.get(i, j))).expect("a line");
                        col.eval(&s0)
                    })
                    .collect();
                let d = B::exact((b + 1) as f64);
                let mut acc = B::exact(0.0);
                let mut anti = vec![B::exact(0.0)];
                for c in &line {
                    acc = acc + *c;
                    anti.push(acc / d);
                }
                let anti = Poly::new(anti).expect("one degree more");
                return Ok(anti.eval(&r1) - anti.eval(&r0));
            }
            if a + b <= 63 {
                // A polynomial of degree at most 63 along the segment: the
                // 32-point rule is exact.
                let mut sum = B::exact(0.0);
                for (t, w) in gauss32_bounded() {
                    let s = s0 + (s1 - s0) * t;
                    let r = r0 + (r1 - r0) * t;
                    sum = sum + w * g.eval(&s, &r);
                }
                return Ok((r1 - r0) * sum);
            }
            let line = |x0: B, x1: B| Poly::new([x0, x1]).expect("a line");
            let one = line(B::exact(1.0), B::exact(1.0));
            let (s, r) = (line(s0, s1), line(r0, r1));
            match g.compose_params([&s, &one], [&r, &one]) {
                Some(c) => Ok((r1 - r0) * mean(&c)),
                None => Ok(loose(g, sp)),
            }
        }
        CellSpan::Curve { coords, .. } => {
            let [(su, du), (sv, dv)] = &**coords;
            ledger.charge(Step::Work, span_units(a * du.degree() + b * dv.degree()))?;
            let Some(gc) = g.compose_params([su, du], [sv, dv]) else {
                return Ok(loose(g, sp));
            };
            let dr = sv.derivative().mul(dv).zip(sv.mul(&dv.derivative()));
            let Some(n) = dr.and_then(|(x, y)| gc.mul(&x.sub(&y))) else {
                return Ok(loose(g, sp));
            };
            let pows = if du == dv {
                vec![(du.clone(), (a + b + 2) as u32)]
            } else {
                vec![(du.clone(), a as u32), (dv.clone(), (b + 2) as u32)]
            };
            rational(&n, &pows, ledger)
        }
    }
}

/// The units of a composition of degree `d`: the extremum query's span
/// cell per coefficient (borrowed, as [`fit_units`]).
fn span_units(d: usize) -> u64 {
    extremum_cell_units(false, 0).saturating_mul(d as u64 + 1)
}

/// The mean of a polynomial's Bernstein coefficients: its integral over
/// `[0, 1]`.
fn mean(p: &Poly<B>) -> B {
    let mut s = B::exact(0.0);
    for c in p.coefs() {
        s = s + *c;
    }
    s / B::exact((p.degree() + 1) as f64)
}

/// `0` with the error `max|G|·var(r)`: a bound of `∫ G dr` along a piece
/// whose composition passes the degree limit.
fn loose(g: &Tensor<B>, sp: &CellSpan) -> B {
    B::new(0.0, mul_up(g.hull().abs_max(), sp.variation()[1]), 0.0)
}

/// The deepest halving of a rational span's parameter.
const MAX_PIECE_DEPTH: u32 = 16;

/// A Gauss piece's target error, relative to its length times the
/// span's bound of the integrand, `max|N| / ∏ min Dᵢ^pᵢ` on the whole
/// span (so a piece where the integrand is near zero isn't refined for
/// nothing).
const PIECE_TAU: f64 = 1.0 / 1_125_899_906_842_624.0; // 2⁻⁵⁰

/// `∫₀¹ N / ∏ Dᵢ^pᵢ dt` (module docs): the 32-point Gauss sum on
/// pieces of the parameter, each halved until its bound meets the
/// target or at [`MAX_PIECE_DEPTH`], the pieces in order.
fn rational(
    n: &Poly<B>,
    dens: &[(Poly<B>, u32)],
    ledger: &mut Ledger<'_>,
) -> Result<B, Stop<MassError>> {
    let rule = gauss32_bounded();
    let mut total = B::exact(0.0);
    let scale = magnitude(n, dens);
    // Depth first, the left half first: the pieces in order of `t`.
    let mut stack = vec![(n.clone(), dens.to_vec(), 1.0f64, 0u32)];
    while let Some((n, dens, len, depth)) = stack.pop() {
        ledger.charge(Step::Work, span_units(n.degree()))?;
        let mut sum = B::exact(0.0);
        for &(x, w) in &rule {
            let mut f = n.eval(&x);
            for (d, p) in &dens {
                let dx = d.eval(&x);
                for _ in 0..*p {
                    f = f / dx;
                }
            }
            sum = sum + w * f;
        }
        let len_b = B::exact(len);
        let value = len_b * sum;
        let e = mul_up(len, piece_bound(&n, &dens));
        if e <= PIECE_TAU * len * scale || depth >= MAX_PIECE_DEPTH {
            total = total + value.with_trunc(e);
            continue;
        }
        let [nl, nr] = n.split_half();
        let (mut dl, mut dr) = (
            Vec::with_capacity(dens.len()),
            Vec::with_capacity(dens.len()),
        );
        for (d, p) in &dens {
            let [l, r] = d.split_half();
            dl.push((l, *p));
            dr.push((r, *p));
        }
        stack.push((nr, dr, len * 0.5, depth + 1));
        stack.push((nl, dl, len * 0.5, depth + 1));
    }
    Ok(total)
}

/// `max|N| / ∏ min Dᵢ^pᵢ` from the coefficients, in `f64` (a scale,
/// not a bound); 0 where a denominator's least coefficient isn't
/// positive.
fn magnitude(n: &Poly<B>, dens: &[(Poly<B>, u32)]) -> f64 {
    let mut d = 1.0;
    for (den, p) in dens {
        let lo = den.hull().lo;
        if lo.is_nan() || lo <= 0.0 {
            return 0.0;
        }
        d *= pow_down(lo, *p);
    }
    n.hull().abs_max() / d
}

/// Twice the distance from `N/∏Dᵢ^pᵢ` to a polynomial of degree at most
/// 63 on `[0, 1]` (module docs): a bound of the 32-point rule's error;
/// `+∞` where a denominator isn't certainly positive or `N` is of degree
/// above 63.
fn piece_bound(n: &Poly<B>, dens: &[(Poly<B>, u32)]) -> f64 {
    if n.degree() > 63 {
        return f64::INFINITY;
    }
    let mut total_deg = 0usize;
    let mut parts = Vec::with_capacity(dens.len());
    let mut d_min = 1.0;
    for (d, p) in dens {
        let h = d.hull();
        if h.lo.is_nan() || h.lo <= 0.0 {
            return f64::INFINITY;
        }
        d_min = down(d_min * pow_down(h.lo, *p));
        // The midpoint of the coefficients' values.
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in d.coefs() {
            if c.v < lo {
                lo = c.v;
            }
            if c.v > hi {
                hi = c.v;
            }
        }
        let c = B::exact(lo * 0.5 + hi * 0.5);
        let one = B::exact(1.0);
        let mut beta: f64 = 0.0;
        for x in d.coefs() {
            let e = (one - *x / c).enclose();
            let m = pick(-e[0] > e[1], -e[0], e[1]);
            if m.is_nan() || m > beta {
                beta = m;
            }
        }
        if beta > 0.0 {
            total_deg += (*p as usize) * d.degree();
        }
        parts.push((beta, *p));
    }
    // The series' length: `deg N + Σ pᵢ·deg Dᵢ·(L − 1) ≤ 63`.
    let l = match (63 - n.degree()).checked_div(total_deg) {
        None => 64,
        Some(q) => 1 + q as u32,
    };
    // `∏(1 + βᵢᴸ)^pᵢ − 1`, through `(1 + x)^p − 1 ≤ p·x·e^{1/2}` while
    // `p·x ≤ ½`.
    let mut rel = 0.0;
    for (beta, p) in parts {
        let x = pow_up(beta, l);
        let px = mul_up(f64::from(p), x);
        let term = if px <= 0.5 {
            mul_up(px, 1.6488)
        } else {
            up(pow_up(add_up(1.0, x), p) - 1.0)
        };
        // (1 + rel)(1 + term) − 1.
        rel = add_up(add_up(rel, term), mul_up(rel, term));
    }
    let n_max = n.hull().abs_max();
    div_up(mul_up(2.0, mul_up(n_max, rel)), d_min)
}

/// The coedge's ribbon (module docs): a bound of its area, from its
/// width (the seam bound), `variation` (a bound of the length of its
/// side on the surface) and its edge curve's control polygon, and its
/// reach `R`, the farthest it gets from `o`. Its flux is at most its
/// area times `R` for the volume's integrand, `R²` for a moment's.
fn ribbon(brep: &Brep, c: CoedgeId, variation: f64, o: [f64; 3]) -> (f64, f64) {
    let w = brep.seam_bound(c);
    if w == 0.0 {
        return (0.0, 0.0);
    }
    let e = brep.edge(c);
    let mut len = 0.0;
    let mut reach: f64 = 0.0;
    for span in brep.edge_spans(e) {
        for k in 0..3 {
            let d2 = |a: [f64; 4], b: [f64; 3]| {
                let mut s = 0.0;
                for i in 0..3 {
                    let d = up((a[i] - b[i]).abs());
                    s = add_up(s, mul_up(d, d));
                }
                up(s.sqrt())
            };
            reach = {
                let r = d2(span[k], o);
                pick(r > reach, r, reach)
            };
            if k > 0 {
                let p = span[k - 1];
                len = add_up(len, d2(span[k], [p[0], p[1], p[2]]));
            }
        }
    }
    let area = mul_up(w, mul_up(add_up(variation, len), 0.5));
    let reach = add_up(reach, w);
    (area, reach)
}
