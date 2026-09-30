use glam::DVec3;

use super::*;
use crate::mesh::tests::{OCTAHEDRON, TOL, UNIT};
use crate::mesh::{Face, FaceName, FacePart, MeshBuilder, Surface};
use crate::par::assert_deterministic;

fn cube(min: [f64; 3], size: [f64; 3]) -> Solid {
    Solid::cuboid(DVec3::from(min), DVec3::from(size), 1, &TOL).unwrap()
}

/// The convex solid on `verts` with triangles `tris` (counter-clockwise
/// from outside), each its own plane face of feature 2.
fn polytope(verts: &[DVec3], tris: &[[u32; 3]]) -> Solid {
    let mut builder = MeshBuilder::new();
    for &p in verts {
        builder.vert(p);
    }
    for (i, &[a, b, c]) in tris.iter().enumerate() {
        let [p, q, r] = [a, b, c].map(|v| verts[v as usize]);
        let n = (q - p).cross(r - p);
        let f = builder.face(Face {
            name: FaceName {
                feature: 2,
                part: FacePart::Split(i as u32),
            },
            surface: Surface::Plane { n, d: n.dot(p) },
        });
        builder.tri([a, b, c], f);
    }
    Solid::new(builder.build().unwrap(), &TOL).unwrap()
}

/// The octahedron `|x − c|₁ ≤ r`.
fn octahedron(c: [f64; 3], r: f64) -> Solid {
    let verts = UNIT.map(|u| DVec3::from(c) + u * r);
    polytope(&verts, &OCTAHEDRON)
}

/// A square prism along `y` from `y0` to `y1` whose cross-section is the
/// diamond `|x − cx| + |z − cz| ≤ r`.
fn diamond(cx: f64, cz: f64, r: f64, y0: f64, y1: f64) -> Solid {
    let ring = [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)];
    let mut verts = Vec::new();
    for y in [y0, y1] {
        for (dx, dz) in ring {
            verts.push(DVec3::new(cx + dx, y, cz + dz));
        }
    }
    // Seen from −y the ring (x, z) runs clockwise, so the y0 cap is
    // (0, 1, 2), (0, 2, 3) reversed.
    let mut tris = vec![[0, 1, 2], [0, 2, 3], [4, 6, 5], [4, 7, 6]];
    for k in 0..4u32 {
        let n = (k + 1) % 4;
        tris.push([k, 4 + k, 4 + n]);
        tris.push([k, 4 + n, n]);
    }
    polytope(&verts, &tris)
}

fn run(a: &Solid, b: &Solid, op: Op) -> Result<Solid, KernelError> {
    boolean(a, b, op, &TOL, &Budget::DEFAULT)
}

/// The volumes of `a ∪ b`, `a ∩ b`, `a − b` and `b − a` from those of
/// `a`, `b` and their intersection.
fn volumes(va: f64, vb: f64, both: f64) -> [f64; 4] {
    [va + vb - both, both, va - both, vb - both]
}

/// Runs the four operations both ways round (union and intersection
/// swapped give the same volume), checking each result's volume, or
/// that it fails as invalid where `expect` has `None`.
fn case(name: &str, a: &Solid, b: &Solid, expect: [Option<f64>; 4]) {
    let jobs = [
        (a, b, Op::Union, expect[0]),
        (b, a, Op::Union, expect[0]),
        (a, b, Op::Intersection, expect[1]),
        (b, a, Op::Intersection, expect[1]),
        (a, b, Op::Difference, expect[2]),
        (b, a, Op::Difference, expect[3]),
    ];
    for (x, y, op, want) in jobs {
        let swapped = !std::ptr::eq(x, a);
        let got = run(x, y, op);
        match (want, got) {
            (Some(v), Ok(solid)) => {
                let vol = solid.volume();
                assert!(
                    (vol - v).abs() < 1e-9 * v.max(1.0),
                    "{name}: {op:?} (swapped {swapped}) has volume {vol}, not {v}"
                );
                if v == 0.0 {
                    assert!(
                        solid.is_empty(),
                        "{name}: {op:?} (swapped {swapped}) not empty"
                    );
                }
                faces_face_out(&solid);
            }
            (None, Err(KernelError::Invalid(_))) => {}
            (want, got) => panic!(
                "{name}: {op:?} (swapped {swapped}) gave {:?}, wanted {want:?}",
                got.map(|s| s.volume())
            ),
        }
    }
}

