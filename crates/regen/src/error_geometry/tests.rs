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

/// `failure`'s geometry as the cache makes it, on the bodies `operands`.
fn made(failure: &Failure, operands: [Option<BodyId>; 2]) -> Option<ErrorGeometry> {
    let failure = KernelFailure::new(failure.clone(), &Tolerance::DEFAULT);
    failure.geometry(operands).map(Arc::unwrap_or_clone)
}

/// Evidence of every kind: a flat patch, a quarter arc, two points, two
/// sketch curves and a face of each operand.
fn evidence() -> Evidence {
    let mut evidence = Evidence::default();
    evidence.add_patches([Patch::flat([DVec3::ZERO, DVec3::X, DVec3::Y]).unwrap()]);
    let arc = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 2.0, 0.0, 1.0).unwrap();
    evidence.add_curves([arc]);
    evidence.add_points([DVec3::new(0.0, 0.0, 3.0), DVec3::new(-1.0, 0.0, 0.0)]);
    evidence.add_sketch_curves([7, 9]);
    evidence.add_faces([(Operand::A, cap(1)), (Operand::B, cap(2))]);
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
    assert_eq!(made(&empty, [None, None]), None);
    // Faces of operands that aren't bodies name nothing either.
    let mut faces = Evidence::default();
    faces.add_faces([(Operand::B, cap(2))]);
    let faces = failure(faces);
    assert_eq!(made(&faces, [None, None]), None);
}

/// Each kind is made drawable: the patch tessellated, the arc and the
/// patch's sides flattened, the points as they are, the sketch curves
/// named, the faces pending until resolved; the box holds them all.
#[test]
fn evidence_is_made_drawable() {
    let geometry = made(&failure(evidence()), [None, None]).expect("geometry");
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
    evidence.add_points([DVec3::ZERO, DVec3::splat(far), DVec3::NAN]);
    let geometry = made(&failure(evidence), [None, None]).unwrap();
    assert_eq!(geometry.points(), [[0.0; 3]]);
    assert!(geometry.truncated());

    let mut evidence = Evidence::default();
    let bad = Patch {
        p: [DVec3::ZERO, DVec3::X, DVec3::Y],
        c: [DVec3::ZERO; 3],
        w: [-1.0; 3],
    };
    evidence.add_patches([bad]);
    evidence.add_curves([Conic3 {
        p0: DVec3::ZERO,
        c: DVec3::X,
        w: f64::NAN,
        p1: DVec3::Y,
    }]);
    let geometry = made(&failure(evidence), [None, None]);
    // Nothing drawable, but the mark says some was left out.
    assert_eq!(geometry, None);

    let mut evidence = Evidence::default();
    evidence.add_sketch_curves([1]);
    evidence.truncated = true;
    let geometry = made(&failure(evidence), [None, None]).unwrap();
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
    evidence.add_patches((0..MAX_EVIDENCE.patches).map(patch));
    let geometry = made(&failure(evidence), [None, None]).unwrap();
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
    evidence.add_faces([(Operand::A, cap(1)), (Operand::B, cap(1))]);
    let failure = failure(evidence);

    let mut geometry = made(&failure, [Some(BodyId::NEW), None]).unwrap();
    assert!(!geometry.is_empty());
    assert_eq!(geometry.bounds(), None);
    geometry.resolve(&mesh, &picking, Some);
    assert_eq!(geometry.faces(), [(BodyId::NEW, face)]);
    let bounds = geometry.bounds().unwrap();
    let range = mesh.face_indices(face as usize).unwrap();
    for &v in &mesh.indices()[range.clone()] {
        let p = Vec3::from(mesh.positions()[v as usize]);
        assert!(bounds.min.cmple(p).all() && p.cmple(bounds.max).all());
    }
    // Its triangles are drawn as the evidence's patches are: the model's,
    // corner for corner.
    let drawn = geometry.mesh();
    assert_eq!(drawn.indices().len(), range.len());
    for (&v, &w) in drawn.indices().iter().zip(&mesh.indices()[range.clone()]) {
        assert_eq!(drawn.positions()[v as usize], mesh.positions()[w as usize]);
        assert_eq!(drawn.normals()[v as usize], mesh.normals()[w as usize]);
    }
    // With its outline, as a patch's boundary is drawn: the square cap
    // once round, closed, through its four corners.
    let lines: Vec<&[[f32; 3]]> = geometry.lines().polylines().collect();
    let [line] = lines[..] else {
        panic!("{lines:?}");
    };
    assert_eq!(line.first(), line.last());
    let corners: Vec<[f32; 3]> = (mesh.indices()[range].iter())
        .map(|&v| mesh.positions()[v as usize])
        .collect();
    assert!(line.iter().all(|p| corners.contains(p)), "{line:?}");
    let mut round = line[1..].to_vec();
    round.sort_by(|p, q| p.partial_cmp(q).unwrap());
    round.dedup();
    assert_eq!(round.len(), 4, "{line:?}");

    // Its body isn't drawn: nothing is left.
    let mut gone = made(&failure, [Some(BodyId::NEW), None]).unwrap();
    gone.resolve(&mesh, &picking, |_| None);
    assert!(gone.is_empty());
}

