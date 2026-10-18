//! The perturbation rows: for each kind of record, the derivative of its
//! value along each motion of the moving operand. This is the only place
//! they are derived; [`decide`] reads them.
//!
//! **The motion.** Only operand `A` moves; `B` is fixed at every order.
//! The moved configuration is `A` moved by `δ·(σ_A·d + η·T₂ + η²·T₃ +
//! η³·T₄)` for an infinitely small `η` and a `δ` infinitely smaller
//! still, to first order in `δ`: `σ_A = +1` for a union, `−1` for an
//! intersection or difference;
//! `d` the direction field on `A` (the moved-vertex blend of the vertex,
//! edge and face directions); `T₂, T₃, T₄` ([`T`]) fixed, generic,
//! linearly independent translations of all of `A`. A record's value `g`
//! then moves as `g + δ·(g₁ + η·g₂ + η²·g₃ + η³·g₄)`, `gₖ` its derivative
//! along motion `k` (motion 1 `σ_A·d`, motion `k ≥ 2` `Tₖ`), and its sign
//! is that of the first of `g, g₁, …, g₄` that isn't zero: the exact sign
//! of the one moved configuration, for every record whose row is its
//! derivative, so all the answers are realisable together.
//!
//! **Rates.** Every point carries its rates along the four motions:
//! `[σ_A·d(x), T₂, T₃, T₄]` on `A` ([`rates_a`]), zero on `B`
//! ([`Moving::fixed`]).
//! They are values of one field, so a point's rates are the same in every
//! record that reads it. A point a record derives (the foot of a vertex's
//! shadow on a curve, the crossing of two shadows, the hit of a column on
//! a face) is no input of its own: it is read through the element it lies
//! on, which moves at the field's value at that point. At a tie only that
//! value enters the first-order value (the element's turning multiplies
//! the value, which is zero), so a straight segment in a record moves as a
//! translation at its rate at the record's point ([`Seg`]), and a curve,
//! a face or a plane by its rate at the record's point (the foot, the
//! crossing, the anchor). Away from a tie the orders aren't read. A rate
//! is a [`Rate`]: `f64` components, or exact rationals, as a flat
//! element's direction is ([`dir`]: equal offsets or the max–min
//! direction, and their blends, exact from the stored normals, of any
//! size near a knife edge). The rows read exact rates as they are,
//! never rounded; a record's one moving element may carry its rates
//! times a positive factor per motion ([`Rate::cleared`], denominators
//! cleared for the expansions), which keeps every sign.
//!
//! **The frame.** One projection [`UP`] (Varde's `(2, 3, 32)`) and one
//! shadow-plane direction [`R_HAT`] `⊥ UP`, with [`NU`] `= UP × R_HAT` the
//! normal of the `R_HAT` line in the shadow plane. A vector `⊥ UP` dotted
//! with a difference gives the shadow-plane component of the difference,
//! so the shadow rows need no projection. Flat rows use these exact,
//! unnormalised; curved ones normalise them, so their values are lengths.
//!
//! | row (record) | `g` | `gₖ` |
//! |---|---|---|
//! | vertex–vertex shadow ([`VvShadow`], [`HomShadow`], [`vv_shadow`]) | `ν·(w − q)`, ahead/behind `r̂·(w − q)` | `ν·(Δw − Δq)`, likewise `r̂` |
//! | vertex–curve shadow, box side ([`LineShadow`], [`HomLineShadow`], [`curve_shadow`]) | `ν·(q − C(s*))`, `ν = ê × T` at the foot | `ν·(Δq − ΔC(s*))` |
//! | column hit on a tile, germ keys, anchor `f` ([`PlaneHeight`], [`tangent_plane_row`]) | from the record's height key (the hit's own, [`crate::records::height::hit`]; [`PointHeight`], [`face_height`]) | `(n_f·Δf − n_f·Δq)·sign(n_f·ê)`: the tangent plane of `f` at the anchor |
//! | column hit on a ribbon, anchor `f` ([`ribbon_row`], [`ribbon_row_signs`]) | the hit's height, a sign condition at a root ([`crate::records::height::hit`]) | `(n_R·ΔR − n_R·Δq)·sign(n_R·ê)`, `n_R = R_t × R_λ`: the tangent plane of the ribbon at the hit |
//! | edge–edge z-order, anchor `g` ([`EdgeEdge`], [`z_order`]) | `m·(C_g(s) − C_e(t))`, `m ∥ T_e × T_g`, `m·ê > 0` | `m·(Δg − Δe)` |
//!
//! The height and z-order rows are per anchor: their order 0 is the value
//! of the record's height key, decided once for every anchor reading it,
//! their orders this anchor's own ([`decide_flat_orders`]).
//!
//! **Flat rows** (lines and planes) are exact predicates ([`Pred`]) over
//! stored coordinates and rates, their orders the derivatives
//! [`crate::num::exact`] works out, decided under [`Band::FLAT`]
//! ([`decide_flat`]). Each is a positive multiple of the spatial value in
//! the table (by a squared length or facing of its unmoved data), which
//! keeps every sign. **Curved rows** are evaluated at a precision level
//! ([`Curved`]) on the record's point and the tangent or normal there
//! ([`curve_point`], [`patch_point`]), as lengths with certified errors
//! ([`Coeffs`]).