/// Checks that every patch is on its face's surface, and that each plane
/// face's normal points the way its triangles face.
fn faces_face_out(solid: &Solid) {
    let mesh = solid.mesh();
    mesh.check_faces(&TOL).unwrap();
    for (t, tri) in mesh.tris().iter().enumerate() {
        if let Surface::Plane { n, .. } = mesh.faces()[tri.face as usize].surface {
            let p = mesh.patch(t).p;
            assert!((p[1] - p[0]).cross(p[2] - p[0]).dot(n) > 0.0, "patch {t}");
        }
    }
}

/// [`case`] for boxes of the given volumes and overlap.
fn boxes(name: &str, a: &Solid, b: &Solid, va: f64, vb: f64, both: f64) {
    case(name, a, b, volumes(va, vb, both).map(Some));
}

#[test]
fn boxes_overlapping_and_apart() {
    let a = cube([0.0; 3], [2.0; 3]);
    boxes("corner", &a, &cube([1.0; 3], [2.0; 3]), 8.0, 8.0, 1.0);
    boxes(
        "skew",
        &a,
        &cube([0.5, 1.25, -0.75], [1.0, 3.0, 2.0]),
        8.0,
        6.0,
        0.75 * 1.25 * 1.0,
    );
    boxes("inside", &a, &cube([0.5; 3], [1.0; 3]), 8.0, 1.0, 1.0);
    boxes(
        "through",
        &a,
        &cube([0.5, 0.5, -1.0], [1.0, 1.0, 4.0]),
        8.0,
        4.0,
        2.0,
    );
    boxes(
        "cross",
        &a,
        &cube([-1.0, 0.5, 0.5], [4.0, 1.0, 1.0]),
        8.0,
        4.0,
        2.0,
    );
    // Apart: the union is both, and fails only if they're too close.
    boxes("apart", &a, &cube([3.0; 3], [1.0; 3]), 8.0, 1.0, 0.0);
}

#[test]
fn boxes_flush() {
    let a = cube([0.0; 3], [2.0; 3]);
    // The same box.
    boxes("same", &a, &a.clone(), 8.0, 8.0, 8.0);
    // Sharing one face's plane, inside: a pocket.
    boxes(
        "pocket",
        &a,
        &cube([0.5, 0.5, 1.0], [1.0, 1.0, 1.0]),
        8.0,
        1.0,
        1.0,
    );
    // Sharing four faces' planes, sticking out.
    boxes(
        "slid",
        &a,
        &cube([1.0, 0.0, 0.0], [2.0, 2.0, 2.0]),
        8.0,
        8.0,
        4.0,
    );
    // Sharing two faces' planes.
    boxes(
        "step",
        &a,
        &cube([1.0, 1.0, 0.0], [2.0, 2.0, 2.0]),
        8.0,
        8.0,
        2.0,
    );
    // Sharing one face's plane, sticking out.
    boxes(
        "ledge",
        &a,
        &cube([1.0, 1.0, 1.0], [2.0, 2.0, 1.0]),
        8.0,
        4.0,
        1.0,
    );
    // A slab through, flush with two sides.
    boxes(
        "slab",
        &a,
        &cube([-1.0, 0.0, 0.5], [4.0, 2.0, 1.0]),
        8.0,
        8.0,
        4.0,
    );
    // Inside, flush with three faces at a corner.
    boxes(
        "corner inside",
        &a,
        &cube([1.0; 3], [1.0; 3]),
        8.0,
        1.0,
        1.0,
    );
}

