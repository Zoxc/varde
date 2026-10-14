//! The curved ray records against the moved configuration they stand for:
//! every span record's crossings are the exact ones of the operands really
//! moved (in rationals, `A` translated by tiny powers of two times the
//! motions' rates); random conic spans with weights from 1/64 to 64, rays
//! through span ends and knots with uneven weights (ends of weight 3 and
//! ⅓, the weight ≈ 0.854 of a split quarter circle, knots weighted
//! differently on their two sides), span ends exactly on the ray line,
//! rays through the span itself, tangent to it away from the point and at
//! it, spans lying along the ray line; a closed chain's records
//! sum to its winding number, which a ray in another direction counts
//! too; an edge stored the other way reads its records negated; each
//! record decided once, replayed alike; a tie no level reaches the target
//! of escalates past the last level; the height keys read through the
//! decided tables.

use std::collections::BTreeMap;

use glam::DVec3;
use malachite::Rational as Q;
use malachite::base::num::arithmetic::traits::PowerOf2;

use super::*;
use crate::ids::VertId;
use crate::perturb::{Seg, T, UP, rates_a};
use crate::records::feet::{EdgeCurve, FeetGeometry};

use Operand::{A, B};

/// Curved edges of either operand, and points of both.
#[derive(Default)]
struct Geom {
    pos: BTreeMap<Point, DVec3>,
    edges: BTreeMap<(Operand, EdgeId), ([VertId; 2], Vec<Span>)>,
    /// `A`'s direction field: one vector, so `A` moves as a translation.
    d: DVec3,
    sigma: Option<Sign>,
    /// Every edge's near radius, and every point's.
    r_g: f64,
}

impl Geom {
    fn with(d: DVec3, sigma: Sign) -> Geom {
        Geom {
            d,
            sigma: Some(sigma),
            ..Geom::default()
        }
    }

    fn sigma(&self) -> Sign {
        self.sigma.unwrap_or(Sign::Pos)
    }

    /// Curved edge `id` of `op` from vertex `v` to `w` along `spans`,
    /// stored from the lower id; its vertices at the chain's end control
    /// points.
    fn edge(&mut self, op: Operand, id: u32, v: u32, w: u32, spans: &[Span]) -> EdgeId {
        let (ends, spans) = if v <= w {
            ([v, w], spans.to_vec())
        } else {
            ([w, v], reversed(spans))
        };
        let last = spans.len() - 1;
        for (vid, at) in [
            (ends[0], point_of(&spans[0][0])),
            (ends[1], point_of(&spans[last][2])),
        ] {
            if let Some(old) = self.pos.insert(Point::vert(op, VertId(vid)), at) {
                assert_eq!(old, at, "a vertex at two places");
            }
        }
        let e = EdgeId(id);
        let ends = ends.map(VertId);
        assert!(self.edges.insert((op, e), (ends, spans)).is_none());
        e
    }

    fn query(&mut self, op: Operand, id: u32, at: DVec3) -> Point {
        let p = Point::vert(op, VertId(id));
        self.pos.insert(p, at);
        p
    }

    fn rates(&self, op: Operand) -> [Rate; ORDERS] {
        match op {
            A => rates_a(self.sigma(), self.d),
            B => [Rate::ZERO; ORDERS],
        }
    }

    /// `A`'s offset in the moved configuration,
    /// `δ·(σ_A·d + η·T₂ + η²·T₃ + η³·T₄)` with `δ = 2⁻²⁰⁰`, `η = 2⁻²⁰`,
    /// so each motion decides only where the ones before leave a tie, as
    /// the orders do.
    fn offset(&self, op: Operand) -> [Q; 3] {
        let mut out = [0, 1, 2].map(|_| Q::from(0u32));
        if op == B {
            return out;
        }
        let sigma = match self.sigma() {
            Sign::Pos => 1.0,
            Sign::Neg => -1.0,
        };
        for (k, r) in (0i64..).zip([self.d * sigma, T[0], T[1], T[2]]) {
            let eps = Q::power_of_2(-200 - 20 * k);
            let r = q3(r);
            for i in 0..3 {
                out[i] = &out[i] + &eps * &r[i];
            }
        }
        out
    }

    fn moved(&self, op: Operand, at: DVec3) -> [Q; 3] {
        let o = self.offset(op);
        let at = q3(at);
        [0, 1, 2].map(|i| &at[i] + &o[i])
    }

    /// A span moved, as homogeneous points: `(w·(x + Δ), w)`, exactly.
    fn moved_span(&self, op: Operand, s: &Span) -> [[Q; 4]; 3] {
        let o = self.offset(op);
        s.map(|p| {
            let w = q(p[3]);
            let x = |k: usize| &w * (q(p[k]) + &o[k]);
            [x(0), x(1), x(2), w]
        })
    }
}

impl ShadowGeometry for Geom {
    fn point(&self, p: Point) -> Moving {
        Moving {
            at: self.pos[&p],
            rates: self.rates(p.operand),
        }
    }

    fn near_radius(&self, _: Point) -> f64 {
        self.r_g
    }

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)].0
    }

    fn line(&self, _: Point, _: EdgeId) -> Seg {
        unreachable!("only curved edges here")
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
            A => {
                let own = match self.sigma() {
                    Sign::Pos => self.d,
                    Sign::Neg => -self.d,
                };
                [lit(own), lit(T[0]), lit(T[1]), lit(T[2])]
            }
            B => std::array::from_fn(|_| lit(DVec3::ZERO)),
        }
    }
}

impl FeetGeometry for Geom {
    fn curve(&self, operand: Operand, g: EdgeId) -> EdgeCurve<'_> {
        let (ends, spans) = &self.edges[&(operand, g)];
        EdgeCurve::Spans {
            spans,
            closed: ends[0] == ends[1],
        }
    }

    fn near_radius_of(&self, _: Operand, _: EdgeId) -> f64 {
        self.r_g
    }
}

