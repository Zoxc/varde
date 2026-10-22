//! The closest point of a rational span or patch to a point `x`
//! ([`Closest`]): a certified upper bound on the distance at whatever
//! parameter Newton's method reaches, and a certificate for the foot
//! point itself where one is needed (closest-point projection), or the
//! answer that this piece must be subdivided first. Also certified
//! curvature bounds ([`Closest::curvature_bound`]) from hodograph
//! Bernstein bounds, for the callers' own uniqueness conditions.
//!
//! **The frame.** The piece is held in the frame centred at `x`: each
//! stored control point's offset `p − x` is formed first, then weighted,
//! `D = W·(S − x)` and `W`, so the coefficients' rounding scales with the
//! piece's distance from `x`, not with the coordinates' magnitude.
//!
//! **The gradient system.** With `D` and `W` as above, `½‖S − x‖²` has
//! the partial derivatives `gₐ / W³`, `gₐ = D·(W·D_a − W_a·D)` (the dot
//! of `D` with the tangent or partial numerator of the offset form,
//! [`HomCurve::tangent_numerator`], [`HomPatch::partial_numerator`]), so
//! with `W > 0` the stationary points of the distance are the common
//! roots of the polynomials `gₐ` ([`Closest::gradient`]): one form in
//! `t` for a span (degree `3n − 1`, `2n − 1` for a polynomial span), two
//! in `(u, v)` for a patch. At a root the Jacobian of `g` is `W³` times
//! the Hessian of `½‖S − x‖²`, so where it is certified positive definite
//! the root is a strict local minimum of the distance. Isolating the
//! roots of `g` over the whole piece ([`isolate`](super::isolate),
//! [`isolate_nd`](super::isolate_nd)) is the subdivision route to every
//! foot; [`Closest::closest`] is the local one.
//!
//! **The distance upper bound.** `|x − S(z)|` at any parameter `z` of the
//! piece bounds the distance from `x` to the piece from above, whether or
//! not `z` is near a foot: no Kantorovich condition is needed for it
//! (whether `z` is inside a face's trim is the caller's to certify).
//! [`Closest::upper_bound`] certifies it from the enclosures of `D(z)`
//! and `W(z)`; where those aren't certified, the farthest point of the
//! piece's control box bounds it instead.
//!
//! **Newton and the certificate.** [`Closest::closest`] runs Newton's
//! method on `g` from a seed (the caller's, else the parameter of the
//! control point nearest `x`) on the lattice `2⁻⁵³` of the piece's
//! domain, each step rounded to the lattice and clamped to the domain (a
//! variable at the boundary whose step leads out is held there and the
//! others solved without it, so Newton runs along the boundary), until a
//! step rounds to zero or [`NEWTON_STEPS`] are taken. Newton descends:
//! its step is taken only where the Jacobian (estimated at the point, on
//! the variables not held) is positive definite and the step goes down
//! the gradient; elsewhere (near a saddle or a maximum of the distance,
//! which plain Newton converges to as readily as to a minimum) the step
//! goes down the gradient instead, to the minimum of the quadratic model
//! along it where that curves up, at most half the domain, halved until
//! the distance (estimated) decreases, and Newton stops where none does.
//! From a seed beside a saddle or a maximum it leaves it; from one
//! exactly at it (the gradient 0) it stays, and the certificate then
//! says the foot isn't a minimum ([`FootBox::minimum`]). Around the
//! point reached, Krawczyk's operator on `g` (the n-D root isolation's,
//! [`isolate_nd`](super::isolate_nd), its Jacobian the hull of the
//! derivative forms on the box: the hodograph Bernstein bounds) either
//! falls inside the box, which certifies exactly one stationary point in
//! it ([`Foot::Certified`]), or it doesn't:
//!
//! - [`Foot::Boundary`]: Newton ended held at the domain's boundary (the
//!   point reached is then the boundary's nearest, as far as Newton
//!   found), or the box couldn't stay inside the domain: the foot, if
//!   any, is on or past this piece's boundary (a neighbour's or the
//!   boundary curve's to find);
//! - [`Foot::Precision`]: the operator contracts (the Jacobian is well
//!   conditioned on the box) and Newton settled, but the value of `g` at
//!   the point is too wide for the box at this level: the next level;
//! - [`Foot::Subdivide`]: anything else, a Jacobian not certified regular
//!   (a foot near a curvature centre, where Newton's error doesn't shrink
//!   with precision; a tangency; a wide box) or Newton not settling: split
//!   the piece ([`Closest::split_half`]) before raising the precision.
//!
//! **Same bits under reversal.** A lattice point mirrors exactly
//! (`1 − l·2⁻⁵³` is a lattice point), an evaluation there gives the
//! reversed form's at the mirrored point to the bit, and the reversed
//! piece's gradient is the mirrored one with its component in that
//! variable negated, its Newton step the mirrored step, rounded to the
//! lattice by ties to even: so a piece reversed in a variable (and the
//! seed mirrored) reaches the mirrored point, with the same bounds, and
//! gets the mirrored certificate. The descent steps mirror too (the
//! distance estimates are evaluations, a halved step truncates towards
//! zero). A tie between seeds takes the first in the net's order, which
//! a reversed net gives the other.
//!
//! **Work.** Newton steps, Krawczyk steps, halvings and box tests are
//! counted ([`Work`]); the caller charges them.

