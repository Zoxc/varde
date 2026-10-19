//! Clusters of edge pairs: a straight pair's one cluster (its clear
//! z-order against the moved crossing; shadows on one line left to the
//! shared-curve rule),
//! curved pairs' clusters summing to the four terms, each clear or of one
//! certified crossing, the same on a second run; pieces far apart in
//! height clear at once, pieces meeting in 3D refining to one crossing;
//! unclear clusters' z-orders read from their height records against the
//! sampled and the moved crossings.

use super::*;
use crate::ids::FaceId;
use crate::order::{EdgeKey, EntityOrder, VertKey};
use crate::records::height::Heights;
use crate::records::height::model::{FaceAt, HeightModel, XAt, XHeights};
use crate::records::keys::{FootId, HKey};

impl EntityOrder for Geom {
    type Key = ();

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)].0
    }

    fn vert_key(&self, operand: Operand, v: VertId) -> VertKey<()> {
        let p = self.pos[&Point::vert(operand, v)];
        VertKey::new([p.x, p.y, p.z], []).expect("no NaN")
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<()> {
        let [s, e] = self.edges[&(operand, g)].0;
        let mid = (self.pos[&Point::vert(operand, s)] + self.pos[&Point::vert(operand, e)]) * 0.5;
        EdgeKey::new(
            [s, e].map(|v| self.vert_key(operand, v)),
            [mid.x, mid.y, mid.z],
            [(), ()],
        )
        .expect("no NaN")
    }
}

/// Two straight edges: one cluster where `X ≠ 0`, its z-order clear
/// where the hulls part, the moved crossing's height gap; edges whose
/// shadows lie on one line are the shared-curve rule's.
#[test]
fn a_straight_pair_is_one_cluster() {
    let (mut collinear, mut clear, mut crossing) = (0, 0, 0);
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
        let out = match recs.clusters(&geom, &pair) {
            Err(XClusterError::Collinear) => {
                // One shadow line: `a`, `g0` and `g1` on `a → b`'s.
                let c = |p: DVec3| {
                    let (u, v) = (b - a, p - a);
                    q(u.dot(R_HAT)) * q(v.dot(NU)) - q(u.dot(NU)) * q(v.dot(R_HAT))
                };
                assert!(c(g0) == 0u32 && c(g1) == 0u32, "seed {seed}");
                collinear += 1;
                continue;
            }
            r => r.expect("a straight pair"),
        };
        assert_eq!(out.rec.degree(), x, "seed {seed}");
        assert_eq!(out.rec.clusters.len(), usize::from(x != 0), "seed {seed}");
        let Some(c) = out.rec.clusters.first() else {
            continue;
        };
        crossing += 1;
        let up = |p: DVec3| q(p.dot(UP));
        let [t, s] = moved_params(&geom).expect("not parallel");
        assert_eq!(c.row, ZRow::EdgeEdge, "seed {seed}");
        let one = Q::from(1u32);
        let ze = (&one - &t) * up(a) + &t * up(b);
        let zg = (&one - &s) * up(g0) + &s * up(g1);
        let gap = sgn(&(zg - ze));
        if let Some(z) = c.clear {
            clear += 1;
            let want = if z == Sign::Pos { 1 } else { -1 };
            assert_eq!(gap, want, "seed {seed}");
        }
    }
    assert!(
        clear > 0 && crossing > clear && collinear > 0,
        "{collinear} collinear, {clear} clear of {crossing}"
    );
}

#[test]
fn curved_clusters_sum_to_the_four_terms() {
    let (mut formed, mut trials, mut several) = (0, 0, 0);
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
        trials += 1;
        let Ok(out) = recs.clusters(&geom, &pair) else {
            continue;
        };
        formed += 1;
        assert_eq!(out.rec.degree(), x_count(&pair.terms), "seed {seed}");
        assert_eq!(out.rec.clusters.len(), out.boxes.len());
        assert!(out.boxes.is_sorted(), "seed {seed}");
        for (c, b) in out.rec.clusters.iter().zip(&out.boxes) {
            assert_ne!(c.degree, 0, "seed {seed}");
            assert_eq!(c.row, ZRow::EdgeEdge);
            assert_eq!(c.e.span, b.cell[0]);
            assert_eq!(c.g.t, [dy(b.s.lo()), dy(b.s.hi())]);
        }
        if out.rec.clusters.len() > 1 {
            several += 1;
        }
        let again = recs.clusters(&geom, &pair).expect("formed once");
        assert_eq!(again, out, "seed {seed}");
    }
    assert!(
        formed * 3 > trials * 2 && several > 0,
        "{formed} formed, {several} with several clusters, of {trials}"
    );
}