use std::cmp::Ordering;
use std::sync::Arc;

use glam::DVec3;
use malachite::base::num::arithmetic::traits::Lcm;
use malachite::{Natural, Rational};

use crate::bezier::{HomCurve, HomPatch, Scalar};
use crate::num::bigf::BigF;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::{Band, Fail, Sign, TieKey, decide, exact_orders};
use crate::num::exact::{
    self, ExactSign, Num, ORDERS, Pred, V3, Work, cross, dot, moved_point, point, rat, sub,
};
use crate::num::poly::{self, Poly, Root};

pub mod dir;

/// The projection direction, Varde's `(2, 3, 32)`, unnormalised: heights
/// are measured along it, shadows taken across it.
pub const UP: DVec3 = DVec3::new(2.0, 3.0, 32.0);

/// The shadow-plane direction `r̂`, `(16, 0, −1)`, exactly `⊥` [`UP`],
/// unnormalised: rays from `A` points go `−r̂`, from `B` points `+r̂`.
pub const R_HAT: DVec3 = DVec3::new(16.0, 0.0, -1.0);

/// `UP × R_HAT`, exactly: the normal `ν` of an `r̂` line in the shadow
/// plane, to its left seen from above.
pub const NU: DVec3 = DVec3::new(-3.0, 514.0, -48.0);

/// `T₂, T₃, T₄`: the fixed generic translations of motions 2 to 4,
/// `(3, 7, 11)/16`, `(−5, 2, 13)/16`, `(7, −11, 2)/16`: exact, of length
/// about 1, linearly independent and tilted off [`UP`], [`R_HAT`],
/// [`NU`] and the axes, so no record's gradient vanishes on all three.
pub const T: [DVec3; 3] = [
    DVec3::new(3.0 / 16.0, 7.0 / 16.0, 11.0 / 16.0),
    DVec3::new(-5.0 / 16.0, 2.0 / 16.0, 13.0 / 16.0),
    DVec3::new(7.0 / 16.0, -11.0 / 16.0, 2.0 / 16.0),
];

/// A point's rate along one motion.
#[derive(Clone, Debug, PartialEq)]
pub enum Rate {
    /// These components, each finite and at most
    /// [`MAX_INPUT`](exact::MAX_INPUT).
    Float(DVec3),
    /// These exact components, of any size: a flat element's direction
    /// ([`dir`]).
    Exact(Arc<[Rational; 3]>),
}

impl Rate {
    /// No motion.
    pub const ZERO: Rate = Rate::Float(DVec3::ZERO);

    /// Every motion's rate as given.
    pub fn floats(rates: [DVec3; ORDERS]) -> [Rate; ORDERS] {
        rates.map(Rate::Float)
    }

    /// The exact vector `d`.
    pub fn exact(d: [Rational; 3]) -> Rate {
        Rate::Exact(Arc::new(d))
    }

    /// `d` times the least common multiple of its denominators: a
    /// positive multiple with integer components, which the expansions
    /// take exactly where `d`'s fractions would need the rational stage.
    /// For the one moving element of a record (records compare an element
    /// of `A` with one of `B`): its rates along a motion may carry any
    /// positive factor without changing a sign.
    pub fn cleared(d: [Rational; 3]) -> Rate {
        let l = d
            .iter()
            .fold(Natural::from(1u32), |l, x| (&l).lcm(x.denominator_ref()));
        let l = Rational::from(l);
        Rate::exact(d.map(|x| x * &l))
    }

    /// `−self`.
    pub fn neg(&self) -> Rate {
        match self {
            Rate::Float(v) => Rate::Float(-*v),
            Rate::Exact(d) => Rate::exact(d.each_ref().map(|x| -x)),
        }
    }

    /// Component `i` as an input.
    fn coord<N: Num>(&self, i: usize) -> N {
        match self {
            Rate::Float(v) => N::lit(v[i]),
            Rate::Exact(d) => N::rational(&d[i]),
        }
    }

    /// The vector as inputs.
    fn vec<N: Num>(&self) -> V3<N> {
        [0, 1, 2].map(|i| self.coord(i))
    }

    /// The vector at a precision level, certified (exact where the level
    /// is).
    fn at<S: Scalar>(&self, like: &S) -> [S; 3] {
        match self {
            Rate::Float(v) => lit3(*v, like),
            Rate::Exact(d) => d.each_ref().map(|x| rational_at(x, like)),
        }
    }
}

impl From<DVec3> for Rate {
    fn from(v: DVec3) -> Rate {
        Rate::Float(v)
    }
}

