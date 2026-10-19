//! The whole-edge ray records and the feet of the same keys: an edge
//! whose shadow passes a point twice (a self-crossing shadow, its two
//! passages at different heights), so the point has two feet on it or
//! one, by its height and the near radius; closed chains of spans with
//! end and knot weights uneven, each crossing at a knot in the record
//! once; every crossing as the moved configuration's, the feet against
//! dense samples of the distance, and each crossing well within the near
//! radius in a foot, each well beyond it in none; each record decided
//! once, replayed alike; chains read the other way negate; the same
//! records on any threads, nothing of a batch stored when its pass runs
//! out; near radii of 0 and beyond the edge; a circle seen nearly
//! edge-on.

use super::*;
use crate::ids::NONE;
use crate::records::feet::{Feet, Foot, NearFeet};
use crate::records::near::OpBands;

/// A point of the shadow frame with about equal lengths along its axes:
/// `x` along `r̂`, `y` along `ν`, `z` along `UP`, each unit about 16.
fn iso(x: f64, y: f64, z: f64) -> DVec3 {
    R_HAT * x + NU * (y / 32.0) + UP * (z / 2.0)
}

/// The span's point at `t`, in `f64`, by de Casteljau on the homogeneous
/// points.
fn eval(s: &Span, t: f64) -> DVec3 {
    let mut h: Vec<[f64; 4]> = s
        .iter()
        .map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
        .collect();
    while h.len() > 1 {
        h = h
            .windows(2)
            .map(|w| std::array::from_fn(|k| (1.0 - t) * w[0][k] + t * w[1][k]))
            .collect();
    }
    let p = h[0];
    DVec3::new(p[0] / p[3], p[1] / p[3], p[2] / p[3])
}

/// `(span, t)` ordered along the chain.
fn before(a: (u32, f64), b: (u32, f64)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1)
}

/// Whether the foot holds `(span, t)`: with its ends' outer bounds
/// (`outer`), or their inner ones.
fn holds(f: &Foot, at: (u32, f64), outer: bool) -> bool {
    let (lo, hi) = if outer {
        ((f.lo.span, f.lo.t[0]), (f.hi.span, f.hi.t[1]))
    } else {
        ((f.lo.span, f.lo.t[1]), (f.hi.span, f.hi.t[0]))
    };
    if f.wraps {
        before(lo, at) || before(at, hi)
    } else {
        before(lo, at) && before(at, hi)
    }
}

/// The feet holding `(span, t)` with their outer bounds.
fn feet_at(feet: &Feet, at: (u32, f64)) -> Vec<usize> {
    (0..feet.feet.len())
        .filter(|&k| holds(&feet.feet[k], at, true))
        .collect()
}

/// Margin of the distance checks against `r`.
fn margin(r: f64) -> f64 {
    1e-7 * (1.0 + r)
}

/// The feet against dense samples: every sample well inside the sphere
/// of radius `r` about `q` in some foot, every one well outside in none.
fn check_feet(spans: &[Span], q: DVec3, r: f64, feet: &Feet, what: &str) {
    const N: u32 = 256;
    for (i, s) in spans.iter().enumerate() {
        let i = i as u32;
        for j in 0..=N {
            let t = f64::from(j) / f64::from(N);
            let d = (eval(s, t) - q).length() - r;
            if d < -margin(r) {
                assert!(
                    feet.feet.iter().any(|f| holds(f, (i, t), true)),
                    "{what}: ({i}, {t}) inside by {d} in no foot: {feet:?}"
                );
            } else if d > margin(r) {
                assert!(
                    !feet.feet.iter().any(|f| holds(f, (i, t), false)),
                    "{what}: ({i}, {t}) outside by {d} in a foot: {feet:?}"
                );
            }
        }
    }
}