use smallvec::smallvec;

use super::curve::HomCurve;
use super::nd::NdTensor;
use super::poly::Hull;
use super::product::{cross, dot};
use super::roots::{MAX_ROOT_DEPTH, Work, mul, pow2_neg, sub, up_signed};
use super::roots_nd::{Axes, Cell, LATTICE, Lat, Narrowed, NdBox, Restrict, SIDE, cover, narrow};
use super::scalar::Scalar;
use super::tensor::{Dir, HomPatch, Tensor, quotient};
use crate::num::bounded::{add_up, down, mul_up, up};

/// The most Newton steps [`Closest::closest`] takes.
pub const NEWTON_STEPS: u32 = 32;

/// The least half width, in lattice units, of the box the certificate
/// starts from around the point reached.
const START_RADIUS: u64 = 4;

/// The piece in the frame centred at the query point.
#[derive(Clone, Debug, PartialEq)]
enum Geom<S> {
    Curve(HomCurve<S, 3>),
    Patch(HomPatch<S>),
}

/// A span or patch and a point `x`, set up for the closest point (module
/// docs): the piece in the frame centred at `x`, the gradient system of
/// the squared distance, and the dyadic cell of the original domain the
/// piece covers (all of it, or a part [`split_half`](Self::split_half)
/// gave). Parameters given and returned are the cell's own, in `[0, 1]`,
/// unless said otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Closest<S> {
    geom: Geom<S>,
    x: [f64; 3],
    grad: Vec<NdTensor<S>>,
    /// `∂gᵢ/∂xⱼ`, row-major.
    jac: Vec<NdTensor<S>>,
    cell: Cell,
}

/// Where [`Closest::closest`] got to.
#[derive(Clone, Debug, PartialEq)]
pub struct Reached {
    /// The parameter Newton stopped at, in the original domain: `[a, b]`
    /// per variable, `f64`s with it between them (both it, exactly, on
    /// the whole domain).
    pub at: Axes<[f64; 2]>,
    /// The same in the cell's own parameters, exactly (a lattice point).
    pub local: Axes<f64>,
    /// A certified upper bound of `|x − S(at)|`, so of the distance from
    /// `x` to the piece ([`Closest::upper_bound`]): finite for every
    /// piece with positive weights whose offsets from `x` square in range.
    pub upper: f64,
    pub foot: Foot,
    pub work: Work,
}

/// The certificate for the foot point near the point reached.
#[derive(Clone, Debug, PartialEq)]
pub enum Foot {
    /// Exactly one stationary point near the point reached.
    Certified(Box<FootBox>),
    /// Split the piece first (module docs).
    Subdivide,
    /// The foot is on or past the piece's boundary.
    Boundary,
    /// The next precision level.
    Precision,
}

/// A certified foot: exactly one stationary point of the distance from
/// `x` in `unique`, and it lies in `near`.
#[derive(Clone, Debug, PartialEq)]
pub struct FootBox {
    /// `[a, b]` per variable in the original domain, `f64`s with the foot
    /// between them (rounded outwards).
    pub near: Axes<[f64; 2]>,
    /// A box of the original domain in which the foot is the only
    /// stationary point (rounded inwards; it holds `near`'s lattice box).
    pub unique: Axes<[f64; 2]>,
    /// Whether the foot is certified a strict local minimum of the
    /// distance (the Jacobian of the gradient system certified positive
    /// definite on `unique`'s lattice box); `false` leaves it open.
    pub minimum: bool,
    /// `[lo, hi]` per coordinate holding the foot point `S(foot)`.
    pub point: [[f64; 2]; 3],
    /// An upper bound of `|S(foot) − S(at)|`, the reached point's
    /// distance from the foot point.
    pub spread: f64,
}

impl<S: Scalar> Closest<S> {
    /// The span with the affine control points `(x, y, z, w)` (as stored,
    /// `b₀` first) and the query point `x`, at the level of `like`. `None`
    /// for a value that isn't finite, a weight that isn't positive, or a
    /// gradient past [`MAX_DEGREE`](super::MAX_DEGREE) (degree above 21,
    /// 32 for a polynomial span).
    pub fn curve(pts: &[[f64; 4]], x: [f64; 3], like: &S) -> Option<Self> {
        if !finite(pts, x) {
            return None;
        }
        let c = HomCurve::new(pts.iter().map(|p| offset(p, x, like)))?;
        Self::with(Geom::Curve(c), x, Cell::root(1))
    }

    /// The patch of degrees `(m, n)` with the affine control points
    /// `(x, y, z, w)` row-major in `u` and the query point `x`, at the
    /// level of `like`. `None` as for [`curve`](Self::curve) (a degree
    /// above 21 in either direction for a rational patch).
    pub fn patch(m: usize, n: usize, pts: &[[f64; 4]], x: [f64; 3], like: &S) -> Option<Self> {
        if !finite(pts, x) {
            return None;
        }
        let p = HomPatch::new(
            m,
            n,
            pts.iter().map(|p| {
                let (d, w) = offset(p, x, like);
                [d[0].clone(), d[1].clone(), d[2].clone(), w]
            }),
        )?;
        Self::with(Geom::Patch(p), x, Cell::root(2))
    }

