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

/// The cylinder rule on patches sharing edge `ea` of `a`, edge `eb` of `b`.
fn cylinder_apart(a: &Patch, ea: usize, b: &Patch, eb: usize, margin: f64) -> bool {
    CurvedPair::new(a, ea, b, eb, margin).is_some_and(|pair| super::cylinder_apart(&pair, margin))
}

/// The pencil rule's member on patches sharing edge `ea` of `a`, edge `eb`
/// of `b`.
fn pencil_member(a: &Patch, ea: usize, b: &Patch, eb: usize, margin: f64) -> Option<(f64, f64)> {
    super::pencil_member(&CurvedPair::new(a, ea, b, eb, margin)?, margin)
}

/// Whether the pencil rule parts them.
fn pencil_apart(a: &Patch, ea: usize, b: &Patch, eb: usize, margin: f64) -> bool {
    pencil_member(a, ea, b, eb, margin).is_some()
}

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
#[derive(Debug)]
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
    if let Piece::Line(a, b) = *piece
        && a[0] != b[0]
        && a[1] != b[1]
    {
        // A cone's strip, exact, its apex where the line meets the axis.
        let apex = DVec3::Z * (a[1] - a[0] * (b[1] - a[1]) / (b[0] - a[0]));
        let strip = crate::sweep::cone_strip(
            &lathe.parallel(at(a), 0).unwrap(),
            &lathe.parallel(at(b), 0).unwrap(),
            apex,
        )
        .unwrap();
        assert!(strip.iter().all(|p| p.fold_direction().is_some()));
        return strip;
    }
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
    assert!(
        strip.iter().all(|p| p.fold_direction().is_some()),
        "{piece:?}"
    );
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
        // The edge off the conic, by a thousandth of its weight or by a
        // bit of its weight or control point: the patches no longer share
        // it.
        let bit = |x: f64| f64::from_bits(x.to_bits() + 1);
        for scale in [1.0 + 1e-3, 1.0 - 1e-3] {
            let mut off = b;
            off.w[1] *= scale;
            assert!(!cylinder_apart(&a, 0, &off, 1, MARGIN), "{name}");
            assert!(!cylinder_apart(&off, 1, &a, 0, MARGIN), "{name}");
        }
        let mut off = b;
        off.w[1] = bit(off.w[1]);
        assert!(!cylinder_apart(&a, 0, &off, 1, MARGIN), "{name}");
        let mut off = b;
        off.c[1].x = bit(off.c[1].x);
        assert!(!cylinder_apart(&off, 1, &a, 0, MARGIN), "{name}");
    }
}

