use glam::Vec3;

use super::*;
use crate::{Projection, View, Viewport};

fn camera(projection: Projection) -> Camera {
    let mut camera = Camera::default();
    camera.set_projection(projection);
    camera
}

/// NDC depth of `p`, which is drawn if it is within `0..=1`.
fn depth(camera: &Camera, bounds: Aabb, p: Vec3) -> f32 {
    view_projection(camera, 1.0, &GridPlane::XY, Some(bounds))
        .project_point3(p)
        .z
}

#[test]
fn depth_range_covers_far_bounds() {
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = camera(projection);
        camera.look_from(View::Front);
        let target = camera.target();
        let far = target + Vec3::Y * 1000.0 * camera.distance();
        let bounds = Aabb {
            min: target,
            max: far,
        };
        for p in [target, far] {
            let z = depth(&camera, bounds, p);
            assert!((0.0..=1.0).contains(&z), "{projection:?} {p}: {z}");
        }
    }
}

#[test]
fn zooming_in_keeps_bounds_in_depth_range() {
    let bounds = Aabb {
        min: Vec3::ZERO,
        max: Vec3::splat(100.0),
    };
    let mut camera = Camera::default();
    camera.set_target(Vec3::splat(50.0));
    for _ in 0..8 {
        camera.zoom(0.1);
        for p in [
            bounds.min,
            bounds.max,
            bounds.min.with_z(100.0),
            bounds.max.with_z(0.0),
        ] {
            let z = depth(&camera, bounds, p);
            assert!((0.0..=1.0).contains(&z), "{} {p}: {z}", camera.distance());
        }
    }
}

#[test]
fn degenerate_viewports_project_finitely() {
    for (width, height) in [(0.0, 0.0), (0.0, 100.0), (100.0, 0.0), (f32::NAN, -1.0)] {
        let viewport = Viewport {
            width,
            height,
            ..Default::default()
        };
        for projection in [Projection::Orthographic, Projection::Perspective] {
            let m = view_projection(&camera(projection), viewport.aspect(), &GridPlane::XY, None);
            assert!(m.is_finite(), "{projection:?} {width}x{height}");
        }
    }
}

#[test]
fn grid_planes_are_made_orthonormal() {
    let plane = GridPlane::new(Vec3::ONE, Vec3::X * 2.0, Vec3::new(1.0, 0.0, 3.0)).unwrap();
    assert_eq!(plane.origin(), Vec3::ONE);
    assert_eq!(plane.x(), Vec3::X);
    assert_eq!(plane.y(), Vec3::Z);
    assert_eq!(plane.normal(), Vec3::NEG_Y);
    assert_eq!(
        plane.project(Vec3::new(5.0, 7.0, -2.0)),
        Vec3::new(5.0, 1.0, -2.0)
    );
    assert_eq!(GridPlane::default(), GridPlane::XY);
    assert_eq!(GridPlane::XY.normal(), Vec3::Z);
}

#[test]
fn grid_planes_must_be_planes_within_reach() {
    let far = Vec3::X * Camera::EXTENT * 2.0;
    for (origin, x, y) in [
        (far, Vec3::X, Vec3::Y),
        (Vec3::NAN, Vec3::X, Vec3::Y),
        (Vec3::ZERO, Vec3::ZERO, Vec3::Y),
        (Vec3::ZERO, Vec3::X, Vec3::X * -3.0),
        (Vec3::ZERO, Vec3::X, Vec3::INFINITY),
    ] {
        assert_eq!(GridPlane::new(origin, x, y), None, "{origin} {x} {y}");
    }
}

#[test]
fn depth_range_covers_the_grid_on_its_plane() {
    // Looking at the front of a plane far behind the target, which a grid
    // on XY would never reach.
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let mut camera = camera(projection);
        camera.look_from(View::Front);
        let grid = GridPlane::new(Vec3::Y * 500.0, Vec3::X, Vec3::Z).unwrap();
        let m = view_projection(&camera, 1.0, &grid, None);
        let height = camera.view_height();
        for p in [
            grid.origin(),
            grid.origin() + Vec3::X * height * GRID_FADE_HEIGHTS,
        ] {
            let z = m.project_point3(p).z;
            assert!((0.0..=1.0).contains(&z), "{projection:?} {p}: {z}");
        }
    }
}

#[test]
fn depth_range_covers_every_bounds() {
    let camera = camera(Projection::Orthographic);
    let near = Aabb {
        min: Vec3::ZERO,
        max: Vec3::ONE,
    };
    let far = Aabb {
        min: Vec3::splat(-900.0),
        max: Vec3::splat(-800.0),
    };
    let m = view_projection(&camera, 1.0, &GridPlane::XY, [near, far]);
    for p in [near.min, near.max, far.min, far.max] {
        let z = m.project_point3(p).z;
        assert!((0.0..=1.0).contains(&z), "{p}: {z}");
    }
}
