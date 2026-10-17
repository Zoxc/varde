//! The high-precision counting oracle: signed crossings of a ray or of
//! an edge curve with a face's counting surface, decided in exact
//! arithmetic, for checking the Varde kernel's counting.
//!
//! # Geometry
//!
//! All inputs are f64, so every value is an exact dyadic rational and
//! the oracle computes exactly with it; it shares no code with the
//! kernel.
//!
//! - A control point `[x, y, z, w]` is an affine point and a weight
//!   `w > 0`; its homogeneous point is `(w·x, w·y, w·z, w)`.
//! - A patch is rational biquadratic: `net[3·i + j]` the control point
//!   `(i, j)`, `i` along `u`, over `[0, 1]²`.
//! - A [`Chain`] (an edge curve) is a run of rational quadratic spans,
//!   span `k` over `t ∈ [knots[k], knots[k + 1]]` mapped linearly onto
//!   its own `[0, 1]`; a [`UvChain`] (a p-curve) the same with
//!   `[u, v, w]` control points.
//! - A [`Face`] is patches (tiles) over unit squares of its UV plane,
//!   tile `[i, j]` over `[i, i + 1] × [j, j + 1]`, and loops of
//!   [`Coedge`]s: a p-curve in the face's UV and the edge's curve over
//!   the same parameter `t`, the range of it used, and whether the loop
//!   runs against `t`. Consecutive coedges join exactly (p-curve ends
//!   and edge-curve ends equal bit for bit), and the loops close in UV.
//!   Tiles meeting along a line must agree on it bit for bit (share the
//!   boundary row of control points), or the surface has a gap there.
//!
//! # The counting surface
//!
//! A face's counting surface is an oriented 2-chain whose boundary is
//! its edge curves, so the faces of a closed solid close up without any
//! normals being given:
//!
//! - the patch part: each tile's patch over its square, oriented by
//!   `x_u × x_v` and weighted by the winding number of the face's loops
//!   (each p-curve run in its coedge's direction) around the UV point;
//! - a ribbon per coedge, `R(t, λ) = (1 − λ)·S(p(t)) + λ·C(t)` over the
//!   coedge's range and `λ ∈ [0, 1]`, `S` the surface, `p` the p-curve,
//!   `C` the edge curve, oriented by `R_λ × R_t` along `t` and
//!   `R_t × R_λ` against it, weight 1. Its `λ = 0` side cancels the
//!   patch part's boundary and its `λ = 1` side is the edge curve. A
//!   ribbon whose p-curve image is the edge curve exactly has no area
//!   and is left out.
//!
//! [`ExactFace::ray`] counts `Σ sign(N·dir)·weight` over the crossings
//! with the ray `q + s·dir`, `s > 0`; for a closed solid's faces the sum
//! is the winding number of `q`. [`ExactFace::curve`] counts
//! `Σ −sign(C′·N)·weight` over the crossings with a chain, ends
//! included (entering counts +1): for a closed solid, `w(end) −
//! w(start)`.
//!
//! # Ties
//!
//! A configuration is degenerate when a crossing sits where the count
//! changes: the ray starts on the surface, a crossing is on a tile line,
//! a trim, a ribbon side or end or a curve end, or isn't transversal.
//! The counting checked resolves such ties by moving one operand by a
//! symbolically small displacement; [`Shift::tie`] is an exact finite
//! stand-in for it, small enough to change no count that isn't tied.
//! Each query takes the shift of the face and of the query point or
//! curve; it counts unshifted first and, if any piece of the face is
//! degenerate, recounts the whole face with the shifts, reporting that
//! in [`Crossings::tie`]. Both are made for data spanning a few dozen
//! binades about unit size: for finer or smaller data (a coordinate a
//! few subnormals off zero, a solid `2^-1000` across) the unshifted
//! count may subdivide further and a tie shift is scaled down to stay
//! below the data. A count is certified throughout: subdivision of
//! Bernstein forms with exact sign tests and a Poincaré–Miranda degree
//! test (see `solve`); f64 is used only for bounding-box rejection with
//! a wide margin and to guide where to subdivide.

mod bern;
mod exact;
mod solve;
mod trim;

#[cfg(test)]
mod tests;

use bern::Bern;
use exact::{Dy, common, cross, dot, dy3, integerize, q, rational_approx};
use malachite::base::num::basic::traits::{One, Zero};
use malachite::{Integer, Rational};
use solve::{Caps, Degenerate, Weight, count};
use trim::{PSpan, Trims};

/// A curve: rational quadratic spans of `[x, y, z, w]` control points,
/// span `k` over `[knots[k], knots[k + 1]]`, consecutive spans sharing
/// their joining control point.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub spans: Vec<[[f64; 4]; 3]>,
    pub knots: Vec<f64>,
}

