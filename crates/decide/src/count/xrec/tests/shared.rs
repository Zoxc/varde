//! Edge pairs on one curve: no geometric search, no clusters at their
//! ends' ties. A pair whose `h = ê·(T × Δ)` is one sign over the overlap
//! has no clusters and four terms counting 0; one where `h` changes sign
//! (the moved edges cross there) is refused `Crossed`; never a record
//! whose `s11` differs from the moved configuration's. Checked against
//! the shared-curve model of two arcs on one circle (the moved shadows
//! cross exactly at the sign changes of `h` in the overlap), on its
//! presets and on random arcs, and on straight edges against their moved
//! crossing.

use super::*;
use crate::num::trig::{cos, sin};
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
                closed: false,
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

/// The pair's record under the shared-curve rule, and its `X`.
fn shared(geom: &Geom) -> Option<(Result<XRec, SharedError>, i64)> {
    let recs = try_decide(geom)?;
    let mut pair = recs.pair(geom);
    pair.shared = true;
    assert_eq!(recs.clusters(geom, &pair), Err(XClusterError::Shared));
    let reads = shared_reads(geom);
    let x = x_count(&pair.terms);
    Some((shared_clusters(&reads, geom, X, &pair), x))
}

fn qcross(a: &[Q; 3], b: &[Q; 3]) -> [Q; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

/// Straight edges on one line, overlapping, abutting or apart, moved by
/// a translation: the moved shadows are parallel, so they never cross.
/// No clusters (the former end-tie clusters came in cancelling pairs) and
/// `X = 0`; refused only where `h` is exactly 0 (the motion's shadow
/// along the line), which no level settles.
#[test]
fn edges_on_one_line_moved_apart_have_no_clusters() {
    let (mut formed, mut overlapping, mut flat) = (0, 0, 0);
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
        let (r, x) = shared(&geom).expect("decided");
        if let Some(want) = moved_crossing(&geom) {
            assert_eq!(x, want, "seed {seed}");
        }
        // The two ranges meet: each starts no later than the other ends.
        let sorted = |a: f64, b: f64| if a < b { [a, b] } else { [b, a] };
        let ([e0, e1], [g0, g1]) = (sorted(t[0], t[1]), sorted(t[2], t[3]));
        let meet = e0 <= g1 && g0 <= e1;
        let h0 = qdot(&q3(UP), &qcross(&q3(dir), &q3(d))) == 0u32;
        match r {
            Ok(rec) => {
                assert!(rec.clusters.is_empty(), "seed {seed}");
                assert_eq!(x, 0, "seed {seed}");
                assert!(!(meet && h0), "seed {seed}: h is 0 on the overlap");
                formed += 1;
                overlapping += i32::from(meet);
            }
            Err(SharedError::Unsettled) => {
                assert!(meet && h0, "seed {seed}");
                flat += 1;
            }
            Err(e) => panic!("seed {seed}: {e:?}"),
        }
    }
    assert!(
        formed > 0 && overlapping > 0,
        "{formed} {overlapping} {flat}"
    );
}

