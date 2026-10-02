use glam::{Vec3, Vec3Swizzles};

use super::*;
use crate::mesh::tests::{TOL, tetrahedron};

#[test]
fn cuboid_normals_point_outwards() {
    let mesh = Solid::cuboid(DVec3::ZERO, DVec3::splat(2.0), 0, &Tolerance::DEFAULT)
        .unwrap()
        .tessellate(&Display::default())
        .unwrap();
    let center = Vec3::splat(1.0);

    assert_eq!(mesh.triangle_count(), 12);
    // Four corners to a side, each with the side's normal.
    assert_eq!(mesh.positions().len(), 24);
    // The box's twelve edges, not the sides' diagonals.
    assert_eq!(mesh.edge_count(), 12);
    for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
        let (p, n) = (Vec3::from(*p), Vec3::from(*n));
        assert!((p - center).dot(n) > 0.0);
        // Flat sides: every normal is an axis.
        assert_eq!(n.abs().max_element(), 1.0);
    }
}

#[test]
fn bounds_match_the_tessellation() {
    let solid = Solid::cuboid(
        DVec3::ZERO,
        DVec3::new(1.0, 2.0, 3.0),
        0,
        &Tolerance::DEFAULT,
    )
    .unwrap();
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
    // One part, with nothing in it.
    let drawn = solid.tessellate(&Display::default()).unwrap();
    assert_eq!(drawn.part_ends(), [[0; 3]]);
    assert_eq!(
        drawn.into_parts(),
        crate::MeshParts {
            part_ends: vec![[0; 3]],
            ..crate::MeshParts::default()
        }
    );
    assert_eq!(Solid::new(Mesh::default(), &TOL), Ok(solid));
}

#[test]
fn volumes_and_areas_are_analytic() {
    let (min, size) = (DVec3::new(-3.0, 1e4, 7.0), DVec3::new(2.0, 0.5, 30.0));
    let cuboid = Solid::cuboid(min, size, 0, &TOL).unwrap();
    let volume = size.x * size.y * size.z;
    assert!((cuboid.volume() - volume).abs() < 1e-12 * volume);
    let area = 2.0 * (size.x * size.y + size.y * size.z + size.z * size.x);
    assert!((cuboid.area() - area).abs() < 1e-12 * area);

    let pi = std::f64::consts::PI;
    for (r, h) in [(2.0, 5.0), (1e-2, 40.0), (300.0, 1.0)] {
        let cylinder = Solid::cylinder(DVec3::new(5.0, -4.0, 1e3), r, h, 1, &TOL).unwrap();
        let volume = pi * r * r * h;
        assert!(
            (cylinder.volume() - volume).abs() < 1e-12 * volume,
            "{} {volume}",
            cylinder.volume()
        );
        let area = 2.0 * pi * r * (r + h);
        assert!((cylinder.area() - area).abs() < 1e-12 * area);
    }
    assert_eq!(Solid::empty().volume(), 0.0);
    assert_eq!(Solid::empty().area(), 0.0);
}
