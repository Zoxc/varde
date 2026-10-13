//! The curved ray records against the moved configuration they stand for:
//! every span record's crossings are the exact ones of the operands really
//! moved (in rationals, `A` translated by tiny powers of two times the
//! motions' rates), counted on the span and on the stretches between its
//! ends and their rounded points; random conic spans with weights from
//! 1/64 to 64, rays through span ends and knots, through rounded span
//! ends and knots next to a root of the span, through the span itself,
//! tangent to it away from the point and at it, spans lying along the
//! ray line; a closed chain's records
//! sum to its winding number, which a ray in another direction counts
//! too; an edge stored the other way reads its records negated; each
//! record decided once, replayed alike; a tie no level reaches the target
//! of escalates past the last level.

use std::collections::BTreeMap;

use glam::DVec3;
use malachite::Rational as Q;
use malachite::base::num::arithmetic::traits::PowerOf2;

use super::*;
use crate::ids::VertId;
use crate::perturb::{Seg, T, UP, rates_a};

use Operand::{A, B};

/// Curved edges of either operand, and points of both.
#[derive(Default)]
struct Geom {
    pos: BTreeMap<Point, DVec3>,
    edges: BTreeMap<(Operand, EdgeId), ([VertId; 2], Vec<HomSpan>)>,
    /// `A`'s direction field: one vector, so `A` moves as a translation.
    d: DVec3,
    sigma: Option<Sign>,
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
    /// stored from the lower id; its vertices at the chain's ends
    /// projected.
    fn edge(&mut self, op: Operand, id: u32, v: u32, w: u32, spans: &[HomSpan]) -> EdgeId {
        let (ends, spans) = if v <= w {
            ([v, w], spans.to_vec())
        } else {
            ([w, v], reversed(spans))
        };
        let last = spans.len() - 1;
        for (vid, at) in [
            (ends[0], project(&spans[0][0])),
            (ends[1], project(&spans[last][2])),
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

    /// A span moved: its homogeneous points `P + W·Δ`.
    fn moved_span(&self, op: Operand, s: &HomSpan) -> [[Q; 4]; 3] {
        let o = self.offset(op);
        s.map(|p| {
            let w = q(p[3]);
            [
                q(p[0]) + &w * &o[0],
                q(p[1]) + &w * &o[1],
                q(p[2]) + &w * &o[2],
                w,
            ]
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
        0.0
    }

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)].0
    }

    fn line(&self, _: Point, _: EdgeId) -> Seg {
        unreachable!("only curved edges here")
    }
}

impl CurveGeometry for Geom {
    fn spans(&self, operand: Operand, g: EdgeId) -> &[HomSpan] {
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

fn reversed(spans: &[HomSpan]) -> Vec<HomSpan> {
    spans.iter().rev().map(|s| [s[2], s[1], s[0]]).collect()
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn q3(p: DVec3) -> [Q; 3] {
    [p.x, p.y, p.z].map(q)
}

fn qsub(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [0, 1, 2].map(|i| &a[i] - &b[i])
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
/// with span `i` of edge `e` (of `op`): the stretch from its start's
/// rounded point to its start, the span, the stretch from its end to its
/// end's rounded point; each `sign(UP·(ρ × T))`. Exact, solved directly
/// (no records, no perturbation); panics at a tie the moved configuration
/// still has.
fn oracle(g: &Geom, op: Operand, e: EdgeId, i: usize, qm: &[Q; 3], rho: &[Q; 3]) -> Vec<i8> {
    let s = &g.edges[&(op, e)].1[i];
    let n = qcross(&q3(UP), rho);
    let k0 = g.moved(op, project(&s[0]));
    let k1 = g.moved(op, project(&s[2]));
    let m = g.moved_span(op, s);
    let pt = |p: &[Q; 4]| [0, 1, 2].map(|k| &p[k] / &p[3]);
    let mut out = Vec::new();
    let segment = |a: &[Q; 3], b: &[Q; 3], out: &mut Vec<i8>| {
        let (sa, sb) = (qdot(&n, &qsub(a, qm)), qdot(&n, &qsub(b, qm)));
        assert!(sgn(&sa) != 0 && sgn(&sb) != 0, "a tie at a span end");
        if sgn(&sa) == sgn(&sb) {
            return;
        }
        let u = &sa / (&sa - &sb);
        let x = [0, 1, 2].map(|k| &a[k] + &u * (&b[k] - &a[k]));
        let along = sgn(&qdot(rho, &qsub(&x, qm)));
        assert!(along != 0, "a tie at a span end");
        if along > 0 {
            out.push(sgn(&qdot(&n, &qsub(b, a))));
        }
    };
    segment(&k0, &pt(&m[0]), &mut out);
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
    segment(&pt(&m[2]), &k1, &mut out);
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
    fill_at(r, s, g, qs, band());
}

/// [`fill_all`] at the curved band `band`.
fn fill_at(r: &mut CurvedRays, s: &mut Shadows, g: &Geom, qs: &[Point], band: Band) {
    let mut keys = Vec::new();
    for &q in qs {
        for ((op, e), (_, spans)) in &g.edges {
            if *op == q.operand.other() {
                keys.extend((0..spans.len() as u32).map(|span| SpanKey { q, g: *e, span }));
            }
        }
    }
    r.fill(keys, s, g, band, &mut Seq::default())
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

/// The homogeneous point `(w·x, w)`, rounded as stored.
fn hom(x: DVec3, w: f64) -> [f64; 4] {
    [w * x.x, w * x.y, w * x.z, w]
}

const DIRECTIONS: [DVec3; 5] = [DVec3::new(0.25, -0.5, 0.125), DVec3::ZERO, R_HAT, NU, UP];

fn sigmas(seed: u64) -> Sign {
    if seed.is_multiple_of(2) {
        Sign::Pos
    } else {
        Sign::Neg
    }
}

/// Random conic spans (weights from 1/64 to 64, homogeneous points rounded
/// as stored, so a span's end and its rounded point may differ) against
/// random points of both operands: every crossing as the moved
/// configuration's.
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
                let s: HomSpan = [0, 1, 2].map(|i| hom(pts[i], weight(&mut rng)));
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
fn circle(c: DVec3, rad: f64, n: usize, start: f64) -> Vec<HomSpan> {
    let step = std::f64::consts::TAU / n as f64;
    let at = |a: f64, r: f64| c + lat(r * libm::cos(a), r * libm::sin(a) / 8.0, 0.0);
    let w = libm::cos(step / 2.0);
    let ends: Vec<[f64; 4]> = (0..=n)
        .map(|k| {
            // Dyadic ends so ties can be placed on them exactly.
            let p = at(start + step * k as f64, rad);
            let r = |x: f64| (x * 64.0).round() / 64.0;
            hom(DVec3::new(r(p.x), r(p.y), r(p.z)), 1.0)
        })
        .collect();
    let mut ends = ends;
    ends[n] = ends[0];
    (0..n)
        .map(|k| {
            let mid = at(start + step * (k as f64 + 0.5), rad / w);
            [ends[k], hom(mid, w), ends[k + 1]]
        })
        .collect()
}

/// A closed chain of curved edges: circles split into two edges of two
/// spans, joined at vertices and knots, against points inside, outside,
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
        let spans = circle(c, rad, 4, rng.range(0.0, 1.0));
        let joints: Vec<DVec3> = spans.iter().map(|s| project(&s[0])).collect();
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

/// Spans whose point at `t = ½` is exact: weights summing, as
/// `w₀ + 2w₁ + w₂`, to a power of two.
const EXACT_MIDDLE: [[f64; 3]; 4] = [
    [1.0, 1.0, 1.0],
    [1.0, 0.5, 2.0],
    [2.0, 1.0, 4.0],
    [1.0, 3.0, 1.0],
];

/// The span's point at `t = ½`, exactly for [`EXACT_MIDDLE`] weights.
fn middle(s: &HomSpan) -> DVec3 {
    let w = s[0][3] + 2.0 * s[1][3] + s[2][3];
    let c = |k: usize| (s[0][k] + 2.0 * s[1][k] + s[2][k]) / w;
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
        let s: HomSpan = [0, 1, 2].map(|i| hom(x[i], w[i]));
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
    let s: HomSpan = [
        hom(lat(-2.0, 1.0, 0.0), 1.0),
        hom(lat(0.0, 3.5, 1.0), 1.0),
        hom(lat(2.0, -2.0, 0.0), 1.0),
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

/// Rays from points on the columns of span ends whose projection rounds
/// (weights not powers of two): an edge's start and end, and an inner knot
/// joining two spans, its weight not 1. The stretch between a span's end
/// and its rounded point lies across the ray line about half the time,
/// next to a root of the span where the point is at the rounded point
/// itself, as close to `t = 1` as to `t = 0`. Every crossing is the moved
/// configuration's.
#[test]
fn rays_through_rounded_span_ends_cross_as_the_moved_configuration() {
    for seed in varde_testing::seeds(64, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut g = Geom::with(DIRECTIONS[seed as usize % 5], sigmas(seed / 5));
        let op = if seed % 2 == 0 { A } else { B };
        let p = |r: &mut varde_testkit::rng::Rng| {
            lat(
                r.range(-2.0, 2.0),
                r.range(-2.0, 2.0) / 8.0,
                r.range(-1.0, 1.0),
            )
        };
        let x: [DVec3; 5] = std::array::from_fn(|_| p(&mut rng));
        let w: [f64; 5] = std::array::from_fn(|_| rng.log_range(1.0 / 64.0, 64.0));
        let h: [[f64; 4]; 5] = std::array::from_fn(|i| hom(x[i], w[i]));
        let spans: [HomSpan; 2] = [[h[0], h[1], h[2]], [h[2], h[3], h[4]]];
        let e = g.edge(op, 0, 1, 2, &spans);
        let mut qs = Vec::new();
        let mut id = 100;
        for at in [&h[0], &h[2], &h[4]].map(project) {
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
/// configuration the line misses the span or the stretch is crossed; every
/// record is the moved configuration's.
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
        let s: HomSpan = [0, 1, 2].map(|i| hom(x[i], w[i]));
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

/// Spans whose ends round, each end exactly on the points' ray line,
/// `ν·P = 0` with `P/W` not a float: the end's side is a tie there that
/// no level certifies. Points on that line before, between and past the
/// ends, above and below; both ends, weights 1/64 to 64. Every record is
/// the moved configuration's.
#[test]
fn rays_through_the_true_ends_of_rounded_spans_cross_as_the_moved_configuration() {
    for seed in varde_testing::seeds(24, 512) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let op = if seed % 2 == 0 { A } else { B };
        let mut g = Geom::with(DIRECTIONS[seed as usize % 5], sigmas(seed / 5));
        // Weights whose quotients round.
        let ws = [0.7, 3.0, 5.1, 1.0 / 64.0 * 3.0, 64.0 / 3.0];
        let wk: [f64; 3] = std::array::from_fn(|_| ws[(int(2) + 2.0) as usize]);
        // `P = k·r̂` on the line `ν·x = 0`, `k` not a multiple of 3: `P/W`
        // rounds for each weight.
        let k = [3.0 * (int(3) - 4.0) + 1.0, 3.0 * (int(3) + 4.0) - 1.0];
        let (a0, a2) = (k[0] / wk[0], k[1] / wk[2]);
        let on = |k: f64| {
            let p = R_HAT * k;
            [p.x, p.y, p.z]
        };
        let (p0, p2) = (on(k[0]), on(k[1]));
        let (w0, w2) = (wk[0], wk[2]);
        let mid = lat(int(4), 1.0 + int(2).abs(), int(2));
        let ends = match seed % 3 {
            0 => [true, false],
            1 => [false, true],
            _ => [true, true],
        };
        let end = |rounded: bool, p: [f64; 3], w: f64, a: f64| {
            if rounded {
                [p[0], p[1], p[2], w]
            } else {
                hom(lat(a, -1.0, 0.0), 1.0)
            }
        };
        let s: HomSpan = [
            end(ends[0], p0, w0, a0),
            hom(mid, wk[1]),
            end(ends[1], p2, w2, a2),
        ];
        let e = g.edge(op, 0, 1, 2, &[s]);
        assert!(ends[0] <= rounds(&s[0]) && ends[1] <= rounds(&s[2]));
        let mut qs = Vec::new();
        let mut id = 100;
        for x in [a0 - 1.0, a0, a0 + 0.5, a2 - 0.25, a2, a2 + 2.0] {
            for m in [0.0, 1.5, -2.0] {
                id += 1;
                qs.push(g.query(op.other(), id, lat(x, 0.0, m)));
            }
        }
        // Points within a rounding of an end: a target below that.
        let band = Band::curved(0.0, libm::ldexp(1.0, -100)).expect("a band");
        let (mut r, mut sh) = (CurvedRays::new(), Shadows::new());
        fill_at(&mut r, &mut sh, &g, &qs, band);
        for &q in &qs {
            check(&r, &g, q, true, &format!("seed {seed}, {e:?}"));
        }
        r.replay(&sh, &g, band).expect("replays");
    }
}
