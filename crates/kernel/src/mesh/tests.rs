#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use glam::DVec3;

use super::*;
use crate::Tolerance;
use crate::par::assert_deterministic;
use crate::patch::{Conic3, PatchError, cylinder_strip};
use crate::test_rng::Rng;

pub(crate) const TOL: Tolerance = Tolerance::DEFAULT;

/// A face named `part` of feature 1 on `surface`.
pub(crate) fn face(part: u32, surface: Surface) -> Face {
    Face {
        name: FaceName {
            feature: 1,
            part: FacePart::Split(part),
        },
        surface,
    }
}

pub(crate) fn free(builder: &mut MeshBuilder) -> u32 {
    builder.face(face(0, Surface::Free))
}

/// The tetrahedron on the origin and the unit axes, moved by `offset`,
/// added to `builder`.
pub(crate) fn add_tetrahedron(builder: &mut MeshBuilder, offset: DVec3) {
    let f = free(builder);
    let v = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z].map(|p| builder.vert(p + offset));
    for [a, b, c] in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
        builder.tri([v[a], v[b], v[c]], f);
    }
}

pub(crate) fn tetrahedron(offset: DVec3) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_tetrahedron(&mut builder, offset);
    builder.build().unwrap()
}

/// The octahedron's triangles, counter-clockwise from outside, on its
/// vertices `+x, +y, +z, -x, -y, -z`.
pub(crate) const OCTAHEDRON: [[u32; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 2],
    [1, 3, 2],
    [4, 0, 2],
    [1, 0, 5],
    [3, 1, 5],
    [4, 3, 5],
    [0, 4, 5],
];

/// An octahedron with vertices `verts`, triangles `tris` (each on its own
/// free face) and curved edges `curves`.
pub(crate) fn octahedron(
    verts: [DVec3; 6],
    tris: &[[u32; 3]],
    curves: &[(u32, u32, DVec3, f64)],
) -> Mesh {
    let mut builder = MeshBuilder::new();
    for p in verts {
        builder.vert(p);
    }
    for (i, &tri) in tris.iter().enumerate() {
        let f = builder.face(face(i as u32, Surface::Free));
        builder.tri(tri, f);
    }
    for &(a, b, c, w) in curves {
        builder.edge(a, b, c, w);
    }
    builder.build().unwrap()
}

pub(crate) const UNIT: [DVec3; 6] = [
    DVec3::X,
    DVec3::Y,
    DVec3::Z,
    DVec3::NEG_X,
    DVec3::NEG_Y,
    DVec3::NEG_Z,
];

/// Adds to `builder` the octahedron of `radius` around `centre` with
/// every edge a quarter circle through its ends (control point where the
/// end tangents meet, weight √½), so each patch is on the sphere along its
/// edges. It faces out, or in if `inward`, and is one free face.
pub(crate) fn add_round_octahedron(
    builder: &mut MeshBuilder,
    centre: DVec3,
    radius: f64,
    inward: bool,
) {
    let f = free(builder);
    let v = UNIT.map(|p| builder.vert(centre + p * radius));
    for [a, b, c] in OCTAHEDRON {
        let tri = if inward { [a, c, b] } else { [a, b, c] };
        builder.tri(tri.map(|i| v[i as usize]), f);
        for (x, y) in [(a, b), (b, c), (c, a)] {
            let ctrl = centre + (UNIT[x as usize] + UNIT[y as usize]) * radius;
            builder.edge(v[x as usize], v[y as usize], ctrl, FRAC_1_SQRT_2);
        }
    }
}

/// The unit round octahedron (see [`add_round_octahedron`]) around
/// `offset`.
pub(crate) fn round_octahedron(offset: DVec3) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, offset, 1.0, false);
    builder.build().unwrap()
}

/// A torus of flat triangles around the z axis: `n` steps around the
/// axis, `m` around the tube.
pub(crate) fn torus(n: u32, m: u32, big: f64, small: f64) -> Mesh {
    let mut builder = MeshBuilder::new();
    let f = free(&mut builder);
    for i in 0..n {
        for j in 0..m {
            let (s, c) = (TAU * i as f64 / n as f64).sin_cos();
            let (sp, cp) = (TAU * j as f64 / m as f64).sin_cos();
            let r = big + small * cp;
            builder.vert(DVec3::new(r * c, r * s, small * sp));
        }
    }
    let id = |i: u32, j: u32| (i % n) * m + j % m;
    for i in 0..n {
        for j in 0..m {
            let [a, b, c, d] = [id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1)];
            builder.tri([a, b, c], f);
            builder.tri([a, c, d], f);
        }
    }
    builder.build().unwrap()
}