/// A curved `e` along `r̂` and a straight-span `g` across it, `g` lifted
/// by `dz`.
fn crossing_pair(dz: f64) -> Geom {
    let mut geom = Geom {
        d: DVec3::new(0.25, -0.5, 0.125),
        ..Geom::default()
    };
    let e = vec![[
        cp(lat(-4.0, 0.0, 0.0), 1.0),
        cp(lat(0.0, 1.0, 0.0), 0.5),
        cp(lat(4.0, 0.0, 0.0), 1.0),
    ]];
    let g = vec![straight(lat(0.5, -4.0, dz), lat(0.5, 4.0, dz), 2.0)];
    geom.curve(A, E, 0, 1, e);
    geom.curve(B, G, 0, 1, g);
    geom
}

#[test]
fn heights_apart_clear_at_once_and_meeting_pieces_refine_to_one_crossing() {
    let geom = crossing_pair(3.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    assert_eq!(x_count(&pair.terms), -1);
    let out = recs.clusters(&geom, &pair).expect("formed");
    assert_eq!(out.rec.clusters.len(), 1);
    assert_eq!(out.rec.clusters[0].clear, Some(Sign::Pos));
    assert_eq!(out.boxes[0], XBox::cells([1, 1]).next().expect("a cell"));

    let geom = crossing_pair(-3.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    let out = recs.clusters(&geom, &pair).expect("formed");
    assert_eq!(out.rec.clusters[0].clear, Some(Sign::Neg));

    // The two meet in 3D: the cluster's box refines until its shadow
    // tangents are certified transversal (one crossing), and reads its
    // record.
    let geom = crossing_pair(0.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    let out = recs.clusters(&geom, &pair).expect("formed");
    assert_eq!(out.rec.degree(), -1);
    assert_eq!(out.rec.clusters.len(), 1);
    let (c, b) = (&out.rec.clusters[0], &out.boxes[0]);
    assert_eq!((c.clear, c.degree), (None, -1));
    let piece = |curve: &XCurve<'_>, span: u32, x: Extent| {
        cut(&curve.span(span, &like()).expect("a span"), x, &like())
    };
    let (pe, pg) = (
        piece(&pair.e, b.cell[0], b.t),
        piece(&pair.g, b.cell[1], b.s),
    );
    assert!(transversal(&pe, &pg, &like(), &mut 0), "{b:?}");
}

#[test]
fn tangents_apart_and_parallel() {
    let span = |p: DVec3, q: DVec3| {
        HomCurve::from_affine([([p.x, p.y, p.z], 1.0), ([q.x, q.y, q.z], 1.0)], &like())
            .expect("a span")
    };
    let e = span(lat(0.0, 0.0, 0.0), lat(1.0, 0.0, 0.0));
    let across = span(lat(0.5, -1.0, 2.0), lat(0.5, 1.0, 2.0));
    let along = span(lat(0.0, 0.0, 5.0), lat(2.0, 0.0, -1.0));
    let mut pieces = 0;
    assert!(transversal(&e, &across, &like(), &mut pieces));
    assert!(!transversal(&e, &along, &like(), &mut pieces));
    assert!(pieces > 0, "the cones' work is counted");
}

/// A crossing of two curves' shadows found by the sampled oracle: its
/// cell, parameters, sign (`T_g × T_e`, as the terms count it) and the
/// height of `g` over `e` along `ê`.
struct Sampled {
    cell: [u32; 2],
    at: [f64; 2],
    sign: i64,
    gap: f64,
}

/// A span's point at `t`, in the frame `(r̂·x, ν·x, ê·x)`.
fn frame_at(span: &Span, t: f64) -> [f64; 3] {
    let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let mut p = DVec3::ZERO;
    let mut w = 0.0;
    for (c, bi) in span.iter().zip(b) {
        p += DVec3::new(c[0], c[1], c[2]) * (bi * c[3]);
        w += bi * c[3];
    }
    let p = p / w;
    [p.dot(R_HAT), p.dot(NU), p.dot(UP)]
}

/// The crossing of two spans' shadows near `at`, by Newton's method
/// (`None` if it doesn't converge).
fn newton(e: &Span, g: &Span, mut at: [f64; 2]) -> Option<[f64; 2]> {
    let h = 1e-7;
    for _ in 0..50 {
        let (pe, pg) = (frame_at(e, at[0]), frame_at(g, at[1]));
        let f = [pe[0] - pg[0], pe[1] - pg[1]];
        let de = frame_at(e, at[0] + h);
        let dg = frame_at(g, at[1] + h);
        let j = [
            [(de[0] - pe[0]) / h, -(dg[0] - pg[0]) / h],
            [(de[1] - pe[1]) / h, -(dg[1] - pg[1]) / h],
        ];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        let step = [
            (f[0] * j[1][1] - f[1] * j[0][1]) / det,
            (j[0][0] * f[1] - j[1][0] * f[0]) / det,
        ];
        at = [at[0] - step[0], at[1] - step[1]];
        if step[0].abs() < 1e-14 && step[1].abs() < 1e-14 {
            return Some(at);
        }
    }
    (at[0].is_finite() && at[1].is_finite()).then_some(at)
}

/// The shadows' crossings of two chains by fine polylines, `None` where
/// one is near a tangency, a knot, an end or a tie in height (the
/// oracle can't tell).
fn sampled_crossings(e: &[Span], g: &[Span]) -> Option<Vec<Sampled>> {
    const N: usize = 4096;
    const CHUNK: usize = 64;
    // Uniform, and crowded towards both ends, where large weights crowd
    // the curve.
    let mut ts: Vec<f64> = (0..=N).map(|i| i as f64 / N as f64).collect();
    for i in 1..N / 4 {
        let u = i as f64 / (N / 4) as f64;
        let u = u * u * u * u;
        ts.extend([u, 1.0 - u]);
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup();
    let n = (ts.len() - 1) / CHUNK * CHUNK;
    ts.truncate(n + 1);
    ts[n] = 1.0;
    let poly = |s: &Span| -> Vec<[f64; 3]> { ts.iter().map(|&t| frame_at(s, t)).collect() };
    let bound = |p: &[[f64; 3]]| {
        let mut b = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for q in p {
            for k in 0..2 {
                if q[k] < b[k][0] {
                    b[k][0] = q[k];
                }
                if q[k] > b[k][1] {
                    b[k][1] = q[k];
                }
            }
        }
        b
    };
    let cross = |u: [f64; 2], v: [f64; 2]| u[0] * v[1] - u[1] * v[0];
    // An end or knot on (or very near) the other chain's shadow is a tie
    // the moved configuration settles: not for the oracle.
    let near = |p: [f64; 3], chain: &[Span]| {
        chain.iter().any(|s| {
            poly(s).windows(2).any(|w| {
                let (a, b) = (w[0], w[1]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let len2 = d[0] * d[0] + d[1] * d[1];
                let u = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
                let x = [a[0] + u * d[0] - p[0], a[1] + u * d[1] - p[1]];
                x[0] * x[0] + x[1] * x[1] < 1e-6
            })
        })
    };
    for (chain, other) in [(e, g), (g, e)] {
        for s in chain {
            if [0.0, 1.0].into_iter().any(|t| near(frame_at(s, t), other)) {
                return None;
            }
        }
    }
    let mut out = Vec::new();
    for (i, se) in (0u32..).zip(e) {
        let pe = poly(se);
        for (j, sg) in (0u32..).zip(g) {
            let pg = poly(sg);
            for ce in (0..n).step_by(CHUNK) {
                let be = bound(&pe[ce..=ce + CHUNK]);
                for cg in (0..n).step_by(CHUNK) {
                    let bg = bound(&pg[cg..=cg + CHUNK]);
                    if (0..2).any(|k| be[k][1] < bg[k][0] || bg[k][1] < be[k][0]) {
                        continue;
                    }
                    for a in ce..ce + CHUNK {
                        for b in cg..cg + CHUNK {
                            let (p0, p1, q0, q1) = (pe[a], pe[a + 1], pg[b], pg[b + 1]);
                            let te = [p1[0] - p0[0], p1[1] - p0[1]];
                            let tg = [q1[0] - q0[0], q1[1] - q0[1]];
                            let d = [q0[0] - p0[0], q0[1] - p0[1]];
                            let den = cross(te, tg);
                            if den == 0.0 {
                                continue;
                            }
                            let u = cross(d, tg) / den;
                            let v = cross(d, te) / den;
                            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                                continue;
                            }
                            let at = newton(
                                se,
                                sg,
                                [
                                    ts[a] + u * (ts[a + 1] - ts[a]),
                                    ts[b] + v * (ts[b + 1] - ts[b]),
                                ],
                            )?;
                            let norm2 = |x: [f64; 2]| x[0] * x[0] + x[1] * x[1];
                            let sin2 = den * den / (norm2(te) * norm2(tg));
                            let ze = frame_at(se, at[0])[2];
                            let zg = frame_at(sg, at[1])[2];
                            let scale = 1.0 + ze.abs() + zg.abs();
                            let edge = |x: f64| !(1e-4..1.0 - 1e-4).contains(&x);
                            if sin2 < 1e-6
                                || (zg - ze).abs() < 1e-6 * scale
                                || edge(at[0])
                                || edge(at[1])
                            {
                                return None;
                            }
                            out.push(Sampled {
                                cell: [i, j],
                                at,
                                sign: if den < 0.0 { 1 } else { -1 },
                                gap: zg - ze,
                            });
                        }
                    }
                }
            }
        }
    }
    Some(out)
}

/// The test model's heights: no faces; straight edges as segments at
/// their rates where the record reads them.
impl HeightModel for Geom {
    fn plane(&self, _: Operand, _: FaceId) -> Option<(DVec3, f64)> {
        None
    }

    fn plane_rates(&self, q: Point, _: FaceId, _: DVec3) -> [Rate; ORDERS] {
        self.rates(q.operand.other())
    }

    fn edge_seg(&self, operand: Operand, g: EdgeId) -> Option<Seg> {
        let (ends, spans) = &self.edges[&(operand, g)];
        if !spans.is_empty() {
            return None;
        }
        let [a, b] = ends.map(|v| self.pos[&Point::vert(operand, v)]);
        Some(Seg {
            a,
            b,
            rates: self.rates(operand),
        })
    }

    fn foot_point(&self, _: Point, _: EdgeId, _: FootId) -> Option<Moving> {
        None
    }

    fn curved_target<S: Curved>(&self, _: HKey, _: &S) -> Option<[S; 3]> {
        None
    }

    fn face_at<S: Curved>(&self, _: Point, _: FaceId, _: &[S; 3], _: &S) -> Option<FaceAt<S>> {
        None
    }
}

/// The z-orders of `rec`'s clusters, `g` above `e` as `Pos`: clear, or
/// read from their height records decided on the record ([`XHeights`]);
/// `None` where a record isn't decided.
pub(super) fn z_orders(geom: &Geom, rec: &XRec) -> Vec<Option<Sign>> {
    let at = [XAt { x: X, rec }];
    let model = XHeights {
        model: geom,
        recs: &at,
        near: 1.0 / 1_073_741_824.0,
    };
    let mut heights = Heights::new();
    rec.iter()
        .map(|(c, cl)| {
            if cl.clear.is_some() {
                return cl.clear;
            }
            let z = zorder_key(X, c, cl.row);
            heights
                .fill([z.height_key(X.g)], &model, band(), &mut Seq::default())
                .ok()?;
            Some(z.above(&heights.get(z.key, ZKey::anchor(X.g))?))
        })
        .collect()
}

/// A random curved pair of the sampled trials (weights 1/64 to 64) and
/// its crossings by the sampled oracle; `None` where the oracle can't
/// tell or the records aren't decided.
fn sampled_pair(seed: u64) -> Option<(Geom, Vec<Sampled>)> {
    let ws = [1.0 / 64.0, 1.0 / 8.0, 1.0, 8.0, 64.0];
    let mut rng = Rng::new(seed);
    let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
    let mut pt = || lat(int(8), int(8), int(2));
    let (a, k, b, g0, h, g1) = (pt(), pt(), pt(), pt(), pt(), pt());
    let mut w = || ws[(rng.next_u64() % 5) as usize];
    let w: [f64; 6] = std::array::from_fn(|_| w());
    let mut geom = Geom {
        d: [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3],
        ..Geom::default()
    };
    let mut rng = Rng::new(seed ^ 0x5e);
    let e = vec![
        conic(&mut rng, a, k, [w[0], w[1]]),
        conic(&mut rng, k, b, [w[2], w[3]]),
    ];
    let g = vec![
        conic(&mut rng, g0, h, [w[4], w[5]]),
        conic(&mut rng, h, g1, [w[5], w[4]]),
    ];
    let found = sampled_crossings(&e, &g)?;
    geom.curve(A, E, 0, 1, e);
    geom.curve(B, G, 0, 1, g);
    Some((geom, found))
}

/// What [`check_sampled`] saw.
#[derive(Default)]
struct SampledRun {
    checked: usize,
    crossings: usize,
    unclear: usize,
}

/// Seed `seed`'s pair against the sampled oracle (see
/// [`s11_matches_the_sampled_crossings`]), adding to `run`.
fn check_sampled(seed: u64, run: &mut SampledRun) {
    let Some((geom, found)) = sampled_pair(seed) else {
        return;
    };
    let Some(recs) = try_decide(&geom) else {
        return;
    };
    let pair = recs.pair(&geom);
    let x = x_count(&pair.terms);
    assert_eq!(found.iter().map(|c| c.sign).sum::<i64>(), x, "seed {seed}");
    let Ok(out) = recs.clusters(&geom, &pair) else {
        return;
    };
    run.checked += 1;
    run.crossings += found.len();
    let inside = |c: &Sampled, b: &XBox| {
        c.cell == b.cell
            && dy(b.t.lo()) <= c.at[0]
            && c.at[0] <= dy(b.t.hi())
            && dy(b.s.lo()) <= c.at[1]
            && c.at[1] <= dy(b.s.hi())
    };
    let zs = z_orders(&geom, &out.rec);
    for ((cl, bx), z) in out.rec.clusters.iter().zip(&out.boxes).zip(&zs) {
        let held: Vec<&Sampled> = found.iter().filter(|c| inside(c, bx)).collect();
        let deg: i64 = held.iter().map(|c| c.sign).sum();
        assert_eq!(deg, cl.degree, "seed {seed}: {bx:?}");
        if cl.clear.is_none() {
            run.unclear += 1;
            assert_eq!(held.len(), 1, "seed {seed}: {bx:?}");
        }
        // Clear or read from its record, the z-order is the oracle's.
        let z = z.unwrap_or_else(|| panic!("seed {seed}: {bx:?} not decided"));
        for c in &held {
            assert_eq!(c.gap > 0.0, z == Sign::Pos, "seed {seed}: {bx:?}");
        }
    }
    let want: i64 = found.iter().filter(|c| c.gap > 0.0).map(|c| c.sign).sum();
    let got = out
        .rec
        .s11(|c, _| zs[c.0 as usize].ok_or(()))
        .expect("read");
    assert_eq!(got, want, "seed {seed}: {out:?}");
}

/// Random curved pairs, weights 1/64 to 64, against the sampled oracle:
/// each cluster holds the signed crossings of its box, an unclear one
/// exactly one crossing, its z-order (clear, or read from its height
/// record) the oracle's at each; `s11` is the oracle's sum over crossings
/// with `g` above `e`.
#[test]
fn s11_matches_the_sampled_crossings() {
    let mut run = SampledRun::default();
    for seed in varde_testing::seeds(80, 1200) {
        check_sampled(seed, &mut run);
    }
    assert!(
        run.checked > 10 && run.crossings > 10 && run.unclear > 0,
        "{} checked, {} crossings, {} unclear",
        run.checked,
        run.crossings,
        run.unclear
    );
}

/// Seed 285 of the sampled trials: two crossings of opposite signs and
/// z-orders close together, which a wide band once held in one box of
/// degree 0 that carried nothing. No size stops the refinement, so they
/// are two clusters and `s11` counts the one with `g` above.
#[test]
fn a_close_pair_of_opposite_z_orders_counts_one() {
    let (_, found) = sampled_pair(285).expect("the oracle tells");
    assert!(
        found.iter().any(|c| c.gap > 0.0) && found.iter().any(|c| c.gap < 0.0),
        "both z-orders"
    );
    let mut run = SampledRun::default();
    check_sampled(285, &mut run);
    assert_eq!(run.checked, 1);
}

/// `e`'s shadow a conic (middle weighted 1/64 to 64) touching `g`'s line
/// at its middle, at the
/// same height, `g` ending there: the motion leaves one crossing on `g`
/// or none. A box around the touching point never certifies its
/// tangents apart nor its z-order, whatever its degree (a degree-0 box
/// there may be two crossings of opposite z-orders): never a cluster,
/// never dropped. Charged per piece, the search stops on its share, at
/// the first level ([`Exhausted::Work`]), past it by one box's pieces at
/// most and the same on every run. Uncharged it escalates at its box cap
/// (a second in release builds per case and level): with
/// `VARDE_TESTS=full`, as are the twelve motions (two by default).
#[test]
fn a_shadow_tangency_escalates() {
    let (mut crossed, mut escalated, mut ran_out) = (0, 0, 0);
    let all: Vec<(DVec3, f64)> = [
        NU,
        -NU,
        DVec3::new(0.25, -0.5, 0.125),
        DVec3::new(0.25, 0.5, 0.125),
    ]
    .into_iter()
    .flat_map(|d| [1.0 / 64.0, 1.0, 64.0].map(|w| (d, w)))
    .collect();
    let full = varde_testing::pick(false, true);
    let cases = varde_testing::pick(vec![(NU, 1.0), (-NU, 1.0)], all);
    let charge = Charge::xrec(1 << 16);
    let unit = charge.units[0];
    for &(d, wm) in &cases {
        let mut geom = Geom {
            d,
            ..Geom::default()
        };
        let e = vec![[
            cp(lat(-1.0, 1.0, 0.0), 1.0),
            cp(lat(0.0, -1.0 / wm, 0.0), wm),
            cp(lat(1.0, 1.0, 0.0), 1.0),
        ]];
        geom.curve(A, E, 0, 1, e);
        let g = vec![straight(lat(-2.0, 0.0, 0.0), lat(0.0, 0.0, 0.0), 1.0)];
        geom.curve(B, G, 0, 1, g);
        let recs = decide(&geom);
        let pair = recs.pair(&geom);
        crossed += usize::from(x_count(&pair.terms) != 0);
        let (r, cost) = recs.clusters_on(&geom, &pair, &charge);
        match r {
            Err(XClusterError::OutOfBudget {
                cause: Exhausted::Work,
                at: Some(_),
            }) => ran_out += 1,
            r => panic!("{d} {wm}: {r:?}"),
        }
        let spent = charge.spent(&cost);
        assert!(
            spent > charge.share && spent <= charge.share + 64 * unit,
            "{d} {wm}: {spent}"
        );
        assert!(cost.levels[1..].iter().all(|l| l.evals == 0));
        assert_eq!(recs.clusters_on(&geom, &pair, &charge).1, cost);
        if full {
            match recs.clusters(&geom, &pair) {
                Err(e) if e.escalates() => escalated += 1,
                r => panic!("{d} {wm}: {r:?}"),
            }
        }
    }
    assert!(
        ran_out == cases.len()
            && (!full || escalated == cases.len())
            && crossed > 0
            && crossed < cases.len(),
        "{ran_out} ran out, {escalated} escalated, {crossed} with a crossing"
    );
}

/// `e` a conic dipping under `g`'s straight line near its apex, rising in
/// height across it: two crossings of opposite signs and z-orders, ever
/// closer. They are two clusters however close (no size stops the
/// refinement), `s11` the one with `g` above.
#[test]
fn close_crossings_of_opposite_z_orders() {
    for k in [2, 6, 10, 14] {
        let c = 1.0 / f64::from(1u32 << k);
        let mut geom = Geom {
            d: DVec3::new(0.25, -0.5, 0.125),
            ..Geom::default()
        };
        // Apex at `y = 0` (the middle at `y = −1` halfway up), `g` at
        // `y = c`.
        let e = vec![[
            cp(lat(-4.0, 1.0, -1.0), 1.0),
            cp(lat(0.0, -1.0, 0.0), 1.0),
            cp(lat(4.0, 1.0, 1.0), 1.0),
        ]];
        let g = vec![straight(lat(-7.25, c, 0.0), lat(8.75, c, 0.0), 1.0)];
        let found = sampled_crossings(&e, &g).expect("transversal");
        assert_eq!(found.len(), 2);
        let want: i64 = found.iter().filter(|c| c.gap > 0.0).map(|c| c.sign).sum();
        assert_ne!(want, 0);
        geom.curve(A, E, 0, 1, e);
        geom.curve(B, G, 0, 1, g);
        let recs = decide(&geom);
        let pair = recs.pair(&geom);
        assert_eq!(x_count(&pair.terms), 0);
        let out = recs.clusters(&geom, &pair).expect("formed");
        assert_eq!(out.rec.clusters.len(), 2, "{k}");
        let s11 = out.rec.s11(|_, _| Err(())).expect("both clear");
        assert_eq!(s11, want, "{k}");
    }
}

/// A curved `e` (a conic through the origin at its middle) and a straight
/// span `g` from there across it, lifted along `UP` by `lift` (the same
/// shadows). Unlifted, `g`'s end lies exactly on `e`: the cluster's
/// height is exactly 0 and the edge–edge orders decide it, the moved
/// configuration's z-order (by finite differences of the moved crossing).
/// Lifted apart, the value decides; lifted by less than the curved
/// target, a near tie, no level decides it (refused, never a band tie).
/// A motion either way: the crossing is there for one of `±d`.
#[test]
fn a_curved_cluster_at_an_end_on_the_other_edge_decides_by_its_orders() {
    let e = vec![[
        cp(lat(-1.0, -1.0, -1.0), 1.0),
        cp(lat(0.0, 1.0, 1.0), 1.0),
        cp(lat(1.0, -1.0, -1.0), 1.0),
    ]];
    let (mut by_orders, mut by_value, mut near) = (0, 0, 0);
    for d in [
        DVec3::new(0.25, -0.5, 0.125),
        DVec3::new(-0.25, 0.5, -0.125),
        NU,
        -NU,
        DVec3::new(1.0, 0.25, 0.0),
        DVec3::new(-1.0, -0.25, 0.0),
    ] {
        for lift in [0.0, 1.0 / 1024.0, -1.0 / 1024.0, 1.0 / 1_099_511_627_776.0] {
            let mut geom = Geom {
                d,
                ..Geom::default()
            };
            let g = vec![straight(UP * lift, lat(0.0, 2.0, 1.0) + UP * lift, 1.0)];
            geom.curve(A, E, 0, 1, e.clone());
            geom.curve(B, G, 0, 1, g.clone());
            let recs = decide(&geom);
            let pair = recs.pair(&geom);
            if x_count(&pair.terms) == 0 {
                continue;
            }
            let out = recs.clusters(&geom, &pair).expect("formed");
            assert_eq!(out.rec.clusters.len(), 1);
            let unclear = out.rec.clusters[0].clear.is_none();
            let z = z_orders(&geom, &out.rec)[0];
            if lift.abs() < 1e-9 && lift != 0.0 {
                assert_eq!(z, None, "{d} {lift}: a near tie is refused");
                near += 1;
                continue;
            }
            let z = z.unwrap_or_else(|| panic!("{d} {lift}: decided"));
            // The moved configuration, `A` by `δ·d`.
            let delta = 1e-7;
            let moved: Vec<Span> = e
                .iter()
                .map(|sp| {
                    sp.map(|p| {
                        let q = DVec3::new(p[0], p[1], p[2]) + d * delta;
                        [q.x, q.y, q.z, p[3]]
                    })
                })
                .collect();
            let at = newton(&moved[0], &g[0], [0.5, 0.0]).expect("a crossing");
            let gap = frame_at(&g[0], at[1])[2] - frame_at(&moved[0], at[0])[2];
            assert_eq!(z == Sign::Pos, gap > 0.0, "{d} {lift}: gap {gap}");
            match (lift == 0.0, unclear) {
                (true, _) => by_orders += 1,
                (false, true) => by_value += 1,
                (false, false) => {}
            }
        }
    }
    assert!(
        by_orders >= 3 && by_value > 0 && near >= 3,
        "{by_orders} by orders, {by_value} by value, {near} near ties"
    );
}

/// A curved `e` and a straight `g` from one vertex (a vertex on a vertex,
/// exactly): their crossing's curved edge–edge key isn't decided at any
/// level (module docs of the decider, "Shared corners").
#[test]
fn a_curved_cluster_at_a_shared_vertex_is_not_decided() {
    let e = vec![[
        cp(lat(0.0, 0.0, 0.0), 1.0),
        cp(lat(1.0, 1.0, 1.0), 1.0),
        cp(lat(2.0, -1.0, -1.0), 1.0),
    ]];
    let mut formed = 0;
    for d in [
        NU,
        -NU,
        DVec3::new(0.25, -0.5, 0.125),
        DVec3::new(-0.25, 0.5, -0.125),
    ] {
        let mut geom = Geom {
            d,
            ..Geom::default()
        };
        geom.curve(A, E, 0, 1, e.clone());
        geom.curve(
            B,
            G,
            0,
            1,
            vec![straight(lat(0.0, 0.0, 0.0), lat(1.0, -2.0, 1.0), 1.0)],
        );
        let recs = decide(&geom);
        let pair = recs.pair(&geom);
        let Ok(out) = recs.clusters(&geom, &pair) else {
            continue;
        };
        for (c, z) in out.rec.clusters.iter().zip(z_orders(&geom, &out.rec)) {
            if c.clear.is_none() {
                assert_eq!(z, None, "{d}: {c:?}");
                formed += 1;
            }
        }
    }
    assert!(formed > 0, "no unclear cluster at the shared vertex");
}

/// A curved edge whose inner knot's shadow lies `10⁻⁴` beside a straight
/// edge's (a sphere's latitude circle against a box edge, from a random
/// placement of the counting's oracle runs): the cell's inner side at
/// the knot can't be certified off at the cell's size, nor at its halves'
/// first splits, so the cell refines until its sides settle, and its
/// clusters sum to the four terms.
#[test]
fn a_knot_beside_the_other_edge_refines_its_cell() {
    let e = vec![
        [
            [
                1.5683476690881548,
                0.7979907065814862,
                -0.7605346356321209,
                0.8535533905932737,
            ],
            [
                1.4463546174240358,
                0.004739113936898006,
                -0.8672753232285304,
                0.6035533905932737,
            ],
            [
                0.9132092291144873,
                0.004739113936897983,
                -0.2579478552769246,
                0.8535533905932737,
            ],
        ],
        [
            [
                0.9132092291144873,
                0.004739113936897983,
                -0.2579478552769246,
                0.8535533905932737,
            ],
            [
                0.38006384080493893,
                0.004739113936898006,
                0.35137961267468115,
                0.6035533905932737,
            ],
            [
                0.502056892469058,
                0.7979907065814862,
                0.4581203002710907,
                0.8535533905932737,
            ],
        ],
    ];
    let g0 = DVec3::new(1.018455089421735, 0.11551080273749709, 1.5170050025337993);
    let g1 = DVec3::new(1.1109126010903216, 1.6619344992686311, 0.21565362879923344);
    let mut geom = Geom {
        d: DVec3::new(
            0.13643047634087474,
            -0.14384127281475897,
            -0.9801512196393126,
        ),
        ..Geom::default()
    };
    geom.curve(A, E, 0, 1, e);
    geom.segment(B, G, (2, g0), (3, g1));
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    let out = recs.clusters(&geom, &pair).expect("formed");
    assert_eq!(out.rec.degree(), x_count(&pair.terms));
    assert!(out.boxes.len() == out.rec.clusters.len());
}

/// Two-span conic chains on a small lattice, `g`'s knot under `e`'s
/// (the knots' shadows one point), weights 1/64 to 64.
fn coinciding_knots(seed: u64) -> Geom {
    let mut rng = Rng::new(seed);
    let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
    let mut pt = |z: f64| lat(int(2), int(2), z);
    let (a, k, b) = (pt(1.0), pt(1.0), pt(1.0));
    let (g0, _, g1) = (pt(-1.0), pt(-1.0), pt(-1.0));
    let h = k - UP * 2.0;
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
    let mut geom = Geom {
        d: [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3],
        ..Geom::default()
    };
    geom.curve(A, E, 0, 1, e);
    geom.curve(B, G, 0, 1, g);
    geom
}

/// Two-span conic chains, `g`'s knot under `e`'s (the knots' shadows
/// one point), weights 1/64 to 64, narrow bands: the boxes at the knots
/// read the side and corner records, so no box within the bands is
/// refused for its degree; the clusters sum to the four terms. Every side
/// record is decided, a held knot where the other chain's shadow leaves
/// it vertically (a shadow cusp on the knot) too. A level whose
/// enclosures stay wider than the bands runs out of boxes, and a long
/// near-tangency of the two shadows takes too many boxes reading records:
/// those are refused, never counted.
#[test]
fn clusters_at_coinciding_knots_read_the_side_records() {
    let (mut formed, mut read, mut trials) = (0, 0, 0);
    for seed in varde_testing::seeds(40, 300) {
        let geom = coinciding_knots(seed);
        let Some(recs) = try_decide(&geom) else {
            continue;
        };
        let pair = recs.pair(&geom);
        trials += 1;
        match recs.clusters(&geom, &pair) {
            Ok(out) => {
                assert_eq!(out.rec.degree(), x_count(&pair.terms), "seed {seed}");
                formed += 1;
                read += usize::from(!recs.sides.borrow().side_table().is_empty());
            }
            Err(XClusterError::Boxes | XClusterError::Sided(_) | XClusterError::Depth(_)) => {}
            Err(e) => panic!("seed {seed}: {e:?}"),
        }
    }
    assert!(
        formed * 50 > trials * 49 && read > 0,
        "{formed} formed of {trials}, {read} reading side records"
    );
}

/// Two straight spans meeting in 3D at a shadow angle of about `2⁻⁵²`
/// (their shadow tangents apart by less than the first level's error):
/// the first level can't certify the cluster transversal, the second
/// does, so the record stores level 1, replays there, and is never
/// formed again at level 0.
#[test]
fn a_record_replays_at_the_level_it_was_formed_at() {
    let mut geom = Geom {
        d: DVec3::new(0.25, -0.5, 0.125),
        ..Geom::default()
    };
    // `2⁻⁵⁰`.
    let tiny = DVec3::new(0.0, 1.0 / 1_125_899_906_842_624.0, 0.0);
    let x = DVec3::X;
    geom.curve(A, E, 0, 1, vec![straight(-x, x, 1.0)]);
    geom.curve(
        B,
        G,
        0,
        1,
        vec![straight(-x - tiny - UP, x + tiny + UP, 1.0)],
    );
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    // One cell, its sides on the square: no side record read.
    let sides = XSides::new();
    let at0 = clusters_at(X, &pair, &sides, 0);
    assert!(
        matches!(&at0, Err(e) if e.escalates()),
        "level 0 doesn't certify the tangents apart: {at0:?}"
    );
    let out = form(X, &pair, &sides).expect("formed");
    assert_eq!(out.rec.level, 1);
    assert_eq!(out.rec.degree(), x_count(&pair.terms));
    assert_eq!(out.rec.clusters.len(), 1);
    assert_eq!(replay(X, &pair, &sides, &out.rec), Ok(()));
    let at_0 = XRec {
        level: 0,
        ..out.rec.clone()
    };
    assert_eq!(
        replay(X, &pair, &sides, &at_0),
        Err(XReplayError::Form(at0.expect_err("escalates")))
    );
    let mut other = out.rec.clone();
    other.clusters[0].degree = -other.clusters[0].degree;
    assert_eq!(replay(X, &pair, &sides, &other), Err(XReplayError::Differs));
}

/// A search reports the work it tested, whatever it finds: two curved
/// edges far apart in shadow have no clusters, and each of their cells'
/// hulls was still tested.
#[test]
fn cells_apart_are_charged() {
    let mut geom = Geom {
        d: DVec3::new(0.25, -0.5, 0.125),
        ..Geom::default()
    };
    let (a, k, b) = (lat(0.0, 0.0, 0.0), lat(2.0, 0.0, 0.0), lat(4.0, 0.0, 0.0));
    let up = lat(0.0, 100.0, 1.0);
    geom.curve(A, E, 0, 1, vec![straight(a, k, 3.0), straight(k, b, 0.5)]);
    geom.curve(
        B,
        G,
        0,
        1,
        vec![straight(a + up, k + up, 1.0), straight(k + up, b + up, 2.0)],
    );
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    let out = recs.clusters(&geom, &pair).expect("formed");
    assert!(out.rec.clusters.is_empty());
    // Two pieces' hulls per cell.
    assert!(out.pieces >= 8, "{} pieces", out.pieces);
}
