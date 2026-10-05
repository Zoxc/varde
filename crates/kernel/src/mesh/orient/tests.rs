#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use glam::DVec2;

use super::*;
use crate::Stripped;
use crate::mesh::MeshBuilder;
use crate::mesh::tests::{TOL, joined};
use crate::par::assert_deterministic;
use crate::profile::tests::{arc, circle, rect};
use crate::{Budget, Frame, KernelError, Loop, Op, Profile, Solid, Tolerance, boolean, extrude};

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
        mushroom(5.0, 0.5, 0.5, 10.0, false),
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
            let (flux, size) = patch_volume(&patch, patch.p[0]);
            assert!(flux.abs() <= size, "{t}: {flux} {size}");
            let own = flux + lune_cones(&patch, patch.p[0]).0;
            let bound = lune_bound(&patch);
            assert!(own.abs() <= bound + 1e-15, "{t}: {own} {bound}");
            // And it is the same measured from anywhere.
            for o in [DVec3::new(7.0, -3.0, 20.0), DVec3::new(-40.0, 5.0, -9.0)] {
                let (flux, size) = patch_volume(&patch, o);
                let (cones, cone_size) = lune_cones(&patch, o);
                let (tet, _) = orient3d(o, patch.p);
                let there = flux - tet / 6.0 + cones;
                let allowed = 1e-9 * (size + cone_size);
                assert!((there - own).abs() <= allowed, "{t} {o}: {there} {own}");
            }
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
fn segment_shares_are_the_conics_areas() {
    // Parabola, circular arcs of half angle α (w = cos α), and every
    // weight against the area worked out by Simpson's rule on the
    // control triangle (−1, 0), (0, 1), (1, 0), whose area is 1.
    assert!((segment_share(1.0) - 2.0 / 3.0).abs() < 1e-15);
    for alpha in [0.05, 0.1, 0.5, PI / 4.0, 1.0, 1.4, 1.5] {
        let (s, c) = f64::sin_cos(alpha);
        let want = (2.0 * alpha - (2.0 * alpha).sin()) * c / (2.0 * s * s * s);
        let got = segment_share(c);
        assert!(
            (got - want).abs() < 1e-12 * want.max(1e-3),
            "{alpha}: {got} {want}"
        );
    }
    let simpson = |w: f64| {
        let f = |s: f64| {
            let d = (1.0 + w) + (1.0 - w) * s * s;
            2.0 * w * (1.0 - s * s) * ((1.0 + w) - (1.0 - w) * s * s) / (d * d * d)
        };
        let n = 200_000;
        let step = 2.0 / n as f64;
        (0..=n)
            .map(|k| {
                let weight = if k == 0 || k == n {
                    1.0
                } else {
                    [2.0, 4.0][k % 2]
                };
                weight * f(-1.0 + step * k as f64)
            })
            .sum::<f64>()
            * step
            / 3.0
    };
    for w in [1.0 / 64.0, 0.1, 0.5, 0.9, 1.0 + 1e-9, 1.1, 2.0, 10.0, 64.0] {
        let (got, want) = (segment_share(w), simpson(w));
        assert!((got - want).abs() < 1e-10, "{w}: {got} {want}");
    }
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
    try_ring(outer, inner, n).unwrap()
}

/// [`ring`], if the extrude makes it.
fn try_ring(outer: f64, inner: f64, n: usize) -> Result<Solid, KernelError> {
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
    extrude(&profile, &Frame::XY, 0.0, 1.0, 5, &TOL, &Budget::DEFAULT).stripped()
}

#[test]
fn thin_curved_shells_are_told_by_integrating() {
    // A ring a twentieth thick, four quarter arcs outside and four inside
    // (split finer by the extrude where their hulls would meet): the
    // corner triangles' volume is off by more than the ring's, and the
    // patches that could move it most are integrated until it is told.
    // (With many arcs inside, the caps refined for quality leave the
    // triangles' volume close enough to tell without.)
    let solid = ring(1.0, 0.95, 4);
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
    let ring = ring(1.0, 0.9, 4);
    let mushrooms = [false, true].map(|turned| mushroom(5.0, 0.5, 0.5, 10.0, turned));
    let counts = assert_deterministic(|| {
        (
            island.check_counted(&TOL),
            ring.mesh().check_counted(&TOL),
            mushrooms.each_ref().map(|m| m.check_counted(&TOL)),
        )
    });
    assert!(
        matches!(counts, (Ok(_), Ok(n), [Ok(m), Err(_)]) if n > 0 && m > 0),
        "{counts:?}"
    );
}

