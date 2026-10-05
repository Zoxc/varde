//! Flush unions whose caps meet along curves in one plane: a cap kept
//! whole by the perturbation, the other cut along its curved rim, leaves a
//! curve between two patches in one plane (a seam), which repair used to
//! split down to flat pieces: tens of thousands of patches, or a fold.

use std::collections::BTreeSet;

use super::*;
use crate::Display;
use crate::mesh::FaceName;

/// The area two discs of radii `r1` and `r2`, centres `d` apart, share
/// (they overlap without one holding the other).
fn lens(r1: f64, r2: f64, d: f64) -> f64 {
    r1 * r1 * ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1)).acos()
        + r2 * r2 * ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2)).acos()
        - 0.5 * ((-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2)).sqrt()
}

/// The size of `solid`: its box's diagonal.
fn size(solid: &Solid) -> f64 {
    let b = solid.bounds3().expect("not empty");
    b.max.distance(b.min)
}

/// `a ∪ b`, checked: of volume `want` (to `1e-12` relative), every patch on
/// its face's surface, fewer than `patches` patches.
fn flush_union(name: &str, a: &Solid, b: &Solid, want: f64, patches: usize) -> Solid {
    let union = run(a, b, Op::Union);
    let n = union.mesh().tris().len();
    assert!(n < patches, "{name}: {n} patches");
    let got = union.volume();
    assert!(
        (got - want).abs() <= 1e-12 * want,
        "{name}: volume {got}, not {want}"
    );
    exact(name, &union, size(&union));
    union
}

/// The names of the faces of `solid`'s patches lying in the plane at
/// `height` along `frame`'s normal.
fn names_in(solid: &Solid, frame: &Frame, height: f64) -> BTreeSet<FaceName> {
    let n = frame.x.cross(frame.y);
    let d = n.dot(frame.origin) + height;
    let near = 1e-9 * size(solid);
    let mesh = solid.mesh();
    (0..mesh.tris().len())
        .filter(|&t| {
            let p = mesh.patch(t);
            p.p.iter().chain(&p.c).all(|q| (q.dot(n) - d).abs() <= near)
        })
        .map(|t| mesh.faces()[mesh.tris()[t].face as usize].name)
        .collect()
}

/// How many drawn feature segments of `solid` lie in its plane `z =
/// height` strictly inside the square `|x|, |y| < half` (by `1e-3`).
fn lines_inside(solid: &Solid, height: f64, half: f64) -> usize {
    let mesh = solid.tessellate(&Display::new(&TOL)).unwrap();
    let at = |i: u32| DVec3::from(mesh.positions()[i as usize].map(f64::from));
    mesh.polylines()
        .flat_map(|polyline| polyline.windows(2))
        .filter(|segment| {
            let [a, b] = [segment[0], segment[1]];
            let m = (at(a) + at(b)) / 2.0;
            (m.z - height).abs() < 1e-5 && m.x.abs() < half - 1e-3 && m.y.abs() < half - 1e-3
        })
        .count()
}

/// A frame turned about two axes, off the origin.
fn turned() -> Frame {
    let q = DQuat::from_rotation_x(0.37) * DQuat::from_rotation_y(-0.61);
    Frame {
        origin: DVec3::new(0.3, -0.2, 0.7),
        x: q * DVec3::X,
        y: q * DVec3::Y,
    }
}

#[test]
fn a_flush_boss_first_in_a_union() {
    // A boss standing in a plate over the same span, the boss first: its
    // caps were kept whole and the plate's cut along its rims, which
    // repair split down to 28 912 patches (12 with the plate first). Its
    // caps are now the plate's: one face each, no line drawn round the
    // boss's rim.
    let plate = extruded(
        vec![rect(DVec2::splat(-1.25), DVec2::splat(1.25), 0)],
        -0.25,
        0.25,
        1,
    );
    let boss = extruded(
        vec![circle(DVec2::new(0.25, 0.0), 0.5, 0, false)],
        -0.25,
        0.25,
        2,
    );
    let union = flush_union("boss first", &boss, &plate, 3.125, 64);
    for z in [-0.25, 0.25] {
        let names = names_in(&union, &Frame::XY, z);
        assert_eq!(names.len(), 1, "at {z}: {names:?}");
        assert_eq!(lines_inside(&union, z, 1.25), 0, "at {z}");
    }
    let plate_first = flush_union("plate first", &plate, &boss, 3.125, 13);
    assert_eq!(names_in(&plate_first, &Frame::XY, 0.25).len(), 1);
    // Flush with the plate's top only.
    let top = extruded(
        vec![circle(DVec2::new(0.25, 0.0), 0.5, 0, false)],
        0.0,
        0.25,
        2,
    );
    flush_union("top only", &top, &plate, 3.125, 48);
    assert_deterministic(|| run(&boss, &plate, Op::Union));
}

