//! Edge-pair crossings against the moved configuration: straight edges on
//! a small lattice (vertices on the other edge's shadow, shared corners,
//! parallel and collinear shadows), whose four-term count is the exact
//! sign of their moved crossing; random partitions of the square into
//! boxes, of straight and of curved edge pairs (conic spans with weights
//! from 1/64 to 64, knots weighted unevenly), whose degrees sum to it;
//! further splits keep the sum; a box apart from both edges' shadows has
//! degree 0 from every ray; data that doesn't fit is refused.

use std::cell::RefCell;
use std::collections::BTreeMap;

use glam::DVec3;
use malachite::Rational as Q;
use malachite::base::num::arithmetic::traits::PowerOf2;
use varde_testkit::rng::Rng;

use super::*;
use crate::ids::{EdgeId, VertId};
use crate::num::bounded::Bounded;
use crate::num::decide::{Band, Fail, Sign};
use crate::num::exact::ORDERS;
use crate::perturb::{Curved, Moving, R_HAT, Rate, Seg, T, UP, rates_a};
use crate::records::keys::{EdgeAt, Operand, Point, RayKey, XKey};
use crate::records::ray::{ChainRec, CurveGeometry, CurvedRays, Span};
use crate::records::shadow::{Cost, Decided, Pass, ShadowGeometry, Shadows};
use crate::records::table::Fresh;
use crate::records::xrec::*;

use Operand::{A, B};

/// Straight or curved edges of either operand; `A` moves as a translation
/// along `d`.
#[derive(Default)]
struct Geom {
    pos: BTreeMap<Point, DVec3>,
    /// Ends, lower id first, and the spans of a curved edge (none for a
    /// straight one).
    edges: BTreeMap<(Operand, EdgeId), ([VertId; 2], Vec<Span>)>,
    d: DVec3,
    /// A linear part of `A`'s direction field: `d + (field·p)·ν` at `p`.
    field: DVec3,
    /// A moved-vertex blend of the field near a vertex.
    blend: Option<Blend>,
}

/// A moved-vertex blend of `A`'s field near the vertex at `at`: the
/// direction `d` within `r` of it, the field's own from `2r` on, linearly
/// between (`λ = clamp(‖p − at‖/r − 1, 0, 1)`, not normalised).
#[derive(Clone, Copy, Debug)]
struct Blend {
    at: DVec3,
    d: DVec3,
    r: f64,
}

impl Geom {
    fn rates(&self, op: Operand) -> [Rate; ORDERS] {
        self.rates_at(op, DVec3::ZERO)
    }

    /// `A`'s direction field at `p`.
    fn d_at(&self, p: DVec3) -> DVec3 {
        let own = self.d + NU * self.field.dot(p);
        match self.blend {
            Some(b) => {
                let l = ((p - b.at).length() / b.r - 1.0).clamp(0.0, 1.0);
                b.d * (1.0 - l) + own * l
            }
            None => own,
        }
    }

    fn rates_at(&self, op: Operand, p: DVec3) -> [Rate; ORDERS] {
        match op {
            A => rates_a(Sign::Pos, self.d_at(p)),
            B => [Rate::ZERO; ORDERS],
        }
    }

    fn vertex(&mut self, op: Operand, v: u32, at: DVec3) {
        if let Some(old) = self.pos.insert(Point::vert(op, VertId(v)), at) {
            assert_eq!(old, at, "a vertex at two places");
        }
    }

    /// Straight edge `id` of `op` from vertex `v` (at `pv`) to `w` (at
    /// `pw`), `v < w`.
    fn segment(&mut self, op: Operand, id: u32, (v, pv): (u32, DVec3), (w, pw): (u32, DVec3)) {
        assert!(v < w);
        self.vertex(op, v, pv);
        self.vertex(op, w, pw);
        self.edges
            .insert((op, EdgeId(id)), ([VertId(v), VertId(w)], Vec::new()));
    }

    /// Curved edge `id` of `op` from vertex `v` to `w`, `v < w`, along
    /// `spans`.
    fn curve(&mut self, op: Operand, id: u32, v: u32, w: u32, spans: Vec<Span>) {
        assert!(v < w);
        let at = |p: &[f64; 4]| DVec3::new(p[0], p[1], p[2]);
        self.vertex(op, v, at(&spans[0][0]));
        self.vertex(op, w, at(&spans[spans.len() - 1][2]));
        self.edges
            .insert((op, EdgeId(id)), ([VertId(v), VertId(w)], spans));
    }

    /// `A`'s offset in the moved configuration: `δ·(d + η·T₂ + η²·T₃ +
    /// η³·T₄)`, `δ = 2⁻²⁰⁰`, `η = 2⁻²⁰`.
    fn offset(&self, op: Operand, p: DVec3) -> [Q; 3] {
        let mut out = [0, 1, 2].map(|_| Q::from(0u32));
        if op == B {
            return out;
        }
        for (k, r) in (0i64..).zip([self.d_at(p), T[0], T[1], T[2]]) {
            let eps = Q::power_of_2(-200 - 20 * k);
            let r = q3(r);
            for i in 0..3 {
                out[i] = &out[i] + &eps * &r[i];
            }
        }
        out
    }

