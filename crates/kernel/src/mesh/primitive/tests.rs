use glam::DVec3;

use super::super::tests::TOL;
use super::*;
use crate::mesh::CheckError;
use crate::test_rng::Rng;

/// Every patch within `1e-12` of its face's surface, relative to `scale`:
/// control points for planes, sampled points for quadrics.
fn assert_on_faces(mesh: &Mesh, scale: f64) {
    let mut rng = Rng::new(3);
    for t in 0..mesh.tris().len() {
        let patch = mesh.patch(t);
        let surface = mesh.faces()[mesh.tris()[t].face as usize].surface;
        let points: Vec<DVec3> = match surface {
            Surface::Quadric(_) => (0..20).map(|_| patch.eval(rng.bary())).collect(),
            _ => patch.hull().to_vec(),
        };
        for x in points {
            let off = surface.distance(x);
            assert!(off <= 1e-12 * scale, "patch {t} is {off} off its face");
        }
    }
}

#[test]
fn constructors_pass_check() {
    let mut rng = Rng::new(11);
    for _ in 0..50 {
        let at = rng.point(1e5);
        let size = DVec3::new(
            rng.log_range(1e-2, 1e3),
            rng.log_range(1e-2, 1e3),
            rng.log_range(1e-2, 1e3),
        );
        let cuboid = Mesh::cuboid(at, size, 7, &TOL).unwrap();
        assert_eq!(cuboid.check_faces(&TOL), Ok(()));
        assert_on_faces(&cuboid, at.abs().max_element() + size.max_element());
        assert_eq!((cuboid.verts().len(), cuboid.tris().len()), (8, 12));

        let (radius, height) = (size.x, size.y);
        let cylinder = Mesh::cylinder(at, radius, height, 7, &TOL).unwrap();
        assert_eq!(cylinder.check_faces(&TOL), Ok(()));
        assert_on_faces(&cylinder, at.abs().max_element() + radius.max(height));
        assert_eq!((cylinder.verts().len(), cylinder.tris().len()), (10, 16));
    }
}

#[test]
fn faces_are_named_as_an_extrude_names_them() {
    let names =
        |mesh: &Mesh| -> Vec<FacePart> { mesh.faces().iter().map(|f| f.name.part).collect() };
    let side = |curve, segment| FacePart::Side { curve, segment };
    let cuboid = Mesh::cuboid(DVec3::ZERO, DVec3::ONE, 5, &TOL).unwrap();
    assert_eq!(
        names(&cuboid),
        [
            FacePart::StartCap,
            FacePart::EndCap,
            side(0, 0),
            side(1, 0),
            side(2, 0),
            side(3, 0)
        ]
    );
    let cylinder = Mesh::cylinder(DVec3::ZERO, 1.0, 1.0, 5, &TOL).unwrap();
    assert_eq!(
        names(&cylinder),
        [
            FacePart::StartCap,
            FacePart::EndCap,
            side(0, 0),
            side(0, 1),
            side(0, 2),
            side(0, 3)
        ]
    );
    let all = cuboid.faces().iter().chain(cylinder.faces());
    assert!(all.clone().all(|f| f.name.feature == 5));
    // The first side faces -y, and the start cap -z.
    let normal = |surface: Surface| match surface {
        Surface::Plane { n, .. } => n,
        _ => DVec3::ZERO,
    };
    assert_eq!(normal(cuboid.faces()[2].surface), DVec3::NEG_Y);
    assert_eq!(normal(cylinder.faces()[0].surface), DVec3::NEG_Z);
}

#[test]
fn bad_parameters_are_refused() {
    let big = f64::from(MAX_COORD);
    for size in [
        DVec3::new(0.0, 1.0, 1.0),
        DVec3::new(1.0, -1.0, 1.0),
        DVec3::new(1.0, 1.0, f64::NAN),
        DVec3::new(f64::INFINITY, 1.0, 1.0),
        DVec3::new(1.0, 2.0 * big, 1.0),
    ] {
        assert!(matches!(
            Mesh::cuboid(DVec3::ZERO, size, 1, &TOL),
            Err(KernelError::Patch(_))
        ));
    }
    assert!(Mesh::cuboid(DVec3::splat(big - 1.0), DVec3::ONE, 1, &TOL).is_ok());
    assert!(Mesh::cuboid(DVec3::splat(big), DVec3::ONE, 1, &TOL).is_err());
    assert!(Mesh::cuboid(DVec3::splat(f64::NAN), DVec3::ONE, 1, &TOL).is_err());
    for (radius, height) in [
        (0.0, 1.0),
        (1.0, -1.0),
        (f64::NAN, 1.0),
        (1.0, f64::INFINITY),
    ] {
        assert!(matches!(
            Mesh::cylinder(DVec3::ZERO, radius, height, 1, &TOL),
            Err(KernelError::Patch(_))
        ));
    }
    assert!(Mesh::cylinder(DVec3::X * (big - 1.0), 2.0, 1.0, 1, &TOL).is_err());

    // Thinner than the resolution: top and bottom come too close.
    let thin = DVec3::new(1.0, 1.0, 0.1 * TOL.resolution());
    assert_eq!(
        Mesh::cuboid(DVec3::ZERO, thin, 1, &TOL),
        Err(KernelError::Invalid(CheckError::Hull(0, 2)))
    );
}

#[test]
fn long_thin_boxes_pass_check() {
    // A hundred resolutions thick and up to 2e7 times as long, where GJK
    // used to give up on the hulls of the top and a long side (from about
    // 1.2e7). Past about 3e7 GJK runs out of digits, and from about 7e7 a
    // corner's normal coefficient falls under the fold check's floor.
    let fine = Tolerance::new(Tolerance::MIN_FIT).unwrap();
    let at = DVec3::new(-147626.8648737723, -656331.4518769319, -870337.0748753285);
    let size = DVec3::new(
        0.00019860975513606848,
        7951.529175430838,
        0.00019860975513606848,
    );
    assert_eq!(Mesh::cuboid(at, size, 1, &fine).map(|_| ()), Ok(()));
    for fit in [Tolerance::MIN_FIT, 1e-3, Tolerance::MAX_FIT] {
        let tol = Tolerance::new(fit).unwrap();
        let thin = 100.0 * tol.resolution();
        for aspect in [1e2, 1e5, 1e7, 2e7] {
            let long = thin * aspect;
            for size in [
                DVec3::new(thin, long, thin),
                DVec3::new(long, thin, thin),
                DVec3::new(thin, thin, long),
                DVec3::new(long, long, thin),
            ] {
                let cuboid = Mesh::cuboid(DVec3::splat(-1e5), size, 1, &tol);
                assert_eq!(cuboid.map(|_| ()), Ok(()), "{fit} {size}");
            }
        }
    }
}
