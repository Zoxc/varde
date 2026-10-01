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
use crate::profile::tests::{arc, circle, polygon, rect, reversed};
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
        builder.face(crate::mesh::Face {
            surface,
            form: face.form.moved(&f),
            ..face
        });
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
    for [(x1, y1, r1), (x2, y2, r2), (bx, by, br)] in cases {
        let plate = run(&slab, &drill(x1, y1, r1), Op::Difference);
        let plate = run(&plate, &drill(x2, y2, r2), Op::Difference);
        let boss = cylinder([bx, by, 1.0], br, 1.0);
        // One of them has a crossing of a cap edge through the hole's
        // wall at its rim that the search only placed, 7.6e-5 off the
        // wall, which goes to the edge's root on the wall.
        let joined = run(&plate, &boss, Op::Union);
        // Where the boss covers a hole, its cap and the hole's wall meet
        // flush along arcs of both rims, some of it fitted.
        let want = 24.0 - PI * (r1 * r1 + r2 * r2) + PI * br * br;
        assert!(
            (joined.volume() - want).abs() < 1e-6,
            "boss at ({bx}, {by}): {} not {want}",
            joined.volume()
        );
    }
}

/// The area two discs (centre, radius) share.
fn lens(a: (DVec2, f64), b: (DVec2, f64)) -> f64 {
    let d = a.0.distance(b.0);
    let (r1, r2) = (a.1, b.1);
    if d >= r1 + r2 {
        return 0.0;
    }
    if d <= (r1 - r2).abs() {
        return PI * r1.min(r2).powi(2);
    }
    r1 * r1 * ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1)).acos()
        + r2 * r2 * ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2)).acos()
        - 0.5 * ((-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2)).sqrt()
}

#[test]
fn bosses_sunk_through_drilled_plates_at_their_middle() {
    // A plate 1 thick drilled twice, and a boss from its bottom up 2 (or
    // from -1 up 4): near the holes refinement splits the boss's wall at
    // the middle of its rulings, which is the plate's top, and joins
    // those points with curves of the wall that bulge out of the top's
    // plane. The top then cuts the wall from end to end of such a curve,
    // beside it. The band between the two was fanned from its ends into
    // slivers with three corners on the top's circle (`Invalid`); where
    // the curve dips into the plate, the curve's crossings at its ends,
    // which the count can't tell from none, were dropped by rounding, and
    // the cut went into the triangle below it. The first six failed so.
    let slab = cube([-3.0, -2.0, 0.0], [6.0, 4.0, 1.0]);
    let drill = |(x, y, r): (f64, f64, f64), feature| {
        Solid::cylinder(DVec3::new(x, y, -1.0), r, 4.0, feature, &TOL).unwrap()
    };
    let cases = [
        (
            [(-1.4, -1.05, 0.4), (1.8, 1.0, 0.4)],
            (-0.2, 0.3, 1.1),
            0.0,
            2.0,
        ),
        (
            [(-1.4, -1.05, 0.4), (1.8, 1.0, 0.4)],
            (-0.2, 0.3, 1.1),
            -1.0,
            4.0,
        ),
        (
            [(-2.0, 0.7, 0.6), (1.9, -1.0, 0.3)],
            (-0.65, -0.5, 1.1),
            0.0,
            2.0,
        ),
        // Holding a hole.
        (
            [(-0.7, -0.45, 0.35), (0.5, 0.7, 0.45)],
            (-0.55, -0.35, 0.75),
            0.0,
            2.0,
        ),
        // Holding a hole, and a curve dipping into the plate.
        (
            [(-0.5, 0.7, 0.65), (1.5, 0.4, 0.35)],
            (-0.25, 0.7, 1.15),
            0.0,
            2.0,
        ),
        // Crossing a hole, and a curve dipping into the plate.
        (
            [(-1.75, -0.55, 0.65), (1.0, 0.6, 0.55)],
            (0.65, 0.2, 0.25),
            0.0,
            2.0,
        ),
        // Crossing a hole: the curve's vertex across from the cut's end
        // at the hole's wall would have come 3e-7 from the walls' crossing
        // on it (worked before; failed with every vertex across kept).
        (
            [(-0.8, 1.05, 0.5), (1.95, -0.75, 0.45)],
            (-0.9, 0.25, 0.7),
            0.0,
            2.0,
        ),
    ];
    for (holes, (bx, by, br), z0, h) in cases {
        let plate = run(&slab, &drill(holes[0], 2), Op::Difference);
        let plate = run(&plate, &drill(holes[1], 3), Op::Difference);
        let boss = Solid::cylinder(DVec3::new(bx, by, z0), br, h, 4, &TOL).unwrap();
        let disc = |(x, y, r): (f64, f64, f64)| (DVec2::new(x, y), r);
        let (vp, vb) = (plate.volume(), boss.volume());
        let shared = PI * br * br
            - holes
                .iter()
                .map(|&o| lens(disc(o), disc((bx, by, br))))
                .sum::<f64>();
        let both = shared * ((z0 + h).min(1.0) - z0.max(0.0));
        let want = [vp + vb - both, both, vp - both, vb - both];
        let jobs = [
            (&plate, &boss, Op::Union),
            (&plate, &boss, Op::Intersection),
            (&plate, &boss, Op::Difference),
            (&boss, &plate, Op::Difference),
        ];
        for ((x, y, op), want) in jobs.into_iter().zip(want) {
            let got = run(x, y, op).volume();
            assert!(
                (got - want).abs() < 1e-6,
                "boss at ({bx}, {by}) from {z0}, {op:?}: {got} not {want}"
            );
        }
    }
}

#[test]
fn a_boss_sunk_through_a_drilled_plate_is_the_same_on_any_thread_count() {
    let slab = cube([-3.0, -2.0, 0.0], [6.0, 4.0, 1.0]);
    let drill = |x: f64, y: f64, r: f64, feature| {
        Solid::cylinder(DVec3::new(x, y, -1.0), r, 4.0, feature, &TOL).unwrap()
    };
    let plate = run(&slab, &drill(-1.4, -1.05, 0.4, 2), Op::Difference);
    let plate = run(&plate, &drill(1.8, 1.0, 0.4, 3), Op::Difference);
    let boss = Solid::cylinder(DVec3::new(-0.2, 0.3, 0.0), 1.1, 2.0, 4, &TOL).unwrap();
    for (x, y, op) in [(&plate, &boss, Op::Union), (&boss, &plate, Op::Difference)] {
        let _ = assert_deterministic(|| {
            boolean(x, y, op, &TOL, &Budget::DEFAULT).map(Solid::into_mesh)
        });
    }
}

#[test]
fn a_hole_through_a_sunk_ring_on_a_turned_frame() {
    // Made as the app makes it, on a turned frame: a plate 1 thick with
    // two holes, a ring joined from -1 to 3 (its wall split at the
    // plate's faces), a slot cut from -1 to 1, then a hole from -1 to 1
    // through the plate and the ring's wall below it. The result came out
    // 1.8e-6 too big, a sliver left where the ring's wall meets the
    // plate's bottom, until crossings at an edge's end in a plane went by
    // the perturbation.
    let turn = DQuat::from_rotation_x(0.37) * DQuat::from_rotation_y(-0.61);
    let frame = Frame {
        origin: DVec3::new(0.3, -0.2, 0.7),
        x: turn * DVec3::X,
        y: turn * DVec3::Y,
    };
    let plate = extruded_on(
        vec![
            rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0),
            circle(DVec2::new(1.7, 0.95), 0.35, 10, true),
            circle(DVec2::new(-1.55, 1.1), 0.65, 20, true),
        ],
        frame,
        0.0,
        1.0,
        1,
    );
    let (c, o) = (DVec2::new(0.1, 0.15), DVec2::new(0.15, -0.05));
    let ring = extruded_on(
        vec![circle(c, 1.15, 0, false), circle(c + o, 0.4, 10, true)],
        frame,
        -1.0,
        3.0,
        10,
    );
    let body = run(&plate, &ring, Op::Union);
    let (c, half, r) = (DVec2::new(-0.45, 1.0), 0.9, 0.55);
    let p = |x: f64, y: f64| c + DVec2::new(x, y);
    let slot = Loop {
        segments: vec![
            Segment::line(p(-half, -r), p(half, -r), 0).unwrap(),
            arc(p(half, 0.0), p(half, -r), p(half + r, 0.0), 1),
            arc(p(half, 0.0), p(half + r, 0.0), p(half, r), 1),
            Segment::line(p(half, r), p(-half, r), 2).unwrap(),
            arc(p(-half, 0.0), p(-half, r), p(-half - r, 0.0), 3),
            arc(p(-half, 0.0), p(-half - r, 0.0), p(-half, -r), 3),
        ],
    };
    let body = run(
        &body,
        &extruded_on(vec![slot], frame, -1.0, 1.0, 12),
        Op::Difference,
    );
    let hole = (DVec2::new(0.05, -0.95), 0.55);
    let drill = extruded_on(vec![circle(hole.0, hole.1, 0, false)], frame, -1.0, 1.0, 13);
    // The hole takes the plate's full disc (clear of its holes and the
    // slot) and, below it, where it overlaps the ring's outer disc (clear
    // of its hole).
    let taken = PI * hole.1 * hole.1 + lens(hole, (DVec2::new(0.1, 0.15), 1.15));
    let got = run(&body, &drill, Op::Difference).volume();
    let want = body.volume() - taken;
    assert!((got - want).abs() < 1e-9, "{got} not {want}");
}

