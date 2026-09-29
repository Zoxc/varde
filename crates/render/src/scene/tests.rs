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
    view_projection(camera, 1.0, Some(bounds))
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
            let m = view_projection(&camera(projection), viewport.aspect(), None);
            assert!(m.is_finite(), "{projection:?} {width}x{height}");
        }
    }
}
