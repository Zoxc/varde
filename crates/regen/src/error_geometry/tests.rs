use glam::DVec3;
use varde_kernel::mesh::PartKey;
use varde_kernel::{BooleanError, KernelError, Solid, Tolerance};

use super::*;
use crate::picking::Drawn;

/// A failure carrying `evidence`.
fn failure(evidence: Evidence) -> Failure {
    Failure {
        error: KernelError::Boolean(BooleanError::NotManifold),
        evidence: Box::new(evidence),
    }
}

fn display() -> Display {
    Display::default()
}

/// Evidence of every kind: a flat patch, a quarter arc, two points, two
/// sketch curves and a face of each operand.
fn evidence() -> Evidence {
    let mut evidence = Evidence::default();
    evidence.patches([Patch::flat([DVec3::ZERO, DVec3::X, DVec3::Y]).unwrap()]);
    let arc = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 2.0, 0.0, 1.0).unwrap();
    evidence.curves([arc]);
    evidence.points([DVec3::new(0.0, 0.0, 3.0), DVec3::new(-1.0, 0.0, 0.0)]);
    evidence.sketch_curves([7, 9]);
    evidence.faces([(Operand::A, cap(1)), (Operand::B, cap(2))]);
    evidence
}

/// The start cap of `feature`'s solid.
fn cap(feature: u64) -> FaceKey {
    FaceKey {
        feature,
        part: PartKey::StartCap,
        instance: 0,
    }
}

#[test]
fn no_evidence_is_no_geometry() {
    let empty = failure(Evidence::default());
    assert_eq!(ErrorGeometry::new(&empty, [None, None], &display()), None);
    // Faces of operands that aren't bodies name nothing either.
    let mut faces = Evidence::default();
    faces.faces([(Operand::B, cap(2))]);
    let faces = failure(faces);
    assert_eq!(ErrorGeometry::new(&faces, [None, None], &display()), None);
}

/// Each kind is made drawable: the patch tessellated, the arc and the
/// patch's sides flattened, the points as they are, the sketch curves
/// named, the faces pending until resolved; the box holds them all.
#[test]
fn evidence_is_made_drawable() {
    let geometry =
        ErrorGeometry::new(&failure(evidence()), [None, None], &display()).expect("geometry");
    assert_eq!(geometry.mesh().triangle_count(), 1);
    assert_eq!(geometry.mesh().face_count(), 1);
    assert_eq!(geometry.mesh().part_ends().len(), 1);
    // The arc, then the patch's three sides.
    assert_eq!(geometry.lines().ends().len(), 4);
    let arc: Vec<[f32; 3]> = geometry.lines().polylines().next().unwrap().to_vec();
    assert!(arc.len() > 2);
    for p in &arc {
        assert!((Vec3::from(*p).length() - 2.0).abs() < 1e-5, "{p:?}");
    }
    assert_eq!(geometry.points(), [[0.0, 0.0, 3.0], [-1.0, 0.0, 0.0]]);
    assert_eq!(geometry.sketch_curves(), [7, 9]);
    assert!(geometry.faces().is_empty());
    assert!(!geometry.truncated());
    let bounds = geometry.bounds().unwrap();
    assert_eq!(bounds.min, Vec3::new(-1.0, 0.0, 0.0));
    assert_eq!(bounds.max.z, 3.0);
    assert!((bounds.max.x - 2.0).abs() < 1e-6 && bounds.max.y > 1.0);
}

/// What can't be drawn is left out and marked: a point past the bound,
/// a patch or curve the kernel's checks refuse. The evidence's own mark
/// carries over.
#[test]
fn what_cannot_be_drawn_is_left_out() {
    let far = f64::from(ErrorGeometry::MAX_POSITION) * 2.0;
    let mut evidence = Evidence::default();
    evidence.points([DVec3::ZERO, DVec3::splat(far), DVec3::NAN]);
    let geometry = ErrorGeometry::new(&failure(evidence), [None, None], &display()).unwrap();
    assert_eq!(geometry.points(), [[0.0; 3]]);
    assert!(geometry.truncated());

    let mut evidence = Evidence::default();
    let bad = Patch {
        p: [DVec3::ZERO, DVec3::X, DVec3::Y],
        c: [DVec3::ZERO; 3],
        w: [-1.0; 3],
    };
    evidence.patches([bad]);
    evidence.curves([Conic3 {
        p0: DVec3::ZERO,
        c: DVec3::X,
        w: f64::NAN,
        p1: DVec3::Y,
    }]);
    let geometry = ErrorGeometry::new(&failure(evidence), [None, None], &display());
    // Nothing drawable, but the mark says some was left out.
    assert_eq!(geometry, None);

    let mut evidence = Evidence::default();
    evidence.sketch_curves([1]);
    evidence.truncated = true;
    let geometry = ErrorGeometry::new(&failure(evidence), [None, None], &display()).unwrap();
    assert!(geometry.truncated());
    assert_eq!(geometry.bounds(), None);
}

