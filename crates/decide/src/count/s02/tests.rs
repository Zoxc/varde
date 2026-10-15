//! The count of a face's layers above or below a point, on whole faces:
//! a half-cylinder overhanging its own edge (the column near the edge's
//! shadow hits only the far sheet, whose own height counts), a cube's
//! faces at a vertex of the other operand on the cube's corner (every
//! face at the corner reads the one point–vertex record, and the count
//! over the cube is the moved point's winding), the fold rule and the
//! straight edges' ray records.

use std::collections::BTreeMap;

use glam::DVec3;

use super::*;
use crate::bezier::HomPatch;
use crate::count::boxes::UvSpan;
use crate::count::testing::*;
use crate::ids::{EdgeId, FaceId, VertId};
use crate::num::bounded::Bounded;
use crate::num::decide::Sign;
use crate::num::trig;
use crate::records::feet::NearFeet;
use crate::records::height::Records;
use crate::records::keys::{Operand, Query};
use crate::records::ray::Span;
use crate::records::shadow::Shadows;

mod budget;
mod cube;
mod fold;

/// The signed crossings of the ray of a point of `of` at `q` with a
/// chain of rational quadratic spans (affine points and weights): per
/// span the roots of `ν·(P − q·W)`, a quadratic in closed form, ahead
/// along the ray, signed `sign(ρ × T)`.
fn chain(spans: &[Span], q: [f64; 3], of: Operand) -> ChainRec {
    let dot = |a: [f64; 3], p: [f64; 4]| a[0] * p[0] + a[1] * p[1] + a[2] * p[2];
    let (r, n) = ([R_HAT.x, R_HAT.y, R_HAT.z], [NU.x, NU.y, NU.z]);
    let rho = if of == Operand::A { -1.0 } else { 1.0 };
    let mut out = ChainRec::default();
    for (k, sp) in spans.iter().enumerate() {
        // `a·(P − q·W)` in Bernstein form, then power form.
        let bern = |a: [f64; 3]| {
            let aq = a[0] * q[0] + a[1] * q[1] + a[2] * q[2];
            sp.map(|p| p[3] * (dot(a, p) - aq))
        };
        let power = |[b0, b1, b2]: [f64; 3]| [b0, 2.0 * (b1 - b0), b0 - 2.0 * b1 + b2];
        let [c0, b, a] = power(bern(n));
        let mut ts: Vec<f64> = Vec::new();
        if a == 0.0 {
            if b != 0.0 {
                ts.push(-c0 / b);
            }
        } else {
            let disc = b * b - 4.0 * a * c0;
            if disc > 0.0 {
                let h = -(b + b.signum() * disc.sqrt()) / 2.0;
                ts.extend([h / a, c0 / h]);
            }
        }
        ts.sort_by(f64::total_cmp);
        let [rc, rb, ra] = power(bern(r));
        for t in ts {
            if !(0.0..=1.0).contains(&t) || rho * (rc + rb * t + ra * t * t) <= 0.0 {
                continue;
            }
            // `ρ × T` in the shadow frame `(r̂, ν)`: `ρ_r·T_ν`.
            let tn = b + 2.0 * a * t;
            let sign = if rho * tn > 0.0 { Sign::Pos } else { Sign::Neg };
            out.crossings.push(ChainCrossing {
                span: k as u32,
                crossing: Crossing {
                    t: [clamp(t - 1e-12), clamp(t + 1e-12)],
                    sign,
                },
            });
        }
    }
    out
}

/// `t` clamped to `[0, 1]`.
fn clamp(t: f64) -> f64 {
    t.clamp(0.0, 1.0)
}

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
/// axis, against the patch's, so it is flipped.
fn half_cylinder(band: f64) -> FaceData {
    let mut patches = Vec::new();
    for i in 0..3 {
        let rows: Vec<Span> = [0.0, 0.5, 1.0].map(|v| arc(i, LENGTH * v)).to_vec();
        let mut net = Vec::new();
        for a in 0..3 {
            for row in &rows {
                net.push(row[a]);
            }
        }
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
    assert!(p.near.is_empty(), "no layer near the point");
    p.count(&reads, &LayerTable::new(), &Heights::new())
        .expect("no record read")
}

#[test]
fn an_overhanging_layer_counts_by_its_own_height() {
    let band = 1.0 / 1024.0;
    let f = half_cylinder(band);
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
    let f = half_cylinder(1.0 / 1024.0);
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
        let got = p
            .count(&reads, &LayerTable::new(), &Heights::new())
            .unwrap();
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
