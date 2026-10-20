//! The smooth pair's oracle: a biquadratic sheet against a cylinder
//! (module docs of [`super`]).

use super::{OracleError, P3, add3, dot3, q, scale3, sub3};
use crate::sheets::SmoothPair;
use malachite::base::num::arithmetic::traits::PowerOf2;
use malachite::base::num::basic::traits::{One, Zero};
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode;
use malachite::{Float, Rational};

/// The deepest a box of [`SmoothOracle::least`] or an interval of
/// [`SmoothOracle::roots`] is split (`2^-60` of the parameter range).
const MAX_DEPTH: u32 = 60;

/// A biquadratic sheet and a cylinder, exactly (module docs of
/// [`super`]).
#[derive(Clone, Debug)]
pub struct SmoothOracle {
    /// The sheet's control points, its one weight divided out.
    net: [P3; 9],
    axis_point: P3,
    axis: P3,
    radius: Rational,
    r2: Rational,
    /// The Bernstein coefficients of `D(u, v)`, `d[i][j]` at `u` index
    /// `i`, degree 4 in each.
    d: [[Rational; 5]; 5],
}

/// An enclosure of the least squared distance from the sheet to the axis
/// ([`SmoothOracle::least`]): `lo ≤ min D ≤ hi = D(at)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Least {
    pub lo: Rational,
    pub hi: Rational,
    /// The parameter `(u, v)` the upper bound is reached at.
    pub at: [Rational; 2],
    /// The boxes split on the way.
    pub boxes: usize,
}

/// An enclosure of the least gap `√D − r` from the sheet to the cylinder
/// (mm), rounded outward ([`SmoothOracle::gap_bounds`]).
#[derive(Clone, Debug, PartialEq)]
pub struct GapBounds {
    pub lo: Float,
    pub hi: Float,
}

/// Whether the sheet clears the cylinder ([`SmoothOracle::relation`]).
#[derive(Clone, Debug, PartialEq)]
pub enum SmoothRelation {
    /// Every point of the sheet lies outside the cylinder: `min D > r²`.
    Clear(Least),
    /// A point of the sheet lies inside (at `Least::at`): `min D < r²`.
    Crossing(Least),
    /// The enclosure contains `r²`: touching, or a gap below its width.
    Undecided(Least),
}

/// A parameter line of the sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// `u` running over `[0, 1]` at this `v`.
    AlongU { v: Rational },
    /// `v` running over `[0, 1]` at this `u`.
    AlongV { u: Rational },
}

/// A zero of `D − r²` along a [`Line`], within `[lo, hi]` of its running
/// parameter ([`SmoothOracle::roots`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    pub lo: Rational,
    pub hi: Rational,
    /// Exactly one zero (`lo = hi`: the zero itself, a value hit
    /// exactly); else an interval at the width limit that may hold
    /// several, or none (a tangency, or two crossings closer than it).
    pub single: bool,
}

const C2: [i64; 3] = [1, 2, 1];
const C4: [i64; 5] = [1, 4, 6, 4, 1];