/// What crosses comes back as it went, its box worked out again.
#[test]
fn parts_round_trip() {
    let (mesh, picking) = cube();
    let mut geometry = made(&failure(evidence()), [Some(BodyId::NEW), None]).unwrap();
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
    let mut geometry = made(&failure(evidence()), [Some(BodyId::NEW), None]).unwrap();
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

/// A failure the cache keeps keeps its geometry, made once: found
/// again, it's the very failure made, and with no operand faces pending
/// its geometry is the very one kept, so nothing is made again.
#[test]
fn a_cached_failure_keeps_its_geometry() {
    use crate::cache::{Cache, Keyer};

    let kept = || KernelFailure::new(failure(evidence()), &Tolerance::DEFAULT);
    let mut cache = Cache::default();
    let key = Keyer::new("boolean").finish();
    let made = cache.boolean(key, || Err(kept())).unwrap_err();
    let found = (cache.boolean(key, || unreachable!("found"))).unwrap_err();
    assert!(Arc::ptr_eq(&made, &found));
    assert_eq!(found.error, failure(evidence()).error);
    let tool = found.geometry([None, None]).unwrap();
    assert!(Arc::ptr_eq(&tool, &found.geometry([None, None]).unwrap()));
    assert_eq!(tool.sketch_curves(), [7, 9]);
    // On a body, its face is pending on it: geometry of its own.
    let on = found.geometry([Some(BodyId::NEW), None]).unwrap();
    assert!(!Arc::ptr_eq(&tool, &on));
    assert_eq!(on.pending, [(BodyId::NEW, cap(1))]);

    let key = Keyer::new("touches").finish();
    let made = cache.touches(key, || Err(kept())).unwrap_err();
    let found = (cache.touches(key, || unreachable!("found"))).unwrap_err();
    assert!(Arc::ptr_eq(&made, &found));
}

/// Of patches sharing sides, only the sides no two share are drawn: two
/// triangles making a square draw its four sides, not its diagonal.
#[test]
fn only_the_patches_boundary_is_drawn() {
    let (o, x, y, xy) = (DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(1.0, 1.0, 0.0));
    let mut evidence = Evidence::default();
    let flat = |p| Patch::flat(p).unwrap();
    evidence.add_patches([flat([o, x, xy]), flat([o, xy, y])]);
    let geometry = made(&failure(evidence), [None, None]).unwrap();
    assert_eq!(geometry.mesh().triangle_count(), 2);
    let sides: Vec<[[f32; 3]; 2]> = (geometry.lines().polylines())
        .map(|line| [line[0], line[line.len() - 1]])
        .collect();
    let f = |p: DVec3| p.as_vec3().to_array();
    assert_eq!(
        sides,
        [[f(o), f(x)], [f(x), f(xy)], [f(xy), f(y)], [f(y), f(o)]]
    );
}

/// Geometry with no operand face pending, shared with the cache, is the
/// very one kept after it is resolved on the model drawn, so an unchanged
/// failure is the same `Arc` from one answer to the next (the renderer
/// uploads it again only when its `Arc` changes).
#[test]
fn resolving_geometry_with_nothing_pending_keeps_it() {
    let (mesh, picking) = cube();
    let kept = KernelFailure::new(failure(evidence()), &Tolerance::DEFAULT);
    let tool = kept.geometry([None, None]).unwrap();
    let mut answered = Some(tool.clone());
    ErrorGeometry::resolve_shared(&mut answered, &mesh, &picking, Some);
    assert!(Arc::ptr_eq(&tool, answered.as_ref().unwrap()));

    // With a face pending it's resolved on a copy of its own.
    let mut answered = kept.geometry([Some(BodyId::NEW), None]);
    ErrorGeometry::resolve_shared(&mut answered, &mesh, &picking, Some);
    assert_eq!(answered.as_ref().unwrap().faces().len(), 1);
    assert!(tool.faces().is_empty());

    // Faces of a body not drawn leave nothing: dropped.
    let mut faces = Evidence::default();
    faces.add_faces([(Operand::A, cap(1))]);
    let kept = KernelFailure::new(failure(faces), &Tolerance::DEFAULT);
    let mut answered = kept.geometry([Some(BodyId::NEW), None]);
    ErrorGeometry::resolve_shared(&mut answered, &mesh, &picking, |_| None);
    assert_eq!(answered, None);
}

/// Evidence of operand faces alone, some left out, keeps its mark once
/// the faces are placed on bodies.
#[test]
fn faces_alone_keep_the_evidence_mark() {
    let (mesh, picking) = cube();
    let mut evidence = Evidence::default();
    evidence.add_faces([(Operand::A, cap(1))]);
    evidence.truncated = true;
    let mut geometry = made(&failure(evidence), [Some(BodyId::NEW), None]).unwrap();
    geometry.resolve(&mesh, &picking, Some);
    assert_eq!(geometry.faces().len(), 1);
    assert!(geometry.truncated());
}

/// Evidence that isn't drawn (a point past the bound, a patch its check
/// refuses) doesn't coarsen how the rest is drawn: the box the curves are
/// flattened relative to is that of what is drawn.
#[test]
fn what_is_left_out_does_not_coarsen_the_rest() {
    let arc = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 2.0, 0.0, 1.0).unwrap();
    let mut alone = Evidence::default();
    alone.add_curves([arc]);
    let alone = made(&failure(alone), [None, None]).unwrap();

    let far = f64::from(ErrorGeometry::MAX_POSITION) * 1e20;
    let mut with = Evidence::default();
    with.add_curves([arc]);
    with.add_points([DVec3::splat(far)]);
    with.add_patches([Patch {
        p: [DVec3::ZERO, DVec3::X * far, DVec3::Y * far],
        c: [DVec3::ZERO; 3],
        w: [-1.0; 3],
    }]);
    let with = made(&failure(with), [None, None]).unwrap();
    assert!(with.truncated());
    assert_eq!(with.lines(), alone.lines());
}

