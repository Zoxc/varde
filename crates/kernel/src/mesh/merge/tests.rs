use glam::DVec3;

use super::super::tests::{TOL, face};
use super::super::{FaceName, FacePart, Form, Mesh, MeshBuilder, Surface};
use crate::budget::{Budget, Work};
use crate::par::assert_deterministic;
use crate::{KernelError, Solid};

/// A small length: the pass's bar, an eighth of the resolution.
fn small() -> f64 {
    TOL.resolution() / 8.0
}

/// A prism over the unit strip `0 ≤ x ≤ k`, `0 ≤ y ≤ 1`, from `z = −1`
/// to a top through the heights `tops[i]` at `x = i`. Each span `i` of
/// the top is its own face `Split(10 + i)`, a plane `z = tags[i]`; the
/// bottom is a plane `Split(1)`, the sides one face `Split(0)` claiming no
/// surface, and the end at `x = k` a face claiming none named `end`.
fn ramp(tops: &[f64], tags: &[f64], end: FaceName) -> Mesh {
    let k = tops.len() - 1;
    let mut builder = MeshBuilder::new();
    let at = |x: usize, y: f64, z: f64| DVec3::new(x as f64, y, z);
    let t0: Vec<u32> = (0..=k).map(|i| builder.vert(at(i, 0.0, tops[i]))).collect();
    let t1: Vec<u32> = (0..=k).map(|i| builder.vert(at(i, 1.0, tops[i]))).collect();
    let b0: Vec<u32> = (0..=k).map(|i| builder.vert(at(i, 0.0, -1.0))).collect();
    let b1: Vec<u32> = (0..=k).map(|i| builder.vert(at(i, 1.0, -1.0))).collect();
    let sides = builder.face(face(0, Surface::Free));
    let bottom = builder.face(face(
        1,
        Surface::Plane {
            n: -DVec3::Z,
            d: 1.0,
        },
    ));
    let last = builder.face(super::super::Face {
        name: end,
        surface: Surface::Free,
        form: Form::Unknown,
    });
    for i in 0..k {
        let top = builder.face(face(
            10 + i as u32,
            Surface::Plane {
                n: DVec3::Z,
                d: tags[i],
            },
        ));
        let j = i + 1;
        builder.tri([t0[i], t0[j], t1[j]], top);
        builder.tri([t0[i], t1[j], t1[i]], top);
        builder.tri([b0[i], b1[j], b0[j]], bottom);
        builder.tri([b0[i], b1[i], b1[j]], bottom);
        builder.tri([b0[i], b0[j], t0[j]], sides);
        builder.tri([b0[i], t0[j], t0[i]], sides);
        builder.tri([b1[i], t1[j], b1[j]], sides);
        builder.tri([b1[i], t1[i], t1[j]], sides);
    }
    builder.tri([b0[0], t0[0], t1[0]], sides);
    builder.tri([b0[0], t1[0], b1[0]], sides);
    builder.tri([b0[k], b1[k], t1[k]], last);
    builder.tri([b0[k], t1[k], t0[k]], last);
    builder.build().unwrap()
}

fn merged(mesh: Mesh) -> Mesh {
    let mut work = Work::new(&Budget::DEFAULT);
    mesh.merge_faces(TOL.resolution(), &mut work).unwrap()
}

/// The names of the faces of `mesh`'s triangles, in order.
fn names(mesh: &Mesh) -> Vec<FaceName> {
    mesh.tris()
        .iter()
        .map(|t| mesh.faces()[t.face as usize].name)
        .collect()
}

/// The faces of `mesh` named `name`.
fn named(mesh: &Mesh, name: FaceName) -> Vec<u32> {
    (0..mesh.faces().len() as u32)
        .filter(|&f| mesh.faces()[f as usize].name == name)
        .collect()
}

fn split(n: u32) -> FaceName {
    FaceName::new(1, FacePart::Split(n))
}

