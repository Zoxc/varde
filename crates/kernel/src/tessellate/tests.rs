#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_1_SQRT_2, PI};

use glam::{DVec3, Vec3};

use super::*;
use crate::mesh::MeshBuilder;
use crate::mesh::tests::{
    TOL, add_round_octahedron, half_cylinder, round_octahedron, tetrahedron, torus,
};
use crate::par::assert_deterministic;
use crate::{Budget, KernelError, Solid};

fn draw(mesh: Mesh) -> RenderMesh {
    let solid = Solid::new(mesh, &TOL).unwrap();
    solid.tessellate(&Display::default()).unwrap()
}

fn bits(p: [f32; 3]) -> [u32; 3] {
    p.map(f32::to_bits)
}

/// Every triangle side is met by another running the other way between
/// the same two positions, to the bit: no cracks, one orientation. And no
/// triangle has two corners at one position.
fn assert_watertight(mesh: &RenderMesh) {
    let p = mesh.positions();
    let mut sides: BTreeMap<([u32; 3], [u32; 3]), i64> = BTreeMap::new();
    for tri in mesh.indices().chunks(3) {
        let q = [0, 1, 2].map(|i| bits(p[tri[i] as usize]));
        assert!(q[0] != q[1] && q[1] != q[2] && q[2] != q[0], "{q:?}");
        for i in 0..3 {
            *sides.entry((q[i], q[(i + 1) % 3])).or_default() += 1;
        }
    }
    for (&(a, b), &n) in &sides {
        assert_eq!(sides.get(&(b, a)), Some(&n), "side {a:?} -> {b:?}");
    }
}

/// How many segments the feature edges have together.
fn edge_segments(mesh: &RenderMesh) -> usize {
    mesh.edge_vertices().len() - mesh.edge_count()
}

/// The positions of `vertices` of `mesh`, to the bit.
fn bits_of(mesh: &RenderMesh, vertices: &[u32]) -> Vec<[u32; 3]> {
    (vertices.iter())
        .map(|&v| bits(mesh.positions()[v as usize]))
        .collect()
}

/// Each edge runs along the vertices of its first face and the positions
/// of its second, starts and ends at its corners, and every corner ends
/// an edge (as `from_parts` checks too); a face's triangles all face the
/// same way where it's flat. Returns how many edge ends each corner is.
fn assert_faces_and_edges(mesh: &RenderMesh) -> Vec<usize> {
    let faces: Vec<&[u32]> = mesh.faces().collect();
    assert_eq!(faces.len(), mesh.face_count());
    let mut ends = vec![0; mesh.corners().len()];
    for ((polyline, &[a, b]), &[start, end]) in mesh
        .polylines()
        .zip(mesh.edge_faces())
        .zip(mesh.edge_corners())
    {
        let on_a = faces[a as usize];
        assert!(polyline.iter().all(|v| on_a.contains(v)));
        let on_b = bits_of(mesh, faces[b as usize]);
        assert!(bits_of(mesh, polyline).iter().all(|p| on_b.contains(p)));
        let at = |c: u32| bits(mesh.corners()[c as usize]);
        assert_eq!(bits_of(mesh, &polyline[..1])[0], at(start));
        assert_eq!(bits_of(mesh, &polyline[polyline.len() - 1..])[0], at(end));
        ends[start as usize] += 1;
        ends[end as usize] += 1;
    }
    assert!(ends.iter().all(|&n| n > 0));
    ends
}

/// Normals are unit length, and the mesh encloses a positive volume,
/// which it returns.
fn assert_normals_and_volume(mesh: &RenderMesh) -> f64 {
    for n in mesh.normals() {
        let n = Vec3::from(*n);
        assert!(n.is_finite() && (n.length() - 1.0).abs() < 1e-6, "{n}");
    }
    let p = |i: u32| Vec3::from(mesh.positions()[i as usize]).as_dvec3();
    let volume: f64 = mesh
        .indices()
        .chunks(3)
        .map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0)
        .sum();
    assert!(volume > 0.0);
    volume
}

#[test]
fn constants_are_the_angles() {
    let turn = Display::MAX_TURN_DEGREES.to_radians();
    assert!((TURN - turn).abs() < 1e-15);
    assert!((COS_TURN - turn.cos()).abs() < 1e-15);
    assert!((COS_SMOOTH - Display::SMOOTH_DEGREES.to_radians().cos()).abs() < 1e-15);
}

#[test]
fn the_chord_follows_the_size() {
    let display = Display::default();
    assert_eq!(display.chord(0.0), 1e-3);
    assert_eq!(display.chord(1.0), 1e-3);
    assert_eq!(display.chord(1e3), 1.0);
    let coarse = Display::new(&Tolerance::new(0.1).unwrap());
    assert_eq!(coarse.chord(10.0), 0.1);
}