#[test]
fn boxes_face_to_face() {
    let a = cube([0.0; 3], [2.0; 3]);
    // Touching over a whole face: the union is one box.
    boxes("full", &a, &cube([2.0, 0.0, 0.0], [2.0; 3]), 8.0, 8.0, 0.0);
    // Over part of a face.
    boxes("part", &a, &cube([2.0, 0.5, 0.5], [1.0; 3]), 8.0, 1.0, 0.0);
    boxes(
        "offset",
        &a,
        &cube([2.0, 1.0, 1.0], [2.0; 3]),
        8.0,
        8.0,
        0.0,
    );
    // Standing on it.
    boxes(
        "on top",
        &a,
        &cube([0.5, 0.5, 2.0], [1.0, 1.0, 3.0]),
        8.0,
        3.0,
        0.0,
    );
    // Over a face larger than the other's.
    boxes(
        "larger",
        &a,
        &cube([2.0, -1.0, -1.0], [1.0, 4.0, 4.0]),
        8.0,
        16.0,
        0.0,
    );
}

#[test]
fn boxes_touching_along_an_edge_or_a_corner() {
    let a = cube([0.0; 3], [2.0; 3]);
    // The union isn't a manifold; the rest are what they'd be apart.
    for (name, b) in [
        ("edge", cube([2.0, 2.0, 0.0], [2.0; 3])),
        ("corner", cube([2.0; 3], [2.0; 3])),
    ] {
        case(name, &a, &b, [None, Some(0.0), Some(8.0), Some(8.0)]);
    }
}

#[test]
fn edge_and_vertex_on_faces() {
    let a = cube([0.0; 3], [2.0; 3]);
    // A diamond prism whose four long edges lie on four faces of the box:
    // what is left of the box is four prisms touching along those edges,
    // not a manifold.
    let d = diamond(1.0, 1.0, 1.0, -1.0, 3.0);
    case("diamond", &a, &d, [Some(12.0), Some(4.0), None, Some(4.0)]);
    // Shorter, inside: the box less it has a void touching its skin.
    let d = diamond(1.0, 1.0, 1.0, 0.5, 1.5);
    case(
        "diamond inside",
        &a,
        &d,
        [Some(8.0), Some(2.0), None, Some(0.0)],
    );
    // Half in the box, its side edges on the box's top edges.
    let d = diamond(1.0, 2.0, 1.0, 0.5, 1.5);
    case("diamond on edges", &a, &d, volumes(8.0, 2.0, 1.0).map(Some));
    // An octahedron half in the box, its middle vertices on the box's top
    // edges and its middle square on the top face.
    let o = octahedron([1.0, 1.0, 2.0], 1.0);
    case(
        "octahedron on edges",
        &a,
        &o,
        volumes(8.0, 4.0 / 3.0, 2.0 / 3.0).map(Some),
    );
    // One inside touching the top face from below with a vertex: the
    // difference has a void touching its skin at a point.
    let o = octahedron([1.0, 1.0, 1.5], 0.5);
    case(
        "octahedron touching",
        &a,
        &o,
        [Some(8.0), Some(1.0 / 6.0), None, Some(0.0)],
    );
    // An octahedron with a vertex on a face from outside.
    let o = octahedron([1.0, 1.0, 2.5], 0.5);
    case(
        "octahedron resting",
        &a,
        &o,
        [None, Some(0.0), Some(8.0), Some(1.0 / 6.0)],
    );
    // Poking through the top face with its lower half.
    let o = octahedron([1.0, 1.0, 2.25], 0.5);
    let below = 0.25f64.powi(3) * 2.0 / 3.0;
    case(
        "octahedron poking",
        &a,
        &o,
        volumes(8.0, 1.0 / 6.0, below).map(Some),
    );
}

#[test]
fn rotated_boxes() {
    // A cube turned 45° about z through a box: coordinates with rounding.
    let a = cube([0.0; 3], [2.0; 3]);
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let c = DVec3::new(1.0, 1.0, 0.0);
    let ring = [(h, 0.0), (0.0, h), (-h, 0.0), (0.0, -h)].map(|(x, y)| c + DVec3::new(x, y, 0.0));
    let mut verts: Vec<DVec3> = ring.iter().map(|&p| p + DVec3::Z * 0.5).collect();
    verts.extend(ring.iter().map(|&p| p + DVec3::Z * 3.0));
    let tris = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    let b = polytope(&verts, &tris);
    // Its cross-section, a square of side 1, lies inside the box's.
    boxes("turned", &a, &b, 8.0, 2.5, 1.5);
}

