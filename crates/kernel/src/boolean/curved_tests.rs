//! Booleans of solids with curved patches: cylinders and boxes, whose
//! cuts are exact, and crossing cylinders, a free surface and a saddle,
//! whose cuts are traced and fitted.

#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::PI;

use glam::{DMat3, DQuat, DVec2, DVec3};

use super::pairs::tests::{cylinder_x, poke, reach, saddle};
use super::*;
use crate::mesh::tests::TOL;
use crate::mesh::{Quadric, Surface, samples};
use crate::par::assert_deterministic;
use crate::profile::tests::{circle, rect};
use crate::{Frame, Loop, Profile, Segment, extrude};

fn cube(min: [f64; 3], size: [f64; 3]) -> Solid {
    Solid::cuboid(DVec3::from(min), DVec3::from(size), 1, &TOL).unwrap()
}

fn cylinder(base: [f64; 3], r: f64, h: f64) -> Solid {
    Solid::cylinder(DVec3::from(base), r, h, 2, &TOL).unwrap()
}

/// The loops on the XY plane extruded from `from` to `to`.
fn extruded(loops: Vec<Loop>, from: f64, to: f64, feature: u64) -> Solid {
    extruded_on(loops, Frame::XY, from, to, feature)
}

/// The loops on `frame` extruded from `from` to `to`.
fn extruded_on(loops: Vec<Loop>, frame: Frame, from: f64, to: f64, feature: u64) -> Solid {
    let profile = Profile { loops };
    extrude(&profile, &frame, from, to, feature, &TOL, &Budget::DEFAULT).unwrap()
}

/// A 6 × 4 plate on XY, 1 thick, with a hole of radius 1 at (0.5, 0.2).
fn plate() -> Solid {
    extruded(
        vec![
            rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0),
            circle(DVec2::new(0.5, 0.2), 1.0, 4, true),
        ],
        0.0,
        1.0,
        5,
    )
}

/// `solid` moved by the rigid motion `f`, face tags too.
fn moved(solid: &Solid, f: impl Fn(DVec3) -> DVec3) -> Solid {
    moved_at(solid, &TOL, f)
}

/// [`moved`], checked at `tol`.
fn moved_at(solid: &Solid, tol: &Tolerance, f: impl Fn(DVec3) -> DVec3) -> Solid {
    let mesh = solid.mesh();
    let origin = f(DVec3::ZERO);
    let turn = |d: DVec3| f(d) - origin;
    let r = DMat3::from_cols(turn(DVec3::X), turn(DVec3::Y), turn(DVec3::Z));
    let mut builder = crate::mesh::MeshBuilder::new();
    for &p in mesh.verts() {
        builder.vert(f(p));
    }
    for &face in mesh.faces() {
        let surface = match face.surface {
            Surface::Plane { n, d } => {
                let n = r * n;
                Surface::Plane {
                    n,
                    d: d + n.dot(origin),
                }
            }
            Surface::Quadric(q) => Surface::Quadric(Quadric {
                origin: f(q.origin),
                a: r * q.a * r.transpose(),
                b: r * q.b,
                c: q.c,
            }),
            Surface::Free => Surface::Free,
        };
        builder.face(crate::mesh::Face { surface, ..face });
    }
    for (t, tri) in mesh.tris().iter().enumerate() {
        let corners = tri.halfedges.map(|h| h.start);
        let patch = mesh.patch(t);
        for i in 0..3 {
            builder.edge(corners[i], corners[(i + 1) % 3], f(patch.c[i]), patch.w[i]);
        }
        builder.tri(corners, tri.face);
    }
    Solid::new(builder.build().unwrap(), tol).unwrap()
}

fn run(a: &Solid, b: &Solid, op: Op) -> Solid {
    boolean(a, b, op, &TOL, &Budget::DEFAULT).unwrap_or_else(|e| panic!("{op:?} failed: {e:?}"))
}

/// `a ∪ b`, `a ∩ b`, `a − b` and `b − a`, each checked (by `Solid`)
/// with its face tags, and `a ∩ b` and `b ∩ a` both, their volumes
/// within `within` of each other.
fn all_four(a: &Solid, b: &Solid, within: f64) -> [Solid; 4] {
    let both = run(a, b, Op::Intersection);
    let other = run(b, a, Op::Intersection);
    assert!((both.volume() - other.volume()).abs() <= within);
    [
        run(a, b, Op::Union),
        both,
        run(a, b, Op::Difference),
        run(b, a, Op::Difference),
    ]
}

/// Checks the four results' volumes against those of `a ∩ b` = `both`
/// (the rest follow from `a` and `b`'s), within `within`.
fn volumes(name: &str, a: &Solid, b: &Solid, results: &[Solid; 4], both: f64, within: f64) {
    let (va, vb) = (a.volume(), b.volume());
    let want = [va + vb - both, both, va - both, vb - both];
    for (k, (solid, want)) in results.iter().zip(want).enumerate() {
        let got = solid.volume();
        assert!(
            (got - want).abs() <= within,
            "{name}, result {k}: volume {got}, not {want}"
        );
    }
}

/// How far the furthest sampled point of any patch is from its face's
/// surface, and how many patches claim none.
fn off_surface(solid: &Solid) -> (f64, usize) {
    let mesh = solid.mesh();
    let mut worst = 0.0f64;
    let mut free = 0;
    for (t, tri) in mesh.tris().iter().enumerate() {
        let surface = mesh.faces()[tri.face as usize].surface;
        if matches!(surface, Surface::Free) {
            free += 1;
            continue;
        }
        let patch = mesh.patch(t);
        for u in samples() {
            worst = worst.max(surface.distance(patch.eval(u)));
        }
    }
    (worst, free)
}

/// Every patch on its face's plane or quadric to `rel` of the model's
/// size: no patch had to be fitted.
fn exact_to(name: &str, solid: &Solid, size: f64, rel: f64) {
    let (worst, free) = off_surface(solid);
    assert_eq!(free, 0, "{name}: {free} patches fitted");
    assert!(
        worst <= rel * size,
        "{name}: a patch {worst:e} off its surface"
    );
}

/// [`exact_to`] about `1e-12`.
fn exact(name: &str, solid: &Solid, size: f64) {
    exact_to(name, solid, size, 1e-12);
}

#[test]
fn a_cylinder_through_a_box_is_exact() {
    let slab = cube([-2.0, -2.0, 0.0], [4.0, 4.0, 2.0]);
    let bar = cylinder([0.0, 0.0, -1.0], 1.0, 4.0);
    let results = all_four(&slab, &bar, 1e-12);
    volumes("through", &slab, &bar, &results, 2.0 * PI, 1e-12);
    for solid in &results {
        exact("through", solid, 4.0);
    }
    // The hole: the slab less the bar keeps the slab's faces and the
    // bar's wall between them, turned in.
    let hole = &results[2];
    let names: std::collections::BTreeSet<_> = hole.mesh().faces().iter().map(|f| f.name).collect();
    assert!(names.iter().any(|n| n.feature == 2));
}

#[test]
fn blind_and_small_holes_are_exact() {
    let slab = cube([-2.0, -2.0, 0.0], [4.0, 4.0, 2.0]);
    // Ending inside the slab.
    let bar = cylinder([0.3, -0.2, 1.0], 0.7, 2.0);
    let results = all_four(&slab, &bar, 1e-12);
    volumes("blind", &slab, &bar, &results, PI * 0.49, 1e-12);
    // All within one triangle of each of the slab's faces: the cut
    // faces' parts are whole discs, around no corner.
    let pin = cylinder([1.0, -1.2, -1.0], 0.3, 4.0);
    let small = all_four(&slab, &pin, 1e-12);
    volumes("small", &slab, &pin, &small, PI * 0.09 * 2.0, 1e-12);
    for solid in results.iter().chain(&small) {
        exact("holes", solid, 4.0);
    }
}