/// A p-curve: as [`Chain`], with `[u, v, w]` control points.
#[derive(Clone, Debug, PartialEq)]
pub struct UvChain {
    pub spans: Vec<[[f64; 3]; 3]>,
    pub knots: Vec<f64>,
}

/// A face's use of an edge: the p-curve in the face's UV and the edge
/// curve over the same `t`, the part `t_range` of both used (two knots
/// of each), and whether the loop runs against `t`.
#[derive(Clone, Debug, PartialEq)]
pub struct Coedge {
    pub pcurve: UvChain,
    pub curve: Chain,
    pub t_range: [f64; 2],
    pub reversed: bool,
}

/// A trimmed face: tiles `([i, j], net)` over `[i, i + 1] × [j, j + 1]`
/// of its UV plane, and loops of coedges.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub tiles: Vec<([i64; 2], [[f64; 4]; 9])>,
    pub loops: Vec<Vec<Coedge>>,
}

/// An exact translation of a face, a point or a curve. A query moves
/// the face and the point (curve) by their shifts; only the difference
/// matters.
#[derive(Clone, Debug, PartialEq)]
pub struct Shift(Option<[Dy; 3]>, bool);

impl Shift {
    /// No translation.
    pub const NONE: Shift = Shift(None, false);

    /// `D` of [`Shift::tie`]: the shift is `2^-D` times unit-sized
    /// terms (relative to the data: scaled down further for data finer
    /// or smaller than unit-sized f64 values, see the module docs). 512
    /// bits below the data puts it far below any separation
    /// of a configuration that isn't tied which low-degree polynomial
    /// conditions on f64 data produce in practice, while keeping the
    /// recount's numbers a few thousand bits long.
    pub const TIE_D: i64 = 512;

    /// `E` of [`Shift::tie`]: each term after the first is `2^-E` times
    /// smaller than the one before, standing in for the next power of
    /// the infinitesimal `η`.
    pub const TIE_E: i64 = 64;

    /// The displacement standing in for `δ·(σ·d + η·T₂ + η²·T₃ + η³·T₄)`,
    /// `η` infinitely small and `δ` infinitely smaller:
    /// `2^-D·(σ·d + 2^-E·T₂ + 2^-2E·T₃ + 2^-3E·T₄)` with
    /// `T₂ = (3, 7, 11)/16`, `T₃ = (−5, 2, 13)/16`, `T₄ = (7, −11, 2)/16`
    /// and `D`, `E` as [`Shift::TIE_D`], [`Shift::TIE_E`].
    pub fn tie(d: [f64; 3], sigma: i8) -> Shift {
        assert!(sigma == 1 || sigma == -1, "σ is ±1, not {sigma}");
        const T: [[i64; 3]; 3] = [[3, 7, 11], [-5, 2, 13], [7, -11, 2]];
        let mut v = dy3(d).map(|x| if sigma < 0 { x.neg() } else { x });
        for (k, t) in T.iter().enumerate() {
            for a in 0..3 {
                v[a] = v[a].add(&Dy::from_int(t[a]).shl(-4 - Self::TIE_E * (k as i64 + 1)));
            }
        }
        Shift(Some(v.map(|x| x.shl(-Self::TIE_D))), false)
    }

    /// The translation `v·2^exp2`, exactly (for explicit small moves).
    pub fn exact(v: [f64; 3], exp2: i64) -> Shift {
        Shift(Some(dy3(v).map(|x| x.shl(exp2))), false)
    }

    /// The translation `v·2^exp2` applied whether or not the unshifted
    /// configuration is degenerate: the moved configuration itself, for
    /// a check at a scale (a query with such a shift on either side is
    /// counted shifted only, and reports `tie`).
    pub fn always(v: [f64; 3], exp2: i64) -> Shift {
        Shift(Some(dy3(v).map(|x| x.shl(exp2))), true)
    }

    /// `self − other`, `None` when that is zero; a tie shift's
    /// difference times `2^k` (see [`Fine`]).
    fn minus_at(&self, other: &Shift, k: i64) -> Option<[Dy; 3]> {
        let d = self.minus(other)?;
        Some(if self.1 || other.1 {
            d
        } else {
            d.map(|x| x.shl(k))
        })
    }

    /// `self − other`, `None` when that is zero.
    fn minus(&self, other: &Shift) -> Option<[Dy; 3]> {
        let z = [Dy::ZERO, Dy::ZERO, Dy::ZERO];
        let (a, b) = (
            self.0.as_ref().unwrap_or(&z),
            other.0.as_ref().unwrap_or(&z),
        );
        let d = [a[0].sub(&b[0]), a[1].sub(&b[1]), a[2].sub(&b[2])];
        d.iter().any(|x| !x.is_zero()).then_some(d)
    }
}

/// A count of signed crossings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crossings {
    pub count: i64,
    /// Whether the unshifted configuration was degenerate (or too close
    /// to it to settle) for some piece of the face, so the count is the
    /// shifted configuration's (all of it).
    pub tie: bool,
}