/// The whole-edge record of `q` against `e`: its span records' crossings
/// in order, its count the moved configuration's; each crossing well
/// within `r` of `q` in a foot, each well beyond in none. Gives the feet
/// holding each crossing.
fn check_chain(
    rays: &CurvedRays,
    feet: &Feet,
    g: &Geom,
    q: Point,
    e: EdgeId,
    r: f64,
    what: &str,
) -> Vec<(u32, Sign, Vec<usize>)> {
    let op = q.operand.other();
    let spans = &g.edges[&(op, e)].1;
    let chain = rays.chain(q, e).expect("stored");
    let mut want = Vec::new();
    let mut moved = 0;
    let qm = g.moved(q.operand, g.pos[&q]);
    for i in 0..spans.len() {
        let rec = rays.span(q, e, i as u32).expect("stored");
        want.extend(rec.crossings.iter().map(|&crossing| ChainCrossing {
            span: i as u32,
            crossing,
        }));
        moved += oracle(g, op, e, i, &qm, &ray_dir(q.operand))
            .iter()
            .map(|&x| i32::from(x))
            .sum::<i32>();
    }
    assert_eq!(
        &chain.crossings[..],
        &want[..],
        "{what}: {q:?} against {e:?}"
    );
    assert_eq!(chain.count(), moved, "{what}: {q:?} against {e:?}");
    let at = g.pos[&q];
    let mut out = Vec::new();
    for c in &chain.crossings {
        let s = &spans[c.span as usize];
        let [lo, hi] = c.crossing.t;
        let d = [lo, hi].map(|t| (eval(s, t) - at).length() - r);
        let mid = lo + (hi - lo) * 0.5;
        let held = feet_at(feet, (c.span, mid));
        if d[0] < -margin(r) && d[1] < -margin(r) {
            assert!(!held.is_empty(), "{what}: {q:?}: {c:?} near, in no foot");
        }
        if d[0] > margin(r) && d[1] > margin(r) {
            let inner = feet.feet.iter().any(|f| holds(f, (c.span, mid), false));
            assert!(!inner, "{what}: {q:?}: {c:?} far, in a foot");
        }
        out.push((c.span, c.crossing.sign, held));
    }
    out
}

/// Fills `q`'s whole-edge records and feet against every edge of the
/// other operand, each in one fill.
fn fill_edges(
    rays: &mut CurvedRays,
    shadows: &mut Shadows,
    feet: &mut NearFeet,
    g: &Geom,
    qs: &[Point],
) -> Seq {
    let keys: Vec<RayKey> = qs
        .iter()
        .flat_map(|&q| {
            g.edges
                .keys()
                .filter(move |(op, _)| *op == q.operand.other())
                .map(move |&(_, e)| RayKey { q, g: e })
        })
        .collect();
    let mut seq = Seq::default();
    rays.fill_chains(keys.clone(), shadows, g, band(), &mut seq)
        .expect("decided");
    feet.fill(keys, g, &mut seq).expect("decided");
    seq
}

/// The near radius of an edge of tolerance `tol` at the geometry's
/// magnitude.
fn near_radius(g: &Geom, tol: f64) -> f64 {
    let most = g
        .pos
        .values()
        .copied()
        .chain(
            g.edges
                .values()
                .flat_map(|(_, s)| s.iter().flatten().map(point_of)),
        )
        .flat_map(|p| [p.x, p.y, p.z])
        .fold(0.0, |m: f64, x| if x.abs() > m { x.abs() } else { m });
    OpBands::new(most)
        .and_then(|b| b.near_radius(tol))
        .expect("a radius")
}

