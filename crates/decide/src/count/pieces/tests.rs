//! Strip pieces on a plane against the exact winding: a face over two
//! tiles mapped affinely onto the plane `z = 0`, one trimming line
//! across both (its p-curve broken at the tile line), the edge's curve
//! beside it, a ribbon's width away; runs of strip boxes (and a box
//! inside the trim) on either side of the tile line, the query point's
//! shadow drawn at random. The piece's degree is `±1` where the shadow
//! is inside the run's part of the face or the ribbon, else 0; runs of
//! any partition sum to the whole; flipped faces, coedges against their
//! edge, a slit and a run ending at the tile line (a cut at the knot). On
//! a curved surface with a curved p-curve and edge curve, runs of any
//! partition sum to the whole run.

use varde_testkit::rng::Rng;

use super::*;
use crate::bezier::Dyadic;
use crate::count::boxes::UvSpan;
use crate::ids::{FaceId, VertId};
use crate::num::decide::Sign;
use crate::records::keys::{Operand, Query};
use crate::records::ray::{ChainCrossing, Crossing};

type Patch = HomPatch<Bounded<f64>>;

const TILES: [std::ops::Range<i64>; 2] = [0..2, 0..1];
/// The surface: `(u, v) ↦ (5u, 10v, 0)`.
const SU: f64 = 5.0;
const SV: f64 = 10.0;
/// The trim's `v`, and its ends' `u`.
const V0: f64 = 0.25;
const U0: f64 = 0.1;
const U1: f64 = 1.9;
/// The ribbon's width.
const DELTA: f64 = 0.01;

fn first() -> Bounded<f64> {
    Bounded::exact(0.0)
}

fn patches() -> Vec<Patch> {
    let mut out = Vec::new();
    for i in TILES[0].clone() {
        for j in TILES[1].clone() {
            let mut net = Vec::new();
            for a in 0..3 {
                for b in 0..3 {
                    let (u, v) = (i as f64 + a as f64 / 2.0, j as f64 + b as f64 / 2.0);
                    net.push([SU * u, SV * v, 0.0, 1.0]);
                }
            }
            out.push(Patch::from_affine(2, 2, net, &first()).unwrap());
        }
    }
    out
}

/// The trim's p-curve: two straight spans, broken at `u = 1`.
fn pspans() -> Vec<UvSpan> {
    let line = |a: f64, b: f64| [[a, V0, 1.0], [(a + b) / 2.0, V0, 1.0], [b, V0, 1.0]];
    vec![line(U0, 1.0), line(1.0, U1)]
}

/// The edge's curve: one straight span beside `S(p)`, `dy` off it.
fn curve(dy: f64) -> Vec<Span> {
    let y = SV * V0 + dy;
    vec![[
        [SU * U0, y, 0.0, 1.0],
        [SU * (U0 + U1) / 2.0, y, 0.0, 1.0],
        [SU * U1, y, 0.0, 1.0],
    ]]
}

/// The p-spans' places on the edge's one span: `u = U0 + 1.8 t`.
fn maps() -> Vec<SpanMap> {
    vec![
        SpanMap {
            span: 0,
            t: [0.0, 0.5],
        },
        SpanMap {
            span: 0,
            t: [0.5, 1.0],
        },
    ]
}

fn trim(spans: &[UvSpan], coedge: u32, reversed: bool) -> Trim<'_> {
    Trim {
        coedge: CoedgeId(coedge),
        reversed,
        spans,
        ends: [VertId(1), VertId(2)],
        zero_length: false,
        band: 0.0,
    }
}

fn point() -> Point {
    Point {
        operand: Operand::B,
        q: Query::Vert(VertId(99)),
    }
}

