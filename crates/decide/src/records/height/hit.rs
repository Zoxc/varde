//! Column hits on stored data, exactly: where the column of a point `q`
//! (the line through `q` along [`UP`]) meets the counting surface of a
//! face, plane or curved, its tiles inside the trim and the ribbons of
//! its coedges, each
//! hit decided on the stored `f64`s as rationals, by sign conditions at
//! the roots of univariate polynomials ([`crate::num::poly`]).
//!
//! **The face** ([`HitFace`]). Its tiles' stored nets (rational
//! biquadratic, affine control points with weights, [`TileNet`]) and its
//! trims as the counting reads them: per coedge the p-curve's rational
//! quadratic spans in global UV, the edge's spans and where each p-span
//! lies on them ([`HitTrim`]). These are the stored surface itself: no
//! frame or key plane stands in for a tile.
//!
//! **Tile hits** ([`tile_hits`]). On tile `[i, j]`, with local `(u, v)`
//! and the net's homogeneous point `X(u, v)`, weight `W(u, v)`, the
//! column meets the tile where `F₁ = r̂·(X − q·W)` and `F₂ = ν·(X − q·W)`
//! both vanish: bivariate of degree two in each. Read as polynomials in
//! `v` over `ℚ[u]`, their resultant `R(u)` (degree eight at most) holds
//! every hit's `u` among its roots, and the first subresultant gives its
//! `v = −s₀(u)/s₁(u)`, a rational function; any sign at the hit (its
//! height `UP·(X − q·W)`, the side of a box line, the facing `det
//! ∂(F₁, F₂)/∂(u, v)`) is a sign of a univariate polynomial at the root,
//! once `v` is substituted and the denominator's power cleared. A hit is
//! the tile's where its local coordinates lie in `(0, 1]` (`[0, 1]` at the
//! face's least tile lines), each tile line's hits found once. Inside the
//! trim is the trims' winding about the hit's UV ([`winding`]): the
//! signed crossings of the half-open ray from a point along `+u`, a point
//! of a trim with `v` at the ray's `v` counting as above it (so a vertex
//! on the ray counts once, a span along the ray's line not at all, a
//! touch not at all), each crossing a root of a quadratic, its side `u >
//! u_p` a sign at the root, as `LoopInRec` counts; read about a rational
//! point of a box round the hit that no trim meets ([`span_meets`]). A
//! UV point on a trim is no tile hit where the trim's ribbon has width:
//! the ribbon's hit at `λ = 0` is that point. The facing is `sign(x_u ×
//! x_v·UP)` times the face's inside winding.
//!
//! **Ribbon hits** ([`ribbon_hits`]). The ribbon of a p-span is
//! `R(τ, λ) = (1 − λ)·S(p(τ)) + λ·C(t(τ))`, `S` the tile's patch, `t` the
//! edge's parameter over the span's part of it (affine in `τ`). With `G =
//! S∘p − q`, `H = C − q` (homogeneous: times the weights; `S∘p` of degree
//! eight), the column meets the radial at `τ` exactly where `G`, `H` and
//! `UP` are coplanar: `D(τ) = G_r·H_ν − G_ν·H_r = 0`, components along `r̂`
//! and `ν`, of degree ten at most (two on an affine tile with a straight
//! p-curve and edge). At a root the hit lies on the radial (`λ ∈ [0, 1]`)
//! where `G` and `H` lie on opposite sides of the column (`G_r·H_r ≤ 0`,
//! or along `ν` where both `r̂` components vanish); its height is the sign
//! of `(G_r·H_u − H_r·G_u)·(G_r·w_c − H_r·w_p)` and its facing
//! `−sign(D′)` along the edge (the ribbon oriented by `R_λ × R_t`, `R_t ×
//! R_λ` against it): signs of polynomials at the root. A double root (the
//! column touching the ribbon, or the tile) is two hits of opposite
//! facing at one height, which cancel. Each knot and vertex belongs to one
//! span, the one ending there in the loop's order, so a hit there is
//! found once. A ribbon of no width (`S∘p` the edge's curve exactly) has
//! no hits.
//!
//! **Folds.** Where the column touches a tile (`(x_u × x_v)·UP = 0` at a
//! double root) away from `q`'s height its two hits, of opposite facing
//! at one height, cancel and none is listed. At `q`'s height (`q` on the
//! tile's silhouette inside the face) the moved column decides: the tile
//! there folds over in the shadow, covering the side of the silhouette
//! its normal curvature along `ê` points to (`sign(n·S_êê)·n`), and the
//! moved column enters the fold exactly where the first motion's `n·(Δq
//! − Δx)` has that sign ([`Motion`]: the face's rates less the point's).
//! Entering, the column meets the fold's two sheets, which part as the
//! square root of the motion while `q` moves along its column linearly:
//! `q` lies between them. So the fold is two hits at the root, the lower
//! sheet's below `q` and the upper's above it, the upper facing
//! `−sign(n_f·S_êê)` (`n_f` the face's normal: the sheet bending away
//! from its normal going up faces up); missing it, none. A fold whose
//! bend or first-order side is zero, or on a trim at `q`, isn't formed
//! ([`HitError::Tile`]).
//!
//! **Curved faces.** Nothing above asks for a plane: a curved face's
//! tiles, p-curves and edges are rational quadratic data stored as `f64`s
//! too, so its hits are the same exact algebraic points, isolated by the
//! same root isolation ([`crate::num::poly`]), and a height or facing at
//! one is exactly zero or certified, never a tie of the precision.
//!
//! **Incidence** (the germ rule). A hit at `q`'s own height on the
//! edge's curve (`λ = 1`, `q` exactly on `C`) records where it lies on
//! the edge ([`HitInc`]); so does a tile hit at `q` on a trim of no
//! width. The counting then reads the edge's or vertex's key there.
//!
//! **Order.** A sheet's hits are numbered ([`HitIx`]) along the edge
//! from its geometrically lesser end ([`Along`]), a tile's along `u`.

use std::cell::OnceCell;
use std::cmp::Ordering;
use std::ops::Range;

use malachite::Rational;
use malachite::base::num::arithmetic::traits::Sign as _;
use malachite::base::num::basic::traits::{One as _, Zero as _};

use crate::count::boxes::UvSpan;
use crate::count::pieces::{ExactRect, SpanMap};
use crate::ids::{CoedgeId, EdgeId, FaceId, VertId};
use crate::num::bounded::mul_up;
use crate::num::decide::Sign;
use crate::num::exact::{ExactSign, ORDERS, rat};
use crate::num::poly::{self, Poly, Root};
use crate::order::Along;
use crate::perturb::{NU, R_HAT, UP, ribbon_row_signs};
use crate::records::keys::{HitIx, Operand, Sheet};
use crate::records::ray::{Span, TileNet};

type Q3 = [Rational; 3];

/// The rates of a face's points less the asking point's, per motion, at
/// the point's column (`Δx − Δq`): what the fold rule's side reads
/// (module docs, "Folds").
pub type Motion = [Q3; ORDERS];

/// One trim of a face as its hits read it.
#[derive(Clone, Copy, Debug)]
pub struct HitTrim<'a> {
    /// The coedge.
    pub coedge: CoedgeId,
    /// Its edge.
    pub edge: EdgeId,
    /// Whether the coedge runs against its edge.
    pub reversed: bool,
    /// The edge's stored start and end vertices.
    pub ends: [VertId; 2],
    /// Which way the edge's stored parameter runs from its geometrically
    /// lesser end.
    pub along: Along,
    /// The edge's band `β_g`; its near radius is `3·β_g` rounded up.
    pub band: f64,
    /// The p-curve's spans in global UV, along the edge's stored
    /// direction.
    pub pspans: &'a [UvSpan],
    /// The edge's spans, along its stored direction.
    pub curve: &'a [Span],
    /// Per p-span, where it lies on `curve`.
    pub map: &'a [SpanMap],
}

impl HitTrim<'_> {
    /// The edge's near radius `r_g = 3·β_g`, rounded up.
    pub fn near_radius(&self) -> f64 {
        mul_up(3.0, self.band)
    }
}

/// A face as its hits read it (module docs).
#[derive(Clone, Debug)]
pub struct HitFace<'a> {
    /// The face's operand.
    pub operand: Operand,
    /// The face.
    pub face: FaceId,
    /// Its tiles' index ranges, `u` then `v`.
    pub tiles: [Range<i64>; 2],
    /// The tiles' stored nets, `u` index first (as
    /// [`FaceUv::tile_boxes`](crate::count::boxes::FaceUv::tile_boxes)
    /// orders the tiles).
    pub nets: &'a [TileNet],
    /// The trims' winding round a point inside: `−1` on a flipped face,
    /// else `+1`.
    pub inside: i64,
    /// Every trim of positive length.
    pub trims: Vec<HitTrim<'a>>,
}

impl HitFace<'_> {
    /// Tile `[i, j]`'s net; `None` for a tile not the face's.
    fn net(&self, [i, j]: [i64; 2]) -> Option<&TileNet> {
        let [us, vs] = &self.tiles;
        if !us.contains(&i) || !vs.contains(&j) {
            return None;
        }
        let nv = usize::try_from(vs.end - vs.start).ok()?;
        let k = usize::try_from(i - us.start).ok()? * nv + usize::try_from(j - vs.start).ok()?;
        self.nets.get(k)
    }

    /// The tile holding p-span `sp`: the one its control points' centroid
    /// lies in, within the face's tiles (a p-curve is broken at the tile
    /// lines, so each span lies in one tile).
    fn tile_of_span(&self, sp: &UvSpan) -> [i64; 2] {
        std::array::from_fn(|k| {
            let c = (sp[0][k] + sp[1][k] + sp[2][k]) / 3.0;
            let r = &self.tiles[k];
            let i = c.floor();
            if i < r.start as f64 {
                r.start
            } else if i >= (r.end - 1) as f64 {
                r.end - 1
            } else {
                i as i64
            }
        })
    }
}

