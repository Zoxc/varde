#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::FRAC_1_SQRT_2;

use glam::{DMat3, DVec3};

use super::*;
use crate::patch::{Conic3, cylinder_strip};
use crate::test_rng::Rng;

const MARGIN: f64 = 1e-6;

/// A random rotation.
fn rotation(rng: &mut Rng) -> DMat3 {
    let x = rng.direction();
    let y = x.any_orthonormal_vector();
    let (s, c) = rng.range(0.0, 6.0).sin_cos();
    let y = y * c + x.cross(y) * s;
    DMat3::from_cols(x, y, x.cross(y))
}

/// The corners of the box from `min` to `max`.
fn box_points(min: DVec3, max: DVec3) -> Vec<DVec3> {
    (0..8)
        .map(|i| {
            DVec3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            )
        })
        .collect()
}

fn moved(points: &[DVec3], m: DMat3, offset: DVec3) -> Vec<DVec3> {
    points.iter().map(|&p| m * p + offset).collect()
}

#[test]
fn boxes_a_known_gap_apart() {
    let mut rng = Rng::new(21);
    for _ in 0..200 {
        let gap = rng.log_range(1e-5, 10.0);
        let a = box_points(DVec3::new(-1.0, -2.0, -0.5), DVec3::new(0.0, 1.0, 0.5));
        // Across the gap along x, overlapping in y and z, so the closest
        // points are a face apart; or diagonally, so they are corners.
        let diagonal = rng.unit() < 0.5;
        let (lo, hi) = if diagonal {
            let d = gap / 3f64.sqrt();
            let lo = DVec3::new(d, 1.0 + d, 0.5 + d);
            (lo, lo + DVec3::new(2.0, 2.0, 1.5))
        } else {
            (DVec3::new(gap, -0.5, -3.0), DVec3::new(gap + 0.1, 0.1, 3.0))
        };
        let b = box_points(lo, hi);
        let m = rotation(&mut rng);
        let offset = rng.point(1e4);
        let (a, b) = (moved(&a, m, offset), moved(&b, m, offset));
        assert!(apart(&a, &b, gap * 0.99), "gap {gap}");
        assert!(apart(&b, &a, gap * 0.99), "gap {gap}");
        assert!(!apart(&a, &b, gap * 1.01), "gap {gap}");
        assert!(!apart(&b, &a, gap * 1.01), "gap {gap}");
    }
}

#[test]
fn clouds_either_side_of_a_plane_are_apart() {
    let mut rng = Rng::new(22);
    for _ in 0..400 {
        let n = rng.direction();
        let gap = rng.log_range(1e-4, 1.0);
        let side = |rng: &mut Rng, sign: f64| {
            (0..6)
                .map(|_| {
                    let p = rng.point(5.0);
                    p - n * p.dot(n) + n * sign * (gap / 2.0 + rng.log_range(1e-9, 3.0))
                })
                .collect::<Vec<_>>()
        };
        let (a, b) = (side(&mut rng, 1.0), side(&mut rng, -1.0));
        assert!(apart(&a, &b, gap * 0.99));
        // Sharing a point, or one reaching into the other, they aren't.
        let mut c = b.clone();
        c[3] = a[rng.next_u64() as usize % 6];
        assert!(!apart(&a, &c, 1e-9));
        c[3] = (a[0] + a[1] + a[2]) / 3.0;
        assert!(!apart(&a, &c, 1e-9));
    }
}

