#![allow(
    clippy::disallowed_methods,
    reason = "std maths to build inputs: rotations"
)]

use glam::{DQuat, DVec3};

use super::*;
use crate::Tolerance;
use crate::mesh::tests::{TOL, add_round_octahedron};
use crate::mesh::{Mesh, MeshBuilder};
use crate::test_rng::Rng;

/// The closest pair among the 15 samples of each, as the search seeds.
fn grid_distance(a: &Patch, b: &Patch) -> f64 {
    let mut best = f64::INFINITY;
    for u in samples() {
        for v in samples() {
            best = best.min(a.eval(u).distance(b.eval(v)));
        }
    }
    best
}

/// The patches of `mesh`.
fn patches(mesh: &Mesh) -> Vec<Patch> {
    (0..mesh.tris().len()).map(|t| mesh.patch(t)).collect()
}

/// Whether some pair of a patch of `a` and one of `b` is found within
/// `limit`, and the closest such pair's [`grid_distance`].
fn any_within(a: &[Patch], b: &[Patch], limit: f64) -> Option<f64> {
    let mut found = None;
    for x in a {
        for y in b {
            if surfaces_within(x, y, limit) {
                let d = grid_distance(x, y);
                found = Some(found.map_or(d, |f: f64| f.min(d)));
            }
        }
    }
    found
}

#[test]
fn flat_triangles_are_found_within_the_limit_only() {
    let m = TOL.resolution();
    for offset in [DVec3::ZERO, DVec3::new(1e5, -3e4, 7e4)] {
        let a = Patch::flat([DVec3::ZERO, DVec3::X, DVec3::Y].map(|p| p + offset)).unwrap();
        // Parallel, above the middle of `a`, and turned round.
        let b = |gap: f64| {
            let p = [
                DVec3::new(0.3, 0.3, 0.0),
                DVec3::new(0.1, 0.4, 0.0),
                DVec3::new(0.4, 0.1, 0.0),
            ];
            Patch::flat(p.map(|p| p + DVec3::Z * gap + offset)).unwrap()
        };
        assert!(surfaces_within(&a, &b(0.5 * m), m), "{offset}");
        assert!(!surfaces_within(&a, &b(1.5 * m), m), "{offset}");
        assert!(!surfaces_within(&a, &b(1.01 * m), m), "{offset}");
    }
}

#[test]
fn a_touch_between_the_samples_is_found_by_the_search() {
    // Two unit cylinders side by side, at 30° round: their walls touch
    // along a line no sample of either triangle is on.
    let m = TOL.resolution();
    let cylinders = |gap: f64| {
        let a = Mesh::cylinder(DVec3::ZERO, 1.0, 2.0, 1, &TOL).unwrap();
        let (s, c) = 30f64.to_radians().sin_cos();
        let base = DVec3::new(c, s, 0.0) * (2.0 + gap);
        let b = Mesh::cylinder(base, 1.0, 2.0, 2, &TOL).unwrap();
        (patches(&a), patches(&b))
    };
    let (a, b) = cylinders(0.5 * m);
    let grid = any_within(&a, &b, m).expect("a witness");
    assert!(grid > 1e-3, "{grid}");
    for gap in [1.01, 1.5] {
        let (a, b) = cylinders(gap * m);
        assert_eq!(any_within(&a, &b, m), None, "{gap}");
    }
}

