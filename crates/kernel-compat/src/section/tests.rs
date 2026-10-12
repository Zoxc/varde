use glam::DVec3;

use super::*;

fn plane(point: DVec3, normal: DVec3) -> Plane {
    Plane {
        point,
        normal: normal.normalize(),
    }
}

#[test]
fn a_conic_crosses_a_plane_at_its_roots() {
    let line = Conic3::line(DVec3::new(0.0, 0.0, -1.0), DVec3::new(0.0, 0.0, 3.0)).unwrap();
    let z = plane(DVec3::ZERO, DVec3::Z);
    assert_eq!(conic_crossings(&line, &z, 1e-9), Ok(vec![0.25]));
    // A half turn of a circle about z, standing in the xz plane, through
    // z = 0.5 twice.
    let arc = Conic3::arc_between(
        DVec3::ZERO,
        1.0,
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.0, 0.0, 1.0),
    )
    .unwrap();
    let half = plane(DVec3::new(0.0, 0.0, 0.5), DVec3::Z);
    let found = crossings(&[arc], &half, 1e-9).unwrap();
    assert_eq!(found.len(), 1);
    assert!((found[0].length() - 1.0).abs() < 1e-12);
    assert!((found[0].z - 0.5).abs() < 1e-12);
    // In the plane: no crossing to find.
    let flat = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    assert_eq!(crossings(&[flat], &z, 1e-9), Err(SectionError::InPlane));
    // Missing it: none.
    let above = Conic3::line(DVec3::Z, DVec3::new(1.0, 0.0, 2.0)).unwrap();
    assert_eq!(crossings(&[above], &z, 1e-9), Ok(Vec::new()));
}

#[test]
fn sampling_a_chain_keeps_its_ends() {
    let a = Conic3::line(DVec3::ZERO, DVec3::X).unwrap();
    let b = Conic3::line(DVec3::X, DVec3::new(1.0, 1.0, 0.0)).unwrap();
    let places = sample(&[a, b], 4, false).unwrap();
    assert_eq!(places.len(), 9);
    assert_eq!(places[0], DVec3::ZERO);
    assert_eq!(places[8], DVec3::new(1.0, 1.0, 0.0));
    assert_eq!(sample(&[a, b], 4, true).unwrap().len(), 8);
}
