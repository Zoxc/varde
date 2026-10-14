//! Tubes, strips, slits and corners on a tilted plane: tube membership
//! against the exact distance of a box's corners to a segment; corners of
//! every angle class (convex, reflex, acute, cusp, with a zero-length
//! edge) refined round the vertex: no box is left thin, the boxes meeting
//! both curves anywhere along an acute or cusp spike are the vertex's
//! corner boxes, and each run lies within the band of the other curve, by
//! the exact distance where it is rational, and reaches most of the way
//! the true component does; a slit's boxes are strips of both its
//! coedges; a box's role is the same whether it tests all spans or only
//! those that met its parent. Corners at the least band (edges of
//! tolerance 0), a lone curve with a zero-length edge, a pinch of four
//! curves and a near-360° notch leave no box thin; curves that come near
//! away from their corner, or a hole near the outer loop, are no
//! corner's.

#![allow(
    clippy::disallowed_methods,
    reason = "tests build angles with std's trigonometry and std's max, an independent reference"
)]

use std::collections::VecDeque;

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode;
use varde_testkit::rng::Rng;

use super::super::{Extent, MAX_BOX_DEPTH, Operand, UvSpan, split};
use super::*;
use crate::ids::FaceId;

type Patch = HomPatch<Bounded<f64>>;

const TILES: [std::ops::Range<i64>; 2] = [0..3, 0..2];