/// The 60 × 40 plate with a hole of radius 8 in its middle, extruded
/// from 0 to `h`, its hole's loop as regeneration builds it: clockwise
/// from (8, 0), in four quarters.
fn tall_plate(h: f64) -> Solid {
    let ends = [DVec2::X, DVec2::NEG_Y, DVec2::NEG_X, DVec2::Y, DVec2::X].map(|d| d * 8.0);
    let hole = Loop {
        segments: (0..4)
            .map(|k| Segment {
                conic: crate::patch::Conic2::arc_between(DVec2::ZERO, 8.0, ends[k], ends[k + 1])
                    .unwrap(),
                curve: 5,
            })
            .collect(),
    };
    extruded(
        vec![
            rect(DVec2::new(-30.0, -20.0), DVec2::new(30.0, 20.0), 1),
            hole,
        ],
        0.0,
        h,
        1,
    )
}

#[test]
fn drilling_a_tall_plate() {
    // A drill of radius 3 through the plate, 10 longer than it: the
    // cap's long edges cross the drill's wall, two patches per quarter
    // that tall and 4.2 wide, and the search for the crossings ran out
    // of pieces splitting the wall across its width, from about 100 tall.
    // Each of these failed so (`Invalid`, the clean-up left with folds).
    let drills = [
        DVec2::new(-20.0, 10.0),
        DVec2::new(-23.15, 10.0),
        DVec2::new(-17.27, 5.21),
        DVec2::new(13.61, 1.26),
    ];
    for h in [100.0, 300.0, 1000.0, 10000.0] {
        let plate = tall_plate(h);
        let want = (2400.0 - 64.0 * PI - 9.0 * PI) * h;
        for d in drills {
            let drill = cylinder([d.x, d.y, -5.0], 3.0, h + 10.0);
            let drilled = boolean(&plate, &drill, Op::Difference, &TOL, &Budget::DEFAULT)
                .unwrap_or_else(|e| panic!("{h} tall, drilled at {d}: {e:?}"));
            let got = drilled.volume();
            assert!(
                (got - want).abs() <= 1e-9 * want,
                "{h} tall, drilled at {d}: volume {got}, not {want}"
            );
        }
    }
    // A plain box drilled where its cap's diagonal crosses the drill.
    for h in [300.0, 1000.0] {
        let slab = cube([-30.0, -20.0, 0.0], [60.0, 40.0, h]);
        let drill = cylinder([20.0, 13.0, -5.0], 3.0, h + 10.0);
        let drilled = run(&slab, &drill, Op::Difference);
        let want = (2400.0 - 9.0 * PI) * h;
        assert!(
            (drilled.volume() - want).abs() <= 1e-9 * want,
            "{h} tall box"
        );
    }
}

#[test]
fn pins_across_a_tall_plates_hole() {
    // A pin of radius 1 standing on the rim of the tall plate's hole: the
    // rim's quarter arcs at the caps cross the pin's wall, 1 010 tall and
    // under 2 wide, and the arcs' control hulls hold its width there.
    // The plate less the pin, and the two joined, to the closed-form
    // lens of the two circles.
    let h = 1000.0;
    let plate = tall_plate(h);
    let lens = {
        // Circles of radius 8 and 1 with centres 8 apart.
        let (r, s, d): (f64, f64, f64) = (8.0, 1.0, 8.0);
        let a = ((d * d + r * r - s * s) / (2.0 * d * r)).acos();
        let b = ((d * d + s * s - r * r) / (2.0 * d * s)).acos();
        r * r * (a - a.sin() * a.cos()) + s * s * (b - b.sin() * b.cos())
    };
    let pinned = PI - lens;
    for angle in [0.3f64, 2.0, 4.0] {
        let centre = DVec2::new(angle.cos(), angle.sin()) * 8.0;
        let pin = cylinder([centre.x, centre.y, -5.0], 1.0, h + 10.0);
        let less = run(&plate, &pin, Op::Difference);
        let want = (2400.0 - 64.0 * PI - pinned) * h;
        assert!(
            (less.volume() - want).abs() <= 1e-9 * want,
            "at {angle}: volume {}, not {want}",
            less.volume()
        );
        let joined = run(&plate, &pin, Op::Union);
        let want = (2400.0 - 64.0 * PI + lens) * h + PI * 10.0;
        assert!(
            (joined.volume() - want).abs() <= 1e-9 * want,
            "at {angle}: volume {}, not {want}",
            joined.volume()
        );
    }
}

#[test]
fn the_result_checks_integrations_are_charged() {
    // A tube whose wall is a twentieth thick, notched through the wall:
    // the corner triangles' volume can't tell which way the result
    // faces, so its check integrates some of the patches. The
    // operation's work is what making the mesh (and naming its faces)
    // takes, a few units a patch for the check, and `INTEGRATE_WORK` for
    // each patch it integrated, charged after: the operation fits that
    // budget exactly.
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
    let mesh = mesh
        .repair_within(&TOL, &mut work)
        .unwrap()
        .merge_faces(TOL.resolution(), &mut work)
        .unwrap();
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

/// The circle round `c` of radius `r` in `n` arcs, counter-clockwise from
/// the angle `turn`: one circle with its vertices somewhere else.
fn turned_circle(c: DVec2, r: f64, curve: u64, turn: f64, n: usize) -> Loop {
    let at = |i: usize| {
        let a = turn + 2.0 * PI * i as f64 / n as f64;
        c + DVec2::new(a.cos(), a.sin()) * r
    };
    Loop {
        segments: (0..n)
            .map(|i| arc(c, at(i), at((i + 1) % n), curve))
            .collect(),
    }
}

/// `a ∪ b`, `a ∩ b`, `a − b` and `b − a` of two solids on one circular
/// cylinder of radius 1, whose spans along it meet over `both`: each
/// checked against the volume π times its span, and the unions (both
/// orders) of no more than `patches` patches.
fn coaxial(name: &str, a: &Solid, b: &Solid, both: f64, patches: usize) {
    let results = all_four(a, b, 1e-12);
    volumes(name, a, b, &results, PI * both, 1e-9);
    let other = run(b, a, Op::Union);
    assert!(
        (other.volume() - results[0].volume()).abs() <= 1e-9,
        "{name}: {} {}",
        other.volume(),
        results[0].volume()
    );
    for union in [&results[0], &other] {
        let n = union.mesh().tris().len();
        assert!(n <= patches, "{name}: the union has {n} patches");
        // One surface, one face.
        assert_eq!(quadric_faces(union), 1, "{name}");
    }
}

#[test]
fn coaxial_cylinders_stacked_or_overlapping_unite() {
    // Two cylinders of one radius on one axis, one standing on the other
    // or running past its cap: the union's wall has a seam, where the
    // first's cap rim lies on the second's wall. Grown for the union, the
    // first left a ring of its cap of zero width between its rim and the
    // second's wall cut at the cap's plane, whose vertices aren't the
    // rim's; the rim takes the cut's vertices, as a boss's rim on a
    // plate's flush cap does. The second circle is drawn as the first,
    // and from another start in 3 arcs.
    let c = DVec2::new(0.5, 0.2);
    let a = extruded(vec![circle(c, 1.0, 0, false)], 0.0, 1.0, 7);
    let circles = [
        ("same", circle(c, 1.0, 0, false)),
        ("turned", turned_circle(c, 1.0, 0, 0.3, 3)),
    ];
    for (what, lp) in &circles {
        for (from, to) in [(1.0, 2.0), (0.5, 2.0), (0.0, 2.0), (-0.5, 0.5)] {
            let b = extruded(vec![lp.clone()], from, to, 8);
            let both = (to.min(1.0) - from.max(0.0)).max(0.0);
            coaxial(&format!("{what} {from}..{to}"), &a, &b, both, 64);
        }
    }
}

#[test]
fn a_cylinder_inside_one_of_its_radius_flush_at_one_end() {
    // The second in 3 arcs from the first's start: a tie leaves two
    // vertices a rounding apart (the first's cap edge crossing at its rim
    // vertex, the second's wall diagonal crossing the cap's plane), and a
    // triangle of zero width between them whose sides from the far corner
    // were two conics on the wall, which the clean-up can't merge. The
    // inner one takes the boundary edge's curve, as at ties to the bit.
    let c = DVec2::new(0.5, 0.2);
    let a = extruded(vec![circle(c, 1.0, 0, false)], 0.0, 1.0, 7);
    let b = extruded(vec![turned_circle(c, 1.0, 0, 0.0, 3)], 0.0, 2.0, 8);
    coaxial("inside", &a, &b, 1.0, 64);
    let union = run(&a, &b, Op::Union);
    assert!((union.volume() - 2.0 * PI).abs() <= 1e-9);
}

/// The XZ and YZ sketch planes, whose caps lie nearly along `UP`.
const SIDE_PLANES: [(&str, Frame); 2] = [
    (
        "XZ",
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Z,
        },
    ),
    (
        "YZ",
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::Y,
            y: DVec3::Z,
        },
    ),
];