/// A mushroom: a square rod `a` across its diagonals from `z = −h` up to
/// a disc of radius `r` and thickness `t` whose top, at `z = 0`, is
/// bounded by four quarter circles and whose underside by the square on
/// their ends. Its first corner is at the rod's foot, far below the top.
fn mushroom(r: f64, t: f64, a: f64, h: f64, turned: bool) -> Mesh {
    let mut builder = MeshBuilder::new();
    let f = crate::mesh::tests::free(&mut builder);
    let dirs = [DVec3::X, DVec3::Y, DVec3::NEG_X, DVec3::NEG_Y];
    let foot = dirs.map(|d| builder.vert(d * a - DVec3::Z * h));
    let neck = dirs.map(|d| builder.vert(d * a - DVec3::Z * t));
    let under = dirs.map(|d| builder.vert(d * r - DVec3::Z * t));
    let top = dirs.map(|d| builder.vert(d * r));
    let mut tri = |[a, b, c]: [u32; 3]| builder.tri(if turned { [a, c, b] } else { [a, b, c] }, f);
    tri([foot[0], foot[3], foot[2]]);
    tri([foot[0], foot[2], foot[1]]);
    for i in 0..4 {
        let j = (i + 1) % 4;
        for (low, high) in [(foot, neck), (under, top)] {
            tri([low[i], low[j], high[j]]);
            tri([low[i], high[j], high[i]]);
        }
        tri([neck[i], neck[j], under[j]]);
        tri([neck[i], under[j], under[i]]);
    }
    let centre = builder.vert(DVec3::ZERO);
    for i in 0..4 {
        let j = (i + 1) % 4;
        builder.tri(
            if turned {
                [centre, top[j], top[i]]
            } else {
                [centre, top[i], top[j]]
            },
            f,
        );
        builder.edge(top[i], top[j], (dirs[i] + dirs[j]) * r, FRAC_1_SQRT_2);
    }
    builder.build().unwrap()
}

#[test]
fn far_flat_faces_bounded_by_curves_count() {
    // The disc's top is flat, so its patches are never integrated, but
    // measured from the rod's foot each adds to its corner triangle's
    // volume the cones of the lunes along its arcs, h·(π − 2)r²/3 in all,
    // more than the whole volume. Only the cones the rim's patches add
    // the other way cancel them, so what an integrated patch adds is
    // taken without its lunes' cones. Before, this mushroom was refused
    // and turned inside out it passed.
    let (r, t, a, h) = (5.0, 0.5, 0.5, 10.0);
    let upright = mushroom(r, t, a, h, false);
    let volume = Solid::new(upright, &TOL).map(|s| s.volume());
    // Between the rod and the prism on the underside, and that plus the
    // lunes' prism.
    let (low, lunes) = (2.0 * r * r * t + 2.0 * a * a * (h - t), (PI - 2.0) * r * r);
    assert!(
        matches!(volume, Ok(v) if low < v && v < low + lunes * t && 3.0 * v < lunes * h),
        "{volume:?}"
    );
    let turned = mushroom(r, t, a, h, true);
    assert_eq!(turned.check(&TOL), Err(CheckError::InsideOut(0)));
}

/// One shell of [`nested`]'s: a box, or a cylinder standing in the box's
/// footprint, turned inside out or not, and the shell it lies in.
struct Nested {
    mesh: Mesh,
    volume: f64,
    turned: bool,
    parent: Option<usize>,
}