/// A pass deciding in order on one thread, counting the keys it decided
/// and the cost.
#[derive(Default)]
struct Seq {
    decided: usize,
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
                self.decided += 1;
                let (v, c) = decide(k);
                self.cost = self.cost.and(c);
                v
            })
            .collect()
    }
}

fn reversed(spans: &[Span]) -> Vec<Span> {
    spans.iter().rev().map(|s| [s[2], s[1], s[0]]).collect()
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn q3(p: DVec3) -> [Q; 3] {
    [p.x, p.y, p.z].map(q)
}

fn qdot(a: &[Q; 3], b: &[Q; 3]) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn qcross(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn sgn(x: &Q) -> i8 {
    if *x > 0u32 {
        1
    } else if *x < 0u32 {
        -1
    } else {
        0
    }
}

/// The sign of `x + y·√d`, `d > 0`.
fn sgn_root(x: &Q, y: &Q, d: &Q) -> i8 {
    let (sx, sy) = (sgn(x), sgn(y));
    if sx >= 0 && sy >= 0 {
        return if sx == 0 && sy == 0 { 0 } else { 1 };
    }
    if sx <= 0 && sy <= 0 {
        return -1;
    }
    // Opposite signs: the larger square wins.
    let c = sgn(&(x * x - y * y * d));
    if sx > 0 { c } else { -c }
}

/// The ray direction of `op`'s points.
fn ray_dir(op: Operand) -> [Q; 3] {
    match op {
        A => q3(-R_HAT),
        B => q3(R_HAT),
    }
}

/// The signed crossings, in order, of the moved ray from `qm` along `rho`
/// with span `i` of edge `e` (of `op`), each `sign(UP·(ρ × T))`. Exact,
/// solved directly (no records, no perturbation); panics at a tie the
/// moved configuration still has.
fn oracle(g: &Geom, op: Operand, e: EdgeId, i: usize, qm: &[Q; 3], rho: &[Q; 3]) -> Vec<i8> {
    let s = &g.edges[&(op, e)].1[i];
    let n = qcross(&q3(UP), rho);
    let m = g.moved_span(op, s);
    let mut out = Vec::new();
    // The span's equations: `g` across the ray line, `h` along it.
    let lin = |a: &[Q; 3]| {
        let c = qdot(a, qm);
        m.each_ref()
            .map(|p| qdot(a, &[p[0].clone(), p[1].clone(), p[2].clone()]) - &c * &p[3])
    };
    let power = |b: [Q; 3]| {
        let [b0, b1, b2] = b;
        (
            &b0 - Q::from(2u32) * &b1 + &b2,
            Q::from(2u32) * (&b1 - &b0),
            b0,
        )
    };
    let (ga, gb, gc) = power(lin(&n));
    let (ha, hb, hc) = power(lin(rho));
    assert!(
        gc != 0u32 && &ga + &gb + &gc != 0u32,
        "a span end on the ray line"
    );
    if ga == 0u32 {
        if gb != 0u32 {
            let t = -(&gc / &gb);
            if t > 0u32 && t < 1u32 {
                let h = &ha * &t * &t + &hb * &t + &hc;
                assert!(sgn(&h) != 0, "the ray through the span");
                if h > 0u32 {
                    out.push(sgn(&gb));
                }
            }
        }
    } else {
        let d = &gb * &gb - Q::from(4u32) * &ga * &gc;
        if d == 0u32 {
            // A double root: no crossing, a tie only on the span.
            let t = -(&gb / (Q::from(2u32) * &ga));
            assert!(t <= 0u32 || t >= 1u32, "the ray line tangent to the span");
        }
        if d > 0u32 {
            let sa = sgn(&ga);
            // `t = (−b + e·√D)/(2a)`, the smaller first.
            for e in [-sa, sa] {
                let eq = Q::from(e);
                let at = |r: &Q| sgn_root(&(-&gb - Q::from(2u32) * &ga * r), &eq, &d) * sa;
                if at(&Q::from(0u32)) <= 0 || at(&Q::from(1u32)) >= 0 {
                    continue;
                }
                // `h(t) = α·t + β` on the root.
                let alpha = &hb - &ha * &gb / &ga;
                let beta = &hc - &ha * &gc / &ga;
                let x = -(&alpha * &gb) + Q::from(2u32) * &ga * &beta;
                let h = sgn_root(&x, &(&eq * &alpha), &d) * sa;
                assert!(h != 0, "the ray through the span");
                if h > 0 {
                    // `g′(t) = e·√D`.
                    out.push(e);
                }
            }
        }
    }
    out
}

/// The crossings' signs of `q`'s record against span `i` of `e`.
fn signs(r: &CurvedRays, q: Point, e: EdgeId, i: usize) -> Vec<i8> {
    let rec = r.span(q, e, i as u32).expect("stored");
    rec.crossings.iter().map(|c| c.sign.as_i8()).collect()
}

/// The curved band at the first rungs: no tie band, a target of `2⁻³⁰`.
fn band() -> Band {
    Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band")
}

/// Fills `q`'s records against every span of every edge of `op`'s.
fn fill_all(r: &mut CurvedRays, s: &mut Shadows, g: &Geom, qs: &[Point]) {
    let mut keys = Vec::new();
    for &q in qs {
        for ((op, e), (_, spans)) in &g.edges {
            if *op == q.operand.other() {
                keys.extend((0..spans.len() as u32).map(|span| SpanKey { q, g: *e, span }));
            }
        }
    }
    r.fill(keys, s, g, band(), &mut Seq::default())
        .expect("decided");
}

/// Checks every record of `q` against the oracle: crossing by crossing
/// (`each`), or summed (where a cluster's crossings cancel).
fn check(r: &CurvedRays, g: &Geom, q: Point, each: bool, what: &str) {
    let op = q.operand.other();
    let qm = g.moved(q.operand, g.pos[&q]);
    for ((o, e), (_, spans)) in &g.edges {
        if *o != op {
            continue;
        }
        for i in 0..spans.len() {
            let want = oracle(g, op, *e, i, &qm, &ray_dir(q.operand));
            let got = signs(r, q, *e, i);
            for c in &r.span(q, *e, i as u32).expect("stored").crossings {
                let [lo, hi] = c.t;
                assert!(
                    0.0 <= lo && lo <= hi && hi <= 1.0,
                    "{what}: {q:?} against {e:?} span {i}: a crossing at {:?}",
                    c.t
                );
            }
            if each {
                assert_eq!(got, want, "{what}: {q:?} against {e:?} span {i}");
            } else {
                let sum = |v: &[i8]| v.iter().map(|&x| i32::from(x)).sum::<i32>();
                assert_eq!(
                    sum(&got),
                    sum(&want),
                    "{what}: {q:?} against {e:?} span {i}"
                );
            }
        }
    }
}

/// A point of the lattice of the shadow frame: `x` along `r̂`, `y` along
/// `ν`, `z` along `UP`.
fn lat(x: f64, y: f64, z: f64) -> DVec3 {
    R_HAT * x + NU * y + UP * z
}

/// The control point `x` with weight `w`, as stored.
fn cp(x: DVec3, w: f64) -> [f64; 4] {
    [x.x, x.y, x.z, w]
}

/// The weight `≈ 0.854` at the knot and inner control points of a quarter
/// circle (end weights 1, middle `cos 45°`) split at `t = ½`:
/// `(1 + cos 45°)/2`, rounded.
const SPLIT: f64 = 0.853_553_390_593_273_8;

/// End and knot weights: 3 and ⅓, [`SPLIT`], 1.
const UNEVEN: [f64; 4] = [3.0, 1.0 / 3.0, SPLIT, 1.0];

const DIRECTIONS: [DVec3; 5] = [DVec3::new(0.25, -0.5, 0.125), DVec3::ZERO, R_HAT, NU, UP];

fn sigmas(seed: u64) -> Sign {
    if seed.is_multiple_of(2) {
        Sign::Pos
    } else {
        Sign::Neg
    }
}

/// Random conic spans (weights from 1/64 to 64) against random points of
/// both operands: every crossing as the moved configuration's.
#[test]
fn random_spans_cross_as_the_moved_configuration() {
    for seed in varde_testing::seeds(64, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let mut g = Geom::with(DIRECTIONS[seed as usize % 5], sigmas(seed / 5));
        let weight = |r: &mut varde_testkit::rng::Rng| {
            if r.next_u64().is_multiple_of(3) {
                1.0
            } else {
                r.log_range(1.0 / 64.0, 64.0)
            }
        };
        let mut rng = varde_testkit::rng::Rng::new(seed ^ 0x5eed);
        for (op, base) in [(A, 0u32), (B, 100)] {
            for k in 0..3u32 {
                let pts: Vec<DVec3> = (0..3)
                    .map(|_| {
                        let mut c = || int(4) / 4.0;
                        lat(c(), c() / 8.0, c())
                    })
                    .collect();
                let s: Span = [0, 1, 2].map(|i| cp(pts[i], weight(&mut rng)));
                g.edge(op, base + k, base + 2 * k, base + 2 * k + 1, &[s]);
            }
        }
        let qs: Vec<Point> = (0..6u32)
            .map(|i| {
                let op = if i % 2 == 0 { A } else { B };
                let at = lat(
                    rng.range(-1.5, 1.5),
                    rng.range(-1.5, 1.5) / 8.0,
                    rng.range(-1.0, 1.0),
                );
                g.query(op, 1000 + i, at)
            })
            .collect();
        let (mut r, mut s) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r, &mut s, &g, &qs);
        for &q in &qs {
            check(&r, &g, q, true, &format!("seed {seed}"));
        }
        r.replay(&s, &g, band()).expect("replays");
    }
}

/// A circle in the shadow frame, `n` spans of `360°/n` from angle
/// `start`, centre `c`, radius `rad` (in the frame's units): the spans'
/// end weights 1, middle ones `cos(θ/2)` rounded.
fn circle(c: DVec3, rad: f64, n: usize, start: f64) -> Vec<Span> {
    let step = std::f64::consts::TAU / n as f64;
    let at = |a: f64, r: f64| c + lat(r * libm::cos(a), r * libm::sin(a) / 8.0, 0.0);
    let w = libm::cos(step / 2.0);
    let ends: Vec<[f64; 4]> = (0..=n)
        .map(|k| {
            // Dyadic ends so ties can be placed on them exactly.
            let p = at(start + step * k as f64, rad);
            let r = |x: f64| (x * 64.0).round() / 64.0;
            cp(DVec3::new(r(p.x), r(p.y), r(p.z)), 1.0)
        })
        .collect();
    let mut ends = ends;
    ends[n] = ends[0];
    (0..n)
        .map(|k| {
            let mid = at(start + step * (k as f64 + 0.5), rad / w);
            [ends[k], cp(mid, w), ends[k + 1]]
        })
        .collect()
}

/// The span `s` with end weights 1 reparametrised to end weights `a` and
/// `b`: the same conic (up to the middle weight `w₁·√(ab)` rounded).
fn reweighted(s: &Span, a: f64, b: f64) -> Span {
    let w1 = s[1][3] * libm::sqrt(a * b);
    [
        [s[0][0], s[0][1], s[0][2], a],
        [s[1][0], s[1][1], s[1][2], w1],
        [s[2][0], s[2][1], s[2][2], b],
    ]
}

/// A closed chain of curved edges: circles split into two edges of two
/// spans, end and knot weights uneven (3, ⅓, ≈ 0.854, 1, differing at a
/// knot from one side to the other), joined at vertices and knots, against
/// points inside, outside,
/// on the knots' and vertices' `r̂` lines and columns; the records sum to
/// the winding number, which a ray along `ν` counts in the moved
/// configuration too; the edges stored the other way read the records
/// negated, span for span from the other end.
#[test]
fn chains_wind_as_the_curve_does() {
    for seed in varde_testing::seeds(24, 512) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let sigma = sigmas(seed);
        let d = DIRECTIONS[(seed / 2) as usize % 5];
        let op = if seed % 3 == 0 { A } else { B };
        let c = lat(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0));
        let c = DVec3::new(
            (c.x * 16.0).round() / 16.0,
            (c.y * 16.0).round() / 16.0,
            (c.z * 16.0).round() / 16.0,
        );
        let rad = 1.0 + rng.range(0.0, 2.0);
        // End weights from [`UNEVEN`] per span, so most knots weigh
        // differently on their two sides.
        let spans: Vec<Span> = circle(c, rad, 4, rng.range(0.0, 1.0))
            .iter()
            .map(|s| {
                let mut pick = || UNEVEN[(rng.next_u64() % 4) as usize];
                reweighted(s, pick(), pick())
            })
            .collect();
        let joints: Vec<DVec3> = spans.iter().map(|s| point_of(&s[0])).collect();
        let mut qs_at = Vec::new();
        for &j in &joints {
            for (l, m) in [(0.5, 0.0), (-0.5, 1.0), (0.0, 0.0), (0.0, -2.0), (3.0, 1.0)] {
                qs_at.push(j + R_HAT * l + UP * m);
            }
        }
        qs_at.push(c);
        qs_at.push(c + UP * 3.0);
        qs_at.push(c + lat(rad * 3.0, 0.0, 0.0));
        let build = |flip: bool| {
            let mut g = Geom::with(d, sigma);
            let ids = |v: u32| if flip { 10 - v } else { v };
            let e0 = g.edge(op, 0, ids(0), ids(1), &spans[0..2]);
            let e1 = g.edge(op, 1, ids(1), ids(0), &spans[2..4]);
            let qs: Vec<Point> = qs_at
                .iter()
                .enumerate()
                .map(|(i, &at)| g.query(op.other(), 100 + i as u32, at))
                .collect();
            // Each edge with the sense the loop runs it in.
            let sense = |a: u32, b: u32| ids(a) < ids(b);
            (g, qs, [(e0, sense(0, 1)), (e1, sense(1, 0))])
        };
        let (g, qs, cycle) = build(false);
        let (g2, qs2, cycle2) = build(true);
        let (mut r, mut s) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r, &mut s, &g, &qs);
        let (mut r2, mut s2) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r2, &mut s2, &g2, &qs2);
        let nu = q3(NU);
        for (&q, &q2) in qs.iter().zip(&qs2) {
            check(&r, &g, q, true, &format!("seed {seed}"));
            let qm = g.moved(q.operand, g.pos[&q]);
            let mut sum = 0;
            let mut across = 0;
            for &(e, forward) in &cycle {
                let n = g.edges[&(op, e)].1.len();
                for i in 0..n {
                    let c = r.span(q, e, i as u32).expect("stored").count();
                    let o: i32 = oracle(&g, op, e, i, &qm, &nu)
                        .iter()
                        .map(|&x| i32::from(x))
                        .sum();
                    sum += if forward { c } else { -c };
                    across += if forward { o } else { -o };
                }
            }
            assert_eq!(sum, across, "seed {seed}, {q:?}");
            let inside = (g.pos[&q] - c).length() < 0.5;
            if inside {
                assert_eq!(sum.abs(), 1, "seed {seed}, {q:?}");
            }
            for (&(e, _), &(e2, _)) in cycle.iter().zip(&cycle2) {
                let n = g.edges[&(op, e)].1.len();
                for i in 0..n {
                    let c = r.span(q, e, i as u32).expect("stored").count();
                    let c2 = r2.span(q2, e2, (n - 1 - i) as u32).expect("stored").count();
                    assert_eq!(c, -c2, "seed {seed}, {q:?}, {e:?} span {i}");
                }
            }
        }
        r.replay(&s, &g, band()).expect("replays");
    }
}