/// The ray record of a query at `q` (of `B`, its ray `+r̂`) against the
/// edge's curve `c`, polynomial quadratic spans: per span the roots of
/// `ν·(C(t) − q)`, a quadratic, in closed form.
fn rays(c: &[Span], q: [f64; 3]) -> ChainRec {
    let dot = |a: [f64; 3], p: [f64; 4]| a[0] * p[0] + a[1] * p[1] + a[2] * p[2];
    let (r, n) = ([R_HAT.x, R_HAT.y, R_HAT.z], [NU.x, NU.y, NU.z]);
    let q4 = [q[0], q[1], q[2], 1.0];
    let mut out = ChainRec::default();
    for (k, sp) in c.iter().enumerate() {
        let f = |a: [f64; 3]| {
            let [p0, p1, p2] = sp.map(|p| dot(a, p));
            // `p0 + 2t(p1 − p0) + t²(p0 − 2p1 + p2)`.
            [p0, 2.0 * (p1 - p0), p0 - 2.0 * p1 + p2]
        };
        let [c0, b, a] = f(n);
        let c0 = c0 - dot(n, q4);
        let mut ts: Vec<f64> = Vec::new();
        if a == 0.0 {
            ts.push(-c0 / b);
        } else {
            let disc = b * b - 4.0 * a * c0;
            if disc > 0.0 {
                let s = disc.sqrt();
                ts.extend([(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)]);
            }
        }
        ts.sort_by(f64::total_cmp);
        let [rc, rb, ra] = f(r);
        for t in ts {
            if !(0.0..=1.0).contains(&t) || rc + rb * t + ra * t * t <= dot(r, q4) {
                continue;
            }
            // `ρ × T` in the shadow plane, `ρ = (1, 0)`: the tangent's `ν`
            // part.
            let sign = if b + 2.0 * a * t > 0.0 {
                Sign::Pos
            } else {
                Sign::Neg
            };
            out.crossings.push(ChainCrossing {
                span: k as u32,
                crossing: Crossing {
                    t: [t - 1e-12, t + 1e-12],
                    sign,
                },
            });
        }
    }
    out
}

fn ext(a: (u64, u32), b: (u64, u32)) -> Extent {
    Extent::new(
        Dyadic::new(a.0, a.1).unwrap(),
        Dyadic::new(b.0, b.1).unwrap(),
    )
    .unwrap()
}

fn bx(i: i64, u: Extent, v: Extent) -> BoxKey {
    BoxKey::new(Operand::A, FaceId(3), [i, 0], u, v)
}

/// The run: `[½, 1] × [0, ½]` of tile 0 in two boxes, `[0, ½] × [0, ½]`
/// of tile 1 in two, one of them cut at `v = ⅜` (its top a box inside
/// the trim), so the outer boundary has T-junctions and crosses the tile
/// line.
fn run() -> Vec<BoxKey> {
    let low = ext((0, 0), (1, 1));
    vec![
        bx(0, ext((1, 1), (3, 2)), low),
        bx(0, ext((3, 2), (1, 0)), low),
        bx(1, ext((0, 0), (1, 2)), ext((0, 0), (3, 3))),
        bx(1, ext((0, 0), (1, 2)), ext((3, 3), (1, 1))),
        bx(1, ext((1, 2), (1, 1)), low),
    ]
}

/// Whether `(x, y)` is inside the region `[x0, x1] × [y0, y1]`, `None`
/// within `m` of its boundary.
fn inside(p: [f64; 2], [x0, x1, y0, y1]: [f64; 4], m: f64) -> Option<bool> {
    let d = [p[0] - x0, x1 - p[0], p[1] - y0, y1 - p[1]];
    if d.iter().any(|x| x.abs() < m) {
        None
    } else {
        Some(d.iter().all(|&x| x > 0.0))
    }
}

struct Setup {
    spans: Vec<UvSpan>,
    curve: Vec<Span>,
    maps: Vec<SpanMap>,
    patches: Vec<Patch>,
}

impl Setup {
    fn new(dy: f64) -> Setup {
        Setup {
            spans: pspans(),
            curve: curve(dy),
            maps: maps(),
            patches: patches(),
        }
    }

