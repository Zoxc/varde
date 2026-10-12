//! One surface, one face, after booleans: faces merged where they lie on
//! one surface and meet, and only there; on turned frames far from the
//! origin, through chains of joins and cuts, with every operand's names
//! still resolving.

use std::collections::BTreeSet;

use super::*;
use crate::Display;
use crate::mesh::{FacePart, off_surface};

/// The merge's bar.
fn small() -> f64 {
    TOL.resolution() / 8.0
}

/// Two counts that should both be 0. Missed merges: edges between faces
/// of different keys whose surfaces agree there as the merge measures
/// them (planes facing alike, quadrics' patches facing alike, each
/// triangle on the other's surface). Wrong merges: triangles claiming a
/// surface that lie farther than the resolution from the surface of
/// their region's first such triangle.
fn audit(solid: &Solid) -> (usize, usize) {
    let mesh = solid.mesh();
    let mut missed = 0;
    for h in 0..(mesh.tris().len() * 3) as u32 {
        let p = mesh.halfedge(h).pair;
        if p < h {
            continue;
        }
        let (t, s) = (h as usize / 3, p as usize / 3);
        let ff = mesh.faces()[mesh.tris()[t].face as usize];
        let fg = mesh.faces()[mesh.tris()[s].face as usize];
        if ff.name.key() == fg.name.key() {
            continue;
        }
        let crossed = off_surface(&mesh.patch(t), &fg.surface) <= small()
            && off_surface(&mesh.patch(s), &ff.surface) <= small();
        let middle = |i: usize| {
            let mut u = DVec3::ZERO;
            u[i] = 0.5;
            u[(i + 1) % 3] = 0.5;
            u
        };
        let same = match (ff.surface, fg.surface) {
            (Surface::Plane { n: a, .. }, Surface::Plane { n: b, .. }) => {
                a.normalize().dot(b.normalize()) > 1.0 - 1e-12
            }
            (Surface::Quadric(_), Surface::Quadric(_)) => {
                let nt = mesh.patch(t).normal(middle(h as usize % 3));
                let ns = mesh.patch(s).normal(middle(p as usize % 3));
                nt.dot(ns) > 0.0
            }
            _ => false,
        };
        if same && crossed {
            missed += 1;
        }
    }
    let mut wrong = 0;
    for region in solid.topology().regions() {
        let surface = |t: u32| mesh.faces()[mesh.tris()[t as usize].face as usize].surface;
        let fixed: Vec<u32> = region
            .tris
            .iter()
            .copied()
            .filter(|&t| !matches!(surface(t), Surface::Free))
            .collect();
        let Some(&first) = fixed.first() else {
            continue;
        };
        for &t in &fixed {
            if off_surface(&mesh.patch(t as usize), &surface(first)) > TOL.resolution() {
                wrong += 1;
            }
        }
    }
    (missed, wrong)
}

/// How many faces (keys) `solid` has.
fn keys(solid: &Solid) -> usize {
    let keys: BTreeSet<_> = solid.mesh().faces().iter().map(|f| f.name.key()).collect();
    keys.len()
}

/// How many feature edges `solid` draws.
fn lines(solid: &Solid) -> usize {
    solid
        .tessellate(&Display::new(&TOL))
        .unwrap()
        .edge_segments()
        .count()
}

fn cuboid(min: DVec3, size: DVec3, feature: u64) -> Solid {
    Solid::cuboid(min, size, feature, &TOL).unwrap()
}