/// Spans whose point at `t = ½` is exact on lattice points: weights
/// summing, as `w₀ + 2w₁ + w₂`, to a power of two.
const EXACT_MIDDLE: [[f64; 3]; 4] = [
    [1.0, 1.0, 1.0],
    [1.0, 0.5, 2.0],
    [2.0, 1.0, 4.0],
    [1.0, 3.0, 1.0],
];

/// The span's point at `t = ½`, exactly for [`EXACT_MIDDLE`] weights.
fn middle(s: &Span) -> DVec3 {
    let w = s[0][3] + 2.0 * s[1][3] + s[2][3];
    let c = |k: usize| (s[0][3] * s[0][k] + 2.0 * s[1][3] * s[1][k] + s[2][3] * s[2][k]) / w;
    DVec3::new(c(0), c(1), c(2))
}

/// Rays through the span itself: points on the span's column at `t = ½`
/// (where the span crosses their `r̂` line under them), on its tangent
/// line there when it runs along `r̂` (a tangency away from the point:
/// two crossings that cancel, or none), and at that tangent point itself
/// (one crossing on the ray or none, by the side the point moves to); and
/// points on the ray lines through its ends. Every record is the moved
/// configuration's.
#[test]
fn ties_on_the_span_follow_the_moved_configuration() {
    for seed in varde_testing::seeds(48, 1024) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let mut g = Geom::with(DIRECTIONS[seed as usize % 5], sigmas(seed / 5));
        let op = if seed % 2 == 0 { A } else { B };
        let w = EXACT_MIDDLE[(seed / 2) as usize % 4];
        // Tangent along `r̂` at `t = ½` for symmetric weights with the
        // ends on one `r̂` line.
        let tangent = w[0] == w[2] && seed % 3 != 0;
        let y0 = int(3);
        // Off the tangent case, the ends on different `r̂` lines (else
        // symmetric weights would make it one).
        let y2 = if tangent { y0 } else { y0 + 1.0 + int(1) * 2.0 };
        let x = [
            lat(int(4), y0, int(2)),
            lat(int(4), int(3) + 0.5, int(2)),
            lat(int(4) + 9.0, y2, int(2)),
        ];
        let s: Span = [0, 1, 2].map(|i| cp(x[i], w[i]));
        let e = g.edge(op, 0, 1, 2, &[s]);
        let mid = middle(&g.edges[&(op, e)].1[0]);
        let mut qs = Vec::new();
        let mut id = 100;
        let mut add = |g: &mut Geom, at: DVec3| {
            id += 1;
            qs.push(g.query(op.other(), id, at));
        };
        for m in [0.0, 2.0, -3.0] {
            add(&mut g, mid + UP * m);
            add(&mut g, mid + R_HAT * 0.5 + UP * m);
            add(&mut g, mid - R_HAT * 0.75 + UP * m);
            add(&mut g, x[0] + R_HAT * 0.5 + UP * m);
            add(&mut g, x[2] - R_HAT * 0.5 + UP * m);
            add(&mut g, x[0] + UP * m);
        }
        let (mut r, mut s) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r, &mut s, &g, &qs);
        for &q in &qs {
            // A tangency away from the point is one cluster, its two
            // crossings cancelling.
            check(&r, &g, q, !tangent, &format!("seed {seed}"));
        }
        r.replay(&s, &g, band()).expect("replays");
    }
}