#[test]
fn flat_and_straight_sets_work() {
    // Coplanar squares side by side, and overlapping.
    let square = |x: f64| {
        [
            DVec3::new(x, 0.0, 0.0),
            DVec3::new(x + 1.0, 0.0, 0.0),
            DVec3::new(x + 1.0, 1.0, 0.0),
            DVec3::new(x, 1.0, 0.0),
        ]
    };
    assert!(apart(&square(0.0), &square(1.0 + 3e-6), MARGIN));
    assert!(!apart(&square(0.0), &square(1.0 + 5e-7), MARGIN));
    assert!(!apart(&square(0.0), &square(0.5), 1e-9));
    // Skew segments a known distance apart, and collinear ones.
    let g = 1e-3;
    let a = [DVec3::new(-1.0, 0.0, 0.0), DVec3::new(1.0, 0.0, 0.0)];
    let b = [DVec3::new(0.0, -1.0, g), DVec3::new(0.0, 1.0, g)];
    assert!(apart(&a, &b, 0.99 * g));
    assert!(!apart(&a, &b, 1.01 * g));
    let c = [DVec3::new(1.0 + g, 0.0, 0.0), DVec3::new(3.0, 0.0, 0.0)];
    assert!(apart(&a, &c, 0.99 * g));
    assert!(!apart(&a, &c, 1.01 * g));
    // Points, the same one repeated.
    assert!(!apart(&[DVec3::ONE; 6], &[DVec3::ONE; 6], 1e-9));
    assert!(apart(&[DVec3::ONE; 6], &[DVec3::ZERO], 1.7));
}

/// A flat triangle.
fn flat(p: [DVec3; 3]) -> Patch {
    Patch::flat(p).unwrap()
}

#[test]
fn vertex_neighbours() {
    let o = DVec3::ZERO;
    let a = flat([o, DVec3::X, DVec3::Y]);
    // Across the vertex in the same plane, and bent up out of it.
    let b = flat([o, -DVec3::X, -DVec3::Y - DVec3::X]);
    assert!(vertex_neighbours_apart(&a, 0, &b, 0, MARGIN));
    let b = flat([-DVec3::X, o, DVec3::new(-1.0, -1.0, 1.0)]);
    assert!(vertex_neighbours_apart(&a, 0, &b, 1, MARGIN));
    // Overlapping a's sector.
    let b = flat([DVec3::new(0.2, 0.2, 1.0), DVec3::new(0.2, 0.2, -1.0), o]);
    assert!(!vertex_neighbours_apart(&a, 0, &b, 2, MARGIN));
}

#[test]
fn straight_edge_neighbours() {
    let (p, q) = (DVec3::ZERO, DVec3::X);
    let a = flat([p, q, DVec3::Y]);
    // b runs the edge the other way: from q to p as its edge 0.
    let b_at = |far: DVec3| flat([q, p, far]);
    // Coplanar, opposite: a plane at right angles splits them.
    assert!(edge_neighbours_apart(
        &a,
        0,
        &b_at(DVec3::new(0.5, -1.0, 0.0)),
        0,
        MARGIN
    ));
    // A thin wedge still passes; folded flat onto a, it doesn't.
    assert!(edge_neighbours_apart(
        &a,
        0,
        &b_at(DVec3::new(0.5, 1.0, 1e-3)),
        0,
        MARGIN
    ));
    assert!(!edge_neighbours_apart(
        &a,
        0,
        &b_at(DVec3::new(0.5, 1.0, 0.0)),
        0,
        MARGIN
    ));
    // A wedge thinner than the margin can't be told from folded.
    assert!(!edge_neighbours_apart(
        &a,
        0,
        &b_at(DVec3::new(0.5, 1.0, 1e-7)),
        0,
        MARGIN
    ));
}

