use super::*;

/// The tetrahedron on the origin and the three unit axes, facing out.
fn tetrahedron() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let triangles = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
    (positions, triangles)
}

#[test]
fn a_tetrahedron_passes_and_its_breaks_are_named() {
    let (positions, triangles) = tetrahedron();
    let mesh = ManifoldMesh::new([0.0; 3], positions.clone(), triangles.clone()).unwrap();
    assert_eq!(mesh.volume(), 1.0 / 6.0);

    let inside_out: Vec<[u32; 3]> = triangles.iter().map(|&[a, b, c]| [a, c, b]).collect();
    assert_eq!(
        ManifoldMesh::new([0.0; 3], positions.clone(), inside_out),
        Err(ManifoldError::InsideOut)
    );
    assert_eq!(
        ManifoldMesh::new([0.0; 3], positions.clone(), Vec::new()),
        Err(ManifoldError::Empty)
    );
    assert_eq!(
        ManifoldMesh::new([0.5, 0.0, 0.0], positions.clone(), triangles.clone()),
        Err(ManifoldError::Origin)
    );
    let mut bad_index = triangles.clone();
    bad_index[2][1] = 9;
    assert_eq!(
        ManifoldMesh::new([0.0; 3], positions.clone(), bad_index),
        Err(ManifoldError::Index(2))
    );
    // 0.1 isn't an `f32` value.
    let mut off_grid = positions.clone();
    off_grid[3][2] = 0.1;
    assert_eq!(
        ManifoldMesh::new([0.0; 3], off_grid, triangles.clone()),
        Err(ManifoldError::Position(3))
    );
    // The apex pulled down onto the line through two corners.
    let mut flat = positions;
    flat[3] = [2.0, -1.0, 0.0];
    assert!(matches!(
        ManifoldMesh::new([0.0; 3], flat, triangles),
        Err(ManifoldError::Degenerate(_))
    ));
}

#[test]
fn collinear_is_decided_exactly() {
    let p = |x: f32, y: f32| DVec3::new(f64::from(x), f64::from(y), 0.0);
    // On a line, with coordinates far apart in size.
    let t = 1e-20f32;
    assert!(cross_component_is_zero(
        [p(0.0, 0.0), p(1e6, 1.0), p(2e6, 2.0)],
        2
    ));
    assert!(cross_component_is_zero(
        [p(t, t), p(1.0, 1.0), p(2e6, 2e6)],
        2
    ));
    // Off the line by `t`, which the differences in `f64` round away:
    // `(p1 − p0) × (p2 − p0)` is `−2t + t = −t` exactly, but `0` from the
    // rounded differences.
    let tri = [p(t, 0.0), p(1e6, 1.0), p(2e6, 2.0)];
    let (u, v) = (tri[1] - tri[0], tri[2] - tri[0]);
    assert_eq!(u.x * v.y - u.y * v.x, 0.0);
    assert!(!cross_component_is_zero(tri, 2));
    // The other components of a triangle in `z = 0` are zero outright.
    assert!(cross_component_is_zero(tri, 0));
    assert!(cross_component_is_zero(tri, 1));
}

#[test]
fn the_expansion_sums_exactly() {
    let mut e = Vec::new();
    for b in [1e300, 1.0, -1e300, 1e-300, -1.0, -1e-300] {
        grow(&mut e, b);
    }
    assert!(e.iter().all(|&x| x == 0.0), "{e:?}");
    let mut e = Vec::new();
    for b in [1e16, 1.0, -1e16] {
        grow(&mut e, b);
    }
    assert_eq!(e.iter().sum::<f64>(), 1.0);
}
