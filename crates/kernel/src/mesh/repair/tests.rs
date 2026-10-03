#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use glam::DVec3;

use super::super::tests::{
    OCTAHEDRON, TOL, UNIT, add_round_octahedron, face, octahedron, round_octahedron, tetrahedron,
};
use super::*;
use crate::Tolerance;
use crate::mesh::hull;
use crate::mesh::{MeshBuilder, Surface};
use crate::par::assert_deterministic;
use crate::patch::{Patch, PatchError};
use crate::test_rng::Rng;

/// A thin curved plate closed on itself: the shell between two round
/// octahedra, of radius `radius` and `radius - thickness`, the inner one
/// facing in. Their hulls overlap until each side is split finely enough.
fn shell(radius: f64, thickness: f64) -> Mesh {
    shell_at(DVec3::ZERO, radius, thickness)
}

/// [`shell`] centred on `center`.
fn shell_at(center: DVec3, radius: f64, thickness: f64) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, center, radius, false);
    add_round_octahedron(&mut builder, center, radius - thickness, true);
    builder.build().unwrap()
}

/// Adds `mesh`'s vertices, faces and triangles to `builder`, which has
/// `base` vertices already.
fn add_mesh(builder: &mut MeshBuilder, mesh: &Mesh, base: u32) {
    for &p in mesh.verts() {
        builder.vert(p);
    }
    let faces: Vec<u32> = mesh.faces().iter().map(|&f| builder.face(f)).collect();
    for t in 0..mesh.tris().len() {
        let c = mesh.corners(t as u32).map(|v| v + base);
        let patch = mesh.patch(t);
        for i in 0..3 {
            builder.edge(c[i], c[(i + 1) % 3], patch.c[i], patch.w[i]);
        }
        builder.tri(c, faces[mesh.tris()[t].face as usize]);
    }
}

/// The unit cylinder with a half-unit box beside it, its nearest corner
/// `gap` off the wall at 30° round from `+x`.
fn cylinder_and_box(gap: f64) -> Mesh {
    let cylinder = Mesh::cylinder(DVec3::ZERO, 1.0, 2.0, 1, &TOL).unwrap();
    let (s, c) = 30f64.to_radians().sin_cos();
    let corner = DVec3::new(c, s, 0.0) * (1.0 + gap) + DVec3::Z * 0.5;
    let cuboid = Mesh::cuboid(corner, DVec3::splat(0.5), 2, &TOL).unwrap();
    both(&cylinder, &cuboid)
}

/// [`cylinder_and_box`] at `tol`, the cylinder of `radius` standing on
/// `base` and the box `radius / 2` across.
fn cylinder_and_box_at(base: DVec3, radius: f64, gap: f64, tol: &Tolerance) -> Mesh {
    let cylinder = Mesh::cylinder(base, radius, 2.0 * radius, 1, tol).unwrap();
    let (s, c) = 30f64.to_radians().sin_cos();
    let corner = base + DVec3::new(c, s, 0.0) * (radius + gap) + DVec3::Z * 0.5 * radius;
    let cuboid = Mesh::cuboid(corner, DVec3::splat(0.5 * radius), 2, tol).unwrap();
    both(&cylinder, &cuboid)
}

/// Two cylinders of `radius`, as tall as they are wide, side by side with
/// `gap` between their walls, the second at `angle` degrees round from
/// `+x`.
fn cylinders(radius: f64, angle: f64, gap: f64, tol: &Tolerance) -> Mesh {
    let a = Mesh::cylinder(DVec3::ZERO, radius, 2.0 * radius, 1, tol).unwrap();
    let (s, c) = angle.to_radians().sin_cos();
    let base = DVec3::new(c, s, 0.0) * (2.0 * radius + gap);
    let b = Mesh::cylinder(base, radius, 2.0 * radius, 2, tol).unwrap();
    both(&a, &b)
}

/// `a` and `b` as one mesh, `b`'s vertices and triangles after `a`'s.
fn both(a: &Mesh, b: &Mesh) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_mesh(&mut builder, a, 0);
    add_mesh(&mut builder, b, a.verts().len() as u32);
    builder.build().unwrap()
}

/// Two round octahedra of `radius`, side by side along `x` with
/// `gap` between their nearest corners.
fn two_spheres(radius: f64, gap: f64) -> Mesh {
    let mut builder = MeshBuilder::new();
    add_round_octahedron(&mut builder, DVec3::ZERO, radius, false);
    add_round_octahedron(&mut builder, DVec3::X * (2.0 * radius + gap), radius, false);
    builder.build().unwrap()
}

/// `mesh` scaled by `s` about the origin.
fn scaled(mut mesh: Mesh, s: f64) -> Mesh {
    for v in &mut mesh.verts {
        *v *= s;
    }
    for e in &mut mesh.edges {
        e.ctrl *= s;
    }
    mesh
}

/// `mesh` repaired with `tol`, and the work that took.
fn repair_counting(mesh: Mesh, tol: &Tolerance) -> (Result<Mesh, KernelError>, u64) {
    let mut work = Work::new(&Budget::DEFAULT);
    let result = mesh.repair_within(tol, &mut work);
    (result, Budget::DEFAULT.work() - work.left())
}