/// Patches past the vertex bound are left out, marked, and the ones
/// before kept.
#[test]
fn patches_past_the_bound_are_left_out() {
    let mut evidence = Evidence::default();
    // Patches bulging hard, all in one place: each is cut finely next to
    // the box, so all of them take more samples than fit.
    let bulge = Patch::new(
        [DVec3::ZERO, DVec3::X * 10.0, DVec3::Y * 10.0],
        [
            DVec3::new(5.0, 0.0, 8.0),
            DVec3::new(5.0, 5.0, 8.0),
            DVec3::new(0.0, 5.0, 8.0),
        ],
        [0.7; 3],
    )
    .unwrap();
    let patch = |_| bulge;
    evidence.patches((0..MAX_EVIDENCE.patches).map(patch));
    let geometry = ErrorGeometry::new(&failure(evidence), [None, None], &display()).unwrap();
    let mesh = geometry.mesh();
    assert!(mesh.positions().len() <= ErrorGeometry::MAX_VERTICES);
    assert!(mesh.indices().len() <= ErrorGeometry::MAX_INDICES);
    assert!(geometry.lines().points().len() <= ErrorGeometry::MAX_LINE_POINTS);
    assert!(mesh.triangle_count() > 0);
    assert!(geometry.truncated());
}

/// A unit cube drawn as `body`'s, and its tables.
fn cube() -> (RenderMesh, Picking) {
    let solid = Solid::cuboid(DVec3::ZERO, DVec3::ONE, 1, &Tolerance::DEFAULT).unwrap();
    let drawn = Drawn::new(&solid, &solid.topology(), &Display::default()).unwrap();
    let mut picking = Picking::default();
    picking.append(BodyId::NEW, &drawn).unwrap();
    (drawn.mesh, picking)
}

/// An operand's face is resolved to the face of the mesh of the body
/// holding the operand's, by its key; one of an operand that isn't a
/// body, or of a body not drawn, names none. The box takes in the face.
#[test]
fn operand_faces_are_resolved_through_the_tables() {
    let (mesh, picking) = cube();
    let face = (picking.faces().iter())
        .position(|face| face.key == cap(1))
        .expect("the cube has a start cap") as u32;
    let mut evidence = Evidence::default();
    evidence.faces([(Operand::A, cap(1)), (Operand::B, cap(1))]);
    let failure = failure(evidence);

    let mut geometry = ErrorGeometry::new(&failure, [Some(BodyId::NEW), None], &display()).unwrap();
    assert!(!geometry.is_empty());
    assert_eq!(geometry.bounds(), None);
    geometry.resolve(&mesh, &picking, Some);
    assert_eq!(geometry.faces(), [(BodyId::NEW, face)]);
    let bounds = geometry.bounds().unwrap();
    let range = mesh.face_indices(face as usize).unwrap();
    for &v in &mesh.indices()[range] {
        let p = Vec3::from(mesh.positions()[v as usize]);
        assert!(bounds.min.cmple(p).all() && p.cmple(bounds.max).all());
    }

    // Its body isn't drawn: nothing is left.
    let mut gone = ErrorGeometry::new(&failure, [Some(BodyId::NEW), None], &display()).unwrap();
    gone.resolve(&mesh, &picking, |_| None);
    assert!(gone.is_empty());
}

/// What crosses comes back as it went, its box worked out again.
#[test]
fn parts_round_trip() {
    let (mesh, picking) = cube();
    let mut geometry =
        ErrorGeometry::new(&failure(evidence()), [Some(BodyId::NEW), None], &display()).unwrap();
    geometry.resolve(&mesh, &picking, Some);
    assert_eq!(geometry.faces().len(), 1);
    let parts = geometry.to_parts();
    let back = ErrorGeometry::from_parts(parts.clone(), &mesh, &picking).unwrap();
    assert_eq!(back, geometry);
    let bytes = postcard::to_stdvec(&parts).unwrap();
    assert_eq!(
        postcard::from_bytes::<GeometryParts>(&bytes).unwrap(),
        parts
    );
}