impl SmoothOracle {
    /// The pair taken exactly. Its net's weights must be one positive
    /// value (a polynomial patch), its axis non-zero, its radius positive.
    pub fn new(p: &SmoothPair) -> Result<SmoothOracle, OracleError> {
        let w = p.net[0][3];
        let finite = p.net.iter().flatten().all(|x| x.is_finite())
            && p.axis_point.iter().chain(&p.axis).all(|x| x.is_finite())
            && p.radius.is_finite();
        if !finite
            || w <= 0.0
            || p.net.iter().any(|c| c[3] != w)
            || p.axis.iter().all(|&x| x == 0.0)
            || p.radius <= 0.0
        {
            return Err(OracleError::BadNet);
        }
        let net: [P3; 9] = std::array::from_fn(|n| [0, 1, 2].map(|c| q(p.net[n][c])));
        let axis_point = p.axis_point.map(q);
        let axis = p.axis.map(q);
        let radius = q(p.radius);
        let r2 = &radius * &radius;
        // D = |x − a|² − ((x − a)·e)²/|e|², from the Bernstein forms of
        // the coordinates of `x − a` (degree 2 in each parameter).
        let rel: Vec<[[Rational; 3]; 3]> = (0..3)
            .map(|c| {
                std::array::from_fn(|i| {
                    std::array::from_fn(|j| &net[3 * i + j][c] - &axis_point[c])
                })
            })
            .collect();
        let along: [[Rational; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                &(&rel[0][i][j] * &axis[0])
                    + &(&rel[1][i][j] * &axis[1])
                    + &(&rel[2][i][j] * &axis[2])
            })
        });
        let e2 = dot3(&axis, &axis);
        let sq: Vec<[[Rational; 5]; 5]> = rel.iter().map(|x| product(x, x)).collect();
        let l2 = product(&along, &along);
        let d = std::array::from_fn(|i| {
            std::array::from_fn(|j| &sq[0][i][j] + &sq[1][i][j] + &sq[2][i][j] - &l2[i][j] / &e2)
        });
        Ok(SmoothOracle {
            net,
            axis_point,
            axis,
            radius,
            r2,
            d,
        })
    }

    /// The radius, exactly.
    pub fn radius(&self) -> &Rational {
        &self.radius
    }

    /// The sheet's point at `(u, v)`.
    pub fn point(&self, u: &Rational, v: &Rational) -> P3 {
        let (bu, bv) = (bern2(u), bern2(v));
        let mut p = [Rational::ZERO, Rational::ZERO, Rational::ZERO];
        for i in 0..3 {
            for j in 0..3 {
                p = add3(&p, &scale3(&self.net[3 * i + j], &(&bu[i] * &bv[j])));
            }
        }
        p
    }

    /// The squared distance from `x` to the axis.
    pub fn axis_dist2(&self, x: &P3) -> Rational {
        let r = sub3(x, &self.axis_point);
        let t = dot3(&r, &self.axis);
        dot3(&r, &r) - &t * &t / dot3(&self.axis, &self.axis)
    }

    /// `D − r²` at the sheet's point `(u, v)`: positive outside the
    /// cylinder, zero on it, negative inside.
    pub fn implicit(&self, u: &Rational, v: &Rational) -> Rational {
        self.axis_dist2(&self.point(u, v)) - &self.r2
    }

    /// `D(u, v)` from its Bernstein form.
    pub fn d_at(&self, u: &Rational, v: &Rational) -> Rational {
        let rows: [Rational; 5] = std::array::from_fn(|i| eval4(&self.d[i], v));
        eval4(&rows, u)
    }

    /// An enclosure of `min D` over the sheet, by branch and bound on
    /// `D`'s Bernstein form over dyadic boxes of `[0, 1]²` (each bounded
    /// below by its least coefficient, above by its corners' values):
    /// until `hi − lo ≤ tol`, `max_boxes` boxes are split, or the box of
    /// the least bound is at the depth limit.
    pub fn least(&self, tol: &Rational, max_boxes: usize) -> Least {
        struct Bx {
            lo: Rational,
            depth: u32,
            at: [u64; 2],
            c: [[Rational; 5]; 5],
        }
        let make = |c: [[Rational; 5]; 5], depth: u32, at: [u64; 2]| Bx {
            lo: least_of(&c),
            depth,
            at,
            c,
        };
        let mut best: Option<(Rational, [Rational; 2])> = None;
        let corners = |b: &Bx, best: &mut Option<(Rational, [Rational; 2])>| {
            let scale = Rational::power_of_2(-(b.depth as i64));
            for (ci, cj) in [(0, 0), (4, 0), (0, 4), (4, 4)] {
                let value = &b.c[ci][cj];
                if best.as_ref().is_none_or(|(h, _)| value < h) {
                    let u = Rational::from(b.at[0] + (ci / 4) as u64) * &scale;
                    let v = Rational::from(b.at[1] + (cj / 4) as u64) * &scale;
                    *best = Some((value.clone(), [u, v]));
                }
            }
        };
        let root = make(self.d.clone(), 0, [0, 0]);
        corners(&root, &mut best);
        let mut work = vec![root];
        let mut boxes = 0;
        loop {
            let (hi, at) = best.clone().expect("the root box has corners");
            work.retain(|b| b.lo < hi);
            let Some(k) = (0..work.len()).min_by(|&x, &y| work[x].lo.cmp(&work[y].lo)) else {
                return Least {
                    lo: hi.clone(),
                    hi,
                    at,
                    boxes,
                };
            };
            if &(&hi - &work[k].lo) <= tol || boxes >= max_boxes || work[k].depth >= MAX_DEPTH {
                return Least {
                    lo: work[k].lo.clone(),
                    hi,
                    at,
                    boxes,
                };
            }
            let b = work.swap_remove(k);
            boxes += 1;
            let (left, right) = split_u(&b.c);
            for (c, du) in [(left, 0), (right, 1)] {
                let (low, high) = split_v(&c);
                for (cc, dv) in [(low, 0), (high, 1)] {
                    let child = make(cc, b.depth + 1, [2 * b.at[0] + du, 2 * b.at[1] + dv]);
                    corners(&child, &mut best);
                    work.push(child);
                }
            }
        }
    }

    /// The least gap `√D − r` (mm) of an enclosure of `min D`, in big
    /// floats of `prec` bits rounded outward.
    pub fn gap_bounds(&self, least: &Least, prec: u64) -> GapBounds {
        let side = |d: &Rational, down: bool| {
            let (rm, rr) = if down {
                (RoundingMode::Floor, RoundingMode::Ceiling)
            } else {
                (RoundingMode::Ceiling, RoundingMode::Floor)
            };
            let d = if *d < 0 { Rational::ZERO } else { d.clone() };
            let root = Float::from_rational_prec_round(d, prec, rm)
                .0
                .sqrt_prec_round(prec, rm)
                .0;
            let r = Float::from_rational_prec_round(self.radius.clone(), prec, rr).0;
            root.sub_prec_round(r, prec, rm).0
        };
        GapBounds {
            lo: side(&least.lo, true),
            hi: side(&least.hi, false),
        }
    }

    /// Whether the sheet clears the cylinder, by [`SmoothOracle::least`].
    pub fn relation(&self, tol: &Rational, max_boxes: usize) -> SmoothRelation {
        let least = self.least(tol, max_boxes);
        if least.lo > self.r2 {
            SmoothRelation::Clear(least)
        } else if least.hi < self.r2 {
            SmoothRelation::Crossing(least)
        } else {
            SmoothRelation::Undecided(least)
        }
    }

    /// The zeros of `D − r²` along `line` (where the sheet crosses or
    /// touches the cylinder), in order: isolated by halving `[0, 1]`,
    /// an interval whose Bernstein coefficients have no sign change
    /// dropped, one with one sign change holding exactly one zero (kept
    /// once at most `width` wide), a value hit exactly kept as itself;
    /// an interval with more sign changes at most `width` wide (or at the
    /// depth limit) is kept as not [`Root::single`].
    pub fn roots(&self, line: &Line, width: &Rational) -> Vec<Root> {
        let c: [Rational; 5] = match line {
            Line::AlongU { v } => std::array::from_fn(|i| eval4(&self.d[i], v) - &self.r2),
            Line::AlongV { u } => std::array::from_fn(|j| {
                let col: [Rational; 5] = std::array::from_fn(|i| self.d[i][j].clone());
                eval4(&col, u) - &self.r2
            }),
        };
        let mut out = Vec::new();
        let (zero, one) = (Rational::ZERO, Rational::ONE);
        if c[0] == 0 {
            out.push(Root {
                lo: zero.clone(),
                hi: zero.clone(),
                single: true,
            });
        }
        let end = c[4] == 0;
        isolate(c, zero, one.clone(), width, 0, &mut out);
        if end {
            out.push(Root {
                lo: one.clone(),
                hi: one,
                single: true,
            });
        }
        out
    }
}

