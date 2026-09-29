use glam::{Vec3, Vec3Swizzles};

use super::*;
use crate::Shape;
use crate::mesh::tests::{TOL, tetrahedron};

#[test]
fn cuboid_normals_point_outwards() {
    let mesh = Shape::cuboid(Vec3::splat(2.0))
        .build()
        .unwrap()
        .tessellate(&Display::default())
        .unwrap();
    let center = Vec3::splat(1.0);

    assert_eq!(mesh.triangle_count(), 12);
    // Four corners to a side, each with the side's normal.
    assert_eq!(mesh.positions().len(), 24);
    // The box's twelve edges, not the sides' diagonals.
    assert_eq!(mesh.edges().len(), 12);
    for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
        let (p, n) = (Vec3::from(*p), Vec3::from(*n));
        assert!((p - center).dot(n) > 0.0);
        // Flat sides: every normal is an axis.
        assert_eq!(n.abs().max_element(), 1.0);
    }
}

#[test]
fn bounds_match_the_tessellation() {
    let solid = Shape::cuboid(Vec3::new(1.0, 2.0, 3.0)).build().unwrap();
    let mesh = solid.tessellate(&Display::default()).unwrap();
    assert_eq!(solid.bounds(), mesh.bounds());
}

#[test]
fn bounds_hold_a_curved_solid() {
    let solid = Solid::cylinder(DVec3::new(1.0, 2.0, 3.0), 2.0, 5.0, 1, &TOL).unwrap();
    let b = solid.bounds().unwrap();
    // The box of the square the quarter arcs' control points make.
    assert_eq!(b.min, Vec3::new(-1.0, 0.0, 3.0));
    assert_eq!(b.max, Vec3::new(3.0, 4.0, 8.0));
    let mesh = solid.tessellate(&Display::default()).unwrap();
    let t = mesh.bounds().unwrap();
    assert!(t.min.cmpge(b.min).all() && t.max.cmple(b.max).all());
    // The circle reaches its extremes at the ends of the arcs.
    assert_eq!(t.min.xy(), b.min.xy());
    assert_eq!(t.max.xy(), b.max.xy());
}

#[test]
fn new_checks_the_mesh() {
    let solid = Solid::new(tetrahedron(DVec3::ZERO), &TOL).unwrap();
    assert!(!solid.is_empty());
    assert_eq!(solid.clone().into_mesh(), *solid.mesh());

    // The same tetrahedron twice over, overlapping itself.
    let mut builder = crate::mesh::MeshBuilder::new();
    crate::mesh::tests::add_tetrahedron(&mut builder, DVec3::ZERO);
    crate::mesh::tests::add_tetrahedron(&mut builder, DVec3::splat(0.1));
    let overlapping = builder.build().unwrap();
    assert!(matches!(
        Solid::new(overlapping, &TOL),
        Err(KernelError::Invalid(_))
    ));
}

#[test]
fn the_empty_solid_draws_nothing() {
    let solid = Solid::empty();
    assert!(solid.is_empty());
    assert_eq!(solid.bounds(), None);
    assert_eq!(
        solid.tessellate(&Display::default()),
        Ok(RenderMesh::default())
    );
    assert_eq!(Solid::new(Mesh::default(), &TOL), Ok(solid));
}
