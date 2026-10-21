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
//! partition sum to the whole run. Runs split along the trim's own line
//! (box lines read as moved to their high side) sum to the whole.
//! Queries on the tile line read the side records of the parts' sides
//! there. Corner pieces: a second coedge turning up from a vertex inside
//! the run (or on a box side, or through a box corner), its own edge's
//! curve meeting the first's at the vertex; runs holding the vertex take
//! both ribbons and sum, with the strip pieces beside them, to the
//! winding of the piece's boundary polygon.

use varde_testkit::rng::Rng;

use glam::DVec3;

use super::*;
use crate::bezier::Dyadic;
use crate::count::boxes::UvSpan;
use crate::ids::{EdgeId, FaceId, VertId};
use crate::num::decide::{Band, Fail, Sign};
use crate::num::exact::ORDERS;
use crate::perturb::{Curved, Moving, Rate, Seg as Line, T, rates_a};
use crate::records::keys::{Operand, Query, SideKey, SideLine, TileCorner, TileEdge, TilePoint};
use crate::records::ray::{ChainCrossing, Crossing, TileNet};
use crate::records::shadow::{Decided, Pass, ShadowGeometry};
use crate::records::side::SideGeometry;
use crate::records::table::Fresh;

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
                // Without cancellation: a nearly straight span has `a`
                // tiny, one root far off, the other `c₀/h`.
                let h = -(b + b.signum() * disc.sqrt()) / 2.0;
                ts.extend([h / a, c0 / h]);
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
                    single: true,
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
        self.degree_touching(trims, flipped, coedges, boxes, q, None)
    }

    /// [`Setup::degree`] with the ray record also listing a tangency of
    /// the edge's shadow with the ray's line at `touch` (an enclosure of
    /// the edge's one span's parameter): two moved crossings that cancel.
    fn degree_touching(
        &self,
        trims: &[Trim<'_>],
        flipped: bool,
        coedges: &[u32],
        boxes: &[BoxKey],
        q: [f64; 3],
        touch: Option<[f64; 2]>,
    ) -> Result<PieceDegree, PieceError> {
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], trims, flipped).unwrap();
        let mut rec = rays(&self.curve, q);
        rec.touches.extend(touch.map(|t| (0, t)));
        let ribbons: Vec<Ribbon<'_>> = coedges
            .iter()
            .map(|&c| Ribbon {
                coedge: CoedgeId(c),
                curve: &self.curve,
                map: &self.maps,
                rays: &rec,
            })
            .collect();
        settle(q, |sides| {
            strip_degree(
                &face,
                &self.patches,
                boxes,
                &ribbons,
                &QShadow::of(q).unwrap(),
                point(),
                sides,
            )
        })
    }
}

/// The face's points move with `A`, along `σ_A·d` and the fixed
/// translations: `d = (1, 0, 0)`, `σ_A = +`.
const D: DVec3 = DVec3::new(1.0, 0.0, 0.0);

/// The plane face's tile edges and corners as the side records read
/// them, and the query point at `q`.
struct Plane {
    q: DVec3,
}

impl ShadowGeometry for Plane {
    fn point(&self, p: Point) -> Moving {
        assert_eq!(p, point());
        Moving::fixed(self.q)
    }

    fn near_radius(&self, _: Point) -> f64 {
        0.0
    }

    fn ends(&self, _: Operand, _: EdgeId) -> [VertId; 2] {
        unreachable!("no edges here")
    }

    fn line(&self, _: Point, _: EdgeId) -> Line {
        unreachable!("no edges here")
    }
}

impl SideGeometry for Plane {
    fn tile_edge(&self, e: &TileEdge) -> Option<Span> {
        let at = |c: f64| match e.fixed {
            Dir::U => [SU * e.line as f64, SV * c, 0.0, 1.0],
            Dir::V => [SU * c, SV * e.line as f64, 0.0, 1.0],
        };
        let c = e.cell as f64;
        Some([at(c), at(c + 0.5), at(c + 1.0)])
    }

    fn corner(&self, _: Point, c: &TileCorner) -> Option<Moving> {
        let at = DVec3::new(SU * c.at[0] as f64, SV * c.at[1] as f64, 0.0);
        Some(Moving::of_a(at, Sign::Pos, D))
    }

    fn tile_net(&self, _: Operand, _: FaceId, tile: [i64; 2]) -> Option<TileNet> {
        Some(crate::count::testing::mapped_net(tile, |u, v| {
            [SU * u, SV * v, 0.0]
        }))
    }

    fn point_rates(&self, _: Point, _: &TilePoint) -> [Rate; ORDERS] {
        rates_a(Sign::Pos, D)
    }

    fn line_rates<S: Curved>(&self, _: Point, _: &SideLine, t: &S) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        [lit(D), lit(T[0]), lit(T[1]), lit(T[2])]
    }
}

/// A pass deciding in order on one thread.
struct Seq;

impl Pass for Seq {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        keys.iter().map(|k| decide(k).0).collect()
    }
}

