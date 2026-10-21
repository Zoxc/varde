//! Feet against the set they stand for: straight edges, an exact circle
//! arc (a 3-4-5 conic whose weights are floats) with the point at its
//! centre, the sphere tangent to it from outside and inside, the point on
//! the arc, a knot exactly on the sphere, closed chains whose vertex lies
//! in a foot; random conics against dense samples of the distance; random
//! polynomial chains, and rational ones scaled so that their pieces are
//! floats, cut at dyadic parameters (exactly, by blossoming) and stored
//! the other way, which give the same feet up to their brackets; a short
//! span far out settled at the first level;
//! inputs a caller must not give fail with the record's key; and
//! ([`oracle`]) every foot against the set worked out exactly.

use glam::DVec3;
use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::*;
use crate::ids::{EdgeId, VertId};
use crate::records::keys::{Operand, Point};

fn key() -> RayKey {
    RayKey {
        q: Point::vert(Operand::A, VertId(0)),
        g: EdgeId(1),
    }
}

fn feet(q: DVec3, curve: EdgeCurve<'_>, r: f64) -> Feet {
    let (got, _) = decide_feet(key(), q, curve, r);
    got.expect("feet decided")
}

fn spans(spans: &[Span]) -> EdgeCurve<'_> {
    EdgeCurve::Spans {
        spans,
        closed: false,
    }
}

fn closed(spans: &[Span]) -> EdgeCurve<'_> {
    EdgeCurve::Spans {
        spans,
        closed: true,
    }
}