/// Where a tile hit lies in its tile's local coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The free coordinate (`u`, or `v` where `swapped`) the root `u`,
    /// the other `−s₀(u)/s₁(u)`.
    Param {
        /// The free coordinate.
        u: Root,
        /// The other coordinate's numerator, negated, in the free one.
        s0: Poly,
        /// The other coordinate's denominator.
        s1: Poly,
        /// Whether the free coordinate is `v`.
        swapped: bool,
    },
    /// Each coordinate its own root (a tile whose `u` and `v` separate).
    Separate {
        /// The local `u`.
        u: Root,
        /// The local `v`.
        v: Root,
    },
}

/// A tile hit's place: its tile and local coordinates, and the tile's
/// plane where all its control points lie on one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileRoot {
    /// The tile.
    pub tile: [i64; 2],
    /// The local coordinates.
    pub place: Place,
    /// A planar tile's normal, exactly (either way round): its hit's
    /// height and tangent plane are the plane's.
    pub plane: Option<Q3>,
}

impl TileRoot {
    /// The sign of local coordinate `k` less `c` at the hit.
    fn coord_cmp(&self, k: usize, c: &Rational) -> Ordering {
        match &self.place {
            Place::Separate { u, v } => [u, v][k].cmp_rational(c),
            Place::Param { u, s0, s1, swapped } => {
                if k == usize::from(*swapped) {
                    return u.cmp_rational(c);
                }
                // `(−s₀ − c·s₁)/s₁`.
                let num = poly::neg(&poly::add(s0, &poly::scale(s1, c)));
                mul_ord(u.sign_of(&num), u.sign_of(s1))
            }
        }
    }
}

/// Where a hit lies on its sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HitAt {
    /// On a tile.
    Tile(Box<TileRoot>),
    /// On the ribbon of span `span` of trim `trim` (an index into
    /// [`HitFace::trims`]), at this root of the hit polynomial in the
    /// span's parameter.
    Ribbon {
        /// The trim.
        trim: usize,
        /// The span.
        span: usize,
        /// The span's parameter.
        tau: Root,
    },
}

/// Where a hit at `q`'s own height lies exactly on an edge of the face
/// (module docs, "Incidence"): its edge's span and the parameter there,
/// enclosed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitInc {
    /// The edge.
    pub edge: EdgeId,
    /// The edge's span, along its stored direction.
    pub span: u32,
    /// The parameter on it, enclosed, within `[0, 1]`.
    pub t: [f64; 2],
}

/// A column hit (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The sheet.
    pub sheet: Sheet,
    /// Its ordinal there.
    pub i: HitIx,
    /// Where it lies.
    pub at: HitAt,
    /// The sheet's facing there as an upward ray counts it, `Pos` facing
    /// up; `None` at a radial of no length at `q` (incidence only).
    pub facing: Option<Sign>,
    /// The hit's height above `q`, exactly: `Equal` where `q` lies on the
    /// sheet (a tie the motion decides).
    pub height: Ordering,
    /// Where it lies exactly on an edge of the face at `q`'s height.
    pub inc: Option<HitInc>,
    /// A closed box of space holding the hit, exactly.
    pub near: [[Rational; 2]; 3],
    /// A ribbon hit at `λ = 0`: the tile's point on the trim.
    pub on_surface: bool,
}

/// Why a face's hits aren't formed: a configuration no exact sign
/// condition settles here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitError {
    /// The column meets tile `[i, j]` in a curve, or the tile's equations
    /// degenerate where it meets it (no subresultant gives the hit).
    Tile([i64; 2]),
    /// The column lies along a radial of this ribbon span, or the span's
    /// radials all meet the column's line (a ribbon in a plane through
    /// it), or the span lies in no tile of the face.
    Degenerate {
        /// The coedge.
        coedge: CoedgeId,
        /// The span.
        span: u32,
    },
    /// The column touches this ribbon span at `q`'s height, or meets it
    /// at a root of odd multiplicity above one.
    Tangent {
        /// The coedge.
        coedge: CoedgeId,
        /// The span.
        span: u32,
    },
    /// A hit on tile `[i, j]` lies on a trim no box round it separates it
    /// from, the trim's ribbon holding no hit there.
    OnTrim([i64; 2]),
    /// The trims wind round a point other than 0 or the face's inside
    /// winding (not a valid face).
    Winding(i64),
}

/// A UV point lies on a trim: this span of this trim (an index into the
/// trims), at this root of its parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnTrim {
    /// The trim.
    pub trim: usize,
    /// The span.
    pub span: usize,
    /// The span's parameter there.
    pub tau: Box<Root>,
}

fn frame_dirs() -> [Q3; 3] {
    [R_HAT, NU, UP].map(|v| [v.x, v.y, v.z].map(rat))
}

fn sign_of(o: Ordering) -> Option<Sign> {
    Sign::of(o)
}

fn neg(s: Sign) -> Sign {
    match s {
        Sign::Pos => Sign::Neg,
        Sign::Neg => Sign::Pos,
    }
}

fn mul_ord(a: Ordering, b: Ordering) -> Ordering {
    match (a, b) {
        (Ordering::Equal, _) | (_, Ordering::Equal) => Ordering::Equal,
        (x, y) if x == y => Ordering::Greater,
        _ => Ordering::Less,
    }
}

/// `2^−bits`, exactly.
fn half_pow(bits: u32) -> Rational {
    let two = Rational::from(2u32);
    (0..bits).fold(Rational::ONE, |r, _| r / &two)
}

fn min_max<'r>(xs: impl IntoIterator<Item = &'r Rational>) -> [Rational; 2] {
    let mut it = xs.into_iter();
    let first = it.next().expect("values").clone();
    it.fold([first.clone(), first], |[lo, hi], x| {
        [
            if *x < lo { x.clone() } else { lo },
            if *x > hi { x.clone() } else { hi },
        ]
    })
}

// ---------------------------------------------------------------------
// Bivariate polynomials: `Σⱼ Fⱼ(u)·vʲ`.

/// A bivariate polynomial, by its coefficients in `v` (each a polynomial
/// in `u`), lowest first.
type Poly2 = Vec<Poly>;

fn trim2(mut f: Poly2) -> Poly2 {
    while f.last().is_some_and(|c| c.is_empty()) {
        f.pop();
    }
    f
}

fn add2(a: &Poly2, b: &Poly2) -> Poly2 {
    let n = a.len().max(b.len());
    trim2(
        (0..n)
            .map(|j| match (a.get(j), b.get(j)) {
                (Some(x), Some(y)) => poly::add(x, y),
                (Some(x), None) | (None, Some(x)) => x.clone(),
                (None, None) => Vec::new(),
            })
            .collect(),
    )
}

fn sub2(a: &Poly2, b: &Poly2) -> Poly2 {
    add2(a, &b.iter().map(|c| poly::neg(c)).collect())
}

fn mul2(a: &Poly2, b: &Poly2) -> Poly2 {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out: Poly2 = vec![Vec::new(); a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] = poly::add(&out[i + j], &poly::mul(x, y));
        }
    }
    trim2(out)
}

fn scale2(a: &Poly2, k: &Rational) -> Poly2 {
    trim2(a.iter().map(|c| poly::scale(c, k)).collect())
}

fn du2(a: &Poly2) -> Poly2 {
    trim2(a.iter().map(|c| poly::deriv(c)).collect())
}

fn dv2(a: &Poly2) -> Poly2 {
    trim2(
        a.iter()
            .enumerate()
            .skip(1)
            .map(|(j, c)| poly::scale(c, &Rational::from(j as u64)))
            .collect(),
    )
}

/// `f(u, n/m)·m^d`, `d` `f`'s degree in `v`, and `d`.
fn subst(f: &Poly2, n: &Poly, m: &Poly) -> (Poly, usize) {
    let d = f.len().saturating_sub(1);
    let mut out: Poly = Vec::new();
    for (j, c) in f.iter().enumerate() {
        let mut t = c.clone();
        for _ in 0..j {
            t = poly::mul(&t, n);
        }
        for _ in j..d {
            t = poly::mul(&t, m);
        }
        out = poly::add(&out, &t);
    }
    (out, d)
}

/// The tensor Bernstein form `c[i][j]` (`i` along `u`) over `[0, 1]²` in
/// the power basis.
fn bern2(c: &[[Rational; 3]; 3]) -> Poly2 {
    let pu: [Poly; 3] =
        std::array::from_fn(|j| poly::from_bernstein(&[0, 1, 2].map(|i| c[i][j].clone())));
    let mut out: Poly2 = Vec::new();
    for (j, p) in pu.iter().enumerate() {
        let mut b = [0, 0, 0].map(Rational::from);
        b[j] = Rational::ONE;
        let basis = poly::from_bernstein(&b);
        let term: Poly2 = basis.iter().map(|bk| poly::scale(p, bk)).collect();
        out = add2(&out, &trim2(term));
    }
    out
}

/// A net's homogeneous coordinates and weight, exactly: `w·x`, `w`.
type Hom3 = ([[[Rational; 3]; 3]; 3], [[Rational; 3]; 3]);

fn net_hom(net: &TileNet) -> Hom3 {
    let w: [[Rational; 3]; 3] =
        std::array::from_fn(|i| std::array::from_fn(|j| rat(net[3 * i + j][3])));
    let x = std::array::from_fn(|k| {
        std::array::from_fn(|i| std::array::from_fn(|j| &w[i][j] * rat(net[3 * i + j][k])))
    });
    (x, w)
}

/// `d·(X − q·W)`'s Bernstein coefficients over the tile.
fn net_coefs(net: &TileNet, d: &Q3, q: &Q3) -> [[Rational; 3]; 3] {
    let (x, w) = net_hom(net);
    let dq = &d[0] * &q[0] + &d[1] * &q[1] + &d[2] * &q[2];
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            &d[0] * &x[0][i][j] + &d[1] * &x[1][i][j] + &d[2] * &x[2][i][j] - &dq * &w[i][j]
        })
    })
}

/// A net's `X` coordinates and `W` in the power basis.
fn net_polys(net: &TileNet) -> ([Poly2; 3], Poly2) {
    let (x, w) = net_hom(net);
    (std::array::from_fn(|k| bern2(&x[k])), bern2(&w))
}

// ---------------------------------------------------------------------
// Tile hits.

