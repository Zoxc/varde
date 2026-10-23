//! Edge pairs on one curve, and straight edges whose shadows lie on one
//! line: a cluster at each sign change of `h = ê·(T × Δ)` along the
//! overlap, none at the ends' ties, its z-order read at the root; the
//! box search away from the overlap off the diagonal band; the clusters
//! summing to the four terms. Checked against really moved geometry: the
//! two arcs on one circle of the shared-curve model (its presets and
//! random arcs, the moved arcs sampled densely), straight edges under a
//! linear field (their moved crossing exactly), a straight line in a
//! vertex's blend zone and a chain whose shadow crosses itself (both
//! sampled); the clusters' order along the curve under either stored
//! direction.

use super::clusters::z_orders;
use super::*;
use crate::count::s02::Charge;
use crate::num::schedule::PredKind;
use crate::num::trig::{atan2, cos, sin};
use crate::order::{EdgeKey, EntityOrder, VertKey};
use crate::records::feet::{EdgeCurve, Feet, decide_feet};
use crate::records::height::HeightReads;
use crate::records::shadow::VvRec;

/// The ends' feet on the other edge a shared-curve pair reads.
struct SharedReads<'a> {
    geom: &'a Geom,
    feet: BTreeMap<(Point, EdgeId), Feet>,
}

impl EntityOrder for SharedReads<'_> {
    type Key = ();

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        EntityOrder::ends(self.geom, operand, g)
    }

    fn vert_key(&self, operand: Operand, v: VertId) -> VertKey<()> {
        self.geom.vert_key(operand, v)
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<()> {
        self.geom.edge_key(operand, g)
    }
}

impl HeightReads for SharedReads<'_> {
    fn vv(&self, _: Point, _: VertId) -> Option<VvRec> {
        None
    }

    fn feet(&self, q: Point, g: EdgeId) -> Option<&Feet> {
        self.feet.get(&(q, g))
    }
}

/// The feet of each end of `e` and `g` on the other edge, at a radius of
/// `10⁻³`.
fn shared_reads(geom: &Geom) -> SharedReads<'_> {
    let mut feet = BTreeMap::new();
    for (op, own, other) in [(A, E, G), (B, G, E)] {
        let (ends, spans) = &geom.edges[&(op.other(), EdgeId(other))];
        let curve = if spans.is_empty() {
            EdgeCurve::Line(ends.map(|v| geom.pos[&Point::vert(op.other(), v)]))
        } else {
            EdgeCurve::Spans {
                spans,
                closed: ends[0] == ends[1],
            }
        };
        for v in geom.edges[&(op, EdgeId(own))].0 {
            let q = Point::vert(op, v);
            let key = RayKey {
                q,
                g: EdgeId(other),
            };
            let (f, _) = decide_feet(key, geom.pos[&q], curve, 1e-3);
            feet.insert((q, EdgeId(other)), f.expect("feet"));
        }
    }
    SharedReads { geom, feet }
}

/// A pair's record under the shared-curve rule (`one_curve`: the edges
/// on one curve), the side records the box search away from the overlap
/// asks for filled, with its `X`; `None` where the four terms aren't
/// decided.
fn shared_rec(geom: &Geom, one_curve: bool) -> Option<(Result<XRec, SharedError>, i64)> {
    shared_rec_cost(geom, one_curve).map(|(r, x, _)| (r, x))
}

/// [`shared_rec`] with the cost of its last search.
fn shared_rec_cost(geom: &Geom, one_curve: bool) -> Option<(Result<XRec, SharedError>, i64, Cost)> {
    let recs = try_decide(geom)?;
    let mut pair = recs.pair(geom);
    pair.shared = one_curve;
    if !one_curve {
        assert_eq!(recs.clusters(geom, &pair), Err(XClusterError::Collinear));
    }
    let reads = shared_reads(geom);
    let x = x_count(&pair.terms);
    for _ in 0..32 {
        let (r, cost) = {
            let sides = recs.sides.borrow();
            shared_clusters(&reads, geom, &sides, X, &pair, &Charge::xrec(u64::MAX))
        };
        match r {
            Err(SharedError::Need(ks)) => {
                for k in ks {
                    if !recs.fill(geom, k) {
                        return Some((Err(SharedError::Need(vec![k])), x, cost));
                    }
                }
            }
            r => return Some((r, x, cost)),
        }
    }
    None
}

/// `s11` of a record: its clusters with `g` above, the z-orders clear or
/// read from their height records; `None` where one isn't decided.
fn s11(geom: &Geom, rec: &XRec) -> Option<i64> {
    let z = z_orders(geom, rec);
    let mut s = 0;
    for (cl, z) in rec.clusters.iter().zip(z) {
        if z? == Sign::Pos {
            s += cl.degree;
        }
    }
    Some(s)
}

