//! The primitives for flat patches: exact, with ties broken by symbolic
//! perturbation.
//!
//! Every tie between the operands (a vertex of one on a face of the
//! other, edges meeting, faces flush) is broken by moving each vertex of
//! `A` by `ε·s·n_v + ε²·T2 + ε³·T3` for an infinitely small `ε` (see
//! [`super::exact`]): `n_v` the vertex's direction out of `A`, `s`
//! +1 for a union (`A` grows a little, so flush faces overlap and merge)
//! and −1 otherwise (`A` shrinks, so flush faces cut cleanly). `B` stays.
//! The perturbed operands are a real configuration in general position,
//! so every decision below is true of it, and the counting identities
//! hold without exception. The positions returned are those of the
//! unperturbed operands.
//!
//! Configurations within a tie distance of a tie (faces flush in exact
//! arithmetic, turned and moved so every coordinate rounds) are decided
//! as the ties they stand for ([`exact::sign_tied`]), as the curved
//! primitives decide heights that close: the decisions are then those of
//! the operands moved by less than that distance, and in the rare case
//! they don't fit one configuration, the boolean decides them again
//! exactly (`tie` 0), never giving a wrong result.

use std::cmp::Ordering;

use glam::DVec3;

use super::exact::{self, Num, Pred, Pt, V3, cross, det, dir, dot, sub};
use super::input::{Input, Side};
use super::{BooleanError, Cross11, Crossing, Found, Primitives, UP};

/// The primitives of two flat operands.
pub(super) struct Flat<'a> {
    a: &'a Input<'a>,
    b: &'a Input<'a>,
    /// Each vertex of `A`'s first perturbation, `s·n_v`.
    perturb: Vec<DVec3>,
    /// Distances this close to a tie are decided as it
    /// ([`exact::sign_tied`]); 0 decides exactly.
    tie: f64,
}

impl<'a> Flat<'a> {
    /// `grow`: whether `A` grows (a union) or shrinks; configurations
    /// within `tie` of a tie are decided as the tie (0: exactly).
    pub(super) fn tied(a: &'a Input<'a>, b: &'a Input<'a>, grow: bool, tie: f64) -> Flat<'a> {
        let s = if grow { 1.0 } else { -1.0 };
        Flat {
            a,
            b,
            perturb: a.vertex_normals().into_iter().map(|n| n * s).collect(),
            tie,
        }
    }

    /// The sign of a deciding predicate, ties as [`Self::tie`] says.
    fn sign(&self, pred: &impl Pred) -> i8 {
        exact::sign_tied(pred, self.tie)
    }

    fn input(&self, side: Side) -> &Input<'a> {
        match side {
            Side::A => self.a,
            Side::B => self.b,
        }
    }

    /// Vertex `v` of `side` as an exact point, perturbed if it is `A`'s.
    pub(super) fn pt(&self, side: Side, v: u32) -> Pt {
        let p = self.input(side).pos(v);
        match side {
            Side::A => Pt {
                p,
                n: Some(self.perturb[v as usize]),
            },
            Side::B => Pt { p, n: None },
        }
    }

    /// Vertex `v` of `A`'s first perturbation, `s·n_v`.
    pub(super) fn perturb(&self, v: u32) -> DVec3 {
        self.perturb[v as usize]
    }

    fn tri(&self, side: Side, t: u32) -> [Pt; 3] {
        self.input(side).tris[t as usize].map(|v| self.pt(side, v))
    }

    fn edge(&self, side: Side, e: u32) -> [Pt; 2] {
        self.input(side).edges[e as usize].map(|v| self.pt(side, v))
    }

    /// Whether the plane of the corners of face `f` of the other operand
    /// is above vertex `v` of `side` along [`UP`], or `None` if the plane
    /// is along `UP`.
    pub(super) fn plane_above(&self, side: Side, v: u32, f: u32) -> Option<bool> {
        let x = self.pt(side, v);
        let t = self.tri(side.other(), f);
        let facing = self.sign(&Orient {
            p: t[0],
            q: t[1],
            r: t[2],
        });
        // The ray `x + s·UP` meets the plane at `s = (t0 − x)·n / UP·n`,
        // and `UP·n` has the sign `facing`.
        (facing != 0).then(|| self.sign(&Reach { x0: x, t }) == facing)
    }

    /// Whether edge `e` of `A` is above edge `g` of `B` where the lines
    /// through them cross seen along [`UP`], the one running from its
    /// right to its left over the other being `sigma` (+1 for `e`, as in
    /// [`Cross11`]).
    pub(super) fn e_above(&self, e: u32, g: u32, sigma: i8) -> bool {
        let [a, b] = self.edge(Side::A, e);
        let [c, d] = self.edge(Side::B, g);
        // The point of `e` is `λ` above that of `g` along `UP`, with
        // `λ = det[a − c, g, e] / det[g, e, UP]`.
        let h = match self.sign(&Height { a, b, c, d }) {
            0 => 1,
            h => h,
        };
        h * sigma > 0
    }

    /// Where along edge `e` of `side` (0 at its start, 1 at its end) the
    /// line through it meets the plane of the corners of face `f` of the
    /// other: only the position, never a decision.
    pub(super) fn crossing(&self, side: Side, e: u32, f: u32) -> f64 {
        let [x0, x1] = self.edge(side, e);
        let t = self.tri(side.other(), f);
        let at = exact::ratio(&Reach { x0, t }, &Across { x0, x1, t });
        if at.is_finite() {
            at.clamp(0.0, 1.0)
        } else {
            0.5
        }
    }

    /// The order along edge `e` of `side`, in its direction, of where its
    /// line meets the planes of the corners of faces `f1` and `f2` of the
    /// other, ties by face.
    pub(super) fn order_faces(&self, side: Side, e: u32, f1: u32, f2: u32) -> Ordering {
        let [x0, x1] = self.edge(side, e);
        let (t1, t2) = (self.tri(side.other(), f1), self.tri(side.other(), f2));
        let d1 = exact::sign(&Across { x0, x1, t: t1 });
        let d2 = exact::sign(&Across { x0, x1, t: t2 });
        let diff = exact::sign(&Between { x0, x1, t1, t2 }) * d1 * d2;
        match diff {
            -1 => Ordering::Less,
            1 => Ordering::Greater,
            _ => f1.cmp(&f2),
        }
    }
}

