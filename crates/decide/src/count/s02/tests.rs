//! The count of a face's column hits above or below a point, on whole
//! faces: a half-cylinder overhanging its own edge (the column near the
//! edge's shadow hits only the far sheet, whose own height counts), a
//! cube's faces at a vertex of the other operand on the cube's corner
//! (every face at the corner reads the one point–vertex record, and the
//! count over the cube is the moved point's winding), plane faces whose
//! hits are exact (on a huge tile, next to their edges, under other
//! refinements one key per hit), runs the trims miss, the fold rule's
//! records and the straight edges' ray records.

use std::collections::BTreeMap;

use glam::DVec3;

use super::*;
use crate::bezier::Dyadic;
use crate::bezier::HomPatch;
use crate::count::boxes::{Extent, UvSpan};
use crate::count::testing::*;
use crate::ids::{EdgeId, FaceId, VertId};
use crate::num::bounded::Bounded;
use crate::num::decide::Sign;
use crate::num::trig;
use crate::records::feet::NearFeet;
use crate::records::height::Records;
use crate::records::keys::{HKey, HeightKey, Operand, Query};
use crate::records::ray::Span;
use crate::records::shadow::Shadows;

mod budget;
mod cube;
mod fold;

/// The part `[a, b]` of a rational quadratic span (affine points and
/// weights), by its blossom.
fn sub_span(s: &Span, a: f64, b: f64) -> Span {
    let h = s.map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]);
    let bl = |x: f64, y: f64| -> [f64; 4] {
        let c = [(1.0 - x) * (1.0 - y), (1.0 - x) * y + x * (1.0 - y), x * y];
        std::array::from_fn(|k| c[0] * h[0][k] + c[1] * h[1][k] + c[2] * h[2][k])
    };
    [bl(a, a), bl(a, b), bl(b, b)].map(|p| [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]])
}

/// A rational quadratic span's point at `t`.
fn eval(s: &Span, t: f64) -> [f64; 3] {
    let c = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let w: f64 = (0..3).map(|i| c[i] * s[i][3]).sum();
    std::array::from_fn(|k| (0..3).map(|i| c[i] * s[i][3] * s[i][k]).sum::<f64>() / w)
}

/// The reads of a run with no records: nothing near.
fn no_reads(order: &Order) -> (Shadows, NearFeet, &Order) {
    (Shadows::new(), NearFeet::new(), order)
}

// ---------------------------------------------------------------------
// A half-cylinder overhanging its own edge.

/// The cylinder's frame: `e1` across, `e3 = UP/|UP|` up, the axis `e2`.
fn frame() -> [DVec3; 3] {
    let e3 = UP.normalize();
    let e1 = R_HAT.normalize();
    [e1, e3.cross(e1), e3]
}

const RADIUS: f64 = 5.0;
const LENGTH: f64 = 8.0;
/// The tiles' first angles, degrees: each tile a 90° arc.
const TILE_AT: [f64; 3] = [-95.0, -5.0, 85.0];
/// The face's angles from `TILE_AT`'s arcs: the local parameter of −80°
/// in the first tile and of 100° in the third (15° in from each start).
fn local_of(deg_in: f64) -> f64 {
    let half = 45.0f64.to_radians();
    trig::tan((deg_in - 45.0).to_radians() / 2.0) / trig::tan(half / 2.0) / 2.0 + 0.5
}

/// The cylinder's point at angle `deg` (in the plane across the axis,
/// `e3` up) and axial `y`.
fn cyl(deg: f64, y: f64) -> [f64; 3] {
    let [e1, e2, e3] = frame();
    let t = deg.to_radians();
    let (sin, cos) = trig::sin_cos(t);
    (e1 * (RADIUS * cos) + e3 * (RADIUS * sin) + e2 * y).to_array()
}

/// The arc of tile `i` at axial `y`, a rational quadratic.
fn arc(i: usize, y: f64) -> Span {
    let a = TILE_AT[i];
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let [p0, p2] = [cyl(a, y), cyl(a + 90.0, y)];
    // The tangents' meet: the middle point pushed out from the axis by
    // `1/cos 45°`.
    let axis = frame()[1] * y;
    let p1 = (axis + (DVec3::from_array(cyl(a + 45.0, y)) - axis) / w).to_array();
    [
        [p0[0], p0[1], p0[2], 1.0],
        [p1[0], p1[1], p1[2], w],
        [p2[0], p2[1], p2[2], 1.0],
    ]
}