#[test]
fn curved_edge_neighbours() {
    // A quarter arc on z = 0, a flat cap inside it and a cylinder wall
    // above it: the cap lies in the arc's plane, the wall clears it.
    let arc = Conic3::new(DVec3::X, DVec3::new(1.0, 1.0, 0.0), FRAC_1_SQRT_2, DVec3::Y).unwrap();
    let [wall, _] = cylinder_strip(&arc, DVec3::Z).unwrap();
    // The cap runs the arc the other way: (O, Y, X), edge 1 from Y to X.
    let cap = Patch::new(
        [DVec3::ZERO, DVec3::Y, DVec3::X],
        [DVec3::Y * 0.5, arc.c, DVec3::X * 0.5],
        [1.0, FRAC_1_SQRT_2, 1.0],
    )
    .unwrap();
    assert!(edge_neighbours_apart(&wall, 0, &cap, 1, MARGIN));
    assert!(edge_neighbours_apart(&cap, 1, &wall, 0, MARGIN));
    // The wall hanging down instead: still apart.
    let [down, _] = cylinder_strip(&arc.reversed(), -DVec3::Z).unwrap();
    assert!(edge_neighbours_apart(&down, 0, &cap, 1, MARGIN));
    // Two caps in the same plane, both inside the arc, aren't.
    assert!(!edge_neighbours_apart(&cap, 1, &cap, 1, MARGIN));
    // A cap tilted down past the plane by more than the margin, with the
    // wall above: apart. Tilted up into the wall's side: not.
    let tilted = |z: f64| {
        let mut c = cap;
        c.p[0].z = z;
        c.c[0].z = z / 2.0;
        c.c[2].z = z / 2.0;
        c
    };
    assert!(edge_neighbours_apart(&wall, 0, &tilted(-0.1), 1, MARGIN));
    assert!(edge_neighbours_apart(
        &wall,
        0,
        &tilted(0.5 * MARGIN),
        1,
        MARGIN
    ));
    assert!(!edge_neighbours_apart(&wall, 0, &tilted(0.1), 1, MARGIN));
}

#[test]
fn curved_edge_fins_need_the_margin() {
    // The cap of `curved_edge_neighbours` against a wall whose lowest far
    // control point (the opposite corner and the other two edges' control
    // points) is 1.5 margins off the arc's plane: the cap may lean towards
    // it only while the wall still clears the cap's highest point by more
    // than the margin.
    let arc = Conic3::new(DVec3::X, DVec3::new(1.0, 1.0, 0.0), FRAC_1_SQRT_2, DVec3::Y).unwrap();
    let cap = Patch::new(
        [DVec3::ZERO, DVec3::Y, DVec3::X],
        [DVec3::Y * 0.5, arc.c, DVec3::X * 0.5],
        [1.0, FRAC_1_SQRT_2, 1.0],
    )
    .unwrap();
    let tilted = |z: f64| {
        let mut c = cap;
        c.p[0].z = z;
        c.c[0].z = z / 2.0;
        c.c[2].z = z / 2.0;
        c
    };
    // The wall's far points from edge 0 scale with its height: measure
    // them at height 1 and scale to put the lowest at 1.5 margins.
    let lowest = |w: &Patch| {
        [w.p[2].z, w.c[1].z, w.c[2].z]
            .map(f64::abs)
            .into_iter()
            .fold(f64::INFINITY, f64::min)
    };
    for (arc, sign) in [(arc, 1.0), (arc.reversed(), -1.0)] {
        let [unit, _] = cylinder_strip(&arc, DVec3::Z * sign).unwrap();
        let h = 1.5 * MARGIN / lowest(&unit);
        let [wall, _] = cylinder_strip(&arc, DVec3::Z * sign * h).unwrap();
        assert!((lowest(&wall) / MARGIN - 1.5).abs() < 1e-9);
        for (lean, apart) in [(0.0, true), (0.9, false), (0.4, true)] {
            let cap = tilted(sign * lean * MARGIN);
            assert_eq!(
                edge_neighbours_apart(&wall, 0, &cap, 1, MARGIN),
                apart,
                "sign {sign}, lean {lean}"
            );
            assert_eq!(
                edge_neighbours_apart(&cap, 1, &wall, 0, MARGIN),
                apart,
                "swapped, sign {sign}, lean {lean}"
            );
        }
    }
}