/// `degree` with the side records it needs for the query at `q`,
/// decided as it asks for them.
fn settle(
    q: [f64; 3],
    degree: impl Fn(&Sides) -> Result<PieceDegree, PieceError>,
) -> Result<PieceDegree, PieceError> {
    let geom = Plane {
        q: DVec3::from_array(q),
    };
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).unwrap();
    let mut sides = Sides::new();
    for _ in 0..8 {
        match degree(&sides) {
            Err(PieceError::Need(k)) => sides.fill([k], &geom, band, &mut Seq).unwrap(),
            r => return r,
        }
    }
    panic!("records asked again after filling")
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

/// A box inside the trim alone: its loop no cut settles. The work of
/// finding so (the loop's scans of the trims) is reported all the same,
/// for the count to charge.
#[test]
fn a_box_inside_the_trim_alone_is_unsettled() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let r = run();
    let q = [5.0, 4.0, 0.0];
    assert_eq!(
        s.degree(&trims, false, &[1], &[r[3]], q),
        Err(PieceError::Unsettled)
    );
    let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, false).unwrap();
    let rec = rays(&s.curve, q);
    let ribbons = [Ribbon {
        coedge: CoedgeId(1),
        curve: &s.curve,
        map: &s.maps,
        rays: &rec,
    }];
    let mut tested = 0;
    let (got, _) = strip_degree_counted(
        &face,
        &s.patches,
        &[r[3]],
        &ribbons,
        &QShadow::of(q).unwrap(),
        point(),
        &Sides::new(),
        &mut tested,
    );
    assert_eq!(got, Err(PieceError::Unsettled));
    assert!(tested > 0, "the scans are work");
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

/// The run `[½, 1] × [0, ½]` of tile 0 and `[0, ½] × [0, ½]` of tile 1,
/// split at `v = ¼`, the trim's own line: per tile the box below the
/// trim and the box above it.
fn split_run() -> [BoxKey; 4] {
    let (low, high) = (ext((0, 0), (1, 2)), ext((1, 2), (1, 1)));
    let (u0, u1) = (ext((1, 1), (1, 0)), ext((0, 0), (1, 1)));
    [
        bx(0, u0, low),
        bx(0, u0, high),
        bx(1, u1, low),
        bx(1, u1, high),
    ]
}

#[test]
fn a_trim_along_the_line_between_two_runs() {
    // Runs whose outer side lies on the trim's line: the box lines read
    // as moved to their high side, the trim lies in the low run (its cuts
    // at the low run's sides just below their ends) and the high run is
    // clear of it, inside or outside the face as a whole by the face's
    // side of the trim there; the two sum to the whole run's degree.
    let r = split_run();
    let (low, high) = (vec![r[0], r[2]], vec![r[1], r[3]]);
    for (dy, reversed, flipped, region, sign, seed) in [
        (-DELTA, false, false, WHOLE, 1, 11),
        (-DELTA, true, true, WHOLE, -1, 12),
        (DELTA, true, false, [2.5, 7.5, 0.0, SV * V0 + DELTA], 1, 13),
    ] {
        let s = Setup::new(dy);
        let trims = [trim(&s.spans, 1, reversed)];
        let parts = [
            vec![low.clone(), high.clone()],
            vec![r.to_vec()],
            vec![vec![r[0]], vec![r[2]], high.clone()],
            vec![low.clone(), vec![r[1], r[3]]],
        ];
        for (k, p) in parts.iter().enumerate() {
            check(
                &s,
                &trims,
                flipped,
                &[1],
                p,
                region,
                sign,
                seed + 10 * k as u64,
            );
        }
        // The high run alone: no cut, its low side along the trim; inside
        // the face where the face lies above the trim, else outside.
        let face_above = dy < 0.0;
        let mut rng = Rng::new(seed);
        let mut ok = 0;
        for _ in 0..40 {
            let q = draw(&mut rng);
            let Some(within) = inside(plane_xy(q), [2.5, 7.5, SV * V0, 5.0], 1e-3) else {
                continue;
            };
            match s.degree(&trims, flipped, &[1], &high, q) {
                Ok(d) => {
                    ok += 1;
                    let want = if face_above && within { sign } else { 0 };
                    assert_eq!(d.degree, want, "{q:?}");
                    assert_eq!(d.cuts, 0, "{q:?}");
                }
                Err(PieceError::Unsettled | PieceError::Through) => (),
                Err(e) => panic!("{e:?} at {q:?}"),
            }
        }
        assert!(ok >= 30, "{ok}");
    }
}

