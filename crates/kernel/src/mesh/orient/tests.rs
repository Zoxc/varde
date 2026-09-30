#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{PI, TAU};

use glam::DVec2;

use super::*;
use crate::mesh::MeshBuilder;
use crate::mesh::tests::{TOL, joined};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, rect};
use crate::{Budget, Frame, KernelError, Loop, Op, Profile, Solid, boolean, extrude};

fn cube(min: [f64; 3], size: f64) -> Mesh {
    Mesh::cuboid(DVec3::from(min), DVec3::splat(size), 1, &TOL).unwrap()
}

fn cylinder(base: [f64; 3], r: f64, h: f64) -> Mesh {
    Mesh::cylinder(DVec3::from(base), r, h, 2, &TOL).unwrap()
}

fn solid(mesh: Mesh) -> Solid {
    Solid::new(mesh, &TOL).unwrap()
}

fn run(a: &Solid, b: &Solid, op: Op) -> Solid {
    boolean(a, b, op, &TOL, &Budget::DEFAULT).unwrap()
}

#[test]
fn the_nudge_is_independent_of_the_other_perturbations() {
    let d = NUDGE.dot(exact::T2.cross(exact::T3));
    assert!(d.abs() > 0.1, "{d}");
}

#[test]
fn corner_volumes_come_with_true_error_bounds() {
    // Where the bound tells a sign, it is the exact one: near-flat
    // tetrahedra, far from the origin and near it.
    let mut rng = crate::test_rng::Rng::new(3);
    for k in 0..4000 {
        let scale = [1e-3, 1.0, 1e5][k % 3];
        let o = rng.point(scale * 10.0);
        let [a, b] = [rng.point(scale), rng.point(scale)].map(|p| o + p);
        // On the plane through o, a and b, then nudged off it a little.
        let c = o
            + (a - o) * rng.range(-1.0, 1.0)
            + (b - o) * rng.range(-1.0, 1.0)
            + rng.point(scale * 1e-12);
        let (det, err) = orient3d(o, [a, b, c]);
        let exact = exact::sign(&Volume {
            start: Pt { p: o, n: None },
            corners: [a, b, c],
        });
        if det.abs() > err {
            assert_eq!(det.signum() as i8, exact, "{o} {a} {b} {c}");
        }
    }
}

#[test]
fn lune_bounds_hold() {
    // Every curved patch of these solids adds to its corner triangle's
    // volume no more than its bound says.
    let across = cylinder([0.0, 0.0, -2.0], 1.0, 4.0);
    let meshes = [
        cylinder([0.3, -0.2, 0.0], 2.0, 0.5),
        crate::mesh::tests::round_octahedron(DVec3::new(0.2, 0.1, 0.3)),
        crate::mesh::tests::half_cylinder(0.1, DVec3::ZERO),
        ring(1.0, 0.95, 64).into_mesh(),
        run(
            &solid(across),
            &solid(cylinder([-1.0, 0.0, 0.5], 0.8, 3.0)),
            Op::Union,
        )
        .into_mesh(),
    ];
    for mesh in &meshes {
        for t in 0..mesh.tris().len() {
            let patch = mesh.patch(t);
            // Measured from a corner, the triangle adds nothing, and of
            // the lunes' cones only the far edge's is left, inside the
            // hull too.
            let (own, size) = patch_volume(&patch, patch.p[0]);
            assert!(own.abs() <= size, "{t}: {own} {size}");
            let bound = lune_bound(&patch);
            assert!(own.abs() <= bound + 1e-15, "{t}: {own} {bound}");
        }
    }
    // The hull's area: a square with points inside and on its sides.
    let square = [
        DVec2::new(0.0, 0.0),
        DVec2::new(0.5, 0.5),
        DVec2::new(2.0, 2.0),
        DVec2::new(0.0, 2.0),
        DVec2::new(1.0, 0.0),
        DVec2::new(2.0, 0.0),
    ];
    assert_eq!(hull_area(square), 4.0);
    let line = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0].map(|x| DVec2::new(x, 2.0 * x));
    assert_eq!(hull_area(line), 0.0);
}

#[test]
fn solids_inside_out_are_refused() {
    for mesh in [cube([0.0; 3], 1.0), cylinder([0.0; 3], 1.0, 2.0)] {
        assert_eq!(mesh.check(&TOL), Ok(()));
        let turned = joined(&[(&mesh, true)]);
        assert_eq!(turned.check(&TOL), Err(CheckError::InsideOut(0)));
        assert_eq!(
            Solid::new(turned, &TOL),
            Err(KernelError::Invalid(CheckError::InsideOut(0)))
        );
    }
}