/// The rates of a point of `A` along the motions: `σ_A·d`, then
/// `T₂, T₃, T₄`. `d` is the direction field at the point (zero where
/// the generic translations alone decide).
pub fn rates_a(sigma_a: Sign, d: impl Into<Rate>) -> [Rate; ORDERS] {
    let d = d.into();
    let own = match sigma_a {
        Sign::Pos => d,
        Sign::Neg => d.neg(),
    };
    [own, T[0].into(), T[1].into(), T[2].into()]
}

/// `p` moved at `rates` (`rates[k − 1]` along motion `k`).
fn moved<N: Num>(p: DVec3, rates: &[Rate; ORDERS]) -> V3<N> {
    let floats = |r: &Rate| match r {
        Rate::Float(v) => Some(*v),
        Rate::Exact(_) => None,
    };
    if let [Some(a), Some(b), Some(c), Some(d)] = rates.each_ref().map(floats) {
        return moved_point(p, [a, b, c, d]);
    }
    [0, 1, 2].map(|i| N::lit(p[i]).moved_by(&rates.each_ref().map(|r| r.coord(i))))
}

/// A point with its rates along the motions (`rates[k − 1]` along motion
/// `k`), each coordinate finite and at most
/// [`MAX_INPUT`](exact::MAX_INPUT).
#[derive(Clone, Debug, PartialEq)]
pub struct Moving {
    /// The point.
    pub at: DVec3,
    /// Its rates.
    pub rates: [Rate; ORDERS],
}

impl Moving {
    /// A point of `B`: it never moves.
    pub const fn fixed(at: DVec3) -> Moving {
        Moving {
            at,
            rates: [Rate::ZERO; ORDERS],
        }
    }

    /// A point of `A` with the direction `d` there ([`rates_a`]).
    pub fn of_a(at: DVec3, sigma_a: Sign, d: impl Into<Rate>) -> Moving {
        Moving {
            at,
            rates: rates_a(sigma_a, d),
        }
    }

    fn eval<N: Num>(&self) -> V3<N> {
        moved(self.at, &self.rates)
    }
}

/// A straight segment from `a` to `b` moving as one at `rates`, its rates
/// at the record's point (module docs): where the record reads it, the
/// field's value there.
#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    /// The start.
    pub a: DVec3,
    /// The end.
    pub b: DVec3,
    /// The rates of its point the record reads.
    pub rates: [Rate; ORDERS],
}

impl Seg {
    /// A segment of `B`: it never moves.
    pub const fn fixed(a: DVec3, b: DVec3) -> Seg {
        Seg {
            a,
            b,
            rates: [Rate::ZERO; ORDERS],
        }
    }

    /// A segment of `A` with the direction `d` at the record's point.
    pub fn of_a(a: DVec3, b: DVec3, sigma_a: Sign, d: impl Into<Rate>) -> Seg {
        Seg {
            a,
            b,
            rates: rates_a(sigma_a, d),
        }
    }

    fn ends<N: Num>(&self) -> [V3<N>; 2] {
        [moved(self.a, &self.rates), moved(self.b, &self.rates)]
    }
}

/// Which shadow-plane direction a vertex–vertex shadow row measures along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShadowAxis {
    /// Across the `r̂` line through `q`, along [`NU`]: `+` with `w` to its
    /// left seen from above.
    Side,
    /// Along it, [`R_HAT`]: `+` with `w` ahead of `q`.
    Along,
}

impl ShadowAxis {
    /// The axis's exact, unnormalised vector.
    pub fn vector(self) -> DVec3 {
        match self {
            ShadowAxis::Side => NU,
            ShadowAxis::Along => R_HAT,
        }
    }
}

/// The vertex–vertex shadow row, flat: `axis·(w − q)`, the shadow side of
/// `w` against `q`'s `r̂` line or whether it is ahead along it, times the
/// axis's length.
#[derive(Clone, Debug, PartialEq)]
pub struct VvShadow {
    /// The query vertex.
    pub q: Moving,
    /// The other operand's vertex.
    pub w: Moving,
    /// The direction measured along.
    pub axis: ShadowAxis,
}

impl Pred for VvShadow {
    fn eval<N: Num>(&self) -> N {
        let l = point::<N>(self.axis.vector());
        dot(&l, &sub(&self.w.eval(), &self.q.eval()))
    }
}

/// The vertex–curve shadow row on a straight curve, flat, also a box
/// side's row (a tile line on a plane): `(UP × (b − a))·(q − a)`, the
/// shadow side of `q` against the line's shadow, `+` to its left seen
/// from above, times `|UP × (b − a)|`. Zero for a line along `UP`.
#[derive(Clone, Debug, PartialEq)]
pub struct LineShadow {
    /// The query vertex.
    pub q: Moving,
    /// The line, moving at its rates at the foot of `q`'s shadow.
    pub line: Seg,
}

