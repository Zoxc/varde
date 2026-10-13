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
//! crossing, the anchor). Away from a tie the orders aren't read. A
//! direction given as a fraction may be scaled by one positive factor per
//! motion and record (its denominator cleared): every order keeps its
//! sign.
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
//! | vertex–vertex shadow ([`VvShadow`], [`vv_shadow`]) | `ν·(w − q)`, ahead/behind `r̂·(w − q)` | `ν·(Δw − Δq)`, likewise `r̂` |
//! | vertex–curve shadow, box side ([`LineShadow`], [`curve_shadow`]) | `ν·(q − C(s*))`, `ν = ê × T` at the foot | `ν·(Δq − ΔC(s*))` |
//! | layer height, anchor `f` ([`PlaneHeight`], [`layer_height`]) | from the record's height key | `(n_f·Δf − n_f·Δq)·sign(n_f·ê)`: the tangent plane of `f` at the anchor |
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

use glam::DVec3;

use crate::bezier::{HomCurve, HomPatch, Scalar};
use crate::num::bigf::BigF;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::{Band, Fail, Sign, TieKey, decide, exact_orders};
use crate::num::exact::{self, Num, ORDERS, Pred, V3, Work, cross, dot, moved_point, point, sub};

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

/// The rates of a point of `A` along the motions: `σ_A·d`, then
/// `T₂, T₃, T₄`. `d` is the direction field at the point (zero where
/// the generic translations alone decide).
pub fn rates_a(sigma_a: Sign, d: DVec3) -> [DVec3; ORDERS] {
    let own = match sigma_a {
        Sign::Pos => d,
        Sign::Neg => -d,
    };
    [own, T[0], T[1], T[2]]
}

/// A point with its rates along the motions (`rates[k − 1]` along motion
/// `k`), each coordinate and rate finite and at most
/// [`MAX_INPUT`](exact::MAX_INPUT).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moving {
    /// The point.
    pub at: DVec3,
    /// Its rates.
    pub rates: [DVec3; ORDERS],
}

impl Moving {
    /// A point of `B`: it never moves.
    pub const fn fixed(at: DVec3) -> Moving {
        Moving {
            at,
            rates: [DVec3::ZERO; ORDERS],
        }
    }

    /// A point of `A` with the direction `d` there ([`rates_a`]).
    pub fn of_a(at: DVec3, sigma_a: Sign, d: DVec3) -> Moving {
        Moving {
            at,
            rates: rates_a(sigma_a, d),
        }
    }

    fn eval<N: Num>(&self) -> V3<N> {
        moved_point(self.at, self.rates)
    }
}

/// A straight segment from `a` to `b` moving as one at `rates`, its rates
/// at the record's point (module docs): where the record reads it, the
/// field's value there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    /// The start.
    pub a: DVec3,
    /// The end.
    pub b: DVec3,
    /// The rates of its point the record reads.
    pub rates: [DVec3; ORDERS],
}

impl Seg {
    /// A segment of `B`: it never moves.
    pub const fn fixed(a: DVec3, b: DVec3) -> Seg {
        Seg {
            a,
            b,
            rates: [DVec3::ZERO; ORDERS],
        }
    }

    /// A segment of `A` with the direction `d` at the record's point.
    pub fn of_a(a: DVec3, b: DVec3, sigma_a: Sign, d: DVec3) -> Seg {
        Seg {
            a,
            b,
            rates: rates_a(sigma_a, d),
        }
    }

    fn ends<N: Num>(&self) -> [V3<N>; 2] {
        [
            moved_point(self.a, self.rates),
            moved_point(self.b, self.rates),
        ]
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
#[derive(Clone, Copy, Debug, PartialEq)]
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
#[derive(Clone, Copy, Debug, PartialEq)]
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

/// The layer-height row on a plane, flat: `(c − n·q)·(n·UP)` for the plane
/// `n·x = c`, the plane's height above `q` along `UP` times `(n·UP)²/|UP|`.
/// Its orders are the tangent-plane row, `(n·Δf − n·Δq)·(n·UP)`, `Δf` the
/// plane's rates at the anchor (its offset moves by `n·Δf`); its value is
/// the height key's for a point–face key on the plane. Zero, orders too,
/// on a vertical plane, where the fold rule decides instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneHeight {
    /// The plane's normal `n`, oriented as its face (scaled freely: a
    /// stored normal of largest component 1).
    pub normal: DVec3,
    /// The offset `c`.
    pub offset: f64,
    /// The plane's rates at the anchor.
    pub rates: [DVec3; ORDERS],
    /// The query point.
    pub q: Moving,
}

impl Pred for PlaneHeight {
    fn eval<N: Num>(&self) -> N {
        let n = point::<N>(self.normal);
        let shift = self.rates.map(|r| dot(&n, &point(r)));
        let c = N::lit(self.offset).moved_by(&shift);
        let above = c.sub(&dot(&n, &self.q.eval()));
        above.mul(&dot(&n, &point(UP)))
    }
}

/// The edge–edge z-order row, flat: `(UP·m)·(m·(g.a − e.a))` with
/// `m = (e.b − e.a) × (g.b − g.a)`: the height of `g` above `e` along
/// `UP` where their shadows cross, times `(m·UP)²/|UP|`. Zero, orders
/// too, where `m·UP = 0`: parallel shadows (no crossing) or an edge
/// along `UP` (its shadow a point); there [`decide`] reaches its guard,
/// never a sign.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    let s = decide(g, exact::order_signs(pred, &mut work), Band::FLAT, key);
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

    /// A flat point at this level, exactly (`like` gives the level and
    /// width).
    pub fn from_flat(m: &Moving, like: &S) -> Self {
        MovingAt {
            at: lit3(m.at, like),
            rates: m.rates.map(|r| lit3(r, like)),
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

/// The layer-height orders, curved: the tangent plane of the anchor face
/// `f`, `(n·Δf − n·Δq)·sign(n·ê)` with `n` the unit normal at the anchor
/// (from `normal`, any positive multiple, oriented as the face) and `Δf`
/// the face's rates there. `None` when the facing `sign(n·ê)` isn't
/// certified: the in-face fold rule decides instead. Order 0 is the height
/// key's.
pub fn layer_height<S: Curved>(
    normal: &[S; 3],
    face_rates: &[[S; 3]; ORDERS],
    q: &MovingAt<S>,
) -> Option<[S; ORDERS]> {
    let n = oriented(unit3(normal), &q.at[0])?;
    Some(std::array::from_fn(|k| {
        dot3(&n, &sub3(&face_rates[k], &q.rates[k]))
    }))
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
    let len = dot3(v, v).sqrt();
    [0, 1, 2].map(|k| v[k].div(&len))
}

#[cfg(test)]
mod tests;