#[test]
fn flush_unions_at_millimetre_scale() {
    // Repair split along the seam down to the resolution, which doesn't
    // grow with the model: the 60 × 40 × 10 plate joined to an r8 boss
    // made first gave 115 024 patches, a 40 × 40 × 5 flange at the foot of
    // an r5 shaft made first 28 846.
    let plate = extruded(
        vec![rect(DVec2::new(-30.0, -20.0), DVec2::new(30.0, 20.0), 0)],
        0.0,
        10.0,
        1,
    );
    let boss = extruded(vec![circle(DVec2::ZERO, 8.0, 0, false)], 0.0, 10.0, 2);
    flush_union("plate on a boss", &boss, &plate, 24_000.0, 100);
    let shaft = extruded(vec![circle(DVec2::ZERO, 5.0, 0, false)], 0.0, 50.0, 1);
    let flange = extruded(
        vec![rect(DVec2::splat(-20.0), DVec2::splat(20.0), 0)],
        0.0,
        5.0,
        2,
    );
    let want = 1600.0 * 5.0 + PI * 25.0 * 45.0;
    let union = flush_union("flange on a shaft", &shaft, &flange, want, 100);
    assert_eq!(names_in(&union, &Frame::XY, 0.0).len(), 1);
    flush_union("shaft on a flange", &flange, &shaft, want, 100);
}

#[test]
fn overlapping_flush_bosses_unite_in_either_order() {
    // Two bosses of one height overlapping: each one's caps cut along the
    // other's rim, whichever goes first, 9 422 to 47 566 patches (and at
    // millimetre scale `Invalid(Fold)`, before the coaxial flush work).
    // The lens they share counts once.
    for (r1, r2, c, h) in [
        (0.5, 0.4, DVec2::new(0.6, 0.1), 0.5),
        (10.0, 8.0, DVec2::new(12.0, 2.0), 10.0),
    ] {
        let a = extruded(vec![circle(DVec2::ZERO, r1, 0, false)], 0.0, h, 1);
        let b = extruded(vec![circle(c, r2, 0, false)], 0.0, h, 2);
        let want = h * (PI * (r1 * r1 + r2 * r2) - lens(r1, r2, c.length()));
        for (x, y) in [(&a, &b), (&b, &a)] {
            let union = flush_union(&format!("bosses r {r1}, {r2}"), x, y, want, 250);
            assert_eq!(names_in(&union, &Frame::XY, h).len(), 1);
            assert_eq!(names_in(&union, &Frame::XY, 0.0).len(), 1);
        }
    }
}

#[test]
fn flush_bosses_over_a_hole_and_an_edge() {
    // A boss half over a plate's hole (both orders, 18 878 and 17 788
    // patches), and one running over the plate's edge, the boss first
    // (11 086).
    let square = || rect(DVec2::splat(-1.25), DVec2::splat(1.25), 0);
    let drilled = extruded(
        vec![square(), circle(DVec2::ZERO, 0.3, 4, true)],
        -0.25,
        0.25,
        1,
    );
    let boss = extruded(
        vec![circle(DVec2::new(0.35, 0.0), 0.3, 0, false)],
        -0.25,
        0.25,
        2,
    );
    let want = 0.5 * (6.25 - PI * 0.09 + lens(0.3, 0.3, 0.35));
    flush_union("over the hole", &drilled, &boss, want, 300);
    flush_union("over the hole, boss first", &boss, &drilled, want, 300);
    let plate = extruded(vec![square()], -0.25, 0.25, 1);
    let boss = extruded(
        vec![circle(DVec2::new(1.2, 0.0), 0.3, 0, false)],
        -0.25,
        0.25,
        2,
    );
    // The part of the boss past `x = 1.25`: a circular segment.
    let (r, d) = (0.3f64, 0.05f64);
    let past = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    flush_union("over the edge", &boss, &plate, 0.5 * (6.25 + past), 80);
}