/// The farthest `curve` gets from the chords of its `n` equal steps, by
/// dense sampling, and the most its tangent turns along one, in degrees.
fn deviation(curve: &Conic3, n: u32) -> (f64, f64) {
    let (mut far, mut turn) = (0.0f64, 0.0f64);
    for s in 0..n {
        let (t0, t1) = (f64::from(s) / f64::from(n), f64::from(s + 1) / f64::from(n));
        let (a, b) = (curve.eval(t0), curve.eval(t1));
        for k in 1..64 {
            let x = curve.eval(t0 + (t1 - t0) * f64::from(k) / 64.0);
            far = far.max(distance_to_line(x, a, b));
        }
        let da = curve.eval_deriv(t0).1.normalize();
        let db = curve.eval_deriv(t1).1.normalize();
        turn = turn.max(da.angle_between(db).to_degrees());
    }
    (far, turn)
}

#[test]
fn edges_get_enough_segments() {
    let straight = Conic3::line(DVec3::ZERO, DVec3::new(100.0, 3.0, -2.0)).unwrap();
    assert_eq!(segments(&straight, 1e-3), 1);

    for radius in [1e-3, 1.0, 10.0, 1e4] {
        let quarter = Conic3::new(
            DVec3::X * radius,
            DVec3::new(1.0, 1.0, 0.0) * radius,
            FRAC_1_SQRT_2,
            DVec3::Y * radius,
        )
        .unwrap();
        for chord in [1e-3, 1e-2, 1.0] {
            let n = segments(&quarter, chord);
            let (far, turn) = deviation(&quarter, n);
            // The count follows the middles of the steps; the curve's
            // farthest point is close by.
            assert!(
                n == 64 || far <= chord * 1.05,
                "{radius} {chord}: {n} {far}"
            );
            assert!(
                n == 64 || turn <= 10.0 + 1e-9,
                "{radius} {chord}: {n} {turn}"
            );
            // Not many more than needed: one fewer would do neither.
            if n > 1 && n < 64 {
                let (far, turn) = deviation(&quarter, n - 1);
                assert!(far > chord * 0.8 || turn > 9.0, "{radius} {chord}: {n}");
            }
            // A quarter circle turns 90°.
            assert!(n >= 9, "{n}");
        }
    }

    // Nearly the control polygon: the most segments, no more.
    let sharp = Conic3::new(DVec3::X, DVec3::ZERO, 64.0, DVec3::Y).unwrap();
    assert_eq!(segments(&sharp, 1e-6), Display::MAX_SEGMENTS);
}

#[test]
fn levels_count_their_points_and_triangles() {
    let single = Level::new([1, 1, 1]);
    assert_eq!(
        (single.inner, single.inner_points(), single.triangles()),
        (None, 0, 1)
    );
    // A curved edge gets a centre to fan from.
    let fan = Level::new([2, 1, 1]);
    assert_eq!(
        (fan.inner, fan.inner_points(), fan.triangles()),
        (Some(0), 1, 4)
    );
    let full = Level::new([64, 64, 64]);
    assert_eq!(full.inner, Some(61));
    assert_eq!(full.inner_points(), 62 * 63 / 2);
    assert_eq!(full.triangles(), 64 * 64);
    // Row by row along `k`.
    let l = 4;
    let mut next = 0;
    for k in 0..=l {
        for j in 0..=l - k {
            assert_eq!(Level::index(l, j, k), next);
            next += 1;
        }
    }
}

#[test]
fn stitching_joins_any_two_counts() {
    for a in 1..8usize {
        for b in 0..8usize {
            let along =
                |i: u32, n: usize, y: f64| DVec3::new(f64::from(i) / n.max(1) as f64, y, 0.0);
            let outer: Vec<(u32, DVec3)> = (0..=a as u32).map(|i| (i, along(i, a, 0.0))).collect();
            let inner: Vec<(u32, DVec3)> = (0..=b as u32)
                .map(|i| (100 + i, along(i, b, 0.3) * 0.8 + DVec3::X * 0.1))
                .collect();
            let mut out = Vec::new();
            stitch(&mut out, &outer, &inner);
            assert_eq!(out.len(), 3 * (a + b));
            // Each outer segment once, forwards; each inner one once,
            // backwards.
            for s in 0..a as u32 {
                let n = out.chunks(3).filter(|t| t[0] == s && t[1] == s + 1).count();
                assert_eq!(n, 1);
            }
            for t in 100..100 + b as u32 {
                let n = out.chunks(3).filter(|x| x[1] == t + 1 && x[2] == t).count();
                assert_eq!(n, 1);
            }
        }
    }
}