#[test]
fn tops_at_a_crease_merge_only_within_the_bar() {
    // A unit box and one beside it whose top rises by `theta` a unit from
    // their shared edge: up to an eighth of the resolution over the unit
    // (`1e-7`) one top, a box's twelve lines; past it (`2e-7`) two tops
    // and the crease drawn. Both ways round.
    let a = cuboid(DVec3::ZERO, DVec3::ONE, 1);
    for (theta, merged) in [
        (0.0, true),
        (1e-8, true),
        (1e-7, true),
        (2e-7, false),
        (1e-5, false),
    ] {
        let pts = [
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(1.0, 1.0 + theta),
            DVec2::new(0.0, 1.0),
        ];
        // On the xz plane at x = 1, extruded back over 0 ≤ y ≤ 1.
        let frame = Frame {
            origin: DVec3::X,
            x: DVec3::X,
            y: DVec3::Z,
        };
        let b = extruded_on(vec![polygon(&pts, 0)], frame, -1.0, 0.0, 2);
        for (x, y) in [(&a, &b), (&b, &a)] {
            let r = run(x, y, Op::Union);
            let want = 2.0 + theta / 2.0;
            assert!((r.volume() - want).abs() < 1e-12, "{theta}: {}", r.volume());
            assert_eq!(keys(&r), if merged { 6 } else { 7 }, "{theta}");
            assert_eq!(lines(&r), if merged { 16 } else { 17 }, "{theta}");
            assert_eq!(audit(&r), (0, 0), "{theta}");
        }
    }
}

#[test]
fn flush_stacks_merge_on_turned_frames_far_from_the_origin() {
    // Boxes and cylinders stacked on frames turned at random about
    // `3e4` from the origin: as upright, one wall per surface, a box's
    // and a cylinder's lines.
    let mut rng = crate::test_rng::Rng::new(63);
    for _ in 0..4 {
        let q = DQuat::from_axis_angle(rng.direction(), rng.range(0.0, 6.0));
        let frame = Frame {
            origin: rng.point(1.0) + DVec3::new(3e4, -2e4, 1e4),
            x: q * DVec3::X,
            y: q * DVec3::Y,
        };
        let block = |from, to, f| {
            let outline = rect(DVec2::ZERO, DVec2::new(1.0, 2.0), 0);
            extruded_on(vec![outline], frame, from, to, f)
        };
        let r = run(&block(0.0, 1.0, 1), &block(1.0, 3.0, 2), Op::Union);
        assert!((r.volume() - 6.0).abs() < 1e-9, "{}", r.volume());
        assert_eq!((keys(&r), lines(&r), audit(&r)), (6, 16, (0, 0)));
        let rod = |from, to, f| {
            let outline = circle(DVec2::new(0.3, 0.1), 1.5, 0, false);
            extruded_on(vec![outline], frame, from, to, f)
        };
        let want = PI * 1.5 * 1.5 * 3.0;
        for (x, y) in [
            (rod(0.0, 1.0, 1), rod(1.0, 3.0, 2)),
            (rod(1.0, 3.0, 2), rod(0.0, 1.0, 1)),
        ] {
            let r = run(&x, &y, Op::Union);
            assert!((r.volume() - want).abs() < 1e-9, "{}", r.volume());
            assert_eq!((keys(&r), lines(&r), audit(&r)), (3, 80, (0, 0)));
        }
    }
}

#[test]
fn a_chain_of_joins_and_cuts_keeps_one_face_a_surface() {
    // Five unit boxes joined in a row, then a hole drilled across each
    // joint: one face a surface throughout (a box's lines, the long ones
    // in a piece a box), every box's top and front
    // still found by its own key near its own place, the same bits at 1
    // and 8 threads.
    let unit = |i: u64| cuboid(DVec3::new(i as f64, 0.0, 0.0), DVec3::ONE, 1 + i);
    let body = assert_deterministic(|| {
        let mut body = unit(0);
        for i in 1..5 {
            body = run(&body, &unit(i), Op::Union);
            assert!((body.volume() - (i + 1) as f64).abs() < 1e-12);
            assert_eq!(
                (keys(&body), lines(&body), audit(&body)),
                (6, 8 + 4 * (i as usize + 1), (0, 0))
            );
        }
        for i in 1..5 {
            let hole = Solid::cylinder(DVec3::new(i as f64, 0.5, -1.0), 0.2, 3.0, 10 + i, &TOL);
            body = run(&body, &hole.unwrap(), Op::Difference);
            let want = 5.0 - i as f64 * PI * 0.04;
            assert!((body.volume() - want).abs() < 1e-12, "{}", body.volume());
            assert_eq!((keys(&body), audit(&body)), (6 + i as usize, (0, 0)));
        }
        body.into_mesh()
    });
    let body = Solid::new(body, &TOL).unwrap();
    let topology = body.topology();
    for i in 0..5 {
        let x = i as f64 + 0.5;
        for (part, near) in [
            (FacePart::EndCap, DVec3::new(x, 0.9, 1.0)),
            (
                FacePart::Side {
                    curve: 0,
                    segment: 0,
                },
                DVec3::new(x, 0.0, 0.5),
            ),
        ] {
            let face = unit(i)
                .mesh()
                .faces()
                .iter()
                .find(|f| f.name.part == part)
                .copied();
            let key = face.unwrap().name.key();
            assert!(
                topology.face(&body, &key, near).is_ok(),
                "box {i}, {part:?}"
            );
        }
    }
}