#[test]
fn touching() {
    let tol = TOL;
    let a = cube([0.0; 3], [2.0; 3]);
    let t = |b: &Solid| touches(&a, b, &tol, &Budget::DEFAULT).unwrap();
    assert!(t(&cube([1.0; 3], [2.0; 3])));
    assert!(t(&cube([0.5; 3], [1.0; 3])));
    assert!(t(&cube([2.0, 0.0, 0.0], [2.0; 3])));
    assert!(t(&cube([2.0; 3], [2.0; 3])));
    assert!(!t(&cube([3.0; 3], [1.0; 3])));
    assert!(!t(&Solid::empty()));
    assert!(touches(&cube([0.5; 3], [1.0; 3]), &a, &tol, &Budget::DEFAULT).unwrap());
}

#[test]
fn empty_operands() {
    let a = cube([0.0; 3], [2.0; 3]);
    let e = Solid::empty();
    assert_eq!(run(&a, &e, Op::Union).unwrap(), a);
    assert_eq!(run(&e, &a, Op::Union).unwrap(), a);
    assert_eq!(run(&a, &e, Op::Difference).unwrap(), a);
    assert!(run(&e, &a, Op::Difference).unwrap().is_empty());
    assert!(run(&a, &e, Op::Intersection).unwrap().is_empty());
}