#[test]
fn a_cylinder_is_drawn_within_the_chord() {
    let (radius, height) = (3.0, 5.0);
    let base = DVec3::new(1.0, -2.0, 0.5);
    let solid = Solid::cylinder(base, radius, height, 7, &TOL).unwrap();
    let display = Display::default();
    let mesh = solid.tessellate(&display).unwrap();
    assert_watertight(&mesh);
    let volume = assert_normals_and_volume(&mesh);
    let exact = PI * radius * radius * height;
    assert!(volume < exact && volume > exact * 0.99, "{volume} {exact}");

    let diagonal = DVec3::new(2.0 * radius, 2.0 * radius, height).length();
    let chord = display.chord(diagonal);
    let p = |i: u32| Vec3::from(mesh.positions()[i as usize]).as_dvec3();
    let off_axis = |x: DVec3| (x - base).truncate().length();
    for (i, n) in mesh.normals().iter().enumerate() {
        let (x, n) = (p(i as u32), Vec3::from(*n).as_dvec3());
        if n.z.abs() > 0.5 {
            // A cap: flat, facing out.
            assert_eq!(n, DVec3::Z * n.z.signum());
            let cap = if n.z > 0.0 { base.z + height } else { base.z };
            assert!((x.z - cap).abs() < 1e-5);
        } else {
            // The wall: radial.
            let radial = (x - base).truncate().normalize().extend(0.0);
            assert!(n.dot(radial) > 1.0 - 1e-6, "{n} at {x}");
            assert!((off_axis(x) - radius).abs() < 1e-5);
        }
    }
    // Every triangle's centre within the chord of the surface, but at the
    // corners of the walls' skewed patches, where the inner grid is two
    // steps round the arc from the corner: there about twice (1.85).
    let mut worst = 0.0f64;
    for t in mesh.indices().chunks(3) {
        let centre = (p(t[0]) + p(t[1]) + p(t[2])) / 3.0;
        let on_cap = [base.z, base.z + height]
            .iter()
            .any(|z| (centre.z - z).abs() < 1e-5);
        if !on_cap {
            worst = worst.max((off_axis(centre) - radius).abs() / chord);
        }
    }
    assert!(worst <= 2.5, "{worst}");

    // The two rims are the feature edges, and the walls' seams aren't:
    // each rim one edge round, closing on one corner, between the wall
    // and a cap.
    let quarter = Conic3::new(
        base + DVec3::X * radius,
        base + DVec3::new(1.0, 1.0, 0.0) * radius,
        FRAC_1_SQRT_2,
        base + DVec3::Y * radius,
    )
    .unwrap();
    let n = segments(&quarter, chord);
    assert_eq!(mesh.face_count(), 3);
    assert_eq!(mesh.edge_count(), 2);
    assert_eq!(mesh.corners().len(), 2);
    assert_eq!(assert_faces_and_edges(&mesh), [2, 2]);
    for (polyline, faces) in mesh.polylines().zip(mesh.edge_faces()) {
        assert_eq!(polyline.len(), 4 * n as usize + 1);
        assert_ne!(faces[0], faces[1]);
        let z = p(polyline[0]).z;
        for &v in polyline {
            let x = p(v);
            assert!((off_axis(x) - radius).abs() < 1e-5);
            assert_eq!(x.z, z);
        }
    }
    assert_eq!(mesh.edge_corners(), [[0, 0], [1, 1]]);
    // A cap's triangles are all its own, flat.
    for face in mesh.faces() {
        let normals: Vec<[f32; 3]> = (face.iter()).map(|&v| mesh.normals()[v as usize]).collect();
        let flat = normals.iter().all(|&n| n == normals[0]);
        assert_eq!(flat, normals[0][2].abs() == 1.0);
    }
    // A rim point is one vertex for the smooth wall and one for the cap.
    let rim = (base + DVec3::X * radius).as_vec3().to_array();
    let at_rim = mesh.positions().iter().filter(|&&x| x == rim).count();
    assert_eq!(at_rim, 2);
}

#[test]
fn curved_solids_are_watertight() {
    let offset = DVec3::new(3e5, -2e5, 1e5);
    for mesh in [
        round_octahedron(DVec3::ZERO),
        round_octahedron(offset),
        half_cylinder(4.0, DVec3::ZERO),
        half_cylinder(0.01, DVec3::new(10.0, 0.0, 0.0)),
        torus(24, 12, 3.0, 1.0),
        Mesh::cuboid(DVec3::splat(-1.0), DVec3::new(2.0, 3.0, 4.0), 1, &TOL).unwrap(),
    ] {
        let mesh = draw(mesh);
        assert_watertight(&mesh);
        assert_normals_and_volume(&mesh);
    }
}

#[test]
fn a_round_octahedron_is_smooth_where_its_patches_meet_smoothly() {
    let mesh = draw(round_octahedron(DVec3::ZERO));
    // Its normals point out, close to the sphere's.
    for (x, n) in mesh.positions().iter().zip(mesh.normals()) {
        let (x, n) = (Vec3::from(*x), Vec3::from(*n));
        assert!(x.normalize().dot(n) > 0.9, "{x} {n}");
    }
    // Twelve quarter circles, each drawn once.
    let n = segments(
        &Conic3::new(DVec3::X, DVec3::new(1.0, 1.0, 0.0), FRAC_1_SQRT_2, DVec3::Y).unwrap(),
        Display::default().chord(2.0 * 3f64.sqrt()),
    );
    assert_eq!(edge_segments(&mesh), 12 * n as usize);
    // One face creased along each quarter circle, four meeting at each
    // of six corners.
    assert_eq!(mesh.face_count(), 1);
    assert_eq!(mesh.edge_count(), 12);
    assert!(mesh.edge_faces().iter().all(|&faces| faces == [0, 0]));
    assert_eq!(assert_faces_and_edges(&mesh), [4; 6]);
}

#[test]
fn a_flat_torus_splits_its_normals() {
    let mesh = draw(torus(24, 12, 3.0, 1.0));
    // Its quads are flat (isosceles trapezoids), and the edges between
    // them fold by 15° or 30°: four vertices at each of its six-corner
    // vertices, and the quads' sides drawn but not their diagonals.
    assert_eq!(mesh.positions().len(), 24 * 12 * 4);
    assert_eq!(mesh.triangle_count(), 24 * 12 * 2);
    assert_eq!(edge_segments(&mesh), 24 * 12 * 2);
    assert_faces_and_edges(&mesh);
}