/// A query the oracle couldn't settle: degenerate with the shifts too
/// (or without any), or past its work caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unresolved {
    pub reason: &'static str,
}

/// The limits of the unshifted count: settling a configuration that
/// isn't degenerate takes boxes far wider than `2^-200` of the domain,
/// so one needing narrower is taken as tied (and recounted shifted,
/// which changes nothing if it wasn't: the shift is far smaller).
const FIRST: Caps = Caps {
    depth: 100,
    width: 200,
    boxes: 20_000,
    trim_depth: 256,
};

/// The limits of the shifted count: the shift is `2^-512`, its smallest
/// term `2^-708`, so splitting a root from a domain end takes boxes
/// down to about `2^-1000` and beyond for roots that are not simple.
const SHIFTED: Caps = Caps {
    depth: 4000,
    width: 3000,
    boxes: 400_000,
    trim_depth: 4000,
};

/// How finely a query's data is resolved: the binary exponents of its
/// smallest and largest non-zero coordinates and weights. Data spanning
/// more than [`Fine::SPAN`] binades (a coordinate a few subnormals off
/// zero beside unit-sized ones) has configurations that aren't degenerate
/// whose separations lie far below `2^-200` of the domain and below the
/// tie shift's `2^-512`, and data far below unit size has every
/// separation below the shift: the caps are widened by the excess span
/// and a tie shift (not [`Shift::always`]) is scaled down by it and by
/// the data's own size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fine {
    lo: i64,
    hi: i64,
}

impl Fine {
    /// The span of binades the caps and the tie shift are made for.
    const SPAN: i64 = 64;

    fn of(xs: impl IntoIterator<Item = f64>) -> Option<Fine> {
        xs.into_iter()
            .filter(|&x| x != 0.0)
            .map(|x| Dy::from_f64(x).log2())
            .fold(None, |f, e| {
                Some(match f {
                    None => Fine { lo: e, hi: e },
                    Some(Fine { lo, hi }) => Fine {
                        lo: lo.min(e),
                        hi: hi.max(e),
                    },
                })
            })
    }

    fn with(a: Option<Fine>, b: Option<Fine>) -> Option<Fine> {
        match (a, b) {
            (Some(a), Some(b)) => Some(Fine {
                lo: a.lo.min(b.lo),
                hi: a.hi.max(b.hi),
            }),
            (a, b) => a.or(b),
        }
    }

    /// The span past [`Fine::SPAN`].
    fn excess(f: Option<Fine>) -> i64 {
        f.map_or(0, |f| (f.hi - f.lo - Self::SPAN).max(0))
    }

    /// The exponent a tie shift is scaled by: down by the excess span and
    /// by the data's size below 1.
    fn shift_exp(f: Option<Fine>) -> i64 {
        f.map_or(0, |f| f.hi.min(0)) - Self::excess(f)
    }

    /// `caps` widened by the excess span (`times` for the shifted count,
    /// whose shift sits that much further down).
    fn caps(f: Option<Fine>, caps: &Caps, times: i64) -> Caps {
        let x = Self::excess(f) * times;
        Caps {
            width: caps.width + x,
            trim_depth: caps.trim_depth + x as u32,
            ..*caps
        }
    }
}

/// A face preprocessed for exact counting: the homogeneous Bernstein
/// forms of its tiles and ribbons, and its trims.
pub struct ExactFace {
    pieces: Vec<Piece>,
    trims: Trims,
    fine: Option<Fine>,
}

/// A tile or a ribbon piece (one tile, one interval of `t`).
struct Piece {
    /// Homogeneous `x, y, z, w` up to one positive factor, degrees `n`
    /// in `(u, v)` or `(t, λ)`.
    hom: [Vec<Integer>; 4],
    n: [usize; 3],
    /// For a tile, its offset (the weight is the trims' winding).
    tile: Option<[i64; 2]>,
    /// For a ribbon piece, the conditions (in `t`) that `p(t)` lies
    /// strictly in its tile, where the piece crosses tile lines.
    preds: Vec<Vec<Integer>>,
    /// The orientation: `N = eps·(x_a × x_b)`.
    eps: i64,
    /// Bounding box `[min, max]` of the piece (where it counts).
    bbox: [[f64; 3]; 2],
}

#[cfg(test)]
impl Piece {
    fn is_ribbon(&self) -> bool {
        self.tile.is_none()
    }
}

fn ctrl_bbox(points: impl Iterator<Item = [f64; 3]>) -> [[f64; 3]; 2] {
    let mut b = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for p in points {
        for a in 0..3 {
            if p[a] < b[0][a] {
                b[0][a] = p[a];
            }
            if p[a] > b[1][a] {
                b[1][a] = p[a];
            }
        }
    }
    b
}

