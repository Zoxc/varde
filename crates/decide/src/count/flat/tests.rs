use super::*;

/// The net of the affine map `p + u·a + v·b` over `[0, 1]²`, weights `w`.
fn net(p: [f64; 3], a: [f64; 3], b: [f64; 3], w: f64) -> TileNet {
    std::array::from_fn(|k| {
        let (i, j) = ((k / 3) as f64 / 2.0, (k % 3) as f64 / 2.0);
        let x: [f64; 3] = std::array::from_fn(|c| p[c] + i * a[c] + j * b[c]);
        [x[0], x[1], x[2], w]
    })
}

#[test]
fn a_tile_is_a_plane_only_exactly() {
    let n = net([0.0, 0.0, 1.0], [1.0, 0.0, 0.5], [0.0, 2.0, -0.25], 1.0);
    assert!(plane_tile(&n));
    // Any orientation, the column along it included.
    assert!(plane_tile(&net(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        1.0
    )));
    // Weights don't move the points off the plane; a zero weight is no tile.
    assert!(plane_tile(&net(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        0.5
    )));
    assert!(!plane_tile(&net(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        0.0
    )));
    // One control point an ulp off the plane: no plane.
    let mut off = n;
    off[4][2] = f64::from_bits(off[4][2].to_bits() + 1);
    assert!(!plane_tile(&off));
    // A net on a line has no normal.
    assert!(!plane_tile(&net(
        [0.0; 3],
        [1.0, 1.0, 1.0],
        [2.0, 2.0, 2.0],
        1.0
    )));
    let mut nan = n;
    nan[3][0] = f64::NAN;
    assert!(!plane_tile(&nan));
}

#[test]
fn a_column_past_the_hull_misses_the_face() {
    use crate::count::testing::{Order, polygon};
    let mut order = Order::default();
    let f = polygon(
        &[[0.25, 0.2], [1.7, 0.3], [1.5, 1.6], [0.3, 1.4]],
        1e-4,
        &mut order,
    );
    let face = f.hit_face(&order).expect("a face");
    for (q, misses) in [
        ([1.9, 0.1, 0.0], true),
        ([0.9, -0.4, 5.0], true),
        // Past the hull by less than the near radius: not certified.
        ([1.7 + 1e-4, 0.3, 0.0], false),
        ([0.9, 0.8, -3.0], false),
        // Inside the hull, outside the face: the hits decide.
        ([0.26, 1.5, 0.0], false),
    ] {
        assert_eq!(column_misses(&face, q), misses, "{q:?}");
    }
    assert!(!column_misses(&face, [f64::NAN, 0.0, 0.0]));
}