    /// Point `p`'s moved shadow `(r̂·x, ν·x)`, exactly.
    fn moved_shadow(&self, p: Point) -> [Q; 2] {
        let o = self.offset(p.operand, self.pos[&p]);
        let at = q3(self.pos[&p]);
        let x: [Q; 3] = [0, 1, 2].map(|i| &at[i] + &o[i]);
        [qdot(&q3(R_HAT), &x), qdot(&q3(NU), &x)]
    }

    fn curve_of(&self, op: Operand, id: u32) -> XCurve<'_> {
        let (ends, spans) = &self.edges[&(op, EdgeId(id))];
        if spans.is_empty() {
            let [a, b] = ends.map(|v| self.pos[&Point::vert(op, v)]);
            XCurve::Line([a.x, a.y, a.z], [b.x, b.y, b.z])
        } else {
            XCurve::Spans(spans)
        }
    }
}

impl ShadowGeometry for Geom {
    fn point(&self, p: Point) -> Moving {
        Moving {
            at: self.pos[&p],
            rates: self.rates_at(p.operand, self.pos[&p]),
        }
    }

    fn near_radius(&self, _: Point) -> f64 {
        0.0
    }

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)].0
    }

    fn line(&self, q: Point, g: EdgeId) -> Seg {
        let op = q.operand.other();
        let [a, b] = self.edges[&(op, g)]
            .0
            .map(|v| self.pos[&Point::vert(op, v)]);
        // The edge's motion where `q` meets it (a tie's): the field is
        // linear, so a straight edge stays straight.
        Seg {
            a,
            b,
            rates: self.rates_at(op, self.pos[&q]),
        }
    }
}

impl CurveGeometry for Geom {
    fn spans(&self, operand: Operand, g: EdgeId) -> &[Span] {
        &self.edges[&(operand, g)].1
    }

    fn knot_rates(&self, q: Point, _: EdgeId, _: u32) -> [Rate; ORDERS] {
        self.rates(q.operand.other())
    }

    fn span_rates<S: Curved>(&self, q: Point, _: EdgeId, _: u32, t: &S) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        match q.operand.other() {
            A => [lit(self.d), lit(T[0]), lit(T[1]), lit(T[2])],
            B => std::array::from_fn(|_| lit(DVec3::ZERO)),
        }
    }
}

impl XGeometry for Geom {
    fn x_curve(&self, operand: Operand, g: EdgeId) -> XCurve<'_> {
        self.curve_of(operand, g.0)
    }

    fn edge_point_rates(&self, operand: Operand, g: EdgeId, at: EdgeAt) -> [Rate; ORDERS] {
        let (ends, spans) = &self.edges[&(operand, g)];
        let n = u32::try_from(spans.len().max(1)).expect("few spans");
        if let Some(i) = at.end(n) {
            return self.point(Point::vert(operand, ends[i])).rates;
        }
        if spans.is_empty() {
            // A straight edge's point: the field there.
            let [a, b] = ends.map(|v| self.pos[&Point::vert(operand, v)]);
            let t = dy(at.at);
            return self.rates_at(operand, a + (b - a) * t);
        }
        // Knots and inner points as the curved records read them.
        self.rates(operand)
    }

    fn edge_rates<S: Curved>(
        &self,
        operand: Operand,
        g: EdgeId,
        span: u32,
        t: &S,
    ) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        match operand {
            A => {
                // `d + (field·p)·ν` at the edge's point `p` there, blended
                // near the blend's vertex.
                let mut d = lit(self.d);
                if self.field != DVec3::ZERO || self.blend.is_some() {
                    let c = self.curve_of(operand, g.0).span(span, t).expect("a span");
                    let p = c.eval(t);
                    let f = lit(self.field);
                    let k = (0..3).fold(S::int(0, t), |s, i| s.add(&f[i].mul(&p[i])));
                    let nu = lit(NU);
                    d = [0, 1, 2].map(|i| d[i].add(&nu[i].mul(&k)));
                    if let Some(b) = self.blend {
                        // `λ` enclosed from the squared distance's
                        // enclosure (its square root rounded outwards by
                        // a part in 2⁴⁰).
                        let at = lit(b.at);
                        let v: [S; 3] = [0, 1, 2].map(|i| p[i].sub(&at[i]));
                        let d2 = (0..3).fold(S::int(0, t), |s, i| s.add(&v[i].mul(&v[i])));
                        let [lo2, hi2] = d2.enclose();
                        let lam = |d2: f64, up: bool| {
                            let k = 1.0 + if up { 1e-12 } else { -1e-12 };
                            let d = if d2 > 0.0 { d2.sqrt() * k } else { 0.0 };
                            (d / b.r - 1.0).clamp(0.0, 1.0)
                        };
                        let (lo, hi) = if hi2.is_finite() {
                            (lam(lo2, false), lam(hi2, true))
                        } else {
                            (0.0, 1.0)
                        };
                        let l = crate::perturb::between(lo, hi, t);
                        let one = S::int(1, t).sub(&l);
                        let bd = lit(b.d);
                        d = [0, 1, 2].map(|i| bd[i].mul(&one).add(&d[i].mul(&l)));
                    }
                }
                [d, lit(T[0]), lit(T[1]), lit(T[2])]
            }
            B => std::array::from_fn(|_| lit(DVec3::ZERO)),
        }
    }

    fn edge_line(&self, operand: Operand, g: EdgeId, near: DVec3) -> Seg {
        let [a, b] = self.edges[&(operand, g)]
            .0
            .map(|v| self.pos[&Point::vert(operand, v)]);
        Seg {
            a,
            b,
            rates: self.rates_at(operand, near),
        }
    }
}