/// A trim along the low edge of the face's tiles (`v = 0`), its edge's
/// curve below the tiles: the low edge read as moved down (not up, as the
/// other box lines), the trim lies in the boxes above it, and its ribbon
/// below the tiles counts in every partition.
#[test]
fn a_trim_along_the_tiles_low_edge_keeps_its_ribbon() {
    let line = |a: f64, b: f64| [[a, 0.0, 1.0], [(a + b) / 2.0, 0.0, 1.0], [b, 0.0, 1.0]];
    let y = -DELTA;
    let s = Setup {
        spans: vec![line(U0, 1.0), line(1.0, U1)],
        curve: vec![[
            [SU * U0, y, 0.0, 1.0],
            [SU * (U0 + U1) / 2.0, y, 0.0, 1.0],
            [SU * U1, y, 0.0, 1.0],
        ]],
        maps: maps(),
        patches: patches(),
    };
    let trims = [trim(&s.spans, 1, false)];
    let r = split_run();
    // The whole run, and its two tiles' columns (each row alone would
    // leave the high one clear of the trim, saying nothing of it).
    let parts = [vec![r.to_vec()], vec![vec![r[0], r[1]], vec![r[2], r[3]]]];
    let mut rng = Rng::new(95);
    for (k, parts) in parts.iter().enumerate() {
        let (mut ribbon, mut settled) = (0, 0);
        for _ in 0..60 {
            // On the plane, in the ribbon below the tiles or above it.
            let x = rng.range(1.0, 9.0);
            let y = if rng.unit() < 0.5 {
                rng.range(-0.009, -0.001)
            } else {
                rng.range(0.001, 6.0)
            };
            let q = [x, y, 0.0];
            let Some(truth) = inside(plane_xy(q), [2.5, 7.5, -DELTA, 5.0], 1e-3) else {
                continue;
            };
            let mut sum = Some(0);
            for p in parts {
                match s.degree(&trims, false, &[1], p, q) {
                    Ok(d) => sum = sum.map(|x| x + d.degree),
                    Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                    Err(e) => panic!("{e:?} partition {k} q {q:?}"),
                }
            }
            if let Some(sum) = sum {
                settled += 1;
                ribbon += usize::from(truth && y < 0.0);
                assert_eq!(sum, i64::from(truth), "partition {k} q {q:?}");
            }
        }
        assert!(
            settled >= 50 && ribbon >= 5,
            "partition {k}: {settled}, {ribbon}"
        );
    }
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
fn a_cut_inside_a_cancelled_tangency_is_unordered() {
    // The run's cuts lie at `u = ½` and `u = 3/2`, the edge's parameters
    // `t = 2/9` and `7/9` (`u = U0 + 1.8t`). A tangency of the edge's
    // shadow with the ray's line whose two moved crossings cancel holds
    // one of them in a piece cut inside it: no record orders that, so the
    // run is unsettled there; one between the cuts changes nothing.
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let q = [4.0, SV * V0 + 0.005, 0.0];
    let deg = |touch| {
        s.degree_touching(&trims, false, &[1], &run(), q, touch)
            .map(|d| d.degree)
    };
    assert_eq!(deg(None), Ok(1));
    assert_eq!(deg(Some([0.5, 0.5 + 1e-9])), Ok(1));
    for t in [2.0 / 9.0, 7.0 / 9.0] {
        assert_eq!(
            deg(Some([t - 1e-3, t + 1e-3])),
            Err(PieceError::Unsettled),
            "t {t}"
        );
        assert_eq!(
            deg(Some([t - 1e-12, t + 1e-12])),
            Err(PieceError::Unsettled),
            "t {t}"
        );
    }
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

/// Queries on the tile line `x = 5`, where the two halves of the run
/// meet; the halves' sides there are read from their side records.
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
            // On the tile line inside the run and away from the cut, the
            // parts' sides there are read from their side records.
            if x == 5.0 && [3.0, 3.75, 4.5].contains(&y) {
                assert_eq!(sum, Some(1), "({x}, {y})");
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

/// A corner: coedge 1 along `v = V0` from `u = U0` to the vertex `W`, at
/// `u = wu`, then coedge 2 up from `W` to `top` (`(wu + 0.1, 0.85)` but
/// where given), each
/// straight (coedge 1 broken at the tile line), the face above the first
/// and left of the second. Each edge's curve is straight, beside its
/// p-curve's image, and the two meet at `W`'s point, `DELTA` right of and
/// below `S(W)`.
struct Corner {
    wu: f64,
    top: [f64; 2],
    spans: [Vec<UvSpan>; 2],
    curves: [Vec<Span>; 2],
    maps: [Vec<SpanMap>; 2],
    patches: Vec<Patch>,
}

/// The vertex's `u`: inside the run's last box.
const WU: f64 = 1.3;

impl Corner {
    fn new() -> Corner {
        Corner::at(WU)
    }

    /// Coedge 2's far end.
    fn top(&self) -> [f64; 2] {
        self.top
    }

    /// The vertex's point.
    fn w_pos(&self) -> [f64; 2] {
        [SU * self.wu + DELTA, SV * V0 - DELTA]
    }

    /// The corner's edge curves at their parameters.
    fn c1_at(&self, t: f64) -> [f64; 2] {
        let (a, b) = ([SU * U0, SV * V0 - DELTA], self.w_pos());
        [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
    }

    fn c2_at(&self, t: f64) -> [f64; 2] {
        let top = self.top();
        let (a, b) = (self.w_pos(), [SU * top[0] + DELTA, SV * top[1]]);
        [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
    }

    /// The corner with its vertex at `u = wu` (in tile 1, right of the
    /// knot of coedge 1 at the tile line).
    fn at(wu: f64) -> Corner {
        Corner::to(wu, [wu + 0.1, 0.85])
    }

    /// The corner with its vertex at `u = wu` and coedge 2 up to `top`,
    /// leaving the run through its top side (`v = ½`).
    fn to(wu: f64, top: [f64; 2]) -> Corner {
        let line = |a: [f64; 2], b: [f64; 2]| {
            [
                [a[0], a[1], 1.0],
                [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 1.0],
                [b[0], b[1], 1.0],
            ]
        };
        let seg = |a: [f64; 2], b: [f64; 2]| {
            vec![[
                [a[0], a[1], 0.0, 1.0],
                [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 0.0, 1.0],
                [b[0], b[1], 0.0, 1.0],
            ]]
        };
        // `u = U0 + (wu − U0)·t` on the first edge: the knot at `u = 1`.
        let k = (1.0 - U0) / (wu - U0);
        let mut c = Corner {
            wu,
            top,
            spans: [
                vec![line([U0, V0], [1.0, V0]), line([1.0, V0], [wu, V0])],
                vec![line([wu, V0], top)],
            ],
            curves: [Vec::new(), Vec::new()],
            maps: [
                vec![
                    SpanMap {
                        span: 0,
                        t: [0.0, k],
                    },
                    SpanMap {
                        span: 0,
                        t: [k, 1.0],
                    },
                ],
                vec![SpanMap {
                    span: 0,
                    t: [0.0, 1.0],
                }],
            ],
            patches: patches(),
        };
        c.curves = [
            seg(c.c1_at(0.0), c.c1_at(1.0)),
            seg(c.c2_at(0.0), c.c2_at(1.0)),
        ];
        c
    }

    /// The corner run's piece boundary in the plane, counter-clockwise:
    /// in at the run's left side along coedge 1's radial, along the edges'
    /// curves through the vertex, out along coedge 2's radial at the run's
    /// top, back along the top and the left side.
    fn polygon(&self) -> Vec<[f64; 2]> {
        let (wu, top) = (self.wu, self.top());
        let entry = (0.5 - U0) / (wu - U0);
        let exit = (0.5 - V0) / (top[1] - V0);
        let ux = wu + (top[0] - wu) * exit;
        vec![
            [SU * 0.5, SV * V0],
            self.c1_at(entry),
            self.w_pos(),
            self.c2_at(exit),
            [SU * ux, SV * 0.5],
            [SU * 0.5, SV * 0.5],
        ]
    }

    fn trims(&self, reversed: bool) -> [Trim<'_>; 2] {
        let t = |k: usize, ends: [u32; 2]| Trim {
            coedge: CoedgeId(k as u32 + 1),
            reversed,
            spans: &self.spans[k],
            ends: ends.map(VertId),
            zero_length: false,
            band: 0.0,
        };
        [t(0, [1, 2]), t(1, [2, 3])]
    }

    fn degree(
        &self,
        trims: &[Trim<'_>],
        flipped: bool,
        coedges: &[u32],
        boxes: &[BoxKey],
        q: [f64; 3],
    ) -> Result<PieceDegree, PieceError> {
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], trims, flipped).unwrap();
        let recs = [rays(&self.curves[0], q), rays(&self.curves[1], q)];
        let ribbons: Vec<Ribbon<'_>> = coedges
            .iter()
            .map(|&c| {
                let k = c as usize - 1;
                Ribbon {
                    coedge: CoedgeId(c),
                    curve: &self.curves[k],
                    map: &self.maps[k],
                    rays: &recs[k],
                }
            })
            .collect();
        settle(q, |sides| {
            strip_degree(
                &face,
                &self.patches,
                boxes,
                &ribbons,
                &QShadow::of(q).unwrap(),
                point(),
                sides,
            )
        })
    }
}

/// The corner run: [`run`], the vertex in its last box (`u ∈ [1¼,
/// 1½]`), coedge 2 leaving it by its top.
fn corner_run() -> Vec<BoxKey> {
    run()
}

/// The winding of `ring` about `p`, `None` within `m` of it.
fn winding(ring: &[[f64; 2]], p: [f64; 2], m: f64) -> Option<i64> {
    let mut w = 0;
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        let s = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
        let (ex, ey) = (a[0] + s * dx - p[0], a[1] + s * dy - p[1]);
        if ex * ex + ey * ey < m * m {
            return None;
        }
        let cross = dx * (p[1] - a[1]) - dy * (p[0] - a[0]);
        if a[1] <= p[1] && p[1] < b[1] && cross > 0.0 {
            w += 1;
        } else if b[1] <= p[1] && p[1] < a[1] && cross < 0.0 {
            w -= 1;
        }
    }
    Some(w)
}

/// The corner run's partitions: strip pieces of either coedge and corner
/// pieces holding the vertex, cut across the tile line and the vertex's
/// box.
fn corner_parts() -> Vec<Vec<Vec<BoxKey>>> {
    let r = corner_run();
    vec![
        vec![r.clone()],
        vec![r[..2].to_vec(), r[2..].to_vec()],
        // `r[3]` lies inside the trim, clear of the curves: not alone.
        vec![vec![r[0]], vec![r[1]], vec![r[2], r[3]], vec![r[4]]],
        vec![r[..4].to_vec(), vec![r[4]]],
        vec![vec![r[0], r[2], r[4]], vec![r[1], r[3]]],
        vec![r[..3].to_vec(), vec![r[3], r[4]]],
    ]
}

/// Runs of any partition of the corner run, each with both ribbons, sum
/// to the winding of the piece's boundary.
fn check_corner(c: &Corner, flipped: bool, sign: i64, seed: u64) {
    let trims = c.trims(flipped);
    let ring = c.polygon();
    for (k, parts) in corner_parts().iter().enumerate() {
        let mut rng = Rng::new(seed + k as u64);
        let (mut ok, mut all) = (0, 0);
        // Random queries, and queries on the tile line `x = 5`.
        let qs: Vec<[f64; 3]> = (0..150)
            .map(|_| draw(&mut rng))
            .chain((0..40).map(|j| [5.0, 1.0 + 0.1 * j as f64, 0.0]))
            .collect();
        for q in qs {
            let Some(truth) = winding(&ring, plane_xy(q), 1e-3) else {
                continue;
            };
            all += 1;
            let mut sum = Some(0);
            for p in parts {
                match c.degree(&trims, flipped, &[1, 2], p, q) {
                    Ok(d) => sum = sum.map(|x| x + d.degree),
                    Err(PieceError::Unsettled | PieceError::Through) => sum = None,
                    Err(e) => panic!("{e:?} partition {k} q {q:?}"),
                }
            }
            if let Some(sum) = sum {
                ok += 1;
                assert_eq!(sum, sign * truth, "partition {k}, q = {q:?}");
            }
        }
        assert!(ok * 10 >= all * 8, "partition {k}: {ok} of {all}");
    }
}

#[test]
fn corner_pieces_sum_to_the_winding() {
    check_corner(&Corner::new(), false, 1, 300);
}

#[test]
fn flipped_corner_with_its_coedges_against_their_edges() {
    // Both coedges against their edges on a flipped face: the same
    // region, its boundary clockwise in UV.
    check_corner(&Corner::new(), true, -1, 400);
}

#[test]
fn a_corner_needs_every_coedge_at_its_vertex() {
    let c = Corner::new();
    let trims = c.trims(false);
    let r = corner_run();
    let q = [4.0, 4.0, 0.0];
    // Coedge 2 starts inside the run: a corner piece must take it.
    assert_eq!(
        c.degree(&trims, false, &[1], &r, q),
        Err(PieceError::Open {
            coedge: CoedgeId(2)
        })
    );
    // Without coedge 1's ribbon, coedge 2 starts where nothing ends.
    assert_eq!(
        c.degree(&trims, false, &[2], &r[2..], q),
        Err(PieceError::Open {
            coedge: CoedgeId(1)
        })
    );
    assert_eq!(c.degree(&trims, false, &[1, 2], &r, q).unwrap().degree, 1);
    // The two halves of the run: the left one a strip piece of coedge 1.
    let left = c.degree(&trims, false, &[1], &r[..2], q).unwrap();
    let right = c.degree(&trims, false, &[1, 2], &r[2..], q).unwrap();
    assert_eq!(left.degree + right.degree, 1);
    // The vertex's two curves meeting there, unbalanced: coedge 2 made
    // to start at another vertex.
    let mut odd = c.trims(false);
    odd[1].ends = [VertId(7), VertId(3)];
    assert_eq!(
        c.degree(&odd, false, &[1, 2], &r, q),
        Err(PieceError::Open {
            coedge: CoedgeId(1)
        })
    );
}

/// The vertex on a box side (`u = 1¼`, between the run's tile-1 boxes):
/// box lines read as moved to their high side, it lies in the box left
/// of the side, and coedge 2 leaves that box through the side just above
/// the vertex; the runs of every partition sum to the winding, and a run
/// the second coedge crosses needs its ribbon.
#[test]
fn a_vertex_on_a_box_side() {
    let c = Corner::at(1.25);
    check_corner(&c, false, 1, 310);
    check_corner(&c, true, -1, 410);
    let trims = c.trims(false);
    let r = corner_run();
    let q = [6.5, 4.0, 0.0];
    // The box right of the vertex holds coedge 2 only.
    assert!(c.degree(&trims, false, &[2], &r[4..], q).is_ok());
    assert_eq!(
        c.degree(&trims, false, &[1], &r[4..], q),
        Err(PieceError::Open {
            coedge: CoedgeId(2)
        })
    );
    // The box holding the vertex needs both.
    assert_eq!(
        c.degree(&trims, false, &[1], &r[2..3], q),
        Err(PieceError::Open {
            coedge: CoedgeId(2)
        })
    );
}

/// A slit (two coedges of one edge sharing its p-curve) along `v = ¼`,
/// the line between the split run's rows: it lies in the low row, its
/// ribbons cancel, and runs of every partition sum to the run's degree,
/// the face on both sides of the slit.
#[test]
fn a_slit_on_a_box_line() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false), trim(&s.spans, 2, true)];
    let r = split_run();
    let (low, high) = (vec![r[0], r[2]], vec![r[1], r[3]]);
    let parts = [
        vec![r.to_vec()],
        vec![low, high],
        r.iter().map(|b| vec![*b]).collect(),
    ];
    for (k, p) in parts.iter().enumerate() {
        check(
            &s,
            &trims,
            false,
            &[1, 2],
            p,
            [2.5, 7.5, 0.0, 5.0],
            1,
            70 + k as u64,
        );
    }
}