impl Pred for LineShadow {
    fn eval<N: Num>(&self) -> N {
        let [a, b] = self.line.ends::<N>();
        let nu = cross(&point(UP), &sub(&b, &a));
        dot(&nu, &sub(&self.q.eval(), &a))
    }
}

/// The plane-height row, flat: `(c − n·q)·(n·UP)` for the plane `n·x = c`,
/// the plane's height above `q` along `UP` times `(n·UP)²/|UP|`. Its
/// orders are the tangent-plane row, `(n·Δf − n·Δq)·(n·UP)`, `Δf` the
/// plane's rates at the anchor (its offset moves by `n·Δf`): a germ key's
/// rows on a plane face. Zero, orders too, on a vertical plane.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneHeight {
    /// The plane's normal `n`, oriented as its face (scaled freely: a
    /// stored normal of largest component 1).
    pub normal: DVec3,
    /// The offset `c`.
    pub offset: f64,
    /// The plane's rates at the anchor.
    pub rates: [Rate; ORDERS],
    /// The query point.
    pub q: Moving,
}

impl Pred for PlaneHeight {
    fn eval<N: Num>(&self) -> N {
        let n = point::<N>(self.normal);
        let shift = self.rates.each_ref().map(|r| dot(&n, &r.vec()));
        let c = N::lit(self.offset).moved_by(&shift);
        let above = c.sub(&dot(&n, &self.q.eval()));
        above.mul(&dot(&n, &point(UP)))
    }
}

/// The point–point height, flat: `UP·(w − q)`, the height of `w` above
/// `q` along `UP` times `|UP|`: a vertex–vertex height key's value, and,
/// with `w` a straight edge's point at the foot, a point–line one's.
#[derive(Clone, Debug, PartialEq)]
pub struct PointHeight {
    /// The lower point asked of.
    pub q: Moving,
    /// The point whose height above `q` it is.
    pub w: Moving,
}

impl Pred for PointHeight {
    fn eval<N: Num>(&self) -> N {
        dot(&point(UP), &sub(&self.w.eval(), &self.q.eval()))
    }
}

/// The edge–edge z-order row, flat: `(UP·m)·(m·(g.a − e.a))` with
/// `m = (e.b − e.a) × (g.b − g.a)`: the height of `g` above `e` along
/// `UP` where their shadows cross, times `(m·UP)²/|UP|`. Zero, orders
/// too, where `m·UP = 0`: parallel shadows (no crossing) or an edge
/// along `UP` (its shadow a point); there [`decide`] reaches its guard,
/// never a sign.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeEdge {
    /// The edge of `A`, at its rates at the crossing.
    pub e: Seg,
    /// The edge of `B` (the anchor), at its rates at the crossing.
    pub g: Seg,
}

impl Pred for EdgeEdge {
    fn eval<N: Num>(&self) -> N {
        let [e0, e1] = self.e.ends::<N>();
        let [g0, g1] = self.g.ends::<N>();
        let m = cross(&sub(&e1, &e0), &sub(&g1, &g0));
        dot(&point(UP), &m).mul(&dot(&m, &sub(&g0, &e0)))
    }
}

/// A point of a rational form at exact parameters, homogeneous: `X =
/// Σ cₖ·pₖ`, `W = Σ cₖ` over stored control points `pₖ`, each `cₖ` the
/// exact product of its weight and its Bernstein value at the parameters
/// (a dyadic point of a tile edge or a tile), the point `X/W` moving at
/// `rates` (the field's value there). With positive weights `W > 0`, so a
/// row multiplied by a positive power of `W` keeps its sign: the point's
/// rows stay flat, exact on the stored data.
#[derive(Clone, Debug, PartialEq)]
pub struct HomPoint {
    /// The control points with their coefficients.
    pub terms: Vec<(DVec3, Rational)>,
    /// The point's rates.
    pub rates: [Rate; ORDERS],
}

impl HomPoint {
    /// `(X, W)`, `X` moved at the rates (`W·Δ` added to it).
    fn eval<N: Num>(&self) -> (V3<N>, N) {
        let zero = N::lit(0.0);
        let (mut x, mut w) = ([zero.clone(), zero.clone(), zero.clone()], zero);
        for (p, c) in &self.terms {
            let c = N::rational(c);
            let m = moved::<N>(*p, &self.rates);
            for k in 0..3 {
                x[k] = x[k].add(&c.mul(&m[k]));
            }
            w = w.add(&c);
        }
        (x, w)
    }
}

impl HomPoint {
    /// The stored point `m`, of weight 1.
    pub fn of_moving(m: &Moving) -> HomPoint {
        HomPoint {
            terms: vec![(m.at, Rational::from(1u32))],
            rates: m.rates.clone(),
        }
    }

