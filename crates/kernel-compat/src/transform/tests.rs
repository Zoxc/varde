use std::f64::consts::{FRAC_1_SQRT_2, PI};

use glam::{DVec2, DVec3};

use super::*;
use crate::Stripped;
use crate::extrude::Frame;
use crate::mesh::{FacePart, Surface};
use crate::par::assert_deterministic;
use crate::patch::{Conic2, PatchError};
use crate::profile::tests::circle;
use crate::profile::{Loop, Profile, Segment};
use crate::{Budget, Topology, extrude};

const TOL: Tolerance = Tolerance::DEFAULT;

fn transformed(s: &Solid, m: &Motion, copy: Option<Instance>) -> Solid {
    s.transformed(m, copy, &TOL, &Budget::DEFAULT).unwrap()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

/// An L of a 4 × 1 bar and a 1 × 3 upright, with a round hole of radius
/// 0.3 through the bar and a half-ellipse (semi-axes 1.5 and 1) on the
/// upright's top, overhanging to the left, extruded 2: planes, a cylinder
/// and a conic cylinder, nothing symmetric. With its volume.
fn part() -> (Solid, f64) {
    let p = DVec2::new;
    let conic = |a, c, b| Segment {
        conic: Conic2::new(a, c, FRAC_1_SQRT_2, b).unwrap(),
        curve: 20,
    };
    let outline = Loop {
        segments: vec![
            Segment::line(p(0.0, 0.0), p(4.0, 0.0), 1).unwrap(),
            Segment::line(p(4.0, 0.0), p(4.0, 1.0), 2).unwrap(),
            Segment::line(p(4.0, 1.0), p(1.0, 1.0), 3).unwrap(),
            Segment::line(p(1.0, 1.0), p(1.0, 4.0), 4).unwrap(),
            // The half-ellipse over the upright's top, from (1, 4) to (-2, 4)
            // round (-0.5, 4).
            conic(p(1.0, 4.0), p(1.0, 5.0), p(-0.5, 5.0)),
            conic(p(-0.5, 5.0), p(-2.0, 5.0), p(-2.0, 4.0)),
            Segment::line(p(-2.0, 4.0), p(0.0, 4.0), 5).unwrap(),
            Segment::line(p(0.0, 4.0), p(0.0, 0.0), 6).unwrap(),
        ],
    };
    let hole = circle(p(2.5, 0.5), 0.3, 7, true);
    let profile = Profile {
        loops: vec![outline, hole],
    };
    let solid = extrude(&profile, &Frame::XY, 0.0, 2.0, 9, &TOL, &Budget::DEFAULT).unwrap();
    let volume = (4.0 + 3.0 + PI * 1.5 * 0.5 - PI * 0.09) * 2.0;
    (solid, volume)
}

/// Every planar face's form points out of the solid: along the patches'
/// outward normals.
fn forms_face_out(solid: &Solid) {
    let mesh = solid.mesh();
    for t in 0..mesh.tris().len() {
        let face = mesh.faces()[mesh.tris()[t].face as usize];
        if let Form::Plane { n, .. } = face.form {
            let normal = mesh.patch(t).normal(DVec3::splat(1.0 / 3.0));
            assert!(n.dot(normal) > 0.0, "triangle {t}: {n} against {normal}");
            if let Surface::Plane { n: tag, .. } = face.surface {
                assert!(tag.dot(normal) > 0.0, "triangle {t}'s tag");
            }
        }
    }
}

#[test]
fn the_part_is_what_it_says() {
    let (solid, volume) = part();
    assert!(close(solid.volume(), volume, 1e-12), "{}", solid.volume());
    let forms = solid.mesh().faces().iter().map(|f| f.form);
    assert!(forms.clone().any(|f| matches!(f, Form::Cylinder { .. })));
    assert!(
        forms
            .clone()
            .any(|f| matches!(f, Form::ConicCylinder { .. }))
    );
    forms_face_out(&solid);
}

#[test]
fn a_move_keeps_names_and_shifts_every_point() {
    let (solid, volume) = part();
    let offset = DVec3::new(10.0, -3.5, 0.25);
    let moved = transformed(&solid, &Motion::translation(offset).unwrap(), None);
    assert!(close(moved.volume(), volume, 1e-12));
    for (a, b) in solid.mesh().verts().iter().zip(moved.mesh().verts()) {
        assert_eq!(*a + offset, *b);
    }
    let names = |s: &Solid| s.mesh().faces().iter().map(|f| f.name).collect::<Vec<_>>();
    assert_eq!(names(&solid), names(&moved));
    moved.mesh().check_faces(&TOL).unwrap();
    forms_face_out(&moved);
}

#[test]
fn quarter_turns_are_exact() {
    let (solid, volume) = part();
    for axis in [DVec3::X, DVec3::Y, DVec3::Z, DVec3::NEG_Z * 3.0] {
        let quarter = Motion::turn(DVec3::ZERO, axis, 90.0).unwrap();
        let mut turned = solid.clone();
        for _ in 0..4 {
            turned = transformed(&turned, &quarter, None);
            assert!(close(turned.volume(), volume, 1e-12));
            forms_face_out(&turned);
        }
        // Four quarter turns are the solid again, to the bit.
        assert_eq!(turned, solid, "about {axis}");
        // A quarter turn about z maps (x, y, z) to (−y, x, z) exactly.
        if axis == DVec3::Z {
            let once = transformed(&solid, &quarter, None);
            for (a, b) in solid.mesh().verts().iter().zip(once.mesh().verts()) {
                assert_eq!(DVec3::new(-a.y, a.x, a.z), *b);
            }
        }
    }
    // Multiples of 90° of either sign and past a turn.
    for (degrees, sin, cos) in [
        (0.0, 0.0, 1.0),
        (90.0, 1.0, 0.0),
        (180.0, 0.0, -1.0),
        (-90.0, -1.0, 0.0),
        (270.0, -1.0, 0.0),
        (-270.0, 1.0, 0.0),
        (450.0, 1.0, 0.0),
        (-180.0, 0.0, -1.0),
        (720.0, 0.0, 1.0),
    ] {
        assert_eq!(sin_cos_degrees(degrees), (sin, cos), "{degrees}");
    }
}

#[test]
fn turns_about_tilted_lines_keep_volumes_and_tags() {
    let (solid, volume) = part();
    for (point, axis, degrees) in [
        (DVec3::new(1.0, 2.0, 3.0), DVec3::new(1.0, 1.0, 1.0), 30.0),
        (
            DVec3::new(-5.0, 0.0, 0.5),
            DVec3::new(0.2, -1.0, 0.7),
            137.25,
        ),
        (DVec3::ZERO, DVec3::Y, -12.5),
    ] {
        let turn = Motion::turn(point, axis, degrees).unwrap();
        let turned = transformed(&solid, &turn, None);
        assert!(close(turned.volume(), volume, 1e-12), "{}", turned.volume());
        assert!(close(turned.area(), solid.area(), 1e-12));
        turned.mesh().check_faces(&TOL).unwrap();
        forms_face_out(&turned);
        // The point on the axis stays; the turn back is the solid to
        // rounding.
        assert!((turn.point(point) - point).length() < 1e-14 * 8.0);
        let back = Motion::turn(point, axis, -degrees).unwrap();
        let again = transformed(&turned, &back, None);
        for (a, b) in solid.mesh().verts().iter().zip(again.mesh().verts()) {
            assert!((*a - *b).length() < 1e-13, "{a} {b}");
        }
    }
}

#[test]
fn mirrors_reverse_triangles_and_face_out() {
    let (solid, volume) = part();
    for (point, normal) in [
        (DVec3::ZERO, DVec3::X),
        (DVec3::new(0.0, 0.0, 2.0), DVec3::Z),
        (DVec3::new(3.0, -1.0, 0.0), DVec3::new(1.0, 2.0, -0.5)),
    ] {
        let mirror = Motion::mirror(point, normal).unwrap();
        assert!(mirror.mirrors());
        let image = transformed(&solid, &mirror, None);
        // Positive: it faces out (`check` passed its orientation too).
        assert!(close(image.volume(), volume, 1e-12), "{}", image.volume());
        image.mesh().check_faces(&TOL).unwrap();
        forms_face_out(&image);
        // Mirrored twice is the solid again: the same triangles, corners
        // in the same order.
        let twice = transformed(&image, &mirror, None);
        assert_eq!(twice.mesh().tris(), solid.mesh().tris());
        if normal == DVec3::X {
            assert_eq!(twice, solid);
        }
    }
    // A mirror then a mirror is a turn: not inside out.
    let a = Motion::mirror(DVec3::ZERO, DVec3::X).unwrap();
    let b = Motion::mirror(DVec3::ZERO, DVec3::Y).unwrap();
    let both = a.then(&b);
    assert!(!both.mirrors());
    let half = Motion::turn(DVec3::ZERO, DVec3::Z, 180.0).unwrap();
    let (solid, _) = part();
    assert_eq!(
        transformed(&solid, &both, None),
        transformed(&solid, &half, None)
    );
}

#[test]
fn mirrors_in_planes_square_to_an_axis_are_exact_for_any_normal_length() {
    // Normals of any length along an axis: the coordinate along it
    // negated, about the plane's (exact for 0 and halves of powers of
    // two, so `x ↦ 2·p − x` takes one rounding at most, none here).
    let (solid, volume) = part();
    for (scale, along) in [(3.0, 2), (0.1, 0), (-7.0, 1), (1e-300, 2), (1e300, 0)] {
        let mut normal = DVec3::ZERO;
        normal[along] = scale;
        let mirror = Motion::mirror(DVec3::ZERO, normal).unwrap();
        let image = transformed(&solid, &mirror, None);
        assert!(close(image.volume(), volume, 1e-12));
        for (a, b) in solid.mesh().verts().iter().zip(image.mesh().verts()) {
            let mut want = *a;
            want[along] = -want[along];
            assert_eq!(*b, want, "{scale} along {along}");
        }
        let through = Motion::mirror(DVec3::splat(2.0), normal).unwrap();
        for p in solid.mesh().verts() {
            let mut want = *p;
            want[along] = 4.0 - want[along];
            assert_eq!(through.point(*p), want);
        }
    }
    // A slanted normal of any length is the same mirror, to rounding.
    let unit = Motion::mirror(DVec3::X, DVec3::new(1.0, 2.0, -0.5)).unwrap();
    for scale in [1e-300, 3.0, 1e300] {
        let mirror = Motion::mirror(DVec3::X, DVec3::new(1.0, 2.0, -0.5) * scale).unwrap();
        for p in solid.mesh().verts() {
            assert!((mirror.point(*p) - unit.point(*p)).length() < 1e-14 * 8.0);
        }
    }
}

#[test]
fn copies_are_named_by_instance() {
    let (solid, _) = part();
    let copy = Instance {
        feature: 40,
        index: 3,
    };
    let moved = Motion::translation(DVec3::X * 20.0).unwrap();
    let image = transformed(&solid, &moved, Some(copy));
    for (a, b) in solid.mesh().faces().iter().zip(image.mesh().faces()) {
        assert_eq!(b.name, a.name.copy(40, 3));
        assert_eq!(b.name.key(), a.name.key().copy(40, 3));
        assert_ne!(b.name.key(), a.name.key());
    }
    // A copy of the copy is another instance again.
    let twice = transformed(&image, &moved, Some(copy));
    assert_ne!(twice.mesh().faces()[0].name, image.mesh().faces()[0].name);
}

#[test]
fn aliases_are_carried_and_renamed() {
    // Two flush boxes joined: the merged faces keep the absorbed keys as
    // aliases.
    let a = Solid::cuboid(DVec3::ZERO, DVec3::splat(1.0), 1, &TOL).unwrap();
    let b = Solid::cuboid(DVec3::new(1.0, 0.0, 0.0), DVec3::splat(1.0), 2, &TOL).unwrap();
    let joined = crate::boolean(&a, &b, Op::Union, &TOL, &Budget::DEFAULT).unwrap();
    assert!(!joined.mesh().aliases().is_empty());
    let copy = Instance {
        feature: 5,
        index: 1,
    };
    let mirror = Motion::mirror(DVec3::new(0.0, 0.0, -1.0), DVec3::Z).unwrap();
    let image = transformed(&joined, &mirror, Some(copy));
    let want: Vec<_> = joined
        .mesh()
        .aliases()
        .iter()
        .map(|&(f, key)| (f, key.copy(5, 1)))
        .collect();
    let mut want = want;
    want.sort_unstable();
    assert_eq!(image.mesh().aliases(), want);
    // The topology resolves the copied keys.
    let topology = Topology::new(&image);
    for &(_, key) in image.mesh().aliases() {
        assert!(topology.face(&image, &key, DVec3::ZERO).is_ok());
    }
}

#[test]
fn a_mirrored_copy_assembles_with_its_source() {
    let (solid, volume) = part();
    // Apart: a mirror in x = −3 puts the copy 2 clear of the upright's top.
    let mirror = Motion::mirror(DVec3::new(-3.0, 0.0, 0.0), DVec3::X).unwrap();
    let copy = Instance {
        feature: 30,
        index: 1,
    };
    let image = transformed(&solid, &mirror, Some(copy));
    let both = assemble(&[solid.clone(), image.clone()], &TOL, &Budget::DEFAULT).unwrap();
    assert!(close(both.volume(), 2.0 * volume, 1e-12));
    // Side by side, no boolean: the two meshes as they were.
    assert_eq!(
        both.mesh().tris().len(),
        solid.mesh().tris().len() + image.mesh().tris().len()
    );
    assert_eq!(
        &both.mesh().verts()[..solid.mesh().verts().len()],
        solid.mesh().verts()
    );
    // Flush: a mirror in the bar's end x = 4 makes one solid, unioned.
    let mirror = Motion::mirror(DVec3::new(4.0, 0.0, 0.0), DVec3::X).unwrap();
    let image = transformed(&solid, &mirror, Some(copy));
    let joined = assemble(&[solid.clone(), image], &TOL, &Budget::DEFAULT).unwrap();
    assert!(
        close(joined.volume(), 2.0 * volume, 1e-12),
        "{}",
        joined.volume()
    );
    joined.mesh().check_faces(&TOL).unwrap();
    // Overlapping: in x = 3, the bars overlap by 1 × 1 × 2 (less the
    // hole's overlap, a disc of 0.3 at 2.5 and its image at 3.5, apart).
    let mirror = Motion::mirror(DVec3::new(3.0, 0.0, 0.0), DVec3::X).unwrap();
    let image = transformed(&solid, &mirror, Some(copy));
    let joined = assemble(&[solid.clone(), image], &TOL, &Budget::DEFAULT).unwrap();
    // The bars overlap in x from 2 to 4 (2 × 1 × 2), less the two holes,
    // each wholly in it and a hole of one of them only.
    let both_bars = 2.0 * 1.0 * 2.0 - 2.0 * (PI * 0.09 * 2.0);
    let want = 2.0 * volume - both_bars;
    assert!(
        close(joined.volume(), want, 1e-9),
        "{} {want}",
        joined.volume()
    );
}

#[test]
fn a_hundred_copies_assemble_in_linear_work() {
    let pin = Solid::cylinder(DVec3::ZERO, 0.4, 3.0, 1, &TOL).unwrap();
    let volume = pin.volume();
    let copies = |n: u32| -> Vec<Solid> {
        (0..n)
            .map(|k| {
                let place = Motion::pattern_step(DVec3::X, 1.0, k % 10)
                    .unwrap()
                    .then(&Motion::pattern_step(DVec3::Y, 1.0, k / 10).unwrap());
                let copy = Instance {
                    feature: 50,
                    index: u64::from(k),
                };
                transformed(&pin, &place, Some(copy))
            })
            .collect()
    };
    let patches = pin.mesh().tris().len() as u64;
    let hundred = copies(100);
    // Charged at most a fixed number of units a patch: no pairs of parts,
    // no boolean.
    let budget = Budget::new(100 * patches * (TRANSFORM_WORK as u64 + 32 + 2) + 100);
    let start = std::time::Instant::now();
    let all = assemble(&hundred, &TOL, &budget).unwrap();
    let t100 = start.elapsed();
    assert!(close(all.volume(), 100.0 * volume, 1e-12));
    assert_eq!(all.mesh().tris().len() as u64, 100 * patches);
    let quarter = copies(25);
    let start = std::time::Instant::now();
    assemble(&quarter, &TOL, &budget).unwrap();
    let t25 = start.elapsed();
    eprintln!("assembled 25 copies in {t25:?}, 100 in {t100:?}");
}

#[test]
fn touching_copies_are_unioned() {
    // A row of five unit cubes, each flush with the next: one box of five
    // in the end, its faces merged.
    let cube = Solid::cuboid(DVec3::ZERO, DVec3::splat(1.0), 1, &TOL).unwrap();
    let row: Vec<Solid> = (0..5)
        .map(|k| {
            let copy = Instance {
                feature: 2,
                index: u64::from(k),
            };
            transformed(
                &cube,
                &Motion::pattern_step(DVec3::X, 1.0, k).unwrap(),
                Some(copy),
            )
        })
        .collect();
    let joined = assemble(&row, &TOL, &Budget::DEFAULT).unwrap();
    assert!(close(joined.volume(), 5.0, 1e-12));
    assert!(close(joined.area(), 22.0, 1e-12));
    assert_eq!(Topology::new(&joined).regions().len(), 6);
    // Two touching pairs and one apart: two shells.
    let parts = [
        row[0].clone(),
        row[1].clone(),
        transformed(&row[0], &Motion::translation(DVec3::Y * 3.0).unwrap(), None),
    ];
    let some = assemble(&parts, &TOL, &Budget::DEFAULT).unwrap();
    assert!(close(some.volume(), 3.0, 1e-12));
    assert_eq!(Topology::new(&some).regions().len(), 12);
}

#[test]
fn a_part_inside_another_is_unioned() {
    // Hulls apart, but the small cube is inside the big one: side by side
    // they wouldn't be a solid.
    let big = Solid::cuboid(DVec3::ZERO, DVec3::splat(4.0), 1, &TOL).unwrap();
    let small = Solid::cuboid(DVec3::splat(1.0), DVec3::splat(1.0), 2, &TOL).unwrap();
    let both = assemble(&[big.clone(), small], &TOL, &Budget::DEFAULT).unwrap();
    assert!(close(both.volume(), 64.0, 1e-12));
    // A circle of four pins turned about z by quarter turns, apart.
    let pin = Solid::cylinder(DVec3::new(3.0, 0.0, 0.0), 0.5, 2.0, 1, &TOL).unwrap();
    let ring: Vec<Solid> = (0..4)
        .map(|k| {
            let turn = Motion::pattern_turn(DVec3::ZERO, DVec3::Z, 360.0, k, 4).unwrap();
            transformed(&pin, &turn, None)
        })
        .collect();
    // Quarter turns: the third pin's base centre is (−3, 0, 0) exactly.
    assert!(ring[2].mesh().verts().contains(&DVec3::new(-2.5, 0.0, 0.0)));
    let all = assemble(&ring, &TOL, &Budget::DEFAULT).unwrap();
    assert!(close(all.volume(), 4.0 * pin.volume(), 1e-12));
}

#[test]
fn a_part_in_a_void_closed_by_other_parts_is_put_beside_them() {
    // Six plates, flush along their edges, close a box of 4 round a void
    // of 2; a cube of 1 floats in the void, in no plate's box, its hulls
    // clear of theirs. The plates are unioned into a hollow box and the
    // cube put beside it: a shell in the void faces out, which is right,
    // and the check confirms it.
    let plate = |min: [f64; 3], size: [f64; 3], feature: u64| {
        Solid::cuboid(DVec3::from(min), DVec3::from(size), feature, &TOL).unwrap()
    };
    let cube = plate([1.5; 3], [1.0; 3], 7);
    let parts = [
        plate([0.0, 0.0, 0.0], [4.0, 4.0, 1.0], 1),
        cube.clone(),
        plate([0.0, 0.0, 3.0], [4.0, 4.0, 1.0], 2),
        plate([0.0, 0.0, 1.0], [1.0, 4.0, 2.0], 3),
        plate([3.0, 0.0, 1.0], [1.0, 4.0, 2.0], 4),
        plate([1.0, 0.0, 1.0], [2.0, 1.0, 2.0], 5),
        plate([1.0, 3.0, 1.0], [2.0, 1.0, 2.0], 6),
    ];
    let all = assemble(&parts, &TOL, &Budget::DEFAULT).unwrap();
    assert!(
        close(all.volume(), 64.0 - 8.0 + 1.0, 1e-12),
        "{}",
        all.volume()
    );
    // The cube is there as it was, beside the hollow box's two shells.
    let cube_tris = cube.mesh().tris().len();
    let tris = all.mesh().tris().len();
    assert_eq!(
        &all.mesh().verts()[all.mesh().verts().len() - cube.mesh().verts().len()..],
        cube.mesh().verts()
    );
    assert!(tris > cube_tris);
    // The cube moved into a wall: unioned with it, not beside it.
    let parts = [parts[3].clone(), plate([0.5, 1.5, 1.5], [1.0; 3], 7)];
    let joined = assemble(&parts, &TOL, &Budget::DEFAULT).unwrap();
    assert!(close(joined.volume(), 8.0 + 0.5, 1e-12));
}

#[test]
fn spokes_overlapping_at_the_hub_are_unioned() {
    // Six bars through the axis, turned by 30° each: all overlap about
    // the axis, so all are unioned.
    let bar = Solid::cuboid(
        DVec3::new(-3.0, -0.25, 0.0),
        DVec3::new(6.0, 0.5, 1.0),
        1,
        &TOL,
    )
    .unwrap();
    let spokes: Vec<Solid> = (0..6)
        .map(|k| {
            let turn = Motion::pattern_turn(DVec3::ZERO, DVec3::Z, 180.0, k, 6).unwrap();
            let copy = Instance {
                feature: 3,
                index: u64::from(k),
            };
            transformed(&bar, &turn, Some(copy))
        })
        .collect();
    let star = assemble(&spokes, &TOL, &Budget::DEFAULT).unwrap();
    // As the unions one after another make it.
    let mut chained = spokes[0].clone();
    for s in &spokes[1..] {
        chained = crate::boolean(&chained, s, Op::Union, &TOL, &Budget::DEFAULT).unwrap();
    }
    assert!(close(star.volume(), chained.volume(), 1e-9));
    assert!(star.volume() < 6.0 * bar.volume());
}

#[test]
fn points_past_the_bounds_are_refused() {
    let cube = Solid::cuboid(DVec3::ZERO, DVec3::splat(1.0), 1, &TOL).unwrap();
    let far = Motion::translation(DVec3::new(f64::from(crate::MAX_COORD), 0.0, 0.0)).unwrap();
    assert!(matches!(
        cube.transformed(&far, None, &TOL, &Budget::DEFAULT)
            .stripped(),
        Err(KernelError::Patch(PatchError::Coordinate(_)))
    ));
    // Within: up to the bound.
    let near =
        Motion::translation(DVec3::new(f64::from(crate::MAX_COORD) - 1.0, 0.0, 0.0)).unwrap();
    assert!(
        cube.transformed(&near, None, &TOL, &Budget::DEFAULT)
            .is_ok()
    );
    // A turn about a far point swings the solid out.
    let swing = Motion::turn(DVec3::new(9e5, 0.0, 0.0), DVec3::Z, 180.0).unwrap();
    assert!(
        cube.transformed(&swing, None, &TOL, &Budget::DEFAULT)
            .is_err()
    );
    // Motions that aren't finite aren't made.
    assert_eq!(Motion::translation(DVec3::new(f64::NAN, 0.0, 0.0)), None);
    assert_eq!(Motion::translation(DVec3::splat(f64::INFINITY)), None);
    assert_eq!(Motion::turn(DVec3::ZERO, DVec3::ZERO, 10.0), None);
    assert_eq!(Motion::turn(DVec3::ZERO, DVec3::Z, f64::NAN), None);
    assert_eq!(Motion::turn(DVec3::ZERO, DVec3::Z, f64::INFINITY), None);
    assert_eq!(Motion::turn(DVec3::splat(f64::NAN), DVec3::Z, 1.0), None);
    assert_eq!(Motion::turn(DVec3::ZERO, DVec3::splat(f64::MAX), 1.0), None);
    assert_eq!(Motion::mirror(DVec3::ZERO, DVec3::ZERO), None);
    assert_eq!(
        Motion::mirror(DVec3::ZERO, DVec3::splat(f64::INFINITY)),
        None
    );
    assert_eq!(Motion::mirror(DVec3::splat(f64::NAN), DVec3::Z), None);
    assert_eq!(Motion::pattern_step(DVec3::X, f64::MAX, u32::MAX), None);
    assert_eq!(
        Motion::pattern_turn(DVec3::ZERO, DVec3::Z, 360.0, 1, 0),
        None
    );
    // A huge pattern step is a motion, and its copies are refused.
    let step = Motion::pattern_step(DVec3::X, 1e3, u32::MAX).unwrap();
    assert!(
        cube.transformed(&step, None, &TOL, &Budget::DEFAULT)
            .is_err()
    );
}

#[test]
fn work_is_budgeted() {
    let (solid, _) = part();
    let turn = Motion::turn(DVec3::ZERO, DVec3::Z, 33.0).unwrap();
    assert_eq!(
        solid
            .transformed(&turn, None, &TOL, &Budget::new(10))
            .stripped(),
        Err(KernelError::TooComplex)
    );
}

#[test]
fn transforms_and_assembly_are_deterministic() {
    let (solid, _) = part();
    assert_deterministic(|| {
        let copies: Vec<Solid> = (0..5)
            .map(|k| {
                let turn = Motion::pattern_turn(
                    DVec3::new(2.0, 2.0, 0.0),
                    DVec3::new(0.1, 0.2, 1.0),
                    360.0,
                    k,
                    5,
                )
                .unwrap()
                .then(&Motion::mirror(DVec3::new(0.0, 0.0, -1.0), DVec3::Z).unwrap());
                transformed(
                    &solid,
                    &turn,
                    Some(Instance {
                        feature: 1,
                        index: u64::from(k),
                    }),
                )
            })
            .collect();
        assemble(&copies, &TOL, &Budget::DEFAULT).unwrap()
    });
}

#[test]
fn a_move_of_an_empty_solid_is_empty() {
    let empty = Solid::empty();
    let moved = transformed(&empty, &Motion::translation(DVec3::X).unwrap(), None);
    assert!(moved.is_empty());
    assert!(assemble(&[], &TOL, &Budget::DEFAULT).unwrap().is_empty());
    assert!(
        assemble(&[empty.clone(), empty], &TOL, &Budget::DEFAULT)
            .unwrap()
            .is_empty()
    );
    let cube = Solid::cuboid(DVec3::ZERO, DVec3::splat(1.0), 1, &TOL).unwrap();
    assert_eq!(
        assemble(&[Solid::empty(), cube.clone()], &TOL, &Budget::DEFAULT).unwrap(),
        cube
    );
}

#[test]
fn side_names_survive_a_turn() {
    let (solid, _) = part();
    let turned = transformed(
        &solid,
        &Motion::turn(DVec3::ZERO, DVec3::X, 45.0).unwrap(),
        None,
    );
    let topology = Topology::new(&turned);
    let key = FaceName::new(
        9,
        FacePart::Side {
            curve: 20,
            segment: 0,
        },
    )
    .key();
    assert!(topology.face(&turned, &key, DVec3::ZERO).is_ok());
}

mod scale;

/// A union's failure inside [`assemble`] names faces of that union's
/// operands, not of `assemble`'s (it has none), so they're left out and
/// marked; the rest of the evidence and the error stay.
#[test]
fn assemble_drops_its_unions_operand_faces() {
    use crate::mesh::{FaceKey, PartKey};
    use crate::{Evidence, KernelError, Operand};

    let key = FaceKey {
        feature: 1,
        part: PartKey::StartCap,
        instance: 0,
    };
    let mut evidence = Evidence::default();
    evidence.add_points([DVec3::ONE]);
    evidence.add_faces([(Operand::A, key), (Operand::B, key)]);
    let failure = Failure {
        error: KernelError::TooComplex,
        evidence: Box::new(evidence),
    };
    let kept = of_parts(failure);
    assert_eq!(kept.error, KernelError::TooComplex);
    assert!(kept.evidence.faces.is_empty());
    assert_eq!(kept.evidence.points, [DVec3::ONE]);
    assert!(kept.evidence.truncated);
    // Without faces it's as it was.
    let bare = Failure::from(KernelError::TooComplex);
    assert_eq!(of_parts(bare.clone()), bare);
}

/// Equal motions have equal bits, however they're built; any change of
/// a number or of whether it mirrors changes them.
#[test]
fn bits_tell_motions_apart() {
    let shift = |x: f64| Motion::translation(DVec3::new(x, 0.0, 0.0)).unwrap();
    assert_eq!(shift(1.0).bits(), shift(1.0).bits());
    assert_eq!(Motion::IDENTITY.then(&shift(2.0)).bits(), shift(2.0).bits());
    assert_ne!(shift(1.0).bits(), shift(1.0 + f64::EPSILON).bits());
    let turn = Motion::turn(DVec3::ZERO, DVec3::Z, 90.0).unwrap();
    let mirror = Motion::mirror(DVec3::ZERO, DVec3::X).unwrap();
    assert_ne!(turn.bits(), Motion::IDENTITY.bits());
    assert_ne!(turn.bits(), mirror.bits());
    assert_eq!(mirror.bits()[22], 1);
    assert_eq!(turn.bits()[22], 0);
    assert_eq!(turn.then(&mirror).bits()[22], 1);
}