fn qcross(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

/// `s11` of two straight edges in the moved configuration, exactly (the
/// field is linear, so the moved edges are straight): the moved crossing's
/// sign where the moved `g` is above the moved `e` there, else 0; `None`
/// at a tie the moved configuration keeps.
fn moved_s11(geom: &Geom) -> Option<i64> {
    let x = moved_crossing(geom)?;
    if x == 0 {
        return Some(0);
    }
    let [t, s] = moved_params(geom)?;
    let height = |op: Operand, id: u32, at: &Q| {
        let [p, q] = geom.edges[&(op, EdgeId(id))].0.map(|v| {
            let p = geom.pos[&Point::vert(op, v)];
            let o = geom.offset(op, p);
            let at = q3(p);
            let x: [Q; 3] = [0, 1, 2].map(|i| &at[i] + &o[i]);
            qdot(&q3(UP), &x)
        });
        &p + at * (q - &p)
    };
    let gap = sgn(&(height(B, G, &s) - height(A, E, &t)));
    match gap {
        0 => None,
        1 => Some(x),
        _ => Some(0),
    }
}

/// Straight edges on one line moved by a translation: the moved shadows
/// are parallel, so they never cross. No clusters and `X = 0`, also where
/// `h` is exactly 0 (the motion's shadow along the line: orders 2–4,
/// constant along a line, give one sign); refused only where the motion
/// runs along the line in 3D, so no order of the z-order row reads.
#[test]
fn edges_on_one_line_moved_apart_have_no_clusters() {
    let (mut formed, mut overlapping, mut flat, mut along) = (0, 0, 0, 0);
    for seed in varde_testing::seeds(200, 4000) {
        let mut rng = Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let dir = lat(int(2), int(2), int(3));
        let base = lat(int(2), int(2), int(2));
        let t = [int(3), int(3), int(3), int(3)];
        if t[0] == t[1] || t[2] == t[3] || dir.dot(R_HAT) == 0.0 && dir.dot(NU) == 0.0 {
            continue;
        }
        let at = |k: usize| base + dir * t[k];
        let d = [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3];
        let mut geom = Geom {
            d,
            ..Geom::default()
        };
        let (e0, e1) = if t[0] < t[1] { (0, 1) } else { (1, 0) };
        let (g0, g1) = if t[2] < t[3] { (2, 3) } else { (3, 2) };
        geom.segment(A, E, (0, at(e0)), (1, at(e1)));
        geom.segment(B, G, (2, at(g0)), (3, at(g1)));
        let (r, x) = shared_rec(&geom, true).expect("decided");
        if let Some(want) = moved_crossing(&geom) {
            assert_eq!(x, want, "seed {seed}");
        }
        let sorted = |a: f64, b: f64| if a < b { [a, b] } else { [b, a] };
        let ([e0, e1], [g0, g1]) = (sorted(t[0], t[1]), sorted(t[2], t[3]));
        let meet = e0 < g1 && g0 < e1;
        let h0 = qdot(&q3(UP), &qcross(&q3(dir), &q3(d))) == 0u32;
        let parallel = qcross(&q3(dir), &q3(d)).iter().all(|c| *c == 0u32);
        match r {
            Ok(rec) => {
                assert!(rec.clusters.is_empty(), "seed {seed}");
                assert_eq!(x, 0, "seed {seed}");
                formed += 1;
                overlapping += i32::from(meet);
                flat += i32::from(meet && h0);
            }
            Err(SharedError::Unsettled) => {
                assert!(meet && parallel, "seed {seed}");
                along += 1;
            }
            Err(e) => panic!("seed {seed}: {e:?}"),
        }
    }
    assert!(
        formed > 0 && overlapping > 0 && flat > 0,
        "{formed} formed, {overlapping} overlapping, {flat} with h = 0, {along} along"
    );
}

/// Straight edges whose shadows lie on one line, under a direction field
/// varying linearly along them (so they stay straight), `d = dir/2 +
/// ê/4 + (field·p)·ν`: `h` changes sign where `field·p` does, at `t = 0`, and
/// the moved edges cross there. The edges on one line (one curve), or in
/// one vertical plane, apart in height, crossing in height over the
/// overlap (at `t = 1`) or meeting at an end (`t = −1`, also an end of
/// `e` for one of them): one cluster where the root lies inside the
/// overlap, at it, of the moved crossing's degree, and `s11` the moved
/// configuration's.
#[test]
fn lines_on_one_shadow_line_cross_at_the_root_of_h() {
    let (mut crossed, mut checked) = (0, 0);
    for dir in [lat(1.0, 0.0, 0.0), lat(1.0, 2.0, 0.0), lat(1.0, 1.0, 1.0)] {
        for k in [1.0, -1.0] {
            for (t0, t1) in [(-1.0, 2.0), (0.5, 2.0), (-2.0, -0.25)] {
                // `g` lifted by `h0` at its start and `h1` at its end
                // along `UP`, `None` on `e`'s line.
                for lift in [
                    None,
                    Some((1.0, 1.0)),
                    Some((-2.0, -1.0)),
                    Some((-1.0, 1.0)),
                    Some((0.0, 2.0)),
                ] {
                    let at = |t: f64| dir * t;
                    let mut geom = Geom {
                        d: dir * 0.5 + UP * 0.25,
                        field: dir * k,
                        ..Geom::default()
                    };
                    geom.segment(A, E, (0, at(t0)), (1, at(t1)));
                    let (h0, h1) = lift.unwrap_or((0.0, 0.0));
                    geom.segment(B, G, (2, at(-1.0) + UP * h0), (3, at(3.0) + UP * h1));
                    let (r, x) = shared_rec(&geom, lift.is_none()).expect("decided");
                    let here = format!("{dir} {k} {t0} {lift:?}");
                    assert_eq!(Some(x), moved_crossing(&geom), "{here}");
                    let rec = r.unwrap_or_else(|e| panic!("{here}: {e:?}"));
                    let inside = t0 < 0.0 && 0.0 < t1;
                    assert_eq!(rec.clusters.len(), usize::from(inside), "{here}");
                    assert_eq!(rec.degree(), x, "{here}");
                    if let Some(c) = rec.clusters.first() {
                        // The root `t = 0` on `e`'s parameter.
                        let root = -t0 / (t1 - t0);
                        assert!(c.e.t[0] <= root && root <= c.e.t[1], "{here}: {c:?}");
                        assert!(matches!(c.row, ZRow::Root { .. }), "{here}");
                        assert_eq!(c.clear.is_some(), lift.is_some(), "{here}");
                        crossed += 1;
                    }
                    if let Some(want) = moved_s11(&geom) {
                        assert_eq!(s11(&geom, &rec), Some(want), "{here}");
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(
        crossed > 20 && checked > 40,
        "{crossed} crossed, {checked} checked"
    );
}

/// The shared-curve record charges every flat sign it reads, by kind and
/// stage (`Cost::exact`; filter, expansion and rational runs): on one
/// line (`g` inside `e`), the shadow-point and ends' place tests (`Side`),
/// the shadows on one line, the lines one in height and the moved signs
/// of `h` at the overlap's ends, and the overlap's ends (`g`'s) against
/// the split positions (`Orient3`, of degree 3); two distinct lines also
/// their height gap at each piece's ends (`Crossing`); on one curve the
/// moved signs at the run's ends and inner knot's two sides alone
/// (`Orient3`, one filter run per order up to the first that isn't 0).
/// Exact ties take the expansions.
#[test]
fn the_shared_record_charges_its_flat_signs() {
    let runs = |c: &Cost| {
        PredKind::ALL.map(|k| {
            let r = c.exact.of(k);
            [r.filters, r.expansions, r.rationals]
        })
    };
    let dir = lat(1.0, 2.0, 0.0);
    let none = [0; 3];
    for (lift, want) in [
        (None, [[5, 0, 0], none, [86, 4, 0], none, none]),
        (
            Some((-1.0, 1.0)),
            [[5, 0, 0], none, [37, 2, 0], [44, 5, 0], none],
        ),
    ] {
        let mut geom = Geom {
            d: dir * 0.5 + UP * 0.25,
            field: dir,
            ..Geom::default()
        };
        geom.segment(A, E, (0, dir * -1.0), (1, dir * 2.0));
        let (h0, h1) = lift.unwrap_or((0.0, 0.0));
        geom.segment(B, G, (2, dir * -0.5 + UP * h0), (3, dir * 1.5 + UP * h1));
        let (r, x, cost) = shared_rec_cost(&geom, lift.is_none()).expect("decided");
        let rec = r.expect("answered");
        assert_eq!((rec.clusters.len(), x), (1, rec.degree()), "{lift:?}");
        assert_eq!(runs(&cost), want, "{lift:?}");
    }
    let knots = model_knots(&[30.0, 150.0, 90.0, 230.0]);
    let ix = |a: f64| knots.iter().position(|&k| k == deg(a)).expect("a knot");
    let m = Model {
        ei: (ix(30.0), ix(150.0)),
        gi: (ix(90.0), ix(230.0)),
        knots: knots.clone(),
        phi: deg(210.0),
        rho: deg(15.0),
        vz: 0.2,
        tilt: 0.8,
        back_e: false,
        back_g: false,
        w: vec![1.0],
    };
    let (r, _, cost) = shared_rec_cost(&m.geom(), true).expect("decided");
    r.expect("answered");
    // One run over `e`'s spans from 90° to 150°: its two ends and the
    // inner knot's two sides, each decided at order 1.
    assert_eq!(runs(&cost), [none, none, [4, 1, 0], none, none]);
}

/// The shared-curve model's circle (its explainer's interactive view):
/// shadow `(cos s, sin s)` seen with the ray `r̂` at angle `rho`, height
/// `tilt·(cos 40°·cos s + sin 40°·sin s)`, mapped into the shadow frame
/// ([`lat`], a linear map keeping the shadow's orientation and the
/// heights' order). Spans between the angles `knots` (radians,
/// increasing, each step under 90°), each knot computed once so adjacent
/// spans share it bit for bit, span `i`'s weights scaled by `w[i]` (the
/// same curve).
fn model_arcs(knots: &[f64], rho: f64, tilt: f64, w: &[f64]) -> Vec<Span> {
    let psi = 40f64.to_radians();
    let (h1, h2) = (tilt * cos(psi), tilt * sin(psi));
    let (cr, sr) = (cos(rho), sin(rho));
    let map = |c: f64, s: f64| lat(c * cr + s * sr, s * cr - c * sr, h1 * c + h2 * s);
    let pts: Vec<DVec3> = knots.iter().map(|&t| map(cos(t), sin(t))).collect();
    (0..knots.len() - 1)
        .map(|i| {
            let h = (knots[i + 1] - knots[i]) / 2.0;
            let m = knots[i] + h;
            let mid = map(cos(m) / cos(h), sin(m) / cos(h));
            let c = w[i % w.len()];
            [cp(pts[i], c), cp(mid, cos(h) * c), cp(pts[i + 1], c)]
        })
        .collect()
}

/// The model's motion `(cos phi, sin phi, vz)` in the same frame.
fn model_motion(phi: f64, rho: f64, vz: f64) -> DVec3 {
    lat(cos(phi - rho), sin(phi - rho), vz)
}

/// One pair of the model: `e` over the knots `ei.0..ei.1` of the chain,
/// `g` over `gi`, `e` or `g` stored backwards if `back_e`, `back_g`.
#[derive(Clone)]
struct Model {
    knots: Vec<f64>,
    ei: (usize, usize),
    gi: (usize, usize),
    phi: f64,
    rho: f64,
    vz: f64,
    tilt: f64,
    back_e: bool,
    back_g: bool,
    w: Vec<f64>,
}

/// A crossing of the moved arcs, sampled: its angle on the circle, its
/// degree and whether the moved `g` is above the moved `e` there.
#[derive(Clone, Copy, Debug)]
struct Moved {
    at: f64,
    degree: i64,
    above: bool,
}

impl Model {
    fn geom(&self) -> Geom {
        let arcs = model_arcs(&self.knots, self.rho, self.tilt, &self.w);
        let e = arcs[self.ei.0..self.ei.1].to_vec();
        let g = arcs[self.gi.0..self.gi.1].to_vec();
        let e = if self.back_e { reversed(&e) } else { e };
        let g = if self.back_g { reversed(&g) } else { g };
        let mut geom = Geom {
            d: model_motion(self.phi, self.rho, self.vz),
            ..Geom::default()
        };
        geom.curve(A, E, 0, 1, e);
        geom.curve(B, G, 2, 3, g);
        geom
    }

    /// The edges' angle ranges.
    fn ranges(&self) -> [[f64; 2]; 2] {
        let r = |i: (usize, usize)| [self.knots[i.0], self.knots[i.1]];
        [r(self.ei), r(self.gi)]
    }

    /// The model's height at angle `s`.
    fn z(&self, s: f64) -> f64 {
        let psi = 40f64.to_radians();
        self.tilt * (cos(psi) * cos(s) + sin(psi) * sin(s))
    }

    /// The crossings of the moved arcs (`e` moved by `δ = 10⁻⁷` along the
    /// motion), sampled densely along `e` and narrowed by halving: where
    /// the moved `e` passes from inside the circle to outside or back at
    /// an angle of `g`. `None` where one lies within `10⁻⁵` of an edge's
    /// end or its z-order within `10⁻³·δ` of 0: a tie the sampling can't
    /// tell (an end tie, decided by the perturbation's orders).
    fn moved(&self) -> Option<Vec<Moved>> {
        let delta = 1e-7;
        let v = [cos(self.phi), sin(self.phi), self.vz];
        let [[e0, e1], [g0, g1]] = self.ranges();
        let moved = |s: f64| [cos(s) + delta * v[0], sin(s) + delta * v[1]];
        let f = |s: f64| {
            let p = moved(s);
            p[0] * p[0] + p[1] * p[1] - 1.0
        };
        let n = 4096;
        let mut out = Vec::new();
        let mut prev = (e0, f(e0));
        for i in 1..=n {
            let s = e0 + (e1 - e0) * f64::from(i) / f64::from(n);
            let fs = f(s);
            if (prev.1 < 0.0) != (fs < 0.0) {
                let (mut lo, mut hi) = (prev.0, s);
                for _ in 0..60 {
                    let m = 0.5 * (lo + hi);
                    if (f(m) < 0.0) == (prev.1 < 0.0) {
                        lo = m;
                    } else {
                        hi = m;
                    }
                }
                let at = 0.5 * (lo + hi);
                let p = moved(at);
                let mut psi = atan2(p[1], p[0]);
                while psi < g0 {
                    psi += std::f64::consts::TAU;
                }
                while psi > g0 + std::f64::consts::TAU {
                    psi -= std::f64::consts::TAU;
                }
                let near = |a: f64, b: f64| (a - b).abs() < 1e-5;
                if near(at, e0) || near(at, e1) || near(psi, g0) || near(psi, g1) {
                    return None;
                }
                if psi < g1 {
                    let gap = self.z(psi) - (self.z(at) + delta * v[2]);
                    if gap.abs() < 1e-3 * delta {
                        return None;
                    }
                    // Outward along `e`'s increasing angle: from the left
                    // of `g` running the same way to its right, `T_g × T_e
                    // < 0`.
                    let out_ward = fs > 0.0;
                    let mut degree = if out_ward { -1 } else { 1 };
                    if self.back_e != self.back_g {
                        degree = -degree;
                    }
                    out.push(Moved {
                        at,
                        degree,
                        above: gap > 0.0,
                    });
                }
            }
            prev = (s, fs);
        }
        Some(out)
    }

    /// The angle on the circle of the point at parameter `t` of `e`'s span
    /// `span` (along its stored direction): a circle's conic of weights
    /// `(1, cos h, 1)` over the half angle `h` runs with `tan((θ − m)/2) =
    /// (2t − 1)·tan(h/2)` about its middle angle `m`.
    fn e_angle(&self, span: u32, t: f64) -> f64 {
        let n = self.ei.1 - self.ei.0;
        let (k, t) = if self.back_e {
            (n - 1 - span as usize, 1.0 - t)
        } else {
            (span as usize, t)
        };
        let (a, b) = (self.knots[self.ei.0 + k], self.knots[self.ei.0 + k + 1]);
        let h = (b - a) / 2.0;
        let x = (2.0 * t - 1.0) * crate::num::trig::tan(h / 2.0);
        a + h + 2.0 * atan2(x, 1.0)
    }
}

/// Degrees to radians, for the presets.
fn deg(x: f64) -> f64 {
    x.to_radians()
}

/// The model's chain over the ends `ends` (degrees), cut to steps of at
/// most 50°.
fn model_knots(ends: &[f64]) -> Vec<f64> {
    let mut ends = ends.to_vec();
    ends.sort_by(f64::total_cmp);
    ends.dedup();
    let mut out = vec![deg(ends[0])];
    for w in ends.windows(2) {
        let n = ((w[1] - w[0]) / 50.0).ceil();
        let n = if n < 1.0 { 1.0 } else { n };
        for i in 1..=n as u32 {
            out.push(deg(w[0] + (w[1] - w[0]) * f64::from(i) / n));
        }
    }
    out
}

/// One answered model pair against its moved arcs: the clusters are the
/// sampled crossings (as many, of their degrees, in order along `e`, each
/// cluster's piece holding its crossing's angle), and `s11` theirs.
fn check_model(m: &Model, rec: &XRec, x: i64, moved: &[Moved], here: &str) {
    let geom = m.geom();
    assert_eq!(rec.degree(), x, "{here}");
    assert_eq!(moved.iter().map(|c| c.degree).sum::<i64>(), x, "{here}");
    assert_eq!(rec.clusters.len(), moved.len(), "{here}: {rec:?} {moved:?}");
    // Along `e` from its geometrically lesser end: the stored direction
    // or against it.
    let mut by_angle: Vec<&Moved> = moved.iter().collect();
    let lesser_first = {
        let [s, e] = geom.edges[&(A, EdgeId(E))].0;
        geom.vert_key(A, s) <= geom.vert_key(A, e)
    };
    // `e` runs along increasing angle unless stored backwards.
    if m.back_e == lesser_first {
        by_angle.reverse();
    }
    for (c, want) in rec.clusters.iter().zip(&by_angle) {
        assert_eq!(c.degree, want.degree, "{here}: {c:?} {want:?}");
        let [lo, hi] = [c.e.t[0], c.e.t[1]].map(|t| m.e_angle(c.e.span, t));
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        assert!(
            lo - 1e-6 <= want.at && want.at <= hi + 1e-6,
            "{here}: {c:?} at {} not in [{lo}, {hi}]",
            want.at
        );
    }
    let want: i64 = moved.iter().filter(|c| c.above).map(|c| c.degree).sum();
    assert_eq!(s11(&geom, rec), Some(want), "{here}");
}

/// The presets of the shared-curve explainer (`e` and `g` from and to in
/// degrees, motion angle, ray angle), height `0.8` tilted, motion up
/// `0.2`, under each stored direction of either edge: a cluster at each
/// crossing of the moved arcs, none at an end. "cancel" has `h = 0` at
/// `e`'s end (150°), its moved sign there from orders 2–4 (the former
/// rule formed a cancelling pair of end clusters whose z-orders differ);
/// "booked" crosses inside the overlap (formerly booked at an end and
/// reading the z-order there); abutting quarters, `a`'s ray exactly
/// through `g1`: `X = 0`, no clusters.
#[test]
fn the_explainers_presets_cross_at_the_roots_of_h() {
    let presets = [
        ("equal", [40.0, 160.0, 40.0, 160.0], 190.0, 15.0),
        ("overlap", [30.0, 150.0, 90.0, 230.0], 210.0, 15.0),
        ("einside", [70.0, 150.0, 20.0, 220.0], 200.0, 15.0),
        ("ginside", [20.0, 220.0, 70.0, 150.0], 200.0, 15.0),
        ("abut", [0.0, 90.0, 90.0, 180.0], 60.0, 0.0),
        ("apart", [20.0, 100.0, 200.0, 300.0], 60.0, 15.0),
        ("cancel", [70.0, 150.0, 20.0, 220.0], 60.0, 65.0),
        ("booked", [70.0, 150.0, 20.0, 220.0], 40.0, 65.0),
    ];
    let mut crossings = 0;
    for (name, ends, phi, rho) in presets {
        let knots = model_knots(&ends);
        let ix = |a: f64| knots.iter().position(|&k| k == deg(a)).expect("a knot");
        for (back_e, back_g) in [(false, false), (false, true), (true, false), (true, true)] {
            let m = Model {
                ei: (ix(ends[0]), ix(ends[1])),
                gi: (ix(ends[2]), ix(ends[3])),
                knots: knots.clone(),
                phi: deg(phi),
                rho: deg(rho),
                vz: 0.2,
                tilt: 0.8,
                back_e,
                back_g,
                w: vec![1.0],
            };
            let here = format!("{name} {back_e} {back_g}");
            let (r, x) = shared_rec(&m.geom(), true).expect("decided");
            let rec = r.unwrap_or_else(|e| panic!("{here}: {e:?}"));
            if name == "cancel" {
                // The root at `e`'s end, which the stored knot misses by
                // its rounding either way: no crossing inside by more.
                assert!(rec.clusters.len() <= 1, "{here}: {rec:?}");
                for c in &rec.clusters {
                    assert!(c.e.t[0] < 1e-9 || c.e.t[1] > 1.0 - 1e-9, "{here}: {c:?}");
                }
                assert_eq!(rec.degree(), x, "{here}");
                continue;
            }
            let moved = m.moved().expect("no tie");
            crossings += moved.len();
            if matches!(name, "abut" | "apart") {
                assert!(rec.clusters.is_empty() && x == 0, "{here}");
            }
            if name == "booked" {
                assert_eq!(moved.len(), 1, "{here}");
            }
            check_model(&m, &rec, x, &moved, &here);
        }
    }
    assert!(crossings > 8, "{crossings} crossings");
}

/// Abutting quarter circles under motions all round, `a`'s ray through
/// `g`'s far end: the overlap is their common end, so `X = 0` and no
/// clusters.
#[test]
fn abutting_quarters_have_no_clusters() {
    let knots = [0.0, 90.0, 180.0].map(deg).to_vec();
    for phi in (0..24).map(|k| 7.5 + 15.0 * f64::from(k)) {
        for (vz, tilt) in [(0.2, 0.8), (-0.5, 0.0), (1.0, 1.5)] {
            for back_g in [false, true] {
                let m = Model {
                    knots: knots.clone(),
                    ei: (0, 1),
                    gi: (1, 2),
                    phi: deg(phi),
                    rho: 0.0,
                    vz,
                    tilt,
                    back_e: false,
                    back_g,
                    w: vec![1.0, 3.0],
                };
                let (r, x) = shared_rec(&m.geom(), true).expect("decided");
                assert_eq!(r, Ok(XRec::default()), "{phi} {vz} {back_g}");
                assert_eq!(x, 0, "{phi} {vz} {back_g}");
            }
        }
    }
}

/// Random arcs of one circle (equal, overlapping, nested, abutting,
/// apart; either stored direction of either; weights 1/64 to 64), random
/// motions and frames, against the moved arcs sampled densely: every pair
/// answered with the moved arcs' crossings and `s11`, none refused but at
/// a tie the sampling can't tell (a crossing at an end).
#[test]
fn random_shared_arcs_cross_at_the_roots_of_h() {
    let (mut answered, mut crossing, mut tied, mut refused, mut trials) = (0, 0, 0, 0, 0);
    for seed in varde_testing::seeds(1000, 20_000) {
        let mut rng = Rng::new(seed);
        let mut f = || (rng.next_u64() % 1_000_000) as f64 / 1_000_000.0;
        let mut knots = vec![f() * 6.0];
        for _ in 0..6 {
            let l = knots[knots.len() - 1];
            knots.push(l + 0.3 + 0.55 * f());
        }
        let (phi, rho) = (f() * 6.3, f() * 6.3);
        let (vz, tilt) = (2.0 * f() - 1.0, 1.5 * f());
        let mut pick = || {
            let a = (f() * 6.0) as usize;
            let b = (f() * 6.0) as usize;
            if a == b {
                (a, a + 1)
            } else {
                (a.min(b), a.max(b) + 1)
            }
        };
        let (ei, gi) = (pick(), pick());
        let ws = [1.0 / 64.0, 0.5, 1.0, 3.0, 64.0];
        let w = (0..6).map(|_| ws[(f() * 5.0) as usize]).collect();
        let m = Model {
            knots,
            ei,
            gi,
            phi,
            rho,
            vz,
            tilt,
            back_e: seed % 4 >= 2,
            back_g: seed % 2 == 1,
            w,
        };
        let Some((r, x)) = shared_rec(&m.geom(), true) else {
            continue;
        };
        trials += 1;
        let moved = m.moved();
        match (r, moved) {
            (Ok(rec), Some(moved)) => {
                check_model(&m, &rec, x, &moved, &format!("seed {seed}"));
                answered += 1;
                crossing += usize::from(!moved.is_empty());
            }
            // A tie at an end: the record decides it by the orders, the
            // sampling can't; still summing to the four terms.
            (Ok(rec), None) => {
                assert_eq!(rec.degree(), x, "seed {seed}");
                tied += 1;
            }
            (Err(SharedError::Unsettled), None) => refused += 1,
            (Err(e), _) => panic!("seed {seed}: {e:?}"),
        }
    }
    eprintln!(
        "{answered} answered ({crossing} crossing), {tied} tied, {refused} refused of {trials}"
    );
    assert!(
        answered * 10 > trials * 9 && crossing > 0 && refused * 200 <= trials,
        "{answered} answered ({crossing} crossing), {tied} tied, {refused} refused of {trials}"
    );
}

/// What the rule can't read is refused, never answered: terms counting a
/// crossing the clusters don't hold (made by hand) `Inconsistent`; an end
/// of one chain inside a span of the other (the overlap not whole spans)
/// `Unaligned`; a line against a chain `Shape`.
#[test]
fn shared_pairs_the_rule_cannot_read_are_refused() {
    let mut geom = Geom {
        d: DVec3::new(0.25, -0.5, 0.125),
        ..Geom::default()
    };
    geom.segment(A, E, (0, lat(0.0, 0.0, 0.0)), (1, lat(2.0, 0.0, 1.0)));
    geom.segment(B, G, (2, lat(1.0, 0.0, 0.5)), (3, lat(3.0, 0.0, 1.5)));
    let recs = decide(&geom);
    let mut pair = recs.pair(&geom);
    pair.shared = true;
    let reads = shared_reads(&geom);
    let sides = XSides::new();
    assert_eq!(
        shared_clusters(&reads, &geom, &sides, X, &pair, &Charge::xrec(u64::MAX)).0,
        Ok(XRec::default())
    );
    let ray = |k| EdgeRays::Line(crate::records::shadow::RayRec { crossing: k });
    pair.terms.b_g = ray(Some(Sign::Pos));
    pair.terms.a_g = ray(None);
    let x = x_count(&pair.terms);
    assert_ne!(x, 0);
    assert_eq!(
        shared_clusters(&reads, &geom, &sides, X, &pair, &Charge::xrec(u64::MAX)).0,
        Err(SharedError::Inconsistent { clusters: 0, x })
    );

    // `g` from 45° to 135° against `e` from 0° to 90°: `g`'s start lies
    // inside `e`'s span.
    let e = model_arcs(&[0.0, 90.0].map(deg), 0.3, 0.8, &[1.0]);
    let g = model_arcs(&[45.0, 90.0, 135.0].map(deg), 0.3, 0.8, &[1.0]);
    assert_eq!(e[0][2], g[0][2], "one knot at 90°");
    let mut geom = Geom {
        d: model_motion(1.0, 0.3, 0.2),
        ..Geom::default()
    };
    geom.curve(A, E, 0, 1, e);
    geom.curve(B, G, 2, 3, g);
    let (r, _) = shared_rec(&geom, true).expect("decided");
    assert_eq!(r, Err(SharedError::Unaligned(Point::vert(B, VertId(2)))));

    let mut geom = Geom::default();
    let arc = model_arcs(&[0.0, 90.0].map(deg), 0.0, 0.0, &[1.0]);
    let [p, q] = [arc[0][0], arc[0][2]].map(|c| DVec3::new(c[0], c[1], c[2]));
    geom.curve(A, E, 0, 1, arc);
    geom.segment(B, G, (2, p), (3, q));
    let recs = decide(&geom);
    let mut pair = recs.pair(&geom);
    pair.shared = true;
    let reads = shared_reads(&geom);
    assert_eq!(
        shared_clusters(
            &reads,
            &geom,
            &XSides::new(),
            X,
            &pair,
            &Charge::xrec(u64::MAX)
        )
        .0,
        Err(SharedError::Shape)
    );
}

/// The point at `t` of the straight edge from `a` to `b` moved by `δ`
/// along the geometry's field, in the shadow frame `(r̂·x, ν·x)` and its
/// height `ê·x` (`f64`).
fn moved_line_point(geom: &Geom, a: DVec3, b: DVec3, t: f64, delta: f64) -> ([f64; 2], f64) {
    let p = a + (b - a) * t;
    let m = p + geom.d_at(p) * delta;
    ([m.dot(R_HAT), m.dot(NU)], m.dot(UP))
}

/// A straight shared line from a vertex `a` of `A` whose blend zone (the
/// vertex's direction within `r` of it, the field's own from `2r` on)
/// turns the motion's shadow across the line: `h` changes sign inside the
/// zone, and the moved edges cross there, at the root (sampled densely on
/// the moved line, `δ = 10⁻⁷`): one cluster holding it, of its degree,
/// `s11` the moved crossing's z-order.
#[test]
fn a_straight_shared_line_in_a_blend_zone_crosses_at_its_root() {
    let mut checked = 0;
    for (d_vertex, d_far) in [
        (lat(0.5, 1.0, 0.25), lat(0.5, -1.0, 0.5)),
        (lat(-0.5, -1.0, 0.75), lat(0.25, 1.0, -0.5)),
        (lat(1.0, 0.5, -0.25), lat(1.0, -2.0, 0.0)),
    ] {
        for back in [false, true] {
            let (a, b) = (lat(0.0, 0.0, 0.0), lat(4.0, 0.0, 1.0));
            let mut geom = Geom {
                d: d_far,
                blend: Some(Blend {
                    at: a,
                    d: d_vertex,
                    r: (b - a).length() / 4.0,
                }),
                ..Geom::default()
            };
            // `e` from `a` (vertex 0) or to it (stored backwards).
            if back {
                geom.segment(A, E, (0, b), (1, a));
            } else {
                geom.segment(A, E, (0, a), (1, b));
            }
            geom.segment(B, G, (2, a), (3, b));
            let (r, x) = shared_rec(&geom, true).expect("decided");
            let here = format!("{d_vertex} {back}");
            let rec = r.unwrap_or_else(|e| panic!("{here}: {e:?}"));
            // The moved line sampled: its offset across the line's shadow
            // changes sign at the crossing.
            let delta = 1e-7;
            let (e0, e1) = if back { (b, a) } else { (a, b) };
            let across = |t: f64| {
                let ([u, v], _) = moved_line_point(&geom, e0, e1, t, delta);
                let ([u0, v0], _) = moved_line_point(&geom, e0, e1, 0.0, 0.0);
                let ([u1, v1], _) = moved_line_point(&geom, e0, e1, 1.0, 0.0);
                (u1 - u0) * (v - v0) - (v1 - v0) * (u - u0)
            };
            let mut moved = Vec::new();
            let n = 4000;
            for i in 0..n {
                let (s, t) = (f64::from(i) / f64::from(n), f64::from(i + 1) / f64::from(n));
                if (across(s) < 0.0) != (across(t) < 0.0) {
                    let (mut lo, mut hi) = (s, t);
                    for _ in 0..60 {
                        let m = 0.5 * (lo + hi);
                        if (across(m) < 0.0) == (across(s) < 0.0) {
                            lo = m;
                        } else {
                            hi = m;
                        }
                    }
                    // `e` from the right of `g` (same way) to its left:
                    // `T_g × T_e > 0` (read off the root, where the
                    // rounding can't flip it).
                    let at = 0.5 * (lo + hi);
                    let up = across(at + 1e-3) > 0.0;
                    let degree = if up { 1 } else { -1 };
                    // The moved `e`'s height against the line's there.
                    let ([u, v], z) = moved_line_point(&geom, e0, e1, at, delta);
                    let ([u0, v0], z0) = moved_line_point(&geom, a, b, 0.0, 0.0);
                    let ([u1, v1], z1) = moved_line_point(&geom, a, b, 1.0, 0.0);
                    let s = ((u - u0) * (u1 - u0) + (v - v0) * (v1 - v0))
                        / ((u1 - u0) * (u1 - u0) + (v1 - v0) * (v1 - v0));
                    let gap = z0 + s * (z1 - z0) - z;
                    moved.push((at, degree * if back { -1 } else { 1 }, gap > 0.0));
                }
            }
            assert_eq!(moved.len(), 1, "{here}: {moved:?}");
            let (at, degree, above) = moved[0];
            assert_eq!(x, degree, "{here}");
            assert_eq!(rec.clusters.len(), 1, "{here}: {rec:?}");
            let c = rec.clusters[0];
            assert_eq!(c.degree, degree, "{here}");
            assert!(
                c.e.t[0] - 1e-6 <= at && at <= c.e.t[1] + 1e-6,
                "{here}: {c:?} {at}"
            );
            // Inside the blend zone, `r` to `2r` from `a`.
            let from_a = if back { 1.0 - at } else { at };
            assert!(0.25 < from_a && from_a < 0.5, "{here}: {at}");
            assert_eq!(
                s11(&geom, &rec),
                Some(if above { degree } else { 0 }),
                "{here}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 6);
}

/// A straight edge of `A` whose shadow lies on the ray line of points of
/// `B` and runs through a vertex's blend zone, where the field turns its
/// motion's shadow across the line and back (the ends outside the zone
/// on one side, its middle on the other): each point's ray record counts
/// the moved edge's crossings of the moved ray, sampled densely (each
/// change of side along the edge a crossing, on the ray where it is
/// ahead of the point), also where the ends' records agree and a point
/// lies between the two changes.
#[test]
fn a_straight_edge_on_a_ray_line_in_a_blend_zone_counts_the_moved_crossings() {
    let mut between = 0;
    for (d_vertex, d_far) in [
        (lat(0.5, 1.0, 0.25), lat(0.5, -1.0, 0.5)),
        (lat(-0.5, -1.0, 0.75), lat(0.25, 1.0, -0.5)),
        (lat(1.0, 0.5, -0.25), lat(1.0, -2.0, 0.0)),
    ] {
        for back in [false, true] {
            let (a, b) = (lat(-4.0, 0.0, 0.0), lat(4.0, 0.0, 1.0));
            let mut geom = Geom {
                d: d_far,
                blend: Some(Blend {
                    at: (a + b) * 0.5,
                    d: d_vertex,
                    r: (b - a).length() / 8.0,
                }),
                ..Geom::default()
            };
            let (e0, e1) = if back { (b, a) } else { (a, b) };
            geom.segment(A, E, (0, e0), (1, e1));
            let xs = [-6.0, -2.5, -1.25, -0.25, 0.5, 1.25, 2.0, 4.0, 6.0];
            let qs: Vec<Point> = xs
                .iter()
                .enumerate()
                .map(|(i, &x)| {
                    let p = Point::vert(B, VertId(10 + i as u32));
                    geom.vertex(B, 10 + i as u32, lat(x, 0.0, 0.5 - x / 8.0));
                    p
                })
                .collect();
            let keys = qs.iter().map(|&q| RayKey { q, g: EdgeId(E) });
            let mut shadows = Shadows::new();
            shadows
                .fill_rays(keys, &geom, band(), &mut Seq::default())
                .expect("decided");
            // The moved edge's side of the points' common ray line along
            // it: `ν·d` at its point (its order 0 is 0), sampled.
            let side = |t: f64| geom.d_at(e0 + (e1 - e0) * t).dot(NU) > 0.0;
            let n = 4000;
            let changes: Vec<(f64, bool)> = (0..n)
                .filter_map(|i| {
                    let (s, t) = (f64::from(i) / f64::from(n), f64::from(i + 1) / f64::from(n));
                    (side(s) != side(t)).then(|| (0.5 * (s + t), side(t)))
                })
                .collect();
            assert!(!changes.is_empty(), "{d_vertex} {back}");
            for &q in &qs {
                let at = geom.pos[&q];
                let want: i8 = changes
                    .iter()
                    .filter(|(t, _)| (e0 + (e1 - e0) * *t - at).dot(R_HAT) > 0.0)
                    .map(|&(_, to)| if to { 1 } else { -1 })
                    .sum();
                let got = shadows.ray(q, EdgeId(E)).expect("stored").count();
                assert_eq!(got, want, "{d_vertex} {back} {at}");
                let ends = [0, 1].map(|j| shadows.vv(q, VertId(j)).expect("stored").place.side);
                between += usize::from(ends[0] == ends[1] && want != 0);
            }
            shadows.replay(&geom, band()).expect("replays");
        }
    }
    assert!(between > 0);
}

/// A straight span from `p` to `q` (middle at the midpoint, weight 1).
fn line_span(p: DVec3, q: DVec3) -> Span {
    [cp(p, 1.0), cp((p + q) * 0.5, 1.0), cp(q, 1.0)]
}

/// A chain whose shadow crosses itself (a straight run, a loop of three
/// quarter circles, a straight run down across the first: a crossing at
/// `x = −1` with the second run a unit above), shared whole by `e` and
/// `g`: two crossings away from the overlap, of opposite degrees and
/// z-orders, found by the box search off the diagonal band (`X` alone
/// can't see them: they cancel), and the roots of `h` on the loop. Each
/// cluster against the moved chain: the away crossings by their
/// geometry, `s11` all of them.
#[test]
fn a_chain_crossing_itself_has_crossings_away_from_the_overlap() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let p = |x: f64, y: f64, z: f64| lat(x, y, z);
    // The loop: a circle of radius 1 about (0, 1), from −90° round to
    // 180°, rising from height 0 to 1.
    let chain = vec![
        line_span(p(-3.0, 0.0, 0.0), p(0.0, 0.0, 0.0)),
        [
            cp(p(0.0, 0.0, 0.0), 1.0),
            cp(p(1.0, 0.0, 0.25), w),
            cp(p(1.0, 1.0, 0.25), 1.0),
        ],
        [
            cp(p(1.0, 1.0, 0.25), 1.0),
            cp(p(1.0, 2.0, 0.5), w),
            cp(p(0.0, 2.0, 0.5), 1.0),
        ],
        [
            cp(p(0.0, 2.0, 0.5), 1.0),
            cp(p(-1.0, 2.0, 0.75), w),
            cp(p(-1.0, 1.0, 1.0), 1.0),
        ],
        line_span(p(-1.0, 1.0, 1.0), p(-1.0, -2.0, 1.0)),
    ];
    let mut done = 0;
    // The roots of `h` on the loop: where its tangent runs along the
    // motion's shadow, at −16.7° and 163.3° for the first (on the first
    // and third arcs), at 78.7° for the second (on the second).
    for (d, arcs) in [
        (lat(0.3, 1.0, 0.2), vec![1, 3]),
        (lat(-1.0, 0.2, -0.3), vec![2]),
    ] {
        for (back_e, back_g) in [(false, false), (true, false), (false, true)] {
            let mut geom = Geom {
                d,
                ..Geom::default()
            };
            let flip = |b: bool| if b { reversed(&chain) } else { chain.clone() };
            geom.curve(A, E, 0, 1, flip(back_e));
            geom.curve(B, G, 2, 3, flip(back_g));
            let here = format!("{d} {back_e} {back_g}");
            let (r, x) = shared_rec(&geom, true).expect("decided");
            let rec = r.unwrap_or_else(|e| panic!("{here}: {e:?}"));
            assert_eq!(rec.degree(), x, "{here}");
            let away: Vec<&XCluster> = rec
                .clusters
                .iter()
                .filter(|c| c.row == ZRow::EdgeEdge)
                .collect();
            assert_eq!(away.len(), 2, "{here}: {rec:?}");
            assert_eq!(away[0].degree + away[1].degree, 0, "{here}");
            // The first run of `e` against the last of `g` and back: the
            // crossing at `x = −1`, `g`'s last run above where it is `g`'s.
            let first = |back: bool| if back { 4 } else { 0 };
            let last = |back: bool| if back { 0 } else { 4 };
            let mut s11_away = 0;
            for c in &away {
                let e_first = c.e.span == first(back_e);
                let g_last = c.g.span == last(back_g);
                assert!(
                    e_first == g_last && (e_first || c.e.span == last(back_e)),
                    "{here}: {c:?}"
                );
                // `T_g × T_e` of the runs, each along its stored way.
                let dir = |run_first: bool, back: bool| {
                    let v = if run_first { (1.0, 0.0) } else { (0.0, -1.0) };
                    if back { (-v.0, -v.1) } else { v }
                };
                let te = dir(e_first, back_e);
                let tg = dir(!e_first, back_g);
                let want = if tg.0 * te.1 - tg.1 * te.0 > 0.0 {
                    1
                } else {
                    -1
                };
                assert_eq!(c.degree, want, "{here}: {c:?}");
                if g_last {
                    s11_away += c.degree;
                }
            }
            // The roots of `h` on the loop: one cluster each, on an arc.
            let roots: Vec<&XCluster> = rec
                .clusters
                .iter()
                .filter(|c| matches!(c.row, ZRow::Root { .. }))
                .collect();
            let mut on: Vec<u32> = roots
                .iter()
                .map(|c| if back_e { 4 - c.e.span } else { c.e.span })
                .collect();
            on.sort_unstable();
            assert_eq!(on, arcs, "{here}: {rec:?}");
            // The roots' z-orders by the model's sampled moved loop: the
            // z-order row's sign at the root, read by the record.
            let z = z_orders(&geom, &rec);
            assert!(z.iter().all(Option::is_some), "{here}");
            let s = s11(&geom, &rec).expect("decided");
            let s_roots: i64 = rec
                .clusters
                .iter()
                .zip(&z)
                .filter(|(c, z)| matches!(c.row, ZRow::Root { .. }) && **z == Some(Sign::Pos))
                .map(|(c, _)| c.degree)
                .sum();
            assert_eq!(s, s_roots + s11_away, "{here}");
            done += 1;
        }
    }
    assert_eq!(done, 6);
}

/// The cut points of a pair with two roots inside the overlap, against
/// the knots of `e`'s curve: ordered by the sign of `h` there, and the
/// same under the other stored direction of `e` (an id permutation),
/// read along the curve from `e`'s geometrically lesser end; the root
/// clusters' indices follow that order, the same roots either way.
#[test]
fn cut_points_order_by_h_whatever_the_stored_direction() {
    let knots = model_knots(&[0.0, 250.0]);
    let mut checked = 0;
    // Roots at a knot (100°), inside spans, two inside the overlap.
    for phi in [10.0, 47.0, 95.0, 130.0] {
        let mut ordered = Vec::new();
        for back_e in [false, true] {
            let m = Model {
                knots: knots.clone(),
                ei: (0, knots.len() - 1),
                gi: (0, knots.len() - 1),
                phi: deg(phi),
                rho: deg(15.0),
                vz: 0.2,
                tilt: 0.8,
                back_e,
                back_g: false,
                w: vec![1.0, 3.0],
            };
            let geom = m.geom();
            let (r, _) = shared_rec(&geom, true).expect("decided");
            let rec = r.expect("answered");
            let moved = m.moved().expect("no tie");
            assert_eq!(rec.clusters.len(), moved.len(), "{phi}");
            let pair = decide(&geom);
            let pair = pair.pair(&geom);
            // Each cluster against each knot of `e`, as the angle of the
            // knot against the root's angle along the curve.
            let mut row = Vec::new();
            for c in &rec.clusters {
                let mut against = Vec::new();
                let n = pair.e.span_count();
                for k in 0..=n {
                    // Each span's start, and the last one's end.
                    let w = if k < n {
                        CurveAt { span: k, t: 0.0 }
                    } else {
                        CurveAt {
                            span: n - 1,
                            t: 1.0,
                        }
                    };
                    let o = root_order(&geom, &geom, X, &pair.e, c, w).expect("ordered");
                    // In stored order; along increasing angle where `e`
                    // isn't stored backwards.
                    let o = if back_e { o.reverse() } else { o };
                    // Rounded to tell the knots apart, whichever way
                    // their angles were worked out.
                    let angle = (m.e_angle(w.span, w.t) * 1e6).round() / 1e6;
                    against.push((angle, o));
                }
                against.sort_by(|a, b| a.0.total_cmp(&b.0));
                row.push((c.degree * if back_e { -1 } else { 1 }, against));
            }
            // The roots in order of angle: the record's order along the
            // curve from the lesser end, reversed where that is the far
            // end.
            ordered.push(row);
            checked += 1;
        }
        // The same roots in the same order, each against each knot.
        let [fwd, back] = [&ordered[0], &ordered[1]];
        assert_eq!(fwd.len(), back.len(), "{phi}");
        for (a, b) in fwd.iter().zip(back.iter()) {
            assert_eq!(a, b, "{phi}");
        }
        assert!(!fwd.is_empty(), "{phi}");
        // Against the knots in angle order: after all knots before the
        // root, before all after it.
        for (_, against) in fwd {
            let firsts: Vec<Ordering> = against.iter().map(|x| x.1).collect();
            let switch = firsts
                .iter()
                .position(|o| *o == Ordering::Less)
                .unwrap_or(firsts.len());
            assert!(
                firsts[..switch].iter().all(|o| *o == Ordering::Greater),
                "{phi}: {firsts:?}"
            );
            assert!(
                firsts[switch..].iter().all(|o| *o == Ordering::Less),
                "{phi}: {firsts:?}"
            );
        }
    }
    assert_eq!(checked, 8);
}
