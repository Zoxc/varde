use glam::DVec3;

use super::*;
use crate::{Display, Solid, Tolerance};

/// Two cubes side by side, one part each.
fn cubes() -> RenderMesh {
    let cube = |at: DVec3| {
        Solid::cuboid(at, at + DVec3::ONE, 0, &Tolerance::DEFAULT)
            .unwrap()
            .tessellate(&Display::default())
            .unwrap()
    };
    let mut mesh = cube(DVec3::ZERO);
    mesh.append(&cube(DVec3::X * 2.0)).unwrap();
    mesh
}

#[test]
fn an_upload_has_each_parts_points_and_box() {
    let mesh = cubes();
    let upload = mesh.upload();
    assert_eq!(upload.parts().len(), 2);
    assert_eq!(upload.bounds().len(), 2);
    let [a, b] = [0, 1].map(|i| upload.bounds()[i].unwrap());
    assert_eq!((a.min, a.max), (Vec3::ZERO, Vec3::ONE));
    assert_eq!(b.min, Vec3::X * 2.0);
    let [first, second] = [&upload.parts()[0], &upload.parts()[1]];
    assert!(first[0].end <= second[0].start && second[0].end <= first[1].start);
    assert_eq!(upload.points().first().unwrap().edge, NO_EDGE);
    assert_eq!(upload.points().last().unwrap().edge, NO_EDGE);
}

#[test]
fn an_upload_is_built_again_once_the_mesh_changes() {
    let mut mesh = cubes();
    assert_eq!(mesh.upload().parts().len(), 2);
    mesh.append(&cubes()).unwrap();
    assert_eq!(mesh.upload().parts().len(), 4);
    // Left out of comparisons: it's of the rest.
    assert_eq!(
        mesh,
        RenderMesh::from_parts(mesh.clone().into_parts()).unwrap()
    );
}

#[test]
fn an_upload_round_trips_its_parts() {
    let mesh = cubes();
    let upload = mesh.upload();
    let back = MeshUpload::from_parts(
        &mesh,
        upload.points().to_vec(),
        upload.parts().to_vec(),
        upload.bounds().to_vec(),
    );
    assert_eq!(back.as_ref(), Ok(upload));
    let empty = RenderMesh::default();
    let none = MeshUpload::from_parts(&empty, Vec::new(), Vec::new(), Vec::new());
    assert_eq!(none.as_ref(), Ok(empty.upload()));
}

#[test]
fn uploads_not_of_the_mesh_are_refused() {
    let mesh = cubes();
    let upload = mesh.upload();
    let parts = || {
        (
            upload.points().to_vec(),
            upload.parts().to_vec(),
            upload.bounds().to_vec(),
        )
    };
    let check = |(points, parts, bounds)| MeshUpload::from_parts(&mesh, points, parts, bounds);
    let (points, mut ranges, bounds) = parts();
    ranges.pop();
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Lengths));
    let (mut points, ranges, bounds) = parts();
    points[1].edge = 1000;
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Points));
    let (mut points, ranges, bounds) = parts();
    points[1].position[0] = f32::NAN;
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Points));
    let (points, mut ranges, bounds) = parts();
    ranges.swap(0, 1);
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Ranges));
    let (points, mut ranges, bounds) = parts();
    ranges[1][1].end = points.len() as u32 + 1;
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Ranges));
    // Ending at the stream's last point, which the renderer reads past.
    let (points, mut ranges, bounds) = parts();
    ranges[1][1].end = points.len() as u32;
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Ranges));
    // Starting at its first, which it reads before.
    let (points, mut ranges, bounds) = parts();
    ranges[0][0].start = 0;
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Ranges));
    // Without its point of no edge at either end.
    for at in [0, upload.points().len() - 1] {
        let (mut points, ranges, bounds) = parts();
        points[at].edge = 0;
        assert_eq!(check((points, ranges, bounds)), Err(UploadError::Ranges));
    }
    let (points, ranges, mut bounds) = parts();
    bounds[0] = Some(Aabb {
        min: Vec3::ONE,
        max: Vec3::ZERO,
    });
    assert_eq!(check((points, ranges, bounds)), Err(UploadError::Bounds));
}
