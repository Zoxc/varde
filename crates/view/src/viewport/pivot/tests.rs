use varde_render::View;

use super::*;
use crate::projection::top_camera;

/// A box from the origin to (2, 2, 2), tessellated.
fn cube() -> RenderMesh {
    let tol = varde_kernel::Tolerance::DEFAULT;
    let solid = varde_kernel::Solid::cuboid(DVec3::ZERO, DVec3::splat(2.0), 0, &tol);
    let display = varde_kernel::Display::new(&tol);
    solid.unwrap().tessellate(&display).unwrap()
}

/// [`top_camera`]'s viewport: a unit is 10 pixels, the origin in the
/// middle.
const SIZE: [f32; 2] = [200.0, 200.0];

/// The pixel over the world point (x, y) from the top.
fn over(x: f64, y: f64) -> DVec2 {
    DVec2::new(100.0 + x * 10.0, 100.0 - y * 10.0)
}

#[test]
fn a_click_on_the_model_picks_its_nearest_face() {
    let mesh = cube();
    let mut perspective = top_camera();
    perspective.set_projection(Projection::Perspective);
    for camera in [top_camera(), perspective] {
        // On the top face, on the ray through (1, 1.5) at the target: in
        // perspective nearer the middle, nearer the eye than that.
        let hit = pick(&mesh, &camera, SIZE, over(1.0, 1.5), None).unwrap();
        let shrunk = match camera.projection() {
            Projection::Orthographic => 1.0,
            Projection::Perspective => (camera.distance() - 2.0) / camera.distance(),
        };
        let expected = Vec3::new(shrunk, 1.5 * shrunk, 2.0);
        assert!(hit.abs_diff_eq(expected, 1e-4), "{hit}");
        // Off it, the grid on XY, which the target is on.
        let hit = pick(&mesh, &camera, SIZE, over(5.0, 1.0), None).unwrap();
        assert!(hit.abs_diff_eq(Vec3::new(5.0, 1.0, 0.0), 1e-4), "{hit}");
    }
    // From below, the bottom face.
    let mut camera = top_camera();
    camera.look_from(View::Bottom);
    camera.set_target(Vec3::ONE);
    let hit = pick(&mesh, &camera, SIZE, DVec2::new(100.0, 100.0), None);
    assert!(
        hit.unwrap().abs_diff_eq(Vec3::new(1.0, 1.0, 0.0), 1e-4),
        "{hit:?}"
    );
}

#[test]
fn an_orthographic_view_picks_the_model_behind_its_eye_too() {
    // The target well inside the box: the top face is behind where the
    // ray starts.
    let mesh = cube();
    let mut camera = top_camera();
    camera.set_target(Vec3::new(0.0, 0.0, 1.0));
    let hit = pick(&mesh, &camera, SIZE, over(1.0, 1.0), None).unwrap();
    assert!((hit.z - 2.0).abs() < 1e-4, "{hit}");
}

#[test]
fn the_grid_is_picked_only_where_it_shows() {
    // In perspective from the front, the eye a little above the XY plane,
    // which runs off to a horizon 24 pixels above the middle; the grid
    // fades out 120 units (6 view heights) from the target, about 19
    // pixels above it.
    let mut camera = top_camera();
    camera.set_projection(Projection::Perspective);
    camera.look_from(View::Front);
    camera.orbit(0.0, 0.1);
    for y in [150.0, 85.0] {
        let near = pick(
            &RenderMesh::default(),
            &camera,
            SIZE,
            DVec2::new(100.0, y),
            None,
        );
        assert!(near.is_some_and(|hit| hit.z.abs() < 1e-3), "{y}: {near:?}");
    }
    let far = pick(
        &RenderMesh::default(),
        &camera,
        SIZE,
        DVec2::new(100.0, 78.0),
        None,
    );
    assert_eq!(far, None);
    // Above the horizon it meets nothing.
    let above = pick(
        &RenderMesh::default(),
        &camera,
        SIZE,
        DVec2::new(100.0, 20.0),
        None,
    );
    assert_eq!(above, None);
}

#[test]
fn in_a_sketch_a_click_off_the_model_picks_its_plane() {
    let placement = OriginPlane::XY.placement();
    let hit = pick(
        &RenderMesh::default(),
        &top_camera(),
        SIZE,
        over(5.0, 1.0),
        Some(placement),
    );
    assert!(
        hit.unwrap().abs_diff_eq(Vec3::new(5.0, 1.0, 0.0), 1e-4),
        "{hit:?}"
    );
    // The model still comes first.
    let hit = pick(
        &cube(),
        &top_camera(),
        SIZE,
        over(1.0, 1.0),
        Some(placement),
    );
    assert!((hit.unwrap().z - 2.0).abs() < 1e-4, "{hit:?}");
}