#[test]
fn shells_facing_the_wrong_way_for_where_they_lie_are_refused() {
    // A 10 mm cube (12 triangles) with a second shell: turned in, far off
    // (a box, a cylinder) or beside it; facing out inside it, clear of its
    // walls or anywhere; and a void inside a void. Each names the first
    // triangle of the shell at fault.
    let big = cube([0.0; 3], 10.0);
    let void = cube([1.0; 3], 8.0);
    let far = [
        (cube([50.0, 0.0, 0.0], 2.0), true, 12),
        (cylinder([50.0, 0.0, 0.0], 2.0, 3.0), true, 12),
        (cube([12.0; 3], 2.0), true, 12),
        (cube([1.0; 3], 2.0), false, 12),
        (cube([6.0; 3], 2.0), false, 12),
        (cylinder([5.0, 5.0, 1.0], 2.0, 3.0), false, 12),
    ];
    for (i, (shell, turned, t)) in far.iter().enumerate() {
        let mesh = joined(&[(&big, false), (shell, *turned)]);
        assert_eq!(mesh.check(&TOL), Err(CheckError::InsideOut(*t)), "case {i}");
    }
    // Inside a void, a void of its own: the second void is wrong.
    let inner = cube([3.0; 3], 2.0);
    let mesh = joined(&[(&big, false), (&void, true), (&inner, true)]);
    assert_eq!(mesh.check(&TOL), Err(CheckError::InsideOut(24)));
    // The wrong shell first: named by its lowest triangle.
    let mesh = joined(&[(&cube([1.0; 3], 2.0), false), (&big, false)]);
    assert_eq!(mesh.check(&TOL), Err(CheckError::InsideOut(0)));
}

#[test]
fn rays_along_edges_and_through_corners_are_decided() {
    // The void's first corner is (1, 1, 1); the ray from it along
    // (2, 3, 32) leaves the box round it through the middle of its top,
    // along the diagonal both of the top's triangles share, or through
    // its top corner. The perturbation decides it either way, and the
    // same shell facing out is refused.
    let void = cube([1.0; 3], 0.5);
    for (min, max) in [
        ([-1.0, -2.0, -3.0], [7.0, 10.0, 33.0]),
        ([-5.0, -5.0, -5.0], [3.0, 4.0, 33.0]),
    ] {
        let (min, max) = (DVec3::from(min), DVec3::from(max));
        let outer = Mesh::cuboid(min, max - min, 1, &TOL).unwrap();
        assert_eq!(
            void.verts()[void.tris()[0].halfedges[0].start as usize],
            DVec3::ONE
        );
        let mesh = joined(&[(&outer, false), (&void, true)]);
        assert_eq!(mesh.check(&TOL), Ok(()), "{min} {max}");
        let mesh = joined(&[(&outer, false), (&void, false)]);
        assert_eq!(
            mesh.check(&TOL),
            Err(CheckError::InsideOut(12)),
            "{min} {max}"
        );
    }
}

#[test]
fn rays_from_the_planes_of_faces_are_decided() {
    // An L-shaped prism with a box in its notch, the box's first corner
    // on the plane of the prism's bottom and inside its bounds: the ray
    // starts on the planes of the bottom's triangles.
    let block = solid(cube([0.0; 3], 10.0));
    let notch = solid(
        Mesh::cuboid(
            DVec3::new(5.0, 5.0, -1.0),
            DVec3::new(6.0, 6.0, 12.0),
            2,
            &TOL,
        )
        .unwrap(),
    );
    let l = run(&block, &notch, Op::Difference);
    let small = cube([6.0, 6.0, 0.0], 2.0);
    assert_eq!(
        small.verts()[small.tris()[0].halfedges[0].start as usize],
        DVec3::new(6.0, 6.0, 0.0)
    );
    let n = l.mesh().tris().len() as u32;
    let beside = joined(&[(l.mesh(), false), (&small, false)]);
    assert_eq!(beside.check(&TOL), Ok(()));
    let turned = joined(&[(l.mesh(), false), (&small, true)]);
    assert_eq!(turned.check(&TOL), Err(CheckError::InsideOut(n)));
}

