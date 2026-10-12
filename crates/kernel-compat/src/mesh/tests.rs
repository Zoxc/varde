use glam::DVec3;

use super::*;

/// A face named `part` of feature 1 on `surface`.
fn face(part: u32, surface: Surface) -> Face {
    Face {
        name: FaceName::new(1, FacePart::Split(part)),
        surface,
        form: Form::Unknown,
        slack: 1.0,
    }
}

fn free(builder: &mut MeshBuilder) -> u32 {
    builder.face(face(0, Surface::Free))
}

/// The tetrahedron on the origin and the unit axes, moved by `offset`,
/// added to `builder`.
fn add_tetrahedron(builder: &mut MeshBuilder, offset: DVec3) {
    let f = free(builder);
    let v = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z].map(|p| builder.vert(p + offset));
    for [a, b, c] in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
        builder.tri([v[a], v[b], v[c]], f);
    }
}

fn tetrahedron(offset: DVec3) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_tetrahedron(&mut builder, offset);
    builder.build().unwrap()
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