#[test]
fn a_top_split_over_two_faces_of_one_plane_is_one() {
    let mesh = ramp(&[0.0; 4], &[0.0; 3], split(2));
    assert_eq!(mesh.check(&TOL), Ok(()));
    let before = names(&mesh);
    let after = merged(mesh.clone());
    // Only labels: the same vertices, edges and triangles.
    assert_eq!(after.verts(), mesh.verts());
    assert_eq!(after.edges(), mesh.edges());
    for (a, b) in after.tris().iter().zip(mesh.tris()) {
        assert_eq!(a.halfedges, b.halfedges);
    }
    // Only names: the same faces, on the same surfaces.
    assert_eq!(after.faces().len(), mesh.faces().len());
    for (a, b) in after.faces().iter().zip(mesh.faces()) {
        assert_eq!(a.surface, b.surface);
    }
    // One top, named by the first span.
    for (a, b) in names(&after).into_iter().zip(before) {
        let top = matches!(b.part, FacePart::Split(n) if n >= 10);
        assert_eq!(a, if top { split(10) } else { b });
    }
    // The others' keys are aliases of each piece of it.
    let tops = named(&after, split(10));
    assert_eq!(tops.len(), 3);
    for f in tops {
        let aliases: Vec<_> = after.face_aliases(f).collect();
        assert_eq!(aliases, vec![split(11).key(), split(12).key()]);
    }
    assert_eq!(after.check(&TOL), Ok(()));
    let solid = Solid::new(after, &TOL).unwrap();
    assert!((solid.volume() - 3.0).abs() < 1e-12);
    let topology = solid.topology();
    let near = DVec3::new(2.5, 0.5, 0.0);
    let found = topology.face(&solid, &split(12).key(), near).unwrap();
    assert_eq!(
        Ok(found),
        topology.face(&solid, &split(10).key(), near),
        "the absorbed name resolves to the merged face"
    );
}

#[test]
fn a_copy_claiming_no_surface_follows_its_face() {
    // The end claims no surface and carries the second span's name: it
    // takes the first span's, with the same aliases.
    let after = merged(ramp(&[0.0; 3], &[0.0; 2], split(11)));
    let tens = named(&after, split(10));
    assert_eq!(tens.len(), 3);
    assert!(matches!(after.faces()[2].surface, Surface::Free));
    assert!(tens.contains(&2));
    for f in tens {
        let aliases: Vec<_> = after.face_aliases(f).collect();
        assert_eq!(aliases, vec![split(11).key()]);
    }
}

#[test]
fn planes_two_bars_apart_stay_apart() {
    let mesh = ramp(&[0.0; 3], &[0.0, 2.0 * small()], split(2));
    let after = merged(mesh.clone());
    assert_eq!(after, mesh);
}

#[test]
fn a_chain_drifting_off_the_first_surface_merges_only_what_stays_on_it() {
    // Each span's tag lies within the bar of the next one's patches, so
    // neighbours pass, but the third span's patches are 1.5 bars off the
    // first one's plane (the spans are alike, so the first is the one
    // measured against): the first two merge, the third stays apart.
    let h = 0.6 * small();
    let tags = [0.5 * h, 1.5 * h, 2.5 * h];
    let mesh = ramp(&[0.0, h, 2.0 * h, 3.0 * h], &tags, split(2));
    let after = merged(mesh);
    assert_eq!(named(&after, split(10)).len(), 2);
    assert_eq!(named(&after, split(12)).len(), 1);
    assert_eq!(after.face_aliases(named(&after, split(12))[0]).count(), 0);
}

#[test]
fn merging_is_charged_and_deterministic() {
    let mesh = ramp(&[0.0; 5], &[0.0; 4], split(2));
    let n = mesh.tris().len();
    let mut work = Work::new(&Budget::new(n as u64 - 1));
    assert_eq!(
        mesh.clone().merge_faces(TOL.resolution(), &mut work),
        Err(KernelError::TooComplex)
    );
    assert_deterministic(|| merged(mesh.clone()));
}