#[test]
fn coaxial_cylinders_on_the_side_planes() {
    // On the XZ and YZ planes a cap's plane lies nearly along `UP`, so a
    // rim's shadow is a thin ellipse. Where both operands have the same
    // arc of it (a rim, or a wall's diagonal), the perturbed shadows
    // cross round the fold once or twice, `A` above at one crossing and
    // below at another; split by one sample's height, the crossings
    // gave the two walls on one cylinder ends, and every operation
    // failed as `Inconsistent` (YZ all, XZ those in 3 arcs).
    let c = DVec2::new(0.5, 0.2);
    for (plane, frame) in SIDE_PLANES {
        let a = extruded_on(vec![circle(c, 1.0, 0, false)], frame, 0.0, 1.0, 7);
        let circles = [
            ("same", circle(c, 1.0, 0, false)),
            ("turned", turned_circle(c, 1.0, 0, 0.7, 3)),
        ];
        for (what, lp) in &circles {
            for (from, to) in [(0.0, 1.0), (0.0, 0.5), (0.5, 1.0)] {
                let b = extruded_on(vec![lp.clone()], frame, from, to, 8);
                let both = (to.min(1.0) - from.max(0.0)).max(0.0);
                coaxial(&format!("{plane} {what} {from}..{to}"), &a, &b, both, 64);
            }
        }
    }
}

#[test]
fn flush_pin_in_a_hole_on_the_side_planes() {
    // A pin in a plate's hole of its radius, flush with both caps or
    // through one of them, on the YZ plane (see above): the pin's rims lie
    // on the hole's.
    let (_, frame) = SIDE_PLANES[1];
    let c = DVec2::new(0.5, 0.2);
    let plate = extruded_on(
        vec![
            rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0),
            circle(c, 1.0, 4, true),
        ],
        frame,
        0.0,
        1.0,
        5,
    );
    for (from, to) in [(0.0, 1.0), (-1.0, 1.0)] {
        let pin = extruded_on(vec![circle(c, 1.0, 0, false)], frame, from, to, 7);
        let results = all_four(&plate, &pin, 1e-12);
        volumes(
            &format!("pin {from}..{to}"),
            &plate,
            &pin,
            &results,
            0.0,
            1e-9,
        );
    }
}

#[test]
fn coaxial_stacks_and_flush_pins_on_turned_frames() {
    // As on the side planes, on a frame turned off every axis and moved
    // far from the origin, and on one whose caps are upright off the axes
    // (nearly along `UP` again): cylinders of one radius stacked,
    // overlapping and over one span, and a pin flush in a plate's hole.
    let turned = DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 1.1);
    let frames = [
        Frame {
            origin: DVec3::new(2.5, -1.25, 7.0),
            x: turned * DVec3::X,
            y: turned * DVec3::Y,
        },
        Frame {
            origin: DVec3::new(0.3, -0.7, 0.2),
            x: DVec3::new(0.6, 0.8, 0.0),
            y: DVec3::Z,
        },
    ];
    let c = DVec2::new(0.5, 0.2);
    for (k, frame) in frames.into_iter().enumerate() {
        let a = extruded_on(vec![circle(c, 1.0, 0, false)], frame, 0.0, 1.0, 7);
        for (from, to) in [(1.0, 2.0), (0.5, 2.0), (0.0, 1.0), (0.25, 0.75)] {
            let b = extruded_on(vec![circle(c, 1.0, 0, false)], frame, from, to, 8);
            let both = (to.min(1.0) - from.max(0.0)).max(0.0);
            coaxial(&format!("frame {k}, {from}..{to}"), &a, &b, both, 128);
        }
        let plate = extruded_on(
            vec![
                rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0),
                circle(c, 1.0, 4, true),
            ],
            frame,
            0.0,
            1.0,
            5,
        );
        for (from, to) in [(0.0, 1.0), (-1.0, 1.0)] {
            let pin = extruded_on(vec![circle(c, 1.0, 0, false)], frame, from, to, 7);
            let results = all_four(&plate, &pin, 1e-12);
            let name = format!("frame {k}, pin {from}..{to}");
            volumes(&name, &plate, &pin, &results, 0.0, 1e-9);
        }
    }
}

#[test]
fn coaxial_seams_on_turned_frames_leave_no_slivers() {
    // Unions whose wall keeps a seam, on frames off the axes, which
    // rounding broke two ways. The second circle in 3 arcs from 0.7 rad,
    // stacked: its wall's cut along the seam runs along the patch's
    // domain side, with one end on the next side a few ulps in, and the
    // triangulation took the diagonal from that end past the cut's
    // vertices, leaving a triangle of zero width with all three corners
    // on the seam (`Invalid(Fold)`). A pin through a plate's hole, and
    // circles through exact axis points over 0..2: a cut whose two ends
    // are a rounding apart was traced round the circle the long way,
    // outside both triangles (`Invalid(Fold)`, `Invalid(Hull)`).
    let rot = |axis: DVec3, angle: f64, origin: DVec3| {
        let q = DQuat::from_axis_angle(axis.normalize(), angle);
        Frame {
            origin,
            x: q * DVec3::X,
            y: q * DVec3::Y,
        }
    };
    let general = rot(DVec3::new(1.0, 2.0, 3.0), 1.1, DVec3::new(2.5, -1.25, 7.0));
    let far = rot(
        DVec3::new(-0.3, 0.9, 0.2),
        2.3,
        DVec3::new(-40.0, 13.0, 5.5),
    );
    let upright = Frame {
        origin: DVec3::new(0.3, -0.7, 0.2),
        x: DVec3::new(0.6, 0.8, 0.0),
        y: DVec3::Z,
    };
    let c = DVec2::new(0.5, 0.2);
    let stacks = [
        ("upright", upright, 1.0, 2.0),
        ("upright", upright, -1.0, 0.0),
        ("general", general, -1.0, 0.0),
        ("far", far, -1.0, 0.0),
    ];
    for (name, frame, from, to) in stacks {
        let a = extruded_on(vec![circle(c, 1.0, 0, false)], frame, 0.0, 1.0, 7);
        let b = extruded_on(vec![turned_circle(c, 1.0, 0, 0.7, 3)], frame, from, to, 8);
        coaxial(&format!("{name} {from}..{to}"), &a, &b, 0.0, 128);
    }
    let a = extruded_on(vec![circle(c, 1.0, 0, false)], general, 0.0, 1.0, 7);
    let b = extruded_on(vec![circle(c, 1.0, 0, false)], general, 0.0, 2.0, 8);
    coaxial("general 0..2", &a, &b, 1.0, 128);
    // The pin and the hole drawn from cosines and sines, as a sketch's
    // circle is.
    let plate = extruded_on(
        vec![
            rect(DVec2::new(-3.0, -2.0), DVec2::new(3.0, 2.0), 0),
            reversed(&turned_circle(c, 1.0, 4, 0.0, 4)),
        ],
        general,
        0.0,
        1.0,
        5,
    );
    let pin = extruded_on(
        vec![turned_circle(c, 1.0, 0, 0.0, 4)],
        general,
        -1.0,
        2.0,
        7,
    );
    let results = all_four(&plate, &pin, 1e-12);
    volumes("pin -1..2", &plate, &pin, &results, 0.0, 1e-9);
    let other = run(&pin, &plate, Op::Union);
    assert!((other.volume() - results[0].volume()).abs() <= 1e-9);
    // A rounded rectangle over 0..1 and 0.5..2: the second's cap cut
    // along the first's rim arcs, one piece a tie long, whose zero-width
    // triangle a flip took a side from (`Invalid(Fold)` intersected).
    let corners = (c - DVec2::new(1.0, 0.7), c + DVec2::new(1.0, 0.7));
    let rounded = || super::seeded_tests::rounded(corners.0, corners.1, 0.4, 4);
    let a = extruded_on(vec![rounded()], general, 0.0, 1.0, 9);
    let b = extruded_on(vec![rounded()], general, 0.5, 2.0, 10);
    let area = a.volume();
    let results = all_four(&a, &b, 1e-12);
    volumes("rounded 0.5..2", &a, &b, &results, area * 0.5, 1e-9);
}