#[test]
fn stacked_cylinders_notched_across_their_joints_keep_one_wall() {
    // Five unit cylinders stacked: drawn as one 0..5 cylinder, then
    // notched by a box across four joints.
    let mut body = Solid::cylinder(DVec3::ZERO, 1.0, 1.0, 1, &TOL).unwrap();
    for i in 1..5 {
        let next = Solid::cylinder(DVec3::new(0.0, 0.0, i as f64), 1.0, 1.0, 1 + i, &TOL);
        body = run(&body, &next.unwrap(), Op::Union);
    }
    let whole = Solid::cylinder(DVec3::ZERO, 1.0, 5.0, 1, &TOL).unwrap();
    assert!((body.volume() - 5.0 * PI).abs() < 1e-12);
    assert_eq!(
        (keys(&body), lines(&body), audit(&body)),
        (3, lines(&whole), (0, 0))
    );
    let notch = cuboid(DVec3::new(0.5, -0.25, 0.5), DVec3::new(1.0, 0.5, 4.0), 20);
    let notched = run(&body, &notch, Op::Difference);
    // The notch takes the part of the disc past x = 0.5 with |y| ≤ 0.25.
    let primitive = |y: f64| 0.5 * (y * (1.0 - y * y).sqrt() + y.asin()) - 0.5 * y;
    let cut = 4.0 * 2.0 * primitive(0.25);
    assert!(
        (notched.volume() - (5.0 * PI - cut)).abs() < 1e-9,
        "{}",
        notched.volume()
    );
    let walls: BTreeSet<_> = notched
        .mesh()
        .faces()
        .iter()
        .filter(|f| matches!(f.surface, Surface::Quadric(_)))
        .map(|f| f.name.key())
        .collect();
    assert_eq!(walls.len(), 1);
    assert_eq!(audit(&notched), (0, 0));
}

#[test]
fn faces_meeting_only_at_a_corner_stay_apart() {
    // A U: a bar with a block on each end. The blocks' tops are one
    // plane but meet nowhere: three tops, the bar's between them; the
    // blocks' outer ends merge with the bar's, their fronts and backs
    // with its front and back.
    let bar = cuboid(DVec3::ZERO, DVec3::new(3.0, 1.0, 1.0), 1);
    let left = cuboid(DVec3::new(0.0, 0.0, 1.0), DVec3::ONE, 2);
    let right = cuboid(DVec3::new(2.0, 0.0, 1.0), DVec3::ONE, 3);
    let u = run(&run(&bar, &left, Op::Union), &right, Op::Union);
    assert!((u.volume() - 5.0).abs() < 1e-12);
    let tops: BTreeSet<_> = u
        .mesh()
        .faces()
        .iter()
        .filter(|f| matches!(f.surface, Surface::Plane { n, .. } if n.normalize().z > 0.9))
        .map(|f| f.name.key())
        .collect();
    assert_eq!(tops.len(), 3);
    // Bottom, front, back, two ends, three tops, two inner sides.
    assert_eq!((keys(&u), audit(&u)), (10, (0, 0)));
}
