//! Exact local shapes ("germs") of trimming curves at a point lying
//! exactly on a box line, for the box-line convention of the parent
//! module ("Box lines read as `c + 0⁺`"): every `u` line `u = c` is read
//! as `u = c + ε` and every `v` line `v = c` as `v = c + √2·ε`, `ε → 0⁺`.
//!
//! A **leg** is a rational quadratic p-span read from one of its ends `P`
//! outwards. Where `P` lies exactly on a box line `x_k = L`, the leg
//! crosses the moved line `x_k = L + δ` (`δ` the line's `ε` or `√2·ε`)
//! exactly when it leaves `P` to the line's high side (its first control
//! point off the line from `P` above it); a leg along the line, or leaving
//! it downwards, stays below the moved line next to `P`. Its **germ** is
//! where along the other coordinate `o` that crossing lies from `P`, as
//! `δ → 0⁺`, exactly:
//!
//! - a leg leaving `P` across the line: a power series `f₁δ + f₂δ² + …`
//!   (the leg's `x_k` and `x_o` as series in its parameter, the parameter
//!   as a series in `δ` by reversion, composed), to the fourth order:
//!   two different conics through `P` meet there with multiplicity at
//!   most 4, so legs agreeing that far are one curve;
//! - a leg leaving `P` tangent to the line and curving up: `±√(c·δ)`, far
//!   larger than any series term;
//! - a straight leg (its middle control point at `P`): `(b₂/a₂)·δ`
//!   exactly.
//!
//! The crossing lies on a side of the moved line whose end, also moved,
//! is at `o = A ± δ_o` where `P_o = A`: past that end exactly when the
//! germ is above `±r·δ`, `r = δ_o/δ`, which is `√2` on a `u` line and
//! `1/√2` on a `v` line, irrational, so a series' first term never ties
//! with it. A line moved to its low side instead (the low edge of the
//! face's tiles, `c − 0⁺`) is the same read with its axis reversed: a leg
//! crosses it where it leaves `P` downwards.
//!
//! **Sectors.** A point infinitesimally near a vertex `w`, in a direction
//! `d` no curve at `w` leaves in, is inside the face exactly when the
//! first curve met turning clockwise from `d` leaves `w` as an outgoing
//! coedge (the face on its left) on an unflipped face, as an incoming one
//! on a flipped face; curves leaving in one direction are ordered by
//! their bend across it (their germs against their common tangent).

use std::cmp::Ordering;

use malachite::Rational;
use malachite::base::num::basic::traits::Zero;

use super::rat;
use crate::count::boxes::UvSpan;
use crate::records::cusp::{scompose, sdiv, srevert};

/// The series orders a germ is compared to.
const ORDERS: usize = 4;

/// Where a leg's crossing with a moved line lies along the line from the
/// leg's end, as the line's shift `δ → 0⁺` (module docs).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Germ {
    /// `±√(sq·δ)`: `+` where `up`.
    Root { up: bool, sq: Rational },
    /// `Σ fᵢ·δ^i`, `i = 1, 2, …`: the terms known, the first first. Two
    /// series agreeing on every term both know are not told apart.
    Series(Vec<Rational>),
}

impl Germ {
    /// The order of two germs, smaller first; `None` where they can't be
    /// told apart.
    pub(super) fn cmp(&self, other: &Germ) -> Option<Ordering> {
        match (self, other) {
            (Germ::Root { up: a, sq: x }, Germ::Root { up: b, sq: y }) => match (a, b) {
                (true, false) => Some(Ordering::Greater),
                (false, true) => Some(Ordering::Less),
                (true, true) => nonzero(x.cmp(y)),
                (false, false) => nonzero(y.cmp(x)),
            },
            (Germ::Root { up, .. }, Germ::Series(_)) => Some(if *up {
                Ordering::Greater
            } else {
                Ordering::Less
            }),
            (Germ::Series(_), Germ::Root { .. }) => other.cmp(self).map(Ordering::reverse),
            (Germ::Series(a), Germ::Series(b)) => a
                .iter()
                .zip(b)
                .map(|(x, y)| x.cmp(y))
                .find(|o| *o != Ordering::Equal),
        }
    }