impl Primitives for Flat<'_> {
    fn s02(&self, side: Side, v: u32, f: u32) -> i8 {
        let x = self.pt(side, v);
        let t = self.tri(side.other(), f);
        let facing = self.sign(&Orient {
            p: t[0],
            q: t[1],
            r: t[2],
        });
        if facing == 0 {
            // A face along the projection: no ray from a vertex lies in it.
            return 0;
        }
        for i in 0..3 {
            let o = self.sign(&Orient {
                p: t[i],
                q: t[(i + 1) % 3],
                r: x,
            });
            if o != facing {
                return 0;
            }
        }
        // The ray `x + s·UP` meets the plane at `s = (t0 − x)·n / UP·n`,
        // and `UP·n` has the sign `facing`.
        if self.sign(&Reach { x0: x, t }) != facing {
            return 0;
        }
        facing
    }

    fn s11(&self, e: u32, g: u32) -> Cross11 {
        let [a, b] = self.edge(Side::A, e);
        let [c, d] = self.edge(Side::B, g);
        let oc = self.sign(&Orient { p: a, q: b, r: c });
        let od = self.sign(&Orient { p: a, q: b, r: d });
        if oc == od || oc == 0 || od == 0 {
            return Cross11::default();
        }
        let oa = self.sign(&Orient { p: c, q: d, r: a });
        let ob = self.sign(&Orient { p: c, q: d, r: b });
        if oa == ob || oa == 0 || ob == 0 {
            return Cross11::default();
        }
        // `det[g, e, UP]` for the directions `g = d − c` and `e = b − a`:
        // with `a` and `b` on opposite sides of `g`, the sign of `b`'s.
        let sigma = ob;
        // `g` crosses `e` from its right to its left where `e` crosses
        // `g` the other way.
        if self.e_above(e, g, sigma) {
            Cross11 {
                a_under: 0,
                b_under: -sigma,
            }
        } else {
            Cross11 {
                a_under: sigma,
                b_under: 0,
            }
        }
    }

    fn searches(&self, _: Side, _: u32, _: u32) -> bool {
        // A segment meets a triangle once at most.
        false
    }

    fn search_work(&self) -> usize {
        1
    }

    fn margin(&self) -> f64 {
        0.0
    }

    fn crossings(&self, side: Side, e: u32, f: u32, x: i32) -> Result<Found, BooleanError> {
        match x {
            0 => Ok((Vec::new(), 1)),
            -1 | 1 => Ok((vec![(x as i8, self.crossing(side, e, f), true)], 1)),
            // A straight edge meets a flat face once at most.
            _ => Err(BooleanError::Inconsistent),
        }
    }

    fn order(&self, side: Side, e: u32, c1: &Crossing, c2: &Crossing) -> Ordering {
        self.order_faces(side, e, c1.face, c2.face)
    }
}

/// `det[q − p, r − p, UP]`: positive when `r` is left of `p → q` seen
/// from `+UP`.
struct Orient {
    p: Pt,
    q: Pt,
    r: Pt,
}