/// A half cylinder of radius 1 and height `h`, standing on z = 0 with its
/// flat side on y = 0: two exact quarter-circle walls from
/// [`cylinder_strip`], two quarter-disc caps at each end, and a flat wall
/// of two rectangles. Faces are tagged with their planes and cylinder,
/// offset by `offset`.
pub(crate) fn half_cylinder(h: f64, offset: DVec3) -> Mesh {
    let mut builder = MeshBuilder::new();
    let at = |x: f64, y: f64, z: f64| DVec3::new(x, y, z) + offset;
    // Bottom: centre, +x, +y, -x; then the same on top.
    let [o, a, b, c] = [
        at(0.0, 0.0, 0.0),
        at(1.0, 0.0, 0.0),
        at(0.0, 1.0, 0.0),
        at(-1.0, 0.0, 0.0),
    ];
    let up = DVec3::Z * h;
    let v = [o, a, b, c, o + up, a + up, b + up, c + up].map(|p| builder.vert(p));
    let [vo, va, vb, vc, to, ta, tb, tc] = v;
    let plane = |n: DVec3, p: DVec3| Surface::Plane { n, d: n.dot(p) };
    let bottom = builder.face(face(0, plane(-DVec3::Z, o)));
    let top = builder.face(face(1, plane(DVec3::Z, o + up)));
    let flat = builder.face(face(2, plane(-DVec3::Y, o)));
    let round = builder.face(face(
        3,
        Surface::Quadric(Quadric::cylinder(o, DVec3::Z, 1.0).unwrap()),
    ));
    builder.tri([vo, vb, va], bottom);
    builder.tri([vo, vc, vb], bottom);
    builder.tri([to, ta, tb], top);
    builder.tri([to, tb, tc], top);
    builder.tri([vo, va, ta], flat);
    builder.tri([vo, ta, to], flat);
    builder.tri([vc, vo, to], flat);
    builder.tri([vc, to, tc], flat);
    let w = FRAC_1_SQRT_2;
    for (p, q, vp, vq, tp, tq) in [(a, b, va, vb, ta, tb), (b, c, vb, vc, tb, tc)] {
        let ctrl = p + q - o;
        let arc = Conic3::new(p, ctrl, w, q).unwrap();
        let strip = cylinder_strip(&arc, up).unwrap();
        builder.edge(vp, vq, ctrl, w);
        builder.edge(tp, tq, ctrl + up, w);
        builder.edge(vp, tq, strip[0].c[2], strip[0].w[2]);
        builder.tri([vp, vq, tq], round);
        builder.tri([vp, tq, tp], round);
    }
    builder.build().unwrap()
}

#[test]
fn hand_built_solids_pass() {
    let far = DVec3::new(1e5, -2e5, 3e5);
    for offset in [DVec3::ZERO, far] {
        let solids = [
            tetrahedron(offset),
            Mesh::cuboid(offset, DVec3::new(1.0, 2.0, 3.0), 1, &TOL).unwrap(),
            octahedron(UNIT.map(|p| p + offset), &OCTAHEDRON, &[]),
            round_octahedron(offset),
            half_cylinder(2.0, offset),
        ];
        for (i, mesh) in solids.iter().enumerate() {
            assert_eq!(mesh.check(&TOL), Ok(()), "solid {i} at {offset}");
            assert_eq!(mesh.check_faces(&TOL), Ok(()), "solid {i} at {offset}");
        }
    }
    assert_eq!(torus(48, 24, 10.0, 4.0).check(&TOL), Ok(()));
    assert_eq!(Mesh::default().check(&TOL), Ok(()));
}