/// Edges on one line under a direction field varying along it (linear,
/// so straight edges stay straight, `d = (field·p)·ν`): `h` changes sign
/// where `field·p` does. With that point inside the overlap the moved
/// edges cross there and the pair is refused `Crossed`; outside it the
/// pair has no clusters and `X = 0`. The four terms count the moved
/// crossing either way.
#[test]
fn a_varying_field_on_one_line_is_refused_where_h_changes_sign() {
    let (mut crossed, mut formed) = (0, 0);
    for dir in [lat(1.0, 0.0, 0.0), lat(1.0, 2.0, 0.0), lat(0.0, 1.0, 1.0)] {
        for k in [1.0, -1.0] {
            for (t0, t1) in [(-1.0, 2.0), (0.5, 2.0), (-2.0, -0.25)] {
                let at = |t: f64| dir * t;
                let mut geom = Geom {
                    field: dir * k,
                    ..Geom::default()
                };
                geom.segment(A, E, (0, at(t0)), (1, at(t1)));
                geom.segment(B, G, (2, at(-3.0)), (3, at(3.0)));
                let (r, x) = shared(&geom).expect("decided");
                assert_eq!(Some(x), moved_crossing(&geom), "{dir} {k} {t0}");
                // `h ∝ ê·(dir × ν)·(field·p)`: identically 0 where the
                // line's shadow is along `ν`.
                let h0 = qdot(&q3(UP), &qcross(&q3(dir), &q3(NU))) == 0u32;
                match r {
                    Ok(rec) => {
                        assert!(t0 > 0.0 || t1 < 0.0, "{dir} {k} {t0}");
                        assert!(rec.clusters.is_empty() && x == 0, "{dir} {k} {t0}");
                        formed += 1;
                    }
                    Err(SharedError::Crossed) => {
                        assert!(t0 < 0.0 && 0.0 < t1, "{dir} {k} {t0}");
                        crossed += i32::from(x != 0);
                    }
                    Err(SharedError::Unsettled) => assert!(h0, "{dir} {k} {t0}"),
                    Err(e) => panic!("{dir} {k} {t0}: {e:?}"),
                }
            }
        }
    }
    assert!(crossed > 0 && formed > 0, "{crossed} {formed}");
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
/// `g` over `gi`, `g` stored backwards if `back`.
struct Model {
    knots: Vec<f64>,
    ei: (usize, usize),
    gi: (usize, usize),
    phi: f64,
    rho: f64,
    vz: f64,
    tilt: f64,
    back: bool,
    w: Vec<f64>,
}

impl Model {
    fn geom(&self) -> Geom {
        let arcs = model_arcs(&self.knots, self.rho, self.tilt, &self.w);
        let e = arcs[self.ei.0..self.ei.1].to_vec();
        let g = arcs[self.gi.0..self.gi.1].to_vec();
        let g = if self.back { reversed(&g) } else { g };
        let mut geom = Geom {
            d: model_motion(self.phi, self.rho, self.vz),
            ..Geom::default()
        };
        geom.curve(A, E, 0, 1, e);
        geom.curve(B, G, 2, 3, g);
        geom
    }

    /// The overlap `[lo, hi]` of the two arcs (model angles), if they
    /// meet.
    fn overlap(&self) -> Option<[f64; 2]> {
        let lo = self.knots[self.ei.0.max(self.gi.0)];
        let hi = self.knots[self.ei.1.min(self.gi.1)];
        (lo <= hi).then_some([lo, hi])
    }

    /// The roots of `h = T × v = −cos(s − phi)` (the model's, in its
    /// angles) in the overlap, as their distance inside it from its
    /// nearer end (negative outside): the moved arcs cross at each root
    /// strictly inside.
    fn roots(&self) -> Vec<f64> {
        let Some([lo, hi]) = self.overlap() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for k in -8..=8 {
            let s = self.phi + std::f64::consts::FRAC_PI_2 + f64::from(k) * std::f64::consts::PI;
            let inside = if s - lo < hi - s { s - lo } else { hi - s };
            if inside > -0.1 {
                out.push(inside);
            }
        }
        out
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

/// The presets of the shared-curve explainer, the motion of each
/// (`e` and `g` from and to in degrees, motion angle, ray angle), height
/// `0.8` tilted, motion up `0.2`. "cancel" (no crossing; the former rule
/// formed a cancelling pair of end clusters whose z-orders differ, a
/// wrong `s11`) has no clusters; "booked" (a crossing inside the overlap,
/// formerly booked at an end and reading the z-order there) is refused;
/// the others as `h` says. Abutting quarters (`a`'s ray exactly through
/// `g1`): `X = 0`, no clusters.
#[test]
fn the_explainers_presets_follow_the_sign_of_h() {
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
    for (name, ends, phi, rho) in presets {
        let knots = model_knots(&ends);
        let ix = |a: f64| knots.iter().position(|&k| k == deg(a)).expect("a knot");
        for back in [false, true] {
            let m = Model {
                ei: (ix(ends[0]), ix(ends[1])),
                gi: (ix(ends[2]), ix(ends[3])),
                knots: knots.clone(),
                phi: deg(phi),
                rho: deg(rho),
                vz: 0.2,
                tilt: 0.8,
                back,
                w: vec![1.0],
            };
            let geom = m.geom();
            let (r, x) = shared(&geom).expect("decided");
            let roots = m.roots();
            let crossing = roots.iter().any(|&d| d > 1e-9);
            let near = roots.iter().any(|d| d.abs() < 1e-9);
            match (name, r) {
                ("booked", r) => assert_eq!(r, Err(SharedError::Crossed), "{back}"),
                // A root of `h` at the overlap's end: no level settles it.
                ("cancel", r) => assert_eq!(r, Err(SharedError::Unsettled), "{back}"),
                (_, Err(SharedError::Unsettled)) if near => {}
                (_, Ok(rec)) if !crossing => {
                    assert!(rec.clusters.is_empty(), "{name} {back}");
                    assert_eq!(x, 0, "{name} {back}");
                }
                (_, Err(SharedError::Crossed)) if crossing => {}
                (_, r) => panic!("{name} {back}: {r:?}, crossing {crossing}"),
            }
            if name == "cancel" {
                assert!(near && !crossing);
            }
        }
    }
}

/// Abutting quarter circles under motions all round, `a`'s ray through
/// `g`'s far end: the overlap is their common end, where `h` isn't 0, so
/// `X = 0` and no clusters, never `Crossed`.
#[test]
fn abutting_quarters_have_no_clusters() {
    let knots = [0.0, 90.0, 180.0].map(deg).to_vec();
    for phi in (0..24).map(|k| 7.5 + 15.0 * f64::from(k)) {
        for (vz, tilt) in [(0.2, 0.8), (-0.5, 0.0), (1.0, 1.5)] {
            for back in [false, true] {
                let m = Model {
                    knots: knots.clone(),
                    ei: (0, 1),
                    gi: (1, 2),
                    phi: deg(phi),
                    rho: 0.0,
                    vz,
                    tilt,
                    back,
                    w: vec![1.0, 3.0],
                };
                let (r, x) = shared(&m.geom()).expect("decided");
                assert_eq!(r, Ok(XRec::default()), "{phi} {vz} {back}");
                assert_eq!(x, 0, "{phi} {vz} {back}");
            }
        }
    }
}

/// Random arcs of one circle (equal, overlapping, nested, abutting,
/// apart; either stored direction; weights 1/64 to 64), random motions
/// and frames, against the model: a pair answered has no clusters, and
/// no root of `h` in its overlap, so its `s11` (0) is the moved arcs';
/// one refused `Crossed` has a root inside; `Unsettled` only with a root
/// within `10⁻³` of the overlap's ends; nothing else refused.
#[test]
fn random_shared_arcs_never_answer_a_wrong_s11() {
    let (mut formed, mut crossed, mut unsettled, mut trials) = (0, 0, 0, 0);
    for seed in varde_testing::seeds(300, 20_000) {
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
            back: seed % 2 == 1,
            w,
        };
        let Some((r, x)) = shared(&m.geom()) else {
            continue;
        };
        trials += 1;
        let roots = m.roots();
        let near = roots.iter().any(|d| d.abs() < 1e-3);
        // Inside by more than the stored spans' rounding moves a root.
        let inside = roots.iter().any(|&d| d > 1e-9);
        match r {
            Ok(rec) => {
                assert!(!inside, "seed {seed}: a crossing answered");
                assert!(rec.clusters.is_empty() && x == 0, "seed {seed}");
                formed += 1;
            }
            Err(SharedError::Crossed) => {
                assert!(inside, "seed {seed}: refused without a crossing");
                crossed += 1;
            }
            Err(SharedError::Unsettled) => {
                assert!(
                    near,
                    "seed {seed}: unsettled away from a root: {roots:?} {:?} {:?} {:?}",
                    m.ei,
                    m.gi,
                    m.overlap()
                );
                unsettled += 1;
            }
            Err(e) => panic!("seed {seed}: {e:?}"),
        }
    }
    eprintln!("{formed} formed, {crossed} crossed, {unsettled} unsettled of {trials}");
    assert!(
        formed > 0 && crossed > 0 && unsettled * 50 <= trials,
        "{formed} formed, {crossed} crossed, {unsettled} unsettled of {trials}"
    );
}

/// What the rule can't read is refused, never answered: terms counting a
/// crossing though `h` is one sign over the overlap (here made by hand:
/// a crossing away from the overlap) `Away`; an end of one chain inside
/// a span of the other (the overlap not whole spans) `Unaligned`; a line
/// against a chain `Shape`.
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
    assert_eq!(
        shared_clusters(&reads, &geom, X, &pair),
        Ok(XRec::default())
    );
    let ray = |k| EdgeRays::Line(crate::records::shadow::RayRec { crossing: k });
    pair.terms.b_g = ray(Some(Sign::Pos));
    pair.terms.a_g = ray(None);
    let x = x_count(&pair.terms);
    assert_ne!(x, 0);
    assert_eq!(
        shared_clusters(&reads, &geom, X, &pair),
        Err(SharedError::Away(x))
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
    let (r, _) = shared(&geom).expect("decided");
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
        shared_clusters(&reads, &geom, X, &pair),
        Err(SharedError::Shape)
    );
}
