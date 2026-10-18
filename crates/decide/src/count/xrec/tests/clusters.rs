//! Clusters of edge pairs: a straight pair's one cluster (its row on
//! exactly parallel shadows, its clear z-order against the moved
//! crossing), curved pairs' clusters summing to the four terms, each
//! clear or within the bands, the same on a second run; pieces far apart
//! in height clear at once, pieces meeting in 3D refining to the bands.

use super::*;
use crate::order::{EdgeKey, EntityOrder, VertKey};

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
        let out = recs
            .clusters(&geom, &pair, [1e-6; 2])
            .expect("a straight pair");
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
            let out = recs.clusters(&geom, &pair, [1e-6; 2]).expect("formed");
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
        shared: false,
    };
    // Two straight edges' one cell has only outer sides: no side record.
    let out = clusters_at(&geom, X, &pair, [1e-6; 2], &XSides::new(), 0).expect("formed");
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
        let Ok(out) = recs.clusters(&geom, &pair, BETA) else {
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
        let again = recs.clusters(&geom, &pair, BETA).expect("formed once");
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
    let out = recs.clusters(&geom, &pair, [1e-3; 2]).expect("formed");
    assert_eq!(out.rec.clusters.len(), 1);
    assert_eq!(out.rec.clusters[0].clear, Some(Sign::Pos));
    assert_eq!(out.boxes[0], XBox::cells([1, 1]).next().expect("a cell"));

    let geom = crossing_pair(-3.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    let out = recs.clusters(&geom, &pair, [1e-3; 2]).expect("formed");
    assert_eq!(out.rec.clusters[0].clear, Some(Sign::Neg));

    // The two meet in 3D: the cluster's pieces shrink to the bands and
    // read their record.
    let geom = crossing_pair(0.0);
    let recs = decide(&geom);
    let pair = recs.pair(&geom);
    for beta in [1.0, 1e-3] {
        let out = recs.clusters(&geom, &pair, [beta; 2]).expect("formed");
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
            let Ok(out) = recs.clusters(&geom, &pair, [beta; 2]) else {
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
                match recs.clusters(&geom, &pair, [beta; 2]) {
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
        let out = recs.clusters(&geom, &pair, [1e-6; 2]).expect("formed");
        assert_eq!(out.rec.clusters.len(), 2, "{k}");
        let s11 = out.rec.s11(|_, _| Err(())).expect("both clear");
        assert_eq!(s11, want, "{k}");
        let out = recs.clusters(&geom, &pair, [1e6; 2]).expect("formed");
        assert!(out.rec.clusters.is_empty(), "{k}");
    }
}

/// The records and feet a shared-curve pair's clusters read.
struct SharedReads<'a> {
    geom: &'a Geom,
    feet: BTreeMap<(Point, EdgeId), crate::records::feet::Feet>,
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

impl crate::records::height::HeightReads for SharedReads<'_> {
    fn vv(&self, _: Point, _: VertId) -> Option<crate::records::shadow::VvRec> {
        None
    }

    fn feet(&self, q: Point, g: EdgeId) -> Option<&crate::records::feet::Feet> {
        self.feet.get(&(q, g))
    }
}

/// Straight edges on one line, overlapping or not, ends on each other
/// included: never searched; the records' ties give clusters summing to
/// the four terms, which are the moved crossing, each at an end lying on
/// the other edge and reading the point–curve row there.
#[test]
fn edges_on_one_line_cluster_at_their_ends_ties() {
    use crate::records::feet::{EdgeCurve, decide_feet};
    use crate::records::keys::RayKey;
    use crate::records::xrec::shared_clusters;
    let mut checked = 0;
    let mut tied = 0;
    let mut runs = 0;
    for seed in varde_testing::seeds(200, 4000) {
        let mut rng = Rng::new(seed);
        let mut int = |n: u64| (rng.next_u64() % (2 * n + 1)) as f64 - n as f64;
        let dir = lat(int(2), int(2), int(3));
        let base = lat(int(2), int(2), int(2));
        let t = [int(3), int(3), int(3), int(3)];
        if t[0] == t[1] || t[2] == t[3] || dir.dot(R_HAT) == 0.0 && dir.dot(NU) == 0.0 {
            continue;
        }
        runs += 1;
        let at = |k: usize| base + dir * t[k];
        let mut geom = Geom {
            d: [DVec3::new(0.25, -0.5, 0.125), R_HAT, NU][seed as usize % 3],
            ..Geom::default()
        };
        geom.segment(A, E, (0, at(0)), (1, at(1)));
        geom.segment(B, G, (0, at(2)), (1, at(3)));
        let recs = decide(&geom);
        let mut pair = recs.pair(&geom);
        pair.shared = true;
        assert_eq!(
            recs.clusters(&geom, &pair, [1e-6; 2]),
            Err(XClusterError::Shared)
        );
        let mut feet = BTreeMap::new();
        for (op, own, other) in [(A, E, G), (B, G, E)] {
            let [p, q] = geom.edges[&(op.other(), EdgeId(other))]
                .0
                .map(|v| geom.pos[&Point::vert(op.other(), v)]);
            for v in geom.edges[&(op, EdgeId(own))].0 {
                let pt = Point::vert(op, v);
                let key = RayKey {
                    q: pt,
                    g: EdgeId(other),
                };
                let (f, _) = decide_feet(key, geom.pos[&pt], EdgeCurve::Line([p, q]), 1e-3);
                feet.insert((pt, EdgeId(other)), f.expect("feet"));
            }
        }
        let reads = SharedReads { geom: &geom, feet };
        let rec = shared_clusters(&reads, X, &pair).expect("formed");
        let x = x_count(&pair.terms);
        assert_eq!(rec.degree(), x, "seed {seed}");
        if let Some(want) = moved_crossing(&geom) {
            assert_eq!(x, want, "seed {seed}");
            checked += 1;
        }
        for cl in &rec.clusters {
            let ZRow::PointCurve(p) = cl.row else {
                panic!("seed {seed}: {cl:?}");
            };
            let (own, other) = if p.operand == A { (E, G) } else { (G, E) };
            assert!(
                reads.feet[&(p, EdgeId(other))].feet.len() == 1,
                "seed {seed}: {p:?} on {own}"
            );
            tied += 1;
        }
    }
    // A run of a few seeds may hold no tie.
    assert!(runs < 20 || (checked > 0 && tied > 0), "{checked} {tied}");
}

/// Edges on one line under a direction field varying along it (linear,
/// so straight edges stay straight): the moved shadows cross where the
/// field's normal part changes sign, between the ends. The four terms
/// still count that crossing; the clusters, formed from the ends' ties
/// alone, sum to it or are refused, never a wrong total.
#[test]
fn a_varying_field_on_one_line_keeps_the_count() {
    use crate::records::feet::{EdgeCurve, decide_feet};
    use crate::records::keys::RayKey;
    use crate::records::xrec::shared_clusters;
    let mut crossed = 0;
    for dir in [lat(1.0, 0.0, 0.0), lat(1.0, 2.0, 0.0), lat(0.0, 1.0, 1.0)] {
        for k in [1.0, -1.0] {
            let at = |t: f64| dir * t;
            let mut geom = Geom {
                field: dir * k,
                ..Geom::default()
            };
            geom.segment(A, E, (0, at(-1.0)), (1, at(2.0)));
            geom.segment(B, G, (0, at(-3.0)), (1, at(3.0)));
            let recs = decide(&geom);
            let mut pair = recs.pair(&geom);
            pair.shared = true;
            let mut feet = BTreeMap::new();
            for (op, own, other) in [(A, E, G), (B, G, E)] {
                let [p, q] = geom.edges[&(op.other(), EdgeId(other))]
                    .0
                    .map(|v| geom.pos[&Point::vert(op.other(), v)]);
                for v in geom.edges[&(op, EdgeId(own))].0 {
                    let pt = Point::vert(op, v);
                    let key = RayKey {
                        q: pt,
                        g: EdgeId(other),
                    };
                    let (f, _) = decide_feet(key, geom.pos[&pt], EdgeCurve::Line([p, q]), 1e-3);
                    feet.insert((pt, EdgeId(other)), f.expect("feet"));
                }
            }
            let reads = SharedReads { geom: &geom, feet };
            let x = x_count(&pair.terms);
            assert_eq!(Some(x), moved_crossing(&geom), "{dir} {k}");
            crossed += i32::from(x != 0);
            if let Ok(rec) = shared_clusters(&reads, X, &pair) {
                assert_eq!(rec.degree(), x, "{dir} {k}");
            }
        }
    }
    assert!(crossed > 0);
}

/// Arcs of one circle in the shadow plane, three spans from a random
/// angle, each span's weights scaled by 1/64 to 64 (the same curve), the
/// knots shared bit for bit.
fn circle_chain(rng: &mut Rng) -> Vec<Span> {
    use crate::num::trig::{cos, sin};
    let mut f = || (rng.next_u64() % 1000) as f64 / 1000.0;
    let mut ang = vec![f() * 6.0];
    for _ in 0..3 {
        let l = ang[ang.len() - 1];
        ang.push(l + 0.3 + f());
    }
    let pt = |t: f64| lat(cos(t), sin(t), 0.0);
    let ws = [1.0 / 64.0, 0.5, 1.0, 3.0, 64.0];
    let mut out: Vec<Span> = Vec::new();
    for i in 0..3 {
        let (t0, t1) = (ang[i], ang[i + 1]);
        let h = (t1 - t0) / 2.0;
        let m = lat(cos(t0 + h) / cos(h), sin(t0 + h) / cos(h), 0.0);
        let c = ws[(rng.next_u64() % 5) as usize];
        let mut s = [cp(pt(t0), c), cp(m, cos(h) * c), cp(pt(t1), c)];
        if let Some(prev) = out.last() {
            let p = prev[2];
            s[0] = [p[0], p[1], p[2], c];
        }
        out.push(s);
    }
    out
}

/// Curved edges on one circle (one ending where the other starts, equal,
/// overlapping, one inside the other, apart), either stored direction,
/// weights 1/64 to 64, under translations: never searched; clusters,
/// where formed, sum to the four terms and sit at ends on the other
/// edge; edges meeting only at a common end are always formed.
#[test]
fn edges_on_one_circle_cluster_at_their_ends_ties() {
    use crate::records::feet::{EdgeCurve, decide_feet};
    use crate::records::keys::RayKey;
    use crate::records::xrec::{SharedError, shared_clusters};
    let mut formed = 0;
    let mut tied = 0;
    for seed in varde_testing::seeds(250, 2000) {
        let mut rng = Rng::new(seed);
        let arcs = circle_chain(&mut rng);
        let cfgs = [
            ("abut", (0, 1), (1, 2)),
            ("same", (0, 1), (0, 1)),
            ("overlap", (0, 2), (1, 3)),
            ("inside", (0, 3), (1, 2)),
            ("apart", (0, 1), (2, 3)),
        ];
        let (name, ei, gi) = cfgs[(seed % 5) as usize];
        let d = [
            DVec3::new(0.25, -0.5, 0.125),
            R_HAT,
            NU,
            -R_HAT,
            DVec3::new(-0.3, 0.7, 0.1),
        ][(seed / 5 % 5) as usize];
        let e: Vec<Span> = arcs[ei.0..ei.1].to_vec();
        let g: Vec<Span> = arcs[gi.0..gi.1].to_vec();
        let g = if seed / 25 % 2 == 1 { reversed(&g) } else { g };
        let mut geom = Geom {
            d,
            ..Geom::default()
        };
        geom.curve(A, E, 0, 1, e.clone());
        geom.curve(B, G, 2, 3, g.clone());
        let Some(recs) = try_decide(&geom) else {
            continue;
        };
        let mut pair = recs.pair(&geom);
        pair.shared = true;
        assert_eq!(
            recs.clusters(&geom, &pair, [1e-6; 2]),
            Err(XClusterError::Shared)
        );
        let mut feet = BTreeMap::new();
        for (op, own, other, spans) in [(A, E, G, &g), (B, G, E, &e)] {
            for v in geom.edges[&(op, EdgeId(own))].0 {
                let pt = Point::vert(op, v);
                let key = RayKey {
                    q: pt,
                    g: EdgeId(other),
                };
                let curve = EdgeCurve::Spans {
                    spans,
                    closed: false,
                };
                let (f, _) = decide_feet(key, geom.pos[&pt], curve, 1e-3);
                feet.insert((pt, EdgeId(other)), f.expect("feet"));
            }
        }
        let reads = SharedReads { geom: &geom, feet };
        let x = x_count(&pair.terms);
        match shared_clusters(&reads, X, &pair) {
            Ok(rec) => {
                assert_eq!(rec.degree(), x, "seed {seed} {name}");
                for cl in &rec.clusters {
                    let ZRow::PointCurve(p) = cl.row else {
                        panic!("seed {seed}: {cl:?}");
                    };
                    let other = if p.operand == A { G } else { E };
                    assert!(!reads.feet[&(p, EdgeId(other))].feet.is_empty());
                    tied += 1;
                }
                formed += 1;
            }
            // The moved arcs cross where the shadow's tangent is along
            // the motion, away from the ends.
            Err(SharedError::Crossed(_)) => assert!(name != "abut", "seed {seed}"),
            Err(err) => panic!("seed {seed} {name}: {err:?}"),
        }
    }
    assert!(formed > 0 && tied > 0, "{formed} {tied}");
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
    let out = recs.clusters(&geom, &pair, [1e-6; 2]).expect("formed");
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
/// refused for its degree; the clusters sum to the four terms. A held
/// knot where the other chain's shadow leaves it vertically (a shadow
/// cusp on the knot) has no side record at any level, a level whose
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
        match recs.clusters(&geom, &pair, [1e-6; 2]) {
            Ok(out) => {
                assert_eq!(out.rec.degree(), x_count(&pair.terms), "seed {seed}");
                formed += 1;
                read += usize::from(!recs.sides.borrow().side_table().is_empty());
            }
            Err(XClusterError::Need(_) | XClusterError::Boxes | XClusterError::Sided(_)) => {}
            Err(e) => panic!("seed {seed}: {e:?}"),
        }
    }
    assert!(
        formed * 5 > trials * 4 && read > 0,
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
    let beta = [1e3; 2];
    // One cell, its sides on the square: no side record read.
    let sides = XSides::new();
    let whole = XBox::cells([1, 1]).next().expect("a cell");
    assert_eq!(
        clusters_at(&geom, X, &pair, beta, &sides, 0),
        Err(XClusterError::Degenerate(whole))
    );
    let out = form(&geom, X, &pair, beta, &sides).expect("formed");
    assert_eq!(out.rec.level, 1);
    assert_eq!(out.rec.degree(), x_count(&pair.terms));
    assert_eq!(out.rec.clusters.len(), 1);
    assert_eq!(replay(&geom, X, &pair, beta, &sides, &out.rec), Ok(()));
    let at_0 = XRec {
        level: 0,
        ..out.rec.clone()
    };
    assert_eq!(
        replay(&geom, X, &pair, beta, &sides, &at_0),
        Err(XReplayError::Form(XClusterError::Degenerate(whole)))
    );
    let mut other = out.rec.clone();
    other.clusters[0].degree = -other.clusters[0].degree;
    assert_eq!(
        replay(&geom, X, &pair, beta, &sides, &other),
        Err(XReplayError::Differs)
    );
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
    let out = recs.clusters(&geom, &pair, [1e-6; 2]).expect("formed");
    assert!(out.rec.clusters.is_empty());
    // Two pieces' hulls per cell.
    assert!(out.pieces >= 8, "{} pieces", out.pieces);
}
