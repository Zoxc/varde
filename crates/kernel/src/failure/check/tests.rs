use glam::DVec3;

use super::*;
use crate::mesh::tests::{TOL, joined, torus};
use crate::par::assert_deterministic;
use crate::patch::PatchError;
use crate::{Budget, Solid};

fn cube(min: DVec3, size: f64) -> Mesh {
    Mesh::cuboid(min, DVec3::splat(size), 1, &TOL).unwrap()
}

/// The patches of `mesh`'s triangles `tris`.
fn patches(mesh: &Mesh, tris: impl IntoIterator<Item = u32>) -> Vec<Patch> {
    tris.into_iter().map(|t| mesh.patch(t as usize)).collect()
}

#[test]
fn each_error_names_its_triangles() {
    let mesh = cube(DVec3::ZERO, 1.0);
    for (error, tris) in [
        (CheckError::Fold(3), vec![3]),
        (CheckError::Face(0), vec![0]),
        (CheckError::FacesAgainst(11), vec![11]),
        (CheckError::Hull(1, 7), vec![1, 7]),
        (CheckError::EdgeNeighbours(7, 1), vec![7, 1]),
        (CheckError::VertexNeighbours(2, 5), vec![2, 5]),
        (CheckError::SameCorners(4, 6), vec![4, 6]),
        // Two pieces of one triangle, as repair may name them.
        (CheckError::Hull(5, 5), vec![5]),
        // The structural errors: the triangle named, or the halfedge's.
        (CheckError::Patch(9, PatchError::Degenerate), vec![9]),
        (CheckError::Index(7), vec![2]),
        (CheckError::Pair(0), vec![0]),
        (CheckError::Loop(35), vec![11]),
        (CheckError::DirectedEdge(3), vec![1]),
        (CheckError::SharedEdge(5), vec![1]),
        (CheckError::Fan(0), vec![]),
        (CheckError::EdgeUse(0), vec![]),
        (CheckError::Alias(0), vec![]),
        (CheckError::Counts, vec![]),
        (CheckError::TooManyPatches(1 << 30), vec![]),
        // Out of range: nothing.
        (CheckError::Fold(12), vec![]),
        (CheckError::Hull(3, 99), vec![3]),
        (CheckError::Index(36), vec![]),
        (CheckError::InsideOut(12), vec![]),
    ] {
        let failure = Failure::of_mesh(KernelError::Invalid(error), &mesh);
        assert_eq!(failure.error, KernelError::Invalid(error));
        assert_eq!(failure.evidence.patches, patches(&mesh, tris), "{error:?}");
        let rest = Evidence {
            patches: Vec::new(),
            ..*failure.evidence
        };
        assert!(rest.is_empty(), "{error:?}");
    }
    // Other errors are bare.
    for error in [
        KernelError::TooComplex,
        KernelError::Patch(PatchError::Degenerate),
    ] {
        assert!(Failure::of_mesh(error, &mesh).evidence.is_empty());
    }
}

#[test]
fn a_triangle_with_indices_out_of_range_is_left_out() {
    let cube = cube(DVec3::ZERO, 1.0);
    let mut tris = cube.tris().to_vec();
    tris[2].halfedges[1].edge = 1000;
    tris[4].halfedges[0].start = 1000;
    let broken = Mesh::from_parts(
        cube.verts().to_vec(),
        cube.edges().to_vec(),
        tris,
        cube.faces().to_vec(),
    );
    assert_eq!(broken.check(&TOL), Err(CheckError::Index(7)));
    for error in [
        CheckError::Fold(2),
        CheckError::Fold(4),
        CheckError::Index(7),
        CheckError::Index(12),
    ] {
        let failure = Failure::of_mesh(KernelError::Invalid(error), &broken);
        assert!(failure.evidence.is_empty(), "{error:?}");
    }
    let failure = Failure::of_mesh(KernelError::Invalid(CheckError::Index(3)), &broken);
    assert_eq!(failure.evidence.patches, patches(&cube, [1]));
}

#[test]
fn an_inside_out_shell_is_given_whole() {
    // A cube with a second, turned inside out: the error names the second
    // shell's lowest triangle, and the evidence is that shell, all 12 of
    // its triangles.
    let mesh = joined(&[
        (&cube(DVec3::ZERO, 1.0), false),
        (&cube(DVec3::splat(5.0), 1.0), true),
    ]);
    let error = mesh.check(&TOL).unwrap_err();
    assert_eq!(error, CheckError::InsideOut(12));
    let failure = Failure::of_mesh(KernelError::Invalid(error), &mesh);
    let evidence = &failure.evidence;
    assert!(!evidence.truncated);
    assert_eq!(evidence.patches.len(), 12);
    assert_eq!(evidence.patches[0], mesh.patch(12));
    let mut got: Vec<String> = evidence.patches.iter().map(|p| format!("{p:?}")).collect();
    let mut want: Vec<String> = patches(&mesh, 12..24)
        .iter()
        .map(|p| format!("{p:?}"))
        .collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn a_large_inside_out_shell_is_cut_at_the_cap() {
    // A torus of 8 192 flat triangles turned inside out: the first
    // `MAX_EVIDENCE` of them outward from its lowest, triangle 0, all
    // different, then truncated. The same at 1 and 8 threads.
    let mesh = joined(&[(&torus(64, 64, 10.0, 2.0), true)]);
    assert_eq!(mesh.tris().len(), 8192);
    let failure = assert_deterministic(|| {
        let error = mesh.check(&TOL).unwrap_err();
        Failure::of_mesh(KernelError::Invalid(error), &mesh)
    });
    assert_eq!(
        failure.error,
        KernelError::Invalid(CheckError::InsideOut(0))
    );
    let evidence = &failure.evidence;
    assert!(evidence.truncated && evidence.within_caps());
    assert_eq!(evidence.patches.len(), MAX_EVIDENCE.patches);
    assert_eq!(evidence.patches[0], mesh.patch(0));
    let all: Vec<String> = patches(&mesh, 0..8192)
        .iter()
        .map(|p| format!("{p:?}"))
        .collect();
    let mut got: Vec<String> = evidence.patches.iter().map(|p| format!("{p:?}")).collect();
    assert!(got.iter().all(|p| all.contains(p)));
    got.sort();
    got.dedup();
    assert_eq!(got.len(), MAX_EVIDENCE.patches);
}

#[test]
fn finishing_an_inside_out_mesh_gives_its_shell() {
    // Through `Solid::finished` (revolve's) and `new_repaired_within`
    // (extrude's): repair passes it as it is, the check refuses it, with
    // the error `Solid::new` gives and the shell's 12 triangles.
    let turned = joined(&[(&cube(DVec3::ZERO, 1.0), true)]);
    let want = Solid::new(turned.clone(), &TOL).unwrap_err();
    assert_eq!(want, KernelError::Invalid(CheckError::InsideOut(0)));
    let budget = Budget::DEFAULT;
    for failure in [
        Solid::finished(turned.clone(), &TOL, &mut Work::new(&budget)).unwrap_err(),
        Solid::new_repaired_within(turned.clone(), &TOL, &mut Work::new(&budget)).unwrap_err(),
    ] {
        assert_eq!(failure.error, want);
        assert_eq!(failure.evidence.patches.len(), 12);
        assert!(!failure.evidence.truncated);
        assert_eq!(failure.evidence.patches[0], turned.patch(0));
    }
}