#[test]
fn a_box_has_six_faces_twelve_edges_and_eight_corners() {
    let (min, max) = (DVec3::new(-1.0, 0.5, 2.0), DVec3::new(3.0, 1.5, 4.0));
    let mesh = Solid::new(Mesh::cuboid(min, max - min, 1, &TOL).unwrap(), &TOL)
        .unwrap()
        .tessellate(&Display::default())
        .unwrap();
    assert_eq!(mesh.face_count(), 6);
    assert_eq!(mesh.face_ends(), [6, 12, 18, 24, 30, 36]);
    // Each face one flat side.
    for face in mesh.faces() {
        let n = mesh.normals()[face[0] as usize];
        assert!(face.iter().all(|&v| mesh.normals()[v as usize] == n));
    }
    assert_eq!(mesh.edge_count(), 12);
    for (polyline, faces) in mesh.polylines().zip(mesh.edge_faces()) {
        assert_eq!(polyline.len(), 2);
        assert_ne!(faces[0], faces[1]);
    }
    assert_eq!(assert_faces_and_edges(&mesh), [3; 8]);
    let mut corners: Vec<[u32; 3]> = mesh.corners().iter().map(|&c| bits(c)).collect();
    corners.sort_unstable();
    let mut expected: Vec<[u32; 3]> = (0..8)
        .map(|i| {
            let pick = |bit: usize, axis: usize| {
                if i >> bit & 1 == 0 {
                    min[axis]
                } else {
                    max[axis]
                }
            };
            bits([0, 1, 2].map(|axis| pick(axis, axis) as f32))
        })
        .collect();
    expected.sort_unstable();
    assert_eq!(corners, expected);
    assert_eq!(mesh.part_ends(), [[6, 12, 8]]);
}

#[test]
fn chained_edges_pass_through_the_vertices_between_them() {
    // A half cylinder: its flat side meets each cap along a line, the
    // round wall along two quarter circles chained into one.
    let mesh = draw(half_cylinder(4.0, DVec3::ZERO));
    let ends = assert_faces_and_edges(&mesh);
    // Each corner of the flat side is where three edges meet.
    assert_eq!(ends, [3; 4]);
    assert_eq!(mesh.edge_count(), 6);
}

#[test]
fn a_refined_mesh_is_watertight() {
    // Two shells close together: repair splits them unevenly, and the
    // pieces' edges get different counts.
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, DVec3::ZERO, 10.0, false);
    add_round_octahedron(&mut builder, DVec3::new(0.0, 0.0, 0.2), 9.7, true);
    let mesh = builder.build().unwrap();
    let repaired = mesh.repair(&TOL, &Budget::default()).unwrap();
    assert!(repaired.tris().len() > 16);
    let fine = Display::new(&Tolerance::new(1e-5).unwrap());
    let solid = Solid::new(repaired, &TOL).unwrap();
    for display in [Display::default(), fine] {
        let drawn = assert_deterministic(|| solid.tessellate(&display).unwrap());
        assert_watertight(&drawn);
        for n in drawn.normals() {
            assert!(Vec3::from(*n).is_finite());
        }
    }
}

#[test]
fn positions_past_the_limit_are_refused() {
    let far = Solid::new(tetrahedron(DVec3::splat(5e6)), &TOL).unwrap();
    assert_eq!(
        far.tessellate(&Display::default()),
        Err(MeshError::Values(crate::MeshPart::Positions))
    );
    // Not an error of the kernel's own.
    assert!(!matches!(
        Solid::new(tetrahedron(DVec3::splat(5e6)), &TOL),
        Err(KernelError::Invalid(_))
    ));
}

#[test]
fn tessellation_is_deterministic() {
    let solid = Solid::new(torus(48, 24, 3.0, 1.0), &TOL).unwrap();
    assert_deterministic(|| solid.tessellate(&Display::default()).unwrap());
    let solid = Solid::cylinder(DVec3::ZERO, 2.0, 1.0, 1, &TOL).unwrap();
    assert_deterministic(|| solid.tessellate(&Display::default()).unwrap());
}

/// The size checks, against limits a small mesh reaches: each part may
/// be as large as its limit and no larger. The real limits take millions
/// of patches to reach.
#[test]
fn a_mesh_past_any_limit_is_too_large() {
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, DVec3::ZERO, 10.0, false);
    add_round_octahedron(&mut builder, DVec3::new(0.0, 0.0, 0.2), 9.7, true);
    let refined = builder
        .build()
        .unwrap()
        .repair(&TOL, &Budget::default())
        .unwrap();
    let display = Display::default();
    let (_, part_torus) = round_solids().swap_remove(3);
    for mesh in [
        Mesh::cylinder(DVec3::ZERO, 3.0, 5.0, 1, &TOL).unwrap(),
        // Refined inner grids: the same at the exact limits.
        part_torus.into_mesh(),
        // Split corners: more vertices than points.
        torus(24, 12, 3.0, 1.0),
        refined,
    ] {
        let full = tessellate(&mesh, &display).unwrap();
        let exact = Limits {
            vertices: full.positions().len() as u64,
            indices: full.indices().len() as u64,
            edge_points: full.edge_vertices().len() as u64,
        };
        assert!(exact.edge_points > 0);
        assert_eq!(
            tessellate_within(&mesh, &display, &exact).as_ref(),
            Ok(&full)
        );
        for tight in [
            Limits {
                vertices: exact.vertices - 1,
                ..exact
            },
            Limits {
                indices: exact.indices - 1,
                ..exact
            },
            Limits {
                edge_points: exact.edge_points - 1,
                ..exact
            },
        ] {
            assert_eq!(
                tessellate_within(&mesh, &display, &tight),
                Err(MeshError::TooLarge),
                "{tight:?} of {exact:?}"
            );
        }
    }
}