fn tile_box(net: &[[f64; 4]; 9]) -> [[f64; 3]; 2] {
    ctrl_bbox(net.iter().map(|p| [p[0], p[1], p[2]]))
}

fn union(a: [[f64; 3]; 2], b: [[f64; 3]; 2]) -> [[f64; 3]; 2] {
    let mut u = a;
    for k in 0..3 {
        if b[0][k] < u[0][k] {
            u[0][k] = b[0][k];
        }
        if b[1][k] > u[1][k] {
            u[1][k] = b[1][k];
        }
    }
    u
}

fn binom(n: usize, k: usize) -> Rational {
    let mut r = Integer::ONE;
    for i in 0..k {
        r = r * Integer::from((n - i) as u64) / Integer::from((i + 1) as u64);
    }
    Rational::from(r)
}

/// The product of two univariate Bernstein forms.
fn bmul(a: &[Rational], b: &[Rational]) -> Vec<Rational> {
    let (m, n) = (a.len() - 1, b.len() - 1);
    let mut c = vec![Rational::ZERO; m + n + 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            c[i + j] += binom(m, i) * binom(n, j) * x * y;
        }
    }
    for (k, x) in c.iter_mut().enumerate() {
        *x /= binom(m + n, k);
    }
    c
}

fn badd(a: &[Rational], b: &[Rational]) -> Vec<Rational> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

fn bscale(a: &[Rational], s: &Rational) -> Vec<Rational> {
    a.iter().map(|x| x * s).collect()
}

/// A quadratic Bernstein form restricted to `[a, b] ⊂ [0, 1]` (blossom).
fn restrict(c: &[Rational; 3], a: &Rational, b: &Rational) -> [Rational; 3] {
    let f = |x: &Rational, y: &Rational| {
        let (x1, y1) = (Rational::ONE - x, Rational::ONE - y);
        &c[0] * &x1 * &y1 + &c[1] * (&x1 * y + x * &y1) + &c[2] * x * y
    };
    [f(a, a), f(a, b), f(b, b)]
}

/// The span of `knots` holding `[k0, k1]`, and `[k0, k1]` in its local
/// parameter.
fn locate(knots: &[f64], k0: f64, k1: f64) -> (usize, Rational, Rational) {
    let s = (0..knots.len() - 1)
        .find(|&s| knots[s] <= k0 && k1 <= knots[s + 1])
        .expect("a coedge's range lies in its chains' knot ranges");
    let (a, b) = (q(knots[s]), q(knots[s + 1]));
    let len = &b - &a;
    ((s), (q(k0) - &a) / &len, (q(k1) - &a) / &len)
}

fn check_knots(knots: &[f64], spans: usize) {
    assert_eq!(
        knots.len(),
        spans + 1,
        "a chain has one knot more than spans"
    );
    assert!(
        knots.windows(2).all(|k| k[0] < k[1]),
        "a chain's knots increase"
    );
}

impl ExactFace {
    pub fn new(face: &Face) -> ExactFace {
        let mut pieces = Vec::new();
        // The tiles' homogeneous nets, exactly.
        let mut nets: Vec<([i64; 2], [[Rational; 4]; 9])> = Vec::new();
        for (off, net) in &face.tiles {
            let mut v = Vec::with_capacity(36);
            for c in 0..4 {
                for p in net {
                    assert!(p[3] > 0.0, "control point weights are positive");
                    let w = Dy::from_f64(p[3]);
                    v.push(if c == 3 {
                        w
                    } else {
                        Dy::from_f64(p[c]).mul(&w)
                    });
                }
            }
            let ints = common(&v);
            let hom: [Vec<Integer>; 4] = std::array::from_fn(|c| ints[9 * c..9 * c + 9].to_vec());
            pieces.push(Piece {
                hom,
                n: [2, 2, 0],
                tile: Some(*off),
                preds: Vec::new(),
                eps: 1,
                bbox: tile_box(net),
            });
            let h: [[Rational; 4]; 9] = std::array::from_fn(|i| {
                let w = q(net[i][3]);
                [q(net[i][0]) * &w, q(net[i][1]) * &w, q(net[i][2]) * &w, w]
            });
            nets.push((*off, h));
        }
        let mut spans = Vec::new();
        for lp in &face.loops {
            for ce in lp {
                Self::coedge(ce, face, &nets, &mut spans, &mut pieces);
            }
        }
        let fine = Fine::of(
            face.tiles
                .iter()
                .flat_map(|(_, net)| net.iter().flatten().copied())
                .chain(face.loops.iter().flatten().flat_map(|ce| {
                    let p = ce.pcurve.spans.iter().flatten().flatten();
                    let c = ce.curve.spans.iter().flatten().flatten();
                    p.chain(c).copied()
                })),
        );
        ExactFace {
            pieces,
            trims: Trims { spans },
            fine,
        }
    }

