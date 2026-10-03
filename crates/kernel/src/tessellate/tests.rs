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
            let outer: Vec<Sample> = (0..=a as u32)
                .map(|i| (i, along(i, a, 0.0), along(i, a, 0.0)))
                .collect();
            let inner: Vec<Sample> = (0..=b as u32)
                .map(|i| {
                    let x = along(i, b, 0.3) * 0.8 + DVec3::X * 0.1;
                    (100 + i, x, x)
                })
                .collect();
            let mut out = Vec::new();
            stitch(&mut out, &outer, &inner, |_, _| 0.0);
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
    // Every triangle's centre within the chord of the surface (0.77), the
    // corners of the walls' skewed patches too, where the inner grid is
    // two steps round the arc from the corner but the diagonal there is
    // flipped (1.85 before).
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
    assert!(worst <= 1.0, "{worst}");
    // Densely sampled along the patches' normals too.
    assert!(worst_off(solid.mesh(), &display) <= 1.05);

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
        // A thin half cylinder's walls are quadrics, so grids.
        assert_tiled(&mesh, &Display::default());
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
    // The diagonals are the wires.
    assert_eq!(mesh.wires().len(), 24 * 12);
    assert!(mesh.wires().all(|wire| wire.len() == 2));
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
    // The sides' diagonals are the wires, a segment each, across a side.
    assert_eq!(mesh.part_ends(), [[6, 12, 8, 6]]);
    for wire in mesh.wires() {
        let [a, b] = wire else { panic!("{wire:?}") };
        let [a, b] = [a, b].map(|&v| Vec3::from(mesh.positions()[v as usize]));
        assert_eq!(
            (a - b).abs().cmpgt(Vec3::splat(0.5)).bitmask().count_ones(),
            2
        );
    }
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
        let edge_points = full.edge_vertices().len() as u64;
        let exact = Limits {
            vertices: full.positions().len() as u64,
            indices: full.indices().len() as u64,
            edge_points: edge_points + full.wire_vertices().len() as u64,
        };
        assert!(edge_points > 0 && exact.edge_points > edge_points);
        assert_eq!(
            tessellate_within(&mesh, &display, &exact).as_ref(),
            Ok(&full)
        );
        // Short of the wires' points, the mesh is drawn without them.
        let parts = full.clone().into_parts();
        let [f, e, c, _] = parts.part_ends[0];
        let bare = RenderMesh::from_parts(MeshParts {
            wire_vertices: Vec::new(),
            wire_ends: Vec::new(),
            part_ends: vec![[f, e, c, 0]],
            ..parts
        })
        .unwrap();
        for edge_points in [edge_points, exact.edge_points - 1] {
            let short = Limits {
                edge_points,
                ..exact
            };
            assert_eq!(tessellate_within(&mesh, &display, &short), Ok(bare.clone()));
        }
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
                edge_points: edge_points - 1,
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

