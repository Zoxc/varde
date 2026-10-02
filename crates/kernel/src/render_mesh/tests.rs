use super::*;
use crate::{Solid, Tolerance};
use glam::DVec3;

fn cube() -> RenderMesh {
    Solid::cuboid(DVec3::ZERO, DVec3::ONE, 0, &Tolerance::DEFAULT)
        .unwrap()
        .tessellate(&crate::Display::default())
        .unwrap()
}

fn cylinder() -> RenderMesh {
    Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 0, &Tolerance::DEFAULT)
        .unwrap()
        .tessellate(&crate::Display::default())
        .unwrap()
}

#[test]
fn append_offsets_ids_and_records_parts() {
    let (cube, cylinder) = (cube(), cylinder());
    assert_eq!(cube.parts().count(), 1);
    let offset = Vec3::new(5.0, 0.0, 0.0);
    let mut mesh = cube.clone();
    mesh.append_at(&cylinder, offset).unwrap();
    let base = cube.positions.len() as u32;
    assert_eq!(
        mesh.positions.len(),
        cube.positions.len() + cylinder.positions.len()
    );
    assert_eq!(mesh.normals.len(), mesh.positions.len());
    assert_eq!(mesh.indices[cube.indices.len()], base + cylinder.indices[0]);
    let indices = cube.indices.len() as u32;
    assert_eq!(mesh.face_ends[6..], cylinder.face_ends.map_add(indices));
    assert_eq!(
        mesh.edge_vertices[cube.edge_vertices.len()..],
        cylinder.edge_vertices.map_add(base)
    );
    let points = cube.edge_vertices.len() as u32;
    assert_eq!(mesh.edge_ends[12..], cylinder.edge_ends.map_add(points));
    let faces = cylinder.edge_faces.iter().map(|f| f.map(|f| f + 6));
    assert!(mesh.edge_faces[12..].iter().copied().eq(faces));
    let corners = cylinder.edge_corners.iter().map(|c| c.map(|c| c + 8));
    assert!(mesh.edge_corners[12..].iter().copied().eq(corners));
    let moved = (cylinder.corners.iter()).map(|&c| (Vec3::from(c) + offset).to_array());
    assert!(mesh.corners[8..].iter().copied().eq(moved));
    let [f, e, c] = cylinder.part_ends[0];
    assert_eq!(mesh.part_ends, [[6, 12, 8], [6 + f, 12 + e, 8 + c]]);

    let parts: Vec<RenderPart> = mesh.parts().collect();
    assert_eq!(parts[0], cube.parts().next().unwrap());
    assert_eq!(
        parts[1],
        RenderPart {
            faces: 6..mesh.face_count(),
            indices: cube.indices.len()..mesh.indices.len(),
            edges: 12..mesh.edge_count(),
            edge_vertices: cube.edge_vertices.len()..mesh.edge_vertices.len(),
            corners: 8..mesh.corners.len(),
        }
    );
    // Still a mesh, and appending an empty one adds nothing.
    assert_eq!(rebuild(mesh.clone()), Ok(mesh.clone()));
    let joined = mesh.clone();
    mesh.append(&RenderMesh::default()).unwrap();
    assert_eq!(mesh, joined);
}

/// Adding to every element, for comparing offsets.
trait MapAdd {
    fn map_add(&self, by: u32) -> Vec<u32>;
}

impl MapAdd for Vec<u32> {
    fn map_add(&self, by: u32) -> Vec<u32> {
        self.iter().map(|x| x + by).collect()
    }
}

#[test]
fn append_refuses_more_than_the_limits() {
    let cube = cube();
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
        edge_vertices: vec![0; RenderMesh::MAX_EDGE_POINTS - cube.edge_vertices.len() + 1],
        ..RenderMesh::default()
    };
    assert_eq!(mesh.append_at(&cube, Vec3::ZERO), Err(MeshError::TooLarge));

    let mut mesh = RenderMesh {
        part_ends: vec![[0; 3]; RenderMesh::MAX_PARTS],
        ..RenderMesh::default()
    };
    assert_eq!(mesh.append_at(&cube, Vec3::ZERO), Err(MeshError::TooLarge));
    mesh.append_at(&RenderMesh::default(), Vec3::ZERO).unwrap();
}