#[test]
fn the_half_cylinder_is_made_of_strips() {
    let mesh = half_cylinder(2.0, DVec3::ZERO);
    let arc = Conic3::new(DVec3::X, DVec3::new(1.0, 1.0, 0.0), FRAC_1_SQRT_2, DVec3::Y).unwrap();
    let strip = cylinder_strip(&arc, DVec3::Z * 2.0).unwrap();
    assert_eq!(mesh.patch(8), strip[0]);
    assert_eq!(mesh.patch(9), strip[1]);
    // Its round face really is on the cylinder.
    let Surface::Quadric(q) = mesh.faces()[3].surface else {
        panic!("not a quadric")
    };
    let mut rng = Rng::new(41);
    for t in 8..12 {
        for _ in 0..20 {
            assert!(q.distance(mesh.patch(t).eval(rng.bary())) < 1e-14);
        }
    }
}

#[test]
fn accessors_follow_the_halfedges() {
    let mesh = tetrahedron(DVec3::ZERO);
    assert_eq!(mesh.tris().len(), 4);
    assert_eq!(mesh.edges().len(), 6);
    for h in 0..12 {
        let he = mesh.halfedge(h);
        assert_eq!(mesh.halfedge(he.pair).pair, h);
        assert_eq!(mesh.end(he.pair), he.start);
        assert_eq!(Mesh::next(Mesh::next(Mesh::next(h))), h);
    }
    let patch = mesh.patch(3);
    assert_eq!(patch.p, [DVec3::X, DVec3::Y, DVec3::Z]);
    assert_eq!(patch.c[1], DVec3::new(0.0, 0.5, 0.5));
}

#[test]
fn builder_refuses_bad_input() {
    let mut builder = MeshBuilder::new();
    add_tetrahedron(&mut builder, DVec3::ZERO);
    builder.tri([0, 1, 2], 0);
    assert_eq!(builder.build(), Err(BuildError::Duplicate(0, 1)));

    let mut builder = MeshBuilder::new();
    let f = free(&mut builder);
    for p in [DVec3::ZERO, DVec3::X, DVec3::Y] {
        builder.vert(p);
    }
    builder.tri([0, 1, 2], f);
    assert_eq!(builder.build(), Err(BuildError::Open(0, 1)));

    for tri in [[0, 0, 1], [0, 1, 4]] {
        let mut builder = MeshBuilder::new();
        add_tetrahedron(&mut builder, DVec3::ZERO);
        builder.tri(tri, 0);
        assert_eq!(builder.build(), Err(BuildError::Tri(4)));
    }
    let mut builder = MeshBuilder::new();
    add_tetrahedron(&mut builder, DVec3::ZERO);
    builder.tri([0, 1, 2], 1);
    assert_eq!(builder.build(), Err(BuildError::Tri(4)));

    let mut builder = MeshBuilder::new();
    add_tetrahedron(&mut builder, DVec3::ZERO);
    builder.vert(DVec3::ONE);
    builder.edge(4, 0, DVec3::ONE, 1.0);
    assert_eq!(builder.build(), Err(BuildError::UnusedEdge(0, 4)));
}

// Invariant 1: topology.

#[test]
fn bad_indices_are_caught() {
    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.tris[1].halfedges[2].start = 99;
    assert_eq!(mesh.check(&TOL), Err(CheckError::Index(5)));
    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.tris[2].face = 1;
    assert_eq!(mesh.check(&TOL), Err(CheckError::Index(6)));
    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.edges.push(Edge::straight(DVec3::ZERO, DVec3::X));
    assert_eq!(mesh.check(&TOL), Err(CheckError::Counts));
    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.verts.extend([DVec3::ONE; 9]);
    assert_eq!(mesh.check(&TOL), Err(CheckError::Counts));
}

#[test]
fn unpaired_halfedges_are_caught() {
    let mut mesh = tetrahedron(DVec3::ZERO);
    let pair = mesh.halfedge(0).pair;
    // Paired with the wrong one, or with itself.
    mesh.tris[0].halfedges[0].pair = (pair + 1) % 12;
    assert_eq!(mesh.check(&TOL), Err(CheckError::Pair(0)));
    mesh.tris[0].halfedges[0].pair = 0;
    assert_eq!(mesh.check(&TOL), Err(CheckError::Pair(0)));
}