impl Pred for Orient {
    fn eval<N: Num>(&self) -> N {
        let p = self.p.v3::<N>();
        det(&sub(&self.q.v3(), &p), &sub(&self.r.v3(), &p), &dir(UP))
    }

    fn scale(&self) -> f64 {
        // `|UP × (q − p)|` times how far `r` is from the plane through
        // `p → q` along `UP`: where its shadow is beside the line's.
        UP.cross(self.q.p - self.p.p).length()
    }
}

/// The normal `(t1 − t0) × (t2 − t0)` of a triangle.
fn normal<N: Num>(t: &[Pt; 3]) -> V3<N> {
    let t0 = t[0].v3::<N>();
    cross(&sub(&t[1].v3(), &t0), &sub(&t[2].v3(), &t0))
}

/// `det[a − c, d − c, b − a]`.
struct Height {
    a: Pt,
    b: Pt,
    c: Pt,
    d: Pt,
}

impl Pred for Height {
    fn eval<N: Num>(&self) -> N {
        let (a, c) = (self.a.v3::<N>(), self.c.v3::<N>());
        det(&sub(&a, &c), &sub(&self.d.v3(), &c), &sub(&self.b.v3(), &a))
    }

    fn scale(&self) -> f64 {
        // `λ·det[g, e, UP]`, the height `λ·|UP|`; but never below that
        // product's own rounding. Where the edges' shadows are parallel to
        // rounding (collinear edges, turned), `det[g, e, UP]` is only
        // rounding, down to an exact 0, and so is the constant term
        // (`Height` is of second order there): taken as it came, the
        // rounding in it decided. The floor swallows no real height:
        // the value is the lines' distance times `|g × e|`, so a value
        // within `tie·4ε·|g|·|e|` is lines within the tie of each other,
        // or edges parallel to rounding in space too, whose constant term
        // is zero at the exact parallel tie, whatever their distance.
        let (g, e) = (self.d.p - self.c.p, self.b.p - self.a.p);
        let rounding = 4.0 * f64::EPSILON * g.length() * e.length();
        (g.cross(e).dot(UP).abs() / UP.length()).max(rounding)
    }
}

/// `(t0 − x0)·n`: positive when `x0` is behind the triangle's plane, and
/// the numerator of the parameter where the line through `x0` and `x1`
/// meets it.
struct Reach {
    x0: Pt,
    t: [Pt; 3],
}

impl Pred for Reach {
    fn eval<N: Num>(&self) -> N {
        dot(&sub(&self.t[0].v3(), &self.x0.v3()), &normal(&self.t))
    }

    fn scale(&self) -> f64 {
        let [t0, t1, t2] = self.t.map(|t| t.p);
        (t1 - t0).cross(t2 - t0).length()
    }
}

/// `(x1 − x0)·n`: its denominator.
struct Across {
    x0: Pt,
    x1: Pt,
    t: [Pt; 3],
}

impl Pred for Across {
    fn eval<N: Num>(&self) -> N {
        dot(&sub(&self.x1.v3(), &self.x0.v3()), &normal(&self.t))
    }
}

/// `num1·den2 − num2·den1`, whose sign times the denominators' orders the
/// parameters where the edge meets the two planes.
struct Between {
    x0: Pt,
    x1: Pt,
    t1: [Pt; 3],
    t2: [Pt; 3],
}

