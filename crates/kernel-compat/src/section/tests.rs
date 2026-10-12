use glam::DVec3;

use super::*;
use crate::Tolerance;
use crate::mesh::{FaceKey, PartKey};

const TOL: Tolerance = Tolerance::DEFAULT;

fn plane(point: DVec3, normal: DVec3) -> Plane {
    Plane {
        point,
        normal: normal.normalize(),
    }
}

/// The triangles of the face of `solid` keyed `part` of feature 1.
fn face(solid: &Solid, part: PartKey) -> Vec<u32> {
    let topology = solid.topology();
    let key = FaceKey {
        feature: 1,
        part,
        instance: 0,
    };
    let region = topology.face(solid, &key, DVec3::ZERO).unwrap();
    topology.regions()[region as usize].tris.clone()
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

#[test]
fn a_box_face_cut_by_a_plane_is_a_segment() {
    let solid = Solid::cuboid(DVec3::ZERO, DVec3::splat(10.0), 1, &TOL).unwrap();
    let top = face(&solid, PartKey::EndCap);
    let cut = plane(DVec3::new(4.0, 0.0, 0.0), DVec3::X);
    let sections = face_section(&solid, &top, &cut, TOL.resolution()).unwrap();
    assert_eq!(sections.len(), 1, "{sections:?}");
    let section = &sections[0];
    assert!(!section.closed);
    for p in &section.places {
        assert!((p.x - 4.0).abs() < 1e-9 && (p.z - 10.0).abs() < 1e-9, "{p}");
    }
    let ys: Vec<f64> = section.places.iter().map(|p| p.y).collect();
    let (lo, hi) = ys
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &y| (lo.min(y), hi.max(y)));
    assert!(lo.abs() < 1e-9 && (hi - 10.0).abs() < 1e-9);
    // The top lying in a plane: refused.
    let level = plane(DVec3::new(0.0, 0.0, 10.0), DVec3::Z);
    assert_eq!(
        face_section(&solid, &top, &level, TOL.resolution()),
        Err(SectionError::InPlane)
    );
    // Missed: nothing.
    let away = plane(DVec3::new(20.0, 0.0, 0.0), DVec3::X);
    assert_eq!(
        face_section(&solid, &top, &away, TOL.resolution()),
        Ok(Vec::new())
    );
}

#[test]
fn a_cylinder_cut_across_square_and_aslant() {
    let solid = Solid::cylinder(DVec3::ZERO, 5.0, 10.0, 1, &TOL).unwrap();
    let wall = face(&solid, PartKey::Side { curve: 0 });
    let weld = TOL.resolution();
    let on_wall = |p: &DVec3| ((p.x * p.x + p.y * p.y).sqrt() - 5.0).abs() < 1e-9;

    // Square to the axis: one closed circle.
    let across = plane(DVec3::new(0.0, 0.0, 4.0), DVec3::Z);
    let sections = face_section(&solid, &wall, &across, weld).unwrap();
    assert_eq!(sections.len(), 1, "{sections:?}");
    assert!(sections[0].closed);
    assert!(
        sections[0]
            .places
            .iter()
            .all(|p| on_wall(p) && (p.z - 4.0).abs() < 1e-9)
    );

    // Through the axis: two lines up the wall. They're along the seams
    // between its quarters, which the patches on both sides trace: each
    // once, not there and back.
    let along = plane(DVec3::ZERO, DVec3::X);
    let sections = face_section(&solid, &wall, &along, weld).unwrap();
    assert_eq!(sections.len(), 2, "{sections:?}");
    for section in &sections {
        assert!(!section.closed);
        assert_eq!(section.places.len(), TRACE, "{section:?}");
        assert!(
            section
                .places
                .iter()
                .all(|p| p.x.abs() < 1e-9 && on_wall(p))
        );
    }

    // Aslant: one closed ellipse, every place on the wall and the plane.
    let aslant = plane(DVec3::new(0.0, 0.0, 5.0), DVec3::new(0.3, 0.0, 1.0));
    let sections = face_section(&solid, &wall, &aslant, weld).unwrap();
    assert_eq!(sections.len(), 1, "{sections:?}");
    assert!(sections[0].closed);
    for p in &sections[0].places {
        assert!(on_wall(p), "{p}");
        assert!(aslant.distance(*p).abs() < 1e-9, "{p}");
    }
}
