#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use glam::DVec3;

use super::*;
use crate::Stripped;
use crate::mesh::tests::{OCTAHEDRON, TOL, UNIT};
use crate::mesh::{CheckError, Face, FaceName, FacePart, Form, Mesh, MeshBuilder, Surface};
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
            name: FaceName::new(2, FacePart::Split(i as u32)),
            surface: Surface::Plane { n, d: n.dot(p) },
            form: Form::plane(n, n.dot(p)),
            slack: 1.0,
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
        let form = face.form.moved(&f);
        builder.face(Face {
            form: if inverted { form.flipped() } else { form },
            ..face
        });
    }
    for t in mesh.tris() {
        let [a, b, c] = t.halfedges.map(|h| h.start);
        builder.tri(if inverted { [a, c, b] } else { [a, b, c] }, t.face);
    }
    builder.build().unwrap()
}

fn run(a: &Solid, b: &Solid, op: Op) -> Result<Solid, KernelError> {
    boolean(a, b, op, &TOL, &Budget::DEFAULT).stripped()
}

/// The volumes of `a ∪ b`, `a ∩ b`, `a − b` and `b − a` from those of
/// `a`, `b` and their intersection.
fn volumes(va: f64, vb: f64, both: f64) -> [f64; 4] {
    [va + vb - both, both, va - both, vb - both]
}

/// Runs the four operations both ways round (union and intersection
/// swapped give the same volume), checking each result's volume, or
/// that it fails as not a manifold where `expect` has `None`.
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
            (None, Err(KernelError::Boolean(BooleanError::NotManifold))) => {}
            (want, got) => panic!(
                "{name}: {op:?} (swapped {swapped}) gave {:?}, wanted {want:?}",
                got.map(|s| s.volume())
            ),
        }
    }
}