#[test]
fn long_thin_hulls_are_told_apart() {
    // The top of a 611 × 0.066 × 0.187 box and its front side meet at a
    // corner, with the rest of each more than 0.03 clear of any plane
    // through it tilted between them. Solved through the Gram matrix, a
    // thin triangle of the Minkowski difference lost enough digits to
    // stop GJK short, in one order of the points and not the other.
    let (sx, sy, sz) = (611.1071037741102, 0.0659348838221904, 0.1866932464521183);
    let t0 = DVec3::new(0.0, 0.0, sz);
    let t1 = DVec3::new(sx, 0.0, sz);
    let t2 = DVec3::new(sx, sy, sz);
    let top = Patch::flat([t0, t1, t2]).unwrap();
    let side = Patch::flat([DVec3::ZERO, DVec3::X * sx, t1]).unwrap();
    assert!(vertex_neighbours_apart(&top, 1, &side, 2, MARGIN));
    let mut points: Vec<DVec3> = top.hull().iter().map(|&x| x - t1).collect();
    points.extend(side.hull().iter().map(|&x| t1 - x));
    points.retain(|&x| x != DVec3::ZERO);
    let mut rng = Rng::new(5);
    for _ in 0..20 {
        // Any order of the points, and any rotation.
        for i in (1..points.len()).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            points.swap(i, j);
        }
        let m = rotation(&mut rng);
        assert!(apart(
            &moved(&points, m, DVec3::ZERO),
            &[DVec3::ZERO],
            MARGIN
        ));
        assert!(!apart(&moved(&points, m, DVec3::ZERO), &[DVec3::ZERO], 0.1));
    }
}

#[test]
fn a_support_point_square_to_the_closest_is_kept() {
    // The top and a long side of a 2e-4 × 7951 × 2e-4 box meet at a
    // corner, and a plane through it clears the rest by about 3e-5. From
    // the corner (0, 0, 1e-4) of the difference, the support point (0,
    // -7951, 0) is square to it: the segment's closest point is nearer by
    // less than the rounding of its squared length. Keeping the point
    // alone then found the same support point again and again, and GJK
    // gave up, in some orders of the points.
    let points = [
        DVec3::new(0.0, -7951.0, 0.0),
        DVec3::new(2e-4, 0.0, 0.0),
        DVec3::new(1e-4, -3975.5, 0.0),
        DVec3::new(1e-4, 0.0, 0.0),
        DVec3::new(0.0, -3975.5, 0.0),
        DVec3::new(-2e-4, 0.0, 2e-4),
        DVec3::new(0.0, 0.0, 2e-4),
        DVec3::new(-1e-4, 0.0, 2e-4),
        DVec3::new(0.0, 0.0, 1e-4),
        DVec3::new(-1e-4, 0.0, 1e-4),
    ];
    for start in 0..points.len() {
        let mut order = points;
        order.rotate_left(start);
        assert!(apart(&order, &[DVec3::ZERO], 1e-8), "from {start}");
        assert!(!apart(&order, &[DVec3::ZERO], 1e-4), "from {start}");
    }
}

/// A meridian piece in the `x`-`z` half-plane, about the `z` axis: an
/// arc of the circle about `(rho, h)` of `radius`, or a line.
enum Piece {
    Arc([f64; 2], f64, [f64; 2], [f64; 2]),
    Line([f64; 2], [f64; 2]),
}

fn at([rho, h]: [f64; 2]) -> DVec3 {
    DVec3::new(rho, 0.0, h)
}