    /// The trim pieces and ribbon pieces of a coedge.
    fn coedge(
        ce: &Coedge,
        face: &Face,
        nets: &[([i64; 2], [[Rational; 4]; 9])],
        spans: &mut Vec<PSpan>,
        pieces: &mut Vec<Piece>,
    ) {
        let (pc, cc) = (&ce.pcurve, &ce.curve);
        check_knots(&pc.knots, pc.spans.len());
        check_knots(&cc.knots, cc.spans.len());
        let [ta, tb] = ce.t_range;
        assert!(ta < tb, "a coedge's range increases");
        for k in [ta, tb] {
            assert!(
                pc.knots.contains(&k) && cc.knots.contains(&k),
                "a coedge's range ends are knots of both chains"
            );
        }
        let mut breaks: Vec<f64> = pc
            .knots
            .iter()
            .chain(&cc.knots)
            .copied()
            .filter(|&k| ta <= k && k <= tb)
            .collect();
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let dir = if ce.reversed { -1 } else { 1 };
        for w in breaks.windows(2) {
            let (k0, k1) = (w[0], w[1]);
            let (ps, pa, pb) = locate(&pc.knots, k0, k1);
            let (cs, ca, cb) = locate(&cc.knots, k0, k1);
            let p = &pc.spans[ps];
            let uvw: [[Rational; 3]; 3] = std::array::from_fn(|c| {
                let full: [Rational; 3] = std::array::from_fn(|i| {
                    let w = q(p[i][2]);
                    assert!(p[i][2] > 0.0, "control point weights are positive");
                    if c == 2 { w } else { q(p[i][c]) * w }
                });
                restrict(&full, &pa, &pb)
            });
            let c = &cc.spans[cs];
            let xyzw: [[Rational; 3]; 4] = std::array::from_fn(|k| {
                let full: [Rational; 3] = std::array::from_fn(|i| {
                    let w = q(c[i][3]);
                    assert!(c[i][3] > 0.0, "control point weights are positive");
                    if k == 3 { w } else { q(c[i][k]) * w }
                });
                restrict(&full, &ca, &cb)
            });
            // The trim piece.
            let all: Vec<Rational> = uvw.iter().flatten().cloned().collect();
            let ints = integerize(&all);
            let tri =
                |c: usize| -> [Integer; 3] { std::array::from_fn(|i| ints[3 * c + i].clone()) };
            let (us, vs): (Vec<f64>, Vec<f64>) = (0..3)
                .map(|i| {
                    (
                        rational_approx(&(&uvw[0][i] / &uvw[2][i])),
                        rational_approx(&(&uvw[1][i] / &uvw[2][i])),
                    )
                })
                .unzip();
            let fold = |v: &[f64], lo: bool| {
                v.iter().copied().fold(
                    if lo { f64::INFINITY } else { f64::NEG_INFINITY },
                    |m, x| {
                        if (lo && x < m) || (!lo && x > m) {
                            x
                        } else {
                            m
                        }
                    },
                )
            };
            spans.push(PSpan {
                u: tri(0),
                v: tri(1),
                w: tri(2),
                sign: dir,
                bbox: [
                    fold(&us, true),
                    fold(&us, false),
                    fold(&vs, true),
                    fold(&vs, false),
                ],
            });
            // The ribbon pieces: the tile holding the p-curve piece, or
            // each tile it may cross into with the conditions that it is
            // inside.
            let mut cands = Vec::new();
            for (ti, (off, _)) in nets.iter().enumerate() {
                let member: [Vec<Rational>; 4] = [
                    badd(&uvw[0], &bscale(&uvw[2], &-Rational::from(off[0]))),
                    badd(
                        &bscale(&uvw[2], &Rational::from(off[0] + 1)),
                        &bscale(&uvw[0], &-Rational::ONE),
                    ),
                    badd(&uvw[1], &bscale(&uvw[2], &-Rational::from(off[1]))),
                    badd(
                        &bscale(&uvw[2], &Rational::from(off[1] + 1)),
                        &bscale(&uvw[1], &-Rational::ONE),
                    ),
                ];
                if member.iter().any(|m| m.iter().all(|x| *x < 0)) {
                    continue;
                }
                // A p-curve piece on the line between this tile and the
                // next one up in `u` (or `v`) is in both, closed: it
                // belongs to the next one, or its ribbon counts twice.
                let next = |a: usize| {
                    let mut o = *off;
                    o[a] += 1;
                    nets.iter().any(|(x, _)| *x == o)
                };
                if (member[1].iter().all(|x| *x == 0) && next(0))
                    || (member[3].iter().all(|x| *x == 0) && next(1))
                {
                    continue;
                }
                let open: Vec<Vec<Integer>> = member
                    .iter()
                    .filter(|m| m.iter().any(|x| *x < 0))
                    .map(|m| integerize(m))
                    .collect();
                if open.is_empty() {
                    cands = vec![(ti, open)];
                    break;
                }
                cands.push((ti, open));
            }
            assert!(!cands.is_empty(), "a p-curve leaves the face's tiles");
            let cbox = ctrl_bbox(c.iter().map(|p| [p[0], p[1], p[2]]));
            for (ti, preds) in cands {
                let (off, h) = &nets[ti];
                if let Some(piece) = ribbon(
                    &uvw,
                    &xyzw,
                    *off,
                    h,
                    preds,
                    -dir,
                    union(tile_box(&face.tiles[ti].1), cbox),
                ) {
                    pieces.push(piece);
                }
            }
        }
    }