    /// The point `X/W` at `like`'s level with its rates, certified
    /// (exact where the level holds the coefficients).
    pub fn at_level<S: Scalar>(&self, like: &S) -> MovingAt<S> {
        let mut x: [S; 3] = std::array::from_fn(|_| S::int(0, like));
        let mut w = S::int(0, like);
        for (p, c) in &self.terms {
            let c = rational_at(c, like);
            let p = lit3(*p, like);
            for k in 0..3 {
                x[k] = x[k].add(&c.mul(&p[k]));
            }
            w = w.add(&c);
        }
        MovingAt {
            at: x.map(|x| x.div(&w)),
            rates: self.rates.each_ref().map(|r| r.at(like)),
        }
    }
}

/// The point a shadow row asks about: a stored point, or a homogeneous
/// one (a dyadic point of a stored curve).
#[derive(Clone, Debug, PartialEq)]
pub enum QueryPoint {
    /// A stored point.
    Flat(Moving),
    /// A homogeneous point with positive weight.
    Hom(HomPoint),
}

impl QueryPoint {
    /// The point as a homogeneous one (a stored point of weight 1).
    pub fn hom(&self) -> HomPoint {
        match self {
            QueryPoint::Flat(m) => HomPoint::of_moving(m),
            QueryPoint::Hom(h) => h.clone(),
        }
    }

    /// The point at `like`'s level with its rates, certified.
    pub fn at_level<S: Scalar>(&self, like: &S) -> MovingAt<S> {
        match self {
            QueryPoint::Flat(m) => MovingAt::from_flat(m, like),
            QueryPoint::Hom(h) => h.at_level(like),
        }
    }
}

/// The vertex–vertex shadow row between two homogeneous points, flat:
/// `axis·(W_q·X_w − W_w·X_q)`, [`VvShadow`]'s value times `W_q·W_w > 0`.
#[derive(Clone, Debug, PartialEq)]
pub struct HomPairShadow {
    /// The query point.
    pub q: HomPoint,
    /// The other operand's point.
    pub w: HomPoint,
    /// The direction measured along.
    pub axis: ShadowAxis,
}

impl Pred for HomPairShadow {
    fn eval<N: Num>(&self) -> N {
        let (xq, wq) = self.q.eval::<N>();
        let (xw, ww) = self.w.eval::<N>();
        let d = [0, 1, 2].map(|k| wq.mul(&xw[k]).sub(&ww.mul(&xq[k])));
        dot(&point::<N>(self.axis.vector()), &d)
    }
}

/// The vertex–curve shadow row of a homogeneous query point on the line
/// through two homogeneous points `a`, `b`, flat: `(UP × (W_a·X_b −
/// W_b·X_a))·(W_a·X_q − W_q·X_a)`, [`HomLineShadow`]'s value times `W_q >
/// 0`.
#[derive(Clone, Debug, PartialEq)]
pub struct HomPointLineShadow {
    /// The query point.
    pub q: HomPoint,
    /// The line's first point.
    pub a: HomPoint,
    /// Its second.
    pub b: HomPoint,
}

impl Pred for HomPointLineShadow {
    fn eval<N: Num>(&self) -> N {
        let (xa, wa) = self.a.eval::<N>();
        let (xb, wb) = self.b.eval::<N>();
        let (xq, wq) = self.q.eval::<N>();
        let d = [0, 1, 2].map(|k| wa.mul(&xb[k]).sub(&wb.mul(&xa[k])));
        let nu = cross(&point(UP), &d);
        let off = [0, 1, 2].map(|k| wa.mul(&xq[k]).sub(&wq.mul(&xa[k])));
        dot(&nu, &off)
    }
}

/// The vertex–vertex shadow row against a homogeneous point, flat:
/// `axis·(X − W·q)`, [`VvShadow`]'s value times `W > 0`.
#[derive(Clone, Debug, PartialEq)]
pub struct HomShadow {
    /// The query point.
    pub q: Moving,
    /// The other operand's point.
    pub w: HomPoint,
    /// The direction measured along.
    pub axis: ShadowAxis,
}

impl Pred for HomShadow {
    fn eval<N: Num>(&self) -> N {
        let (x, w) = self.w.eval::<N>();
        let q = self.q.eval::<N>();
        let wq = [0, 1, 2].map(|k| w.mul(&q[k]));
        dot(&point::<N>(self.axis.vector()), &sub(&x, &wq))
    }
}

/// The vertex–curve shadow row on the line through two homogeneous
/// points `a`, `b`, flat: `(UP × (W_a·X_b − W_b·X_a))·(W_a·q − X_a)`,
/// [`LineShadow`]'s value on `a`, `b` times `W_a²·W_b > 0`.
#[derive(Clone, Debug, PartialEq)]
pub struct HomLineShadow {
    /// The query point.
    pub q: Moving,
    /// The line's first point.
    pub a: HomPoint,
    /// Its second.
    pub b: HomPoint,
}

impl Pred for HomLineShadow {
    fn eval<N: Num>(&self) -> N {
        let (xa, wa) = self.a.eval::<N>();
        let (xb, wb) = self.b.eval::<N>();
        let q = self.q.eval::<N>();
        let d = [0, 1, 2].map(|k| wa.mul(&xb[k]).sub(&wb.mul(&xa[k])));
        let nu = cross(&point(UP), &d);
        let off = [0, 1, 2].map(|k| wa.mul(&q[k]).sub(&xa[k]));
        dot(&nu, &off)
    }
}