/// The half-cylinder face of `B`: tiles `[0, 3) × [0, 1)`, `u` the arcs,
/// `v` the axis over `[0, LENGTH]`; trimmed to `u ∈ [u_a, u_b]` (−80° to
/// 100°), `v ∈ [0.3, 0.7]`; its outward normal points away from the
/// axis, against the patch's, so it is flipped. With `with_nets` its
/// column hits are exact on its tiles' nets, else not formed.
fn half_cylinder(band: f64, with_nets: bool) -> FaceData {
    let mut patches = Vec::new();
    let mut nets = Vec::new();
    for i in 0..3 {
        let rows: Vec<Span> = [0.0, 0.5, 1.0].map(|v| arc(i, LENGTH * v)).to_vec();
        let mut net = Vec::new();
        for a in 0..3 {
            for row in &rows {
                net.push(row[a]);
            }
        }
        nets.push(std::array::from_fn(|k| net[k]));
        patches.push(Patch::from_affine(2, 2, net, &first()).unwrap());
    }
    let (ua, ub) = (local_of(15.0), 2.0 + local_of(15.0));
    let (v0, v1) = (0.3, 0.7);
    let (la, lb) = (ua, ub - 2.0);
    let arc_at = |v: f64| -> Vec<Span> {
        let y = LENGTH * v;
        vec![
            sub_span(&arc(0, y), la, 1.0),
            arc(1, y),
            sub_span(&arc(2, y), 0.0, lb),
        ]
    };
    let parc = |v: f64| -> Vec<UvSpan> {
        vec![
            pline([ua, v], [1.0, v]),
            pline([1.0, v], [2.0, v]),
            pline([2.0, v], [ub, v]),
        ]
    };
    let at = |u: f64, v: f64| -> [f64; 3] {
        let (i, l) = if u < 1.0 {
            (0, u)
        } else if u < 2.0 {
            (1, u - 1.0)
        } else {
            (2, u - 2.0)
        };
        let s = arc(i, LENGTH * v);
        eval(&s, l)
    };
    let (p1, p2, p3, p4) = (at(ua, v0), at(ub, v0), at(ub, v1), at(ua, v1));
    // V1 (ua, v0), V2 (ub, v0), V3 (ub, v1), V4 (ua, v1); the loop runs
    // V1 → V4 → V3 → V2, clockwise in UV (a flipped face).
    let trims = vec![
        TrimData {
            coedge: CoedgeId(1),
            edge: EdgeId(1),
            reversed: true,
            spans: parc(v0),
            ends: [VertId(1), VertId(2)],
            band,
            curve: arc_at(v0),
            maps: whole(3),
        },
        TrimData {
            coedge: CoedgeId(2),
            edge: EdgeId(2),
            reversed: true,
            spans: vec![pline([ub, v0], [ub, v1])],
            ends: [VertId(2), VertId(3)],
            band,
            curve: vec![line(p2, p3)],
            maps: whole(1),
        },
        TrimData {
            coedge: CoedgeId(3),
            edge: EdgeId(3),
            reversed: false,
            spans: parc(v1),
            ends: [VertId(4), VertId(3)],
            band,
            curve: arc_at(v1),
            maps: whole(3),
        },
        TrimData {
            coedge: CoedgeId(4),
            edge: EdgeId(4),
            reversed: false,
            spans: vec![pline([ua, v0], [ua, v1])],
            ends: [VertId(1), VertId(4)],
            band,
            curve: vec![line(p1, p4)],
            maps: whole(1),
        },
    ];
    FaceData {
        operand: Operand::B,
        id: FaceId(1),
        tiles: [0..3, 0..1],
        patches,
        flipped: true,
        trims,
        nets: with_nets.then_some(nets),
    }
}

fn cylinder_order(f: &FaceData) -> Order {
    let mut o = Order::default();
    for t in &f.trims {
        o.edges.insert((Operand::B, t.edge), t.ends);
        let last = t.curve.len() - 1;
        let [a, b] = [t.curve[0][0], t.curve[last][2]];
        o.pos.insert((Operand::B, t.ends[0]), [a[0], a[1], a[2]]);
        o.pos.insert((Operand::B, t.ends[1]), [b[0], b[1], b[2]]);
    }
    o
}

/// `s02` of the point of `A` at `q` against the half-cylinder: from its
/// pieces alone (no near piece there).
fn s02_cylinder(f: &FaceData, q: [f64; 3]) -> i64 {
    let order = cylinder_order(f);
    let point = Point::vert(Operand::A, VertId(7));
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let rays = |t: &TrimData| chain(&t.curve, q, Operand::A);
    let p = f
        .pieces(&order, point, q, &rays, &reads, &Sides::new())
        .expect("certified");
    assert!(p.near.is_empty(), "no piece near the point");
    p.count(&reads, &Heights::new(), &[])
        .expect("no record read")
}

#[test]
fn an_overhanging_sheet_counts_by_its_own_height() {
    let band = 1.0 / 1024.0;
    let f = half_cylinder(band, false);
    // The edge at −80° and the point across the axis plane from it, half
    // a band inside and outside its shadow.
    let [e1, e2, _] = frame();
    let c = f.trims[3].curve[0][0];
    let g = DVec3::new(c[0], c[1], c[2]);
    let x_g = g.dot(e1);
    for dx in [-band / 2.0, band / 2.0, -0.25, 0.25] {
        let q = (e1 * (x_g + dx) + e2 * (LENGTH / 2.0)).to_array();
        assert_eq!(s02_cylinder(&f, q), 1, "dx = {dx}");
    }
}

#[test]
fn columns_past_the_edge_count_the_sheets_above() {
    let f = half_cylinder(1.0 / 1024.0, false);
    let [e1, e2, e3] = frame();
    // Beside the cylinder: nothing. Above it: nothing. Below it, across
    // the axis from the edge: the lower sheet there (at −95.7°) is past
    // the edge, outside the face, the upper one (at 95.7°) inside it,
    // facing up.
    for (q, want) in [
        (e1 * 6.0 + e2 * 4.0, 0),
        (e1 * 0.5 + e2 * 4.0 + e3 * 6.0, 0),
        (e1 * -0.5 + e2 * 4.0 + e3 * -6.0, 1),
    ] {
        assert_eq!(s02_cylinder(&f, q.to_array()), want, "q = {q:?}");
    }
}

// ---------------------------------------------------------------------
// A vertex on a tile corner (the boxes round it in four tiles).

/// A triangle of `B` on the plane `z = 0`, `(u, v) ↦ (u, v, 0)` over the
/// tiles `[0, 2)²`, its vertex `W` on the tile corner `(1, 1)`; another
/// with `W` on the tile line `u = 1`.
const TRI: [[f64; 2]; 3] = [[1.0, 1.0], [1.7, 1.3], [1.3, 1.7]];
const TRI_LINE: [[f64; 2]; 3] = [[1.0, 1.15], [1.7, 1.3], [1.3, 1.7]];