/// A patch given twice draws its boundary once; two patches back to
/// back (the same triangle both ways round) close on each other and draw
/// none, as a closed shell does.
#[test]
fn a_patch_given_twice_draws_its_boundary_once() {
    let (o, x, y) = (DVec3::ZERO, DVec3::X, DVec3::Y);
    let flat = |p| Patch::flat(p).unwrap();
    let mut twice = Evidence::default();
    twice.add_patches([flat([o, x, y]), flat([o, x, y])]);
    let twice = made(&failure(twice), [None, None]).unwrap();
    assert_eq!(twice.mesh().triangle_count(), 2);
    assert_eq!(twice.lines().ends().len(), 3);

    let mut back = Evidence::default();
    back.add_patches([flat([o, x, y]), flat([o, y, x])]);
    let back = made(&failure(back), [None, None]).unwrap();
    assert_eq!(back.mesh().triangle_count(), 2);
    assert!(back.lines().points().is_empty());
}

/// The box from the other side is that of what is drawn: a position no
/// triangle uses doesn't stretch it.
#[test]
fn unused_positions_do_not_stretch_the_box() {
    let (mesh, picking) = cube();
    let geometry = made(&failure(evidence()), [None, None]).unwrap();
    let mut parts = geometry.to_parts();
    parts.positions.push([1000.0; 3]);
    parts.normals.push([0.0, 0.0, 1.0]);
    let back = ErrorGeometry::from_parts(parts, &mesh, &picking).unwrap();
    assert_eq!(back.bounds(), geometry.bounds());
}

/// A face's outline is the sides of its triangles no other runs back
/// along, by position: a seam's twin vertices are one, a triangle of no
/// area left out; joined into one loop where it comes round.
#[test]
fn a_face_s_outline_runs_round_it_once() {
    // A unit square in two triangles, its diagonal's corners given twice
    // (a seam), and a sliver of zero width along its bottom.
    let positions = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, -0.0],
        [1.0, 1.0, 0.0],
    ];
    let corners = [0, 1, 2, 4, 5, 3, 0, 1, 1];
    let lines = super::outline(&positions, &corners);
    let at = |v: usize| Vec3::from(positions[v]);
    assert_eq!(lines, [vec![at(0), at(1), at(2), at(3), at(0)]]);
    // Nothing for no triangles.
    assert!(super::outline(&positions, &[]).is_empty());
}