    /// The patch already held in the frame centred at `x`, its net
    /// `(W·(S − x), W)` (as [`patch`](Self::patch) forms it from affine
    /// points; the caller's weights certified positive), set up as
    /// [`patch`](Self::patch) does. `None` for a gradient past the degree
    /// limit.
    pub fn from_offset(p: HomPatch<S>, x: [f64; 3]) -> Option<Self> {
        Self::with(Geom::Patch(p), x, Cell::root(2))
    }

    fn with(geom: Geom<S>, x: [f64; 3], cell: Cell) -> Option<Self> {
        let grad = gradient(&geom)?;
        let d = grad.len();
        let jac = (0..d * d).map(|k| grad[k / d].derivative(k % d)).collect();
        Some(Closest {
            geom,
            x,
            grad,
            jac,
            cell,
        })
    }

    /// The number of parameters: 1 for a span, 2 for a patch.
    pub fn dims(&self) -> usize {
        self.grad.len()
    }

    /// The query point.
    pub fn point(&self) -> [f64; 3] {
        self.x
    }

    /// The cell of the original domain this piece covers.
    pub fn cell(&self) -> NdBox {
        self.cell.to_box()
    }

    /// The gradient system `gₐ` on the cell (module docs): its common
    /// roots are the stationary points of the distance from `x`.
    pub fn gradient(&self) -> &[NdTensor<S>] {
        &self.grad
    }

    /// The halves of the piece at ½ in variable `axis` (0 for `u` or `t`,
    /// 1 for `v`), by the forms' split at ½ (the same bits under
    /// reversal), each set up afresh; `None` for another axis, past the
    /// depth [`MAX_ROOT_DEPTH`], or past the degrees.
    pub fn split_half(&self, axis: usize) -> Option<[Self; 2]> {
        if axis >= self.dims() || self.cell.k[axis] >= MAX_ROOT_DEPTH {
            return None;
        }
        let [l, r] = halves(&self.geom, axis);
        Some([
            Self::with(l, self.x, self.cell.child(axis, 0))?,
            Self::with(r, self.x, self.cell.child(axis, 1))?,
        ])
    }

    /// A certified upper bound of `|x − S(z)|` at the parameter `z` of the
    /// cell (one per variable, each in `[0, 1]`), so of the distance from
    /// `x` to the piece: from the enclosures of `D(z)` and `W(z)`, else
    /// (`W(z)` not certified positive, a coordinate not certified) the
    /// farthest point of the piece's control box; `+∞` if neither is
    /// certified or `z` isn't in the cell.
    pub fn upper_bound(&self, z: &[f64]) -> f64 {
        if z.len() != self.dims() || !z.iter().all(|t| (0.0..=1.0).contains(t)) {
            return f64::INFINITY;
        }
        if let Some(e) = self.offset_at(z) {
            return farthest(&e);
        }
        self.control_box().map_or(f64::INFINITY, |b| farthest(&b))
    }

    /// Newton's method on the gradient system from `seed` (the cell's
    /// parameters, one per variable, rounded to the lattice and clamped to
    /// `[0, 1]`; `None` for the parameter of the control point nearest
    /// `x`), then the certificate around the point reached (module docs).
    /// `None` for a seed of the wrong length or not finite.
    pub fn closest(&self, seed: Option<&[f64]>) -> Option<Reached> {
        let d = self.dims();
        let mut work = Work::default();
        let start: Axes<u64> = match seed {
            Some(s) if s.len() == d && s.iter().all(|t| t.is_finite()) => {
                s.iter().map(|&t| to_lattice(t)).collect()
            }
            Some(_) => return None,
            None => self.seed(),
        };
        let newton = self.newton(start, &mut work);
        let c = &newton.at;
        let local: Axes<f64> = c.iter().map(|&l| lattice_f64(l)).collect();
        let point: Lat = c.iter().map(|&l| [l, l]).collect();
        let at = self.cell.bounds(&point, true);
        let upper = self.upper_bound(&local);
        let foot = if newton.pushed_out {
            Foot::Boundary
        } else {
            self.certify(&newton, &mut work)
        };
        Some(Reached {
            at,
            local,
            upper,
            foot,
            work,
        })
    }

