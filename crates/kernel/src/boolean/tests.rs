#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use glam::DVec3;

use super::*;
use crate::mesh::tests::{OCTAHEDRON, TOL, UNIT};
use crate::mesh::{CheckError, Face, FaceName, FacePart, Mesh, MeshBuilder, Surface};
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

/// The flat solid with `mesh`'s triangles and faces, its vertices moved
/// by `f`.
fn rebuilt(mesh: &Mesh, f: impl Fn(DVec3) -> DVec3) -> Solid {
    Solid::new(rebuilt_mesh(mesh, f, false), &TOL).unwrap()
}

/// The flat mesh with `mesh`'s triangles and faces, its vertices moved by
/// `f`, and every triangle reversed if `inverted`.
fn rebuilt_mesh(mesh: &Mesh, f: impl Fn(DVec3) -> DVec3, inverted: bool) -> Mesh {
    let mut builder = MeshBuilder::new();
    for &p in mesh.verts() {
        builder.vert(f(p));
    }
    for &face in mesh.faces() {
        builder.face(face);
    }
    for t in mesh.tris() {
        let [a, b, c] = t.halfedges.map(|h| h.start);
        builder.tri(if inverted { [a, c, b] } else { [a, b, c] }, t.face);
    }
    builder.build().unwrap()
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

/// Checks that each plane face's normal points the way its triangles
/// face (that every patch is on its face's surface, `Solid::new` checks).
fn faces_face_out(solid: &Solid) {
    let mesh = solid.mesh();
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
    // Boxes apart are told without any work.
    let apart = |b: &Solid| touches(&a, b, &tol, &Budget::new(0));
    assert_eq!(apart(&cube([2.1, 0.0, 0.0], [1.0; 3])), Ok(false));
    assert_eq!(
        apart(&cube([2.0, 0.0, 0.0], [1.0; 3])),
        Err(KernelError::TooComplex)
    );
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
    // Inside out, every triangle reversed: not a solid.
    let inverted = rebuilt_mesh(a.mesh(), |p| p, true);
    assert_eq!(
        Solid::new(inverted, &TOL),
        Err(KernelError::Invalid(CheckError::InsideOut(0)))
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
fn a_void_thinner_than_the_resolution_fails_at_once() {
    // A blind void in a cylinder, its wall half a resolution thick: the
    // result's round walls are within the resolution of each other over
    // an area. Repair used to split them until the whole budget was gone
    // (`TooComplex`, about a second); points of the two found within the
    // resolution now refuse it at once, well inside a small budget.
    let half = 0.5 * TOL.resolution();
    for r in [5.0, 1.0] {
        let a = Solid::cylinder(DVec3::ZERO, r, 10.0, 1, &TOL).unwrap();
        let b = Solid::cylinder(DVec3::Z * 2.0, r - half, 6.0, 2, &TOL).unwrap();
        let result = boolean(&a, &b, Op::Difference, &TOL, &Budget::new(100_000));
        assert!(
            matches!(result, Err(KernelError::Invalid(CheckError::Hull(..)))),
            "{r}: {result:?}"
        );
    }
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
fn tori_keep_the_volume_identities() {
    // Flat tori of 2 304 patches: one turned upright through the other's
    // hole, crossing its tube on both sides, and one through a box.
    // Nothing is flush, so every result's
    // volume follows from the others': |A ∪ B| + |A ∩ B| = |A| + |B| and
    // |A − B| = |A| − |A ∩ B|.
    let torus = crate::mesh::tests::torus(48, 24, 3.0, 1.0);
    let a = rebuilt(&torus, |p| p);
    let turn = glam::DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)
        * glam::DQuat::from_rotation_z(0.1);
    let upright = rebuilt(&torus, |p| turn * p + DVec3::new(1.5, 0.1, 0.2));
    let block = cube([0.5, -0.7, -2.0], [4.0, 1.3, 4.0]);
    for (name, b) in [("upright", &upright), ("box", &block)] {
        let (va, vb) = (a.volume(), b.volume());
        let [union, both, less] =
            [Op::Union, Op::Intersection, Op::Difference].map(|op| run(&a, b, op).unwrap());
        let [union, both, less] = [&union, &both, &less].map(|s| {
            faces_face_out(s);
            s.volume()
        });
        assert!(both > 0.1, "{name}: {both}");
        let scale = 1e-9 * (va + vb);
        assert!(
            (union + both - va - vb).abs() < scale,
            "{name}: {union} + {both}"
        );
        assert!((less - (va - both)).abs() < scale, "{name}: {less}");
    }
}

#[test]
fn signs_worked_out_exactly_are_counted() {
    // A flat torus against itself: every primitive is a tie, worked out
    // exactly with the perturbation, some ten microseconds each. The
    // counting charges them, far more than for the same torus moved off
    // itself, where floating point tells every sign.
    let torus = crate::mesh::tests::torus(24, 12, 3.0, 1.0);
    let a = rebuilt(&torus, |p| p);
    let moved = rebuilt(&torus, |p| p + DVec3::new(0.31, 0.17, 0.23));
    let spent = |b: &Solid| {
        let mut work = Work::new(&Budget::DEFAULT);
        let (ia, ib) = (Input::new(a.mesh(), &TOL), Input::new(b.mesh(), &TOL));
        let prims = flat::Flat::tied(&ia, &ib, true, tie(&TOL));
        count::count(&ia, &ib, &prims, &TOL, &mut work).unwrap();
        Budget::DEFAULT.work() - work.left()
    };
    let (itself, off) = (spent(&a), spent(&moved));
    assert!(itself > 5 * off, "{itself} {off}");
}

#[test]
fn unused_faces_are_dropped() {
    // A box less one through it: its two end faces go, and a chain of
    // results carries only faces it uses.
    let a = cube([0.0; 3], [2.0; 3]);
    let b = cube([-1.0, 0.5, 0.5], [4.0, 1.0, 1.0]);
    let r = run(&a, &b, Op::Difference).unwrap();
    let mesh = r.mesh();
    let mut used = vec![false; mesh.faces().len()];
    for t in mesh.tris() {
        used[t.face as usize] = true;
    }
    assert!(used.iter().all(|&u| u), "{used:?}");
    assert_eq!(mesh.faces().len(), 10);
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

/// A box on the half grid: corners at `−0.5 + 0.5·min` and sizes
/// `0.5·size`, with its cells in a grid of 8³.
fn grid_box(min: [i32; 3], size: [i32; 3]) -> (Solid, Cells) {
    let at = |i: i32| -0.5 + 0.5 * f64::from(i);
    let lo = DVec3::new(at(min[0]), at(min[1]), at(min[2]));
    let solid = cube(lo.to_array(), size.map(|s| 0.5 * f64::from(s)));
    let mut cells = [[[false; 8]; 8]; 8];
    for (x, plane) in cells.iter_mut().enumerate() {
        for (y, row) in plane.iter_mut().enumerate() {
            for (z, cell) in row.iter_mut().enumerate() {
                let inside = |i: usize, k: usize| {
                    let i = i as i32;
                    i >= min[k] && i < min[k] + size[k]
                };
                *cell = inside(x, 0) && inside(y, 1) && inside(z, 2);
            }
        }
    }
    (solid, cells)
}

/// A random [`grid_box`] inside the grid.
fn random_grid_box(rng: &mut crate::test_rng::Rng) -> (Solid, Cells) {
    let (min, size) = random_grid_corner(rng);
    grid_box(min, size)
}

/// The corner and sizes of a [`random_grid_box`], without building it.
fn random_grid_corner(rng: &mut crate::test_rng::Rng) -> ([i32; 3], [i32; 3]) {
    let min: [i32; 3] = std::array::from_fn(|_| (rng.unit() * 6.0) as i32);
    let size = min.map(|m| (1 + (rng.unit() * f64::from(7 - m)) as i32).min(8 - m));
    (min, size)
}

/// Cells of the half grid, of 0.5 each way.
type Cells = [[[bool; 8]; 8]; 8];

fn combine(a: &Cells, b: &Cells, op: Op) -> Cells {
    let mut out = *a;
    for (x, plane) in out.iter_mut().enumerate() {
        for (y, row) in plane.iter_mut().enumerate() {
            for (z, cell) in row.iter_mut().enumerate() {
                let q = b[x][y][z];
                *cell = match op {
                    Op::Union => *cell || q,
                    Op::Intersection => *cell && q,
                    Op::Difference => *cell && !q,
                };
            }
        }
    }
    out
}

fn cells_volume(c: &Cells) -> f64 {
    c.iter().flatten().flatten().filter(|&&c| c).count() as f64 / 8.0
}

/// Whether the cells make a manifold: round every grid vertex, the full
/// cells and the empty ones of its eight are each joined through faces.
fn cells_manifold(c: &Cells) -> bool {
    let cell = |x: i32, y: i32, z: i32| {
        let ok = |i: i32| (0..8).contains(&i);
        ok(x) && ok(y) && ok(z) && c[x as usize][y as usize][z as usize]
    };
    for x in 0..=8 {
        for y in 0..=8 {
            for z in 0..=8 {
                let full: [bool; 8] = std::array::from_fn(|i| {
                    cell(
                        x - 1 + (i & 1) as i32,
                        y - 1 + (i >> 1 & 1) as i32,
                        z - 1 + (i >> 2) as i32,
                    )
                });
                for want in [true, false] {
                    let Some(first) = (0..8).find(|&i| full[i] == want) else {
                        continue;
                    };
                    let mut seen = vec![first];
                    let mut k = 0;
                    while k < seen.len() {
                        for bit in 0..3 {
                            let d = seen[k] ^ (1 << bit);
                            if full[d] == want && !seen.contains(&d) {
                                seen.push(d);
                            }
                        }
                        k += 1;
                    }
                    if seen.len() != full.iter().filter(|&&f| f == want).count() {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Runs `a op b` against the cells it should fill: a manifold result
/// must come out with their volume, and one that isn't may only fail
/// as invalid.
fn against_cells(name: &str, a: &Solid, b: &Solid, op: Op, want: &Cells) -> Option<Solid> {
    let volume = cells_volume(want);
    match run(a, b, op) {
        Ok(s) => {
            assert!(
                (s.volume() - volume).abs() < 1e-9,
                "{name} {op:?}: volume {} not {volume}",
                s.volume()
            );
            Some(s)
        }
        Err(KernelError::Invalid(_)) if !cells_manifold(want) => None,
        Err(e) => panic!("{name} {op:?}: {e}"),
    }
}

#[test]
fn grid_boxes_flush_and_touching() {
    // A face touching the domain side of a cut face (a notch at a face's
    // edge), and a crossing landing a rounding past a cut face's side.
    for (a, b) in [
        (([2, 2, 1], [4, 5, 6]), ([1, 5, 4], [1, 2, 1])),
        (([2, 1, 5], [1, 5, 2]), ([0, 2, 1], [3, 5, 5])),
    ] {
        let ((sa, ca), (sb, cb)) = (grid_box(a.0, a.1), grid_box(b.0, b.1));
        for op in [Op::Union, Op::Intersection, Op::Difference] {
            against_cells("pair", &sa, &sb, op, &combine(&ca, &cb, op));
            against_cells("pair", &sb, &sa, op, &combine(&cb, &ca, op));
        }
    }
    // Random boxes on the half grid: flush faces, shared edges and
    // corners everywhere.
    let mut rng = crate::test_rng::Rng::new(1);
    for i in 0..40 {
        let ((sa, ca), (sb, cb)) = (random_grid_box(&mut rng), random_grid_box(&mut rng));
        for op in [Op::Union, Op::Intersection, Op::Difference] {
            against_cells(&format!("{i}"), &sa, &sb, op, &combine(&ca, &cb, op));
        }
    }
}

#[test]
fn grid_boxes_chained() {
    // Results fed on: crossings on faces square to an axis stay exactly
    // on them, and a vertex where faces facing opposite ways meet (so the
    // perturbation can't move every face outwards) leaves a folded sheet
    // the clean-up takes out.
    let steps = [
        (([2, 5, 3], [5, 2, 2]), Op::Union),
        (([0, 4, 0], [2, 1, 7]), Op::Union),
        (([5, 2, 4], [1, 3, 3]), Op::Union),
        (([1, 5, 2], [5, 1, 3]), Op::Intersection),
    ];
    let (mut s, mut c) = grid_box([1, 1, 1], [4, 4, 4]);
    for (i, ((min, size), op)) in steps.into_iter().enumerate() {
        let (b, cb) = grid_box(min, size);
        c = combine(&c, &cb, op);
        s = against_cells(&format!("step {i}"), &s, &b, op, &c).expect("a manifold");
    }
    let mut rng = crate::test_rng::Rng::new(2);
    let ops = [Op::Union, Op::Intersection, Op::Difference];
    for i in 0..12 {
        let (mut s, mut c) = random_grid_box(&mut rng);
        for step in 0..5 {
            let (b, cb) = random_grid_box(&mut rng);
            let op = ops[(rng.unit() * 3.0) as usize];
            let want = combine(&c, &cb, op);
            if let Some(r) = against_cells(&format!("{i}/{step}"), &s, &b, op, &want) {
                (s, c) = (r, want);
            }
        }
    }
}

/// The tetrahedron on four points, facing out.
fn tetrahedron(p: [DVec3; 4]) -> Solid {
    let n = (p[1] - p[0]).cross(p[2] - p[0]);
    let tris = if n.dot(p[3] - p[0]) < 0.0 {
        [[0, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]]
    } else {
        [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]]
    };
    polytope(&p, &tris)
}

/// Pairs of tetrahedra sharing a face on a plane askew to the axes, its
/// corners the same numbers in both: the second one on the far side of
/// it, or inside the first.
fn shared_faces(count: usize) -> Vec<(Solid, Solid, bool)> {
    let mut rng = crate::test_rng::Rng::new(9);
    let mut out = Vec::new();
    while out.len() < count {
        let p: [DVec3; 3] = std::array::from_fn(|_| rng.point(2.0));
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        if n.length() < 0.5 {
            continue;
        }
        let c = (p[0] + p[1] + p[2]) / 3.0;
        let top = c + n.normalize() * rng.range(0.5, 2.0) + rng.point(0.3);
        let inside = out.len() % 2 == 0;
        let apex = if inside {
            let w: [f64; 4] =
                std::array::from_fn(|i| rng.range(if i == 3 { 0.2 } else { 0.1 }, 1.0));
            (p[0] * w[0] + p[1] * w[1] + p[2] * w[2] + top * w[3]) / w.iter().sum::<f64>()
        } else {
            c - n.normalize() * rng.range(0.5, 2.0) + rng.point(0.3)
        };
        out.push((
            tetrahedron([p[0], p[1], p[2], top]),
            tetrahedron([p[0], p[1], p[2], apex]),
            inside,
        ));
    }
    out
}

#[test]
fn tetrahedra_sharing_a_face_askew() {
    // The same corners in both, so the faces are flush exactly, but
    // their normals round: the perturbation must move every face of a
    // thin corner outwards (the normals' sum doesn't always), or flush
    // faces leave slivers.
    for (i, (a, b, inside)) in shared_faces(24).into_iter().enumerate() {
        let (va, vb) = (a.volume(), b.volume());
        let want = if inside {
            [va, vb, va - vb, 0.0]
        } else {
            [va + vb, 0.0, va, vb]
        };
        for (k, (x, y, op)) in [
            (&a, &b, Op::Union),
            (&a, &b, Op::Intersection),
            (&a, &b, Op::Difference),
            (&b, &a, Op::Difference),
        ]
        .into_iter()
        .enumerate()
        {
            let got = run(x, y, op).unwrap_or_else(|e| panic!("{i} {op:?} {k}: {e}"));
            assert!(
                (got.volume() - want[k]).abs() < 1e-9,
                "{i} {op:?} {k}: {} not {}",
                got.volume(),
                want[k]
            );
        }
    }
}

#[test]
fn crossings_are_where_the_perturbed_edges_cross() {
    // An edge in the other's face plane crosses it (for the perturbed
    // operands) where the first powers of the perturbation put it, not
    // where rounding in the plane's normal does.
    for (a, b, _) in shared_faces(24) {
        for op in [Op::Union, Op::Difference] {
            let mut work = Work::new(&Budget::DEFAULT);
            let (ia, ib) = (Input::new(a.mesh(), &TOL), Input::new(b.mesh(), &TOL));
            let grow = op == Op::Union;
            let prims = flat::Flat::tied(&ia, &ib, grow, 0.0);
            let counts = count::count(&ia, &ib, &prims, &TOL, &mut work).unwrap();
            let s = if grow { 1.0 } else { -1.0 };
            let normals = ia.vertex_normals();
            // Where edge `e` of `side` crosses face `f` of the other, with
            // `A` moved by `eps`, in floating point.
            let at = |side: Side, e: u32, f: u32, eps: f64| {
                let moved = |side: Side, v: u32| {
                    let input = if side == Side::A { &ia } else { &ib };
                    let p = input.pos(v);
                    if side == Side::A {
                        p + normals[v as usize] * (s * eps)
                            + exact::T2 * eps * eps
                            + exact::T3 * eps * eps * eps
                    } else {
                        p
                    }
                };
                let (input, other) = if side == Side::A {
                    (&ia, &ib)
                } else {
                    (&ib, &ia)
                };
                let [x0, x1] = input.edges[e as usize].map(|v| moved(side, v));
                let [t0, t1, t2] = other.tris[f as usize].map(|v| moved(side.other(), v));
                let n = (t1 - t0).cross(t2 - t0);
                (t0 - x0).dot(n) / (x1 - x0).dot(n)
            };
            for (side, crossings) in [(Side::A, &counts.x12), (Side::B, &counts.x21)] {
                for c in crossings {
                    let got = prims.crossing(side, c.edge, c.face);
                    let want = at(side, c.edge, c.face, 1e-7).clamp(0.0, 1.0);
                    assert!(
                        (got - want).abs() < 1e-4,
                        "{side:?} {c:?}: {got} not {want}"
                    );
                }
            }
        }
    }
}

/// A random turn and move, as the turned grid boxes take them.
fn random_turn(rng: &mut crate::test_rng::Rng) -> (glam::DQuat, DVec3) {
    let q = glam::DQuat::from_axis_angle(rng.direction(), rng.range(0.0, 6.0));
    (q, rng.point(100.0))
}

/// `s` turned by `q` and moved by `shift`, its faces claiming no surface
/// (turned plane tags would be off by rounding).
fn turned(s: &Solid, (q, shift): (glam::DQuat, DVec3)) -> Solid {
    let mut builder = MeshBuilder::new();
    for &p in s.mesh().verts() {
        builder.vert(q * p + shift);
    }
    for &f in s.mesh().faces() {
        builder.face(Face {
            surface: Surface::Free,
            ..f
        });
    }
    for t in s.mesh().tris() {
        builder.tri(t.halfedges.map(|h| h.start), t.face);
    }
    Solid::new(builder.build().unwrap(), &TOL).unwrap()
}

/// Turned grid boxes `a op b`: the cells' volume, or refused. Gives
/// the result, checked to have the cells' volume if it is one.
fn turned_against_cells(
    name: &str,
    a: &Solid,
    b: &Solid,
    op: Op,
    want: &Cells,
) -> Result<Solid, KernelError> {
    let r = run(a, b, op);
    if let Ok(s) = &r {
        let want = cells_volume(want);
        assert!(
            (s.volume() - want).abs() < 1e-6,
            "{name} {op:?}: {} not {want}",
            s.volume()
        );
    }
    r
}

#[test]
fn turned_grid_boxes_are_right_or_refused() {
    // Flush boxes turned and moved together: every coordinate rounded, so
    // flush faces are near ties rather than ties. A few fail, but
    // whatever comes out must have the right volume: a crossing of an
    // edge nearly in a face's plane is placed from the exact ratio, not
    // from two tiny rounded numbers (which put vertices off the result).
    let mut rng = crate::test_rng::Rng::new(31);
    let mut right = 0;
    for i in 0..=231 {
        let (ga, gb) = (random_grid_corner(&mut rng), random_grid_corner(&mut rng));
        let turn = random_turn(&mut rng);
        if i < 200 {
            continue;
        }
        let ((a, ca), (b, cb)) = (grid_box(ga.0, ga.1), grid_box(gb.0, gb.1));
        let (a, b) = (turned(&a, turn), turned(&b, turn));
        for op in [Op::Union, Op::Intersection, Op::Difference] {
            match turned_against_cells(&format!("{i}"), &a, &b, op, &combine(&ca, &cb, op)) {
                Ok(_) => right += 1,
                Err(KernelError::Invalid(_) | KernelError::Boolean(_)) => {}
                Err(e) => panic!("{i} {op:?}: {e}"),
            }
        }
    }
    // Near ties within the tie distance are decided as the ties they
    // stand for, at every order: 95 of the 96 work (94 with only the
    // constant term tied, 71 with exact signs).
    assert!(right >= 95, "{right}");
}

#[test]
fn turned_grid_boxes_tied_at_higher_orders() {
    // Pairs of the turned grid boxes above whose collinear edges (a
    // `Height` with a zero first order) or vertices moving along a face
    // (a `Reach`) were decided by the rounding in their first order, so
    // the decisions fit no configuration: `Inconsistent`. Decided at
    // every order as the exact ties, all of them come out right.
    let mut rng = crate::test_rng::Rng::new(31);
    for i in 0..=594 {
        let (ga, gb) = (random_grid_corner(&mut rng), random_grid_corner(&mut rng));
        let turn = random_turn(&mut rng);
        if ![117, 130, 239, 452, 509, 561, 573, 594].contains(&i) {
            continue;
        }
        let ((a, ca), (b, cb)) = (grid_box(ga.0, ga.1), grid_box(gb.0, gb.1));
        let (a, b) = (turned(&a, turn), turned(&b, turn));
        for op in [Op::Union, Op::Intersection, Op::Difference] {
            if let Err(e) =
                turned_against_cells(&format!("{i}"), &a, &b, op, &combine(&ca, &cb, op))
            {
                panic!("{i} {op:?}: {e}");
            }
        }
    }
}

#[test]
fn turned_grid_boxes_chained_are_never_inconsistent() {
    // Turned grid boxes, each result fed on: crossings are snapped only
    // onto faces square to an axis, so chained flush faces stay near
    // ties, often with a zero first order at the exact tie. 12 of 356
    // steps were `Inconsistent` with only the constant term tied; now
    // none are, and 349 of 359 are right (the rest `Invalid`).
    let mut rng = crate::test_rng::Rng::new(77);
    let ops = [Op::Union, Op::Intersection, Op::Difference];
    let chains = if cfg!(debug_assertions) { 5 } else { 100 };
    let (mut right, mut all) = (0, 0);
    for i in 0..chains {
        let turn = random_turn(&mut rng);
        let (s, mut c) = random_grid_box(&mut rng);
        let mut s = turned(&s, turn);
        for step in 0..6 {
            let (b, cb) = random_grid_box(&mut rng);
            let b = turned(&b, turn);
            let op = ops[(rng.unit() * 3.0) as usize];
            let want = combine(&c, &cb, op);
            all += 1;
            match turned_against_cells(&format!("{i}/{step}"), &s, &b, op, &want) {
                Ok(r) => {
                    right += 1;
                    if r.is_empty() {
                        break;
                    }
                    (s, c) = (r, want);
                }
                Err(KernelError::Invalid(_)) => {}
                Err(e) => panic!("{i}/{step} {op:?}: {e}"),
            }
        }
    }
    if chains == 100 {
        assert!(right >= 349, "{right} of {all}");
    }
}