/// `f(u, v)` as a polynomial with its variables' roles swapped: the
/// coefficients in `u` of polynomials in `v`.
fn transpose2(f: &Poly2) -> Poly2 {
    let n = f.iter().map(|c| c.len()).max().unwrap_or(0);
    trim2(
        (0..n)
            .map(|i| {
                poly::trim(
                    f.iter()
                        .map(|c| c.get(i).cloned().unwrap_or(Rational::ZERO))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The sign of `f(u, v)` at the tile root `r`; `None` at a root whose
/// coordinates separate (each its own algebraic number), where a
/// bivariate sign isn't formed.
fn sign_at_tile(f: &Poly2, r: &TileRoot) -> Option<Ordering> {
    let Place::Param { u, s0, s1, swapped } = &r.place else {
        return None;
    };
    let t;
    let f = if *swapped {
        t = transpose2(f);
        &t
    } else {
        f
    };
    let n = poly::neg(s0);
    let (g, d) = subst(f, &n, s1);
    let s = u.sign_of(&g);
    Some(if d % 2 == 1 {
        mul_ord(s, u.sign_of(s1))
    } else {
        s
    })
}

/// `f(u, v)` at the rational point `(u, v)`.
fn eval2(f: &Poly2, u: &Rational, v: &Rational) -> Rational {
    f.iter()
        .rev()
        .fold(Rational::ZERO, |acc, c| acc * v + poly::eval(c, u))
}

/// The resultant of `a` and `b` in `v` and the first subresultant's
/// coefficients `(s₀, s₁)` (the common root `v = −s₀/s₁`), for degrees
/// one or two in `v`; `None` for a degree outside them.
fn eliminate(a: &Poly2, b: &Poly2) -> Option<(Poly, Poly, Poly)> {
    let g = |f: &Poly2, j: usize| f.get(j).cloned().unwrap_or_default();
    let m = poly::mul;
    let s = poly::sub;
    match (a.len(), b.len()) {
        (2, 2) => {
            // a₁v + a₀, b₁v + b₀: R = a₁b₀ − a₀b₁, v = −a₀/a₁.
            let (a0, a1, b0, b1) = (g(a, 0), g(a, 1), g(b, 0), g(b, 1));
            Some((s(&m(&a1, &b0), &m(&a0, &b1)), a0, a1))
        }
        (3, 2) | (2, 3) => {
            let (q, l) = if a.len() == 3 { (a, b) } else { (b, a) };
            let (a0, a1, a2, b0, b1) = (g(q, 0), g(q, 1), g(q, 2), g(l, 0), g(l, 1));
            // R = a₂b₀² − a₁b₀b₁ + a₀b₁², v = −b₀/b₁.
            let r = poly::add(
                &s(&m(&a2, &m(&b0, &b0)), &m(&a1, &m(&b0, &b1))),
                &m(&a0, &m(&b1, &b1)),
            );
            Some((r, b0, b1))
        }
        (3, 3) => {
            let (a0, a1, a2, b0, b1, b2) = (g(a, 0), g(a, 1), g(a, 2), g(b, 0), g(b, 1), g(b, 2));
            // R = (a₂b₀ − a₀b₂)² − (a₂b₁ − a₁b₂)(a₁b₀ − a₀b₁); the
            // subresultant (a₂b₁ − a₁b₂)v + (a₂b₀ − a₀b₂).
            let p = s(&m(&a2, &b0), &m(&a0, &b2));
            let q = s(&m(&a2, &b1), &m(&a1, &b2));
            let r = s(&m(&p, &p), &m(&q, &s(&m(&a1, &b0), &m(&a0, &b1))));
            Some((r, p, q))
        }
        _ => None,
    }
}

/// Whether Bernstein coefficients all have one strict sign (no zero
/// over the tile).
fn one_sign(c: &[[Rational; 3]; 3]) -> bool {
    let mut s = Ordering::Equal;
    for row in c {
        for x in row {
            let o = x.sign();
            if o == Ordering::Equal || (s != Ordering::Equal && o != s) {
                return false;
            }
            s = o;
        }
    }
    true
}

/// The hits of the column of `q` on the tiles of the face, by tile (`u`
/// index first) and then along `u` (module docs, "Tile hits"); `ribbons`
/// the face's ribbon hits, whose hits at `λ = 0` lie on the trims.
pub fn tile_hits(
    face: &HitFace<'_>,
    q: [f64; 3],
    ribbons: &[Hit],
    motion: &Motion,
) -> Result<Vec<Hit>, HitError> {
    let mut out = Vec::new();
    let [us, vs] = face.tiles.clone();
    for i in us {
        for j in vs.clone() {
            out.extend(on_tile(face, [i, j], q, ribbons, motion)?);
        }
    }
    Ok(out)
}

/// The roots of the column's equations on `tile`, eliminating `v` (or
/// `u` where `swapped`), with the free coordinate's multiplicity in the
/// resultant; `None` where the elimination degenerates (a degree outside
/// one or two, or no subresultant giving the other coordinate at a
/// root in `[0, 1]`). Where the first subresultant vanishes at a root
/// (the equations' leading coefficients vanishing there), the other
/// coordinate is one equation's linear root there; every root is checked
/// to satisfy both equations.
fn tile_roots(
    f1: &Poly2,
    f2: &Poly2,
    tile: [i64; 2],
    swapped: bool,
) -> Option<Vec<(TileRoot, u32)>> {
    let (a, b) = if swapped {
        (transpose2(f1), transpose2(f2))
    } else {
        (f1.clone(), f2.clone())
    };
    let (r, s0, s1) = eliminate(&a, &b)?;
    if r.is_empty() {
        return None;
    }
    let g = |f: &Poly2, j: usize| f.get(j).cloned().unwrap_or_default();
    let (zero, one) = (Rational::ZERO, Rational::ONE);
    let mut out = Vec::new();
    for (u, mult) in poly::roots(&r, &zero, &one) {
        let (s0, s1) = if u.sign_of(&s1) != Ordering::Equal {
            (s0.clone(), s1.clone())
        } else if a.len() == 3
            && b.len() == 3
            && u.sign_of(&g(&a, 2)) == Ordering::Equal
            && u.sign_of(&g(&b, 2)) == Ordering::Equal
        {
            // Both linear in the other coordinate here.
            if u.sign_of(&g(&a, 1)) != Ordering::Equal {
                (g(&a, 0), g(&a, 1))
            } else if u.sign_of(&g(&b, 1)) != Ordering::Equal {
                (g(&b, 0), g(&b, 1))
            } else {
                return None;
            }
        } else {
            return None;
        };
        let root = TileRoot {
            tile,
            place: Place::Param { u, s0, s1, swapped },
            plane: None,
        };
        // A common root of both equations, not one at infinity.
        let zero = Some(Ordering::Equal);
        if sign_at_tile(f1, &root) != zero || sign_at_tile(f2, &root) != zero {
            continue;
        }
        out.push((root, mult));
    }
    Some(out)
}

/// The roots of equations whose coordinates separate: `f1` of `u` alone
/// and `f2` of `v` alone (or the other way round), each pair of roots in
/// `[0, 1]`; `None` where they don't separate.
fn separate_roots(f1: &Poly2, f2: &Poly2, tile: [i64; 2]) -> Option<Vec<(TileRoot, u32)>> {
    // Of `u` alone: one coefficient in `v`. Of `v` alone: every
    // coefficient constant in `u`.
    let of_u = |f: &Poly2| f.len() <= 1;
    let of_v = |f: &Poly2| f.iter().all(|c| c.len() <= 1);
    let (fu, fv) = if of_u(f1) && of_v(f2) {
        (f1, f2)
    } else if of_u(f2) && of_v(f1) {
        (f2, f1)
    } else {
        return None;
    };
    let pu: Poly = fu.first().cloned().unwrap_or_default();
    let pv: Poly = poly::trim(
        fv.iter()
            .map(|c| c.first().cloned().unwrap_or(Rational::ZERO))
            .collect(),
    );
    if pu.is_empty() || pv.is_empty() {
        return None;
    }
    let (zero, one) = (Rational::ZERO, Rational::ONE);
    let mut out = Vec::new();
    for (u, mu) in poly::roots(&pu, &zero, &one) {
        for (v, mv) in poly::roots(&pv, &zero, &one) {
            let place = Place::Separate { u: u.clone(), v };
            out.push((
                TileRoot {
                    tile,
                    place,
                    plane: None,
                },
                mu.max(mv),
            ));
        }
    }
    Some(out)
}

/// The exact normal of a net whose control points all lie on one plane
/// (any orientation); `None` where they don't, or lie on a line.
fn planar_normal(net: &TileNet) -> Option<Q3> {
    let p: Vec<Q3> = net.iter().map(|c| [c[0], c[1], c[2]].map(rat)).collect();
    let sub = |a: &Q3, b: &Q3| -> Q3 { std::array::from_fn(|k| &a[k] - &b[k]) };
    let cross = |a: &Q3, b: &Q3| -> Q3 {
        [
            &a[1] * &b[2] - &a[2] * &b[1],
            &a[2] * &b[0] - &a[0] * &b[2],
            &a[0] * &b[1] - &a[1] * &b[0],
        ]
    };
    let dot = |a: &Q3, b: &Q3| &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2];
    // The corners `(0, 0)`, `(2, 0)`, `(0, 2)`: row-major in `u`.
    let n = cross(&sub(&p[6], &p[0]), &sub(&p[2], &p[0]));
    if n.iter().all(|x| *x == 0u32) {
        return None;
    }
    p.iter()
        .all(|x| dot(&n, &sub(x, &p[0])) == 0u32)
        .then_some(n)
}

/// A tile's homogeneous partials `X_u·W − X·W_u`, `X_v·W − X·W_v`
/// (`W²` times `x_u`, `x_v`) and their cross product `n`, `W⁴` times the
/// normal `x_u × x_v`.
struct Tangents {
    tu: [Poly2; 3],
    tv: [Poly2; 3],
    n: [Poly2; 3],
}

impl Tangents {
    fn of(x: &[Poly2; 3], w: &Poly2) -> Tangents {
        let (wu, wv) = (du2(w), dv2(w));
        let tu: [Poly2; 3] =
            std::array::from_fn(|k| sub2(&mul2(&du2(&x[k]), w), &mul2(&x[k], &wu)));
        let tv: [Poly2; 3] =
            std::array::from_fn(|k| sub2(&mul2(&dv2(&x[k]), w), &mul2(&x[k], &wv)));
        let n = cross2(&tu, &tv);
        Tangents { tu, tv, n }
    }
}

fn cross2(a: &[Poly2; 3], b: &[Poly2; 3]) -> [Poly2; 3] {
    [
        sub2(&mul2(&a[1], &b[2]), &mul2(&a[2], &b[1])),
        sub2(&mul2(&a[2], &b[0]), &mul2(&a[0], &b[2])),
        sub2(&mul2(&a[0], &b[1]), &mul2(&a[1], &b[0])),
    ]
}

/// `a·d`, `d` a constant vector.
fn dot2(a: &[Poly2; 3], d: &Q3) -> Poly2 {
    (0..3).fold(Vec::new(), |acc, k| add2(&acc, &scale2(&a[k], &d[k])))
}

/// `a·b` of two vectors of polynomials.
fn dot2v(a: &[Poly2; 3], b: &[Poly2; 3]) -> Poly2 {
    (0..3).fold(Vec::new(), |acc, k| add2(&acc, &mul2(&a[k], &b[k])))
}

/// The fold rule at a tile root where the column is tangent to the tile
/// at `q`'s height (module docs, "Folds"): `Some(Some(upper))`, the upper
/// sheet's facing as an upward ray counts it, where the moved column
/// enters the fold, `Some(None)` where it misses it; `None` where the
/// rule doesn't decide (the tile not bending along `ê` there, or the
/// motion's first order along the tile's normal zero).
fn fold(
    x: &[Poly2; 3],
    w: &Poly2,
    t: &Tangents,
    root: &TileRoot,
    inside: i64,
    motion: &Motion,
) -> Option<Option<Sign>> {
    let up: Q3 = [UP.x, UP.y, UP.z].map(rat);
    // The parameter direction `(a, b)` along which the tile runs along
    // `ê`: `a·x_u + b·x_v ∥ ê` (`|n|²/W²` times `ê`).
    let upv: [Poly2; 3] = up.clone().map(|c| vec![poly::constant(c)]);
    let a = dot2v(&cross2(&upv, &t.tv), &t.n);
    let b = dot2v(&cross2(&t.tu, &upv), &t.n);
    // `n·x_ij ∝ n·(X_ij·W − X·W_ij)`, the tangential parts dropped.
    let second = |xij: &[Poly2; 3], wij: &Poly2| -> Poly2 {
        let v: [Poly2; 3] = std::array::from_fn(|k| sub2(&mul2(&xij[k], w), &mul2(&x[k], wij)));
        dot2v(&t.n, &v)
    };
    let xuu: [Poly2; 3] = std::array::from_fn(|k| du2(&du2(&x[k])));
    let xuv: [Poly2; 3] = std::array::from_fn(|k| du2(&dv2(&x[k])));
    let xvv: [Poly2; 3] = std::array::from_fn(|k| dv2(&dv2(&x[k])));
    let (huu, huv, hvv) = (
        second(&xuu, &du2(&du2(w))),
        second(&xuv, &du2(&dv2(w))),
        second(&xvv, &dv2(&dv2(w))),
    );
    let two = Rational::from(2u32);
    let bend = add2(
        &add2(
            &mul2(&mul2(&a, &a), &huu),
            &scale2(&mul2(&mul2(&a, &b), &huv), &two),
        ),
        &mul2(&mul2(&b, &b), &hvv),
    );
    let bend = sign_of(sign_at_tile(&bend, root)?)?;
    // The moved column's side of the silhouette along `n`: the first
    // motion's `n·(Δq − Δx)`; the fold covers the side `n·S_êê` points
    // to.
    let side = sign_of(sign_at_tile(&dot2(&t.n, &motion[0]), root)?)?;
    let enters = side != bend;
    // The upper sheet faces up where the tile bends away from its normal
    // going up: `−sign(n_f·S_êê)`, `n_f` the face's normal.
    let upper = if inside < 0 { bend } else { neg(bend) };
    Some(enters.then_some(upper))
}

fn on_tile(
    face: &HitFace<'_>,
    tile: [i64; 2],
    q: [f64; 3],
    ribbons: &[Hit],
    motion: &Motion,
) -> Result<Vec<Hit>, HitError> {
    let Some(net) = face.net(tile) else {
        return Ok(Vec::new());
    };
    let qv = q.map(rat);
    let [a, b, up] = frame_dirs();
    let (c1, c2) = (net_coefs(net, &a, &qv), net_coefs(net, &b, &qv));
    if one_sign(&c1) || one_sign(&c2) {
        return Ok(Vec::new());
    }
    let bad = HitError::Tile(tile);
    let (x, w) = net_polys(net);
    // A planar tile: the column's hit is its plane's, exactly; the tile's
    // local coordinates there solve two of its coordinates.
    let planar = planar_normal(net);
    let (roots, flat_height) = match &planar {
        Some(n) => {
            let dot = |a: &Q3, b: &Q3| &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2];
            let p0: Q3 = [net[0][0], net[0][1], net[0][2]].map(rat);
            let nu = dot(n, &up);
            if nu == 0u32 {
                // The column along the plane: in it, or missing it.
                let rel: Q3 = std::array::from_fn(|k| &p0[k] - &qv[k]);
                return if dot(n, &rel) == 0u32 {
                    Err(bad)
                } else {
                    Ok(Vec::new())
                };
            }
            // `q + s·UP` on the plane: `s = n·(p₀ − q)/(n·UP)`.
            let rel: Q3 = std::array::from_fn(|k| &p0[k] - &qv[k]);
            let sv = dot(n, &rel) / &nu;
            let hit: Q3 = std::array::from_fn(|k| &qv[k] + &sv * &up[k]);
            // The two coordinates across the normal's largest component.
            let big = (0..3)
                .max_by(|&i, &j| {
                    let (x, y) = (n[i].clone(), n[j].clone());
                    let (x, y) = (if x < 0u32 { -x } else { x }, if y < 0u32 { -y } else { y });
                    x.cmp(&y).then(j.cmp(&i))
                })
                .expect("three axes");
            let axes: Vec<usize> = (0..3).filter(|&k| k != big).collect();
            let g: Vec<Poly2> = axes
                .iter()
                .map(|&k| sub2(&x[k], &scale2(&w, &hit[k])))
                .collect();
            let roots = separate_roots(&g[0], &g[1], tile)
                .or_else(|| tile_roots(&g[0], &g[1], tile, false))
                .or_else(|| tile_roots(&g[0], &g[1], tile, true))
                .ok_or(bad)?;
            (roots, Some(sv.sign()))
        }
        None => {
            let (f1, f2) = (bern2(&c1), bern2(&c2));
            let roots = tile_roots(&f1, &f2, tile, false)
                .or_else(|| tile_roots(&f1, &f2, tile, true))
                .ok_or(bad)?;
            (roots, None)
        }
    };
    let h = bern2(&net_coefs(net, &up, &qv));
    // The facing: `(x_u × x_v)·UP`. On a planar tile `W⁴` times it from
    // the homogeneous partials, read anywhere; at a hit on another the
    // Jacobian of the column's equations, `det ∂(F₁, F₂)/∂(u, v) =
    // W²·|r̂|²·(x_u × x_v)·UP` there (`r̂·(x − q) = ν·(x − q) = 0` at the
    // hit, `r̂ × ν = |r̂|²·UP`), of far lower degree.
    let tangents = OnceCell::new();
    let tangents = || tangents.get_or_init(|| Tangents::of(&x, &w));
    let jacobian = || {
        let (f1, f2) = (bern2(&c1), bern2(&c2));
        sub2(&mul2(&du2(&f1), &dv2(&f2)), &mul2(&dv2(&f1), &du2(&f2)))
    };
    let (zero, one, half) = (Rational::ZERO, Rational::ONE, Rational::from_signeds(1, 2));
    // The local coordinates' half-open ranges: `(0, 1]`, `[0, 1]` at the
    // face's least tile line.
    let low = [
        tile[0] == face.tiles[0].start,
        tile[1] == face.tiles[1].start,
    ];
    let mut out = Vec::new();
    for (mut root, mult) in roots {
        root.plane = planar.clone();
        let inside = (0..2).all(|k| {
            let lo = root.coord_cmp(k, &zero);
            let hi = root.coord_cmp(k, &one);
            hi != Ordering::Greater
                && match lo {
                    Ordering::Greater => true,
                    Ordering::Equal => low[k],
                    Ordering::Less => false,
                }
        });
        if !inside {
            continue;
        }
        let height = match flat_height {
            Some(hs) => hs,
            None => sign_at_tile(&h, &root).ok_or(bad)?,
        };
        // Transversal where the tile's normal isn't along the column's
        // plane; a planar tile's facing is the same all over it (its
        // parametrisation regular), read at its middle. Else the column
        // touches the tile, two hits of opposite facing at one height.
        let facing = match &planar {
            Some(_) => sign_of(eval2(&dot2(&tangents().n, &up), &half, &half).sign()),
            None => sign_of(sign_at_tile(&jacobian(), &root).ok_or(bad)?),
        };
        let Some(facing) = facing else {
            if height != Ordering::Equal && mult % 2 == 0 {
                continue;
            }
            // The column tangent to the tile at `q`'s height: the fold
            // rule (module docs, "Folds").
            if height != Ordering::Equal || mult % 2 == 1 {
                return Err(bad);
            }
            let Some(upper) = fold(&x, &w, tangents(), &root, face.inside, motion).ok_or(bad)?
            else {
                continue;
            };
            let Some((wn, inc)) = in_trim(face, &root, height, ribbons)? else {
                continue;
            };
            if wn != face.inside {
                return Err(HitError::Winding(wn));
            }
            if inc.is_some() {
                // A fold on the face's boundary at `q`.
                return Err(bad);
            }
            let near = tile_space_box(net, &root);
            for (height, facing) in [(Ordering::Less, neg(upper)), (Ordering::Greater, upper)] {
                out.push(Hit {
                    sheet: Sheet::Tile(tile),
                    i: HitIx(0),
                    at: HitAt::Tile(Box::new(root.clone())),
                    facing: Some(facing),
                    height,
                    inc: None,
                    near: near.clone(),
                    on_surface: false,
                });
            }
            continue;
        };
        let facing = if face.inside < 0 { neg(facing) } else { facing };
        let Some((wn, inc)) = in_trim(face, &root, height, ribbons)? else {
            continue;
        };
        if wn != face.inside {
            return Err(HitError::Winding(wn));
        }
        let near = tile_space_box(net, &root);
        out.push(Hit {
            sheet: Sheet::Tile(tile),
            i: HitIx(0),
            at: HitAt::Tile(Box::new(root)),
            facing: Some(facing),
            height,
            inc,
            near,
            on_surface: false,
        });
    }
    for (k, h) in out.iter_mut().enumerate() {
        h.i = HitIx(u32::try_from(k).expect("few hits"));
    }
    Ok(out)
}

/// The trims' winding about the tile root (`None` for 0, outside) and,
/// on a trim of no width at `q`'s height, the incidence there (module
/// docs, "Tile hits"): about a rational point of a box round the hit that
/// no trim meets; where a trim meets every box tried, the point is on it.
fn in_trim(
    face: &HitFace<'_>,
    root: &TileRoot,
    height: Ordering,
    ribbons: &[Hit],
) -> Result<Option<(i64, Option<HitInc>)>, HitError> {
    const BITS: [u32; 4] = [30, 60, 120, 240];
    for bits in BITS {
        let bx = uv_box(root, bits);
        let met = face.trims.iter().enumerate().find_map(|(ti, t)| {
            t.pspans
                .iter()
                .position(|sp| span_meets(sp, &bx))
                .map(|si| (ti, si))
        });
        let Some((ti, si)) = met else {
            let two = Rational::from(2u32);
            let c = [0, 1].map(|k| (&bx[k][0] + &bx[k][1]) / &two);
            return match winding(face, &c) {
                Ok(0) => Ok(None),
                Ok(w) => Ok(Some((w, None))),
                Err(_) => Err(HitError::OnTrim(root.tile)),
            };
        };
        let t = &face.trims[ti];
        // On a trim where a ribbon has a hit at `λ = 0` in the box: that
        // hit is this point.
        let at_trim = ribbons
            .iter()
            .any(|rh| rh.on_surface && rh.in_rect(face, &bx));
        let flat = ribbon_flat(face, t, si);
        if at_trim && !flat {
            return Ok(None);
        }
        if bits == BITS[BITS.len() - 1] {
            if !flat {
                return Err(HitError::OnTrim(root.tile));
            }
            // A trim of no width: the point is on the edge's curve, inside
            // the face (its boundary, which the boxes' records place).
            let inc = if height == Ordering::Equal {
                trim_point(&t.pspans[si], &bx).map(|tau| inc_of(t, si, &tau))
            } else {
                None
            };
            return Ok(Some((face.inside, inc)));
        }
    }
    Err(HitError::OnTrim(root.tile))
}

/// A closed rational box of global UV holding the tile root, its free
/// coordinate (or each where they separate) narrowed to `2^−bits`, the
/// other enclosed over it.
fn uv_box(r: &TileRoot, bits: u32) -> ExactRect {
    let off = r.tile.map(Rational::from);
    let narrow = |x: &Root| -> [Rational; 2] {
        let mut x = x.clone();
        x.narrow(&half_pow(bits));
        x.interval().map(|y| y.clone())
    };
    let mut out: ExactRect = std::array::from_fn(|_| [Rational::ZERO, Rational::ZERO]);
    match &r.place {
        Place::Separate { u, v } => {
            for (k, x) in [u, v].into_iter().enumerate() {
                let [lo, hi] = narrow(x);
                out[k] = [&off[k] + lo, &off[k] + hi];
            }
        }
        Place::Param { u, s0, s1, swapped } => {
            let (kf, ko) = (usize::from(*swapped), 1 - usize::from(*swapped));
            let [ulo, uhi] = narrow(u);
            let n = poly::neg(s0);
            let [nlo, nhi] = poly::enclose(&n, &ulo, &uhi);
            let [mlo, mhi] = poly::enclose(s1, &ulo, &uhi);
            out[kf] = [&off[kf] + &ulo, &off[kf] + &uhi];
            if mlo.sign() != mhi.sign() || mlo == 0u32 {
                // The denominator not kept off zero over so wide an
                // interval: the tile's whole range of the other coordinate.
                out[ko] = [off[ko].clone(), &off[ko] + Rational::ONE];
            } else {
                // `n/m` over the interval, `m` of one sign there.
                let cands = [&nlo / &mlo, &nlo / &mhi, &nhi / &mlo, &nhi / &mhi];
                let [vlo, vhi] = min_max(&cands);
                out[ko] = [&off[ko] + vlo, &off[ko] + vhi];
            }
        }
    }
    out
}

/// The parameter of p-span `sp` at the box `bx`: a root of the span's `u`
/// against the box's middle `u`, else of its `v`.
fn trim_point(sp: &UvSpan, bx: &ExactRect) -> Option<Root> {
    let (uvw, w) = hom(sp);
    let two = Rational::from(2u32);
    for k in 0..2 {
        let c = (&bx[k][0] + &bx[k][1]) / &two;
        let f = poly::sub(&uvw[k], &poly::scale(&w, &c));
        if f.is_empty() {
            continue;
        }
        if let Some((r, _)) = poly::roots(&f, &Rational::ZERO, &Rational::ONE)
            .into_iter()
            .next()
        {
            return Some(r);
        }
    }
    None
}

/// A closed box of space holding the tile hit: the net's point at a
/// rational point of a box round the hit, grown by the net's extent times
/// that box's width (a bound on the patch's partials over the tile).
fn tile_space_box(net: &TileNet, r: &TileRoot) -> [[Rational; 2]; 3] {
    let bx = uv_box(r, 60);
    let off = r.tile.map(Rational::from);
    let two = Rational::from(2u32);
    let c: [Rational; 2] = std::array::from_fn(|k| (&bx[k][0] + &bx[k][1]) / &two - &off[k]);
    let width = (&bx[0][1] - &bx[0][0]) + (&bx[1][1] - &bx[1][0]);
    let (x, w) = net_polys(net);
    let at = |f: &Poly2| -> Rational {
        f.iter().enumerate().fold(Rational::ZERO, |s, (j, cj)| {
            let mut vj = Rational::ONE;
            for _ in 0..j {
                vj *= &c[1];
            }
            s + poly::eval(cj, &c[0]) * vj
        })
    };
    let wc = at(&w);
    std::array::from_fn(|k| {
        let p = at(&x[k]) / &wc;
        let ext = min_max(&net.iter().map(|q| rat(q[k])).collect::<Vec<_>>());
        let pad = (&ext[1] - &ext[0]) * Rational::from(4u32) * &width + half_pow(200);
        [&p - &pad, &p + &pad]
    })
}

// ---------------------------------------------------------------------
// Ribbon hits.

/// The incidence of a point at parameter `tau` of p-span `span` of `t`
/// on the edge.
fn inc_of(t: &HitTrim<'_>, span: usize, tau: &Root) -> HitInc {
    HitInc {
        edge: t.edge,
        span: t.map[span].span,
        t: edge_t(t, span, tau),
    }
}

/// The edge's parameter on its span at parameter `tau` of p-span `span`
/// of `t`, enclosed: the root narrowed to `2⁻⁶⁰`, mapped, widened by a
/// few ulps of the map's rounding, inside `[0, 1]`.
pub fn edge_t(t: &HitTrim<'_>, span: usize, tau: &Root) -> [f64; 2] {
    let m = t.map[span];
    let mut r = tau.clone();
    r.narrow(&half_pow(60));
    let [lo, hi] = r.enclose();
    let at = |x: f64| m.t[0] + (m.t[1] - m.t[0]) * x;
    let clamp = |x: f64| x.clamp(0.0, 1.0);
    let (a, b) = (at(clamp(lo)), at(clamp(hi)));
    let pad = 4.0 * f64::EPSILON;
    [clamp(a - pad), clamp(b + pad)]
}

/// A rational quadratic span's homogeneous coordinates in the power
/// basis: per coordinate `Σ wᵢ·xᵢ·Bᵢ(τ)`, and the weight `Σ wᵢ·Bᵢ(τ)`.
fn hom<const N: usize>(pts: &[[f64; N]; 3]) -> ([Poly; 3], Poly) {
    let w = N - 1;
    let wb: Vec<Rational> = pts.iter().map(|p| rat(p[w])).collect();
    let coord = |k: usize| -> Poly {
        if k >= w {
            return Vec::new();
        }
        let b: Vec<Rational> = pts.iter().map(|p| rat(p[w]) * rat(p[k])).collect();
        poly::from_bernstein(&b)
    };
    ([coord(0), coord(1), coord(2)], poly::from_bernstein(&wb))
}

/// The ribbon's two curves over p-span `span` of `t`: `S(p(τ))` on the
/// span's tile and `C(t(τ))`, homogeneous, with their weights.
struct RibbonPolys {
    p: [Poly; 3],
    wp: Poly,
    c: [Poly; 3],
    wc: Poly,
}

fn ribbon_polys(face: &HitFace<'_>, t: &HitTrim<'_>, span: usize) -> Option<RibbonPolys> {
    let sp = &t.pspans[span];
    let tile = face.tile_of_span(sp);
    let net = face.net(tile)?;
    let (uvw, w) = hom(sp);
    // Local coordinates, homogeneous: `û − i·ŵ`, `v̂ − j·ŵ`; the Bernstein
    // basis of degree two in them over the weight `ŵ`.
    let lu = poly::sub(&uvw[0], &poly::scale(&w, &Rational::from(tile[0])));
    let lv = poly::sub(&uvw[1], &poly::scale(&w, &Rational::from(tile[1])));
    let basis = |l: &Poly| -> [Poly; 3] {
        let r = poly::sub(&w, l);
        [
            poly::mul(&r, &r),
            poly::scale(&poly::mul(l, &r), &Rational::from(2u32)),
            poly::mul(l, l),
        ]
    };
    let (bu, bv) = (basis(&lu), basis(&lv));
    let (x, wn) = net_hom(net);
    let mut p: [Poly; 3] = std::array::from_fn(|_| Vec::new());
    let mut wp: Poly = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let b = poly::mul(&bu[i], &bv[j]);
            for k in 0..3 {
                p[k] = poly::add(&p[k], &poly::scale(&b, &x[k][i][j]));
            }
            wp = poly::add(&wp, &poly::scale(&b, &wn[i][j]));
        }
    }
    let m = t.map[span];
    let (cs, wc) = hom(&t.curve[m.span as usize]);
    let (a, bb) = (rat(m.t[0]), rat(m.t[1]) - rat(m.t[0]));
    let c = cs.map(|x| poly::compose_affine(&x, &a, &bb));
    let wc = poly::compose_affine(&wc, &a, &bb);
    Some(RibbonPolys { p, wp, c, wc })
}

/// Whether the ribbon of p-span `span` of `t` has no area: `S∘p` is the
/// edge's curve there exactly, or the ribbon's normal `R_t × R_λ`
/// vanishes all over it (its radials and both curves on one line).
fn ribbon_flat(face: &HitFace<'_>, t: &HitTrim<'_>, span: usize) -> bool {
    ribbon_polys(face, t, span).is_some_and(|r| no_area(&r))
}

fn no_area(r: &RibbonPolys) -> bool {
    if (0..3).all(|k| poly::mul(&r.p[k], &r.wc) == poly::mul(&r.c[k], &r.wp)) {
        return true;
    }
    // `R_λ ∝ C·w_p − P·w_c`; `R_t` spans `P′ ∝ P′·w_p − P·w_p′` and
    // `C′ ∝ C′·w_c − C·w_c′` as `λ` runs: none area where both cross
    // `R_λ` to zero.
    let rl: [Poly; 3] =
        std::array::from_fn(|k| poly::sub(&poly::mul(&r.c[k], &r.wp), &poly::mul(&r.p[k], &r.wc)));
    let tangent = |x: &[Poly; 3], w: &Poly| -> [Poly; 3] {
        let dw = poly::deriv(w);
        std::array::from_fn(|k| {
            poly::sub(&poly::mul(&poly::deriv(&x[k]), w), &poly::mul(&x[k], &dw))
        })
    };
    let crossed_zero = |a: &[Poly; 3]| {
        (0..3).all(|k| {
            let (i, j) = ((k + 1) % 3, (k + 2) % 3);
            poly::mul(&a[i], &rl[j]) == poly::mul(&a[j], &rl[i])
        })
    };
    crossed_zero(&tangent(&r.p, &r.wp)) && crossed_zero(&tangent(&r.c, &r.wc))
}

/// A vector of polynomials dotted with a constant vector.
fn vdot(v: &[Poly; 3], d: &Q3) -> Poly {
    (0..3).fold(Vec::new(), |acc, k| {
        poly::add(&acc, &poly::scale(&v[k], &d[k]))
    })
}

/// The hits of the column of `q` on the ribbon of p-span `span` of trim
/// `trim`, in ordinal order (module docs, "Ribbon hits").
pub fn ribbon_hits(
    face: &HitFace<'_>,
    trim: usize,
    span: usize,
    q: [f64; 3],
) -> Result<Vec<Hit>, HitError> {
    let t = &face.trims[trim];
    let degenerate = HitError::Degenerate {
        coedge: t.coedge,
        span: span as u32,
    };
    let qv = q.map(rat);
    let [a, b, u] = frame_dirs();
    // The ribbon within the control points of its tile and of its edge's
    // span (positive weights): away from the column in the shadow, no
    // hit, before the ribbon's polynomials are formed.
    let Some(net) = face.net(face.tile_of_span(&t.pspans[span])) else {
        return Err(degenerate);
    };
    let ctrl: Vec<[f64; 3]> = net
        .iter()
        .chain(&t.curve[t.map[span].span as usize])
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    let hull: [[Rational; 2]; 3] =
        std::array::from_fn(|k| min_max(&ctrl.iter().map(|p| rat(p[k])).collect::<Vec<_>>()));
    if shadow_misses(&hull, &qv, &a) || shadow_misses(&hull, &qv, &b) {
        return Ok(Vec::new());
    }
    let Some(r) = ribbon_polys(face, t, span) else {
        return Err(degenerate);
    };
    if no_area(&r) {
        return Ok(Vec::new());
    }
    let near = ribbon_box(&r);
    // Away from the column in the shadow: no hit.
    if shadow_misses(&near, &qv, &a) || shadow_misses(&near, &qv, &b) {
        return Ok(Vec::new());
    }
    let g: [Poly; 3] = std::array::from_fn(|k| poly::sub(&r.p[k], &poly::scale(&r.wp, &qv[k])));
    let h: [Poly; 3] = std::array::from_fn(|k| poly::sub(&r.c[k], &poly::scale(&r.wc, &qv[k])));
    let (ga, gb, gu) = (vdot(&g, &a), vdot(&g, &b), vdot(&g, &u));
    let (ha, hb, hu) = (vdot(&h, &a), vdot(&h, &b), vdot(&h, &u));
    let d = poly::sub(&poly::mul(&ga, &hb), &poly::mul(&gb, &ha));
    if d.is_empty() {
        return Err(degenerate);
    }
    let dd = poly::deriv(&d);
    let (zero, one) = (Rational::ZERO, Rational::ONE);
    let mut out = Vec::new();
    for (root, mult) in poly::roots(&d, &zero, &one) {
        // Each knot and vertex belongs to the span ending there in the
        // loop's order.
        let start = if t.reversed { &one } else { &zero };
        if root.rational() == Some(start) {
            continue;
        }
        // The axis whose components at the root aren't both zero.
        let (sa, sh) = (root.sign_of(&ga), root.sign_of(&ha));
        let (gx, hx, sg, shx) = if sa != Ordering::Equal || sh != Ordering::Equal {
            (&ga, &ha, sa, sh)
        } else {
            let (sb, shb) = (root.sign_of(&gb), root.sign_of(&hb));
            (&gb, &hb, sb, shb)
        };
        let hit = |facing, height, inc, on_surface| Hit {
            sheet: Sheet::Ribbon(t.coedge, span as u32),
            i: HitIx(0),
            facing,
            height,
            inc,
            near: near.clone(),
            on_surface,
            at: HitAt::Ribbon {
                trim,
                span,
                tau: root.clone(),
            },
        };
        if sg == Ordering::Equal && shx == Ordering::Equal {
            // Both ends of the radial on the column's line: a hit at `q`
            // where they meet there in space, else the column along it.
            let g_at = (0..3).all(|k| root.sign_of(&g[k]) == Ordering::Equal);
            let h_at = (0..3).all(|k| root.sign_of(&h[k]) == Ordering::Equal);
            if g_at && h_at {
                out.push(hit(
                    None,
                    Ordering::Equal,
                    Some(inc_of(t, span, &root)),
                    true,
                ));
                continue;
            }
            return Err(degenerate);
        }
        // On the radial: `G` and `H` on opposite sides of the column.
        if sg == shx {
            continue;
        }
        let e = poly::sub(&poly::mul(gx, &hu), &poly::mul(hx, &gu));
        let l = poly::sub(&poly::mul(gx, &r.wc), &poly::mul(hx, &r.wp));
        let height = mul_ord(root.sign_of(&e), root.sign_of(&l));
        let facing = match mult {
            1 => {
                let f = sign_of(root.sign_of(&dd)).expect("a simple root");
                Some(if t.reversed { f } else { neg(f) })
            }
            m if m % 2 == 0 && height != Ordering::Equal => continue,
            _ => {
                return Err(HitError::Tangent {
                    coedge: t.coedge,
                    span: span as u32,
                });
            }
        };
        let on_curve = shx == Ordering::Equal;
        let inc = (on_curve && height == Ordering::Equal).then(|| inc_of(t, span, &root));
        // At `λ = 0`: on the tile's surface, the trim's point.
        out.push(hit(facing, height, inc, sg == Ordering::Equal));
    }
    if t.along == Along::Reversed {
        out.reverse();
    }
    for (k, h) in out.iter_mut().enumerate() {
        h.i = HitIx(u32::try_from(k).expect("few hits"));
    }
    Ok(out)
}

/// Whether the box `b` lies on one side of `q` along the shadow direction
/// `d`: `d·x` of one strict sign against `d·q` over its corners.
fn shadow_misses(b: &[[Rational; 2]; 3], q: &Q3, d: &Q3) -> bool {
    let dq = &d[0] * &q[0] + &d[1] * &q[1] + &d[2] * &q[2];
    // `d·x` over the box: per coordinate the lesser and greater term.
    let mut lo = Rational::ZERO;
    let mut hi = Rational::ZERO;
    for k in 0..3 {
        let (x, y) = (&d[k] * &b[k][0], &d[k] * &b[k][1]);
        let (mn, mx) = if x <= y { (x, y) } else { (y, x) };
        lo += mn;
        hi += mx;
    }
    lo > dq || hi < dq
}

/// A closed box of space holding the ribbon over its span: the control
/// boxes of its two curves (positive weights keep each in its control
/// points' hull, and the radials between them).
fn ribbon_box(r: &RibbonPolys) -> [[Rational; 2]; 3] {
    std::array::from_fn(|k| {
        let mut vals = Vec::new();
        for (x, w) in [(&r.p[k], &r.wp), (&r.c[k], &r.wc)] {
            let n = w.len().max(x.len()).max(1);
            let (bx, bw) = (poly::to_bernstein(x, n), poly::to_bernstein(w, n));
            for (cx, cw) in bx.iter().zip(&bw) {
                vals.push(cx / cw);
            }
        }
        min_max(&vals)
    })
}

/// Every hit of the column of `q` on the face: its tiles' hits, then the
/// ribbons' by trim and span (module docs).
pub fn face_hits(face: &HitFace<'_>, q: [f64; 3], motion: &Motion) -> Result<Vec<Hit>, HitError> {
    let ribbons = all_ribbon_hits(face, q)?;
    let mut out = tile_hits(face, q, &ribbons, motion)?;
    out.extend(ribbons);
    Ok(out)
}

fn all_ribbon_hits(face: &HitFace<'_>, q: [f64; 3]) -> Result<Vec<Hit>, HitError> {
    let mut out = Vec::new();
    for (k, t) in face.trims.iter().enumerate() {
        for s in 0..t.pspans.len() {
            out.extend(ribbon_hits(face, k, s, q)?);
        }
    }
    Ok(out)
}

/// The hit of the column of `q` on `sheet` with ordinal `i`, as
/// [`face_hits`] numbers it; `None` if there is none.
pub fn hit_on(
    face: &HitFace<'_>,
    sheet: Sheet,
    i: HitIx,
    q: [f64; 3],
    motion: &Motion,
) -> Result<Option<Hit>, HitError> {
    match sheet {
        Sheet::Tile(tile) => {
            // The trims' ribbon hits at `λ = 0` say which tile points lie
            // on the trims.
            let ribbons = all_ribbon_hits(face, q)?;
            let hits = on_tile(face, tile, q, &ribbons, motion)?;
            Ok(hits.into_iter().find(|h| h.i == i))
        }
        Sheet::Ribbon(c, span) => {
            let Some(trim) = face.trims.iter().position(|t| t.coedge == c) else {
                return Ok(None);
            };
            if span as usize >= face.trims[trim].pspans.len() {
                return Ok(None);
            }
            let hits = ribbon_hits(face, trim, span as usize, q)?;
            Ok(hits.into_iter().find(|h| h.i == i))
        }
    }
}

// ---------------------------------------------------------------------
// The trims' winding.

/// The winding of the face's trims round the UV point `p`, exactly
/// (module docs, "Tile hits"), or where `p` lies on a trim.
pub fn winding(face: &HitFace<'_>, p: &[Rational; 2]) -> Result<i64, OnTrim> {
    winding_of(face.trims.iter().map(|t| (t.pspans, t.reversed)), p)
}

/// [`winding`] of the trims `trims` (each its p-spans along its edge and
/// whether its coedge runs against them), `OnTrim` naming a trim by its
/// place among them.
pub fn winding_of<'t>(
    trims: impl IntoIterator<Item = (&'t [UvSpan], bool)>,
    p: &[Rational; 2],
) -> Result<i64, OnTrim> {
    let mut w = 0i64;
    let (zero, one) = (Rational::ZERO, Rational::ONE);
    for (ti, (pspans, reversed)) in trims.into_iter().enumerate() {
        let mut n = 0i64;
        for (si, sp) in pspans.iter().enumerate() {
            // The control points' box clear of the ray: no crossing.
            let (vs, us) = (sp.map(|c| rat(c[1])), sp.map(|c| rat(c[0])));
            if vs.iter().all(|v| v < &p[1])
                || vs.iter().all(|v| v > &p[1])
                || us.iter().all(|u| u < &p[0])
            {
                continue;
            }
            let (uvw, ww) = hom(sp);
            let f = poly::sub(&uvw[1], &poly::scale(&ww, &p[1]));
            let g = poly::sub(&uvw[0], &poly::scale(&ww, &p[0]));
            let on = |tau: Root| OnTrim {
                trim: ti,
                span: si,
                tau: Box::new(tau),
            };
            if f.is_empty() {
                // Along the ray's line: no crossing, `p` on it or not.
                if g.is_empty() {
                    // A span of one point, `p`.
                    let at0 = poly::roots(&[zero.clone(), one.clone()], &zero, &one);
                    return Err(on(at0.into_iter().next().expect("τ = 0").0));
                }
                if let Some((r, _)) = poly::roots(&g, &zero, &one).into_iter().next() {
                    return Err(on(r));
                }
                continue;
            }
            for (r, m) in poly::roots(&f, &zero, &one) {
                let right = r.sign_of(&g);
                if right == Ordering::Equal {
                    return Err(on(r));
                }
                if right == Ordering::Less {
                    continue;
                }
                // The sign of `f` just after the root: its `m`th
                // derivative's; just before: times `(−1)^m`.
                let mut dm = f.clone();
                for _ in 0..m {
                    dm = poly::deriv(&dm);
                }
                let after = r.sign_of(&dm);
                let before = if m % 2 == 0 { after } else { after.reverse() };
                if r.rational() != Some(&zero) && before == Ordering::Less {
                    n += 1;
                }
                if r.rational() != Some(&one) && after == Ordering::Less {
                    n -= 1;
                }
            }
        }
        w += if reversed { -n } else { n };
    }
    Ok(w)
}

/// Whether p-span `sp` (affine control points with weights, in UV) meets
/// the closed rectangle `r`, exactly: an end inside, or a crossing of one
/// of its sides' lines within the side.
pub fn span_meets(sp: &UvSpan, r: &ExactRect) -> bool {
    let inside = |p: &[f64; 3]| (0..2).all(|k| r[k][0] <= rat(p[k]) && rat(p[k]) <= r[k][1]);
    if inside(&sp[0]) || inside(&sp[2]) {
        return true;
    }
    // The control points beyond a side: the span misses the rectangle.
    let away = (0..2)
        .any(|k| sp.iter().all(|p| rat(p[k]) < r[k][0]) || sp.iter().all(|p| rat(p[k]) > r[k][1]));
    if away {
        return false;
    }
    let (uvw, w) = hom(sp);
    let (zero, one) = (Rational::ZERO, Rational::ONE);
    for k in 0..2 {
        let o = 1 - k;
        for c in &r[k] {
            let f = poly::sub(&uvw[k], &poly::scale(&w, c));
            let lo = poly::sub(&uvw[o], &poly::scale(&w, &r[o][0]));
            let hi = poly::sub(&uvw[o], &poly::scale(&w, &r[o][1]));
            if f.is_empty() {
                // Along the side's line: clear only where its other
                // coordinate stays below the side's start or above its end.
                let below = !lo.is_empty()
                    && poly::roots(&lo, &zero, &one).is_empty()
                    && poly::sign_at(&lo, &zero) == Ordering::Less;
                let above = !hi.is_empty()
                    && poly::roots(&hi, &zero, &one).is_empty()
                    && poly::sign_at(&hi, &zero) == Ordering::Greater;
                if !(below || above) {
                    return true;
                }
                continue;
            }
            for (root, _) in poly::roots(&f, &zero, &one) {
                if root.sign_of(&lo) != Ordering::Less && root.sign_of(&hi) != Ordering::Greater {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------
// Germ keys' rows.

/// Where a germ key's anchor point lies on a face's boundary
/// ([`HitFace::germ_orders`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GermAt {
    /// At the face's vertex.
    Vertex(VertId),
    /// At the asking point itself, exactly on the edge's curve.
    Curve(EdgeId),
}

/// `f(l_u/w, l_v/w)·w^(d_u + d_v)`, `d_u`, `d_v` `f`'s degrees in `u`
/// and `v`: a tile polynomial along a p-span, homogeneous in its weight
/// `w` (positive, so of the sign of `f` there).
fn along_span(f: &Poly2, lu: &Poly, lv: &Poly, w: &Poly) -> Poly {
    let du = f
        .iter()
        .map(|c| c.len())
        .max()
        .unwrap_or(1)
        .saturating_sub(1);
    let dv = f.len().saturating_sub(1);
    let pow =
        |p: &Poly, n: usize| (0..n).fold(poly::constant(Rational::ONE), |a, _| poly::mul(&a, p));
    let mut out: Poly = Vec::new();
    for (j, cj) in f.iter().enumerate() {
        for (i, c) in cj.iter().enumerate() {
            if *c == 0u32 {
                continue;
            }
            let t = poly::mul(
                &poly::mul(&pow(lu, i), &pow(lv, j)),
                &pow(w, du - i + dv - j),
            );
            out = poly::add(&out, &poly::scale(&t, c));
        }
    }
    out
}

impl HitFace<'_> {
    /// The exact orders of a germ key's row at this face (§ "Rows" in
    /// [`crate::perturb`]): the face's tangent plane at the anchor point
    /// against the moving point, `(n·Δₖ)·sign(n·UP)`, `deltas` the face's
    /// rates less the point's; the anchor point the vertex `w` (its UV
    /// the p-curve's end there), or `q` itself on the edge's curve (its UV
    /// the p-curve's point where the curve passes through `q`, exactly).
    /// `None` where the point isn't found on the face's trims, or the
    /// face's facing there is zero (a silhouette through it).
    pub fn germ_orders(
        &self,
        at: GermAt,
        q: [f64; 3],
        deltas: &Motion,
    ) -> Option<[ExactSign; ORDERS]> {
        let up: Q3 = [UP.x, UP.y, UP.z].map(rat);
        let row = |n: &dyn Fn(&Q3) -> Ordering| -> Option<[ExactSign; ORDERS]> {
            let facing = n(&up);
            if facing == Ordering::Equal {
                return None;
            }
            Some(deltas.each_ref().map(|d| Ok(mul_ord(n(d), facing))))
        };
        match at {
            GermAt::Vertex(w) => {
                let (sp, uv) = self.trims.iter().find_map(|t| {
                    let last = t.pspans.len().checked_sub(1)?;
                    if t.ends[0] == w {
                        Some((&t.pspans[0], t.pspans[0][0]))
                    } else if t.ends[1] == w {
                        Some((&t.pspans[last], t.pspans[last][2]))
                    } else {
                        None
                    }
                })?;
                let tile = self.tile_of_span(sp);
                let (x, wt) = net_polys(self.net(tile)?);
                let n = Tangents::of(&x, &wt).n;
                let local = [0, 1].map(|k| rat(uv[k]) - Rational::from(tile[k]));
                let nv: Q3 = n.each_ref().map(|c| eval2(c, &local[0], &local[1]));
                row(&|d| dot(&nv, d).sign())
            }
            GermAt::Curve(g) => {
                let qv = q.map(rat);
                for t in self.trims.iter().filter(|t| t.edge == g) {
                    for si in 0..t.pspans.len() {
                        let Some(tau) = through(t, si, &qv) else {
                            continue;
                        };
                        let sp = &t.pspans[si];
                        let tile = self.tile_of_span(sp);
                        let (x, wt) = net_polys(self.net(tile)?);
                        let n = Tangents::of(&x, &wt).n;
                        let (uvw, w) = hom(sp);
                        let lu = poly::sub(&uvw[0], &poly::scale(&w, &Rational::from(tile[0])));
                        let lv = poly::sub(&uvw[1], &poly::scale(&w, &Rational::from(tile[1])));
                        let nt: [Poly; 3] = n.each_ref().map(|c| along_span(c, &lu, &lv, &w));
                        return row(&|d| tau.sign_of(&vdot(&nt, d)));
                    }
                }
                None
            }
        }
    }
}

/// The first parameter of p-span `span` of `t` where the edge's curve
/// passes through `q` exactly, if it does.
fn through(t: &HitTrim<'_>, span: usize, q: &Q3) -> Option<Root> {
    let m = t.map[span];
    let (cs, wc) = hom(t.curve.get(m.span as usize)?);
    let (a, b) = (rat(m.t[0]), rat(m.t[1]) - rat(m.t[0]));
    let wc = poly::compose_affine(&wc, &a, &b);
    let h: Vec<Poly> = (0..3)
        .map(|k| {
            poly::sub(
                &poly::compose_affine(&cs[k], &a, &b),
                &poly::scale(&wc, &q[k]),
            )
        })
        .collect();
    let mut g: Poly = Vec::new();
    for hk in &h {
        g = poly::gcd(&g, hk);
    }
    if poly::degree(&g).unwrap_or(0) == 0 {
        return None;
    }
    poly::roots(&g, &Rational::ZERO, &Rational::ONE)
        .into_iter()
        .next()
        .map(|(r, _)| r)
}

/// `a·b`, exactly.
fn dot(a: &Q3, b: &Q3) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

// ---------------------------------------------------------------------
// Membership, nearness, orders.

impl Hit {
    /// The hit's UV point against the closed rectangle `r`: whether it
    /// lies in it, exactly.
    pub fn in_rect(&self, face: &HitFace<'_>, r: &ExactRect) -> bool {
        self.in_rect_as(face, r, false)
    }

    /// Whether the hit's UV point lies strictly inside the rectangle `r`,
    /// exactly.
    pub fn in_rect_open(&self, face: &HitFace<'_>, r: &ExactRect) -> bool {
        self.in_rect_as(face, r, true)
    }

    fn in_rect_as(&self, face: &HitFace<'_>, r: &ExactRect, open: bool) -> bool {
        // `lo ≤ x ≤ hi`, or `lo < x < hi` where open, by the signs of
        // `x − lo` and `x − hi`.
        let fits = |lo: Ordering, hi: Ordering| {
            if open {
                lo == Ordering::Greater && hi == Ordering::Less
            } else {
                lo != Ordering::Less && hi != Ordering::Greater
            }
        };
        match &self.at {
            HitAt::Tile(t) => {
                // The tile's square first.
                let off = t.tile.map(Rational::from);
                if (0..2).any(|k| &off[k] + Rational::ONE < r[k][0] || off[k] > r[k][1]) {
                    return false;
                }
                (0..2).all(|k| {
                    let lo = t.coord_cmp(k, &(&r[k][0] - &off[k]));
                    let hi = t.coord_cmp(k, &(&r[k][1] - &off[k]));
                    fits(lo, hi)
                })
            }
            HitAt::Ribbon { trim, span, tau } => {
                let sp = &face.trims[*trim].pspans[*span];
                // The span's control box first: a rectangle away from it
                // holds none of its points.
                let away = (0..2).any(|k| {
                    sp.iter().all(|p| rat(p[k]) < r[k][0]) || sp.iter().all(|p| rat(p[k]) > r[k][1])
                });
                if away {
                    return false;
                }
                let (uvw, w) = hom(sp);
                (0..2).all(|k| {
                    let lo = poly::sub(&uvw[k], &poly::scale(&w, &r[k][0]));
                    let hi = poly::sub(&uvw[k], &poly::scale(&w, &r[k][1]));
                    fits(tau.sign_of(&lo), tau.sign_of(&hi))
                })
            }
        }
    }

    /// Whether the hit lies within edge `g`'s near radius of `g`'s curve
    /// (the lobe fallback's neighbourhood): a hit on a ribbon of `g`
    /// always; another where its box of space reaches within the radius
    /// of the curve's control box, a superset.
    pub fn near_edge(&self, face: &HitFace<'_>, g: EdgeId) -> bool {
        if let HitAt::Ribbon { trim, .. } = &self.at
            && face.trims[*trim].edge == g
        {
            return true;
        }
        face.trims.iter().filter(|t| t.edge == g).any(|t| {
            let r = rat(t.near_radius());
            t.curve.iter().any(|s| {
                (0..3).all(|k| {
                    let [lo, hi] = min_max(&s.iter().map(|p| rat(p[k])).collect::<Vec<_>>());
                    let (lo, hi) = (lo - &r, hi + &r);
                    self.near[k][0] <= hi && lo <= self.near[k][1]
                })
            })
        })
    }

    /// The exact orders of the hit's height at a tie: the sheet's tangent
    /// plane at the hit against the moving point, `(n·Δₖ)·sign(n·UP)` per
    /// motion, `deltas` the sheet's rates less `q`'s per motion
    /// ([`ribbon_row_signs`] on a ribbon). `None` where the hit's tile or
    /// ribbon isn't the face's.
    pub fn orders(
        &self,
        face: &HitFace<'_>,
        q: [f64; 3],
        deltas: &[Q3; ORDERS],
    ) -> Option<[ExactSign; ORDERS]> {
        match &self.at {
            HitAt::Ribbon { .. } => self.ribbon_orders(face, q, deltas),
            HitAt::Tile(t) => {
                let up = [UP.x, UP.y, UP.z].map(rat);
                let dot = |a: &Q3, b: &Q3| &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2];
                if let Some(n) = &t.plane {
                    // The plane itself is the tangent plane.
                    let facing = dot(n, &up).sign();
                    return Some(
                        deltas
                            .each_ref()
                            .map(|d| Ok(mul_ord(dot(n, d).sign(), facing))),
                    );
                }
                let net = face.net(t.tile)?;
                let (x, w) = net_polys(net);
                // `n ∝ (X_u·W − X·W_u) × (X_v·W − X·W_v)`, `W⁴` times the
                // normal `x_u × x_v`.
                let n = Tangents::of(&x, &w).n;
                let along = |d: &Q3| -> Poly2 { dot2(&n, d) };
                let facing = sign_at_tile(&along(&up), t)?;
                let mut out = [Ok(Ordering::Equal); ORDERS];
                for (o, d) in out.iter_mut().zip(deltas) {
                    *o = Ok(mul_ord(sign_at_tile(&along(d), t)?, facing));
                }
                Some(out)
            }
        }
    }

    /// The exact orders of a ribbon hit's height at a tie: the ribbon's
    /// tangent plane at the hit against the moving point
    /// ([`ribbon_row_signs`]), `deltas` the ribbon point's rates less
    /// `q`'s per motion. `None` for a tile hit.
    pub fn ribbon_orders(
        &self,
        face: &HitFace<'_>,
        q: [f64; 3],
        deltas: &[Q3; ORDERS],
    ) -> Option<[ExactSign; ORDERS]> {
        let HitAt::Ribbon { trim, span, tau } = &self.at else {
            return None;
        };
        let t = &face.trims[*trim];
        let r = ribbon_polys(face, t, *span)?;
        let qv = q.map(rat);
        let g: [Poly; 3] = std::array::from_fn(|k| poly::sub(&r.p[k], &poly::scale(&r.wp, &qv[k])));
        let h: [Poly; 3] = std::array::from_fn(|k| poly::sub(&r.c[k], &poly::scale(&r.wc, &qv[k])));
        let [a, b, _] = frame_dirs();
        let (mut ag, mut ah) = (vdot(&g, &a), vdot(&h, &a));
        if tau.sign_of(&ag) == Ordering::Equal && tau.sign_of(&ah) == Ordering::Equal {
            ag = vdot(&g, &b);
            ah = vdot(&h, &b);
        }
        // `λ = A_g/(A_g − A_h)`, `A_g = G_x·w_c`, `A_h = H_x·w_p`: then
        // `R_t ∝ −A_h·P̃·w_c² + A_g·C̃·w_p²` and `R_λ ∝ C·w_p − P·w_c`, with
        // `P̃ = P′·w_p − P·w_p′`, `C̃ = C′·w_c − C·w_c′`.
        let a_g = poly::mul(&ag, &r.wc);
        let a_h = poly::mul(&ah, &r.wp);
        let (wp2, wc2) = (poly::mul(&r.wp, &r.wp), poly::mul(&r.wc, &r.wc));
        let (dwp, dwc) = (poly::deriv(&r.wp), poly::deriv(&r.wc));
        let rt: [Poly; 3] = std::array::from_fn(|k| {
            let pt = poly::sub(
                &poly::mul(&poly::deriv(&r.p[k]), &r.wp),
                &poly::mul(&r.p[k], &dwp),
            );
            let ct = poly::sub(
                &poly::mul(&poly::deriv(&r.c[k]), &r.wc),
                &poly::mul(&r.c[k], &dwc),
            );
            poly::sub(
                &poly::mul(&a_g, &poly::mul(&ct, &wp2)),
                &poly::mul(&a_h, &poly::mul(&pt, &wc2)),
            )
        });
        let rl: [Poly; 3] = std::array::from_fn(|k| {
            poly::sub(&poly::mul(&r.c[k], &r.wp), &poly::mul(&r.p[k], &r.wc))
        });
        Some(ribbon_row_signs(&rt, &rl, tau, deltas))
    }
}

#[cfg(test)]
mod tests;