/// An edge whose shadow crosses itself at the column of the origin: a
/// passage along the diagonal arching up to about `h` there, a loop out
/// along `r̂` and back, and a passage along the other diagonal arching
/// down to about `−h` there (heights in [`iso`] units), the passages'
/// middle weights `w`, the knots weighing 3 against ⅓ and [`SPLIT`]
/// against 2. Its shadow's two passages pass near a point on the origin's
/// column at once; in space they are about `2h·8 ≈ 1` apart. Points about
/// the column at heights 0 and `+h`: at height 0 the two passages are two
/// feet within a radius of 1, both crossings by a ray along the column's
/// `r̂` line read different feet (one height record each); at `+h` one
/// foot within a radius of ¾; within ¼, none.
#[test]
fn a_shadow_passing_a_point_twice_gives_two_feet() {
    const H: f64 = 1.0 / 32.0;
    for seed in varde_testing::seeds(24, 256) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let op = if seed % 2 == 0 { A } else { B };
        let w = [1.0, SPLIT, 2.0][(seed / 2) as usize % 3];
        let mut g = Geom::with(DIRECTIONS[(seed / 6) as usize % 5], sigmas(seed / 30));
        let spans: [Span; 3] = [
            [
                cp(iso(-1.0, -1.0, 0.0), 3.0),
                cp(iso(0.0, 0.0, 2.0 * H), 3.0 * w),
                cp(iso(1.0, 1.0, 0.0), 3.0),
            ],
            [
                cp(iso(1.0, 1.0, 0.0), 1.0 / 3.0),
                cp(iso(3.0, 0.0, 0.0), 1.0),
                cp(iso(1.0, -1.0, 0.0), SPLIT),
            ],
            [
                cp(iso(1.0, -1.0, 0.0), 2.0),
                cp(iso(0.0, 0.0, -2.0 * H), 2.0 * w),
                cp(iso(-1.0, 1.0, 0.0), 2.0),
            ],
        ];
        let e = g.edge(op, 0, 0, 1, &spans);
        // The first passage's height at the origin's column, at `t = ½`,
        // rounded to a dyadic: points there are exactly on the shadows
        // they are placed on, so their ties are the moved configuration's.
        let top = (2.0 * H * w / (1.0 + w) * 1024.0).round() / 1024.0;
        let off =
            |rng: &mut varde_testkit::rng::Rng| (rng.next_u64() % 5) as f64 / 256.0 - 1.0 / 128.0;
        let mut qs = Vec::new();
        for i in 0..8u32 {
            let (dx, dy) = if i == 0 {
                (0.0, 0.0)
            } else {
                (off(&mut rng), off(&mut rng))
            };
            let z = if i < 4 { 0.0 } else { top };
            qs.push(g.query(op.other(), 100 + i, iso(dx, dy, z)));
        }
        // The point before the column along its ray: both passages cross
        // the ray there, beside each other in shadow.
        let toward = match op.other() {
            A => 1.0,
            B => -1.0,
        };
        qs.push(g.query(op.other(), 200, iso(toward / 256.0, 0.0, 0.0)));
        // At height `top` the other passage is `2·top` below, between
        // 0.92 and 1.34 by `w`: within a radius of 1 or not.
        for (r, n_at_0, n_at_top) in [(1.0, 2, None), (0.75, 2, Some(1)), (0.25, 0, Some(1))] {
            g.r_g = r;
            let (mut rays, mut shadows, mut feet) =
                (CurvedRays::new(), Shadows::new(), NearFeet::new());
            fill_edges(&mut rays, &mut shadows, &mut feet, &g, &qs);
            let what = format!("seed {seed}, r {r}");
            for (k, &q) in qs.iter().enumerate() {
                check(&rays, &g, q, true, &what);
                let f = feet.get(q, e).expect("stored");
                check_feet(&spans, g.pos[&q], r, f, &what);
                let held = check_chain(&rays, f, &g, q, e, r, &what);
                let want = if k < 4 || k == 8 {
                    Some(n_at_0)
                } else {
                    n_at_top
                };
                let want = want.unwrap_or(f.feet.len());
                assert_eq!(f.feet.len(), want, "{what}: {q:?}: {f:?}");
                for foot in &f.feet {
                    assert!(!foot.hull && !foot.wraps, "{what}: {foot:?}");
                    assert!(
                        foot.end_vertices(g.ends(op, e), crate::order::Along::Stored)
                            .is_empty()
                    );
                }
                if want == 2 {
                    // A foot on each passage.
                    assert_eq!((f.feet[0].lo.span, f.feet[0].hi.span), (0, 0), "{what}");
                    assert_eq!((f.feet[1].lo.span, f.feet[1].hi.span), (2, 2), "{what}");
                }
                if k == 8 && want == 2 {
                    // Both passages cross the ray beside the column, each
                    // in its own foot, with the same sign.
                    let near: Vec<_> = held.iter().filter(|(s, _, _)| *s != 1).collect();
                    assert_eq!(near.len(), 2, "{what}: {held:?}");
                    assert_eq!(near[0].2, vec![0], "{what}: {held:?}");
                    assert_eq!(near[1].2, vec![1], "{what}: {held:?}");
                    assert_eq!(near[0].1, near[1].1, "{what}: {held:?}");
                }
            }
            rays.replay(&shadows, &g, band()).expect("replays");
            feet.replay(&g).expect("replays");
        }
    }
}