/// Random boxes and cylinders nested up to four deep, some side by
/// side, each facing the right way for where it lies. Corners are on a grid of `(0.5, 0.75, 8)`, so rays along
/// `(2, 3, 32)` from one shell's corner often run through the others'
/// edges and corners.
fn nested(rng: &mut crate::test_rng::Rng) -> Vec<Nested> {
    const STEP: DVec3 = DVec3::new(0.5, 0.75, 8.0);
    fn place(
        rng: &mut crate::test_rng::Rng,
        (lo, hi): (DVec3, DVec3),
        depth: usize,
        parent: Option<usize>,
        winding: i32,
        out: &mut Vec<Nested>,
    ) {
        let n = if depth == 0 {
            1 + rng.next_u64() % 3
        } else if depth < 4 {
            rng.next_u64() % 3
        } else {
            0
        } as usize;
        let axis = depth % 2;
        fn steps(rng: &mut crate::test_rng::Rng, k: u64) -> f64 {
            (rng.next_u64() % k) as f64
        }
        for i in 0..n {
            let (mut slab_lo, mut slab_hi) = (lo, hi);
            let width = hi[axis] - lo[axis];
            slab_lo[axis] = lo[axis] + width * i as f64 / n as f64;
            slab_hi[axis] = lo[axis] + width * (i + 1) as f64 / n as f64;
            // At least a step in from the slab on every side, on the grid.
            let min = ((slab_lo / STEP).floor()
                + 1.0
                + DVec3::new(steps(rng, 3), steps(rng, 3), steps(rng, 2)))
                * STEP;
            let max = ((slab_hi / STEP).ceil()
                - 1.0
                - DVec3::new(steps(rng, 3), steps(rng, 3), steps(rng, 2)))
                * STEP;
            if (max - min).cmplt(DVec3::new(1.0, 1.5, 8.0)).any() {
                continue;
            }
            let cylinder = rng.next_u64().is_multiple_of(3);
            let turned = winding == 1;
            let (mesh, volume, inside) = if cylinder {
                let r = ((max - min).truncate().min_element() / 2.0 / 0.25).floor() * 0.25;
                let centre = (min + max) / 2.0;
                let base = DVec3::new(centre.x, centre.y, min.z);
                let mesh = Mesh::cylinder(base, r, max.z - min.z, 2, &TOL).unwrap();
                // Clear of the walls' chords, which run between the
                // quarter points: a square of half side r/2 fits inside.
                let half = DVec3::new(0.5 * r - 0.3, 0.5 * r - 0.3, 0.0);
                (
                    mesh,
                    PI * r * r * (max.z - min.z),
                    (
                        DVec3::new(centre.x, centre.y, min.z) - half,
                        DVec3::new(centre.x, centre.y, max.z) + half,
                    ),
                )
            } else {
                let mesh = Mesh::cuboid(min, max - min, 1, &TOL).unwrap();
                (mesh, (max - min).element_product(), (min, max))
            };
            let me = out.len();
            out.push(Nested {
                mesh,
                volume,
                turned,
                parent,
            });
            let inner = winding + if turned { -1 } else { 1 };
            place(rng, inside, depth + 1, Some(me), inner, out);
        }
    }
    let mut out = Vec::new();
    let span = DVec3::new(24.0, 24.0, 64.0);
    place(rng, (DVec3::ZERO, span), 0, None, 0, &mut out);
    out
}

