use glam::Vec3;
use varde_kernel::MeshParts;

use super::*;

/// Two edges, the first three points round a corner, the second two, and
/// their three corners.
fn mesh() -> RenderMesh {
    let positions: Vec<[f32; 3]> = [Vec3::ZERO, Vec3::X, Vec3::ONE, Vec3::Z * 2.0, Vec3::Z * 3.0]
        .map(|p| p.to_array())
        .to_vec();
    RenderMesh::from_parts(MeshParts {
        normals: vec![[0.0, 0.0, 1.0]; positions.len()],
        positions,
        indices: vec![0, 1, 2],
        face_ends: vec![3],
        edge_vertices: vec![0, 1, 2, 3, 4],
        edge_ends: vec![3, 5],
        edge_faces: vec![[0, 0]; 2],
        corners: vec![[0.0; 3], [1.0; 3], [0.0, 0.0, 2.0], [0.0, 0.0, 3.0]],
        edge_corners: vec![[0, 1], [2, 3]],
        part_ends: vec![[1, 2, 4]],
    })
    .unwrap()
}

#[test]
fn outlined_and_selected_edges_are_apart_in_the_stream() {
    // The second edge both outlined and selected: its points twice, with a
    // point of no edge between, and one at either end.
    let highlights = Highlights {
        outlined: vec![0, 1],
        selected_edges: vec![1],
        vertices: Vec::new(),
    };
    let built = highlights.build(&mesh());
    assert_eq!(built.outlined, 1..6);
    assert_eq!(built.selected, 7..9);
    assert_eq!(built.edges.len(), 10);
    let edges: Vec<u32> = built.edges.iter().map(|point| point.edge).collect();
    let none = u32::MAX;
    assert_eq!(edges, [none, 0, 0, 0, 1, 1, none, 1, 1, none]);
}

#[test]
fn ids_the_mesh_has_not_and_plain_vertices_are_left_out() {
    let highlights = Highlights {
        outlined: vec![2, u32::MAX],
        selected_edges: vec![0],
        vertices: vec![
            Vertex {
                corner: 4,
                hovered: true,
                selected: false,
            },
            Vertex {
                corner: 1,
                hovered: true,
                selected: true,
            },
            // Neither hovered nor selected, it isn't drawn.
            Vertex {
                corner: 2,
                hovered: false,
                selected: false,
            },
        ],
    };
    let built = highlights.build(&mesh());
    assert!(built.outlined.is_empty());
    assert_eq!(built.selected.len(), 3);
    assert_eq!(built.vertices.len(), 1);
    assert_eq!(built.vertices[0].position, [1.0; 3]);
    assert_eq!(built.vertices[0].flags, HOVERED | SELECTED);
}

#[test]
fn outlined_edges_that_meet_are_joined_either_way_round() {
    // A square's four sides as edges, the second and fourth backwards,
    // and a lone edge apart: the square one closed polyline, the lone
    // edge its own.
    let square = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut positions = square.to_vec();
    positions.extend([[5.0, 0.0, 0.0], [6.0, 0.0, 0.0]]);
    let mesh = RenderMesh::from_parts(MeshParts {
        normals: vec![[0.0, 0.0, 1.0]; positions.len()],
        positions,
        indices: vec![0, 1, 2],
        face_ends: vec![3],
        edge_vertices: vec![0, 1, 2, 1, 2, 3, 0, 3, 4, 5],
        edge_ends: vec![2, 4, 6, 8, 10],
        edge_faces: vec![[0, 0]; 5],
        corners: vec![
            square[0],
            square[1],
            square[2],
            square[3],
            [5.0, 0.0, 0.0],
            [6.0, 0.0, 0.0],
        ],
        edge_corners: vec![[0, 1], [2, 1], [2, 3], [0, 3], [4, 5]],
        part_ends: vec![[1, 5, 6]],
    })
    .unwrap();
    let highlights = Highlights {
        // Out of order, with an edge twice.
        outlined: vec![1, 4, 2, 0, 3, 2],
        ..Highlights::default()
    };
    let built = highlights.build(&mesh);
    let edges: Vec<u32> = built.edges.iter().map(|point| point.edge).collect();
    let (none, only) = (u32::MAX, 1 << 31);
    // Round the square from its lowest edge, named by it, closed, so with
    // its neighbours either side; then the lone edge.
    assert_eq!(edges, [none, only, 0, 0, 0, 0, 0, only, 4, 4, none, none]);
    let points: Vec<[f32; 3]> = built.edges[2..7].iter().map(|p| p.position).collect();
    assert_eq!(
        points,
        [square[0], square[1], square[2], square[3], square[0]]
    );
    assert_eq!(built.outlined, 1..10);

    // Two sides, the one after the lowest's start before it.
    let highlights = Highlights {
        outlined: vec![0, 3],
        ..Highlights::default()
    };
    let built = highlights.build(&mesh);
    let points: Vec<[f32; 3]> = built.edges[1..4].iter().map(|p| p.position).collect();
    assert_eq!(points, [square[3], square[0], square[1]]);
    assert_eq!(built.outlined, 1..4);
}