/// Closed chains of spans (a circle in one closed edge, or in two edges),
/// end and knot weights uneven (3, ⅓, ≈ 0.854, 1, differing at a knot
/// between its two spans), against points on the knots' and vertices'
/// columns and `r̂` lines, before and past them, inside and outside: each
/// whole-edge record is its spans' crossings, its count the moved
/// configuration's, the closed chain's its winding number about the
/// point (each crossing at a knot or vertex counted once); the feet hold
/// the knots near the point across them, and every near crossing.
#[test]
fn whole_edges_count_each_knot_once() {
    for seed in varde_testing::seeds(24, 256) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let op = if seed % 3 == 0 { A } else { B };
        let c = lat(
            (rng.range(-1.0, 1.0) * 16.0).round() / 16.0,
            0.0,
            (rng.range(-1.0, 1.0) * 16.0).round() / 16.0,
        );
        let rad = 1.0 + rng.range(0.0, 2.0);
        let spans: Vec<Span> = circle(c, rad, 4, rng.range(0.0, 1.0))
            .iter()
            .map(|s| {
                let mut pick = || UNEVEN[(rng.next_u64() % 4) as usize];
                reweighted(s, pick(), pick())
            })
            .collect();
        let two = seed % 2 == 1;
        let mut g = Geom::with(DIRECTIONS[(seed / 2) as usize % 5], sigmas(seed / 3));
        let edges = if two {
            vec![
                g.edge(op, 0, 0, 1, &spans[0..2]),
                g.edge(op, 1, 1, 0, &spans[2..4]),
            ]
        } else {
            vec![g.edge(op, 0, 0, 0, &spans)]
        };
        let mut qs = Vec::new();
        let mut id = 100;
        for s in &spans {
            let j = point_of(&s[0]);
            for (l, m) in [(0.5, 0.0), (-0.5, 1.0), (0.0, 0.0), (0.0, -2.0), (3.0, 1.0)] {
                id += 1;
                qs.push(g.query(op.other(), id, j + R_HAT * l + UP * m));
            }
        }
        qs.push(g.query(op.other(), 1, c));
        qs.push(g.query(op.other(), 2, c + lat(rad * 3.0, 0.0, 0.0)));
        // About the distance of the points beside a joint: feet across
        // the knots there.
        let r = near_radius(&g, 4.0);
        g.r_g = r;
        let (mut rays, mut shadows, mut feet) =
            (CurvedRays::new(), Shadows::new(), NearFeet::new());
        fill_edges(&mut rays, &mut shadows, &mut feet, &g, &qs);
        let what = format!("seed {seed}");
        for &q in &qs {
            for &e in &edges {
                let f = feet.get(q, e).expect("stored");
                check_feet(&g.edges[&(op, e)].1, g.pos[&q], r, f, &what);
                check_chain(&rays, f, &g, q, e, r, &what);
            }
            // The second edge, stored from its lower id, runs against the
            // circle.
            let count = |e: EdgeId| rays.chain(q, e).expect("stored").count();
            let winding = match edges[..] {
                [e0, e1] => count(e0) - count(e1),
                _ => count(edges[0]),
            };
            if (g.pos[&q] - c).length() < 0.5 {
                assert_eq!(winding.abs(), 1, "{what}: {q:?}");
            }
        }
        rays.replay(&shadows, &g, band()).expect("replays");
        feet.replay(&g).expect("replays");
    }
}

/// Each whole-edge record and each point's feet are decided once: a
/// second fill decides nothing and charges nothing; the whole-edge record
/// reads each span record once; a near radius a caller must not give
/// fails its pass naming the ray record, storing nothing.
#[test]
fn whole_edges_and_feet_are_decided_once() {
    let mut g = Geom::with(DIRECTIONS[0], Sign::Pos);
    let spans = circle(lat(0.0, 0.0, 0.0), 1.0, 4, 0.25);
    let e = g.edge(B, 0, 0, 0, &spans);
    let q = g.query(A, 9, lat(0.25, 0.0, 0.5));
    g.r_g = 20.0;
    let (mut rays, mut shadows, mut feet) = (CurvedRays::new(), Shadows::new(), NearFeet::new());
    let seq = fill_edges(&mut rays, &mut shadows, &mut feet, &g, &[q]);
    // One vertex record, three knot records, four spans, the whole edge,
    // the feet.
    assert_eq!(seq.decided, 1 + 3 + 4 + 1 + 1);
    assert_eq!(rays.chain(q, e).expect("stored").count().abs(), 1);
    assert!(feet.get(q, e).is_some());
    #[cfg(debug_assertions)]
    for span in 0..4 {
        let key = SpanKey { q, g: e, span };
        assert_eq!(rays.span_table().reads(&key), Some(1), "{key:?}");
    }
    let again = fill_edges(&mut rays, &mut shadows, &mut feet, &g, &[q]);
    assert_eq!(again.decided, 0);
    assert_eq!(again.cost, Cost::default());
    rays.replay(&shadows, &g, band()).expect("replays");
    feet.replay(&g).expect("replays");

    // The span records of a whole edge filled first are not decided again.
    let p = g.query(A, 10, lat(-0.25, 0.0, 0.5));
    let keys: Vec<SpanKey> = (0..4).map(|span| SpanKey { q: p, g: e, span }).collect();
    let mut seq = Seq::default();
    rays.fill(keys, &mut shadows, &g, band(), &mut seq)
        .expect("decided");
    let mut chain = Seq::default();
    rays.fill_chains(
        [RayKey { q: p, g: e }],
        &mut shadows,
        &g,
        band(),
        &mut chain,
    )
    .expect("decided");
    assert_eq!(chain.decided, 1);

    g.r_g = f64::NAN;
    let mut bad = NearFeet::new();
    let key = RayKey { q: p, g: e };
    let got = bad.fill([key], &g, &mut Seq::default());
    let want = Fail::Internal {
        key: TieKey {
            row: Row::VertexCurveShadow,
            record: RecordKey::Ray(key),
        },
    };
    assert_eq!(got, Err(ShadowError::Pass(want)));
    assert!(bad.get(p, e).is_none());
}

