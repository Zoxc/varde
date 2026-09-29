use super::*;
use crate::Shape;

#[test]
fn append_offsets_indices() {
    let cube = Shape::cuboid(Vec3::ONE)
        .build()
        .unwrap()
        .tessellate(&crate::Display::default())
        .unwrap();
    let mut mesh = cube.clone();
    mesh.append_at(&cube, Vec3::ZERO).unwrap();
    let base = cube.positions.len() as u32;
    assert_eq!(mesh.positions.len(), 2 * cube.positions.len());
    assert_eq!(mesh.normals.len(), mesh.positions.len());
    assert_eq!(mesh.indices[cube.indices.len()], base + cube.indices[0]);
    assert_eq!(mesh.edges[cube.edges.len()][1], base + cube.edges[0][1]);
}

#[test]
fn append_refuses_more_than_the_limits() {
    let cube = Shape::cuboid(Vec3::ONE)
        .build()
        .unwrap()
        .tessellate(&crate::Display::default())
        .unwrap();
    // Zeroed, so the pages aren't touched.
    let len = RenderMesh::MAX_VERTICES - cube.positions.len() + 1;
    let mut mesh = RenderMesh {
        positions: vec![[0.0; 3]; len],
        normals: vec![[0.0; 3]; len],
        ..RenderMesh::default()
    };
    assert_eq!(mesh.append_at(&cube, Vec3::ZERO), Err(MeshError::TooLarge));
    assert_eq!(mesh.positions.len(), len);
    assert!(mesh.indices.is_empty());

    let mut mesh = RenderMesh {
        indices: vec![0; RenderMesh::MAX_INDICES - cube.indices.len() + 1],
        ..RenderMesh::default()
    };
    assert_eq!(mesh.append_at(&cube, Vec3::ZERO), Err(MeshError::TooLarge));

    let mut mesh = RenderMesh {
        edges: vec![[0; 2]; RenderMesh::MAX_EDGES - cube.edges.len() + 1],
        ..RenderMesh::default()
    };
    assert_eq!(mesh.append_at(&cube, Vec3::ZERO), Err(MeshError::TooLarge));
}

#[test]
fn append_at_moves_positions_but_not_normals() {
    let cube = Shape::cuboid(Vec3::ONE)
        .build()
        .unwrap()
        .tessellate(&crate::Display::default())
        .unwrap();
    let mut mesh = RenderMesh::default();
    mesh.append_at(&cube, Vec3::new(1.0, 2.0, 3.0)).unwrap();
    assert_eq!(mesh.normals, cube.normals);
    assert_eq!(
        mesh.bounds(),
        Some(Aabb {
            min: Vec3::new(1.0, 2.0, 3.0),
            max: Vec3::new(2.0, 3.0, 4.0)
        })
    );
}

#[test]
fn append_at_refuses_positions_past_the_limit() {
    // Reaching the limit is fine.
    let mut edge = triangle();
    edge.positions[1][0] = RenderMesh::MAX_POSITION;
    let mut mesh = RenderMesh::default();
    mesh.append_at(&edge, Vec3::ZERO).unwrap();

    // A mesh already at the limit, moved further, or moved anywhere
    // non-finite, is not.
    for offset in [Vec3::X * MAX_COORD, Vec3::NAN, Vec3::NEG_INFINITY] {
        let mut mesh = triangle();
        assert_eq!(
            mesh.append_at(&edge, offset),
            Err(MeshError::Values(MeshPart::Positions))
        );
        assert_eq!(mesh, triangle());
    }
}

/// A mesh with one triangle, its fields to break.
fn triangle() -> RenderMesh {
    RenderMesh {
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        indices: vec![0, 1, 2],
        edges: vec![[0, 1], [1, 2], [2, 0]],
    }
}

/// `mesh` again, by way of [`RenderMesh::from_parts`].
fn rebuild(mesh: RenderMesh) -> Result<RenderMesh, MeshError> {
    RenderMesh::from_parts(mesh.positions, mesh.normals, mesh.indices, mesh.edges)
}

#[test]
fn from_parts_takes_a_mesh() {
    assert_eq!(rebuild(triangle()), Ok(triangle()));
    assert_eq!(rebuild(RenderMesh::default()), Ok(RenderMesh::default()));
    let cube = Shape::cuboid(Vec3::ONE)
        .build()
        .unwrap()
        .tessellate(&crate::Display::default())
        .unwrap();
    assert_eq!(rebuild(cube.clone()), Ok(cube));
}

#[test]
fn from_parts_needs_a_normal_per_position() {
    let mut mesh = triangle();
    mesh.normals.pop();
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::Normals {
            positions: 3,
            normals: 2
        })
    );
}

#[test]
fn from_parts_needs_whole_triangles() {
    let mut mesh = triangle();
    mesh.indices.pop();
    assert_eq!(rebuild(mesh), Err(MeshError::Triangles(2)));
}

#[test]
fn from_parts_needs_indices_and_edges_to_refer_to_vertices() {
    let mut mesh = triangle();
    mesh.indices[1] = 3;
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::OutOfRange {
            part: MeshPart::Indices,
            index: 3,
            vertices: 3
        })
    );

    let mut mesh = triangle();
    mesh.edges[2][1] = u32::MAX;
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::OutOfRange {
            part: MeshPart::Edges,
            index: u32::MAX,
            vertices: 3
        })
    );

    // No vertices at all: any index is out of range.
    let mesh = RenderMesh {
        positions: vec![],
        normals: vec![],
        ..triangle()
    };
    assert!(matches!(
        rebuild(mesh),
        Err(MeshError::OutOfRange { vertices: 0, .. })
    ));
}

#[test]
fn from_parts_needs_bounded_positions_and_finite_normals() {
    let mut mesh = triangle();
    mesh.positions[1][0] = RenderMesh::MAX_POSITION;
    mesh.normals[1] = [0.0; 3];
    assert!(rebuild(mesh).is_ok());

    for bad in [f32::NAN, f32::INFINITY, -1.01 * RenderMesh::MAX_POSITION] {
        let mut mesh = triangle();
        mesh.positions[1][2] = bad;
        assert_eq!(rebuild(mesh), Err(MeshError::Values(MeshPart::Positions)));
    }
    for bad in [f32::NAN, f32::NEG_INFINITY] {
        let mut mesh = triangle();
        mesh.normals[2][0] = bad;
        assert_eq!(rebuild(mesh), Err(MeshError::Values(MeshPart::Normals)));
    }
}

#[test]
fn from_parts_refuses_more_than_the_limits() {
    // Zeroed, so the pages aren't touched.
    let len = RenderMesh::MAX_VERTICES + 1;
    assert_eq!(
        RenderMesh::from_parts(vec![[0.0; 3]; len], vec![[0.0; 3]; len], vec![], vec![]),
        Err(MeshError::TooLarge)
    );
    assert_eq!(
        RenderMesh::from_parts(vec![], vec![], vec![0; RenderMesh::MAX_INDICES + 1], vec![]),
        Err(MeshError::TooLarge)
    );
    assert_eq!(
        RenderMesh::from_parts(
            vec![],
            vec![],
            vec![],
            vec![[0; 2]; RenderMesh::MAX_EDGES + 1]
        ),
        Err(MeshError::TooLarge)
    );
}