/// A tetrahedron of four patches bulging out like a sphere, with each
/// edge's control point `k` times as far out as the arc's: at `k = 1.5`
/// the patches fail the fold check, though their pieces pass.
fn bulging_tetrahedron(k: f64) -> Mesh {
    let v = [
        DVec3::new(1.0, 1.0, 1.0),
        DVec3::new(1.0, -1.0, -1.0),
        DVec3::new(-1.0, 1.0, -1.0),
        DVec3::new(-1.0, -1.0, 1.0),
    ]
    .map(|p| p / 3f64.sqrt());
    let mut builder = MeshBuilder::new();
    let f = builder.face(face(0, Surface::Free));
    let ids = v.map(|p| builder.vert(p));
    for [a, b, c] in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
        builder.tri([ids[a], ids[b], ids[c]], f);
    }
    for a in 0..4 {
        for b in a + 1..4 {
            // The arc of the unit sphere between them.
            let cos = v[a].dot(v[b]);
            let ctrl = (v[a] + v[b]) / (1.0 + cos);
            builder.edge(ids[a], ids[b], ctrl * k, ((1.0 + cos) / 2.0).sqrt());
        }
    }
    builder.build().unwrap()
}

#[test]
fn a_thin_curved_plate_is_repaired() {
    for (thickness, patches) in [(0.2, 1024), (0.05, 4096)] {
        let mesh = shell(10.0, thickness);
        assert!(matches!(mesh.check(&TOL), Err(CheckError::Hull(..))));
        let repaired = mesh.repair(&TOL, &Budget::DEFAULT).unwrap();
        assert_eq!(repaired.check(&TOL), Ok(()));
        // Both sides split evenly, into pieces of about 22.5° or 11.25°.
        assert_eq!(repaired.tris().len(), patches, "{thickness}");
    }
}

#[test]
fn repair_is_the_same_on_any_thread_count() {
    let repaired = assert_deterministic(|| shell(10.0, 0.2).repair(&TOL, &Budget::DEFAULT));
    assert!(repaired.is_ok());
    let repaired = assert_deterministic(|| cylinder_and_box(1e-3).repair(&TOL, &Budget::DEFAULT));
    assert!(repaired.is_ok());
    // Curved pieces split below `MIN_SPLIT`.
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let res = coarse.resolution();
    let small = cylinders(30.0 * res, 45.0, 3.0 * res, &coarse);
    let repaired = assert_deterministic(|| small.clone().repair(&coarse, &Budget::DEFAULT));
    assert!(repaired.is_ok());
    // Failing on a witness, and the work it took.
    let res = TOL.resolution();
    for mesh in [
        shell(10.0, 0.5 * res),
        cylinders(1.0, 30.0, 0.5 * res, &TOL),
    ] {
        let (result, _) = assert_deterministic(|| repair_counting(mesh.clone(), &TOL));
        assert!(matches!(
            result,
            Err(KernelError::Invalid(CheckError::Hull(..)))
        ));
    }
}

#[test]
fn surfaces_just_over_the_resolution_apart_pass() {
    // Pieces flat within the resolution of a curved surface still have
    // hulls up to about half a resolution off it. Repair used to stop
    // splitting such pairs and refuse surfaces up to about 1.3
    // resolutions apart; now only those within about 1.03.
    let res = TOL.resolution();
    for (mesh, most) in [
        (cylinder_and_box(1.05 * res), 300_000),
        (cylinders(1.0, 30.0, 1.1 * res, &TOL), 2_000_000),
    ] {
        let (result, work) = repair_counting(mesh, &TOL);
        let repaired = result.unwrap();
        assert_eq!(repaired.check(&TOL), Ok(()));
        assert!(work < most, "{work}");
    }
}

#[test]
fn far_out_at_the_finest_tolerance_surfaces_just_apart_pass() {
    // At the finest resolution far from the origin the witness can't
    // claim anything (its rounding allowance is past the resolution),
    // so only flatness stops touching pairs, and rounding of a ulp there,
    // about a hundredth of a resolution, widens the window the flat stop
    // refuses from about 1.02 resolutions to about 1.05.
    let fine = Tolerance::new(Tolerance::MIN_FIT).unwrap();
    let res = fine.resolution();
    let radius = 1e5 * res;
    for (base, apart, touching) in [
        (DVec3::ZERO, 1.03, 0.98),
        (DVec3::new(9e5, -6e5, 3e5), 1.06, 0.98),
    ] {
        let (result, work) =
            repair_counting(cylinder_and_box_at(base, radius, apart * res, &fine), &fine);
        let repaired = result.unwrap();
        assert_eq!(repaired.check(&fine), Ok(()));
        assert!(work < 300_000, "{work}");
        let (result, work) = repair_counting(
            cylinder_and_box_at(base, radius, touching * res, &fine),
            &fine,
        );
        assert!(
            matches!(result, Err(KernelError::Invalid(CheckError::Hull(..)))),
            "{result:?}"
        );
        assert!(work < 300_000, "{work}");
    }
}