/// The signs of a whole-edge record, in order.
fn chain_signs(rays: &CurvedRays, q: Point, e: EdgeId) -> Vec<i8> {
    let chain = rays.chain(q, e).expect("stored");
    chain
        .crossings
        .iter()
        .map(|c| c.crossing.sign.as_i8())
        .collect()
}

/// Chains read the other way, closed (one edge, its spans reversed) and
/// open (its vertices' ids swapped): each whole-edge record is the
/// other's crossings in reverse order with signs negated, and the feet
/// are as many, wrapping and hulled alike; points on knots and vertices,
/// beside them by much less than the band across and along the ray
/// line, and on their columns.
#[test]
fn chains_read_the_other_way_negate() {
    for seed in varde_testing::seeds(24, 256) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let op = if seed % 2 == 0 { A } else { B };
        let c = lat(
            (rng.range(-1.0, 1.0) * 16.0).round() / 16.0,
            0.0,
            (rng.range(-1.0, 1.0) * 16.0).round() / 16.0,
        );
        let rad = 1.0 + rng.range(0.0, 2.0);
        let spans: Vec<Span> = circle(c, rad, 4, rng.range(0.0, 1.0))
            .iter()
            .map(|s| {
                let mut pick = || UNEVEN[(rng.next_u64() % 4) as usize];
                reweighted(s, pick(), pick())
            })
            .collect();
        let closed = seed % 4 < 2;
        let tiny = 1.0 / 68_719_476_736.0;
        let mut at = Vec::new();
        for s in &spans {
            let j = point_of(&s[0]);
            for d in [
                DVec3::ZERO,
                R_HAT * tiny,
                -R_HAT * tiny,
                NU * tiny,
                -NU * tiny,
                UP * 2.0,
                R_HAT * 0.5,
            ] {
                at.push(j + d);
            }
        }
        at.push(c);
        at.push(point_of(&spans[3][2]) + NU * tiny);
        let d = DIRECTIONS[(seed / 4) as usize % 5];
        let sigma = sigmas(seed / 20);
        let build = |flip: bool| {
            let mut g = Geom::with(d, sigma);
            let e = if closed {
                let s = if flip {
                    reversed(&spans)
                } else {
                    spans.clone()
                };
                g.edge(op, 7, 3, 3, &s)
            } else {
                let (v, w) = if flip { (5, 3) } else { (3, 5) };
                g.edge(op, 7, v, w, &spans[0..3])
            };
            let qs: Vec<Point> = at
                .iter()
                .enumerate()
                .map(|(i, &p)| g.query(op.other(), 100 + i as u32, p))
                .collect();
            g.r_g = 0.25;
            let (mut rays, mut shadows, mut feet) =
                (CurvedRays::new(), Shadows::new(), NearFeet::new());
            fill_edges(&mut rays, &mut shadows, &mut feet, &g, &qs);
            rays.replay(&shadows, &g, band()).expect("replays");
            feet.replay(&g).expect("replays");
            (g, e, qs, rays, feet)
        };
        let (_, e, qs, rays, feet) = build(false);
        let (_, e2, _, rays2, feet2) = build(true);
        let what = format!("seed {seed}, closed {closed}");
        for &q in &qs {
            let one = chain_signs(&rays, q, e);
            let mut two: Vec<i8> = chain_signs(&rays2, q, e2).iter().map(|s| -s).collect();
            two.reverse();
            assert_eq!(one, two, "{what}: {q:?}");
            if closed {
                assert!(rays.chain(q, e).unwrap().count().abs() <= 1, "{what}");
            }
            let (f, f2) = (feet.get(q, e).unwrap(), feet2.get(q, e2).unwrap());
            assert_eq!(f.feet.len(), f2.feet.len(), "{what}: {q:?}: {f:?} {f2:?}");
            let flags = |f: &Feet| {
                let mut v: Vec<_> = f.feet.iter().map(|k| (k.wraps, k.hull)).collect();
                v.sort();
                v
            };
            assert_eq!(flags(f), flags(f2), "{what}: {q:?}");
        }
    }
}