/// Coedge 2 through the run's top right corner `(1½, ½)` exactly, between
/// its ends: leaving by the top side or the right side as box lines read
/// moved (its slope against `√2`), from a vertex inside the last box or
/// on its left side `u = 1¼`; or through an inner box corner clipping the
/// box there; runs of every partition sum to the winding.
#[test]
fn a_corner_curve_through_box_corners() {
    for (j, (wu, top)) in [
        // Slope 2 (steeper than `√2`): out through the top side.
        (1.375, [1.6875, 0.875]),
        // Slope 1: out through the right side.
        (1.25, [1.625, 0.625]),
        // Slope −2 through the top right corner of `r[3]`, `(1¼, ½)`:
        // in through its right side and out through its top, both next
        // to the corner (a clip), between the vertex and the run's top.
        (1.375, [1.125, 0.75]),
    ]
    .into_iter()
    .enumerate()
    {
        // Unflipped, then flipped with the coedges against their edges.
        let flipped = j % 2 == 1;
        let sign = if flipped { -1 } else { 1 };
        check_corner(&Corner::to(wu, top), flipped, sign, 320 + j as u64);
    }
}

#[test]
fn a_side_on_the_tile_line_needs_its_record() {
    let s = Setup::new(-DELTA);
    let trims = [trim(&s.spans, 1, false)];
    let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, false).unwrap();
    let q = [5.0, 4.0, 0.0];
    let rec = rays(&s.curve, q);
    let ribbons = [Ribbon {
        coedge: CoedgeId(1),
        curve: &s.curve,
        map: &s.maps,
        rays: &rec,
    }];
    let left = &run()[..2];
    let got = strip_degree(
        &face,
        &s.patches,
        left,
        &ribbons,
        &QShadow::of(q).unwrap(),
        point(),
        &Sides::new(),
    );
    let edge = TileEdge {
        operand: Operand::A,
        face: FaceId(3),
        fixed: Dir::U,
        line: 1,
        cell: 0,
    };
    assert_eq!(
        got,
        Err(PieceError::Need(Want::Side(SideKey {
            q: point(),
            line: SideLine::Edge(edge),
        })))
    );
}