#[test]
fn shells_facing_the_right_way_pass() {
    let big = cube([0.0; 3], 10.0);
    // A void, flat, and a body beside another.
    for (shell, turned) in [(cube([1.0; 3], 2.0), true), (cube([12.0; 3], 2.0), false)] {
        let mesh = joined(&[(&big, false), (&shell, turned)]);
        assert_eq!(mesh.check(&TOL), Ok(()));
    }
    // A void and an island in it, by hand: the winding numbers are 1, 0
    // and 1 going in.
    let island = joined(&[
        (&big, false),
        (&cube([1.0; 3], 8.0), true),
        (&cube([3.0; 3], 2.0), false),
    ]);
    assert_eq!(island.check(&TOL), Ok(()));

    // A box less a cylinder inside it: a curved void.
    let block = solid(big.clone());
    let pocket = run(
        &block,
        &solid(cylinder([5.0, 5.0, 2.0], 3.0, 4.0)),
        Op::Difference,
    );
    assert!((pocket.volume() - (1000.0 - 36.0 * PI)).abs() < 1e-9);
    // A pin joined into a void: an island.
    let hollow = run(&block, &solid(cube([1.0; 3], 8.0)), Op::Difference);
    let pinned = run(
        &hollow,
        &solid(cylinder([5.0, 5.0, 3.0], 1.0, 4.0)),
        Op::Union,
    );
    assert!((pinned.volume() - (1000.0 - 512.0 + 4.0 * PI)).abs() < 1e-9);
    // A ring with a disc in its hole, extruded at once.
    let profile = Profile {
        loops: vec![
            circle(DVec2::ZERO, 3.0, 0, false),
            circle(DVec2::ZERO, 2.0, 1, true),
            circle(DVec2::ZERO, 1.0, 2, false),
        ],
    };
    let ring = extrude(&profile, &Frame::XY, 0.0, 1.0, 3, &TOL, &Budget::DEFAULT).unwrap();
    assert!((ring.volume() - 6.0 * PI).abs() < 1e-9);
    for solid in [&pocket, &pinned, &ring] {
        let mesh = solid.mesh();
        assert_eq!(mesh.check(&TOL), Ok(()));
        // Their shells turned over, each alone and all at once, aren't.
        let shells = Shells::new(mesh);
        assert!(shells.len() >= 2);
        let turned = joined(&[(mesh, true)]);
        assert!(matches!(turned.check(&TOL), Err(CheckError::InsideOut(_))));
    }
}

#[test]
fn crossing_and_drilled_curved_solids_pass() {
    // The solids the booleans' own operand test was tried on.
    let upright = solid(cylinder([0.0, 0.0, -2.0], 1.0, 4.0));
    let across = extrude(
        &Profile {
            loops: vec![circle(DVec2::new(0.1, 0.2), 0.7, 0, false)],
        },
        &Frame {
            origin: DVec3::ZERO,
            x: DVec3::Y,
            y: DVec3::Z,
        },
        -2.0,
        2.0,
        3,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    let crossing = run(&upright, &across, Op::Union);
    let plate = extrude(
        &Profile {
            loops: vec![rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0)],
        },
        &Frame::XY,
        0.0,
        1.0,
        4,
        &TOL,
        &Budget::DEFAULT,
    )
    .unwrap();
    let drilled = run(
        &plate,
        &solid(cylinder([-1.5, -0.5, -1.0], 0.4, 3.0)),
        Op::Difference,
    );
    for (name, solid) in [("crossing", &crossing), ("drilled", &drilled)] {
        let mesh = solid.mesh();
        let integrated = mesh.check_counted(&TOL).unwrap();
        let patches = mesh.tris().len();
        assert!(
            2 * integrated < patches,
            "{name}: {integrated} of {patches}"
        );
        let turned = joined(&[(mesh, true)]);
        assert_eq!(turned.check(&TOL), Err(CheckError::InsideOut(0)), "{name}");
    }
}

/// A ring of height 1, its outside a circle of radius `outer` in four
/// quarter arcs, its hole a circle of radius `inner` in `n` arcs.
fn ring(outer: f64, inner: f64, n: usize) -> Solid {
    let hole = (0..n)
        .map(|k| {
            let at = |k: usize| {
                let a = -TAU * k as f64 / n as f64;
                DVec2::new(a.cos(), a.sin()) * inner
            };
            arc(DVec2::ZERO, at(k), at((k + 1) % n), 1)
        })
        .collect();
    let profile = Profile {
        loops: vec![
            circle(DVec2::ZERO, outer, 0, false),
            Loop { segments: hole },
        ],
    };
    extrude(&profile, &Frame::XY, 0.0, 1.0, 5, &TOL, &Budget::DEFAULT).unwrap()
}

#[test]
fn thin_curved_shells_are_told_by_integrating() {
    // A ring a twentieth thick, four quarter arcs outside and 64 inside
    // (split finer by the extrude where their hulls would meet): the
    // corner triangles' volume is off by more than the ring's, and the
    // patches that could move it most are integrated until it is told.
    let solid = ring(1.0, 0.95, 64);
    let want = PI * (1.0 - 0.95 * 0.95);
    assert!((solid.volume() - want).abs() < 1e-9, "{}", solid.volume());
    let mesh = solid.mesh();
    let integrated = mesh.check_counted(&TOL).unwrap();
    assert!(
        integrated > 0 && 4 * integrated < mesh.tris().len(),
        "{integrated}"
    );
    let turned = joined(&[(mesh, true)]);
    assert_eq!(turned.check(&TOL), Err(CheckError::InsideOut(0)));
}