    /// The curved face: `(u, v) ↦ (5u, 10v, K(u − 1)² + Lv)`, its trim a
    /// p-curve bulging up over tile 0 and down over tile 1 (broken at the
    /// tile line), the edge's curve per p-span the quadratic through the
    /// surface's points over the p-span's control points, `DELTA` below.
    fn curved() -> Setup {
        const K: f64 = 0.75;
        const L: f64 = 0.5;
        let s = |u: f64, v: f64| [SU * u, SV * v, K * (u - 1.0) * (u - 1.0) + L * v];
        // `K(u − 1)²` over each tile as a quadratic: its control values.
        let zu = |i: i64| if i == 0 { [K, 0.0, 0.0] } else { [0.0, 0.0, K] };
        let mut patches = Vec::new();
        for i in TILES[0].clone() {
            for j in TILES[1].clone() {
                let mut net = Vec::new();
                for a in 0..3 {
                    for b in 0..3 {
                        let (u, v) = (i as f64 + a as f64 / 2.0, j as f64 + b as f64 / 2.0);
                        net.push([SU * u, SV * v, zu(i)[a] + L * v, 1.0]);
                    }
                }
                patches.push(Patch::from_affine(2, 2, net, &first()).unwrap());
            }
        }
        let spans: Vec<UvSpan> = vec![
            [[U0, V0, 1.0], [0.55, V0 + 0.1, 1.0], [1.0, V0, 1.0]],
            [[1.0, V0, 1.0], [1.45, V0 - 0.1, 1.0], [U1, V0, 1.0]],
        ];
        let curve = spans
            .iter()
            .map(|sp| {
                sp.map(|p| {
                    let x = s(p[0], p[1]);
                    [x[0], x[1] - DELTA, x[2], 1.0]
                })
            })
            .collect();
        let maps = (0..2)
            .map(|k| SpanMap {
                span: k,
                t: [0.0, 1.0],
            })
            .collect();
        Setup {
            spans,
            curve,
            maps,
            patches,
        }
    }

    /// The degree of the run `boxes` of the face with `trims`, the
    /// ribbons of `coedges`, about `q`.
    fn degree(
        &self,
        trims: &[Trim<'_>],
        flipped: bool,
        coedges: &[u32],
        boxes: &[BoxKey],
        q: [f64; 3],
    ) -> Result<PieceDegree, PieceError> {
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], trims, flipped).unwrap();
        let rec = rays(&self.curve, q);
        let ribbons: Vec<Ribbon<'_>> = coedges
            .iter()
            .map(|&c| Ribbon {
                coedge: CoedgeId(c),
                curve: &self.curve,
                map: &self.maps,
                rays: &rec,
            })
            .collect();
        strip_degree(
            &face,
            &self.patches,
            boxes,
            &ribbons,
            &QShadow::of(q).unwrap(),
            point(),
        )
    }
}

/// A random query point over the run and round it, above the plane.
fn draw(rng: &mut Rng) -> [f64; 3] {
    [
        rng.range(1.0, 9.0),
        rng.range(0.0, 6.0),
        rng.range(-1.0, 1.0),
    ]
}

/// The query's position in the plane `z = 0` as its shadow sees it: the
/// point of the plane with the same shadow.
fn plane_xy(q: [f64; 3]) -> [f64; 2] {
    // Solve `r̂·x = r̂·q`, `ν·x = ν·q` for `x = (a, b, 0)`.
    let rq = R_HAT.x * q[0] + R_HAT.y * q[1] + R_HAT.z * q[2];
    let nq = NU.x * q[0] + NU.y * q[1] + NU.z * q[2];
    let a = rq / R_HAT.x;
    let b = (nq - NU.x * a) / NU.y;
    [a, b]
}

/// Checks 300 random queries: every certified degree is the exact
/// one, and most are certified.
#[allow(clippy::too_many_arguments)]
fn check(
    s: &Setup,
    trims: &[Trim<'_>],
    flipped: bool,
    coedges: &[u32],
    parts: &[Vec<BoxKey>],
    region: [f64; 4],
    sign: i64,
    seed: u64,
) {
    let mut rng = Rng::new(seed);
    let (mut ok, mut all) = (0, 0);
    for _ in 0..300 {
        let q = draw(&mut rng);
        let Some(truth) = inside(plane_xy(q), region, 1e-3) else {
            continue;
        };
        all += 1;
        let mut sum = 0;
        let mut settled = true;
        for p in parts {
            match s.degree(trims, flipped, coedges, p, q) {
                Ok(d) => sum += d.degree,
                Err(PieceError::Unsettled | PieceError::Through) => settled = false,
                Err(e) => panic!("{e:?} at {q:?}"),
            }
        }
        if settled {
            ok += 1;
            assert_eq!(sum, if truth { sign } else { 0 }, "q = {q:?}");
        }
    }
    assert!(ok * 10 >= all * 9, "{ok} of {all} settled");
}

/// The run's region in the plane: the face's part above the trim and the
/// ribbon below it.
const WHOLE: [f64; 4] = [2.5, 7.5, SV * V0 - DELTA, 5.0];

#[test]
fn degree_is_the_winding() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    check(&s, &trims, false, &[1], &[run()], WHOLE, 1, 1);
}