#[test]
fn append_at_moves_positions_but_not_normals() {
    let cube = cube();
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
    edge.corners[1][0] = RenderMesh::MAX_POSITION;
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

/// A mesh with one triangle, one face, and its sides as two edges with
/// two corners, its fields to break.
fn triangle() -> RenderMesh {
    RenderMesh {
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        indices: vec![0, 1, 2],
        face_ends: vec![3],
        edge_vertices: vec![0, 1, 1, 2, 0],
        edge_ends: vec![2, 5],
        edge_faces: vec![[0, 0]; 2],
        corners: vec![[0.0; 3], [1.0, 0.0, 0.0]],
        edge_corners: vec![[0, 1], [1, 0]],
        part_ends: vec![[1, 2, 2]],
    }
}

/// `mesh` again, by way of [`RenderMesh::from_parts`].
fn rebuild(mesh: RenderMesh) -> Result<RenderMesh, MeshError> {
    RenderMesh::from_parts(mesh.into_parts())
}

#[test]
fn from_parts_takes_a_mesh() {
    assert_eq!(rebuild(triangle()), Ok(triangle()));
    assert_eq!(rebuild(RenderMesh::default()), Ok(RenderMesh::default()));
    let cube = cube();
    assert_eq!(rebuild(cube.clone()), Ok(cube));
    // An empty part.
    let empty = RenderMesh {
        part_ends: vec![[0; 3]],
        ..RenderMesh::default()
    };
    assert_eq!(rebuild(empty.clone()), Ok(empty));
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
fn from_parts_needs_indices_and_edge_vertices_to_refer_to_vertices() {
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
    mesh.edge_vertices[3] = u32::MAX;
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::OutOfRange {
            part: MeshPart::EdgeVertices,
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
    mesh.corners[1][0] = RenderMesh::MAX_POSITION;
    mesh.normals[1] = [0.0; 3];
    assert!(rebuild(mesh).is_ok());

    for bad in [f32::NAN, f32::INFINITY, -1.01 * RenderMesh::MAX_POSITION] {
        let mut mesh = triangle();
        mesh.positions[1][2] = bad;
        mesh.corners[1][2] = bad;
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
    let too_large = [
        MeshParts {
            positions: vec![[0.0; 3]; len],
            normals: vec![[0.0; 3]; len],
            ..MeshParts::default()
        },
        MeshParts {
            indices: vec![0; RenderMesh::MAX_INDICES + 1],
            ..MeshParts::default()
        },
        MeshParts {
            face_ends: vec![0; RenderMesh::MAX_FACES + 1],
            ..MeshParts::default()
        },
        MeshParts {
            edge_vertices: vec![0; RenderMesh::MAX_EDGE_POINTS + 1],
            ..MeshParts::default()
        },
        MeshParts {
            edge_ends: vec![0; RenderMesh::MAX_EDGE_POLYLINES + 1],
            ..MeshParts::default()
        },
        MeshParts {
            edge_faces: vec![[0; 2]; RenderMesh::MAX_EDGE_POLYLINES + 1],
            ..MeshParts::default()
        },
        MeshParts {
            corners: vec![[0.0; 3]; RenderMesh::MAX_CORNERS + 1],
            ..MeshParts::default()
        },
        MeshParts {
            edge_corners: vec![[0; 2]; RenderMesh::MAX_EDGE_POLYLINES + 1],
            ..MeshParts::default()
        },
        MeshParts {
            part_ends: vec![[0; 3]; RenderMesh::MAX_PARTS + 1],
            ..MeshParts::default()
        },
    ];
    for parts in too_large {
        assert_eq!(RenderMesh::from_parts(parts), Err(MeshError::TooLarge));
    }
}

#[test]
fn from_parts_needs_faces_of_whole_triangles() {
    let face_ends: [&[u32]; 5] = [&[], &[2], &[0, 3], &[3, 3], &[6]];
    for ends in face_ends {
        let mesh = RenderMesh {
            face_ends: ends.to_vec(),
            ..triangle()
        };
        assert_eq!(
            rebuild(mesh),
            Err(MeshError::Ends(MeshPart::FaceEnds)),
            "{ends:?}"
        );
    }
}

#[test]
fn from_parts_needs_edges_of_two_vertices_or_more() {
    let edge_ends: [&[u32]; 4] = [&[1, 5], &[2, 4], &[3, 2], &[2, 6]];
    for ends in edge_ends {
        let mesh = RenderMesh {
            edge_ends: ends.to_vec(),
            ..triangle()
        };
        assert_eq!(
            rebuild(mesh),
            Err(MeshError::Ends(MeshPart::EdgeEnds)),
            "{ends:?}"
        );
    }
}

#[test]
fn from_parts_needs_faces_and_corners_for_every_edge() {
    let mut mesh = triangle();
    mesh.edge_faces.pop();
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::Count {
            part: MeshPart::EdgeFaces,
            len: 1,
            edges: 2
        })
    );
    let mut mesh = triangle();
    mesh.edge_corners.push([0, 0]);
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::Count {
            part: MeshPart::EdgeCorners,
            len: 3,
            edges: 2
        })
    );
}