/// A pass deciding in order on one thread.
#[derive(Default)]
struct Seq {
    cost: Cost,
}

impl Pass for Seq {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        keys.iter()
            .map(|k| {
                let (v, c) = decide(k);
                self.cost = self.cost.and(c);
                v
            })
            .collect()
    }
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn q3(p: DVec3) -> [Q; 3] {
    [q(p.x), q(p.y), q(p.z)]
}

fn qdot(a: &[Q; 3], b: &[Q; 3]) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn sgn(x: &Q) -> i64 {
    match x.partial_cmp(&Q::from(0u32)) {
        Some(Ordering::Greater) => 1,
        Some(Ordering::Less) => -1,
        _ => 0,
    }
}

/// A point of the lattice of the shadow frame: `x` along `r̂`, `y` along
/// `ν`, `z` along `UP`.
fn lat(x: f64, y: f64, z: f64) -> DVec3 {
    R_HAT * x + NU * y + UP * z
}

fn cp(x: DVec3, w: f64) -> [f64; 4] {
    [x.x, x.y, x.z, w]
}

/// The four records of edge `e` of `A` against edge `g` of `B`, decided,
/// and the square's side records, filled as box degrees ask for them.
struct Recs {
    shadows: Shadows,
    curved: CurvedRays,
    sides: RefCell<XSides>,
}

const E: u32 = 1;
const G: u32 = 2;
const X: XKey = XKey {
    e: EdgeId(E),
    g: EdgeId(G),
};

/// The curved band the records are decided at.
fn band() -> Band {
    Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band")
}

fn decide(geom: &Geom) -> Recs {
    try_decide(geom).expect("decided")
}

/// The records, `None` if one isn't decided at the records' levels.
fn try_decide(geom: &Geom) -> Option<Recs> {
    let mut shadows = Shadows::new();
    let mut curved = CurvedRays::new();
    let band = band();
    for (op, id) in [(A, E), (B, G)] {
        let spans = &geom.edges[&(op, EdgeId(id))].1;
        // The other edge's ends ask against this one.
        let other = op.other();
        let oid = if other == A { E } else { G };
        let keys: Vec<RayKey> = geom.edges[&(other, EdgeId(oid))]
            .0
            .iter()
            .map(|&v| RayKey {
                q: Point::vert(other, v),
                g: EdgeId(id),
            })
            .collect();
        if spans.is_empty() {
            shadows.fill_rays(keys, geom, &mut Seq::default()).ok()?;
        } else {
            curved
                .fill_chains(keys, &mut shadows, geom, band, &mut Seq::default())
                .ok()?;
        }
    }
    Some(Recs {
        shadows,
        curved,
        sides: RefCell::new(XSides::new()),
    })
}

impl Recs {
    fn rays<'a>(&'a self, geom: &Geom, q: Point, g: EdgeId) -> EdgeRays<'a> {
        if geom.edges[&(q.operand.other(), g)].1.is_empty() {
            EdgeRays::Line(self.shadows.ray(q, g).expect("decided"))
        } else {
            EdgeRays::Chain(self.curved.chain(q, g).expect("decided"))
        }
    }

    fn pair<'a>(&'a self, geom: &'a Geom) -> XPair<'a> {
        let [a, b] = geom.edges[&(A, EdgeId(E))].0.map(|v| Point::vert(A, v));
        let [g0, g1] = geom.edges[&(B, EdgeId(G))].0.map(|v| Point::vert(B, v));
        XPair {
            e: geom.curve_of(A, E),
            g: geom.curve_of(B, G),
            terms: XTerms {
                a_g: self.rays(geom, a, EdgeId(G)),
                b_g: self.rays(geom, b, EdgeId(G)),
                g0_e: self.rays(geom, g0, EdgeId(E)),
                g1_e: self.rays(geom, g1, EdgeId(E)),
            },
            shared: false,
        }
    }

    /// Fills the side or corner record `k`; `false` if it isn't decided
    /// at the records' levels.
    fn fill(&self, geom: &Geom, k: XWant) -> bool {
        let base = XBase {
            shadows: &self.shadows,
            curved: &self.curved,
        };
        let r = self
            .sides
            .borrow_mut()
            .fill([k], base, geom, band(), &mut Seq::default());
        r.is_ok()
    }

    /// Box `b`'s degree at `like`'s level, the side records it asks for
    /// filled.
    fn degree<S: Scalar>(
        &self,
        geom: &Geom,
        pair: &XPair<'_>,
        b: &XBox,
        like: &S,
    ) -> Result<XDegree, XDegreeError> {
        loop {
            let r = box_degree(X, pair, &self.sides.borrow(), b, like);
            match r {
                Err(XDegreeError::Need(k)) => {
                    if !self.fill(geom, *k) {
                        return Err(XDegreeError::Unsettled);
                    }
                }
                r => return r,
            }
        }
    }

    /// The pair's clusters at level 0, the side records they ask for
    /// filled.
    fn clusters(&self, geom: &Geom, pair: &XPair<'_>) -> Result<XClusters, XClusterError> {
        loop {
            let r = clusters_at(X, pair, &self.sides.borrow(), 0);
            match r {
                Err(XClusterError::Need(ks)) => {
                    for k in &ks {
                        if !self.fill(geom, *k) {
                            return Err(XClusterError::Need(ks));
                        }
                    }
                }
                r => return r,
            }
        }
    }
}