#[test]
fn a_cylinder_and_a_box_tangent_to_it_make_a_slot() {
    // A box whose sides run on from the cylinder's at a tangent, both over
    // one span, the cylinder first: the rim's arcs between the tangent
    // points are seams in the box's caps. It failed as `Invalid(Fold)`.
    let cylinder = extruded(vec![circle(DVec2::ZERO, 5.0, 0, false)], 0.0, 5.0, 1);
    let bar = extruded(
        vec![rect(DVec2::new(0.0, -5.0), DVec2::new(20.0, 5.0), 0)],
        0.0,
        5.0,
        2,
    );
    flush_union("slot", &cylinder, &bar, 5.0 * (200.0 + 12.5 * PI), 120);
}

#[test]
fn a_cylinder_inside_a_larger_one_over_one_span() {
    // The smaller first: 55 314 patches.
    let big = extruded(vec![circle(DVec2::ZERO, 1.0, 0, false)], -0.25, 0.25, 1);
    let small = extruded(
        vec![circle(DVec2::new(0.2, 0.1), 0.5, 0, false)],
        -0.25,
        0.25,
        2,
    );
    let union = flush_union("inside", &small, &big, 0.5 * PI, 150);
    assert_eq!(names_in(&union, &Frame::XY, 0.25).len(), 1);
}

#[test]
fn flush_bosses_on_a_turned_frame() {
    // Off every axis: the caps' plane is tested within the clean-up's
    // short length, not by coordinates.
    let frame = turned();
    let plate = extruded_on(
        vec![rect(DVec2::splat(-1.25), DVec2::splat(1.25), 0)],
        frame,
        -0.25,
        0.25,
        1,
    );
    let boss = extruded_on(
        vec![circle(DVec2::new(0.25, 0.0), 0.5, 0, false)],
        frame,
        -0.25,
        0.25,
        2,
    );
    let union = flush_union("turned", &boss, &plate, 3.125, 100);
    assert_eq!(names_in(&union, &frame, 0.25).len(), 1);
    let (r1, r2, c) = (0.5, 0.4, DVec2::new(0.6, 0.1));
    let a = extruded_on(vec![circle(DVec2::ZERO, r1, 0, false)], frame, 0.0, 0.5, 1);
    let b = extruded_on(vec![circle(c, r2, 0, false)], frame, 0.0, 0.5, 2);
    let want = 0.5 * (PI * (r1 * r1 + r2 * r2) - lens(r1, r2, c.length()));
    flush_union("turned bosses", &a, &b, want, 250);
    flush_union("turned bosses, other way", &b, &a, want, 250);
}

#[test]
fn a_pin_filling_its_hole_stays_light() {
    // The pin over the plate's span, united with it: the curves between
    // its caps and the plate's flipped away or straightened.
    let pin = extruded(
        vec![circle(DVec2::new(0.5, 0.2), 1.0, 0, false)],
        0.0,
        1.0,
        7,
    );
    flush_union("pin", &plate(), &pin, 24.0, 37);
}

#[test]
fn a_chain_of_flush_joins() {
    // Joins as the app makes them, the body first, every boss from the one
    // sketch plane: a drilled plate, a boss through it filling one hole
    // (its bottom flush), one flush with both its caps, one standing on
    // it, and one through it whose top is flush with the last one's. Each
    // step's volume exactly, the plate's caps one face each, the two
    // flush tops one between them.
    let (r1, c1) = (8.0, DVec2::new(-15.0, 0.0));
    let (r2, c2) = (6.0, DVec2::new(-5.0, 2.0));
    let (r3, c3) = (5.0, DVec2::new(4.0, -1.0));
    let (r4, c4) = (4.0, DVec2::new(11.0, 0.0));
    let (filled, open) = ((DVec2::new(-15.0, 1.0), 3.0), (DVec2::new(20.0, 12.0), 3.0));
    let plate_layer = 10.0 * (2400.0 - PI * open.1 * open.1);
    let steps = [
        plate_layer + 10.0 * PI * r1 * r1,
        plate_layer + 10.0 * PI * r1 * r1,
        plate_layer + 10.0 * PI * (r1 * r1 + r3 * r3),
        plate_layer + 10.0 * (PI * (r1 * r1 + r3 * r3 + r4 * r4) - lens(r3, r4, c3.distance(c4))),
    ];
    for frame in [Frame::XY, turned()] {
        let plate = extruded_on(
            vec![
                rect(DVec2::new(-30.0, -20.0), DVec2::new(30.0, 20.0), 0),
                circle(filled.0, filled.1, 4, true),
                circle(open.0, open.1, 5, true),
            ],
            frame,
            0.0,
            10.0,
            1,
        );
        let bosses = [
            extruded_on(vec![circle(c1, r1, 0, false)], frame, 0.0, 20.0, 2),
            extruded_on(vec![circle(c2, r2, 0, false)], frame, 0.0, 10.0, 3),
            extruded_on(vec![circle(c3, r3, 0, false)], frame, 10.0, 20.0, 4),
            extruded_on(vec![circle(c4, r4, 0, false)], frame, 0.0, 20.0, 5),
        ];
        let mut body = plate;
        for (k, (boss, want)) in bosses.iter().zip(steps).enumerate() {
            let name = format!("step {k} on {:?}", frame.origin);
            body = if k == 3 {
                assert_deterministic(|| flush_union(&name, &body, boss, want, 400))
            } else {
                flush_union(&name, &body, boss, want, 400)
            };
            for z in [0.0, 10.0] {
                assert_eq!(names_in(&body, &frame, z).len(), 1, "{name} at {z}");
            }
        }
        // The first boss's top and the merged tops of the last two.
        assert_eq!(names_in(&body, &frame, 20.0).len(), 2);
    }
}