#[test]
fn a_tilted_bar_is_exact() {
    // Cut by the slab's faces in ellipses; its wall's bands between them
    // exact by the common-point construction.
    let q = DQuat::from_rotation_x(0.4) * DQuat::from_rotation_y(0.3);
    let bar = moved(&cylinder([0.0, 0.0, -3.0], 0.8, 6.0), |p| q * p);
    let slab = cube([-2.0, -2.0, -0.5], [4.0, 4.0, 1.5]);
    let axis = q * DVec3::Z;
    let both = PI * 0.64 * 1.5 / axis.z;
    let results = all_four(&slab, &bar, 1e-11);
    volumes("tilted", &slab, &bar, &results, both, 1e-11);
    for solid in &results {
        exact("tilted", solid, 4.0);
    }
}

/// The volume of the upright unit cylinder crossed by one of radius `r`
/// along `x` through `(·, y0, ·)`: `∫ 2√(1 − y²) · 2√(r² − (y − y0)²) dy`,
/// with `y = y0 + r·sin φ`, by Simpson's rule.
fn crossed(r: f64, y0: f64) -> f64 {
    let n = 20_000;
    let f = |phi: f64| {
        let y = y0 + r * phi.sin();
        4.0 * r * r * phi.cos().powi(2) * (1.0 - y * y).sqrt()
    };
    let (a, b) = (-PI / 2.0, PI / 2.0);
    let h = (b - a) / n as f64;
    let mut sum = f(a) + f(b);
    for k in 1..n {
        sum += f(a + k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

#[test]
fn crossing_cylinders_are_traced_within_the_tolerance() {
    let upright = cylinder([0.0, 0.0, -2.0], 1.0, 4.0);
    let across = cylinder_x(0.1, 0.2, 0.7, -2.0, 2.0);
    // Every patch within the fit tolerance of the true surfaces: the
    // volumes within it times their areas.
    let area = upright.area() + across.area();
    let results = all_four(&upright, &across, TOL.fit() * area / 10.0);
    volumes(
        "crossing",
        &upright,
        &across,
        &results,
        crossed(0.7, 0.1),
        TOL.fit() * area / 10.0,
    );
    let on = |p: DVec3| {
        let a = DVec2::new(p.x, p.y).length() - 1.0;
        let b = DVec2::new(p.y - 0.1, p.z - 0.2).length() - 0.7;
        a.abs().min(b.abs())
    };
    for solid in &results {
        // The cut's vertices on both cylinders; every vertex on one.
        for &p in solid.mesh().verts() {
            if p.z.abs() < 1.99 && p.x.abs() < 1.99 {
                assert!(on(p) <= TOL.fit() / 4.0, "{p}");
            }
        }
        // The bands along the cut are fitted, the rest exact.
        let (worst, free) = off_surface(solid);
        assert!(free > 0);
        assert!(worst <= 1e-12 * 4.0, "{worst:e}");
    }
}

#[test]
fn a_pin_through_a_holes_wall() {
    // Two upright cylinders crossing in straight lines, and the plate's
    // faces: all exact.
    let plate = plate();
    let pin = extruded(
        vec![circle(DVec2::new(1.4, 0.2), 0.3, 0, false)],
        -1.0,
        3.0,
        7,
    );
    // The pin's disc less the part in the hole: two circles' lens.
    let (r, big, d) = (0.3f64, 1.0f64, 0.9f64);
    let lens = r * r * ((d * d + r * r - big * big) / (2.0 * d * r)).acos()
        + big * big * ((d * d + big * big - r * r) / (2.0 * d * big)).acos()
        - 0.5 * ((-d + r + big) * (d + r - big) * (d - r + big) * (d + r + big)).sqrt();
    let results = all_four(&plate, &pin, 1e-10);
    volumes("pin", &plate, &pin, &results, PI * r * r - lens, 1e-10);
    for solid in &results {
        exact("pin", solid, 6.0);
    }
}

#[test]
fn a_boss_joined_flush() {
    // Standing on the plate's top: the faces flush where they meet, the
    // plate's top cut round the boss's foot.
    let plate = plate();
    let boss = extruded(
        vec![circle(DVec2::new(-1.5, 0.0), 0.4, 0, false)],
        1.0,
        2.0,
        6,
    );
    let joined = run(&plate, &boss, Op::Union);
    let want = plate.volume() + boss.volume();
    assert!(
        (joined.volume() - want).abs() < 1e-10,
        "{}",
        joined.volume()
    );
    exact("boss", &joined, 6.0);
    // Nothing of either is inside the other.
    assert!(run(&plate, &boss, Op::Intersection).is_empty());
    let less = run(&plate, &boss, Op::Difference);
    assert!((less.volume() - plate.volume()).abs() < 1e-10);
}

/// The area of the plate's hole (radius 1 round (0.5, 0.2)) right of
/// `x0` between `y0` and `y1`, by Simpson's rule.
fn hole_part(x0: f64, y0: f64, y1: f64) -> f64 {
    let n = 20_000;
    let h = (y1 - y0) / n as f64;
    let width = |y: f64| {
        let dy = y - 0.2;
        let half = (1.0 - dy * dy).max(0.0).sqrt();
        (0.5 + half - x0).max(0.0)
    };
    let mut sum = width(y0) + width(y1);
    for k in 1..n {
        sum += width(y0 + k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

#[test]
fn a_block_through_a_plates_hole() {
    // Plane against plane gives straight cuts, plane against the hole's
    // wall arcs, and the block's side cuts the hole's wall in lines. The
    // second block's side runs exactly through a vertex of the plate's
    // caps: a tie.
    let plate = plate();
    for (min, x1) in [([0.95, -0.45, -1.0], 3.0), ([0.9, -0.5, -1.0], 3.0)] {
        let block = cube(min, [3.0, 1.0, 3.0]);
        let results = all_four(&plate, &block, 1e-9);
        let footprint = (x1 - min[0]) * 1.0;
        let both = footprint - hole_part(min[0], min[1], min[1] + 1.0);
        volumes("block", &plate, &block, &results, both, 1e-9);
        for solid in &results {
            exact("block", solid, 6.0);
        }
    }
}

#[test]
fn a_free_surface_against_a_plane() {
    // A round octahedron (not a quadric: traced and fitted) cut by a
    // slab's face through its middle, and cutting off a small cap no edge
    // crosses (a loop only refinement finds).
    let slack = |a: &Solid, b: &Solid| TOL.fit() * (a.area() + b.area()) / 10.0;
    let (ball, slab) = poke(0.3);
    let results = all_four(&ball, &slab, slack(&ball, &slab));
    let both = results[1].volume();
    volumes("ball", &ball, &slab, &results, both, slack(&ball, &slab));
    let (ball, slab) = poke(reach() - 0.01);
    let results = all_four(&ball, &slab, slack(&ball, &slab));
    let cap = results[2].volume();
    assert!(cap > 0.0 && cap < 1e-3, "{cap}");
    volumes(
        "hidden",
        &ball,
        &slab,
        &results,
        ball.volume() - cap,
        slack(&ball, &slab),
    );
    // The slab's plane stays exact, and so do the parts of the ball's
    // patches away from the cut.
    for solid in &results {
        let (worst, _) = off_surface(solid);
        assert!(worst <= 1e-12 * 400.0, "{worst:e}");
    }
}

#[test]
fn a_saddle_pairs_its_cut_the_right_way() {
    // Above the saddle point the plane cuts the saddle in two branches,
    // below it in one band; the pairing decided on the saddle's patch
    // has to follow.
    let solid = saddle();
    for c in [10.2, 9.8] {
        let slab = cube([-5.0, -5.0, -1.0], [14.0, 14.0, c + 1.0]);
        let within = TOL.fit() * (solid.area() + slab.area()) / 10.0;
        let results = all_four(&solid, &slab, within);
        let both = results[1].volume();
        volumes("saddle", &solid, &slab, &results, both, within);
        // Above the plane, a band or two lobes.
        let above = &results[2];
        assert!(above.volume() > 0.0);
    }
}

/// `x op y` for two cylinders side by side, touching along a line, at
/// the coarsest tolerance (to keep the tests quick).
fn tangent(op: Op, swap: bool) -> Result<(Solid, f64), KernelError> {
    let tol = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    let a = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
    let b = Solid::cylinder(DVec3::new(2.0, 0.0, 0.5), 1.0, 1.0, 3, &tol).unwrap();
    let (x, y) = if swap { (b, a) } else { (a, b) };
    boolean(&x, &y, op, &tol, &Budget::DEFAULT).map(|s| (s, x.volume()))
}

#[test]
fn tangent_cylinders_meet_in_no_manifold() {
    // Their union isn't a manifold (as boxes touching along an edge), and
    // they have nothing in common.
    assert!(tangent(Op::Union, false).is_err());
    assert!(tangent(Op::Intersection, false).unwrap().0.is_empty());
}

#[test]
fn tangent_cylinders_less_each_other_are_themselves() {
    for swap in [false, true] {
        let (less, whole) = tangent(Op::Difference, swap).unwrap();
        assert!((less.volume() - whole).abs() < 1e-9);
    }
}

#[test]
fn cylinders_a_hair_apart_are_right_or_refused() {
    // Side by side 1e-9 apart, far below the resolution at the coarsest
    // tolerance: near ties the curved primitives decide as ties. Every
    // result must be the operands as they are, or an error; the seam
    // vertex's ray once took the gap as closed, with no pair of faces
    // there to cut, and put all of one inside the other.
    let tol = Tolerance::new(Tolerance::MAX_FIT).unwrap();
    for (z0, h) in [(0.0, 2.0), (-0.5, 3.0)] {
        let a = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 2, &tol).unwrap();
        let b = Solid::cylinder(DVec3::new(2.0 + 1e-9, 0.0, z0), 1.0, h, 3, &tol).unwrap();
        let (va, vb) = (a.volume(), b.volume());
        for (x, y, op, want) in [
            (&a, &b, Op::Union, va + vb),
            (&b, &a, Op::Union, va + vb),
            (&a, &b, Op::Intersection, 0.0),
            (&b, &a, Op::Intersection, 0.0),
            (&a, &b, Op::Difference, va),
            (&b, &a, Op::Difference, vb),
        ] {
            if let Ok(solid) = boolean(x, y, op, &tol, &Budget::DEFAULT) {
                let got = solid.volume();
                assert!(
                    (got - want).abs() < 1e-6,
                    "{z0} {h}, {op:?}: {got}, not {want}"
                );
            }
        }
    }
}

#[test]
fn a_bar_cut_at_its_refinement_midpoints_keeps_its_planes() {
    // A box's face through the middle of a bar's wall, where refinement
    // put its midpoints: crossings at one place, which the clean-up
    // collapses. Two edges it joined there had different curves (a cut's
    // arc on the face's plane, and the wall's inner edge 0.02 off it):
    // the box's face took the wall's, and repair trusted its tag. Now
    // right or refused.
    let block = extruded_on(
        vec![rect(DVec2::new(-0.75, -0.375), DVec2::new(0.75, 1.875), 0)],
        Frame {
            origin: DVec3::new(0.25, -1.0, -0.5),
            x: DVec3::Y,
            y: DVec3::Z,
        },
        -1.25,
        0.5,
        1,
    );
    let bar = extruded_on(
        vec![circle(DVec2::new(0.0, 0.75), 0.75, 0, false)],
        Frame {
            origin: DVec3::new(0.5, -0.5, 0.5),
            x: DVec3::Z,
            y: DVec3::X,
        },
        -1.5,
        -1.0,
        2,
    );
    // The bar's part in the block: a circular segment, its chord 0.5
    // from the axis, a quarter long.
    let (r, d) = (0.75f64, 0.5f64);
    let both = 0.25 * (r * r * (d / r).acos() - d * (r * r - d * d).sqrt());
    let (va, vb) = (bar.volume(), block.volume());
    for (x, y, op, want) in [
        (&bar, &block, Op::Difference, va - both),
        (&block, &bar, Op::Difference, vb - both),
        (&bar, &block, Op::Union, va + vb - both),
        (&bar, &block, Op::Intersection, both),
    ] {
        if let Ok(solid) = boolean(x, y, op, &TOL, &Budget::DEFAULT) {
            assert!((solid.volume() - want).abs() < 1e-9, "{op:?}");
        }
    }
}

#[test]
fn a_crossing_the_search_misses_stays_on_its_plane() {
    // A tilted bar's arc edge crosses a plate's cap so near a side of the
    // cap's triangle that the search for the crossing the count has
    // finds none, and it goes where the two came closest: 1e-4 off the
    // cap's plane, whose tag then claimed triangles off it.
    let bar = extruded_on(
        vec![circle(DVec2::new(0.25, -0.75), 0.75, 0, false)],
        Frame {
            origin: DVec3::new(0.0, 0.5, 0.75),
            x: DVec3::new(
                0.9911369476818921,
                0.10649674407834678,
                -0.07941029177968072,
            ),
            y: DVec3::new(0.0, 0.5977706251198731, 0.8016671876432242),
        },
        -1.25,
        1.0,
        1,
    );
    let plate = extruded_on(
        vec![
            rect(DVec2::new(-1.0, -1.5), DVec2::new(1.0, 1.5), 0),
            circle(DVec2::new(0.25, 0.0), 0.25, 10, true),
        ],
        Frame {
            origin: DVec3::new(1.0, 0.0, 0.25),
            x: DVec3::Y,
            y: DVec3::Z,
        },
        -0.25,
        0.5,
        2,
    );
    let got = [
        (&bar, &plate, Op::Union),
        (&bar, &plate, Op::Intersection),
        (&bar, &plate, Op::Difference),
        (&plate, &bar, Op::Difference),
    ]
    .map(|(x, y, op)| {
        let solid = boolean(x, y, op, &TOL, &Budget::DEFAULT).ok()?;
        Some(solid.volume())
    });
    let (va, vb) = (bar.volume(), plate.volume());
    let within = TOL.fit() * (bar.area() + plate.area()) / 100.0;
    if let [Some(u), Some(i), Some(d), Some(e)] = got {
        assert!((u + i - va - vb).abs() <= within);
        assert!((d - (va - i)).abs() <= within);
        assert!((e - (vb - i)).abs() <= within);
    }
}

#[test]
fn chained_results() {
    // A plate drilled twice, a boss joined on and drilled through: each
    // result fed on, every step exact.
    let slab = cube([-3.0, -2.0, 0.0], [6.0, 4.0, 1.0]);
    let drill = |x: f64, y: f64, r: f64| cylinder([x, y, -1.0], r, 4.0);
    let mut solid = run(&slab, &drill(-1.5, 0.0, 0.5), Op::Difference);
    solid = run(&solid, &drill(1.2, 0.3, 0.7), Op::Difference);
    let boss = cylinder([0.0, 0.2, 1.0], 0.4, 1.0);
    solid = run(&solid, &boss, Op::Union);
    solid = run(&solid, &drill(0.0, 0.2, 0.2), Op::Difference);
    let want = 24.0 - PI * (0.25 + 0.49) + PI * 0.16 - PI * 0.04 * 2.0;
    assert!(
        (solid.volume() - want).abs() < 1e-9,
        "{} {want}",
        solid.volume()
    );
    // Thin band triangles along the cuts, beside nearly coincident
    // points, lose some digits.
    exact_to("chained", &solid, 6.0, 1e-10);
}

#[test]
fn flush_bosses_joined_on_drilled_plates() {
    // A plate drilled twice, then a boss standing on it joined: the boss's
    // wall is cut along its rim, flush with the plate's cap, and the cut's
    // curves halved in rounds lie on the rim. A rim that didn't get the
    // halves' vertices kept a band of zero width with a triangle whose
    // three corners lay on it (three arcs of one circle), and a cut
    // inverted into the wall's domain a rounding inside its side or not
    // was triangulated as a fan from the corner across it; both fold.
    // These failed so before (as did one in ten such random cases).
    let slab = cube([-3.0, -2.0, 0.0], [6.0, 4.0, 1.0]);
    let drill = |x: f64, y: f64, r: f64| cylinder([x, y, -1.0], r, 4.0);
    let cases = [
        [(-1.55, 0.6, 0.3), (0.65, -0.75, 0.4), (0.95, 0.0, 0.65)],
        [(-1.55, -0.15, 0.25), (0.5, 0.05, 0.5), (-0.05, -0.35, 0.85)],
        [(-1.5, -0.65, 0.55), (2.15, -0.1, 0.65), (0.2, 0.4, 0.3)],
        [(-0.8, 0.45, 0.4), (2.15, 0.2, 0.55), (0.5, -0.45, 0.5)],
        [(-2.0, -0.05, 0.3), (0.8, 0.65, 0.5), (-0.2, 0.3, 0.3)],
    ];
    let mut refused = 0;
    for [(x1, y1, r1), (x2, y2, r2), (bx, by, br)] in cases {
        let plate = run(&slab, &drill(x1, y1, r1), Op::Difference);
        let plate = run(&plate, &drill(x2, y2, r2), Op::Difference);
        let boss = cylinder([bx, by, 1.0], br, 1.0);
        // One of them has a crossing of a cap edge through the hole's
        // wall at its rim that the search only placed, 7.6e-5 off the
        // wall: refused, since nothing puts such a crossing on a quadric
        // yet.
        let joined = match boolean(&plate, &boss, Op::Union, &TOL, &Budget::DEFAULT) {
            Ok(joined) => joined,
            Err(KernelError::Boolean(BooleanError::Inconsistent)) => {
                refused += 1;
                continue;
            }
            Err(e) => panic!("boss at ({bx}, {by}): {e:?}"),
        };
        // Where the boss covers a hole, its cap and the hole's wall meet
        // flush along arcs of both rims, some of it fitted.
        let want = 24.0 - PI * (r1 * r1 + r2 * r2) + PI * br * br;
        assert!(
            (joined.volume() - want).abs() < 1e-6,
            "boss at ({bx}, {by}): {} not {want}",
            joined.volume()
        );
    }
    assert!(refused <= 1, "{refused}");
}

#[test]
fn the_result_checks_integrations_are_charged() {
    // A tube whose wall is a twentieth thick, notched through the wall:
    // the corner triangles' volume can't tell which way the result
    // faces, so its check integrates some of the patches. The
    // operation's work is what making the mesh takes, a few units a
    // patch for the check, and `INTEGRATE_WORK` for each patch it
    // integrated, charged after: the operation fits that budget exactly.
    let tube = extruded(
        vec![
            circle(DVec2::ZERO, 1.0, 1, false),
            circle(DVec2::ZERO, 0.95, 2, true),
        ],
        0.0,
        1.0,
        3,
    );
    let notch = cube([0.9, -0.1, 0.5], [0.2, 0.2, 1.0]);
    // The notch takes the wall where `|y| < 0.1` (the inner circle there
    // is past `x = 0.9`), half the tube's height.
    let strip = |r: f64| 0.1 * (r * r - 0.01).sqrt() + r * r * crate::trig::asin(0.1 / r);
    let want = PI * (1.0 - 0.95 * 0.95) - 0.5 * (strip(1.0) - strip(0.95));
    let mut work = Work::new(&Budget::DEFAULT);
    let mesh = unchecked(&tube, &notch, Op::Difference, &TOL, &mut work).unwrap();
    let mesh = mesh.repair_within(&TOL, &mut work).unwrap();
    let integrated = mesh.check_counted(&TOL).unwrap();
    assert!(integrated > 0);
    assert!((Solid::new(mesh.clone(), &TOL).unwrap().volume() - want).abs() < 1e-9);
    let made = Budget::DEFAULT.work() - work.left();
    let total =
        made + (mesh.tris().len() * CHECK_WORK + integrated * crate::solid::INTEGRATE_WORK) as u64;
    let with = |work: u64| boolean(&tube, &notch, Op::Difference, &TOL, &Budget::new(work));
    assert_eq!(with(total).map(|s| s.mesh().clone()), Ok(mesh));
    assert_eq!(with(total - 1), Err(KernelError::TooComplex));
}

#[test]
fn flush_rims_take_the_halved_cuts_vertices() {
    // A cylinder inside a larger one, sharing its top's plane (off
    // centre, on a sketch plane): the cut of the larger's top along the
    // smaller's flush rim is halved in rounds, and the rim, whose pieces
    // the cut lies on, takes the halves' vertices too. Without them the
    // band of zero width between the two kept a triangle with three
    // corners on the rim, which folds. The union is the larger.
    let frame = Frame {
        origin: DVec3::new(0.75, -0.25, 0.0),
        x: DVec3::Y,
        y: DVec3::Z,
    };
    let a = extruded_on(
        vec![circle(DVec2::new(0.25, -0.25), 0.5, 0, false)],
        frame,
        -0.25,
        0.5,
        1,
    );
    let b = extruded_on(
        vec![circle(DVec2::new(0.0, -0.5), 1.0, 0, false)],
        frame,
        -1.0,
        0.5,
        2,
    );
    let union = run(&a, &b, Op::Union);
    assert!(
        (union.volume() - b.volume()).abs() < 1e-9,
        "{} {}",
        union.volume(),
        b.volume()
    );
}

#[test]
fn over_refined_patches_merge_back() {
    // The ball just inside the slab: the pairs near its top were refined
    // (the slab's face too) to certify they hold no loop, and nothing is
    // cut. Each result is the operand it keeps, patch for patch.
    let (ball, slab) = poke(reach() + 0.01);
    let union = run(&slab, &ball, Op::Union);
    assert_eq!(union.mesh().tris().len(), slab.mesh().tris().len());
    let both = run(&slab, &ball, Op::Intersection);
    assert_eq!(both.mesh().tris().len(), ball.mesh().tris().len());
    assert!((both.volume() - ball.volume()).abs() < 1e-12);
}

#[test]
fn curved_booleans_are_deterministic() {
    let upright = cylinder([0.0, 0.0, -2.0], 1.0, 4.0);
    let across = cylinder_x(0.1, 0.2, 0.7, -2.0, 2.0);
    assert_deterministic(|| run(&upright, &across, Op::Union));
    let slab = cube([-2.0, -2.0, 0.0], [4.0, 4.0, 2.0]);
    let bar = cylinder([0.3, -0.2, 1.0], 0.7, 2.0);
    assert_deterministic(|| run(&slab, &bar, Op::Difference));
}

#[test]
fn random_bars_through_boxes_are_right_or_refused() {
    // Turned and moved bars against boxes: every result checked and with
    // its volume, or refused as invalid (a hull or fold repair can't
    // mend) or too complex; never a wrong solid.
    let mut rng = crate::test_rng::Rng::new(7);
    let mut done = 0;
    for _ in 0..24 {
        let c = rng.point(1.0);
        let r = rng.log_range(0.2, 1.5);
        let q = DQuat::from_rotation_x(rng.range(-1.0, 1.0))
            * DQuat::from_rotation_y(rng.range(-1.0, 1.0));
        let bar = moved(&cylinder([0.0, 0.0, -2.0], r, 4.0), |p| q * p + c);
        let min = rng.point(1.0) - DVec3::splat(1.5);
        let size = DVec3::new(
            rng.range(0.5, 3.0),
            rng.range(0.5, 3.0),
            rng.range(0.5, 3.0),
        );
        let block = Solid::cuboid(min, size, 1, &TOL).unwrap();
        let jobs = [
            (&bar, &block, Op::Union),
            (&bar, &block, Op::Intersection),
            (&bar, &block, Op::Difference),
            (&block, &bar, Op::Difference),
        ];
        let got = jobs.map(
            |(x, y, op)| match boolean(x, y, op, &TOL, &Budget::DEFAULT) {
                Ok(solid) => Some(solid.volume()),
                Err(KernelError::Invalid(_) | KernelError::TooComplex) => None,
                Err(e) => panic!("{op:?}: {e:?}"),
            },
        );
        if let [Some(u), Some(i), Some(d), Some(e)] = got {
            let (va, vb) = (bar.volume(), block.volume());
            let within = TOL.fit() * (bar.area() + block.area()) / 100.0;
            assert!((u + i - va - vb).abs() <= within);
            assert!((d - (va - i)).abs() <= within);
            assert!((e - (vb - i)).abs() <= within);
            done += 1;
        }
    }
    // Most go through.
    assert!(done >= 20, "{done}");
}

/// How far the furthest sample point of `solid`'s claim-free patches is
/// from the surface of the face of `of` it was split off (by name).
fn free_off(solid: &Solid, of: &[crate::mesh::Face]) -> f64 {
    let mesh = solid.mesh();
    let mut worst = 0.0f64;
    for (t, tri) in mesh.tris().iter().enumerate() {
        let face = mesh.faces()[tri.face as usize];
        if !matches!(face.surface, Surface::Free) {
            continue;
        }
        let Some(source) = of.iter().find(|f| f.name == face.name) else {
            continue;
        };
        let patch = mesh.patch(t);
        for u in samples() {
            let d = source.surface.distance(patch.eval(u));
            worst = worst.max(if d.is_nan() { f64::INFINITY } else { d });
        }
    }
    worst
}

/// The faces of `solid` on quadrics.
fn walls(solid: &Solid) -> Vec<crate::mesh::Face> {
    solid
        .mesh()
        .faces()
        .iter()
        .filter(|f| matches!(f.surface, Surface::Quadric(_)))
        .copied()
        .collect()
}

/// How far off both operands' surfaces the vertices of `result` that are
/// no operand's vertex and lie within a thousand resolutions of both are,
/// at most: where the operands meet, which a new vertex must be on (to
/// the resolution). Each operand's surfaces near the vertex: those of its
/// faces with a patch whose box, grown by the thousand resolutions, holds
/// it.
fn off_both(result: &Solid, a: &Solid, b: &Solid) -> f64 {
    let reach = 1000.0 * TOL.resolution();
    let old: Vec<[u64; 3]> = a
        .mesh()
        .verts()
        .iter()
        .chain(b.mesh().verts())
        .map(|p| p.to_array().map(f64::to_bits))
        .collect();
    let near = |solid: &Solid, x: DVec3| {
        let mesh = solid.mesh();
        (0..mesh.tris().len())
            .filter(|&t| {
                let b = mesh.patch(t).bounds();
                (b.min - x).max(x - b.max).max_element() <= reach
            })
            .map(|t| mesh.faces()[mesh.tris()[t].face as usize].surface)
            .filter(|s| !matches!(s, Surface::Free))
            .map(|s| s.distance(x))
            .fold(f64::INFINITY, f64::min)
    };
    let mut worst = 0.0f64;
    for &x in result.mesh().verts() {
        if old.contains(&x.to_array().map(f64::to_bits)) {
            continue;
        }
        let d = near(a, x).max(near(b, x));
        if d <= reach {
            worst = worst.max(d);
        }
    }
    worst
}

#[test]
fn crossings_the_search_only_placed_are_on_both_surfaces_or_refused() {
    // A cylinder's curved rim edge against a crossing cylinder's wall
    // (found fuzzing related solids, seed 1, case 19): the search stopped
    // at its cap having found nothing, and the crossing the count has was
    // placed where the two came closest, 4 resolutions off the wall.
    // Solving it again on the quadric didn't move it that far, and its
    // band went on a copy of the wall claiming no surface: all four
    // operations were `Ok` with that vertex off. Each must be refused or
    // have every new vertex on both surfaces.
    let a = extruded_on(
        vec![circle(
            DVec2::new(-0.005620956664202037, -0.278018636511416),
            0.7279328535649163,
            0,
            false,
        )],
        Frame {
            origin: DVec3::new(
                -0.9438686345190801,
                -0.6103550207631896,
                -0.47465561761757846,
            ),
            x: DVec3::Z,
            y: DVec3::X,
        },
        -0.2188208636714538,
        0.5634685272668086,
        1,
    );
    let b = extruded_on(
        vec![circle(
            DVec2::new(0.0, 0.2495900097054698),
            0.5442945356289746,
            0,
            false,
        )],
        Frame {
            origin: DVec3::new(
                -1.221887271030496,
                -0.6103550207631896,
                -0.47465561761757846,
            ),
            x: DVec3::X,
            y: DVec3::Y,
        },
        -2.005620956664202,
        1.994379043335798,
        2,
    );
    for (x, y, op) in [
        (&a, &b, Op::Union),
        (&a, &b, Op::Intersection),
        (&a, &b, Op::Difference),
        (&b, &a, Op::Difference),
    ] {
        if let Ok(solid) = boolean(x, y, op, &TOL, &Budget::DEFAULT) {
            let off = off_both(&solid, &a, &b);
            assert!(off <= TOL.resolution(), "{op:?}: a vertex {off:e} off");
        }
    }
}

/// `f` run with the check that crossings the search only placed lie on
/// the surface they cross skipped (`certified` false) or not.
fn certified<T>(certified: bool, f: impl FnOnce() -> T) -> T {
    super::assemble::UNCERTIFIED.set(!certified);
    let out = f();
    super::assemble::UNCERTIFIED.set(false);
    out
}

/// `a ∪ b`, `a ∩ b`, `a − b` and `b − a` at `tol`: each result's volume and
/// how far its claim-free patches are from the surfaces of the faces of
/// `of` they were split off (see [`free_off`]), or the error.
fn four_off(
    a: &Solid,
    b: &Solid,
    of: &[crate::mesh::Face],
    tol: &Tolerance,
) -> [Result<(f64, f64), KernelError>; 4] {
    [
        (a, b, Op::Union),
        (a, b, Op::Intersection),
        (a, b, Op::Difference),
        (b, a, Op::Difference),
    ]
    .map(|(x, y, op)| {
        boolean(x, y, op, tol, &Budget::DEFAULT).map(|s| (s.volume(), free_off(&s, of)))
    })
}

#[test]
fn bands_left_straying_past_the_tolerance_are_refused() {
    // A turned bar through a box (the seeded bars' generator, seed 1, its
    // 55th draw), at the finest tolerance: a crossing the search never
    // found sits 5.6e-4 off the bar's cylinder, and no halving of the cut
    // moves the bands at it closer. When the rounds of halving ran out
    // the result was kept, its union and `bar − box` with bands that far
    // off: 56 times the tolerance. The crossing is now refused as off the
    // surface; past that check, the bands are, as too complex. Each
    // result must be refused or within the tolerance, and past the check
    // at the default tolerance all four work.
    let c = DVec3::new(
        -0.5602419740309785,
        -0.9158090161441121,
        -0.5998650939477481,
    );
    let r = 0.25075971600313085;
    let q = DQuat::from_xyzw(
        0.24549388299162075,
        0.23391712913142826,
        0.06117124477628997,
        0.9387617423633786,
    );
    let min = DVec3::new(
        -0.5613423608645569,
        -1.3805744401035636,
        -1.0752425209742054,
    );
    let size = DVec3::new(2.1318065114018347, 0.8784937534517324, 1.328642506451723);
    for fit in [1e-5, 1e-3] {
        let tol = Tolerance::new(fit).unwrap();
        let bar = Solid::cylinder(DVec3::new(0.0, 0.0, -2.0), r, 4.0, 2, &tol).unwrap();
        let bar = moved_at(&bar, &tol, |p| q * p + c);
        let block = Solid::cuboid(min, size, 1, &tol).unwrap();
        for check in [true, false] {
            let got = certified(check, || four_off(&bar, &block, &walls(&bar), &tol));
            for (k, result) in got.iter().enumerate() {
                match result {
                    Ok((_, off)) => assert!(*off <= fit, "fit {fit}, result {k}: {off:e} off"),
                    Err(e) => assert!(check || fit < 1e-3, "fit {fit}, result {k}: {e:?}"),
                }
            }
            if !check && fit < 1e-3 {
                // The union and `bar − box`, which were kept.
                assert!(matches!(got[0], Err(KernelError::TooComplex)));
                assert!(matches!(got[2], Err(KernelError::TooComplex)));
            }
            if let [Ok((u, _)), Ok((i, _)), Ok((d, _)), Ok((e, _))] = got {
                let (va, vb) = (bar.volume(), block.volume());
                let within = fit * (bar.area() + block.area()) / 100.0;
                assert!((u + i - va - vb).abs() <= within);
                assert!((d - (va - i)).abs() <= within);
                assert!((e - (vb - i)).abs() <= within);
            }
        }
    }
}

/// A 10 × 10 square whose top side, from (10, 10) to (0, 10), is an arc
/// of `deg` degrees bulging out (`convex`) or in, cut into `parts` equal
/// pieces. The arc's ends are level, and so are the middle piece's of an
/// odd count (made by mirroring), so each such wall's corners share a
/// coordinate that its bulge doesn't.
struct Arch {
    deg: f64,
    convex: bool,
    parts: usize,
}

impl Arch {
    /// The arc's centre's height and its radius.
    fn circle(&self) -> (f64, f64) {
        let half = (self.deg / 2.0).to_radians();
        let r = 5.0 / half.sin();
        let h = r * half.cos();
        (if self.convex { 10.0 - h } else { 10.0 + h }, r)
    }

    /// The top side's height at `x` in 0..10.
    fn top(&self, x: f64) -> f64 {
        let (yc, r) = self.circle();
        let s = (r * r - (x - 5.0) * (x - 5.0)).sqrt();
        if self.convex { yc + s } else { yc - s }
    }

    fn profile(&self) -> Loop {
        let (yc, r) = self.circle();
        let center = DVec2::new(5.0, yc);
        // The arc's points from (10, 10), the second half mirrored.
        let n = self.parts;
        let half = (self.deg / 2.0).to_radians();
        let mut points: Vec<DVec2> = (0..=n)
            .map(|i| {
                if i == 0 {
                    return DVec2::new(10.0, 10.0);
                }
                if 2 * i > n {
                    return DVec2::ZERO;
                }
                // Measured from the arc's middle, towards +x.
                let a = half * (1.0 - 2.0 * i as f64 / n as f64);
                let y = if self.convex {
                    yc + r * a.cos()
                } else {
                    yc - r * a.cos()
                };
                DVec2::new(5.0 + r * a.sin(), y)
            })
            .collect();
        for i in 0..=n {
            if 2 * i > n {
                points[i] = DVec2::new(10.0 - points[n - i].x, points[n - i].y);
            }
        }
        let (a0, a1) = (DVec2::ZERO, DVec2::new(10.0, 0.0));
        let mut segments = vec![
            Segment::line(a0, a1, 0).unwrap(),
            Segment::line(a1, points[0], 1).unwrap(),
        ];
        for i in 0..n {
            segments.push(crate::profile::tests::arc(
                center,
                points[i],
                points[i + 1],
                2,
            ));
        }
        segments.push(Segment::line(points[n], a0, 3).unwrap());
        Loop { segments }
    }

    /// Extruded from 0 to 5 on `frame`.
    fn solid(&self, frame: Frame) -> Solid {
        extruded_on(vec![self.profile()], frame, 0.0, 5.0, 9)
    }

    /// The square's area and the arc's segment, exactly.
    fn area(&self) -> f64 {
        let (_, r) = self.circle();
        let t = self.deg.to_radians();
        let segment = r * r / 2.0 * (t - t.sin());
        if self.convex {
            100.0 + segment
        } else {
            100.0 - segment
        }
    }

    /// The area of the profile within `x0..x1` × `y0..y1`, in closed form
    /// between the places the top crosses `y0` and `y1`.
    fn within(&self, [x0, x1]: [f64; 2], [y0, y1]: [f64; 2]) -> f64 {
        let (yc, r) = self.circle();
        let (x0, x1, y0) = (x0.max(0.0), x1.min(10.0), y0.max(0.0));
        if x0 >= x1 || y0 >= y1 {
            return 0.0;
        }
        // ∫ √(r² − u²) du.
        let big = |u: f64| (u * (r * r - u * u).sqrt() + r * r * (u / r).asin()) / 2.0;
        let under = |a: f64, b: f64| {
            let s = big(b - 5.0) - big(a - 5.0);
            yc * (b - a) + if self.convex { s } else { -s }
        };
        let mut cuts = vec![x0, x1];
        for y in [y0, y1] {
            let d = r * r - (y - yc) * (y - yc);
            if d > 0.0 && (y >= yc) == self.convex {
                for x in [5.0 - d.sqrt(), 5.0 + d.sqrt()] {
                    if x0 < x && x < x1 {
                        cuts.push(x);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let top = self.top((a + b) / 2.0);
                if top <= y0 {
                    0.0
                } else if top >= y1 {
                    (y1 - y0) * (b - a)
                } else {
                    under(a, b) - y0 * (b - a)
                }
            })
            .sum()
    }
}

/// The box `x` × `y` × `z` in `frame`'s coordinates (heights along its
/// normal), in the world: square to the axes where the frame is.
fn framed_box(frame: Frame, x: [f64; 2], y: [f64; 2], z: [f64; 2]) -> Solid {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for (i, j, k) in [0, 1].into_iter().flat_map(|i| {
        [0, 1]
            .into_iter()
            .flat_map(move |j| [0, 1].into_iter().map(move |k| (i, j, k)))
    }) {
        let p = frame.point(DVec2::new(x[i], y[j]), z[k]);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    Solid::cuboid(lo, hi - lo, 1, &TOL).unwrap()
}

/// The arch on `frame` less, with and within the box `x` × `y` × `z` (in
/// the frame's coordinates): the volume `A ∩ B` should have.
fn arch_and_box(arch: &Arch, x: [f64; 2], y: [f64; 2], z: [f64; 2]) -> f64 {
    let height = (z[1].min(5.0) - z[0].max(0.0)).max(0.0);
    arch.within(x, y) * height
}

/// Frames square to the axes on which a level side is level in `y`, `z`
/// and `x` of the world.
const LEVEL_FRAMES: [Frame; 3] = [
    Frame::XY,
    Frame {
        origin: DVec3::ZERO,
        x: DVec3::X,
        y: DVec3::Z,
    },
    Frame {
        origin: DVec3::ZERO,
        x: DVec3::Y,
        y: DVec3::X,
    },
];

#[test]
fn a_box_across_a_wall_with_level_ends() {
    // A wall piece whose ends are level has three corners sharing a
    // coordinate its bulge doesn't: crossings on it stay on the
    // cylinder rather than being put on its corners' plane.
    let arches = [
        Arch {
            deg: 60.0,
            convex: false,
            parts: 1,
        },
        Arch {
            deg: 60.0,
            convex: true,
            parts: 1,
        },
        Arch {
            deg: 20.0,
            convex: true,
            parts: 1,
        },
        Arch {
            deg: 20.0,
            convex: false,
            parts: 3,
        },
    ];
    for (f, frame) in LEVEL_FRAMES.into_iter().enumerate() {
        for arch in &arches {
            let a = arch.solid(frame);
            let va = arch.area() * 5.0;
            assert!((a.volume() - va).abs() <= 1e-9, "{}", a.volume() - va);
            let mid = arch.top(5.0);
            let tools = [
                // A box across the arc's middle (for the 60° concave arc,
                // near the one it was found with, (4, 8, −1) + (2, 2, 7)).
                ([4.0, 6.0], [mid - 1.0, mid + 1.0], [-1.0, 6.0]),
                // A half-space whose face x = 4 crosses the wall.
                ([4.0, 14.0], [-1.0, 14.0], [-1.0, 6.0]),
            ];
            let found = ([4.0, 6.0], [8.0, 10.0], [-1.0, 6.0]);
            let found = (!arch.convex).then_some(found);
            for (x, y, z) in tools.into_iter().chain(found) {
                let name = format!(
                    "frame {f}, {}° {} in {}, box {x:?} {y:?}",
                    arch.deg,
                    if arch.convex { "convex" } else { "concave" },
                    arch.parts
                );
                let b = framed_box(frame, x, y, z);
                let results = all_four(&a, &b, 1e-9);
                volumes(&name, &a, &b, &results, arch_and_box(arch, x, y, z), 1e-9);
                for solid in &results {
                    exact_to(&name, solid, 10.0, 1e-10);
                }
            }
        }
    }
}

#[test]
fn random_boxes_across_walls_with_level_ends() {
    // Boxes of random sizes across 20°, 45° and 60° walls with level
    // ends, through them or from the top: each result right by its
    // volume in closed form, or refused.
    let mut rng = crate::test_rng::Rng::new(60);
    let (mut done, mut all) = (0, 0);
    for i in 0..40 {
        let arch = Arch {
            deg: [20.0, 45.0, 60.0][i % 3],
            convex: rng.unit() < 0.5,
            parts: 1,
        };
        let frame = LEVEL_FRAMES[i % 3];
        let a = arch.solid(frame);
        let va = arch.area() * 5.0;
        let t = rng.range(0.5, 9.5);
        let size = rng.range(0.3, 3.0);
        let off = DVec2::new(rng.range(-0.8, 0.8), rng.range(-0.8, 0.8)) * size;
        let c = DVec2::new(t, arch.top(t)) + off;
        let x = [c.x - size / 2.0, c.x + size / 2.0];
        let y = [c.y - size / 2.0, c.y + size / 2.0];
        let z = if rng.unit() < 0.5 {
            [-1.0, 6.0]
        } else {
            [2.0, 7.0]
        };
        let b = framed_box(frame, x, y, z);
        let vb = b.volume();
        let both = arch_and_box(&arch, x, y, z);
        for (op, want) in [
            (Op::Union, va + vb - both),
            (Op::Intersection, both),
            (Op::Difference, va - both),
        ] {
            all += 1;
            match boolean(&a, &b, op, &TOL, &Budget::DEFAULT) {
                Ok(solid) => {
                    let got = solid.volume();
                    assert!(
                        (got - want).abs() <= 1e-8,
                        "case {i}, {op:?}: volume {got}, not {want}"
                    );
                    done += 1;
                }
                Err(KernelError::Invalid(_) | KernelError::TooComplex) => {}
                Err(e) => panic!("case {i}, {op:?}: {e:?}"),
            }
        }
    }
    // Most go through (a third did when crossings were put on the walls'
    // corners' planes). The rest are unions and differences with convex
    // walls whose cap pieces fold.
    assert!(done * 100 >= all * 85, "{done} of {all}");
}

/// The circle of radius 0.5 round (0.5, 0.2) in six arcs from `+x`, its
/// points from exact constants: the arcs from 60° to 120° and from 240°
/// to 300° have level ends.
fn six_arcs() -> Loop {
    let center = DVec2::new(0.5, 0.2);
    let s = 0.5 * 0.75f64.sqrt();
    let points = [
        DVec2::new(1.0, 0.2),
        DVec2::new(0.75, 0.2 + s),
        DVec2::new(0.25, 0.2 + s),
        DVec2::new(0.0, 0.2),
        DVec2::new(0.25, 0.2 - s),
        DVec2::new(0.75, 0.2 - s),
    ];
    Loop {
        segments: (0..6)
            .map(|i| crate::profile::tests::arc(center, points[i], points[(i + 1) % 6], 0))
            .collect(),
    }
}

#[test]
fn a_wall_with_level_corners_keeps_its_crossings() {
    // A circle of six arcs, two of whose walls have corners at one `y`,
    // against a coaxial cylinder of four and a slab.
    let lp = six_arcs();
    let level = lp.segments[1].conic;
    assert_eq!(level.p0.y, level.p1.y);
    let big = extruded(
        vec![circle(DVec2::new(0.5, 0.2), 1.0, 0, false)],
        0.0,
        1.0,
        7,
    );
    let quarter = PI / 4.0;
    let slab = cube([-2.0, -2.0, 0.5], [4.0, 4.0, 0.5]);
    // Each job's other operand and its volume, the six-arc circle's
    // heights and what the two share, all in closed form.
    let jobs = [
        ("over 0..2", big.clone(), PI, 0.0, 2.0, quarter),
        ("over 1..2", big, PI, 1.0, 2.0, 0.0),
        ("a slab", slab, 8.0, 0.0, 2.0, quarter / 2.0),
    ];
    for (name, a, va, from, to, both) in jobs {
        let b = extruded(vec![lp.clone()], from, to, 8);
        let vb = quarter * (to - from);
        assert!((a.volume() - va).abs() <= 1e-12, "{name}: {}", a.volume());
        assert!((b.volume() - vb).abs() <= 1e-12, "{name}: {}", b.volume());
        let results = all_four(&a, &b, 1e-9);
        volumes(name, &a, &b, &results, both, 1e-9);
    }
}

#[test]
fn shallow_level_arcs_are_never_wrong() {
    // Crossings put on the corners' plane of a wall this shallow cut it
    // by a band on a copy of its face that no tag check sees: the union
    // and difference came back with the wrong volume. Every result must
    // be right by its volume in closed form, or refused.
    let mut done = 0;
    for deg in [0.5, 2.0] {
        let arch = Arch {
            deg,
            convex: true,
            parts: 1,
        };
        let a = arch.solid(Frame::XY);
        let va = arch.area() * 5.0;
        assert!(
            (a.volume() - va).abs() <= 1e-9,
            "{deg}°: {}",
            a.volume() - va
        );
        for (x0, y0) in [(8.3, 7.77), (-1.0, 8.54)] {
            let (x, y, z) = ([x0, x0 + 1.7], [y0, y0 + 2.3], [-0.7, 3.3]);
            let b = framed_box(Frame::XY, x, y, z);
            let (vb, both) = (b.volume(), arch_and_box(&arch, x, y, z));
            for (op, want) in [
                (Op::Union, va + vb - both),
                (Op::Intersection, both),
                (Op::Difference, va - both),
            ] {
                match boolean(&a, &b, op, &TOL, &Budget::DEFAULT) {
                    Ok(solid) => {
                        let got = solid.volume();
                        assert!(
                            (got - want).abs() <= 1e-9,
                            "{deg}°, box at ({x0}, {y0}), {op:?}: volume {got}, not {want}"
                        );
                    }
                    Err(KernelError::Invalid(_) | KernelError::TooComplex) => continue,
                    Err(e) => panic!("{deg}°, {op:?}: {e:?}"),
                }
                done += 1;
            }
        }
    }
    // All twelve go through today.
    assert!(done >= 9, "{done}");
}

#[test]
fn band_roots_off_their_wall_are_bounded() {
    // A small cylinder across a 75° concave wall in three pieces (found
    // fuzzing): a crossing the search only placed sits 1.3e-4 off the
    // small cylinder's wall, and the triangles at it, some along no cut
    // (band trees' roots no ruling frees), went on a copy of the wall
    // claiming no surface, that far off at every tolerance: the union
    // was kept at 1.3 times the tolerance of 1e-4. The crossing is now
    // refused as off the surface; past that check, the triangles are,
    // as too complex. Each result must be refused or within the
    // tolerance of the walls, and past the check at the default
    // tolerance all four work, their volumes right.
    let arch = Arch {
        deg: 75.35906468803832,
        convex: false,
        parts: 3,
    };
    let frame = Frame {
        origin: DVec3::ZERO,
        x: DVec3::NEG_X,
        y: DVec3::Y,
    };
    let c = DVec2::new(9.226584160671807, 9.44224065070859);
    let tool = circle(c, 9.335067680464189 - c.x, 30, false);
    for fit in [1e-5, 1e-4, 1e-3] {
        let tol = Tolerance::new(fit).unwrap();
        let build = |lp: &Loop, from: f64, to: f64, feature: u64| {
            let profile = Profile {
                loops: vec![lp.clone()],
            };
            extrude(&profile, &frame, from, to, feature, &tol, &Budget::DEFAULT).unwrap()
        };
        let a = build(&arch.profile(), 0.0, 5.0, 9);
        let b = build(&tool, 2.0, 7.0, 30);
        let wall = [walls(&a), walls(&b)].concat();
        for check in [true, false] {
            let got = certified(check, || four_off(&a, &b, &wall, &tol));
            for (k, result) in got.iter().enumerate() {
                match result {
                    Ok((_, off)) => assert!(*off <= fit, "fit {fit}, result {k}: {off:e} off"),
                    Err(e) => assert!(check || fit < 1e-3, "fit {fit}, result {k}: {e:?}"),
                }
            }
            if !check && fit < 1e-3 {
                assert!(matches!(got[0], Err(KernelError::TooComplex)));
            }
            if let [Ok((u, _)), Ok((i, _)), Ok((d, _)), Ok((e, _))] = got {
                let (va, vb) = (a.volume(), b.volume());
                let within = fit * (a.area() + b.area()) / 100.0;
                assert!((u + i - va - vb).abs() <= within);
                assert!((d - (va - i)).abs() <= within);
                assert!((e - (vb - i)).abs() <= within);
            }
        }
    }
}

/// A 10 × 10 square whose top side, from (10, 10) to (0, 10), is the
/// parabola (a spline's piece: weight 1) with its control point at
/// (5, 10 − 2·`depth`): `y = 10 − 0.04·depth·x·(10 − x)`.
fn parabolic_arch(depth: f64) -> Loop {
    let (a0, a1) = (DVec2::ZERO, DVec2::new(10.0, 0.0));
    let (top0, top1) = (DVec2::new(10.0, 10.0), DVec2::new(0.0, 10.0));
    let top = crate::patch::Conic2::new(top0, DVec2::new(5.0, 10.0 - 2.0 * depth), 1.0, top1);
    Loop {
        segments: vec![
            Segment::line(a0, a1, 0).unwrap(),
            Segment::line(a1, top0, 1).unwrap(),
            Segment {
                conic: top.unwrap(),
                curve: 2,
            },
            Segment::line(top1, a0, 3).unwrap(),
        ],
    }
}

#[test]
fn boxes_across_parabolic_walls_are_right() {
    // The patches of a wall over a parabola have their curves' planes
    // meet at infinity, along its axis. Bands along a cut took their
    // inner edges through a point found as the far root of a quadratic
    // whose leading term was all rounding, off in a random direction, and
    // came out up to 1e-3 off the wall on a copy of its face claiming no
    // surface: unions and differences 3.6e-3 off in volume.
    let depth = 3.0;
    let lp = parabolic_arch(depth);
    let top = |x: f64| 10.0 - 0.04 * depth * x * (10.0 - x);
    // ∫ top from `x0` to `x1`.
    let under = |x0: f64, x1: f64| {
        10.0 * (x1 - x0)
            - 0.04 * depth * (5.0 * (x1 * x1 - x0 * x0) - (x1.powi(3) - x0.powi(3)) / 3.0)
    };
    let frames = [
        Frame::XY,
        Frame {
            origin: DVec3::new(-12.25, -0.5, -8.0),
            x: DVec3::NEG_Z,
            y: DVec3::NEG_Y,
        },
        Frame {
            origin: DVec3::new(0.1, 0.2, 0.3),
            x: DVec3::X,
            y: DVec3::new(0.0, 0.6, 0.8),
        },
    ];
    let va = 5.0 * under(0.0, 10.0);
    let (mut done, mut all) = (0, 0);
    for (f, frame) in frames.into_iter().enumerate() {
        let a = extruded_on(vec![lp.clone()], frame, 0.0, 5.0, 9);
        assert!((a.volume() - va).abs() <= 1e-9, "{}", a.volume() - va);
        for (x0, width, z) in [
            (8.41, 0.06, [1.0, 4.0]),
            (6.2, 0.47, [-1.0, 6.0]),
            (1.3, 0.2, [2.0, 7.0]),
            (4.9, 0.3, [1.0, 4.0]),
        ] {
            let x1 = x0 + width;
            // Across the wall: from under its lowest point there to over
            // its highest.
            let (lo, hi) = (
                top(x0).min(top(x1)).min(top(5.0f64.clamp(x0, x1))),
                top(x0).max(top(x1)),
            );
            let y = [lo - 0.02, hi + 0.02];
            let b = extruded_on(
                vec![rect(DVec2::new(x0, y[0]), DVec2::new(x1, y[1]), 30)],
                frame,
                z[0],
                z[1],
                30,
            );
            let height = z[1].min(5.0) - z[0].max(0.0);
            let both = (under(x0, x1) - y[0] * width) * height;
            if f == 2 && x0 == 8.41 {
                // The same bits at 1 and 8 threads.
                let mesh = assert_deterministic(|| {
                    boolean(&a, &b, Op::Difference, &TOL, &Budget::DEFAULT).map(|s| s.into_mesh())
                });
                assert!(mesh.is_ok());
            }
            let vb = b.volume();
            for (x, y, op, want) in [
                (&a, &b, Op::Union, va + vb - both),
                (&a, &b, Op::Intersection, both),
                (&a, &b, Op::Difference, va - both),
                (&b, &a, Op::Difference, vb - both),
            ] {
                all += 1;
                match boolean(x, y, op, &TOL, &Budget::DEFAULT) {
                    Ok(solid) => {
                        let got = solid.volume();
                        assert!(
                            (got - want).abs() <= 1e-8,
                            "frame {f}, box at {x0}, {op:?}: volume {got}, not {want}"
                        );
                        done += 1;
                    }
                    Err(KernelError::Invalid(_) | KernelError::TooComplex) => {
                        println!("frame {f}, box at {x0}, {op:?} refused");
                    }
                    Err(e) => panic!("frame {f}, box at {x0}, {op:?}: {e:?}"),
                }
            }
        }
    }
    assert!(done * 10 >= all * 9, "{done} of {all}");
}

#[test]
fn cuts_across_nearly_straight_edges_of_results() {
    // A box cut from a wall over a very shallow hyperbola (weight 2.13)
    // leaves a cut on its cap within the resolution of straight, whose
    // control point is far from its chord's middle. Crossings on such an
    // edge were found along the segment and put at the conic's point of
    // the segment's parameter, 2e-3 away along it: off the plane crossed
    // (moved back only on planes square to an axis, past the snap's
    // bound) and, on a tilted frame, the union 1.8e-4 off in volume.
    let v = DVec2::new;
    let top = crate::patch::Conic2::new(
        v(10.0, 10.0),
        v(4.378525581743227, 9.999726510682816),
        2.1312268667924372,
        v(0.0, 10.0),
    )
    .unwrap();
    let lp = Loop {
        segments: vec![
            Segment::line(v(0.0, 0.0), v(10.0, 0.0), 0).unwrap(),
            Segment::line(v(10.0, 0.0), v(10.0, 10.0), 1).unwrap(),
            Segment {
                conic: top,
                curve: 2,
            },
            Segment::line(v(0.0, 10.0), v(0.0, 0.0), 3).unwrap(),
        ],
    };
    let first = rect(
        v(6.154462296097111, 9.973702360893435),
        v(6.486625153233848, 10.445907560013437),
        30,
    );
    let second = rect(
        v(6.190644268668397, 9.960157904094217),
        v(6.272710048175984, 10.086441493557576),
        40,
    );
    let q = DQuat::from_axis_angle(DVec3::new(0.3, -0.5, 0.8).normalize(), 0.7);
    for frame in [
        Frame {
            origin: DVec3::new(7.5, 13.0, -10.75),
            x: DVec3::X,
            y: DVec3::NEG_Z,
        },
        Frame {
            origin: DVec3::new(0.5, -1.0, 2.0),
            x: q * DVec3::X,
            y: q * DVec3::Y,
        },
    ] {
        let a = extruded_on(vec![lp.clone()], frame, 0.0, 5.0, 9);
        let b = extruded_on(vec![first.clone()], frame, 1.0, 4.0, 30);
        let c = extruded_on(vec![second.clone()], frame, -1.0, 3.0, 40);
        let piece = run(&a, &b, Op::Intersection);
        let [u, i, d] =
            [Op::Union, Op::Intersection, Op::Difference].map(|op| run(&piece, &c, op).volume());
        let (vp, vc) = (piece.volume(), c.volume());
        // The piece's volume: the box's, less what lies over the top.
        assert!((vp - 0.026049739363).abs() <= 1e-10, "{vp}");
        assert!((u + i - vp - vc).abs() <= 1e-7, "{frame:?}: {u} + {i}");
        assert!((d + i - vp).abs() <= 1e-7, "{frame:?}: {d} + {i}");
        assert!((i - 0.004290201848).abs() <= 1e-7, "{frame:?}: {i}");
    }
}
