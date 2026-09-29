use glam::DVec3;

use super::super::tests::{
    OCTAHEDRON, TOL, UNIT, add_round_octahedron, face, octahedron, round_octahedron, tetrahedron,
};
use super::*;
use crate::Tolerance;
use crate::mesh::{MeshBuilder, Surface};
use crate::par::assert_deterministic;
use crate::patch::PatchError;
use crate::test_rng::Rng;

/// A thin curved plate closed on itself: the shell between two round
/// octahedra, of radius `radius` and `radius - thickness`, the inner one
/// facing in. Their hulls overlap until each side is split finely enough.
fn shell(radius: f64, thickness: f64) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, DVec3::ZERO, radius, false);
    add_round_octahedron(&mut builder, DVec3::ZERO, radius - thickness, true);
    builder.build().unwrap()
}

/// Adds `mesh`'s vertices, faces and triangles to `builder`, which has
/// `base` vertices already.
fn add_mesh(builder: &mut MeshBuilder, mesh: &Mesh, base: u32) {
    for &p in mesh.verts() {
        builder.vert(p);
    }
    let faces: Vec<u32> = mesh.faces().iter().map(|&f| builder.face(f)).collect();
    for t in 0..mesh.tris().len() {
        let c = mesh.corners(t as u32).map(|v| v + base);
        let patch = mesh.patch(t);
        for i in 0..3 {
            builder.edge(c[i], c[(i + 1) % 3], patch.c[i], patch.w[i]);
        }
        builder.tri(c, faces[mesh.tris()[t].face as usize]);
    }
}

/// The unit cylinder with a half-unit box beside it, its nearest corner
/// `gap` off the wall at 30° round from `+x`.
fn cylinder_and_box(gap: f64) -> Mesh {
    let cylinder = Mesh::cylinder(DVec3::ZERO, 1.0, 2.0, 1, &TOL).unwrap();
    let (s, c) = 30f64.to_radians().sin_cos();
    let corner = DVec3::new(c, s, 0.0) * (1.0 + gap) + DVec3::Z * 0.5;
    let cuboid = Mesh::cuboid(corner, DVec3::splat(0.5), 2, &TOL).unwrap();
    both(&cylinder, &cuboid)
}

/// `a` and `b` as one mesh, `b`'s vertices and triangles after `a`'s.
fn both(a: &Mesh, b: &Mesh) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_mesh(&mut builder, a, 0);
    add_mesh(&mut builder, b, a.verts().len() as u32);
    builder.build().unwrap()
}

/// Two round octahedra of `radius`, side by side along `x` with
/// `gap` between their nearest corners.
fn two_spheres(radius: f64, gap: f64) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, DVec3::ZERO, radius, false);
    add_round_octahedron(&mut builder, DVec3::X * (2.0 * radius + gap), radius, false);
    builder.build().unwrap()
}

/// `mesh` repaired with `tol`, and the work that took.
fn repair_counting(mesh: Mesh, tol: &Tolerance) -> (Result<Mesh, KernelError>, u64) {
    let mut work = Work::new(&Budget::DEFAULT);
    let result = mesh.repair_within(tol, &mut work);
    (result, Budget::DEFAULT.work() - work.left())
}

/// A tetrahedron of four patches bulging out like a sphere, with each
/// edge's control point `k` times as far out as the arc's: at `k = 1.5`
/// the patches fail the fold check, though their pieces pass.
fn bulging_tetrahedron(k: f64) -> Mesh {
    let v = [
        DVec3::new(1.0, 1.0, 1.0),
        DVec3::new(1.0, -1.0, -1.0),
        DVec3::new(-1.0, 1.0, -1.0),
        DVec3::new(-1.0, -1.0, 1.0),
    ]
    .map(|p| p / 3f64.sqrt());
    let mut builder = MeshBuilder::new();
    let f = builder.face(face(0, Surface::Free));
    let ids = v.map(|p| builder.vert(p));
    for [a, b, c] in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
        builder.tri([ids[a], ids[b], ids[c]], f);
    }
    for a in 0..4 {
        for b in a + 1..4 {
            // The arc of the unit sphere between them.
            let cos = v[a].dot(v[b]);
            let ctrl = (v[a] + v[b]) / (1.0 + cos);
            builder.edge(ids[a], ids[b], ctrl * k, ((1.0 + cos) / 2.0).sqrt());
        }
    }
    builder.build().unwrap()
}

#[test]
fn a_thin_curved_plate_is_repaired() {
    for (thickness, patches) in [(0.2, 1024), (0.05, 4096)] {
        let mesh = shell(10.0, thickness);
        assert!(matches!(mesh.check(&TOL), Err(CheckError::Hull(..))));
        let repaired = mesh.repair(&TOL, &Budget::DEFAULT).unwrap();
        assert_eq!(repaired.check(&TOL), Ok(()));
        // Both sides split evenly, into pieces of about 22.5° or 11.25°.
        assert_eq!(repaired.tris().len(), patches, "{thickness}");
    }
}

#[test]
fn repair_is_the_same_on_any_thread_count() {
    let repaired = assert_deterministic(|| shell(10.0, 0.2).repair(&TOL, &Budget::DEFAULT));
    assert!(repaired.is_ok());
    let repaired = assert_deterministic(|| cylinder_and_box(1e-3).repair(&TOL, &Budget::DEFAULT));
    assert!(repaired.is_ok());
}