fn like() -> Bounded<f64> {
    Bounded::exact(0.0)
}

/// A random dyadic strictly inside `x`, at most four levels finer.
fn inside(rng: &mut Rng, x: Extent) -> Dyadic {
    let e = x.lo().exp().max(x.hi().exp()) + 1 + (rng.next_u64() % 4) as u32;
    let lo = x.lo().num() << (e - x.lo().exp());
    let hi = x.hi().num() << (e - x.hi().exp());
    Dyadic::new(lo + 1 + rng.next_u64() % (hi - lo - 1), e).expect("a dyadic")
}

/// A random partition of the pair's square: its cells, boxes split
/// `splits` times at random dyadics.
fn partition(rng: &mut Rng, spans: [u32; 2], splits: usize) -> Vec<XBox> {
    let mut boxes: Vec<XBox> = XBox::cells(spans).collect();
    for _ in 0..splits {
        let i = (rng.next_u64() % boxes.len() as u64) as usize;
        let b = boxes[i];
        let dir = if rng.next_u64().is_multiple_of(2) {
            Dir::U
        } else {
            Dir::V
        };
        let ext = if dir == Dir::U { b.t } else { b.s };
        let at = inside(rng, ext);
        if let Some(h) = b.split_at(dir, at) {
            boxes.splice(i..=i, h);
        }
    }
    boxes
}

/// The degrees of `boxes` summed, `None` if one isn't settled here.
fn sum(recs: &Recs, geom: &Geom, pair: &XPair<'_>, boxes: &[XBox]) -> Option<i64> {
    boxes
        .iter()
        .map(|b| recs.degree(geom, pair, b, &like()).ok().map(|d| d.degree))
        .sum()
}

/// The exact sign of the moved crossing of two straight edges' shadows,
/// `sign(T_g × T_e)` where they cross (0 where they don't); `None` at a
/// tie the moved configuration still has.
fn moved_crossing(geom: &Geom) -> Option<i64> {
    let pts = |op, id| {
        geom.edges[&(op, EdgeId(id))]
            .0
            .map(|v| geom.moved_shadow(Point::vert(op, v)))
    };
    let [a, b] = pts(A, E);
    let [g0, g1] = pts(B, G);
    let orient = |p: &[Q; 2], q: &[Q; 2], r: &[Q; 2]| {
        sgn(&((&q[0] - &p[0]) * (&r[1] - &p[1]) - (&q[1] - &p[1]) * (&r[0] - &p[0])))
    };
    let s = [
        orient(&a, &b, &g0),
        orient(&a, &b, &g1),
        orient(&g0, &g1, &a),
        orient(&g0, &g1, &b),
    ];
    if s.contains(&0) {
        return None;
    }
    if s[0] == s[1] || s[2] == s[3] {
        return Some(0);
    }
    // `T_g × T_e`: `g0 → g1` against `a → b`, the side of `b` from `g`'s line
    // relative to `a`'s.
    Some(s[3])
}

/// The moved crossing's parameters `(t, s)` on two straight edges whose
/// shadows aren't parallel.
fn moved_params(geom: &Geom) -> Option<[Q; 2]> {
    let pts = |op, id| {
        geom.edges[&(op, EdgeId(id))]
            .0
            .map(|v| geom.moved_shadow(Point::vert(op, v)))
    };
    let [a, b] = pts(A, E);
    let [g0, g1] = pts(B, G);
    let sub = |p: &[Q; 2], q: &[Q; 2]| [&p[0] - &q[0], &p[1] - &q[1]];
    let cross = |u: &[Q; 2], v: &[Q; 2]| &u[0] * &v[1] - &u[1] * &v[0];
    let (te, tg, d) = (sub(&b, &a), sub(&g1, &g0), sub(&g0, &a));
    let den = cross(&te, &tg);
    if den == 0u32 {
        return None;
    }
    Some([cross(&d, &tg) / &den, cross(&d, &te) / &den])
}

/// Whether `x` lies in the extent, `None` on its boundary.
fn in_extent(x: &Q, ext: Extent) -> Option<bool> {
    let lo = Q::try_from(dy(ext.lo())).expect("finite");
    let hi = Q::try_from(dy(ext.hi())).expect("finite");
    if *x == lo || *x == hi {
        return None;
    }
    Some(lo < *x && *x < hi)
}