#[test]
fn caps_a_hair_apart_take_the_region_fallback() {
    // A boss whose top is a hair above the plate's (closer than the
    // clean-up's short length, an eighth of the resolution): the clean-up
    // takes the two caps as one plane, and the region fallback
    // triangulates the rim's clusters again. Right within that hair times
    // the plate's top, in either order and on a turned frame.
    let hair = 2e-8;
    // Each union spends over three million units, some 0.3 s in a debug
    // build: quick runs take the turned frame only.
    let frames = if varde_testing::full() {
        &[Frame::XY, turned()][..]
    } else {
        &[turned()][..]
    };
    for &frame in frames {
        let plate = extruded_on(
            vec![rect(DVec2::splat(-1.25), DVec2::splat(1.25), 0)],
            frame,
            -0.25,
            0.25,
            1,
        );
        let boss = extruded_on(
            vec![circle(DVec2::new(0.25, 0.0), 0.5, 0, false)],
            frame,
            -0.25,
            0.25 + hair,
            2,
        );
        let want = 3.125 + PI * 0.25 * hair;
        for (k, (a, b)) in [(&boss, &plate), (&plate, &boss)].into_iter().enumerate() {
            // Whether the clean-up dissolved anything, counted on the
            // thread the union runs on.
            let union = || {
                let before = super::super::cleanup::DISSOLVED.get();
                let union = run(a, b, Op::Union);
                (super::super::cleanup::DISSOLVED.get() > before, union)
            };
            // The boss first the same on 1 and 8 threads.
            let (dissolved, union) = if k == 0 {
                assert_deterministic(union)
            } else {
                union()
            };
            assert!(dissolved);
            assert!(union.mesh().tris().len() < 100);
            let got = union.volume();
            assert!((got - want).abs() <= 6.25 * hair, "{got} not {want}");
            exact_to("hair", &union, size(&union), hair);
        }
    }
}

#[test]
fn bosses_on_a_rounded_corner() {
    // A boss over a rounded rectangle's corner arc, inside it, or round
    // the arc's centre: all four operations either way round, by their
    // identities (and exact values where the boss lies inside).
    let h = 0.5;
    let block = extruded(
        vec![super::super::seeded_tests::rounded(
            DVec2::new(-1.0, -0.5),
            DVec2::new(0.0, 0.5),
            0.2,
            0,
        )],
        0.0,
        h,
        1,
    );
    let vb = (1.0 - (4.0 - PI) * 0.04) * h;
    for (c, r, inside) in [
        (DVec2::new(0.0, 0.5), 0.3, false),
        (DVec2::new(-0.1, 0.4), 0.25, false),
        (DVec2::new(-0.2, 0.3), 0.1, true),
    ] {
        let boss = extruded(vec![circle(c, r, 7, false)], 0.0, h, 2);
        let disc = PI * r * r * h;
        let results = all_four(&block, &boss, 1e-12);
        let both = results[1].volume();
        if inside {
            assert!((both - disc).abs() <= 1e-12);
        }
        volumes("rounded corner", &block, &boss, &results, both, 1e-12);
        let swapped = run(&boss, &block, Op::Union);
        assert!((swapped.volume() - (vb + disc - both)).abs() <= 1e-12);
    }
}