/// The strip of `piece` between the first two of 64 stations round the
/// `z` axis, its diagonal fitted to the surface it sweeps: `[0]` has the
/// parallel at the piece's start as edge 0, `[1]` the one at its end as
/// edge 1 (run the other way).
fn ring_strip(piece: &Piece) -> [Patch; 2] {
    let lathe = crate::sweep::Lathe::new(DVec3::ZERO, DVec3::Z, None, 64).unwrap();
    let (meridian, form) = match *piece {
        Piece::Arc(centre, minor, a, b) => (
            Conic3::arc_between(at(centre), minor, at(a), at(b)).unwrap(),
            crate::mesh::Form::Torus {
                centre: DVec3::Z * centre[1],
                axis: DVec3::Z,
                major: centre[0],
                minor,
            },
        ),
        Piece::Line(a, b) if a[1] == b[1] => (
            Conic3::line(at(a), at(b)).unwrap(),
            crate::mesh::Form::plane(DVec3::Z, a[1]),
        ),
        Piece::Line(a, b) => (
            Conic3::line(at(a), at(b)).unwrap(),
            crate::mesh::Form::Cylinder {
                point: DVec3::ZERO,
                axis: DVec3::Z,
                radius: a[0],
            },
        ),
    };
    let strip = crate::sweep::fitted_strip(
        &lathe.parallel(meridian.p0, 0).unwrap(),
        &lathe.parallel(meridian.p1, 0).unwrap(),
        &lathe.meridian(&meridian, 0).unwrap(),
        &lathe.meridian(&meridian, 1).unwrap(),
        &form,
    )
    .unwrap()
    .patches;
    assert!(strip.iter().all(|p| p.fold_direction().is_some()));
    strip
}

/// The patches either side of the ring where `below` ends and `above`
/// starts: `above`'s first (its edge 0) and `below`'s second (edge 1).
fn ring_pair(below: &Piece, above: &Piece) -> (Patch, Patch) {
    (ring_strip(above)[0], ring_strip(below)[1])
}

#[test]
fn rings_at_turns_are_parted_by_the_cylinder() {
    use Piece::{Arc, Line};
    let cases = [
        // A torus's top: both sides under the ring's plane.
        (
            "torus top",
            Arc([20.0, 0.0], 2.0, [21.0, 3f64.sqrt()], [20.0, 2.0]),
            Arc([20.0, 0.0], 2.0, [20.0, 2.0], [19.0, 3f64.sqrt()]),
        ),
        // A puck's rounded edge meeting its flat top.
        (
            "round into flat",
            Arc([8.0, 2.0], 2.0, [10.0, 2.0], [8.0, 4.0]),
            Line([8.0, 4.0], [6.0, 4.0]),
        ),
        // A plate meeting a concave fillet up a boss.
        (
            "flat into concave",
            Line([12.0, 0.0], [6.0, 0.0]),
            Arc([6.0, 2.0], 2.0, [6.0, 0.0], [4.0, 2.0]),
        ),
        // Convex then concave, the turn at the joint.
        (
            "S",
            Arc([7.0, 3.0], 3.0, [10.0, 3.0], [7.0, 6.0]),
            Arc([7.0, 8.0], 2.0, [7.0, 6.0], [5.0, 8.0]),
        ),
    ];
    for (name, below, above) in cases {
        let (a, b) = ring_pair(&below, &above);
        assert!(!edge_neighbours_apart(&a, 0, &b, 1, MARGIN), "{name}");
        assert!(!edge_neighbours_apart(&b, 1, &a, 0, MARGIN), "{name}");
        assert!(cylinder_apart(&a, 0, &b, 1, MARGIN), "{name}");
        assert!(cylinder_apart(&b, 1, &a, 0, MARGIN), "{name}");
        assert!(edge_neighbours_parted(&a, 0, &b, 1, MARGIN), "{name}");
        // The edge row off the conic: the patches no longer share it.
        for scale in [1.0 + 1e-3, 1.0 - 1e-3] {
            let mut off = b;
            off.w[1] *= scale;
            assert!(!cylinder_apart(&a, 0, &off, 1, MARGIN), "{name}");
            assert!(!cylinder_apart(&off, 1, &a, 0, MARGIN), "{name}");
        }
    }
}