impl GapBounds {
    /// The bounds as f64s, rounded outward.
    pub fn f64_bounds(&self) -> [f64; 2] {
        [
            f64::rounding_from(&self.lo, RoundingMode::Floor).0,
            f64::rounding_from(&self.hi, RoundingMode::Ceiling).0,
        ]
    }

    /// The kernel's value `v` with its error bound `err` agrees with the
    /// enclosure: `[v − err, v + err]` meets `[lo, hi]`, exactly.
    pub fn meets(&self, v: f64, err: f64) -> bool {
        if !v.is_finite() || !err.is_finite() || err < 0.0 {
            return false;
        }
        let (Ok(lo), Ok(hi)) = (Rational::try_from(&self.lo), Rational::try_from(&self.hi)) else {
            return false;
        };
        let (v, e) = (q(v), q(err));
        &v - &e <= hi && &v + &e >= lo
    }
}

/// The isolation of [`SmoothOracle::roots`] on `[lo, hi]`, its Bernstein
/// coefficients `c`.
fn isolate(
    c: [Rational; 5],
    lo: Rational,
    hi: Rational,
    width: &Rational,
    depth: u32,
    out: &mut Vec<Root>,
) {
    let v = variations(&c);
    if v == 0 {
        return;
    }
    let narrow = &(&hi - &lo) <= width;
    if narrow || depth >= MAX_DEPTH {
        out.push(Root {
            lo,
            hi,
            single: v == 1,
        });
        return;
    }
    let (left, right) = split(&c);
    let mid = (&lo + &hi) / Rational::from(2);
    let hit = left[4] == 0;
    isolate(left, lo, mid.clone(), width, depth + 1, out);
    if hit {
        out.push(Root {
            lo: mid.clone(),
            hi: mid.clone(),
            single: true,
        });
    }
    isolate(right, mid, hi, width, depth + 1, out);
}