#[test]
fn round_surfaces_turned_and_moved_are_found_within_the_limit_only() {
    // Two round octahedra corner to corner or edge to edge, `gap` apart,
    // turned and moved anywhere within 3e5 (3e4 at the finest
    // tolerance, where rounding leaves less room): found at half the
    // margin, never past it.
    let mut rng = Rng::new(9);
    for (tol, extent) in [
        (TOL, 3e5),
        (Tolerance::new(Tolerance::MIN_FIT).unwrap(), 3e4),
    ] {
        let m = tol.resolution();
        let mut found = 0;
        for case in 0..40 {
            let turn = DQuat::from_axis_angle(rng.direction(), rng.range(0.0, 6.0));
            let at = rng.point(extent);
            let dir = if case % 2 == 0 {
                DVec3::X
            } else {
                DVec3::new(1.0, 1.0, 0.0).normalize()
            };
            let radius = rng.log_range(1e-2, 1e2);
            let pair = |gap: f64| {
                let mut a = MeshBuilder::new();
                add_round_octahedron(&mut a, DVec3::ZERO, radius, false);
                let mut b = MeshBuilder::new();
                add_round_octahedron(&mut b, dir * (2.0 * radius + gap), radius, false);
                let moved = |mesh: Mesh| -> Vec<Patch> {
                    patches(&mesh)
                        .into_iter()
                        .map(|p| Patch {
                            p: p.p.map(|x| turn * x + at),
                            c: p.c.map(|x| turn * x + at),
                            w: p.w,
                        })
                        .collect()
                };
                (moved(a.build().unwrap()), moved(b.build().unwrap()))
            };
            // The limit repair gives, rounding taken off.
            let (a, b) = pair(0.5 * m);
            let scale = a.iter().chain(&b).flat_map(Patch::hull);
            let scale = scale.fold(0.0, |s: f64, x| s.max(x.abs().max_element()));
            let limit = m - ROUNDING * scale;
            if limit > 0.5 * m {
                assert!(any_within(&a, &b, limit).is_some(), "{case}");
                found += 1;
            }
            let (a, b) = pair(1.01 * m);
            assert_eq!(any_within(&a, &b, limit), None, "{case}");
        }
        assert_eq!(found, 40);
    }
}

#[test]
fn degenerate_patches_and_limits_give_no_nan() {
    let m = TOL.resolution();
    let flat = Patch::flat([DVec3::ZERO, DVec3::X, DVec3::Y]).unwrap();
    // Every point the same, and a triangle on a line.
    let point = |z: f64| {
        let x = DVec3::new(0.2, 0.2, z);
        Patch::new([x; 3], [x; 3], [1.0; 3]).unwrap()
    };
    let line = |z: f64| {
        let p = [0.1, 0.5, 0.9].map(|t| DVec3::new(t, t, z));
        Patch::flat(p).unwrap()
    };
    for degenerate in [point(0.5 * m), line(0.5 * m)] {
        assert!(surfaces_within(&flat, &degenerate, m));
        assert!(surfaces_within(&degenerate, &flat, m));
    }
    for degenerate in [point(2.0 * m), line(2.0 * m)] {
        assert!(!surfaces_within(&flat, &degenerate, m));
        assert!(!surfaces_within(&degenerate, &flat, m));
        assert!(!surfaces_within(&degenerate, &degenerate, 0.0));
    }
    let touching = point(0.0);
    for limit in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!surfaces_within(&flat, &touching, limit), "{limit}");
    }
}

#[test]
fn the_search_stays_in_the_triangles() {
    // Two flat triangles in one plane, their nearest corners half the
    // limit apart, beyond the corners: the closest points are those
    // corners, and the planes past them don't count.
    let m = TOL.resolution();
    let a = Patch::flat([DVec3::ZERO, DVec3::X, DVec3::Y]).unwrap();
    let b = |gap: f64| {
        let o = DVec3::new(-gap, 0.0, 0.0);
        Patch::flat([o, o - DVec3::X, o - DVec3::Y + DVec3::Z]).unwrap()
    };
    assert!(surfaces_within(&a, &b(0.5 * m), m));
    assert!(!surfaces_within(&a, &b(1.01 * m), m));
    // Crossing planes, apart within the triangles.
    let c = Patch::flat([
        DVec3::new(2.0, 0.0, -1.0),
        DVec3::new(2.0, 0.0, 1.0),
        DVec3::new(2.0, 1.0, 0.0),
    ])
    .unwrap();
    assert!(!surfaces_within(&a, &c, m));
}

#[test]
fn inside_keeps_points_in_the_triangle() {
    for (u0, u1) in [
        (0.2, 0.3),
        (-1.0, 0.5),
        (0.7, 0.9),
        (2.0, -3.0),
        (f64::NAN, 0.5),
        (1e300, 1e300),
    ] {
        let u = inside(u0, u1);
        assert!(u.min_element() >= 0.0 && u.x + u.y <= 1.0, "{u0} {u1}: {u}");
        assert!(u.is_finite());
    }
    assert_eq!(inside(0.2, 0.3), DVec3::new(0.2, 0.3, 0.5));
}
