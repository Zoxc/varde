//! The adversarial families: placements that put a vertex, an edge or a
//! face of `A` on or next to a feature of `B` where the counting's rules
//! meet (a band of an edge, a silhouette, a tile line, a fold, an acute or
//! cusp corner, a shared corner, a ridge, coplanar faces), and ties exact
//! in the counting's own records ([`kernel_exact`]), each family a
//! function of a seed.

use glam::DVec3;
use malachite::Rational;
use varde_decide::num::trig;
use varde_testkit::rng::Rng;

use super::runs::Placement;
use super::solids::{Make, Shape, rotation};
use crate::geom::curve::Span3;
use crate::geom::form::{CylinderKey, SphereKey};
use crate::geom::lattice::{Lattice, Level};

const UP: DVec3 = DVec3::new(2.0, 3.0, 32.0);

/// A family: its name and its placements by seed.
pub(super) type Family = (&'static str, fn(u64) -> Placement);

/// The families, by name.
pub(super) const FAMILIES: [Family; 14] = [
    ("vertex within the band of an edge", near_edge),
    ("vertex on a silhouette", on_silhouette),
    ("vertex on a curved face's tile line", on_tile_line),
    ("vertex at a plane lattice's origin", on_plane_origin),
    ("folds inside a face (axis across UP)", folds),
    ("acute corner (2^-6 rad knife wedge)", knife),
    ("roof-ridge pyramid, vertex on the ridge", roof),
    ("cusp corners (arc and its tangents)", cusp),
    ("shared corner (vertex on a vertex)", shared_corner),
    ("overhanging cylinder half", overhang),
    ("exact ties: axis-aligned boxes on a half grid", grid_boxes),
    ("trim through a plane lattice's origin", trim_through_origin),
    ("stacked half cylinders (arcs on one circle)", stacked),
    (KERNEL_EXACT, kernel_exact),
];

/// The name of the family of ties exact in the counting's records.
pub(super) const KERNEL_EXACT: &str =
    "kernel-exact ties: vertices on key planes, rays through edges";

fn make(rng: &mut Rng) -> Make {
    Make {
        pert: rng.range(5e-7, 1e-6),
        pert_seed: rng.next_u64(),
        ids: Some(rng.next_u64()),
        ..Make::PLAIN
    }
}

fn point(rng: &mut Rng, extent: f64) -> DVec3 {
    DVec3::new(
        rng.range(-extent, extent),
        rng.range(-extent, extent),
        rng.range(-extent, extent),
    )
}

/// A box of `A` with a corner at `at`, turned at random.
fn box_at(rng: &mut Rng, at: DVec3) -> Shape {
    let r = rotation(rng);
    let l = [0, 1, 2].map(|_| rng.range(0.6, 1.6));
    let [x, y, z] = [DVec3::X, DVec3::Y, DVec3::Z].map(|a| r * a);
    Shape::Box {
        o: at.to_array(),
        axes: [
            (x * l[0]).to_array(),
            (y * l[1]).to_array(),
            (z * l[2]).to_array(),
        ],
    }
}

fn placement(rng: &mut Rng, a: Shape, b: Shape) -> Placement {
    let (make_a, make_b) = (make(rng), make(rng));
    Placement {
        a,
        b,
        make_a,
        make_b,
        d: rng.direction(),
    }
}

fn random_b(rng: &mut Rng) -> Shape {
    let k = (rng.next_u64() % 3) as u32;
    let at = point(rng, 0.5);
    Shape::random(rng, k, at, 2.0)
}

/// A vertex of `A` at a point of an edge of `B` moved off it by up to a
/// band (or not at all). Not moved, on a curved edge, the point is the
/// curve's inner knot nearest the drawn one: a control point, so exactly
/// on the stored curve (a point evaluated there is a rounding off it, a
/// near coincidence rather than the incidence meant), and `A`'s corner
/// there is kept unsnapped ([`Make::keep_first`]).
fn near_edge(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let b = random_b(&mut rng);
    let mb = make(&mut rng);
    let sb = b.build(&mb);
    let edges = sb.edges();
    let g = edges[(rng.next_u64() % edges.len() as u64) as usize];
    let [t0, t1] = sb.brep.t_range(g);
    let t = rng.range(t0, t1);
    let curve = sb.brep.edge_curve(g);
    let p = curve.eval(t).expect("a point").map(|c| c.v);
    let along = [0.0, 0.25, 0.5, 0.999][(seed % 4) as usize];
    let knot = if along == 0.0 && !sb.straight[g.index()] {
        knot_nearest(curve.spans(), curve.knots(), t)
    } else {
        None
    };
    let p = knot.unwrap_or(p);
    let band = sb.brep.edge_tol(g) + 1e-12;
    let off = rng.direction() * band * along;
    let a = box_at(&mut rng, DVec3::from_array(p) + off);
    let mut out = placement(&mut rng, a, b);
    out.make_b = mb;
    // `A`'s corner on the knot exactly, not snapped off it.
    out.make_a.keep_first = knot.is_some();
    out
}

/// The point of a chain's inner knot nearest the parameter `t`, its
/// spans' shared control point; `None` for a chain of one span.
fn knot_nearest(spans: &[Span3], knots: &[f64], t: f64) -> Option<[f64; 3]> {
    (1..spans.len())
        .min_by(|&i, &j| (knots[i] - t).abs().total_cmp(&(knots[j] - t).abs()))
        .map(|k| {
            let c = spans[k][0];
            [c[0], c[1], c[2]]
        })
}

/// `B` a half cylinder or sphere zone, a vertex of `A` on its outline as
/// seen along `UP` (or a hair off it).
fn on_silhouette(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let at = point(&mut rng, 0.5);
    let up = UP.normalize();
    let p = if seed.is_multiple_of(2) {
        let b = Shape::random(&mut rng, 1, at, 2.0);
        let Shape::HalfCylinder {
            point,
            axis,
            radius,
            z,
            first,
        } = &b
        else {
            unreachable!()
        };
        let lat = Lattice::new(CylinderKey::new(*point, *axis, *radius).unwrap().into()).unwrap();
        let (f, a) = (
            DVec3::from_array(lat.origin()),
            DVec3::from_array(lat.axes()[2]),
        );
        let d1 = quarter(&lat, first + 1);
        let mut n = a.cross(up).normalize();
        if n.dot(d1) < 0.0 {
            n = -n;
        }
        let h = a.dot(DVec3::from_array(*point) - f) + rng.range(z[0], z[1]);
        (f + a * h + n * *radius, b)
    } else {
        let b = Shape::random(&mut rng, 2, at, 2.0);
        let Shape::SphereZone {
            centre,
            axis,
            radius,
            first,
        } = &b
        else {
            unreachable!()
        };
        let lat = Lattice::new(SphereKey::new(*centre, *radius, *axis).unwrap().into()).unwrap();
        let a = DVec3::from_array(lat.axes()[2]);
        let d1 = quarter(&lat, first + 1);
        // A direction ⊥ UP inside the zone's half and latitudes.
        // A direction ⊥ UP inside the zone's half and latitudes: the
        // silhouette circle meets the zone where such a direction exists
        // (it needn't: then the most inside of a few tries).
        let mut best = (f64::NEG_INFINITY, up);
        for _ in 0..64 {
            let m = rng.direction().reject_from(up).normalize();
            let (x, y) = (m.dot(d1) - 0.05, 0.6 - m.dot(a).abs());
            let score = if x < y { x } else { y };
            if score > best.0 {
                best = (score, m);
            }
        }
        let m = best.1;
        (DVec3::from_array(*centre) + m * *radius, b)
    };
    let (p, b) = p;
    let off = up * [0.0, 1e-9, -1e-9, 1e-6][((seed / 2) % 4) as usize];
    let a = box_at(&mut rng, p + off);
    placement(&mut rng, a, b)
}

fn quarter(lat: &Lattice, k: i64) -> DVec3 {
    let [e1, e2, _] = lat.axes().map(DVec3::from_array);
    match k.rem_euclid(4) {
        0 => e1,
        1 => e2,
        2 => -e1,
        _ => -e2,
    }
}

/// A vertex of `A` on a tile line inside a curved face of `B`: the
/// cylinder's ruling between its two angle cells, the sphere zone's
/// equator or its middle meridian.
fn on_tile_line(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let at = point(&mut rng, 0.5);
    let (p, b) = if seed.is_multiple_of(3) {
        let b = Shape::random(&mut rng, 1, at, 2.0);
        let Shape::HalfCylinder {
            point,
            axis,
            radius,
            z,
            first,
        } = &b
        else {
            unreachable!()
        };
        let lat = Lattice::new(CylinderKey::new(*point, *axis, *radius).unwrap().into()).unwrap();
        let (f, a) = (
            DVec3::from_array(lat.origin()),
            DVec3::from_array(lat.axes()[2]),
        );
        let h = a.dot(DVec3::from_array(*point) - f) + rng.range(z[0], z[1]);
        (f + a * h + quarter(&lat, first + 1) * *radius, b)
    } else {
        let b = Shape::random(&mut rng, 2, at, 2.0);
        let Shape::SphereZone {
            centre,
            axis,
            radius,
            first,
        } = &b
        else {
            unreachable!()
        };
        let lat = Lattice::new(SphereKey::new(*centre, *radius, *axis).unwrap().into()).unwrap();
        let a = DVec3::from_array(lat.axes()[2]);
        let (d0, d1) = (quarter(&lat, *first), quarter(&lat, first + 1));
        let th = rng.range(0.1, 1.4);
        let m = if seed % 3 == 1 {
            // The equator.
            (d0 * trig::cos(2.0 * th) + d1 * trig::sin(2.0 * th)).normalize()
        } else {
            // The middle meridian.
            (d1 * trig::cos(th - 0.75) + a * trig::sin(th - 0.75)).normalize()
        };
        (DVec3::from_array(*centre) + m * *radius, b)
    };
    let a = box_at(&mut rng, p);
    placement(&mut rng, a, b)
}

/// `B` a box about the world origin, so each face's plane lattice has its
/// origin (a corner of four tiles at every level) at the face's centre;
/// a vertex of `A` there.
fn on_plane_origin(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let r = rotation(&mut rng);
    let [x, y, z] = [DVec3::X, DVec3::Y, DVec3::Z].map(|a| r * a * 2.0);
    let b = Shape::Box {
        o: (-(x + y + z) * 0.5).to_array(),
        axes: [x.to_array(), y.to_array(), z.to_array()],
    };
    let sb = b.build(&Make::PLAIN);
    let f = sb.faces()[(seed % 6) as usize];
    let lat = sb.brep.face_surface(f).lattice().clone();
    let a = box_at(&mut rng, DVec3::from_array(lat.origin()));
    placement(&mut rng, a, b)
}

/// `B` a half cylinder whose axis lies across `UP`: its face folds over
/// in the shadow along two silhouettes inside it.
fn folds(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let axis = rng.direction().cross(UP).normalize();
    let r = rng.range(0.6, 1.2);
    let h = rng.range(1.5, 3.0);
    let at = point(&mut rng, 0.3);
    let b = Shape::HalfCylinder {
        point: (at - axis * (0.5 * h)).to_array(),
        axis: axis.to_array(),
        radius: r,
        z: [0.0, h],
        first: (rng.next_u64() % 4) as i64,
    };
    let ka = (rng.next_u64() % 3) as u32;
    let p = point(&mut rng, 1.0);
    let a = Shape::random(&mut rng, ka, p, 1.5);
    placement(&mut rng, a, b)
}

/// A triangular prism: the triangle `p0, p1, p2` (counter-clockwise
/// about `n`) extruded by `h` along `n`.
fn prism(p: [DVec3; 3], n: DVec3, h: f64) -> Shape {
    let top = p.map(|x| x + n * h);
    let verts = [p[0], p[1], p[2], top[0], top[1], top[2]].map(|x| x.to_array());
    Shape::Poly {
        verts: verts.to_vec(),
        faces: vec![
            vec![0, 2, 1],
            vec![3, 4, 5],
            vec![0, 1, 4, 3],
            vec![1, 2, 5, 4],
            vec![2, 0, 3, 5],
        ],
    }
}

/// `B` a knife wedge, its blade an edge of angle `2⁻⁶` rad; `A` a box
/// across the blade.
fn knife(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let q = rotation(&mut rng);
    let (x, y, n) = (q * DVec3::X, q * DVec3::Y, q * DVec3::Z);
    let l = rng.range(2.0, 3.0);
    let alpha = 1.0 / 64.0;
    let o = point(&mut rng, 0.3) - x * (0.5 * l);
    let p = [
        o,
        o + x * l,
        o + (x * trig::cos(alpha) + y * trig::sin(alpha)) * l,
    ];
    let b = prism(p, n, rng.range(1.0, 2.0));
    // A box near the blade's end.
    let tip = o + x * (l * rng.range(0.2, 0.9)) + n * rng.range(-0.5, 1.5);
    let near = point(&mut rng, 0.05);
    let a = box_at(&mut rng, tip + near);
    placement(&mut rng, a, b)
}

/// `B` a hip roof (a rectangle's base, a ridge above it, two trapezoids
/// and two triangles); a vertex of `A` on the ridge (its stored middle,
/// within rounding of the line).
fn roof(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let q = rotation(&mut rng);
    let (x, y, n) = (q * DVec3::X, q * DVec3::Y, q * DVec3::Z);
    let (lx, ly, h) = (
        rng.range(2.0, 3.0),
        rng.range(1.0, 2.0),
        rng.range(0.5, 1.5),
    );
    let o = point(&mut rng, 0.3) - x * (0.5 * lx) - y * (0.5 * ly);
    let inset = rng.range(0.2, 0.8);
    let base = [o, o + x * lx, o + x * lx + y * ly, o + y * ly];
    let ridge = [
        o + x * inset + y * (0.5 * ly) + n * h,
        o + x * (lx - inset) + y * (0.5 * ly) + n * h,
    ];
    let verts: Vec<[f64; 3]> = base.iter().chain(&ridge).map(|p| p.to_array()).collect();
    let b = Shape::Poly {
        verts,
        faces: vec![
            vec![0, 3, 2, 1],
            vec![0, 1, 5, 4],
            vec![2, 3, 4, 5],
            vec![1, 2, 5],
            vec![3, 0, 4],
        ],
    };
    let t = rng.range(0.1, 0.9);
    let at = ridge[0] + (ridge[1] - ridge[0]) * t;
    let a = box_at(&mut rng, at);
    placement(&mut rng, a, b)
}

/// `B` a spandrel (a quarter cylinder's outside up to its two tangent
/// planes): cusps at the arc's ends on both caps; `A` a box about one.
fn cusp(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let axis = rng.direction();
    let (r, h) = (rng.range(0.8, 1.5), rng.range(1.0, 2.0));
    let at = point(&mut rng, 0.3);
    let b = Shape::Spandrel {
        point: (at - axis * (0.5 * h)).to_array(),
        axis: axis.to_array(),
        radius: r,
        z: [0.0, h],
        first: (rng.next_u64() % 4) as i64,
    };
    let sb = b.build(&Make::PLAIN);
    // Near a cusp vertex (the arc's ends, ids 0, 2, 3, 5 of the build;
    // ids may be shuffled later, positions are what count).
    let v = sb.verts();
    let p = DVec3::from_array(sb.brep.pos(v[(rng.next_u64() % v.len() as u64) as usize]));
    let near = point(&mut rng, 0.02);
    let a = box_at(&mut rng, p + near);
    placement(&mut rng, a, b)
}

/// The snapping grid of [`shared_corner`]'s solids, `2^(4 − 52)`: every
/// coordinate of both lies below `2^4`.
const SHARED_GRID: Option<i64> = Some(4);

/// A vertex of `A` exactly at a vertex of `B`: both solids snapped to one
/// grid ([`Make::grid`]), so the vertex `A` is placed at stays where it
/// is (on each solid's own grid a coarser one rounds it off `B`'s by an
/// ulp, a near coincidence rather than the shared vertex meant).
fn shared_corner(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let b = random_b(&mut rng);
    let sb = b.build(&Make {
        grid: SHARED_GRID,
        ..Make::PLAIN
    });
    let v = sb.verts();
    let p = sb.brep.pos(v[(rng.next_u64() % v.len() as u64) as usize]);
    let a = box_at(&mut rng, DVec3::from_array(p));
    let mut out = placement(&mut rng, a, b);
    out.make_a.grid = SHARED_GRID;
    out.make_b.grid = SHARED_GRID;
    out
}

/// `B` a half cylinder with its axis across `UP` and its half facing
/// down: its face overhangs its own edges; `A` a box under it.
fn overhang(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let up = UP.normalize();
    let axis = rng.direction().cross(up).normalize();
    let r = rng.range(0.8, 1.5);
    let point_ = point(&mut rng, 0.3);
    // The cell whose middle direction points most downwards.
    let lat = Lattice::new(
        CylinderKey::new(point_.to_array(), axis.to_array(), r)
            .unwrap()
            .into(),
    )
    .unwrap();
    let first = (0..4)
        .min_by(|&a, &b| {
            quarter(&lat, a + 1)
                .dot(up)
                .total_cmp(&quarter(&lat, b + 1).dot(up))
        })
        .unwrap();
    let h = rng.range(1.5, 2.5);
    let b = Shape::HalfCylinder {
        point: point_.to_array(),
        axis: axis.to_array(),
        radius: r,
        z: [-0.5 * h, 0.5 * h],
        first,
    };
    let below = point_ - up * rng.range(0.0, 1.2 * r) + axis * rng.range(-0.5 * h, 0.5 * h);
    let a = box_at(&mut rng, below);
    placement(&mut rng, a, b)
}

/// `A` and `B` axis-aligned boxes with corners and sizes on the half
/// grid: coincident faces, edges and vertices, exact ties of every kind
/// (`A` turned by a quarter about an axis for some seeds).
fn grid_boxes(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let half = |rng: &mut Rng, lo: i64, hi: i64| {
        (lo + (rng.next_u64() % (hi - lo + 1) as u64) as i64) as f64 * 0.5
    };
    let b = Shape::Box {
        o: [0.0; 3],
        axes: [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
    };
    let o = [0, 1, 2].map(|_| half(&mut rng, -2, 4));
    let l = [0, 1, 2].map(|_| half(&mut rng, 1, 4));
    let mut axes = [[l[0], 0.0, 0.0], [0.0, l[1], 0.0], [0.0, 0.0, l[2]]];
    if seed.is_multiple_of(3) {
        // A quarter turn about z: (x, y) → (−y, x), exactly.
        axes = [[0.0, l[0], 0.0], [-l[1], 0.0, 0.0], [0.0, 0.0, l[2]]];
    }
    let a = Shape::Box { o, axes };
    placement(&mut rng, a, b)
}

/// `B` a half cylinder whose axis passes through the world origin, its
/// axial range starting at the axis's foot: the diameter at that end
/// crosses its plane lattice's origin, a corner of four tiles.
fn trim_through_origin(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let axis = rng.direction();
    let b = Shape::HalfCylinder {
        point: [0.0; 3],
        axis: axis.to_array(),
        radius: rng.range(0.8, 1.5),
        z: [0.0, rng.range(1.5, 2.5)],
        first: (rng.next_u64() % 4) as i64,
    };
    let ka = (rng.next_u64() % 3) as u32;
    let p = point(&mut rng, 1.0);
    let a = Shape::random(&mut rng, ka, p, 1.5);
    placement(&mut rng, a, b)
}

/// Two half cylinders on one axis, `A` stacked on `B`: `A`'s bottom
/// arc is `B`'s top arc bit for bit (one curve) where their halves
/// start in the same angle cell, or overlaps it by a quarter (the same
/// circle, other spans) where `A`'s starts a cell later.
fn stacked(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let axis = rng.direction();
    let point_ = point(&mut rng, 0.3);
    let (r, h) = (rng.range(0.8, 1.5), rng.range(1.0, 2.0));
    let first = (rng.next_u64() % 4) as i64;
    let b = Shape::HalfCylinder {
        point: point_.to_array(),
        axis: axis.to_array(),
        radius: r,
        z: [0.0, h],
        first,
    };
    let a = Shape::HalfCylinder {
        point: point_.to_array(),
        axis: axis.to_array(),
        radius: r,
        z: [h, h + rng.range(1.0, 2.0)],
        first: if seed.is_multiple_of(2) {
            first
        } else {
            (first + 1) % 4
        },
    };
    placement(&mut rng, a, b)
}

/// The cube `[0, 2]³`: its faces' key planes hold its vertices exactly,
/// and with unperturbed p-curves its tiles' patches, p-curves and edges
/// agree bit for bit (no ribbon has width).
fn cube() -> Shape {
    Shape::Box {
        o: [0.0; 3],
        axes: [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
    }
}

/// A point's coordinates as rationals.
fn rational(p: [f64; 3]) -> [Rational; 3] {
    p.map(|x| Rational::try_from(x).expect("finite"))
}

/// The cube's corners ([`cube`]).
fn cube_corners() -> Vec<[f64; 3]> {
    (0..8)
        .map(|i| [i & 1, i >> 1 & 1, i >> 2 & 1].map(|b| 2.0 * f64::from(b)))
        .collect()
}

fn int(rng: &mut Rng, lo: i64, hi: i64) -> i64 {
    lo + (rng.next_u64() % (hi - lo + 1) as u64) as i64
}

fn icross(a: [i64; 3], b: [i64; 3]) -> [i64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A turned box's edges, exactly: integer vectors `u`, `v = u × t` and
/// `w = u × v` (orthogonal, right-handed), each scaled by a power of two
/// to a length in `[0.75, 1.5)`, so the box's corners are exact sums
/// where `o` is a short dyadic.
/// No component is zero (no face parallel to a coordinate plane) but
/// `u`'s on axis `flat`, if given (an edge in that coordinate plane).
/// The box has its corner at `o`, and none of the points `avoid` (the
/// other solid's vertices) lies on the plane of one of its faces: a
/// turned face's key plane is rounded, so such a vertex would be tied
/// only in the oracle's tiles, within their misfit, not in the
/// counting's records.
fn exact_axes(
    rng: &mut Rng,
    flat: Option<usize>,
    o: [f64; 3],
    avoid: &[[f64; 3]],
) -> [[f64; 3]; 3] {
    loop {
        let mut u = [0, 1, 2].map(|_| int(rng, -4, 4));
        if let Some(k) = flat {
            u[k] = 0;
        }
        let t = [0, 1, 2].map(|_| int(rng, -4, 4));
        let v = icross(u, t);
        let w = icross(u, v);
        let zero = |x: &[i64; 3], skip: Option<usize>| (0..3).any(|k| Some(k) != skip && x[k] == 0);
        if zero(&u, flat) || zero(&v, None) || zero(&w, None) {
            continue;
        }
        let axes = [u, v, w].map(|x| {
            let n2 = x.iter().map(|c| c * c).sum::<i64>() as f64;
            let mut s = 1.0;
            while n2 * s * s >= 2.25 {
                s *= 0.5;
            }
            DVec3::from_array(x.map(|c| c as f64 * s))
        });
        // The axes are short dyadics (their cross products exact); `o`
        // may not be, so the planes are tested in rationals.
        let on_a_face = (0..3).any(|k| {
            let n = rational(axes[(k + 1) % 3].cross(axes[(k + 2) % 3]).to_array());
            let (o, step) = (rational(o), rational(axes[k].to_array()));
            let far = [0, 1, 2].map(|i| &o[i] + &step[i]);
            [o, far].iter().any(|c| {
                avoid.iter().any(|&p| {
                    let p = rational(p);
                    (0..3).map(|i| &n[i] * (&p[i] - &c[i])).sum::<Rational>() == 0
                })
            })
        });
        if !on_a_face {
            return axes.map(|x| x.to_array());
        }
    }
}

/// A point of the cube's face on axis `k` (at 0 or 2), inside it by at
/// least `3/8` (far outside every band), on the `1/8` grid.
fn on_cube_face(rng: &mut Rng, k: usize) -> [f64; 3] {
    let mut p = [0, 1, 2].map(|_| int(rng, 3, 13) as f64 / 8.0);
    p[k] = 2.0 * int(rng, 0, 1) as f64;
    p
}

/// Ties exact in the counting's own records, not only in the oracle's
/// tiles: the cube (its faces on their key planes, every p-curve
/// unperturbed) and a box turned by an exact frame, all corners exact
/// (but at a silhouette: there its corner on `B` is kept, the others
/// snapped), with
/// - a corner of `A` on a face of `B` (a point–face height of 0 on the
///   key plane);
/// - a corner of `B` on a face of `A` (the cube as `A`);
/// - a corner of `A` on a face of `B` and an edge of `A` in its plane
///   (crossing `B`'s edges in it where it leaves the face);
/// - a corner of `A` whose upward ray passes through a point of an edge
///   of `B` (a ray record's tie);
/// - a corner of `A` exactly on an edge of `B` (its column hits on the
///   two faces there read the edge's germ key);
/// - a corner of `A` exactly on the curved face of an exact half
///   cylinder `B`, at the tile corner nearest a silhouette
///   ([`silhouette`]): the column grazes the face there, and the
///   corner's winding turns with the side of the face `d` moves it to.
///
/// Moving `A` by `δ·d` settles each; counting with `d` and `−d` must
/// differ exactly where the oracle's answers at `±2^-28·d` do.
fn kernel_exact(seed: u64) -> Placement {
    let mut rng = Rng::new(seed);
    let k = int(&mut rng, 0, 2) as usize;
    let kind = exact_kind(seed);
    let mut levels = [0, 0];
    let (a, b) = match kind {
        ExactKind::CornerOnFace | ExactKind::EdgeInPlane => {
            let o = on_cube_face(&mut rng, k);
            let axes = exact_axes(
                &mut rng,
                (kind == ExactKind::EdgeInPlane).then_some(k),
                o,
                &cube_corners(),
            );
            (Shape::Box { o, axes }, cube())
        }
        ExactKind::CubeCornerOnFace => {
            let o = on_cube_face(&mut rng, k);
            let axes = exact_axes(&mut rng, None, o, &cube_corners());
            (cube(), Shape::Box { o, axes })
        }
        ExactKind::RayThroughEdge => {
            // A point of an edge of the cube along axis `k`, and a corner
            // of `A` below it along `UP` by `s·UP`.
            let mut p = [0, 1, 2].map(|_| 2.0 * int(&mut rng, 0, 1) as f64);
            p[k] = int(&mut rng, 3, 13) as f64 / 8.0;
            let s = [1.0 / 32.0, 1.0 / 64.0][int(&mut rng, 0, 1) as usize];
            let o = (DVec3::from_array(p) - UP * s).to_array();
            let axes = exact_axes(&mut rng, None, o, &cube_corners());
            (Shape::Box { o, axes }, cube())
        }
        ExactKind::CornerOnEdge => {
            // A corner of `A` at a point of an edge of the cube along axis
            // `k`.
            let mut o = [0, 1, 2].map(|_| 2.0 * int(&mut rng, 0, 1) as f64);
            o[k] = int(&mut rng, 3, 13) as f64 / 8.0;
            let axes = exact_axes(&mut rng, None, o, &cube_corners());
            (Shape::Box { o, axes }, cube())
        }
        ExactKind::CornerOnSilhouette => {
            let (b, level, o) = silhouette(&mut rng);
            let verts = b.build(&Make::PLAIN);
            let verts: Vec<[f64; 3]> = verts.verts().iter().map(|&v| verts.brep.pos(v)).collect();
            let axes = exact_axes(&mut rng, None, o, &verts);
            levels = [level, 0];
            (Shape::Box { o, axes }, b)
        }
    };
    let mut make = || Make {
        ids: Some(rng.next_u64()),
        ..Make::PLAIN
    };
    let (mut make_a, mut make_b) = (make(), make());
    make_b.levels = levels;
    // `A`'s corner on `B`'s tile corner, which isn't on `A`'s snapping
    // grid ([`Make::keep_first`]).
    make_a.keep_first = kind == ExactKind::CornerOnSilhouette;
    Placement {
        a,
        b,
        make_a,
        make_b,
        d: rng.direction(),
    }
}

/// The levels [`silhouette`] draws its cylinder's angle tiles at.
const SILHOUETTE_LEVELS: [u32; 3] = [0, 3, 5];

/// An exact half cylinder about an axis along `x` or `y` (its key, frame
/// and tiles exact: dyadic foot and radius) at an angle level drawn from
/// [`SILHOUETTE_LEVELS`], its half about one of its silhouettes seen
/// along `UP` (the rulings whose tangent plane holds `UP`), and the
/// point of its face on the tile line inside the half nearest that
/// silhouette: a corner of the stored tiles, exactly on them (a tile's
/// corner is its control point; the line is straight along the axis, so
/// any dyadic axial position stays on it). The silhouette itself lies
/// at no `f64` point of the tiles (their angle cells are conics whose
/// tangents run along `UP`'s shadow only at irrational parameters), so
/// the point is on the face exactly with the column grazing it, the
/// face's facing there small but not zero. Also the level.
fn silhouette(rng: &mut Rng) -> (Shape, u32, [f64; 3]) {
    let k = int(rng, 0, 1) as usize;
    let mut axis = [0.0; 3];
    axis[k] = 1.0;
    let radius = int(rng, 6, 10) as f64 / 8.0;
    let h = int(rng, 12, 20) as f64 / 8.0;
    let mut point = [0, 1, 2].map(|_| int(rng, -4, 4) as f64 / 8.0);
    point[k] = 0.0;
    let level = SILHOUETTE_LEVELS[int(rng, 0, SILHOUETTE_LEVELS.len() as i64 - 1) as usize];
    let lat = Lattice::new(CylinderKey::new(point, axis, radius).unwrap().into()).unwrap();
    let a = DVec3::from_array(axis);
    // A silhouette's outward normal, `⊥ a` and `⊥ UP`, and the angle
    // cell `c` holding it: the half has it inside, the cell boundary
    // nearer it its middle ruling.
    let sign = if int(rng, 0, 1) == 0 { 1.0 } else { -1.0 };
    let n = a.cross(UP) * sign;
    let c = (0..4)
        .find(|&c| quarter(&lat, c).dot(n) > 0.0 && quarter(&lat, c + 1).dot(n) > 0.0)
        .expect("a cell holds the normal");
    let first = if quarter(&lat, c).dot(n) > quarter(&lat, c + 1).dot(n) {
        (c - 1).rem_euclid(4)
    } else {
        c
    };
    // The tile lines inside the half at the level, the one whose tile
    // corner's tangent plane is nearest to holding `UP`.
    let at = Level { u: level, v: 0 };
    let tiles = ((first << level) + 1)..((first + 2) << level);
    let graze = |i: i64| {
        let t = lat.tile(at, i, 0).expect("a tile");
        let (p0, p1) = (t.point(0, 0), t.point(1, 0));
        let tu = DVec3::from_array(p1) - DVec3::from_array(p0);
        let m = tu.cross(a);
        (m.dot(UP) / (m.length() * UP.length())).abs()
    };
    let i = tiles
        .min_by(|&x, &y| graze(x).total_cmp(&graze(y)))
        .expect("a tile line inside the half");
    let mut q = lat.tile(at, i, 0).expect("a tile").point(0, 0);
    q[k] = int(rng, -2, 2) as f64 / 8.0;
    let b = Shape::HalfCylinder {
        point,
        axis,
        radius,
        z: [-0.5 * h, 0.5 * h],
        first,
    };
    (b, level, q)
}

/// The kinds of [`kernel_exact`]'s placements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExactKind {
    /// A corner of `A` on a face of `B`.
    CornerOnFace,
    /// A corner of `A` on a face of `B`, an edge of `A` in its plane.
    EdgeInPlane,
    /// A corner of `B` on a face of `A`.
    CubeCornerOnFace,
    /// A corner of `A` whose upward ray passes through an edge of `B`.
    RayThroughEdge,
    /// A corner of `A` on an edge of `B`.
    CornerOnEdge,
    /// A corner of `A` on a cylinder face of `B`, at the tile corner
    /// nearest its silhouette.
    CornerOnSilhouette,
}

fn exact_kind(seed: u64) -> ExactKind {
    if seed % 10 == 7 {
        return ExactKind::CornerOnSilhouette;
    }
    match seed % 5 {
        0 | 2 if seed % 4 == 2 => ExactKind::EdgeInPlane,
        0 | 2 => ExactKind::CornerOnFace,
        1 => ExactKind::CubeCornerOnFace,
        3 => ExactKind::RayThroughEdge,
        _ => ExactKind::CornerOnEdge,
    }
}

/// The kind of [`KERNEL_EXACT`]'s placement of seed `seed`, by name.
pub(super) fn kernel_exact_kind(seed: u64) -> &'static str {
    match exact_kind(seed) {
        ExactKind::CornerOnFace => "corner of A on a face of B",
        ExactKind::EdgeInPlane => "edge of A in a key plane",
        ExactKind::CubeCornerOnFace => "corner of B on a face of A",
        ExactKind::RayThroughEdge => "ray through an edge",
        ExactKind::CornerOnEdge => "corner of A on an edge of B",
        ExactKind::CornerOnSilhouette => "corner of A at a silhouette of B",
    }
}
