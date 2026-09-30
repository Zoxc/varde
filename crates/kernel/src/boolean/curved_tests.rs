//! Booleans of solids with curved patches: cylinders and boxes, whose
//! cuts are exact, and crossing cylinders, a free surface and a saddle,
//! whose cuts are traced and fitted.

use std::f64::consts::PI;

use glam::{DMat3, DQuat, DVec2, DVec3};

use super::pairs::tests::{cylinder_x, poke, reach, saddle};
use super::*;
use crate::mesh::tests::TOL;
use crate::mesh::{Quadric, Surface, samples};
use crate::par::assert_deterministic;
use crate::profile::tests::{circle, rect};
use crate::{Frame, Loop, Profile, extrude};

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
    Solid::new(builder.build().unwrap(), &TOL).unwrap()
}

fn run(a: &Solid, b: &Solid, op: Op) -> Solid {
    let solid = boolean(a, b, op, &TOL, &Budget::DEFAULT)
        .unwrap_or_else(|e| panic!("{op:?} failed: {e:?}"));
    solid.mesh().check_faces(&TOL).unwrap();
    solid
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
            solid.mesh().check_faces(&TOL).unwrap();
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
        solid.mesh().check_faces(&TOL).unwrap();
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