/// Sign changes along `c`, zeros skipped.
fn variations(c: &[Rational; 5]) -> u32 {
    let mut last: Option<bool> = None;
    let mut n = 0;
    for x in c {
        if *x == 0 {
            continue;
        }
        let pos = *x > 0;
        if last.is_some_and(|l| l != pos) {
            n += 1;
        }
        last = Some(pos);
    }
    n
}

/// The product of two biquadratic Bernstein forms, degree 4 in each.
fn product(a: &[[Rational; 3]; 3], b: &[[Rational; 3]; 3]) -> [[Rational; 5]; 5] {
    let mut c: [[Rational; 5]; 5] =
        std::array::from_fn(|_| std::array::from_fn(|_| Rational::ZERO));
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                for l in 0..3 {
                    let w = Rational::from(C2[i] * C2[j] * C2[k] * C2[l])
                        / Rational::from(C4[i + k] * C4[j + l]);
                    c[i + k][j + l] += &a[i][j] * &b[k][l] * w;
                }
            }
        }
    }
    c
}

/// The quadratic Bernstein basis at `t`.
fn bern2(t: &Rational) -> [Rational; 3] {
    let s = Rational::ONE - t;
    [&s * &s, Rational::from(2) * &s * t, t * t]
}

/// A degree-4 Bernstein form at `t` (de Casteljau).
fn eval4(c: &[Rational; 5], t: &Rational) -> Rational {
    let s = Rational::ONE - t;
    let mut b = c.clone();
    for r in 1..5 {
        for k in 0..5 - r {
            b[k] = &b[k] * &s + &b[k + 1] * t;
        }
    }
    b[0].clone()
}

/// A degree-4 Bernstein form split at `½` into its halves' forms.
fn split(c: &[Rational; 5]) -> ([Rational; 5], [Rational; 5]) {
    let half = Rational::from_signeds(1i64, 2);
    let mut b = c.clone();
    let mut left: [Rational; 5] = std::array::from_fn(|_| Rational::ZERO);
    let mut right = left.clone();
    left[0] = b[0].clone();
    right[4] = b[4].clone();
    for r in 1..5 {
        for k in 0..5 - r {
            b[k] = (&b[k] + &b[k + 1]) * &half;
        }
        left[r] = b[0].clone();
        right[4 - r] = b[4 - r].clone();
    }
    (left, right)
}

/// The grid split at `u = ½`.
fn split_u(c: &[[Rational; 5]; 5]) -> ([[Rational; 5]; 5], [[Rational; 5]; 5]) {
    let mut left: [[Rational; 5]; 5] =
        std::array::from_fn(|_| std::array::from_fn(|_| Rational::ZERO));
    let mut right = left.clone();
    for j in 0..5 {
        let col: [Rational; 5] = std::array::from_fn(|i| c[i][j].clone());
        let (l, r) = split(&col);
        for i in 0..5 {
            left[i][j] = l[i].clone();
            right[i][j] = r[i].clone();
        }
    }
    (left, right)
}

/// The grid split at `v = ½`.
fn split_v(c: &[[Rational; 5]; 5]) -> ([[Rational; 5]; 5], [[Rational; 5]; 5]) {
    let mut low = c.clone();
    let mut high = c.clone();
    for i in 0..5 {
        let (l, r) = split(&c[i]);
        low[i] = l;
        high[i] = r;
    }
    (low, high)
}

fn least_of(c: &[[Rational; 5]; 5]) -> Rational {
    let mut m = &c[0][0];
    for x in c.iter().flatten() {
        if x < m {
            m = x;
        }
    }
    m.clone()
}