/// The span's point at `t`, in `f64`, by de Casteljau on the homogeneous
/// points.
fn eval(s: &[[f64; 4]], t: f64) -> DVec3 {
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

/// A straight span from `a` to `b`, the middle control point on the
/// chord (exact for integer ends).
fn straight(a: [f64; 3], b: [f64; 3]) -> Span {
    let m = std::array::from_fn::<f64, 3, _>(|k| 0.5 * (a[k] + b[k]));
    [
        [a[0], a[1], a[2], 1.0],
        [m[0], m[1], m[2], 1.0],
        [b[0], b[1], b[2], 1.0],
    ]
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

/// Every sample well inside the sphere in some foot, every one well
/// outside in none (beyond the brackets).
fn check_samples(chain: &[&[[f64; 4]]], q: DVec3, r: f64, got: &Feet, what: &str) {
    const N: u32 = 512;
    let margin = 1e-7 * (1.0 + r);
    for (i, s) in chain.iter().enumerate() {
        let i = i as u32;
        for j in 0..=N {
            let t = f64::from(j) / f64::from(N);
            let d = (eval(s, t) - q).length() - r;
            let feet = got.feet.iter();
            if d < -margin {
                assert!(
                    feet.clone().any(|f| holds(f, (i, t), true)),
                    "{what}: ({i}, {t}) inside by {d} in no foot: {got:?}"
                );
            } else if d > margin {
                assert!(
                    !feet.clone().any(|f| holds(f, (i, t), false)),
                    "{what}: ({i}, {t}) outside by {d} in a foot: {got:?}"
                );
            }
        }
    }
}

fn line_check(a: DVec3, b: DVec3, q: DVec3, r: f64) -> Feet {
    let got = feet(q, EdgeCurve::Line([a, b]), r);
    let seg = [[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]];
    check_samples(&[&seg[..]], q, r, &got, "line");
    got
}

#[test]
fn a_straight_edge_has_one_foot_at_most() {
    let (a, b) = (DVec3::new(-10.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0));
    // Crossings at `x = ±√3`.
    let got = line_check(a, b, DVec3::new(0.0, 1.0, 0.0), 2.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert_eq!(
        (f.lo.kind, f.hi.kind),
        (EndKind::Crossing, EndKind::Crossing)
    );
    for (end, x) in [(f.lo, -(3f64.sqrt())), (f.hi, 3f64.sqrt())] {
        let t = (x + 10.0) / 20.0;
        assert!(
            end.t[0] - 1e-15 <= t && t <= end.t[1] + 1e-15,
            "{end:?} {t}"
        );
    }
    assert!(!f.hull && !f.wraps);
    assert_eq!(f.ends(), [false, false]);

    let got = line_check(a, b, DVec3::new(-10.0, 0.5, 0.0), 1.0);
    assert_eq!(got.feet.len(), 1);
    assert_eq!(got.feet[0].lo.kind, EndKind::Vertex);
    assert_eq!(got.feet[0].hi.kind, EndKind::Crossing);
    assert_eq!(got.feet[0].ends(), [true, false]);

    assert!(
        line_check(a, b, DVec3::new(0.0, 5.0, 0.0), 4.0)
            .feet
            .is_empty()
    );

    let got = line_check(a, b, DVec3::new(0.0, 5.0, 0.0), 100.0);
    assert_eq!(got.feet.len(), 1);
    assert_eq!(got.feet[0].ends(), [true, true]);
    assert!(!got.feet[0].hull);

    // An end exactly on the sphere, the rest outside: the vertex alone.
    let got = line_check(a, b, DVec3::new(-13.0, 4.0, 0.0), 5.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert_eq!(f.lo.kind, EndKind::Vertex);
    assert_eq!((f.hi.kind, f.hi.t), (EndKind::OnSphere, [0.0, 0.0]));

    // Tangent to the sphere: a foot at the touching point.
    let got = line_check(a, b, DVec3::new(2.5, 3.0, 0.0), 3.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert!(f.hull);
    assert_eq!((f.lo.kind, f.hi.kind), (EndKind::Hull, EndKind::Hull));
    let t = 12.5 / 20.0;
    assert!(f.lo.t[0] <= t && t <= f.hi.t[1] && f.hi.t[1] - f.lo.t[0] < 1e-9);
}

/// An arc of the circle of radius 25 about the origin from `(25, 0)` to
/// `(7, 24)`, exactly: the tangents meet at `(25, 18.75)`, and the
/// weights `(1, 1, 25/16)` give `w₁²/(w₀·w₂) = cos²(θ/2) = 16/25`.
const ARC: Span = [
    [25.0, 0.0, 0.0, 1.0],
    [25.0, 18.75, 0.0, 1.0],
    [7.0, 24.0, 0.0, 1.5625],
];

fn arc_check(q: DVec3, r: f64) -> Feet {
    let got = feet(q, spans(&[ARC]), r);
    check_samples(&[&ARC[..]], q, r, &got, "arc");
    got
}

#[test]
fn an_arc_about_the_point() {
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        assert!((eval(&ARC, t).length() - 25.0).abs() < 1e-12);
    }
    // On the sphere: one foot, the whole edge, every cluster inside.
    let got = arc_check(DVec3::ZERO, 25.0);
    assert_eq!(got.feet.len(), 1);
    assert_eq!(got.feet[0].ends(), [true, true]);
    assert!(got.feet[0].hull);

    let got = arc_check(DVec3::ZERO, 25.000_001);
    assert_eq!(got.feet.len(), 1);
    assert_eq!(got.feet[0].ends(), [true, true]);
    assert!(!got.feet[0].hull);

    assert!(arc_check(DVec3::ZERO, 24.999_999).feet.is_empty());
    assert!(arc_check(DVec3::new(0.0, 0.0, 1.0), 25.0).feet.is_empty());
}

/// The parameter bounds of a foot's whole extent on a one-span edge.
fn extent(f: &Foot) -> [f64; 2] {
    [f.lo.t[0], f.hi.t[1]]
}

#[test]
fn spheres_tangent_to_the_arc() {
    // `(20, 15)` is on the arc, `(40, 30)` 25 beyond it along the normal:
    // the sphere touches the arc there from outside.
    let touch = DVec3::new(20.0, 15.0, 0.0);
    let got = arc_check(DVec3::new(40.0, 30.0, 0.0), 25.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert!(f.hull);
    assert_eq!((f.lo.kind, f.hi.kind), (EndKind::Hull, EndKind::Hull));
    let [lo, hi] = extent(&f);
    assert!(hi - lo < 1e-9, "{f:?}");
    assert!((eval(&ARC, lo) - touch).length() < 1e-7);

    // Slightly larger: two crossings; slightly smaller: none.
    let got = arc_check(DVec3::new(40.0, 30.0, 0.0), 25.001);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert!(!f.hull);
    assert_eq!(
        (f.lo.kind, f.hi.kind),
        (EndKind::Crossing, EndKind::Crossing)
    );
    assert!(
        arc_check(DVec3::new(40.0, 30.0, 0.0), 24.999)
            .feet
            .is_empty()
    );

    // From inside: the sphere about `(−4, −3)` of radius 30 holds the
    // circle, touching it at `(20, 15)`; one foot, the touching point in
    // it.
    let got = arc_check(DVec3::new(-4.0, -3.0, 0.0), 30.0);
    assert_eq!(got.feet.len(), 1);
    assert_eq!(got.feet[0].ends(), [true, true]);
    assert!(got.feet[0].hull);
    let got = arc_check(DVec3::new(-4.0, -3.0, 0.0), 30.001);
    assert_eq!(got.feet.len(), 1);
    assert!(!got.feet[0].hull);
    // Slightly smaller: the stretches on both sides of it apart.
    let got = arc_check(DVec3::new(-4.0, -3.0, 0.0), 29.999);
    assert_eq!(got.feet.len(), 2);
    assert_eq!(got.feet[0].ends(), [true, false]);
    assert_eq!(got.feet[1].ends(), [false, true]);
}

#[test]
fn a_point_on_the_arc() {
    let q = DVec3::new(20.0, 15.0, 0.0);
    let got = arc_check(q, 1.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert!(!f.hull);
    for end in [f.lo, f.hi] {
        assert_eq!(end.kind, EndKind::Crossing);
        assert!(((eval(&ARC, end.t[0]) - q).length() - 1.0).abs() < 1e-9);
    }
    // At radius 0 the set is the point: a foot of one cluster.
    let got = arc_check(q, 0.0);
    assert_eq!(got.feet.len(), 1);
    assert!(got.feet[0].hull);
    let [lo, hi] = extent(&got.feet[0]);
    assert!(hi - lo < 1e-9);
    // At a vertex, radius 0: just the vertex, a double root at the
    // span's end, found exactly at the first level: the vertex on the
    // sphere.
    let got = arc_check(DVec3::new(7.0, 24.0, 0.0), 0.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert!(!f.hull);
    assert_eq!(f.lo.kind, EndKind::OnSphere);
    assert_eq!(f.lo.t, [1.0, 1.0]);
    assert_eq!(f.hi.kind, EndKind::Vertex);
    assert_eq!(f.ends(), [false, true]);
}

/// A V: straight spans from `(−1, 1)` to `(0, 0)` and on to `(1, 1)`.
fn vee() -> [Span; 2] {
    [
        straight([-1.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
        straight([0.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
    ]
}

#[test]
fn a_knot_on_the_sphere() {
    let v = vee();
    let chain = [&v[0][..], &v[1][..]];
    let q = DVec3::new(0.0, -1.0, 0.0);
    // Only the knot is within 1 of `q`: one foot, the knot alone, read
    // from its exact side by both spans.
    let got = feet(q, spans(&v), 1.0);
    check_samples(&chain, q, 1.0, &got, "vee");
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert_eq!(
        (f.lo.span, f.lo.t, f.lo.kind),
        (0, [1.0, 1.0], EndKind::OnSphere)
    );
    assert_eq!(
        (f.hi.span, f.hi.t, f.hi.kind),
        (1, [0.0, 0.0], EndKind::OnSphere)
    );
    assert!(!f.hull);
    // Larger: one foot across the knot.
    let got = feet(q, spans(&v), 1.5);
    check_samples(&chain, q, 1.5, &got, "vee");
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert_eq!((f.lo.span, f.lo.kind), (0, EndKind::Crossing));
    assert_eq!((f.hi.span, f.hi.kind), (1, EndKind::Crossing));
    // Smaller: none.
    assert!(feet(q, spans(&v), 0.999).feet.is_empty());
}

/// The square `(0, 0) → (4, 0) → (4, 4) → (0, 4) → (0, 0)`, closed.
fn square() -> [Span; 4] {
    let c = [
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [0.0, 4.0, 0.0],
    ];
    std::array::from_fn(|i| straight(c[i], c[(i + 1) % 4]))
}

#[test]
fn a_closed_edge_joins_the_feet_at_its_vertex() {
    let sq = square();
    let chain: Vec<&[[f64; 4]]> = sq.iter().map(|s| &s[..]).collect();
    let check = |q: DVec3, r: f64| {
        let got = feet(q, closed(&sq), r);
        check_samples(&chain, q, r, &got, "square");
        got
    };
    // About the vertex: one foot through it, from the last span to the
    // first.
    let got = check(DVec3::ZERO, 1.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert!(f.wraps);
    assert_eq!((f.lo.span, f.hi.span), (3, 0));
    assert_eq!(f.ends(), [true, true]);
    // Larger: through the vertex, from the third span to the second.
    let got = check(DVec3::ZERO, 4.5);
    assert_eq!(got.feet.len(), 1);
    assert!(got.feet[0].wraps);
    assert_eq!((got.feet[0].lo.span, got.feet[0].hi.span), (2, 1));
    // Open, the same chain has two feet there.
    assert_eq!(feet(DVec3::ZERO, spans(&sq), 1.0).feet.len(), 2);

    // About the centre: a foot on each side, or the whole loop.
    let c = DVec3::new(2.0, 2.0, 0.0);
    let got = check(c, 2.5);
    assert_eq!(got.feet.len(), 4);
    assert!(
        got.feet
            .iter()
            .all(|f| !f.wraps && f.ends() == [false, false])
    );
    for (i, f) in got.feet.iter().enumerate() {
        assert_eq!((f.lo.span, f.hi.span), (i as u32, i as u32));
    }
    let ids: Vec<u32> = got.in_order(Along::Reversed).map(|k| k.0).collect();
    assert_eq!(ids, [3, 2, 1, 0]);
    // Each side touches the sphere of radius 2 at its middle.
    let got = check(c, 2.0);
    assert_eq!(got.feet.len(), 4);
    assert!(got.feet.iter().all(|f| f.hull));
    let got = check(c, 3.0);
    assert_eq!(got.feet.len(), 1);
    assert!(!got.feet[0].wraps);
    assert_eq!(got.feet[0].ends(), [true, true]);

    // About a point near the vertex and the side beside it: one foot
    // through the vertex.
    let got = check(DVec3::new(0.0, 1.0, 0.0), 1.2);
    assert_eq!(got.feet.len(), 1);
    assert!(got.feet[0].wraps);
}

#[test]
fn a_foot_through_a_closed_edges_vertex_comes_first() {
    let foot = |lo: u32, hi: u32, wraps: bool| Foot {
        lo: FootEnd {
            span: lo,
            t: [0.5, 0.5],
            kind: EndKind::Crossing,
        },
        hi: FootEnd {
            span: hi,
            t: [0.5, 0.5],
            kind: EndKind::Crossing,
        },
        wraps,
        hull: false,
    };
    let order = |f: &Feet, along| f.in_order(along).map(|k| k.0).collect::<Vec<_>>();
    let got = Feet {
        feet: [foot(3, 0, true), foot(1, 1, false), foot(2, 2, false)]
            .into_iter()
            .collect(),
    };
    assert_eq!(order(&got, Along::Stored), [0, 1, 2]);
    assert_eq!(order(&got, Along::Reversed), [0, 2, 1]);
    let open = Feet {
        feet: [foot(0, 0, false), foot(1, 1, false)].into_iter().collect(),
    };
    assert_eq!(order(&open, Along::Reversed), [1, 0]);
    assert_eq!(got.get(FootId(1)), Some(&got.feet[1]));
    assert_eq!(got.get(FootId(3)), None);
}

fn conic(rng: &mut varde_testkit::rng::Rng) -> Span {
    let mut cp = || {
        let w = if rng.next_u64().is_multiple_of(3) {
            1.0
        } else {
            rng.log_range(1.0 / 64.0, 64.0)
        };
        let p = rng.point(4.0);
        [p.x, p.y, p.z, w]
    };
    [cp(), cp(), cp()]
}

/// Random conic chains against dense samples of the distance.
#[test]
fn random_conics_against_samples() {
    for seed in varde_testing::seeds(64, 2048) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let n = 1 + (rng.next_u64() % 3) as usize;
        let mut chain: Vec<Span> = Vec::new();
        for i in 0..n {
            let mut s = conic(&mut rng);
            if i > 0 {
                let p = chain[i - 1][2];
                s[0] = [p[0], p[1], p[2], s[0][3]];
            }
            chain.push(s);
        }
        let q = rng.point(4.0);
        let r = rng.log_range(0.05, 6.0);
        let got = feet(q, spans(&chain), r);
        let refs: Vec<&[[f64; 4]]> = chain.iter().map(|s| &s[..]).collect();
        check_samples(&refs, q, r, &got, &format!("seed {seed}"));
    }
}

/// The span on `[a, b]` of the polynomial span `s` (weights 1), by
/// blossoming: exact for small integer control points and parameters in
/// eighths.
fn piece(s: &Span, a: f64, b: f64) -> Span {
    let blossom = |u: f64, v: f64| -> [f64; 4] {
        let c = [(1.0 - u) * (1.0 - v), (1.0 - u) * v + u * (1.0 - v), u * v];
        let x = |k: usize| c[0] * s[0][k] + c[1] * s[1][k] + c[2] * s[2][k];
        [x(0), x(1), x(2), 1.0]
    };
    [blossom(a, a), blossom(a, b), blossom(b, b)]
}

/// Whether two brackets meet, within `slack`.
fn meet(a: [f64; 2], b: [f64; 2], slack: f64) -> bool {
    a[0] <= b[1] + slack && b[0] <= a[1] + slack
}

/// Feet of a cut chain, mapped back onto the original's spans: `map`
/// gives each cut span's original span and its `[a, b]` there.
fn mapped(got: &Feet, map: &[(u32, f64, f64, bool)]) -> Vec<[(u32, [f64; 2], EndKind); 2]> {
    let back = |e: FootEnd| {
        let (span, a, b, rev) = map[e.span as usize];
        let t = e.t.map(|t| a + (b - a) * if rev { 1.0 - t } else { t });
        (span, if rev { [t[1], t[0]] } else { t }, e.kind)
    };
    got.feet.iter().map(|f| [back(f.lo), back(f.hi)]).collect()
}

/// Random polynomial chains cut at random eighths, and stored the other
/// way: the same feet, their ends' brackets meeting.
#[test]
fn feet_do_not_depend_on_the_cuts() {
    let (mut compared, mut with_feet) = (0, 0);
    for seed in varde_testing::seeds(128, 4096) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let int = |rng: &mut varde_testkit::rng::Rng, n: u64| {
            (rng.next_u64() % (2 * n + 1)) as f64 - n as f64
        };
        let n = 1 + (rng.next_u64() % 3) as usize;
        let mut chain: Vec<Span> = Vec::new();
        for i in 0..n {
            let mut s: Span = std::array::from_fn(|_| {
                [int(&mut rng, 8), int(&mut rng, 8), int(&mut rng, 2), 1.0]
            });
            if i > 0 {
                s[0] = chain[i - 1][2];
            }
            chain.push(s);
        }
        let q = DVec3::new(
            int(&mut rng, 8) / 2.0,
            int(&mut rng, 8) / 2.0,
            int(&mut rng, 2) / 2.0,
        );
        let r = rng.log_range(0.5, 8.0);
        // Cut each span at up to two eighths.
        let mut cut: Vec<Span> = Vec::new();
        let mut map = Vec::new();
        for (i, s) in chain.iter().enumerate() {
            let mut at: Vec<f64> = (0..rng.next_u64() % 3)
                .map(|_| (1 + rng.next_u64() % 7) as f64 / 8.0)
                .collect();
            at.sort_by(f64::total_cmp);
            at.dedup();
            let bounds: Vec<f64> = std::iter::once(0.0)
                .chain(at)
                .chain(std::iter::once(1.0))
                .collect();
            for w in bounds.windows(2) {
                cut.push(piece(s, w[0], w[1]));
                map.push((i as u32, w[0], w[1], false));
            }
        }
        if let Some(any) = same_feet(seed, &chain, &cut, &map, q, r) {
            compared += 1;
            with_feet += usize::from(any);
        }
    }
    if varde_testing::replay_seed().is_none() {
        assert!(with_feet > compared / 4, "{with_feet} of {compared}");
    }
}

/// The feet of `chain`, of `cut` (its spans cut, `map` as for [`mapped`])
/// and of `chain` stored the other way are the same: as many, their ends
/// on the same spans of `chain`, brackets meeting, of one kind (or a
/// crossing against a knot exactly on the sphere). `None` where any of
/// the three has a hull foot, else whether there are feet.
fn same_feet(
    seed: u64,
    chain: &[Span],
    cut: &[Span],
    map: &[(u32, f64, f64, bool)],
    q: DVec3,
    r: f64,
) -> Option<bool> {
    let n = chain.len();
    let base = feet(q, spans(chain), r);
    let refs: Vec<&[[f64; 4]]> = chain.iter().map(|s| &s[..]).collect();
    check_samples(&refs, q, r, &base, &format!("seed {seed}"));
    let rev: Vec<Span> = chain.iter().rev().map(|s| [s[2], s[1], s[0]]).collect();
    let rev_map: Vec<(u32, f64, f64, bool)> =
        (0..n).rev().map(|i| (i as u32, 0.0, 1.0, true)).collect();
    let cut_feet = feet(q, spans(cut), r);
    let rev_feet = feet(q, spans(&rev), r);
    let any_hull = |f: &Feet| f.feet.iter().any(|f| f.hull);
    if any_hull(&base) || any_hull(&cut_feet) || any_hull(&rev_feet) {
        return None;
    }
    let ident: Vec<(u32, f64, f64, bool)> = (0..n).map(|i| (i as u32, 0.0, 1.0, false)).collect();
    let ours = mapped(&base, &ident);
    let mut theirs_rev = mapped(&rev_feet, &rev_map);
    theirs_rev.reverse();
    let theirs_rev: Vec<_> = theirs_rev.into_iter().map(|[a, b]| [b, a]).collect();
    for (what, theirs) in [("cut", mapped(&cut_feet, map)), ("reversed", theirs_rev)] {
        assert_eq!(ours.len(), theirs.len(), "seed {seed} {what}: {base:?}");
        for (a, b) in ours.iter().zip(&theirs) {
            for (x, y) in a.iter().zip(b) {
                assert_eq!(x.0, y.0, "seed {seed} {what}: span");
                assert!(meet(x.1, y.1, 1e-12), "seed {seed} {what}: {x:?} {y:?}");
                let kinds = [x.2, y.2];
                let ok = x.2 == y.2
                    || kinds.contains(&EndKind::OnSphere) && kinds.contains(&EndKind::Crossing);
                assert!(ok, "seed {seed} {what}: {x:?} {y:?}");
            }
        }
    }
    Some(!base.feet.is_empty())
}

/// The piece on `[a, b]` of the rational span `s`, by blossoming its
/// homogeneous points exactly: the affine points and weights, rationals.
fn rational_piece(s: &Span, a: &Q, b: &Q) -> [[Q; 4]; 3] {
    let hom: Vec<[Q; 4]> = s
        .iter()
        .map(|p| {
            let w = Q::try_from(p[3]).expect("finite");
            let x = |k: usize| Q::try_from(p[k]).expect("finite") * &w;
            [x(0), x(1), x(2), w]
        })
        .collect();
    let one = Q::from(1u32);
    let blossom = |u: &Q, v: &Q| -> [Q; 4] {
        let (u1, v1) = (&one - u, &one - v);
        let c = [&u1 * &v1, &u1 * v + u * &v1, u * v];
        let h: [Q; 4] =
            std::array::from_fn(|k| c.iter().zip(&hom).map(|(c, h)| c * &h[k]).sum::<Q>());
        let w = h[3].clone();
        [&h[0] / &w, &h[1] / &w, &h[2] / &w, w]
    };
    [blossom(a, a), blossom(a, b), blossom(b, b)]
}

/// The odd part of `n > 0`.
fn odd(n: u64) -> u64 {
    n >> n.trailing_zeros()
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Random rational chains (dyadic weights) cut at random eighths, scaled
/// so that every piece's affine points are floats: the cut chain is the
/// same curve exactly, and gives the same feet.
#[test]
fn feet_of_rational_chains_do_not_depend_on_the_cuts() {
    let (mut compared, mut with_feet) = (0, 0);
    for seed in varde_testing::seeds(128, 4096) {
        let mut rng = varde_testkit::rng::Rng::new(seed);
        let int = |rng: &mut varde_testkit::rng::Rng, n: u64| {
            (rng.next_u64() % (2 * n + 1)) as f64 - n as f64
        };
        let n = 1 + (rng.next_u64() % 3) as usize;
        let mut chain: Vec<Span> = Vec::new();
        for i in 0..n {
            let mut s: Span = std::array::from_fn(|_| {
                let w = (1 + rng.next_u64() % 8) as f64 / 4.0;
                [int(&mut rng, 8), int(&mut rng, 8), int(&mut rng, 2), w]
            });
            if i > 0 {
                let p = chain[i - 1][2];
                s[0] = [p[0], p[1], p[2], s[0][3]];
            }
            chain.push(s);
        }
        let mut cuts: Vec<Vec<f64>> = Vec::new();
        for _ in 0..n {
            let mut at: Vec<f64> = (0..rng.next_u64() % 3)
                .map(|_| (1 + rng.next_u64() % 7) as f64 / 8.0)
                .collect();
            at.sort_by(f64::total_cmp);
            at.dedup();
            cuts.push(
                std::iter::once(0.0)
                    .chain(at)
                    .chain(std::iter::once(1.0))
                    .collect(),
            );
        }
        let pieces = |chain: &[Span]| -> Vec<[[Q; 4]; 3]> {
            let mut out = Vec::new();
            for (s, bounds) in chain.iter().zip(&cuts) {
                for w in bounds.windows(2) {
                    let (a, b) = (Q::try_from(w[0]).unwrap(), Q::try_from(w[1]).unwrap());
                    out.push(rational_piece(s, &a, &b));
                }
            }
            out
        };
        // The scale: the least common multiple of the odd parts of the
        // pieces' denominators.
        // Chains whose scale passes `2¹⁶` are skipped (the multiple can
        // pass `u64` before that is seen, so it stops there).
        let mut l = Some(1u64);
        for p in pieces(&chain).iter().flatten().flat_map(|p| &p[..3]) {
            let Some(d) = u64::try_from(p.denominator_ref()).ok() else {
                l = None;
                break;
            };
            let o = odd(d);
            l = l.and_then(|l| (l / gcd(l, o)).checked_mul(o));
            if l.is_none_or(|l| l > 1 << 16) {
                l = None;
                break;
            }
        }
        let Some(l) = l else {
            continue;
        };
        let lf = l as f64;
        for s in &mut chain {
            for p in s.iter_mut() {
                for c in &mut p[..3] {
                    *c *= lf;
                }
            }
        }
        let cut: Vec<Span> = pieces(&chain)
            .iter()
            .map(|p| {
                p.clone().map(|c| {
                    c.map(|x| {
                        let f = f64::rounding_from(&x, Nearest).0;
                        assert_eq!(Q::try_from(f).ok(), Some(x), "seed {seed}: inexact");
                        f
                    })
                })
            })
            .collect();
        let mut map = Vec::new();
        for (i, bounds) in cuts.iter().enumerate() {
            for w in bounds.windows(2) {
                map.push((i as u32, w[0], w[1], false));
            }
        }
        let q = DVec3::new(
            int(&mut rng, 8) / 2.0,
            int(&mut rng, 8) / 2.0,
            int(&mut rng, 2) / 2.0,
        ) * lf;
        let r = (8 + rng.next_u64() % 120) as f64 / 16.0 * lf;
        if let Some(any) = same_feet(seed, &chain, &cut, &map, q, r) {
            compared += 1;
            with_feet += usize::from(any);
        }
    }
    if varde_testing::replay_seed().is_none() {
        assert!(compared > 32, "{compared}");
        assert!(with_feet > compared / 4, "{with_feet} of {compared}");
    }
}

#[test]
fn bad_inputs_fail_with_the_record() {
    let internal = Err(Fail::Internal {
        key: TieKey {
            row: Row::VertexCurveShadow,
            record: RecordKey::Ray(key()),
        },
    });
    let v = vee();
    let q = DVec3::ZERO;
    for r in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(decide_feet(key(), q, spans(&v), r).0, internal);
    }
    assert_eq!(decide_feet(key(), DVec3::NAN, spans(&v), 1.0).0, internal);
    assert_eq!(decide_feet(key(), q, spans(&[]), 1.0).0, internal);
    let mut broken = v;
    broken[1][0][0] = 1e-300;
    assert_eq!(decide_feet(key(), q, spans(&broken), 1.0).0, internal);
    // Not closed: its ends are apart.
    assert_eq!(decide_feet(key(), q, closed(&v), 1.0).0, internal);
    let mut weight = v;
    weight[0][1][3] = 0.0;
    assert_eq!(decide_feet(key(), q, spans(&weight), 1.0).0, internal);
    let mut inf = v;
    inf[0][1][1] = f64::INFINITY;
    assert_eq!(decide_feet(key(), q, spans(&inf), 1.0).0, internal);
}

/// Roots exactly at dyadic parameters, where the isolation splits: their
/// brackets narrowed to the root; a cut there ends the foot exactly at
/// the knot.
#[test]
fn roots_on_split_points_and_cuts() {
    let s = straight([-4.0, 0.0, 0.0], [4.0, 0.0, 0.0]);
    let got = feet(DVec3::ZERO, spans(&[s]), 2.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    for (end, t) in [(f.lo, 0.25), (f.hi, 0.75)] {
        assert_eq!(end.kind, EndKind::Crossing);
        assert!(
            end.t[0] <= t && t <= end.t[1] && end.t[1] - end.t[0] < 1e-12,
            "{end:?}"
        );
    }
    let cut = [piece(&s, 0.0, 0.25), piece(&s, 0.25, 1.0)];
    let got = feet(DVec3::ZERO, spans(&cut), 2.0);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert_eq!(
        (f.lo.span, f.lo.t, f.lo.kind),
        (0, [1.0, 1.0], EndKind::OnSphere)
    );
    assert_eq!((f.hi.span, f.hi.kind), (1, EndKind::Crossing));
}

/// A near radius far below the coordinates' rounding at the first level:
/// the arc scaled by 4096 and moved by `10⁵`, the point `2⁻²¹` off it.
#[test]
fn a_small_radius_at_large_coordinates() {
    let (k, o) = (4096.0, 1e5);
    let arc: Span = ARC.map(|p| [p[0] * k + o, p[1] * k, p[2] * k, p[3]]);
    let on = DVec3::new(20.0 * k + o, 15.0 * k, 0.0);
    let off = 1.0 / 2_097_152.0;
    let q = on + DVec3::new(0.8, 0.6, 0.0) * off;
    let got = feet(q, spans(&[arc]), 1e-6);
    assert_eq!(got.feet.len(), 1);
    let f = got.feet[0];
    assert!(!f.hull);
    for end in [f.lo, f.hi] {
        assert_eq!(end.kind, EndKind::Crossing);
        let d = (eval(&arc, 0.5 * (end.t[0] + end.t[1])) - q).length();
        assert!((d - 1e-6).abs() < 1e-8, "{d}");
    }
    assert!(feet(q, spans(&[arc]), 0.4e-6).feet.is_empty());
}

/// The same at a short span: the arc scaled by `1/32` and moved by
/// `10⁵`. Formed about the point, the first level settles it.
#[test]
fn a_small_radius_on_a_short_span_far_out() {
    let (k, o) = (1.0 / 32.0, 1e5);
    let arc: Span = ARC.map(|p| [p[0] * k + o, p[1] * k, p[2] * k, p[3]]);
    let on = DVec3::new(20.0 * k + o, 15.0 * k, 0.0);
    let q = on + DVec3::new(0.8, 0.6, 0.0) / 2_097_152.0;
    let (got, cost) = decide_feet(key(), q, spans(&[arc]), 1e-6);
    let got = got.expect("feet decided");
    assert_eq!(got.feet.len(), 1);
    assert!(!got.feet[0].hull);
    assert!(
        cost.levels[1..].iter().all(|l| l.evals == 0 && l.feet == 0),
        "{:?}",
        cost.levels
    );
    for end in [got.feet[0].lo, got.feet[0].hi] {
        assert_eq!(end.kind, EndKind::Crossing);
        let d = (eval(&arc, 0.5 * (end.t[0] + end.t[1])) - q).length();
        assert!((d - 1e-6).abs() < 1e-8, "{d}");
    }
}

mod oracle;

/// A sphere tangent to a span, from outside and from inside, a straight
/// edge and the 3-4-5 arc: decided at the first level by the exact stage
/// (no level past it runs), the touching point a hull foot; the radius a
/// little larger or smaller decides at the first level by value.
#[test]
fn tangent_spheres_decide_at_the_first_level() {
    let line = straight([-4.0, 3.0, 0.0], [4.0, 3.0, 0.0]);
    // About the origin, radius 3: touching the line at `(0, 3)` from
    // outside; the arc as in `spheres_tangent_to_the_arc`.
    for (span, q, r, inside) in [
        (line, DVec3::ZERO, 3.0, false),
        (ARC, DVec3::new(40.0, 30.0, 0.0), 25.0, false),
        (ARC, DVec3::new(-4.0, -3.0, 0.0), 30.0, true),
    ] {
        let (got, cost) = decide_feet(key(), q, spans(&[span]), r);
        let got = got.expect("decided");
        check_samples(&[&span[..]], q, r, &got, "tangent");
        assert!(
            cost.levels[1..].iter().all(|l| *l == LevelWork::default()),
            "{cost:?}"
        );
        assert!(cost.rationals > 0, "{cost:?}");
        assert!(got.feet.iter().any(|f| f.hull), "{got:?}");
        if !inside {
            let f = got.feet[0];
            assert!(f.hi.t[1] - f.lo.t[0] < 1e-15, "{f:?}");
        }
    }
}

/// A crossing near `t = 1` is bracketed on the span run backwards, as the
/// same crossing near `t = 0` of the span stored the other way: the
/// brackets of the two storage directions, mirrored, meet.
#[test]
fn brackets_near_the_end_mirror_those_near_the_start() {
    let mut rng = varde_testkit::rng::Rng::new(7);
    for _ in 0..64 {
        let s = conic(&mut rng);
        let rev: Span = [s[2], s[1], s[0]];
        let t = 1.0 - rng.range(1e-12, 0.25);
        let q = eval(&s, t) + DVec3::new(rng.range(-0.5, 0.5), rng.range(-0.5, 0.5), 0.25);
        let r = (eval(&s, t) - q).length();
        let a = feet(q, spans(&[s]), r);
        let b = feet(q, spans(&[rev]), r);
        let ends = |f: &Feet| -> Vec<[f64; 2]> {
            f.feet
                .iter()
                .flat_map(|f| [f.lo, f.hi])
                .filter(|e| e.kind == EndKind::Crossing)
                .map(|e| e.t)
                .collect()
        };
        let (ea, mut eb) = (ends(&a), ends(&b));
        eb.reverse();
        assert_eq!(ea.len(), eb.len(), "{a:?} {b:?}");
        for (x, y) in ea.iter().zip(&eb) {
            let m = crate::records::ray::mirrored(*y);
            assert!(x[0] <= m[1] && m[0] <= x[1], "{x:?} against {m:?}");
        }
    }
}