    /// A certified upper bound of the curvature of the piece on its cell:
    /// of a span's curvature, or of a patch's normal curvature in every
    /// direction (so of both principal curvatures' magnitudes). `None`
    /// where the hodograph bounds don't certify one (a speed or a normal
    /// not certified non-zero on the cell, the forms past
    /// [`MAX_DEGREE`](super::MAX_DEGREE)); subdividing tightens it.
    ///
    /// A span `x = P/W` with the tangent numerator `T = W·P' − W'·P` has
    /// the curvature `W²·|T × T'| / |T|³`, bounded by the hulls of `W`,
    /// of the components of `T × T'` and of `T·T`. A patch with the
    /// partial numerators `N_u`, `N_v` (`W²·x_u`, `W²·x_v`) and the normal
    /// `Q = N_u × N_v` has the second fundamental form `(L, M, N) / (W²·|Q|)`,
    /// `L = A·Q`, `M = B·Q`, `N = C·Q` for `A = ∂N_u/∂u`, `B = ∂N_u/∂v`,
    /// `C = ∂N_v/∂v` (the tangent parts of the second derivatives drop out
    /// against `Q`), and the first `(E, F, G) / W⁴`, `E = N_u·N_u`, `F =
    /// N_u·N_v`, `G = N_v·N_v`: its normal curvatures are `W²·λ / |Q|`
    /// over the generalized eigenvalues `λ` of the two, bounded by the
    /// hulls of polynomial forms (`patch_curvature`).
    pub fn curvature_bound(&self) -> Option<f64> {
        match &self.geom {
            Geom::Curve(c) => {
                let t = c.tangent_numerator()?;
                let dt = t.clone().map(|p| p.derivative());
                let tt = dot(&t, &t)?.hull();
                let n = norm_up(&cross(&t, &dt)?.map(|p| p.hull()));
                over_cubed(c.weights().hull(), n, tt.lo)
            }
            Geom::Patch(p) => patch_curvature(p),
        }
    }

    /// The seed: the lattice point nearest the parameter `i/n` of the
    /// control point nearest `x` (by the control points' approximate
    /// offsets; the first of a tie in the net's order).
    fn seed(&self) -> Axes<u64> {
        let near = approx_dist2::<S>;
        let mut best = (f64::INFINITY, smallvec![0; self.dims()]);
        match &self.geom {
            Geom::Curve(c) => {
                let n = c.degree();
                for i in 0..=n {
                    let d = c.coords().clone().map(|p| p.coefs()[i].clone());
                    let a = near(&d, &c.weights().coefs()[i]);
                    if a < best.0 {
                        best = (a, smallvec![grid(i, n)]);
                    }
                }
            }
            Geom::Patch(p) => {
                let (m, n) = p.degrees();
                let [x, y, z, w] = p.tensors();
                for i in 0..=m {
                    for j in 0..=n {
                        let d = [x, y, z].map(|t| t.get(i, j).clone());
                        let a = near(&d, w.get(i, j));
                        if a < best.0 {
                            best = (a, smallvec![grid(i, m), grid(j, n)]);
                        }
                    }
                }
            }
        }
        best.1
    }

    /// Newton's method on the lattice from `c`, safeguarded to descend
    /// (module docs).
    fn newton(&self, mut c: Axes<u64>, work: &mut Work) -> Newton {
        let d = self.dims();
        let mut out = Newton {
            at: c.clone(),
            last: smallvec![0; d],
            settled: false,
            pushed_out: false,
        };
        for _ in 0..NEWTON_STEPS {
            work.steps = work.steps.saturating_add(1);
            let pt = self.lattice_point(&c);
            let at = |f: &NdTensor<S>| mid(&f.eval(&pt).expect("d variables"));
            let g: Axes<f64> = self.grad.iter().map(at).collect();
            let j: Axes<f64> = self.jac.iter().map(at).collect();
            // A variable at the boundary whose step leads out is held
            // there, the others solved without it: Newton along the
            // boundary.
            let newton = held_step(&c, |held| solve(&j, &g, held));
            let step = match newton {
                Some((s, held)) if s.iter().all(|&r| r == 0) => {
                    out.pushed_out = held.iter().any(|&h| h);
                    out.settled = true;
                    break;
                }
                Some((s, held)) if convex(&j, &held) && along(&g, &s) > 0.0 => (s, held),
                // Not a descent step (a saddle or a maximum attracts it,
                // or the Jacobian is singular): down the gradient.
                _ => match self.descent(&c, &g, &j, work) {
                    Descent::Step(s, held) => (s, held),
                    Descent::Held => {
                        out.pushed_out = true;
                        break;
                    }
                    Descent::Stuck => break,
                },
            };
            out.pushed_out = step.1.iter().any(|&h| h);
            c = moved(&c, &step.0);
            out.last = step.0;
        }
        out.at = c;
        out
    }

    /// A step down the gradient `g` from `c` (`j` the Jacobian there,
    /// both estimates): the variables at the boundary it leads out of
    /// held, the minimum of the quadratic model along `g` where its
    /// curvature is positive, at most half the domain in any variable,
    /// halved until the distance from `x` (estimated) decreases.
    fn descent(&self, c: &[u64], g: &[f64], j: &[f64], work: &mut Work) -> Descent {
        let d = g.len();
        let found = held_step(c, |held| {
            let free = |a: usize| !held[a];
            let (mut gg, mut gjg, mut top) = (0.0, 0.0, 0.0f64);
            for a in (0..d).filter(|&a| free(a)) {
                gg += g[a] * g[a];
                if g[a].abs() > top {
                    top = g[a].abs();
                }
                for b in (0..d).filter(|&b| free(b)) {
                    gjg += g[a] * j[a * d + b] * g[b];
                }
            }
            let cap = 0.5 / top;
            let alpha = if gjg > 0.0 && gg / gjg < cap {
                gg / gjg
            } else {
                cap
            };
            (0..d)
                .map(|a| {
                    if free(a) {
                        units(alpha * g[a])
                    } else {
                        Some(0)
                    }
                })
                .collect()
        });
        let Some((mut s, held)) = found else {
            return Descent::Stuck;
        };
        if held.iter().all(|&h| h) {
            return Descent::Held;
        }
        let here = self.dist2(c);
        while s.iter().any(|&r| r != 0) {
            work.steps = work.steps.saturating_add(1);
            if self.dist2(&moved(c, &s)) < here {
                return Descent::Step(s, held);
            }
            for r in &mut s {
                *r /= 2;
            }
        }
        Descent::Stuck
    }