/// A hole: a closed straight triangle (one vertex, clockwise in UV)
/// above the trim in tile 0, its apex at `(0.75, top)`, its edge's curve
/// its image on the plane.
struct Hole {
    spans: Vec<UvSpan>,
    curve: Vec<Span>,
    maps: Vec<SpanMap>,
}

impl Hole {
    fn new(top: f64) -> Hole {
        let pts = [[0.75, top], [0.9, 0.3], [0.6, 0.3]];
        let mut spans = Vec::new();
        let mut curve = Vec::new();
        for k in 0..3 {
            let (a, b) = (pts[k], pts[(k + 1) % 3]);
            let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            spans.push([a, m, b].map(|p| [p[0], p[1], 1.0]));
            curve.push([a, m, b].map(|p| [SU * p[0], SV * p[1], 0.0, 1.0]));
        }
        let maps = (0..3)
            .map(|k| SpanMap {
                span: k,
                t: [0.0, 1.0],
            })
            .collect();
        Hole { spans, curve, maps }
    }
}

/// A closed trim inside a run counts its whole ray record; one touching
/// the run's top side at its vertex lies in the run all the same (its
/// vertex below the side's moved line); and a closed trim inside the run
/// must be among its ribbons.
#[test]
fn a_closed_trim_touching_the_boundary_lies_on_one_side() {
    let s = Setup::new(-DELTA);
    let run = run();
    let degree = |h: &Hole, q: [f64; 3], with_hole: bool| {
        let hole = Trim {
            coedge: CoedgeId(2),
            reversed: false,
            spans: &h.spans,
            ends: [VertId(5), VertId(5)],
            zero_length: false,
            band: 0.0,
        };
        let trims = [trim(&s.spans, 1, false), hole];
        let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, false).unwrap();
        let recs = [rays(&s.curve, q), rays(&h.curve, q)];
        let ribbons = [
            Ribbon {
                coedge: CoedgeId(1),
                curve: &s.curve,
                map: &s.maps,
                rays: &recs[0],
            },
            Ribbon {
                coedge: CoedgeId(2),
                curve: &h.curve,
                map: &h.maps,
                rays: &recs[1],
            },
        ];
        let n = if with_hole { 2 } else { 1 };
        settle(q, |sides| {
            strip_degree(
                &face,
                &s.patches,
                &run,
                &ribbons[..n],
                &QShadow::of(q).unwrap(),
                point(),
                sides,
            )
        })
    };
    // In the hole, and in the face beside it.
    let (hole_q, face_q) = ([3.7, 3.6, 0.0], [3.0, 4.8, 0.0]);
    for top in [0.45, 0.5] {
        let h = Hole::new(top);
        assert_eq!(degree(&h, hole_q, true).unwrap().degree, 0, "{top}");
        assert_eq!(degree(&h, face_q, true).unwrap().degree, 1, "{top}");
        assert_eq!(
            degree(&h, face_q, false),
            Err(PieceError::Open {
                coedge: CoedgeId(2)
            }),
            "{top}"
        );
    }
}