/// A long edge barely curved (its control point 5e-8 off a chord 147
/// long, five resolutions), in a tilted frame, with two patches folded
/// up out of its plane, both far corners truly outside the cylinder over
/// it (by exact rational arithmetic on these values). Its control triangle
/// is so flat that the plane's normal tilts by about `1e-7` rad under
/// rounding, which moves the far corners' coordinates by more than they
/// are from the cylinder: without the rounding bound the coefficients
/// showed the two on opposite sides.
#[test]
fn rounding_does_not_make_up_a_cylinder() {
    let p = DVec3::new(-8.452028400071006, 66.86895641488572, -19.590011613882453);
    let c = DVec3::new(-19.7162975792843, -2.1354513415981318, 2.868569447024921);
    let q = DVec3::new(-30.980566659169703, -71.13985911888999, 25.32715049381794);
    let ra = DVec3::new(-20.00960990664421, -2.5570772890284044, 1.426002896703111);
    let rb = DVec3::new(-20.05629314429112, -2.624182477776339, 1.1964066302263376);
    let mid = |x: DVec3, y: DVec3| (x + y) / 2.0;
    let a = Patch {
        p: [p, q, ra],
        c: [c, mid(q, ra), mid(ra, p)],
        w: [1.0; 3],
    };
    let b = Patch {
        p: [q, p, rb],
        c: [c, mid(p, rb), mid(rb, q)],
        w: [1.0; 3],
    };
    let margin = 1e-8;
    assert!(!straight(p, c, q, margin));
    assert!(!edge_neighbours_apart(&a, 0, &b, 0, margin));
    assert!(!cylinder_apart(&a, 0, &b, 0, margin));
    assert!(!cylinder_apart(&b, 0, &a, 0, margin));
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

/// The centres of the two arcs from `p` to `q` turning through `2·half`
/// each, bulging either side of the chord: a lens. The first arc runs from
/// `p` to `q` (bulging right of the chord), the second back.
fn lens_centres(p: [f64; 2], q: [f64; 2], half: f64) -> ([f64; 2], [f64; 2], f64) {
    let (m, d) = (
        [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0],
        [q[0] - p[0], q[1] - p[1]],
    );
    let length = d[0].hypot(d[1]);
    let right = [d[1] / length, -d[0] / length];
    let t = 0.5 * length / half.tan();
    let c1 = [m[0] - right[0] * t, m[1] - right[1] * t];
    let c2 = [m[0] + right[0] * t, m[1] + right[1] * t];
    (c1, c2, (0.5 * length).hypot(t))
}

/// Creases of revolved profiles where both faces leave the ring on one
/// side of its plane and on one side of the cylinder over it (or one
/// along it): neither the plane nor the cylinder parts them, a member of
/// their pencil does, either way round.
#[test]
fn creases_are_parted_by_the_pencil() {
    use Piece::{Arc, Line};
    let (c1, c2, r) = lens_centres([2.0, 0.0], [6.0, 2.0], 20f64.to_radians());
    let (t1, t2, s) = lens_centres([2.0, 0.0], [6.0, 2.0], 5f64.to_radians());
    // The point `deg` degrees round the circle about `centre` through the
    // tip `(2, 0)`, towards the lens's other tip.
    let along = |centre: [f64; 2], deg: f64| {
        let (x, y) = (2.0 - centre[0], 0.0 - centre[1]);
        let turned = [-deg, deg].map(|d: f64| {
            let (sin, cos) = d.to_radians().sin_cos();
            [centre[0] + x * cos - y * sin, centre[1] + x * sin + y * cos]
        });
        let far = |p: [f64; 2]| (p[0] - 6.0).hypot(p[1] - 2.0);
        if far(turned[0]) < far(turned[1]) {
            turned[0]
        } else {
            turned[1]
        }
    };
    let round = |deg: f64| {
        let a = deg.to_radians();
        [5.3 + 2.0 * a.cos(), 1.0 + 2.0 * a.sin()]
    };
    let (top, end) = (round(100.0), round(200.0));
    let cases = [
        // A triangle's inner corner: a wall up, a cone up and out.
        (
            "triangle",
            Line([2.0, 2.0], [2.0, 0.0]),
            Line([2.0, 0.0], [5.0, 1.0]),
        ),
        // Two lines leaving a corner up and out.
        (
            "one quadrant",
            Line([4.0, 3.0], [2.0, 0.0]),
            Line([2.0, 0.0], [6.0, 1.0]),
        ),
        // A D: a wall down onto the bottom of a circle, the arc leaving
        // out along the ring's plane and the wall up along its cylinder.
        (
            "D bottom",
            Line([3.0, 4.0], [3.0, 0.0]),
            Arc([3.0, 2.0], 2.0, [3.0, 0.0], [5.0, 2.0]),
        ),
        (
            "D top",
            Arc([3.0, 2.0], 2.0, [5.0, 2.0], [3.0, 4.0]),
            Line([3.0, 4.0], [3.0, 0.0]),
        ),
        // A round just past its turn against a wall straight down.
        (
            "round past turn",
            Line([top[0], end[1]], top),
            Arc([5.3, 1.0], 2.0, top, end),
        ),
        // The tips of lenses of two arcs (an eighth of each: a coarse
        // piece's curvature spoils the first-order picture, and repair
        // would split it).
        (
            "lens tip",
            Arc(c2, r, along(c2, 5.0), [2.0, 0.0]),
            Arc(c1, r, [2.0, 0.0], along(c1, 5.0)),
        ),
        (
            "thin lens tip",
            Arc(t2, s, along(t2, 1.25), [2.0, 0.0]),
            Arc(t1, s, [2.0, 0.0], along(t1, 1.25)),
        ),
    ];
    for (name, below, above) in cases {
        let (a, b) = ring_pair(&below, &above);
        for (x, ex, y, ey) in [(&a, 0, &b, 1), (&b, 1, &a, 0)] {
            assert!(!edge_neighbours_apart(x, ex, y, ey, MARGIN), "{name}");
            assert!(!cylinder_apart(x, ex, y, ey, MARGIN), "{name}");
            assert!(pencil_apart(x, ex, y, ey, MARGIN), "{name}");
            assert!(edge_neighbours_parted(x, ex, y, ey, MARGIN), "{name}");
        }
        // Either way round the member is the same up to its sign: `F`
        // keeps its sign, the plane's normal turns over.
        let (alpha, beta) = pencil_member(&a, 0, &b, 1, MARGIN).unwrap();
        let (alpha2, beta2) = pencil_member(&b, 1, &a, 0, MARGIN).unwrap();
        assert!(alpha * alpha2 <= 0.0 && beta * beta2 >= 0.0, "{name}");
        // The edge off by a bit of its weight or control point: the
        // patches no longer share it.
        let bit = |x: f64| f64::from_bits(x.to_bits() + 1);
        let mut off = b;
        off.w[1] = bit(off.w[1]);
        assert!(!pencil_apart(&a, 0, &off, 1, MARGIN), "{name}");
        let mut off = b;
        off.c[1].z = bit(off.c[1].z);
        assert!(!pencil_apart(&off, 1, &a, 0, MARGIN), "{name}");
    }
}

/// The pencil's search on made-up coefficients where its points are
/// degenerate: all on one line, all the same, one at the origin, not
/// finite. It finds the member where there is one and never divides by
/// a zero length.
#[test]
fn the_pencil_search_takes_degenerate_points() {
    use Piece::Line;
    let (a, b) = ring_pair(&Line([2.0, 2.0], [2.0, 0.0]), &Line([2.0, 0.0], [5.0, 1.0]));
    let pair = CurvedPair::new(&a, 0, &b, 1, MARGIN).unwrap();
    let g = 4.0 * pair.edge.w2 / pair.edge.height();
    let k = |f: f64, p: f64| Coefficient {
        f: f * g,
        ef: 0.0,
        p,
        ep: 0.0,
    };
    let member = |a: [Coefficient; 10], b: [Coefficient; 10]| {
        let pair = CurvedPair {
            edge: pair.edge,
            a,
            b,
        };
        super::pencil_member(&pair, MARGIN)
    };
    let spread = |i: usize| (i as f64 - 4.5) / 10.0;
    // `F` alone parts them, `P` varying either way: the points lie on a
    // line square to the `F` axis, and the member found is `F`'s.
    let (alpha, beta) = member(
        std::array::from_fn(|i| k(1.0, spread(i))),
        std::array::from_fn(|i| k(-1.0, spread(i))),
    )
    .unwrap();
    assert!(alpha > 0.0 && beta == 0.0, "{alpha} {beta}");
    // Every point the same: one direction, segments of no length.
    let (alpha, beta) = member([k(1.0, 1.0); 10], [k(-1.0, -1.0); 10]).unwrap();
    assert!(alpha > 0.0 && beta > 0.0 && (alpha * g - beta).abs() <= 1e-12 * beta);
    // Either side of the origin on one line: no member.
    assert_eq!(member([k(1.0, 0.0); 10], [k(1.0, 0.0); 10]), None);
    // A coefficient on the zero set, or not finite.
    for bad in [k(0.0, 0.0), k(f64::NAN, 1.0), k(1.0, f64::INFINITY)] {
        let (mut ka, kb) = ([k(1.0, 1.0); 10], [k(-1.0, -1.0); 10]);
        ka[3] = bad;
        assert_eq!(member(ka, kb), None);
        assert_eq!(member(kb, ka), None);
    }
}

#[test]
fn the_pencil_refuses_folds() {
    use Piece::{Arc, Line};
    // A profile turning back on itself: the cone's two strips lie on each
    // other.
    let (a, b) = ring_pair(&Line([5.0, 1.0], [2.0, 0.0]), &Line([2.0, 0.0], [5.0, 1.0]));
    assert!(!pencil_apart(&a, 0, &b, 1, MARGIN));
    assert!(!pencil_apart(&b, 1, &a, 0, MARGIN));
    // An arc turning back along the cone, tangent to it at the ring and
    // bending away from it, but on the side of the cone away from where
    // the pencil's parabolas bend: no member passes between the two.
    let (a, b) = ring_pair(
        &Line([5.0, 1.0], [2.0, 0.0]),
        &Arc(
            [2.0 + 1.0, -3.0],
            10f64.sqrt(),
            [2.0, 0.0],
            [2.0 + 1.0 + 10f64.sqrt(), -3.0],
        ),
    );
    assert!(!edge_neighbours_parted(&a, 0, &b, 1, MARGIN));
    assert!(!edge_neighbours_parted(&b, 1, &a, 0, MARGIN));
}

/// A pair from an adversarial hunt (an edge some 250 out, weights at
/// both limits, a margin of `6e-13`): with the rounding bounds zeroed the
/// pencil passed it, and exact rational arithmetic on these values shows
/// the member it chose has coefficients of the wrong sign. With them it
/// is refused.
#[test]
fn rounding_does_not_make_up_a_pencil() {
    let v = DVec3::new;
    let a = Patch {
        p: [
            v(-160.5231787678521, -96.0347965575042, 162.34118757734277),
            v(-161.19436234639616, -95.84506881712997, 161.9603534193856),
            v(-161.15380992307692, -95.85653202911998, 161.98336314203456),
        ],
        c: [
            v(-160.99966166839172, -95.90010609883242, 162.07082791716695),
            v(-161.17040967833125, -95.8500660058536, 161.96924734463906),
            v(-160.83856980546628, -95.94393594784135, 162.16362719048277),
        ],
        w: [0.30320805521398614, 0.015625, 64.0],
    };
    let b = Patch {
        p: [
            v(-160.5231787678521, -96.0347965575042, 162.34118757734277),
            v(-160.59487367096213, -96.0158794174747, 162.30564656240352),
            v(-161.19436234639616, -95.84506881712997, 161.9603534193856),
        ],
        c: [
            v(-160.55902638926102, -96.02534000914608, 162.3234159725943),
            v(-160.89767007791013, -95.93093588624055, 162.13499737685885),
            v(-160.99966166839172, -95.90010609883242, 162.07082791716695),
        ],
        w: [0.12441390840394971, 0.015625, 0.30320805521398614],
    };
    let margin = 6.225597036426252e-13;
    assert!(!pencil_apart(&a, 0, &b, 2, margin));
    assert!(!pencil_apart(&b, 2, &a, 0, margin));
    assert!(!edge_neighbours_parted(&a, 0, &b, 2, margin));
}

/// Random pairs sharing a random conic edge, the far corners leaving it
/// into one quadrant of (across, up) or anywhere: wherever the pencil
/// parts them, `G = α·F + β·P` evaluated at points of each, away from the
/// edge, has the promised signs, positive on the first and negative on
/// the second.
#[test]
fn the_pencil_rule_is_sound_on_random_pairs() {
    use std::f64::consts::FRAC_PI_2;
    let mut rng = Rng::new(96);
    let (mut tried, mut passed, mut only) = (0, 0, 0);
    let sign = |rng: &mut Rng| if rng.unit() < 0.5 { 1.0 } else { -1.0 };
    // Quick mode runs the first fifth.
    let cases = varde_testing::pick(20_000, 100_000);
    for _ in 0..cases {
        // An edge from `p` to `q` across the x axis in the xy plane, its
        // control point above it, in a random frame.
        let length = rng.log_range(1e-1, 1e1);
        let (p, q) = (DVec3::new(-length, 0.0, 0.0), DVec3::new(length, 0.0, 0.0));
        let c = DVec3::new(
            rng.range(-0.5, 0.5) * length,
            rng.log_range(1e-3, 2.0) * length,
            0.0,
        );
        let w = rng.log_range(0.2, 5.0);
        let margin = length * rng.log_range(1e-9, 1e-6);
        // A point of the edge at `t`, moved `across` in its plane (square
        // to it) and `up` out of it.
        let near = |t: f64, across: f64, up: f64| {
            let b = [(1.0 - t) * (1.0 - t), 2.0 * w * t * (1.0 - t), t * t];
            let wt = b[0] + b[1] + b[2];
            let x = (p * b[0] + c * b[1] + q * b[2]) / wt;
            let db = [-2.0 * (1.0 - t), 2.0 * w * (1.0 - 2.0 * t), 2.0 * t];
            let dw = db[0] + db[1] + db[2];
            let tangent = ((p * db[0] + c * db[1] + q * db[2]) - x * dw) / wt;
            x + DVec3::Z.cross(tangent).normalize() * across + DVec3::Z * up
        };
        let far = |rng: &mut Rng, angle: f64| {
            let reach = length * rng.log_range(1e-2, 2.0);
            near(
                rng.range(0.1, 0.9),
                reach * angle.cos(),
                reach * angle.sin(),
            )
        };
        let (ra, rb) = if rng.unit() < 0.8 {
            // A crease into one quadrant.
            let (sx, sy) = (sign(&mut rng), sign(&mut rng));
            let fa = rng.range(0.0, FRAC_PI_2);
            let fb = rng.range(0.0, FRAC_PI_2);
            let mirrored = |f: f64| (sy * f.sin()).atan2(sx * f.cos());
            (far(&mut rng, mirrored(fa)), far(&mut rng, mirrored(fb)))
        } else {
            let fa = rng.range(-3.2, 3.2);
            let fb = rng.range(-3.2, 3.2);
            (far(&mut rng, fa), far(&mut rng, fb))
        };
        let m = rotation(&mut rng);
        let o = rng.point(1.0) * rng.log_range(1e-2, 1e2);
        let g = |x: DVec3| m * x + o;
        let ctl =
            |x: DVec3, y: DVec3, rng: &mut Rng| (x + y) / 2.0 + rng.point(0.2 * (x - y).length());
        let (pg, cg, qg, rag, rbg) = (g(p), g(c), g(q), g(ra), g(rb));
        let a = Patch {
            p: [pg, qg, rag],
            c: [cg, ctl(qg, rag, &mut rng), ctl(rag, pg, &mut rng)],
            w: [w, rng.log_range(0.3, 3.0), rng.log_range(0.3, 3.0)],
        };
        let b = Patch {
            p: [qg, pg, rbg],
            c: [cg, ctl(pg, rbg, &mut rng), ctl(rbg, qg, &mut rng)],
            w: [w, rng.log_range(0.3, 3.0), rng.log_range(0.3, 3.0)],
        };
        if a.check().is_err() || b.check().is_err() {
            continue;
        }
        tried += 1;
        // The shared pass gives the rules' answer. Each rule is asked
        // once: the pencil's member is most of the time here.
        let plane = edge_neighbours_apart(&a, 0, &b, 0, margin);
        let pair = CurvedPair::new(&a, 0, &b, 0, margin);
        let cylinder = (pair.as_ref()).is_some_and(|pair| super::cylinder_apart(pair, margin));
        let member = (pair.as_ref()).and_then(|pair| super::pencil_member(pair, margin));
        assert_eq!(
            edge_neighbours_parted(&a, 0, &b, 0, margin),
            plane || cylinder || member.is_some()
        );
        let Some((alpha, beta)) = member else {
            continue;
        };
        passed += 1;
        if !plane && !cylinder {
            only += 1;
        }
        let (e1, e2) = (pg - cg, qg - cg);
        let n = e1.cross(e2);
        let nn = n.dot(n);
        let gee = |x: DVec3| {
            let s = x - cg;
            let lp = n.dot(s.cross(e2)) / nn;
            let lq = n.dot(e1.cross(s)) / nn;
            let lc = 1.0 - lp - lq;
            alpha * (lc * lc - 4.0 * w * w * lp * lq) + beta * n.dot(s) / nn.sqrt()
        };
        let sign_of = |x: &Patch| {
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for i in 0..=30 {
                for j in 0..=30 - i {
                    let far = (30 - i - j) as f64 / 30.0;
                    if far < 0.02 {
                        continue;
                    }
                    let v = gee(x.eval(DVec3::new(i as f64 / 30.0, j as f64 / 30.0, far)));
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
        assert!(
            sign_of(&a) == 1.0 && sign_of(&b) == -1.0,
            "{a:?} {b:?} {margin:e}"
        );
    }
    assert!(
        tried * 2 > cases && passed * 20 > cases && only * 100 > cases,
        "tried {tried}, passed {passed}, pencil only {only}"
    );
}