/// A conic of a profile: `p0`, its control point, its weight and `p1`
/// (as `x, y` each), and its curve.
type Piece = (f64, f64, f64, f64, f64, f64, f64, u64);

/// A loop of conics given as [`Piece`]s.
fn conics(segments: &[Piece]) -> Loop {
    Loop {
        segments: segments
            .iter()
            .map(|&(x0, y0, cx, cy, w, x1, y1, curve)| Segment {
                conic: crate::patch::Conic2::new(
                    DVec2::new(x0, y0),
                    DVec2::new(cx, cy),
                    w,
                    DVec2::new(x1, y1),
                )
                .unwrap(),
                curve,
            })
            .collect(),
    }
}

#[test]
fn a_cluster_at_a_rim_is_triangulated_again() {
    // A slot standing through a drilled plate, its foot flush with the
    // plate's bottom, on a turned frame (numbers from a random run, to the
    // bit), the slot first: at one of its arcs' joints four vertices at
    // one place, in triangles of zero size in the bottom's plane, that no
    // collapse could take (each would leave a vertex two fans) and no
    // straightening mended. That region is triangulated again from its
    // boundary; without it the union failed as `Invalid(Fold)`.
    let frame = Frame {
        origin: DVec3::new(
            0.05652284280473905,
            0.02928126036141243,
            0.08336387150316037,
        ),
        x: DVec3::new(0.6031511254039772, -0.16352410610992357, 0.7806846909251302),
        y: DVec3::new(0.0, 0.9787591934004145, 0.20501327111718892),
    };
    let e = 0.30000000000000004;
    let square = conics(&[
        (-e, -e, 0.0, -e, 1.0, e, -e, 0),
        (e, -e, e, 0.0, 1.0, e, e, 1),
        (e, e, 0.0, e, 1.0, -e, e, 2),
        (-e, e, -e, 0.0, 1.0, -e, -e, 3),
    ]);
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let hole = conics(&[
        (
            0.010000000000000002,
            -0.08,
            0.009999999999999988,
            -0.13,
            r,
            -0.04,
            -0.13,
            10,
        ),
        (
            -0.04,
            -0.13,
            -0.08999999999999997,
            -0.12999999999999998,
            0.7071067811865477,
            -0.09,
            -0.08,
            10,
        ),
        (-0.09, -0.08, -0.09, -0.03, r, -0.04, -0.03, 10),
        (
            -0.04,
            -0.03,
            0.010000000000000002,
            -0.03,
            r,
            0.010000000000000002,
            -0.08,
            10,
        ),
    ]);
    let (y0, y1) = (0.030000000000000006, 0.11000000000000001);
    let slot = conics(&[
        (-0.19, y0, -0.155, y0, 1.0, -0.12, y0, 20),
        (
            -0.12,
            y0,
            -0.08000000000000002,
            0.03000000000000002,
            0.7071067811865477,
            -0.07999999999999999,
            0.07,
            21,
        ),
        (
            -0.07999999999999999,
            0.07,
            -0.07999999999999999,
            y1,
            r,
            -0.12,
            y1,
            21,
        ),
        (-0.12, y1, -0.155, y1, 1.0, -0.19, y1, 22),
        (
            -0.19,
            y1,
            -0.23,
            0.10999999999999999,
            0.7071067811865478,
            -0.23,
            0.07,
            23,
        ),
        (
            -0.23,
            0.07,
            -0.23,
            0.030000000000000027,
            0.7071067811865478,
            -0.19,
            y0,
            23,
        ),
    ]);
    let plate = extruded_on(vec![square, hole], frame, 0.0, 0.5, 1);
    let slot = extruded_on(vec![slot], frame, 0.0, 1.0, 2);
    // The slot's foot lies on the plate, clear of its hole: half of it
    // is inside.
    let foot = 0.07 * 0.08 + PI * 0.04 * 0.04;
    let want = 0.5 * (0.36 - PI * 0.05 * 0.05) + 0.5 * foot;
    let before = super::super::cleanup::DISSOLVED.get();
    let union = run(&slot, &plate, Op::Union);
    assert!(super::super::cleanup::DISSOLVED.get() > before);
    let got = union.volume();
    assert!((got - want).abs() <= 1e-9 * want, "{got} not {want}");
    exact("cluster", &union, size(&union));
    assert!(union.mesh().tris().len() < 250);
}