/// A solid smaller than an `f32` step where it is loses its shape to
/// rounding, but no cracks open: the triangles that keep three distinct
/// corners still meet side to side, to the bit, as the samples each side
/// reads are the same. (A collapsed triangle's two other sides cancel.)
#[test]
fn a_tiny_solid_far_out_opens_no_cracks() {
    let solid = Solid::cylinder(DVec3::new(9e5, -9e5, 0.0), 0.01, 0.01, 1, &TOL).unwrap();
    let mesh = solid.tessellate(&Display::default()).unwrap();
    let p = mesh.positions();
    let mut sides: BTreeMap<([u32; 3], [u32; 3]), i64> = BTreeMap::new();
    let mut collapsed = 0;
    for tri in mesh.indices().chunks(3) {
        let q = [0, 1, 2].map(|i| bits(p[tri[i] as usize]));
        if q[0] == q[1] || q[1] == q[2] || q[2] == q[0] {
            collapsed += 1;
            continue;
        }
        for i in 0..3 {
            *sides.entry((q[i], q[(i + 1) % 3])).or_default() += 1;
        }
    }
    assert!(collapsed > 0);
    for (&(a, b), &n) in &sides {
        assert_eq!(sides.get(&(b, a)), Some(&n), "side {a:?} -> {b:?}");
    }
    for n in mesh.normals() {
        assert!((Vec3::from(*n).length() - 1.0).abs() < 1e-6);
    }
}

/// Checks that `drawn`, `solid` tessellated with its `topology`, has the
/// topology's regions for faces and its chains for first edges: one face
/// per region, every triangle's corners on its region's surface (its
/// first triangle's face's form: a region's faces share one surface);
/// edge `c` of chain `c` between the chain's regions, its vertices on
/// both of their surfaces, closed where the chain is; the edges after the
/// chains creases, one face either side. Positions are `f32`, so within
/// `within`.
fn assert_regions_and_chains(solid: &Solid, topology: &Topology, drawn: &RenderMesh, within: f64) {
    let mesh = solid.mesh();
    let form = |r: u32| {
        let t = topology.regions()[r as usize].tris[0];
        mesh.faces()[mesh.tris()[t as usize].face as usize].form
    };
    let at = |v: u32| Vec3::from(drawn.positions()[v as usize]).as_dvec3();
    assert_eq!(drawn.face_count(), topology.regions().len());
    for (r, face) in drawn.faces().enumerate() {
        for &v in face {
            let d = form(r as u32).distance(at(v));
            assert!(d <= within, "region {r}: {:?} is {d} off", at(v));
        }
    }
    let chains = topology.chains();
    assert!(drawn.edge_count() >= chains.len());
    for (c, (polyline, faces)) in drawn.polylines().zip(drawn.edge_faces()).enumerate() {
        let Some(chain) = chains.get(c) else {
            assert_eq!(faces[0], faces[1], "edge {c} is a crease");
            continue;
        };
        assert_eq!(*faces, chain.regions);
        for r in chain.regions {
            for &v in polyline {
                let d = form(r).distance(at(v));
                assert!(d <= within, "chain {c}, region {r}: {:?} is {d} off", at(v));
            }
        }
        if chain.closed {
            assert_eq!(polyline.first(), polyline.last());
            let [start, end] = drawn.edge_corners()[c];
            assert_eq!(start, end);
        }
    }
}

#[test]
fn a_box_s_faces_and_edges_are_its_regions_and_chains() {
    let solid = Solid::cuboid(DVec3::ZERO, DVec3::new(1.0, 2.0, 3.0), 1, &TOL).unwrap();
    let topology = solid.topology();
    let drawn = solid
        .tessellate_with(&Display::default(), &topology)
        .unwrap();
    assert_regions_and_chains(&solid, &topology, &drawn, 1e-6);
    // No creases.
    assert_eq!(drawn.edge_count(), 12);
}

#[test]
fn a_cylinder_s_quarter_walls_are_one_face_and_its_rims_two_edges() {
    let solid = Solid::cylinder(DVec3::ZERO, 2.0, 1.0, 1, &TOL).unwrap();
    let topology = solid.topology();
    let drawn = solid
        .tessellate_with(&Display::default(), &topology)
        .unwrap();
    assert_regions_and_chains(&solid, &topology, &drawn, 1e-5);
    assert_eq!(topology.regions().len(), 3);
    assert_eq!(topology.chains().len(), 2);
    assert!(topology.chains().iter().all(|c| c.closed));
    assert_eq!(drawn.edge_count(), 2);
}