#[test]
fn repair_splits_only_near_the_trouble_and_keeps_surfaces() {
    let mut previous = 0;
    for gap in [0.1, 1e-2, 1e-3] {
        let mesh = cylinder_and_box(gap);
        assert_eq!(mesh.check(&TOL), Err(CheckError::Hull(0, 16)));
        let repaired = mesh.clone().repair(&TOL, &Budget::DEFAULT).unwrap();
        assert_eq!(repaired.check(&TOL), Ok(()));
        assert_eq!(repaired.check_faces(&TOL), Ok(()));
        // Graded: a closer box takes a few more pieces near it.
        let n = repaired.tris().len();
        assert!(n > previous && n < 200, "{gap}: {n}");
        previous = n;
        // The flat box is never split.
        for t in 16..28 {
            let found = (0..n).any(|u| repaired.patch(u) == mesh.patch(t));
            assert!(found, "box patch {t} is gone");
        }
        // Pieces of the wall are on the cylinder, and pieces of the caps
        // on their planes, to rounding.
        let mut rng = Rng::new(7);
        for t in 0..n {
            let patch = repaired.patch(t);
            let surface = repaired.faces()[repaired.tris()[t].face as usize].surface;
            let off = match surface {
                Surface::Quadric(_) => (0..10)
                    .map(|_| surface.distance(patch.eval(rng.bary())))
                    .fold(0.0, f64::max),
                _ => patch
                    .hull()
                    .map(|x| surface.distance(x))
                    .into_iter()
                    .fold(0.0, f64::max),
            };
            assert!(off < 1e-12, "patch {t} is {off} off its face");
        }
    }
}

#[test]
fn face_tags_are_carried_through() {
    // Repair neither checks the input's face tags nor promises them: a
    // wrong one comes through, and the result fails only `check_faces`.
    let mut mesh = cylinder_and_box(1e-2);
    // Face 2 is the cylinder's first wall.
    mesh.faces[2].surface = Surface::Plane {
        n: DVec3::Z,
        d: 100.0,
    };
    let repaired = mesh.repair(&TOL, &Budget::DEFAULT).unwrap();
    assert_eq!(repaired.check_embedding(&TOL).err(), None);
    assert!(repaired.check_faces(&TOL).is_err());
}

#[test]
fn a_fold_is_repaired() {
    let mesh = bulging_tetrahedron(1.5);
    assert_eq!(mesh.check(&TOL), Err(CheckError::Fold(0)));
    let repaired = mesh.repair(&TOL, &Budget::DEFAULT).unwrap();
    assert_eq!(repaired.check(&TOL), Ok(()));
    assert_eq!(repaired.tris().len(), 16);
}

#[test]
fn a_mesh_that_passes_is_kept() {
    let mesh = round_octahedron(DVec3::ZERO);
    assert_eq!(mesh.clone().repair(&TOL, &Budget::DEFAULT), Ok(mesh));
}

#[test]
fn what_splitting_cant_mend_fails() {
    // A corner whose two edges leave it the same way fails at once.
    let ctrl = DVec3::X + (DVec3::new(0.5, 0.0, 0.5) - DVec3::X) * 0.5;
    let cusp = octahedron(UNIT, &OCTAHEDRON, &[(0, 1, ctrl, 1.0)]);
    assert_eq!(
        cusp.repair(&TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(CheckError::Fold(0)))
    );

    // Flat triangles closer than the resolution fail at once: their
    // pieces keep the gap, and nothing splitting does brings them apart.
    // Two boxes face to face used to be split to the end of the budget.
    let gap = 0.5 * TOL.resolution();
    let tetrahedra = both(
        &tetrahedron(DVec3::ZERO),
        &tetrahedron(DVec3::X * (1.0 + gap)),
    );
    let (result, work) = repair_counting(tetrahedra, &TOL);
    assert_eq!(result, Err(KernelError::Invalid(CheckError::Hull(0, 4))));
    assert!(work < 1000, "{work}");
    let boxes = both(
        &Mesh::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap(),
        &Mesh::cuboid(DVec3::Z * (1.0 + gap), DVec3::ONE, 2, &TOL).unwrap(),
    );
    // The lower box's top against the upper one's bottom.
    let (result, work) = repair_counting(boxes, &TOL);
    assert_eq!(result, Err(KernelError::Invalid(CheckError::Hull(2, 12))));
    assert!(work < 1000, "{work}");

    // Round surfaces that touch are split until the pieces at the touch
    // are flat within the resolution, then fail the same way.
    let (result, work) = repair_counting(two_spheres(1.0, gap), &TOL);
    assert!(
        matches!(result, Err(KernelError::Invalid(CheckError::Hull(..)))),
        "{result:?}"
    );
    assert!(work < 100_000, "{work}");
    // Small ones reach the smallest piece repair splits first.
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let spheres = two_spheres(200.0 * coarse.resolution(), 0.5 * coarse.resolution());
    let (result, work) = repair_counting(spheres, &coarse);
    assert_eq!(result, Err(KernelError::TooComplex));
    assert!(work < 100_000, "{work}");

    // Running out of the budget.
    assert_eq!(
        shell(10.0, 0.05).repair(&TOL, &Budget::new(1000)),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn invalid_input_is_refused() {
    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.tris[0].halfedges[0].pair = 0;
    assert_eq!(
        mesh.repair(&TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(CheckError::Pair(0)))
    );
    let mut mesh = round_octahedron(DVec3::ZERO);
    let e = mesh.halfedge(4).edge as usize;
    mesh.edges[e].weight = 100.0;
    assert_eq!(
        mesh.repair(&TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(CheckError::Patch(
            1,
            PatchError::Weight(100.0)
        )))
    );
}