    /// Signed crossings of the ray `q + s·dir`, `s > 0`, with the
    /// counting surface: `Σ sign(N·dir)·weight`. The face is translated
    /// by `face_shift` and `q` by `q_shift`.
    pub fn ray(
        &self,
        face_shift: &Shift,
        q: [f64; 3],
        q_shift: &Shift,
        dir: [f64; 3],
    ) -> Result<Crossings, Unresolved> {
        self.ray_parts(face_shift, q, q_shift, dir, |_| true)
    }

    fn ray_parts(
        &self,
        face_shift: &Shift,
        q: [f64; 3],
        q_shift: &Shift,
        dir: [f64; 3],
        keep: impl Fn(&Piece) -> bool,
    ) -> Result<Crossings, Unresolved> {
        assert!(dir.iter().any(|&x| x != 0.0), "a ray has a direction");
        let fine = Fine::with(self.fine, Fine::of(q.into_iter().chain(dir)));
        let delta = q_shift.minus_at(face_shift, Fine::shift_exp(fine));
        let d = dy3(dir);
        // ν₁ = dir × e_k (k the smallest component), ν₂ = dir × ν₁: then
        // ν₁ × ν₂ = |ν₁|²·dir.
        let k = (0..3).fold(0, |b, a| if dir[a].abs() < dir[b].abs() { a } else { b });
        let mut e = [Dy::ZERO, Dy::ZERO, Dy::ZERO];
        e[k] = Dy::from_int(1);
        let nu1 = cross(&d, &e);
        let nu2 = cross(&d, &nu1);
        let q0 = dy3(q);
        // The unshifted configuration; if any piece of it is degenerate,
        // the shifted configuration, all of it: counts of the pieces
        // that were settled unshifted stand only for shifts below their
        // separations, which an explicit shift needn't be.
        let pass = |q1: &[Dy; 3], grow: f64, caps: &Caps| -> Result<i64, Degenerate> {
            let mut n = 0;
            for piece in self.pieces.iter().filter(|p| keep(p)) {
                if ray_meets_box(q, dir, &piece.bbox, grow) {
                    n += self.ray_piece(piece, q1, &d, &nu1, &nu2, caps)?;
                }
            }
            Ok(n)
        };
        if !(face_shift.1 || q_shift.1)
            && let Ok(count) = pass(&q0, 0.0, &Fine::caps(fine, &FIRST, 1))
        {
            return Ok(Crossings { count, tie: false });
        }
        let delta = delta.as_ref().ok_or(Unresolved {
            reason: "degenerate ray, no shift given",
        })?;
        let q1 = [
            q0[0].add(&delta[0]),
            q0[1].add(&delta[1]),
            q0[2].add(&delta[2]),
        ];
        let count = pass(&q1, reach(delta), &Fine::caps(fine, &SHIFTED, 2)).map_err(unresolved)?;
        Ok(Crossings { count, tie: true })
    }

    fn ray_piece(
        &self,
        piece: &Piece,
        q: &[Dy; 3],
        dir: &[Dy; 3],
        nu1: &[Dy; 3],
        nu2: &[Dy; 3],
        caps: &Caps,
    ) -> Result<i64, Degenerate> {
        // G_i = ν_i·(P − q·W): zero on the line; H = dir·(P − q·W) > 0
        // ahead. At a root, det G′ = W²·(ν₁ × ν₂)·(x_a × x_b), so its
        // sign is sign(dir·(x_a × x_b)).
        let lin = |v: &[Dy; 3]| -> Bern {
            let vq = dot(v, q);
            let c: Vec<Dy> = (0..piece.hom[0].len())
                .map(|i| {
                    let p = [0, 1, 2].map(|a| Dy {
                        m: piece.hom[a][i].clone(),
                        e: 0,
                    });
                    dot(v, &p).sub(&vq.mul(&Dy {
                        m: piece.hom[3][i].clone(),
                        e: 0,
                    }))
                })
                .collect();
            Bern::new(piece.n, common(&c))
        };
        let f = vec![lin(nu1), lin(nu2)];
        let mut preds = vec![lin(dir)];
        preds.extend(piece.preds.iter().map(|p| Bern::along(0, p.clone())));
        let weight = piece.tile.map(|off| Weight {
            trims: &self.trims,
            axes: [0, 1],
            off,
        });
        Ok(piece.eps * count(2, f, preds, weight.as_ref(), caps)?)
    }