#[test]
fn flush_faces_merged_under_one_name_are_one_face() {
    // Two boxes of different features side by side: their tops, bottoms,
    // fronts and backs meet flush and take one name each, so the union
    // draws as one box of six faces and twelve edges.
    let tol = &TOL;
    let left = Solid::cuboid(DVec3::ZERO, DVec3::new(1.0, 1.0, 1.0), 1, tol).unwrap();
    let right =
        Solid::cuboid(DVec3::new(1.0, 0.0, 0.0), DVec3::new(2.0, 1.0, 1.0), 2, tol).unwrap();
    let solid = crate::boolean(&left, &right, crate::Op::Union, tol, &Budget::DEFAULT).unwrap();
    let topology = solid.topology();
    let drawn = solid
        .tessellate_with(&Display::default(), &topology)
        .unwrap();
    assert_regions_and_chains(&solid, &topology, &drawn, 1e-6);
    assert_eq!(topology.regions().len(), 6);
    assert_eq!(topology.chains().len(), 12);
    assert_eq!(drawn.edge_count(), 12);
    // The top has pieces of both boxes' faces, all one region.
    let mesh = solid.mesh();
    let top: Vec<usize> = (drawn.faces().enumerate())
        .filter(|(_, face)| {
            face.iter()
                .all(|&v| drawn.positions()[v as usize][2] == 1.0)
        })
        .map(|(r, face)| {
            assert!(face.len() >= 12);
            r
        })
        .collect();
    let [top] = top[..] else {
        panic!("one face on top: {top:?}");
    };
    let faces: std::collections::BTreeSet<u32> = (topology.regions()[top].tris)
        .iter()
        .map(|&t| mesh.tris()[t as usize].face)
        .collect();
    assert!(faces.len() >= 2, "the top's region joins both boxes' faces");
}

#[test]
fn a_crease_inside_one_face_is_on_no_chain() {
    // A torus of flat triangles, one face with no claim: its creases are
    // drawn but border no other face.
    let solid = Solid::new(torus(24, 12, 3.0, 1.0), &TOL).unwrap();
    let topology = solid.topology();
    let drawn = solid
        .tessellate_with(&Display::default(), &topology)
        .unwrap();
    assert_regions_and_chains(&solid, &topology, &drawn, 0.0);
    assert!(topology.chains().is_empty());
    assert_eq!(drawn.face_count(), 1);
    assert!(drawn.edge_count() > 0);
    assert!(drawn.edge_faces().iter().all(|&faces| faces == [0, 0]));
}