#[test]
fn a_profile_joined_again_over_a_longer_span() {
    // "Make it taller": a profile extruded over 0..10 and the same loops
    // again over a longer span, flush at one cap or past both, or
    // overlapping. Grown for the union, the first leaves a ring of its
    // cap of zero width between its rim and the second's wall cut at the
    // cap's plane, round every curved wall (the hole's too); the rim
    // takes the cut's vertices either way round, so the clean-up merges
    // the two. On the XZ and YZ planes the walls' rims and diagonals lie
    // on each other with shadows on one thin ellipse, whose crossings are
    // decided one by one. All five operations, against area × span.
    let plate = vec![
        rect(DVec2::new(-30.0, -20.0), DVec2::new(30.0, 20.0), 0),
        circle(DVec2::ZERO, 8.0, 4, true),
    ];
    let rounded = vec![super::seeded_tests::rounded(
        DVec2::new(-30.0, -20.0),
        DVec2::new(30.0, 20.0),
        5.0,
        0,
    )];
    // Straight bottom and left, an elliptic right side and a parabolic
    // top.
    let p = [
        DVec2::new(-20.0, -10.0),
        DVec2::new(20.0, -10.0),
        DVec2::new(20.0, 10.0),
        DVec2::new(-20.0, 10.0),
    ];
    let conic = |c: DVec2, w: f64, a: DVec2, b: DVec2, curve: u64| Segment {
        conic: crate::patch::Conic2::new(a, c, w, b).unwrap(),
        curve,
    };
    let outline = vec![Loop {
        segments: vec![
            Segment::line(p[0], p[1], 0).unwrap(),
            conic(DVec2::new(30.0, 0.0), 0.6, p[1], p[2], 1),
            conic(DVec2::new(0.0, 20.0), 1.0, p[2], p[3], 2),
            Segment::line(p[3], p[0], 3).unwrap(),
        ],
    }];
    let frames = [
        Frame::XY,
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Z,
        },
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::Y,
            y: DVec3::Z,
        },
    ];
    // Every span on XY; on the side planes the two the app's repro has
    // (Two sides 12 + 3, One side 12), which there failed for a shared
    // cap plane, as every operation did.
    let spans = [
        (-3.0, 12.0),
        (0.0, 12.0),
        (-15.0, 15.0),
        (5.0, 15.0),
        (-3.0, 10.0),
    ];
    for (name, loops) in [("plate", plate), ("rounded", rounded), ("outline", outline)] {
        for (k, &frame) in frames.iter().enumerate() {
            let a = extruded_on(loops.clone(), frame, 0.0, 10.0, 1);
            let area = a.volume() / 10.0;
            for &(from, to) in &spans[..if k == 0 { 5 } else { 2 }] {
                let b = extruded_on(loops.clone(), frame, from, to, 2);
                let both = area * (to.min(10.0) - from.max(0.0)).max(0.0);
                let (va, vb) = (a.volume(), b.volume());
                let cases = [
                    (&a, &b, Op::Union, va + vb - both),
                    (&b, &a, Op::Union, va + vb - both),
                    (&a, &b, Op::Difference, va - both),
                    (&b, &a, Op::Difference, vb - both),
                    (&a, &b, Op::Intersection, both),
                ];
                for (x, y, op, want) in cases {
                    let got = run(x, y, op);
                    let at = format!("{name} on frame {k}, {from}..{to}, {op:?}");
                    assert!(
                        (got.volume() - want).abs() <= 1e-9 * want.max(area),
                        "{at}: volume {}, not {want}",
                        got.volume()
                    );
                    if op == Op::Union {
                        let (n, most) = (got.mesh().tris().len(), b.mesh().tris().len());
                        assert!(n <= 4 * most + 16, "{at}: {n} patches, B has {most}");
                    }
                }
            }
        }
    }
}

#[test]
fn primitive_cylinders_stacked_unite() {
    // `Solid::cylinder`s of one radius on one axis, overlapping: the
    // union failed as `Invalid(EdgeNeighbours)`.
    let a = cylinder([0.0, 0.0, 0.0], 1.0, 1.0);
    for (base, height) in [(1.0, 1.0), (0.5, 1.5), (0.0, 2.0), (-0.5, 1.0)] {
        let b = cylinder([0.0, 0.0, base], 1.0, height);
        let both = ((base + height).min(1.0) - base.max(0.0)).max(0.0);
        coaxial(&format!("primitive {base}+{height}"), &a, &b, both, 64);
    }
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
    // Coaxial walls of one circle: rims taking the cut's vertices, near
    // twins, crossings of edges lying on each other decided one by one
    // (YZ), and the cut grazing its own vertices (an upright frame).
    let c = DVec2::new(0.5, 0.2);
    let (_, side) = SIDE_PLANES[1];
    let upright = Frame {
        origin: DVec3::new(0.3, -0.7, 0.2),
        x: DVec3::new(0.6, 0.8, 0.0),
        y: DVec3::Z,
    };
    for (frame, from, to) in [(side, 0.0, 2.0), (upright, 1.0, 2.0)] {
        let a = extruded_on(vec![circle(c, 1.0, 0, false)], frame, 0.0, 1.0, 7);
        let b = extruded_on(vec![turned_circle(c, 1.0, 0, 0.7, 3)], frame, from, to, 8);
        assert_deterministic(|| run(&a, &b, Op::Union));
    }
}

#[test]
fn random_bars_through_boxes_are_right_or_refused() {
    // Turned and moved bars against boxes: every result checked, exact
    // and with its volume, or refused as invalid (a hull or fold repair
    // can't mend) or too complex; never a wrong solid. Seeds 1 and 2 had
    // crossings the search missed left off the cylinder (cases 54 and
    // 131, see `bands_left_straying_past_the_tolerance_are_refused`) and
    // a sliver pulled off it by a nearly straight arc's weight (case
    // 154, see `nearly_straight_arcs_keep_their_bands_on_the_cylinder`):
    // copies of the wall claiming no surface, identities off by up to
    // 2.7e-5.
    for (seed, count, least) in [(7, 24, 20), (1, 24, 20), (2, 24, 20)] {
        let done = bars_through_boxes(seed, 0..count);
        // Most go through.
        assert!(done >= least, "seed {seed}: {done}");
    }
}

/// The turned and moved bar through a box that the seeded bars' generator
/// draws at `seed`, draw `case` (from 0).
fn bar_and_box(seed: u64, case: usize) -> (Solid, Solid) {
    let mut rng = crate::test_rng::Rng::new(seed);
    let mut draw = || {
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
        (bar, Solid::cuboid(min, size, 1, &TOL).unwrap())
    };
    for _ in 0..case {
        draw();
    }
    draw()
}

/// The bars through boxes of `cases` at `seed`, all four operations:
/// each result exact (every patch on its face's surface to `1e-11` of
/// the size 4, none claiming no surface) or refused as invalid, too
/// complex or with loops that can't be triangulated (`Degenerate`: an
/// intersection and a difference each of seed 7's case 189 and seed 1's
/// case 97), and where all four go through, the volume identities within
/// `1e-11`. How many cases went through.
fn bars_through_boxes(seed: u64, cases: std::ops::Range<usize>) -> usize {
    let mut done = 0;
    for case in cases {
        let (bar, block) = bar_and_box(seed, case);
        let jobs = [
            (&bar, &block, Op::Union),
            (&bar, &block, Op::Intersection),
            (&bar, &block, Op::Difference),
            (&block, &bar, Op::Difference),
        ];
        let got = jobs.map(
            |(x, y, op)| match boolean(x, y, op, &TOL, &Budget::DEFAULT) {
                Ok(solid) => {
                    exact_to(
                        &format!("seed {seed}, case {case}, {op:?}"),
                        &solid,
                        4.0,
                        1e-11,
                    );
                    Some(solid.volume())
                }
                Err(
                    KernelError::Invalid(_)
                    | KernelError::TooComplex
                    | KernelError::Boolean(BooleanError::Degenerate),
                ) => None,
                Err(e) => panic!("seed {seed}, case {case}, {op:?}: {e:?}"),
            },
        );
        if let [Some(u), Some(i), Some(d), Some(e)] = got {
            let (va, vb) = (bar.volume(), block.volume());
            for (what, off) in [
                ("A ∪ B + A ∩ B", u + i - va - vb),
                ("A − B", d - (va - i)),
                ("B − A", e - (vb - i)),
            ] {
                assert!(
                    off.abs() <= 1e-11,
                    "seed {seed}, case {case}: {what} off by {off:e}"
                );
            }
            done += 1;
        }
    }
    done
}