#[test]
fn random_nested_shells_are_told_right() {
    // Against the nesting they were built with: a shell is right when the
    // shells round it wind 0 about it and it faces out, or 1 and it faces
    // in. The parts go into the mesh in a random order; the check names
    // the first wrong shell in that order, and a right mesh's volume is
    // the shells' own, signed.
    let mut rng = crate::test_rng::Rng::new(56);
    let (mut right, mut wrong) = (0, 0);
    // Quick mode runs the first third.
    let cases = varde_testing::pick(100, 300);
    for case in 0..cases {
        let mut shells = nested(&mut rng);
        // Half the cases with one or two shells turned the wrong way.
        if case % 2 == 1 {
            for _ in 0..1 + rng.next_u64() % 2 {
                let i = (rng.next_u64() % shells.len() as u64) as usize;
                shells[i].turned = !shells[i].turned;
            }
        }
        let sign = |s: &Nested| if s.turned { -1 } else { 1 };
        let mut order: Vec<usize> = (0..shells.len()).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
        }
        let mut first = 0;
        let mut want = Ok(());
        for &i in &order {
            let mut winding = 0;
            let mut up = shells[i].parent;
            while let Some(p) = up {
                winding += sign(&shells[p]);
                up = shells[p].parent;
            }
            let fine = if shells[i].turned {
                winding == 1
            } else {
                winding == 0
            };
            if !fine && want.is_ok() {
                want = Err(CheckError::InsideOut(first));
            }
            first += shells[i].mesh.tris().len() as u32;
        }
        let parts: Vec<(&Mesh, bool)> = order
            .iter()
            .map(|&i| (&shells[i].mesh, shells[i].turned))
            .collect();
        let mesh = joined(&parts);
        assert_eq!(mesh.check(&TOL), want, "case {case}");
        if want.is_ok() {
            right += 1;
            let volume: f64 = shells.iter().map(|s| f64::from(sign(s)) * s.volume).sum();
            let got = solid(mesh).volume();
            assert!(
                (got - volume).abs() < 1e-9 * volume.abs().max(1.0),
                "case {case}: {got} {volume}"
            );
        } else {
            wrong += 1;
        }
    }
    assert!(right * 6 > cases && wrong * 6 > cases, "{right} {wrong}");
}

/// `mesh` with every vertex and control point moved by `f`, on one free
/// face, turned inside out if `turned`.
fn moved(mesh: &Mesh, f: impl Fn(DVec3) -> DVec3, turned: bool) -> Mesh {
    let mut builder = MeshBuilder::new();
    let face = crate::mesh::tests::free(&mut builder);
    for &p in mesh.verts() {
        builder.vert(f(p));
    }
    for (t, tri) in mesh.tris().iter().enumerate() {
        let [a, b, c] = tri.halfedges.map(|h| h.start);
        builder.tri(if turned { [a, c, b] } else { [a, b, c] }, face);
        let patch = mesh.patch(t);
        for (i, (u, v)) in [(a, b), (b, c), (c, a)].into_iter().enumerate() {
            builder.edge(u, v, f(patch.c[i]), patch.w[i]);
        }
    }
    builder.build().unwrap()
}

#[test]
fn thin_tilted_slabs_far_out_are_told() {
    // Slabs as thin for their size as the fold rule lets a box be (about
    // 1e7 times as wide as thick), tilted off every axis, near the origin
    // and far out, at the finest tolerance. Upright they pass with their
    // volume; turned over they are refused. (Floating point tells them:
    // a corner triangle's rounding is about ε·size³, the volume size²
    // times the thickness.)
    let tol = Tolerance::new(Tolerance::MIN_FIT).unwrap();
    let x = DVec3::new(2.0, 1.0, 2.0) / 3.0;
    let y = DVec3::new(-1.0, 2.0, 0.0).normalize();
    let z = x.cross(y);
    for (size, thickness, offset) in [
        (1.2e6, 0.1, DVec3::ZERO),
        (1e4, 1e-3, DVec3::new(-2e5, 1e5, 3e5)),
        (1e3, 1e-4, DVec3::new(6e5, 5e5, -6e5)),
    ] {
        let corner = DVec3::new(-size / 2.0, -size / 2.0, 0.0);
        let slab = Mesh::cuboid(corner, DVec3::new(size, size, thickness), 1, &tol).unwrap();
        let tilt = |p: DVec3| offset + x * p.x + y * p.y + z * p.z;
        let upright = moved(&slab, tilt, false);
        let want = size * size * thickness;
        let volume = Solid::new(upright, &tol).map(|s| s.volume());
        assert!(
            matches!(volume, Ok(v) if (v - want).abs() < 0.01 * want),
            "{size} {thickness}: {volume:?} {want}"
        );
        let turned = moved(&slab, tilt, true);
        assert_eq!(
            turned.check(&tol),
            Err(CheckError::InsideOut(0)),
            "{thickness}"
        );
        // A void a tenth as wide and half as thick in the middle of it,
        // and the same facing out.
        let inner = DVec3::new(size / 20.0, size / 20.0, thickness / 2.0);
        let void = Mesh::cuboid(-inner / 2.0 + DVec3::Z * thickness / 2.0, inner, 1, &tol).unwrap();
        for (turned, want) in [(true, Ok(())), (false, Err(CheckError::InsideOut(12)))] {
            let mesh = joined(&[(&slab, false), (&void, turned)]);
            let mesh = moved(&mesh, tilt, false);
            assert_eq!(mesh.check(&tol), want, "{size} {thickness} {turned}");
        }
    }
}