#[test]
fn corner_volumes_floating_point_cant_tell_are_worked_out() {
    // A tetrahedron whose far corner is one unit off the plane of the
    // three others, each 2^52 from the origin: its six times volume is
    // 2^104, and the terms floating point sums to it are 2^156, whose
    // rounding hides it.
    let l = (1u64 << 52) as f64;
    let [a, b, c, d] = [
        DVec3::new(l, 0.0, 0.0),
        DVec3::new(0.0, l, 0.0),
        DVec3::new(0.0, 0.0, l),
        DVec3::new(1.0, 1.0, l - 1.0),
    ];
    for turned in [false, true] {
        let mut builder = MeshBuilder::new();
        let v = [a, b, c, d].map(|p| builder.vert(p));
        let f = builder.face(crate::mesh::tests::face(0, crate::mesh::Surface::Free));
        for [i, j, k] in [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]] {
            let tri = if turned { [i, k, j] } else { [i, j, k] };
            builder.tri(tri.map(|i| v[i]), f);
        }
        let mesh = builder.build().unwrap();
        let patches: Vec<Patch> = (0..4).map(|t| mesh.patch(t)).collect();
        // Flat: nothing to integrate.
        let shares: Vec<Share> = (0..4)
            .map(|t| Share {
                bound: 0.0,
                ..mesh.share(t, &patches[t as usize], a)
            })
            .collect();
        let (tet, err) = (shares[2].tet, shares[2].err);
        assert!(err > tet.abs(), "{tet} {err}");
        let before = exact::exact_count();
        let sign = shell_sign(&mesh, &[0, 1, 2, 3], &shares, &patches, a);
        assert_eq!(sign, (Some(!turned), 0));
        assert_eq!(exact::exact_count(), before + 1);
    }
}

#[test]
fn plates_with_twenty_voids_pass() {
    // Twenty pockets cut from inside a plate, boxes and cylinders by
    // turns, one boolean each.
    let mut plate =
        solid(Mesh::cuboid(DVec3::ZERO, DVec3::new(50.0, 40.0, 10.0), 1, &TOL).unwrap());
    let mut want = 20000.0;
    for k in 0..20 {
        let (i, j) = ((k % 5) as f64, (k / 5) as f64);
        let at = [3.0 + 10.0 * i, 3.0 + 10.0 * j, 2.0 + 0.25 * i];
        let (tool, volume) = if k % 2 == 0 {
            (cube(at, 4.0), 64.0)
        } else {
            (
                cylinder([at[0] + 2.0, at[1] + 2.0, at[2]], 2.0, 4.0),
                16.0 * PI,
            )
        };
        plate = run(&plate, &solid(tool), Op::Difference);
        want -= volume;
    }
    assert!(
        (plate.volume() - want).abs() < 1e-8,
        "{} {want}",
        plate.volume()
    );
    let shells = Shells::new(plate.mesh());
    assert_eq!(shells.len(), 21);
    // One void facing out instead: refused, naming it.
    let mesh = plate.mesh();
    let s = 7;
    let bad = shells.tris(s)[0];
    let mut builder = MeshBuilder::new();
    for &p in mesh.verts() {
        builder.vert(p);
    }
    for &f in mesh.faces() {
        builder.face(f);
    }
    for (t, tri) in mesh.tris().iter().enumerate() {
        let [a, b, c] = tri.halfedges.map(|h| h.start);
        let flip = shells.of[t] as usize == s;
        builder.tri(if flip { [a, c, b] } else { [a, b, c] }, tri.face);
        let patch = mesh.patch(t);
        for (i, (u, v)) in [(a, b), (b, c), (c, a)].into_iter().enumerate() {
            builder.edge(u, v, patch.c[i], patch.w[i]);
        }
    }
    let flipped = builder.build().unwrap();
    assert_eq!(flipped.check(&TOL), Err(CheckError::InsideOut(bad)));
}

#[test]
fn orientation_is_the_same_on_any_thread_count() {
    let big = cube([0.0; 3], 10.0);
    let island = joined(&[
        (&big, false),
        (&cube([1.0; 3], 8.0), true),
        (&cylinder([5.0, 5.0, 3.0], 1.0, 4.0), false),
        (&cylinder([20.0, 5.0, 3.0], 1.0, 4.0), false),
    ]);
    let ring = ring(1.0, 0.9, 64);
    let counts =
        assert_deterministic(|| (island.check_counted(&TOL), ring.mesh().check_counted(&TOL)));
    assert!(matches!(counts, (Ok(_), Ok(n)) if n > 0), "{counts:?}");
}