/// Each record is decided once: a second fill of the same keys decides
/// nothing; the knot records are read by the spans on both sides.
#[test]
fn records_are_decided_once() {
    let mut g = Geom::with(DIRECTIONS[0], Sign::Pos);
    let spans = circle(lat(0.0, 0.0, 0.0), 1.0, 4, 0.25);
    let e = g.edge(B, 0, 0, 0, &spans);
    let q = g.query(A, 9, lat(0.25, 0.0, 0.5));
    let keys: Vec<SpanKey> = (0..4).map(|span| SpanKey { q, g: e, span }).collect();
    let (mut r, mut s) = (CurvedRays::new(), Shadows::new());
    let mut seq = Seq::default();
    r.fill(keys.clone(), &mut s, &g, band(), &mut seq)
        .expect("decided");
    // One vertex record, three knot records, four spans.
    assert_eq!(seq.decided, 1 + 3 + 4);
    let total: i32 = (0..4)
        .map(|i| r.span(q, e, i).expect("stored").count())
        .sum();
    assert_eq!(total.abs(), 1);
    let mut again = Seq::default();
    r.fill(keys, &mut s, &g, band(), &mut again)
        .expect("decided");
    assert_eq!(again.decided, 0);
    assert_eq!(again.cost, Cost::default());
    #[cfg(debug_assertions)]
    for k in 1..4 {
        let key = KnotKey { q, g: e, knot: k };
        assert_eq!(r.knot_table().reads(&key), Some(2), "{key:?}");
    }
    assert!(r.knot(q, e, 1).is_some());
    r.replay(&s, &g, band()).expect("replays");
}