/// Checks that each plane face's normal points the way its triangles
/// face at their middles. `check` makes the same test (and that every
/// patch is on its face's surface), so this only names the patch.
fn faces_face_out(solid: &Solid) {
    let mesh = solid.mesh();
    for (t, tri) in mesh.tris().iter().enumerate() {
        if let Surface::Plane { n, .. } = mesh.faces()[tri.face as usize].surface {
            let normal = mesh.patch(t).normal(DVec3::splat(1.0 / 3.0));
            assert!(normal.dot(n) > 0.0, "patch {t}");
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
fn pinched_finds_two_vertices_within_the_distance() {
    let d = 1e-4;
    let mut work = Work::new(&Budget::DEFAULT);
    let far = [DVec3::ZERO, DVec3::X, DVec3::new(2.0 * d, 0.0, 0.0)];
    assert_eq!(pinched(&far, d, &mut work), Ok(None));
    // Across a cell's side, and diagonally across a corner of cells: the
    // pair, the earlier first.
    let near = [DVec3::ZERO, DVec3::X, DVec3::new(0.0, 0.0, -0.9 * d)];
    assert_eq!(pinched(&near, d, &mut work), Ok(Some([near[0], near[2]])));
    let corner = [DVec3::splat(-0.3 * d), DVec3::X, DVec3::splat(0.2 * d)];
    assert_eq!(
        pinched(&corner, d, &mut work),
        Ok(Some([corner[0], corner[2]]))
    );
    // Far out, where keys are large, and non-finite positions.
    let out = [DVec3::splat(1e6), DVec3::splat(1e6) + DVec3::Y * (0.5 * d)];
    assert_eq!(pinched(&out, d, &mut work), Ok(Some(out)));
    let nan = [DVec3::NAN, DVec3::NAN, DVec3::INFINITY, DVec3::INFINITY];
    assert_eq!(pinched(&nan, d, &mut work), Ok(None));
    // A pile of vertices on one point stops at the first pair.
    let pile = vec![DVec3::ONE; 100_000];
    let mut work = Work::new(&Budget::new(10));
    assert_eq!(pinched(&pile, d, &mut work), Ok(Some([DVec3::ONE; 2])));
    // A grid of vertices just over the distance apart costs about 27
    // units a vertex, and stops on the budget.
    let grid: Vec<DVec3> = (0..64_000)
        .map(|i| DVec3::new((i % 40) as f64, (i / 40 % 40) as f64, (i / 1600) as f64) * 1.01 * d)
        .collect();
    assert_eq!(
        pinched(&grid, d, &mut Work::new(&Budget::DEFAULT)),
        Ok(None)
    );
    let mut work = Work::new(&Budget::new(100_000));
    assert_eq!(pinched(&grid, d, &mut work), Err(KernelError::TooComplex));
}

#[test]
fn apart_tells_triangles_of_separate_shells() {
    let shells = crate::mesh::tests::joined(&[
        (cube([0.0; 3], [1.0; 3]).mesh(), false),
        (cube([3.0; 3], [1.0; 3]).mesh(), false),
    ]);
    let mut work = Work::new(&Budget::DEFAULT);
    let half = shells.tris().len() as u32 / 2;
    assert_eq!(apart(&shells, 0, half - 1, &mut work), Ok(false));
    assert_eq!(apart(&shells, 0, half, &mut work), Ok(true));
    // Out of range: not told apart.
    assert_eq!(apart(&shells, 0, 2 * half, &mut work), Ok(false));
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
    let apart = |b: &Solid| touches(&a, b, &tol, &Budget::new(0)).stripped();
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

/// What regen relies on to fail a feature that would empty a body: a
/// tool that takes the body whole, flush on its top and bottom, or one
/// that only touches it (flush on a face, along an edge, at a corner),
/// touches it, and the difference or intersection is the empty solid, not
/// an error.
#[test]
fn a_body_taken_whole_or_met_flush_gives_the_empty_solid() {
    let body = cube([0.0; 3], [2.0; 3]);
    let tol = TOL;
    let whole = cube([-1.0, -1.0, 0.0], [4.0, 4.0, 2.0]);
    assert!(touches(&body, &whole, &tol, &Budget::DEFAULT).unwrap());
    assert!(run(&body, &whole, Op::Difference).unwrap().is_empty());
    for only in [
        cube([0.5, 0.5, -1.0], [1.0, 1.0, 1.0]),
        cube([2.0, 2.0, 0.0], [2.0; 3]),
        cube([2.0; 3], [2.0; 3]),
    ] {
        assert!(touches(&body, &only, &tol, &Budget::DEFAULT).unwrap());
        assert!(run(&body, &only, Op::Intersection).unwrap().is_empty());
    }
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
        )
        .stripped(),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn a_void_thinner_than_the_resolution_fails_at_once() {
    // A blind void in a cylinder, its wall half a resolution thick: the
    // result's round walls are within the resolution of each other over
    // an area. Repair used to split them until the whole budget was gone
    // (`TooComplex`, about a second); points of the two found within the
    // resolution now refuse it at once, well inside a small budget. The
    // hull failure is between the void's shell and the outer one: a
    // result closer to itself than the resolution, named so.
    let half = 0.5 * TOL.resolution();
    for r in [5.0, 1.0] {
        let a = Solid::cylinder(DVec3::ZERO, r, 10.0, 1, &TOL).unwrap();
        let b = Solid::cylinder(DVec3::Z * 2.0, r - half, 6.0, 2, &TOL).unwrap();
        let result = boolean(&a, &b, Op::Difference, &TOL, &Budget::new(100_000)).stripped();
        assert_eq!(
            result.map(|_| ()),
            Err(KernelError::Boolean(BooleanError::NotManifold)),
            "{r}"
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
                Err(KernelError::Invalid(_) | KernelError::Boolean(BooleanError::NotManifold))
                    if both == 0.0 && op == Op::Union => {}
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
/// must come out with their volume, and one that isn't may only fail,
/// as not a manifold or, where it has no two vertices that near, as
/// invalid.
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
        Err(KernelError::Boolean(BooleanError::NotManifold) | KernelError::Invalid(_))
            if !cells_manifold(want) =>
        {
            None
        }
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

#[test]
fn grid_boxes_folded_sheets() {
    // Chains of grid boxes (from seeded runs like the one above) whose
    // last step leaves a sheet of zero thickness folded onto a flush
    // face, its two sides triangulated differently: a vertex whose star
    // lies in one plane, or on a crease, with triangles in a plane facing
    // both ways. No two of its triangles are the same, so no short
    // collapse cancels them, and the result failed the check (`Fold`,
    // `EdgeNeighbours`, `VertexNeighbours`). Moving the vertex within
    // its star's planes takes the sheet out. The first three failed this
    // way before other fixes mended them; they stay as cases of the
    // kind.
    use Op::{Difference as D, Intersection as I, Union as U};
    type Step = (Op, [i32; 3], [i32; 3]);
    type Chain = (([i32; 3], [i32; 3]), &'static [Step]);
    let chains: [Chain; 9] = [
        (
            ([4, 0, 4], [3, 6, 1]),
            &[(U, [0, 2, 3], [4, 3, 2]), (D, [0, 2, 3], [4, 1, 3])],
        ),
        (
            ([1, 3, 5], [3, 3, 1]),
            &[
                (I, [2, 0, 2], [5, 5, 4]),
                (I, [0, 2, 3], [4, 3, 4]),
                (U, [3, 3, 3], [2, 4, 2]),
                (I, [2, 4, 5], [2, 3, 1]),
            ],
        ),
        (
            ([3, 0, 5], [4, 3, 1]),
            &[
                (I, [3, 2, 5], [2, 3, 2]),
                (I, [1, 3, 2], [6, 2, 3]),
                (U, [1, 1, 3], [6, 1, 1]),
                (U, [1, 2, 3], [4, 4, 3]),
                (U, [2, 0, 2], [5, 2, 4]),
            ],
        ),
        (
            ([5, 2, 2], [1, 2, 4]),
            &[
                (D, [3, 5, 1], [4, 1, 6]),
                (U, [2, 4, 3], [4, 2, 3]),
                (U, [2, 1, 5], [3, 2, 2]),
                (U, [1, 0, 2], [2, 7, 1]),
                (D, [1, 0, 3], [3, 5, 2]),
            ],
        ),
        (
            ([1, 4, 4], [5, 2, 2]),
            &[
                (U, [0, 3, 3], [5, 2, 1]),
                (D, [4, 2, 2], [1, 3, 5]),
                (U, [1, 5, 3], [6, 1, 2]),
            ],
        ),
        (
            ([2, 4, 2], [4, 2, 4]),
            &[(U, [0, 1, 0], [2, 4, 3]), (D, [2, 2, 1], [4, 5, 4])],
        ),
        (
            ([4, 5, 1], [3, 1, 6]),
            &[(U, [0, 1, 4], [6, 4, 1]), (D, [3, 5, 5], [4, 2, 2])],
        ),
        (
            ([5, 2, 3], [2, 4, 4]),
            &[(U, [3, 1, 1], [2, 3, 6]), (U, [2, 3, 3], [3, 4, 2])],
        ),
        (
            ([2, 3, 4], [1, 2, 1]),
            &[
                (D, [4, 0, 4], [3, 1, 1]),
                (U, [0, 2, 3], [2, 2, 2]),
                (U, [0, 3, 0], [2, 2, 7]),
            ],
        ),
    ];
    for (k, (start, steps)) in chains.into_iter().enumerate() {
        let (mut s, mut c) = grid_box(start.0, start.1);
        for (i, &(op, min, size)) in steps.iter().enumerate() {
            let (b, cb) = grid_box(min, size);
            c = combine(&c, &cb, op);
            s = against_cells(&format!("chain {k} step {i}"), &s, &b, op, &c).expect("a manifold");
            faces_face_out(&s);
        }
    }
}

#[test]
fn grid_boxes_folded_sheets_on_turned_frames() {
    // Chains of grid boxes extruded on one turned and moved frame (from
    // a seeded run), so their faces keep their plane tags but are flush
    // only to rounding: each last step left a sheet folded onto a flush
    // face (`Fold`, or the hull check) until the clean-up unfolded it,
    // turning triangles over onto faces of their way, a hair off the
    // star's plane. The turned grid boxes of the tests above claim no
    // surfaces, which the rule leaves alone.
    use crate::profile::tests::rect;
    use crate::{Frame, Profile, extrude};
    use Op::{Difference as D, Intersection as I, Union as U};
    type Step = (Op, [i32; 3], [i32; 3]);
    type Chain = ([f64; 4], [f64; 3], ([i32; 3], [i32; 3]), &'static [Step]);
    let chains: [Chain; 5] = [
        (
            [
                0.5023521821698758,
                -0.1780177652138604,
                -0.06758011658996597,
                0.8434363569227458,
            ],
            [
                -23.084401332986687,
                32.466619415219384,
                -0.16877942517028544,
            ],
            ([3, 0, 5], [1, 7, 2]),
            &[(U, [2, 2, 4], [5, 3, 1]), (D, [4, 1, 1], [1, 3, 4])],
        ),
        (
            [
                -0.41213562876180715,
                0.22182897020007855,
                0.06923044229435728,
                0.8809899416819753,
            ],
            [-63.004230059066366, -94.38223617030962, 76.1774928746695],
            ([0, 4, 3], [7, 2, 4]),
            &[(U, [3, 2, 3], [4, 4, 3]), (U, [5, 4, 0], [2, 2, 3])],
        ),
        (
            [
                0.5108323374739001,
                0.3300633684368514,
                -0.17828588009453525,
                -0.7735131807319043,
            ],
            [-46.65199374376567, 12.548537585258131, -52.433518810422505],
            ([3, 4, 2], [1, 1, 3]),
            &[(U, [4, 3, 1], [3, 2, 3]), (I, [4, 2, 3], [1, 5, 3])],
        ),
        (
            [
                0.25370145257231175,
                0.16921646175829835,
                -0.11027560823277095,
                0.9459601747756582,
            ],
            [-51.33443578067143, -54.98174984730111, 60.233360154727336],
            ([1, 5, 5], [3, 2, 1]),
            &[
                (U, [1, 1, 0], [5, 4, 6]),
                (U, [1, 5, 2], [3, 2, 4]),
                (U, [0, 4, 5], [3, 1, 2]),
            ],
        ),
        (
            [
                0.3055451280551021,
                0.6287398789296482,
                -0.23645682453364247,
                0.6748455449188271,
            ],
            [98.86112526908661, 9.914054616353269, 6.898593799041095],
            ([4, 4, 5], [2, 2, 2]),
            &[(U, [4, 3, 2], [1, 1, 4]), (I, [2, 4, 1], [3, 2, 6])],
        ),
    ];
    let at = |i: i32| -0.5 + 0.5 * f64::from(i);
    for (k, (q, shift, start, steps)) in chains.into_iter().enumerate() {
        let q = glam::DQuat::from_array(q);
        let frame = Frame {
            origin: DVec3::from_array(shift),
            x: (q * DVec3::X).normalize(),
            y: (q * DVec3::Y).normalize(),
        };
        let framed = |(min, size): ([i32; 3], [i32; 3]), feature| {
            let lo = glam::DVec2::new(at(min[0]), at(min[1]));
            let hi = glam::DVec2::new(at(min[0] + size[0]), at(min[1] + size[1]));
            let profile = Profile {
                loops: vec![rect(lo, hi, 0)],
            };
            let solid = extrude(
                &profile,
                &frame,
                at(min[2]),
                at(min[2] + size[2]),
                feature,
                &TOL,
                &Budget::DEFAULT,
            )
            .unwrap();
            (solid, grid_box(min, size).1)
        };
        let (mut s, mut c) = framed(start, 1);
        for (i, &(op, min, size)) in steps.iter().enumerate() {
            let (b, cb) = framed((min, size), 2 + i as u64);
            c = combine(&c, &cb, op);
            s = turned_against_cells(&format!("chain {k} step {i}"), &s, &b, op, &c)
                .unwrap_or_else(|e| panic!("chain {k} step {i}: {e}"));
            faces_face_out(&s);
        }
    }
}

/// The grid box at `min` of `size` (half units, as [`grid_box`]),
/// extruded as `feature` on the frame of origin, `x` and `y` `frame`,
/// with its cells in the grid's own frame.
fn framed_grid_box(
    min: [i32; 3],
    size: [i32; 3],
    frame: [[f64; 3]; 3],
    feature: u64,
) -> (Solid, Cells) {
    use crate::profile::tests::rect;
    use crate::{Frame, Profile, extrude};
    let at = |i: i32| -0.5 + 0.5 * f64::from(i);
    let frame = Frame {
        origin: DVec3::from_array(frame[0]),
        x: DVec3::from_array(frame[1]),
        y: DVec3::from_array(frame[2]),
    };
    let lo = glam::DVec2::new(at(min[0]), at(min[1]));
    let hi = glam::DVec2::new(at(min[0] + size[0]), at(min[1] + size[1]));
    let profile = Profile {
        loops: vec![rect(lo, hi, 0)],
    };
    let solid = extrude(
        &profile,
        &frame,
        at(min[2]),
        at(min[2] + size[2]),
        feature,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    (solid, grid_box(min, size).1)
}

#[test]
fn grid_boxes_a_hair_off_each_other_keep_their_result_when_unfolding_fails() {
    // Grid boxes each extruded on its own frame, turned and moved alike
    // but for a hair (about 1e-9 apart), less one and then another. The
    // second leaves a triangle facing against its plane face on each side
    // of a vertex; unfolding the vertex takes one out and leaves the
    // other where the Delaunay flips no longer mend it, which the check
    // refused (`VertexNeighbours`). The clean-up without the rule gives
    // the result.
    let framed = framed_grid_box;
    let (a, ca) = framed(
        [3, 1, 2],
        [1, 5, 3],
        [
            [-77.83999764203944, -39.74511370412731, -25.954402152041855],
            [0.5048189834550261, -0.8611838342182273, 0.05933125335459444],
            [0.6550176505623232, 0.4269187505317222, 0.6234518889988606],
        ],
        1,
    );
    let (b, cb) = framed(
        [4, 4, 1],
        [2, 1, 5],
        [
            [-77.83999764173862, -39.74511370319701, -25.954402149762235],
            [0.5048189834352008, -0.8611838342012809, 0.05933125376925241],
            [0.6550176504534279, 0.42691875075858077, 0.6234518889579244],
        ],
        2,
    );
    let (c, cc) = framed(
        [2, 2, 4],
        [2, 1, 1],
        [
            [-77.83999764944019, -39.7451137082906, -25.954402156326395],
            [0.5048189835852241, -0.8611838341317449, 0.05933125350208318],
            [0.6550176504170823, 0.42691875069793017, 0.6234518890376416],
        ],
        3,
    );
    let first = combine(&ca, &cb, Op::Difference);
    let ab = run(&a, &b, Op::Difference).unwrap();
    assert!((ab.volume() - cells_volume(&first)).abs() < 1e-5);
    let want = cells_volume(&combine(&first, &cc, Op::Difference));
    let abc = run(&ab, &c, Op::Difference).unwrap();
    assert!(
        (abc.volume() - want).abs() < 1e-5,
        "{} not {want}",
        abc.volume()
    );
    faces_face_out(&abc);
}

#[test]
fn grid_boxes_a_hair_off_each_other_keep_their_faces() {
    // Grid boxes each extruded on its own frame, a hair apart, with flush
    // sides. An edge of one lying in the other's flush plane within the
    // tie (decided as in it) met that plane, as rounding had it, beyond
    // the triangle it crossed, so the crossing sat half a unit off the
    // face: the loop through it wound the wrong way, and the tool's side
    // was left filling the target's flush face, facing against its own
    // plane tag (debug builds' form check panicked; release passed).
    // Such crossings are now kept inside the triangle, and where no part
    // of the edge is, the operation is decided again exactly; `check`
    // refuses a plane patch facing against its tag.
    let framed = framed_grid_box;
    let (a, ca) = framed(
        [0, 2, 0],
        [6, 2, 3],
        [
            [44.36970915246301, -9.081799137530764, -6.457953029287201],
            [0.7454606892761992, -0.6157373509965998, 0.2552564893034927],
            [
                -0.5520700001061614,
                -0.7849507655031489,
                -0.2811956804767876,
            ],
        ],
        1,
    );
    let (b, cb) = framed(
        [3, 3, 0],
        [2, 4, 3],
        [
            [44.36970915321567, -9.081799138079475, -6.457953030961496],
            [0.745460688454698, -0.615737349980998, 0.25525649415250073],
            [
                -0.5520700026312176,
                -0.7849507672528597,
                -0.2811956706350831,
            ],
        ],
        2,
    );
    let (c, cc) = framed(
        [0, 3, 1],
        [1, 3, 5],
        [
            [44.36970915256131, -9.08179913750429, -6.45795302922593],
            [0.7454606883685, -0.6157373500903354, 0.2552564941404899],
            [
                -0.5520700026895156,
                -0.7849507671606006,
                -0.2811956707781665,
            ],
        ],
        3,
    );
    let first = combine(&ca, &cb, Op::Union);
    let ab = run(&a, &b, Op::Union).unwrap();
    assert!((ab.volume() - cells_volume(&first)).abs() < 1e-5);
    faces_face_out(&ab);
    let want = cells_volume(&combine(&first, &cc, Op::Difference));
    let abc = run(&ab, &c, Op::Difference).unwrap();
    assert!(
        (abc.volume() - want).abs() < 1e-5,
        "{} not {want}",
        abc.volume()
    );
    faces_face_out(&abc);

    // One difference whose in-plane edge misses the triangle it was
    // decided to cross: decided again exactly (it was `Invalid(Fold)`).
    let (a, ca) = framed(
        [0, 2, 4],
        [3, 2, 2],
        [
            [-29.443174343512496, 54.03706875218741, -74.92980968005651],
            [0.2964563634555907, -0.09998156461713209, 0.9497985635403946],
            [
                0.3965998278277893,
                0.9175886042653624,
                -0.027197975831156074,
            ],
        ],
        1,
    );
    let (b, cb) = framed(
        [2, 2, 3],
        [4, 5, 3],
        [
            [-29.443174343485115, 54.0370687528298, -74.92980968118371],
            [0.2964563636353201, -0.09998156415167433, 0.9497985635332934],
            [0.39659982785630565, 0.917588604237126, -0.02719797636795357],
        ],
        2,
    );
    let want = cells_volume(&combine(&ca, &cb, Op::Difference));
    let ab = run(&a, &b, Op::Difference).unwrap();
    assert!(
        (ab.volume() - want).abs() < 1e-5,
        "{} not {want}",
        ab.volume()
    );
    faces_face_out(&ab);
}

#[test]
fn vertices_an_ulp_inside_a_face_pair_their_triangles_with_it() {
    // Grid boxes at a hundredth of the size, moved off the origin, chained
    // as random chains have them: the first two intersections leave
    // vertices across the middle of the two-cell box, an ulp inside the
    // plane of the last box's side through it. Decided as on it (a tie),
    // and by the perturbation on its far side, those vertices' edges
    // crossed that side, but the triangles they belong to on the near
    // side had boxes an ulp short of it, and the exact broad phase
    // (margin 0) paired none of them with it: no crossing was counted,
    // the whole box was outside the other, and the intersection came out
    // empty and the difference whole, both `Ok`. Ties reach as far as the
    // resolution for the flat primitives as for the curved ones.
    use crate::{Frame, Profile, Segment, extrude};
    use glam::DVec2;
    let shift = DVec3::new(
        -0.04547329606056105,
        -0.3390715991830786,
        0.3179193587904965,
    );
    let frame = Frame {
        origin: shift,
        x: DVec3::X,
        y: DVec3::Y,
    };
    let small = |min: [i32; 3], size: [i32; 3], feature: u64| {
        let at = |i: i32| (-0.5 + 0.5 * f64::from(i)) * 0.01;
        let (lo, hi) = (
            DVec2::new(at(min[0]), at(min[1])),
            DVec2::new(at(min[0] + size[0]), at(min[1] + size[1])),
        );
        let p = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        let segments = (0..4)
            .map(|i| Segment::line(p[i], p[(i + 1) % 4], i as u64).unwrap())
            .collect();
        let profile = Profile {
            loops: vec![crate::Loop { segments }],
        };
        let solid = extrude(
            &profile,
            &frame,
            at(min[2]),
            at(min[2] + size[2]),
            feature,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
        (solid, grid_box(min, size).1)
    };
    let (a, ca) = small([2, 0, 0], [2, 3, 4], 1);
    let (b, cb) = small([2, 1, 2], [2, 5, 3], 2);
    let (c, cc) = small([2, 2, 3], [2, 2, 2], 3);
    let (d, cd) = small([2, 0, 1], [1, 6, 3], 4);
    let cells = combine(&ca, &cb, Op::Intersection);
    let ab = run(&a, &b, Op::Intersection).unwrap();
    let cells = combine(&cells, &cc, Op::Intersection);
    let abc = run(&ab, &c, Op::Intersection).unwrap();
    assert!((abc.volume() - cells_volume(&cells) * 1e-6).abs() < 1e-15);
    for op in [Op::Intersection, Op::Difference, Op::Union] {
        let want = cells_volume(&combine(&cells, &cd, op)) * 1e-6;
        let got = run(&abc, &d, op).unwrap();
        assert!(
            (got.volume() - want).abs() < 1e-15,
            "{op:?}: {} not {want}",
            got.volume()
        );
        faces_face_out(&got);
    }
}

#[test]
fn edges_with_one_end_in_a_flush_plane_cross_it_inside_their_triangle() {
    // A box and a prism on frames a hair apart, and a box touching the
    // first along a side from outside, its own side beside the first's
    // other side in one plane a hair apart. An edge of the first's side
    // with one end within the tie of the tool's side and the other a bit
    // further crossed it, as the counting had it, but as rounding placed
    // it, nearly parallel to the plane, half a unit outside the triangle
    // it crossed: the tool's side was left on the target's flush side,
    // facing against its tag (`Invalid(FacesAgainst)`, before that an
    // `Invalid(Face)` from the booleans' look at the forms). The tool
    // takes nothing away.
    use crate::profile::tests::polygon;
    use crate::{Frame, Profile, extrude};
    use glam::DVec2;
    let (a, _) = framed_grid_box(
        [4, 2, 2],
        [3, 1, 2],
        [
            [90.7976943597779, -68.56024080319376, 42.71813372607313],
            [
                0.9999990797439315,
                0.001063986363890537,
                0.0008416913376111664,
            ],
            [
                -0.0010645111850754786,
                0.9999992391377956,
                0.0006233295813199928,
            ],
        ],
        1,
    );
    let prism = Profile {
        loops: vec![polygon(
            &[
                DVec2::new(0.5, 2.75),
                DVec2::new(2.25, 2.25),
                DVec2::new(3.25, 3.25),
                DVec2::new(0.75, 3.25),
            ],
            0,
        )],
    };
    let frame = Frame {
        origin: DVec3::new(90.79769435856642, -68.56024080260039, 42.718133724806144),
        x: DVec3::new(
            0.9999990797426671,
            0.0010639872650841894,
            0.0008416917006606851,
        ),
        y: DVec3::new(
            -0.001064512086299837,
            0.9999992391369816,
            0.0006233293481978193,
        ),
    };
    let p = extrude(&prism, &frame, 1.5, 3.5, 2, &TOL, &Budget::DEFAULT).unwrap();
    let (c, _) = framed_grid_box(
        [4, 3, 1],
        [2, 2, 4],
        [
            [90.79769435863315, -68.56024080128681, 42.7181337244894],
            [
                0.9999990797503366,
                0.0010639639275988714,
                0.0008417120889127035,
            ],
            [
                -0.0010644887351945824,
                0.9999992391813621,
                0.00062329802649558,
            ],
        ],
        3,
    );
    let ap = run(&a, &p, Op::Union).unwrap();
    assert!((ap.volume() - 4.25).abs() < 1e-9, "{}", ap.volume());
    let cut = run(&ap, &c, Op::Difference).unwrap();
    assert!((cut.volume() - 4.25).abs() < 1e-5, "{}", cut.volume());
    faces_face_out(&cut);
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
                    let got = prims.crossing(side, c.edge, c.face).unwrap();
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
    moved_free(s, |p| q * p + shift)
}

/// `s` with its vertices moved by `f`, its faces claiming no surface.
fn moved_free(s: &Solid, f: impl Fn(DVec3) -> DVec3) -> Solid {
    let mut builder = MeshBuilder::new();
    for &p in s.mesh().verts() {
        builder.vert(f(p));
    }
    for &face in s.mesh().faces() {
        builder.face(Face {
            surface: Surface::Free,
            form: face.form.moved(&f),
            ..face
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
    // flush faces are near ties rather than ties. Whatever comes out must
    // have the right volume: a crossing of an edge nearly in a face's
    // plane is placed from the exact ratio, not from two tiny rounded
    // numbers (which put vertices off the result). Near ties decided as
    // the ties they stand for at every order, every result whose cells
    // are a manifold comes out right (of 900 pairs' 2 700 operations,
    // all but one, pair 866's union, a thin triangle across two faces
    // that fails the hull rules); the others fail as `Invalid`.
    let mut rng = crate::test_rng::Rng::new(31);
    let (first, last) = if cfg!(debug_assertions) {
        (200, 231)
    } else {
        (0, 149)
    };
    let (mut right, mut refused) = (0, 0);
    for i in 0..=last {
        let (ga, gb) = (random_grid_corner(&mut rng), random_grid_corner(&mut rng));
        let turn = random_turn(&mut rng);
        if i < first {
            continue;
        }
        let ((a, ca), (b, cb)) = (grid_box(ga.0, ga.1), grid_box(gb.0, gb.1));
        let (a, b) = (turned(&a, turn), turned(&b, turn));
        for op in [Op::Union, Op::Intersection, Op::Difference] {
            let want = combine(&ca, &cb, op);
            match turned_against_cells(&format!("{i}"), &a, &b, op, &want) {
                Ok(_) => right += 1,
                Err(e) if cells_manifold(&want) => panic!("{i} {op:?}: {e}"),
                Err(KernelError::Invalid(_) | KernelError::Boolean(BooleanError::NotManifold)) => {
                    refused += 1
                }
                Err(e) => panic!("{i} {op:?}: {e}"),
            }
        }
    }
    // 95 of the 96 in a debug build (71 with exact signs).
    assert!(
        right >= 3 * (last - first + 1) * 9 / 10,
        "{right}, {refused} refused"
    );
}

#[test]
fn turned_grid_boxes_whose_edge_shadows_round_parallel() {
    // Pair 100 of the turned grid boxes: two collinear edges whose
    // shadows come out exactly parallel in floating point, so `Height`'s
    // scale is 0. It fell back to exact signs, which took the constant
    // term's rounding as it came, and the intersection and difference
    // were `Inconsistent`. With the tie kept at a zero scale (only an
    // exact zero is a tie, and the later orders that are only rounding
    // are skipped), both are right.
    let mut rng = crate::test_rng::Rng::new(31);
    for i in 0..=100 {
        let (ga, gb) = (random_grid_corner(&mut rng), random_grid_corner(&mut rng));
        let turn = random_turn(&mut rng);
        if i < 100 {
            continue;
        }
        let ((a, ca), (b, cb)) = (grid_box(ga.0, ga.1), grid_box(gb.0, gb.1));
        let (a, b) = (turned(&a, turn), turned(&b, turn));
        for op in [Op::Intersection, Op::Difference] {
            if let Err(e) = turned_against_cells("100", &a, &b, op, &combine(&ca, &cb, op)) {
                panic!("{op:?}: {e}");
            }
        }
    }
}

#[test]
fn near_ties_that_dont_fit_together_are_decided_again_exactly() {
    // Turned grid boxes, one moved by about the tie distance: decided
    // with near ties as ties, these give decisions no one configuration
    // has (`Inconsistent`), and the boolean decides them again exactly,
    // from the same budget: right. Of 3 000 such operations (seed 5),
    // 60 are `Inconsistent`; with the retry 34 of those are right and
    // the rest `Invalid` (parts closer than the resolution), none
    // `Inconsistent`. (100 were before crossings of edges decided to lie
    // in a face's plane were kept inside the triangle; the cases once
    // here, pairs 6, 68 and 141, are now right on the first try.)
    let mut rng = crate::test_rng::Rng::new(5);
    let t = tie(&TOL);
    let cases = [
        (49, Op::Intersection),
        (157, Op::Difference),
        (225, Op::Union),
    ];
    for i in 0..=225 {
        let (ga, gb) = (random_grid_corner(&mut rng), random_grid_corner(&mut rng));
        let turn = random_turn(&mut rng);
        let nudge = rng.direction() * t * 10f64.powf(rng.range(-1.5, 1.5));
        let Some(&(_, op)) = cases.iter().find(|c| c.0 == i) else {
            continue;
        };
        let ((a, ca), (b, cb)) = (grid_box(ga.0, ga.1), grid_box(gb.0, gb.1));
        let (a, b) = (turned(&a, turn), turned(&b, (turn.0, turn.1 + nudge)));
        let (ia, ib) = (Input::new(a.mesh(), &TOL), Input::new(b.mesh(), &TOL));
        let spent = |tie: f64, both: bool| {
            let mut work = Work::new(&Budget::DEFAULT);
            let soup = if both {
                flat_soup(op, &ia, &ib, tie, &TOL, &mut work)
            } else {
                flat_decided(op, &ia, &ib, tie, &TOL, &mut work)
            };
            (
                soup.map(|_| ()).stripped(),
                Budget::DEFAULT.work() - work.left(),
            )
        };
        let (tied, first) = spent(t, false);
        assert_eq!(
            tied,
            Err(KernelError::Boolean(BooleanError::Inconsistent)),
            "{i}"
        );
        let (exact, second) = spent(0.0, false);
        assert_eq!(exact, Ok(()), "{i}");
        // Both tries are paid for.
        let (retried, both) = spent(t, true);
        assert_eq!(retried, Ok(()), "{i}");
        assert_eq!(both, first + second, "{i}");
        // And the whole operation is right, its volume the cells' within
        // the move.
        let r = run(&a, &b, op).unwrap_or_else(|e| panic!("{i} {op:?}: {e}"));
        let want = cells_volume(&combine(&ca, &cb, op));
        assert!((r.volume() - want).abs() < 1e-5, "{i}: {}", r.volume());
        assert_eq!(touches(&a, &b, &TOL, &Budget::DEFAULT), Ok(true), "{i}");
    }
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
                Err(KernelError::Invalid(_) | KernelError::Boolean(BooleanError::NotManifold)) => {}
                Err(e) => panic!("{i}/{step} {op:?}: {e}"),
            }
        }
    }
    if chains == 100 {
        assert!(right >= 349, "{right} of {all}");
    }
}

#[test]
fn turned_grid_boxes_moved_along_the_projection_are_right_or_refused() {
    // Turned grid boxes, the second moved along `UP` by 1 to 10⁷ times
    // the tie distance (far from the origin for every other pair): edges
    // that were flush now have shadows on one line, one edge above the
    // other. Where the shadows were decided to cross (each edge's ends
    // on the other's shadow to within the tie, so the perturbation's),
    // the height was taken from its constant term, the gap times only
    // rounding, against the perturbation's side, and the edges came out
    // either way round however far apart: pair 467's union lost 4e-6 of
    // volume (the boxes 1.8e-5 apart), pair 556's intersection, a
    // million from the origin, gained 1.4e-5 (2.8e-4 apart). Every
    // result must have the boxes' volume, worked out in the grid's frame
    // (to within the moves of a few ties, where the boxes are that
    // close), or fail as `Invalid`.
    let mut rng = crate::test_rng::Rng::new(5);
    let t = tie(&TOL);
    let pairs = if cfg!(debug_assertions) {
        vec![467, 556]
    } else {
        (0..300).chain([467, 556]).collect()
    };
    let last = *pairs.last().unwrap();
    let corner = |g: ([i32; 3], [i32; 3])| {
        let at = |i: i32| -0.5 + 0.5 * f64::from(i);
        let lo = DVec3::new(at(g.0[0]), at(g.0[1]), at(g.0[2]));
        (lo, lo + DVec3::from(g.1.map(f64::from)) * 0.5)
    };
    let volume = |(lo, hi): (DVec3, DVec3)| {
        let d = (hi - lo).max(DVec3::ZERO);
        d.x * d.y * d.z
    };
    let area = |(lo, hi): (DVec3, DVec3)| {
        let d = hi - lo;
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    };
    let (mut right, mut refused) = (0, 0);
    for i in 0..=last {
        let (ga, gb) = (random_grid_corner(&mut rng), random_grid_corner(&mut rng));
        let (q, shift) = random_turn(&mut rng);
        let gap = t * 10f64.powf(rng.range(0.0, 7.0));
        let way = if rng.unit() < 0.5 { 1.0 } else { -1.0 };
        if !pairs.contains(&i) {
            continue;
        }
        let shift = shift * if i % 2 == 0 { 1e4 } else { 1.0 };
        // The move in the grid's frame.
        let v = q.inverse() * UP.normalize() * (gap * way);
        let (a, b) = (grid_box(ga.0, ga.1).0, grid_box(gb.0, gb.1).0);
        let a = turned(&a, (q, shift));
        let b = moved_free(&b, |p| q * (p + v) + shift);
        let (ba, bb) = (corner(ga), corner(gb));
        let bb = (bb.0 + v, bb.1 + v);
        let both = volume((ba.0.max(bb.0), ba.1.min(bb.1)));
        let (va, vb) = (volume(ba), volume(bb));
        // Boxes closer than the resolution may come out as touching.
        let close = if gap <= 64.0 * t { gap } else { 0.0 };
        let within = (area(ba) + area(bb)) * (close + 4.0 * t) + 1e-9;
        for (op, want) in [
            (Op::Union, va + vb - both),
            (Op::Intersection, both),
            (Op::Difference, va - both),
        ] {
            match run(&a, &b, op) {
                Ok(s) => {
                    assert!(
                        (s.volume() - want).abs() <= within,
                        "{i} {op:?} (moved {gap:e}): {} not {want}",
                        s.volume()
                    );
                    right += 1;
                }
                Err(KernelError::Invalid(_) | KernelError::Boolean(BooleanError::NotManifold)) => {
                    refused += 1
                }
                Err(e) => panic!("{i} {op:?} (moved {gap:e}): {e}"),
            }
        }
    }
    // 740 of the 906 in a release build, the rest `Invalid`: parts
    // closer than the resolution, and thin slivers whose triangles fail
    // the hull rules.
    assert!(right >= refused, "{right}, {refused} refused");
}

#[test]
fn boxes_flush_with_a_slanted_wall_on_tilted_frames() {
    // A hexagonal prism (circumradius 3, 4 tall) and boxes extruded on a
    // frame on one of its slanted walls: joined flush to it (a boss),
    // cut flush into it (a pocket), straddling it, at the wall's ends
    // (sharing the prism's edge lines), flush with the prism's top too,
    // and a slot the wall's height. The prism upright, and on random
    // frames, turned and moved, so the wall's plane and every flush face
    // are flush only to rounding. Every operation works and has the
    // analytic volume. With only the constant term tied, 5 of the 6
    // turned frames' 216 operations were `Inconsistent` and one
    // `Invalid` (of 60 such frames' 2 160, 97 and 6; now 8 fail, all the
    // union with the boss in the corner, as `Invalid(Hull)`).
    use crate::profile::tests::{polygon, rect};
    use crate::{Frame, Profile, extrude};
    use glam::{DQuat, DVec2};
    let extruded = |loops, frame: Frame, from, to, feature| {
        let profile = Profile { loops: vec![loops] };
        extrude(&profile, &frame, from, to, feature, &TOL, &Budget::DEFAULT).unwrap()
    };
    let (r, height) = (3.0, 4.0);
    let h = r * 3f64.sqrt() / 2.0;
    let hexagon = [
        DVec2::new(r, 0.0),
        DVec2::new(r / 2.0, h),
        DVec2::new(-r / 2.0, h),
        DVec2::new(-r, 0.0),
        DVec2::new(-r / 2.0, -h),
        DVec2::new(r / 2.0, -h),
    ];
    let prism_volume = 3.0 * r * h * height;
    // The prism upright, then on frames turned and moved at random.
    let mut rng = crate::test_rng::Rng::new(40);
    let bases = std::iter::once(Frame::XY).chain((0..6).map(|_| {
        let q = DQuat::from_axis_angle(rng.direction(), rng.range(0.0, 6.0));
        Frame {
            origin: rng.point(100.0),
            x: q * DVec3::X,
            y: q * DVec3::Y,
        }
    }));
    // Rectangles on the wall's frame (`x` along the wall, 3 long, `y` up
    // the prism, 4 tall, both from its middle) and the depths they are
    // extruded between (out of the prism is positive).
    let boxes = [
        ("boss", [-0.5, -1.0], [0.75, 1.25], 0.0, 1.0),
        ("pocket", [-0.5, -1.0], [0.75, 1.25], -1.0, 0.0),
        ("straddling", [-0.5, -1.0], [0.75, 1.25], -0.5, 0.75),
        ("boss at the end", [-1.5, 0.0], [0.0, 2.0], 0.0, 1.0),
        ("pocket at the end", [-1.5, 0.0], [0.0, 2.0], -1.0, 0.0),
        ("boss at the other end", [1.0, 0.5], [1.5, 1.25], 0.0, 1.0),
        ("boss in the corner", [1.25, 1.75], [1.5, 2.0], 0.0, 1.0),
        ("slot", [-0.25, -2.0], [0.25, 2.0], -1.0, 0.0),
        ("pocket at the top", [-1.0, 1.25], [0.25, 2.0], -1.0, 0.0),
    ];
    for (k, base) in bases.enumerate() {
        let prism = extruded(polygon(&hexagon, 0), base, 0.0, height, 1);
        let (b, c) = (hexagon[0], hexagon[1]);
        let along = (c - b).normalize();
        let wall = Frame {
            origin: base.point((b + c) * 0.5, height / 2.0),
            x: base.x * along.x + base.y * along.y,
            y: base.normal(),
        };
        for (name, min, max, from, to) in boxes {
            let (min, max) = (DVec2::from(min), DVec2::from(max));
            let tool = extruded(rect(min, max, 0), wall, from, to, 2);
            let area = (max.x - min.x) * (max.y - min.y);
            let inside = area * (-f64::min(from, 0.0));
            let tool_volume = area * (to - from);
            let want = volumes(prism_volume, tool_volume, inside);
            for (op, x, y, want) in [
                (Op::Union, &prism, &tool, want[0]),
                (Op::Intersection, &prism, &tool, want[1]),
                (Op::Difference, &prism, &tool, want[2]),
                (Op::Difference, &tool, &prism, want[3]),
            ] {
                let got = run(x, y, op)
                    .unwrap_or_else(|e| panic!("frame {k}, {name}, {op:?}: {e}"))
                    .volume();
                assert!(
                    (got - want).abs() < 1e-9,
                    "frame {k}, {name}, {op:?}: {got} not {want}"
                );
            }
        }
    }
}

// One surface, one face.

/// A box of feature `feature`.
fn box_of(min: [f64; 3], size: [f64; 3], feature: u64) -> Solid {
    Solid::cuboid(DVec3::from(min), DVec3::from(size), feature, &TOL).unwrap()
}

/// How many faces (keys) of `solid` lie on a plane facing along `n`.
fn planes_facing(solid: &Solid, n: DVec3) -> usize {
    let keys: std::collections::BTreeSet<_> = solid
        .mesh()
        .faces()
        .iter()
        .filter(|f| match f.surface {
            Surface::Plane { n: m, .. } => m.normalize().dot(n) > 1.0 - 1e-12,
            _ => false,
        })
        .map(|f| f.name.key())
        .collect();
    keys.len()
}

/// How many faces (keys) `solid` has.
fn keys(solid: &Solid) -> usize {
    let keys: std::collections::BTreeSet<_> =
        solid.mesh().faces().iter().map(|f| f.name.key()).collect();
    keys.len()
}

/// The midpoints of `solid`'s drawn feature edges.
fn feature_middles(solid: &Solid) -> Vec<DVec3> {
    let render = solid.tessellate(&crate::Display::new(&TOL)).unwrap();
    let at = |i: u32| DVec3::from(render.positions()[i as usize].map(f64::from));
    render
        .edge_segments()
        .map(|[a, b]| (at(a) + at(b)) * 0.5)
        .collect()
}

#[test]
fn stacked_boxes_are_one_box_of_faces() {
    // A box on another: its sides and the bottom's are four faces, each
    // named by the first operand, the second's names aliases of them.
    let a = box_of([0.0; 3], [1.0; 3], 1);
    let b = box_of([0.0, 0.0, 1.0], [1.0; 3], 2);
    for (first, second) in [(&a, &b), (&b, &a)] {
        let r = run(first, second, Op::Union).unwrap();
        assert!((r.volume() - 2.0).abs() < 1e-12);
        let mesh = r.mesh();
        assert_eq!(keys(&r), 6);
        let lead = first.mesh().faces()[0].name.feature;
        for face in mesh.faces() {
            let cap = matches!(face.name.part, FacePart::StartCap | FacePart::EndCap);
            if !cap {
                assert_eq!(face.name.feature, lead, "{face:?}");
            }
        }
        // Every key of either operand names a face of the result.
        let topology = r.topology();
        let sides = [DVec3::new(0.5, 0.0, 1.5), DVec3::new(1.0, 0.5, 0.5)];
        for operand in [&a, &b] {
            for face in operand.mesh().faces() {
                let cap = matches!(face.name.part, FacePart::StartCap | FacePart::EndCap);
                if cap {
                    continue;
                }
                for near in sides {
                    assert!(topology.face(&r, &face.name.key(), near).is_ok());
                }
            }
        }
        // A box's twelve edges: no line round the middle.
        assert!(feature_middles(&r).iter().all(|m| (m.z - 1.0).abs() > 1e-9));
    }
}

#[test]
fn an_l_of_two_boxes_has_one_top_and_one_bottom() {
    let a = box_of([0.0; 3], [2.0, 1.0, 1.0], 1);
    let b = box_of([0.0, 1.0, 0.0], [1.0, 1.0, 1.0], 2);
    let r = run(&a, &b, Op::Union).unwrap();
    assert!((r.volume() - 3.0).abs() < 1e-12);
    // Six sides (the two at x = 0 one face), a top and a bottom.
    assert_eq!(keys(&r), 8);
    assert_eq!(planes_facing(&r, DVec3::Z), 1);
    assert_eq!(planes_facing(&r, -DVec3::Z), 1);
    // No line drawn inside the top's or the bottom's outline.
    let inside = |m: &DVec3| {
        let e = 1e-6;
        (m.x > e && m.x < 2.0 - e && m.y > e && m.y < 1.0 - e)
            || (m.x > e && m.x < 1.0 - e && m.y > e && m.y < 2.0 - e)
    };
    for m in feature_middles(&r) {
        assert!(!inside(&m), "a line across the flat at {m}");
    }
}

#[test]
fn flush_differences_and_intersections_keep_a_face_a_plane() {
    // Overlapping boxes sharing four planes: each result is a box.
    let a = box_of([0.0; 3], [2.0; 3], 1);
    let b = box_of([1.0, 0.0, 0.0], [2.0; 3], 2);
    for (op, volume) in [
        (Op::Union, 12.0),
        (Op::Difference, 4.0),
        (Op::Intersection, 4.0),
    ] {
        let r = run(&a, &b, op).unwrap();
        assert!((r.volume() - volume).abs() < 1e-12, "{op:?}");
        assert_eq!(keys(&r), 6, "{op:?}");
    }
}

#[test]
fn tops_a_step_apart_stay_two_faces() {
    // Side by side, one a thousandth taller: the bottoms and the flush
    // sides merge, the tops don't, and the step is drawn.
    let a = box_of([0.0; 3], [1.0; 3], 1);
    let b = box_of([1.0, 0.0, 0.0], [1.0, 1.0, 1.001], 2);
    let r = run(&a, &b, Op::Union).unwrap();
    assert!((r.volume() - 2.001).abs() < 1e-12);
    assert_eq!(planes_facing(&r, DVec3::Z), 2);
    assert_eq!(planes_facing(&r, -DVec3::Z), 1);
    assert_eq!(planes_facing(&r, -DVec3::Y), 1);
    assert!(
        feature_middles(&r)
            .iter()
            .any(|m| (m.x - 1.0).abs() < 1e-9 && m.z > 0.5)
    );
}

#[test]
fn merged_faces_are_deterministic() {
    let a = box_of([0.0; 3], [2.0, 1.0, 1.0], 1);
    let b = box_of([0.0, 1.0, 0.0], [1.0, 1.0, 1.0], 2);
    assert_deterministic(|| run(&a, &b, Op::Union).map(Solid::into_mesh)).unwrap();
}

#[test]
fn thin_triangles_across_two_faces() {
    // A 10 × 10 × 2 plate of four 5 × 5 squares joined, so four coplanar
    // faces meet at the vertex (5, 5) of its top, cut by a slanted wall
    // `2x + y = 15 − e` passing `k` resolutions from that vertex, as a
    // boss (1.1 to 4.3) and a pocket (1.1 to 3.3). Each operation is right
    // (volumes from the area `5(c − 10) + 25` of the square on the near
    // side of `2x + y = c`) or fails; counted: the failures whose cleaned
    // result had a thin triangle across two faces, and of those, ones
    // whose far corner lies inside a plane (`cleanup::thin_across`).
    // All 40 fail today, 20 of them with such triangles, but none with the
    // far corner inside a plane: there the thin triangle's far corner is a
    // cut vertex and its long side lies along the seam of two of the top's
    // faces, so splitting that side and collapsing the corner onto it
    // wouldn't apply. The operations fail on the thin triangles round the
    // cut's few vertices near (5, 5) all the same.
    use crate::profile::tests::polygon;
    use crate::{Frame, Profile, extrude};
    let prism = |points: &[(f64, f64)], from: f64, to: f64, feature: u64| {
        let points: Vec<_> = points
            .iter()
            .map(|&(x, y)| glam::DVec2::new(x, y))
            .collect();
        let profile = Profile {
            loops: vec![polygon(&points, feature * 10)],
        };
        extrude(
            &profile,
            &Frame::XY,
            from,
            to,
            feature,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap()
    };
    let square = |x: f64, y: f64, feature: u64| {
        prism(
            &[(x, y), (x + 5.0, y), (x + 5.0, y + 5.0), (x, y + 5.0)],
            0.0,
            2.0,
            feature,
        )
    };
    let mut plate = square(0.0, 0.0, 1);
    for (i, (x, y)) in [(5.0, 0.0), (0.0, 5.0), (5.0, 5.0)].into_iter().enumerate() {
        plate = boolean(
            &plate,
            &square(x, y, 2 + i as u64),
            Op::Union,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap();
    }
    assert!((plate.volume() - 200.0).abs() < 1e-9);
    let res = TOL.resolution();
    let (mut operations, mut failed, mut thin, mut mendable) = (0, 0, 0, 0);
    for k in [0.2, 0.5, 1.0, 2.0, 4.0] {
        let e = k * res * 5f64.sqrt();
        let c = 15.0 - e;
        for (name, lo, hi) in [("boss", 1.1, 4.3), ("pocket", 1.1, 3.3)] {
            let tool = prism(
                &[(-5.0, -5.0), (10.0 - e / 2.0, -5.0), (-5.0, 25.0 - e)],
                lo,
                hi,
                9,
            );
            let shared = (5.0 * (c - 10.0) + 25.0) * (2.0f64.min(hi) - lo);
            let (vp, vt) = (plate.volume(), tool.volume());
            let jobs = [
                (&plate, &tool, Op::Union, vp + vt - shared),
                (&plate, &tool, Op::Intersection, shared),
                (&plate, &tool, Op::Difference, vp - shared),
                (&tool, &plate, Op::Difference, vt - shared),
            ];
            for (a, b, op, want) in jobs {
                operations += 1;
                match boolean(a, b, op, &TOL, &Budget::DEFAULT) {
                    Ok(solid) => {
                        let got = solid.volume();
                        assert!(
                            (got - want).abs() < 1e-8,
                            "k {k}, {name}, {op:?}: {got} vs {want}"
                        );
                    }
                    Err(why) => {
                        failed += 1;
                        let (all, inside) = THIN_ACROSS.get();
                        println!(
                            "k {k}, {name}, {op:?}: {why:?}, thin across {all}, inside a plane {inside}"
                        );
                        thin += usize::from(all > 0);
                        mendable += usize::from(inside > 0);
                    }
                }
            }
        }
    }
    println!(
        "{failed} of {operations} failed, {thin} with thin triangles across two faces, {mendable} of them with the far corner inside a plane"
    );
    assert_eq!(mendable, 0);
}

/// The patches of `mesh`'s triangles that `why` names, for the errors
/// these tests reach: one for a triangle's own failure, two (or one, if
/// the same) for a pair's.
fn named_patches(mesh: &Mesh, why: CheckError) -> Vec<crate::patch::Patch> {
    let tris = match why {
        CheckError::Fold(t) | CheckError::Face(t) | CheckError::FacesAgainst(t) => vec![t],
        CheckError::Hull(t, u)
        | CheckError::EdgeNeighbours(t, u)
        | CheckError::VertexNeighbours(t, u)
        | CheckError::SameCorners(t, u) => {
            if t == u {
                vec![t]
            } else {
                vec![t, u]
            }
        }
        why => panic!("{why:?}"),
    };
    tris.into_iter().map(|t| mesh.patch(t as usize)).collect()
}

/// `a op b`'s failure at `tol`, the same at 1 and 8 threads, checked to
/// be `want` (the error it gave before failures carried evidence) and
/// to carry what the first try's error names, of the mesh that failed:
/// the pieces repair names where repair failed, the triangles of the
/// repaired mesh where the check did. Each patch lies within the
/// operands' boxes.
/// Checks `a op b` fails with `want` (a `NotManifold` from repair or the
/// check), deterministically, with the triangles the check's error names
/// (or repair's pieces), within the operands' box; and returns it, for
/// its points (a pinch's) to be checked.
fn fails_with_its_triangles(
    a: &Solid,
    b: &Solid,
    op: Op,
    tol: &Tolerance,
    want: KernelError,
) -> Failure {
    let failure = assert_deterministic(|| boolean(a, b, op, tol, &Budget::DEFAULT).unwrap_err());
    assert_eq!(failure.error, want);
    let mut work = Work::new(&Budget::DEFAULT);
    let cleaned = unchecked(a, b, op, tol, &mut work).unwrap();
    let Err((error @ KernelError::Invalid(why), Some(unfinished))) =
        Solid::finished_or_unfinished(cleaned, CHECK_WORK, tol, &mut work)
    else {
        panic!("the first try passes");
    };
    // Renamed `NotManifold` or not, the check's evidence.
    assert!(
        error == want || want == KernelError::Boolean(BooleanError::NotManifold),
        "{error:?}"
    );
    let named = match &unfinished {
        Unfinished::Repair { pieces, .. } if !pieces.is_empty() => pieces.clone(),
        Unfinished::Repair { given: mesh, .. } | Unfinished::Check { checked: mesh, .. } => {
            named_patches(mesh, why)
        }
    };
    assert!(!named.is_empty());
    assert_eq!(failure.evidence.patches, named, "{why:?}");
    let evidence = crate::Evidence {
        patches: Vec::new(),
        points: Vec::new(),
        ..(*failure.evidence).clone()
    };
    assert!(evidence.is_empty());
    let (ba, bb) = (a.bounds3().unwrap(), b.bounds3().unwrap());
    let slack = DVec3::splat(tol.resolution());
    let (lo, hi) = (ba.min.min(bb.min) - slack, ba.max.max(bb.max) + slack);
    for patch in &failure.evidence.patches {
        for p in patch.p.iter().chain(&patch.c) {
            assert!(p.cmpge(lo).all() && p.cmple(hi).all(), "{p}");
        }
    }
    failure
}

#[test]
fn a_pinch_comes_with_its_two_vertices() {
    // Boxes sharing an edge, or a corner, united: the neck the
    // perturbation leaves has two vertices within the clean-up's short
    // length of each other, given as points (one where they are at one
    // place) beside the triangles that failed, on the edge or at the
    // corner. Either way round, and the same at 1 and 8 threads.
    let a = cube([0.0; 3], [2.0; 3]);
    let not_manifold = KernelError::Boolean(BooleanError::NotManifold);
    let r = TOL.resolution();
    let on_edge = |p: DVec3| {
        (p.x - 2.0).abs() <= r && (p.y - 2.0).abs() <= r && (-r..=2.0 + r).contains(&p.z)
    };
    let at_corner = |p: DVec3| p.distance(DVec3::splat(2.0)) <= r;
    let cases: [(Solid, &dyn Fn(DVec3) -> bool); 2] = [
        (cube([2.0, 2.0, 0.0], [2.0; 3]), &on_edge),
        (cube([2.0; 3], [2.0; 3]), &at_corner),
    ];
    for (b, near) in &cases {
        for (x, y) in [(&a, b), (b, &a)] {
            let failure = fails_with_its_triangles(x, y, Op::Union, &TOL, not_manifold);
            let points = &failure.evidence.points;
            assert!(matches!(points.len(), 1 | 2), "{points:?}");
            assert!(points.iter().all(|&p| near(p)), "{points:?}");
            if let [p, q] = points[..] {
                assert!(p != q && p.distance(q) <= short(&TOL), "{points:?}");
            }
        }
    }
}

#[test]
fn a_mesh_that_doesnt_pair_up_names_what_the_builder_found() {
    // The soup's triangles go to the mesh's builder as they are: a lone
    // triangle has halfedges with none running back, and the builder's
    // `Open` comes back as `Degenerate` with that halfedge (its curve,
    // where it has one) and its ends.
    let pos = vec![DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(5.0, 5.0, 5.0)];
    let face = cube([0.0; 3], [1.0; 3]).mesh().faces()[0];
    let ctrl = DVec3::new(0.5, -0.5, 0.0);
    let soup = cleanup::Soup {
        pos: pos.clone(),
        tris: vec![[0, 1, 2]],
        faces: vec![0],
        curves: [((0, 1), crate::mesh::Edge { ctrl, weight: 0.5 })]
            .into_iter()
            .collect(),
        sources: vec![0],
        absorbed: Vec::new(),
        made: vec![true],
    };
    let failure = build(soup, vec![face], &[Vec::new()]).unwrap_err();
    assert_eq!(
        failure.error,
        KernelError::Boolean(BooleanError::Degenerate)
    );
    let [curve] = failure.evidence.curves[..] else {
        panic!("{failure:?}");
    };
    let ends = [curve.p0, curve.p1];
    assert!(
        ends == [pos[0], pos[1]] || ends == [pos[1], pos[0]],
        "{curve:?}"
    );
    assert_eq!((curve.c, curve.w), (ctrl, 0.5));
    assert_eq!(failure.evidence.points.len(), 2);
    assert!(failure.evidence.points.iter().all(|p| ends.contains(p)));

    // A triangle naming one vertex twice: its sides and its corners, each
    // place once.
    let soup = cleanup::Soup {
        pos,
        tris: vec![[0, 1, 1]],
        faces: vec![0],
        curves: BTreeMap::new(),
        sources: vec![0],
        absorbed: Vec::new(),
        made: vec![true],
    };
    let failure = build(soup, vec![face], &[Vec::new()]).unwrap_err();
    assert_eq!(
        failure.error,
        KernelError::Boolean(BooleanError::Degenerate)
    );
    assert_eq!(failure.evidence.curves.len(), 3);
    assert_eq!(failure.evidence.points, [DVec3::ZERO, DVec3::X]);
}

#[test]
fn failures_of_the_result_carry_the_triangles_they_name() {
    // Boxes along an edge, united: a neck the check refuses, named a
    // pinch, with the triangles that failed.
    let a = cube([0.0; 3], [2.0; 3]);
    let not_manifold = KernelError::Boolean(BooleanError::NotManifold);
    for b in [cube([2.0, 2.0, 0.0], [2.0; 3]), cube([2.0; 3], [2.0; 3])] {
        fails_with_its_triangles(&a, &b, Op::Union, &TOL, not_manifold);
    }
    // A void whose wall is half a resolution thick: repair's `Hull`
    // between two shells, named a pinch too, with no near vertices.
    let half = 0.5 * TOL.resolution();
    let tube = Solid::cylinder(DVec3::ZERO, 1.0, 10.0, 1, &TOL).unwrap();
    let hole = Solid::cylinder(DVec3::Z * 2.0, 1.0 - half, 6.0, 2, &TOL).unwrap();
    let hull = fails_with_its_triangles(&tube, &hole, Op::Difference, &TOL, not_manifold);
    assert!(hull.evidence.points.is_empty());
    // A boss tangent to a plate's side from inside: a cusp no patch
    // holds, `Invalid` as it was.
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let plate = Solid::cuboid(DVec3::ZERO, DVec3::new(4.0, 2.0, 1.0), 1, &coarse).unwrap();
    let boss = Solid::cylinder(DVec3::new(2.0, 0.5, 0.5), 0.5, 1.5, 2, &coarse).unwrap();
    let fold = KernelError::Invalid(CheckError::Fold(35));
    fails_with_its_triangles(&plate, &boss, Op::Union, &coarse, fold);
}

#[test]
fn a_pinch_is_told_from_the_mesh_repair_was_given() {
    // `pinched_named` on each kind of mesh a failure leaves, built by
    // hand, as the boolean's failures rarely reach some (the check
    // refusing what repair passed): the near vertices are those of the
    // mesh repair was given, the cleaned mesh (the checked one where
    // repair kept it as it was), never the repaired one's; a `Hull` is
    // told apart on the mesh whose triangles it names. The work: a unit
    // a vertex and one a vertex measured against, then for a `Hull` a
    // unit a triangle listed of the check's mesh (the cleaned mesh's go
    // uncharged) and one a triangle `apart` looks at. The evidence the
    // error came with stays, renamed or not, and a near pair adds its
    // two vertices.
    let d = short(&TOL);
    let shells = |second: [f64; 3]| {
        crate::mesh::tests::joined(&[
            (cube([0.0; 3], [1.0; 3]).mesh(), false),
            (cube(second, [1.0; 3]).mesh(), false),
        ])
    };
    // Shells apart, and shells with vertices within the short length.
    let far = shells([3.0; 3]);
    let near = shells([1.0 + 0.5 * d, 0.0, 0.0]);
    let half = far.tris().len() as u32 / 2;
    let not_manifold = KernelError::Boolean(BooleanError::NotManifold);
    let invalid = KernelError::Invalid(CheckError::Hull(0, half));
    let boxed = |mesh: &Mesh| Box::new(mesh.clone());
    let check = |given: Option<&Mesh>, checked: &Mesh| Unfinished::Check {
        given: given.map(boxed),
        checked: boxed(checked),
    };
    let repair = |given: &Mesh| Unfinished::Repair {
        given: boxed(given),
        pieces: Vec::new(),
    };
    let measured = |mesh: &Mesh| {
        let mut work = Work::new(&Budget::DEFAULT);
        pinched(mesh.verts(), d, &mut work).unwrap();
        Budget::DEFAULT.work() - work.left()
    };
    let n = u64::from(2 * half);
    for (unfinished, hull, want, spent) in [
        // Near vertices: the checked mesh's where repair kept the mesh.
        (check(None, &near), None, not_manifold, measured(&near)),
        // The mesh given, not the one the check refused.
        (check(Some(&far), &near), None, invalid, measured(&far)),
        (
            check(Some(&near), &far),
            None,
            not_manifold,
            measured(&near),
        ),
        (repair(&near), None, not_manifold, measured(&near)),
        // A `Hull` across shells: the check's, listed and charged.
        (
            check(None, &far),
            Some((0, half)),
            not_manifold,
            measured(&far) + 2 * n,
        ),
        (
            check(Some(&far), &far),
            Some((0, half)),
            not_manifold,
            measured(&far) + 2 * n,
        ),
        // Repair's, of the cleaned mesh: not listed.
        (
            repair(&far),
            Some((0, half)),
            not_manifold,
            measured(&far) + n,
        ),
        // Within one shell: as it was.
        (repair(&far), Some((0, 1)), invalid, measured(&far) + n),
        // Near vertices first: the `Hull` isn't looked at.
        (
            check(None, &near),
            Some((0, half)),
            not_manifold,
            measured(&near),
        ),
    ] {
        let mut evidence = crate::Evidence::default();
        evidence.add_patches([far.patch(0)]);
        let failure = Failure {
            error: invalid,
            evidence: Box::new(evidence.clone()),
        };
        let pinch = pinched(
            unfinished.given().verts(),
            d,
            &mut Work::new(&Budget::DEFAULT),
        );
        if let Ok(Some(pair)) = pinch {
            evidence.add_points(pair);
        }
        let failed = Some(Failed { unfinished, hull });
        let mut work = Work::new(&Budget::DEFAULT);
        let got = pinched_named(Err(failure), &failed, &TOL, &mut work).unwrap_err();
        assert_eq!(got.error, want, "{hull:?}");
        assert_eq!(*got.evidence, evidence);
        assert_eq!(Budget::DEFAULT.work() - work.left(), spent, "{hull:?}");
    }
}