/// A flat record decided on its own value and orders: the sign of `pred`
/// under the perturbation, and what it took to charge.
pub fn decide_flat(pred: &impl Pred, key: TieKey) -> (Result<Sign, Fail>, Work) {
    let signs = exact::sign(pred);
    let (g, ord) = exact_orders(&signs);
    (decide(g, ord, Band::FLAT, key), signs.work)
}

/// A flat record whose value `g` is its height key's (decided once for
/// every anchor reading it) and whose orders are `pred`'s, this anchor's
/// row: each worked out only when reached. Returns what the orders took.
pub fn decide_flat_orders(
    g: Ordering,
    pred: &impl Pred,
    key: TieKey,
) -> (Result<Sign, Fail>, Work) {
    let mut work = Work::default();
    let s = decide(Ok(g), exact::order_signs(pred, &mut work), Band::FLAT, key);
    (s, work)
}

/// A precision level curved rows are evaluated at: a [`Scalar`] with a
/// certified square root (uncertified unless its argument is certainly
/// `≥ 0`), so directions are normalised and values are lengths.
pub trait Curved: Scalar {
    /// `√self`.
    fn sqrt(&self) -> Self;
}

impl Curved for Bounded<f64> {
    fn sqrt(&self) -> Self {
        Bounded::<f64>::sqrt(*self)
    }
}

impl Curved for Bounded<Dd> {
    fn sqrt(&self) -> Self {
        Bounded::<Dd>::sqrt(*self)
    }
}

impl<B: BigF> Curved for Bounded<B> {
    fn sqrt(&self) -> Self {
        Bounded::<B>::sqrt(self)
    }
}

/// A point at a precision level with its rates along the motions
/// ([`Moving`]'s counterpart for curved rows).
#[derive(Clone, Debug, PartialEq)]
pub struct MovingAt<S> {
    /// The point.
    pub at: [S; 3],
    /// Its rates, `rates[k − 1]` along motion `k`.
    pub rates: [[S; 3]; ORDERS],
}

impl<S: Scalar> MovingAt<S> {
    /// A point of `B`: it never moves.
    pub fn fixed(at: [S; 3]) -> Self {
        let zero = S::int(0, &at[0]);
        let rates = std::array::from_fn(|_| [zero.clone(), zero.clone(), zero.clone()]);
        MovingAt { at, rates }
    }

    /// A point of `A` with the direction `d` there: rates `σ_A·d`, then
    /// `T₂, T₃, T₄` ([`rates_a`]).
    pub fn of_a(at: [S; 3], sigma_a: Sign, d: [S; 3]) -> Self {
        let own = match sigma_a {
            Sign::Pos => d,
            Sign::Neg => d.map(|x| x.neg()),
        };
        let t = |k: usize| lit3(T[k], &at[0]);
        let rates = [own, t(0), t(1), t(2)];
        MovingAt { at, rates }
    }

    /// A flat point at this level (`like` gives the level and width):
    /// exactly, but for exact rates the level can't hold, which it
    /// certifies.
    pub fn from_flat(m: &Moving, like: &S) -> Self {
        MovingAt {
            at: lit3(m.at, like),
            rates: m.rates.each_ref().map(|r| r.at(like)),
        }
    }
}

/// A curved record's value and its orders, lengths with certified errors
/// at one level, for [`decide`].
#[derive(Clone, Debug, PartialEq)]
pub struct Coeffs<S> {
    /// The value `g`.
    pub g: S,
    /// `g₁, …, g₄`.
    pub ord: [S; ORDERS],
}

/// The vertex–vertex shadow row, curved (a vertex read on a curved
/// surface): `ν̂·(w − q)` or `r̂·(w − q)`, unit axes.
pub fn vv_shadow<S: Curved>(q: &MovingAt<S>, w: &MovingAt<S>, axis: ShadowAxis) -> Coeffs<S> {
    let l = unit3(&lit3(axis.vector(), &q.at[0]));
    along(&l, w, q)
}

/// The vertex–curve shadow row, curved, also a box side's: `ν·(q − C)`
/// with `ν = unit(ê × T)` for the curve's point `C` at the foot of `q`'s
/// shadow and its tangent `T` there (any positive multiple). `foot`
/// carries the curve's rates at `C`. Uncertified (escalates) where `T` is
/// too near `UP` to normalise `ν`.
pub fn curve_shadow<S: Curved>(q: &MovingAt<S>, foot: &MovingAt<S>, tangent: &[S; 3]) -> Coeffs<S> {
    let nu = unit3(&cross3(&lit3(UP, &q.at[0]), tangent));
    along(&nu, q, foot)
}

