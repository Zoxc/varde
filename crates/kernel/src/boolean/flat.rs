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
}

impl<'a> Flat<'a> {
    /// `grow`: whether `A` grows (a union) or shrinks.
    pub(super) fn new(a: &'a Input<'a>, b: &'a Input<'a>, grow: bool) -> Flat<'a> {
        let s = if grow { 1.0 } else { -1.0 };
        Flat {
            a,
            b,
            perturb: a.vertex_normals().into_iter().map(|n| n * s).collect(),
        }
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
        let facing = exact::sign(&Orient {
            p: t[0],
            q: t[1],
            r: t[2],
        });
        // The ray `x + s·UP` meets the plane at `s = (t0 − x)·n / UP·n`,
        // and `UP·n` has the sign `facing`.
        (facing != 0).then(|| exact::sign(&Reach { x0: x, t }) == facing)
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
        let h = match exact::sign(&Height { a, b, c, d }) {
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
        let facing = exact::sign(&Orient {
            p: t[0],
            q: t[1],
            r: t[2],
        });
        if facing == 0 {
            // A face along the projection: no ray from a vertex lies in it.
            return 0;
        }
        for i in 0..3 {
            let o = exact::sign(&Orient {
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
        if exact::sign(&Reach { x0: x, t }) != facing {
            return 0;
        }
        facing
    }

    fn s11(&self, e: u32, g: u32) -> Cross11 {
        let [a, b] = self.edge(Side::A, e);
        let [c, d] = self.edge(Side::B, g);
        let oc = exact::sign(&Orient { p: a, q: b, r: c });
        let od = exact::sign(&Orient { p: a, q: b, r: d });
        if oc == od || oc == 0 || od == 0 {
            return Cross11::default();
        }
        let oa = exact::sign(&Orient { p: c, q: d, r: a });
        let ob = exact::sign(&Orient { p: c, q: d, r: b });
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
            -1 | 1 => Ok((vec![(x as i8, self.crossing(side, e, f))], 1)),
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