#[test]
fn runs_of_any_partition_sum_to_the_whole() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let r = run();
    let parts = [
        vec![vec![r[0]], vec![r[1]], vec![r[2], r[3]], vec![r[4]]],
        vec![vec![r[0], r[1]], r[2..].to_vec()],
        vec![vec![r[0], r[2], r[3]], vec![r[1], r[4]]],
        // The box inside the trim alone: no cut, a loop the curve misses.
        vec![r[..3].to_vec(), r[4..].to_vec(), vec![r[3]]],
    ];
    for (k, p) in parts.iter().enumerate() {
        let mut rng = Rng::new(100 + k as u64);
        let (mut ok, mut all) = (0, 0);
        for _ in 0..100 {
            let q = draw(&mut rng);
            let Some(truth) = inside(plane_xy(q), WHOLE, 1e-3) else {
                continue;
            };
            all += 1;
            let whole = s.degree(&trims, false, &[1], &r, q);
            let mut sum = Some(0);
            for part in p {
                match s.degree(&trims, false, &[1], part, q) {
                    Ok(d) => sum = sum.map(|x| x + d.degree),
                    Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                    Err(e) => panic!("{e:?} partition {k} q {q:?}"),
                }
            }
            if let (Some(sum), Ok(w)) = (sum, whole) {
                ok += 1;
                assert_eq!(sum, w.degree, "partition {k}, q = {q:?}");
                assert_eq!(sum, i64::from(truth), "partition {k}, q = {q:?}");
            }
        }
        // The box inside the trim alone has a loop no cut settles.
        if k < 3 {
            assert!(ok * 10 >= all * 8, "partition {k}: {ok} of {all}");
        }
    }
}

#[test]
fn a_box_inside_the_trim_alone_is_unsettled() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let r = run();
    assert_eq!(
        s.degree(&trims, false, &[1], &[r[3]], [5.0, 4.0, 0.0]),
        Err(PieceError::Unsettled)
    );
}

#[test]
fn flipped_face_with_its_coedge_against_the_edge() {
    // The trim runs `−u` against its edge: the face is still above it,
    // its boundary clockwise in UV.
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, true)];
    check(&s, &trims, true, &[1], &[run()], WHOLE, -1, 2);
}

#[test]
fn face_below_the_trim() {
    // Against its edge on an unflipped face: the face is below the trim,
    // the ribbon above it.
    let s = Setup::new(DELTA);
    let trims = [trim(&s.spans, 1, true)];
    let region = [2.5, 7.5, 0.0, SV * V0 + DELTA];
    check(&s, &trims, false, &[1], &[run()], region, 1, 3);
}

#[test]
fn slit_ribbons_cancel() {
    // Two coedges of one edge share the p-curve: the face on both sides.
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false), trim(&s.spans, 2, true)];
    let region = [2.5, 7.5, 0.0, 5.0];
    check(&s, &trims, false, &[1, 2], &[run()], region, 1, 4);
}

#[test]
fn a_cut_at_the_knot_on_the_tile_line() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let r = run();
    let left = vec![r[0], r[1]];
    let region = [2.5, 5.0, SV * V0 - DELTA, 5.0];
    check(
        &s,
        &trims,
        false,
        &[1],
        std::slice::from_ref(&left),
        region,
        1,
        5,
    );
    // The two halves of the run meet at the knot: the sum is the whole.
    let right = r[2..].to_vec();
    check(&s, &trims, false, &[1], &[left, right], WHOLE, 1, 6);
}

#[test]
fn far_queries_have_degree_zero() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    for q in [
        [20.0, 3.0, 0.0],
        [5.0, 20.0, 0.0],
        [5.0, -9.0, 0.0],
        [-9.0, 2.5, 0.0],
    ] {
        let d = s.degree(&trims, false, &[1], &run(), q).unwrap();
        assert_eq!(d.degree, 0, "{q:?}");
        assert_eq!(d.cuts, 2);
    }
}