fn triangle(band: f64, tri: [[f64; 2]; 3]) -> FaceData {
    let mut patches = Vec::new();
    for i in 0..2 {
        for j in 0..2 {
            let mut net = Vec::new();
            for a in 0..3 {
                for b in 0..3 {
                    let (u, v) = (i as f64 + a as f64 / 2.0, j as f64 + b as f64 / 2.0);
                    net.push([u, v, 0.0, 1.0]);
                }
            }
            patches.push(Patch::from_affine(2, 2, net, &first()).unwrap());
        }
    }
    let p3 = |p: [f64; 2]| [p[0], p[1], 0.0];
    // Edges 1: V1 → V2, 2: V2 → V3, 3: V1 → V3 (against the loop).
    let tri = |k: usize, a: usize, b: usize, reversed: bool| TrimData {
        coedge: CoedgeId(k as u32),
        edge: EdgeId(k as u32),
        reversed,
        spans: vec![pline(tri[a], tri[b])],
        ends: [VertId(a as u32 + 1), VertId(b as u32 + 1)],
        band,
        curve: vec![line(p3(tri[a]), p3(tri[b]))],
        maps: whole(1),
    };
    FaceData {
        operand: Operand::B,
        id: FaceId(2),
        tiles: [0..2, 0..2],
        patches,
        flipped: false,
        trims: vec![tri(1, 0, 1, false), tri(2, 1, 2, false), tri(3, 0, 2, true)],
        // Counted as a curved face is: refined till height-clear or small
        // (the corner pieces' rules under test, no hit records read).
        nets: None,
    }
}

/// Whether `p` is inside the triangle, `None` within `m` of its sides.
fn in_triangle(tri: [[f64; 2]; 3], p: [f64; 2], m: f64) -> Option<bool> {
    let mut all = true;
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let c = (dx * (p[1] - a[1]) - dy * (p[0] - a[0])) / (dx * dx + dy * dy).sqrt();
        if c.abs() < m {
            return None;
        }
        all &= c > 0.0;
    }
    Some(all)
}

/// Random and near columns round the triangle's vertex `W`: each count
/// the exact one.
fn check_triangle(tri: [[f64; 2]; 3]) {
    let band = 1.0 / 4096.0;
    let f = triangle(band, tri);
    let mut order = Order::default();
    for (k, p) in tri.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), [p[0], p[1], 0.0]);
    }
    for t in &f.trims {
        order.edges.insert((Operand::B, t.edge), t.ends);
    }
    let (sh, ft, o) = no_reads(&order);
    let reads = Records {
        shadows: &sh,
        feet: &ft,
        order: o,
    };
    let point = Point::vert(Operand::A, VertId(7));
    let mut rng = varde_testkit::rng::Rng::new(0x751);
    let (mut inside, mut all) = (0, 0);
    // Columns passing a few bands from `W`, inside the corner and beside
    // it, above and below the plane; then at random round `W`.
    let mut near = Vec::new();
    for r in [0.02, 0.4, 1.0, 1.7, 2.6] {
        for (dx, dy) in [
            (1.0, 1.0),
            (1.0, -0.2),
            (-0.3, 1.0),
            (2.0, 1.5),
            (-1.0, -0.5),
        ] {
            for z in [0.01, -0.01] {
                let (hx, hy) = (tri[0][0] + r * band * dx, tri[0][1] + r * band * dy);
                near.push([hx + z / UP.z * UP.x, hy + z / UP.z * UP.y, z]);
            }
        }
    }
    let mut k = 0;
    // The near columns, then random ones (more in the full run).
    let total = varde_testing::pick(52, 72);
    while all < total {
        let q = if k < near.len() {
            k += 1;
            near[k - 1]
        } else {
            [
                tri[0][0] + rng.range(-0.03, 0.1),
                tri[0][1] + rng.range(-0.03, 0.1),
                rng.range(-0.05, 0.05),
            ]
        };
        // The column's hit on the plane.
        let t = -q[2] / UP.z;
        let hit = [q[0] + t * UP.x, q[1] + t * UP.y];
        let Some(truth) = in_triangle(tri, hit, 0.01 * band) else {
            continue;
        };
        if q[2].abs() < 4.0 * band {
            continue;
        }
        all += 1;
        inside += usize::from(truth);
        let rays = |t: &TrimData| chain(&t.curve, q, Operand::A);
        let p = f
            .pieces(&order, point, q, &rays, &reads, &Sides::new())
            .unwrap_or_else(|e| panic!("q = {q:?}: {e:?}"));
        let want = i64::from(truth && q[2] < 0.0);
        let got = p.count(&reads, &Heights::new(), &[]).unwrap();
        assert_eq!(got, want, "q = {q:?}");
    }
    assert!(inside > 5, "{inside} of {all} inside");
}

#[test]
fn a_corner_on_a_tile_corner_spans_the_boxes_round_it() {
    check_triangle(TRI);
}

#[test]
fn a_corner_on_a_tile_line_spans_the_boxes_round_it() {
    check_triangle(TRI_LINE);
}

#[test]
fn a_straight_edge_s_ray_record_as_a_chain() {
    let rec = RayRec {
        crossing: Some(Sign::Neg),
    };
    let (a, b) = ([0.0, -1.0, 0.0], [0.0, 3.0, 0.0]);
    // `ν·(q − a)/ν·(b − a)`: `ν = (−3, 514, −48)`, so a quarter of the
    // way along.
    let c = line_chain(&rec, a, b, [0.0, 0.0, 0.0]);
    assert_eq!(c.crossings.len(), 1);
    let x = c.crossings[0].crossing;
    assert_eq!(x.sign, Sign::Neg);
    assert!(x.t[0] <= 0.25 && 0.25 <= x.t[1] && x.t[1] - x.t[0] < 1e-12);
    // At an end exactly; none; an edge along the line.
    let c = line_chain(&rec, a, b, [0.0, -1.0, 0.0]);
    assert_eq!(c.crossings[0].crossing.t[0], 0.0);
    assert!(
        line_chain(&RayRec { crossing: None }, a, b, [0.0; 3])
            .crossings
            .is_empty()
    );
    let along = [16.0, 0.0, -1.0];
    let c = line_chain(&rec, [0.0; 3], along, [0.0; 3]);
    assert_eq!(c.crossings[0].crossing.t, [0.0, 1.0]);
}