#[test]
fn the_cylinder_refuses_what_it_cannot_part() {
    use Piece::{Arc, Line};
    // Two rounds out from a top, both outside its cylinder and under its
    // plane: folded onto each other.
    let (a, b) = ring_pair(
        &Arc([20.0, 0.0], 2.0, [21.0, 3f64.sqrt()], [20.0, 2.0]),
        &Arc([20.0, 1.0], 1.0, [20.0, 2.0], [20.5, 1.0 + 0.75f64.sqrt()]),
    );
    assert!(!edge_neighbours_parted(&a, 0, &b, 1, MARGIN));
    assert!(!edge_neighbours_parted(&b, 1, &a, 0, MARGIN));
    // A wall under a round where it is upright: the plane parts them, but
    // the wall lies on the cylinder.
    let (a, b) = ring_pair(
        &Line([10.0, 0.0], [10.0, 2.0]),
        &Arc([8.0, 2.0], 2.0, [10.0, 2.0], [8.0, 4.0]),
    );
    assert!(edge_neighbours_apart(&a, 0, &b, 1, MARGIN));
    assert!(!cylinder_apart(&a, 0, &b, 1, MARGIN));
    assert!(!cylinder_apart(&b, 1, &a, 0, MARGIN));
    assert!(edge_neighbours_parted(&a, 0, &b, 1, MARGIN));
    // A straight edge has no cylinder.
    let a = flat([DVec3::ZERO, DVec3::X, DVec3::new(0.5, 1.0, -0.1)]);
    let b = flat([DVec3::X, DVec3::ZERO, DVec3::new(0.5, -1.0, -0.1)]);
    assert!(edge_neighbours_apart(&a, 0, &b, 0, MARGIN));
    assert!(!cylinder_apart(&a, 0, &b, 0, MARGIN));
}

/// Pairs of random patches sharing a random conic edge: wherever the
/// cylinder rule parts them, `F` evaluated at points of each, away from
/// the edge, has the sign the coefficients promised, opposite on the two.
#[test]
fn the_cylinder_rule_is_sound_on_random_pairs() {
    let mut rng = Rng::new(86);
    let (mut tried, mut passed) = (0, 0);
    for _ in 0..200_000 {
        let p = rng.point(1.0);
        let q = rng.point(1.0);
        let c = (p + q) / 2.0 + rng.point(0.6);
        let w = rng.log_range(0.2, 5.0);
        let patch = |start: DVec3, end: DVec3, rng: &mut Rng| Patch {
            p: [start, end, rng.point(1.5)],
            c: [c, rng.point(1.5), rng.point(1.5)],
            w: [w, rng.log_range(0.3, 3.0), rng.log_range(0.3, 3.0)],
        };
        let (a, b) = (patch(p, q, &mut rng), patch(q, p, &mut rng));
        if a.check().is_err() || b.check().is_err() {
            continue;
        }
        tried += 1;
        if !cylinder_apart(&a, 0, &b, 0, 1e-9) {
            continue;
        }
        passed += 1;
        let (e1, e2) = (p - c, q - c);
        let n = e1.cross(e2);
        let nn = n.dot(n);
        let f = |x: DVec3| {
            let s = x - c;
            let lp = n.dot(s.cross(e2)) / nn;
            let lq = n.dot(e1.cross(s)) / nn;
            let lc = 1.0 - lp - lq;
            lc * lc - 4.0 * w * w * lp * lq
        };
        let sign = |x: &Patch| {
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for i in 0..=30 {
                for j in 0..=30 - i {
                    let far = (30 - i - j) as f64 / 30.0;
                    if far < 0.02 {
                        continue;
                    }
                    let v = f(x.eval(DVec3::new(i as f64 / 30.0, j as f64 / 30.0, far)));
                    (lo, hi) = (lo.min(v), hi.max(v));
                }
            }
            if lo > 0.0 {
                1.0
            } else if hi < 0.0 {
                -1.0
            } else {
                0.0
            }
        };
        let (sa, sb) = (sign(&a), sign(&b));
        assert!(sa != 0.0 && sa == -sb, "{a:?} {b:?}");
    }
    assert!(
        tried > 100_000 && passed > 100,
        "tried {tried}, passed {passed}"
    );
}