#[test]
fn loops_are_caught() {
    // Triangles (a, a, b) and (a, b, a), paired up.
    let he = |start, pair, edge| Halfedge { start, pair, edge };
    let tris = vec![
        Tri {
            halfedges: [he(0, 5, 0), he(0, 4, 1), he(1, 3, 2)],
            face: 0,
        },
        Tri {
            halfedges: [he(0, 2, 2), he(1, 1, 1), he(0, 0, 0)],
            face: 0,
        },
    ];
    let edges = vec![Edge::straight(DVec3::ZERO, DVec3::X); 3];
    let mesh = Mesh::from_parts(
        vec![DVec3::ZERO, DVec3::X],
        edges,
        tris,
        vec![face(0, Surface::Free)],
    );
    assert_eq!(mesh.check(&TOL), Err(CheckError::Loop(0)));
}

#[test]
fn repeated_directed_edges_are_caught() {
    // Two copies of a tetrahedron on the same four vertices.
    let one = tetrahedron(DVec3::ZERO);
    let mut tris = one.tris.clone();
    for tri in &one.tris {
        let mut tri = *tri;
        for he in &mut tri.halfedges {
            he.pair += 12;
            he.edge += 6;
        }
        tris.push(tri);
    }
    let edges = [one.edges.clone(), one.edges.clone()].concat();
    let mesh = Mesh::from_parts(one.verts.clone(), edges, tris, one.faces.clone());
    assert_eq!(mesh.check(&TOL), Err(CheckError::DirectedEdge(0)));
}

#[test]
fn pinched_and_unused_vertices_are_caught() {
    // Two tetrahedra sharing vertex 0: two fans there.
    let mut builder = MeshBuilder::new();
    add_tetrahedron(&mut builder, DVec3::ZERO);
    let f = free(&mut builder);
    let v = [DVec3::X, DVec3::Y, DVec3::Z].map(|p| builder.vert(-p));
    for [a, b, c] in [[0, 1, 2], [0, 3, 1], [0, 2, 3]] {
        let id = |k: u32| if k == 0 { 0 } else { v[k as usize - 1] };
        builder.tri([id(a), id(b), id(c)], f);
    }
    builder.tri([v[0], v[2], v[1]], f);
    let mesh = builder.build().unwrap();
    assert_eq!(mesh.check(&TOL), Err(CheckError::Fan(0)));

    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.verts.push(DVec3::ONE);
    assert_eq!(mesh.check(&TOL), Err(CheckError::Fan(4)));
}

// Invariant 2: shared edges.

#[test]
fn edges_not_shared_by_pairs_are_caught() {
    let mut mesh = tetrahedron(DVec3::ZERO);
    let e = mesh.halfedge(0).edge;
    mesh.tris[0].halfedges[0].edge = (e + 1) % 6;
    assert_eq!(mesh.check(&TOL), Err(CheckError::SharedEdge(0)));

    // Both halfedges of one pair moved onto another pair's edge.
    let mut mesh = tetrahedron(DVec3::ZERO);
    let (h, pair) = (0, mesh.halfedge(0).pair);
    let other = (mesh.halfedge(0).edge + 1) % 6;
    mesh.tris[h / 3].halfedges[h % 3].edge = other;
    mesh.tris[pair as usize / 3].halfedges[pair as usize % 3].edge = other;
    assert!(matches!(mesh.check(&TOL), Err(CheckError::EdgeUse(_))));
}

#[test]
fn bad_weights_and_coordinates_are_caught() {
    let mut mesh = round_octahedron(DVec3::ZERO);
    let e = mesh.halfedge(4).edge as usize;
    mesh.edges[e].weight = 100.0;
    assert_eq!(
        mesh.check(&TOL),
        Err(CheckError::Patch(1, PatchError::Weight(100.0)))
    );
    mesh.edges[e].weight = 0.0;
    assert_eq!(
        mesh.check(&TOL),
        Err(CheckError::Patch(1, PatchError::Weight(0.0)))
    );
    let mut mesh = round_octahedron(DVec3::ZERO);
    mesh.verts[5].z = f64::NAN;
    assert!(matches!(
        mesh.check(&TOL),
        Err(CheckError::Patch(4, PatchError::Coordinate(_)))
    ));
}

// Invariant 3: folds.

#[test]
fn folds_are_caught() {
    // Triangle 0 (+x, +y, +z) leaving +x along edge 01 the way its edge 20
    // arrives: its corner normal is zero.
    let ctrl = DVec3::X + (DVec3::new(0.5, 0.0, 0.5) - DVec3::X) * 0.5;
    let mesh = octahedron(UNIT, &OCTAHEDRON, &[(0, 1, ctrl, 1.0)]);
    assert_eq!(mesh.check(&TOL), Err(CheckError::Fold(0)));
}