/// A point on the span whose tie can't reach a target of zero escalates
/// through every level and fails its pass; a key past the edge's spans is
/// a bug.
#[test]
fn a_tie_past_every_level_escalates() {
    let mut g = Geom::with(DIRECTIONS[0], Sign::Pos);
    let s: Span = [
        cp(lat(-2.0, 1.0, 0.0), 1.0),
        cp(lat(0.0, 3.5, 1.0), 1.0),
        cp(lat(2.0, -2.0, 0.0), 1.0),
    ];
    let e = g.edge(B, 0, 0, 1, &[s]);
    let q = g.query(A, 9, middle(&s) + UP);
    let zero = Band::curved(0.0, 0.0).expect("a band");
    let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
    let mut seq = Seq::default();
    let got = r.fill([SpanKey { q, g: e, span: 0 }], &mut sh, &g, zero, &mut seq);
    assert!(
        matches!(got, Err(ShadowError::Pass(Fail::Escalate { .. }))),
        "{got:?}"
    );
    assert!(
        seq.cost.levels.iter().all(|l| l.evals > 0),
        "{:?}",
        seq.cost
    );
    assert!(r.span(q, e, 0).is_none());
    // At a reachable target the same record is the moved configuration's.
    r.fill(
        [SpanKey { q, g: e, span: 0 }],
        &mut sh,
        &g,
        band(),
        &mut Seq::default(),
    )
    .expect("decided");
    check(&r, &g, q, true, "tie");
    let past = r.fill(
        [SpanKey { q, g: e, span: 1 }],
        &mut sh,
        &g,
        band(),
        &mut Seq::default(),
    );
    assert!(
        matches!(past, Err(ShadowError::Pass(Fail::Internal { .. }))),
        "{past:?}"
    );
}

/// Rays from points on the columns of span ends and an inner knot, the
/// end and knot weights uneven: 3, ⅓, [`SPLIT`] or random from 1/64 to 64,
/// the knot's weight differing between its two spans. Points at the end
/// or knot itself, above and below it, and on its `r̂` line before and
/// past it (the end on the ray line, `g` 0 there, next to a root of the
/// span about half the time), as close to `t = 1` as to `t = 0`. Half
/// the seeds place the points on a dyadic lattice, so a point is exactly
/// at an end or on its `r̂` line (ties every level reaches, decided by the
/// motions); the others anywhere, so a point placed there is within a
/// rounding of it, where the rows at the end are ties at the first level.
/// Every crossing is the moved configuration's.
#[test]
fn rays_through_span_ends_and_knots_cross_as_the_moved_configuration() {
    for seed in varde_testing::seeds(64, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut g = Geom::with(DIRECTIONS[seed as usize % 5], sigmas(seed / 5));
        let op = if seed % 2 == 0 { A } else { B };
        let exact = (seed / 2) % 2 == 0;
        let p = |r: &mut varde_testkit::rng::Rng| {
            if exact {
                let mut c = || (r.next_u64() % 17) as f64 / 4.0 - 2.0;
                lat(c(), c() / 8.0, c() / 2.0)
            } else {
                lat(
                    r.range(-2.0, 2.0),
                    r.range(-2.0, 2.0) / 8.0,
                    r.range(-1.0, 1.0),
                )
            }
        };
        let mut x: [DVec3; 5] = std::array::from_fn(|_| p(&mut rng));
        if exact {
            // The middle control points off the ends' `r̂` lines (`ν` an odd
            // multiple of 1/64, the ends' a multiple of 1/32), so no span
            // lies along a ray line (where a fold of its shadow, `T ∥ UP`,
            // is not decided).
            for k in [1, 3] {
                x[k] += NU / 64.0;
            }
        }
        let weight = |r: &mut varde_testkit::rng::Rng| match r.next_u64() % 5 {
            k @ 0..=2 => UNEVEN[k as usize],
            _ => r.log_range(1.0 / 64.0, 64.0),
        };
        // The knot `x[2]` weighs `w[2]` in the first span, `w[5]` in the
        // second.
        let w: [f64; 6] = std::array::from_fn(|_| weight(&mut rng));
        let spans: [Span; 2] = [
            [cp(x[0], w[0]), cp(x[1], w[1]), cp(x[2], w[2])],
            [cp(x[2], w[5]), cp(x[3], w[3]), cp(x[4], w[4])],
        ];
        let e = g.edge(op, 0, 1, 2, &spans);
        let mut qs = Vec::new();
        let mut id = 100;
        for at in [x[0], x[2], x[4]] {
            for (l, m) in [
                (0.0, 0.0),
                (0.0, 2.0),
                (0.0, -1.5),
                (0.25, 1.0),
                (-0.25, 0.5),
            ] {
                id += 1;
                qs.push(g.query(op.other(), id, at + R_HAT * l + UP * m));
            }
        }
        let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r, &mut sh, &g, &qs);
        for &q in &qs {
            check(&r, &g, q, true, &format!("seed {seed}, {e:?}"));
        }
        r.replay(&sh, &g, band()).expect("replays");
    }
}