/// Straight edges on a small lattice, ties included: the four terms give
/// the moved crossing, and every partition settled sums to them.
#[test]
fn straight_edges_count_their_moved_crossing() {
    let mut checked = 0;
    let mut summed = 0;
    let mut boxed = 0;
    for seed in varde_testing::seeds(400, 8000) {
        let mut rng = Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let mut pt = || lat(int(2), int(2), int(3));
        let (a, b, g0, g1) = (pt(), pt(), pt(), pt());
        let flat = |p: DVec3, q: DVec3| p.dot(R_HAT) == q.dot(R_HAT) && p.dot(NU) == q.dot(NU);
        if flat(a, b) || flat(g0, g1) {
            continue;
        }
        let mut geom = Geom {
            d: [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3],
            ..Geom::default()
        };
        geom.segment(A, E, (0, a), (1, b));
        geom.segment(B, G, (0, g0), (1, g1));
        let recs = decide(&geom);
        let pair = recs.pair(&geom);
        let x = x_count(&pair.terms);
        if let Some(want) = moved_crossing(&geom) {
            assert_eq!(x, want, "seed {seed}");
            checked += 1;
        }
        let mut rng = Rng::new(seed ^ 0x5eed);
        let params = moved_params(&geom);
        for splits in [0, 1, 3, 8] {
            let boxes = partition(&mut rng, [1, 1], splits);
            if let Some(s) = sum(&recs, &geom, &pair, &boxes) {
                assert_eq!(s, x, "seed {seed}, {splits} splits");
                summed += 1;
            }
            // Box by box: the crossing's sign in the box holding it.
            let Some([t, s]) = &params else { continue };
            for b in &boxes {
                let Ok(d) = recs.degree(&geom, &pair, b, &like()) else {
                    continue;
                };
                if let (Some(it), Some(is)) = (in_extent(t, b.t), in_extent(s, b.s)) {
                    let want = if it && is { x } else { 0 };
                    assert_eq!(d.degree, want, "seed {seed}: {b:?}");
                    boxed += 1;
                }
            }
        }
    }
    assert!(
        checked > 0 && summed > 0 && boxed > 0,
        "{checked} checked, {summed} summed, {boxed} boxes"
    );
}

/// The plan's first check in this frame: `e` along `r̂`, `g` across it
/// along `ν`, crossing once with `sign(T_g × T_e) = sign(ν × r̂) = −1`;
/// moved away along `r̂`, not at all.
#[test]
fn a_cross_counts_minus_one_and_apart_zero() {
    for (gx, want) in [(1.0, -1), (-5.0, 0)] {
        let mut geom = Geom {
            d: DVec3::new(0.25, -0.5, 0.125),
            ..Geom::default()
        };
        geom.segment(A, E, (0, lat(0.0, 0.0, 1.0)), (1, lat(2.0, 0.0, 1.0)));
        geom.segment(B, G, (0, lat(gx, -1.0, 0.0)), (1, lat(gx, 1.0, 0.0)));
        let recs = decide(&geom);
        let pair = recs.pair(&geom);
        assert_eq!(x_count(&pair.terms), want);
        let whole = XBox::cells([1, 1]).next().expect("a cell");
        let d = recs.degree(&geom, &pair, &whole, &like()).expect("settled");
        assert_eq!(d.degree, want);
        // Cut through the crossing's surroundings, away from it.
        let [l, r] = whole
            .split_at(Dir::U, Dyadic::new(3, 3).expect("3/8"))
            .expect("split");
        let [rl, rh] = r
            .split_at(Dir::V, Dyadic::new(1, 2).expect("¼"))
            .expect("split");
        let parts = [l, rl, rh];
        let degrees: Vec<i64> = parts
            .iter()
            .map(|b| {
                recs.degree(&geom, &pair, b, &like())
                    .expect("settled")
                    .degree
            })
            .collect();
        assert_eq!(degrees.iter().sum::<i64>(), want);
        if want != 0 {
            // The crossing is at `t = ½`, `s = ½`: in the upper right box.
            assert_eq!(degrees, vec![0, 0, want]);
        }
    }
}

/// A random conic span from `p0` to `p2` through the random `p1`.
fn conic(rng: &mut Rng, p0: DVec3, p2: DVec3, w: [f64; 2]) -> Span {
    let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
    let p1 = lat(int(3), int(3), int(2));
    let wm = [1.0 / 64.0, 0.5, 1.0, 3.0, 64.0][(rng.next_u64() % 5) as usize];
    [cp(p0, w[0]), cp(p1, wm), cp(p2, w[1])]
}

/// Curved edges of two spans each, ends and knots weighted unevenly: the
/// degrees of every settled partition sum to the four terms; the
/// crossings over a deep partition are all found.
#[test]
fn curved_partitions_sum_to_the_four_terms() {
    let mut summed = 0;
    let mut crossing = 0;
    let mut trials = 0;
    for seed in varde_testing::seeds(60, 800) {
        let mut rng = Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let mut pt = || lat(int(40), int(40), int(3));
        let (a, k, b, g0, h, g1) = (pt(), pt(), pt(), pt(), pt(), pt());
        let weights = [3.0, 1.0 / 3.0, 0.853_553_390_593_273_8, 1.0];
        let mut w = || weights[(rng.next_u64() % 4) as usize];
        let (w0, w1, w2, w3, w4, w5) = (w(), w(), w(), w(), w(), w());
        let mut geom = Geom {
            d: [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3],
            ..Geom::default()
        };
        let mut rng = Rng::new(seed ^ 0xc0);
        let e = vec![
            conic(&mut rng, a, k, [w0, w1]),
            conic(&mut rng, k, b, [w2, w3]),
        ];
        let g = vec![
            conic(&mut rng, g0, h, [w4, w5]),
            conic(&mut rng, h, g1, [w5, w4]),
        ];
        geom.curve(A, E, 0, 1, e);
        geom.curve(B, G, 0, 1, g);
        let recs = decide(&geom);
        let pair = recs.pair(&geom);
        let x = x_count(&pair.terms);
        for splits in [0, 4, 16, 48] {
            trials += 1;
            if let Some(s) = sum(&recs, &geom, &pair, &partition(&mut rng, [2, 2], splits)) {
                assert_eq!(s, x, "seed {seed}, {splits} splits");
                summed += 1;
                if x != 0 {
                    crossing += 1;
                }
            }
        }
    }
    assert!(
        summed * 3 > trials * 2 && crossing * 5 > trials,
        "{summed} summed, {crossing} crossing of {trials}"
    );
}