// ---------------------------------------------------------------------
// A small face on a plane's one huge tile.

/// Half a plane tile's width: the tile is `4·10⁶` wide (a plane
/// lattice's base cell).
const HALF: f64 = 2_097_152.0;

/// A square face `1/8` wide trimmed from a plane tile `4·10⁶` wide, its
/// plane spanned by `a3` and `b3` (unit, orthogonal) through the
/// origin, the tile's middle, its corners at `a3·s + b3·t` for
/// `s ∈ c[0] + {0, 1/8}`, `t ∈ c[1] + {0, 1/8}`, edges `1/8` long with
/// bands `band`, its corners' positions and edges in `order`.
fn huge_tile_square(c: [f64; 2], a3: DVec3, b3: DVec3, band: f64, order: &mut Order) -> FaceData {
    let mut net = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let [s, t] = [i, j].map(|k| -HALF + HALF * f64::from(k));
            let p = a3 * s + b3 * t;
            net.push([p.x, p.y, p.z, 1.0]);
        }
    }
    let patch = Patch::from_affine(2, 2, net, &first()).unwrap();
    let uv = |s: f64, t: f64| [(s + HALF) / (2.0 * HALF), (t + HALF) / (2.0 * HALF)];
    let corners =
        [[0.0, 0.0], [0.125, 0.0], [0.125, 0.125], [0.0, 0.125]].map(|[s, t]| [c[0] + s, c[1] + t]);
    let pos = |c: [f64; 2]| (a3 * c[0] + b3 * c[1]).to_array();
    let trims = (0..4)
        .map(|k| {
            let (p, q) = (k, (k + 1) % 4);
            let (lo, hi) = if p < q { (p, q) } else { (q, p) };
            TrimData {
                coedge: CoedgeId(k as u32 + 1),
                edge: EdgeId(k as u32 + 1),
                reversed: p > q,
                spans: vec![pline(
                    uv(corners[lo][0], corners[lo][1]),
                    uv(corners[hi][0], corners[hi][1]),
                )],
                ends: [VertId(lo as u32 + 1), VertId(hi as u32 + 1)],
                band,
                curve: vec![line(pos(corners[lo]), pos(corners[hi]))],
                maps: whole(1),
            }
        })
        .collect();
    let patches = vec![patch];
    let f = FaceData {
        operand: Operand::B,
        id: FaceId(1),
        tiles: [0..1, 0..1],
        nets: Some(nets_of(&patches)),
        patches,
        flipped: false,
        trims,
    };
    for (k, c) in corners.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), pos(*c));
    }
    for t in &f.trims {
        order.edges.insert((Operand::B, t.edge), t.ends);
    }
    f
}

/// A planar face of `B` with straight edges (`n·x = c`, `n` its
/// outward normal) and a vertex of `A` at `q`, for the records the
/// vertex's count reads: its feet on the edges (its near radius and the
/// edges' `3·band`), its point–vertex records and its hits' heights.
struct Planar<'a> {
    face: &'a FaceData,
    order: &'a Order,
    /// The face's plane; `None` for a curved face (its hits exact on its
    /// nets all the same).
    plane: Option<(DVec3, f64)>,
    q: [f64; 3],
    band: f64,
    /// Where the records keep the column's hits, if they do.
    memo: Option<&'a crate::records::height::model::HitMemo>,
}

impl Planar<'_> {
    fn end(&self, g: EdgeId, k: usize) -> DVec3 {
        let v = self.order.edges[&(Operand::B, g)][k];
        DVec3::from_array(self.order.pos[&(Operand::B, v)])
    }
}

impl crate::records::shadow::ShadowGeometry for Planar<'_> {
    fn point(&self, p: Point) -> crate::perturb::Moving {
        match (p.operand, p.q) {
            (Operand::A, _) => {
                crate::perturb::Moving::of_a(DVec3::from_array(self.q), Sign::Pos, R_HAT)
            }
            (Operand::B, Query::Vert(w)) => {
                crate::perturb::Moving::fixed(DVec3::from_array(self.order.pos[&(Operand::B, w)]))
            }
            _ => panic!("no such point {p:?}"),
        }
    }

    fn near_radius(&self, _: Point) -> f64 {
        3.0 * self.band
    }

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.order.edges[&(operand, g)]
    }

    fn line(&self, _: Point, g: EdgeId) -> crate::perturb::Seg {
        crate::perturb::Seg::fixed(self.end(g, 0), self.end(g, 1))
    }
}

impl crate::records::feet::FeetGeometry for Planar<'_> {
    fn curve(&self, _: Operand, g: EdgeId) -> crate::records::feet::EdgeCurve<'_> {
        crate::records::feet::EdgeCurve::Line([self.end(g, 0), self.end(g, 1)])
    }

    fn near_radius_of(&self, _: Operand, _: EdgeId) -> f64 {
        3.0 * self.band
    }
}