#[test]
fn from_parts_needs_parts_to_take_up_the_mesh() {
    let part_ends: [&[[u32; 3]]; 5] = [
        &[],
        &[[1, 2, 1]],
        &[[1, 2, 2], [1, 2, 3]],
        &[[1, 2, 2], [1, 1, 2]],
        &[[1, 2, 2], [0, 2, 2]],
    ];
    for ends in part_ends {
        let mesh = RenderMesh {
            part_ends: ends.to_vec(),
            ..triangle()
        };
        assert_eq!(
            rebuild(mesh),
            Err(MeshError::Ends(MeshPart::PartEnds)),
            "{ends:?}"
        );
    }
    // Parts may be empty, and a mesh of no faces, edges or corners needs
    // none.
    let mesh = RenderMesh {
        part_ends: vec![[0, 0, 0], [1, 2, 2], [1, 2, 2]],
        ..triangle()
    };
    assert!(rebuild(mesh).is_ok());
    let mesh = RenderMesh {
        positions: vec![[0.0; 3]],
        normals: vec![[0.0; 3]],
        ..RenderMesh::default()
    };
    assert!(rebuild(mesh).is_ok());
}

#[test]
fn from_parts_needs_edges_to_refer_to_faces_and_corners_of_their_part() {
    let mut mesh = triangle();
    mesh.edge_faces[1][1] = 1;
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::OutsidePart {
            part: MeshPart::EdgeFaces,
            id: 1
        })
    );
    let mut mesh = triangle();
    mesh.edge_corners[0][1] = 2;
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::OutsidePart {
            part: MeshPart::EdgeCorners,
            id: 2
        })
    );

    // Two triangles, each its own part: the second's edges can't refer
    // to the first's face or corners.
    let mut two = triangle();
    two.append(&triangle()).unwrap();
    assert!(rebuild(two.clone()).is_ok());
    let mut mesh = two.clone();
    mesh.edge_faces[2][0] = 0;
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::OutsidePart {
            part: MeshPart::EdgeFaces,
            id: 0
        })
    );
    let mut mesh = two;
    mesh.edge_corners[1][0] = 2;
    assert_eq!(
        rebuild(mesh),
        Err(MeshError::OutsidePart {
            part: MeshPart::EdgeCorners,
            id: 2
        })
    );
}

#[test]
fn from_parts_needs_corners_where_their_edges_end() {
    // Swapped: neither edge starts or ends at its corners.
    let mut mesh = triangle();
    mesh.edge_corners = vec![[1, 0], [0, 1]];
    assert_eq!(rebuild(mesh), Err(MeshError::Corners));
    // Moved.
    let mut mesh = triangle();
    mesh.corners[1][2] = 0.5;
    assert_eq!(rebuild(mesh), Err(MeshError::Corners));
    // Equal but not to the bit: -0 for 0.
    let mut mesh = triangle();
    mesh.corners[1][2] = -0.0;
    assert_eq!(rebuild(mesh), Err(MeshError::Corners));
    // Ending no edge.
    let mut mesh = triangle();
    mesh.corners.push([0.0, 1.0, 0.0]);
    mesh.part_ends[0][2] = 3;
    assert_eq!(rebuild(mesh), Err(MeshError::Corners));
    // A closed edge on one corner is fine.
    let mut mesh = triangle();
    mesh.edge_vertices = vec![0, 1, 2, 0];
    mesh.edge_ends = vec![4];
    mesh.edge_faces = vec![[0, 0]];
    mesh.corners.pop();
    mesh.edge_corners = vec![[0, 0]];
    mesh.part_ends = vec![[1, 1, 1]];
    assert!(rebuild(mesh).is_ok());
}
