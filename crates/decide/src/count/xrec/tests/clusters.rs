//! Clusters of edge pairs: a straight pair's one cluster (its row on
//! exactly parallel shadows, its clear z-order against the moved
//! crossing), curved pairs' clusters summing to the four terms, each
//! clear or within the bands, the same on a second run; pieces far apart
//! in height clear at once, pieces meeting in 3D refining to the bands.

use super::*;
use crate::order::{EdgeKey, EntityOrder, VertKey};
use crate::records::keys::XKey;

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

const X: XKey = XKey {
    e: EdgeId(E),
    g: EdgeId(G),
};

#[test]
fn a_straight_pair_is_one_cluster() {
    let (mut parallel, mut clear, mut crossing) = (0, 0, 0);
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
        let out = clusters(&geom, X, &pair, [1e-6; 2], &like()).expect("a straight pair");
        assert_eq!(out.rec.degree(), x, "seed {seed}");
        assert_eq!(out.rec.clusters.len(), usize::from(x != 0), "seed {seed}");
        let Some(c) = out.rec.clusters.first() else {
            continue;
        };
        crossing += 1;
        let up = |p: DVec3| q(p.dot(UP));
        match moved_params(&geom) {
            None => {
                // Exactly parallel shadows: the crossing is an end's tie.
                parallel += 1;
                let ZRow::PointCurve(p) = c.row else {
                    panic!("seed {seed}: {c:?}");
                };
                assert!(
                    [Point::vert(A, VertId(0)), Point::vert(A, VertId(1))].contains(&p)
                        || [Point::vert(B, VertId(0)), Point::vert(B, VertId(1))].contains(&p),
                    "seed {seed}"
                );
            }
            Some([t, s]) => {
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
        }
    }
    assert!(
        clear > 0 && crossing > clear,
        "{parallel} parallel, {clear} clear of {crossing}"
    );
}

/// Straight edges with exactly parallel (collinear) shadows: moved by a
/// translation they never cross, so no cluster; where the terms do count
/// one (a motion moving the ends apart, here terms made by hand), the
/// cluster reads the point–curve row at the counting end.
#[test]
fn parallel_shadows_read_the_point_curve_row() {
    for d in [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU, -NU] {
        for (g0, g1) in [(1.0, 3.0), (2.0, 4.0), (-1.0, 0.0), (0.5, 1.5)] {
            let mut geom = Geom {
                d,
                ..Geom::default()
            };
            geom.segment(A, E, (0, lat(0.0, 0.0, 0.0)), (1, lat(2.0, 0.0, 1.0)));
            geom.segment(B, G, (0, lat(g0, 0.0, 5.0)), (1, lat(g1, 0.0, 3.0)));
            let recs = decide(&geom);
            let pair = recs.pair(&geom);
            assert_eq!(x_count(&pair.terms), 0);
            let out = clusters(&geom, X, &pair, [1e-6; 2], &like()).expect("formed");
            assert!(out.rec.clusters.is_empty(), "{d} {g0}");
        }
    }
    let mut geom = Geom::default();
    geom.segment(A, E, (0, lat(0.0, 0.0, 0.0)), (1, lat(2.0, 0.0, 1.0)));
    geom.segment(B, G, (0, lat(1.0, 0.0, 5.0)), (1, lat(3.0, 0.0, 3.0)));
    let ray = |k: Option<Sign>| EdgeRays::Line(crate::records::shadow::RayRec { crossing: k });
    let pair = XPair {
        e: geom.curve_of(A, E),
        g: geom.curve_of(B, G),
        terms: XTerms {
            a_g: ray(None),
            b_g: ray(Some(Sign::Pos)),
            g0_e: ray(None),
            g1_e: ray(None),
        },
    };
    let out = clusters(&geom, X, &pair, [1e-6; 2], &like()).expect("formed");
    assert_eq!(out.rec.degree(), 1);
    assert_eq!(
        out.rec.clusters[0].row,
        ZRow::PointCurve(Point::vert(A, VertId(1)))
    );
}