#[test]
fn drawing_with_the_topology_is_deterministic() {
    let left = Solid::cuboid(DVec3::ZERO, DVec3::new(10.0, 4.0, 2.0), 1, &TOL).unwrap();
    let hole = Solid::cylinder(DVec3::new(5.0, 2.0, -1.0), 1.0, 4.0, 2, &TOL).unwrap();
    let solid =
        crate::boolean(&left, &hole, crate::Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    let topology = solid.topology();
    let drawn = assert_deterministic(|| {
        solid
            .tessellate_with(&Display::default(), &topology)
            .unwrap()
    });
    assert_regions_and_chains(&solid, &topology, &drawn, 1e-5);
}

/// One triangle of a tessellation in its patch: the patch's index, and
/// its corners' barycentric parameters and points.
struct InPatch {
    t: usize,
    params: [DVec3; 3],
    points: [DVec3; 3],
}

/// The triangles `plan` makes of each patch, as the drawing does (the
/// strips choosing from points rounded to `f32`), with their parameters,
/// and the points in `f64`.
fn in_patches(plan: &Plan) -> Vec<InPatch> {
    let mut out = Vec::new();
    for &t in &plan.tri_ids {
        let patch = plan.mesh.patch(t as usize);
        let level = &plan.levels[t as usize];
        let mut params = Vec::new();
        let mut first = [0u32; 3];
        for (i, &n) in level.counts.iter().enumerate() {
            first[i] = params.len() as u32;
            for r in 0..=n {
                let s = f64::from(r) / f64::from(n);
                let mut u = DVec3::ZERO;
                u[i] = 1.0 - s;
                u[(i + 1) % 3] = s;
                params.push(u);
            }
        }
        let base = params.len() as u32;
        params.extend(level.inner_params());
        let points: Vec<DVec3> = params.iter().map(|&u| patch.eval(u)).collect();
        let rounded: Vec<DVec3> = (points.iter()).map(|p| p.as_vec3().as_dvec3()).collect();
        let indices = level.triangulate(base, &rounded[base as usize..], |i, r| {
            let v = first[i as usize] + r;
            (v, rounded[v as usize])
        });
        for tri in indices.as_chunks::<3>().0 {
            out.push(InPatch {
                t: t as usize,
                params: tri.map(|v| params[v as usize]),
                points: tri.map(|v| points[v as usize]),
            });
        }
    }
    out
}

/// The farthest `tri` gets from its patch, along the patch's normal, by
/// dense sampling: the points of the 12-step barycentric grid of the
/// triangle against the patch at the same mix of the corners' parameters.
fn off_patch(mesh: &Mesh, tri: &InPatch) -> f64 {
    let patch = mesh.patch(tri.t);
    let steps = 12;
    let mut far = 0.0f64;
    for a in 0..=steps {
        for b in 0..=steps - a {
            let l =
                DVec3::new(f64::from(a), f64::from(b), f64::from(steps - a - b)) / f64::from(steps);
            let x = tri.points[0] * l.x + tri.points[1] * l.y + tri.points[2] * l.z;
            let u = tri.params[0] * l.x + tri.params[1] * l.y + tri.params[2] * l.z;
            let n = unit_normal(&patch, u);
            far = far.max((x - patch.eval(u)).dot(n).abs());
        }
    }
    far
}

/// What a tessellation of `mesh` is like, before and after the inner
/// grids are refined: the triangles, and the farthest a triangle of a
/// patch curved both ways ([`curved_both_ways`]) gets from it, over the
/// chord.
#[derive(Debug, PartialEq)]
struct Drawn {
    triangles: (u64, u64),
    worst: (f64, f64),
}

fn drawn(mesh: &Mesh, display: &Display) -> Drawn {
    let plan = Plan::new(mesh, display, &Limits::RENDER).unwrap().unwrap();
    let bounds = Bounds3::around(mesh.verts()).unwrap();
    let chord = display.chord((bounds.max - bounds.min).length());
    let form = |t: usize| &mesh.faces()[mesh.tris()[t].face as usize].form;
    let worst = |plan: &Plan| {
        (in_patches(plan).iter())
            .filter(|tri| curved_both_ways(form(tri.t)))
            .map(|tri| off_patch(mesh, tri) / chord)
            .fold(0.0, f64::max)
    };
    let mut before = Plan::new(mesh, display, &Limits::RENDER).unwrap().unwrap();
    before.levels = (before.levels.iter())
        .map(|level| Level::new(level.counts))
        .collect();
    before.count();
    Drawn {
        triangles: (before.triangles, plan.triangles),
        worst: (worst(&before), worst(&plan)),
    }
}

/// Whether every patch's triangles, in its parameters, turn the same way
/// as the patch and cover it once: positive areas summing to the
/// domain's.
fn assert_tiled(mesh: &Mesh, display: &Display) {
    let plan = Plan::new(mesh, display, &Limits::RENDER).unwrap().unwrap();
    let mut area = vec![0.0f64; plan.tri_ids.len()];
    for tri in in_patches(&plan) {
        let [a, b, c] = tri.params.map(|u| glam::DVec2::new(u.y, u.z));
        let twice = (b - a).perp_dot(c - a);
        assert!(twice > 0.0, "patch {}: {:?}", tri.t, tri.params);
        area[tri.t] += 0.5 * twice;
    }
    for (t, a) in area.iter().enumerate() {
        assert!((a - 0.5).abs() < 1e-12, "patch {t}: {a}");
    }
}

/// The revolved solids curved both ways: a ball, a hollow ball, a torus,
/// the outside of a spindle torus and an ellipse turned (fitted bands),
/// and a part turn of a torus.
fn round_solids() -> Vec<(&'static str, Solid)> {
    use crate::extrude::Frame;
    use crate::profile::tests::{arc, circle};
    use crate::{Loop, Profile, Segment, Sweep, revolve};
    let v = glam::DVec2::new;
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let z = Frame {
        origin: DVec3::new(1.0, -2.0, 0.5),
        x: DVec3::X,
        y: DVec3::Z,
    };
    let ball = Loop {
        segments: vec![
            arc(v(0.0, 0.0), v(0.0, -2.0), v(2.0, 0.0), 1),
            arc(v(0.0, 0.0), v(2.0, 0.0), v(0.0, 2.0), 1),
            line(v(0.0, 2.0), v(0.0, -2.0), 2),
        ],
    };
    let shell = Loop {
        segments: vec![
            arc(v(0.0, 0.0), v(0.0, -3.0), v(3.0, 0.0), 1),
            arc(v(0.0, 0.0), v(3.0, 0.0), v(0.0, 3.0), 1),
            line(v(0.0, 3.0), v(0.0, 2.0), 2),
            arc(v(0.0, 0.0), v(0.0, 2.0), v(2.0, 0.0), 3),
            arc(v(0.0, 0.0), v(2.0, 0.0), v(0.0, -2.0), 3),
            line(v(0.0, -2.0), v(0.0, -3.0), 4),
        ],
    };
    let (c, r) = (v(2.0, 0.0), 3.0);
    let at = |a: f64| c + glam::DVec2::new(a.cos(), a.sin()) * r;
    let (low, high) = (at(-PI / 3.0), at(PI / 3.0));
    let spindle = Loop {
        segments: vec![
            arc(c, low, v(5.0, 0.0), 1),
            arc(c, v(5.0, 0.0), high, 1),
            line(high, v(3.0, 0.0), 2),
            line(v(3.0, 0.0), low, 3),
        ],
    };
    let ellipse = Loop {
        segments: vec![
            line(v(0.0, 0.0), v(3.0, 0.0), 1),
            Segment {
                conic: crate::patch::Conic2::new(
                    v(3.0, 0.0),
                    v(3.0, 2.0),
                    FRAC_1_SQRT_2,
                    v(0.0, 4.0),
                )
                .unwrap(),
                curve: 2,
            },
            line(v(0.0, 4.0), v(0.0, 0.0), 3),
        ],
    };
    let torus = || vec![circle(v(10.0, 0.0), 2.0, 1, false)];
    let turn = |loops: Vec<Loop>, sweep: Sweep| {
        revolve(&Profile { loops }, &z, sweep, 7, &TOL, &Budget::DEFAULT).unwrap()
    };
    let part = Sweep::Part {
        from: -0.5,
        to: 2.0,
    };
    vec![
        ("ball", turn(vec![ball], Sweep::Full)),
        ("hollow ball", turn(vec![shell], Sweep::Full)),
        ("torus", turn(torus(), Sweep::Full)),
        ("part torus", turn(torus(), part)),
        ("spindle", turn(vec![spindle], Sweep::Full)),
        ("ellipse", turn(vec![ellipse], Sweep::Full)),
    ]
}

#[test]
fn doubly_curved_patches_are_drawn_within_the_chord() {
    // Coarse enough for the fit tolerance to set the chord.
    let coarse = Display::new(&Tolerance::new(0.05).unwrap());
    let mut solids = round_solids();
    solids.push((
        "round octahedron",
        Solid::new(round_octahedron(DVec3::ZERO), &TOL).unwrap(),
    ));
    for (name, solid) in &solids {
        for display in [Display::default(), coarse] {
            let d = drawn(solid.mesh(), &display);
            eprintln!("{name} at {display:?}: {d:?}");
            // The edges' own segments are within the chord at their
            // middles, a hair more between.
            assert!(d.worst.1 <= 1.05, "{name}: {d:?}");
            assert!(d.triangles.1 >= d.triangles.0);
            assert_tiled(solid.mesh(), &display);
            let mesh = solid.tessellate(&display).unwrap();
            assert_watertight(&mesh);
            assert_normals_and_volume(&mesh);
            solid.manifold_mesh(&display).unwrap();
        }
    }
}

#[test]
fn planes_and_cylinders_keep_their_grids() {
    use crate::profile::tests::circle;
    let plate = Solid::cuboid(DVec3::ZERO, DVec3::new(10.0, 4.0, 2.0), 1, &TOL).unwrap();
    let hole = Solid::cylinder(DVec3::new(5.0, 2.0, -1.0), 1.0, 4.0, 2, &TOL).unwrap();
    let drilled =
        crate::boolean(&plate, &hole, crate::Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    let frame = crate::Frame {
        origin: DVec3::ZERO,
        x: DVec3::X,
        y: DVec3::Y,
    };
    let disc = crate::extrude(
        &crate::Profile {
            loops: vec![circle(glam::DVec2::ZERO, 50.0, 1, false)],
        },
        &frame,
        0.0,
        0.1,
        3,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    // A tube turned from a rectangle: cylinders and flat rings.
    let tube = crate::revolve(
        &crate::Profile {
            loops: vec![crate::profile::tests::rect(
                glam::DVec2::new(2.0, 0.0),
                glam::DVec2::new(3.0, 6.0),
                1,
            )],
        },
        &frame,
        crate::Sweep::Full,
        4,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    for (name, solid) in [
        ("revolved tube", tube),
        (
            "cylinder",
            Solid::cylinder(DVec3::ZERO, 3.0, 5.0, 1, &TOL).unwrap(),
        ),
        ("drilled plate", drilled),
        ("thin disc", disc),
    ] {
        let plan = Plan::new(solid.mesh(), &Display::default(), &Limits::RENDER)
            .unwrap()
            .unwrap();
        assert!(
            (plan.levels.iter()).all(|level| *level == Level::new(level.counts)),
            "{name}"
        );
        eprintln!("{name}: {} triangles", plan.triangles);
    }
}

#[test]
fn round_solids_draw_the_same_on_any_thread_count() {
    for (_, solid) in round_solids() {
        assert_deterministic(|| solid.tessellate(&Display::default()).unwrap());
        assert_deterministic(|| solid.manifold_mesh(&Display::default()).unwrap());
    }
}

#[test]
fn a_refined_level_counts_its_points_and_triangles() {
    // Finer than the counts ask: the ring's strips join each edge's
    // segments to a longer inner side.
    let level = Level::with_steps([2, 5, 1], 9);
    assert_eq!((level.inner, level.steps()), (Some(6), 9));
    assert_eq!(level.inner_points(), 7 * 8 / 2);
    assert_eq!(level.triangles(), 36 + 18 + 8);
    let inner: Vec<DVec3> = level.inner_params();
    assert_eq!(inner.len() as u64, level.inner_points());
    let indices = level.triangulate(100, &inner, |i, r| {
        (10 * i + r, DVec3::new(f64::from(i), f64::from(r), 0.0))
    });
    assert_eq!(indices.len() as u64, 3 * level.triangles());
    // Never coarser than the counts ask, and a single triangle stays one.
    assert_eq!(Level::with_steps([7, 1, 1], 4), Level::new([7, 1, 1]));
    assert_eq!(Level::with_steps([1, 1, 1], 9), Level::new([1, 1, 1]));
}