impl Pred for Between {
    fn eval<N: Num>(&self) -> N {
        let (x0, x1) = (self.x0, self.x1);
        let num = |t: [Pt; 3]| Reach { x0, t }.eval::<N>();
        let den = |t: [Pt; 3]| Across { x0, x1, t }.eval::<N>();
        num(self.t1)
            .mul(&den(self.t2))
            .sub(&num(self.t2).mul(&den(self.t1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rng::Rng;

    /// `x` moved by up to four units in its last place, as rounding
    /// would; a zero (a unit vector's component) by as much as one near 1.
    fn rounded(rng: &mut Rng, x: f64) -> f64 {
        let k = (rng.unit() * 9.0) as i64 - 4;
        if x == 0.0 {
            k as f64 * f64::EPSILON / 2.0
        } else {
            f64::from_bits(x.to_bits().wrapping_add_signed(k))
        }
    }

    fn rounded_pt(rng: &mut Rng, pt: Pt) -> Pt {
        let mut v = |d: DVec3| DVec3::new(rounded(rng, d.x), rounded(rng, d.y), rounded(rng, d.z));
        Pt {
            p: v(pt.p),
            n: pt.n.map(v),
        }
    }

    /// A point of `A`, moved along `n` by the perturbation.
    fn a(p: [f64; 3], n: [f64; 3]) -> Pt {
        Pt {
            p: DVec3::from(p),
            n: Some(DVec3::from(n).normalize()),
        }
    }

    /// A point of `B`.
    fn b(p: [f64; 3]) -> Pt {
        Pt {
            p: DVec3::from(p),
            n: None,
        }
    }

    #[test]
    fn rounded_ties_go_as_the_exact_ones_at_every_order() {
        // Exact ties whose first order is zero too, every coordinate and
        // direction then moved by rounding: each must be decided as the
        // exact tie, in every draw, not by the rounding left in its first
        // order.
        let tie = 1e-9;
        // Two collinear edges, `A`'s ends moving different ways: both of
        // `Height`'s first-order terms have parallel columns, the second
        // order decides.
        let height = |f: &mut dyn FnMut(Pt) -> Pt| Height {
            a: f(a([3.25, 1.5, 7.75], [-1.0, 1.0, 2.0])),
            b: f(a([5.25, 2.5, 8.25], [1.0, 2.0, 1.0])),
            c: f(b([4.25, 2.0, 8.0])),
            d: f(b([6.25, 3.0, 8.5])),
        };
        // A vertex on a face's plane whose direction lies in it (a vertex
        // on the edge between a `+x` and a `+y` face, against a `z`
        // face): `Reach`'s first order is zero, `T2` decides.
        let reach = |f: &mut dyn FnMut(Pt) -> Pt| Reach {
            x0: f(a([3.25, 1.5, 7.75], [1.0, 1.0, 0.0])),
            t: [
                f(b([2.0, 1.0, 7.75])),
                f(b([6.0, 1.0, 7.75])),
                f(b([2.0, 4.5, 7.75])),
            ],
        };
        let want_height = exact::sign(&height(&mut |p| p));
        let want_reach = exact::sign(&reach(&mut |p| p));
        assert_ne!(want_height, 0);
        assert_ne!(want_reach, 0);
        let mut rng = Rng::new(39);
        let mut unscaled = 0;
        for draw in 0..2000 {
            let h = height(&mut |p| rounded_pt(&mut rng, p));
            let r = reach(&mut |p| rounded_pt(&mut rng, p));
            // Where rounding leaves the edges' shadows exactly parallel,
            // `det[g, e, UP]` is 0, and `Height`'s scale is that
            // product's rounding.
            let (g, e) = (h.d.p - h.c.p, h.b.p - h.a.p);
            if g.cross(e).dot(UP) == 0.0 {
                unscaled += 1;
            }
            assert_eq!(
                exact::sign_tied(&h, tie),
                want_height,
                "height, draw {draw}"
            );
            assert_eq!(exact::sign_tied(&r, tie), want_reach, "reach, draw {draw}");
        }
        assert!(unscaled > 0);
    }

    #[test]
    fn real_first_orders_decide_far_from_the_origin() {
        // A vertex exactly on a face's plane, its direction well into the
        // face, `2¹⁷` from the origin: its first order is real (the vertex
        // moves off the face) and decides, though it is far below its
        // terms' absolute values (`|x|²`), which measured against them
        // took it as rounding and let `T2` decide the other way. What
        // rounding there moves it by is some `|x|·1e-16`.
        let x = f64::from(1 << 17);
        let z = x + 0.75;
        let reach = Reach {
            x0: a([x + 0.25, x + 0.25, z], [0.3, 0.0, -0.95]),
            t: [b([x, x, z]), b([x + 1.0, x, z]), b([x, x + 1.0, z])],
        };
        assert_eq!(exact::sign(&reach), 1);
        for tie in [1e-8, 1e-10, 1e-12] {
            assert_eq!(exact::sign_tied(&reach, tie), 1, "{tie}");
        }
        // And two edges one above the other, their shadows collinear:
        // `Height`'s first order (with the gap `D`) is real, the second
        // order doesn't know which is above.
        for gap in [1e-3, 1e-6] {
            let up = UP.normalize();
            let height = Height {
                a: a([x, x, z], [1.0, 2.0, -1.0]),
                b: a([x + 1.0, x, z], [-1.0, 1.0, 2.0]),
                c: b((DVec3::new(x, x, z) + up * gap).into()),
                d: b((DVec3::new(x + 1.0, x, z) + up * gap).into()),
            };
            let want = exact::sign(&height);
            assert_ne!(want, 0);
            let flipped = Height {
                c: b((DVec3::new(x, x, z) - up * gap).into()),
                d: b((DVec3::new(x + 1.0, x, z) - up * gap).into()),
                ..height
            };
            assert_eq!(exact::sign(&flipped), -want, "{gap}");
            for tie in [1e-8, 1e-10] {
                assert_eq!(exact::sign_tied(&height, tie), want, "{gap} {tie}");
                assert_eq!(exact::sign_tied(&flipped, tie), -want, "{gap} {tie}");
            }
        }
    }
}