    /// Signed crossings of the chain `c` (all of it, ends included) with
    /// the counting surface: `Σ −sign(C′·N)·weight`, entering +1. The
    /// face is translated by `face_shift` and the chain by `c_shift`.
    pub fn curve(
        &self,
        face_shift: &Shift,
        c: &Chain,
        c_shift: &Shift,
    ) -> Result<Crossings, Unresolved> {
        self.curve_parts(face_shift, c, c_shift, |_| true)
    }

    fn curve_parts(
        &self,
        face_shift: &Shift,
        c: &Chain,
        c_shift: &Shift,
        keep: impl Fn(&Piece) -> bool,
    ) -> Result<Crossings, Unresolved> {
        check_knots(&c.knots, c.spans.len());
        let fine = Fine::with(
            self.fine,
            Fine::of(c.spans.iter().flatten().flatten().copied()),
        );
        let delta = c_shift.minus_at(face_shift, Fine::shift_exp(fine));
        // As for rays: on any degeneracy, the whole shifted configuration.
        let pass = |shift: Option<&[Dy; 3]>, caps: &Caps| -> Result<i64, Degenerate> {
            let grow = shift.map_or(0.0, reach);
            let mut n = 0;
            for span in &c.spans {
                let sbox = ctrl_bbox(span.iter().map(|p| [p[0], p[1], p[2]]));
                let w: Vec<Dy> = span
                    .iter()
                    .map(|p| {
                        assert!(p[3] > 0.0, "control point weights are positive");
                        Dy::from_f64(p[3])
                    })
                    .collect();
                let x: [Vec<Dy>; 3] = std::array::from_fn(|a| {
                    (0..3)
                        .map(|i| {
                            let v = Dy::from_f64(span[i][a]);
                            let v = match shift {
                                Some(s) => v.add(&s[a]),
                                None => v,
                            };
                            v.mul(&w[i])
                        })
                        .collect()
                });
                let hom = (x, w);
                for piece in self.pieces.iter().filter(|p| keep(p)) {
                    if boxes_meet(&sbox, &piece.bbox, grow) {
                        n += self.curve_piece(piece, &hom, caps)?;
                    }
                }
            }
            Ok(n)
        };
        if !(face_shift.1 || c_shift.1)
            && let Ok(count) = pass(None, &Fine::caps(fine, &FIRST, 1))
        {
            return Ok(Crossings { count, tie: false });
        }
        let delta = delta.as_ref().ok_or(Unresolved {
            reason: "degenerate curve, no shift given",
        })?;
        let count = pass(Some(delta), &Fine::caps(fine, &SHIFTED, 2)).map_err(unresolved)?;
        Ok(Crossings { count, tie: true })
    }

    fn curve_piece(
        &self,
        piece: &Piece,
        span: &([Vec<Dy>; 3], Vec<Dy>),
        caps: &Caps,
    ) -> Result<i64, Degenerate> {
        // F = P(a, b)·W_c(τ) − Q(τ)·W(a, b), over (τ, a, b): at a root
        // det F′ = −(W·W_c)³·C′·(x_a × x_b).
        let (qx, wq) = span;
        let n = [2, piece.n[0], piece.n[1]];
        let m = piece.hom[0].len();
        let f: Vec<Bern> = (0..3)
            .map(|a| {
                let mut c = Vec::with_capacity(3 * m);
                for tau in 0..3 {
                    for i in 0..m {
                        let p = Dy {
                            m: piece.hom[a][i].clone(),
                            e: 0,
                        };
                        let w = Dy {
                            m: piece.hom[3][i].clone(),
                            e: 0,
                        };
                        c.push(p.mul(&wq[tau]).sub(&qx[a][tau].mul(&w)));
                    }
                }
                Bern::new(n, common(&c))
            })
            .collect();
        let preds = piece
            .preds
            .iter()
            .map(|p| Bern::along(1, p.clone()))
            .collect();
        let weight = piece.tile.map(|off| Weight {
            trims: &self.trims,
            axes: [1, 2],
            off,
        });
        // −sign(C′·N) = eps·sign det F′.
        Ok(piece.eps * count(3, f, preds, weight.as_ref(), caps)?)
    }
}

fn unresolved(e: Degenerate) -> Unresolved {
    Unresolved {
        reason: match e {
            Degenerate::Depth => "degenerate after the shift (subdivision depth cap)",
            Degenerate::Boxes => "degenerate after the shift or too much work (box cap)",
            Degenerate::Winding => "a crossing on a trim after the shift",
            Degenerate::Zero => "the query lies in the surface after the shift",
        },
    }
}