/// The triangles `plan` makes of each patch, as drawing, welding and
/// measuring do, with their parameters and points.
fn in_patches(plan: &Plan) -> Vec<InPatch> {
    let mut out = Vec::new();
    for &t in &plan.tri_ids {
        let patch = plan.mesh.patch(t as usize);
        let level = &plan.levels[t as usize];
        let Sampled {
            params,
            points,
            indices,
            ..
        } = Sampled::new(&patch, level, |i, r| plan.sample_point(3 * t + i, r));
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
    // No straight lines across a patch curved both ways.
    for (t, level) in plan.levels.iter().enumerate() {
        assert!(level.straight.is_none() || !curved_both_ways(form(t)));
    }
    let worst = |plan: &Plan| {
        (in_patches(plan).iter())
            .filter(|tri| curved_both_ways(form(tri.t)))
            .map(|tri| off_patch(mesh, tri) / chord)
            .fold(0.0, f64::max)
    };
    let mut before = Plan::new(mesh, display, &Limits::RENDER).unwrap().unwrap();
    before.levels = (before.levels.iter())
        .map(|level| match level.straight {
            Some(_) => *level,
            None => Level::new(level.counts),
        })
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
        // Fractions of at most 256: anything less is rounding, three
        // samples in line.
        let twice = (b - a).perp_dot(c - a);
        assert!(twice > 1e-10, "patch {}: {:?}", tri.t, tri.params);
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
fn planes_cylinders_and_cones_are_not_refined() {
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
        let (ruled, walls) = assert_not_refined(name, &solid);
        // The hole's wall is cut by the plate's faces, and only its
        // pieces with a ruling side are strips.
        if name == "drilled plate" {
            assert!(ruled > 0 && ruled < walls, "{name}: {ruled} of {walls}");
        } else {
            assert_eq!(ruled, walls, "{name}");
        }
    }
}

/// Checks that each level of `solid`'s plan is the one its counts give
/// unrefined: a grid as [`Level::new`] makes, or a ruled strip on a
/// cylinder or cone, never on a plane. Returns how many patches on
/// cylinders and cones are ruled strips, and how many there are.
fn assert_not_refined(name: &str, solid: &Solid) -> (usize, usize) {
    let mesh = solid.mesh();
    let plan = Plan::new(mesh, &Display::default(), &Limits::RENDER)
        .unwrap()
        .unwrap();
    let mut walls = (0, 0);
    for (t, level) in plan.levels.iter().enumerate() {
        let form = &mesh.faces()[mesh.tris()[t].face as usize].form;
        assert!(!curved_both_ways(form), "{name}");
        let plane = matches!(form, Form::Plane { .. });
        match level.straight {
            Some(side) => {
                assert!(!plane, "{name}");
                assert_eq!(*level, Level::ruled(level.counts, side), "{name}");
            }
            None => assert_eq!(*level, Level::new(level.counts), "{name}"),
        }
        if !plane {
            walls.0 += usize::from(level.straight.is_some());
            walls.1 += 1;
        }
    }
    walls
}

/// The farthest a triangle of `plan` gets from its patch, densely sampled
/// along the patch's normals ([`off_patch`]), over the chord.
fn worst_of(plan: &Plan) -> f64 {
    (in_patches(plan).iter())
        .map(|tri| off_patch(plan.mesh, tri) / plan.chord)
        .fold(0.0, f64::max)
}

/// [`worst_of`] `mesh`'s plan within `display`.
fn worst_off(mesh: &Mesh, display: &Display) -> f64 {
    worst_of(&Plan::new(mesh, display, &Limits::RENDER).unwrap().unwrap())
}

/// Walls of every height, from a thousandth of their arcs to a thousand
/// times, extruded from circles, an ellipse and parabola and hyperbola
/// arcs: each wall patch with a side of one segment is a ruled strip of a
/// triangle per segment of its other two sides less one, the lines across
/// it straight to the bit, within the chord densely sampled (the grid was
/// 2.7 to 3.9, at the ring's corners and, on tall walls, along strips
/// whose shorter diagonals ran round the arc), tiling the patch.
#[test]
fn walls_are_ruled_strips_within_the_chord() {
    use crate::patch::Conic2;
    use crate::profile::tests::circle;
    use crate::{Loop, Profile, Segment};
    let v = glam::DVec2::new;
    let frame = crate::Frame {
        origin: DVec3::new(1.0, -2.0, 0.5),
        x: DVec3::new(0.0, 0.6, 0.8),
        y: DVec3::X,
    };
    let conic = |a, c, w, b, curve| Segment {
        conic: Conic2::new(a, c, w, b).unwrap(),
        curve,
    };
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let ellipse = Loop {
        segments: vec![
            conic(v(3.0, 0.0), v(3.0, 2.0), FRAC_1_SQRT_2, v(0.0, 2.0), 1),
            conic(v(0.0, 2.0), v(-3.0, 2.0), FRAC_1_SQRT_2, v(-3.0, 0.0), 2),
            conic(v(-3.0, 0.0), v(-3.0, -2.0), FRAC_1_SQRT_2, v(0.0, -2.0), 3),
            conic(v(0.0, -2.0), v(3.0, -2.0), FRAC_1_SQRT_2, v(3.0, 0.0), 4),
        ],
    };
    let conics = Loop {
        segments: vec![
            line(v(0.0, 0.0), v(4.0, 0.0), 1),
            conic(v(4.0, 0.0), v(5.0, 2.0), 1.0, v(3.0, 3.0), 2),
            conic(v(3.0, 3.0), v(1.0, 4.0), 2.0, v(0.0, 2.0), 3),
            line(v(0.0, 2.0), v(0.0, 0.0), 4),
        ],
    };
    let display = Display::default();
    for (name, profile, arc) in [
        ("circle", circle(v(0.5, 0.0), 50.0, 1, false), 78.5),
        ("ellipse", ellipse, 4.0),
        ("conics", conics, 3.0),
    ] {
        let profile = Profile {
            loops: vec![profile],
        };
        for height in [1e-3, 1e-2, 0.1, 1.0, 10.0, 1e3].map(|k| k * arc) {
            let solid =
                crate::extrude(&profile, &frame, 0.0, height, 3, &TOL, &Budget::DEFAULT).unwrap();
            let mesh = solid.mesh();
            let plan = Plan::new(mesh, &display, &Limits::RENDER).unwrap().unwrap();
            let (mut strips, mut grids) = (0, 0);
            for (t, level) in plan.levels.iter().enumerate() {
                let form = &mesh.faces()[mesh.tris()[t].face as usize].form;
                if matches!(form, Form::Plane { .. }) {
                    continue;
                }
                let side = Level::straight_side(level.counts);
                assert_eq!(level.straight, side, "{name} {height}: {level:?}");
                if side.is_some() {
                    let [a, b, c] = level.counts.map(u64::from);
                    assert_eq!(level.triangles(), a + b + c - 2);
                    strips += level.triangles();
                    grids += Level::new(level.counts).triangles();
                }
            }
            let worst = worst_of(&plan);
            eprintln!("{name} {height}: {grids} wall triangles to {strips}, worst {worst:.3}");
            // A wall a thousand times its arc has a chord past its
            // radius, and the diagonals are one segment.
            assert!(strips > 0 || height > 100.0 * arc, "{name} {height}");
            assert!(strips * 3 <= grids, "{name} {height}");
            assert!(worst <= 1.05, "{name} {height}: {worst}");
            assert_tiled(mesh, &display);
            assert_watertight(&solid.tessellate(&display).unwrap());
            solid.manifold_mesh(&display).unwrap();
        }
    }
}

/// A wall patch is ruled across its ruling, and not across an arc short
/// enough for one segment (a cut piece of a wall once was, and its strip
/// fanned round the arc, 1.14 chords off), nor across a ruling of a
/// patch whose lines across bend.
#[test]
fn only_rulings_make_ruled_strips() {
    let chord = 0.1;
    // A tenth of a radian of a circle of radius 1: its sagitta is an
    // eighth of a chord of 0.1.
    let (a, b) = (DVec3::X, DVec3::new(0.1f64.cos(), 0.1f64.sin(), 0.0));
    let w = 0.05f64.cos();
    let arc = Conic3::new(a, (a + b) * 0.5 / (w * w), w, b).unwrap();
    let [first, second] = crate::patch::cylinder_strip(&arc, DVec3::Z * 3.0).unwrap();
    // `(a0, a1, b1)`: side 0 the arc, side 1 the ruling; `(a0, b1, b0)`:
    // side 2 the ruling.
    assert!(ruled(&first, 1, chord) && ruled(&second, 2, chord));
    assert!(!ruled(&first, 0, chord) && !ruled(&second, 1, chord));
    assert!(!ruled(&first, 0, 1e3));
    // A ball's patch: no lines across are straight, whatever its sides.
    let (_, ball) = round_solids().swap_remove(0);
    let patch = ball.mesh().patch(0);
    assert!((0..3).all(|side| !ruled(&patch, side, chord)));
}

/// Cut walls, whose pieces have no ruling side, and walls drawn on the
/// grid as if they had none: the ring's diagonals chosen by how far the
/// patch strays from them and its corners flipped keep them within the
/// chord (they were 2.7 to 3.9), with the same triangles as the grid.
#[test]
fn skewed_grids_choose_their_diagonals_by_the_patch() {
    let display = Display::default();
    let big = Solid::cylinder(DVec3::ZERO, 10.0, 5.0, 1, &TOL).unwrap();
    let small = Solid::cylinder(DVec3::new(2.0, 1.0, -1.0), 5.0, 7.0, 2, &TOL).unwrap();
    let slot = Solid::cuboid(
        DVec3::new(-3.0, -20.0, 2.0),
        DVec3::new(6.0, 40.0, 10.0),
        2,
        &TOL,
    )
    .unwrap();
    let cut = |b: &Solid| crate::boolean(&big, b, crate::Op::Difference, &TOL, &Budget::DEFAULT);
    let (_, spindle) = round_solids().swap_remove(4);
    for (name, solid) in [
        ("less a cylinder", cut(&small).unwrap()),
        ("less a slot", cut(&slot).unwrap()),
        ("spindle", spindle),
    ] {
        let worst = worst_off(solid.mesh(), &display);
        eprintln!("{name}: {worst:.3}");
        assert!(worst <= 1.05, "{name}: {worst}");
        assert_tiled(solid.mesh(), &display);
        assert_watertight(&assert_deterministic(|| {
            solid.tessellate(&display).unwrap()
        }));
        assert_deterministic(|| solid.manifold_mesh(&display).unwrap());
    }
    for height in [0.05, 5.0, 50.0, 500.0] {
        let solid = Solid::cylinder(DVec3::ZERO, 10.0, height, 1, &TOL).unwrap();
        let mut plan = Plan::new(solid.mesh(), &display, &Limits::RENDER)
            .unwrap()
            .unwrap();
        let ruled = plan.levels.clone();
        plan.levels = (ruled.iter())
            .map(|level| Level::new(level.counts))
            .collect();
        plan.count();
        let worst = worst_of(&plan);
        eprintln!("cylinder {height} on grids: {worst:.3}");
        assert!(worst <= 1.05, "{height}: {worst}");
    }
}

/// The triangles drawn, welded and measured while refining are the same:
/// each chooses its diagonals from the same `f64` points (a mesh vertex,
/// an edge's curve, the patch inside), so a ring corner, a strip step or a
/// flip near a tie can't go one way in one and the other in another.
#[test]
fn drawn_welded_and_measured_triangles_are_the_same() {
    let display = Display::default();
    let big = Solid::cylinder(DVec3::ZERO, 10.0, 5.0, 1, &TOL).unwrap();
    let small = Solid::cylinder(DVec3::new(2.0, 1.0, -1.0), 5.0, 7.0, 2, &TOL).unwrap();
    let cut = crate::boolean(&big, &small, crate::Op::Difference, &TOL, &Budget::DEFAULT);
    let mut solids = round_solids();
    solids.push(("cylinder less a cylinder", cut.unwrap()));
    solids.push(("cylinder", big));
    // A triangle as its corners' bits, from its lowest.
    let canonical = |tri: [[u32; 3]; 3]| {
        let k = (0..3).min_by_key(|&k| tri[k]).unwrap_or(0);
        [0, 1, 2].map(|i| tri[(k + i) % 3])
    };
    for (name, solid) in solids {
        let mesh = solid.mesh();
        let plan = Plan::new(mesh, &display, &Limits::RENDER).unwrap().unwrap();
        let measured = in_patches(&plan);
        assert_eq!(measured.len() as u64, plan.triangles, "{name}");
        // Drawn: region by region, so as sorted triangles.
        let drawn = solid.tessellate(&display).unwrap();
        let p = drawn.positions();
        let mut ours: Vec<_> = (measured.iter())
            .map(|tri| canonical(tri.points.map(|x| bits(x.as_vec3().to_array()))))
            .collect();
        let mut theirs: Vec<_> = (drawn.indices().chunks(3))
            .map(|t| canonical([0, 1, 2].map(|i| bits(p[t[i] as usize]))))
            .collect();
        ours.sort_unstable();
        theirs.sort_unstable();
        assert!(ours == theirs, "{name}: drawn");
        // Welded: patch by patch, in order, about its origin.
        let (origin, positions, triangles) = weld(mesh, &display, &Limits::EXPORT).unwrap();
        let origin = DVec3::from_array(origin);
        assert_eq!(triangles.len(), measured.len(), "{name}");
        for (tri, welded) in measured.iter().zip(&triangles) {
            let about = tri
                .points
                .map(|x| (x - origin).as_vec3().as_dvec3().to_array());
            assert_eq!(
                about,
                welded.map(|v| positions[v as usize]),
                "{name}: welded"
            );
        }
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
    let patch = tetrahedron(DVec3::ZERO).patch(0);
    let indices = level.triangulate(&patch, 100, &inner, |i, r| {
        (10 * i + r, DVec3::new(f64::from(i), f64::from(r), 0.0))
    });
    assert_eq!(indices.len() as u64, 3 * level.triangles());
    // Never coarser than the counts ask, and a single triangle stays one.
    assert_eq!(Level::with_steps([7, 1, 1], 4), Level::new([7, 1, 1]));
    assert_eq!(Level::with_steps([1, 1, 1], 1), Level::new([1, 1, 1]));
    assert_eq!(Level::with_steps([1, 1, 1], 2).inner, Some(0));
}

#[test]
fn scaled_balls_are_measured_and_scaled_cones_keep_their_grids() {
    use crate::profile::tests::rect;
    use crate::{Loop, Motion, Profile, Segment, Sweep, revolve};
    let v = glam::DVec2::new;
    // Unequal across every axis: no circle stays round.
    let stretch = Motion::scale(DVec3::new(1.0, -2.0, 0.5), DVec3::new(1.0, 0.5, 2.0)).unwrap();
    let scaled =
        |solid: &Solid| (solid.transformed(&stretch, None, &TOL, &Budget::DEFAULT)).unwrap();
    let quadrics = |solid: &Solid| -> Vec<crate::mesh::Quadric> {
        (solid.mesh().faces().iter())
            .filter_map(|face| match face.form {
                Form::Quadric(q) => Some(q),
                _ => None,
            })
            .collect()
    };
    let (_, ball) = round_solids().swap_remove(0);
    let ellipsoid = scaled(&ball);
    let ellipsoids = quadrics(&ellipsoid);
    assert!(!ellipsoids.is_empty());
    assert!((ellipsoids.iter()).all(|&q| q.c < 0.0 && curved_both_ways(&Form::Quadric(q))));
    let display = Display::default();
    let d = drawn(ellipsoid.mesh(), &display);
    assert!(d.worst.1 <= 1.05, "{d:?}");
    assert_tiled(ellipsoid.mesh(), &display);
    assert_watertight(&ellipsoid.tessellate(&display).unwrap());

    // A cone turned from a triangle, and a cylinder from a rectangle.
    let frame = crate::Frame {
        origin: DVec3::new(1.0, -2.0, 0.5),
        x: DVec3::X,
        y: DVec3::Z,
    };
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let triangle = Loop {
        segments: vec![
            line(v(0.0, 0.0), v(2.0, 0.0), 1),
            line(v(2.0, 0.0), v(0.0, 3.0), 2),
            line(v(0.0, 3.0), v(0.0, 0.0), 3),
        ],
    };
    let turn = |loops: Vec<Loop>| {
        revolve(
            &Profile { loops },
            &frame,
            Sweep::Full,
            5,
            &TOL,
            &Budget::DEFAULT,
        )
        .unwrap()
    };
    let cone = scaled(&turn(vec![triangle]));
    let cones = quadrics(&cone);
    assert!(!cones.is_empty());
    assert!((cones.iter()).all(|&q| q.c == 0.0 && !curved_both_ways(&Form::Quadric(q))));
    let tube = scaled(&turn(vec![rect(v(2.0, 0.0), v(3.0, 6.0), 1)]));
    for (name, solid) in [("cone", cone), ("tube", tube)] {
        assert_not_refined(name, &solid);
    }
}
/// Ring corners steep to their patches: on fat tori and next to a ball's
/// cuts the patch bends hard near a corner, and the triangle from it to
/// the inner grid runs well off the patch while the patch stays near the
/// triangle's plane (measured against the plane, a nearly horn torus read
/// within at 2 chords). And a single-triangle patch of a ball's piece
/// whose middle is past the chord gets an inner point (it was 1.12).
#[test]
fn steep_ring_corners_and_single_triangles_are_measured() {
    use crate::extrude::Frame;
    use crate::profile::tests::circle;
    use crate::{Profile, Sweep, revolve};
    let v = glam::DVec2::new;
    let z = Frame {
        origin: DVec3::ZERO,
        x: DVec3::X,
        y: DVec3::Z,
    };
    let torus = |major: f64, minor: f64, sweep: Sweep| {
        let profile = Profile {
            loops: vec![circle(v(major, 0.0), minor, 1, false)],
        };
        revolve(&profile, &z, sweep, 7, &TOL, &Budget::DEFAULT).unwrap()
    };
    let (_, ball) = round_solids().swap_remove(0);
    let corner = DVec3::new(1.0, -2.0, 0.5) + DVec3::splat(0.6);
    let block = Solid::cuboid(corner, DVec3::splat(4.0), 11, &TOL).unwrap();
    let cut = crate::boolean(&ball, &block, crate::Op::Difference, &TOL, &Budget::DEFAULT).unwrap();
    // A corner of a box, rounded by the ball: one of its patches is a
    // small single triangle bulging past the chord.
    let block = Solid::cuboid(
        DVec3::new(0.6302371828494238, -0.2586836385809508, 0.8276457806392021),
        DVec3::new(2.3076261783934213, 2.470170510860735, 1.6083669627111867),
        11,
        &TOL,
    )
    .unwrap();
    let corner = crate::boolean(
        &ball,
        &block,
        crate::Op::Intersection,
        &TOL,
        &Budget::DEFAULT,
    );
    let part = Sweep::Part {
        from: -0.43,
        to: 3.2,
    };
    let display = Display::default();
    for (name, solid) in [
        ("fat torus", torus(2.85, 1.42, Sweep::Full)),
        ("fat part torus", torus(1.8, 0.83, part)),
        ("nearly horn torus", torus(34.0, 30.0, part)),
        ("cut ball", cut),
        ("rounded corner", corner.unwrap()),
    ] {
        let d = drawn(solid.mesh(), &display);
        eprintln!("{name}: {d:?}");
        assert!(d.worst.1 <= 1.05, "{name}: {d:?}");
        assert_tiled(solid.mesh(), &display);
        assert_watertight(&solid.tessellate(&display).unwrap());
        solid.manifold_mesh(&display).unwrap();
        let plan = Plan::new(solid.mesh(), &display, &Limits::RENDER)
            .unwrap()
            .unwrap();
        let opened = (plan.levels.iter())
            .filter(|level| level.counts == [1, 1, 1] && level.inner.is_some())
            .count();
        assert_eq!(opened > 0, name == "rounded corner", "{name}");
    }
}

/// A curve is flattened from end to end, as an edge would be cut, within
/// the chord of the curve.
#[test]
fn a_loose_curve_is_flattened_as_an_edge_is() {
    let arc = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 10.0, 0.0, PI / 2.0).unwrap();
    let display = Display::default();
    let points = display.flatten(&arc, 20.0);
    assert_eq!(points.len() as u32, segments(&arc, display.chord(20.0)) + 1);
    assert!(points.len() > 2);
    assert_eq!(points[0], arc.p0);
    assert_eq!(*points.last().unwrap(), arc.p1);
    for p in &points {
        assert!((p.length() - 10.0).abs() < 1e-9, "{p}");
    }
    let line = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    assert_eq!(display.flatten(&line, 1.0), [DVec3::ZERO, DVec3::X]);
}

/// A loose patch is sampled on its own: a flat one as one triangle, a
/// sphere's eighth on a grid within the chord of the sphere, its normals
/// unit and outward.
#[test]
fn a_loose_patch_is_sampled_as_a_face_is() {
    let display = Display::default();
    let flat = Patch::flat([DVec3::ZERO, DVec3::X, DVec3::Y]).unwrap();
    let samples = display.sample_patch(&flat, 1.0);
    assert_eq!(samples.indices.len(), 3);
    let corners: Vec<DVec3> = (samples.indices.iter())
        .map(|&v| samples.points[v as usize])
        .collect();
    assert_eq!(corners, [DVec3::ZERO, DVec3::X, DVec3::Y]);

    // On the sphere along its edges (inside, near it).
    let round = round_octahedron(DVec3::ZERO).patch(0);
    let diagonal = 2.0 * 3f64.sqrt();
    let samples = display.sample_patch(&round, diagonal);
    assert_eq!(samples.points.len(), samples.normals.len());
    assert!(samples.indices.len() > 3 && samples.indices.len().is_multiple_of(3));
    assert!(
        (samples.indices.iter()).all(|&v| (v as usize) < samples.points.len()),
        "indices in range"
    );
    for (p, n) in samples.points.iter().zip(&samples.normals) {
        assert!((p.length() - 1.0).abs() < 0.1, "{p}");
        assert!(
            (n.length() - 1.0).abs() < 1e-9 && n.dot(*p) > 0.9,
            "{n} at {p}"
        );
    }
    for curve in [0, 1, 2].map(|i| round.edge(i)) {
        for p in display.flatten(&curve, diagonal) {
            let at = |q: &DVec3| (*q - p).length() < 1e-12;
            assert!(samples.points.iter().any(at), "edge sample {p} is a sample");
        }
    }
}

#[test]
fn a_tall_cylinder_s_walls_follow_the_surface() {
    // On a tall wall the heights of a strip's inner points differ by more
    // than a step round the arc, and choosing the shorter diagonals
    // paired points steps apart: slivers near the caps up to 4 chords
    // inside the surface, their faces 43° off their vertex normals.
    for height in [50.0, 100.0, 200.0] {
        let radius = 5.0;
        let solid = Solid::cylinder(DVec3::ZERO, radius, height, 7, &TOL).unwrap();
        let display = Display::default();
        let mesh = solid.tessellate(&display).unwrap();
        assert_watertight(&mesh);
        let chord = display.chord(DVec3::new(2.0 * radius, 2.0 * radius, height).length());
        let p = |i: u32| Vec3::from(mesh.positions()[i as usize]).as_dvec3();
        let n = |i: u32| Vec3::from(mesh.normals()[i as usize]).as_dvec3();
        for t in mesh.indices().as_chunks::<3>().0 {
            let x = t.map(p);
            if x.iter().all(|y| y.z == x[0].z) {
                continue; // a cap
            }
            let steps = 12;
            for a in 0..=steps {
                for b in 0..=steps - a {
                    let c = steps - a - b;
                    let y = (x[0] * f64::from(a) + x[1] * f64::from(b) + x[2] * f64::from(c))
                        / f64::from(steps);
                    let off = radius - y.truncate().length();
                    assert!(off < chord, "{height}: {} chords at {y}", off / chord);
                }
            }
            let face = (x[1] - x[0]).cross(x[2] - x[0]).normalize();
            for v in t {
                assert!(face.dot(n(*v)) > 0.99, "{height}: {face} against {}", n(*v));
            }
        }
    }
}

#[test]
fn a_cylinder_s_walls_weld_into_a_prism() {
    // The wall's edges all get the arc's count, so every sample is on a
    // ruling through one of the arc's and the triangles run between
    // neighbouring rulings: the welded wall creases only along them.
    // Counted from its own curve, a tall wall's diagonal got fewer
    // segments, and the triangles near it crossed the rulings: creases
    // of up to 30° winding round the wall, which readers shading by the
    // faces show.
    for (radius, height) in [
        (10.0, 20.0),
        (10.0, 50.0),
        (5.0, 100.0),
        (5.0, 200.0),
        (0.5, 200.0),
    ] {
        let solid = Solid::cylinder(DVec3::ZERO, radius, height, 7, &TOL).unwrap();
        let welded = solid.manifold_mesh(&Display::default()).unwrap();
        let origin = DVec3::from(welded.origin());
        let p = |i: u32| DVec3::from(welded.positions()[i as usize]) + origin;
        let mut faces: BTreeMap<(u32, u32), Vec<DVec3>> = BTreeMap::new();
        for t in welded.triangles() {
            let x = t.map(p);
            let face = (x[1] - x[0]).cross(x[2] - x[0]).normalize();
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                faces.entry((a.min(b), a.max(b))).or_default().push(face);
            }
        }
        for ((a, b), f) in &faces {
            let crease = f[0].dot(f[1]);
            let rim = crease.abs() < 0.5;
            let ruling = (p(*a) - p(*b)).truncate().length() < 1e-9;
            assert!(
                rim || ruling || crease > 1.0 - 1e-9,
                "{radius} × {height}: {crease}"
            );
        }
    }
}