/// The bands of the curved trials: a fraction of the lattice's scale.
const BETA: [f64; 2] = [0.5, 0.25];

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
        let Ok(out) = clusters(&geom, X, &pair, BETA, &like()) else {
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
        let again = clusters(&geom, X, &pair, BETA, &like()).expect("formed once");
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
fn heights_apart_clear_at_once_and_meeting_pieces_refine_to_the_bands() {
    let geom = crossing_pair(3.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    assert_eq!(x_count(&pair.terms), -1);
    let out = clusters(&geom, X, &pair, [1e-3; 2], &like()).expect("formed");
    assert_eq!(out.rec.clusters.len(), 1);
    assert_eq!(out.rec.clusters[0].clear, Some(Sign::Pos));
    assert_eq!(out.boxes[0], XBox::cells([1, 1]).next().expect("a cell"));

    let geom = crossing_pair(-3.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    let out = clusters(&geom, X, &pair, [1e-3; 2], &like()).expect("formed");
    assert_eq!(out.rec.clusters[0].clear, Some(Sign::Neg));

    // The two meet in 3D: the cluster's pieces shrink to the bands and
    // read their record.
    let geom = crossing_pair(0.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    for beta in [1.0, 1e-3] {
        let out = clusters(&geom, X, &pair, [beta; 2], &like()).expect("formed");
        assert_eq!(out.rec.degree(), -1);
        for (c, b) in out.rec.clusters.iter().zip(&out.boxes) {
            if c.clear.is_none() {
                let size = |x: Extent| dy(x.hi()) - dy(x.lo());
                assert!(size(b.t) < 1.0 && size(b.s) < 1.0, "{beta}: {b:?}");
            }
        }
        assert!(out.rec.clusters.iter().any(|c| c.clear.is_none()));
    }
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
    assert!(transversal(&e, &across, &like()));
    assert!(!transversal(&e, &along, &like()));
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

/// Random curved pairs, weights 1/64 to 64, against the sampled oracle:
/// each cluster holds the signed crossings of its box, a clear one all
/// on its side in height, an unclear one exactly one crossing; `s11` is
/// the oracle's sum over crossings with `g` above `e`.
#[test]
fn s11_matches_the_sampled_crossings() {
    let (mut checked, mut unclear, mut crossings) = (0, 0, 0);
    let ws = [1.0 / 64.0, 1.0 / 8.0, 1.0, 8.0, 64.0];
    for seed in varde_testing::seeds(80, 1200) {
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
        let Some(found) = sampled_crossings(&e, &g) else {
            continue;
        };
        geom.curve(A, E, 0, 1, e);
        geom.curve(B, G, 0, 1, g);
        let Some(recs) = try_decide(&geom) else {
            continue;
        };
        let pair = recs.pair(&geom);
        let x = x_count(&pair.terms);
        assert_eq!(found.iter().map(|c| c.sign).sum::<i64>(), x, "seed {seed}");
        for beta in [1e-3, 20.0, 500.0] {
            let Ok(out) = clusters(&geom, X, &pair, [beta; 2], &like()) else {
                continue;
            };
            checked += 1;
            crossings += found.len();
            let inside = |c: &Sampled, b: &XBox| {
                c.cell == b.cell
                    && dy(b.t.lo()) <= c.at[0]
                    && c.at[0] <= dy(b.t.hi())
                    && dy(b.s.lo()) <= c.at[1]
                    && c.at[1] <= dy(b.s.hi())
            };
            for (cl, bx) in out.rec.clusters.iter().zip(&out.boxes) {
                let held: Vec<&Sampled> = found.iter().filter(|c| inside(c, bx)).collect();
                let deg: i64 = held.iter().map(|c| c.sign).sum();
                assert_eq!(deg, cl.degree, "seed {seed}: {bx:?}");
                match cl.clear {
                    Some(z) => {
                        for c in &held {
                            assert_eq!(c.gap > 0.0, z == Sign::Pos, "seed {seed}: {bx:?}");
                        }
                    }
                    None => {
                        unclear += 1;
                        assert_eq!(held.len(), 1, "seed {seed}: {bx:?}");
                    }
                }
            }
            let want: i64 = found.iter().filter(|c| c.gap > 0.0).map(|c| c.sign).sum();
            let got = out
                .rec
                .s11(|c, _| {
                    let bx = &out.boxes[c.0 as usize];
                    let c = found.iter().find(|c| inside(c, bx)).expect("one crossing");
                    Ok::<_, ()>(if c.gap > 0.0 { Sign::Pos } else { Sign::Neg })
                })
                .expect("read");
            // Within wide bands a pair of crossings of opposite signs and
            // z-orders may share one box of degree 0, which carries nothing.
            if beta < 100.0 {
                assert_eq!(got, want, "seed {seed} beta {beta}: {out:?}");
            }
        }
    }
    assert!(
        checked > 30 && crossings > 30,
        "{checked} checked, {crossings} crossings, {unclear} unclear"
    );
}

/// `e`'s shadow a conic (middle weighted 1/64 to 64) touching `g`'s line
/// at its middle, at the
/// same height, `g` ending there: the motion leaves one crossing on `g`
/// or none. A box of non-zero degree around the touching point never
/// certifies its tangents apart nor its z-order: it escalates, never a
/// cluster; with no crossing nothing is formed.
#[test]
fn a_shadow_tangency_with_a_crossing_escalates() {
    let (mut escalated, mut empty) = (0, 0);
    for d in [
        NU,
        -NU,
        DVec3::new(0.25, -0.5, 0.125),
        DVec3::new(0.25, 0.5, 0.125),
    ] {
        for wm in [1.0 / 64.0, 1.0, 64.0] {
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
            let x = x_count(&pair.terms);
            for beta in [1e-3, 1.0, 1e3] {
                match clusters(&geom, X, &pair, [beta; 2], &like()) {
                    Err(XClusterError::Degenerate(_) | XClusterError::Depth(_)) => {
                        assert_ne!(x, 0, "{d} {wm} {beta}");
                        escalated += 1;
                    }
                    Err(XClusterError::Degree(..)) => {}
                    Err(e) => panic!("{d} {wm} {beta}: {e:?}"),
                    Ok(out) => {
                        assert_eq!(x, 0, "{d} {wm} {beta}: {out:?}");
                        assert!(out.rec.clusters.is_empty(), "{d} {wm} {beta}");
                        empty += 1;
                    }
                }
            }
        }
    }
    assert!(
        escalated > 0 && empty > 0,
        "{escalated} escalated, {empty} empty"
    );
}

/// `e` a conic dipping under `g`'s straight line near its apex, rising in
/// height across it: two crossings of opposite signs and z-orders, ever
/// closer. Narrow bands split them into two clusters, `s11` the one with
/// `g` above; in one band (a box of degree 0) they carry nothing.
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
        let out = clusters(&geom, X, &pair, [1e-6; 2], &like()).expect("formed");
        assert_eq!(out.rec.clusters.len(), 2, "{k}");
        let s11 = out.rec.s11(|_, _| Err(())).expect("both clear");
        assert_eq!(s11, want, "{k}");
        let out = clusters(&geom, X, &pair, [1e6; 2], &like()).expect("formed");
        assert!(out.rec.clusters.is_empty(), "{k}");
    }
}