/// A pass on `threads` threads: the answers and the cost in key order,
/// the first failure in key order.
struct Par {
    threads: usize,
    decided: usize,
    cost: Cost,
}

impl Pass for Par {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        let n = keys.len().div_ceil(self.threads).max(1);
        let parts: Vec<Vec<Decided<V>>> = std::thread::scope(|s| {
            let hs: Vec<_> = keys
                .chunks(n)
                .map(|c| s.spawn(move || c.iter().map(decide).collect::<Vec<_>>()))
                .collect();
            hs.into_iter().map(|h| h.join().expect("joined")).collect()
        });
        let mut out = Vec::with_capacity(keys.len());
        for (v, c) in parts.into_iter().flatten() {
            self.decided += 1;
            self.cost = self.cost.and(c);
            out.push(v?);
        }
        Ok(out)
    }
}

/// A pass running out at its `at`-th key, counted over every batch.
struct OutAt {
    at: usize,
    seen: usize,
}

impl Pass for OutAt {
    type Error = &'static str;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, &'static str> {
        let mut out = Vec::new();
        for k in keys {
            if self.seen == self.at {
                return Err("out");
            }
            self.seen += 1;
            out.push(decide(k).0.map_err(|_| "failed")?);
        }
        Ok(out)
    }
}

/// Every stored whole-edge record, span record and foot, in key order.
type Stored = (
    Vec<(RayKey, ChainRec)>,
    Vec<(SpanKey, SpanRec)>,
    Vec<(RayKey, Feet)>,
);

fn stored(rays: &CurvedRays, feet: &NearFeet) -> Stored {
    (
        rays.chain_table()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        rays.span_table()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        feet.table().iter().map(|(k, v)| (*k, v.clone())).collect(),
    )
}