#[test]
fn touching_round_surfaces_fail_at_once() {
    // Points of the two surfaces found within the resolution show that
    // no split can mend the pair, so repair fails in the first round. It
    // used to split until the pieces at the touch were flat (67k to 670k
    // units of work here).
    let res = TOL.resolution();
    for mesh in [
        cylinders(1.0, 30.0, 0.5 * res, &TOL),
        cylinders(1.0, 17.0, 0.9 * res, &TOL),
        cylinder_and_box(0.9 * res),
    ] {
        let (result, work) = repair_counting(mesh, &TOL);
        assert!(
            matches!(result, Err(KernelError::Invalid(CheckError::Hull(..)))),
            "{result:?}"
        );
        assert!(work < 1000, "{work}");
    }
}

#[test]
fn a_shell_thinner_than_the_resolution_fails_at_once() {
    // Its sides are within the resolution over their whole area, and used
    // to be split until the budget ran out (`TooComplex`, seconds). At
    // the origin and far from it.
    let res = TOL.resolution();
    for (offset, thickness) in [(0.0, 0.5), (0.0, 0.99), (1e5, 0.5)] {
        let mesh = shell_at(DVec3::splat(offset), 10.0, thickness * res);
        let (result, work) = repair_counting(mesh, &TOL);
        assert!(
            matches!(result, Err(KernelError::Invalid(CheckError::Hull(..)))),
            "{offset} {thickness}: {result:?}"
        );
        assert!(work < 1000, "{work}");
    }
    // A valid shell a few resolutions thick is still split until it fits
    // the budget or runs out of it.
    assert_eq!(
        shell(10.0, 3.0 * res).repair(&TOL, &Budget::new(100_000)),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn repair_splits_only_near_the_trouble_and_keeps_surfaces() {
    let mut previous = 0;
    for gap in [0.1, 1e-2, 1e-3] {
        let mesh = cylinder_and_box(gap);
        assert_eq!(mesh.check(&TOL), Err(CheckError::Hull(0, 16)));
        let repaired = mesh.clone().repair(&TOL, &Budget::DEFAULT).unwrap();
        assert_eq!(repaired.check(&TOL), Ok(()));
        // Graded: a closer box takes a few more pieces near it.
        let n = repaired.tris().len();
        assert!(n > previous && n < 200, "{gap}: {n}");
        previous = n;
        // The flat box is never split.
        for t in 16..28 {
            let found = (0..n).any(|u| repaired.patch(u) == mesh.patch(t));
            assert!(found, "box patch {t} is gone");
        }
        // Pieces of the wall are on the cylinder, and pieces of the caps
        // on their planes, to rounding.
        let mut rng = Rng::new(7);
        for t in 0..n {
            let patch = repaired.patch(t);
            let surface = repaired.faces()[repaired.tris()[t].face as usize].surface;
            let off = match surface {
                Surface::Quadric(_) => (0..10)
                    .map(|_| surface.distance(patch.eval(rng.bary())))
                    .fold(0.0, f64::max),
                _ => patch
                    .hull()
                    .map(|x| surface.distance(x))
                    .into_iter()
                    .fold(0.0, f64::max),
            };
            assert!(off < 1e-12, "patch {t} is {off} off its face");
        }
    }
}

#[test]
fn a_wrong_plane_tag_fails() {
    // Repair splits a patch on a `Plane` face with straight inner edges,
    // which is only right if it is on the plane. One that isn't fails,
    // naming its triangle, rather than be reshaped. This used to pass,
    // with the wall's pieces moved 7.6e-2 off the cylinder.
    let mut mesh = cylinder_and_box(1e-2);
    // Face 2 is the cylinder's first wall.
    mesh.faces[2].surface = Surface::Plane {
        n: DVec3::Z,
        d: 100.0,
    };
    let result = mesh.clone().repair(&TOL, &Budget::DEFAULT);
    let Err(KernelError::Invalid(CheckError::Face(t))) = result else {
        panic!("{result:?}");
    };
    assert_eq!(mesh.tris()[t as usize].face, 2);
}

#[test]
fn plane_tags_within_the_resolution_are_trusted() {
    // The cylinder's caps tagged half a resolution off their planes:
    // within the tolerance `check` allows, so the caps are still split as
    // planar, with straight inner edges, and the result passes `check`.
    let mut mesh = cylinder_and_box(1e-3);
    let half = 0.5 * TOL.resolution();
    let mut caps = Vec::new();
    for (f, face) in mesh.faces.iter_mut().enumerate() {
        if let Surface::Plane { n, d } = face.surface {
            face.surface = Surface::Plane {
                n,
                d: d + half * n.length(),
            };
            caps.push(f as u32);
        }
        if caps.len() == 2 {
            break;
        }
    }
    assert_eq!(caps, [0, 1]);
    let on_caps = |mesh: &Mesh| {
        (0..mesh.tris().len())
            .filter(|&t| caps.contains(&mesh.tris()[t].face))
            .collect::<Vec<_>>()
    };
    let before = on_caps(&mesh).len();
    let repaired = mesh.repair(&TOL, &Budget::DEFAULT).unwrap();
    assert_eq!(repaired.check(&TOL), Ok(()));
    let after = on_caps(&repaired);
    assert!(after.len() > before, "{before} -> {}", after.len());
    // The pieces are on the caps' true planes, z = 0 and z = 2, and the
    // middle pieces of red splits have straight edges all round, where an
    // exact split's would be arcs.
    let straight = |patch: &crate::patch::Patch, i: usize| {
        let (a, b) = (patch.p[i], patch.p[(i + 1) % 3]);
        (patch.c[i] - (a + b) * 0.5).length() < 1e-12 && patch.w[i] == 1.0
    };
    let mut middles = 0;
    for &t in &after {
        let patch = repaired.patch(t);
        let z = patch.p[0].z;
        assert!(z == 0.0 || z == 2.0, "patch {t}");
        assert!(patch.hull().iter().all(|x| x.z == z), "patch {t}");
        middles += usize::from((0..3).all(|i| straight(&patch, i)));
    }
    assert!(middles > 0);
}

#[test]
fn plane_tags_past_the_resolution_fail() {
    // The other side of the guard: the caps tagged one and a half
    // resolutions off their planes. Their patches are split, so repair
    // fails naming a cap triangle rather than trust the tag.
    let mut mesh = cylinder_and_box(1e-3);
    let off = 1.5 * TOL.resolution();
    for face in &mut mesh.faces[..2] {
        let Surface::Plane { n, d } = face.surface else {
            panic!("the caps are faces 0 and 1");
        };
        face.surface = Surface::Plane {
            n,
            d: d + off * n.length(),
        };
    }
    let result = mesh.clone().repair(&TOL, &Budget::DEFAULT);
    let Err(KernelError::Invalid(CheckError::Face(t))) = result else {
        panic!("{result:?}");
    };
    assert!(mesh.tris()[t as usize].face < 2, "{t}");
}

#[test]
fn a_wrong_quadric_tag_passes_repair_but_not_solid_new() {
    // Repair splits quadric patches exactly, trusting nothing, so it
    // carries a wrong `Quadric` tag through; `Solid::new` refuses the
    // result, in every build, naming the same triangle at 1 and 8
    // threads. Face 2 is the cylinder's first wall.
    let mut mesh = cylinder_and_box(1e-2);
    let Surface::Quadric(_) = mesh.faces[2].surface else {
        panic!("face 2 is a wall");
    };
    mesh.faces[2].surface =
        Surface::Quadric(crate::mesh::Quadric::cylinder(DVec3::ZERO, DVec3::Z, 1.01).unwrap());
    let result = assert_deterministic(|| {
        let repaired = mesh.clone().repair(&TOL, &Budget::DEFAULT).unwrap();
        assert_eq!(repaired.check_embedding(&TOL).err(), None);
        let face = |t: u32| repaired.tris()[t as usize].face;
        crate::Solid::new(repaired.clone(), &TOL).map_err(|e| match e {
            KernelError::Invalid(CheckError::Face(t)) => face(t),
            e => panic!("{e:?}"),
        })
    });
    assert_eq!(result.err(), Some(2));
}

#[test]
fn a_fold_is_repaired() {
    let mesh = bulging_tetrahedron(1.5);
    assert_eq!(mesh.check(&TOL), Err(CheckError::Fold(0)));
    let repaired = mesh.repair(&TOL, &Budget::DEFAULT).unwrap();
    assert_eq!(repaired.check(&TOL), Ok(()));
    assert_eq!(repaired.tris().len(), 16);
}

/// The solid `mesh` gives repaired, merged and then checked, and
/// checked first and repaired only if that fails, each within `units` of
/// work, with the work left.
fn both_ways(mesh: &Mesh, units: u64) -> [(Result<crate::Solid, KernelError>, u64); 2] {
    let budget = Budget::new(units);
    let mut work = Work::new(&budget);
    let repaired = mesh
        .clone()
        .repair_within(&TOL, &mut work)
        .and_then(|mesh| mesh.merge_faces(TOL.resolution(), &mut work))
        .and_then(|mesh| crate::Solid::new_within(mesh, &TOL, &mut work));
    let left = work.left();
    let mut work = Work::new(&budget);
    let checked =
        crate::Solid::new_repaired_within(mesh.clone(), &TOL, &mut work).map_err(|f| f.error);
    [(repaired, left), (checked, work.left())]
}

#[test]
fn checking_first_gives_what_repairing_first_does() {
    // A mesh that passes is kept, charged what repair's pass over it
    // would be, so it runs out of work just where that did; one that
    // fails, at a hull (a thin plate) or a fold, is repaired.
    let passes = round_octahedron(DVec3::ZERO);
    let [(repaired, left), (checked, same)] = both_ways(&passes, Budget::DEFAULT.work());
    assert_eq!(checked, repaired);
    assert!(checked.is_ok());
    assert_eq!(left, same);
    let spent = Budget::DEFAULT.work() - left;
    for units in [spent - 1, spent] {
        let [(repaired, _), (checked, _)] = both_ways(&passes, units);
        assert_eq!(checked, repaired, "{units}");
    }
    assert!(both_ways(&passes, spent - 1)[1].0.is_err());
    for fails in [shell(10.0, 0.2), bulging_tetrahedron(1.5)] {
        assert!(fails.check(&TOL).is_err());
        let [(repaired, _), (checked, _)] = both_ways(&fails, Budget::DEFAULT.work());
        assert!(checked.is_ok());
        assert_eq!(checked, repaired);
    }
}

#[test]
fn a_mesh_that_passes_is_kept() {
    let mesh = round_octahedron(DVec3::ZERO);
    assert_eq!(mesh.clone().repair(&TOL, &Budget::DEFAULT), Ok(mesh));
}

#[test]
fn what_splitting_cant_mend_fails() {
    // A corner whose two edges leave it the same way fails at once.
    let ctrl = DVec3::X + (DVec3::new(0.5, 0.0, 0.5) - DVec3::X) * 0.5;
    let cusp = octahedron(UNIT, &OCTAHEDRON, &[(0, 1, ctrl, 1.0)]);
    assert_eq!(
        cusp.repair(&TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(CheckError::Fold(0)))
    );

    // Flat triangles closer than the resolution fail at once: their
    // pieces keep the gap, and nothing splitting does brings them apart.
    // Two boxes face to face used to be split to the end of the budget.
    let gap = 0.5 * TOL.resolution();
    let tetrahedra = both(
        &tetrahedron(DVec3::ZERO),
        &tetrahedron(DVec3::X * (1.0 + gap)),
    );
    let (result, work) = repair_counting(tetrahedra, &TOL);
    assert_eq!(result, Err(KernelError::Invalid(CheckError::Hull(0, 4))));
    assert!(work < 1000, "{work}");
    let boxes = both(
        &Mesh::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap(),
        &Mesh::cuboid(DVec3::Z * (1.0 + gap), DVec3::ONE, 2, &TOL).unwrap(),
    );
    // The lower box's top against the upper one's bottom.
    let (result, work) = repair_counting(boxes, &TOL);
    assert_eq!(result, Err(KernelError::Invalid(CheckError::Hull(2, 12))));
    assert!(work < 1000, "{work}");

    // Round surfaces that touch fail as soon as points of them are found
    // within the resolution, small ones too: they used to be split until
    // the pieces at the touch were flat, and small ones then reached the
    // smallest piece repair splits (`TooComplex`).
    let (result, work) = repair_counting(two_spheres(1.0, gap), &TOL);
    assert_eq!(result, Err(KernelError::Invalid(CheckError::Hull(0, 9))));
    assert!(work < 1000, "{work}");
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let spheres = two_spheres(200.0 * coarse.resolution(), 0.5 * coarse.resolution());
    let (result, work) = repair_counting(spheres, &coarse);
    assert_eq!(result, Err(KernelError::Invalid(CheckError::Hull(0, 9))));
    assert!(work < 1000, "{work}");
    // Small surfaces apart, but by too little for pieces as large as the
    // smallest repair splits even curved, still reach it, and fail with
    // what asked for the split (they used to give `TooComplex`): at this
    // tolerance splitting can't mend them. A finer one does.
    let small = 10.0 * coarse.resolution();
    let gap = 1.5 * coarse.resolution();
    let (result, work) = repair_counting(cylinders(small, 30.0, gap, &coarse), &coarse);
    assert!(
        matches!(result, Err(KernelError::Invalid(CheckError::Hull(..)))),
        "{result:?}"
    );
    assert!(work < 100_000, "{work}");
    let fine = Tolerance::new(Tolerance::MAX_FIT / 10.0).unwrap();
    let repaired = cylinders(small, 30.0, gap, &fine).repair(&fine, &Budget::DEFAULT);
    assert_eq!(repaired.unwrap().check(&fine), Ok(()));
    // So do curved pieces failing the fold check: a bulging tetrahedron a
    // resolution across. At 32 resolutions across they are split
    // and pass (`TooComplex` before, under `MIN_SPLIT`).
    let res = TOL.resolution();
    let (result, _) = repair_counting(scaled(bulging_tetrahedron(1.5), res), &TOL);
    assert!(
        matches!(result, Err(KernelError::Invalid(CheckError::Fold(_)))),
        "{result:?}"
    );
    let repaired = scaled(bulging_tetrahedron(1.5), 32.0 * res).repair(&TOL, &Budget::DEFAULT);
    assert_eq!(repaired.unwrap().check(&TOL), Ok(()));

    // Running out of the budget.
    assert_eq!(
        shell(10.0, 0.05).repair(&TOL, &Budget::new(1000)),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn small_round_surfaces_are_repaired() {
    // Cylinders of radius 20 to 200 resolutions a few resolutions apart
    // need curved pieces under `MIN_SPLIT` to pass, and are split to them
    // (they gave `TooComplex`).
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let res = coarse.resolution();
    for (radius, angle, gap) in [(30.0, 45.0, 3.0), (20.0, 30.0, 1.5), (200.0, 30.0, 1.5)] {
        let mesh = cylinders(radius * res, angle, gap * res, &coarse);
        assert!(matches!(mesh.check(&coarse), Err(CheckError::Hull(..))));
        let (result, work) = repair_counting(mesh, &coarse);
        let repaired = result.unwrap();
        assert_eq!(repaired.check(&coarse), Ok(()), "{radius}");
        assert_eq!(repaired.check_faces(&coarse), Ok(()), "{radius}");
        assert!(work < 100_000, "{radius}: {work}");
    }
}

#[test]
fn pieces_too_small_to_split_fail_with_what_asked_for_it() {
    // A flat piece 0 and a curved piece 1 sharing an edge, 1 folded back
    // over 0, which the edge rule refuses; neither is degenerate, and
    // they aren't both flat, so both are to be split. Under `MIN_SPLIT`
    // resolutions the flat one can't be.
    let res = TOL.resolution();
    let faces = [face(0, Surface::Free)];
    let pair = |size: f64, shift: DVec3, ids: [u32; 4]| {
        let [a, b, c, d] = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(0.25, 0.25, 0.0)]
            .map(|p| p * size + shift);
        let mid = |p: DVec3, q: DVec3| (p + q) / 2.0;
        let flat = Patch::flat([a, b, c]).unwrap();
        let bent = mid(c, d) + DVec3::Z * 0.3 * size;
        let curved = Patch::new([c, b, d], [mid(c, b), mid(b, d), bent], [1.0; 3]).unwrap();
        assert!(hull::flat(&flat, res) && !hull::flat(&curved, res));
        let piece = |corners, patch, i: u32| Piece {
            corners,
            patch,
            face: 0,
            leaf: ids[i as usize],
            origin: ids[i as usize],
            changed: true,
        };
        [
            piece([ids[0], ids[1], ids[2]], flat, 0),
            piece([ids[2], ids[1], ids[3]], curved, 1),
        ]
    };
    let run = |pieces: &[Piece]| {
        let mut work = Work::new(&Budget::DEFAULT);
        failures(
            pieces,
            |t| &pieces[t as usize].patch,
            &faces,
            &TOL,
            &mut work,
        )
    };
    assert_eq!(
        run(&pair(100.0 * res, DVec3::ZERO, [0, 1, 2, 3])),
        Ok(vec![0, 1])
    );
    let small = pair(32.0 * res, DVec3::ZERO, [0, 1, 2, 3]);
    let refused = Err(KernelError::Invalid(CheckError::EdgeNeighbours(0, 1)));
    assert_eq!(run(&small), refused);
    // A failure no split mends, checked first, still names the error: two
    // flat triangles half a resolution apart.
    let far = DVec3::X * 1.0;
    let gap = DVec3::Z * 0.5 * res;
    let lower = Patch::flat([far, far + DVec3::X, far + DVec3::Y]).unwrap();
    let upper = Patch::flat([far + gap, far + gap + DVec3::Y, far + gap + DVec3::X]).unwrap();
    let mut pieces = small.to_vec();
    for (i, (patch, corners)) in [(lower, [4, 5, 6]), (upper, [7, 8, 9])]
        .into_iter()
        .enumerate()
    {
        pieces.push(Piece {
            corners,
            patch,
            face: 0,
            leaf: 2 + i as u32,
            origin: 2 + i as u32,
            changed: true,
        });
    }
    assert_eq!(
        run(&pieces),
        Err(KernelError::Invalid(CheckError::Hull(2, 3)))
    );
}

#[test]
fn only_curved_pieces_are_split_below_min_split() {
    let res = TOL.resolution();
    let flat = |size: f64| Patch::flat([DVec3::ZERO, DVec3::X * size, DVec3::Y * size]).unwrap();
    let curved = |size: f64| {
        let [a, b, c] = [DVec3::ZERO, DVec3::X * size, DVec3::Y * size];
        let bent = (a + c) / 2.0 + DVec3::Z * 0.3 * size;
        Patch::new([a, b, c], [(a + b) / 2.0, (b + c) / 2.0, bent], [1.0; 3]).unwrap()
    };
    assert!(splittable(&flat(MIN_SPLIT * 1.01 * res), res));
    assert!(!splittable(&flat(MIN_SPLIT * 0.99 * res), res));
    assert!(splittable(&curved(MIN_SPLIT * 0.5 * res), res));
    assert!(splittable(&curved(MIN_CURVED_SPLIT * 1.01 * res), res));
    assert!(!splittable(&curved(MIN_CURVED_SPLIT * 0.99 * res), res));
}

#[test]
fn a_witness_needs_the_leaves_apart_and_counts_planar_splits() {
    // Pieces 0 and 1 are the halves of one leaf, piece 2 shares no vertex
    // with piece 0 but one with its leaf, piece 3 none with anything.
    let res = TOL.resolution();
    let tri = |corners: [u32; 3], leaf: u32, face: u32, z: [f64; 3]| {
        let p = [0, 1, 2].map(|i| DVec3::new(corners[i] as f64, (corners[i] % 3) as f64, z[i]));
        Piece {
            corners,
            patch: crate::patch::Patch::flat(p).unwrap(),
            face,
            leaf,
            origin: leaf,
            changed: true,
        }
    };
    let faces = [
        face(0, Surface::Free),
        face(
            1,
            Surface::Plane {
                n: DVec3::Z * 1e-3,
                d: 0.0,
            },
        ),
        face(
            2,
            Surface::Plane {
                n: DVec3::ZERO,
                d: 0.0,
            },
        ),
    ];
    let flat = [0.0; 3];
    let mut pieces = vec![
        tri([0, 4, 2], 0, 0, flat),
        tri([1, 4, 2], 0, 0, flat),
        tri([1, 5, 6], 1, 0, flat),
        tri([7, 8, 9], 2, 0, flat),
    ];
    assert_eq!(witness_limit(&pieces, &faces, [0, 2], res), None);
    assert_eq!(witness_limit(&pieces, &faces, [2, 0], res), None);
    let limit = witness_limit(&pieces, &faces, [0, 3], res).unwrap();
    assert!(limit < res && limit > 0.99 * res, "{limit}");
    // On a plane, the leaf's control points a third of a resolution apart
    // along its normal (in the sibling): that much less. A plane that
    // isn't one leaves nothing.
    pieces[3].face = 1;
    let limit = witness_limit(&pieces, &faces, [0, 3], res).unwrap();
    assert!(limit > 0.99 * res, "{limit}");
    pieces[0].face = 1;
    pieces[1] = tri([1, 4, 2], 0, 1, [0.0, res / 3.0, 0.0]);
    let limit = witness_limit(&pieces, &faces, [0, 3], res).unwrap();
    assert!(limit < 0.67 * res && limit > 0.66 * res, "{limit}");
    pieces[3].face = 2;
    assert_eq!(witness_limit(&pieces, &faces, [0, 3], res), None);
    // A planar piece that may fold, whose straight pieces needn't cover
    // it, leaves nothing either.
    pieces[3].face = 1;
    assert!(witness_limit(&pieces, &faces, [0, 3], res).is_some());
    pieces[3].patch.c[0] += DVec3::Y * 10.0;
    assert_eq!(pieces[3].patch.fold_direction(), None);
    assert_eq!(witness_limit(&pieces, &faces, [0, 3], res), None);
    // Nor do halves that pass it but face opposite ways along the normal,
    // folded over each other, though they lie in the plane.
    pieces[3] = tri([7, 8, 9], 2, 0, flat);
    assert!(witness_limit(&pieces, &faces, [0, 3], res).is_some());
    let [a, b, c] = pieces[1].patch.p;
    pieces[1].patch = crate::patch::Patch::flat([a, c, b]).unwrap();
    assert!(pieces[1].patch.fold_direction().is_some());
    assert_eq!(witness_limit(&pieces, &faces, [0, 3], res), None);
    assert_eq!(witness_limit(&pieces, &faces, [1, 3], res), None);
}

#[test]
fn invalid_input_is_refused() {
    let mut mesh = tetrahedron(DVec3::ZERO);
    mesh.tris[0].halfedges[0].pair = 0;
    assert_eq!(
        mesh.repair(&TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(CheckError::Pair(0)))
    );
    let mut mesh = round_octahedron(DVec3::ZERO);
    let e = mesh.halfedge(4).edge as usize;
    mesh.edges[e].weight = 100.0;
    assert_eq!(
        mesh.repair(&TOL, &Budget::DEFAULT),
        Err(KernelError::Invalid(CheckError::Patch(
            1,
            PatchError::Weight(100.0)
        )))
    );
}

/// A tetrahedron whose base is a flat sliver `aspect` times longer than
/// wide: apex angle about `1/aspect` at the origin, its far side `width`
/// across, and the fourth corner `width` above that side's middle, so
/// every edge's rule sees the thin side across, not the long length.
fn sliver_tetrahedron(aspect: f64, width: f64) -> Mesh {
    let mut builder = MeshBuilder::new();
    let f = builder.face(face(0, Surface::Free));
    let l = aspect * width;
    let v = [
        DVec3::ZERO,
        DVec3::new(l, width / 2.0, 0.0),
        DVec3::new(l, -width / 2.0, 0.0),
        DVec3::new(l, 0.0, width),
    ]
    .map(|p| builder.vert(p));
    for [a, b, c] in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
        builder.tri([v[a], v[b], v[c]], f);
    }
    builder.build().unwrap()
}

#[test]
fn flat_slivers_failing_the_fold_check_fail_at_once() {
    // Between about 7.6e7 and 9.8e7 times longer than wide, the sliver
    // fails the fold check with no corner degenerate, while every pair
    // passes the hull rules. Its red pieces are like it and fail the same
    // way; splitting them used to go on until their pairs failed the
    // vertex rule (9 984 units, `Invalid(VertexNeighbours)`).
    for aspect in [8e7, 9e7] {
        let mesh = sliver_tetrahedron(aspect, 1e-4);
        let res = TOL.resolution();
        for t in 0..4 {
            let patch = mesh.patch(t);
            assert!(hull::flat(&patch, res));
            assert_eq!(patch.degenerate_corner(), None, "{aspect:e}");
        }
        assert!(mesh.patch(0).fold_direction().is_none(), "{aspect:e}");
        for a in 0..4u32 {
            for b in a + 1..4 {
                let ids = [a, b];
                let patches = [a, b].map(|t| mesh.patch(t as usize));
                let corners = [a, b].map(|t| mesh.corners(t));
                assert_eq!(
                    check_pair(ids, [&patches[0], &patches[1]], corners, res),
                    Ok(()),
                    "{aspect:e}"
                );
            }
        }
        let (result, work) = repair_counting(mesh, &TOL);
        assert_eq!(
            result.err(),
            Some(KernelError::Invalid(CheckError::Fold(0)))
        );
        assert!(work < 100, "{aspect:e}: {work} units");
    }
}

#[test]
fn only_affine_whole_leaves_failing_the_fold_check_fail_at_once() {
    let res = TOL.resolution();
    let faces = [face(0, Surface::Free)];
    let piece = |patch: Patch, leaf: u32, corners: [u32; 3]| Piece {
        corners,
        patch,
        face: 0,
        leaf,
        origin: leaf,
        changed: true,
    };
    let run = |pieces: &[Piece]| {
        let mut work = Work::new(&Budget::DEFAULT);
        failures(
            pieces,
            |t| &pieces[t as usize].patch,
            &faces,
            &TOL,
            &mut work,
        )
    };
    // A sliver 1.3 resolutions wide and 84 long, flat within the
    // resolution, whose control points sit off its edges' middles (one
    // edge weighted 7): it fails the fold check with no corner degenerate,
    // but each of its red pieces passes. So it is split.
    let sliver = Patch::new(
        [
            DVec3::ZERO,
            DVec3::new(8.318400314376304e-5, 0.0, 0.0),
            DVec3::new(0.00011430863617174811, 1.2758873352597488e-6, 0.0),
        ],
        [
            DVec3::new(
                7.385575459372186e-5,
                -8.813299798056541e-7,
                -3.155804839965728e-7,
            ),
            DVec3::new(
                8.501320431512118e-5,
                1.0346155606943339e-7,
                8.219087101315864e-8,
            ),
            DVec3::new(
                2.4727440750598842e-5,
                -4.3043062918168886e-7,
                6.177306275150514e-7,
            ),
        ],
        [1.0, 6.966971185462435, 1.0],
    )
    .unwrap();
    assert!(hull::flat(&sliver, res));
    assert_eq!(sliver.fold_direction(), None);
    assert_eq!(sliver.degenerate_corner(), None);
    assert!(
        sliver
            .split4()
            .unwrap()
            .iter()
            .all(|k| k.fold_direction().is_some())
    );
    assert_eq!(run(&[piece(sliver, 0, [0, 1, 2])]), Ok(vec![0]));

    // An affine sliver failing it fails at once as a whole leaf. As one
    // half of a green leaf it is split: the leaf, of another shape, may
    // pass.
    let affine = sliver_tetrahedron(9e7, 1e-4).patch(0);
    assert_eq!(affine.fold_direction(), None);
    assert_eq!(affine.degenerate_corner(), None);
    assert_eq!(
        run(&[piece(affine, 0, [0, 1, 2])]),
        Err(KernelError::Invalid(CheckError::Fold(0)))
    );
    let far = Patch::flat([
        DVec3::splat(1e3),
        DVec3::new(1e3 + 1.0, 1e3, 1e3),
        DVec3::new(1e3, 1e3 + 1.0, 1e3),
    ])
    .unwrap();
    assert_eq!(
        run(&[piece(affine, 0, [0, 1, 2]), piece(far, 0, [3, 4, 5])]),
        Ok(vec![0])
    );
}

#[test]
fn a_failing_repair_gives_back_what_it_was_given() {
    // Failures no split mends at once (flat boxes face to face, round
    // surfaces touching), and those found only after splitting (small
    // cylinders reaching the smallest piece, a small bulging
    // tetrahedron): repair gives the mesh back as it was, with the error
    // and work it gave before, and the error names its triangles, which
    // a solid finished from it (`Solid::finished`, a revolve's or a
    // boolean's) carries as its evidence. The same at 1 and 8 threads.
    let gap = 0.5 * TOL.resolution();
    let coarse = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let boxes = both(
        &Mesh::cuboid(DVec3::ZERO, DVec3::ONE, 1, &TOL).unwrap(),
        &Mesh::cuboid(DVec3::Z * (1.0 + gap), DVec3::ONE, 2, &TOL).unwrap(),
    );
    let small = 10.0 * coarse.resolution();
    let res = TOL.resolution();
    for (mesh, tol) in [
        (boxes, TOL),
        (two_spheres(1.0, gap), TOL),
        (
            cylinders(small, 30.0, 1.5 * coarse.resolution(), &coarse),
            coarse,
        ),
        (scaled(bulging_tetrahedron(1.5), res), TOL),
    ] {
        let (want, units) = repair_counting(mesh.clone(), &tol);
        let error = want.unwrap_err();
        let KernelError::Invalid(why) = error else {
            panic!("{error:?}");
        };
        let mut work = Work::new(&Budget::DEFAULT);
        let (given_error, given) = mesh.clone().repair_or_given(&tol, &mut work).unwrap_err();
        assert_eq!(given_error, error);
        assert_eq!(*given, mesh);
        assert_eq!(Budget::DEFAULT.work() - work.left(), units);

        let failure = assert_deterministic(|| {
            let mut work = Work::new(&Budget::DEFAULT);
            let failure = crate::Solid::finished(mesh.clone(), &tol, &mut work).unwrap_err();
            (failure, work.left())
        });
        assert_eq!(failure.0.error, error);
        assert_eq!(Budget::DEFAULT.work() - failure.1, units);
        let named: Vec<Patch> = match why {
            CheckError::Fold(t) => vec![mesh.patch(t as usize)],
            CheckError::Hull(t, u)
            | CheckError::EdgeNeighbours(t, u)
            | CheckError::VertexNeighbours(t, u) => {
                vec![mesh.patch(t as usize), mesh.patch(u as usize)]
            }
            why => panic!("{why:?}"),
        };
        assert_eq!(failure.0.evidence.patches, named, "{why:?}");
        assert!(!failure.0.evidence.truncated);
    }
}