    /// The germ against `s·r·δ`, `r = √2` for a crossing of a `u` line
    /// (`k = 0`), `1/√2` of a `v` line, `s = ±1` the direction the end is
    /// moved in (module docs): never equal.
    pub(super) fn against(&self, k: usize, s: i8) -> Ordering {
        match self {
            Germ::Root { up, .. } => {
                if *up {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            Germ::Series(f) => {
                let s = Rational::from(i64::from(s));
                // `f₁ − s·√2`, or `(2f₁ − s·√2)/2`.
                if k == 0 {
                    sign_sqrt2(&f[0], &-s)
                } else {
                    sign_sqrt2(&(Rational::from(2) * &f[0]), &-s)
                }
            }
        }
    }

    /// The germ of the crossing on the line moved the other way along
    /// the crossing axis, read from the same point: `f₁` negated (a first
    /// term only, [`corner_pass`](super::corner_pass)'s).
    pub(super) fn first(f1: Rational, down: bool) -> Germ {
        Germ::Series(vec![if down { -f1 } else { f1 }])
    }
}

fn nonzero(o: Ordering) -> Option<Ordering> {
    (o != Ordering::Equal).then_some(o)
}

/// A rational quadratic span read from one end `P` outwards: its other
/// two control points relative to `P` and its weights, from `P`.
#[derive(Clone, Debug)]
pub(super) struct Leg {
    d: [[Rational; 2]; 2],
    w: [Rational; 3],
}

impl Leg {
    /// `sp` read from its end (`at_end`) or its start.
    pub(super) fn new(sp: &UvSpan, at_end: bool) -> Leg {
        let order = if at_end { [2, 1, 0] } else { [0, 1, 2] };
        let p = [rat(sp[order[0]][0]), rat(sp[order[0]][1])];
        let rel = |i: usize| [0, 1].map(|c| rat(sp[i][c]) - &p[c]);
        Leg {
            d: [rel(order[1]), rel(order[2])],
            w: order.map(|i| rat(sp[i][2])),
        }
    }

    /// `sp` split at the exact parameter `t` (strictly inside `(0, 1)`,
    /// the weights positive), read from the point there: back towards its
    /// start, then on towards its end; and the point. Exact homogeneous
    /// de Casteljau.
    pub(super) fn split(sp: &UvSpan, t: &Rational) -> ([Rational; 2], [Leg; 2]) {
        let s = Rational::from(1) - t;
        let h: [[Rational; 3]; 3] = sp.map(|p| {
            let w = rat(p[2]);
            [rat(p[0]) * &w, rat(p[1]) * &w, w]
        });
        let lerp = |a: &[Rational; 3], b: &[Rational; 3]| -> [Rational; 3] {
            std::array::from_fn(|i| &s * &a[i] + t * &b[i])
        };
        let (h01, h12) = (lerp(&h[0], &h[1]), lerp(&h[1], &h[2]));
        let m = lerp(&h01, &h12);
        let at = |x: &[Rational; 3]| [&x[0] / &x[2], &x[1] / &x[2]];
        let p = at(&m);
        let leg = |a: &[Rational; 3], b: &[Rational; 3]| {
            let rel = |x: &[Rational; 3]| {
                let x = at(x);
                [&x[0] - &p[0], &x[1] - &p[1]]
            };
            Leg {
                d: [rel(a), rel(b)],
                w: [m[2].clone(), a[2].clone(), b[2].clone()],
            }
        };
        let legs = [leg(&h01, &h[0]), leg(&h12, &h[2])];
        (p, legs)
    }

    /// The straight leg from `p` to `q`.
    pub(super) fn straight(p: &[Rational; 2], q: &[Rational; 2]) -> Leg {
        let d = [&q[0] - &p[0], &q[1] - &p[1]];
        let half = Rational::from(1) / Rational::from(2);
        Leg {
            d: [[&d[0] * &half, &d[1] * &half], d],
            w: [1, 1, 1].map(Rational::from),
        }
    }

    /// The point `P`, exactly.
    pub(super) fn point(sp: &UvSpan, at_end: bool) -> [Rational; 2] {
        let i = if at_end { 2 } else { 0 };
        [rat(sp[i][0]), rat(sp[i][1])]
    }

    /// The germ of the leg's crossing with the moved line `x_k = P_k + δ`
    /// (`P_k − δ` where `down`) along the other coordinate (module docs);
    /// `None` where the leg doesn't leave `P` across it (its first control
    /// point off the line from `P` on `P`'s side of the moved line, or none
    /// off it).
    pub(super) fn germ(&self, k: usize, down: bool) -> Option<Germ> {
        let o = 1 - k;
        let a = [0, 1].map(|i| {
            if down {
                -&self.d[i][k]
            } else {
                self.d[i][k].clone()
            }
        });
        let b = [self.d[0][o].clone(), self.d[1][o].clone()];
        germ_of(&a, &b, &self.w)
    }

    /// The leg's direction out of `P`: towards its first control point
    /// apart from `P`; `None` where the span is one point.
    pub(super) fn tangent(&self) -> Option<[Rational; 2]> {
        self.d.iter().find(|d| d.iter().any(|x| *x != 0)).cloned()
    }

    /// How the leg bends across its tangent `t` (to the left of it
    /// positive): the germ of its offset along `t`'s left normal against
    /// its distance along `t`, in `t`'s units.
    fn bend(&self, t: &[Rational; 2]) -> Option<Germ> {
        let along = |d: &[Rational; 2]| &d[0] * &t[0] + &d[1] * &t[1];
        let left = |d: &[Rational; 2]| &d[1] * &t[0] - &d[0] * &t[1];
        let a = [along(&self.d[0]), along(&self.d[1])];
        let b = [left(&self.d[0]), left(&self.d[1])];
        germ_of(&a, &b, &self.w)
    }
}

/// The germ of a leg whose control points are `(aᵢ, bᵢ)` from `P` (`a`
/// across the line, `b` along it), weights `w` (module docs).
fn germ_of(a: &[Rational; 2], b: &[Rational; 2], w: &[Rational; 3]) -> Option<Germ> {
    let two = Rational::from(2);
    if a[0] > 0 {
        // `Σ wᵢ·xᵢ·Bᵢ(s)` over `Σ wᵢ·Bᵢ(s)`, in power form in `s`.
        let num = |x: &[Rational; 2]| {
            let one = &two * &w[1] * &x[0];
            let sq = &w[2] * &x[1] - &one;
            vec![Rational::ZERO, one, sq]
        };
        let den = vec![
            w[0].clone(),
            &two * (&w[1] - &w[0]),
            &w[0] - &two * &w[1] + &w[2],
        ];
        let x = sdiv(&num(a), &den);
        let s = srevert(&x);
        let f = scompose(&sdiv(&num(b), &den), &s);
        return Some(Germ::Series(f[1..=ORDERS].to_vec()));
    }
    if a[0] != 0 || a[1] <= 0 {
        return None;
    }
    if b[0] == 0 {
        // A straight leg: its middle control point at `P`.
        let mut f = vec![Rational::ZERO; ORDERS];
        f[0] = &b[1] / &a[1];
        return Some(Germ::Series(f));
    }
    // Tangent to the line: `x_k ≈ (w₂a₂/w₀)·s²`, `x_o ≈ (2w₁b₁/w₀)·s`.
    let sq = Rational::from(4) * &w[1] * &w[1] * &b[0] * &b[0] / (&w[0] * &w[2] * &a[1]);
    Some(Germ::Root { up: b[0] > 0, sq })
}

/// A direction from a vertex, `(u, v·√2)`, each `−1`, `0` or `1`, not both
/// `0`: towards the moved point of a box side or corner next to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Toward {
    pub(super) u: i8,
    pub(super) v: i8,
}

/// The sign of `p + q·√2`.
fn sign_sqrt2(p: &Rational, q: &Rational) -> Ordering {
    let (sp, sq) = (p.cmp(&Rational::ZERO), q.cmp(&Rational::ZERO));
    match (sp, sq) {
        (Ordering::Equal, s) | (s, Ordering::Equal) => s,
        (a, b) if a == b => a,
        (Ordering::Greater, _) => (p * p).cmp(&(Rational::from(2) * q * q)),
        _ => (Rational::from(2) * q * q).cmp(&(p * p)),
    }
}

/// Where `t` lies turning clockwise from `d`: `0` within the half turn,
/// `1` opposite, `2` beyond; `None` along `d`.
fn half(d: Toward, t: &[Rational; 2]) -> Option<u8> {
    let (du, dv) = (
        Rational::from(i64::from(d.u)),
        Rational::from(i64::from(d.v)),
    );
    // `d × t = d_u·t_v − √2·d_v·t_u` and `d · t = d_u·t_u + √2·d_v·t_v`.
    let cross = sign_sqrt2(&(&du * &t[1]), &-(&dv * &t[0]));
    let dot = sign_sqrt2(&(&du * &t[0]), &(&dv * &t[1]));
    match (cross, dot) {
        (Ordering::Less, _) => Some(0),
        (Ordering::Greater, _) => Some(2),
        (_, Ordering::Less) => Some(1),
        _ => None,
    }
}

/// Whether the point infinitesimally near a vertex in direction `d` is
/// inside the face (module docs, "Sectors"): `legs` every curve at the
/// vertex read from it, each with whether its coedge leaves the vertex,
/// `w` the face's inside winding. `None` where a curve leaves along `d`,
/// two can't be told apart, or there is none.
pub(super) fn inside_toward(legs: &[(Leg, bool)], d: Toward, w: i64) -> Option<bool> {
    let mut best: Option<(u8, [Rational; 2], usize)> = None;
    let mut tied = false;
    for (i, (leg, _)) in legs.iter().enumerate() {
        let t = leg.tangent()?;
        let h = half(d, &t)?;
        let Some((bh, bt, bi)) = &best else {
            best = Some((h, t, i));
            continue;
        };
        // First turning clockwise from `d`: the lower half, then within
        // a half the one counter-clockwise of the other; along one
        // direction, the one bending to the left (counter-clockwise)
        // first.
        let o = h.cmp(bh).then_with(|| {
            let cross = &bt[0] * &t[1] - &bt[1] * &t[0];
            Rational::ZERO.cmp(&cross)
        });
        let o = match o {
            Ordering::Equal => {
                let (Some(a), Some(b)) = (leg.bend(&t), legs[*bi].0.bend(&t)) else {
                    return None;
                };
                match a.cmp(&b) {
                    // Bending further left: met first.
                    Some(x) => x.reverse(),
                    None => {
                        tied = true;
                        continue;
                    }
                }
            }
            x => x,
        };
        if o == Ordering::Less {
            best = Some((h, t, i));
            tied = false;
        }
    }
    let (_, _, i) = best?;
    if tied {
        return None;
    }
    Some(legs[i].1 == (w > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(p: [[f64; 2]; 3], w: [f64; 3]) -> UvSpan {
        [0, 1, 2].map(|i| [p[i][0], p[i][1], w[i]])
    }

    fn q(n: i64, d: i64) -> Rational {
        Rational::from(n) / Rational::from(d)
    }

    const U: Toward = Toward { u: 1, v: 0 };
    const V: Toward = Toward { u: 0, v: 1 };
    const CORNER: Toward = Toward { u: 1, v: 1 };

    #[test]
    fn a_straight_leg_has_its_slope() {
        let s = span([[0.0, 0.0], [0.5, 0.25], [1.0, 0.5]], [1.0, 1.0, 1.0]);
        let g = Leg::new(&s, false).germ(0, false).unwrap();
        let Germ::Series(f) = &g else { panic!("{g:?}") };
        assert_eq!(f[0], q(1, 2));
        assert!(f[1..].iter().all(|x| *x == 0), "{f:?}");
        // Slope ½ against √2 on a `u` line, `1/√2` read across a `v` line
        // (slope 2 there).
        assert_eq!(g.against(0, 1), Ordering::Less);
        let g = Leg::new(&s, false).germ(1, false).unwrap();
        assert_eq!(g.against(1, 1), Ordering::Greater);
        // Read from its end, it leaves downwards: no crossing, but of a
        // line moved down (a low edge), where the end, moved down too,
        // lies below the crossing.
        assert!(Leg::new(&s, true).germ(0, false).is_none());
        let g = Leg::new(&s, true).germ(0, true).unwrap();
        assert_eq!(g, Germ::Series(vec![q(-1, 2), q(0, 1), q(0, 1), q(0, 1)]));
        assert_eq!(g.against(0, -1), Ordering::Greater);
        assert_eq!(Germ::Series(vec![q(-3, 2)]).against(0, -1), Ordering::Less);
    }

    #[test]
    fn germs_order_by_slope_then_bend() {
        // A line along `+u` and an arc leaving tangent to it, bending up:
        // crossing `u = δ`, the arc lies above the line.
        let line = span([[0.0, 0.0], [0.5, 0.0], [1.0, 0.0]], [1.0, 1.0, 1.0]);
        let arc = span([[0.0, 0.0], [0.5, 0.0], [1.0, 1.0]], [1.0, 0.7, 1.0]);
        let (l, a) = (
            Leg::new(&line, false).germ(0, false).unwrap(),
            Leg::new(&arc, false).germ(0, false).unwrap(),
        );
        assert_eq!(l.cmp(&a), Some(Ordering::Less));
        assert_eq!(l.against(0, 1), Ordering::Less);
        assert_eq!(a.against(0, 1), Ordering::Less);
        // Crossing `v = δ`, the arc is tangent there: `√δ` far along `+u`.
        assert!(Leg::new(&line, false).germ(1, false).is_none());
        let g = Leg::new(&arc, false).germ(1, false).unwrap();
        assert!(matches!(g, Germ::Root { up: true, .. }), "{g:?}");
        assert_eq!(g.against(1, 1), Ordering::Greater);
        // One curve against itself: not told apart.
        assert_eq!(a.cmp(&a.clone()), None);
    }

    #[test]
    fn sectors_at_a_cusp() {
        // The face between a line out of the vertex along `+u` and an
        // arc above it back into the vertex: the spike only.
        let line = span([[0.0, 0.0], [0.5, 0.0], [1.0, 0.0]], [1.0, 1.0, 1.0]);
        let arc = span([[1.0, 1.0], [0.5, 0.0], [0.0, 0.0]], [1.0, 0.7, 1.0]);
        let legs = [
            (Leg::new(&line, false), true),
            (Leg::new(&arc, true), false),
        ];
        for d in [CORNER, V] {
            assert_eq!(inside_toward(&legs, d, 1), Some(false), "{d:?}");
            assert_eq!(inside_toward(&legs, d, -1), Some(true), "{d:?}");
        }
        // Along a curve's own direction: not decided.
        assert_eq!(inside_toward(&legs, U, 1), None);
        // The loop run the other way round: everything but the spike.
        let rev = [
            (Leg::new(&line, false), false),
            (Leg::new(&arc, true), true),
        ];
        assert_eq!(inside_toward(&rev, CORNER, 1), Some(true));
    }

    #[test]
    fn sectors_at_a_right_angle() {
        // Out along `−u`, in from `−v`: on an unflipped face the face is
        // the quadrant `u < 0, v < 0`, on the coedges' left.
        let out = span([[0.0, 0.0], [-0.5, 0.0], [-1.0, 0.0]], [1.0, 1.0, 1.0]);
        let inn = span([[0.0, -1.0], [0.0, -0.5], [0.0, 0.0]], [1.0, 1.0, 1.0]);
        let legs = [(Leg::new(&out, false), true), (Leg::new(&inn, true), false)];
        for d in [U, V, CORNER] {
            assert_eq!(inside_toward(&legs, d, 1), Some(false), "{d:?}");
        }
        // In along `+u`, out along `+v` (a left turn): the face is the
        // quadrant `u < 0, v > 0`; towards the moved corner both curves
        // lie beyond the half turn, the incoming one met first.
        let inn = span([[-1.0, 0.0], [-0.5, 0.0], [0.0, 0.0]], [1.0, 1.0, 1.0]);
        let out = span([[0.0, 0.0], [0.0, 0.5], [0.0, 1.0]], [1.0, 1.0, 1.0]);
        let legs = [(Leg::new(&inn, true), false), (Leg::new(&out, false), true)];
        let rev = [legs[1].clone(), legs[0].clone()];
        for d in [U, CORNER] {
            assert_eq!(inside_toward(&legs, d, 1), Some(false), "{d:?}");
            assert_eq!(inside_toward(&rev, d, 1), Some(false), "{d:?}");
        }
        assert_eq!(sign_sqrt2(&q(3, 2), &q(-1, 1)), Ordering::Greater);
        assert_eq!(sign_sqrt2(&q(1, 1), &q(-1, 1)), Ordering::Less);
    }
}