/// Closed and open chains with ids up to the highest, `NONE − 1` (the edge, its vertices
/// and the points): the whole-edge records and the feet are the same on
/// 1 to 4 threads and for the wanted keys permuted and repeated, at the
/// same cost; a pass running out at any key stores nothing of its
/// batch, no whole-edge record before its spans' passes are through,
/// and a fill after it stores what one fill stores.
#[test]
fn whole_edges_and_feet_on_any_threads_and_out_of_budget() {
    for seed in varde_testing::seeds(8, 64) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let op = if seed % 2 == 0 { A } else { B };
        let high = seed % 3 != 2;
        let id = |k: u32| if high { NONE - 1 - k } else { 10 + k };
        let spans = circle(lat(0.0, 0.0, 0.0), 1.0 + rng.range(0.0, 1.0), 4, 0.25);
        let mut g = Geom::with(DIRECTIONS[(seed / 2) as usize % 5], sigmas(seed / 3));
        let e0 = g.edge(op, id(0), id(1), id(1), &spans);
        let e1 = g.edge(op, id(2), id(2), id(3), &spans[1..3]);
        let mut qs = Vec::new();
        for (k, s) in spans.iter().enumerate() {
            let j = point_of(&s[0]);
            for (i, d) in [DVec3::ZERO, R_HAT * 0.25, UP, NU * 0.01]
                .into_iter()
                .enumerate()
            {
                qs.push(g.query(op.other(), id(10 + 4 * k as u32 + i as u32), j + d));
            }
        }
        g.r_g = 0.5;
        let keys: Vec<RayKey> = qs
            .iter()
            .flat_map(|&q| [e0, e1].map(|g| RayKey { q, g }))
            .collect();
        let run = |pass: &mut dyn FnMut(&mut CurvedRays, &mut Shadows, &mut NearFeet)| {
            let (mut rays, mut shadows, mut feet) =
                (CurvedRays::new(), Shadows::new(), NearFeet::new());
            pass(&mut rays, &mut shadows, &mut feet);
            (rays, shadows, feet)
        };
        let mut seq = Seq::default();
        let (rays, shadows, feet) = run(&mut |r, s, f| {
            r.fill_chains(keys.clone(), s, &g, band(), &mut seq)
                .expect("decided");
            f.fill(keys.clone(), &g, &mut seq).expect("decided");
        });
        rays.replay(&shadows, &g, band()).expect("replays");
        feet.replay(&g).expect("replays");
        let want = stored(&rays, &feet);
        for &q in &qs {
            check_chain(&rays, feet.get(q, e0).unwrap(), &g, q, e0, 0.5, "seq");
            check_chain(&rays, feet.get(q, e1).unwrap(), &g, q, e1, 0.5, "seq");
        }
        for threads in 1..=4 {
            let mut wanted = keys.clone();
            wanted.extend(keys.iter().rev().take(5));
            wanted.reverse();
            let mut par = Par {
                threads,
                decided: 0,
                cost: Cost::default(),
            };
            let (rays, _, feet) = run(&mut |r, s, f| {
                r.fill_chains(wanted.clone(), s, &g, band(), &mut par)
                    .expect("decided");
                f.fill(wanted.clone(), &g, &mut par).expect("decided");
            });
            assert_eq!(stored(&rays, &feet), want, "seed {seed}, {threads} threads");
            assert_eq!((par.decided, par.cost), (seq.decided, seq.cost));
        }
        for at in 0..seq.decided {
            let (mut rays, mut shadows, mut feet) =
                (CurvedRays::new(), Shadows::new(), NearFeet::new());
            let mut out = OutAt { at, seen: 0 };
            let got = rays
                .fill_chains(keys.clone(), &mut shadows, &g, band(), &mut out)
                .and_then(|()| feet.fill(keys.clone(), &g, &mut out));
            assert_eq!(got, Err(ShadowError::Pass("out")), "seed {seed}, at {at}");
            // The whole-edge batch is the last of its passes, the feet's
            // after it: each stores all or nothing.
            let n = keys.len();
            assert!(
                rays.chain_table().is_empty() || rays.chain_table().len() == n,
                "seed {seed}, at {at}"
            );
            assert!(feet.table().is_empty(), "seed {seed}, at {at}");
            if !rays.chain_table().is_empty() {
                assert_eq!(rays.span_table().len(), want.1.len());
            }
            rays.fill_chains(keys.clone(), &mut shadows, &g, band(), &mut Seq::default())
                .expect("decided");
            feet.fill(keys.clone(), &g, &mut Seq::default())
                .expect("decided");
            assert_eq!(stored(&rays, &feet), want, "seed {seed}, at {at}");
        }
    }
}

/// Near radii at the extremes: 0 (feet only where the edge passes
/// through the point exactly, the knot and vertex points), and larger
/// than the edge, 1024 (one foot over the whole of it, through a closed edge's
/// vertex once).
#[test]
fn feet_at_radius_zero_and_beyond_the_edge() {
    let spans = circle(lat(0.0, 0.0, 0.0), 2.0, 4, 0.25);
    for op in [A, B] {
        let mut g = Geom::with(DIRECTIONS[0], Sign::Pos);
        let closed = g.edge(op, 0, 0, 0, &spans);
        let open = g.edge(op, 1, 1, 2, &spans[0..2]);
        let mut qs = Vec::new();
        for (k, s) in spans.iter().enumerate() {
            qs.push(g.query(op.other(), 10 + k as u32, point_of(&s[0])));
        }
        let off = g.query(op.other(), 20, lat(0.5, 0.0, 0.25));
        qs.push(off);
        for r in [0.0, 1024.0] {
            g.r_g = r;
            let (mut rays, mut shadows, mut feet) =
                (CurvedRays::new(), Shadows::new(), NearFeet::new());
            fill_edges(&mut rays, &mut shadows, &mut feet, &g, &qs);
            for (k, &q) in qs.iter().enumerate() {
                for e in [closed, open] {
                    let f = feet.get(q, e).expect("stored");
                    check_feet(&g.edges[&(op, e)].1, g.pos[&q], r, f, "extremes");
                    check_chain(&rays, f, &g, q, e, r, "extremes");
                    let ends = g.ends(op, e);
                    let n = g.edges[&(op, e)].1.len();
                    if r > 0.0 {
                        assert_eq!(f.feet.len(), 1, "{op:?} {k} {e:?}: {f:?}");
                        let verts = f.feet[0].end_vertices(ends, crate::order::Along::Stored);
                        let want = if e == closed { 1 } else { 2 };
                        assert_eq!(verts.len(), want, "{op:?} {k} {e:?}: {f:?}");
                        continue;
                    }
                    // At radius 0: a foot at the point's knot or vertex,
                    // if it is on the edge.
                    let on = k < 4 && (e == closed || k <= n);
                    assert_eq!(f.feet.len(), usize::from(on), "{op:?} {k} {e:?}: {f:?}");
                    if on {
                        let foot = f.feet[0];
                        let vertex = e == closed && k == 0 || e == open && (k == 0 || k == n);
                        assert_eq!(
                            foot.end_vertices(ends, crate::order::Along::Stored).len(),
                            usize::from(vertex),
                            "{op:?} {k} {e:?}: {f:?}"
                        );
                    }
                }
            }
            rays.replay(&shadows, &g, band()).expect("replays");
            feet.replay(&g).expect("replays");
        }
    }
}