/// Splitting a settled box into settled halves keeps its degree.
#[test]
fn splits_keep_a_box_degree() {
    let mut kept = 0;
    let mut decided = 0;
    for seed in varde_testing::seeds(60, 800) {
        let mut rng = Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let mut pt = || lat(int(3), int(3), int(3));
        let (a, b, g0, g1) = (pt(), pt(), pt(), pt());
        let mut geom = Geom {
            d: DVec3::new(0.25, -0.5, 0.125),
            ..Geom::default()
        };
        let mut rng = Rng::new(seed ^ 0xab);
        geom.curve(A, E, 0, 1, vec![conic(&mut rng, a, b, [1.0, 3.0])]);
        geom.curve(B, G, 0, 1, vec![conic(&mut rng, g0, g1, [1.0 / 3.0, 1.0])]);
        // A span whose shadow folds back to a point of the other edge
        // (a shadow cusp at its end on the other's vertex) has no ray
        // record at any level: no degree to keep.
        let Some(recs) = try_decide(&geom) else {
            continue;
        };
        decided += 1;
        let pair = recs.pair(&geom);
        for b in partition(&mut rng, [1, 1], 6) {
            let Ok(d) = recs.degree(&geom, &pair, &b, &like()) else {
                continue;
            };
            for dir in [Dir::U, Dir::V] {
                let ext = if dir == Dir::U { b.t } else { b.s };
                let at = inside(&mut rng, ext);
                let Some(halves) = b.split_at(dir, at) else {
                    continue;
                };
                if let Some(s) = sum(&recs, &geom, &pair, &halves) {
                    assert_eq!(s, d.degree, "seed {seed}: {b:?} at {at:?}");
                    kept += 1;
                }
            }
        }
    }
    // A run of one seed may hold only geometry with no records.
    assert!(kept > 0 || decided == 0);
}

/// A box with no outer side whose two pieces lie far apart has degree 0,
/// from the first ray; the same box at the double-double level agrees.
#[test]
fn an_inner_box_apart_has_degree_zero() {
    let mut geom = Geom {
        d: DVec3::new(0.25, -0.5, 0.125),
        ..Geom::default()
    };
    geom.curve(
        A,
        E,
        0,
        1,
        vec![[
            cp(lat(0.0, 0.0, 0.0), 1.0),
            cp(lat(4.0, 1.0, 0.0), 2.0),
            cp(lat(8.0, 0.0, 0.0), 1.0),
        ]],
    );
    geom.curve(
        B,
        G,
        0,
        1,
        vec![[
            cp(lat(4.0, -4.0, 1.0), 1.0),
            cp(lat(5.0, 0.0, 1.0), 0.5),
            cp(lat(4.0, 4.0, 1.0), 1.0),
        ]],
    );
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    let quarter = |a, b| Extent::new(Dyadic::new(a, 2).expect("a"), Dyadic::new(b, 2).expect("b"));
    let b = XBox {
        cell: [0, 0],
        t: quarter(1, 2).expect("t"),
        s: quarter(1, 2).expect("s"),
    };
    let d = recs.degree(&geom, &pair, &b, &like()).expect("settled");
    assert_eq!(d.degree, 0);
    let dd = recs
        .degree(
            &geom,
            &pair,
            &b,
            &Bounded::exact(crate::num::dd::Dd::from(0.0)),
        )
        .expect("settled");
    assert_eq!(dd.degree, 0);
}

/// A box outside the cells, or a record of the wrong kind, is refused.
#[test]
fn data_that_does_not_fit_is_refused() {
    let mut geom = Geom::default();
    geom.segment(A, E, (0, lat(0.0, 0.0, 1.0)), (1, lat(2.0, 0.0, 1.0)));
    geom.segment(B, G, (0, lat(1.0, -1.0, 0.0)), (1, lat(1.0, 1.0, 0.0)));
    let recs = decide(&geom);
    let mut pair = recs.pair(&geom);
    let out = XBox {
        cell: [1, 0],
        t: Extent::WHOLE,
        s: Extent::WHOLE,
    };
    assert_eq!(
        recs.degree(&geom, &pair, &out, &like()),
        Err(XDegreeError::Shape)
    );
    let chain = ChainRec::default();
    pair.terms.b_g = EdgeRays::Chain(&chain);
    let whole = XBox::cells([1, 1]).next().expect("a cell");
    assert_eq!(
        recs.degree(&geom, &pair, &whole, &like()),
        Err(XDegreeError::Shape)
    );
}