#[test]
fn refuses_what_isnt_a_strip_piece() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let q = [5.0, 4.0, 0.0];
    assert_eq!(
        s.degree(&trims, false, &[1], &[], q),
        Err(PieceError::Boxes)
    );
    let r = run();
    assert_eq!(
        s.degree(&trims, false, &[1], &[r[0], r[0]], q),
        Err(PieceError::Boxes)
    );
    let big = bx(0, ext((0, 0), (1, 0)), ext((0, 0), (1, 1)));
    assert_eq!(
        s.degree(&trims, false, &[1], &[big, r[0]], q),
        Err(PieceError::Boxes)
    );
    assert_eq!(
        s.degree(&trims, false, &[5], &r, q),
        Err(PieceError::Ribbon {
            coedge: CoedgeId(5)
        })
    );
    assert_eq!(
        s.degree(&trims, false, &[1, 1], &r, q),
        Err(PieceError::Ribbon {
            coedge: CoedgeId(1)
        })
    );
    // Boxes meeting at a corner only.
    let low = ext((0, 0), (1, 2));
    let high = ext((1, 2), (1, 0));
    let pinch = [bx(0, low, low), bx(0, high, high)];
    assert_eq!(
        s.degree(&trims, false, &[1], &pinch, q),
        Err(PieceError::Pinch)
    );
    // A box of the query's own operand.
    let foreign = [BoxKey::new(Operand::B, FaceId(3), [0, 0], low, low)];
    assert_eq!(
        s.degree(&trims, false, &[1], &foreign, q),
        Err(PieceError::Box(BoxError::Foreign))
    );
}

#[test]
fn a_query_on_the_cut_is_unsettled() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    // Right on the left cut's point, `S(p)` on the side `u = ½`.
    let q = [2.5, SV * V0, 0.0];
    assert_eq!(
        s.degree(&trims, false, &[1], &run(), q),
        Err(PieceError::Unsettled)
    );
    // The ray crosses the ribbon's radial at the right cut, `x = 7.5`.
    let q = [6.5, SV * V0 - 0.015, 0.0];
    assert_eq!(
        s.degree(&trims, false, &[1], &run(), q),
        Err(PieceError::Unsettled)
    );
}

#[test]
fn queries_in_and_beside_the_ribbon() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let y = SV * V0;
    for (dy, want, ribbon) in [(0.005, 1, 0), (-0.005, 1, 0), (-0.015, 0, -1), (0.5, 1, 0)] {
        for x in [3.0, 4.0, 4.5] {
            let d = s
                .degree(&trims, false, &[1], &run(), [x, y + dy, 0.0])
                .unwrap();
            assert_eq!(d.degree, want, "({x}, {dy})");
            // The ray `+r̂` rises `3/514` per unit of `x`: from below the
            // edge's curve it crosses it inside the run, right to left
            // (`−1`), and leaves by the run's right side above the cut.
            assert_eq!(d.ribbon, ribbon, "({x}, {dy})");
        }
    }
}

#[test]
fn curved_runs_of_any_partition_sum_to_the_whole() {
    // A curved surface and curved p-curve: the parts' degrees, certified,
    // sum to the whole run's, which is 0 away from the run.
    let s = Setup::curved();
    let trims = [trim(&s.spans, 1, false)];
    let r = run();
    let parts = [
        vec![vec![r[0]], vec![r[1]], vec![r[2], r[3]], vec![r[4]]],
        vec![vec![r[0], r[1]], r[2..].to_vec()],
        vec![vec![r[0], r[2], r[3]], vec![r[1], r[4]]],
    ];
    let (mut ok, mut all, mut nonzero) = (0, 0, 0);
    for (k, p) in parts.iter().enumerate() {
        let mut rng = Rng::new(200 + k as u64);
        for _ in 0..100 {
            let q = draw(&mut rng);
            all += 1;
            let Ok(whole) = s.degree(&trims, false, &[1], &r, q) else {
                continue;
            };
            assert!((0..=1).contains(&whole.degree), "q = {q:?}");
            let mut sum = Some(0);
            for part in p {
                match s.degree(&trims, false, &[1], part, q) {
                    Ok(d) => sum = sum.map(|x| x + d.degree),
                    Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                    Err(e) => panic!("{e:?} partition {k} q {q:?}"),
                }
            }
            if let Some(sum) = sum {
                ok += 1;
                nonzero += whole.degree;
                assert_eq!(sum, whole.degree, "partition {k}, q = {q:?}");
            }
        }
    }
    assert!(ok * 10 >= all * 8, "{ok} of {all} settled");
    assert!(nonzero > 0, "no query inside the run");
    for q in [[20.0, 3.0, 0.0], [5.0, 20.0, 0.0], [-9.0, 2.5, 0.0]] {
        let d = s.degree(&trims, false, &[1], &run(), q).unwrap();
        assert_eq!(d.degree, 0, "{q:?}");
        assert_eq!(d.cuts, 2);
    }
}