impl crate::records::height::model::HeightModel for Planar<'_> {
    fn plane(&self, _: Operand, _: FaceId) -> Option<(DVec3, f64)> {
        self.plane
    }

    fn hit_face(&self, _: Operand, _: FaceId) -> Option<crate::records::height::hit::HitFace<'_>> {
        self.face.hit_face(self.order)
    }

    fn hit_memo(&self) -> Option<&crate::records::height::model::HitMemo> {
        self.memo
    }

    fn plane_rates(
        &self,
        _: Point,
        _: FaceId,
        _: DVec3,
    ) -> [crate::perturb::Rate; crate::num::exact::ORDERS] {
        [crate::perturb::Rate::ZERO; crate::num::exact::ORDERS]
    }

    fn foot_point(
        &self,
        _: Point,
        g: EdgeId,
        _: crate::records::keys::FootId,
    ) -> Option<crate::perturb::Moving> {
        // The edge's point nearest `q` (each edge has one foot).
        let (a, b) = (self.end(g, 0), self.end(g, 1));
        let t =
            ((DVec3::from_array(self.q) - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        Some(crate::perturb::Moving::fixed(a + (b - a) * t))
    }

    fn curved_target<S: crate::perturb::Curved>(
        &self,
        _: crate::records::keys::HKey,
        _: &S,
    ) -> Option<[S; 3]> {
        None
    }

    fn face_at<S: crate::perturb::Curved>(
        &self,
        _: Point,
        _: FaceId,
        _: &[S; 3],
        _: &S,
    ) -> Option<crate::records::height::model::FaceAt<S>> {
        None
    }
}

/// The count of `f` (on the plane `n·x = c`, `n` its outward normal,
/// its edges straight with bands `band`) against a vertex of `A` at `q`,
/// which must settle (at the first level unless `ladder`, else at any),
/// with the records it reads.
fn huge_tile_count(
    f: &FaceData,
    order: &Order,
    plane: (DVec3, f64),
    q: [f64; 3],
    band: f64,
    ladder: bool,
) -> i64 {
    planar_count(f, order, plane, q, band, ladder, [1.0, 1.0]).0
}

/// [`huge_tile_count`] with the box splits at `scale` (another
/// partition), and the height keys it read: the hits' own, `q` lying
/// exactly on none of the face's vertices or edges (no foot or near
/// vertex keys a height).
fn planar_count(
    f: &FaceData,
    order: &Order,
    plane: (DVec3, f64),
    q: [f64; 3],
    band: f64,
    ladder: bool,
    scale: [f64; 2],
) -> (i64, Vec<HeightKey>) {
    let geom = Planar {
        face: f,
        order,
        plane: Some(plane),
        q,
        band,
        memo: None,
    };
    model_count(f, &geom, ladder, scale)
}

/// The count of `f` on `geom` against a vertex of `A` at `geom.q`
/// ([`planar_count`]), and the height keys it read.
fn model_count(
    f: &FaceData,
    geom: &Planar<'_>,
    ladder: bool,
    scale: [f64; 2],
) -> (i64, Vec<HeightKey>) {
    let q = geom.q;
    model_count_with(f, geom, ladder, scale, &|t: &TrimData| {
        chain(&t.curve, q, Operand::A)
    })
}

/// [`model_count`] with the edges' ray records from `rays`.
fn model_count_with(
    f: &FaceData,
    geom: &Planar<'_>,
    ladder: bool,
    scale: [f64; 2],
    rays: &dyn Fn(&TrimData) -> ChainRec,
) -> (i64, Vec<HeightKey>) {
    let (order, q) = (geom.order, geom.q);
    let point = Point::vert(Operand::A, VertId(7));
    let mut feet = NearFeet::new();
    let keys = f.trims.iter().map(|t| crate::records::keys::RayKey {
        q: point,
        g: t.edge,
    });
    feet.fill(keys, geom, &mut Seq).expect("feet");
    let mut sh = Shadows::new();
    let vv = order
        .pos
        .keys()
        .map(|&(_, w)| crate::records::keys::VvKey::of(point, w).key);
    sh.fill_vv(vv, geom, &mut Seq)
        .expect("point-vertex records");
    let reads = Records {
        shadows: &sh,
        feet: &feet,
        order,
    };
    let sides = Sides::new();
    let p = if ladder {
        f.query_scaled(scale, None, order, point, q, rays, &|fq, _| {
            count_pieces(fq, &sides, &reads, &Charge::new(u64::MAX)).0
        })
    } else {
        f.query_scaled(scale, None, order, point, q, rays, &|fq, edges| {
            let out = face_pieces(fq, &sides, &reads)?;
            assert_eq!(out.degree(), ray_degree(fq.face, edges).unwrap());
            Ok(out)
        })
    };
    let p = p.unwrap_or_else(|e| panic!("q = {q:?}: {e:?}"));
    let wanted = p.wanted(&reads, &[]).expect("keys");
    for k in &wanted {
        assert!(matches!(k.h, HKey::Hit { .. }), "q = {q:?}: {k:?}");
    }
    let mut heights = Heights::new();
    let model = crate::records::height::model::ModelHeights { model: geom };
    let band = crate::num::decide::Band::curved(0.0, 1.0 / 1_073_741_824.0).unwrap();
    heights
        .fill(wanted.clone(), &model, band, &mut Seq)
        .expect("heights");
    let n = p.count(&reads, &heights, &[]).expect("records filled");
    (n, wanted)
}

/// Columns `10⁻⁸` from an edge of a small face on a huge plane tile, a
/// hair below and above it: under another partition of the face (its
/// boxes split along other sides first) the count and the hit keys it
/// reads are the same: a hit is a point of the stored data, whatever
/// boxes found it.
#[test]
fn two_refinements_give_one_hit_key() {
    let up = UP.normalize();
    let a3 = R_HAT.cross(UP).normalize();
    let b3 = (up + a3.cross(up).normalize() * 0.3).normalize();
    let n = a3.cross(b3).normalize();
    let c = [0.0371, 0.0529];
    let mut order = Order::default();
    let f = huge_tile_square(c, a3, b3, 1e-6, &mut order);
    let mut read = 0;
    for (t, z) in [(0.03, 2e-6), (0.07, -2e-6), (0.1, 3e-7), (0.04, -1e-6)] {
        let hit = a3 * (c[0] + 0.125 - 1e-8) + b3 * (c[1] + t);
        let q = (hit + up * z).to_array();
        let (n1, k1) = planar_count(&f, &order, (n, 0.0), q, 1e-6, true, [1.0, 1.0]);
        let (n2, k2) = planar_count(&f, &order, (n, 0.0), q, 1e-6, true, [7.0, 1.0 / 7.0]);
        assert_eq!((n1, &k1), (n2, &k2), "q = {q:?}");
        read += k1.len();
    }
    assert!(read > 0, "some piece near the column read a hit");
}

/// Columns a hair above and below the half-cylinder's upper sheet, on
/// two of its tiles, with its column hits exact on its tiles:
/// under another partition the count and the hit keys it reads are the
/// same, each the hit's own (a point of the stored tiles, whatever boxes
/// found it); the counts are the sheets above.
#[test]
fn two_refinements_give_one_hit_key_on_a_curved_tile() {
    let band = 1.0 / 1024.0;
    let f = half_cylinder(band, true);
    let order = cylinder_order(&f);
    let [e1, e2, e3] = frame();
    let mut read = 0;
    // At 30° and 60° (tile 1) and at 95° (tile 2), the upper sheet
    // facing up; `z` along `ê` from it.
    for (deg, z) in [
        (30.0, 1e-13),
        (30.0, -1e-13),
        (60.0, -2e-13),
        (95.0, 3e-13),
        (95.0, -1e-7),
    ] {
        let t = f64::to_radians(deg);
        let (sin, cos) = trig::sin_cos(t);
        let on = e1 * (RADIUS * cos) + e3 * (RADIUS * sin) + e2 * (LENGTH / 2.0);
        let q = (on + UP.normalize() * z).to_array();
        let geom = Planar {
            face: &f,
            order: &order,
            plane: None,
            q,
            band,
            memo: None,
        };
        let (n1, k1) = model_count(&f, &geom, true, [1.0, 1.0]);
        let (n2, k2) = model_count(&f, &geom, true, [7.0, 1.0 / 7.0]);
        // The records reading the column's hits kept once for the point
        // and face: the same records, the hits found once.
        let memo = crate::records::height::model::HitMemo::default();
        let kept = Planar {
            memo: Some(&memo),
            ..geom
        };
        assert_eq!(model_count(&f, &kept, true, [1.0, 1.0]), (n1, k1.clone()));
        assert_eq!(memo.kept(), usize::from(!k1.is_empty()), "q = {q:?}");
        // One count, and whichever partition's near piece reads the hit
        // (the other's boxes may end clear of `q`'s height), one key.
        assert_eq!(n1, n2, "q = {q:?}");
        let mut keys: Vec<HeightKey> = k1.iter().chain(&k2).copied().collect();
        keys.dedup();
        assert!(keys.len() <= 1, "q = {q:?}: {keys:?}");
        // Below the upper sheet, which faces up, it counts; above it not.
        assert_eq!(n1, i64::from(z < 0.0), "deg {deg}, z {z}");
        read += k1.len().min(k2.len());
    }
    assert!(read > 0, "some piece near the column read a hit");
}

/// A square face `1/8` wide trimmed from a plane tile `4·10⁶` wide (a
/// plane lattice's base cell), tilted to `UP` along both its directions,
/// and points `10⁻⁵` in front of and behind it: the counts settle at the
/// first level and are the plane's crossings by the column. Its boxes
/// take about 25 halvings in each direction, past what a cap on a box's
/// splits in both directions together would allow.
#[test]
fn a_small_face_on_a_huge_tile_settles() {
    let a3 = (R_HAT.normalize() + UP.normalize()).normalize();
    let b3 = (NU.normalize() + UP.normalize()).normalize();
    let b3 = (b3 - a3 * a3.dot(b3)).normalize();
    let n = a3.cross(b3).normalize();
    let mut order = Order::default();
    let f = huge_tile_square([0.0, 0.0], a3, b3, 1e-6, &mut order);
    for side in [1.0, -1.0] {
        let q = (a3 * 0.0625 + b3 * 0.0625 + n * (1e-5 * side)).to_array();
        // The column meets the plane above `q` on the side the normal
        // faces down from, where the face faces `sign(n·UP)`.
        let up = n.dot(UP);
        let want = if side * up < 0.0 {
            up.signum() as i64
        } else {
            0
        };
        assert_eq!(
            huge_tile_count(&f, &order, (n, 0.0), q, 1e-6, false),
            want,
            "side {side}"
        );
    }
}

/// Columns passing `10⁻⁸` inside and outside an edge of a small face on a
/// huge plane tile, along the edge and next to its ends, a hair to a few
/// hundredths below and above the face, on planes from steep (`UP` in
/// the plane: the face's shadow a line) to flat: the strip runs round
/// the column, whose cuts lie within `10⁻⁸` of it, settle (a level up
/// where the `f64` hulls of so small a part of so large a tile can't
/// tell `10⁻⁸`) and count the plane's crossing. The splits there must not use
/// up the boxes' depth: a split shifted off the query by `⅛` or `1/16`
/// of a box that isn't aligned to its dyadic grid would add three or four
/// bits of depth for a halving, and a box `20` splits down would be as
/// deep as boxes go while still millimetres long.
#[test]
fn runs_with_cuts_next_to_the_column_settle_on_a_huge_tile() {
    let up = UP.normalize();
    let mut rng = varde_testkit::rng::Rng::new(0x1d71);
    // Planes along `UP` tilted by `tilt` (`a3` across `UP`), then planes
    // along `z` (about `0.11` from `UP`) through the horizontal `across`.
    let planes = [0.0, 1e-3, 0.3, 1.0, 4.0]
        .map(|tilt| (tilt, None))
        .into_iter()
        .chain([[3.0, 4.0], [-1.0, 5.0], [7.0, -2.0]].map(|xy| (f64::NAN, Some(xy))));
    for (tilt, across) in planes {
        let (a3, b3) = match across {
            None => {
                let a3 = R_HAT.cross(UP).normalize();
                let side = a3.cross(up).normalize();
                (a3, (up + side * tilt).normalize())
            }
            Some([x, y]) => (DVec3::new(x, y, 0.0).normalize(), DVec3::Z),
        };
        let n = a3.cross(b3).normalize();
        // The corners off the tile's middle, so none lies on a box's side
        // (`V1` would be on the tile's first split).
        let c = [0.0371, 0.0529];
        let mut order = Order::default();
        let f = huge_tile_square(c, a3, b3, 1e-6, &mut order);
        let facing = n.dot(UP);
        for _ in 0..varde_testing::pick(12, 48) {
            // On an edge, along it or next to an end, `10⁻⁸` inside the
            // face or outside: `(s, t)` from the corner `V1`, the edge
            // `t = 0`, turned by quarters about the square's middle.
            let along = match rng.next_u64() % 3 {
                0 => rng.range(0.01, 0.115),
                1 => rng.range(1e-7, 1e-5),
                _ => 0.125 - rng.range(1e-7, 1e-5),
            };
            let inside = rng.next_u64().is_multiple_of(2);
            let d = if inside { 1e-8 } else { -1e-8 };
            let [s, t] = match rng.next_u64() % 4 {
                0 => [along, d],
                1 => [0.125 - d, along],
                2 => [0.125 - along, 0.125 - d],
                _ => [d, 0.125 - along],
            };
            let hit = a3 * (c[0] + s) + b3 * (c[1] + t);
            // Along the column: `q` below the hit (`z < 0`: the plane above
            // `q`) or above it.
            let z = [-2.0, -3e-2, -1e-5, 1e-5, 3e-2, 2.0][(rng.next_u64() % 6) as usize];
            let (q, want) = if tilt == 0.0 {
                // A plane along `UP`: the column `10⁻⁸` off it crosses
                // nothing.
                let off = if rng.next_u64().is_multiple_of(2) {
                    1e-8
                } else {
                    -1e-8
                };
                (hit + up * z + n * off, 0)
            } else if inside && z < 0.0 {
                (hit + up * z, facing.signum() as i64)
            } else {
                (hit + up * z, 0)
            };
            let q = q.to_array();
            assert_eq!(
                huge_tile_count(&f, &order, (n, 0.0), q, 1e-6, true),
                want,
                "tilt {tilt}, across {across:?}, (s, t) ({s}, {t}), inside {inside}, z {z}"
            );
        }
    }
}

/// A column `1.5·10⁻⁶` above and below a steep plane (`UP` tilted by
/// `0.3` into it), meeting it `10⁻⁸` inside an edge of a small face on
/// a huge tile and `2.7·10⁻⁶` from a corner, within the near radius of
/// both: the strip's boxes holding the crossing are height-clear (below
/// the upper point, above the lower one) next to near ones whose cut no
/// window settles. Their degree is the crossing's, so it must count by
/// their own height, not by a key the corner's vertex names (above the
/// upper point while the plane is below it).
#[test]
fn clear_boxes_holding_the_crossing_next_to_a_near_run_count_by_their_height() {
    let up = UP.normalize();
    let a3 = R_HAT.cross(UP).normalize();
    let b3 = (up + a3.cross(up).normalize() * 0.3).normalize();
    let n = a3.cross(b3).normalize();
    let c = [0.0371, 0.0529];
    let mut order = Order::default();
    let f = huge_tile_square(c, a3, b3, 1e-6, &mut order);
    let hit = a3 * (c[0] + 0.125 - 1e-8) + b3 * (c[1] + 0.12499728797435371);
    // Below the plane the column crosses it, facing down.
    let facing = n.dot(UP).signum() as i64;
    for (z, want) in [(1.5e-6, 0), (-1.5e-6, facing)] {
        let q = (hit + up * z).to_array();
        assert_eq!(
            huge_tile_count(&f, &order, (n, 0.0), q, 1e-6, true),
            want,
            "z {z}"
        );
    }
}

/// Columns `1.1·10⁻⁶` above a plane at 45° to `UP` (inside its near
/// radius), meeting it `10⁻⁸` inside either side edge of a small face on
/// a huge tile: the near strip's cut walks past one dropped box at a
/// time, each join a round of its own. Rounds that only join refine
/// nothing and don't count against [`MAX_ROUNDS`], so the count settles
/// (the column misses the plane above `q`).
#[test]
fn joining_dropped_boxes_one_by_one_settles() {
    let up = UP.normalize();
    let a3 = R_HAT.cross(UP).normalize();
    let b3 = (up + a3.cross(up).normalize()).normalize();
    let n = a3.cross(b3).normalize();
    let c = [0.0371, 0.0529];
    let mut order = Order::default();
    let f = huge_tile_square(c, a3, b3, 1e-6, &mut order);
    for [s, t] in [
        [0.125 - 1e-8, 0.08369934805544053],
        [1e-8, 0.0781092637202587],
    ] {
        let hit = a3 * (c[0] + s) + b3 * (c[1] + t);
        let q = (hit + up * 1.1e-6).to_array();
        assert_eq!(
            huge_tile_count(&f, &order, (n, 0.0), q, 1e-6, true),
            0,
            "(s, t) ({s}, {t})"
        );
    }
}

/// A box whose p-span's control points reach over it while the span
/// itself passes below or above (boxes the hulls don't separate): the
/// trims miss it exactly, and it is core or outside by their winding
/// about its centre; a box the span crosses, or touches, is not clear.
#[test]
fn a_box_the_trims_miss_is_core_or_outside() {
    // The triangle's first side bowed up: from (1.25, 1.125) to (1.75,
    // 1.125) through the control point (1.5, 1.625), its top `(1.5,
    // 1.375)`; over `u ∈ [1.4375, 1.5625]` it runs at `v ∈ [1.359, 1.375]`.
    let tri = [[1.25, 1.125], [1.75, 1.125], [1.5, 1.9]];
    let mut f = triangle(1.0 / 4096.0, tri);
    f.trims[0].spans = vec![[[1.25, 1.125, 1.0], [1.5, 1.625, 1.0], [1.75, 1.125, 1.0]]];
    let mut order = Order::default();
    for (k, p) in tri.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), [p[0], p[1], 0.0]);
    }
    for t in &f.trims {
        order.edges.insert((Operand::B, t.edge), t.ends);
    }
    let met = [SpanRef {
        coedge: f.trims[0].coedge,
        span: 0,
    }];
    let dy = |n: u64, e: u32| Dyadic::new(n, e).unwrap();
    // Tile `[1, 1]`'s local `u ∈ [7/16, 9/16]`, `v` in sixty-fourths.
    let b = |v0: u64, v1: u64| {
        BoxKey::new(
            Operand::B,
            f.id,
            [1, 1],
            Extent::new(dy(7, 4), dy(9, 4)).unwrap(),
            Extent::new(dy(v0, 6), dy(v1, 6)).unwrap(),
        )
    };
    let point = Point::vert(Operand::A, VertId(7));
    let rays = |t: &TrimData| chain(&t.curve, [0.0; 3], Operand::A);
    let class = |bx: BoxKey| {
        f.query(&order, point, [1.5, 1.5, 1.0], &rays, &|fq, _| {
            clear_class(fq.face, &bx, &met)
        })
    };
    // Above the bow, inside the face; below it, outside.
    assert_eq!(class(b(26, 30)), Ok(Some(true)));
    assert_eq!(class(b(10, 14)), Ok(Some(false)));
    // Across the bow, and touching its top from below and from above.
    assert_eq!(class(b(22, 26)), Ok(None));
    assert_eq!(class(b(16, 24)), Ok(None));
    assert_eq!(class(b(24, 28)), Ok(None));
}