/// A corner whose vertex lies on the tile line `u = 1` inside the run:
/// coedge 1 along `v = V0` up to it in tile 0, coedge 2 from it up to
/// `(1.1, 0.85)` in tile 1, each edge's curve its p-curve's image.
#[test]
fn a_corner_on_the_tile_line() {
    let line = |a: [f64; 2], b: [f64; 2]| {
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        (
            [a, m, b].map(|p| [p[0], p[1], 1.0]),
            [a, m, b].map(|p| [SU * p[0], SV * p[1], 0.0, 1.0]),
        )
    };
    let top = [1.1, 0.85];
    let (s1, c1) = line([U0, V0], [1.0, V0]);
    let (s2, c2) = line([1.0, V0], top);
    let (s1, s2, c1, c2) = (vec![s1], vec![s2], vec![c1], vec![c2]);
    let map = [SpanMap {
        span: 0,
        t: [0.0, 1.0],
    }];
    let t = |k: u32, spans, ends: [u32; 2]| Trim {
        coedge: CoedgeId(k),
        reversed: false,
        spans,
        ends: ends.map(VertId),
        zero_length: false,
        band: 0.0,
    };
    let trims = [t(1, &s1[..], [1, 2]), t(2, &s2[..], [2, 3])];
    let face = FaceUv::new(Operand::A, FaceId(3), TILES, [1.0, 1.0], &trims, false).unwrap();
    let r = run();
    let exit = 1.0 + (top[0] - 1.0) * (0.5 - V0) / (top[1] - V0);
    let ring = [
        [SU * 0.5, SV * V0],
        [SU, SV * V0],
        [SU * exit, SV * 0.5],
        [SU * 0.5, SV * 0.5],
    ];
    let mut rng = Rng::new(500);
    let (mut ok, mut all) = (0, 0);
    for _ in 0..200 {
        let q = draw(&mut rng);
        let Some(truth) = winding(&ring, plane_xy(q), 1e-3) else {
            continue;
        };
        all += 1;
        let recs = [rays(&c1, q), rays(&c2, q)];
        let ribbons = [
            Ribbon {
                coedge: CoedgeId(1),
                curve: &c1,
                map: &map,
                rays: &recs[0],
            },
            Ribbon {
                coedge: CoedgeId(2),
                curve: &c2,
                map: &map,
                rays: &recs[1],
            },
        ];
        let got = settle(q, |sides| {
            strip_degree(
                &face,
                &patches(),
                &r,
                &ribbons,
                &QShadow::of(q).unwrap(),
                point(),
                sides,
            )
        });
        match got {
            Ok(d) => {
                ok += 1;
                assert_eq!(d.degree, truth, "q = {q:?}");
            }
            Err(PieceError::Unsettled | PieceError::Through) => (),
            Err(e) => panic!("{e:?} at {q:?}"),
        }
    }
    assert!(ok * 10 >= all * 8, "{ok} of {all}");
}