// Invariant 4: control hulls.

#[test]
fn overlapping_hulls_are_caught() {
    let two = |offset: DVec3| {
        let mut builder = MeshBuilder::new();
        add_tetrahedron(&mut builder, DVec3::ZERO);
        add_tetrahedron(&mut builder, offset);
        builder.build().unwrap()
    };
    assert!(matches!(
        two(DVec3::new(0.3, 0.2, 0.1)).check(&TOL),
        Err(CheckError::Hull(_, _))
    ));
    // Corner to corner across a gap: more than the resolution passes.
    let res = TOL.resolution();
    assert_eq!(two(DVec3::X * (1.0 + 2.0 * res)).check(&TOL), Ok(()));
    assert!(matches!(
        two(DVec3::X * (1.0 + 0.5 * res)).check(&TOL),
        Err(CheckError::Hull(_, _))
    ));
}

/// The octahedron's triangles with (+x, +y, +z) and `second` first, so
/// the hull check meets that pair first.
fn octahedron_starting(second: usize) -> Vec<[u32; 3]> {
    let mut tris = vec![OCTAHEDRON[0], OCTAHEDRON[second]];
    tris.extend((1..8).filter(|&i| i != second).map(|i| OCTAHEDRON[i]));
    tris
}

#[test]
fn folded_edge_neighbours_are_caught() {
    // (-y, +x, +z) folded over onto (+x, +y, +z) across their straight
    // edge, flat on it.
    let mut verts = UNIT;
    verts[2] = DVec3::ZERO;
    verts[4] = DVec3::new(0.5, 0.5, 0.0);
    let mesh = octahedron(verts, &octahedron_starting(3), &[]);
    assert_eq!(mesh.check(&TOL), Err(CheckError::EdgeNeighbours(0, 1)));

    // Their shared edge curved sideways: the plane through its control
    // points has both on one side.
    let ctrl = DVec3::new(0.5, 0.1, 0.5);
    let mesh = octahedron(UNIT, &octahedron_starting(3), &[(0, 2, ctrl, 1.0)]);
    assert_eq!(mesh.check(&TOL), Err(CheckError::EdgeNeighbours(0, 1)));
    // Curved outwards instead, it passes.
    let ctrl = DVec3::new(0.6, 0.0, 0.6);
    let mesh = octahedron(UNIT, &octahedron_starting(3), &[(0, 2, ctrl, 1.0)]);
    assert_eq!(mesh.check(&TOL), Ok(()));
}

#[test]
fn crossing_vertex_neighbours_are_caught() {
    // (-x, -y, +z) stood up through (+x, +y, +z), sharing only +z.
    let verts = [
        DVec3::X,
        DVec3::Y,
        DVec3::ZERO,
        DVec3::new(0.3, 0.3, 1.0),
        DVec3::new(0.3, 0.3, -1.0),
        DVec3::new(2.0, 3.0, 4.0),
    ];
    let mesh = octahedron(verts, &octahedron_starting(1), &[]);
    assert_eq!(mesh.check(&TOL), Err(CheckError::VertexNeighbours(0, 1)));
}

#[test]
fn patches_on_the_same_corners_are_caught() {
    let mut builder = MeshBuilder::new();
    let f = free(&mut builder);
    for p in [DVec3::ZERO, DVec3::X, DVec3::Y] {
        builder.vert(p);
    }
    builder.tri([0, 1, 2], f);
    builder.tri([0, 2, 1], f);
    let mesh = builder.build().unwrap();
    assert_eq!(mesh.check(&TOL), Err(CheckError::SameCorners(0, 1)));
}

// Invariant 5: face tags.