/// Reverses a chain of spans: the same curve run backwards.
fn reversed(spans: &[Span]) -> Vec<Span> {
    spans.iter().rev().map(|s| [s[2], s[1], s[0]]).collect()
}

/// Two-span chains on a small lattice, knots of `e` and `g` often on one
/// shadow point or one `r̂` line, weights 1/64 to 64: every settled
/// partition sums to the four terms, the whole cells included, and
/// running either edge backwards negates both.
#[test]
fn knot_ties_never_give_a_wrong_degree() {
    let mut summed = 0;
    let mut tied = 0;
    for seed in varde_testing::seeds(300, 4000) {
        let mut rng = Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let mut pt = |z: f64| lat(int(2), int(2), z);
        let (a, k, b) = (pt(1.0), pt(1.0), pt(1.0));
        let (g0, mut h, g1) = (pt(-1.0), pt(-1.0), pt(-1.0));
        if seed % 2 == 0 {
            // `g`'s knot under `e`'s.
            h = k - UP * 2.0;
            tied += 1;
        }
        let ws = [1.0 / 64.0, 0.5, 1.0, 3.0, 64.0];
        let mut rng = Rng::new(seed ^ 0x7e);
        let mut w = || ws[(rng.next_u64() % 5) as usize];
        let (w0, w1, w2, w3) = (w(), w(), w(), w());
        let mut rng = Rng::new(seed ^ 0x77);
        let e = vec![
            conic(&mut rng, a, k, [1.0, w0]),
            conic(&mut rng, k, b, [w1, 1.0]),
        ];
        let g = vec![
            conic(&mut rng, g0, h, [w2, 1.0]),
            conic(&mut rng, h, g1, [w3, w2]),
        ];
        let d = [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3];
        let mut xs = Vec::new();
        for (re, rg) in [(false, false), (true, false), (false, true)] {
            let mut geom = Geom {
                d,
                ..Geom::default()
            };
            let flip = |r: bool, s: &Vec<Span>| if r { reversed(s) } else { s.clone() };
            geom.curve(A, E, 0, 1, flip(re, &e));
            geom.curve(B, G, 0, 1, flip(rg, &g));
            let Some(recs) = try_decide(&geom) else {
                break;
            };
            let pair = recs.pair(&geom);
            let x = x_count(&pair.terms);
            xs.push(x);
            let mut rng = Rng::new(seed ^ 0x99);
            for splits in [0, 3, 12, 40] {
                if let Some(s) = sum(&recs, &geom, &pair, &partition(&mut rng, [2, 2], splits)) {
                    assert_eq!(s, x, "seed {seed} {re} {rg}, {splits} splits");
                    summed += 1;
                }
            }
        }
        if let [x, xe, xg] = xs[..] {
            assert_eq!(xe, -x, "seed {seed}: e backwards");
            assert_eq!(xg, -x, "seed {seed}: g backwards");
        }
    }
    assert!(summed > 0 && tied > 0, "{summed} summed");
}

/// A straight span from `p` to `q`, its middle weighted `w`: a line
/// along a reparametrised parameter.
fn straight(p: DVec3, q: DVec3, w: f64) -> Span {
    [cp(p, 1.0), cp((p + q) * 0.5, w), cp(q, 1.0)]
}

/// Two-span chains of straight spans on a small lattice, `g`'s knot
/// often on `e`'s knot's shadow (and ends and knots on each other's `r̂`
/// lines, weights skewed): every whole cell settles, its common corner
/// read from the knots' corner record and its inner sides from the
/// knots' side records, its degree the moved crossing of its two spans,
/// and the four cells sum to the four terms.
#[test]
fn cells_at_coinciding_knots_count_their_moved_crossing() {
    let (mut cells, mut at_knot, mut read) = (0, 0, 0);
    for seed in varde_testing::seeds(400, 6000) {
        let mut rng = Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let mut pt = |z: f64| lat(int(2), int(2), z);
        let (a, k, b) = (pt(1.0), pt(1.0), pt(1.0));
        let (g0, mut h, g1) = (pt(-1.0), pt(-1.0), pt(-1.0));
        if seed % 4 != 3 {
            h = k - UP * 2.0;
        }
        let ws = [1.0 / 64.0, 0.5, 1.0, 3.0, 64.0];
        let mut w = || ws[(rng.next_u64() % 5) as usize];
        let flat = |p: DVec3, q: DVec3| p.dot(R_HAT) == q.dot(R_HAT) && p.dot(NU) == q.dot(NU);
        if flat(a, k) || flat(k, b) || flat(g0, h) || flat(h, g1) {
            continue;
        }
        let d = [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3];
        let mut geom = Geom {
            d,
            ..Geom::default()
        };
        geom.curve(A, E, 0, 1, vec![straight(a, k, w()), straight(k, b, w())]);
        geom.curve(B, G, 0, 1, vec![straight(g0, h, w()), straight(h, g1, w())]);
        let Some(recs) = try_decide(&geom) else {
            continue;
        };
        let pair = recs.pair(&geom);
        let (es, gs) = ([(a, k), (k, b)], [(g0, h), (h, g1)]);
        let mut total = 0;
        for c in XBox::cells([2, 2]) {
            let got = recs
                .degree(&geom, &pair, &c, &like())
                .unwrap_or_else(|e| panic!("seed {seed}: cell {:?}: {e:?}", c.cell));
            total += got.degree;
            cells += 1;
            let ([p, q], [r, s]) = (
                <[DVec3; 2]>::from(es[c.cell[0] as usize]),
                <[DVec3; 2]>::from(gs[c.cell[1] as usize]),
            );
            let mut seg = Geom {
                d,
                ..Geom::default()
            };
            seg.segment(A, E, (0, p), (1, q));
            seg.segment(B, G, (2, r), (3, s));
            if let Some(want) = moved_crossing(&seg) {
                assert_eq!(got.degree, want, "seed {seed}: cell {:?}", c.cell);
                if seed % 4 != 3 && want != 0 {
                    at_knot += 1;
                }
            }
        }
        assert_eq!(total, x_count(&pair.terms), "seed {seed}");
        read += usize::from(!recs.sides.borrow().side_table().is_empty());
    }
    assert!(
        cells > 0 && at_knot > 0 && read > 0,
        "{cells} cells, {at_knot} crossing at a knot, {read} reading side records"
    );
}