/// Spans lying on the ray line of the points (every control point on one
/// `r̂` line, so `g ≡ 0`): the points before, between, at and past their
/// ends along it, above and below; weights 1/64 to 64. In the moved
/// configuration the line misses the span; every record is the moved
/// configuration's.
#[test]
fn spans_on_the_ray_line_follow_the_moved_configuration() {
    // Every level runs for most of these records: few seeds when quick.
    for seed in varde_testing::seeds(8, 400) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let op = if (seed / 6) % 2 == 0 { A } else { B };
        let mut g = Geom::with(DIRECTIONS[seed as usize % 5], sigmas(seed / 5));
        let w = [
            [1.0, 1.0, 1.0],
            [2.0, 1.0, 4.0],
            [1.0, 3.0, 1.0],
            [1.0 / 64.0, 1.0, 64.0],
            [64.0, 0.5, 1.0 / 64.0],
            [1.0, 64.0, 1.0],
        ][seed as usize % 6];
        let y = int(3);
        // The middle control point between the ends along `r̂`, so the
        // span's shadow doesn't fold back (where its tangent is along
        // `UP`).
        let x0 = int(4);
        let xs = [x0, x0 + 4.0 + int(3), x0 + 9.0];
        let x = [0, 1, 2].map(|i| lat(xs[i], y, int(2)));
        let s: Span = [0, 1, 2].map(|i| cp(x[i], w[i]));
        g.edge(op, 0, 1, 2, &[s]);
        let mut qs = Vec::new();
        let mut id = 100;
        for xq in [
            xs[0] - 1.0,
            xs[0],
            xs[0] + 0.5,
            xs[2],
            xs[2] + 1.0,
            xs[2] - 2.0,
        ] {
            for m in [0.0, 1.5, -2.0] {
                id += 1;
                qs.push(g.query(op.other(), id, lat(xq, y, m)));
            }
        }
        let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r, &mut sh, &g, &qs);
        for &q in &qs {
            check(&r, &g, q, true, &format!("seed {seed}"));
        }
        r.replay(&sh, &g, band()).expect("replays");
    }
}

/// Spans with one end or both exactly on the points' ray line (`ν·x = 0`),
/// the middle control point off it, end weights uneven (3, ⅓, [`SPLIT`],
/// 64/3, 0.7), so `g` is 0 at the end but its coefficients round at the
/// first level. Points on that line before, at and past the ends, above
/// and below. Every record is the moved configuration's.
#[test]
fn span_ends_on_the_ray_line_cross_as_the_moved_configuration() {
    for seed in varde_testing::seeds(24, 512) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let op = if seed % 2 == 0 { A } else { B };
        let mut g = Geom::with(DIRECTIONS[seed as usize % 5], sigmas(seed / 5));
        let ws = [3.0, 1.0 / 3.0, SPLIT, 64.0 / 3.0, 0.7];
        let wk: [f64; 3] = std::array::from_fn(|_| ws[(int(2) + 2.0) as usize]);
        let (a0, a2) = (int(3) - 5.0, int(3) + 5.0);
        let ends = match seed % 3 {
            0 => [true, false],
            1 => [false, true],
            _ => [true, true],
        };
        let y = |on: bool| if on { 0.0 } else { -1.0 };
        let mid = lat(int(4), 1.0 + int(2).abs(), int(2));
        let s: Span = [
            cp(lat(a0, y(ends[0]), int(2)), wk[0]),
            cp(mid, wk[1]),
            cp(lat(a2, y(ends[1]), int(2)), wk[2]),
        ];
        let e = g.edge(op, 0, 1, 2, &[s]);
        let mut qs = Vec::new();
        let mut id = 100;
        for x in [a0 - 1.0, a0, a0 + 0.5, a2 - 0.25, a2, a2 + 2.0] {
            for m in [0.0, 1.5, -2.0] {
                id += 1;
                qs.push(g.query(op.other(), id, lat(x, 0.0, m)));
            }
        }
        let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r, &mut sh, &g, &qs);
        for &q in &qs {
            check(&r, &g, q, true, &format!("seed {seed}, {e:?}"));
        }
        r.replay(&sh, &g, band()).expect("replays");
    }
}