#[test]
fn wrong_face_tags_are_caught() {
    // Face 3 is the side on x = 1, triangles 6 and 7.
    let mut mesh = Mesh::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap();
    mesh.faces[3].surface = Surface::Plane {
        n: DVec3::X,
        d: 1.0 + 2.0 * TOL.resolution(),
    };
    assert_eq!(mesh.check_faces(&TOL), Err(CheckError::Face(6)));
    // Within the resolution it passes.
    mesh.faces[3].surface = Surface::Plane {
        n: DVec3::X * 3.0,
        d: 3.0 * (1.0 + 0.5 * TOL.resolution()),
    };
    assert_eq!(mesh.check_faces(&TOL), Ok(()));
    mesh.faces[3].surface = Surface::Plane {
        n: DVec3::ZERO,
        d: 0.0,
    };
    assert_eq!(mesh.check_faces(&TOL), Err(CheckError::Face(6)));

    let mut mesh = half_cylinder(1.0, DVec3::ZERO);
    let radius = 1.0 + 2.0 * TOL.resolution();
    mesh.faces[3].surface =
        Surface::Quadric(Quadric::cylinder(DVec3::ZERO, DVec3::Z, radius).unwrap());
    assert_eq!(mesh.check_faces(&TOL), Err(CheckError::Face(8)));
    if cfg!(debug_assertions) {
        assert_eq!(mesh.check(&TOL), Err(CheckError::Face(8)));
    }
}

#[test]
fn face_tags_of_any_scale_are_measured() {
    // The side on x = 1 is face 3, triangles 6 and 7. A normal whose
    // length overflows used to make every distance 0, and one whose square
    // underflows every distance infinite.
    let mut mesh = Mesh::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap();
    for scale in [1e-200, 1e-160, 1e160, 1e200, 1e300] {
        mesh.faces[3].surface = Surface::Plane {
            n: DVec3::X * scale,
            d: scale,
        };
        assert_eq!(mesh.check_faces(&TOL), Ok(()), "{scale:e}");
        mesh.faces[3].surface = Surface::Plane {
            n: DVec3::X * scale,
            d: 2.0 * scale,
        };
        assert_eq!(
            mesh.check_faces(&TOL),
            Err(CheckError::Face(6)),
            "{scale:e}"
        );
    }
    // A quadric whose gradient overflows where its value doesn't: about
    // the plane z = 0, which the wall rises half a unit off.
    let mut mesh = half_cylinder(0.5, DVec3::ZERO);
    let Surface::Quadric(mut q) = mesh.faces[3].surface else {
        panic!("the half cylinder's wall");
    };
    q.b = DVec3::Z * 1e308;
    q.c = 0.0;
    mesh.faces[3].surface = Surface::Quadric(q);
    assert!(matches!(mesh.check_faces(&TOL), Err(CheckError::Face(_))));
}

// Order.

#[test]
fn the_first_failure_is_by_invariant_then_index() {
    // A fold in triangle 0 and a coordinate out of bounds in triangle 4:
    // the bounds (invariant 2) come first.
    let ctrl = DVec3::X + (DVec3::new(0.5, 0.0, 0.5) - DVec3::X) * 0.5;
    let mut mesh = octahedron(UNIT, &OCTAHEDRON, &[(0, 1, ctrl, 1.0)]);
    mesh.verts[5].z = f64::INFINITY;
    assert!(matches!(
        mesh.check(&TOL),
        Err(CheckError::Patch(4, PatchError::Coordinate(_)))
    ));
    // Overlapping hulls and a wrong face tag: the hulls (invariant 4),
    // in debug builds as in release.
    let mut builder = MeshBuilder::new();
    add_tetrahedron(&mut builder, DVec3::ZERO);
    add_tetrahedron(&mut builder, DVec3::new(0.3, 0.2, 0.1));
    let mut mesh = builder.build().unwrap();
    mesh.faces[1].surface = Surface::Plane {
        n: DVec3::Z,
        d: 100.0,
    };
    assert!(matches!(mesh.check(&TOL), Err(CheckError::Hull(..))));
}

// Determinism.

#[test]
fn checks_are_the_same_on_any_thread_count() {
    let mesh = torus(64, 32, 10.0, 4.0);
    let (result, pairs) = assert_deterministic(|| {
        let bvh = Bvh::new(
            (0..mesh.tris().len())
                .map(|t| mesh.patch(t).bounds())
                .collect(),
        );
        (mesh.check(&TOL), bvh.self_pairs(TOL.resolution()))
    });
    assert_eq!(result, Ok(()));
    assert!(pairs.len() > 4 * mesh.tris().len());

    // Jittered until it fails: the same failure, found first either way.
    let mut rng = Rng::new(42);
    let mut mesh = torus(64, 32, 10.0, 4.0);
    for v in &mut mesh.verts {
        *v += rng.point(0.4);
    }
    let result = assert_deterministic(|| mesh.check(&TOL));
    assert!(result.is_err());
}