/// The triangle face with its first side bowed up (a parabola), its
/// edge's curve the p-curve's image exactly (no ribbon), tilted so that
/// its shadow at its parameter `½` runs along `r̂`; the column of a point
/// of `A` passing just inside the bow there, a hair above or below the
/// plane, the point's ray line tangent to the bow's shadow a little
/// ahead of it, as the curved ray records keep it: a tangency whose two
/// moved crossings cancel, no crossing listed. The strip runs round the
/// column are cut next to the tangency, each cut ordered against it
/// (a cut inside it is unordered and moves:
/// `a_cut_inside_a_cancelled_tangency_is_unordered` in the pieces'
/// tests), and the face's count is the oracle's.
#[test]
fn a_cancelled_tangency_next_to_the_column_counts_as_the_oracle() {
    use varde_testkit::ray_oracle::{Chain, Coedge, ExactFace, Face, Shift, UvChain};
    // `P₂ − P₀ ∥ (514, 3)`, so `ν·C′(½) = −3·514 + 514·3 = 0`, up to
    // the rounding of `P₂`.
    let p2 = [1.75, 1.125 + 1.5 / 514.0];
    let tri = [[1.25, 1.125], p2, [1.5, 1.9]];
    let mut f = triangle(1.0 / 4096.0, tri);
    let bow: UvSpan = [[1.25, 1.125, 1.0], [1.5, 1.625, 1.0], [p2[0], p2[1], 1.0]];
    f.trims[0].spans = vec![bow];
    f.trims[0].curve = vec![bow.map(|p| [p[0], p[1], 0.0, 1.0])];
    f.nets = Some(nets_of(&f.patches));
    let mut order = Order::default();
    for (k, p) in tri.iter().enumerate() {
        order
            .pos
            .insert((Operand::B, VertId(k as u32 + 1)), [p[0], p[1], 0.0]);
    }
    for t in &f.trims {
        order.edges.insert((Operand::B, t.edge), t.ends);
    }
    let c = DVec3::new(
        (1.25 + 2.0 * 1.5 + p2[0]) / 4.0,
        (1.125 + 2.0 * 1.625 + p2[1]) / 4.0,
        0.0,
    );
    let oracle = ExactFace::new(&Face {
        tiles: (0..2)
            .flat_map(|i| (0..2).map(move |j| [i, j]))
            .zip(f.nets.clone().unwrap())
            .collect(),
        loops: vec![
            f.trims
                .iter()
                .map(|t| Coedge {
                    pcurve: UvChain {
                        spans: t.spans.clone(),
                        knots: vec![0.0, 1.0],
                    },
                    curve: Chain {
                        spans: t.curve.clone(),
                        knots: vec![0.0, 1.0],
                    },
                    t_range: [0.0, 1.0],
                    reversed: t.reversed,
                })
                .collect(),
        ],
    });
    let up = [UP.x, UP.y, UP.z];
    let mut counted = 0;
    for s in [1e-7, 3e-8, 3e-7] {
        for lambda in [1e-8, -1e-8] {
            let q = (c + R_HAT * s + UP * lambda).to_array();
            let rays = |t: &TrimData| {
                let mut r = chain(&t.curve, q, Operand::A);
                if t.coedge == CoedgeId(1) {
                    r.crossings.clear();
                    r.touches.push((0, [0.5 - 1e-9, 0.5 + 1e-9]));
                }
                r
            };
            let geom = Planar {
                face: &f,
                order: &order,
                plane: Some((DVec3::Z, 0.0)),
                q,
                band: 1.0 / 4096.0,
                memo: None,
            };
            let (n, _) = model_count_with(&f, &geom, true, [1.0, 1.0], &rays);
            let truth = oracle
                .ray(&Shift::NONE, q, &Shift::NONE, up)
                .expect("settled");
            assert!(!truth.tie);
            assert_eq!(n, truth.count, "s {s}, λ {lambda}");
            counted += usize::from(n != 0);
        }
    }
    assert_eq!(counted, 3, "the columns pass inside the face");
}