/// A p-curve's ends at `0` and `−0` are one UV point: the curve is closed,
/// and a vertex's curves meeting there balance.
#[test]
fn signed_zeros_are_one_uv_point() {
    let spans = [
        [[0.0, 0.3, 1.0], [0.2, 0.1, 1.0], [0.4, 0.3, 1.0]],
        [[0.4, 0.3, 1.0], [0.2, 0.5, 1.0], [-0.0, 0.3, 1.0]],
    ];
    let mut t = trim(&spans, 1, false);
    t.ends = [VertId(4), VertId(4)];
    assert!(is_closed(&t));
    let (a, b) = uv_ends(&t);
    assert_eq!(a, b);
}

#[test]
fn ends_in_corner_pieces_are_the_vertex_only_near_it() {
    let w = [1.0, 2.0, 3.0];
    let hull = |r: f64| HullBox {
        lo: w.map(|x| x - r),
        hi: w.map(|x| x + r),
    };
    let named = [(5u32, CoedgeId(1)), (2u32, CoedgeId(7))];
    // Within `r_w` (the box's corner at `√3·r`): the vertex.
    assert_eq!(
        corner_end(&hull(0.5), w, 1.0, &named),
        Some(CornerEnd::Vertex)
    );
    // Past it, or not certified within it: the curve lower in the
    // geometric order, whatever its id.
    assert_eq!(
        corner_end(&hull(0.6), w, 1.0, &named),
        Some(CornerEnd::Curve(CoedgeId(7)))
    );
    assert_eq!(
        corner_end(&hull(0.5), w, 0.5, &named[..1]),
        Some(CornerEnd::Curve(CoedgeId(1)))
    );
    // Equal keys: the lesser id.
    let tied = [(3u32, CoedgeId(9)), (3u32, CoedgeId(4))];
    assert_eq!(
        corner_end(&hull(2.0), w, 1.0, &tied),
        Some(CornerEnd::Curve(CoedgeId(4)))
    );
    // Nothing named far out; an unbounded hull is never near.
    assert_eq!(corner_end::<u32>(&hull(2.0), w, 1.0, &[]), None);
    let open = HullBox {
        lo: [f64::NEG_INFINITY; 3],
        hi: w,
    };
    assert_eq!(corner_end::<u32>(&open, w, 1.0, &[]), None);
}