/// Parts that don't make geometry are refused, each for its reason.
#[test]
fn bad_parts_are_refused() {
    let (mesh, picking) = cube();
    let mut geometry =
        ErrorGeometry::new(&failure(evidence()), [Some(BodyId::NEW), None], &display()).unwrap();
    geometry.resolve(&mesh, &picking, Some);
    let good = geometry.to_parts();
    let refused = |change: &dyn Fn(&mut GeometryParts)| {
        let mut parts = good.clone();
        change(&mut parts);
        ErrorGeometry::from_parts(parts, &mesh, &picking).unwrap_err()
    };
    let far = ErrorGeometry::MAX_POSITION * 2.0;
    assert_eq!(
        refused(&|p| p.points[0][1] = f32::NAN),
        GeometryError::Position
    );
    assert_eq!(refused(&|p| p.points[0][1] = far), GeometryError::Position);
    assert_eq!(
        refused(&|p| p.positions[0][0] = f32::INFINITY),
        GeometryError::Position
    );
    assert_eq!(
        refused(&|p| p.line_points[0][2] = -far),
        GeometryError::Position
    );
    assert!(matches!(
        refused(&|p| p.normals[0][0] = f32::NAN),
        GeometryError::Mesh(_)
    ));
    assert!(matches!(
        refused(&|p| {
            p.normals.pop();
        }),
        GeometryError::Mesh(_)
    ));
    assert!(matches!(
        refused(&|p| p.indices[0] = 1000),
        GeometryError::Mesh(_)
    ));
    assert!(matches!(
        refused(&|p| {
            p.indices.pop();
        }),
        GeometryError::Mesh(_)
    ));
    assert!(matches!(
        refused(&|p| p.line_ends[0] = 1),
        GeometryError::Lines(_)
    ));
    assert!(matches!(
        refused(&|p| {
            p.line_ends.pop();
        }),
        GeometryError::Lines(_)
    ));
    let faces = mesh.face_count() as u32;
    assert_eq!(refused(&|p| p.faces[0].1 = faces), GeometryError::Face);
    assert_eq!(refused(&|p| p.faces[0].1 = u32::MAX), GeometryError::Face);
    let other = |p: &mut GeometryParts| {
        // A body the model doesn't draw this face as.
        let body: BodyId = postcard::from_bytes(&[0]).unwrap();
        p.faces[0].0 = body;
    };
    assert_eq!(refused(&other), GeometryError::Face);
    let many = vec![[0.0; 3]; ErrorGeometry::MAX_POINTS + 1];
    assert_eq!(
        refused(&|p| p.points = many.clone()),
        GeometryError::TooLarge
    );
}

/// Sequences past their bounds are refused as they're decoded.
#[test]
fn oversized_parts_are_refused_as_decoded() {
    let mut parts = GeometryParts {
        points: vec![[0.0; 3]; ErrorGeometry::MAX_POINTS + 1],
        ..GeometryParts::default()
    };
    let bytes = postcard::to_stdvec(&parts).unwrap();
    assert!(postcard::from_bytes::<GeometryParts>(&bytes).is_err());
    parts.points.clear();
    parts.faces = vec![(BodyId::NEW, 0); ErrorGeometry::MAX_FACES + 1];
    let bytes = postcard::to_stdvec(&parts).unwrap();
    assert!(postcard::from_bytes::<GeometryParts>(&bytes).is_err());
}

/// A failure the cache keeps keeps its evidence: found again, it's the
/// very failure made.
#[test]
fn a_cached_failure_keeps_its_evidence() {
    use crate::cache::{Cache, Keyer};

    let mut cache = Cache::default();
    let key = Keyer::new("boolean").finish();
    let made = cache.boolean(key, || Err(failure(evidence()))).unwrap_err();
    let found = (cache.boolean(key, || unreachable!("found"))).unwrap_err();
    assert!(Arc::ptr_eq(&made, &found));
    assert_eq!(*found.evidence, evidence());

    let key = Keyer::new("touches").finish();
    let made = cache.touches(key, || Err(failure(evidence()))).unwrap_err();
    let found = (cache.touches(key, || unreachable!("found"))).unwrap_err();
    assert!(Arc::ptr_eq(&made, &found));
}