#[test]
fn a_crossing_inside_a_window_is_unsettled() {
    let at = |t: f64| EdgePos { span: 0, t };
    let w = [at(0.4), at(0.6)];
    // The window holds no crossing: an enclosure inside it is a bad
    // record, not one past or before it.
    assert_eq!(past([at(0.45), at(0.55)], w), None);
    assert_eq!(past([at(0.4), at(0.6)], w), None);
    assert_eq!(past([at(0.3), at(0.7)], w), None);
    assert_eq!(past([at(0.5), at(0.7)], w), Some(true));
    assert_eq!(past([at(0.3), at(0.5)], w), Some(false));
    assert_eq!(past([at(0.1), at(0.2)], w), Some(false));
    assert_eq!(past([at(0.7), at(0.8)], w), Some(true));
    // Another span of the edge is wholly before or past.
    let other = |s: u32| [EdgePos { span: s, t: 0.5 }; 2];
    assert_eq!(past(other(1), w), Some(true));
    assert!(past([at(0.5), EdgePos { span: 1, t: 0.1 }], w).unwrap());
}

#[test]
fn box_order_and_uniform_weights_do_not_change_the_degree() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let r = run();
    let mut rev = r.clone();
    rev.reverse();
    let rot: Vec<BoxKey> = r[2..].iter().chain(&r[..2]).copied().collect();
    for wt in [1.0 / 64.0, 64.0] {
        let mut sw = Setup::new(-DELTA);
        sw.spans = s
            .spans
            .iter()
            .map(|sp| sp.map(|p| [p[0], p[1], wt]))
            .collect();
        sw.curve = s
            .curve
            .iter()
            .map(|sp| sp.map(|p| [p[0], p[1], p[2], wt]))
            .collect();
        let mut net = Vec::new();
        for i in TILES[0].clone() {
            for a in 0..3 {
                for b in 0..3 {
                    let (u, v) = (i as f64 + a as f64 / 2.0, b as f64 / 2.0);
                    net.push([SU * u, SV * v, 0.0, wt]);
                }
            }
        }
        sw.patches = net
            .chunks(9)
            .map(|c| Patch::from_affine(2, 2, c.iter().copied(), &first()).unwrap())
            .collect();
        let tw = [trim(&sw.spans, 1, false)];
        let mut rng = Rng::new(7);
        for _ in 0..60 {
            let q = draw(&mut rng);
            let want = s.degree(&trims, false, &[1], &r, q).map(|d| d.degree);
            for b in [&r, &rev, &rot] {
                assert_eq!(s.degree(&trims, false, &[1], b, q).map(|d| d.degree), want);
                if want.is_ok() {
                    let got = sw.degree(&tw, false, &[1], b, q).map(|d| d.degree);
                    assert!(
                        got.is_err() || got == want,
                        "{wt} {q:?}: {got:?} vs {want:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn queries_on_tile_and_box_lines() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let r = run();
    let parts = [r[..2].to_vec(), r[2..].to_vec()];
    // `x = 5` the tile line, `2.5`, `3.75`, `6.25`, `7.5` box sides,
    // `y = 3.75` the cut box's top, `5` the run's top.
    for x in [2.5, 3.75, 5.0, 6.25, 7.5] {
        for y in [1.0, 2.0, 3.0, 3.75, 4.5, 5.0, 6.0] {
            let q = [x, y, 0.0];
            let truth = inside([x, y], WHOLE, 1e-9);
            let whole = s.degree(&trims, false, &[1], &r, q);
            let mut sum = Some(0);
            for p in &parts {
                match s.degree(&trims, false, &[1], p, q) {
                    Ok(d) => sum = sum.map(|a| a + d.degree),
                    Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                    Err(e) => panic!("{e:?} at ({x}, {y})"),
                }
            }
            match whole {
                Ok(w) => {
                    if let Some(t) = truth {
                        assert_eq!(w.degree, i64::from(t), "({x}, {y})");
                    }
                    if let Some(sum) = sum {
                        assert_eq!(sum, w.degree, "({x}, {y})");
                    }
                }
                Err(PieceError::Unsettled | PieceError::Through) => (),
                Err(e) => panic!("{e:?} at ({x}, {y})"),
            }
        }
    }
}