/// A point of `B` beside the column of an inner knot of an edge of `A`,
/// by about a rounding in both shadow directions (`ν·(k − q) ≈ −3.4e−13`,
/// `r̂·(k − q) ≈ −7.1e−15`, the knot behind it), at the default band: the
/// knot record reads its side and its place exactly, and the spans on
/// both sides cross the ray line there, behind `q`. Each span's crossing
/// at the knot is decided as that record: neither on the ray.
#[test]
fn a_point_beside_a_knot_column_reads_its_spans_as_the_knot() {
    let mut g = Geom::with(DVec3::ZERO, Sign::Neg);
    let knot = [
        -20.754_781_819_796_154,
        -78.709_251_237_318_01,
        -16.090_137_967_494_65,
    ];
    let spans: [Span; 2] = [
        [
            [
                13.654_189_940_062_357,
                -16.459_316_372_817_6,
                -28.064_851_716_026_972,
                3.0,
            ],
            [
                -24.026_874_838_735_17,
                14.982_872_656_597_38,
                21.076_065_325_591_62,
                1.106_347_468_807_837_4,
            ],
            [knot[0], knot[1], knot[2], 0.105_269_215_776_864_96],
        ],
        [
            [knot[0], knot[1], knot[2], 0.017_205_687_999_857_285],
            [
                26.715_366_514_029_142,
                -96.534_185_123_725_07,
                7.256_026_667_433_561,
                1.0 / 3.0,
            ],
            [
                -20.219_504_659_139_49,
                99.036_393_598_453_98,
                -13.895_136_626_820_364,
                1.0 / 3.0,
            ],
        ],
    ];
    let e = g.edge(A, 0, 1, 2, &spans);
    let q = g.query(
        B,
        9,
        DVec3::new(
            -23.754_781_819_796_154,
            -83.209_251_237_318_01,
            -64.090_137_967_494_66,
        ),
    );
    let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
    fill_all(&mut r, &mut sh, &g, &[q]);
    assert_eq!(
        r.knot(q, e, 1),
        Some(KnotRec {
            side: Sign::Neg,
            ahead: Sign::Neg
        })
    );
    check(&r, &g, q, true, "beside the knot");
    assert_eq!(signs(&r, q, e, 0), Vec::<i8>::new());
    assert_eq!(signs(&r, q, e, 1), vec![1]);
    r.replay(&sh, &g, band()).expect("replays");
}

/// Points beside the columns of vertices and knots of a closed chain (a
/// circle in two edges of two spans, end and knot weights uneven), off
/// them along `r̂` and `ν` by nothing, by a few roundings or by up to
/// `4·2⁻ᵏ`, above and below, at the default band (whose target the rows there reach at the
/// first level, so the rows at the joint are ties): every crossing is the
/// moved configuration's, the records sum to the winding number, which a
/// ray along `ν` counts too; the edges stored the other way read the
/// records negated, span for span from the other end.
#[test]
fn points_beside_joint_columns_cross_as_the_moved_configuration() {
    for seed in varde_testing::seeds(24, 1024) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let sigma = sigmas(seed);
        let d = DIRECTIONS[(seed / 2) as usize % 5];
        let op = if seed % 3 == 0 { A } else { B };
        let c = lat(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0));
        let rad = 1.0 + rng.range(0.0, 2.0);
        let spans: Vec<Span> = circle(c, rad, 4, rng.range(0.0, 1.0))
            .iter()
            .map(|s| {
                let mut pick = || UNEVEN[(rng.next_u64() % 4) as usize];
                reweighted(s, pick(), pick())
            })
            .collect();
        let mut qs_at = Vec::new();
        for s in &spans {
            let j = point_of(&s[0]);
            for _ in 0..6 {
                // Off by nothing, by up to a few roundings of the joint, or
                // by up to `4·2⁻ᵏ`, `k < 40`.
                let mut off = || match rng.next_u64() % 4 {
                    0 => 0.0,
                    1 => rng.range(-4.0, 4.0) * f64::EPSILON,
                    _ => rng.range(-4.0, 4.0) * libm::ldexp(1.0, -((rng.next_u64() % 40) as i32)),
                };
                let (a, b) = (off(), off());
                let m = [0.0, 1.5, -2.0][(rng.next_u64() % 3) as usize];
                qs_at.push(j + R_HAT * a + NU * (b / 32.0) + UP * m);
            }
        }
        let build = |flip: bool| {
            let mut g = Geom::with(d, sigma);
            let ids = |v: u32| if flip { 10 - v } else { v };
            let e0 = g.edge(op, 0, ids(0), ids(1), &spans[0..2]);
            let e1 = g.edge(op, 1, ids(1), ids(0), &spans[2..4]);
            let qs: Vec<Point> = qs_at
                .iter()
                .enumerate()
                .map(|(i, &at)| g.query(op.other(), 100 + i as u32, at))
                .collect();
            let sense = |a: u32, b: u32| ids(a) < ids(b);
            (g, qs, [(e0, sense(0, 1)), (e1, sense(1, 0))])
        };
        let (g, qs, cycle) = build(false);
        let (g2, qs2, cycle2) = build(true);
        let (mut r, mut s) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r, &mut s, &g, &qs);
        let (mut r2, mut s2) = (CurvedRays::new(), Shadows::new());
        fill_all(&mut r2, &mut s2, &g2, &qs2);
        let nu = q3(NU);
        for (&q, &q2) in qs.iter().zip(&qs2) {
            check(&r, &g, q, true, &format!("seed {seed}"));
            check(
                &r2,
                &g2,
                q2,
                true,
                &format!("seed {seed}, stored the other way"),
            );
            let qm = g.moved(q.operand, g.pos[&q]);
            let (mut sum, mut across) = (0, 0);
            for &(e, forward) in &cycle {
                for i in 0..2 {
                    let c = r.span(q, e, i as u32).expect("stored").count();
                    let o: i32 = oracle(&g, op, e, i, &qm, &nu)
                        .iter()
                        .map(|&x| i32::from(x))
                        .sum();
                    sum += if forward { c } else { -c };
                    across += if forward { o } else { -o };
                }
            }
            assert_eq!(sum, across, "seed {seed}, {q:?}");
            for (&(e, _), &(e2, _)) in cycle.iter().zip(&cycle2) {
                for i in 0..2 {
                    let c = r.span(q, e, i as u32).expect("stored").count();
                    let c2 = r2.span(q2, e2, (1 - i) as u32).expect("stored").count();
                    assert_eq!(c, -c2, "seed {seed}, {q:?}, {e:?} span {i}");
                }
            }
        }
        r.replay(&s, &g, band()).expect("replays");
        r2.replay(&s2, &g2, band()).expect("replays");
    }
}