fn first() -> Bounded<f64> {
    Bounded::exact(0.0)
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// The tilted plane `(u, v) ↦ (u, v, u/4 + v/2)`, exactly.
fn map(u: f64, v: f64) -> [f64; 3] {
    [u, v, u * 0.25 + v * 0.5]
}

fn map_q(p: &[Q; 2]) -> [Q; 3] {
    let z = &p[0] / Q::from(4) + &p[1] / Q::from(2);
    [p[0].clone(), p[1].clone(), z]
}

/// The tiles' patches of the plane, `u` index first.
fn plane() -> Vec<Patch> {
    let mut out = Vec::new();
    for i in TILES[0].clone() {
        for j in TILES[1].clone() {
            let mut net = Vec::new();
            for a in 0..2 {
                for b in 0..2 {
                    let [x, y, z] = map((i + a) as f64, (j + b) as f64);
                    net.push([x, y, z, 1.0]);
                }
            }
            out.push(Patch::from_affine(1, 1, net, &first()).unwrap());
        }
    }
    out
}

fn line(a: [f64; 2], b: [f64; 2]) -> UvSpan {
    let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    assert!(
        m[0] * 2.0 == a[0] + b[0] && m[1] * 2.0 == a[1] + b[1],
        "exact middle"
    );
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

/// A multiple of `2⁻³⁰` near `x`.
fn snap(x: f64) -> f64 {
    (x * 1_073_741_824.0).round() / 1_073_741_824.0
}

fn sq(x: &Q) -> Q {
    x * x
}

/// The squared distance from `p` to the segment `ab`, exactly.
fn seg_dist2(p: &[Q; 3], a: &[Q; 3], b: &[Q; 3]) -> Q {
    let d: Vec<Q> = (0..3).map(|k| &b[k] - &a[k]).collect();
    let w: Vec<Q> = (0..3).map(|k| &p[k] - &a[k]).collect();
    let dd: Q = d.iter().map(sq).sum();
    let wd: Q = (0..3).map(|k| &w[k] * &d[k]).sum();
    let t = if wd <= 0 {
        Q::from(0)
    } else if wd >= dd {
        Q::from(1)
    } else {
        wd / dd
    };
    (0..3).map(|k| sq(&(&w[k] - &t * &d[k]))).sum()
}

/// The point of a span at `t`, exactly.
fn span_at(s: &UvSpan, t: &Q) -> [Q; 2] {
    let one_t = Q::from(1) - t;
    let b = [sq(&one_t), Q::from(2) * &one_t * t, sq(t)];
    let w: Q = (0..3).map(|i| &b[i] * q(s[i][2])).sum();
    std::array::from_fn(|k| (0..3).map(|i| &b[i] * q(s[i][2]) * q(s[i][k])).sum::<Q>() / &w)
}

fn to_f(p: &[Q; 2]) -> [f64; 2] {
    p.each_ref()
        .map(|x| f64::rounding_from(x, RoundingMode::Nearest).0)
}

/// The point of a span at `t`, in floats.
fn span_at_f(s: &UvSpan, t: f64) -> [f64; 2] {
    let b = [(1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t, t * t];
    let w: f64 = (0..3).map(|i| b[i] * s[i][2]).sum();
    std::array::from_fn(|k| (0..3).map(|i| b[i] * s[i][2] * s[i][k]).sum::<f64>() / w)
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// One edge of a test loop: its start vertex, its spans along the loop,
/// whether it has length zero, whether stored against the loop.
#[derive(Clone)]
struct Edge {
    start: VertId,
    spans: Vec<UvSpan>,
    zero: bool,
    stored_back: bool,
}

fn edge(start: u32, spans: Vec<UvSpan>) -> Edge {
    Edge {
        start: VertId(start),
        spans,
        zero: false,
        stored_back: false,
    }
}

/// A test face: per coedge (ids `0, 1, …` in loop order) the stored
/// spans, whether reversed, the stored ends and length zero.
struct Built {
    coedges: Vec<(Vec<UvSpan>, bool, [VertId; 2], bool)>,
    band: f64,
}

impl Built {
    fn new(loops: &[Vec<Edge>], band: f64) -> Built {
        let mut coedges = Vec::new();
        for l in loops {
            for (k, e) in l.iter().enumerate() {
                let end = l[(k + 1) % l.len()].start;
                if e.stored_back {
                    let back: Vec<UvSpan> =
                        e.spans.iter().rev().map(|s| [s[2], s[1], s[0]]).collect();
                    coedges.push((back, true, [end, e.start], e.zero));
                } else {
                    coedges.push((e.spans.clone(), false, [e.start, end], e.zero));
                }
            }
        }
        Built { coedges, band }
    }

    fn trims(&self) -> Vec<Trim<'_>> {
        self.coedges
            .iter()
            .enumerate()
            .map(|(k, (spans, reversed, ends, zero))| Trim {
                coedge: CoedgeId(k as u32),
                reversed: *reversed,
                spans,
                ends: *ends,
                zero_length: *zero,
                band: self.band,
            })
            .collect()
    }
}

fn face_uv<'a>(trims: &'a [Trim<'a>]) -> FaceUv<'a> {
    FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], trims, false).unwrap()
}

/// The face's vertices in the geometric order: by their points on the
/// plane, `x` then `y` (`u` then `v`), ids last.
fn geo(face: &FaceUv<'_>) -> Vec<VertId> {
    let mut pts: Vec<([f64; 2], VertId)> = Vec::new();
    for t in face.trims() {
        let n = t.spans.len();
        for (v, p) in [(t.ends[0], t.spans[0][0]), (t.ends[1], t.spans[n - 1][2])] {
            pts.push(([p[0], p[1]], v));
        }
    }
    pts.sort_by(|a, b| {
        a.0[0]
            .total_cmp(&b.0[0])
            .then(a.0[1].total_cmp(&b.0[1]))
            .then(a.1.cmp(&b.1))
    });
    let mut out: Vec<VertId> = Vec::new();
    for (_, v) in pts {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// The box's UV rectangle `[[u_lo, u_hi], [v_lo, v_hi]]`.
fn rect(b: &BoxKey) -> [[f64; 2]; 2] {
    b.sides(&first())
        .map(|[lo, hi]| [lo.enclose()[0], hi.enclose()[0]])
}

/// The UV distance from `p` to the box (0 inside).
fn box_dist(b: &BoxKey, p: [f64; 2]) -> f64 {
    let r = rect(b);
    let g = |k: usize| {
        if p[k] < r[k][0] {
            r[k][0] - p[k]
        } else if p[k] > r[k][1] {
            p[k] - r[k][1]
        } else {
            0.0
        }
    };
    (g(0) * g(0) + g(1) * g(1)).sqrt()
}

/// Refines from the tiles while a box is in `region`: thin boxes split,
/// strip and corner boxes split until their hull is within the band;
/// every box's role is also asked of all spans, which must agree with
/// the role from its parent's spans. The leaves in the region.
fn refine(
    face: &FaceUv<'_>,
    corners: &Corners,
    ps: &[Patch],
    beta: f64,
    region: &dyn Fn(&BoxKey) -> bool,
    depth: u32,
) -> Vec<(BoxKey, Role)> {
    let mut queue: VecDeque<(BoxKey, Option<SmallVec<[SpanRef; 8]>>)> =
        face.tile_boxes().map(|b| (b, None)).collect();
    let mut leaves = Vec::new();
    while let Some((b, among)) = queue.pop_front() {
        if !region(&b) {
            continue;
        }
        let r = role(face, corners, &b, &first(), among.as_deref()).expect("role");
        if among.is_some() {
            let all = role(face, corners, &b, &first(), None).expect("role");
            assert_eq!((&all.role, &all.met), (&r.role, &r.met), "box {b:?}");
        }
        let patch = face.patch_of(ps, b.tile()).unwrap();
        let more = match &r.role {
            Role::Thin(_) => true,
            Role::Strip(_) | Role::Corner { .. } => {
                !within_band(&box_hull(patch, &b).expect("positive weights"), beta)
            }
            Role::Core | Role::Outside => false,
        };
        if more && b.depth() < depth {
            let s = split(face, &b, patch, None).expect("split");
            for h in s.halves {
                queue.push_back((h, Some(r.met.clone())));
            }
        } else {
            leaves.push((b, r.role));
        }
        assert!(leaves.len() + queue.len() < 100_000, "refinement runs away");
    }
    leaves
}

#[test]
fn tube_membership_holds_exactly() {
    let ps = plane();
    let (mut certified, mut wanted) = (0, 0);
    for seed in varde_testing::seeds(8, 64) {
        let mut rng = Rng::new(seed);
        let pt = |rng: &mut Rng| [snap(rng.range(0.2, 2.8)), snap(rng.range(0.2, 1.8))];
        let (a, b) = (pt(&mut rng), pt(&mut rng));
        let beta = rng.log_range(0.002, 0.05);
        let spans = vec![line(a, b)];
        let built = Built {
            coedges: vec![(spans, false, [VertId(0), VertId(1)], false)],
            band: beta,
        };
        let trims = built.trims();
        let face = face_uv(&trims);
        let (a3, b3) = (map_q(&[q(a[0]), q(a[1])]), map_q(&[q(b[0]), q(b[1])]));
        for _ in 0..150 {
            let t = rng.unit();
            let off = [rng.range(-2.0, 2.0) * beta, rng.range(-2.0, 2.0) * beta];
            let p = [
                a[0] + t * (b[0] - a[0]) + off[0],
                a[1] + t * (b[1] - a[1]) + off[1],
            ];
            if !(0.0..3.0).contains(&p[0]) || !(0.0..2.0).contains(&p[1]) {
                continue;
            }
            let size = beta * rng.log_range(1.0 / 32.0, 2.0);
            let mut d = 1;
            while 1.0 / (1u64 << d) as f64 > size {
                d += 1;
            }
            let tile = [p[0].floor() as i64, p[1].floor() as i64];
            let ext = |x: f64| {
                let k = (x * (1u64 << d) as f64).floor() as u64;
                Extent::new(Dyadic::new(k, d).unwrap(), Dyadic::new(k + 1, d).unwrap()).unwrap()
            };
            let bx = BoxKey::new(
                Operand::A,
                FaceId(3),
                tile,
                ext(p[0] - tile[0] as f64),
                ext(p[1] - tile[1] as f64),
            );
            let got = in_tube(&face, &ps, &bx, CoedgeId(0)).expect("tube");
            // The exact greatest distance of the box from the segment: at a
            // corner, the distance to a convex set being convex.
            let s = bx.sides(&Q::from(0));
            let corners: Vec<[Q; 3]> = [0, 1]
                .iter()
                .flat_map(|&i| [0, 1].map(|j| map_q(&[s[0][i].clone(), s[1][j].clone()])))
                .collect();
            let far = corners
                .iter()
                .map(|c| seg_dist2(c, &a3, &b3))
                .max()
                .unwrap();
            let diam2 = sq(&(&s[0][1] - &s[0][0])) + sq(&(&s[1][1] - &s[1][0]));
            // The plane stretches lengths by at most `√(1 + 1/16 + 1/4) < 1.15`.
            let small = diam2 * Q::try_from(1.32).unwrap() <= sq(&q(beta / 8.0));
            if got.inside {
                certified += 1;
                assert!(far <= sq(&q(beta)), "box {bx:?} certified in the tube");
            }
            if small && far <= sq(&q(beta / 4.0)) {
                wanted += 1;
                assert!(got.inside, "box {bx:?} deep in the tube, not certified");
            }
        }
    }
    assert!(certified > 100 && wanted > 20, "{certified} {wanted}");
}

/// A corner case: the loops, the corner's vertex, the curve into it and
/// the curve out, the band.
struct Case {
    loops: Vec<Vec<Edge>>,
    /// The vertex and its point.
    w: VertId,
    at: [f64; 2],
    into: CoedgeId,
    out: CoedgeId,
    beta: f64,
}

const W: [f64; 2] = [1.0, 0.5];
const BETA: f64 = 1.0 / 128.0;

/// A triangle with angle `deg` at `W`, the curve into `W` coedge 2.
fn wedge(deg: f64) -> Case {
    let l = 0.75;
    let r = deg.to_radians();
    let a = [W[0] + l, W[1]];
    let b = [snap(W[0] + l * r.cos()), snap(W[1] + l * r.sin())];
    Case {
        loops: vec![vec![
            edge(0, vec![line(W, a)]),
            edge(1, vec![line(a, b)]),
            edge(2, vec![line(b, W)]),
        ]],
        w: VertId(0),
        at: W,
        into: CoedgeId(2),
        out: CoedgeId(0),
        beta: BETA,
    }
}

/// An L-shape whose inner corner `(1, 1)` is reflex.
fn reflex() -> Case {
    let p = [
        [0.25, 0.25],
        [1.75, 0.25],
        [1.75, 1.0],
        [1.0, 1.0],
        [1.0, 1.75],
        [0.25, 1.75],
    ];
    Case {
        loops: vec![
            (0..6)
                .map(|k| edge(k as u32, vec![line(p[k], p[(k + 1) % 6])]))
                .collect(),
        ],
        w: VertId(3),
        at: [1.0, 1.0],
        into: CoedgeId(2),
        out: CoedgeId(3),
        beta: BETA,
    }
}

/// A line out of `W` along `+u` and an arc of radius 1 back into `W`
/// tangent to it: a cusp, its spike between them.
fn cusp() -> Case {
    let (s, t, c) = (0.8660254037844386, 0.5773502691896257, 0.8660254037844386);
    let a = [W[0] + s, W[1]];
    let p = [W[0] + s, W[1] + 0.5];
    let arc = [[p[0], p[1], 1.0], [W[0] + t, W[1], c], [W[0], W[1], 1.0]];
    Case {
        loops: vec![vec![
            edge(0, vec![line(W, a)]),
            edge(1, vec![line(a, p)]),
            edge(2, vec![arc]),
        ]],
        w: VertId(0),
        at: W,
        into: CoedgeId(2),
        out: CoedgeId(0),
        beta: BETA,
    }
}

/// The first 3D distance from `W` along `from`'s span (from its end at
/// `W`) where it leaves the band of `to`'s span, both sampled, and the
/// parameter there.
fn stretch(from: &UvSpan, at_end: bool, to: &UvSpan, beta: f64, w: [f64; 2]) -> f64 {
    let pt = span_at_f;
    let others: Vec<[f64; 3]> = (0..=5000)
        .map(|i| {
            let p = pt(to, i as f64 / 5000.0);
            map(p[0], p[1])
        })
        .collect();
    let w3 = map(w[0], w[1]);
    for i in 0..=2000 {
        let t = i as f64 / 2000.0;
        let p = pt(from, if at_end { 1.0 - t } else { t });
        let p3 = map(p[0], p[1]);
        let d = others
            .iter()
            .map(|o| dist3(*o, p3))
            .fold(f64::INFINITY, |m, x| if x < m { x } else { m });
        if d > beta {
            return dist3(p3, w3);
        }
    }
    f64::INFINITY
}

fn check_corner(case: Case, spiky: bool) {
    let ps = plane();
    let built = Built::new(&case.loops, case.beta);
    let trims = built.trims();
    let face = face_uv(&trims);
    let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
    let corner = corners.corner(case.w).expect("the corner");
    assert!(corner.curves.contains(&case.into) && corner.curves.contains(&case.out));
    let span_of = |c: CoedgeId| &trims[c.index()].spans[0];
    let (s_in, s_out) = (span_of(case.into), span_of(case.out));
    // The runs: within the band of the other curve (at a sharp corner,
    // whose runs reach the record's cut, within a few bands), and most of
    // the true component's way.
    let sharp = corners.cusp(corner.vertex).is_some();
    let tol = if sharp { 16.0 * case.beta } else { case.beta };
    let (mut reach, mut limit) = (0.0f64, 0.0f64);
    for (c, end, s, other) in [(case.into, 1u8, s_in, s_out), (case.out, 0u8, s_out, s_in)] {
        let (at_corner, run) = corners.run(&face, c, end).expect("a run");
        assert_eq!(at_corner.vertex, corner.vertex);
        let (span, at) = run.reach();
        assert_eq!(span, 0);
        let at = at.bounds()[0];
        let samples = (0..=64).map(|i| {
            let f = i as f64 / 64.0;
            if end == 1 {
                1.0 - f * (1.0 - at)
            } else {
                f * at
            }
        });
        for t in samples {
            let p = span_at(s, &q(t));
            let p3 = map_q(&p);
            if other[1][2] == 1.0 {
                // A straight other curve: exactly.
                let (a, b) = (other[0], other[2]);
                let d2 = seg_dist2(
                    &p3,
                    &map_q(&[q(a[0]), q(a[1])]),
                    &map_q(&[q(b[0]), q(b[1])]),
                );
                assert!(d2 <= sq(&q(tol)), "run of {c:?} leaves the band at {t}");
            } else {
                let f = to_f(&p);
                let p3 = map(f[0], f[1]);
                let d = (0..=50_000)
                    .map(|i| {
                        let o = span_at_f(other, i as f64 / 50_000.0);
                        dist3(map(o[0], o[1]), p3)
                    })
                    .fold(f64::INFINITY, |m, x| if x < m { x } else { m });
                assert!(d <= tol + 3e-5, "run of {c:?} leaves the band at {t}: {d}");
            }
        }
        let far = to_f(&span_at(s, &q(at)));
        let far = dist3(map(far[0], far[1]), map(case.at[0], case.at[1]));
        let want = stretch(s, end == 1, other, case.beta, case.at);
        assert!(
            far >= 0.5 * want,
            "run of {c:?} reaches {far}, the component {want}"
        );
        reach = reach.max(want);
        limit = limit.max(if sharp { far } else { want });
    }
    // Refined round the vertex: nothing thin, the boxes meeting both
    // curves the corner's, anywhere along a spike.
    let radius = 1.5 * reach + 4.0 * case.beta;
    let region = |b: &BoxKey| box_dist(b, case.at) <= radius;
    let leaves = refine(&face, &corners, &ps, case.beta, &region, 30);
    let mut farthest = 0.0f64;
    for (b, r) in &leaves {
        match r {
            Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
            Role::Corner { vertex, met } => {
                assert_eq!(*vertex, corner.vertex, "box {b:?}");
                assert!(
                    met.iter()
                        .all(|c| corner.curves.contains(c) || corner.zero.contains(c))
                );
                let d = box_dist(b, case.at);
                assert!(
                    d <= limit + case.beta,
                    "corner box {b:?} at {d}, the component {limit}"
                );
                farthest = farthest.max(d);
            }
            _ => {}
        }
    }
    assert!(farthest >= 0.0, "corner boxes found");
    if spiky {
        assert!(
            farthest >= 0.4 * reach,
            "the spike's boxes stop at {farthest} of {reach}"
        );
    }
}

#[test]
fn convex_corner() {
    check_corner(wedge(90.0), false);
}

#[test]
fn reflex_corner() {
    check_corner(reflex(), false);
}

#[test]
fn acute_corners_take_their_spike() {
    // At 20° the spike is about three bands long: the boxes meeting both
    // sides are the corner's, but the band-sized ones there are few.
    check_corner(wedge(20.0), false);
    check_corner(wedge(3.0), true);
}

#[test]
fn cusp_takes_its_spike() {
    check_corner(cusp(), true);
}

#[test]
fn zero_length_edges_belong_to_the_corner() {
    let mut case = wedge(60.0);
    // `W` split into vertices 0 and 3, joined by a zero-length edge.
    let w = [[W[0], W[1], 1.0]; 3];
    case.loops[0].push(Edge {
        start: VertId(3),
        spans: vec![w],
        zero: true,
        stored_back: false,
    });
    case.loops[0].rotate_right(1);
    // The loop now runs 3 → 0 (zero) → … → 2 → 3.
    let built = Built::new(&case.loops, case.beta);
    let trims = built.trims();
    let face = face_uv(&trims);
    let ps = plane();
    let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
    let corner = corners
        .corner(VertId(0))
        .expect("one corner of both vertices");
    assert_eq!(corner.vertices.as_slice(), &[VertId(0), VertId(3)]);
    assert_eq!(corner.zero.as_slice(), &[CoedgeId(0)]);
    assert_eq!(corner.curves.as_slice(), &[CoedgeId(1), CoedgeId(3)]);
    let region = |b: &BoxKey| box_dist(b, W) <= 0.1;
    let leaves = refine(&face, &corners, &ps, case.beta, &region, 30);
    let mut with_zero = 0;
    for (b, r) in &leaves {
        match r {
            Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
            Role::Corner { vertex, met } => {
                assert_eq!(*vertex, VertId(0));
                with_zero += usize::from(met.contains(&CoedgeId(0)));
            }
            Role::Strip(c) => assert!(!c.contains(&CoedgeId(0)), "box {b:?}"),
            _ => {}
        }
    }
    assert!(with_zero > 0);
}

#[test]
fn slits_are_strips_of_both_sides() {
    let ps = plane();
    for back in [false, true] {
        // A square with a slit from the middle of its lower side up to its
        // centre; vertex 1 is the slit's foot, passed twice.
        let (p0, p1, tip, p3, p4, p5) = (
            [0.5, 0.5],
            [1.0, 0.5],
            [1.0, 1.0],
            [1.5, 0.5],
            [1.5, 1.5],
            [0.5, 1.5],
        );
        let up = edge(1, vec![line(p1, tip)]);
        let mut down = edge(2, vec![line(tip, p1)]);
        down.stored_back = back;
        let loops = vec![vec![
            edge(0, vec![line(p0, p1)]),
            up,
            down,
            edge(1, vec![line(p1, p3)]),
            edge(3, vec![line(p3, p4)]),
            edge(4, vec![line(p4, p5)]),
            edge(5, vec![line(p5, p0)]),
        ]];
        let built = Built::new(&loops, BETA);
        let trims = built.trims();
        let face = face_uv(&trims);
        let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
        let region = |b: &BoxKey| box_dist(b, [1.0, 0.75]) <= 0.4;
        let leaves = refine(&face, &corners, &ps, BETA, &region, 30);
        let (mut strips, mut feet) = (0, 0);
        for (b, r) in &leaves {
            match r {
                Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
                Role::Strip(c) if c.contains(&CoedgeId(1)) || c.contains(&CoedgeId(2)) => {
                    assert_eq!(c.as_slice(), &[CoedgeId(1), CoedgeId(2)], "box {b:?}");
                    strips += 1;
                }
                Role::Corner { vertex, met } if *vertex == VertId(1) => {
                    feet += usize::from(met.len() == 4);
                }
                _ => {}
            }
        }
        assert!(strips > 20 && feet > 0, "{strips} {feet}");
    }
}

#[test]
fn bad_corners_input_is_refused() {
    let case = wedge(45.0);
    let built = Built::new(&case.loops, BETA);
    let trims = built.trims();
    let face = face_uv(&trims);
    let ps = plane();
    assert_eq!(
        Corners::new(&face, &ps[1..], &geo(&face)).err(),
        Some(BoxError::Patches)
    );
    let b = face.tile_boxes().next().unwrap();
    assert_eq!(
        in_tube(&face, &ps, &b, CoedgeId(9)).err(),
        Some(BoxError::Coedge {
            coedge: CoedgeId(9)
        })
    );
    // Corners of another face.
    let other = Built::new(&reflex().loops, BETA);
    let other_trims = other.trims();
    let other_face = face_uv(&other_trims);
    let corners = Corners::new(&other_face, &ps, &geo(&other_face)).unwrap();
    assert_eq!(
        role(&face, &corners, &b, &first(), None).err(),
        Some(BoxError::Foreign)
    );
    let mut bad = trims.clone();
    bad[0].band = f64::NAN;
    assert_eq!(
        FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &bad, false).err(),
        Some(BoxError::Band {
            coedge: CoedgeId(0)
        })
    );
    let mut twice = trims.clone();
    twice[1].coedge = CoedgeId(0);
    assert_eq!(
        FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &twice, false).err(),
        Some(BoxError::Coedge {
            coedge: CoedgeId(0)
        })
    );
}

/// The leaves round `case`'s vertex, out to eight bands, refined as far
/// as boxes go: the corner boxes and the thin ones.
fn round_vertex(case: &Case) -> (usize, Vec<BoxKey>) {
    let ps = plane();
    let built = Built::new(&case.loops, case.beta);
    let trims = built.trims();
    let face = face_uv(&trims);
    let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
    let r = 8.0 * case.beta;
    let region = |b: &BoxKey| box_dist(b, case.at) <= r;
    let leaves = refine(&face, &corners, &ps, case.beta, &region, MAX_BOX_DEPTH);
    let mut corner = 0;
    let mut thin = Vec::new();
    for (b, role) in leaves {
        match role {
            Role::Thin(_) => thin.push(b),
            Role::Corner { vertex, .. } => {
                assert_eq!(vertex, case.w, "box {b:?}");
                corner += 1;
            }
            _ => {}
        }
    }
    (corner, thin)
}

#[test]
fn corners_at_the_least_band() {
    // Edges of tolerance 0: the band is the operation's rounding bound
    // alone, at the plane's largest coordinate, 3. The boxes at the vertex
    // are then a few ulps across and their hulls wider than the boxes;
    // each meets the curves along pieces far finer than the runs' ends,
    // all of which must be found held without running out of pieces.
    // (A cusp's spike is then `√(2β)` long, millions of bands: not here.)
    let beta = crate::records::near::OpBands::new(3.0)
        .and_then(|b| b.band(0.0))
        .unwrap();
    for case in [wedge(90.0), wedge(20.0), wedge(3.0), reflex()] {
        let case = Case { beta, ..case };
        let (corner, thin) = round_vertex(&case);
        assert!(thin.is_empty(), "left thin: {thin:?}");
        assert!(corner > 0);
    }
}

/// The leaves round `at` out to `r`, refined to depth 40.
fn leaves_round(loops: &[Vec<Edge>], at: [f64; 2], r: f64) -> Vec<(BoxKey, Role)> {
    let ps = plane();
    let built = Built::new(loops, BETA);
    let trims = built.trims();
    let face = face_uv(&trims);
    let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
    let region = |b: &BoxKey| box_dist(b, at) <= r;
    refine(&face, &corners, &ps, BETA, &region, 40)
}

#[test]
fn a_lone_curve_and_a_zero_length_edge_are_a_corner() {
    // One closed edge from vertex 0 round to vertex 1 at the same point,
    // joined back by a zero-length edge: the boxes at the point meet the
    // curve's two ends and the zero-length edge.
    let (a, b) = ([1.75, 0.5], [1.0, 1.25]);
    let loops = vec![vec![
        edge(0, vec![line(W, a), line(a, b), line(b, W)]),
        Edge {
            start: VertId(1),
            spans: vec![[[W[0], W[1], 1.0]; 3]],
            zero: true,
            stored_back: false,
        },
    ]];
    let mut with_zero = 0;
    for (b, r) in leaves_round(&loops, W, 0.05) {
        match r {
            Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
            Role::Corner { vertex, met } => {
                assert_eq!(vertex, VertId(0));
                with_zero += usize::from(met.contains(&CoedgeId(1)));
            }
            _ => {}
        }
    }
    assert!(with_zero > 0);
}

#[test]
fn a_pinch_is_one_corner_of_four_curves() {
    // One loop passing `W` twice, a triangle on each side.
    let loops = vec![vec![
        edge(0, vec![line(W, [1.5, 0.25])]),
        edge(1, vec![line([1.5, 0.25], [1.5, 0.75])]),
        edge(2, vec![line([1.5, 0.75], W)]),
        edge(0, vec![line(W, [0.5, 0.75])]),
        edge(3, vec![line([0.5, 0.75], [0.5, 0.25])]),
        edge(4, vec![line([0.5, 0.25], W)]),
    ]];
    let mut four = 0;
    for (b, r) in leaves_round(&loops, W, 0.05) {
        match r {
            Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
            Role::Corner { vertex, met } => {
                assert_eq!(vertex, VertId(0));
                four += usize::from(met.len() == 4);
            }
            _ => {}
        }
    }
    assert!(four > 0);
}

#[test]
fn a_notch_takes_its_spike() {
    // A corner of nearly 360°: a 3° notch cut into a square to `W`, its
    // axis off the dyadic lines.
    let h = snap(0.75 * 1.5f64.to_radians().tan());
    let p = [
        [0.25, 0.25],
        [1.75, 0.25],
        [1.75, 0.7 - h],
        W,
        [1.75, 0.7 + h],
        [1.75, 1.75],
        [0.25, 1.75],
    ];
    let case = Case {
        loops: vec![
            (0..7)
                .map(|k| edge(k as u32, vec![line(p[k], p[(k + 1) % 7])]))
                .collect(),
        ],
        w: VertId(3),
        at: W,
        into: CoedgeId(2),
        out: CoedgeId(3),
        beta: BETA,
    };
    check_corner(case, true);
}

#[test]
fn curves_near_away_from_their_corner_are_thin() {
    // Two curves out of `W` that come within a quarter band again at the
    // far end, and a hole a quarter band inside the outer loop: the boxes
    // meeting both there are no corner's, and refine until they don't.
    let g = BETA / 4.0;
    let (a, b, m) = ([1.75, 0.5], [1.75, 0.5 + g], [1.375, 1.25]);
    let loops = vec![vec![
        edge(0, vec![line(W, a)]),
        edge(1, vec![line(a, b)]),
        edge(2, vec![line(b, m), line(m, W)]),
    ]];
    for (bx, r) in leaves_round(&loops, [1.6, 0.5], 0.05) {
        match r {
            Role::Thin(c) => panic!("box {bx:?} left thin, meeting {c:?}"),
            Role::Corner { vertex, met } => {
                assert_ne!(vertex, VertId(0), "box {bx:?} meeting {met:?}");
            }
            _ => {}
        }
    }
    let square = |p: [[f64; 2]; 4], first: u32| -> Vec<Edge> {
        (0..4)
            .map(|k| edge(first + k as u32, vec![line(p[k], p[(k + 1) % 4])]))
            .collect()
    };
    let y = snap(0.25 + g);
    let loops = vec![
        square([[0.25, 0.25], [1.75, 0.25], [1.75, 1.75], [0.25, 1.75]], 0),
        square([[0.5, y], [0.5, 1.0], [1.5, 1.0], [1.5, y]], 10),
    ];
    for (bx, r) in leaves_round(&loops, [1.0, 0.25], 0.05) {
        assert!(
            !matches!(r, Role::Thin(_) | Role::Corner { .. }),
            "box {bx:?}: {r:?}"
        );
    }
}

/// The cusp of [`cusp`] at band `beta`, its face's trims built.
fn cusp_at(beta: f64) -> (Built, Case) {
    let case = Case { beta, ..cusp() };
    (Built::new(&case.loops, beta), case)
}

#[test]
fn a_cusp_at_a_tight_band_is_one_record() {
    // A line tangent to an arc of radius 1 at `β = 10⁻⁸`: the spike is
    // about `√(2β·2)/…` long, tens of thousands of bands. The record cuts
    // it at a fixed cost; the runs reach the cut, and boxes anywhere along
    // the spike meeting both curves are the corner's, down to the finest.
    let (built, case) = cusp_at(1e-8);
    let trims = built.trims();
    let face = face_uv(&trims);
    let ps = plane();
    let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
    let rec = corners.cusp(case.w).expect("a sharp corner's record");
    assert_eq!(rec.m, 2);
    let s_beta = rec.s_beta_f64();
    // The tilted plane stretches lengths by at most 1.15: the cut lies
    // where the arc is between one and eight bands from the line.
    assert!(s_beta > 1e-4 && s_beta < 1e-3, "{s_beta}");
    for (c, end) in [(case.into, 1u8), (case.out, 0u8)] {
        let (_, run) = corners.run(&face, c, end).expect("a run");
        let t = run.reach().1.bounds()[0];
        let s = trims[c.index()].spans[0];
        let p = span_at_f(&s, t);
        let d = ((p[0] - W[0]) * (p[0] - W[0]) + (p[1] - W[1]) * (p[1] - W[1])).sqrt();
        assert!(
            d > 0.5 * s_beta && d < 1.01 * s_beta,
            "run of {c:?} reaches {d}"
        );
    }
    // The runs' search would have cost thousands of pieces per end.
    assert!(corners.pieces() < 20_000, "{}", corners.pieces());
    // Round points of the spike, refined until within the band: none is
    // left thin. Near `W`, where the curves are far less than a band
    // apart, the leaves meeting both are the corner's; at the middle the
    // band-sized leaves meet one curve each.
    for (f, corner_leaves) in [(0.05, true), (0.7, false)] {
        let s = f * s_beta;
        let at = [W[0] + s, W[1] + 0.25 * s * s];
        let region = |b: &BoxKey| box_dist(b, at) <= 1e-7;
        let leaves = refine(&face, &corners, &ps, case.beta, &region, MAX_BOX_DEPTH);
        let mut corner_boxes = 0;
        for (b, r) in &leaves {
            match r {
                Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
                Role::Corner { vertex, .. } => {
                    assert_eq!(*vertex, case.w);
                    corner_boxes += 1;
                }
                _ => {}
            }
        }
        assert!(leaves.len() < 5000, "{}", leaves.len());
        assert!(!corner_leaves || corner_boxes > 0, "at {f}");
    }
}

/// A lens of two parabolic arcs between `p` and `p + (L, 0)`, `L` a
/// quarter band, its vertices numbered `ids` (the one at `p` first): the
/// boxes holding the lens lie in both vertices' corners.
fn lens(ids: [u32; 2]) -> Vec<Vec<Edge>> {
    let l = BETA / 4.0;
    let (p, q) = ([1.0, 0.5], [1.0 + l, 0.5]);
    let h = l * 0.25;
    let mid = [1.0 + l * 0.5, 0.5];
    let arc = |a: [f64; 2], b: [f64; 2], up: f64| -> UvSpan {
        [
            [a[0], a[1], 1.0],
            [mid[0], mid[1] + up, 1.0],
            [b[0], b[1], 1.0],
        ]
    };
    vec![vec![
        edge(ids[0], vec![arc(p, q, h)]),
        edge(ids[1], vec![arc(q, p, -h)]),
    ]]
}

#[test]
fn a_box_in_two_corners_goes_to_the_lower_vertex() {
    let ps = plane();
    for ids in [[0, 1], [1, 0], [7, 2]] {
        let built = Built::new(&lens(ids), BETA);
        let trims = built.trims();
        let face = face_uv(&trims);
        let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
        assert_eq!(corners.corners().len(), 2);
        let region = |b: &BoxKey| box_dist(b, [1.0, 0.5]) <= BETA;
        let leaves = refine(&face, &corners, &ps, BETA, &region, 30);
        let mut seen = 0;
        for (b, r) in &leaves {
            match r {
                Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
                Role::Corner { vertex, met } if met.len() == 2 => {
                    // Both corners claim it: the vertex at `(1, ½)`, lower
                    // in `x`, whatever its id.
                    assert_eq!(*vertex, VertId(ids[0]), "ids {ids:?}, box {b:?}");
                    seen += 1;
                }
                _ => {}
            }
        }
        assert!(seen > 0, "ids {ids:?}");
    }
    // The geometric order must list the face's vertices.
    let built = Built::new(&lens([0, 1]), BETA);
    let trims = built.trims();
    let face = face_uv(&trims);
    for order in [
        vec![VertId(0)],
        vec![VertId(0), VertId(0)],
        vec![VertId(0), VertId(5)],
    ] {
        assert_eq!(
            Corners::new(&face, &ps, &order).err(),
            Some(BoxError::Order)
        );
    }
}

#[test]
fn a_teardrop_is_a_strip_of_one_range_at_a_time() {
    // One edge closed at `W` in two spans out to `(1½, ½)` and back, its
    // ends `90°` apart: a box beside `W` meeting both spans, away from the
    // runs, is no strip; refined round `W` none is left thin, and no strip
    // box meets the curve on both sides.
    let v = [snap(1.3), snap(0.47)];
    let tip = [v[0] + 0.5, v[1]];
    let (lo, hi) = ([v[0] + 0.25, v[1] - 0.25], [v[0] + 0.25, v[1] + 0.25]);
    let loops = vec![vec![edge(
        0,
        vec![
            [
                [v[0], v[1], 1.0],
                [lo[0], lo[1], 1.0],
                [tip[0], tip[1], 1.0],
            ],
            [
                [tip[0], tip[1], 1.0],
                [hi[0], hi[1], 1.0],
                [v[0], v[1], 1.0],
            ],
        ],
    )]];
    let built = Built::new(&loops, BETA);
    let trims = built.trims();
    let face = face_uv(&trims);
    let ps = plane();
    let corners = Corners::new(&face, &ps, &geo(&face)).expect("corners");
    // A box an eighth wide just right of the vertex, a half high: both
    // spans cross it.
    let grid = |x: f64, up: bool| {
        let k = x * 32.0;
        Dyadic::new((if up { k.ceil() } else { k.floor() }) as u64, 5).unwrap()
    };
    let u0 = grid(v[0] - 1.0 + 0.04, true);
    let u1 = Dyadic::new(u0.num() * (32 >> u0.exp()) + 4, 5).unwrap();
    let bx = BoxKey::new(
        Operand::A,
        FaceId(3),
        [1, 0],
        Extent::new(u0, u1).unwrap(),
        Extent::new(grid(v[1] - 0.25, false), grid(v[1] + 0.25, true)).unwrap(),
    );
    let r = role(&face, &corners, &bx, &first(), None).expect("role");
    assert_eq!(r.role, Role::Thin(smallvec::smallvec![CoedgeId(0)]));
    let region = |b: &BoxKey| box_dist(b, v) <= 0.2;
    let leaves = refine(&face, &corners, &ps, BETA, &region, 30);
    let mut corner_boxes = 0;
    for (b, r) in &leaves {
        match r {
            Role::Thin(c) => panic!("box {b:?} left thin, meeting {c:?}"),
            Role::Corner { vertex, .. } => {
                assert_eq!(*vertex, VertId(0));
                corner_boxes += 1;
            }
            Role::Strip(_) => {
                // One range: the box doesn't meet both halves, unless it
                // holds the vertex (the range runs across it).
                let rc = rect(b);
                let holds =
                    (rc[0][0]..=rc[0][1]).contains(&v[0]) && (rc[1][0]..=rc[1][1]).contains(&v[1]);
                let meets = |s: &UvSpan| {
                    (0..=400).any(|i| {
                        let p = span_at_f(s, i as f64 / 400.0);
                        box_dist(b, p) == 0.0
                    })
                };
                let both = meets(&trims[0].spans[0]) && meets(&trims[0].spans[1]);
                // Sampled points beside a box side may miss it: only a
                // box meeting both clearly inside is checked.
                if both && !holds {
                    let inner = |s: &UvSpan| {
                        (0..=400).any(|i| {
                            let p = span_at_f(s, i as f64 / 400.0);
                            p[0] > rc[0][0] && p[0] < rc[0][1] && p[1] > rc[1][0] && p[1] < rc[1][1]
                        })
                    };
                    let tip_in = rc[0][0] <= tip[0] && tip[0] <= rc[0][1];
                    assert!(
                        tip_in || !(inner(&trims[0].spans[0]) && inner(&trims[0].spans[1])),
                        "strip box {b:?} meets both sides"
                    );
                }
            }
            _ => {}
        }
    }
    assert!(corner_boxes > 0);
}