/// The column-hit orders, curved: the tangent plane of the anchor face
/// `f`, `(n·Δf − n·Δq)·sign(n·ê)` with `n` the unit normal at the anchor
/// (from `normal`, any positive multiple, oriented as the face) and `Δf`
/// the face's rates there. `None` when the facing `sign(n·ê)` isn't
/// certified: the in-face fold rule decides instead. Order 0 is the height
/// key's.
pub fn tangent_plane_row<S: Curved>(
    normal: &[S; 3],
    face_rates: &[[S; 3]; ORDERS],
    q: &MovingAt<S>,
) -> Option<[S; ORDERS]> {
    let n = oriented(unit3(normal), &q.at[0])?;
    Some(std::array::from_fn(|k| {
        dot3(&n, &sub3(&face_rates[k], &q.rates[k]))
    }))
}

/// The column-hit orders on a ribbon, curved: the tangent plane of the
/// ribbon at the hit, `(n_R·ΔR − n_R·Δq)·sign(n_R·ê)` with `n_R` the unit
/// normal `R_t × R_λ` from the ribbon's partials `rt`, `rl` there and
/// `ΔR` the ribbon point's rates (the moving operand's field there, zero
/// on `B`): the height's derivative along each motion times `|n_R·ê|`.
/// `None` when `sign(n_R·ê)` isn't certified (the column tangent to the
/// ribbon). Order 0 is the hit's height.
pub fn ribbon_row<S: Curved>(
    rt: &[S; 3],
    rl: &[S; 3],
    ribbon_rates: &[[S; 3]; ORDERS],
    q: &MovingAt<S>,
) -> Option<[S; ORDERS]> {
    tangent_plane_row(&cross3(rt, rl), ribbon_rates, q)
}

/// [`ribbon_row`] exactly at a hit on flat data, the ribbon's parameter
/// the root `at` of the hit polynomial: the signs of `(N·Δₖ)·(N·UP)`,
/// `N = rt × rl` from polynomial numerators in the parameter whose cross
/// product is `n_R` times a non-zero factor of either sign (which the
/// product cancels), `Δₖ = ΔR − Δq` per motion. A sign is `Equal` where
/// the motion moves the height to no first order (or the column is
/// tangent to the ribbon there, `N·UP = 0`).
pub fn ribbon_row_signs(
    rt: &[Poly; 3],
    rl: &[Poly; 3],
    at: &Root,
    deltas: &[[Rational; 3]; ORDERS],
) -> [ExactSign; ORDERS] {
    let n = [
        poly::sub(&poly::mul(&rt[1], &rl[2]), &poly::mul(&rt[2], &rl[1])),
        poly::sub(&poly::mul(&rt[2], &rl[0]), &poly::mul(&rt[0], &rl[2])),
        poly::sub(&poly::mul(&rt[0], &rl[1]), &poly::mul(&rt[1], &rl[0])),
    ];
    let along = |d: &[Rational; 3]| -> Poly {
        (0..3).fold(Vec::new(), |acc, k| {
            poly::add(&acc, &poly::scale(&n[k], &d[k]))
        })
    };
    let facing = at.sign_of(&along(&[UP.x, UP.y, UP.z].map(rat)));
    deltas.each_ref().map(|d| {
        let s = at.sign_of(&along(d));
        Ok(match facing {
            Ordering::Less => s.reverse(),
            Ordering::Equal => Ordering::Equal,
            Ordering::Greater => s,
        })
    })
}

/// A rate as exact components.
pub fn rate_rational(r: &Rate) -> [Rational; 3] {
    match r {
        Rate::Float(v) => [v.x, v.y, v.z].map(rat),
        Rate::Exact(d) => (**d).clone(),
    }
}

/// The point–face height key's value, curved: `n_f·(x_h − q)·sign(n_f·ê)`
/// with `n_f` the unit normal at the hit `x_h` of `q`'s column on the face
/// (from `normal`, any positive multiple, oriented as the face): the
/// face's height above `q`, a length, positive where the face is above
/// whichever way it faces. `None` when the facing `sign(n_f·ê)` isn't
/// certified: the in-face fold rule decides instead. Flat, the value is
/// [`PlaneHeight`]'s.
pub fn face_height<S: Curved>(normal: &[S; 3], hit: &[S; 3], q: &[S; 3]) -> Option<S> {
    let n = oriented(unit3(normal), &q[0])?;
    Some(dot3(&n, &sub3(hit, q)))
}

/// The edge–edge z-order row, curved: `m·(C_g − C_e)` and its orders
/// `m·(Δg − Δe)`, `m = unit(T_e × T_g)` oriented `m·ê > 0`, at the
/// crossing's points `e` (on the edge of `A`) and `g` (on the anchor edge
/// of `B`), each with its edge's rates there, from the tangents `te`, `tg`
/// (any positive multiples). `+` with `g` above `e`. `None` when `m·ê`
/// isn't certified non-zero (shadows too near parallel).
pub fn z_order<S: Curved>(
    e: &MovingAt<S>,
    te: &[S; 3],
    g: &MovingAt<S>,
    tg: &[S; 3],
) -> Option<Coeffs<S>> {
    let m = oriented(unit3(&cross3(te, tg)), &e.at[0])?;
    Some(along(&m, g, e))
}