    /// The squared distance from `x` at the lattice point `c`, an
    /// estimate.
    fn dist2(&self, c: &[u64]) -> f64 {
        let pt = self.lattice_point(c);
        match &self.geom {
            Geom::Curve(cv) => {
                let (d, w) = cv.eval_hom(&pt[0]);
                approx_dist2(&d, &w)
            }
            Geom::Patch(p) => {
                let [hx, hy, hz, w] = p.eval_hom(&pt[0], &pt[1]);
                approx_dist2(&[hx, hy, hz], &w)
            }
        }
    }

    /// The certificate around Newton's point (module docs).
    fn certify(&self, newton: &Newton, work: &mut Work) -> Foot {
        let full: Lat = smallvec![[0, SIDE]; self.dims()];
        let start: Lat = newton
            .at
            .iter()
            .zip(&newton.last)
            .map(|(&c, &r)| {
                let rad = u64::try_from(r.unsigned_abs())
                    .unwrap_or(SIDE)
                    .saturating_mul(2)
                    .saturating_add(START_RADIUS);
                [c.saturating_sub(rad), c.saturating_add(rad).min(SIDE)]
            })
            .collect();
        let mut sys = Restrict::new(&self.grad);
        let (x, verdict) = narrow(&mut sys, start, &full, false, work);
        let touches = || x.iter().any(|&[lo, hi]| lo == 0 || hi == SIDE);
        match verdict {
            Narrowed::Certified { cert, .. } => {
                // On the box the operator certified (its Jacobian regular
                // there); deeper boxes restrict the forms further, whose
                // rounding then widens the derivatives' hulls.
                let jac = sys.jacobian(&cert, work);
                let minimum = positive_definite(&jac, self.dims());
                let (point, spread) = self.foot_point(&x, &newton.at, work);
                Foot::Certified(Box::new(FootBox {
                    near: self.cell.bounds(&x, true),
                    unique: self.cell.bounds(&cert, false),
                    minimum,
                    point,
                    spread,
                }))
            }
            Narrowed::Unsettled { .. } if touches() => Foot::Boundary,
            Narrowed::Unsettled { contracting: true } if newton.settled => Foot::Precision,
            Narrowed::Empty | Narrowed::Unsettled { .. } => Foot::Subdivide,
        }
    }