#[test]
fn random_thin_curved_shells_are_told() {
    // Rings of random size and thickness with holes in random numbers of
    // arcs, and mushrooms of random proportions: upright they pass (the
    // rings with their volume), turned over they are refused.
    let mut rng = crate::test_rng::Rng::new(6);
    let mut rings = 0;
    // Quick mode runs the first third of each kind (integrating the rings'
    // volumes is most of the time).
    let ring_cases = varde_testing::pick(14, 40);
    for case in 0..ring_cases {
        let outer = rng.range(1.0, 5.0);
        let inner = outer * (1.0 - rng.log_range(0.005, 0.2));
        let n = 4 + (rng.next_u64() % 61) as usize;
        let Ok(solid) = try_ring(outer, inner, n) else {
            continue;
        };
        rings += 1;
        let want = PI * (outer * outer - inner * inner);
        assert!(
            (solid.volume() - want).abs() < 1e-9 * want.max(1.0),
            "ring {case}: {} {want}",
            solid.volume()
        );
        let turned = joined(&[(solid.mesh(), true)]);
        assert_eq!(
            turned.check(&TOL),
            Err(CheckError::InsideOut(0)),
            "ring {case}"
        );
    }
    assert!(rings * 2 > ring_cases, "{rings}");
    for case in 0..varde_testing::pick(34, 100) {
        let r = rng.range(1.0, 10.0);
        let t = rng.range(0.05, 1.0);
        let a = rng.range(0.1, r / 4.0);
        let h = t + rng.range(0.5, 40.0);
        let upright = mushroom(r, t, a, h, false);
        assert_eq!(
            upright.check(&TOL),
            Ok(()),
            "mushroom {case}: {r} {t} {a} {h}"
        );
        let turned = mushroom(r, t, a, h, true);
        assert_eq!(
            turned.check(&TOL),
            Err(CheckError::InsideOut(0)),
            "mushroom {case}: {r} {t} {a} {h}"
        );
    }
}

#[test]
fn lune_bounds_hold_on_random_patches() {
    // Random corners, control points off the edges' middles by up to three
    // times their size, weights over their whole range: whatever passes
    // the fold rule adds no more than its bound (185 000 of them came to
    // at most a third of it).
    let mut rng = crate::test_rng::Rng::new(9);
    let mut tried = 0;
    // Quick mode runs the first fifth: integrating a patch with weights
    // this far from 1 takes its 1024 pieces, most of the time here.
    let cases = varde_testing::pick(600, 3000);
    for _ in 0..cases {
        let p = [rng.point(1.0), rng.point(1.0), rng.point(1.0)];
        let c = [0, 1, 2].map(|i| {
            let e = rng.log_range(1e-3, 3.0);
            (p[i] + p[(i + 1) % 3]) / 2.0 + rng.point(e)
        });
        let w = [0, 1, 2].map(|_| rng.log_range(1.0 / 64.0, 64.0));
        let Ok(patch) = Patch::new(p, c, w) else {
            continue;
        };
        if patch.fold_direction().is_none() {
            continue;
        }
        tried += 1;
        let own = patch_volume(&patch, p[0]).0 + lune_cones(&patch, p[0]).0;
        let bound = lune_bound(&patch);
        assert!(own.abs() <= 0.5 * bound, "{patch:?}: {own} {bound}");
    }
    assert!(tried * 3 > cases, "{tried}");
}