/// The bands at magnitude 0: `ρ_op` is the least positive value, above
/// the real 0, and the bands and radii still at least the tolerance's.
#[test]
fn the_bands_at_magnitude_zero() {
    let b = OpBands::new(0.0).expect("finite");
    assert!(b.rho_op() > 0.0 && b.rho_op() < 1e-300);
    assert_eq!(OpBands::new(-0.0), Some(b));
    assert!(b.band(0.0).unwrap() > 0.0);
    assert!(b.near_radius(1.0).unwrap() >= 3.0);
}

/// The other geometry of a shadow passing a point twice: a circle seen
/// nearly edge-on (its plane along `ν` and nearly along `UP`, tilted
/// towards `r̂`), whose shadow is a thin ellipse, its upper half on one
/// side of the `r̂` line through the centre, its lower on the other.
/// Points inside the circle near its top, beside it in shadow: one foot,
/// on the upper half; the ray crosses one half once, the upper in that
/// foot, the lower (far in space) in none, so it reads its own height.
#[test]
fn a_circle_seen_nearly_edge_on_gives_one_foot_near_its_top() {
    for seed in varde_testing::seeds(16, 128) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let op = if seed % 2 == 0 { A } else { B };
        let c = lat(
            (rng.range(-1.0, 1.0) * 16.0).round() / 16.0,
            0.0,
            (rng.range(-1.0, 1.0) * 16.0).round() / 16.0,
        );
        // About 16 long each; the tilt moves the top 1/4 along `r̂`.
        let a = NU / 32.0;
        let b = UP / 2.0 + R_HAT / 64.0;
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let pts = [c + a, c + b, c - a, c - b, c + a];
        let corner = [c + a + b, c - a + b, c - a - b, c + a - b];
        let spans: Vec<Span> = (0..4)
            .map(|k| [cp(pts[k], 1.0), cp(corner[k], w), cp(pts[k + 1], 1.0)])
            .collect();
        let mut g = Geom::with(DIRECTIONS[(seed / 2) as usize % 5], sigmas(seed / 10));
        let e = g.edge(op, 0, 0, 0, &spans);
        let mut qs = Vec::new();
        for i in 0..3u32 {
            let along = (rng.range(-1.0, 1.0) * 16.0).round() / 256.0;
            let down = 1.0 - f64::from(i + 1) / 16.0;
            qs.push(g.query(op.other(), 10 + i, c + a * along + b * down));
        }
        g.r_g = 4.0;
        let (mut rays, mut shadows, mut feet) =
            (CurvedRays::new(), Shadows::new(), NearFeet::new());
        fill_edges(&mut rays, &mut shadows, &mut feet, &g, &qs);
        let what = format!("seed {seed}");
        for &q in &qs {
            check(&rays, &g, q, true, &what);
            let f = feet.get(q, e).expect("stored");
            check_feet(&spans, g.pos[&q], g.r_g, f, &what);
            assert_eq!(f.feet.len(), 1, "{what}: {q:?}: {f:?}");
            let held = check_chain(&rays, f, &g, q, e, g.r_g, &what);
            assert_eq!(held.len(), 1, "{what}: {q:?}: {held:?}");
            let (span, _, ref k) = held[0];
            // Spans 0 and 1 are the upper half.
            assert_eq!(k.is_empty(), span >= 2, "{what}: {q:?}: {held:?}");
            assert_eq!(rays.chain(q, e).unwrap().count().abs(), 1, "{what}");
        }
        rays.replay(&shadows, &g, band()).expect("replays");
        feet.replay(&g).expect("replays");
    }
}