#[test]
#[ignore = "slow: 200 cases of four seeds; run in release"]
fn many_bars_through_boxes_are_exact_or_refused() {
    // 185, 190, 190 and 189 of 200 go through.
    for seed in [7, 1, 2, 3] {
        let done = bars_through_boxes(seed, 0..200);
        assert!(done >= 180, "seed {seed}: {done}");
    }
}

#[test]
fn nearly_straight_arcs_keep_their_bands_on_the_cylinder() {
    // The seeded bars' generator, seed 1, its 155th draw: a nearly
    // straight arc of a plane section took its weight from a point where
    // the line from its chord's middle to its control point meets the
    // cylinder, a rounding's worth of bulge away: 1.000019, where an
    // ellipse arc's is under 1. The arc lay on the cylinder, but the
    // sliver beside it was pulled 1.7e-6 off onto a copy of the wall
    // claiming no surface, in the union and `bar − box`. The weight is
    // now the arc's angle about the axis.
    let (bar, block) = bar_and_box(1, 154);
    four_exact(&bar, &block, &TOL, 4.0, 1e-12, 1e-11);
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
fn slivers_cut_off_a_wall_along_its_rulings_are_kept() {
    // A box face along a cylinder's rulings, cutting a sliver 1.5e-3 to
    // 0.1 deep off its wall (seen fuzzing boxes grazing walls, at a
    // fit of 1e-3): the sliver's wall triangles have every corner on the
    // box face, so by its triangles' corners it enclosed nothing, and the
    // clean-up dropped it as a component of no volume: `a − b` was `Ok`
    // and empty, and `b − a` and `a ∪ b` had a slot where it was.
    // Volumes against the segment's closed form, by its angle.
    let a = cylinder([0.0, 0.0, 0.0], 1.0, 5.0);
    let va = a.volume();
    let mut done = 0;
    for (depth, turn) in [
        (1.5e-3, 0.0),
        (3e-3, 0.0),
        (2e-2, 0.0),
        (0.1, 0.0),
        (1.5e-3, PI / 4.0),
        (3e-3, 0.4),
        (2e-2, 2.0),
        (0.1, 0.4),
    ] {
        let (s, c) = turn.sin_cos();
        let to = |p: DVec3| DVec3::new(c * p.x - s * p.y, s * p.x + c * p.y, p.z);
        // `x ≥ 1 − depth` in the turned frame, past the cylinder.
        let b = moved(
            &cube([1.0 - depth, -2.0, -1.0], [2.0 + depth, 4.0, 7.0]),
            to,
        );
        let half = (1.0 - depth).acos();
        let sliver = 5.0 * (half - half.sin() * half.cos());
        let vb = b.volume();
        for (x, y, op, want) in [
            (&a, &b, Op::Difference, va - sliver),
            (&a, &b, Op::Intersection, sliver),
            (&b, &a, Op::Difference, vb - sliver),
            (&a, &b, Op::Union, va + vb - sliver),
        ] {
            if let Ok(r) = boolean(x, y, op, &TOL, &Budget::DEFAULT) {
                let got = r.volume();
                assert!(
                    (got - want).abs() < 1e-9,
                    "{depth} {turn} {op:?}: {got} not {want}"
                );
                done += 1;
            }
        }
    }
    // Two slivers' intersections and `b − a` are refused (a fold).
    assert!(done >= 28, "{done}");
}

#[test]
fn crossings_the_search_only_placed_go_onto_both_surfaces() {
    // A cylinder's curved rim edge against a crossing cylinder's wall
    // (found fuzzing related solids, seed 1, case 19): the search stopped
    // at its cap having found nothing, and the crossing the count has was
    // placed where the two came closest, 4 resolutions off the wall.
    // Solving it again on the quadric didn't move it that far, and its
    // band went on a copy of the wall claiming no surface: all four
    // operations were `Ok` with that vertex off (then refused, once
    // crossings only placed were checked). Now it goes to the rim's root
    // on the wall: all four `Ok`, every new vertex on both surfaces, the
    // volumes adding up.
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
    let [u, i, d, e] = [
        (&a, &b, Op::Union),
        (&a, &b, Op::Intersection),
        (&a, &b, Op::Difference),
        (&b, &a, Op::Difference),
    ]
    .map(|(x, y, op)| {
        let solid = run(x, y, op);
        let off = off_both(&solid, &a, &b);
        assert!(off <= 1e-12, "{op:?}: a vertex {off:e} off");
        solid.volume()
    });
    let (va, vb) = (a.volume(), b.volume());
    let within = TOL.fit() * (a.area() + b.area()) / 100.0;
    assert!((u + i - va - vb).abs() <= within);
    assert!((d - (va - i)).abs() <= within);
    assert!((e - (vb - i)).abs() <= within);
}

/// `f` run with crossings the search only placed left as they were
/// before they were placed at a root on the patch crossed, and the check
/// that they lie on the surface they cross skipped (`certified` false),
/// or not.
fn certified<T>(certified: bool, f: impl FnOnce() -> T) -> T {
    super::assemble::LOOSE.set(!certified);
    let out = f();
    super::assemble::LOOSE.set(false);
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

/// `a ∪ b`, `a ∩ b`, `a − b` and `b − a` at `tol`, each checked exact
/// (every patch on its face's surface to `rel` of `size`, none claiming
/// no surface), and the three volume identities to `within`.
fn four_exact(a: &Solid, b: &Solid, tol: &Tolerance, size: f64, rel: f64, within: f64) {
    let [u, i, d, e] = [
        (a, b, Op::Union),
        (a, b, Op::Intersection),
        (a, b, Op::Difference),
        (b, a, Op::Difference),
    ]
    .map(|(x, y, op)| {
        let solid = boolean(x, y, op, tol, &Budget::DEFAULT)
            .unwrap_or_else(|e| panic!("fit {}, {op:?}: {e:?}", tol.fit()));
        exact_to(&format!("fit {}, {op:?}", tol.fit()), &solid, size, rel);
        solid.volume()
    });
    let (va, vb) = (a.volume(), b.volume());
    for (what, off) in [
        ("A ∪ B + A ∩ B", u + i - va - vb),
        ("A − B", d - (va - i)),
        ("B − A", e - (vb - i)),
    ] {
        assert!(
            off.abs() <= within,
            "fit {}: {what} off by {off:e}",
            tol.fit()
        );
    }
}

/// [`four_off`] with crossings the search only placed left where they
/// were and not checked: each result refused (only at a fit finer than
/// `1e-3`) or its claim-free patches within the fit tolerance of their
/// walls `of`, and the volume identities within the tolerance's
/// allowance. The results, for the caller's own checks.
fn four_loose(
    a: &Solid,
    b: &Solid,
    of: &[crate::mesh::Face],
    tol: &Tolerance,
) -> [Result<(f64, f64), KernelError>; 4] {
    let fit = tol.fit();
    let got = certified(false, || four_off(a, b, of, tol));
    for (k, result) in got.iter().enumerate() {
        match result {
            Ok((_, off)) => assert!(*off <= fit, "fit {fit}, result {k}: {off:e} off"),
            Err(e) => assert!(fit < 1e-3, "fit {fit}, result {k}: {e:?}"),
        }
    }
    if let [Ok((u, _)), Ok((i, _)), Ok((d, _)), Ok((e, _))] = got {
        let (va, vb) = (a.volume(), b.volume());
        let within = fit * (a.area() + b.area()) / 100.0;
        assert!((u + i - va - vb).abs() <= within);
        assert!((d - (va - i)).abs() <= within);
        assert!((e - (vb - i)).abs() <= within);
    }
    got
}

#[test]
fn bands_left_straying_past_the_tolerance_are_refused() {
    // A turned bar through a box (the seeded bars' generator, seed 1, its
    // 55th draw): the search for a box edge's crossing through the bar's
    // wall ran out of pieces, and the crossing the count has was placed
    // where the two came closest, 1.6e-3 of the edge from its root and
    // 5.6e-4 off the cylinder; no halving of the cut moves the bands at
    // it closer. When the rounds of halving ran out the result was kept,
    // at the finest tolerance with bands 56 times the tolerance off. Now
    // the crossing goes to the edge's root on the patch crossed: all four
    // exact at every tolerance. Left where it was (and not checked), the
    // bands are refused as too complex where they are past the tolerance,
    // and kept within it at the default one.
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
        four_exact(&bar, &block, &tol, 4.0, 1e-12, 1e-11);
        let loose = four_loose(&bar, &block, &walls(&bar), &tol);
        if fit < 1e-3 {
            // The union and `bar − box`, which were kept.
            assert!(matches!(loose[0], Err(KernelError::TooComplex)));
            assert!(matches!(loose[2], Err(KernelError::TooComplex)));
        } else {
            assert!(
                loose
                    .iter()
                    .all(|r| r.as_ref().is_ok_and(|&(_, off)| off > 1e-4))
            );
        }
    }
}

#[test]
fn a_crossing_the_search_misses_lands_on_the_cylinder() {
    // Another turned bar through a box (the seeded bars' generator, seed
    // 2, its 132nd draw): the search for a box edge's crossing through
    // the bar's wall ran out of pieces, the crossing was placed 1.0e-4 of
    // the edge from its root, 2.0e-4 off the cylinder, and 2 to 4 patches
    // of every result went on copies of the wall claiming no surface,
    // the volume identities off by 2.7e-5; then all four were refused as
    // off the surface. Now they are exact.
    let c = DVec3::new(0.7778535523495764, 0.3332140911888426, 0.3730829564654845);
    let q = DQuat::from_xyzw(
        -0.2856207075786154,
        0.34875799816262115,
        -0.11249121681212451,
        0.885513634146883,
    );
    let bar = moved(&cylinder([0.0, 0.0, -2.0], 0.20027727793058037, 4.0), |p| {
        q * p + c
    });
    let block = Solid::cuboid(
        DVec3::new(-0.789859021193339, -0.6396402831554717, -0.7650470984448043),
        DVec3::new(1.1924531490632726, 2.0146868583713413, 2.642857569158646),
        1,
        &TOL,
    )
    .unwrap();
    four_exact(&bar, &block, &TOL, 4.0, 1e-12, 1e-11);
}

#[test]
fn a_hole_from_a_slanted_wall_through_a_prism_is_exact() {
    // A triangular prism (legs 6 and 4 on its base, 3 tall), and a hole
    // of radius 0.5 sketched on a frame on its slanted wall, halfway up,
    // cut square to that wall through the prism: it leaves by the base's
    // wall at 42° to its axis, in ellipses. The prism upright and turned
    // and moved every way; every patch of all four results on its
    // surface, none claiming no surface, and the volumes the analytic
    // ones (the hole's is its section times its length on the axis).
    let (a, b, c) = (
        DVec2::new(0.0, 0.0),
        DVec2::new(6.0, 0.0),
        DVec2::new(1.5, 4.0),
    );
    let (r, height) = (0.5, 3.0);
    let turned = DQuat::from_rotation_x(0.4) * DQuat::from_rotation_z(0.3);
    for base in [
        Frame::XY,
        Frame {
            origin: DVec3::new(0.3, -0.2, 0.1),
            x: turned * DVec3::X,
            y: turned * DVec3::Y,
        },
    ] {
        let prism = extruded_on(vec![polygon(&[a, b, c], 0)], base, 0.0, height, 1);
        // On the wall from `b` to `c`, facing out of the prism.
        let along = (c - b).normalize();
        let wall = Frame {
            origin: base.point((b + c) * 0.5, height / 2.0),
            x: base.x * along.x + base.y * along.y,
            y: base.normal(),
        };
        let hole = extruded_on(vec![circle(DVec2::ZERO, r, 10, false)], wall, -8.0, 0.5, 2);
        // Into the prism, square to the wall, from its middle (`y` 2 on
        // the base) to the base's wall `y = 0`.
        let inward = DVec2::new(-along.y, along.x);
        let length = -((b + c) * 0.5).y / inward.y;
        let both = PI * r * r * length;
        let results = all_four(&prism, &hole, 1e-12);
        volumes("slanted", &prism, &hole, &results, both, 1e-11);
        for solid in &results {
            exact("slanted", solid, 10.0);
        }
    }
}

#[test]
fn a_box_turned_a_little_on_an_elliptic_wall_is_exact() {
    // A wall over an ellipse arc (weight 0.2) and a small box turned by a
    // thousandth of a radian across it (seen fuzzing turned boxes on
    // walls over conics, seed 21, case 109): the box's sides nearly along
    // the rulings cut the wall in nearly straight arcs whose weights came
    // from a rounding's worth of bulge, and the volumes were 5.6e-7 off.
    // On elliptic cylinders the weights come from the arcs' angles: all
    // four exact.
    let top = Segment {
        conic: crate::patch::Conic2::new(
            DVec2::new(10.0, 10.0),
            DVec2::new(5.400097164621307, 13.24986059045539),
            0.20018564583233608,
            DVec2::new(0.0, 10.0),
        )
        .unwrap(),
        curve: 2,
    };
    let lp = Loop {
        segments: vec![
            Segment::line(DVec2::ZERO, DVec2::new(10.0, 0.0), 0).unwrap(),
            Segment::line(DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0), 1).unwrap(),
            top,
            Segment::line(DVec2::new(0.0, 10.0), DVec2::ZERO, 3).unwrap(),
        ],
    };
    let frame = Frame {
        origin: DVec3::ZERO,
        x: DVec3::NEG_Z,
        y: DVec3::Y,
    };
    let wall = extruded_on(vec![lp], frame, 0.0, 5.0, 9);
    let size = DVec3::new(0.10543297279619278, 0.1334177726580953, 0.47900062681421013);
    let q = DQuat::from_xyzw(
        0.0009735174955277531,
        -0.000904343589993779,
        -0.00012403122298213602,
        0.99999910952091,
    );
    let near = DVec3::new(4.169835138127173, 10.18762286849587, -9.508341255174468);
    let off = DVec3::new(
        0.024884254504193626,
        -0.031342430114924796,
        -0.06721143679136833,
    );
    let block = moved(&cube([0.0; 3], size.to_array()), |p| {
        q * (p - size / 2.0 + off) + near
    });
    four_exact(&wall, &block, &TOL, 10.0, 1e-12, 1e-10);
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
    // walls: the boolean's own cap triangles fold (an arc's cut piece
    // fanned to a far corner) or are flat slivers.
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
    // fuzzing): a crossing the search only placed sat 1.3e-4 off the
    // small cylinder's wall, and the triangles at it, some along no cut
    // (band trees' roots no ruling frees), went on a copy of the wall
    // claiming no surface, that far off at every tolerance: the union
    // was kept at 1.3 times the tolerance of 1e-4. Now the crossing goes
    // to the edge's root on the patch crossed: all four exact at every
    // tolerance. Left where it was (and not checked), the triangles are
    // refused as too complex where they are past the tolerance, and kept
    // within it at the default one; at the finest, the cut from it isn't
    // traced, and its fallback, off the true cut, is refused first.
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
        four_exact(&a, &b, &tol, 10.0, 1e-12, 1e-10);
        let wall = [walls(&a), walls(&b)].concat();
        let loose = four_loose(&a, &b, &wall, &tol);
        if fit < 1e-3 {
            assert!(matches!(
                loose[0],
                Err(KernelError::TooComplex | KernelError::Boolean(BooleanError::Inconsistent))
            ));
        } else {
            assert!(
                loose
                    .iter()
                    .all(|r| r.as_ref().is_ok_and(|&(_, off)| off > 1e-5))
            );
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

#[test]
fn a_cap_folding_when_refined_is_refused() {
    // A horizontal cylinder across the top of a prism whose cap is one
    // triangle with two curved sides, one a concave hyperbola. The pair
    // refinement splits the cap with straight inner edges, and a piece's
    // corner at a curve's midpoint turns inside out: the folded piece goes
    // into the result, which repair can't mend, and every operation is
    // refused. Patches made safe to split at construction (their straight
    // children passing the fold check) would turn these into `Ok`s.
    let a = extruded(vec![crate::profile::tests::folding_cap(false)], 0.0, 5.0, 9);
    let b = cylinder_x(3.0, 5.2, 0.6, -2.0, 12.0);
    let mut work = Work::new(&Budget::DEFAULT);
    let refined = pairs::refined(a.mesh(), b.mesh(), false, &TOL, &mut work).unwrap();
    let folded = (0..refined.a.tris().len())
        .filter(|&t| refined.a.patch(t).fold_direction().is_none())
        .count();
    assert!(folded > 0);
    for op in [Op::Union, Op::Intersection, Op::Difference] {
        for (x, y) in [(&a, &b), (&b, &a)] {
            let got = boolean(x, y, op, &TOL, &Budget::DEFAULT);
            assert!(
                matches!(got, Err(KernelError::Invalid(_))),
                "{op:?}: {got:?}"
            );
        }
    }
    // With a parabola for the concave side, all three go through, the
    // identities within the fit tolerance's allowance (the cuts between
    // the walls are fitted; they are off by about 1e-4).
    let a = extruded(vec![crate::profile::tests::folding_cap(true)], 0.0, 5.0, 9);
    let (va, vb) = (a.volume(), b.volume());
    let within = TOL.fit() * (a.area() + b.area()) / 100.0;
    let [u, i, d] =
        [Op::Union, Op::Intersection, Op::Difference].map(|op| run(&a, &b, op).volume());
    assert!((u + i - va - vb).abs() <= within, "{u} + {i}");
    assert!((d + i - va).abs() <= within, "{d} + {i}");
    // The intersection by numerical integration over the profile (the
    // conics flattened to 4 000 points each, the disk's chords in `z`
    // cut at the top): 1.75423, which the result is about 2e-5 under.
    assert!((i - 1.75423).abs() <= within, "{i}");
}

/// The circle round `center` of radius `r` in `arcs` arcs, the first
/// from the angle `start`.
fn arcs_circle(center: DVec2, r: f64, arcs: usize, start: f64) -> Loop {
    let points: Vec<DVec2> = (0..arcs)
        .map(|i| {
            let t = start + std::f64::consts::TAU * i as f64 / arcs as f64;
            center + DVec2::new(t.cos(), t.sin()) * r
        })
        .collect();
    Loop {
        segments: (0..arcs)
            .map(|i| {
                crate::profile::tests::arc(center, points[i], points[(i + 1) % arcs], i as u64)
            })
            .collect(),
    }
}

/// A cylinder of radius `r` on XY from 0 to 5, its circle in `arcs` arcs
/// from the angle `start`, and a box whose face is the plane `n·(x − c) =
/// s` through `c` = (0, 0, 2.5), `n` tilted `alpha` up from the
/// horizontal direction at the angle `phi` (so the plane is `alpha` off
/// the cylinder's rulings), the box on the side `n·(x − c) ≤ s`; and the
/// volume of their intersection in closed form.
fn cylinder_and_tilted_box(
    r: f64,
    arcs: usize,
    start: f64,
    alpha: f64,
    phi: f64,
    s: f64,
) -> (Solid, Solid, f64) {
    let a = extruded(vec![arcs_circle(DVec2::ZERO, r, arcs, start)], 0.0, 5.0, 9);
    let m = DVec2::new(phi.cos(), phi.sin());
    let n = DVec3::new(m.x * alpha.cos(), m.y * alpha.cos(), alpha.sin());
    let e1 = DVec3::new(-m.y, m.x, 0.0);
    let e2 = n.cross(e1);
    let c = DVec3::new(0.0, 0.0, 2.5);
    let (depth, wide) = (2.0 * r + 8.0, r + 8.0);
    let b = moved(
        &cube([s - depth, -wide, -wide], [depth, 2.0 * wide, 2.0 * wide]),
        |p| c + n * p.x + e1 * p.y + e2 * p.z,
    );
    // At height `z` the box holds the disk's points `x` with `x·m ≤
    // u(z)`, which is `cap(−u(z))`.
    let u = |z: f64| (s - (z - 2.5) * alpha.sin()) / alpha.cos();
    (a, b, caps_along(r, -u(0.0), -u(5.0), 5.0))
}

/// `∫ cap(w(z)) dz` over `0..len`, `cap(w)` the area of the part of a
/// disk of radius `r` past the chord `w` from its centre, `w` going
/// linearly from `w0` to `w1` (not equal): by the antiderivative of the
/// cap in `w`, `r²·w·acos(w/r) − r²·√(r² − w²) + (r² − w²)^{3/2}/3`
/// within the disk (0 past it, `πr²·w` before it), on the side where
/// the caps are small (the rest is the disk's).
fn caps_along(r: f64, w0: f64, w1: f64, len: f64) -> f64 {
    if w0 + w1 < 0.0 {
        return PI * r * r * len - caps_along(r, -w0, -w1, len);
    }
    let f = |w: f64| {
        if w >= r {
            0.0
        } else if w <= -r {
            PI * r * r * w
        } else {
            let s = (r * r - w * w).sqrt();
            r * r * w * (w / r).acos() - r * r * s + s * s * s / 3.0
        }
    };
    (f(w1) - f(w0)) * len / (w1 - w0)
}

#[test]
fn slivers_round_a_section_tip_are_kept_or_refused() {
    // A box face 1e-4 off a cylinder's rulings, 1.75e-4 inside its wall
    // at mid-height: the cut is the tip of a long, thin ellipse, a U on
    // the wall from z 0.75 to 4.25 (seen fuzzing boxes grazing walls).
    // The arc round the tip turns back inside one patch, so the exact
    // section, guided by the patch's middle (on the chord), took the
    // wrong arc and failed, tracing and the conic along the ends'
    // tangents failed too, and the cut was the straight chord between
    // the ends, which the bands along it passed (it is on the plane and
    // within the fit of the wall). The intersection, the sliver, was
    // `Ok` but ended at z 2.559: a tip 1.69 long and 10 % of its volume
    // gone.
    let r = 1.7487237938385212;
    let (a, b, both) = cylinder_and_tilted_box(
        r,
        5,
        3.4279196762232473,
        1e-4,
        4.684556737659165,
        -r + 1.7487237938396127e-4,
    );
    assert!((both - 3.7109669699556e-5).abs() <= 1e-15, "{both}");
    let (va, vb) = (a.volume(), b.volume());
    for (x, y, op, want) in [
        (&a, &b, Op::Intersection, both),
        (&a, &b, Op::Difference, va - both),
        (&b, &a, Op::Difference, vb - both),
        (&a, &b, Op::Union, va + vb - both),
    ] {
        if let Ok(result) = boolean(x, y, op, &TOL, &Budget::DEFAULT) {
            let got = result.volume();
            assert!((got - want).abs() <= 1e-9, "{op:?}: {got} not {want}");
            if op == Op::Intersection {
                let top = result.bounds3().expect("a sliver").max.z;
                assert!(top >= 4.2, "the sliver ends at z {top}");
            }
        }
    }
}

#[test]
fn chords_across_a_section_tip_are_refused() {
    // The sliver above: its chord across the U is checked against the
    // true cut and refused, and the operation with it.
    let r = 1.7487237938385212;
    let (a, b, _) = cylinder_and_tilted_box(
        r,
        5,
        3.4279196762232473,
        1e-4,
        4.684556737659165,
        -r + 1.7487237938396127e-4,
    );
    let before = super::chain::REFUSED.get();
    let got = boolean(&a, &b, Op::Intersection, &TOL, &Budget::DEFAULT);
    assert!(
        matches!(got, Err(KernelError::Boolean(BooleanError::Inconsistent))),
        "{:?}",
        got.map(|s| s.volume())
    );
    assert!(super::chain::REFUSED.get() > before);
}

#[test]
fn a_box_tangent_on_a_seam_is_exact_or_refused() {
    // A box face 1e-5 off a cylinder's rulings, tangent to its wall at
    // mid-height, on the seam between two of its six arcs, both ways
    // round: a sliver 2.5e-5 deep at most, its cut a U 2.5 tall and 0.02
    // wide. The arc round the tip took the straight chord, as above, and
    // the results keeping the cylinder had a band claiming no surface,
    // fanned from the chord to the far corner of its strip (area 2.34),
    // and were 5.1e-6 off in volume on a sliver of 3.3e-7.
    let r = 1.8794419898132737;
    for s in [r, -r] {
        let (a, b, both) = cylinder_and_tilted_box(r, 6, 0.0, 1e-5, 0.0, s);
        let (va, vb) = (a.volume(), b.volume());
        for (x, y, op, want) in [
            (&a, &b, Op::Intersection, both),
            (&a, &b, Op::Difference, va - both),
            (&b, &a, Op::Difference, vb - both),
            (&a, &b, Op::Union, va + vb - both),
        ] {
            if let Ok(result) = boolean(x, y, op, &TOL, &Budget::DEFAULT) {
                let got = result.volume();
                assert!((got - want).abs() <= 1e-9, "{s} {op:?}: {got} not {want}");
                let (_, free) = off_surface(&result);
                assert_eq!(free, 0, "{s} {op:?}: {free} patches claim no surface");
            }
        }
    }
}

#[test]
fn slanted_cuts_round_a_boss_silhouette_are_exact() {
    // A plane half a radian off a boss's rulings, at 24 offsets across
    // it: the cut is an ellipse arc wherever it is, and where it turns
    // round near the silhouette within one patch the arc the patch's
    // middle picked was the wrong one, which failed to stay on the patch;
    // the other arc wasn't tried, and the cut was traced and fitted
    // (about 40 of the 48 operations), right but not exact.
    let mut done = 0;
    for k in 0..24 {
        let s = -1.0 + 2.0 * (k as f64 + 0.5) / 24.0;
        let (a, b, both) = cylinder_and_tilted_box(1.0, 4, 0.0, 0.5, 0.0, s);
        for (op, want) in [
            (Op::Intersection, both),
            (Op::Difference, a.volume() - both),
        ] {
            let before = super::chain::NOT_EXACT.get();
            let result = run(&a, &b, op);
            assert_eq!(
                super::chain::NOT_EXACT.get(),
                before,
                "{s} {op:?}: a cut not exact"
            );
            let got = result.volume();
            assert!((got - want).abs() <= 1e-9, "{s} {op:?}: {got} not {want}");
            done += 1;
        }
    }
    assert_eq!(done, 48);
}

#[test]
fn nicks_by_a_crossing_cylinder_keep_their_checked_fallbacks() {
    // An upright cylinder (z 0..5) nicked by a level one whose axis passes
    // a few `1e-5` short of touching it: the cut is a small loop round
    // the near-contact, between two curved faces. In the differences,
    // where the second operand's faces are turned over, fitting the
    // traced cut fails on short arcs of it, and they fall back to the
    // conic along their end tangents, checked against the true cut. These
    // pass (on both surfaces within the resolution at each sample), and
    // the results are right to `1e-9` by the closed-form volume, the
    // integral across of the two cylinders' chords. Seen fuzzing crossing
    // cylinders near tangency: no fallback the check kept was more than
    // half the fit tolerance off the true cut, either way.
    // (radius, arcs, start; radius, arcs, start; axis offset, height,
    // turn about z)
    let cases = [
        (
            2.9353162683929117,
            3,
            0.0,
            1.4612062815086588,
            3,
            0.07775758968323955,
            4.396493196738887,
            2.026010514718798,
            0.0,
        ),
        (
            2.576506370630734,
            5,
            5.809234706107411,
            2.281975447942656,
            5,
            2.2764929082702388,
            4.858456053509683,
            2.5564323366386157,
            5.877674653880665,
        ),
        (
            0.8209315858225135,
            6,
            0.0,
            1.2262911563217929,
            8,
            0.0,
            2.0471406489857245,
            2.5254436350503964,
            3.8358433064912223,
        ),
    ];
    let mut ok = 0;
    for (big, na, start, rho, nb, sb, c, zc, turn) in cases {
        let a = extruded(vec![arcs_circle(DVec2::ZERO, big, na, start)], 0.0, 5.0, 9);
        let frame = Frame {
            origin: DVec3::ZERO,
            x: DQuat::from_rotation_z(turn) * DVec3::Y,
            y: DVec3::Z,
        };
        let len = big + 5.0;
        let b = extruded_on(
            vec![arcs_circle(DVec2::new(c, zc), rho, nb, sb)],
            frame,
            -len,
            len,
            30,
        );
        // Across the level cylinder's axis, at `y` from the upright one's
        // axis, the two chords' lengths multiply; by `y = lo + (hi −
        // lo)(1 − cos t)/2`, which takes the square roots at the ends, and
        // Simpson's rule.
        let (lo, hi) = ((c - rho).max(-big), (c + rho).min(big));
        let f = |t: f64| {
            let y = lo + (hi - lo) * (1.0 - t.cos()) / 2.0;
            let chords = 4.0
                * (big * big - y * y).max(0.0).sqrt()
                * (rho * rho - (y - c) * (y - c)).max(0.0).sqrt();
            chords * (hi - lo) * t.sin() / 2.0
        };
        let panels = 20000;
        let h = PI / panels as f64;
        let both: f64 = (0..panels)
            .map(|k| {
                let t = k as f64 * h;
                h / 6.0 * (f(t) + 4.0 * f(t + h / 2.0) + f(t + h))
            })
            .sum();
        let (va, vb) = (a.volume(), b.volume());
        for (x, y, want) in [(&a, &b, va - both), (&b, &a, vb - both)] {
            let before = super::chain::REFUSED.get();
            let got = boolean(x, y, Op::Difference, &TOL, &Budget::DEFAULT);
            assert_eq!(
                super::chain::REFUSED.get(),
                before,
                "{big}: a chain refused"
            );
            if let Ok(result) = got {
                let got = result.volume();
                assert!((got - want).abs() <= 1e-9, "{big}: {got} not {want}");
                ok += 1;
            }
        }
    }
    // Four of the six go through today, the others are refused later on
    // (`Invalid`), their chains kept.
    assert!(ok >= 4, "{ok}");
}

#[test]
fn a_cross_hole_through_a_round_boss() {
    // The app's way: a circle of radius 10 extruded on XY 10 tall, cut
    // through by a small circle sketched on YZ and extruded across it.
    // The hole lies inside one of the wall's strip triangles, whose
    // corners its rim was joined to in thin fans; repair ran out of
    // budget on them before the cut faces took points for their shapes.
    // Radius, centre across (`y`) and height.
    let cases = [
        (0.31256995285820194, 5.964459210246115, 3.9767549202973522),
        (0.3035026029895933, -2.3024810212792435, 3.18835851057847),
        (0.8884143554800221, -7.26102927247244, 3.9467772697627765),
        (1.0604441575349153, -1.2265091824143948, 4.416762355501527),
        (0.460251704955262, 5.126249626072438, 1.829180504957189),
        (0.5352456799963462, 2.5048105870136936, 4.511384144216814),
        (0.9788495762708371, -0.1612215448299814, 5.779121451610788),
    ];
    let big = 10.0;
    let boss = extruded(vec![circle(DVec2::ZERO, big, 1, false)], 0.0, 10.0, 1);
    let yz = Frame {
        origin: DVec3::ZERO,
        x: DVec3::Y,
        y: DVec3::Z,
    };
    for (r, s, z) in cases {
        let hole = extruded_on(
            vec![circle(DVec2::new(s, z), r, 5, false)],
            yz,
            -15.0,
            15.0,
            2,
        );
        // The cut faces' points are placed the same at 1 and 8 threads.
        let got = assert_deterministic(|| run(&boss, &hole, Op::Difference));
        // The hole's part inside the boss, scaled from the unit boss's.
        let want = PI * big * big * 10.0 - big.powi(3) * crossed(r / big, s / big);
        let within = TOL.fit() * (boss.area() + hole.area()) / 5.0;
        assert!(
            (got.volume() - want).abs() <= within,
            "r {r} at {s}, {z}: {} not {want}",
            got.volume()
        );
    }
}

mod flush_seams;
mod one_face;

/// How many faces (keys) of `solid` lie on a quadric.
fn quadric_faces(solid: &Solid) -> usize {
    let keys: std::collections::BTreeSet<_> = solid
        .mesh()
        .faces()
        .iter()
        .filter(|f| matches!(f.surface, Surface::Quadric(_)))
        .map(|f| f.name.key())
        .collect();
    keys.len()
}

/// How many feature edges `solid` draws.
fn feature_edges(solid: &Solid) -> usize {
    solid
        .tessellate(&crate::Display::new(&TOL))
        .unwrap()
        .edges()
        .len()
}

#[test]
fn stacked_cylinders_have_one_wall() {
    // One on the other: the walls are one face, named by the first, drawn
    // as one cylinder's from 0 to 2 is, and the second's names resolve to
    // it.
    let low = Solid::cylinder(DVec3::ZERO, 1.0, 1.0, 1, &TOL).unwrap();
    let high = Solid::cylinder(DVec3::Z, 1.0, 1.0, 2, &TOL).unwrap();
    let whole = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 1, &TOL).unwrap();
    for (a, b) in [(&low, &high), (&high, &low)] {
        let r = run(a, b, Op::Union);
        assert!((r.volume() - 2.0 * PI).abs() < 1e-9, "{}", r.volume());
        assert_eq!(quadric_faces(&r), 1);
        assert_eq!(feature_edges(&r), feature_edges(&whole));
        let wall = r
            .mesh()
            .faces()
            .iter()
            .find(|f| matches!(f.surface, Surface::Quadric(_)))
            .unwrap();
        assert_eq!(wall.name.feature, a.mesh().faces()[0].name.feature);
        let topology = r.topology();
        for operand in [a, b] {
            for face in operand.mesh().faces() {
                if matches!(face.surface, Surface::Quadric(_)) {
                    let near = DVec3::new(1.0, 0.0, 0.5);
                    assert!(topology.face(&r, &face.name.key(), near).is_ok());
                }
            }
        }
    }
    assert_deterministic(|| run(&low, &high, Op::Union).into_mesh());
}

#[test]
fn stacked_cylinders_a_step_apart_keep_the_step() {
    // Radii 1 and 1.001: two walls, and the step drawn.
    let low = Solid::cylinder(DVec3::ZERO, 1.0, 1.0, 1, &TOL).unwrap();
    let high = Solid::cylinder(DVec3::Z, 1.001, 1.0, 2, &TOL).unwrap();
    let whole = Solid::cylinder(DVec3::ZERO, 1.0, 2.0, 1, &TOL).unwrap();
    let r = run(&low, &high, Op::Union);
    let want = PI * (1.0 + 1.001 * 1.001);
    assert!((r.volume() - want).abs() < 1e-9, "{}", r.volume());
    assert_eq!(quadric_faces(&r), 2);
    assert!(feature_edges(&r) > feature_edges(&whole));
}