#[test]
fn refusals() {
    let a = cube([0.0; 3], [2.0; 3]);
    let cyl = Solid::cylinder(DVec3::ONE, 0.5, 1.0, 3, &TOL).unwrap();
    assert_eq!(
        run(&a, &cyl, Op::Union),
        Err(KernelError::Boolean(BooleanError::Curved))
    );
    // Inside out: every triangle reversed.
    let m = a.mesh();
    let mut builder = MeshBuilder::new();
    for &p in m.verts() {
        builder.vert(p);
    }
    for &f in m.faces() {
        builder.face(f);
    }
    for t in m.tris() {
        let c = t.halfedges.map(|h| h.start);
        builder.tri([c[0], c[2], c[1]], t.face);
    }
    let inverted = Solid::new(builder.build().unwrap(), &TOL).unwrap();
    assert_eq!(
        run(&inverted, &cube([1.0; 3], [2.0; 3]), Op::Union),
        Err(KernelError::Boolean(BooleanError::InsideOut))
    );
    assert_eq!(
        boolean(
            &a,
            &cube([1.0; 3], [2.0; 3]),
            Op::Union,
            &TOL,
            &Budget::new(10)
        ),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn chained_results_feed_on() {
    // Each result is the next one's input: steps joined flush, a hole
    // through them all, then half cut away.
    let mut s = cube([0.0; 3], [4.0, 4.0, 1.0]);
    s = run(&s, &cube([1.0, 0.0, 1.0], [3.0, 4.0, 1.0]), Op::Union).unwrap();
    s = run(&s, &cube([2.0, 0.0, 2.0], [2.0, 4.0, 1.0]), Op::Union).unwrap();
    assert!((s.volume() - 36.0).abs() < 1e-9, "{}", s.volume());
    s = run(&s, &cube([1.5, 1.5, -1.0], [1.0, 1.0, 5.0]), Op::Difference).unwrap();
    assert!((s.volume() - 33.5).abs() < 1e-9, "{}", s.volume());
    s = run(
        &s,
        &cube([0.0, 0.0, 0.0], [4.0, 2.0, 3.0]),
        Op::Intersection,
    )
    .unwrap();
    assert!((s.volume() - 16.75).abs() < 1e-9, "{}", s.volume());
    // Filling the half hole back in, flush with the steps.
    s = run(&s, &cube([1.5, 1.5, 0.0], [1.0, 0.5, 1.0]), Op::Union).unwrap();
    assert!((s.volume() - 17.25).abs() < 1e-9, "{}", s.volume());
}

/// The box from `min` to `max`, its corners moved by `f`.
fn moved_box(min: DVec3, max: DVec3, f: impl Fn(DVec3) -> DVec3) -> Solid {
    let verts: Vec<DVec3> = (0..8)
        .map(|i| {
            let pick = |bit: u32, lo: f64, hi: f64| if i >> bit & 1 == 0 { lo } else { hi };
            f(DVec3::new(
                pick(0, min.x, max.x),
                pick(1, min.y, max.y),
                pick(2, min.z, max.z),
            ))
        })
        .collect();
    let tris = [
        [0, 2, 1],
        [1, 2, 3],
        [4, 5, 6],
        [5, 7, 6],
        [0, 1, 4],
        [1, 5, 4],
        [2, 6, 3],
        [3, 6, 7],
        [0, 4, 2],
        [2, 4, 6],
        [1, 3, 5],
        [3, 7, 5],
    ];
    polytope(&verts, &tris)
}

#[test]
fn turned_and_moved() {
    // Boxes in general position, turned and moved at random together:
    // the volumes don't change.
    let mut rng = crate::test_rng::Rng::new(11);
    for i in 0..40 {
        let axis = rng.direction();
        let angle = rng.range(0.0, 6.0);
        let q = glam::DQuat::from_axis_angle(axis, angle);
        let shift = rng.point(100.0);
        let f = |p: DVec3| q * p + shift;
        let lo = DVec3::new(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        );
        let size = DVec3::new(
            rng.range(0.5, 2.0),
            rng.range(0.5, 2.0),
            rng.range(0.5, 2.0),
        );
        let a = moved_box(DVec3::ZERO, DVec3::splat(1.0), f);
        let b = moved_box(lo, lo + size, f);
        let overlap = (DVec3::splat(1.0).min(lo + size) - DVec3::ZERO.max(lo)).max(DVec3::ZERO);
        let both = overlap.x * overlap.y * overlap.z;
        let vb = size.x * size.y * size.z;
        let want = volumes(1.0, vb, both);
        for (op, v) in [
            (Op::Union, want[0]),
            (Op::Intersection, want[1]),
            (Op::Difference, want[2]),
        ] {
            match run(&a, &b, op) {
                Ok(s) => assert!(
                    (s.volume() - v).abs() < 1e-9,
                    "{i} {op:?}: {} not {v}",
                    s.volume()
                ),
                // Apart but too close for the resolution.
                Err(KernelError::Invalid(_)) if both == 0.0 && op == Op::Union => {}
                Err(e) => panic!("{i} {op:?}: {e}"),
            }
        }
    }
}

#[test]
fn faces_keep_their_names() {
    let a = cube([0.0; 3], [2.0; 3]);
    let b = polytope(&UNIT.map(|u| DVec3::splat(2.0) + u), &OCTAHEDRON);
    for op in [Op::Union, Op::Intersection, Op::Difference] {
        let r = run(&a, &b, op).unwrap();
        let mesh = r.mesh();
        let used = |feature| {
            mesh.tris()
                .iter()
                .any(|t| mesh.faces()[t.face as usize].name.feature == feature)
        };
        assert!(used(1) && used(2), "{op:?}");
        faces_face_out(&r);
    }
}

#[test]
fn deterministic() {
    let a = cube([0.0; 3], [2.0; 3]);
    let b = octahedron([1.0, 1.0, 2.0], 1.0);
    for op in [Op::Union, Op::Intersection, Op::Difference] {
        assert_deterministic(|| run(&a, &b, op).map(Solid::into_mesh)).unwrap();
    }
    let c = cube([1.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
    assert_deterministic(|| run(&a, &c, Op::Union).map(Solid::into_mesh)).unwrap();
    let t = Solid::new(crate::mesh::tests::torus(48, 24, 3.0, 1.0), &TOL).unwrap();
    let d = cube([0.5, -0.7, -2.0], [4.0, 1.3, 4.0]);
    assert_deterministic(|| run(&t, &d, Op::Difference).map(Solid::into_mesh)).unwrap();
}