/// A crossing of a side line next to the side's end, within an `f64`
/// enclosure of it, is placed against the end exactly: a straight span
/// (flat data) a hair off the end on either side, or through it; and a
/// curved span at one of its two roots.
#[test]
fn crossings_next_to_a_side_end_are_placed_exactly() {
    let r = |x: f64| crate::num::exact::rat(x);
    let tiny = f64::EPSILON;
    // `v = (1 + 2^-52)·u` crosses `u = 1/2` at `v = 1/2 + 2^-53`.
    let sp: UvSpan = [
        [0.0, 0.0, 1.0],
        [0.5, 0.5 + tiny / 2.0, 1.0],
        [1.0, 1.0 + tiny, 1.0],
    ];
    let line = r(0.5);
    let at = |e: f64| root_against(&sp, 0, &line, &r(e), [0.0, 1.0]);
    assert_eq!(at(0.5), Some(Ordering::Greater));
    assert_eq!(at(0.5 + tiny), Some(Ordering::Less));
    // Through the end exactly: the corner rule's case.
    let half =
        Rational::from(1) / Rational::from(2) + Rational::from(1) / Rational::from(1u64 << 53);
    assert_eq!(
        root_against(&sp, 0, &line, &half, [0.25, 0.75]),
        Some(Ordering::Equal)
    );
    // Weights scale nothing of the place: the same line, weighted.
    let w: UvSpan = sp.map(|p| [p[0], p[1], 3.0]);
    assert_eq!(
        root_against(&w, 0, &line, &r(0.5), [0.0, 1.0]),
        Some(Ordering::Greater)
    );
    // Along `v` lines too: `u = (1 − 2^-52)·v` crosses `v = 1/2` below
    // `u = 1/2`.
    let sv: UvSpan = [
        [0.0, 0.0, 1.0],
        [0.5 - tiny / 4.0, 0.5, 1.0],
        [1.0 - tiny / 2.0, 1.0, 1.0],
    ];
    assert_eq!(
        root_against(&sv, 1, &line, &r(0.5), [0.0, 1.0]),
        Some(Ordering::Less)
    );
    // A curved span, `u = 2t(1 − t)`, `v = t`, crosses `u = 3/8` at
    // `t = 1/4` and `3/4`: each root placed by its own interval.
    let c: UvSpan = [[0.0, 0.0, 1.0], [1.0, 0.5, 1.0], [0.0, 1.0, 1.0]];
    let l = r(0.375);
    for (span, v) in [([0.0, 0.5], 0.25), ([0.5, 1.0], 0.75)] {
        assert_eq!(root_against(&c, 0, &l, &r(v), span), Some(Ordering::Equal));
        assert_eq!(
            root_against(&c, 0, &l, &r(v + tiny), span),
            Some(Ordering::Less)
        );
        assert_eq!(
            root_against(&c, 0, &l, &r(v - tiny), span),
            Some(Ordering::Greater)
        );
    }
    // A straight span's crossing: its place and slope as exact values.
    let (a, slope) = straight_cross(&sp, 0, &line, [0.0, 1.0]).expect("straight");
    assert_eq!(a, half);
    assert_eq!(
        slope,
        Rational::from(1) + Rational::from(1) / Rational::from(1u64 << 52)
    );
    assert_eq!(straight_cross(&sp, 0, &line, [0.75, 1.0]), None);
    assert_eq!(straight_cross(&c, 0, &l, [0.0, 0.5]), None);
    // No sign change over the interval, or a weight not positive: none.
    assert_eq!(root_against(&c, 0, &l, &r(0.25), [0.4, 0.6]), None);
    let neg: UvSpan = [[0.0, 0.0, 1.0], [0.5, 0.5, -1.0], [1.0, 1.0, 1.0]];
    assert_eq!(root_against(&neg, 0, &line, &r(0.5), [0.0, 1.0]), None);
}

mod lines;
mod spike;