/// The ribbon piece over one tile, or `None` when it has no area (its
/// p-curve image is the edge curve exactly).
fn ribbon(
    uvw: &[[Rational; 3]; 3],
    xyzw: &[[Rational; 3]; 4],
    off: [i64; 2],
    h: &[[Rational; 4]; 9],
    preds: Vec<Vec<Integer>>,
    eps: i64,
    fallback: [[f64; 3]; 2],
) -> Option<Piece> {
    // B_a(u(t))·W_p(t)² with u the tile-local u.
    let basis = |x: &[Rational; 3], o: i64| -> [Vec<Rational>; 3] {
        let a = badd(
            &bscale(&uvw[2], &Rational::from(o + 1)),
            &bscale(x, &-Rational::ONE),
        );
        let b = badd(x, &bscale(&uvw[2], &-Rational::from(o)));
        [
            bmul(&a, &a),
            bscale(&bmul(&a, &b), &Rational::from(2)),
            bmul(&b, &b),
        ]
    };
    let bu = basis(&uvw[0], off[0]);
    let bv = basis(&uvw[1], off[1]);
    let mut ps: [Vec<Rational>; 4] = std::array::from_fn(|_| vec![Rational::ZERO; 9]);
    for a in 0..3 {
        for b in 0..3 {
            let prod = bmul(&bu[a], &bv[b]);
            for (c, out) in ps.iter_mut().enumerate() {
                *out = badd(out, &bscale(&prod, &h[3 * a + b][c]));
            }
        }
    }
    let wc = &xyzw[3];
    let lam0: Vec<Vec<Rational>> = (0..3).map(|c| bmul(&ps[c], wc)).collect();
    let lam1: Vec<Vec<Rational>> = (0..3).map(|c| bmul(&xyzw[c], &ps[3])).collect();
    if lam0 == lam1 {
        return None;
    }
    let w = bmul(&ps[3], wc);
    let mut all = Vec::with_capacity(88);
    for c in 0..4 {
        for t in 0..11 {
            for l in 0..2 {
                all.push(if c == 3 {
                    w[t].clone()
                } else if l == 0 {
                    lam0[c][t].clone()
                } else {
                    lam1[c][t].clone()
                });
            }
        }
    }
    let ints = integerize(&all);
    let hom: [Vec<Integer>; 4] = std::array::from_fn(|c| ints[22 * c..22 * c + 22].to_vec());
    // The convex hull of the affine control points when the weights are
    // positive; else the tile's and the edge span's hulls (the ribbon
    // point is between a patch point of the tile and an edge point).
    let bbox = if w.iter().all(|x| *x > 0) {
        ctrl_bbox((0..22).map(|i| {
            let (t, l) = (i / 2, i % 2);
            std::array::from_fn(|c| {
                let x = if l == 0 { &lam0[c][t] } else { &lam1[c][t] };
                rational_approx(&(x / &w[t]))
            })
        }))
    } else {
        fallback
    };
    Some(Piece {
        hom,
        n: [10, 1, 0],
        tile: None,
        preds,
        eps,
        bbox,
    })
}

/// The slack around boxes for the f64 rejection tests: far above their
/// rounding errors and the shifts.
fn slack(scale: f64) -> f64 {
    scale * 1e-9 + 1e-100
}

/// Twice the largest component of a shift: how far it may move a
/// query relative to the face, with room for rounding.
fn reach(delta: &[Dy; 3]) -> f64 {
    delta.iter().fold(0.0, |m, x| {
        let a = x.approx().abs() * 2.0;
        if a > m { a } else { m }
    })
}

/// Whether two boxes, the first moved by up to `grow` along each axis,
/// may meet.
fn boxes_meet(a: &[[f64; 3]; 2], b: &[[f64; 3]; 2], grow: f64) -> bool {
    (0..3).all(|k| {
        let m = slack(a[0][k].abs() + a[1][k].abs() + b[0][k].abs() + b[1][k].abs()) + grow;
        a[0][k] <= b[1][k] + m && b[0][k] <= a[1][k] + m
    })
}

/// Whether the ray, its start moved by up to `grow` along each axis, may
/// meet the box (slab test on a box grown by that and the slack, which
/// covers the slab arithmetic's rounding).
fn ray_meets_box(q: [f64; 3], dir: [f64; 3], b: &[[f64; 3]; 2], grow: f64) -> bool {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for k in 0..3 {
        let m = slack(b[0][k].abs() + b[1][k].abs() + q[k].abs()) + grow;
        let (l, h) = (b[0][k] - m, b[1][k] + m);
        if dir[k] == 0.0 {
            if q[k] < l || q[k] > h {
                return false;
            }
            continue;
        }
        let (s1, s2) = ((l - q[k]) / dir[k], (h - q[k]) / dir[k]);
        let (a, z) = if s1 < s2 { (s1, s2) } else { (s2, s1) };
        if a > lo {
            lo = a;
        }
        if z < hi {
            hi = z;
        }
    }
    lo <= hi && hi >= 0.0
}