/// Near the coordinate limit (`2¹⁹`, lattice steps of `2¹⁷`) and with
/// negative zeros, straight and curved: settled boxes still hold the
/// moved crossing (at either level), and partitions sum to the four
/// terms.
#[test]
fn far_and_negative_zero_coordinates_count_alike() {
    let mut boxed = 0;
    let mut summed = 0;
    for seed in varde_testing::seeds(200, 3000) {
        let mut rng = Rng::new(seed);
        let scale = [1.0, 131_072.0][seed as usize % 2];
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        // `−0.0` for every zero coordinate.
        let nz = |p: DVec3| p.map(|c| if c == 0.0 { -0.0 } else { c });
        let mut pt = |z: f64| nz(lat(int(4), int(4), z) * scale);
        let (a, b, g0, g1) = (pt(1.0), pt(1.0), pt(-1.0), pt(-1.0));
        let flat = |p: DVec3, q: DVec3| p.dot(R_HAT) == q.dot(R_HAT) && p.dot(NU) == q.dot(NU);
        if flat(a, b) || flat(g0, g1) {
            continue;
        }
        let d = [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3];
        let mut geom = Geom {
            d,
            ..Geom::default()
        };
        geom.segment(A, E, (0, a), (1, b));
        geom.segment(B, G, (0, g0), (1, g1));
        let Some(recs) = try_decide(&geom) else {
            continue;
        };
        let pair = recs.pair(&geom);
        let x = x_count(&pair.terms);
        if let Some(want) = moved_crossing(&geom) {
            assert_eq!(x, want, "seed {seed}");
        }
        let params = moved_params(&geom);
        let mut rng = Rng::new(seed ^ 0xfa);
        for splits in [0, 2, 6] {
            let boxes = partition(&mut rng, [1, 1], splits);
            if let Some(s) = sum(&recs, &geom, &pair, &boxes) {
                assert_eq!(s, x, "seed {seed}, {splits} splits");
                summed += 1;
            }
            let Some([t, s]) = &params else { continue };
            for b in &boxes {
                let Ok(dg) = recs.degree(&geom, &pair, b, &like()) else {
                    continue;
                };
                // The double-double level agrees where it settles.
                let dd = Bounded::exact(crate::num::dd::Dd::from(0.0));
                if let Ok(hi) = recs.degree(&geom, &pair, b, &dd) {
                    assert_eq!(hi.degree, dg.degree, "seed {seed}: {b:?} at dd");
                }
                if let (Some(it), Some(is)) = (in_extent(t, b.t), in_extent(s, b.s)) {
                    assert_eq!(
                        dg.degree,
                        if it && is { x } else { 0 },
                        "seed {seed}: {b:?}"
                    );
                    boxed += 1;
                }
            }
        }
        // The same ends joined by curved spans of extreme weights.
        let ws = [1.0 / 64.0, 64.0];
        let mut geom = Geom {
            d,
            ..Geom::default()
        };
        let mut rng = Rng::new(seed ^ 0xcc);
        let mid = |p: DVec3, q: DVec3, rng: &mut Rng| {
            let m = nz((p + q) * 0.5 + lat(1.0, -1.0, 0.0) * scale);
            [
                cp(p, 1.0),
                cp(m, ws[(rng.next_u64() % 2) as usize]),
                cp(q, 1.0),
            ]
        };
        geom.curve(A, E, 0, 1, vec![mid(a, b, &mut rng)]);
        geom.curve(B, G, 0, 1, vec![mid(g0, g1, &mut rng)]);
        let Some(recs) = try_decide(&geom) else {
            continue;
        };
        let pair = recs.pair(&geom);
        let x = x_count(&pair.terms);
        for splits in [0, 4, 12] {
            if let Some(s) = sum(&recs, &geom, &pair, &partition(&mut rng, [1, 1], splits)) {
                assert_eq!(s, x, "seed {seed} curved, {splits} splits");
                summed += 1;
            }
        }
    }
    assert!(boxed > 0 && summed > 0, "{boxed} boxes, {summed} summed");
}

mod clusters;
mod shared;
