use super::*;

#[test]
fn cuboid_normals_point_outwards() {
    let mesh = Shape::cuboid(Vec3::splat(2.0))
        .build()
        .unwrap()
        .tessellate();
    let center = Vec3::splat(1.0);

    assert_eq!(mesh.triangle_count(), 12);
    for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
        assert!((Vec3::from(*p) - center).dot(Vec3::from(*n)) > 0.0);
    }
}

#[test]
fn bounds_match_the_tessellation() {
    let solid = Shape::cuboid(Vec3::new(1.0, 2.0, 3.0)).build().unwrap();
    assert_eq!(Some(solid.bounds()), solid.tessellate().bounds());
}