/// Bounds mirrored onto the span (`1 − u`) enclose it and stay on
/// `[0, 1]`, also for a `u` whose `1 − u` rounds to 1 or is subnormal.
#[test]
fn mirrored_bounds_stay_on_the_span() {
    for u in [
        [0.0, 0.0],
        [1e-300, 3e-17],
        [2.7e-17, 1e-16],
        [0.25, 0.5],
        [0.999_999_999_999_999_9, 1.0],
        [1.0, 1.0],
    ] {
        let [lo, hi] = mirrored(u);
        assert!(0.0 <= lo && lo <= hi && hi <= 1.0, "{u:?} → {:?}", [lo, hi]);
        // `[lo, hi] ∋ 1 − u` exactly.
        for x in u {
            let t = Q::from(1u32) - q(x);
            assert!(q(lo) <= t && t <= q(hi), "{u:?} → {:?}", [lo, hi]);
        }
    }
}

/// A span whose end isn't exactly its vertex's point or the next span's
/// start (here off by a rounding) breaks the geometry's contract: its
/// record would read an end record of another point. Its pass fails
/// (`Internal`) rather than count the gap.
#[test]
fn a_span_end_off_its_vertex_or_knot_is_refused() {
    let spans = |knot: DVec3| -> [Span; 2] {
        [
            [
                cp(lat(-2.0, 0.0, 0.0), 1.0),
                cp(lat(-1.0, 1.0, 0.0), 0.5),
                cp(lat(0.0, 0.0, 0.0), 3.0),
            ],
            [
                cp(knot, SPLIT),
                cp(lat(1.0, -1.0, 0.0), 2.0),
                cp(lat(2.0, 0.0, 0.0), 1.0),
            ],
        ]
    };
    let k = lat(0.0, 0.0, 0.0);
    let nudged = DVec3::new(k.x.next_up(), k.y, k.z);
    for (knot, vertex_off) in [(nudged, false), (k, true)] {
        let mut g = Geom::with(DIRECTIONS[0], Sign::Pos);
        let e = g.edge(B, 0, 1, 2, &spans(knot));
        if vertex_off {
            let v = Point::vert(B, VertId(2));
            let at = g.pos[&v];
            g.pos.insert(v, DVec3::new(at.x, at.y.next_up(), at.z));
        }
        let q = g.query(A, 9, lat(-0.5, 0.0, 1.0));
        let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
        let keys = (0..2).map(|span| SpanKey { q, g: e, span });
        let got = r.fill(keys, &mut sh, &g, band(), &mut Seq::default());
        assert!(
            matches!(got, Err(ShadowError::Pass(Fail::Internal { .. }))),
            "{got:?}"
        );
    }
}

mod chains;

/// The geometric order of [`Geom`]'s edges: positions only, no face keys.
impl crate::records::height::EntityOrder for Geom {
    type Key = ();

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)].0
    }

    fn vert_key(&self, operand: Operand, v: VertId) -> crate::order::VertKey<()> {
        let p = self.pos[&Point::vert(operand, v)];
        crate::order::VertKey::new(p.to_array(), []).expect("no NaN")
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> crate::order::EdgeKey<()> {
        let (ends, spans) = &self.edges[&(operand, g)];
        let mid = middle(&spans[spans.len() / 2]).to_array();
        let ends = ends.map(|v| self.vert_key(operand, v));
        crate::order::EdgeKey::new(ends, mid, [(), ()]).expect("no NaN")
    }
}

/// The height keys read through the decided tables: a point near one end
/// of an arc reads that end's vertex key for its foot on the arc, and for
/// a layer whose feature is the foot; the tables' reads are the stored
/// records.
#[test]
fn height_keys_read_the_decided_tables() {
    use crate::records::feet::NearFeet;
    use crate::records::height::{
        Feature, HeightReads, LayerPieces, LayerTable, Records, height_key,
    };
    use crate::records::keys::{FootId, HKey, LayerId, Target};

    let mut g = Geom::with(DIRECTIONS[0], Sign::Pos);
    g.r_g = 0.125;
    let spans = circle(lat(0.0, 0.0, 0.0), 1.0, 4, 0.25);
    let e = g.edge(B, 0, 0, 1, &spans[..2]);
    let start = point_of(&spans[0][0]);
    let q = g.query(A, 9, start + DVec3::new(0.03125, 0.0, 0.0625));
    let (mut r, mut s) = (CurvedRays::new(), Shadows::new());
    let keys: Vec<SpanKey> = (0..2).map(|span| SpanKey { q, g: e, span }).collect();
    r.fill(keys, &mut s, &g, band(), &mut Seq::default())
        .expect("decided");
    let mut feet = NearFeet::new();
    feet.fill([RayKey { q, g: e }], &g, &mut Seq::default())
        .expect("decided");
    let reads = Records {
        shadows: &s,
        feet: &feet,
        order: &g,
    };
    let [v0, v1] = [VertId(0), VertId(1)];
    assert_eq!(reads.vv(q, v0), s.vv(q, v0));
    assert!(reads.vv(q, v0).expect("decided").near);
    assert!(!reads.vv(q, v1).expect("decided").near);
    assert_eq!(reads.feet(q, e), feet.get(q, e));
    let k = FootId(0);
    let want = HKey::vv(q, v0);
    let mut layers = LayerTable::new();
    assert_eq!(height_key(&reads, &layers, q, Target::Edge(e, k)), Ok(want));
    let mut pieces = LayerPieces::new();
    pieces.piece([Feature::Foot(e, k)], None);
    let f = crate::ids::FaceId(3);
    layers.insert(q, f, pieces.layers(&reads, q, f).expect("layers"));
    assert_eq!(
        height_key(&reads, &layers, q, Target::Layer(f, LayerId(0))),
        Ok(want)
    );
}