/// A curve's point at `s` and its tangent there, `W·P′ − W′·P` (`W²`
/// times the point's derivative, so of the curve's direction where the
/// weights are positive).
pub fn curve_point<S: Scalar>(c: &HomCurve<S, 3>, s: &S) -> ([S; 3], [S; 3]) {
    let (p, w) = c.eval_hom(s);
    let (dp, dw) = c.derivative().eval_hom(s);
    let at = [0, 1, 2].map(|k| p[k].div(&w));
    let tangent = [0, 1, 2].map(|k| w.mul(&dp[k]).sub(&dw.mul(&p[k])));
    (at, tangent)
}

/// A patch's point at `(u, v)` and its normal there, `x_u × x_v` (the
/// patch's orientation).
pub fn patch_point<S: Scalar>(f: &HomPatch<S>, u: &S, v: &S) -> ([S; 3], [S; 3]) {
    let p = f.eval_partials(u, v);
    let normal = cross3(&p.du, &p.dv);
    (p.p, normal)
}

/// `l·(plus − minus)` and its orders `l·(Δplus − Δminus)`.
fn along<S: Scalar>(l: &[S; 3], plus: &MovingAt<S>, minus: &MovingAt<S>) -> Coeffs<S> {
    Coeffs {
        g: dot3(l, &sub3(&plus.at, &minus.at)),
        ord: std::array::from_fn(|k| dot3(l, &sub3(&plus.rates[k], &minus.rates[k]))),
    }
}

/// The unit vector `v` turned to `v·ê > 0`; `None` unless that sign is
/// certified.
fn oriented<S: Curved>(v: [S; 3], like: &S) -> Option<[S; 3]> {
    match dot3(&v, &lit3(UP, like)).sign()? {
        Ordering::Greater => Some(v),
        Ordering::Less => Some(v.map(|x| x.neg())),
        Ordering::Equal => None,
    }
}

/// The rational `r` at `like`'s level, certified: its numerator and
/// denominator built from their 64-bit limbs, then divided (exact at the
/// rational level; uncertified where the level's range can't hold them).
fn rational_at<S: Scalar>(r: &Rational, like: &S) -> S {
    let natural = |n: &Natural| {
        let base = S::from_f64(18_446_744_073_709_551_616.0, like); // 2⁶⁴
        let limbs = n.to_limbs_asc();
        limbs.iter().rev().fold(S::int(0, like), |acc, &l| {
            acc.mul(&base).add(&S::int_u64(l, like))
        })
    };
    let num = natural(r.numerator_ref());
    let num = if *r < 0u32 { num.neg() } else { num };
    if *r.denominator_ref() == 1u32 {
        num
    } else {
        num.div(&natural(r.denominator_ref()))
    }
}

fn lit3<S: Scalar>(v: DVec3, like: &S) -> [S; 3] {
    [v.x, v.y, v.z].map(|x| S::from_f64(x, like))
}

fn sub3<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> [S; 3] {
    [0, 1, 2].map(|k| a[k].sub(&b[k]))
}

fn dot3<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> S {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn cross3<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> [S; 3] {
    [
        a[1].mul(&b[2]).sub(&a[2].mul(&b[1])),
        a[2].mul(&b[0]).sub(&a[0].mul(&b[2])),
        a[0].mul(&b[1]).sub(&a[1].mul(&b[0])),
    ]
}

/// `v / |v|`: uncertified where `|v|` isn't certainly non-zero.
fn unit3<S: Curved>(v: &[S; 3]) -> [S; 3] {
    // Scaled first by a power of two to a largest component near 1: the
    // same direction, but its squared length neither underflows (a
    // tangent of a face at coordinates near `10⁻³⁰⁰`) nor overflows.
    let big = v
        .iter()
        .flat_map(|x| x.enclose())
        .map(f64::abs)
        .fold(0.0, |m: f64, x| if x > m { x } else { m });
    let v = if big > 0.0 && big.is_finite() {
        // `2^−e` for `big` in `[2^e, 2^(e+1))`, exact, kept normal.
        let e = ((big.to_bits() >> 52) & 0x7ff) as i64 - 1023;
        let e = if big < f64::MIN_POSITIVE {
            -1023
        } else {
            e.clamp(-1000, 1000)
        };
        let k = f64::from_bits(((1023 - e) as u64) << 52);
        let k = S::from_f64(k, &v[0]);
        [0, 1, 2].map(|i| v[i].mul(&k))
    } else {
        v.clone()
    };
    let len = dot3(&v, &v).sqrt();
    [0, 1, 2].map(|k| v[k].div(&len))
}

#[cfg(test)]
mod tests;