    /// A box holding `S(foot)` for the foot in the lattice box `x`, from
    /// the control boxes of the piece on the (at most two per variable)
    /// dyadic cells covering `x`, split down by halves; and the bound of
    /// its distance from `S` at the lattice point `at`.
    fn foot_point(&self, x: &Lat, at: &Axes<u64>, work: &mut Work) -> ([[f64; 2]; 3], f64) {
        let covers: Axes<(u32, u64, u64)> = x.iter().map(|&[lo, hi]| cover(lo, hi)).collect();
        let mut pieces = vec![self.geom.clone()];
        for (axis, &(l, first, last)) in covers.iter().enumerate() {
            pieces = pieces
                .into_iter()
                .flat_map(|g| {
                    // Each cell at level `l` in `axis`, by halves from the
                    // piece.
                    (first..=last)
                        .map(|idx| {
                            let mut a = g.clone();
                            for t in 0..l {
                                work.splits = work.splits.saturating_add(1);
                                let [lo, hi] = halves(&a, axis);
                                a = if (idx >> (l - 1 - t)) & 1 == 1 {
                                    hi
                                } else {
                                    lo
                                };
                            }
                            a
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
        }
        let mut b = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for g in &pieces {
            let Some(c) = control_box(g) else {
                return ([[f64::NEG_INFINITY, f64::INFINITY]; 3], f64::INFINITY);
            };
            for k in 0..3 {
                if c[k][0] < b[k][0] {
                    b[k][0] = c[k][0];
                }
                if c[k][1] > b[k][1] {
                    b[k][1] = c[k][1];
                }
            }
        }
        let local: Axes<f64> = at.iter().map(|&l| lattice_f64(l)).collect();
        let spread = match self.offset_at(&local) {
            Some(e) => {
                let mut s = 0.0;
                for k in 0..3 {
                    let a = up_signed(b[k][1] - e[k][0]);
                    let c = up_signed(e[k][1] - b[k][0]);
                    s = add_up(s, sq_up(if a > c { a } else { c }));
                }
                up(s.sqrt())
            }
            None => f64::INFINITY,
        };
        let point =
            std::array::from_fn(|k| [down(self.x[k] + b[k][0]), up_signed(self.x[k] + b[k][1])]);
        (point, spread)
    }

    /// The enclosure of `S(z) − x` at the cell's parameter `z`, `None`
    /// unless `W(z)` is certified positive.
    fn offset_at(&self, z: &[f64]) -> Option<[[f64; 2]; 3]> {
        let like = self.like();
        let (d, w) = match &self.geom {
            Geom::Curve(c) => c.eval_hom(&S::from_f64(z[0], like)),
            Geom::Patch(p) => {
                let [hx, hy, hz, w] =
                    p.eval_hom(&S::from_f64(z[0], like), &S::from_f64(z[1], like));
                ([hx, hy, hz], w)
            }
        };
        let w = w.enclose();
        if w[0].is_nan() || w[0] <= 0.0 {
            return None;
        }
        let e = d.map(|c| quotient(c.enclose(), w));
        e.iter()
            .all(|[lo, hi]| lo.is_finite() && hi.is_finite())
            .then_some(e)
    }

    /// The control box of the piece in the frame centred at `x`.
    fn control_box(&self) -> Option<[[f64; 2]; 3]> {
        control_box(&self.geom)
    }

    fn lattice_point(&self, c: &[u64]) -> Vec<S> {
        let like = self.like();
        c.iter()
            .map(|&l| S::from_f64(lattice_f64(l), like))
            .collect()
    }

    fn like(&self) -> &S {
        &self.grad[0].coefs()[0]
    }
}

/// Newton's end.
struct Newton {
    /// The lattice point reached.
    at: Axes<u64>,
    /// The last step taken (in lattice units), zero if none.
    last: Axes<i128>,
    /// Whether the last step rounded to zero.
    settled: bool,
    /// Whether the last step held a variable at the boundary.
    pushed_out: bool,
}

/// A step down the gradient ([`Closest::descent`]).
enum Descent {
    /// The step (in lattice units, subtracted) and the variables held.
    Step(Axes<i128>, Axes<bool>),
    /// Every variable held at the boundary.
    Held,
    /// No step decreases the distance (or none is finite).
    Stuck,
}

/// `step(held)` with the variables at the boundary whose step leads out
/// held, repeated until none more is: the step and the variables held,
/// `None` where `step` gives none.
fn held_step(
    c: &[u64],
    step: impl Fn(&[bool]) -> Option<Axes<i128>>,
) -> Option<(Axes<i128>, Axes<bool>)> {
    let mut held: Axes<bool> = smallvec![false; c.len()];
    loop {
        let s = step(&held)?;
        let mut more = false;
        for a in 0..c.len() {
            let out = (c[a] == 0 && s[a] > 0) || (c[a] == SIDE && s[a] < 0);
            if out && !held[a] {
                held[a] = true;
                more = true;
            }
        }
        if !more {
            return Some((s, held));
        }
    }
}

/// `c − s` on the lattice, clamped to the domain.
fn moved(c: &[u64], s: &[i128]) -> Axes<u64> {
    let side = i128::from(SIDE);
    c.iter()
        .zip(s)
        .map(|(&l, &r)| (i128::from(l) - r).clamp(0, side) as u64)
        .collect()
}

/// Whether the Jacobian estimate `j` is positive definite on the
/// variables not held (its symmetric part: the Hessian's where `g` is
/// small), so a Newton step heads for a minimum.
fn convex(j: &[f64], held: &[bool]) -> bool {
    match held {
        [false] | [false, true] => j[0] > 0.0,
        [true, false] => j[3] > 0.0,
        [false, false] => {
            let off = 0.5 * (j[1] + j[2]);
            j[0] > 0.0 && j[3] > 0.0 && j[0] * j[3] > off * off
        }
        _ => true,
    }
}

/// `g·s`: positive where the step `s` (subtracted) descends.
fn along(g: &[f64], s: &[i128]) -> f64 {
    g.iter().zip(s).map(|(&a, &r)| a * r as f64).sum()
}

/// The squared distance of the homogeneous offset `(d, w)` from the
/// origin, from the enclosures' middles: an estimate.
fn approx_dist2<S: Scalar>(d: &[S; 3], w: &S) -> f64 {
    let w = mid(w);
    d.iter().fold(0.0, |acc, c| {
        let v = mid(c) / w;
        acc + v * v
    })
}

/// [`Closest::curvature_bound`] for a patch: the better of two bounds of
/// `W²·|λ| / |Q|`, `λ` the generalized eigenvalues of the forms `(L, M,
/// N)` and `(E, F, G)` (`D = EG − F² = |Q|²`). Exactly, `λ = (H ±
/// √(H² − 4DK)) / 2D` with `H = LG + NE − 2MF` and `K = LN − M²`, bounded
/// from the hulls of `H`, `H² − 4DK` and `D`; where those forms pass
/// [`MAX_DEGREE`](super::MAX_DEGREE), `|λ| ≤ ρ / λ_min`, `ρ` the second
/// form's largest eigenvalue magnitude (at most `|L + N|/2 + √(((L −
/// N)/2)² + M²)` and at most its Frobenius norm) and `λ_min` the first
/// form's least eigenvalue (at least `(E + G)/2 − √(((E − G)/2)² + F²)`
/// and at least `D/(E + G)`), each from the hulls of its forms.
fn patch_curvature<S: Scalar>(p: &HomPatch<S>) -> Option<f64> {
    let nu = p.partial_numerator(Dir::U)?;
    let nv = p.partial_numerator(Dir::V)?;
    let q = cross(&nu, &nv)?;
    let d = |f: &[Tensor<S>; 3], dir| f.clone().map(|t| t.derivative(dir));
    let (a, b, c) = (d(&nu, Dir::U), d(&nu, Dir::V), d(&nv, Dir::V));
    let [l, m, n] = [dot(&a, &q)?, dot(&b, &q)?, dot(&c, &q)?];
    let [e, f, g] = [dot(&nu, &nu)?, dot(&nu, &nv)?, dot(&nv, &nv)?];
    let dd = dot(&q, &q)?;
    let dlo = dd.hull().lo;
    if dlo.is_nan() || dlo <= 0.0 {
        return None;
    }
    let w = sq_up(p.weights().hull().abs_max());
    let like = &dd.coefs()[0];
    let fine = || -> Option<f64> {
        let h = l
            .mul(&g)?
            .add(&n.mul(&e)?)
            .sub(&m.mul(&f)?.scale(&S::int(2, like)));
        let k = l.mul(&n)?.sub(&m.mul(&m)?);
        let disc = h.mul(&h)?.sub(&dd.mul(&k)?.scale(&S::int(4, like)));
        let hi = disc.hull().hi;
        let root = if hi > 0.0 { up(hi.sqrt()) } else { 0.0 };
        let num = add_up(h.hull().abs_max(), root);
        let den = down(2.0 * down(dlo * down(dlo.sqrt())));
        (den > 0.0).then(|| up(mul_up(w, num) / den))
    };
    let crude = || -> Option<f64> {
        let half = |h: Hull| mul_up(h.abs_max(), 0.5);
        let root = |a: f64, b: f64| up(add_up(sq_up(a), sq_up(b)).sqrt());
        let eig = add_up(
            half(l.add(&n).hull()),
            root(half(l.sub(&n).hull()), m.hull().abs_max()),
        );
        // `√2·|M|`, the factor rounded up.
        let m2 = mul_up(
            m.hull().abs_max(),
            std::f64::consts::SQRT_2 * (1.0 + 4.0 * f64::EPSILON),
        );
        let frob = root(root(l.hull().abs_max(), n.hull().abs_max()), m2);
        let second = if frob < eig { frob } else { eig };
        let trace = e.add(&g).hull();
        let by_eig =
            down(mul_up(down(trace.lo), 0.5) - root(half(e.sub(&g).hull()), f.hull().abs_max()));
        let by_det = down(dlo / trace.hi);
        let least = if by_det > by_eig { by_det } else { by_eig };
        let den = down(down(dlo.sqrt()) * least);
        (least > 0.0 && den > 0.0).then(|| up(mul_up(w, second) / den))
    };
    match (
        fine().filter(|r| r.is_finite()),
        crude().filter(|r| r.is_finite()),
    ) {
        (Some(a), Some(b)) => Some(if b < a { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// Whether every control point and `x` are finite and every weight is
/// positive.
fn finite(pts: &[[f64; 4]], x: [f64; 3]) -> bool {
    x.iter().all(|c| c.is_finite())
        && pts
            .iter()
            .all(|p| p.iter().all(|c| c.is_finite()) && p[3] > 0.0)
}

/// The homogeneous control point `(w·(p − x), w)` of the affine `(p, w)`,
/// the offset formed first (one rounding at most), then weighted.
fn offset<S: Scalar>(p: &[f64; 4], x: [f64; 3], like: &S) -> ([S; 3], S) {
    let w = S::from_f64(p[3], like);
    let d = std::array::from_fn(|k| {
        S::from_f64(p[k], like)
            .sub(&S::from_f64(x[k], like))
            .mul(&w)
    });
    (d, w)
}

/// The gradient system of the piece (module docs), `None` past the
/// degrees.
fn gradient<S: Scalar>(geom: &Geom<S>) -> Option<Vec<NdTensor<S>>> {
    Some(match geom {
        Geom::Curve(c) => {
            let t = c.tangent_numerator()?;
            vec![NdTensor::from_poly(&dot(c.coords(), &t)?)]
        }
        Geom::Patch(p) => {
            let [x, y, z, _] = p.tensors().clone();
            let d = [x, y, z];
            let gu = dot(&d, &p.partial_numerator(Dir::U)?)?;
            let gv = dot(&d, &p.partial_numerator(Dir::V)?)?;
            vec![NdTensor::from_tensor(&gu), NdTensor::from_tensor(&gv)]
        }
    })
}

/// The piece's halves at ½ in `axis`.
fn halves<S: Scalar>(geom: &Geom<S>, axis: usize) -> [Geom<S>; 2] {
    match geom {
        Geom::Curve(c) => c.split_half().map(Geom::Curve),
        Geom::Patch(p) => {
            let dir = if axis == 0 { Dir::U } else { Dir::V };
            p.split_half(dir).map(Geom::Patch)
        }
    }
}

/// The control box of a piece, `None` unless every weight is certified
/// positive.
fn control_box<S: Scalar>(geom: &Geom<S>) -> Option<[[f64; 2]; 3]> {
    match geom {
        Geom::Curve(c) => c.control_box(),
        Geom::Patch(p) => p
            .hull_box()
            .map(|b| std::array::from_fn(|k| [b.lo[k], b.hi[k]])),
    }
}

/// An upper bound of the largest distance from the origin to a point of
/// the box.
fn farthest(b: &[[f64; 2]; 3]) -> f64 {
    let mut s = 0.0;
    for [lo, hi] in b {
        let (a, c) = (-lo, *hi);
        s = add_up(s, sq_up(if a > c { a } else { c }));
    }
    up(s.sqrt())
}

/// An upper bound of the norm of a vector whose components lie in the
/// hulls.
fn norm_up(h: &[Hull; 3]) -> f64 {
    let mut s = 0.0;
    for c in h {
        s = add_up(s, sq_up(c.abs_max()));
    }
    up(s.sqrt())
}

/// `w_max²·num / lo^(3/2)` rounded up, `w` the hull of the (positive)
/// weights; `None` unless `lo > 0` and the result is finite.
fn over_cubed(w: Hull, num: f64, lo: f64) -> Option<f64> {
    if lo.is_nan() || lo <= 0.0 {
        return None;
    }
    let den = down(lo * down(lo.sqrt()));
    if den.is_nan() || den <= 0.0 {
        return None;
    }
    let r = up(mul_up(sq_up(w.abs_max()), num) / den);
    r.is_finite().then_some(r)
}

/// `x²` rounded up.
fn sq_up(x: f64) -> f64 {
    mul_up(x, x)
}

/// The middle of a value's enclosure, an estimate.
fn mid<S: Scalar>(x: &S) -> f64 {
    let [lo, hi] = x.enclose();
    0.5 * lo + 0.5 * hi
}

/// The Newton step `J⁻¹·g` in lattice units, the `held` variables' zero
/// and the system solved in the others (Cramer's rule for two, which
/// commutes with negating a variable to the bit); `None` for a zero or
/// non-finite pivot or a step that isn't finite.
fn solve(j: &[f64], g: &[f64], held: &[bool]) -> Option<Axes<i128>> {
    let s: Axes<f64> = match (g.len(), held) {
        (1, [true]) | (2, [true, true]) => smallvec![0.0; g.len()],
        (1, _) => smallvec![g[0] / j[0]],
        (_, [true, false]) => smallvec![0.0, g[1] / j[3]],
        (_, [false, true]) => smallvec![g[0] / j[0], 0.0],
        _ => {
            let det = j[0] * j[3] - j[1] * j[2];
            if det == 0.0 {
                return None;
            }
            smallvec![
                (j[3] * g[0] - j[1] * g[1]) / det,
                (j[0] * g[1] - j[2] * g[0]) / det
            ]
        }
    };
    s.iter().map(|&v| units(v)).collect()
}

/// A step `v` in lattice units, rounded to an integer by ties to even
/// (`units(−v) = −units(v)`), clamped far past the lattice; `None` for a
/// value that isn't finite.
fn units(v: f64) -> Option<i128> {
    const FAR: f64 = (1u64 << 60) as f64;
    let s = v * SIDE as f64;
    s.is_finite()
        .then(|| s.round_ties_even().clamp(-FAR, FAR) as i128)
}

/// The lattice point nearest `t`, clamped to `[0, 1]` (ties to even).
fn to_lattice(t: f64) -> u64 {
    let s = (t * SIDE as f64).round_ties_even();
    if s > 0.0 {
        if s < SIDE as f64 { s as u64 } else { SIDE }
    } else {
        0
    }
}

/// The lattice point `l·2⁻⁵³`, exactly.
fn lattice_f64(l: u64) -> f64 {
    l as f64 * pow2_neg(LATTICE)
}

/// The lattice point nearest `i/n` (ties to even, in integers, so
/// `grid(n − i, n) = SIDE − grid(i, n)`).
fn grid(i: usize, n: usize) -> u64 {
    if n == 0 {
        return 0;
    }
    let (i, n) = (i as u128, n as u128);
    let num = i * u128::from(SIDE);
    let (q, r) = (num / n, num % n);
    let round_up = 2 * r > n || (2 * r == n && q % 2 == 1);
    (q + u128::from(round_up)) as u64
}

/// Whether every matrix in the interval matrix `j` (`d × d`, row-major)
/// is positive definite as the symmetric Jacobian at a root: its first
/// entry positive and, for two variables, its determinant.
fn positive_definite(j: &[[f64; 2]], d: usize) -> bool {
    if j[0][0].is_nan() || j[0][0] <= 0.0 {
        return false;
    }
    if d == 1 {
        return true;
    }
    let det = sub(mul(j[0], j[3]), mul(j[1], j[2]));
    det[0] > 0.0
}

#[cfg(test)]
mod tests;
